//! Desert's updater (`animate`, pushed onto `world.updaters` by `build()`)
//! as one [`Animator`]: the glows' shared clock and colours, the flame,
//! beam and pool opacities, the freight train (`updateTrain`) and the
//! tumbleweeds rolling across the highway (`updateWeeds`), and helpers the
//! build shares with it.
//!
//! The JS reads the rendered ground (`this.gy`) and the track every frame.
//! The animator keeps a clone of the track and, of the ground, only the
//! lattice heights of the terrain tiles the tumbleweeds can reach
//! ([`GroundPatch`], the same arithmetic as `valley::ground`), so that the
//! terrain itself is not kept alive by the scene (DECISIONS D553). The
//! tumbleweeds draw from the page's `Math.random` in the JS; here from a
//! mulberry32 stream of their own (D552).

use std::sync::Arc;

use mr_math::{Mulberry32, js, kernel, smoothstep};
use mr_track::Track;

use super::{Bld, Rail, RailPt, Range, TrainCar};
use crate::color::Color;
use crate::object::{MaterialId, NodeId, SceneGraph};
use crate::terrain::{TERRAIN_TILE, Terrain, Tile};
use crate::three_geom::{Euler, EulerOrder, Matrix4, Quaternion, Vector3};
use crate::valley::ground::Ground;
use crate::world::{Animator, Change, Edit, Handle, UpdateCtx};

/// The default seed of the tumbleweeds' generator: the seed the scene
/// captures give the page's `Math.random` (`tools/parity/lib/
/// seed-random.mjs`), which `tools/parity/desert-animators.mjs` reseeds
/// before its frames.
pub const RANDOM_SEED: u32 = 0x5eed;

// ── Helpers shared with the build ───────────────────────────────────────

/// `root.updateMatrixWorld(true)` for a hierarchy not in the scene, then
/// `root.traverse`: each node, its world matrix and its parent's name, in
/// traversal order (pre-order, children in order).
pub fn world_matrices(
    graph: &mut SceneGraph,
    root: NodeId,
) -> Vec<(NodeId, Matrix4, Option<String>)> {
    let mut out = Vec::new();
    let mut stack: Vec<(NodeId, Option<Matrix4>, Option<String>)> = vec![(root, None, None)];
    while let Some((id, parent, pname)) = stack.pop() {
        let o = graph.get_mut(id);
        if o.matrix_auto_update {
            o.update_matrix();
        }
        let world = match parent {
            Some(p) => Matrix4::multiply_matrices(&p, &o.matrix),
            None => o.matrix,
        };
        let name = o.name.clone();
        for &c in o.children.iter().rev() {
            stack.push((c, Some(world), Some(name.clone())));
        }
        out.push((id, world, pname));
    }
    out
}

/// `buildCars`' material signature: `[type, color hex, emissive hex,
/// emissiveIntensity, roughness, metalness, map uuid].join('|')`. Only its
/// equality matters (it names a bucket whose mesh is renamed), so numbers
/// are written as Rust writes them and a texture by its handle.
pub fn material_sig(graph: &SceneGraph, m: MaterialId) -> String {
    let mt = graph.material(m);
    let hex = |k: &str| {
        mt.color(k)
            .map_or(String::new(), |c| format!("{:06x}", c.get_hex()))
    };
    let num = |k: &str| {
        mt.number(k)
            .map_or(String::new(), |v| format!("{}", v + 0.0))
    };
    let map = match mt.get("map").and_then(|v| v.get("texture")) {
        Some(t) => t.to_string(),
        None => String::new(),
    };
    format!(
        "{}|{}|{}|{}|{}|{}|{}",
        mt.desc.ty,
        hex("color"),
        hex("emissive"),
        num("emissiveIntensity"),
        num("roughness"),
        num("metalness"),
        map
    )
}

// ── The rendered ground, where the tumbleweeds roll ─────────────────────

struct PatchTile {
    x0: f64,
    z0: f64,
    step: f64,
    /// Lattice points per side.
    n: usize,
    /// `terrain.heightAt` at `(x0 + ci·step, z0 + cj·step)`, row `cj`.
    h: Vec<f64>,
}

/// `makeGround(terrain).height` over the tiles a region touches: the
/// heights at those tiles' lattice points, and the same triangle
/// interpolation. Outside them it is NaN.
pub struct GroundPatch {
    min_x: f64,
    min_z: f64,
    nx: i64,
    nz: i64,
    tiles: Vec<Option<PatchTile>>,
}

