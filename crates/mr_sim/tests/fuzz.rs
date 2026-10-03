//! SPEC 4.6 tests 4 and 5 (roadmap WP 1.7).
//!
//! Fuzz: random controls (`fuzzer(1)`) on every level for three minutes, in
//! race mode and, where the level has police, Hot Pursuit from heat 3, the
//! runs of `tools/parity/sim-fuzz.mjs`. Nothing becomes NaN, the state does
//! not grow, and no body gets further outside the walls than the JS game
//! does under the same fuzz (`parity/golden/sim/fuzz.json`). Since the race
//! is bit-identical, the excursions come out equal, at the same tick.
//!
//! Determinism: a state cloned mid-race and stepped beside the original
//! stays equal to it, and the same race run twice gives the same hashes.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use std::sync::Arc;

use mr_sim::autopilot::autopilot;
use mr_sim::fuzz::Fuzzer;
use mr_sim::input::{Input, InputFrame};
use mr_sim::race::{LevelRuntime, RaceOpts, SimState, hash, step};
use mr_track::Track;
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/sim/fuzz.json");

#[derive(Default)]
struct Max {
    e: Option<f64>,
    tick: u32,
    name: String,
}

fn note(m: &mut Max, t: &Track, tick: u32, name: &str, lat: f64, s: f64, half_w: f64) {
    let f = t.frame(s);
    let e = (lat + half_w - f.wall_r).max(-lat + half_w - f.wall_l);
    // `if (!(e <= (max ?? -Infinity)))`: a NaN would be kept too.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(e <= m.e.unwrap_or(f64::NEG_INFINITY)) {
        m.e = Some(e);
        m.tick = tick;
        m.name = name.to_string();
    }
}

/// A measure of everything the state holds that could grow.
fn size(st: &SimState) -> usize {
    let p = &st.players[0];
    let pv = st.pv.as_ref().map_or(0, |pv| {
        pv.pursuit.racers.len()
            + pv.pursuit.units.len()
            + pv.pursuit.block_cars.len()
            + pv.pursuit.sawhorses.len()
            + pv.pursuit.spots.len()
    });
    st.rivals.len()
        + st.traffic.cars.len()
        + p.rules.lap_times.len()
        + p.rules.passed.len()
        + p.phys.events.len()
        + pv
}

fn fuzz_run(id: &str, level: &str, heat: Option<f64>) {
    let g: Value = serde_json::from_str(GOLDEN).unwrap();
    let ticks = g["ticks"].as_u64().unwrap() as u32;
    let want = &g["runs"][id];
    let lr = LevelRuntime::new(common::level(level)).unwrap();
    let t = Arc::clone(&lr.track);
    let mut st = SimState::new(
        &lr,
        RaceOpts {
            car: "sports",
            seed: 1,
            pursuit: heat.is_some(),
            heat: heat.unwrap_or(1.0),
        },
    );
    let mut fz = Fuzzer::new(1);
    let mut events = Vec::new();
    let (mut player, mut rivals, mut traffic, mut police) = (
        Max::default(),
        Max::default(),
        Max::default(),
        Max::default(),
    );
    let size0 = size(&st);
    let mut size_max = size0;
    for _ in 0..ticks {
        let frame = InputFrame::quantise(&fz.next(Input::default()));
        step(&lr, &mut st, &[frame], &mut events);
        events.clear();
        let tick = st.tick;
        let v = &st.players[0].v;
        for (k, x) in [
            ("x", v.x),
            ("y", v.y),
            ("z", v.z),
            ("vx", v.vx),
            ("vz", v.vz),
            ("yaw", v.yaw),
            ("s", v.s),
            ("lat", v.lat),
        ] {
            assert!(x.is_finite(), "{id}: player.{k} = {x} at tick {tick}");
        }
        note(&mut player, &t, tick, "player", v.lat, v.s, v.half_w);
        for a in &st.rivals {
            note(&mut rivals, &t, tick, a.name, a.k.lat, a.k.s, a.k.v.half_w);
            assert!(
                a.k.s.is_finite() && a.k.lat.is_finite() && a.k.speed.is_finite(),
                "{id}: a body went non-finite"
            );
        }
        for (i, c) in st.traffic.cars.iter().enumerate() {
            if c.active && !c.opposite {
                note(
                    &mut traffic,
                    &t,
                    tick,
                    &format!("traffic {i} {}", c.kind_name),
                    c.k.lat,
                    c.k.s,
                    c.k.v.half_w,
                );
            }
            assert!(
                c.k.s.is_finite() && c.k.lat.is_finite() && c.k.speed.is_finite(),
                "{id}: a body went non-finite"
            );
        }
        if let Some(pv) = &st.pv {
            let pu = &pv.pursuit;
            for i in 0..pu.units.len() + pu.block_cars.len() {
                let u = pu.police(i);
                if u.active {
                    note(
                        &mut police,
                        &t,
                        tick,
                        &format!("police {i}"),
                        u.k.lat,
                        u.k.s,
                        u.k.v.half_w,
                    );
                }
                assert!(
                    u.k.s.is_finite() && u.k.lat.is_finite() && u.k.speed.is_finite(),
                    "{id}: a body went non-finite"
                );
            }
        }
        size_max = size_max.max(size(&st));
    }
    // The state does not grow (lap times on a circuit: at most the laps).
    assert!(
        size_max <= size0 + 4,
        "{id}: the state grew from {size0} to {size_max}"
    );
    for (cls, m) in [
        ("player", &player),
        ("rivals", &rivals),
        ("traffic", &traffic),
        ("police", &police),
    ] {
        let Some(js) = want["max"].get(cls).and_then(Value::as_f64) else {
            assert!(m.e.is_none(), "{id}: {cls} measured in Rust, not in the JS");
            continue;
        };
        let e = m.e.expect("measured");
        assert!(
            e <= js,
            "{id}: {cls} {e:.4} m outside the walls, the JS {js:.4} m"
        );
        // Bit-identical simulation: the same excursion at the same tick.
        assert_eq!(e, js, "{id}: {cls} excursion");
        assert_eq!(
            m.tick as u64,
            want["where"][cls]["tick"].as_u64().unwrap(),
            "{id}: {cls} tick"
        );
    }
}

