//! The viewer's cameras as plain math, with no engine state (SPEC 8.6): a
//! pose, the free fly, orbit and overview cameras, the ride's live
//! adjustments, picking a point on the ground, and the flights between
//! views. The client puts the pose on its camera (`super::drive`).
//!
//! A pose is a position and two angles: `yaw` about the world's up axis
//! (0 looks along −Z, positive turns left, as Bevy's rotations do) and
//! `pitch` about the camera's own x axis (positive looks up). The camera's
//! rotation is `Ry(yaw) · Rx(pitch)`: never any roll.

use crate::options::FlyParams;
use bevy::math::{DVec3, Quat, Vec3};
use bevy::prelude::Transform;
use std::f64::consts::{FRAC_PI_2, PI};

/// The game's vertical field of view (`main.js:60`), which the viewer keeps.
pub const FOV_Y_DEG: f64 = 62.0;

/// The steepest a free or orbiting camera looks up or down (just short of
/// straight, where yaw would lose its meaning).
pub const PITCH_LIMIT: f64 = 1.55;

/// Free fly speeds (m/s): from walking pace to several hundred; Shift (or
/// the right bumper held) is four times faster.
pub const SPEED_MIN: f64 = 1.0;
pub const SPEED_MAX: f64 = 800.0;
pub const SPEED_DEFAULT: f64 = 25.0;
pub const BOOST: f64 = 4.0;

/// Orbit distances (m).
pub const ORBIT_MIN: f64 = 2.0;
pub const ORBIT_MAX: f64 = 30_000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: DVec3,
    pub yaw: f64,
    pub pitch: f64,
}

impl Pose {
    pub fn new(pos: DVec3, yaw: f64, pitch: f64) -> Pose {
        Pose { pos, yaw, pitch }
    }

    /// Where the camera looks (unit): `Ry(yaw) · Rx(pitch) · (0, 0, −1)`.
    pub fn forward(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        DVec3::new(-sy * cp, sp, -cy * cp)
    }

    /// The camera's right (level, unit).
    pub fn right(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        DVec3::new(cy, 0.0, -sy)
    }

    /// Forward along the ground (level, unit).
    pub fn ahead(&self) -> DVec3 {
        let (sy, cy) = self.yaw.sin_cos();
        DVec3::new(-sy, 0.0, -cy)
    }

    /// The camera's up (unit), for a pose looking straight down: screen up.
    pub fn up(&self) -> DVec3 {
        self.right().cross(self.forward()).normalize_or_zero()
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw as f32) * Quat::from_rotation_x(self.pitch as f32)
    }

    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.pos.as_vec3()).with_rotation(self.rotation())
    }

    /// The pose of a camera transform (its roll, if any, dropped).
    pub fn from_transform(t: &Transform) -> Pose {
        let f = (t.rotation * Vec3::NEG_Z).as_dvec3();
        let yaw = if f.x.abs() < 1e-4 && f.z.abs() < 1e-4 {
            // Straight up or down: the yaw is where the camera's up points.
            let u = (t.rotation * Vec3::Y).as_dvec3();
            let u = if f.y < 0.0 { u } else { -u };
            (-u.x).atan2(-u.z)
        } else {
            (-f.x).atan2(-f.z)
        };
        Pose {
            pos: t.translation.as_dvec3(),
            yaw,
            pitch: f.y.clamp(-1.0, 1.0).asin(),
        }
    }

    /// From `eye`, looking at `target`.
    pub fn looking(eye: DVec3, target: DVec3) -> Pose {
        let d = (target - eye).normalize_or_zero();
        let flat = (d.x * d.x + d.z * d.z).sqrt();
        Pose {
            pos: eye,
            yaw: if flat < 1e-9 { 0.0 } else { (-d.x).atan2(-d.z) },
            pitch: d.y.atan2(flat),
        }
    }
}

/// `a` to `b` the short way round, in radians.
pub fn angle_diff(a: f64, b: f64) -> f64 {
    let d = (b - a) % (2.0 * PI);
    if d > PI {
        d - 2.0 * PI
    } else if d < -PI {
        d + 2.0 * PI
    } else {
        d
    }
}

/// An angle in (−π, π].
pub fn wrap_angle(a: f64) -> f64 {
    angle_diff(0.0, a)
}

/// What the controls ask of a camera this frame. Movement is −1 to 1 per
/// axis (a key held, a stick); `look` is radians already (a drag scaled by
/// the field of view, a stick by its rate); `zoom` is a factor's logarithm
/// (positive brings the camera closer); `pan` is in screen pixels (a drag
/// on the overview, two fingers).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drive {
    pub fwd: f64,
    pub right: f64,
    pub up: f64,
    pub look: (f64, f64),
    pub zoom: f64,
    pub pan: (f64, f64),
    /// Speed multiplier (Shift, a bumper held).
    pub boost: f64,
}

