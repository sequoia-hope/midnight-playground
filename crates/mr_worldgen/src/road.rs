//! Port of `src/world/Road.js`: the generic `extrude` sweep and the run
//! helpers (`runs`, `chunks`, `groupRuns`) the road and the scenery use
//! (roadmap WP 3.3), and the [`Road`] class (WP 3.5).
//!
//! Road surface, shoulders, markings and barriers, all extruded along the
//! track centreline from cross-section profiles.
//!
//! The asphalt, shoulder and markings shaders (the GLSL `patchAsphalt`,
//! the shoulder patch and `patchMarkings` inject) are the renderer's: the
//! scene carries their material kinds and uniforms (SPEC 6.2).

// Index loops stay index loops (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

use mr_math::{clamp, js, kernel, smoothstep};
use mr_scene::{MaterialKind, three};
use mr_track::road_types::{ROAD_TYPES, RoadType};
use mr_track::{Frame, Track};
use serde_json::Value;

use crate::colorizer::Colorizer;
use crate::material::{Material, num, texture_value};
use crate::object::{Instances, Layer, MaterialId, NodeId, SceneGraph, TextureId};
use crate::terrain::Terrain;
use crate::textures::TextureCache;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, Matrix4, Quaternion, Vector3, box_geometry,
    cylinder_geometry, plane_geometry,
};
use crate::world::{Change, Edit, Handle, UpdateCtx};

pub const ROAD_STEP: f64 = 2.0;

/// `(f, s) => number`.
pub type LatFn = Box<dyn Fn(&Frame, f64) -> f64 + Send + Sync>;

/// `(f, s, p) => [r, g, b]`.
pub type ColorFn = Box<dyn Fn(&Frame, f64, usize) -> [f64; 3] + Send + Sync>;

/// A profile value: a number, or a function of the frame and s (`(f, s)`).
pub enum Lat {
    Num(f64),
    Fn(LatFn),
}

impl Lat {
    /// `(f, s) => ...`.
    pub fn f(f: impl Fn(&Frame, f64) -> f64 + Send + Sync + 'static) -> Lat {
        Lat::Fn(Box::new(f))
    }
}

impl From<f64> for Lat {
    fn from(v: f64) -> Lat {
        Lat::Num(v)
    }
}

/// One point of a cross-section: `{lat, dy, u, abs, gapAfter}`.
pub struct ProfilePoint {
    pub lat: Lat,
    /// `None` is the JS's missing `dy` (0).
    pub dy: Option<Lat>,
    /// `None`: u is `lat / 4`.
    pub u: Option<f64>,
    /// y is `dy` alone, not relative to the road.
    pub abs: bool,
    /// No faces between this point and the next.
    pub gap_after: bool,
}

impl ProfilePoint {
    /// `{ lat }`.
    pub fn new(lat: impl Into<Lat>) -> ProfilePoint {
        ProfilePoint {
            lat: lat.into(),
            dy: None,
            u: None,
            abs: false,
            gap_after: false,
        }
    }

    /// `{ ..., dy }`.
    pub fn dy(mut self, dy: impl Into<Lat>) -> ProfilePoint {
        self.dy = Some(dy.into());
        self
    }

    /// `{ ..., u }`.
    pub fn u(mut self, u: f64) -> ProfilePoint {
        self.u = Some(u);
        self
    }

    pub fn abs(mut self) -> ProfilePoint {
        self.abs = true;
        self
    }

    pub fn gap_after(mut self) -> ProfilePoint {
        self.gap_after = true;
        self
    }
}

/// `color`: one colour for every vertex, or `(f, s, p) => [r, g, b]`.
pub enum ExtrudeColor {
    Rgb([f64; 3]),
    Fn(ColorFn),
}

/// `extrude`'s options: `{ step = ROAD_STEP, vScale = 8, flat = false,
/// color = null }` (`flat` is unused by the JS body and not ported).
pub struct ExtrudeOpts {
    pub step: f64,
    pub v_scale: f64,
    pub color: Option<ExtrudeColor>,
}

impl Default for ExtrudeOpts {
    fn default() -> Self {
        ExtrudeOpts {
            step: ROAD_STEP,
            v_scale: 8.0,
            color: None,
        }
    }
}

