//! "Midnight on the Coast": the open world's first corridor
//! (docs/vision/peninsula.md, stage 1). Highway 1 from the San Gregorio
//! junction to Half Moon Bay's big intersection, the real road compressed
//! to the atlas plan's scale; the race starts at the crest above San
//! Gregorio: down off the crest, along the bluffs past the farms and over
//! the creeks, into the town's lights.
//!
//! The road is `assets/atlas/peninsula/corridors/coast-south.txt`, written
//! by `cargo run -p mp_atlas -- corridor coast-south` and included here at
//! compile time (a sample every 5 game metres: x, z, y and the real land
//! cover beside the road). The ground under the whole box is real too
//! (stage 2): `coast-south.ground.bin`, written by `tools/atlas/ground.py`
//! from the terrain tiles, 16 game metres a cell, which the level is open
//! onto: the car may leave the road, and world generation shapes the land
//! from it instead of the kit's landform. Highway 84 leaves Highway 1 at
//! the junction and climbs to La Honda (stage 3): a branch of the road,
//! `hwy-84-lower.txt`, that the Track joins to the main road.

use std::sync::{Arc, OnceLock};

use crate::ground::HeightGrid;
use crate::{police, zone};
use mp_track::{Branch, Level, LoopPath, LoopRoad, LoopSpec, LoopTag, Mode, Route};

/// The corridor file: Highway 1 from the junction to Half Moon Bay.
pub const CORRIDOR: &str =
    include_str!("../../../assets/atlas/peninsula/corridors/coast-south.txt");
/// Highway 84 from the junction to La Honda.
pub const BRANCH_84: &str =
    include_str!("../../../assets/atlas/peninsula/corridors/hwy-84-lower.txt");
/// The ground file (541 kB, zlib): inflated once, the first time the level
/// is made.
pub const GROUND: &[u8] =
    include_bytes!("../../../assets/atlas/peninsula/corridors/coast-south.ground.bin");
/// The race starts at the first crest along the road: the highest point
/// within this many game metres of the junction.
pub const CREST_WITHIN: f64 = 2000.0;

/// The ground under the box, decoded once.
pub fn ground() -> Arc<HeightGrid> {
    static GRID: OnceLock<Arc<HeightGrid>> = OnceLock::new();
    GRID.get_or_init(|| Arc::new(HeightGrid::parse(GROUND).expect("the committed ground parses")))
        .clone()
}

/// A corridor's samples: game metres and the land cover beside the road.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Corridor {
    pub x: Vec<f64>,
    pub z: Vec<f64>,
    pub y: Vec<f64>,
    pub cover: Vec<String>,
    /// Game metres between samples.
    pub step: f64,
}

impl Corridor {
    /// Reads a corridor file: `#` comments, then `x z y cover` a line.
    pub fn parse(text: &str) -> Result<Corridor, String> {
        let mut c = Corridor::default();
        for (k, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut f = line.split_whitespace();
            let mut num = |what: &str| -> Result<f64, String> {
                f.next()
                    .ok_or_else(|| format!("corridor line {}: no {what}", k + 1))?
                    .parse::<f64>()
                    .map_err(|e| format!("corridor line {}: {what}: {e}", k + 1))
            };
            c.x.push(num("x")?);
            c.z.push(num("z")?);
            c.y.push(num("y")?);
            c.cover.push(
                f.next()
                    .ok_or_else(|| format!("corridor line {}: no cover", k + 1))?
                    .to_owned(),
            );
        }
        if c.x.len() < 2 {
            return Err("corridor: fewer than two samples".to_owned());
        }
        // The samples' mean spacing (the writer's smoothing moves them a
        // little along the road).
        let mut len = 0.0;
        for k in 1..c.x.len() {
            len += mp_math::kernel::hypot(c.x[k] - c.x[k - 1], c.z[k] - c.z[k - 1]);
        }
        c.step = len / (c.x.len() - 1) as f64;
        Ok(c)
    }

    /// Where the town starts: the first sample of the longest run of urban
    /// cover, in game metres along the road.
    pub fn town_start(&self) -> f64 {
        let (mut best, mut best_len) = (self.cover.len(), 0usize);
        let mut i = 0;
        while i < self.cover.len() {
            if self.cover[i] != "urban" {
                i += 1;
                continue;
            }
            let start = i;
            while i < self.cover.len() && self.cover[i] == "urban" {
                i += 1;
            }
            if i - start > best_len {
                best = start;
                best_len = i - start;
            }
        }
        best as f64 * self.step
    }

    /// The crest the race starts on: the highest sample within `within`
    /// metres of the road's start, in game metres along the road.
    pub fn crest_within(&self, within: f64) -> f64 {
        let n = ((within / self.step) as usize).min(self.y.len() - 1);
        let mut best = 0;
        for k in 1..=n {
            if self.y[k] > self.y[best] {
                best = k;
            }
        }
        best as f64 * self.step
    }
}

