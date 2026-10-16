//! What the end-to-end tests share: starting a server binary on a free
//! port and knowing when it listens.
//!
//! Polling the port with a connect can't tell. `free_port`, in a test
//! running beside, listens for a moment on whatever port the kernel gives
//! it, which can be the one a server was just told and has not bound yet,
//! so the poll met that and the test went on to a refused connection.
//! The servers print a line once their listener is bound, and nothing
//! (the pipe closes) when they can't listen.

#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A port the kernel calls free just now (it may not stay so).
pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Whether `child` (its stdout piped) says it listens; the rest of its
/// output is read away, so a later line doesn't meet a closed pipe.
pub fn listening(child: &mut Child) -> bool {
    let mut out = BufReader::new(child.stdout.take().expect("stdout piped")).lines();
    if out.next().is_some_and(|l| l.is_ok()) {
        std::thread::spawn(move || out.for_each(drop));
        true
    } else {
        false
    }
}

/// The server `cmd(port)` makes, on a free port, once it listens; another
/// port if one was taken before it could bind.
pub fn serve(mut cmd: impl FnMut(u16) -> Command) -> (Child, u16) {
    let t = Instant::now();
    loop {
        let port = free_port();
        let mut child = cmd(port)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        if listening(&mut child) {
            return (child, port);
        }
        let _ = child.wait();
        assert!(t.elapsed() < Duration::from_secs(20), "the server came up");
    }
}

/// A server and everything it started, stopped together (`wrangler dev`
/// runs workerd as a child of its own), and the state it kept, if any.
pub struct Group(pub Child, pub Option<std::path::PathBuf>);

impl Drop for Group {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", "--", &format!("-{}", self.0.id())])
            .status();
        let _ = self.0.kill();
        let _ = self.0.wait();
        if let Some(dir) = &self.1 {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Whether the signalling tests run against the Cloudflare Worker
/// (`tools/signal-worker`, DECISIONS D1222) instead of `mp-signal`:
/// `MP_SIGNAL_WORKER=1`, with its `npm install` done.
pub fn on_worker() -> bool {
    std::env::var("MP_SIGNAL_WORKER").is_ok_and(|v| v == "1")
}

/// The signalling relay the tests talk to, on a free port: `mp-signal`,
/// or the Worker under `wrangler dev` (each with its own state) when
/// [`on_worker`].
pub fn relay() -> (Group, u16) {
    use std::os::unix::process::CommandExt;
    if !on_worker() {
        let (child, port) = serve(|port| {
            let mut c = Command::new(env!("CARGO_BIN_EXE_mp-signal"));
            c.args(["--port", &port.to_string()]).process_group(0);
            c
        });
        return (Group(child, None), port);
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/signal-worker");
    let t = Instant::now();
    loop {
        let port = free_port();
        let state =
            std::env::temp_dir().join(format!("mp-signal-worker-{}-{port}", std::process::id()));
        let mut child = Command::new(dir.join("node_modules/.bin/wrangler"))
            .args(["dev", "--ip", "127.0.0.1", "--port", &port.to_string()])
            .args(["--inspector-port", "0", "--persist-to"])
            .arg(&state)
            .current_dir(&dir)
            .env("WRANGLER_SEND_METRICS", "false")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("wrangler is installed: npm install in tools/signal-worker");
        let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
        let ready = out
            .by_ref()
            .map_while(Result::ok)
            .any(|l| l.contains("Ready on"));
        let group = Group(child, Some(state));
        if ready {
            std::thread::spawn(move || out.for_each(drop));
            return (group, port);
        }
        drop(group);
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "wrangler dev came up"
        );
    }
}