/// Generic extrusion. profile: [{lat(f,i), dy(f,i), u}] across the section.
/// ranges: [[s0, s1], ...]. Produces one BufferGeometry.
pub fn extrude(
    track: &Track,
    ranges: &[[f64; 2]],
    profile: &[ProfilePoint],
    o: &ExtrudeOpts,
) -> BufferGeometry {
    let mut pos: Vec<f64> = Vec::new();
    let mut uv: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut col: Vec<f64> = Vec::new();
    let p_len = profile.len();
    for &[s0, s1] in ranges {
        if s1 - s0 < 0.5 {
            continue;
        }
        let base = pos.len() / 3;
        let mut rows = 0;
        let mut s = s0;
        loop {
            let ss = js::min(s, s1);
            let f = track.frame(ss);
            for (p, pr) in profile.iter().enumerate() {
                let lat = match &pr.lat {
                    Lat::Fn(l) => l(&f, ss),
                    Lat::Num(v) => *v,
                };
                let dy = match &pr.dy {
                    Some(Lat::Fn(d)) => d(&f, ss),
                    // `pr.dy || 0`
                    Some(Lat::Num(v)) => js::or(*v, 0.0),
                    None => 0.0,
                };
                let x = f.x + f.rx * lat;
                let z = f.z + f.rz * lat;
                let y = (if pr.abs { 0.0 } else { f.y - lat * f.bank }) + dy;
                pos.extend_from_slice(&[x, y, z]);
                uv.extend_from_slice(&[pr.u.unwrap_or(lat / 4.0), ss / o.v_scale]);
                match &o.color {
                    Some(ExtrudeColor::Rgb(c)) => col.extend_from_slice(c),
                    Some(ExtrudeColor::Fn(cf)) => col.extend_from_slice(&cf(&f, ss, p)),
                    None => {}
                }
            }
            rows += 1;
            if ss >= s1 {
                break;
            }
            s += o.step;
        }
        for r in 0..rows.max(1) - 1 {
            for p in 0..p_len.saturating_sub(1) {
                if profile[p].gap_after {
                    continue;
                }
                let a = (base + r * p_len + p) as u32;
                let b = a + 1;
                let c = a + p_len as u32;
                let d = c + 1;
                idx.extend_from_slice(&[a, b, c, b, d, c]);
            }
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
    if o.color.is_some() {
        g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
    }
    g.set_index(&idx);
    g.compute_vertex_normals();
    g
}

/// Split [0, len] into chunks for frustum culling (`size` 560 in the JS
/// default).
pub fn chunks(s0: f64, s1: f64, size: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let mut s = s0;
    while s < s1 {
        out.push([s, js::min(s1, s + size)]);
        s += size;
    }
    out
}

/// Split runs into chunks and bundle neighbouring pieces so each mesh covers
/// about `size` metres of road (fewer draw calls, still frustum-cullable).
pub fn group_runs(ranges: &[[f64; 2]], size: f64) -> Vec<Vec<[f64; 2]>> {
    let pieces: Vec<[f64; 2]> = ranges
        .iter()
        .flat_map(|r| chunks(r[0], r[1], size))
        .collect();
    let mut groups = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    let mut start: Option<f64> = None;
    for p in pieces {
        if start.is_none() {
            start = Some(p[0]);
        }
        if p[1] - start.unwrap() > size && !cur.is_empty() {
            groups.push(std::mem::take(&mut cur));
            start = Some(p[0]);
        }
        cur.push(p);
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    groups
}

/// Runs where pred(s) holds, sampled every `step` metres from `s0` to `s1`
/// (the JS defaults: 2, 0 and `track.length`).
pub fn runs(pred: impl Fn(f64) -> bool, step: f64, s0: f64, s1: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let mut start: Option<f64> = None;
    let mut s = s0;
    while s <= s1 {
        let ok = pred(s);
        if ok && start.is_none() {
            start = Some(s);
        }
        if !ok && let Some(st) = start {
            out.push([st, s]);
            start = None;
        }
        s += step;
    }
    if let Some(st) = start {
        out.push([st, s1]);
    }
    out
}

// ── The Road class ──────────────────────────────────────────────────────

/// A stretch where scenery blanks the paint across junctions (an entry of
/// `track.noMarks`, which Streets' `plan()` sets).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkGap {
    pub s0: f64,
    pub s1: f64,
}

/// Lane layout across the section for a marking scheme: lanes start at
/// `origin` (lateral metres) and repeat every `width`. The asphalt shader uses
/// it to lay tyre-worn wheel paths and an oil stripe down each lane.
fn lane_layout(marks: &str, hw: f64) -> [f64; 2] {
    if marks == "freeway" && hw > 9.0 {
        let l = -hw + 1.2;
        let r = hw - 2.0;
        return [l, (r - l) / 4.0];
    }
    if marks == "boulevard" {
        return [0.0, (hw - 0.45) / 2.0];
    }
    if marks == "avenue" || marks == "guide" {
        return [0.0, hw / 2.0];
    }
    [0.0, hw - 0.3]
}

/// What a road reads and writes of the world being built.
pub struct RoadCtx<'a> {
    pub track: &'a Track,
    pub terrain: &'a Terrain,
    pub graph: &'a mut SceneGraph,
    pub textures: &'a mut TextureCache,
    /// `track.noMarks` (empty where no scenery sets it).
    pub no_marks: &'a [MarkGap],
}

/// `new Road(track, terrain)` and what `build()` leaves on it.
pub struct Road {
    /// `this.group`: the group `road`.
    pub group: NodeId,
    /// What sits at each metre's left and right edge: 0 none, 1 rock wall,
    /// 2 guardrail, 3 fence, 4 jersey barrier, 5 kerb + railing, 6 kerb
    /// (`track.sideL` and `sideR` in the JS; scenery reads them).
    pub side_l: Vec<u8>,
    pub side_r: Vec<u8>,
    /// `this.materials`, keys in the order made (`asphalt0`..`asphalt2`,
    /// `shoulder`, `markings`, `steel`, `post`, `wood`, `concrete`,
    /// `railPaint`, `chevron`).
    pub materials: Vec<(String, MaterialId)>,
    /// `this.uniforms.tDetail`: the texture module's
    /// `terrainDetailTexture`, the one texture the terrain, sky and sea use
    /// too.
    pub detail: TextureId,
}

/// One painted line of a section (`{ k, lat, w, c, dash }`).
#[derive(Clone, Copy, Debug)]
struct Line {
    k: &'static str,
    lat: f64,
    w: f64,
    c: [f64; 3],
    dash: Option<[f64; 2]>,
}

/// `{ L, f: { ...f } }`.
struct Section {
    l: Vec<Line>,
    f: Frame,
}

/// `ROAD_TYPES[ROAD_KEYS[t.roadType[i]]]`.
fn road_type_at(t: &Track, i: usize) -> &'static RoadType {
    &ROAD_TYPES[t.road_type[i] as usize]
}

/// A profile point `{ lat: (f) => ..., dy, u }`.
fn pt(lat: impl Fn(&Frame) -> f64 + Send + Sync + 'static, dy: f64, u: f64) -> ProfilePoint {
    ProfilePoint::new(Lat::f(move |f, _| lat(f))).dy(dy).u(u)
}

/// `{ step, vScale }`.
fn opts(step: f64, v_scale: f64) -> ExtrudeOpts {
    ExtrudeOpts {
        step,
        v_scale,
        color: None,
    }
}

