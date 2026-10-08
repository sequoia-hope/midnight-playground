//! The call-log gate of WP 5.3–5.5 (roadmap M5; DECISIONS D41): each
//! scripted drive's facade log (`parity/golden/audio/drive-<name>.jsonl.gz`,
//! the game's calls on its `GameAudio`) is played back into the Rust
//! `GameAudio` on the null backend, under the reference's rules
//! (`tools/parity/lib/audio-facade.mjs` `replay`): a call in tick k at
//! k / 120 s, the timers due before it run first at their due times, every
//! promise settles after each call and each timer, `Math.random` is one
//! `mulberry32(1)`, the radio clips are read from `audio/radio/` and
//! "decoded" to the lengths in `radio-clips.json`. The Web Audio call log
//! that comes out must be the JS one: the digest in `calllog.json` (whole
//! log and per second), and, when the cache holds the JS log
//! (`node tools/parity/audio-ref.mjs calllog`), line for line, reporting the
//! first difference.
//!
//! `MP_AUDIO_CALLLOG_OUT=<dir>` writes the Rust logs there
//! (`calllog-<drive>.rust.jsonl`) for diffing.

#![cfg(not(target_arch = "wasm32"))]

use flate2::read::GzDecoder;
use mp_audio::game::{CarState, GameAudio, InitOptions, Platform, Rival, SirenUnit, Volume};
use mp_audio::radio::{Bytes, Fetch};
use mp_audio::wa::null::{self, ClipInfo, NullHandle, NullOptions};
use mp_audio::wa::{AudioContext, ContextOptions, ContextState, Pending};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden(name: &str) -> PathBuf {
    root().join("parity/golden/audio").join(name)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha(s: &[u8]) -> String {
    hex(&Sha256::digest(s))
}

/// `audio/radio/` from disk.
struct DiskFetch;

impl Fetch for DiskFetch {
    fn fetch(&self, file: &str) -> Pending<Bytes> {
        let p = Pending::new();
        let path = root().join("audio/radio").join(file);
        p.resolve(Ok(std::fs::read(path).ok().map(Rc::new)));
        p
    }
}

/// `radio-clips.json` by SHA-256.
fn clip_table() -> HashMap<String, ClipInfo> {
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(golden("radio-clips.json")).unwrap())
            .unwrap();
    v["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["sha256"].as_str().unwrap().to_owned(),
                ClipInfo {
                    name: c["file"].as_str().unwrap().to_owned(),
                    channels: c["channels"].as_u64().unwrap() as u32,
                    length: c["length"].as_u64().unwrap() as u32,
                },
            )
        })
        .collect()
}

fn f(v: &Value) -> Option<f64> {
    match v {
        Value::Null => None,
        Value::String(s) if s == "$undefined" => None,
        Value::String(s) => Some(match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            _ => panic!("not a number: {s}"),
        }),
        v => Some(v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))),
    }
}

fn b(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(x) => Some(*x),
        Value::Null => None,
        Value::String(s) if s == "$undefined" => None,
        v => panic!("not a boolean: {v}"),
    }
}

fn arg(args: &[Value], i: usize) -> &Value {
    args.get(i).unwrap_or(&Value::Null)
}

fn num_or(args: &[Value], i: usize, d: f64) -> f64 {
    f(arg(args, i)).unwrap_or(d)
}

/// A JS argument's truthiness (`up`, `final`, `on`).
fn truthy(args: &[Value], i: usize, d: bool) -> bool {
    match arg(args, i) {
        Value::Bool(x) => *x,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::Null => d,
        Value::String(s) if s == "$undefined" => d,
        v => panic!("not a boolean: {v}"),
    }
}

fn car_state(v: &Value) -> CarState {
    let mut s = CarState::default();
    for (k, x) in v.as_object().expect("a state object") {
        match k.as_str() {
            "rpm" => s.rpm = f(x),
            "rpmMax" => s.rpm_max = f(x),
            "throttle" => s.throttle = f(x),
            "gear" => s.gear = f(x),
            "speed" => s.speed = f(x),
            "skid" => s.skid = f(x),
            "nitro" => s.nitro = b(x),
            "onGround" => s.on_ground = b(x),
            "scrape" => s.scrape = f(x),
            "boost" => s.boost = f(x),
            "slip" => s.slip = f(x),
            "offroad" => s.offroad = f(x),
            "scrapeSide" => s.scrape_side = f(x),
            "motor" => s.motor = f(x),
            "power" => s.power = f(x),
            "regen" => s.regen = f(x),
            k => panic!("update: unknown field {k}"),
        }
    }
    s
}

