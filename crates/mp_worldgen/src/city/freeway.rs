//! Port of `src/world/city/freeway.js` (roadmap WP 3.8): everything that
//! belongs to the Interstate itself: the westbound carriageway on the other
//! side of the median, the road beyond the finish, lighting, sign gantries,
//! sound walls, overpasses, the Meridian tunnel, billboards and the finish
//! gantry.
//!
//! Lateral layout (metres from our centreline, + = right):
//!   our barrier back   -10.52          median centre  -10.95
//!   opposite barrier   -11.38 … -12.02 opposite centre -21.92
//!   opposite outer     -31.82 … -32.46
//!
//! The JS `ctx` object the city hands the freeway is [`FwCtx`]; the JS
//! `Freeway` instance's state is [`Freeway`]. Static geometry is batched by
//! material and chunk in insertion order (the JS `Map`), as the JS does.

// The JS signatures and loops are kept (DECISIONS D52, D130).
#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]

use std::ops::Deref;
use std::sync::Arc;

use mp_math::{Mulberry32, clamp, js, kernel, lerp, smoothstep};
use mp_scene::three;
use mp_track::{Frame, Tag, Track};

use super::textures::{
    BannerOpts, ad_texture, banner_texture, canvas_of, park_texture, sound_wall_texture,
    tunnel_tile_texture,
};
use super::{Grid, ParkTree};
use crate::color::Color;
use crate::geom::{GeoBuilder, P2, P3, PrismOpts, StaticOpts, instanced, static_mesh, trs, yaw_of};
use crate::material::Material;
use crate::object::{Image, Layer, MaterialId, NodeId, SceneGraph};
use crate::terrain::Terrain;
use crate::textures::{Cached, SignOpts, Texture, TextureCache};
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Matrix4, box_geometry, cylinder_geometry, sphere_geometry,
};
use crate::world::{Animator, Change, Edit, Handle};

const PI: f64 = std::f64::consts::PI;

pub const MED_C: f64 = -10.95;
pub const OPP_FACE_IN: f64 = -12.02;
pub const OPP_C: f64 = -21.92;
pub const OPP_FACE_OUT: f64 = -31.82;
pub const OPP_BACK: f64 = -32.46;
pub const RIGHT_CLEAR: f64 = 10.6;
/// Anything to the right must be beyond this.
const W_DECK: f64 = 13.0;
/// Westbound lane centres (lat), nearest the median first. Traffic on them
/// drives toward -s; surface height is oppY(frame).
pub const OPP_LANES: [f64; 4] = mp_levels::world::OPP_LANES;
const HALF: f64 = 9.4;

// ── The path ─────────────────────────────────────────────────────────────

/// A frame of the path: the track's frame, and whether it lies on one of
/// the straight extensions (`out.ext`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PFrame {
    pub f: Frame,
    pub ext: bool,
}

impl Deref for PFrame {
    type Target = Frame;
    fn deref(&self) -> &Frame {
        &self.f
    }
}

/// A path along the freeway (`makePath`): the track between sA and sB, and
/// straight extensions beyond both ends so the westbound lanes and our lanes
/// past the finish carry on into the distance. On a loop the path is the
/// track itself and s wraps.
#[derive(Clone, Debug, PartialEq)]
pub struct FwPath {
    pub s_a: f64,
    pub s_b: f64,
    pub u0: f64,
    pub u1: f64,
    pub is_loop: bool,
    fa: Frame,
    fb: Frame,
}

impl FwPath {
    /// `makePath(track, sA, sB, back, fwd)`.
    pub fn new(track: &Track, s_a: f64, s_b: f64, back: f64, fwd: f64) -> FwPath {
        if track.is_loop {
            return FwPath {
                s_a: 0.0,
                s_b: track.length,
                u0: 0.0,
                u1: track.length,
                is_loop: true,
                fa: Frame::default(),
                fb: Frame::default(),
            };
        }
        FwPath {
            s_a,
            s_b,
            u0: s_a - back,
            u1: s_b + fwd,
            is_loop: false,
            fa: track.frame(s_a),
            fb: track.frame(s_b - 0.01),
        }
    }

    /// `path.frame(u, out)`.
    pub fn frame(&self, track: &Track, u: f64) -> PFrame {
        if self.is_loop || (u >= self.s_a && u <= self.s_b) {
            return PFrame {
                f: track.frame(u),
                ext: false,
            };
        }
        let b = if u < self.s_a { &self.fa } else { &self.fb };
        let du = u - if u < self.s_a { self.s_a } else { self.s_b };
        PFrame {
            f: Frame {
                x: b.x + b.fx * du,
                z: b.z + b.fz * du,
                y: b.y,
                fx: b.fx,
                fz: b.fz,
                rx: b.rx,
                rz: b.rz,
                hw: HALF,
                bank: 0.0,
                grade: 0.0,
                kappa: 0.0,
                wall_l: 9.9,
                wall_r: 9.9,
                zone: 2,
                s: u,
            },
            ext: true,
        }
    }
}

/// Height of the westbound carriageway: flat across, never below the ground
/// the terrain flattened for our side.
pub fn opp_y(f: &Frame) -> f64 {
    mp_levels::world::opp_y(f)
}

pub fn our_y(f: &Frame, lat: f64) -> f64 {
    f.y - lat * f.bank
}

// ── sweep ────────────────────────────────────────────────────────────────

/// One point of a swept cross-section: `{ lat, y(f, lat) }`.
pub struct Prof<'a> {
    pub lat: f64,
    pub y: Box<dyn Fn(&PFrame, f64) -> f64 + 'a>,
}

/// A profile point.
pub fn prof<'a>(lat: f64, y: impl Fn(&PFrame, f64) -> f64 + 'a) -> Prof<'a> {
    Prof {
        lat,
        y: Box::new(y),
    }
}

/// `uv: 'road'` → (lat/uS, s/vS), `'wall'` → (s/uS, (y - f.y)/vS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepUv {
    Road,
    Wall,
}

/// `sweep`'s options, with the JS defaults.
#[derive(Clone, Copy, Debug)]
pub struct SweepOpts {
    pub step: f64,
    pub uv: SweepUv,
    pub u_s: f64,
    pub v_s: f64,
    pub color: Option<[f64; 3]>,
}

impl Default for SweepOpts {
    fn default() -> Self {
        SweepOpts {
            step: 4.0,
            uv: SweepUv::Road,
            u_s: 4.0,
            v_s: 8.0,
            color: None,
        }
    }
}

/// `{ step }` with the other defaults.
pub fn step(step: f64) -> SweepOpts {
    SweepOpts {
        step,
        ..SweepOpts::default()
    }
}

/// Sweep a cross-section along the path. profile: [{lat, y(f, lat)}] in
/// order of increasing lat for upward faces.
pub fn sweep(
    track: &Track,
    path: &FwPath,
    ranges: &[[f64; 2]],
    profile: &[Prof],
    o: &SweepOpts,
) -> BufferGeometry {
    let mut pos: Vec<f64> = Vec::new();
    let mut uvs: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut col: Vec<f64> = Vec::new();
    let p_n = profile.len();
    for &[a, b] in ranges {
        if b - a < 0.5 {
            continue;
        }
        let base = (pos.len() / 3) as u32;
        let mut rows = 0u32;
        let mut s = a;
        loop {
            let ss = js::min(s, b);
            let f = path.frame(track, ss);
            for pr in profile {
                let lat = pr.lat;
                let y = (pr.y)(&f, lat);
                pos.extend_from_slice(&[f.x + f.rx * lat, y, f.z + f.rz * lat]);
                if o.uv == SweepUv::Wall {
                    uvs.extend_from_slice(&[ss / o.u_s, (y - f.y) / o.v_s]);
                } else {
                    uvs.extend_from_slice(&[lat / o.u_s, ss / o.v_s]);
                }
                if let Some(c) = o.color {
                    col.extend_from_slice(&c);
                }
            }
            rows += 1;
            if ss >= b {
                break;
            }
            s += o.step;
        }
        let pn = p_n as u32;
        for r in 0..rows.saturating_sub(1) {
            for p in 0..pn.saturating_sub(1) {
                let i0 = base + r * pn + p;
                let i1 = i0 + 1;
                let i2 = i0 + pn;
                let i3 = i2 + 1;
                idx.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
            }
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&uvs, 2));
    if o.color.is_some() {
        g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
    }
    g.set_index(&idx);
    g.compute_vertex_normals();
    g.compute_bounding_sphere();
    g
}

/// `chunked(ranges, size = 240)`.
pub fn chunked(ranges: &[[f64; 2]], size: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for &[a, b] in ranges {
        let mut s = a;
        while s < b {
            out.push([s, js::min(b, s + size)]);
            s += size;
        }
    }
    out
}

/// Jersey barrier offsets from its face: [outward, dy].
const JERSEY: [[f64; 2]; 6] = [
    [-0.02, 0.0],
    [0.02, 0.25],
    [0.2, 0.36],
    [0.28, 1.0],
    [0.42, 1.0],
    [0.62, 0.25],
];

// ── Sites ───────────────────────────────────────────────────────────────

/// One sign on a gantry.
#[derive(Clone, Debug, PartialEq)]
pub struct GantrySign {
    pub lines: Vec<String>,
    pub arrow: Option<&'static str>,
    pub lane: f64,
    pub bg: Option<&'static str>,
    pub fg: Option<&'static str>,
    pub border: Option<&'static str>,
}

fn sign(lines: &[&str], arrow: Option<&'static str>, lane: f64) -> GantrySign {
    GantrySign {
        lines: lines.iter().map(|s| s.to_string()).collect(),
        arrow,
        lane,
        bg: None,
        fg: None,
        border: None,
    }
}

/// A yellow tunnel-warning sign.
fn warn(lines: &[&str], lane: f64) -> GantrySign {
    GantrySign {
        bg: Some("#f2c230"),
        fg: Some("#111"),
        border: Some("#111"),
        ..sign(lines, None, lane)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GantrySite {
    pub s: f64,
    pub signs: Vec<GantrySign>,
}

/// A lamp on an overpass, collected with the city street lamps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BridgeLamp {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub dx: f64,
    pub dz: f64,
}

/// Where a grid street crosses the freeway: `{ s, k, fam }`, and once
/// built (not too oblique) its span along the street and its lamps.
#[derive(Clone, Debug, PartialEq)]
pub struct Overpass {
    pub s: f64,
    pub k: f64,
    /// `'u'` (a street of constant U) or `'v'`.
    pub fam_v: bool,
    pub lamps: Vec<BridgeLamp>,
    /// `o.v0`, `o.v1`, `o.U`, set when built.
    pub span: Option<(f64, f64, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BillboardSite {
    pub s: f64,
    pub lat: f64,
    pub h: f64,
}

/// A light pool draped on the road (`{ s, lat, rx, rz, y, c, k }`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pool {
    pub s: f64,
    pub lat: f64,
    pub rx: f64,
    pub rz: f64,
    /// `y: 'opp'`.
    pub opp: bool,
    pub c: [f64; 3],
    pub k: f64,
}

/// A lamp head of the median poles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampHead {
    pub x: f64,
    pub z: f64,
    pub y: f64,
    pub yaw: f64,
    pub c: [f64; 3],
}

/// A lamp of the park on a tunnel's lid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParkLamp {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A quad of `atlasQuads`: `{ image, c, rx, rz, w, h }`. Images are told
/// apart by identity, as the JS `images.includes` does.
#[derive(Clone)]
pub struct AtlasQuad {
    pub image: Arc<Cached>,
    pub c: P3,
    pub rx: f64,
    pub rz: f64,
    pub w: f64,
    pub h: f64,
}

// ── The context ─────────────────────────────────────────────────────────

/// The JS `ctx` the city builds for the freeway.
pub struct FwCtx<'a> {
    pub track: &'a Track,
    pub terrain: &'a Terrain,
    pub path: &'a FwPath,
    pub grid: &'a Grid,
    pub city_y: f64,
    pub zone: usize,
    pub group: NodeId,
    pub graph: &'a mut SceneGraph,
    pub textures: &'a mut TextureCache,
    pub animators: &'a mut Vec<Box<dyn Animator>>,
    pub rng: &'a mut Mulberry32,
    /// `ctx.soundWallSpans` (the loop's residential stretches).
    pub sound_wall_spans: Option<Vec<[f64; 2]>>,
    /// `ctx.lampTint(u)`.
    pub lamp_tint: &'a dyn Fn(f64) -> [f64; 3],
}

