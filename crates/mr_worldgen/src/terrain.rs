//! Heightfield terrain sculpted around the road (port of
//! `src/world/Terrain.js`; SPEC 5.4, roadmap WP 3.4).
//!
//! Two helper fields are rasterised from the track first:
//!   far field  (32 m grid, whole map): distance to road, nearest road height,
//!              nearest s and which side — drives the large landforms.
//!   near field (4 m grid, sparse tiles hugging the road): how strongly the
//!              road flattens the ground (K) and the road height to flatten to.
//! Final height = lerp(zone landform, road height, K). Everything is
//! continuous, so switchbacks stacked up a face turn into a smooth slope.
//!
//! The JS keeps a reference to the track; here the methods that read it
//! ([`Terrain::new`], [`Terrain::build_fields`], [`Terrain::is_elevated`])
//! take it as an argument, and the two caches the JS fills lazily from the
//! track (`_open` for `openBias`, `_cw` for `canyonWidth`) are filled by
//! `build_fields`, so [`Terrain::height_at`] needs only `&self` and can run
//! on several threads. Both caches are pure functions of the track and the
//! noise, so filling them earlier changes no value (DECISIONS D231).
//!
//! Every `Float32Array` of the JS is a `Vec<f32>` here, stored through
//! `as f32` (round to nearest, as a typed array stores) and read back as
//! `f64`, so the arithmetic matches the JS bit for bit.

use std::collections::BTreeMap;
use std::sync::Arc;

use mr_math::{Noise2D, clamp, fbm, js, kernel, lerp, ridged, ridged_with, smoothstep};
use mr_track::level::GroundFn;
use mr_track::{Level, RUNOFF_FLAT, Track};

const FAR: f64 = 32.0; // far-field cell size (m)
const NEAR: f64 = 4.0; // near-field node spacing (m)
const NT: i64 = 64; // near-field nodes per tile side
const TILE: f64 = NEAR * NT as f64; // 256 m
/// `TERRAIN_TILE`: the side of a terrain tile (256 m).
pub const TERRAIN_TILE: f64 = TILE;

/// The most zones a level has (the zone weights live in a fixed array).
pub const MAX_ZONES: usize = 8;

/// A level's surveyed ground colour (`level.groundColor(x, z, out)`, sRGB
/// 0..1): Seaside's aerial photo.
pub type ColorFn = Arc<dyn Fn(f64, f64) -> [f64; 3] + Send + Sync>;

/// Per-landform near-road shaping: r0 = flat shoulder past the paved edge,
/// band = distance over which ground returns to the landform, pow < 1 makes
/// the ground climb straight away (rock walls). flat = level ground (city,
/// port) where raised road sections stand on piers instead of embankments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LandformParams {
    pub r0: f64,
    pub band: f64,
    pub pow: f64,
    pub flat: bool,
    /// Seaside Raceway: inside the barriers (the track's wallL/wallR) the
    /// run-off is graded flush with the tarmac, so the car drives on what
    /// you see; outside it's the real land.
    pub corridor: bool,
}

const fn lf(r0: f64, band: f64, pow: f64, flat: bool, corridor: bool) -> LandformParams {
    LandformParams {
        r0,
        band,
        pow,
        flat,
        corridor,
    }
}

/// The landforms a zone can have, in `FORM_FN`'s order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Mountain,
    Valley,
    City,
    Coast,
    Beach,
    Harbor,
    /// Downtown Streets: the level supplies the ground (flat blocks and a
    /// steep hill district); the road only trims it at the kerbs.
    Streets,
    Canyon,
    Desert,
    Playa,
    Raceway,
}

/// `LANDFORMS`, by name, in the JS order.
pub const LANDFORMS: [(&str, Form, LandformParams); 11] = [
    ("mountain", Form::Mountain, lf(2.4, 38.0, 0.6, false, false)),
    ("valley", Form::Valley, lf(2.6, 60.0, 1.0, false, false)),
    ("city", Form::City, lf(1.5, 14.0, 1.0, true, false)),
    ("coast", Form::Coast, lf(2.2, 30.0, 0.7, false, false)),
    ("beach", Form::Beach, lf(3.0, 36.0, 1.0, false, false)),
    ("harbor", Form::Harbor, lf(2.0, 18.0, 1.0, true, false)),
    ("streets", Form::Streets, lf(1.0, 10.0, 1.0, false, false)),
    ("canyon", Form::Canyon, lf(2.2, 20.0, 0.7, false, false)),
    ("desert", Form::Desert, lf(3.0, 40.0, 1.0, false, false)),
    ("playa", Form::Playa, lf(6.0, 60.0, 1.0, false, false)),
    ("raceway", Form::Raceway, lf(0.5, 7.0, 1.0, false, true)),
];

/// A landform by name.
pub fn landform(name: &str) -> Option<(Form, LandformParams)> {
    LANDFORMS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|&(_, f, p)| (f, p))
}

impl Form {
    /// The JS name (`zone.landform`), which the colouriser's methods carry.
    pub fn name(self) -> &'static str {
        LANDFORMS
            .iter()
            .find(|(_, f, _)| *f == self)
            .map(|&(n, _, _)| n)
            .expect("every form is listed")
    }
}

/// A disc flattened to height `y`, or to the landform's own height at its
/// centre when `y` is `None` (`addFlatten`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flatten {
    pub x: f64,
    pub z: f64,
    pub r: f64,
    pub falloff: f64,
    pub y: Option<f64>,
    /// `resolveFlattens()`: the landform at the centre, where `y` is `None`.
    pub y_resolved: Option<f64>,
}

/// A channel carved along a polyline (`addCarve`).
#[derive(Clone, Debug, PartialEq)]
pub struct Carve {
    /// `[{x, z}, ...]`.
    pub points: Vec<[f64; 2]>,
    pub width: f64,
    pub depth: f64,
    /// Applied after the road blend, so the channel passes under the road
    /// deck (a bridge) instead of being filled in by the road.
    pub under_road: bool,
    pub min_x: f64,
    pub max_x: f64,
    pub min_z: f64,
    pub max_z: f64,
}

/// `terrain.desertRail`: Desert's level bed for the railway beside the
/// road (see `form_desert`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesertRail {
    pub lat: f64,
    pub half: f64,
    pub drop: f64,
    pub s0: f64,
    pub s1: f64,
}

/// A zone transition line, by x.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub x: f64,
    pub w: f64,
}

/// A shipping channel: water under a bridge span (harbour).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Channel {
    pub s0: f64,
    pub s1: f64,
}

/// One 256 m tile of near-field nodes (`(NT + 1)²` of each array).
#[derive(Clone, Debug, PartialEq)]
pub struct NearTile {
    pub tx: i64,
    pub tz: i64,
    pub ox: i64,
    pub oz: i64,
    pub k: Vec<f32>,
    pub sw: Vec<f32>,
    pub swh: Vec<f32>,
    pub d: Vec<f32>,
    pub s: Vec<f32>,
}

