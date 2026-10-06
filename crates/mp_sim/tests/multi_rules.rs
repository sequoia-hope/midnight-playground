//! The multiplayer rules of `SimState::new_multi` and `step` one at a time
//! (`docs/rust-port/MULTIPLAYER.md` section 2), beside the whole races of
//! `multi.rs`: the grid for every count of humans and AI, the field cap and
//! the names, rubber-banding to the nearest human, ghost mode against AI,
//! the AUTOPILOT and AWAY input flags, each player's own perfect start, and
//! the order of standings and results with finishers and DNFs.

mod common;

use common::level;
use mp_sim::autopilot::autopilot;
use mp_sim::input::{AUTOPILOT, AWAY, Input, InputFrame};
use mp_sim::race::{
    Human, LevelRuntime, MAX_FIELD, MultiOpts, RaceStateKind, SimEvent, SimState, hash, results,
    standings, step,
};

fn lr(id: &str) -> LevelRuntime {
    LevelRuntime::new(level(id)).unwrap()
}

fn opts(n: usize) -> MultiOpts {
    const CARS: [&str; 5] = ["sports", "muscle", "super", "rally", "electric"];
    MultiOpts {
        seed: 3,
        humans: (0..n)
            .map(|i| Human {
                car: CARS[i % CARS.len()],
                color: None,
            })
            .collect(),
        grid: (0..n).collect(),
        field: None,
        rubber_band: true,
        ghost: false,
    }
}

fn drive(st: &SimState, lr: &LevelRuntime, i: usize) -> InputFrame {
    let mut inp = Input::default();
    autopilot(&mut inp, &st.players[i].v, &lr.track);
    InputFrame::quantise(&inp)
}

/// Steps with nobody touching anything until GO.
fn to_go(lr: &LevelRuntime, st: &mut SimState) {
    let mut ev = Vec::new();
    while st.race.state == RaceStateKind::Countdown {
        step(lr, st, &[], &mut ev);
    }
}

/// Grid slot `k`: rows of two, ten metres apart, three metres stagger.
fn slot(lr: &LevelRuntime, k: usize) -> (f64, f64) {
    let t = &lr.track;
    let s = t.wrap(t.start_s - 5.0 - (k / 2) as f64 * 10.0 - (k % 2) as f64 * 3.0);
    (s, if k % 2 == 1 { 2.4 } else { -2.4 })
}

/// For every count of humans and every field setting: how many AI race,
/// and that the grid has humans (in grid order) from fourth place back
/// among five or more AI, from the front otherwise.
#[test]
fn the_grid_for_every_count_of_humans_and_ai() {
    let lr = lr("sierra");
    let level_rivals = lr.level.rivals.len();
    assert!(level_rivals >= 5, "sierra has a full field");
    for nh in 1..=MAX_FIELD {
        for field in [None, Some(0), Some(nh), Some(6), Some(8), Some(20)] {
            let mut o = opts(nh);
            o.field = field;
            // A grid order that is not the identity.
            o.grid = (0..nh).rev().collect();
            let st = SimState::new_multi(&lr, &o);
            let want_ai = field
                .map_or(level_rivals, |f| f.saturating_sub(nh))
                .min(MAX_FIELD - nh);
            let nai = st.rivals.len();
            assert_eq!(nai, want_ai, "{nh} humans, field {field:?}");
            assert!(nh + nai <= MAX_FIELD);
            assert_eq!(st.players.len(), nh);
            assert_eq!(st.race.prog_s.len(), nh + nai);

            let mut order: Vec<Result<usize, usize>> = Vec::new();
            if nai >= 5 {
                order.extend((0..3).map(Ok));
                order.extend(o.grid.iter().map(|&h| Err(h)));
                order.extend((3..nai).map(Ok));
            } else {
                order.extend(o.grid.iter().map(|&h| Err(h)));
                order.extend((0..nai).map(Ok));
            }
            for (k, c) in order.into_iter().enumerate() {
                let (s, lat) = slot(&lr, k);
                let (cs, clat, prog, what) = match c {
                    Err(h) => {
                        let v = &st.players[h].v;
                        (v.s, v.lat, v.prog, format!("human {h}"))
                    }
                    Ok(i) => {
                        let a = &st.rivals[i];
                        (a.k.s, a.k.lat, a.prog, format!("rival {i}"))
                    }
                };
                let ctx = format!("{nh} humans, field {field:?}: {what} in slot {k}");
                assert!((cs - s).abs() < 1e-6, "{ctx}: s {cs} != {s}");
                assert!((clat - lat).abs() < 1e-6, "{ctx}: lat {clat} != {lat}");
                assert!(prog.is_some(), "{ctx}: has a progress");
            }
            // Everyone starts held, at rest.
            for p in &st.players {
                assert!(p.phys.locked);
                assert_eq!((p.v.vx, p.v.vz), (0.0, 0.0));
            }
            assert_eq!(st.race.state, RaceStateKind::Countdown);
        }
    }
}

