//! `mr_math::kernel` compiled to wasm for the JS reference run (SPEC 4.2,
//! roadmap WP 0.3).
//!
//! `src/parity/kernel.js` instantiates `mr_kernel.wasm` and replaces
//! `Math.sin` and the rest with these exports, so the JS oracle computes
//! every inexact function with the same bits as the Rust port.
//!
//! The crate also holds the input generator for the bit check (`cargo xtask
//! kernel`): native Rust and the JS-patched `Math` evaluate every function on
//! the same million inputs and must hash identically.

/// The functions the check covers, in a fixed order. `hypot3` is the
/// three-argument `Math.hypot`, which the JS side computes in its wrapper.
pub const FUNCTIONS: &[(&str, u32)] = &[
    ("sin", 1),
    ("cos", 1),
    ("tan", 1),
    ("asin", 1),
    ("acos", 1),
    ("atan", 1),
    ("atan2", 2),
    ("exp", 1),
    ("log", 1),
    ("log2", 1),
    ("log10", 1),
    ("pow", 2),
    ("tanh", 1),
    ("hypot", 2),
    ("hypot3", 3),
];

/// Evaluates function `f` (an index into [`FUNCTIONS`]) on `args`, natively.
pub fn eval(f: usize, args: &[f64]) -> f64 {
    use mr_math::kernel as k;
    match FUNCTIONS[f].0 {
        "sin" => k::sin(args[0]),
        "cos" => k::cos(args[0]),
        "tan" => k::tan(args[0]),
        "asin" => k::asin(args[0]),
        "acos" => k::acos(args[0]),
        "atan" => k::atan(args[0]),
        "atan2" => k::atan2(args[0], args[1]),
        "exp" => k::exp(args[0]),
        "log" => k::log(args[0]),
        "log2" => k::log2(args[0]),
        "log10" => k::log10(args[0]),
        "pow" => k::pow(args[0], args[1]),
        "tanh" => k::tanh(args[0]),
        "hypot" => k::hypot(args[0], args[1]),
        "hypot3" => k::hypot3(args[0], args[1], args[2]),
        other => unreachable!("{other}"),
    }
}

/// Values every function is tried on, beside the random ones.
const SPECIALS: &[f64] = &[
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    -0.5,
    2.0,
    10.0,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
    f64::MIN_POSITIVE,
    -f64::MIN_POSITIVE,
    5e-324,
    -5e-324,
    f64::MAX,
    -f64::MAX,
    f64::EPSILON,
    core::f64::consts::PI,
    -core::f64::consts::PI,
    core::f64::consts::FRAC_PI_2,
    -core::f64::consts::FRAC_PI_2,
    core::f64::consts::FRAC_PI_4,
    core::f64::consts::E,
    709.782712893384,
    -745.1332191019411,
    1e300,
    1e-300,
    0.49999999999999994,
    1.0000000000000002,
    0.9999999999999999,
];

/// SplitMix64's finaliser: a stateless hash, so input `i` of function `f`
/// can be produced on either side without replaying a stream.
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A uniform double in [0, 1) from the top 53 bits.
fn unit(r: u64) -> f64 {
    (r >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// Argument `arg` of input `i` for function `f`. Five kinds in turn: any bit
/// pattern (NaNs, infinities, subnormals, huge), [-1, 1), angles in [-8, 8),
/// any sign and magnitude from 2^-64 to 2^64, and the special values.
pub fn input(f: u32, i: u32, arg: u32) -> f64 {
    let r = mix(((f as u64) << 40) ^ ((i as u64) << 8) ^ arg as u64);
    match i % 5 {
        0 => f64::from_bits(r),
        1 => unit(r) * 2.0 - 1.0,
        2 => (unit(r) * 2.0 - 1.0) * 8.0,
        3 => {
            let sign = if r & 1 == 0 { 1.0 } else { -1.0 };
            let e = ((r >> 1) % 129) as i32 - 64;
            let m = 1.0 + unit(mix(r));
            // 2^e built from its bits, so no pow is involved.
            let scale = f64::from_bits(((1023 + e) as u64) << 52);
            sign * m * scale
        }
        _ => SPECIALS[(r % SPECIALS.len() as u64) as usize],
    }
}

/// The hash both sides compute over a function's outputs: FNV-1a over 32-bit
/// words (low word first), with every NaN taken as the canonical quiet NaN,
/// since wasm does not fix NaN payloads.
pub fn hash_step(h: u32, y: f64) -> u32 {
    let bits = if y.is_nan() {
        0x7FF8_0000_0000_0000
    } else {
        y.to_bits()
    };
    let h = (h ^ bits as u32).wrapping_mul(0x0100_0193);
    (h ^ (bits >> 32) as u32).wrapping_mul(0x0100_0193)
}

pub const HASH_SEED: u32 = 0x811C_9DC5;

/// The native side of the check: the hash of function `f` over `n` inputs.
pub fn native_hash(f: usize, n: u32) -> u32 {
    let arity = FUNCTIONS[f].1;
    let mut h = HASH_SEED;
    let mut args = [0.0; 3];
    for i in 0..n {
        for a in 0..arity {
            args[a as usize] = input(f as u32, i, a);
        }
        h = hash_step(h, eval(f, &args[..arity as usize]));
    }
    h
}

/// The wasm exports. Prefixed, so a native build never defines a symbol
/// that the C library also defines.
#[cfg(target_arch = "wasm32")]
mod exports {
    use mr_math::kernel as k;

    macro_rules! export1 {
        ($($name:ident => $f:path),* $(,)?) => {$(
            #[unsafe(no_mangle)]
            pub extern "C" fn $name(x: f64) -> f64 { $f(x) }
        )*};
    }
    macro_rules! export2 {
        ($($name:ident => $f:path),* $(,)?) => {$(
            #[unsafe(no_mangle)]
            pub extern "C" fn $name(x: f64, y: f64) -> f64 { $f(x, y) }
        )*};
    }

    export1! {
        k_sin => k::sin, k_cos => k::cos, k_tan => k::tan,
        k_asin => k::asin, k_acos => k::acos, k_atan => k::atan,
        k_exp => k::exp, k_log => k::log, k_log2 => k::log2, k_log10 => k::log10,
        k_tanh => k::tanh,
    }
    export2! { k_atan2 => k::atan2, k_pow => k::pow, k_hypot => k::hypot }

    /// The check's inputs, so the JS side need not reimplement the generator.
    #[unsafe(no_mangle)]
    pub extern "C" fn k_input(f: u32, i: u32, arg: u32) -> f64 {
        super::input(f, i, arg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_cover_the_kinds() {
        let xs: Vec<f64> = (0..1000).map(|i| input(0, i, 0)).collect();
        assert!(xs.iter().any(|x| x.is_nan()));
        assert!(xs.iter().any(|x| x.is_infinite()));
        assert!(xs.iter().any(|x| *x == 0.0 && x.is_sign_negative()));
        assert!(xs.iter().filter(|x| x.abs() <= 1.0).count() > 200);
    }

    #[test]
    fn hypot3_matches_the_wrapper_rule() {
        assert_eq!(eval(14, &[2.0, 3.0, 6.0]), 7.0);
    }
}
