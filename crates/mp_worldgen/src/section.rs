//! A section of a level: the level's own build restricted to a stretch of
//! its route, for the menu's flyover (DECISIONS D676, D740 on). Not part
//! of the JS game, which always builds a level whole.
//!
//! A section is the full build with three cuts, so what it draws is what
//! the level draws there (same seeds, same order of draws):
//!
//! - **Terrain:** only the tiles within [`Section::terrain_radius`] of the
//!   stretch are meshed (`terrain_mesh`'s tile jobs), at the steps and
//!   seams the whole grid gives them.
//! - **Scenery:** every module's `plan()` runs (its flattens and carves
//!   shape the land everywhere), but a module's `build()` runs only when
//!   one of its zones passes within [`Section::scenery_radius`] of the
//!   stretch.
//! - **The cut:** after the last job, [`cut`] takes every drawable whose
//!   bounds lie wholly beyond `scenery_radius` out of the tree (the road's
//!   chunks, the scenery's merged chunks), so the assembled scene, its
//!   meshes and its textures hold only what is near. Animators that
//!   address what was cut find nothing (`WorldBuild::resolve` drops
//!   those edits).
//!
//! With no section set ([`World::section`] `None`) nothing here runs, and
//! a build is the level's.

use crate::anim::AnimatorDesc;
use crate::object::{HandleMap, NodeId, SceneGraph};
use crate::terrain::Tile;
use crate::three_geom::math::{Matrix4, Vector3};
use crate::world::{World, WorldBuild};
use mp_track::Track;

/// A stretch of the route and how far around it the section reaches.
///
/// A streamed section (docs/vision/peninsula.md, stage 4b) sets
/// `partition`: it keeps only what belongs to its stretch, by the centre
/// of each drawable and tile, so adjacent sections share nothing and
/// together are the level; the road's chunks are clipped to the stretch
/// exactly. A `base` section is the horizon under them all: the terrain's
/// coarse tiles, the sky and the sea, nothing of the road or scenery.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// The stretch, along the route (wrapped on a loop).
    pub s0: f64,
    pub s1: f64,
    /// Terrain tiles closer than this (m, horizontally) are meshed.
    pub terrain_radius: f64,
    /// Scenery and road closer than this are kept.
    pub scenery_radius: f64,
    /// Whether the scenery modules near it build at all. False for the
    /// menu's simplified views (D748): their `plan()` still runs, so the
    /// land and the road are the level's, but nothing of theirs is built.
    pub scenery: bool,
    /// Keep by membership of the stretch (streaming), not by radius.
    pub partition: bool,
    /// The horizon layer: coarse tiles, sky and sea only.
    pub base: bool,
    /// The road of the network the stretch is on: 0 the level's road, k
    /// its (k - 1)th branch (`Track::road`; stage 4d). A partitioned
    /// section owns what is nearer its road than any other.
    pub road: usize,
}

impl Default for Section {
    fn default() -> Self {
        Section {
            s0: 0.0,
            s1: 0.0,
            terrain_radius: 1000.0,
            scenery_radius: 600.0,
            scenery: true,
            partition: false,
            base: false,
            road: 0,
        }
    }
}

/// How far a point may be from the road and still be looked up on it:
/// everything a section keeps stands within the kits' reach of a road.
const NEAR_ROAD: f64 = 900.0;

/// Points of the stretch every this many metres.
const STEP: f64 = 10.0;

