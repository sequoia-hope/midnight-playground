//! The exports `crates/mp_audio/web/music-worklet.js` calls: the station
//! player as its own wasm on the audio thread (no wasm-bindgen; plain
//! numbers and one pointer), as `mp_exhaust::wasm` does for the engine.
//!
//! The worklet creates one player per node with [`mpm_new`], forwards the
//! node's parameters with [`mpm_params`] once per block, and calls
//! [`mpm_process`], which renders 128 frames into the player's own buffer
//! (left then right) and returns its address in linear memory.

use crate::json::{Val, canon};
use crate::player::Player;
use crate::radio::{STATIONS, block, block_at, cue_in, wall_from};
use std::cell::RefCell;

/// The worklet's block.
pub const BLOCK: usize = 128;

thread_local! {
    /// The last string an export made, for the page to read (one thread).
    static TEXT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn give(text: String) -> *const u8 {
    TEXT.with(|t| {
        *t.borrow_mut() = text.into_bytes();
        t.borrow().as_ptr()
    })
}

/// The length of the string the last [`mpm_stations`] or [`mpm_schedule`]
/// returned.
#[unsafe(no_mangle)]
pub extern "C" fn mpm_text_len() -> usize {
    TEXT.with(|t| t.borrow().len())
}

/// The stations as JSON (`[{key, name, freq, dj, genres: [[key, weight]]}]`),
/// UTF-8 at the returned address, [`mpm_text_len`] bytes. For the radio page
/// (`tools/radio.html`), which runs this wasm on the main thread too.
#[unsafe(no_mangle)]
pub extern "C" fn mpm_stations() -> *const u8 {
    let v = Val::Arr(
        STATIONS
            .iter()
            .map(|s| {
                Val::obj(vec![
                    ("key", Val::str(s.key)),
                    ("name", Val::str(s.name)),
                    ("freq", Val::str(s.freq)),
                    ("dj", s.dj.map(|d| Val::Str(d.to_owned()))),
                    (
                        "genres",
                        Some(Val::Arr(
                            s.genres
                                .iter()
                                .map(|(g, w)| {
                                    Val::Arr(vec![Val::Str((*g).to_owned()), Val::Num(*w)])
                                })
                                .collect(),
                        )),
                    ),
                ])
            })
            .collect(),
    );
    give(canon(&v))
}

/// A station's programme at a wall time, as JSON: `{block, into, slot,
/// step, frac, left, slots: [{genre, seed, start, secs, bpm, bars, title,
/// style}]}` (`into`: seconds into the block; `slot`: the song playing).
#[unsafe(no_mangle)]
pub extern "C" fn mpm_schedule(station: i32, wall_day: f32, wall_sec: f32) -> *const u8 {
    let Some(st) = usize::try_from(station).ok().and_then(|i| STATIONS.get(i)) else {
        return give("null".to_owned());
    };
    let wall = wall_from(wall_day as f64, wall_sec as f64);
    let (b, into) = block_at(wall);
    let slots = block(st, b);
    let c = cue_in(b, &slots, into);
    let v = Val::obj(vec![
        ("block", Val::num(b as f64)),
        ("into", Val::num(into)),
        ("slot", Val::num(c.slot as f64)),
        ("step", Val::num(c.step as f64)),
        ("frac", Val::num(c.frac)),
        ("left", Val::num(c.left)),
        (
            "slots",
            Some(Val::Arr(
                slots
                    .iter()
                    .map(|s| {
                        Val::obj(vec![
                            ("genre", Val::str(s.genre)),
                            ("seed", Val::num(s.seed as f64)),
                            ("start", Val::num(s.start)),
                            ("secs", Val::num(s.secs)),
                            ("bpm", Val::num(s.track.bpm)),
                            ("bars", Val::num(s.track.bars() as f64)),
                            ("title", Val::ostr(&s.track.title)),
                            ("style", Val::ostr(&s.track.style)),
                        ])
                    })
                    .collect(),
            )),
        ),
    ]);
    give(canon(&v))
}

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
