//! The file server half of `mp-host`: plain HTTP/1.1 GET and HEAD, with
//! `tools/serve.py`'s rules, and the upgrade to WebSocket on `…/ws`.
//!
//! - Everything is sent `no-store` (so a phone never mixes old and new
//!   modules), except `vendor/` and `audio/`, which are `no-cache` and
//!   answered 304 when unchanged.
//! - Under `dist/`, a `.br` or `.gz` beside a file is sent instead when the
//!   browser takes it.
//! - `.wasm` is `application/wasm`, for streaming compilation.
//! - A directory serves its `index.html`; a directory without a trailing
//!   slash is redirected to one (so relative URLs in it resolve).
//! - Paths can't climb out of the root.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::Role;

use crate::net::Incoming;
use mp_host::signal::{Rooms, room_of, ws_config};

pub struct Files {
    root: PathBuf,
}

impl Files {
    pub fn new(root: PathBuf) -> Files {
        Files { root }
    }

    /// The file a URL path names, if it is inside the root.
    pub fn resolve(&self, url_path: &str) -> Option<PathBuf> {
        let decoded = percent_decode(url_path)?;
        let mut p = self.root.clone();
        for c in Path::new(decoded.trim_start_matches('/')).components() {
            match c {
                Component::Normal(s) => p.push(s),
                Component::CurDir => {}
                _ => return None,
            }
        }
        Some(p)
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "webmanifest" => "application/json",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" | "md" => "text/plain; charset=utf-8",
        "gz" | "br" | "bin" | "mrscene" => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_request(r: &mut BufReader<TcpStream>) -> Option<Request> {
    let mut line = String::new();
    if r.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).ok()? == 0 {
            return None;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if headers.len() > 100 {
            return None;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let path = target.split(['?', '#']).next().unwrap_or("/").to_string();
    Some(Request {
        method,
        path,
        headers,
    })
}

/// Serves one connection: requests until it closes, or the upgrade.
pub fn serve(stream: TcpStream, files: &Files, incoming: &Incoming, rooms: &Rooms) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut out = stream;
    while let Some(req) = read_request(&mut reader) {
        let upgrade = req
            .header("Upgrade")
            .is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
        // WebRTC signalling: `…/signal/<room>` (DECISIONS D1123).
        let signal = req
            .path
            .rsplit_once("/signal/")
            .and_then(|(_, r)| room_of(r).map(str::to_string));
        if (req.path.ends_with("/ws") || signal.is_some()) && upgrade {
            let Some(key) = req.header("Sec-WebSocket-Key") else {
                let _ = respond(&mut out, 400, "Bad Request", &[], b"missing key", false);
                return;
            };
            let accept = derive_accept_key(key.as_bytes());
            let head = format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
            );
            if out.write_all(head.as_bytes()).is_err() {
                return;
            }
            // Bytes the reader buffered past the request belong to the socket.
            let buffered = reader.buffer().to_vec();
            let _ = out.set_read_timeout(None);
            match signal {
                Some(room) => {
                    let ws = tungstenite::WebSocket::from_partially_read(
                        out,
                        buffered,
                        Role::Server,
                        Some(ws_config()),
                    );
                    rooms.serve(&room, ws);
                }
                None => {
                    let ws = tungstenite::WebSocket::from_partially_read(
                        out,
                        buffered,
                        Role::Server,
                        None,
                    );
                    incoming.accept(ws);
                }
            }
            return;
        }
        let keep = !req
            .header("Connection")
            .is_some_and(|c| c.eq_ignore_ascii_case("close"));
        if !answer(&mut out, files, &req) || !keep {
            return;
        }
    }
}

fn respond(
    out: &mut TcpStream,
    code: u16,
    reason: &str,
    headers: &[(&str, String)],
    body: &[u8],
    head_only: bool,
) -> bool {
    let mut h = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    h.push_str("\r\n");
    out.write_all(h.as_bytes()).is_ok() && (head_only || out.write_all(body).is_ok())
}

fn http_date(t: SystemTime) -> String {
    // RFC 7231 IMF-fixdate, from the Unix time (civil-from-days).
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if mo <= 2 { 1 } else { 0 };
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    let mon = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(mo - 1) as usize];
    format!("{wd}, {d:02} {mon} {y} {h:02}:{m:02}:{s:02} GMT")
}

