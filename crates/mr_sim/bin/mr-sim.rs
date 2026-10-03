//! `mr-sim`: the simulation from the command line (roadmap WP 1.8). Runs a
//! race with the autopilot (or the fuzzer), prints the results, and can
//! write the trace or dump the state at a tick; `bench` measures ticks per
//! second for the full Sierra field (SPEC 4.6 test 6).
//!
//! It lives outside `src/` because it reads files and the clock, which the
//! simulation library must not (DECISIONS D62).
//!
//!   cargo run --release -p mr_sim --bin mr-sim -- race --level sierra --car sports
//!   cargo run --release -p mr_sim --bin mr-sim -- race --level coast --pursuit 3 --trace out.trace
//!   cargo run --release -p mr_sim --bin mr-sim -- race --level desert --fuzz 1 --ticks 21600
//!   cargo run --release -p mr_sim --bin mr-sim -- race --level sierra --state-at 6000
//!   cargo run --release -p mr_sim --bin mr-sim -- bench

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use mr_levels::{SeasideData, levels, seaside};
use mr_sim::autopilot::autopilot;
use mr_sim::fuzz::Fuzzer;
use mr_sim::input::{Input, InputFrame, quantise};
use mr_sim::race::{
    LevelRuntime, RaceOpts, SimState, cruise_results, hash, pursuit_stats, results, step,
};
use mr_sim::staged::{Sim, StageOpts, stage_level};
use mr_sim::trace::{TraceFile, race_record};
use mr_track::{Level, Mode};

const USAGE: &str = "usage:
  mr-sim race [--level ID] [--car KIND] [--seed N] [--pursuit HEAT] [--fuzz SEED]
              [--ticks N] [--trace FILE] [--state-at TICK] [--survey FILE]
  mr-sim bench [--runs N] [--ticks N]

race: runs until the results (or --ticks), driving with the autopilot
(or random controls from --fuzz), and prints the results, the tick count
and the final state hash. --trace writes the trace record of every tick
(parity/trace-format.md); --state-at prints the whole state after that
tick. Seaside needs its survey (default assets/seaside/survey.bin).";

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn level(id: &str, survey: &str) -> Result<Level, String> {
    let mut l = levels().into_iter().find(|l| l.id == id).ok_or_else(|| {
        format!("no level {id} (sierra, coast, streets, desert, seaside, cruise)")
    })?;
    if l.id == "seaside" {
        let bytes = std::fs::read(survey).map_err(|e| format!("{survey}: {e}"))?;
        seaside::prepare(&mut l, Arc::new(SeasideData::parse(&bytes)?));
    }
    Ok(l)
}

fn car_name(c: &str) -> Result<&'static str, String> {
    mr_sim::physics::CAR_SPECS
        .iter()
        .map(|(k, _)| *k)
        .find(|k| *k == c)
        .ok_or_else(|| format!("no car {c} (sports, muscle, super, rally, electric)"))
}

