//! Audio: a facade shaped like Web Audio with web, native and null backends,
//! and the game's engine, SFX, music and radio on top of it (SPEC 7).
//!
//! - [`wa`]: the facade (roadmap WP 5.1). Backends are features: `null`
//!   (default; records calls, strict validation), `web` (the browser's Web
//!   Audio through `web-sys`, wasm only), `native` (the `web-audio-api`
//!   crate).
//! - The generated audio data (WP 5.2), bit-identical to the JS arrays:
//!   [`samples`] (`samples.js`: the SFX buffers and the drum kit),
//!   [`noise`] (the noise beds and the tunnel's impulse response),
//!   [`engine`] (car profiles, engine and rumble cycles), [`shapes`]
//!   (wave-shaper curves, pulse and soft-square waves), [`music`] (the
//!   music's pulse wave and hall impulse response).
//! - [`reference`]: every array of the audio reference
//!   (`parity/golden/audio/arrays.json`), built in the JS order.

#![forbid(unsafe_code)]
// A JS index loop stays an index loop, so the port reads beside the JS line
// for line (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

pub mod engine;
pub mod music;
pub mod noise;
pub mod reference;
pub mod samples;
pub mod shapes;
pub mod wa;

use sha2::{Digest, Sha256};

/// `'#'` + the first 16 hex digits of the SHA-256 of an array's
/// little-endian float32 bytes: how the audio reference names arrays
/// (`parity/golden/audio/README.md`).
pub fn array_hash(a: &[f32]) -> String {
    let mut h = Sha256::new();
    for x in a {
        h.update(x.to_le_bytes());
    }
    let d = h.finalize();
    let mut s = String::with_capacity(17);
    s.push('#');
    for b in &d[..8] {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