fn rivals(v: &Value) -> Vec<Rival> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|r| Rival {
                    dist: f(&r["dist"]),
                    pan: f(&r["pan"]),
                    rpm_norm: f(&r["rpmNorm"]),
                    electric: b(&r["electric"]),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn sirens(v: &Value) -> Vec<SirenUnit> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|r| SirenUnit {
                    id: f(&r["id"]),
                    dist: f(&r["dist"]),
                    pan: f(&r["pan"]),
                    rel_speed: f(&r["relSpeed"]),
                    mode: r["mode"].as_str().map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

/// Call `method(...args)` on the audio, as the game did.
fn dispatch(a: &mut GameAudio, method: &str, args: &[Value]) {
    match method {
        "setPaused" => a.set_paused(truthy(args, 0, false)),
        "init" => a.init(InitOptions::default()),
        "unlock" => a.unlock(),
        "playTrack" => a.play_track(arg(args, 0).as_str().unwrap()),
        "nextTrack" => {
            a.next_track();
        }
        "setVolume" => {
            let v = arg(args, 0);
            a.set_volume(Volume {
                master: f(&v["master"]),
                sfx: f(&v["sfx"]),
                music: f(&v["music"]),
            })
        }
        "setMusic" => a.set_music(truthy(args, 0, false)),
        "setCar" => a.set_car(arg(args, 0).as_str().unwrap_or("sports")),
        "setEnvironment" => a.set_environment(
            arg(args, 0).as_str().unwrap_or("open"),
            truthy(args, 1, false),
        ),
        "radioVoice.prefetch" => {
            a.radio_voice.prefetch(&strings(arg(args, 0)));
        }
        "update" => a.update(num_or(args, 0, f64::NAN), &car_state(arg(args, 1))),
        "setRivalEngines" => a.set_rival_engines(&rivals(arg(args, 0))),
        "setSirens" => a.set_sirens(&sirens(arg(args, 0))),
        "setPursuitMood" => a.set_pursuit_mood(arg(args, 0).as_str().unwrap_or("off")),
        "setDamage" => a.set_damage(num_or(args, 0, 0.0)),
        "setSpikedTyres" => a.set_spiked_tyres(truthy(args, 0, false), num_or(args, 1, 0.0)),
        "beep" => a.beep(truthy(args, 0, false)),
        "nitroBurst" => a.nitro_burst(),
        "shift" => a.shift(truthy(args, 0, true)),
        "impact" => a.impact(num_or(args, 0, 0.5), num_or(args, 1, 0.0)),
        "landing" => a.landing(num_or(args, 0, 0.5)),
        "whoosh" => a.whoosh(num_or(args, 0, 0.0), num_or(args, 1, 0.5)),
        "uiClick" => a.ui_click(arg(args, 0).as_str().unwrap_or("click")),
        "finishFanfare" => a.finish_fanfare(),
        "radioLine" => a.radio_line(&strings(arg(args, 0)), num_or(args, 1, 0.0)),
        "radio" => a.radio(num_or(args, 0, 1.6), num_or(args, 1, 0.0), None),
        "sirenHorn" => a.siren_horn(num_or(args, 0, 0.0)),
        "takedown" => a.takedown(num_or(args, 0, 0.8), num_or(args, 1, 0.0)),
        "spikePop" => a.spike_pop(num_or(args, 0, 0.0)),
        "busted" => a.busted(),
        "escaped" => a.escaped(),
        "wrecked" => a.wrecked(),
        m => panic!("no facade method {m}"),
    }
}

struct Run {
    lines: Vec<String>,
    throws: Vec<String>,
    problems: Vec<String>,
}

fn replay(drive: &str) -> Run {
    let mut text = String::new();
    GzDecoder::new(std::fs::File::open(golden(&format!("drive-{drive}.jsonl.gz"))).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let clips = Rc::new(clip_table());
    let handle: Rc<RefCell<Option<(AudioContext, NullHandle)>>> = Rc::new(RefCell::new(None));
    let hc = handle.clone();
    let platform = Platform {
        new_context: Some(Box::new(move |opts: &ContextOptions| {
            let (ctx, h) = null::context(
                NullOptions {
                    sample_rate: 48000.0,
                    log: true,
                    state: ContextState::Suspended,
                },
                opts.clone(),
            );
            let clips = clips.clone();
            h.set_decoder(move |bytes| clips.get(&hex(&Sha256::digest(bytes))).cloned());
            *hc.borrow_mut() = Some((ctx.clone(), h));
            Some(ctx)
        })),
        audio_session: None,
        radio: Rc::new(DiskFetch),
        dj: None,
        random: Rc::new(RefCell::new(mp_math::Mulberry32::new(1))),
    };
    let mut a = GameAudio::new(platform);
    let mut out: Vec<String> = Vec::new();
    let mut now = 0.0f64;
    let take = |out: &mut Vec<String>| {
        if let Some((_, h)) = handle.borrow().as_ref() {
            out.extend(h.take_log());
        }
    };
    for line in text.lines().filter(|l| !l.is_empty()) {
        let v: Vec<Value> = serde_json::from_str(line).unwrap();
        let tick = v[0].as_f64().unwrap();
        let method = v[1].as_str().unwrap();
        let t = tick / 120.0;
        // advanceTo(t): the timers due by then, each at its due time.
        let h = handle.clone();
        a.run_due_timers(t, |due| {
            if due > now {
                now = due;
                if let Some((_, h)) = h.borrow().as_ref() {
                    h.set_now(due);
                }
            }
        });
        now = t;
        if let Some((_, h)) = handle.borrow().as_ref() {
            h.set_now(t);
        }
        take(&mut out);
        let rest = &line[line.find(',').unwrap() + 1..];
        out.push(format!("[{},\"api\",{rest}", null::fmt_num(now)));
        dispatch(&mut a, method, &v[2..]);
        a.settle();
    }
    take(&mut out);
    let (throws, problems) = match handle.borrow().as_ref() {
        Some((ctx, h)) => (
            h.throws(),
            ctx.problems().iter().map(|p| p.to_string()).collect(),
        ),
        None => (vec![], vec![]),
    };
    Run {
        lines: out,
        throws,
        problems,
    }
}

/// The cache entry holding the JS call logs the golden describes.
fn cache_dir() -> Option<PathBuf> {
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(golden("calllog.json")).unwrap()).unwrap();
    for d in std::fs::read_dir(root().join("parity/cache"))
        .ok()?
        .flatten()
    {
        let dir = d.path().join("audio");
        let ok = golden["drives"].as_array().unwrap().iter().all(|g| {
            let p = dir.join(format!("calllog-{}.jsonl", g["name"].as_str().unwrap()));
            std::fs::metadata(&p).is_ok_and(|m| m.len() == g["bytes"].as_u64().unwrap())
        });
        if ok {
            return Some(dir);
        }
    }
    None
}

fn check(drive: &str) {
    let t0 = std::time::Instant::now();
    let run = replay(drive);
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(golden("calllog.json")).unwrap()).unwrap();
    let g = golden["drives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == drive)
        .unwrap()
        .clone();
    let mut text = run.lines.join("\n");
    text.push('\n');
    if let Ok(dir) = std::env::var("MP_AUDIO_CALLLOG_OUT") {
        std::fs::write(
            Path::new(&dir).join(format!("calllog-{drive}.rust.jsonl")),
            &text,
        )
        .unwrap();
    }
    assert!(
        run.throws.is_empty(),
        "{drive}: browser exceptions {:?}",
        &run.throws[..run.throws.len().min(5)]
    );
    assert!(
        run.problems.is_empty(),
        "{drive}: problems {:?}",
        &run.problems[..run.problems.len().min(5)]
    );
    let ours = sha(text.as_bytes());
    if ours == g["sha256"].as_str().unwrap() {
        println!(
            "{drive}: {} lines, sha256 {} identical to the JS call log ({:.1} s)",
            run.lines.len(),
            &ours[..16],
            t0.elapsed().as_secs_f64()
        );
        return;
    }
    // Locate the first difference: line by line against the cached JS log,
    // else by the per-second digests.
    let mut msg = format!(
        "{drive}: the call log differs ({} lines here, {} in JS)",
        run.lines.len(),
        g["lines"]
    );
    if let Some(dir) = cache_dir() {
        let js = std::fs::read_to_string(dir.join(format!("calllog-{drive}.jsonl"))).unwrap();
        for (i, (a, b)) in run.lines.iter().zip(js.lines()).enumerate() {
            if a != b {
                let from = i.saturating_sub(6);
                msg += &format!("\nfirst difference at line {}:", i + 1);
                for (k, l) in js.lines().enumerate().skip(from).take(i - from) {
                    msg += &format!("\n  {:>7}      {l}", k + 1);
                }
                msg += &format!("\n  {:>7} Rust {a}\n  {:>7} JS   {b}", i + 1, i + 1);
                break;
            }
        }
    } else {
        let mut secs: Vec<(i64, Sha256, usize)> = Vec::new();
        for l in &run.lines {
            let t: f64 = l[1..l.find(',').unwrap()].parse().unwrap();
            let s = t.floor() as i64;
            if secs.last().is_none_or(|x| x.0 != s) {
                secs.push((s, Sha256::new(), 0));
            }
            let e = secs.last_mut().unwrap();
            e.1.update(l.as_bytes());
            e.1.update(b"\n");
            e.2 += 1;
        }
        for (s, h, n) in secs {
            let h = hex(&h.finalize())[..16].to_owned();
            let js = g["seconds"]
                .as_array()
                .unwrap()
                .iter()
                .find(|x| x["second"] == s);
            if js.is_none_or(|j| j["sha256"] != h.as_str()) {
                msg += &format!(
                    "\nfirst differing second: {s} ({n} lines here, {} in JS); run `node tools/parity/audio-ref.mjs calllog` for the full JS log",
                    js.map_or(Value::Null, |j| j["lines"].clone())
                );
                break;
            }
        }
    }
    panic!("{msg}");
}

#[test]
fn race_call_log_matches_js() {
    check("race");
}

#[test]
fn pursuit_call_log_matches_js() {
    check("pursuit");
}
