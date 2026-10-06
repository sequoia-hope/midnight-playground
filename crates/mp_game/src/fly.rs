//! The debug fly camera and the menu's attract camera (`src/main.js`
//! `flyCamera` and the attract branch of `tick`), on the Rust `Track`.

use crate::options::FlyParams;
use bevy::math::{DVec3, Quat, Vec3};
use bevy::prelude::Transform;
use mp_track::Track;

/// The attract camera's drift (`attract.s += dt * 16`).
pub const ATTRACT_SPEED: f64 = 16.0;

/// The camera's pose for these parameters, and the focus point the world
/// updates around. Advances `f.s` by `f.speed * dt` first, as `flyCamera`.
pub fn fly_camera(track: &Track, dt: f64, f: &mut FlyParams) -> (Transform, DVec3) {
    f.s = if track.is_loop {
        track.wrap(f.s + f.speed * dt)
    } else {
        (track.road_end() - 1.0).min(f.s + f.speed * dt)
    };
    let a = track.frame(if track.is_loop {
        f.s - f.back
    } else {
        0f64.max(f.s - f.back)
    });
    let b = track.frame(f.s + 20.0);
    let eye = DVec3::new(a.x + a.rx * f.lat, a.y + f.h, a.z + a.rz * f.lat);
    let target = DVec3::new(b.x, b.y + f.h * 0.4, b.z);
    (pose(eye, target, f.yaw, f.pitch), DVec3::new(a.x, a.y, a.z))
}

/// `camera.position.copy(eye); camera.lookAt(target); camera.rotateY(yaw);
/// camera.rotateX(pitch)`. three's camera `lookAt` and Bevy's `looking_at`
/// both point -Z at the target with +Y up; the rotations are local.
pub fn pose(eye: DVec3, target: DVec3, yaw: f64, pitch: f64) -> Transform {
    let e = eye.as_vec3();
    let mut t = Transform::from_translation(e).looking_at(target.as_vec3(), Vec3::Y);
    t.rotation =
        t.rotation * Quat::from_rotation_y(yaw as f32) * Quat::from_rotation_x(pitch as f32);
    t
}

/// The attract camera (`main.js` `tick`, behind the menu): drifts along the
/// first zone at 16 m/s and starts over from `startS + 60` at its end.
pub struct Attract {
    pub s: f64,
}

impl Default for Attract {
    fn default() -> Self {
        Attract {
            s: FlyParams::ATTRACT.s,
        }
    }
}

impl Attract {
    pub fn step(&mut self, track: &Track, dt: f64) -> (Transform, DVec3) {
        self.s += dt * ATTRACT_SPEED;
        let end = if track.is_loop {
            track.length
        } else {
            track.zones[0].s1 - 200.0
        };
        if self.s > end {
            self.s = track.start_s + 60.0;
        }
        let mut p = FlyParams {
            s: self.s,
            ..FlyParams::ATTRACT
        };
        let (pose, _) = fly_camera(track, dt, &mut p);
        let f = track.frame(self.s);
        (pose, DVec3::new(f.x, f.y, f.z))
    }
}
