//! A finisher's parking spot (`Race.parkSpot`): in the JS an object holding
//! two closures, `speed(s)` and `lat(s)`; here the data they close over
//! (SPEC 4.3, "plain data, no closures"). Race hands one to a rival that has
//! finished, and to the player's cool-down driver.

use mr_math::{js, lerp, smoothstep, stop_speed};
use mr_track::Track;

pub const COOL_CRUISE: f64 = 24.0; // m/s
pub const COOL_DECEL: f64 = 1.8; // m/s², the final roll to a stop
pub const PARK_GAP: f64 = 240.0; // front row, metres short of the road's end
pub const PARK_ROW: f64 = 13.0; // spacing of the rows behind it

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Park {
    /// A circuit has no end to park at: a slow lap on the right-hand side,
    /// easing off for the corners.
    Circuit,
    /// A lane on the road past the finish, stopping at `stop_at`.
    Lane {
        stop_at: f64,
        lane_lat: f64,
        s0: f64,
        lat0: f64,
    },
}

impl Park {
    pub fn speed(&self, t: &Track, s: f64) -> f64 {
        match *self {
            Park::Circuit => {
                let p = |d: f64| t.speed_profile[t.idx(s + d)] as f64 * 0.7;
                js::min_n(&[20.0, p(0.0), p(25.0), p(50.0)])
            }
            Park::Lane { stop_at, .. } => js::min(COOL_CRUISE, stop_speed(stop_at - s, COOL_DECEL)),
        }
    }

    pub fn lat(&self, t: &Track, s: f64) -> f64 {
        match *self {
            Park::Circuit => t.hw[t.idx(s)] as f64 - 2.2,
            Park::Lane {
                lane_lat, s0, lat0, ..
            } => lerp(lat0, lane_lat, smoothstep(s0, s0 + 300.0, s)),
        }
    }
}
