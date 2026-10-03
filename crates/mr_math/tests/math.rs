//! util/math.js: the small helpers every module leans on, the seeded random
//! numbers that make the world identical on every load, and the noise the
//! terrain is built from. Port of `test/unit/math.test.js`, same assertions.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use core::f64::consts::PI;
use mr_math::kernel::{cos, sin};
use mr_math::*;

fn near(a: f64, b: f64, eps: f64, msg: &str) {
    assert!((a - b).abs() <= eps, "{a} ≉ {b} {msg}");
}

#[test]
fn clamp_keeps_values_inside_lo_hi() {
    assert_eq!(clamp(5.0, 0.0, 10.0), 5.0);
    assert_eq!(clamp(-1.0, 0.0, 10.0), 0.0);
    assert_eq!(clamp(11.0, 0.0, 10.0), 10.0);
    assert_eq!(clamp(0.0, 0.0, 10.0), 0.0);
    assert_eq!(clamp(10.0, 0.0, 10.0), 10.0);
    assert_eq!(clamp(f64::NEG_INFINITY, -1.0, 1.0), -1.0);
    assert_eq!(clamp(f64::INFINITY, -1.0, 1.0), 1.0);
}

#[test]
fn lerp_and_inv_lerp_are_inverses_and_inv_lerp_clamps() {
    assert_eq!(lerp(2.0, 6.0, 0.0), 2.0);
    assert_eq!(lerp(2.0, 6.0, 1.0), 6.0);
    assert_eq!(lerp(2.0, 6.0, 0.25), 3.0);
    assert_eq!(lerp(2.0, 6.0, 1.5), 8.0, "lerp extrapolates");
    for t in [0.0, 0.1, 0.5, 0.9, 1.0] {
        near(inv_lerp(2.0, 6.0, lerp(2.0, 6.0, t)), t, 1e-9, "");
    }
    assert_eq!(inv_lerp(2.0, 6.0, 0.0), 0.0);
    assert_eq!(inv_lerp(2.0, 6.0, 99.0), 1.0);
    assert_eq!(inv_lerp(6.0, 2.0, 3.0), 0.75, "works with a reversed range");
}

#[test]
fn smoothstep_eases_from_0_to_1_with_flat_ends() {
    assert_eq!(smoothstep(0.0, 10.0, -5.0), 0.0);
    assert_eq!(smoothstep(0.0, 10.0, 0.0), 0.0);
    assert_eq!(smoothstep(0.0, 10.0, 5.0), 0.5);
    assert_eq!(smoothstep(0.0, 10.0, 10.0), 1.0);
    assert_eq!(smoothstep(0.0, 10.0, 50.0), 1.0);
    let mut prev = -1.0;
    let mut x = 0.0;
    while x <= 10.0 {
        let y = smoothstep(0.0, 10.0, x);
        assert!(y >= prev);
        prev = y;
        x += 0.5;
    }
    // Flat at the ends: small steps near 0 and 1 change it far less than a linear ramp.
    assert!(smoothstep(0.0, 10.0, 0.1) < 0.01 * 0.1 * 10.0);
    assert!(1.0 - smoothstep(0.0, 10.0, 9.9) < 0.001);
}

#[test]
fn damp_approaches_the_target_frame_rate_independently() {
    assert_eq!(damp(0.0, 10.0, 5.0, 0.0), 0.0);
    near(damp(0.0, 10.0, 5.0, 1e6), 10.0, 1e-9, "");
    // One 0.1 s step lands where ten 0.01 s steps do.
    let mut a = 0.0;
    for _ in 0..10 {
        a = damp(a, 10.0, 3.0, 0.01);
    }
    near(a, damp(0.0, 10.0, 3.0, 0.1), 1e-9, "");
    // Never overshoots.
    assert!(damp(0.0, 10.0, 50.0, 0.5) <= 10.0);
}

#[test]
fn wrap_angle_maps_any_angle_into_minus_pi_pi() {
    for a in [
        0.0,
        1.0,
        -1.0,
        PI,
        -PI,
        3.0 * PI,
        -3.0 * PI,
        7.5,
        -7.5,
        100.0,
        -100.0,
        1e4,
    ] {
        let w = wrap_angle(a);
        assert!((-PI..=PI).contains(&w), "wrapAngle({a}) = {w}");
        near(cos(w), cos(a), 1e-9, &format!("same direction for {a}"));
        near(sin(w), sin(a), 1e-9, &format!("same direction for {a}"));
    }
    near(wrap_angle(2.0 * PI + 0.25), 0.25, 1e-12, "");
    near(wrap_angle(-2.0 * PI - 0.25), -0.25, 1e-12, "");
    near(DEG * 180.0, PI, 1e-9, "");
}

