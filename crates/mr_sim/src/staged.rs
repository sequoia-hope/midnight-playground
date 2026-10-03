//! The module oracle's staging, in Rust (`tools/parity/lib/node-sim.mjs` and
//! the catalogue in `tools/parity/sim-module.mjs`): the simulation's modules
//! on the real levels, with the world data and vehicle dimensions the game
//! uses, stepped in `Race.update`'s order at a fixed 1/120 s, without Race's
//! own rules and without PursuitView. The module traces
//! (`parity/golden/sim/module/`) are replayed through this.

use std::sync::Arc;

use mr_levels::world::{WorldData, world_data};
use mr_track::{Level, Track};

use crate::ai::{AiCtx, AiDriver, AiOpts};
use crate::body::BodyId;
use crate::dims::dims;
use crate::field::{Field, RacerAccess};
use crate::input::Input;
use crate::pursuit::{Pursuit, PursuitOpts, top_speed};
use crate::race::PlayerCar;
use crate::rng::RngStreams;
use crate::trace::{PlayerView, View, trace_record};
use crate::traffic::Traffic;
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

/// What to stage (`stage()` in sim-module.mjs).
#[derive(Clone, Copy, Debug)]
pub struct StageOpts {
    pub car: &'static str,
    pub with_rivals: bool,
    pub traffic_count: usize,
    /// The police from this heat, if any.
    pub police_heat: Option<f64>,
}

/// A staged world.
pub struct Sim {
    pub track: Arc<Track>,
    pub players: Vec<PlayerCar>,
    pub ais: Vec<AiDriver>,
    pub traf: Option<Traffic>,
    pub pu: Option<Pursuit>,
    pub streams: RngStreams,
    pub time: f64,
    pub tick: u32,
    pub started: bool,
    pub odo: Option<f64>,
    pub last_s: Option<f64>,
}

impl Sim {
    /// The level, the player (and optionally the rival field) on Race's
    /// grid, traffic, the police, the streams.
    pub fn new(stage: &Stage, o: StageOpts) -> Sim {
        let t = stage.track.clone();
        let mut streams = RngStreams::new(SEED);
        let mut p = PlayerCar::new(o.car);
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
        let pu = o.police_heat.map(|heat| {
            let opts = PursuitOpts {
                heat,
                max_units: 6.0,
                player_top: top_speed(&p.spec),
                flash: true,
            };
            let RngStreams {
                pursuit, police, ..
            } = &mut streams;
            let mut pu = Pursuit::new(&t, &stage.level, opts, pursuit, police);
            let mut list = vec![(BodyId::Player(0), true, false, "You")];
            list.extend(
                ais.iter()
                    .enumerate()
                    .map(|(i, a)| (BodyId::Rival(i), false, true, a.name)),
            );
            pu.set_racers(list);
            pu
        });
        Sim {
            track: t,
            players: vec![p],
            ais,
            traf,
            pu,
            streams,
            time: 0.0,
            tick: 0,
            started: true,
            odo: None,
            last_s: None,
        }
    }

    pub fn field(&mut self) -> Field<'_> {
        Field {
            players: &mut self.players,
            rivals: &mut self.ais,
            traffic: self.traf.as_mut(),
            pursuit: self.pu.as_mut(),
        }
    }

    /// One tick; `inp` is the player's input, already quantised.
    pub fn step(&mut self, inp: &Input) {
        let t = self.track.clone();
        self.tick += 1;
        self.time += DT;
        let mut agents = self.field().agents();
        {
            let p = &mut self.players[0];
            p.phys.update(&mut p.v, &t, DT, inp);
        }
        let ps = self.players[0].v.s;
        for i in 0..self.ais.len() {
            let views = self.field().views(&agents);
            let ctx = AiCtx {
                cars: &views,
                me: agents.iter().position(|&r| r == BodyId::Rival(i)),
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
            let list = self.field().traffic_agents(&agents);
            let dist = if t.is_loop { odo } else { ps };
            if let Some(tr) = &mut self.traf {
                tr.update(&t, DT, ps, &list, 0.0, dist, &mut self.streams.traffic);
            }
        }
        if self.pu.is_some() {
            let list = self.field().pursuit_agents(&agents);
            let (time, started) = (self.time, self.started);
            let pu = self.pu.as_mut().unwrap();
            let mut racers = RacerAccess {
                players: &mut self.players,
                rivals: &mut self.ais,
            };
            pu.update(
                &t,
                DT,
                &list,
                &mut racers,
                time,
                started,
                &mut self.streams.pursuit,
            );
            // Units that joined this tick collide from now on.
            for b in self.field().agents() {
                if !agents.contains(&b) {
                    agents.push(b);
                }
            }
        }
        let hits = self.field().collide(&t, &agents);
        for a in &mut self.ais {
            a.write_pos(&t);
        }
        if let Some(tr) = &mut self.traf {
            for c in tr.cars.iter_mut().filter(|c| c.active) {
                c.k.write_pos(&t);
            }
        }
        if let Some(pu) = &mut self.pu {
            pu.write_pos(&t);
        }
        for h in hits {
            let (a, b) = (agents[h.a], agents[h.b]);
            if self.pu.is_some() {
                let (va, vb) = (self.field().velocity(a), self.field().velocity(b));
                let plat = self.players[0].v.lat;
                let pu = self.pu.as_mut().unwrap();
                let mut racers = RacerAccess {
                    players: &mut self.players,
                    rivals: &mut self.ais,
                };
                let pit = pu.on_hit(
                    &t,
                    &mut racers,
                    a,
                    b,
                    h.strength,
                    va,
                    vb,
                    plat,
                    &mut self.streams.pursuit,
                );
                if pit != 0.0 {
                    self.players[0].v.yaw_rate += pit * 2.2;
                }
            }
            // The crash flag: the body that is not the player, or h.a when the
            // player is not in the hit; only traffic cars carry one.
            let other = if a == BodyId::Player(0) { b } else { a };
            if let BodyId::Traffic(i) = other
                && h.strength > 0.15
            {
                let c = &mut self.traf.as_mut().unwrap().cars[i];
                c.crashed = mr_math::js::max(c.crashed, 0.01);
            }
        }
        self.players[0].phys.events.clear();
        if let Some(pu) = &mut self.pu {
            pu.events.clear();
        }
    }

    /// Only the player's physics, at a frame time instead of the tick (the
    /// `phys-frame-dt` scenario).
    pub fn step_frame(&mut self, frame_dt: f64, inp: &Input) {
        self.tick += 1;
        let t = self.track.clone();
        let p = &mut self.players[0];
        p.phys.update(&mut p.v, &t, frame_dt, inp);
        p.phys.events.clear();
    }

    /// The tick's trace record.
    pub fn record(&self, inp: &Input) -> Vec<u8> {
        let players = self
            .players
            .iter()
            .map(|p| PlayerView {
                v: &p.v,
                phys: &p.phys,
                input: Some(inp),
                rules: None,
                pursuit_view: None,
            })
            .collect();
        trace_record(&View {
            tick: self.tick as i32,
            race: None,
            players,
            rivals: &self.ais,
            traffic: self.traf.as_ref(),
            pursuit: self.pu.as_ref(),
            last_hit: None,
            streams: self.streams.draws(),
        })
    }
}