impl Section {
    /// The road the stretch is on, of the level's network.
    pub fn track<'a>(&self, track: &'a Track) -> &'a Track {
        track.road(self.road.min(track.branches.len()))
    }

    /// The stretch as points (x, z) every [`STEP`] m.
    pub fn points(&self, track: &Track) -> Vec<[f64; 2]> {
        let t = self.track(track);
        let n = ((self.s1 - self.s0) / STEP).ceil().max(1.0) as usize;
        (0..=n)
            .map(|k| {
                let s = self.s0 + (self.s1 - self.s0) * k as f64 / n as f64;
                let f = t.frame(if t.is_loop {
                    t.wrap(s)
                } else {
                    s.min(t.length)
                });
                [f.x, f.z]
            })
            .collect()
    }

    /// The stretch's length along the route.
    pub fn len(&self) -> f64 {
        self.s1 - self.s0
    }

    /// Whether `s` (metres along the section's road) lies on the stretch,
    /// round a loop's wrap.
    pub fn holds(&self, track: &Track, s: f64) -> bool {
        self.holds_on(self.track(track), s)
    }

    /// [`Section::holds`] on the given road's own track.
    pub fn holds_on(&self, t: &Track, s: f64) -> bool {
        let d = if t.is_loop {
            t.wrap(s - self.s0)
        } else {
            s - self.s0
        };
        d >= 0.0 && d < self.len()
    }

    /// The place of (x, z) in the level's network: the nearest road within
    /// [`NEAR_ROAD`] and the metres along it.
    pub fn s_of(track: &Track, x: f64, z: f64) -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64, f64)> = None;
        for (r, t) in track.roads().enumerate() {
            let d = t.distance_to_road(x, z, NEAR_ROAD);
            let Some(s) = d.s else { continue };
            // The distance to the place itself: beyond an open road's end
            // the lateral distance from its last segment says nothing.
            let f = t.frame(if t.is_loop {
                t.wrap(s)
            } else {
                s.clamp(0.0, t.length)
            });
            let dd = mp_math::kernel::hypot(x - f.x, z - f.z);
            if dd <= NEAR_ROAD && best.is_none_or(|b| dd < b.2) {
                best = Some((r, s, dd));
            }
        }
        best.map(|(r, s, _)| (r, s))
    }

    /// Whether the point (x, z) belongs to the stretch: its nearest place
    /// in the network is on the section's road, on the stretch.
    pub fn owns(&self, track: &Track, x: f64, z: f64) -> bool {
        Self::s_of(track, x, z).is_some_and(|(r, s)| r == self.road && self.holds(track, s))
    }

    /// The parts of the range `[a, b]` of the road that lie on the
    /// stretch (none, one, or two round a loop's wrap), for the road's
    /// chunks: a partitioned section builds exactly its own metres.
    pub fn clip(&self, track: &Track, a: f64, b: f64) -> Vec<[f64; 2]> {
        if !self.partition {
            return vec![[a, b]];
        }
        let mut out = Vec::new();
        let candidates: &[f64] = if track.is_loop {
            &[-track.length, 0.0, track.length]
        } else {
            &[0.0]
        };
        for off in candidates {
            let (s0, s1) = (self.s0 + off, self.s1 + off);
            let (lo, hi) = (a.max(s0), b.min(s1));
            if hi > lo {
                out.push([lo, hi]);
            }
        }
        out
    }

    /// Whether a scenery module that owns these zones (indices into
    /// `track.zones`) reaches the section.
    pub fn wants_zones(&self, track: &Track, pts: &[[f64; 2]], zones: &[usize]) -> bool {
        zones.iter().any(|&z| {
            let Some(tz) = track.zones.get(z) else {
                return false;
            };
            let mut s = tz.s0;
            loop {
                let f = track.frame(s);
                if dist2(pts, f.x, f.z) <= self.scenery_radius * self.scenery_radius {
                    return true;
                }
                if s >= tz.s1 {
                    return false;
                }
                s = (s + 25.0).min(tz.s1);
            }
        })
    }
}

/// The squared horizontal distance from (x, z) to the nearest point.
fn dist2(pts: &[[f64; 2]], x: f64, z: f64) -> f64 {
    pts.iter()
        .map(|p| {
            let (dx, dz) = (p[0] - x, p[1] - z);
            dx * dx + dz * dz
        })
        .fold(f64::INFINITY, f64::min)
}

/// The squared horizontal distance from the nearest point to a box.
fn box_dist2(pts: &[[f64; 2]], x0: f64, z0: f64, x1: f64, z1: f64) -> f64 {
    pts.iter()
        .map(|p| {
            let dx = (x0 - p[0]).max(0.0).max(p[0] - x1);
            let dz = (z0 - p[1]).max(0.0).max(p[1] - z1);
            dx * dx + dz * dz
        })
        .fold(f64::INFINITY, f64::min)
}

