//! `src/world/Beach.js` and `src/world/beach/*` (roadmap WP 7.1):
//! Seabright, the beach town on Level 2. The boulevard runs along the sand with the sea on the left: a
//! boardwalk promenade, lifeguard towers and a long pier with a Ferris
//! wheel and a little coaster on the ocean side; pastel motels, surf shops,
//! a diner and a gas station on the town side, with a grid of beach houses
//! behind; palms everywhere; a marina with sailboats at the far end. It's
//! sunrise here, so lamps and neon fade out as the sun comes up.
//!
//! The port keeps the JS structure: [`Beach::plan`] opens the kerb railing
//! at the cross streets and cuts the terraces for the hill houses;
//! [`Beach::build`] makes the materials and the two sign atlases, then runs
//! `reserveStreets`, `buildPromenade`, `buildFrontage`, `buildResidential`
//! and `buildHillHouses` (on the builder's far channel), the street
//! furniture, sidewalk props, street parking, beach props, the pier (with
//! the Ferris wheel and coaster), the marina, the traffic lights, the town
//! sign and the surf, in that order, into the ColorBuilder; then the
//! builder's meshes, the ribbons, the palms, the parked cars and the light
//! pools go into the group `beach`. `animate` is one
//! [`Animator`](crate::world::Animator).
//!
//! `beach/ColorBuilder.js` is [`crate::color_builder`] (WP 3.3);
//! `beach/atlas.js` is [`atlas`] and `beach/parts.js` [`parts`], which
//! Desert uses too.

// Index loops stay index loops, and the JS signatures stay (D52, D130).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod atlas;
pub mod parts;

use std::f64::consts::PI;
use std::sync::Arc;

use mp_canvas::Canvas;
use mp_math::{Mulberry32, js, kernel, lerp, rpick, rrange, smoothstep};
use mp_scene::{MaterialKind, three};
use mp_track::{Frame, Track};
use serde_json::Value;

use crate::builder::{BuildOpts, Builder, CastShadow};
use crate::car_model::{self, BuildOpts as CarOpts, Lod};
use crate::color::Color;
use crate::material::{Material, texture_value};
use crate::object::{GeoId, Image, Layer, MaterialId, NodeId, SceneGraph, TextureId};
use crate::textures::{Texture, TextureCache};
use crate::three_geom::{
    BufferAttribute, BufferGeometry, CatmullRomCurve3, Curve3, CurveType, Euler, Matrix4,
    Quaternion, Vector3, box_geometry, cylinder_geometry, icosahedron_geometry, merge_geometries,
    plane_geometry, sphere_geometry, torus_geometry, tube_geometry,
};
use crate::valley::ground::Ground;
use crate::world::{Animator, Change, Edit, Handle, Scenery, SceneryInfo, UpdateCtx, World};

use atlas::{NeonOpts, PaintedOpts, Rect, SignAtlas, neon_sign, painted_sign, sign_geometry};
use parts::{
    GasOpts, HotelOpts, HouseOpts, MotelOpts, ShopOpts, beach_house, bench, diner, gas_station,
    hotel, lifeguard_tower, motel, motorboat, picnic_table, prom_lamp, sailboat, shop,
    street_light, surf_shop, surfboards_in_sand, taco_stand, trash_can, umbrella_set,
    volleyball_net,
};

const ADDITIVE: f64 = three::ADDITIVE_BLENDING as f64;
const DOUBLE_SIDE: f64 = three::DOUBLE_SIDE as f64;

/// `yawZ(dx, dz)`: local +Z → (dx, dz).
fn yaw_z(dx: f64, dz: f64) -> f64 {
    kernel::atan2(dx, dz)
}

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// Plain paints: merged into one vertex-coloured mesh per channel.
const PALETTE: &[(&str, u32)] = &[
    ("wPink", 0xe9aea8),
    ("wMint", 0xa9d8c1),
    ("wYellow", 0xf1d690),
    ("wBlue", 0x9fc3de),
    ("wWhite", 0xefebe1),
    ("wPeach", 0xf1bf95),
    ("wLilac", 0xc6b6de),
    ("wTeal", 0x6db8b1),
    ("wSand", 0xdcc9a2),
    ("trim", 0xf6f3ea),
    ("concrete", 0xc4bdb0),
    ("white", 0xf2f2ee),
    ("wood", 0x9c7651),
    ("woodDark", 0x5a3f2b),
    ("roofTar", 0x55524d),
    ("roofTile", 0xbf6a4c),
    ("black", 0x1b1b1d),
    ("boardB", 0xff8a3c),
    ("paintRed", 0xc83a34),
    ("lawn", 0x6f9a45),
    ("hedge", 0x3f6a32),
    ("sailBlue", 0x2b4f86),
    ("hullWhite", 0xf1f2ee),
];

/// `canvasTex(w, h, draw, { repeat = true, srgb = true })`: anisotropy 8.
fn canvas_tex(
    graph: &mut SceneGraph,
    w: u32,
    h: u32,
    repeat: bool,
    srgb: bool,
    draw: impl FnOnce(&mut Canvas, f64, f64),
) -> TextureId {
    let mut c = Canvas::new(w, h);
    draw(&mut c, f64::from(w), f64::from(h));
    let t = Arc::new(Texture::from_canvas(&c, repeat, srgb, 8.0));
    let desc = t.desc("", 0);
    graph.add_texture(Image::Own(t), desc)
}

/// The plain-painted surfaces (walls, trim, wood) share one vertex-coloured
/// material. A world-space triplanar grain gives the stucco and boards some
/// tooth, and a broad mottling breaks up the colour, so big pastel walls
/// don't read as flat fills (kind `Stucco`).
fn stucco_material(graph: &mut SceneGraph) -> Material {
    let noise = canvas_tex(graph, 128, 128, true, false, |g, w, h| {
        let (wi, hi) = (w as u32, h as u32);
        let mut img = g.create_image_data(wi, hi);
        let mut rng = Mulberry32::new(19);
        let lat: Vec<f64> = (0..256).map(|_| f64::from(rng.next_f64() as f32)).collect();
        for y in 0..hi {
            for x in 0..wi {
                let fx = f64::from(x) / 8.0;
                let fy = f64::from(y) / 8.0;
                let x0 = fx.floor();
                let y0 = fy.floor();
                let tx = fx - x0;
                let ty = fy - y0;
                let at = |i: f64, j: f64| {
                    lat[((js::to_int32(j) & 15) * 16 + (js::to_int32(i) & 15)) as usize]
                };
                let broad = (at(x0, y0) * (1.0 - tx) + at(x0 + 1.0, y0) * tx) * (1.0 - ty)
                    + (at(x0, y0 + 1.0) * (1.0 - tx) + at(x0 + 1.0, y0 + 1.0) * tx) * ty;
                let v = 150.0 + broad * 60.0 + (rng.next_f64() - 0.5) * 50.0;
                let i = ((y * wi + x) * 4) as usize;
                img.set(i, v);
                img.set(i + 1, v);
                img.set(i + 2, v);
                img.set(i + 3, 255.0);
            }
        }
        g.put_image_data(&img, 0, 0);
    });
    Material::standard()
        .set("vertexColors", true)
        .set("roughness", 0.85)
        .kind(MaterialKind::Stucco, None)
        .program_key("beach-stucco")
        .uniform("tGrain", texture_value(noise))
        .uniform("clippingPlanes", Value::Null)
}

// ── The module ──────────────────────────────────────────────────────────

/// A house on the hill at the town entry, whose terrace `plan()` cuts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HillHouse {
    pub s: f64,
    pub lat: f64,
    pub x: f64,
    pub z: f64,
    pub yaw: f64,
    pub seed: u32,
}

/// The Seabright scenery module.
pub struct Beach {
    pub zone: usize,
    pub label: &'static str,
    pub zs0: f64,
    pub zs1: f64,
    pub pier_s: f64,
    pub marina_s: f64,
    pub town_s0: f64,
    pub town_s1: f64,
    /// Where the cross streets meet the boulevard.
    pub cross_s: Vec<f64>,
    pub hill_houses: Vec<HillHouse>,
    pub group: Option<NodeId>,
    pub palm_count: usize,
    pub car_count: usize,
    pub boat_count: usize,
}

impl Beach {
    /// `new Beach({ zone = 1 })`.
    pub fn new(info: &SceneryInfo) -> Beach {
        Beach {
            zone: info.zone,
            label: "Building Seabright",
            zs0: 0.0,
            zs1: 0.0,
            pier_s: 0.0,
            marina_s: 0.0,
            town_s0: 0.0,
            town_s1: 0.0,
            cross_s: Vec::new(),
            hill_houses: Vec::new(),
            group: None,
            palm_count: 0,
            car_count: 0,
            boat_count: 0,
        }
    }

    /// `setupRanges()`: the town's stretch of road, the pier and marina,
    /// the cross streets, and the kerb railing opened where each meets the
    /// boulevard.
    fn setup_ranges(&mut self, t: &mut Track) {
        let z = self.zone;
        self.zs0 = t.zone_start[z] as f64;
        self.zs1 = if z + 1 < t.zone_start.len() {
            t.zone_start[z + 1] as f64
        } else {
            t.length
        };
        let (zs0, zs1) = (self.zs0, self.zs1);
        let tag = |n: &str| {
            t.tags
                .iter()
                .find(|g| g.tag == n && g.s0 >= zs0 - 5.0 && g.s0 < zs1)
                .map(|g| (g.s0, g.s1))
        };
        let mid = |g: Option<(f64, f64)>, d: f64| g.map_or(d, |g| js::round((g.0 + g.1) / 2.0));
        self.pier_s = mid(tag("pier"), zs0 + 700.0);
        self.marina_s = mid(tag("marina"), zs1 - 300.0);
        self.town_s0 = tag("promenade").map_or(zs0 + 420.0, |g| g.0) + 50.0;
        self.town_s1 = zs1 - 70.0;
        let mut cs = Vec::new();
        for k in -1..6 {
            let s = self.pier_s + f64::from(k) * 257.0;
            if s > self.town_s0 + 30.0 && s < self.town_s1 - 25.0 {
                cs.push(s);
            }
        }
        // Open the kerb railing where each cross street meets the boulevard.
        for &c in &cs {
            t.fence_gaps.push(mp_track::FenceGap {
                s0: c - 5.5,
                s1: c + 5.5,
                side: 1.0,
            });
        }
        self.cross_s = cs;
    }
}