/// A bilinear far-field sample (`far()`'s `out`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FarSample {
    pub d: f64,
    pub y: f64,
    pub s: f64,
    pub side: f64,
    pub lat: f64,
    pub open: f64,
}

/// `roadInfo()`: road proximity for scenery placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadInfo {
    pub d: f64,
    pub s: f64,
    pub near: bool,
}

/// The zone weights at an x (`zoneWeights`), one per zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneWeights {
    pub w: [f64; MAX_ZONES],
    pub n: usize,
}

impl ZoneWeights {
    pub fn as_slice(&self) -> &[f64] {
        &self.w[..self.n]
    }
}

/// A mesh tile (`tileList()`'s entries).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub x0: f64,
    pub z0: f64,
    pub size: f64,
    pub step: f64,
    pub dmin: f64,
    pub i: i64,
    pub j: i64,
    /// Neighbour step per edge: z0 (j-1), z1 (j+1), x0 (i-1), x1 (i+1).
    pub nsteps: [f64; 4],
}

/// A lookout window (`openBias`'s `_open`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Open {
    s0: f64,
    s1: f64,
    side: f64,
    v: f64,
}

/// `canyonWidth`'s smoothed wall distances (`_cw`).
#[derive(Clone, Debug, PartialEq)]
struct CanyonWidths {
    l: Vec<f32>,
    r: Vec<f32>,
    n: usize,
}

/// `new Terrain(track, level, opts)`'s options.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainOpts {
    pub seed: u32,
    pub margin: f64,
}

impl Default for TerrainOpts {
    fn default() -> Self {
        TerrainOpts {
            seed: 7,
            margin: 2600.0,
        }
    }
}

/// The far field (`fd`, `fy`, `fs`, `fl`, `flat`, `fopen`).
#[derive(Clone, Debug, Default, PartialEq)]
struct FarField {
    fw: usize,
    fh: usize,
    fd: Vec<f32>,
    fy: Vec<f32>,
    fs: Vec<f32>,
    fl: Vec<f32>,
    flat: Vec<f32>,
    fopen: Option<Vec<f32>>,
}

/// A typed-array read the JS way: an index outside the array is
/// `undefined`, which arithmetic turns into NaN.
fn at(a: &[f32], i: f64) -> f64 {
    if i >= 0.0 && i.fract() == 0.0 && (i as usize) < a.len() {
        f64::from(a[i as usize])
    } else {
        f64::NAN
    }
}

pub struct Terrain {
    pub noise: Noise2D,
    pub noise2: Noise2D,
    pub flattens: Vec<Flatten>,
    pub carves: Vec<Carve>,
    pub margin: f64,
    pub min_x: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_z: f64,
    pub sea_y: Option<f64>,
    /// The zones' landforms.
    pub forms: Vec<Form>,
    pub lf: Vec<LandformParams>,
    /// Zones and their transition lines (by x — routes run broadly west→east).
    pub edges: Vec<Edge>,
    /// Level ground for flat landforms: the lowest road not on a structure.
    pub flat_y: Vec<Option<f64>>,
    /// Older scenery reads cityY: the first flat zone's ground.
    pub city_y: f64,
    /// Shipping channels: water under bridge spans (harbour).
    pub channels: Vec<Channel>,
    pub desert_rail: Option<DesertRail>,
    /// `level.ground`.
    pub ground: Option<GroundFn>,
    /// `level.groundColor` (the colouriser's raceway landform).
    pub ground_color: Option<ColorFn>,
    far: FarField,
    near_tiles: Vec<NearTile>,
    /// "tx,tz" → index in `near_tiles` (only looked up, never iterated).
    near_index: BTreeMap<(i64, i64), usize>,
    /// The same as a dense grid over the tiles' range, for `height_at`.
    near_grid: NearGrid,
    open: Option<Vec<Open>>,
    cw: Option<CanyonWidths>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct NearGrid {
    tx0: i64,
    tz0: i64,
    w: i64,
    h: i64,
    /// Index + 1 into `near_tiles`, 0 for none.
    cells: Vec<u32>,
}

impl Terrain {
    /// `new Terrain(track, level, opts)`. Fails on a landform the JS has no
    /// entry for (where the JS would fail in `buildFields`).
    pub fn new(track: &Track, level: &Level, opts: &TerrainOpts) -> Result<Terrain, String> {
        let b = track.bounds;
        let margin = opts.margin;
        let min_x = ((b.min_x - margin) / TILE).floor() * TILE;
        let min_z = ((b.min_z - margin) / TILE).floor() * TILE;
        let max_x = ((b.max_x + margin) / TILE).ceil() * TILE;
        let max_z = ((b.max_z + margin) / TILE).ceil() * TILE;
        let t = track;
        let mut forms = Vec::new();
        let mut lfs = Vec::new();
        for z in &level.zones {
            let (f, p) = landform(z.landform)
                .ok_or_else(|| format!("Terrain: unknown landform '{}'", z.landform))?;
            forms.push(f);
            lfs.push(p);
        }
        if forms.len() > MAX_ZONES {
            return Err(format!(
                "Terrain: {} zones (at most {MAX_ZONES})",
                forms.len()
            ));
        }
        let edges = level
            .zones
            .iter()
            .skip(1)
            .enumerate()
            .map(|(k, z)| Edge {
                x: at(
                    &t.px,
                    t.zone_start.get(k + 1).map_or(f64::NAN, |&s| s as f64),
                ) + js::or_opt(z.blend_offset, 0.0),
                w: js::or_opt(z.blend, 300.0),
            })
            .collect();
        let flat_y: Vec<Option<f64>> = lfs
            .iter()
            .enumerate()
            .map(|(zi, p)| {
                if !p.flat {
                    return None;
                }
                let mut lo = f64::INFINITY;
                for i in 0..t.n {
                    if t.zone[i] as usize == zi && t.elevated[i] == 0 {
                        lo = js::min(lo, f64::from(t.py[i]));
                    }
                }
                Some(lo)
            })
            .collect();
        let city_y = flat_y.iter().find_map(|&y| y).unwrap_or(0.0);
        let channels = t
            .tags
            .iter()
            .filter(|g| g.tag == "bridge")
            .map(|g| Channel {
                s0: g.s0 + 50.0,
                s1: g.s1 - 50.0,
            })
            .collect();
        Ok(Terrain {
            noise: Noise2D::new(opts.seed),
            noise2: Noise2D::new(opts.seed.wrapping_add(101)),
            flattens: Vec::new(),
            carves: Vec::new(),
            margin,
            min_x,
            min_z,
            max_x,
            max_z,
            sea_y: level.sea_y,
            forms,
            lf: lfs,
            edges,
            flat_y,
            city_y,
            channels,
            desert_rail: None,
            ground: level.ground.clone(),
            ground_color: None,
            far: FarField::default(),
            near_tiles: Vec::new(),
            near_index: BTreeMap::new(),
            near_grid: NearGrid::default(),
            open: None,
            cw: None,
        })
    }

