//! The terrain's vertex colours (port of `TerrainColorizer` in
//! `src/world/TerrainMesh.js`; roadmap WP 3.4): each zone's landform
//! paints its ground (alpine meadow and rock, farm parcels, city lots,
//! coastal scrub, sand, port yards, sandstone strata, playa, the raceway's
//! aerial photo), blended across zone edges by x, then shorelines.
//!
//! The JS colouriser keeps `this.paved` (how much of the spot is paved)
//! from its last `color()` call for the mesh to read; here [`Colorizer::color`]
//! returns it with the colour, so the colouriser holds no state and the
//! mesh tiles can be coloured on several threads.

use mr_math::{clamp, fbm, hash2, js, kernel, smoothstep};

use crate::color::Color;
use crate::terrain::{Form, Terrain};

type Rgb = [f64; 3];

fn lin(hex: u32) -> Rgb {
    Color::hex(hex).to_array()
}

fn mix3(out: &mut Rgb, a: Rgb, b: Rgb, t: f64) {
    out[0] = a[0] + (b[0] - a[0]) * t;
    out[1] = a[1] + (b[1] - a[1]) * t;
    out[2] = a[2] + (b[2] - a[2]) * t;
}

/// `mix3(c, c, b, t)`: mix toward `b` in place.
fn mix_to(c: &mut Rgb, b: Rgb, t: f64) {
    let a = *c;
    mix3(c, a, b, t);
}

/// The palette `P` (linear, as `new THREE.Color(hex)` gives it).
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub gravel: Rgb,
    pub alpine: Rgb,
    pub alpine_dry: Rgb,
    pub forest: Rgb,
    pub rock: Rgb,
    pub rock_dark: Rgb,
    pub snow: Rgb,
    pub grass: Rgb,
    pub grass_dry: Rgb,
    pub wheat: Rgb,
    pub plowed: Rgb,
    pub crop: Rgb,
    pub hay: Rgb,
    pub lavender: Rgb,
    pub city: Rgb,
    pub city_park: Rgb,
    pub dirt: Rgb,
    pub sandstone: Rgb,
    pub cliff_grey: Rgb,
    pub coast_grass: Rgb,
    pub scrub: Rgb,
    pub sand: Rgb,
    pub wet_sand: Rgb,
    pub seabed: Rgb,
    pub pavement: Rgb,
    pub quay: Rgb,
    pub asphalt_lot: Rgb,
    pub red_rock: Rgb,
    pub red_rock_dark: Rgb,
    pub desert_sand: Rgb,
    pub desert_scrub: Rgb,
    pub playa: Rgb,
    pub red_sand: Rgb,
    pub pavement_dark: Rgb,
    pub playa_dark: Rgb,
    pub range_blue: Rgb,
    // Enrichment: patches that break up each landform's base colour.
    pub heather: Rgb,
    pub lichen: Rgb,
    pub meadow: Rgb,
    pub lush: Rgb,
    pub ice_plant: Rgb,
    pub varnish: Rgb,
    pub concrete: Rgb,
    pub yard_gravel: Rgb,
    pub rust: Rgb,
    /// Sandstone layers, bottom to top of each 9 m band set (`STRATA`).
    pub strata: [Rgb; 8],
    /// `FIELD_COLS`.
    pub field_cols: [Rgb; 8],
}

