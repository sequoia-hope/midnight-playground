//! Level data invariants beyond the JS unit tests (`levels.rs`): the menu
//! is the same every time and `level_by_id` hands back the very level the
//! menu lists, the fallback is the menu's first level whatever the input,
//! every optional number and colour is in range, police and loop specs fit
//! their zones and laps, and every start-grid spot (`Race.js`) is on the
//! tarmac.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mp_levels::world::world_data;
use mp_levels::{level_by_id, levels};
use mp_track::{Level, Mode, Route, road_type};

/// Everything a level holds as plain data, for comparing two copies.
fn fingerprint(l: &Level) -> String {
    let route = match &l.route {
        Route::Segments(s) => format!("{s:?}"),
        Route::Loop(spec) => format!("{spec:?} roads {:?}", spec.roads),
    };
    format!(
        "{} {:?} {} {} {} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?} {} {:?} {:?} {:?} {:?} {:?} {route}",
        l.id,
        l.mode,
        l.num,
        l.title,
        l.desc,
        l.laps,
        l.lap_length,
        l.start_height,
        l.start_heading,
        l.start_x,
        l.start_z,
        l.finish_runoff,
        l.elevation_smooth,
        l.sea_y,
        l.zones,
        l.sky,
        l.sun_azimuth,
        l.moon_dir,
        l.traffic_paint,
        l.traffic,
        l.police,
        l.rivals,
    )
}

#[test]
fn the_menu_is_the_same_every_time() {
    let a: Vec<String> = levels().iter().map(fingerprint).collect();
    let b: Vec<String> = levels().iter().map(fingerprint).collect();
    assert_eq!(a, b);
}

#[test]
fn level_by_id_returns_the_menus_level_not_just_its_id() {
    for l in levels() {
        assert_eq!(fingerprint(&level_by_id(l.id)), fingerprint(&l), "{}", l.id);
    }
}

#[test]
fn coast_then_sierra_lead_the_menu_and_every_unknown_id_falls_back_to_the_first() {
    let first = &levels()[0];
    assert_eq!(first.id, "coast", "D1102: Coast Highway is Level 1");
    assert_eq!(levels()[1].id, "sierra", "D1102: Sierra second");
    for bad in [
        " coast", "coast ", "Coast", "coast\0", "seaside/", "levels", "1", "LEVEL 1", "🏁",
    ] {
        assert_eq!(
            fingerprint(&level_by_id(bad)),
            fingerprint(first),
            "{bad:?} falls back to the first level"
        );
    }
}

#[test]
fn titles_and_descriptions_are_distinct_and_read_as_text() {
    let ls = levels();
    for (i, a) in ls.iter().enumerate() {
        for b in &ls[i + 1..] {
            assert_ne!(a.title, b.title);
            assert_ne!(a.desc, b.desc);
        }
        for (what, s) in [("title", a.title), ("desc", a.desc), ("num", a.num)] {
            assert_eq!(s.trim(), s, "{}: {what} has stray spaces", a.id);
            assert!(!s.contains("  "), "{}: {what} has a double space", a.id);
        }
        assert!(
            a.desc.ends_with('.'),
            "{}: the description is a sentence",
            a.id
        );
        assert!(
            a.id.bytes().all(|b| b.is_ascii_lowercase()),
            "{}: ids are lower-case words (saved settings, ?level=)",
            a.id
        );
    }
}

#[test]
fn the_endless_cruise_is_the_only_cruise_and_comes_last() {
    let ls = levels();
    let cruises: Vec<_> = ls.iter().filter(|l| l.mode == Mode::Cruise).collect();
    assert_eq!(cruises.len(), 1);
    assert_eq!(ls.last().unwrap().mode, Mode::Cruise);
    assert_eq!(ls.last().unwrap().num, "ENDLESS");
    for (k, l) in ls.iter().filter(|l| l.mode == Mode::Race).enumerate() {
        assert_eq!(l.num, format!("LEVEL {}", k + 1));
    }
}

