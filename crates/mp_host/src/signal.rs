//! The signalling relay (MULTIPLAYER 8, DECISIONS D1123): introduces
//! WebRTC peers in a room and passes their messages along. It speaks
//! matchbox 0.14's protocol (JSON text frames), so the game's clients
//! could as well use a stock `matchbox_server`, and it is full mesh: when
//! a peer arrives, every peer already in the room is told (`NewPeer`).
//!
//! The game seals every payload with the room's secret (`mp_net::signal`),
//! so all this relay sees is a room id, peer ids it made up, and
//! ciphertext. It writes nothing down: no rooms, peers or addresses are
//! logged.
//!
//! Each connection runs on its own thread (as `mp-host`'s WebSockets do);
//! the rooms are shared behind a mutex.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use matchbox_protocol::{PeerEvent, PeerId, PeerRequest};
use serde_json::Value;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

/// The most peers in one room (a full lobby is eight).
pub const MAX_PEERS: usize = 16;
/// The most rooms open at once.
pub const MAX_ROOMS: usize = 1024;
/// The largest message, bytes (an offer with its candidates is a few KB).
pub const MAX_MESSAGE: usize = 64 * 1024;
/// A peer that says nothing for this long is dropped (matchbox sends a
/// keep-alive every 10 s).
pub const IDLE: Duration = Duration::from_secs(60);
/// How long a connection waits for its peer before looking at its outbox.
const TICK: Duration = Duration::from_millis(20);

/// The WebSocket settings for a signalling connection.
pub fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE))
}

/// A room id as matchbox clients send it: the path's last segment, 1 to 64
/// of `A-Z a-z 0-9 - _`.
pub fn room_of(path: &str) -> Option<&str> {
    let path = path.split(['?', '#']).next()?;
    let room = path.trim_end_matches('/').rsplit('/').next()?;
    let ok = !room.is_empty()
        && room.len() <= 64
        && room
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then_some(room)
}

type Outbox = Sender<String>;

#[derive(Default)]
struct Inner {
    /// Room → its peers, in the order they came.
    rooms: BTreeMap<String, Vec<(PeerId, Outbox)>>,
}

/// Every room. Cloning shares them.
#[derive(Clone, Default)]
pub struct Rooms(Arc<Mutex<Inner>>);

fn event(e: &PeerEvent<Value>) -> String {
    serde_json::to_string(e).expect("an event serialises")
}

fn new_id() -> PeerId {
    PeerId(uuid::Uuid::new_v4())
}

