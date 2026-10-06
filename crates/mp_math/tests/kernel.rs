//! Properties of the math kernel (`mp_math::kernel`): the exact values and
//! IEEE special cases each function must hit, its symmetries, ranges and
//! monotonicity, and agreement with the platform library within a few ulps
//! as a sanity check. The kernel has no golden of its own (the parity runs
//! use it on both sides), so a wrong wrapper here would go unnoticed there.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use core::f64::consts::{E, FRAC_PI_2, FRAC_PI_4, LN_2, PI};
use mp_math::Mulberry32;
use mp_math::kernel::*;

const NEG0: u64 = 0x8000_0000_0000_0000;

fn is_neg0(x: f64) -> bool {
    x.to_bits() == NEG0
}

fn is_pos0(x: f64) -> bool {
    x.to_bits() == 0
}

/// Deterministic inputs spread over [-scale, scale].
fn spread(n: usize, seed: u32, scale: f64) -> Vec<f64> {
    let mut r = Mulberry32::new(seed);
    (0..n).map(|_| (r.next_f64() * 2.0 - 1.0) * scale).collect()
}

/// Distance in units in the last place between two finite doubles of the
/// same sign (or zeros).
fn ulps(a: f64, b: f64) -> u64 {
    if a == b {
        return 0;
    }
    let key = |x: f64| {
        let b = x.to_bits() as i64;
        if b < 0 { i64::MIN - b } else { b }
    };
    key(a).abs_diff(key(b))
}

