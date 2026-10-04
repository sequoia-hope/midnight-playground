//! Port of `src/world/City.js` (roadmap WP 3.8): Meridian, a street grid
//! laid out along the Interstate. On Level 1 it is the last zone — mid-rise
//! outskirts climbing to a skyscraper core around the finish. On the endless
//! cruise the whole level is a freeway loop through the city, with districts
//! strung round it: two downtown cores, an industrial belt under the
//! viaducts, a waterfront, brick residential and a midtown. The freeway
//! furniture itself (westbound lanes, lights, gantries, tunnels, overpasses,
//! finish/welcome gantry) lives in [`freeway`]; the city's textures and its
//! façade material in [`textures`].
//!
//! The JS class keeps its working state on `this` while it builds; here
//! [`City`] holds what `plan()` decides and a `Build` holds the rest for
//! the length of `build()`. Every draw from the seeded generators happens in
//! the JS order; each `world.updaters.push` is an [`Animator`] in the JS
//! order.

// The JS signatures, loops and comparisons are kept (DECISIONS D52, D130),
// and its literals: the trees turn by `rng() * 6.28`, not τ.
#![allow(
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::type_complexity,
    clippy::manual_range_contains,
    clippy::needless_late_init,
    clippy::approx_constant
)]

pub mod freeway;
pub mod textures;

use std::collections::{BTreeMap, BTreeSet};

use mr_levels::world::City as WorldCity;
use mr_math::{Mulberry32, clamp, js, kernel, lerp, smoothstep};
use mr_scene::{MaterialKind, NodeType, three};
use mr_track::{Frame, Track};
use serde_json::Value;

use self::freeway::{
    AtlasQuad, Freeway, FwCtx, FwPath, OPP_C, Prof, SweepOpts, SweepUv, atlas_quads, prof, step,
    sweep,
};
use self::textures::{
    BRICK_CELL, CELL_COLS, CELL_ROWS, CELL_TILE, GLASS_CELL, ROOF_CELL, WAREHOUSE_CELL, ad_texture,
    cell_seed, city_facade_atlas, patch_city_material,
};
use crate::color::Color;
use crate::geom::{GeoBuilder, P2, P3, PrismOpts, StaticOpts, instanced, static_mesh, trs};
use crate::material::{Material, num};
use crate::object::{Layer, MaterialId, NodeId, SceneGraph};
use crate::terrain::Terrain;
use crate::textures::TextureCache;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, Matrix4, Quaternion, Sphere, Vector3,
    cylinder_geometry, icosahedron_geometry, plane_geometry, sphere_geometry, torus_geometry,
};
use crate::world::{Animator, Change, Edit, Handle, Scenery, SceneryInfo, UpdateCtx, World};

const PI: f64 = std::f64::consts::PI;

/// Westbound lanes carry on behind the on-ramp merge.
const BACK_EXT: f64 = 700.0;
/// And both directions carry on past the finish.
const FWD_EXT: f64 = 900.0;
const PU: f64 = 110.0;
const PV: f64 = 90.0;
const STREET: f64 = 14.0;

const NEON: [[f64; 3]; 7] = [
    [3.2, 0.25, 2.6],
    [0.3, 2.6, 3.2],
    [3.4, 0.5, 1.0],
    [3.2, 1.6, 0.3],
    [0.5, 3.2, 1.2],
    [1.8, 0.5, 3.4],
    [3.0, 3.0, 3.2],
];

// ── Small records ────────────────────────────────────────────────────────

/// The street grid: `toU`, `toV`, `toWorld` about an origin, turned by an
/// angle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grid {
    pub pu: f64,
    pub pv: f64,
    pub street: f64,
    pub u_axis: [f64; 2],
    pub v_axis: [f64; 2],
    ox: f64,
    oz: f64,
    c: f64,
    s: f64,
}

impl Grid {
    fn new(ang: f64, ox: f64, oz: f64) -> Grid {
        let c = kernel::cos(ang);
        let s = kernel::sin(ang);
        Grid {
            pu: PU,
            pv: PV,
            street: STREET,
            u_axis: [c, s],
            v_axis: [-s, c],
            ox,
            oz,
            c,
            s,
        }
    }

    pub fn to_u(&self, x: f64, z: f64) -> f64 {
        (x - self.ox) * self.c + (z - self.oz) * self.s
    }

    pub fn to_v(&self, x: f64, z: f64) -> f64 {
        -(x - self.ox) * self.s + (z - self.oz) * self.c
    }

    pub fn to_world(&self, u: f64, v: f64) -> P2 {
        [
            self.ox + u * self.c - v * self.s,
            self.oz + u * self.s + v * self.c,
        ]
    }
}

/// A street lamp's place in grid space: `{ u, v, du, dv, h?, arm? }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampSpot {
    pub u: f64,
    pub v: f64,
    pub du: f64,
    pub dv: f64,
    pub h: Option<f64>,
    pub arm: Option<f64>,
}

/// A tree: `{ x, y, z, s }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParkTree {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub s: f64,
}

/// A lamp in world space (`buildLamps`' spots and `extraLamps`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    x: f64,
    y: f64,
    z: f64,
    dx: f64,
    dz: f64,
    h: f64,
    arm: Option<f64>,
    led: bool,
}

/// One of `freewayHits`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hit {
    lat: f64,
    y: f64,
    back: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Core {
    x: f64,
    z: f64,
    r: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Waterfront {
    s0: f64,
    s1: f64,
    lat: f64,
    width: f64,
    depth: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum District {
    Interior,
    Downtown,
    Industrial,
    Waterfront,
    Midrise,
    Residential,
}

/// What `building()` is asked for: `{ tower, core, ring, r, cell, floors }`.
#[derive(Clone, Copy, Debug, Default)]
struct BuildingOpts {
    tower: bool,
    core: f64,
    ring: f64,
    cell: Option<usize>,
    floors: Option<[f64; 2]>,
}

// ── The module ───────────────────────────────────────────────────────────

/// `class City` (`new City({ zone, key, level })`), with what `plan()`
/// decided.
pub struct City {
    pub zone: usize,
    pub key: String,
    is_loop: bool,
    s_a: f64,
    path: Option<FwPath>,
    centre: Option<P2>,
    /// `this.insideSign` (0 until the loop's plan sets it).
    inside_sign: f64,
    waterfront: Option<Waterfront>,
}

impl City {
    pub fn new(info: &SceneryInfo) -> City {
        City {
            zone: info.zone,
            key: info.key.to_string(),
            is_loop: false,
            s_a: 0.0,
            path: None,
            centre: None,
            inside_sign: 0.0,
            waterfront: None,
        }
    }

    // Loop: level the middle of the ring (the city landform raises hills far
    // from the road) and dig the waterfront basin outside it.
    fn plan_loop(&mut self, t: &Track, tr: &mut Terrain) {
        let mut cx = 0.0;
        let mut cz = 0.0;
        let mut i = 0;
        while i < t.n {
            cx += f64::from(t.px[i]);
            cz += f64::from(t.pz[i]);
            i += 10;
        }
        let m = (t.n as f64 / 10.0).ceil();
        cx /= m;
        cz /= m;
        self.centre = Some([cx, cz]);
        // Which side of the road is the inside of the ring (+1 = right).
        let mut votes = 0.0;
        let mut i = 0;
        while i < t.n {
            votes += js::sign(
                (cx - f64::from(t.px[i])) * f64::from(t.rx[i])
                    + (cz - f64::from(t.pz[i])) * f64::from(t.rz[i]),
            );
            i += 97;
        }
        self.inside_sign = if votes >= 0.0 { 1.0 } else { -1.0 };
        let g = tr
            .flat_y
            .get(self.zone)
            .copied()
            .flatten()
            .unwrap_or(tr.city_y)
            - 0.25;
        tr.add_flatten(cx, cz, 1350.0, 260.0, Some(g));
        let l = t.length;
        let wf = Waterfront {
            s0: js::round(0.405 * l),
            s1: js::round(0.495 * l),
            lat: -self.inside_sign * 230.0,
            width: 170.0,
            depth: 6.0,
        };
        let mut pts = Vec::new();
        let mut s = wf.s0 - 150.0;
        while s <= wf.s1 + 150.0 {
            let f = t.frame(s);
            pts.push([f.x + f.rx * wf.lat, f.z + f.rz * wf.lat]);
            s += 30.0;
        }
        tr.add_carve(pts, wf.width, wf.depth, false);
        self.waterfront = Some(wf);
    }
}

impl Scenery for City {
    fn name(&self) -> &str {
        "City"
    }

    fn label(&self) -> Option<&str> {
        Some("Building the city")
    }

    fn plan(&mut self, w: &mut World) -> Result<(), String> {
        let World {
            track,
            terrain,
            sim_data,
            ..
        } = w;
        let t = track.as_mut().ok_or("the route is surveyed first")?;
        let tr = terrain.as_mut().ok_or("no terrain")?;
        self.is_loop = t.is_loop;
        if self.is_loop {
            self.s_a = 0.0;
            self.path = Some(FwPath::new(t, 0.0, t.length, 0.0, 0.0));
            self.plan_loop(t, tr);
            return Ok(());
        }
        // `t.tag('merge')[0]?.s0 ?? Z.s0 + 220` and the runout, by the rules
        // the simulation reads its world data with (mr_levels::world::City).
        self.s_a = WorldCity::s_a(t, self.zone);
        let path = FwPath::new(t, self.s_a, t.length, BACK_EXT, FWD_EXT);
        t.runout = WorldCity::plan_runout(t, t.runout); // drivable after the finish
        sim_data.runout = t.runout;
        // Level ground under the westbound lanes behind the merge, where the
        // terrain doesn't know about them.
        let mut u = self.s_a - BACK_EXT;
        while u < self.s_a - 15.0 {
            let f = path.frame(t, u);
            tr.add_flatten(
                f.x + f.rx * OPP_C,
                f.z + f.rz * OPP_C,
                16.0,
                45.0,
                Some(f.y - 0.04),
            );
            u += 30.0;
        }
        self.path = Some(path);
        Ok(())
    }

    fn build(&mut self, w: &mut World) -> Result<(), String> {
        let World {
            track,
            terrain,
            graph,
            textures,
            root,
            animators,
            sim_data,
            ..
        } = w;
        let t = track.as_ref().ok_or("the route is surveyed first")?;
        let tr = terrain.as_ref().ok_or("no terrain")?;
        let path = self.path.clone().ok_or("City.plan has not run")?;
        let mut b = Build::new(self, t, tr, graph, textures, path);
        graph_add(b.graph, *root, b.group);
        b.run();
        // For traffic: the westbound carriageway (lat relative to our centreline).
        // Level 1: from the merge to the end. Loop: all the way round.
        sim_data.opposite_carriageway = Some(WorldCity::opposite_carriageway(t, self.zone));
        animators.extend(b.finish());
        Ok(())
    }
}

fn graph_add(g: &mut SceneGraph, parent: NodeId, child: NodeId) {
    g.add(parent, child);
}

// ── Lookups that need no build state ─────────────────────────────────────

/// Nearest-cell far-field lookups (no interpolation, so s doesn't smear
/// across the loop's seam).
fn far_cell(tr: &Terrain, x: f64, z: f64) -> usize {
    let (fw, fh) = tr.far_size();
    let i = clamp(js::round((x - tr.min_x) / 32.0), 0.0, fw as f64 - 1.0);
    let j = clamp(js::round((z - tr.min_z) / 32.0), 0.0, fh as f64 - 1.0);
    j as usize * fw + i as usize
}

fn nearest_s(tr: &Terrain, x: f64, z: f64) -> f64 {
    tr.far_cell(far_cell(tr, x, z)).0
}

/// Loop districts by position along the ring.
fn district_at(t: &Track, s: f64) -> District {
    let l = t.length;
    let within = |a: f64, b: f64| {
        let d = t.ds(a, s);
        let w = t.ds(a, b);
        d >= 0.0 && d <= w
    };
    if t.tag("downtown")
        .iter()
        .any(|g| within(g.s0 - 260.0, g.s1 + 260.0))
    {
        return District::Downtown;
    }
    if ["viaduct", "viaduct-up", "viaduct-down"]
        .iter()
        .any(|n| t.tag(n).iter().any(|g| within(g.s0 - 120.0, g.s1 + 120.0)))
    {
        return District::Industrial;
    }
    let f = t.wrap(s) / l;
    if f > 0.405 && f < 0.495 {
        return District::Waterfront;
    }
    if f > 0.86 || f < 0.07 {
        return District::Midrise;
    }
    District::Residential
}

/// What `ledAt` needs: white LED street lights downtown, sodium elsewhere.
struct Led {
    is_loop: bool,
    dc: Option<Frame>,
    cores: Vec<Core>,
}

impl Led {
    fn at(&self, t: &Track, tr: &Terrain, x: f64, z: f64) -> bool {
        if !self.is_loop {
            return self
                .dc
                .is_some_and(|dc| kernel::hypot(x - dc.x, z - dc.z) < 750.0);
        }
        for c in &self.cores {
            if kernel::hypot(x - c.x, z - c.z) < c.r * 0.9 {
                return true;
            }
        }
        district_at(t, nearest_s(tr, x, z)) == District::Midrise
    }
}

fn circle(x: f64, z: f64, r: f64, n: usize) -> Vec<P2> {
    (0..n)
        .map(|k| {
            let a = (k as f64 / n as f64) * PI * 2.0;
            [x + kernel::cos(a) * r, z + kernel::sin(a) * r]
        })
        .collect()
}

fn hash2(x: f64, z: f64) -> f64 {
    let v = kernel::sin(x * 12.9898 + z * 78.233) * 43758.5453;
    v - v.floor()
}

fn normal_y(a: &P3, b: &P3, c: &P3) -> f64 {
    let abx = b[0] - a[0];
    let abz = b[2] - a[2];
    let acx = c[0] - a[0];
    let acz = c[2] - a[2];
    abz * acx - abx * acz
}

/// Walls facing into a closed footprint (the inside of a parapet).
fn walls_inward(b: &mut GeoBuilder, pts: &[P2], y0: f64, y1: f64, cell: usize) {
    let n = pts.len();
    let mut area = 0.0;
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        area += p[0] * q[1] - q[0] * p[1];
    }
    let c: Vec<P2> = if area > 0.0 {
        pts.iter().rev().copied().collect()
    } else {
        pts.to_vec()
    };
    for i in 0..n {
        let (p, q) = (c[i], c[(i + 1) % n]);
        b.quad(
            [q[0], y0, q[1]],
            [p[0], y0, p[1]],
            [p[0], y1, p[1]],
            [q[0], y1, q[1]],
            Some([[0.0, 0.0], [1.0, 0.0], [1.0, 0.2], [0.0, 0.2]]),
            None,
            cell as f64,
        );
    }
}

/// Horizontal quad forced to face up (or down).
fn up_quad(
    b: &mut GeoBuilder,
    a: P3,
    bb: P3,
    c: P3,
    d: P3,
    cell: f64,
    col: Option<P3>,
    down: bool,
) {
    if (normal_y(&a, &bb, &c) >= 0.0) != down {
        b.quad(a, bb, c, d, None, col, cell);
    } else {
        b.quad(a, d, c, bb, None, col, cell);
    }
}

/// Push each corner outward from the centre by `d` metres.
fn grow_rect(rect: &[P2], c: P2, d: f64) -> Vec<P2> {
    rect.iter()
        .map(|p| {
            let dx = p[0] - c[0];
            let dz = p[1] - c[1];
            let l = js::or(kernel::hypot(dx, dz), 1.0);
            [p[0] + (dx / l) * d * 1.41, p[1] + (dz / l) * d * 1.41]
        })
        .collect()
}

/// `mergeTwo(a, b)`: position, normal and colour concatenated.
fn merge_two(a: &BufferGeometry, b: &BufferGeometry) -> BufferGeometry {
    let mut g = BufferGeometry::new();
    for name in ["position", "normal", "color"] {
        let (Some(aa), Some(bb)) = (a.get_attribute(name), b.get_attribute(name)) else {
            continue;
        };
        let mut arr = aa.as_f32().expect("Float32").to_vec();
        arr.extend_from_slice(bb.as_f32().expect("Float32"));
        g.set_attribute(name, BufferAttribute::from_f32(arr, aa.item_size));
    }
    g
}

/// `mergeFlatGeos(geos)`: indexed or not, into one non-indexed geometry
/// (position + normal only).
fn merge_flat_geos(geos: &[BufferGeometry]) -> BufferGeometry {
    let flat: Vec<BufferGeometry> = geos
        .iter()
        .map(|g| {
            if g.index.is_some() {
                g.to_non_indexed()
            } else {
                g.clone()
            }
        })
        .collect();
    let mut out = BufferGeometry::new();
    for n in ["position", "normal"] {
        let mut arr: Vec<f32> = Vec::new();
        for g in &flat {
            arr.extend_from_slice(g.get_attribute(n).expect("attr").as_f32().expect("f32"));
        }
        out.set_attribute(n, BufferAttribute::from_f32(arr, 3));
    }
    out.compute_bounding_sphere();
    out
}

/// `paint(g, c)`: a Float32 colour attribute of one colour.
fn paint(g: &mut BufferGeometry, c: P3) {
    let n = g.get_attribute("position").expect("position").count();
    let mut a = Vec::with_capacity(n * 3);
    for _ in 0..n {
        a.extend_from_slice(&[c[0] as f32, c[1] as f32, c[2] as f32]);
    }
    g.set_attribute("color", BufferAttribute::from_f32(a, 3));
}

/// A chunk index (`Math.floor(x / size)`), as a key.
fn chunk_key(x: f64, z: f64, size: f64) -> (i64, i64) {
    ((x / size).floor() as i64, (z / size).floor() as i64)
}

/// A set of GeoBuilders keyed by spatial chunk; call quad/prism/box as on a
/// single builder and each primitive lands in the chunk of its first vertex.
struct ChunkedGeo {
    size: f64,
    color: bool,
    map: Vec<GeoBuilder>,
    index: BTreeMap<(i64, i64), usize>,
}

impl ChunkedGeo {
    fn new(size: f64, color: bool) -> ChunkedGeo {
        ChunkedGeo {
            size,
            color,
            map: Vec::new(),
            index: BTreeMap::new(),
        }
    }

    fn of(&mut self, x: f64, z: f64) -> &mut GeoBuilder {
        let key = chunk_key(x, z, self.size);
        let i = match self.index.get(&key) {
            Some(&i) => i,
            None => {
                self.map.push(GeoBuilder::new(self.color, false));
                self.index.insert(key, self.map.len() - 1);
                self.map.len() - 1
            }
        };
        &mut self.map[i]
    }

    fn quad(&mut self, a: P3, b: P3, c: P3, d: P3, col: Option<P3>) {
        self.of(a[0], a[2]).quad(a, b, c, d, None, col, 0.0);
    }

    fn prism(&mut self, c: &[P2], y0: f64, y1: f64, o: &PrismOpts) {
        self.of(c[0][0], c[0][1]).prism(c, y0, y1, o);
    }

    fn box_(&mut self, x: f64, y: f64, z: f64, sx: f64, sy: f64, sz: f64, yaw: f64, o: &PrismOpts) {
        self.of(x, z).box_(x, y, z, sx, sy, sz, yaw, o);
    }
}

/// A mesh that hides beyond `far` metres of the camera (`fadeable`).
#[derive(Clone, Copy)]
struct Fade {
    node: NodeId,
    c: Vector3,
    r: f64,
    far: f64,
}

#[derive(Default)]
struct Stats {
    blocks: u32,
    buildings: u32,
    towers: u32,
}

fn cell_opts(cell: usize, tile: f64) -> PrismOpts {
    PrismOpts {
        cell: Some(cell as f64),
        tile_w: Some(tile),
        tile_h: Some(tile),
        ..PrismOpts::default()
    }
}

fn no_roof(o: PrismOpts) -> PrismOpts {
    PrismOpts {
        roof: Some(false),
        ..o
    }
}

fn colored(col: P3, roof: bool) -> PrismOpts {
    PrismOpts {
        color: Some(col),
        roof: Some(roof),
        ..PrismOpts::default()
    }
}

fn scale3(c: [f64; 3], k: f64) -> P3 {
    [c[0] * k, c[1] * k, c[2] * k]
}

const ASPH: P3 = [0.07, 0.07, 0.075];
const WALK: P3 = [0.38, 0.37, 0.35];
const GRASS: P3 = [0.12, 0.24, 0.07];
const PLAZA: P3 = [0.46, 0.43, 0.38];
const YARD: P3 = [0.1, 0.1, 0.11];

/// Lots in a block, as fractions: [a0, b0, a1, b1].
const Q4: [[f64; 4]; 4] = [
    [0.0, 0.0, 0.5, 0.5],
    [0.5, 0.0, 1.0, 0.5],
    [0.0, 0.5, 0.5, 1.0],
    [0.5, 0.5, 1.0, 1.0],
];
const Q6: [[f64; 4]; 6] = [
    [0.0, 0.0, 1.0 / 3.0, 0.5],
    [1.0 / 3.0, 0.0, 2.0 / 3.0, 0.5],
    [2.0 / 3.0, 0.0, 1.0, 0.5],
    [0.0, 0.5, 1.0 / 3.0, 1.0],
    [1.0 / 3.0, 0.5, 2.0 / 3.0, 1.0],
    [2.0 / 3.0, 0.5, 1.0, 1.0],
];
const Q2: [[f64; 4]; 2] = [[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 1.0, 1.0]];
const Q1: [[f64; 4]; 1] = [[0.0, 0.0, 1.0, 1.0]];

/// A block of the grid as `loopBlock` gets it.
#[derive(Clone, Copy)]
struct Block {
    bu0: f64,
    bu1: f64,
    bv0: f64,
    bv1: f64,
    cw: P2,
    district: District,
    block_clear: bool,
}

// ── The build ────────────────────────────────────────────────────────────

/// `City.build`'s working state (`this.*` while it builds).
struct Build<'w> {
    track: &'w Track,
    terrain: &'w Terrain,
    graph: &'w mut SceneGraph,
    textures: &'w mut TextureCache,
    anim: Vec<Box<dyn Animator>>,
    zone: usize,
    is_loop: bool,
    path: FwPath,
    centre: Option<P2>,
    inside_sign: f64,
    waterfront: Option<Waterfront>,
    flat_y: f64,
    ground_y: f64,
    fa: Frame,
    fe: Frame,
    grid: Grid,
    group: NodeId,
    rng: Mulberry32,
    led: Led,
    fw: Freeway,
    chunks: Vec<GeoBuilder>,
    chunk_index: BTreeMap<(i64, i64), usize>,
    ground: ChunkedGeo,
    neon: GeoBuilder,
    lamp_spots: Vec<LampSpot>,
    park_trees: Vec<ParkTree>,
    aircraft: Vec<P3>,
    pad_lights: Vec<P3>,
    street_cells: BTreeSet<(i64, i64)>,
    parked: Vec<[f64; 4]>,
    sites: u32,
    glows: Vec<[f64; 7]>,
    street_segs: Vec<[f64; 4]>,
    roof_ads: Vec<AtlasQuad>,
    stats: Stats,
    extra_lamps: Vec<Spot>,
    thin: u32,
    /// Where the fade updater goes among the animators, and its list.
    fade: Option<(usize, Vec<Fade>)>,
}