fn race(args: &[String]) -> Result<(), String> {
    let id = arg(args, "--level").unwrap_or_else(|| "sierra".into());
    let car = car_name(&arg(args, "--car").unwrap_or_else(|| "sports".into()))?;
    let seed: u32 = arg(args, "--seed")
        .map_or(Ok(1), |s| s.parse())
        .map_err(|e| format!("--seed: {e}"))?;
    let heat: Option<f64> = arg(args, "--pursuit")
        .map(|s| s.parse())
        .transpose()
        .map_err(|e| format!("--pursuit: {e}"))?;
    let fuzz: Option<u32> = arg(args, "--fuzz")
        .map(|s| s.parse())
        .transpose()
        .map_err(|e| format!("--fuzz: {e}"))?;
    let max_ticks: Option<u32> = arg(args, "--ticks")
        .map(|s| s.parse())
        .transpose()
        .map_err(|e| format!("--ticks: {e}"))?;
    let state_at: Option<u32> = arg(args, "--state-at")
        .map(|s| s.parse())
        .transpose()
        .map_err(|e| format!("--state-at: {e}"))?;
    let survey = arg(args, "--survey").unwrap_or_else(|| "assets/seaside/survey.bin".into());
    let trace_out = arg(args, "--trace");

    let lr = LevelRuntime::new(level(&id, &survey)?)?;
    let t = Arc::clone(&lr.track);
    let mut st = SimState::new(
        &lr,
        RaceOpts {
            car,
            seed,
            pursuit: heat.is_some(),
            heat: heat.unwrap_or(1.0),
        },
    );
    let mut fz = fuzz.map(Fuzzer::new);
    let mut trace = trace_out.as_ref().map(|_| {
        TraceFile::new(format!(
            "{{\"id\":\"{id}\",\"level\":\"{id}\",\"car\":\"{car}\",\"seed\":{seed},\"source\":\"rust\"}}"
        ))
    });
    let mut events = Vec::new();
    let cruise = lr.level.mode == Mode::Cruise;
    let limit = max_ticks.unwrap_or(if cruise { 3 * 60 * 120 } else { 15 * 60 * 120 });
    let t0 = Instant::now();
    loop {
        let inp = match &mut fz {
            Some(f) => f.next(Input::default()),
            None => {
                let mut inp = Input::default();
                autopilot(&mut inp, &st.players[0].v, &t);
                inp
            }
        };
        let frame = InputFrame::quantise(&inp);
        step(&lr, &mut st, &[frame], &mut events);
        events.clear();
        if let Some(tr) = &mut trace {
            tr.add(st.tick, race_record(&st, &[quantise(&inp)]));
        }
        if state_at == Some(st.tick) {
            println!("{st:#?}");
        }
        let done = if max_ticks.is_some() {
            false
        } else {
            st.players[0].rules.reported
        };
        if done || st.tick >= limit {
            break;
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    if let (Some(tr), Some(path)) = (trace, trace_out) {
        std::fs::write(&path, tr.finish()).map_err(|e| format!("{path}: {e}"))?;
        eprintln!("wrote {path}");
    }
    println!(
        "{id}: {} ticks ({:.1} s of race), final hash {:016x}",
        st.tick,
        st.race.time,
        hash(&st)
    );
    if cruise {
        let r = cruise_results(&st);
        println!(
            "score {} | {:.0} m | top {:.1} km/h | {} near misses",
            r.score,
            r.dist,
            r.top * 3.6,
            r.near_misses
        );
    } else {
        for r in results(&st) {
            println!(
                "{:>2}. {:<8} {:>9.3} s{}{}",
                r.place,
                r.name,
                r.time,
                if r.estimated { " (est.)" } else { "" },
                if r.player { "  <- you" } else { "" }
            );
        }
        if let Some(p) = pursuit_stats(&st) {
            println!(
                "pursuit: heat {} | {} takedowns | {} busts | {} wrecks | {:.1} s penalties",
                p.heat, p.takedowns, p.busts, p.wrecks, p.penalty
            );
        }
    }
    eprintln!("{:.0} ticks/s", st.tick as f64 / secs);
    Ok(())
}

/// The JS baseline's field (`tools/parity/sim-bench.mjs`): the player
/// (sports, autopilot), Sierra's five rivals and 22 traffic cars,
/// collisions; no Race rules, no trace. Median of `runs` runs.
fn bench(args: &[String]) -> Result<(), String> {
    let runs: usize = arg(args, "--runs")
        .map_or(Ok(5), |s| s.parse())
        .map_err(|e| format!("--runs: {e}"))?;
    let ticks: u32 = arg(args, "--ticks")
        .map_or(Ok(14_400), |s| s.parse())
        .map_err(|e| format!("--ticks: {e}"))?;
    let stage = stage_level(level("sierra", "")?);
    let mut rates = Vec::new();
    for _ in 0..runs {
        let mut sim = Sim::new(
            &stage,
            StageOpts {
                car: "sports",
                with_rivals: true,
                traffic_count: 22,
                police_heat: None,
            },
        );
        let t0 = Instant::now();
        for _ in 0..ticks {
            let mut inp = Input::default();
            autopilot(&mut inp, &sim.players[0].v, &sim.track);
            sim.step(&quantise(&inp));
        }
        rates.push(ticks as f64 / t0.elapsed().as_secs_f64());
    }
    rates.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = rates[rates.len() / 2];
    let load = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(str::to_owned));
    println!(
        "full Sierra field: {:.0} ticks/s (median of {runs} runs of {ticks}), best {:.0}{}",
        median,
        rates.last().unwrap(),
        load.map(|l| format!(", load average {l}"))
            .unwrap_or_default()
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("race") => race(&args[1..]),
        Some("bench") => bench(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mr-sim: {e}");
            ExitCode::FAILURE
        }
    }
}