impl FwCtx<'_> {
    fn add_night(&mut self, m: MaterialId, prop: &str, day: f64, night: f64) {
        self.graph.add_night(Some(m), prop, day, night);
    }

    fn sweep(&self, ranges: &[[f64; 2]], profile: &[Prof], o: &SweepOpts) -> BufferGeometry {
        sweep(self.track, self.path, ranges, profile, o)
    }
}

struct Batch {
    key: (MaterialId, u64, u64, bool),
    mat: MaterialId,
    cast: bool,
    receive: bool,
    geos: Vec<BufferGeometry>,
}

/// A chunk index as the JS writes it into a key (`Math.floor(c / CH)`;
/// -0 and 0 are one key, NaN is "NaN").
fn key_part(v: f64) -> u64 {
    if v.is_nan() {
        f64::NAN.to_bits()
    } else {
        (v + 0.0).to_bits()
    }
}

/// The freeway (`class Freeway`).
#[derive(Default)]
pub struct Freeway {
    pub is_loop: bool,
    pub tunnels: Vec<Tag>,
    /// s-ranges where median poles must not stand.
    pub reserved: Vec<[f64; 2]>,
    pub gantry_sites: Vec<GantrySite>,
    pub overpasses: Vec<Overpass>,
    pub billboard_sites: Option<Vec<BillboardSite>>,
    pub welcome_s: f64,
    pub tunnel_pools: Vec<Pool>,
    pub park_trees: Vec<ParkTree>,
    pub park_lamps: Vec<ParkLamp>,
    pub tunnel_cols: Vec<Matrix4>,
    pub tunnel_strips: Vec<Matrix4>,
    pub sodium: Option<MaterialId>,
    pub overpass_cols: Vec<Matrix4>,
    pub sign_lamps: Vec<ParkLamp>,
    pub lamp_heads: Vec<LampHead>,
    /// `[x, y, z, amber]`.
    pub reflectors: Vec<([f64; 3], bool)>,
    batches: Vec<Batch>,
    mats: Vec<(&'static str, MaterialId)>,
}

impl Freeway {
    pub fn new() -> Freeway {
        Freeway::default()
    }

    /// Batch static geometry by material and ~700 m chunk; flushed at the end
    /// of build() so the whole freeway costs a handful of draw calls.
    fn add(&mut self, mut geo: BufferGeometry, mat: MaterialId, cast: bool, receive: bool) {
        geo.compute_bounding_sphere();
        let c = geo.bounding_sphere.expect("a bounding sphere").center;
        let ch = if self.is_loop { 1800.0 } else { 700.0 };
        let key = (
            mat,
            key_part((c.x / ch).floor()),
            key_part((c.z / ch).floor()),
            cast,
        );
        match self.batches.iter_mut().find(|b| b.key == key) {
            Some(b) => b.geos.push(geo),
            None => self.batches.push(Batch {
                key,
                mat,
                cast,
                receive,
                geos: vec![geo],
            }),
        }
    }

    fn flush(&mut self, c: &mut FwCtx) {
        for b in std::mem::take(&mut self.batches) {
            let geo = merge_flat(&b.geos);
            let g = c.graph.add_geometry(geo);
            let m = static_mesh(
                c.graph,
                g,
                b.mat,
                &StaticOpts {
                    cast: b.cast,
                    receive: b.receive,
                    ..StaticOpts::default()
                },
            );
            c.graph.add(c.group, m);
        }
    }

    fn mat(
        &mut self,
        c: &mut FwCtx,
        key: &'static str,
        make: impl FnOnce(&mut FwCtx) -> MaterialId,
    ) -> MaterialId {
        if let Some(&(_, m)) = self.mats.iter().find(|(k, _)| *k == key) {
            return m;
        }
        let m = make(c);
        self.mats.push((key, m));
        m
    }

    fn concrete(&mut self, c: &mut FwCtx) -> MaterialId {
        self.mat(c, "concrete", |c| {
            let map = c
                .graph
                .cached_texture(&c.textures.concrete_texture(), Layer::Main, "");
            c.graph.add_material(
                Material::standard()
                    .set("map", map)
                    .set("roughness", 0.9)
                    .set("color", 0xd8d5ce),
            )
        })
    }

    /// Is u within [a, b]? On a loop the range may straddle the seam.
    pub fn near(&self, track: &Track, u: f64, a: f64, b: f64) -> bool {
        if !self.is_loop {
            return u > a && u < b;
        }
        let l = track.length;
        let w = |x: f64| ((x % l) + l) % l;
        let (uu, aa, bb) = (w(u), w(a), w(b));
        if aa <= bb {
            uu > aa && uu < bb
        } else {
            uu > aa || uu < bb
        }
    }

    pub fn reserved_at(&self, track: &Track, u: f64, pad: f64) -> bool {
        self.reserved
            .iter()
            .any(|&[a, b]| self.near(track, u, a - pad, b + pad))
    }

    /// `this.inTunnel(u)`.
    pub fn in_tunnel(&self, track: &Track, u: f64) -> bool {
        self.tunnels
            .iter()
            .any(|t| self.near(track, u, t.s0 - 1.0, t.s1 + 1.0))
    }

    /// `this.elevated(u)`.
    fn elevated(&self, c: &FwCtx, u: f64) -> bool {
        u >= c.path.s_a && u <= c.path.s_b && c.terrain.is_elevated(c.track, c.track.idx(u))
    }

    pub fn build(&mut self, c: &mut FwCtx) {
        let track = c.track;
        self.is_loop = track.is_loop;
        self.tunnels = track.tag("tunnel").into_iter().cloned().collect();
        self.tunnel_pools = Vec::new();
        self.park_trees = Vec::new();
        self.park_lamps = Vec::new();

        if self.is_loop {
            self.pick_loop_sites(c);
        } else {
            self.pick_feature_sites(c);
        }
        self.build_opposite(c);
        self.build_forward_extension(c);
        self.build_sound_walls(c);
        let names: &[&str] = if self.is_loop {
            &["CENTRAL TUNNEL", "HARBOR TUNNEL", "LOOP TUNNEL"]
        } else {
            &["MERIDIAN TUNNEL"]
        };
        for (i, t) in self.tunnels.clone().iter().enumerate() {
            self.build_tunnel(c, Some(t), names[i % names.len()]);
        }
        let mut overs = std::mem::take(&mut self.overpasses);
        for o in &mut overs {
            self.build_overpass(c, o);
        }
        self.overpasses = overs;
        // One instanced mesh each for every tunnel's and overpass's columns and
        // strip lights, however many there are.
        if !self.tunnel_cols.is_empty() {
            let mut g = box_geometry(1.6, 1.0, 0.6, 1.0, 1.0, 1.0);
            g.translate(0.0, 0.5, 0.0);
            let g = c.graph.add_geometry(g);
            let conc = self.concrete(c);
            let m = instanced(c.graph, g, conc, &self.tunnel_cols, false, true);
            c.graph.add(c.group, m);
            let b = c
                .graph
                .add_geometry(box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0));
            let sodium = self.sodium.expect("the tunnels' sodium");
            let m = instanced(c.graph, b, sodium, &self.tunnel_strips, false, false);
            c.graph.add(c.group, m);
        }
        if !self.overpass_cols.is_empty() {
            let mut g = cylinder_geometry(0.38, 0.38, 1.0, 10.0, 1.0, false, 0.0, 2.0 * PI);
            g.translate(0.0, 0.5, 0.0);
            let g = c.graph.add_geometry(g);
            let conc = self.concrete(c);
            let m = instanced(c.graph, g, conc, &self.overpass_cols, true, false);
            c.graph.add(c.group, m);
        }
        self.build_gantries(c);
        if track.finish_s.is_finite() {
            self.build_finish(c, track.finish_s, "FINISH", true);
        } else {
            self.build_finish(c, self.welcome_s, "MERIDIAN LOOP", false);
        }
        self.build_billboards(c);
        self.build_lighting(c);
        self.flush(c);
    }

    // Loop: gantries, overpasses and billboards spread round the whole ring.
    fn pick_loop_sites(&mut self, c: &mut FwCtx) {
        let track = c.track;
        let grid = c.grid;
        let city_y = c.city_y;
        let t_ = c.terrain;
        let l = track.length;
        let signs_table: Vec<[GantrySign; 2]> = vec![
            [
                sign(&["LOOP", "Downtown", "Next 2 exits"], Some("up"), -3.2),
                sign(&["EXIT 7", "Harbor Blvd", "1 MILE"], None, 4.6),
            ],
            [
                sign(&["EXIT 8", "Grand Ave"], Some("right"), 4.6),
                sign(&["LOOP", "Central Tunnel"], Some("up"), -3.2),
            ],
            [
                sign(&["LOOP", "Next exit", "Harbor Blvd"], None, -3.4),
                sign(&["EXIT 9", "Waterfront"], Some("right"), 4.6),
            ],
            [
                sign(&["I-9 WEST", "Coast Hwy"], Some("up"), -3.2),
                sign(&["EXIT 10", "Industrial Pkwy", "1/2 MILE"], None, 4.6),
            ],
            [
                sign(&["LOOP", "Midtown", "Arena"], Some("up"), -3.2),
                sign(&["EXIT 11", "Union Station"], None, 4.6),
            ],
            [
                sign(&["EXIT 12", "5th Street", "1 MILE"], None, 4.6),
                sign(&["LOOP", "City Center"], Some("up"), -3.2),
            ],
            [
                warn(&["HARBOR TUNNEL", "LIGHTS ON"], -3.4),
                sign(&["EXIT 13", "Pier 39"], Some("right"), 4.6),
            ],
            [
                sign(&["EXIT 14", "Airport Rd", "2 MILES"], None, 4.6),
                sign(&["LOOP", "Downtown", "Stay left"], Some("up"), -3.2),
            ],
            [
                sign(&["LOOP", "Meridian Park"], Some("up"), -3.2),
                sign(&["EXIT 15", "University"], Some("right"), 4.6),
            ],
            [
                sign(&["EXIT 16", "Market St", "1/2 MILE"], None, 4.6),
                sign(&["LOOP", "Harbor Blvd"], Some("up"), -3.2),
            ],
            [
                sign(&["LOOP", "Next exit", "Old Town"], None, -3.4),
                sign(&["EXIT 17", "Old Town"], Some("right"), 4.6),
            ],
            [
                sign(&["EXIT 18", "Stadium", "1 MILE"], None, 4.6),
                sign(&["LOOP", "Downtown 3"], Some("up"), -3.2),
            ],
        ];
        self.welcome_s = track.start_s + 360.0;
        self.reserved
            .push([self.welcome_s - 10.0, self.welcome_s + 10.0]);
        for t in &self.tunnels {
            self.reserved.push([t.s0 - 12.0, t.s1 + 12.0]);
        }
        self.gantry_sites = Vec::new();
        let mut k = 0usize;
        let mut s = self.welcome_s + 700.0;
        while s < l + self.welcome_s - 600.0 {
            let mut ss = s;
            // Keep gantries out of tunnels and off the portals.
            let mut tries = 0;
            while tries < 6
                && self
                    .tunnels
                    .iter()
                    .any(|t| self.near(track, ss, t.s0 - 120.0, t.s1 + 40.0))
            {
                ss += 90.0;
                tries += 1;
            }
            // Tunnel-warning sign when a tunnel lies just ahead.
            let ahead = self.tunnels.iter().position(|t| {
                let d = track.ds(ss, t.s0);
                d > 60.0 && d < 600.0
            });
            let signs = match ahead {
                Some(a) => {
                    let name = if a == 0 { "CENTRAL" } else { "HARBOR" };
                    vec![
                        warn(&[&format!("{name} TUNNEL"), "LIGHTS ON"], -3.4),
                        signs_table[k % signs_table.len()][1].clone(),
                    ]
                }
                None => signs_table[k % signs_table.len()].to_vec(),
            };
            self.gantry_sites.push(GantrySite {
                s: track.wrap(ss),
                signs,
            });
            k += 1;
            s += 1050.0;
        }
        for g in &self.gantry_sites {
            self.reserved.push([g.s - 9.0, g.s + 9.0]);
        }

        // Overpasses where either family of grid streets crosses the freeway at
        // ground level, roughly square-on.
        self.overpasses = Vec::new();
        struct Cand {
            s: f64,
            fam_v: bool,
            k: f64,
            rate: f64,
        }
        let mut cands: Vec<Cand> = Vec::new();
        let (mut pu, mut pv): (Option<f64>, Option<f64>) = (None, None);
        let mut s = 0.0;
        while s <= l {
            let f = track.frame(s);
            let u = grid.to_u(f.x, f.z);
            let v = grid.to_v(f.x, f.z);
            if let (Some(pu), Some(pv)) = (pu, pv) {
                let a0 = (pu / grid.pu).floor();
                let a1 = (u / grid.pu).floor();
                if a1 != a0 {
                    cands.push(Cand {
                        s,
                        fam_v: false,
                        k: js::max(a0, a1),
                        rate: grid.v_axis[0] * f.rx + grid.v_axis[1] * f.rz,
                    });
                }
                let b0 = (pv / grid.pv).floor();
                let b1 = (v / grid.pv).floor();
                if b1 != b0 {
                    cands.push(Cand {
                        s,
                        fam_v: true,
                        k: js::max(b0, b1),
                        rate: grid.u_axis[0] * f.rx + grid.u_axis[1] * f.rz,
                    });
                }
            }
            pu = Some(u);
            pv = Some(v);
            s += 1.0;
        }
        let mut taken: Vec<f64> = Vec::new();
        for cd in &cands {
            let s = cd.s;
            if cd.rate.abs() < 0.75 {
                continue;
            }
            let low = [-40.0, 0.0, 40.0].iter().all(|&d| {
                !t_.is_elevated(track, track.idx(s + d))
                    && f64::from(track.py[track.idx(s + d)]) < city_y + 0.35
            });
            if !low
                || self.reserved_at(track, s, 45.0)
                || self
                    .tunnels
                    .iter()
                    .any(|t| self.near(track, s, t.s0 - 80.0, t.s1 + 80.0))
            {
                continue;
            }
            if taken.iter().any(|&q| track.ds(q, s).abs() < 520.0) {
                continue;
            }
            self.overpasses.push(Overpass {
                s,
                k: cd.k,
                fam_v: cd.fam_v,
                lamps: Vec::new(),
                span: None,
            });
            taken.push(s);
        }
        for o in &self.overpasses {
            self.reserved.push([o.s - 16.0, o.s + 16.0]);
        }

        // Billboards, alternating sides, clear of everything else.
        let mut sites = Vec::new();
        let mut side = 1.0;
        let mut s = self.welcome_s + 250.0;
        while s < l + self.welcome_s - 200.0 {
            let ss = track.wrap(s + ((k * 97) % 120) as f64);
            k += 1;
            if self.reserved_at(track, ss, 30.0)
                || self
                    .tunnels
                    .iter()
                    .any(|t| self.near(track, ss, t.s0 - 60.0, t.s1 + 60.0))
            {
                s += 430.0;
                continue;
            }
            sites.push(BillboardSite {
                s: ss,
                lat: if side > 0.0 {
                    19.0 + (k % 3) as f64
                } else {
                    -42.0 - (k % 2) as f64
                },
                h: 11.0 + (k % 4) as f64 * 1.5,
            });
            side = -side;
            s += 430.0;
        }
        self.billboard_sites = Some(sites);
    }

