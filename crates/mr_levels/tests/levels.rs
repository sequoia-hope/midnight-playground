//! Level data (src/levels/*.js): what the menu lists and the world builder,
//! terrain, traffic and rivals read from each level. Port of
//! `test/unit/levels.test.js`, same assertions, except:
//!
//! - "README: level lengths and rival counts match the data" is dropped
//!   (it asserts on README text; DEVIATIONS.md, dropped tests);
//! - a zone's landform is checked against Terrain's LANDFORMS, and its
//!   scenery against the world modules, when world generation is ported
//!   (M3); here they are checked against the names the JS uses;
//! - a rival's kind is checked against the five car names; `mr_sim`'s
//!   tests check them against CAR_SPECS (WP 1.3).

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mr_levels::{level_by_id, levels};
use mr_track::{Mode, road_type};

/// The keys of `LANDFORMS` in `src/world/Terrain.js`.
const LANDFORMS: [&str; 11] = [
    "mountain", "valley", "city", "coast", "beach", "harbor", "streets", "canyon", "desert",
    "playa", "raceway",
];
const SCENERY: [&str; 9] = [
    "Mountain", "Valley", "City", "Coast", "Beach", "Harbor", "Streets", "Desert", "Raceway",
];
const CARS: [&str; 5] = ["sports", "muscle", "super", "electric", "rally"];

#[test]
fn the_menu_lists_six_levels_with_unique_ids_in_order() {
    let ls = levels();
    let ids: Vec<_> = ls.iter().map(|l| l.id).collect();
    assert_eq!(
        ids,
        ["sierra", "coast", "streets", "desert", "seaside", "cruise"]
    );
    let nums: Vec<_> = ls.iter().map(|l| l.num).collect();
    assert_eq!(
        nums,
        [
            "LEVEL 1", "LEVEL 2", "LEVEL 3", "LEVEL 4", "LEVEL 5", "ENDLESS"
        ]
    );
}

#[test]
fn level_by_id_finds_each_level_and_falls_back_to_the_first() {
    for l in levels() {
        assert_eq!(level_by_id(l.id).id, l.id);
    }
    // A stale saved id, a bad ?level= or nothing at all: Level 1.
    for bad in ["nope", "", "SIERRA"] {
        assert_eq!(level_by_id(bad).id, "sierra");
    }
}

#[test]
fn required_fields() {
    for l in common::all() {
        for (k, v) in [
            ("id", l.id),
            ("num", l.num),
            ("title", l.title),
            ("desc", l.desc),
        ] {
            assert!(!v.is_empty(), "{}: {k} is a non-empty string", l.id);
        }
        assert!(!l.zones.is_empty(), "zones");
        assert!(l.sky.len() >= 2, "sky keyframes");
        assert!(l.sun_azimuth.is_finite(), "sunAzimuth");
        match l.mode {
            Mode::Cruise => {
                assert!(l.is_loop(), "a cruise is a loop");
                assert!(l.rivals.is_empty(), "a cruise has no rivals");
            }
            Mode::Race if l.laps.is_some() => {
                // A circuit: a loop raced over laps.
                assert!(l.is_loop(), "a circuit is a loop");
                assert!(l.laps.unwrap() >= 1, "laps");
                assert!(!l.rivals.is_empty(), "a race has rivals");
                assert!(l.police.is_none(), "no Hot Pursuit on a closed circuit");
            }
            Mode::Race => {
                assert!(!l.segments().unwrap().is_empty(), "a race has segments");
                assert!(!l.rivals.is_empty(), "a race has rivals");
                assert!(l.start_height.unwrap().is_finite(), "startHeight");
            }
        }
    }
}

#[test]
fn zones_name_a_known_landform_and_existing_scenery() {
    for l in common::all() {
        let mut keys: Vec<_> = l.zones.iter().map(|z| z.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), l.zones.len(), "unique zone keys");
        for z in &l.zones {
            for (k, v) in [("key", z.key), ("name", z.name), ("sub", z.sub)] {
                assert!(!v.is_empty(), "{}.{k}", z.key);
            }
            assert!(
                LANDFORMS.contains(&z.landform),
                "{}: landform '{}'",
                z.key,
                z.landform
            );
            assert!(
                SCENERY.contains(&z.scenery),
                "{}: scenery {}",
                z.key,
                z.scenery
            );
            let c = z.color.as_bytes();
            assert!(
                c.len() == 7 && c[0] == b'#' && c[1..].iter().all(u8::is_ascii_hexdigit),
                "{}: color",
                z.key
            );
        }
    }
}

