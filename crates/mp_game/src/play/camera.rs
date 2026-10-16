//! The chase camera (port of `src/game/CameraRig.js`, SPEC 6.5, roadmap WP
//! 4.3) and Race's intro swing (`Race.introCamera`). It trails the car along
//! a blend of its heading and its actual direction of travel (so drifts read
//! as drifts), widens the field of view with speed and nitro, shakes on
//! impacts, and never dips below the road. It runs per rendered frame on the
//! interpolated car.

use bevy::math::DVec3;
use mp_sim::vehicle::Vehicle;
use mp_track::Track;
use pk_math::{clamp, damp, kernel, lerp, smoothstep};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    pub name: &'static str,
    pub back: f64,
    pub up: f64,
    pub look: f64,
    pub ahead: f64,
    pub fov: f64,
}

pub const MODES: [Mode; 3] = [
    Mode {
        name: "chase",
        back: 6.4,
        up: 2.05,
        look: 1.05,
        ahead: 3.5,
        fov: 60.0,
    },
    Mode {
        name: "far",
        back: 9.5,
        up: 3.1,
        look: 1.2,
        ahead: 4.0,
        fov: 58.0,
    },
    Mode {
        name: "bumper",
        back: -1.2,
        up: 0.72,
        look: 0.7,
        ahead: 20.0,
        fov: 72.0,
    },
];

/// What the camera reads of the car (the physics `y`, not `visY`, and the
/// heading without the visual spin, as the JS reads `v.y` and `v.yaw`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Car {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    pub vx: f64,
    pub vy: f64,
    pub vz: f64,
    pub s: f64,
    pub lat: f64,
    /// The road of the level's network under the car and the car's place
    /// on it (`Vehicle::road`, `road_s`, `road_lat`): on a branch `s` and
    /// `lat` are the car's place on the level's road, hundreds of metres
    /// off, which is no floor for the camera (D1211).
    pub road: usize,
    pub road_s: f64,
    pub road_lat: f64,
}

impl Car {
    pub fn of(v: &Vehicle) -> Car {
        Car {
            x: v.x,
            y: v.y,
            z: v.z,
            yaw: v.yaw,
            vx: v.vx,
            vy: v.vy,
            vz: v.vz,
            s: v.s,
            lat: v.lat,
            road: v.road,
            road_s: v.road_s,
            road_lat: v.road_lat,
        }
    }

    /// `a` at 0, `b` at 1 exactly (see `pose::Pose::lerp`).
    pub fn lerp(track: &Track, a: &Car, b: &Car, t: f64) -> Car {
        if t <= 0.0 {
            return *a;
        }
        if t >= 1.0 {
            return *b;
        }
        let l = |p: f64, q: f64| p + (q - p) * t;
        Car {
            x: l(a.x, b.x),
            y: l(a.y, b.y),
            z: l(a.z, b.z),
            yaw: a.yaw + pk_math::wrap_angle(b.yaw - a.yaw) * t,
            vx: l(a.vx, b.vx),
            vy: l(a.vy, b.vy),
            vz: l(a.vz, b.vz),
            s: if track.is_loop {
                track.wrap(a.s + track.ds(a.s, b.s) * t)
            } else {
                l(a.s, b.s)
            },
            lat: l(a.lat, b.lat),
            // Across a junction the car changes road between ticks: the
            // new road's place, whole.
            road: b.road,
            road_s: if a.road == b.road {
                l(a.road_s, b.road_s)
            } else {
                b.road_s
            },
            road_lat: if a.road == b.road {
                l(a.road_lat, b.road_lat)
            } else {
                b.road_lat
            },
        }
    }
}

/// The camera this frame: eye, the point it looks at, the vertical field
/// of view in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub eye: DVec3,
    pub target: DVec3,
    pub fov: f64,
}

#[derive(Clone, Debug)]
pub struct CameraRig {
    pub mode: usize,
    pub pos: DVec3,
    pub look_at: DVec3,
    pub dir_x: f64,
    pub dir_z: f64,
    pub shake: f64,
    pub fov: f64,
    pub t: f64,
    pub snap: bool,
    /// Race's `introT`.
    pub intro_t: f64,
    /// The camera's field of view as last set (`cam.fov`; the JS camera is
    /// made with 62°).
    pub cam_fov: f64,
}

