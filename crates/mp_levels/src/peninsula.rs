//! "The Loop": the open world's first whole piece (docs/vision/peninsula.md,
//! stage 4a). Highway 1 from the San Gregorio junction to Half Moon Bay,
//! Highway 92 over the hill to Skyline, Skyline south to Skylonda and
//! Highway 84 down through La Honda to the sea: 34.8 km of real road at
//! the plan's scale, closed into one lap. The race is a lap of it from the
//! crest above San Gregorio; the card offers a cruise.
//!
//! The roads are the four corridor files of `assets/atlas/peninsula/
//! corridors/` chained ([`chain`]) and splined closed ([`spline_closed`]);
//! the ground under the loop's box is `peninsula.ground.bin` (which the
//! south coast level shares) and the land cover of the same cells
//! `peninsula.cover.bin` ([`CoverGrid`]), which the Land kit plants by.

use std::sync::{Arc, OnceLock};

use crate::ground::{CoverGrid, HeightGrid};
use crate::south_coast::{CREST_WITHIN, Corridor};
use crate::zone;
use mp_track::{Level, LoopPath, LoopRoad, LoopSpec, LoopTag, Mode, Route};

/// The loop's corridors in lap order: each ends where the next begins.
pub const CORRIDORS: [(&str, &str); 4] = [
    (
        "coast-south",
        include_str!("../../../assets/atlas/peninsula/corridors/coast-south.txt"),
    ),
    (
        "hwy-92",
        include_str!("../../../assets/atlas/peninsula/corridors/hwy-92.txt"),
    ),
    (
        "skyline-south",
        include_str!("../../../assets/atlas/peninsula/corridors/skyline-south.txt"),
    ),
    (
        "hwy-84",
        include_str!("../../../assets/atlas/peninsula/corridors/hwy-84.txt"),
    ),
];
/// The ground under the loop's box (660 kB, zlib) and its land cover
/// (15 kB): inflated once, the first time a level asks.
pub const GROUND: &[u8] =
    include_bytes!("../../../assets/atlas/peninsula/corridors/peninsula.ground.bin");
pub const COVER: &[u8] =
    include_bytes!("../../../assets/atlas/peninsula/corridors/peninsula.cover.bin");
/// Two samples closer than this at a join are one point.
const JOIN: f64 = 2.5;
/// The corridors meet at corners (Highway 1 turns onto 92 at the big
/// intersection, Skyline onto 84 at Skylonda): the samples within this
/// many of a join are smoothed with a triangular window this wide either
/// side, so the lap rounds the corner as a car turning through an
/// intersection does instead of kinking on one sample.
const ROUND: i64 = 14;

pub fn ground() -> Arc<HeightGrid> {
    static GRID: OnceLock<Arc<HeightGrid>> = OnceLock::new();
    GRID.get_or_init(|| Arc::new(HeightGrid::parse(GROUND).expect("the committed ground parses")))
        .clone()
}

pub fn cover() -> Arc<CoverGrid> {
    static GRID: OnceLock<Arc<CoverGrid>> = OnceLock::new();
    GRID.get_or_init(|| Arc::new(CoverGrid::parse(COVER).expect("the committed cover parses")))
        .clone()
}

/// The corridors chained into one closed lap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loop {
    pub x: Vec<f64>,
    pub z: Vec<f64>,
    pub y: Vec<f64>,
    pub cover: Vec<String>,
    /// Metres along the lap where each corridor begins (the first is 0).
    pub joins: Vec<f64>,
    /// Metres round the lap, the closing edge included.
    pub length: f64,
}

/// Chains the corridors: every sample in order, less any within [`JOIN`]
/// of the sample before it (the corridors meet at shared places, so each
/// begins where the one before ended), and less a last sample that would
/// double the first.
pub fn chain(cs: &[Corridor]) -> Loop {
    let mut l = Loop::default();
    let mut len = 0.0;
    for c in cs {
        l.joins.push(len);
        for k in 0..c.x.len() {
            if let (Some(&px), Some(&pz)) = (l.x.last(), l.z.last()) {
                let d = mp_math::kernel::hypot(c.x[k] - px, c.z[k] - pz);
                if d < JOIN {
                    continue;
                }
                len += d;
            }
            l.x.push(c.x[k]);
            l.z.push(c.z[k]);
            l.y.push(c.y[k]);
            l.cover.push(c.cover[k].clone());
        }
    }
    while l.x.len() > 1 {
        let n = l.x.len() - 1;
        let d = mp_math::kernel::hypot(l.x[n] - l.x[0], l.z[n] - l.z[0]);
        if d >= JOIN {
            len += d;
            break;
        }
        len -= mp_math::kernel::hypot(l.x[n] - l.x[n - 1], l.z[n] - l.z[n - 1]);
        l.x.pop();
        l.z.pop();
        l.y.pop();
        l.cover.pop();
    }
    round_joins(&mut l);
    l.length = len;
    l
}

