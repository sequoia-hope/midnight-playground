//! The host's transport over WebSocket: each connection runs on its own
//! thread (`http::serve` hands upgraded sockets to [`Incoming::accept`]);
//! the session's thread sees them through [`WsNet`], a [`Transport`].
//! WebSocket is reliable and ordered, so both channels map to it.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mp_net::transport::{Channel, NetEvent, PeerId, Transport};
use tungstenite::{Message, WebSocket};

/// Commands from the session to a connection's thread.
enum Out {
    Send(Vec<u8>),
    Close,
}

#[derive(Clone)]
pub struct Incoming {
    events: Sender<NetEvent>,
    peers: Arc<Mutex<HashMap<PeerId, Sender<Out>>>>,
    next: Arc<AtomicU32>,
}

pub struct WsNet {
    events: Receiver<NetEvent>,
    peers: Arc<Mutex<HashMap<PeerId, Sender<Out>>>>,
}

impl WsNet {
    pub fn new() -> (WsNet, Incoming) {
        let (tx, rx) = channel();
        let peers = Arc::new(Mutex::new(HashMap::new()));
        (
            WsNet {
                events: rx,
                peers: peers.clone(),
            },
            Incoming {
                events: tx,
                peers,
                next: Arc::new(AtomicU32::new(1)),
            },
        )
    }
}

impl Transport for WsNet {
    fn send(&mut self, peer: PeerId, _channel: Channel, bytes: &[u8]) {
        if let Some(tx) = self.peers.lock().unwrap().get(&peer) {
            let _ = tx.send(Out::Send(bytes.to_vec()));
        }
    }

    fn poll(&mut self, out: &mut Vec<NetEvent>) {
        while let Ok(e) = self.events.try_recv() {
            out.push(e);
        }
    }

    fn close(&mut self, peer: PeerId) {
        if let Some(tx) = self.peers.lock().unwrap().remove(&peer) {
            let _ = tx.send(Out::Close);
        }
    }
}

impl Incoming {
    /// Runs a connection that has finished its WebSocket handshake, until
    /// it closes.
    pub fn accept(&self, mut ws: WebSocket<TcpStream>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.peers.lock().unwrap().insert(id, tx);
        let _ = self.events.send(NetEvent::Connected(id));
        // Short reads, so outgoing messages aren't held up by a quiet peer.
        let _ = ws
            .get_mut()
            .set_read_timeout(Some(Duration::from_millis(2)));
        let _ = ws.get_mut().set_nodelay(true);
        'conn: loop {
            // Outgoing first.
            loop {
                match rx.try_recv() {
                    Ok(Out::Send(b)) => {
                        if ws.write(Message::Binary(b.into())).is_err() {
                            break 'conn;
                        }
                    }
                    Ok(Out::Close) | Err(TryRecvError::Disconnected) => {
                        let _ = ws.close(None);
                        let _ = ws.flush();
                        break 'conn;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }
            if ws.flush().is_err() {
                break;
            }
            match ws.read() {
                Ok(Message::Binary(b)) => {
                    let _ = self.events.send(NetEvent::Message(id, b.to_vec()));
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
                Err(_) => break,
            }
        }
        self.peers.lock().unwrap().remove(&id);
        let _ = self.events.send(NetEvent::Disconnected(id));
    }
}