#[test]
fn humans_are_p1_to_p8_by_player_index_and_one_human_is_you() {
    let lr = lr("coast");
    let st = SimState::new_multi(&lr, &opts(1));
    assert_eq!(st.players[0].v.name, "You");
    for nh in 2..=MAX_FIELD {
        let mut o = opts(nh);
        o.grid = (0..nh).rev().collect();
        let st = SimState::new_multi(&lr, &o);
        for (i, p) in st.players.iter().enumerate() {
            assert_eq!(p.v.name, format!("P{}", i + 1));
            assert_eq!(p.v.kind, o.humans[i].car, "player {i} drives their car");
        }
    }
}

/// Filling past the level's own rivals reuses its cars (kind, skill,
/// colour) in turn, under new names, after the level's own in order.
#[test]
fn extra_rivals_reuse_the_levels_cars_under_new_names() {
    let lr = lr("sierra");
    let defs = &lr.level.rivals;
    let mut o = opts(1);
    o.field = Some(8);
    let st = SimState::new_multi(&lr, &o);
    assert_eq!(st.rivals.len(), 7);
    for (i, a) in st.rivals.iter().enumerate() {
        let d = &defs[i % defs.len()];
        assert_eq!(a.k.v.kind, d.kind, "rival {i}");
        assert_eq!(a.skill, d.skill, "rival {i}");
        assert_eq!(a.color, d.color, "rival {i}");
        if i < defs.len() {
            assert_eq!(a.name, d.name, "the level's own rival {i}");
        } else {
            assert!(defs.iter().all(|d| d.name != a.name), "{}", a.name);
        }
    }
    assert_eq!(st.rivals[defs.len()].name, "Nova");
}

/// The cruise has no rivals of its own, so there is nothing to fill with.
#[test]
fn a_level_without_rivals_stays_without_them() {
    let lr = lr("cruise");
    for field in [None, Some(6), Some(8)] {
        let mut o = opts(2);
        o.field = field;
        let st = SimState::new_multi(&lr, &o);
        assert!(st.rivals.is_empty(), "field {field:?}");
        assert_eq!(st.race.prog_s.len(), 2);
    }
}

#[test]
#[should_panic(expected = "1 to 8 humans")]
fn no_humans_is_refused() {
    let lr = lr("coast");
    SimState::new_multi(&lr, &opts(0));
}

#[test]
#[should_panic(expected = "1 to 8 humans")]
fn nine_humans_is_refused() {
    let lr = lr("coast");
    SimState::new_multi(&lr, &opts(MAX_FIELD + 1));
}

#[test]
fn a_grid_that_is_not_an_order_of_the_humans_is_refused() {
    let lr = lr("coast");
    for grid in [vec![0, 0], vec![0], vec![0, 1, 2], vec![1, 2]] {
        let mut o = opts(2);
        o.grid = grid.clone();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SimState::new_multi(&lr, &o);
        }));
        assert!(r.is_err(), "grid {grid:?} accepted");
    }
}

