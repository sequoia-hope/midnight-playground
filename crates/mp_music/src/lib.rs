//! The Music Lab (`tools/music-lab/`) ported line for line: the DSP
//! blocks, the instruments and kits, the sequencer, the composer, the genre
//! grammars and the engine that mixes a song (docs/vision/sound.md 3, and
//! 3.5 for the genres). It is the music of the radio stations (radio.md 7):
//! a station is a deterministic schedule of generated songs on the wall
//! clock, so every player hears the same song at the same moment.
//!
//! The lab is the oracle: `parity/golden/music/` (written by
//! `node tools/music-lab/test/golden.mjs`) holds every grammar's tracks as
//! canonical JSON hashes, the sequencer's events for a few tracks, and
//! renders; the tests here reproduce the first two exactly and the renders
//! to a tolerance. The lab uses `Math.*`; this crate uses the platform's
//! `f64` functions, which may differ in the last bit. Audio is not
//! simulation, so `mp_math`'s kernel is not required here (as in
//! `mp_exhaust`).
//!
//! Port rules (DECISIONS D1150):
//! - Same structure, names, constants, order of operations and order of
//!   random draws as the JS. A JS object whose keys are iterated is a `Vec`
//!   of pairs in insertion order, never a map.
//! - JS semantics kept: `Math.round` is `(x + 0.5).floor()`; `|0`, `>>> 0`
//!   and `^` are ToInt32 / ToUint32 of a double; `%` keeps the dividend's
//!   sign; `x || d` treats 0 as missing; `?? d` only `undefined`/`null`.
//! - Stores into a `Float32Array` round to `f32`; everything else is `f64`.
//!
//! On the web the engine runs as its own small wasm inside an AudioWorklet
//! (see [`wasm`] and `crates/mp_audio/web/music-worklet.js`); natively the
//! same engine runs in `mp_audio`'s worklet processor.

// Only the worklet wasm's C-ABI exports (`wasm`) touch raw pointers.
#![cfg_attr(not(feature = "worklet"), forbid(unsafe_code))]
// A JS index loop stays an index loop, so the port reads beside the JS line
// for line (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

pub mod compose;
pub mod dsp;
pub mod engine;
pub mod genres;
pub mod instruments;
pub mod json;
pub mod kits;
pub mod patches;
pub mod player;
pub mod radio;
pub mod seq;
pub mod track;
#[cfg(all(target_arch = "wasm32", feature = "worklet"))]
pub mod wasm;

/// JS `Math.round`: `floor(x + 0.5)` (half rounds up, also for negatives).
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// JS `ToUint32` of a double: modulo 2^32 of its truncation.
pub fn to_u32(x: f64) -> u32 {
    (x.trunc().rem_euclid(4294967296.0)) as u32
}

/// JS `ToInt32` of a double.
pub fn to_i32(x: f64) -> i32 {
    to_u32(x) as i32
}

/// JS `Math.imul` on two int32 values given as `u32` bit patterns.
pub fn imul(a: u32, b: u32) -> u32 {
    a.wrapping_mul(b)
}
