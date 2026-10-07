//! The living world (vision ROADMAP M14, `docs/vision/living-world.md`):
//! birds on every level, pelicans along the Coast Highway, fans at Seaside
//! Raceway and people in Seabright.
//!
//! [`Life`] is a scenery module the client adds after a level's own ones
//! (`world::level_jobs_with`), so the ported modules, and the parity
//! digests their tests hold, are untouched. Everything here is instanced:
//! birds are three instanced meshes (a body and two wings) for the whole
//! level, people six (two legs, a torso, a head, two arms). One animator
//! moves what is near the camera or the player each frame by rewriting
//! instance matrices; what is far keeps its last pose.
//!
//! Flight and walking are functions of time, not integrated state, so a
//! flock or a walker that comes back into range is where it should be.
//! Every inexact function goes through `mp_math::kernel`, and every random
//! choice through a seeded `Mulberry32`, as the rest of world generation.

mod birds;
mod people;

use mp_math::kernel;
use mp_track::Level;

use crate::three_geom::{Matrix4, Quaternion, Vector3};
use crate::world::{Animator, Change, Edit, Handle, Scenery, UpdateCtx, World};

pub use birds::Species;

/// The extra modules for a level (the client passes this to
/// `level_jobs_with`).
pub fn modules(level: &Level) -> Vec<Box<dyn Scenery>> {
    vec![Box::new(Life::new(level.id))]
}

/// The living world for one level.
pub struct Life {
    level: &'static str,
}

impl Life {
    pub fn new(level: &'static str) -> Life {
        Life { level }
    }
}

impl Scenery for Life {
    fn name(&self) -> &str {
        "Life"
    }

    fn label(&self) -> Option<&str> {
        Some("Bringing the world to life")
    }

    fn build(&mut self, w: &mut World) -> Result<(), String> {
        // The menu's flyover sections build only the level's own scenery.
        if w.section.is_some() {
            return Ok(());
        }
        let birds = birds::build(w, self.level)?;
        let people = people::build(w, self.level)?;
        if birds.is_none() && people.is_none() {
            return Ok(());
        }
        w.animators.push(Box::new(Animate {
            time: 0.0,
            last_s: None,
            birds,
            people,
        }));
        Ok(())
    }
}

/// One animator for the level's birds and people.
struct Animate {
    time: f64,
    /// The player's `s` last frame (people react to the car going by).
    last_s: Option<f64>,
    birds: Option<birds::Birds>,
    people: Option<people::People>,
}

impl Animator for Animate {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        self.time += u.dt;
        let cam = u.camera.map(|c| c.position);
        if let Some(b) = &mut self.birds {
            b.update(self.time, u, cam, out);
        }
        if let Some(p) = &mut self.people {
            p.update(self.time, u, cam, self.last_s, out);
        }
        self.last_s = Some(u.s);
    }
}

// ── Shared helpers ──────────────────────────────────────────────────

pub(crate) fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

/// A rotation of `angle` about an axis through the origin.
pub(crate) fn rot(axis: Vector3, angle: f64) -> Quaternion {
    Quaternion::from_axis_angle(axis, angle)
}

pub(crate) const X: Vector3 = Vector3::new(1.0, 0.0, 0.0);
pub(crate) const Y: Vector3 = Vector3::new(0.0, 1.0, 0.0);
pub(crate) const Z: Vector3 = Vector3::new(0.0, 0.0, 1.0);

/// Heading from a direction in the ground plane: the yaw that turns the
/// models' +z towards (dx, dz).
pub(crate) fn heading(dx: f64, dz: f64) -> f64 {
    kernel::atan2(dx, dz)
}

pub(crate) fn instance_matrix(node: crate::object::NodeId, index: usize, m: &Matrix4) -> Edit {
    Edit {
        target: Handle::Node(node),
        change: Change::InstanceMatrix {
            index: index as u32,
            matrix: m.elements.map(|v| v as f32),
        },
    }
}

pub(crate) fn instance_count(node: crate::object::NodeId, n: usize) -> Edit {
    Edit {
        target: Handle::Node(node),
        change: Change::InstanceCount(n as u32),
    }
}

/// Distance in the ground plane.
pub(crate) fn dist2d(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (dx, dz) = (a[0] - b[0], a[2] - b[2]);
    (dx * dx + dz * dz).sqrt()
}

/// Ground height at (x, z): the terrain, else the road.
pub(crate) fn ground(w: &World, x: f64, z: f64, fallback: f64) -> f64 {
    w.terrain.as_ref().map_or(fallback, |t| t.height_at(x, z))
}
