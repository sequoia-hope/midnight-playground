//! track/Track.js on every level: the route sampled every metre, its zones,
//! the frame and projection queries physics and AI use, the racing line and
//! speed profile, and that no two parts of a road run into each other. Port
//! of `test/unit/track.test.js`, same assertions.
//!
//! Not here: "the terrain never covers the road" needs Terrain.js, which is
//! world generation (roadmap M3); it is ported with it.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mr_math::{Mulberry32, kernel::hypot};

fn near(a: f64, b: f64, eps: f64, msg: &str) {
    assert!((a - b).abs() <= eps, "{msg}: {a} vs {b}");
}

#[test]
fn length_start_and_finish() {
    for (l, t) in common::tracks() {
        assert_eq!(t.is_loop, l.is_loop());
        if t.is_loop {
            assert_eq!(t.length, t.n as f64, "a loop is as long as its samples");
            assert_eq!(t.finish_s, f64::INFINITY, "a loop has no finish");
            assert_eq!(t.road_end(), f64::INFINITY);
        } else {
            let total: f64 = l.segments().unwrap().iter().map(|s| s.len).sum();
            assert_eq!(t.length, total, "one sample per metre");
            assert_eq!(t.n as f64, total + 1.0);
            assert_eq!(t.finish_s, t.length - l.finish_runoff.unwrap_or(180.0));
            assert!(
                t.finish_s > 3000.0,
                "a real race (finish at {} m)",
                t.finish_s
            );
        }
        // (A circuit starts its lap on the line: startS 0.)
        let ok = if t.laps > 0 {
            t.start_s >= 0.0 && t.start_s < 200.0
        } else {
            t.start_s > 0.0 && t.start_s < 200.0
        };
        assert!(ok, "{}: startS {}", l.id, t.start_s);
    }
}

#[test]
fn samples_are_finite() {
    for (l, t) in common::tracks() {
        let arrays: [(&str, &Vec<f32>); 16] = [
            ("px", &t.px),
            ("py", &t.py),
            ("pz", &t.pz),
            ("hw", &t.hw),
            ("fx", &t.fx),
            ("fz", &t.fz),
            ("rx", &t.rx),
            ("rz", &t.rz),
            ("kappa", &t.kappa),
            ("kSmooth", &t.k_smooth),
            ("grade", &t.grade),
            ("bank", &t.bank),
            ("wallL", &t.wall_l),
            ("wallR", &t.wall_r),
            ("speedProfile", &t.speed_profile),
            ("racingLine", &t.racing_line),
        ];
        for (name, a) in arrays {
            assert_eq!(a.len(), t.n, "{name} has a value per sample");
            for (i, v) in a.iter().enumerate() {
                assert!(v.is_finite(), "{}: {name}[{i}] = {v}", l.id);
            }
        }
        for i in 0..t.n {
            assert!(t.hw[i] > 3.0 && t.hw[i] < 12.0, "hw[{i}] = {}", t.hw[i]);
            assert!(
                t.wall_l[i] >= t.hw[i] && t.wall_r[i] >= t.hw[i],
                "wall inside the tarmac at {i}"
            );
            near(
                hypot(t.fx[i] as f64, t.fz[i] as f64),
                1.0,
                1e-5,
                &format!("forward is a unit vector at {i}"),
            );
        }
    }
}

#[test]
fn consecutive_samples_are_a_metre_apart() {
    for (l, t) in common::tracks() {
        let last = if t.is_loop { t.n } else { t.n - 1 };
        for i in 0..last {
            let j = (i + 1) % t.n;
            let d = hypot((t.px[j] - t.px[i]) as f64, (t.pz[j] - t.pz[i]) as f64);
            assert!(
                (d - 1.0).abs() <= 0.02,
                "{}: samples {i}→{j} are {d:.3} m apart",
                l.id
            );
        }
    }
}

#[test]
fn zones_are_contiguous_and_cover_the_route() {
    for (l, t) in common::tracks() {
        assert_eq!(t.zones.len(), l.zones.len());
        assert_eq!(t.zones[0].s0, 0.0);
        assert_eq!(t.zones.last().unwrap().s1, t.length);
        for (z, zn) in t.zones.iter().enumerate() {
            assert!(zn.s1 > zn.s0, "zone {} has length", zn.zone.key);
            if z > 0 {
                assert_eq!(
                    zn.s0,
                    t.zones[z - 1].s1,
                    "zone {} starts where the last ends",
                    zn.zone.key
                );
            }
            assert_eq!(zn.zone.key, l.zones[z].key);
            let mut s = zn.s0;
            while s < zn.s1 {
                assert_eq!(
                    t.zone[s as usize] as usize, z,
                    "sample {s} is in zone {}",
                    zn.zone.key
                );
                s += 97.0;
            }
        }
        // Blend weights always sum to one.
        let mut s = 0.0;
        while s < t.length {
            let w = t.zone_blend(s, 250.0);
            near(w.iter().sum(), 1.0, 1e-9, &format!("zoneBlend({s})"));
            for x in w {
                assert!((-1e-9..=1.0 + 1e-9).contains(&x));
            }
            s += 211.0;
        }
    }
}

