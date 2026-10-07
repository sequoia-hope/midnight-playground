//! The sim model on a player's car (docs/vehicle-dynamics/SPEC.md V2,
//! simulation side): it drives the track, holds on the grid, writes the
//! body view the race reads, takes back what collisions and resets do to
//! it, and is deterministic, rollback included.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_sim::autopilot::autopilot_at;
use mp_sim::input::{Input, InputFrame};
use mp_sim::model::{VehicleModel, use_sim};
use mp_sim::race::{LevelRuntime, RaceOpts, RaceStateKind, SimState, hash, step};

fn level() -> LevelRuntime {
    LevelRuntime::new(mp_levels::level_by_id("sierra")).unwrap()
}

const OPTS: RaceOpts = RaceOpts {
    car: "sports",
    seed: 7,
    pursuit: false,
    heat: 1.0,
};

fn sim_race(lr: &LevelRuntime) -> SimState {
    let mut st = SimState::new(lr, OPTS);
    use_sim(&mut st.players[0], &lr.track);
    st
}

fn drive(lr: &LevelRuntime, st: &mut SimState, ticks: u32) {
    let mut ev = Vec::new();
    for _ in 0..ticks {
        let mut inp = Input::default();
        autopilot_at(&mut inp, &st.players[0].v, &lr.track, 0.67);
        step(lr, st, &[InputFrame::quantise(&inp)], &mut ev);
        ev.clear();
    }
}

fn sim(st: &SimState) -> &mp_sim::model::SimCar {
    match &st.players[0].model {
        VehicleModel::Sim(s) => s,
        VehicleModel::Arcade => panic!("not a sim car"),
    }
}

#[test]
fn it_holds_on_the_grid_then_drives_the_track() {
    let lr = level();
    let mut st = sim_race(&lr);
    let (x0, z0, s0) = (st.players[0].v.x, st.players[0].v.z, st.players[0].v.s);
    // The countdown: full throttle, and the car stays put.
    let mut ev = Vec::new();
    let gas = InputFrame {
        throttle: 255,
        ..InputFrame::default()
    };
    while st.race.state == RaceStateKind::Countdown {
        step(&lr, &mut st, &[gas], &mut ev);
        let v = &st.players[0].v;
        assert!((v.x - x0).abs() < 0.05 && (v.z - z0).abs() < 0.05);
        if st.race.state == RaceStateKind::Countdown {
            assert!(st.players[0].phys.rpm > 6000.0, "revs on the grid");
        }
    }
    drive(&lr, &mut st, 40 * 120);
    let p = &st.players[0];
    let gone = lr.track.ds(s0, p.v.s);
    assert!(gone > 600.0, "{gone} m in 40 s");
    assert!(p.phys.gear >= 2);
    assert!(p.rules.reset_cooldown == 0.0);
    // The body view is the rigid body's.
    let s = sim(&st);
    let b = &s.car.body;
    assert_eq!(
        (p.v.x, p.v.z, p.v.vx, p.v.vz),
        (b.pos.x, b.pos.z, b.vel.x, b.vel.z)
    );
    assert_eq!(p.v.yaw, s.car.yaw());
    let pose = p.v.pose.as_ref().expect("a sim car's pose");
    assert_eq!(pose.wheels.len(), 4);
    assert_eq!(pose.orient, b.rot.to_array());
    assert!(p.v.on_ground);
    // On the road surface: the body view's y is the ground under it.
    let ground = lr.track.surface_y(p.v.s, p.v.lat);
    assert!((p.v.y - ground).abs() < 0.1, "{} vs {ground}", p.v.y);
}

#[test]
fn the_same_inputs_give_the_same_race_and_a_rollback_replays_it() {
    let lr = level();
    let mut a = sim_race(&lr);
    let mut b = sim_race(&lr);
    drive(&lr, &mut a, 20 * 120);
    drive(&lr, &mut b, 20 * 120);
    assert_eq!(a, b);
    assert_eq!(hash(&a), hash(&b));
    // Native and wasm (CI runs these tests in Node) must both get this.
    assert_eq!(hash(&a), 0x3b912e4b6f8511ed, "hash {:#018x}", hash(&a));
    // Rollback: a saved state stepped again gives the same bits.
    let saved = a.clone();
    drive(&lr, &mut a, 5 * 120);
    let mut again = saved;
    drive(&lr, &mut again, 5 * 120);
    assert_eq!(hash(&a), hash(&again));
    // And the sim car's state is in the hash: the arcade race differs.
    let mut arcade = SimState::new(&lr, OPTS);
    drive(&lr, &mut arcade, 25 * 120);
    assert_ne!(hash(&a), hash(&arcade));
}

#[test]
fn it_takes_back_pushes_and_resets() {
    let lr = level();
    let mut st = sim_race(&lr);
    drive(&lr, &mut st, 10 * 120);
    // A collision's push: the body view's velocity and spin change, and the
    // rigid body follows on the next tick.
    let before = sim(&st).car.body.vel;
    let p = &mut st.players[0];
    p.v.vx += 3.0;
    p.v.yaw_rate += 0.5;
    let yaw_rate = sim(&st).car.yaw_rate();
    drive(&lr, &mut st, 1);
    let after = sim(&st).car.body.vel;
    assert!(after.x - before.x > 2.0, "{} -> {}", before.x, after.x);
    assert!(sim(&st).car.yaw_rate() - yaw_rate > 0.3);
    // A reset elsewhere: the car starts again there.
    let t = &lr.track;
    let p = &mut st.players[0];
    p.phys.reset(&mut p.v, t, 1200.0, 0.0);
    drive(&lr, &mut st, 1);
    let v = &st.players[0].v;
    assert!(t.ds(1200.0, v.s).abs() < 1.0, "s {}", v.s);
    assert!(v.speed.abs() < 0.5);
}