/// Places the one rival at `s0`, rolling, and human 1 at `h1` (human 0 is
/// left on the grid), then lets them run `secs`. Returns how far the rival
/// got and the final state.
fn rubber_band_run(rubber_band: bool, h1: Option<f64>, secs: f64) -> (f64, SimState) {
    let lr = lr("sierra");
    let t = lr.track.clone();
    let mut o = opts(2);
    o.field = Some(3);
    o.rubber_band = rubber_band;
    let mut st = SimState::new_multi(&lr, &o);
    assert_eq!(st.rivals.len(), 1);
    to_go(&lr, &mut st);
    let s0 = t.start_s + 600.0;
    {
        let a = &mut st.rivals[0];
        a.k.s = s0;
        a.k.lat = 0.0;
        a.k.speed = 25.0;
        a.prog = Some(s0);
        a.write_pos(&t);
    }
    if let Some(s) = h1 {
        let p = &mut st.players[1];
        p.phys.reset(&mut p.v, &t, s, 3.0);
        p.v.prog = Some(s);
    }
    let mut ev = Vec::new();
    for _ in 0..(secs * 120.0) as u32 {
        step(&lr, &mut st, &[], &mut ev);
    }
    (st.rivals[0].k.s - s0, st)
}

/// A rival bands to the human nearest it: one human sitting 70 m ahead of
/// it puts it in the neutral window, exactly as with banding off, though
/// the other human is 600 m back. With both humans far behind it eases off;
/// with the nearest far ahead it pushes.
#[test]
fn rivals_rubber_band_to_the_nearest_human() {
    let s0 = lr("sierra").track.start_s + 600.0;
    let (near, a) = rubber_band_run(true, Some(s0 + 70.0), 4.0);
    let (off, b) = rubber_band_run(false, Some(s0 + 70.0), 4.0);
    assert_eq!(a.rivals[0], b.rivals[0], "neutral gap: as with no banding");
    assert_eq!(near, off);
    assert!(near > 60.0, "the rival drove: {near}");

    let (behind, _) = rubber_band_run(true, None, 4.0);
    let (ahead, _) = rubber_band_run(true, Some(s0 + 300.0), 4.0);
    assert!(
        behind < near - 1.0,
        "both humans far behind: eases off ({behind:.2} vs {near:.2})"
    );
    assert!(
        ahead > near + 1.0,
        "the nearest human far ahead: pushes ({ahead:.2} vs {near:.2})"
    );
}

/// Two humans overlapping each other at one spot, a third overlapping the
/// rival 30 m on, one tick: how far apart across the road each pair ends
/// up (they start a metre apart side by side, overlapping, at rest).
fn pile_up(ghost: bool) -> (f64, f64) {
    let lr = lr("sierra");
    let t = lr.track.clone();
    let mut o = opts(3);
    o.field = Some(4);
    o.ghost = ghost;
    let mut st = SimState::new_multi(&lr, &o);
    assert_eq!(st.rivals.len(), 1);
    to_go(&lr, &mut st);
    let s0 = t.start_s + 300.0;
    for (i, s, lat) in [(0, s0, -0.5), (1, s0, 0.5), (2, s0 + 30.0, -0.5)] {
        let p = &mut st.players[i];
        p.phys.reset(&mut p.v, &t, s, lat);
    }
    {
        let a = &mut st.rivals[0];
        a.k.s = s0 + 30.0;
        a.k.lat = 0.5;
        a.k.speed = 0.0;
        a.write_pos(&t);
    }
    let mut ev = Vec::new();
    step(&lr, &mut st, &[], &mut ev);
    let (p0, p1, p2, r) = (
        &st.players[0].v,
        &st.players[1].v,
        &st.players[2].v,
        &st.rivals[0].k.v,
    );
    (
        (p1.x - p0.x).hypot(p1.z - p0.z),
        (r.x - p2.x).hypot(r.z - p2.z),
    )
}

/// Ghost mode only lets humans through each other: they are still pushed
/// apart from the AI.
#[test]
fn ghosts_still_collide_with_the_ai() {
    let (humans, rival) = pile_up(false);
    assert!(humans > 1.3, "solid humans are pushed apart: {humans}");
    assert!(rival > 1.3, "and from the rival: {rival}");
    let (humans, rival) = pile_up(true);
    assert!(humans < 1.05, "ghosts pass through each other: {humans}");
    assert!(rival > 1.3, "a ghost is pushed off the rival: {rival}");
}

