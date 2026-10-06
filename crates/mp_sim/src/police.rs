//! A police unit (Hot Pursuit; port of `src/vehicles/PoliceDriver.js`).
//! Lives in track coordinates like the rivals and traffic, so walls, spin and
//! collision response come for free. The Pursuit decides who it chases and
//! which tactic it uses (`behaviour`, `slot`); this only drives it there.
//!
//! mode:
//!   parked     waiting on the shoulder, lights off (patrol)
//!   chase      after `target` using `behaviour`:
//!                chase  follow a few lengths back
//!                bump   drive into the target's rear bumper
//!                pit    line up on a rear quarter and push it sideways
//!                roll   get in front and brake gently (rolling block)
//!                box    hold a slot round the target (`slot`: ahead,
//!                       left, right, behind) at its speed minus a little
//!   oncoming   spawned ahead in the oncoming lane; U-turns once it passes
//!              its target
//!   search     lost the suspect: cruise to its last known s and weave
//!   hold       stopped beside a busted car, lights flashing
//!   standdown  off the chase: roll to a stop on the shoulder
//!   block      a roadblock or spike-strip car: parked at an angle, heavy
//!   disabled   taken down: rolls to a stop, smoking

use core::f64::consts::PI;

use mp_math::{Rng, clamp, damp, js, kernel, smoothstep};
use mp_track::{Frame, Track};

use crate::ai::{Seeker, find_block, pass_lat};
use crate::body::{AgentView, Body, BodyId};
use crate::kinematic::Kinematic;
use crate::vehicle::Vehicle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitType {
    Patrol,
    Interceptor,
    Suv,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitSpec {
    pub mass: f64,
    pub top: f64,
    pub power: f64,
    pub aggression: f64,
}

impl UnitType {
    pub fn spec(self) -> UnitSpec {
        match self {
            UnitType::Patrol => UnitSpec {
                mass: 1700.0,
                top: 0.88,
                power: 520.0,
                aggression: 0.4,
            },
            UnitType::Interceptor => UnitSpec {
                mass: 1550.0,
                top: 1.0,
                power: 640.0,
                aggression: 0.7,
            },
            UnitType::Suv => UnitSpec {
                mass: 2400.0,
                top: 0.93,
                power: 560.0,
                aggression: 1.0,
            },
        }
    }

    /// The model kind PursuitView builds for it.
    pub fn kind(self) -> &'static str {
        match self {
            UnitType::Patrol => "police",
            UnitType::Interceptor => "muscle",
            UnitType::Suv => "policeSuv",
        }
    }
}

/// Roadblock cars barely move when you hit them.
const BLOCK_MASS: f64 = 6000.0;

/// The trace's codes are the index in each list (trace-format.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Parked,
    Chase,
    Oncoming,
    Search,
    Standdown,
    Hold,
    Disabled,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    Chase,
    Bump,
    Pit,
    Roll,
    Box,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Ahead,
    Left,
    Right,
    Behind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Siren {
    Off,
    Flash,
    Disabled,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PoliceDriver {
    pub k: Kinematic,
    pub unit_type: UnitType,
    pub spec: UnitSpec,
    pub aggression: f64,
    pub active: bool,
    pub mode: Mode,
    pub behaviour: Behaviour,
    /// Time in the current behaviour.
    pub beh_t: f64,
    /// Box slot.
    pub slot: Option<Slot>,
    /// Which side of the target a PIT lines up on.
    pub pit_side: i32,
    pub pit_cooldown: f64,
    /// Missing until the first PIT line-up.
    pub pit_push: Option<f64>,
    /// The racer it chases (an index into the Pursuit's racers).
    pub target: Option<usize>,
    pub last_seen_s: f64,
    pub health: f64,
    pub disabled_t: f64,
    pub mode_t: f64,
    pub park_lat: f64,
    pub block_yaw: f64,
    /// Roadblock cars: where the gap is (rivals aim for it).
    pub gap_lat: Option<f64>,
    pub lane_lat: f64,
    pub siren: Siren,
    /// Top speed (m/s), set by the Pursuit from heat.
    pub cap: f64,
    pub weave: f64,
    /// Missing until the first swerve.
    pub avoid: Option<f64>,
    pub avoid_timer: Option<f64>,
    pub uturned: bool,
    pub retarget: Option<f64>,
    pub callsign: i32,
}

/// What a unit's update reads (`ctx`): every agent as it is now, its own
/// index there, and its target's body.
pub struct PoliceCtx<'a> {
    pub cars: &'a [AgentView],
    pub me: Option<usize>,
    pub target: Option<AgentView>,
}

