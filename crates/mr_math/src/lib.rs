//! Scalar math on `libm`: the kernel that both the Rust and the JS reference
//! run use for every inexact `Math` function, the JavaScript-semantics helpers,
//! `mulberry32`, `hash2` and the noise functions (SPEC 4.2; port of
//! `src/util/math.js`).
//!
//! No `f64::sin` and friends anywhere downstream: they use the platform
//! library on native and would break bit-identical results.

#![forbid(unsafe_code)]
