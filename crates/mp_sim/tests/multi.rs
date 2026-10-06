//! Multiplayer races in the simulation (`SimState::new_multi`, the rules of
//! `docs/rust-port/MULTIPLAYER.md`): one human is exactly single-player,
//! and several race, finish, end and report as the rules say.

mod common;

use common::level;
use mp_sim::autopilot::autopilot;
use mp_sim::input::{AUTOPILOT, Input, InputFrame, RESET};
use mp_sim::race::{
    FINISH_WINDOW, Human, LevelRuntime, MAX_FIELD, MultiOpts, RaceOpts, RaceStateKind, SimEvent,
    SimState, hash, results, standings, step,
};

fn lr(id: &str) -> LevelRuntime {
    LevelRuntime::new(level(id)).unwrap()
}

fn drive(st: &SimState, lr: &LevelRuntime, i: usize) -> InputFrame {
    let mut inp = Input::default();
    autopilot(&mut inp, &st.players[i].v, &lr.track);
    InputFrame::quantise(&inp)
}

fn opts(humans: &[&'static str]) -> MultiOpts {
    MultiOpts {
        seed: 7,
        humans: humans
            .iter()
            .map(|&car| Human { car, color: None })
            .collect(),
        grid: (0..humans.len()).collect(),
        field: None,
        rubber_band: true,
        ghost: false,
    }
}

/// One human with the default rules is the single-player race, tick for
/// tick: the same state hash and the same events, on every level with a
/// race (a circuit, sprints, rivals or none).
#[test]
fn one_human_is_exactly_single_player() {
    for id in ["coast", "sierra", "streets", "desert", "seaside", "cruise"] {
        let lr = lr(id);
        let sp_opts = RaceOpts {
            car: "muscle",
            seed: 7,
            pursuit: false,
            heat: 1.0,
        };
        let mut a = SimState::new(&lr, sp_opts);
        let mut b = SimState::new_multi(&lr, &opts(&["muscle"]));
        assert_eq!(hash(&a), hash(&b), "{id}: the grid");
        let (mut ea, mut eb) = (Vec::new(), Vec::new());
        for k in 0..6000 {
            let f = drive(&a, &lr, 0);
            step(&lr, &mut a, &[f], &mut ea);
            step(&lr, &mut b, &[f], &mut eb);
            if k % 50 == 0 {
                assert_eq!(hash(&a), hash(&b), "{id}: tick {k}");
            }
        }
        assert_eq!(hash(&a), hash(&b), "{id}: the end");
        assert_eq!(ea, eb, "{id}: the events");
    }
}

/// Runs a race with every human on the autopilot until the results, or
/// `max` ticks. Returns the events. The autopilot doesn't steer round other
/// cars, so like a player it presses reset when it has been stuck a while.
fn run(lr: &LevelRuntime, st: &mut SimState, max: u32) -> Vec<SimEvent> {
    let mut ev = Vec::new();
    for _ in 0..max {
        let frames: Vec<InputFrame> = (0..st.players.len())
            .map(|i| {
                let mut f = drive(st, lr, i);
                if st.players[i].rules.stuck.unwrap_or(0.0) > 2.0 {
                    f.flags |= RESET;
                }
                f
            })
            .collect();
        let from = ev.len();
        step(lr, st, &frames, &mut ev);
        if ev[from..].contains(&SimEvent::Results) {
            break;
        }
    }
    ev
}

#[test]
fn four_humans_race_finish_and_get_results() {
    let lr = lr("sierra");
    let mut st = SimState::new_multi(&lr, &opts(&["sports", "muscle", "super", "rally"]));
    assert_eq!(st.players.len(), 4);
    let ev = run(&lr, &mut st, 120 * 600);
    assert!(ev.contains(&SimEvent::Results), "the race ended");
    assert_eq!(st.race.state, RaceStateKind::Finished);
    for i in 0..4 {
        assert!(
            ev.iter()
                .any(|e| matches!(e, SimEvent::Finished { player, .. } if *player == i)),
            "player {i} finished"
        );
        assert!(st.players[i].rules.finished);
    }
    let places: Vec<usize> = ev
        .iter()
        .filter_map(|e| match e {
            SimEvent::Finished { place, .. } => Some(*place),
            _ => None,
        })
        .collect();
    let mut sorted = places.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        4,
        "every human has their own place: {places:?}"
    );
    let r = results(&st);
    assert_eq!(r.iter().filter(|r| r.player).count(), 4);
    let humans: Vec<usize> = r.iter().filter_map(|r| r.human).collect();
    let mut h = humans.clone();
    h.sort_unstable();
    assert_eq!(h, vec![0, 1, 2, 3]);
    // Names are the simulation's own; the client shows the chosen ones.
    assert_eq!(st.players[2].v.name, "P3");
}

