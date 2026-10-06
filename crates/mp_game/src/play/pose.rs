//! Where a car is drawn (port of `Vehicle.sync`, `src/vehicles/Vehicle.js`,
//! SPEC 6.5): the pose interpolated between the last two ticks, the body's
//! orientation from the road frame (grade along, bank across, the nose
//! pitched along the flight path in the air), and the suspension feel, the
//! body's pitch and roll springs driven by the accelerations, which are
//! presentation and live in the client (SPEC 4.3; physics' landing kick
//! arrives as `PhysEvent::Touchdown`).

use bevy::math::{DMat3, DQuat, DVec3};
use mp_math::{clamp, kernel, wrap_angle};
use mp_sim::vehicle::Vehicle;
use mp_track::Track;

/// What `sync` reads of a vehicle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f64,
    /// `visY ?? y`: where the body is drawn.
    pub y: f64,
    pub z: f64,
    /// `yaw + visualYaw`.
    pub yaw: f64,
    pub s: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub on_ground: bool,
    pub speed: f64,
    pub steer_angle: f64,
}

impl Pose {
    pub fn of(v: &Vehicle) -> Pose {
        Pose {
            x: v.x,
            y: v.vis_y.unwrap_or(v.y),
            z: v.z,
            yaw: v.yaw + v.visual_yaw,
            s: v.s,
            vx: v.vx,
            vy: v.vy,
            vz: v.vz,
            on_ground: v.on_ground,
            speed: v.speed,
            steer_angle: v.steer_angle,
        }
    }

    /// The pose a fraction `t` of the way from tick `a` to tick `b`: `a` at
    /// 0 and `b` at 1 exactly, the heading the short way round, s along the
    /// loop.
    pub fn lerp(track: &Track, a: &Pose, b: &Pose, t: f64) -> Pose {
        if t <= 0.0 {
            return *a;
        }
        if t >= 1.0 {
            return *b;
        }
        let l = |p: f64, q: f64| p + (q - p) * t;
        Pose {
            x: l(a.x, b.x),
            y: l(a.y, b.y),
            z: l(a.z, b.z),
            yaw: a.yaw + wrap_angle(b.yaw - a.yaw) * t,
            s: if track.is_loop {
                track.wrap(a.s + track.ds(a.s, b.s) * t)
            } else {
                l(a.s, b.s)
            },
            vx: l(a.vx, b.vx),
            vy: l(a.vy, b.vy),
            vz: l(a.vz, b.vz),
            on_ground: if t < 0.5 { a.on_ground } else { b.on_ground },
            speed: l(a.speed, b.speed),
            steer_angle: l(a.steer_angle, b.steer_angle),
        }
    }
}

/// three's `Vector3.normalize`: a zero vector stays zero.
fn norm(v: DVec3) -> DVec3 {
    let l = v.length();
    if l == 0.0 { v } else { v / l }
}

/// `Vehicle.sync`'s placement of the model's root: its position and the
/// rotation whose basis is the road-aligned (X, Y, Z), +Z the nose.
pub fn root(track: &Track, p: &Pose) -> (DVec3, DQuat) {
    let f = track.frame(p.s);
    // Surface normal from the road frame (grade along, bank across).
    let t = norm(DVec3::new(f.fx, f.grade, f.fz));
    let a = norm(DVec3::new(f.rx, -f.bank, f.rz));
    let mut y = norm(a.cross(t));
    let mut z = DVec3::new(kernel::cos(p.yaw), 0.0, kernel::sin(p.yaw));
    // In the air, pitch the nose along the flight path a little.
    if !p.on_ground {
        let sp = kernel::hypot(p.vx, p.vz) + 0.01;
        let tilt = clamp(p.vy / sp, -0.35, 0.35) * 0.6;
        z.y += tilt;
    }
    z = norm(z - y * (z.dot(y) * if p.on_ground { 1.0 } else { 0.4 }));
    let x = norm(y.cross(z));
    y = norm(z.cross(x));
    let q = DQuat::from_mat3(&DMat3::from_cols(x, y, z));
    (DVec3::new(p.x, p.y, p.z), q)
}

/// Suspension feel: body pitch and roll springs driven by accelerations.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Springs {
    pub pitch: f64,
    pub roll: f64,
    pub pitch_v: f64,
    pub roll_v: f64,
}

