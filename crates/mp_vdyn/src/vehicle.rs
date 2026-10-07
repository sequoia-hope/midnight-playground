//! A Tier 1 vehicle (SPEC 4): one rigid chassis, independent suspension
//! with unsprung hubs, wheels that spin with brake friction, a brush tyre on
//! each, a simple drive, aerodynamics, all substepped inside the game's
//! 1/120 s tick (SPEC 3).
//!
//! A vehicle is data ([`VehicleDef`]): any number of axles, each with two
//! wheels. The state ([`Vehicle`]) is plain data: `Clone + PartialEq`, with
//! a byte hash for rollback and desync checks.

use core::f64::consts::PI;

use mp_math::{clamp, kernel};

use crate::body::RigidBody;
use crate::drivetrain::{EngineDef, EngineState, engine_step, gearbox_tick};
use crate::ground::{Collider, Ground};
use crate::math::{Aabb, Iso3, Quat, Vec3};
use crate::tyre::{BrushParams, BrushTyre, HubState, Tyre, TyreOutput};

pub const G: f64 = 9.81;
/// The game's tick (s).
pub const TICK: f64 = 1.0 / 120.0;
/// Anti-lock brakes: the slip ratio they aim for, and how fast the brake
/// pressure follows the error (per second per unit of slip ratio).
const ABS_TARGET: f64 = -0.1;
const ABS_GAIN: f64 = 60.0;
/// Traction control: the driven wheels' slip ratio it allows, and how fast
/// it cuts the throttle past it and gives it back below.
const TC_TARGET: f64 = 0.1;
const TC_CUT: f64 = 40.0;
const TC_RESTORE: f64 = 4.0;

/// One axle's suspension (independent: each hub slides along the body's up
/// axis, strut-like, SPEC 4.2). Travel is measured up from the hub's design
/// position: positive is bump (compression), negative droop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuspensionDef {
    /// Spring rate at the wheel (N/m).
    pub k: f64,
    /// The travel at which the spring force is zero (a negative number: the
    /// spring is preloaded at the design position).
    pub free: f64,
    /// Damping (N·s/m) in bump (compressing) and rebound.
    pub c_bump: f64,
    pub c_rebound: f64,
    /// Travel limits (m), with a stiff bump stop past them.
    pub travel_min: f64,
    pub travel_max: f64,
    pub stop_k: f64,
}

/// One axle and its two wheels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxleDef {
    /// Position of the axle along the body (m, forward of the centre of
    /// mass is positive).
    pub x: f64,
    /// Half the track width (m).
    pub half_track: f64,
    /// Height of the hub centres relative to the centre of mass at zero
    /// travel (m, negative: below it).
    pub hub_y: f64,
    pub susp: SuspensionDef,
    /// Anti-roll bar (N/m of travel difference between the two sides).
    pub arb: f64,
    /// The tyre's in-plane forces act on the body this far above the
    /// contact patch (m): the suspension's roll centre.
    pub roll_centre: f64,
    /// Road-wheel angle at full lock (rad); 0 for an unsteered axle.
    pub steer_lock: f64,
    /// Ackermann, 0..1 (1 is the full geometric difference in lock).
    pub ackermann: f64,
    /// Share of the drive torque that this axle takes (the shares add to 1).
    pub drive: f64,
    /// Brake torque per wheel at full pedal, and the handbrake's (N·m).
    pub brake: f64,
    pub handbrake: f64,
    /// Per wheel: spin inertia (kg·m²) and unsprung mass (kg).
    pub spin_inertia: f64,
    pub unsprung: f64,
    pub tyre: BrushParams,
}

/// The drive (V1): a torque at the wheels, capped by power, split by the
/// axles' shares and equally across each axle (an open differential).
/// V2 replaces it with an engine, a gearbox and differentials (SPEC 4.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveDef {
    /// Total drive torque at the wheels (N·m).
    pub max_torque: f64,
    /// Power limit (W).
    pub max_power: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AeroDef {
    /// Air density (kg/m³).
    pub rho: f64,
    /// Drag area `Cd·A` (m²).
    pub cda: f64,
    /// Downforce areas `C_L·A` at the first and the last axle (m²).
    pub cla_front: f64,
    pub cla_rear: f64,
}

