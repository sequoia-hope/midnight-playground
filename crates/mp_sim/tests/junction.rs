//! The open world's first junction (docs/vision/peninsula.md, stage 3):
//! the arcade car turns off Highway 1 onto Highway 84 and drives up it,
//! on 84's own surface, while its race progress stays at the junction.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use common::make_vehicle;
use mp_math::{clamp, kernel, wrap_angle};
use mp_sim::input::Input;
use mp_sim::physics::{CarPhysics, car_spec};
use mp_sim::race::LevelRuntime;
use std::f64::consts::PI;

const DT: f64 = 1.0 / 120.0;

#[test]
fn the_car_turns_off_highway_1_onto_84_and_climbs_to_la_honda() {
    let t = LevelRuntime::new(mp_levels::level_by_id("southcoast"))
        .unwrap()
        .track;
    assert_eq!(t.branches.len(), 1);
    let b = &t.branches[0];
    assert_eq!(b.id, "hwy84");
    assert!(b.s < 30.0, "84 leaves at the road's start: {}", b.s);
    let spec = car_spec("sports").unwrap();
    let mut v = make_vehicle("sports", spec.mass);
    let mut phys = CarPhysics::new(&v, spec);
    // 150 m up Highway 1 from the junction, facing back down to it.
    phys.reset(&mut v, &t, b.s + 150.0, 0.0);
    v.yaw += PI;
    assert_eq!(v.road, 0);
    // Aim at 84's mouth, then at a point 30 m ahead along it, at 50 km/h.
    let mut el = 0.0;
    let mut switched_at = None;
    while el < 90.0 {
        let aim = if v.road == 0 {
            b.track.point_at(8.0, 0.0)
        } else {
            b.track.point_at(v.road_s + 30.0, 0.0)
        };
        let alpha = wrap_angle(kernel::atan2(aim.z - v.z, aim.x - v.x) - v.yaw);
        let speed = kernel::hypot(v.vx, v.vz);
        let inp = Input {
            steer: clamp(alpha * 2.5, -1.0, 1.0),
            throttle: if speed < 14.0 { 0.6 } else { 0.0 },
            brake: if speed > 16.0 { 0.3 } else { 0.0 },
            ..Input::default()
        };
        phys.update(&mut v, &t, DT, &inp);
        if v.road == 1 && switched_at.is_none() {
            switched_at = Some((el, v.road_s, v.s));
        }
        el += DT;
    }
    let (at, rs, s) = switched_at.expect("the car got onto 84");
    eprintln!("onto 84 after {at:.1} s at {rs:.1} m along it, Highway 1 s {s:.1}");
    assert!(rs < 40.0, "took 84 from its mouth: {rs}");
    assert_eq!(v.road, 1, "still on 84");
    assert!(v.road_s > 600.0, "well up 84: {}", v.road_s);
    assert!(
        v.road_lat.abs() < 4.0,
        "on 84's pavement: lat {}",
        v.road_lat
    );
    assert!(v.on_ground);
    let g = b.track.ground_y(v.x, v.z, v.road_s, v.road_lat);
    assert!((v.y - g).abs() < 0.05, "on 84's surface: y {} vs {g}", v.y);
    assert!(phys.off_track.unwrap_or(1.0) < 0.01, "on the tarmac");
    // The race's progress is the nearest point of Highway 1, short of the
    // start, and the car is far off it.
    assert!(
        v.s < t.start_s,
        "Highway 1 progress {} (start {})",
        v.s,
        t.start_s
    );
    assert!(v.lat.abs() > 100.0, "far off Highway 1: {}", v.lat);
}