impl Palette {
    pub fn new() -> Palette {
        let wheat = lin(0xc4a452);
        let plowed = lin(0x6e5439);
        let crop = lin(0x4d8a2c);
        let hay = lin(0x9e9a52);
        let grass_dry = lin(0x88914a);
        let lavender = lin(0x8a8a5c);
        Palette {
            gravel: lin(0x7a7266),
            alpine: lin(0x5b6a3c),
            alpine_dry: lin(0x7d7a4c),
            forest: lin(0x2f4526),
            rock: lin(0x77726b),
            rock_dark: lin(0x55504b),
            snow: lin(0xe9eef2),
            grass: lin(0x5e8a33),
            grass_dry,
            wheat,
            plowed,
            crop,
            hay,
            lavender,
            city: lin(0x4c4f4a),
            city_park: lin(0x3f5f2e),
            dirt: lin(0x7b6448),
            sandstone: lin(0x9c8468),
            cliff_grey: lin(0x7c7670),
            coast_grass: lin(0x7f8a4a),
            scrub: lin(0x4c5c34),
            sand: lin(0xd9c69a),
            wet_sand: lin(0xa89572),
            seabed: lin(0x4f6258),
            pavement: lin(0x86857f),
            quay: lin(0x6e6f71),
            asphalt_lot: lin(0x46474b),
            red_rock: lin(0xa0573a),
            red_rock_dark: lin(0x6e3a28),
            desert_sand: lin(0xc9a878),
            desert_scrub: lin(0x8a8458),
            playa: lin(0xf0eadc),
            red_sand: lin(0xb97a52),
            pavement_dark: lin(0x7e6a54),
            playa_dark: lin(0xd4ccbc),
            range_blue: lin(0x6c6a78),
            heather: lin(0x6a5c44),
            lichen: lin(0x8c8a6e),
            meadow: lin(0x7d9a3a),
            lush: lin(0x46702a),
            ice_plant: lin(0x7a6a3c),
            varnish: lin(0x5a3a2c),
            concrete: lin(0x7c7b76),
            yard_gravel: lin(0x6f685e),
            rust: lin(0x7a5a44),
            strata: [
                lin(0xa4553a),
                lin(0xb86a44),
                lin(0x8a3f2c),
                lin(0xc88a5c),
                lin(0xa04a34),
                lin(0xd4a47a),
                lin(0x94503a),
                lin(0xb4603e),
            ],
            field_cols: [wheat, plowed, crop, hay, grass_dry, crop, wheat, lavender],
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Palette::new()
    }
}

/// The per-zone context (`ctx`).
#[derive(Clone, Copy, Debug)]
struct Ctx {
    x: f64,
    y: f64,
    z: f64,
    d: f64,
    n1: f64,
    slope: f64,
    pv: f64,
}

/// `TerrainColorizer`.
pub struct Colorizer<'a> {
    pub t: &'a Terrain,
    pub p: Palette,
}

