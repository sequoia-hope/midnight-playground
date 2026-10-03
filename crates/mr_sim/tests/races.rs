//! L2 gate of WP 1.5: whole races, the real `Race.update` recorded from the
//! game oracle (headless Chrome, every parity hook on, the autopilot
//! driving; `tools/parity/sim-race.mjs`), replayed by `mr_sim::race::step`.
//!
//! Always checked, from the committed summaries
//! (`parity/golden/sim/races.json`): the tick the results came, the hash of
//! that tick's record, and the results. When the recordings themselves are
//! in the cache (`parity/cache/<key>/sim-races/`, regenerated on demand),
//! every tick's hash is compared too, so a difference is found where it
//! starts.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use std::sync::Arc;

use mr_sim::autopilot::autopilot;
use mr_sim::input::{Input, InputFrame};
use mr_sim::race::{
    LevelRuntime, RaceOpts, SimState, cruise_results, pursuit_stats, results, step,
};
use mr_sim::trace::{TraceFile, fnv1a64, race_record, read_trace};
use serde_json::Value;

const SUMMARY: &str = include_str!("../../../parity/golden/sim/races.json");

/// A cached recording whose final hash is the committed one, if any.
fn cached(id: &str, final_hash: &str) -> Option<Vec<u64>> {
    let root = format!("{}/../../parity/cache", env!("CARGO_MANIFEST_DIR"));
    let dirs = std::fs::read_dir(&root).ok()?;
    for d in dirs.flatten() {
        let path = d.path().join("sim-races").join(format!("{id}.trace"));
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let tr = read_trace(&bytes).ok()?;
        if tr.hashes.last().map(|h| format!("{h:016x}")) == Some(final_hash.to_string()) {
            return Some(tr.hashes);
        }
    }
    None
}

fn run(id: &str, level: &str, car: &'static str, stop_ticks: Option<u32>) {
    run_heat(id, level, car, stop_ticks, None);
}

