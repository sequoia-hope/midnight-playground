//! Properties of the JavaScript-semantics helpers (`mp_math::js`) over wide
//! random ranges, beside the bit-exact golden in `golden.rs` (which pins a
//! fixed input list against the JS): what each helper must satisfy for every
//! input, where Rust's lookalike operation would quietly differ.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_math::Mulberry32;
use mp_math::js::*;

const NEG0: u64 = 0x8000_0000_0000_0000;
const TWO32: f64 = 4294967296.0;

fn spread(n: usize, seed: u32, scale: f64) -> Vec<f64> {
    let mut r = Mulberry32::new(seed);
    (0..n).map(|_| (r.next_f64() * 2.0 - 1.0) * scale).collect()
}

/// Ordinary values plus the halves and near-halves where rounding differs.
fn round_inputs() -> Vec<f64> {
    let mut v = spread(3000, 1, 1e3);
    v.extend(spread(1000, 2, 1e12));
    for k in -50..50 {
        let h = k as f64 + 0.5;
        v.extend([h, h.next_up(), h.next_down()]);
    }
    v
}

#[test]
fn round_gives_the_nearest_integer_with_halves_toward_plus_infinity() {
    for x in round_inputs() {
        let r = round(x);
        assert_eq!(r, r.floor(), "round({x}) = {r} is integral");
        // Exact below 2^51: x lies in [r - 0.5, r + 0.5).
        assert!(r - 0.5 <= x && x < r + 0.5, "round({x}) = {r}");
        // Not Rust's round (halves away from zero), except where they agree.
        if x.fract().abs() != 0.5 {
            assert_eq!(r, x.round(), "away from the halves, round({x})");
        }
        // Shifting by an integer shifts the result, where the shift is exact
        // (and away from the sign of a zero result).
        if r != 0.0 && (x + 3.0) - 3.0 == x {
            assert_eq!(round(x + 3.0), r + 3.0, "round({x} + 3)");
        }
    }
}

#[test]
fn round_keeps_large_values_infinities_nan_and_the_sign_of_zero() {
    for x in [
        4503599627370496.0,
        -4503599627370496.0,
        9007199254740993.0,
        1e300,
        -1e300,
        f64::MAX,
        f64::MIN,
    ] {
        assert_eq!(round(x), x, "already integral: {x}");
    }
    assert_eq!(round(f64::INFINITY), f64::INFINITY);
    assert_eq!(round(f64::NEG_INFINITY), f64::NEG_INFINITY);
    assert!(round(f64::NAN).is_nan());
    assert_eq!(round(-0.0).to_bits(), NEG0);
    assert_eq!(round(-1e-300).to_bits(), NEG0);
    assert_eq!(round(-f64::MIN_POSITIVE).to_bits(), NEG0);
    assert_eq!(round(1e-300).to_bits(), 0);
    assert_eq!(round(-0.5000000000000001), -1.0);
    assert_eq!(round(-0.49999999999999994).to_bits(), NEG0);
}

#[test]
fn sign_is_minus_one_zero_or_one_and_keeps_the_zero_and_nan() {
    for x in spread(2000, 3, 1e6) {
        let s = sign(x);
        assert!(s == 1.0 || s == -1.0);
        assert_eq!(s * x.abs(), x);
    }
    assert_eq!(sign(f64::INFINITY), 1.0);
    assert_eq!(sign(f64::NEG_INFINITY), -1.0);
    assert_eq!(
        sign(-f64::MIN_POSITIVE / 2.0),
        -1.0,
        "subnormals have a sign"
    );
    assert_eq!(sign(0.0).to_bits(), 0);
    assert_eq!(sign(-0.0).to_bits(), NEG0);
}

#[test]
fn max_and_min_are_commutative_to_the_bit() {
    let mut vals = spread(200, 4, 10.0);
    vals.extend([0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, 1.0, -1.0]);
    for &a in &vals {
        for &b in &vals {
            let (mx, mn) = (max(a, b), min(a, b));
            assert_eq!(mx.to_bits(), max(b, a).to_bits(), "max({a}, {b})");
            assert_eq!(mn.to_bits(), min(b, a).to_bits(), "min({a}, {b})");
            assert!(mx >= a && mx >= b && mn <= a && mn <= b);
            assert!(mx == a || mx == b);
            assert!(mn == a || mn == b);
        }
    }
}

#[test]
fn max_n_and_min_n_propagate_a_nan_from_any_position() {
    let base = [3.0, -1.0, 7.0, 0.5];
    for at in 0..=base.len() {
        let mut v = base.to_vec();
        v.insert(at, f64::NAN);
        assert!(max_n(&v).is_nan(), "NaN at {at}");
        assert!(min_n(&v).is_nan(), "NaN at {at}");
    }
    assert_eq!(max_n(&base), 7.0);
    assert_eq!(min_n(&base), -1.0);
    assert_eq!(min_n(&[]), f64::INFINITY);
    assert_eq!(max_n(&[-0.0]).to_bits(), NEG0, "one -0 stays -0");
    assert_eq!(min_n(&[0.0]).to_bits(), 0);
    assert_eq!(max_n(&[-0.0, 0.0, -0.0]).to_bits(), 0);
    assert_eq!(min_n(&[0.0, -0.0, 0.0]).to_bits(), NEG0);
    assert_eq!(max_n(&[f64::NEG_INFINITY]), f64::NEG_INFINITY);
}

