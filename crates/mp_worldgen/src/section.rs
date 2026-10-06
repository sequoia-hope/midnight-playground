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

use crate::object::{NodeId, SceneGraph};
use crate::terrain::Tile;
use crate::three_geom::math::{Matrix4, Vector3};
use crate::world::World;
use mp_track::Track;

/// A stretch of the route and how far around it the section reaches.
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
}

/// Points of the stretch every this many metres.
const STEP: f64 = 10.0;

impl Section {
    /// The stretch as points (x, z) every [`STEP`] m.
    pub fn points(&self, track: &Track) -> Vec<[f64; 2]> {
        let n = ((self.s1 - self.s0) / STEP).ceil().max(1.0) as usize;
        (0..=n)
            .map(|k| {
                let s = self.s0 + (self.s1 - self.s0) * k as f64 / n as f64;
                let f = track.frame(if track.is_loop { track.wrap(s) } else { s });
                [f.x, f.z]
            })
            .collect()
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
/// stretch for a section.
pub fn keep_tiles(w: &World, tiles: Vec<Tile>) -> Vec<Tile> {
    let (Some(sec), Some(track)) = (&w.section, &w.track) else {
        return tiles;
    };
    let pts = sec.points(track);
    let r2 = sec.terrain_radius * sec.terrain_radius;
    tiles
        .into_iter()
        .filter(|t| box_dist2(&pts, t.x0, t.z0, t.x0 + t.size, t.z0 + t.size) <= r2)
        .collect()
}

/// Whether the scenery module `name` builds: always for a level; for a
/// section, when one of the zones that name it reaches the stretch.
pub fn builds_module(w: &World, name: &str) -> bool {
    let (Some(sec), Some(track)) = (&w.section, &w.track) else {
        return true;
    };
    if !sec.scenery {
        return false;
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
            match reach(g, id, &world) {
                Some((c, radius)) if dist2(&pts, c.x, c.z).sqrt() - radius > r => {
                    far.push(id);
                    stats.cut += 1;
                    continue;
                }
                _ => stats.kept += 1,
            }
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

/// A finished section build: [`cut`], then the scene and its animators.
pub fn finish(b: crate::world::Build) -> (crate::world::WorldBuild, CutStats) {
    let mut w = b.world;
    let stats = cut(&mut w);
    (w.finish(), stats)
}