/// The chassis as a box for contact with walls (SPEC 4.1): its half
/// extents in the body frame (forward, up, right) about the centre of
/// mass, and a penalty spring and damper per corner with Coulomb friction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChassisDef {
    pub half: Vec3,
    pub contact_k: f64,
    pub contact_c: f64,
    pub contact_mu: f64,
}

impl Default for ChassisDef {
    fn default() -> ChassisDef {
        ChassisDef {
            half: Vec3::new(2.2, 0.4, 0.9),
            contact_k: 5.0e6,
            contact_c: 4.0e4,
            contact_mu: 0.4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VehicleDef {
    pub name: &'static str,
    /// Total mass (kg), unsprung included.
    pub mass: f64,
    /// Principal moments of inertia of the chassis in the body frame:
    /// roll (x), yaw (y), pitch (z) (kg·m²).
    pub inertia: Vec3,
    pub axles: Vec<AxleDef>,
    /// The direct drive, used when there is no `engine`.
    pub drive: DriveDef,
    /// An engine and gearbox (SPEC 4.5), in place of the direct drive.
    pub engine: Option<EngineDef>,
    pub chassis: ChassisDef,
    pub aero: AeroDef,
    /// Road-wheel steering rate (rad/s).
    pub steer_rate: f64,
    /// Substeps per tick: `h = (1/120)/n` (SPEC 3).
    pub substeps: u32,
}

impl VehicleDef {
    pub fn unsprung(&self) -> f64 {
        self.axles.iter().map(|a| 2.0 * a.unsprung).sum()
    }

    pub fn sprung(&self) -> f64 {
        self.mass - self.unsprung()
    }

    /// The distance between the first and the last axle (m).
    pub fn wheelbase(&self) -> f64 {
        match (self.axles.first(), self.axles.last()) {
            (Some(f), Some(r)) => f.x - r.x,
            _ => 0.0,
        }
    }

    /// Each axle's static load on level ground (N, both wheels): by the
    /// lever rule for two axles, shared equally otherwise.
    pub fn static_axle_loads(&self) -> Vec<f64> {
        let w = self.mass * G;
        if self.axles.len() == 2 {
            let (f, r) = (self.axles[0].x, self.axles[1].x);
            vec![w * -r / (f - r), w * f / (f - r)]
        } else {
            vec![w / self.axles.len() as f64; self.axles.len()]
        }
    }

    /// Sets each spring's free length so that the static load puts the
    /// hubs at zero travel.
    pub fn balance_springs(&mut self) {
        let loads = self.static_axle_loads();
        for (a, load) in self.axles.iter_mut().zip(loads) {
            let spring = load / 2.0 - a.unsprung * G;
            a.susp.free = -spring / a.susp.k;
        }
    }

    /// The linear bicycle model's understeer gradient (rad per m/s² of
    /// lateral acceleration) for a two-axle car: `m_f/C_f − m_r/C_r`.
    pub fn understeer_gradient(&self) -> f64 {
        let loads = self.static_axle_loads();
        let (f, r) = (&self.axles[0], &self.axles[1]);
        let cf = 2.0 * f.tyre.stiffness();
        let cr = 2.0 * r.tyre.stiffness();
        loads[0] / G / cf - loads[1] / G / cr
    }
}

/// The driver's controls for one tick, held across its substeps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    /// −1..1 of full lock, positive to the right.
    pub steer: f64,
    /// With the direct drive, −1..1 of the drive torque (negative
    /// reverses); with an engine, 0..1 (the gear decides the direction).
    pub throttle: f64,
    /// 0..1.
    pub brake: f64,
    /// 0..1.
    pub handbrake: f64,
    /// Anti-lock brakes.
    pub abs: bool,
    /// Traction control (with an engine).
    pub tc: bool,
    /// A shift request this tick: +1 up, −1 down (the paddles; down from
    /// first is neutral, then reverse).
    pub shift: i32,
    /// No automatic gearbox: only `shift` changes gear.
    pub manual: bool,
    /// Extra engine torque, as a fraction (nitro): 0 normally.
    pub boost: f64,
}

/// One wheel's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Wheel {
    /// Suspension travel (m, positive in bump) and its rate (m/s).
    pub travel: f64,
    pub travel_vel: f64,
    /// Spin rate (rad/s, positive rolling forward) and the spin angle for
    /// drawing (rad, −π..π).
    pub omega: f64,
    pub spin: f64,
    /// Road-wheel angle (rad, positive to the right).
    pub steer: f64,
    /// Anti-lock modulation of the brake, 0..1.
    pub abs: f64,
    pub tyre: Tyre,
    /// The tyre's output in the last substep (derived; not hashed).
    pub out: TyreOutput,
}

/// A Tier 1 vehicle's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Vehicle {
    pub body: RigidBody,
    /// Axle by axle, left wheel then right.
    pub wheels: Vec<Wheel>,
    /// Road-wheel angle the steering has reached (rad, before Ackermann).
    pub steer: f64,
    /// The chassis's acceleration in the last substep (world, m/s²;
    /// derived, not hashed).
    pub accel: Vec3,
    /// The engine and gearbox, when the definition has one.
    pub engine: Option<EngineState>,
    /// Wall contact in the last tick (derived, not hashed).
    pub contact: WallContact,
}

