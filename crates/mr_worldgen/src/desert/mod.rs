//! `src/world/Desert.js` (roadmap WP 7.3): Desert Run, Level 4, all three
//! zones.
//!
//! Red Rock Canyon: the road threads a sandstone canyon (the walls and their
//! strata are terrain, see `Terrain.formCanyon`) past boulder falls,
//! junipers and an amphitheatre of hoodoos, and under a natural arch.
//! Route 66: the canyon opens onto a basin. A ranch fence lines the highway,
//! telephone poles march along the right, a railway runs parallel on the
//! left with a freight train the player catches and passes, and the Oasis
//! (motel, diner and gas station under palms and neon) sits halfway.
//! Silver Lake: the course crosses a dry lake bed marked out with cones,
//! flags and flares, past a spectators' camp, to a floodlit finish.
//!
//! The port keeps the JS structure: [`Desert::plan`] lays the railway bed,
//! the Oasis lot and the marshals' pull-out into the terrain before its
//! fields are built; [`Desert::build`] runs the JS `build*` methods in the
//! JS order on a [`Bld`] (the JS `this` during the build) into the group
//! `desert`, and pushes one [`Animator`] (`animate`: the glows' colours and
//! clock, the freight train, the rolling tumbleweeds). `desert/parts.js`,
//! `desert/props.js` and `desert/glow.js` are [`parts`], [`props`] and
//! [`glow`]; the canvas code is [`signs`]. The sign atlases, `paintedSign`,
//! `neonSign` and the Oasis buildings are Coast's `beach::atlas` and
//! `beach::parts` (WP 7.1), as the JS imports them from `beach/`.
//!
//! The material patches (`Sandstone`, `FlickerPoints`, `GroundPool`,
//! `FloodBeam`) are tagged kinds; their GLSL is the renderer's.

// Index loops stay index loops, and the JS signatures stay (D52, D130).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]
// The JS's forms stay: `f.fx * -1`, counters kept beside the loop index.
#![allow(clippy::neg_multiply, clippy::explicit_counter_loop)]

pub mod anim;
pub mod glow;
pub mod parts;
pub mod props;
pub mod signs;

use std::f64::consts::PI;

use mr_math::{Mulberry32, clamp, hash2, js, kernel, lerp, rpick, rrange, smoothstep};
use mr_scene::{LightDesc, NodeType, three};
use mr_track::{Frame, Track};
use serde_json::Value;

use crate::beach::atlas::{Rect, sign_geometry};
use crate::beach::parts::{GasOpts, MotelOpts, diner, gas_station, motel};
use crate::builder::{BuildOpts, Builder, CastShadow};
use crate::car_model::{BuildOpts as CarOpts, Lod, build_vehicle};
use crate::city::textures::{BannerOpts, banner_texture};
use crate::color::Color;
use crate::material::Material;
use crate::object::{GeoId, MaterialId, NodeId, SceneGraph};
use crate::road::Road;
use crate::terrain::{DesertRail, Terrain};
use crate::textures::TextureCache;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, EulerOrder, Matrix4, Quaternion, Vector3,
    merge_geometries, plane_geometry, sphere_geometry, torus_geometry,
};
use crate::valley::ground::Ground;
use crate::valley::parts::pole_geometry;
use crate::world::{Scenery, SceneryInfo, World};

use glow::{Glow, PointOpts, Pool, flicker_points, flicker_pools};
use parts::{
    Load, autorack_geometry, boxcar_geometry, bush_geometry, cone, cone_marker_geometry, cyl,
    gondola_geometry, hopper_geometry, joshua_geometry, juniper_geometry, locomotive_geometry,
    lumber_geometry, paint, palm_geometry, prep, saguaro_geometry, sandstone_material,
    stack_geometry, tank_geometry, yucca_geometry,
};
use props::{
    ArchOpts, RockKind, arch_geometry, beam_geometry, butte_geometry, camp_set_geometry,
    cholla_geometry, crack_decal_texture, delineator_geometry, dome_tent_geometry, hoodoo_geometry,
    motorhome, ocotillo_geometry, person_geometry, pinyon_geometry, rock_geometry, sage_geometry,
    tumbleweed_geometry,
};
use signs::{Signs, lakebed_texture, make_signs, own_texture, start_banner, ties_texture};

/// Metres of road per instancing chunk.
const CHUNK: f64 = 500.0;
/// Railway centreline, left of the road.
pub const RAIL_LAT: f64 = -64.0;
/// The JS writes a random yaw as `rng() * 6.3`, not 2π.
const TURN: f64 = 6.3;

const DOUBLE_SIDE: f64 = three::DOUBLE_SIDE as f64;
const ADDITIVE: f64 = three::ADDITIVE_BLENDING as f64;

/// Yaw that turns local +Z onto (dx, dz).
fn yaw_z(dx: f64, dz: f64) -> f64 {
    kernel::atan2(dx, dz)
}

/// Yaw that turns local +X onto (dx, dz).
fn yaw_x(dx: f64, dz: f64) -> f64 {
    kernel::atan2(-dz, dx)
}

/// The ColorBuilder's palette, in the JS object's order.
const PALETTE: &[(&str, u32)] = &[
    ("wPeach", 0xe8b48a),
    ("wTeal", 0x5fb0a8),
    ("wWhite", 0xefe9dc),
    ("wYellow", 0xf0cf7a),
    ("wSand", 0xd8c098),
    ("wPink", 0xe4a49a),
    ("wMint", 0xa9d8c1),
    ("trim", 0xf4efe4),
    ("concrete", 0xc2b8a8),
    ("white", 0xf2f0ea),
    ("roofTile", 0xb85c3c),
    ("roofTar", 0x4e4a46),
    ("black", 0x1b1b1d),
    ("paintRed", 0xc23a2e),
    ("wood", 0x8a6a4a),
    ("woodDark", 0x4e3a2a),
    ("steel", 0x6c7076),
    ("rust", 0x7a4a30),
    ("hay", 0xc8a860),
    ("tire", 0x1a1a1a),
    ("canvasW", 0xe8e2d4),
    ("canvasR", 0xc84a3a),
    ("canvasB", 0x3a6aa8),
];

/// `this.Z[i]`: a zone's range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub s0: f64,
    pub s1: f64,
}

/// The marshals' pull-out by the start (`this.pullout`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pullout {
    pub s: f64,
    pub x: f64,
    pub z: f64,
    pub y: f64,
}

/// The Desert Run scenery module.
pub struct Desert {
    pub label: &'static str,
    pub zone: usize,
    /// `this.Z`.
    pub z: Vec<Range>,
    /// `this.oasis.s`: the Oasis's place, when the route has one.
    pub oasis: Option<f64>,
    pub pullout: Option<Pullout>,
    /// The seed of the generator the rolling tumbleweeds draw from: the JS
    /// draws them from the page's `Math.random` (DECISIONS D552).
    pub random_seed: u32,
    /// The group `desert`, once built.
    pub group: Option<NodeId>,
}

impl Desert {
    /// `new Desert({ zone })`.
    pub fn new(info: &SceneryInfo) -> Desert {
        Desert {
            label: "Painting the desert",
            zone: info.zone,
            z: Vec::new(),
            oasis: None,
            pullout: None,
            random_seed: anim::RANDOM_SEED,
            group: None,
        }
    }

    fn setup_ranges(&mut self, t: &Track) {
        self.z = t
            .zones
            .iter()
            .map(|z| Range { s0: z.s0, s1: z.s1 })
            .collect();
    }
}

impl Scenery for Desert {
    fn name(&self) -> &str {
        "Desert"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    // ── Planning (before terrain fields) ────────────────────────────────
    fn plan(&mut self, world: &mut World) -> Result<(), String> {
        let t = world.track.as_ref().ok_or("the route is surveyed first")?;
        let tr = world.terrain.as_mut().ok_or("no terrain")?;
        self.setup_ranges(t);
        // Level bed for the railway beside the highway (see Terrain.formDesert).
        tr.desert_rail = Some(DesertRail {
            lat: RAIL_LAT,
            half: 6.0,
            drop: 0.7,
            s0: self.z[1].s0 - 260.0,
            s1: self.z[2].s0 + 600.0,
        });
        // The Oasis: a gravel lot on the right, level with the road.
        if let Some(oa) = t.tag("oasis").first() {
            let s = js::round((oa.s0 + oa.s1) / 2.0);
            self.oasis = Some(s);
            for k in -2..=2 {
                let ss = s + f64::from(k) * 30.0;
                let f = t.frame(ss);
                let p = t.point_at(ss, f.wall_r + 34.0);
                tr.add_flatten(p.x, p.z, 30.0, 22.0, Some(f.y - 0.25));
            }
        }
        // Start: a pull-out on the right for the marshals' trucks.
        let f0 = t.frame(t.start_s + 30.0);
        let pp = t.point_at(t.start_s + 30.0, f0.wall_r + 12.0);
        let p = Pullout {
            s: t.start_s + 30.0,
            x: pp.x,
            z: pp.z,
            y: f0.y - 0.15,
        };
        self.pullout = Some(p);
        tr.add_flatten(pp.x, pp.z, 11.0, 12.0, Some(p.y));
        Ok(())
    }

    // ── Build ───────────────────────────────────────────────────────────
    fn build(&mut self, world: &mut World) -> Result<(), String> {
        let World {
            track,
            terrain,
            graph,
            textures,
            road,
            root,
            animators,
            ..
        } = world;
        let t = track.as_ref().ok_or("the route is surveyed first")?;
        let terrain = terrain.as_ref().ok_or("no terrain")?;
        if self.z.is_empty() {
            self.setup_ranges(t);
        }
        let pullout = self.pullout.ok_or("the pull-out is planned first")?;
        let group = graph.group("desert");
        let n_chunks = ((t.length + 400.0) / CHUNK).ceil() as usize + 1;
        let m = Mats::make(graph, textures);
        let signs = make_signs(graph);
        let mut m = m;
        m.signs2 = graph.add_material(
            Material::standard()
                .set("map", signs.at2)
                .set("roughness", 0.6)
                .set("emissive", 0xffffff)
                .set("emissiveMap", signs.at2)
                .set("emissiveIntensity", 0.0)
                .set("transparent", true)
                .set("alphaTest", 0.5),
        );
        m.signs = graph.add_material(
            Material::standard()
                .set("map", signs.at)
                .set("roughness", 0.6)
                .set("emissive", 0xffffff)
                .set("emissiveMap", signs.at)
                .set("emissiveIntensity", 0.0)
                .set("transparent", true)
                .set("alphaTest", 0.5),
        );
        m.neon = graph.add_material(
            Material::standard()
                .set("map", signs.nt)
                .set("color", 0xffffff)
                .set("roughness", 0.5)
                .set("emissive", 0xffffff)
                .set("emissiveMap", signs.nt)
                .set("emissiveIntensity", 0.6),
        );
        let mut b = Bld {
            t,
            terrain,
            ground: Ground::new(terrain),
            road: road.as_ref(),
            graph,
            textures,
            group,
            z: self.z.clone(),
            occ: Vec::new(),
            n_chunks,
            m,
            signs,
            cb: Builder::new_color(PALETTE),
            oasis: self.oasis.map(|s| Oasis { s }),
            pullout,
            cars: Vec::new(),
            pools: Vec::new(),
            flares: Vec::new(),
            fires: Vec::new(),
            lanterns: Vec::new(),
            floods: Vec::new(),
            lamps: Vec::new(),
            reflectors: Vec::new(),
            bulbs: Vec::new(),
            strings: Vec::new(),
            lamp_pools: Vec::new(),
            beams: Vec::new(),
            weeds: Vec::new(),
            fence_weeds: Vec::new(),
            people: Vec::new(),
            shirts: None,
            mouth_s: 0.0,
            rail: None,
            train: None,
            rollers: None,
            glow_mats: Vec::new(),
            flare_mat: None,
            fire_mat: None,
            lantern_mat: None,
            flood_mat: None,
            lamp_mat: None,
            refl_mat: None,
            bulb_mat: None,
            string_mat: None,
            bb_lamp_mat: None,
            flame_mat: None,
            beam_mat: None,
            pool_mat: None,
        };

        b.build_start();
        b.build_arch();
        b.build_rocks();
        b.build_talus();
        b.build_horizon();
        b.build_vegetation();
        b.build_fence();
        b.build_poles();
        b.build_railway();
        b.build_train();
        b.build_oasis();
        b.build_road_signs();
        b.build_roadside();
        b.build_lakebed();
        b.build_playa();
        b.build_course();
        b.build_camp();
        b.build_finish();

        let mats = b.m.list();
        let meshes = b.cb.build(
            b.graph,
            &mats,
            &BuildOpts {
                cast_shadow: CastShadow::Keys(vec!["metal".into(), "neonFrame".into()]),
                receive_shadow: true,
            },
        );
        for mesh in meshes {
            b.add(mesh);
        }
        b.build_cars();
        b.build_glows();
        let m = b.m;
        let g = &mut *b.graph;
        g.add(*root, group);
        g.add_night(Some(m.glass_lit), "emissiveIntensity", 0.05, 1.6);
        g.add_night(Some(m.neon), "emissiveIntensity", 0.5, 3.4);
        g.add_night(Some(m.signs), "emissiveIntensity", 0.0, 0.25);
        g.add_night(Some(m.canopy_light), "emissiveIntensity", 0.3, 2.8);
        g.add_night(Some(m.lamp), "emissiveIntensity", 0.2, 5.0);
        g.add_night(Some(m.pool), "emissiveIntensity", 0.05, 0.7);
        g.add_night(Some(m.signs2), "emissiveIntensity", 0.0, 0.3);
        g.add_night(Some(m.rv_glass), "emissiveIntensity", 0.0, 1.2);
        let a = anim::DesertAnimator::new(&b, self.random_seed);
        animators.push(Box::new(a));
        self.group = Some(group);
        Ok(())
    }
}

/// `this.M`.
#[derive(Clone, Copy, Debug)]
pub struct Mats {
    pub solid: MaterialId,
    pub veg: MaterialId,
    pub bush: MaterialId,
    pub rock: MaterialId,
    pub glass: MaterialId,
    pub glass_lit: MaterialId,
    pub pool: MaterialId,
    pub paint_white: MaterialId,
    pub asphalt_lot: MaterialId,
    pub canopy_light: MaterialId,
    pub lamp: MaterialId,
    pub metal: MaterialId,
    pub rail: MaterialId,
    pub ballast: MaterialId,
    pub rv_glass: MaterialId,
    pub rust: MaterialId,
    pub paint_v: MaterialId,
    pub signs2: MaterialId,
    pub signs: MaterialId,
    pub neon: MaterialId,
}

impl Mats {
    /// `makeMaterials()` (the sign materials follow in `makeSigns`).
    fn make(graph: &mut SceneGraph, textures: &mut TextureCache) -> Mats {
        // S(color, o): MeshStandardMaterial({ color, roughness: 0.85,
        // metalness: 0, ...o }).
        let s = |hex: u32| {
            Material::standard()
                .set("color", hex)
                .set("roughness", 0.85)
                .set("metalness", 0.0)
        };
        let vc = |r: f64| {
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", r)
        };
        let solid = graph.add_material(vc(0.85));
        let veg = graph.add_material(vc(0.95));
        let bush = graph.add_material(vc(1.0));
        let rock = sandstone_material(graph, textures, "desert-rock");
        let rock = graph.add_material(rock);
        let glass = graph.add_material(s(0x2a3440).set("roughness", 0.15).set("metalness", 0.5));
        let glass_lit = graph.add_material(
            s(0x3a3632)
                .set("roughness", 0.2)
                .set("metalness", 0.2)
                .set("emissive", 0xffc98a)
                .set("emissiveIntensity", 0.0),
        );
        let pool = graph.add_material(
            s(0x3fc7d6)
                .set("roughness", 0.1)
                .set("emissive", 0x2aa6c0)
                .set("emissiveIntensity", 0.05),
        );
        let paint_white = graph.add_material(
            s(0xf0f0ea)
                .set("roughness", 0.7)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -2.0)
                .set("polygonOffsetUnits", -2.0),
        );
        let asphalt_lot = graph.add_material(
            s(0x55504a)
                .set("roughness", 0.95)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -1.0)
                .set("polygonOffsetUnits", -1.0),
        );
        let canopy_light = graph.add_material(
            s(0xffffff)
                .set("emissive", 0xf4f8ff)
                .set("emissiveIntensity", 0.3),
        );
        let lamp = graph.add_material(
            s(0xfff4dc)
                .set("emissive", 0xffd9a0)
                .set("emissiveIntensity", 0.2),
        );
        let metal = graph.add_material(s(0x9a9fa4).set("metalness", 0.7).set("roughness", 0.35));
        let rail = graph.add_material(s(0x8a8680).set("metalness", 0.8).set("roughness", 0.35));
        let ballast = graph.add_material(s(0x8a7a68).set("roughness", 1.0));
        let rv_glass = graph.add_material(
            s(0x2a2e34)
                .set("roughness", 0.2)
                .set("metalness", 0.3)
                .set("emissive", 0xffb06a)
                .set("emissiveIntensity", 0.0),
        );
        let rust = graph.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.9)
                .set("metalness", 0.15),
        );
        let paint_v = graph.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.5)
                .set("metalness", 0.3),
        );
        Mats {
            solid,
            veg,
            bush,
            rock,
            glass,
            glass_lit,
            pool,
            paint_white,
            asphalt_lot,
            canopy_light,
            lamp,
            metal,
            rail,
            ballast,
            rv_glass,
            rust,
            paint_v,
            // Filled in by makeSigns.
            signs2: solid,
            signs: solid,
            neon: solid,
        }
    }

    /// The JS object `this.M` as `build`'s `materials`, in its key order.
    fn list(&self) -> Vec<(&'static str, MaterialId)> {
        vec![
            ("solid", self.solid),
            ("veg", self.veg),
            ("bush", self.bush),
            ("rock", self.rock),
            ("glass", self.glass),
            ("glassLit", self.glass_lit),
            ("pool", self.pool),
            ("paintWhite", self.paint_white),
            ("asphaltLot", self.asphalt_lot),
            ("canopyLight", self.canopy_light),
            ("lamp", self.lamp),
            ("metal", self.metal),
            ("rail", self.rail),
            ("ballast", self.ballast),
            ("rvGlass", self.rv_glass),
            ("rust", self.rust),
            ("paintV", self.paint_v),
            ("signs2", self.signs2),
            ("signs", self.signs),
            ("neon", self.neon),
        ]
    }
}

/// One instance: `{ x, y, z, sx, sy?, sz?, rx?, ry?, rz?, col?, b? }`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Item {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub sx: f64,
    pub sy: Option<f64>,
    pub sz: Option<f64>,
    pub rx: f64,
    pub ry: f64,
    pub rz: f64,
    pub col: Option<u32>,
    pub b: Option<f64>,
}

impl Item {
    fn at(x: f64, y: f64, z: f64, sx: f64, ry: f64) -> Item {
        Item {
            x,
            y,
            z,
            sx,
            ry,
            ..Item::default()
        }
    }

    fn scaled(x: f64, y: f64, z: f64, sx: f64, sy: f64, sz: f64, ry: f64) -> Item {
        Item {
            x,
            y,
            z,
            sx,
            sy: Some(sy),
            sz: Some(sz),
            ry,
            ..Item::default()
        }
    }

