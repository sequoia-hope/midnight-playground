//! Roadmap WP 5.2's gate (SPEC 7.5, L2): every array of the audio reference
//! (`parity/golden/audio/arrays.json`) reproduced by the port: the same
//! names, kinds, channels, lengths, sample rate and options, and the same
//! hash, so bit-identical. The arrays up to 16384 floats are also compared
//! value by value with `arrays-small.bin`, and, when the cache holds the
//! whole set (`parity/cache/<key>/audio/arrays.bin`, rebuilt by
//! `node tools/parity/audio-ref.mjs arrays`), every array is (native only),
//! to name the first differing sample if a hash ever differs.
//!
//! The golden is compiled in, so this runs in wasm too.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mr_audio::array_hash;
use mr_audio::reference::{self, Kind, RefArray};
use mr_math::Mulberry32;
use serde_json::Value;

const ARRAYS_JSON: &str = include_str!("../../../parity/golden/audio/arrays.json");
const ARRAYS_SMALL: &[u8] = include_bytes!("../../../parity/golden/audio/arrays-small.bin");

fn floats(bytes: &[u8], off: usize, len: usize) -> Vec<f32> {
    bytes[off * 4..(off + len) * 4]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

/// The first differing sample, as a message.
fn first_difference(name: &str, part: usize, ours: &[f32], js: &[f32]) -> Option<String> {
    let mut max = 0f64;
    let mut first = None;
    for (i, (a, b)) in ours.iter().zip(js).enumerate() {
        if a.to_bits() != b.to_bits() {
            first.get_or_insert(i);
            max = max.max((*a as f64 - *b as f64).abs());
        }
    }
    first.map(|i| {
        format!(
            "{name}[{part}]: first difference at {i}: {} vs JS {} (largest {max:e})",
            ours[i], js[i]
        )
    })
}

/// The cached `arrays.bin` of the current reference, if one is there.
#[cfg(not(target_arch = "wasm32"))]
fn cached_full() -> Option<Vec<u8>> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../parity/cache");
    let golden: Value = serde_json::from_str(ARRAYS_JSON).unwrap();
    let total: usize = golden["arrays"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["length"].as_u64().unwrap() as usize * a["hashes"].as_array().unwrap().len())
        .sum();
    for d in std::fs::read_dir(dir).ok()?.flatten() {
        let p = d.path().join("audio/arrays.bin");
        if let Ok(b) = std::fs::read(&p)
            && b.len() == total * 4
        {
            return Some(b);
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn cached_full() -> Option<Vec<u8>> {
    None
}

#[test]
fn every_reference_array_is_bit_identical() {
    let golden: Value = serde_json::from_str(ARRAYS_JSON).unwrap();
    assert_eq!(golden["seed"], 1);
    let sr = golden["sampleRate"].as_f64().unwrap();
    assert_eq!(sr, 48000.0);
    assert_eq!(
        golden["cars"],
        serde_json::json!(reference::CAR_ORDER),
        "the cars setCar was called for"
    );
    let mut random = Mulberry32::new(golden["seed"].as_u64().unwrap() as u32);
    let ours: Vec<RefArray> = reference::arrays(sr, &mut random);
    let theirs = golden["arrays"].as_array().unwrap();
    let full = cached_full();

    let mut failures = Vec::new();
    let mut identical = 0;
    assert_eq!(
        ours.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
        theirs
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        "the arrays, in creation order"
    );
    for (a, g) in ours.iter().zip(theirs) {
        let name = &a.name;
        match &a.kind {
            Kind::Buffer { sample_rate } => {
                assert_eq!(g["kind"], "buffer", "{name}");
                assert_eq!(g["sampleRate"].as_f64(), Some(*sample_rate), "{name}");
                assert_eq!(
                    g["channels"].as_u64(),
                    Some(a.parts.len() as u64),
                    "{name}: channels"
                );
            }
            Kind::Wave {
                disable_normalization,
            } => {
                assert_eq!(g["kind"], "wave", "{name}");
                assert_eq!(g["parts"], serde_json::json!(["real", "imag"]), "{name}");
                let opts = match disable_normalization {
                    Some(d) => serde_json::json!({ "disableNormalization": d }),
                    None => serde_json::json!({}),
                };
                assert_eq!(g["options"], opts, "{name}: options");
            }
            Kind::Curve => assert_eq!(g["kind"], "curve", "{name}"),
        }
        if !matches!(a.kind, Kind::Buffer { .. }) {
            let aliases: Vec<&str> = g["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_str().unwrap())
                .collect();
            assert_eq!(a.aliases, aliases, "{name}: aliases");
        }
        let len = g["length"].as_u64().unwrap() as usize;
        let hashes = g["hashes"].as_array().unwrap();
        assert_eq!(hashes.len(), a.parts.len(), "{name}: parts");
        let mut same = true;
        for (p, part) in a.parts.iter().enumerate() {
            assert_eq!(part.len(), len, "{name}[{p}]: length");
            if array_hash(part) == hashes[p].as_str().unwrap() {
                continue;
            }
            same = false;
            let js = if let Some(off) = g.get("small") {
                Some(floats(ARRAYS_SMALL, off[p].as_u64().unwrap() as usize, len))
            } else {
                full.as_ref()
                    .map(|f| floats(f, g["cache"][p].as_u64().unwrap() as usize, len))
            };
            failures.push(match js {
                Some(js) => first_difference(name, p, part, &js)
                    .unwrap_or_else(|| format!("{name}[{p}]: hash differs, values equal")),
                None => format!("{name}[{p}]: hash differs (no cached JS array to compare)"),
            });
        }
        if same {
            identical += 1;
        }
    }
    println!("{identical} of {} arrays bit-identical", ours.len());
    assert!(
        failures.is_empty(),
        "{} of {} arrays differ:\n  {}",
        ours.len() - identical,
        ours.len(),
        failures.join("\n  ")
    );
}

/// The small arrays, value by value against `arrays-small.bin` (the hash
/// test above implies it; this one checks the golden's own consistency).
#[test]
fn small_golden_arrays_match_their_hashes() {
    let golden: Value = serde_json::from_str(ARRAYS_JSON).unwrap();
    let mut n = 0;
    for g in golden["arrays"].as_array().unwrap() {
        let Some(off) = g.get("small") else {
            continue;
        };
        let len = g["length"].as_u64().unwrap() as usize;
        for (p, h) in g["hashes"].as_array().unwrap().iter().enumerate() {
            let a = floats(ARRAYS_SMALL, off[p].as_u64().unwrap() as usize, len);
            assert_eq!(array_hash(&a), h.as_str().unwrap(), "{}", g["name"]);
            n += 1;
        }
    }
    assert!(n > 40, "{n} small arrays");
}

/// With the cache: every array value by value (native only).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cached_arrays_match_their_hashes() {
    let Some(full) = cached_full() else {
        eprintln!("no cached audio arrays: run `node tools/parity/audio-ref.mjs arrays`");
        return;
    };
    let golden: Value = serde_json::from_str(ARRAYS_JSON).unwrap();
    for g in golden["arrays"].as_array().unwrap() {
        let len = g["length"].as_u64().unwrap() as usize;
        for (p, h) in g["hashes"].as_array().unwrap().iter().enumerate() {
            let a = floats(&full, g["cache"][p].as_u64().unwrap() as usize, len);
            assert_eq!(array_hash(&a), h.as_str().unwrap(), "{}", g["name"]);
        }
    }
}
