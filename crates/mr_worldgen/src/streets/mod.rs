//! `src/world/Streets.js` (roadmap WP 7.2): Downtown Streets (Level 3), the
//! city grid the race runs through.
//!
//! The level supplies the grid (crossing spacing, kerb and pavement widths,
//! the ground function) and the route's straight legs. Every block is a
//! kerbed pavement slab with buildings round its edge; blocks on the inside
//! of the route's corners have their kerbs pushed out to follow the curve,
//! so the kerb is always where the car's wall is. Where the wall crosses open
//! road instead — side streets, the outside of corners, the ends — the race
//! has closed the street with barriers.
//!
//! Districts by grid column: the Neon District (shop fronts, signs, lantern
//! strings, an elevated railway), Nob Hill (painted rowhouses stepped up the
//! ridge) and the Financial District (towers round the finish).
//!
//! The port keeps the JS structure: [`Streets::plan`] blanks the lane paint
//! through crossings and corners and sets the runout; [`Streets::build`]
//! runs the JS methods in the JS order on a [`Bld`] (the JS `this` while it
//! builds) into the group `streets`: this file holds the route analysis, the
//! materials, the blocks, the streets and the pavements; [`buildings`] the
//! blocks' buildings; [`dressing`] the lamps, signals, barriers, gantries,
//! crowds, lanterns, the elevated railway, the vents and the wet-road
//! reflections; [`props`] is `streets/props.js`; the canvas pictures are
//! [`facades`] (`streets/facades.js`) and [`textures`]
//! (`streets/textures.js`); [`anim`] the closures the build pushes onto
//! `world.updaters`. The grid itself is `mr_levels::streets`, as the JS
//! reads `level.grid`.
//!
//! The material patches (`StreetFacade`, `StreetAtlas`, `AmbientProp`,
//! `Neon`) are tagged kinds whose GLSL is the renderer's; `Steam` is a
//! `ShaderMaterial` with its own GLSL.

// Index loops stay index loops, and the JS signatures stay (D52, D130).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]
// The JS's forms stay: `tx * -1`, counters kept beside the loop index.
#![allow(clippy::neg_multiply, clippy::explicit_counter_loop)]

pub mod anim;
pub mod buildings;
pub mod dressing;
pub mod facades;
pub mod props;
pub mod textures;

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::PI;

use mr_levels::streets::{self as grid, Leg};
use mr_math::{Mulberry32, js, kernel, lerp};
use mr_scene::MaterialKind;
use mr_track::Track;
use serde_json::{Value, json};

use crate::color::Color;
use crate::geom::{GeoBuilder, P2, P3, StaticOpts, static_mesh};
use crate::material::{Material, num};
use crate::object::{Layer, MaterialId, NodeId, SceneGraph};
use crate::road::MarkGap;
use crate::textures::TextureCache;
use crate::world::{Animator, Scenery, SceneryInfo, World};

/// Neon colours for LED bands (HDR).
pub const NEON: [P3; 7] = [
    [3.2, 0.25, 2.6],
    [0.3, 2.6, 3.2],
    [3.4, 0.5, 1.0],
    [3.2, 1.6, 0.3],
    [0.5, 3.2, 1.2],
    [1.8, 0.5, 3.4],
    [3.0, 3.0, 3.2],
];
/// Painted-lady body colours for the rowhouses (tints on a light atlas cell).
pub const PAINTED: [P3; 11] = [
    [0.55, 0.74, 0.8],
    [0.95, 0.78, 0.48],
    [0.86, 0.55, 0.58],
    [0.62, 0.74, 0.52],
    [0.97, 0.93, 0.82],
    [0.62, 0.6, 0.8],
    [0.9, 0.62, 0.45],
    [0.45, 0.56, 0.7],
    [0.98, 0.84, 0.88],
    [0.72, 0.86, 0.78],
    [0.96, 0.9, 0.62],
];
pub const AWNING: [P3; 5] = [
    [0.55, 0.08, 0.08],
    [0.08, 0.32, 0.18],
    [0.1, 0.14, 0.34],
    [0.36, 0.1, 0.24],
    [0.5, 0.3, 0.06],
];

/// The grid as `level.grid` hands it to the scenery.
#[derive(Clone, Debug)]
pub struct Grid {
    pub px: f64,
    pub pz: f64,
    pub hw: f64,
    pub walk: f64,
    pub setback: f64,
    pub legs: Vec<Leg>,
}

impl Grid {
    pub fn new() -> Grid {
        let route = grid::build_route();
        Grid {
            px: grid::PX,
            pz: grid::PZ,
            hw: grid::HW,
            walk: grid::WALK,
            setback: route.setback,
            legs: route.legs,
        }
    }
}

impl Default for Grid {
    fn default() -> Self {
        Grid::new()
    }
}

/// `G.ground(x, z)`.
pub fn ground(x: f64, z: f64) -> f64 {
    grid::ground(x, z)
}

/// `t.px[s]` (and `pz`): a Float32Array read; a non-index reads `undefined`
/// (NaN in arithmetic).
fn arr_at(a: &[f32], s: f64) -> f64 {
    if s >= 0.0 && s.fract() == 0.0 && (s as usize) < a.len() {
        f64::from(a[s as usize])
    } else {
        f64::NAN
    }
}

/// Builders keyed by spatial chunk so far-off parts can be frustum culled.
/// The JS `Map` keyed by `"cx,cz"`: builders in the order first asked for.
pub struct Chunks {
    pub size: f64,
    color: bool,
    cell: bool,
    index: BTreeMap<(i64, i64), usize>,
    pub builders: Vec<GeoBuilder>,
}

impl Chunks {
    pub fn new(size: f64, color: bool, cell: bool) -> Chunks {
        Chunks {
            size,
            color,
            cell,
            index: BTreeMap::new(),
            builders: Vec::new(),
        }
    }

    /// `at(x, z)`.
    pub fn at(&mut self, x: f64, z: f64) -> &mut GeoBuilder {
        // `Math.floor(x / size)` written into a string: -0 reads "0".
        let key = (
            (x / self.size).floor() as i64,
            (z / self.size).floor() as i64,
        );
        let k = match self.index.get(&key) {
            Some(&k) => k,
            None => {
                self.builders.push(GeoBuilder::new(self.color, self.cell));
                self.index.insert(key, self.builders.len() - 1);
                self.builders.len() - 1
            }
        };
        &mut self.builders[k]
    }