/// The free fly camera: moves where it looks, strafes level, rises and
/// sinks straight; its velocity eases toward what the controls ask, so a
/// key tap does not jerk the view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Free {
    pub pose: Pose,
    /// m/s at full stick.
    pub speed: f64,
    pub vel: DVec3,
}

impl Free {
    pub fn new(pose: Pose) -> Free {
        Free {
            pose,
            speed: SPEED_DEFAULT,
            vel: DVec3::ZERO,
        }
    }

    pub fn step(&mut self, dt: f64, d: &Drive) {
        self.pose.yaw = wrap_angle(self.pose.yaw + d.look.0);
        self.pose.pitch = (self.pose.pitch + d.look.1).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        // Pinch (zoom) moves along the view as a push would.
        let fwd = d.fwd + d.zoom * 8.0;
        let mut dir = self.pose.forward() * fwd + self.pose.right() * d.right + DVec3::Y * d.up;
        if dir.length() > 1.0 {
            dir = dir.normalize();
        }
        let want = dir * self.speed * if d.boost > 0.0 { d.boost } else { 1.0 };
        let k = 1.0 - (-dt * 10.0).exp();
        self.vel += (want - self.vel) * k;
        if self.vel.length() < 1e-4 && want == DVec3::ZERO {
            self.vel = DVec3::ZERO;
        }
        self.pose.pos += self.vel * dt;
    }

    /// The speed nudged by `steps` (a wheel notch or a bumper press each):
    /// a quarter more or less per step, within the limits.
    pub fn nudge_speed(&mut self, steps: f64) {
        self.speed = (self.speed * 1.25f64.powf(steps)).clamp(SPEED_MIN, SPEED_MAX);
    }
}

/// The orbit camera: circles, tilts and zooms about a centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orbit {
    pub centre: DVec3,
    pub dist: f64,
    pub yaw: f64,
    pub pitch: f64,
}

impl Orbit {
    pub fn pose(&self) -> Pose {
        let mut p = Pose::new(DVec3::ZERO, self.yaw, self.pitch);
        p.pos = self.centre - p.forward() * self.dist;
        p
    }

    /// The orbit that puts the camera at `pose`, about the point `dist`
    /// ahead of it.
    pub fn from_pose(pose: &Pose, dist: f64) -> Orbit {
        Orbit {
            centre: pose.pos + pose.forward() * dist,
            dist,
            yaw: pose.yaw,
            pitch: pose.pitch,
        }
    }

    /// The orbit about `centre` whose camera is at `eye`.
    pub fn about(centre: DVec3, eye: DVec3) -> Orbit {
        let p = Pose::looking(eye, centre);
        Orbit {
            centre,
            dist: (centre - eye).length().max(ORBIT_MIN),
            yaw: p.yaw,
            pitch: p.pitch,
        }
    }

    pub fn step(&mut self, dt: f64, d: &Drive) {
        self.yaw = wrap_angle(self.yaw + d.look.0);
        self.pitch = (self.pitch + d.look.1).clamp(-PITCH_LIMIT, 0.6);
        self.dist = (self.dist * (-d.zoom).exp()).clamp(ORBIT_MIN, ORBIT_MAX);
        // The centre moves with the keys or the left stick, faster the
        // farther out the camera is.
        let p = Pose::new(DVec3::ZERO, self.yaw, 0.0);
        let rate = self.dist.max(5.0) * 0.9 * if d.boost > 0.0 { d.boost } else { 1.0 };
        self.centre +=
            (p.ahead() * d.fwd + p.right() * d.right + DVec3::Y * d.up * 0.6) * rate * dt;
    }
}

/// The overview: straight down over a point of the ground, from a height,
/// the map turned by `yaw` (0: north, −Z, up the screen).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overview {
    /// The point below, at the level's ground height.
    pub centre: DVec3,
    /// Above `centre.y`.
    pub height: f64,
    pub yaw: f64,
}

impl Overview {
    pub fn pose(&self) -> Pose {
        Pose::new(self.centre + DVec3::Y * self.height, self.yaw, -FRAC_PI_2)
    }

    pub fn from_pose(pose: &Pose, ground: f64) -> Overview {
        Overview {
            centre: DVec3::new(pose.pos.x, ground, pose.pos.z),
            height: (pose.pos.y - ground).max(10.0),
            yaw: pose.yaw,
        }
    }

    /// Ground metres per screen pixel at the centre.
    pub fn metres_per_px(&self, viewport_h: f64) -> f64 {
        2.0 * self.height * (FOV_Y_DEG.to_radians() / 2.0).tan() / viewport_h.max(1.0)
    }

