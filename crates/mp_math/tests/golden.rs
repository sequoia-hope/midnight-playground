//! L2 for `mp_math` (roadmap WP 1.1): every function of `util/math.js` and
//! the JS semantics of SPEC 4.2, bit for bit against the JS run with the
//! parity kernel (`parity/golden/math/math.json`, written by
//! `tools/parity/math-golden.mjs`). The inputs are rebuilt here exactly as
//! that tool builds them; read the two side by side.
//!
//! The golden is compiled in, so the same test runs in wasm.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_math::*;
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/math/math.json");
const HEAD: usize = 16;

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("math.json parses")
}

fn fnv1a64(values: &[f64]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for v in values {
        for b in v.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

fn hex(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

/// Compare a sequence with its golden entry (`n`, `hash`, `head`). NaN is
/// written canonically, as the tool writes it.
fn check(name: &str, values: Vec<f64>, g: &Value) {
    let values: Vec<f64> = values
        .into_iter()
        .map(|v| if v.is_nan() { f64::NAN } else { v })
        .collect();
    assert_eq!(
        values.len() as u64,
        g["n"].as_u64().unwrap(),
        "{name}: length"
    );
    let head = g["head"].as_array().unwrap();
    for (i, h) in head.iter().enumerate() {
        assert_eq!(
            hex(values[i]),
            h.as_str().unwrap(),
            "{name}[{i}]: {}",
            values[i]
        );
    }
    assert_eq!(
        format!("{:016x}", fnv1a64(&values)),
        g["hash"].as_str().unwrap(),
        "{name}: hash differs past the first {HEAD} values"
    );
}

fn seed_list() -> Vec<f64> {
    vec![
        0.0,
        1.0,
        7.0,
        42.0,
        99.0,
        0x9e3779b9u32 as f64,
        4294967295.0,
        -1.0,
        3.7,
        8589934592.0 + 5.0,
        -123456789.9,
        js::to_uint32(1.0 + 2654435769.0 * 1.0) as f64,
        js::to_uint32(1.0 + 2654435769.0 * 2.0) as f64,
        js::to_uint32(1.0 + 2654435769.0 * 3.0) as f64,
    ]
}

fn edges() -> Vec<f64> {
    use core::f64::consts::PI;
    vec![
        0.0,
        -0.0,
        0.5,
        -0.5,
        1.5,
        -1.5,
        2.5,
        -2.5,
        0.49999999999999994,
        -0.49999999999999994,
        0.5000000000000001,
        -0.5000000000000001,
        1e-300,
        -1e-300,
        4503599627370495.5,
        -4503599627370495.5,
        4503599627370496.0,
        9007199254740993.0,
        -9007199254740993.0,
        2147483647.5,
        2147483648.0,
        -2147483649.0,
        4294967296.7,
        1e20,
        -1e20,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        PI,
        -PI,
        3.0 * PI,
        -7.5,
        1e4,
        0.1,
        0.2,
        0.3,
    ]
}

fn spread(n: usize, seed: u32, scale: f64) -> Vec<f64> {
    let mut r = Mulberry32::new(seed);
    (0..n).map(|_| (r.next_f64() * 2.0 - 1.0) * scale).collect()
}

struct Inputs {
    edges: Vec<f64>,
    ordinary: Vec<f64>,
    all: Vec<f64>,
    pairs: Vec<(f64, f64)>,
}

fn inputs() -> Inputs {
    let edges = edges();
    let mut ordinary = spread(2000, 11, 10.0);
    ordinary.extend(spread(2000, 12, 1000.0));
    ordinary.extend(spread(1000, 13, 1e7));
    let mut all = edges.clone();
    all.extend(&ordinary);
    let mut pairs = Vec::new();
    for &a in &edges {
        for &b in &edges {
            pairs.push((a, b));
        }
    }
    let mut i = 0;
    while i + 1 < ordinary.len() {
        pairs.push((ordinary[i], ordinary[i + 1]));
        i += 2;
    }
    Inputs {
        edges,
        ordinary,
        all,
        pairs,
    }
}

#[test]
fn inputs_match() {
    let g = golden();
    let inp = inputs();
    assert_eq!(inp.edges.len() + inp.ordinary.len(), inp.all.len());
    check("inputs", inp.all, &g["inputs"]);
}

#[test]
fn js_semantics() {
    let g = &golden()["js"];
    let Inputs { all, pairs, .. } = inputs();
    let map = |f: &dyn Fn(f64) -> f64| all.iter().map(|&x| f(x)).collect::<Vec<_>>();
    let map2 =
        |f: &dyn Fn(f64, f64) -> f64| pairs.iter().map(|&(a, b)| f(a, b)).collect::<Vec<_>>();
    check("round", map(&js::round), &g["round"]);
    check("sign", map(&js::sign), &g["sign"]);
    check("max", map2(&js::max), &g["max"]);
    check("min", map2(&js::min), &g["min"]);
    check(
        "max3",
        map2(&|a, b| js::max_n(&[a, b, a * 0.5])),
        &g["max3"],
    );
    check(
        "min3",
        map2(&|a, b| js::min_n(&[a, b, a * 0.5])),
        &g["min3"],
    );
    check("toInt32", map(&|x| js::to_int32(x) as f64), &g["toInt32"]);
    check(
        "toUint32",
        map(&|x| js::to_uint32(x) as f64),
        &g["toUint32"],
    );
    check("imul", map2(&|a, b| js::imul(a, b) as f64), &g["imul"]);
    check("fround", map(&js::fround), &g["fround"]);
    check("or", map(&|x| js::or(x, 7.0)), &g["or"]);
    check("mod", map2(&|a, b| a % b), &g["mod"]);
}

#[test]
fn helpers() {
    let g = &golden()["helpers"];
    let Inputs { all, pairs, .. } = inputs();
    assert_eq!(hex(DEG), g["DEG"].as_str().unwrap());
    let map2 =
        |f: &dyn Fn(f64, f64) -> f64| pairs.iter().map(|&(a, b)| f(a, b)).collect::<Vec<_>>();
    check(
        "clamp",
        all.iter().map(|&x| clamp(x, -1.0, 1.0)).collect(),
        &g["clamp"],
    );
    check(
        "clampVar",
        map2(&|a, b| clamp(a, js::min(b, 0.0), js::max(b, 0.0))),
        &g["clampVar"],
    );
    check("lerp", map2(&|a, b| lerp(a, b, 0.3)), &g["lerp"]);
    check(
        "lerpT",
        map2(&|a, b| lerp(-3.0, 8.0, a / (b.abs() + 1.0))),
        &g["lerpT"],
    );
    check(
        "invLerp",
        map2(&|a, b| inv_lerp(b, b + 10.0, a)),
        &g["invLerp"],
    );
    check(
        "smoothstep",
        map2(&|a, b| smoothstep(b, b + 25.0, a)),
        &g["smoothstep"],
    );
    check(
        "damp",
        map2(&|a, b| damp(a, b, 4.5, 1.0 / 120.0)),
        &g["damp"],
    );
    check(
        "dampLong",
        map2(&|a, b| damp(a, b, b.abs() % 50.0, a.abs() % 2.0)),
        &g["dampLong"],
    );
    check(
        "wrapAngle",
        all.iter()
            .filter(|x| x.is_finite() && x.abs() < 1e6)
            .map(|&x| wrap_angle(x))
            .collect(),
        &g["wrapAngle"],
    );
    check(
        "stopSpeed",
        map2(&|a, b| stop_speed(a, b.abs())),
        &g["stopSpeed"],
    );
}

#[test]
fn mulberry32_streams() {
    let g = golden();
    let entries = g["mulberry32"].as_array().unwrap();
    let seeds = seed_list();
    assert_eq!(entries.len(), seeds.len());
    for (seed, e) in seeds.iter().zip(entries) {
        assert_eq!(hex(*seed), e["seed"].as_str().unwrap(), "seed list");
        let mut r = Mulberry32::new(js::to_uint32(*seed));
        let draws = (0..100000).map(|_| r.next_f64()).collect();
        check(&format!("mulberry32({seed})"), draws, &e["draws"]);
    }
}

#[test]
fn hash2_values() {
    let g = &golden()["hash2"];
    let mut grid = Vec::new();
    for seed in [0.0, 3.0, 9.0, 11.0, 2147483647.0] {
        for i in -60..=60 {
            for j in -60..=60 {
                grid.push(hash2(i as f64, j as f64, seed));
            }
        }
    }
    check("hash2 grid", grid, &g["grid"]);
    let all = inputs().all;
    let mut odd = Vec::new();
    let mut i = 0;
    while i + 2 < all.len() {
        odd.push(hash2(all[i], all[i + 1], all[i + 2]));
        i += 3;
    }
    odd.push(hash2(5.0, 9.0, 0.0));
    check("hash2 odd", odd, &g["odd"]);
}

fn points() -> Vec<(f64, f64)> {
    let mut r = Mulberry32::new(5);
    let mut pts = Vec::new();
    for (n, scale) in [(4000, 50.0), (3000, 5000.0), (1000, 3e9), (1000, 1e16)] {
        for _ in 0..n {
            let x = (r.next_f64() * 2.0 - 1.0) * scale;
            let y = (r.next_f64() * 2.0 - 1.0) * scale;
            pts.push((x, y));
        }
    }
    for k in -20..=20 {
        let k = k as f64;
        pts.push((k, k * 0.5));
        pts.push((k * 0.25, -k));
    }
    pts
}

#[test]
fn noise_fbm_ridged() {
    let g = golden();
    let pts = points();
    let entries = g["noise"].as_array().unwrap();
    for (seed, e) in [1u32, 5, 7, 99, 108, 313].into_iter().zip(entries) {
        assert_eq!(e["seed"].as_u64().unwrap(), seed as u64);
        let n = Noise2D::new(seed);
        let at =
            |f: &dyn Fn(f64, f64) -> f64| pts.iter().map(|&(x, y)| f(x, y)).collect::<Vec<_>>();
        check(
            &format!("noise {seed}"),
            at(&|x, y| n.noise(x, y)),
            &e["noise"],
        );
        check(
            &format!("fbm {seed}"),
            at(&|x, y| fbm(&n, x / 90.0, y / 90.0, 5)),
            &e["fbm"],
        );
        check(
            &format!("fbm3 {seed}"),
            at(&|x, y| fbm(&n, x / 380.0, y / 380.0, 3)),
            &e["fbm3"],
        );
        check(
            &format!("fbm2 {seed}"),
            at(&|x, y| fbm_with(&n, x / 40.0 + 9.0, y / 40.0, 2, 2.0, 0.5)),
            &e["fbm2"],
        );
        check(
            &format!("fbmOdd {seed}"),
            at(&|x, y| fbm_with(&n, x / 70.0, y / 70.0, 4, 1.9, 0.55)),
            &e["fbmOdd"],
        );
        check(
            &format!("ridged {seed}"),
            at(&|x, y| ridged(&n, x / 1700.0 + 9.1, y / 1700.0, 5)),
            &e["ridged"],
        );
        check(
            &format!("ridged4 {seed}"),
            at(&|x, y| ridged(&n, x / 1500.0 + 4.2, y / 1500.0, 4)),
            &e["ridged4"],
        );
        check(
            &format!("ridgedOdd {seed}"),
            at(&|x, y| ridged_with(&n, x / 1900.0 + 3.3, y / 1900.0, 5, 2.0, 0.45)),
            &e["ridgedOdd"],
        );
    }
    let n = Noise2D::default();
    check(
        "noise default",
        pts.iter().map(|&(x, y)| n.noise(x, y)).collect(),
        &g["noiseDefault"],
    );
}
