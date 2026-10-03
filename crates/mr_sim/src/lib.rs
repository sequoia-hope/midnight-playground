//! The simulation: a plain `SimState` stepped by one fixed tick of 1/120 s.
//! No engine, no renderer, no clock and no global state (SPEC 1.1, 4).

#![forbid(unsafe_code)]
// Index loops stay index loops: they mirror the JS line for line (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

pub mod autopilot;
pub mod dims;
pub mod input;
pub mod physics;
pub mod staged;
pub mod trace;
pub mod vehicle;
