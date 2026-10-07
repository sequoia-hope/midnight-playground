//! The WebRTC transport (SPEC 9.3, MULTIPLAYER 8; DECISIONS D1120 to
//! D1122): `matchbox_socket` with our own signaller, which seals every
//! signalling payload with the room's key and keeps the room a star
//! around the host ([`crate::signal`]). The same code runs natively
//! (webrtc-rs) and in the browser (the page's `RTCPeerConnection`).
//!
//! - Channel 0 is reliable and ordered, channel 1 unordered with no
//!   retransmits: `Channel::Reliable` and `Channel::Unreliable`.
//! - A guest sees the host as peer 0, as over WebSocket; the host numbers
//!   its guests from 1.
//! - The socket's message loop runs on its own thread natively and as a
//!   task of the page in the browser; [`RtcNet`] talks to it through
//!   matchbox's channels, so `poll` and `send` never block.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use futures::{SinkExt, StreamExt};
use matchbox_protocol::PeerEvent as WireEvent;
use matchbox_protocol::PeerRequest as WireRequest;
use matchbox_socket::async_trait::async_trait;
use matchbox_socket::{
    ChannelConfig, PeerEvent, PeerId as RtcId, PeerRequest, PeerSignal, PeerState, SignalingError,
    Signaller, SignallerBuilder, WebRtcSocket,
};

use crate::signal::{Action, Inbound, JoinLink, NONCE_BYTES, Plain, Role, RoomKey, Star};
use crate::transport::{Channel, NetEvent, PeerId, Transport};

/// How far along the connection is, for the lobby's status line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RtcStatus {
    /// Reaching the signalling server.
    Signalling,
    /// In the room: a guest waiting for the host to answer, a host waiting
    /// for guests.
    InRoom,
    /// A guest has found its host and is connecting to it.
    FoundHost,
    /// The signalling server could not be reached, or dropped us.
    Failed(String),
}

#[derive(Debug)]
struct Shared {
    status: RtcStatus,
    /// The message loop has ended.
    ended: bool,
}

type SharedRef = Arc<Mutex<Shared>>;

fn lock(s: &SharedRef) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// ICE servers (STUN, TURN) for the peers to find a way to each other.
pub use matchbox_socket::RtcIceServerConfig as IceServers;

/// What a WebRTC transport needs.
#[derive(Clone, Debug)]
pub struct RtcOptions {
    /// The signalling server: `wss://…/` (the room is added to the path).
    pub signal: String,
    pub link: JoinLink,
    pub role: Role,
    /// The ICE servers; `None` for matchbox's default (public STUN).
    pub ice: Option<IceServers>,
}

impl RtcOptions {
    /// The room's address on the signalling server.
    pub fn room_url(&self) -> String {
        let base = self.signal.split(['?', '#']).next().unwrap_or("");
        format!("{}/{}", base.trim_end_matches('/'), self.link.room)
    }
}

/// Secure random bytes: the OS's natively, `crypto.getRandomValues` in the
/// browser.
pub fn random_bytes(b: &mut [u8]) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        getrandom::fill(b).expect("the OS has random numbers");
    }
    #[cfg(target_arch = "wasm32")]
    {
        let crypto = web_sys::window()
            .and_then(|w| w.crypto().ok())
            .expect("the page has crypto");
        // At most 65536 bytes a call.
        for c in b.chunks_mut(65536) {
            crypto
                .get_random_values_with_u8_array(c)
                .expect("getRandomValues");
        }
    }
}

/// A new room's link.
pub fn new_link() -> JoinLink {
    JoinLink::new(&mut random_bytes)
}

// ── The sealed signaller ─────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
mod ws {
    use async_tungstenite::async_std::{ConnectStream, connect_async};
    use async_tungstenite::tungstenite::Message;
    use matchbox_socket::SignalingError;

    pub type Ws = async_tungstenite::WebSocketStream<ConnectStream>;
    pub type Msg = Message;

    pub async fn connect(url: &str) -> Result<Ws, SignalingError> {
        Ok(connect_async(url).await?.0)
    }
    pub fn text(s: String) -> Msg {
        Message::Text(s.into())
    }
    /// The next text message's text; `None` at the end. Other messages are
    /// skipped.
    pub fn read(m: Option<Result<Msg, async_tungstenite::tungstenite::Error>>) -> Read {
        match m {
            Some(Ok(Message::Text(t))) => Read::Text(t.to_string()),
            Some(Ok(_)) => Read::Skip,
            Some(Err(e)) => Read::Err(e.into()),
            None => Read::End,
        }
    }
    pub enum Read {
        Text(String),
        Skip,
        Err(SignalingError),
        End,
    }
}

