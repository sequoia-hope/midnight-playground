//! Joining by link, with the signalling sealed by the link's secret
//! (MULTIPLAYER 6.1 and 8; DECISIONS D1121, D1122). No I/O here: the
//! WebRTC transport (`crate::rtc`) runs this inside its signaller, and the
//! tests run it directly.
//!
//! - [`JoinLink`]: the room and the secret, as `#join=<room>.<secret>`.
//! - [`RoomKey`]: seals and opens one signalling payload. The signalling
//!   server sees the room (in its URL) and ciphertext, never the secret.
//! - [`Plain`]: what a sealed payload holds: a WebRTC offer, answer or ICE
//!   candidate, or a [`Role`].
//! - [`Star`]: the rules that turn the server's full mesh into a star
//!   around the host: only the host offers, guests never connect to each
//!   other, and a guest listens only to the peer that proved it is the
//!   host.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};

/// Bytes of randomness in a room id and in a secret.
pub const ROOM_BYTES: usize = 12;
pub const SECRET_BYTES: usize = 16;
/// XChaCha20-Poly1305's nonce.
pub const NONCE_BYTES: usize = 24;
/// The key's domain, so the same room and secret could never key anything
/// else.
const DOMAIN: &[u8] = b"midnight-playground/signal/v1";

// ── base64url, unpadded (RFC 4648 section 5) ────────────────────────

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn b64_encode(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        let k = c.len() + 1;
        for i in 0..k {
            s.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s
}

pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| B64.iter().position(|&x| x == c).map(|v| v as u32);
    let b = s.as_bytes();
    if b.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() * 3 / 4);
    for c in b.chunks(4) {
        let mut n = 0u32;
        for (i, &ch) in c.iter().enumerate() {
            n |= val(ch)? << (18 - 6 * i);
        }
        let bytes = c.len() - 1;
        for i in 0..bytes {
            out.push((n >> (16 - 8 * i)) as u8);
        }
        // The unused low bits must be zero, so each string decodes from
        // exactly one encoding.
        if n & ((1 << (8 * (3 - bytes))) - 1) != 0 {
            return None;
        }
    }
    Some(out)
}

/// `s` as a URL query value: everything but RFC 3986's unreserved
/// characters percent-encoded (a link carries the signalling server's
/// address this way, D1124).
pub fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// ── The link ─────────────────────────────────────────────────────────

/// A room and its secret: everything a guest needs to join.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinLink {
    /// What the signalling server sees: base64url of [`ROOM_BYTES`].
    pub room: String,
    pub secret: [u8; SECRET_BYTES],
}

impl JoinLink {
    /// A new room, from `random` (which fills a buffer with secure random
    /// bytes).
    pub fn new(random: &mut dyn FnMut(&mut [u8])) -> JoinLink {
        let mut room = [0u8; ROOM_BYTES];
        let mut secret = [0u8; SECRET_BYTES];
        random(&mut room);
        random(&mut secret);
        JoinLink {
            room: b64_encode(&room),
            secret,
        }
    }

    /// `<room>.<secret>`, what goes after `#join=`.
    pub fn code(&self) -> String {
        format!("{}.{}", self.room, b64_encode(&self.secret))
    }

    /// The link's fragment: `join=<room>.<secret>`.
    pub fn fragment(&self) -> String {
        format!("join={}", self.code())
    }

    /// Reads a link: a whole URL (`…#join=…`), a fragment (`#join=…` or
    /// `join=…`), or the bare code. Anything else, or a room or secret of
    /// the wrong size, is `None`.
    pub fn parse(s: &str) -> Option<JoinLink> {
        let s = s.trim();
        let frag = s.rsplit_once('#').map_or(s, |(_, f)| f);
        // The fragment may hold other `&`-separated parts.
        let code = frag
            .split('&')
            .find_map(|p| p.strip_prefix("join="))
            .unwrap_or(frag);
        let (room, secret) = code.split_once('.')?;
        if b64_decode(room)?.len() != ROOM_BYTES {
            return None;
        }
        let secret: [u8; SECRET_BYTES] = b64_decode(secret)?.try_into().ok()?;
        Some(JoinLink {
            room: room.to_string(),
            secret,
        })
    }

