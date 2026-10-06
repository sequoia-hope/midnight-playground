//! Hot Pursuit (port of `src/game/Pursuit.js`; docs/hot-pursuit.md). Police
//! patrol a sprint race and chase whichever racer is nearest, the player
//! most of all: heat, line of sight, the pursuit / cooldown states, busts,
//! the unit pool and its tactics, roadblocks, spike strips and breakable
//! barriers. The race feeds it racers and collisions and turns its events
//! into sound, HUD and effects.
//!
//! Getting busted or wrecked doesn't end a race. The car is held on the spot
//! for a penalty (the race clock and the rivals keep going), then released:
//! the race resets it onto the road ahead of the police that caught it, and
//! they stand down.
//!
//! The JS Pursuit holds its racers' bodies; here it reaches them through the
//! [`Racers`] trait, and a racer's identity is its index in `racers`.

use core::f64::consts::PI;

use mp_math::{Rng, clamp, js, kernel, rrange, smoothstep};
use mp_track::{Level, ROAD_TYPES, Track};

use crate::body::{AgentView, Body, BodyId};
use crate::dims::dims;
use crate::kinematic::{Kinematic, Surface};
use crate::physics::CarSpec;
use crate::police::{Behaviour, Mode, PoliceCtx, PoliceDriver, Siren, Slot, UnitType};
use crate::vehicle::Vehicle;

/// Heat: max active units, the unit mix, and a top-speed factor relative to
/// the player's car (at heat 5 the fastest units edge you on a straight).
pub struct HeatLevel {
    pub units: usize,
    pub mix: &'static [UnitType],
    pub speed: f64,
}

use UnitType::{Interceptor, Patrol, Suv};
pub const HEAT: [HeatLevel; 6] = [
    HeatLevel {
        units: 0,
        mix: &[],
        speed: 0.0,
    },
    HeatLevel {
        units: 2,
        mix: &[Patrol],
        speed: 0.92,
    },
    HeatLevel {
        units: 3,
        mix: &[Patrol, Patrol, Interceptor],
        speed: 0.97,
    },
    HeatLevel {
        units: 4,
        mix: &[Patrol, Interceptor, Interceptor],
        speed: 1.0,
    },
    HeatLevel {
        units: 5,
        mix: &[Patrol, Interceptor, Suv],
        speed: 1.03,
    },
    HeatLevel {
        units: 6,
        mix: &[Interceptor, Suv, Suv, Patrol],
        speed: 1.06,
    },
];
/// Seconds out of sight to escape, by heat.
pub const EVADE_TIME: [f64; 6] = [0.0, 8.0, 11.0, 14.0, 18.0, 22.0];
pub mod heat_gain {
    pub const SECOND: f64 = 0.02;
    pub const HIT_COP: f64 = 0.15;
    pub const TAKEDOWN: f64 = 0.35;
    pub const DODGE: f64 = 0.2;
    pub const HIT_CIVILIAN: f64 = 0.05;
}
/// Metres along the road.
pub const LOS_RANGE: f64 = 300.0;
/// Total road turn that hides you.
pub const LOS_TURN: f64 = 70.0 * PI / 180.0;
pub mod bust {
    pub const SPEED: f64 = 6.0;
    pub const RADIUS: f64 = 9.0;
    pub const RATE: f64 = 0.45;
    pub const EXTRA: f64 = 0.25;
    pub const DRAIN: f64 = 0.8;
}
/// Seconds held, 5.5–11.5.
pub fn bust_penalty(heat: i32) -> f64 {
    4.0 + heat as f64 * 1.5
}
pub const WRECK_PENALTY: f64 = 8.0;
/// Seconds after a release before police can bust you again.
pub const GRACE: f64 = 8.0;
/// A parked unit notices racers this close.
const SPOT_RANGE: f64 = 120.0;
/// Seconds between units joining from behind.
const SPAWN_GAP: f64 = 4.0;
/// Seconds between target choices.
const RETARGET: f64 = 1.0;
const MIN_BEHAVIOUR: f64 = 1.5;
/// Chasers (at most 6 active), in the JS's insertion order.
const POOL: [(UnitType, usize); 3] = [(Patrol, 3), (Interceptor, 2), (Suv, 2)];
const BLOCK_POOL: [UnitType; 5] = [Patrol, Patrol, Patrol, Suv, Suv];
const SAWHORSES: usize = 2;

/// Radio callsigns: unit i answers to 10 + 3i, give or take two, so every
/// pursuit's numbers differ but never collide.
pub fn callsign(i: usize, rng: &mut dyn Rng) -> i32 {
    10 + i as i32 * 3 + (rng.next_f64() * 3.0).floor() as i32
}

/// Every callsign there is (`CALLSIGNS`): 10 up to 10 + three per unit of
/// the chase pool, less one.
pub const CALLSIGNS: [i32; POOL_UNITS * 3] = {
    let mut a = [0; POOL_UNITS * 3];
    let mut k = 0;
    while k < a.len() {
        a[k] = 10 + k as i32;
        k += 1;
    }
    a
};

/// The chase pool's size (`Object.values(POOL).reduce((a, b) => a + b)`).
const POOL_UNITS: usize = {
    let mut n = 0;
    let mut i = 0;
    while i < POOL.len() {
        n += POOL[i].1;
        i += 1;
    }
    n
};

/// Top speed of a player car spec on the flat: where drive = drag.
pub fn top_speed(spec: &CarSpec) -> f64 {
    let mut v = 40.0;
    for _ in 0..60 {
        let mut a = js::min(spec.launch.unwrap_or(9.5), spec.power / v);
        if let Some(vmax) = spec.vmax.filter(|&m| m != 0.0) {
            a *= 1.0 - smoothstep(vmax - 5.0, vmax, v);
        }
        let drag = 0.00115 * v * v + 0.01 * v;
        v += clamp((a - drag) * 2.0, -2.0, 2.0);
    }
    v
}

/// A breakable sawhorse barrier (heavy roadblocks fill their gap with two).
#[derive(Clone, Debug, PartialEq)]
pub struct Sawhorse {
    pub k: Kinematic,
    pub active: bool,
    pub broken: bool,
    pub h: f64,
    pub vy: f64,
    pub age: f64,
    pub gap_lat: Option<f64>,
}

impl Sawhorse {
    fn write_pos(&mut self, t: &Track) {
        self.k.surface = Surface::Raised(self.h);
        self.k.write_pos(t);
    }
}