#[cfg(target_arch = "wasm32")]
mod ws {
    use matchbox_socket::SignalingError;
    use ws_stream_wasm::{WsMessage, WsMeta, WsStream};

    pub type Ws = WsStream;
    pub type Msg = WsMessage;

    pub async fn connect(url: &str) -> Result<Ws, SignalingError> {
        Ok(WsMeta::connect(url, None).await?.1)
    }
    pub fn text(s: String) -> Msg {
        WsMessage::Text(s)
    }
    pub fn read(m: Option<Msg>) -> Read {
        match m {
            Some(WsMessage::Text(t)) => Read::Text(t),
            Some(_) => Read::Skip,
            None => Read::End,
        }
    }
    pub enum Read {
        Text(String),
        Skip,
        #[allow(dead_code)]
        Err(SignalingError),
        End,
    }
}

#[derive(Debug)]
struct SealedBuilder {
    key: RoomKey,
    role: Role,
    shared: SharedRef,
}

struct Sealed {
    ws: ws::Ws,
    key: RoomKey,
    star: Star<RtcId>,
    /// Events for the socket, in order.
    events: VecDeque<PeerEvent>,
    /// Text for the server, in order, not yet handed to the WebSocket.
    outbox: VecDeque<String>,
    shared: SharedRef,
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl SignallerBuilder for SealedBuilder {
    async fn new_signaller(
        &self,
        mut attempts: Option<u16>,
        room_url: String,
    ) -> Result<Box<dyn Signaller>, SignalingError> {
        let ws = loop {
            match ws::connect(&room_url).await {
                Ok(ws) => break ws,
                Err(e) => match attempts.as_mut() {
                    Some(n) if *n <= 1 => {
                        return Err(SignalingError::NegotiationFailed(Box::new(e)));
                    }
                    Some(n) => {
                        *n -= 1;
                        futures_timer::Delay::new(std::time::Duration::from_secs(2)).await;
                    }
                    None => {
                        futures_timer::Delay::new(std::time::Duration::from_secs(2)).await;
                    }
                },
            }
        };
        lock(&self.shared).status = RtcStatus::InRoom;
        Ok(Box::new(Sealed {
            ws,
            key: self.key.clone(),
            star: Star::new(self.role),
            events: VecDeque::new(),
            outbox: VecDeque::new(),
            shared: self.shared.clone(),
        }))
    }
}

fn plain_of(s: PeerSignal) -> Plain {
    match s {
        PeerSignal::Offer(s) => Plain::Offer(s),
        PeerSignal::Answer(s) => Plain::Answer(s),
        PeerSignal::IceCandidate(s) => Plain::Ice(s),
    }
}

impl Sealed {
    /// Seals `plain` for `to` and queues it for the server.
    fn queue_sealed(&mut self, to: RtcId, plain: &Plain) {
        // Our id comes first from the server; nothing is sent before it.
        let Some(me) = self.star.me else { return };
        let mut nonce = [0u8; NONCE_BYTES];
        random_bytes(&mut nonce);
        let data = self
            .key
            .seal(me.0.as_bytes(), to.0.as_bytes(), &plain.encode(), nonce);
        let req: WireRequest<String> = WireRequest::Signal { receiver: to, data };
        self.outbox
            .push_back(serde_json::to_string(&req).expect("a request serialises"));
    }

    /// Hands the outbox to the WebSocket. Cancel-safe: a message leaves
    /// the outbox only once the WebSocket has taken it (a cancelled flush
    /// is finished by the next one).
    async fn flush(&mut self) -> Result<(), SignalingError> {
        while let Some(t) = self.outbox.front() {
            futures::future::poll_fn(|cx| self.ws.poll_ready_unpin(cx)).await?;
            self.ws.start_send_unpin(ws::text(t.clone()))?;
            self.outbox.pop_front();
        }
        futures::future::poll_fn(|cx| self.ws.poll_flush_unpin(cx)).await?;
        Ok(())
    }

