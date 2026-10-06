//! The `mp-host` binary end to end: it serves files as `tools/serve.py`
//! does and runs a lobby and a race over real WebSockets.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use mp_net::proto::{AiFill, Msg, Settings, VERSION};
use mp_sim::input::InputFrame;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

struct HostProc(Child, u16);

impl Drop for HostProc {
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

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn start() -> HostProc {
    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_mp-host"))
        .args(["--port", &port.to_string(), "--bind", "127.0.0.1", "--root"])
        .arg(root())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let t = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(t.elapsed() < Duration::from_secs(20), "mp-host came up");
        std::thread::sleep(Duration::from_millis(50));
    }
    HostProc(child, port)
}

fn get(port: u16, path: &str) -> (u16, String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut b = Vec::new();
    s.read_to_end(&mut b).unwrap();
    let split = b.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&b[..split]).to_string();
    let code = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (code, head, b[split + 4..].to_vec())
}

#[test]
fn it_serves_files_like_serve_py() {
    let h = start();
    let (code, head, body) = get(h.1, "/index.html");
    assert_eq!(code, 200);
    assert!(head.contains("Cache-Control: no-store"), "{head}");
    assert_eq!(body, std::fs::read(root().join("index.html")).unwrap());
    let (code, head, _) = get(h.1, "/vendor/");
    assert!(code == 200 || code == 404, "{code}");
    let _ = head;
    let (code, _, _) = get(h.1, "/tools");
    assert_eq!(code, 301, "a directory gets its slash");
    let (code, _, _) = get(h.1, "/../../etc/passwd");
    assert_eq!(code, 404);
    let (code, _, _) = get(h.1, "/no-such-file.js");
    assert_eq!(code, 404);
}

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

fn join(port: u16, name: &str) -> Ws {
    // Under a sub-path, as the game is served.
    let (mut ws, _) =
        tungstenite::connect(format!("ws://127.0.0.1:{port}/midnight-racer/dist/next/ws")).unwrap();
    if let MaybeTlsStream::Plain(s) = ws.get_mut() {
        s.set_read_timeout(Some(Duration::from_millis(20))).unwrap();
    }
    send(
        &mut ws,
        &Msg::Hello {
            version: VERSION,
            name: name.into(),
            car: "rally".into(),
            color: 0xabcdef,
        },
    );
    ws
}

fn send(ws: &mut Ws, m: &Msg) {
    ws.send(Message::Binary(m.encode().into())).unwrap();
}

/// Reads messages for up to `ms`, returning them.
fn read_for(ws: &mut Ws, ms: u64) -> Vec<Msg> {
    let t = Instant::now();
    let mut out = Vec::new();
    while t.elapsed() < Duration::from_millis(ms) {
        match ws.read() {
            Ok(Message::Binary(b)) => out.push(Msg::decode(&b).unwrap()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => panic!("{e}"),
        }
    }
    out
}

#[test]
fn players_meet_in_the_lobby_and_race() {
    let h = start();
    let mut a = join(h.1, "Marisol");
    let got = read_for(&mut a, 400);
    assert!(got.contains(&Msg::Welcome { slot: 0 }), "{got:?}");
    let mut b = join(h.1, "Kit");
    let got = read_for(&mut b, 400);
    assert!(got.contains(&Msg::Welcome { slot: 1 }));
    let lobby = got.iter().rev().find_map(|m| match m {
        Msg::Lobby { players, .. } => Some(players.clone()),
        _ => None,
    });
    let names: Vec<String> = lobby.unwrap().into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["Marisol", "Kit"]);

    // Marisol leads: a short race setup, then go.
    let s = Settings {
        level: "coast".into(),
        ai: AiFill::None,
        ..Settings::default()
    };
    send(&mut a, &Msg::Configure(s));
    send(&mut a, &Msg::Go(true));
    let got = read_for(&mut b, 600);
    let start = got.iter().find_map(|m| match m {
        Msg::Start(s) => Some(s.clone()),
        _ => None,
    });
    let start = start.expect("the race starts");
    assert_eq!(start.humans.len(), 2);
    assert_eq!(start.settings.level, "coast");
    // Both have "loaded": the host begins the race.
    send(&mut a, &Msg::Loaded);
    send(&mut b, &Msg::Loaded);
    // Inputs flow: send some, and the host relays ticks for both.
    let full = InputFrame {
        throttle: 255,
        ..InputFrame::default()
    };
    let mut ticks = 0;
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(4) {
        send(
            &mut b,
            &Msg::Input {
                first: 1,
                frames: vec![full; 300],
            },
        );
        for m in read_for(&mut b, 50) {
            if let Msg::Inputs { humans, frames, .. } = m {
                assert_eq!(humans, 2);
                ticks += frames.len() / 2;
            }
        }
        let _ = read_for(&mut a, 1);
    }
    assert!(ticks > 120, "the host stepped and relayed {ticks} ticks");
}
