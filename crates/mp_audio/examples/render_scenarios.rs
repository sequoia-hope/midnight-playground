//! The L4 renders of the audio reference (SPEC 7.5), on the native backend:
//! every scenario of `parity/golden/audio/renders.json` (the table
//! `tools/parity/lib/audio-scenarios.mjs` makes) rendered the way
//! `tools/parity/audio-ref.html` renders it in Chrome: a 48 kHz stereo
//! offline context with a whole `GameAudio` on it, steered every 384 samples
//! (8 ms) through `suspend`. Writes `<out>/<id>.wav` (32-bit float).
//! `tools/parity/audio-bands.mjs` runs this and compares the band levels.
//!
//!   cargo run --release -p mp_audio --features native --example render_scenarios -- <out> [--scenarios <file.json>] [id-prefix ...]

use mp_audio::game::{CarState, GameAudio, InitOptions, Platform, Rival, SirenUnit, Volume};
use mp_audio::wa::native;
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const SR: f64 = 48000.0;
const FRAME: usize = 384; // control frame: 8 ms, three render quanta
const PUMP: usize = 64; // music pump every 64 frames (0.512 s), a second ahead

fn f(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn car_state(m: &Map<String, Value>) -> CarState {
    let b = |k: &str| m.get(k).and_then(Value::as_bool);
    let n = |k: &str| m.get(k).and_then(Value::as_f64);
    CarState {
        rpm: n("rpm"),
        rpm_max: n("rpmMax"),
        throttle: n("throttle"),
        speed: n("speed"),
        on_ground: b("onGround"),
        gear: n("gear"),
        boost: n("boost"),
        skid: n("skid"),
        slip: n("slip"),
        offroad: n("offroad"),
        scrape: n("scrape"),
        scrape_side: n("scrapeSide"),
        nitro: b("nitro"),
        motor: n("motor"),
        power: n("power"),
        regen: n("regen"),
    }
}

fn call(a: &mut GameAudio, call: &[Value]) {
    let m = call[0].as_str().expect("a method");
    let num = |i: usize, d: f64| call.get(i).and_then(f).unwrap_or(d);
    let flag = |i: usize, d: bool| call.get(i).and_then(Value::as_bool).unwrap_or(d);
    let s = |i: usize, d: &'static str| -> String {
        call.get(i).and_then(Value::as_str).unwrap_or(d).to_owned()
    };
    match m {
        "impact" => a.impact(num(1, 0.5), num(2, 0.0)),
        "landing" => a.landing(num(1, 0.5)),
        "beep" => a.beep(flag(1, false)),
        "whoosh" => a.whoosh(num(1, 0.0), num(2, 0.5)),
        "nitroBurst" => a.nitro_burst(),
        "uiClick" => a.ui_click(&s(1, "click")),
        "finishFanfare" => a.finish_fanfare(),
        "sirenHorn" => a.siren_horn(num(1, 0.0)),
        "busted" => a.busted(),
        "escaped" => a.escaped(),
        "takedown" => a.takedown(num(1, 0.8), num(2, 0.0)),
        "spikePop" => a.spike_pop(num(1, 0.0)),
        "wrecked" => a.wrecked(),
        "shift" => a.shift(flag(1, true)),
        "radio" => a.radio(num(1, 1.6), num(2, 0.0), None),
        m => panic!("scenario event {m}"),
    }
}

fn rivals(v: &Value) -> Vec<Rival> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|r| Rival {
            dist: f(&r["dist"]),
            pan: f(&r["pan"]),
            rpm_norm: f(&r["rpmNorm"]),
            electric: r["electric"].as_bool(),
        })
        .collect()
}

fn sirens(v: &Value) -> Vec<SirenUnit> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|r| SirenUnit {
            id: f(&r["id"]),
            dist: f(&r["dist"]),
            pan: f(&r["pan"]),
            rel_speed: f(&r["relSpeed"]),
            mode: r["mode"].as_str().map(str::to_owned),
        })
        .collect()
}

