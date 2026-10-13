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

use mp_math::{clamp, kernel, smoothstep, wrap_angle};
use mp_track::Track;
use mp_vdyn::cars::vento_gt;
use mp_vdyn::{Controls, Ground, Quat, Vec3, VehicleDef};

use crate::ground::TrackGround;
use crate::input::{Input, InputFrame, SHIFT_DOWN, SHIFT_UP};
use crate::kinematic::Kinematic;
use crate::physics::{CarPhysics, PhysEvent, car_spec};
use crate::vehicle::{SimPose, Vehicle, WheelPose};

/// Which cars of a race the sim model moves (the "Handling" setting).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Physics {
    /// Every car on the arcade model, as the game has always been.
    #[default]
    Arcade,
    /// The player's car on the sim model, the rivals arcade.
    Player,
    /// The player's car and every rival on the sim model.
    All,
}

impl Physics {
    pub const ALL: [Physics; 3] = [Physics::Arcade, Physics::Player, Physics::All];

    /// The setting's and the address's word for it.
    pub fn name(self) -> &'static str {
        match self {
            Physics::Arcade => "arcade",
            Physics::Player => "player",
            Physics::All => "all",
        }
    }

    pub fn parse(s: &str) -> Option<Physics> {
        Physics::ALL.into_iter().find(|p| p.name() == s)
    }
}

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
/// Full steering asks the front tyres for this share of their peak force
/// (`SimCar::steer_max`).
const LOCK_FORCE: f64 = 0.9;

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

    /// Grip-scaled lock (the arcade's ANALOG_LOCK idea): full steering asks
    /// the front tyres for `LOCK_FORCE` of their peak at `speed`, so the
    /// whole travel steers and a tap does not throw the car into a slide.
    /// The wheel angle is the steady state's: the kinematic angle for that
    /// lateral acceleration plus the front axle's slip angle, less the
    /// rear's carrying its share (the brush curve inverted). The old lock
    /// added the full-slide slip angle (7.7°) at every speed, so at speed
    /// the first third of the stick already slid all four tyres. Radians.
    pub fn steer_max(&self, speed: f64) -> f64 {
        let def = &self.def;
        let loads = def.static_axle_loads();
        let v = speed.abs().max(1.0);
        let g = mp_vdyn::vehicle::G;
        // The slip angle at which an axle's tyres give `f` of their peak:
        // the brush force is `1 − (1−λ)³` of the peak at `λ`, and `λ = 1`
        // is the slip of the full slide, `3μFz/C`.
        let slip = |i: usize, f: f64| {
            let t = &def.axles[i].tyre;
            let fz = loads[i] / 2.0;
            let l = 1.0 - kernel::pow(1.0 - clamp(f, 0.0, 1.0), 1.0 / 3.0);
            kernel::atan(l * 3.0 * t.mu_at(fz) * fz / t.stiffness())
        };
        let ay = LOCK_FORCE * def.axles[0].tyre.mu_at(loads[0] / 2.0) * g;
        let f_rear = ay / (def.axles[1].tyre.mu_at(loads[1] / 2.0) * g);
        let want = def.wheelbase() * ay / (v * v) + slip(0, LOCK_FORCE) - slip(1, f_rear);
        clamp(want, 0.0, def.axles[0].steer_lock)
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
        let lock = self.def.axles[0].steer_lock;
        let steer_max = self.steer_max(speed);
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

/// Puts the cars `physics` names on the sim model, where they stand (on
/// the grid, when the race is built).
pub fn use_physics(st: &mut crate::race::SimState, t: &Track, physics: Physics) {
    if physics == Physics::Arcade {
        return;
    }
    for p in st.players.iter_mut() {
        use_sim(p, t);
    }
    if physics == Physics::All {
        for a in st.rivals.iter_mut() {
            a.sim = Some(Box::new(SimRival::new(&a.k.v, t)));
        }
    }
}

/// How far a sim rival's pace may fall below the profile before it counts
/// as stuck, and how long it may stay so before it is put back on the road
/// (s).
const STUCK_SPEED: f64 = 1.5;
/// A sim rival's wheel angle (rad) per rad/s of yaw rate it lacks.
const YAW_GAIN: f64 = 0.1;
const STUCK_SECS: f64 = 2.5;

/// A rival on the sim model: the AI picks its speed and line as it does for
/// a kinematic rival (`AiDriver::update`), and this drives a sim car to
/// them through the same *Casual* assists the player's goes through. The
/// rival's `Kinematic` stays its body view: `k.v` is what the sim writes,
/// and `k.s`, `k.lat`, `k.speed` and `k.lat_vel` are read back from it
/// for the drivers that look at it.
#[derive(Clone, Debug, PartialEq)]
pub struct SimRival {
    pub car: SimCar,
    /// The readouts the sim car writes (gear, rpm, drift, ...).
    pub phys: CarPhysics,
    /// Seconds spent stuck (`STUCK_SPEED`), or facing back up the road.
    stuck: f64,
    /// For the headless runs: wall impacts, and times put back on the road
    /// (not in the hash: nothing reads them).
    pub impacts: u32,
    pub resets: u32,
}

impl SimRival {
    pub fn new(v: &Vehicle, t: &Track) -> SimRival {
        let spec = car_spec(v.kind)
            .or_else(|| car_spec("sports"))
            .expect("a car in CAR_SPECS");
        SimRival {
            car: SimCar::new(sim_def(v.kind, v.mass), v, t),
            phys: CarPhysics::new(v, spec),
            stuck: 0.0,
            impacts: 0,
            resets: 0,
        }
    }

    /// One tick: steer for `lat_t` a little ahead, hold `v_t` (m/s), and
    /// write the body view back into `k`. Before the start the car is held
    /// on its brakes.
    #[allow(clippy::too_many_arguments)]
    pub fn drive(
        &mut self,
        k: &mut Kinematic,
        t: &Track,
        dt: f64,
        v_t: f64,
        lat_t: f64,
        nitro: bool,
        spiked: f64,
        started: bool,
    ) {
        let v = &k.v;
        let speed = v.speed;
        let mut inp = Input::default();
        if started {
            // Steering: at a point on the wanted line a little ahead.
            // The arc through it (pure pursuit) asks a wheel angle and a
            // yaw rate; the yaw rate's error is fed back, so a sliding car
            // is caught with opposite lock.
            let la = 7.0 + speed.abs() * 0.4;
            let p = t.point_at(k.s + la, lat_t);
            let dx = p.x - v.x;
            let dz = p.z - v.z;
            let want = kernel::atan2(dz, dx);
            let err = wrap_angle(want - v.yaw);
            let dist = (dx * dx + dz * dz).sqrt().max(1.0);
            let curv = 2.0 * kernel::sin(err) / dist;
            let wheel = self.car.def.wheelbase() * curv + YAW_GAIN * (speed * curv - v.yaw_rate);
            inp.steer = clamp(wheel / self.car.steer_max(speed), -1.0, 1.0);
            inp.analog = true;
            // Speed: throttle up to it, brake past it. To stand still, the
            // handbrake: the brake held at a standstill would reverse.
            let dv = v_t - speed;
            if v_t < 0.5 && speed < 1.0 {
                inp.handbrake = true;
            } else if dv > 0.0 {
                // Off the power while the rear steps out.
                let slide = clamp(1.0 - (self.phys.slip.abs() - 0.06) * 8.0, 0.2, 1.0);
                inp.throttle = clamp(dv * 0.5, 0.0, 1.0) * slide;
            } else if dv < -1.0 {
                inp.brake = clamp(-dv * 0.4, 0.0, 1.0);
            }
            inp.nitro = nitro;
        }
        self.phys.locked = !started;
        self.phys.nitro = if nitro { 1.0 } else { 0.0 };
        self.phys.spiked = spiked;
        self.car.update(
            &mut k.v,
            &mut self.phys,
            t,
            dt,
            &inp,
            &InputFrame::default(),
        );
        // Nobody listens for a rival's shifts and knocks.
        self.impacts += self
            .phys
            .events
            .iter()
            .filter(|e| matches!(e, PhysEvent::Impact { .. }))
            .count() as u32;
        self.phys.events.clear();
        read_back(k, t);

        // Stuck against something, or turned round: back on the road a
        // little behind, facing along it, as the player's reset puts them.
        let f = k.f;
        let facing = wrap_angle(k.v.yaw - kernel::atan2(f.fz, f.fx)).abs();
        if started && ((v_t > 4.0 && k.speed < STUCK_SPEED) || facing > 1.6) {
            self.stuck += dt;
        } else {
            self.stuck = 0.0;
        }
        if self.stuck > STUCK_SECS {
            self.stuck = 0.0;
            self.resets += 1;

            let s = if t.is_loop {
                t.wrap(k.s - 5.0)
            } else {
                (k.s - 5.0).max(t.start_s)
            };
            let hw = t.frame(s).hw;
            k.s = s;
            k.lat = clamp(lat_t, -hw * 0.5, hw * 0.5);
            k.speed = 0.0;
            k.lat_vel = 0.0;
            k.write_pos(t);
            k.v.yaw_rate = 0.0;
            self.car = SimCar::new(self.car.def.clone(), &k.v, t);
        }
    }

    /// Appends the sim state's bits for the race's hash.
    pub fn hash_into(&self, out: &mut Vec<u8>) {
        self.car.hash_into(out);
        out.extend_from_slice(&self.stuck.to_bits().to_le_bytes());
    }
}

/// A sim rival's track coordinates from its body view (after the sim's
/// step, and after the collision pass moved it).
pub fn read_back(k: &mut Kinematic, t: &Track) {
    let p = t.project(k.v.x, k.v.z, k.v.s);
    k.v.s = p.s;
    k.v.lat = p.lat;
    k.s = p.s;
    k.lat = p.lat;
    let f = k.frame(t);
    k.speed = k.v.vx * f.fx + k.v.vz * f.fz;
    k.lat_vel = k.v.vx * f.rx + k.v.vz * f.rz;
}