/// The box (`[x0, z0, x1, z1]`) the section's terrain may reach: its
/// stretch's bounds widened by the terrain radius. `None` for a level.
pub fn bounds(w: &World) -> Option<[f64; 4]> {
    let (Some(sec), Some(track)) = (&w.section, &w.track) else {
        return None;
    };
    let pts = sec.points(track);
    let r = sec.terrain_radius;
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in &pts {
        b[0] = b[0].min(p[0] - r);
        b[1] = b[1].min(p[1] - r);
        b[2] = b[2].max(p[0] + r);
        b[3] = b[3].max(p[1] + r);
    }
    Some(b)
}

/// The tiles a build meshes: all of them for a level, those near the
/// stretch for a section; for a streamed section the near-road tiles
/// (4 m) whose centre is on its stretch, for the base every coarser one.
pub fn keep_tiles(w: &World, tiles: Vec<Tile>) -> Vec<Tile> {
    let (Some(sec), Some(track)) = (&w.section, &w.track) else {
        return tiles;
    };
    if sec.base {
        return tiles.into_iter().filter(|t| t.step > 4.0).collect();
    }
    if sec.partition {
        return tiles
            .into_iter()
            .filter(|t| t.step <= 4.0 && sec.owns(track, t.x0 + t.size / 2.0, t.z0 + t.size / 2.0))
            .collect();
    }
    let pts = sec.points(track);
    let r2 = sec.terrain_radius * sec.terrain_radius;
    tiles
        .into_iter()
        .filter(|t| box_dist2(&pts, t.x0, t.z0, t.x0 + t.size, t.z0 + t.size) <= r2)
        .collect()
}

/// Whether a box of land (`[x0, z0, x1, z1]`) is within the section's
/// scenery radius: always for a level. The Land kit plants a cell only
/// when it is (D1212).
pub fn wants_box(w: &World, x0: f64, z0: f64, x1: f64, z1: f64) -> bool {
    let (Some(sec), Some(track)) = (&w.section, &w.track) else {
        return true;
    };
    if !sec.scenery || sec.base {
        return false;
    }
    if sec.partition {
        return sec.owns(track, (x0 + x1) / 2.0, (z0 + z1) / 2.0);
    }
    let pts = sec.points(track);
    box_dist2(&pts, x0, z0, x1, z1) <= sec.scenery_radius * sec.scenery_radius
}

/// Whether the scenery module `name` builds: always for a level; for a
/// section, when one of the zones that name it reaches the stretch.
pub fn builds_module(w: &World, name: &str) -> bool {
    let (Some(sec), Some(track)) = (&w.section, &w.track) else {
        return true;
    };
    if !sec.scenery || sec.base {
        return false;
    }
    // The Land kit plants by land cover wherever a road is, whatever the
    // zone, as a whole build has it do, and fences the crop land (stage
    // 4d): it builds in every section of an open level.
    if name == "Land" && w.level.open {
        return true;
    }
    let zones: Vec<usize> = w
        .level
        .zones
        .iter()
        .enumerate()
        .filter(|(_, z)| z.scenery == name)
        .map(|(i, _)| i)
        .collect();
    sec.wants_zones(track, &sec.points(track), &zones)
}

/// What [`cut`] left and took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CutStats {
    pub kept: usize,
    pub cut: usize,
}

