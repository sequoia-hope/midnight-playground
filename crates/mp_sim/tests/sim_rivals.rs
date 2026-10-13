//! The Handling setting (DECISIONS D1202): `RaceOpts::physics` puts the
//! player's car, or every racer, on the sim model. A sim rival holds on the
//! grid, drives the course its AI picks, keeps its track coordinates on its
//! body view, and is deterministic, rollback included.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_sim::autopilot::autopilot_at;
use mp_sim::input::{Input, InputFrame};
use mp_sim::model::{Physics, VehicleModel};
use mp_sim::race::{LevelRuntime, RaceOpts, RaceStateKind, SimState, hash, step};

fn level() -> LevelRuntime {
    LevelRuntime::new(mp_levels::level_by_id("sierra")).unwrap()
}

fn opts(physics: Physics) -> RaceOpts {
    RaceOpts {
        car: "sports",
        seed: 7,
        pursuit: false,
        heat: 1.0,
        physics,
    }
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

#[test]
fn the_setting_picks_the_cars() {
    for p in Physics::ALL {
        assert_eq!(Physics::parse(p.name()), Some(p));
    }
    assert_eq!(Physics::parse("sim"), None);
    let lr = level();
    let sim_player = |st: &SimState| matches!(st.players[0].model, VehicleModel::Sim(_));
    let sim_rivals = |st: &SimState| st.rivals.iter().filter(|a| a.sim.is_some()).count();

    let st = SimState::new(&lr, opts(Physics::Arcade));
    assert!(!sim_player(&st) && sim_rivals(&st) == 0);
    let st = SimState::new(&lr, opts(Physics::Player));
    assert!(sim_player(&st) && sim_rivals(&st) == 0);
    let st = SimState::new(&lr, opts(Physics::All));
    assert!(sim_player(&st) && sim_rivals(&st) == st.rivals.len());
    assert!(!st.rivals.is_empty());
}

#[test]
fn sim_rivals_hold_on_the_grid_then_race() {
    let lr = level();
    let t = &*lr.track;
    let mut st = SimState::new(&lr, opts(Physics::All));
    let grid: Vec<(f64, f64, f64)> = st
        .rivals
        .iter()
        .map(|a| (a.k.v.x, a.k.v.z, a.k.s))
        .collect();
    let mut ev = Vec::new();
    while st.race.state == RaceStateKind::Countdown {
        step(&lr, &mut st, &[InputFrame::default()], &mut ev);
        for (a, &(x, z, _)) in st.rivals.iter().zip(&grid) {
            assert!((a.k.v.x - x).abs() < 0.05 && (a.k.v.z - z).abs() < 0.05);
        }
    }
    drive(&lr, &mut st, 40 * 120);
    for (a, &(_, _, s0)) in st.rivals.iter().zip(&grid) {
        let gone = t.ds(s0, a.k.s);
        assert!(gone > 600.0, "{}: {gone} m in 40 s", a.name);
        // Its track coordinates are its body view's.
        let p = t.project(a.k.v.x, a.k.v.z, a.k.s);
        assert!((p.s - a.k.s).abs() < 1e-9 && (p.lat - a.k.lat).abs() < 1e-9);
        assert!(a.k.v.pose.is_some(), "drawn from its chassis");
        assert!(
            a.k.lat.abs() < t.frame(a.k.s).hw + 1.0,
            "{} on the road",
            a.name
        );
    }
}

#[test]
fn a_sim_field_is_deterministic_and_a_rollback_replays_it() {
    let lr = level();
    let mut a = SimState::new(&lr, opts(Physics::All));
    let mut b = SimState::new(&lr, opts(Physics::All));
    drive(&lr, &mut a, 20 * 120);
    drive(&lr, &mut b, 20 * 120);
    assert_eq!(hash(&a), hash(&b));
    let saved = a.clone();
    drive(&lr, &mut a, 600);
    let mut c = saved;
    drive(&lr, &mut c, 600);
    assert_eq!(a, c);
    assert_eq!(hash(&a), hash(&c));
    // The rivals' sim state is in the hash.
    let arcade_rivals = {
        let mut st = SimState::new(&lr, opts(Physics::Player));
        drive(&lr, &mut st, 20 * 120 + 600);
        hash(&st)
    };
    assert_ne!(hash(&a), arcade_rivals);
}
