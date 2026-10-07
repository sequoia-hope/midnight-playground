//! Which model drives a player's car (docs/vehicle-dynamics/SPEC.md 8.1),
//! and the sim car (Tier 1) behind it.
//!
//! The spec's sketch makes `PlayerCar.phys` itself the enum. Here
//! `PlayerCar.phys` stays a `CarPhysics` (DECISIONS D1140): it is the body
//! view's half that the race, the HUD, the audio and the client read
//! (gear, rpm, nitro, drifting, slip, events, locked, damage, ...), the
//! arcade model runs on it exactly as before, and a sim car writes the same
//! fields every tick (SPEC 4.7). `PlayerCar.model` says which model moves
//! the car.
//!
//! A sim car (`SimCar`) owns an `mp_vdyn` vehicle. Each tick it reads the
//! controls through the *Casual* assists (SPEC 4.8: a grip-scaled steering
//! lock, ABS, traction control, the automatic gearbox, the arcade's brake-
//! to-reverse), steps the vehicle on the track through `TrackGround`, and
//! writes the body view (`Vehicle`) and the `CarPhysics` readouts. Whatever
//! the rest of the tick then does to the body view (a collision's push and
//! spin, the perfect start's kick, a reset) it takes back at the start of
//! its next update: small changes as impulses on the rigid body, a jump as
//! a re-placement (SPEC 4.7).

use mp_math::{clamp, kernel, smoothstep};
use mp_track::Track;
use mp_vdyn::cars::vento_gt;
use mp_vdyn::{Controls, Ground, Quat, Vec3, VehicleDef};

use crate::ground::TrackGround;
use crate::input::{Input, InputFrame, SHIFT_DOWN, SHIFT_UP};
use crate::physics::{ANALOG_LOCK, CarPhysics, PhysEvent};
use crate::vehicle::{SimPose, Vehicle, WheelPose};

/// The model that moves a player's car.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum VehicleModel {
    /// Today's handling: `CarPhysics` on the `Vehicle` (Tier 0).
    #[default]
    Arcade,
    /// A Tier 1 sim car.
    Sim(Box<SimCar>),
}

/// The sim definition for a garage car. V2 has one, the Vento GT; until
/// the others have theirs (V4) every car drives as a Vento with its own
/// mass.
pub fn sim_def(kind: &str, mass: f64) -> VehicleDef {
    let mut def = vento_gt();
    if kind != "sports" {
        def.mass = mass;
        def.balance_springs();
    }
    def
}

/// What the sim car last wrote into the body view, to tell what others
/// changed since.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Written {
    x: f64,
    z: f64,
    yaw: f64,
    vx: f64,
    vz: f64,
    yaw_rate: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimCar {
    pub def: VehicleDef,
    pub car: mp_vdyn::Vehicle,
    /// The chassis's centre of mass over the ground at rest (m): the body
    /// view's `y` is the centre of mass less this.
    rest_height: f64,
    written: Written,
    /// Seconds without any tyre on the ground.
    air: f64,
}

/// Physics constants shared with the arcade model's readouts.
const DRIFT_ON: f64 = 0.18;
const DRIFT_OFF: f64 = 0.06;
/// Nitro: extra engine torque while it burns, and how long a full tank
/// lasts (s), as the arcade's.
const NITRO_BOOST: f64 = 0.35;
const NITRO_SECS: f64 = 6.0;
/// The arcade's idle, for the countdown's revs.
const IDLE: f64 = 900.0;

impl SimCar {
    /// A sim car where the body view `v` is, at its speed.
    pub fn new(def: VehicleDef, v: &Vehicle, t: &Track) -> SimCar {
        let ground = t.surface_y(v.s, v.lat);
        let mut car = mp_vdyn::Vehicle::new(&def, v.x, ground, v.z, v.yaw);
        let rest_height = car.body.pos.y - ground;
        let speed = v.vx * kernel::cos(v.yaw) + v.vz * kernel::sin(v.yaw);
        car.set_speed(&def, speed);
        let mut s = SimCar {
            def,
            car,
            rest_height,
            written: Written::default(),
            air: 0.0,
        };
        s.written = s.snapshot_of(v);
        s
    }

    fn snapshot_of(&self, v: &Vehicle) -> Written {
        Written {
            x: v.x,
            z: v.z,
            yaw: v.yaw,
            vx: v.vx,
            vz: v.vz,
            yaw_rate: v.yaw_rate,
        }
    }

