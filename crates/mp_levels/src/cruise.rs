//! Night City Cruise — an endless loop of freeway through Meridian at night
//! (port of `src/levels/cruise.js`).
//!
//! The loop is a closed polar curve: a stretched circle (~13 km around) with
//! a few layers of gentle waviness, so it keeps turning in long, slow sweeps
//! without ever crossing itself. Viaducts and tunnels are placed by fraction
//! of the loop.

use std::f64::consts::PI;
use std::sync::Arc;

use crate::zone;
use mp_math::kernel::{cos, sin};
use mp_track::{Level, LoopPath, LoopSpec, LoopTag, Mode, Route};

pub fn loop_path() -> LoopPath {
    let (r0, n) = (1900.0, 6000);
    let terms = [
        (2.0, 0.14, 0.3),
        (3.0, 0.05, 1.1),
        (5.0, 0.03, 2.0),
        (7.0, 0.02, 0.4),
        (11.0, 0.013, 2.6),
        (13.0, 0.019, 1.7),
        (17.0, 0.011, 0.9),
    ];
    let mut x = Vec::with_capacity(n);
    let mut z = Vec::with_capacity(n);
    for i in 0..n {
        let th = (i as f64 / n as f64) * PI * 2.0;
        let mut r = 1.0;
        for (k, a, ph) in terms {
            r += a * cos(k * th + ph);
        }
        x.push(cos(th) * r * r0 * 1.22);
        z.push(sin(th) * r * r0);
    }
    LoopPath {
        x,
        z,
        ..LoopPath::default()
    }
}

fn tag(tag: &'static str, f0: f64, f1: f64, elevated: Option<f64>) -> LoopTag {
    LoopTag {
        tag,
        s0: None,
        s1: None,
        f0: Some(f0),
        f1: Some(f1),
        elevated,
    }
}

pub fn level() -> Level {
    let night = sky!(
        0, -18, 0x03050c, 0x2a1e30, 0x9ab0e0, 0.28, 0x2c3252, 0x16131a, 0.3, 0x1c1826, 0.00038,
        1.22, 1
    );
    Level {
        id: "cruise",
        mode: Mode::Cruise,
        num: "ENDLESS",
        title: "Night City Cruise",
        desc: "An endless loop of freeway through Meridian at midnight. No finish line: weave through traffic, chain near misses for a multiplier and keep the speed up.",
        laps: None,
        lap_length: None,
        start_height: None,
        start_heading: None,
        start_x: None,
        start_z: None,
        finish_runoff: None,
        elevation_smooth: None,
        route: Route::Loop(LoopSpec {
            path: Arc::new(|| Ok(loop_path())),
            base_y: Some(20.),
            // Fractions of the loop. elevated: metres above the street.
            tags: vec![
                tag("viaduct", 0.1, 0.19, Some(10.)),
                tag("downtown", 0.2, 0.32, None),
                tag("tunnel", 0.34, 0.365, None),
                tag("viaduct", 0.52, 0.61, Some(11.)),
                tag("downtown", 0.64, 0.74, None),
                tag("tunnel", 0.8, 0.82, None),
            ],
            road: None,
            roads: Vec::new(),
            start_s: None,
            elevation_smooth: None,
            zones: None,
        }),
        elevation: None,
        ground: None,
        loose_ground: None,
        sea_y: None,
        zones: vec![zone(
            "city",
            "MERIDIAN LOOP",
            "Endless night cruise",
            "city",
            "City",
            "#5a4acb",
        )],
        sky: vec![night, mp_track::SkyKey { s: 1.0, ..night }],
        sun_azimuth: 0.2,
        moon_dir: None,
        traffic_paint: None,
        traffic: vec![traffic!(
            [30, 75],
            [
                ("sedan", 0.34),
                ("hatch", 0.2),
                ("van", 0.14),
                ("pickup", 0.12),
                ("boxtruck", 0.2)
            ],
            0,
            [21, 32],
            0.8
        )],
        police: None,
        rivals: Vec::new(),
    }
}