/// The chassis's contact with walls over the last tick, for the race's
/// impact and scrape events.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WallContact {
    /// The fastest approach speed into a wall at first touch (m/s).
    pub impact: f64,
    /// Where (world), and the wall's normal there.
    pub point: Vec3,
    pub normal: Vec3,
    /// Touching a wall at all this tick.
    pub touching: bool,
}

/// Per wheel telemetry (SPEC 8.4).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelTelemetry {
    pub load: f64,
    /// Slip angle (rad) from the tyre's transient state, and the slip
    /// ratio from the wheel's and the ground's speed (as ABS sees it; over
    /// at least 1 m/s).
    pub slip_angle: f64,
    pub slip_ratio: f64,
    pub sliding: f64,
    pub travel: f64,
    pub omega: f64,
    pub steer: f64,
    pub fx: f64,
    pub fy: f64,
    pub aligning: f64,
    pub in_contact: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Telemetry {
    pub wheels: Vec<WheelTelemetry>,
    /// Sum of the steered tyres' aligning moments (N·m): the start of the
    /// rack force (SPEC 7.2).
    pub steer_torque: f64,
    /// Forward speed (m/s), yaw rate (rad/s, positive turning right) and
    /// the chassis's acceleration in its own frame (m/s²: forward, up,
    /// right).
    pub speed: f64,
    pub yaw_rate: f64,
    pub accel: Vec3,
}

fn wrap_pi(mut a: f64) -> f64 {
    while a > PI {
        a -= 2.0 * PI;
    }
    while a < -PI {
        a += 2.0 * PI;
    }
    a
}

impl Vehicle {
    /// The vehicle at rest on level ground at height `ground_y`, its centre
    /// of mass over (x, z), heading `yaw` (the game's convention), each hub
    /// at zero travel and each tyre at its static deflection.
    pub fn new(def: &VehicleDef, x: f64, ground_y: f64, z: f64, yaw: f64) -> Vehicle {
        let loads = def.static_axle_loads();
        let a0 = &def.axles[0];
        let pen = loads[0] / 2.0 / a0.tyre.kz;
        let y = ground_y + a0.tyre.radius - pen - a0.hub_y;
        let wheels = def
            .axles
            .iter()
            .flat_map(|a| {
                (0..2).map(|_| Wheel {
                    travel: 0.0,
                    travel_vel: 0.0,
                    omega: 0.0,
                    spin: 0.0,
                    steer: 0.0,
                    abs: 1.0,
                    tyre: Tyre::Brush(BrushTyre::new(a.tyre)),
                    out: TyreOutput::default(),
                })
            })
            .collect();
        Vehicle {
            body: RigidBody {
                pos: Vec3::new(x, y, z),
                rot: Quat::from_yaw(yaw),
                vel: Vec3::ZERO,
                ang_vel: Vec3::ZERO,
            },
            wheels,
            steer: 0.0,
            accel: Vec3::ZERO,
            engine: def.engine.as_ref().map(EngineState::new),
            contact: WallContact::default(),
        }
    }