/// 2^k, exactly, for every k with a double (subnormals included).
fn pow2(k: i32) -> f64 {
    if k >= -1022 {
        f64::from_bits(((k + 1023) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (k + 1074))
    }
}

fn assert_close_ulps(name: &str, x: f64, got: f64, want: f64, max: u64) {
    assert!(
        (got.is_nan() && want.is_nan()) || ulps(got, want) <= max,
        "{name}({x:e}) = {got:e}, std gives {want:e} ({} ulps)",
        ulps(got, want)
    );
}

#[test]
fn unary_functions_agree_with_the_platform_library_within_a_few_ulps() {
    type F = fn(f64) -> f64;
    let fns: [(&str, F, F, f64); 9] = [
        ("sin", sin, f64::sin, 100.0),
        ("cos", cos, f64::cos, 100.0),
        ("tan", tan, f64::tan, 1.5),
        ("asin", asin, f64::asin, 1.0),
        ("acos", acos, f64::acos, 1.0),
        ("atan", atan, f64::atan, 1e3),
        ("exp", exp, f64::exp, 700.0),
        ("tanh", tanh, f64::tanh, 25.0),
        ("log", log, f64::ln, 1e6),
    ];
    for (name, k, s, scale) in fns {
        for x in spread(2000, 3, scale) {
            let x = if name == "log" { x.abs() } else { x };
            assert_close_ulps(name, x, k(x), s(x), 2);
        }
    }
    for x in spread(2000, 4, 1e6) {
        let x = x.abs();
        assert_close_ulps("log2", x, log2(x), x.log2(), 2);
        assert_close_ulps("log10", x, log10(x), x.log10(), 2);
    }
    for (i, y) in spread(1000, 5, 50.0).into_iter().enumerate() {
        let x = spread(1000, 6, 50.0)[i];
        assert_close_ulps("atan2", y, atan2(y, x), y.atan2(x), 2);
        assert_close_ulps("hypot", y, hypot(y, x), y.hypot(x), 1);
        let b = x.abs() * 0.1;
        assert_close_ulps("pow", b, pow(b, y * 0.2), b.powf(y * 0.2), 2);
    }
}

#[test]
fn sin_and_cos_hit_their_exact_values_and_symmetries() {
    assert!(is_pos0(sin(0.0)));
    assert_eq!(cos(0.0), 1.0);
    assert_eq!(cos(-0.0), 1.0);
    assert_eq!(sin(FRAC_PI_2), 1.0);
    assert_eq!(cos(PI), -1.0);
    // PI is not π: sin(PI) is the gap between them, correctly rounded.
    assert_eq!(sin(PI), 1.2246467991473532e-16);
    assert_eq!(cos(FRAC_PI_2), 6.123233995736766e-17);
    // Argument reduction at a huge argument (the classic test value).
    assert_eq!(sin(1e22), -0.8522008497671888);
    for x in spread(2000, 7, 1e4) {
        assert_eq!(sin(-x).to_bits(), (-sin(x)).to_bits(), "sin is odd at {x}");
        assert_eq!(cos(-x).to_bits(), cos(x).to_bits(), "cos is even at {x}");
        let (s, c) = (sin(x), cos(x));
        assert!((-1.0..=1.0).contains(&s) && (-1.0..=1.0).contains(&c));
        assert!((s * s + c * c - 1.0).abs() < 4e-16, "sin² + cos² at {x}");
    }
    for f in [sin as fn(f64) -> f64, cos, tan] {
        assert!(f(f64::INFINITY).is_nan());
        assert!(f(f64::NEG_INFINITY).is_nan());
        assert!(f(f64::NAN).is_nan());
    }
}

#[test]
fn tan_is_odd_and_matches_sin_over_cos() {
    assert!(is_pos0(tan(0.0)));
    assert!((tan(FRAC_PI_4) - 1.0).abs() <= f64::EPSILON);
    for x in spread(1000, 8, 1.5) {
        assert_eq!(tan(-x).to_bits(), (-tan(x)).to_bits());
        let q = sin(x) / cos(x);
        assert!((tan(x) - q).abs() <= 4.0 * f64::EPSILON * q.abs().max(1.0));
    }
}

#[test]
fn inverse_trig_has_its_ranges_domains_and_exact_ends() {
    assert_eq!(asin(1.0), FRAC_PI_2);
    assert_eq!(asin(-1.0), -FRAC_PI_2);
    assert!(is_pos0(acos(1.0)));
    assert_eq!(acos(-1.0), PI);
    assert_eq!(acos(0.0), FRAC_PI_2);
    assert_eq!(atan(1.0), FRAC_PI_4);
    assert_eq!(atan(f64::INFINITY), FRAC_PI_2);
    assert_eq!(atan(f64::NEG_INFINITY), -FRAC_PI_2);
    for bad in [
        1.0000000000000002,
        -1.0000000000000002,
        2.0,
        f64::INFINITY,
        f64::NAN,
    ] {
        assert!(asin(bad).is_nan(), "asin({bad})");
        assert!(acos(bad).is_nan(), "acos({bad})");
    }
    assert!(atan(f64::NAN).is_nan());
    for x in spread(2000, 9, 1.0) {
        let (a, c) = (asin(x), acos(x));
        assert!((-FRAC_PI_2..=FRAC_PI_2).contains(&a));
        assert!((0.0..=PI).contains(&c));
        assert_eq!(asin(-x).to_bits(), (-a).to_bits(), "asin is odd");
        assert!(
            (a + c - FRAC_PI_2).abs() < 1e-15,
            "asin + acos = π/2 at {x}"
        );
        assert!((sin(a) - x).abs() < 1e-15, "sin(asin(x)) = x at {x}");
    }
    for x in spread(2000, 10, 1e6) {
        assert_eq!(atan(-x).to_bits(), (-atan(x)).to_bits(), "atan is odd");
        assert!(atan(x).abs() <= FRAC_PI_2);
    }
}

#[test]
fn inverse_trig_is_monotonic() {
    let mut xs = spread(3000, 11, 1.0);
    xs.sort_by(f64::total_cmp);
    for w in xs.windows(2) {
        assert!(asin(w[0]) <= asin(w[1]));
        assert!(acos(w[0]) >= acos(w[1]));
        assert!(atan(w[0] * 1e3) <= atan(w[1] * 1e3));
        assert!(tanh(w[0] * 30.0) <= tanh(w[1] * 30.0));
    }
}

#[test]
fn atan2_follows_the_c99_special_cases() {
    // Signed zeros pick the half-plane.
    assert!(is_pos0(atan2(0.0, 0.0)));
    assert!(is_neg0(atan2(-0.0, 0.0)));
    assert_eq!(atan2(0.0, -0.0), PI);
    assert_eq!(atan2(-0.0, -0.0), -PI);
    assert_eq!(atan2(-0.0, -1.0), -PI);
    assert_eq!(atan2(1.0, 0.0), FRAC_PI_2);
    assert_eq!(atan2(1.0, -0.0), FRAC_PI_2);
    assert_eq!(atan2(-1.0, 0.0), -FRAC_PI_2);
    // Infinities.
    assert_eq!(atan2(f64::INFINITY, f64::INFINITY), FRAC_PI_4);
    assert_eq!(atan2(f64::INFINITY, f64::NEG_INFINITY), 3.0 * FRAC_PI_4);
    assert_eq!(
        atan2(f64::NEG_INFINITY, f64::NEG_INFINITY),
        -3.0 * FRAC_PI_4
    );
    assert_eq!(atan2(1.0, f64::NEG_INFINITY), PI);
    assert_eq!(atan2(-1.0, f64::NEG_INFINITY), -PI);
    assert!(is_pos0(atan2(1.0, f64::INFINITY)));
    assert_eq!(atan2(f64::INFINITY, 3.0), FRAC_PI_2);
    assert!(atan2(f64::NAN, 1.0).is_nan());
    assert!(atan2(1.0, f64::NAN).is_nan());
}

#[test]
fn atan2_recovers_the_angle_of_a_point_in_every_quadrant() {
    for (i, a) in spread(2000, 12, 3.1).into_iter().enumerate() {
        let r = 1.0 + i as f64;
        let got = atan2(r * sin(a), r * cos(a));
        assert!((got - a).abs() < 1e-14, "angle {a} came back as {got}");
        // Mirroring y negates the angle; the result stays in [-π, π].
        assert_eq!(atan2(-r * sin(a), r * cos(a)), -got);
        assert!((-PI..=PI).contains(&got));
    }
}

#[test]
fn exp_and_log_are_inverses_with_the_ieee_edges() {
    assert_eq!(exp(0.0), 1.0);
    assert_eq!(exp(-0.0), 1.0);
    // libm's exp(1) is one ulp above E (V8's is E); the parity kernel
    // replaces Math.exp on the JS side, so both runs see this value.
    assert!(ulps(exp(1.0), E) <= 1);
    assert_eq!(exp(f64::NEG_INFINITY), 0.0);
    assert_eq!(exp(f64::INFINITY), f64::INFINITY);
    assert_eq!(exp(710.0), f64::INFINITY, "overflow");
    assert!(is_pos0(exp(-746.0)), "underflow");
    assert!(exp(-745.0) > 0.0, "subnormal result");
    assert!(exp(f64::NAN).is_nan());
    assert!(is_pos0(log(1.0)));
    assert_eq!(log(E), 1.0);
    assert_eq!(log(2.0), LN_2);
    assert_eq!(log(0.0), f64::NEG_INFINITY);
    assert_eq!(log(-0.0), f64::NEG_INFINITY);
    assert_eq!(log(f64::INFINITY), f64::INFINITY);
    assert!(log(-1e-300).is_nan());
    assert!(log(f64::NEG_INFINITY).is_nan());
    assert!(log(f64::NAN).is_nan());
    for x in spread(2000, 13, 700.0) {
        let y = exp(x);
        assert!(y > 0.0);
        assert!((log(y) - x).abs() <= 2.0 * f64::EPSILON * x.abs().max(1.0));
    }
    let mut xs = spread(3000, 14, 50.0);
    xs.sort_by(f64::total_cmp);
    for w in xs.windows(2) {
        assert!(exp(w[0]) <= exp(w[1]), "exp is monotonic");
        let (a, b) = (w[0].abs(), w[1].abs());
        if a <= b {
            assert!(log(a) <= log(b), "log is monotonic");
        }
    }
}

#[test]
fn log2_and_log10_are_exact_on_powers_of_their_base() {
    for k in -1074..=1023 {
        let x = pow2(k);
        assert_eq!(log2(x), k as f64, "log2(2^{k})");
    }
    let mut x = 1.0;
    for k in 0..=22 {
        assert_eq!(log10(x), k as f64, "log10(1e{k})");
        x *= 10.0;
    }
    assert_eq!(log2(0.0), f64::NEG_INFINITY);
    assert_eq!(log10(-0.0), f64::NEG_INFINITY);
    assert!(log2(-1.0).is_nan());
    assert!(log10(-1.0).is_nan());
}

#[test]
fn tanh_saturates_and_is_odd() {
    assert_eq!(tanh(f64::INFINITY), 1.0);
    assert_eq!(tanh(f64::NEG_INFINITY), -1.0);
    assert_eq!(tanh(40.0), 1.0);
    assert!(tanh(f64::NAN).is_nan());
    for x in spread(2000, 15, 30.0) {
        let t = tanh(x);
        assert!((-1.0..=1.0).contains(&t));
        assert_eq!(tanh(-x).to_bits(), (-t).to_bits());
    }
}

#[test]
fn pow_keeps_the_ecmascript_and_ieee_edges() {
    // y = ±0 gives 1 for every x, NaN and infinities included.
    for x in [
        0.0,
        -0.0,
        1.0,
        -1.0,
        7.5,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ] {
        assert_eq!(pow(x, 0.0), 1.0, "{x} ** 0");
        assert_eq!(pow(x, -0.0), 1.0, "{x} ** -0");
    }
    // ...but a NaN exponent is NaN even for a base of 1 (ECMAScript, not C).
    assert!(pow(1.0, f64::NAN).is_nan());
    assert!(pow(-1.0, f64::INFINITY).is_nan());
    assert!(pow(1.0, f64::NEG_INFINITY).is_nan());
    assert!(pow(f64::NAN, 1.0).is_nan());
    assert_eq!(pow(1.0, 1e300), 1.0, "1 ** finite stays 1");
    // Negative bases: integer exponents keep the sign, others are NaN.
    assert_eq!(pow(-2.0, 3.0), -8.0);
    assert_eq!(pow(-2.0, 2.0), 4.0);
    assert!(pow(-8.0, 1.0 / 3.0).is_nan());
    // Zeros and infinities.
    assert_eq!(pow(0.0, -1.0), f64::INFINITY);
    assert_eq!(pow(-0.0, -2.0), f64::INFINITY);
    assert!(is_neg0(pow(-0.0, 3.0)));
    assert!(is_pos0(pow(-0.0, 2.0)));
    assert_eq!(pow(f64::NEG_INFINITY, 3.0), f64::NEG_INFINITY);
    assert_eq!(pow(f64::NEG_INFINITY, 2.0), f64::INFINITY);
    assert!(is_neg0(pow(f64::NEG_INFINITY, -3.0)));
    assert_eq!(pow(2.0, f64::INFINITY), f64::INFINITY);
    assert_eq!(pow(2.0, f64::NEG_INFINITY), 0.0);
    assert_eq!(pow(0.5, f64::NEG_INFINITY), f64::INFINITY);
    assert_eq!(pow(-0.5, f64::INFINITY), 0.0);
    // Exact on powers of two and on small integers.
    for k in -1074..=1023 {
        assert_eq!(pow(2.0, k as f64), pow2(k), "2 ** {k}");
    }
    assert_eq!(pow(3.0, 20.0), 3486784401.0);
    assert_eq!(pow(10.0, 15.0), 1e15);
    for x in spread(1000, 16, 1e6) {
        let x = x.abs();
        assert_eq!(pow(x, 1.0), x);
        assert!(ulps(pow(x, 0.5), x.sqrt()) <= 1, "x ** 0.5 is sqrt at {x}");
        assert!(ulps(pow(x, 2.0), x * x) <= 1, "x ** 2 at {x}");
    }
}

#[test]
fn hypot_is_symmetric_and_neither_overflows_nor_underflows() {
    assert_eq!(hypot(1e300, 1e300), 1e300 * 2f64.sqrt());
    assert!(hypot(1e-310, 1e-310) > 0.0);
    assert_eq!(hypot(f64::MAX, 0.0), f64::MAX);
    assert_eq!(hypot(-0.0, -0.0).to_bits(), 0, "always +0");
    assert_eq!(hypot(f64::INFINITY, f64::NAN), f64::INFINITY);
    assert_eq!(hypot(f64::NEG_INFINITY, 1.0), f64::INFINITY);
    for (i, a) in spread(1000, 17, 1e3).into_iter().enumerate() {
        let b = spread(1000, 18, 1e3)[i];
        let h = hypot(a, b);
        assert_eq!(h, hypot(b, a), "commutative");
        assert_eq!(h, hypot(-a, b), "sign-blind");
        assert_eq!(h, hypot(a, -b), "sign-blind");
        assert!(h >= a.abs() && h >= b.abs());
        assert!(h <= a.abs() + b.abs());
    }
}

#[test]
fn hypot_n_dispatches_by_argument_count() {
    for (i, a) in spread(200, 19, 1e3).into_iter().enumerate() {
        let b = spread(200, 20, 1e3)[i];
        let c = spread(200, 21, 1e3)[i];
        assert_eq!(hypot_n(&[a]), a.abs());
        assert_eq!(hypot_n(&[a, b]).to_bits(), hypot(a, b).to_bits());
        assert_eq!(hypot3(a, b, c).to_bits(), hypot_n(&[a, b, c]).to_bits());
        // Three or more: the plain left-to-right sum of squares.
        assert_eq!(hypot3(a, b, c), (a * a + b * b + c * c).sqrt());
    }
    assert_eq!(hypot_n(&[3.0, 4.0, 12.0, 84.0]), 85.0);
    assert!(hypot_n(&[f64::NAN]).is_nan());
    assert_eq!(hypot_n(&[f64::NEG_INFINITY]), f64::INFINITY);
    assert_eq!(hypot3(1.0, f64::NAN, f64::NEG_INFINITY), f64::INFINITY);
    assert!(hypot3(1.0, f64::NAN, 2.0).is_nan());
    assert_eq!(hypot3(-0.0, -0.0, -0.0).to_bits(), 0);
}
