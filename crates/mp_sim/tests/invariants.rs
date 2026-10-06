//! Determinism and invariants of `race::step` beyond the single-player runs
//! of `fuzz.rs`: the state hash notices a change to any field it should,
//! the seed matters, missing inputs are no input, a multiplayer state
//! cloned mid-race steps as the original, and over a two-human race on
//! every level nothing becomes non-finite, cars stay near the road, and a
//! circuit's laps are counted in order and add up to the finish time.

mod common;

use common::level;
use mp_math::Rng;
use mp_sim::autopilot::autopilot;
use mp_sim::input::{Input, InputFrame, RESET};
use mp_sim::race::{
    Human, LevelRuntime, MultiOpts, RaceOpts, RaceStateKind, SimEvent, SimState, hash, step,
};

fn lr(id: &str) -> LevelRuntime {
    LevelRuntime::new(level(id)).unwrap()
}

fn opts(n: usize, seed: u32) -> MultiOpts {
    MultiOpts {
        seed,
        humans: (0..n)
            .map(|i| Human {
                car: ["sports", "rally", "muscle"][i % 3],
                color: None,
            })
            .collect(),
        grid: (0..n).collect(),
        field: None,
        rubber_band: true,
        ghost: false,
    }
}

/// The autopilot for every player, with a reset when stuck.
fn frames(st: &SimState, lr: &LevelRuntime) -> Vec<InputFrame> {
    (0..st.players.len())
        .map(|i| {
            let mut inp = Input::default();
            autopilot(&mut inp, &st.players[i].v, &lr.track);
            let mut f = InputFrame::quantise(&inp);
            if st.players[i].rules.stuck.unwrap_or(0.0) > 2.0 {
                f.flags |= RESET;
            }
            f
        })
        .collect()
}

fn sp(lr: &LevelRuntime, seed: u32) -> SimState {
    SimState::new(
        lr,
        RaceOpts {
            car: "sports",
            seed,
            pursuit: false,
            heat: 1.0,
        },
    )
}

/// Each of these changes something a later tick depends on, so each must
/// change the hash a desync check compares.
#[test]
fn the_hash_notices_a_change_to_any_hashed_field() {
    let lr = lr("sierra");
    let mut base = SimState::new_multi(&lr, &opts(2, 5));
    let mut ev = Vec::new();
    for _ in 0..700 {
        let f = frames(&base, &lr);
        step(&lr, &mut base, &f, &mut ev);
    }
    assert!(!base.traffic.cars.is_empty() && !base.rivals.is_empty());
    let h0 = hash(&base);
    type Change = fn(&mut SimState);
    let changes: [(&str, Change); 17] = [
        ("tick", |s| s.tick += 1),
        ("race time", |s| s.race.time += 1e-9),
        ("race state", |s| s.race.state = RaceStateKind::Finished),
        ("player 0 x", |s| s.players[0].v.x += 1e-9),
        ("player 0 vz", |s| s.players[0].v.vz += 1e-9),
        ("player 0 rpm", |s| s.players[0].phys.rpm += 1.0),
        ("player 0 nitro", |s| s.players[0].phys.nitro += 0.01),
        ("player 0 score", |s| s.players[0].rules.score += 1.0),
        ("player 0 lap", |s| s.players[0].rules.lap += 1),
        ("player 1 yaw", |s| s.players[1].v.yaw += 1e-9),
        ("player 1 finished", |s| s.players[1].rules.finished = true),
        ("rival s", |s| s.rivals[0].k.s += 1e-9),
        ("rival nitro timer", |s| s.rivals[0].nitro_timer += 0.1),
        ("traffic car s", |s| s.traffic.cars[0].k.s += 1e-6),
        ("traffic car active", |s| {
            s.traffic.cars[0].active = !s.traffic.cars[0].active
        }),
        ("a draw from the ai stream", |s| {
            s.rng.ai.next_f64();
        }),
        ("a draw from the traffic stream", |s| {
            s.rng.traffic.next_f64();
        }),
    ];
    for (what, change) in changes {
        let mut st = base.clone();
        change(&mut st);
        assert_ne!(hash(&st), h0, "{what}");
    }
    assert_eq!(hash(&base.clone()), h0, "a clone hashes the same");
}

