//! Seaside Raceway (src/levels/seaside.js + src/levels/seaside/).
//!
//! First, L2: the survey data read from `assets/seaside/survey.bin` is what
//! `load.js` decodes from `circuit.js` and `ground.js`, bit for bit
//! (`parity/golden/seaside/survey.json`, `tools/parity/seaside-golden.mjs`).
//!
//! Then the Track parts of `test/unit/seaside.test.js`, same assertions: the
//! survey decodes to the real circuit (its length, its 55 m of climb and the
//! Corkscrew's drop, the tarmac's measured width), the barriers stand off
//! the tarmac, the lidar ground meets the road, the run-off surface joins
//! the road without a step. The tests that need Terrain (M3), CarPhysics
//! (WP 1.3) and AIDriver (WP 1.4) are ported with those.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use std::f64::consts::PI;

use mp_math::{Mulberry32, kernel::hypot};
use mp_track::{RUNOFF_FLAT, Track};
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/seaside/survey.json");

fn fnv1a64(values: &[f64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for v in values {
        for b in v.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

fn hex(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn check(name: &str, values: Vec<f64>, g: &Value) {
    let values: Vec<f64> = values
        .into_iter()
        .map(|v| if v.is_nan() { f64::NAN } else { v })
        .collect();
    assert_eq!(
        values.len() as u64,
        g["n"].as_u64().unwrap(),
        "{name}: length"
    );
    for (i, h) in g["head"].as_array().unwrap().iter().enumerate() {
        assert_eq!(hex(values[i]), h.as_str().unwrap(), "{name}[{i}]");
    }
    assert_eq!(
        format!("{:016x}", fnv1a64(&values)),
        g["hash"].as_str().unwrap(),
        "{name}: hash"
    );
}

fn f32s(v: &[f32]) -> Vec<f64> {
    v.iter().map(|&x| x as f64).collect()
}

#[test]
fn the_survey_decodes_as_load_js_decodes_it() {
    let g: Value = serde_json::from_str(GOLDEN).unwrap();
    let d = common::survey();
    let o = g["origin"].as_array().unwrap();
    for (v, want) in d.origin.iter().zip(o) {
        assert_eq!(hex(*v), want.as_str().unwrap(), "origin");
    }
    assert_eq!(hex(d.lap), g["lap"].as_str().unwrap(), "lap");
    let l = &d.line;
    for (k, v) in [
        ("x", &l.x),
        ("z", &l.z),
        ("y", &l.y),
        ("bank", &l.bank),
        ("hw", &l.hw),
        ("wallL", &l.wall_l),
        ("wallR", &l.wall_r),
        ("runL", &l.run_l),
        ("runR", &l.run_r),
    ] {
        check(&format!("line.{k}"), v.clone(), &g["line"][k]);
    }
    let p = &d.photo;
    for (k, v) in [p.x0, p.z0, p.x1, p.z1].into_iter().enumerate() {
        assert_eq!(hex(v), g["photo"][k].as_str().unwrap(), "photo");
    }
    for (k, grid) in [
        ("heightFine", &d.height_fine),
        ("colorFine", &d.color_fine),
        ("treesFine", &d.trees_fine),
        ("treesWide", &d.trees_wide),
    ] {
        let e = &g["grids"][k];
        assert_eq!(e["w"].as_u64().unwrap() as usize, grid.w, "{k}.w");
        assert_eq!(e["h"].as_u64().unwrap() as usize, grid.h, "{k}.h");
        assert_eq!(hex(grid.x0), e["x0"].as_str().unwrap(), "{k}.x0");
        assert_eq!(hex(grid.z0), e["z0"].as_str().unwrap(), "{k}.z0");
        check(k, f32s(&grid.values), &e["values"]);
    }
    check(
        "loose.values",
        f32s(&d.loose_grid.values),
        &g["loose"]["values"],
    );
    check(
        "pitLane",
        d.pit_lane.iter().flat_map(|p| [p.x, p.z]).collect(),
        &g["pitLane"],
    );
    check(
        "walls",
        d.walls
            .iter()
            .flat_map(|w| w.iter().flat_map(|p| [p.x, p.z]))
            .collect(),
        &g["walls"],
    );
    let names: Vec<&str> = d.buildings.iter().map(|b| b.name.as_str()).collect();
    let want: Vec<&str> = g["buildings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(names, want, "buildings");
    let kinds: Vec<&str> = d.paths.iter().map(|b| b.name.as_str()).collect();
    let want: Vec<&str> = g["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(kinds, want, "paths");

    // The samplers, at the tool's points.
    let mut pts = Vec::new();
    let mut r = Mulberry32::new(17);
    for (n, scale) in [(6000, 4000.0), (6000, 1100.0)] {
        for _ in 0..n {
            let x = (r.next_f64() * 2.0 - 1.0) * scale;
            let z = (r.next_f64() * 2.0 - 1.0) * scale;
            pts.push((x, z));
        }
    }
    let mut s = 0;
    while s < l.n {
        for lat in [-30.0, -18.0, -9.0, 9.0, 18.0, 30.0] {
            let (i, j) = (s, (s + 1) % l.n);
            let (dx, dz) = (l.x[j] - l.x[i], l.z[j] - l.z[i]);
            let len = hypot(dx, dz);
            pts.push((l.x[i] - (dz / len) * lat, l.z[i] + (dx / len) * lat));
        }
        s += 3;
    }
    check(
        "points",
        pts.iter().flat_map(|&(x, z)| [x, z]).collect(),
        &g["points"],
    );
    check(
        "height",
        pts.iter().map(|&(x, z)| d.height(x, z)).collect(),
        &g["height"],
    );
    check(
        "color",
        pts.iter().flat_map(|&(x, z)| d.color(x, z)).collect(),
        &g["color"],
    );
    check(
        "trees",
        pts.iter().map(|&(x, z)| d.trees(x, z)).collect(),
        &g["trees"],
    );
    check(
        "loose",
        pts.iter().map(|&(x, z)| d.loose(x, z)).collect(),
        &g["loose"]["at"],
    );
}

fn track() -> &'static Track {
    &common::tracks()
        .iter()
        .find(|(l, _)| l.id == "seaside")
        .unwrap()
        .1
}

#[test]
fn the_lap_is_the_surveyed_centreline_3_6_km_anticlockwise_three_laps() {
    let t = track();
    let d = common::survey();
    assert!(
        (t.n as f64 - 3602.0).abs() < 15.0,
        "lap {} m (Laguna Seca: 3,602 m)",
        t.n
    );
    assert!(
        (t.n as f64 - d.lap).abs() < 1.0,
        "the track is as long as the survey"
    );
    assert_eq!(t.laps, 3);
    assert_eq!(t.start_s, 0.0, "the lap starts on the line");
    assert_eq!(
        t.finish_s,
        f64::INFINITY,
        "the finish is by laps, not by distance"
    );
    // Anticlockwise seen from above: in x-east / z-south coordinates the
    // signed area is negative, and most of the turning is to the left.
    let (mut area, mut left, mut right) = (0.0, 0.0, 0.0);
    for i in 0..t.n {
        let j = (i + 1) % t.n;
        area += t.px[i] as f64 * t.pz[j] as f64 - t.px[j] as f64 * t.pz[i] as f64;
        let k = t.kappa[i] as f64;
        if k < 0.0 {
            left -= k;
        } else {
            right += k;
        }
    }
    assert!(area < 0.0, "anticlockwise");
    assert!(
        (left - right - PI * 2.0).abs() < 0.05,
        "net turning is one full turn left ({:.3})",
        left - right
    );
}

#[test]
fn the_heights_are_the_lidars_55_m_of_climb_and_the_corkscrews_drop() {
    let t = track();
    let (mut lo, mut hi, mut top) = (f64::INFINITY, f64::NEG_INFINITY, 0usize);
    for s in 0..t.n {
        let y = t.py[s] as f64;
        lo = lo.min(y);
        hi = hi.max(y);
        if t.py[s] > t.py[top] {
            top = s;
        }
    }
    // 180 ft from the lowest point of the lap to the highest.
    assert!(
        (hi - lo - 54.9).abs() < 1.0,
        "height range {:.2} m",
        hi - lo
    );
    // The top of the hill is the Corkscrew, and it drops about 18 m in the
    // next 150 m.
    let cork = t.tags.iter().find(|g| g.tag == "corkscrew").unwrap();
    assert!(
        top as f64 > cork.s0 - 80.0 && (top as f64) < cork.s0 + 20.0,
        "the top (s {top}) is at the Corkscrew ({})",
        cork.s0
    );
    let drop = t.py[top + 50] as f64 - t.py[top + 200] as f64;
    assert!(
        drop > 16.0 && drop < 21.0,
        "Corkscrew drop {drop:.1} m in 150 m"
    );
    // Real camber, but nothing wild.
    for s in 0..t.n {
        assert!((t.bank[s] as f64).abs() < 0.16, "bank {} at {s}", t.bank[s]);
    }
}

#[test]
fn the_barriers_stand_off_the_tarmac_as_far_out_as_the_real_walls() {
    let t = track();
    let mut wide = 0;
    for s in 0..t.n {
        for w in [t.wall_l[s] as f64, t.wall_r[s] as f64] {
            let hw = t.hw[s] as f64;
            assert!(w >= hw + 1.5 - 1e-4, "wall {w:.1} m at s {s} (hw {hw:.1})");
            assert!(w <= 34.5, "wall {w:.1} m at s {s}");
            if w > hw + 10.0 {
                wide += 1;
            }
        }
    }
    // Plenty of room to run wide somewhere, as at the real circuit.
    assert!(
        wide as f64 > t.n as f64 * 0.3,
        "{wide} of {} sides have more than 10 m of run-off",
        2 * t.n
    );
}

#[test]
fn the_run_off_leaves_the_road_without_a_step_and_the_lidar_ground_meets_it() {
    let t = track();
    let level = common::level("seaside");
    let mut s = 0.5;
    while s < t.n as f64 {
        let f = t.frame(s);
        for side in [-1.0, 1.0] {
            // Continuous across the tarmac edge and where the grade takes over.
            for lat in [f.hw, f.hw + RUNOFF_FLAT] {
                let a = t.surface_y(s, side * (lat - 0.01));
                let b = t.surface_y(s, side * (lat + 0.01));
                assert!(
                    (a - b).abs() < 0.01,
                    "step of {:.3} m at s {s:.1}, lat {}",
                    b - a,
                    side * lat
                );
            }
            // pointAt agrees with surfaceY out on the run-off.
            let w = if side < 0.0 { f.wall_l } else { f.wall_r };
            let p = t.point_at(s, side * (w - 0.5));
            assert!((p.y - t.surface_y(s, side * (w - 0.5))).abs() < 1e-4);
        }
        s += 37.3;
    }
    // The surveyed road sits on the surveyed ground (both from the lidar).
    let ground = level.ground.as_ref().unwrap();
    let mut worst: f64 = 0.0;
    let mut s = 0;
    while s < t.n {
        worst = worst.max((ground(t.px[s] as f64, t.pz[s] as f64) - t.py[s] as f64).abs());
        s += 11;
    }
    assert!(
        worst < 0.6,
        "road vs ground on the centreline: worst {worst:.2} m"
    );
}

#[test]
fn the_run_off_is_the_real_mix_of_paved_and_loose_and_the_photo_covers_the_lap() {
    let t = track();
    let level = common::level("seaside");
    let loose = level.loose_ground.as_ref().unwrap();
    let (mut paved, mut all) = (0.0, 0.0);
    let mut s = 0;
    while s < t.n {
        let f = t.frame(s as f64);
        for side in [-1.0, 1.0] {
            let w = if side < 0.0 { f.wall_l } else { f.wall_r };
            let mut lat = f.hw + 2.0;
            while lat < w - 1.0 {
                let p = t.point_at(s as f64, side * lat);
                paved += 1.0 - loose(p.x, p.z);
                all += 1.0;
                lat += 2.0;
            }
        }
        s += 3;
    }
    // Laguna Seca's run-off is about half asphalt now.
    assert!(
        paved / all > 0.3 && paved / all < 0.75,
        "{:.0} % of the run-off is paved",
        paved / all * 100.0
    );
    let p = &common::survey().photo;
    let mut s = 0;
    while s < t.n {
        let (x, z) = (t.px[s] as f64, t.pz[s] as f64);
        assert!(
            x > p.x0 + 100.0 && x < p.x1 - 100.0 && z > p.z0 + 100.0 && z < p.z1 - 100.0,
            "the photo reaches 100 m past s {s}"
        );
        s += 50;
    }
    let g = &common::survey().loose_grid;
    assert_eq!(g.w * g.h, g.values.len());
}

#[test]
fn the_tarmac_is_as_wide_as_the_photo_shows() {
    // Narrowest between Five and Six, widest down the pit straight.
    let t = track();
    let (mut lo, mut hi, mut lo_s) = (f64::INFINITY, 0.0, 0);
    for s in 0..t.n {
        let h = t.hw[s] as f64;
        if h < lo {
            lo = h;
            lo_s = s;
        }
        if h > hi {
            hi = h;
        }
    }
    assert!(lo >= 5.2 && hi <= 7.8, "half-widths {lo:.2}..{hi:.2} m");
    assert!(lo_s > 1700 && lo_s < 2050, "narrowest at s {lo_s}");
    let pit = t.tag("pit-straight")[0];
    let mut pit_w = 0.0;
    let mut s = pit.s0 + 20.0;
    while s < pit.s1 - 20.0 {
        pit_w += 2.0 * t.hw[s as usize] as f64 / (pit.s1 - pit.s0 - 40.0);
        s += 1.0;
    }
    assert!(pit_w > 14.0, "pit straight {pit_w:.1} m wide");
}
