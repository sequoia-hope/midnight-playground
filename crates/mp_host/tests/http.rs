//! `mp-host`'s file server and its port rule, end to end against the
//! binary on a scratch root: caching headers, the precompressed copies
//! under `dist/`, directories, HEAD and other methods, keep-alive, paths
//! that try to leave the root, the WebSocket upgrade, and where the port
//! comes from (`--port`, `$PORT`, `proj port`, else a loud failure). Ports
//! are picked by the OS, never written down.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use mp_net::proto::{Msg, VERSION};
use tungstenite::{Message, stream::MaybeTlsStream};

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

/// A fresh directory for one test.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mp_host-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn put(root: &Path, rel: &str, body: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

/// A root with a little of everything, and a secret beside it.
fn site() -> PathBuf {
    let d = scratch("site");
    let root = d.join("root");
    put(&d, "secret.txt", b"SECRET");
    put(&root, "index.html", b"<h1>root</h1>");
    put(&root, "sub/index.html", b"sub");
    std::fs::create_dir_all(root.join("empty")).unwrap();
    put(&root, "vendor/lib.js", b"lib");
    put(&root, "audio/a.mp3", b"ID3");
    put(&root, "dist/next/app.wasm", b"WASM");
    put(&root, "dist/next/app.wasm.br", b"BR");
    put(&root, "dist/next/app.wasm.gz", b"GZ");
    put(&root, "dist/next/only.js", b"JS");
    put(&root, "dist/next/only.js.gz", b"JSGZ");
    put(&root, "plain.js", b"PLAIN");
    put(&root, "plain.js.gz", b"NOT-HERE");
    put(&root, "a b.txt", b"spaced");
    root
}

fn wait_up(child: &mut Child, port: u16) {
    let t = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        if let Ok(Some(st)) = child.try_wait() {
            panic!("mp-host exited: {st}");
        }
        assert!(t.elapsed() < Duration::from_secs(20), "mp-host came up");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn host_cmd() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_mp-host"));
    c.env_remove("PORT")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    c
}

fn serve(root: &Path) -> HostProc {
    let port = free_port();
    let mut child = host_cmd()
        .args(["--port", &port.to_string(), "--bind", "127.0.0.1", "--root"])
        .arg(root)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_up(&mut child, port);
    HostProc(child, port)
}

struct Resp {
    code: u16,
    head: String,
    body: Vec<u8>,
}

impl Resp {
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }
}

/// Reads one response, its body by Content-Length (none for HEAD/304).
fn read_resp(r: &mut BufReader<TcpStream>, head_only: bool) -> Resp {
    let mut head = String::new();
    loop {
        let mut l = String::new();
        assert!(r.read_line(&mut l).unwrap() > 0, "a response");
        if l == "\r\n" {
            break;
        }
        head.push_str(&l);
    }
    let code = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut resp = Resp {
        code,
        head,
        body: Vec::new(),
    };
    let n: usize = resp.header("Content-Length").unwrap().parse().unwrap();
    if !head_only && code != 304 {
        resp.body = vec![0; n];
        r.read_exact(&mut resp.body).unwrap();
    }
    resp
}

fn request(port: u16, method: &str, path: &str, headers: &[(&str, &str)]) -> Resp {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    read_resp(&mut BufReader::new(s), method == "HEAD")
}

fn get(port: u16, path: &str) -> Resp {
    request(port, "GET", path, &[])
}

