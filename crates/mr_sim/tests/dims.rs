//! `mr_sim::dims` against the JS game's models and bodies
//! (`parity/golden/sim/world-data.json`): every kind at every detail level,
//! and every body a race builds (half width and length as `Vehicle` takes
//! them).

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mr_sim::dims::{DIMS, Dims, dims};
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/sim/world-data.json");

fn same(kind: &str, d: &Dims, j: &Value) {
    assert_eq!(j["length"].as_f64().unwrap(), d.length, "{kind}: length");
    assert_eq!(j["width"].as_f64().unwrap(), d.width, "{kind}: width");
    assert_eq!(j["height"].as_f64().unwrap(), d.height, "{kind}: height");
    assert_eq!(
        j["wheelRadius"].as_f64().unwrap(),
        d.wheel_radius,
        "{kind}: wheelRadius"
    );
    assert_eq!(
        j["wheelBase"].as_f64().unwrap(),
        d.wheel_base,
        "{kind}: wheelBase"
    );
    assert_eq!(
        j.get("track").and_then(Value::as_f64),
        d.track,
        "{kind}: track"
    );
}

#[test]
fn every_kind_matches_car_model() {
    let g: Value = serde_json::from_str(GOLDEN).unwrap();
    let kinds = g["kinds"].as_object().unwrap();
    assert_eq!(kinds.len(), 13);
    for (kind, v) in kinds {
        let d = dims(kind).unwrap_or_else(|| panic!("{kind} is missing"));
        for lod in ["default", "high", "lowFar"] {
            same(kind, &d, &v[lod]);
        }
    }
    let names: Vec<&str> = DIMS.iter().map(|(k, _)| *k).take(13).collect();
    let mut want: Vec<&str> = kinds.keys().map(String::as_str).collect();
    want.sort();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(sorted, want, "the same kinds");
}

#[test]
fn every_body_a_race_builds_has_its_kinds_dimensions() {
    let g: Value = serde_json::from_str(GOLDEN).unwrap();
    for (level, l) in g["levels"].as_object().unwrap() {
        let b = &l["bodies"];
        let mut all = vec![&b["player"]];
        for grp in ["rivals", "traffic", "police", "sawhorses"] {
            all.extend(b[grp].as_array().unwrap());
        }
        for body in all {
            let kind = body["kind"].as_str().unwrap();
            let d = dims(kind).unwrap_or_else(|| panic!("{level}: {kind} is missing"));
            same(kind, &d, body);
            assert_eq!(
                body["halfW"].as_f64().unwrap(),
                d.width / 2.0,
                "{level}: {kind} halfW"
            );
            assert_eq!(
                body["halfL"].as_f64().unwrap(),
                d.length / 2.0,
                "{level}: {kind} halfL"
            );
        }
    }
}
