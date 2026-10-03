//! The module oracle's staging, in Rust (`tools/parity/lib/node-sim.mjs` and
//! the catalogue in `tools/parity/sim-module.mjs`): the simulation's modules
//! on the real levels, with the world data and vehicle dimensions the game
//! uses, stepped in `Race.update`'s order at a fixed 1/120 s, without Race's
//! own rules. The module traces (`parity/golden/sim/module/`) are replayed
//! through this.
//!
//! Grows with the work packages: physics (WP 1.3); rivals, traffic and
//! collisions (1.4); the pursuit (1.6).

use std::sync::Arc;

use mr_levels::world::{WorldData, world_data};
use mr_track::{Level, Track};

use crate::ai::{AiCtx, AiDriver, AiOpts};
use crate::body::{AgentView, Body, PhysicsBody, player_view};
use crate::collisions::{Hit, resolve_collisions};
use crate::dims::dims;
use crate::input::Input;
use crate::physics::{CarPhysics, CarSpec, car_spec};
use crate::rng::RngStreams;
use crate::trace::{PlayerView, View, trace_record};
use crate::traffic::{Agent, Traffic};
use crate::vehicle::Vehicle;

pub const DT: f64 = 1.0 / 120.0;
pub const SEED: u32 = 1;

/// A level's Track with what the world's scenery sets on it (runout), and
/// the world data.
pub struct Stage {
    pub level: Level,
    pub track: Arc<Track>,
    pub world: WorldData,
}

pub fn stage_level(level: Level) -> Stage {
    let mut t = Track::new(&level).expect("the level builds a track");
    let world = world_data(level.id);
    t.runout = world.runout;
    Stage {
        level,
        track: Arc::new(t),
        world,
    }
}

/// The player as Race builds it: the car's spec and dimensions.
pub struct Player {
    pub v: Vehicle,
    pub phys: CarPhysics,
    pub spec: CarSpec,
}

pub fn player(car: &'static str) -> Player {
    let spec = car_spec(car).expect("a car in CAR_SPECS");
    let v = Vehicle::new(dims(car).expect("dims"), car, spec.mass, "You", spec.color);
    let phys = CarPhysics::new(&v, spec);
    Player { v, phys, spec }
}

/// The level's rivals as Race builds them (Race.js:80-88), drawing from the
/// `ai` stream.
pub fn rivals(level: &Level, streams: &mut RngStreams) -> Vec<AiDriver> {
    level
        .rivals
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let v = Vehicle::new(dims(r.kind).expect("dims"), r.kind, 1400.0, r.name, r.color);
            let opts = AiOpts {
                skill: Some(r.skill),
                name: r.name,
                power: Some(r.power),
                bias: Some((if i % 2 == 1 { 1.0 } else { -1.0 }) * 0.6),
                line_factor: Some(0.8 + (i % 3) as f64 * 0.08),
            };
            let mut ai = AiDriver::new(v, opts, &mut streams.ai);
            ai.color = r.color;
            ai
        })
        .collect()
}

/// Who an entry of the agent list is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRef {
    Player,
    Rival(usize),
    Traffic(usize),
}

/// What to stage (`stage()` in sim-module.mjs).
#[derive(Clone, Copy, Debug)]
pub struct StageOpts {
    pub car: &'static str,
    pub with_rivals: bool,
    pub traffic_count: usize,
}

/// A staged world.
pub struct Sim {
    pub track: Arc<Track>,
    pub p: Option<Player>,
    pub ais: Vec<AiDriver>,
    pub traf: Option<Traffic>,
    pub streams: RngStreams,
    pub time: f64,
    pub tick: u32,
    pub started: bool,
    pub odo: Option<f64>,
    pub last_s: Option<f64>,
}

impl Sim {
    /// The level, the player (and optionally the rival field) on Race's
    /// grid, traffic, the streams.
    pub fn new(stage: &Stage, o: StageOpts) -> Sim {
        let t = stage.track.clone();
        let mut streams = RngStreams::new(SEED);
        let mut p = player(o.car);
        let mut ais = if o.with_rivals {
            rivals(&stage.level, &mut streams)
        } else {
            Vec::new()
        };
        // Race's grid (Race.js:90-98): rows of two; the player fourth with
        // five rivals, else first.
        let order: Vec<Option<usize>> = if ais.len() >= 5 {
            vec![Some(0), Some(1), Some(2), None, Some(3), Some(4)]
        } else {
            std::iter::once(None)
                .chain((0..ais.len()).map(Some))
                .collect()
        };
        for (k, c) in order.into_iter().enumerate() {
            let row = (k / 2) as f64;
            let col = k % 2;
            let s = t.start_s - 5.0 - row * 10.0 - col as f64 * 3.0;
            let lat = if col == 1 { 2.4 } else { -2.4 };
            match c {
                None => {
                    p.phys.reset(&mut p.v, &t, t.wrap(s), lat);
                    p.v.prog = Some(s);
                }
                Some(i) => {
                    let a = &mut ais[i];
                    a.k.s = t.wrap(s);
                    a.k.lat = lat;
                    a.k.speed = 0.0;
                    a.prog = Some(s);
                    a.write_pos(&t);
                }
            }
        }
        let traf = (o.traffic_count > 0).then(|| {
            Traffic::new(
                &stage.level,
                stage.world.opposite_carriageway.clone(),
                o.traffic_count,
                &mut streams.traffic,
            )
        });
        Sim {
            track: t,
            p: Some(p),
            ais,
            traf,
            streams,
            time: 0.0,
            tick: 0,
            started: true,
            odo: None,
            last_s: None,
        }
    }