    /// Sets the chassis's forward speed and spins every wheel to match.
    pub fn set_speed(&mut self, def: &VehicleDef, v: f64) {
        let fwd = self.forward();
        self.body.vel = fwd * v;
        for (i, w) in self.wheels.iter_mut().enumerate() {
            w.omega = v / def.axles[i / 2].tyre.radius;
        }
    }

    pub fn forward(&self) -> Vec3 {
        self.body.rot.rotate(Vec3::X)
    }

    pub fn up(&self) -> Vec3 {
        self.body.rot.rotate(Vec3::Y)
    }

    pub fn right(&self) -> Vec3 {
        self.body.rot.rotate(Vec3::Z)
    }

    /// Heading of the forward axis on the ground plane (the game's yaw).
    pub fn yaw(&self) -> f64 {
        let f = self.forward();
        kernel::atan2(f.z, f.x)
    }

    /// Signed forward speed (m/s).
    pub fn speed(&self) -> f64 {
        self.body.vel.dot(self.forward())
    }

    /// Lateral speed (m/s, positive to the right).
    pub fn lateral_speed(&self) -> f64 {
        self.body.vel.dot(self.right())
    }

    /// Yaw rate in the game's convention (rad/s, positive turning right).
    pub fn yaw_rate(&self) -> f64 {
        -self.body.ang_vel.dot(self.up())
    }

    /// The hub's design position in the body frame.
    fn mount(def: &VehicleDef, i: usize) -> Vec3 {
        let a = &def.axles[i / 2];
        let side = if i.is_multiple_of(2) { -1.0 } else { 1.0 };
        Vec3::new(a.x, a.hub_y, side * a.half_track)
    }

    /// Hub centre in world coordinates.
    pub fn hub_pos(&self, def: &VehicleDef, i: usize) -> Vec3 {
        let b = Vehicle::mount(def, i) + Vec3::Y * self.wheels[i].travel;
        self.body.pos + self.body.rot.rotate(b)
    }

    /// Each wheel's road-wheel angle for the steering angle `delta`, with
    /// Ackermann (the inner wheel steers more).
    fn wheel_angles(def: &VehicleDef, delta: f64, out: &mut [f64]) {
        let l = def.wheelbase();
        for (i, o) in out.iter_mut().enumerate() {
            let a = &def.axles[i / 2];
            let d = delta
                * if def.axles[0].steer_lock > 0.0 {
                    a.steer_lock / def.axles[0].steer_lock
                } else {
                    0.0
                };
            *o = d;
            if a.ackermann > 0.0 && d != 0.0 && l > 0.0 {
                // Turning right (d > 0) the right wheel (odd) is inside.
                let r = l / kernel::tan(d.abs());
                let inner = (i % 2 == 1) == (d > 0.0);
                let w = if inner { -a.half_track } else { a.half_track };
                let geo = kernel::atan(l / (r + w)) * d.signum();
                *o = d + a.ackermann * (geo - d);
            }
        }
    }

