//! WebRTC end to end, natively (MULTIPLAYER 8.2): `mp-signal` on an
//! ephemeral port, a `Host` and two `Client`s on `RtcNet` in this process
//! (webrtc-rs over the machine's own addresses), from the lobby into a
//! race with inputs and hashes flowing; a guest with the wrong secret
//! never gets in.

use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mp_net::client::Client;
use mp_net::host::Host;
use mp_net::proto::{AiFill, Settings};
use mp_net::rtc::{IceServers, RtcNet, RtcOptions, RtcStatus, new_link};
use mp_net::signal::{JoinLink, Role};
use mp_net::transport::{Channel, NetEvent, Transport};
use mp_sim::input::InputFrame;
use mp_sim::race::LevelRuntime;

struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `mp-signal` on a free port; its address for the clients.
fn signal_server() -> (Proc, String) {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = Command::new(env!("CARGO_BIN_EXE_mp-signal"))
        .args(["--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let t = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(t.elapsed() < Duration::from_secs(20), "mp-signal came up");
        std::thread::sleep(Duration::from_millis(50));
    }
    (Proc(child), format!("ws://127.0.0.1:{port}/"))
}

/// No STUN: the peers find each other on this machine's own addresses, so
/// the test needs no internet.
fn opts(signal: &str, link: &JoinLink, role: Role) -> RtcOptions {
    RtcOptions {
        signal: signal.into(),
        link: link.clone(),
        role,
        ice: Some(IceServers {
            urls: Vec::new(),
            username: None,
            credential: None,
        }),
    }
}

/// Polls until `done` or the time is up.
fn until(secs: u64, mut step: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if step() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

/// The raw transport: a host and a guest connect, bytes go both ways on
/// both channels, and the guest sees the host as peer 0.
#[test]
fn a_host_and_a_guest_connect_and_talk_on_both_channels() {
    let (_s, signal) = signal_server();
    let link = new_link();
    let mut host = RtcNet::open(opts(&signal, &link, Role::Host));
    let mut guest = RtcNet::open(opts(&signal, &link, Role::Guest));
    let (mut he, mut ge) = (Vec::new(), Vec::new());
    let ok = until(30, || {
        host.poll(&mut he);
        guest.poll(&mut ge);
        !he.is_empty() && !ge.is_empty()
    });
    assert!(
        ok,
        "connected: host {he:?}, guest {ge:?}, {:?}",
        guest.status()
    );
    assert_eq!(he, vec![NetEvent::Connected(1)]);
    assert_eq!(ge, vec![NetEvent::Connected(0)]);
    assert_eq!(guest.status(), RtcStatus::FoundHost);
    assert_eq!(host.status(), RtcStatus::InRoom);

    guest.send(0, Channel::Reliable, b"hello host");
    guest.send(0, Channel::Unreliable, b"input");
    host.send(1, Channel::Reliable, &vec![7u8; 20_000]);
    let (mut he, mut ge) = (Vec::new(), Vec::new());
    until(10, || {
        host.poll(&mut he);
        guest.poll(&mut ge);
        he.len() >= 2 && !ge.is_empty()
    });
    assert!(
        he.contains(&NetEvent::Message(1, b"hello host".to_vec())),
        "{he:?}"
    );
    assert!(
        he.contains(&NetEvent::Message(1, b"input".to_vec())),
        "{he:?}"
    );
    assert_eq!(ge, vec![NetEvent::Message(0, vec![7u8; 20_000])]);

    // The guest leaves: the host sees it go.
    drop(guest);
    let mut he = Vec::new();
    assert!(
        until(30, || {
            host.poll(&mut he);
            he.contains(&NetEvent::Disconnected(1))
        }),
        "{he:?}"
    );
}

/// Someone with the room but not the secret: the host never offers, so
/// they never connect, and the host sees no one.
#[test]
fn a_wrong_secret_never_connects() {
    let (_s, signal) = signal_server();
    let link = new_link();
    let mut host = RtcNet::open(opts(&signal, &link, Role::Host));
    let mut forged = link.clone();
    forged.secret[3] ^= 0x10;
    let mut stranger = RtcNet::open(opts(&signal, &forged, Role::Guest));
    let (mut he, mut se) = (Vec::new(), Vec::new());
    until(5, || {
        host.poll(&mut he);
        stranger.poll(&mut se);
        false
    });
    assert!(he.is_empty(), "{he:?}");
    assert!(se.is_empty(), "{se:?}");
    assert_eq!(stranger.status(), RtcStatus::InRoom, "no host found");
    // A real guest still gets in.
    let mut guest = RtcNet::open(opts(&signal, &link, Role::Guest));
    let mut ge = Vec::new();
    assert!(until(30, || {
        host.poll(&mut he);
        guest.poll(&mut ge);
        stranger.poll(&mut se);
        !ge.is_empty()
    }));
    assert_eq!(ge, vec![NetEvent::Connected(0)]);
    assert_eq!(he, vec![NetEvent::Connected(1)]);
    assert!(se.is_empty());
}

/// The game's session over WebRTC: two players meet in the host's lobby,
/// the leader starts a race, inputs (on the unreliable channel) reach the
/// host, the host's inputs and hashes reach both, and nobody desyncs.
#[test]
fn two_players_race_over_webrtc() {
    let (_s, signal) = signal_server();
    let link = new_link();
    let lr = Arc::new(LevelRuntime::new(mp_levels::level_by_id("coast")).unwrap());
    let lv = lr.clone();
    let mut host = Host::new(
        RtcNet::open(opts(&signal, &link, Role::Host)),
        Rc::new(move |id: &str| (id == "coast").then(|| lv.clone())),
        5,
    );
    let mut clients: Vec<Client<RtcNet>> = ["Ann", "Bob"]
        .iter()
        .map(|n| {
            Client::new(
                RtcNet::open(opts(&signal, &link, Role::Guest)),
                n,
                "rally",
                1,
            )
        })
        .collect();
    let t0 = Instant::now();
    let now = || t0.elapsed().as_secs_f64() * 1000.0;
    let full = InputFrame {
        throttle: 255,
        ..InputFrame::default()
    };
    let frame = |host: &mut Host<RtcNet>, clients: &mut Vec<Client<RtcNet>>| {
        let t = now();
        host.update(t);
        for c in clients.iter_mut() {
            c.update(t, |_, _| full);
        }
    };
    assert!(
        until(30, || {
            frame(&mut host, &mut clients);
            clients
                .iter()
                .all(|c| c.slot.is_some() && c.lobby.players.len() == 2)
        }),
        "both joined the lobby"
    );
    let leader = clients.iter().position(|c| c.is_leader()).unwrap();
    clients[leader].configure(Settings {
        ai: AiFill::None,
        ..Settings::default()
    });
    clients[leader].go(true);
    assert!(until(10, || {
        frame(&mut host, &mut clients);
        clients.iter().all(|c| c.pending.is_some())
    }));
    for c in &mut clients {
        assert!(c.attach(lr.clone()));
    }
    // Six seconds of race.
    until(8, || {
        frame(&mut host, &mut clients);
        host.tick() > 600
    });
    assert!(host.tick() > 600, "the race ran: {}", host.tick());
    for c in &clients {
        let r = c.race.as_ref().unwrap();
        assert!(r.confirmed() > 400, "{}: {}", c.name, r.confirmed());
        assert!(c.stats.hashes_checked > 5, "{:?}", c.stats);
        assert_eq!(c.stats.desyncs, 0, "{:?}", c.stats);
        assert!(c.stats.rtt < 200.0, "{:?}", c.stats);
    }
    // Both players' throttles reached the host, on the unreliable channel.
    let (log, n) = host.input_log().unwrap();
    assert_eq!(n, 2);
    let late = &log[log.len() - 2 * 60..];
    assert!(late.iter().all(|f| f.throttle == 255), "{:?}", &late[..4]);
}

/// `mp-host --room`: the native host opens a room on the signalling
/// server and prints the link; a guest joining by that link over WebRTC
/// meets a WebSocket player in the same lobby.
#[test]
fn mp_host_takes_webrtc_guests_by_its_link() {
    use mp_net::proto::{Msg, VERSION};
    use std::io::{BufRead, BufReader};
    let (_s, signal) = signal_server();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mp-host"))
        .args(["--port", &port.to_string(), "--bind", "127.0.0.1", "--root"])
        .arg(&root)
        .args(["--room", "--signal", &signal])
        .args(["--page", "https://example.test/game/"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let _h = Proc(child);
    let mut lines = BufReader::new(out).lines();
    let url = loop {
        let l = lines.next().expect("mp-host printed its link").unwrap();
        if let Some((_, u)) = l.split_once("join at ") {
            break u.to_string();
        }
    };
    let q = mp_net::signal::query_escape(&signal);
    assert!(
        url.starts_with(&format!("https://example.test/game/?signal={q}#join=")),
        "{url}"
    );
    let link = JoinLink::parse(&url).expect("a link");

    // One player over WebSocket, one over WebRTC.
    let (mut ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}/ws")).unwrap();
    let hello = Msg::Hello {
        version: VERSION,
        name: "Wired".into(),
        car: "sports".into(),
        color: 1,
    };
    ws.send(tungstenite::Message::Binary(hello.encode().into()))
        .unwrap();
    let mut c = Client::new(
        RtcNet::open(opts(&signal, &link, Role::Guest)),
        "Wireless",
        "rally",
        2,
    );
    let t0 = Instant::now();
    assert!(
        until(30, || {
            let t = t0.elapsed().as_secs_f64() * 1000.0;
            c.update(t, |_, _| InputFrame::default());
            c.lobby.players.len() == 2
        }),
        "the WebRTC guest is in the lobby: {:?}",
        c.lobby.players
    );
    let names: Vec<&str> = c.lobby.players.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Wired", "Wireless"]);
    assert_eq!(c.slot, Some(1));
}
