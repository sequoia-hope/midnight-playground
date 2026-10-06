//! The `Track` on small synthetic routes (mp_track cannot see the real
//! levels): a straight, turtle corners, a closed square of turtle corners, a
//! circle loop, a surveyed loop and a figure of eight. Covers construction
//! from level data, the s queries (`wrap`, `ds`, `idx`, `locate`), frames and
//! their interpolation, projection onto the centreline (on a sample, between
//! samples, across the lap seam, past the ends, far off, at a crossing),
//! the spatial hash, zones, tags, road types and the per-sample invariants.
//! Semantics are those of `src/track/Track.js`.

use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::sync::Arc;

use mp_math::DEG;
use mp_track::*;

fn zone(key: &'static str) -> Zone {
    Zone {
        key,
        name: key,
        sub: "",
        landform: "hills",
        scenery: "forest",
        color: "#000",
        blend: None,
        blend_offset: None,
    }
}

fn level(route: Route, zones: usize) -> Level {
    const KEYS: [&str; 4] = ["a", "b", "c", "d"];
    Level {
        id: "test",
        mode: Mode::Race,
        num: "0",
        title: "Test",
        desc: "",
        laps: None,
        lap_length: None,
        start_height: None,
        start_heading: None,
        start_x: None,
        start_z: None,
        finish_runoff: None,
        elevation_smooth: None,
        route,
        elevation: None,
        ground: None,
        loose_ground: None,
        sea_y: None,
        zones: KEYS[..zones].iter().map(|k| zone(k)).collect(),
        sky: Vec::new(),
        sun_azimuth: 0.0,
        moon_dir: None,
        traffic_paint: None,
        traffic: Vec::new(),
        police: None,
        rivals: Vec::new(),
    }
}

fn segments(segs: Vec<Segment>) -> Level {
    let zones = segs
        .iter()
        .filter_map(|s| s.extra.zone)
        .max()
        .map_or(1, |z| z as usize + 1);
    level(Route::Segments(segs), zones)
}

fn loop_spec(path: impl Fn() -> Result<LoopPath, String> + Send + Sync + 'static) -> LoopSpec {
    LoopSpec {
        path: Arc::new(path),
        base_y: None,
        tags: Vec::new(),
        road: None,
        roads: Vec::new(),
        start_s: None,
        elevation_smooth: None,
        zones: None,
    }
}

fn loop_level(spec: LoopSpec) -> Level {
    let zones = spec.zones.as_ref().map_or(1, |z| z.len());
    level(Route::Loop(spec), zones)
}

/// A circle of radius `r` as an `m`-gon, counter-clockwise from (r, 0).
fn circle_path(r: f64, m: usize) -> LoopPath {
    let a = |k: usize| k as f64 / m as f64 * TAU;
    LoopPath {
        x: (0..m).map(|k| r * mp_math::kernel::cos(a(k))).collect(),
        z: (0..m).map(|k| r * mp_math::kernel::sin(a(k))).collect(),
        ..LoopPath::default()
    }
}

fn circle(r: f64) -> Track {
    Track::new(&loop_level(loop_spec(move || Ok(circle_path(r, 720))))).unwrap()
}

fn straight(len: f64) -> Track {
    Track::new(&segments(vec![seg(len, 0.0, 0.0)])).unwrap()
}

/// Four identical 90° corners with straights: a closed rounded square, as
/// a point-to-point route that ends where it began.
fn square() -> Track {
    let mut segs = Vec::new();
    for _ in 0..4 {
        segs.push(seg(60.0, 0.0, 0.0));
        segs.push(seg(80.0, 90.0, 0.0));
    }
    Track::new(&segments(segs)).unwrap()
}

/// A lemniscate of Gerono, x = a sin 2θ / 2... scaled so the two lobes
/// cross at the origin at right angles.
fn figure_eight() -> Track {
    let path = || {
        let m = 2000;
        let th = |k: usize| k as f64 / m as f64 * TAU;
        Ok(LoopPath {
            x: (0..m)
                .map(|k| 150.0 * mp_math::kernel::sin(th(k)))
                .collect(),
            z: (0..m)
                .map(|k| 150.0 * mp_math::kernel::sin(th(k)) * mp_math::kernel::cos(th(k)))
                .collect(),
            ..LoopPath::default()
        })
    };
    Track::new(&loop_level(loop_spec(path))).unwrap()
}

fn near(a: f64, b: f64, eps: f64, msg: &str) {
    assert!((a - b).abs() <= eps, "{a} vs {b} (eps {eps}) {msg}");
}

fn dist(t: &Track, a: usize, b: usize) -> f64 {
    let dx = t.px[b] as f64 - t.px[a] as f64;
    let dz = t.pz[b] as f64 - t.pz[a] as f64;
    (dx * dx + dz * dz).sqrt()
}

// ── Construction ────────────────────────────────────────────────────

#[test]
fn a_straight_segment_walks_one_metre_per_sample_from_the_start() {
    let t = straight(200.0);
    assert!(!t.is_loop);
    assert_eq!(t.n, 201);
    assert_eq!(t.length, 200.0);
    for k in 0..t.n {
        assert_eq!(t.px[k], k as f32, "heading 0 runs along +x");
        assert_eq!(t.pz[k], 0.0);
        assert_eq!((t.fx[k], t.fz[k]), (1.0, 0.0));
        assert_eq!(t.kappa[k], 0.0);
    }
    // The right-hand vector is the forward one turned a quarter: +z here.
    assert_eq!((t.rx[0], t.rz[0]), (-0.0, 1.0));
    // A flat road at the default start height.
    for k in 0..t.n {
        near(t.py[k] as f64, 100.0, 1e-4, "default start height");
        near(t.grade[k] as f64, 0.0, 1e-6, "flat");
    }
}