/// `e.set(0, yaw, 0); q.setFromEuler(e)`.
fn yaw_quaternion(yaw: f64) -> Quaternion {
    Quaternion::from_euler(&Euler::new(0.0, yaw, 0.0))
}

const DOUBLE_SIDE: f64 = three::DOUBLE_SIDE as f64;

const PI: f64 = std::f64::consts::PI;

/// An instanced mesh's instances.
fn instances(graph: &mut SceneGraph, im: NodeId) -> &mut Instances {
    graph
        .get_mut(im)
        .instances
        .as_mut()
        .expect("an instanced mesh")
}

impl Road {
    /// `new Road(track, terrain)`: the group and the shared uniforms.
    pub fn new(graph: &mut SceneGraph, textures: &mut TextureCache) -> Road {
        let group = graph.group("road");
        let detail = graph.cached_texture(&textures.terrain_detail_texture(), Layer::Main, "");
        Road {
            group,
            side_l: Vec::new(),
            side_r: Vec::new(),
            materials: Vec::new(),
            detail,
        }
    }

    /// `this.materials[key]`.
    pub fn material(&self, key: &str) -> Option<MaterialId> {
        self.materials
            .iter()
            .find(|(k, _)| k == key)
            .map(|&(_, m)| m)
    }

    /// The asphalt materials (they share one `uWet` uniform).
    pub fn asphalt_materials(&self) -> Vec<MaterialId> {
        self.materials
            .iter()
            .filter(|(k, _)| k.starts_with("asphalt"))
            .map(|&(_, m)| m)
            .collect()
    }

    /// `setNight(n)`: night makes the tarmac look damp (see patchAsphalt).
    /// `uWet` is one uniform object shared by the asphalt materials, so the
    /// change is an edit of each.
    pub fn set_night(&self, n: f64, out: &mut Vec<Edit>) {
        for m in self.asphalt_materials() {
            out.push(Edit {
                target: Handle::Material(m),
                change: Change::Number {
                    prop: "uWet",
                    value: n,
                },
            });
        }
    }