#[test]
fn sky_keyframes_run_from_0_to_1_with_finite_values() {
    for l in common::all() {
        assert_eq!(l.sky[0].s, 0.0);
        assert_eq!(l.sky.last().unwrap().s, 1.0);
        for (i, k) in l.sky.iter().enumerate() {
            if i > 0 {
                assert!(k.s > l.sky[i - 1].s, "s increases at keyframe {i}");
            }
            for v in [k.s, k.sun_el, k.sun_i, k.hemi_i, k.fog_d, k.exp, k.night] {
                assert!(v.is_finite(), "{}: keyframe {i}", l.id);
            }
            assert!(k.night >= 0.0 && k.night <= 1.0, "night {}", k.night);
            assert!(k.fog_d > 0.0 && k.exp > 0.0 && k.sun_i >= 0.0 && k.hemi_i >= 0.0);
        }
    }
}

#[test]
fn segments_are_well_formed() {
    for l in common::all() {
        let Some(segs) = l.segments() else { continue };
        let first = &segs[0].extra;
        assert_eq!(first.zone, Some(0), "the first segment starts zone 0");
        assert!(
            first.road.is_some(),
            "the first segment names its road type"
        );
        let mut zone = 0;
        for (i, s) in segs.iter().enumerate() {
            assert!(
                s.len == s.len.floor() && s.len > 0.0,
                "segment {i}: length {}",
                s.len
            );
            assert!(
                s.turn.is_finite() && s.turn.abs() <= 270.0,
                "segment {i}: turn {}",
                s.turn
            );
            assert!(s.rise.is_finite(), "segment {i}: rise {}", s.rise);
            assert!(
                (s.rise / s.len).abs() < 0.3,
                "segment {i}: grade {}",
                s.rise / s.len
            );
            if let Some(r) = s.extra.road {
                assert!(road_type(r).is_some(), "segment {i}: road '{r}'");
            }
            if let Some(z) = s.extra.zone {
                assert!(
                    z >= zone && (z as usize) < l.zones.len(),
                    "segment {i}: zone {z} (zones only move forward)"
                );
                zone = z;
            }
        }
        assert_eq!(
            zone as usize,
            l.zones.len() - 1,
            "the route reaches the last zone"
        );
    }
}

#[test]
fn rivals_drive_real_cars() {
    for l in common::all() {
        let mut names: Vec<_> = l.rivals.iter().map(|r| r.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), l.rivals.len(), "unique names");
        for r in &l.rivals {
            assert!(CARS.contains(&r.kind), "{}: kind '{}'", r.name, r.kind);
            assert!(
                r.skill > 0.8 && r.skill <= 1.0,
                "{}: skill {}",
                r.name,
                r.skill
            );
            assert!(
                r.power > 300.0 && r.power < 700.0,
                "{}: power {}",
                r.name,
                r.power
            );
            assert!(r.color <= 0xffffff, "{}: color", r.name);
        }
    }
}

#[test]
fn one_traffic_rule_per_zone_with_sane_numbers() {
    for l in common::all() {
        // Traffic.update reads rules[zone].gap for every spawn point.
        assert_eq!(l.traffic.len(), l.zones.len());
        for (i, r) in l.traffic.iter().enumerate() {
            assert!(
                r.gap[0] > 0.0 && r.gap[0] <= r.gap[1],
                "rule {i}: gap {:?}",
                r.gap
            );
            assert!(
                r.oncoming >= 0.0 && r.oncoming <= 1.0,
                "rule {i}: oncoming {}",
                r.oncoming
            );
            if let Some(o) = r.opposite {
                assert!((0.0..=1.0).contains(&o), "rule {i}: opposite");
            }
            // An empty mix is a zone with no traffic (the desert's lake bed).
            if !r.mix.is_empty() {
                assert!(
                    r.speed[0] > 0.0 && r.speed[0] <= r.speed[1] && r.speed[1] < 45.0,
                    "rule {i}: speed {:?}",
                    r.speed
                );
                let sum: f64 = r.mix.iter().map(|(_, w)| w).sum();
                assert!(
                    (sum - 1.0).abs() < 1e-6,
                    "rule {i}: mix weights sum to {sum}"
                );
                for (k, w) in &r.mix {
                    assert!(!k.is_empty() && *w > 0.0, "rule {i}: {k} {w}");
                }
            }
        }
    }
}

/// The menu card quotes the lap; it has to be the surveyed one.
#[test]
fn seaside_the_lap_length_the_menu_shows_is_the_surveys() {
    let sea = common::level("seaside");
    let d = common::survey();
    assert!(
        (sea.lap_length.unwrap() - d.lap).abs() < 1.0,
        "lapLength {:?} vs survey {}",
        sea.lap_length,
        d.lap
    );
}