/// Every human on the autopilot is not "every driving human finished": the
/// race goes on until one of them is home, then ends on that tick.
#[test]
fn a_race_of_only_autopiloted_humans_ends_when_the_first_is_home() {
    let lr = lr("sierra");
    let mut o = opts(2);
    o.field = Some(0);
    let mut st = SimState::new_multi(&lr, &o);
    let away = InputFrame {
        flags: AUTOPILOT,
        ..InputFrame::default()
    };
    let mut ev = Vec::new();
    for _ in 0..120 * 20 {
        step(&lr, &mut st, &[away, away], &mut ev);
    }
    assert_eq!(st.race.state, RaceStateKind::Racing, "nobody is home yet");
    assert!(st.players.iter().all(|p| !p.rules.finished));
    let mut finished_at = None;
    for k in 0..120 * 600 {
        let from = ev.len();
        step(&lr, &mut st, &[away, away], &mut ev);
        let new = &ev[from..];
        if finished_at.is_none() && new.iter().any(|e| matches!(e, SimEvent::Finished { .. })) {
            finished_at = Some(k);
            assert_eq!(st.race.state, RaceStateKind::Finished, "ended on that tick");
        }
        if new.contains(&SimEvent::Results) {
            let after = (k - finished_at.unwrap()) as f64 / 120.0;
            assert!((after - 3.2).abs() < 0.02, "results {after} s later");
            break;
        }
    }
    assert!(ev.contains(&SimEvent::Results));
    assert_eq!(
        ev.iter().filter(|e| **e == SimEvent::Results).count(),
        1,
        "the results come once"
    );
}

/// AWAY is for the screens only: the race is the same with it set.
#[test]
fn the_away_flag_changes_nothing_in_the_simulation() {
    let lr = lr("coast");
    let o = opts(2);
    let mut a = SimState::new_multi(&lr, &o);
    let mut b = SimState::new_multi(&lr, &o);
    let (mut ea, mut eb) = (Vec::new(), Vec::new());
    for k in 0..120 * 10 {
        let f0 = drive(&a, &lr, 0);
        let f1 = if k % 240 < 120 {
            drive(&a, &lr, 1)
        } else {
            InputFrame::default()
        };
        step(&lr, &mut a, &[f0, f1], &mut ea);
        let away = InputFrame {
            flags: f1.flags | AWAY,
            ..f1
        };
        step(&lr, &mut b, &[f0, away], &mut eb);
    }
    assert_eq!(hash(&a), hash(&b));
    assert_eq!(a, b);
    assert_eq!(ea, eb);
}

/// Each player has their own perfect start: on the throttle within the
/// last 0.75 s before GO (a lift resets the moment), not before.
#[test]
fn each_player_gets_their_own_perfect_start() {
    let lr = lr("sierra");
    let mut o = opts(4);
    o.field = Some(4);
    let mut st = SimState::new_multi(&lr, &o);
    let gas = InputFrame {
        throttle: 255,
        ..InputFrame::default()
    };
    let none = InputFrame::default();
    let mut ev = Vec::new();
    let mut k = 0;
    while st.race.state == RaceStateKind::Countdown {
        // Countdown 3.999 - k/120: below 0.75 from tick 390.
        let frames = [
            if k >= 400 { gas } else { none },                 // late: perfect
            gas,                                               // too early
            none,                                              // never
            if !(380..420).contains(&k) { gas } else { none }, // lifted, back late
        ];
        step(&lr, &mut st, &frames, &mut ev);
        k += 1;
    }
    let perfect: Vec<usize> = ev
        .iter()
        .filter_map(|e| match e {
            SimEvent::PerfectStart { player } => Some(*player),
            _ => None,
        })
        .collect();
    assert_eq!(perfect, vec![0, 3]);
    // The kick: six metres a second along the car.
    let sp = |i: usize| st.players[i].v.vx.hypot(st.players[i].v.vz);
    assert!(sp(0) > 5.0 && sp(3) > 5.0, "{} {}", sp(0), sp(3));
    assert!(sp(1) < 1.0 && sp(2) < 1.0, "{} {}", sp(1), sp(2));
}

