//! Level 4 — "Desert Run" (port of `src/levels/desert.js`). Golden hour to
//! moonrise: a twisting two-lane through a red sandstone canyon, out onto the
//! open highway across the basin as the sun goes down behind the far range,
//! and a flat-out finish across a dry lake bed after dark.
//!
//! Same segment format as Levels 1 and 2: [length m, turn deg, rise m, extras].

use crate::{police, zone};
use mp_track::{Level, Mode, Route, Segment, seg};

fn segments() -> Vec<Segment> {
    vec![
        // ── RED ROCK CANYON ────────────────────────────────────────────
        seg(180., 0., -1.).zone(0).road("coastal").tag("start"),
        seg(140., -26., -2.),
        seg(120., 34., -3.),
        seg(110., -40., -3.).tag("narrows"),
        seg(100., 48., -2.).tag("narrows"),
        seg(90., -52., -3.),
        seg(130., 22., -4.),
        seg(160., 0., -4.).tag("arch"),
        seg(120., -44., -5.),
        seg(100., 60., -4.),
        seg(140., -30., -5.).tag("hoodoos"),
        seg(180., 18., -6.).tag("hoodoos"),
        seg(110., -64., -4.).tag("narrows"),
        seg(95., 70., -3.).tag("narrows"),
        seg(150., 0., -5.).tag("fin"),
        seg(140., -36., -5.),
        seg(130., 28., -6.),
        seg(170., -12., -7.).tag("mouth"),
        seg(220., 8., -6.).tag("mouth"),
        // ── ROUTE 66: the open highway ─────────────────────────────────
        seg(260., 16., -4.).zone(1).road("desert").tag("flats"),
        seg(380., 0., -3.),
        // Washes: the road drops into each one and crests the far bank, which
        // can put a fast car in the air.
        seg(35., 0., -2.4).tag("dip"),
        seg(35., 0., 2.9),
        seg(35., 0., -2.5),
        seg(300., 0., -1.).tag("oasis"),
        seg(35., 0., -2.5).tag("dip"),
        seg(35., 0., 3.0),
        seg(35., 0., -2.6),
        seg(260., -14., 0.),
        seg(340., 0., -2.).tag("rail"),
        seg(35., 0., -2.5).tag("dip"),
        seg(35., 0., 3.1),
        seg(35., 0., -2.6),
        seg(110., 0., 0.),
        seg(35., 0., -2.4).tag("dip"),
        seg(35., 0., 2.9),
        seg(35., 0., -2.5),
        seg(300., 12., -1.),
        seg(220., 0., -2.),
        // ── SILVER LAKE: the dry lake bed ──────────────────────────────
        seg(300., -8., -2.).zone(2).road("playa").tag("lakeshore"),
        seg(1900., 0., 0.).tag("lakebed"),
        seg(450., 0., 0.).tag("finish"),
    ]
}

pub fn level() -> Level {
    Level {
        id: "desert",
        mode: Mode::Race,
        num: "LEVEL 4",
        title: "Desert Run",
        desc: "Wind down a red rock canyon at golden hour, race the sunset and a freight train down the old highway, then go flat out across a dry lake bed under the moon.",
        laps: None,
        lap_length: None,
        start_height: Some(420.),
        start_heading: Some(0.),
        start_x: None,
        start_z: None,
        finish_runoff: Some(450.),
        elevation_smooth: None,
        route: Route::Segments(segments()),
        elevation: None,
        ground: None,
        loose_ground: None,
        sea_y: None,
        zones: vec![
            zone(
                "canyon",
                "RED ROCK CANYON",
                "Golden hour in the sandstone",
                "canyon",
                "Desert",
                "#c8643c",
            ),
            zone(
                "desert",
                "ROUTE 66",
                "Race the sunset",
                "desert",
                "Desert",
                "#e0a652",
            )
            .blend(380.),
            zone(
                "playa",
                "SILVER LAKE",
                "Flat out on the dry lake",
                "playa",
                "Desert",
                "#b9c6d6",
            )
            .blend(420.),
        ],
        // Golden hour in the canyon, sunset down the highway, moonlight on the lake.
        sky: vec![
            sky!(
                0.00, 12, 0x3563a8, 0xe9c79a, 0xffd29a, 3.3, 0xbccbe4, 0x8a6448, 1.35, 0xd8b894,
                0.00018, 1.0, 0
            ),
            sky!(
                0.22, 7, 0x3a5c9e, 0xf4b77c, 0xffb866, 3.1, 0xb4b8d4, 0x7a5440, 1.15, 0xe0ac7c,
                0.0002, 1.02, 0
            ),
            sky!(
                0.40, 2.5, 0x33508e, 0xff9a58, 0xff8a40, 2.6, 0x9a96bc, 0x62463c, 0.9, 0xd88a62,
                0.00022, 1.04, 0.05
            ),
            sky!(
                0.55, -0.6, 0x283c78, 0xf2765a, 0xff6a3a, 1.3, 0x7c76a8, 0x483644, 0.7, 0xa86470,
                0.00024, 1.08, 0.25
            ),
            sky!(
                0.68, -4, 0x18204e, 0xa45474, 0xd8704a, 0.5, 0x505a8c, 0x2c2434, 0.5, 0x5c4466,
                0.00026, 1.12, 0.55
            ),
            sky!(
                0.84, -9, 0x0a1030, 0x40385e, 0x9ab0e0, 0.4, 0x44508a, 0x24222e, 0.42, 0x2a2a46,
                0.00024, 1.16, 0.85
            ),
            sky!(
                1.00, -14, 0x050a20, 0x2a2a48, 0xa8bce8, 0.42, 0x3e4a7c, 0x22222c, 0.42, 0x1e2034,
                0.00022, 1.2, 1
            ),
        ],
        sun_azimuth: 0.12,
        moon_dir: Some([0.85, 0.3, -0.35]), // rising low ahead over the lake bed
        traffic_paint: None,
        traffic: vec![
            traffic!(
                [320, 620],
                [
                    ("sedan", 0.35),
                    ("pickup", 0.35),
                    ("van", 0.15),
                    ("hatch", 0.15)
                ],
                0.8,
                [13, 18]
            ),
            traffic!(
                [220, 420],
                [
                    ("pickup", 0.35),
                    ("sedan", 0.25),
                    ("boxtruck", 0.25),
                    ("van", 0.15)
                ],
                0.7,
                [20, 27]
            ),
            // Nobody else is out on the lake bed.
            traffic!([300, 500], [], 0, [0, 0]),
        ],
        // Hot Pursuit: the canyon hides you (heat 2); Route 66 and the dry lake
        // are open ground, where they can always see you (4 / 5).
        police: Some(police(&[2, 4, 5], &[false, true, true])),
        rivals: vec![
            rival!("Razor", "super", 0xe9ecef, 0.99, 530),
            rival!("Marlowe", "electric", 0x1ab8c4, 0.978, 520),
            rival!("Kaito", "sports", 0x19c46b, 0.968, 505),
            rival!("Rook", "rally", 0x2a6ee8, 0.958, 530),
            rival!("Duke", "muscle", 0x30343b, 0.94, 535),
        ],
    }
}
