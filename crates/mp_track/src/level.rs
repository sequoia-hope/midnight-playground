//! Level definition types: what a file in `src/levels/` is, as data
//! (SPEC 3.1). The levels themselves live in `mp_levels`.
//!
//! The JS levels are object literals read with `??` and `||` defaults at the
//! point of use; fields that a level may leave out are `Option`s here, and
//! the defaults stay where the JS applies them.

use std::fmt;
use std::sync::Arc;

/// A function of ground position, as the JS levels carry them (`elevation`,
/// `ground`, `looseGround`).
pub type GroundFn = Arc<dyn Fn(f64, f64) -> f64 + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Race,
    Cruise,
}

/// `extras` of a turtle segment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SegExtra {
    pub zone: Option<u8>,
    pub road: Option<&'static str>,
    pub tag: Option<&'static str>,
    /// On a bridge/viaduct; the ground isn't raised to it.
    pub elevated: bool,
    /// Downtown Streets' corners: the grid crossing they turn at.
    pub cross: Option<(i32, i32)>,
}

/// `[length m, turn deg, rise m, extras]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub len: f64,
    pub turn: f64,
    pub rise: f64,
    pub extra: SegExtra,
}

/// A segment with no extras.
pub const fn seg(len: f64, turn: f64, rise: f64) -> Segment {
    Segment {
        len,
        turn,
        rise,
        extra: SegExtra {
            zone: None,
            road: None,
            tag: None,
            elevated: false,
            cross: None,
        },
    }
}

impl Segment {
    pub fn zone(mut self, z: u8) -> Self {
        self.extra.zone = Some(z);
        self
    }
    pub fn road(mut self, r: &'static str) -> Self {
        self.extra.road = Some(r);
        self
    }
    pub fn tag(mut self, t: &'static str) -> Self {
        self.extra.tag = Some(t);
        self
    }
    pub fn elevated(mut self) -> Self {
        self.extra.elevated = true;
        self
    }
    pub fn cross(mut self, i: i32, j: i32) -> Self {
        self.extra.cross = Some((i, j));
        self
    }
}

/// A loop's named stretch: by fraction of the loop (`f0`/`f1`) or by metres
/// (`s0`/`s1`); `elevated` lifts the road that many metres on long ramps.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopTag {
    pub tag: &'static str,
    pub s0: Option<f64>,
    pub s1: Option<f64>,
    pub f0: Option<f64>,
    pub f1: Option<f64>,
    pub elevated: Option<f64>,
}

/// A loop's road type over a stretch (`spec.roads`).
#[derive(Clone, Debug, PartialEq)]
pub struct LoopRoad {
    pub s0: i64,
    pub s1: i64,
    pub road: &'static str,
}

/// What `loop.path()` returns: the closed centreline, and optionally
/// surveyed heights, camber, half-widths, barrier distances and run-off
/// grades at the same points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoopPath {
    pub x: Vec<f64>,
    pub z: Vec<f64>,
    pub y: Option<Vec<f64>>,
    pub bank: Option<Vec<f64>>,
    pub hw: Option<Vec<f64>>,
    pub wall_l: Option<Vec<f64>>,
    pub wall_r: Option<Vec<f64>>,
    pub run_l: Option<Vec<f64>>,
    pub run_r: Option<Vec<f64>>,
}

/// `loop.path`: computed when the Track is built, as in the JS. Seaside's
/// fails until its survey data is loaded (`prepare()`).
pub type PathFn = Arc<dyn Fn() -> Result<LoopPath, String> + Send + Sync>;

#[derive(Clone)]
pub struct LoopSpec {
    pub path: PathFn,
    pub base_y: Option<f64>,
    pub tags: Vec<LoopTag>,
    pub road: Option<&'static str>,
    pub roads: Vec<LoopRoad>,
    pub start_s: Option<f64>,
    pub elevation_smooth: Option<f64>,
    /// Zone starts in metres into the lap (`spec.zones`).
    pub zones: Option<Vec<f64>>,
}

