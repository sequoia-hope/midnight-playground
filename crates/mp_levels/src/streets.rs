//! Level 3 — "Downtown Streets" (port of `src/levels/streets.js`). A
//! midnight street race through Meridian's grid: the Neon District's shopping
//! streets, the switchback-free but very steep streets of Nob Hill (level at
//! every crossing, so the car flies off each one going downhill), and the
//! towers of the Financial District.
//!
//! The route is laid out on the street grid rather than written as turtle
//! segments by hand: a list of moves from crossing to crossing, turned into
//! straights and 90° corners whose lengths are chosen so every straight runs
//! exactly down the middle of a grid street. The level also owns the ground
//! (flat blocks, and a ridge under Nob Hill with the streets stepped across
//! it), which the road, the terrain and the scenery all read.

use std::sync::Arc;

use crate::{police, zone};
use mp_math::{DEG, js, kernel, smoothstep};
use mp_track::{Level, Mode, Route, Segment, SkyKey, seg, trapezoid};

// ── The grid ─────────────────────────────────────────────────────
pub const PX: f64 = 130.0; // crossing spacing east–west (m)
pub const PZ: f64 = 105.0; // and north–south
pub const HW: f64 = 7.0; // kerb-to-centre (matches roadTypes.street)
pub const WALK: f64 = 4.5; // pavement width
pub const FLAT: f64 = 22.0; // level stretch either side of a crossing on the hill
pub const EASE: f64 = 15.0; // rounding where a hill street tips over (m)
pub const BASE: f64 = 20.0; // street level away from the hill

/// Nob Hill: an east–west ridge. Heights at crossings; streets ramp between.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ridge {
    pub z: f64,
    pub x0: f64,
    pub x1: f64,
    pub fade: f64,
    pub a: f64,
    pub sigma: f64,
}

pub const RIDGE: Ridge = Ridge {
    z: 6.0 * PZ,
    x0: 12.0 * PX - 60.0,
    x1: 16.0 * PX + 60.0,
    fade: 250.0,
    a: 34.0,
    sigma: 200.0,
};

pub fn ridge(x: f64, z: f64) -> f64 {
    let w = smoothstep(RIDGE.x0 - RIDGE.fade, RIDGE.x0, x)
        * (1.0 - smoothstep(RIDGE.x1, RIDGE.x1 + RIDGE.fade, x));
    if w <= 0.0 {
        return 0.0;
    }
    let d = (z - RIDGE.z) / RIDGE.sigma;
    RIDGE.a * kernel::exp(-0.5 * d * d) * w
}

pub fn crossing_y(i: f64, j: f64) -> f64 {
    BASE + ridge(i * PX, j * PZ)
}

/// 0 → 1 across a block: level round each crossing, a straight ramp between
/// with rounded ends (a trapezoid slope, integrated).
fn ramp(t: f64, p: f64) -> f64 {
    let a = FLAT;
    let b = p - FLAT;
    let l = b - a;
    let m = 1.0 / (l - EASE);
    if t <= a {
        return 0.0;
    }
    if t >= b {
        return 1.0;
    }
    if t < a + EASE {
        return (m * kernel::pow(t - a, 2.0)) / (2.0 * EASE);
    }
    if t > b - EASE {
        return 1.0 - (m * kernel::pow(b - t, 2.0)) / (2.0 * EASE);
    }
    m * (EASE / 2.0 + (t - a - EASE))
}

/// Ground height anywhere: a bilinear blend of the four surrounding
/// crossings, eased so every street is level across and stepped along.
pub fn ground(x: f64, z: f64) -> f64 {
    let fi = x / PX;
    let fj = z / PZ;
    let i = fi.floor();
    let j = fj.floor();
    let u = ramp((fi - i) * PX, PX);
    let v = ramp((fj - j) * PZ, PZ);
    let a = crossing_y(i, j);
    let b = crossing_y(i + 1.0, j);
    let c = crossing_y(i, j + 1.0);
    let d = crossing_y(i + 1.0, j + 1.0);
    (a + (b - a) * u) * (1.0 - v) + (c + (d - c) * u) * v
}

/// Districts by grid column: Neon District west of 12, Nob Hill to 16.
pub fn district(i: i32) -> u32 {
    if i < 12 {
        0
    } else if i <= 16 {
        1
    } else {
        2
    }
}

// ── The route ────────────────────────────────────────────────────
/// The road begins mid-block, heading east.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Start {
    pub i: i32,
    pub j: i32,
    pub x: f64,
}
pub const START: Start = Start {
    i: 0,
    j: 0,
    x: 20.0,
};

/// Extras on a move apply from its start (zone, road, tag).
struct Move {
    dir: char,
    blocks: i32,
    zone: Option<u8>,
    road: Option<&'static str>,
    tag: Option<&'static str>,
}