#[test]
fn stop_speed_is_the_speed_that_stops_in_exactly_that_room() {
    assert_eq!(stop_speed(0.0, 10.0), 0.0);
    assert_eq!(stop_speed(-50.0, 10.0), 0.0, "no room: stand still");
    // v² = 2 a d
    near(stop_speed(45.0, 10.0), 30.0, 1e-9, "");
    let v = stop_speed(120.0, 7.5);
    near((v * v) / (2.0 * 7.5), 120.0, 1e-9, "");
}

fn draws(seed: u32, n: usize) -> Vec<f64> {
    let mut r = Mulberry32::new(seed);
    (0..n).map(|_| r.next_f64()).collect()
}

#[test]
fn mulberry32_is_deterministic_seeded_and_in_0_1() {
    let (sa, sb, sc) = (draws(42, 1000), draws(42, 1000), draws(43, 1000));
    assert_eq!(sa, sb, "same seed, same sequence");
    assert_ne!(sa, sc, "different seed, different sequence");
    for &x in &sa {
        assert!((0.0..1.0).contains(&x));
    }
    let mean = sa.iter().sum::<f64>() / sa.len() as f64;
    assert!((mean - 0.5).abs() < 0.05, "roughly uniform (mean {mean})");
    // Spread over the whole range.
    let mut bins = [0; 10];
    for &x in &sa {
        bins[(x * 10.0).floor() as usize] += 1;
    }
    for n in bins {
        assert!(n > 60 && n < 140, "bins {bins:?}");
    }
}

#[test]
fn rrange_and_rpick_draw_from_their_range_and_list() {
    let mut r = Mulberry32::new(7);
    for _ in 0..500 {
        let x = rrange(&mut r, -3.0, 5.0);
        assert!((-3.0..5.0).contains(&x));
    }
    let list = ["a", "b", "c"];
    let mut seen = Vec::new();
    for _ in 0..200 {
        let p = *rpick(&mut r, &list);
        if !seen.contains(&p) {
            seen.push(p);
        }
    }
    seen.sort();
    assert_eq!(seen, list);
}

#[test]
fn hash2_is_deterministic_and_in_0_1() {
    assert_eq!(hash2(3.0, 4.0, 1.0), hash2(3.0, 4.0, 1.0));
    assert_ne!(hash2(3.0, 4.0, 1.0), hash2(4.0, 3.0, 1.0));
    assert_ne!(hash2(3.0, 4.0, 1.0), hash2(3.0, 4.0, 2.0));
    for i in -20..20 {
        for j in -20..20 {
            let h = hash2(i as f64, j as f64, 0.0);
            assert!((0.0..1.0).contains(&h));
        }
    }
}

#[test]
fn noise_fbm_and_ridged_are_finite_bounded_and_deterministic() {
    let (n1, n2, n3) = (Noise2D::new(5), Noise2D::new(5), Noise2D::new(6));
    let (mut differs, mut min, mut max) = (false, f64::INFINITY, f64::NEG_INFINITY);
    for i in 0..400 {
        let i = i as f64;
        let x = (i * 7.31) % 97.0 - 40.0;
        let y = (i * 3.77) % 53.0 - 20.0;
        let v = n1.noise(x, y);
        assert!(v.is_finite());
        assert_eq!(v, n2.noise(x, y), "same seed, same noise");
        if v != n3.noise(x, y) {
            differs = true;
        }
        min = js::min(min, v);
        max = js::max(max, v);
        let f = fbm(&n1, x * 0.1, y * 0.1, 5);
        assert!(f.is_finite() && f.abs() <= 1.05, "fbm {f}");
        let r = ridged(&n1, x * 0.1, y * 0.1, 5);
        assert!(r.is_finite() && (0.0..=1.0001).contains(&r), "ridged {r}");
    }
    assert!(differs, "a different seed gives different noise");
    assert!(min >= -1.05 && max <= 1.05, "noise range {min}..{max}");
    assert!(max - min > 0.8, "noise actually varies");
    // Continuous: nearby points give nearby values.
    assert!((n1.noise(10.0, 10.0) - n1.noise(10.001, 10.0)).abs() < 0.01);
}
