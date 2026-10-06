//! Level 1 — "Sierra to the City" (port of `src/levels/sierra.js`).
//!
//! The road is laid out like a surveyor would: a turtle walks forward along a
//! list of segments, each with a length, a total turn and a total climb. Within
//! a segment curvature eases in and out (a smoothed trapezoid), so straights
//! flow into arcs the way real highway spirals do instead of kinking.
//!
//! turn > 0 is a right-hander. Heading 0 points down +X; the right-hand vector
//! of a heading (fx, fz) is (-fz, fx).
//!
//! [length m, turn deg, rise m, extras]
//! extras: zone (index), road (type key), tag (named feature for scenery),
//!         elevated (true = on a bridge/viaduct; the ground isn't raised to it)

use crate::{police, zone};
use mp_track::{Level, Mode, Route, Segment, seg};

fn segments() -> Vec<Segment> {
    vec![
        // ── SIERRA PASS ────────────────────────────────────────────────
        seg(170., 0., 1.).zone(0).road("mountain").tag("start"),
        seg(150., 22., 6.),
        seg(120., -38., 9.),
        seg(140., 30., 12.),
        seg(90., 0., 8.),
        // Swing right to traverse the face, then stack four hairpins up it.
        seg(110., 72., 9.),
        seg(150., 0., 15.).tag("switchbacks"),
        seg(88., -180., 6.).tag("hairpin"),
        seg(170., 0., 17.),
        seg(88., 180., 6.).tag("hairpin"),
        seg(170., 0., 17.),
        seg(88., -180., 6.).tag("hairpin"),
        seg(150., 8., 14.),
        seg(120., 82., 9.),
        // Narrow canyon: rocky S-curves between walls.
        seg(90., -38., 8.).tag("canyon"),
        seg(85., 46., 7.),
        seg(80., -52., 6.),
        seg(100., 34., 7.),
        seg(110., -22., 5.),
        // The summit and its lookout.
        seg(160., 0., 2.).tag("summit"),
        seg(110., 18., -3.),
        // Descent toward the valley, sweeping bends with long views.
        seg(160., -46., -15.).tag("descent"),
        seg(140., 62., -15.),
        seg(120., -34., -13.),
        seg(150., 52., -16.),
        seg(140., -62., -15.),
        seg(170., 24., -17.),
        seg(160., -26., -15.),
        seg(200., 12., -16.),
        // ── OLD MILL VALLEY ────────────────────────────────────────────
        seg(200., 34., -10.)
            .zone(1)
            .road("valley")
            .tag("valley-entry"),
        seg(250., 0., -6.).tag("farm"),
        seg(50., 0., 3.2).tag("crest"),
        seg(50., 0., -3.8),
        seg(200., -30., -3.),
        seg(150., 45., -2.).tag("farm"),
        seg(120., -45., -2.),
        seg(220., 0., -3.).tag("bridge"),
        seg(60., 0., 3.8).tag("crest"),
        seg(60., 0., -4.4),
        seg(180., 66., -3.).tag("farm"),
        seg(260., 0., -4.).tag("fields"),
        seg(140., -58., -3.),
        seg(50., 0., 3.2).tag("crest"),
        seg(50., 0., -3.8),
        seg(220., 22., -5.).tag("farm"),
        seg(160., 0., -3.),
        // ── INTERSTATE 9 / DOWNTOWN MERIDIAN ───────────────────────────
        seg(220., 48., -3.).zone(2).road("freeway").tag("onramp"),
        seg(300., -10., -2.).tag("merge"),
        seg(400., -22., 0.).tag("outskirts"),
        seg(300., 26., 10.).tag("viaduct-up"),
        seg(420., 0., 0.).tag("viaduct"),
        seg(300., -30., -10.).tag("viaduct-down"),
        seg(260., 0., 0.).tag("tunnel"),
        seg(400., 20., 0.).tag("downtown"),
        seg(300., -16., 0.),
        seg(280., 0., 0.).tag("finish"),
    ]
}