    /// The updater `World.build` registers after the road:
    /// `road.setNight(smoothstep(0.55, 1.0, n) * 0.85)` every frame.
    pub fn dew_animator(&self) -> impl FnMut(&UpdateCtx, &mut Vec<Edit>) + Send + Sync + 'static {
        let mats = self.asphalt_materials();
        move |u: &UpdateCtx, out: &mut Vec<Edit>| {
            // Dew on the tarmac after dark: lamps and headlights glint off it.
            let v = smoothstep(0.55, 1.0, u.night) * 0.85;
            for &m in &mats {
                out.push(Edit {
                    target: Handle::Material(m),
                    change: Change::Number {
                        prop: "uWet",
                        value: v,
                    },
                });
            }
        }
    }

    /// `build()`: fills the group (not yet added anywhere) and returns it.
    pub fn build(&mut self, c: &mut RoadCtx) -> NodeId {
        self.classify_sides(c);
        self.build_surface(c);
        self.build_markings(c);
        self.build_barriers(c);
        self.build_lines(c);
        self.build_chevrons(c);
        self.build_viaduct(c);
        self.group
    }

    /// For each metre and side decide what sits at the edge: a rock face, a
    /// guardrail, a fence or a concrete barrier. Physics and scenery read it.
    pub fn classify_sides(&mut self, c: &RoadCtx) {
        let (t, tr) = (c.track, c.terrain);
        let n = t.n;
        let mut l = vec![0u8; n];
        let mut r = vec![0u8; n];
        // 0 none, 1 rock wall, 2 guardrail, 3 fence, 4 jersey barrier, 5 kerb + railing
        let edge = |i: usize| road_type_at(t, i).edge;
        // EDGE_CODE: fence 3, jersey 4, rail 5, curb 6, none 0, circuit 0.
        let edge_code = |e: &str| -> u8 {
            match e {
                "fence" => 3,
                "jersey" => 4,
                "rail" => 5,
                "curb" => 6,
                _ => 0,
            }
        };
        // Float32Arrays.
        let mut tmp_l = vec![0f32; n];
        let mut tmp_r = vec![0f32; n];
        for i in (0..n).step_by(2) {
            if edge(i) == "terrain" {
                for side in [-1.0, 1.0] {
                    let hw = f64::from(t.hw[i]) + f64::from(t.margin[i]);
                    let probe = side * (hw + 5.0);
                    let x = f64::from(t.px[i]) + f64::from(t.rx[i]) * probe;
                    let zz = f64::from(t.pz[i]) + f64::from(t.rz[i]) * probe;
                    let th = tr.height_at(x, zz);
                    let rh = t.surface_y(i as f64, side * hw);
                    let v = th - rh; // positive = ground rises (wall)
                    if side < 0.0 {
                        tmp_l[i] = v as f32;
                    } else {
                        tmp_r[i] = v as f32;
                    }
                }
            }
        }
        // Smooth the decision so barriers come in sensible runs.
        let decide = |tmp: &[f32], out: &mut [u8]| {
            for i in 0..n {
                let e = edge(i);
                if e != "terrain" {
                    out[i] = edge_code(e);
                    continue;
                }
                let mut acc = 0.0;
                let mut cnt = 0.0;
                let mut k: i64 = -16;
                while k <= 16 {
                    let j = clamp(((i & !1) as i64 + k) as f64, 0.0, (n - 1) as f64) as usize;
                    acc += f64::from(tmp[j]);
                    cnt += 1.0;
                    k += 2;
                }
                out[i] = if acc / cnt > 1.6 { 1 } else { 2 };
            }
        };
        decide(&tmp_l, &mut l);
        decide(&tmp_r, &mut r);
        // Drop stubs: short rock-wall gaps inside guardrail runs become guardrail,
        // and short guardrail pieces between rock walls disappear.
        let despeckle = |arr: &mut [u8]| {
            for (val, other, min_len) in [(1u8, 2u8, 30usize), (2, 1, 40)] {
                let mut i = 0;
                while i < n {
                    if edge(i) != "terrain" || arr[i] != val {
                        i += 1;
                        continue;
                    }
                    let mut j = i;
                    while j < n && arr[j] == val && edge(j) == "terrain" {
                        j += 1;
                    }
                    let before = if i > 0 { arr[i - 1] } else { other };
                    let after = if j < n { arr[j] } else { other };
                    if j - i < min_len && before == other && after == other {
                        arr[i..j].fill(other);
                    }
                    i = j;
                }
            }
        };
        despeckle(&mut l);
        despeckle(&mut r);
        // Hairpin outsides always get a guardrail.
        for i in 0..n {
            let k = f64::from(t.k_smooth[i]);
            if edge(i) == "terrain" && k.abs() > 0.022 {
                if k > 0.0 {
                    l[i] = 2;
                } else {
                    r[i] = 2;
                }
            }
        }
        self.side_l = l;
        self.side_r = r;
    }

    /// `mat(key, make)`: one material per key.
    fn mat(
        &mut self,
        c: &mut RoadCtx,
        key: &str,
        make: impl FnOnce(&mut RoadCtx) -> Material,
    ) -> MaterialId {
        if let Some(m) = self.material(key) {
            return m;
        }
        let m = make(c);
        let id = c.graph.add_material(m);
        self.materials.push((key.to_string(), id));
        id
    }

    /// `new THREE.Mesh(g, m)` with its shadow flags, added to the group.
    fn add_mesh(
        &self,
        c: &mut RoadCtx,
        g: BufferGeometry,
        m: MaterialId,
        cast: bool,
        receive: bool,
    ) -> NodeId {
        let gid = c.graph.add_geometry(g);
        let mesh = c.graph.mesh(gid, m);
        let o = c.graph.get_mut(mesh);
        o.cast_shadow = cast;
        o.receive_shadow = receive;
        c.graph.add(self.group, mesh);
        mesh
    }

    pub fn build_surface(&mut self, c: &mut RoadCtx) {
        let t = c.track;
        let type_at = |s: f64| road_type_at(t, t.idx(s));
        let detail = self.detail;
        let mut asphalt_mats = Vec::new();
        for tone in [0u32, 1, 2] {
            let id = self.mat(c, &format!("asphalt{tone}"), |c| {
                let map =
                    c.graph
                        .cached_texture(&c.textures.asphalt_texture(tone), Layer::Main, "");
                // m.map.repeat.set(1, 1) is three's default; then patchAsphalt.
                Material::standard()
                    .set("map", map)
                    .set("roughness", 0.86)
                    .set("metalness", 0.0)
                    .set("color", 0xffffff)
                    .kind(MaterialKind::Asphalt, None)
                    .uniform("tDetail", texture_value(detail))
                    .uniform("uWet", num(0.0))
                    .uniform("clippingPlanes", Value::Null)
            });
            asphalt_mats.push(id);
        }
        let shoulder_mat = self.mat(c, "shoulder", |c| {
            let map = c
                .graph
                .cached_texture(&c.textures.gravel_texture(), Layer::Main, "");
            // Toward the outer edge the gravel texture flattens to its average so
            // the verge meets the terrain (whose colour the outer vertices carry)
            // without a hard seam.
            Material::standard()
                .set("map", map)
                .set("roughness", 1.0)
                .set("vertexColors", true)
                .kind(MaterialKind::Shoulder, None)
                .uniform("clippingPlanes", Value::Null)
        });
        let colorizer = Colorizer::new(c.terrain);

        for z in &t.zones {
            let s0 = js::max(0.0, z.s0 - 1.0);
            let s1 = js::min(t.length, z.s1 + 1.0);
            for r in chunks(s0, s1, 560.0) {
                // Asphalt UVs in metres / 4 both ways, so the aggregate isn't
                // stretched along the road; the shader recovers metres from them.
                let mut surf = extrude(
                    t,
                    &[r],
                    &[
                        ProfilePoint::new(Lat::f(|f, _| -f.hw)),
                        ProfilePoint::new(0.0),
                        ProfilePoint::new(Lat::f(|f, _| f.hw)),
                    ],
                    &opts(ROAD_STEP, 4.0),
                );
                let uv = surf.get_attribute("uv").expect("uv");
                let count = uv.count();
                let mut lane = vec![0f32; count * 3];
                for k in 0..count {
                    let s = uv.get_y(k) * 4.0;
                    let f = t.frame(js::min(s, t.length));
                    let [o, w] = lane_layout(road_type_at(t, t.idx(s)).marks, f.hw);
                    lane[k * 3] = o as f32;
                    lane[k * 3 + 1] = w as f32;
                    lane[k * 3 + 2] = f.hw as f32;
                }
                surf.set_attribute("aLane", BufferAttribute::from_f32(lane, 3));
                let mid = (r[0] + r[1]) / 2.0;
                let mat = asphalt_mats[type_at(mid).tone as usize];
                self.add_mesh(c, surf, mat, false, true);

                // Shoulders + skirt (sloped into the terrain; vertical fascia in the city).
                // Profiles run left→right so faces point up/outward.
                let edge_type = type_at(mid).edge;
                // Kerbed streets: the scenery lays the kerbs and pavements; circuits
                // their kerbs and run-off.
                if edge_type == "curb" || edge_type == "circuit" {
                    continue;
                }
                let city = edge_type == "jersey";
                let tint = match z.zone.landform {
                    "mountain" => [0.55, 0.52, 0.48],
                    "coast" => [0.6, 0.55, 0.47],
                    "valley" => [0.45, 0.42, 0.33],
                    "beach" => [0.66, 0.64, 0.6],
                    "canyon" => [0.62, 0.45, 0.33],
                    "desert" => [0.66, 0.56, 0.42],
                    "playa" => [0.78, 0.76, 0.7],
                    _ => [0.62, 0.62, 0.6],
                };
                for side in [-1.0f64, 1.0] {
                    let w = move |f: &Frame| if side < 0.0 { f.wall_l } else { f.wall_r };
                    let mut prof = if city {
                        vec![
                            pt(move |f| side * f.hw, 0.0, 0.0),
                            pt(move |f| side * (w(f) + 0.35), 0.0, 0.4),
                            pt(move |f| side * (w(f) + 0.35), -1.6, 1.0),
                        ]
                    } else {
                        vec![
                            pt(move |f| side * f.hw, -0.01, 0.0),
                            pt(move |f| side * w(f), -0.08, 0.5),
                            pt(move |f| side * (w(f) + 2.5), -1.6, 1.0),
                        ]
                    };
                    if side < 0.0 {
                        prof.reverse();
                    }
                    let mut g = extrude(
                        t,
                        &[r],
                        &prof,
                        &ExtrudeOpts {
                            step: ROAD_STEP,
                            v_scale: 6.0,
                            color: Some(ExtrudeColor::Rgb(tint)),
                        },
                    );
                    if !city {
                        // Outer edge takes the terrain's own colour (scaled so gravel
                        // map × vertex colour lands on what the terrain shader shows).
                        let gu: Vec<f64> = {
                            let a = g.get_attribute("uv").expect("uv");
                            (0..a.count()).map(|k| a.get_x(k)).collect()
                        };
                        let gp: Vec<(f64, f64)> = {
                            let a = g.get_attribute("position").expect("position");
                            (0..a.count()).map(|k| (a.get_x(k), a.get_z(k))).collect()
                        };
                        let gc = g.get_attribute_mut("color").expect("color");
                        for k in 0..gu.len() {
                            if gu[k] < 0.99 {
                                continue;
                            }
                            let (x, zz) = gp[k];
                            let y = c.terrain.height_at(x, zz);
                            let sl = c.terrain.slope_at(x, zz, 3.0);
                            let (tcol, _) = colorizer.color(x, y, zz, 1.0 / (1.0 + sl * sl).sqrt());
                            let q = 1.25 / 0.29;
                            gc.set_xyz(k, tcol[0] * q, tcol[1] * q, tcol[2] * q);
                        }
                    }
                    self.add_mesh(c, g, shoulder_mat, false, true);
                }
            }
        }
    }

    pub fn build_markings(&mut self, c: &mut RoadCtx) {
        let t = c.track;
        let white = [0.92, 0.92, 0.9];
        let yellow = [0.95, 0.72, 0.12];
        let mut pos: Vec<f64> = Vec::new();
        let mut col: Vec<f64> = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let mut uvs: Vec<f64> = Vec::new();
        let no_marks = c.no_marks;
        let ln = |k, lat, w, c| Line {
            k,
            lat,
            w,
            c,
            dash: None,
        };
        let dashed = |k, lat, w, c, d: [f64; 2]| Line {
            k,
            lat,
            w,
            c,
            dash: Some(d),
        };
        let lines_at = |s: f64| -> Section {
            let f = t.frame(s);
            let hw = f.hw;
            let marks = road_type_at(t, t.idx(s)).marks;
            let mut l = Vec::new();
            // Scenery can blank the paint across junctions (crossings, plazas).
            if no_marks.iter().any(|g| s > g.s0 && s < g.s1) {
                return Section { l, f };
            }
            if marks == "double" {
                l.push(ln("c1", -0.14, 0.12, yellow));
                l.push(ln("c2", 0.14, 0.12, yellow));
                l.push(ln("el", -(hw - 0.3), 0.15, white));
                l.push(ln("er", hw - 0.3, 0.15, white));
            } else if marks == "boulevard" {
                // Boulevard: two lanes each way, double yellow in the middle.
                l.push(ln("c1", -0.14, 0.12, yellow));
                l.push(ln("c2", 0.14, 0.12, yellow));
                let lane = (hw - 0.45) / 2.0;
                l.push(dashed("bl", -lane, 0.13, white, [3.0, 9.0]));
                l.push(dashed("br", lane, 0.13, white, [3.0, 9.0]));
                l.push(ln("el", -(hw - 0.3), 0.15, white));
                l.push(ln("er", hw - 0.3, 0.15, white));
            } else if marks == "avenue" {
                // Kerbed city street: double yellow and lane lines; the kerbs are the edges.
                l.push(ln("c1", -0.14, 0.12, yellow));
                l.push(ln("c2", 0.14, 0.12, yellow));
                let lane = hw / 2.0;
                l.push(dashed("bl", -lane, 0.13, white, [3.0, 9.0]));
                l.push(dashed("br", lane, 0.13, white, [3.0, 9.0]));
            } else if marks == "dashed" {
                l.push(dashed("c", 0.0, 0.13, yellow, [4.0, 11.0]));
                l.push(ln("el", -(hw - 0.3), 0.13, white));
                l.push(ln("er", hw - 0.3, 0.13, white));
            } else if marks == "guide" {
                // Dry lake course: one dark guide line down the middle, no edges.
                l.push(ln("g", 0.0, 0.3, [0.09, 0.08, 0.08]));
            } else if marks == "freeway" && hw > 9.0 {
                let left = -hw + 1.2;
                let right = hw - 2.0;
                let lw = (right - left) / 4.0;
                l.push(ln("fl", left, 0.15, yellow));
                l.push(ln("fr", right, 0.18, white));
                for (q, k) in [(1.0, "f1"), (2.0, "f2"), (3.0, "f3")] {
                    l.push(dashed(k, left + lw * q, 0.14, white, [3.0, 12.0]));
                }
            } else {
                l.push(ln("el", -(hw - 0.3), 0.15, white));
                l.push(ln("er", hw - 0.3, 0.15, white));
            }
            Section { l, f }
        };
        let mut prev = lines_at(0.0);
        let mut s = 1.0;
        while s <= t.length {
            let cur = lines_at(s);
            for a in &prev.l {
                let Some(b) = cur.l.iter().find(|x| x.k == a.k) else {
                    continue;
                };
                if let Some(d) = a.dash {
                    let ph = (s - 0.5) % d[1];
                    if ph > d[0] {
                        continue;
                    }
                }
                let base = (pos.len() / 3) as u32;
                for (fr, ln, ss) in [(&prev.f, a, s - 1.0), (&cur.f, b, s)] {
                    for side in [-1.0, 1.0] {
                        let lat = ln.lat + side * ln.w * 0.5;
                        pos.extend_from_slice(&[
                            fr.x + fr.rx * lat,
                            fr.y - lat * fr.bank + 0.018,
                            fr.z + fr.rz * lat,
                        ]);
                        col.extend_from_slice(&ln.c);
                        uvs.extend_from_slice(&[lat * 8.0, ss]);
                    }
                }
                idx.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
            }
            prev = cur;
            s += 1.0;
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
        g.set_attribute("uv", BufferAttribute::from_f64(&uvs, 2));
        g.set_index(&idx);
        g.compute_vertex_normals();
        // patchMarkings: paint wear. Markings are thin quads carrying (side,
        // s) in uv; the paint fades toward asphalt in blotches and where
        // tyres cross.
        let m = Material::standard()
            .set("vertexColors", true)
            .set("roughness", 0.55)
            .set("metalness", 0.0)
            .set("emissive", 0xffffff)
            .set("emissiveIntensity", 0.0)
            .set("polygonOffset", true)
            .set("polygonOffsetFactor", -2.0)
            .set("polygonOffsetUnits", -2.0)
            .kind(MaterialKind::Markings, None)
            .uniform("tDetail", texture_value(self.detail))
            .uniform("clippingPlanes", Value::Null);
        let id = c.graph.add_material(m);
        self.materials.push(("markings".into(), id));
        self.add_mesh(c, g, id, false, true);
    }

    pub fn build_lines(&mut self, c: &mut RoadCtx) {
        // Start and finish lines — checkered bands across the road.
        let t = c.track;
        let tex = c
            .graph
            .cached_texture(&c.textures.checker_texture(10), Layer::Main, "");
        let mat = c.graph.add_material(
            Material::standard()
                .set("map", tex)
                .set("roughness", 0.6)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -3.0)
                .set("polygonOffsetUnits", -3.0),
        );
        // A circuit starts and finishes on the same line.
        let list: Vec<f64> = if t.is_loop {
            if t.laps != 0 {
                vec![t.start_s]
            } else {
                Vec::new()
            }
        } else {
            vec![t.start_s, t.finish_s]
        };
        for s in list {
            let mut g = extrude(
                t,
                &[[s - 1.2, s + 1.2]],
                &[pt(|f| -f.hw, 0.022, 0.0), pt(|f| f.hw, 0.022, 1.0)],
                &opts(2.4, 2.4),
            );
            // u runs across the road (checker columns), v along it (4 rows).
            let uv = g.get_attribute_mut("uv").expect("uv");
            let v0 = uv.get_y(0);
            for k in 0..uv.count() {
                let v = uv.get_y(k) - v0;
                uv.set_y(k, v);
            }
            self.add_mesh(c, g, mat, false, false);
        }
    }

    pub fn build_barriers(&mut self, c: &mut RoadCtx) {
        let t = c.track;
        let steel = self.mat(c, "steel", |_| {
            Material::standard()
                .set("color", 0xa9adb3)
                .set("metalness", 0.45)
                .set("roughness", 0.55)
                .set("side", DOUBLE_SIDE)
        });
        let post = self.mat(c, "post", |_| {
            Material::standard()
                .set("color", 0x6d6f73)
                .set("metalness", 0.5)
                .set("roughness", 0.5)
        });
        let wood = self.mat(c, "wood", |_| {
            Material::standard()
                .set("color", 0x7a6248)
                .set("roughness", 0.95)
        });
        let concrete = self.mat(c, "concrete", |c| {
            let map = c
                .graph
                .cached_texture(&c.textures.concrete_texture(), Layer::Main, "");
            Material::standard()
                .set("map", map)
                .set("roughness", 0.9)
                .set("color", 0xd8d5ce)
        });

        for side in [-1.0f64, 1.0] {
            let arr = if side < 0.0 {
                self.side_l.clone()
            } else {
                self.side_r.clone()
            };
            let w = move |f: &Frame| if side < 0.0 { f.wall_l } else { f.wall_r };
            // Guardrail W-beam: a vertical band.
            let gr = runs(|s| arr[t.idx(s)] == 2, 2.0, 0.0, t.length);
            for r in group_runs(&gr, 560.0) {
                // f.hw + (wall - f.hw), as the JS writes it.
                let out = move |f: &Frame| f.hw + (w(f) - f.hw);
                let prof = [
                    pt(move |f| side * (out(f) + 0.15), 0.48, 0.0),
                    pt(move |f| side * (out(f) + 0.1), 0.64, 0.5),
                    pt(move |f| side * (out(f) + 0.15), 0.8, 1.0),
                ];
                let g = extrude(t, &r, &prof, &opts(2.0, 4.0));
                self.add_mesh(c, g, steel, true, false);
            }
            self.post_instances(c, &gr, side, 4.0, post, [0.12, 0.85, 0.12], 0.25);

            // Wooden fence: two rails + posts.
            let gaps: Vec<_> = t
                .fence_gaps
                .iter()
                .filter(|g| g.side == side)
                .copied()
                .collect();
            let fr = runs(
                |s| arr[t.idx(s)] == 3 && !gaps.iter().any(|g| s > g.s0 && s < g.s1),
                1.0,
                0.0,
                t.length,
            );
            for r in group_runs(&fr, 560.0) {
                for h in [0.55, 1.0] {
                    let mut prof = vec![
                        pt(move |f| side * (w(f) + 0.25), h - 0.07, 0.0),
                        pt(move |f| side * (w(f) + 0.25), h + 0.07, 1.0),
                    ];
                    if side > 0.0 {
                        prof.reverse();
                    }
                    let g = extrude(t, &r, &prof, &opts(3.0, 3.0));
                    // m.material.side = THREE.DoubleSide: the shared wood
                    // material, its posts included.
                    c.graph
                        .material_mut(wood)
                        .set_value("side", DOUBLE_SIDE.into());
                    self.add_mesh(c, g, wood, true, false);
                }
            }
            self.post_instances(c, &fr, side, 3.0, wood, [0.14, 1.2, 0.14], 0.3);

            // Jersey barrier: extruded profile.
            let jr = runs(|s| arr[t.idx(s)] == 4, 2.0, 0.0, t.length);
            for r in group_runs(&jr, 560.0) {
                let mut prof = vec![
                    pt(move |f| side * (w(f) - 0.02), 0.0, 0.0),
                    pt(move |f| side * (w(f) + 0.02), 0.25, 0.2),
                    pt(move |f| side * (w(f) + 0.2), 0.36, 0.35),
                    pt(move |f| side * (w(f) + 0.28), 1.0, 0.7),
                    pt(move |f| side * (w(f) + 0.42), 1.0, 0.8),
                    pt(move |f| side * (w(f) + 0.62), 0.25, 0.9),
                    pt(move |f| side * (w(f) + 0.62), -1.8, 1.0),
                ];
                if side < 0.0 {
                    prof.reverse();
                }
                let g = extrude(t, &r, &prof, &opts(4.0, 3.0));
                self.add_mesh(c, g, concrete, true, true);
            }

            // Kerb + pipe railing (seafront promenade).
            let rr = runs(
                |s| arr[t.idx(s)] == 5 && !gaps.iter().any(|g| s > g.s0 && s < g.s1),
                1.0,
                0.0,
                t.length,
            );
            let paint = self.mat(c, "railPaint", |_| {
                Material::standard()
                    .set("color", 0xdfe6e8)
                    .set("metalness", 0.4)
                    .set("roughness", 0.45)
                    .set("side", DOUBLE_SIDE)
            });
            for r in group_runs(&rr, 560.0) {
                let mut kerb = vec![
                    pt(move |f| side * (w(f) - 0.05), 0.0, 0.0),
                    pt(move |f| side * (w(f) - 0.05), 0.22, 0.3),
                    pt(move |f| side * (w(f) + 0.35), 0.22, 0.7),
                    pt(move |f| side * (w(f) + 0.35), -1.2, 1.0),
                ];
                if side < 0.0 {
                    kerb.reverse();
                }
                let gk = extrude(t, &r, &kerb, &opts(4.0, 3.0));
                self.add_mesh(c, gk, concrete, false, true);
                for h in [0.62, 1.08] {
                    let bar = [
                        pt(move |f| side * (w(f) + 0.15), h - 0.03, 0.0),
                        pt(move |f| side * (w(f) + 0.15), h + 0.03, 1.0),
                    ];
                    let gb = extrude(t, &r, &bar, &opts(3.0, 3.0));
                    self.add_mesh(c, gb, paint, true, false);
                }
            }
            self.post_instances(c, &rr, side, 2.5, paint, [0.06, 1.1, 0.06], 0.15);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn post_instances(
        &self,
        c: &mut RoadCtx,
        ranges: &[[f64; 2]],
        side: f64,
        spacing: f64,
        material: MaterialId,
        size: [f64; 3],
        extra: f64,
    ) {
        let t = c.track;
        let mut list = Vec::new();
        for &[s0, s1] in ranges {
            let mut s = s0;
            while s <= s1 {
                list.push(s);
                s += spacing;
            }
        }
        if list.is_empty() {
            return;
        }
        let mut geo = box_geometry(size[0], size[1], size[2], 1.0, 1.0, 1.0);
        geo.translate(0.0, size[1] / 2.0, 0.0);
        let gid = c.graph.add_geometry(geo);
        let im = c.graph.instanced_mesh(gid, material, list.len() as u32);
        for (k, &s) in list.iter().enumerate() {
            let f = t.frame(s);
            let lat = side * ((if side < 0.0 { f.wall_l } else { f.wall_r }) + extra);
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let y = f.y - lat * f.bank - 0.1;
            let q = yaw_quaternion(-kernel::atan2(f.fz, f.fx));
            let m4 = Matrix4::compose(Vector3::new(x, y, z), q, Vector3::new(1.0, 1.0, 1.0));
            instances(c.graph, im).set_matrix_at(k, &m4);
        }
        let o = c.graph.get_mut(im);
        o.cast_shadow = true;
        o.receive_shadow = true;
        c.graph.compute_instance_bounding_sphere(im);
        c.graph.add(self.group, im);
    }

    pub fn build_chevrons(&mut self, c: &mut RoadCtx) {
        let t = c.track;
        let tex = c
            .graph
            .cached_texture(&c.textures.chevron_texture(), Layer::Main, "");
        let mat = c.graph.add_material(
            Material::standard()
                .set("map", tex)
                .set("roughness", 0.5)
                .set("emissive", 0xffffff)
                .set("emissiveMap", tex)
                .set("emissiveIntensity", 0.12)
                .set("side", DOUBLE_SIDE),
        );
        self.materials.push(("chevron".into(), mat));
        struct Item {
            s: f64,
            side: f64,
            dir: f64,
        }
        let mut items = Vec::new();
        let mut last = -100.0;
        let mut s = 0.0;
        while s < t.length {
            let k = f64::from(t.k_smooth[t.idx(s)]);
            let key = road_type_at(t, t.idx(s)).key;
            let skip = [
                "freeway",
                "boulevard",
                "street",
                "playa",
                "circuit",
                "circuitWide",
            ]
            .contains(&key);
            if !skip && k.abs() > 0.03 && s - last > 11.0 {
                items.push(Item {
                    s,
                    side: if k > 0.0 { -1.0 } else { 1.0 },
                    dir: if k > 0.0 { 1.0 } else { -1.0 },
                });
                last = s;
            }
            s += 1.0;
        }
        if items.is_empty() {
            return;
        }
        let mut plate = plane_geometry(0.75, 0.75, 1.0, 1.0);
        plate.translate(0.0, 1.35, 0.0);
        let mut pole = cylinder_geometry(0.04, 0.04, 1.1, 5.0, 1.0, false, 0.0, 2.0 * PI);
        pole.translate(0.0, 0.55, 0.0);
        let post = self.mat(c, "post", |_| Material::standard().set("color", 0x777777));
        let gp = c.graph.add_geometry(plate);
        let gq = c.graph.add_geometry(pole);
        let n = items.len() as u32;
        let im_plate = c.graph.instanced_mesh(gp, mat, n);
        let im_pole = c.graph.instanced_mesh(gq, post, n);
        for (k, it) in items.iter().enumerate() {
            let f = t.frame(it.s);
            let lat = it.side * ((if it.side < 0.0 { f.wall_l } else { f.wall_r }) + 0.9);
            let v = Vector3::new(f.x + f.rx * lat, f.y - lat * f.bank, f.z + f.rz * lat);
            // Face back down the road toward approaching drivers.
            let q = yaw_quaternion(kernel::atan2(-f.fx, -f.fz));
            let m4 = Matrix4::compose(v, q, Vector3::new(it.dir, 1.0, 1.0));
            instances(c.graph, im_plate).set_matrix_at(k, &m4);
            let m4 = Matrix4::compose(v, q, Vector3::new(1.0, 1.0, 1.0));
            instances(c.graph, im_pole).set_matrix_at(k, &m4);
        }
        c.graph.compute_instance_bounding_sphere(im_plate);
        c.graph.compute_instance_bounding_sphere(im_pole);
        c.graph.add(self.group, im_plate);
        c.graph.add(self.group, im_pole);
    }

    pub fn build_viaduct(&mut self, c: &mut RoadCtx) {
        // Elevated freeway: piers and a deck underside where the road is well
        // above the city ground.
        let (t, tr) = (c.track, c.terrain);
        let elevated = |s: f64| tr.is_elevated(t, t.idx(s));
        // Cable-stayed main spans have no piers; the harbour scenery builds those.
        let spans: Vec<[f64; 2]> = t
            .tags
            .iter()
            .filter(|g| g.tag == "bridge")
            .map(|g| [g.s0, g.s1])
            .collect();
        let concrete = self.mat(c, "concrete", |c| {
            let map = c
                .graph
                .cached_texture(&c.textures.concrete_texture(), Layer::Main, "");
            Material::standard().set("map", map).set("roughness", 0.9)
        });
        let ranges = runs(elevated, 2.0, 0.0, t.length);
        for r in group_runs(&ranges, 560.0) {
            let g = extrude(
                t,
                &r,
                &[
                    ProfilePoint::new(Lat::f(|f, _| f.wall_r + 0.62)).dy(-1.8),
                    ProfilePoint::new(Lat::f(|f, _| -(f.wall_l + 0.62))).dy(-1.8),
                ],
                &opts(4.0, 8.0),
            );
            self.add_mesh(c, g, concrete, false, false);
        }
        let mut piers = Vec::new();
        for &[s0, s1] in &ranges {
            let mut s = s0 + 15.0;
            while s < s1 - 5.0 {
                if !spans.iter().any(|g| s > g[0] - 10.0 && s < g[1] + 10.0) {
                    piers.push(s);
                }
                s += 32.0;
            }
        }
        if piers.is_empty() {
            return;
        }
        let mut col_geo = cylinder_geometry(1.1, 1.3, 1.0, 10.0, 1.0, false, 0.0, 2.0 * PI);
        col_geo.translate(0.0, 0.5, 0.0);
        let cap_geo = box_geometry(2.2, 1.2, 1.0, 1.0, 1.0, 1.0);
        let gc = c.graph.add_geometry(col_geo);
        let gk = c.graph.add_geometry(cap_geo);
        let im_col = c.graph.instanced_mesh(gc, concrete, piers.len() as u32 * 2);
        let im_cap = c.graph.instanced_mesh(gk, concrete, piers.len() as u32);
        for (k, &s) in piers.iter().enumerate() {
            let f = t.frame(s);
            let q = yaw_quaternion(-kernel::atan2(f.fz, f.fx));
            let top = f.y - 2.8;
            for (j, lat) in [(0usize, -f.hw * 0.55), (1, f.hw * 0.55)] {
                let x = f.x + f.rx * lat;
                let z = f.z + f.rz * lat;
                let g = tr.height_at(x, z) - 0.5;
                let m4 = Matrix4::compose(
                    Vector3::new(x, g, z),
                    q,
                    Vector3::new(1.0, js::max(0.5, top - g), 1.0),
                );
                instances(c.graph, im_col).set_matrix_at(k * 2 + j, &m4);
            }
            // Crossbeam: local z maps onto the road's right vector, so stretch z.
            let m4 = Matrix4::compose(
                Vector3::new(f.x, top + 0.4, f.z),
                q,
                Vector3::new(1.0, 1.0, f.hw * 2.0 + 2.0),
            );
            instances(c.graph, im_cap).set_matrix_at(k, &m4);
        }
        c.graph.get_mut(im_col).cast_shadow = true;
        c.graph.get_mut(im_cap).cast_shadow = true;
        c.graph.compute_instance_bounding_sphere(im_col);
        c.graph.compute_instance_bounding_sphere(im_cap);
        c.graph.add(self.group, im_col);
        c.graph.add(self.group, im_cap);
    }
}
