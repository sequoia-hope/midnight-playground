//! A player's controls for one tick (SPEC 4.3): [`Input`] is what the JS
//! passes physics (`inp`), [`InputFrame`] the quantised form the network, a
//! replay and the trace carry. The reference runs quantise exactly so
//! (`quantiseInput`, `src/parity/sim.js`), so both sides integrate identical
//! values.

use mp_math::js;

/// The controls physics reads. `cruise` is the cruise level's part-throttle
/// gearbox hint; nothing in the race sets it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    pub steer: f64,
    pub throttle: f64,
    pub brake: f64,
    pub handbrake: bool,
    pub nitro: bool,
    pub analog: bool,
    pub cruise: bool,
}

/// One player's controls for one tick, already quantised.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct InputFrame {
    /// -32767..32767
    pub steer: i16,
    pub throttle: u8,
    pub brake: u8,
    /// handbrake 1, nitro 2, analog 4, reset 8 (an edge).
    pub flags: u8,
}

pub const HANDBRAKE: u8 = 1;
pub const NITRO: u8 = 2;
pub const ANALOG: u8 = 4;
/// Multiplayer: this player's car is driven by the autopilot (they dropped
/// out; MULTIPLAYER 2.7). The host sets it in the inputs it relays, so
/// every device drives the car the same way.
pub const AUTOPILOT: u8 = 16;
/// Multiplayer: the player has the menu open (hands off; MULTIPLAYER 2.9).
/// The simulation ignores it; the client shows them as away.
pub const AWAY: u8 = 32;
pub const RESET: u8 = 8;

fn c(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

impl InputFrame {
    pub fn quantise(inp: &Input) -> InputFrame {
        InputFrame {
            steer: js::round(c(inp.steer, -1.0, 1.0) * 32767.0) as i16,
            throttle: js::round(c(inp.throttle, 0.0, 1.0) * 255.0) as u8,
            brake: js::round(c(inp.brake, 0.0, 1.0) * 255.0) as u8,
            flags: (if inp.handbrake { HANDBRAKE } else { 0 })
                | (if inp.nitro { NITRO } else { 0 })
                | (if inp.analog { ANALOG } else { 0 }),
        }
    }

    /// The values physics integrates (`quantiseInput`'s output).
    pub fn input(&self) -> Input {
        Input {
            steer: self.steer as f64 / 32767.0,
            throttle: self.throttle as f64 / 255.0,
            brake: self.brake as f64 / 255.0,
            handbrake: self.flags & HANDBRAKE != 0,
            nitro: self.flags & NITRO != 0,
            analog: self.flags & ANALOG != 0,
            cruise: false,
        }
    }
}

/// `quantiseInput`: the input as the simulation sees it. Fields an
/// `InputFrame` does not carry (`cruise`) pass through.
pub fn quantise(inp: &Input) -> Input {
    Input {
        cruise: inp.cruise,
        ..InputFrame::quantise(inp).input()
    }
}
