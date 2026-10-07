//! `mp-signal`: the signalling server for joining by link (MULTIPLAYER
//! 8.3, DECISIONS D1123). It introduces WebRTC peers in a room and relays
//! their sealed messages; the game's traffic never passes through it.
//!
//! Run it behind a reverse proxy that gives it https (Caddy:
//! `reverse_proxy` to it), then point the game at `wss://<name>/`.
//!
//! The port: `--port`, then `$PORT`, then `proj port`, else it stops with
//! an error. It logs nothing but the line it starts with.

#![forbid(unsafe_code)]

use std::net::TcpListener;

use mp_host::signal::{Rooms, accept};

fn usage() -> ! {
    eprintln!(
        "usage: mp-signal [--port N] [--bind ADDR]\n\
         The port is --port, else $PORT, else `proj port`; there is no default.\n\
         Clients connect with a WebSocket to /<room> (any path; its last segment is the room)."
    );
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut port_arg, mut bind) = (None, "127.0.0.1".to_string());
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => {
                port_arg = Some(
                    args.next()
                        .and_then(|p| p.parse().ok())
                        .unwrap_or_else(|| usage()),
                )
            }
            "--bind" => bind = args.next().unwrap_or_else(|| usage()),
            _ => usage(),
        }
    }
    let port = mp_host::port("mp-signal", port_arg);
    let listener = TcpListener::bind((bind.as_str(), port)).unwrap_or_else(|e| {
        eprintln!("mp-signal: can't listen on {bind}:{port}: {e}");
        std::process::exit(1)
    });
    println!("mp-signal: signalling on ws://{bind}:{port}/<room>");
    let rooms = Rooms::new();
    for stream in listener.incoming().flatten() {
        let rooms = rooms.clone();
        std::thread::spawn(move || accept(stream, &rooms));
    }
}
