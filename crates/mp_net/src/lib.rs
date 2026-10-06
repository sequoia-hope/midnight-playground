//! Multiplayer: protocol, transports, the authoritative session and client
//! prediction with rollback (SPEC 9; the rules are `docs/rust-port/
//! MULTIPLAYER.md`).
//!
//! - [`proto`]: the messages and their binary encoding.
//! - [`transport`]: the `Transport` trait and the in-process network the
//!   tests and the single-player loopback use.

#![forbid(unsafe_code)]

pub mod proto;
pub mod transport;
