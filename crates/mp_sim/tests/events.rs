//! Events at places (docs/vision/peninsula.md, stage 5): on the loop's
//! cruise, stopping on a start's marker and holding gas and brake begins
//! its event; the event runs as a race does and hands back to the cruise.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_sim::event::{self, EventResult};
use mp_sim::input::{AUTOPILOT, Input, InputFrame};
use mp_sim::race::{LevelRuntime, RaceOpts, RaceStateKind, SimEvent, SimState, step};
use mp_track::{EventKind, Mode};

/// The loop as the client's Cruise makes it (`play::cruise_level`).
fn cruise() -> LevelRuntime {
    let mut level = mp_levels::level_by_id("peninsula");
    level.mode = Mode::Cruise;
    level.laps = None;
    level.lap_length = None;
    LevelRuntime::new(level).unwrap()
}

fn state(lr: &LevelRuntime) -> SimState {
    SimState::new(
        lr,
        RaceOpts {
            car: "sports",
            seed: 3,
            pursuit: false,
            heat: 1.0,
            physics: mp_sim::model::Physics::Arcade,
        },
    )
}

fn frame(throttle: f64, brake: f64) -> InputFrame {
    InputFrame::quantise(&Input {
        throttle,
        brake,
        ..Input::default()
    })
}

fn autopilot() -> InputFrame {
    InputFrame {
        flags: AUTOPILOT,
        ..InputFrame::default()
    }
}

/// Ticks `secs` on one input, returning the events.
fn run(lr: &LevelRuntime, st: &mut SimState, f: InputFrame, secs: f64) -> Vec<SimEvent> {
    let mut ev = Vec::new();
    for _ in 0..(secs * 120.0) as usize {
        step(lr, st, &[f], &mut ev);
    }
    ev
}

/// Ticks on `f` until the event ends (at most `secs`).
fn until_end(lr: &LevelRuntime, st: &mut SimState, f: InputFrame, secs: f64) -> EventResult {
    let mut ev = Vec::new();
    for _ in 0..(secs * 120.0) as usize {
        step(lr, st, &[f], &mut ev);
        if let Some(r) = ev.iter().find_map(|e| match e {
            SimEvent::EventEnd(r) => Some(r.clone()),
            _ => None,
        }) {
            return r;
        }
        ev.clear();
    }
    panic!("the event did not end in {secs} s");
}

/// The cruise begins on the first race's marker, past its countdown.
fn on_the_first_marker(lr: &LevelRuntime) -> SimState {
    let mut st = state(lr);
    assert!(
        st.race.cruise && st.rivals.is_empty(),
        "a cruise has no rivals"
    );
    run(lr, &mut st, InputFrame::default(), 4.5);
    assert_eq!(st.race.state, RaceStateKind::Racing);
    assert_eq!(
        event::at_start(&lr.track, &lr.level, &st.players[0].v),
        Some(0),
        "the cruise starts on the first race's marker"
    );
    st
}

#[test]
fn holding_gas_and_brake_on_a_marker_starts_its_race() {
    let lr = cruise();
    let mut st = on_the_first_marker(&lr);
    // Gas alone, or both for less than the hold, starts nothing.
    run(&lr, &mut st, frame(1.0, 0.0), 0.5);
    run(&lr, &mut st, frame(0.0, 1.0), 1.0);
    run(&lr, &mut st, frame(1.0, 1.0), 0.6);
    assert!(st.race.hold.is_some() && st.race.event.is_none());
    run(&lr, &mut st, frame(0.0, 1.0), 0.2);
    assert!(st.race.hold.is_none(), "letting go empties the hold");
    let ev = run(&lr, &mut st, frame(1.0, 1.0), 1.1);
    assert!(ev.contains(&SimEvent::EventStart { index: 0 }), "{ev:?}");
    assert!(!st.race.cruise);
    assert_eq!(st.race.state, RaceStateKind::Countdown);
    assert_eq!(st.rivals.len(), lr.level.rivals.len(), "the rivals join");
    let e = &lr.level.events[0];
    assert_eq!(
        st.race.finish_prog,
        e.s + event::ahead(&lr.track, e.s, e.finish)
    );
    // The grid: the player fourth, behind the start.
    let p = &st.players[0].v;
    assert!(lr.track.ds(e.s, p.s) < -10.0 && lr.track.ds(e.s, p.s) > -20.0);
    for a in &st.rivals {
        assert!(lr.track.ds(e.s, a.k.s) < 0.0);
    }
}

#[test]
fn a_race_at_a_place_runs_to_its_finish_and_the_cruise_comes_back() {
    let lr = cruise();
    let mut st = on_the_first_marker(&lr);
    // Some cruise score first, to see it kept.
    st.players[0].rules.score = 1234.0;
    st.players[0].rules.mult = 3.0;
    run(&lr, &mut st, frame(1.0, 1.0), 1.1);
    assert!(st.race.event.is_some());
    let r = until_end(&lr, &mut st, autopilot(), 600.0);
    eprintln!(
        "race: place {} time {:.1} made {}",
        r.place, r.time, r.made_it
    );
    for row in &r.rows {
        eprintln!(
            "  {} {} {:.1}{}",
            row.place,
            row.name,
            row.time,
            if row.estimated { " est" } else { "" }
        );
    }
    assert_eq!(r.kind, EventKind::Race);
    assert_eq!(r.rows.len(), 1 + lr.level.rivals.len());
    assert!(r.time > 120.0 && r.time < 500.0, "{}", r.time);
    // The finish is at Half Moon Bay.
    let e = &lr.level.events[0];
    let s = st.players[0].v.s;
    assert!(lr.track.ds(e.finish, s) > -5.0 && lr.track.ds(e.finish, s) < 300.0);
    // The cruise is back as it was.
    assert!(st.race.cruise && st.race.event.is_none());
    assert_eq!(st.race.state, RaceStateKind::Racing);
    assert!(st.rivals.is_empty());
    assert_eq!(st.players[0].rules.score, 1234.0);
    assert!(!st.players[0].rules.finished);
    assert!(!st.players[0].phys.locked);
    // And it drives on.
    let before = st.players[0].rules.dist;
    run(&lr, &mut st, autopilot(), 5.0);
    assert!(st.players[0].rules.dist > before + 50.0);
}

