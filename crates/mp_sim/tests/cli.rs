//! The `mp-sim` binary from the outside: bad arguments fail with a message
//! (and no subcommand prints the usage), and a short `race` run (autopilot
//! or `--fuzz`) prints the same final hash, and writes the same trace, as
//! the library stepped here with the same controls.

use std::process::{Command, Output};

use mp_levels::levels;
use mp_sim::autopilot::autopilot;
use mp_sim::fuzz::Fuzzer;
use mp_sim::input::{Input, InputFrame, quantise};
use mp_sim::race::{LevelRuntime, RaceOpts, SimState, hash, step};
use mp_sim::trace::{fnv1a64, race_record, read_trace};

/// Runs the binary from the repo root (where its default survey path is).
fn mp_sim(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mp-sim"))
        .args(args)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .expect("mp-sim runs")
}

fn fails_with(args: &[&str], msg: &str) {
    let out = mp_sim(args);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{args:?} succeeded");
    assert!(err.contains(msg), "{args:?}: stderr {err:?} lacks {msg:?}");
}

#[test]
fn bad_arguments_fail_with_a_message() {
    fails_with(&[], "usage:");
    fails_with(&["drive"], "usage:");
    fails_with(&["race", "--level", "atlantis"], "no level atlantis");
    fails_with(&["race", "--car", "tank"], "no car tank");
    fails_with(&["race", "--seed", "x"], "--seed:");
    fails_with(&["race", "--seed", "-1"], "--seed:");
    fails_with(&["race", "--ticks", "ten"], "--ticks:");
    fails_with(&["race", "--pursuit", "hot"], "--pursuit:");
    fails_with(&["race", "--fuzz", "1.5"], "--fuzz:");
    fails_with(&["race", "--state-at", "soon"], "--state-at:");
    fails_with(
        &[
            "race",
            "--level",
            "seaside",
            "--survey",
            "no/such/survey.bin",
        ],
        "no/such/survey.bin",
    );
    fails_with(&["bench", "--runs", "many"], "--runs:");
}

/// The final hash the binary prints for `race --level L --seed N --ticks T`.
fn printed_hash(out: &Output, level: &str, ticks: u32) -> u64 {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let first = stdout.lines().next().unwrap();
    let prefix = format!("{level}: {ticks} ticks (");
    assert!(first.starts_with(&prefix), "{first}");
    let hex = first.rsplit("final hash ").next().unwrap();
    u64::from_str_radix(hex.trim(), 16).unwrap()
}

fn runtime(id: &str) -> LevelRuntime {
    LevelRuntime::new(levels().into_iter().find(|l| l.id == id).unwrap()).unwrap()
}

#[test]
fn a_short_autopilot_race_matches_the_library() {
    let ticks = 600;
    let dir = env!("CARGO_TARGET_TMPDIR");
    let trace = format!("{dir}/mp-sim-cli-sierra.trace");
    let out = mp_sim(&[
        "race", "--level", "sierra", "--car", "muscle", "--seed", "4", "--ticks", "600", "--trace",
        &trace,
    ]);
    let h = printed_hash(&out, "sierra", ticks);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("(est.)"), "the results estimate: {stdout}");
    assert!(stdout.contains("<- you"), "{stdout}");

    let lr = runtime("sierra");
    let mut st = SimState::new(
        &lr,
        RaceOpts {
            car: "muscle",
            seed: 4,
            pursuit: false,
            heat: 1.0,
        },
    );
    let mut ev = Vec::new();
    let mut hashes = Vec::new();
    for _ in 0..ticks {
        let mut inp = Input::default();
        autopilot(&mut inp, &st.players[0].v, &lr.track);
        step(&lr, &mut st, &[InputFrame::quantise(&inp)], &mut ev);
        hashes.push(fnv1a64(&race_record(&st, &[quantise(&inp)])));
    }
    assert_eq!(h, hash(&st), "the printed hash");

    let tr = read_trace(&std::fs::read(&trace).unwrap()).unwrap();
    let _ = std::fs::remove_file(&trace);
    assert_eq!(tr.hashes, hashes, "the trace holds every tick's record");
    assert!(tr.meta.contains("\"level\":\"sierra\""), "{}", tr.meta);
    assert!(tr.meta.contains("\"seed\":4"), "{}", tr.meta);
}

#[test]
fn a_short_fuzzed_race_matches_the_library() {
    let ticks = 400;
    let out = mp_sim(&["race", "--level", "coast", "--fuzz", "3", "--ticks", "400"]);
    let h = printed_hash(&out, "coast", ticks);
    let lr = runtime("coast");
    let mut st = SimState::new(
        &lr,
        RaceOpts {
            car: "sports",
            seed: 1,
            pursuit: false,
            heat: 1.0,
        },
    );
    let mut fz = Fuzzer::new(3);
    let mut ev = Vec::new();
    for _ in 0..ticks {
        let inp = fz.next(Input::default());
        step(&lr, &mut st, &[InputFrame::quantise(&inp)], &mut ev);
    }
    assert_eq!(h, hash(&st));
}

/// The cruise prints its score line instead of a results table.
#[test]
fn a_cruise_prints_its_score() {
    let out = mp_sim(&["race", "--level", "cruise", "--ticks", "120"]);
    printed_hash(&out, "cruise", 120);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("near misses"), "{stdout}");
    assert!(!stdout.contains("<- you"), "{stdout}");
}