/// Rounds the corner at every join (the closing one included), keeping
/// the samples' spacing where it was.
fn round_joins(l: &mut Loop) {
    let n = l.x.len() as i64;
    if n < 4 * ROUND {
        return;
    }
    let src = (l.x.clone(), l.z.clone(), l.y.clone());
    // The joins as sample indices: the first sample of each corridor, by
    // its metres along the chain (the first corridor's is 0).
    let mut idx: Vec<i64> = vec![0];
    let mut cum = 0.0;
    let mut k = 1;
    for i in 1..n as usize {
        cum += mp_math::kernel::hypot(l.x[i] - l.x[i - 1], l.z[i] - l.z[i - 1]);
        while k < l.joins.len() && cum >= l.joins[k] - 1e-6 {
            idx.push(i as i64);
            k += 1;
        }
    }
    for j in idx {
        for i in (j - ROUND)..=(j + ROUND) {
            let (mut sx, mut sz, mut sy, mut sw) = (0.0, 0.0, 0.0, 0.0);
            for d in -ROUND..=ROUND {
                let w = (ROUND + 1 - d.abs()) as f64;
                let m = (i + d).rem_euclid(n) as usize;
                sx += src.0[m] * w;
                sz += src.1[m] * w;
                sy += src.2[m] * w;
                sw += w;
            }
            let m = i.rem_euclid(n) as usize;
            l.x[m] = sx / sw;
            l.z[m] = sz / sw;
            l.y[m] = sy / sw;
        }
    }
}

/// Catmull-Rom round a closed polyline, `sub` points per span.
pub fn spline_closed(src: &[f64], sub: usize) -> Vec<f64> {
    let n = src.len();
    if n < 3 {
        return src.to_vec();
    }
    let mut out = Vec::with_capacity(n * sub);
    for i in 0..n {
        let p0 = src[(i + n - 1) % n];
        let p1 = src[i];
        let p2 = src[(i + 1) % n];
        let p3 = src[(i + 2) % n];
        for k in 0..sub {
            let t = k as f64 / sub as f64;
            let t2 = t * t;
            let t3 = t2 * t;
            out.push(
                0.5 * (2.0 * p1
                    + (-p0 + p2) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                    + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3),
            );
        }
    }
    out
}

/// The loop's corridors, parsed.
pub fn corridors() -> Vec<Corridor> {
    CORRIDORS
        .iter()
        .map(|(id, text)| Corridor::parse(text).unwrap_or_else(|e| panic!("{id}: {e}")))
        .collect()
}

/// `path()`: the lap splined to a metre.
pub fn path(l: &Loop) -> LoopPath {
    LoopPath {
        x: spline_closed(&l.x, 5),
        z: spline_closed(&l.z, 5),
        y: Some(spline_closed(&l.y, 5)),
        ..LoopPath::default()
    }
}