impl Body for Sawhorse {
    fn v(&self) -> &Vehicle {
        &self.k.v
    }
    fn mass(&self) -> f64 {
        60.0
    }
    fn crashy(&self) -> bool {
        true
    }
    fn velocity(&self) -> (f64, f64) {
        self.k.velocity()
    }
    fn set_velocity(&mut self, vx: f64, vz: f64) {
        self.k.set_velocity(vx, vz)
    }
    fn translate(&mut self, t: &Track, dx: f64, dz: f64) {
        let f = &self.k.f;
        self.k.s += dx * f.fx + dz * f.fz;
        self.k.lat += dx * f.rx + dz * f.rz;
        self.write_pos(t);
    }
    fn add_spin(&mut self, w: f64) {
        self.k.add_spin(w)
    }
    fn view(&self) -> AgentView {
        AgentView {
            gap_lat: self.gap_lat,
            ..self.k.view()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Patrol,
    Pursuit,
    Cooldown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldReason {
    Busted,
    Wrecked,
}

/// A racer: the player or a rival, and the pursuit's bookkeeping on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Racer {
    pub id: BodyId,
    pub player: bool,
    /// A rival (`r.ai`).
    pub ai: bool,
    pub name: &'static str,
    pub bust: f64,
    pub hold: f64,
    pub hold_total: f64,
    pub hold_reason: Option<HoldReason>,
    pub grace: f64,
    pub finished: bool,
    pub prev_s: Option<f64>,
}

/// What the pursuit reads of a racer's body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RacerBody {
    pub s: f64,
    pub lat: f64,
    pub speed_along: f64,
    pub half_w: f64,
    pub half_l: f64,
    pub x: f64,
    pub z: f64,
    pub vx: f64,
    pub vz: f64,
}

impl RacerBody {
    pub fn view(&self, id: BodyId) -> AgentView {
        AgentView {
            id,
            police: false,
            kinematic_only: false,
            s: self.s,
            lat: self.lat,
            dir: 1,
            half_w: self.half_w,
            half_l: self.half_l,
            speed_along: self.speed_along,
            gap_lat: None,
        }
    }
}

/// The racers' bodies, as the pursuit reaches them.
pub trait Racers {
    fn body(&self, id: BodyId) -> RacerBody;
    /// A rival's own finish (`r.ai.finished`).
    fn ai_finished(&self, id: BodyId) -> bool;
    /// A busted or wrecked rival pulls over: `ai.hold`, `ai.holdLat`.
    fn hold_ai(&mut self, id: BodyId, seconds: f64, hold_lat: f64);
    /// Shredded tyres: the player's `phys.spiked` or a rival's `spiked`.
    fn spike(&mut self, id: BodyId);
}

#[derive(Clone, Debug, PartialEq)]
pub struct Roadblock {
    pub s: f64,
    /// Indices into `block_cars`.
    pub cars: Vec<usize>,
    pub gap_lat: f64,
    pub heavy: bool,
    pub passed: bool,
    pub touched: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spikes {
    pub s: f64,
    pub lat0: f64,
    pub lat1: f64,
    /// Index into `block_cars`.
    pub car: usize,
    /// Per racer: hit by the strip.
    pub hit: Vec<bool>,
    pub passed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub s: f64,
    pub side: f64,
    pub lat: Option<f64>,
    pub used: bool,
}

/// What the pursuit tells the race (`pu.events`).
#[derive(Clone, Debug, PartialEq)]
pub enum PursuitEvent {
    Pursuit {
        heat: i32,
    },
    Reacquired {
        heat: i32,
    },
    Cooldown,
    Escaped {
        heat: i32,
    },
    Heat {
        heat: i32,
    },
    Spotted {
        unit: usize,
        racer: usize,
        player: bool,
    },
    Join {
        unit: usize,
    },
    Uturn {
        unit: usize,
        x: f64,
        z: f64,
    },
    Takedown {
        unit: usize,
        by_player: bool,
        x: f64,
        z: f64,
    },
    Roadblock {
        s: f64,
        heavy: bool,
    },
    Spikes {
        s: f64,
    },
    Spiked {
        racer: usize,
        player: bool,
        name: &'static str,
    },
    Dodge {
        spikes: bool,
    },
    Barrier {
        x: f64,
        z: f64,
        player: bool,
    },
    Busted {
        racer: usize,
        seconds: f64,
        player: bool,
        name: &'static str,
    },
    Wrecked {
        racer: usize,
        seconds: f64,
        player: bool,
        name: &'static str,
    },
    /// A held racer is free; for the player, where to put the car back.
    Release {
        racer: usize,
        player: bool,
        spot: Option<(f64, f64)>,
    },
}

/// One entry of the agent list a unit reads: one of the pursuit's own bodies
/// (read live) or another body as it is.
#[derive(Clone, Copy, Debug)]
pub enum PAgent {
    Police(usize),
    Sawhorse(usize),
    Other(AgentView),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pursuit {
    pub flash: bool,
    pub heat_cap: Vec<i32>,
    pub open_ground: Vec<bool>,
    pub player_top: f64,
    pub max_units: usize,
    pub heat: i32,
    pub max_heat: i32,
    pub heat_meter: f64,
    pub state: State,
    /// The player's bust meter.
    pub bust: f64,
    pub evade: f64,
    pub events: Vec<PursuitEvent>,
    pub time: f64,
    pub spawn_t: f64,
    pub prop_t: f64,
    pub patrol_t: Option<f64>,
    pub takedowns: i32,
    pub busts: i32,
    pub racers: Vec<Racer>,
    /// The player's index in `racers`.
    pub player: Option<usize>,
    /// Cumulative road turn (radians) per metre, f32 as in the JS.
    pub cum_turn: Vec<f32>,
    pub tunnels: Vec<(f64, f64)>,
    pub units: Vec<PoliceDriver>,
    pub block_cars: Vec<PoliceDriver>,
    pub sawhorses: Vec<Sawhorse>,
    pub roadblock: Option<Roadblock>,
    pub spikes: Option<Spikes>,
    pub spots: Vec<Spot>,
    /// The level's traffic speeds by zone (`spotSpeed`) and onc oming share.
    traffic: Vec<(f64, Option<f64>)>,
}

/// Options of `new Pursuit({...})`.
pub struct PursuitOpts {
    pub heat: f64,
    pub max_units: f64,
    pub player_top: f64,
    pub flash: bool,
}

impl Pursuit {
    /// `rng` is the pursuit stream, `police_rng` the police stream (the
    /// weave), SPEC 4.3.
    pub fn new(
        t: &Track,
        level: &Level,
        o: PursuitOpts,
        rng: &mut dyn Rng,
        police_rng: &mut dyn Rng,
    ) -> Pursuit {
        let p = level.police.as_ref();
        let heat_cap = (0..t.zones.len())
            .map(|i| p.and_then(|p| p.heat_cap.get(i)).map_or(5, |&c| c as i32))
            .collect();
        let open_ground = (0..t.zones.len())
            .map(|i| {
                p.and_then(|p| p.los_open_ground.get(i))
                    .copied()
                    .unwrap_or(false)
            })
            .collect();
        let heat = clamp(js::round(o.heat), 1.0, 5.0) as i32;
        // Cumulative road turn (radians) per metre: line of sight round a bend
        // is a subtraction.
        let n = t.n;
        let mut cum = vec![0f32; n + 1];
        for i in 0..n {
            cum[i + 1] = (cum[i] as f64 + (t.k_smooth[i] as f64).abs()) as f32;
        }
        let tunnels = t
            .tags
            .iter()
            .filter(|g| g.tag == "tunnel")
            .map(|g| (g.s0, g.s1))
            .collect();

        let unit_vehicle = |ty: UnitType| {
            let kind = ty.kind();
            Vehicle::new(dims(kind).expect("dims"), kind, ty.spec().mass, "Police", 0)
        };
        let mut units = Vec::new();
        for (ty, k) in POOL {
            for _ in 0..k {
                let mut u = PoliceDriver::new(unit_vehicle(ty), ty, police_rng);
                u.callsign = callsign(units.len(), rng);
                units.push(u);
            }
        }
        let block_cars = BLOCK_POOL
            .iter()
            .map(|&ty| PoliceDriver::new(unit_vehicle(ty), ty, police_rng))
            .collect();
        let sawhorses = (0..SAWHORSES)
            .map(|_| Sawhorse {
                k: Kinematic::new(Vehicle::new(
                    dims("sawhorse").unwrap(),
                    "sawhorse",
                    60.0,
                    "",
                    0,
                )),
                active: false,
                broken: false,
                h: 0.0,
                vy: 0.0,
                age: 0.0,
                gap_lat: None,
            })
            .collect();
        let traffic = level
            .traffic
            .iter()
            .map(|r| (r.speed[1], Some(r.oncoming)))
            .collect();
        let mut pu = Pursuit {
            flash: o.flash,
            heat_cap,
            open_ground,
            player_top: o.player_top,
            max_units: clamp(o.max_units, 0.0, 6.0) as usize,
            heat,
            max_heat: heat,
            heat_meter: 0.0,
            state: State::Patrol,
            bust: 0.0,
            evade: 0.0,
            events: Vec::new(),
            time: 0.0,
            spawn_t: 0.0,
            prop_t: 12.0, // no roadblock in the first seconds of a pursuit
            patrol_t: None,
            takedowns: 0,
            busts: 0,
            racers: Vec::new(),
            player: None,
            cum_turn: cum,
            tunnels,
            units,
            block_cars,
            sawhorses,
            roadblock: None,
            spikes: None,
            spots: Vec::new(),
            traffic,
        };
        // Parked patrol cars along the route.
        pu.spots = pu.auto_spots(t, rng);
        pu
    }