impl GroundPatch {
    /// The tiles `keep(i, j)` selects.
    pub fn new(terrain: &Terrain, tiles: &[Tile], keep: impl Fn(i64, i64) -> bool) -> GroundPatch {
        let nx = tiles.iter().map(|t| t.i + 1).max().unwrap_or(0);
        let nz = tiles.iter().map(|t| t.j + 1).max().unwrap_or(0);
        let mut out = Vec::with_capacity(tiles.len());
        for t in tiles {
            if !keep(t.i, t.j) {
                out.push(None);
                continue;
            }
            // One lattice point past the far edge, for a point on it.
            let n = (t.size / t.step) as usize + 2;
            let mut h = Vec::with_capacity(n * n);
            for cj in 0..n {
                for ci in 0..n {
                    let x = t.x0 + ci as f64 * t.step;
                    let z = t.z0 + cj as f64 * t.step;
                    h.push(terrain.height_at(x, z));
                }
            }
            out.push(Some(PatchTile {
                x0: t.x0,
                z0: t.z0,
                step: t.step,
                n,
                h,
            }));
        }
        GroundPatch {
            min_x: terrain.min_x,
            min_z: terrain.min_z,
            nx,
            nz,
            tiles: out,
        }
    }

    /// `ground.height(x, z)`.
    pub fn height(&self, x: f64, z: f64) -> f64 {
        let i = ((x - self.min_x) / TERRAIN_TILE).floor();
        let j = ((z - self.min_z) / TERRAIN_TILE).floor();
        if !(i >= 0.0 && j >= 0.0 && i < self.nx as f64 && j < self.nz as f64) {
            return f64::NAN;
        }
        let Some(tile) = &self.tiles[(j as i64 * self.nx + i as i64) as usize] else {
            return f64::NAN;
        };
        let st = tile.step;
        let ci = ((x - tile.x0) / st).floor();
        let cj = ((z - tile.z0) / st).floor();
        if ci < 0.0 || cj < 0.0 || ci as usize + 1 >= tile.n || cj as usize + 1 >= tile.n {
            return f64::NAN;
        }
        let (ii, jj) = (ci as usize, cj as usize);
        // The lattice is exact: tiles start on whole multiples of 256 m and
        // steps are 4, 16 or 32 m, so `x0 + ci·st + st` is the next point.
        let at = |a: usize, b: usize| tile.h[(jj + b) * tile.n + ii + a];
        let x0 = tile.x0 + ci * st;
        let z0 = tile.z0 + cj * st;
        let fx = (x - x0) / st;
        let fz = (z - z0) / st;
        let a = at(0, 0);
        let b = at(1, 0);
        let c = at(0, 1);
        let d = at(1, 1);
        if js::to_int32(ci + cj) & 1 != 0 {
            if fx + fz <= 1.0 {
                return a + fx * (b - a) + fz * (c - a);
            }
            return d + (1.0 - fx) * (c - d) + (1.0 - fz) * (b - d);
        }
        if fz >= fx {
            return a + fx * (d - c) + fz * (c - a);
        }
        a + fx * (b - a) + fz * (d - b)
    }
}

/// The tiles within reach of the tumbleweeds: every tile within 300 m of
/// the road from 400 m before Route 66 to 800 m into Silver Lake.
fn weed_patch(t: &Track, terrain: &Terrain, ground: &Ground, z: &[Range]) -> GroundPatch {
    let mut keep = std::collections::BTreeSet::new();
    let mut s = js::max(0.0, z[1].s0 - 400.0);
    let s1 = z[2].s0 + 800.0;
    while s < s1 {
        let f = t.frame(s);
        let mut lat = -300.0;
        while lat <= 300.0 {
            let x = f.x + f.rx * lat;
            let zz = f.z + f.rz * lat;
            let i = ((x - terrain.min_x) / TERRAIN_TILE).floor() as i64;
            let j = ((zz - terrain.min_z) / TERRAIN_TILE).floor() as i64;
            keep.insert((i, j));
            lat += 8.0;
        }
        s += 8.0;
    }
    GroundPatch::new(terrain, &ground.tiles, |i, j| keep.contains(&(i, j)))
}

// ── The animator ────────────────────────────────────────────────────────

/// One rolling tumbleweed (`this.rollers.list[i]`).
#[derive(Clone, Copy, Debug)]
struct Weed {
    live: bool,
    q: Quaternion,
    x: f64,
    z: f64,
    vx: f64,
    vz: f64,
    r: f64,
    age: f64,
    bounce: f64,
    by: f64,
    life: f64,
}