    /// `pan` drags the ground with the finger; `zoom` lowers the camera;
    /// the stick or keys move over the map.
    pub fn step(&mut self, dt: f64, d: &Drive, viewport_h: f64, max_height: f64) {
        let p = self.pose();
        let k = self.metres_per_px(viewport_h);
        self.centre -= p.right() * d.pan.0 * k;
        self.centre += p.up() * d.pan.1 * k;
        let rate = self.height * 0.8 * if d.boost > 0.0 { d.boost } else { 1.0 };
        self.centre += (p.up() * d.fwd + p.right() * d.right) * rate * dt;
        self.yaw = wrap_angle(self.yaw + d.look.0);
        let zoom = d.zoom + d.up * dt * 1.5;
        self.height = (self.height * (-zoom).exp()).clamp(30.0, max_height.max(60.0));
    }
}

/// The height that shows a box of `w` × `d` metres (x by z) whole, at a
/// screen aspect (width over height), with a margin.
pub fn fit_height(w: f64, d: f64, aspect: f64) -> f64 {
    let t = (FOV_Y_DEG.to_radians() / 2.0).tan();
    let half = (d / 2.0).max(w / 2.0 / aspect.max(0.1));
    (half / t * 1.12).max(100.0)
}

/// The ride: the game's fly camera (`fly::fly_camera`), its speed, height,
/// side offset and look adjusted live.
pub fn ride_step(f: &mut FlyParams, dt: f64, d: &Drive) {
    f.speed = (f.speed + d.fwd * 60.0 * dt).clamp(-300.0, 600.0);
    f.h = (f.h * (d.up * dt * 1.2).exp()).clamp(0.3, 2_000.0);
    f.lat = (f.lat + d.right * 12.0 * dt).clamp(-200.0, 200.0);
    f.yaw = wrap_angle(f.yaw + d.look.0);
    f.pitch = (f.pitch + d.look.1).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    f.back = (f.back * (-d.zoom).exp()).clamp(0.0, 2_000.0);
}

/// The ray through a screen point (CSS or logical px, the same units as
/// `w` and `h`), as a unit direction from the camera.
pub fn ray(pose: &Pose, w: f64, h: f64, px: f64, py: f64) -> DVec3 {
    let t = (FOV_Y_DEG.to_radians() / 2.0).tan();
    let aspect = w / h.max(1.0);
    let x = (2.0 * px / w.max(1.0) - 1.0) * t * aspect;
    let y = (1.0 - 2.0 * py / h.max(1.0)) * t;
    let dir = pose.right() * x + pose.up() * y + pose.forward();
    dir.normalize_or_zero()
}

/// Where a point is on the screen (the units of `w` and `h`), if it is
/// in front of the camera.
pub fn project(pose: &Pose, w: f64, h: f64, p: DVec3) -> Option<(f64, f64)> {
    let d = p - pose.pos;
    let z = d.dot(pose.forward());
    if z <= 1e-6 {
        return None;
    }
    let t = (FOV_Y_DEG.to_radians() / 2.0).tan();
    let aspect = w / h.max(1.0);
    let x = d.dot(pose.right()) / (z * t * aspect);
    let y = d.dot(pose.up()) / (z * t);
    Some(((x + 1.0) / 2.0 * w, (1.0 - y) / 2.0 * h))
}

/// Where a ray meets the level plane at height `y`, if it does ahead.
pub fn hit_plane(from: DVec3, dir: DVec3, y: f64) -> Option<DVec3> {
    if dir.y.abs() < 1e-9 {
        return None;
    }
    let t = (y - from.y) / dir.y;
    (t > 0.0).then(|| from + dir * t)
}

/// A flight from one pose to another over `dur` seconds, eased at both
/// ends; the yaw turns the short way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flight {
    pub from: Pose,
    pub to: Pose,
    pub t: f64,
    pub dur: f64,
}

impl Flight {
    pub fn new(from: Pose, to: Pose, dur: f64) -> Flight {
        Flight {
            from,
            to,
            t: 0.0,
            dur: dur.max(1e-3),
        }
    }