    /// One message from the server, through the star; no awaiting, so
    /// nothing is lost if `next_message` is cancelled.
    fn take(&mut self, text: &str) {
        let Ok(ev) = serde_json::from_str::<WireEvent<serde_json::Value>>(text) else {
            return;
        };
        let inbound = match ev {
            WireEvent::IdAssigned(p) => Inbound::IdAssigned(p),
            WireEvent::NewPeer(p) => Inbound::NewPeer(p),
            WireEvent::PeerLeft(p) => Inbound::PeerLeft(p),
            WireEvent::Signal { sender, data } => {
                // Anything that doesn't open with the room's key, from that
                // sender to us, is dropped here.
                let (Some(me), Some(s)) = (self.star.me, data.as_str()) else {
                    return;
                };
                let Some(plain) = self
                    .key
                    .open(sender.0.as_bytes(), me.0.as_bytes(), s)
                    .and_then(|b| Plain::decode(&b))
                else {
                    return;
                };
                Inbound::Sealed {
                    from: sender,
                    plain,
                }
            }
        };
        for a in self.star.on(inbound) {
            match a {
                Action::Send { to, plain } => self.queue_sealed(to, &plain),
                Action::Id(p) => self.events.push_back(PeerEvent::IdAssigned(p)),
                Action::Offer(p) => self.events.push_back(PeerEvent::NewPeer(p)),
                Action::Left(p) => self.events.push_back(PeerEvent::PeerLeft(p)),
                Action::Signal { from, plain } => {
                    let data = match plain {
                        Plain::Offer(s) => PeerSignal::Offer(s),
                        Plain::Answer(s) => PeerSignal::Answer(s),
                        Plain::Ice(s) => PeerSignal::IceCandidate(s),
                        Plain::Role(_) => continue,
                    };
                    self.events
                        .push_back(PeerEvent::Signal { sender: from, data });
                }
            }
        }
        if self.star.role == Role::Guest {
            let mut sh = lock(&self.shared);
            if !matches!(sh.status, RtcStatus::Failed(_)) {
                sh.status = if self.star.host.is_some() {
                    RtcStatus::FoundHost
                } else {
                    RtcStatus::InRoom
                };
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Signaller for Sealed {
    async fn send(&mut self, request: PeerRequest) -> Result<(), SignalingError> {
        match request {
            PeerRequest::KeepAlive => {
                let req: WireRequest<String> = WireRequest::KeepAlive;
                self.outbox
                    .push_back(serde_json::to_string(&req).expect("a request serialises"));
            }
            PeerRequest::Signal { receiver, data } => {
                self.queue_sealed(receiver, &plain_of(data));
            }
        }
        self.flush().await
    }

    async fn next_message(&mut self) -> Result<PeerEvent, SignalingError> {
        loop {
            self.flush().await?;
            if let Some(e) = self.events.pop_front() {
                return Ok(e);
            }
            match ws::read(self.ws.next().await) {
                ws::Read::Text(t) => self.take(&t),
                ws::Read::Skip => {}
                ws::Read::Err(e) => return Err(e),
                ws::Read::End => return Err(SignalingError::StreamExhausted),
            }
        }
    }
}

// ── The transport ────────────────────────────────────────────────────

/// A transport's [`RtcStatus`], kept by whoever handed the transport on
/// (the tab's host gives its `RtcNet` to a `Mux`, and the lobby still
/// shows how the room is doing).
#[derive(Clone, Debug)]
pub struct StatusHandle(SharedRef);

impl StatusHandle {
    pub fn get(&self) -> RtcStatus {
        lock(&self.0).status.clone()
    }
}

/// A [`Transport`] over WebRTC data channels.
pub struct RtcNet {
    socket: WebRtcSocket,
    role: Role,
    shared: SharedRef,
    ids: BTreeMap<RtcId, PeerId>,
    rev: BTreeMap<PeerId, RtcId>,
    next: PeerId,
    /// Peers the host has closed: their messages are dropped (matchbox
    /// can't close one peer's connection).
    forgotten: BTreeSet<RtcId>,
    /// The end of the message loop has been told to the session.
    end_told: bool,
    /// Events for the next `poll` (a close's disconnect).
    pending: Vec<NetEvent>,
}

impl RtcNet {
    /// Opens the room (a host) or joins it (a guest). Connecting goes on in
    /// the background; `poll` reports peers as they connect.
    pub fn open(opts: RtcOptions) -> RtcNet {
        let shared: SharedRef = Arc::new(Mutex::new(Shared {
            status: RtcStatus::Signalling,
            ended: false,
        }));
        let builder = SealedBuilder {
            key: opts.link.key(),
            role: opts.role,
            shared: shared.clone(),
        };
        let mut b = WebRtcSocket::builder(opts.room_url())
            .signaller_builder(Arc::new(builder))
            .add_channel(ChannelConfig::reliable())
            .add_channel(ChannelConfig::unreliable());
        if let Some(ice) = opts.ice.clone() {
            b = b.ice_server(ice);
        }
        let (socket, run) = b.build();
        let sh = shared.clone();
        let done = async move {
            let r = run.await;
            let mut s = lock(&sh);
            s.ended = true;
            if let Err(e) = r {
                s.status = RtcStatus::Failed(e.to_string());
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::Builder::new()
            .name("webrtc".into())
            .spawn(move || futures::executor::block_on(done))
            .expect("a thread for the WebRTC loop");
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(done);
        RtcNet {
            socket,
            role: opts.role,
            shared,
            ids: BTreeMap::new(),
            rev: BTreeMap::new(),
            next: 1,
            forgotten: BTreeSet::new(),
            end_told: false,
            pending: Vec::new(),
        }
    }

    pub fn status(&self) -> RtcStatus {
        lock(&self.shared).status.clone()
    }

    /// The status, readable after the transport has gone into a `Mux`.
    pub fn status_handle(&self) -> StatusHandle {
        StatusHandle(self.shared.clone())
    }

    /// Peers connected now.
    pub fn peers(&self) -> usize {
        self.ids.len()
    }

    fn id_of(&mut self, p: RtcId) -> PeerId {
        if let Some(&i) = self.ids.get(&p) {
            return i;
        }
        // A guest's peer is its host: 0.
        let i = if self.role == Role::Guest && !self.rev.contains_key(&0) {
            0
        } else {
            let i = self.next;
            self.next += 1;
            i
        };
        self.ids.insert(p, i);
        self.rev.insert(i, p);
        i
    }

    fn drop_id(&mut self, p: RtcId) -> Option<PeerId> {
        let i = self.ids.remove(&p)?;
        self.rev.remove(&i);
        Some(i)
    }
}

impl Transport for RtcNet {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        let Some(&p) = self.rev.get(&peer) else {
            return;
        };
        let ch = match channel {
            Channel::Reliable => 0,
            Channel::Unreliable => 1,
        };
        if let Ok(c) = self.socket.get_channel_mut(ch) {
            let _ = c.try_send(bytes.into(), p);
        }
    }

    fn poll(&mut self, out: &mut Vec<NetEvent>) {
        out.append(&mut self.pending);
        if let Ok(changes) = self.socket.try_update_peers() {
            for (p, state) in changes {
                match state {
                    PeerState::Connected => {
                        if !self.forgotten.contains(&p) {
                            let i = self.id_of(p);
                            out.push(NetEvent::Connected(i));
                        }
                    }
                    PeerState::Disconnected => {
                        self.forgotten.remove(&p);
                        if let Some(i) = self.drop_id(p) {
                            out.push(NetEvent::Disconnected(i));
                        }
                    }
                }
            }
        }
        for ch in 0..2 {
            let Ok(c) = self.socket.get_channel_mut(ch) else {
                continue;
            };
            for (p, packet) in c.receive() {
                if let Some(&i) = self.ids.get(&p) {
                    out.push(NetEvent::Message(i, packet.into_vec()));
                }
            }
        }
        // The loop ended (the signalling server went away, or we closed):
        // every peer with it.
        if !self.end_told && lock(&self.shared).ended {
            self.end_told = true;
            let gone: Vec<RtcId> = self.ids.keys().copied().collect();
            for p in gone {
                if let Some(i) = self.drop_id(p) {
                    out.push(NetEvent::Disconnected(i));
                }
            }
            // A guest that never reached its host is told it's over too.
            if self.role == Role::Guest && !out.contains(&NetEvent::Disconnected(0)) {
                out.push(NetEvent::Disconnected(0));
            }
        }
    }

    fn close(&mut self, peer: PeerId) {
        match self.role {
            Role::Guest => self.socket.close(),
            Role::Host => {
                if let Some(&p) = self.rev.get(&peer) {
                    self.forgotten.insert(p);
                    self.drop_id(p);
                    self.pending.push(NetEvent::Disconnected(peer));
                }
            }
        }
    }
}

impl Drop for RtcNet {
    fn drop(&mut self) {
        self.socket.close();
    }
}