    fn auto_spots(&self, t: &Track, rng: &mut dyn Rng) -> Vec<Spot> {
        let mut out = Vec::new();
        let mut s = t.start_s + 650.0;
        let mut side = 1.0;
        while s < t.finish_s - 350.0 {
            // Somewhere fairly straight so it's seen before it's passed.
            let (mut best, mut best_k) = (s, f64::INFINITY);
            let mut d = 0.0;
            while d < 200.0 {
                let k = (t.k_smooth[t.idx(s + d)] as f64).abs();
                if k < best_k {
                    best_k = k;
                    best = s + d;
                }
                d += 20.0;
            }
            out.push(Spot {
                s: best,
                side,
                lat: None,
                used: false,
            });
            side = -side;
            s = best + rrange(rng, 700.0, 1100.0);
        }
        out
    }

    /// `setRacers`: the player and the rivals, in that order in the race.
    pub fn set_racers(&mut self, list: Vec<(BodyId, bool, bool, &'static str)>) {
        self.racers = list
            .into_iter()
            .map(|(id, player, ai, name)| Racer {
                id,
                player,
                ai,
                name,
                bust: 0.0,
                hold: 0.0,
                hold_total: 0.0,
                hold_reason: None,
                grace: 0.0,
                finished: false,
                prev_s: None,
            })
            .collect();
        self.player = self.racers.iter().position(|r| r.player);
        if let Some(s) = &mut self.spikes {
            s.hit = vec![false; self.racers.len()];
        }
    }

    /// The racer whose body this is (`racerFor`).
    pub fn racer_for(&self, id: BodyId) -> Option<usize> {
        self.racers.iter().position(|r| r.id == id)
    }

    /// The body id of police car `i` (units, then block cars).
    pub fn police_id(i: usize) -> BodyId {
        BodyId::Police(i)
    }

    pub fn police(&self, i: usize) -> &PoliceDriver {
        if i < self.units.len() {
            &self.units[i]
        } else {
            &self.block_cars[i - self.units.len()]
        }
    }

    pub fn police_mut(&mut self, i: usize) -> &mut PoliceDriver {
        let n = self.units.len();
        if i < n {
            &mut self.units[i]
        } else {
            &mut self.block_cars[i - n]
        }
    }

    /// Everything that collides: active units, the roadblock and barriers.
    pub fn bodies(&self) -> Vec<PAgent> {
        let n = self.units.len();
        let mut out = Vec::new();
        out.extend((0..n).filter(|&i| self.units[i].active).map(PAgent::Police));
        out.extend(
            (0..self.block_cars.len())
                .filter(|&i| self.block_cars[i].active)
                .map(|i| PAgent::Police(n + i)),
        );
        out.extend(
            (0..self.sawhorses.len())
                .filter(|&i| self.sawhorses[i].active && !self.sawhorses[i].broken)
                .map(PAgent::Sawhorse),
        );
        out
    }

    /// After collisions: pin the bodies to their track positions
    /// (`for (const u of pu.bodies()) u.writePos()`).
    pub fn write_pos(&mut self, t: &Track) {
        for a in self.bodies() {
            match a {
                PAgent::Police(i) => self.police_mut(i).k.write_pos(t),
                PAgent::Sawhorse(i) => self.sawhorses[i].write_pos(t),
                PAgent::Other(_) => {}
            }
        }
    }

    pub fn view_of(&self, a: &PAgent) -> AgentView {
        match *a {
            PAgent::Police(i) => AgentView {
                id: BodyId::Police(i),
                ..self.police(i).view()
            },
            PAgent::Sawhorse(i) => AgentView {
                id: BodyId::Sawhorse(i),
                ..self.sawhorses[i].view()
            },
            PAgent::Other(v) => v,
        }
    }

    fn zone_at(&self, t: &Track, s: f64) -> usize {
        t.zone[t.idx(s)] as usize
    }

    fn cap_at(&self, t: &Track, s: f64) -> i32 {
        self.heat_cap[self.zone_at(t, s)]
    }

    fn in_tunnel(&self, s: f64) -> Option<usize> {
        self.tunnels
            .iter()
            .position(|&(s0, s1)| s > s0 - 5.0 && s < s1 + 5.0)
    }

    /// Can a unit at s1 see a car at s2? Along-track range, tunnels, and how
    /// far the road turns between them (canyon walls and city blocks hide you
    /// round a corner; open ground doesn't).
    pub fn can_see(&self, t: &Track, s1: f64, s2: f64) -> bool {
        let d = t.ds(s1, s2);
        if d.abs() > LOS_RANGE {
            return false;
        }
        let (g1, g2) = (self.in_tunnel(s1), self.in_tunnel(s2));
        if (g1.is_some() || g2.is_some()) && g1 != g2 {
            return false;
        }
        if self.open_ground[self.zone_at(t, s1)] && self.open_ground[self.zone_at(t, s2)] {
            return true;
        }
        let a = js::min(s1, s1 + d);
        let b = js::max(s1, s1 + d);
        let n = t.n as f64;
        let ia = clamp(js::round(a), 0.0, n) as usize;
        let ib = clamp(js::round(b), 0.0, n) as usize;
        (self.cum_turn[ib] as f64 - self.cum_turn[ia] as f64) < LOS_TURN
    }

    fn emit(&mut self, e: PursuitEvent) {
        self.events.push(e);
    }

    /// A racer is caught (busted) or wrecked: held for `seconds`.
    pub fn arrest(
        &mut self,
        t: &Track,
        racers: &mut dyn Racers,
        ri: usize,
        reason: HoldReason,
        seconds: f64,
    ) {
        if self.racers[ri].hold > 0.0 {
            return;
        }
        {
            let r = &mut self.racers[ri];
            r.hold = seconds;
            r.hold_total = seconds;
            r.hold_reason = Some(reason);
            r.bust = 0.0;
        }
        let r = self.racers[ri].clone();
        let rb = racers.body(r.id);
        if r.ai {
            let lat = shoulder_lat(t, rb.s, rb.lat, rb.half_w);
            racers.hold_ai(r.id, seconds, lat);
        }
        // The units on its tail stop beside it; the rest drop the chase.
        for u in &mut self.units {
            if !u.active || u.target != Some(ri) {
                continue;
            }
            let near = t.ds(u.k.s, rb.s).abs() < 60.0;
            u.set_mode(if near { Mode::Hold } else { Mode::Standdown });
            u.target = if near { Some(ri) } else { None };
        }
        if r.player {
            if reason == HoldReason::Busted {
                self.busts += 1;
            }
            self.bust = 0.0;
            self.evade = 0.0;
            if self.state != State::Patrol {
                self.state = State::Patrol;
                self.heat = (self.heat - 1).max(1);
                self.heat_meter = 0.0;
            }
            self.clear_props(t, racers);
        }
        let e = match reason {
            HoldReason::Busted => PursuitEvent::Busted {
                racer: ri,
                seconds,
                player: r.player,
                name: r.name,
            },
            HoldReason::Wrecked => PursuitEvent::Wrecked {
                racer: ri,
                seconds,
                player: r.player,
                name: r.name,
            },
        };
        self.emit(e);
    }

    /// Where to put a released car back on the road: just ahead of any unit
    /// around it, in the middle of the road.
    fn release_spot(&self, t: &Track, rb: &RacerBody) -> (f64, f64) {
        let s0 = rb.s;
        let mut s = s0;
        for u in &self.units {
            if u.active && t.ds(s0, u.k.s).abs() < 40.0 {
                s = js::max(s, s0 + t.ds(s0, u.k.s) + u.k.half_l() + 6.0);
            }
        }
        if let Some(rbk) = &self.roadblock {
            for &c in &rbk.cars {
                let cs = self.block_cars[c].k.s;
                if t.ds(s0, cs).abs() < 30.0 {
                    s = js::max(s, s0 + t.ds(s0, cs) + 8.0);
                }
            }
        }
        s = if t.is_loop {
            t.wrap(s)
        } else {
            js::min(s, t.road_end() - 5.0)
        };
        let f = t.frame(s);
        (s, clamp(rb.lat, -f.hw * 0.4, f.hw * 0.4))
    }

    /// A car-to-car hit: units take damage from every hit; the player taking
    /// one out is a takedown. Returns a PIT push for the player's car, if any.
    /// `a`/`b` are the bodies' ids, `vel_a`/`vel_b` their velocities after
    /// the collision pass, `player_lat` the player's lat.
    #[allow(clippy::too_many_arguments)]
    pub fn on_hit(
        &mut self,
        t: &Track,
        racers: &mut dyn Racers,
        a: BodyId,
        b: BodyId,
        strength: f64,
        vel_a: (f64, f64),
        vel_b: (f64, f64),
        player_lat: f64,
        rng: &mut dyn Rng,
    ) -> f64 {
        let mut pit = 0.0;
        let pb = self.player.map(|p| self.racers[p].id);
        for (me, other, other_vel) in [(a, b, vel_b), (b, a, vel_a)] {
            if let BodyId::Sawhorse(i) = me
                && !self.sawhorses[i].broken
            {
                self.break_sawhorse(i, other, other_vel, rng);
            }
            let BodyId::Police(pi) = me else { continue };
            {
                let u = self.police(pi);
                if !u.active || u.mode == Mode::Disabled {
                    continue;
                }
                if u.mode == Mode::Block {
                    if Some(other) == pb && strength > 0.2 {
                        self.heat_up(t, racers, heat_gain::HIT_COP);
                    }
                    continue;
                }
            }
            // Resting contact (pushing, boxing in) doesn't wear a unit down; real
            // hits do, less so the ones it braced for.
            // Taking units out is the player's game: other cars do half damage.
            let by_player = Some(other) == pb;
            if strength > 0.08 {
                let u = self.police_mut(pi);
                let braced = if matches!(u.behaviour, Behaviour::Bump | Behaviour::Pit) {
                    0.5
                } else {
                    1.0
                };
                u.health -= strength
                    * (1.4 - u.spec.mass / 3000.0)
                    * braced
                    * (if by_player { 1.0 } else { 0.5 });
            }
            if by_player {
                if strength > 0.2 {
                    self.heat_up(t, racers, heat_gain::HIT_COP);
                }
                let pidx = self.player;
                let u = self.police_mut(pi);
                if u.mode == Mode::Chase
                    && u.behaviour == Behaviour::Pit
                    && u.pit_cooldown == 0.0
                    && strength > 0.02
                {
                    pit = -js::sign(player_lat - u.k.lat) * u.aggression; // the rear goes away from the unit
                    u.pit_cooldown = 6.0;
                    u.behaviour = Behaviour::Chase;
                    u.beh_t = 0.0;
                }
                if u.target != pidx && u.mode != Mode::Hold {
                    u.target = pidx;
                    u.set_mode(Mode::Chase);
                }
            }
            if self.police(pi).health <= 0.0 {
                let u = self.police_mut(pi);
                u.set_mode(Mode::Disabled);
                u.target = None;
                u.siren = Siren::Flash;
                let (x, z) = (u.k.v.x, u.k.v.z);
                if by_player {
                    self.takedowns += 1;
                    self.heat_up(t, racers, heat_gain::TAKEDOWN);
                }
                self.emit(PursuitEvent::Takedown {
                    unit: pi,
                    by_player,
                    x,
                    z,
                });
            }
        }
        // Hitting a civilian while the police are watching.
        if let Some(pb) = pb
            && (a == pb || b == pb)
        {
            let o = if a == pb { b } else { a };
            if matches!(o, BodyId::Traffic(_)) && self.state == State::Pursuit && strength > 0.15 {
                self.heat_up(t, racers, heat_gain::HIT_CIVILIAN);
            }
        }
        pit
    }

    fn break_sawhorse(&mut self, i: usize, by: BodyId, by_vel: (f64, f64), rng: &mut dyn Rng) {
        let pb = self.player.map(|p| self.racers[p].id);
        let b = &mut self.sawhorses[i];
        b.broken = true;
        b.age = 0.0;
        b.vy = 4.0 + rng.next_f64() * 3.0;
        let (vx, vz) = by_vel;
        b.k.set_velocity(vx * 1.1, vz * 1.1);
        b.k.spin_rate = (rng.next_f64() - 0.5) * 16.0;
        let player = Some(by) == pb;
        let (x, z) = (b.k.v.x, b.k.v.z);
        if player && let Some(rb) = &mut self.roadblock {
            rb.touched = true;
        }
        self.emit(PursuitEvent::Barrier { x, z, player });
    }

    pub fn heat_up(&mut self, t: &Track, racers: &dyn Racers, k: f64) {
        if self.state == State::Patrol {
            return;
        }
        let cap = match self.player {
            Some(p) => self.cap_at(t, racers.body(self.racers[p].id).s),
            None => 5,
        };
        self.heat_meter += k;
        while self.heat_meter >= 1.0 {
            if self.heat >= cap {
                self.heat_meter = 0.999;
                break;
            }
            self.heat_meter -= 1.0;
            self.heat += 1;
            self.max_heat = self.max_heat.max(self.heat);
            self.emit(PursuitEvent::Heat { heat: self.heat });
        }
    }

    /// `update(dt, { cars, time, started })`. `cars` is the agent list as
    /// the race built it this tick.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        t: &Track,
        dt: f64,
        cars: &[PAgent],
        racers: &mut dyn Racers,
        time: f64,
        started: bool,
        rng: &mut dyn Rng,
    ) {
        self.time = time;
        if !started {
            for u in &mut self.units {
                if u.active {
                    u.k.write_pos(t);
                }
            }
            return;
        }
        for ri in 0..self.racers.len() {
            let r = &self.racers[ri];
            let fin = if r.player {
                racers.body(r.id).s >= t.finish_s
            } else {
                r.ai && racers.ai_finished(r.id)
            };
            if !r.finished && fin {
                self.on_finished(t, racers, ri);
            }
            let r = &mut self.racers[ri];
            r.grace = js::max(0.0, r.grace - dt);
            if r.hold > 0.0 {
                r.hold -= dt;
                if r.hold <= 0.0 {
                    self.release(t, racers, ri);
                }
            }
        }

        self.activate_spots(t, racers);
        self.pick_targets(t, racers, dt);
        self.update_state(t, racers, dt);
        self.spawn(t, racers, dt, rng);
        self.assign_behaviours(t, racers, rng);
        self.bust_meters(t, racers, dt);

        let cap = self.player_top * HEAT[self.heat as usize].speed;
        for i in 0..self.units.len() {
            if !self.units[i].active {
                continue;
            }
            self.units[i].cap = cap * self.units[i].spec.top;
            self.update_police(t, dt, cars, racers, i);
            let u = &mut self.units[i];
            u.siren = match u.mode {
                Mode::Parked => Siren::Off,
                Mode::Disabled => {
                    if u.disabled_t > 3.0 {
                        Siren::Disabled
                    } else {
                        Siren::Flash
                    }
                }
                _ => Siren::Flash,
            };
            if u.uturned {
                u.uturned = false;
                let (x, z) = (u.k.v.x, u.k.v.z);
                self.emit(PursuitEvent::Uturn { unit: i, x, z });
            }
        }
        let n = self.units.len();
        for i in 0..self.block_cars.len() {
            if self.block_cars[i].active {
                self.update_police(t, dt, cars, racers, n + i);
            }
        }
        for i in 0..self.sawhorses.len() {
            if self.sawhorses[i].active {
                self.update_sawhorse(t, i, dt);
            }
        }
        self.props(t, racers, dt, rng);
        self.recycle(t, racers);
    }

