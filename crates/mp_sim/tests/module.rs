//! L2 gates of WP 1.3 onwards: the scenarios of the module oracle
//! (`parity/scenarios.md`) replayed in Rust, every tick's trace record hash
//! equal to the JS golden, and the full records it keeps byte for byte.
//! WP 1.3 (the proof that bit-identical works) is the `phys-*` rows; WP 1.4
//! adds the rivals, collisions and traffic.
//!
//! The scenarios are the catalogue in `tools/parity/sim-module.mjs`; each
//! input function here is a transcription of its JS one.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mp_math::{Mulberry32, js, kernel};
use mp_sim::autopilot::autopilot;
use mp_sim::input::{Input, quantise};
use mp_sim::staged::{Sim, StageOpts};
use mp_sim::trace::TraceFile;

fn idle() -> Input {
    Input::default()
}

fn auto(sim: &Sim) -> Input {
    let mut inp = idle();
    let p = &sim.players[0];
    autopilot(&mut inp, &p.v, &sim.track);
    inp
}

type InputFn = Box<dyn Fn(&Sim, u32) -> Input>;
type SetupFn = Box<dyn Fn(&mut Sim)>;

struct Scenario {
    id: String,
    ticks: u32,
    level: &'static str,
    car: &'static str,
    with_rivals: bool,
    traffic_count: usize,
    police_heat: Option<f64>,
    input: InputFn,
    setup: Option<SetupFn>,
    frame_dt: bool,
}

fn sc(id: &str, ticks: u32, level: &'static str, car: &'static str, input: InputFn) -> Scenario {
    Scenario {
        id: id.to_string(),
        ticks,
        level,
        car,
        with_rivals: false,
        traffic_count: 0,
        police_heat: None,
        input,
        setup: None,
        frame_dt: false,
    }
}

fn sec(s: f64) -> u32 {
    js::round(s * 120.0) as u32
}

const LEVEL_CARS: [(&str, &str); 6] = [
    ("sierra", "sports"),
    ("coast", "muscle"),
    ("streets", "super"),
    ("desert", "rally"),
    ("seaside", "electric"),
    ("cruise", "sports"),
];

fn phys_scenarios() -> Vec<Scenario> {
    let mut out = Vec::new();
    for car in ["sports", "muscle", "super", "rally", "electric"] {
        out.push(sc(
            &format!("phys-launch-{car}"),
            sec(20.0),
            "sierra",
            car,
            Box::new(|sim, k| {
                let a = auto(sim);
                let tt = k as f64 / 120.0;
                Input {
                    throttle: if tt < 12.0 { 1.0 } else { 0.0 },
                    brake: if (12.0..15.0).contains(&tt) { 1.0 } else { 0.0 },
                    nitro: false,
                    ..a
                }
            }),
        ));
    }
    for (lvl, car) in LEVEL_CARS {
        out.push(sc(
            &format!("phys-autopilot-{lvl}"),
            sec(90.0),
            lvl,
            car,
            Box::new(|sim, _| auto(sim)),
        ));
    }
    out.push(sc(
        "phys-handbrake",
        sec(40.0),
        "sierra",
        "sports",
        Box::new(|sim, k| {
            let mut a = auto(sim);
            if k % 480 >= 240 && k % 480 < 384 {
                a.handbrake = true;
                a.steer = if a.steer >= 0.0 { 1.0 } else { -1.0 };
            }
            a
        }),
    ));
    out.push(sc(
        "phys-wall",
        sec(30.0),
        "streets",
        "muscle",
        Box::new(|sim, k| {
            let mut a = auto(sim);
            if k % 360 < 120 {
                a.steer = js::min(1.0, a.steer + 0.6);
            }
            a
        }),
    ));
    out.push(sc(
        "phys-analog",
        sec(40.0),
        "coast",
        "super",
        Box::new(|sim, k| {
            let mut a = auto(sim);
            a.analog = true;
            a.steer = js::max(
                -1.0,
                js::min(
                    1.0,
                    a.steer * 0.8 + 0.2 * kernel::sin(3.0 * k as f64 / 120.0),
                ),
            );
            a
        }),
    ));
    out.push(sc(
        "phys-reverse",
        sec(10.0),
        "sierra",
        "sports",
        Box::new(|_, k| {
            if k < 480 {
                Input {
                    brake: 1.0,
                    steer: 0.5,
                    ..idle()
                }
            } else {
                Input {
                    throttle: 1.0,
                    ..idle()
                }
            }
        }),
    ));
    let mut s = sc(
        "phys-spiked-damaged",
        sec(30.0),
        "desert",
        "rally",
        Box::new(|sim, _| auto(sim)),
    );
    s.setup = Some(Box::new(|sim| {
        let p = &mut sim.players[0];
        p.phys.spiked = 10.0;
        p.phys.damage = 0.8;
    }));
    out.push(s);
    let mut s = sc(
        "phys-frame-dt",
        2400,
        "sierra",
        "sports",
        Box::new(|sim, _| auto(sim)),
    );
    s.frame_dt = true;
    out.push(s);
    out
}

const FRAME_DTS: [f64; 5] = [1.0 / 60.0, 1.0 / 144.0, 1.0 / 30.0, 0.025, 0.05];