    /// One game tick: `def.substeps` substeps with the controls held.
    pub fn step(&mut self, def: &VehicleDef, ctl: &Controls, ground: &dyn Ground) {
        let n = def.substeps.max(1);
        let h = TICK / n as f64;

        // Steering: the road-wheel angle follows the command at the rate
        // limit, once per tick.
        let lock = def.axles.first().map_or(0.0, |a| a.steer_lock);
        let target = clamp(ctl.steer, -1.0, 1.0) * lock;
        let rate = def.steer_rate * TICK;
        self.steer += clamp(target - self.steer, -rate, rate);
        let mut angles = vec![0.0; self.wheels.len()];
        Vehicle::wheel_angles(def, self.steer, &mut angles);
        // The hub orientations relative to the body: a rotation by −δ
        // about the body's up axis (turning the rolling direction right).
        let steer_rot: Vec<Quat> = angles
            .iter()
            .map(|&d| {
                if d == 0.0 {
                    Quat::IDENTITY
                } else {
                    Quat::from_axis_angle(Vec3::Y, -d)
                }
            })
            .collect();
        for (w, &d) in self.wheels.iter_mut().zip(&angles) {
            w.steer = d;
        }

        // The gearbox, once a tick.
        if let (Some(ed), Some(es)) = (&def.engine, &mut self.engine) {
            gearbox_tick(ed, es, ctl.shift, !ctl.manual, TICK);
        }

        // Walls near the chassis, once a tick.
        let reach = def.chassis.half.length() + 1.0;
        let r = Vec3::new(reach, reach, reach);
        let mut walls = Vec::new();
        ground.colliders(
            Aabb {
                min: self.body.pos - r,
                max: self.body.pos + r,
            },
            &mut walls,
        );
        let was_touching = self.contact.touching;
        self.contact = WallContact::default();

        for _ in 0..n {
            self.substep(def, ctl, ground, &walls, &steer_rot, h);
        }
        if was_touching {
            // Only the first touch is an impact; sliding along is a scrape.
            self.contact.impact = 0.0;
        }
    }

