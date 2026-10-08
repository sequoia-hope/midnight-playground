//! The exports `crates/mp_audio/web/music-worklet.js` calls: the station
//! player as its own wasm on the audio thread (no wasm-bindgen; plain
//! numbers and one pointer), as `mp_exhaust::wasm` does for the engine.
//!
//! The worklet creates one player per node with [`mpm_new`], forwards the
//! node's parameters with [`mpm_params`] once per block, and calls
//! [`mpm_process`], which renders 128 frames into the player's own buffer
//! (left then right) and returns its address in linear memory.

use crate::radio::Player;

/// The worklet's block.
pub const BLOCK: usize = 128;

pub struct Worklet {
    player: Player,
    buf: [f32; 2 * BLOCK],
}

/// A new player at `sample_rate`, off (silent) until tuned.
#[unsafe(no_mangle)]
pub extern "C" fn mpm_new(sample_rate: f64, seed: u32) -> *mut Worklet {
    Box::into_raw(Box::new(Worklet {
        player: Player::new(seed, sample_rate),
        buf: [0.0; 2 * BLOCK],
    }))
}

/// The node's params for this block (see the worklet's descriptors).
#[unsafe(no_mangle)]
pub extern "C" fn mpm_params(
    w: *mut Worklet,
    station: f32,
    wall_day: f32,
    wall_sec: f32,
    tune: f32,
    energy: f32,
) {
    // SAFETY: `w` came from `mpm_new` and has not been freed; the worklet
    // calls these from one thread, one at a time.
    let w = unsafe { &mut *w };
    w.player
        .set_params(station, wall_day, wall_sec, tune, energy);
}

/// Renders one block; returns the address of 2 × [`BLOCK`] `f32`s.
#[unsafe(no_mangle)]
pub extern "C" fn mpm_process(w: *mut Worklet) -> *const f32 {
    // SAFETY: as above.
    let w = unsafe { &mut *w };
    let (l, r) = w.buf.split_at_mut(BLOCK);
    w.player.process(l, r);
    w.buf.as_ptr()
}

/// Frees a player made by [`mpm_new`].
#[unsafe(no_mangle)]
pub extern "C" fn mpm_free(w: *mut Worklet) {
    if !w.is_null() {
        // SAFETY: `w` came from `mpm_new` and is freed once.
        drop(unsafe { Box::from_raw(w) });
    }
}