#[test]
fn start_position_and_heading_place_the_route() {
    let mut l = segments(vec![seg(50.0, 0.0, 0.0)]);
    l.start_x = Some(10.0);
    l.start_z = Some(-20.0);
    l.start_heading = Some(90.0);
    l.start_height = Some(7.0);
    let t = Track::new(&l).unwrap();
    assert_eq!((t.px[0], t.pz[0]), (10.0, -20.0));
    near(t.px[50] as f64, 10.0, 1e-4, "heading 90° runs along +z");
    near(t.pz[50] as f64, 30.0, 1e-4, "");
    near(t.py[0] as f64, 7.0, 1e-4, "start height");
    near(t.fz[25] as f64, 1.0, 1e-6, "");
}

#[test]
fn a_climb_spreads_its_rise_over_the_segment() {
    let t = Track::new(&segments(vec![seg(400.0, 0.0, 40.0)])).unwrap();
    // y climbs 0.1 per metre; smoothing only touches the clamped ends.
    near(t.py[200] as f64, 120.0, 1e-3, "halfway up");
    near(t.grade[200] as f64, 0.1, 1e-4, "grade");
    for k in 1..t.n {
        assert!(t.py[k] >= t.py[k - 1], "climbs monotonically at {k}");
    }
}

#[test]
fn a_level_elevation_function_owns_the_road_height() {
    let mut l = segments(vec![seg(300.0, 0.0, 50.0)]);
    l.elevation = Some(Arc::new(|x, _z| 3.0 + x * 0.01));
    let t = Track::new(&l).unwrap();
    // The segment's rise is ignored: y follows the ground.
    near(t.py[150] as f64, 4.5, 1e-3, "on the ground");
}

#[test]
fn a_turtle_corner_turns_the_heading_by_its_angle() {
    let t = Track::new(&segments(vec![
        seg(50.0, 0.0, 0.0),
        seg(100.0, 90.0, 0.0),
        seg(50.0, 0.0, 0.0),
    ]))
    .unwrap();
    let turned: f64 = t.kappa.iter().map(|&k| k as f64).sum();
    near(turned, FRAC_PI_2, 1e-5, "curvature integrates to the turn");
    // Positive turns go from +x towards +z.
    near(t.fx[190] as f64, 0.0, 1e-5, "heading after the corner");
    near(t.fz[190] as f64, 1.0, 1e-5, "");
    // Curvature is zero on the straights and peaks mid-corner.
    assert_eq!(t.kappa[20], 0.0);
    assert!(t.kappa[100] > t.kappa[55] && t.kappa[100] > t.kappa[145]);
    // Consecutive samples stay a metre apart through the corner.
    for k in 1..t.n {
        near(dist(&t, k - 1, k), 1.0, 2e-4, "spacing");
    }
}

#[test]
fn four_quarter_turns_close_the_route() {
    let t = square();
    assert_eq!(t.n, 561);
    near(
        t.px[t.n - 1] as f64,
        t.px[0] as f64,
        5e-3,
        "ends where it began",
    );
    near(t.pz[t.n - 1] as f64, t.pz[0] as f64, 5e-3, "");
    // Symmetric: the four corners sit at the same distance from the centre.
    let cx = (t.bounds.min_x + t.bounds.max_x) / 2.0;
    let cz = (t.bounds.min_z + t.bounds.max_z) / 2.0;
    near(
        t.bounds.max_x - t.bounds.min_x,
        t.bounds.max_z - t.bounds.min_z,
        1e-3,
        "square",
    );
    let r = |k: usize| (t.px[k] as f64 - cx).hypot(t.pz[k] as f64 - cz);
    for c in 0..4 {
        near(r(100 + 140 * c), r(100), 1e-3, "corner apexes");
    }
}

#[test]
fn a_circle_loop_resamples_to_whole_metres() {
    let t = circle(100.0);
    assert!(t.is_loop);
    let perimeter = 720.0 * 200.0 * (PI / 720.0).sin();
    assert_eq!(t.n, perimeter.round() as usize);
    assert_eq!(t.length, t.n as f64, "a loop's length is its sample count");
    let step = perimeter / t.n as f64;
    for k in 0..t.n {
        // Including the seam from the last sample back to the first.
        near(
            dist(&t, k, (k + 1) % t.n),
            step,
            1e-3,
            &format!("spacing at {k}"),
        );
        let r = (t.px[k] as f64).hypot(t.pz[k] as f64);
        near(r, 100.0, 0.01, "on the circle");
    }
    assert_eq!(
        (t.px[0], t.pz[0]),
        (100.0, 0.0),
        "starts at the first vertex"
    );
}

#[test]
fn a_loop_gets_its_curvature_from_the_geometry() {
    let t = circle(100.0);
    let step = t.length.recip() * 200.0 * PI;
    // Per sample it is lumpy (the 720-gon's corners every 0.87 m), but it
    // sums to one full turn and smooths to 1/r.
    let turned: f64 = t.kappa.iter().map(|&k| k as f64).sum();
    near(turned, TAU, 1e-4, "one full turn per lap");
    for k in 0..t.n {
        near(t.kappa[k] as f64, step / 100.0, 1e-3, "κ ≈ 1/r per sample");
        near(t.k_smooth[k] as f64, step / 100.0, 2e-5, "smoothed κ = 1/r");
    }
    // Counter-clockwise: forward is the radius turned +90°.
    near(t.fx[0] as f64, 0.0, 1e-3, "");
    near(t.fz[0] as f64, 1.0, 1e-3, "");
    // The right-hand vector points out of a left-hand bend.
    assert!(t.rx[0] < -0.99, "right of a CCW circle is the centre side");
}