    /// Zone boundary x positions (Level 1 scenery reads these by name).
    pub fn x1(&self) -> f64 {
        self.edges.first().map_or(f64::INFINITY, |e| e.x)
    }

    pub fn x2(&self) -> f64 {
        self.edges.get(1).map_or(f64::INFINITY, |e| e.x)
    }

    /// The far field's size in cells (`fw`, `fh`), once built.
    pub fn far_size(&self) -> (usize, usize) {
        (self.far.fw, self.far.fh)
    }

    /// Road on a bridge/viaduct at sample i: the ground isn't raised to it.
    pub fn is_elevated(&self, t: &Track, i: usize) -> bool {
        if t.elevated[i] != 0 {
            return true;
        }
        match self.flat_y.get(t.zone[i] as usize) {
            Some(Some(fy)) => f64::from(t.py[i]) > fy + 1.2,
            _ => false,
        }
    }

    /// Flatten a disc to height y (or to the terrain's own height at its
    /// centre when y is `None`). Must be registered before build. The JS
    /// default falloff is 20.
    pub fn add_flatten(&mut self, x: f64, z: f64, r: f64, falloff: f64, y: Option<f64>) {
        self.flattens.push(Flatten {
            x,
            z,
            r,
            falloff,
            y,
            y_resolved: None,
        });
    }

    /// Carve a channel along a polyline [{x,z}...].
    /// `under_road`: apply after the road blend, so the channel passes under
    /// the road deck (a bridge) instead of being filled in by the road.
    pub fn add_carve(&mut self, points: Vec<[f64; 2]>, width: f64, depth: f64, under_road: bool) {
        let (mut min_x, mut max_x, mut min_z, mut max_z) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for p in &points {
            min_x = js::min(min_x, p[0]);
            max_x = js::max(max_x, p[0]);
            min_z = js::min(min_z, p[1]);
            max_z = js::max(max_z, p[1]);
        }
        self.carves.push(Carve {
            points,
            width,
            depth,
            under_road,
            min_x: min_x - width,
            max_x: max_x + width,
            min_z: min_z - width,
            max_z: max_z + width,
        });
    }

    // Scripted views: the summit saddle looks down both sides and the
    // descent opens toward the valley.
    fn open_windows(t: &Track) -> Vec<Open> {
        let mut open = Vec::new();
        let zs1 = t.zone_start.get(1).map_or(f64::NAN, |&s| s as f64);
        let vs = zs1 + 800.0;
        let (vx, vz) = (at(&t.px, vs), at(&t.pz, vs));
        for tg in &t.tags {
            if tg.tag == "summit" {
                open.push(Open {
                    s0: tg.s0 - 40.0,
                    s1: tg.s1 + 120.0,
                    side: 0.0,
                    v: 0.8,
                });
            }
            if tg.tag == "descent" {
                let m = js::round((tg.s0 + zs1) / 2.0);
                let toward = if (vx - at(&t.px, m)) * at(&t.rx, m)
                    + (vz - at(&t.pz, m)) * at(&t.rz, m)
                    > 0.0
                {
                    1.0
                } else {
                    -1.0
                };
                open.push(Open {
                    s0: tg.s0,
                    s1: zs1,
                    side: toward,
                    v: 0.9,
                });
            }
        }
        open
    }

    /// `openBias(s, side)` (its windows are made by `build_fields`).
    pub fn open_bias(&self, s: f64, side: f64) -> f64 {
        let mut v = 0.0;
        for o in self.open.as_deref().unwrap_or(&[]) {
            let w = smoothstep(o.s0 - 80.0, o.s0, s) * (1.0 - smoothstep(o.s1, o.s1 + 80.0, s));
            let side_match = if o.side == 0.0 {
                1.0
            } else {
                clamp(0.5 + 0.5 * side * o.side, 0.0, 1.0)
            };
            v = js::max(v, o.v * w * side_match);
        }
        v
    }

    /// `zoneWeights(x)`.
    pub fn zone_weights(&self, x: f64) -> ZoneWeights {
        let n = self.forms.len();
        let mut w = [0.0; MAX_ZONES];
        let mut prev = 1.0;
        for (z, wz) in w.iter_mut().enumerate().take(n) {
            let t = match self.edges.get(z) {
                Some(e) => smoothstep(e.x - e.w, e.x + e.w, x),
                None => 0.0,
            };
            *wz = prev - t;
            prev = t;
        }
        ZoneWeights { w, n }
    }

    // ── Field construction ─────────────────────────────────────────

    /// `buildFields()`.
    pub fn build_fields(&mut self, t: &Track) {
        // Far field.
        let fw = js::round((self.max_x - self.min_x) / FAR) as usize + 1;
        let fh = js::round((self.max_z - self.min_z) / FAR) as usize + 1;
        let mut fd = vec![1e9f32; fw * fh];
        let mut fy = vec![0f32; fw * fh];
        let mut fs = vec![0f32; fw * fh];
        let mut fl = vec![0f32; fw * fh];
        let mut flat = vec![0f32; fw * fh]; // signed lateral distance
        const R: f64 = 3000.0;
        let mut i = 0;
        while i < t.n {
            let x = f64::from(t.px[i]);
            let z = f64::from(t.pz[i]);
            let i0 = js::max(0.0, ((x - R - self.min_x) / FAR).floor()) as i64;
            let i1 = js::min((fw - 1) as f64, ((x + R - self.min_x) / FAR).ceil()) as i64;
            let j0 = js::max(0.0, ((z - R - self.min_z) / FAR).floor()) as i64;
            let j1 = js::min((fh - 1) as f64, ((z + R - self.min_z) / FAR).ceil()) as i64;
            let (rx, rz) = (f64::from(t.rx[i]), f64::from(t.rz[i]));
            let py = t.py[i];
            for j in j0..=j1 {
                let wz = self.min_z + j as f64 * FAR - z;
                for ii in i0..=i1 {
                    let wx = self.min_x + ii as f64 * FAR - x;
                    let d2 = wx * wx + wz * wz;
                    let k = j as usize * fw + ii as usize;
                    if d2 < f64::from(fd[k]) {
                        fd[k] = d2 as f32;
                        fy[k] = py;
                        fs[k] = i as f32;
                        let lat = wx * rx + wz * rz;
                        fl[k] = if lat >= 0.0 { 1.0 } else { -1.0 };
                        flat[k] = lat as f32;
                    }
                }
            }
            i += 12;
        }
        for v in fd.iter_mut() {
            *v = f64::from(*v).sqrt() as f32;
        }
        self.far = FarField {
            fw,
            fh,
            fd,
            fy,
            fs,
            fl,
            flat,
            fopen: None,
        };
        if self.forms.contains(&Form::Mountain) {
            self.open_field(t);
        }
        if self.forms.contains(&Form::Canyon) {
            self.cw = Some(Self::canyon_widths(t, &self.noise, &self.noise2));
        }

        // Near field: sparse 256 m tiles of 4 m nodes.
        self.near_tiles.clear();
        self.near_index.clear();
        // Circuits: the flat run-off reaches the barriers on each side.
        let corridor = self.lf.iter().all(|f| f.corridor);
        let mut i = 0;
        while i < t.n {
            self.near_sample(t, i, corridor);
            i += 2;
        }
        self.index_near_tiles();
    }