const fn mv(dir: char, blocks: i32) -> Move {
    Move {
        dir,
        blocks,
        zone: None,
        road: None,
        tag: None,
    }
}

const fn mvt(dir: char, blocks: i32, tag: &'static str) -> Move {
    Move {
        tag: Some(tag),
        ..mv(dir, blocks)
    }
}

const MOVES: [Move; 17] = [
    // ── NEON DISTRICT ──
    Move {
        zone: Some(0),
        road: Some("street"),
        tag: Some("start"),
        ..mv('E', 4)
    },
    mv('S', 2),
    mvt('E', 2, "arcade"),
    mv('N', 1),
    mvt('E', 3, "el"),
    mv('S', 2),
    mv('E', 3),
    // ── NOB HILL ──
    Move {
        zone: Some(1),
        tag: Some("hill-down"),
        ..mv('S', 6)
    },
    mv('E', 2),
    mvt('N', 3, "hill-up"),
    mvt('E', 2, "crest"),
    mvt('S', 3, "hill-down"),
    // ── FINANCIAL DISTRICT ──
    Move {
        zone: Some(2),
        ..mv('E', 3)
    },
    mv('N', 2),
    mvt('E', 4, "towers"),
    mv('S', 1),
    mvt('E', 4, "finish"),
];
pub const CORNER: f64 = 48.0; // length of a 90° corner (minimum radius ≈ 21 m)
pub const END_RUN: f64 = 45.0; // the last move stops this far short of its crossing

fn dirs(d: char) -> (i32, i32) {
    match d {
        'E' => (1, 0),
        'S' => (0, 1),
        'W' => (-1, 0),
        _ => (0, -1),
    }
}