    pub fn key(&self) -> RoomKey {
        let mut h = Sha256::new();
        h.update(DOMAIN);
        h.update([0]);
        h.update(self.room.as_bytes());
        h.update([0]);
        h.update(self.secret);
        RoomKey(h.finalize().into())
    }
}

// ── Sealing ──────────────────────────────────────────────────────────

/// The room's key: seals what one peer sends another through the
/// signalling server.
#[derive(Clone)]
pub struct RoomKey([u8; 32]);

impl std::fmt::Debug for RoomKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RoomKey(…)")
    }
}

fn aad(from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut a = Vec::with_capacity(from.len() + to.len() + 2);
    a.push(from.len() as u8);
    a.extend_from_slice(from);
    a.push(to.len() as u8);
    a.extend_from_slice(to);
    a
}

impl RoomKey {
    /// Seals `plain` from peer `from` to peer `to` (their ids as the server
    /// gives them, which become the associated data), with a fresh random
    /// nonce. The result is text for the signalling server's JSON.
    pub fn seal(&self, from: &[u8], to: &[u8], plain: &[u8], nonce: [u8; NONCE_BYTES]) -> String {
        let cipher = XChaCha20Poly1305::new((&self.0).into());
        let ad = aad(from, to);
        let ct = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plain,
                    aad: &ad,
                },
            )
            .expect("sealing never fails for in-memory sizes");
        let mut b = nonce.to_vec();
        b.extend_from_slice(&ct);
        b64_encode(&b)
    }

    /// Opens what [`RoomKey::seal`] made, if it was sealed with this key,
    /// from `from` to `to`, and not changed.
    pub fn open(&self, from: &[u8], to: &[u8], sealed: &str) -> Option<Vec<u8>> {
        let b = b64_decode(sealed)?;
        if b.len() < NONCE_BYTES {
            return None;
        }
        let (nonce, ct) = b.split_at(NONCE_BYTES);
        let cipher = XChaCha20Poly1305::new((&self.0).into());
        let ad = aad(from, to);
        cipher
            .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: &ad })
            .ok()
    }
}

// ── What a sealed payload holds ──────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Host,
    Guest,
}

/// A signalling payload, before sealing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plain {
    Role(Role),
    Offer(String),
    Answer(String),
    Ice(String),
}

impl Plain {
    pub fn encode(&self) -> Vec<u8> {
        let (tag, s) = match self {
            Plain::Role(Role::Host) => (1, ""),
            Plain::Role(Role::Guest) => (2, ""),
            Plain::Offer(s) => (3, s.as_str()),
            Plain::Answer(s) => (4, s.as_str()),
            Plain::Ice(s) => (5, s.as_str()),
        };
        let mut b = vec![tag];
        b.extend_from_slice(s.as_bytes());
        b
    }

    pub fn decode(b: &[u8]) -> Option<Plain> {
        let (&tag, rest) = b.split_first()?;
        let s = || String::from_utf8(rest.to_vec()).ok();
        Some(match tag {
            1 if rest.is_empty() => Plain::Role(Role::Host),
            2 if rest.is_empty() => Plain::Role(Role::Guest),
            3 => Plain::Offer(s()?),
            4 => Plain::Answer(s()?),
            5 => Plain::Ice(s()?),
            _ => return None,
        })
    }
}

// ── The star ─────────────────────────────────────────────────────────

/// What the signalling server said, once opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inbound<P> {
    IdAssigned(P),
    NewPeer(P),
    PeerLeft(P),
    /// A payload that opened with the room's key.
    Sealed {
        from: P,
        plain: Plain,
    },
}

/// What to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action<P> {
    /// Seal this and send it to that peer through the server.
    Send { to: P, plain: Plain },
    /// Tell the WebRTC socket: our id.
    Id(P),
    /// Tell the WebRTC socket to make an offer to this peer.
    Offer(P),
    /// Tell the WebRTC socket this peer's offer, answer or candidate.
    Signal { from: P, plain: Plain },
    /// Tell the WebRTC socket this peer has gone.
    Left(P),
}

