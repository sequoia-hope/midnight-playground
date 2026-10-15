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