#[test]
fn the_same_inputs_give_the_same_race() {
    let lr = lr("coast");
    let o = opts(&["sports", "electric", "rally"]);
    let mut a = SimState::new_multi(&lr, &o);
    let mut b = SimState::new_multi(&lr, &o);
    run(&lr, &mut a, 120 * 60);
    run(&lr, &mut b, 120 * 60);
    assert_eq!(hash(&a), hash(&b));
    assert_eq!(a, b);
}

#[test]
fn the_grid_puts_humans_fourth_back_among_five_rivals_in_grid_order() {
    let lr = lr("sierra");
    let mut o = opts(&["sports", "muscle"]);
    o.grid = vec![1, 0];
    let st = SimState::new_multi(&lr, &o);
    assert_eq!(st.rivals.len(), 5);
    let t = &lr.track;
    // Slots: rivals 0..2 in places 1-3, then human 1, human 0, rivals 3, 4.
    let s_of = |k: usize| t.wrap(t.start_s - 5.0 - (k / 2) as f64 * 10.0 - (k % 2) as f64 * 3.0);
    assert!((st.players[1].v.s - s_of(3)).abs() < 0.5);
    assert!((st.players[0].v.s - s_of(4)).abs() < 0.5);
    assert!((st.rivals[3].k.s - s_of(5)).abs() < 0.5);
    // Everyone has their own spot.
    let mut spots: Vec<(i64, i64)> = st
        .players
        .iter()
        .map(|p| ((p.v.s * 10.0) as i64, (p.v.lat * 10.0) as i64))
        .chain(
            st.rivals
                .iter()
                .map(|a| ((a.k.s * 10.0) as i64, (a.k.lat * 10.0) as i64)),
        )
        .collect();
    spots.sort_unstable();
    spots.dedup();
    assert_eq!(spots.len(), 7);
}

#[test]
fn the_field_fills_to_eight_and_never_more() {
    let lr = lr("sierra");
    let mut o = opts(&["sports"]);
    o.field = Some(8);
    let st = SimState::new_multi(&lr, &o);
    assert_eq!(st.rivals.len(), 7);
    let names: Vec<&str> = st.rivals.iter().map(|a| a.name).collect();
    let mut u = names.clone();
    u.sort_unstable();
    u.dedup();
    assert_eq!(u.len(), 7, "every rival has its own name: {names:?}");

    let mut o = opts(&["sports"; MAX_FIELD]);
    o.field = Some(8);
    let st = SimState::new_multi(&lr, &o);
    assert_eq!((st.players.len(), st.rivals.len()), (8, 0));

    let mut o = opts(&["sports", "muscle"]);
    o.field = Some(0);
    let st = SimState::new_multi(&lr, &o);
    assert!(st.rivals.is_empty());
    let mut o = opts(&["sports", "muscle"]);
    o.field = Some(6);
    let st = SimState::new_multi(&lr, &o);
    assert_eq!(st.rivals.len(), 4);
}