/// Takes every drawable whose bounds lie wholly beyond the section's
/// scenery radius out of the tree. Nothing for a level.
pub fn cut(w: &mut World) -> CutStats {
    let mut stats = CutStats::default();
    let (Some(sec), Some(track)) = (&w.section, &w.track) else {
        return stats;
    };
    // The base keeps all it built: the sky and the sea reach everywhere.
    if sec.base {
        return stats;
    }
    let pts = sec.points(track);
    let r = sec.scenery_radius;
    let g = &w.graph;
    let mut far: Vec<NodeId> = Vec::new();
    // Depth first from the roots with the world matrices, as the assembly
    // walks the tree.
    let mut stack: Vec<(NodeId, Matrix4)> = g
        .roots
        .iter()
        .rev()
        .map(|&n| (n, Matrix4::IDENTITY))
        .collect();
    while let Some((id, parent)) = stack.pop() {
        let o = g.get(id);
        let local = if o.matrix_auto_update {
            Matrix4::compose(o.position, o.quaternion, o.scale)
        } else {
            o.matrix
        };
        let world = Matrix4::multiply_matrices(&parent, &local);
        if o.geometry.is_some() {
            let gone = match reach(g, id, &world) {
                Some((c, _)) if sec.partition => !sec.owns(track, c.x, c.z),
                Some((c, radius)) => dist2(&pts, c.x, c.z).sqrt() - radius > r,
                None => false,
            };
            if gone {
                far.push(id);
                stats.cut += 1;
                continue;
            }
            stats.kept += 1;
        }
        for &c in o.children.iter().rev() {
            stack.push((c, world));
        }
    }
    for id in far {
        w.graph.detach(id);
    }
    // The geometry only what was cut drew is dropped now, not with the
    // world after the scene is assembled: less memory at the build's end
    // (D745).
    let mut used = vec![false; w.graph.geometries.len()];
    let mut stack: Vec<NodeId> = w.graph.roots.clone();
    while let Some(id) = stack.pop() {
        let o = w.graph.get(id);
        if let Some(g) = o.geometry {
            used[g.0 as usize] = true;
        }
        stack.extend(o.children.iter().copied());
    }
    for (g, used) in w.graph.geometries.iter_mut().zip(used) {
        if !used {
            *g = crate::three_geom::BufferGeometry::new();
        }
    }
    stats
}

