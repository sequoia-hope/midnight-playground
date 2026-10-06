//! The client's WebSocket natively: tungstenite on its own thread, talking
//! to the session through channels.

use std::io::ErrorKind;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use mp_net::transport::{Channel, NetEvent, PeerId, Transport};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, connect};

enum Out {
    Send(Vec<u8>),
    Close,
}

pub struct NativeWs {
    out: Sender<Out>,
    events: Receiver<NetEvent>,
}

impl NativeWs {
    pub fn open(url: &str) -> NativeWs {
        let (out, out_rx) = channel::<Out>();
        let (ev_tx, events) = channel();
        let url = url.to_string();
        std::thread::spawn(move || {
            let Ok((mut ws, _)) = connect(&url) else {
                let _ = ev_tx.send(NetEvent::Disconnected(0));
                return;
            };
            if let MaybeTlsStream::Plain(s) = ws.get_mut() {
                let _ = s.set_read_timeout(Some(Duration::from_millis(2)));
                let _ = s.set_nodelay(true);
            }
            let _ = ev_tx.send(NetEvent::Connected(0));
            'conn: loop {
                loop {
                    match out_rx.try_recv() {
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
                        let _ = ev_tx.send(NetEvent::Message(0, b.to_vec()));
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(tungstenite::Error::Io(e))
                        if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                    Err(_) => break,
                }
            }
            let _ = ev_tx.send(NetEvent::Disconnected(0));
        });
        NativeWs { out, events }
    }
}

impl Transport for NativeWs {
    fn send(&mut self, _peer: PeerId, _channel: Channel, bytes: &[u8]) {
        let _ = self.out.send(Out::Send(bytes.to_vec()));
    }

    fn poll(&mut self, out: &mut Vec<NetEvent>) {
        while let Ok(e) = self.events.try_recv() {
            out.push(e);
        }
    }

    fn close(&mut self, _peer: PeerId) {
        let _ = self.out.send(Out::Close);
    }
}
