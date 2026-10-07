//! Transports (SPEC 9.3): how bytes get between the host and the clients.
//! The session code sees only [`Transport`]; WebSocket (native host and the
//! web client) comes in WP 10.5, WebRTC in M11. This file has the two that
//! need no network: [`SimNet`], an in-process network on a virtual clock
//! with latency, jitter, loss and reordering, for tests and the headless
//! soak; with every knob at zero it is the loopback of single-player.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use mp_math::Mulberry32;

/// A connection's id. On a client the host is always peer 0.
pub type PeerId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// Delivered, in order (WebSocket is always this).
    Reliable,
    /// May be lost or reordered.
    Unreliable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetEvent {
    Connected(PeerId),
    Disconnected(PeerId),
    Message(PeerId, Vec<u8>),
}

pub trait Transport {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]);
    fn poll(&mut self, out: &mut Vec<NetEvent>);
    /// Ends a connection (a kick, or leaving).
    fn close(&mut self, peer: PeerId);
}

/// A boxed transport is a transport (the client picks WebSocket or the
/// loopback at run time).
impl Transport for Box<dyn Transport> {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        (**self).send(peer, channel, bytes)
    }
    fn poll(&mut self, out: &mut Vec<NetEvent>) {
        (**self).poll(out)
    }
    fn close(&mut self, peer: PeerId) {
        (**self).close(peer)
    }
}

/// One direction of a link's conditions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conditions {
    /// One-way delay, ms.
    pub latency: f64,
    /// Extra random delay, 0..jitter ms.
    pub jitter: f64,
    /// Chance an unreliable message is lost.
    pub loss: f64,
}

impl Conditions {
    pub const PERFECT: Conditions = Conditions {
        latency: 0.0,
        jitter: 0.0,
        loss: 0.0,
    };
    pub const LAN: Conditions = Conditions {
        latency: 2.0,
        jitter: 2.0,
        loss: 0.0,
    };
}

struct Packet {
    due: f64,
    seq: u64,
    to: usize,
    from: PeerId,
    ev: NetEvent,
}

struct Inner {
    now: f64,
    seq: u64,
    rng: Mulberry32,
    /// Per endpoint (0 is the host): its link's conditions toward it, and
    /// whether it is connected.
    cond: Vec<Conditions>,
    up: Vec<bool>,
    /// Per endpoint, the due time of the last reliable message sent to it
    /// from each side, so reliable delivery stays in order.
    last_reliable_to_host: Vec<f64>,
    last_reliable_to_client: Vec<f64>,
    in_flight: Vec<Packet>,
    inbox: Vec<VecDeque<NetEvent>>,
}

/// An in-process star network: the host is endpoint 0, clients 1..; each
/// client's link has its own [`Conditions`] (both directions). Time moves
/// only by [`SimNet::advance_to`], so a test is deterministic for a seed.
#[derive(Clone)]
pub struct SimNet(Rc<RefCell<Inner>>);

impl SimNet {
    pub fn new(seed: u32) -> SimNet {
        SimNet(Rc::new(RefCell::new(Inner {
            now: 0.0,
            seq: 0,
            rng: Mulberry32::new(seed),
            cond: vec![Conditions::PERFECT],
            up: vec![true],
            last_reliable_to_host: vec![0.0],
            last_reliable_to_client: vec![0.0],
            in_flight: Vec::new(),
            inbox: vec![VecDeque::new()],
        })))
    }

    /// The host's end.
    pub fn host(&self) -> SimEnd {
        SimEnd {
            net: self.clone(),
            me: 0,
        }
    }

    /// Connects a new client with these link conditions; returns its end.
    /// The host sees `Connected(peer)` with the client's peer id, the
    /// client sees `Connected(0)`.
    pub fn connect(&self, cond: Conditions) -> SimEnd {
        let mut n = self.0.borrow_mut();
        let me = n.cond.len();
        n.cond.push(cond);
        n.up.push(true);
        n.last_reliable_to_host.push(0.0);
        n.last_reliable_to_client.push(0.0);
        n.inbox.push(VecDeque::new());
        n.inbox[me].push_back(NetEvent::Connected(0));
        n.inbox[0].push_back(NetEvent::Connected(me as PeerId));
        SimEnd {
            net: self.clone(),
            me,
        }
    }

    /// Changes a client's link conditions from now on.
    pub fn set_conditions(&self, client: &SimEnd, cond: Conditions) {
        self.0.borrow_mut().cond[client.me] = cond;
    }

