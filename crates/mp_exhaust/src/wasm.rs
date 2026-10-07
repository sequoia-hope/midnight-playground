//! The exports `web/exhaust-worklet.js` calls: the model as its own wasm on
//! the audio thread (no wasm-bindgen; plain numbers and one pointer).
//!
//! The worklet creates one engine per node with [`mpx_new`], forwards the
//! node's parameters with [`mpx_state`] and [`mpx_run`] once per block, and
//! calls [`mpx_process`], which renders 128 frames into the engine's own
//! buffer (left then right) and returns its address in linear memory.

use crate::{Engine, ORDER, State, preset};

/// The worklet's block.
pub const BLOCK: usize = 128;

pub struct Worklet {
    engine: Engine,
    buf: [f32; 2 * BLOCK],
}

/// A new engine for preset `index` (in [`ORDER`]; out of range is the
/// first) at `sample_rate`, idling. A car change (the node's `preset`
/// param) frees it with [`mpx_free`] and makes another.
#[unsafe(no_mangle)]
pub extern "C" fn mpx_new(index: u32, sample_rate: f64, seed: u32) -> *mut Worklet {
    let key = ORDER.get(index as usize).copied().unwrap_or(ORDER[0]);
    let p = preset(key).expect("ORDER names presets");
    let idle = p.idle;
    let engine = Engine::new(
        p,
        State {
            rpm: idle,
            ..State::default()
        },
        seed,
        sample_rate,
    );
    Box::into_raw(Box::new(Worklet {
        engine,
        buf: [0.0; 2 * BLOCK],
    }))
}

/// Frees an engine made by [`mpx_new`].
///
/// # Safety
/// `w` must come from [`mpx_new`] and not be used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpx_free(w: *mut Worklet) {
    if !w.is_null() {
        drop(unsafe { Box::from_raw(w) });
    }
}

/// # Safety
/// `w` must come from [`mpx_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpx_state(
    w: *mut Worklet,
    rpm: f64,
    throttle: f64,
    boost: f64,
    speed: f64,
) {
    let w = unsafe { &mut *w };
    w.engine.set_state(
        State {
            rpm,
            throttle,
            boost,
            speed,
        },
        false,
    );
}

/// Sets the state at once (`setState` with `jump`): an engine made for a
/// car change starts where the old one was, as `Engine::new` with that
/// state would.
///
/// # Safety
/// `w` must come from [`mpx_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpx_jump(
    w: *mut Worklet,
    rpm: f64,
    throttle: f64,
    boost: f64,
    speed: f64,
) {
    let w = unsafe { &mut *w };
    w.engine.set_state(
        State {
            rpm,
            throttle,
            boost,
            speed,
        },
        true,
    );
}

/// # Safety
/// `w` must come from [`mpx_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpx_run(w: *mut Worklet, on: u32) {
    unsafe { &mut *w }.engine.set_running(on != 0);
}

/// # Safety
/// `w` must come from [`mpx_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpx_psycho(w: *mut Worklet, amount: f64) {
    unsafe { &mut *w }.engine.psycho = amount;
}

/// Renders one block; returns the address of 128 left then 128 right
/// samples.
///
/// # Safety
/// `w` must come from [`mpx_new`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpx_process(w: *mut Worklet) -> *const f32 {
    let w = unsafe { &mut *w };
    let (l, r) = w.buf.split_at_mut(BLOCK);
    w.engine.process(l, r);
    w.buf.as_ptr()
}