#[test]
fn files_directories_and_missing_paths() {
    let root = site();
    let h = serve(&root);
    let r = get(h.1, "/");
    assert_eq!((r.code, &r.body[..]), (200, &b"<h1>root</h1>"[..]));
    assert_eq!(r.header("Content-Type"), Some("text/html; charset=utf-8"));
    assert_eq!(r.header("Cache-Control"), Some("no-store"));
    assert!(r.header("Last-Modified").unwrap().ends_with(" GMT"));

    let r = get(h.1, "/sub");
    assert_eq!(r.code, 301);
    assert_eq!(r.header("Location"), Some("/sub/"));
    assert_eq!(get(h.1, "/sub/").body, b"sub");
    assert_eq!(get(h.1, "/empty/").code, 404, "a directory with no index");
    assert_eq!(get(h.1, "/nope.js").code, 404);
    assert_eq!(get(h.1, "/a%20b.txt").body, b"spaced");
    let r = get(h.1, "/index.html?v=3#top");
    assert_eq!((r.code, &r.body[..]), (200, &b"<h1>root</h1>"[..]));
}

#[test]
fn paths_outside_the_root_are_never_served() {
    let root = site();
    let h = serve(&root);
    for path in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/sub/../../secret.txt",
        "/..%2fsecret.txt",
        "/sub/%2E%2E/%2E%2E/secret.txt",
    ] {
        let r = get(h.1, path);
        assert_eq!(r.code, 404, "{path}");
        assert_ne!(r.body, b"SECRET", "{path}");
    }
}

#[test]
fn vendor_and_audio_are_revalidated_everything_else_is_not_stored() {
    let root = site();
    let h = serve(&root);
    let r = get(h.1, "/vendor/lib.js");
    assert_eq!(r.header("Cache-Control"), Some("no-cache"));
    assert_eq!(
        r.header("Content-Type"),
        Some("text/javascript; charset=utf-8")
    );
    let lm = r.header("Last-Modified").unwrap().to_string();
    let r = request(h.1, "GET", "/vendor/lib.js", &[("If-Modified-Since", &lm)]);
    assert_eq!(r.code, 304);
    assert!(r.body.is_empty());
    let r = request(
        h.1,
        "GET",
        "/vendor/lib.js",
        &[("If-Modified-Since", "Thu, 01 Jan 1970 00:00:00 GMT")],
    );
    assert_eq!((r.code, &r.body[..]), (200, &b"lib"[..]));
    let r = get(h.1, "/audio/a.mp3");
    assert_eq!(r.header("Cache-Control"), Some("no-cache"));
    assert_eq!(r.header("Content-Type"), Some("audio/mpeg"));
    // Elsewhere a conditional request still gets the whole file.
    let r = request(h.1, "GET", "/plain.js", &[("If-Modified-Since", &lm)]);
    assert_eq!(r.code, 200);
    assert_eq!(r.header("Cache-Control"), Some("no-store"));
    // A missing file under vendor/ is a 404 with the same caching.
    let r = get(h.1, "/vendor/none.js");
    assert_eq!(r.code, 404);
    assert_eq!(r.header("Cache-Control"), Some("no-cache"));
}

#[test]
fn dist_sends_the_precompressed_copy_the_browser_takes() {
    let root = site();
    let h = serve(&root);
    let wasm = "/dist/next/app.wasm";
    for (accept, body, enc) in [
        ("gzip, deflate, br", "BR", Some("br")),
        ("br;q=1.0, gzip;q=0.8", "BR", Some("br")),
        ("GZIP", "GZ", Some("gzip")),
        ("identity", "WASM", None),
        ("", "WASM", None),
    ] {
        let r = request(h.1, "GET", wasm, &[("Accept-Encoding", accept)]);
        assert_eq!(r.body, body.as_bytes(), "{accept:?}");
        assert_eq!(r.header("Content-Encoding"), enc, "{accept:?}");
        assert_eq!(r.header("Content-Type"), Some("application/wasm"));
        if enc.is_some() {
            assert_eq!(r.header("Vary"), Some("Accept-Encoding"));
        }
    }
    // Only a .gz beside it: br is skipped.
    let r = request(
        h.1,
        "GET",
        "/dist/next/only.js",
        &[("Accept-Encoding", "br, gzip")],
    );
    assert_eq!(
        (&r.body[..], r.header("Content-Encoding")),
        (&b"JSGZ"[..], Some("gzip"))
    );
    // Outside dist/ the copies are just files.
    let r = request(h.1, "GET", "/plain.js", &[("Accept-Encoding", "gzip")]);
    assert_eq!(
        (&r.body[..], r.header("Content-Encoding")),
        (&b"PLAIN"[..], None)
    );
}

