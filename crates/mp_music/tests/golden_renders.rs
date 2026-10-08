//! The engine against the lab's renders (`parity/golden/music/renders.json`,
//! written by `node tools/music-lab/test/golden.mjs`): a few tracks from a
//! given bar, as RMS per 2400 samples and whole 256-sample windows. The lab
//! uses `Math.*` and this crate the platform's `f64` functions, which may
//! differ in the last bit; the filters are stable, so the renders agree to
//! far below anything audible. A run that differs by much more is a port
//! bug: find the first differing window sample and work back.

use mp_music::engine::Engine;
use mp_music::genres::genre;
use serde_json::Value;
use std::path::PathBuf;

fn file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../parity/golden/music/renders.json")
}

/// Base64 of little-endian `f32`s.
fn b64(s: &str) -> Vec<f32> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("bad base64"),
        }
    };
    let s = s.trim_end_matches('=').as_bytes();
    let mut bytes = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c) << (18 - 6 * i);
        }
        let take = chunk.len() - 1;
        for i in 0..take {
            bytes.push((n >> (16 - 8 * i)) as u8);
        }
    }
    bytes
        .chunks(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

const RMS: usize = 2400;
/// RMS within 0.2 % relative or 2e-5 absolute, whichever is larger.
const RMS_REL: f64 = 0.002;
const RMS_ABS: f64 = 2e-5;
/// Window samples within 2e-4 of full scale.
const WIN_ABS: f64 = 2e-4;

/// The worst differences of one run: (relative RMS, where), (absolute
/// window sample, where).
struct Worst {
    rms: (f64, String),
    win: (f64, String),
    fail: Vec<String>,
}

fn check(run: &Value) -> Worst {
    let g = run["genre"].as_str().unwrap();
    let seed = run["seed"].as_u64().unwrap() as u32;
    let bar = run["bar"].as_u64().unwrap() as u32;
    let energy = run["energy"].as_f64().unwrap();
    let engine_seed = run["engineSeed"].as_u64().unwrap() as u32;
    let rate = run["rate"].as_f64().unwrap();
    let n = run["samples"].as_u64().unwrap() as usize;
    let name = format!("{g} {seed} bar {bar} energy {energy}");

    let t = (genre(g).unwrap_or_else(|| panic!("genre {g}")).make)(seed, None);
    let mut e = Engine::new(engine_seed, rate);
    e.set_track(&t);
    e.set_energy(energy);
    e.play(bar, 0.02);
    let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
    let mut i = 0;
    while i < n {
        e.process(&mut l[i..i + 128], &mut r[i..i + 128]);
        i += 128;
    }

    let mut w = Worst {
        rms: (0.0, String::new()),
        win: (0.0, String::new()),
        fail: Vec::new(),
    };
    for (side, x) in [("rmsL", &l), ("rmsR", &r)] {
        let want = b64(run[side].as_str().unwrap());
        assert_eq!(want.len(), n / RMS, "{name}: {side} blocks");
        for (k, &want) in want.iter().enumerate() {
            let s = &x[k * RMS..(k + 1) * RMS];
            let got = (s.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / RMS as f64).sqrt();
            let want = want as f64;
            let diff = (got - want).abs();
            let rel = diff / want.max(1e-12);
            let at = format!(
                "{name}: {side} block {k} ({:.2} s): {got} vs {want}",
                (k * RMS) as f64 / rate
            );
            if rel > w.rms.0 {
                w.rms = (rel, at.clone());
            }
            if diff > (RMS_REL * want).max(RMS_ABS) {
                w.fail.push(at);
            }
        }
    }
    for win in run["windows"].as_array().unwrap() {
        let start = win["start"].as_u64().unwrap() as usize;
        for (side, x) in [("l", &l), ("r", &r)] {
            let want = b64(win[side].as_str().unwrap());
            assert_eq!(want.len(), 256, "{name}: window length");
            for (k, &v) in want.iter().enumerate() {
                let got = x[start + k];
                let diff = (got as f64 - v as f64).abs();
                let at = format!(
                    "{name}: {side}[{}] ({:.4} s): {got} vs {v}",
                    start + k,
                    (start + k) as f64 / rate
                );
                if diff > w.win.0 {
                    w.win = (diff, at.clone());
                }
                if diff > WIN_ABS {
                    w.fail.push(at);
                }
            }
        }
    }
    w
}

#[test]
fn every_render_matches_the_lab() {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(file()).unwrap()).unwrap();
    let runs = doc.as_array().unwrap();
    assert_eq!(runs.len(), 14, "the lab's render runs");
    let mut worst_rms = (0.0, String::new());
    let mut worst_win = (0.0, String::new());
    let mut fails = Vec::new();
    for run in runs {
        let w = check(run);
        eprintln!(
            "{} {} bar {}: worst RMS {:.3e} rel, worst window {:.3e}{}",
            run["genre"].as_str().unwrap(),
            run["seed"],
            run["bar"],
            w.rms.0,
            w.win.0,
            if w.fail.is_empty() {
                String::new()
            } else {
                format!(" ({} out of tolerance, first: {})", w.fail.len(), w.fail[0])
            }
        );
        if w.rms.0 > worst_rms.0 {
            worst_rms = w.rms;
        }
        if w.win.0 > worst_win.0 {
            worst_win = w.win;
        }
        fails.extend(w.fail);
    }
    eprintln!("worst RMS: {} ({:.3e} relative)", worst_rms.1, worst_rms.0);
    eprintln!("worst window sample: {} ({:.3e})", worst_win.1, worst_win.0);
    assert!(
        fails.is_empty(),
        "{} values out of tolerance; first: {}\nworst RMS: {}\nworst window: {}",
        fails.len(),
        fails[0],
        worst_rms.1,
        worst_win.1
    );
}