    // Decide where gantries and overpasses go before anything is built, so
    // lighting can leave gaps for them.
    fn pick_feature_sites(&mut self, c: &mut FwCtx) {
        let track = c.track;
        let grid = c.grid;
        let city_y = c.city_y;
        let t = self.tunnels.first().cloned();
        // Sites are anchored to the zone start, the tunnel and the finish so
        // they stay put when earlier parts of the route are edited.
        let z2 = track.zones[c.zone].s0;
        let tun = t.as_ref().map_or(z2 + 1940.0, |t| t.s0);
        let fin = track.finish_s;
        self.gantry_sites = vec![
            GantrySite {
                s: z2 + 491.0,
                signs: vec![
                    sign(&["I-9 WEST", "Downtown Meridian"], Some("up"), 2.2),
                    sign(&["EXIT 41", "Airport Rd", "1 MILE"], None, -4.6),
                ],
            },
            GantrySite {
                s: z2 + 841.0,
                signs: vec![
                    sign(&["EXIT 42", "Main St", "1/2 MILE"], None, 4.6),
                    sign(&["I-9 WEST", "Meridian Tunnel"], Some("up"), -3.2),
                ],
            },
            GantrySite {
                s: z2 + 1401.0,
                signs: vec![
                    sign(&["DOWNTOWN", "NEXT 3 EXITS"], None, -3.4),
                    sign(&["EXIT 42", "Main St"], Some("right"), 4.6),
                ],
            },
            GantrySite {
                s: tun - 80.0,
                signs: vec![
                    warn(&["MERIDIAN TUNNEL", "LIGHTS ON"], -3.4),
                    sign(&["I-9 WEST", "Coast Hwy"], Some("up"), 4.4),
                ],
            },
            GantrySite {
                s: fin - 560.0,
                signs: vec![
                    sign(&["EXIT 43", "Harbor Blvd", "1 MILE"], None, 4.6),
                    sign(&["I-9 WEST", "City Center"], Some("up"), -3.2),
                ],
            },
            GantrySite {
                s: fin - 190.0,
                signs: vec![
                    sign(&["EXIT 44", "Grand Ave"], Some("right"), 4.6),
                    sign(&["I-9 WEST", "Ocean Beach 12"], Some("up"), -3.2),
                ],
            },
        ];
        for g in &self.gantry_sites {
            self.reserved.push([g.s - 9.0, g.s + 9.0]);
        }
        if let Some(t) = &t {
            self.reserved.push([t.s0 - 12.0, t.s1 + 12.0]);
        }
        self.reserved
            .push([track.finish_s - 10.0, track.finish_s + 10.0]);

        // Overpasses where grid cross streets meet the freeway at ground level.
        self.overpasses = Vec::new();
        let mut prev_u: Option<f64> = None;
        let mut cands: Vec<(f64, f64)> = Vec::new();
        let mut s = z2 + 500.0;
        while s < track.length - 20.0 {
            let f = track.frame(s);
            let u = grid.to_u(f.x, f.z);
            if let Some(pu) = prev_u {
                let k0 = (pu / grid.pu).floor();
                let k1 = (u / grid.pu).floor();
                if k1 != k0 {
                    cands.push((s, js::max(k0, k1)));
                }
            }
            prev_u = Some(u);
            s += 1.0;
        }
        let mut last_s = -1e9;
        for &(s, k) in &cands {
            let low = [-40.0, 0.0, 40.0]
                .iter()
                .all(|&d| f64::from(track.py[track.idx(s + d)]) < city_y + 0.35);
            let clear = self
                .reserved
                .iter()
                .all(|&[a, b]| s < a - 45.0 || s > b + 45.0);
            let in_tunnel = t
                .as_ref()
                .is_some_and(|t| s > t.s0 - 80.0 && s < t.s1 + 80.0);
            if !low || !clear || in_tunnel || s - last_s < 380.0 {
                continue;
            }
            self.overpasses.push(Overpass {
                s,
                k,
                fam_v: false,
                lamps: Vec::new(),
                span: None,
            });
            last_s = s;
        }
        for o in &self.overpasses {
            self.reserved.push([o.s - 16.0, o.s + 16.0]);
        }
    }

    // ── Westbound carriageway ──────────────────────────────────────
    fn build_opposite(&mut self, c: &mut FwCtx) {
        let path = c.path;
        let asphalt = self.mat(c, "asphaltOpp", |c| {
            let map = c
                .graph
                .cached_texture(&c.textures.asphalt_texture(2), Layer::Main, "");
            c.graph
                .add_material(Material::standard().set("map", map).set("roughness", 0.88))
        });
        let conc = self.concrete(c);
        let all = [[path.u0, path.u1]];
        let mut ground: Vec<[f64; 2]> = Vec::new();
        let mut elev: Vec<[f64; 2]> = Vec::new();
        // Split into ground/elevated runs.
        let mut cur: Option<f64> = None;
        let mut cur_e: Option<bool> = None;
        let mut u = path.u0;
        while u <= path.u1 {
            let e = self.elevated(c, u);
            if cur_e.is_none() || Some(e) != cur_e {
                // `if (cur)`: a run starting at u = 0 (the loop's first) is
                // falsy, so the JS drops it.
                if let Some(cu) = cur
                    && cu != 0.0
                {
                    if cur_e == Some(true) {
                        elev.push([cu, u]);
                    } else {
                        ground.push([cu, u]);
                    }
                }
                cur = Some(u);
                cur_e = Some(e);
            }
            u += 2.0;
        }
        if let Some(cu) = cur {
            if cur_e == Some(true) {
                elev.push([cu, path.u1]);
            } else {
                ground.push([cu, path.u1]);
            }
        }

        // Surface.
        for r in chunked(&all, 260.0) {
            let g = c.sweep(
                &[r],
                &[
                    prof(OPP_FACE_OUT, |f, _| opp_y(f)),
                    prof(OPP_C, |f, _| opp_y(f)),
                    prof(OPP_FACE_IN, |f, _| opp_y(f)),
                ],
                &SweepOpts {
                    step: 4.0,
                    u_s: 4.8,
                    v_s: 10.0,
                    ..SweepOpts::default()
                },
            );
            self.add(g, asphalt, false, true);
        }
        // Median: opposite barrier + strip back to our barrier. Before sA there
        // is no eastbound road, so the barrier stands alone.
        let med = || -> Vec<Prof<'static>> {
            JERSEY
                .iter()
                .map(|&[o, dy]| prof(OPP_FACE_IN + o, move |f, _| opp_y(f) + dy))
                .collect()
        };
        let mut med_profile = med();
        med_profile.push(prof(-11.38, |f, _| opp_y(f) + 0.25));
        med_profile.push(prof(-10.52, |f, _| our_y(f, -10.52) + 0.25));
        let mut med_alone = vec![prof(OPP_FACE_IN, |f, _| opp_y(f) - 0.7)];
        med_alone.extend(med());
        med_alone.push(prof(-11.32, |f, _| opp_y(f) + 0.25));
        med_alone.push(prof(-11.28, |f, _| opp_y(f)));
        med_alone.push(prof(-11.28, |f, _| opp_y(f) - 0.7));
        let with_east = [[js::max(path.u0, path.s_a), path.u1]];
        let alone: Vec<[f64; 2]> = if path.u0 < path.s_a {
            vec![[path.u0, path.s_a]]
        } else {
            Vec::new()
        };
        for r in chunked(&with_east, 300.0) {
            let g = c.sweep(&[r], &med_profile, &step(4.0));
            self.add(g, conc, true, true);
        }
        for r in alone {
            let g = c.sweep(&[r], &med_alone, &step(4.0));
            self.add(g, conc, true, true);
        }