#[test]
fn frame_and_point_at_describe_the_road() {
    for (l, t) in common::tracks() {
        let f = t.frame(0.0);
        near(f.x, t.px[0] as f64, 1e-4, "frame(0).x");
        near(f.z, t.pz[0] as f64, 1e-4, "frame(0).z");
        near(f.y, t.py[0] as f64, 1e-4, "frame(0).y");
        let mut s = 0.5;
        while s < t.length {
            let fr = t.frame(s);
            near(hypot(fr.fx, fr.fz), 1.0, 1e-6, &format!("|forward| at {s}"));
            near(
                fr.fx * fr.rx + fr.fz * fr.rz,
                0.0,
                1e-6,
                &format!("right ⟂ forward at {s}"),
            );
            // Right is forward turned clockwise seen from above: (−fz, fx).
            near(fr.rx, -fr.fz, 1e-9, "rx");
            near(fr.rz, fr.fx, 1e-9, "rz");
            // Interpolates between its two samples.
            let i = s.floor() as usize;
            let (a, b) = (t.px[i] as f64, t.px[(i + 1) % t.n] as f64);
            assert!(
                a.min(b) - 1e-3 <= fr.x && fr.x <= a.max(b) + 1e-3,
                "{}: frame({s}) between samples",
                l.id
            );
            for lat in [-4.0, 0.0, 3.0] {
                let p = t.point_at(s, lat);
                near(
                    hypot(p.x - fr.x, p.z - fr.z),
                    f64::abs(lat),
                    1e-4,
                    &format!("pointAt({s}, {lat}) is {lat} m across"),
                );
                near(
                    p.y,
                    t.surface_y(s, lat),
                    1e-4,
                    "pointAt sits on the surface",
                );
            }
            s += 173.3;
        }
    }
}

#[test]
fn project_inverts_point_at() {
    for (l, t) in common::tracks() {
        let mut r = Mulberry32::new(l.id.len() as u32);
        for _ in 0..300 {
            let s = 5.0 + r.next_f64() * (t.length - 10.0);
            let lat = (r.next_f64() * 2.0 - 1.0) * (t.hw[t.idx(s)] as f64 - 0.5);
            let p = t.point_at(s, lat);
            // A nearby hint, as the physics passes last frame's s.
            let q = t.project(p.x, p.z, t.wrap(s + (r.next_f64() * 2.0 - 1.0) * 8.0));
            let ds = if t.is_loop { t.ds(s, q.s) } else { q.s - s };
            assert!(ds.abs() < 0.15, "{}: s {s:.2} → {:.2}", l.id, q.s);
            assert!(
                (q.lat - lat).abs() < 0.15,
                "{}: lat {lat:.2} → {:.2} at s {s:.1}",
                l.id,
                q.lat
            );
        }
    }
}

#[test]
fn idx_wrap_and_ds() {
    for (_, t) in common::tracks() {
        let n = t.n as f64;
        if t.is_loop {
            assert_eq!(t.idx(-1.0), t.n - 1);
            assert_eq!(t.idx(n), 0);
            assert_eq!(t.idx(n + 5.2), 5);
            assert_eq!(t.wrap(n + 5.0), 5.0);
            assert_eq!(t.wrap(-5.0), n - 5.0);
            assert_eq!(t.wrap(3.0 * n + 1.0), 1.0);
            assert_eq!(t.ds(n - 10.0, 10.0), 20.0, "shortest way round, forwards");
            assert_eq!(t.ds(10.0, n - 10.0), -20.0, "shortest way round, backwards");
            // The loop closes: the last sample runs into the first.
            let last = t.n - 1;
            assert!(hypot((t.px[last] - t.px[0]) as f64, (t.pz[last] - t.pz[0]) as f64) < 1.1);
            let (a, b) = (t.frame(n - 0.5), t.frame(-0.5));
            near(a.x, b.x, 1e-3, "frame wraps");
            near(a.z, b.z, 1e-3, "frame wraps");
        } else {
            assert_eq!(t.idx(-5.0), 0);
            assert_eq!(t.idx(1e9), t.n - 1);
            assert_eq!(t.idx(10.4), 10);
            assert_eq!(t.wrap(-5.0), -5.0, "point-to-point: wrap is the identity");
            assert_eq!(t.wrap(n + 50.0), n + 50.0);
            assert_eq!(t.ds(100.0, 40.0), -60.0);
            // Past the last sample the road carries on straight (the runout).
            let (end, past) = (t.frame(t.length), t.frame(t.length + 20.0));
            near(
                hypot(past.x - end.x, past.z - end.z),
                20.0,
                0.05,
                "runout carries on",
            );
        }
    }
}