impl<'a> Colorizer<'a> {
    pub fn new(terrain: &'a Terrain) -> Colorizer<'a> {
        Colorizer {
            t: terrain,
            p: Palette::new(),
        }
    }

    /// `this.noise` (the terrain's `noise2`).
    pub fn noise(&self, x: f64, y: f64) -> f64 {
        self.t.noise2.noise(x, y)
    }

    fn fbm(&self, x: f64, y: f64, octaves: u32) -> f64 {
        fbm(&self.t.noise2, x, y, octaves)
    }

    /// Valley field patchwork: a rotated grid of parcels, each its own crop.
    pub fn field_color(&self, x: f64, z: f64, out: &mut Rgb) -> f64 {
        let a: f64 = 0.38; // grid rotation
        let u = x * kernel::cos(a) - z * kernel::sin(a);
        let v = x * kernel::sin(a) + z * kernel::cos(a);
        let fu = (u / 110.0).floor();
        let fv = (v / 160.0).floor();
        let h = hash2(fu, fv, 3.0);
        let col = self.p.field_cols[(h * self.p.field_cols.len() as f64).floor() as usize];
        // Furrows: stripes inside the parcel.
        let stripe =
            kernel::sin(((if h > 0.5 { u } else { v }) / 2.2) * std::f64::consts::PI) * 0.5 + 0.5;
        let edge = js::min_n(&[
            u / 110.0 - fu,
            1.0 - (u / 110.0 - fu),
            v / 160.0 - fv,
            1.0 - (v / 160.0 - fv),
        ]);
        out[0] = col[0] * (0.9 + stripe * 0.12);
        out[1] = col[1] * (0.9 + stripe * 0.12);
        out[2] = col[2] * (0.9 + stripe * 0.12);
        if edge < 0.025 {
            mix_to(out, self.p.grass, 0.8); // hedgerow / grass margin
        }
        h
    }

    /// `color(x, y, z, ny, out)`: the colour, and `this.paved` after it.
    pub fn color(&self, x: f64, y: f64, z: f64, ny: f64) -> (Rgb, f64) {
        let t = self.t;
        let w = t.zone_weights(x);
        let info = t.road_info(x, z);
        let mut ctx = Ctx {
            x,
            y,
            z,
            d: info.d,
            n1: self.fbm(x / 160.0, z / 160.0, 3),
            slope: 1.0 - ny,
            pv: 0.0,
        };
        let mut c = [0.0; 3];
        let mut out = [0.0; 3];
        // How much of this spot is paved (concrete/asphalt yards, pavements):
        // forms set ctx.pv; the ground shader draws slab joints there instead
        // of grass/soil detail.
        let mut paved = 0.0;
        for k in 0..w.n {
            let wk = w.w[k];
            if wk <= 0.001 {
                continue;
            }
            ctx.pv = 0.0;
            self.form(t.forms[k], &mut c, &mut ctx);
            out[0] += c[0] * wk;
            out[1] += c[1] * wk;
            out[2] += c[2] * wk;
            paved += ctx.pv * wk;
        }
        // Shorelines: wet sand at the waterline, seabed below it.
        if let Some(sea) = t.sea_y {
            let sandy = (1.0 - smoothstep(sea + 1.2, sea + 3.5, y + ctx.n1 * 1.5))
                * (1.0 - smoothstep(0.35, 0.6, ctx.slope));
            mix_to(&mut out, self.p.sand, sandy * 0.9);
            mix_to(
                &mut out,
                self.p.wet_sand,
                (1.0 - smoothstep(sea - 0.3, sea + 0.8, y)) * 0.8,
            );
            mix_to(
                &mut out,
                self.p.seabed,
                1.0 - smoothstep(sea - 4.0, sea - 1.2, y),
            );
        }
        (out, paved)
    }

    fn form(&self, f: Form, c: &mut Rgb, ctx: &mut Ctx) {
        match f {
            Form::Mountain => self.mountain(c, ctx),
            Form::Valley => self.valley(c, ctx),
            Form::City => self.city(c, ctx),
            Form::Coast => self.coast(c, ctx),
            Form::Beach => self.beach(c, ctx),
            Form::Harbor => self.harbor(c, ctx),
            Form::Streets => self.streets(c, ctx),
            Form::Canyon => self.canyon(c, ctx),
            Form::Desert => self.desert(c, ctx),
            Form::Playa => self.playa(c, ctx),
            Form::Raceway => self.raceway(c, ctx),
        }
    }

    fn mountain(&self, c: &mut Rgb, ctx: &Ctx) {
        let (
            p,
            &Ctx {
                x,
                y,
                z,
                d,
                n1,
                slope,
                ..
            },
        ) = (&self.p, ctx);
        let rock = smoothstep(0.22, 0.42, slope + n1 * 0.08);
        mix3(c, p.alpine, p.alpine_dry, clamp(0.5 + n1, 0.0, 1.0));
        let forest = smoothstep(0.1, 0.35, self.fbm(x / 420.0 + 7.0, z / 420.0, 3))
            * (1.0 - smoothstep(300.0, 380.0, y));
        mix_to(c, p.forest, forest * 0.8);
        // Heather and dry patches on the open slopes, lichen on the rock.
        mix_to(
            c,
            p.heather,
            smoothstep(0.15, 0.45, self.fbm(x / 90.0 + 2.0, z / 90.0 - 5.0, 2))
                * 0.45
                * (1.0 - forest),
        );
        let mut rock_col = [0.0; 3];
        mix3(
            &mut rock_col,
            p.rock,
            p.rock_dark,
            clamp(0.5 + n1 * 1.5, 0.0, 1.0),
        );
        mix_to(
            &mut rock_col,
            p.lichen,
            smoothstep(0.1, 0.5, self.fbm(x / 45.0 - 3.0, z / 45.0, 2)) * 0.35,
        );
        mix_to(c, rock_col, rock);
        let snow = smoothstep(470.0, 560.0, y + n1 * 80.0) * (1.0 - smoothstep(0.45, 0.7, slope));
        mix_to(c, p.snow, snow);
        if d < 30.0 {
            mix_to(c, p.gravel, (1.0 - smoothstep(8.0, 22.0, d)) * 0.85);
        }
    }

    fn valley(&self, c: &mut Rgb, ctx: &Ctx) {
        let (
            p,
            &Ctx {
                x, z, d, n1, slope, ..
            },
        ) = (&self.p, ctx);
        mix3(c, p.grass, p.grass_dry, clamp(0.45 + n1 * 1.2, 0.0, 1.0));
        // Lusher grass in hollows, meadow flowers in drifts.
        mix_to(
            c,
            p.lush,
            smoothstep(0.1, 0.4, self.fbm(x / 130.0 - 6.0, z / 130.0, 2)) * 0.5,
        );
        mix_to(
            c,
            p.meadow,
            smoothstep(0.3, 0.6, self.fbm(x / 40.0 + 9.0, z / 40.0, 2)) * 0.35,
        );
        let field_ok = smoothstep(14.0, 26.0, d)
            * (1.0 - smoothstep(700.0, 1000.0, d))
            * (1.0 - smoothstep(0.06, 0.14, slope));
        if field_ok > 0.0 {
            let mut f = [0.0; 3];
            self.field_color(x, z, &mut f);
            mix_to(c, f, field_ok);
        }
        let hill_forest = smoothstep(0.05, 0.3, self.fbm(x / 300.0 + 3.0, z / 300.0, 3))
            * smoothstep(500.0, 900.0, d);
        mix_to(c, p.forest, hill_forest * 0.85);
        mix_to(c, p.rock, smoothstep(0.35, 0.6, slope));
        if d < 16.0 {
            mix_to(c, p.dirt, (1.0 - smoothstep(7.0, 12.0, d)) * 0.6);
        }
    }

    fn city(&self, c: &mut Rgb, ctx: &mut Ctx) {
        let (p, Ctx { x, z, d, slope, .. }) = (&self.p, *ctx);
        let park = smoothstep(0.2, 0.35, self.fbm(x / 200.0 + 1.0, z / 200.0, 2));
        mix3(c, p.city, p.city_park, park);
        ctx.pv = (1.0 - park)
            * (1.0 - smoothstep(1200.0, 2000.0, d))
            * (1.0 - smoothstep(0.2, 0.4, slope));
        mix_to(c, p.forest, smoothstep(1200.0, 2000.0, d) * 0.8);
        mix_to(c, p.rock, smoothstep(0.4, 0.65, slope));
    }

    fn coast(&self, c: &mut Rgb, ctx: &Ctx) {
        let (
            p,
            &Ctx {
                x, z, d, n1, slope, ..
            },
        ) = (&self.p, ctx);
        // Dry coastal grass and scrub on top, banded sandstone/grey cliffs.
        mix3(
            c,
            p.coast_grass,
            p.scrub,
            smoothstep(-0.1, 0.35, self.fbm(x / 240.0 + 5.0, z / 240.0, 3)),
        );
        // Ice plant mats and bare earth scattered through the grass.
        mix_to(
            c,
            p.ice_plant,
            smoothstep(0.25, 0.55, self.fbm(x / 55.0 - 1.0, z / 55.0 + 4.0, 2)) * 0.45,
        );
        mix_to(
            c,
            p.dirt,
            smoothstep(0.35, 0.6, self.fbm(x / 30.0 + 3.0, z / 30.0 - 2.0, 2)) * 0.3,
        );
        let rock = smoothstep(0.25, 0.45, slope + n1 * 0.1);
        let mut rock_col = [0.0; 3];
        mix3(
            &mut rock_col,
            p.sandstone,
            p.cliff_grey,
            clamp(0.5 + n1 * 1.6, 0.0, 1.0),
        );
        mix_to(c, rock_col, rock);
        if d < 26.0 {
            mix_to(c, p.gravel, (1.0 - smoothstep(8.0, 20.0, d)) * 0.8);
        }
    }

    fn beach(&self, c: &mut Rgb, ctx: &mut Ctx) {
        let (
            p,
            Ctx {
                x, z, d, n1, slope, ..
            },
        ) = (&self.p, *ctx);
        // Town side: pavements, lots and gardens near the road, giving way to
        // dry grass and scrub on slopes and further inland. Sea side: sand.
        let garden = smoothstep(0.15, 0.4, self.fbm(x / 150.0 + 2.0, z / 150.0, 2));
        mix3(c, p.pavement, p.grass, garden * 0.7);
        let wild = js::max(smoothstep(330.0, 470.0, d), smoothstep(0.08, 0.2, slope));
        let mut scrub_col = [0.0; 3];
        mix3(
            &mut scrub_col,
            p.coast_grass,
            p.scrub,
            clamp(0.5 + n1 * 1.4, 0.0, 1.0),
        );
        mix_to(c, scrub_col, wild);
        let sand = smoothstep(10.0, 26.0, d) * self.t.sea_side(&self.t.far(x, z));
        mix_to(c, p.sand, sand);
        ctx.pv = (1.0 - garden * 0.7) * (1.0 - wild) * (1.0 - sand);
        mix_to(c, p.forest, smoothstep(700.0, 1300.0, d) * 0.7);
        mix_to(c, p.rock, smoothstep(0.4, 0.65, slope));
    }

    /// Downtown Streets: paved lots and yards between buildings, parks and
    /// wooded hills far off.
    fn streets(&self, c: &mut Rgb, ctx: &mut Ctx) {
        let (p, Ctx { x, z, d, slope, .. }) = (&self.p, *ctx);
        let lot = smoothstep(0.0, 0.3, self.fbm(x / 90.0 + 4.0, z / 90.0, 2));
        mix3(c, p.pavement, p.asphalt_lot, lot * 0.8);
        let park = smoothstep(0.25, 0.4, self.fbm(x / 260.0 + 1.0, z / 260.0, 2))
            * smoothstep(60.0, 200.0, d)
            * 0.8;
        mix_to(c, p.city_park, park);
        ctx.pv = (1.0 - park)
            * (1.0 - smoothstep(900.0, 1700.0, d))
            * (1.0 - smoothstep(0.2, 0.45, slope));
        mix_to(c, p.forest, smoothstep(900.0, 1700.0, d) * 0.8);
        mix_to(c, p.rock, smoothstep(0.45, 0.7, slope));
    }

    /// Seaside Raceway: the aerial photo's own colour (level.groundColor,
    /// sRGB 0..1). An overhead summer photo is pale and hazy, so it's graded a
    /// little darker and richer for the game's sunlight. Grey, unsaturated
    /// ground (the paddock, car parks, service roads) is paved.
    fn raceway(&self, c: &mut Rgb, ctx: &mut Ctx) {
        let g = (self
            .t
            .ground_color
            .as_ref()
            .expect("the raceway landform needs level.groundColor"))(ctx.x, ctx.z);
        let lin = |v: f64| kernel::pow(v, 2.2);
        // Warmer (the photo's haze is blue) and richer.
        let r = lin(g[0]) * 1.04;
        let gg = lin(g[1]);
        let b = lin(g[2]) * 0.82;
        let l = (r + gg + b) / 3.0;
        let (sat, k) = (1.55, 0.74);
        c[0] = js::max(0.0, l + (r - l) * sat) * k;
        c[1] = js::max(0.0, l + (gg - l) * sat) * k;
        c[2] = js::max(0.0, l + (b - l) * sat) * k;
        // Paved yards and lots get slab joints; not the run-off inside the
        // barriers (that's the scenery's asphalt or bare graded dirt).
        let chroma = js::max_n(&g) - js::min_n(&g);
        let grey = (1.0 - smoothstep(0.035, 0.08, chroma))
            * smoothstep(0.4, 0.55, (g[0] + g[1] + g[2]) / 3.0);
        ctx.pv = grey * smoothstep(36.0, 50.0, ctx.d) * 0.6;
    }

    // ── Desert Run (Level 4) ───────────────────────────────────────
    /// Banded sandstone on the steep faces, red sand on the floor and ledges.
    fn strata_color(&self, out: &mut Rgb, y: f64, n1: f64) {
        let s = &self.p.strata;
        let len = s.len() as f64;
        let k = ((y + n1 * 5.0) / 4.5).floor();
        let a = s[(((k % len) + len) % len) as usize];
        let b = s[((((k + 1.0) % len) + len) % len) as usize];
        mix3(
            out,
            a,
            b,
            clamp(((y + n1 * 5.0) / 4.5 - k) * 1.5 - 0.9, 0.0, 1.0),
        );
    }

    fn canyon(&self, c: &mut Rgb, ctx: &Ctx) {
        let (
            p,
            &Ctx {
                x,
                y,
                z,
                n1,
                slope,
                d,
                ..
            },
        ) = (&self.p, ctx);
        mix3(
            c,
            p.red_sand,
            p.desert_sand,
            clamp(0.35 + n1 * 1.4, 0.0, 1.0) * 0.55,
        );
        mix_to(
            c,
            p.desert_scrub,
            smoothstep(0.1, 0.35, self.fbm(x / 70.0 + 3.0, z / 70.0, 2))
                * 0.25
                * (1.0 - smoothstep(0.1, 0.25, slope)),
        );
        let rock = smoothstep(0.18, 0.4, slope + n1 * 0.08);
        // Desert varnish: dark streaks down the faces.
        let mut strata = [0.0; 3];
        self.strata_color(&mut strata, y, n1);
        mix_to(
            &mut strata,
            p.varnish,
            smoothstep(0.2, 0.6, self.fbm(x / 14.0, z / 14.0 + y / 60.0, 2)) * 0.4,
        );
        mix_to(c, strata, rock);
        if d < 24.0 {
            mix_to(c, p.gravel, (1.0 - smoothstep(7.0, 18.0, d)) * 0.55);
        }
    }

    fn desert(&self, c: &mut Rgb, ctx: &Ctx) {
        let (
            p,
            &Ctx {
                x,
                y,
                z,
                n1,
                slope,
                d,
                ..
            },
        ) = (&self.p, ctx);
        // Pale sand, a dark varnished gravel "pavement" in patches, scrub.
        mix3(
            c,
            p.desert_sand,
            p.pavement_dark,
            smoothstep(0.0, 0.4, self.fbm(x / 220.0 + 7.0, z / 220.0, 3)) * 0.55,
        );
        mix_to(
            c,
            p.desert_scrub,
            smoothstep(-0.1, 0.3, self.fbm(x / 60.0 + 1.0, z / 60.0, 2)) * 0.3,
        );
        let mut strata = [0.0; 3];
        self.strata_color(&mut strata, y, n1);
        mix_to(c, strata, smoothstep(0.25, 0.45, slope));
        mix_to(
            c,
            p.range_blue,
            smoothstep(1800.0, 3200.0, d) * 0.35 * smoothstep(0.15, 0.4, slope),
        );
        if d < 24.0 {
            mix_to(c, p.gravel, (1.0 - smoothstep(7.0, 18.0, d)) * 0.5);
        }
    }

    fn playa(&self, c: &mut Rgb, ctx: &Ctx) {
        let (p, &Ctx { x, y, z, slope, .. }) = (&self.p, ctx);
        mix3(
            c,
            p.playa,
            p.playa_dark,
            clamp(0.5 + self.fbm(x / 90.0, z / 90.0, 3) * 1.2, 0.0, 1.0) * 0.6,
        );
        // Wet-season stains, long and faint.
        mix_to(
            c,
            p.playa_dark,
            smoothstep(0.3, 0.5, self.fbm(x / 400.0 + 2.0, z / 40.0, 2)) * 0.25,
        );
        // The shore: sand and scrub on the fans, rock on the ranges.
        let above = y - (self.t.far(x, z).y - 0.35);
        mix_to(c, p.desert_sand, smoothstep(0.4, 3.0, above));
        mix_to(c, p.desert_scrub, smoothstep(3.0, 12.0, above) * 0.3);
        mix_to(c, p.red_rock_dark, smoothstep(0.3, 0.55, slope) * 0.7);
    }

    /// Port land: a grid of yards along the quay — pale concrete aprons, dark
    /// asphalt container stacks, gravel rail yards — with lighter haul roads
    /// between them and rust/oil stains, instead of one grey sheet.
    fn harbor(&self, c: &mut Rgb, ctx: &mut Ctx) {
        let (p, Ctx { x, z, d, slope, .. }) = (&self.p, *ctx);
        let a: f64 = 0.21;
        let u = x * kernel::cos(a) - z * kernel::sin(a);
        let v = x * kernel::sin(a) + z * kernel::cos(a);
        let fu = (u / 95.0).floor();
        let fv = (v / 62.0).floor();
        let h = hash2(fu, fv, 9.0);
        let yard = if h < 0.38 {
            p.concrete
        } else if h < 0.7 {
            p.asphalt_lot
        } else if h < 0.85 {
            p.yard_gravel
        } else {
            p.quay
        };
        let edge = js::min_n(&[
            u / 95.0 - fu,
            1.0 - (u / 95.0 - fu),
            v / 62.0 - fv,
            1.0 - (v / 62.0 - fv),
        ]);
        mix3(
            c,
            yard,
            p.concrete,
            (1.0 - smoothstep(0.03, 0.06, edge)) * 0.8,
        );
        let stain = smoothstep(0.2, 0.55, self.fbm(x / 35.0 + 9.0, z / 35.0, 2));
        mix_to(
            c,
            if h > 0.85 { p.rust } else { p.asphalt_lot },
            stain * 0.3,
        );
        // Weathering: large faint patches.
        let wear = self.fbm(x / 150.0 + 9.0, z / 150.0, 2);
        c[0] *= 0.92 + wear * 0.16;
        c[1] *= 0.92 + wear * 0.16;
        c[2] *= 0.92 + wear * 0.16;
        let far = smoothstep(1100.0, 1900.0, d);
        mix_to(c, p.forest, far * 0.75);
        let rock = smoothstep(0.4, 0.65, slope);
        mix_to(c, p.rock, rock);
        ctx.pv = (if h < 0.85 { 1.0 } else { 0.3 }) * (1.0 - far) * (1.0 - rock);
    }
}