#[test]
fn forward_vectors_are_unit_length_and_right_is_perpendicular() {
    for t in [straight(100.0), square(), circle(80.0), figure_eight()] {
        for k in 0..t.n {
            let (fx, fz) = (t.fx[k] as f64, t.fz[k] as f64);
            near(fx.hypot(fz), 1.0, 1e-6, "unit forward");
            assert_eq!(t.rx[k], -t.fz[k]);
            assert_eq!(t.rz[k], t.fx[k]);
        }
    }
}

#[test]
fn per_sample_arrays_have_one_entry_per_sample_and_are_finite() {
    for t in [straight(100.0), square(), circle(80.0), figure_eight()] {
        let n = t.n;
        for a in [
            &t.px,
            &t.py,
            &t.pz,
            &t.kappa,
            &t.k_smooth,
            &t.grade,
            &t.fx,
            &t.fz,
            &t.rx,
            &t.rz,
            &t.hw,
            &t.margin,
            &t.bank,
            &t.wall_l,
            &t.wall_r,
            &t.racing_line,
            &t.speed_profile,
        ] {
            assert_eq!(a.len(), n);
            assert!(a.iter().all(|v| v.is_finite()));
        }
        assert_eq!(t.zone.len(), n);
        assert_eq!(t.road_type.len(), n);
        assert_eq!(t.elevated.len(), n);
    }
}

#[test]
fn defaults_for_finish_start_laps_and_road_end() {
    let t = straight(500.0);
    assert_eq!(t.finish_s, 320.0, "180 m of run-off by default");
    assert_eq!(t.start_s, 60.0);
    assert_eq!(t.laps, 0);
    assert_eq!(t.road_end(), 500.0);
    let mut l = segments(vec![seg(500.0, 0.0, 0.0)]);
    l.finish_runoff = Some(20.0);
    l.laps = Some(3);
    let t = Track::new(&l).unwrap();
    assert_eq!(t.finish_s, 480.0);
    assert_eq!(t.laps, 0, "only a loop has laps");

    let mut spec = loop_spec(|| Ok(circle_path(100.0, 360)));
    let c = Track::new(&loop_level(spec.clone())).unwrap();
    assert_eq!(c.finish_s, f64::INFINITY);
    assert_eq!(c.start_s, 120.0);
    assert_eq!(c.road_end(), f64::INFINITY);
    spec.start_s = Some(33.0);
    let mut l = loop_level(spec);
    l.laps = Some(3);
    let c = Track::new(&l).unwrap();
    assert_eq!((c.start_s, c.laps), (33.0, 3));
}

#[test]
fn a_failing_loop_path_is_an_error() {
    let l = loop_level(loop_spec(|| Err("survey not loaded".to_string())));
    assert_eq!(Track::new(&l).err().as_deref(), Some("survey not loaded"));
}

#[test]
fn a_fractional_segment_length_builds() {
    // round(10.4) = 10 gives 11 samples, but the walk takes 11 one-metre
    // steps (k = 0..=10 < 10.4) and would write px[11]. Float32Array
    // ignores the out-of-range store, and so does the port. No shipped
    // level has a fractional length.
    let t = Track::new(&segments(vec![seg(10.4, 0.0, 0.0)])).unwrap();
    assert_eq!(t.n, 11);
}

// ── Road types, zones and tags ──────────────────────────────────────

#[test]
fn segments_carry_their_road_type_forward() {
    let t = Track::new(&segments(vec![
        seg(300.0, 0.0, 0.0),
        seg(300.0, 0.0, 0.0).road("freeway"),
        seg(300.0, 0.0, 0.0),
    ]))
    .unwrap();
    let mountain = road_index("mountain") as u8;
    let freeway = road_index("freeway") as u8;
    assert_eq!(
        t.road_type[0], mountain,
        "the first segment defaults to mountain"
    );
    assert_eq!(t.road_type[299], mountain);
    assert_eq!(t.road_type[300], freeway);
    assert_eq!(t.road_type[899], freeway, "carried on to the next segment");
    assert_eq!(
        t.road_type[900], freeway,
        "the last sample copies the one before"
    );
    // Widths blend between the types (smoothed over ~40 m).
    near(t.hw[100] as f64, 5.4, 1e-4, "mountain width");
    near(t.hw[600] as f64, 9.4, 1e-4, "freeway width");
    assert!(t.hw[300] > 5.4 && t.hw[300] < 9.4);
    // Open roads: the collision limit is the width plus the type's margin.
    near(t.wall_l[600] as f64, 9.4 + 0.5, 1e-4, "");
    assert_eq!(t.wall_l, t.wall_r);
}