impl<'w> Build<'w> {
    fn new(
        city: &City,
        t: &'w Track,
        tr: &'w Terrain,
        graph: &'w mut SceneGraph,
        textures: &'w mut TextureCache,
        path: FwPath,
    ) -> Build<'w> {
        let flat_y = tr
            .flat_y
            .get(city.zone)
            .copied()
            .flatten()
            .unwrap_or(tr.city_y);
        let (fa, fe) = if city.is_loop {
            (Frame::default(), Frame::default())
        } else {
            (t.frame(city.s_a), t.frame(t.length - 0.01))
        };
        // setupGrid
        let grid = if city.is_loop {
            // One grid for the whole ring; the freeway crosses it at every angle.
            let c = city.centre.expect("the loop's plan sets the centre");
            Grid::new(0.3, c[0], c[1])
        } else {
            let fa = t.frame(city.s_a);
            let fe = t.frame(t.length - 1.0);
            let ang = kernel::atan2(fe.z - fa.z, fe.x - fa.x);
            let o = t.frame(t.zones[city.zone].s0 + 1400.0);
            Grid::new(ang, o.x, o.z)
        };
        let group = graph.group("city");
        Build {
            track: t,
            terrain: tr,
            graph,
            textures,
            anim: Vec::new(),
            zone: city.zone,
            is_loop: city.is_loop,
            path,
            centre: city.centre,
            inside_sign: city.inside_sign,
            waterfront: city.waterfront,
            flat_y,
            ground_y: flat_y - 0.25,
            fa,
            fe,
            grid,
            group,
            rng: Mulberry32::new(4242),
            led: Led {
                is_loop: city.is_loop,
                dc: None,
                cores: Vec::new(),
            },
            fw: Freeway::new(),
            chunks: Vec::new(),
            chunk_index: BTreeMap::new(),
            ground: ChunkedGeo::new(f64::INFINITY, true),
            neon: GeoBuilder::new(true, false),
            lamp_spots: Vec::new(),
            park_trees: Vec::new(),
            aircraft: Vec::new(),
            pad_lights: Vec::new(),
            street_cells: BTreeSet::new(),
            parked: Vec::new(),
            sites: 0,
            glows: Vec::new(),
            street_segs: Vec::new(),
            roof_ads: Vec::new(),
            stats: Stats::default(),
            extra_lamps: Vec::new(),
            thin: 0,
            fade: None,
        }
    }

    /// The animators in the JS order, the fade updater in its place.
    fn finish(mut self) -> Vec<Box<dyn Animator>> {
        if let Some((at, list)) = self.fade.take() {
            let a = move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                let Some(cam) = u.camera else {
                    return;
                };
                let p = cam.position;
                for f in &list {
                    let dx = f.c.x - p[0];
                    let dy = f.c.y - p[1];
                    let dz = f.c.z - p[2];
                    let d = (dx * dx + dy * dy + dz * dz).sqrt() - f.r;
                    out.push(Edit {
                        target: Handle::Node(f.node),
                        change: Change::Visible(d < f.far),
                    });
                }
            };
            self.anim.insert(at, Box::new(a));
        }
        self.anim
    }

    fn rnd(&mut self) -> f64 {
        self.rng.next_f64()
    }

    fn run(&mut self) {
        let t = self.track;
        let sound_wall_spans = self.is_loop.then(|| self.loop_sound_walls());
        // Skyscraper cores (also decide where lamps are white LED).
        self.led.dc = (!self.is_loop).then(|| t.frame(t.finish_s - 440.0));
        self.led.cores = self.cores();
        {
            let Build {
                track,
                terrain,
                graph,
                textures,
                anim,
                path,
                grid,
                group,
                rng,
                led,
                fw,
                flat_y,
                zone,
                ..
            } = self;
            let tint = |u: f64| {
                let lf = path.frame(track, u);
                if led.at(track, terrain, lf.x, lf.z) {
                    [0.75, 0.86, 1.05]
                } else {
                    [1.0, 0.72, 0.4]
                }
            };
            let mut ctx = FwCtx {
                track,
                terrain,
                path,
                grid,
                city_y: *flat_y,
                zone: *zone,
                group: *group,
                graph,
                textures,
                animators: anim,
                rng,
                sound_wall_spans,
                lamp_tint: &tint,
            };
            fw.build(&mut ctx);
        }
        self.build_blocks();
        if self.is_loop {
            self.build_waterfront();
        }
        self.build_verge();
        self.build_lamps();
        self.build_trees();
        self.build_parked_cars();
        self.build_aircraft_lights();
        self.build_glows();
        self.build_traffic();
        self.build_sky_glow();
        // Meshes and points never move: their matrices are made once.
        self.freeze(self.group);
    }

    fn freeze(&mut self, id: NodeId) {
        let children = self.graph.get(id).children.clone();
        {
            let o = self.graph.get_mut(id);
            if matches!(
                o.ty,
                NodeType::Mesh | NodeType::InstancedMesh | NodeType::Points
            ) {
                o.matrix_auto_update = false;
                o.update_matrix();
            }
        }
        for c in children {
            self.freeze(c);
        }
    }

    fn cores(&self) -> Vec<Core> {
        let t = self.track;
        if !self.is_loop {
            return Vec::new();
        }
        t.tag("downtown")
            .iter()
            .map(|g| {
                let c = t.frame((g.s0 + g.s1) / 2.0);
                Core {
                    x: c.x,
                    z: c.z,
                    r: js::max(450.0, (g.s1 - g.s0) * 0.42),
                }
            })
            .collect()
    }

    fn loop_sound_walls(&self) -> Vec<[f64; 2]> {
        let t = self.track;
        let mut spans = Vec::new();
        let mut st: Option<f64> = None;
        let mut s = 0.0;
        while s <= t.length {
            let ok = district_at(t, s) == District::Residential;
            if ok && st.is_none() {
                st = Some(s);
            }
            if !ok && let Some(a) = st {
                if s - a > 150.0 {
                    spans.push([a, s]);
                }
                st = None;
            }
            s += 20.0;
        }
        if let Some(a) = st
            && t.length - a > 150.0
        {
            spans.push([a, t.length]);
        }
        spans
    }

    // ── Lookups ───────────────────────────────────────────────────

    /// Signed lateral offsets of (x,z) from every nearby bit of freeway.
    fn freeway_hits(&self, x: f64, z: f64) -> Vec<Hit> {
        let t = self.track;
        let mut out = Vec::new();
        // The far field is coarse (32 m) — only ask the track when plausibly close.
        if self.terrain.far(x, z).d < 175.0 {
            let r = t.distance_to_road(x, z, 120.0);
            if r.d < 1e8
                && let Some(s) = r.s
                && (self.is_loop || s >= t.zones[self.zone].s0 - 60.0)
            {
                out.push(Hit {
                    lat: r.lat,
                    y: f64::from(t.py[t.idx(s)]),
                    back: false,
                });
            }
        }
        if self.is_loop {
            return out;
        }
        let f = &self.fa;
        let e = &self.fe;
        let du = (x - f.x) * f.fx + (z - f.z) * f.fz;
        if du <= 0.0 && du >= -BACK_EXT - 30.0 {
            out.push(Hit {
                lat: (x - f.x) * f.rx + (z - f.z) * f.rz,
                y: f.y,
                back: true,
            });
        }
        let de = (x - e.x) * e.fx + (z - e.z) * e.fz;
        if de >= 0.0 && de <= FWD_EXT + 30.0 {
            out.push(Hit {
                lat: (x - e.x) * e.rx + (z - e.z) * e.rz,
                y: e.y,
                back: false,
            });
        }
        out
    }

    /// Would a building here crowd the freeway? (20 m+ setback past barriers.)
    fn build_blocked(&self, x: f64, z: f64) -> bool {
        self.freeway_hits(x, z).iter().any(|h| {
            if h.back {
                h.lat > -56.0 && h.lat < -4.0
            } else {
                h.lat > -54.0 && h.lat < 31.0
            }
        })
    }

    /// Would a street here run through the freeway at grade?
    fn street_blocked(&self, x: f64, z: f64) -> bool {
        for h in self.freeway_hits(x, z) {
            let lo = -37.0;
            let hi = if h.back { -6.0 } else { 15.5 };
            if h.lat > lo && h.lat < hi && h.y < self.flat_y + 3.5 {
                return true;
            }
            if h.lat > lo && h.lat < hi && (h.lat - (-10.95)).abs() < 1.5 {
                return true; // median piers
            }
        }
        false
    }

    /// Street lamps also keep out from under low viaduct ramps (loop).
    fn lamp_blocked(&self, x: f64, z: f64) -> bool {
        if self.street_blocked(x, z) {
            return true;
        }
        if !self.is_loop {
            return false;
        }
        self.freeway_hits(x, z)
            .iter()
            .any(|h| h.lat > -37.0 && h.lat < 15.5 && h.y < self.flat_y + 10.5)
    }

    fn in_city(&self, x: f64, z: f64) -> bool {
        let tr = self.terrain;
        if tr.zone_weights(x).w[self.zone] < 0.9 {
            return false;
        }
        let f = tr.far(x, z);
        if self.is_loop {
            return f.d < 950.0 || self.side_at(x, z) > 0.0;
        }
        if f.d > 1250.0 {
            return false;
        }
        if f.s < self.track.zones[self.zone].s0 - 30.0 && f.d < 600.0 {
            return false;
        }
        true
    }

    /// +1 inside the ring, -1 outside.
    fn side_at(&self, x: f64, z: f64) -> f64 {
        let fl = self.terrain.far_cell(far_cell(self.terrain, x, z)).1;
        (if fl >= 0.0 { 1.0 } else { -1.0 }) * js::or(self.inside_sign, 1.0)
    }

    fn flat(&self, x: f64, z: f64) -> bool {
        (self.terrain.height_at(x, z) - self.ground_y).abs() < 0.3
    }

    fn w(&self, u: f64, v: f64) -> P2 {
        self.grid.to_world(u, v)
    }

    fn chunk_of(&mut self, x: f64, z: f64) -> usize {
        let ch = if self.is_loop { 2000.0 } else { 600.0 };
        let key = chunk_key(x, z, ch);
        match self.chunk_index.get(&key) {
            Some(&i) => i,
            None => {
                self.chunks.push(GeoBuilder::new(false, true));
                self.chunk_index.insert(key, self.chunks.len() - 1);
                self.chunks.len() - 1
            }
        }
    }

    /// Up-facing quad in grid space.
    fn quad_uv(&mut self, u0: f64, v0: f64, u1: f64, v1: f64, y: f64, col: P3) {
        let p3 = |p: P2| [p[0], y, p[1]];
        let a = p3(self.w(u0, v0));
        let b = p3(self.w(u1, v0));
        let c = p3(self.w(u1, v1));
        let d = p3(self.w(u0, v1));
        let n = normal_y(&a, &b, &c);
        if n >= 0.0 {
            self.ground.quad(a, b, c, d, Some(col));
        } else {
            self.ground.quad(a, d, c, b, Some(col));
        }
    }

    fn pad(&mut self, u0: f64, v0: f64, u1: f64, v1: f64, col: P3, h: f64) {
        let g0 = self.ground_y;
        self.quad_uv(u0, v0, u1, v1, g0 + h, col);
        // Curb faces.
        let pts: Vec<P2> = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]
            .iter()
            .map(|&[u, v]| self.w(u, v))
            .collect();
        self.ground.prism(
            &pts,
            g0 - 0.2,
            g0 + h,
            &PrismOpts {
                roof: Some(false),
                color: Some([col[0] * 0.8, col[1] * 0.8, col[2] * 0.8]),
                ..PrismOpts::default()
            },
        );
    }

    fn fadeable(&mut self, node: NodeId, far: f64) {
        if self.fade.is_none() {
            self.fade = Some((self.anim.len(), Vec::new()));
        }
        let o = self.graph.get(node);
        let sph = if o.ty == NodeType::InstancedMesh {
            self.graph.compute_instance_bounding_sphere(node);
            self.graph
                .get(node)
                .instances
                .as_ref()
                .and_then(|i| i.bounding_sphere)
                .expect("computed")
        } else {
            let g = o.geometry.expect("a mesh");
            let geo = self.graph.geometry_mut(g);
            if geo.bounding_sphere.is_none() {
                geo.compute_bounding_sphere();
            }
            geo.bounding_sphere.expect("computed")
        };
        if let Some((_, list)) = &mut self.fade {
            list.push(Fade {
                node,
                c: sph.center,
                r: sph.radius,
                far,
            });
        }
    }

    // Level 1: one InstancedMesh as before. Loop: split into spatial chunks
    // (so frustum culling works) that also switch off beyond `far` metres.
    fn emit_instanced(
        &mut self,
        geo: BufferGeometry,
        mat: MaterialId,
        matrices: &[Matrix4],
        cast: bool,
        receive: bool,
        far: f64,
        colors: Option<&[Color]>,
    ) -> Vec<NodeId> {
        let mut out = Vec::new();
        if matrices.is_empty() {
            return out;
        }
        let g = self.graph.add_geometry(geo);
        if !self.is_loop {
            let im = instanced(self.graph, g, mat, matrices, cast, receive);
            if let Some(cols) = colors {
                let inst = self
                    .graph
                    .get_mut(im)
                    .instances
                    .as_mut()
                    .expect("instanced");
                for (i, c) in cols.iter().enumerate() {
                    inst.set_color_at(i, *c);
                }
            }
            self.graph.add(self.group, im);
            out.push(im);
            return out;
        }
        const CH: f64 = 2000.0;
        let mut keys: Vec<(i64, i64)> = Vec::new();
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for (i, m) in matrices.iter().enumerate() {
            let e = &m.elements;
            let key = chunk_key(e[12], e[14], CH);
            match keys.iter().position(|k| *k == key) {
                Some(k) => groups[k].push(i),
                None => {
                    keys.push(key);
                    groups.push(vec![i]);
                }
            }
        }
        for idx in groups {
            let ms: Vec<Matrix4> = idx.iter().map(|&i| matrices[i]).collect();
            let im = instanced(self.graph, g, mat, &ms, cast, receive);
            if let Some(cols) = colors {
                let inst = self
                    .graph
                    .get_mut(im)
                    .instances
                    .as_mut()
                    .expect("instanced");
                for (k, &i) in idx.iter().enumerate() {
                    inst.set_color_at(k, cols[i]);
                }
            }
            self.graph.add(self.group, im);
            self.fadeable(im, far);
            out.push(im);
        }
        out
    }

    // ── Blocks, lots and buildings ────────────────────────────────
    fn build_blocks(&mut self) {
        let t = self.track;
        let g0 = self.ground_y;
        // Skyscraper cores: around the finish on Level 1, round each 'downtown'
        // stretch on the loop.
        let dc = (!self.is_loop).then(|| t.frame(t.finish_s - 440.0));
        self.led.dc = dc;
        self.led.cores = self.cores();
        // Grid extent from the freeway path.
        let (mut umin, mut umax, mut vmin, mut vmax) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        let mut u = self.path.u0;
        while u <= self.path.u1 {
            let f = self.path.frame(t, u);
            let gu = self.grid.to_u(f.x, f.z);
            let gv = self.grid.to_v(f.x, f.z);
            umin = js::min(umin, gu);
            umax = js::max(umax, gu);
            vmin = js::min(vmin, gv);
            vmax = js::max(vmax, gv);
            u += 40.0;
        }
        let i0 = ((umin - 1300.0) / PU).floor() as i64;
        let i1 = ((umax + 1300.0) / PU).ceil() as i64;
        let j0 = ((vmin - 1400.0) / PV).floor() as i64;
        let j1 = ((vmax + 1400.0) / PV).ceil() as i64;

        let atlas = city_facade_atlas(self.textures);
        let map = self.graph.cached_texture(&atlas.map, Layer::Main, "");
        let emissive = self.graph.cached_texture(&atlas.emissive, Layer::Main, "");
        let mask = self.graph.cached_texture(&atlas.mask, Layer::Main, "");
        let mat = self.graph.add_material(patch_city_material(
            Material::standard()
                .set("map", map)
                .set("emissiveMap", emissive)
                .set("emissive", 0xffffff)
                .set("emissiveIntensity", 0.1)
                .set("roughness", 0.72)
                .set("metalness", 0.15),
            mask,
            g0 + 0.15,
        ));
        self.graph
            .add_night(Some(mat), "emissiveIntensity", 0.06, 1.0);

        // Level 1: one ground mesh. Loop: the ring is ~8 × 6 km, so chunk it.
        self.ground = ChunkedGeo::new(if self.is_loop { 2000.0 } else { f64::INFINITY }, true);

        for i in i0..=i1 {
            for j in j0..=j1 {
                let (fi, fj) = (i as f64, j as f64);
                let cu = (fi + 0.5) * PU;
                let cv = (fj + 0.5) * PV;
                let cw = self.w(cu, cv);
                if !self.in_city(cw[0], cw[1]) {
                    continue;
                }
                let mut district = District::Residential;
                if self.is_loop {
                    let far_d = self.terrain.far(cw[0], cw[1]).d;
                    let inside = self.side_at(cw[0], cw[1]) > 0.0;
                    let cell_s = nearest_s(self.terrain, cw[0], cw[1]);
                    district = if far_d > 700.0 && inside {
                        District::Interior
                    } else {
                        district_at(t, cell_s)
                    };
                    // The promenade and basin own the waterfront's outer side.
                    if district == District::Waterfront && !inside && far_d < 430.0 {
                        continue;
                    }
                }
                let corners = [
                    [fi * PU, fj * PV],
                    [(fi + 1.0) * PU, fj * PV],
                    [(fi + 1.0) * PU, (fj + 1.0) * PV],
                    [fi * PU, (fj + 1.0) * PV],
                ];
                let mut pts: Vec<P2> = corners.iter().map(|&[u, v]| self.w(u, v)).collect();
                pts.push(cw);
                if !pts.iter().all(|p| self.flat(p[0], p[1])) {
                    continue;
                }
                // Streets: the whole cell in asphalt unless the freeway runs through.
                let street_ok = pts.iter().all(|p| !self.street_blocked(p[0], p[1]))
                    && [[0.25, 0.5], [0.75, 0.5], [0.5, 0.25], [0.5, 0.75]]
                        .iter()
                        .all(|&[a, b]| {
                            let p = self.w((fi + a) * PU, (fj + b) * PV);
                            !self.street_blocked(p[0], p[1])
                        });
                let bu0 = fi * PU + STREET / 2.0;
                let bu1 = (fi + 1.0) * PU - STREET / 2.0;
                let bv0 = fj * PV + STREET / 2.0;
                let bv1 = (fj + 1.0) * PV - STREET / 2.0;
                if street_ok {
                    self.street_cells.insert((i, j));
                    self.quad_uv(
                        fi * PU,
                        fj * PV,
                        (fi + 1.0) * PU,
                        (fj + 1.0) * PV,
                        g0 + 0.06,
                        ASPH,
                    );
                    // Lamps on this cell's +V and +U street edges.
                    for a in [0.28, 0.72] {
                        self.lamp_spots.push(LampSpot {
                            u: lerp(bu0, bu1, a),
                            v: bv1 + 1.2,
                            du: 0.0,
                            dv: 1.0,
                            h: None,
                            arm: None,
                        });
                    }
                    self.lamp_spots.push(LampSpot {
                        u: bu1 + 1.2,
                        v: lerp(bv0, bv1, 0.5),
                        du: 1.0,
                        dv: 0.0,
                        h: None,
                        arm: None,
                    });
                    // Street centre lines on the same two edges, for the traffic lights.
                    self.street_segs.push([
                        fi * PU,
                        (fj + 1.0) * PV,
                        (fi + 1.0) * PU,
                        (fj + 1.0) * PV,
                    ]);
                    self.street_segs.push([
                        (fi + 1.0) * PU,
                        fj * PV,
                        (fi + 1.0) * PU,
                        (fj + 1.0) * PV,
                    ]);
                }
                let mu = (bu0 + bu1) / 2.0;
                let mv = (bv0 + bv1) / 2.0;
                let block_pts = [
                    [bu0, bv0],
                    [bu1, bv0],
                    [bu1, bv1],
                    [bu0, bv1],
                    [mu, mv],
                    [mu, bv0],
                    [mu, bv1],
                    [bu0, mv],
                    [bu1, mv],
                ];
                let block_clear = block_pts.iter().all(|&[u, v]| {
                    let p = self.w(u, v);
                    !self.build_blocked(p[0], p[1])
                });
                if self.is_loop {
                    self.stats.blocks += 1;
                    self.loop_block(Block {
                        bu0,
                        bu1,
                        bv0,
                        bv1,
                        cw,
                        district,
                        block_clear,
                    });
                    continue;
                }
                let dc = dc.expect("the downtown core");
                let r = kernel::hypot(cw[0] - dc.x, cw[1] - dc.z);
                let core = kernel::exp(-kernel::pow(r / 640.0, 2.0));
                let ring = kernel::exp(-kernel::pow(r / 1500.0, 2.0));
                let roll = self.rnd();
                self.stats.blocks += 1;
                if block_clear && roll < 0.05 + core * 0.05 {
                    // Park (or a plaza downtown).
                    let plaza = core > 0.5;
                    self.make_park(bu0, bv0, bu1, bv1, plaza, false, 26, 0.7);
                    continue;
                }
                let tower = core > 0.25 && roll < 0.35 + core * 0.55;
                let lots: &[[f64; 4]] = if tower {
                    if self.rnd() < 0.55 { &Q1 } else { &Q2 }
                } else if ring > 0.55 || self.rnd() < 0.4 {
                    &Q4
                } else {
                    &Q6
                };
                let bu = bu1 - bu0;
                let bv = bv1 - bv0;
                if block_clear {
                    self.pad(bu0, bv0, bu1, bv1, WALK, 0.15);
                }
                for &[a0, b0, a1, b1] in lots {
                    let lu0 = bu0 + bu * a0 + 2.5;
                    let lu1 = bu0 + bu * a1 - 2.5;
                    let lv0 = bv0 + bv * b0 + 2.5;
                    let lv1 = bv0 + bv * b1 - 2.5;
                    if !block_clear {
                        if !self.lot_clear(lu0, lv0, lu1, lv1) {
                            self.parking_lot(lu0, lv0, lu1, lv1);
                            continue;
                        }
                        self.pad(lu0 - 2.5, lv0 - 2.5, lu1 + 2.5, lv1 + 2.5, WALK, 0.15);
                    }
                    if self.rnd() < 0.06 && !tower {
                        continue; // empty lot
                    }
                    // Now and then a tower still going up, with its crane.
                    if tower && self.sites < 5 && self.rnd() < 0.07 {
                        self.site(lu0, lv0, lu1, lv1, core);
                        continue;
                    }
                    self.building(
                        lu0,
                        lv0,
                        lu1,
                        lv1,
                        BuildingOpts {
                            tower,
                            core,
                            ring,
                            ..BuildingOpts::default()
                        },
                    );
                }
            }
        }

        // Emit building chunks.
        for b in std::mem::take(&mut self.chunks) {
            if b.is_empty() {
                continue;
            }
            let g = self.graph.add_geometry(b.build());
            let m = static_mesh(self.graph, g, mat, &StaticOpts::default());
            self.graph.add(self.group, m);
        }
        let gm = self.graph.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.93)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -2.0)
                .set("polygonOffsetUnits", -4.0),
        );
        let ground = std::mem::replace(&mut self.ground, ChunkedGeo::new(1.0, true));
        for b in ground.map.iter().filter(|b| !b.is_empty()) {
            let g = self.graph.add_geometry(b.build());
            let m = static_mesh(self.graph, g, gm, &StaticOpts::default());
            self.graph.add(self.group, m);
            if self.is_loop {
                self.fadeable(m, 2600.0);
            }
        }
        if let Some((g, m)) = atlas_quads(self.graph, &self.roof_ads, 1024.0, 384.0, 3) {
            let mat = self
                .graph
                .add_material(m.set("roughness", 0.6).set("emissiveIntensity", 0.8));
            self.graph
                .add_night(Some(mat), "emissiveIntensity", 0.25, 1.2);
            let g = self.graph.add_geometry(g);
            let n = static_mesh(self.graph, g, mat, &StaticOpts::default());
            self.graph.add(self.group, n);
        }
        let nm = self
            .graph
            .add_material(Material::basic().set("vertexColors", true));
        self.anim
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                let v = 0.3 + 0.7 * smoothstep(0.2, 0.8, u.night);
                out.push(Edit {
                    target: Handle::Material(nm),
                    change: Change::Color {
                        prop: "color",
                        rgb: [v, v, v],
                    },
                });
            }));
        if !self.neon.is_empty() {
            let g = self.graph.add_geometry(self.neon.build());
            let m = static_mesh(
                self.graph,
                g,
                nm,
                &StaticOpts {
                    receive: false,
                    ..StaticOpts::default()
                },
            );
            self.graph.add(self.group, m);
        }
    }

    /// Every corner and the centre of a lot clear of the freeway.
    fn lot_clear(&self, lu0: f64, lv0: f64, lu1: f64, lv1: f64) -> bool {
        [
            [lu0, lv0],
            [lu1, lv0],
            [lu1, lv1],
            [lu0, lv1],
            [(lu0 + lu1) / 2.0, (lv0 + lv1) / 2.0],
        ]
        .iter()
        .all(|&[u, v]| {
            let p = self.w(u, v);
            !self.build_blocked(p[0], p[1])
        })
    }

    fn tree(&mut self, u: f64, v: f64, sc: f64) {
        let p = self.w(u, v);
        let s = sc * (0.8 + self.rnd() * 0.6);
        self.park_trees.push(ParkTree {
            x: p[0],
            y: self.ground_y + 0.2,
            z: p[1],
            s,
        });
    }

    /// A park (or a plaza): Level 1's inline park (`n` 26, trees 0.8 + r ×
    /// 0.7) and the loop's `makePark` (24, `tree()`), with its lamps.
    fn make_park(
        &mut self,
        bu0: f64,
        bv0: f64,
        bu1: f64,
        bv1: f64,
        plaza: bool,
        lamps: bool,
        n_park: usize,
        spread: f64,
    ) {
        let g0 = self.ground_y;
        self.pad(bu0, bv0, bu1, bv1, if plaza { PLAZA } else { GRASS }, 0.2);
        let mu = (bu0 + bu1) / 2.0;
        let mv = (bv0 + bv1) / 2.0;
        if !plaza {
            self.quad_uv(mu - 2.0, bv0, mu + 2.0, bv1, g0 + 0.23, PLAZA);
            self.quad_uv(bu0, mv - 2.0, bu1, mv + 2.0, g0 + 0.23, PLAZA);
        }
        let n = if plaza { 8 } else { n_park };
        for _ in 0..n {
            let u = lerp(bu0 + 4.0, bu1 - 4.0, self.rnd());
            let v = lerp(bv0 + 4.0, bv1 - 4.0, self.rnd());
            if !plaza && ((u - mu).abs() < 4.0 || (v - mv).abs() < 4.0) {
                continue;
            }
            if self.is_loop {
                self.tree(u, v, 1.0);
            } else {
                let p = self.w(u, v);
                let s = 0.8 + self.rnd() * spread;
                self.park_trees.push(ParkTree {
                    x: p[0],
                    y: g0 + 0.2,
                    z: p[1],
                    s,
                });
            }
        }
        if lamps {
            for [du, dv] in [[-20.0, 3.0], [20.0, -3.0], [3.0, -18.0], [-3.0, 18.0]] {
                let p = self.w(mu + du, mv + dv);
                self.extra_lamps.push(Spot {
                    x: p[0],
                    y: g0 + 0.2,
                    z: p[1],
                    dx: 1.0,
                    dz: 0.0,
                    h: 4.2,
                    arm: Some(0.0),
                    led: false,
                });
            }
        }
        if plaza {
            let p = self.w(mu, mv);
            // Fountain basin + glowing jet.
            let bi = self.chunk_of(p[0], p[1]);
            self.chunks[bi].prism(
                &circle(p[0], p[1], 7.0, 12),
                g0,
                g0 + 0.8,
                &PrismOpts {
                    cell: Some(GLASS_CELL as f64),
                    tile_w: Some(8.0),
                    tile_h: Some(8.0),
                    roof_cell: Some(ROOF_CELL as f64),
                    ..PrismOpts::default()
                },
            );
            self.neon.prism(
                &circle(p[0], p[1], 0.6, 8),
                g0 + 0.8,
                g0 + 4.5,
                &PrismOpts {
                    color: Some([0.6, 1.4, 2.2]),
                    roof_color: Some([0.9, 1.8, 2.6]),
                    ..PrismOpts::default()
                },
            );
        }
    }

    // ── Loop districts ────────────────────────────────────────────

    /// Run fn over lots; lots that crowd the freeway are dropped (with their
    /// own pads when the block as a whole isn't clear).
    fn lots_do(
        &mut self,
        o: &Block,
        lots: &[[f64; 4]],
        pad_col: P3,
        mut f: impl FnMut(&mut Self, f64, f64, f64, f64),
    ) {
        let (bu0, bu1, bv0, bv1) = (o.bu0, o.bu1, o.bv0, o.bv1);
        let bu = bu1 - bu0;
        let bv = bv1 - bv0;
        if o.block_clear {
            self.pad(bu0, bv0, bu1, bv1, pad_col, 0.15);
        }
        for &[a0, b0, a1, b1] in lots {
            let lu0 = bu0 + bu * a0 + 2.5;
            let lu1 = bu0 + bu * a1 - 2.5;
            let lv0 = bv0 + bv * b0 + 2.5;
            let lv1 = bv0 + bv * b1 - 2.5;
            if !o.block_clear {
                if !self.lot_clear(lu0, lv0, lu1, lv1) {
                    self.parking_lot(lu0, lv0, lu1, lv1);
                    continue;
                }
                self.pad(lu0 - 2.5, lv0 - 2.5, lu1 + 2.5, lv1 + 2.5, pad_col, 0.15);
            }
            f(self, lu0, lv0, lu1, lv1);
        }
    }

    fn loop_block(&mut self, o: Block) {
        let Block {
            bu0,
            bu1,
            bv0,
            bv1,
            cw,
            district,
            block_clear,
        } = o;
        let mut core: f64 = 0.0;
        for c in self.led.cores.clone() {
            core = js::max(
                core,
                kernel::exp(
                    -((kernel::pow(cw[0] - c.x, 2.0) + kernel::pow(cw[1] - c.z, 2.0))
                        / (c.r * c.r)),
                ),
            );
        }
        let centre = self.centre.expect("the ring's centre");
        let dc = kernel::hypot(cw[0] - centre[0], cw[1] - centre[1]);
        let roll = self.rnd();

        // Meridian Park fills the middle of the ring.
        if district == District::Interior && dc < 330.0 {
            if block_clear {
                self.make_park(bu0, bv0, bu1, bv1, dc < 60.0, true, 24, 0.6);
            }
            return;
        }
        if district == District::Downtown || (district == District::Interior && core > 0.3) {
            if block_clear && roll < 0.05 + core * 0.05 {
                self.make_park(bu0, bv0, bu1, bv1, core > 0.5, false, 24, 0.6);
                return;
            }
            let tower = core > 0.2 && roll < 0.35 + core * 0.55;
            let lots: &[[f64; 4]] = if tower {
                if self.rnd() < 0.55 { &Q1 } else { &Q2 }
            } else {
                &Q4
            };
            self.lots_do(&o, lots, WALK, |s, a, b, c, d| {
                if !tower && s.rnd() < 0.06 {
                    return;
                }
                if tower && s.sites < 8 && s.rnd() < 0.06 {
                    s.site(a, b, c, d, core);
                    return;
                }
                s.building(
                    a,
                    b,
                    c,
                    d,
                    BuildingOpts {
                        tower,
                        core: js::max(core, 0.35),
                        ring: 1.0,
                        ..BuildingOpts::default()
                    },
                );
            });
            return;
        }
        if district == District::Industrial {
            if roll < 0.2 {
                self.container_yard(&o);
                return;
            }
            let lots: &[[f64; 4]] = if self.rnd() < 0.5 { &Q1 } else { &Q2 };
            self.lots_do(&o, lots, YARD, |s, a, b, c, d| s.warehouse(a, b, c, d));
            return;
        }
        if district == District::Midrise {
            if block_clear && roll < 0.06 {
                self.make_park(bu0, bv0, bu1, bv1, false, true, 24, 0.6);
                return;
            }
            self.lots_do(&o, &Q4, WALK, |s, a, b, c, d| {
                if s.rnd() < 0.05 {
                    return;
                }
                let brick = s.rnd() < 0.25;
                s.building(
                    a,
                    b,
                    c,
                    d,
                    BuildingOpts {
                        ring: 1.0,
                        cell: brick.then_some(BRICK_CELL),
                        floors: Some(if brick { [5.0, 10.0] } else { [7.0, 22.0] }),
                        ..BuildingOpts::default()
                    },
                );
            });
            return;
        }
        // Residential (and the waterfront's inner side / far shore, and the
        // rest of the interior): brick walk-ups, low-rise and parks.
        let p = if district == District::Waterfront {
            0.14
        } else {
            0.08
        };
        if block_clear && roll < p {
            self.make_park(bu0, bv0, bu1, bv1, false, true, 24, 0.6);
            return;
        }
        let lots: &[[f64; 4]] = if self.rnd() < 0.5 { &Q6 } else { &Q4 };
        self.lots_do(&o, lots, WALK, |s, a, b, c, d| {
            if s.rnd() < 0.05 {
                return;
            }
            let k = s.rnd();
            let opts = if k < 0.55 {
                BuildingOpts {
                    cell: Some(BRICK_CELL),
                    floors: Some([3.0, 7.0]),
                    ..BuildingOpts::default()
                }
            } else if k < 0.85 {
                BuildingOpts::default()
            } else {
                BuildingOpts {
                    ring: 1.0,
                    floors: Some([6.0, 12.0]),
                    ..BuildingOpts::default()
                }
            };
            s.building(a, b, c, d, opts);
        });
        // streetTrees
        if !block_clear {
            return;
        }
        if self.rnd() < 0.4 {
            return;
        }
        let mut u = bu0 + 6.0;
        while u < bu1 - 4.0 {
            self.tree(u, bv1 - 1.6, 0.8);
            u += 18.0;
        }
    }

    fn yaw(&self) -> f64 {
        kernel::atan2(self.grid.u_axis[1], self.grid.u_axis[0])
    }

    // Warehouse with a flat roof, maybe a chimney or tanks beside it.
    fn warehouse(&mut self, lu0: f64, lv0: f64, lu1: f64, lv1: f64) {
        let g0 = self.ground_y + 0.15;
        let cu = (lu0 + lu1) / 2.0;
        let cv = (lv0 + lv1) / 2.0;
        let cw = self.w(cu, cv);
        let bi = self.chunk_of(cw[0], cw[1]);
        let mut b = std::mem::take(&mut self.chunks[bi]);
        let wu = (lu1 - lu0) - 3.0 - self.rnd() * 6.0;
        let wv = (lv1 - lv0) - 3.0 - self.rnd() * 6.0;
        let h = 7.0 + self.rnd() * 8.0;
        let rect: Vec<P2> = [
            [cu - wu / 2.0, cv - wv / 2.0],
            [cu + wu / 2.0, cv - wv / 2.0],
            [cu + wu / 2.0, cv + wv / 2.0],
            [cu - wu / 2.0, cv + wv / 2.0],
        ]
        .iter()
        .map(|&[u, v]| self.w(u, v))
        .collect();
        let u_off = (self.rnd() * 4.0).floor() / 4.0;
        b.prism(
            &rect,
            g0 - 1.0,
            g0 + h,
            &PrismOpts {
                cell: Some(WAREHOUSE_CELL as f64),
                tile_w: Some(CELL_TILE[WAREHOUSE_CELL][0]),
                tile_h: Some(CELL_TILE[WAREHOUSE_CELL][1]),
                v_ref: Some(g0),
                u_off: Some(u_off),
                v_off: Some(0.0),
                roof_cell: Some(ROOF_CELL as f64),
                roof_tile: Some(16.0),
                ..PrismOpts::default()
            },
        );
        self.stats.buildings += 1;
        let yaw = self.yaw();
        let mut k = 0.0;
        loop {
            let lim = 2.0 + (self.rnd() * 3.0).floor();
            if k >= lim {
                break;
            }
            let pu = cu + (self.rnd() - 0.5) * wu * 0.7;
            let pv = cv + (self.rnd() - 0.5) * wv * 0.7;
            let p = self.w(pu, pv);
            let sx = 2.0 + self.rnd() * 4.0;
            let sy = 1.2 + self.rnd() * 2.0;
            let sz = 2.0 + self.rnd() * 3.0;
            b.box_(
                p[0],
                g0 + h - 0.1,
                p[1],
                sx,
                sy,
                sz,
                yaw,
                &cell_opts(ROOF_CELL, 6.0),
            );
            k += 1.0;
        }
        let extra = self.rnd();
        if extra < 0.07 {
            // Chimney stack with an aircraft light.
            let p = self.w(lu1 - 3.0, lv1 - 3.0);
            let hh = 35.0 + self.rnd() * 30.0;
            b.prism(
                &circle(p[0], p[1], 1.6, 10),
                g0 - 1.0,
                g0 + hh,
                &cell_opts(ROOF_CELL, 6.0),
            );
            self.neon.prism(
                &circle(p[0], p[1], 1.65, 10),
                g0 + hh - 5.0,
                g0 + hh - 4.2,
                &colored([3.2, 0.3, 0.2], false),
            );
            self.aircraft.push([p[0], g0 + hh + 0.6, p[1]]);
        } else if extra < 0.3 {
            // Storage tanks.
            for k in [0.0, 1.0] {
                let p = self.w(lu0 + 7.0 + k * 13.0, lv1 - 7.0);
                let r = 5.0 + self.rnd() * 1.5;
                let top = g0 + 8.0 + self.rnd() * 6.0;
                b.prism(
                    &circle(p[0], p[1], r, 14),
                    g0 - 1.0,
                    top,
                    &PrismOpts {
                        cell: Some(WAREHOUSE_CELL as f64),
                        tile_w: Some(24.0),
                        tile_h: Some(48.0),
                        v_ref: Some(g0 + 20.0),
                        roof_cell: Some(ROOF_CELL as f64),
                        ..PrismOpts::default()
                    },
                );
            }
        }
        self.chunks[bi] = b;
    }

    // Stacked shipping containers under floodlights.
    fn container_yard(&mut self, o: &Block) {
        if !o.block_clear {
            return;
        }
        let g0 = self.ground_y;
        self.pad(o.bu0, o.bv0, o.bu1, o.bv1, [0.1, 0.1, 0.11], 0.12);
        let yaw = self.yaw();
        const COLS: [[f64; 3]; 7] = [
            [0.45, 0.12, 0.08],
            [0.08, 0.2, 0.42],
            [0.1, 0.32, 0.16],
            [0.6, 0.32, 0.06],
            [0.35, 0.36, 0.38],
            [0.5, 0.45, 0.1],
            [0.12, 0.3, 0.36],
        ];
        // Rows of stacks in pairs, with aisles between.
        let mut v = o.bv0 + 6.0;
        while v < o.bv1 - 4.0 {
            let mut u = o.bu0 + 8.0;
            while u < o.bu1 - 8.0 {
                for dv in [0.0, 2.55] {
                    if self.rnd() < 0.15 {
                        continue;
                    }
                    let stack = 1.0 + (self.rnd() * 3.0).floor();
                    let p = self.w(u + 6.1, v + dv);
                    let c = COLS[(self.rnd() * COLS.len() as f64).floor() as usize];
                    self.ground.box_(
                        p[0],
                        g0 + 0.12,
                        p[1],
                        12.2,
                        2.6 * stack - 0.05,
                        2.45,
                        yaw,
                        &PrismOpts {
                            color: Some(c),
                            roof_color: Some(scale3(c, 1.15)),
                            ..PrismOpts::default()
                        },
                    );
                }
                u += 13.0;
            }
            v += 8.5;
        }
        for [a, b] in [[0.1, 0.1], [0.9, 0.9], [0.1, 0.9], [0.9, 0.1]] {
            self.lamp_spots.push(LampSpot {
                u: lerp(o.bu0, o.bu1, a),
                v: lerp(o.bv0, o.bv1, b),
                du: if a < 0.5 { 1.0 } else { -1.0 },
                dv: 0.0,
                h: Some(14.0),
                arm: None,
            });
        }
    }

    // Promenade, quay, water and the ferris wheel along the waterfront.
    fn build_waterfront(&mut self) {
        let Some(wf) = self.waterfront else {
            return;
        };
        let t = self.track;
        let g0 = self.ground_y;
        let out = -self.inside_sign;
        let path = self.path.clone();
        let ranges = [[wf.s0 - 150.0, wf.s1 + 150.0]];
        let lats = |a: f64, b: f64| {
            if out < 0.0 {
                (out * b, out * a)
            } else {
                (out * a, out * b)
            }
        };
        // Water: dark and glossy; the basin's banks hide its edges.
        let (w0, w1) = lats(80.0, 385.0);
        let water = self.graph.add_material(
            Material::standard()
                .set("color", 0x0a1726)
                .set("roughness", 0.06)
                .set("metalness", 0.85)
                .set("envMapIntensity", 1.4),
        );
        let g = sweep(
            t,
            &path,
            &ranges,
            &[prof(w0, |_, _| g0 - 1.6), prof(w1, |_, _| g0 - 1.6)],
            &step(6.0),
        );
        self.add_static(g, water, false, true, 0.0);
        // Reflections: long soft streaks running across the water from the
        // lamps on the quay and the lit far shore.
        {
            let mut pos: Vec<f64> = Vec::new();
            let mut uv: Vec<f64> = Vec::new();
            let mut col: Vec<f64> = Vec::new();
            let mut streak = |s: f64, lat0: f64, len: f64, w: f64, c: P3| {
                let f = path.frame(t, s);
                let a = [f.x + f.rx * lat0, f.z + f.rz * lat0];
                let d = [f.rx * out * len, f.rz * out * len];
                let sw = [f.fx * w / 2.0, f.fz * w / 2.0];
                let y = g0 - 1.55;
                let p = [
                    [a[0] - sw[0], a[1] - sw[1]],
                    [a[0] + sw[0], a[1] + sw[1]],
                    [a[0] + sw[0] + d[0], a[1] + sw[1] + d[1]],
                    [a[0] - sw[0] + d[0], a[1] - sw[1] + d[1]],
                ];
                let uu = [[0.0, 0.5], [1.0, 0.5], [1.0, 1.0], [0.0, 1.0]];
                // Two triangles facing up whichever way `out` points.
                let order = if out < 0.0 {
                    [0, 1, 2, 0, 2, 3]
                } else {
                    [0, 2, 1, 0, 3, 2]
                };
                for k in order {
                    pos.extend_from_slice(&[p[k][0], y, p[k][1]]);
                    uv.extend_from_slice(&uu[k]);
                    col.extend_from_slice(&c);
                }
            };
            let mut s = wf.s0 - 30.0;
            while s < wf.s1 + 30.0 {
                let len = 70.0 + self.rng.next_f64() * 40.0;
                streak(s + 1.0, out * 80.0, len, 3.0, [0.5, 0.36, 0.2]);
                s += 24.0;
            }
            let tints = [
                [0.5, 0.4, 0.25],
                [0.35, 0.4, 0.5],
                [0.55, 0.15, 0.45],
                [0.15, 0.45, 0.5],
            ];
            let mut s = wf.s0 - 60.0;
            while s < wf.s1 + 60.0 {
                if self.rng.next_f64() < 0.35 {
                    s += 9.0;
                    continue;
                }
                let lat0 = out * (360.0 - self.rng.next_f64() * 20.0);
                let len = -(60.0 + self.rng.next_f64() * 110.0);
                let w = 1.5 + self.rng.next_f64() * 3.0;
                let a = self.rng.next_f64();
                let n = if self.rng.next_f64() < 0.8 { 2.0 } else { 4.0 };
                let tint = tints[(a * n).floor() as usize];
                streak(s, lat0, len, w, tint);
                s += 9.0;
            }
            let mut g = BufferGeometry::new();
            g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
            g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
            g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
            g.compute_bounding_sphere();
            let glow = self
                .graph
                .cached_texture(&self.textures.glow_texture(), Layer::Main, "");
            let m = self.graph.add_material(
                Material::basic()
                    .set("map", glow)
                    .set("vertexColors", true)
                    .set("transparent", true)
                    .set("depthWrite", false)
                    .set("blending", three::ADDITIVE_BLENDING as f64),
            );
            self.add_static(g, m, false, true, 2.0);
        }
        // Promenade deck and quay wall.
        let pr = [[wf.s0 - 40.0, wf.s1 + 40.0]];
        let (p0, p1) = lats(36.0, 79.0);
        let deck = self.graph.add_material(
            Material::standard()
                .set("color", 0x8f877a)
                .set("roughness", 0.85),
        );
        let g = sweep(
            t,
            &path,
            &pr,
            &[prof(p0, |_, _| g0 + 0.12), prof(p1, |_, _| g0 + 0.12)],
            &SweepOpts {
                step: 5.0,
                u_s: 3.0,
                v_s: 3.0,
                ..SweepOpts::default()
            },
        );
        self.add_static(g, deck, false, true, 0.0);
        let quay: Vec<Prof> = if out < 0.0 {
            vec![prof(-79.0, |_, _| g0 - 2.6), prof(-79.0, |_, _| g0 + 0.12)]
        } else {
            vec![prof(79.0, |_, _| g0 + 0.12), prof(79.0, |_, _| g0 - 2.6)]
        };
        let conc = self.graph.add_material(
            Material::standard()
                .set("color", 0x6f6a62)
                .set("roughness", 0.9),
        );
        let g = sweep(
            t,
            &path,
            &pr,
            &quay,
            &SweepOpts {
                step: 5.0,
                uv: SweepUv::Wall,
                ..SweepOpts::default()
            },
        );
        self.add_static(g, conc, false, true, 0.0);
        let rail: Vec<Prof> = if out < 0.0 {
            vec![prof(-78.6, |_, _| g0 + 0.9), prof(-78.6, |_, _| g0 + 1.05)]
        } else {
            vec![prof(78.6, |_, _| g0 + 1.05), prof(78.6, |_, _| g0 + 0.9)]
        };
        let rail_mat = self.graph.add_material(
            Material::standard()
                .set("color", 0xb8bcc2)
                .set("metalness", 0.7)
                .set("roughness", 0.4)
                .set("side", three::DOUBLE_SIDE as f64),
        );
        let g = sweep(t, &path, &pr, &rail, &step(5.0));
        self.add_static(g, rail_mat, false, true, 0.0);
        // Lamps and trees along the promenade.
        let mut s = wf.s0 - 30.0;
        while s < wf.s1 + 30.0 {
            let f = path.frame(t, s);
            let lat = out * 76.0;
            self.extra_lamps.push(Spot {
                x: f.x + f.rx * lat,
                y: g0 + 0.12,
                z: f.z + f.rz * lat,
                dx: -f.rx * out,
                dz: -f.rz * out,
                h: 5.5,
                arm: Some(0.8),
                led: false,
            });
            for tl in [48.0, 60.0] {
                if self.rnd() < 0.3 {
                    continue;
                }
                let tt = out * (tl + self.rnd() * 6.0);
                let sc = 0.9 + self.rnd() * 0.5;
                self.park_trees.push(ParkTree {
                    x: f.x + f.rx * tt,
                    y: g0 + 0.12,
                    z: f.z + f.rz * tt,
                    s: sc,
                });
            }
            s += 24.0;
        }
        // The ferris wheel, two-thirds of the way along.
        let sw = wf.s0 + (wf.s1 - wf.s0) * 0.62;
        let f = path.frame(t, sw);
        let lat = out * 64.0;
        self.build_ferris_wheel(f.x + f.rx * lat, g0 + 0.12, f.z + f.rz * lat, f.fx, f.fz);
        self.fw.reserved.push([sw - 40.0, sw + 40.0]);
    }

    /// `this.group.add(staticMesh(g, m, { cast, receive }))` (and its render order).
    fn add_static(&mut self, g: BufferGeometry, m: MaterialId, cast: bool, receive: bool, ro: f64) {
        let g = self.graph.add_geometry(g);
        let n = static_mesh(
            self.graph,
            g,
            m,
            &StaticOpts {
                cast,
                receive,
                ..StaticOpts::default()
            },
        );
        self.graph.get_mut(n).render_order = ro;
        self.graph.add(self.group, n);
    }

    fn build_ferris_wheel(&mut self, x: f64, y0: f64, z: f64, fx: f64, fz: f64) {
        const R: f64 = 27.0;
        let hub = y0 + R + 6.0;
        let root = self.graph.group("");
        {
            let o = self.graph.get_mut(root);
            o.position = Vector3::new(x, hub, z);
            o.set_rotation(&Euler::new(0.0, kernel::atan2(-fz, fx), 0.0)); // local X along the road
        }
        let rotor = self.graph.group("");
        self.graph.add(root, rotor);
        let metal = self.graph.add_material(
            Material::standard()
                .set("color", 0xd8dce2)
                .set("metalness", 0.6)
                .set("roughness", 0.35)
                .set("emissive", 0x223044)
                .set("emissiveIntensity", 0.6),
        );
        let mut parts = Vec::new();
        for zz in [-1.4, 1.4] {
            let mut rim = torus_geometry(R, 0.28, 6.0, 72.0, PI * 2.0);
            rim.translate(0.0, 0.0, zz);
            parts.push(rim);
        }
        for k in 0..24 {
            let a = (k as f64 / 24.0) * PI * 2.0;
            for zz in [-1.4, 1.4] {
                let mut sp = cylinder_geometry(0.08, 0.08, R, 4.0, 1.0, false, 0.0, PI * 2.0);
                sp.translate(0.0, R / 2.0, 0.0);
                sp.rotate_z(a - PI / 2.0);
                sp.translate(0.0, 0.0, zz);
                parts.push(sp);
            }
        }
        let mut hub_g = cylinder_geometry(1.4, 1.4, 3.6, 12.0, 1.0, false, 0.0, PI * 2.0);
        hub_g.rotate_x(PI / 2.0);
        parts.push(hub_g);
        let g = self.graph.add_geometry(merge_flat_geos(&parts));
        let m = static_mesh(self.graph, g, metal, &StaticOpts::default());
        self.graph.add(rotor, m);
        // Rim lights in rainbow order, and gondolas.
        let mut lp: Vec<f64> = Vec::new();
        let mut lc: Vec<f64> = Vec::new();
        let mut c = Color::new(1.0, 1.0, 1.0);
        for k in 0..96 {
            let a = (k as f64 / 96.0) * PI * 2.0;
            c.set_hsl(k as f64 / 96.0, 0.9, 0.55);
            for zz in [-1.7, 1.7] {
                lp.extend_from_slice(&[kernel::cos(a) * R, kernel::sin(a) * R, zz]);
                lc.extend_from_slice(&[c.r * 3.2, c.g * 3.2, c.b * 3.2]);
            }
        }
        let mut lg = BufferGeometry::new();
        lg.set_attribute("position", BufferAttribute::from_f64(&lp, 3));
        lg.set_attribute("color", BufferAttribute::from_f64(&lc, 3));
        let glow = self
            .graph
            .cached_texture(&self.textures.glow_texture(), Layer::Main, "");
        let lm = self.graph.add_material(
            Material::points()
                .set("size", 0.9)
                .set("vertexColors", true)
                .set("map", glow)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", three::ADDITIVE_BLENDING as f64),
        );
        let lg = self.graph.add_geometry(lg);
        let lights = self.graph.drawable(NodeType::Points, lg, lm);
        self.graph.add(rotor, lights);
        let mut gond = GeoBuilder::new(true, false);
        for k in 0..16 {
            let a = (k as f64 / 16.0) * PI * 2.0;
            let gx = kernel::cos(a) * (R + 1.8);
            let gy = kernel::sin(a) * (R + 1.8);
            c.set_hsl(k as f64 / 16.0, 0.7, 0.5);
            gond.box_(
                gx,
                gy - 1.4,
                0.0,
                2.2,
                2.4,
                2.2,
                0.0,
                &PrismOpts {
                    color: Some([c.r * 0.8, c.g * 0.8, c.b * 0.8]),
                    ..PrismOpts::default()
                },
            );
        }
        let gm = self
            .graph
            .add_material(Material::basic().set("vertexColors", true));
        let g = self.graph.add_geometry(gond.build());
        let m = static_mesh(self.graph, g, gm, &StaticOpts::default());
        self.graph.add(rotor, m);
        // A-frame legs.
        let mut legs = Vec::new();
        for zz in [-3.2, 3.2] {
            for xx in [-15.0, 15.0] {
                let len = kernel::hypot(xx, hub - y0);
                let mut g = cylinder_geometry(0.45, 0.6, len, 8.0, 1.0, false, 0.0, PI * 2.0);
                g.translate(0.0, -len / 2.0, 0.0);
                g.rotate_z(kernel::atan2(xx, hub - y0));
                g.translate(0.0, 0.0, zz);
                legs.push(g);
            }
        }
        let g = self.graph.add_geometry(merge_flat_geos(&legs));
        let m = static_mesh(self.graph, g, metal, &StaticOpts::default());
        self.graph.add(root, m);
        self.graph.add(self.group, root);
        let mut rz = 0.0;
        self.anim
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                rz += u.dt * 0.06;
                let q = Quaternion::from_euler(&Euler::new(0.0, 0.0, rz));
                out.push(Edit {
                    target: Handle::Node(rotor),
                    change: Change::Transform {
                        position: [0.0, 0.0, 0.0],
                        quaternion: [q.x, q.y, q.z, q.w],
                        scale: [1.0, 1.0, 1.0],
                    },
                });
            }));
    }

    // One building on a lot [lu0,lu1]×[lv0,lv1] in grid space.
    fn building(&mut self, lu0: f64, lv0: f64, lu1: f64, lv1: f64, o: BuildingOpts) {
        let g0 = self.ground_y + 0.15;
        let grid = self.grid;
        let w = |u: f64, v: f64| grid.to_world(u, v);
        let cu = (lu0 + lu1) / 2.0;
        let cv = (lv0 + lv1) / 2.0;
        let cw = w(cu, cv);
        let bi = self.chunk_of(cw[0], cw[1]);
        let mut b = std::mem::take(&mut self.chunks[bi]);
        let lu = lu1 - lu0;
        let lv = lv1 - lv0;
        self.stats.buildings += 1;
        // Per-building seed for the window shader, from position so the layout
        // rng stream is untouched.
        let seed = hash2(cw[0], cw[1]);
        // Distance to the freeway for neon/visibility decisions.
        let hits = self.freeway_hits(cw[0], cw[1]);
        let near_fw = if hits.is_empty() {
            999.0
        } else {
            js::min_n(&hits.iter().map(|h| h.lat.abs()).collect::<Vec<_>>())
        };
        // Roof clutter and trim only where it will be seen up close: along the
        // freeway. Further out, the silhouette is what counts.
        let detail = near_fw < 420.0;
        let yaw = self.yaw();

        // Snap a footprint to the chosen cell's window pitch.
        let foot = |rng: &mut Mulberry32, cell: usize, inset: f64, jitter: f64| {
            let pw = CELL_TILE[cell][0] / CELL_COLS[cell];
            let wu = js::max(
                pw * 2.0,
                ((lu - inset * 2.0 - rng.next_f64() * jitter) / pw).floor() * pw,
            );
            let wv = js::max(
                pw * 2.0,
                ((lv - inset * 2.0 - rng.next_f64() * jitter) / pw).floor() * pw,
            );
            (wu, wv)
        };
        let rect = |wu: f64, wv: f64, du: f64, dv: f64| -> [P2; 4] {
            [
                w(cu + du - wu / 2.0, cv + dv - wv / 2.0),
                w(cu + du + wu / 2.0, cv + dv - wv / 2.0),
                w(cu + du + wu / 2.0, cv + dv + wv / 2.0),
                w(cu + du - wu / 2.0, cv + dv + wv / 2.0),
            ]
        };
        let floor_h = |cell: usize| CELL_TILE[cell][1] / CELL_ROWS[cell];
        let pitch = |cell: usize| CELL_TILE[cell][0] / CELL_COLS[cell];
        let boxc = |rng: &mut Mulberry32,
                    b: &mut GeoBuilder,
                    cell: usize,
                    wu: f64,
                    wv: f64,
                    y0: f64,
                    y1: f64,
                    du: f64,
                    dv: f64|
         -> [P2; 4] {
            let cols = CELL_COLS[cell];
            let rows = CELL_ROWS[cell];
            let r = rect(wu, wv, du, dv);
            let u_off = (rng.next_f64() * cols).floor() / cols;
            let v_off = (rng.next_f64() * rows).floor() / rows;
            b.prism(
                &r,
                y0,
                y1,
                &PrismOpts {
                    cell: Some(cell_seed(cell, seed)),
                    tile_w: Some(CELL_TILE[cell][0]),
                    tile_h: Some(CELL_TILE[cell][1]),
                    v_ref: Some(g0),
                    u_off: Some(u_off),
                    v_off: Some(v_off),
                    roof_cell: Some(ROOF_CELL as f64),
                    roof_tile: Some(16.0),
                    roof: Some(true),
                    ..PrismOpts::default()
                },
            );
            r
        };
        let centre_of = |r: &[P2]| [(r[0][0] + r[2][0]) / 2.0, (r[0][1] + r[2][1]) / 2.0];
        // Roof plant: AC units and vents.
        let plant = |rng: &mut Mulberry32,
                     b: &mut GeoBuilder,
                     wu: f64,
                     wv: f64,
                     top: f64,
                     n: f64,
                     du: f64,
                     dv: f64| {
            let mut k = 0.0;
            while k < n {
                let pu = cu + du + (rng.next_f64() - 0.5) * wu * 0.5;
                let pv = cv + dv + (rng.next_f64() - 0.5) * wv * 0.5;
                let p = w(pu, pv);
                let sx = 2.0 + rng.next_f64() * 5.0;
                let sy = 1.5 + rng.next_f64() * 2.5;
                let sz = 2.0 + rng.next_f64() * 4.0;
                b.box_(
                    p[0],
                    top - 0.1,
                    p[1],
                    sx,
                    sy,
                    sz,
                    yaw,
                    &cell_opts(ROOF_CELL, 6.0),
                );
                k += 1.0;
            }
        };
        // Timber-and-steel water tank on a stand, New York style.
        let tank = |rng: &mut Mulberry32, b: &mut GeoBuilder, du: f64, dv: f64, top: f64| {
            let p = w(cu + du, cv + dv);
            let r = 1.5 + rng.next_f64() * 0.8;
            let h = 3.0 + rng.next_f64() * 1.5;
            let leg = 2.2 + rng.next_f64();
            b.box_(
                p[0],
                top - 0.1,
                p[1],
                r * 2.0,
                leg,
                0.25,
                yaw,
                &no_roof(cell_opts(ROOF_CELL, 6.0)),
            );
            b.box_(
                p[0],
                top - 0.1,
                p[1],
                0.25,
                leg,
                r * 2.0,
                yaw,
                &no_roof(cell_opts(ROOF_CELL, 6.0)),
            );
            let ring = circle(p[0], p[1], r, 8);
            let y0 = top + leg;
            let y1 = y0 + h;
            b.prism(
                &ring,
                y0,
                y1,
                &PrismOpts {
                    cell: Some(WAREHOUSE_CELL as f64),
                    tile_w: Some(24.0),
                    tile_h: Some(48.0),
                    v_ref: Some(y0 - 22.0),
                    roof: Some(false),
                    ..PrismOpts::default()
                },
            );
            // Underside (seen from the street), as a fan of quads facing down.
            for k in [1, 3, 5] {
                let q = |p: P2| [p[0], y0, p[1]];
                up_quad(
                    b,
                    q(ring[0]),
                    q(ring[k]),
                    q(ring[k + 1]),
                    q(ring[k + 2]),
                    ROOF_CELL as f64,
                    None,
                    true,
                );
            }
            let apex = [p[0], y1 + r * 0.7, p[1]];
            for k in 0..8 {
                let a = ring[k];
                let bb = ring[(k + 1) % 8];
                b.tri(
                    [a[0], y1, a[1]],
                    apex,
                    [bb[0], y1, bb[1]],
                    None,
                    ROOF_CELL as f64,
                );
            }
        };
        // Parapet ring round a roof: outer face, inner face and coping.
        let parapet = |b: &mut GeoBuilder, r: &[P2; 4], top: f64, h: f64, t: f64| {
            let c = centre_of(r);
            let inner = grow_rect(r, c, -t);
            b.prism(r, top - 0.05, top + h, &no_roof(cell_opts(ROOF_CELL, 8.0)));
            walls_inward(b, &inner, top - 0.05, top + h, ROOF_CELL);
            for k in 0..4 {
                let (a, bb, ai, bi) = (r[k], r[(k + 1) % 4], inner[k], inner[(k + 1) % 4]);
                up_quad(
                    b,
                    [a[0], top + h, a[1]],
                    [bb[0], top + h, bb[1]],
                    [bi[0], top + h, bi[1]],
                    [ai[0], top + h, ai[1]],
                    ROOF_CELL as f64,
                    None,
                    false,
                );
            }
        };
        let antenna =
            |b: &mut GeoBuilder, aircraft: &mut Vec<P3>, x: f64, z: f64, top: f64, h: f64| {
                b.box_(
                    x,
                    top - 0.3,
                    z,
                    0.5,
                    h,
                    0.5,
                    yaw,
                    &cell_opts(ROOF_CELL, 4.0),
                );
                b.box_(
                    x,
                    top + h * 0.55,
                    z,
                    2.4,
                    0.2,
                    0.2,
                    yaw,
                    &no_roof(cell_opts(ROOF_CELL, 4.0)),
                );
                aircraft.push([x, top + h + 0.3, z]);
            };
        // LED strips up the corners of a volume.
        let corner_leds = |neon: &mut GeoBuilder, r: &[P2; 4], y0: f64, y1: f64, col: P3| {
            let c = centre_of(r);
            for p in grow_rect(r, c, 0.12) {
                neon.box_(p[0], y0, p[1], 0.3, y1 - y0, 0.3, yaw, &colored(col, false));
            }
        };
        let pick_neon =
            |rng: &mut Mulberry32| NEON[(rng.next_f64() * NEON.len() as f64).floor() as usize];
        let pick = |rng: &mut Mulberry32, arr: &[usize]| {
            arr[(rng.next_f64() * arr.len() as f64).floor() as usize]
        };

        if o.tower {
            self.stats.towers += 1;
            let rng = &mut self.rng;
            let h = 70.0 + o.core * (60.0 + rng.next_f64() * 210.0) + rng.next_f64() * 30.0;
            let cell = pick(rng, &[2, 4, 4, 0, 2, 1]);
            let pod_cell = pick(rng, &[1, 5, 3, 0]);
            let fh = floor_h(pod_cell);
            let pod_h = fh * (3.0 + (rng.next_f64() * 3.0).floor());
            let pf = foot(rng, pod_cell, 1.0, 0.0);
            let pod_r = boxc(
                rng,
                &mut b,
                pod_cell,
                pf.0,
                pf.1,
                g0 - 1.0,
                g0 + pod_h,
                0.0,
                0.0,
            );
            if detail && rng.next_f64() < 0.5 {
                parapet(&mut b, &pod_r, g0 + pod_h, 0.9, 0.3);
            }
            let pw = pitch(cell);
            let inset = 3.0 + rng.next_f64() * 6.0;
            let sf = foot(rng, cell, inset, 4.0);
            // Massing: a straight shaft with a slimmer top tier, a stepped
            // "wedding cake", or an off-centre slab with a taller slim twin.
            let form = rng.next_f64();
            let (mut tw, mut tv, mut tdu, mut tdv) = (sf.0, sf.1, 0.0, 0.0);
            let mut top_rect: [P2; 4];
            let mut top: f64;
            if form < 0.3 {
                let steps = 3 + if rng.next_f64() < 0.4 { 1 } else { 0 };
                let fr = [0.5, 0.25, 0.15, 0.1];
                let mut y = g0 + pod_h - 0.5;
                top_rect = [[0.0; 2]; 4];
                for k in 0..steps {
                    if k > 0 {
                        tw = js::max(
                            pw * 3.0,
                            tw - pw * 2.0 * (1.0 + (rng.next_f64() * 2.0).floor()),
                        );
                        tv = js::max(
                            pw * 3.0,
                            tv - pw * 2.0 * (1.0 + (rng.next_f64() * 2.0).floor()),
                        );
                    }
                    let y1 = if k == steps - 1 {
                        g0 + h
                    } else {
                        y + (h - pod_h) * fr[k] * if steps == 3 { 1.15 } else { 1.0 }
                    };
                    let c = if k == steps - 1 && rng.next_f64() < 0.3 {
                        GLASS_CELL
                    } else {
                        cell
                    };
                    top_rect = boxc(rng, &mut b, c, tw, tv, y, y1, 0.0, 0.0);
                    if k < steps - 1 && detail {
                        plant(rng, &mut b, tw, tv, y1, 1.0, 0.0, 0.0);
                    }
                    // Setback terraces get a lit band along their edge.
                    if k < steps - 1 && rng.next_f64() < 0.4 {
                        let col = scale3(pick_neon(rng), 0.6);
                        self.neon.prism(
                            &grow_rect(&top_rect, cw, 0.25),
                            y1 - 1.6,
                            y1 - 0.9,
                            &colored(col, false),
                        );
                    }
                    y = y1 - 0.3;
                }
                top = g0 + h;
            } else if form < 0.75 {
                let shaft_top = g0 + h * (0.7 + rng.next_f64() * 0.1);
                top_rect = boxc(
                    rng,
                    &mut b,
                    cell,
                    sf.0,
                    sf.1,
                    g0 + pod_h - 0.5,
                    shaft_top,
                    0.0,
                    0.0,
                );
                top = shaft_top;
                if rng.next_f64() < 0.8 {
                    tw = js::max(
                        pw * 3.0,
                        sf.0 - pw * 2.0 * (1.0 + (rng.next_f64() * 3.0).floor()),
                    );
                    tv = js::max(
                        pw * 3.0,
                        sf.1 - pw * 2.0 * (1.0 + (rng.next_f64() * 3.0).floor()),
                    );
                    if detail {
                        plant(rng, &mut b, sf.0, sf.1, shaft_top, 2.0, 0.0, 0.0);
                    }
                    let c = if rng.next_f64() < 0.3 {
                        GLASS_CELL
                    } else {
                        cell
                    };
                    top_rect = boxc(rng, &mut b, c, tw, tv, shaft_top - 0.3, g0 + h, 0.0, 0.0);
                    top = g0 + h;
                }
            } else {
                // Slab on one side of the lot, a slimmer tower rising past it.
                let main = (sf.0, js::max(pw * 3.0, (sf.1 * 0.55 / pw).floor() * pw));
                let side = if rng.next_f64() < 0.5 { -1.0 } else { 1.0 };
                let dv_main = side * (sf.1 - main.1) / 2.0;
                let slab_top = g0 + h * (0.5 + rng.next_f64() * 0.15);
                let slab = boxc(
                    rng,
                    &mut b,
                    cell,
                    main.0,
                    main.1,
                    g0 + pod_h - 0.5,
                    slab_top,
                    0.0,
                    dv_main,
                );
                parapet(&mut b, &slab, slab_top, 1.2, 0.3);
                if detail {
                    plant(rng, &mut b, main.0, main.1, slab_top, 2.0, 0.0, dv_main);
                }
                tw = js::max(pw * 3.0, (sf.0 * 0.6 / pw).floor() * pw);
                tv = js::max(pw * 3.0, sf.1 - main.1);
                tdv = -side * (sf.1 - tv) / 2.0;
                tdu = (rng.next_f64() - 0.5) * (sf.0 - tw);
                let c = if rng.next_f64() < 0.4 {
                    GLASS_CELL
                } else {
                    cell
                };
                top_rect = boxc(rng, &mut b, c, tw, tv, g0 + pod_h - 0.5, g0 + h, tdu, tdv);
                top = g0 + h;
            }
            let tier_top = top;
            let tc = centre_of(&top_rect);
            // Crown.
            let style = rng.next_f64();
            if style < 0.25 {
                // Glass cap, maybe with a spire.
                let ch = 6.0 + rng.next_f64() * 10.0;
                boxc(
                    rng,
                    &mut b,
                    GLASS_CELL,
                    tw * 0.6,
                    tv * 0.6,
                    top - 0.2,
                    top + ch,
                    tdu,
                    tdv,
                );
                top += ch;
            } else if style < 0.45 {
                // Pyramid.
                let apex = [tc[0], top + js::min(tw, tv) * 0.7, tc[1]];
                let r = top_rect;
                for k in 0..4 {
                    let (a, bb) = (r[k], r[(k + 1) % 4]);
                    b.tri(
                        [a[0], top, a[1]],
                        [bb[0], top, bb[1]],
                        apex,
                        None,
                        GLASS_CELL as f64,
                    );
                    b.tri(
                        [a[0], top, a[1]],
                        apex,
                        [bb[0], top, bb[1]],
                        None,
                        GLASS_CELL as f64,
                    );
                }
                top = apex[1];
            } else if style < 0.6 {
                // Stepped crown: three shrinking glass tiers, each edged in light.
                let col = pick_neon(rng);
                let (mut cw2, mut cv2) = (tw, tv);
                for _ in 0..3 {
                    cw2 *= 0.78;
                    cv2 *= 0.78;
                    let hh = 4.0 + rng.next_f64() * 3.0;
                    let r = boxc(
                        rng,
                        &mut b,
                        GLASS_CELL,
                        cw2,
                        cv2,
                        top - 0.2,
                        top + hh,
                        tdu,
                        tdv,
                    );
                    self.neon.prism(
                        &grow_rect(&r, tc, 0.2),
                        top + hh - 0.8,
                        top + hh - 0.2,
                        &colored(scale3(col, 0.8), false),
                    );
                    top += hh;
                }
            } else if style < 0.75 {
                // Spire on a plant room.
                boxc(
                    rng,
                    &mut b,
                    ROOF_CELL,
                    tw * 0.45,
                    tv * 0.45,
                    top - 0.2,
                    top + 5.0,
                    tdu,
                    tdv,
                );
                top += 5.0;
                let sh = 18.0 + rng.next_f64() * 40.0;
                let sr = js::min(tw, tv) * 0.12 + 0.6;
                let base = circle(tc[0], tc[1], sr, 6);
                let apex = [tc[0], top + sh, tc[1]];
                for k in 0..6 {
                    let (a, bb) = (base[k], base[(k + 1) % 6]);
                    b.tri(
                        [a[0], top, a[1]],
                        apex,
                        [bb[0], top, bb[1]],
                        None,
                        GLASS_CELL as f64,
                    );
                }
                top = apex[1];
            } else if style < 0.87 && tw > 18.0 && tv > 18.0 {
                // Helipad: a raised deck with edge lights and a painted H.
                parapet(&mut b, &top_rect, top, 1.1, 0.35);
                let s = js::min(tw, tv) * 0.7;
                let deck_y = top + 1.4;
                boxc(rng, &mut b, ROOF_CELL, s, s, top - 0.2, deck_y, tdu, tdv);
                self.helipad(tc[0], deck_y + 0.03, tc[1], s, yaw);
            } else {
                // Flat roof: parapet, plant, maybe LED corners.
                parapet(&mut b, &top_rect, top, 1.4, 0.35);
                let rng = &mut self.rng;
                if detail {
                    plant(rng, &mut b, tw, tv, top, 2.0, tdu, tdv);
                }
                if rng.next_f64() < 0.5 {
                    let col = scale3(pick_neon(rng), 0.7);
                    corner_leds(
                        &mut self.neon,
                        &top_rect,
                        top - js::min(40.0, (top - g0) * 0.3),
                        top + 1.4,
                        col,
                    );
                }
            }
            let rng = &mut self.rng;
            // LED crown band.
            if rng.next_f64() < 0.55 {
                let col = pick_neon(rng);
                let band = grow_rect(&top_rect, tc, 0.3);
                let by = tier_top - 3.2;
                self.neon.prism(&band, by, by + 1.4, &colored(col, false));
            }
            // Antennas and aircraft lights.
            if h > 150.0 && rng.next_f64() < 0.6 {
                let ah = 15.0 + rng.next_f64() * 30.0;
                b.box_(
                    tc[0],
                    top - 0.5,
                    tc[1],
                    1.0,
                    ah,
                    1.0,
                    0.0,
                    &cell_opts(ROOF_CELL, 4.0),
                );
                self.aircraft.push([tc[0], top + ah + 0.4, tc[1]]);
            } else if h > 90.0 {
                self.aircraft.push([tc[0], top + 0.6, tc[1]]);
            }
            if h > 200.0 && rng.next_f64() < 0.7 {
                for p in top_rect {
                    self.aircraft.push([p[0], tier_top + 0.8, p[1]]);
                }
            }
            self.chunks[bi] = b;
            return;
        }

        // Mid- and low-rise.
        let rng = &mut self.rng;
        let mid = o.ring > 0.45 && rng.next_f64() < 0.75;
        let cell = match o.cell {
            Some(c) => c,
            None => {
                if mid {
                    pick(rng, &[0, 1, 3, 1])
                } else {
                    pick(rng, &[5, 5, 3, 1])
                }
            }
        };
        let brick = cell == BRICK_CELL;
        let fh = floor_h(cell);
        let mut floors = match o.floors {
            Some([a, bb]) => a + (rng.next_f64() * (bb - a + 1.0)).floor(),
            None => {
                if mid {
                    4.0 + (rng.next_f64() * (6.0 + o.ring * 12.0)).floor()
                } else {
                    2.0 + (rng.next_f64() * 4.0).floor()
                }
            }
        };
        if rng.next_f64() < 0.07 + o.ring * 0.08 {
            floors = js::round(floors * (1.8 + rng.next_f64() * 1.4)); // the odd taller block
        }
        let h = floors * fh;
        let inset = 0.5 + rng.next_f64() * 3.0;
        let f = foot(rng, cell, inset, 3.0);
        let r: [P2; 4];
        let roof_r: [P2; 4];
        let (mut wu, mut wv, mut du, mut dv) = (f.0, f.1, 0.0, 0.0);
        if !brick && floors >= 8.0 && rng.next_f64() < 0.35 {
            // Setback: the top few floors step in on one or two sides.
            let hs = g0 + fh * js::round(floors * (0.6 + rng.next_f64() * 0.2));
            r = boxc(rng, &mut b, cell, f.0, f.1, g0 - 1.0, hs, 0.0, 0.0);
            let pw = pitch(cell);
            wu = js::max(pw * 2.0, f.0 - pw * (1.0 + (rng.next_f64() * 2.0).floor()));
            wv = js::max(pw * 2.0, f.1 - pw * (1.0 + (rng.next_f64() * 2.0).floor()));
            du = (if rng.next_f64() < 0.5 { -1.0 } else { 1.0 }) * (f.0 - wu) / 2.0;
            dv = (if rng.next_f64() < 0.5 { -1.0 } else { 1.0 }) * (f.1 - wv) / 2.0;
            if detail {
                parapet(&mut b, &r, hs, 0.9, 0.3);
            }
            roof_r = boxc(rng, &mut b, cell, wu, wv, hs - 0.3, g0 + h, du, dv);
        } else {
            r = boxc(rng, &mut b, cell, f.0, f.1, g0 - 1.0, g0 + h, 0.0, 0.0);
            roof_r = r;
        }
        let top = g0 + h;
        let rc = centre_of(&roof_r);
        if detail {
            if brick {
                // Cornice and parapet.
                let cr = grow_rect(&roof_r, rc, 0.45);
                b.prism(
                    &cr,
                    top - 0.9,
                    top - 0.1,
                    &no_roof(cell_opts(ROOF_CELL, 8.0)),
                );
                for k in 0..4 {
                    let (a, bb, ai, bi2) = (cr[k], cr[(k + 1) % 4], roof_r[k], roof_r[(k + 1) % 4]);
                    up_quad(
                        &mut b,
                        [a[0], top - 0.9, a[1]],
                        [bb[0], top - 0.9, bb[1]],
                        [bi2[0], top - 0.9, bi2[1]],
                        [ai[0], top - 0.9, ai[1]],
                        ROOF_CELL as f64,
                        None,
                        true,
                    );
                }
                parapet(&mut b, &roof_r, top, 0.8, 0.3);
            } else if rng.next_f64() < 0.65 {
                let ph = 0.9 + rng.next_f64() * 0.6;
                parapet(&mut b, &roof_r, top, ph, 0.3);
            }
            let n = 1.0 + (rng.next_f64() * 3.0).floor();
            plant(rng, &mut b, wu, wv, top, n, du, dv);
            if rng.next_f64() < (if brick { 0.45 } else { 0.18 }) && wu > 9.0 && wv > 9.0 {
                let tdu = (rng.next_f64() - 0.5) * wu * 0.4 + du;
                let tdv = (rng.next_f64() - 0.5) * wv * 0.4 + dv;
                tank(rng, &mut b, tdu, tdv, top);
            }
            if h > 30.0 && rng.next_f64() < 0.2 {
                let ax = rc[0] + (rng.next_f64() - 0.5) * 4.0;
                let az = rc[1] + (rng.next_f64() - 0.5) * 4.0;
                let ah = 8.0 + rng.next_f64() * 12.0;
                antenna(&mut b, &mut self.aircraft, ax, az, top, ah);
            }
            if near_fw < 260.0 && h < 50.0 && self.rng.next_f64() < 0.14 {
                self.roof_billboard(&mut b, &roof_r, top, cw);
            }
            let rng = &mut self.rng;
            if !brick && rng.next_f64() < 0.08 {
                let col = scale3(pick_neon(rng), 0.6);
                self.neon.prism(
                    &grow_rect(&roof_r, rc, 0.2),
                    top - 0.6,
                    top - 0.2,
                    &colored(col, false),
                );
            }
        } else {
            plant(rng, &mut b, wu, wv, top, 1.0, du, dv);
        }
        // Neon signage on buildings that face the freeway.
        if near_fw < 220.0 && self.rng.next_f64() < 0.4 {
            let rng = &mut self.rng;
            let col = pick_neon(rng);
            // Face whose outward normal points most toward the freeway.
            let t = self.track;
            let tr = t.distance_to_road(cw[0], cw[1], 400.0);
            let mut best = 0;
            let mut best_dot = f64::NEG_INFINITY;
            for k in 0..4 {
                let (a, bb) = (r[k], r[(k + 1) % 4]);
                let mx = (a[0] + bb[0]) / 2.0;
                let mz = (a[1] + bb[1]) / 2.0;
                let nx = mx - cw[0];
                let nz = mz - cw[1];
                let tf = if tr.i >= 0 {
                    t.frame(tr.s.expect("in range"))
                } else {
                    self.led.dc.expect("the downtown core")
                };
                let dx = tf.x - mx;
                let dz = tf.z - mz;
                let d =
                    (nx * dx + nz * dz) / (kernel::hypot(nx, nz) * kernel::hypot(dx, dz) + 1e-6);
                if d > best_dot {
                    best_dot = d;
                    best = k;
                }
            }
            let (a, bb) = (r[best], r[(best + 1) % 4]);
            let mx = (a[0] + bb[0]) / 2.0;
            let mz = (a[1] + bb[1]) / 2.0;
            let nx = mx - cw[0];
            let nz = mz - cw[1];
            let nl = kernel::hypot(nx, nz);
            let ox = (nx / nl) * 0.35;
            let oz = (nz / nl) * 0.35;
            let tx = bb[0] - a[0];
            let tz = bb[1] - a[1];
            let tl = kernel::hypot(tx, tz);
            let along = (rng.next_f64() - 0.5) * 0.6;
            let px = mx + (tx / tl) * along * tl + ox;
            let pz = mz + (tz / tl) * along * tl + oz;
            let ww = 1.6 + rng.next_f64() * 1.2;
            let hh = js::min(h - 6.0, 6.0 + rng.next_f64() * 10.0);
            let y0 = js::max(g0 + 5.0, g0 + h - hh - 1.5);
            let ux = (tx / tl) * ww / 2.0;
            let uz = (tz / tl) * ww / 2.0;
            // Double-sided vertical sign.
            let qa = [px - ux, y0, pz - uz];
            let qb = [px + ux, y0, pz + uz];
            let qc = [px + ux, y0 + hh, pz + uz];
            let qd = [px - ux, y0 + hh, pz - uz];
            self.neon.quad(qa, qb, qc, qd, None, Some(col), 0.0);
            self.neon.quad(qb, qa, qd, qc, None, Some(col), 0.0);
        }
        self.chunks[bi] = b;
    }

    // The setback strips either side of the freeway: landscaped verges with
    // trees and lamps, so the ground between the barriers and the first
    // buildings isn't a black void at night.
    fn build_verge(&mut self) {
        let t = self.track;
        let tr = self.terrain;
        let path = self.path.clone();
        let mut rng = Mulberry32::new(31);
        let mut k = 0usize;
        let mut u = path.u0 + 10.0;
        while u < path.u1 - 10.0 {
            let skip = self.fw.reserved_at(t, u, 45.0)
                || self.fw.in_tunnel(t, u)
                || (u >= path.s_a && u <= path.s_b && tr.is_elevated(t, t.idx(u)));
            if !skip {
                let f = path.frame(t, u);
                let mut sides = vec![(-52.0, -41.0, -1.0)];
                if !(f.ext && u < path.s_a) {
                    sides.push((17.5, 29.0, 1.0));
                }
                for (a, b, side) in sides {
                    let lamp = k % 5 == if side > 0.0 { 0 } else { 2 };
                    if !lamp && rng.next_f64() < 0.4 {
                        continue;
                    }
                    let lat = if lamp {
                        if side > 0.0 { 17.5 } else { -41.0 }
                    } else {
                        lerp(a, b, rng.next_f64())
                    };
                    let x = f.x + f.rx * lat;
                    let z = f.z + f.rz * lat;
                    let gy = tr.height_at(x, z);
                    if (gy - self.ground_y).abs() > 1.2 {
                        continue;
                    }
                    let key = (
                        (self.grid.to_u(x, z) / PU).floor() as i64,
                        (self.grid.to_v(x, z) / PV).floor() as i64,
                    );
                    if self.street_cells.contains(&key) {
                        continue;
                    }
                    if self
                        .freeway_hits(x, z)
                        .iter()
                        .any(|h| h.lat > -40.0 && h.lat < 16.0)
                    {
                        continue;
                    }
                    if lamp {
                        self.extra_lamps.push(Spot {
                            x,
                            y: gy,
                            z,
                            dx: f.rx * side,
                            dz: f.rz * side,
                            h: 7.0,
                            arm: Some(1.4),
                            led: false,
                        });
                    } else {
                        let s = 0.8 + rng.next_f64() * 0.6;
                        self.park_trees.push(ParkTree { x, y: gy, z, s });
                    }
                }
            }
            u += 9.0;
            k += 1;
        }
    }

    // Lots too close to the freeway for a building become car parks: asphalt,
    // tall lamps and rows of parked cars instead of dead black ground.
    fn parking_lot(&mut self, lu0: f64, lv0: f64, lu1: f64, lv1: f64) {
        for a in 0..=4 {
            for b in 0..=4 {
                let p = self.w(
                    lerp(lu0 - 2.5, lu1 + 2.5, a as f64 / 4.0),
                    lerp(lv0 - 2.5, lv1 + 2.5, b as f64 / 4.0),
                );
                if !self.flat(p[0], p[1]) {
                    return;
                }
                if self
                    .freeway_hits(p[0], p[1])
                    .iter()
                    .any(|h| h.lat > -39.0 && h.lat < 15.0)
                {
                    return;
                }
            }
        }
        self.pad(
            lu0 - 2.5,
            lv0 - 2.5,
            lu1 + 2.5,
            lv1 + 2.5,
            [0.1, 0.1, 0.105],
            0.12,
        );
        let y = self.ground_y + 0.27;
        let yaw = self.yaw();
        let mut row = 0;
        let mut v = lv0 + 3.0;
        while v + 2.7 < lv1 {
            if row % 2 != 1 {
                let mut u = lu0 + 1.5;
                while u + 1.4 < lu1 {
                    if self.rnd() < 0.4 {
                        u += 2.8;
                        continue;
                    }
                    let pv = v + 1.5 + (self.rnd() - 0.5) * 0.4;
                    let p = self.w(u + 1.4, pv);
                    let flip = if self.rnd() < 0.5 { PI } else { 0.0 };
                    let jit = (self.rnd() - 0.5) * 0.08;
                    self.parked
                        .push([p[0], y, p[1], yaw + PI / 2.0 + flip + jit]);
                    u += 2.8;
                }
            }
            v += 6.2;
            row += 1;
        }
        let mut u = lu0 + 8.0;
        while u < lu1 - 4.0 {
            let mut v = lv0 + 8.0;
            while v < lv1 - 4.0 {
                self.lamp_spots.push(LampSpot {
                    u,
                    v,
                    du: 1.0,
                    dv: 0.0,
                    h: Some(9.5),
                    arm: Some(0.5),
                });
                v += 24.0;
            }
            u += 24.0;
        }
    }

    fn build_parked_cars(&mut self) {
        if self.parked.is_empty() {
            return;
        }
        let mut body =
            crate::three_geom::box_geometry(4.3, 0.72, 1.78, 1.0, 1.0, 1.0).to_non_indexed();
        body.translate(0.0, 0.62, 0.0);
        let mut cab =
            crate::three_geom::box_geometry(2.3, 0.56, 1.58, 1.0, 1.0, 1.0).to_non_indexed();
        cab.translate(-0.25, 1.26, 0.0);
        paint(&mut body, [1.0, 1.0, 1.0]);
        paint(&mut cab, [0.25, 0.27, 0.3]);
        let geo = merge_two(&body, &cab);
        let mat = self.graph.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.35)
                .set("metalness", 0.55),
        );
        let mut rng = Mulberry32::new(7);
        const PAINT: [[f64; 3]; 8] = [
            [0.55, 0.56, 0.58],
            [0.06, 0.06, 0.07],
            [0.7, 0.7, 0.72],
            [0.35, 0.05, 0.05],
            [0.08, 0.14, 0.32],
            [0.3, 0.3, 0.32],
            [0.9, 0.9, 0.9],
            [0.15, 0.25, 0.18],
        ];
        let ms: Vec<Matrix4> = self
            .parked
            .iter()
            .map(|p| trs(p[0], p[1], p[2], p[3], 1.0, 1.0, 1.0, 0.0, 0.0))
            .collect();
        let cols: Vec<Color> = self
            .parked
            .iter()
            .map(|_| {
                let c = PAINT[(rng.next_f64() * PAINT.len() as f64).floor() as usize];
                Color::new(c[0], c[1], c[2])
            })
            .collect();
        self.emit_instanced(geo, mat, &ms, false, true, 1300.0, Some(&cols));
    }

    // A tower under construction: bare concrete core and floor slabs, work
    // lights, and a tower crane with aircraft lights on the mast and jib.
    fn site(&mut self, lu0: f64, lv0: f64, lu1: f64, lv1: f64, core: f64) {
        self.sites += 1;
        let g0 = self.ground_y + 0.15;
        let cu = (lu0 + lu1) / 2.0;
        let cv = (lv0 + lv1) / 2.0;
        let cw = self.w(cu, cv);
        let bi = self.chunk_of(cw[0], cw[1]);
        let mut b = std::mem::take(&mut self.chunks[bi]);
        let yaw = self.yaw();
        let o = cell_opts(ROOF_CELL, 8.0);
        let ob = PrismOpts {
            bottom: true,
            ..o.clone()
        };
        let s = js::min(lu1 - lu0, lv1 - lv0) * 0.7;
        let hs = 25.0 + self.rnd() * (30.0 + core * 60.0);
        b.box_(
            cw[0],
            g0 - 1.0,
            cw[1],
            s * 0.35,
            hs + 5.0,
            s * 0.35,
            yaw,
            &o,
        );
        // Slabs every floor on thin columns.
        let mut y = g0 + 4.0;
        while y < g0 + hs {
            b.box_(cw[0], y, cw[1], s, 0.35, s, yaw, &ob);
            y += 4.0;
        }
        for [a, bb] in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            let p = self.w(cu + a * s * 0.45, cv + bb * s * 0.45);
            b.box_(p[0], g0 - 1.0, p[1], 0.6, hs + 1.0, 0.6, yaw, &o);
            if self.rnd() < 0.6 {
                let fl = (self.rnd() * hs / 4.0).floor();
                self.glows
                    .push([p[0], g0 + 4.0 + fl * 4.0 - 0.4, p[1], 1.2, 1.2, 1.1, 1.2]);
            }
        }
        // Crane beside it.
        let mp = self.w(cu + s * 0.65, cv - s * 0.3);
        let mh = hs + 25.0 + self.rnd() * 25.0;
        b.box_(mp[0], g0 - 0.5, mp[1], 2.0, mh, 2.0, yaw, &o);
        let jy = yaw + self.rnd() * PI * 2.0;
        let jl = 40.0 + self.rnd() * 20.0;
        let cl = 14.0;
        let jx = kernel::cos(jy);
        let jz = kernel::sin(jy);
        b.box_(
            mp[0] + jx * (jl - cl) / 2.0,
            g0 + mh,
            mp[1] + jz * (jl - cl) / 2.0,
            jl + cl,
            1.4,
            1.4,
            jy,
            &ob,
        );
        b.box_(mp[0], g0 + mh + 1.4, mp[1], 1.4, 7.0, 1.4, jy, &o);
        b.box_(
            mp[0] + jx * 1.8,
            g0 + mh - 3.0,
            mp[1] + jz * 1.8,
            2.4,
            3.0,
            2.4,
            jy,
            &ob,
        );
        b.box_(
            mp[0] - jx * (cl - 2.0),
            g0 + mh - 2.5,
            mp[1] - jz * (cl - 2.0),
            3.5,
            2.5,
            3.0,
            jy,
            &ob,
        );
        // Cab light, then the aviation lights.
        self.glows.push([
            mp[0] + jx * 1.8,
            g0 + mh - 1.5,
            mp[1] + jz * 1.8,
            1.3,
            1.1,
            0.8,
            0.7,
        ]);
        self.aircraft.push([mp[0], g0 + mh + 8.8, mp[1]]);
        self.aircraft
            .push([mp[0] + jx * jl, g0 + mh + 1.8, mp[1] + jz * jl]);
        self.aircraft
            .push([mp[0] - jx * cl, g0 + mh + 1.6, mp[1] - jz * cl]);
        self.chunks[bi] = b;
    }

    // A rooftop helipad: painted H and a ring of green edge lights.
    fn helipad(&mut self, x: f64, y: f64, z: f64, s: f64, yaw: f64) {
        let c = kernel::cos(yaw);
        let sn = kernel::sin(yaw);
        let p = |a: f64, b: f64| [x + a * c - b * sn, y, z + a * sn + b * c];
        let neon = &mut self.neon;
        let mut q = |a0: f64, b0: f64, a1: f64, b1: f64| {
            up_quad(
                neon,
                p(a0, b0),
                p(a1, b0),
                p(a1, b1),
                p(a0, b1),
                0.0,
                Some([0.35, 0.35, 0.32]),
                false,
            );
        };
        let h = s * 0.18;
        q(-h, -h * 1.3, -h * 0.65, h * 1.3);
        q(h * 0.65, -h * 1.3, h, h * 1.3);
        q(-h * 0.65, -h * 0.18, h * 0.65, h * 0.18);
        for k in 0..12 {
            let a = (k as f64 / 12.0) * PI * 2.0;
            self.pad_lights.push([
                x + kernel::cos(a) * s * 0.42,
                y + 0.2,
                z + kernel::sin(a) * s * 0.42,
            ]);
        }
    }

    // A billboard standing on a roof, facing the nearest bit of freeway.
    fn roof_billboard(&mut self, b: &mut GeoBuilder, r: &[P2; 4], top: f64, cw: P2) {
        let tr = self.track.distance_to_road(cw[0], cw[1], 400.0);
        if tr.i < 0 {
            return;
        }
        let f = self.track.frame(tr.s.expect("in range"));
        let c = [(r[0][0] + r[2][0]) / 2.0, (r[0][1] + r[2][1]) / 2.0];
        let mut nx = f.x - c[0];
        let mut nz = f.z - c[1];
        let nl = js::or(kernel::hypot(nx, nz), 1.0);
        nx /= nl;
        nz /= nl;
        let wb = 12.0;
        let hb = wb * 384.0 / 1024.0;
        let lift = 2.5;
        let ax = [nz, -nx];
        let yaw_a = kernel::atan2(ax[1], ax[0]);
        for k in [-0.3, 0.3] {
            b.box_(
                c[0] - nx * 0.6 + ax[0] * wb * k,
                top - 0.1,
                c[1] - nz * 0.6 + ax[1] * wb * k,
                0.35,
                lift + hb * 0.6,
                0.35,
                yaw_a,
                &cell_opts(ROOF_CELL, 4.0),
            );
        }
        b.box_(
            c[0] - nx * 0.25,
            top + lift - 0.2,
            c[1] - nz * 0.25,
            wb + 0.4,
            hb + 0.4,
            0.3,
            yaw_a,
            &cell_opts(ROOF_CELL, 4.0),
        );
        // Nudge a hair toward the viewer so the ad sits in front of its frame.
        let image = ad_texture(self.textures, self.roof_ads.len() as i64 * 5 + 3);
        self.roof_ads.push(AtlasQuad {
            image,
            c: [c[0] + nx * 0.02, top + lift + hb / 2.0, c[1] + nz * 0.02],
            rx: ax[0],
            rz: ax[1],
            w: wb,
            h: hb,
        });
    }

    // ── Street lamps (grid streets, overpasses, lid park) ─────────
    fn build_lamps(&mut self) {
        let g0 = self.ground_y;
        self.thin = 0;
        let mut spots: Vec<Spot> = Vec::new();
        let overs: Vec<(bool, f64, f64, f64, Vec<freeway::BridgeLamp>)> = self
            .fw
            .overpasses
            .iter()
            .filter_map(|o| {
                o.span
                    .map(|(v0, v1, uu)| (o.fam_v, v0, v1, uu, o.lamps.clone()))
            })
            .collect();
        for l in self.lamp_spots.clone() {
            let on_bridge = overs.iter().any(|&(fam_v, v0, v1, uu, _)| {
                if fam_v {
                    (l.v - uu).abs() < 10.0 && l.u > v0 - 5.0 && l.u < v1 + 5.0
                } else {
                    (l.u - uu).abs() < 10.0 && l.v > v0 - 5.0 && l.v < v1 + 5.0
                }
            });
            if on_bridge {
                continue;
            }
            let p = self.w(l.u, l.v);
            if self.lamp_blocked(p[0], p[1]) {
                continue;
            }
            if self.is_loop {
                self.thin += 1;
                if self.thin % 2 == 1 && self.terrain.far(p[0], p[1]).d > 600.0 {
                    continue;
                }
            }
            let g = &self.grid;
            let d = [
                g.u_axis[0] * l.du + g.v_axis[0] * l.dv,
                g.u_axis[1] * l.du + g.v_axis[1] * l.dv,
            ];
            spots.push(Spot {
                x: p[0],
                y: g0 + 0.15,
                z: p[1],
                dx: d[0],
                dz: d[1],
                h: l.h.unwrap_or(7.5),
                arm: l.arm,
                led: self.led.at(self.track, self.terrain, p[0], p[1]),
            });
        }
        for (_, _, _, _, lamps) in &overs {
            for l in lamps {
                spots.push(Spot {
                    x: l.x,
                    y: l.y,
                    z: l.z,
                    dx: l.dx,
                    dz: l.dz,
                    h: 6.5,
                    arm: None,
                    led: false,
                });
            }
        }
        for l in &self.fw.park_lamps {
            spots.push(Spot {
                x: l.x,
                y: l.y,
                z: l.z,
                dx: 1.0,
                dz: 0.0,
                h: 4.2,
                arm: Some(0.0),
                led: false,
            });
        }
        spots.extend(self.extra_lamps.iter().copied());
        if spots.is_empty() {
            return;
        }
        // Pole with an arm along local +X.
        let mut pole = cylinder_geometry(0.1, 0.14, 1.0, 6.0, 1.0, false, 0.0, PI * 2.0);
        pole.translate(0.0, 0.5, 0.0);
        let mut pole_m = Vec::new();
        let mut arm_m = Vec::new();
        let mut lens_m = Vec::new();
        let mut pool_m = Vec::new();
        let mut tints = Vec::new();
        // Downtown streets have white LED heads, the rest orange sodium.
        let sod = Color::new(1.0, 0.72, 0.42);
        let led = Color::new(0.75, 0.86, 1.05);
        for s in &spots {
            let tint = if s.led { led } else { sod };
            tints.push(tint);
            let yaw = -kernel::atan2(s.dz, s.dx);
            pole_m.push(trs(s.x, s.y, s.z, 0.0, 1.0, s.h, 1.0, 0.0, 0.0));
            let arm = s.arm.unwrap_or(1.8);
            if arm > 0.0 {
                arm_m.push(trs(
                    s.x + s.dx * arm / 2.0,
                    s.y + s.h - 0.1,
                    s.z + s.dz * arm / 2.0,
                    yaw,
                    arm,
                    0.12,
                    0.12,
                    0.0,
                    0.0,
                ));
            }
            lens_m.push(trs(
                s.x + s.dx * arm,
                s.y + s.h - 0.25,
                s.z + s.dz * arm,
                yaw,
                0.7,
                0.12,
                0.35,
                0.0,
                0.0,
            ));
            self.glows.push([
                s.x + s.dx * arm,
                s.y + s.h - 0.35,
                s.z + s.dz * arm,
                tint.r,
                tint.g,
                tint.b,
                1.0,
            ]);
            pool_m.push(trs(
                s.x + s.dx * (arm + 1.5),
                s.y + 0.09,
                s.z + s.dz * (arm + 1.5),
                0.0,
                13.0,
                1.0,
                13.0,
                0.0,
                0.0,
            ));
        }
        let metal = self.graph.add_material(
            Material::standard()
                .set("color", 0x5c6066)
                .set("metalness", 0.5)
                .set("roughness", 0.6),
        );
        self.emit_instanced(pole, metal, &pole_m, false, false, 1900.0, None);
        let unit = crate::three_geom::box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0);
        self.emit_instanced(unit.clone(), metal, &arm_m, false, false, 1900.0, None);
        let lens = self
            .graph
            .add_material(Material::basic().set("color", Color::new(4.0, 2.9, 1.7)));
        self.emit_instanced(unit, lens, &lens_m, false, false, 2600.0, Some(&tints));
        let mut pool_geo = plane_geometry(1.0, 1.0, 1.0, 1.0);
        pool_geo.rotate_x(-PI / 2.0);
        let glow = self
            .graph
            .cached_texture(&self.textures.glow_texture(), Layer::Main, "");
        let pool_mat = self.graph.add_material(
            Material::basic()
                .set("map", glow)
                .set("color", Color::new(1.0, 0.72, 0.42))
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", three::ADDITIVE_BLENDING as f64)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -4.0)
                .set("polygonOffsetUnits", -4.0),
        );
        for pools in self.emit_instanced(
            pool_geo,
            pool_mat,
            &pool_m,
            false,
            false,
            2200.0,
            Some(&tints),
        ) {
            self.graph.get_mut(pools).render_order = 2.0;
        }
        self.anim
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                let k = smoothstep(0.2, 0.8, u.night);
                out.push(Edit {
                    target: Handle::Material(pool_mat),
                    change: Change::Color {
                        prop: "color",
                        rgb: [0.26 * k, 0.25 * k, 0.24 * k],
                    },
                });
                let l = 0.3 + 0.7 * k;
                out.push(Edit {
                    target: Handle::Material(lens),
                    change: Change::Color {
                        prop: "color",
                        rgb: [4.0 * l, 4.0 * l, 4.0 * l],
                    },
                });
            }));
    }

    // Halo sprites on every lamp head (street, freeway, helipads). Their
    // on-screen size never drops below a couple of pixels, so the street grid
    // and the freeway stay traced in light all the way to the horizon instead
    // of fading to black once the lamp meshes are too small to see.
    fn build_glows(&mut self) {
        let mut list = self.glows.clone();
        for h in &self.fw.lamp_heads {
            list.push([h.x, h.y - 0.3, h.z, h.c[0], h.c[1], h.c[2], 1.1]);
        }
        for p in &self.pad_lights {
            list.push([p[0], p[1], p[2], 0.3, 1.4, 0.5, 0.5]);
        }
        for (r, amber) in &self.fw.reflectors {
            list.push(if *amber {
                [r[0], r[1], r[2], 0.5, 0.3, 0.05, 0.1]
            } else {
                [r[0], r[1], r[2], 0.35, 0.35, 0.35, 0.1]
            });
        }
        if list.is_empty() {
            return;
        }
        let mut pos = Vec::with_capacity(list.len() * 3);
        let mut col = Vec::with_capacity(list.len() * 3);
        let mut size = Vec::with_capacity(list.len());
        for g in &list {
            pos.extend_from_slice(&g[0..3]);
            col.extend_from_slice(&g[3..6]);
            size.push(g[6]);
        }
        let mut geo = BufferGeometry::new();
        geo.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        geo.set_attribute("color", BufferAttribute::from_f64(&col, 3));
        geo.set_attribute("gsize", BufferAttribute::from_f64(&size, 1));
        geo.compute_bounding_sphere();
        let mat = self.glow_points_material(2.0, 1.6);
        let g = self.graph.add_geometry(geo);
        let pts = self.graph.drawable(NodeType::Points, g, mat);
        {
            let o = self.graph.get_mut(pts);
            o.frustum_culled = false;
            o.render_order = 3.0;
        }
        self.graph.add(self.group, pts);
    }

    /// `glowPointsMaterial(world, size, minPx)`: additive halo sprites with a
    /// per-point colour and size (metres) that never shrink below minPx on
    /// screen; dimmer when clamped so far lamps don't outshine near ones. Fog
    /// is replaced by a gentler fade: lights carry much further through haze
    /// than the geometry they sit on (kind `GlowPoints`).
    fn glow_points_material(&mut self, size: f64, min_px: f64) -> MaterialId {
        let glow = self
            .graph
            .cached_texture(&self.textures.glow_texture(), Layer::Main, "");
        let m = self.graph.add_material(
            Material::points()
                .set("size", size)
                .set("sizeAttenuation", true)
                .set("map", glow)
                .set("vertexColors", true)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", three::ADDITIVE_BLENDING as f64)
                .set("fog", false)
                .kind(MaterialKind::GlowPoints, None)
                .program_key("city-glow")
                .uniform("uMinPx", num(min_px))
                .uniform("uFogK", num(0.0))
                .uniform("clippingPlanes", Value::Null),
        );
        self.anim
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                let v = 1.6 * smoothstep(0.2, 0.7, u.night);
                out.push(Edit {
                    target: Handle::Material(m),
                    change: Change::Color {
                        prop: "color",
                        rgb: [v, v, v],
                    },
                });
                // `world.scene` is the world's root group, which has no fog, so
                // this is always 0 in the JS (`fog?.density ?? 0`).
                out.push(Edit {
                    target: Handle::Material(m),
                    change: Change::Number {
                        prop: "uFogK",
                        value: 0.0 * 0.4,
                    },
                });
            }));
        m
    }

    // Cars on the grid streets, as pairs of head- and tail-light sprites that
    // slide along each block in the vertex shader: one draw call brings the
    // whole city grid to life when seen from the freeway or the skyline.
    fn build_traffic(&mut self) {
        let segs = self.street_segs.clone();
        let mut rng = Mulberry32::new(99);
        if segs.is_empty() {
            return;
        }
        let mut start: Vec<f64> = Vec::new();
        let mut dir: Vec<f64> = Vec::new();
        let mut par: Vec<f64> = Vec::new();
        let y = self.ground_y + 0.75;
        for [u0, v0, u1, v1] in segs {
            if rng.next_f64() < 0.25 {
                continue; // quiet streets
            }
            let a = self.w(u0, v0);
            let b = self.w(u1, v1);
            let dx = b[0] - a[0];
            let dz = b[1] - a[1];
            let len = kernel::hypot(dx, dz);
            let nx = -dz / len;
            let nz = dx / len;
            for lane in [-1.0, 1.0] {
                let cars = (len / 55.0 + rng.next_f64() * 2.0).floor();
                // Lane on the right-hand side of travel; lane -1 runs backwards.
                let sx = if lane > 0.0 { a[0] } else { b[0] };
                let sz = if lane > 0.0 { a[1] } else { b[1] };
                let ox = nx * 2.6 * -lane;
                let oz = nz * 2.6 * -lane;
                let ddx = dx * lane;
                let ddz = dz * lane;
                let speed = (7.0 + rng.next_f64() * 7.0) / len;
                let ph0 = rng.next_f64();
                let mut c = 0.0;
                while c < cars {
                    let ph = (ph0 + c / cars + rng.next_f64() * 0.15) % 1.0;
                    for tail in [0.0, 1.0] {
                        let back = if tail != 0.0 { -4.2 / len } else { 0.0 };
                        start.extend_from_slice(&[sx + ox + ddx * back, y, sz + oz + ddz * back]);
                        dir.extend_from_slice(&[ddx, 0.0, ddz]);
                        par.extend_from_slice(&[ph, speed, tail]);
                    }
                    c += 1.0;
                }
            }
        }
        if start.is_empty() {
            return;
        }
        let mut geo = BufferGeometry::new();
        geo.set_attribute("position", BufferAttribute::from_f64(&start, 3));
        geo.set_attribute("aDir", BufferAttribute::from_f64(&dir, 3));
        geo.set_attribute("aPar", BufferAttribute::from_f64(&par, 3));
        geo.bounding_sphere = Some(Sphere {
            center: Vector3::new(0.0, 0.0, 0.0),
            radius: 1e5,
        });
        let mat = self.graph.add_material(
            Material::shader()
                .kind(MaterialKind::TrafficStreams, None)
                .shader_source(TRAFFIC_VERT, TRAFFIC_FRAG)
                .uniform("uTime", num(0.0))
                .uniform("uNight", num(1.0))
                .uniform("uFogK", num(0.0))
                .uniform("uHalfH", num(360.0))
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", three::ADDITIVE_BLENDING as f64),
        );
        let g = self.graph.add_geometry(geo);
        let pts = self.graph.drawable(NodeType::Points, g, mat);
        {
            let o = self.graph.get_mut(pts);
            o.frustum_culled = false;
            o.render_order = 3.0;
        }
        self.graph.add(self.group, pts);
        let mut time = 0.0;
        let mut half_h = 360.0;
        self.anim
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                time += u.dt;
                if let Some(c) = u.camera {
                    half_h = c.viewport_height / 2.0;
                }
                for (prop, value) in [
                    ("uTime", time),
                    ("uNight", smoothstep(0.2, 0.7, u.night)),
                    // `world.scene` has no fog (see glowPointsMaterial).
                    ("uFogK", 0.0 * 0.4),
                    ("uHalfH", half_h),
                ] {
                    out.push(Edit {
                        target: Handle::Material(mat),
                        change: Change::Number { prop, value },
                    });
                }
            }));
    }

    // Light pollution: a flattened dome over the city, seen from inside as a
    // warm haze low on the horizon and from outside as a glow hanging over the
    // skyline, so the gaps between buildings aren't dead black.
    fn build_sky_glow(&mut self) {
        let c = if self.is_loop {
            self.centre
        } else {
            self.led.dc.map(|f| [f.x, f.z])
        };
        let Some(c) = c else {
            return;
        };
        let t = self.track;
        let mut r = 0.0;
        if self.is_loop {
            let mut i = 0;
            while i < t.n {
                r = js::max(
                    r,
                    kernel::hypot(f64::from(t.px[i]) - c[0], f64::from(t.pz[i]) - c[1]),
                );
                i += 50;
            }
        } else {
            r = 1400.0;
        }
        r += 900.0;
        let mut geo = sphere_geometry(1.0, 48.0, 12.0, 0.0, PI * 2.0, 0.0, PI / 2.0);
        geo.scale(r, r * 0.14, r);
        let mat = self.graph.add_material(
            Material::shader()
                .kind(MaterialKind::SkyGlow, None)
                .shader_source(SKY_GLOW_VERT, SKY_GLOW_FRAG)
                .uniform("uK", num(0.0))
                .uniform("uGround", num(self.ground_y))
                .uniform("uH", num(r * 0.05))
                .set("side", three::BACK_SIDE as f64)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", three::ADDITIVE_BLENDING as f64)
                .set("fog", false),
        );
        let g = self.graph.add_geometry(geo);
        let m = self.graph.mesh(g, mat);
        {
            let o = self.graph.get_mut(m);
            o.position = Vector3::new(c[0], self.ground_y - 10.0, c[1]);
            o.frustum_culled = false;
            o.render_order = -1.0;
        }
        self.graph.add(self.group, m);
        self.anim
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                out.push(Edit {
                    target: Handle::Material(mat),
                    change: Change::Number {
                        prop: "uK",
                        value: 0.2 * smoothstep(0.3, 0.8, u.night),
                    },
                });
            }));
    }

    fn build_trees(&mut self) {
        let mut trees = self.park_trees.clone();
        trees.extend(self.fw.park_trees.iter().copied());
        if trees.is_empty() {
            return;
        }
        let mut trunk = cylinder_geometry(0.18, 0.26, 3.2, 5.0, 1.0, false, 0.0, PI * 2.0);
        trunk.translate(0.0, 1.6, 0.0);
        let mut crown = icosahedron_geometry(2.4, 0.0);
        crown.scale(1.0, 1.15, 1.0);
        crown.translate(0.0, 4.6, 0.0);
        let painted = |g0: BufferGeometry, c: P3| {
            let mut g = if g0.index.is_some() {
                g0.to_non_indexed()
            } else {
                g0
            };
            paint(&mut g, c);
            g
        };
        let g1 = painted(trunk, [0.22, 0.14, 0.08]);
        let g2 = painted(crown, [0.13, 0.26, 0.09]);
        let mut geo = merge_two(&g1, &g2);
        geo.compute_vertex_normals();
        let mat = self.graph.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.95)
                .set("flatShading", true),
        );
        let mut ms = Vec::with_capacity(trees.len());
        for t in &trees {
            let yaw = self.rnd() * 6.28;
            let sy = t.s * (0.85 + self.rnd() * 0.3);
            ms.push(trs(t.x, t.y, t.z, yaw, t.s, sy, t.s, 0.0, 0.0));
        }
        let mut cols = Vec::with_capacity(ms.len());
        for _ in 0..ms.len() {
            let h = 0.25 + self.rnd() * 0.08;
            let l = 0.35 + self.rnd() * 0.25;
            let mut c = Color::new(1.0, 1.0, 1.0);
            c.set_hsl(h, 0.5, l);
            cols.push(c);
        }
        self.emit_instanced(geo, mat, &ms, true, true, 1700.0, Some(&cols));
    }

    fn build_aircraft_lights(&mut self) {
        if self.aircraft.is_empty() {
            return;
        }
        let pos: Vec<f64> = self.aircraft.iter().flatten().copied().collect();
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.compute_bounding_sphere();
        let glow = self
            .graph
            .cached_texture(&self.textures.glow_texture(), Layer::Main, "");
        let m = self.graph.add_material(
            Material::points()
                .set("size", 3.5)
                .set("sizeAttenuation", false)
                .set("map", glow)
                .set("color", Color::new(6.0, 0.35, 0.25))
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", three::ADDITIVE_BLENDING as f64),
        );
        let g = self.graph.add_geometry(g);
        let pts = self.graph.drawable(NodeType::Points, g, m);
        self.graph.get_mut(pts).frustum_culled = false;
        self.graph.add(self.group, pts);
        let mut time = 0.0;
        self.anim
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                time += u.dt;
                let blink = if kernel::sin(time * PI * 1.6) > 0.2 {
                    1.0
                } else {
                    0.08
                };
                let on = blink * (0.3 + 0.7 * u.night);
                out.push(Edit {
                    target: Handle::Material(m),
                    change: Change::Color {
                        prop: "color",
                        rgb: [6.0 * on, 0.35 * on, 0.25 * on],
                    },
                });
            }));
    }
}

