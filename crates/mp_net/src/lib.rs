//! Multiplayer: protocol, transports, the authoritative session and client
//! prediction with rollback (SPEC 9; the rules are `docs/rust-port/
//! MULTIPLAYER.md`).
//!
//! - [`proto`]: the messages and their binary encoding.
//! - [`transport`]: the `Transport` trait and the in-process network the
//!   tests and the single-player loopback use.
//! - [`host`]: the lobby, the authoritative race and the points table.
//! - [`client`]: a player's session: prediction, rollback, the clock.
//! - [`signal`]: joining by link: the room, its key, the sealed
//!   signalling and the star around the host (M11).
//! - [`rtc`] (feature `rtc`): the WebRTC transport.

#![forbid(unsafe_code)]

pub mod client;
pub mod host;
pub mod proto;
#[cfg(feature = "rtc")]
pub mod rtc;
pub mod signal;
pub mod transport;