    fn near_sample(&mut self, t: &Track, i: usize, corridor: bool) {
        let zb = t.zone_blend(i as f64, 150.0);
        let hw = f64::from(t.hw[i]);
        let (mut r0, mut band, mut k_pow) = (hw, 0.0, 0.0);
        for (z, &w) in zb.iter().enumerate() {
            r0 += w * self.lf[z].r0;
            band += w * self.lf[z].band;
            k_pow += w * self.lf[z].pow;
        }
        let (r_l, r_r) = if corridor {
            (
                f64::from(t.wall_l[i]) + r0 - hw,
                f64::from(t.wall_r[i]) + r0 - hw,
            )
        } else {
            (r0, r0)
        };
        let r1 = js::max(r_l, r_r) + band;
        // Circuits keep the run-off nearly flush; in a sag (the foot of the
        // Corkscrew) a straight 4 m chord of ground would cut above the
        // curving road, so it sits a little lower there.
        let sag = if corridor {
            js::max(
                0.0,
                f64::from(t.py[t.idx(i as f64 - 3.0)]) + f64::from(t.py[t.idx(i as f64 + 3.0)])
                    - 2.0 * f64::from(t.py[i]),
            ) / 9.0
        } else {
            0.0
        };
        let drop = if corridor { 0.1 + sag * 6.0 } else { 0.3 };
        // Bridges and viaducts: no embankment, the deck stands on piers.
        if self.is_elevated(t, i) {
            return;
        }
        let (x, z, y) = (f64::from(t.px[i]), f64::from(t.pz[i]), f64::from(t.py[i]));
        let (rx, rz, bank) = (f64::from(t.rx[i]), f64::from(t.rz[i]), f64::from(t.bank[i]));
        let n0x = ((x - r1) / NEAR).floor() as i64;
        let n1x = ((x + r1) / NEAR).ceil() as i64;
        let n0z = ((z - r1) / NEAR).floor() as i64;
        let n1z = ((z + r1) / NEAR).ceil() as i64;
        for nz in n0z..=n1z {
            let dz = nz as f64 * NEAR - z;
            for nx in n0x..=n1x {
                let dx = nx as f64 * NEAR - x;
                let d = (dx * dx + dz * dz).sqrt();
                if d > r1 {
                    continue;
                }
                let lat = dx * rx + dz * rz;
                let rs = if lat >= 0.0 { r_r } else { r_l };
                if corridor && d > rs + band {
                    continue;
                }
                let ti = self.near_tile_for(nx, nz);
                let tile = &mut self.near_tiles[ti];
                let li = ((nz - tile.oz) * (NT + 1) + (nx - tile.ox)) as usize;
                let k = 1.0 - kernel::pow(smoothstep(rs, rs + band, d), k_pow);
                // Run-off: the track's own surface out to the barriers. Where the
                // ground folds up from a banked road (concave), the 4 m chord
                // would stand proud of the road's edge, so drop it by the fold.
                let fold = if corridor {
                    let run = if lat >= 0.0 {
                        f64::from(t.run_r.as_ref().expect("a corridor track has run-off")[i]) + bank
                    } else {
                        f64::from(t.run_l.as_ref().expect("a corridor track has run-off")[i]) - bank
                    };
                    js::max(0.0, run)
                        * 0.5
                        * (1.0
                            - smoothstep(hw + RUNOFF_FLAT + 2.0, hw + RUNOFF_FLAT + 6.0, lat.abs()))
                } else {
                    0.0
                };
                if k > f64::from(tile.k[li]) {
                    tile.k[li] = k as f32;
                }
                if corridor {
                    // The run-off under a car is the surface of the nearest bit of
                    // road (physics projects onto it), so take that one alone: an
                    // average over the stretches round a tight corner on a steep
                    // drop (the Corkscrew) rides high. Offset along the road too,
                    // since samples are 2 m apart.
                    if d < f64::from(tile.d[li]) {
                        let along = dx * f64::from(t.fx[i]) + dz * f64::from(t.fz[i]);
                        tile.swh[li] = (t.surface_y(i as f64 + along, lat) - drop - fold) as f32;
                        tile.sw[li] = 1.0;
                    }
                } else {
                    let h = y - clamp(lat, -hw, hw) * bank - drop;
                    let w = (k * k) / ((d * d + 1.0) * (d * d + 1.0));
                    tile.sw[li] = (f64::from(tile.sw[li]) + w) as f32;
                    tile.swh[li] = (f64::from(tile.swh[li]) + w * h) as f32;
                }
                if d < f64::from(tile.d[li]) {
                    tile.d[li] = d as f32;
                    tile.s[li] = i as f32;
                }
            }
        }
    }

    /// `nearTileFor(nx, nz, true)`: the tile holding a node, made if new.
    fn near_tile_for(&mut self, nx: i64, nz: i64) -> usize {
        let tx = nx.div_euclid(NT);
        let tz = nz.div_euclid(NT);
        if let Some(&k) = self.near_index.get(&(tx, tz)) {
            return k;
        }
        let n = ((NT + 1) * (NT + 1)) as usize;
        self.near_tiles.push(NearTile {
            tx,
            tz,
            ox: tx * NT,
            oz: tz * NT,
            k: vec![0.0; n],
            sw: vec![0.0; n],
            swh: vec![0.0; n],
            d: vec![1e9; n],
            s: vec![0.0; n],
        });
        let k = self.near_tiles.len() - 1;
        self.near_index.insert((tx, tz), k);
        k
    }

    fn index_near_tiles(&mut self) {
        let mut g = NearGrid::default();
        if !self.near_tiles.is_empty() {
            let tx0 = self.near_tiles.iter().map(|t| t.tx).min().expect("tiles");
            let tx1 = self.near_tiles.iter().map(|t| t.tx).max().expect("tiles");
            let tz0 = self.near_tiles.iter().map(|t| t.tz).min().expect("tiles");
            let tz1 = self.near_tiles.iter().map(|t| t.tz).max().expect("tiles");
            g.tx0 = tx0;
            g.tz0 = tz0;
            g.w = tx1 - tx0 + 1;
            g.h = tz1 - tz0 + 1;
            g.cells = vec![0; (g.w * g.h) as usize];
            for (k, t) in self.near_tiles.iter().enumerate() {
                g.cells[((t.tz - tz0) * g.w + (t.tx - tx0)) as usize] = k as u32 + 1;
            }
        }
        self.near_grid = g;
    }

    /// The near tiles in the order they were made.
    pub fn near_tiles(&self) -> &[NearTile] {
        &self.near_tiles
    }