struct TrainAnim {
    cars: Vec<TrainCar>,
    len: f64,
    glow: [NodeId; 3],
    glow_mat: MaterialId,
    beam: NodeId,
    beam_color: [f64; 3],
    target: NodeId,
    speed: f64,
    rail: Rail,
    /// `this.trainU` (`null` until the first frame).
    u: Option<f64>,
    rolling: bool,
    /// `this._uCache`, `this._sCache`.
    cache: Option<(f64, f64)>,
}

/// `animate(dt, night, camera, s)`.
pub struct DesertAnimator {
    time: f64,
    z1: f64,
    z2: f64,
    track: Arc<Track>,
    ground: Arc<GroundPatch>,
    glow_mats: Vec<MaterialId>,
    flare: Option<MaterialId>,
    fire: Option<MaterialId>,
    lantern: Option<MaterialId>,
    flood: Option<MaterialId>,
    lamp: Option<MaterialId>,
    refl: Option<MaterialId>,
    bulb: Option<MaterialId>,
    string: Option<MaterialId>,
    bb_lamp: Option<MaterialId>,
    flame: Option<MaterialId>,
    beam: Option<MaterialId>,
    pool: Option<MaterialId>,
    train: Option<TrainAnim>,
    rollers: Option<(NodeId, Vec<Weed>)>,
    random: Mulberry32,
}

impl DesertAnimator {
    pub(crate) fn new(b: &Bld<'_>, seed: u32) -> DesertAnimator {
        let train = match (&b.train, &b.rail) {
            (Some(tr), Some(rail)) => {
                let beam_color = b
                    .graph
                    .get(tr.beam)
                    .light
                    .as_ref()
                    .map_or([1.0; 3], |l| l.color);
                Some(TrainAnim {
                    cars: tr.cars.clone(),
                    len: tr.len,
                    glow: tr.glow,
                    glow_mat: tr.glow_mat,
                    beam: tr.beam,
                    beam_color,
                    target: tr.target,
                    speed: tr.speed,
                    rail: rail.clone(),
                    u: None,
                    rolling: false,
                    cache: None,
                })
            }
            _ => None,
        };
        let weed = Weed {
            live: false,
            q: Quaternion::IDENTITY,
            x: 0.0,
            z: 0.0,
            vx: 0.0,
            vz: 0.0,
            r: 0.0,
            age: 0.0,
            bounce: 0.0,
            by: 0.0,
            life: 0.0,
        };
        DesertAnimator {
            time: 0.0,
            z1: b.z[1].s0,
            z2: b.z[2].s0,
            track: Arc::new(b.t.clone()),
            ground: Arc::new(weed_patch(b.t, b.terrain, &b.ground, &b.z)),
            glow_mats: b.glow_mats.clone(),
            flare: b.flare_mat,
            fire: b.fire_mat,
            lantern: b.lantern_mat,
            flood: b.flood_mat,
            lamp: b.lamp_mat,
            refl: b.refl_mat,
            bulb: b.bulb_mat,
            string: b.string_mat,
            bb_lamp: b.bb_lamp_mat,
            flame: b.flame_mat,
            beam: b.beam_mat,
            pool: b.pool_mat,
            train,
            rollers: b.rollers.map(|n| (n, vec![weed; 7])),
            random: Mulberry32::new(seed),
        }
    }
}

fn mat_edit(out: &mut Vec<Edit>, m: MaterialId, change: Change) {
    out.push(Edit {
        target: Handle::Material(m),
        change,
    });
}

fn color(out: &mut Vec<Edit>, m: Option<MaterialId>, rgb: [f64; 3], k: f64) {
    if let Some(m) = m {
        let mut c = Color::new(rgb[0], rgb[1], rgb[2]);
        c.multiply_scalar(k);
        mat_edit(
            out,
            m,
            Change::Color {
                prop: "color",
                rgb: [c.r, c.g, c.b],
            },
        );
    }
}

fn opacity(out: &mut Vec<Edit>, m: Option<MaterialId>, v: f64) {
    if let Some(m) = m {
        mat_edit(
            out,
            m,
            Change::Number {
                prop: "opacity",
                value: v,
            },
        );
    }
}

fn f32s(m: &Matrix4) -> [f32; 16] {
    m.elements.map(|v| v as f32)
}

fn instance(out: &mut Vec<Edit>, n: NodeId, index: u32, m: &Matrix4) {
    out.push(Edit {
        target: Handle::Node(n),
        change: Change::InstanceMatrix {
            index,
            matrix: f32s(m),
        },
    });
}

fn transform(out: &mut Vec<Edit>, n: NodeId, p: [f64; 3], s: f64) {
    out.push(Edit {
        target: Handle::Node(n),
        change: Change::Transform {
            position: p,
            quaternion: [0.0, 0.0, 0.0, 1.0],
            scale: [s, s, s],
        },
    });
}