/// A player who stays on the grid doesn't hold the race up: the others'
/// window runs out 45 s after the first human finishes, and the results
/// estimate the one still out there.
#[test]
fn the_finish_window_ends_the_race_for_whoever_is_still_out() {
    let lr = lr("sierra");
    let mut o = opts(&["sports", "muscle"]);
    o.field = Some(0);
    let mut st = SimState::new_multi(&lr, &o);
    let mut ev = Vec::new();
    let mut first_finish = None;
    for k in 0..120 * 600 {
        let frames = [drive(&st, &lr, 0), InputFrame::default()];
        let from = ev.len();
        step(&lr, &mut st, &frames, &mut ev);
        if first_finish.is_none() && st.players[0].rules.finished {
            first_finish = Some(k);
        }
        if ev[from..].contains(&SimEvent::Results) {
            let f = first_finish.expect("player 0 finished");
            let after = (k - f) as f64 / 120.0;
            assert!(
                (after - (FINISH_WINDOW + 3.2)).abs() < 0.05,
                "results {after:.2} s after the finish"
            );
            break;
        }
    }
    assert!(ev.contains(&SimEvent::Results));
    assert!(!st.players[1].rules.finished);
    let r = results(&st);
    let p1 = r.iter().find(|r| r.human == Some(1)).unwrap();
    assert!(p1.estimated);
    assert_eq!(r[0].human, Some(0));
}

/// A dropped player is driven home by the autopilot (the flag in their
/// relayed input) and doesn't hold the race open.
#[test]
fn a_dropped_player_is_driven_home_and_the_race_ends_without_waiting() {
    let lr = lr("coast");
    let mut o = opts(&["sports", "muscle"]);
    o.field = Some(0);
    let mut st = SimState::new_multi(&lr, &o);
    let away = InputFrame {
        flags: AUTOPILOT,
        ..InputFrame::default()
    };
    let mut ev = Vec::new();
    let s0 = st.players[1].v.s;
    for _ in 0..120 * 30 {
        let frames = [drive(&st, &lr, 0), away];
        step(&lr, &mut st, &frames, &mut ev);
    }
    assert!(st.players[1].v.s - s0 > 300.0, "the autopilot drove it");
    // Player 0 finishing ends the race at once: the other is the AI's.
    for _ in 0..120 * 600 {
        let frames = [drive(&st, &lr, 0), away];
        let from = ev.len();
        step(&lr, &mut st, &frames, &mut ev);
        if ev[from..].contains(&SimEvent::Results) {
            break;
        }
    }
    assert!(ev.contains(&SimEvent::Results));
    let end = st.race.multi.as_ref().unwrap();
    assert!(
        end.end_timer.unwrap() > 0.0,
        "ended before the window ran out"
    );
}

/// In ghost mode two humans driving through the same spot don't touch;
/// otherwise they do.
#[test]
fn ghost_mode_lets_humans_pass_through_each_other() {
    let lr = lr("sierra");
    for ghost in [false, true] {
        let mut o = opts(&["sports", "sports"]);
        o.field = Some(0);
        o.ghost = ghost;
        let mut st = SimState::new_multi(&lr, &o);
        let mut ev = Vec::new();
        let mut contact = false;
        for k in 0..120 * 12 {
            // Both drive the same line; the one behind (on the right, a car
            // length back) steers across into the other.
            let f0 = drive(&st, &lr, 0);
            let mut f1 = drive(&st, &lr, 1);
            if k > 120 * 4 {
                f1.steer = -12000;
            }
            let from = ev.len();
            step(&lr, &mut st, &[f0, f1], &mut ev);
            contact |= ev[from..].iter().any(|e| match e {
                SimEvent::CarHit { hit, .. } => hit.a < 2 && hit.b < 2,
                _ => false,
            });
        }
        assert_eq!(contact, !ghost, "ghost {ghost}");
    }
}

#[test]
fn standings_list_every_human() {
    let lr = lr("seaside");
    let st = SimState::new_multi(&lr, &opts(&["sports", "super", "electric"]));
    let s = standings(&st);
    assert_eq!(s.len(), 3 + st.rivals.len());
    assert_eq!(s.iter().filter(|r| r.player).count(), 3);
    assert_eq!(st.race.prog_s.len(), 3 + st.rivals.len());
}

#[test]
fn colours_can_be_chosen() {
    let lr = lr("coast");
    let mut o = opts(&["sports", "sports"]);
    o.humans[1].color = Some(0x123456);
    let st = SimState::new_multi(&lr, &o);
    assert_eq!(st.players[1].spec.color, 0x123456);
    assert_ne!(st.players[0].spec.color, 0x123456);
    let s = standings(&st);
    assert!(s.iter().any(|r| r.human == Some(1) && r.color == 0x123456));
}