        // Outer barrier (+ skirt on the ground, fascia when elevated).
        let outer = |skirt: bool| -> Vec<Prof<'static>> {
            let mut v = if skirt {
                vec![
                    prof(OPP_BACK - 3.2, |f, _| opp_y(f) - 2.8),
                    prof(OPP_BACK, |f, _| opp_y(f) - 0.02),
                ]
            } else {
                vec![prof(OPP_BACK, |f, _| opp_y(f) - 1.8)]
            };
            for &[o, dy] in JERSEY.iter().rev() {
                v.push(prof(OPP_FACE_OUT - o, move |f, _| opp_y(f) + dy));
            }
            v
        };
        for r in chunked(&ground, 300.0) {
            let g = c.sweep(&[r], &outer(true), &step(4.0));
            self.add(g, conc, true, true);
        }
        for r in chunked(&elev, 300.0) {
            let g = c.sweep(&[r], &outer(false), &step(4.0));
            self.add(g, conc, true, true);
        }

        // Deck underside and piers where elevated.
        for r in chunked(&elev, 300.0) {
            let g = c.sweep(
                &[r],
                &[
                    prof(-10.52, |f, _| our_y(f, -10.52) - 1.8),
                    prof(OPP_BACK, |f, _| opp_y(f) - 1.8),
                ],
                &step(4.0),
            );
            self.add(g, conc, false, true);
        }
        self.build_opposite_piers(c);
        self.build_opposite_markings(c);
    }

    fn build_opposite_piers(&mut self, c: &mut FwCtx) {
        let track = c.track;
        let t_ = c.terrain;
        // Same spacing rule as Road.buildViaduct so piers line up in rows.
        let mut runs: Vec<[f64; 2]> = Vec::new();
        let mut start: Option<f64> = None;
        let mut s = 0.0;
        while s <= track.length {
            let i = track.idx(s);
            let ok = track.zone[i] as usize == c.zone && t_.is_elevated(track, i);
            if ok && start.is_none() {
                start = Some(s);
            }
            if !ok && let Some(st) = start {
                runs.push([st, s]);
                start = None;
            }
            s += 2.0;
        }
        if let Some(st) = start {
            runs.push([st, track.length]);
        }
        let mut cols = Vec::new();
        let mut caps = Vec::new();
        for &[s0, s1] in &runs {
            let mut s = s0 + 15.0;
            while s < s1 - 5.0 {
                let f = track.frame(s);
                let top = opp_y(&f) - 2.8;
                let yaw = yaw_of(f.fx, f.fz);
                for lat in [OPP_C - HALF * 0.55, OPP_C + HALF * 0.55] {
                    let x = f.x + f.rx * lat;
                    let z = f.z + f.rz * lat;
                    let gy = t_.height_at(x, z) - 0.5;
                    cols.push(trs(
                        x,
                        gy,
                        z,
                        yaw,
                        1.0,
                        js::max(0.5, top - gy),
                        1.0,
                        0.0,
                        0.0,
                    ));
                }
                let cx = f.x + f.rx * OPP_C;
                let cz = f.z + f.rz * OPP_C;
                caps.push(trs(
                    cx,
                    top + 0.4,
                    cz,
                    yaw,
                    1.0,
                    1.0,
                    HALF * 2.0 + 2.0,
                    0.0,
                    0.0,
                ));
                s += 32.0;
            }
        }
        if cols.is_empty() {
            return;
        }
        let mut col_geo = cylinder_geometry(1.1, 1.3, 1.0, 10.0, 1.0, false, 0.0, 2.0 * PI);
        col_geo.translate(0.0, 0.5, 0.0);
        let cap_geo = box_geometry(2.2, 1.2, 1.0, 1.0, 1.0, 1.0);
        let conc = self.concrete(c);
        let cg = c.graph.add_geometry(col_geo);
        let m = instanced(c.graph, cg, conc, &cols, true, true);
        c.graph.add(c.group, m);
        let kg = c.graph.add_geometry(cap_geo);
        let m = instanced(c.graph, kg, conc, &caps, true, true);
        c.graph.add(c.group, m);
    }

    fn build_opposite_markings(&mut self, c: &mut FwCtx) {
        let path = c.path;
        let white = [0.92, 0.92, 0.9];
        let yellow = [0.95, 0.72, 0.12];
        let mut lines = vec![
            Line {
                lat: OPP_C + HALF - 1.2,
                w: 0.15,
                c: yellow,
                dash: None,
            },
            Line {
                lat: OPP_C - HALF + 2.0,
                w: 0.18,
                c: white,
                dash: None,
            },
        ];
        let lw = ((OPP_C + HALF - 1.2) - (OPP_C - HALF + 2.0)) / 4.0;
        for q in 1..4 {
            lines.push(Line {
                lat: OPP_C + HALF - 1.2 - lw * q as f64,
                w: 0.14,
                c: white,
                dash: Some([3.0, 12.0]),
            });
        }
        // On a loop run one metre past the end so the last quad closes the ring.
        let end = path.u1 + if path.is_loop { 1.0 } else { 0.0 };
        self.add_markings(c, &lines, &[[path.u0, end]], &|f, _| opp_y(f));
    }

    fn add_markings(
        &mut self,
        c: &mut FwCtx,
        lines: &[Line],
        ranges: &[[f64; 2]],
        y_of: &dyn Fn(&PFrame, f64) -> f64,
    ) {
        let path = c.path;
        let mut pos: Vec<f64> = Vec::new();
        let mut col: Vec<f64> = Vec::new();
        for &[a, b] in ranges {
            let mut s = a;
            while s < b - 1.0 {
                let f0 = path.frame(c.track, s);
                let f1 = path.frame(c.track, s + 1.0);
                for ln in lines {
                    if let Some([d0, d1]) = ln.dash
                        && ((s + 0.5) % d1 + d1) % d1 > d0
                    {
                        continue;
                    }
                    let mut q: Vec<P3> = Vec::with_capacity(4);
                    for fr in [&f0, &f1] {
                        for side in [-1.0, 1.0] {
                            let lat = ln.lat + side * ln.w * 0.5;
                            q.push([fr.x + fr.rx * lat, y_of(fr, lat) + 0.02, fr.z + fr.rz * lat]);
                        }
                    }
                    // q: [f0-left, f0-right, f1-left, f1-right] → two tris facing up.
                    for v in [q[0], q[1], q[2], q[1], q[3], q[2]] {
                        pos.extend_from_slice(&v);
                        col.extend_from_slice(&ln.c);
                    }
                }
                s += 1.0;
            }
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
        g.compute_vertex_normals();
        g.compute_bounding_sphere();
        let m = self.mat(c, "markings", |c| {
            let mm = c.graph.add_material(
                Material::standard()
                    .set("vertexColors", true)
                    .set("roughness", 0.55)
                    .set("emissive", 0xffffff)
                    .set("emissiveIntensity", 0.0)
                    .set("polygonOffset", true)
                    .set("polygonOffsetFactor", -2.0)
                    .set("polygonOffsetUnits", -2.0),
            );
            c.add_night(mm, "emissiveIntensity", 0.0, 0.06);
            mm
        });
        self.add(g, m, false, true);
    }

    // ── Our lanes continuing past the end of the track ─────────────
    fn build_forward_extension(&mut self, c: &mut FwCtx) {
        let path = c.path;
        if path.u1 <= path.s_b {
            return;
        }
        let r = [[path.s_b - 0.5, path.u1]];
        let asphalt = self.mat(c, "asphaltOur", |c| {
            let map = c
                .graph
                .cached_texture(&c.textures.asphalt_texture(2), Layer::Main, "");
            c.graph
                .add_material(Material::standard().set("map", map).set("roughness", 0.88))
        });
        let g = c.sweep(
            &r,
            &[
                prof(-10.25, |f, _| f.y - 1.6),
                prof(-10.25, |f, _| f.y),
                prof(0.0, |f, _| f.y),
                prof(10.25, |f, _| f.y),
                prof(10.25, |f, _| f.y - 1.6),
            ],
            &SweepOpts {
                step: 6.0,
                u_s: 4.8,
                v_s: 10.0,
                ..SweepOpts::default()
            },
        );
        self.add(g, asphalt, false, true);
        let conc = self.concrete(c);
        let mut right: Vec<Prof> = JERSEY
            .iter()
            .map(|&[o, dy]| prof(9.9 + o, move |f, _| f.y + dy))
            .collect();
        right.push(prof(10.52, |f, _| f.y - 1.8));
        let mut left = vec![prof(-10.52, |f, _| f.y - 1.8)];
        for &[o, dy] in JERSEY.iter().rev() {
            left.push(prof(-9.9 - o, move |f, _| f.y + dy));
        }
        let g = c.sweep(&r, &right, &step(6.0));
        self.add(g, conc, true, true);
        let g = c.sweep(&r, &left, &step(6.0));
        self.add(g, conc, true, true);
        let white = [0.92, 0.92, 0.9];
        let yellow = [0.95, 0.72, 0.12];
        let l = -HALF + 1.2;
        let rr = HALF - 2.0;
        let lw = (rr - l) / 4.0;
        let mut lines = vec![
            Line {
                lat: l,
                w: 0.15,
                c: yellow,
                dash: None,
            },
            Line {
                lat: rr,
                w: 0.18,
                c: white,
                dash: None,
            },
        ];
        for q in 1..4 {
            lines.push(Line {
                lat: l + lw * q as f64,
                w: 0.14,
                c: white,
                dash: Some([3.0, 12.0]),
            });
        }
        self.add_markings(c, &lines, &r, &|f, _| f.y);
    }

    // ── Sound walls along the outskirts ───────────────────────────
    fn build_sound_walls(&mut self, c: &mut FwCtx) {
        let track = c.track;
        let city_y = c.city_y;
        // Level 1: the outskirts stretch. Loop: whatever the city asked for.
        let spans = match c.sound_wall_spans.clone() {
            Some(s) => s,
            None => {
                let out = track.tag("outskirts").first().map(|t| (*t).clone());
                let merge = track.tag("merge").first().map(|t| (*t).clone());
                let Some(out) = out else {
                    return;
                };
                vec![[merge.map_or(out.s0, |m| m.s0 + 140.0), out.s1 + 60.0]]
            }
        };
        let ok = |s: f64| {
            f64::from(track.py[track.idx(s)]) < city_y + 1.0
                && !c.terrain.is_elevated(track, track.idx(s))
        };
        let mut ranges: Vec<[f64; 2]> = Vec::new();
        for &[s0, s1] in &spans {
            let mut st: Option<f64> = None;
            let mut s = s0;
            while s <= s1 {
                let good = ok(s) && !self.reserved_at(track, s, 4.0) && !self.in_tunnel(track, s);
                if good && st.is_none() {
                    st = Some(s);
                }
                if !good && let Some(a) = st {
                    ranges.push([a, s]);
                    st = None;
                }
                s += 2.0;
            }
            if let Some(a) = st {
                ranges.push([a, s1]);
            }
        }
        let mat = self.mat(c, "soundwall", |c| {
            let map = c
                .graph
                .cached_texture(&sound_wall_texture(c.textures), Layer::Main, "");
            c.graph
                .add_material(Material::standard().set("map", map).set("roughness", 0.95))
        });
        const H: f64 = 5.2;
        let right = [
            prof(11.3, |f, _| our_y(f, 10.5) - 0.8),
            prof(11.3, |f, _| our_y(f, 10.5) + H),
            prof(11.65, |f, _| our_y(f, 10.5) + H),
            prof(11.65, |f, _| our_y(f, 10.5) - 0.8),
        ];
        let left = [
            prof(OPP_BACK - 1.2, |f, _| opp_y(f) - 0.8),
            prof(OPP_BACK - 1.2, |f, _| opp_y(f) + H),
            prof(OPP_BACK - 0.85, |f, _| opp_y(f) + H),
            prof(OPP_BACK - 0.85, |f, _| opp_y(f) - 0.8),
        ];
        let wall = SweepOpts {
            step: 4.0,
            uv: SweepUv::Wall,
            u_s: 6.0,
            v_s: 6.0,
            color: None,
        };
        for r in chunked(&ranges, 200.0) {
            for p in [&right[..], &left[..]] {
                let g = c.sweep(&[r], p, &wall);
                self.add(g, mat, true, true);
            }
        }
    }

    // ── The Meridian tunnel (cut-and-cover with a park on the lid) ──
    fn build_tunnel(&mut self, c: &mut FwCtx, t: Option<&Tag>, name: &str) {
        let path = c.path;
        let Some(t) = t else {
            return;
        };
        let r = [[t.s0, t.s1]];
        let tiles = self.mat(c, "tunnelTiles", |c| {
            let tex = c
                .graph
                .cached_texture(&tunnel_tile_texture(c.textures), Layer::Main, "");
            c.graph.add_material(
                Material::standard()
                    .set("map", tex)
                    .set("roughness", 0.3)
                    .set("metalness", 0.0)
                    .set("color", 0xf4f0e6)
                    .set("emissive", 0xffa860)
                    .set("emissiveMap", tex)
                    .set("emissiveIntensity", 0.12),
            )
        });
        let conc = self.concrete(c);
        const CEIL: f64 = 7.0;
        const TOP: f64 = 8.4;
        // Heights from the high edge of the (possibly banked) roadway.
        fn yb(f: &Frame) -> f64 {
            f.y + f.bank.abs() * 10.5
        }
        let wall = |u_s: f64| SweepOpts {
            step: 4.0,
            uv: SweepUv::Wall,
            u_s,
            v_s: u_s,
            color: None,
        };
        // Right wall: inner face at +11.4. Profile ascending lat, going up
        // at the inner face (normal faces the road).
        let g = c.sweep(
            &r,
            &[
                prof(11.4, |f, _| f.y - 1.0),
                prof(11.4, |f, _| yb(f) + CEIL),
            ],
            &wall(4.0),
        );
        self.add(g, tiles, false, true);
        // Left wall inner face at -33.0 faces +lat: profile going down.
        let g = c.sweep(
            &r,
            &[
                prof(-33.0, |f, _| yb(f) + CEIL),
                prof(-33.0, |f, _| f.y - 1.0),
            ],
            &wall(4.0),
        );
        self.add(g, tiles, false, true);
        // Ceiling underside (faces down → decreasing lat).
        let ceil_mat = self.mat(c, "tunnelCeil", |c| {
            c.graph.add_material(
                Material::standard()
                    .set("color", 0x3a3632)
                    .set("roughness", 0.9)
                    .set("emissive", 0x2a1606)
                    .set("emissiveIntensity", 1.0),
            )
        });
        // The ceiling faces down, so seen from the moon it shows its back face —
        // which is the side three.js renders into the shadow map. It shades the
        // tunnel (the lid's park trees would otherwise cast through).
        let g = c.sweep(
            &r,
            &[
                prof(11.4, |f, _| yb(f) + CEIL),
                prof(-33.0, |f, _| yb(f) + CEIL),
            ],
            &step(4.0),
        );
        self.add(g, ceil_mat, true, true);
        // Lid: outer walls + park top.
        let park = self.mat(c, "park", |c| {
            let map = c
                .graph
                .cached_texture(&park_texture(c.textures), Layer::Main, "");
            c.graph
                .add_material(Material::standard().set("map", map).set("roughness", 1.0))
        });
        // The lid casts the moon's shadow into the tunnel (and hides the park
        // trees' shadows from it).
        let g = c.sweep(
            &r,
            &[
                prof(-36.0, |f, _| yb(f) + TOP),
                prof(14.5, |f, _| yb(f) + TOP),
            ],
            &SweepOpts {
                step: 4.0,
                u_s: 6.0,
                v_s: 6.0,
                ..SweepOpts::default()
            },
        );
        self.add(g, park, true, true);
        // Outer faces of the lid box (right: normal +lat → profile going down; left: going up).
        let g = c.sweep(
            &r,
            &[
                prof(14.5, |f, _| yb(f) + TOP + 1.1),
                prof(14.5, |f, _| f.y - 1.2),
            ],
            &wall(5.0),
        );
        self.add(g, conc, true, true);
        let g = c.sweep(
            &r,
            &[
                prof(-36.0, |f, _| f.y - 1.2),
                prof(-36.0, |f, _| yb(f) + TOP + 1.1),
            ],
            &wall(5.0),
        );
        self.add(g, conc, true, true);
        // Parapets on the lid edges (inner faces).
        let g = c.sweep(
            &r,
            &[
                prof(14.1, |f, _| yb(f) + TOP),
                prof(14.1, |f, _| yb(f) + TOP + 1.1),
                prof(14.5, |f, _| yb(f) + TOP + 1.1),
            ],
            &wall(5.0),
        );
        self.add(g, conc, false, true);
        let g = c.sweep(
            &r,
            &[
                prof(-36.0, |f, _| yb(f) + TOP + 1.1),
                prof(-35.6, |f, _| yb(f) + TOP + 1.1),
                prof(-35.6, |f, _| yb(f) + TOP),
            ],
            &wall(5.0),
        );
        self.add(g, conc, false, true);

        // Portals: a band from the ceiling to above the lid, both ends, with
        // wing walls down to the ground at the outer walls.
        let mut geo = GeoBuilder::new(false, false);
        for (s, dir) in [(t.s0, -1.0), (t.s1, 1.0)] {
            let f = path.frame(c.track, s);
            let p = |lat: f64, dy: f64, along: f64| -> P3 {
                [
                    f.x + f.rx * lat + f.fx * along,
                    f.y + dy,
                    f.z + f.rz * lat + f.fz * along,
                ]
            };
            let d = dir * 0.02;
            // Face points along dir*F. Quad CCW seen from outside.
            // Seen from outside, +lat is on the viewer's right at the entrance
            // (dir < 0) and on the left at the exit — start bottom-left, go CCW.
            let pos = dir > 0.0;
            let a = p(if pos { 14.5 } else { -36.0 }, CEIL, d);
            let b = p(if pos { -36.0 } else { 14.5 }, CEIL, d);
            let cc = p(if pos { -36.0 } else { 14.5 }, TOP + 1.1, d);
            let dd = p(if pos { 14.5 } else { -36.0 }, TOP + 1.1, d);
            geo.quad(
                a,
                b,
                cc,
                dd,
                Some([[0.0, 0.0], [12.0, 0.0], [12.0, 0.6], [0.0, 0.6]]),
                None,
                0.0,
            );
            // Wing columns at both sides (the lid ends are also the outer faces).
            for (l0, l1) in [(11.4, 14.5), (-36.0, -33.0)] {
                let a = p(if pos { l1 } else { l0 }, -1.0, d);
                let b = p(if pos { l0 } else { l1 }, -1.0, d);
                let cc = p(if pos { l0 } else { l1 }, CEIL, d);
                let dd = p(if pos { l1 } else { l0 }, CEIL, d);
                geo.quad(
                    a,
                    b,
                    cc,
                    dd,
                    Some([[0.0, 0.0], [1.0, 0.0], [1.0, 2.0], [0.0, 2.0]]),
                    None,
                    0.0,
                );
            }
            // Median columns at the portal.
        }
        self.add(geo.build(), conc, true, true);

        // Portal name sign above our lanes at the entrance.
        let f = path.frame(c.track, t.s0);
        let sign_tex = banner_texture(
            name,
            &BannerOpts {
                w: 1024,
                h: 160,
                bg: "#0d4f2e",
                fg: "#ffffff",
                font: "bold 96px \"Arial Narrow\", Arial, sans-serif",
                ..BannerOpts::default()
            },
        );
        let tex = own_texture(c.graph, sign_tex);
        let sign_mat = c.graph.add_material(
            Material::standard()
                .set("map", tex)
                .set("emissive", 0xffffff)
                .set("emissiveMap", tex)
                .set("emissiveIntensity", 0.3)
                .set("roughness", 0.6),
        );
        c.add_night(sign_mat, "emissiveIntensity", 0.1, 0.45);
        let pg = c
            .graph
            .add_geometry(crate::three_geom::plane_geometry(14.0, 2.2, 1.0, 1.0));
        let sgn = c.graph.mesh(pg, sign_mat);
        {
            let o = c.graph.get_mut(sgn);
            o.position = crate::three_geom::Vector3::new(
                f.x - f.fx * 0.08 + f.rx * 0.0,
                f.y + CEIL + 0.75,
                f.z - f.fz * 0.08 + f.rz * 0.0,
            );
            o.set_rotation(&crate::three_geom::Euler::new(
                0.0,
                kernel::atan2(-f.fx, -f.fz),
                0.0,
            ));
            o.update_matrix();
            o.matrix_auto_update = false;
        }
        c.graph.add(c.group, sgn);

        // Median columns.
        let mut cols = Vec::new();
        let mut s = t.s0 + 4.0;
        while s < t.s1 - 2.0 {
            let f = path.frame(c.track, s);
            let x = f.x + f.rx * MED_C;
            let z = f.z + f.rz * MED_C;
            cols.push(trs(
                x,
                f.y - 0.3,
                z,
                yaw_of(f.fx, f.fz),
                1.0,
                yb(&f) + CEIL - f.y + 0.3,
                1.0,
                0.0,
                0.0,
            ));
            s += 9.0;
        }
        self.tunnel_cols.extend(cols);

        // Sodium strip lights along the ceiling and warm pools on the road.
        let lamp = self.mat(c, "sodium", |c| {
            c.graph
                .add_material(Material::basic().set("color", Color::new(4.0, 1.9, 0.55)))
        });
        let mut strips = Vec::new();
        let mut s = t.s0 + 2.0;
        while s < t.s1 - 2.0 {
            let f = path.frame(c.track, s);
            for lat in [-4.6, 4.2, OPP_C - 4.6, OPP_C + 4.6] {
                strips.push(trs(
                    f.x + f.rx * lat,
                    yb(&f) + CEIL - 0.12,
                    f.z + f.rz * lat,
                    yaw_of(f.fx, f.fz),
                    2.3,
                    0.12,
                    0.4,
                    0.0,
                    0.0,
                ));
            }
            s += 3.6;
        }
        // Lamps along the underside of both portal lintels.
        for (s, dir) in [(t.s0, -1.0), (t.s1, 1.0)] {
            let f = path.frame(c.track, s);
            let mut lat = -32.0;
            while lat <= 10.5 {
                if (lat - MED_C).abs() >= 1.2 {
                    strips.push(trs(
                        f.x + f.rx * lat + f.fx * dir * 0.35,
                        yb(&f) + CEIL - 0.1,
                        f.z + f.rz * lat + f.fz * dir * 0.35,
                        yaw_of(f.fx, f.fz),
                        0.5,
                        0.14,
                        1.4,
                        0.0,
                        0.0,
                    ));
                }
                lat += 2.6;
            }
        }
        self.tunnel_strips.extend(strips);
        self.sodium = Some(lamp);
        let mut s = t.s0 + 3.0;
        while s < t.s1 - 3.0 {
            let pool = |lat: f64, opp: bool| Pool {
                s,
                lat,
                rx: 12.0,
                rz: 7.0,
                opp,
                c: [1.0, 0.55, 0.18],
                k: 0.2,
            };
            self.tunnel_pools.push(pool(0.0, false));
            self.tunnel_pools.push(pool(OPP_C, true));
            s += 7.0;
        }

        // Park on the lid: trees, paths and lamps.
        let mut s = t.s0 + 8.0;
        while s < t.s1 - 8.0 {
            for _ in 0..2 {
                let lat = lerp(-33.0, 12.0, c.rng.next_f64());
                if (lat - (-10.0)).abs() < 3.0 {
                    continue; // a path down the middle
                }
                let f = path.frame(c.track, s + (c.rng.next_f64() - 0.5) * 6.0);
                let x = f.x + f.rx * lat;
                let y = f.y + TOP;
                let z = f.z + f.rz * lat;
                let sc = 0.8 + c.rng.next_f64() * 0.6;
                self.park_trees.push(ParkTree { x, y, z, s: sc });
            }
            s += 7.0;
        }
        let path_mat = self.mat(c, "parkPath", |c| {
            c.graph.add_material(
                Material::standard()
                    .set("color", 0xa89f8e)
                    .set("roughness", 1.0)
                    .set("polygonOffset", true)
                    .set("polygonOffsetFactor", -1.0),
            )
        });
        let g = c.sweep(
            &r,
            &[
                prof(-12.2, |ff, _| ff.y + TOP + 0.02),
                prof(-8.2, |ff, _| ff.y + TOP + 0.02),
            ],
            &step(6.0),
        );
        self.add(g, path_mat, false, true);
        let mut s = t.s0 + 10.0;
        while s < t.s1 - 5.0 {
            for lat in [-13.0, -7.4] {
                let f = path.frame(c.track, s);
                self.park_lamps.push(ParkLamp {
                    x: f.x + f.rx * lat,
                    y: f.y + TOP,
                    z: f.z + f.rz * lat,
                });
            }
            s += 26.0;
        }
    }

    // ── Overpasses carrying cross streets over the freeway ────────
    fn build_overpass(&mut self, c: &mut FwCtx, o: &mut Overpass) {
        let track = c.track;
        let grid = c.grid;
        let city_y = c.city_y;
        let f = track.frame(o.s);
        // The street is a grid line: constant U (runs along V) or constant V
        // (runs along U). `v` below is the coordinate along the street.
        let fam_u = !o.fam_v;
        let u_line = if fam_u { o.k * grid.pu } else { o.k * grid.pv };
        let v0 = if fam_u {
            grid.to_v(f.x, f.z)
        } else {
            grid.to_u(f.x, f.z)
        };
        let p = |v: f64| -> P2 {
            if fam_u {
                grid.to_world(u_line, v)
            } else {
                grid.to_world(v, u_line)
            }
        };
        // lat along the line changes at this rate per metre of v.
        let lat_at = |v: f64| {
            let q = p(v);
            (q[0] - f.x) * f.rx + (q[1] - f.z) * f.rz
        };
        let rate = lat_at(v0 + 1.0) - lat_at(v0);
        if rate.abs() < 0.5 {
            return; // too oblique
        }
        let v_for_lat = |lat: f64| v0 + (lat - lat_at(v0)) / rate;
        // On the loop streets cross at an angle, so the deck's corners reach
        // further across than its centreline: push the abutments out to match.
        let dir_u = if fam_u { grid.u_axis } else { grid.v_axis }; // across the deck
        let skew = if self.is_loop {
            (W_DECK / 2.0) * (dir_u[0] * f.rx + dir_u[1] * f.rz).abs()
        } else {
            0.0
        };
        let v_r = v_for_lat(13.2 + skew);
        let v_l = v_for_lat(OPP_BACK - 2.2 - skew);
        let v_a = js::min(v_r, v_l);
        let v_b = js::max(v_r, v_l);
        let road = f.y + js::max(0.6, f.bank.abs() * 10.5 + 0.1);
        let bottom = road + 6.9;
        let top = bottom + 1.3;
        let ground = city_y - 0.25;
        let ramp_len = (top - ground) / 0.07;
        const W: f64 = 13.0; // deck width
        let mut geo = GeoBuilder::new(true, false);
        let deck = [0.23, 0.23, 0.25];
        let side = [0.72, 0.7, 0.66];
        let under = [0.45, 0.44, 0.42];
        // Height profile along v: ramp up, span, ramp down.
        let h = |v: f64| {
            if v < v_a {
                return lerp(ground, top, clamp(1.0 - (v_a - v) / ramp_len, 0.0, 1.0));
            }
            if v > v_b {
                return lerp(ground, top, clamp(1.0 - (v - v_b) / ramp_len, 0.0, 1.0));
            }
            top
        };
        let mut vs = Vec::new();
        let mut v = v_a - ramp_len;
        while v < v_a {
            vs.push(v);
            v += ramp_len / 6.0;
        }
        vs.push(v_a);
        let mut v = v_a + 6.0;
        while v < v_b {
            vs.push(v);
            v += 6.0;
        }
        vs.push(v_b);
        let mut v = v_b + ramp_len / 6.0;
        while v <= v_b + ramp_len + 0.01 {
            vs.push(v);
            v += ramp_len / 6.0;
        }
        let edge = |v: f64, s: f64| -> P2 {
            let q = p(v);
            [q[0] + dir_u[0] * s * W / 2.0, q[1] + dir_u[1] * s * W / 2.0]
        };
        for i in 0..vs.len() - 1 {
            let va = vs[i];
            let vb = vs[i + 1];
            let ha = h(va);
            let hb = h(vb);
            let la = edge(va, -1.0);
            let ra = edge(va, 1.0);
            let lb = edge(vb, -1.0);
            let rb = edge(vb, 1.0);
            let span = va >= v_a - 0.01 && vb <= v_b + 0.01;
            // Top surface (normal up). Winding decided by GeoBuilder normal; fix by testing.
            let q = [
                [la[0], ha, la[1]],
                [ra[0], ha, ra[1]],
                [rb[0], hb, rb[1]],
                [lb[0], hb, lb[1]],
            ];
            push_up(&mut geo, q, deck);
            // Sides.
            let low_a = if span { bottom } else { ground - 0.5 };
            let low_b = if span { bottom } else { ground - 0.5 };
            let mid = p(va);
            push_side(
                &mut geo,
                [la[0], low_a, la[1]],
                [lb[0], low_b, lb[1]],
                [lb[0], hb + 1.0, lb[1]],
                [la[0], ha + 1.0, la[1]],
                side,
                mid,
            );
            push_side(
                &mut geo,
                [ra[0], low_a, ra[1]],
                [rb[0], low_b, rb[1]],
                [rb[0], hb + 1.0, rb[1]],
                [ra[0], ha + 1.0, ra[1]],
                side,
                mid,
            );
            if span {
                let u = [
                    [la[0], bottom, la[1]],
                    [ra[0], bottom, ra[1]],
                    [rb[0], bottom, rb[1]],
                    [lb[0], bottom, lb[1]],
                ];
                push_down(&mut geo, u, under);
            }
        }
        // Abutment faces at the ends of the span (facing the freeway).
        for (v, dir) in [(v_a, 1.0), (v_b, -1.0)] {
            let l = edge(v, -1.0);
            let r = edge(v, 1.0);
            let quad = [
                [l[0], ground - 0.5, l[1]],
                [r[0], ground - 0.5, r[1]],
                [r[0], bottom, r[1]],
                [l[0], bottom, l[1]],
            ];
            let cc = p(v + dir * 5.0);
            push_facing(
                &mut geo,
                quad,
                side,
                [cc[0], (ground + bottom) / 2.0, cc[1]],
            );
        }
        let mat = self.mat(c, "overpass", |c| {
            let map = c
                .graph
                .cached_texture(&c.textures.concrete_texture(), Layer::Main, "");
            c.graph.add_material(
                Material::standard()
                    .set("vertexColors", true)
                    .set("map", map)
                    .set("roughness", 0.9),
            )
        });
        self.add(geo.build(), mat, true, true);

        // Median columns under the span.
        let vm = v_for_lat(MED_C);
        for s in [-3.5, 3.5] {
            let q = p(vm);
            // Along the median (loop) so skewed crossings keep them out of the lanes.
            let (ax, az) = if self.is_loop {
                (f.fx, f.fz)
            } else {
                (dir_u[0], dir_u[1])
            };
            let x = q[0] + ax * s;
            let z = q[1] + az * s;
            let cr = if self.is_loop { 0.88 } else { 1.0 };
            self.overpass_cols.push(trs(
                x,
                road - 1.5,
                z,
                0.0,
                cr,
                bottom - road + 1.5,
                cr,
                0.0,
                0.0,
            ));
        }
        // Street lamps on the bridge (collected with the city street lamps).
        o.lamps = Vec::new();
        let mut v = v_a - ramp_len * 0.6;
        while v <= v_b + ramp_len * 0.6 {
            let e = edge(v, -1.0);
            o.lamps.push(BridgeLamp {
                x: e[0] + dir_u[0] * 0.5,
                y: h(v) + 1.0,
                z: e[1] + dir_u[1] * 0.5,
                dx: dir_u[0],
                dz: dir_u[1],
            });
            v += 28.0;
        }
        o.span = Some((v_a - ramp_len, v_b + ramp_len, u_line));
    }

    // ── Overhead sign gantries ────────────────────────────────────
    fn build_gantries(&mut self, c: &mut FwCtx) {
        let track = c.track;
        let steel = self.mat(c, "gantrySteel", |c| {
            c.graph.add_material(
                Material::standard()
                    .set("color", 0x9aa0a6)
                    .set("metalness", 0.6)
                    .set("roughness", 0.45),
            )
        });
        let mut geo = GeoBuilder::new(false, false);
        let mut sign_quads = Vec::new();
        for gs in &self.gantry_sites {
            let f = track.frame(gs.s);
            let yaw = kernel::atan2(f.fz, f.fx);
            let across_yaw = kernel::atan2(f.rz, f.rx);
            let p = |lat: f64, along: f64| -> P2 {
                [
                    f.x + f.rx * lat + f.fx * along,
                    f.z + f.rz * lat + f.fz * along,
                ]
            };
            // Highest point of the banked carriageway under the gantry.
            let road = f.y + f.bank.abs() * 10.5;
            // Posts.
            for lat in [11.15, MED_C] {
                let q = p(lat, 0.0);
                let base = our_y(&f, js::max(-10.5, js::min(10.5, lat))) - 0.2;
                geo.box_(
                    q[0],
                    base,
                    q[1],
                    0.5,
                    9.9 - (base - road),
                    0.5,
                    yaw,
                    &PrismOpts {
                        roof: Some(true),
                        ..PrismOpts::default()
                    },
                );
            }
            // Truss chords and verticals.
            let span = 11.15 - MED_C + 0.6;
            for (dy, along) in [(7.6, -0.5), (7.6, 0.5), (9.5, -0.5), (9.5, 0.5)] {
                let q = p((11.15 + MED_C) / 2.0, along);
                geo.box_(
                    q[0],
                    road + dy,
                    q[1],
                    span,
                    0.16,
                    0.16,
                    across_yaw,
                    &PrismOpts::default(),
                );
            }
            let mut lat = MED_C + 1.0;
            while lat < 11.0 {
                for along in [-0.5, 0.5] {
                    let q = p(lat, along);
                    geo.box_(
                        q[0],
                        road + 7.6,
                        q[1],
                        0.1,
                        1.9,
                        0.1,
                        across_yaw,
                        &PrismOpts::default(),
                    );
                }
                lat += 1.6;
            }
            // Signs hanging on the approach side.
            for sg in &gs.signs {
                let lines: Vec<&str> = sg.lines.iter().map(String::as_str).collect();
                let font = format!(
                    "bold {}px \"Arial Narrow\", Arial, sans-serif",
                    if sg.lines.len() > 2 { 54 } else { 64 }
                );
                let cached = c.textures.sign_texture(
                    &lines,
                    &SignOpts {
                        bg: sg.bg.unwrap_or("#0b6b3a"),
                        fg: sg.fg.unwrap_or("#fff"),
                        border: Some(sg.border.unwrap_or("#fff")),
                        w: 512,
                        h: 256,
                        arrow: sg.arrow,
                        font: &font,
                    },
                );
                let Cached::Sign { aspect, .. } = &*cached else {
                    unreachable!("signTexture returns a sign")
                };
                let h = 3.0;
                let w = h * aspect;
                // Faces oncoming traffic (normal -F); its +u runs along the road's right.
                let q = p(sg.lane, -0.75);
                let cy = road + 6.8 + h / 2.0;
                sign_quads.push(AtlasQuad {
                    image: cached.clone(),
                    c: [q[0], cy, q[1]],
                    rx: f.rx,
                    rz: f.rz,
                    w,
                    h,
                });
                // Backing plate.
                let b = p(sg.lane, -0.65);
                geo.box_(
                    b[0],
                    road + 6.75,
                    b[1],
                    w + 0.1,
                    h + 0.1,
                    0.12,
                    across_yaw,
                    &PrismOpts::default(),
                );
                // Sign lights: small lamp arms under the sign.
                let mut k = -1.0;
                while k <= 1.0 {
                    let q = p(sg.lane + k * w * 0.25, -1.4);
                    self.sign_lamps.push(ParkLamp {
                        x: q[0],
                        y: road + 6.68,
                        z: q[1],
                    });
                    k += 2.0;
                }
            }
        }
        self.add(geo.build(), steel, true, true);
        // All sign faces share one atlas texture, so they cost one draw call.
        if let Some((g, m)) = atlas_quads(c.graph, &sign_quads, 512.0, 256.0, 4) {
            let mat = c.graph.add_material(m.set("emissiveIntensity", 0.2));
            c.add_night(mat, "emissiveIntensity", 0.05, 0.42);
            let g = c.graph.add_geometry(g);
            let n = static_mesh(c.graph, g, mat, &StaticOpts::default());
            c.graph.add(c.group, n);
        }
    }

    // ── Finish gantry ─────────────────────────────────────────────
    // The finish gantry (Level 1) or, on the endless loop, a welcome arch.
    fn build_finish(&mut self, c: &mut FwCtx, s_at: f64, text: &str, checker: bool) {
        let track = c.track;
        let f = track.frame(s_at);
        let across_yaw = kernel::atan2(f.rz, f.rx);
        let yaw = kernel::atan2(f.fz, f.fx);
        let p = |lat: f64, along: f64| -> P2 {
            [
                f.x + f.rx * lat + f.fx * along,
                f.z + f.rz * lat + f.fz * along,
            ]
        };
        let mut geo = GeoBuilder::new(false, false);
        let road = f.y + f.bank.abs() * 10.5;
        for lat in [11.4, MED_C] {
            let q = p(lat, 0.0);
            geo.box_(
                q[0],
                road - 0.3,
                q[1],
                0.8,
                11.2,
                0.8,
                yaw,
                &PrismOpts::default(),
            );
        }
        let mid = p((11.4 + MED_C) / 2.0, 0.0);
        let span = 11.4 - MED_C + 0.8;
        geo.box_(
            mid[0],
            road + 9.8,
            mid[1],
            span,
            0.5,
            1.2,
            across_yaw,
            &PrismOpts::default(),
        );
        geo.box_(
            mid[0],
            road + 6.9,
            mid[1],
            span,
            0.35,
            1.0,
            across_yaw,
            &PrismOpts::default(),
        );
        let mat = c.graph.add_material(
            Material::standard()
                .set("color", 0x202226)
                .set("metalness", 0.6)
                .set("roughness", 0.4),
        );
        self.add(geo.build(), mat, true, true);
        // Banner, front and back.
        let tex = if checker {
            banner_texture(
                text,
                &BannerOpts {
                    w: 1024,
                    h: 256,
                    checker: true,
                    font: "italic 900 150px \"Arial Narrow\", Arial, sans-serif",
                    ..BannerOpts::default()
                },
            )
        } else {
            banner_texture(
                text,
                &BannerOpts {
                    w: 1024,
                    h: 256,
                    bg: "#12082c",
                    fg: "#7cf6ff",
                    font: "italic 900 132px \"Arial Narrow\", Arial, sans-serif",
                    ..BannerOpts::default()
                },
            )
        };
        let tex = own_texture(c.graph, tex);
        let bm = c.graph.add_material(
            Material::standard()
                .set("map", tex)
                .set("emissive", 0xffffff)
                .set("emissiveMap", tex)
                .set("emissiveIntensity", 0.35)
                .set("roughness", 0.6)
                .set("side", three::DOUBLE_SIDE as f64),
        );
        c.add_night(bm, "emissiveIntensity", 0.15, 0.6);
        let w = span - 0.9;
        let pg = c
            .graph
            .add_geometry(crate::three_geom::plane_geometry(w, 2.6, 1.0, 1.0));
        let banner = c.graph.mesh(pg, bm);
        {
            let o = c.graph.get_mut(banner);
            o.position = crate::three_geom::Vector3::new(
                mid[0] - f.fx * 0.62,
                road + 8.4,
                mid[1] - f.fz * 0.62,
            );
            o.set_rotation(&crate::three_geom::Euler::new(
                0.0,
                kernel::atan2(-f.fx, -f.fz),
                0.0,
            ));
            o.update_matrix();
            o.matrix_auto_update = false;
        }
        c.graph.add(c.group, banner);
        // Chase lights along the top and bottom beams.
        let mut bulbs = Vec::new();
        let mut lat = MED_C + 0.4;
        while lat <= 11.0 {
            for dy in [7.0, 9.8] {
                let q = p(lat, -0.65);
                bulbs.push(trs(q[0], road + dy, q[1], 0.0, 0.18, 0.18, 0.18, 0.0, 0.0));
            }
            lat += 0.7;
        }
        let bulb_mat = c
            .graph
            .add_material(Material::basic().set("color", Color::new(5.0, 4.0, 2.4)));
        let sg = c
            .graph
            .add_geometry(sphere_geometry(1.0, 8.0, 6.0, 0.0, 2.0 * PI, 0.0, PI));
        let bulb_mesh = instanced(c.graph, sg, bulb_mat, &bulbs, false, false);
        // Per-instance colour for a chase pattern.
        {
            let inst = c
                .graph
                .get_mut(bulb_mesh)
                .instances
                .as_mut()
                .expect("instanced");
            for i in 0..bulbs.len() {
                inst.set_color_at(i, Color::new(1.0, 1.0, 1.0));
            }
        }
        c.graph.add(c.group, bulb_mesh);
        let n = bulbs.len();
        let mut time = 0.0;
        c.animators.push(Box::new(
            move |u: &crate::world::UpdateCtx, out: &mut Vec<Edit>| {
                time += u.dt;
                let ph = (time * 8.0).floor();
                for i in 0..n {
                    let on = (((i >> 1) as f64 + ph) % 3.0) == 0.0;
                    let v = if on { 1.0 } else { 0.12 };
                    out.push(Edit {
                        target: Handle::Node(bulb_mesh),
                        change: Change::InstanceColor {
                            index: i as u32,
                            rgb: [v as f32; 3],
                        },
                    });
                }
            },
        ));
    }

    // ── Billboards ───────────────────────────────────────────────
    fn build_billboards(&mut self, c: &mut FwCtx) {
        let track = c.track;
        let path = c.path;
        let z2 = track.zones[c.zone].s0;
        let fin = track.finish_s;
        let sites = self.billboard_sites.clone().unwrap_or_else(|| {
            let b = |s: f64, lat: f64, h: f64| BillboardSite { s, lat, h };
            vec![
                b(z2 + 661.0, 19.0, 11.0),
                b(z2 + 1081.0, -42.0, 14.0),
                b(z2 + 1281.0, 21.0, 16.0),
                b(z2 + 1781.0, -43.0, 13.0),
                b(fin - 680.0, 20.0, 12.0),
                b(fin - 400.0, -42.0, 12.0),
                b(fin - 80.0, 20.0, 13.0),
                b(fin + 300.0, -42.0, 12.0),
            ]
        });
        let mut frame = GeoBuilder::new(false, false);
        let mut boards = Vec::new();
        for (i, st) in sites.iter().enumerate() {
            if st.s > path.u1 - 50.0 {
                continue;
            }
            let f = path.frame(track, st.s);
            let tex = ad_texture(c.textures, i as i64);
            const W: f64 = 17.0;
            let h = W * 384.0 / 1024.0;
            let x = f.x + f.rx * st.lat;
            let z = f.z + f.rz * st.lat;
            let gy = c.terrain.height_at(x, z);
            let base_y = js::max(gy, f.y) + st.h;
            // Face oncoming traffic, toed in 18° toward the road.
            let toe = if st.lat > 0.0 { 0.32 } else { -0.32 };
            let fx = -f.fx;
            let fz = -f.fz;
            let cc = kernel::cos(toe);
            let s = kernel::sin(toe);
            let nx = fx * cc - fz * s;
            let nz = fx * s + fz * cc;
            // Frame and poles (plane x axis is perpendicular to the normal).
            let ax = [nz, -nx];
            boards.push(AtlasQuad {
                image: tex,
                c: [x, base_y + h / 2.0, z],
                rx: ax[0],
                rz: ax[1],
                w: W,
                h,
            });
            let yaw_across = kernel::atan2(ax[1], ax[0]);
            frame.box_(
                x - nx * 0.25,
                base_y - 0.3,
                z - nz * 0.25,
                W + 0.6,
                h + 0.6,
                0.4,
                yaw_across,
                &PrismOpts::default(),
            );
            for k in [-0.28, 0.28] {
                let px = x - nx * 0.8 + ax[0] * W * k;
                let pz = z - nz * 0.8 + ax[1] * W * k;
                frame.box_(
                    px,
                    gy - 0.5,
                    pz,
                    0.7,
                    base_y - gy + 0.6,
                    0.7,
                    yaw_across,
                    &PrismOpts::default(),
                );
            }
            // Catwalk.
            frame.box_(
                x + nx * 0.5,
                base_y - 0.5,
                z + nz * 0.5,
                W,
                0.15,
                1.2,
                yaw_across,
                &PrismOpts::default(),
            );
        }
        let mat = self.mat(c, "billboardFrame", |c| {
            c.graph.add_material(
                Material::standard()
                    .set("color", 0x3a3c40)
                    .set("metalness", 0.5)
                    .set("roughness", 0.6),
            )
        });
        self.add(frame.build(), mat, true, true);
        if let Some((g, m)) = atlas_quads(c.graph, &boards, 1024.0, 384.0, 2) {
            let mat = c
                .graph
                .add_material(m.set("roughness", 0.6).set("emissiveIntensity", 0.9));
            c.add_night(mat, "emissiveIntensity", 0.25, 1.35);
            let g = c.graph.add_geometry(g);
            let n = static_mesh(c.graph, g, mat, &StaticOpts::default());
            c.graph.add(c.group, n);
        }
    }

    // ── Freeway lighting: median poles with twin arms, glow pools ─
    fn build_lighting(&mut self, c: &mut FwCtx) {
        let path = c.path;
        let track = c.track;
        struct Pole {
            x: f64,
            z: f64,
            base: f64,
            top: f64,
            rx: f64,
            rz: f64,
        }
        let mut poles = Vec::new();
        let mut heads: Vec<LampHead> = Vec::new();
        let mut pools = self.tunnel_pools.clone();
        let mut u = path.u0 + 20.0;
        while u < path.u1 - 10.0 {
            if self.reserved_at(track, u, 0.0) || self.in_tunnel(track, u) {
                u += 48.0;
                continue;
            }
            let f = path.frame(track, u);
            let x = f.x + f.rx * MED_C;
            let z = f.z + f.rz * MED_C;
            let back = f.ext && u < path.s_a;
            let base = if back {
                opp_y(&f) - 0.2
            } else {
                js::min(our_y(&f, -10.5), opp_y(&f)) - 0.2
            };
            let top = base + 12.2;
            let yaw = yaw_of(f.fx, f.fz);
            poles.push(Pole {
                x,
                z,
                base,
                top,
                rx: f.rx,
                rz: f.rz,
            });
            for (lat, opp, side) in [(-4.6, false, 1.0), (OPP_C + 5.4, true, -1.0)] {
                if side == 1.0 && back {
                    continue; // no eastbound lanes back there
                }
                let cl = (c.lamp_tint)(u);
                heads.push(LampHead {
                    x: f.x + f.rx * (lat + side * 0.8),
                    z: f.z + f.rz * (lat + side * 0.8),
                    y: top - 0.25,
                    yaw,
                    c: cl,
                });
                pools.push(Pool {
                    s: u,
                    lat,
                    rx: 12.0,
                    rz: 12.0,
                    opp,
                    c: cl,
                    k: 0.17,
                });
            }
            u += 48.0;
        }
        // Retro-reflectors on the barrier faces (amber on the median side, white
        // on the outside), collected for the city's glow sprites.
        self.reflectors = Vec::new();
        let mut u = path.u0 + 8.0;
        while u < path.u1 - 8.0 {
            if self.in_tunnel(track, u) {
                u += 16.0;
                continue;
            }
            let f = path.frame(track, u);
            let mut p = |lat: f64, y: f64, amber: bool| {
                self.reflectors
                    .push(([f.x + f.rx * lat, y, f.z + f.rz * lat], amber));
            };
            if !(f.ext && u < path.s_a) {
                p(9.84, our_y(&f, 9.84) + 0.78, false);
                p(-9.84, our_y(&f, -9.84) + 0.78, true);
            }
            p(OPP_FACE_IN - 0.06, opp_y(&f) + 0.78, true);
            p(OPP_FACE_OUT + 0.06, opp_y(&f) + 0.78, false);
            u += 16.0;
        }
        // Pole geometry: mast + twin arms, built per pole into one merged mesh.
        let mut geo = GeoBuilder::new(false, false);
        for p in &poles {
            geo.box_(
                p.x,
                p.base,
                p.z,
                0.36,
                p.top - p.base,
                0.36,
                kernel::atan2(p.rz, p.rx),
                &PrismOpts::default(),
            );
            // Arms reach 6.3 m each way across the road.
            let arm_yaw = kernel::atan2(p.rz, p.rx);
            geo.box_(
                p.x + p.rx * 0.0,
                p.top - 0.35,
                p.z + p.rz * 0.0,
                13.4,
                0.18,
                0.18,
                arm_yaw,
                &PrismOpts::default(),
            );
        }
        for h in &heads {
            geo.box_(
                h.x,
                h.y - 0.1,
                h.z,
                1.3,
                0.22,
                0.45,
                h.yaw + PI / 2.0,
                &PrismOpts::default(),
            );
        }
        let pole_mat = self.mat(c, "pole", |c| {
            c.graph.add_material(
                Material::standard()
                    .set("color", 0x80868c)
                    .set("metalness", 0.6)
                    .set("roughness", 0.5),
            )
        });
        self.add(geo.build(), pole_mat, false, true);
        // Lamp lenses.
        // Lens colour per head (sodium, or white LED where the city asks for it).
        let lens = c
            .graph
            .add_material(Material::basic().set("color", Color::new(4.5, 4.5, 4.5)));
        let mut lens_m: Vec<Matrix4> = heads
            .iter()
            .map(|h| trs(h.x, h.y - 0.14, h.z, 0.0, 1.1, 0.06, 0.4, 0.0, 0.0))
            .collect();
        let mut lens_c: Vec<Color> = heads
            .iter()
            .map(|h| Color::new(h.c[0], h.c[1], h.c[2]))
            .collect();
        for l in &self.sign_lamps {
            lens_m.push(trs(l.x, l.y, l.z, 0.0, 0.5, 0.12, 0.3, 0.0, 0.0));
            lens_c.push(Color::new(1.0, 0.71, 0.4));
        }
        let bg = c
            .graph
            .add_geometry(box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0));
        let lens_mesh = instanced(c.graph, bg, lens, &lens_m, false, false);
        {
            let inst = c
                .graph
                .get_mut(lens_mesh)
                .instances
                .as_mut()
                .expect("instanced");
            for (i, col) in lens_c.iter().enumerate() {
                inst.set_color_at(i, *col);
            }
        }
        c.graph.add(c.group, lens_mesh);
        self.lamp_heads = heads;
        c.animators.push(Box::new(
            move |u: &crate::world::UpdateCtx, out: &mut Vec<Edit>| {
                let k = 0.35 + 0.65 * smoothstep(0.2, 0.7, u.night);
                let v = 4.5 * k;
                out.push(Edit {
                    target: Handle::Material(lens),
                    change: Change::Color {
                        prop: "color",
                        rgb: [v, v, v],
                    },
                });
            },
        ));

        self.build_pools(c, &pools);
    }

    // Additive light pools draped on the road surface.
    fn build_pools(&mut self, c: &mut FwCtx, pools: &[Pool]) {
        let path = c.path;
        let mut pos: Vec<f64> = Vec::new();
        let mut uv: Vec<f64> = Vec::new();
        let mut col: Vec<f64> = Vec::new();
        for p in pools {
            let y_of = |fr: &Frame, lat: f64| if p.opp { opp_y(fr) } else { our_y(fr, lat) };
            const N: usize = 2;
            let mut grid: Vec<Vec<[f64; 5]>> = Vec::new();
            for a in 0..=N {
                let mut row = Vec::new();
                let u = p.s + (a as f64 / N as f64 - 0.5) * 2.0 * p.rz;
                let f = path.frame(c.track, u);
                for b in 0..=N {
                    let lat = p.lat + (b as f64 / N as f64 - 0.5) * 2.0 * p.rx;
                    row.push([
                        f.x + f.rx * lat,
                        y_of(&f, lat) + 0.05,
                        f.z + f.rz * lat,
                        b as f64 / N as f64,
                        a as f64 / N as f64,
                    ]);
                }
                grid.push(row);
            }
            for a in 0..N {
                for b in 0..N {
                    let (aa, bb, cc, dd) = (
                        grid[a][b],
                        grid[a][b + 1],
                        grid[a + 1][b],
                        grid[a + 1][b + 1],
                    );
                    for v in [aa, bb, cc, bb, dd, cc] {
                        pos.extend_from_slice(&[v[0], v[1], v[2]]);
                        uv.extend_from_slice(&[v[3], v[4]]);
                        col.extend_from_slice(&[p.c[0] * p.k, p.c[1] * p.k, p.c[2] * p.k]);
                    }
                }
            }
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
        g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
        g.compute_bounding_sphere();
        let glow = c
            .graph
            .cached_texture(&c.textures.glow_texture(), Layer::Main, "");
        let m = c.graph.add_material(
            Material::basic()
                .set("map", glow)
                .set("vertexColors", true)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", three::ADDITIVE_BLENDING as f64)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -4.0)
                .set("polygonOffsetUnits", -4.0)
                .set("color", 0xffffff),
        );
        c.animators.push(Box::new(
            move |u: &crate::world::UpdateCtx, out: &mut Vec<Edit>| {
                let v = smoothstep(0.25, 0.8, u.night);
                out.push(Edit {
                    target: Handle::Material(m),
                    change: Change::Color {
                        prop: "color",
                        rgb: [v, v, v],
                    },
                });
            },
        ));
        let g = c.graph.add_geometry(g);
        let mesh = static_mesh(
            c.graph,
            g,
            m,
            &StaticOpts {
                receive: false,
                ..StaticOpts::default()
            },
        );
        c.graph.get_mut(mesh).render_order = 2.0;
        c.graph.add(c.group, mesh);
    }
}