// The GLSL of the two ShaderMaterials, verbatim from City.js (the export
// carries the text; its hash is compared).
const TRAFFIC_VERT: &str = "\n        uniform float uTime, uFogK, uHalfH;\n        attribute vec3 aDir;\n        attribute vec3 aPar;\n        varying vec3 vCol;\n        varying float vA;\n        void main() {\n          float t = fract(aPar.x + uTime * aPar.y);\n          vec4 mv = modelViewMatrix * vec4(position + aDir * t, 1.0);\n          gl_Position = projectionMatrix * mv;\n          float d = max(-mv.z, 0.1);\n          float raw = 1.5 * projectionMatrix[1][1] * uHalfH / d;\n          gl_PointSize = max(raw, 1.5);\n          // Fade in/out at the block ends (junctions) and close to the camera.\n          vA = sqrt(min(1.0, raw / 1.5)) * exp(-d * uFogK) * smoothstep(25.0, 70.0, d)\n            * smoothstep(0.0, 0.06, t) * smoothstep(1.0, 0.94, t);\n          vCol = aPar.z > 0.5 ? vec3(1.0, 0.1, 0.04) : vec3(1.0, 0.88, 0.66);\n        }";
const TRAFFIC_FRAG: &str = "\n        uniform float uNight;\n        varying vec3 vCol;\n        varying float vA;\n        void main() {\n          vec2 q = gl_PointCoord - 0.5;\n          float a = exp(-dot(q, q) * 14.0) * vA * uNight;\n          gl_FragColor = vec4(vCol * 2.2, a);\n        }";
const SKY_GLOW_VERT: &str = "\n        varying float vY;\n        void main() {\n          vec4 w = modelMatrix * vec4(position, 1.0);\n          vY = w.y;\n          gl_Position = projectionMatrix * viewMatrix * w;\n        }";
const SKY_GLOW_FRAG: &str = "\n        uniform float uK, uGround, uH;\n        varying float vY;\n        void main() {\n          float h = max(vY - uGround, 0.0);\n          float g = exp(-h / uH);\n          vec3 col = mix(vec3(0.5, 0.26, 0.16), vec3(0.22, 0.14, 0.3), smoothstep(0.0, 2.5 * uH, h));\n          gl_FragColor = vec4(col * g * uK, 1.0);\n        }";
