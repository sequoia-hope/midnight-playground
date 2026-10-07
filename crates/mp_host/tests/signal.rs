//! The signalling relay end to end (DECISIONS D1123): `mp-signal` on an
//! ephemeral port, and `mp-host`'s `…/signal/<room>`, with plain WebSocket
//! clients speaking matchbox's JSON.

use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

struct Proc(Child, u16);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_up(port: u16) {
    let t = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(t.elapsed() < Duration::from_secs(20), "the server came up");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn signal_server() -> Proc {
    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_mp-signal"))
        .args(["--port", &port.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_up(port);
    Proc(child, port)
}

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

fn join(port: u16, path: &str) -> Ws {
    let (mut ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}{path}")).unwrap();
    if let MaybeTlsStream::Plain(s) = ws.get_mut() {
        s.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
    }
    ws
}

/// The next JSON message, within two seconds.
fn next(ws: &mut Ws) -> Value {
    let t = Instant::now();
    loop {
        match ws.read() {
            Ok(Message::Text(t)) => return serde_json::from_str(&t).unwrap(),
            Ok(_) => {}
            Err(tungstenite::Error::Io(_)) => {
                assert!(t.elapsed() < Duration::from_secs(2), "a message came");
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// Nothing arrives for a quarter of a second.
fn quiet(ws: &mut Ws) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(250) {
        match ws.read() {
            Ok(Message::Text(t)) => panic!("unexpected {t}"),
            Ok(_) | Err(tungstenite::Error::Io(_)) => {}
            Err(e) => panic!("{e}"),
        }
    }
}

/// Whether the server has closed this connection (within two seconds).
fn closed(ws: &mut Ws) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(2) {
        match ws.read() {
            Ok(Message::Close(_)) | Err(tungstenite::Error::ConnectionClosed) => return true,
            Err(tungstenite::Error::Io(e)) if e.kind() != std::io::ErrorKind::WouldBlock => {
                if e.kind() != std::io::ErrorKind::TimedOut {
                    return true;
                }
            }
            Err(tungstenite::Error::Io(_)) | Ok(_) => {}
            Err(_) => return true,
        }
    }
    false
}

fn id(ws: &mut Ws) -> String {
    let m = next(ws);
    m["IdAssigned"]
        .as_str()
        .unwrap_or_else(|| panic!("{m}"))
        .to_string()
}

fn send(ws: &mut Ws, v: Value) {
    ws.send(Message::Text(v.to_string().into())).unwrap();
}

/// The relay's whole conversation, on any server that runs it.
fn conversation(port: u16, prefix: &str) {
    let room = format!("{prefix}/room-A_1");
    let mut a = join(port, &room);
    let ida = id(&mut a);
    quiet(&mut a);
    let mut b = join(port, &room);
    let idb = id(&mut b);
    assert_ne!(ida, idb);
    // Full mesh: the one already here hears of the newcomer; the newcomer
    // hears nothing.
    assert_eq!(next(&mut a), json!({ "NewPeer": idb }));
    quiet(&mut b);

    // Another room is another world.
    let mut x = join(port, &format!("{prefix}/room-B"));
    let idx = id(&mut x);
    quiet(&mut a);

    // Signals pass to their receiver with the sender filled in, whatever
    // the data (the game's is sealed text).
    send(
        &mut a,
        json!({ "Signal": { "receiver": idb, "data": "sealed…" } }),
    );
    assert_eq!(
        next(&mut b),
        json!({ "Signal": { "sender": ida, "data": "sealed…" } })
    );
    send(
        &mut b,
        json!({ "Signal": { "receiver": ida, "data": { "Offer": "sdp" } } }),
    );
    assert_eq!(
        next(&mut a),
        json!({ "Signal": { "sender": idb, "data": { "Offer": "sdp" } } })
    );
    // Not across rooms, not to a made-up id, and keep-alives say nothing.
    send(
        &mut a,
        json!({ "Signal": { "receiver": idx, "data": "x" } }),
    );
    send(
        &mut a,
        json!({ "Signal": { "receiver": "00000000-0000-0000-0000-000000000000", "data": "x" } }),
    );
    send(&mut a, json!("KeepAlive"));
    quiet(&mut x);
    quiet(&mut b);
    quiet(&mut a);

    // Leaving tells the others.
    drop(b);
    assert_eq!(next(&mut a), json!({ "PeerLeft": idb }));
    // A client that speaks something else is dropped.
    send(&mut x, json!({ "Hello": 1 }));
    assert!(closed(&mut x));
}

#[test]
fn mp_signal_introduces_relays_and_says_goodbye() {
    let s = signal_server();
    conversation(s.1, "");
    // Any path: its last segment is the room.
    conversation(s.1, "/deep/path");
}

#[test]
fn a_room_holds_sixteen_and_turns_away_the_seventeenth() {
    let s = signal_server();
    let mut peers: Vec<Ws> = (0..16).map(|_| join(s.1, "/full")).collect();
    for p in &mut peers {
        id(p);
    }
    let mut late = join(s.1, "/full");
    assert!(closed(&mut late), "a full room says no");
    // One leaves: there's room again.
    drop(peers.pop());
    std::thread::sleep(Duration::from_millis(200));
    let mut again = join(s.1, "/full");
    id(&mut again);
}

#[test]
fn bad_rooms_and_huge_messages_are_refused() {
    let s = signal_server();
    for bad in ["/", "/a.b", "/%20"] {
        let r = tungstenite::connect(format!("ws://127.0.0.1:{}{bad}", s.1));
        assert!(r.is_err(), "{bad}");
    }
    let mut a = join(s.1, "/r");
    let ida = id(&mut a);
    let mut b = join(s.1, "/r");
    let idb = id(&mut b);
    next(&mut a);
    let big = "x".repeat(70 * 1024);
    let _ = a.send(Message::Text(
        json!({ "Signal": { "receiver": idb, "data": big } })
            .to_string()
            .into(),
    ));
    assert!(closed(&mut a), "a message over 64 KiB ends the connection");
    // ... and is never relayed: b hears only that a has gone.
    assert_eq!(next(&mut b), json!({ "PeerLeft": ida }));
    quiet(&mut b);
}

#[test]
fn mp_signal_needs_a_port() {
    let out = Command::new(env!("CARGO_BIN_EXE_mp-signal"))
        .env_remove("PORT")
        .env("HOME", "/nonexistent")
        .env("PATH", "/nonexistent")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("refusing to guess"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `mp-host` relays the same conversation at `…/signal/<room>`, beside its
/// game WebSocket and files.
#[test]
fn mp_host_relays_signalling_too() {
    let port = free_port();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_mp-host"))
        .args(["--port", &port.to_string(), "--bind", "127.0.0.1", "--root"])
        .arg(&root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let h = Proc(child, port);
    wait_up(h.1);
    conversation(h.1, "/signal");
    conversation(h.1, "/midnight-racer/dist/next/signal");
}