    /// `nearTiles.has(tx + ',' + tz)`.
    pub fn has_near_tile(&self, tx: i64, tz: i64) -> bool {
        self.near_index.contains_key(&(tx, tz))
    }

    /// Near-field node values (`nearNode`); `None` when no road influence.
    pub fn near_node(&self, nx: i64, nz: i64) -> Option<(&NearTile, usize)> {
        let tx = nx.div_euclid(NT);
        let tz = nz.div_euclid(NT);
        let g = &self.near_grid;
        let (cx, cz) = (tx - g.tx0, tz - g.tz0);
        if cx < 0 || cz < 0 || cx >= g.w || cz >= g.h {
            return None;
        }
        let k = g.cells[(cz * g.w + cx) as usize];
        if k == 0 {
            return None;
        }
        let tile = &self.near_tiles[k as usize - 1];
        let li = ((nz - tile.oz) * (NT + 1) + (nx - tile.ox)) as usize;
        Some((tile, li))
    }

    /// Bilinear far-field sample → {d, y, s, side, lat, open}.
    pub fn far(&self, x: f64, z: f64) -> FarSample {
        let f = &self.far;
        let gx = clamp((x - self.min_x) / FAR, 0.0, f.fw as f64 - 1.001);
        let gz = clamp((z - self.min_z) / FAR, 0.0, f.fh as f64 - 1.001);
        let i = gx.floor();
        let j = gz.floor();
        let tx = gx - i;
        let tz = gz - j;
        let k00 = j as usize * f.fw + i as usize;
        let k10 = k00 + 1;
        let k01 = k00 + f.fw;
        let k11 = k01 + 1;
        let b = |a: &[f32]| {
            lerp(
                lerp(f64::from(a[k00]), f64::from(a[k10]), tx),
                lerp(f64::from(a[k01]), f64::from(a[k11]), tx),
                tz,
            )
        };
        FarSample {
            d: b(&f.fd),
            y: b(&f.fy),
            s: b(&f.fs),
            side: b(&f.fl),
            lat: b(&f.flat),
            open: f.fopen.as_deref().map_or(0.0, b),
        }
    }

    // ── Landforms per zone ─────────────────────────────────────────

    /// `landform(x, z)`: the zones' landforms blended by x.
    pub fn landform(&self, x: f64, z: f64) -> f64 {
        let f = self.far(x, z);
        self.landform_at(x, z, &f)
    }

    /// `landform(x, z, F)` with the far-field sample given.
    pub fn landform_at(&self, x: f64, z: f64, f: &FarSample) -> f64 {
        let w = self.zone_weights(x);
        let detail = fbm(&self.noise2, x / 90.0, z / 90.0, 3) * 3.0;
        let mut h = 0.0;
        for k in 0..w.n {
            if w.w[k] > 0.001 {
                h += w.w[k] * self.form(self.forms[k], x, z, f, detail, k);
            }
        }
        h
    }

    fn form(&self, form: Form, x: f64, z: f64, f: &FarSample, detail: f64, k: usize) -> f64 {
        match form {
            Form::Mountain => self.form_mountain(x, z, f, detail),
            Form::Valley => self.form_valley(x, z, f, detail),
            Form::City => self.form_city(x, z, f, detail, k),
            Form::Coast => self.form_coast(x, z, f, detail),
            Form::Beach => self.form_beach(x, z, f, detail),
            Form::Harbor => self.form_harbor(x, z, f, detail, k),
            Form::Streets => self.form_streets(x, z, f),
            Form::Canyon => self.form_canyon(x, z, f, detail),
            Form::Desert => self.form_desert(x, z, f, detail),
            Form::Playa => self.form_playa(x, z, f),
            Form::Raceway => self.form_raceway(x, z),
        }
    }

    // Mountain lookouts: per far-field cell, how far the ground falls away
    // instead of rising (0 wall .. 1 drop), from that cell's own nearest road
    // s and side. Bilinear sampling of this is smooth even where neighbouring
    // cells belong to different road branches.
    fn open_field(&mut self, t: &Track) {
        if self.open.is_none() {
            self.open = Some(Self::open_windows(t));
        }
        let n = &self.noise;
        let mut fopen = vec![0f32; self.far.fd.len()];
        for (k, o) in fopen.iter_mut().enumerate() {
            let s = f64::from(self.far.fs[k]);
            let side = f64::from(self.far.fl[k]);
            *o = js::max(
                smoothstep(0.15, 0.55, n.noise(s / 520.0, side * 3.1 + 11.7)),
                self.open_bias(s, side),
            ) as f32;
        }
        self.far.fopen = Some(fopen);
    }

    pub fn form_mountain(&self, x: f64, z: f64, f: &FarSample, detail: f64) -> f64 {
        let (n, n2, d) = (&self.noise, &self.noise2, f.d);
        // Terrain rises on both sides of the pass, except where a stretch of
        // road opens up onto a drop (a lookout) on one side.
        // How open each side is comes precomputed per far-field cell (see
        // openField): computing it from the interpolated nearest-road s and side
        // raced through noise cells and lookout windows wherever neighbouring
        // cells were nearest to different road branches (hairpins), leaving
        // sawtooth spikes on the walls beside the road.
        let open = f.open;
        // Soft-edged modulation: sharp ridged noise here turns into needles.
        let rocky = 0.5 + 0.5 * fbm(n, x / 320.0, z / 320.0, 3);
        let wall = (18.0 + 150.0 * (1.0 - kernel::exp(-d / 90.0))) * (0.62 + 0.7 * rocky);
        let drop = -130.0 * (1.0 - kernel::exp(-d / 160.0)) * (0.7 + 0.5 * rocky);
        let local = lerp(wall, drop, open);
        let rp = ridged_with(n, x / 1900.0 + 3.3, z / 1900.0, 5, 2.0, 0.45);
        let peaks = smoothstep(250.0, 1600.0, d)
            * (180.0 + 420.0 * rp * rp * 1.6 + 120.0 * fbm(n2, x / 2500.0, z / 2500.0, 3));
        f.y + local * (1.0 - smoothstep(600.0, 1400.0, d)) + peaks + detail * 2.0
    }

    pub fn form_valley(&self, x: f64, z: f64, f: &FarSample, detail: f64) -> f64 {
        let (n, d) = (&self.noise, f.d);
        let roll = fbm(n, x / 380.0, z / 380.0, 3) * 7.0 * smoothstep(15.0, 90.0, d);
        let hills = smoothstep(260.0, 1500.0, d)
            * (40.0 + 380.0 * ridged(n, x / 1700.0 + 9.1, z / 1700.0, 5));
        f.y - 1.2 + roll + hills + detail * 0.4
    }

    pub fn form_city(&self, x: f64, z: f64, f: &FarSample, _detail: f64, k: usize) -> f64 {
        let (n, d) = (&self.noise, f.d);
        let hills = smoothstep(1300.0, 2600.0, d)
            * (90.0 + 320.0 * ridged(n, x / 1500.0 + 4.2, z / 1500.0, 4));
        self.flat_y[k].unwrap_or(f64::NAN) - 0.25 + hills
    }