    /// Cuts a client off abruptly (both sides see `Disconnected`); what was
    /// in flight is lost.
    pub fn cut(&self, client: &SimEnd) {
        let mut n = self.0.borrow_mut();
        Self::drop_link(&mut n, client.me);
    }

    fn drop_link(n: &mut Inner, me: usize) {
        if !n.up[me] {
            return;
        }
        n.up[me] = false;
        n.in_flight
            .retain(|p| !(p.to == me || (p.to == 0 && p.from == me as PeerId)));
        n.inbox[me].push_back(NetEvent::Disconnected(0));
        n.inbox[0].push_back(NetEvent::Disconnected(me as PeerId));
    }

    pub fn now(&self) -> f64 {
        self.0.borrow().now
    }

    /// Moves the clock to `t` ms and delivers everything due by then, in
    /// due order (ties in send order).
    pub fn advance_to(&self, t: f64) {
        let mut n = self.0.borrow_mut();
        n.now = n.now.max(t);
        let now = n.now;
        let mut due: Vec<Packet> = Vec::new();
        let mut i = 0;
        while i < n.in_flight.len() {
            if n.in_flight[i].due <= now {
                due.push(n.in_flight.swap_remove(i));
            } else {
                i += 1;
            }
        }
        due.sort_by(|a, b| a.due.total_cmp(&b.due).then(a.seq.cmp(&b.seq)));
        for p in due {
            n.inbox[p.to].push_back(p.ev);
        }
    }

    /// Messages still on the wire.
    pub fn in_flight(&self) -> usize {
        self.0.borrow().in_flight.len()
    }
}

/// One endpoint of a [`SimNet`].
pub struct SimEnd {
    net: SimNet,
    me: usize,
}

impl SimEnd {
    /// This end's peer id as the host knows it (0 for the host).
    pub fn id(&self) -> PeerId {
        self.me as PeerId
    }
}

impl Transport for SimEnd {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        let mut n = self.net.0.borrow_mut();
        // The client end of the link sets the conditions both ways.
        let (to, link) = if self.me == 0 {
            (peer as usize, peer as usize)
        } else {
            (0, self.me)
        };
        if to >= n.up.len() || !n.up[link] {
            return;
        }
        let c = n.cond[link];
        if channel == Channel::Unreliable && c.loss > 0.0 && n.rng.next_f64() < c.loss {
            return;
        }
        let jitter = if c.jitter > 0.0 {
            n.rng.next_f64() * c.jitter
        } else {
            0.0
        };
        let mut due = n.now + c.latency + jitter;
        if channel == Channel::Reliable {
            let last = if to == 0 {
                &mut n.last_reliable_to_host[link]
            } else {
                &mut n.last_reliable_to_client[link]
            };
            due = due.max(*last);
            *last = due;
        }
        n.seq += 1;
        let seq = n.seq;
        let from = self.me as PeerId;
        let ev = NetEvent::Message(if to == 0 { from } else { 0 }, bytes.to_vec());
        // Straight to the inbox on a perfect link, unless something for the
        // same end is still in flight (sent before the link became perfect):
        // then behind it, so reliable order holds.
        let queued = n.in_flight.iter().any(|p| p.to == to && p.from == from);
        if c.latency == 0.0 && c.jitter == 0.0 && !queued {
            n.inbox[to].push_back(ev);
        } else {
            n.in_flight.push(Packet {
                due,
                seq,
                to,
                from,
                ev,
            });
        }
    }

    fn poll(&mut self, out: &mut Vec<NetEvent>) {
        out.extend(self.net.0.borrow_mut().inbox[self.me].drain(..));
    }

    fn close(&mut self, peer: PeerId) {
        let mut n = self.net.0.borrow_mut();
        let link = if self.me == 0 { peer as usize } else { self.me };
        if link < n.up.len() {
            SimNet::drop_link(&mut n, link);
        }
    }
}

/// Several transports as one, for a host that listens on more than one
/// (MULTIPLAYER 8.1, DECISIONS D1125): the host tab's own player on the
/// in-process loopback and everyone else on WebRTC; `mp-host --room` on
/// WebSocket and WebRTC. Each inner peer gets its own number here, from 1,
/// never reused; the session sees only those.
#[derive(Default)]
pub struct Mux {
    parts: Vec<Box<dyn Transport>>,
    /// Ours → (part, its peer).
    to_inner: std::collections::BTreeMap<PeerId, (usize, PeerId)>,
    /// (part, its peer) → ours.
    to_outer: std::collections::BTreeMap<(usize, PeerId), PeerId>,
    next: PeerId,
    buf: Vec<NetEvent>,
}