impl Default for CameraRig {
    fn default() -> Self {
        CameraRig {
            mode: 0,
            pos: DVec3::ZERO,
            look_at: DVec3::ZERO,
            dir_x: 1.0,
            dir_z: 0.0,
            shake: 0.0,
            fov: 60.0,
            t: 0.0,
            snap: true,
            intro_t: 0.0,
            cam_fov: 62.0,
        }
    }
}

impl CameraRig {
    pub fn cycle(&mut self) {
        self.mode = (self.mode + 1) % MODES.len();
        self.snap = true;
    }

    pub fn bump(&mut self, amount: f64) {
        self.shake = f64::min(1.2, self.shake + amount);
    }

    /// `update(dt, car, { lookBack, nitro })`; `aspect` is the screen's.
    /// Terrain: the JS also keeps the camera above `terrain.heightAt`; the
    /// drawn levels have no terrain heights on the client, so only the
    /// road counts there (DECISIONS D434). An open level's ground is in
    /// its Track ([`Track::ground_y`]), so there the floor is the land or
    /// road under the eye, on the road the car is on (D1211).
    pub fn update(
        &mut self,
        dt: f64,
        track: &Track,
        v: &Car,
        look_back: bool,
        nitro: bool,
        aspect: f64,
    ) -> View {
        let m = MODES[self.mode];
        let bumper = m.name == "bumper";
        self.t += dt;
        let fx = kernel::cos(v.yaw);
        let fz = kernel::sin(v.yaw);
        let sp = kernel::hypot(v.vx, v.vz);
        // Direction the camera trails along.
        let (mut tx, mut tz) = (fx, fz);
        if sp > 3.0 && !bumper {
            let vxn = v.vx / sp;
            let vzn = v.vz / sp;
            let w = 0.45 * smoothstep(3.0, 12.0, sp);
            tx = lerp(fx, vxn, w);
            tz = lerp(fz, vzn, w);
            if v.vx * fx + v.vz * fz < -1.0 {
                // reversing
                tx = fx;
                tz = fz;
            }
        }
        let l = or1(kernel::hypot(tx, tz));
        tx /= l;
        tz /= l;
        let k = if self.snap {
            1.0
        } else {
            1.0 - kernel::exp(-(if bumper { 30.0 } else { 7.0 }) * dt)
        };
        self.dir_x += (tx - self.dir_x) * k;
        self.dir_z += (tz - self.dir_z) * k;
        let dl = or1(kernel::hypot(self.dir_x, self.dir_z));
        let (mut dx, mut dz) = (self.dir_x / dl, self.dir_z / dl);
        if look_back {
            dx = -dx;
            dz = -dz;
        }

        let back = m.back
            + if bumper {
                0.0
            } else {
                clamp(sp * 0.018, 0.0, 1.3)
            };
        let mut desired = DVec3::new(
            v.x - dx * back,
            v.y + m.up + if bumper { 0.0 } else { sp * 0.004 },
            v.z - dz * back,
        );
        if bumper {
            desired = DVec3::new(v.x + fx * 1.2, v.y + m.up, v.z + fz * 1.2);
            if look_back {
                desired = DVec3::new(v.x - fx * 2.3, v.y + m.up + 0.3, v.z - fz * 2.3);
            }
        }

        if self.snap {
            self.pos = desired;
            self.snap = false;
        } else {
            let kp = if bumper {
                1.0
            } else {
                1.0 - kernel::exp(-14.0 * dt)
            };
            self.pos.x += (desired.x - self.pos.x) * kp;
            self.pos.z += (desired.z - self.pos.z) * kp;
            let ky = if bumper {
                1.0
            } else {
                1.0 - kernel::exp(-9.0 * dt)
            };
            self.pos.y += (desired.y - self.pos.y) * ky;
        }
        // Keep above the ground and the road.
        let g = floor(track, v, self.pos.x, self.pos.z) + if bumper { 0.3 } else { 0.9 };
        if self.pos.y < g {
            self.pos.y = g;
        }

        let ahead_dir = if look_back { -1.0 } else { 1.0 };
        self.look_at = DVec3::new(
            v.x + fx * m.ahead * ahead_dir,
            v.y + m.look,
            v.z + fz * m.ahead * ahead_dir,
        );
        if bumper {
            self.look_at = DVec3::new(v.x + dx * 30.0, v.y + 0.9 + v.vy * 0.3, v.z + dz * 30.0);
        }

        let mut eye = self.pos;
        // Shake.
        self.shake = damp(self.shake, 0.0, 4.0, dt);
        let rumble = smoothstep(40.0, 85.0, sp) * 0.035 + if nitro { 0.05 } else { 0.0 };
        let sh = self.shake * 0.35 + rumble;
        if sh > 0.001 {
            let t = self.t;
            eye.x += (kernel::sin(t * 53.0) + kernel::sin(t * 31.0)) * sh * 0.5;
            eye.y += (kernel::sin(t * 47.0) + kernel::sin(t * 23.0)) * sh * 0.35;
        }
        let mut fov_t = m.fov + smoothstep(10.0, 85.0, sp) * 16.0 + if nitro { 7.0 } else { 0.0 };
        // Narrow (portrait) screens: widen the vertical angle so the horizontal
        // view stays close to a landscape screen's, short of fisheye.
        if aspect < 1.2 {
            fov_t = f64::min(
                96.0,
                (2.0 * kernel::atan((kernel::tan((fov_t * PI) / 360.0) * 1.2) / aspect) * 180.0)
                    / PI,
            );
        }
        self.fov = damp(self.fov, fov_t, 3.0, dt);
        if (self.cam_fov - self.fov).abs() > 0.01 {
            self.cam_fov = self.fov;
        }
        View {
            eye,
            target: self.look_at,
            fov: self.cam_fov,
        }
    }

