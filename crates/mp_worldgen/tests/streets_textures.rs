//! WP 7.2: Downtown Streets' own canvas pictures (`streets/facades.js`'s
//! atlas, `streets/textures.js`'s street atlas, neon atlas, pavement,
//! barrier, puddle and train) against Chrome's drawing of them with the
//! bundled fonts (`tools/parity/streets-textures.mjs`,
//! `parity/golden/streets/textures.json`, the RGBA in
//! `parity/cache/<key>/streets/`). Each picture is held to the captured
//! picture of its size it matches best, within WP 3.2's threshold (mean
//! absolute difference under 3/255 per channel). The scene gate
//! (`tests/streets.rs`) holds them again in their places; this one checks
//! the pictures alone. Without the cache (CI, wasm) it checks that every
//! picture is made, of the captured size.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mp_canvas::compare::mean_abs_diff;
use mp_scene::digest::sha256_hex;
use mp_worldgen::streets::facades::facade_atlas;
use mp_worldgen::streets::textures::{
    barrier_texture, neon_atlas, pavement_texture, puddle_texture, street_atlas, train_texture,
};
use mp_worldgen::textures::{Cached, Texture, TextureCache};
use serde_json::Value;

/// WP 3.2's threshold, per channel.
const LIMIT: f64 = 3.0;

fn golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/streets/textures.json"))
        .expect("streets textures golden parses")
}

/// Every picture, by name.
fn pictures(cache: &mut TextureCache) -> Vec<(String, std::sync::Arc<Cached>, bool)> {
    let mut out = Vec::new();
    let pairs = [
        ("facade", facade_atlas(cache)),
        ("street", street_atlas(cache)),
        ("neon", neon_atlas(cache)),
        ("train", train_texture(cache)),
    ];
    for (n, c) in pairs {
        out.push((format!("{n}.main"), c.clone(), false));
        out.push((format!("{n}.second"), c, true));
    }
    out.push(("pavement".into(), pavement_texture(cache), false));
    out.push(("barrier".into(), barrier_texture(cache), false));
    out.push(("puddle".into(), puddle_texture(cache), false));
    out
}

fn layer(c: &Cached, second: bool) -> &Texture {
    match (c, second) {
        (Cached::Facade { emissive, .. }, true) => emissive,
        (c, _) => c.texture(),
    }
}

fn captured(k: usize, sha: &str) -> Option<Vec<u8>> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let dir = mp_scene::cache::scenes_dir(&common::root()).ok()?;
    let b = std::fs::read(dir.parent()?.join(format!("streets/canvas-{k}.rgba"))).ok()?;
    (sha256_hex(&b) == sha).then_some(b)
}

#[test]
fn streets_pictures_match_chrome() {
    let g = golden();
    let pics = g["pictures"].as_array().expect("pictures");
    let mut cache = TextureCache::new();
    let mut problems = Vec::new();
    let mut with_cache = 0;
    for (name, c, second) in pictures(&mut cache) {
        let t = layer(&c, second);
        let same: Vec<usize> = pics
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p["width"].as_u64() == Some(t.width as u64)
                    && p["height"].as_u64() == Some(t.height as u64)
            })
            .map(|(k, _)| k)
            .collect();
        if same.is_empty() {
            problems.push(format!(
                "{name} ({}×{}): no capture of that size",
                t.width, t.height
            ));
            continue;
        }
        let mut best: Option<(usize, [f64; 4])> = None;
        for &k in &same {
            let Some(js) = captured(k, pics[k]["sha256"].as_str().expect("sha")) else {
                continue;
            };
            let d = mean_abs_diff(&t.rgba, &js);
            let score: f64 = d.iter().sum();
            if best.is_none_or(|(_, b)| score < b.iter().sum::<f64>()) {
                best = Some((k, d));
            }
        }
        let Some((k, d)) = best else {
            println!("{name}: {}×{}, no cached capture", t.width, t.height);
            continue;
        };
        with_cache += 1;
        println!("{name}: capture {k}, mean abs diff {d:.3?}");
        if d.iter().any(|&x| x >= LIMIT) {
            problems.push(format!("{name}: capture {k}, mean abs diff {d:.3?}"));
        }
    }
    println!("{with_cache} pictures compared with the capture");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
