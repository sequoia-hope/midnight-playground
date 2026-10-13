//! An open level's terrain is the simulation's ground (docs/vision/peninsula.md,
//! stage 2): away from the road the land is the real grid, beside it the
//! road is blended in with the Track's own law, so what the car sits on is
//! what is drawn.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

#[test]
fn the_terrain_is_the_tracks_ground_beside_and_away_from_the_road() {
    let (level, t, tr) = common::terrain("southcoast", None);
    assert!(level.open);
    let land = level.ground.as_ref().expect("the level's ground");
    let mut worst = (0.0f64, 0.0, 0.0);
    let mut worst_far = (0.0f64, 0.0, 0.0);
    let mut s = 200.0;
    while s < t.length - 200.0 {
        for lat in [-150.0, -60.0, -25.0, -9.0, 9.0, 25.0, 60.0, 150.0] {
            let p = t.point_at(s, lat);
            let drawn = tr.height_at(p.x, p.z);
            let driven = t.ground_y(p.x, p.z, s, lat);
            let d = (drawn - driven).abs();
            if d > worst.0 {
                worst = (d, s, lat);
            }
            // Past the band the land is the grid itself.
            if lat.abs() >= 60.0 {
                let e = (drawn - land(p.x, p.z)).abs();
                if e > worst_far.0 {
                    worst_far = (e, s, lat);
                }
            }
        }
        s += 250.0;
    }
    assert!(
        worst.0 < 0.75,
        "drawn against driven: {:.2} m at s {} lat {}",
        worst.0,
        worst.1,
        worst.2
    );
    assert!(
        worst_far.0 < 1e-6,
        "far from the road the terrain is the grid: {:.3} m at s {} lat {}",
        worst_far.0,
        worst_far.1,
        worst_far.2
    );
    eprintln!(
        "open ground: drawn against driven at worst {:.2} m (s {}, lat {})",
        worst.0, worst.1, worst.2
    );
}