pub fn level() -> Level {
    Level {
        id: "sierra",
        mode: Mode::Race,
        num: "LEVEL 1",
        title: "Sierra to the City",
        desc: "Sprint up the switchbacks of Sierra Pass, drop through the old farms of Mill Valley, and finish on the Interstate through downtown Meridian. Sunset to midnight.",
        laps: None,
        lap_length: None,
        start_height: Some(120.),
        start_heading: Some(0.),
        start_x: None,
        start_z: None,
        finish_runoff: Some(180.),
        elevation_smooth: None,
        route: Route::Segments(segments()),
        elevation: None,
        ground: None,
        loose_ground: None,
        sea_y: None,
        zones: vec![
            zone(
                "mountain",
                "SIERRA PASS",
                "Climb the switchbacks",
                "mountain",
                "Mountain",
                "#b8925f",
            ),
            zone(
                "valley",
                "OLD MILL VALLEY",
                "Farm country",
                "valley",
                "Valley",
                "#82aa44",
            )
            .blend(350.),
            zone(
                "city",
                "INTERSTATE 9",
                "Downtown Meridian",
                "city",
                "City",
                "#5a4acb",
            )
            .blend(300.)
            .blend_offset(120.),
        ],
        // Time of day along the route (s = fraction of the track length).
        sky: vec![
            sky!(
                0.00, 30, 0x2f63b0, 0xb9d0e6, 0xfff0d6, 3.4, 0xc2d6f2, 0x8a8068, 1.6, 0xb5cadf,
                0.00024, 1.0, 0
            ),
            sky!(
                0.18, 17, 0x3a64a8, 0xe8cfa6, 0xffdca8, 3.3, 0xc0cfe6, 0x857a64, 1.45, 0xd2c3a8,
                0.00025, 1.0, 0
            ),
            sky!(
                0.30, 6, 0x3e5a9a, 0xf6b77a, 0xffb46a, 3.0, 0xb0b4d0, 0x6a5a4e, 1.1, 0xdcae86,
                0.00027, 1.02, 0
            ),
            sky!(
                0.42, 1.2, 0x33467e, 0xff8f52, 0xff7a3a, 2.2, 0x8f8fb8, 0x5a4a46, 0.85, 0xc78468,
                0.0003, 1.05, 0.15
            ),
            sky!(
                0.54, -3, 0x1c2452, 0xb86068, 0xff6a40, 0.6, 0x6c6a98, 0x3e3640, 0.6, 0x5e4a66,
                0.00034, 1.1, 0.5
            ),
            sky!(
                0.66, -8, 0x0e1432, 0x3c3a66, 0x9ab0e0, 0.3, 0x3a4270, 0x1c1a22, 0.3, 0x2a2c48,
                0.00038, 1.15, 0.82
            ),
            sky!(
                0.80, -14, 0x05070f, 0x1e1c30, 0x9ab0e0, 0.26, 0x2a3050, 0x151218, 0.26, 0x181828,
                0.00042, 1.2, 1
            ),
            sky!(
                1.00, -18, 0x03050c, 0x241c2a, 0x9ab0e0, 0.26, 0x283050, 0x151218, 0.26, 0x1a1624,
                0.00042, 1.2, 1
            ),
        ],
        sun_azimuth: 0.2,
        moon_dir: None,
        traffic_paint: None,
        traffic: vec![
            traffic!(
                [260, 520],
                [
                    ("sedan", 0.4),
                    ("hatch", 0.3),
                    ("pickup", 0.2),
                    ("van", 0.1)
                ],
                0.85,
                [12, 16]
            ),
            traffic!(
                [180, 360],
                [
                    ("pickup", 0.3),
                    ("sedan", 0.25),
                    ("hatch", 0.15),
                    ("tractor", 0.15),
                    ("boxtruck", 0.15)
                ],
                0.65,
                [16, 21]
            ),
            traffic!(
                [45, 110],
                [
                    ("sedan", 0.34),
                    ("hatch", 0.2),
                    ("van", 0.14),
                    ("pickup", 0.14),
                    ("boxtruck", 0.18)
                ],
                0,
                [21, 31],
                0.5
            ),
        ],
        // Hot Pursuit (docs/hot-pursuit.md §8): the narrow, twisty pass is capped
        // at heat 2, the valley at 3 and Interstate 9 at 5.
        police: Some(police(&[2, 3, 5], &[false, false, false])),
        rivals: vec![
            rival!("Razor", "super", 0xe9ecef, 0.99, 525),
            rival!("Kaito", "sports", 0x19c46b, 0.972, 505),
            rival!("Vex", "muscle", 0x8b2cff, 0.958, 540),
            rival!("Nina", "rally", 0xff7a1a, 0.945, 495),
            rival!("Duke", "muscle", 0x30343b, 0.93, 530),
        ],
    }
}