    /// The pose after `dt` more seconds, and whether it has arrived.
    pub fn step(&mut self, dt: f64) -> (Pose, bool) {
        self.t = (self.t + dt).min(self.dur);
        let u = self.t / self.dur;
        let e = u * u * (3.0 - 2.0 * u);
        let p = Pose {
            pos: self.from.pos.lerp(self.to.pos, e),
            yaw: wrap_angle(self.from.yaw + angle_diff(self.from.yaw, self.to.yaw) * e),
            pitch: self.from.pitch + (self.to.pitch - self.from.pitch) * e,
        };
        (p, self.t >= self.dur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn a_pose_round_trips_through_its_transform() {
        for (yaw, pitch) in [(0.0, 0.0), (1.2, -0.4), (-2.9, 1.1), (3.0, -1.5)] {
            let p = Pose::new(DVec3::new(10.0, 20.0, -30.0), yaw, pitch);
            let q = Pose::from_transform(&p.transform());
            assert!(close(q.yaw, yaw) && close(q.pitch, pitch), "{p:?} → {q:?}");
            let f = (p.rotation() * Vec3::NEG_Z).as_dvec3();
            assert!((f - p.forward()).length() < 1e-6);
        }
        // Straight down keeps its yaw (the overview's map turn).
        let p = Pose::new(DVec3::ZERO, 0.7, -FRAC_PI_2);
        assert!(close(Pose::from_transform(&p.transform()).yaw, 0.7));
    }

    #[test]
    fn looking_points_the_camera_at_the_target() {
        let eye = DVec3::new(5.0, 10.0, 5.0);
        let target = DVec3::new(-20.0, 0.0, 40.0);
        let p = Pose::looking(eye, target);
        assert!((p.forward() - (target - eye).normalize()).length() < 1e-9);
    }

    #[test]
    fn orbit_and_pose_agree() {
        let o = Orbit {
            centre: DVec3::new(100.0, 5.0, -50.0),
            dist: 80.0,
            yaw: 0.6,
            pitch: -0.5,
        };
        let p = o.pose();
        assert!(((p.pos - o.centre).length() - 80.0).abs() < 1e-9);
        let back = Orbit::from_pose(&p, 80.0);
        assert!((back.centre - o.centre).length() < 1e-9);
        let a = Orbit::about(o.centre, p.pos);
        assert!(close(a.yaw, o.yaw) && close(a.pitch, o.pitch) && close(a.dist, 80.0));
    }

    #[test]
    fn free_fly_moves_where_it_looks() {
        let mut f = Free::new(Pose::new(DVec3::ZERO, FRAC_PI_2, 0.0));
        f.speed = 10.0;
        let d = Drive {
            fwd: 1.0,
            ..Default::default()
        };
        for _ in 0..600 {
            f.step(1.0 / 60.0, &d);
        }
        // Yaw +90° looks along −X; ten seconds at 10 m/s, less the ease-in.
        assert!(
            f.pose.pos.x < -95.0 && f.pose.pos.x > -100.0,
            "{:?}",
            f.pose.pos
        );
        assert!(f.pose.pos.z.abs() < 1e-6);
        f.nudge_speed(100.0);
        assert_eq!(f.speed, SPEED_MAX);
    }

    #[test]
    fn the_overview_drags_the_ground_with_the_finger() {
        let mut o = Overview {
            centre: DVec3::ZERO,
            height: 1000.0,
            yaw: 0.0,
        };
        let k = o.metres_per_px(800.0);
        o.step(
            0.0,
            &Drive {
                pan: (100.0, 0.0),
                ..Default::default()
            },
            800.0,
            1e5,
        );
        // Finger right: the camera goes left (−X at yaw 0).
        assert!(close(o.centre.x, -100.0 * k));
        // The centre pixel looks straight down at the centre.
        let p = o.pose();
        let r = ray(&p, 1280.0, 800.0, 640.0, 400.0);
        let hit = hit_plane(p.pos, r, 0.0).unwrap();
        assert!((hit - o.centre).length() < 1e-6);
        // The top of the screen is north (−Z) at yaw 0.
        let top = hit_plane(p.pos, ray(&p, 1280.0, 800.0, 640.0, 0.0), 0.0).unwrap();
        assert!(top.z < o.centre.z - 100.0 && (top.x - o.centre.x).abs() < 1e-6);
        // And a point projects back to where it was picked.
        let corner = hit_plane(p.pos, ray(&p, 1280.0, 800.0, 100.0, 700.0), 0.0).unwrap();
        let (x, y) = project(&p, 1280.0, 800.0, corner).unwrap();
        assert!((x - 100.0).abs() < 1e-6 && (y - 700.0).abs() < 1e-6);
    }

    #[test]
    fn fit_height_shows_the_box() {
        let h = fit_height(10_000.0, 4_000.0, 1.6);
        let t = (FOV_Y_DEG.to_radians() / 2.0).tan();
        assert!(h * t * 1.6 >= 5_000.0);
        assert!(h * t >= 2_000.0);
    }

    #[test]
    fn a_flight_arrives_the_short_way_round() {
        let a = Pose::new(DVec3::ZERO, 3.0, 0.0);
        let b = Pose::new(DVec3::new(10.0, 0.0, 0.0), -3.0, -0.5);
        let mut f = Flight::new(a, b, 1.0);
        let (mid, done) = f.step(0.5);
        assert!(!done);
        assert!(
            mid.yaw.abs() > 3.0,
            "through ±π, not through 0: {}",
            mid.yaw
        );
        let (end, done) = f.step(0.6);
        assert!(done && end.pos == b.pos && close(end.yaw, b.yaw) && end.pitch == b.pitch);
    }
}