fn head(d: char) -> f64 {
    match d {
        'E' => 0.0,
        'S' => 90.0,
        'W' => 180.0,
        _ => -90.0,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Turtle {
    x: f64,
    z: f64,
    h: f64,
}

// The same easing Track.walkSegments uses, so the turtle here lands where
// the real one does.
fn walk(state: &mut Turtle, len: f64, turn_deg: f64) {
    let r = if turn_deg.abs() > 120.0 { 0.22 } else { 0.32 };
    let k_peak = (turn_deg * DEG) / (len * (1.0 - r));
    let mut k = 0.0;
    while k < len {
        let kap = k_peak * trapezoid((k + 0.5) / len, r);
        state.h += kap;
        state.x += kernel::cos(state.h - kap * 0.5);
        state.z += kernel::sin(state.h - kap * 0.5);
        k += 1.0;
    }
}

/// Setback of a corner: how far before the crossing a 90° corner starts.
fn corner_setback(len: f64) -> f64 {
    let mut s = Turtle::default();
    walk(&mut s, len, 90.0);
    s.x
}

/// A straight run on a grid street, for the scenery.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leg {
    pub dir: char,
    pub x0: f64,
    pub z0: f64,
    pub x1: f64,
    pub z1: f64,
    pub line: i32,
    /// 'z' for an east–west street (constant z), 'x' for north–south.
    pub axis: char,
}

#[derive(Clone, Debug)]
pub struct StreetRoute {
    pub segs: Vec<Segment>,
    pub legs: Vec<Leg>,
    pub setback: f64,
}

pub fn build_route() -> StreetRoute {
    let t = corner_setback(CORNER);
    let mut segs = Vec::new();
    let mut turtle = Turtle {
        x: START.x,
        z: START.j as f64 * PZ,
        h: 0.0,
    };
    let (mut ci, mut cj) = (START.i, START.j);
    let mut legs = Vec::new(); // straight runs on grid streets, for the scenery
    for (m, mov) in MOVES.iter().enumerate() {
        let (dx, dz) = dirs(mov.dir);
        let (ti, tj) = (ci + dx * mov.blocks, cj + dz * mov.blocks);
        let next = MOVES.get(m + 1);
        let (tx, tz) = (ti as f64 * PX, tj as f64 * PZ);
        // Distance left along this street to the target crossing.
        let along = if dx != 0 {
            (tx - turtle.x) * dx as f64
        } else {
            (tz - turtle.z) * dz as f64
        };
        let stop = if next.is_some() { t } else { END_RUN };
        let len = js::max(1.0, js::round(along - stop));
        let (x0, z0) = (turtle.x, turtle.z);
        walk(&mut turtle, len, 0.0);
        legs.push(Leg {
            dir: mov.dir,
            x0,
            z0,
            x1: turtle.x,
            z1: turtle.z,
            line: if dx != 0 { cj } else { ci },
            axis: if dx != 0 { 'z' } else { 'x' },
        });
        let mut s = seg(len, 0.0, 0.0);
        s.extra.zone = mov.zone;
        s.extra.road = mov.road;
        s.extra.tag = mov.tag;
        segs.push(s);
        if let Some(next) = next {
            let mut turn = head(next.dir) - head(mov.dir);
            if turn > 180.0 {
                turn -= 360.0;
            }
            if turn < -180.0 {
                turn += 360.0;
            }
            walk(&mut turtle, CORNER, turn);
            segs.push(seg(CORNER, turn, 0.0).tag("corner").cross(ti, tj));
            // Snap the heading error away (it is ~1e-4 rad) so straights stay on the grid.
            turtle.h = head(next.dir) * DEG;
        }
        ci = ti;
        cj = tj;
    }
    StreetRoute {
        segs,
        legs,
        setback: t,
    }
}

pub fn level() -> Level {
    let route = build_route();
    // ── Look ─────────────────────────────────────────────────────────
    // Midnight all the way; the haze gets a little thicker downtown.
    let night = sky!(
        0.0, -16, 0x04050d, 0x2a1a36, 0x9ab0e0, 0.3, 0x2e3456, 0x1a1420, 0.34, 0x21182c, 0.00055,
        1.22, 1
    );
    Level {
        id: "streets",
        mode: Mode::Race,
        num: "LEVEL 3",
        title: "Downtown Streets",
        desc: "A midnight street race through the grid of downtown Meridian: ninety-degree corners under the Neon District signs, flat-out jumps over the crossings of Nob Hill, and a sprint between the towers to the finish.",
        laps: None,
        lap_length: None,
        start_height: Some(BASE),
        start_heading: Some(0.),
        start_x: Some(START.x),
        start_z: Some(START.j as f64 * PZ),
        finish_runoff: Some(170.),
        elevation_smooth: Some(2.),
        route: Route::Segments(route.segs),
        elevation: Some(Arc::new(ground)),
        ground: Some(Arc::new(ground)),
        loose_ground: None,
        sea_y: None,
        zones: vec![
            zone(
                "neon",
                "NEON DISTRICT",
                "Shopping streets after dark",
                "streets",
                "Streets",
                "#e0409a",
            ),
            zone(
                "hill",
                "NOB HILL",
                "Level at every crossing",
                "streets",
                "Streets",
                "#e0a040",
            )
            .blend(200.),
            zone(
                "fin",
                "FINANCIAL DISTRICT",
                "Sprint through the towers",
                "streets",
                "Streets",
                "#40b8e0",
            )
            .blend(200.),
        ],
        sky: vec![
            SkyKey { s: 0.0, ..night },
            SkyKey {
                s: 0.5,
                hor: 0x241a38,
                fog: 0x1c1830,
                fog_d: 0.00045,
                ..night
            },
            SkyKey {
                s: 1.0,
                hor: 0x301c34,
                fog: 0x241a2c,
                fog_d: 0.0006,
                ..night
            },
        ],
        sun_azimuth: 0.9,
        moon_dir: None,
        traffic_paint: Some(vec![
            0xf2c318, 0xf2c318, 0xf2c318, 0xc9ccd1, 0x2b2f36, 0x8a1c1c, 0x1d3f75, 0xe8e6df,
            0x9aa3ad, 0x2e6d8e,
        ]),
        traffic: vec![
            traffic!(
                [70, 150],
                [
                    ("sedan", 0.4),
                    ("hatch", 0.25),
                    ("van", 0.2),
                    ("boxtruck", 0.15)
                ],
                0.5,
                [10, 14]
            ),
            traffic!(
                [110, 220],
                [
                    ("sedan", 0.45),
                    ("hatch", 0.3),
                    ("van", 0.15),
                    ("pickup", 0.1)
                ],
                0.5,
                [8, 12]
            ),
            traffic!(
                [60, 130],
                [
                    ("sedan", 0.45),
                    ("hatch", 0.15),
                    ("van", 0.2),
                    ("boxtruck", 0.2)
                ],
                0.5,
                [10, 15]
            ),
        ],
        // Hot Pursuit: every corner in the grid breaks line of sight, so the
        // city is where you lose them; heat builds 3 / 4 / 5.
        police: Some(police(&[3, 4, 5], &[false, false, false])),
        rivals: vec![
            rival!("Razor", "super", 0xe9ecef, 0.985, 520),
            rival!("Kaito", "sports", 0x19c46b, 0.972, 505),
            rival!("Vex", "muscle", 0x8b2cff, 0.958, 540),
            rival!("Juno", "electric", 0x6fe0ff, 0.945, 500),
            rival!("Rook", "rally", 0x2a6ee8, 0.93, 505),
        ],
    }
}