    /// `emit(group, mat, opts)`.
    pub fn emit(
        &self,
        graph: &mut SceneGraph,
        group: NodeId,
        mat: MaterialId,
        o: &StaticOpts,
    ) -> Vec<NodeId> {
        let mut out = Vec::new();
        for b in &self.builders {
            if b.is_empty() {
                continue;
            }
            let geo = graph.add_geometry(b.build());
            let m = static_mesh(graph, geo, mat, o);
            graph.add(group, m);
            out.push(m);
        }
        out
    }
}

/// `pointInPoly(x, z, poly)`.
pub fn point_in_poly(x: f64, z: f64, poly: &[P2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let [xi, zi] = poly[i];
        let [xj, zj] = poly[j];
        if (zi > z) != (zj > z) && x < ((xj - xi) * (z - zi)) / (zj - zi + 1e-12) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// A block of the grid (`this.blocks`' values).
#[derive(Clone, Debug)]
pub struct Block {
    pub i: i64,
    pub j: i64,
    /// 0 by the route, 1 near, 2 far.
    pub tier: i32,
    pub d: f64,
    pub district: u32,
    pub cx: f64,
    pub cz: f64,
    pub poly: Vec<P2>,
}

/// A light the wet road reflects (`this.signLights`).
#[derive(Clone, Debug)]
pub struct SignLight {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub col: &'static str,
    pub lamp: bool,
    pub big: bool,
}

/// A street tree (`this.trees`).
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub s: f64,
}

/// Light spilt on the pavement (`this.spill`).
#[derive(Clone, Copy, Debug)]
pub struct Spill {
    pub x: f64,
    pub z: f64,
    pub r: f64,
    pub col: P3,
    pub tx: f64,
    pub tz: f64,
    pub long: bool,
}

/// A street front (`this.fronts`): `{ fa, tx, tz, nx, nz, L, y, district,
/// tier, shop, lit }`. `shop` is `None` for a rowhouse (`undefined`).
#[derive(Clone, Copy, Debug)]
pub struct Front {
    pub fa: P2,
    pub tx: f64,
    pub tz: f64,
    pub nx: f64,
    pub nz: f64,
    pub l: f64,
    pub y: f64,
    pub district: u32,
    pub tier: i32,
    pub shop: Option<usize>,
    pub lit: Option<P3>,
}

/// A rooftop billboard (`this.billboards`).
#[derive(Clone, Copy, Debug)]
pub struct Billboard {
    pub ox: f64,
    pub oz: f64,
    pub y0: f64,
    pub tx: f64,
    pub tz: f64,
    pub w: f64,
    pub h: f64,
    pub ad: usize,
}

/// The materials `materials()` makes.
pub struct Mats {
    pub fac: MaterialId,
    pub street: MaterialId,
    pub pave: MaterialId,
    pub asphalt: MaterialId,
    pub paint: MaterialId,
    pub plain: MaterialId,
    pub glow: MaterialId,
    pub neon_h: MaterialId,
    pub neon_v: MaterialId,
    pub car: MaterialId,
}

/// `ambientPatch(mat, rgb, key)`: a faint emissive proportional to albedo,
/// the city's bounce light.
pub fn ambient_patch(m: Material, rgb: P3, key: &str) -> Material {
    m.kind(MaterialKind::AmbientProp, Some(json!({ "rgb": rgb })))
        .program_key(&format!("streets-amb-{key}"))
        .uniform("clippingPlanes", Value::Null)
}

/// `neonFlicker(mat, time, key)`: per-sign data (seed, mode) in `ndata`.
/// The uniform `uNTime` is the shared clock (0 when made).
pub fn neon_flicker(m: Material, key: &str) -> Material {
    m.kind(MaterialKind::Neon, None)
        .program_key(&format!("streets-neon-{key}"))
        .uniform("uNTime", num(0.0))
        .uniform("clippingPlanes", Value::Null)
}

/// `facadeMaterial()` (`streets/facades.js`).
pub fn facade_material(graph: &mut SceneGraph, cache: &mut TextureCache) -> MaterialId {
    let a = facades::facade_atlas(cache);
    let map = graph.cached_texture(&a, Layer::Main, "");
    let em = graph.cached_texture(&a, Layer::Emissive, "");
    graph.add_material(
        Material::standard()
            .set("map", map)
            .set("emissiveMap", em)
            .set("emissive", 0xffffff)
            .set("emissiveIntensity", 1.1)
            .set("roughness", 0.62)
            .set("metalness", 0.2)
            .kind(MaterialKind::StreetFacade, None)
            .program_key("streets-facade")
            .uniform("clippingPlanes", Value::Null),
    )
}

/// `patchStreetAtlas(material)` (`streets/textures.js`).
pub fn patch_street_atlas(m: Material) -> Material {
    m.kind(MaterialKind::StreetAtlas, None)
        .program_key("street-atlas-2")
        .uniform("clippingPlanes", Value::Null)
}

/// The Downtown Streets scenery module.
pub struct Streets {
    pub label: &'static str,
    pub zone: usize,
    pub g: Grid,
    /// The group `streets`, once built.
    pub group: Option<NodeId>,
}

impl Streets {
    /// `new Streets({ level })`.
    pub fn new(info: &SceneryInfo) -> Streets {
        Streets {
            label: "Building downtown",
            zone: info.zone,
            g: Grid::new(),
            group: None,
        }
    }
}

impl Scenery for Streets {
    fn name(&self) -> &str {
        "Streets"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    // ── Plan: road ends, and no lane paint through crossings ──────
    fn plan(&mut self, world: &mut World) -> Result<(), String> {
        let t = world.track.as_mut().ok_or("the route is surveyed first")?;
        let g = &self.g;
        t.runout = mr_levels::world::plan_runout("Streets", t, t.runout);
        world.sim_data.runout = t.runout;
        let (px, pz, hw) = (g.px, g.pz, g.hw);
        let mut nm = Vec::new();
        for tg in &t.tags {
            if tg.tag == "corner" {
                nm.push(MarkGap {
                    s0: tg.s0 - 2.0,
                    s1: tg.s1 + 2.0,
                });
            }
        }
        let mut st: Option<f64> = None;
        let mut s = 0.0;
        while s <= t.length {
            let x = arr_at(&t.px, s);
            let z = arr_at(&t.pz, s);
            let dx = (x - js::round(x / px) * px).abs();
            let dz = (z - js::round(z / pz) * pz).abs();
            let in_box = dx < hw + 5.0 && dz < hw + 5.0;
            if in_box && st.is_none() {
                st = Some(s);
            }
            if !in_box && let Some(s0) = st {
                nm.push(MarkGap { s0, s1: s });
                st = None;
            }
            s += 1.0;
        }
        // `t.noMarks = nm`.
        world.no_marks = nm;
        Ok(())
    }

    fn build(&mut self, world: &mut World) -> Result<(), String> {
        let World {
            track,
            graph,
            textures,
            road,
            root,
            animators,
            ..
        } = world;
        let t = track.as_ref().ok_or("the route is surveyed first")?;
        let group = graph.group("streets");
        graph.add(*root, group);
        self.group = Some(group);
        let mut anims: Vec<Box<dyn Animator>> = Vec::new();
        let m = materials(graph, textures, &mut anims, road.as_ref());
        let mut b = Bld {
            t,
            g: self.g.clone(),
            graph,
            textures,
            group,
            rng: Mulberry32::new(9090),
            m,
            anims,
            full_seg: BTreeSet::new(),
            cross_use: BTreeSet::new(),
            leg_segs: Vec::new(),
            gi0: 0,
            gi1: 0,
            gj0: 0,
            gj1: 0,
            core: [0.0, 0.0],
            blocks: Vec::new(),
            block_index: BTreeMap::new(),
            b_fac: Chunks::new(560.0, true, true),
            b_street: Chunks::new(560.0, true, true),
            b_pave: Chunks::new(560.0, true, false),
            b_asphalt: Chunks::new(560.0, false, false),
            b_paint: Chunks::new(560.0, true, false),
            b_plain: Chunks::new(560.0, true, false),
            b_glow: Chunks::new(560.0, true, false),
            b_car: Chunks::new(560.0, true, false),
            b_neon_h: Chunks::new(560.0, true, false),
            b_neon_v: Chunks::new(560.0, true, false),
            aircraft: Vec::new(),
            sign_lights: Vec::new(),
            trees: Vec::new(),
            spill: Vec::new(),
            steam: Vec::new(),
            cables: Vec::new(),
            fronts: Vec::new(),
            billboards: Vec::new(),
            ad_mats: Vec::new(),
            kit: None,
            barrier_count: 0,
        };
        b.analyse_route();
        b.plan_blocks();
        b.build_streets();
        b.build_blocks();
        b.build_lamps();
        b.build_signals();
        b.build_barriers();
        b.build_gantries();
        b.build_crowds();
        b.build_lanterns();
        props::kerb_furniture(&mut b);
        props::front_props(&mut b);
        props::bus_shelters(&mut b);
        props::parked_cars(&mut b);
        b.build_elevated();
        b.build_vents();
        b.build_reflections();
        props::build_puddles_and_spill(&mut b);
        props::build_steam(&mut b);
        props::build_wires(&mut b);
        b.emit_all();
        let anims = std::mem::take(&mut b.anims);
        // `group.traverse(o => { if ((o.isMesh || o.isPoints) &&
        // !o.userData.dynamic) { o.matrixAutoUpdate = false; o.updateMatrix(); } })`.
        let mut stack = vec![group];
        while let Some(id) = stack.pop() {
            let o = graph.get_mut(id);
            let drawn = o.ty.is_mesh() || o.ty == mr_scene::NodeType::Points;
            if drawn && o.user_data.get("dynamic") != Some(&Value::Bool(true)) {
                o.matrix_auto_update = false;
                o.update_matrix();
            }
            for &c in o.children.iter().rev() {
                stack.push(c);
            }
        }
        animators.extend(anims);
        Ok(())
    }
}

/// `materials()`: the shared materials, the neon clock and the route road's
/// wetter asphalt.
fn materials(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    anims: &mut Vec<Box<dyn Animator>>,
    road: Option<&crate::road::Road>,
) -> Mats {
    // Upper floors: the Streets façade atlas (per-window lights, street bounce).
    let fac = facade_material(graph, textures);
    let s = textures::street_atlas(textures);
    let s_map = graph.cached_texture(&s, Layer::Main, "");
    let s_em = graph.cached_texture(&s, Layer::Emissive, "");
    let street = graph.add_material(patch_street_atlas(
        Material::standard()
            .set("map", s_map)
            .set("emissiveMap", s_em)
            .set("emissive", 0xffffff)
            .set("emissiveIntensity", 1.2)
            .set("roughness", 0.7)
            .set("metalness", 0.05)
            .set("vertexColors", true),
    ));
    let pv = textures::pavement_texture(textures);
    let pv = graph.cached_texture(&pv, Layer::Main, "");
    // Pavements, props and plain dressing pick up a little of the street's
    // light (ambientPatch), so kerbs and walls don't sink to flat black.
    let pave = graph.add_material(ambient_patch(
        Material::standard()
            .set("map", pv)
            .set("vertexColors", true)
            .set("roughness", 0.78),
        [0.05, 0.042, 0.036],
        "pave",
    ));
    let a2 = textures.asphalt_texture(2);
    let a2 = graph.cached_texture(&a2, Layer::Main, "");
    let as_ = graph.clone_texture(a2);
    let asphalt = graph.add_material(
        Material::standard()
            .set("map", as_)
            .set("color", 0xb8b8b8)
            .set("roughness", 0.55)
            .set("metalness", 0.05)
            .set("polygonOffset", true)
            .set("polygonOffsetFactor", 1.0)
            .set("polygonOffsetUnits", 2.0),
    );
    let paint = graph.add_material(
        Material::standard()
            .set("vertexColors", true)
            .set("roughness", 0.55)
            .set("emissive", 0xffffff)
            .set("emissiveIntensity", 0.05)
            .set("polygonOffset", true)
            .set("polygonOffsetFactor", -2.0)
            .set("polygonOffsetUnits", -2.0),
    );
    let plain = graph.add_material(ambient_patch(
        Material::standard()
            .set("vertexColors", true)
            .set("roughness", 0.7)
            .set("metalness", 0.15),
        [0.06, 0.05, 0.045],
        "plain",
    ));
    // HDR vertex colours (LED bands).
    let glow = graph.add_material(Material::basic().set("vertexColors", true));
    let n = textures::neon_atlas(textures);
    let nh = graph.cached_texture(&n, Layer::Main, "");
    let nv = graph.cached_texture(&n, Layer::Emissive, "");
    // Single-sided: blade signs carry a face each way (see signQuad). Each
    // sign's vertices carry a seed and a mode, so a few buzz and blink.
    let neon_h = graph.add_material(neon_flicker(
        Material::basic()
            .set("map", nh)
            .set("color", Color::new(2.4, 2.4, 2.4)),
        "h",
    ));
    let neon_v = graph.add_material(neon_flicker(
        Material::basic()
            .set("map", nv)
            .set("color", Color::new(2.4, 2.4, 2.4)),
        "v",
    ));
    anims.push(Box::new(anim::NeonClock {
        time: 0.0,
        mats: [neon_h, neon_v],
    }));
    // Route road: a little wetter than a dry street.
    if let Some(road) = road
        && let Some(&(_, rm)) = road.materials.iter().find(|(k, _)| k == "asphalt2")
    {
        let mat = graph.material_mut(rm);
        mat.set_value("roughness", crate::material::Param::Num(0.62));
        mat.set_value("metalness", crate::material::Param::Num(0.05));
        let mut c = mat.color("color").unwrap_or(Color::new(1.0, 1.0, 1.0));
        c.set_scalar(0.72);
        mat.set_value("color", crate::material::Param::Color(c));
    }
    let car = graph.add_material(ambient_patch(
        Material::standard()
            .set("vertexColors", true)
            .set("roughness", 0.4)
            .set("metalness", 0.4),
        [0.05, 0.045, 0.045],
        "car",
    ));
    Mats {
        fac,
        street,
        pave,
        asphalt,
        paint,
        plain,
        glow,
        neon_h,
        neon_v,
        car,
    }
}

/// `Streets` while it builds: the JS `this` (the grid, the generator 9090,
/// the chunked builders and what the methods collect for later ones).
pub struct Bld<'a> {
    pub t: &'a Track,
    pub g: Grid,
    pub graph: &'a mut SceneGraph,
    pub textures: &'a mut TextureCache,
    pub group: NodeId,
    pub rng: Mulberry32,
    pub m: Mats,
    /// `world.updaters.push`, in order.
    pub anims: Vec<Box<dyn Animator>>,
    /// `this.fullSeg`: grid street segments the route runs along end to end.
    pub full_seg: BTreeSet<(char, i64, i64)>,
    /// `this.crossUse`'s keys: crossings the route uses (every value is
    /// truthy, so only the keys are read).
    pub cross_use: BTreeSet<(i64, i64)>,
    /// `this.legSegs`: `[x0, x1, z0, z1]`.
    pub leg_segs: Vec<[f64; 4]>,
    pub gi0: i64,
    pub gi1: i64,
    pub gj0: i64,
    pub gj1: i64,
    /// The finish-area core for the tallest towers.
    pub core: P2,
    /// `this.blocks` in insertion order, and its keys.
    pub blocks: Vec<Block>,
    pub block_index: BTreeMap<(i64, i64), usize>,
    pub b_fac: Chunks,
    pub b_street: Chunks,
    pub b_pave: Chunks,
    pub b_asphalt: Chunks,
    pub b_paint: Chunks,
    pub b_plain: Chunks,
    pub b_glow: Chunks,
    pub b_car: Chunks,
    pub b_neon_h: Chunks,
    pub b_neon_v: Chunks,
    pub aircraft: Vec<P3>,
    pub sign_lights: Vec<SignLight>,
    pub trees: Vec<Tree>,
    pub spill: Vec<Spill>,
    /// Steam emitters.
    pub steam: Vec<P3>,
    /// Overhead wire segments.
    pub cables: Vec<f64>,
    pub fronts: Vec<Front>,
    pub billboards: Vec<Billboard>,
    pub ad_mats: Vec<(usize, MaterialId)>,
    /// `streets/props.js`'s `KIT`, built once.
    pub kit: Option<props::Kit>,
    pub barrier_count: usize,
}

impl Bld<'_> {
    pub fn rng(&mut self) -> f64 {
        self.rng.next_f64()
    }

    /// A building's seed goes in the high bits of `cell` (see facades.js).
    pub fn seed(&mut self) -> f64 {
        1.0 + (self.rng() * 900.0).floor()
    }

    // ── The route on the grid ─────────────────────────────────────
    fn analyse_route(&mut self) {
        let t = self.t;
        let (px, pz, hw) = (self.g.px, self.g.pz, self.g.hw);
        // Grid street segments the route runs straight along, end to end
        // (Road.js paints and paves those), and crossings it uses.
        for leg in &self.g.legs {
            if leg.axis == 'z' {
                let j = i64::from(leg.line);
                let a = js::min(leg.x0, leg.x1);
                let b = js::max(leg.x0, leg.x1);
                let mut i = (a / px).floor() as i64 - 1;
                while i <= (b / px).ceil() as i64 {
                    let fi = i as f64;
                    if a <= fi * px + hw + 6.0 && b >= (fi + 1.0) * px - hw - 6.0 {
                        self.full_seg.insert(('h', i, j));
                    }
                    i += 1;
                }
            } else {
                let i = i64::from(leg.line);
                let a = js::min(leg.z0, leg.z1);
                let b = js::max(leg.z0, leg.z1);
                let mut j = (a / pz).floor() as i64 - 1;
                while j <= (b / pz).ceil() as i64 {
                    let fj = j as f64;
                    if a <= fj * pz + hw + 6.0 && b >= (fj + 1.0) * pz - hw - 6.0 {
                        self.full_seg.insert(('v', i, j));
                    }
                    j += 1;
                }
            }
        }
        let mut s = 0.0;
        while s <= t.length {
            let x = arr_at(&t.px, s);
            let z = arr_at(&t.pz, s);
            let i = js::round(x / px);
            let j = js::round(z / pz);
            if (x - i * px).abs() < hw && (z - j * pz).abs() < hw {
                self.cross_use.insert((i as i64, j as i64));
            }
            s += 2.0;
        }
        for tg in &t.tags {
            if tg.tag != "corner" {
                continue;
            }
            let m = js::round((tg.s0 + tg.s1) / 2.0);
            self.cross_use.insert((
                js::round(arr_at(&t.px, m) / px) as i64,
                js::round(arr_at(&t.pz, m) / pz) as i64,
            ));
        }
        // Route legs as axis-aligned segments for distance queries.
        self.leg_segs = self
            .g
            .legs
            .iter()
            .map(|l| {
                [
                    js::min(l.x0, l.x1),
                    js::max(l.x0, l.x1),
                    js::min(l.z0, l.z1),
                    js::max(l.z0, l.z1),
                ]
            })
            .collect();
        let b = t.bounds;
        self.gi0 = (b.min_x / px).floor() as i64 - 12;
        self.gi1 = (b.max_x / px).ceil() as i64 + 12;
        self.gj0 = (b.min_z / pz).floor() as i64 - 13;
        self.gj1 = (b.max_z / pz).ceil() as i64 + 13;
        // The finish-area core for the tallest towers.
        let f = t.frame(t.finish_s - 200.0);
        self.core = [f.x + 60.0, f.z - 150.0];
    }

    /// Height of whatever is underfoot: the race road's surface, else the ground.
    pub fn surface_at(&self, x: f64, z: f64) -> f64 {
        let r = self.t.distance_to_road(x, z, 30.0);
        if r.i >= 0
            && r.d < self.g.hw + 0.5
            && let Some(s) = r.s
        {
            return self.t.surface_y(s, r.lat);
        }
        ground(x, z)
    }

    pub fn route_dist(&self, x: f64, z: f64) -> f64 {
        let mut best = f64::INFINITY;
        for l in &self.leg_segs {
            let dx = js::max_n(&[l[0] - x, 0.0, x - l[1]]);
            let dz = js::max_n(&[l[2] - z, 0.0, z - l[3]]);
            best = js::min(best, kernel::hypot(dx, dz));
        }
        best
    }

    // ── Blocks: which exist, how detailed, and their kerb outlines ─
    fn plan_blocks(&mut self) {
        let (px, pz) = (self.g.px, self.g.pz);
        for i in self.gi0..=self.gi1 {
            for j in self.gj0..=self.gj1 {
                let cx = (i as f64 + 0.5) * px;
                let cz = (j as f64 + 0.5) * pz;
                let d = self.route_dist(cx, cz);
                let tier = if d < 70.0 {
                    0
                } else if d < 420.0 {
                    1
                } else if d < 1250.0 {
                    2
                } else {
                    -1
                };
                if tier < 0 {
                    continue;
                }
                let poly = self.block_poly(i, j, tier == 0);
                let b = Block {
                    i,
                    j,
                    tier,
                    d,
                    district: grid::district(i as i32),
                    cx,
                    cz,
                    poly,
                };
                self.block_index.insert((i, j), self.blocks.len());
                self.blocks.push(b);
            }
        }
    }

    pub fn block(&self, i: i64, j: i64) -> Option<&Block> {
        self.block_index.get(&(i, j)).map(|&k| &self.blocks[k])
    }

    /// Kerb line of block (i, j): a rounded rectangle, with any part inside the
    /// route's paved width pushed out to the road edge (the inside of corners).
    fn block_poly(&self, i: i64, j: i64, near_route: bool) -> Vec<P2> {
        let (px, pz, hw) = (self.g.px, self.g.pz, self.g.hw);
        let t = self.t;
        let (fi, fj) = (i as f64, j as f64);
        let x0 = fi * px + hw;
        let x1 = (fi + 1.0) * px - hw;
        let z0 = fj * pz + hw;
        let z1 = (fj + 1.0) * pz - hw;
        let r = 4.5;
        let mut pts: Vec<P2> = Vec::new();
        let corners = [
            [x1 - r, z0 + r, -PI / 2.0],
            [x1 - r, z1 - r, 0.0],
            [x0 + r, z1 - r, PI / 2.0],
            [x0 + r, z0 + r, PI],
        ];
        let step = if near_route { 1.5 } else { 1e9 };
        for k in 0..4 {
            let [cx, cz, a0] = corners[k];
            for q in 0..=5 {
                let a = a0 + (f64::from(q) / 5.0) * (PI / 2.0);
                pts.push([cx + kernel::cos(a) * r, cz + kernel::sin(a) * r]);
            }
            // Straight edge to the next corner's start.
            let [nx, nz, na] = corners[(k + 1) % 4];
            let sx = nx + kernel::cos(na) * r;
            let sz = nz + kernel::sin(na) * r;
            let [ex, ez] = pts[pts.len() - 1];
            let l = kernel::hypot(sx - ex, sz - ez);
            let n = js::max(1.0, (l / step).floor()) as i64;
            for q in 1..n {
                let q = q as f64;
                let nf = n as f64;
                pts.push([ex + ((sx - ex) * q) / nf, ez + ((sz - ez) * q) / nf]);
            }
        }
        if !near_route {
            return pts;
        }
        for p in pts.iter_mut() {
            let r = t.distance_to_road(p[0], p[1], 48.0);
            if r.i < 0 || r.d >= hw - 0.01 {
                continue;
            }
            let s = r.s.unwrap_or(f64::NAN);
            if !t.is_loop && (s < 0.5 || s > t.length - 0.5) {
                continue;
            }
            let f = t.frame(s);
            let side = if r.lat >= 0.0 { 1.0 } else { -1.0 };
            p[0] = f.x + f.rx * side * hw;
            p[1] = f.z + f.rz * side * hw;
        }
        // Drop points that bunched up, then straight-line runs.
        let mut out: Vec<P2> = Vec::new();
        for p in &pts {
            if out.is_empty() || {
                let l = out[out.len() - 1];
                kernel::hypot(p[0] - l[0], p[1] - l[1]) > 0.35
            } {
                out.push(*p);
            }
        }
        let mut simp = Vec::new();
        let n = out.len();
        for k in 0..n {
            let a = out[(k + n - 1) % n];
            let p = out[k];
            let c = out[(k + 1) % n];
            let cross = (p[0] - a[0]) * (c[1] - a[1]) - (p[1] - a[1]) * (c[0] - a[0]);
            let l = kernel::hypot(c[0] - a[0], c[1] - a[1]);
            if cross.abs() / js::or(l, 1.0) > 0.02 || l > 6.0 {
                simp.push(p);
            }
        }
        simp
    }

    pub fn block_at(&self, x: f64, z: f64) -> Option<&Block> {
        let (px, pz) = (self.g.px, self.g.pz);
        self.block((x / px).floor() as i64, (z / pz).floor() as i64)
    }

    /// Is (x, z) on a pavement (behind a kerb)?
    pub fn on_kerb(&self, x: f64, z: f64) -> bool {
        self.block_at(x, z)
            .is_some_and(|b| point_in_poly(x, z, &b.poly))
    }

    // ── Streets: asphalt, paint and pavements ─────────────────────
    fn build_streets(&mut self) {
        let (px, pz, hw) = (self.g.px, self.g.pz, self.g.hw);
        // Every street segment and crossing within the paved tiers.
        let mut seen: BTreeSet<(char, i64, i64)> = BTreeSet::new();
        for bi in 0..self.blocks.len() {
            let (b_i, b_j, tier) = {
                let b = &self.blocks[bi];
                (b.i, b.j, b.tier)
            };
            if tier > 1 {
                continue;
            }
            for [di, dj] in [[0, 0], [1, 0], [0, 1], [1, 1]] {
                let i = b_i + di;
                let j = b_j + dj;
                if !seen.insert(('c', i, j)) {
                    continue;
                }
                let (fi, fj) = (i as f64, j as f64);
                self.asph(fi * px - hw, fj * pz - hw, fi * px + hw, fj * pz + hw);
                self.crosswalks(i, j);
            }
            // Its north (h) and west (v) street segments, plus south/east when
            // the neighbouring block isn't paved.
            let paved = |s: &Self, i: i64, j: i64| s.block(i, j).is_some_and(|b| b.tier <= 1);
            let mut segs = vec![('h', b_i, b_j), ('v', b_i, b_j)];
            if !paved(self, b_i, b_j + 1) {
                segs.push(('h', b_i, b_j + 1));
            }
            if !paved(self, b_i + 1, b_j) {
                segs.push(('v', b_i + 1, b_j));
            }
            for (k, i, j) in segs {
                if !seen.insert((k, i, j)) {
                    continue;
                }
                let full = self.full_seg.contains(&(k, i, j));
                let (fi, fj) = (i as f64, j as f64);
                if k == 'h' {
                    if !full {
                        self.asph(
                            fi * px + hw,
                            fj * pz - hw,
                            (fi + 1.0) * px - hw,
                            fj * pz + hw,
                        );
                    }
                    self.lane_paint('h', i, j, full);
                } else {
                    if !full {
                        self.asph(
                            fi * px - hw,
                            fj * pz + hw,
                            fi * px + hw,
                            (fj + 1.0) * pz - hw,
                        );
                    }
                    self.lane_paint('v', i, j, full);
                }
            }
            self.pavement(bi, false);
        }
        // Far blocks: no kerbs, just a slab so buildings don't float on the terrain.
        for bi in 0..self.blocks.len() {
            if self.blocks[bi].tier == 2 {
                self.pavement(bi, true);
            }
        }
    }

    /// Axis-aligned rectangle, subdivided along its long side for hills.
    fn asph(&mut self, x0: f64, z0: f64, x1: f64, z1: f64) {
        let b = self.b_asphalt.at((x0 + x1) / 2.0, (z0 + z1) / 2.0);
        let along_x = x1 - x0 > z1 - z0;
        let l = if along_x { x1 - x0 } else { z1 - z0 };
        let n = js::max(1.0, (l / 6.0).ceil());
        let mut k = 0.0;
        while k < n {
            let a = k / n;
            let bb = (k + 1.0) / n;
            let [ax0, ax1, az0, az1] = if along_x {
                [lerp(x0, x1, a), lerp(x0, x1, bb), z0, z1]
            } else {
                [x0, x1, lerp(z0, z1, a), lerp(z0, z1, bb)]
            };
            let p = |x: f64, z: f64| [x, ground(x, z) - 0.012, z];
            b.quad(
                p(ax0, az1),
                p(ax1, az1),
                p(ax1, az0),
                p(ax0, az0),
                Some([
                    [ax0 / 9.0, az1 / 9.0],
                    [ax1 / 9.0, az1 / 9.0],
                    [ax1 / 9.0, az0 / 9.0],
                    [ax0 / 9.0, az0 / 9.0],
                ]),
                None,
                0.0,
            );
            k += 1.0;
        }
    }

    /// Zebra crossings on each arm of crossing (i, j).
    fn crosswalks(&mut self, i: i64, j: i64) {
        let (px, pz, hw) = (self.g.px, self.g.pz, self.g.hw);
        let cx = i as f64 * px;
        let cz = j as f64 * pz;
        let w_col = [0.9, 0.9, 0.88];
        for [ax, az] in [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]] {
            // Stripes run along the arm, spread across it.
            let d0 = hw + 1.2;
            let d1 = hw + 4.2;
            let mut q = -hw + 0.6;
            while q < hw - 0.5 {
                let w = 0.55;
                let pts = if ax != 0.0 {
                    [
                        [cx + ax * d0, cz + q],
                        [cx + ax * d1, cz + q],
                        [cx + ax * d1, cz + q + w],
                        [cx + ax * d0, cz + q + w],
                    ]
                } else {
                    [
                        [cx + q, cz + az * d0],
                        [cx + q, cz + az * d1],
                        [cx + q + w, cz + az * d1],
                        [cx + q + w, cz + az * d0],
                    ]
                };
                let b = self.b_paint.at(cx, cz);
                paint_quad(b, &pts, w_col, 0.03);
                q += 1.2;
            }
        }
    }

    /// Lane lines on a street segment. On route segments Road.js has painted
    /// the lanes; we only add them on the arms of corners it left blank.
    fn lane_paint(&mut self, k: char, i: i64, j: i64, full: bool) {
        let (px, pz, hw, setback) = (self.g.px, self.g.pz, self.g.hw, self.g.setback);
        if full {
            return;
        }
        let y_col = [0.95, 0.72, 0.12];
        let wt = [0.92, 0.92, 0.9];
        let along = k == 'h';
        let (fi, fj) = (i as f64, j as f64);
        let a0 = if along { fi * px } else { fj * pz };
        let a1 = if along {
            (fi + 1.0) * px
        } else {
            (fj + 1.0) * pz
        };
        let c = if along { fj * pz } else { fi * px };
        let (bx, bz) = if along {
            ((a0 + a1) / 2.0, c)
        } else {
            (c, (a0 + a1) / 2.0)
        };
        // Where the route's corner already covers part of this segment, stop the
        // paint at the corner's start so it doesn't cross the curve.
        let end_a = self.cross_use.contains(&(i, j));
        let end_b = self
            .cross_use
            .contains(&if along { (i + 1, j) } else { (i, j + 1) });
        let on_route_line = self.seg_touches_route(k, i, j);
        let mut s0 = a0 + hw + 4.6;
        let mut s1 = a1 - hw - 4.6;
        if on_route_line && end_a {
            s0 = js::max(s0, a0 + setback + 1.0);
        }
        if on_route_line && end_b {
            s1 = js::min(s1, a1 - setback - 1.0);
        }
        if on_route_line && end_a && end_b {
            return;
        }
        let b = self.b_paint.at(bx, bz);
        let strip = |b: &mut GeoBuilder, lat: f64, w: f64, col: P3, dash: Option<[f64; 2]>| {
            if s1 - s0 <= 0.5 {
                return;
            }
            let mut pieces = Vec::new();
            let mut u = s0;
            match dash {
                Some(d) => {
                    while u < s1 {
                        pieces.push([u, js::min(s1, u + d[0])]);
                        u += d[1];
                    }
                }
                None => {
                    while u < s1 {
                        pieces.push([u, js::min(s1, u + 6.0)]);
                        u += 6.0;
                    }
                }
            }
            for [u0, u1] in pieces {
                let pts = if along {
                    [
                        [u0, c + lat - w / 2.0],
                        [u1, c + lat - w / 2.0],
                        [u1, c + lat + w / 2.0],
                        [u0, c + lat + w / 2.0],
                    ]
                } else {
                    [
                        [c + lat - w / 2.0, u0],
                        [c + lat - w / 2.0, u1],
                        [c + lat + w / 2.0, u1],
                        [c + lat + w / 2.0, u0],
                    ]
                };
                paint_quad(b, &pts, col, 0.03);
            }
        };
        strip(b, -0.14, 0.12, y_col, None);
        strip(b, 0.14, 0.12, y_col, None);
        strip(b, -hw / 2.0, 0.13, wt, Some([3.0, 9.0]));
        strip(b, hw / 2.0, 0.13, wt, Some([3.0, 9.0]));
        // Stop lines: traffic keeps right, so the approach half is on its right.
        let stop = |b: &mut GeoBuilder, u: f64, sign: f64| {
            let lat0 = if sign > 0.0 { 0.25 } else { -hw + 0.3 };
            let lat1 = if sign > 0.0 { hw - 0.3 } else { -0.25 };
            let pts = if along {
                [
                    [u - 0.2, c + lat0],
                    [u + 0.2, c + lat0],
                    [u + 0.2, c + lat1],
                    [u - 0.2, c + lat1],
                ]
            } else {
                [
                    [c + lat0, u - 0.2],
                    [c + lat0, u + 0.2],
                    [c + lat1, u + 0.2],
                    [c + lat1, u - 0.2],
                ]
            };
            paint_quad(b, &pts, wt, 0.03);
        };
        // Heading +along, the right side is +lat for 'h' (right of east is south, +z)
        // and −lat for 'v' (right of south is west, −x).
        if !(on_route_line && end_b) {
            stop(b, a1 - hw - 4.8, if along { 1.0 } else { -1.0 });
        }
        if !(on_route_line && end_a) {
            stop(b, a0 + hw + 4.8, if along { -1.0 } else { 1.0 });
        }
    }

    /// Does the route run along any part of this grid street segment?
    fn seg_touches_route(&self, k: char, i: i64, j: i64) -> bool {
        let (px, pz) = (self.g.px, self.g.pz);
        let (fi, fj) = (i as f64, j as f64);
        for leg in &self.g.legs {
            if k == 'h' && leg.axis == 'z' && i64::from(leg.line) == j {
                let a = js::min(leg.x0, leg.x1);
                let b = js::max(leg.x0, leg.x1);
                if b > fi * px - 40.0 && a < (fi + 1.0) * px + 40.0 {
                    return true;
                }
            }
            if k == 'v' && leg.axis == 'x' && i64::from(leg.line) == i {
                let a = js::min(leg.z0, leg.z1);
                let b = js::max(leg.z0, leg.z1);
                if b > fj * pz - 40.0 && a < (fj + 1.0) * pz + 40.0 {
                    return true;
                }
            }
        }
        false
    }

    /// Pavement slab for a block: kerb face, kerb stones, pavement ring and
    /// the lot inside, all following the ground.
    fn pavement(&mut self, bi: usize, simple: bool) {
        let walk = self.g.walk;
        let poly = self.blocks[bi].poly.clone();
        let n = poly.len();
        let mut cx = 0.0;
        let mut cz = 0.0;
        for p in &poly {
            cx += p[0];
            cz += p[1];
        }
        cx /= n as f64;
        cz /= n as f64;
        let b = self.b_pave.at(cx, cz);
        let kerb = [0.66, 0.65, 0.62];
        let walk_c = [0.5, 0.48, 0.46];
        let lot = [0.44, 0.42, 0.4];
        // Inward offsets along vertex normals (the outline is convex).
        let inset = |d: f64| -> Vec<P2> {
            (0..n)
                .map(|k| {
                    let p = poly[k];
                    let a = poly[(k + n - 1) % n];
                    let c = poly[(k + 1) % n];
                    let mut nx = 0.0;
                    let mut nz = 0.0;
                    for (u, v) in [(a, p), (p, c)] {
                        let ex = v[0] - u[0];
                        let ez = v[1] - u[1];
                        let l = js::or(kernel::hypot(ex, ez), 1.0);
                        nx += -ez / l;
                        nz += ex / l;
                    }
                    let l = js::or(kernel::hypot(nx, nz), 1.0);
                    nx /= l;
                    nz /= l;
                    // Point the normal inward.
                    if (cx - p[0]) * nx + (cz - p[1]) * nz < 0.0 {
                        nx = -nx;
                        nz = -nz;
                    }
                    [p[0] + nx * d, p[1] + nz * d]
                })
                .collect()
        };
        let lift = 0.15;
        let v = |p: P2, dy: f64| -> P3 { [p[0], ground(p[0], p[1]) + dy, p[1]] };
        let quad_up = |b: &mut GeoBuilder, a: P3, bb: P3, c: P3, d: P3, col: P3| {
            let n1 = (bb[2] - a[2]) * (c[0] - a[0]) - (bb[0] - a[0]) * (c[2] - a[2]);
            let uv = [a, bb, c, d].map(|q| [q[0] / 3.0, q[2] / 3.0]);
            if n1 >= 0.0 {
                b.quad(a, bb, c, d, Some(uv), Some(col), 0.0);
            } else {
                b.quad(
                    a,
                    d,
                    c,
                    bb,
                    Some([uv[0], uv[3], uv[2], uv[1]]),
                    Some(col),
                    0.0,
                );
            }
        };
        if simple {
            let c = [cx, ground(cx, cz) + 0.05, cz];
            for k in 0..n {
                let a = v(poly[k], 0.05);
                let d = v(poly[(k + 1) % n], 0.05);
                quad_up(b, a, d, c, c, lot);
            }
            return;
        }
        let r1 = inset(0.35);
        let r2 = inset(walk);
        for k in 0..n {
            let k2 = (k + 1) % n;
            // Kerb face, facing out.
            let a = poly[k];
            let c = poly[k2];
            let lo = -0.12;
            let a0 = v(a, lo);
            let a1 = v(a, lift);
            let c0 = v(c, lo);
            let c1 = v(c, lift);
            let ex = c[0] - a[0];
            let ez = c[1] - a[1];
            let outward = (a[0] - cx) * -ez + (a[1] - cz) * ex; // sign of the left normal · outward
            if outward > 0.0 {
                b.quad(a0, c0, c1, a1, None, Some(kerb), 0.0);
            } else {
                b.quad(c0, a0, a1, c1, None, Some(kerb), 0.0);
            }
            quad_up(
                b,
                v(a, lift),
                v(c, lift),
                v(r1[k2], lift),
                v(r1[k], lift),
                kerb,
            );
            quad_up(
                b,
                v(r1[k], lift),
                v(r1[k2], lift),
                v(r2[k2], lift),
                v(r2[k], lift),
                walk_c,
            );
        }
        // The lot inside (mostly under buildings; open corners read as plazas).
        let c = [cx, ground(cx, cz) + lift, cz];
        for k in 0..n {
            quad_up(b, v(r2[k], lift), v(r2[(k + 1) % n], lift), c, c, lot);
        }
    }

    // ── Emit ──────────────────────────────────────────────────────
    fn emit_all(&mut self) {
        let g = self.group;
        let recv = StaticOpts::default();
        let no_recv = StaticOpts {
            receive: false,
            ..StaticOpts::default()
        };
        for b in &self.b_fac.builders {
            if b.is_empty() {
                continue;
            }
            let geo = self
                .graph
                .add_geometry(props::emit_data(b.build(), "fdata"));
            let m = static_mesh(self.graph, geo, self.m.fac, &recv);
            self.graph.add(g, m);
        }
        self.b_street.emit(self.graph, g, self.m.street, &recv);
        self.b_pave.emit(self.graph, g, self.m.pave, &recv);
        self.b_asphalt.emit(self.graph, g, self.m.asphalt, &recv);
        self.b_paint.emit(self.graph, g, self.m.paint, &recv);
        self.b_plain.emit(self.graph, g, self.m.plain, &recv);
        self.b_glow.emit(self.graph, g, self.m.glow, &no_recv);
        self.b_car.emit(self.graph, g, self.m.car, &recv);
        for (chunks, mat) in [
            (&self.b_neon_h, self.m.neon_h),
            (&self.b_neon_v, self.m.neon_v),
        ] {
            for b in &chunks.builders {
                if b.is_empty() {
                    continue;
                }
                let geo = self
                    .graph
                    .add_geometry(props::emit_data(b.build(), "ndata"));
                let m = static_mesh(self.graph, geo, mat, &no_recv);
                self.graph.add(g, m);
            }
        }
        self.build_trees();
        self.build_aircraft_lights();
    }
}

/// `paintQuad(B, pts, col, lift = 0.03)`.
pub fn paint_quad(b: &mut GeoBuilder, pts: &[P2; 4], col: P3, lift: f64) {
    let p = pts.map(|[x, z]| [x, ground(x, z) + lift, z]);
    // Face up whichever way the corners wind.
    let n = (p[1][2] - p[0][2]) * (p[2][0] - p[0][0]) - (p[1][0] - p[0][0]) * (p[2][2] - p[0][2]);
    if n >= 0.0 {
        b.quad(p[0], p[1], p[2], p[3], None, Some(col), 0.0);
    } else {
        b.quad(p[0], p[3], p[2], p[1], None, Some(col), 0.0);
    }
}