    fn tint(mut self, col: u32, b: f64) -> Item {
        self.col = Some(col);
        self.b = Some(b);
        self
    }
}

/// A parked vehicle (`this.cars`): `{ kind, color, seed, x, y, z, yaw,
/// rust?, pitch?, roll? }`.
#[derive(Clone, Debug, PartialEq)]
pub struct CarSpot {
    pub kind: &'static str,
    pub color: u32,
    pub seed: u32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    pub rust: bool,
    pub pitch: f64,
    pub roll: f64,
}

/// A floodlight beam: from (x, y, z) to (tx, ty, tz).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Beam {
    x: f64,
    y: f64,
    z: f64,
    tx: f64,
    ty: f64,
    tz: f64,
}

/// The Oasis while it is built (`this.oasis`): its s.
#[derive(Clone, Copy, Debug)]
struct Oasis {
    s: f64,
}

/// A point with a yaw (`{ x, y, z, yaw }`).
#[derive(Clone, Copy, Debug)]
struct Pose {
    x: f64,
    y: f64,
    z: f64,
    yaw: f64,
}

/// A point of the railway's path.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RailPt {
    pub x: f64,
    pub z: f64,
    pub u: f64,
    pub y: f64,
    pub fx: f64,
    pub fz: f64,
}

/// `this.rail`: the path, its length and how much of it runs behind.
#[derive(Clone, Debug)]
pub struct Rail {
    pub pts: Vec<RailPt>,
    pub len: f64,
    pub back: f64,
}

/// `railAt(u)`: `{ x, y, z, fx, fz }` (a [`RailPt`] whose `u` is unset).
impl Rail {
    pub fn at(&self, u: f64) -> RailPt {
        let p = &self.pts;
        let i = clamp((u / 5.0).floor(), 0.0, (p.len() - 2) as f64) as usize;
        let f = clamp((u - p[i].u) / js::max(1e-3, p[i + 1].u - p[i].u), 0.0, 1.0);
        let (a, b) = (p[i], p[i + 1]);
        RailPt {
            x: lerp(a.x, b.x, f),
            y: lerp(a.y, b.y, f),
            z: lerp(a.z, b.z, f),
            fx: lerp(a.fx, b.fx, f),
            fz: lerp(a.fz, b.fz, f),
            u: 0.0,
        }
    }
}

/// One car of the freight train (`this.trainCars[i]`).
#[derive(Clone, Copy, Debug)]
pub struct TrainCar {
    pub off: f64,
    pub l: f64,
    /// The InstancedMesh of its type.
    pub mesh: NodeId,
    pub idx: u32,
}

/// The freight train as built.
pub struct Train {
    pub cars: Vec<TrainCar>,
    pub len: f64,
    pub glow: [NodeId; 3],
    pub glow_mat: MaterialId,
    pub beam: NodeId,
    pub target: NodeId,
    pub speed: f64,
}

/// What `this` holds during `build()`.
pub struct Bld<'a> {
    pub t: &'a Track,
    pub terrain: &'a Terrain,
    pub ground: Ground<'a>,
    road: Option<&'a Road>,
    pub graph: &'a mut SceneGraph,
    textures: &'a mut TextureCache,
    pub group: NodeId,
    pub z: Vec<Range>,
    /// `this.occ`: `{ x, z, r }`.
    occ: Vec<[f64; 3]>,
    n_chunks: usize,
    pub m: Mats,
    signs: Signs,
    /// `this.B`: the ColorBuilder.
    cb: Builder,
    oasis: Option<Oasis>,
    pullout: Pullout,
    cars: Vec<CarSpot>,
    pools: Vec<Pool>,
    flares: Vec<Glow>,
    fires: Vec<Glow>,
    lanterns: Vec<Glow>,
    floods: Vec<Glow>,
    lamps: Vec<Glow>,
    reflectors: Vec<Glow>,
    bulbs: Vec<Glow>,
    strings: Vec<Glow>,
    lamp_pools: Vec<Glow>,
    beams: Vec<Beam>,
    weeds: Vec<Item>,
    fence_weeds: Vec<Item>,
    people: Vec<Item>,
    shirts: Option<Vec<u32>>,
    pub mouth_s: f64,
    pub rail: Option<Rail>,
    pub train: Option<Train>,
    /// The rolling tumbleweeds' InstancedMesh.
    pub rollers: Option<NodeId>,
    /// Every material with the shared `uTime` (`glowTime`).
    pub glow_mats: Vec<MaterialId>,
    pub flare_mat: Option<MaterialId>,
    pub fire_mat: Option<MaterialId>,
    pub lantern_mat: Option<MaterialId>,
    pub flood_mat: Option<MaterialId>,
    pub lamp_mat: Option<MaterialId>,
    pub refl_mat: Option<MaterialId>,
    pub bulb_mat: Option<MaterialId>,
    pub string_mat: Option<MaterialId>,
    pub bb_lamp_mat: Option<MaterialId>,
    pub flame_mat: Option<MaterialId>,
    pub beam_mat: Option<MaterialId>,
    pub pool_mat: Option<MaterialId>,
}

/// `roadSign`'s options: `{ lat = null, posts = 1, y0 = 1.6, face = -1,
/// key = 'signs' }`.
#[derive(Clone, Copy, Debug)]
struct SignOpts {
    lat: Option<f64>,
    posts: u32,
    y0: f64,
    face: f64,
    key: &'static str,
}

impl Default for SignOpts {
    fn default() -> Self {
        SignOpts {
            lat: None,
            posts: 1,
            y0: 1.6,
            face: -1.0,
            key: "signs",
        }
    }
}