    fn substep(
        &mut self,
        def: &VehicleDef,
        ctl: &Controls,
        ground: &dyn Ground,
        walls: &[Collider],
        steer_rot: &[Quat],
        h: f64,
    ) {
        let nw = self.wheels.len();
        let m_s = def.sprung();
        let m_all = def.mass;
        let rot = self.body.rot;
        let up = rot.rotate(Vec3::Y);
        let fwd = rot.rotate(Vec3::X);
        let gravity = Vec3::new(0.0, -G, 0.0);

        let mut f_up = 0.0; // force on the body along its up axis
        let mut f_perp = Vec3::ZERO; // everything else
        let mut torque = Vec3::ZERO;

        // Tyres.
        for i in 0..nw {
            let a = &def.axles[i / 2];
            let r = rot.rotate(Vehicle::mount(def, i) + Vec3::Y * self.wheels[i].travel);
            let centre = self.body.pos + r;
            let w = &mut self.wheels[i];
            let hub = HubState {
                pose: Iso3 {
                    pos: centre,
                    rot: rot * steer_rot[i],
                },
                vel: self.body.point_vel(r) + up * w.travel_vel,
                ang_vel: self.body.ang_vel,
                omega: w.omega,
            };
            w.out = w.tyre.step(&hub, ground, h);
            let out = &w.out;
            // The in-plane part reaches the body at the roll centre; the
            // part along the strut goes through the hub and its spring.
            let ft = out.force;
            let along = ft.dot(up);
            let perp = ft - up * along;
            let at = if out.info.in_contact {
                out.info.point + up * a.roll_centre
            } else {
                centre
            };
            f_perp += perp;
            torque += (at - self.body.pos).cross(perp);
            // The aligning moment (the moment's part about the normal).
            torque += up * out.moment.dot(up);
            // Unsprung weight across the strut.
            let wg = gravity * a.unsprung;
            let wg_perp = wg - up * wg.dot(up);
            f_perp += wg_perp;
            torque += r.cross(wg_perp);
        }

        // Suspension: springs, dampers, bump stops and anti-roll bars.
        let mut f_susp = vec![0.0; nw];
        for i in 0..nw {
            let s = &def.axles[i / 2].susp;
            let w = &self.wheels[i];
            let mut f = s.k * (w.travel - s.free);
            f += w.travel_vel
                * if w.travel_vel > 0.0 {
                    s.c_bump
                } else {
                    s.c_rebound
                };
            if w.travel > s.travel_max {
                f += s.stop_k * (w.travel - s.travel_max);
            } else if w.travel < s.travel_min {
                f += s.stop_k * (w.travel - s.travel_min);
            }
            f_susp[i] = f;
        }
        for (k, a) in def.axles.iter().enumerate() {
            let d = self.wheels[2 * k].travel - self.wheels[2 * k + 1].travel;
            f_susp[2 * k] += a.arb * d;
            f_susp[2 * k + 1] -= a.arb * d;
        }
        for i in 0..nw {
            let r = rot.rotate(Vehicle::mount(def, i));
            f_up += f_susp[i];
            torque += r.cross(up * f_susp[i]);
        }

        // Aerodynamics: drag at the centre of mass, downforce at the axles.
        let v = self.body.vel;
        let a = &def.aero;
        f_perp += v * (-0.5 * a.rho * a.cda * v.length());
        let vf = v.dot(fwd);
        let q = 0.5 * a.rho * vf * vf;
        if let (Some(fa), Some(ra)) = (def.axles.first(), def.axles.last()) {
            for (x, cla) in [(fa.x, a.cla_front), (ra.x, a.cla_rear)] {
                let f = -q * cla;
                f_up += f;
                torque += rot.rotate(Vec3::new(x, 0.0, 0.0)).cross(up * f);
            }
        }

        // Walls: each corner of the chassis box at the centre of mass's
        // height, pushed out by a penalty spring and damper, with Coulomb
        // friction along the wall (SPEC 4.1).
        let ch = &def.chassis;
        for (sx, sz) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let r = rot.rotate(Vec3::new(sx * ch.half.x, 0.0, sz * ch.half.z));
            let corner = self.body.pos + r;
            for wall in walls {
                let Collider::Plane { point, normal } = *wall;
                let d = (corner - point).dot(normal);
                if d >= 0.0 {
                    continue;
                }
                let vc = self.body.point_vel(r);
                let vn = vc.dot(normal);
                let fn_ = -ch.contact_k * d - ch.contact_c * vn;
                if fn_ <= 0.0 {
                    continue;
                }
                let vt = vc - normal * vn;
                let vt_mag = vt.length();
                let mut f = normal * fn_;
                if vt_mag > 0.0 {
                    let ft = (ch.contact_mu * fn_).min(ch.contact_c * vt_mag);
                    f -= vt * (ft / vt_mag);
                }
                let along_up = f.dot(up);
                f_up += along_up;
                f_perp += f - up * along_up;
                torque += r.cross(f);
                let c = &mut self.contact;
                if !c.touching || -vn > c.impact {
                    c.impact = c.impact.max(-vn);
                    c.point = corner;
                    c.normal = normal;
                }
                c.touching = true;
            }
        }

        // The chassis: its weight, and the hubs moving with it across the
        // strut axis (they slide only along it).
        let wg = gravity * m_s;
        let along = f_up + wg.dot(up);
        let perp = f_perp + (wg - up * wg.dot(up));
        let accel = up * (along / m_s) + perp * (1.0 / m_all);
        // Hubs' absolute speed along the strut, before the kick.
        let mut u_hub = vec![0.0; nw];
        for i in 0..nw {
            let a = &def.axles[i / 2];
            let r = rot.rotate(Vehicle::mount(def, i) + Vec3::Y * self.wheels[i].travel);
            let w = &self.wheels[i];
            let f = w.out.force.dot(up) - f_susp[i] + a.unsprung * gravity.dot(up);
            u_hub[i] = self.body.point_vel(r).dot(up) + w.travel_vel + f / a.unsprung * h;
        }
        self.body.kick(accel, torque, def.inertia, h);
        self.accel = accel;