    /// Sea on the left of the road, land on the right. Sharp right beside the
    /// road; the split softens with distance so that where the nearest bit of
    /// road changes (behind the start, past the end) the land ramps into the
    /// sea instead of standing as a straight wall.
    pub fn sea_side(&self, f: &FarSample) -> f64 {
        let r = 4.0 + 0.35 * f.d;
        smoothstep(r, -r, f.lat)
    }

    /// Cliff road: sheer drop to the sea on the left, steep hills on the right.
    pub fn form_coast(&self, x: f64, z: f64, f: &FarSample, detail: f64) -> f64 {
        let (n, d) = (&self.noise, f.d);
        let sea = self.sea_y.unwrap_or(0.0);
        // The cliff edge wanders in and out so the coastline isn't a copy of the road.
        let wob = 0.7 + 0.6 * (0.5 + 0.5 * fbm(n, x / 170.0, z / 170.0, 3));
        let cliff = kernel::pow(smoothstep(7.0, 75.0, d * wob), 0.6);
        let floor = sea - 14.0 - 12.0 * smoothstep(200.0, 900.0, d);
        // (Sea stacks and arches are placed as meshes by Coast.js.)
        let hs = lerp(f.y - 1.0, floor, cliff) + detail * 1.5 * cliff;
        let rocky = 0.5 + 0.5 * fbm(n, x / 300.0, z / 300.0, 3);
        let hl = f.y
            + (10.0 + 150.0 * (1.0 - kernel::exp(-d / 140.0))) * (0.55 + 0.8 * rocky)
            + smoothstep(400.0, 1800.0, d)
                * (120.0 + 260.0 * ridged(n, x / 1500.0 + 2.2, z / 1500.0, 5))
            + detail * 2.0;
        lerp(hl, hs, self.sea_side(f))
    }

    /// Beach town: promenade and a wide sand beach on the left, a flat town
    /// backed by hills on the right.
    pub fn form_beach(&self, x: f64, z: f64, f: &FarSample, detail: f64) -> f64 {
        let (n, n2, d) = (&self.noise, &self.noise2, f.d);
        let sea = self.sea_y.unwrap_or(0.0);
        let sand = smoothstep(16.0, 150.0, d);
        let hs = lerp(f.y - 1.6, sea - 2.5, sand) - 9.0 * smoothstep(160.0, 520.0, d)
            + fbm(n2, x / 40.0, z / 40.0, 2) * 0.35 * sand;
        let hl = f.y - 0.4
            + smoothstep(420.0, 1600.0, d)
                * (60.0 + 240.0 * ridged(n, x / 1400.0 + 5.1, z / 1400.0, 5))
            + detail * 0.3 * smoothstep(300.0, 500.0, d);
        lerp(hl, hs, self.sea_side(f))
    }

    /// Harbour: flat quays, a quay wall to the sea on the left, a shipping
    /// channel under the bridge, hills far inland.
    pub fn form_harbor(&self, x: f64, z: f64, f: &FarSample, _detail: f64, k: usize) -> f64 {
        let (n, d) = (&self.noise, f.d);
        let sea = self.sea_y.unwrap_or(0.0);
        let water = sea - 14.0;
        let mut h = self.flat_y[k].unwrap_or(f64::NAN) - 0.3;
        h += (1.0 - self.sea_side(f))
            * smoothstep(900.0, 2200.0, d)
            * (100.0 + 220.0 * ridged(n, x / 1600.0 + 8.8, z / 1600.0, 5));
        h = lerp(h, water, smoothstep(175.0, 195.0, d) * self.sea_side(f));
        for c in &self.channels {
            let m = smoothstep(c.s0, c.s0 + 90.0, f.s) * (1.0 - smoothstep(c.s1 - 90.0, c.s1, f.s));
            h = lerp(h, water, m);
        }
        h
    }

    /// Downtown Streets: the level's own ground function (level.ground(x, z)).
    pub fn form_streets(&self, x: f64, z: f64, f: &FarSample) -> f64 {
        let d = f.d;
        let hills = smoothstep(1200.0, 2400.0, d)
            * (60.0 + 260.0 * ridged(&self.noise, x / 1500.0 + 6.1, z / 1500.0, 4));
        (match &self.ground {
            Some(g) => g(x, z),
            None => f.y,
        }) - 0.3
            + hills
    }

    /// Seaside Raceway: the surveyed ground (level.ground), as is.
    pub fn form_raceway(&self, x: f64, z: f64) -> f64 {
        (self
            .ground
            .as_ref()
            .expect("the raceway landform needs level.ground"))(x, z)
    }

    // ── Desert Run (Level 4) ───────────────────────────────────────

    /// Red sandstone: horizontal strata, each a soft slope topped by a hard
    /// cliff band. Applied in absolute height so layers stay level while the
    /// road descends through them.
    pub fn strata(&self, y: f64) -> f64 {
        const B: f64 = 9.0;
        let w = y + 1.6 * self.noise2.noise(y * 0.05, 3.7);
        let k = (w / B).floor();
        let f = w / B - k;
        let g = if f < 0.55 {
            (f / 0.55) * 0.22
        } else {
            0.22 + ((f - 0.55) / 0.45) * 0.78
        };
        y + ((k + g) * B - w)
    }

    // Distance from the road centreline to the foot of each canyon wall,
    // every 10 m of road: narrows, an amphitheatre of hoodoos, the arch, and
    // the mouth opening onto the basin. Smoothed so the walls wander.
    fn canyon_widths(t: &Track, noise: &Noise2D, noise2: &Noise2D) -> CanyonWidths {
        let n = (t.length / 10.0).ceil() as usize + 1;
        let mut l = vec![0f32; n];
        let mut r = vec![0f32; n];
        let within = |tag: &str, sv: f64, pad: f64| {
            t.tags
                .iter()
                .any(|g| g.tag == tag && sv > g.s0 - pad && sv < g.s1 + pad)
        };
        let zs1 = t.zone_start.get(1).map_or(f64::NAN, |&s| s as f64);
        let mouth: Vec<f64> = t
            .tags
            .iter()
            .filter(|g| g.tag == "mouth")
            .map(|g| g.s0)
            .collect();
        let m0 = if mouth.is_empty() {
            zs1 - 300.0
        } else {
            js::min_n(&mouth)
        };
        for i in 0..n {
            let sv = (i * 10) as f64;
            for (arr, sd) in [(&mut l, -1.0), (&mut r, 1.0)] {
                let mut w = 24.0 + 22.0 * (0.5 + 0.5 * noise.noise(sv / 260.0, sd * 4.1 + 1.3));
                // Side canyons: the wall steps back for a while.
                w += 55.0 * smoothstep(0.45, 0.7, noise2.noise(sv / 140.0, sd * 7.7 + 2.2));
                if within("narrows", sv, 20.0) {
                    w = 13.5 + 2.0 * (0.5 + 0.5 * noise.noise(sv / 60.0, sd));
                }
                if within("arch", sv, 10.0) {
                    w = js::min(w, 17.0);
                }
                if within("fin", sv, 0.0) {
                    w = js::min(w, 16.0);
                }
                if within("hoodoos", sv, 30.0) && sd > 0.0 {
                    w = 150.0;
                }
                if sv < 220.0 {
                    w = js::max(w, 34.0); // room round the start
                }
                w += smoothstep(m0, zs1 + 250.0, sv) * 520.0;
                arr[i] = w as f32;
            }
        }
        let sm = |a: &[f32]| -> Vec<f32> {
            let mut out = vec![0f32; a.len()];
            for (i, o) in out.iter_mut().enumerate() {
                let (mut acc, mut wt) = (0.0, 0.0);
                for k in -4i64..=4 {
                    let j = clamp((i as i64 + k) as f64, 0.0, (a.len() - 1) as f64) as usize;
                    let q = kernel::exp(-((k * k) as f64) / 8.0);
                    acc += f64::from(a[j]) * q;
                    wt += q;
                }
                *o = (acc / wt) as f32;
            }
            out
        };
        CanyonWidths {
            l: sm(&l),
            r: sm(&r),
            n,
        }
    }