#[test]
fn zones_start_where_their_segment_starts_and_blend_across_the_boundary() {
    let t = Track::new(&segments(vec![
        seg(1000.0, 0.0, 0.0).zone(0),
        seg(1000.0, 0.0, 0.0).zone(1),
    ]))
    .unwrap();
    assert_eq!(t.zone_start, vec![0, 1000]);
    assert_eq!(t.zones.len(), 2);
    assert_eq!((t.zones[0].s0, t.zones[0].s1), (0.0, 1000.0));
    assert_eq!((t.zones[1].s0, t.zones[1].s1), (1000.0, t.length));
    assert_eq!(t.zones[1].id, 1);
    assert_eq!(t.frame(999.5).zone, 0);
    assert_eq!(t.frame(1000.0).zone, 1);
    for s in [0.0, 500.0, 800.0, 900.0, 1000.0, 1100.0, 1250.0, 1600.0] {
        let w = t.zone_blend(s, 250.0);
        assert_eq!(w.len(), 2);
        near(w.iter().sum::<f64>(), 1.0, 1e-12, "weights sum to 1");
        assert!(w.iter().all(|&x| (0.0..=1.0).contains(&x)));
    }
    assert_eq!(t.zone_blend(500.0, 250.0), vec![1.0, 0.0]);
    assert_eq!(t.zone_blend(1000.0, 250.0), vec![0.5, 0.5]);
    assert_eq!(t.zone_blend(1600.0, 250.0), vec![0.0, 1.0]);
    // One zone: all weight on it.
    assert_eq!(straight(100.0).zone_blend(50.0, 250.0), vec![1.0]);
}

#[test]
fn loop_zones_start_at_their_metres() {
    let mut spec = loop_spec(|| Ok(circle_path(100.0, 360)));
    spec.zones = Some(vec![0.0, 200.0, 400.0]);
    let t = Track::new(&loop_level(spec)).unwrap();
    assert_eq!(t.zone_start, vec![0, 200, 400]);
    assert_eq!((t.zone[199], t.zone[200], t.zone[t.n - 1]), (0, 1, 2));
    assert_eq!(t.zones[2].s1, t.length);
}

#[test]
fn segment_tags_mark_their_segment() {
    let t = Track::new(&segments(vec![
        seg(100.0, 0.0, 0.0).tag("start"),
        seg(150.0, -40.0, 0.0).tag("bend"),
        seg(100.0, 0.0, 0.0).tag("bend"),
    ]))
    .unwrap();
    let start = t.tag("start");
    assert_eq!(start.len(), 1);
    assert_eq!(
        (start[0].s0, start[0].s1, start[0].turn),
        (0.0, 100.0, Some(0.0))
    );
    let bends = t.tag("bend");
    assert_eq!(bends.len(), 2, "every match, in route order");
    assert_eq!(
        (bends[0].s0, bends[0].s1, bends[0].turn),
        (100.0, 250.0, Some(-40.0))
    );
    assert_eq!(bends[1].s0, 250.0);
    assert!(t.tag("missing").is_empty());
}

#[test]
fn loop_tags_by_fraction_and_metres_and_elevated_ramps() {
    let mut spec = loop_spec(|| Ok(circle_path(300.0, 720)));
    spec.base_y = Some(5.0);
    spec.tags = vec![
        LoopTag {
            tag: "pier",
            s0: None,
            s1: None,
            f0: Some(0.25),
            f1: Some(0.5),
            elevated: None,
        },
        LoopTag {
            tag: "bridge",
            s0: Some(1100.0),
            s1: Some(1300.0),
            f0: None,
            f1: None,
            elevated: Some(8.0),
        },
    ];
    let t = Track::new(&loop_level(spec)).unwrap();
    let n = t.n as f64;
    let pier = t.tag("pier");
    assert_eq!(
        (pier[0].s0, pier[0].s1),
        ((n * 0.25).round(), (n * 0.5).round())
    );
    let up = t.tag("bridge-up");
    let down = t.tag("bridge-down");
    assert_eq!((up[0].s0, up[0].s1, up[0].turn), (840.0, 1100.0, None));
    assert_eq!((down[0].s0, down[0].s1), (1300.0, 1560.0));
    // The road rises the full 8 m over the bridge and is flagged elevated;
    // away from it it stays at base_y.
    near(t.py[1200] as f64, 13.0, 1e-3, "on the bridge");
    assert_eq!(t.elevated[1200], 1);
    near(t.py[100] as f64, 5.0, 1e-4, "base height");
    assert_eq!(t.elevated[100], 0);
    assert!(
        t.py[970] > t.py[900] && t.py[1030] > t.py[970],
        "ramping up"
    );
}

#[test]
fn loop_roads_override_the_default_type_and_wrap_negative_metres() {
    let mut spec = loop_spec(|| Ok(circle_path(100.0, 360)));
    spec.road = Some("valley");
    spec.roads = vec![LoopRoad {
        s0: -10,
        s1: 10,
        road: "street",
    }];
    let t = Track::new(&loop_level(spec)).unwrap();
    let street = road_index("street") as u8;
    assert_eq!(t.road_type[300], road_index("valley") as u8);
    assert_eq!(t.road_type[0], street);
    assert_eq!(t.road_type[9], street);
    assert_eq!(t.road_type[t.n - 10], street, "-10 wraps to n - 10");
    assert_ne!(t.road_type[t.n - 11], street);
    // Without a type, a loop is a freeway.
    assert_eq!(circle(100.0).road_type[0], road_index("freeway") as u8);
}

#[test]
fn a_street_stays_level_across_in_corners() {
    let t = Track::new(&segments(vec![
        seg(100.0, 0.0, 0.0).road("street"),
        seg(80.0, 90.0, 0.0),
        seg(100.0, 0.0, 0.0),
    ]))
    .unwrap();
    assert!(t.bank.iter().all(|&b| b == 0.0), "street bank is 0");
    let m = Track::new(&segments(vec![
        seg(100.0, 0.0, 0.0),
        seg(80.0, 90.0, 0.0),
        seg(100.0, 0.0, 0.0),
    ]))
    .unwrap();
    let peak = m.bank.iter().fold(0f32, |a, &b| a.max(b.abs()));
    assert!(
        peak > 0.05 && peak <= 0.085,
        "mountain roads bank, capped: {peak}"
    );
}

// ── Surveyed loops ──────────────────────────────────────────────────