/// Puts the cruising car on start `k`'s marker, stopped.
fn on_marker(lr: &LevelRuntime, st: &mut SimState, k: usize) {
    let e = &lr.level.events[k];
    let p = &mut st.players[0];
    p.phys.reset(&mut p.v, &lr.track, e.s, 2.0);
    run(lr, st, InputFrame::default(), 0.5);
    assert_eq!(
        event::at_start(&lr.track, &lr.level, &st.players[0].v),
        Some(k)
    );
}

#[test]
fn a_time_trial_is_alone_against_its_time() {
    let lr = cruise();
    let mut st = on_the_first_marker(&lr);
    on_marker(&lr, &mut st, 2);
    run(&lr, &mut st, frame(1.0, 1.0), 1.1);
    assert_eq!(
        st.race.event.as_ref().map(|e| e.kind),
        Some(EventKind::TimeTrial)
    );
    assert!(st.rivals.is_empty());
    let r = until_end(&lr, &mut st, autopilot(), 900.0);
    for e in &lr.level.events {
        eprintln!(
            "{}: profile {:.0} s, time {}",
            e.id,
            event::profile_time(&lr.track, e.s, e.finish),
            event::time_of(&lr.track, e)
        );
    }
    eprintln!("trial: time {:.1} made {}", r.time, r.made_it);
    assert_eq!(r.place, 1);
    assert!(r.rows.is_empty());
    assert!(st.race.cruise);
}

#[test]
fn a_delivery_is_made_at_its_drop_by_any_road() {
    let lr = cruise();
    let mut st = on_the_first_marker(&lr);
    let k = lr
        .level
        .events
        .iter()
        .position(|e| e.kind == EventKind::Delivery)
        .unwrap();
    on_marker(&lr, &mut st, k);
    run(&lr, &mut st, frame(1.0, 1.0), 1.1);
    run(&lr, &mut st, InputFrame::default(), 4.5);
    assert_eq!(st.race.state, RaceStateKind::Racing);
    // Driving the wrong way round the loop is no wrong way.
    let mut ev = Vec::new();
    {
        let p = &mut st.players[0];
        let s = p.v.s;
        p.phys.reset(&mut p.v, &lr.track, s, 2.0);
        p.v.yaw += std::f64::consts::PI;
        p.v.vx = -p.v.vx;
        p.v.vz = -p.v.vz;
    }
    for _ in 0..240 {
        step(&lr, &mut st, &[frame(0.6, 0.0)], &mut ev);
    }
    assert!(
        !ev.iter().any(|e| matches!(e, SimEvent::WrongWay { .. })),
        "a delivery knows no wrong way"
    );
    // At the drop (on Stage Road), the delivery is made.
    let [dx, dz] = lr.level.events[k].drop.unwrap();
    {
        let p = &mut st.players[0];
        p.v.x = dx;
        p.v.z = dz;
        p.v.vx = 0.0;
        p.v.vz = 0.0;
        p.v.project_onto(&lr.track);
    }
    let r = until_end(&lr, &mut st, InputFrame::default(), 10.0);
    assert_eq!(r.kind, EventKind::Delivery);
    assert!(r.made_it, "{r:?}");
    assert!(st.race.cruise);
}

#[test]
fn a_delivery_out_of_time_is_not_made() {
    let lr = cruise();
    let mut st = on_the_first_marker(&lr);
    let k = lr.level.events.len() - 1;
    on_marker(&lr, &mut st, k);
    run(&lr, &mut st, frame(1.0, 1.0), 1.1);
    let limit = lr.level.events[k].time;
    let r = until_end(&lr, &mut st, frame(0.0, 1.0), limit + 20.0);
    assert!(!r.made_it);
    assert!(r.time > limit);
    assert!(st.race.cruise && st.race.event.is_none());
}

#[test]
fn with_physics_all_an_events_rivals_drive_the_sim_model() {
    let lr = cruise();
    let mut st = SimState::new(
        &lr,
        RaceOpts {
            car: "sports",
            seed: 3,
            pursuit: false,
            heat: 1.0,
            physics: mp_sim::model::Physics::All,
        },
    );
    run(&lr, &mut st, InputFrame::default(), 4.5);
    assert_eq!(
        event::at_start(&lr.track, &lr.level, &st.players[0].v),
        Some(0)
    );
    run(&lr, &mut st, frame(1.0, 1.0), 1.1);
    assert!(st.race.event.is_some());
    assert_eq!(st.rivals.len(), lr.level.rivals.len());
    assert!(st.rivals.iter().all(|a| a.sim.is_some()));
    // They get going off the grid.
    run(&lr, &mut st, InputFrame::default(), 4.0);
    let s0: Vec<f64> = st.rivals.iter().map(|a| a.k.s).collect();
    run(&lr, &mut st, autopilot(), 10.0);
    for (a, s) in st.rivals.iter().zip(s0) {
        assert!(
            lr.track.ds(s, a.k.s) > 100.0,
            "{} moved {}",
            a.name,
            lr.track.ds(s, a.k.s)
        );
    }
}