#[test]
fn optional_numbers_are_finite_and_plausible() {
    for l in common::all() {
        let id = l.id;
        if let Some(h) = l.start_heading {
            assert!(h.is_finite() && h.abs() <= 360.0, "{id}: startHeading {h}");
        }
        for (what, v) in [("startX", l.start_x), ("startZ", l.start_z)] {
            if let Some(v) = v {
                assert!(v.is_finite() && v.abs() < 1e5, "{id}: {what} {v}");
            }
        }
        if let Some(h) = l.start_height {
            assert!((-100.0..3000.0).contains(&h), "{id}: startHeight {h}");
        }
        if let Some(r) = l.finish_runoff {
            assert!(r > 0.0 && r < 1000.0, "{id}: finishRunoff {r}");
        }
        if let Some(e) = l.elevation_smooth {
            assert!(e > 0.0 && e.is_finite(), "{id}: elevationSmooth {e}");
        }
        if let Some(y) = l.sea_y {
            assert!(y.is_finite(), "{id}: seaY {y}");
        }
        if let Some(len) = l.lap_length {
            assert!(l.laps.is_some(), "{id}: a lap length without laps");
            assert!(len > 1000.0 && len < 20000.0, "{id}: lapLength {len}");
        }
        if let Some(n) = l.laps {
            assert!((1..=10).contains(&n), "{id}: laps {n}");
        }
        assert!(
            l.sun_azimuth.abs() <= 360.0,
            "{id}: sunAzimuth {}",
            l.sun_azimuth
        );
        if let Some(m) = l.moon_dir {
            assert!(m.iter().all(|v| v.is_finite()), "{id}: moonDir");
            assert!(m[1] > 0.0, "{id}: the moon is above the horizon");
        }
        if let Some(p) = &l.traffic_paint {
            assert!(!p.is_empty(), "{id}: an empty paint list");
            assert!(p.iter().all(|&c| c <= 0xffffff), "{id}: paint {p:x?}");
        }
    }
}

#[test]
fn sky_colours_and_angles_are_in_range() {
    for l in levels() {
        for (i, k) in l.sky.iter().enumerate() {
            for c in [k.zen, k.hor, k.sun, k.hemi_s, k.hemi_g, k.fog] {
                assert!(c <= 0xffffff, "{} key {i}: colour {c:#x}", l.id);
            }
            assert!(
                (-90.0..=90.0).contains(&k.sun_el),
                "{} key {i}: sun elevation {}",
                l.id,
                k.sun_el
            );
            assert!(
                k.fog_d < 0.01,
                "{} key {i}: fog density {} hides the road",
                l.id,
                k.fog_d
            );
        }
    }
}

/// `Pursuit.js` reads `heatCap[zone] ?? 5` and `!!losOpenGround[zone]`:
/// an entry per zone, caps within the five heat levels.
#[test]
fn police_settings_cover_every_zone() {
    for l in levels() {
        let Some(p) = &l.police else { continue };
        assert_eq!(p.heat_cap.len(), l.zones.len(), "{}: heatCap", l.id);
        assert_eq!(
            p.los_open_ground.len(),
            l.zones.len(),
            "{}: losOpenGround",
            l.id
        );
        for &c in &p.heat_cap {
            assert!((1..=5).contains(&c), "{}: heat cap {c}", l.id);
        }
        assert!(
            p.heat_cap.windows(2).all(|w| w[0] <= w[1]),
            "{}: the heat cap never drops along the route {:?}",
            l.id,
            p.heat_cap
        );
        assert!(l.laps.is_none(), "{}: no pursuit on a circuit", l.id);
    }
}

#[test]
fn rival_colours_and_names_are_distinct_within_a_level() {
    for l in levels() {
        for (i, a) in l.rivals.iter().enumerate() {
            for b in &l.rivals[i + 1..] {
                assert_ne!(
                    a.color, b.color,
                    "{}: {} and {} share a colour",
                    l.id, a.name, b.name
                );
            }
            assert_eq!(a.name.trim(), a.name, "{}: rival name", l.id);
        }
        assert!(l.rivals.len() <= 5, "{}: the grid has six places", l.id);
    }
}