#[test]
fn head_has_the_headers_without_the_body_and_other_methods_are_refused() {
    let root = site();
    let h = serve(&root);
    // HEAD: read the head, then the connection must close with no body.
    let mut s = TcpStream::connect(("127.0.0.1", h.1)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"HEAD /index.html HTTP/1.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut all = Vec::new();
    s.read_to_end(&mut all).unwrap();
    let text = String::from_utf8(all).unwrap();
    assert!(text.starts_with("HTTP/1.1 200"), "{text}");
    assert!(text.contains("Content-Length: 13\r\n"), "{text}");
    assert!(text.ends_with("\r\n\r\n"), "no body: {text:?}");

    let r = request(h.1, "POST", "/index.html", &[]);
    assert_eq!(r.code, 405);
    assert_eq!(r.header("Allow"), Some("GET, HEAD"));
}

#[test]
fn a_connection_carries_several_requests() {
    let root = site();
    let h = serve(&root);
    let s = TcpStream::connect(("127.0.0.1", h.1)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut w = s.try_clone().unwrap();
    let mut r = BufReader::new(s);
    for (path, body) in [
        ("/sub/", &b"sub"[..]),
        ("/a%20b.txt", b"spaced"),
        ("/", b"<h1>root</h1>"),
    ] {
        write!(w, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let resp = read_resp(&mut r, false);
        assert_eq!((resp.code, &resp.body[..]), (200, body), "{path}");
    }
    // Pipelined, both at once.
    w.write_all(b"GET /sub/ HTTP/1.1\r\n\r\nGET /plain.js HTTP/1.1\r\n\r\n")
        .unwrap();
    assert_eq!(read_resp(&mut r, false).body, b"sub");
    assert_eq!(read_resp(&mut r, false).body, b"PLAIN");
}

#[test]
fn nonsense_requests_close_the_connection_and_the_server_carries_on() {
    let root = site();
    let h = serve(&root);
    for junk in [
        &b"\r\n\r\n"[..],
        b"GARBAGE\r\n\r\n",
        b"\xff\xfe\xfd\r\n\r\n",
        b"GET\r\n\r\n",
    ] {
        let mut s = TcpStream::connect(("127.0.0.1", h.1)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(junk).unwrap();
        let _ = s.shutdown(std::net::Shutdown::Write);
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
    }
    // Too many headers.
    let mut s = TcpStream::connect(("127.0.0.1", h.1)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut req = String::from("GET / HTTP/1.1\r\n");
    for k in 0..200 {
        req.push_str(&format!("X-{k}: y\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    assert!(out.is_empty(), "no answer to 200 headers");
    assert_eq!(get(h.1, "/").code, 200);
}

#[test]
fn the_websocket_is_on_any_path_ending_in_ws_and_nowhere_else() {
    let root = site();
    let h = serve(&root);
    // Without a key: refused.
    let r = request(
        h.1,
        "GET",
        "/game/ws",
        &[("Upgrade", "websocket"), ("Connection", "Upgrade")],
    );
    assert_eq!(r.code, 400);
    // An upgrade elsewhere is just a GET.
    let r = request(
        h.1,
        "GET",
        "/index.html",
        &[
            ("Upgrade", "websocket"),
            ("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ=="),
        ],
    );
    assert_eq!(r.code, 200);
    // At the root's /ws, a player is welcomed.
    let (mut ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{}/ws", h.1)).unwrap();
    if let MaybeTlsStream::Plain(s) = ws.get_mut() {
        s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
    }
    let hello = Msg::Hello {
        version: VERSION,
        name: "Ann".into(),
        car: "sports".into(),
        color: 1,
    };
    ws.send(Message::Binary(hello.encode().into())).unwrap();
    let t = Instant::now();
    let mut welcomed = false;
    while !welcomed && t.elapsed() < Duration::from_secs(5) {
        if let Ok(Message::Binary(b)) = ws.read() {
            welcomed = Msg::decode(&b) == Ok(Msg::Welcome { slot: 0 });
        }
    }
    assert!(welcomed);
}

// ── The port ─────────────────────────────────────────────────────

/// Runs the binary to its exit (it should fail fast); its code and stderr.
fn fails(mut c: Command) -> (i32, String) {
    let mut child = c.spawn().unwrap();
    let t = Instant::now();
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            let mut err = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut err)
                .unwrap();
            return (st.code().unwrap_or(-1), err);
        }
        if t.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            panic!("mp-host should have refused to start");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A command with no `proj` to be found: an empty HOME and PATH.
fn no_proj(tag: &str) -> (Command, PathBuf) {
    let home = scratch(tag);
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let mut c = host_cmd();
    c.env("HOME", &home)
        .env("PATH", &bin)
        .args(["--bind", "127.0.0.1", "--root"])
        .arg(site());
    (c, home)
}

#[cfg(unix)]
fn fake_proj(home: &Path, says: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = home.join("scripts/proj");
    put(
        home,
        "scripts/proj",
        format!("#!/bin/sh\n[ \"$1\" = port ] && echo '{says}' && exit 0\nexit 1\n").as_bytes(),
    );
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn with_no_port_from_anywhere_it_refuses_to_guess() {
    let (c, _) = no_proj("none");
    let (code, err) = fails(c);
    assert_eq!(code, 2);
    assert!(err.contains("refusing to guess"), "{err}");
}

#[test]
fn a_bad_port_argument_or_variable_is_an_error() {
    let (mut c, _) = no_proj("badenv");
    c.env("PORT", "eighty");
    let (code, err) = fails(c);
    assert_eq!(code, 2);
    assert!(err.contains("$PORT is not a port"), "{err}");

    let (mut c, _) = no_proj("badarg");
    c.args(["--port", "x"]);
    let (code, err) = fails(c);
    assert_eq!(code, 2);
    assert!(err.contains("usage"), "{err}");

    let (mut c, _) = no_proj("toobig");
    c.args(["--port", &u32::MAX.to_string()]);
    assert_eq!(fails(c).0, 2);

    let (mut c, _) = no_proj("unknown");
    c.arg("--verbose");
    assert_eq!(fails(c).0, 2);
}

#[test]
fn the_port_comes_from_port_when_there_is_no_flag() {
    let (mut c, _) = no_proj("env");
    let port = free_port();
    c.env("PORT", port.to_string()).stderr(Stdio::null());
    let mut h = HostProc(c.spawn().unwrap(), port);
    wait_up(&mut h.0, port);
    assert_eq!(get(port, "/sub/").body, b"sub");
}

#[test]
fn the_flag_wins_over_port() {
    let (mut c, _) = no_proj("flag");
    let port = free_port();
    c.env("PORT", "not even a number")
        .args(["--port", &port.to_string()])
        .stderr(Stdio::null());
    let mut h = HostProc(c.spawn().unwrap(), port);
    wait_up(&mut h.0, port);
    assert_eq!(get(port, "/").code, 200);
}

#[cfg(unix)]
#[test]
fn without_flag_or_port_it_asks_proj() {
    let (mut c, home) = no_proj("proj");
    let port = free_port();
    fake_proj(&home, &port.to_string());
    c.stderr(Stdio::null());
    let mut h = HostProc(c.spawn().unwrap(), port);
    wait_up(&mut h.0, port);
    assert_eq!(get(port, "/").code, 200);

    let (c, home) = no_proj("projjunk");
    fake_proj(&home, "no idea");
    let (code, err) = fails(c);
    assert_eq!(code, 2);
    assert!(err.contains("`proj port` gave no port"), "{err}");
}