    /// `Race.introCamera`: during the countdown, swing from a front
    /// three-quarter view round to the chase position. Takes the view
    /// `update` gave and returns the one to draw.
    pub fn intro(&mut self, dt: f64, v: &Car, view: View) -> View {
        self.intro_t += dt;
        let k = smoothstep(0.0, 3.2, self.intro_t);
        let fx = kernel::cos(v.yaw);
        let fz = kernel::sin(v.yaw);
        let (rx, rz) = (-fz, fx);
        let ang = lerp(PI * 0.8, 0.0, k);
        let dist = lerp(7.5, 6.6, k);
        let bx = -fx * kernel::cos(ang) + rx * kernel::sin(ang);
        let bz = -fz * kernel::cos(ang) + rz * kernel::sin(ang);
        let pos = DVec3::new(v.x + bx * dist, v.y + lerp(1.3, 2.1, k), v.z + bz * dist);
        // camera.position.lerp(pos, 1 - k²)
        let eye = view.eye + (pos - view.eye) * (1.0 - k * k);
        self.pos = eye;
        View {
            eye,
            target: DVec3::new(v.x + fx * 1.5 * k, v.y + 0.9, v.z + fz * 1.5 * k),
            fov: view.fov,
        }
    }
}

/// The surface under (x, z) the camera must stay above: on a drawn level
/// the road's plane at the car's place (the JS; the camera is a few
/// metres behind the car, on the same stretch); on an open level the land
/// or road under the eye itself, projected onto the road the car is on,
/// since on a branch or off the road the level's road is far away and its
/// plane, banked, is nowhere near the ground (D1211).
fn floor(track: &Track, v: &Car, x: f64, z: f64) -> f64 {
    if track.open_ground.is_none() {
        return track.surface_y(v.s, v.lat);
    }
    let r = track.road(v.road.min(track.branches.len()));
    let p = r.project(x, z, v.road_s);
    r.ground_y(x, z, p.s, p.lat)
}