#[test]
fn loop_specs_fit_their_lap_and_zones() {
    for (l, t) in common::tracks() {
        let Some(spec) = l.loop_spec() else { continue };
        let id = l.id;
        if let Some(r) = spec.road {
            assert!(road_type(r).is_some(), "{id}: road {r}");
        }
        for r in &spec.roads {
            assert!(road_type(r.road).is_some(), "{id}: road {}", r.road);
            assert!(r.s0 < r.s1, "{id}: road range {}..{}", r.s0, r.s1);
        }
        if let Some(s) = spec.start_s {
            assert!(s >= 0.0 && s < t.length, "{id}: startS {s}");
        }
        match &spec.zones {
            Some(z) => {
                assert_eq!(z.len(), l.zones.len(), "{id}: a start per zone");
                assert_eq!(z[0], 0.0, "{id}: the first zone starts the lap");
                assert!(z.windows(2).all(|w| w[0] < w[1]), "{id}: {z:?}");
                assert!(*z.last().unwrap() < t.length, "{id}: {z:?}");
            }
            None => assert_eq!(l.zones.len(), 1, "{id}: one zone without starts"),
        }
        for g in &spec.tags {
            assert!(!g.tag.is_empty());
            match (g.s0, g.s1, g.f0, g.f1) {
                (Some(a), Some(b), None, None) => {
                    assert!(
                        0.0 <= a && a < b && b <= t.length,
                        "{id}: {} {a}..{b}",
                        g.tag
                    )
                }
                (None, None, Some(a), Some(b)) => {
                    assert!(0.0 <= a && a < b && b <= 1.0, "{id}: {} {a}..{b}", g.tag)
                }
                other => panic!("{id}: {} has a mixed range {other:?}", g.tag),
            }
        }
        // The path is the same every time it is asked for.
        assert_eq!((spec.path)().unwrap(), (spec.path)().unwrap(), "{id}");
    }
}

#[test]
fn the_seaside_loop_has_no_path_until_its_survey_is_loaded() {
    let raw = level_by_id("seaside");
    let spec = raw.loop_spec().unwrap();
    assert!((spec.path)().is_err());
    assert!(mp_track::Track::new(&raw).is_err());
    assert!(raw.ground.is_none() && raw.loose_ground.is_none());
    let prepared = common::level("seaside");
    assert!((prepared.loop_spec().unwrap().path)().is_ok());
    assert!(prepared.ground.is_some() && prepared.loose_ground.is_some());
}

/// `Race.js`: rows of two from `startS - 5`, ten metres between rows,
/// the second column 3 m further back and the columns at lat ±2.4; the
/// player fourth when there are five rivals, else first.
#[test]
fn every_start_grid_spot_is_on_the_tarmac() {
    const HALF_CAR: f64 = 1.0;
    for (l, t) in common::tracks() {
        let cars = l.rivals.len() + 1;
        for k in 0..cars {
            let (row, col) = ((k / 2) as f64, k % 2);
            let s = t.start_s - 5.0 - row * 10.0 - col as f64 * 3.0;
            let lat: f64 = if col == 1 { 2.4 } else { -2.4 };
            if !t.is_loop {
                assert!(s >= 0.0, "{}: grid spot {k} at s {s} before the road", l.id);
            }
            let f = t.frame(t.wrap(s));
            assert!(
                lat.abs() + HALF_CAR <= f.hw,
                "{}: grid spot {k} at s {s}, lat {lat}: half-width {}",
                l.id,
                f.hw
            );
            let p = t.point_at(t.wrap(s), lat);
            for v in [p.x, p.y, p.z] {
                assert!(v.is_finite(), "{}: grid spot {k}", l.id);
            }
            assert!(t.surface_y(t.wrap(s), lat).is_finite());
            // The grid is well short of the finish.
            assert!(s < t.finish_s, "{}: grid spot {k}", l.id);
        }
    }
}

/// `Track.js` starts the route at `(startX ?? 0, startZ ?? 0)`.
#[test]
fn a_point_to_point_road_starts_where_its_level_says() {
    for (l, t) in common::tracks() {
        if t.is_loop {
            continue;
        }
        let (x, z) = (l.start_x.unwrap_or(0.0), l.start_z.unwrap_or(0.0));
        assert!(
            (f64::from(t.px[0]) - x).abs() < 1e-3 && (f64::from(t.pz[0]) - z).abs() < 1e-3,
            "{}: first sample ({}, {}) vs start ({x}, {z})",
            l.id,
            t.px[0],
            t.pz[0]
        );
    }
}

#[test]
fn world_data_by_an_unknown_id_is_the_first_levels() {
    let first = levels()[0].id;
    assert_eq!(world_data("no-such-level"), world_data(first));
    // Levels without a City or a Harbor have neither runout nor carriageway.
    for id in ["seaside", "streets", "desert"] {
        let w = world_data(id);
        assert!(w.opposite_carriageway.is_none(), "{id}");
        assert_eq!(w.runout, 0.0, "{id}");
    }
}