#[test]
fn racing_line_stays_on_the_tarmac() {
    for (l, t) in common::tracks() {
        for i in 0..t.n {
            assert!(
                (t.racing_line[i] as f64).abs() <= t.hw[i] as f64 - 1.6 + 1e-3,
                "{}: racingLine[{i}] = {} with hw {}",
                l.id,
                t.racing_line[i],
                t.hw[i]
            );
        }
    }
}

#[test]
fn speed_profile_is_positive_capped_and_brakeable() {
    for (l, t) in common::tracks() {
        let v = &t.speed_profile;
        for (i, &x) in v.iter().enumerate() {
            assert!(
                x > 10.0 && x as f64 <= 69.5 + 1e-4,
                "{}: speedProfile[{i}] = {x}",
                l.id
            );
        }
        // No corner needs more than the 11 m/s² the profile assumes to brake for.
        let last = if t.is_loop { t.n } else { t.n - 1 };
        for i in 0..last {
            let j = (i + 1) % t.n;
            let (a, b) = (v[i] as f64, v[j] as f64);
            assert!(
                a * a <= b * b + 2.0 * 11.0 + 1e-2,
                "{}: can't brake from {a} to {b} at {i}",
                l.id
            );
        }
    }
}

/// tools/check-track.js: two stretches of road further apart along the
/// route than 300 m must not come within their widths (+18 m) of each
/// other, unless one passes over the other with real clearance.
#[test]
fn no_two_parts_of_the_road_overlap() {
    for (l, t) in common::tracks() {
        let (step, gap, clearance) = (4, 300, 6.0);
        let mut hits = Vec::new();
        let far = |i: usize, j: usize| {
            let d = if t.is_loop {
                (j - i).min(t.n - (j - i))
            } else {
                j - i
            };
            d >= gap
        };
        let mut i = 0;
        while i < t.n {
            let mut j = i + gap;
            while j < t.n {
                if far(i, j) {
                    let d = hypot((t.px[i] - t.px[j]) as f64, (t.pz[i] - t.pz[j]) as f64);
                    if d < t.hw[i] as f64 + t.hw[j] as f64 + 18.0
                        && ((t.py[j] - t.py[i]) as f64).abs() < clearance
                    {
                        hits.push(format!("s={i} and s={j} are {d:.1} m apart"));
                    }
                }
                j += step;
            }
            i += step;
        }
        assert!(
            hits.is_empty(),
            "{}: {:?}",
            l.id,
            &hits[..hits.len().min(5)]
        );
    }
}

#[test]
fn nearest_and_distance_to_road_find_the_road() {
    for (l, t) in common::tracks() {
        let mut s = 20.0;
        while s < t.length - 20.0 {
            let p = t.point_at(s, 2.0);
            let k = t.nearest(p.x, p.z, 96.0);
            assert!(
                k >= 0 && t.ds(k as f64, s).abs() < 3.0,
                "{}: nearest to s={s} is {k}",
                l.id
            );
            let d = t.distance_to_road(p.x, p.z, 96.0);
            assert!(
                (d.d - 2.0).abs() < 0.2,
                "{}: 2 m off the centreline at {s}: {}",
                l.id,
                d.d
            );
            s += 331.0;
        }
        let b = t.bounds;
        assert_eq!(
            t.nearest(b.max_x + 5000.0, b.max_z + 5000.0, 96.0),
            -1,
            "nothing out in the wilds"
        );
        assert_eq!(
            t.distance_to_road(b.max_x + 5000.0, 0.0, 96.0).d,
            f64::INFINITY
        );
    }
}

#[test]
fn tags_mark_named_features_inside_the_route() {
    for (l, t) in common::tracks() {
        assert!(!t.tags.is_empty(), "{} has tags", l.id);
        for g in &t.tags {
            assert!(g.s1 > g.s0, "{}: {} {}-{}", l.id, g.tag, g.s0, g.s1);
            if !t.is_loop {
                assert!(
                    g.s0 >= 0.0 && g.s1 <= t.length,
                    "{}: {} inside the route",
                    l.id,
                    g.tag
                );
            }
        }
        if !t.is_loop {
            assert_eq!(t.tag("start").len(), 1, "{} has one start", l.id);
        }
    }
}