/// `x || 1`.
fn or1(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() { 1.0 } else { x }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_sim::race::{LevelRuntime, RaceOpts, SimState};

    fn grid() -> (LevelRuntime, Car) {
        let lr = LevelRuntime::new(mp_levels::level_by_id("sierra")).unwrap();
        let st = SimState::new(
            &lr,
            RaceOpts {
                car: "sports",
                seed: 1,
                pursuit: false,
                heat: 1.0,
                physics: mp_sim::model::Physics::Arcade,
            },
        );
        let car = Car::of(&st.players[0].v);
        (lr, car)
    }

    /// The chase view snaps behind and above the car, looking past it.
    #[test]
    fn chase_sits_behind_the_car() {
        let (lr, car) = grid();
        let mut rig = CameraRig::default();
        let v = rig.update(1.0 / 60.0, &lr.track, &car, false, false, 1.6);
        let (fx, fz) = (kernel::cos(car.yaw), kernel::sin(car.yaw));
        let to_eye = v.eye - DVec3::new(car.x, car.y, car.z);
        assert!(
            (to_eye.x * fx + to_eye.z * fz + 6.4).abs() < 1e-9,
            "6.4 m back"
        );
        assert!((to_eye.y - 2.05).abs() < 1e-9, "2.05 m up");
        assert!((v.target.y - car.y - 1.05).abs() < 1e-9);
        // The field of view eases from 60° toward its target, never jumps.
        assert!((v.fov - 60.0).abs() < 0.5);
    }

    /// C cycles chase → far → bumper → chase; the bumper sits at the nose.
    #[test]
    fn modes_cycle() {
        let (lr, car) = grid();
        let mut rig = CameraRig::default();
        rig.cycle();
        rig.cycle();
        let v = rig.update(1.0 / 60.0, &lr.track, &car, false, false, 1.6);
        let (fx, fz) = (kernel::cos(car.yaw), kernel::sin(car.yaw));
        assert!((v.eye.x - (car.x + fx * 1.2)).abs() < 1e-9);
        assert!((v.eye.z - (car.z + fz * 1.2)).abs() < 1e-9);
        rig.cycle();
        assert_eq!(rig.mode, 0);
    }

    /// On a branch the camera's floor is the branch under the eye, not the
    /// level's road: a car 1.5 km up Highway 84 is hundreds of metres off
    /// Highway 1, whose banked plane is no height to hold the camera at
    /// (the owner's "weird camera angle" on 84, D1211).
    #[test]
    fn the_floor_follows_the_road_under_the_car() {
        let lr = LevelRuntime::new(mp_levels::level_by_id("southcoast")).unwrap();
        let t = &lr.track;
        let b = t.road(1);
        let f = b.frame(1500.0);
        let mut v = Vehicle::new(
            mp_sim::dims::dims("sports").unwrap(),
            "sports",
            1200.0,
            "You",
            0xffffff,
        );
        v.x = f.x;
        v.z = f.z;
        v.y = f.y;
        v.yaw = kernel::atan2(f.fz, f.fx);
        v.road = 1;
        v.road_s = 1500.0;
        v.project_onto(t);
        assert_eq!(v.road, 1);
        let car = Car::of(&v);
        assert!(car.lat.abs() > 100.0, "off the level's road: {}", car.lat);
        let mut rig = CameraRig::default();
        let mut view = rig.update(1.0 / 60.0, t, &car, false, false, 1.6);
        for _ in 0..120 {
            view = rig.update(1.0 / 60.0, t, &car, false, false, 1.6);
        }
        let up = view.eye.y - car.y;
        assert!(
            (1.5..3.5).contains(&up),
            "the eye sits {up} m above the car"
        );
        // And off the road, on the land: the floor is the ground there.
        let (rx, rz) = (f.rx, f.rz);
        v.x = f.x + rx * 40.0;
        v.z = f.z + rz * 40.0;
        v.project_onto(t);
        v.y = t.road(v.road).ground_y(v.x, v.z, v.road_s, v.road_lat);
        let car = Car::of(&v);
        let mut rig = CameraRig::default();
        let mut view = rig.update(1.0 / 60.0, t, &car, false, false, 1.6);
        for _ in 0..120 {
            view = rig.update(1.0 / 60.0, t, &car, false, false, 1.6);
        }
        let up = view.eye.y - car.y;
        assert!(
            (1.0..6.0).contains(&up),
            "off the road the eye sits {up} m above the car"
        );
    }

    /// The intro starts in front of the car and ends where the chase is.
    #[test]
    fn intro_swings_round() {
        let (lr, car) = grid();
        let mut rig = CameraRig::default();
        let v = rig.update(0.0, &lr.track, &car, false, false, 1.6);
        let first = rig.intro(0.0, &car, v);
        let (fx, fz) = (kernel::cos(car.yaw), kernel::sin(car.yaw));
        let ahead = |e: DVec3| (e.x - car.x) * fx + (e.z - car.z) * fz;
        assert!(ahead(first.eye) > 5.0, "front three-quarter view");
        let mut last = first;
        for _ in 0..240 {
            let v = rig.update(1.0 / 60.0, &lr.track, &car, false, false, 1.6);
            last = rig.intro(1.0 / 60.0, &car, v);
        }
        assert!(ahead(last.eye) < -6.0, "behind the car at the end");
    }
}
