//! The module oracle's staging, in Rust (`tools/parity/lib/node-sim.mjs` and
//! the catalogue in `tools/parity/sim-module.mjs`): the simulation's modules
//! on the real levels, with the world data and vehicle dimensions the game
//! uses, stepped in `Race.update`'s order at a fixed 1/120 s, without Race's
//! own rules. The module traces (`parity/golden/sim/module/`) are replayed
//! through this.
//!
//! Grows with the work packages: physics alone (WP 1.3) first; rivals,
//! traffic, collisions (1.4) and the pursuit (1.6) join it.

use std::sync::Arc;

use mr_levels::world::{WorldData, world_data};
use mr_track::{Level, Track};

use crate::dims::dims;
use crate::input::Input;
use crate::physics::{CarPhysics, CarSpec, car_spec};
use crate::trace::{PlayerView, View, trace_record};
use crate::vehicle::Vehicle;

pub const DT: f64 = 1.0 / 120.0;

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

/// A staged world.
pub struct Sim {
    pub track: Arc<Track>,
    pub p: Option<Player>,
    pub time: f64,
    pub tick: u32,
    pub started: bool,
}

impl Sim {
    /// The level, the player on Race's grid (rows of two; with no rivals the
    /// player is first).
    pub fn new(stage: &Stage, car: &'static str) -> Sim {
        let t = stage.track.clone();
        let mut p = player(car);
        // Race's grid (Race.js:90-98), the player alone: row 0, column 0.
        let s = t.start_s - 5.0;
        p.phys.reset(&mut p.v, &t, t.wrap(s), -2.4);
        p.v.prog = Some(s);
        Sim {
            track: t,
            p: Some(p),
            time: 0.0,
            tick: 0,
            started: true,
        }
    }

    /// One tick; `inp` is the player's input, already quantised.
    pub fn step(&mut self, inp: &Input) {
        self.tick += 1;
        self.time += DT;
        if let Some(p) = &mut self.p {
            p.phys.update(&mut p.v, &self.track, DT, inp);
            p.phys.events.clear();
        }
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

    /// The tick's trace record. The four streams exist but nothing in a
    /// physics scenario draws from them.
    pub fn record(&self, inp: &Input) -> Vec<u8> {
        let players = self
            .p
            .iter()
            .map(|p| PlayerView {
                v: &p.v,
                phys: &p.phys,
                input: Some(inp),
            })
            .collect();
        trace_record(&View {
            tick: self.tick as i32,
            players,
            n_rivals: 0,
            n_traffic: 0,
            streams: [0, 0, 0, 0],
        })
    }
}