/// A drawable's horizontal reach in world space: the centre and radius of
/// a sphere around its vertices (and, for an InstancedMesh, around every
/// drawn instance). `None` when it has no positions.
fn reach(g: &SceneGraph, id: NodeId, world: &Matrix4) -> Option<(Vector3, f64)> {
    let o = g.get(id);
    let geo = g.geometry(o.geometry?);
    let pos = geo.get_attribute("position")?;
    let n = pos.count();
    if n == 0 {
        return None;
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for i in 0..n {
        let v = pos.get_vector3(i);
        for (k, x) in [v.x, v.y, v.z].into_iter().enumerate() {
            lo[k] = lo[k].min(x);
            hi[k] = hi[k].max(x);
        }
    }
    let centre = Vector3::new(
        (lo[0] + hi[0]) / 2.0,
        (lo[1] + hi[1]) / 2.0,
        (lo[2] + hi[2]) / 2.0,
    );
    let half = Vector3::new(
        (hi[0] - lo[0]) / 2.0,
        (hi[1] - lo[1]) / 2.0,
        (hi[2] - lo[2]) / 2.0,
    );
    let radius = (half.x * half.x + half.y * half.y + half.z * half.z).sqrt();
    let Some(inst) = &o.instances else {
        return Some((
            centre.apply_matrix4(world),
            radius * crate::object::max_scale_on_axis(world),
        ));
    };
    // The instances' own centres, through the node's matrix.
    let (mut ilo, mut ihi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    let mut scale: f64 = 0.0;
    for i in 0..inst.count as usize {
        let m = inst.get_matrix_at(i);
        let s = crate::object::max_scale_on_axis(&m);
        if s == 0.0 {
            continue;
        }
        scale = scale.max(s);
        let c = centre.apply_matrix4(&m).apply_matrix4(world);
        ilo[0] = ilo[0].min(c.x);
        ilo[1] = ilo[1].min(c.z);
        ihi[0] = ihi[0].max(c.x);
        ihi[1] = ihi[1].max(c.z);
    }
    if ilo[0] > ihi[0] {
        return None;
    }
    let (hx, hz) = ((ihi[0] - ilo[0]) / 2.0, (ihi[1] - ilo[1]) / 2.0);
    Some((
        Vector3::new((ilo[0] + ihi[0]) / 2.0, 0.0, (ilo[1] + ihi[1]) / 2.0),
        (hx * hx + hz * hz).sqrt() + radius * scale * crate::object::max_scale_on_axis(world),
    ))
}

/// Every drawable of the world as built: the name of its nearest named
/// ancestor (its group: `road`, `terrain`, `land`, `coast`...) and the
/// centre of its reach, for the tests of what a section keeps.
pub fn drawables(w: &World) -> Vec<(String, [f64; 3])> {
    let g = &w.graph;
    let mut out = Vec::new();
    let mut stack: Vec<(NodeId, Matrix4, String)> = g
        .roots
        .iter()
        .rev()
        .map(|&n| (n, Matrix4::IDENTITY, String::new()))
        .collect();
    while let Some((id, parent, group)) = stack.pop() {
        let o = g.get(id);
        let local = if o.matrix_auto_update {
            Matrix4::compose(o.position, o.quaternion, o.scale)
        } else {
            o.matrix
        };
        let world = Matrix4::multiply_matrices(&parent, &local);
        let group = if o.name.is_empty() {
            group
        } else {
            o.name.clone()
        };
        if o.geometry.is_some()
            && let Some((c, _)) = reach(g, id, &world)
        {
            out.push((group.clone(), [c.x, c.y, c.z]));
        }
        for &c in o.children.iter().rev() {
            stack.push((c, world, group.clone()));
        }
    }
    out
}

/// Clears the kits' instances off the tarmac of an open level's branches
/// (D1218): the ported kits know only the level's road and plant across a
/// branch, at its mouth most of all. An instance whose origin lies on a
/// branch's paved width, near its surface, is scaled to nothing. The
/// roads' own furniture, the road-end barricades and the Land kit (which
/// keeps clear of every road) are left alone.
pub fn clear_branch_tarmac(w: &mut World) {
    let Some(track) = &w.track else { return };
    if !w.level.open || track.branches.is_empty() {
        return;
    }
    // Each branch's paved box, to pass most instances by at once.
    let boxes: Vec<[f64; 4]> = track
        .branches
        .iter()
        .map(|b| {
            let mut bx = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            let mut s = 0.0;
            while s <= b.track.length {
                let f = b.track.frame(s);
                bx = [
                    bx[0].min(f.x),
                    bx[1].min(f.z),
                    bx[2].max(f.x),
                    bx[3].max(f.z),
                ];
                s += 5.0;
            }
            [bx[0] - 20.0, bx[1] - 20.0, bx[2] + 20.0, bx[3] + 20.0]
        })
        .collect();
    let on_branch = |v: &Vector3| {
        track.branches.iter().zip(&boxes).any(|(b, bx)| {
            if v.x < bx[0] || v.x > bx[2] || v.z < bx[1] || v.z > bx[3] {
                return false;
            }
            let t = &b.track;
            let d = t.distance_to_road(v.x, v.z, 20.0);
            let Some(s) = d.s else { return false };
            if s < b.under || s > t.length {
                return false;
            }
            let f = t.frame(s);
            let y = t.surface_y(s, d.lat);
            d.lat.abs() < f.hw + 0.5 && v.y > y - 2.0 && v.y < y + 6.0
        })
    };
    let g = &w.graph;
    let mut hits: Vec<(NodeId, Vec<usize>)> = Vec::new();
    let mut stack: Vec<(NodeId, Matrix4, String)> = g
        .roots
        .iter()
        .map(|&n| (n, Matrix4::IDENTITY, String::new()))
        .collect();
    while let Some((id, parent, group)) = stack.pop() {
        let o = g.get(id);
        let local = if o.matrix_auto_update {
            Matrix4::compose(o.position, o.quaternion, o.scale)
        } else {
            o.matrix
        };
        let world = Matrix4::multiply_matrices(&parent, &local);
        let group = if o.name.is_empty() {
            group
        } else {
            o.name.clone()
        };
        if matches!(group.as_str(), "road" | "closed" | "land") {
            continue;
        }
        if let Some(inst) = &o.instances {
            let gone: Vec<usize> = (0..inst.count as usize)
                .filter(|&i| {
                    let m = inst.get_matrix_at(i);
                    crate::object::max_scale_on_axis(&m) > 0.0
                        && on_branch(
                            &Vector3::new(0.0, 0.0, 0.0)
                                .apply_matrix4(&m)
                                .apply_matrix4(&world),
                        )
                })
                .collect();
            if !gone.is_empty() {
                hits.push((id, gone));
            }
        }
        for &c in &o.children {
            stack.push((c, world, group.clone()));
        }
    }
    for (id, gone) in hits {
        if let Some(inst) = w.graph.get_mut(id).instances.as_mut() {
            for i in gone {
                inst.set_matrix_at(
                    i,
                    &Matrix4 {
                        elements: [0.0; 16],
                    },
                );
            }
        }
        w.graph.compute_instance_bounding_sphere(id);
    }
}

/// A finished section build: [`cut`], then the scene and its animators.
pub fn finish(b: crate::world::Build) -> (crate::world::WorldBuild, CutStats) {
    let mut w = b.world;
    let stats = cut(&mut w);
    (w.finish(), stats)
}

/// The packed form's magic.
const PACK_MAGIC: &[u8; 8] = b"MPSECT01";

/// What a packed section carries beside its scene.
#[derive(serde::Serialize, serde::Deserialize)]
struct Packed {
    descs: Vec<AnimatorDesc>,
    handles: HandleMap,
    log: Vec<String>,
    kept: usize,
    cut: usize,
}

/// A built section as bytes, for a worker to hand to the client (stage
/// 4c, D1215): the scene as a `.mrscene`, and beside it the animators
/// that are data, the handle map, the log and the cut's counts. An
/// animator that is a closure cannot be packed; the log says how many
/// were lost. The sim data is not carried: the plan's base has it.
pub fn pack(wb: &WorldBuild, stats: CutStats) -> Result<Vec<u8>, String> {
    let mut log = wb.log.clone();
    let lost = wb.animators.len().saturating_sub(wb.descs.len());
    if lost > 0 {
        log.push(format!("{lost} animators are not data; lost in the pack"));
    }
    let extra = serde_json::to_vec(&Packed {
        descs: wb.descs.clone(),
        handles: wb.handles.clone(),
        log,
        kept: stats.kept,
        cut: stats.cut,
    })
    .map_err(|e| format!("packing a section: {e}"))?;
    let scene = mp_scene::write(&wb.scene)?;
    let mut out = Vec::with_capacity(16 + extra.len() + scene.len());
    out.extend_from_slice(PACK_MAGIC);
    out.extend_from_slice(&(extra.len() as u32).to_le_bytes());
    out.extend_from_slice(&(scene.len() as u32).to_le_bytes());
    out.extend_from_slice(&extra);
    out.extend_from_slice(&scene);
    Ok(out)
}

/// A packed section read back: its scene, its animators made from their
/// descriptions, and the cut's counts.
pub fn unpack(bytes: &[u8]) -> Result<(WorldBuild, CutStats), String> {
    if bytes.len() < 16 || &bytes[..8] != PACK_MAGIC {
        return Err("not a packed section (bad magic)".into());
    }
    let extra_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let scene_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let extra = bytes
        .get(16..16 + extra_len)
        .ok_or("a packed section ends inside its header")?;
    let scene = bytes
        .get(16 + extra_len..16 + extra_len + scene_len)
        .ok_or("a packed section ends inside its scene")?;
    let p: Packed = serde_json::from_slice(extra).map_err(|e| format!("a packed section: {e}"))?;
    let scene = mp_scene::read(scene)?;
    let animators = p
        .descs
        .iter()
        .cloned()
        .map(AnimatorDesc::into_animator)
        .collect();
    Ok((
        WorldBuild {
            scene,
            animators,
            descs: p.descs,
            sim_data: mp_levels::world::WorldData {
                runout: 0.0,
                opposite_carriageway: None,
            },
            handles: p.handles,
            log: p.log,
            sky: None,
            on_countdown: None,
        },
        CutStats {
            kept: p.kept,
            cut: p.cut,
        },
    ))
}