#[test]
fn or_treats_zero_and_nan_as_missing_and_keeps_everything_else() {
    for x in spread(500, 5, 1e3) {
        assert_eq!(or(x, 7.0), x);
        assert_eq!(or_opt(Some(x), 7.0), x);
    }
    for missing in [0.0, -0.0, f64::NAN] {
        assert_eq!(or(missing, 7.0), 7.0);
        assert_eq!(or_opt(Some(missing), 7.0), 7.0);
    }
    assert_eq!(or(f64::INFINITY, 7.0), f64::INFINITY);
    assert_eq!(or(f64::NEG_INFINITY, 7.0), f64::NEG_INFINITY);
    assert_eq!(or(5e-324, 7.0), 5e-324, "the smallest subnormal is truthy");
    // The default is returned as given, even when it is itself "missing".
    assert_eq!(or(0.0, -0.0).to_bits(), NEG0);
    assert!(or(0.0, f64::NAN).is_nan());
}

#[test]
fn to_int32_and_to_uint32_wrap_modulo_two_to_the_32() {
    let mut xs = spread(2000, 6, 1e10);
    xs.extend(spread(1000, 7, 1e15));
    for x in xs {
        let t = x.trunc();
        let u = to_uint32(x);
        let i = to_int32(x);
        assert_eq!(u as i32, i, "same 32 bits");
        assert_eq!(i, to_int32(t), "truncates first");
        // Below 2^63 the conversion is the low 32 bits of the integer.
        assert_eq!(i, t as i64 as i32, "ToInt32({x})");
        assert_eq!(to_uint32(t + TWO32), u, "period 2^32 at {x}");
        assert_eq!(to_int32(t - TWO32), i, "period 2^32 at {x}");
        // Within range they are the identity.
        if (-2147483648.0..2147483648.0).contains(&t) {
            assert_eq!(i as f64, t);
        }
    }
    for x in [
        0.0,
        -0.0,
        0.9,
        -0.9,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
    ] {
        assert_eq!(to_int32(x), 0, "ToInt32({x})");
        assert_eq!(to_uint32(x), 0, "ToUint32({x})");
    }
    // Rust's `as` saturates; ToInt32 wraps.
    assert_eq!(to_int32(1e10), 1410065408);
    assert_eq!(to_uint32(-2147483648.0), 2147483648);
    assert_eq!(to_int32(9007199254740992.0), 0, "2^53");
    assert_eq!(to_int32(1.8446744073709552e19), 0, "2^64");
    assert_eq!(to_uint32(1e300), 0, "a huge integral double is 0 mod 2^32");
}

#[test]
fn imul_is_the_low_32_bits_of_the_integer_product() {
    let xs = spread(400, 8, 1e10);
    let ys = spread(400, 9, 1e5);
    for (&a, &b) in xs.iter().zip(&ys) {
        let want = (to_int32(a) as i64 * to_int32(b) as i64) as i32;
        assert_eq!(imul(a, b), want, "imul({a}, {b})");
        assert_eq!(imul(a, b), imul(b, a));
    }
    assert_eq!(imul(-1.0, -2147483648.0), -2147483648);
    assert_eq!(imul(65536.0, 65536.0), 0);
    assert_eq!(imul(f64::NAN, 3.0), 0);
    assert_eq!(imul(3.7, -2.9), -6, "operands truncate before multiplying");
}

#[test]
fn fround_rounds_to_the_nearest_f32_with_ties_to_even() {
    for x in spread(2000, 10, 1e6) {
        let f = fround(x);
        assert_eq!(fround(f), f, "idempotent");
        assert_eq!(f, f as f32 as f64, "representable as f32");
        assert!((f - x).abs() <= x.abs() * f32::EPSILON as f64 / 2.0);
    }
    let ulp1 = 2f64.powi(-23);
    assert_eq!(fround(1.0 + ulp1 / 2.0), 1.0, "tie goes to even (down)");
    assert_eq!(
        fround(1.0 + 1.5 * ulp1),
        1.0 + 2.0 * ulp1,
        "tie goes to even (up)"
    );
    assert_eq!(fround(1.0 + ulp1 / 2.0 + 1e-12), 1.0 + ulp1, "past the tie");
    // The top of the range: past half an ulp beyond f32::MAX is infinite.
    let max = f32::MAX as f64;
    let half_ulp_top = 2f64.powi(103);
    assert_eq!(fround(max + half_ulp_top * 0.99), max);
    assert_eq!(fround(max + half_ulp_top), f64::INFINITY);
    assert_eq!(fround(-1e39), f64::NEG_INFINITY);
    // The bottom: subnormal f32s survive, half the smallest rounds to 0.
    let tiny = 2f64.powi(-149);
    assert_eq!(fround(tiny), tiny);
    assert_eq!(fround(tiny / 2.0).to_bits(), 0);
    assert_eq!(fround(-tiny / 2.0).to_bits(), NEG0);
    assert_eq!(fround(-0.0).to_bits(), NEG0);
    assert!(fround(f64::NAN).is_nan());
    assert_eq!(fround(f64::INFINITY), f64::INFINITY);
    assert_eq!(fround(0.1), 0.10000000149011612);
}
