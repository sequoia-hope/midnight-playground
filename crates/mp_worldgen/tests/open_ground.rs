//! An open level's terrain is the simulation's ground (docs/vision/peninsula.md,
//! stage 2): away from the road the land is the real grid, beside it the
//! road is blended in with the Track's own law, so what the car sits on is
//! what is drawn. Along every road of the network (stage 3), the junction
//! included.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

#[test]
fn the_terrain_is_the_tracks_ground_beside_and_away_from_the_road() {
    let (level, t, tr) = common::terrain("southcoast", None);
    assert!(level.open);
    assert_eq!(t.branches.len(), 1, "Highway 84 leaves Highway 1");
    let land = level.ground.as_ref().expect("the level's ground");
    let mut worst = (0.0f64, 0, 0.0, 0.0);
    let mut worst_far = (0.0f64, 0, 0.0, 0.0);
    for (k, road) in t.roads().enumerate() {
        let mut s = if k == 0 { 200.0 } else { 60.0 };
        while s < road.length - 200.0 {
            for lat in [-150.0, -60.0, -25.0, -9.0, 9.0, 25.0, 60.0, 150.0] {
                let p = road.point_at(s, lat);
                // Where another road runs (the junction) the car is on that
                // road's blend instead: `project_road` picks it.
                let other = t
                    .roads()
                    .enumerate()
                    .filter(|&(j, _)| j != k)
                    .any(|(_, o)| o.distance_to_road(p.x, p.z, 96.0).d < 60.0);
                if other {
                    continue;
                }
                let drawn = tr.height_at(p.x, p.z);
                // Driven from the nearest bit of the road, as the physics
                // projects it (84 winds back on itself within 150 m).
                let q = road.distance_to_road(p.x, p.z, 400.0);
                let (qs, qlat) = match q.s {
                    Some(qs) => (qs, q.lat),
                    None => (s, lat),
                };
                let driven = road.ground_y(p.x, p.z, qs, qlat);
                let d = (drawn - driven).abs();
                if d > worst.0 {
                    worst = (d, k, s, lat);
                }
                // Past the band the land is the grid itself.
                if qlat.abs() >= 60.0 {
                    let e = (drawn - land(p.x, p.z)).abs();
                    if e > worst_far.0 {
                        worst_far = (e, k, s, lat);
                    }
                }
            }
            s += 250.0;
        }
    }
    assert!(
        worst.0 < 0.75,
        "drawn against driven: {:.2} m on road {} at s {} lat {}",
        worst.0,
        worst.1,
        worst.2,
        worst.3
    );
    assert!(
        worst_far.0 < 1e-6,
        "far from the road the terrain is the grid: {:.3} m on road {} at s {} lat {}",
        worst_far.0,
        worst_far.1,
        worst_far.2,
        worst_far.3
    );
    eprintln!(
        "open ground: drawn against driven at worst {:.2} m (road {}, s {}, lat {})",
        worst.0, worst.1, worst.2, worst.3
    );
}

/// At the junction the two roads meet at one height: the branch's first
/// metres are the parent's surface, and the land between them is drawn
/// within a hand of both.
#[test]
fn the_junction_is_one_surface() {
    let (_, t, tr) = common::terrain("southcoast", None);
    let b = &t.branches[0];
    let mut s = 0.0;
    while s <= b.clear {
        let p = b.track.point_at(s, 0.0);
        let on_parent = t.project(p.x, p.z, b.s);
        let parent = t.surface_y(on_parent.s, on_parent.lat);
        let own = b.track.surface_y(s, 0.0);
        assert!(
            (own - parent).abs() < 0.08,
            "at {s} m along 84: {own} over Highway 1's {parent}"
        );
        s += 1.0;
    }
    // Beside the mouth, the drawn ground sits by the roads.
    for (s, lat) in [
        (b.clear + 8.0, 6.0),
        (b.clear + 8.0, -6.0),
        (b.clear + 20.0, 9.0),
    ] {
        let p = b.track.point_at(s, lat);
        let drawn = tr.height_at(p.x, p.z);
        let driven = b.track.ground_y(p.x, p.z, s, lat);
        assert!(
            (drawn - driven).abs() < 0.75,
            "beside the mouth at {s}/{lat}: drawn {drawn} driven {driven}"
        );
    }
}
