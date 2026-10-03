//! The named random streams of the simulation (SPEC 4.3): `ai` (rival nitro
//! timers), `police` (the weave), `pursuit` (callsigns, spawns, props) and
//! `traffic` (Traffic's own stream, seed 99). Each counts its draws, which
//! the trace record carries (`simStreams` in `src/parity/sim.js`).

use mr_math::{Mulberry32, Rng};

pub const TRAFFIC_SEED: u32 = 99;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stream {
    pub rng: Mulberry32,
    pub draws: u32,
}

impl Stream {
    pub fn new(seed: u32) -> Stream {
        Stream {
            rng: Mulberry32::new(seed),
            draws: 0,
        }
    }
}

impl Rng for Stream {
    fn next_f64(&mut self) -> f64 {
        self.draws += 1;
        self.rng.next_f64()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RngStreams {
    pub ai: Stream,
    pub police: Stream,
    pub pursuit: Stream,
    pub traffic: Stream,
}

impl RngStreams {
    /// Stream k (ai 0, police 1, pursuit 2) starts at
    /// `(seed + 0x9E3779B9 * (k + 1)) >>> 0`; traffic at 99.
    pub fn new(seed: u32) -> RngStreams {
        let k = |k: u32| Stream::new(seed.wrapping_add(0x9E37_79B9u32.wrapping_mul(k + 1)));
        RngStreams {
            ai: k(0),
            police: k(1),
            pursuit: k(2),
            traffic: Stream::new(TRAFFIC_SEED),
        }
    }

    /// Draws so far, in the trace's order: ai, police, pursuit, traffic.
    pub fn draws(&self) -> [i32; 4] {
        [
            self.ai.draws,
            self.police.draws,
            self.pursuit.draws,
            self.traffic.draws,
        ]
        .map(|d| d as i32)
    }
}