    /// Takes back what others did to the body view since the last tick.
    fn sync_in(&mut self, v: &Vehicle, t: &Track) {
        let w = self.written;
        let dx = v.x - w.x;
        let dz = v.z - w.z;
        if dx * dx + dz * dz > 1.0 || (v.yaw - w.yaw).abs() > 0.3 {
            // A reset or a placement: start again there.
            *self = SimCar::new(self.def.clone(), v, t);
            return;
        }
        let b = &mut self.car.body;
        b.pos.x += dx;
        b.pos.z += dz;
        b.vel.x += v.vx - w.vx;
        b.vel.z += v.vz - w.vz;
        // The game's yaw rate is −ω about up.
        let up = b.rot.rotate(Vec3::Y);
        b.ang_vel -= up * (v.yaw_rate - w.yaw_rate);
        let dyaw = v.yaw - w.yaw;
        if dyaw != 0.0 {
            b.rot = (Quat::from_yaw(dyaw) * b.rot).normalize();
        }
    }

    /// The *Casual* assists: the race's controls to the vehicle's.
    fn controls(&mut self, phys: &CarPhysics, inp: &Input, frame: &InputFrame) -> Controls {
        let car = &mut self.car;
        let speed = car.speed();
        // Stopped means stopped, not sliding sideways or spinning.
        let still = car.body.vel.length() < 0.5;
        let es = car.engine.as_mut();
        let mut throttle = inp.throttle;
        let mut brake = inp.brake;
        let mut shift = 0;
        if frame.flags & SHIFT_UP != 0 {
            shift = 1;
        } else if frame.flags & SHIFT_DOWN != 0 {
            shift = -1;
        }
        if let Some(es) = es {
            // Brake held at a standstill reverses, as in the arcade; the
            // throttle then brakes, and takes it out of reverse when
            // stopped.
            if es.gear >= 0 && still && inp.brake > 0.3 && inp.throttle < 0.05 {
                es.gear = -1;
            } else if es.gear == -1 && speed > -0.5 && inp.throttle > 0.05 {
                es.gear = 1;
            }
            if es.gear == -1 {
                throttle = inp.brake;
                brake = inp.throttle;
            }
        }
        if phys.locked {
            throttle = 0.0;
            brake = 1.0;
        }
        // Grip-scaled lock: full steering asks a little more than the front
        // tyres hold at this speed (the arcade's ANALOG_LOCK idea).
        let a0 = &self.def.axles[0];
        let lock = a0.steer_lock;
        let mu_g = a0.tyre.mu0 * mp_vdyn::vehicle::G;
        let v = speed.abs().max(1.0);
        let peak_slip = 3.0 * a0.tyre.mu0 * a0.tyre.fz0 / a0.tyre.stiffness();
        let want = ANALOG_LOCK * self.def.wheelbase() * mu_g / (v * v) + peak_slip;
        let steer_max = clamp(want, 0.0, lock);
        let boost = if inp.nitro && phys.nitro > 0.0 && speed > 4.0 && inp.throttle > 0.1 {
            NITRO_BOOST
        } else {
            0.0
        };
        Controls {
            steer: inp.steer * steer_max / lock,
            throttle,
            brake,
            handbrake: if inp.handbrake { 1.0 } else { 0.0 },
            abs: true,
            tc: true,
            shift,
            manual: false,
            boost,
        }
    }

    /// One tick for the player's car: steps the sim and writes the body
    /// view and the `CarPhysics` readouts.
    pub fn update(
        &mut self,
        v: &mut Vehicle,
        phys: &mut CarPhysics,
        t: &Track,
        dt: f64,
        inp: &Input,
        frame: &InputFrame,
    ) {
        self.sync_in(v, t);
        let ctl = self.controls(phys, inp, frame);
        // Shredded tyres (Hot Pursuit's spikes) grip less.
        if phys.spiked > 0.0 {
            phys.spiked = (phys.spiked - dt).max(0.0);
        }
        let grip = if phys.spiked > 0.0 { 0.7 } else { 1.0 };
        for w in self.car.wheels.iter_mut() {
            w.tyre.set_grip(grip);
        }
        let ground = TrackGround::new(t, v.s);
        self.car.step(&self.def, &ctl, &ground);
        if ctl.boost > 0.0 {
            phys.nitro = (phys.nitro - dt / NITRO_SECS).max(0.0);
        }
        phys.nitro_active = ctl.boost > 0.0;
        self.write_out(v, phys, t, &ground, &ctl, dt);
        if phys.locked {
            // Held on the grid: the engine revs with the throttle for the
            // start, as the arcade's does.
            phys.rpm = IDLE + inp.throttle * 6200.0;
        }
    }