/// Replays one scenario; returns the first tick whose hash differs.
fn replay(s: &Scenario) -> Option<u32> {
    let golden = common::golden(&s.id);
    assert_eq!(
        golden.hashes.len() as u32,
        s.ticks,
        "{}: golden length",
        s.id
    );
    let mut sim = Sim::new(
        common::stage(s.level),
        StageOpts {
            car: s.car,
            with_rivals: s.with_rivals,
            traffic_count: s.traffic_count,
            police_heat: s.police_heat,
        },
    );
    if let Some(f) = &s.setup {
        f(&mut sim);
    }
    let mut dt_rng = s.frame_dt.then(|| Mulberry32::new(7));
    let mut out = TraceFile::new(format!("{{\"id\":\"{}\",\"source\":\"rust\"}}", s.id));
    let mut first = None;
    for k in 0..s.ticks {
        let inp = quantise(&(s.input)(&sim, k));
        match &mut dt_rng {
            Some(r) => {
                let i = (r.next_f64() * FRAME_DTS.len() as f64).floor() as usize;
                sim.step_frame(FRAME_DTS[i], &inp);
            }
            None => sim.step(&inp),
        }
        let rec = sim.record(&inp);
        out.add(sim.tick, rec);
        if first.is_none() && out.hashes[k as usize] != golden.hashes[k as usize] {
            first = Some(sim.tick);
        }
        // Run on a little past the first difference, for the diff.
        if let Some(f) = first
            && sim.tick >= f + 240
        {
            break;
        }
    }
    if first.is_some() {
        common::save_trace(&s.id, &out.finish());
        return first;
    }
    // The full records the golden keeps, byte for byte.
    let full = out.full.clone();
    for (tick, bytes) in &golden.full {
        if let Some((_, mine)) = full.iter().find(|(t, _)| t == tick) {
            assert_eq!(mine, bytes, "{}: full record at tick {tick}", s.id);
        }
    }
    assert!(
        golden.full.len() >= (s.ticks / 120) as usize,
        "{}: the golden keeps its full records",
        s.id
    );
    None
}

fn ai_traffic_scenarios() -> Vec<Scenario> {
    let mut out = Vec::new();
    for (lvl, car) in LEVEL_CARS.iter().filter(|(l, _)| *l != "cruise") {
        let mut s = sc(
            &format!("ai-field-{lvl}"),
            sec(60.0),
            lvl,
            car,
            Box::new(|sim, _| auto(sim)),
        );
        s.with_rivals = true;
        out.push(s);
    }
    let mut s = sc(
        "collide-rear-pin",
        sec(15.0),
        "sierra",
        "sports",
        Box::new(|_, _| Input {
            brake: 1.0,
            ..idle()
        }),
    );
    s.with_rivals = true;
    s.setup = Some(Box::new(|sim| {
        let t = sim.track.clone();
        let f = t.frame(1500.0);
        let p = &mut sim.players[0];
        let lat = f.wall_r - p.v.half_w - 0.5;
        p.phys.reset(&mut p.v, &t, 1500.0, lat);
        let plat = p.v.lat;
        sim.ais.truncate(1);
        let a = &mut sim.ais[0];
        a.k.s = 1460.0;
        a.k.lat = plat;
        a.k.speed = 35.0;
        a.bias = 4.0;
        a.write_pos(&t);
    }));
    out.push(s);
    for (lvl, car) in LEVEL_CARS {
        let mut s = sc(
            &format!("traffic-{lvl}"),
            sec(60.0),
            lvl,
            car,
            Box::new(|sim, _| auto(sim)),
        );
        s.traffic_count = if lvl == "cruise" { 30 } else { 22 };
        out.push(s);
    }
    let mut s = sc(
        "full-field-sierra",
        sec(120.0),
        "sierra",
        "sports",
        Box::new(|sim, _| auto(sim)),
    );
    s.with_rivals = true;
    s.traffic_count = 22;
    out.push(s);
    out
}

fn pursuit_scenarios() -> Vec<Scenario> {
    let mut out = Vec::new();
    for (lvl, car) in [
        ("sierra", "sports"),
        ("coast", "rally"),
        ("streets", "muscle"),
        ("desert", "electric"),
    ] {
        let mut s = sc(
            &format!("pursuit-{lvl}"),
            sec(90.0),
            lvl,
            car,
            Box::new(|sim, _| auto(sim)),
        );
        s.with_rivals = true;
        s.traffic_count = 18;
        s.police_heat = Some(3.0);
        out.push(s);
    }
    let mut s = sc(
        "pursuit-heat5-props",
        sec(60.0),
        "sierra",
        "sports",
        Box::new(|sim, _| auto(sim)),
    );
    s.with_rivals = true;
    s.traffic_count = 18;
    s.police_heat = Some(5.0);
    s.setup = Some(Box::new(|sim| {
        let pu = sim.pu.as_mut().unwrap();
        pu.state = mp_sim::pursuit::State::Pursuit;
        pu.prop_t = 0.0;
        pu.spawn_t = 0.0;
    }));
    out.push(s);
    out
}

fn check_all(scenarios: Vec<Scenario>) {
    let mut bad = Vec::new();
    for s in scenarios {
        if let Some(tick) = replay(&s) {
            bad.push(format!("{} first differs at tick {tick}", s.id));
        }
    }
    assert!(
        bad.is_empty(),
        "{bad:#?}\n(the Rust traces are in target/parity/; diff one with\n  node tools/parity/trace-inspect.mjs target/parity/<id>.trace --diff <(gunzip -c parity/golden/sim/module/<id>.trace.gz))"
    );
}

#[test]
fn physics_module_traces_are_identical() {
    check_all(phys_scenarios());
}

#[test]
fn pursuit_module_traces_are_identical() {
    check_all(pursuit_scenarios());
}

#[test]
fn rival_collision_and_traffic_module_traces_are_identical() {
    check_all(ai_traffic_scenarios());
}