impl Rooms {
    pub fn new() -> Rooms {
        Rooms::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Rooms open now (for the tests).
    pub fn rooms(&self) -> usize {
        self.lock().rooms.len()
    }

    /// Joins `room`: tells the others, then relays until the connection
    /// ends, then tells them it has gone. Refuses (closing the socket) when
    /// the room or the server is full.
    pub fn serve(&self, room: &str, mut ws: WebSocket<TcpStream>) {
        let id = new_id();
        let (tx, rx) = channel();
        {
            let mut g = self.lock();
            let full = match g.rooms.get(room) {
                Some(peers) => peers.len() >= MAX_PEERS,
                None => g.rooms.len() >= MAX_ROOMS,
            };
            if full {
                drop(g);
                let _ = ws.close(None);
                let _ = ws.flush();
                return;
            }
            let peers = g.rooms.entry(room.to_string()).or_default();
            let _ = tx.send(event(&PeerEvent::IdAssigned(id)));
            for (_, other) in peers.iter() {
                let _ = other.send(event(&PeerEvent::NewPeer(id)));
            }
            peers.push((id, tx));
        }
        self.relay(room, id, &mut ws, &rx);
        let mut g = self.lock();
        if let Some(peers) = g.rooms.get_mut(room) {
            peers.retain(|(p, _)| *p != id);
            for (_, other) in peers.iter() {
                let _ = other.send(event(&PeerEvent::PeerLeft(id)));
            }
            if peers.is_empty() {
                g.rooms.remove(room);
            }
        }
    }

    fn relay(&self, room: &str, id: PeerId, ws: &mut WebSocket<TcpStream>, rx: &Receiver<String>) {
        let _ = ws.get_mut().set_read_timeout(Some(TICK));
        let _ = ws.get_mut().set_nodelay(true);
        let mut heard = Instant::now();
        loop {
            loop {
                match rx.try_recv() {
                    Ok(t) => {
                        if ws.write(Message::Text(t.into())).is_err() {
                            return;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if ws.flush().is_err() {
                return;
            }
            match ws.read() {
                Ok(Message::Text(t)) => {
                    heard = Instant::now();
                    match serde_json::from_str::<PeerRequest<Value>>(&t) {
                        Ok(PeerRequest::Signal { receiver, data }) => {
                            let g = self.lock();
                            let to = g
                                .rooms
                                .get(room)
                                .and_then(|ps| ps.iter().find(|(p, _)| *p == receiver));
                            if let Some((_, out)) = to {
                                let _ = out.send(event(&PeerEvent::Signal { sender: id, data }));
                            }
                        }
                        Ok(PeerRequest::KeepAlive) => {}
                        // Anything else is a client that isn't ours.
                        Err(_) => return,
                    }
                }
                Ok(Message::Close(_)) => return,
                Ok(_) => heard = Instant::now(),
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    if heard.elapsed() > IDLE {
                        let _ = ws.close(None);
                        let _ = ws.flush();
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }
}

/// Accepts one connection as `mp-signal` does: the WebSocket handshake
/// (the room from the request's path), then [`Rooms::serve`]. Anything
/// that isn't a WebSocket for a valid room is turned away.
// The handshake callback's error type is tungstenite's.
#[allow(clippy::result_large_err)]
pub fn accept(stream: TcpStream, rooms: &Rooms) {
    use tungstenite::handshake::server::{ErrorResponse, Request, Response};
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut room = None;
    let cb = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        match room_of(req.uri().path()) {
            Some(r) => {
                room = Some(r.to_string());
                Ok(resp)
            }
            None => {
                let mut e = ErrorResponse::new(Some("no such room".into()));
                *e.status_mut() = tungstenite::http::StatusCode::NOT_FOUND;
                Err(e)
            }
        }
    };
    let Ok(ws) = tungstenite::accept_hdr_with_config(stream, cb, Some(ws_config())) else {
        return;
    };
    if let Some(room) = room {
        rooms.serve(&room, ws);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_room_is_the_paths_last_segment() {
        assert_eq!(room_of("/abcDEF-_09"), Some("abcDEF-_09"));
        assert_eq!(room_of("/signal/room1"), Some("room1"));
        assert_eq!(room_of("/midnight-racer/dist/next/signal/r/"), Some("r"));
        assert_eq!(room_of("/r?next=2"), Some("r"));
        assert_eq!(
            room_of(&format!("/{}", "a".repeat(64))).map(str::len),
            Some(64)
        );
        for bad in [
            "/",
            "",
            "/a b",
            "/a.b",
            "/ä",
            &format!("/{}", "a".repeat(65)),
        ] {
            assert_eq!(room_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn events_are_matchboxs_json() {
        let id = PeerId(uuid::Uuid::nil());
        assert_eq!(
            event(&PeerEvent::IdAssigned(id)),
            r#"{"IdAssigned":"00000000-0000-0000-0000-000000000000"}"#
        );
        assert_eq!(
            event(&PeerEvent::Signal {
                sender: id,
                data: Value::String("x".into())
            }),
            r#"{"Signal":{"sender":"00000000-0000-0000-0000-000000000000","data":"x"}}"#
        );
        let r: PeerRequest<Value> = serde_json::from_str(r#""KeepAlive""#).unwrap();
        assert_eq!(r, PeerRequest::KeepAlive);
    }
}