/// Multiplayer-only state that decides later ticks: the end countdown, a
/// second player's perfect-start moment and their near-miss bookkeeping.
/// The JS trace record has one player and no place for them, so the hash
/// adds them for a multiplayer race; otherwise a desync in them would go
/// unseen until it showed somewhere else.
#[test]
fn the_hash_notices_multiplayer_only_state() {
    let lr = lr("sierra");
    let mut base = SimState::new_multi(&lr, &opts(2, 5));
    let mut ev = Vec::new();
    for _ in 0..700 {
        let f = frames(&base, &lr);
        step(&lr, &mut base, &f, &mut ev);
    }
    let h0 = hash(&base);
    type Change = fn(&mut SimState);
    let changes: [(&str, Change); 4] = [
        ("end timer", |s| {
            s.race.multi.as_mut().unwrap().end_timer = Some(10.0)
        }),
        ("end delay", |s| {
            s.race.multi.as_mut().unwrap().end_delay = 1.0
        }),
        ("player 1 throttle_at", |s| {
            s.players[1].rules.throttle_at = Some(0.5)
        }),
        ("player 1 passed", |s| {
            s.players[1].rules.passed[0] = Some(123.0)
        }),
    ];
    let mut missed = Vec::new();
    for (what, change) in changes {
        let mut st = base.clone();
        change(&mut st);
        if hash(&st) == h0 {
            missed.push(what);
        }
    }
    assert!(missed.is_empty(), "the hash misses: {missed:?}");
}

#[test]
fn the_seed_decides_the_race() {
    let lr = lr("coast");
    assert_eq!(hash(&sp(&lr, 1)), hash(&sp(&lr, 1)));
    assert_ne!(
        hash(&sp(&lr, 1)),
        hash(&sp(&lr, 2)),
        "rival nitro timers from the seed"
    );
    let a = SimState::new_multi(&lr, &opts(2, 1));
    let b = SimState::new_multi(&lr, &opts(2, 2));
    assert_ne!(hash(&a), hash(&b));
}

/// `inputs[i]` missing is no input; extra inputs are ignored.
#[test]
fn missing_inputs_are_no_input_and_extra_ones_are_ignored() {
    let lr = lr("streets");
    let o = opts(3, 9);
    let mut full = SimState::new_multi(&lr, &o);
    let mut short = full.clone();
    let mut long = full.clone();
    let (mut e1, mut e2, mut e3) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..900 {
        let mut f = frames(&full, &lr);
        f[1] = InputFrame::default();
        f[2] = InputFrame::default();
        step(&lr, &mut full, &f, &mut e1);
        step(&lr, &mut short, &f[..1], &mut e2);
        let mut more = f.clone();
        more.push(InputFrame {
            throttle: 255,
            steer: 20000,
            ..InputFrame::default()
        });
        step(&lr, &mut long, &more, &mut e3);
    }
    assert_eq!(full, short);
    assert_eq!(full, long);
    assert_eq!(e1, e2);
    assert_eq!(e1, e3);
}

#[test]
fn a_multiplayer_state_cloned_mid_race_steps_identically() {
    let lr = lr("seaside");
    let mut a = SimState::new_multi(&lr, &opts(3, 4));
    let mut ev = Vec::new();
    for _ in 0..1200 {
        let f = frames(&a, &lr);
        step(&lr, &mut a, &f, &mut ev);
    }
    let mut b = a.clone();
    let (mut ea, mut eb) = (Vec::new(), Vec::new());
    for k in 0..1200 {
        let f = frames(&a, &lr);
        step(&lr, &mut a, &f, &mut ea);
        step(&lr, &mut b, &f, &mut eb);
        if k % 100 == 0 {
            assert_eq!(hash(&a), hash(&b), "tick {k}");
        }
    }
    assert_eq!(a, b);
    assert_eq!(ea, eb);
}

fn assert_finite(st: &SimState, id: &str) {
    let tick = st.tick;
    for (i, p) in st.players.iter().enumerate() {
        let v = &p.v;
        for (name, x) in [
            ("x", v.x),
            ("y", v.y),
            ("z", v.z),
            ("vx", v.vx),
            ("vy", v.vy),
            ("vz", v.vz),
            ("yaw", v.yaw),
            ("yaw_rate", v.yaw_rate),
            ("s", v.s),
            ("lat", v.lat),
            ("speed", v.speed),
            ("rpm", p.phys.rpm),
            ("nitro", p.phys.nitro),
            ("dist", p.rules.dist),
            ("score", p.rules.score),
        ] {
            assert!(x.is_finite(), "{id} tick {tick}: player {i} {name} = {x}");
        }
        if let Some(prog) = v.prog {
            assert!(prog.is_finite(), "{id} tick {tick}: player {i} prog");
        }
    }
    for (i, a) in st.rivals.iter().enumerate() {
        for (name, x) in [
            ("s", a.k.s),
            ("lat", a.k.lat),
            ("speed", a.k.speed),
            ("x", a.k.v.x),
            ("z", a.k.v.z),
            ("yaw", a.k.v.yaw),
        ] {
            assert!(x.is_finite(), "{id} tick {tick}: rival {i} {name} = {x}");
        }
    }
    for (i, c) in st.traffic.cars.iter().enumerate().filter(|(_, c)| c.active) {
        for (name, x) in [("s", c.k.s), ("lat", c.k.lat), ("speed", c.k.speed)] {
            assert!(x.is_finite(), "{id} tick {tick}: traffic {i} {name} = {x}");
        }
    }
    for x in [st.race.time, st.race.countdown] {
        assert!(x.is_finite(), "{id} tick {tick}: race clock");
    }
}