impl Scenery for Beach {
    fn name(&self) -> &str {
        "Beach"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    /// Terraces for the houses on the hill at the town entry.
    fn plan(&mut self, world: &mut World) -> Result<(), String> {
        let t = world.track.as_mut().ok_or("the route is surveyed first")?;
        self.setup_ranges(t);
        let t = world.track.as_ref().ok_or("the route is surveyed first")?;
        let terrain = world.terrain.as_mut().ok_or("no terrain")?;
        let mut rng = Mulberry32::new(8812);
        self.hill_houses = Vec::new();
        let mut s = self.zs0 + 40.0;
        while s < self.town_s0 - 40.0 {
            let f = t.frame(s);
            let lat = f.wall_r + rrange(&mut rng, 32.0, 60.0);
            let p = t.point_at(s, lat);
            let seed = (rng.next_f64() * 1e9).floor() as u32;
            self.hill_houses.push(HillHouse {
                s,
                lat,
                x: p.x,
                z: p.z,
                yaw: yaw_z(-f.rx, -f.rz),
                seed,
            });
            terrain.add_flatten(p.x, p.z, 10.0, 9.0, None);
            s += rrange(&mut rng, 55.0, 80.0);
        }
        Ok(())
    }

    fn build(&mut self, world: &mut World) -> Result<(), String> {
        let World {
            track,
            terrain,
            graph,
            textures,
            root,
            animators,
            ..
        } = world;
        let track = track.as_ref().ok_or("the route is surveyed first")?;
        let terrain = terrain.as_ref().ok_or("no terrain")?;
        let group = graph.group("beach");
        let mut b = Bld {
            t: track,
            ground: Ground::new(terrain),
            beach: self,
            graph,
            textures,
            group,
            b: Builder::new_color(PALETTE),
            ribbons: Vec::new(),
            rng: Mulberry32::new(4210),
            occ: Vec::new(),
            palms: Vec::new(),
            cars: Vec::new(),
            pools: Vec::new(),
            m: Vec::new(),
            sg: None,
            nn: None,
            streets: Vec::new(),
            back_lats: Vec::new(),
            prom_in: 0.0,
            prom_out: 0.0,
            front_lat: 0.0,
            bulb_mat: None,
            ferris: None,
            coaster: None,
            foam_mats: Vec::new(),
            pool_mat: None,
        };
        b.make_materials();
        b.make_signs();

        b.reserve_streets();
        b.build_promenade();
        b.build_frontage();
        b.b.set_channel("far"); // houses behind main street: no shadow casting
        b.build_residential();
        b.build_hill_houses();
        b.b.set_channel("near");
        b.build_street_furniture();
        b.build_sidewalk_props();
        b.build_street_parking();
        b.build_beach_props();
        let boats = b.build_pier_and_marina();
        b.build_traffic_lights();
        b.build_town_sign();
        b.build_foam();

        let mats: Vec<(&str, MaterialId)> = b.m.iter().map(|(k, m)| (*k, *m)).collect();
        let opts = BuildOpts {
            cast_shadow: CastShadow::Keys(
                ["metal", "awnRed", "awnBlue", "awnYellow", "awnGreen"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
            receive_shadow: true,
        };
        for mesh in b.b.build(b.graph, &mats, &opts) {
            b.graph.add(group, mesh);
        }
        b.merge_ribbons();
        b.build_palms();
        b.build_cars();
        b.build_pools();

        let night = [
            (b.mat_of("glassLit"), 0.0, 1.5),
            (b.mat_of("lampGlow"), 0.15, 4.5),
            (b.mat_of("neon"), 0.55, 3.2),
            (b.mat_of("signs"), 0.0, 0.3),
            (b.mat_of("canopyLight"), 0.4, 2.6),
            (b.bulb_mat, 0.6, 5.0),
            (b.mat_of("pool"), 0.05, 0.6),
        ];
        let (sig_green, sig_amber, sig_red) = (
            b.mat_of("sigGreen").expect("sigGreen"),
            b.mat_of("sigAmber").expect("sigAmber"),
            b.mat_of("sigRed").expect("sigRed"),
        );
        let anim = Animate {
            time: 0.0,
            ferris: b.ferris.take(),
            coaster: b.coaster.take(),
            foam_mats: std::mem::take(&mut b.foam_mats),
            pool_mat: b.pool_mat,
            signal_t: 0.0,
            sig_green,
            sig_amber,
            sig_red,
        };
        let (palms, cars) = (b.palms.len(), b.cars.len());
        drop(b);
        graph.add(*root, group);
        for (mat, day, nt) in night {
            graph.add_night(mat, "emissiveIntensity", day, nt);
        }
        animators.push(Box::new(anim));
        self.palm_count = palms;
        self.car_count = cars;
        self.boat_count = boats;
        self.group = Some(group);
        Ok(())
    }
}

// ── Build state ─────────────────────────────────────────────────────────

/// `this.sg`: the painted sign atlas's cells.
struct Signs {
    surf: Rect,
    icecream: Rect,
    bikes: Rect,
    cafe: Rect,
    swim: Rect,
    pizza: Rect,
    pharmacy: Rect,
    supply: Rect,
    bait: Rect,
    records: Rect,
    tacos: Rect,
    motel_board: Rect,
    gas: Rect,
    price: Rect,
    pier: Rect,
    welcome: Rect,
    tower: Vec<Rect>,
    marina: Rect,
    blades: Vec<Rect>,
}

/// `this.nn`: the neon atlas's cells.
struct Neons {
    vacancy: Rect,
    diner: Rect,
    inn: Rect,
    arcade: Rect,
}

/// A frame at (s, lat) on the rendered ground (`frameAt`).
#[derive(Clone, Copy, Debug)]
struct Fr {
    x: f64,
    z: f64,
    y: f64,
    yaw: f64,
}

#[derive(Clone, Copy, Debug)]
struct Palm {
    x: f64,
    z: f64,
    y: f64,
    h: f64,
    yaw: f64,
    s: f64,
    c: f64,
}

#[derive(Clone, Debug)]
struct Car {
    x: f64,
    z: f64,
    y: f64,
    yaw: f64,
    kind: &'static str,
    color: u32,
    seed: u32,
}

/// A frontage lot: `{ w, d, build }`.
#[derive(Clone, Copy, Debug)]
enum Lot {
    Shop { rect: Rect, w: f64 },
    Surf,
    Taco,
    Diner,
    Motel,
    Gas,
    Hotel,
    Parking,
    House { w: f64 },
}

impl Lot {
    fn w(&self) -> f64 {
        match *self {
            Lot::Shop { w, .. } => w,
            Lot::Surf => 16.0,
            Lot::Taco => 12.0,
            Lot::Diner => 26.0,
            Lot::Motel => 46.0,
            Lot::Gas => 38.0,
            Lot::Hotel => 40.0,
            Lot::Parking => 26.0,
            Lot::House { w } => w,
        }
    }

    fn d(&self) -> f64 {
        match *self {
            Lot::Shop { .. } => 16.0,
            Lot::Surf => 15.0,
            Lot::Taco => 8.0,
            Lot::Diner => 26.0,
            Lot::Motel => 34.0,
            Lot::Gas => 30.0,
            Lot::Hotel => 18.0,
            Lot::Parking => 24.0,
            Lot::House { .. } => 16.0,
        }
    }
}

/// The Ferris wheel's moving parts (`this.ferris`).
struct Ferris {
    wheel: NodeId,
    gondolas: NodeId,
    r: f64,
    n: usize,
    angle: f64,
}

/// The coaster and its train (`this.coaster`).
struct Coaster {
    curve: CatmullRomCurve3,
    train: NodeId,
    u: f64,
    len: f64,
}

/// What `this` holds while Seabright is built.
struct Bld<'a> {
    t: &'a Track,
    ground: Ground<'a>,
    beach: &'a Beach,
    graph: &'a mut SceneGraph,
    textures: &'a mut TextureCache,
    group: NodeId,
    /// `this.B`: the ColorBuilder.
    b: Builder,
    /// `this.ribbons`: material → (name, geometries), in first-use order.
    ribbons: Vec<(MaterialId, String, Vec<BufferGeometry>)>,
    rng: Mulberry32,
    /// Occupied discs `{ x, z, r }`.
    occ: Vec<(f64, f64, f64)>,
    palms: Vec<Palm>,
    cars: Vec<Car>,
    /// Road light pools.
    pools: Vec<[f64; 3]>,
    /// `this.M`: the materials by key, in the JS object's order.
    m: Vec<(&'static str, MaterialId)>,
    sg: Option<Signs>,
    nn: Option<Neons>,
    /// The cross streets: `{ s, pts }`.
    streets: Vec<(f64, Vec<(f64, f64)>)>,
    back_lats: Vec<f64>,
    prom_in: f64,
    prom_out: f64,
    front_lat: f64,
    bulb_mat: Option<MaterialId>,
    ferris: Option<Ferris>,
    coaster: Option<Coaster>,
    foam_mats: Vec<(MaterialId, TextureId)>,
    pool_mat: Option<MaterialId>,
}

/// `stripAlong`'s options.
#[derive(Clone, Copy, Debug)]
struct StripOpts {
    step: f64,
    lift: f64,
    u_scale: f64,
    v_scale: f64,
    name: &'static str,
}

impl Default for StripOpts {
    fn default() -> Self {
        StripOpts {
            step: 4.0,
            lift: 0.12,
            u_scale: 4.0,
            v_scale: 4.0,
            name: "strip",
        }
    }
}

fn boxg(w: f64, h: f64, d: f64) -> BufferGeometry {
    box_geometry(w, h, d, 1.0, 1.0, 1.0)
}

fn cyl(rt: f64, rb: f64, h: f64, radial: f64) -> BufferGeometry {
    cylinder_geometry(rt, rb, h, radial, 1.0, false, 0.0, PI * 2.0)
}

impl Bld<'_> {
    fn gy(&self, x: f64, z: f64) -> f64 {
        self.ground.height(x, z)
    }

    fn mat_of(&self, key: &str) -> Option<MaterialId> {
        self.m.iter().find(|(k, _)| *k == key).map(|&(_, m)| m)
    }

    fn mat(&mut self, m: Material) -> MaterialId {
        self.graph.add_material(m)
    }

    fn geo(&mut self, g: BufferGeometry) -> GeoId {
        self.graph.add_geometry(g)
    }

    fn sg(&self) -> &Signs {
        self.sg.as_ref().expect("the signs are made first")
    }

    fn nn(&self) -> &Neons {
        self.nn.as_ref().expect("the signs are made first")
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    fn free(&self, x: f64, z: f64, r: f64) -> bool {
        for &(ox, oz, or) in &self.occ {
            let dx = x - ox;
            let dz = z - oz;
            if dx * dx + dz * dz < (r + or) * (r + or) {
                return false;
            }
        }
        self.clear_of_road(x, z, r)
    }

    fn take(&mut self, x: f64, z: f64, r: f64) {
        self.occ.push((x, z, r));
    }

    /// Keep everything outside the driving corridor (with room to spare).
    fn clear_of_road(&self, x: f64, z: f64, r: f64) -> bool {
        let t = self.t;
        let k = t.nearest(x, z, r + 40.0);
        if k < 0 {
            return true;
        }
        let p = t.project_window(x, z, k as f64, 8);
        let wall = if p.lat > 0.0 {
            f64::from(t.wall_r[p.i])
        } else {
            f64::from(t.wall_l[p.i])
        };
        p.lat.abs() - r > wall + 0.6
    }

    /// Frame at (s, lat): origin on the rendered ground, +Z facing the road
    /// (`road`) or away from it, X along the road.
    fn frame_at(&self, s: f64, lat: f64, road: bool) -> (Fr, Frame) {
        let f = self.t.frame(s);
        let p = self.t.point_at(s, lat);
        let side = js::or(js::sign(lat), 1.0);
        let k = if road { -side } else { side };
        (
            Fr {
                x: p.x,
                z: p.z,
                y: self.gy(p.x, p.z),
                yaw: yaw_z(k * f.rx, k * f.rz),
            },
            f,
        )
    }

    fn to_world(fr: &Fr, lx: f64, lz: f64) -> (f64, f64) {
        let c = kernel::cos(fr.yaw);
        let s = kernel::sin(fr.yaw);
        (fr.x + lx * c + lz * s, fr.z - lx * s + lz * c)
    }

    /// Ribbon following the road between two lateral offsets.
    fn strip_along(
        &mut self,
        s0: f64,
        s1: f64,
        lat0: f64,
        lat1: f64,
        mat: MaterialId,
        o: StripOpts,
    ) {
        let t = self.t;
        let mut pos = Vec::new();
        let mut uv = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let lats = [lat0, (lat0 + lat1) / 2.0, lat1];
        let mut rows = 0u32;
        let mut s = s0;
        loop {
            let ss = js::min(s, s1);
            for lat in lats {
                let p = t.point_at(ss, lat);
                pos.extend_from_slice(&[p.x, self.gy(p.x, p.z) + o.lift, p.z]);
                uv.extend_from_slice(&[(lat - lat0) / o.u_scale, ss / o.v_scale]);
            }
            rows += 1;
            if ss >= s1 {
                break;
            }
            s += o.step;
        }
        for r in 0..rows.saturating_sub(1) {
            for c in 0..2 {
                let a = r * 3 + c;
                let b = a + 1;
                let d = a + 3;
                let e = d + 1;
                idx.extend_from_slice(&[a, b, d, b, e, d]); // counter-clockwise from above
            }
        }
        self.add_ribbon_mesh(&pos, &uv, &idx, mat, o.name);
    }

    /// Ribbon along an arbitrary path (`ribbon(path, width, mat, { lift =
    /// 0.1, name, yFn })`).
    fn ribbon(
        &mut self,
        path: &[(f64, f64)],
        width: f64,
        mat: MaterialId,
        lift: f64,
        name: &'static str,
        y_fn: Option<&dyn Fn(usize) -> f64>,
    ) {
        let mut pos = Vec::new();
        let mut uv = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let mut along = 0.0;
        let n = path.len();
        for i in 0..n {
            let p = path[i];
            let a = path[i.saturating_sub(1)];
            let b = path[(i + 1).min(n - 1)];
            let mut dx = b.0 - a.0;
            let mut dz = b.1 - a.1;
            let l = js::or(kernel::hypot(dx, dz), 1.0);
            dx /= l;
            dz /= l;
            if i > 0 {
                along += kernel::hypot(p.0 - path[i - 1].0, p.1 - path[i - 1].1);
            }
            for sgn in [-1.0, 1.0] {
                let x = p.0 - dz * sgn * width / 2.0;
                let z = p.1 + dx * sgn * width / 2.0;
                let y = match y_fn {
                    Some(f) => f(i),
                    None => self.gy(x, z),
                };
                pos.extend_from_slice(&[x, y + lift, z]);
                uv.extend_from_slice(&[sgn * 0.5 + 0.5, along / 4.0]);
            }
            if i > 0 {
                let k = (i * 2) as u32;
                idx.extend_from_slice(&[k - 2, k - 1, k, k - 1, k + 1, k]);
            }
        }
        self.add_ribbon_mesh(&pos, &uv, &idx, mat, name);
    }

    fn add_ribbon_mesh(
        &mut self,
        pos: &[f64],
        uv: &[f64],
        idx: &[u32],
        mat: MaterialId,
        name: &str,
    ) {
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(pos, 3));
        g.set_attribute("uv", BufferAttribute::from_f64(uv, 2));
        g.set_index(idx);
        g.compute_vertex_normals();
        let n = g.get_attribute_mut("normal").expect("normal");
        for i in 0..n.count() {
            if n.get_y(i) < 0.0 {
                let (x, y, z) = (n.get_x(i), n.get_y(i), n.get_z(i));
                n.set_xyz(i, -x, -y, -z);
            }
        }
        match self.ribbons.iter_mut().find(|(m, _, _)| *m == mat) {
            Some((_, _, geos)) => geos.push(g),
            None => self.ribbons.push((mat, name.to_string(), vec![g])),
        }
    }

    /// `mergeList(geos)`: through a plain Builder's `add`, as the JS does.
    fn merge_list(geos: &[BufferGeometry]) -> Option<BufferGeometry> {
        let mut b = Builder::new();
        for g in geos {
            b.add("x", g, None);
        }
        b.merge_all()
    }

    /// One mesh per ribbon material (streets, sidewalk, boardwalk, foam).
    fn merge_ribbons(&mut self) {
        for (mat, name, geos) in std::mem::take(&mut self.ribbons) {
            let mut g = if geos.len() == 1 {
                geos.into_iter().next().expect("one")
            } else {
                let flat: Vec<BufferGeometry> = geos.iter().map(|x| x.to_non_indexed()).collect();
                Self::merge_list(&flat).expect("the ribbons merge")
            };
            g.compute_bounding_sphere();
            let transparent =
                self.graph.material(mat).get("transparent") == Some(&Value::Bool(true));
            let g = self.geo(g);
            let m = self.graph.mesh(g, mat);
            let o = self.graph.get_mut(m);
            o.name = format!("beach:{name}");
            o.receive_shadow = true;
            o.matrix_auto_update = false;
            if transparent {
                o.render_order = 3.0;
            }
            self.graph.add(self.group, m);
        }
    }

    // ── Materials and signage ───────────────────────────────────────────

    fn make_materials(&mut self) {
        // `S(color, o)`: MeshStandardMaterial({ color, roughness: 0.85,
        // metalness: 0, ...o }).
        let s = |color: u32| {
            Material::standard()
                .set("color", color)
                .set("roughness", 0.85)
                .set("metalness", 0.0)
        };
        let stripes = |graph: &mut SceneGraph, a: &str, b: &str| {
            let (a, b) = (a.to_string(), b.to_string());
            canvas_tex(graph, 64, 64, true, true, move |g, w, h| {
                for i in 0..8 {
                    g.set_fill_style(if i % 2 == 1 { b.as_str() } else { a.as_str() });
                    g.fill_rect(f64::from(i) * w / 8.0, 0.0, w / 8.0, h);
                }
            })
        };
        let planks = canvas_tex(self.graph, 256, 256, true, true, |g, w, h| {
            g.set_fill_style("#9a7650");
            g.fill_rect(0.0, 0.0, w, h);
            let mut rng = Mulberry32::new(3);
            let mut y = 0.0;
            while y < h {
                let r = 130.0 + rng.next_f64() * 40.0;
                let gg = 98.0 + rng.next_f64() * 30.0;
                let b = 64.0 + rng.next_f64() * 22.0;
                g.set_fill_style(format!("rgb({r},{gg},{b})"));
                g.fill_rect(0.0, y, w, 14.0);
                g.set_fill_style("rgba(40,25,15,0.55)");
                g.fill_rect(0.0, y + 14.0, w, 2.0);
                g.set_fill_style("rgba(40,25,15,0.35)");
                g.fill_rect((rng.next_f64() * w).floor(), y, 2.0, 14.0);
                y += 16.0;
            }
        });
        let paving = canvas_tex(self.graph, 128, 128, true, true, |g, w, h| {
            g.set_fill_style("#c9c3b6");
            g.fill_rect(0.0, 0.0, w, h);
            g.set_stroke_style("rgba(80,70,60,0.35)");
            g.set_line_width(2.0);
            let mut k = 0.0;
            while k <= w {
                g.begin_path();
                g.move_to(k, 0.0);
                g.line_to(k, h);
                g.stroke();
                g.begin_path();
                g.move_to(0.0, k);
                g.line_to(w, k);
                g.stroke();
                k += 32.0;
            }
            let mut rng = Mulberry32::new(9);
            for _ in 0..400 {
                let c = if rng.next_f64() < 0.5 {
                    "255,255,255"
                } else {
                    "0,0,0"
                };
                g.set_fill_style(format!("rgba({c},0.05)"));
                let x = rng.next_f64() * w;
                let y = rng.next_f64() * h;
                g.fill_rect(x, y, 2.0, 2.0);
            }
        });
        let asphalt = canvas_tex(self.graph, 128, 128, true, true, |g, w, h| {
            g.set_fill_style("#3f4044");
            g.fill_rect(0.0, 0.0, w, h);
            let mut rng = Mulberry32::new(12);
            for _ in 0..1500 {
                let v = 50.0 + rng.next_f64() * 40.0;
                g.set_fill_style(format!("rgb({v},{v},{})", v + 3.0));
                let x = rng.next_f64() * w;
                let y = rng.next_f64() * h;
                g.fill_rect(x, y, 1.5, 1.5);
            }
            g.set_fill_style("rgba(235,190,60,0.9)");
            g.fill_rect(w / 2.0 - 2.0, 0.0, 4.0, h * 0.55);
        });
        // Window panes: a painted frame and glazing bars around a sky
        // reflection, so every glass box reads as a window. The lit variant
        // glows only through the panes.
        let pane = |graph: &mut SceneGraph, lit: bool| {
            canvas_tex(graph, 64, 64, false, true, move |g, w, h| {
                if lit {
                    g.set_fill_style("#000");
                    g.fill_rect(0.0, 0.0, w, h);
                } else {
                    let mut grd = g.create_linear_gradient(0.0, 0.0, w * 0.4, h);
                    grd.add_color_stop(0.0, "#9fb8c8");
                    grd.add_color_stop(0.45, "#5f7888");
                    grd.add_color_stop(0.5, "#7d97a8");
                    grd.add_color_stop(1.0, "#3a4a58");
                    g.set_fill_style(&grd);
                    g.fill_rect(0.0, 0.0, w, h);
                }
                let fr = if lit { "#000" } else { "#eeeae0" };
                g.set_fill_style(if lit { "#ffd9a0" } else { "rgba(0,0,0,0)" });
                if lit {
                    g.fill_rect(5.0, 5.0, w - 10.0, h - 10.0);
                }
                g.set_fill_style(fr);
                g.fill_rect(0.0, 0.0, w, 5.0);
                g.fill_rect(0.0, h - 5.0, w, 5.0);
                g.fill_rect(0.0, 0.0, 5.0, h);
                g.fill_rect(w - 5.0, 0.0, 5.0, h);
                g.fill_rect(w / 2.0 - 2.0, 0.0, 4.0, h);
                g.fill_rect(0.0, h * 0.45 - 2.0, w, 4.0);
            })
        };
        let mut m: Vec<(&'static str, Material)> = Vec::new();
        for (k, c) in [
            ("wPink", 0xe9aea8),
            ("wMint", 0xa9d8c1),
            ("wYellow", 0xf1d690),
            ("wBlue", 0x9fc3de),
            ("wWhite", 0xefebe1),
            ("wPeach", 0xf1bf95),
            ("wLilac", 0xc6b6de),
            ("wTeal", 0x6db8b1),
            ("wSand", 0xdcc9a2),
            ("trim", 0xf6f3ea),
            ("concrete", 0xc4bdb0),
        ] {
            m.push((k, s(c)));
        }
        m.push(("white", s(0xf2f2ee).set("roughness", 0.6)));
        let p0 = pane(self.graph, false);
        m.push((
            "glass",
            s(0xffffff)
                .set("map", p0)
                .set("roughness", 0.2)
                .set("metalness", 0.1),
        ));
        let p1 = pane(self.graph, false);
        let p2 = pane(self.graph, true);
        m.push((
            "glassLit",
            s(0x8a8478)
                .set("map", p1)
                .set("roughness", 0.2)
                .set("metalness", 0.1)
                .set("emissive", 0xffc98a)
                .set("emissiveMap", p2)
                .set("emissiveIntensity", 0.0),
        ));
        m.push((
            "balGlass",
            s(0x9fc6d4)
                .set("roughness", 0.1)
                .set("metalness", 0.3)
                .set("transparent", true)
                .set("opacity", 0.45),
        ));
        m.push(("wood", s(0x9c7651)));
        m.push(("woodDark", s(0x5a3f2b)));
        for (k, a) in [
            ("awnRed", "#d8433f"),
            ("awnBlue", "#2f78b8"),
            ("awnYellow", "#f2c233"),
            ("awnGreen", "#3f9e6a"),
        ] {
            let t = stripes(self.graph, a, "#f6f1e6");
            m.push((k, s(0xffffff).set("map", t)));
        }
        m.push(("roofTar", s(0x55524d)));
        m.push(("roofTile", s(0xbf6a4c)));
        m.push((
            "metal",
            s(0x9a9fa4).set("metalness", 0.7).set("roughness", 0.35),
        ));
        m.push(("black", s(0x1b1b1d).set("roughness", 0.6)));
        m.push(("boardB", s(0xff8a3c).set("roughness", 0.35)));
        m.push(("lawn", s(0x6f9a45).set("roughness", 1.0)));
        m.push(("hedge", s(0x3f6a32).set("roughness", 1.0)));
        m.push(("paintRed", s(0xc83a34).set("roughness", 0.5)));
        m.push((
            "paintWhite",
            s(0xf0f0ea)
                .set("roughness", 0.7)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -2.0)
                .set("polygonOffsetUnits", -2.0),
        ));
        m.push((
            "pool",
            s(0x3fc7d6)
                .set("roughness", 0.1)
                .set("emissive", 0x2aa6c0)
                .set("emissiveIntensity", 0.05),
        ));
        m.push((
            "asphaltLot",
            s(0x4a4b4f)
                .set("roughness", 0.95)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -1.0)
                .set("polygonOffsetUnits", -1.0),
        ));
        m.push((
            "canopyLight",
            s(0xffffff)
                .set("emissive", 0xf4f8ff)
                .set("emissiveIntensity", 0.4),
        ));
        m.push((
            "lampGlow",
            s(0xfff4dc)
                .set("emissive", 0xffd9a0)
                .set("emissiveIntensity", 0.15),
        ));
        m.push(("hullWhite", s(0xf1f2ee).set("roughness", 0.4)));
        m.push(("sailBlue", s(0x2b4f86)));
        for (k, c, e) in [
            ("sigRed", 0x300808, 0xff2a1a),
            ("sigAmber", 0x302008, 0xffa020),
            ("sigGreen", 0x08301a, 0x30ff90),
        ] {
            m.push((k, s(c).set("emissive", e).set("emissiveIntensity", 0.0)));
        }
        let stucco = stucco_material(self.graph);
        m.push(("solid", stucco));
        m.push((
            "deckPlank",
            s(0xffffff).set("map", planks).set("roughness", 0.9),
        ));
        m.push((
            "paving",
            s(0xffffff)
                .set("map", paving)
                .set("roughness", 0.95)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -1.0)
                .set("polygonOffsetUnits", -2.0),
        ));
        m.push((
            "street",
            s(0xffffff)
                .set("map", asphalt)
                .set("roughness", 0.95)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -2.0)
                .set("polygonOffsetUnits", -3.0),
        ));
        self.m = m
            .into_iter()
            .map(|(k, mat)| (k, self.graph.add_material(mat)))
            .collect();
    }

    fn make_signs(&mut self) {
        let mut a = SignAtlas::new(2048, Some("#ffffff"));
        let mut n = SignAtlas::new(1024, Some("#0b0a10"));
        let p = |a: &mut SignAtlas, text: &str, o: PaintedOpts| {
            a.add(512.0, 128.0, painted_sign(text, o))
        };
        let d = PaintedOpts::default;
        let surf = p(
            &mut a,
            "SURF SHOP",
            PaintedOpts {
                bg: "#1f7f8c",
                fg: "#fff4d8",
                font: "bold 84px \"Arial Black\", Arial",
                stripe: Some("#f2c233"),
                ..d()
            },
        );
        let icecream = p(
            &mut a,
            "ICE CREAM",
            PaintedOpts {
                bg: "#f7d6e3",
                fg: "#c2346c",
                font: "bold 80px \"Brush Script MT\", cursive",
                ..d()
            },
        );
        let bikes = p(
            &mut a,
            "BIKE RENTALS",
            PaintedOpts {
                bg: "#f6efe0",
                fg: "#2f6aa8",
                border: Some("#2f6aa8"),
                ..d()
            },
        );
        let cafe = p(
            &mut a,
            "BOARDWALK CAFE",
            PaintedOpts {
                bg: "#3b2a20",
                fg: "#f3dfb8",
                font: "bold 70px Georgia, serif",
                ..d()
            },
        );
        let swim = p(
            &mut a,
            "SWIMWEAR",
            PaintedOpts {
                bg: "#ffffff",
                fg: "#e24d6c",
                font: "bold 84px \"Arial Black\", Arial",
                ..d()
            },
        );
        let pizza = p(
            &mut a,
            "SLICE OF LIFE PIZZA",
            PaintedOpts {
                bg: "#1c6b3a",
                fg: "#fff",
                stripe: Some("#c83a34"),
                ..d()
            },
        );
        let pharmacy = p(
            &mut a,
            "PHARMACY",
            PaintedOpts {
                bg: "#ffffff",
                fg: "#1b7a3a",
                border: Some("#1b7a3a"),
                ..d()
            },
        );
        let supply = p(
            &mut a,
            "MARINA SUPPLY",
            PaintedOpts {
                bg: "#1d3f75",
                fg: "#fff",
                stripe: Some("#e8e6df"),
                ..d()
            },
        );
        let bait = p(
            &mut a,
            "BAIT & TACKLE",
            PaintedOpts {
                bg: "#e8d9a8",
                fg: "#23466e",
                font: "bold 74px Georgia, serif",
                ..d()
            },
        );
        let records = p(
            &mut a,
            "WAVE RECORDS",
            PaintedOpts {
                bg: "#141414",
                fg: "#ffcf4d",
                ..d()
            },
        );
        let tacos = a.add(
            384.0,
            128.0,
            painted_sign(
                "TACOS",
                PaintedOpts {
                    bg: "#f2c233",
                    fg: "#c83a34",
                    font: "bold 96px \"Arial Black\", Arial",
                    sub: None,
                    ..d()
                },
            ),
        );
        let motel_board = p(
            &mut a,
            "SEA BREEZE MOTEL",
            PaintedOpts {
                bg: "#f6efe0",
                fg: "#1f7f8c",
                font: "bold 64px Georgia, serif",
                ..d()
            },
        );
        let gas = p(
            &mut a,
            "SEABRIGHT FUEL",
            PaintedOpts {
                bg: "#c83a34",
                fg: "#fff",
                ..d()
            },
        );
        let price = a.add(256.0, 256.0, |g, w, _h| {
            g.set_fill_style("#fff");
            g.fill_rect(0.0, 0.0, w, w);
            g.set_fill_style("#c83a34");
            g.fill_rect(0.0, 0.0, w, 64.0);
            g.set_fill_style("#fff");
            g.set_font("bold 44px Arial");
            g.set_text_align("center");
            g.set_text_baseline("middle");
            g.fill_text("FUEL", w / 2.0, 32.0);
            g.set_fill_style("#111");
            g.set_font("bold 40px \"Courier New\", monospace");
            for (i, l) in ["REG 4.39", "PLS 4.59", "DSL 4.79"].iter().enumerate() {
                g.fill_text(l, w / 2.0, 100.0 + i as f64 * 56.0);
            }
        });
        let pier = a.add(
            1024.0,
            160.0,
            painted_sign(
                "SEABRIGHT PIER",
                PaintedOpts {
                    bg: "#f6f1e6",
                    fg: "#1f5f8b",
                    font: "bold 110px \"Arial Black\", Arial",
                    stripe: Some("#d8433f"),
                    ..d()
                },
            ),
        );
        let welcome = a.add(768.0, 384.0, |g, w, h| {
            let mut grd = g.create_linear_gradient(0.0, 0.0, 0.0, h);
            grd.add_color_stop(0.0, "#ffb86b");
            grd.add_color_stop(0.55, "#ff7a8a");
            grd.add_color_stop(1.0, "#3fb6c8");
            g.set_fill_style(&grd);
            g.fill_rect(0.0, 0.0, w, h);
            g.set_fill_style("rgba(255,240,200,0.9)");
            g.begin_path();
            g.arc(w * 0.78, h * 0.46, 70.0, 0.0, PI * 2.0, false);
            g.fill();
            g.set_stroke_style("#fff");
            g.set_line_width(10.0);
            g.stroke_rect(10.0, 10.0, w - 20.0, h - 20.0);
            g.set_fill_style("#fff");
            g.set_text_align("center");
            g.set_text_baseline("middle");
            g.set_font("bold 42px Arial");
            g.fill_text("WELCOME TO", w / 2.0, 70.0);
            g.set_font("bold 120px \"Brush Script MT\", \"Segoe Script\", cursive");
            g.set_shadow_color("rgba(0,0,0,0.35)");
            g.set_shadow_blur(8.0);
            g.fill_text("Seabright", w / 2.0, 180.0);
            g.set_shadow_blur(0.0);
            g.set_font("bold 40px Arial");
            g.fill_text("pop. 4,210  ·  est. 1912", w / 2.0, 300.0);
        });
        let tower: Vec<Rect> = (1..=7)
            .map(|k| {
                a.add(128.0, 128.0, |g, w, h| {
                    g.set_fill_style("#c83a34");
                    g.fill_rect(0.0, 0.0, w, h);
                    g.set_fill_style("#fff");
                    g.set_font("bold 96px Arial");
                    g.set_text_align("center");
                    g.set_text_baseline("middle");
                    g.fill_text(&k.to_string(), w / 2.0, h / 2.0 + 4.0);
                })
            })
            .collect();
        let marina = p(
            &mut a,
            "SEABRIGHT MARINA",
            PaintedOpts {
                bg: "#f6f1e6",
                fg: "#1d3f75",
                stripe: Some("#1d3f75"),
                ..d()
            },
        );
        // Projecting blade signs: a word stacked letter by letter, or an
        // icon.
        let blades: Vec<Rect> = [
            ("OPEN", "#c83a34", "#fff"),
            ("SURF", "#1f7f8c", "#fff4d8"),
            ("EAT", "#f2c233", "#1b1b1d"),
            ("BAR", "#1d3f75", "#ffcf4d"),
            ("COFFEE", "#3b2a20", "#f3dfb8"),
            ("GIFTS", "#f7d6e3", "#c2346c"),
            ("CAFE", "#2f6d4a", "#fff"),
            ("TOYS", "#8f6ad8", "#fff"),
        ]
        .iter()
        .map(|&(txt, bg, fg)| {
            a.add(128.0, 224.0, |g, w, h| {
                g.set_fill_style(bg);
                g.fill_rect(0.0, 0.0, w, h);
                g.set_stroke_style(fg);
                g.set_line_width(6.0);
                g.stroke_rect(8.0, 8.0, w - 16.0, h - 16.0);
                g.set_fill_style(fg);
                g.set_text_align("center");
                g.set_text_baseline("middle");
                let upper = txt.len() > 1 && txt.chars().all(|c| c.is_ascii_uppercase());
                let chars: Vec<String> = if upper {
                    txt.chars().map(|c| c.to_string()).collect()
                } else {
                    vec![txt.to_string()]
                };
                let n = chars.len() as f64;
                let size = if chars.len() > 1 {
                    js::min(52.0, 180.0 / n)
                } else {
                    80.0
                };
                g.set_font(&format!("bold {size}px \"Arial Black\", Arial, sans-serif"));
                for (i, c) in chars.iter().enumerate() {
                    g.fill_text(
                        c,
                        w / 2.0,
                        h / 2.0 + (i as f64 - (n - 1.0) / 2.0) * size * 1.05,
                    );
                }
            })
        })
        .collect();
        let vacancy = n.add(
            512.0,
            160.0,
            neon_sign(
                "Vacancy",
                NeonOpts {
                    color: "#ff3b6b",
                    sub: Some("MOTEL · COLOR TV"),
                    sub_color: "#5ff2ff",
                    ..NeonOpts::default()
                },
            ),
        );
        let diner = n.add(
            512.0,
            170.0,
            neon_sign(
                "Starlite Diner",
                NeonOpts {
                    color: "#ff5fd2",
                    sub: Some("OPEN 24 HOURS"),
                    sub_color: "#ffd84d",
                    ..NeonOpts::default()
                },
            ),
        );
        let inn = n.add(
            1024.0,
            180.0,
            neon_sign(
                "Seabright Inn",
                NeonOpts {
                    color: "#5ff2ff",
                    font: "bold 120px \"Brush Script MT\", \"Segoe Script\", cursive",
                    sub: None,
                    frame: false,
                    ..NeonOpts::default()
                },
            ),
        );
        let arcade = n.add(
            512.0,
            150.0,
            neon_sign(
                "ARCADE",
                NeonOpts {
                    color: "#ffd84d",
                    font: "bold 104px \"Arial Black\", Arial",
                    sub: Some("SKEE-BALL · PINBALL"),
                    sub_color: "#ff5fd2",
                    ..NeonOpts::default()
                },
            ),
        );
        let at = a.texture(self.graph);
        let nt = n.texture(self.graph);
        let signs = self.mat(
            Material::standard()
                .set("map", at)
                .set("roughness", 0.6)
                .set("emissive", 0xffffff)
                .set("emissiveMap", at)
                .set("emissiveIntensity", 0.0),
        );
        let neon = self.mat(
            Material::standard()
                .set("map", nt)
                .set("color", 0xffffff)
                .set("roughness", 0.5)
                .set("emissive", 0xffffff)
                .set("emissiveMap", nt)
                .set("emissiveIntensity", 0.6),
        );
        self.m.push(("signs", signs));
        self.m.push(("neon", neon));
        self.sg = Some(Signs {
            surf,
            icecream,
            bikes,
            cafe,
            swim,
            pizza,
            pharmacy,
            supply,
            bait,
            records,
            tacos,
            motel_board,
            gas,
            price,
            pier,
            welcome,
            tower,
            marina,
            blades,
        });
        self.nn = Some(Neons {
            vacancy,
            diner,
            inn,
            arcade,
        });
    }

    // Streets are laid first so nothing is built across them.
    fn reserve_streets(&mut self) {
        let t = self.t;
        let cross = self.beach.cross_s.clone();
        for s in cross {
            let f = t.frame(s);
            let mut pts = Vec::new();
            let mut d = f.wall_r + 0.8;
            while d < 330.0 {
                if d > 300.0 {
                    break;
                }
                let x = f.x + f.rx * d;
                let z = f.z + f.rz * d;
                pts.push((x, z));
                self.take(x, z, 7.5);
                d += 5.0;
            }
            self.streets.push((s, pts));
        }
        // Back streets parallel to the boulevard.
        self.back_lats = vec![95.0, 187.0, 279.0];
        for lat in self.back_lats.clone() {
            let mut s = self.beach.town_s0 - 20.0;
            while s < self.beach.town_s1 + 20.0 {
                let p = t.point_at(s, lat);
                self.take(p.x, p.z, 5.5);
                s += 5.0;
            }
        }
    }

    // ── Sea side: boardwalk promenade ───────────────────────────────────

    fn build_promenade(&mut self) {
        let t = self.t;
        let (town_s0, town_s1, pier_s, marina_s) = (
            self.beach.town_s0,
            self.beach.town_s1,
            self.beach.pier_s,
            self.beach.marina_s,
        );
        let w = t.frame(pier_s).wall_l;
        self.prom_in = w + 0.45;
        self.prom_out = w + 6.8;
        let deck = self.mat_of("deckPlank").expect("deckPlank");
        self.strip_along(
            town_s0 - 30.0,
            town_s1 + 30.0,
            -self.prom_out,
            -self.prom_in,
            deck,
            StripOpts {
                lift: 0.22,
                u_scale: 6.35,
                v_scale: 3.0,
                name: "promenade",
                ..StripOpts::default()
            },
        );
        // Sand-side edge beam and a low timber rail, open at the beach stairs.
        let mut s = town_s0 - 30.0;
        while s < town_s1 + 30.0 {
            let cur = s;
            s += 4.0;
            let (fr, _) = self.frame_at(cur, -self.prom_out - 0.1, true);
            self.b.set_frame(fr.x, fr.y, fr.z, fr.yaw);
            self.b
                .box_("woodDark", 4.05, 0.35, 0.2, 0.0, -0.1, 0.0, 0.0, 0.0, 0.0);
            if (cur - pier_s).abs() < 10.0
                || (cur - marina_s).abs() < 8.0
                || ((cur - town_s0) % 120.0 + 120.0) % 120.0 < 4.0
            {
                continue;
            }
            self.b
                .box_("wood", 0.14, 1.0, 0.14, -2.0, 0.22, 0.02, 0.0, 0.0, 0.0);
            self.b
                .box_("wood", 4.05, 0.1, 0.16, 0.0, 1.12, 0.02, 0.0, 0.0, 0.0);
            self.b
                .box_("wood", 4.05, 0.08, 0.1, 0.0, 0.7, 0.02, 0.0, 0.0, 0.0);
        }
        // Lamps, benches and bins along the boardwalk.
        let mut s = town_s0;
        while s < town_s1 {
            let cur = s;
            s += 28.0;
            if (cur - pier_s).abs() < 12.0 {
                continue;
            }
            let (fr, _) = self.frame_at(cur, -(self.prom_out - 0.6), false);
            self.b.set_frame(fr.x, fr.y + 0.22, fr.z, fr.yaw);
            prom_lamp(&mut self.b, 0.0, 0.0);
            let (fr, _) = self.frame_at(cur + 14.0, -(self.prom_out - 1.2), false);
            self.b.set_frame(fr.x, fr.y + 0.22, fr.z, fr.yaw);
            bench(&mut self.b, 0.0, 0.0, 0.0);
            if self.rng.next_f64() < 0.5 {
                trash_can(&mut self.b, 1.6, 0.2);
            }
        }
    }

    // ── Town side: storefronts along the boulevard ──────────────────────

    fn lot_shop(&mut self, rect: Rect, w: Option<f64>) -> Lot {
        let w = w.unwrap_or_else(|| rrange(&mut self.rng, 11.0, 14.0));
        Lot::Shop { rect, w }
    }

    fn lot_house(&mut self) -> Lot {
        Lot::House {
            w: rrange(&mut self.rng, 13.0, 16.0),
        }
    }

    fn build_frontage(&mut self) {
        let t = self.t;
        let f0 = t.frame(self.beach.pier_s);
        let w = f0.wall_r;
        let paving = self.mat_of("paving").expect("paving");
        self.strip_along(
            self.beach.town_s0 - 40.0,
            self.beach.town_s1 + 40.0,
            w + 0.4,
            w + 5.9,
            paving,
            StripOpts {
                lift: 0.14,
                u_scale: 4.0,
                v_scale: 4.0,
                name: "sidewalk",
                ..StripOpts::default()
            },
        );
        self.front_lat = w + 6.2;
        let sg = |me: &Self, f: fn(&Signs) -> Rect| f(me.sg());
        let mut blocks: Vec<Vec<Lot>> = Vec::new();
        {
            let surf = Lot::Surf;
            let icecream = self.lot_shop(sg(self, |s| s.icecream), None);
            let taco = Lot::Taco;
            let bikes = self.lot_shop(sg(self, |s| s.bikes), None);
            let cafe = self.lot_shop(sg(self, |s| s.cafe), Some(14.0));
            let parking = Lot::Parking;
            let records = self.lot_shop(sg(self, |s| s.records), None);
            let house = self.lot_house();
            blocks.push(vec![
                surf, icecream, taco, bikes, cafe, parking, records, house,
            ]);
        }
        {
            let hotel = Lot::Hotel;
            let swim = self.lot_shop(sg(self, |s| s.swim), None);
            let pizza = self.lot_shop(sg(self, |s| s.pizza), Some(15.0));
            let parking = Lot::Parking;
            let h1 = self.lot_house();
            let h2 = self.lot_house();
            blocks.push(vec![hotel, swim, pizza, parking, h1, h2]);
        }
        {
            let motel = Lot::Motel;
            let diner = Lot::Diner;
            let h = self.lot_house();
            let icecream = self.lot_shop(sg(self, |s| s.icecream), None);
            blocks.push(vec![motel, diner, h, icecream]);
        }
        {
            let gas = Lot::Gas;
            let pharmacy = self.lot_shop(sg(self, |s| s.pharmacy), Some(15.0));
            let h1 = self.lot_house();
            let h2 = self.lot_house();
            let cafe = self.lot_shop(sg(self, |s| s.cafe), None);
            blocks.push(vec![gas, pharmacy, h1, h2, cafe]);
        }
        {
            let supply = self.lot_shop(sg(self, |s| s.supply), Some(16.0));
            let bait = self.lot_shop(sg(self, |s| s.bait), None);
            let h1 = self.lot_house();
            let h2 = self.lot_house();
            let h3 = self.lot_house();
            blocks.push(vec![supply, bait, h1, h2, h3]);
        }
        {
            let h1 = self.lot_house();
            let h2 = self.lot_house();
            let h3 = self.lot_house();
            let h4 = self.lot_house();
            blocks.push(vec![h1, h2, h3, h4]);
        }
        let mut edges = vec![self.beach.town_s0];
        edges.extend(self.beach.cross_s.iter().copied());
        edges.push(self.beach.town_s1);
        for b in 0..edges.len() - 1 {
            let a = edges[b] + if b == 0 { 0.0 } else { 9.0 };
            let z = edges[b + 1] - if b == edges.len() - 2 { 0.0 } else { 9.0 };
            let list = blocks[b.min(blocks.len() - 1)].clone();
            let mut s = a;
            let mut k = 0;
            loop {
                let lot = match list.get(k) {
                    Some(l) => *l,
                    None => self.lot_house(),
                };
                k += 1;
                if s + lot.w() > z {
                    break;
                }
                self.place_lot(s + lot.w() / 2.0, self.front_lat, &lot);
                s += lot.w() + rrange(&mut self.rng, 0.5, 2.5);
            }
        }
    }

    fn place_lot(&mut self, s: f64, lat: f64, lot: &Lot) -> bool {
        let (mut fr, _) = self.frame_at(s, lat, true);
        let (w, d) = (lot.w(), lot.d());
        let (cx, cz) = Self::to_world(&fr, 0.0, -d / 2.0);
        let r = js::min(w, d) * 0.45;
        if !self.clear_of_road(cx, cz, r) {
            return false;
        }
        self.take(cx, cz, js::max(w, d) * 0.5);
        // Sit on the lowest corner so nothing floats.
        let mut y = f64::INFINITY;
        for (lx, lz) in [
            (-w / 2.0, 0.0),
            (w / 2.0, 0.0),
            (-w / 2.0, -d),
            (w / 2.0, -d),
        ] {
            let (x, z) = Self::to_world(&fr, lx, lz);
            y = js::min(y, self.gy(x, z));
        }
        fr.y = y;
        self.b.set_frame(fr.x, y, fr.z, fr.yaw);
        self.build_lot(lot, &fr);
        true
    }

    fn build_lot(&mut self, lot: &Lot, fr: &Fr) {
        match *lot {
            Lot::Shop { rect, w } => {
                let blade = if self.rng.next_f64() < 0.6 {
                    let blades = self.sg().blades.clone();
                    Some(*rpick(&mut self.rng, &blades))
                } else {
                    None
                };
                shop(
                    &mut self.b,
                    &mut self.rng,
                    ShopOpts {
                        w: w - 1.0,
                        d: 14.0,
                        rect: Some(rect),
                        blade,
                        ..ShopOpts::default()
                    },
                );
            }
            Lot::Surf => {
                let r = self.sg().surf;
                surf_shop(&mut self.b, &mut self.rng, &r);
            }
            Lot::Taco => {
                let r = self.sg().tacos;
                taco_stand(&mut self.b, &mut self.rng, Some(&r));
            }
            Lot::Diner => {
                self.b
                    .box_("asphaltLot", 25.0, 0.08, 7.5, 0.0, 0.0, -3.9, 0.0, 0.0, 0.0);
                self.b.push_frame(0.0, 0.0, -8.0, 0.0);
                let r = self.nn().diner;
                diner(&mut self.b, &mut self.rng, &r);
                self.b.pop_frame();
                self.lot_cars(fr, -9.0, 9.0, -3.9, 4, false);
            }
            Lot::Motel => {
                let (sr, nr) = (self.sg().motel_board, self.nn().vacancy);
                motel(
                    &mut self.b,
                    &mut self.rng,
                    MotelOpts {
                        w: 42.0,
                        sign_rect: Some(sr),
                        neon_rect: Some(nr),
                        wall: None,
                    },
                );
                self.lot_cars(fr, -6.0, 17.0, -13.0, 5, false);
            }
            Lot::Gas => {
                let (sr, pr) = (self.sg().gas, self.sg().price);
                gas_station(
                    &mut self.b,
                    &mut self.rng,
                    GasOpts {
                        sign_rect: Some(sr),
                        price_rect: Some(pr),
                    },
                );
                self.lot_cars(fr, -2.4, 2.4, -8.0, 2, true);
            }
            Lot::Hotel => {
                let r = self.nn().inn;
                hotel(
                    &mut self.b,
                    &mut self.rng,
                    HotelOpts {
                        w: 36.0,
                        rect: Some(r),
                        ..HotelOpts::default()
                    },
                );
            }
            Lot::Parking => {
                self.b.box_(
                    "asphaltLot",
                    24.0,
                    0.08,
                    22.0,
                    0.0,
                    0.0,
                    -11.0,
                    0.0,
                    0.0,
                    0.0,
                );
                for z in [-17.0, -5.5] {
                    let mut x = -10.5;
                    while x <= 10.5 {
                        self.b
                            .box_("paintWhite", 0.12, 0.02, 5.0, x, 0.09, z, 0.0, 0.0, 0.0);
                        x += 3.0;
                    }
                }
                self.lot_cars(fr, -9.0, 9.0, -17.0, 5, false);
                self.lot_cars(fr, -9.0, 9.0, -5.5, 3, false);
            }
            Lot::House { .. } => {
                let w = rrange(&mut self.rng, 10.0, 12.0);
                beach_house(
                    &mut self.b,
                    &mut self.rng,
                    HouseOpts {
                        w: Some(w),
                        ..HouseOpts::default()
                    },
                );
            }
        }
    }

    /// Parked cars in a row inside a lot frame (local x0..x1 at local z).
    fn lot_cars(&mut self, fr: &Fr, x0: f64, x1: f64, z: f64, n: usize, at_pumps: bool) {
        const KINDS: [&str; 6] = ["sedan", "hatch", "pickup", "van", "sedan", "hatch"];
        const COLORS: [u32; 8] = [
            0xe8e6df, 0x2b2f36, 0x8a1c1c, 0x2e6d8e, 0xc9ccd1, 0xd9b45a, 0x4d8a7a, 0x6b4a2e,
        ];
        for i in 0..n {
            if !at_pumps && self.rng.next_f64() < 0.3 {
                continue;
            }
            let lx = if n == 1 {
                (x0 + x1) / 2.0
            } else {
                x0 + (x1 - x0) * (i as f64 / (n - 1) as f64)
            };
            let (x, zw) = Self::to_world(fr, lx, z);
            // `atPumps ? 0 : rng() < 0.5 ? 0 : Math.PI`: no draw at the pumps.
            let turn = if !at_pumps && self.rng.next_f64() >= 0.5 {
                PI
            } else {
                0.0
            };
            let yaw = fr.yaw + turn + rrange(&mut self.rng, -0.05, 0.05);
            let y = self.gy(x, zw);
            let kind = *rpick(&mut self.rng, &KINDS);
            let color = *rpick(&mut self.rng, &COLORS);
            let seed = (self.rng.next_f64() * 999.0).floor() as u32;
            self.cars.push(Car {
                x,
                z: zw,
                y,
                yaw,
                kind,
                color,
                seed,
            });
        }
    }

    // ── Houses behind the main street ───────────────────────────────────

    fn build_residential(&mut self) {
        let t = self.t;
        // Street surfaces.
        let street = self.mat_of("street").expect("street");
        for (_, pts) in self.streets.clone() {
            self.ribbon(&pts, 9.0, street, 0.14, "crossStreet", None);
        }
        for lat in self.back_lats.clone() {
            self.strip_along(
                self.beach.town_s0 - 20.0,
                self.beach.town_s1 + 20.0,
                lat - 4.0,
                lat + 4.0,
                street,
                StripOpts {
                    lift: 0.13,
                    u_scale: 8.0,
                    v_scale: 4.0,
                    name: "backStreet",
                    ..StripOpts::default()
                },
            );
        }
        let bl = self.back_lats.clone();
        let rows = [
            (bl[0] - 7.0, 1.0, 0.9),
            (bl[0] + 7.0, -1.0, 0.9),
            (bl[1] - 7.0, 1.0, 0.8),
            (bl[1] + 7.0, -1.0, 0.75),
            (bl[2] - 7.0, 1.0, 0.65),
            (bl[2] + 7.0, -1.0, 0.55),
        ];
        for (row_lat, dir, dens) in rows {
            let mut s = self.beach.town_s0 + rrange(&mut self.rng, 0.0, 8.0);
            while s < self.beach.town_s1 {
                'body: {
                    if self.rng.next_f64() > dens {
                        break 'body;
                    }
                    let f = t.frame(s);
                    let p = t.point_at(s, row_lat);
                    // +Z (house front) toward the back street.
                    let k = dir;
                    let mut fr = Fr {
                        x: p.x,
                        z: p.z,
                        y: 0.0,
                        yaw: yaw_z(k * f.rx, k * f.rz),
                    };
                    let w = rrange(&mut self.rng, 9.0, 12.0);
                    let d = rrange(&mut self.rng, 10.0, 13.0);
                    let (cx, cz) = Self::to_world(&fr, 0.0, -d / 2.0);
                    if !self.free(cx, cz, js::max(w, d) * 0.55) {
                        break 'body;
                    }
                    self.take(cx, cz, js::max(w, d) * 0.55);
                    fr.y = js::min(self.gy(cx, cz), self.gy(p.x, p.z));
                    self.b.set_frame(fr.x, fr.y, fr.z, fr.yaw);
                    let floors = if self.rng.next_f64() < 0.7 { 2.0 } else { 3.0 };
                    beach_house(
                        &mut self.b,
                        &mut self.rng,
                        HouseOpts {
                            w: Some(w),
                            d: Some(d),
                            floors: Some(floors),
                            ..HouseOpts::default()
                        },
                    );
                    // Front lawn and hedge, sometimes a pool out back.
                    self.b.box_(
                        "lawn",
                        w * 0.55,
                        0.05,
                        3.6,
                        w * 0.2,
                        0.02,
                        2.0,
                        0.0,
                        0.0,
                        0.0,
                    );
                    self.b.box_(
                        "concrete",
                        w * 0.4,
                        0.05,
                        3.6,
                        -w * 0.25,
                        0.02,
                        2.0,
                        0.0,
                        0.0,
                        0.0,
                    );
                    if self.rng.next_f64() < 0.6 {
                        let h = rrange(&mut self.rng, 0.7, 1.1);
                        self.b
                            .box_("hedge", w * 0.5, h, 0.7, w * 0.22, 0.0, 4.1, 0.0, 0.0, 0.0);
                    }
                    if self.rng.next_f64() < 0.25 {
                        self.b.box_(
                            "concrete",
                            5.5,
                            0.12,
                            4.0,
                            0.0,
                            0.0,
                            -d - 3.2,
                            0.0,
                            0.0,
                            0.0,
                        );
                        self.b
                            .box_("pool", 4.5, 0.04, 3.0, 0.0, 0.1, -d - 3.2, 0.0, 0.0, 0.0);
                    }
                    // Driveway car and a palm or two in the yard.
                    if self.rng.next_f64() < 0.16 {
                        let (x, z) = Self::to_world(&fr, -w * 0.22, 3.2);
                        let y = self.gy(x, z);
                        let turn = if self.rng.next_f64() < 0.5 { 0.0 } else { PI };
                        let kind = *rpick(&mut self.rng, &["sedan", "hatch", "pickup", "van"]);
                        let color = *rpick(
                            &mut self.rng,
                            &[0xe8e6df, 0x2b2f36, 0x8a1c1c, 0x2e6d8e, 0x9aa3ad, 0xb5a27a],
                        );
                        let seed = (self.rng.next_f64() * 999.0).floor() as u32;
                        self.cars.push(Car {
                            x,
                            z,
                            y,
                            yaw: fr.yaw + turn,
                            kind,
                            color,
                            seed,
                        });
                    }
                    if self.rng.next_f64() < 0.55 {
                        let lz = rrange(&mut self.rng, -d, 1.0);
                        let (x, z) = Self::to_world(&fr, w / 2.0 + 1.4, lz);
                        self.add_palm(x, z, None, true);
                    }
                }
                s += rrange(&mut self.rng, 14.0, 19.0);
            }
        }
    }

    fn build_hill_houses(&mut self) {
        for h in self.beach.hill_houses.clone() {
            let mut rng = Mulberry32::new(h.seed);
            let y = self.gy(h.x, h.z);
            self.b.set_frame(h.x, y, h.z, h.yaw);
            self.b
                .box_("concrete", 14.0, 1.2, 15.0, 0.0, -1.1, -6.0, 0.0, 0.0, 0.0);
            let w = rrange(&mut rng, 10.0, 12.0);
            let tile = rng.next_f64() < 0.6;
            beach_house(
                &mut self.b,
                &mut rng,
                HouseOpts {
                    w: Some(w),
                    d: Some(11.0),
                    floors: Some(2.0),
                    tile: Some(tile),
                    ..HouseOpts::default()
                },
            );
            self.take(h.x, h.z, 9.0);
            let (x, z) = Self::to_world(
                &Fr {
                    x: h.x,
                    z: h.z,
                    y: 0.0,
                    yaw: h.yaw,
                },
                8.0,
                -2.0,
            );
            self.add_palm(x, z, Some(&mut rng), false);
        }
    }

    // ── Palms, street lights, light pools ───────────────────────────────

    /// `addPalm(x, z, rng, force = false)`: `rng` `None` is `this.rng`.
    fn add_palm(&mut self, x: f64, z: f64, rng: Option<&mut Mulberry32>, force: bool) {
        if !force && !self.free(x, z, 1.2) {
            return;
        }
        self.take(x, z, 1.0);
        let y = self.gy(x, z);
        let rng = match rng {
            Some(r) => r,
            None => &mut self.rng,
        };
        let h = rrange(rng, 9.0, 15.0);
        let yaw = rng.next_f64() * PI * 2.0;
        let _lean = rrange(rng, 0.6, 1.4);
        let s = rrange(rng, 0.85, 1.2);
        let c = rng.next_f64();
        self.palms.push(Palm {
            x,
            z,
            y,
            h,
            yaw,
            s,
            c,
        });
    }

    fn build_street_furniture(&mut self) {
        let t = self.t;
        let (town_s0, town_s1, pier_s, marina_s, zs0) = (
            self.beach.town_s0,
            self.beach.town_s1,
            self.beach.pier_s,
            self.beach.marina_s,
            self.beach.zs0,
        );
        let cross = self.beach.cross_s.clone();
        let near_cross = |s: f64, d: f64| cross.iter().any(|c| (s - c).abs() < d);
        let mut s = town_s0 - 40.0;
        while s < town_s1 + 30.0 {
            for side in [1.0, -1.0] {
                let ss = if side > 0.0 { s } else { s + 23.0 };
                if near_cross(ss, 10.0) || (side < 0.0 && (ss - pier_s).abs() < 12.0) {
                    continue;
                }
                let f = t.frame(ss);
                let wall = if side > 0.0 { f.wall_r } else { f.wall_l };
                let (fr, _) = self.frame_at(ss, side * (wall + 1.0), true);
                self.b.set_frame(
                    fr.x,
                    fr.y + if side < 0.0 { 0.22 } else { 0.14 },
                    fr.z,
                    fr.yaw,
                );
                street_light(&mut self.b, 0.0, 0.0, 0.0, 3.4, 8.6);
                let p = t.point_at(ss, side * (wall + 1.0 - 3.7));
                self.pools.push([p.x, p.y + 0.04, p.z]);
            }
            s += 46.0;
        }
        // Palms: along the town sidewalk, the sand edge and the boulevard's
        // approach down the hill.
        let mut s = town_s0 - 20.0;
        while s < town_s1 + 20.0 {
            if !near_cross(s, 9.0) {
                let p = t.point_at(s + 11.0, t.frame(s).wall_r + 3.4);
                self.add_palm(p.x, p.z, None, true);
            }
            s += 23.0;
        }
        let mut s = town_s0 - 20.0;
        while s < town_s1 + 20.0 {
            if !((s - pier_s).abs() < 14.0 || (s - marina_s).abs() < 10.0) {
                let lat = -(self.prom_out + 2.5 + self.rng.next_f64() * 2.0);
                let p = t.point_at(s, lat);
                self.add_palm(p.x, p.z, None, true);
            }
            s += 17.0;
        }
        let mut s = zs0 + 60.0;
        while s < town_s0 - 20.0 {
            let f = t.frame(s);
            let lat = f.wall_r + rrange(&mut self.rng, 4.0, 9.0);
            let p = t.point_at(s, lat);
            self.add_palm(p.x, p.z, None, false);
            s += 30.0;
        }
    }

    // Hydrants, bike racks, newspaper boxes and planters on the town
    // sidewalk, kept clear of the lamp posts, palms and cross streets.
    fn build_sidewalk_props(&mut self) {
        let t = self.t;
        let w = t.frame(self.beach.pier_s).wall_r;
        let hydrant = cylinder_geometry(0.16, 0.2, 0.75, 8.0, 1.0, false, 0.0, PI * 2.0);
        let cap = sphere_geometry(0.17, 8.0, 5.0, 0.0, PI * 2.0, 0.0, PI / 2.0);
        let cross = self.beach.cross_s.clone();
        let mut s = self.beach.town_s0 + 7.0;
        while s < self.beach.town_s1 - 5.0 {
            'body: {
                if cross.iter().any(|c| (s - c).abs() < 9.0) {
                    break 'body;
                }
                let k = (self.rng.next_f64() * 5.0).floor();
                let (fr, _) = self.frame_at(s, w + 1.3, true);
                self.b.set_frame(fr.x, fr.y + 0.14, fr.z, fr.yaw + PI);
                let b = &mut self.b;
                if k == 0.0 {
                    b.put_at("paintRed", &hydrant, 0.0, 0.37, 0.0);
                    b.put_at("paintRed", &cap, 0.0, 0.74, 0.0);
                    b.cbox("paintRed", 0.5, 0.1, 0.1, 0.0, 0.5, 0.0, [0.0; 3]);
                } else if k == 1.0 {
                    for i in 0..3 {
                        let i = f64::from(i);
                        b.box_(
                            "metal",
                            0.05,
                            0.8,
                            0.05,
                            -0.8 + i * 0.8 - 0.3,
                            0.0,
                            0.0,
                            0.0,
                            0.0,
                            0.0,
                        );
                        b.box_(
                            "metal",
                            0.05,
                            0.8,
                            0.05,
                            -0.8 + i * 0.8 + 0.3,
                            0.0,
                            0.0,
                            0.0,
                            0.0,
                            0.0,
                        );
                        b.box_(
                            "metal",
                            0.65,
                            0.05,
                            0.05,
                            -0.8 + i * 0.8,
                            0.8,
                            0.0,
                            0.0,
                            0.0,
                            0.0,
                        );
                    }
                } else if k == 2.0 {
                    const KEYS: [&str; 4] = ["wBlue", "paintRed", "wYellow", "white"];
                    let k1 = *rpick(&mut self.rng, &KEYS);
                    self.b
                        .box_(k1, 0.5, 1.05, 0.45, -0.3, 0.0, 0.0, 0.0, 0.0, 0.0);
                    let k2 = *rpick(&mut self.rng, &KEYS);
                    self.b
                        .box_(k2, 0.5, 1.05, 0.45, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0);
                } else if k == 3.0 {
                    b.box_("concrete", 1.6, 0.55, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
                    b.box_("hedge", 1.45, 0.45, 0.65, 0.0, 0.55, 0.0, 0.0, 0.0, 0.0);
                } else {
                    bench(b, 0.0, 0.0, 0.0);
                }
            }
            s += rrange(&mut self.rng, 9.0, 16.0);
        }
    }

    // Cars parked along the first stretch of each cross street, where they
    // show from the boulevard. (Each car costs a couple of thousand
    // triangles, so the far ends of the streets stay empty.)
    fn build_street_parking(&mut self) {
        let t = self.t;
        const KINDS: [&str; 6] = ["sedan", "hatch", "pickup", "van", "sedan", "hatch"];
        const COLORS: [u32; 10] = [
            0xe8e6df, 0x2b2f36, 0x8a1c1c, 0x2e6d8e, 0xc9ccd1, 0xd9b45a, 0x4d8a7a, 0x6b4a2e,
            0x9fd8cf, 0xf2c14e,
        ];
        for (st_s, _) in self.streets.clone() {
            let f = t.frame(st_s);
            let yaw = yaw_z(f.rx, f.rz);
            let mut d = f.wall_r + 16.0;
            while d < 42.0 {
                for side in [-1.0, 1.0] {
                    if self.rng.next_f64() < 0.45 {
                        continue;
                    }
                    let x = f.x + f.rx * d + f.fx * side * 3.4;
                    let z = f.z + f.rz * d + f.fz * side * 3.4;
                    let y = self.gy(x, z);
                    let cy = yaw
                        + if side > 0.0 { 0.0 } else { PI }
                        + rrange(&mut self.rng, -0.03, 0.03);
                    let kind = *rpick(&mut self.rng, &KINDS);
                    let color = *rpick(&mut self.rng, &COLORS);
                    let seed = (self.rng.next_f64() * 999.0).floor() as u32;
                    self.cars.push(Car {
                        x,
                        z,
                        y,
                        yaw: cy,
                        kind,
                        color,
                        seed,
                    });
                }
                d += 6.2;
            }
        }
    }

    // ── The palms ───────────────────────────────────────────────────────

    fn build_palms(&mut self) {
        let list = self.palms.clone();
        if list.is_empty() {
            return;
        }
        // Trunk: tapered, ringed, bending to +X by 1.4 m over its height.
        let mut trunk = cylinder_geometry(0.17, 0.28, 1.0, 7.0, 10.0, true, 0.0, PI * 2.0);
        trunk.translate(0.0, 0.5, 0.0);
        {
            let p = trunk.get_attribute_mut("position").expect("position");
            for i in 0..p.count() {
                let y = p.get_y(i);
                let bulge = 1.0 + 0.07 * js::max(0.0, kernel::sin(y * 24.0 * PI * 2.0));
                let (x, z) = (p.get_x(i), p.get_z(i));
                p.set_xyz(i, x * bulge + 1.4 * y * y, y, z * bulge);
            }
        }
        trunk.compute_vertex_normals();
        let bark = canvas_tex(self.graph, 64, 256, true, true, |g, w, h| {
            g.set_fill_style("#8a6f52");
            g.fill_rect(0.0, 0.0, w, h);
            let mut y = 0.0;
            while y < h {
                g.set_fill_style("rgba(60,40,25,0.55)");
                g.fill_rect(0.0, y, w, 3.0);
                g.set_fill_style("rgba(200,170,130,0.25)");
                g.fill_rect(0.0, y + 3.0, w, 2.0);
                y += h / 24.0;
            }
        });
        let trunk_mat = self.mat(Material::standard().set("map", bark).set("roughness", 0.95));
        // Crown: arching fronds with feathered leaflets that hang from the
        // midrib, plus a few dead fronds drooping below as a brown skirt.
        let leaf = canvas_tex(self.graph, 64, 256, false, true, |g, w, h| {
            g.clear_rect(0.0, 0.0, w, h);
            let mut rng = Mulberry32::new(5);
            let mut y = 4.0;
            while y < h - 6.0 {
                let t = y / h;
                let span = (w / 2.0 - 1.0)
                    * kernel::sin(t * PI * 0.92 + 0.12)
                    * (0.8 + rng.next_f64() * 0.2);
                let droop = 10.0 + t * 8.0;
                let c = 110.0 + rng.next_f64() * 60.0;
                g.set_stroke_style(format!("rgb({},{},{})", c * 0.45, c, c * 0.32));
                g.set_line_width(1.8);
                for sd in [-1.0, 1.0] {
                    if rng.next_f64() < 0.08 {
                        continue; // a torn leaflet here and there
                    }
                    g.begin_path();
                    g.move_to(w / 2.0, y);
                    g.quadratic_curve_to(
                        w / 2.0 + sd * span * 0.6,
                        y + droop * 0.2,
                        w / 2.0 + sd * span,
                        y + droop,
                    );
                    g.stroke();
                }
                y += 3.0;
            }
            g.set_stroke_style("#6a7a3a");
            g.set_line_width(3.0);
            g.begin_path();
            g.move_to(w / 2.0, 0.0);
            g.line_to(w / 2.0, h);
            g.stroke();
        });
        let mut fronds = Vec::new();
        let n = 16;
        let dead = Color::hex(0x9a7a4a);
        let green = Color::hex(0xffffff);
        for i in 0..n {
            let fi = f64::from(i);
            let is_dead = i >= 13;
            let a = if is_dead {
                (fi - 13.0) * 2.1 + 0.4
            } else {
                (fi / 13.0) * PI * 2.0 + f64::from(i % 2) * 0.24
            };
            let len = if is_dead {
                2.8
            } else if i % 3 == 0 {
                3.4
            } else {
                4.4
            };
            let up = if is_dead {
                -1.6
            } else if i % 3 == 0 {
                1.3
            } else if i % 3 == 1 {
                0.7
            } else {
                0.35
            };
            let mut g = plane_geometry(1.5, 1.0, 1.0, 7.0);
            let c = if is_dead { dead } else { green };
            let count = g.position().count();
            let mut cols = vec![0.0f32; count * 3];
            {
                let q = g.get_attribute_mut("position").expect("position");
                for k in 0..count {
                    let tt = q.get_y(k) + 0.5; // 0 at base → 1 at tip
                    let across = q.get_x(k) * (1.0 - tt * 0.45);
                    let r = tt * len;
                    let h = up * tt * 2.2 - (if is_dead { 0.6 } else { 2.6 }) * tt * tt;
                    let fold = -across.abs() * 0.55; // leaflets hang below the midrib
                    let ca = kernel::cos(a);
                    let sa = kernel::sin(a);
                    let tw = across * kernel::cos(tt * 1.2);
                    q.set_xyz(k, ca * r - sa * tw, h + fold, sa * r + ca * tw);
                    cols[k * 3] = c.r as f32;
                    cols[k * 3 + 1] = c.g as f32;
                    cols[k * 3 + 2] = c.b as f32;
                }
            }
            g.set_attribute("color", BufferAttribute::from_f32(cols, 3));
            g.compute_vertex_normals();
            fronds.push(g.to_non_indexed());
        }
        let refs: Vec<&BufferGeometry> = fronds.iter().collect();
        let crown = merge_geometries(&refs, false).expect("the fronds merge");
        let crown_mat = self.mat(
            Material::standard()
                .set("map", leaf)
                .set("vertexColors", true)
                .set("alphaTest", 0.4)
                .set("side", DOUBLE_SIDE)
                .set("roughness", 0.85)
                .set("color", 0xffffff),
        );
        let nut_mat = self.mat(
            Material::standard()
                .set("color", 0x6a4a28)
                .set("roughness", 0.8),
        );
        let nut = icosahedron_geometry(0.22, 0.0);
        let n = list.len() as u32;
        let (tg, cg, ng) = (self.geo(trunk), self.geo(crown), self.geo(nut));
        let im_t = self.graph.instanced_mesh(tg, trunk_mat, n);
        let im_c = self.graph.instanced_mesh(cg, crown_mat, n);
        let im_n = self.graph.instanced_mesh(ng, nut_mat, n * 3);
        for (k, pm) in list.iter().enumerate() {
            let q = Quaternion::from_euler(&Euler::new(0.0, pm.yaw, 0.0));
            let m4 = Matrix4::compose(v3(pm.x, pm.y - 0.2, pm.z), q, v3(pm.s, pm.h, pm.s));
            self.inst(im_t).set_matrix_at(k, &m4);
            // Crown at the bent trunk's tip.
            let bx = 1.4 * pm.s;
            let tx = pm.x + kernel::cos(pm.yaw) * bx;
            let tz = pm.z - kernel::sin(pm.yaw) * bx;
            let ty = pm.y - 0.2 + pm.h;
            let q = Quaternion::from_euler(&Euler::new(0.1, pm.yaw * 1.7, 0.05));
            let cs = pm.s * lerp(0.85, 1.15, pm.c);
            let m4 = Matrix4::compose(v3(tx, ty, tz), q, v3(cs, cs, cs));
            self.inst(im_c).set_matrix_at(k, &m4);
            let mut col = Color::new(0.0, 0.0, 0.0);
            col.set_hsl(0.26 + pm.c * 0.06, 0.35, 0.75 + pm.c * 0.2);
            self.inst(im_c).set_color_at(k, col);
            for j in 0..3 {
                let a = f64::from(j) * 2.1 + pm.yaw;
                let m4 = Matrix4::compose(
                    v3(
                        tx + kernel::cos(a) * 0.3,
                        ty - 0.35,
                        tz + kernel::sin(a) * 0.3,
                    ),
                    Quaternion::IDENTITY,
                    v3(1.0, 1.0, 1.0),
                );
                self.inst(im_n).set_matrix_at(k * 3 + j as usize, &m4);
            }
        }
        for im in [im_t, im_c, im_n] {
            {
                let o = self.graph.get_mut(im);
                o.cast_shadow = im != im_n;
                o.receive_shadow = true;
                o.name = "beach:palms".to_string();
            }
            self.graph.compute_instance_bounding_sphere(im);
            self.graph.add(self.group, im);
        }
    }

    fn inst(&mut self, n: NodeId) -> &mut crate::object::Instances {
        self.graph.get_mut(n).instances.as_mut().expect("instanced")
    }

    // Additive glow on the road under each street light.
    fn build_pools(&mut self) {
        if self.pools.is_empty() {
            return;
        }
        let mut pos = Vec::new();
        let mut uv = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let r = 7.0;
        for (k, p) in self.pools.iter().enumerate() {
            let b = (k * 4) as u32;
            let [x, y, z] = *p;
            pos.extend_from_slice(&[
                x - r,
                y,
                z - r,
                x + r,
                y,
                z - r,
                x - r,
                y,
                z + r,
                x + r,
                y,
                z + r,
            ]);
            uv.extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            idx.extend_from_slice(&[b, b + 2, b + 1, b + 1, b + 2, b + 3]);
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
        g.set_index(&idx);
        g.compute_bounding_sphere();
        let glow = self
            .graph
            .cached_texture(&self.textures.glow_texture(), Layer::Main, "");
        let pool_mat = self.mat(
            Material::basic()
                .set("map", glow)
                .set("color", 0xffd9a0)
                .set("transparent", true)
                .set("opacity", 0.0)
                .set("blending", ADDITIVE)
                .set("depthWrite", false)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -6.0)
                .set("polygonOffsetUnits", -6.0),
        );
        self.pool_mat = Some(pool_mat);
        let g = self.geo(g);
        let m = self.graph.mesh(g, pool_mat);
        {
            let o = self.graph.get_mut(m);
            o.name = "beach:lightPools".to_string();
            o.render_order = 2.0;
        }
        self.graph.add(self.group, m);
    }

    // Parked cars: built from the vehicle kit, then merged by material so
    // the whole town's cars cost a dozen or so draw calls.
    fn build_cars(&mut self) {
        if self.cars.is_empty() {
            return;
        }
        let mut b = Builder::new();
        // `mats[sig]`: the first material (or material list) with each
        // signature, in the order first met.
        let mut mats: Vec<(String, Vec<MaterialId>)> = Vec::new();
        for c in self.cars.clone() {
            let Some(m) = car_model::build_vehicle(
                self.graph,
                self.textures,
                c.kind,
                &CarOpts {
                    color: Some(c.color),
                    seed: c.seed,
                    lod: Some(Lod::Low),
                    ..CarOpts::default()
                },
            ) else {
                continue;
            };
            {
                let o = self.graph.get_mut(m.root);
                o.position = v3(c.x, c.y, c.z);
                o.set_rotation(&Euler::new(0.0, c.yaw, 0.0));
            }
            for (id, world) in crate::mountain::kit::world_matrices(self.graph, m.root) {
                let o = self.graph.get(id);
                let is_mesh = matches!(
                    o.ty,
                    mp_scene::NodeType::Mesh | mp_scene::NodeType::InstancedMesh
                );
                let Some(geo) = o.geometry.filter(|_| is_mesh) else {
                    continue;
                };
                let list = o.materials.clone();
                let sig = if o.multi_material {
                    // An array: every field of the signature is missing.
                    "||||||||".to_string()
                } else {
                    self.signature(list[0])
                };
                if !mats.iter().any(|(s, _)| *s == sig) {
                    mats.push((sig.clone(), list));
                }
                b.set_frame(0.0, 0.0, 0.0, 0.0);
                let g = self.graph.geometry(geo).clone();
                b.add(&sig, &g, Some(&world));
            }
        }
        // `B.build(mats, { castShadow: true })`.
        for (key, geos) in std::mem::take(&mut b.buckets) {
            let Some((_, list)) = mats.iter().find(|(s, _)| *s == key) else {
                continue;
            };
            let refs: Vec<&BufferGeometry> = geos.iter().collect();
            let Some(mut merged) = merge_geometries(&refs, false) else {
                continue;
            };
            merged.compute_bounding_sphere();
            let g = self.geo(merged);
            let mesh = if list.len() == 1 && !key.starts_with("||||||||") {
                self.graph.mesh(g, list[0])
            } else {
                self.graph.multi_mesh(g, list)
            };
            let o = self.graph.get_mut(mesh);
            o.name = "beach:cars".to_string();
            o.cast_shadow = true;
            o.receive_shadow = true;
            o.matrix_auto_update = false;
            o.update_matrix();
            self.graph.add(self.group, mesh);
        }
    }

    /// The JS signature `[type, color hex, emissive hex, emissiveIntensity,
    /// roughness, metalness, map uuid, transparent, opacity].join('|')`:
    /// equal for materials the JS would merge.
    fn signature(&self, m: MaterialId) -> String {
        let mat = self.graph.material(m);
        let hex = |k: &str| mat.color(k).map_or(String::new(), |c| c.get_hex_string());
        let n = |k: &str| {
            mat.number(k)
                .map_or(String::new(), |v| format!("{:?}", v + 0.0))
        };
        let map = match mat.get("map") {
            Some(Value::Object(o)) => o.get("texture").map_or(String::new(), |t| format!("t{t}")),
            _ => String::new(),
        };
        let b = |k: &str| match mat.get(k) {
            Some(Value::Bool(v)) => v.to_string(),
            _ => String::new(),
        };
        [
            mat.desc.ty.clone(),
            hex("color"),
            hex("emissive"),
            n("emissiveIntensity"),
            n("roughness"),
            n("metalness"),
            map,
            b("transparent"),
            n("opacity"),
        ]
        .join("|")
    }

    // ── The beach ───────────────────────────────────────────────────────

    /// Search seaward for where the rendered ground meets the sea.
    fn waterline_lat(&self, s: f64) -> Option<f64> {
        let t = self.t;
        let mut lo = -(self.prom_out + 5.0);
        let mut hi = -260.0;
        let p = t.point_at(s, hi);
        if self.gy(p.x, p.z) > 0.1 {
            return None;
        }
        for _ in 0..18 {
            let m = (lo + hi) / 2.0;
            let p = t.point_at(s, m);
            if self.gy(p.x, p.z) > 0.1 {
                lo = m;
            } else {
                hi = m;
            }
        }
        Some((lo + hi) / 2.0)
    }

    fn build_beach_props(&mut self) {
        let (town_s0, town_s1, pier_s, marina_s) = (
            self.beach.town_s0,
            self.beach.town_s1,
            self.beach.pier_s,
            self.beach.marina_s,
        );
        let busy = |s: f64| (s - pier_s).abs() < 22.0 || (s - marina_s).abs() < 75.0;
        // Lifeguard towers.
        let mut n = 0;
        let mut s = town_s0 + 60.0;
        while s < town_s1 - 40.0 {
            'body: {
                if busy(s) {
                    s += 50.0;
                }
                let wl = self.waterline_lat(s).unwrap_or(-100.0);
                let (fr, _) = self.frame_at(s, lerp(-self.prom_out, wl, 0.55), false);
                if !self.free(fr.x, fr.z, 4.0) {
                    break 'body;
                }
                self.take(fr.x, fr.z, 5.0);
                self.b.set_frame(fr.x, fr.y, fr.z, fr.yaw);
                let r = self.sg().tower[n % 7];
                n += 1;
                lifeguard_tower(&mut self.b, &mut self.rng, Some(&r));
                self.b.push_frame(3.5, 0.0, -1.5, 0.3);
                let k = 2.0 + (self.rng.next_f64() * 3.0).floor();
                surfboards_in_sand(&mut self.b, &mut self.rng, k);
                self.b.pop_frame();
            }
            s += 215.0;
        }
        // Umbrella clusters.
        const FABRICS: [&str; 4] = ["awnRed", "awnBlue", "awnYellow", "awnGreen"];
        let mut s = town_s0 + 20.0;
        while s < town_s1 - 20.0 {
            'body: {
                if busy(s) {
                    break 'body;
                }
                let wl = self.waterline_lat(s).unwrap_or(-100.0);
                let cols = 2.0 + (self.rng.next_f64() * 3.0).floor();
                let rows = 1.0 + (self.rng.next_f64() * 2.0).floor();
                let f = rrange(&mut self.rng, 0.2, 0.35);
                let lat0 = lerp(-self.prom_out, wl, f);
                let fabric = *rpick(&mut self.rng, &FABRICS);
                let mut i = 0.0;
                while i < cols {
                    let mut j = 0.0;
                    while j < rows {
                        let (fr, _) = self.frame_at(s + i * 5.5, lat0 - j * 6.0, false);
                        j += 1.0;
                        if !self.free(fr.x, fr.z, 2.0) {
                            continue;
                        }
                        self.take(fr.x, fr.z, 2.2);
                        self.b.set_frame(fr.x, fr.y, fr.z, fr.yaw);
                        let fab = if self.rng.next_f64() < 0.7 {
                            fabric
                        } else {
                            *rpick(&mut self.rng, &FABRICS)
                        };
                        umbrella_set(&mut self.b, &mut self.rng, fab, None);
                    }
                    i += 1.0;
                }
            }
            s += rrange(&mut self.rng, 35.0, 70.0);
        }
        // Volleyball courts.
        for s in [
            town_s0 + 150.0,
            pier_s + 140.0,
            pier_s + 400.0,
            marina_s - 170.0,
        ] {
            if s > town_s1 || busy(s) {
                continue;
            }
            let wl = self.waterline_lat(s).unwrap_or(-100.0);
            let (fr, _) = self.frame_at(s, lerp(-self.prom_out, wl, 0.4), false);
            if !self.free(fr.x, fr.z, 6.0) {
                continue;
            }
            self.take(fr.x, fr.z, 7.0);
            // Net parallel to the shore → rotate so X runs along the beach.
            self.b.set_frame(fr.x, fr.y, fr.z, fr.yaw + PI / 2.0);
            volleyball_net(&mut self.b);
        }
    }

    // ── The pier ────────────────────────────────────────────────────────

    /// `buildPier()` then `buildMarina()` (the JS calls them one after the
    /// other); returns the marina's boat count.
    fn build_pier_and_marina(&mut self) -> usize {
        self.build_pier();
        self.build_marina()
    }

    fn build_pier(&mut self) {
        let t = self.t;
        let s = self.beach.pier_s;
        let (mut fr, _) = self.frame_at(s, -self.prom_out, false);
        let deck_y = t.surface_y(s, 0.0) + 0.25;
        fr.y = deck_y;
        let l = 240.0;
        let w = 12.0;
        let pw = 46.0;
        let pl = 90.0;
        // Keep the beach clear of the pier.
        let mut z = 0.0;
        while z < l {
            let (x, zz) = Self::to_world(&fr, 0.0, z);
            self.take(x, zz, if z > l - pl { 26.0 } else { 9.0 });
            z += 8.0;
        }
        self.b.set_frame(fr.x, deck_y, fr.z, fr.yaw);
        let b0 = |b: &mut Builder, key: &str, w: f64, h: f64, d: f64, x: f64, y: f64, z: f64| {
            b.box_(key, w, h, d, x, y, z, 0.0, 0.0, 0.0)
        };
        // Deck: main walk + the wide platform at the end.
        b0(
            &mut self.b,
            "wood",
            w,
            0.35,
            l - pl,
            0.0,
            -0.35,
            (l - pl) / 2.0,
        );
        b0(&mut self.b, "wood", pw, 0.35, pl, 0.0, -0.35, l - pl / 2.0);
        b0(
            &mut self.b,
            "woodDark",
            w + 0.3,
            0.6,
            l - pl,
            0.0,
            -0.9,
            (l - pl) / 2.0,
        );
        b0(
            &mut self.b,
            "woodDark",
            pw + 0.3,
            0.6,
            pl,
            0.0,
            -0.9,
            l - pl / 2.0,
        );
        // Piles down to the sea floor.
        let pile = cylinder_geometry(0.28, 0.32, 1.0, 8.0, 1.0, false, 0.0, PI * 2.0);
        let pile_at = |me: &mut Self, lx: f64, lz: f64| {
            let (x, z) = Self::to_world(&fr, lx, lz);
            let g = js::min(me.gy(x, z), 0.0) - 1.0;
            let h = deck_y - 0.9 - g;
            if h > 0.3 {
                me.b.put(
                    "woodDark",
                    &pile,
                    lx,
                    -0.9 - h / 2.0,
                    lz,
                    [0.0; 3],
                    [1.0, h, 1.0],
                );
            }
        };
        let mut z = 6.0;
        while z < l - pl {
            for x in [-w / 2.0 + 0.6, 0.0, w / 2.0 - 0.6] {
                pile_at(self, x, z);
            }
            z += 8.0;
        }
        // Cross bracing between the piles, down to just above the water.
        let mut z = 6.0;
        while z < l - pl {
            let cur = z;
            z += 8.0;
            let (x, zz) = Self::to_world(&fr, 0.0, cur);
            let drop = js::min(
                4.5,
                deck_y - 0.9 - js::max(0.6, js::min(self.gy(x, zz), 0.0) + 0.6),
            );
            if drop < 1.0 {
                continue;
            }
            self.b.beam(
                "woodDark",
                v3(-w / 2.0 + 0.6, -0.9 - drop, cur),
                v3(0.0, -1.0, cur),
                0.16,
            );
            self.b.beam(
                "woodDark",
                v3(w / 2.0 - 0.6, -0.9 - drop, cur),
                v3(0.0, -1.0, cur),
                0.16,
            );
            self.b.cbox(
                "woodDark",
                w - 1.0,
                0.2,
                0.14,
                0.0,
                -0.9 - drop * 0.6,
                cur,
                [0.0; 3],
            );
        }
        let mut z = l - pl + 4.0;
        while z < l {
            let mut x = -pw / 2.0 + 1.0;
            while x <= pw / 2.0 - 1.0 {
                pile_at(self, x, z);
                x += 9.0;
            }
            z += 9.0;
        }
        // Railings.
        let rail = |b: &mut Builder, x0: f64, z0: f64, x1: f64, z1: f64| {
            let len = kernel::hypot(x1 - x0, z1 - z0);
            let ry = kernel::atan2(x1 - x0, z1 - z0);
            b.cbox(
                "white",
                0.1,
                0.1,
                len,
                (x0 + x1) / 2.0,
                1.05,
                (z0 + z1) / 2.0,
                [0.0, ry, 0.0],
            );
            b.cbox(
                "white",
                0.06,
                0.06,
                len,
                (x0 + x1) / 2.0,
                0.55,
                (z0 + z1) / 2.0,
                [0.0, ry, 0.0],
            );
            let n = (len / 2.4).floor();
            let mut i = 0.0;
            while i <= n {
                let u = i / n;
                b.box_(
                    "white",
                    0.1,
                    1.05,
                    0.1,
                    x0 + (x1 - x0) * u,
                    0.0,
                    z0 + (z1 - z0) * u,
                    0.0,
                    0.0,
                    0.0,
                );
                i += 1.0;
            }
        };
        rail(&mut self.b, -w / 2.0, 0.0, -w / 2.0, l - pl);
        rail(&mut self.b, w / 2.0, 0.0, w / 2.0, l - pl);
        rail(&mut self.b, -pw / 2.0, l - pl, -w / 2.0, l - pl);
        rail(&mut self.b, w / 2.0, l - pl, pw / 2.0, l - pl);
        rail(&mut self.b, -pw / 2.0, l - pl, -pw / 2.0, l);
        rail(&mut self.b, pw / 2.0, l - pl, pw / 2.0, l);
        rail(&mut self.b, -pw / 2.0, l, pw / 2.0, l);
        // Lamps and benches along the walk.
        let mut z = 12.0;
        while z < l - 4.0 {
            for x in [-w / 2.0 + 0.5, w / 2.0 - 0.5] {
                if z > l - pl && x.abs() < pw / 2.0 - 2.0 {
                    continue;
                }
                prom_lamp(&mut self.b, x, z);
            }
            if z < l - pl - 10.0 {
                bench(&mut self.b, -w / 2.0 + 1.3, z + 8.0, PI / 2.0);
                bench(&mut self.b, w / 2.0 - 1.3, z + 8.0, -PI / 2.0);
            }
            z += 22.0;
        }
        // Entrance arch over the start of the pier.
        for x in [-w / 2.0 - 0.2, w / 2.0 + 0.2] {
            b0(&mut self.b, "white", 0.8, 7.2, 0.8, x, 0.0, 1.2);
        }
        b0(&mut self.b, "white", w + 2.4, 1.9, 0.5, 0.0, 6.2, 1.2);
        let pier = self.sg().pier;
        let sgeo = sign_geometry(&pier, w + 1.6, 1.6);
        self.b
            .put("signs", &sgeo, 0.0, 7.15, 0.92, [0.0, PI, 0.0], [1.0; 3]);
        let sgeo = sign_geometry(&pier, w + 1.6, 1.6);
        self.b
            .put("signs", &sgeo, 0.0, 7.15, 1.48, [0.0, 0.0, 0.0], [1.0; 3]);
        for k in 0..9 {
            let g = sphere_geometry(0.14, 6.0, 5.0, 0.0, PI * 2.0, 0.0, PI);
            self.b.put_at(
                "lampGlow",
                &g,
                -w / 2.0 - 0.2 + f64::from(k) * (w + 0.4) / 8.0,
                8.25,
                1.2,
            );
        }
        // Bait shop halfway out, with rods leaning on the rail.
        b0(
            &mut self.b,
            "wBlue",
            3.2,
            2.7,
            6.0,
            -w / 2.0 + 2.2,
            0.0,
            70.0,
        );
        self.b.cbox(
            "roofTar",
            3.8,
            0.16,
            6.6,
            -w / 2.0 + 2.2,
            2.85,
            70.0,
            [0.0, 0.0, -0.08],
        );
        self.b.cbox(
            "glass",
            0.08,
            1.1,
            2.4,
            -w / 2.0 + 3.82,
            1.6,
            70.0,
            [0.0; 3],
        );
        let bait = self.sg().bait;
        let sgeo = sign_geometry(&bait, 3.6, 0.9);
        self.b.put(
            "signs",
            &sgeo,
            -w / 2.0 + 3.86,
            2.35,
            70.0,
            [0.0, PI / 2.0, 0.0],
            [1.0; 3],
        );
        for k in 0..4 {
            let k = f64::from(k);
            self.b.beam(
                "black",
                v3(w / 2.0 - 0.3, 0.1, 40.0 + k * 23.0),
                v3(w / 2.0 + 1.6, 3.2, 40.0 + k * 23.0 + 0.6),
                0.03,
            );
        }
        // Arcade near the end of the walk.
        b0(
            &mut self.b,
            "wLilac",
            5.5,
            4.2,
            16.0,
            w / 2.0 - 3.0,
            0.0,
            l - pl - 10.0,
        );
        b0(
            &mut self.b,
            "trim",
            5.9,
            0.4,
            16.4,
            w / 2.0 - 3.0,
            4.2,
            l - pl - 10.0,
        );
        self.b.cbox(
            "glassLit",
            0.1,
            2.0,
            12.0,
            w / 2.0 - 5.78,
            1.6,
            l - pl - 10.0,
            [0.0; 3],
        );
        b0(
            &mut self.b,
            "black",
            0.3,
            1.9,
            6.4,
            w / 2.0 - 5.9,
            4.4,
            l - pl - 10.0,
        );
        let arcade = self.nn().arcade;
        let sgeo = sign_geometry(&arcade, 6.0, 1.75);
        self.b.put(
            "neon",
            &sgeo,
            w / 2.0 - 6.08,
            5.35,
            l - pl - 10.0,
            [0.0, -PI / 2.0, 0.0],
            [1.0; 3],
        );
        // Snack kiosk and tables on the platform.
        b0(&mut self.b, "wYellow", 5.0, 3.0, 4.0, 3.0, 0.0, l - 9.0);
        b0(&mut self.b, "awnRed", 5.6, 0.3, 4.6, 3.0, 3.0, l - 9.0);
        for (x, z) in [
            (-1.5, l - 16.0),
            (4.0, l - 18.0),
            (9.5, l - 15.0),
            (9.0, l - 6.0),
        ] {
            picnic_table(&mut self.b, x, z);
        }

        self.build_ferris(&fr, deck_y, 12.0, l - 45.0);
        self.build_coaster(&fr, deck_y, l);
    }

    fn build_ferris(&mut self, fr: &Fr, deck_y: f64, lx: f64, lz: f64) {
        let mut b = Builder::new();
        let r = 15.0;
        let ax = 20.0;
        // Rotating part in its own frame: axle along local X, wheel in the
        // YZ plane.
        let mut ring = torus_geometry(r, 0.22, 6.0, 64.0, PI * 2.0);
        ring.rotate_y(PI / 2.0);
        for x in [-1.4, 1.4] {
            b.put_at("w", &ring, x, 0.0, 0.0);
        }
        let mut ring2 = torus_geometry(r * 0.55, 0.12, 5.0, 40.0, PI * 2.0);
        ring2.rotate_y(PI / 2.0);
        for x in [-1.4, 1.4] {
            b.put_at("w", &ring2, x, 0.0, 0.0);
        }
        let spokes = 16;
        for i in 0..spokes {
            let a = (f64::from(i) / f64::from(spokes)) * PI * 2.0;
            let y = kernel::sin(a) * r;
            let z = kernel::cos(a) * r;
            for x in [-1.4, 1.4] {
                b.beam("w", v3(x * 0.4, 0.0, 0.0), v3(x, y, z), 0.1);
            }
            b.beam("w", v3(-1.4, y, z), v3(1.4, y, z), 0.12);
        }
        b.put(
            "w",
            &cylinder_geometry(0.6, 0.6, 3.4, 12.0, 1.0, false, 0.0, PI * 2.0),
            0.0,
            0.0,
            0.0,
            [0.0, 0.0, PI / 2.0],
            [1.0; 3],
        );
        let wheel_geo = b.merge_all().expect("the wheel merges");
        let wheel_mat = self.mat(
            Material::standard()
                .set("color", 0xf4f1ea)
                .set("roughness", 0.5)
                .set("metalness", 0.3),
        );
        let pivot = self.graph.group("beach:ferris");
        let (wx, wz) = Self::to_world(fr, lx, lz);
        {
            let o = self.graph.get_mut(pivot);
            o.position = v3(wx, deck_y + ax, wz);
            o.set_rotation(&Euler::new(0.0, fr.yaw, 0.0));
        }
        let wheel = self.graph.group("");
        let wg = self.geo(wheel_geo);
        let wm = self.graph.mesh(wg, wheel_mat);
        self.graph.get_mut(wm).cast_shadow = true;
        self.graph.add(wheel, wm);
        // Bulbs on both rims.
        let bulb_mat = self.mat(
            Material::standard()
                .set("color", 0xfff2d0)
                .set("emissive", 0xffc870)
                .set("emissiveIntensity", 0.6),
        );
        self.bulb_mat = Some(bulb_mat);
        let nb = 48;
        let bg = self.geo(sphere_geometry(0.2, 6.0, 4.0, 0.0, PI * 2.0, 0.0, PI));
        let bulbs = self.graph.instanced_mesh(bg, bulb_mat, nb * 2);
        for i in 0..nb {
            let a = (f64::from(i) / f64::from(nb)) * PI * 2.0;
            for (j, x) in [(0, -1.5), (1, 1.5)] {
                let m4 = Matrix4::make_translation(
                    x,
                    kernel::sin(a) * (r + 0.35),
                    kernel::cos(a) * (r + 0.35),
                );
                self.inst(bulbs).set_matrix_at((i * 2 + j) as usize, &m4);
            }
        }
        self.graph.compute_instance_bounding_sphere(bulbs);
        self.graph.add(wheel, bulbs);
        self.graph.add(pivot, wheel);
        // Static A-frame legs.
        let mut legs = Builder::new();
        for x in [-3.2, 3.2] {
            for z in [-7.5, 7.5] {
                legs.beam("l", v3(x, -ax, z), v3(x * 0.5, 0.0, 0.0), 0.45);
            }
        }
        legs.beam(
            "l",
            v3(-3.2, -ax + 0.2, -7.5),
            v3(-3.2, -ax + 0.2, 7.5),
            0.35,
        );
        legs.beam("l", v3(3.2, -ax + 0.2, -7.5), v3(3.2, -ax + 0.2, 7.5), 0.35);
        let lg = self.geo(legs.merge_all().expect("the legs merge"));
        let leg_mesh = self.graph.mesh(lg, wheel_mat);
        self.graph.get_mut(leg_mesh).cast_shadow = true;
        self.graph.add(pivot, leg_mesh);
        // Gondolas hang level: an instanced mesh updated each frame.
        let mut gb = Builder::new();
        gb.cbox("g", 1.7, 1.3, 1.5, 0.0, -1.9, 0.0, [0.0; 3]);
        gb.cbox("g", 1.9, 0.12, 1.7, 0.0, -1.2, 0.0, [0.0; 3]);
        gb.beam("g", v3(0.0, -1.2, 0.0), v3(0.0, 0.0, 0.0), 0.08);
        let g_mat = self.mat(
            Material::standard()
                .set("color", 0xffffff)
                .set("roughness", 0.5),
        );
        let gg = self.geo(gb.merge_all().expect("the gondola merges"));
        let gondolas = self.graph.instanced_mesh(gg, g_mat, spokes);
        const PAL: [u32; 6] = [0xe24d6c, 0x2fa7c9, 0xf2c233, 0x3f9e6a, 0xff8a3c, 0x8f6ad8];
        for i in 0..spokes as usize {
            self.inst(gondolas)
                .set_color_at(i, Color::hex(PAL[i % PAL.len()]));
        }
        self.graph.get_mut(gondolas).frustum_culled = false;
        self.graph.add(pivot, gondolas);
        self.graph.add(self.group, pivot);
        let ferris = Ferris {
            wheel,
            gondolas,
            r,
            n: spokes as usize,
            angle: 0.0,
        };
        // `updateGondolas()` at build.
        for (i, m4) in gondola_matrices(&ferris).into_iter().enumerate() {
            self.inst(gondolas).set_matrix_at(i, &m4);
        }
        self.ferris = Some(ferris);
    }

    // A small wild-mouse coaster at the end of the pier with a train
    // running.
    fn build_coaster(&mut self, fr: &Fr, deck_y: f64, l: f64) {
        let base = [
            [-20.0, 1.5, l - 84.0],
            [-8.0, 3.0, l - 86.0],
            [-5.0, 9.0, l - 78.0],
            [-6.0, 13.0, l - 66.0],
            [-10.0, 13.5, l - 56.0],
            [-19.0, 9.0, l - 50.0],
            [-20.0, 4.0, l - 40.0],
            [-12.0, 6.0, l - 30.0],
            [-6.0, 10.0, l - 20.0],
            [-12.0, 12.0, l - 10.0],
            [-20.0, 7.0, l - 12.0],
            [-21.0, 3.0, l - 26.0],
            [-21.0, 2.0, l - 60.0],
            [-21.0, 1.6, l - 76.0],
        ];
        let pts: Vec<Vector3> = base.iter().map(|p| v3(p[0], p[1], p[2])).collect();
        let curve = CatmullRomCurve3::new(pts, true, CurveType::Centripetal, 0.5);
        let n = 200;
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut sleepers = Builder::new();
        let mut supports = Builder::new();
        let unit = boxg(1.0, 1.0, 1.0);
        for i in 0..n {
            let u = f64::from(i) / f64::from(n);
            let p = curve.get_point_at(u);
            let tg = curve.get_tangent_at(u);
            let side = v3(-tg.z, 0.0, tg.x).normalize();
            left.push(p.add_scaled_vector(side, 0.55));
            right.push(p.add_scaled_vector(side, -0.55));
            if i % 2 == 0 {
                let q = Quaternion::from_unit_vectors(v3(1.0, 0.0, 0.0), side);
                let m = Matrix4::compose(v3(p.x, p.y - 0.12, p.z), q, v3(1.4, 0.1, 0.2));
                sleepers.add("s", &unit, Some(&m));
            }
            if i % 6 == 0 && p.y > 1.0 {
                supports.beam("s", v3(p.x, 0.0, p.z), v3(p.x, p.y - 0.15, p.z), 0.22);
            }
        }
        let mut rail_geo = Builder::new();
        for arr in [left, right] {
            let c = CatmullRomCurve3::new(arr, true, CurveType::Centripetal, 0.5);
            rail_geo.add("r", &tube_geometry(&c, 400.0, 0.09, 5.0, true), None);
        }
        let grp = self.graph.group("beach:coaster");
        {
            let o = self.graph.get_mut(grp);
            o.position = v3(fr.x, deck_y, fr.z);
            o.set_rotation(&Euler::new(0.0, fr.yaw, 0.0));
        }
        let rail_mat = self.mat(
            Material::standard()
                .set("color", 0xd8433f)
                .set("metalness", 0.5)
                .set("roughness", 0.4),
        );
        let sup_mat = self.mat(
            Material::standard()
                .set("color", 0xf2f2ee)
                .set("roughness", 0.6),
        );
        let rg = self.geo(rail_geo.merge_all().expect("the rails merge"));
        let rails = self.graph.mesh(rg, rail_mat);
        let parts = [
            sleepers.merge_all().expect("the sleepers merge"),
            supports.merge_all().expect("the supports merge"),
        ];
        let sg = self.geo(Self::merge_list(&parts).expect("the structure merges"));
        let sup = self.graph.mesh(sg, sup_mat);
        self.graph.get_mut(rails).cast_shadow = true;
        self.graph.get_mut(sup).cast_shadow = true;
        self.graph.add(grp, rails);
        self.graph.add(grp, sup);
        // Train of three cars.
        let mut cb = Builder::new();
        cb.cbox("c", 1.2, 0.7, 1.6, 0.0, 0.45, 0.0, [0.0; 3]);
        cb.cbox("c", 1.25, 0.25, 0.3, 0.0, 0.9, 0.6, [0.0; 3]);
        let train_mat = self.mat(
            Material::standard()
                .set("color", 0x2fa7c9)
                .set("metalness", 0.3)
                .set("roughness", 0.4),
        );
        let tg = self.geo(cb.merge_all().expect("the train merges"));
        let train = self.graph.instanced_mesh(tg, train_mat, 3);
        self.graph.get_mut(train).frustum_culled = false;
        self.graph.add(grp, train);
        self.graph.add(self.group, grp);
        let len = curve.get_length();
        self.coaster = Some(Coaster {
            curve,
            train,
            u: 0.0,
            len,
        });
    }

    // ── Marina ──────────────────────────────────────────────────────────

    fn build_marina(&mut self) -> usize {
        let t = self.t;
        let s = self.beach.marina_s;
        let wl = self.waterline_lat(s).unwrap_or(-110.0);
        let (mut fr, _) = self.frame_at(s, wl + 2.0, false);
        fr.y = 0.0;
        let dy = 0.75;
        let l = 110.0;
        let mut z = -4.0;
        while z < l + 20.0 {
            let (x, zz) = Self::to_world(&fr, 0.0, z);
            self.take(x, zz, 30.0);
            z += 8.0;
        }
        self.b.set_frame(fr.x, 0.0, fr.z, fr.yaw);
        let b0 = |b: &mut Builder, key: &str, w: f64, h: f64, d: f64, x: f64, y: f64, z: f64| {
            b.box_(key, w, h, d, x, y, z, 0.0, 0.0, 0.0)
        };
        // Main dock and finger piers on floats.
        b0(
            &mut self.b,
            "wood",
            3.2,
            0.3,
            l,
            0.0,
            dy - 0.3,
            l / 2.0 - 4.0,
        );
        b0(
            &mut self.b,
            "white",
            3.0,
            0.35,
            l,
            0.0,
            dy - 0.65,
            l / 2.0 - 4.0,
        );
        let mut k = 0;
        let mut z = 10.0;
        while z < l - 4.0 {
            for side in [-1.0, 1.0] {
                b0(
                    &mut self.b,
                    "wood",
                    12.0,
                    0.25,
                    1.4,
                    side * 7.6,
                    dy - 0.25,
                    z,
                );
                b0(
                    &mut self.b,
                    "white",
                    11.8,
                    0.3,
                    1.2,
                    side * 7.6,
                    dy - 0.55,
                    z,
                );
                for px in [3.5, 13.0] {
                    b0(
                        &mut self.b,
                        "woodDark",
                        0.3,
                        2.2,
                        0.3,
                        side * px,
                        dy - 1.4,
                        z + 0.8,
                    );
                }
                // A boat in most slips, bow toward the main dock.
                if self.rng.next_f64() < 0.8 {
                    let bx = side * 8.0;
                    let bz = z + 5.5;
                    self.b
                        .push_frame(bx, 0.05, bz, if side > 0.0 { -PI / 2.0 } else { PI / 2.0 });
                    if self.rng.next_f64() < 0.7 {
                        sailboat(&mut self.b, &mut self.rng);
                    } else {
                        motorboat(&mut self.b, &mut self.rng);
                    }
                    self.b.pop_frame();
                    k += 1;
                }
            }
            z += 11.0;
        }
        // Gangway down from the promenade across the sand.
        let p0 = t.point_at(s, -self.prom_out + 0.3);
        let (ex, ez) = Self::to_world(&fr, 0.0, -3.0);
        let y0 = self.gy(p0.x, p0.z) + 0.25;
        let mut path = Vec::new();
        for i in 0..=20 {
            let u = f64::from(i) / 20.0;
            path.push((lerp(p0.x, ex, u), lerp(p0.z, ez, u)));
        }
        let deck = self.mat_of("deckPlank").expect("deckPlank");
        let y_fn = |i: usize| lerp(y0, dy, i as f64 / 20.0);
        self.ribbon(&path, 2.6, deck, 0.0, "gangway", Some(&y_fn));
        // Harbour-master hut at the dock head, and a sign.
        self.b.set_frame(fr.x, 0.0, fr.z, fr.yaw);
        b0(&mut self.b, "wBlue", 4.0, 2.8, 3.5, 5.5, dy, 3.0);
        b0(&mut self.b, "white", 4.6, 0.3, 4.1, 5.5, dy + 2.8, 3.0);
        b0(&mut self.b, "white", 0.25, 4.2, 0.25, -3.0, dy - 0.2, 0.0);
        b0(&mut self.b, "white", 0.25, 4.2, 0.25, 3.0, dy - 0.2, 0.0);
        b0(&mut self.b, "white", 6.6, 1.4, 0.25, 0.0, dy + 3.9, 0.0);
        let marina = self.sg().marina;
        let sgeo = sign_geometry(&marina, 6.2, 1.2);
        self.b.put(
            "signs",
            &sgeo,
            0.0,
            dy + 4.6,
            -0.14,
            [0.0, PI, 0.0],
            [1.0; 3],
        );
        // Breakwater of rocks enclosing the slips.
        let mut rocks: Vec<[f64; 5]> = Vec::new();
        let add_rock = |me: &mut Self, rocks: &mut Vec<[f64; 5]>, lx: f64, lz: f64| {
            let (x, z) = Self::to_world(&fr, lx, lz);
            let y = js::min(me.gy(x, z), 0.0) + rrange(&mut me.rng, -0.6, 0.2);
            let s = rrange(&mut me.rng, 1.6, 2.8);
            let r = me.rng.next_f64() * 6.0;
            rocks.push([x, z, y, s, r]);
        };
        let mut z = 20.0;
        while z < l + 16.0 {
            add_rock(self, &mut rocks, -30.0, z);
            add_rock(self, &mut rocks, 30.0, z);
            z += 2.6;
        }
        let mut x = -30.0;
        while x < -8.0 {
            add_rock(self, &mut rocks, x, l + 16.0);
            x += 2.6;
        }
        let mut x = 8.0;
        while x <= 30.0 {
            add_rock(self, &mut rocks, x, l + 16.0);
            x += 2.6;
        }
        let rg = self.geo(icosahedron_geometry(1.0, 0.0));
        let rm = self.mat(
            Material::standard()
                .set("color", 0x7c756c)
                .set("roughness", 0.95)
                .set("flatShading", true),
        );
        let im = self.graph.instanced_mesh(rg, rm, rocks.len() as u32);
        for (i, r) in rocks.iter().enumerate() {
            let [x, z, y, s, rr] = *r;
            let q = Quaternion::from_euler(&Euler::new(rr, rr * 1.3, 0.0));
            let m4 = Matrix4::compose(v3(x, y, z), q, v3(s, s * 0.75, s));
            self.inst(im).set_matrix_at(i, &m4);
        }
        self.graph.compute_instance_bounding_sphere(im);
        {
            let o = self.graph.get_mut(im);
            o.cast_shadow = true;
            o.name = "beach:breakwater".to_string();
        }
        self.graph.add(self.group, im);
        k
    }

    // ── Traffic lights and crosswalk at the pier intersection ───────────

    fn build_traffic_lights(&mut self) {
        let t = self.t;
        let s = self.beach.pier_s;
        // Crosswalk stripes on both sides of the intersection.
        for ds in [-7.0, 7.0] {
            let f = t.frame(s + ds);
            let mut lat = -f.hw + 0.6;
            while lat < f.hw - 0.3 {
                let p = t.point_at(s + ds, lat);
                self.b.set_frame(p.x, p.y + 0.02, p.z, yaw_z(f.fx, f.fz));
                self.b
                    .box_("paintWhite", 0.6, 0.02, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
                lat += 1.2;
            }
            // Stop line.
            let sl = s + ds * 1.55;
            let g = t.frame(sl);
            let lat0 = if ds < 0.0 { 0.3 } else { -g.hw + 0.3 };
            let lat1 = if ds < 0.0 { g.hw - 0.3 } else { -0.3 };
            let p = t.point_at(sl, (lat0 + lat1) / 2.0);
            self.b.set_frame(p.x, p.y + 0.02, p.z, yaw_z(g.fx, g.fz));
            self.b.box_(
                "paintWhite",
                lat1 - lat0,
                0.02,
                0.45,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            );
        }
        // Mast arms: ours on the far right corner, oncoming on their far right.
        let mast = |me: &mut Self, ss: f64, side: f64| {
            let f = me.t.frame(ss);
            let wall = if side > 0.0 { f.wall_r } else { f.wall_l };
            let (fr, _) = me.frame_at(ss, side * (wall + 1.1), true);
            me.b.set_frame(fr.x, fr.y + 0.2, fr.z, fr.yaw);
            let reach = wall + 1.1 - 2.2;
            let b = &mut me.b;
            b.box_("metal", 0.36, 8.3, 0.36, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            b.cbox("metal", 0.22, 0.22, reach, 0.0, 8.0, reach / 2.0, [0.0; 3]);
            // Signal heads face traffic coming toward them (−s for ours, +s
            // for oncoming).
            let face_yaw = if side > 0.0 { PI / 2.0 } else { -PI / 2.0 };
            for z in [reach * 0.45, reach * 0.95] {
                b.push_frame(0.0, 0.0, z, 0.0);
                b.box_("black", 0.45, 1.25, 0.4, 0.0, 6.6, 0.0, 0.0, 0.0, 0.0);
                b.box_("metal", 0.08, 0.35, 0.08, 0.0, 7.85, 0.0, 0.0, 0.0, 0.0);
                let mut lens = cyl(0.13, 0.13, 0.06, 10.0);
                lens.rotate_x(PI / 2.0);
                let dx = 0.23;
                b.put(
                    "sigRed",
                    &lens,
                    dx,
                    7.55,
                    0.0,
                    [0.0, face_yaw, 0.0],
                    [1.0; 3],
                );
                b.put(
                    "sigAmber",
                    &lens,
                    dx,
                    7.22,
                    0.0,
                    [0.0, face_yaw, 0.0],
                    [1.0; 3],
                );
                b.put(
                    "sigGreen",
                    &lens,
                    dx,
                    6.89,
                    0.0,
                    [0.0, face_yaw, 0.0],
                    [1.0; 3],
                );
                b.pop_frame();
            }
        };
        mast(self, s + 11.0, 1.0);
        mast(self, s - 11.0, -1.0);
    }

    fn build_town_sign(&mut self) {
        let t = self.t;
        let s = js::max(self.beach.zs0 + 80.0, self.beach.town_s0 - 110.0);
        let f = t.frame(s);
        let p = t.point_at(s, f.wall_r + 5.0);
        // Face drivers arriving down the hill (−s direction).
        let y = self.gy(p.x, p.z);
        self.b.set_frame(p.x, y, p.z, yaw_z(-f.fx, -f.fz));
        let b = &mut self.b;
        b.box_("concrete", 9.0, 0.6, 1.6, 0.0, -0.3, 0.0, 0.0, 0.0, 0.0);
        for x in [-3.6, 3.6] {
            b.box_("woodDark", 0.35, 3.2, 0.35, x, 0.0, 0.0, 0.0, 0.0, 0.0);
        }
        b.box_("white", 7.6, 3.9, 0.3, 0.0, 2.6, 0.0, 0.0, 0.0, 0.0);
        let welcome = self.sg().welcome;
        let sgeo = sign_geometry(&welcome, 7.2, 3.6);
        self.b.put_at("signs", &sgeo, 0.0, 4.55, 0.16);
        // Surfboard leaning on the sign.
        self.b.put(
            "boardB",
            &cyl(1.0, 1.0, 1.0, 14.0),
            4.6,
            2.2,
            0.4,
            [0.0, 0.0, -0.18],
            [0.3, 4.6, 0.05],
        );
        self.take(p.x, p.z, 5.0);
        for dx in [-7.0, 7.0] {
            let q = t.point_at(s + dx, f.wall_r + 6.0);
            self.add_palm(q.x, q.z, None, true);
        }
    }

    // ── Surf: foam lines washing up the beach ───────────────────────────

    fn build_foam(&mut self) {
        let t = self.t;
        let mut c = Canvas::new(256, 64);
        {
            let (w, h) = (256.0, 64.0);
            let g = &mut c;
            g.clear_rect(0.0, 0.0, w, h);
            let mut rng = Mulberry32::new(77);
            for _ in 0..260 {
                let x = rng.next_f64() * w;
                let y = (0.25 + 0.5 * kernel::pow(rng.next_f64(), 2.0)) * h;
                g.set_fill_style(format!("rgba(255,255,255,{})", 0.25 + rng.next_f64() * 0.5));
                g.begin_path();
                let rx = 4.0 + rng.next_f64() * 14.0;
                let ry = 1.5 + rng.next_f64() * 3.0;
                g.ellipse(x, y, rx, ry, 0.0, 0.0, 7.0, false);
                g.fill();
            }
            let mut grd = g.create_linear_gradient(0.0, 0.0, 0.0, h);
            grd.add_color_stop(0.0, "rgba(255,255,255,0)");
            grd.add_color_stop(0.35, "rgba(255,255,255,0.8)");
            grd.add_color_stop(0.5, "rgba(255,255,255,0.35)");
            grd.add_color_stop(1.0, "rgba(255,255,255,0)");
            g.set_fill_style(&grd);
            g.fill_rect(0.0, 0.0, w, h);
        }
        let foam = {
            let t = Arc::new(Texture::from_canvas(&c, true, true, 8.0));
            let desc = t.desc("", 0);
            self.graph.add_texture(Image::Own(t), desc)
        };
        let (town_s0, town_s1, marina_s) =
            (self.beach.town_s0, self.beach.town_s1, self.beach.marina_s);
        let mut mats: Vec<Option<(MaterialId, TextureId)>> = vec![None, None];
        for layer in 0..2usize {
            let lf = layer as f64;
            let mut pos: Vec<f64> = Vec::new();
            let mut uv: Vec<f64> = Vec::new();
            let mut idx: Vec<u32> = Vec::new();
            let mut rows = 0u32;
            let mut s = town_s0 - 60.0;
            while s < town_s1 + 60.0 {
                let cur = s;
                s += 6.0;
                if (cur - marina_s).abs() < 60.0 {
                    if rows > 1 {
                        self.foam_mesh(&pos, &uv, &idx, foam, &mut mats, layer);
                    }
                    pos.clear();
                    uv.clear();
                    idx.clear();
                    rows = 0;
                    continue;
                }
                let Some(wl) = self.waterline_lat(cur) else {
                    continue;
                };
                let off = lf * 9.0;
                for (lat, v) in [(wl + 3.0 - off, 0.0), (wl - 7.0 - off, 1.0)] {
                    let p = t.point_at(cur, lat);
                    pos.extend_from_slice(&[p.x, 0.05 + lf * 0.01, p.z]);
                    uv.extend_from_slice(&[cur / 40.0, v]);
                }
                if rows > 0 {
                    let a = (rows - 1) * 2;
                    idx.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
                }
                rows += 1;
            }
            if rows > 1 {
                self.foam_mesh(&pos, &uv, &idx, foam, &mut mats, layer);
            }
        }
        self.foam_mats = mats.into_iter().flatten().collect();
    }

    fn foam_mesh(
        &mut self,
        pos: &[f64],
        uv: &[f64],
        idx: &[u32],
        tex: TextureId,
        mats: &mut [Option<(MaterialId, TextureId)>],
        layer: usize,
    ) {
        let m = match mats[layer] {
            Some((m, _)) => m,
            None => {
                let t2 = self.graph.clone_texture(tex);
                {
                    let d = &mut self.graph.texture_mut(t2).desc;
                    d.wrap_s = three::REPEAT_WRAPPING;
                    d.wrap_t = three::REPEAT_WRAPPING;
                }
                let m = self.mat(
                    Material::lambert()
                        .set("map", t2)
                        .set("transparent", true)
                        .set("depthWrite", false)
                        .set("opacity", if layer == 1 { 0.55 } else { 0.85 })
                        .set("polygonOffset", true)
                        .set("polygonOffsetFactor", -3.0)
                        .set("polygonOffsetUnits", -3.0),
                );
                mats[layer] = Some((m, t2));
                m
            }
        };
        self.add_ribbon_mesh(pos, uv, idx, m, "foam");
    }
}

/// The gondolas' instance matrices (`updateGondolas`).
fn gondola_matrices(f: &Ferris) -> Vec<Matrix4> {
    (0..f.n)
        .map(|i| {
            let a = (i as f64 / f.n as f64) * PI * 2.0 + f.angle;
            Matrix4::make_translation(0.0, kernel::sin(a) * f.r, kernel::cos(a) * f.r)
        })
        .collect()
}

// ── Per-frame animation ─────────────────────────────────────────────────

/// `animate(dt, night)`: the wheel turns and its gondolas hang level, the
/// coaster's train runs, the surf washes in and out, the light pools fade
/// in after dusk and the traffic lights cycle.
struct Animate {
    time: f64,
    ferris: Option<Ferris>,
    coaster: Option<Coaster>,
    foam_mats: Vec<(MaterialId, TextureId)>,
    pool_mat: Option<MaterialId>,
    signal_t: f64,
    sig_green: MaterialId,
    sig_amber: MaterialId,
    sig_red: MaterialId,
}

fn number(m: MaterialId, prop: &'static str, value: f64) -> Edit {
    Edit {
        target: Handle::Material(m),
        change: Change::Number { prop, value },
    }
}

impl Animator for Animate {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        let dt = u.dt;
        let night = u.night;
        self.time += dt;
        if let Some(f) = &mut self.ferris {
            f.angle += dt * 0.06;
            let q = Quaternion::from_euler(&Euler::new(f.angle, 0.0, 0.0));
            out.push(Edit {
                target: Handle::Node(f.wheel),
                change: Change::Transform {
                    position: [0.0, 0.0, 0.0],
                    quaternion: [q.x, q.y, q.z, q.w],
                    scale: [1.0, 1.0, 1.0],
                },
            });
            for (i, m4) in gondola_matrices(f).into_iter().enumerate() {
                out.push(Edit {
                    target: Handle::Node(f.gondolas),
                    change: Change::InstanceMatrix {
                        index: i as u32,
                        matrix: m4.elements.map(|v| v as f32),
                    },
                });
            }
        }
        if let Some(c) = &mut self.coaster {
            // Speed from height: slow up the lift, fast down the drops.
            let p = c.curve.get_point_at(c.u);
            let v = js::max(3.0, js::max(0.0, 2.0 * 9.81 * (14.5 - p.y)).sqrt() + 3.0);
            c.u = (c.u + (v * dt) / c.len) % 1.0;
            let z_axis = v3(0.0, 0.0, 1.0);
            for i in 0..3 {
                let uu = (c.u - f64::from(i) * (2.0 / c.len) + 1.0) % 1.0;
                let pos = c.curve.get_point_at(uu);
                let tg = c.curve.get_tangent_at(uu);
                let q = Quaternion::from_unit_vectors(z_axis, tg);
                let m4 = Matrix4::compose(pos, q, v3(1.0, 1.0, 1.0));
                out.push(Edit {
                    target: Handle::Node(c.train),
                    change: Change::InstanceMatrix {
                        index: i as u32,
                        matrix: m4.elements.map(|v| v as f32),
                    },
                });
            }
        }
        // Waves washing in and out.
        for (i, &(m, t)) in self.foam_mats.iter().enumerate() {
            let fi = i as f64;
            let ph = self.time * (0.35 + fi * 0.1) + fi * 1.7;
            out.push(Edit {
                target: Handle::Texture(t),
                change: Change::TextureOffset([
                    self.time * 0.01 * if i == 1 { -1.0 } else { 1.0 },
                    kernel::sin(ph) * 0.18,
                ]),
            });
            out.push(number(
                m,
                "opacity",
                (if i == 1 { 0.45 } else { 0.75 }) + 0.2 * kernel::sin(ph + 1.0),
            ));
        }
        if let Some(pm) = self.pool_mat {
            out.push(number(pm, "opacity", 0.42 * smoothstep(0.08, 0.5, night)));
        }
        // Traffic lights: green 9 s, amber 2.5 s, red 7 s (visual only).
        self.signal_t = (self.signal_t + dt) % 18.5;
        let ph = self.signal_t;
        let on = 2.4 + night * 1.5;
        out.push(number(
            self.sig_green,
            "emissiveIntensity",
            if ph < 9.0 { on } else { 0.02 },
        ));
        out.push(number(
            self.sig_amber,
            "emissiveIntensity",
            if (9.0..11.5).contains(&ph) { on } else { 0.02 },
        ));
        out.push(number(
            self.sig_red,
            "emissiveIntensity",
            if ph >= 11.5 { on } else { 0.02 },
        ));
    }
}