    /// `canyonWidth(s, side)` (its table is made by `build_fields`).
    pub fn canyon_width(&self, s: f64, side: f64) -> f64 {
        let cw = self
            .cw
            .as_ref()
            .expect("canyon widths are made by build_fields");
        let i = clamp(s / 10.0, 0.0, cw.n as f64 - 1.001);
        let i0 = i.floor();
        let f = i - i0;
        let a = if side < 0.0 { &cw.l } else { &cw.r };
        let i0 = i0 as usize;
        f64::from(a[i0]) + (f64::from(a[i0 + 1]) - f64::from(a[i0])) * f
    }

    pub fn form_canyon(&self, x: f64, z: f64, f: &FarSample, detail: f64) -> f64 {
        let (n, n2, d) = (&self.noise, &self.noise2, f.d);
        let side = if f.lat >= 0.0 { 1.0 } else { -1.0 };
        // Buttresses and alcoves: the foot of the wall wanders in and out.
        let w = self.canyon_width(f.s, side)
            * (0.88 + 0.24 * (0.5 + 0.5 * fbm(n, x / 120.0, z / 120.0, 2)))
            + 9.0 * fbm(n2, x / 38.0 + 5.5, z / 38.0, 2);
        let u = d - w; // metres past the foot of the wall
        // Wall height and how far back it climbs before the rim.
        let rim = 46.0 + 62.0 * (0.5 + 0.5 * fbm(n, x / 650.0 + 3.1, z / 650.0, 2));
        let reach = 7.0 + 16.0 * (0.5 + 0.5 * n2.noise(x / 260.0, z / 260.0));
        let mut wall = rim * smoothstep(-3.0, reach, u) + 5.0 * smoothstep(-14.0, 2.0, u);
        // Slickrock domes and fins along the rim.
        wall += 26.0
            * smoothstep(0.15, 0.6, fbm(n, x / 230.0 + 1.7, z / 230.0 - 2.3, 2))
            * smoothstep(reach, reach + 60.0, u);
        // Beyond the rim: benchland rising in steps toward the far ranges.
        wall += smoothstep(200.0, 900.0, d)
            * (30.0 + 70.0 * (0.5 + 0.5 * fbm(n2, x / 900.0, z / 900.0, 3)));
        wall += smoothstep(1300.0, 2600.0, d)
            * (120.0 + 380.0 * ridged(n, x / 1700.0 + 6.6, z / 1700.0, 5));
        let y = f.y + wall + detail * 0.6;
        // Terrace the rock; the canyon floor stays smooth sand.
        let rock = smoothstep(3.0, 12.0, wall);
        lerp(y, self.strata(y), rock)
            + (1.0 - rock) * fbm(n2, x / 50.0, z / 50.0, 2) * 0.6 * smoothstep(8.0, 20.0, d)
    }

    /// Open basin: gentle swells, washes, flat-topped mesas standing off in
    /// the distance and a jagged range on the horizon. Desert.plan() can lay
    /// a level bed for the railway beside the road (`desert_rail`).
    pub fn form_desert(&self, x: f64, z: f64, f: &FarSample, detail: f64) -> f64 {
        let (n, n2, d) = (&self.noise, &self.noise2, f.d);
        let mut h = f.y - 0.8
            + fbm(n, x / 380.0, z / 380.0, 3) * 4.5 * smoothstep(25.0, 140.0, d)
            + detail * 0.35 * smoothstep(20.0, 60.0, d);
        // Mesas: a thresholded noise field gives flat tops and sheer terraced sides.
        let m = fbm(n2, x / 1100.0 + 17.3, z / 1100.0 - 4.2, 3);
        let mesa = smoothstep(0.2, 0.3, m) * smoothstep(500.0, 800.0, d);
        if mesa > 0.0 {
            let top = f.y + 70.0 + 90.0 * (0.5 + 0.5 * n.noise(x / 2300.0, z / 2300.0));
            let y = lerp(h, top, mesa);
            h = lerp(
                y,
                self.strata(y),
                smoothstep(0.05, 0.4, mesa) * (1.0 - smoothstep(0.9, 1.0, mesa)),
            );
        }
        h += smoothstep(1500.0, 2800.0, d)
            * (140.0 + 420.0 * ridged(n, x / 1600.0 + 2.7, z / 1600.0, 5));
        if let Some(r) = &self.desert_rail
            && f.s > r.s0
            && f.s < r.s1
        {
            let k = 1.0 - smoothstep(r.half, r.half + 22.0, (f.lat - r.lat).abs());
            h = lerp(h, f.y - r.drop, k);
        }
        h
    }

    /// Dry lake: dead level out to a wandering shoreline, alluvial fans, then
    /// the mountains that ring the basin.
    pub fn form_playa(&self, x: f64, z: f64, f: &FarSample) -> f64 {
        let (n, d) = (&self.noise, f.d);
        let shore = 700.0 + 500.0 * (0.5 + 0.5 * fbm(n, x / 1400.0 + 9.1, z / 1400.0, 2));
        let fan = smoothstep(shore, shore + 900.0, d);
        f.y - 0.35
            + fan * fan * 60.0
            + smoothstep(shore + 500.0, shore + 2200.0, d)
                * (160.0 + 420.0 * ridged(n, x / 1500.0 + 1.9, z / 1500.0, 5))
    }

