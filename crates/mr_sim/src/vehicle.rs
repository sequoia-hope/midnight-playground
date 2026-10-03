//! Shared state for every car on the road — player, rivals and traffic (port
//! of the state in `src/vehicles/Vehicle.js`).
//!
//! Heading convention: yaw θ, forward = (cos θ, sin θ) in the XZ plane,
//! right = (−sin θ, cos θ); a positive yaw rate turns right.
//!
//! `sync()` and the body's pitch and roll springs are presentation: they live
//! in the client (SPEC 4.4). Physics' landing kick to the pitch spring
//! travels in [`crate::physics::PhysEvent::Touchdown`].

use mr_math::kernel;
use mr_track::Track;

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
        }
    }

    pub fn fx(&self) -> f64 {
        kernel::cos(self.yaw)
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
        self.yaw = kernel::atan2(f.fz, f.fx) + yaw_offset;
        self.vx = 0.0;
        self.vz = 0.0;
        self.vy = 0.0;
        self.yaw_rate = 0.0;
        self.speed = 0.0;
    }
}