impl PoliceDriver {
    /// `rng`: the weave's phase (the police stream, SPEC 4.3).
    pub fn new(vehicle: Vehicle, unit_type: UnitType, rng: &mut dyn Rng) -> PoliceDriver {
        let spec = unit_type.spec();
        PoliceDriver {
            k: Kinematic::new(vehicle),
            unit_type,
            spec,
            aggression: spec.aggression,
            active: false,
            mode: Mode::Parked,
            behaviour: Behaviour::Chase,
            beh_t: 0.0,
            slot: None,
            pit_side: 1,
            pit_cooldown: 0.0,
            pit_push: None,
            target: None,
            last_seen_s: 0.0,
            health: 1.0,
            disabled_t: 0.0,
            mode_t: 0.0,
            park_lat: 0.0,
            block_yaw: 0.0,
            gap_lat: None,
            lane_lat: 0.0,
            siren: Siren::Off,
            cap: 60.0,
            weave: rng.next_f64() * 10.0,
            avoid: None,
            avoid_timer: None,
            uturned: false,
            retarget: None,
            callsign: 0,
        }
    }

    pub fn mass(&self) -> f64 {
        if self.mode == Mode::Block {
            BLOCK_MASS
        } else {
            self.spec.mass
        }
    }

    /// Units that are actually on the chase (count toward the heat's limit).
    pub fn chasing(&self) -> bool {
        self.active && matches!(self.mode, Mode::Chase | Mode::Oncoming | Mode::Search)
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode == mode {
            return;
        }
        self.force_mode(mode);
    }