impl Springs {
    /// One rendered frame of `sync`'s springs.
    pub fn step(&mut self, accel_long: f64, accel_lat: f64, dt: f64) {
        let tp = clamp(-accel_long * 0.0045, -0.055, 0.055);
        let tr = clamp(-accel_lat * 0.0042, -0.065, 0.065);
        let (k, c) = (90.0, 11.0);
        self.pitch_v += ((tp - self.pitch) * k - self.pitch_v * c) * dt;
        self.roll_v += ((tr - self.roll) * k - self.roll_v * c) * dt;
        self.pitch += self.pitch_v * dt;
        self.roll += self.roll_v * dt;
    }

    /// Physics' landing kick (`v.pitchV -= impact * 0.02`).
    pub fn touchdown(&mut self, impact: f64) {
        self.pitch_v -= impact * 0.02;
    }

    /// `body.rotation.x = pitch; body.rotation.z = roll` (three's XYZ order).
    pub fn rotation(&self) -> DQuat {
        DQuat::from_euler(bevy::math::EulerRot::XYZ, self.pitch, 0.0, self.roll)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_sim::autopilot::autopilot;
    use mp_sim::input::{Input, InputFrame};
    use mp_sim::race::{LevelRuntime, RaceOpts, SimState, step};

    /// The drawn pose at alpha 0 is the previous tick's and at alpha 1 the
    /// current tick's, bit for bit, through `sync`'s placement too; in
    /// between it moves monotonically from one to the other.
    #[test]
    fn interpolation_ends_on_the_ticks() {
        let lr = LevelRuntime::new(mp_levels::level_by_id("sierra")).unwrap();
        let mut st = SimState::new(
            &lr,
            RaceOpts {
                car: "sports",
                seed: 1,
                pursuit: false,
                heat: 1.0,
            },
        );
        let mut ev = Vec::new();
        let t = &*lr.track;
        for tick in 0..1500 {
            let prev = st.clone();
            let mut inp = Input::default();
            autopilot(&mut inp, &st.players[0].v, t);
            step(&lr, &mut st, &[InputFrame::quantise(&inp)], &mut ev);
            if tick % 50 != 0 {
                continue;
            }
            let bodies = prev
                .rivals
                .iter()
                .map(|r| &r.k.v)
                .zip(st.rivals.iter().map(|r| &r.k.v))
                .chain(std::iter::once((&prev.players[0].v, &st.players[0].v)));
            for (a, b) in bodies {
                let (pa, pb) = (Pose::of(a), Pose::of(b));
                assert_eq!(Pose::lerp(t, &pa, &pb, 0.0), pa);
                assert_eq!(Pose::lerp(t, &pa, &pb, 1.0), pb);
                assert_eq!(root(t, &Pose::lerp(t, &pa, &pb, 0.0)), root(t, &pa));
                assert_eq!(root(t, &Pose::lerp(t, &pa, &pb, 1.0)), root(t, &pb));
                let mid = Pose::lerp(t, &pa, &pb, 0.5);
                let between = |m: f64, p: f64, q: f64| m >= p.min(q) && m <= p.max(q);
                assert!(between(mid.x, pa.x, pb.x) && between(mid.z, pa.z, pb.z));
            }
        }
        assert!(st.race.time > 5.0, "the field is moving");
    }

    /// On a level road the body's nose is along the heading and its up is
    /// up; the heading convention is forward = (cos yaw, sin yaw).
    #[test]
    fn root_follows_the_heading() {
        let lr = LevelRuntime::new(mp_levels::level_by_id("sierra")).unwrap();
        let t = &*lr.track;
        let f = t.frame(t.start_s);
        let yaw = kernel::atan2(f.fz, f.fx);
        let p = Pose {
            x: f.x,
            y: f.y,
            z: f.z,
            yaw,
            s: t.start_s,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            on_ground: true,
            speed: 0.0,
            steer_angle: 0.0,
        };
        let (pos, q) = root(t, &p);
        assert_eq!(pos, DVec3::new(f.x, f.y, f.z));
        let nose = q * DVec3::Z;
        assert!((nose.x - f.fx).abs() < 0.05 && (nose.z - f.fz).abs() < 0.05);
        assert!((q * DVec3::Y).y > 0.95);
    }

    #[test]
    fn springs_settle_and_kick() {
        let mut s = Springs::default();
        for _ in 0..600 {
            s.step(-10.0, 0.0, 1.0 / 120.0);
        }
        assert!(
            (s.pitch - 0.045).abs() < 1e-3,
            "braking dips the nose: {}",
            s.pitch
        );
        s.touchdown(5.0);
        assert!(s.pitch_v < -0.09);
    }
}
