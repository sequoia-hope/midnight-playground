//! `mp-host`: the native multiplayer host (SPEC 9.4, WP 10.5). One program
//! that
//!
//! - serves the repository's files the way `tools/serve.py` does (so the JS
//!   game, the Rust build at `dist/next/` and the tools all work from it),
//! - accepts WebSocket connections on any path ending in `/ws` (the game is
//!   always served under a sub-path, so clients connect relative to it),
//! - and runs the lobby and the authoritative race headless
//!   (`mp_net::host::Host`),
//! - relays WebRTC signalling at `…/signal/<room>` (DECISIONS D1123), so
//!   joining by link works on a LAN with no internet,
//! - and with `--room --signal <wss://…/>` opens a room on a signalling
//!   server, prints its link, and takes WebRTC guests (from GitHub Pages,
//!   no certificate needed) beside the WebSocket ones (D1126).
//!
//! The port: `--port`, then `$PORT`, then `proj port`, else it stops with
//! an error. There is no default (CLAUDE.md). On the tailnet, `serve.sh`'s
//! `tailscale serve` front gives it https, which browsers need for WebGPU;
//! `serve.sh` runs it instead of `tools/serve.py` when `MP_HOST=1`.

#![forbid(unsafe_code)]

mod http;
mod net;

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mp_net::host::{Host, HostEvent, Levels};
use mp_net::rtc::{RtcNet, RtcOptions};
use mp_net::signal::{JoinLink, Role};
use mp_net::transport::Mux;

/// Where the game is published (D1101): the link `--room` prints opens it.
const PAGES: &str = "https://sequoia-hope.github.io/midnight-racer/dist/next/";

/// The link to a room: the page, the signalling server in its query (the
/// page may not have it built in, D1124), the room and secret after `#`.
fn join_url(page: &str, signal: &str, link: &JoinLink) -> String {
    let page = page.split('#').next().unwrap_or(page);
    let sep = if page.contains('?') { '&' } else { '?' };
    format!(
        "{page}{sep}signal={}#{}",
        mp_net::signal::query_escape(signal),
        link.fragment()
    )
}

fn usage() -> ! {
    eprintln!(
        "usage: mp-host [--port N] [--bind ADDR] [--root DIR] [--room --signal URL [--page URL]]\n\
         The port is --port, else $PORT, else `proj port`; there is no default.\n\
         --room opens a WebRTC room on the signalling server --signal (wss://…/) and\n\
         prints the link to join it, on --page (the game's address; GitHub Pages's by default)."
    );
    std::process::exit(2)
}

/// The levels, with Seaside's survey data from the repository.
fn levels(root: &std::path::Path) -> Levels {
    use mp_sim::race::LevelRuntime;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    let survey = std::fs::read(root.join("assets/seaside/survey.bin"))
        .ok()
        .and_then(|b| mp_levels::SeasideData::parse(&b).ok())
        .map(Arc::new);
    if survey.is_none() {
        eprintln!(
            "mp-host: no assets/seaside/survey.bin under {}; Seaside Raceway is unavailable",
            root.display()
        );
    }
    let cache: RefCell<HashMap<String, Arc<LevelRuntime>>> = RefCell::default();
    Rc::new(move |id: &str| {
        if !mp_net::host::known_level(id) {
            return None;
        }
        if let Some(lr) = cache.borrow().get(id) {
            return Some(lr.clone());
        }
        let mut level = mp_levels::level_by_id(id);
        if id == "seaside" {
            mp_levels::seaside::prepare(&mut level, survey.clone()?);
        }
        let lr = Arc::new(LevelRuntime::new(level).ok()?);
        cache.borrow_mut().insert(id.into(), lr.clone());
        Some(lr)
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut port_arg, mut bind, mut root) = (None, "0.0.0.0".to_string(), None);
    let (mut room, mut signal, mut page) = (false, None, PAGES.to_string());
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
            "--root" => root = args.next().map(PathBuf::from),
            "--room" => room = true,
            "--signal" => signal = Some(args.next().unwrap_or_else(|| usage())),
            "--page" => page = args.next().unwrap_or_else(|| usage()),
            _ => usage(),
        }
    }
    // The repository root: --root, else the checkout the binary was built
    // in, else here.
    let root = root
        .or_else(|| {
            std::env::current_exe().ok().and_then(|e| {
                e.ancestors()
                    .find(|p| p.join("Cargo.toml").exists() && p.join("index.html").exists())
                    .map(PathBuf::from)
            })
        })
        .unwrap_or_else(|| PathBuf::from("."));
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    if room && signal.is_none() {
        eprintln!("mp-host: --room needs --signal wss://…/ (the signalling server)");
        std::process::exit(2)
    }
    let port = mp_host::port("mp-host", port_arg);
    let listener = TcpListener::bind((bind.as_str(), port)).unwrap_or_else(|e| {
        eprintln!("mp-host: can't listen on {bind}:{port}: {e}");
        std::process::exit(1)
    });
    println!(
        "mp-host: serving {} on http://{bind}:{port}/ (multiplayer WebSocket at …/ws, signalling at …/signal/<room>)",
        root.display()
    );

    let (ws, incoming) = net::WsNet::new();
    let files = Arc::new(http::Files::new(root.clone()));
    let rooms = mp_host::signal::Rooms::new();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let files = files.clone();
            let incoming = incoming.clone();
            let rooms = rooms.clone();
            std::thread::spawn(move || http::serve(stream, &files, &incoming, &rooms));
        }
    });
    let mut net = Mux::new();
    net.add(Box::new(ws));
    if let Some(signal) = signal.filter(|_| room) {
        let link = mp_net::rtc::new_link();
        println!(
            "mp-host: room open on {signal}; join at {}",
            join_url(&page, &signal, &link)
        );
        net.add(Box::new(RtcNet::open(RtcOptions {
            signal,
            link,
            role: Role::Host,
            ice: None,
        })));
    }

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u32);
    let mut host = Host::new(net, levels(&root), seed);
    let t0 = Instant::now();
    loop {
        let now = t0.elapsed().as_secs_f64() * 1000.0;
        host.update(now);
        for e in host.events.drain(..) {
            match e {
                HostEvent::Joined(s) => println!("mp-host: player {} joined", s + 1),
                HostEvent::Left(s) => println!("mp-host: player {} left", s + 1),
                HostEvent::RaceStarted { race } => println!("mp-host: race {} started", race + 1),
                HostEvent::RaceEnded { race } => println!("mp-host: race {} ended", race + 1),
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