impl fmt::Debug for LoopSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoopSpec")
            .field("base_y", &self.base_y)
            .field("tags", &self.tags)
            .field("road", &self.road)
            .field("start_s", &self.start_s)
            .field("zones", &self.zones)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub enum Route {
    Segments(Vec<Segment>),
    Loop(LoopSpec),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub key: &'static str,
    pub name: &'static str,
    pub sub: &'static str,
    pub landform: &'static str,
    pub scenery: &'static str,
    pub color: &'static str,
    pub blend: Option<f64>,
    pub blend_offset: Option<f64>,
}

impl Zone {
    pub fn blend(mut self, b: f64) -> Self {
        self.blend = Some(b);
        self
    }
    pub fn blend_offset(mut self, b: f64) -> Self {
        self.blend_offset = Some(b);
        self
    }
}

/// A time-of-day keyframe; `s` is a fraction of the track length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyKey {
    pub s: f64,
    pub sun_el: f64,
    pub zen: u32,
    pub hor: u32,
    pub sun: u32,
    pub sun_i: f64,
    pub hemi_s: u32,
    pub hemi_g: u32,
    pub hemi_i: f64,
    pub fog: u32,
    pub fog_d: f64,
    pub exp: f64,
    pub night: f64,
}

/// One zone's traffic: spacing, the mix of kinds (in the JS order, which
/// the draws follow), how much comes the other way, speeds, and the share
/// on the far carriageway.
#[derive(Clone, Debug, PartialEq)]
pub struct TrafficRule {
    pub gap: [f64; 2],
    pub mix: Vec<(&'static str, f64)>,
    pub oncoming: f64,
    pub speed: [f64; 2],
    pub opposite: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Police {
    pub heat_cap: Vec<u32>,
    pub los_open_ground: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RivalDef {
    pub name: &'static str,
    pub kind: &'static str,
    pub color: u32,
    pub skill: f64,
    pub power: f64,
}

#[derive(Clone)]
pub struct Level {
    pub id: &'static str,
    pub mode: Mode,
    pub num: &'static str,
    pub title: &'static str,
    pub desc: &'static str,
    /// A circuit's laps (`L.laps`); `None` elsewhere.
    pub laps: Option<u32>,
    /// Metres round a circuit's lap, as the menu shows it.
    pub lap_length: Option<f64>,
    pub start_height: Option<f64>,
    pub start_heading: Option<f64>,
    pub start_x: Option<f64>,
    pub start_z: Option<f64>,
    pub finish_runoff: Option<f64>,
    pub elevation_smooth: Option<f64>,
    pub route: Route,
    /// The level owns the ground the road sits on (Downtown Streets).
    pub elevation: Option<GroundFn>,
    /// The level's own ground for the terrain (Streets, Seaside).
    pub ground: Option<GroundFn>,
    /// Off the tarmac: 0 on paved run-off, 1 on dirt and grass (Seaside).
    pub loose_ground: Option<GroundFn>,
    pub sea_y: Option<f64>,
    pub zones: Vec<Zone>,
    pub sky: Vec<SkyKey>,
    pub sun_azimuth: f64,
    pub moon_dir: Option<[f64; 3]>,
    pub traffic_paint: Option<Vec<u32>>,
    pub traffic: Vec<TrafficRule>,
    pub police: Option<Police>,
    pub rivals: Vec<RivalDef>,
}

impl Level {
    pub fn is_loop(&self) -> bool {
        matches!(self.route, Route::Loop(_))
    }

    pub fn segments(&self) -> Option<&[Segment]> {
        match &self.route {
            Route::Segments(s) => Some(s),
            Route::Loop(_) => None,
        }
    }

    pub fn loop_spec(&self) -> Option<&LoopSpec> {
        match &self.route {
            Route::Loop(l) => Some(l),
            Route::Segments(_) => None,
        }
    }
}

impl fmt::Debug for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Level")
            .field("id", &self.id)
            .field("mode", &self.mode)
            .field("route", &self.route)
            .field("zones", &self.zones)
            .finish_non_exhaustive()
    }
}