    /// Full terrain height at a world point.
    pub fn height_at(&self, x: f64, z: f64) -> f64 {
        let mut h = self.landform(x, z);
        // Modifiers (farm pads, creek) act on the landform only.
        for f in &self.flattens {
            let d = kernel::hypot(x - f.x, z - f.z);
            if d < f.r + f.falloff {
                let target = f.y.or(f.y_resolved).unwrap_or(f64::NAN);
                h = lerp(target, h, smoothstep(f.r, f.r + f.falloff, d));
            }
        }
        for c in &self.carves {
            if !c.under_road {
                h -= Self::carve_depth(c, x, z);
            }
        }

        // Road influence (bilinear over near-field nodes).
        let gx = x / NEAR;
        let gz = z / NEAR;
        let nx = gx.floor();
        let nz = gz.floor();
        let tx = gx - nx;
        let tz = gz - nz;
        let (mut k, mut sw, mut swh) = (0.0, 0.0, 0.0);
        for c in 0..4 {
            let ax = nx as i64 + (c & 1);
            let az = nz as i64 + (c >> 1);
            let Some((tile, li)) = self.near_node(ax, az) else {
                continue;
            };
            let wgt = (if c & 1 != 0 { tx } else { 1.0 - tx })
                * (if c >> 1 != 0 { tz } else { 1.0 - tz });
            k += f64::from(tile.k[li]) * wgt;
            if tile.sw[li] > 0.0 {
                let rh = f64::from(tile.swh[li]) / f64::from(tile.sw[li]);
                sw += wgt;
                swh += wgt * rh;
            }
        }
        if k > 0.0 && sw > 0.0 {
            h = lerp(h, swh / sw, k);
        }
        for c in &self.carves {
            if c.under_road {
                h -= Self::carve_depth(c, x, z);
            }
        }
        h
    }

    /// Resolve flatten targets that were given without a height.
    pub fn resolve_flattens(&mut self) {
        let resolved: Vec<Option<f64>> = self
            .flattens
            .iter()
            .map(|f| match f.y {
                None => Some(self.landform(f.x, f.z)),
                Some(_) => f.y_resolved,
            })
            .collect();
        for (f, r) in self.flattens.iter_mut().zip(resolved) {
            f.y_resolved = r;
        }
    }

    /// `carveDepth(c, x, z)`.
    pub fn carve_depth(c: &Carve, x: f64, z: f64) -> f64 {
        if x < c.min_x || x > c.max_x || z < c.min_z || z > c.max_z {
            return 0.0;
        }
        let mut best = f64::INFINITY;
        let pts = &c.points;
        for i in 0..pts.len().saturating_sub(1) {
            let (a, b) = (pts[i], pts[i + 1]);
            let abx = b[0] - a[0];
            let abz = b[1] - a[1];
            let t = clamp(
                ((x - a[0]) * abx + (z - a[1]) * abz) / (abx * abx + abz * abz),
                0.0,
                1.0,
            );
            let dx = x - (a[0] + abx * t);
            let dz = z - (a[1] + abz * t);
            let d = dx * dx + dz * dz;
            if d < best {
                best = d;
            }
        }
        best = best.sqrt();
        c.depth * (1.0 - smoothstep(c.width * 0.35, c.width, best))
    }

    /// Road proximity info for scenery placement.
    pub fn road_info(&self, x: f64, z: f64) -> RoadInfo {
        if let Some((tile, li)) =
            self.near_node(js::round(x / NEAR) as i64, js::round(z / NEAR) as i64)
            && tile.d[li] < 1e8
        {
            return RoadInfo {
                d: f64::from(tile.d[li]),
                s: f64::from(tile.s[li]),
                near: true,
            };
        }
        let f = self.far(x, z);
        RoadInfo {
            d: f.d,
            s: f.s,
            near: false,
        }
    }

    /// `slopeAt(x, z, e)` (the JS default e is 2).
    pub fn slope_at(&self, x: f64, z: f64, e: f64) -> f64 {
        let hx = self.height_at(x + e, z) - self.height_at(x - e, z);
        let hz = self.height_at(x, z + e) - self.height_at(x, z - e);
        kernel::hypot(hx, hz) / (2.0 * e)
    }

    // ── Mesh ───────────────────────────────────────────────────────

    /// Returns plain values per tile so the builder can run anywhere; the
    /// mesh side (`terrain_mesh`) turns them into geometry.
    pub fn tile_list(&self) -> Vec<Tile> {
        let mut tiles = Vec::new();
        let nx = js::round((self.max_x - self.min_x) / TILE) as i64;
        let nz = js::round((self.max_z - self.min_z) / TILE) as i64;
        for j in 0..nz {
            for i in 0..nx {
                let x0 = self.min_x + i as f64 * TILE;
                let z0 = self.min_z + j as f64 * TILE;
                // Closest approach of the road to this tile, from the far field.
                let mut dmin = f64::INFINITY;
                for a in (0..=8).step_by(2) {
                    for b in (0..=8).step_by(2) {
                        dmin = js::min(
                            dmin,
                            self.far(x0 + f64::from(a) * 32.0, z0 + f64::from(b) * 32.0)
                                .d,
                        );
                    }
                }
                let has_near =
                    self.has_near_tile((x0 / TILE).floor() as i64, (z0 / TILE).floor() as i64);
                let step = if has_near || dmin < 60.0 {
                    4.0
                } else if dmin < 700.0 {
                    16.0
                } else {
                    32.0
                };
                tiles.push(Tile {
                    x0,
                    z0,
                    size: TILE,
                    step,
                    dmin,
                    i,
                    j,
                    nsteps: [step; 4],
                });
            }
        }
        // The grid is complete (nx × nz in order), so a neighbour is found
        // by its position.
        let steps: Vec<f64> = tiles.iter().map(|t| t.step).collect();
        let get = |i: i64, j: i64| -> Option<f64> {
            (i >= 0 && j >= 0 && i < nx && j < nz).then(|| steps[(j * nx + i) as usize])
        };
        for t in &mut tiles {
            // Neighbour resolution per edge: z0 (j-1), z1 (j+1), x0 (i-1), x1 (i+1).
            for (e, (a, b)) in [(0, -1), (0, 1), (-1, 0), (1, 0)].into_iter().enumerate() {
                t.nsteps[e] = get(t.i + a, t.j + b).unwrap_or(t.step);
            }
        }
        tiles
    }
}

// ── The plan the scenery registers ──────────────────────────────────────

/// What the scenery modules' `plan()` registers with the terrain before
/// its fields are built: flattens, carves, Desert's railway bed. Until the
/// scenery is ported (WP 3.6 on) a recorded plan
/// (`parity/golden/terrain/<level>.json`, `tools/parity/terrain-plan.mjs`)
/// stands in for it (DECISIONS D232).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerrainPlan {
    pub flattens: Vec<Flatten>,
    pub carves: Vec<Carve>,
    pub desert_rail: Option<DesertRail>,
}

impl TerrainPlan {
    /// Registers the plan, in its order, as the scenery would.
    pub fn apply(&self, t: &mut Terrain) {
        for f in &self.flattens {
            t.add_flatten(f.x, f.z, f.r, f.falloff, f.y);
        }
        for c in &self.carves {
            t.add_carve(c.points.clone(), c.width, c.depth, c.under_road);
        }
        if self.desert_rail.is_some() {
            t.desert_rail = self.desert_rail;
        }
    }
}