/// `window.renderScenario(sc)`.
fn render(sc: &Value) -> Vec<Vec<f32>> {
    let secs = sc["secs"].as_f64().unwrap();
    let frames = mp_math::js::round((secs * SR) / FRAME as f64) as usize * FRAME;
    let ctx = native::offline_context(2, frames, SR as f32);
    let seed = sc["seed"].as_u64().unwrap_or(1) as u32;
    let a = Rc::new(RefCell::new(GameAudio::new(Platform::headless(seed))));
    {
        let mut a = a.borrow_mut();
        a.init(InitOptions {
            context: Some(ctx.clone()),
            latency_hint: None,
        });
        let v = &sc["volume"];
        a.set_volume(if v.is_object() {
            Volume {
                master: f(&v["master"]),
                sfx: f(&v["sfx"]),
                music: f(&v["music"]),
            }
        } else {
            Volume {
                master: Some(1.0),
                sfx: Some(0.85),
                music: Some(0.0),
            }
        });
        a.set_car(sc["car"].as_str().unwrap_or("sports"));
        if let Some(env) = sc["env"].as_str() {
            a.set_environment(env, true);
        }
        if let Some(track) = sc["track"].as_str() {
            a.music_play(track);
            a.set_music(true);
        }
    }
    let dt = FRAME as f64 / SR;
    let mut state: Option<Map<String, Value>> = sc["state"].as_object().cloned();
    let mut events: Vec<(f64, Vec<Value>, bool)> = sc["events"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            let e = e.as_array().unwrap();
            (e[0].as_f64().unwrap(), e[1..].to_vec(), false)
        })
        .collect();
    let mut changes: Vec<(f64, Map<String, Value>, bool)> = sc["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let c = c.as_array().unwrap();
            (
                c[0].as_f64().unwrap(),
                c[1].as_object().unwrap().clone(),
                false,
            )
        })
        .collect();
    let rv = (!sc["rivals"].is_null()).then(|| rivals(&sc["rivals"]));
    let sv = (!sc["sirens"].is_null()).then(|| sirens(&sc["sirens"]));
    let damage = f(&sc["damage"]);
    let spiked = f(&sc["spiked"]);
    let mood = sc["mood"].as_str().map(str::to_owned);
    let track = sc["track"].is_string();
    let aa = a.clone();
    let mut control = move |k: usize| {
        let mut a = aa.borrow_mut();
        let t = k as f64 * dt;
        for c in changes.iter_mut() {
            if !c.2 && c.0 <= t {
                if let Some(s) = state.as_mut() {
                    for (key, v) in &c.1 {
                        s.insert(key.clone(), v.clone());
                    }
                }
                c.2 = true;
            }
        }
        for e in events.iter_mut() {
            if !e.2 && e.0 <= t {
                call(&mut a, &e.1);
                e.2 = true;
            }
        }
        if let Some(s) = &state {
            a.update(dt, &car_state(s));
        }
        if let Some(r) = &rv {
            a.set_rival_engines(r);
        }
        if let Some(s) = &sv {
            a.set_sirens(s);
        }
        if let Some(d) = damage {
            a.set_damage(d);
        }
        if let Some(sp) = spiked {
            a.set_spiked_tyres(true, sp);
        }
        if let Some(m) = &mood {
            a.set_pursuit_mood(m);
        }
        if track && k.is_multiple_of(PUMP) {
            a.music_pump_until(t + 1.0);
        }
    };
    control(0);
    let out = ctx
        .start_rendering_steered(FRAME, Box::new(control))
        .expect("an offline native context");
    let problems = ctx.problems();
    assert!(
        problems.is_empty(),
        "{}: {:?}",
        sc["id"],
        &problems[..problems.len().min(5)]
    );
    out
}

fn write_wav(path: &Path, ch: &[Vec<f32>]) {
    let n = ch[0].len();
    let nc = ch.len();
    let mut b: Vec<u8> = Vec::with_capacity(44 + n * nc * 4);
    let u32le = |b: &mut Vec<u8>, x: u32| b.extend_from_slice(&x.to_le_bytes());
    let u16le = |b: &mut Vec<u8>, x: u16| b.extend_from_slice(&x.to_le_bytes());
    b.extend_from_slice(b"RIFF");
    u32le(&mut b, (36 + n * nc * 4) as u32);
    b.extend_from_slice(b"WAVEfmt ");
    u32le(&mut b, 16);
    u16le(&mut b, 3);
    u16le(&mut b, nc as u16);
    u32le(&mut b, SR as u32);
    u32le(&mut b, (SR as usize * nc * 4) as u32);
    u16le(&mut b, (nc * 4) as u16);
    u16le(&mut b, 32);
    b.extend_from_slice(b"data");
    u32le(&mut b, (n * nc * 4) as u32);
    for i in 0..n {
        for c in ch {
            b.extend_from_slice(&c[i].to_le_bytes());
        }
    }
    std::fs::write(path, b).unwrap();
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // `--scenarios <file>`: a JSON list of scenarios instead of the golden's.
    let file = args.iter().position(|a| a == "--scenarios").map(|i| {
        let f = args[i + 1].clone();
        args.drain(i..i + 2);
        f
    });
    let out = PathBuf::from(
        args.first()
            .expect("usage: render_scenarios <out> [--scenarios <file.json>] [id-prefix ...]"),
    );
    let prefixes = &args[1..];
    std::fs::create_dir_all(&out).unwrap();
    let scenarios: Vec<Value> = match file {
        Some(f) => serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap(),
        None => {
            let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../parity/golden/audio/renders.json");
            let v: Value = serde_json::from_str(&std::fs::read_to_string(golden).unwrap()).unwrap();
            v["renders"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["scenario"].clone())
                .collect()
        }
    };
    for sc in &scenarios {
        let id = sc["id"].as_str().unwrap();
        if !prefixes.is_empty() && !prefixes.iter().any(|p| id.starts_with(p.as_str())) {
            continue;
        }
        let t0 = std::time::Instant::now();
        let ch = render(sc);
        write_wav(&out.join(format!("{id}.wav")), &ch);
        println!("{id} {:.2}s", t0.elapsed().as_secs_f64());
    }
}