/// Two humans on the autopilot for 40 s of every level: everything stays
/// finite, every car stays on (or by) the road, the race clock runs from
/// GO only, and nobody's distance driven goes down.
#[test]
fn a_two_human_race_on_every_level_stays_sane() {
    for id in ["sierra", "coast", "streets", "desert", "seaside", "cruise"] {
        let lr = lr(id);
        let t = lr.track.clone();
        let mut st = SimState::new_multi(&lr, &opts(2, 11));
        let mut ev = Vec::new();
        let mut dist = [0.0; 2];
        let mut went = false;
        for _ in 0..120 * 40 {
            let f = frames(&st, &lr);
            let before = st.race.state;
            let time = st.race.time;
            step(&lr, &mut st, &f, &mut ev);
            if before == RaceStateKind::Countdown && st.race.state == RaceStateKind::Countdown {
                assert_eq!(st.race.time, 0.0, "{id}: the clock waits for GO");
            } else {
                assert!(st.race.time > time, "{id}: the clock runs");
            }
            went |= ev.contains(&SimEvent::Go);
            if st.tick.is_multiple_of(30) {
                assert_finite(&st, id);
                for (i, p) in st.players.iter().enumerate() {
                    assert!(p.rules.dist >= dist[i], "{id}: player {i} distance");
                    dist[i] = p.rules.dist;
                    let hw = t.frame(p.v.s).hw;
                    assert!(
                        p.v.lat.abs() < hw + 30.0,
                        "{id} tick {}: player {i} at lat {} (half width {hw})",
                        st.tick,
                        p.v.lat
                    );
                    assert!(p.v.s >= 0.0 && p.v.s <= t.length + 1e-6, "{id}: s in range");
                }
            }
        }
        assert!(went, "{id}: GO came");
        assert_eq!(
            ev.iter().filter(|e| **e == SimEvent::Go).count(),
            1,
            "{id}: one GO"
        );
        let countdown: Vec<i32> = ev
            .iter()
            .filter_map(|e| match e {
                SimEvent::Countdown(n) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(countdown, vec![3, 2, 1], "{id}: the countdown");
        for (i, d) in dist.iter().enumerate() {
            assert!(*d > 400.0, "{id}: player {i} drove only {d} m");
        }
    }
}

/// The circuit, raced to the end by two humans: each one's laps come in
/// order (2, then 3), the lap times add up to the finish time, and nobody
/// finishes before their progress reaches the finish.
#[test]
fn circuit_laps_are_counted_in_order_and_add_up_to_the_finish() {
    let lr = lr("seaside");
    let t = lr.track.clone();
    let mut o = opts(2, 2);
    o.field = Some(2);
    let mut st = SimState::new_multi(&lr, &o);
    let laps = st.race.laps;
    assert_eq!(laps, 3);
    assert_eq!(
        st.race.finish_prog,
        t.start_s + laps as f64 * t.n as f64,
        "three loops from the start"
    );
    let mut ev = Vec::new();
    for _ in 0..120 * 900 {
        let f = frames(&st, &lr);
        let from = ev.len();
        step(&lr, &mut st, &f, &mut ev);
        for e in &ev[from..] {
            if let SimEvent::Finished { player, .. } = e {
                let p = &st.players[*player];
                assert!(p.v.prog.unwrap() >= st.race.finish_prog);
            }
        }
        if ev[from..].contains(&SimEvent::Results) {
            break;
        }
    }
    assert!(ev.contains(&SimEvent::Results), "the race ended");
    for i in 0..2 {
        let r = &st.players[i].rules;
        let laps_seen: Vec<i32> = ev
            .iter()
            .filter_map(|e| match e {
                SimEvent::Lap { player, lap, .. } if *player == i => Some(*lap),
                _ => None,
            })
            .collect();
        if !r.finished {
            continue; // the window closed on them
        }
        assert_eq!(laps_seen, vec![2, 3], "player {i}'s laps");
        assert_eq!(r.lap_times.len(), 3, "player {i}: three lap times");
        let total: f64 = r.lap_times.iter().sum();
        let ft = r.finish_time.unwrap();
        assert!(
            (total - ft).abs() < 1e-6,
            "player {i}: laps {total} vs finish {ft}"
        );
        assert!(r.lap_times.iter().all(|&l| l > 10.0), "{:?}", r.lap_times);
    }
    assert!(
        st.players.iter().any(|p| p.rules.finished),
        "someone finished"
    );
}