impl Bld<'_> {
    // ── Helpers ─────────────────────────────────────────────────────────

    fn add(&mut self, n: NodeId) {
        self.graph.add(self.group, n);
    }

    pub fn gy(&self, x: f64, z: f64) -> f64 {
        self.ground.height(x, z)
    }

    fn zone_at(&self, s: f64) -> u8 {
        self.t.zone[self.t.idx(s)]
    }

    fn take(&mut self, x: f64, z: f64, r: f64) {
        self.occ.push([x, z, r]);
    }

    fn free(&self, x: f64, z: f64, r: f64) -> bool {
        for o in &self.occ {
            if kernel::pow(x - o[0], 2.0) + kernel::pow(z - o[1], 2.0) < kernel::pow(r + o[2], 2.0)
            {
                return false;
            }
        }
        true
    }

    /// Clear of every stretch of road, walls included.
    fn clear_of_road(&self, x: f64, z: f64, r: f64) -> bool {
        let info = self.terrain.road_info(x, z);
        if !info.near {
            return info.d > r + 14.0;
        }
        let i = self.t.idx(info.s);
        info.d - r >= js::max(f64::from(self.t.wall_l[i]), f64::from(self.t.wall_r[i])) + 0.8
    }

    fn chunk_of(&self, s: f64) -> usize {
        clamp((s / CHUNK).floor(), 0.0, (self.n_chunks - 1) as f64) as usize
    }

    fn chunks(&self) -> Vec<Vec<Item>> {
        vec![Vec::new(); self.n_chunks]
    }

    /// Merge every k neighbouring chunks: sparse props don't earn a draw
    /// call per 500 m.
    fn coarse(chunks: &[Vec<Item>], k: usize) -> Vec<Vec<Item>> {
        chunks.chunks(k).map(|c| c.concat()).collect()
    }

    /// Instanced mesh per chunk from item lists.
    fn add_instanced(
        &mut self,
        geo: GeoId,
        mat: MaterialId,
        chunks: &[Vec<Item>],
        cast: bool,
        receive: bool,
    ) {
        for items in chunks {
            if items.is_empty() {
                continue;
            }
            let im = self.graph.instanced_mesh(geo, mat, items.len() as u32);
            {
                let inst = self
                    .graph
                    .get_mut(im)
                    .instances
                    .as_mut()
                    .expect("instanced");
                for (k, it) in items.iter().enumerate() {
                    let q = Quaternion::from_euler(&Euler::new(
                        js::or(it.rx, 0.0),
                        js::or(it.ry, 0.0),
                        js::or(it.rz, 0.0),
                    ));
                    let m = Matrix4::compose(
                        Vector3::new(it.x, it.y, it.z),
                        q,
                        Vector3::new(it.sx, it.sy.unwrap_or(it.sx), it.sz.unwrap_or(it.sx)),
                    );
                    inst.set_matrix_at(k, &m);
                    if let Some(col) = it.col {
                        let mut c = Color::hex(col);
                        c.multiply_scalar(it.b.unwrap_or(1.0));
                        inst.set_color_at(k, c);
                    }
                }
            }
            self.graph.compute_instance_bounding_sphere(im);
            let o = self.graph.get_mut(im);
            o.cast_shadow = cast;
            o.receive_shadow = receive;
            o.matrix_auto_update = false;
            o.update_matrix();
            self.add(im);
        }
    }

    fn geo(&mut self, g: BufferGeometry) -> GeoId {
        self.graph.add_geometry(g)
    }

    fn mat(&mut self, m: Material) -> MaterialId {
        self.graph.add_material(m)
    }

    /// A flat sign on a post (or two), facing back down the road.
    fn road_sign(&mut self, s: f64, side: f64, rect: Rect, w: f64, h: f64, o: SignOpts) {
        let f = self.t.frame(s);
        let l = match o.lat {
            Some(l) => l,
            None => side * ((if side > 0.0 { f.wall_r } else { f.wall_l }) + 0.9),
        };
        let x = f.x + f.rx * l;
        let z = f.z + f.rz * l;
        let y = self.gy(x, z);
        let b = &mut self.cb;
        b.set_frame(x, y, z, yaw_z(f.fx * o.face, f.fz * o.face));
        let px: Vec<f64> = if o.posts == 1 {
            vec![0.0]
        } else {
            vec![-w * 0.35, w * 0.35]
        };
        for p in px {
            b.box_yaw("steel", 0.09, o.y0 + h * 0.5, 0.09, p, 0.0, -0.06, 0.0);
        }
        b.put_at(o.key, &sign_geometry(&rect, w, h), 0.0, o.y0 + h / 2.0, 0.0);
        b.put(
            o.key,
            &sign_geometry(&rect, w, h),
            0.0,
            o.y0 + h / 2.0,
            -0.04,
            [0.0, PI, 0.0],
            [1.0; 3],
        );
    }

    // ── Start gantry and marshals' pull-out ─────────────────────────────
    fn build_start(&mut self) {
        let t = self.t;
        let sf = t.frame(t.start_s);
        let (wl, wr) = (sf.wall_l, sf.wall_r);
        let yaw = kernel::atan2(sf.fx, sf.fz);
        let h = 7.2;
        let place = |lat: f64| (sf.x + sf.rx * lat, sf.y, sf.z + sf.rz * lat);
        for lat in [-(wl + 1.3), wr + 1.3] {
            let (px, py, pz) = place(lat);
            let b = &mut self.cb;
            b.set_frame(px, py, pz, yaw);
            for off in [-0.35, 0.35] {
                b.box_yaw("steel", 0.16, h + 1.2, 0.16, 0.0, -1.0, off, 0.0);
            }
            for k in 0..7 {
                b.box_yaw("steel", 0.08, 0.08, 0.7, 0.0, 0.5 + f64::from(k), 0.0, 0.0);
            }
            b.box_yaw("concrete", 1.1, 0.5, 1.4, 0.0, -0.4, 0.0, 0.0);
        }
        let span = wl + wr + 2.6;
        let (cx, cy, cz) = place((wr - wl) / 2.0);
        self.cb.set_frame(cx, cy, cz, yaw);
        for dy in [h, h - 0.9] {
            self.cb
                .box_yaw("steel", span, 0.18, 0.18, 0.0, dy - 0.09, 0.0, 0.0);
        }
        let bw = span - 1.6;
        let bh = bw * 160.0 / 1024.0;
        for front in [true, false] {
            let tex = own_texture(self.graph, start_banner(front));
            let mat = self.mat(
                Material::standard()
                    .set("map", tex)
                    .set("roughness", 0.7)
                    .set("emissive", 0xffffff)
                    .set("emissiveMap", tex)
                    .set("emissiveIntensity", 0.2),
            );
            self.graph
                .add_night(Some(mat), "emissiveIntensity", 0.15, 0.7);
            let mut g = plane_geometry(bw, bh, 1.0, 1.0);
            g.rotate_y(if front { PI } else { 0.0 });
            g.translate(
                0.0,
                sf.y + h + 0.1 - bh / 2.0,
                if front { -0.05 } else { 0.05 },
            );
            g.rotate_y(yaw);
            g.translate(cx, 0.0, cz);
            let geo = self.geo(g);
            let mesh = self.graph.mesh(geo, mat);
            self.graph.get_mut(mesh).matrix_auto_update = false;
            self.add(mesh);
        }
        // Marshals' pull-out: gravel pad, a tow truck and a pickup.
        let d = self.pullout;
        let f = t.frame(d.s);
        let ry = yaw_z(f.fx, f.fz);
        self.cb.set_frame(d.x, d.y, d.z, ry);
        self.cb
            .box_yaw("asphaltLot", 18.0, 0.06, 16.0, 0.0, 0.02, 0.0, 0.0);
        let fy = kernel::atan2(f.fx, f.fz);
        self.cars.push(CarSpot {
            kind: "pickup",
            color: 0xd8d2c4,
            seed: 3,
            x: d.x - f.rx * 2.0 + f.fx * 4.0,
            y: d.y,
            z: d.z - f.rz * 2.0 + f.fz * 4.0,
            yaw: fy + 0.3,
            rust: false,
            pitch: 0.0,
            roll: 0.0,
        });
        self.cars.push(CarSpot {
            kind: "van",
            color: 0x2b2f36,
            seed: 5,
            x: d.x + f.rx * 3.0 - f.fx * 4.0,
            y: d.y,
            z: d.z + f.rz * 3.0 - f.fz * 4.0,
            yaw: fy - 0.2,
            rust: false,
            pitch: 0.0,
            roll: 0.0,
        });
        self.take(d.x, d.z, 14.0);
        let byway = self.signs.sg.byway;
        self.road_sign(
            t.start_s + 150.0,
            1.0,
            byway,
            4.2,
            1.4,
            SignOpts {
                posts: 2,
                y0: 1.2,
                ..SignOpts::default()
            },
        );
        // Behind the grid the road is closed: striped barricades and a sign.
        let fb = t.frame(1.5);
        let by = yaw_z(fb.fx, fb.fz);
        let mut lat = -fb.hw - 0.5;
        while lat <= fb.hw + 0.6 {
            let x = fb.x + fb.rx * lat;
            let z = fb.z + fb.rz * lat;
            let b = &mut self.cb;
            b.set_frame(x, fb.y, z, by);
            for px in [-0.9, 0.9] {
                b.box_("woodDark", 0.08, 1.1, 0.5, px, 0.0, 0.0, 0.0, 0.35, 0.0);
            }
            b.box_yaw("paintRed", 2.2, 0.22, 0.06, 0.0, 0.72, 0.1, 0.0);
            b.box_yaw("white", 2.2, 0.22, 0.06, 0.0, 0.42, 0.1, 0.0);
            lat += 2.4;
        }
        let closed = self.signs.sg.closed;
        self.road_sign(
            1.2,
            1.0,
            closed,
            2.6,
            1.0,
            SignOpts {
                lat: Some(0.0),
                posts: 2,
                y0: 1.3,
                face: 1.0,
                ..SignOpts::default()
            },
        );
        let p = t.point_at(t.start_s + 150.0, 9.0);
        self.take(p.x, p.z, 4.0);
    }

    // ── Natural arch spanning the road ──────────────────────────────────
    fn build_arch(&mut self) {
        let t = self.t;
        let Some(tag) = t.tag("arch").first().copied() else {
            return;
        };
        let s = js::round((tag.s0 + tag.s1) / 2.0);
        let f = t.frame(s);
        let r = 21.0;
        // Legs stand on whatever the canyon floor is doing either side.
        let gl = self.gy(f.x - f.rx * r, f.z - f.rz * r) - f.y;
        let gr = self.gy(f.x + f.rx * r, f.z + f.rz * r) - f.y;
        let mut merged = arch_geometry(
            17,
            &ArchOpts {
                r,
                h: 31.0,
                ground_l: gl,
                ground_r: gr,
                base: f.y,
            },
        );
        let yaw = yaw_x(f.rx, f.rz);
        merged.rotate_y(yaw);
        merged.translate(f.x, f.y, f.z);
        merged.compute_bounding_sphere();
        let geo = self.geo(merged);
        let mesh = self.graph.mesh(geo, self.m.rock);
        let o = self.graph.get_mut(mesh);
        o.cast_shadow = true;
        o.receive_shadow = true;
        o.matrix_auto_update = false;
        self.add(mesh);
        self.take(f.x + f.rx * r, f.z + f.rz * r, 12.0);
        self.take(f.x - f.rx * r, f.z - f.rz * r, 12.0);
    }

    // ── Hoodoos, boulder falls and slickrock outcrops ───────────────────
    fn build_rocks(&mut self) {
        let t = self.t;
        let mut rng = Mulberry32::new(311);
        let mut hoodoos: Vec<Vec<Vec<Item>>> = (0..4).map(|_| self.chunks()).collect();
        // Rockfall comes in three characters: weathered boulders, fresh broken
        // blocks and fallen slabs of ledge. The strata and varnish are in the
        // vertex colours, so instance tints only nudge brightness and hue.
        const KINDS: [RockKind; 3] = [RockKind::Round, RockKind::Block, RockKind::Slab];
        let mut boulders: Vec<Vec<Vec<Item>>> = (0..3).map(|_| self.chunks()).collect();
        const PAL: [u32; 5] = [0xffffff, 0xfff0e4, 0xf4e4d8, 0xffe8d0, 0xe8d8cc];
        let kind_of = |rng: &mut Mulberry32| {
            let r = rng.next_f64();
            if r < 0.4 {
                0
            } else if r < 0.8 {
                1
            } else {
                2
            }
        };
        let z_end = self.z[1].s0 + 250.0;
        // Hoodoo amphitheatre: the wide bay on the right.
        for tag in t.tag("hoodoos") {
            let mut s = tag.s0 - 20.0;
            while s < tag.s1 + 20.0 {
                let sf = t.frame(s);
                for _ in 0..3 {
                    let lat = sf.wall_r + rrange(&mut rng, 6.0, 120.0);
                    let x = sf.x + sf.rx * lat + (rng.next_f64() - 0.5) * 4.0;
                    let z = sf.z + sf.rz * lat + (rng.next_f64() - 0.5) * 4.0;
                    let h = lerp(6.0, 24.0, kernel::pow(rng.next_f64(), 1.3))
                        * lerp(0.7, 1.2, smoothstep(10.0, 100.0, lat - sf.wall_r));
                    let r = h * 0.2;
                    if !self.clear_of_road(x, z, r + 1.0) || !self.free(x, z, r * 0.8) {
                        continue;
                    }
                    if self.terrain.slope_at(x, z, 3.0) > 0.35 {
                        continue;
                    }
                    self.take(x, z, r * 0.6);
                    let y = self.gy(x, z);
                    let hk = (rng.next_f64() * 4.0).floor() as usize;
                    let ch = self.chunk_of(s);
                    let sx = h * lerp(0.9, 1.3, rng.next_f64());
                    let sz = h * lerp(0.9, 1.3, rng.next_f64());
                    let ry = rng.next_f64() * TURN;
                    let bb = lerp(0.85, 1.05, rng.next_f64());
                    hoodoos[hk][ch]
                        .push(Item::scaled(x, y - 0.4, z, sx, h, sz, ry).tint(0xffffff, bb));
                    // Spalled pieces round the pedestal.
                    for _ in 0..3 {
                        let a = rng.next_f64() * TURN;
                        let d = r * lerp(0.9, 2.2, rng.next_f64());
                        let px = x + kernel::cos(a) * d;
                        let pz = z + kernel::sin(a) * d;
                        let sz = h * lerp(0.03, 0.07, rng.next_f64());
                        if !self.clear_of_road(px, pz, sz) {
                            continue;
                        }
                        let kk = kind_of(&mut rng);
                        let ch = self.chunk_of(s);
                        let y = self.gy(px, pz) - sz * 0.2;
                        let sx = sz * lerp(0.8, 1.4, rng.next_f64());
                        let sy = sz * lerp(0.6, 1.0, rng.next_f64());
                        let ry = rng.next_f64() * TURN;
                        let col = *rpick(&mut rng, &PAL);
                        let bb = lerp(0.85, 1.05, rng.next_f64());
                        boulders[kk][ch]
                            .push(Item::scaled(px, y, pz, sx, sy, sz, ry).tint(col, bb));
                    }
                }
                s += 3.0;
            }
        }
        // Along the canyon: boulders fallen from the walls, the odd hoodoo on a
        // ledge, rock at the wall foot.
        let mut s = 20.0;
        while s < z_end {
            let sf = t.frame(s);
            for side in [-1.0, 1.0] {
                if rng.next_f64() < 0.45 {
                    continue;
                }
                let wall = if side > 0.0 { sf.wall_r } else { sf.wall_l };
                let rr = rrange(&mut rng, 2.0, 45.0);
                let mul = if rng.next_f64() < 0.3 { 2.5 } else { 1.0 };
                let lat = side * (wall + rr * mul);
                let x = sf.x + sf.rx * lat + (rng.next_f64() - 0.5) * 3.0;
                let z = sf.z + sf.rz * lat + (rng.next_f64() - 0.5) * 3.0;
                let big = rng.next_f64() < 0.18;
                let size = if big {
                    rrange(&mut rng, 2.5, 6.0)
                } else {
                    rrange(&mut rng, 0.5, 1.8)
                };
                if !self.clear_of_road(x, z, size) || !self.free(x, z, size * 0.7) {
                    continue;
                }
                let slope = self.terrain.slope_at(x, z, 2.0);
                if slope > 0.6 {
                    continue;
                }
                let kk = kind_of(&mut rng);
                let ch = self.chunk_of(s);
                let y = self.gy(x, z) - size * 0.25;
                let sx = size * lerp(0.8, 1.4, rng.next_f64());
                let sy = size * lerp(0.6, 1.0, rng.next_f64());
                let ry = rng.next_f64() * TURN;
                let col = *rpick(&mut rng, &PAL);
                let bb = lerp(0.8, 1.05, rng.next_f64());
                boulders[kk][ch].push(Item::scaled(x, y, z, sx, sy, size, ry).tint(col, bb));
                if big {
                    self.take(x, z, size * 0.8);
                }
                if rng.next_f64() < 0.05 && slope < 0.4 {
                    let h = rrange(&mut rng, 5.0, 14.0);
                    let hk = (rng.next_f64() * 4.0).floor() as usize;
                    let ch = self.chunk_of(s);
                    let y = self.gy(x, z) - 0.4;
                    let ry = rng.next_f64() * TURN;
                    hoodoos[hk][ch]
                        .push(Item::scaled(x, y, z, h * 1.1, h, h * 1.1, ry).tint(0xffffff, 0.95));
                }
            }
            s += 4.0;
        }
        // Scattered rock on the basin and lake shore: duller and browner than
        // the canyon's red sandstone.
        const DARK: [u32; 4] = [0xc8b0a4, 0xb8a094, 0xd0bcb0, 0xa89488];
        let mut s = self.z[1].s0;
        while s < t.length {
            let sf = t.frame(s);
            for side in [-1.0, 1.0] {
                if rng.next_f64() < 0.55 {
                    continue;
                }
                let lat = side * rrange(&mut rng, 12.0, 260.0);
                let x = sf.x + sf.rx * lat;
                let z = sf.z + sf.rz * lat;
                if self.zone_at(s) == 2 && lat.abs() < 400.0 {
                    continue;
                }
                let size = rrange(&mut rng, 0.4, 1.6);
                if !self.clear_of_road(x, z, size) || (lat - RAIL_LAT).abs() < 10.0 {
                    continue;
                }
                let ch = self.chunk_of(s);
                let y = self.gy(x, z) - size * 0.3;
                let ry = rng.next_f64() * TURN;
                let col = *rpick(&mut rng, &DARK);
                boulders[0][ch]
                    .push(Item::scaled(x, y, z, size * 1.2, size * 0.7, size, ry).tint(col, 1.0));
            }
            s += 9.0;
        }
        let hg: Vec<GeoId> = [11, 23, 37, 51]
            .iter()
            .map(|&sd| {
                let g = hoodoo_geometry(sd);
                self.geo(g)
            })
            .collect();
        let rock = self.m.rock;
        for (i, ch) in hoodoos.iter().enumerate() {
            self.add_instanced(hg[i], rock, ch, true, true);
        }
        for (i, k) in KINDS.iter().enumerate() {
            let g = rock_geometry(91 + i as u32 * 17, *k);
            let g = self.geo(g);
            self.add_instanced(g, rock, &Self::coarse(&boulders[i], 2), true, true);
        }
    }

    // ── Talus: scree aprons at the foot of the canyon walls ─────────────
    fn build_talus(&mut self) {
        let t = self.t;
        let mut rng = Mulberry32::new(5150);
        let mut scree = self.chunks();
        let mut blocks = self.chunks();
        const PAL: [u32; 4] = [0xffffff, 0xf8e8dc, 0xecd8c8, 0xfff2e0];
        let z_end = self.z[1].s0 + 150.0;
        let mut s = 10.0;
        while s < z_end {
            let sf = t.frame(s);
            for side in [-1.0, 1.0] {
                let wall = if side > 0.0 { sf.wall_r } else { sf.wall_l };
                // Walk outward to where the wall starts to climb: that's the foot.
                let mut foot = -1.0;
                let mut prev_y = self.gy(
                    sf.x + sf.rx * side * (wall + 1.0),
                    sf.z + sf.rz * side * (wall + 1.0),
                );
                let mut d = wall + 3.0;
                while d < wall + 130.0 {
                    let x = sf.x + sf.rx * side * d;
                    let z = sf.z + sf.rz * side * d;
                    let y = self.gy(x, z);
                    if y - prev_y > 1.1 {
                        foot = d;
                        break;
                    }
                    prev_y = y;
                    d += 2.5;
                }
                if foot < 0.0 {
                    continue;
                }
                // A fan of chips spilling out from the foot, thinning with distance;
                // a few blocks lodged higher up the slope.
                let n = 5 + (rng.next_f64() * 5.0).floor() as usize;
                for _ in 0..n {
                    // metres past the foot (negative = out on the floor)
                    let u = -kernel::pow(rng.next_f64(), 1.8) * 10.0 + rng.next_f64() * 2.0;
                    let d = foot + u;
                    let x = sf.x + sf.rx * side * d + sf.fx * (rng.next_f64() - 0.5) * 3.0;
                    let z = sf.z + sf.rz * side * d + sf.fz * (rng.next_f64() - 0.5) * 3.0;
                    if !self.clear_of_road(x, z, 1.0) || self.terrain.slope_at(x, z, 1.5) > 0.7 {
                        continue;
                    }
                    let sz = lerp(0.3, 1.2, kernel::pow(rng.next_f64(), 2.0))
                        * if u > -2.0 { 1.3 } else { 1.0 };
                    let ch = self.chunk_of(s);
                    let y = self.gy(x, z) - sz * 0.15;
                    let sx = sz * lerp(0.8, 1.5, rng.next_f64());
                    let sy = sz * lerp(0.5, 0.9, rng.next_f64());
                    let ry = rng.next_f64() * TURN;
                    let rx = (rng.next_f64() - 0.5) * 0.5;
                    let col = *rpick(&mut rng, &PAL);
                    let bb = lerp(0.75, 1.05, rng.next_f64());
                    let mut it = Item::scaled(x, y, z, sx, sy, sz, ry).tint(col, bb);
                    it.rx = rx;
                    scree[ch].push(it);
                }
                if rng.next_f64() < 0.35 {
                    let d = foot + rrange(&mut rng, -5.0, 1.5);
                    let x = sf.x + sf.rx * side * d;
                    let z = sf.z + sf.rz * side * d;
                    let sz = rrange(&mut rng, 1.2, 3.2);
                    if self.clear_of_road(x, z, sz)
                        && self.free(x, z, sz * 0.6)
                        && self.terrain.slope_at(x, z, 2.0) < 0.8
                    {
                        let ch = self.chunk_of(s);
                        let y = self.gy(x, z) - sz * 0.3;
                        let sx = sz * lerp(0.9, 1.4, rng.next_f64());
                        let sy = sz * lerp(0.6, 1.0, rng.next_f64());
                        let ry = rng.next_f64() * TURN;
                        let rz = (rng.next_f64() - 0.5) * 0.4;
                        let col = *rpick(&mut rng, &PAL);
                        let bb = lerp(0.8, 1.0, rng.next_f64());
                        let mut it = Item::scaled(x, y, z, sx, sy, sz, ry).tint(col, bb);
                        it.rz = rz;
                        blocks[ch].push(it);
                    }
                }
            }
            s += 3.0;
        }
        let rock = self.m.rock;
        let g = self.geo(rock_geometry(401, RockKind::Scree));
        self.add_instanced(g, rock, &Self::coarse(&scree, 3), false, true);
        let g = self.geo(rock_geometry(433, RockKind::Block));
        self.add_instanced(g, rock, &Self::coarse(&blocks, 3), true, true);
    }

    // ── Buttes and spires on the horizon ────────────────────────────────
    // Monument-style silhouettes standing off across the basin, kilometres
    // away: a single instanced mesh each for the two shapes.
    fn build_horizon(&mut self) {
        let t = self.t;
        let mut rng = Mulberry32::new(1966);
        let mut buttes = Vec::new();
        let mut spires = Vec::new();
        let s0 = self.z[0].s1 - 500.0;
        let s1 = self.z[2].s0 + 900.0;
        let mut s = s0;
        while s < s1 {
            let sf = t.frame(s);
            let side = if rng.next_f64() < 0.5 { -1.0 } else { 1.0 };
            let lat = side * rrange(&mut rng, 850.0, 2600.0);
            let x = sf.x + sf.rx * lat;
            let z = sf.z + sf.rz * lat;
            // Skip where the terrain already stands tall (its own mesas and the
            // far range): a butte there would just be buried.
            if self.terrain.road_info(x, z).d < 700.0 {
                s += 60.0;
                continue;
            }
            let y = self.gy(x, z);
            if y - sf.y > 25.0 {
                s += 60.0;
                continue;
            }
            if rng.next_f64() < 0.3 {
                let h = rrange(&mut rng, 90.0, 170.0);
                let sx = h * rrange(&mut rng, 0.14, 0.22);
                let sz = h * rrange(&mut rng, 0.14, 0.22);
                let ry = rng.next_f64() * TURN;
                let bb = lerp(0.9, 1.05, rng.next_f64());
                spires.push(Item::scaled(x, y - 6.0, z, sx, h, sz, ry).tint(0xffffff, bb));
            } else {
                let h = rrange(&mut rng, 60.0, 140.0);
                let r = h * rrange(&mut rng, 0.6, 1.6);
                let sz = r * rrange(&mut rng, 0.6, 1.1);
                let ry = rng.next_f64() * TURN;
                let bb = lerp(0.9, 1.05, rng.next_f64());
                buttes.push(Item::scaled(x, y - 8.0, z, r, h, sz, ry).tint(0xffffff, bb));
            }
            s += 60.0;
        }
        let rock = self.m.rock;
        let g = self.geo(butte_geometry(7, false));
        self.add_instanced(g, rock, &[buttes], false, false);
        let g = self.geo(butte_geometry(19, true));
        self.add_instanced(g, rock, &[spires], false, false);
    }

    // ── Plants ──────────────────────────────────────────────────────────
    fn build_vegetation(&mut self) {
        let t = self.t;
        let mut rng = Mulberry32::new(4401);
        let mut bushes = self.chunks();
        let mut junipers = self.chunks();
        let mut yuccas = self.chunks();
        let mut pinyons = self.chunks();
        let mut sage = self.chunks();
        let mut cholla = self.chunks();
        let mut ocotillo = self.chunks();
        let mut joshua: Vec<Vec<Vec<Item>>> = (0..3).map(|_| self.chunks()).collect();
        let mut saguaro: Vec<Vec<Vec<Item>>> = (0..2).map(|_| self.chunks()).collect();
        let mut weeds = Vec::new();
        // Creosote olive, brittlebush grey-green, dead-brown; sage silver-blue.
        const BUSH: [u32; 7] = [
            0x646440, 0x70683f, 0x585a38, 0x7a6e4c, 0x6e5e40, 0x7c7650, 0x867a58,
        ];
        const SAGE: [u32; 5] = [0x7e8c70, 0x72806a, 0x8a9678, 0x6a7a62, 0x94a080];
        let zc = self.z[1].s0;
        let zp = self.z[2].s0;
        self.mouth_s = t.tag("mouth").first().map_or(zc - 300.0, |m| m.s0);
        let mouth_s = self.mouth_s;
        // `bush(list, pal, lo, hi)`.
        let bush = |rng: &mut Mulberry32, x: f64, y: f64, z: f64, pal: &[u32], lo: f64, hi: f64| {
            let sx = rrange(rng, lo, hi);
            let sy = rrange(rng, lo * 1.1, hi * 1.1);
            let sz = rrange(rng, lo, hi);
            let ry = rng.next_f64() * TURN;
            let col = *rpick(rng, pal);
            let b = lerp(0.8, 1.1, rng.next_f64());
            Item::scaled(x, y - 0.05, z, sx, sy, sz, ry).tint(col, b)
        };
        let mut s = 0.0;
        while s < t.length {
            let sf = t.frame(s);
            let zone = self.zone_at(s);
            let in_canyon = s < zc + 150.0;
            let on_lake = s > zp + 250.0;
            for side in [-1.0, 1.0] {
                let wall = if side > 0.0 { sf.wall_r } else { sf.wall_l };
                // Distance out: dense near, thinning with distance.
                let reach = if in_canyon {
                    70.0
                } else if on_lake {
                    60.0
                } else {
                    420.0
                };
                let lat = side * (wall + 1.5 + reach * kernel::pow(rng.next_f64(), 1.6));
                let x = sf.x + sf.rx * lat + (rng.next_f64() - 0.5) * 2.0;
                let z = sf.z + sf.rz * lat + (rng.next_f64() - 0.5) * 2.0;
                if on_lake && lat.abs() < 900.0 {
                    // Out on the lake bed: nothing grows until the shore.
                    continue;
                }
                let r = rng.next_f64();
                let slope = self.terrain.slope_at(x, z, 2.0);
                if slope > 0.45 {
                    continue;
                }
                if (lat - RAIL_LAT).abs() < 8.0 && zone >= 1 {
                    continue;
                }
                if !self.clear_of_road(x, z, 1.2) || !self.free(x, z, 0.6) {
                    continue;
                }
                let y = self.gy(x, z);
                let ch = self.chunk_of(s);
                if in_canyon {
                    if r < 0.42 {
                        bushes[ch].push(bush(&mut rng, x, y, z, &BUSH, 0.5, 1.05));
                    } else if r < 0.62 {
                        sage[ch].push(bush(&mut rng, x, y, z, &SAGE, 0.6, 1.1));
                    } else if r < 0.68 && s < mouth_s {
                        let sx = rrange(&mut rng, 0.8, 1.4);
                        let ry = rng.next_f64() * TURN;
                        junipers[ch].push(Item::at(x, y - 0.2, z, sx, ry));
                        self.take(x, z, 1.5);
                    } else if r < 0.72 && s < mouth_s {
                        let sx = rrange(&mut rng, 0.7, 1.2);
                        let ry = rng.next_f64() * TURN;
                        pinyons[ch].push(Item::at(x, y - 0.2, z, sx, ry));
                        self.take(x, z, 1.5);
                    } else if r < 0.79 {
                        let sx = rrange(&mut rng, 0.7, 1.2);
                        let ry = rng.next_f64() * TURN;
                        yuccas[ch].push(Item::at(x, y - 0.05, z, sx, ry));
                    }
                } else {
                    let near = lat.abs() - wall < 60.0;
                    if r < 0.5 {
                        bushes[ch].push(bush(&mut rng, x, y, z, &BUSH, 0.5, 1.1));
                    } else if r < 0.66 {
                        sage[ch].push(bush(&mut rng, x, y, z, &SAGE, 0.5, 1.0));
                    } else if r < if near { 0.715 } else { 0.69 } {
                        let k = (rng.next_f64() * 3.0).floor() as usize;
                        let sx = rrange(&mut rng, 0.8, 1.35);
                        let ry = rng.next_f64() * TURN;
                        joshua[k][ch].push(Item::at(x, y - 0.15, z, sx, ry));
                        self.take(x, z, 2.0);
                    } else if r < if near { 0.728 } else { 0.70 } {
                        let k = (rng.next_f64() * 2.0).floor() as usize;
                        let sx = rrange(&mut rng, 0.85, 1.2);
                        let ry = rng.next_f64() * TURN;
                        saguaro[k][ch].push(Item::at(x, y - 0.2, z, sx, ry));
                        self.take(x, z, 1.2);
                    } else if r < 0.75 {
                        let sx = rrange(&mut rng, 0.8, 1.4);
                        let ry = rng.next_f64() * TURN;
                        let b = lerp(0.9, 1.15, rng.next_f64());
                        cholla[ch].push(Item::at(x, y - 0.05, z, sx, ry).tint(0xffffff, b));
                    } else if r < 0.765 {
                        let sx = rrange(&mut rng, 0.8, 1.2);
                        let ry = rng.next_f64() * TURN;
                        ocotillo[ch].push(Item::at(x, y - 0.05, z, sx, ry));
                    } else if r < 0.8 {
                        let sx = rrange(&mut rng, 0.6, 1.1);
                        let ry = rng.next_f64() * TURN;
                        yuccas[ch].push(Item::at(x, y - 0.05, z, sx, ry));
                    } else if r < 0.81 {
                        let sx = rrange(&mut rng, 0.45, 0.7);
                        let ry = rng.next_f64() * TURN;
                        let rx = rng.next_f64() * 3.0;
                        let mut it = Item::at(x, y + 0.35, z, sx, ry);
                        it.rx = rx;
                        weeds.push(it);
                    }
                }
            }
            s += 2.2;
        }
        // Route 66 roadside: a denser band of the showy species within ~45 m
        // of the fence, where the camera actually looks; the open basin
        // beyond stays sparse.
        let mut s = zc + 40.0;
        while s < zp + 120.0 {
            if let Some(o) = self.oasis
                && (s - o.s).abs() < 95.0
            {
                s += 5.0;
                continue;
            }
            let sf = t.frame(s);
            for side in [-1.0, 1.0] {
                let wall = if side > 0.0 { sf.wall_r } else { sf.wall_l };
                let lat = side * (wall + 2.5 + 42.0 * kernel::pow(rng.next_f64(), 1.3));
                if (lat - RAIL_LAT).abs() < 9.0 {
                    continue;
                }
                let x = sf.x + sf.rx * lat + (rng.next_f64() - 0.5) * 3.0;
                let z = sf.z + sf.rz * lat + (rng.next_f64() - 0.5) * 3.0;
                if !self.clear_of_road(x, z, 1.2) || !self.free(x, z, 1.0) {
                    continue;
                }
                let y = self.gy(x, z);
                let ch = self.chunk_of(s);
                let r = rng.next_f64();
                if r < 0.12 {
                    let k = (rng.next_f64() * 3.0).floor() as usize;
                    let sx = rrange(&mut rng, 0.75, 1.3);
                    let ry = rng.next_f64() * TURN;
                    joshua[k][ch].push(Item::at(x, y - 0.15, z, sx, ry));
                    self.take(x, z, 2.0);
                } else if r < 0.16 {
                    let k = (rng.next_f64() * 2.0).floor() as usize;
                    let sx = rrange(&mut rng, 0.8, 1.15);
                    let ry = rng.next_f64() * TURN;
                    saguaro[k][ch].push(Item::at(x, y - 0.2, z, sx, ry));
                    self.take(x, z, 1.2);
                } else if r < 0.34 {
                    let sx = rrange(&mut rng, 0.8, 1.5);
                    let ry = rng.next_f64() * TURN;
                    let b = lerp(0.9, 1.15, rng.next_f64());
                    cholla[ch].push(Item::at(x, y - 0.05, z, sx, ry).tint(0xffffff, b));
                } else if r < 0.40 {
                    let sx = rrange(&mut rng, 0.8, 1.2);
                    let ry = rng.next_f64() * TURN;
                    ocotillo[ch].push(Item::at(x, y - 0.05, z, sx, ry));
                } else if r < 0.56 {
                    let sx = rrange(&mut rng, 0.7, 1.2);
                    let ry = rng.next_f64() * TURN;
                    yuccas[ch].push(Item::at(x, y - 0.05, z, sx, ry));
                } else if r < 0.64 {
                    let sx = rrange(&mut rng, 0.45, 0.75);
                    let ry = rng.next_f64() * TURN;
                    let rx = rng.next_f64() * 3.0;
                    let mut it = Item::at(x, y + 0.35, z, sx, ry);
                    it.rx = rx;
                    weeds.push(it);
                } else {
                    let sx = rrange(&mut rng, 0.6, 1.2);
                    let sy = rrange(&mut rng, 0.6, 1.2);
                    let sz = rrange(&mut rng, 0.6, 1.2);
                    let ry = rng.next_f64() * TURN;
                    let col = *rpick(&mut rng, &BUSH);
                    let b = lerp(0.8, 1.1, rng.next_f64());
                    bushes[ch].push(Item::scaled(x, y - 0.05, z, sx, sy, sz, ry).tint(col, b));
                }
            }
            s += 5.0;
        }
        let (bush_m, veg) = (self.m.bush, self.m.veg);
        let g = self.geo(bush_geometry(12));
        self.add_instanced(g, bush_m, &Self::coarse(&bushes, 2), false, true);
        let g = self.geo(sage_geometry(21));
        self.add_instanced(g, bush_m, &Self::coarse(&sage, 2), false, true);
        let g = self.geo(juniper_geometry(5));
        self.add_instanced(g, veg, &junipers, true, true);
        let g = self.geo(pinyon_geometry(8));
        self.add_instanced(g, veg, &Self::coarse(&pinyons, 3), true, true);
        let g = self.geo(yucca_geometry(9));
        self.add_instanced(g, veg, &yuccas, false, true);
        let g = self.geo(cholla_geometry(31));
        self.add_instanced(g, veg, &Self::coarse(&cholla, 2), false, true);
        let g = self.geo(ocotillo_geometry(13));
        self.add_instanced(g, veg, &Self::coarse(&ocotillo, 3), false, true);
        for (i, sd) in [3, 17, 29].into_iter().enumerate() {
            let g = self.geo(joshua_geometry(sd));
            self.add_instanced(g, veg, &Self::coarse(&joshua[i], 2), true, true);
        }
        for (i, sd) in [7, 41].into_iter().enumerate() {
            let g = self.geo(saguaro_geometry(sd));
            self.add_instanced(g, veg, &Self::coarse(&saguaro[i], 2), true, true);
        }
        self.weeds = weeds;
    }

    // ── Ranch fence along the highway (the wall) ───────────────────────
    fn build_fence(&mut self) {
        let t = self.t;
        let s0 = self.z[1].s0 - 120.0;
        let s1 = self.z[2].s0 + 200.0;
        let mut posts = Vec::new();
        let mut wires: Vec<f64> = Vec::new();
        let mut refl = Vec::new();
        // `this.fenceGaps`: `{ s0, s1, side }`.
        let mut gaps: Vec<(f64, f64, f64)> = Vec::new();
        if let Some(o) = self.oasis {
            gaps.push((o.s - 70.0, o.s + 70.0, 1.0));
        }
        for side in [-1.0, 1.0] {
            let mut prev: Option<(f64, f64, f64)> = None;
            let mut k = 0u32;
            let mut s = s0;
            while s <= s1 {
                let sf = t.frame(s);
                let lat = side * ((if side > 0.0 { sf.wall_r } else { sf.wall_l }) + 0.25);
                let x = sf.x + sf.rx * lat;
                let z = sf.z + sf.rz * lat;
                let y = self.gy(x, z);
                let gap = gaps.iter().any(|g| g.2 == side && s > g.0 && s < g.1);
                if gap {
                    // In front of the Oasis: a kerb and painted bollards instead.
                    prev = None;
                    let b = &mut self.cb;
                    b.set_frame(x, sf.y, z, yaw_z(sf.fx, sf.fz));
                    b.box_yaw("concrete", 0.5, 0.25, 4.05, 0.0, -0.05, 0.0, 0.0);
                    for dz in [-1.0, 1.0] {
                        b.box_yaw("white", 0.22, 0.95, 0.22, 0.0, 0.0, dz, 0.0);
                        b.box_yaw("paintRed", 0.24, 0.16, 0.24, 0.0, 0.62, dz, 0.0);
                    }
                    s += 4.0;
                    k += 1;
                    continue;
                }
                let fade = smoothstep(s0, s0 + 80.0, s) * (1.0 - smoothstep(s1 - 80.0, s1, s));
                let kf = f64::from(k);
                let tall = if k.is_multiple_of(4) { 1.35 } else { 1.2 };
                posts.push(
                    Item::scaled(
                        x,
                        y - 0.3,
                        z,
                        1.0,
                        tall * lerp(0.4, 1.0, fade),
                        1.0,
                        hash2(kf, side, 0.0) * 3.0,
                    )
                    .tint(
                        if k.is_multiple_of(4) {
                            0x6a5846
                        } else {
                            0x7a6a56
                        },
                        1.0,
                    ),
                );
                // The JS item has no `b`.
                posts.last_mut().expect("pushed").b = None;
                if k.is_multiple_of(10) {
                    refl.push(Glow {
                        x: x - sf.rx * side * 0.08,
                        y: y + 0.85,
                        z: z - sf.rz * side * 0.08,
                        ph: None,
                    });
                }
                if let Some(p) = prev
                    && fade > 0.9
                {
                    for h in [0.45, 0.72, 0.98] {
                        wires.extend_from_slice(&[p.0, p.1 + h, p.2, x, y + h, z]);
                    }
                    // Now and then a tumbleweed has blown up against the wire.
                    if hash2(kf, side, 9.0) < 0.035 {
                        let mut it = Item::at(
                            x - sf.rx * side * 0.7,
                            y + 0.45,
                            z - sf.rz * side * 0.7,
                            0.5 + hash2(kf, side, 3.0) * 0.3,
                            kf,
                        );
                        it.rx = kf * 0.7;
                        self.fence_weeds.push(it);
                    }
                }
                prev = Some((x, y, z));
                s += 4.0;
                k += 1;
            }
        }
        let mut post = paint(
            prep(cyl(0.055, 0.07, 1.3, 5.0, 1.0, false)),
            0xffffff,
            0.0,
            None,
        );
        post.translate(0.0, 0.65, 0.0);
        let g = self.geo(post);
        let bush = self.m.bush;
        self.add_instanced(g, bush, &[posts], false, true);
        let mut wg = BufferGeometry::new();
        wg.set_attribute("position", BufferAttribute::from_f64(&wires, 3));
        wg.compute_bounding_sphere();
        let lm = self.mat(
            Material::line_basic()
                .set("color", 0x3a3632)
                .set("transparent", true)
                .set("opacity", 0.8),
        );
        let g = self.geo(wg);
        let lines = self.graph.drawable(NodeType::LineSegments, g, lm);
        self.graph.get_mut(lines).matrix_auto_update = false;
        self.add(lines);
        self.reflectors = refl;
    }

    // ── Telephone poles on the right ────────────────────────────────────
    fn build_poles(&mut self) {
        let t = self.t;
        // `{ x, y, z, rx, rz, yaw }`.
        let mut poles: Vec<[f64; 6]> = Vec::new();
        let mut s = self.z[1].s0 - 300.0;
        while s < self.z[2].s0 + 900.0 {
            let sf = t.frame(s);
            if let Some(o) = self.oasis
                && (s - o.s).abs() < 80.0
            {
                s += 46.0;
                continue;
            }
            let lat = sf.wall_r + 7.0;
            let x = sf.x + sf.rx * lat;
            let z = sf.z + sf.rz * lat;
            if !self.clear_of_road(x, z, 1.5) {
                s += 46.0;
                continue;
            }
            poles.push([x, self.gy(x, z) - 0.3, z, sf.rx, sf.rz, 0.0]);
            s += 46.0;
        }
        if poles.is_empty() {
            return;
        }
        let geo = pole_geometry(&mut Builder::new()).expect("the pole's parts merge");
        let items: Vec<Item> = poles
            .iter_mut()
            .enumerate()
            .map(|(k, p)| {
                let kf = k as f64;
                p[5] = yaw_x(p[3], p[4]) + (hash2(kf, 3.0, 0.0) - 0.5) * 0.06;
                let mut it = Item::at(p[0], p[1], p[2], 1.0, p[5]);
                it.rx = (hash2(kf, 5.0, 0.0) - 0.5) * 0.05;
                it.rz = (hash2(kf, 7.0, 0.0) - 0.5) * 0.05;
                it
            })
            .collect();
        let mat = self.mat(
            Material::standard()
                .set("color", 0x5e4c3a)
                .set("roughness", 0.95),
        );
        let g = self.geo(geo);
        self.add_instanced(g, mat, &[items], true, true);
        let mut wpos: Vec<f64> = Vec::new();
        for k in 0..poles.len() - 1 {
            let (a, b) = (poles[k], poles[k + 1]);
            let span = kernel::hypot(b[0] - a[0], b[2] - a[2]);
            if span > 120.0 {
                continue;
            }
            let sag = 0.5 + span * 0.012;
            for off in [-0.95, 0.0, 0.95] {
                let ax = a[0] + kernel::cos(a[5]) * off;
                let az = a[2] - kernel::sin(a[5]) * off;
                let ay = a[1] + 8.85;
                let bx = b[0] + kernel::cos(b[5]) * off;
                let bz = b[2] - kernel::sin(b[5]) * off;
                let by = b[1] + 8.85;
                const N: usize = 10;
                for i in 0..N {
                    for u in [i as f64 / N as f64, (i + 1) as f64 / N as f64] {
                        wpos.push(lerp(ax, bx, u));
                        wpos.push(lerp(ay, by, u) - sag * 4.0 * u * (1.0 - u));
                        wpos.push(lerp(az, bz, u));
                    }
                }
            }
        }
        let mut wg = BufferGeometry::new();
        wg.set_attribute("position", BufferAttribute::from_f64(&wpos, 3));
        wg.compute_bounding_sphere();
        let lm = self.mat(Material::line_basic().set("color", 0x1a1816));
        let g = self.geo(wg);
        let wires = self.graph.drawable(NodeType::LineSegments, g, lm);
        self.graph.get_mut(wires).matrix_auto_update = false;
        self.add(wires);
    }

    // ── Railway: path, ballast, ties and rails ──────────────────────────
    fn rail_path(&mut self) {
        let t = self.t;
        let sa = self.z[1].s0 - 200.0;
        let sb = js::min(t.finish_s - 300.0, self.z[2].s0 + 1400.0);
        let mut pts: Vec<RailPt> = Vec::new();
        let mut s = sa;
        while s <= sb {
            let sf = t.frame(s);
            // Beyond the highway the line drifts off along the lake shore.
            let lat = RAIL_LAT - 120.0 * kernel::pow(smoothstep(self.z[2].s0 + 200.0, sb, s), 1.5);
            pts.push(RailPt {
                x: sf.x + sf.rx * lat,
                z: sf.z + sf.rz * lat,
                ..RailPt::default()
            });
            s += 5.0;
        }
        // Extend: bend away to the left behind, carry straight on ahead.
        let ext = |a: RailPt, b: RailPt, n: usize, bend: f64| {
            let mut out = Vec::with_capacity(n);
            let mut dx = b.x - a.x;
            let mut dz = b.z - a.z;
            let l = kernel::hypot(dx, dz);
            dx /= l;
            dz /= l;
            let (mut x, mut z) = (b.x, b.z);
            let mut h = kernel::atan2(dz, dx);
            for k in 0..n {
                let kf = k as f64;
                h += bend * smoothstep(0.0, 60.0, kf) * (1.0 - smoothstep(100.0, 160.0, kf));
                x += kernel::cos(h) * 5.0;
                z += kernel::sin(h) * 5.0;
                out.push(RailPt {
                    x,
                    z,
                    ..RailPt::default()
                });
            }
            out
        };
        let mut back = ext(pts[1], pts[0], 400, 0.0022);
        back.reverse();
        let np = pts.len();
        let fwd = ext(pts[np - 2], pts[np - 1], 500, 0.0);
        let back_len = back.len();
        let mut all = back;
        all.extend(pts);
        all.extend(fwd);
        // Heights: the rendered ground, smoothed along the line.
        let raw: Vec<f64> = all.iter().map(|p| self.gy(p.x, p.z)).collect();
        let n = raw.len();
        let ys: Vec<f64> = (0..n)
            .map(|i| {
                let mut a = 0.0;
                let mut w = 0.0;
                for k in -10i64..=10 {
                    let j = clamp((i as i64 + k) as f64, 0.0, (n - 1) as f64) as usize;
                    let q = kernel::exp(-((k * k) as f64) / 30.0);
                    a += raw[j] * q;
                    w += q;
                }
                a / w
            })
            .collect();
        let mut u = 0.0;
        for i in 0..n {
            if i > 0 {
                u += kernel::hypot(all[i].x - all[i - 1].x, all[i].z - all[i - 1].z);
            }
            all[i].u = u;
            all[i].y = ys[i] + 0.55;
        }
        for i in 0..n {
            let a = all[i.saturating_sub(1)];
            let b = all[(i + 1).min(n - 1)];
            let dx = b.x - a.x;
            let dz = b.z - a.z;
            let l = js::or(kernel::hypot(dx, dz), 1.0);
            all[i].fx = dx / l;
            all[i].fz = dz / l;
        }
        self.rail = Some(Rail {
            pts: all,
            len: u,
            back: back_len as f64 * 5.0,
        });
    }

    fn build_railway(&mut self) {
        self.rail_path();
        // Profile across: [lat, dy, u?]. Ballast shoulders run down into the
        // ground.
        let sweep =
            |b: &mut Self, prof: &[(f64, f64, Option<f64>)], mat: MaterialId, us: f64, vs: f64| {
                let p = &b.rail.as_ref().expect("a rail").pts;
                let mut pos: Vec<f64> = Vec::new();
                let mut uv: Vec<f64> = Vec::new();
                let mut idx: Vec<u32> = Vec::new();
                let n = prof.len();
                for (i, q) in p.iter().enumerate() {
                    let rx = -q.fz;
                    let rz = q.fx;
                    for &(lat, dy, uu) in prof {
                        pos.extend_from_slice(&[q.x + rx * lat, q.y + dy, q.z + rz * lat]);
                        uv.push(uu.unwrap_or(lat / us));
                        uv.push(q.u / vs);
                    }
                    if i < p.len() - 1 {
                        for k in 0..n - 1 {
                            let a = (i * n + k) as u32;
                            let bb = a + 1;
                            let c = a + n as u32;
                            let d = c + 1;
                            idx.extend_from_slice(&[a, c, bb, bb, c, d]);
                        }
                    }
                }
                let mut g = BufferGeometry::new();
                g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
                g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
                g.set_index(&idx);
                g.compute_vertex_normals();
                g.compute_bounding_sphere();
                let geo = b.geo(g);
                let m = b.graph.mesh(geo, mat);
                let o = b.graph.get_mut(m);
                o.receive_shadow = true;
                o.matrix_auto_update = false;
                b.add(m);
            };
        // Ties texture: dark sleepers across pale ballast.
        let tex = own_texture(self.graph, ties_texture());
        let bed = self.mat(Material::standard().set("map", tex).set("roughness", 1.0));
        sweep(
            self,
            &[(2.6, 0.02, Some(0.0)), (-2.6, 0.02, Some(1.0))],
            bed,
            1.0,
            10.4,
        );
        let mut prof = [(5.6, -2.5), (2.6, 0.0), (-2.6, 0.0), (-5.6, -2.5)];
        prof.reverse();
        let prof: Vec<(f64, f64, Option<f64>)> =
            prof.iter().map(|&(a, b)| (a, b - 0.02, None)).collect();
        let ballast = self.m.ballast;
        sweep(self, &prof, ballast, 3.0, 3.0);
        let rail = self.m.rail;
        for lat in [0.72, -0.72] {
            let mut p = [
                (lat + 0.05, 0.2),
                (lat + 0.05, 0.05),
                (lat - 0.05, 0.05),
                (lat - 0.05, 0.2),
                (lat + 0.05, 0.2),
            ];
            p.reverse();
            let p: Vec<(f64, f64, Option<f64>)> = p.iter().map(|&(a, b)| (a, b, None)).collect();
            sweep(self, &p, rail, 1.0, 4.0);
        }
    }

    // ── Freight train pacing the player ─────────────────────────────────
    fn build_train(&mut self) {
        if self.rail.is_none() {
            return;
        }
        // (car, key): two units up front in the road's colours, a third in
        // another railroad's paint (a borrowed unit, as real consists often
        // have).
        let loco = std::rc::Rc::new(locomotive_geometry(0));
        let loco2 = std::rc::Rc::new(locomotive_geometry(1));
        let mut cars: Vec<(std::rc::Rc<parts::Car>, String)> = vec![
            (loco.clone(), "loco".into()),
            (loco, "loco".into()),
            (loco2, "loco2".into()),
        ];
        let types: Vec<std::rc::Rc<parts::Car>> = vec![
            boxcar_geometry(0x7a3a28, 1),
            boxcar_geometry(0x4a4e54, 2),
            boxcar_geometry(0x8a6a3a, 3),
            boxcar_geometry(0x2f5a7a, 14),
            tank_geometry(0x1e1e20, 4),
            tank_geometry(0xe0ddd4, 5),
            hopper_geometry(0x8a8e92, 6),
            hopper_geometry(0xb8b0a0, 7),
            stack_geometry([0x1d4f8a, 0xb3342b, 0xd9d4c7], 8),
            stack_geometry([0x2f7d3a, 0xe0b52b, 0x8a8f96], 9),
            stack_geometry([0xd8612a, 0x36414d, 0x1f7f86], 10),
            gondola_geometry(0x5a3a2a, 11, Load::Scrap),
            gondola_geometry(0x3a3a3e, 15, Load::Ballast),
            autorack_geometry(0xb8b4ac, 12),
            lumber_geometry(13),
        ]
        .into_iter()
        .map(std::rc::Rc::new)
        .collect();
        let mut rng = Mulberry32::new(2718);
        // Cars run in short blocks of one type, like a real manifest freight.
        let mut i = 0;
        while i < 28 {
            let k = (rng.next_f64() * types.len() as f64).floor() as usize;
            let mut n = 1 + (rng.next_f64() * 3.0).floor() as usize;
            while n > 0 && i < 28 {
                cars.push((types[k].clone(), format!("c{k}")));
                n -= 1;
                i += 1;
            }
        }
        // One InstancedMesh per car type: (key, car, idx list length).
        let mut groups: Vec<(String, std::rc::Rc<parts::Car>, u32)> = Vec::new();
        let mut off = 0.0;
        let mut entries: Vec<(f64, f64, usize, u32)> = Vec::new();
        for (c, key) in &cars {
            let gi = match groups.iter().position(|g| &g.0 == key) {
                Some(gi) => gi,
                None => {
                    groups.push((key.clone(), c.clone(), 0));
                    groups.len() - 1
                }
            };
            let idx = groups[gi].2;
            groups[gi].2 += 1;
            entries.push((off + c.l / 2.0, c.l, gi, idx));
            off += c.l + 1.2;
        }
        let len = off;
        let solid = self.m.solid;
        let mut meshes = Vec::new();
        // The same car geometry (the two lead units) is one geometry.
        let mut geos: Vec<(*const parts::Car, GeoId)> = Vec::new();
        for (_, car, count) in &groups {
            let ptr = std::rc::Rc::as_ptr(car);
            let geo = match geos.iter().find(|(p, _)| *p == ptr) {
                Some(&(_, g)) => g,
                None => {
                    let g = self.geo(car.geo.clone());
                    geos.push((ptr, g));
                    g
                }
            };
            let im = self.graph.instanced_mesh(geo, solid, *count);
            let o = self.graph.get_mut(im);
            o.cast_shadow = true;
            o.receive_shadow = true;
            o.frustum_culled = false;
            self.add(im);
            meshes.push(im);
        }
        let train_cars = entries
            .into_iter()
            .map(|(off, l, gi, idx)| TrainCar {
                off,
                l,
                mesh: meshes[gi],
                idx,
            })
            .collect();
        // Headlight glow sprite and ditch lights on the lead unit.
        let glow = self.graph.cached_texture(
            &self.textures.glow_texture(),
            crate::object::Layer::Main,
            "",
        );
        let gm = self.mat(
            Material::sprite()
                .set("map", glow)
                .set("color", 0xfff0c8)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", ADDITIVE),
        );
        let sg = self.geo(crate::mountain::sprite_geometry());
        let mut sprites = [NodeId(0); 3];
        for sp in &mut sprites {
            let n = self.graph.drawable(NodeType::Sprite, sg, gm);
            self.graph.get_mut(n).scale = Vector3::splat(4.0);
            self.add(n);
            *sp = n;
        }
        let mut beam = crate::object::Object3D::new(NodeType::SpotLight);
        // three's lights start at DEFAULT_UP.
        beam.position = Vector3::new(0.0, 1.0, 0.0);
        let mut c = Color::hex(0xfff0d0);
        c.multiply_scalar(1.0);
        beam.light = Some(LightDesc {
            node: 0,
            ty: NodeType::SpotLight,
            color: [c.r, c.g, c.b],
            intensity: 0.0,
            cast_shadow: false,
            ground_color: None,
            distance: Some(220.0),
            decay: Some(1.3),
            angle: Some(0.3),
            penumbra: Some(0.6),
            target: Some([0.0, 0.0, 0.0]),
            shadow: None,
        });
        let beam = self.graph.object(beam);
        let target = self
            .graph
            .object(crate::object::Object3D::new(NodeType::Object3D));
        self.add(beam);
        self.add(target);
        // Where the train is: parked out ahead until the player nears the
        // highway, then rolling at a steady 27 m/s.
        self.train = Some(Train {
            cars: train_cars,
            len,
            glow: sprites,
            glow_mat: gm,
            beam,
            target,
            speed: 27.0,
        });
    }

    // ── The Oasis: motel, diner and gas station ─────────────────────────
    fn build_oasis(&mut self) {
        let Some(oasis) = self.oasis else {
            return;
        };
        let t = self.t;
        let mut rng = Mulberry32::new(66);
        let s = oasis.s;
        let f = t.frame(s);
        // buildings' fronts face the road
        let yaw = yaw_z(-f.rx, -f.rz);
        let along = |ds: f64, lat: f64| {
            let fr = t.frame(s + ds);
            Pose {
                x: fr.x + fr.rx * lat,
                z: fr.z + fr.rz * lat,
                y: f.y - 0.25,
                yaw,
            }
        };
        let lot = along(0.0, f.wall_r + 34.0);
        // Gravel lot and an apron from the road.
        self.cb.set_frame(lot.x, lot.y, lot.z, yaw);
        self.cb
            .box_yaw("asphaltLot", 150.0, 0.08, 52.0, 0.0, 0.0, 0.0, 0.0);
        // Gas station nearest the approach, the diner in the middle, motel on.
        let gp = along(-48.0, f.wall_r + 20.0);
        self.cb.set_frame(gp.x, gp.y + 0.05, gp.z, yaw);
        gas_station(
            &mut self.cb,
            &mut rng,
            GasOpts {
                sign_rect: Some(self.signs.sg.gas),
                price_rect: Some(self.signs.sg.price),
            },
        );
        let dp = along(-2.0, f.wall_r + 24.0);
        self.cb.set_frame(dp.x, dp.y + 0.05, dp.z, yaw);
        diner(&mut self.cb, &mut rng, &self.signs.nn.diner);
        let mp = along(52.0, f.wall_r + 18.0);
        self.cb.set_frame(mp.x, mp.y + 0.05, mp.z, yaw);
        motel(
            &mut self.cb,
            &mut rng,
            MotelOpts {
                w: 44.0,
                sign_rect: None,
                neon_rect: Some(self.signs.nn.vacancy),
                wall: Some("wPeach"),
            },
        );
        // The big sign: a tall pole carrying the OASIS board and an arrow of
        // chaser bulbs pointing at the lot.
        let sp = along(-18.0, f.wall_r + 6.0);
        let sy = yaw_z(f.fx * -1.0, f.fz * -1.0) + 0.5;
        let (nn_oasis, nn_eat) = (self.signs.nn.oasis, self.signs.nn.eat);
        {
            let b = &mut self.cb;
            b.set_frame(sp.x, sp.y, sp.z, sy);
            b.box_yaw("steel", 0.5, 13.0, 0.5, -1.5, 0.0, 0.0, 0.0);
            b.box_yaw("steel", 0.5, 13.0, 0.5, 1.5, 0.0, 0.0, 0.0);
            b.box_yaw("black", 9.4, 3.2, 0.4, 0.0, 9.6, 0.0, 0.0);
            b.put_at("neon", &sign_geometry(&nn_oasis, 9.0, 2.9), 0.0, 11.2, 0.22);
            b.put(
                "neon",
                &sign_geometry(&nn_oasis, 9.0, 2.9),
                0.0,
                11.2,
                -0.22,
                [0.0, PI, 0.0],
                [1.0; 3],
            );
            b.box_yaw("black", 7.0, 1.1, 0.35, -0.5, 7.6, 0.0, 0.0);
            b.put_at("neon", &sign_geometry(&nn_eat, 2.1, 1.0), -2.5, 8.15, 0.2);
        }
        let sign = Pose {
            x: sp.x,
            y: sp.y,
            z: sp.z,
            yaw: sy,
        };
        // Palms round the lot and along the frontage.
        let mut palms = Vec::new();
        for k in 0..16 {
            let kf = f64::from(k);
            let ds = -70.0 + kf * 9.5 + (rng.next_f64() - 0.5) * 3.0;
            let p = along(
                ds,
                f.wall_r + 7.0 + f64::from(k % 2) * 2.5 + if k % 5 == 0 { 38.0 } else { 0.0 },
            );
            let sx = rrange(&mut rng, 0.85, 1.15);
            let ry = rng.next_f64() * TURN;
            palms.push(Item::at(p.x, p.y, p.z, sx, ry));
        }
        for k in 0..6 {
            let p = along(20.0 + f64::from(k) * 12.0, f.wall_r + 48.0);
            let sx = rrange(&mut rng, 0.9, 1.2);
            let ry = rng.next_f64() * TURN;
            palms.push(Item::at(p.x, p.y, p.z, sx, ry));
        }
        let veg = self.m.veg;
        let even: Vec<Item> = palms.iter().step_by(2).copied().collect();
        let odd: Vec<Item> = palms.iter().skip(1).step_by(2).copied().collect();
        let g = self.geo(palm_geometry(5));
        self.add_instanced(g, veg, &[even], true, true);
        let g = self.geo(palm_geometry(9));
        self.add_instanced(g, veg, &[odd], true, true);
        // Cars on the lot, big rigs parked by the diner.
        const KINDS: [&str; 6] = ["sedan", "pickup", "van", "hatch", "pickup", "sedan"];
        const COLS: [u32; 7] = [
            0xc9ccd1, 0x8a1c1c, 0x2e6d8e, 0xe8e6df, 0x4d5a3a, 0x9aa3ad, 0xb5a27a,
        ];
        for k in 0..9u32 {
            let p = along(30.0 + f64::from(k) * 4.1, f.wall_r + 26.0);
            let color = *rpick(&mut rng, &COLS);
            self.cars.push(CarSpot {
                kind: KINDS[k as usize % KINDS.len()],
                color,
                seed: k,
                x: p.x,
                y: p.y + 0.05,
                z: p.z,
                yaw: yaw + PI,
                rust: false,
                pitch: 0.0,
                roll: 0.0,
            });
        }
        for k in 0..3u32 {
            let p = along(-20.0 + f64::from(k) * 7.0, f.wall_r + 38.0);
            let color = *rpick(&mut rng, &[0xe8e6df, 0x8a1c1c, 0x1d3f75]);
            self.cars.push(CarSpot {
                kind: "boxtruck",
                color,
                seed: 20 + k,
                x: p.x,
                y: p.y + 0.05,
                z: p.z,
                yaw: yaw_z(f.fx, f.fz) + 1.2,
                rust: false,
                pitch: 0.0,
                roll: 0.0,
            });
        }
        // Lot lights: pools of warm light on the gravel.
        for k in 0..6 {
            let p = along(-60.0 + f64::from(k) * 24.0, f.wall_r + 14.0);
            let b = &mut self.cb;
            b.set_frame(p.x, p.y, p.z, yaw);
            b.box_yaw("steel", 0.2, 7.0, 0.2, 0.0, 0.0, 0.0, 0.0);
            b.box_yaw("steel", 1.4, 0.15, 0.2, 0.6, 6.9, 0.0, 0.0);
            b.box_yaw("lamp", 0.8, 0.12, 0.4, 1.1, 6.75, 0.0, 0.0);
            self.pools.push(Pool {
                x: p.x,
                y: p.y + 0.12,
                z: p.z,
                r: 12.0,
                c: [0.5, 0.36, 0.2],
                fl: None,
                ph: None,
            });
        }
        // Under the canopy.
        let cp = along(-48.0, f.wall_r + 13.0);
        self.pools.push(Pool {
            x: cp.x,
            y: cp.y + 0.14,
            z: cp.z,
            r: 16.0,
            c: [0.5, 0.55, 0.6],
            fl: None,
            ph: None,
        });
        self.oasis_extras(&along, yaw, &f, sign);
    }

    // Dressing round the Oasis: water tower, the MOTEL arrow, vending
    // machines, vintage pumps, a shade ramada, string lights and chaser bulbs.
    fn oasis_extras(&mut self, along: &dyn Fn(f64, f64) -> Pose, yaw: f64, f: &Frame, sg: Pose) {
        let w_ = |p: Pose, ry: f64, lx: f64, ly: f64, lz: f64| {
            (
                p.x + lx * kernel::cos(ry) + lz * kernel::sin(ry),
                p.y + ly,
                p.z - lx * kernel::sin(ry) + lz * kernel::cos(ry),
            )
        };
        let v = Vector3::new;
        // Water tower behind the diner, the town name on the tank.
        let wt = along(8.0, f.wall_r + 66.0);
        let tower = self.signs.sg.tower;
        {
            let b = &mut self.cb;
            b.set_frame(wt.x, wt.y, wt.z, yaw);
            let th = 15.0;
            for [lx, lz] in [[-2.6, -2.6], [2.6, -2.6], [-2.6, 2.6], [2.6, 2.6]] {
                b.beam("steel", v(lx * 1.25, 0.0, lz * 1.25), v(lx, th, lz), 0.28);
            }
            for h in [4.0, 9.0] {
                for [a, bb] in [
                    [[-1.0, -1.0], [1.0, -1.0]],
                    [[1.0, -1.0], [1.0, 1.0]],
                    [[1.0, 1.0], [-1.0, 1.0]],
                    [[-1.0, 1.0], [-1.0, -1.0]],
                ] {
                    let k = 1.25 - 0.25 * (h / th);
                    b.beam(
                        "steel",
                        v(a[0] * 2.6 * k, h, a[1] * 2.6 * k),
                        v(bb[0] * 2.6 * k, h + 4.0, bb[1] * 2.6 * k),
                        0.1,
                    );
                }
            }
            b.beam("steel", v(0.0, 0.0, 0.0), v(0.0, th, 0.0), 0.5);
            b.put_at(
                "white",
                &cyl(3.8, 3.8, 5.5, 20.0, 1.0, false),
                0.0,
                th + 2.75,
                0.0,
            );
            b.put_at(
                "roofTar",
                &cone(4.0, 2.0, 20.0, 1.0, false),
                0.0,
                th + 6.5,
                0.0,
            );
            b.put_at(
                "white",
                &cyl(4.2, 4.2, 0.2, 20.0, 1.0, false),
                0.0,
                th + 0.05,
                0.0,
            );
            b.put_at(
                "signs2",
                &sign_geometry(&tower, 5.2, 1.62),
                0.0,
                th + 2.9,
                3.86,
            );
        }
        // MOTEL arrow on its own pole at the motel end of the frontage.
        let ma = along(78.0, f.wall_r + 7.0);
        let may = yaw_z(-f.fx, -f.fz) + 0.6;
        let arrow = self.signs.nn.motel_arrow;
        {
            let b = &mut self.cb;
            b.set_frame(ma.x, ma.y, ma.z, may);
            b.box_yaw("steel", 0.3, 7.2, 0.3, 0.0, 0.0, 0.0, 0.0);
            b.box_yaw("black", 5.2, 2.7, 0.3, 0.0, 6.2, 0.0, 0.0);
            b.put_at("neon", &sign_geometry(&arrow, 5.0, 2.5), 0.0, 7.55, 0.17);
            b.put(
                "neon",
                &sign_geometry(&arrow, 5.0, 2.5),
                0.0,
                7.55,
                -0.17,
                [0.0, PI, 0.0],
                [1.0; 3],
            );
        }
        // Chaser bulbs round the big OASIS board and the arrow.
        let ring = |bulbs: &mut Vec<Glow>,
                    p: Pose,
                    ry: f64,
                    cx: f64,
                    cy: f64,
                    w: f64,
                    h: f64,
                    step: f64,
                    face: &[f64]| {
            let mut i = 0;
            let per = 2.0 * (w + h);
            let n = (per / step).floor() as usize;
            for k in 0..n {
                // `if (d < w) … else if ((d -= w) < h) … else if ((d -= h) < w)
                // … else { d -= w; … }`.
                let mut d = k as f64 * step;
                let (lx, ly) = if d < w {
                    (-w / 2.0 + d, h / 2.0)
                } else {
                    d -= w;
                    if d < h {
                        (w / 2.0, h / 2.0 - d)
                    } else {
                        d -= h;
                        if d < w {
                            (w / 2.0 - d, -h / 2.0)
                        } else {
                            d -= w;
                            (-w / 2.0, -h / 2.0 + d)
                        }
                    }
                };
                for &fz in face {
                    let (x, y, z) = w_(p, ry, cx + lx, cy + ly, fz);
                    bulbs.push(Glow {
                        x,
                        y,
                        z,
                        ph: Some(f64::from(i)),
                    });
                }
                i += 1;
            }
        };
        let mut bulbs = std::mem::take(&mut self.bulbs);
        ring(
            &mut bulbs,
            sg,
            sg.yaw,
            0.0,
            11.2,
            9.6,
            3.4,
            0.42,
            &[0.26, -0.26],
        );
        ring(&mut bulbs, sg, sg.yaw, -0.5, 8.15, 7.2, 1.3, 0.42, &[0.24]);
        ring(&mut bulbs, ma, may, 0.0, 7.55, 5.4, 2.9, 0.4, &[0.2, -0.2]);
        self.bulbs = bulbs;
        // Vending and ice machines by the motel office and the gas kiosk.
        let soda = self.signs.sg.soda;
        for (ds, lat, kind) in [
            (36.0, f.wall_r + 11.0, "soda"),
            (37.4, f.wall_r + 11.0, "soda"),
            (38.9, f.wall_r + 11.0, "ice"),
            (-40.0, f.wall_r + 37.0, "soda"),
            (-41.4, f.wall_r + 37.0, "ice"),
        ] {
            let p = along(ds, lat);
            let b = &mut self.cb;
            b.set_frame(p.x, p.y + 0.05, p.z, yaw);
            if kind == "soda" {
                b.box_yaw("paintRed", 1.0, 1.9, 0.8, 0.0, 0.0, 0.0, 0.0);
                b.put_at("signs2", &sign_geometry(&soda, 0.9, 1.75), 0.0, 0.97, 0.41);
            } else {
                b.box_yaw("white", 1.3, 1.8, 0.9, 0.0, 0.0, 0.0, 0.0);
                b.cbox("canvasB", 1.32, 0.35, 0.92, 0.0, 1.3, 0.0, [0.0; 3]);
                b.cbox("glassLit", 0.9, 0.3, 0.05, 0.0, 1.3, 0.46, [0.0; 3]);
            }
        }
        // Vintage visible-register pumps kept out front of the diner.
        for k in [-1.0, 1.0] {
            let p = along(-2.0 + k * 3.5, f.wall_r + 16.5);
            let b = &mut self.cb;
            b.set_frame(p.x, p.y + 0.05, p.z, yaw);
            b.box_yaw("concrete", 1.1, 0.25, 1.1, 0.0, 0.0, 0.0, 0.0);
            b.box_yaw("paintRed", 0.6, 2.2, 0.5, 0.0, 0.25, 0.0, 0.0);
            b.cbox("white", 0.62, 0.35, 0.52, 0.0, 1.6, 0.0, [0.0; 3]);
            b.put_at(
                "lamp",
                &sphere_geometry(0.28, 10.0, 6.0, 0.0, PI * 2.0, 0.0, PI),
                0.0,
                2.75,
                0.0,
            );
            b.box_yaw("black", 0.05, 0.9, 0.05, 0.33, 0.9, 0.1, 0.0);
        }
        // Shade ramada with picnic tables on the diner's far side.
        let rp = along(22.0, f.wall_r + 30.0);
        {
            let b = &mut self.cb;
            b.set_frame(rp.x, rp.y + 0.05, rp.z, yaw);
            for [px, pz] in [[-4.0, -2.5], [4.0, -2.5], [-4.0, 2.5], [4.0, 2.5]] {
                b.box_yaw("woodDark", 0.25, 2.9, 0.25, px, 0.0, pz, 0.0);
            }
            for k in 0..11 {
                b.box_yaw(
                    "wood",
                    9.4,
                    0.08,
                    0.28,
                    0.0,
                    2.95,
                    -2.8 + f64::from(k) * 0.56,
                    0.0,
                );
            }
            for [px, pz] in [[-2.2, 0.0], [2.2, 0.0]] {
                b.box_yaw("wood", 1.8, 0.08, 0.8, px, 0.75, pz, 0.0);
                for dz in [-0.65, 0.65] {
                    b.box_yaw("wood", 1.8, 0.06, 0.3, px, 0.45, pz + dz, 0.0);
                }
                for sx in [-0.7, 0.7] {
                    b.box_yaw("woodDark", 0.1, 0.75, 1.5, px + sx, 0.0, pz, 0.0);
                }
            }
        }
        // Stack of old tyres and a trash barrel by the kiosk.
        let tp = along(-58.0, f.wall_r + 30.0);
        for k in 0..5 {
            let kf = f64::from(k);
            let b = &mut self.cb;
            b.set_frame(tp.x, tp.y + kf * 0.28, tp.z, kf * 0.4);
            b.put(
                "tire",
                &torus_geometry(0.34, 0.14, 5.0, 10.0, PI * 2.0),
                0.0,
                0.14,
                0.0,
                [PI / 2.0, 0.0, 0.0],
                [1.0; 3],
            );
        }
        // Festoon lights zig-zagging over the diner forecourt.
        let mut posts = Vec::new();
        for k in 0..5 {
            let p = along(
                -14.0 + f64::from(k) * 7.0,
                f.wall_r + if k % 2 == 1 { 11.0 } else { 17.0 },
            );
            let b = &mut self.cb;
            b.set_frame(p.x, p.y, p.z, yaw);
            b.box_yaw("woodDark", 0.18, 5.0, 0.18, 0.0, 0.0, 0.0, 0.0);
            posts.push((p.x, p.y + 4.9, p.z));
        }
        for k in 0..posts.len() - 1 {
            let (a, b) = (posts[k], posts[k + 1]);
            let n = 12;
            for i in 1..n {
                let u = f64::from(i) / f64::from(n);
                self.strings.push(Glow {
                    x: lerp(a.0, b.0, u),
                    y: lerp(a.1, b.1, u) - 0.9 * 4.0 * u * (1.0 - u),
                    z: lerp(a.2, b.2, u),
                    ph: Some(f64::from(i) + k as f64 * 5.0),
                });
            }
        }
    }

    // ── Road signs, billboards, DIP warnings ────────────────────────────
    fn build_road_signs(&mut self) {
        let t = self.t;
        let sg = &self.signs.sg;
        let (dip, rocks, flood, route) = (sg.dip, sg.rocks, sg.flood, sg.route);
        let warn = SignOpts {
            y0: 1.4,
            ..SignOpts::default()
        };
        for tag in t.tag("dip") {
            self.road_sign(tag.s0 - 110.0, 1.0, dip, 1.1, 1.1, warn);
        }
        self.road_sign(260.0, 1.0, rocks, 1.1, 1.1, warn);
        self.road_sign(1500.0, 1.0, flood, 1.1, 1.1, warn);
        self.road_sign(
            self.z[1].s0 + 60.0,
            1.0,
            route,
            0.9,
            0.9,
            SignOpts {
                y0: 1.5,
                ..SignOpts::default()
            },
        );
        // Billboards on wooden legs, angled toward oncoming drivers.
        let z1 = self.z[1].s0;
        let sg = &self.signs.sg;
        let boards: [(f64, Rect, f64, f64, &str, bool); 9] = [
            (z1 + 140.0, sg.bb_gas, 1.0, 30.0, "signs", false),
            (
                self.oasis.map_or(z1 + 600.0, |o| o.s - 420.0),
                sg.bb_oasis,
                1.0,
                26.0,
                "signs",
                false,
            ),
            (z1 + 1450.0, sg.bb_jerky, -1.0, 28.0, "signs", false),
            (
                self.z[2].s0 - 380.0,
                sg.bb_trials,
                1.0,
                24.0,
                "signs",
                false,
            ),
            (z1 + 420.0, sg.bb_snakes, -1.0, 26.0, "signs2", false),
            (z1 + 1150.0, sg.bb_pie, 1.0, 22.0, "signs2", false),
            (z1 + 1800.0, sg.bb_motor, 1.0, 32.0, "signs2", false),
            (z1 + 2150.0, sg.bb_dino, -1.0, 24.0, "signs2", false),
            (z1 + 950.0, sg.bb_faded, -1.0, 40.0, "signs2", true),
        ];
        self.lamp_pools = Vec::new();
        for (i, (s, rect, side, off, key, wreck)) in boards.into_iter().enumerate() {
            let f = t.frame(s);
            let mut lat = side * ((if side > 0.0 { f.wall_r } else { f.wall_l }) + off);
            // The railway runs at RAIL_LAT on the left: stand boards clear of it.
            if side < 0.0 && (lat - RAIL_LAT).abs() < 14.0 {
                lat = RAIL_LAT + 16.0;
            }
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let y = self.gy(x, z);
            let ry = yaw_z(-f.fx, -f.fz) + side * 0.35;
            let b = &mut self.cb;
            b.set_frame(x, y, z, ry);
            for px in [-4.0, 0.0, 4.0] {
                b.box_yaw("wood", 0.3, 5.2, 0.3, px, -0.3, -0.4, 0.0);
            }
            b.box_yaw("woodDark", 11.0, 0.2, 1.2, 0.0, 3.7, -0.3, 0.0);
            // Catwalk and diagonal bracing behind the face.
            for px in [-4.0, 4.0] {
                b.box_("woodDark", 0.14, 4.4, 0.14, px, -0.2, -2.3, 0.0, 0.42, 0.0);
            }
            if wreck {
                // An abandoned board: the face has slumped on its legs.
                b.put(
                    key,
                    &sign_geometry(&rect, 10.6, 4.0),
                    0.3,
                    6.3,
                    0.0,
                    [0.0, 0.0, -0.08],
                    [1.0; 3],
                );
                b.box_("woodDark", 10.8, 4.2, 0.12, 0.3, 4.2, -0.1, 0.0, 0.0, -0.08);
            } else {
                b.put_at(key, &sign_geometry(&rect, 10.6, 4.0), 0.0, 6.8, 0.0);
                b.box_yaw("woodDark", 10.8, 4.2, 0.12, 0.0, 4.7, -0.1, 0.0);
                // Gooseneck lamps along the top: they light the face at night.
                for px in [-3.6, 0.0, 3.6] {
                    b.box_yaw("steel", 0.08, 0.08, 1.0, px, 8.95, 0.35, 0.0);
                    b.box_yaw("lamp", 0.5, 0.14, 0.3, px, 8.85, 0.85, 0.0);
                    let lx = x + kernel::cos(ry) * px + kernel::sin(ry) * 0.9;
                    let lz = z - kernel::sin(ry) * px + kernel::cos(ry) * 0.9;
                    self.lamp_pools.push(Glow {
                        x: lx,
                        y: y + 8.6,
                        z: lz,
                        ph: Some(i as f64 * 3.0 + px),
                    });
                }
            }
            self.take(x, z, 7.0);
        }
    }

    // ── Roadside life along Route 66 ────────────────────────────────────
    // Delineators and mile markers, abandoned wrecks out in the scrub, and
    // tumbleweeds: a few caught on the fence, a few blowing across the road.
    fn build_roadside(&mut self) {
        let t = self.t;
        let mut rng = Mulberry32::new(6606);
        let mut posts = Vec::new();
        let s0 = self.z[1].s0 - 60.0;
        let s1 = self.z[2].s0 + 150.0;
        let mut s = s0;
        while s < s1 {
            if let Some(o) = self.oasis
                && (s - o.s).abs() < 80.0
            {
                s += 64.0;
                continue;
            }
            let sf = t.frame(s);
            for side in [-1.0, 1.0] {
                let lat = side * ((if side > 0.0 { sf.wall_r } else { sf.wall_l }) - 0.35);
                let x = sf.x + sf.rx * lat;
                let z = sf.z + sf.rz * lat;
                posts.push(Item::at(
                    x,
                    self.gy(x, z) - 0.05,
                    z,
                    1.0,
                    yaw_z(-sf.fx, -sf.fz),
                ));
                self.reflectors.push(Glow {
                    x: x - sf.fx * 0.05,
                    y: self.gy(x, z) + 1.0,
                    z: z - sf.fz * 0.05,
                    ph: None,
                });
            }
            s += 64.0;
        }
        let g = self.geo(delineator_geometry());
        let veg = self.m.veg;
        self.add_instanced(g, veg, &[posts], false, true);
        // Mile markers on the right, one a mile, all the way to the lake.
        for k in 0..5 {
            let s = 700.0 + f64::from(k) * 1609.0;
            if s > t.finish_s - 200.0 {
                break;
            }
            let f = t.frame(s);
            let mm = self.signs.sg.mm[k as usize];
            self.road_sign(
                s,
                1.0,
                mm,
                0.36,
                0.96,
                SignOpts {
                    lat: Some(f.wall_r + 1.2),
                    y0: 0.9,
                    key: "signs2",
                    ..SignOpts::default()
                },
            );
        }
        // Wrecks: rusted-out cars abandoned in the desert, sunk to the sills.
        const RUST: [u32; 6] = [0x7a4a30, 0x6a4030, 0x8a5a3a, 0x5a4a3e, 0x7a6a50, 0x6e5a4a];
        let z1 = self.z[1].s0;
        let z2 = self.z[2].s0;
        let os = self.oasis.map_or(3500.0, |o| o.s);
        let wrecks: [(f64, f64, f64, &'static str); 8] = [
            (z1 + 330.0, 1.0, 28.0, "sedan"),
            (z1 + 700.0, -1.0, 22.0, "pickup"),
            (z1 + 1320.0, 1.0, 60.0, "hatch"),
            (z1 + 1620.0, 1.0, 18.0, "van"),
            (z1 + 2050.0, -1.0, 34.0, "sedan"),
            (z2 - 120.0, 1.0, 40.0, "pickup"),
            (os + 34.0, 1.0, 70.0, "sedan"),
            (os + 40.0, 1.0, 74.0, "hatch"),
        ];
        for (s, side, off, kind) in wrecks {
            let f = t.frame(s);
            let mut lat = side * ((if side > 0.0 { f.wall_r } else { f.wall_l }) + off);
            if (lat - RAIL_LAT).abs() < 12.0 {
                lat -= 16.0;
            }
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            if !self.clear_of_road(x, z, 3.0) || !self.free(x, z, 2.5) {
                continue;
            }
            self.take(x, z, 3.0);
            let color = *rpick(&mut rng, &RUST);
            let seed = 100 + (rng.next_f64() * 50.0).floor() as u32;
            let y = self.gy(x, z) - 0.25;
            let yaw = rng.next_f64() * TURN;
            let pitch = (rng.next_f64() - 0.5) * 0.12;
            let roll = (rng.next_f64() - 0.5) * 0.14;
            self.cars.push(CarSpot {
                kind,
                color,
                seed,
                x,
                y,
                z,
                yaw,
                rust: true,
                pitch,
                roll,
            });
        }
        // Tumbleweeds at rest: out in the scrub and against the fence.
        let still: Vec<Item> = self
            .weeds
            .iter()
            .chain(self.fence_weeds.iter())
            .map(|w| Item {
                col: Some(0xffffff),
                ..*w
            })
            .collect();
        let tg = self.geo(tumbleweed_geometry(3));
        let bush = self.m.bush;
        self.add_instanced(tg, bush, &[still], false, true);
        // Rolling ones, recycled round the player (see updateWeeds).
        const N: u32 = 7;
        let im = self.graph.instanced_mesh(tg, bush, N);
        {
            let o = self.graph.get_mut(im);
            o.frustum_culled = false;
            o.cast_shadow = true;
            let inst = o.instances.as_mut().expect("instanced");
            let m4 = Matrix4::make_scale(0.0, 0.0, 0.0);
            for i in 0..N as usize {
                inst.set_matrix_at(i, &m4);
            }
        }
        self.add(im);
        self.rollers = Some(im);
    }

    // ── The lake bed: a pale surface replaces the asphalt ───────────────
    fn build_lakebed(&mut self) {
        let t = self.t;
        let Some(road) = self.road else {
            return;
        };
        // Cracked mud: a canvas of polygon cracks.
        let tex = own_texture(self.graph, lakebed_texture());
        self.graph.texture_mut(tex).desc.repeat = [4.4, 2.5];
        let mat = self.mat(
            Material::standard()
                .set("map", tex)
                .set("roughness", 0.95)
                .set("color", 0xffffff),
        );
        let lake_from = self.z[2].s0 + 120.0;
        let children: Vec<NodeId> = self.graph.get(road.group).children.clone();
        // `Object.keys(materials).find(...)` matching /asphalt/: the asphalt
        // keys; the material must have a map.
        let covers = |g: &SceneGraph, c: NodeId, want: &dyn Fn(MaterialId) -> bool| {
            let o = g.get(c);
            if !o.ty.is_mesh() || o.materials.is_empty() {
                return false;
            }
            want(o.materials[0])
        };
        let asphalt: Vec<MaterialId> = road
            .materials
            .iter()
            .filter(|(k, _)| k.contains("asphalt"))
            .map(|&(_, m)| m)
            .collect();
        for pass in 0..2 {
            for &c in &children {
                let ok = if pass == 0 {
                    covers(self.graph, c, &|m| {
                        asphalt.contains(&m) && {
                            let mm = self.graph.material(m);
                            !matches!(mm.get("map"), None | Some(Value::Null))
                        }
                    })
                } else {
                    let shoulder = road.material("shoulder");
                    covers(self.graph, c, &|m| Some(m) == shoulder)
                };
                if !ok {
                    continue;
                }
                let geo = self.graph.get(c).geometry.expect("a mesh");
                let g = self.graph.geometry_mut(geo);
                g.compute_bounding_sphere();
                let ctr = g.bounding_sphere.expect("a sphere").center;
                let hint = t.nearest(ctr.x, ctr.z, 400.0);
                if hint < 0 {
                    continue;
                }
                let p = t.project_window(ctr.x, ctr.z, hint as f64, 30);
                if p.s > lake_from {
                    self.graph.get_mut(c).materials[0] = mat;
                }
            }
        }
    }

    // ── Course marking on the lake: cones, flags, flares, mile boards ───
    fn build_course(&mut self) {
        let t = self.t;
        let s0 = self.z[2].s0 + 180.0;
        let s1 = t.road_end() - 2.0;
        let mut cones = Vec::new();
        let mut flag_poles = Vec::new();
        let mut flags = Vec::new();
        self.flares = Vec::new();
        for side in [-1.0, 1.0] {
            let mut s = s0;
            let mut k = 0u32;
            while s < s1 {
                let sf = t.frame(s);
                let wall = if side > 0.0 { sf.wall_r } else { sf.wall_l };
                let lat = side * (wall + 0.3);
                let x = sf.x + sf.rx * lat;
                let z = sf.z + sf.rz * lat;
                let y = sf.y - 0.04;
                let kf = f64::from(k);
                cones.push(Item::at(x, y, z, 1.0, kf * 0.7));
                if k.is_multiple_of(6) {
                    let fl = side * (wall + 1.2);
                    let fx = sf.x + sf.rx * fl;
                    let fz = sf.z + sf.rz * fl;
                    flag_poles.push(Item::at(fx, y - 0.05, fz, 1.0, 0.0));
                    let mut it = Item::at(fx, y + 3.2, fz, 1.0, yaw_x(-sf.fx, -sf.fz));
                    it.col = Some(if (k / 6) % 2 == 1 { 0xff5a14 } else { 0xf2f2f2 });
                    flags.push(it);
                }
                if k % 3 == 1 {
                    self.flares.push(Glow {
                        x: x + sf.fx * 4.0,
                        y: y + 0.12,
                        z: z + sf.fz * 4.0,
                        ph: Some(hash2(kf, side, 0.0) * 10.0),
                    });
                }
                s += 9.0;
                k += 1;
            }
        }
        let veg = self.m.veg;
        let g = self.geo(cone_marker_geometry());
        self.add_instanced(g, veg, &[cones], false, true);
        let mut pole = paint(
            prep(cyl(0.03, 0.04, 3.6, 5.0, 1.0, false)),
            0xdddddd,
            0.0,
            None,
        );
        pole.translate(0.0, 1.8, 0.0);
        let g = self.geo(pole);
        self.add_instanced(g, veg, &[flag_poles], false, true);
        let mut flag_g = BufferGeometry::new();
        flag_g.set_attribute(
            "position",
            BufferAttribute::from_f64(&[0.0, 0.35, 0.0, 0.0, -0.35, 0.0, 1.2, 0.05, 0.0], 3),
        );
        flag_g.compute_vertex_normals();
        let flag_g = paint(flag_g, 0xffffff, 0.0, None);
        let flag_mat = self.mat(
            Material::standard()
                .set("vertexColors", true)
                .set("side", DOUBLE_SIDE)
                .set("roughness", 0.8),
        );
        let g = self.geo(flag_g);
        self.add_instanced(g, flag_mat, &[flags], false, true);
        // Distance boards before the finish, on the right.
        for (i, d) in [1609.0, 805.0, 402.0].into_iter().enumerate() {
            let s = t.finish_s - d;
            if s > s0 {
                let rect = self.signs.sg.mile[i];
                self.road_sign(
                    s,
                    1.0,
                    rect,
                    2.4,
                    1.2,
                    SignOpts {
                        posts: 2,
                        y0: 1.0,
                        lat: Some(t.frame(s).wall_r + 3.0),
                        ..SignOpts::default()
                    },
                );
            }
        }
        let lake = self.signs.sg.lake;
        let sl = self.z[2].s0 + 120.0;
        self.road_sign(
            sl,
            1.0,
            lake,
            4.2,
            1.4,
            SignOpts {
                posts: 2,
                y0: 1.2,
                lat: Some(t.frame(sl).wall_r + 4.0),
                ..SignOpts::default()
            },
        );
        // End of the course: hay bales and a tyre wall across the lake bed.
        let fe = t.frame(t.road_end() - 1.0);
        let yaw = yaw_z(-fe.fx, -fe.fz);
        let mut lat = -fe.wall_l - 3.0;
        while lat <= fe.wall_r + 3.0 {
            let x = fe.x + fe.rx * lat + fe.fx * 1.5;
            let z = fe.z + fe.rz * lat + fe.fz * 1.5;
            let b = &mut self.cb;
            b.set_frame(x, fe.y - 0.05, z, yaw);
            b.box_yaw("hay", 1.2, 0.9, 0.9, 0.0, 0.0, 0.0, 0.0);
            if js::round(lat / 1.3) % 2.0 == 0.0 {
                b.box_yaw("hay", 1.2, 0.9, 0.9, 0.0, 0.9, 0.05, 0.0);
            }
            b.box_yaw("tire", 1.1, 1.1, 0.8, 0.0, 0.0, 1.3, 0.0);
            lat += 1.3;
        }
        let ep = (fe.x + fe.fx * 3.0, fe.z + fe.fz * 3.0);
        let end = self.signs.sg.end;
        let b = &mut self.cb;
        b.set_frame(ep.0, fe.y, ep.1, yaw);
        for px in [-2.2, 2.2] {
            b.box_yaw("steel", 0.12, 3.2, 0.12, px, 0.0, -0.05, 0.0);
        }
        b.put_at("signs", &sign_geometry(&end, 5.6, 1.9), 0.0, 2.9, 0.0);
        b.put(
            "signs",
            &sign_geometry(&end, 5.6, 1.9),
            0.0,
            2.9,
            -0.04,
            [0.0, PI, 0.0],
            [1.0; 3],
        );
    }

    // ── Silver Lake: cracked-mud and salt-crust decals ──────────────────
    // The terrain's lake bed is a flat colour at racing speed; overlapping
    // translucent tiles of curled mud plates and white salt near the road
    // give it texture where the camera actually looks.
    fn build_playa(&mut self) {
        let t = self.t;
        let mut rng = Mulberry32::new(8080);
        // 500 m bands along the road, for culling
        let mut tiles = self.chunks();
        let s0 = self.z[2].s0 + 160.0;
        let s1 = js::min(t.length - 5.0, t.road_end() + 60.0);
        let mut s = s0;
        while s < s1 {
            let sf = t.frame(s);
            for side in [-1.0, 1.0] {
                for _ in 0..2 {
                    let wall = if side > 0.0 { sf.wall_r } else { sf.wall_l };
                    let lat = side * (wall + 3.0 + kernel::pow(rng.next_f64(), 1.5) * 110.0);
                    let x = sf.x + sf.rx * lat;
                    let z = sf.z + sf.rz * lat;
                    let size = lerp(9.0, 22.0, rng.next_f64()) * (1.0 + (lat.abs() - wall) / 120.0);
                    let salt = rng.next_f64() < 0.25;
                    let ch = self.chunk_of(s);
                    let y = self.gy(x, z) + 0.03;
                    let sz = size * lerp(0.7, 1.2, rng.next_f64());
                    let ry = rng.next_f64() * TURN;
                    let col = if salt {
                        0xffffff
                    } else {
                        *rpick(&mut rng, &[0xf0e4d4, 0xe4d4c0, 0xd8c8b4])
                    };
                    let b = if salt {
                        1.15
                    } else {
                        lerp(0.85, 1.0, rng.next_f64())
                    };
                    tiles[ch].push(Item::scaled(x, y, z, size, 1.0, sz, ry).tint(col, b));
                }
            }
            s += 7.0;
        }
        let mut g = plane_geometry(1.0, 1.0, 1.0, 1.0);
        g.rotate_x(-PI / 2.0);
        let tex = own_texture(self.graph, crack_decal_texture(5));
        let m = self.mat(
            Material::standard()
                .set("map", tex)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("roughness", 1.0)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -2.0)
                .set("polygonOffsetUnits", -2.0),
        );
        let g = self.geo(g);
        self.add_instanced(g, m, &tiles, false, true);
    }

    // ── Spectators' camp along the last mile ────────────────────────────
    // Pickups and motorhomes pulled up in a loose line either side, tents,
    // camp chairs round fire rings, and people drifting toward the course.
    fn build_camp(&mut self) {
        let t = self.t;
        let mut rng = Mulberry32::new(909);
        self.fires = Vec::new();
        self.lanterns = Vec::new();
        const KINDS: [&str; 7] = [
            "pickup", "van", "pickup", "sedan", "boxtruck", "pickup", "hatch",
        ];
        const COLS: [u32; 9] = [
            0xc9ccd1, 0x8a1c1c, 0x2e6d8e, 0xe8e6df, 0x4d5a3a, 0x9aa3ad, 0xb5a27a, 0x2b2f36,
            0xd8a03a,
        ];
        const TENT: [u32; 6] = [0xd84a2a, 0x2e7ac8, 0xe8b830, 0x3a8a4a, 0x8a4ab8, 0xe86a2a];
        const SHIRT: [u32; 8] = [
            0xe8e4dc, 0x2a2a2e, 0xb8322a, 0x2e5a8a, 0xd8a03a, 0x4a7a4a, 0x8a8a8e, 0xd86a8a,
        ];
        let mut tents = Vec::new();
        let mut sets = Vec::new();
        let mut people = Vec::new();
        let mut rv_body = Vec::new();
        let mut rv_glass = Vec::new();
        let rv_kinds: Vec<props::Motorhome> = (0..3)
            .map(|k| motorhome(&mut Mulberry32::new(70 + k)))
            .collect();
        let person =
            |b: &Self, rng: &mut Mulberry32, people: &mut Vec<Item>, x: f64, z: f64, face: f64| {
                let y = b.gy(x, z) - 0.02;
                let sx = lerp(0.92, 1.08, rng.next_f64());
                let ry = face + (rng.next_f64() - 0.5) * 1.2;
                let col = *rpick(rng, &SHIRT);
                let bb = lerp(0.8, 1.05, rng.next_f64());
                people.push(Item::at(x, y, z, sx, ry).tint(col, bb));
            };
        let mut s = t.finish_s - 1300.0;
        while s < t.finish_s + 120.0 {
            for side in [-1.0, 1.0] {
                if rng.next_f64() < 0.25 {
                    continue;
                }
                let sf = t.frame(s);
                let wall = if side > 0.0 { sf.wall_r } else { sf.wall_l };
                let lat = side * (wall + rrange(&mut rng, 16.0, 46.0));
                let x = sf.x + sf.rx * lat;
                let z = sf.z + sf.rz * lat;
                let y = sf.y - 0.3;
                if !self.free(x, z, 6.0) {
                    continue;
                }
                self.take(x, z, 6.0);
                let face = kernel::atan2(sf.fx, sf.fz)
                    + if rng.next_f64() < 0.5 { 0.0 } else { PI }
                    + (rng.next_f64() - 0.5) * 0.6;
                if rng.next_f64() < 0.4 {
                    // A motorhome, awning side toward the course.
                    let rv = &rv_kinds[(rng.next_f64() * 3.0).floor() as usize];
                    let yaw = kernel::atan2(sf.fx, sf.fz)
                        + if side > 0.0 { PI } else { 0.0 }
                        + (rng.next_f64() - 0.5) * 0.3;
                    let m4 = Matrix4::compose(
                        Vector3::new(x, self.gy(x, z) - 0.05, z),
                        Quaternion::from_euler(&Euler::new(0.0, yaw, 0.0)),
                        Vector3::splat(1.0),
                    );
                    let mut body = rv.body.clone();
                    body.apply_matrix4(&m4);
                    rv_body.push(body);
                    let mut glass = rv.glass.clone();
                    glass.apply_matrix4(&m4);
                    rv_glass.push(glass);
                    // Lantern under the awning.
                    let aw = Vector3::new(2.6, 2.3, -1.6).apply_matrix4(&m4);
                    self.lanterns.push(Glow {
                        x: aw.x,
                        y: aw.y,
                        z: aw.z,
                        ph: Some(rng.next_f64() * 10.0),
                    });
                } else {
                    let kind = *rpick(&mut rng, &KINDS);
                    let color = *rpick(&mut rng, &COLS);
                    let seed = (rng.next_f64() * 1000.0).floor() as u32;
                    self.cars.push(CarSpot {
                        kind,
                        color,
                        seed,
                        x,
                        y,
                        z,
                        yaw: face,
                        rust: false,
                        pitch: 0.0,
                        roll: 0.0,
                    });
                }
                // Canopy or dome tent, camp set and a fire on the course side.
                let cx = x - sf.rx * side * 7.0;
                let cz = z - sf.rz * side * 7.0;
                let r = rng.next_f64();
                if r < 0.4 {
                    let b = &mut self.cb;
                    b.set_frame(cx + sf.fx * 3.0, y, cz + sf.fz * 3.0, face);
                    for [px, pz] in [[-1.4, -1.4], [1.4, -1.4], [-1.4, 1.4], [1.4, 1.4]] {
                        b.box_yaw("steel", 0.05, 2.3, 0.05, px, 0.0, pz, 0.0);
                    }
                    let k1 = *rpick(&mut rng, &["canvasW", "canvasR", "canvasB"]);
                    b.box_yaw(k1, 3.1, 0.12, 3.1, 0.0, 2.3, 0.0, 0.0);
                    let pk = cone(2.2, 0.6, 4.0, 1.0, false);
                    let k2 = *rpick(&mut rng, &["canvasW", "canvasR", "canvasB"]);
                    b.put(k2, &pk, 0.0, 2.7, 0.0, [0.0, PI / 4.0, 0.0], [1.0; 3]);
                    b.box_yaw("wood", 1.6, 0.06, 0.7, 0.0, 0.72, 0.0, 0.0);
                } else if r < 0.85 {
                    let n = 1 + (rng.next_f64() * 2.0).floor() as usize;
                    for k in 0..n {
                        let kf = k as f64;
                        let sgn = if rng.next_f64() < 0.5 { 1.0 } else { -1.0 };
                        let tx = x
                            + sf.fx * (5.0 + kf * 3.2) * sgn
                            + sf.rx * side * rrange(&mut rng, -2.0, 3.0);
                        let tz = z
                            + sf.fz * (5.0 + kf * 3.2)
                            + sf.rz * side * rrange(&mut rng, -2.0, 3.0);
                        if !self.clear_of_road(tx, tz, 2.0) {
                            continue;
                        }
                        let ty = self.gy(tx, tz) - 0.02;
                        let sx = lerp(0.9, 1.3, rng.next_f64());
                        let ry = rng.next_f64() * TURN;
                        let mut it = Item::at(tx, ty, tz, sx, ry);
                        it.col = Some(*rpick(&mut rng, &TENT));
                        tents.push(it);
                    }
                }
                if rng.next_f64() < 0.75 {
                    let fx = cx - sf.fx * 2.0;
                    let fz = cz - sf.fz * 2.0;
                    let fy = self.gy(fx, fz) - 0.02;
                    sets.push(Item::at(fx, fy, fz, 1.0, rng.next_f64() * TURN));
                    self.fires.push(Glow {
                        x: fx,
                        y: y + 0.55,
                        z: fz,
                        ph: Some(rng.next_f64() * 10.0),
                    });
                    let n = (rng.next_f64() * 3.0).floor() as usize;
                    for _ in 0..n {
                        let a = rng.next_f64() * TURN;
                        person(
                            self,
                            &mut rng,
                            &mut people,
                            fx + kernel::cos(a) * 2.8,
                            fz + kernel::sin(a) * 2.8,
                            kernel::atan2(-kernel::cos(a), -kernel::sin(a)),
                        );
                    }
                }
            }
            s += rrange(&mut rng, 18.0, 34.0);
        }
        // Spectators along the barriers in the last few hundred metres.
        let to_road = |sf: &Frame, side: f64| kernel::atan2(-sf.rx * side, -sf.rz * side);
        let mut barrier = Vec::new();
        let mut s = t.finish_s - 320.0;
        while s < t.finish_s + 60.0 {
            let sf = t.frame(s);
            for side in [-1.0, 1.0] {
                let wl = (if side > 0.0 { sf.wall_r } else { sf.wall_l }) + 5.0;
                let bx = sf.x + sf.rx * side * wl;
                let bz = sf.z + sf.rz * side * wl;
                barrier.push(Item::at(
                    bx,
                    self.gy(bx, bz),
                    bz,
                    1.0,
                    kernel::atan2(sf.fx, sf.fz),
                ));
                if rng.next_f64() < 0.55 {
                    let d = wl + rrange(&mut rng, 0.8, 4.5);
                    let px = sf.x + sf.rx * side * d + sf.fx * rng.next_f64() * 2.0;
                    let pz = sf.z + sf.rz * side * d + sf.fz * rng.next_f64() * 2.0;
                    person(self, &mut rng, &mut people, px, pz, to_road(&sf, side));
                }
            }
            s += 3.0;
        }
        // Crowd-control barrier: a steel section per 3 m.
        let mut bb = Builder::new_color(&[("m", 0xb8bcc0)]);
        bb.box_yaw("m", 0.06, 1.1, 0.06, 0.0, 0.0, -1.45, 0.0);
        bb.box_yaw("m", 0.06, 1.1, 0.06, 0.0, 0.0, 1.45, 0.0);
        bb.box_yaw("m", 0.06, 0.06, 2.95, 0.0, 1.05, 0.0, 0.0);
        bb.box_yaw("m", 0.06, 0.06, 2.95, 0.0, 0.2, 0.0, 0.0);
        for k in -6..=6 {
            bb.box_yaw("m", 0.025, 0.85, 0.025, 0.0, 0.2, f64::from(k) * 0.22, 0.0);
        }
        for [px, pz] in [[0.35, -1.45], [-0.35, -1.45], [0.35, 1.45], [-0.35, 1.45]] {
            bb.box_yaw("m", 0.05, 0.04, 0.05, px, 0.0, pz, 0.0);
        }
        let all: Vec<&BufferGeometry> = bb.buckets.iter().flat_map(|(_, g)| g.iter()).collect();
        let barrier_geo = merge_geometries(&all, false).expect("the barrier's parts merge");
        let solid = self.m.solid;
        let g = self.geo(paint(barrier_geo, 0xb8bcc0, 0.0, None));
        self.add_instanced(g, solid, &[barrier], false, true);
        let bush = self.m.bush;
        let g = self.geo(dome_tent_geometry(4));
        self.add_instanced(g, bush, &[tents], true, true);
        let g = self.geo(camp_set_geometry(9));
        self.add_instanced(g, solid, &[sets], false, true);
        self.people = people;
        self.shirts = Some(SHIRT.to_vec());
        for (list, mat) in [(rv_body, self.m.solid), (rv_glass, self.m.rv_glass)] {
            if list.is_empty() {
                continue;
            }
            let refs: Vec<&BufferGeometry> = list.iter().collect();
            let mut g = merge_geometries(&refs, false).expect("the motorhomes merge");
            g.compute_bounding_sphere();
            let geo = self.geo(g);
            let mesh = self.graph.mesh(geo, mat);
            let o = self.graph.get_mut(mesh);
            o.cast_shadow = mat == self.m.solid;
            o.receive_shadow = true;
            o.matrix_auto_update = false;
            self.add(mesh);
        }
        // Fire flames: a few crossed emissive blades, drawn additive.
        if !self.fires.is_empty() {
            let mut fl = Vec::new();
            for k in 0..3 {
                let mut b = cone(0.32, 1.1, 5.0, 1.0, true);
                b.translate(0.0, 0.55, 0.0);
                b.scale(1.0, 1.0, 0.3);
                b.rotate_y((f64::from(k) / 3.0) * PI);
                fl.push(b);
            }
            let refs: Vec<&BufferGeometry> = fl.iter().collect();
            let fg = merge_geometries(&refs, false).expect("the flames merge");
            let fm = self.mat(
                Material::basic()
                    .set("color", 0xff8a2a)
                    .set("transparent", true)
                    .set("opacity", 0.85)
                    .set("blending", ADDITIVE)
                    .set("depthWrite", false)
                    .set("side", DOUBLE_SIDE),
            );
            self.flame_mat = Some(fm);
            let g = self.geo(fg);
            let flames = self.graph.instanced_mesh(g, fm, self.fires.len() as u32);
            {
                let inst = self
                    .graph
                    .get_mut(flames)
                    .instances
                    .as_mut()
                    .expect("instanced");
                for (i, f) in self.fires.iter().enumerate() {
                    inst.set_matrix_at(i, &Matrix4::make_translation(f.x, f.y - 0.45, f.z));
                }
            }
            self.graph.compute_instance_bounding_sphere(flames);
            self.add(flames);
        }
    }

    // ── Finish: truss gantry, timing lights and floodlight towers ───────
    fn build_finish(&mut self) {
        let t = self.t;
        let f = t.frame(t.finish_s);
        let yaw = kernel::atan2(f.fx, f.fz);
        let wl = f.wall_l + 1.2;
        let wr = f.wall_r + 1.2;
        let p_ = |lat: f64, along: f64| {
            (
                f.x + f.rx * lat + f.fx * along,
                f.z + f.rz * lat + f.fz * along,
            )
        };
        let h = 8.5;
        for lat in [-wl, wr] {
            let p = p_(lat, 0.0);
            let b = &mut self.cb;
            b.set_frame(p.0, f.y, p.1, yaw);
            for [a, bb] in [[-0.4, -0.4], [0.4, -0.4], [-0.4, 0.4], [0.4, 0.4]] {
                b.box_yaw("steel", 0.1, h + 1.0, 0.1, a, 0.0, bb, 0.0);
            }
            for k in 0..9 {
                b.box_(
                    "steel",
                    0.8,
                    0.05,
                    0.05,
                    0.0,
                    0.6 + f64::from(k),
                    -0.4,
                    0.0,
                    0.0,
                    0.8,
                );
            }
            b.box_yaw("concrete", 1.4, 0.5, 1.4, 0.0, -0.3, 0.0, 0.0);
        }
        let c = p_((wr - wl) / 2.0, 0.0);
        let span = wl + wr;
        {
            let b = &mut self.cb;
            b.set_frame(c.0, f.y, c.1, yaw);
            for [dy, dz] in [[h, -0.4], [h, 0.4], [h + 0.9, -0.4], [h + 0.9, 0.4]] {
                b.cbox("steel", span, 0.1, 0.1, 0.0, dy, dz, [0.0; 3]);
            }
            let mut k = 0;
            while f64::from(k) <= span / 1.2 {
                b.cbox(
                    "steel",
                    0.05,
                    0.9,
                    0.05,
                    -span / 2.0 + f64::from(k) * 1.2,
                    h + 0.45,
                    -0.4 + f64::from(k % 2) * 0.8,
                    [0.0; 3],
                );
                k += 1;
            }
        }
        let tex = banner_texture(
            "FINISH",
            &BannerOpts {
                w: 1024,
                h: 256,
                checker: true,
                font: "italic 900 150px \"Arial Narrow\", Arial, sans-serif",
                ..BannerOpts::default()
            },
        );
        let tex = own_texture(self.graph, tex);
        let bm = self.mat(
            Material::standard()
                .set("map", tex)
                .set("emissive", 0xffffff)
                .set("emissiveMap", tex)
                .set("emissiveIntensity", 0.3)
                .set("roughness", 0.6)
                .set("side", DOUBLE_SIDE),
        );
        self.graph
            .add_night(Some(bm), "emissiveIntensity", 0.2, 0.9);
        let g = self.geo(plane_geometry(span - 1.2, 2.8, 1.0, 1.0));
        let banner = self.graph.mesh(g, bm);
        {
            let o = self.graph.get_mut(banner);
            o.position = Vector3::new(c.0 - f.fx * 0.5, f.y + h - 0.9, c.1 - f.fz * 0.5);
            o.set_rotation(&Euler::new(0.0, kernel::atan2(-f.fx, -f.fz), 0.0));
        }
        self.add(banner);
        // Timing trailer on the right.
        let tp = p_(wr + 8.0, 6.0);
        let timing = self.signs.sg.timing;
        {
            let b = &mut self.cb;
            b.set_frame(tp.0, f.y - 0.3, tp.1, yaw_z(-f.rx, -f.rz));
            b.box_yaw("white", 6.5, 2.6, 2.4, 0.0, 0.6, 0.0, 0.0);
            b.box_yaw("black", 6.5, 0.6, 2.2, 0.0, 0.0, 0.0, 0.0);
            b.cbox("glassLit", 4.0, 0.9, 0.08, 0.0, 2.2, 1.22, [0.0; 3]);
            b.put_at("signs", &sign_geometry(&timing, 3.2, 0.8), 0.0, 3.6, 1.1);
            b.box_yaw("steel", 3.4, 0.9, 0.1, 0.0, 3.2, 1.05, 0.0);
        }
        // Bleachers facing the line on the left, behind the barrier.
        let bp = p_(-(wl + 13.0), -14.0);
        {
            let b = &mut self.cb;
            b.set_frame(bp.0, f.y - 0.3, bp.1, yaw_z(f.rx, f.rz));
            for r in 0..6 {
                let rf = f64::from(r);
                b.box_yaw(
                    "steel",
                    16.0,
                    0.08,
                    0.7,
                    0.0,
                    0.5 + rf * 0.45,
                    -rf * 0.75,
                    0.0,
                );
                b.box_yaw(
                    "wood",
                    16.0,
                    0.06,
                    0.35,
                    0.0,
                    0.58 + rf * 0.45,
                    -rf * 0.75 + 0.1,
                    0.0,
                );
            }
            for k in -4..=4 {
                let mut r = 0;
                while r < 6 {
                    let rf = f64::from(r);
                    b.box_yaw(
                        "steel",
                        0.08,
                        0.5 + rf * 0.45,
                        0.08,
                        f64::from(k) * 2.0,
                        0.0,
                        -rf * 0.75,
                        0.0,
                    );
                    r += 2;
                }
            }
            b.box_yaw("steel", 16.0, 0.06, 0.06, 0.0, 3.6, -4.1, 0.0);
        }
        // A crowd on the benches, then everyone goes into one instanced mesh.
        let by = yaw_z(f.rx, f.rz);
        let mut rng = Mulberry32::new(4242);
        let mut people = std::mem::take(&mut self.people);
        let shirts = self.shirts.clone().unwrap_or_else(|| vec![0xffffff]);
        for r in 0..6 {
            for k in 0..20 {
                if rng.next_f64() < 0.35 {
                    continue;
                }
                let lx = -7.6 + f64::from(k) * 0.8 + (rng.next_f64() - 0.5) * 0.2;
                let ly = 0.3 + f64::from(r) * 0.45;
                let lz = -f64::from(r) * 0.75 - 0.1;
                let sy = lerp(0.8, 0.95, rng.next_f64());
                let ry = by + (rng.next_f64() - 0.5) * 0.6;
                let col = *rpick(&mut rng, &shirts);
                let b = lerp(0.8, 1.05, rng.next_f64());
                people.push(
                    Item::scaled(
                        bp.0 + lx * kernel::cos(by) + lz * kernel::sin(by),
                        f.y - 0.3 + ly,
                        bp.1 - lx * kernel::sin(by) + lz * kernel::cos(by),
                        1.0,
                        sy,
                        1.0,
                        ry,
                    )
                    .tint(col, b),
                );
            }
        }
        let bush = self.m.bush;
        let g = self.geo(person_geometry(2));
        self.add_instanced(g, bush, &[people], false, true);
        // Floodlight towers either side, before and after the line.
        self.floods = Vec::new();
        for [lat, along] in [
            [-(wl + 16.0), -60.0],
            [wr + 16.0, -60.0],
            [-(wl + 16.0), 60.0],
            [wr + 16.0, 60.0],
        ] {
            let p = p_(lat, along);
            let h2 = 14.0;
            let sgn = js::sign(lat);
            let toward = yaw_z(-f.rx * sgn, -f.rz * sgn);
            {
                let b = &mut self.cb;
                b.set_frame(p.0, f.y - 0.3, p.1, 0.0);
                for [a, bb] in [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]] {
                    b.box_yaw("steel", 0.12, h2, 0.12, a, 0.0, bb, 0.0);
                }
                for k in 1..7 {
                    b.box_yaw("steel", 1.1, 0.06, 1.1, 0.0, f64::from(k) * 2.0, 0.0, 0.0);
                }
                // Lamp bank facing the course.
                b.set_frame(p.0, f.y - 0.3, p.1, toward);
                b.box_yaw("steel", 3.2, 2.0, 0.3, 0.0, h2, 0.0, 0.0);
            }
            for i in 0..3 {
                for j in 0..2 {
                    let (fi, fj) = (f64::from(i), f64::from(j));
                    self.cb.box_yaw(
                        "lamp",
                        0.8,
                        0.7,
                        0.12,
                        -1.0 + fi,
                        h2 + 0.25 + fj * 0.9,
                        0.17,
                        0.0,
                    );
                    let lx = -1.0 + fi;
                    let ly = h2 + 0.6 + fj * 0.9;
                    self.lamps.push(Glow {
                        x: p.0 + lx * kernel::cos(toward) + 0.4 * kernel::sin(toward),
                        y: f.y - 0.3 + ly,
                        z: p.1 - lx * kernel::sin(toward) + 0.4 * kernel::cos(toward),
                        ph: Some(fi + fj * 3.0),
                    });
                }
            }
            // Diesel generator trailer at the tower's foot.
            {
                let b = &mut self.cb;
                b.box_yaw("paintRed", 1.4, 1.3, 2.6, 1.8, 0.0, -1.2, 0.0);
                b.box_yaw("black", 1.45, 0.25, 2.65, 1.8, 1.3, -1.2, 0.0);
                b.box_yaw("steel", 0.12, 1.1, 0.12, 2.2, 1.5, -2.2, 0.0);
            }
            self.floods.push(Glow {
                x: p.0,
                y: f.y + h2 + 1.0,
                z: p.1,
                ph: None,
            });
            // Beam: from the lamp bank down onto the course just past the line.
            let aim = p_(-lat * 0.2, along * 0.25);
            self.beams.push(Beam {
                x: p.0 - f.rx * sgn * 0.5,
                y: f.y + h2 + 1.0,
                z: p.1 - f.rz * sgn * 0.5,
                tx: aim.0,
                ty: f.y,
                tz: aim.1,
            });
            let q = p_(lat * 0.35, along * 0.8);
            self.pools.push(Pool {
                x: q.0,
                y: f.y + 0.03,
                z: q.1,
                r: 26.0,
                c: [0.26, 0.28, 0.32],
                fl: None,
                ph: None,
            });
        }
    }

    // Parked vehicles baked into merged meshes (like Beach.buildCars). Body
    // paint goes into a vertex-coloured bucket, so a car park of different
    // colours costs one draw call instead of one per colour. Wrecks get a
    // rusty bucket, sit tilted and sunk, and some have lost their wheels.
    fn build_cars(&mut self) {
        if self.cars.is_empty() {
            return;
        }
        let mut b = Builder::new();
        // `mats`: `{ paintV, rust, [sig]: material }` in insertion order.
        let mut sigs: Vec<(String, MaterialId)> = Vec::new();
        let mut painted_v: Vec<BufferGeometry> = Vec::new();
        let mut painted_r: Vec<BufferGeometry> = Vec::new();
        let cars = std::mem::take(&mut self.cars);
        for c in &cars {
            let m = build_vehicle(
                self.graph,
                self.textures,
                c.kind,
                &CarOpts {
                    color: Some(c.color),
                    seed: c.seed,
                    lod: Some(Lod::Low),
                    ..CarOpts::default()
                },
            )
            .expect("a known kind");
            {
                let r = self.graph.get_mut(m.root);
                r.position = Vector3::new(c.x, c.y, c.z);
                r.set_rotation(&Euler::with_order(
                    js::or(c.pitch, 0.0),
                    c.yaw,
                    js::or(c.roll, 0.0),
                    EulerOrder::YXZ,
                ));
            }
            let bare = c.rust && c.seed % 3 == 0;
            for (id, world, parent_name) in anim::world_matrices(self.graph, m.root) {
                let o = self.graph.get(id);
                if !o.ty.is_mesh() {
                    continue;
                }
                if bare && parent_name.as_deref() == Some("wheel") {
                    continue;
                }
                let mt = o.materials[0];
                let name = o.name.clone();
                let geo = o.geometry.expect("a mesh");
                if name == "paint" || (c.rust && name == "stripe") {
                    let src = self.graph.geometry(geo);
                    let mut g = if src.index.is_some() {
                        src.to_non_indexed()
                    } else {
                        src.clone()
                    };
                    let names: Vec<String> = g.attributes.iter().map(|(n, _)| n.clone()).collect();
                    for k in names {
                        if k != "position" && k != "normal" {
                            g.delete_attribute(&k);
                        }
                    }
                    g.apply_matrix4(&world);
                    let n = g.position().count();
                    let col = Color::hex(if c.rust {
                        c.color
                    } else {
                        self.graph
                            .material(mt)
                            .color("color")
                            .expect("a colour")
                            .get_hex()
                    });
                    let mut a = vec![0f32; n * 3];
                    for i in 0..n {
                        // Rust: blotchy, darker toward the sills.
                        let k = if c.rust {
                            0.75 + 0.35 * hash2((i / 3) as f64, f64::from(c.seed), 0.0)
                        } else {
                            1.0
                        };
                        a[i * 3] = (col.r * k) as f32;
                        a[i * 3 + 1] = (col.g * k) as f32;
                        a[i * 3 + 2] = (col.b * k) as f32;
                    }
                    g.set_attribute("color", BufferAttribute::from_f32(a, 3));
                    if c.rust {
                        painted_r.push(g);
                    } else {
                        painted_v.push(g);
                    }
                    continue;
                }
                if c.rust && (name == "head" || name == "tail" || name == "rev") {
                    continue;
                }
                let sig = anim::material_sig(self.graph, mt);
                if !sigs.iter().any(|(k, _)| *k == sig) {
                    sigs.push((sig.clone(), mt));
                }
                b.set_frame(0.0, 0.0, 0.0, 0.0);
                let src = self.graph.geometry(geo).clone();
                b.add(&sig, &src, Some(&world));
            }
        }
        let mut mats: Vec<(&str, MaterialId)> =
            vec![("paintV", self.m.paint_v), ("rust", self.m.rust)];
        mats.extend(sigs.iter().map(|(k, m)| (k.as_str(), *m)));
        let meshes = b.build(
            self.graph,
            &mats,
            &BuildOpts {
                cast_shadow: CastShadow::All,
                receive_shadow: true,
            },
        );
        for mesh in meshes {
            self.graph.get_mut(mesh).name = "desert:cars".into();
            self.add(mesh);
        }
        for (key, list, mat) in [
            ("paintV", painted_v, self.m.paint_v),
            ("rust", painted_r, self.m.rust),
        ] {
            if list.is_empty() {
                continue;
            }
            let refs: Vec<&BufferGeometry> = list.iter().collect();
            let mut g = merge_geometries(&refs, false).expect("the paint merges");
            g.compute_bounding_sphere();
            let geo = self.geo(g);
            let mesh = self.graph.mesh(geo, mat);
            let o = self.graph.get_mut(mesh);
            o.name = format!("desert:cars-{key}");
            o.cast_shadow = true;
            o.receive_shadow = true;
            o.matrix_auto_update = false;
            self.add(mesh);
        }
    }

    // ── Night lights: pools, flares, fires, floodlight halos ───────────
    fn build_glows(&mut self) {
        // Ground pools: flat additive quads; fires and flares flicker.
        let mut pools = self.pools.clone();
        for f in &self.flares {
            pools.push(Pool {
                x: f.x,
                y: f.y - 0.08,
                z: f.z,
                r: 1.7,
                c: [0.55, 0.07, 0.03],
                fl: Some(0.6),
                ph: f.ph,
            });
        }
        for f in &self.fires {
            pools.push(Pool {
                x: f.x,
                y: f.y - 0.5,
                z: f.z,
                r: 6.5,
                c: [0.7, 0.32, 0.1],
                fl: Some(0.45),
                ph: f.ph,
            });
        }
        for l in &self.lanterns {
            pools.push(Pool {
                x: l.x,
                y: l.y - 2.2,
                z: l.z,
                r: 4.0,
                c: [0.4, 0.3, 0.16],
                fl: Some(0.08),
                ph: l.ph,
            });
        }
        if !pools.is_empty() {
            let (mesh, material) = flicker_pools(self.graph, self.textures, &pools, 0.0);
            self.add(mesh);
            self.pool_mat = Some(material);
            self.glow_mats.push(material);
        }
        // Sprite points: flares (red, flickering), fires (orange), floods
        // (white), fence reflectors (amber, only in headlights: approximated
        // by night).
        let add = |b: &mut Self, list: &[Glow], o: PointOpts| {
            if list.is_empty() {
                return None;
            }
            let (pts, m) = flicker_points(b.graph, b.textures, list, &o, 0.0);
            b.add(pts);
            b.glow_mats.push(m);
            Some(m)
        };
        let d = PointOpts::default();
        let flares = self.flares.clone();
        self.flare_mat = add(
            self,
            &flares,
            PointOpts {
                size: 1.1,
                color: Color::new(3.0, 0.35, 0.15),
                rate: 1.6,
                depth: 0.55,
                ..d
            },
        );
        let fires = self.fires.clone();
        self.fire_mat = add(
            self,
            &fires,
            PointOpts {
                size: 2.4,
                color: Color::new(2.6, 1.2, 0.35),
                rate: 1.0,
                depth: 0.4,
                ..d
            },
        );
        let lanterns = self.lanterns.clone();
        self.lantern_mat = add(
            self,
            &lanterns,
            PointOpts {
                size: 0.7,
                color: Color::new(2.4, 1.7, 0.9),
                depth: 0.05,
                ..d
            },
        );
        let floods = self.floods.clone();
        self.flood_mat = add(
            self,
            &floods,
            PointOpts {
                size: 7.0,
                color: Color::new(2.2, 2.3, 2.5),
                depth: 0.0,
                ..d
            },
        );
        let lamps = self.lamps.clone();
        self.lamp_mat = add(
            self,
            &lamps,
            PointOpts {
                size: 1.6,
                color: Color::new(2.2, 2.3, 2.5),
                depth: 0.0,
                ..d
            },
        );
        let refl = self.reflectors.clone();
        self.refl_mat = add(
            self,
            &refl,
            PointOpts {
                size: 0.5,
                color: Color::new(2.5, 1.4, 0.3),
                depth: 0.0,
                ..d
            },
        );
        let bulbs = self.bulbs.clone();
        self.bulb_mat = add(
            self,
            &bulbs,
            PointOpts {
                size: 0.35,
                color: Color::new(2.6, 1.9, 1.0),
                blink: 2.2,
                ..d
            },
        );
        let strings = self.strings.clone();
        self.string_mat = add(
            self,
            &strings,
            PointOpts {
                size: 0.3,
                color: Color::new(2.4, 1.6, 0.8),
                depth: 0.08,
                rate: 0.3,
                ..d
            },
        );
        let lp = self.lamp_pools.clone();
        self.bb_lamp_mat = add(
            self,
            &lp,
            PointOpts {
                size: 1.2,
                color: Color::new(2.4, 2.1, 1.6),
                depth: 0.0,
                ..d
            },
        );
        // Flare cores: small emissive sticks so they read by day too.
        if !self.flares.is_empty() {
            let mut g = paint(
                prep(cyl(0.03, 0.03, 0.25, 5.0, 1.0, false)),
                0xffffff,
                0.0,
                None,
            );
            g.rotate_z(PI / 2.0);
            let m = self.mat(
                Material::standard()
                    .set("color", 0xff3010)
                    .set("emissive", 0xff3010)
                    .set("emissiveIntensity", 2.0),
            );
            let items: Vec<Item> = self
                .flares
                .iter()
                .map(|p| Item::at(p.x, p.y - 0.08, p.z, 1.0, p.ph.unwrap_or(f64::NAN)))
                .collect();
            let g = self.geo(g);
            self.add_instanced(g, m, &[items], false, true);
        }
        // Floodlight beams: open additive cones, visible only after dark.
        if !self.beams.is_empty() {
            let geo = beam_geometry(1.0, 1.0);
            // Front faces only: from inside the beam it vanishes rather than
            // washing out the screen. It fades where the surface turns
            // edge-on, so it reads as a soft shaft of lit dust, not a solid
            // lampshade.
            let mat = self.mat(
                Material::basic()
                    .set("vertexColors", true)
                    .set("transparent", true)
                    .set("opacity", 0.0)
                    .set("blending", ADDITIVE)
                    .set("depthWrite", false)
                    .set("fog", true)
                    .kind(mr_scene::MaterialKind::FloodBeam, None)
                    .program_key("desert-beam")
                    .uniform("clippingPlanes", Value::Null),
            );
            let g = self.geo(geo);
            let im = self.graph.instanced_mesh(g, mat, self.beams.len() as u32);
            let dn = Vector3::new(0.0, -1.0, 0.0);
            {
                let inst = self
                    .graph
                    .get_mut(im)
                    .instances
                    .as_mut()
                    .expect("instanced");
                for (i, b) in self.beams.iter().enumerate() {
                    let d = Vector3::new(b.tx - b.x, b.ty - b.y, b.tz - b.z);
                    let len = d.length() * 1.05;
                    let q = Quaternion::from_unit_vectors(dn, d.normalize());
                    let m4 = Matrix4::compose(
                        Vector3::new(b.x, b.y, b.z),
                        q,
                        Vector3::new(len * 0.3, len, len * 0.3),
                    );
                    inst.set_matrix_at(i, &m4);
                }
            }
            self.graph.compute_instance_bounding_sphere(im);
            self.graph.get_mut(im).render_order = 3.0;
            self.add(im);
            self.beam_mat = Some(mat);
        }
    }
}
