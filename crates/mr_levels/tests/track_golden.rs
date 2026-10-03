//! L1/L2 gate of WP 1.2: every Track array identical to the JS dump. The
//! goldens (`parity/golden/world/<level>.json`, WP 0.5, DECISIONS D28) hold
//! the SHA-256 of each typed array's bytes and the plain values, from the
//! live JS world with the parity kernel on.
//!
//! `sideL`/`sideR` are written by the scenery (world generation, M3) and
//! `runout`/`fenceGaps` are set by it; they are not part of a Track the
//! level alone builds.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use serde_json::Value;
use sha2::{Digest, Sha256};

fn golden(id: &str) -> Value {
    let text = match id {
        "sierra" => include_str!("../../../parity/golden/world/sierra.json"),
        "coast" => include_str!("../../../parity/golden/world/coast.json"),
        "streets" => include_str!("../../../parity/golden/world/streets.json"),
        "desert" => include_str!("../../../parity/golden/world/desert.json"),
        "seaside" => include_str!("../../../parity/golden/world/seaside.json"),
        "cruise" => include_str!("../../../parity/golden/world/cruise.json"),
        _ => unreachable!(),
    };
    serde_json::from_str(text).unwrap()
}

fn sha_f32(a: &[f32]) -> String {
    let mut h = Sha256::new();
    for v in a {
        h.update(v.to_le_bytes());
    }
    format!("{:x}", h.finalize())
}

fn sha_u8(a: &[u8]) -> String {
    format!("{:x}", Sha256::digest(a))
}

/// A number as JSON.stringify writes it: `null` when it is not finite.
fn num(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

#[test]
fn every_track_array_matches_the_js_dump() {
    let mut failures = Vec::new();
    for (level, t) in common::tracks() {
        let g = golden(level.id);
        let arrays = g["track"]["arrays"].as_object().unwrap();
        for (name, want) in arrays {
            let (len, sha) = match name.as_str() {
                "px" => (t.px.len(), sha_f32(&t.px)),
                "py" => (t.py.len(), sha_f32(&t.py)),
                "pz" => (t.pz.len(), sha_f32(&t.pz)),
                "kappa" => (t.kappa.len(), sha_f32(&t.kappa)),
                "kSmooth" => (t.k_smooth.len(), sha_f32(&t.k_smooth)),
                "grade" => (t.grade.len(), sha_f32(&t.grade)),
                "zone" => (t.zone.len(), sha_u8(&t.zone)),
                "roadType" => (t.road_type.len(), sha_u8(&t.road_type)),
                "fx" => (t.fx.len(), sha_f32(&t.fx)),
                "fz" => (t.fz.len(), sha_f32(&t.fz)),
                "rx" => (t.rx.len(), sha_f32(&t.rx)),
                "rz" => (t.rz.len(), sha_f32(&t.rz)),
                "hw" => (t.hw.len(), sha_f32(&t.hw)),
                "margin" => (t.margin.len(), sha_f32(&t.margin)),
                "bank" => (t.bank.len(), sha_f32(&t.bank)),
                "elevated" => (t.elevated.len(), sha_u8(&t.elevated)),
                "wallL" => (t.wall_l.len(), sha_f32(&t.wall_l)),
                "wallR" => (t.wall_r.len(), sha_f32(&t.wall_r)),
                "racingLine" => (t.racing_line.len(), sha_f32(&t.racing_line)),
                "speedProfile" => (t.speed_profile.len(), sha_f32(&t.speed_profile)),
                "runL" => {
                    let a = t.run_l.as_ref().expect("runL");
                    (a.len(), sha_f32(a))
                }
                "runR" => {
                    let a = t.run_r.as_ref().expect("runR");
                    (a.len(), sha_f32(a))
                }
                // Written by the scenery (M3).
                "sideL" | "sideR" => continue,
                other => {
                    failures.push(format!(
                        "{}: golden array {other} has no Rust counterpart",
                        level.id
                    ));
                    continue;
                }
            };
            if len as u64 != want["length"].as_u64().unwrap()
                || sha != want["sha256"].as_str().unwrap()
            {
                failures.push(format!("{}.{name}", level.id));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "arrays differ from the JS: {failures:?}"
    );
}

#[test]
fn track_scalars_match_the_js_dump() {
    for (level, t) in common::tracks() {
        let g = golden(level.id);
        let s = &g["track"]["scalars"];
        let id = level.id;
        assert_eq!(s["loop"], Value::Bool(t.is_loop), "{id}: loop");
        assert_eq!(s["n"].as_u64().unwrap() as usize, t.n, "{id}: n");
        assert_eq!(s["length"].as_f64().unwrap(), t.length, "{id}: length");
        assert_eq!(s["laps"].as_u64().unwrap() as u32, t.laps, "{id}: laps");
        assert_eq!(s["finishS"].as_f64(), num(t.finish_s), "{id}: finishS");
        assert_eq!(s["startS"].as_f64().unwrap(), t.start_s, "{id}: startS");
        assert_eq!(s["cell"].as_f64().unwrap(), t.cell, "{id}: cell");
        let zs: Vec<usize> = s["zoneStart"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(zs, t.zone_start, "{id}: zoneStart");
        for (gz, tz) in s["zones"].as_array().unwrap().iter().zip(&t.zones) {
            assert_eq!(gz["key"].as_str().unwrap(), tz.zone.key);
            assert_eq!(gz["s0"].as_f64().unwrap(), tz.s0, "{id}: zone s0");
            assert_eq!(gz["s1"].as_f64().unwrap(), tz.s1, "{id}: zone s1");
        }
        let b = &s["bounds"];
        assert_eq!(b["minX"].as_f64().unwrap(), t.bounds.min_x, "{id}: bounds");
        assert_eq!(b["maxX"].as_f64().unwrap(), t.bounds.max_x, "{id}: bounds");
        assert_eq!(b["minZ"].as_f64().unwrap(), t.bounds.min_z, "{id}: bounds");
        assert_eq!(b["maxZ"].as_f64().unwrap(), t.bounds.max_z, "{id}: bounds");
        let tags = s["tags"].as_array().unwrap();
        assert_eq!(tags.len(), t.tags.len(), "{id}: tag count");
        for (gt, tt) in tags.iter().zip(&t.tags) {
            assert_eq!(gt["tag"].as_str().unwrap(), tt.tag, "{id}: tag");
            assert_eq!(gt["s0"].as_f64().unwrap(), tt.s0, "{id}: {} s0", tt.tag);
            assert_eq!(gt["s1"].as_f64().unwrap(), tt.s1, "{id}: {} s1", tt.tag);
            assert_eq!(
                gt.get("turn").and_then(Value::as_f64),
                tt.turn,
                "{id}: {} turn",
                tt.tag
            );
        }
    }
}