    /// One police car's update, with the agent list read as it is now.
    fn update_police(
        &mut self,
        t: &Track,
        dt: f64,
        cars: &[PAgent],
        racers: &dyn Racers,
        pi: usize,
    ) {
        let views: Vec<AgentView> = cars.iter().map(|a| self.view_of(a)).collect();
        let me = views.iter().position(|v| v.id == BodyId::Police(pi));
        let target = self.police(pi).target.map(|ri| {
            let id = self.racers[ri].id;
            racers.body(id).view(id)
        });
        let ctx = PoliceCtx {
            cars: &views,
            me,
            target,
        };
        self.police_mut(pi).update(t, dt, &ctx);
    }

    fn on_finished(&mut self, t: &Track, racers: &dyn Racers, ri: usize) {
        // The finish line is a safe zone: everyone on its tail breaks off.
        self.racers[ri].finished = true;
        self.racers[ri].bust = 0.0;
        for u in &mut self.units {
            if u.active && u.target == Some(ri) && u.mode != Mode::Hold {
                u.set_mode(Mode::Standdown);
                u.target = None;
            }
        }
        if self.racers[ri].player {
            self.state = State::Patrol;
            self.bust = 0.0;
            self.evade = 0.0;
            self.clear_props(t, racers);
        }
    }