        // Wheels: spin, with drive, the road's torque and brake friction.
        let (t_engine, j_engine) = match (&def.engine, &mut self.engine) {
            (Some(ed), Some(es)) => {
                let omega: f64 = (0..nw)
                    .map(|i| self.wheels[i].omega * def.axles[i / 2].drive * 0.5)
                    .sum();
                // Traction control: the worst driven wheel's slip ratio,
                // from wheel speed against ground speed.
                if ctl.tc {
                    let mut slip: f64 = 0.0;
                    for (i, w) in self.wheels.iter().enumerate() {
                        let info = &w.out.info;
                        if def.axles[i / 2].drive > 0.0 && info.in_contact {
                            let v = info.ground_speed.abs().max(2.0);
                            slip = slip.max((info.roll_speed.abs() - info.ground_speed.abs()) / v);
                        }
                    }
                    es.tc = if slip > TC_TARGET {
                        (es.tc - h * TC_CUT * (slip - TC_TARGET)).max(0.0)
                    } else {
                        (es.tc + h * TC_RESTORE).min(1.0)
                    };
                } else {
                    es.tc = 1.0;
                }
                let out = engine_step(ed, es, ctl.throttle * es.tc, omega);
                let t = if out.wheel_torque > 0.0 {
                    out.wheel_torque * (1.0 + ctl.boost)
                } else {
                    out.wheel_torque
                };
                (Some(t), out.wheel_inertia)
            }
            _ => (None, 0.0),
        };
        let driven: f64 = (0..nw)
            .filter(|&i| def.axles[i / 2].drive > 0.0)
            .map(|i| self.wheels[i].omega.abs())
            .sum::<f64>();
        let n_driven = (0..nw).filter(|&i| def.axles[i / 2].drive > 0.0).count();
        let w_avg = if n_driven > 0 {
            driven / n_driven as f64
        } else {
            0.0
        };
        let w_cap = if w_avg > 1.0 { w_avg } else { 1.0 };
        let t_avail = clamp(def.drive.max_power / w_cap, 0.0, def.drive.max_torque);
        let t_total = t_engine.unwrap_or(clamp(ctl.throttle, -1.0, 1.0) * t_avail);
        for i in 0..nw {
            let a = &def.axles[i / 2];
            let w = &mut self.wheels[i];
            // Anti-lock: hold each wheel's slip ratio near the tyre's peak,
            // reading only what a car's electronics could (wheel speed
            // against the ground speed).
            let info = &w.out.info;
            if ctl.abs && ctl.brake > 0.0 && info.in_contact && info.ground_speed > 2.0 {
                let kappa = (info.roll_speed - info.ground_speed) / info.ground_speed;
                w.abs = clamp(w.abs + h * ABS_GAIN * (kappa - ABS_TARGET), 0.0, 1.0);
            } else {
                w.abs = 1.0;
            }
            let t_drive = t_total * a.drive * 0.5;
            let t_brake = a.brake * clamp(ctl.brake, 0.0, 1.0) * w.abs
                + a.handbrake * clamp(ctl.handbrake, 0.0, 1.0)
                + w.out.rolling_torque;
            // A closed clutch adds the engine's inertia to the driven wheels.
            let j = a.spin_inertia + j_engine * a.drive * 0.5;
            let free = w.omega + (t_drive + w.out.spin_torque) / j * h;
            // Brakes are friction, not a negative torque: they can stop the
            // wheel, never turn it the other way (SPEC 4.3).
            let stop = t_brake / j * h;
            w.omega = if free.abs() <= stop {
                0.0
            } else {
                free - stop * free.signum()
            };
            w.spin = wrap_pi(w.spin + w.omega * h);
        }

