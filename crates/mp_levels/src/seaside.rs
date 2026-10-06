//! Level 5 — "Seaside Raceway" (port of `src/levels/seaside.js`). Three laps
//! of a real circuit: Laguna Seca, in the hills above Monterey Bay, rebuilt
//! from survey data (see tools/seaside/build.py). The racing line follows
//! OpenStreetMap, the road surface, its camber and the hills round it come
//! from USGS lidar, the barriers stand where the real walls do, and the ground
//! takes its colour, and the oaks their places, from aerial photos.
//!
//! It runs anticlockwise, 3.6 km a lap: over the crest at Turn 1, down into
//! the hairpin, through Three to Six and up the long climb to the Corkscrew,
//! a blind left-right that drops five storeys, then down through the Rainey
//! curve and Ten to the last hairpin and the line.
//!
//! The data (`assets/seaside/survey.bin`, read by [`SeasideData::parse`]) is
//! only loaded when the level is built ([`prepare`]), so the menu doesn't pay
//! for it.

use std::sync::Arc;

use crate::survey::SeasideData;
use crate::zone;
use mp_track::{Level, LoopPath, LoopSpec, LoopTag, Mode, Route, SkyKey};

/// Sectors, by metres into the lap (the lap starts on the line).
pub const SECTORS: [f64; 3] = [0.0, 1180.0, 2480.0];

/// Catmull-Rom through the 2 m survey points, 4 per span, so the 1 m
/// resample in Track has no corners to find.
pub fn spline(src: &[f64], n: usize, sub: usize) -> Vec<f64> {
    let mut out = vec![0f64; n * sub];
    for i in 0..n {
        let p0 = src[(i + n - 1) % n];
        let p1 = src[i];
        let p2 = src[(i + 1) % n];
        let p3 = src[(i + 2) % n];
        for k in 0..sub {
            let t = k as f64 / sub as f64;
            let t2 = t * t;
            let t3 = t2 * t;
            out[i * sub + k] = 0.5
                * (2.0 * p1
                    + (-p0 + p2) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                    + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
        }
    }
    out
}

/// `path()`: the survey's centreline and its extras, splined.
pub fn path(d: &SeasideData) -> LoopPath {
    let l = &d.line;
    let n = l.n;
    let sp = |a: &[f64]| spline(a, n, 4);
    LoopPath {
        x: sp(&l.x),
        z: sp(&l.z),
        y: Some(sp(&l.y)),
        bank: Some(sp(&l.bank)),
        hw: Some(sp(&l.hw)),
        wall_l: Some(sp(&l.wall_l)),
        wall_r: Some(sp(&l.wall_r)),
        run_l: Some(sp(&l.run_l)),
        run_r: Some(sp(&l.run_r)),
    }
}

/// Fill in what the survey data gives the level (`prepare()` in the JS):
/// the loop's path, the ground, and the loose-ground mask.
pub fn prepare(level: &mut Level, data: Arc<SeasideData>) {
    if let Route::Loop(spec) = &mut level.route {
        let d = data.clone();
        spec.path = Arc::new(move || Ok(path(&d)));
    }
    // The level owns the ground: the lidar surface round the circuit.
    let d = data.clone();
    level.ground = Some(Arc::new(move |x, z| d.height(x, z)));
    // Off the tarmac: 0 on paved run-off, 1 on dirt and grass (the photo's).
    let d = data;
    level.loose_ground = Some(Arc::new(move |x, z| d.loose(x, z)));
}

fn tag(tag: &'static str, s0: f64, s1: f64) -> LoopTag {
    LoopTag {
        tag,
        s0: Some(s0),
        s1: Some(s1),
        f0: None,
        f1: None,
        elevated: None,
    }
}

/// The level without its survey data: what the menu needs. Building its
/// Track fails until [`prepare`] has run.
pub fn level() -> Level {
    // Late afternoon: the sun low over the bay to the west-south-west, dry
    // gold hills and a little haze. The same all race (a loop reads the
    // middle of the keys).
    let key = sky!(
        0, 15, 0x3a6cb8, 0xe6d6b8, 0xffddb0, 3.3, 0xc4d2ea, 0x8e7a58, 1.45, 0xd8cbb2, 0.00019, 1.0,
        0
    );
    Level {
        id: "seaside",
        mode: Mode::Race,
        num: "LEVEL 5",
        title: "Seaside Raceway",
        desc: "Three laps of a real circuit in the golden hills above the bay, rebuilt from lidar survey: over the crest into the hairpin, up the long climb and over the blind drop of the Corkscrew.",
        laps: Some(3),
        // Metres round the lap (the survey's centreline; the menu shows it).
        lap_length: Some(3595.),
        start_height: None,
        start_heading: None,
        start_x: None,
        start_z: None,
        finish_runoff: None,
        elevation_smooth: None,
        route: Route::Loop(LoopSpec {
            path: Arc::new(|| Err("Seaside Raceway: prepare() first".to_string())),
            base_y: None,
            // Named places round the lap (metres from the line).
            tags: vec![
                tag("pit-straight", 3380., 3595.),
                tag("crest", 200., 330.),
                tag("hairpin", 500., 680.),
                tag("climb", 2040., 2480.),
                tag("corkscrew", 2500., 2640.),
                tag("hairpin", 3290., 3420.),
            ],
            road: Some("circuit"),
            roads: Vec::new(),
            start_s: Some(0.),
            // Real heights: only the lightest smoothing on top of the survey's own.
            elevation_smooth: Some(1.5),
            zones: Some(SECTORS.to_vec()),
        }),
        elevation: None,
        ground: None,
        loose_ground: None,
        sea_y: None,
        zones: vec![
            zone(
                "hairpin",
                "THE HAIRPIN",
                "Over the crest and down to the hairpin",
                "raceway",
                "Raceway",
                "#d6a24a",
            ),
            zone(
                "climb",
                "THE CLIMB",
                "Up through Five and Six",
                "raceway",
                "Raceway",
                "#b9853c",
            ),
            zone(
                "corkscrew",
                "THE CORKSCREW",
                "Over the top and five storeys down",
                "raceway",
                "Raceway",
                "#e05a3a",
            ),
        ],
        sky: vec![key, SkyKey { s: 1.0, ..key }],
        sun_azimuth: 2.79, // west-south-west (compass 250°)
        moon_dir: None,
        traffic_paint: None,
        // A closed circuit: no traffic.
        traffic: vec![
            traffic!([300, 500], [], 0, [0, 0]),
            traffic!([300, 500], [], 0, [0, 0]),
            traffic!([300, 500], [], 0, [0, 0]),
        ],
        police: None,
        rivals: vec![
            rival!("Razor", "super", 0xe9ecef, 0.985, 525),
            rival!("Kaito", "sports", 0x19c46b, 0.972, 505),
            rival!("Marlowe", "electric", 0x1ab8c4, 0.962, 515),
            rival!("Rook", "rally", 0x2a6ee8, 0.95, 520),
            rival!("Vex", "muscle", 0x8b2cff, 0.94, 540),
        ],
    }
}