    fn release(&mut self, t: &Track, racers: &dyn Racers, ri: usize) {
        self.racers[ri].hold = 0.0;
        self.racers[ri].grace = GRACE;
        for u in &mut self.units {
            if u.active && u.target == Some(ri) && u.mode == Mode::Hold {
                u.set_mode(Mode::Standdown);
                u.target = None;
            }
        }
        let player = self.racers[ri].player;
        let spot = player.then(|| self.release_spot(t, &racers.body(self.racers[ri].id)));
        self.emit(PursuitEvent::Release {
            racer: ri,
            player,
            spot,
        });
    }

    /// Parked patrol cars appear ahead of the player and notice any racer who
    /// blasts past them.
    fn activate_spots(&mut self, t: &Track, racers: &dyn Racers) {
        let Some(p) = self.player else { return };
        let ps = racers.body(self.racers[p].id).s;
        for k in 0..self.spots.len() {
            if self.spots[k].used {
                continue;
            }
            let d = t.ds(ps, self.spots[k].s);
            if d > 900.0 || d < 150.0 {
                continue;
            }
            let Some(ui) = self.free_unit(&[Patrol, Interceptor]) else {
                return;
            };
            self.spots[k].used = true;
            let sp = self.spots[k];
            let f = t.frame(sp.s);
            let hw = self.units[ui].k.half_w();
            let lat = sp.lat.unwrap_or_else(|| {
                sp.side
                    * js::max(
                        0.0,
                        (if sp.side > 0.0 { f.wall_r } else { f.wall_l }) - hw - 0.5,
                    )
            });
            self.activate(t, ui, sp.s, lat, 0.0, Mode::Parked, 1);
            self.units[ui].park_lat = lat;
        }
    }

    fn free_unit(&self, types: &[UnitType]) -> Option<usize> {
        types.iter().find_map(|&ty| {
            self.units
                .iter()
                .position(|x| !x.active && x.unit_type == ty)
        })
    }

    /// `activate(u, s, lat, speed, mode, dir)` for police car `pi`.
    #[allow(clippy::too_many_arguments)]
    pub fn activate(
        &mut self,
        t: &Track,
        pi: usize,
        s: f64,
        lat: f64,
        speed: f64,
        mode: Mode,
        dir: i32,
    ) {
        let u = self.police_mut(pi);
        u.active = true;
        u.k.s = s;
        u.k.lat = lat;
        u.k.speed = speed;
        u.k.dir = dir;
        u.k.lat_vel = 0.0;
        u.k.spin = 0.0;
        u.k.spin_rate = 0.0;
        u.k.stunned = 0.0;
        u.health = 1.0;
        u.disabled_t = 0.0;
        u.pit_cooldown = 3.0;
        u.avoid_timer = Some(0.0);
        u.target = None;
        u.force_mode(mode);
        u.k.write_pos(t);
    }

    fn deactivate(&mut self, pi: usize) {
        let u = self.police_mut(pi);
        u.active = false;
        u.target = None;
    }

    /// Who each unit chases: the nearest racer by distance along the road,
    /// weighted toward the player so it gets most of the attention.
    fn pick_targets(&mut self, t: &Track, racers: &dyn Racers, dt: f64) {
        let full = self.active_count() >= HEAT[self.heat as usize].units.min(self.max_units);
        for ui in 0..self.units.len() {
            if !self.units[ui].active {
                continue;
            }
            if self.units[ui].mode == Mode::Parked {
                if full {
                    continue; // the heat's quota is out already
                }
                let us = self.units[ui].k.s;
                for ri in 0..self.racers.len() {
                    let r = &self.racers[ri];
                    if r.finished || r.hold > 0.0 || r.grace > 0.0 {
                        continue;
                    }
                    // A rival only sets it off when the player isn't coming too: it
                    // waits for the one it's really after.
                    if !r.player
                        && let Some(p) = self.player
                        && !self.racers[p].finished
                    {
                        let ps = racers.body(self.racers[p].id).s;
                        if t.ds(ps, us) < 500.0 && t.ds(ps, us) > -20.0 {
                            continue;
                        }
                    }
                    let rb = racers.body(r.id);
                    let d = t.ds(us, rb.s).abs();
                    if d < SPOT_RANGE && rb.speed_along.abs() > self.spot_speed(t, us) {
                        let player = r.player;
                        let u = &mut self.units[ui];
                        u.target = Some(ri);
                        u.set_mode(Mode::Chase);
                        self.emit(PursuitEvent::Spotted {
                            unit: ui,
                            racer: ri,
                            player,
                        });
                        break;
                    }
                }
                continue;
            }
            if self.units[ui].mode != Mode::Chase {
                continue;
            }
            {
                let u = &mut self.units[ui];
                u.retarget = Some(u.retarget.unwrap_or(0.0) - dt);
            }
            if let Some(cur) = self.units[ui].target {
                let c = &self.racers[cur];
                if c.hold > 0.0 || c.finished || c.grace > 0.0 {
                    self.units[ui].target = None;
                }
            }
            if self.units[ui].retarget.unwrap() > 0.0 && self.units[ui].target.is_some() {
                continue;
            }
            self.units[ui].retarget = Some(RETARGET);
            let us = self.units[ui].k.s;
            let (mut best, mut best_d) = (None, f64::INFINITY);
            for (ri, r) in self.racers.iter().enumerate() {
                if r.finished || r.hold > 0.0 || r.grace > 0.0 {
                    continue;
                }
                let rs = racers.body(r.id).s;
                if !self.can_see(t, us, rs) {
                    continue;
                }
                let d = t.ds(us, rs).abs() * (if r.player { 0.45 } else { 1.0 });
                if d < best_d {
                    best_d = d;
                    best = Some(ri);
                }
            }
            let u = &mut self.units[ui];
            if best.is_some() {
                u.target = best;
            } else if u.target.is_none() {
                u.last_seen_s = u.k.s;
                u.set_mode(Mode::Standdown);
            }
        }
    }

    fn spot_speed(&self, t: &Track, s: f64) -> f64 {
        let rule = self.traffic.get(self.zone_at(t, s));
        1.25 * rule.map_or(20.0, |r| r.0)
    }

    /// The player's pursuit state: pursuit while a unit can see you, cooldown
    /// (the evade meter fills) while none can.
    fn update_state(&mut self, t: &Track, racers: &mut dyn Racers, dt: f64) {
        let Some(p) = self.player else { return };
        if self.racers[p].finished {
            return;
        }
        let ps = racers.body(self.racers[p].id).s;
        let mut seen = false;
        for ui in 0..self.units.len() {
            let u = &self.units[ui];
            if !u.active
                || matches!(
                    u.mode,
                    Mode::Disabled | Mode::Parked | Mode::Standdown | Mode::Hold
                )
            {
                continue;
            }
            if u.target != Some(p) && u.mode != Mode::Search {
                continue;
            }
            if self.can_see(t, u.k.s, ps) {
                seen = true;
                let u = &mut self.units[ui];
                if u.mode == Mode::Search {
                    u.target = Some(p);
                    u.set_mode(Mode::Chase);
                }
                u.last_seen_s = ps;
            }
        }
        if self.racers[p].hold > 0.0 || self.racers[p].grace > 0.0 {
            seen = false;
        }
        let was = self.state;
        if seen {
            if self.state != State::Pursuit {
                self.state = State::Pursuit;
                let heat = self.heat;
                self.emit(if was == State::Cooldown {
                    PursuitEvent::Reacquired { heat }
                } else {
                    PursuitEvent::Pursuit { heat }
                });
                if was == State::Patrol {
                    self.prop_t = 12.0;
                    self.spawn_t = 2.0;
                }
            }
            self.evade = js::max(0.0, self.evade - 0.5 * dt);
            self.heat_up(t, racers, heat_gain::SECOND * dt);
        } else if self.state == State::Pursuit {
            self.state = State::Cooldown;
            self.emit(PursuitEvent::Cooldown);
            for u in &mut self.units {
                if u.active && u.target == Some(p) && u.mode == Mode::Chase {
                    u.set_mode(Mode::Search);
                    u.last_seen_s = ps;
                }
            }
        } else if self.state == State::Cooldown {
            self.evade += dt / EVADE_TIME[self.heat as usize];
            if self.evade >= 1.0 {
                self.evade = 0.0;
                self.state = State::Patrol;
                let heat = self.heat;
                self.emit(PursuitEvent::Escaped { heat });
                for u in &mut self.units {
                    if u.active && (u.mode == Mode::Search || u.target == Some(p)) {
                        u.set_mode(Mode::Standdown);
                        u.target = None;
                    }
                }
                self.clear_props(t, racers);
            }
        }
    }