#[test]
fn fuzz_race_levels() {
    for level in ["sierra", "coast", "streets", "desert", "seaside", "cruise"] {
        fuzz_run(&format!("{level}-race"), level, None);
    }
}

#[test]
fn fuzz_pursuit_levels() {
    for level in ["sierra", "coast", "streets", "desert"] {
        fuzz_run(&format!("{level}-pursuit"), level, Some(3.0));
    }
}

fn auto_frame(st: &SimState, t: &Track) -> InputFrame {
    let mut inp = Input::default();
    autopilot(&mut inp, &st.players[0].v, t);
    InputFrame::quantise(&inp)
}

/// A clone stepped beside the original stays equal to it, tick for tick.
#[test]
fn a_cloned_state_steps_identically() {
    let lr = LevelRuntime::new(common::level("sierra")).unwrap();
    let t = Arc::clone(&lr.track);
    let mut a = SimState::new(
        &lr,
        RaceOpts {
            car: "super",
            seed: 7,
            pursuit: true,
            heat: 4.0,
        },
    );
    let mut ev = Vec::new();
    for _ in 0..3000 {
        let f = auto_frame(&a, &t);
        step(&lr, &mut a, &[f], &mut ev);
    }
    let mut b = a.clone();
    assert_eq!(a, b);
    for _ in 0..3000 {
        let f = auto_frame(&a, &t);
        step(&lr, &mut a, &[f], &mut ev);
        step(&lr, &mut b, &[f], &mut ev);
        assert_eq!(hash(&a), hash(&b), "tick {}", a.tick);
    }
    assert_eq!(a, b, "the whole state, not only the hashed part");
}

/// The same race twice: the same hash at every tick (no hidden state, no
/// clock, no ambient randomness).
#[test]
fn the_same_race_twice_gives_the_same_hashes() {
    let run = || {
        let lr = LevelRuntime::new(common::level("coast")).unwrap();
        let t = Arc::clone(&lr.track);
        let mut st = SimState::new(
            &lr,
            RaceOpts {
                car: "muscle",
                seed: 3,
                pursuit: true,
                heat: 2.0,
            },
        );
        let mut ev = Vec::new();
        (0..4000)
            .map(|_| {
                let f = auto_frame(&st, &t);
                step(&lr, &mut st, &[f], &mut ev);
                hash(&st)
            })
            .collect::<Vec<u64>>()
    };
    assert_eq!(run(), run());
}
