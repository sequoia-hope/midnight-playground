//! An open level (docs/vision/peninsula.md, stage 2): the arcade car may
//! leave the road onto the real ground, and sits on it; on a closed level
//! the same driving stays between the walls.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use common::make_vehicle;
use mp_sim::input::Input;
use mp_sim::physics::{CarPhysics, car_spec};
use mp_sim::race::LevelRuntime;
use mp_sim::vehicle::Vehicle;
use mp_track::Track;
use std::sync::Arc;

const DT: f64 = 1.0 / 120.0;

fn track(id: &str) -> Arc<Track> {
    LevelRuntime::new(mp_levels::level_by_id(id)).unwrap().track
}

/// Full lock to one side with the throttle down, from `s` at road speed:
/// where the car ends up after `seconds`.
fn swerve(t: &Track, s: f64, seconds: f64) -> (Vehicle, CarPhysics) {
    let spec = car_spec("sports").unwrap();
    let mut v = make_vehicle("sports", spec.mass);
    let mut phys = CarPhysics::new(&v, spec);
    phys.reset(&mut v, t, s, 0.0);
    let f = t.frame(s);
    v.vx = f.fx * 20.0;
    v.vz = f.fz * 20.0;
    let inp = Input {
        steer: 1.0,
        throttle: 0.6,
        ..Input::default()
    };
    let mut el = 0.0;
    while el < seconds {
        phys.update(&mut v, t, DT, &inp);
        el += DT;
    }
    (v, phys)
}

#[test]
fn an_open_level_lets_the_car_leave_the_road_and_keeps_it_on_the_ground() {
    let t = track("southcoast");
    assert!(t.open_ground.is_some(), "the corridor is open");
    let (v, _) = swerve(&t, 600.0, 8.0);
    let f = t.frame(v.s);
    assert!(
        v.lat.abs() > f.hw + f.margin + 10.0,
        "well off the road: lat {} (hw {})",
        v.lat,
        f.hw
    );
    for c in [v.x, v.y, v.z, v.s, v.lat] {
        assert!(c.is_finite());
    }
    // On the land: the simulation's ground there, which is the real
    // grid away from the road.
    let g = t.ground_y(v.x, v.z, v.s, v.lat);
    assert!((v.y - g).abs() < 0.05, "y {} vs ground {g}", v.y);
    let land = t.open_ground.as_ref().unwrap()(v.x, v.z);
    assert!((g - land).abs() < 0.5, "past the band: {g} vs {land}");
    assert!(v.on_ground);
}

#[test]
fn a_closed_level_keeps_the_car_between_the_walls() {
    let t = track("coast");
    assert!(t.open_ground.is_none());
    let (v, _) = swerve(&t, 600.0, 8.0);
    let f = t.frame(v.s);
    assert!(
        v.lat <= f.wall_r + 0.01 && v.lat >= -f.wall_l - 0.01,
        "lat {} within walls {} / {}",
        v.lat,
        f.wall_l,
        f.wall_r
    );
}

#[test]
fn off_the_shoulder_the_car_is_on_loose_ground() {
    let t = track("southcoast");
    let (v, phys) = swerve(&t, 600.0, 8.0);
    assert!(v.lat.abs() > 20.0);
    assert!(
        phys.off_track.unwrap_or(0.0) > 0.99,
        "loose: {:?}",
        phys.off_track
    );
}