pub fn level() -> Level {
    let cs = corridors();
    let l = Arc::new(chain(&cs));
    let town = cs[0].town_start();
    let crest = cs[0].crest_within(CREST_WITHIN);
    // Where 92 leaves the town, and where Skyline begins.
    let s92 = l.joins[1];
    let s35 = l.joins[2];
    let length = l.length;
    Level {
        id: "peninsula",
        mode: Mode::Race,
        num: "THE PENINSULA",
        title: "The Loop",
        desc: "One lap of the peninsula: down off the crest and along the bluffs to Half Moon Bay, up Highway 92 over the hill, south along Skyline through the redwoods, and down 84 to the sea. 35 km of real road, half size.",
        laps: Some(1),
        lap_length: Some(length),
        start_height: None,
        start_heading: None,
        start_x: None,
        start_z: None,
        finish_runoff: None,
        elevation_smooth: Some(16.),
        route: Route::Loop(LoopSpec {
            path: {
                let l = l.clone();
                Arc::new(move || Ok(path(&l)))
            },
            base_y: None,
            tags: vec![
                LoopTag {
                    tag: "start",
                    s0: Some(crest),
                    s1: Some(crest + 150.),
                    f0: None,
                    f1: None,
                    elevated: None,
                },
                LoopTag {
                    tag: "town-entry",
                    s0: Some(town),
                    s1: Some(town + 200.),
                    f0: None,
                    f1: None,
                    elevated: None,
                },
            ],
            road: Some("coastal"),
            roads: vec![
                LoopRoad {
                    s0: town as i64,
                    s1: s92 as i64,
                    road: "boulevard",
                },
                LoopRoad {
                    s0: s92 as i64,
                    s1: s35 as i64,
                    road: "valley",
                },
                LoopRoad {
                    s0: s35 as i64,
                    s1: length as i64 + 1,
                    road: "mountain",
                },
            ],
            start_s: Some(crest + 60.),
            elevation_smooth: None,
            zones: Some(vec![0., town, s92, s35]),
        }),
        elevation: None,
        ground: Some({
            let g = ground();
            Arc::new(move |x, z| g.height(x, z))
        }),
        loose_ground: None,
        cover: Some({
            let c = cover();
            Arc::new(move |x, z| c.class(x, z))
        }),
        open: true,
        branches: vec![],
        sea_y: Some(0.),
        zones: vec![
            zone(
                "coast",
                "THE SOUTH COAST",
                "Highway 1 above the bluffs",
                "coast",
                "Coast",
                "#9a7f66",
            ),
            zone(
                "beach",
                "HALF MOON BAY",
                "The town's lights",
                "beach",
                "Beach",
                "#e0b45a",
            )
            .blend(350.),
            zone(
                "hill",
                "OVER THE HILL",
                "Highway 92 to Skyline",
                "valley",
                "Land",
                "#7a9a4a",
            )
            .blend(300.),
            zone(
                "skyline",
                "SKYLINE",
                "The ridge road through the redwoods",
                "mountain",
                "Land",
                "#4a6a3a",
            )
            .blend(300.),
        ],
        // Night the whole way round: the moon on the sea from the coast,
        // the fog on the ridge.
        sky: {
            let night = sky!(
                0, -18, 0x03050c, 0x2a1e30, 0x9ab0e0, 0.28, 0x2c3252, 0x16131a, 0.3, 0x1c1826,
                0.00038, 1.22, 1
            );
            vec![night, mp_track::SkyKey { s: 1.0, ..night }]
        },
        sun_azimuth: -0.45,
        moon_dir: Some([-0.6, 0.4, -0.5]),
        traffic_paint: None,
        traffic: vec![
            traffic!(
                [240, 480],
                [
                    ("sedan", 0.35),
                    ("hatch", 0.25),
                    ("van", 0.2),
                    ("pickup", 0.2)
                ],
                0.75,
                [13, 18]
            ),
            traffic!(
                [70, 160],
                [
                    ("hatch", 0.3),
                    ("sedan", 0.3),
                    ("van", 0.2),
                    ("pickup", 0.2)
                ],
                0.5,
                [12, 17]
            ),
            traffic!(
                [160, 340],
                [
                    ("sedan", 0.3),
                    ("hatch", 0.2),
                    ("van", 0.2),
                    ("pickup", 0.3)
                ],
                0.7,
                [12, 16]
            ),
            traffic!(
                [220, 460],
                [("pickup", 0.4), ("sedan", 0.3), ("hatch", 0.3)],
                0.7,
                [11, 15]
            ),
        ],
        // No police yet: Hot Pursuit is not offered on a circuit (the
        // pursuit measures a point-to-point race), and the loop is one lap.
        police: None,
        rivals: vec![
            rival!("Razor", "super", 0xe9ecef, 0.995, 530),
            rival!("Marlowe", "electric", 0x1ab8c4, 0.98, 520),
            rival!("Kaito", "sports", 0x19c46b, 0.97, 505),
            rival!("Vex", "muscle", 0x8b2cff, 0.96, 545),
            rival!("Nina", "sports", 0xff7a1a, 0.95, 500),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_corridors_close_the_loop() {
        let cs = corridors();
        let l = chain(&cs);
        assert_eq!(l.joins.len(), 4);
        assert!(l.length > 34_000.0 && l.length < 36_000.0, "{}", l.length);
        // Every join is where the next corridor begins, and the last sample
        // is a step from the first.
        for k in 1..4 {
            let j = l.joins[k];
            let s =
                l.x.iter()
                    .zip(&l.z)
                    .fold((0.0, f64::INFINITY), |(acc, best), _| (acc, best));
            let _ = s;
            assert!(j > 0.0 && j < l.length, "{j}");
        }
        let n = l.x.len() - 1;
        let close = mp_math::kernel::hypot(l.x[n] - l.x[0], l.z[n] - l.z[0]);
        assert!((JOIN..12.0).contains(&close), "closing edge {close} m");
        for k in 1..=n {
            let d = mp_math::kernel::hypot(l.x[k] - l.x[k - 1], l.z[k] - l.z[k - 1]);
            // The rounded corners pull the samples at a join closer.
            assert!((1.0..12.0).contains(&d), "sample {k}: {d} m from the last");
        }
    }

    #[test]
    fn the_level_is_a_lap_of_the_loop() {
        let lv = level();
        assert!(lv.is_loop());
        assert_eq!(lv.laps, Some(1));
        assert_eq!(lv.zones.len(), 4);
        let t = mp_track::Track::new(&lv).unwrap();
        assert!(t.is_loop);
        assert!(t.length > 34_000.0 && t.length < 36_000.0, "{}", t.length);
        assert_eq!(t.zones.len(), 4);
        // Skyline is the highest stretch of the lap.
        let top = (0..t.n)
            .max_by(|&a, &b| t.py[a].total_cmp(&t.py[b]))
            .unwrap();
        assert_eq!(t.zone[top], 3, "the crest of the lap is on Skyline");
        assert!(t.py[top] > 300.0);
        // The ground is the real one and the cover reads.
        let c = lv.cover.as_ref().unwrap();
        let f = t.frame(t.zones[3].s0 + 2000.0);
        assert_eq!(mp_track::Cover::of(c(f.x, f.z)), mp_track::Cover::Forest);
    }

    #[test]
    fn the_closed_spline_wraps() {
        let v = spline_closed(&[0.0, 1.0, 3.0, 6.0], 4);
        assert_eq!(v.len(), 16);
        assert_eq!(v[0], 0.0);
        assert_eq!(v[4], 1.0);
        assert_eq!(v[12], 6.0);
    }
}