impl Mux {
    pub fn new() -> Mux {
        Mux {
            next: 1,
            ..Mux::default()
        }
    }

    /// Adds a transport; returns its index.
    pub fn add(&mut self, t: Box<dyn Transport>) -> usize {
        self.parts.push(t);
        self.parts.len() - 1
    }

    /// The transport at `i` (to reach what only it has, such as a status).
    pub fn part_mut(&mut self, i: usize) -> Option<&mut Box<dyn Transport>> {
        self.parts.get_mut(i)
    }

    /// Which transport, and which of its peers, our `peer` is.
    pub fn inner(&self, peer: PeerId) -> Option<(usize, PeerId)> {
        self.to_inner.get(&peer).copied()
    }

    fn outer(&mut self, part: usize, inner: PeerId) -> PeerId {
        if let Some(&p) = self.to_outer.get(&(part, inner)) {
            return p;
        }
        let p = self.next.max(1);
        self.next = p + 1;
        self.to_outer.insert((part, inner), p);
        self.to_inner.insert(p, (part, inner));
        p
    }
}

impl Transport for Mux {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        if let Some(&(part, inner)) = self.to_inner.get(&peer) {
            self.parts[part].send(inner, channel, bytes);
        }
    }

    fn poll(&mut self, out: &mut Vec<NetEvent>) {
        let mut buf = std::mem::take(&mut self.buf);
        for part in 0..self.parts.len() {
            self.parts[part].poll(&mut buf);
            for ev in buf.drain(..) {
                out.push(match ev {
                    NetEvent::Connected(p) => NetEvent::Connected(self.outer(part, p)),
                    NetEvent::Message(p, b) => NetEvent::Message(self.outer(part, p), b),
                    NetEvent::Disconnected(p) => {
                        // A peer never seen has nothing to say goodbye to.
                        let Some(o) = self.to_outer.remove(&(part, p)) else {
                            continue;
                        };
                        self.to_inner.remove(&o);
                        NetEvent::Disconnected(o)
                    }
                });
            }
        }
        self.buf = buf;
    }

    fn close(&mut self, peer: PeerId) {
        if let Some(&(part, inner)) = self.to_inner.get(&peer) {
            self.parts[part].close(inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs(e: &mut SimEnd) -> Vec<Vec<u8>> {
        let mut ev = Vec::new();
        e.poll(&mut ev);
        ev.into_iter()
            .filter_map(|e| match e {
                NetEvent::Message(_, b) => Some(b),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn loopback_delivers_at_once_both_ways() {
        let net = SimNet::new(1);
        let mut h = net.host();
        let mut c = net.connect(Conditions::PERFECT);
        let mut ev = Vec::new();
        h.poll(&mut ev);
        assert_eq!(ev, vec![NetEvent::Connected(1)]);
        c.send(0, Channel::Reliable, b"hi");
        h.send(1, Channel::Unreliable, b"yo");
        ev.clear();
        h.poll(&mut ev);
        assert_eq!(ev, vec![NetEvent::Message(1, b"hi".to_vec())]);
        let mut ev = Vec::new();
        c.poll(&mut ev);
        assert_eq!(
            ev,
            vec![NetEvent::Connected(0), NetEvent::Message(0, b"yo".to_vec())]
        );
    }

    #[test]
    fn latency_holds_messages_until_due_and_reliable_keeps_order() {
        let net = SimNet::new(7);
        let mut h = net.host();
        let mut c = net.connect(Conditions {
            latency: 30.0,
            jitter: 25.0,
            loss: 0.5,
        });
        for i in 0..200u8 {
            c.send(0, Channel::Reliable, &[i]);
        }
        net.advance_to(29.0);
        let mut ev = Vec::new();
        h.poll(&mut ev);
        assert_eq!(ev, vec![NetEvent::Connected(1)]);
        net.advance_to(100.0);
        let got = msgs(&mut h);
        assert_eq!(got, (0..200u8).map(|i| vec![i]).collect::<Vec<_>>());
    }

    #[test]
    fn unreliable_messages_can_be_lost_and_reordered() {
        let net = SimNet::new(3);
        let mut h = net.host();
        let mut c = net.connect(Conditions {
            latency: 10.0,
            jitter: 40.0,
            loss: 0.2,
        });
        for i in 0..250u8 {
            c.send(0, Channel::Unreliable, &[i]);
        }
        net.advance_to(1000.0);
        let got: Vec<u8> = msgs(&mut h).into_iter().map(|b| b[0]).collect();
        assert!(got.len() > 150 && got.len() < 240, "{}", got.len());
        assert!(got.windows(2).any(|w| w[0] > w[1]), "some reordering");
    }

    #[test]
    fn a_cut_link_disconnects_both_ends_and_drops_what_was_in_flight() {
        let net = SimNet::new(1);
        let mut h = net.host();
        let mut a = net.connect(Conditions::LAN);
        let mut b = net.connect(Conditions::LAN);
        a.send(0, Channel::Reliable, b"a");
        b.send(0, Channel::Reliable, b"b");
        net.cut(&a);
        net.advance_to(50.0);
        let mut ev = Vec::new();
        h.poll(&mut ev);
        assert!(ev.contains(&NetEvent::Disconnected(1)));
        assert!(ev.contains(&NetEvent::Message(2, b"b".to_vec())));
        assert!(!ev.contains(&NetEvent::Message(1, b"a".to_vec())));
        let mut ev = Vec::new();
        a.poll(&mut ev);
        assert!(ev.contains(&NetEvent::Disconnected(0)));
        // Sends on a dead link go nowhere.
        a.send(0, Channel::Reliable, b"late");
        net.advance_to(100.0);
        assert!(msgs(&mut h).is_empty());
    }

    /// Two networks behind one `Mux`: each inner peer gets its own number,
    /// messages and closes reach the right one, and a number is never
    /// reused after a disconnect.
    #[test]
    fn a_mux_numbers_every_inner_peer_and_routes_to_it() {
        let lan = SimNet::new(1);
        let far = SimNet::new(2);
        let mut mux = Mux::new();
        assert_eq!(mux.add(Box::new(lan.host())), 0);
        assert_eq!(mux.add(Box::new(far.host())), 1);
        let mut a = lan.connect(Conditions::PERFECT);
        let mut b = far.connect(Conditions::PERFECT);
        let mut c = lan.connect(Conditions::PERFECT);
        let mut ev = Vec::new();
        mux.poll(&mut ev);
        // The parts are polled in order: the LAN's two, then the far one.
        assert_eq!(
            ev,
            vec![
                NetEvent::Connected(1),
                NetEvent::Connected(2),
                NetEvent::Connected(3)
            ]
        );
        assert_eq!(mux.inner(1), Some((0, a.id())));
        assert_eq!(mux.inner(2), Some((0, c.id())));
        assert_eq!(mux.inner(3), Some((1, b.id())));
        // Both inner networks number their first client 1: no mix-up.
        assert_eq!(a.id(), b.id());

        b.send(0, Channel::Reliable, b"from b");
        a.send(0, Channel::Unreliable, b"from a");
        ev.clear();
        mux.poll(&mut ev);
        assert_eq!(
            ev,
            vec![
                NetEvent::Message(1, b"from a".to_vec()),
                NetEvent::Message(3, b"from b".to_vec())
            ]
        );
        mux.send(3, Channel::Reliable, b"to b");
        mux.send(2, Channel::Reliable, b"to c");
        mux.send(9, Channel::Reliable, b"to nobody");
        assert_eq!(msgs(&mut b), vec![b"to b".to_vec()]);
        assert_eq!(msgs(&mut c), vec![b"to c".to_vec()]);
        assert!(msgs(&mut a).is_empty());

        // Closing ours closes theirs; the disconnect comes back numbered.
        mux.close(1);
        ev.clear();
        mux.poll(&mut ev);
        assert_eq!(ev, vec![NetEvent::Disconnected(1)]);
        assert_eq!(mux.inner(1), None);
        let mut ev_a = Vec::new();
        a.poll(&mut ev_a);
        assert!(ev_a.contains(&NetEvent::Disconnected(0)));
        // A newcomer gets a fresh number, not the freed one.
        let _d = lan.connect(Conditions::PERFECT);
        ev.clear();
        mux.poll(&mut ev);
        assert_eq!(ev, vec![NetEvent::Connected(4)]);
        // A peer that comes and goes between two polls: both, in order.
        far.cut(&far.connect(Conditions::PERFECT));
        ev.clear();
        mux.poll(&mut ev);
        assert_eq!(ev, vec![NetEvent::Connected(5), NetEvent::Disconnected(5)]);
        assert!(mux.part_mut(1).is_some() && mux.part_mut(2).is_none());
    }
}