fn surveyed() -> Track {
    let path = || {
        let mut p = circle_path(200.0, 720);
        let m = p.x.len();
        p.y = Some(vec![2.0; m]);
        p.bank = Some(vec![0.0; m]);
        p.hw = Some(vec![6.0; m]);
        p.wall_l = Some(vec![3.0; m]); // closer than the tarmac edge allows
        p.wall_r = Some(vec![20.0; m]);
        p.run_l = Some(vec![-0.1; m]);
        p.run_r = Some(vec![0.2; m]);
        Ok(p)
    };
    Track::new(&loop_level(loop_spec(path))).unwrap()
}

#[test]
fn a_surveyed_loop_brings_its_own_width_heights_and_walls() {
    let t = surveyed();
    for k in (0..t.n).step_by(37) {
        near(t.hw[k] as f64, 6.0, 1e-5, "surveyed width");
        near(t.py[k] as f64, 2.0, 1e-5, "surveyed height");
        assert_eq!(t.bank[k], 0.0);
        near(t.wall_l[k] as f64, 7.5, 1e-5, "never closer than hw + 1.5");
        near(t.wall_r[k] as f64, 20.0, 1e-5, "surveyed wall");
    }
    assert!(t.run_l.is_some() && t.run_r.is_some());
}

#[test]
fn surface_y_carries_the_road_plane_over_the_verge_then_the_run_off_grade() {
    let t = surveyed();
    let s = 300.25;
    let flat = 6.0 + RUNOFF_FLAT;
    near(t.surface_y(s, 0.0), 2.0, 1e-5, "centre");
    near(t.surface_y(s, flat), 2.0, 1e-5, "edge of the verge");
    near(
        t.surface_y(s, flat + 10.0),
        2.0 + 10.0 * 0.2,
        1e-4,
        "right run-off rises",
    );
    near(
        t.surface_y(s, -flat - 10.0),
        2.0 - 10.0 * 0.1,
        1e-4,
        "left run-off falls",
    );
    // point_at uses the same surface off the tarmac.
    near(t.point_at(s, flat + 5.0).y, 3.0, 1e-4, "");
    near(t.point_at(s, 3.0).y, 2.0, 1e-5, "on the tarmac");
}

#[test]
fn off_a_surveyed_road_the_banked_plane_carries_on() {
    // No run-off data: y - lat * bank everywhere.
    let mut l = segments(vec![
        seg(100.0, 0.0, 0.0),
        seg(80.0, 90.0, 0.0),
        seg(100.0, 0.0, 0.0),
    ]);
    l.start_height = Some(0.0);
    let t = Track::new(&l).unwrap();
    let f = t.frame(140.0);
    assert!(f.bank > 0.0);
    for lat in [-30.0, -2.0, 0.0, 4.0, 30.0] {
        near(t.surface_y(140.0, lat), f.y - lat * f.bank, 1e-9, "plane");
        near(t.point_at(140.0, lat).y, f.y - lat * f.bank, 1e-9, "plane");
    }
}

// ── s queries ───────────────────────────────────────────────────────

#[test]
fn wrap_maps_any_s_into_the_lap_and_leaves_open_roads_alone() {
    let t = circle(100.0);
    let n = t.n as f64;
    assert_eq!(t.wrap(0.0), 0.0);
    assert_eq!(t.wrap(n), 0.0);
    assert_eq!(t.wrap(-1.0), n - 1.0);
    assert_eq!(t.wrap(n + 2.5), 2.5);
    assert_eq!(t.wrap(-0.0), 0.0);
    for k in -5..5 {
        for s in [0.25, 13.0, 300.75] {
            let w = t.wrap(s + k as f64 * n);
            assert!((0.0..n).contains(&w));
            near(w, s, 1e-9, &format!("{s} + {k} laps"));
        }
    }
    let o = straight(100.0);
    for s in [-5.0, 0.0, 50.5, 150.0] {
        assert_eq!(o.wrap(s), s);
    }
}

#[test]
fn ds_is_the_signed_short_way_round() {
    let t = circle(100.0);
    let n = t.n as f64;
    assert_eq!(t.ds(n - 2.0, 1.0), 3.0, "forward across the seam");
    assert_eq!(t.ds(1.0, n - 2.0), -3.0, "backward across the seam");
    assert_eq!(t.ds(10.0, 10.0 + n * 3.0), 0.0, "whole laps vanish");
    for (a, b) in [(0.0, 100.0), (5.5, 400.0), (600.0, 20.0), (300.0, 2.0)] {
        let d = t.ds(a, b);
        assert!(d.abs() <= n / 2.0);
        near(t.wrap(a + d), t.wrap(b), 1e-9, "a + ds(a, b) lands on b");
        if d.abs() < n / 2.0 {
            near(t.ds(b, a), -d, 1e-9, "antisymmetric");
        }
    }
    // Exactly half a lap is reported forward.
    assert_eq!(t.ds(0.0, n / 2.0), n / 2.0);
    let o = straight(100.0);
    assert_eq!(o.ds(90.0, 10.0), -80.0);
}

#[test]
fn idx_rounds_like_javascript_and_wraps_or_clamps() {
    let t = circle(100.0);
    let n = t.n;
    assert_eq!(t.idx(2.5), 3, "halves round up");
    assert_eq!(t.idx(2.4999), 2);
    assert_eq!(t.idx(n as f64 - 0.4), 0, "rounds onto the seam");
    assert_eq!(t.idx(-0.6), n - 1);
    assert_eq!(t.idx(-0.5), 0, "-0.5 rounds to -0, which is sample 0");
    assert_eq!(t.idx(3.0 * n as f64 + 7.0), 7);
    let o = straight(100.0);
    assert_eq!(o.idx(-20.0), 0);
    assert_eq!(o.idx(-0.5), 0);
    assert_eq!(o.idx(99.5), 100);
    assert_eq!(o.idx(1e9), 100, "clamped to the last sample");
}

