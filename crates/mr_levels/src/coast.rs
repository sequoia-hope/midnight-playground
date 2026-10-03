//! Level 2 — "Coast Highway" (port of `src/levels/coast.js`). A dawn run
//! with the sea on your left the whole way: a cliff road carved above the
//! ocean, down into the beach town of Seabright as the sun comes up over the
//! water, then over the harbour bridge into Port Meridian's container docks.
//!
//! Same segment format as Level 1: [length m, turn deg, rise m, extras].

use crate::{police, zone};
use mr_track::{Level, Mode, Route, Segment, seg};

fn segments() -> Vec<Segment> {
    vec![
        // ── HOLLOW POINT: cliff road ───────────────────────────────────
        seg(180., 0., 1.).zone(0).road("coastal").tag("start"),
        seg(160., -24., 3.),
        seg(140., 38., 4.),
        seg(120., -46., -2.).tag("cove"),
        seg(110., 52., 2.),
        seg(160., 0., 5.).tag("lighthouse"),
        seg(140., -30., 6.),
        seg(95., 72., 3.),
        seg(130., -58., 2.),
        seg(150., 34., -3.).tag("arch"),
        seg(100., -40., 3.),
        seg(180., 22., 6.).tag("bluff"),
        seg(160., -30., 4.),
        seg(140., 45., -4.),
        seg(120., -52., -2.).tag("cove"),
        seg(160., 26., -8.).tag("descent"),
        seg(180., -20., -14.),
        seg(160., 34., -16.),
        seg(200., -16., -18.),
        // ── SEABRIGHT: beach town boulevard ────────────────────────────
        seg(220., 20., -20.)
            .zone(1)
            .road("boulevard")
            .tag("town-entry"),
        seg(200., -18., -20.),
        seg(160., 0., -9.).tag("promenade"),
        seg(260., 8., -1.).tag("pier"),
        seg(200., -14., 0.),
        seg(240., 0., 0.).tag("boardwalk"),
        seg(180., 20., 0.),
        seg(220., -10., 0.).tag("marina"),
        seg(200., 0., 1.),
        // ── PORT MERIDIAN: harbour bridge and container docks ──────────
        seg(200., 18., 0.).zone(2).road("freeway").tag("approach"),
        seg(420., 0., 38.).tag("bridge-up").elevated(),
        seg(900., 0., 0.).tag("bridge").elevated(),
        seg(420., 0., -40.).tag("bridge-down").elevated(),
        seg(260., -30., 0.).tag("port"),
        seg(300., 26., 0.).tag("containers"),
        seg(260., -14., 0.),
        seg(240., 0., 0.).tag("finish"),
    ]
}

pub fn level() -> Level {
    Level {
        id: "coast",
        mode: Mode::Race,
        num: "LEVEL 2",
        title: "Coast Highway",
        desc: "Leave before dawn on the cliff road above the ocean, cruise the beach boulevard of Seabright as the sun comes up, and finish over the harbour bridge in Port Meridian.",
        laps: None,
        lap_length: None,
        start_height: Some(85.),
        start_heading: Some(0.),
        start_x: None,
        start_z: None,
        finish_runoff: Some(180.),
        elevation_smooth: None,
        route: Route::Segments(segments()),
        elevation: None,
        ground: None,
        loose_ground: None,
        sea_y: Some(0.),
        zones: vec![
            zone(
                "coast",
                "HOLLOW POINT",
                "The cliff road",
                "coast",
                "Coast",
                "#9a7f66",
            ),
            zone(
                "beach",
                "SEABRIGHT",
                "Beach town at sunrise",
                "beach",
                "Beach",
                "#e0b45a",
            )
            .blend(350.),
            zone(
                "harbor",
                "PORT MERIDIAN",
                "Harbour bridge",
                "harbor",
                "Harbor",
                "#3f8fc0",
            )
            .blend(300.),
        ],
        // Dawn: blue hour on the cliffs, sunrise over the sea at the beach town,
        // golden morning on the bridge. The blue-hour keys carry a strong sky fill
        // (hemisphere light) so the cliffs read before the sun is up.
        sky: vec![
            sky!(
                0.00, -9, 0x0c1636, 0x3e4c80, 0xa4b8e6, 0.55, 0x5a6ca8, 0x2e2e40, 1.0, 0x34406a,
                0.0003, 1.22, 0.75
            ),
            sky!(
                0.18, -5, 0x182652, 0x76648e, 0xb8a8d8, 0.55, 0x6c76ac, 0x36303e, 1.05, 0x55547c,
                0.0003, 1.16, 0.55
            ),
            sky!(
                0.32, -1.5, 0x22346e, 0xe08a6c, 0xff8a58, 0.9, 0x8a88b8, 0x4a3c3e, 1.0, 0xa27888,
                0.00028, 1.08, 0.3
            ),
            sky!(
                0.44, 2.5, 0x33549a, 0xffb070, 0xff9a50, 2.2, 0xa8b0d0, 0x5e4e44, 1.0, 0xe0a882,
                0.00026, 1.04, 0.05
            ),
            sky!(
                0.60, 7, 0x3f6cb4, 0xf6cfa0, 0xffc080, 2.9, 0xb8c8e6, 0x6e6252, 1.25, 0xe6c8a8,
                0.00022, 1.0, 0
            ),
            sky!(
                0.80, 13, 0x3e78c6, 0xcfe0ee, 0xffe8c8, 3.3, 0xc2d6f2, 0x857a64, 1.45, 0xc2d4e4,
                0.0002, 1.0, 0
            ),
            sky!(
                1.00, 18, 0x3a78cc, 0xc6dcee, 0xfff0d8, 3.4, 0xc2d6f2, 0x8a8068, 1.55, 0xbfd3e6,
                0.0002, 1.0, 0
            ),
        ],
        sun_azimuth: -0.45, // rising over the sea, ahead and to the left
        moon_dir: None,
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
                [50, 120],
                [
                    ("sedan", 0.3),
                    ("hatch", 0.15),
                    ("van", 0.15),
                    ("pickup", 0.1),
                    ("boxtruck", 0.3)
                ],
                0,
                [21, 29],
                0.5
            ),
        ],
        // Hot Pursuit: the cliffs at heat 2, the boulevard at 3 (units come
        // the other way and U-turn), the harbour bridge and docks at 5.
        police: Some(police(&[2, 3, 5], &[false, false, true])),
        rivals: vec![
            rival!("Razor", "super", 0xe9ecef, 0.995, 530),
            rival!("Marlowe", "electric", 0x1ab8c4, 0.98, 520),
            rival!("Kaito", "sports", 0x19c46b, 0.97, 505),
            rival!("Vex", "muscle", 0x8b2cff, 0.96, 545),
            rival!("Nina", "sports", 0xff7a1a, 0.95, 500),
        ],
    }
}