    pub fn active_count(&self) -> usize {
        self.units.iter().filter(|u| u.chasing()).count()
    }

    /// Reinforcements while the player is in sight: from behind, out of view
    /// of the chase camera, arriving fast with the siren on; or ahead in the
    /// oncoming lane on two-way roads, to U-turn as you pass.
    fn spawn(&mut self, t: &Track, racers: &dyn Racers, dt: f64, rng: &mut dyn Rng) {
        self.spawn_t -= dt;
        let limit = HEAT[self.heat as usize].units.min(self.max_units);
        // Between pursuits a patrol car on the same road clocks you now and
        // then and comes up from behind.
        if let Some(p) = self.player {
            let pr = &self.racers[p];
            if self.state == State::Patrol && !pr.finished && pr.hold <= 0.0 && pr.grace <= 0.0 {
                let pt = match self.patrol_t {
                    Some(v) => v,
                    None => rrange(rng, 20.0, 30.0),
                } - dt;
                self.patrol_t = Some(pt);
                if pt <= 0.0 && self.active_count() < limit {
                    self.patrol_t = Some(rrange(rng, 30.0, 50.0));
                    let ui = self.free_unit(&[Patrol, Interceptor]);
                    let pb = racers.body(pr.id);
                    let s = pb.s - rrange(rng, 150.0, 220.0);
                    if let Some(ui) = ui
                        && s > t.start_s
                        && t.ds(s, pb.s) > 0.0
                    {
                        let f = t.frame(s);
                        let lat = clamp(pb.lat, -f.hw * 0.6, f.hw * 0.6);
                        self.activate(
                            t,
                            ui,
                            s,
                            lat,
                            js::max(20.0, pb.speed_along.abs() + 8.0),
                            Mode::Chase,
                            1,
                        );
                        self.units[ui].target = Some(p);
                        self.emit(PursuitEvent::Join { unit: ui });
                    }
                }
            }
        }
        let Some(p) = self.player else { return };
        let pr = &self.racers[p];
        if self.state != State::Pursuit || self.spawn_t > 0.0 || pr.hold > 0.0 || pr.finished {
            return;
        }
        if self.active_count() >= limit {
            return;
        }
        let mix = HEAT[self.heat as usize].mix;
        let want = mix[(rng.next_f64() * mix.len() as f64).floor() as usize];
        let Some(ui) = self.free_unit(&[want, Patrol, Interceptor, Suv]) else {
            return;
        };
        self.spawn_t = SPAWN_GAP;
        let pb = racers.body(pr.id);
        let ps = pb.s;
        let oncoming = self
            .traffic
            .get(self.zone_at(t, ps))
            .and_then(|r| r.1)
            .unwrap_or(0.0);
        let road = ROAD_TYPES[t.road_type[t.idx(ps)] as usize].key;
        let two_way = oncoming > 0.3 && !["freeway", "playa"].contains(&road);
        if two_way && rng.next_f64() < 0.35 {
            let s = ps + rrange(rng, 400.0, 600.0);
            if !t.is_loop && s > t.finish_s - 50.0 {
                return;
            }
            let f = t.frame(s);
            // Along the far edge of the oncoming lane, out of the racers' way.
            let lat = -js::max(f.hw * 0.5, f.wall_l - self.units[ui].k.half_w() - 0.5);
            self.activate(t, ui, s, lat, 22.0, Mode::Oncoming, -1);
            self.units[ui].lane_lat = lat;
            self.units[ui].target = Some(p);
        } else {
            let s = ps - rrange(rng, 180.0, 260.0);
            if !t.is_loop && s < 5.0 {
                return;
            }
            let f = t.frame(s);
            let lat = clamp(pb.lat, -f.hw * 0.6, f.hw * 0.6);
            self.activate(
                t,
                ui,
                s,
                lat,
                js::max(20.0, pb.speed_along.abs() + 8.0),
                Mode::Chase,
                1,
            );
            self.units[ui].target = Some(p);
        }
        self.emit(PursuitEvent::Join { unit: ui });
    }

    /// Tactics, by heat. Each unit keeps a behaviour at least 1.5 s.
    fn assign_behaviours(&mut self, t: &Track, racers: &dyn Racers, rng: &mut dyn Rng) {
        // byTarget: a Map in insertion order (the first unit chasing each).
        let mut by_target: Vec<(usize, Vec<usize>)> = Vec::new();
        for (ui, u) in self.units.iter().enumerate() {
            let Some(tg) = u.target.filter(|_| u.active && u.mode == Mode::Chase) else {
                continue;
            };
            match by_target.iter_mut().find(|(k, _)| *k == tg) {
                Some((_, l)) => l.push(ui),
                None => by_target.push((tg, vec![ui])),
            }
        }
        for (tg, list) in by_target {
            let is_player = Some(tg) == self.player;
            let ts = racers.body(self.racers[tg].id).s;
            let near: Vec<usize> = list
                .iter()
                .copied()
                .filter(|&u| t.ds(self.units[u].k.s, ts).abs() < 40.0)
                .collect();
            // Box (heat 4+): three units close by hold slots round the target.
            if is_player && self.heat >= 4 && near.len() >= 3 {
                let f = t.frame(ts);
                let wide = f.hw > 5.0;
                // furthest ahead first
                let mut order = near.clone();
                order.sort_by(|&a, &b| {
                    js_order(t.ds(ts, self.units[b].k.s) - t.ds(ts, self.units[a].k.s))
                });
                let slots: &[Slot] = if wide {
                    &[Slot::Ahead, Slot::Left, Slot::Right, Slot::Behind]
                } else {
                    &[Slot::Ahead, Slot::Behind, Slot::Behind, Slot::Behind]
                };
                let mut sides: Vec<usize> = order.iter().skip(1).take(2).copied().collect();
                sides.sort_by(|&a, &b| js_order(self.units[a].k.lat - self.units[b].k.lat));
                for &ui in &near {
                    let mut slot = if ui == order[0] {
                        Slot::Ahead
                    } else if wide && sides.contains(&ui) {
                        if ui == sides[0] {
                            Slot::Left
                        } else {
                            Slot::Right
                        }
                    } else {
                        Slot::Behind
                    };
                    if !slots.contains(&slot) {
                        slot = Slot::Behind;
                    }
                    let u = &mut self.units[ui];
                    if u.behaviour != Behaviour::Box && u.beh_t < MIN_BEHAVIOUR {
                        continue;
                    }
                    if u.behaviour != Behaviour::Box {
                        u.beh_t = 0.0;
                    }
                    u.behaviour = Behaviour::Box;
                    u.slot = Some(slot);
                }
                continue;
            }
            let mut rolling = list
                .iter()
                .any(|&u| self.units[u].behaviour == Behaviour::Roll);
            for &ui in &list {
                if self.units[ui].behaviour == Behaviour::Box && near.len() < 3 {
                    self.units[ui].behaviour = Behaviour::Chase;
                    self.units[ui].beh_t = 0.0;
                }
                if self.units[ui].beh_t < MIN_BEHAVIOUR {
                    continue;
                }
                let gap = t.ds(self.units[ui].k.s, ts);
                let r = rng.next_f64();
                let u = &self.units[ui];
                let mut b = Behaviour::Chase;
                let mut pit_side = None;
                if self.heat >= 3
                    && !rolling
                    && is_player
                    && gap < 20.0
                    && gap > -30.0
                    && u.unit_type != Patrol
                    && r < 0.35 * u.aggression + 0.15
                {
                    b = Behaviour::Roll;
                    rolling = true;
                } else if self.heat >= 2
                    && u.pit_cooldown == 0.0
                    && gap > 0.0
                    && gap < 18.0
                    && r < 0.5 * u.aggression + 0.1
                {
                    b = Behaviour::Pit;
                    pit_side = Some(if rng.next_f64() < 0.5 { -1 } else { 1 });
                } else if gap > 0.0 && gap < 25.0 && r < 0.4 + u.aggression * 0.5 {
                    b = Behaviour::Bump;
                }
                if u.behaviour == Behaviour::Roll && b != Behaviour::Roll {
                    rolling = list
                        .iter()
                        .any(|&o| o != ui && self.units[o].behaviour == Behaviour::Roll);
                }
                let u = &mut self.units[ui];
                if let Some(ps) = pit_side {
                    u.pit_side = ps;
                }
                if b != u.behaviour {
                    u.behaviour = b;
                    u.beh_t = 0.0;
                } else {
                    u.beh_t = MIN_BEHAVIOUR * 0.5; // re-roll soon
                }
            }
        }
    }