fn run_heat(id: &str, level: &str, car: &'static str, stop_ticks: Option<u32>, heat: Option<f64>) {
    let summary: Value = serde_json::from_str(SUMMARY).unwrap();
    let g = &summary["recordings"][id];
    let want_ticks = g["ticks"].as_u64().unwrap() as u32;
    let final_hash = g["finalHash"].as_str().unwrap();
    let per_tick = cached(id, final_hash);

    let lr = LevelRuntime::new(common::level(level)).unwrap();
    let t = Arc::clone(&lr.track);
    let mut st = SimState::new(
        &lr,
        RaceOpts {
            car,
            seed: 1,
            pursuit: heat.is_some(),
            heat: heat.unwrap_or(1.0),
        },
    );
    let mut events = Vec::new();
    let mut out = TraceFile::new(format!("{{\"id\":\"{id}\",\"source\":\"rust\"}}"));
    let mut first_diff = None;
    let last_hash = loop {
        let mut inp = Input::default();
        autopilot(&mut inp, &st.players[0].v, &t);
        let frame = InputFrame::quantise(&inp);
        step(&lr, &mut st, &[frame], &mut events);
        events.clear();
        let rec = race_record(&st, &[frame.input()]);
        let last_hash = fnv1a64(&rec);
        out.add(st.tick, rec);
        if let Some(h) = &per_tick
            && first_diff.is_none()
            && h.get(st.tick as usize - 1) != Some(&last_hash)
        {
            first_diff = Some(st.tick);
        }
        let done = match stop_ticks {
            Some(n) => st.tick >= n,
            None => st.players[0].rules.reported,
        };
        if done || st.tick >= 15 * 60 * 120 || first_diff.is_some_and(|f| st.tick > f + 240) {
            break last_hash;
        }
    };
    if let Some(f) = first_diff {
        common::save_trace(id, &out.finish());
        panic!(
            "{id}: first differs from the recording at tick {f} (Rust trace in target/parity/{id}.trace)"
        );
    }
    assert_eq!(
        st.tick, want_ticks,
        "{id}: the results came at tick {}, the JS at {want_ticks}",
        st.tick
    );
    assert_eq!(
        format!("{last_hash:016x}"),
        final_hash,
        "{id}: final record differs (no cached recording to localise it)"
    );

    // The results.
    if stop_ticks.is_some() {
        let r = cruise_results(&st);
        let w = &g["results"];
        assert_eq!(r.score, w["score"].as_f64().unwrap(), "{id}: score");
        assert_eq!(r.dist, w["dist"].as_f64().unwrap(), "{id}: dist");
        assert_eq!(r.top, w["top"].as_f64().unwrap(), "{id}: top");
        assert_eq!(r.time, w["time"].as_f64().unwrap(), "{id}: time");
        assert_eq!(
            r.near_misses as i64,
            w["nearMisses"].as_i64().unwrap(),
            "{id}: nearMisses"
        );
    } else {
        let res = results(&st);
        let w = g["results"].as_array().unwrap();
        assert_eq!(res.len(), w.len(), "{id}: results");
        for (r, w) in res.iter().zip(w) {
            assert_eq!(r.place as u64, w["place"].as_u64().unwrap(), "{id}: place");
            assert_eq!(
                r.name,
                w["name"].as_str().unwrap(),
                "{id}: name at {}",
                r.place
            );
            assert_eq!(
                r.time,
                w["time"].as_f64().unwrap(),
                "{id}: {}'s time",
                r.name
            );
            assert_eq!(
                r.estimated,
                w["estimated"].as_bool().unwrap(),
                "{id}: {} estimated",
                r.name
            );
        }
        if let Some(p) = g.get("pursuit") {
            let ps = pursuit_stats(&st).expect("a pursuit");
            assert_eq!(ps.busts as i64, p["busts"].as_i64().unwrap(), "{id}: busts");
            assert_eq!(
                ps.wrecks as i64,
                p["wrecks"].as_i64().unwrap(),
                "{id}: wrecks"
            );
            assert_eq!(
                ps.takedowns as i64,
                p["takedowns"].as_i64().unwrap(),
                "{id}: takedowns"
            );
            assert_eq!(ps.penalty, p["penalty"].as_f64().unwrap(), "{id}: penalty");
            assert_eq!(ps.heat as i64, p["heat"].as_i64().unwrap(), "{id}: heat");
        }
        if let Some(l) = g.get("laps") {
            let times: Vec<f64> = l["times"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            assert_eq!(st.players[0].rules.lap_times, times, "{id}: lap times");
        }
    }
    println!(
        "{id}: {} ticks identical{}",
        st.tick,
        if per_tick.is_some() {
            " (every tick)"
        } else {
            " (final record)"
        }
    );
}

#[test]
fn sierra_race() {
    run("sierra-race", "sierra", "sports", None);
}

#[test]
fn coast_race() {
    run("coast-race", "coast", "muscle", None);
}

#[test]
fn streets_race() {
    run("streets-race", "streets", "super", None);
}

#[test]
fn desert_race() {
    run("desert-race", "desert", "rally", None);
}

#[test]
fn seaside_race() {
    run("seaside-race", "seaside", "electric", None);
}

#[test]
fn cruise_three_minutes() {
    run("cruise-3min", "cruise", "sports", Some(3 * 60 * 120));
}

#[test]
fn sierra_pursuit() {
    run_heat("sierra-pursuit", "sierra", "sports", None, Some(1.0));
}

#[test]
fn coast_pursuit() {
    run_heat("coast-pursuit", "coast", "rally", None, Some(1.0));
}

#[test]
fn streets_pursuit() {
    run_heat("streets-pursuit", "streets", "muscle", None, Some(1.0));
}

#[test]
fn desert_pursuit() {
    run_heat("desert-pursuit", "desert", "electric", None, Some(1.0));
}

#[test]
fn sierra_pursuit_from_heat_5() {
    run_heat("sierra-pursuit-heat5", "sierra", "super", None, Some(5.0));
}
