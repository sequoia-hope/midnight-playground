//! Cars that live in track coordinates (s along the road, lat across it):
//! rivals and traffic (port of `src/vehicles/Kinematic.js`). Much cheaper and
//! more robust than full physics, and they can still be shoved around —
//! collisions feed back into speed, lateral velocity and a decaying spin.

use core::f64::consts::PI;

use mr_math::{clamp, damp, js, kernel};
use mr_track::{Frame, Track};

use crate::body::{AgentView, Body};
use crate::vehicle::Vehicle;

/// The surface a car's height comes from (`yFn`, SPEC 4.3: a closure in the
/// JS, data here).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Surface {
    /// The road: `track.surfaceY(s, lat)`.
    #[default]
    Road,
    /// The opposite carriageway: `oppY(track.frame(s))`.
    Opposite,
    /// The road plus a height (a sawhorse flying off a roadblock).
    Raised(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kinematic {
    pub v: Vehicle,
    pub s: f64,
    pub lat: f64,
    /// Along the road in direction `dir`.
    pub speed: f64,
    pub lat_vel: f64,
    /// +1 with the race, −1 oncoming.
    pub dir: i32,
    /// Extra yaw from being hit.
    pub spin: f64,
    pub spin_rate: f64,
    /// The last frame `frame()` computed (`this.F`): `velocity()`,
    /// `setVelocity()` and `translate()` use it, whatever s it was taken at.
    pub f: Frame,
    /// Seconds of reduced control after a hit.
    pub stunned: f64,
    /// True: not bound by our road's walls (other carriageway).
    pub free_lat: bool,
    /// Optional surface height override.
    pub surface: Surface,
}

impl Kinematic {
    pub fn new(v: Vehicle) -> Kinematic {
        Kinematic {
            v,
            s: 0.0,
            lat: 0.0,
            speed: 0.0,
            lat_vel: 0.0,
            dir: 1,
            spin: 0.0,
            spin_rate: 0.0,
            f: Frame::default(),
            stunned: 0.0,
            free_lat: false,
            surface: Surface::Road,
        }
    }

    pub fn frame(&mut self, t: &Track) -> Frame {
        self.f = t.frame(self.s);
        self.f
    }

    pub fn half_w(&self) -> f64 {
        self.v.half_w
    }

    pub fn half_l(&self) -> f64 {
        self.v.half_l
    }

    /// World-space velocity (for collisions).
    pub fn velocity(&self) -> (f64, f64) {
        let f = &self.f;
        let dir = self.dir as f64;
        (
            f.fx * self.speed * dir + f.rx * self.lat_vel,
            f.fz * self.speed * dir + f.rz * self.lat_vel,
        )
    }

    pub fn set_velocity(&mut self, vx: f64, vz: f64) {
        let f = &self.f;
        self.speed = (vx * f.fx + vz * f.fz) * self.dir as f64;
        self.lat_vel = vx * f.rx + vz * f.rz;
    }

    pub fn translate(&mut self, t: &Track, dx: f64, dz: f64) {
        let f = &self.f;
        self.s += dx * f.fx + dz * f.fz;
        self.lat += dx * f.rx + dz * f.rz;
        self.write_pos(t);
    }

    pub fn add_spin(&mut self, w: f64) {
        self.spin_rate += w;
        self.stunned = js::max(self.stunned, js::min(1.5, w.abs() * 0.8));
    }

    /// Advance along the road; curvature makes inside lines shorter.
    pub fn advance(&mut self, t: &Track, dt: f64) {
        let f = self.frame(t);
        let k = f.kappa;
        let scale = 1.0 / js::max(0.4, 1.0 - k * self.lat);
        self.s += self.speed * self.dir as f64 * dt * scale;
        self.lat += self.lat_vel * dt;
        // Walls.
        let f2 = self.frame(t);
        let lim = self.v.half_w + 0.15;
        // Cars on the other carriageway (freeLat) aren't bound by our walls.
        if !self.free_lat {
            if self.lat > f2.wall_r - lim {
                self.lat = f2.wall_r - lim;
                if self.lat_vel > 0.0 {
                    self.lat_vel *= -0.3;
                }
            }
            if self.lat < -(f2.wall_l - lim) {
                self.lat = -(f2.wall_l - lim);
                if self.lat_vel < 0.0 {
                    self.lat_vel *= -0.3;
                }
            }
        }
        self.s = if t.is_loop {
            t.wrap(self.s)
        } else {
            clamp(self.s, 0.0, t.road_end() - 1.0)
        };
        // Spin decays back to straight.
        self.spin += self.spin_rate * dt;
        self.spin_rate = damp(self.spin_rate, 0.0, 2.5, dt);
        self.spin = damp(
            self.spin,
            0.0,
            if self.stunned > 0.0 { 0.6 } else { 2.2 },
            dt,
        );
        self.stunned = js::max(0.0, self.stunned - dt);
        self.write_pos(t);
    }

    pub fn write_pos(&mut self, t: &Track) {
        let f = self.frame(t);
        let v = &mut self.v;
        v.s = self.s;
        v.lat = self.lat;
        v.x = f.x + f.rx * self.lat;
        v.z = f.z + f.rz * self.lat;
        v.y = match self.surface {
            Surface::Road => t.surface_y(self.s, self.lat),
            Surface::Opposite => mr_levels::world::opp_y(&t.frame(self.s)),
            Surface::Raised(h) => t.surface_y(self.s, self.lat) + h,
        };
        let base = kernel::atan2(f.fz, f.fx) + if self.dir < 0 { PI } else { 0.0 };
        let crab = kernel::atan2(self.lat_vel * self.dir as f64, js::max(4.0, self.speed));
        v.yaw = base + crab * 0.8;
        v.visual_yaw = self.spin;
        v.speed = self.speed;
        let (vx, vz) = self.velocity();
        self.v.vx = vx;
        self.v.vz = vz;
    }

    pub fn view(&self) -> AgentView {
        AgentView {
            id: crate::body::BodyId::Anon,
            police: false,
            kinematic_only: false,
            s: self.s,
            lat: self.lat,
            dir: self.dir,
            half_w: self.v.half_w,
            half_l: self.v.half_l,
            speed_along: self.speed,
            gap_lat: None,
        }
    }
}

impl Body for Kinematic {
    fn v(&self) -> &Vehicle {
        &self.v
    }
    fn mass(&self) -> f64 {
        self.v.mass
    }
    fn velocity(&self) -> (f64, f64) {
        Kinematic::velocity(self)
    }
    fn set_velocity(&mut self, vx: f64, vz: f64) {
        Kinematic::set_velocity(self, vx, vz)
    }
    fn translate(&mut self, t: &Track, dx: f64, dz: f64) {
        Kinematic::translate(self, t, dx, dz)
    }
    fn add_spin(&mut self, w: f64) {
        Kinematic::add_spin(self, w)
    }
    fn view(&self) -> AgentView {
        Kinematic::view(self)
    }
}