        // Hubs: travel from their new speed relative to the chassis.
        let up_new = self.body.rot.rotate(Vec3::Y);
        for i in 0..nw {
            let r = rot.rotate(Vehicle::mount(def, i) + Vec3::Y * self.wheels[i].travel);
            let w = &mut self.wheels[i];
            w.travel_vel = u_hub[i] - self.body.point_vel(r).dot(up_new);
            w.travel += w.travel_vel * h;
        }
        self.body.drift(h);
    }

    /// Readouts for tuning (SPEC 8.4), from the last substep.
    pub fn telemetry(&self, def: &VehicleDef) -> Telemetry {
        let wheels: Vec<WheelTelemetry> = self
            .wheels
            .iter()
            .map(|w| {
                let i = &w.out.info;
                let v = i.ground_speed.abs();
                WheelTelemetry {
                    load: i.load,
                    slip_angle: kernel::atan(i.slip_tan),
                    slip_ratio: (i.roll_speed - i.ground_speed) / if v > 1.0 { v } else { 1.0 },
                    sliding: i.sliding,
                    travel: w.travel,
                    omega: w.omega,
                    steer: w.steer,
                    fx: i.fx,
                    fy: i.fy,
                    aligning: i.aligning,
                    in_contact: i.in_contact,
                }
            })
            .collect();
        let steer_torque = wheels
            .iter()
            .enumerate()
            .filter(|(i, _)| def.axles[i / 2].steer_lock > 0.0)
            .map(|(_, w)| w.aligning)
            .sum();
        let rot = self.body.rot;
        let ab = rot.inv_rotate(self.accel);
        Telemetry {
            wheels,
            steer_torque,
            speed: self.speed(),
            yaw_rate: self.yaw_rate(),
            accel: ab,
        }
    }

    /// Kinetic energy of the chassis, the hubs along their struts and the
    /// wheels' spin, plus potential energy (gravity and springs) about
    /// `ground_y` (J). The energy test's measure.
    pub fn energy(&self, def: &VehicleDef) -> f64 {
        let m_s = def.sprung();
        let mut e = self.body.kinetic_energy(m_s, def.inertia) + m_s * G * self.body.pos.y;
        let up = self.up();
        for (i, w) in self.wheels.iter().enumerate() {
            let a = &def.axles[i / 2];
            let r = self
                .body
                .rot
                .rotate(Vehicle::mount(def, i) + Vec3::Y * w.travel);
            let v = self.body.point_vel(r) + up * w.travel_vel;
            e += 0.5 * a.unsprung * v.length_sq();
            e += a.unsprung * G * (self.body.pos.y + r.y);
            e += 0.5 * a.spin_inertia * w.omega * w.omega;
            let s = &a.susp;
            let x = w.travel - s.free;
            e += 0.5 * s.k * x * x;
            if w.travel > s.travel_max {
                let d = w.travel - s.travel_max;
                e += 0.5 * s.stop_k * d * d;
            } else if w.travel < s.travel_min {
                let d = w.travel - s.travel_min;
                e += 0.5 * s.stop_k * d * d;
            }
            if w.out.info.in_contact {
                let d = w.out.info.deflection;
                e += 0.5 * a.tyre.kz * d * d;
            }
        }
        for (k, a) in def.axles.iter().enumerate() {
            let d = self.wheels[2 * k].travel - self.wheels[2 * k + 1].travel;
            e += 0.5 * a.arb * d * d;
        }
        e
    }

    /// Appends every field that affects later ticks, as bits (rollback and
    /// desync checks; the tyre outputs and acceleration are derived).
    pub fn hash_into(&self, out: &mut Vec<u8>) {
        self.body.hash_into(out);
        out.extend_from_slice(&self.steer.to_bits().to_le_bytes());
        if let Some(e) = &self.engine {
            e.hash_into(out);
        }
        for w in &self.wheels {
            for v in [w.travel, w.travel_vel, w.omega, w.spin, w.steer, w.abs] {
                out.extend_from_slice(&v.to_bits().to_le_bytes());
            }
            w.tyre.hash_into(out);
        }
    }

    /// FNV-1a 64 of [`Vehicle::hash_into`].
    pub fn hash(&self) -> u64 {
        let mut b = Vec::with_capacity(512);
        self.hash_into(&mut b);
        let mut h: u64 = 0xcbf29ce484222325;
        for x in b {
            h ^= x as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
}