    /// Bust meters: fill while a racer crawls with a unit right beside it.
    fn bust_meters(&mut self, t: &Track, racers: &mut dyn Racers, dt: f64) {
        for ri in 0..self.racers.len() {
            let r = &self.racers[ri];
            if r.finished || r.hold > 0.0 {
                continue;
            }
            let b = racers.body(r.id);
            let sp = kernel::hypot(b.vx, b.vz);
            let mut n = 0;
            if r.grace <= 0.0 && sp < bust::SPEED && (!r.player || self.state == State::Pursuit) {
                for u in &self.units {
                    if !u.active || u.mode != Mode::Chase || u.target != Some(ri) {
                        continue;
                    }
                    if kernel::hypot(u.k.v.x - b.x, u.k.v.z - b.z) < bust::RADIUS {
                        n += 1;
                    }
                }
                if let Some(rb) = &self.roadblock {
                    for &c in &rb.cars {
                        let cv = &self.block_cars[c].k.v;
                        if kernel::hypot(cv.x - b.x, cv.z - b.z) < bust::RADIUS
                            && self.unit_near(t, ri, b.s, 80.0)
                        {
                            n += 1;
                        }
                    }
                }
            }
            let r = &mut self.racers[ri];
            if n > 0 {
                r.bust += (bust::RATE + bust::EXTRA * (n - 1) as f64) * dt;
            } else {
                r.bust = js::max(0.0, r.bust - bust::DRAIN * dt);
            }
            if r.player {
                self.bust = js::min(1.0, r.bust);
            }
            if self.racers[ri].bust >= 1.0 {
                let pen = bust_penalty(self.heat);
                self.arrest(t, racers, ri, HoldReason::Busted, pen);
            }
        }
    }

    fn unit_near(&self, t: &Track, ri: usize, bs: f64, d: f64) -> bool {
        self.units.iter().any(|u| {
            u.active && u.mode == Mode::Chase && u.target == Some(ri) && t.ds(u.k.s, bs).abs() < d
        })
    }

    // ── Roadblocks and spike strips ────────────────────────────────
    fn props(&mut self, t: &Track, racers: &mut dyn Racers, dt: f64, rng: &mut dyn Rng) {
        let Some(p) = self.player else { return };
        let pid = self.racers[p].id;
        let ps = racers.body(pid).s;
        self.prop_t -= dt;
        // Place one when the chase is on and the zone allows it.
        if self.state == State::Pursuit
            && self.roadblock.is_none()
            && self.spikes.is_none()
            && self.prop_t <= 0.0
            && !self.racers[p].finished
        {
            let spikes = self.heat >= 4 && rng.next_f64() < 0.45;
            if self.heat >= 3 {
                let s = self.find_straight(
                    t,
                    ps + if spikes { 380.0 } else { 470.0 },
                    ps + if spikes { 600.0 } else { 750.0 },
                    if spikes { 80.0 } else { 150.0 },
                    if spikes { 0.004 } else { 0.002 },
                );
                if let Some(s) = s
                    && self.cap_at(t, s) >= if spikes { 4 } else { 3 }
                {
                    if spikes {
                        self.place_spikes(t, racers, s);
                    } else {
                        self.place_roadblock(t, s, rng);
                    }
                }
            }
            self.prop_t = 6.0; // look again soon if nothing fitted
        }
        // Spike strips shred any racer whose wheels cross them.
        if let Some(sp) = self.spikes.clone() {
            for ri in 0..self.racers.len() {
                let r = &self.racers[ri];
                let b = racers.body(r.id);
                let prev = r.prev_s.unwrap_or(b.s);
                if prev < sp.s
                    && b.s >= sp.s
                    && !sp.hit[ri]
                    && b.lat + b.half_w > sp.lat0
                    && b.lat - b.half_w < sp.lat1
                {
                    if let Some(s) = &mut self.spikes {
                        s.hit[ri] = true;
                    }
                    let (player, name, id, ai) = (r.player, r.name, r.id, r.ai);
                    if player || ai {
                        racers.spike(id);
                    }
                    self.emit(PursuitEvent::Spiked {
                        racer: ri,
                        player,
                        name,
                    });
                }
            }
        }
        for ri in 0..self.racers.len() {
            let s = racers.body(self.racers[ri].id).s;
            self.racers[ri].prev_s = Some(s);
        }
        // Passed: count the dodge, clear up once it's well behind.
        for key in [false, true] {
            // false: the roadblock, true: the spikes (the JS's order)
            let (ps0, passed, hit_it) = match key {
                false => match &self.roadblock {
                    Some(r) => (r.s, r.passed, r.touched),
                    None => continue,
                },
                true => match &self.spikes {
                    Some(s) => (s.s, s.passed, s.hit[p]),
                    None => continue,
                },
            };
            let d = t.ds(ps0, ps);
            if d > 5.0 && !passed {
                match key {
                    false => self.roadblock.as_mut().unwrap().passed = true,
                    true => self.spikes.as_mut().unwrap().passed = true,
                }
                if !hit_it && self.state != State::Patrol {
                    self.heat_up(t, racers, heat_gain::DODGE);
                    self.emit(PursuitEvent::Dodge { spikes: key });
                }
            }
            if d > 250.0 || d < -1200.0 {
                if key {
                    self.clear_spikes();
                } else {
                    self.clear_roadblock();
                }
            }
        }
        if let Some(rb) = &self.roadblock
            && !rb.touched
        {
            let pb = racers.body(pid);
            let mut touched = false;
            for &c in &rb.cars {
                let cv = &self.block_cars[c];
                if kernel::hypot(cv.k.v.x - pb.x, cv.k.v.z - pb.z) < cv.k.half_l() + pb.half_l + 0.5
                {
                    touched = true;
                }
            }
            if touched {
                self.roadblock.as_mut().unwrap().touched = true;
            }
        }
    }

    /// The first s in [a, b] with a straight run `len` long around it.
    fn find_straight(&self, t: &Track, a: f64, b: f64, len: f64, kmax: f64) -> Option<f64> {
        let mut s = a;
        while s <= b {
            let skip = (!t.is_loop && (s > t.finish_s - 120.0 || s < t.start_s + 200.0))
                || self.in_tunnel(s).is_some();
            if !skip {
                let mut ok = true;
                let mut d = -len / 2.0;
                while d <= len / 2.0 {
                    if (t.k_smooth[t.idx(s + d)] as f64).abs() > kmax {
                        ok = false;
                        break;
                    }
                    d += 5.0;
                }
                if ok {
                    return Some(s);
                }
            }
            s += 10.0;
        }
        None
    }

