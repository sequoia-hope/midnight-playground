//! Road cross-section presets shared by every level (port of
//! `src/track/roadTypes.js`).
//!
//! hw = half the paved width (m); margin = how far past the paved edge the
//! barrier/cliff sits; edge = what lines the road: 'terrain' (rock wall or
//! guardrail, decided from the ground), 'fence', 'jersey' (concrete barrier),
//! 'rail' (kerb + railing), 'curb' (kerb and pavement, built by the level's
//! scenery), 'circuit' (run-off and barriers built by the level's scenery) or
//! 'none'. marks = the painted line scheme (see Road.buildMarkings); bank
//! scales the automatic banking in corners (default 1).
//!
//! New types go on the end: tracks store the index into this list.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadType {
    pub key: &'static str,
    pub hw: f64,
    pub margin: f64,
    pub lanes: u32,
    pub edge: &'static str,
    pub tone: u32,
    pub marks: &'static str,
    /// `bank` in the JS, where it is given (`rt.bank ?? 1`).
    pub bank: Option<f64>,
    /// Off the tarmac the car loses speed (circuits).
    pub runoff: bool,
}

const fn rt(
    key: &'static str,
    hw: f64,
    margin: f64,
    lanes: u32,
    edge: &'static str,
    tone: u32,
    marks: &'static str,
) -> RoadType {
    RoadType {
        key,
        hw,
        margin,
        lanes,
        edge,
        tone,
        marks,
        bank: None,
        runoff: false,
    }
}

/// `ROAD_TYPES` in key order; `ROAD_KEYS[i]` is `ROAD_TYPES[i].key`.
pub const ROAD_TYPES: [RoadType; 10] = [
    rt("mountain", 5.4, 1.4, 2, "terrain", 0, "double"),
    rt("valley", 5.2, 1.6, 2, "fence", 1, "dashed"),
    rt("freeway", 9.4, 0.5, 4, "jersey", 2, "freeway"),
    rt("coastal", 5.6, 1.4, 2, "terrain", 0, "double"),
    rt("boulevard", 8.0, 1.1, 4, "rail", 1, "boulevard"),
    // Downtown Streets: two lanes each way between kerbs; the wall is the
    // kerb. Level across (no banking) so the pavements line up.
    RoadType {
        bank: Some(0.0),
        ..rt("street", 7.0, 0.25, 4, "curb", 2, "avenue")
    },
    // Desert: a two-lane highway and the open dry-lake course.
    rt("desert", 5.6, 2.2, 2, "none", 0, "dashed"),
    rt("playa", 9.0, 3.0, 4, "none", 0, "guide"),
    // Seaside Raceway: a race circuit, 12 m of tarmac (15 m down the pit
    // straight) with white edge lines. Past the edge is run-off out to the
    // surveyed walls (the track carries them); the scenery lays the kerbs,
    // run-off and barriers. Off the tarmac the car loses speed (runoff).
    RoadType {
        runoff: true,
        ..rt("circuit", 6.0, 12.0, 2, "circuit", 1, "circuit")
    },
    RoadType {
        runoff: true,
        ..rt("circuitWide", 7.5, 12.0, 2, "circuit", 1, "circuit")
    },
];

/// `ROAD_KEYS.indexOf(key)`: -1 when there is no such type, as in the JS
/// (a `Uint8Array` then stores 255).
pub fn road_index(key: &str) -> i32 {
    ROAD_TYPES
        .iter()
        .position(|r| r.key == key)
        .map_or(-1, |i| i as i32)
}

/// `ROAD_TYPES[key]`.
pub fn road_type(key: &str) -> Option<&'static RoadType> {
    ROAD_TYPES.iter().find(|r| r.key == key)
}