/// Answers a GET or HEAD; false if the connection should close.
fn answer(out: &mut TcpStream, files: &Files, req: &Request) -> bool {
    let head = req.method == "HEAD";
    if req.method != "GET" && !head {
        return respond(
            out,
            405,
            "Method Not Allowed",
            &[("Allow", "GET, HEAD".into())],
            b"",
            false,
        );
    }
    let pinned = req.path.starts_with("/vendor/") || req.path.starts_with("/audio/");
    let cache = (
        "Cache-Control",
        if pinned { "no-cache" } else { "no-store" }.to_string(),
    );
    let Some(mut file) = files.resolve(&req.path) else {
        return respond(out, 404, "Not Found", &[cache], b"not found", head);
    };
    if file.is_dir() {
        if !req.path.ends_with('/') {
            let loc = format!("{}/", req.path);
            return respond(
                out,
                301,
                "Moved Permanently",
                &[("Location", loc), cache],
                b"",
                head,
            );
        }
        file.push("index.html");
    }
    let Ok(meta) = std::fs::metadata(&file) else {
        return respond(out, 404, "Not Found", &[cache], b"not found", head);
    };
    let ctype = content_type(&file).to_string();
    let modified = meta.modified().ok().map(http_date);
    if pinned
        && let (Some(m), Some(since)) = (&modified, req.header("If-Modified-Since"))
        && m == since
    {
        return respond(out, 304, "Not Modified", &[cache], b"", true);
    }
    // The release build's precompressed copies under dist/.
    if req.path.starts_with("/dist/") {
        let accepted: Vec<String> = req
            .header("Accept-Encoding")
            .unwrap_or("")
            .split(',')
            .map(|t| {
                t.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            })
            .collect();
        for (enc, ext) in [("br", "br"), ("gzip", "gz")] {
            let alt = PathBuf::from(format!("{}.{ext}", file.display()));
            if accepted.iter().any(|a| a == enc)
                && let Ok(body) = std::fs::read(&alt)
            {
                return respond(
                    out,
                    200,
                    "OK",
                    &[
                        ("Content-Type", ctype),
                        ("Content-Encoding", enc.into()),
                        ("Vary", "Accept-Encoding".into()),
                        cache,
                    ],
                    &body,
                    head,
                );
            }
        }
    }
    let Ok(mut f) = std::fs::File::open(&file) else {
        return respond(out, 404, "Not Found", &[cache], b"not found", head);
    };
    let mut body = Vec::with_capacity(meta.len() as usize);
    if f.read_to_end(&mut body).is_err() {
        return respond(out, 500, "Internal Server Error", &[cache], b"", head);
    }
    let mut hs = vec![("Content-Type", ctype), cache];
    if let Some(m) = modified {
        hs.push(("Last-Modified", m));
    }
    respond(out, 200, "OK", &hs, &body, head)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_cannot_leave_the_root() {
        let f = Files::new(PathBuf::from("/srv/game"));
        assert_eq!(
            f.resolve("/src/main.js"),
            Some(PathBuf::from("/srv/game/src/main.js"))
        );
        assert_eq!(
            f.resolve("/a%20b.txt"),
            Some(PathBuf::from("/srv/game/a b.txt"))
        );
        assert_eq!(f.resolve("/../etc/passwd"), None);
        assert_eq!(f.resolve("/src/%2e%2e/%2e%2e/etc"), None);
        assert_eq!(f.resolve("/%zz"), None);
    }

    #[test]
    fn dates_are_http_dates() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_791_331_200); // 2026-10-07
        assert_eq!(http_date(t), "Wed, 07 Oct 2026 00:00:00 GMT");
        assert_eq!(
            http_date(SystemTime::UNIX_EPOCH),
            "Thu, 01 Jan 1970 00:00:00 GMT"
        );
    }

    #[test]
    fn every_way_of_spelling_a_climb_out_is_refused() {
        let f = Files::new(PathBuf::from("/srv/game"));
        let inside = |p: &str| PathBuf::from("/srv/game").join(p);
        for (url, want) in [
            ("/", Some(inside(""))),
            ("", Some(inside(""))),
            ("//etc/passwd", Some(inside("etc/passwd"))),
            ("/a/./b", Some(inside("a/b"))),
            ("/a%2Fb", Some(inside("a/b"))),
            ("/%C3%A9t%C3%A9.txt", Some(inside("été.txt"))),
            ("/a\\..\\b", Some(inside("a\\..\\b"))),
            ("/a/../b", None),
            ("/..", None),
            ("/%2E%2E/etc", None),
            ("/%2e%2e%2fetc", None),
            ("/sub/..%2f..%2fetc", None),
            ("/%2f%2e%2e", None),
            ("/.%2e/x", None),
            ("/%", None),
            ("/%2", None),
            ("/%g0", None),
            ("/%ff", None),
            ("/%C3", None),
        ] {
            assert_eq!(f.resolve(url), want, "{url}");
            if let Some(p) = f.resolve(url) {
                assert!(p.starts_with("/srv/game"), "{url}");
            }
        }
    }

    #[test]
    fn dates_are_http_dates_across_leap_years_and_centuries() {
        let at = |s: u64| http_date(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(1_709_164_800), "Thu, 29 Feb 2024 00:00:00 GMT");
        assert_eq!(at(951_868_800), "Wed, 01 Mar 2000 00:00:00 GMT");
        assert_eq!(at(4_107_542_400), "Mon, 01 Mar 2100 00:00:00 GMT");
        assert_eq!(at(946_684_799), "Fri, 31 Dec 1999 23:59:59 GMT");
        assert_eq!(at(2_147_483_648), "Tue, 19 Jan 2038 03:14:08 GMT");
    }

    #[test]
    fn content_types_follow_serve_py() {
        for (file, want) in [
            ("index.html", "text/html; charset=utf-8"),
            ("a/b.js", "text/javascript; charset=utf-8"),
            ("m.mjs", "text/javascript; charset=utf-8"),
            ("s.css", "text/css; charset=utf-8"),
            ("d.json", "application/json"),
            ("site.webmanifest", "application/json"),
            ("i.png", "image/png"),
            ("i.jpg", "image/jpeg"),
            ("i.jpeg", "image/jpeg"),
            ("i.svg", "image/svg+xml"),
            ("a.mp3", "audio/mpeg"),
            ("a.flac", "audio/flac"),
            ("a.ogg", "audio/ogg"),
            ("a.wav", "audio/wav"),
            ("f.woff2", "font/woff2"),
            ("f.ttf", "font/ttf"),
            ("README.md", "text/plain; charset=utf-8"),
            ("x.txt", "text/plain; charset=utf-8"),
            ("app.wasm.br", "application/octet-stream"),
            ("survey.bin", "application/octet-stream"),
            ("Makefile", "application/octet-stream"),
            (".hidden", "application/octet-stream"),
        ] {
            assert_eq!(content_type(Path::new(file)), want, "{file}");
        }
    }

    #[test]
    fn wasm_is_application_wasm() {
        assert_eq!(content_type(Path::new("a/b.wasm")), "application/wasm");
        assert_eq!(
            content_type(Path::new("x.html")),
            "text/html; charset=utf-8"
        );
    }
}