    pub fn place_roadblock(&mut self, t: &Track, s: f64, rng: &mut dyn Rng) {
        let f = t.frame(s);
        let heavy = self.heat >= 5;
        let l = -f.wall_l + 0.3;
        let r = f.wall_r - 0.3;
        let span = r - l;
        let angle = 1.05; // ~60° to the road
        let mut pool: Vec<usize> = (0..self.block_cars.len()).collect();
        if heavy {
            // SUVs first (a stable sort).
            let suv = |i: usize| i32::from(self.block_cars[i].unit_type == Suv);
            pool.sort_by(|&a, &b| (suv(b) - suv(a)).cmp(&0));
        }
        let car0 = &self.block_cars[pool[0]];
        let ext =
            car0.k.half_l() * 2.0 * kernel::sin(angle) + car0.k.half_w() * 2.0 * kernel::cos(angle);
        let gap_w = 1.6 * 2.0 * 1.0; // 1.6 car widths
        let n = clamp(((span - gap_w) / ext).ceil(), 2.0, pool.len() as f64) as usize;
        let step = (span - gap_w) / n as f64;
        // The gap sits between two cars, away from the walls.
        let k = 1 + (rng.next_f64() * js::max(1.0, n as f64 - 1.0)).floor() as usize;
        let mut cars = Vec::new();
        let mut lat = l;
        for (i, &c) in pool.iter().enumerate().take(n) {
            if i == k {
                lat += gap_w;
            }
            let odd = i % 2 == 1;
            self.activate_block(
                t,
                c,
                s + if odd { 1.5 } else { -1.5 },
                lat + step / 2.0,
                (if odd { 1.0 } else { -1.0 }) * angle,
            );
            cars.push(c);
            lat += step;
        }
        let gap_lat = l + k as f64 * step + gap_w / 2.0;
        for &c in &cars {
            self.block_cars[c].gap_lat = Some(gap_lat);
        }
        if heavy {
            // Heavy roadblock: the gap is closed by barriers you can smash.
            for i in 0..self.sawhorses.len() {
                let b_lat = gap_lat + if i > 0 { 0.7 } else { -0.7 };
                self.activate_barrier(t, i, s + if i > 0 { 2.0 } else { -2.0 }, b_lat);
                self.sawhorses[i].gap_lat = Some(gap_lat);
            }
        }
        self.roadblock = Some(Roadblock {
            s,
            cars,
            gap_lat,
            heavy,
            passed: false,
            touched: false,
        });
        self.emit(PursuitEvent::Roadblock { s, heavy });
    }

    fn activate_block(&mut self, t: &Track, c: usize, s: f64, lat: f64, yaw: f64) {
        let pi = self.units.len() + c;
        self.activate(t, pi, s, lat, 0.0, Mode::Block, 1);
        let u = &mut self.block_cars[c];
        u.block_yaw = yaw;
        u.k.spin = yaw;
        u.siren = Siren::Flash;
        u.k.write_pos(t);
    }

    fn activate_barrier(&mut self, t: &Track, i: usize, s: f64, lat: f64) {
        let b = &mut self.sawhorses[i];
        b.active = true;
        b.broken = false;
        b.k.s = s;
        b.k.lat = lat;
        b.k.speed = 0.0;
        b.k.lat_vel = 0.0;
        b.k.dir = 1;
        b.h = 0.0;
        b.vy = 0.0;
        b.age = 0.0;
        b.k.spin = PI / 2.0;
        b.k.spin_rate = 0.0;
        b.write_pos(t);
    }

    fn update_sawhorse(&mut self, t: &Track, i: usize, dt: f64) {
        let b = &mut self.sawhorses[i];
        if !b.broken {
            b.k.spin = PI / 2.0;
            b.k.speed = 0.0;
            b.k.lat_vel = 0.0;
            b.write_pos(t);
            return;
        }
        // Flying debris: a tumble up and back down, then gone.
        b.age += dt;
        b.vy -= 9.8 * dt;
        b.h = js::max(0.0, b.h + b.vy * dt);
        if b.h == 0.0 {
            b.vy = 0.0;
            b.k.speed *= 1.0 - 3.0 * dt;
            b.k.lat_vel *= 1.0 - 3.0 * dt;
        }
        b.k.s += b.k.speed * dt;
        b.k.lat += b.k.lat_vel * dt;
        b.k.spin += b.k.spin_rate * dt;
        b.write_pos(t);
        if b.age > 3.0 {
            b.active = false;
        }
    }

    pub fn place_spikes(&mut self, t: &Track, racers: &dyn Racers, s: f64) {
        let f = t.frame(s);
        let p = self.player.expect("spikes are for the player");
        // Across the half of the road the player's on, with a unit parked on
        // the far shoulder.
        let l = -f.wall_l + 0.4;
        let r = f.wall_r - 0.4;
        let span = r - l;
        let w = js::min(span * 0.6, 9.0);
        let c = clamp(racers.body(self.racers[p].id).lat, l + w / 2.0, r - w / 2.0);
        let side = if c > 0.0 { -1.0 } else { 1.0 };
        let hw = self.block_cars[0].k.half_w();
        let lat = side
            * js::max(
                0.0,
                (if side > 0.0 { f.wall_r } else { f.wall_l }) - hw - 0.4,
            );
        self.activate_block(t, 0, s - 6.0, lat, side * 0.25);
        self.block_cars[0].gap_lat = None;
        self.spikes = Some(Spikes {
            s,
            lat0: c - w / 2.0,
            lat1: c + w / 2.0,
            car: 0,
            hit: vec![false; self.racers.len()],
            passed: false,
        });
        self.emit(PursuitEvent::Spikes { s });
    }

    fn clear_roadblock(&mut self) {
        let Some(rb) = self.roadblock.take() else {
            return;
        };
        let n = self.units.len();
        for c in rb.cars {
            self.deactivate(n + c);
        }
        for b in &mut self.sawhorses {
            b.active = false;
        }
        self.prop_t = 25.0;
    }

    fn clear_spikes(&mut self) {
        let Some(sp) = self.spikes.take() else { return };
        let n = self.units.len();
        self.deactivate(n + sp.car);
        self.prop_t = 20.0;
    }

    /// Only clear props still ahead (out of view); ones in sight stay.
    fn clear_props(&mut self, t: &Track, racers: &dyn Racers) {
        let ps = self
            .player
            .map_or(0.0, |p| racers.body(self.racers[p].id).s);
        if self
            .roadblock
            .as_ref()
            .is_some_and(|r| t.ds(ps, r.s) > 250.0)
        {
            self.clear_roadblock();
        }
        if self.spikes.as_ref().is_some_and(|s| t.ds(ps, s.s) > 250.0) {
            self.clear_spikes();
        }
    }

    /// Units well behind the player and out of sight go back to the pool.
    // Two conditions, as the JS writes them.
    #[allow(clippy::if_same_then_else)]
    fn recycle(&mut self, t: &Track, racers: &dyn Racers) {
        let Some(p) = self.player else { return };
        let ps = racers.body(self.racers[p].id).s;
        for ui in 0..self.units.len() {
            let u = &self.units[ui];
            if !u.active {
                continue;
            }
            let d = t.ds(ps, u.k.s);
            let busy = u.mode == Mode::Chase
                && u.target.is_some_and(|tg| {
                    let r = &self.racers[tg];
                    !r.player && t.ds(u.k.s, racers.body(r.id).s).abs() < 400.0
                });
            if busy {
                continue;
            }
            if (d < -600.0 && !self.can_see(t, u.k.s, ps)) || d > 1500.0 {
                self.deactivate(ui);
            } else if matches!(
                u.mode,
                Mode::Standdown | Mode::Disabled | Mode::Parked | Mode::Hold
            ) && d < -350.0
            {
                self.deactivate(ui);
            }
        }
    }
}

/// A comparator result as JS's sort reads it.
fn js_order(x: f64) -> std::cmp::Ordering {
    if x < 0.0 {
        std::cmp::Ordering::Less
    } else if x > 0.0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// The shoulder on the side of `lat`, clear of the wall.
pub fn shoulder_lat(t: &Track, s: f64, lat: f64, half_w: f64) -> f64 {
    let f = t.frame(s);
    let side = if lat >= 0.0 { 1.0 } else { -1.0 };
    side * js::max(
        0.0,
        (if side > 0.0 { f.wall_r } else { f.wall_l }) - half_w - 0.6,
    )
}