    /// `u.mode = null; u.setMode(mode)`: always a change.
    pub fn force_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.mode_t = 0.0;
        if mode == Mode::Chase {
            self.behaviour = Behaviour::Chase;
            self.beh_t = 0.0;
            self.slot = None;
        }
    }

    pub fn update(&mut self, t: &Track, dt: f64, ctx: &PoliceCtx) {
        let f = self.k.frame(t);
        self.mode_t += dt;
        self.beh_t += dt;
        self.pit_cooldown = js::max(0.0, self.pit_cooldown - dt);
        let my_w = self.k.v.half_w;
        let lim = js::min(f.wall_r, f.wall_l) - my_w - 0.35;
        let mut v_t: f64;
        let mut lat_t = self.k.lat;
        let acc = js::min(10.0, self.spec.power / js::max(self.k.speed, 5.0));
        let mut brake = 14.0;

        match self.mode {
            Mode::Block => {
                // Parked across the road at an angle; hits shove it a little.
                let k = &mut self.k;
                k.speed = damp(k.speed, 0.0, 5.0, dt);
                k.lat_vel = damp(k.lat_vel, 0.0, 5.0, dt);
                k.s += k.speed * k.dir as f64 * dt;
                k.lat += k.lat_vel * dt;
                k.spin_rate = damp(k.spin_rate, 0.0, 4.0, dt);
                self.block_yaw += self.k.spin_rate * dt * 0.3;
                self.k.spin = self.block_yaw;
                self.k.write_pos(t);
                return;
            }
            Mode::Parked => {
                v_t = 0.0;
                lat_t = self.park_lat;
                brake = 20.0;
            }
            Mode::Hold => {
                v_t = 0.0;
                lat_t = self.k.lat;
                brake = 9.0;
            }
            Mode::Standdown => {
                // Off the chase: pull onto the nearer shoulder and stop.
                let side = if self.k.lat >= 0.0 { 1.0 } else { -1.0 };
                v_t = js::max(0.0, self.k.speed - 6.0 * dt);
                lat_t = side
                    * js::max(
                        0.0,
                        (if side > 0.0 { f.wall_r } else { f.wall_l }) - my_w - 0.5,
                    );
                brake = 6.0;
            }
            Mode::Disabled => {
                v_t = 0.0;
                lat_t = self.k.lat;
                brake = 7.0;
                self.disabled_t += dt;
            }
            Mode::Oncoming => {
                // Head toward the suspect in the oncoming lane; once past it, flip
                // round in a cloud of tyre smoke and give chase.
                v_t = js::min(t.speed_profile[t.idx(self.k.s)] as f64 * 0.8, 26.0);
                lat_t = self.lane_lat;
                // Slow right down for anything coming at us in our lane.
                for (i, o) in ctx.cars.iter().enumerate() {
                    if Some(i) == ctx.me {
                        continue;
                    }
                    let ahead = t.ds(o.s, self.k.s); // > 0: o is in front of us (we drive toward lower s)
                    if ahead > 0.0
                        && ahead < 70.0
                        && (o.lat - self.k.lat).abs() < self.k.half_w() + o.half_w + 0.6
                    {
                        v_t = js::min(v_t, 4.0);
                    }
                }
                if let Some(tv) = &ctx.target
                    && t.ds(tv.s, self.k.s) < -4.0
                {
                    self.k.dir = 1;
                    self.k.speed = 3.0;
                    self.k.spin = PI; // the heading flips with dir; the spin unwinds it into a U-turn
                    self.k.spin_rate = 0.0;
                    self.uturned = true;
                    self.set_mode(Mode::Chase);
                }
            }
            Mode::Search => {
                // Cruise toward where the suspect was last seen, weaving across
                // the lanes to look down the side roads.
                let d = t.ds(self.k.s, self.last_seen_s);
                v_t = if d > -60.0 {
                    js::min(t.speed_profile[t.idx(self.k.s + 20.0)] as f64, 26.0)
                } else {
                    12.0
                };
                self.weave += dt;
                lat_t = kernel::sin(self.weave * 0.5) * lim * 0.7;
            }
            Mode::Chase => match &ctx.target {
                None => v_t = 20.0,
                Some(tv) => {
                    let (a, b) = self.drive_chase(t, tv, &f, lim);
                    v_t = a;
                    lat_t = b;
                }
            },
        }

        // Stay out of other cars' way (except whoever we're trying to hit).
        // Police avoid civilians, but an aggressive unit won't give up the
        // chase for one: it ploughs through.
        if matches!(self.mode, Mode::Chase | Mode::Search) {
            let target_id = ctx.target.map(|tv| tv.id);
            let boxing = self.behaviour == Behaviour::Box;
            let me = self.seeker();
            let skip = |i: usize| {
                let o = &ctx.cars[i];
                Some(o.id) == target_id || (o.police && boxing)
            };
            if let Some(bi) = find_block(t, &me, ctx.cars, ctx.me, lat_t, &skip) {
                let b = &ctx.cars[bi];
                match pass_lat(&me, b, &f) {
                    Some(choice) => {
                        self.avoid = Some(choice);
                        self.avoid_timer = Some(0.7);
                    }
                    None => {
                        if b.dir == 1 && !(b.kinematic_only && self.aggression > 0.6) {
                            v_t = js::min(v_t, b.speed_along - 0.5);
                        }
                    }
                }
            }
            if self.avoid_timer.is_some_and(|a| a > 0.0) {
                self.avoid_timer = Some(self.avoid_timer.unwrap() - dt);
                lat_t = self.avoid.unwrap_or(f64::NAN);
            }
        }
        if self.k.stunned > 0.0 {
            v_t *= 0.6;
        }
        lat_t = clamp(lat_t, -lim, lim);

        // Lateral controller: a little sharper than the rivals'. A PIT push
        // adds lateral speed on top.
        let lat_a = clamp((lat_t - self.k.lat) * 4.0 - self.k.lat_vel * 2.8, -9.0, 9.0);
        self.k.lat_vel += lat_a * dt;
        if self.mode == Mode::Chase
            && self.behaviour == Behaviour::Pit
            && let Some(p) = self.pit_push.filter(|&p| p != 0.0)
        {
            self.k.lat_vel += p * dt;
        }
        self.k.lat_vel = clamp(self.k.lat_vel, -8.0, 8.0);

        let dv = v_t - self.k.speed;
        self.k.speed += if dv > 0.0 {
            js::min(dv, acc * dt)
        } else {
            js::max(dv, -brake * dt)
        };
        self.k.speed = js::max(0.0, self.k.speed);
        self.k.v.brake_light = if dv < -1.5 { 1.0 } else { 0.0 };
        self.k.v.accel_long = if dv > 0.0 {
            acc * 0.6
        } else {
            -js::min(8.0, brake)
        };
        self.k.v.accel_lat = self.k.speed * self.k.speed * f.kappa;
        self.k.advance(t, dt);
    }

    pub fn seeker(&self) -> Seeker {
        Seeker {
            s: self.k.s,
            lat: self.k.lat,
            speed: self.k.speed,
            half_w: self.k.v.half_w,
        }
    }

    /// Speed and lane for a unit on the chase.
    fn drive_chase(&mut self, t: &Track, tv: &AgentView, _f: &Frame, lim: f64) -> (f64, f64) {
        let gap = t.ds(self.k.s, tv.s); // > 0: the target is ahead
        let contact = self.k.half_l() + tv.half_l;
        let look = clamp(self.k.speed * 0.5, 6.0, 40.0);
        let profile = js::min(
            t.speed_profile[t.idx(self.k.s)] as f64,
            t.speed_profile[t.idx(self.k.s + look)] as f64,
        );
        // Far behind: flat out (a little over the corner speeds the rivals
        // use, and +8 m/s over the heat's top speed), so the chase stays tense.
        let mut v_t = js::min(profile * 1.04, self.cap);
        if gap > 60.0 {
            v_t = js::min(profile * 1.12, self.cap) + 8.0 * smoothstep(60.0, 140.0, gap);
        }
        let mut lat_t = tv.lat;
        if gap > 60.0 || gap < -60.0 {
            return (
                if gap < -60.0 {
                    js::min(v_t, tv.speed_along * 0.8)
                } else {
                    v_t
                },
                lat_t,
            );
        }

        let my_hw = self.k.half_w();
        // A lane beside the target on the `want` side, or the other if that
        // one doesn't fit between the walls.
        let side = |want: f64| -> Option<f64> {
            let off = tv.half_w + my_hw + 0.5;
            let a = tv.lat + want * off;
            let b = tv.lat - want * off;
            if a.abs() < lim {
                Some(a)
            } else if b.abs() < lim {
                Some(b)
            } else {
                None
            }
        };
        let want; // the gap to hold (negative = in front)
        self.pit_push = Some(0.0);
        match self.behaviour {
            Behaviour::Bump => want = contact - 1.2,
            Behaviour::Pit => match side(self.pit_side as f64) {
                None => want = contact + 5.0,
                Some(l) => {
                    want = contact * 0.35;
                    lat_t = tv.lat + (l - tv.lat) * 0.55; // overlapping its rear quarter
                    // Alongside the rear quarter: steer into it.
                    if gap < contact * 0.9 && gap > -1.0 {
                        self.pit_push = Some(-js::sign(l - tv.lat) * 14.0 * self.aggression);
                    }
                }
            },
            Behaviour::Roll | Behaviour::Box => {
                let slot = if self.behaviour == Behaviour::Roll {
                    Slot::Ahead
                } else {
                    self.slot.unwrap_or(Slot::Behind)
                };
                match slot {
                    Slot::Ahead => {
                        want = -(contact + 3.0);
                        // Still behind it: go round first.
                        if gap > -contact {
                            lat_t = side(if tv.lat > self.k.lat { -1.0 } else { 1.0 })
                                .unwrap_or(tv.lat);
                        }
                    }
                    Slot::Left | Slot::Right => {
                        let l = side(if slot == Slot::Left { -1.0 } else { 1.0 });
                        want = if l.is_none() { contact + 1.5 } else { 0.0 };
                        lat_t = l.unwrap_or(tv.lat);
                    }
                    Slot::Behind => want = contact + 1.2,
                }
            }
            Behaviour::Chase => want = contact + 7.0,
        }
        // Hold the gap: target speed plus a correction, harder when aggressive.
        let push = if self.behaviour == Behaviour::Roll
            || (self.behaviour == Behaviour::Box && self.slot == Some(Slot::Ahead))
        {
            -4.0
        } else {
            0.0
        };
        let corr = clamp((gap - want) * 0.9, -12.0, 6.0 + self.aggression * 5.0);
        v_t = js::min(self.cap + 8.0, js::max(0.0, tv.speed_along + corr + push));
        // Don't take corners faster than the road allows while following.
        v_t = js::min(v_t, profile * 1.12 + 4.0);
        (v_t, lat_t)
    }
}

impl Body for PoliceDriver {
    fn v(&self) -> &Vehicle {
        &self.k.v
    }
    fn mass(&self) -> f64 {
        PoliceDriver::mass(self)
    }
    fn velocity(&self) -> (f64, f64) {
        self.k.velocity()
    }
    fn set_velocity(&mut self, vx: f64, vz: f64) {
        self.k.set_velocity(vx, vz)
    }
    fn translate(&mut self, t: &Track, dx: f64, dz: f64) {
        self.k.translate(t, dx, dz)
    }
    fn add_spin(&mut self, w: f64) {
        self.k.add_spin(w)
    }
    fn view(&self) -> AgentView {
        AgentView {
            id: BodyId::Anon,
            police: true,
            gap_lat: self.gap_lat,
            ..self.k.view()
        }
    }
}
