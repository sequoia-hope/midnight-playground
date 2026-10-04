//! What the simulation needs from the world that the JS computes in scenery
//! code rather than in the level files (SPEC 4.3, "Data the world gives the
//! simulation"):
//!
//! - `Track.runout`, the drivable road past the last sample: City raises it
//!   to 900 m on a point-to-point road (`City.js:48`), Harbor to 700 m
//!   (`Harbor.js:57`), Streets sets it to 0 (`Streets.js:73`); it starts at 0
//!   (`Track.js:143`).
//! - `world.oppositeCarriageway` (`City.js:222`, `Harbor.js:179`): the range
//!   (`s0`, `s1`), the lane offsets (`OPP_LANES`, `city/freeway.js:26`), the
//!   direction, and the height of the far carriageway, which is ported as its
//!   formula, `oppY` (`city/freeway.js:64`).
//!
//! Both are computed here from the level and its Track by the scenery's own
//! rules (roadmap WP 3.9, DECISIONS D471): [`City`] and [`Harbor`] are the
//! parts of those modules' `plan()` and `build()` that decide them, and
//! [`level_world_data`] runs them as `World.build` runs the modules (each
//! `plan()` in the level's order, then each `build()`). mr_worldgen's ported
//! City computes its own through the same functions, so the simulation and
//! the world cannot disagree; the simulation needs no world build for them.
//! `tests/world_data.rs` holds the result to the JS world's dump
//! (`tools/parity/sim-world.mjs`, `parity/golden/sim/world-data.json`).

use mr_math::js;
use mr_track::{Frame, Level, Track};

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

    /// `{ s0, s1: t.length, lanes: OPP_LANES.slice(), dir: -1 }`, as both
    /// City and Harbor set it.
    fn westbound(s0: f64, t: &Track) -> OppositeCarriageway {
        OppositeCarriageway {
            s0,
            s1: t.length,
            lanes: OPP_LANES.to_vec(),
            dir: -1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorldData {
    pub runout: f64,
    pub opposite_carriageway: Option<OppositeCarriageway>,
}

/// `City.js`'s rules for the world data (Meridian: Sierra's zone 2, the
/// whole cruise loop).
pub struct City;

impl City {
    /// `FWD_EXT`: both directions carry on past the finish.
    pub const FWD_EXT: f64 = 900.0;

    /// `this.sA`: where the westbound carriageway joins, the on-ramp merge
    /// (`t.tag('merge')[0]?.s0 ?? Z.s0 + 220`); 0 on a loop.
    pub fn s_a(t: &Track, zone: usize) -> f64 {
        if t.is_loop {
            return 0.0;
        }
        let z0 = t.zones[zone].s0;
        t.tag("merge").first().map_or(z0 + 220.0, |g| g.s0)
    }

    /// `plan()`: the runout after it (unchanged on a loop).
    pub fn plan_runout(t: &Track, runout: f64) -> f64 {
        if t.is_loop {
            return runout;
        }
        js::max(runout, City::FWD_EXT) // drivable after the finish
    }

    /// `build()`'s `world.oppositeCarriageway`: from the merge to the end;
    /// on a loop, all the way round.
    pub fn opposite_carriageway(t: &Track, zone: usize) -> OppositeCarriageway {
        OppositeCarriageway::westbound(City::s_a(t, zone), t)
    }
}

/// `Harbor.js`'s rules for the world data (Port Meridian: Coast's zone 2).
pub struct Harbor;

impl Harbor {
    /// `FWD_EXT`: buildForward draws it.
    pub const FWD_EXT: f64 = 700.0;

    /// `this.sWS`: the westbound carriageway starts just before the climb;
    /// west of that it peels away to the right into the terminal gate.
    pub fn s_ws(t: &Track, zone: usize) -> f64 {
        let z0 = t.zone_start[zone] as f64;
        let up = t.tag("bridge-up").first().map(|g| g.s0);
        (match up {
            Some(s0) => s0,
            None => z0 + 200.0,
        }) - 40.0
    }

    /// `plan()`: the runout after it.
    pub fn plan_runout(runout: f64) -> f64 {
        js::max(runout, Harbor::FWD_EXT) // buildForward draws it; drivable after the finish
    }

    /// `build()`'s `world.oppositeCarriageway`: westbound lanes alongside
    /// our track.
    pub fn opposite_carriageway(t: &Track, zone: usize) -> OppositeCarriageway {
        OppositeCarriageway::westbound(Harbor::s_ws(t, zone) + 40.0, t)
    }
}

/// The scenery modules `World.loadScenery` makes for a level: each distinct
/// `zone.scenery` in order of first appearance, with that first zone.
pub fn scenery_modules(level: &Level) -> Vec<(&'static str, usize)> {
    let mut out: Vec<(&'static str, usize)> = Vec::new();
    for (k, z) in level.zones.iter().enumerate() {
        if !z.scenery.is_empty() && !out.iter().any(|&(n, _)| n == z.scenery) {
            out.push((z.scenery, k));
        }
    }
    out
}

/// What a module's `plan()` leaves `track.runout` at, given what it was.
pub fn plan_runout(name: &str, t: &Track, runout: f64) -> f64 {
    match name {
        "City" => City::plan_runout(t, runout),
        "Harbor" => Harbor::plan_runout(runout),
        "Streets" => 0.0,
        _ => runout,
    }
}

/// The opposite carriageway a module's `build()` sets, if it sets one.
pub fn built_carriageway(name: &str, t: &Track, zone: usize) -> Option<OppositeCarriageway> {
    match name {
        "City" => Some(City::opposite_carriageway(t, zone)),
        "Harbor" => Some(Harbor::opposite_carriageway(t, zone)),
        _ => None,
    }
}

/// The world data of a level whose Track is `t`, as its scenery sets it:
/// every module's `plan()` in order (the runout), then every `build()` (the
/// last carriageway set wins, as the JS assigns `world.oppositeCarriageway`).
pub fn level_world_data(level: &Level, t: &Track) -> WorldData {
    let modules = scenery_modules(level);
    let mut runout = t.runout;
    for &(name, _) in &modules {
        runout = plan_runout(name, t, runout);
    }
    let mut opposite_carriageway = None;
    for &(name, zone) in &modules {
        if let Some(oc) = built_carriageway(name, t, zone) {
            opposite_carriageway = Some(oc);
        }
    }
    WorldData {
        runout,
        opposite_carriageway,
    }
}

/// The world data of a level, by id: [`level_world_data`] on a Track built
/// for it. A level without a City or a Harbor (Seaside among them, whose
/// Track needs its survey) has neither a runout nor a carriageway, and no
/// Track is built for it.
pub fn world_data(id: &str) -> WorldData {
    let level = crate::level_by_id(id);
    let needs_track = scenery_modules(&level)
        .iter()
        .any(|&(n, _)| matches!(n, "City" | "Harbor"));
    if !needs_track {
        return WorldData {
            runout: 0.0,
            opposite_carriageway: None,
        };
    }
    let t = Track::new(&level).expect("the level builds a track");
    level_world_data(&level, &t)
}
