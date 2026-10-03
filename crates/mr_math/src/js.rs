//! JavaScript semantics that Rust's own operations do not share (SPEC 4.2).
//!
//! The simulation and world generation port each use of these JS operations
//! through a helper here, because the Rust lookalike differs at an edge that
//! reaches the results: the sign of a zero (which `atan2` sees), NaN, the
//! halfway case of rounding, or the wrap of a 32-bit conversion.
//!
//! What needs no helper: `+ - * /`, `Math.sqrt`, `Math.floor`, `Math.ceil`,
//! `Math.trunc` and `Math.abs` are the same operations as `f64`'s; the `%`
//! operator on numbers is `f64`'s `%` (both are C's `fmod`). The inexact
//! functions are in [`crate::kernel`]. `Array.prototype.sort` is stable: port
//! it with `sort_by`, never `sort_unstable_by`.

/// `Math.sign(x)`: NaN for NaN, and the zero itself for ±0 (so
/// `Math.sign(-0)` is -0), where `f64::signum` gives ±1. This decides the
/// handbrake and coasting terms at a standstill (`CarPhysics.js:208,217`).
pub fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x // ±0 or NaN
    }
}

/// `Math.round(x)`: the nearest integer, halves toward +∞. Neither
/// `f64::round` (halves away from zero) nor a naive `floor(x + 0.5)` (wrong
/// for 0.49999999999999994, whose sum rounds up to 1) is the same. Values in
/// [-0.5, 0) round to -0.
pub fn round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    if x > 0.0 && x < 0.5 {
        return 0.0;
    }
    if (-0.5..0.0).contains(&x) {
        return -0.0;
    }
    // x - floor(x) is exact for every double (it is 0 once x is integral).
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// `Math.max(a, b)`: NaN if either is NaN, and +0 above -0. `f64::max`
/// ignores a NaN and may return either zero.
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() && b.is_sign_negative() {
            -0.0
        } else {
            0.0
        };
    }
    if a > b { a } else { b }
}

/// `Math.min(a, b)`: NaN if either is NaN, and -0 below +0.
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() || b.is_sign_negative() {
            -0.0
        } else {
            0.0
        };
    }
    if a < b { a } else { b }
}

/// `Math.max(...args)`; -Infinity for none.
pub fn max_n(args: &[f64]) -> f64 {
    args.iter().fold(f64::NEG_INFINITY, |m, &x| max(m, x))
}

/// `Math.min(...args)`; +Infinity for none.
pub fn min_n(args: &[f64]) -> f64 {
    args.iter().fold(f64::INFINITY, |m, &x| min(m, x))
}

/// `x || d` for a number: 0, -0 and NaN count as missing (`x ?? d` would
/// keep them).
pub fn or(x: f64, d: f64) -> f64 {
    if x == 0.0 || x.is_nan() { d } else { x }
}

/// `x || d` where `x` may be `undefined` too.
pub fn or_opt(x: Option<f64>, d: f64) -> f64 {
    match x {
        Some(v) => or(v, d),
        None => d,
    }
}

/// ToInt32, as `x | 0`, `Math.imul` and the bitwise operators apply it:
/// truncate, then wrap modulo 2^32 into the signed range; NaN and ±∞ give
/// 0. Rust's `x as i32` saturates instead.
pub fn to_int32(x: f64) -> i32 {
    to_uint32(x) as i32
}

/// ToUint32, as `x >>> 0` applies it.
pub fn to_uint32(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    const TWO32: f64 = 4294967296.0;
    // Both steps are exact: fmod of an integral double, and a sum below 2^32.
    let mut m = x.trunc() % TWO32;
    if m < 0.0 {
        m += TWO32;
    }
    m as u32
}

/// `Math.imul(a, b)` on numbers: the low 32 bits of the product of their
/// ToInt32 values.
pub fn imul(a: f64, b: f64) -> i32 {
    to_int32(a).wrapping_mul(to_int32(b))
}

/// `Math.fround(x)`, and what storing into a `Float32Array` and reading
/// back does: round to the nearest `f32` (ties to even).
pub fn fround(x: f64) -> f64 {
    x as f32 as f64
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    const NEG0: u64 = 0x8000_0000_0000_0000;

    #[test]
    fn sign_keeps_zeros() {
        assert_eq!(sign(-0.0).to_bits(), NEG0);
        assert_eq!(sign(0.0).to_bits(), 0);
        assert_eq!(sign(-3.0), -1.0);
        assert_eq!(sign(1e-300), 1.0);
        assert!(sign(f64::NAN).is_nan());
    }

    #[test]
    fn round_is_javascripts() {
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(-0.3).to_bits(), NEG0);
        assert_eq!(round(-0.5).to_bits(), NEG0);
        assert_eq!(round(0.5), 1.0);
        assert_eq!(round(-1.5), -1.0);
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(-2.6), -3.0);
        assert_eq!(round(4503599627370495.5), 4503599627370496.0);
        assert_eq!(round(-4503599627370495.5), -4503599627370495.0);
    }

    #[test]
    fn max_and_min_order_zeros_and_keep_nan() {
        assert_eq!(max(-0.0, 0.0).to_bits(), 0);
        assert_eq!(max(0.0, -0.0).to_bits(), 0);
        assert_eq!(max(-0.0, -0.0).to_bits(), NEG0);
        assert_eq!(min(0.0, -0.0).to_bits(), NEG0);
        assert_eq!(min(0.0, 0.0).to_bits(), 0);
        assert!(max(f64::NAN, 1.0).is_nan());
        assert!(min(1.0, f64::NAN).is_nan());
        assert_eq!(max_n(&[]), f64::NEG_INFINITY);
        assert_eq!(min_n(&[3.0, -2.0, 5.0]), -2.0);
    }

    #[test]
    fn conversions_wrap() {
        assert_eq!(to_int32(2147483648.0), -2147483648);
        assert_eq!(to_int32(-2147483649.0), 2147483647);
        assert_eq!(to_int32(-1.9), -1);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_uint32(-1.0), 4294967295);
        assert_eq!(to_uint32(4294967296.7), 0);
        assert_eq!(imul(0xffff_ffffu32 as f64, 5.0), -5);
        assert_eq!(or(0.0, 7.0), 7.0);
        assert_eq!(or(-0.0, 7.0), 7.0);
        assert_eq!(or(f64::NAN, 7.0), 7.0);
        assert_eq!(or(-2.0, 7.0), -2.0);
        assert_eq!(or_opt(None, 7.0), 7.0);
    }
}