/// Finishers first by time (equal times in player-then-rival order, a
/// stable sort), then everyone else by distance along the road; the results
/// estimate the unfinished from what they have left at 45 m/s.
#[test]
fn standings_and_results_put_finishers_by_time_then_the_rest_by_progress() {
    let lr = lr("sierra");
    let t = lr.track.clone();
    let mut o = opts(3);
    o.field = Some(6);
    let mut st = SimState::new_multi(&lr, &o);
    assert_eq!(st.rivals.len(), 3);
    let at = |f: f64| t.start_s + f * (t.finish_s - t.start_s);
    st.race.time = 200.0;
    st.race.state = RaceStateKind::Racing;
    // Rival 2 and player 2 finish in the same time; player 1 is fastest.
    for (i, time) in [(1, 150.0), (2, 170.0)] {
        let r = &mut st.players[i].rules;
        r.finished = true;
        r.finish_time = Some(time);
    }
    st.rivals[2].finished = true;
    st.rivals[2].finish_time = Some(170.0);
    // The rest, by distance: rival 0 ahead of player 0 ahead of rival 1.
    st.players[0].v.s = at(0.6);
    st.rivals[0].k.s = at(0.8);
    st.rivals[1].k.s = at(0.3);

    let order: Vec<(bool, Option<usize>, &str)> = standings(&st)
        .iter()
        .map(|r| (r.player, r.human, r.name))
        .collect();
    let rn = |i: usize| st.rivals[i].name;
    assert_eq!(
        order,
        vec![
            (true, Some(1), "P2"),
            (true, Some(2), "P3"),
            (false, None, rn(2)),
            (false, None, rn(0)),
            (true, Some(0), "P1"),
            (false, None, rn(1)),
        ]
    );

    let r = results(&st);
    assert_eq!(
        r.iter().map(|r| r.place).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6]
    );
    assert_eq!(
        r.iter().map(|r| r.estimated).collect::<Vec<_>>(),
        vec![false, false, false, true, true, true]
    );
    assert_eq!((r[0].time, r[1].time, r[2].time), (150.0, 170.0, 170.0));
    for w in r[3..].windows(2) {
        assert!(w[0].time <= w[1].time, "estimates in order: {r:?}");
    }
    let p0 = r.iter().find(|r| r.human == Some(0)).unwrap();
    let want = 200.0 + (st.race.finish_prog - at(0.6)) / 45.0;
    assert!((p0.time - want).abs() < 1e-9, "{} vs {want}", p0.time);
    for row in &r {
        assert_eq!(row.player, row.human.is_some());
    }
}

/// On a circuit the order is race progress (laps included), not where on
/// the loop a car is.
#[test]
fn circuit_standings_go_by_race_progress_not_position_on_the_loop() {
    let lr = lr("seaside");
    let t = lr.track.clone();
    assert!(st_laps(&lr) > 0, "seaside is a circuit");
    let mut o = opts(2);
    o.field = Some(2);
    let mut st = SimState::new_multi(&lr, &o);
    let n = t.n as f64;
    // Player 0 a lap up but further back round the loop.
    st.players[0].v.s = t.wrap(t.start_s + 100.0);
    st.players[0].v.prog = Some(t.start_s + n + 100.0);
    st.players[1].v.s = t.wrap(t.start_s + 900.0);
    st.players[1].v.prog = Some(t.start_s + 900.0);
    let s = standings(&st);
    assert_eq!(s[0].human, Some(0));
    assert_eq!(s[1].human, Some(1));
}

fn st_laps(lr: &LevelRuntime) -> u32 {
    SimState::new_multi(lr, &opts(1)).race.laps
}

/// Whatever the grid order, `human` is the player index: name, colour and
/// position all belong to that player.
#[test]
fn standing_rows_map_to_their_player_whatever_the_grid() {
    let lr = lr("coast");
    let mut o = opts(4);
    o.grid = vec![2, 0, 3, 1];
    o.humans[3].color = Some(0xabcdef);
    let st = SimState::new_multi(&lr, &o);
    let s = standings(&st);
    for row in s.iter().filter(|r| r.player) {
        let k = row.human.unwrap();
        let p = &st.players[k];
        assert_eq!(row.name, p.v.name);
        assert_eq!(row.color, p.spec.color);
        assert_eq!(row.s, p.v.s);
    }
    assert_eq!(
        s.iter().find(|r| r.human == Some(3)).unwrap().color,
        0xabcdef
    );
}