#[test]
fn locate_splits_s_into_sample_and_fraction() {
    let t = circle(100.0);
    let n = t.n;
    assert_eq!(t.locate(10.25), (10, 0.25, 11, 10.25));
    let (i, f, j, s) = t.locate(n as f64 - 0.5);
    assert_eq!(
        (i, j),
        (n - 1, 0),
        "the last sample interpolates to the first"
    );
    near(f, 0.5, 1e-12, "");
    near(s, n as f64 - 0.5, 1e-12, "");
    let (i, f, j, _) = t.locate(-0.25);
    assert_eq!((i, j), (n - 1, 0));
    near(f, 0.75, 1e-12, "");
    for s in [0.0, 0.999, 77.5, 400.1, n as f64 * 2.0 + 3.3] {
        let (i, f, j, _) = t.locate(s);
        assert!(i < n && j == (i + 1) % n);
        assert!((0.0..1.0).contains(&f));
    }
    let o = straight(100.0);
    assert_eq!(o.locate(-5.0), (0, 0.0, 1, 0.0), "clamped to the start");
    let (i, f, j, s) = o.locate(500.0);
    assert_eq!((i, j), (99, 100), "never past the last pair");
    near(f, 0.999, 1e-9, "");
    near(s, 99.999, 1e-9, "");
}

// ── Frames ──────────────────────────────────────────────────────────

#[test]
fn frame_on_a_sample_is_that_sample() {
    let t = square();
    for k in [0, 1, 77, 140, 300, 559] {
        let f = t.frame(k as f64);
        assert_eq!(f.x, t.px[k] as f64);
        assert_eq!(f.y, t.py[k] as f64);
        assert_eq!(f.z, t.pz[k] as f64);
        assert_eq!(f.hw, t.hw[k] as f64);
        assert_eq!(f.zone, t.zone[k]);
        assert_eq!(f.s, k as f64);
        near(f.fx, t.fx[k] as f64, 1e-7, "");
        near(f.fz, t.fz[k] as f64, 1e-7, "");
    }
}

#[test]
fn frame_interpolates_linearly_and_renormalises_the_tangent() {
    let t = square();
    for s in [0.5, 100.25, 210.75, 333.5] {
        let (i, f, j, _) = t.locate(s);
        let fr = t.frame(s);
        let l = |a: &[f32]| a[i] as f64 + (a[j] as f64 - a[i] as f64) * f;
        near(fr.x, l(&t.px), 1e-9, "x");
        near(fr.z, l(&t.pz), 1e-9, "z");
        near(fr.bank, l(&t.bank), 1e-9, "bank");
        near(fr.kappa, l(&t.k_smooth), 1e-9, "kappa");
        near(
            fr.fx.hypot(fr.fz),
            1.0,
            1e-12,
            "unit tangent between samples",
        );
        assert_eq!((fr.rx, fr.rz), (-fr.fz, fr.fx));
    }
}

#[test]
fn frame_is_continuous_across_a_loops_seam() {
    let t = circle(100.0);
    let n = t.n as f64;
    let a = t.frame(n - 1e-6);
    let b = t.frame(0.0);
    let c = t.frame(n);
    near(a.x, b.x, 1e-4, "");
    near(a.z, b.z, 1e-4, "");
    near(a.fx, b.fx, 1e-4, "");
    assert_eq!(b, c, "s = n is s = 0");
    assert_eq!(t.frame(-1.0), t.frame(n - 1.0));
}

#[test]
fn past_the_end_of_an_open_road_the_frame_runs_straight_on() {
    let t = straight(100.0);
    let f = t.frame(130.0);
    near(f.x, 130.0, 1e-6, "extrapolated along the last tangent");
    near(f.z, 0.0, 1e-9, "");
    assert_eq!(f.s, 130.0, "and keeps the asked-for s");
    // Before the start it clamps instead.
    let b = t.frame(-30.0);
    assert_eq!((b.x, b.s), (0.0, 0.0));
    // A loop never extrapolates.
    let c = circle(100.0);
    assert!(c.frame(c.n as f64 + 30.0).s < c.n as f64);
}

#[test]
fn point_at_offsets_along_the_right_hand_vector() {
    let t = circle(100.0);
    for s in [0.0, 100.5, 333.3] {
        let f = t.frame(s);
        let p = t.point_at(s, 4.0);
        near(p.x, f.x + f.rx * 4.0, 1e-12, "");
        near(p.z, f.z + f.rz * 4.0, 1e-12, "");
        // Right of a CCW circle is inward.
        let r = p.x.hypot(p.z);
        near(r, 96.0, 0.02, "lat 4 is 4 m toward the centre");
    }
}

// ── Projection ──────────────────────────────────────────────────────

#[test]
fn project_on_a_sample_returns_it_exactly() {
    for t in [square(), circle(100.0)] {
        for k in [0, 1, 50, 139, 140, 401, t.n - 1] {
            let p = t.project(t.px[k] as f64, t.pz[k] as f64, k as f64);
            assert_eq!(p.i, k);
            assert_eq!(p.s, k as f64);
            assert_eq!(p.lat, 0.0);
        }
    }
}