    /// The agent list (`agents()`): player, rivals, active traffic.
    pub fn agents(&self) -> Vec<AgentRef> {
        let mut out = Vec::new();
        if self.p.is_some() {
            out.push(AgentRef::Player);
        }
        out.extend((0..self.ais.len()).map(AgentRef::Rival));
        if let Some(tr) = &self.traf {
            out.extend(
                tr.cars
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.active)
                    .map(|(i, _)| AgentRef::Traffic(i)),
            );
        }
        out
    }

    pub fn view(&self, r: AgentRef) -> AgentView {
        match r {
            AgentRef::Player => player_view(&self.p.as_ref().unwrap().v),
            AgentRef::Rival(i) => self.ais[i].view(),
            AgentRef::Traffic(i) => self.traf.as_ref().unwrap().cars[i].view(),
        }
    }

    /// One tick; `inp` is the player's input, already quantised.
    pub fn step(&mut self, inp: &Input) {
        let t = self.track.clone();
        self.tick += 1;
        self.time += DT;
        let agents = self.agents();
        if let Some(p) = &mut self.p {
            p.phys.update(&mut p.v, &t, DT, inp);
        }
        let ps = match &self.p {
            Some(p) => p.v.s,
            None => self.ais.first().map_or(0.0, |a| a.k.s),
        };
        for i in 0..self.ais.len() {
            let views: Vec<AgentView> = agents.iter().map(|&r| self.view(r)).collect();
            let ctx = AiCtx {
                cars: &views,
                me: agents.iter().position(|&r| r == AgentRef::Rival(i)),
                player_s: ps,
                player_prog: None,
                started: self.started,
                time: self.time,
            };
            self.ais[i].update(&t, DT, &ctx, &mut self.streams.ai);
        }
        if self.traf.is_some() {
            let d_s = t.ds(self.last_s.unwrap_or(ps), ps);
            self.last_s = Some(ps);
            let odo = self.odo.unwrap_or(ps) + d_s;
            self.odo = Some(odo);
            let list: Vec<Agent> = agents
                .iter()
                .map(|&r| match r {
                    AgentRef::Traffic(i) => Agent::Traffic(i),
                    r => Agent::Other(self.view(r)),
                })
                .collect();
            let dist = if t.is_loop { odo } else { ps };
            if let Some(tr) = &mut self.traf {
                tr.update(&t, DT, ps, &list, 0.0, dist, &mut self.streams.traffic);
            }
        }
        let hits = self.collide(&agents);
        for a in &mut self.ais {
            a.write_pos(&t);
        }
        if let Some(tr) = &mut self.traf {
            for c in tr.cars.iter_mut().filter(|c| c.active) {
                c.k.write_pos(&t);
            }
        }
        for h in hits {
            // The crash flag: the body that is not the player, or h.a when the
            // player is not in the hit; only traffic cars carry one.
            let pb = agents.iter().position(|&r| r == AgentRef::Player);
            let other = if Some(h.a) == pb { h.b } else { h.a };
            if let AgentRef::Traffic(i) = agents[other]
                && h.strength > 0.15
            {
                let c = &mut self.traf.as_mut().unwrap().cars[i];
                c.crashed = mr_math::js::max(c.crashed, 0.01);
            }
        }
        if let Some(p) = &mut self.p {
            p.phys.events.clear();
        }
    }

    /// `resolveCollisions(agents, hits)`.
    fn collide(&mut self, agents: &[AgentRef]) -> Vec<Hit> {
        let t = self.track.clone();
        let mut hits = Vec::new();
        let mut pb = self.p.as_mut().map(|p| PhysicsBody { v: &mut p.v });
        let mut rivals: Vec<Option<&mut AiDriver>> = self.ais.iter_mut().map(Some).collect();
        let mut cars: Vec<Option<&mut crate::traffic::TrafficCar>> = match &mut self.traf {
            Some(tr) => tr.cars.iter_mut().map(Some).collect(),
            None => Vec::new(),
        };
        let mut bodies: Vec<&mut dyn Body> = Vec::with_capacity(agents.len());
        let mut player = pb.as_mut();
        for &r in agents {
            match r {
                AgentRef::Player => bodies.push(player.take().unwrap()),
                AgentRef::Rival(i) => bodies.push(rivals[i].take().unwrap()),
                AgentRef::Traffic(i) => bodies.push(cars[i].take().unwrap()),
            }
        }
        resolve_collisions(&mut bodies[..], &t, &mut hits);
        hits
    }

    /// Only the player's physics, at a frame time instead of the tick (the
    /// `phys-frame-dt` scenario).
    pub fn step_frame(&mut self, frame_dt: f64, inp: &Input) {
        self.tick += 1;
        if let Some(p) = &mut self.p {
            p.phys.update(&mut p.v, &self.track, frame_dt, inp);
            p.phys.events.clear();
        }
    }

    /// The tick's trace record.
    pub fn record(&self, inp: &Input) -> Vec<u8> {
        let players = self
            .p
            .iter()
            .map(|p| PlayerView {
                v: &p.v,
                phys: &p.phys,
                input: Some(inp),
                rules: None,
            })
            .collect();
        trace_record(&View {
            tick: self.tick as i32,
            race: None,
            players,
            rivals: &self.ais,
            traffic: self.traf.as_ref(),
            streams: self.streams.draws(),
        })
    }
}