impl Animator for DesertAnimator {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        let dt = u.dt;
        self.time += dt;
        let t = self.time;
        let k = smoothstep(0.15, 0.7, u.night);
        // glowTime.value = T: every material that shares it.
        for &m in &self.glow_mats {
            mat_edit(
                out,
                m,
                Change::Number {
                    prop: "uTime",
                    value: t,
                },
            );
        }
        color(out, self.flare, [3.2, 0.4, 0.15], 0.25 + 0.75 * k);
        color(out, self.fire, [2.6, 1.2, 0.35], 0.3 + 0.7 * k);
        color(out, self.lantern, [2.4, 1.7, 0.9], k);
        color(out, self.flood, [2.2, 2.3, 2.5], 0.15 + 0.85 * k);
        color(out, self.lamp, [2.2, 2.3, 2.5], 0.1 + 0.9 * k);
        color(out, self.refl, [2.5, 1.4, 0.3], k);
        color(out, self.bulb, [2.6, 1.9, 1.0], 0.35 + 0.65 * k);
        color(out, self.string, [2.4, 1.6, 0.8], k);
        color(out, self.bb_lamp, [2.4, 2.1, 1.6], k);
        opacity(out, self.flame, 0.35 + 0.5 * k);
        opacity(out, self.beam, 0.13 * smoothstep(0.3, 0.8, u.night));
        opacity(out, self.pool, k);
        self.update_train(dt, u.night, u.s, out);
        self.update_weeds(dt, u.s, out);
    }
}

impl DesertAnimator {
    /// Road s → rail u (by projection on the nearest rail point).
    fn rail_u_for(track: &Track, rail: &Rail, s: f64) -> f64 {
        let f = track.frame(s);
        let x = f.x + f.rx * super::RAIL_LAT;
        let z = f.z + f.rz * super::RAIL_LAT;
        let p = &rail.pts;
        let mut best = 0;
        let mut bd = f64::INFINITY;
        let mut i = 0;
        while i < p.len() {
            let d = kernel::pow(p[i].x - x, 2.0) + kernel::pow(p[i].z - z, 2.0);
            if d < bd {
                bd = d;
                best = i;
            }
            i += 4;
        }
        p[best].u
    }

    fn update_train(&mut self, dt: f64, night: f64, s: f64, out: &mut Vec<Edit>) {
        let time = self.time;
        let start = self.z1 - 500.0;
        let track = self.track.clone();
        let Some(tr) = &mut self.train else {
            return;
        };
        let (uc, sc) = match tr.cache {
            Some((uc, sc)) if (s - sc).abs() <= 20.0 => (uc, sc),
            _ => {
                let uc = Self::rail_u_for(&track, &tr.rail, s);
                tr.cache = Some((uc, s));
                (uc, s)
            }
        };
        let pu = uc + (s - sc);
        if s < start || tr.u.is_none() {
            // Waiting: the whole train sits ahead of the player, tail 150 m
            // ahead.
            tr.u = Some(pu + tr.len + 150.0);
            if s < start {
                tr.rolling = false;
            }
        }
        if s >= start {
            tr.rolling = true;
        }
        if tr.rolling {
            tr.u = Some(tr.u.expect("set") + tr.speed * dt);
        }
        let head = tr.u.expect("set");
        let one = Vector3::splat(1.0);
        for c in &tr.cars {
            let u = head - c.off;
            if u < c.l || u > tr.rail.len - c.l {
                instance(out, c.mesh, c.idx, &Matrix4::make_scale(0.0, 0.0, 0.0));
            } else {
                let a = tr.rail.at(u + c.l * 0.36);
                let bp = tr.rail.at(u - c.l * 0.36);
                let dx = a.x - bp.x;
                let dy = a.y - bp.y;
                let dz = a.z - bp.z;
                let yaw = kernel::atan2(-dz, dx);
                let pitch = kernel::atan2(dy, kernel::hypot(dx, dz));
                let q =
                    Quaternion::from_euler(&Euler::with_order(0.0, yaw, pitch, EulerOrder::YZX));
                let p = Vector3::new(
                    (a.x + bp.x) / 2.0,
                    (a.y + bp.y) / 2.0 + 0.25,
                    (a.z + bp.z) / 2.0,
                );
                instance(out, c.mesh, c.idx, &Matrix4::compose(p, q, one));
            }
        }
        // Lights on the lead locomotive.
        let lead = tr.cars[0];
        let lu = head - lead.off + lead.l / 2.0;
        let f: RailPt = tr.rail.at(lu);
        let rx = -f.fz;
        let rz = f.fx;
        let k = 0.25 + 0.75 * smoothstep(0.15, 0.6, night);
        let blink = kernel::sin(time * 6.0) > 0.0;
        transform(
            out,
            tr.glow[0],
            [f.x + f.fx * 0.4, f.y + 3.45, f.z + f.fz * 0.4],
            3.0 + 4.0 * k,
        );
        transform(
            out,
            tr.glow[1],
            [
                f.x + f.fx * 0.3 + rx * 1.0,
                f.y + 1.6,
                f.z + f.fz * 0.3 + rz * 1.0,
            ],
            (if blink { 2.5 } else { 1.2 }) * (0.5 + k),
        );
        transform(
            out,
            tr.glow[2],
            [
                f.x + f.fx * 0.3 - rx * 1.0,
                f.y + 1.6,
                f.z + f.fz * 0.3 - rz * 1.0,
            ],
            (if !blink { 2.5 } else { 1.2 }) * (0.5 + k),
        );
        opacity(out, Some(tr.glow_mat), 0.5 + 0.5 * k);
        transform(
            out,
            tr.beam,
            [f.x + f.fx * 0.5, f.y + 4.2, f.z + f.fz * 0.5],
            1.0,
        );
        transform(
            out,
            tr.target,
            [f.x + f.fx * 40.0, f.y, f.z + f.fz * 40.0],
            1.0,
        );
        out.push(Edit {
            target: Handle::Node(tr.beam),
            change: Change::Light {
                color: tr.beam_color,
                intensity: 90.0 * smoothstep(0.3, 0.8, night),
                ground_color: None,
            },
        });
    }

