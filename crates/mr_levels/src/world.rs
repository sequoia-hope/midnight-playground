//! What the simulation needs from the world that the JS computes in scenery
//! code rather than in the level files (SPEC 4.3, "Data the world gives the
//! simulation"):
//!
//! - `Track.runout`, the drivable road past the last sample: 900 m on Sierra
//!   (`City.js:48`), 700 m on Coast (`Harbor.js:57`), 0 elsewhere.
//! - `world.oppositeCarriageway` (`City.js:222`, `Harbor.js:179`): the range
//!   (`s0`, `s1`), the lane offsets (`OPP_LANES`, `city/freeway.js:26`), the
//!   direction, and the height of the far carriageway, which is ported as its
//!   formula, `oppY` (`city/freeway.js:64`).
//!
//! The numbers are the JS world's, dumped by `tools/parity/sim-world.mjs`
//! (`parity/golden/sim/world-data.json`). When the scenery that computes
//! them is ported (M3, M7), a test checks it reproduces them.

use mr_math::js;
use mr_track::{Frame, Track};

/// Westbound lane centres (lat), nearest the median first. Traffic on them
/// drives toward -s; surface height is oppY(frame).
pub const OPP_LANES: [f64; 4] = [-15.67, -19.57, -23.47, -27.37];
/// Half the freeway's paved width (`HALF`, `city/freeway.js`).
const HALF: f64 = 9.4;

/// Height of the westbound carriageway: flat across, never below the ground
/// the terrain flattened for our side.
pub fn opp_y(f: &Frame) -> f64 {
    f.y + js::max(0.04, HALF * f.bank - 0.2)
}

/// The westbound carriageway beside the track (lat relative to it).
#[derive(Clone, Debug, PartialEq)]
pub struct OppositeCarriageway {
    pub s0: f64,
    pub s1: f64,
    pub lanes: Vec<f64>,
    pub dir: f64,
}

impl OppositeCarriageway {
    /// `y(s)`: the carriageway's height at s.
    pub fn y(&self, t: &Track, s: f64) -> f64 {
        opp_y(&t.frame(s))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldData {
    pub runout: f64,
    pub opposite_carriageway: Option<OppositeCarriageway>,
}

/// The world data of a level, by id.
pub fn world_data(id: &str) -> WorldData {
    let opp = |s0: f64, s1: f64| {
        Some(OppositeCarriageway {
            s0,
            s1,
            lanes: OPP_LANES.to_vec(),
            dir: -1.0,
        })
    };
    match id {
        // City.js: from the merge to the end; the road carries on 900 m.
        "sierra" => WorldData {
            runout: 900.0,
            opposite_carriageway: opp(6419.0, 9379.0),
        },
        // Harbor.js: from the bridge's west end to the end; 700 m on.
        "coast" => WorldData {
            runout: 700.0,
            opposite_carriageway: opp(4865.0, 7665.0),
        },
        // City.js on a loop: all the way round.
        "cruise" => WorldData {
            runout: 0.0,
            opposite_carriageway: opp(0.0, 14320.0),
        },
        _ => WorldData {
            runout: 0.0,
            opposite_carriageway: None,
        },
    }
}
