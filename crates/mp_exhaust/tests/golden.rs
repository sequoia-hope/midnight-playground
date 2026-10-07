//! The port against the engine lab (`parity/golden/exhaust/`, written by
//! `node tools/engine-lab/test/golden.mjs`): every preset through the lab
//! driver's rev script, and two through its drive cycle, with the driver's
//! states as the lab sent them. The lab uses `Math.*` and this crate the
//! platform's `f64` functions, which may differ in the last bit; the model's
//! filters are stable, so the renders agree to far below anything audible.

use mp_exhaust::{Engine, State, preset};
use serde_json::Value;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../parity/golden/exhaust")
}

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

fn check(file: &str) {
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(dir().join(file)).unwrap()).unwrap();
    let key = doc["preset"].as_str().unwrap();
    let rate = doc["rate"].as_f64().unwrap();
    let n = doc["samples"].as_u64().unwrap() as usize;
    let frames = b64(doc["frames"].as_str().unwrap());
    let p = preset(key).unwrap();
    let idle = p.idle;
    let mut e = Engine::new(
        p,
        State {
            rpm: idle,
            ..State::default()
        },
        doc["seed"].as_u64().unwrap() as u32,
        rate,
    );
    // The lab harness: 128-sample blocks, a state message at the first block
    // at or after each 60 Hz frame.
    let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
    let (mut next_frame, mut f) = (0.0, 0);
    let mut i = 0;
    while i < n {
        if i as f64 >= next_frame {
            let s = &frames[f * 4..f * 4 + 4];
            e.set_state(
                State {
                    rpm: s[0] as f64,
                    throttle: s[1] as f64,
                    boost: s[2] as f64,
                    speed: s[3] as f64,
                },
                false,
            );
            f += 1;
            next_frame += rate / 60.0;
        }
        e.process(&mut l[i..i + 128], &mut r[i..i + 128]);
        i += 128;
    }
    assert_eq!(f * 4, frames.len(), "{file}: every frame used");

    // Loudness in 50 ms blocks, to 0.1 %.
    let block = doc["rmsBlock"].as_u64().unwrap() as usize;
    for (side, x) in [("rmsL", &l), ("rmsR", &r)] {
        let want = b64(doc[side].as_str().unwrap());
        for (k, &w) in want.iter().enumerate() {
            let s = &x[k * block..(k + 1) * block];
            let got = (s.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / block as f64).sqrt();
            assert!(
                (got - w as f64).abs() <= 1e-3 * (w as f64) + 1e-6,
                "{file}: {side} block {k} ({:.2} s): {got} vs {w}",
                (k * block) as f64 / rate
            );
        }
    }
    // Sample for sample in the windows, to -100 dB of full scale.
    for w in doc["windows"].as_array().unwrap() {
        let start = w["start"].as_u64().unwrap() as usize;
        for (side, x) in [("l", &l), ("r", &r)] {
            let want = b64(w[side].as_str().unwrap());
            for (k, &v) in want.iter().enumerate() {
                let got = x[start + k];
                assert!(
                    (got - v).abs() <= 1e-5,
                    "{file}: {side}[{}] ({:.4} s): {got} vs {v}",
                    start + k,
                    (start + k) as f64 / rate
                );
            }
        }
    }
}

#[test]
fn every_golden_matches_the_lab() {
    let mut files: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|f| f.ends_with(".json"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 10, "{files:?}");
    for f in &files {
        check(f);
    }
}