/// One peer's side of the star (D1121).
///
/// When the server announces a peer, each side tells it its role. A guest
/// that hears from a host answers with its own role; the host offers to
/// every peer that has said it is a guest, once. So whichever of the two
/// arrived first, the host learns of the guest and the guest of the host
/// before the offer, and two hosts or two guests never offer.
#[derive(Clone, Debug)]
pub struct Star<P> {
    pub role: Role,
    pub me: Option<P>,
    /// A guest's host, once it has said so.
    pub host: Option<P>,
    /// The peers this side has told its role.
    pub told: Vec<P>,
    /// The host's guests it has offered to.
    pub offered: Vec<P>,
}

impl<P: Copy + Eq> Star<P> {
    pub fn new(role: Role) -> Star<P> {
        Star {
            role,
            me: None,
            host: None,
            told: Vec::new(),
            offered: Vec::new(),
        }
    }

    fn tell(&mut self, p: P, out: &mut Vec<Action<P>>) {
        if !self.told.contains(&p) {
            self.told.push(p);
            out.push(Action::Send {
                to: p,
                plain: Plain::Role(self.role),
            });
        }
    }

    /// Takes one event from the server; returns what to do.
    pub fn on(&mut self, ev: Inbound<P>) -> Vec<Action<P>> {
        let mut out = Vec::new();
        match (self.role, ev) {
            (_, Inbound::IdAssigned(me)) => {
                self.me = Some(me);
                out.push(Action::Id(me));
            }
            // Nobody talks to themselves, even if the server says so.
            (_, Inbound::NewPeer(p) | Inbound::Sealed { from: p, .. }) if Some(p) == self.me => {}
            (_, Inbound::NewPeer(p)) => self.tell(p, &mut out),
            (Role::Host, Inbound::Sealed { from, plain }) => match plain {
                Plain::Role(Role::Guest) => {
                    // The role goes out before the offer, so it arrives
                    // first.
                    self.tell(from, &mut out);
                    if !self.offered.contains(&from) {
                        self.offered.push(from);
                        out.push(Action::Offer(from));
                    }
                }
                // A second host in the room is ignored.
                Plain::Role(Role::Host) => {}
                // Answers and candidates only from guests offered to.
                plain => {
                    if self.offered.contains(&from) {
                        out.push(Action::Signal { from, plain });
                    }
                }
            },
            (Role::Guest, Inbound::Sealed { from, plain }) => match plain {
                Plain::Role(Role::Host) => {
                    if self.host.is_none() {
                        self.host = Some(from);
                        // Told already when we were here first; the host
                        // offers once either way.
                        self.told.retain(|&q| q != from);
                        self.tell(from, &mut out);
                    }
                }
                // Other guests are no one's business.
                Plain::Role(Role::Guest) => {}
                // Offers and candidates only from the host.
                plain => {
                    if self.host == Some(from) {
                        out.push(Action::Signal { from, plain });
                    }
                }
            },
            (_, Inbound::PeerLeft(p)) => {
                if self.host == Some(p) {
                    self.host = None;
                }
                self.told.retain(|&q| q != p);
                self.offered.retain(|&q| q != p);
                out.push(Action::Left(p));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_math::Mulberry32;

    /// Not secure: for the tests' rooms and nonces only.
    fn rng(seed: u32) -> impl FnMut(&mut [u8]) {
        let mut r = Mulberry32::new(seed);
        move |b: &mut [u8]| {
            for x in b {
                *x = (r.next_f64() * 256.0) as u8;
            }
        }
    }

    #[test]
    fn base64url_round_trips_and_rejects_what_is_not_canonical() {
        let mut fill = rng(1);
        for n in 0..70 {
            let mut b = vec![0u8; n];
            fill(&mut b);
            let s = b64_encode(&b);
            assert!(!s.contains(['=', '+', '/']), "{s}");
            assert_eq!(s.len(), (n * 4).div_ceil(3));
            assert_eq!(b64_decode(&s).as_deref(), Some(&b[..]));
        }
        assert_eq!(b64_encode(b"Man"), "TWFu");
        assert_eq!(b64_encode(b"Ma"), "TWE");
        assert_eq!(b64_encode(&[0xfb, 0xff]), "-_8");
        assert_eq!(b64_decode("TWE"), Some(b"Ma".to_vec()));
        // A length that no byte string has, a foreign character, padding,
        // and stray low bits.
        for bad in ["T", "TWFuT", "TW=", "TW+u", "TW/u", "TWF", "TWG", "TR"] {
            assert_eq!(b64_decode(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_link_round_trips_in_every_form_and_rejects_the_wrong_sizes() {
        let link = JoinLink::new(&mut rng(7));
        assert_eq!(link.room.len(), 16);
        let code = link.code();
        assert_eq!(code.len(), 16 + 1 + 22);
        assert!(link.fragment().starts_with("join="));
        for form in [
            code.clone(),
            format!("join={code}"),
            format!("#join={code}"),
            format!("https://x.github.io/midnight-racer/dist/next/?signal=wss://s/#join={code}"),
            format!("https://x/#mode=a&join={code}&b=c"),
            format!("  {code}\n"),
        ] {
            assert_eq!(JoinLink::parse(&form).as_ref(), Some(&link), "{form}");
        }
        let (room, secret) = code.split_once('.').unwrap();
        for bad in [
            "".to_string(),
            "join=".into(),
            room.into(),
            format!("{room}."),
            format!(".{secret}"),
            format!("{}.{secret}", &room[..12]),
            format!("{room}.{}", &secret[..20]),
            format!("{room}.{secret}AA"),
            format!("{room}!.{secret}"),
        ] {
            assert_eq!(JoinLink::parse(&bad), None, "{bad}");
        }
        // Two new rooms differ.
        let mut fill = rng(8);
        let a = JoinLink::new(&mut fill);
        let b = JoinLink::new(&mut fill);
        assert_ne!(a, b);
    }

    #[test]
    fn sealed_payloads_open_only_with_the_key_ids_and_bytes_they_were_sealed_with() {
        let link = JoinLink::new(&mut rng(3));
        let key = link.key();
        let mut fill = rng(4);
        let mut nonce = [0u8; NONCE_BYTES];
        fill(&mut nonce);
        let plain = Plain::Offer("v=0\r\no=- 1 2 IN IP4 127.0.0.1\r\n".into()).encode();
        let s = key.seal(b"alice", b"bob", &plain, nonce);
        assert_eq!(key.open(b"alice", b"bob", &s), Some(plain.clone()));
        // The secret is not in the sealed text, nor is the offer.
        assert!(!s.contains(&b64_encode(&link.secret)));
        assert!(!s.contains("IN IP4"));
        // Re-addressed or re-attributed.
        assert_eq!(key.open(b"alice", b"carol", &s), None);
        assert_eq!(key.open(b"carol", b"bob", &s), None);
        assert_eq!(key.open(b"bob", b"alice", &s), None);
        // Another room, or the same room with another secret.
        let other = JoinLink::new(&mut rng(5)).key();
        assert_eq!(other.open(b"alice", b"bob", &s), None);
        let mut guessed = link.clone();
        guessed.secret[0] ^= 1;
        assert_eq!(guessed.key().open(b"alice", b"bob", &s), None);
        let mut moved = link.clone();
        moved.room = JoinLink::new(&mut rng(6)).room;
        assert_eq!(moved.key().open(b"alice", b"bob", &s), None);
        // Any changed byte, or a cut.
        let raw = b64_decode(&s).unwrap();
        for i in 0..raw.len() {
            let mut t = raw.clone();
            t[i] ^= 0x40;
            assert_eq!(key.open(b"alice", b"bob", &b64_encode(&t)), None, "{i}");
        }
        for n in 0..raw.len() {
            assert_eq!(key.open(b"alice", b"bob", &b64_encode(&raw[..n])), None);
        }
        assert_eq!(key.open(b"alice", b"bob", "not base64!"), None);
        // A fresh nonce makes fresh text for the same message.
        fill(&mut nonce);
        let s2 = key.seal(b"alice", b"bob", &plain, nonce);
        assert_ne!(s, s2);
        assert_eq!(key.open(b"alice", b"bob", &s2), Some(plain));
    }

    #[test]
    fn payloads_round_trip_and_garbage_is_refused() {
        for p in [
            Plain::Role(Role::Host),
            Plain::Role(Role::Guest),
            Plain::Offer("o".into()),
            Plain::Answer(String::new()),
            Plain::Ice("{\"candidate\":\"c ✿\"}".into()),
        ] {
            assert_eq!(Plain::decode(&p.encode()), Some(p));
        }
        for bad in [
            &[][..],
            &[0],
            &[1, 0],
            &[2, 9],
            &[6, b'x'],
            &[3, 0xff, 0xfe],
        ] {
            assert_eq!(Plain::decode(bad), None, "{bad:?}");
        }
    }

    /// The rooms' traffic as the server would relay it: every peer's
    /// actions are carried out (a `Send` arrives as `Sealed` at its peer,
    /// in order), and the offers made are collected.
    struct Room {
        stars: Vec<Star<u32>>,
        offers: Vec<(u32, u32)>,
        /// Signals each peer's socket was handed: (to, from, plain).
        signals: Vec<(u32, u32, Plain)>,
    }

    impl Room {
        fn new() -> Room {
            Room {
                stars: Vec::new(),
                offers: Vec::new(),
                signals: Vec::new(),
            }
        }

        fn deliver(&mut self, to: u32, ev: Inbound<u32>) {
            let mut queue = vec![(to, ev)];
            while let Some((at, ev)) = queue.pop() {
                for a in self.stars[at as usize].on(ev) {
                    match a {
                        Action::Send { to, plain } => {
                            queue.insert(0, (to, Inbound::Sealed { from: at, plain }))
                        }
                        Action::Offer(p) => self.offers.push((at, p)),
                        Action::Signal { from, plain } => self.signals.push((at, from, plain)),
                        Action::Id(_) | Action::Left(_) => {}
                    }
                }
            }
        }

        /// A peer joins: the server assigns it the next id and tells every
        /// peer already here (full mesh).
        fn join(&mut self, role: Role) -> u32 {
            let id = self.stars.len() as u32;
            self.stars.push(Star::new(role));
            self.deliver(id, Inbound::IdAssigned(id));
            for p in 0..id {
                self.deliver(p, Inbound::NewPeer(id));
            }
            id
        }
    }

    fn sorted(mut v: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
        v.sort();
        v
    }

    #[test]
    fn whoever_comes_first_only_the_host_offers_and_only_to_guests() {
        // Host first, then three guests.
        let mut r = Room::new();
        let h = r.join(Role::Host);
        let g: Vec<u32> = (0..3).map(|_| r.join(Role::Guest)).collect();
        assert_eq!(
            sorted(r.offers.clone()),
            g.iter().map(|&x| (h, x)).collect::<Vec<_>>()
        );
        for &x in &g {
            assert_eq!(r.stars[x as usize].host, Some(h));
        }

        // Two guests first, then the host, then another guest.
        let mut r = Room::new();
        let a = r.join(Role::Guest);
        let b = r.join(Role::Guest);
        let h = r.join(Role::Host);
        let c = r.join(Role::Guest);
        assert_eq!(sorted(r.offers.clone()), vec![(h, a), (h, b), (h, c)]);
        for x in [a, b, c] {
            assert_eq!(r.stars[x as usize].host, Some(h), "guest {x}");
        }
        assert_eq!(r.stars[h as usize].offered.len(), 3);

        // Only guests: nobody offers, nobody has a host.
        let mut r = Room::new();
        for _ in 0..4 {
            r.join(Role::Guest);
        }
        assert!(r.offers.is_empty());
        assert!(r.stars.iter().all(|s| s.host.is_none()));
    }

    #[test]
    fn signals_pass_only_between_the_host_and_its_guests() {
        let mut r = Room::new();
        let h = r.join(Role::Host);
        let a = r.join(Role::Guest);
        let b = r.join(Role::Guest);
        let offer = Plain::Offer("sdp".into());
        let answer = Plain::Answer("sdp".into());
        // The host's offer reaches its guest's socket.
        r.deliver(
            a,
            Inbound::Sealed {
                from: h,
                plain: offer.clone(),
            },
        );
        // Another guest posing with an offer, or a candidate, is dropped.
        r.deliver(
            a,
            Inbound::Sealed {
                from: b,
                plain: offer.clone(),
            },
        );
        r.deliver(
            a,
            Inbound::Sealed {
                from: b,
                plain: Plain::Ice("c".into()),
            },
        );
        // A guest's answer reaches the host; a stranger's doesn't.
        r.deliver(
            h,
            Inbound::Sealed {
                from: a,
                plain: answer.clone(),
            },
        );
        r.deliver(
            h,
            Inbound::Sealed {
                from: 9,
                plain: answer.clone(),
            },
        );
        assert_eq!(
            r.signals,
            vec![(a, h, offer.clone()), (h, a, answer.clone())]
        );
        // A second host is ignored by the first and by the guests that
        // already have one.
        let h2 = r.join(Role::Host);
        assert_eq!(r.stars[a as usize].host, Some(h));
        assert!(!r.stars[h as usize].offered.contains(&h2));
        // The host leaves: its guests forget it and drop its signals; a new
        // host can then claim them.
        r.deliver(a, Inbound::PeerLeft(h));
        assert_eq!(r.stars[a as usize].host, None);
        r.signals.clear();
        r.deliver(
            a,
            Inbound::Sealed {
                from: h,
                plain: offer.clone(),
            },
        );
        assert!(r.signals.is_empty());
        r.deliver(
            a,
            Inbound::Sealed {
                from: h2,
                plain: Plain::Role(Role::Host),
            },
        );
        assert_eq!(r.stars[a as usize].host, Some(h2));
        // A guest that leaves can be offered to again if it comes back.
        r.deliver(h, Inbound::PeerLeft(b));
        assert!(!r.stars[h as usize].offered.contains(&b));
        r.offers.clear();
        r.deliver(
            h,
            Inbound::Sealed {
                from: b,
                plain: Plain::Role(Role::Guest),
            },
        );
        assert_eq!(r.offers, vec![(h, b)]);
        // ... but a repeated role doesn't make a second offer.
        r.deliver(
            h,
            Inbound::Sealed {
                from: b,
                plain: Plain::Role(Role::Guest),
            },
        );
        assert_eq!(r.offers, vec![(h, b)]);
    }

    #[test]
    fn the_role_goes_out_before_the_offer() {
        let mut s = Star::<u32>::new(Role::Host);
        assert_eq!(s.on(Inbound::IdAssigned(0)), vec![Action::Id(0)]);
        let role = |to, r| Action::Send {
            to,
            plain: Plain::Role(r),
        };
        assert_eq!(s.on(Inbound::NewPeer(5)), vec![role(5, Role::Host)]);
        let guest = |from| Inbound::Sealed {
            from,
            plain: Plain::Role(Role::Guest),
        };
        assert_eq!(s.on(guest(5)), vec![Action::Offer(5)]);
        // A guest the host hadn't heard of: its role, then the offer.
        assert_eq!(s.on(guest(6)), vec![role(6, Role::Host), Action::Offer(6)]);
        assert_eq!(s.on(guest(6)), vec![]);
        // Nobody talks to themselves, even if the server says so.
        assert_eq!(s.on(Inbound::NewPeer(0)), vec![]);
        assert_eq!(s.on(guest(0)), vec![]);

        let mut g = Star::<u32>::new(Role::Guest);
        assert_eq!(g.on(Inbound::IdAssigned(2)), vec![Action::Id(2)]);
        assert_eq!(g.on(Inbound::NewPeer(3)), vec![role(3, Role::Guest)]);
        let host = Inbound::Sealed {
            from: 3,
            plain: Plain::Role(Role::Host),
        };
        // The host answers the role it was told: the guest says it again
        // (the host offers once either way).
        assert_eq!(g.on(host.clone()), vec![role(3, Role::Guest)]);
        assert_eq!(g.on(host), vec![]);
        assert_eq!(g.on(Inbound::PeerLeft(3)), vec![Action::Left(3)]);
        assert_eq!(g.host, None);
    }
}