/// A painted line of `addMarkings`.
struct Line {
    lat: f64,
    w: f64,
    c: [f64; 3],
    dash: Option<[f64; 2]>,
}

/// A canvas texture made for one material (not from a cache).
pub fn own_texture(graph: &mut SceneGraph, t: Texture) -> crate::object::TextureId {
    let desc = t.desc("", 0);
    graph.add_texture(Image::Own(Arc::new(t)), desc)
}

// Helpers that make a quad face a known direction regardless of how its
// corners were listed.
fn push_up(geo: &mut GeoBuilder, mut q: [P3; 4], c: P3) {
    let n = normal_of(&q);
    if n[1] < 0.0 {
        q = [q[0], q[3], q[2], q[1]];
    }
    geo.quad(
        q[0],
        q[1],
        q[2],
        q[3],
        Some([[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]),
        Some(c),
        0.0,
    );
}

fn push_down(geo: &mut GeoBuilder, mut q: [P3; 4], c: P3) {
    let n = normal_of(&q);
    if n[1] > 0.0 {
        q = [q[0], q[3], q[2], q[1]];
    }
    geo.quad(
        q[0],
        q[1],
        q[2],
        q[3],
        Some([[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]]),
        Some(c),
        0.0,
    );
}

/// Side wall facing away from centre point `mid` ([x, z]).
fn push_side(geo: &mut GeoBuilder, a: P3, b: P3, c: P3, d: P3, col: P3, mid: P2) {
    let q = [a, b, c, d];
    let n = normal_of(&q);
    let cx = (a[0] + b[0]) / 2.0 - mid[0];
    let cz = (a[2] + b[2]) / 2.0 - mid[1];
    if n[0] * cx + n[2] * cz < 0.0 {
        geo.quad(
            a,
            d,
            c,
            b,
            Some([[0.0, 0.0], [0.0, 1.0], [2.0, 1.0], [2.0, 0.0]]),
            Some(col),
            0.0,
        );
        return;
    }
    geo.quad(
        a,
        b,
        c,
        d,
        Some([[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]]),
        Some(col),
        0.0,
    );
}

fn push_facing(geo: &mut GeoBuilder, mut q: [P3; 4], col: P3, toward: P3) {
    let n = normal_of(&q);
    let cx = (q[0][0] + q[2][0]) / 2.0;
    let cz = (q[0][2] + q[2][2]) / 2.0;
    let dx = toward[0] - cx;
    let dz = toward[2] - cz;
    if n[0] * dx + n[2] * dz < 0.0 {
        q = [q[0], q[3], q[2], q[1]];
    }
    geo.quad(
        q[0],
        q[1],
        q[2],
        q[3],
        Some([[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]]),
        Some(col),
        0.0,
    );
}

fn normal_of(q: &[P3; 4]) -> P3 {
    let ab = [q[1][0] - q[0][0], q[1][1] - q[0][1], q[1][2] - q[0][2]];
    let ac = [q[2][0] - q[0][0], q[2][1] - q[0][1], q[2][2] - q[0][2]];
    [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ]
}

/// Concatenate geometries (indexed or not) into one non-indexed geometry with
/// the attributes they all share.
pub fn merge_flat(geos: &[BufferGeometry]) -> BufferGeometry {
    let flat: Vec<std::borrow::Cow<BufferGeometry>> = geos
        .iter()
        .map(|g| {
            if g.index.is_some() {
                std::borrow::Cow::Owned(g.to_non_indexed())
            } else {
                std::borrow::Cow::Borrowed(g)
            }
        })
        .collect();
    let names: Vec<&str> = ["position", "normal", "uv", "color"]
        .into_iter()
        .filter(|n| flat.iter().all(|g| g.get_attribute(n).is_some()))
        .collect();
    let mut out = BufferGeometry::new();
    for n in names {
        let size = flat[0].get_attribute(n).expect("shared").item_size;
        let mut arr: Vec<f32> = Vec::new();
        for g in &flat {
            let a = g.get_attribute(n).expect("shared");
            arr.extend_from_slice(a.as_f32().expect("a Float32 attribute"));
        }
        out.set_attribute(n, BufferAttribute::from_f32(arr, size));
    }
    out.compute_bounding_sphere();
    out
}

/// Pack distinct canvases into one atlas and make a single mesh of textured
/// quads. quads: [{image, c:[x,y,z], rx, rz, w, h}] — each quad faces the
/// side whose right-hand vector is (rx, rz). Returns the geometry and the
/// material before the caller's `setup` (`map`, `emissive` white,
/// `emissiveMap`, roughness 0.5); `None` without quads.
pub fn atlas_quads(
    graph: &mut SceneGraph,
    quads: &[AtlasQuad],
    cw: f64,
    chh: f64,
    cols: usize,
) -> Option<(BufferGeometry, Material)> {
    if quads.is_empty() {
        return None;
    }
    let mut images: Vec<Arc<Cached>> = Vec::new();
    for q in quads {
        if !images.iter().any(|i| Arc::ptr_eq(i, &q.image)) {
            images.push(q.image.clone());
        }
    }
    let rows = images.len().div_ceil(cols);
    let width = cw * cols.min(images.len()) as f64;
    let height = chh * rows as f64;
    let mut cv = mp_canvas::Canvas::new(width as u32, height as u32);
    for (k, im) in images.iter().enumerate() {
        cv.draw_image(
            &canvas_of(im.texture()),
            (k % cols) as f64 * cw,
            (k / cols) as f64 * chh,
            cw,
            chh,
        );
    }
    let tex = own_texture(graph, Texture::from_canvas(&cv, false, true, 8.0));
    let mut geo = GeoBuilder::new(false, false);
    for q in quads {
        let k = images
            .iter()
            .position(|i| Arc::ptr_eq(i, &q.image))
            .expect("listed");
        let u0 = ((k % cols) as f64 * cw) / width;
        let u1 = (((k % cols) + 1) as f64 * cw) / width;
        let v1 = 1.0 - ((k / cols) as f64 * chh) / height;
        let v0 = 1.0 - (((k / cols) + 1) as f64 * chh) / height;
        let hx = q.rx * q.w / 2.0;
        let hz = q.rz * q.w / 2.0;
        let hy = q.h / 2.0;
        let [x, y, z] = q.c;
        geo.quad(
            [x - hx, y - hy, z - hz],
            [x + hx, y - hy, z + hz],
            [x + hx, y + hy, z + hz],
            [x - hx, y + hy, z - hz],
            Some([[u0, v0], [u1, v0], [u1, v1], [u0, v1]]),
            None,
            0.0,
        );
    }
    let material = Material::standard()
        .set("map", tex)
        .set("emissive", 0xffffff)
        .set("emissiveMap", tex)
        .set("roughness", 0.5);
    Some((geo.build(), material))
}
