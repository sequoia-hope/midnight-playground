//! Shared state for every car on the road — player, rivals and traffic (port
//! of the state in `src/vehicles/Vehicle.js`).
//!
//! Heading convention: yaw θ, forward = (cos θ, sin θ) in the XZ plane,
//! right = (−sin θ, cos θ); a positive yaw rate turns right.
//!
//! `sync()` and the body's pitch and roll springs are presentation: they live
//! in the client (SPEC 4.4). Physics' landing kick to the pitch spring
//! travels in [`crate::physics::PhysEvent::Touchdown`].

use mp_track::Track;
use pk_math::kernel;

use crate::dims::Dims;

#[derive(Clone, Debug, PartialEq)]
pub struct Vehicle {
    pub kind: &'static str,
    pub name: &'static str,
    pub color: u32,
    pub mass: f64,
    /// The model's dimensions (`model.dims`).
    pub dims: Dims,
    pub half_w: f64,
    pub half_l: f64,
    pub radius: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f64,
    pub vx: f64,
    pub vz: f64,
    pub vy: f64,
    pub yaw_rate: f64,
    pub s: f64,
    pub lat: f64,
    /// The road of the level's network the car is on (0: the level's own
    /// road; k: its (k - 1)th branch), and its place along and across it.
    /// On road 0 these are `s` and `lat`; on a branch `s` and `lat` are the
    /// car's place on the level's road (its progress), as the race, the
    /// rivals and the HUD measure it.
    pub road: usize,
    pub road_s: f64,
    pub road_lat: f64,
    /// Signed forward speed.
    pub speed: f64,
    pub steer_angle: f64,
    /// Read by the client's body springs.
    pub accel_long: f64,
    pub accel_lat: f64,
    pub on_ground: bool,
    pub airborne: f64,
    pub brake_light: f64,
    /// Extra yaw for spins (kinematic cars).
    pub visual_yaw: f64,
    pub alive: bool,
    /// Where the body is drawn: the ground under it while on the ground
    /// (`visY`, set by physics).
    pub vis_y: Option<f64>,
    /// Race progress (`v.prog`), set by the grid and the race rules.
    pub prog: Option<f64>,
    /// A simulated chassis's pose for drawing (docs/vehicle-dynamics/SPEC.md
    /// 8.3), beside this body view: present only for a car whose
    /// orientation and wheels are real state. The client uses it when
    /// present and its own pitch and roll springs otherwise. Derived each
    /// tick; not part of the state hash.
    pub pose: Option<Box<SimPose>>,
}

/// What the client draws for a simulated chassis (SPEC 8.3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SimPose {
    /// The chassis's orientation, body to world, as a quaternion
    /// `[x, y, z, w]`. Body axes: x forward, y up, z right.
    pub orient: [f64; 4],
    /// Axle by axle, left wheel then right.
    pub wheels: Vec<WheelPose>,
}

/// One wheel of a [`SimPose`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelPose {
    /// Hub centre, world frame.
    pub hub: [f64; 3],
    /// Road-wheel angle (rad, positive to the right).
    pub steer: f64,
    /// Spin angle about the axle (rad, −π..π, increasing rolling forward).
    pub spin: f64,
    /// Suspension travel (m, positive in bump).
    pub travel: f64,
}

impl Vehicle {
    pub fn new(
        dims: Dims,
        kind: &'static str,
        mass: f64,
        name: &'static str,
        color: u32,
    ) -> Vehicle {
        Vehicle {
            kind,
            name,
            color,
            mass,
            dims,
            half_w: dims.width / 2.0,
            half_l: dims.length / 2.0,
            radius: dims.width / 2.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            vx: 0.0,
            vz: 0.0,
            vy: 0.0,
            yaw_rate: 0.0,
            s: 0.0,
            lat: 0.0,
            road: 0,
            road_s: 0.0,
            road_lat: 0.0,
            speed: 0.0,
            steer_angle: 0.0,
            accel_long: 0.0,
            accel_lat: 0.0,
            on_ground: true,
            airborne: 0.0,
            brake_light: 0.0,
            visual_yaw: 0.0,
            alive: true,
            vis_y: None,
            prog: None,
            pose: None,
        }
    }

    pub fn fx(&self) -> f64 {
        kernel::cos(self.yaw)
    }

    /// Where to search for the car along the road it is on: `s` on the
    /// level's road (which the race may set without `road_s`), `road_s`
    /// on a branch.
    pub fn road_hint(&self) -> f64 {
        if self.road == 0 { self.s } else { self.road_s }
    }

    /// Projects the car onto the level's road network: the road under it
    /// (`road`, `road_s`, `road_lat`; [`Track::project_road`]) and its
    /// place on the level's road (`s`, `lat`). Returns the road it is on.
    pub fn project_onto<'a>(&mut self, t: &'a Track) -> &'a Track {
        let (road, p) = t.project_road(self.x, self.z, self.road, self.road_hint());
        self.road = road;
        self.road_s = p.s;
        self.road_lat = p.lat;
        if road == 0 {
            self.s = p.s;
            self.lat = p.lat;
        } else {
            let q = t.project(self.x, self.z, self.s);
            self.s = q.s;
            self.lat = q.lat;
        }
        t.road(road)
    }

    pub fn fz(&self) -> f64 {
        kernel::sin(self.yaw)
    }

    pub fn place(&mut self, track: &Track, s: f64, lat: f64, yaw_offset: f64) {
        let p = track.point_at(s, lat);
        let f = track.frame(s);
        self.x = p.x;
        self.y = p.y;
        self.z = p.z;
        self.s = s;
        self.lat = lat;
        self.road = 0;
        self.road_s = s;
        self.road_lat = lat;
        self.yaw = kernel::atan2(f.fz, f.fx) + yaw_offset;
        self.vx = 0.0;
        self.vz = 0.0;
        self.vy = 0.0;
        self.yaw_rate = 0.0;
        self.speed = 0.0;
    }
}