    /// Tumbleweeds bowling across the highway on the wind, recycled round
    /// the player: each rolls in from the left, crosses, and dies far right.
    fn update_weeds(&mut self, dt: f64, s: f64, out: &mut Vec<Edit>) {
        let time = self.time;
        let track = self.track.clone();
        let ground = self.ground.clone();
        let random = &mut self.random;
        let Some((im, list)) = &mut self.rollers else {
            return;
        };
        let in_zone = s > self.z1 - 200.0 && s < self.z2 + 300.0;
        for (i, w) in list.iter_mut().enumerate() {
            if !w.live {
                if !in_zone || random.next_f64() > dt * 0.8 {
                    instance(out, *im, i as u32, &Matrix4::make_scale(0.0, 0.0, 0.0));
                    continue;
                }
                let ss = s + 30.0 + random.next_f64() * 160.0;
                let f = track.frame(ss);
                let lat = -(f.wall_l + 25.0 + random.next_f64() * 20.0);
                w.x = f.x + f.rx * lat;
                w.z = f.z + f.rz * lat;
                // Wind from the left, a little along the road.
                let sp = 3.5 + random.next_f64() * 4.0;
                w.vx = (f.rx + f.fx * (random.next_f64() - 0.3) * 0.6) * sp;
                w.vz = (f.rz + f.fz * (random.next_f64() - 0.3) * 0.6) * sp;
                w.r = 0.55 + random.next_f64() * 0.35;
                w.age = 0.0;
                w.bounce = 0.0;
                w.by = 0.0;
                w.life = (70.0 + f.wall_r + f.wall_l) / sp;
                w.live = true;
            }
            w.age += dt;
            // Hops: a damped bounce every so often.
            w.by -= 9.8 * dt;
            w.bounce += w.by * dt;
            if w.bounce <= 0.0 {
                w.bounce = 0.0;
                w.by = if random.next_f64() < 0.35 {
                    2.0 + random.next_f64() * 2.5
                } else {
                    0.0
                };
            }
            let gust = 1.0 + 0.35 * kernel::sin(time * 1.7 + i as f64);
            w.x += w.vx * dt * gust;
            w.z += w.vz * dt * gust;
            let v = kernel::hypot(w.vx, w.vz) * gust;
            let ax = Vector3::new(w.vz, 0.0, -w.vx).normalize();
            let dq = Quaternion::from_axis_angle(ax, (v * dt) / w.r);
            w.q = w.q.premultiply(dq);
            let fade = js::min_n(&[1.0, w.age / 0.6, (w.life - w.age) / 0.6]);
            let p = Vector3::new(w.x, ground.height(w.x, w.z) + w.r * 0.9 + w.bounce, w.z);
            let sc = w.r * js::max(0.0, fade);
            instance(
                out,
                *im,
                i as u32,
                &Matrix4::compose(p, w.q, Vector3::splat(sc)),
            );
            if w.age > w.life {
                w.live = false;
            }
        }
    }
}