#[test]
fn project_inverts_point_at_on_curves() {
    for t in [square(), circle(100.0), figure_eight()] {
        let mut s = 3.3;
        while s < t.length - 3.0 {
            for lat in [-5.0, -1.5, 0.0, 2.0, 5.0] {
                let p = t.point_at(s, lat);
                let q = t.project(p.x, p.z, s + 7.0);
                near(t.ds(s, q.s), 0.0, 0.08, &format!("s at ({s}, {lat})"));
                near(q.lat, lat, 0.05, &format!("lat at ({s}, {lat})"));
                assert!(
                    t.ds(q.i as f64, q.s).abs() <= 1.0,
                    "i is the sample s came from"
                );
            }
            s += 17.7;
        }
    }
}

#[test]
fn project_across_a_loops_seam_wraps_s() {
    let t = circle(100.0);
    let n = t.n as f64;
    for (s, hint) in [
        (0.3, n - 1.0),
        (n - 0.3, 0.0),
        (0.0, n - 5.0),
        (n - 2.0, 3.0),
    ] {
        let p = t.point_at(s, 1.0);
        let q = t.project(p.x, p.z, hint);
        assert!((0.0..n).contains(&q.s), "s {} stays in the lap", q.s);
        near(t.ds(s, q.s), 0.0, 0.05, &format!("s {s} hint {hint}"));
        near(q.lat, 1.0, 0.02, "");
    }
}

#[test]
fn project_widens_its_search_when_the_hint_is_far_off() {
    let t = circle(100.0);
    let n = t.n as f64;
    for (s, hint) in [(10.0, 60.0), (10.0, 200.0), (300.0, 300.0 + n / 2.0 - 1.0)] {
        let p = t.point_at(s, -2.0);
        let q = t.project(p.x, p.z, hint);
        near(t.ds(s, q.s), 0.0, 0.05, &format!("s {s} from hint {hint}"));
    }
    let o = square();
    let p = o.point_at(500.0, 1.0);
    let q = o.project(p.x, p.z, 380.0);
    near(q.s, 500.0, 0.05, "open road, hint 120 m behind");
}

#[test]
fn project_past_the_ends_of_an_open_road_clamps_to_the_road_and_its_runout() {
    let mut t = straight(100.0);
    let q = t.project(130.0, 2.0, 100.0);
    assert_eq!(
        (q.s, q.i),
        (100.0, 100),
        "no runout: clamped to the last sample"
    );
    near(q.lat, 2.0, 1e-9, "lateral offset still measured");
    t.runout = 50.0;
    let q = t.project(130.0, 2.0, 100.0);
    near(q.s, 130.0, 1e-9, "the runout extends the road");
    let q = t.project(200.0, 0.0, 100.0);
    assert_eq!(q.s, 150.0, "but not beyond it");
    let q = t.project(-20.0, -3.0, 0.0);
    assert_eq!(q.s, 0.0, "clamped at the start");
    near(q.lat, -3.0, 1e-9, "");
}

#[test]
fn project_far_off_the_road_still_gives_a_finite_answer() {
    let t = straight(400.0);
    let q = t.project(200.0, 5000.0, 200.0);
    near(q.s, 200.0, 1e-9, "");
    near(q.lat, 5000.0, 1e-9, "lat is right of the road (+z)");
    let q = t.project(200.0, -5000.0, 10.0);
    near(q.s, 200.0, 1e-9, "found from a distant hint");
    near(q.lat, -5000.0, 1e-9, "");
    // From the far side of a circle's centre the nearest sample is found.
    let c = circle(100.0);
    let q = c.project(0.0, -1000.0, 0.0);
    near(
        c.ds(q.s, c.n as f64 * 0.75),
        0.0,
        1.0,
        "the bottom of the circle",
    );
    assert!(q.lat < -890.0, "outside a CCW circle is left");
}

#[test]
fn project_at_a_crossing_stays_on_the_hinted_branch() {
    let t = figure_eight();
    let n = t.n as f64;
    // The lemniscate passes the origin at θ = 0 and θ = π: s 0 and n / 2.
    for branch in [0.0, n / 2.0] {
        let q = t.project(0.0, 0.0, branch + 3.0);
        near(t.ds(branch, q.s), 0.0, 0.5, &format!("branch at {branch}"));
        near(q.lat, 0.0, 0.01, "");
    }
}

#[test]
fn projection_progress_increases_along_the_road() {
    let t = square();
    let mut prev = -1.0;
    let mut hint = 0.0;
    let mut s = 0.0;
    while s <= t.length {
        let p = t.point_at(s, 2.5);
        let q = t.project(p.x, p.z, hint);
        assert!(q.s > prev, "s {s}: {} after {prev}", q.s);
        prev = q.s;
        hint = q.s;
        s += 0.7;
    }
}

// ── Spatial hash, nearest and distance to road ──────────────────────

#[test]
fn the_spatial_hash_holds_every_other_sample_once_and_bounds_hold_all() {
    let t = figure_eight();
    let mut seen = vec![0u8; t.n];
    for (&(cx, cz), ks) in &t.hash {
        for &k in ks {
            assert_eq!(k % 2, 0, "even samples only");
            seen[k] += 1;
            assert_eq!((t.px[k] as f64 / 32.0).floor() as i64, cx);
            assert_eq!((t.pz[k] as f64 / 32.0).floor() as i64, cz);
        }
        assert!(ks.windows(2).all(|w| w[0] < w[1]), "insertion order");
    }
    for (k, &count) in seen.iter().enumerate() {
        assert_eq!(count, u8::from(k % 2 == 0), "sample {k}");
        let (x, z) = (t.px[k] as f64, t.pz[k] as f64);
        assert!(x >= t.bounds.min_x && x <= t.bounds.max_x);
        assert!(z >= t.bounds.min_z && z <= t.bounds.max_z);
    }
    near(t.bounds.max_x, 150.0, 0.1, "");
    near(t.bounds.min_x, -150.0, 0.1, "");
}