    fn write_out(
        &mut self,
        v: &mut Vehicle,
        phys: &mut CarPhysics,
        t: &Track,
        ground: &TrackGround,
        ctl: &Controls,
        dt: f64,
    ) {
        let car = &self.car;
        let b = &car.body;
        let tm = car.telemetry(&self.def);

        // The body view.
        v.x = b.pos.x;
        v.z = b.pos.z;
        v.y = b.pos.y - self.rest_height;
        v.vx = b.vel.x;
        v.vz = b.vel.z;
        v.vy = b.vel.y;
        v.yaw = car.yaw();
        v.yaw_rate = car.yaw_rate();
        let p = t.project(v.x, v.z, v.s);
        v.s = p.s;
        v.lat = p.lat;
        v.speed = car.speed();
        v.steer_angle = car.steer;
        v.accel_long = tm.accel.x;
        v.accel_lat = tm.accel.z;
        let on_ground = tm.wheels.iter().any(|w| w.in_contact);
        v.on_ground = on_ground;
        v.brake_light = if ctl.brake > 0.0 && v.speed > 0.5 {
            1.0
        } else {
            0.0
        };
        v.vis_y = Some(v.y);
        let pose = SimPose {
            orient: b.rot.to_array(),
            wheels: car
                .wheels
                .iter()
                .enumerate()
                .map(|(i, w)| WheelPose {
                    hub: car.hub_pos(&self.def, i).to_array(),
                    steer: w.steer,
                    spin: w.spin,
                    travel: w.travel,
                })
                .collect(),
        };
        v.pose = Some(Box::new(pose));
        self.written = self.snapshot_of(v);

        // The readouts the race and the client take from CarPhysics.
        let speed = kernel::hypot(v.vx, v.vz);
        if let Some(e) = &car.engine {
            phys.gear = e.gear;
            phys.rpm = e.rpm;
            if e.shifted != 0 {
                phys.events.push(PhysEvent::Shift { up: e.shifted > 0 });
            }
        }
        // Drift from the rear axle's slip angle, with the arcade's
        // thresholds, so drift scoring carries over.
        let rear = &self.def.axles[self.def.axles.len() - 1];
        let r = b.rot.rotate(Vec3::new(rear.x, 0.0, 0.0));
        let vr = b.point_vel(r);
        let long = vr.dot(car.forward());
        let lat = vr.dot(car.right());
        phys.slip = kernel::atan2(lat, long.abs().max(0.5));
        if !phys.drifting && speed > 12.0 && phys.slip.abs() > DRIFT_ON {
            phys.drifting = true;
        }
        if phys.drifting && (phys.slip.abs() < DRIFT_OFF || speed < 7.0) {
            phys.drifting = false;
        }
        if phys.drifting {
            phys.drift_time += dt;
            phys.nitro = (phys.nitro
                + dt * 0.1
                    * clamp(phys.slip.abs() / 0.4, 0.3, 1.2)
                    * smoothstep(12.0, 30.0, speed))
            .min(1.0);
        } else {
            phys.drift_time = 0.0;
        }
        let sliding = tm.wheels.iter().map(|w| w.sliding).fold(0.0, f64::max);
        phys.skid = clamp((sliding - 0.6) / 0.4, 0.0, 1.0) * smoothstep(3.0, 8.0, speed);
        let g = ground.height(v.x, v.z);
        phys.off_track = Some(g.surface.loose);

        // Walls: scrape and impacts, as the arcade reports them.
        phys.scrape = (phys.scrape - dt * 4.0).max(0.0);
        let c = car.contact;
        if c.touching {
            let side = if v.lat > 0.0 { 1 } else { -1 };
            if speed > 1.0 {
                phys.scrape = (0.4 + speed / 60.0).min(1.0);
                phys.scrape_side = Some(side);
            }
            if c.impact > 1.5 {
                phys.events.push(PhysEvent::Impact {
                    strength: clamp(c.impact / 18.0, 0.0, 1.0),
                    x: c.point.x,
                    y: v.y + 0.5,
                    z: c.point.z,
                    side,
                });
            }
        }

        // Air and landings.
        if on_ground {
            if self.air > 0.0 {
                let impact = -b.vel.y;
                if impact > 2.5 {
                    phys.events.push(PhysEvent::Land {
                        strength: clamp(impact / 12.0, 0.0, 1.0),
                        air: self.air,
                    });
                }
            }
            self.air = 0.0;
        } else {
            self.air += dt;
        }
        phys.air_time = self.air;
        phys.on_ground = on_ground;
        phys.power_out = 0.0;
    }

    /// Appends the sim state's bits for the race's hash.
    pub fn hash_into(&self, out: &mut Vec<u8>) {
        self.car.hash_into(out);
        out.extend_from_slice(&self.air.to_bits().to_le_bytes());
    }
}

/// Switches player `p`'s car to the sim model ("Sim handling"), where it
/// stands. The client calls this after building the race when the setting
/// is on; `mp-sim race --sim` does the same.
pub fn use_sim(p: &mut crate::race::PlayerCar, t: &Track) {
    let def = sim_def(p.v.kind, p.spec.mass);
    p.model = VehicleModel::Sim(Box::new(SimCar::new(def, &p.v, t)));
}
