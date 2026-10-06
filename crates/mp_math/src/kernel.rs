//! The math kernel (SPEC 4.2): every `Math` function that is not exact by
//! definition, implemented once on the `libm` crate (software, the same bits
//! on every platform).
//!
//! Both sides of every parity comparison use it. The Rust simulation and
//! world generation call these functions, never `f64::sin` and friends. The
//! JS reference run replaces `Math.sin` and the rest with this module compiled
//! to wasm (`tools/parity/kernel/`). So the reference differs from the live
//! JS game by at most the last bit of these functions, and the Rust can be
//! required to match the reference exactly.
//!
//! Each function follows the ECMAScript definition where it differs from C's
//! (`pow` and `hypot` at their edges), so the kernel-on reference run takes
//! the same branches as the live game.
//!
//! The set is what the game and three.js r180 call: the thirteen of SPEC 4.2
//! plus `log10`, which three.js's `mergeVertices` uses during world
//! generation (DECISIONS.md D6). The exact functions (`sqrt`, `floor`,
//! `abs`, ...) are not here: `f64`'s own are correctly rounded already.

pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

/// `Math.atan2(y, x)`. ECMAScript's special cases are C99's.
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// `Math.log`, the natural logarithm.
pub fn log(x: f64) -> f64 {
    libm::log(x)
}

pub fn log2(x: f64) -> f64 {
    libm::log2(x)
}

pub fn log10(x: f64) -> f64 {
    libm::log10(x)
}

pub fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

/// `Math.pow(x, y)` and the `**` operator. ECMAScript differs from C in two
/// places: a NaN exponent always gives NaN (C: `pow(1, NaN)` is 1), and
/// `(±1) ** ±Infinity` is NaN (C: 1).
pub fn pow(x: f64, y: f64) -> f64 {
    if y.is_nan() {
        return f64::NAN;
    }
    if y.is_infinite() && x.abs() == 1.0 {
        return f64::NAN;
    }
    libm::pow(x, y)
}

/// `Math.hypot(x, y)`. ECMAScript: any infinite argument gives +Infinity,
/// even beside a NaN; otherwise any NaN gives NaN.
pub fn hypot(x: f64, y: f64) -> f64 {
    if x.is_infinite() || y.is_infinite() {
        return f64::INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    libm::hypot(x, y)
}

/// `Math.hypot(...args)` for any number of arguments, as the kernel defines
/// it (SPEC 4.2): `abs` for one, [`hypot`] for two, and for more the square
/// root of the left-to-right sum of squares. With the ECMAScript rules for
/// infinities and NaN, and +0 for none.
pub fn hypot_n(args: &[f64]) -> f64 {
    match args {
        [] => 0.0,
        [x] => x.abs(),
        [x, y] => hypot(*x, *y),
        _ => {
            if args.iter().any(|a| a.is_infinite()) {
                return f64::INFINITY;
            }
            if args.iter().any(|a| a.is_nan()) {
                return f64::NAN;
            }
            let mut sum = 0.0;
            for a in args {
                sum += a * a;
            }
            sum.sqrt()
        }
    }
}

/// `hypot_n` for three arguments without a slice, the common case.
pub fn hypot3(x: f64, y: f64, z: f64) -> f64 {
    hypot_n(&[x, y, z])
}

#[cfg(test)]
mod tests {
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    use super::*;

    #[test]
    fn pow_follows_ecmascript() {
        assert!(pow(1.0, f64::NAN).is_nan());
        assert!(pow(1.0, f64::INFINITY).is_nan());
        assert!(pow(-1.0, f64::NEG_INFINITY).is_nan());
        assert_eq!(pow(f64::NAN, 0.0), 1.0);
        assert_eq!(pow(2.0, 10.0), 1024.0);
        assert_eq!(pow(0.5, f64::INFINITY), 0.0);
        assert_eq!(pow(-0.0, -1.0), f64::NEG_INFINITY);
    }

    #[test]
    fn hypot_follows_ecmascript() {
        assert_eq!(hypot(f64::NAN, f64::NEG_INFINITY), f64::INFINITY);
        assert!(hypot(f64::NAN, 1.0).is_nan());
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert_eq!(hypot_n(&[]), 0.0);
        assert_eq!(hypot_n(&[-3.0]), 3.0);
        assert_eq!(hypot_n(&[-0.0]).to_bits(), 0.0f64.to_bits());
        assert_eq!(hypot_n(&[2.0, 3.0, 6.0]), 7.0);
        assert_eq!(hypot_n(&[f64::NAN, 1.0, f64::NEG_INFINITY]), f64::INFINITY);
        assert!(hypot_n(&[f64::NAN, 1.0, 2.0]).is_nan());
    }

    #[test]
    fn signed_zeros_survive() {
        for f in [sin as fn(f64) -> f64, tan, asin, atan, tanh] {
            assert_eq!(f(-0.0).to_bits(), (-0.0f64).to_bits());
        }
        assert_eq!(atan2(-0.0, 1.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(atan2(0.0, -1.0), core::f64::consts::PI);
    }
}