/// Catmull-Rom through the samples, `sub` points per span, the ends
/// clamped (an open road: nothing wraps).
pub fn spline_open(src: &[f64], sub: usize) -> Vec<f64> {
    let n = src.len();
    if n < 2 {
        return src.to_vec();
    }
    let mut out = Vec::with_capacity((n - 1) * sub + 1);
    for i in 0..n - 1 {
        let p0 = src[i.saturating_sub(1)];
        let p1 = src[i];
        let p2 = src[i + 1];
        let p3 = src[(i + 2).min(n - 1)];
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
    out.push(src[n - 1]);
    out
}

/// `path()`: the corridor splined to a metre.
pub fn path(c: &Corridor) -> LoopPath {
    let sub = (c.step.round() as usize).max(1);
    LoopPath {
        x: spline_open(&c.x, sub),
        z: spline_open(&c.z, sub),
        y: Some(spline_open(&c.y, sub)),
        ..LoopPath::default()
    }
}

pub fn level() -> Level {
    let c = Arc::new(Corridor::parse(CORRIDOR).expect("the committed corridor parses"));
    let b84 = Arc::new(Corridor::parse(BRANCH_84).expect("the committed branch parses"));
    let town = c.town_start();
    let crest = c.crest_within(CREST_WITHIN);
    Level {
        id: "southcoast",
        mode: Mode::Race,
        num: "THE PENINSULA",
        title: "Midnight on the Coast",
        desc: "Start in the dark on the crest above San Gregorio, race Highway 1 along the bluffs through the farmland, and finish at the big intersection in Half Moon Bay. The real road, half size.",
        laps: None,
        lap_length: None,
        start_height: None,
        start_heading: None,
        start_x: None,
        start_z: None,
        finish_runoff: Some(180.),
        // The planning grid is coarse (50 m real cells): smooth the road's
        // heights more than a drawn level's.
        elevation_smooth: Some(16.),
        route: Route::Path(LoopSpec {
            path: {
                let c = c.clone();
                Arc::new(move || Ok(path(&c)))
            },
            base_y: None,
            // By metres along the road: the crest the race starts on, and
            // where the town begins.
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
            roads: vec![LoopRoad {
                s0: town as i64,
                s1: (c.x.len() as f64 * c.step) as i64 + 1,
                road: "boulevard",
            }],
            start_s: Some(crest + 60.),
            elevation_smooth: None,
            zones: Some(vec![0., town]),
        }),
        elevation: None,
        ground: Some({
            let g = ground();
            Arc::new(move |x, z| g.height(x, z))
        }),
        loose_ground: None,
        open: true,
        // Highway 84 leaves the junction at the road's start and climbs
        // through the redwoods to La Honda.
        branches: vec![Branch {
            id: "hwy84",
            name: "Highway 84",
            path: {
                let c = b84.clone();
                Arc::new(move || Ok(path(&c)))
            },
            road: "mountain",
            zone: 0,
            elevation_smooth: None,
        }],
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
        ],
        // Night the whole way: the moon on the sea, the farms dark.
        sky: {
            let night = sky!(
                0, -18, 0x03050c, 0x2a1e30, 0x9ab0e0, 0.28, 0x2c3252, 0x16131a, 0.3, 0x1c1826,
                0.00038, 1.22, 1
            );
            vec![night, mp_track::SkyKey { s: 1.0, ..night }]
        },
        sun_azimuth: -0.45,
        moon_dir: Some([-0.6, 0.4, -0.5]), // over the sea, ahead and to the left
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
        ],
        police: Some(police(&[2, 3], &[false, false])),
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
    fn the_corridor_parses_and_the_town_is_at_the_end() {
        let c = Corridor::parse(CORRIDOR).unwrap();
        assert!((c.step - 5.0).abs() < 0.2, "{}", c.step);
        let len = (c.x.len() - 1) as f64 * c.step;
        assert!(len > 8000.0 && len < 9500.0, "{len}");
        let town = c.town_start();
        assert!(town > len - 1500.0 && town < len, "{town} of {len}");
        assert_eq!(c.cover[(town / c.step) as usize], "urban");
        // Up from the junction near sea level to the crest, then down into
        // the town near sea level.
        let crest = c.crest_within(CREST_WITHIN);
        assert!(crest > 800.0 && crest < 1400.0, "{crest}");
        assert!(c.y[0] < 20.0 && c.y[(crest / c.step) as usize] > 60.0);
        assert!(*c.y.last().unwrap() < 15.0);
    }

    #[test]
    fn highway_84_starts_at_the_junction_and_climbs_to_la_honda() {
        let c = Corridor::parse(CORRIDOR).unwrap();
        let b = Corridor::parse(BRANCH_84).unwrap();
        let d = mp_math::kernel::hypot(b.x[0] - c.x[0], b.z[0] - c.z[0]);
        assert!(d < 5.0, "the branch starts {d} m from the junction");
        let len = (b.x.len() - 1) as f64 * b.step;
        assert!(len > 6000.0 && len < 8000.0, "{len}");
        assert!(*b.y.last().unwrap() > 40.0);
        assert!(b.x.last().unwrap() > &(c.x[0] + 4000.0), "east to La Honda");
    }

    #[test]
    fn the_open_spline_keeps_the_ends() {
        let v = spline_open(&[0.0, 1.0, 3.0, 6.0], 4);
        assert_eq!(v.len(), 13);
        assert_eq!(v[0], 0.0);
        assert_eq!(v[4], 1.0);
        assert_eq!(v[12], 6.0);
    }
}