#[test]
fn nearest_finds_the_closest_hashed_sample_within_the_radius() {
    let t = circle(100.0);
    // Brute force over the hashed (even) samples.
    // (Off the axes: a point on one has two equidistant samples.)
    for (x, z) in [(100.0, 0.0), (3.0, 97.0), (-60.0, -75.0), (150.0, 10.0)] {
        let k = t.nearest(x, z, 96.0);
        let mut best = (f64::INFINITY, -1i64);
        for j in (0..t.n).step_by(2) {
            let d = (x - t.px[j] as f64).powi(2) + (z - t.pz[j] as f64).powi(2);
            if d < best.0 && d < 96.0 * 96.0 {
                best = (d, j as i64);
            }
        }
        assert_eq!(k, best.1, "nearest to ({x}, {z})");
    }
    assert_eq!(
        t.nearest(0.0, 0.0, 96.0),
        -1,
        "the centre is 100 m from the road"
    );
    assert!(
        t.nearest(0.0, 0.0, 101.0) >= 0,
        "a bigger radius reaches it"
    );
    assert_eq!(t.nearest(5000.0, 5000.0, 96.0), -1);
}

#[test]
fn distance_to_road_measures_the_lateral_offset() {
    let t = straight(300.0);
    let d = t.distance_to_road(151.0, 7.0, 96.0);
    near(d.d, 7.0, 1e-9, "");
    near(d.lat, 7.0, 1e-9, "");
    near(d.s.unwrap(), 151.0, 1e-9, "");
    assert_eq!(
        d.i, 151,
        "refined from the hashed sample to the nearest one"
    );
    let d = t.distance_to_road(151.0, -40.0, 96.0);
    near(d.d, 40.0, 1e-9, "");
    assert!(d.lat < 0.0);
    let far = t.distance_to_road(150.0, 500.0, 96.0);
    assert_eq!(
        far,
        RoadDistance {
            d: f64::INFINITY,
            i: -1,
            lat: 0.0,
            s: None
        }
    );
}

// ── AI helpers ──────────────────────────────────────────────────────

#[test]
fn the_racing_line_cuts_to_the_inside_and_stays_on_the_tarmac() {
    let t = Track::new(&segments(vec![
        seg(300.0, 0.0, 0.0),
        seg(150.0, 90.0, 0.0),
        seg(300.0, 0.0, 0.0),
    ]))
    .unwrap();
    for k in 0..t.n {
        assert!((t.racing_line[k].abs() as f64) <= t.hw[k] as f64 - 1.6 + 1e-6);
    }
    // A left-to-right (positive) turn: the inside is +lat.
    assert!(t.racing_line[375] > 1.0, "apex {}", t.racing_line[375]);
    near(
        t.racing_line[50] as f64,
        0.0,
        1e-3,
        "centred on the long straight",
    );
}

#[test]
fn the_speed_profile_brakes_before_corners_and_caps_at_top_speed() {
    let t = Track::new(&segments(vec![
        seg(400.0, 0.0, 0.0),
        seg(40.0, 120.0, 0.0),
        seg(400.0, 0.0, 0.0),
    ]))
    .unwrap();
    let v = &t.speed_profile;
    assert!(v.iter().all(|&x| x > 0.0 && x <= 69.5));
    assert_eq!(v[100], 69.5f32, "flat out on the straight");
    let corner = v[420];
    assert!(corner < 30.0, "slow in the hairpin: {corner}");
    for i in 0..t.n - 1 {
        let (a, b) = (v[i] as f64, v[i + 1] as f64);
        assert!(a * a <= b * b + 22.0 + 1e-3, "brakeable at {i}: {a} -> {b}");
    }
    // A loop is brakeable across its seam too.
    let c = circle(60.0);
    let w = &c.speed_profile;
    let (a, b) = (w[c.n - 1] as f64, w[0] as f64);
    assert!(a * a <= b * b + 22.0 + 1e-3);
}

// ── trapezoid ───────────────────────────────────────────────────────

#[test]
fn trapezoid_ramps_symmetrically_and_integrates_to_one_minus_r() {
    for r in [0.22, 0.32] {
        assert_eq!(trapezoid(0.0, r), 0.0);
        assert_eq!(trapezoid(1.0, r), 0.0);
        assert_eq!(trapezoid(0.5, r), 1.0);
        let steps = 100_000;
        let mut sum = 0.0;
        for k in 0..steps {
            let t = (k as f64 + 0.5) / steps as f64;
            let v = trapezoid(t, r);
            assert!((0.0..=1.0).contains(&v));
            near(v, trapezoid(1.0 - t, r), 1e-9, "symmetric");
            sum += v;
        }
        near(sum / steps as f64, 1.0 - r, 1e-6, "integral");
    }
}

#[test]
fn a_full_turn_segment_integrates_to_a_full_turn() {
    // Above 120° the ramps are shorter (r = 0.22), and the peak curvature
    // rises to keep the integral at the turn.
    let t = Track::new(&segments(vec![seg(400.0, 360.0, 0.0)])).unwrap();
    let turned: f64 = t.kappa.iter().map(|&k| k as f64).sum();
    near(turned, 360.0 * DEG, 1e-4, "");
    let peak = t.kappa.iter().fold(0f32, |a, &k| a.max(k)) as f64;
    near(peak, 360.0 * DEG / (400.0 * 0.78), 1e-6, "k_peak");
}
