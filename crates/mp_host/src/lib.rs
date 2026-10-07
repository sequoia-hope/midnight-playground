//! What `mp-host` and `mp-signal` share: the port rule and the signalling
//! relay (MULTIPLAYER 8; DECISIONS D1123).

#![forbid(unsafe_code)]

pub mod signal;

use std::process::Command;

/// The port to listen on: `arg` (`--port`), then `$PORT`, then
/// `proj port`; otherwise the program stops with an error. There is no
/// default (CLAUDE.md).
pub fn port(prog: &str, arg: Option<u16>) -> u16 {
    if let Some(p) = arg {
        return p;
    }
    if let Ok(p) = std::env::var("PORT") {
        return p.parse().unwrap_or_else(|_| {
            eprintln!("{prog}: $PORT is not a port: {p:?}");
            std::process::exit(2)
        });
    }
    let proj = std::env::var("HOME")
        .map(|h| format!("{h}/scripts/proj"))
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
        .unwrap_or_else(|| "proj".into());
    match Command::new(proj).arg("port").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse()
            .unwrap_or_else(|_| {
                eprintln!("{prog}: `proj port` gave no port");
                std::process::exit(2)
            }),
        _ => {
            eprintln!("{prog}: no --port, no $PORT, and `proj port` failed: refusing to guess one");
            std::process::exit(2)
        }
    }
}
