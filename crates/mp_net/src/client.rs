//! A player's session (SPEC 9.1, WP 10.4): joins a host, follows the
//! lobby, and in a race simulates the whole world locally, ahead of the
//! host, rolling back when the host's final inputs differ from its guesses.
//!
//! - **Prediction.** Each tick uses this player's own input, and for every
//!   other human the last input the host confirmed for them (edges like
//!   reset not repeated).
//! - **Rollback.** The states since the last fully confirmed tick are kept.
//!   When the host's inputs for a tick differ from the ones used, the client
//!   restores the state before that tick and steps forward again.
//! - **Clock.** Pings measure the round trip and the host's tick; the client
//!   runs ahead of the host by half the round trip plus a margin, so its
//!   inputs reach the host before the host needs them.
//! - **Desync check.** Every [`crate::host::HASH_EVERY`] ticks the host
//!   sends its state hash; the client compares it with its own confirmed
//!   state. On a mismatch it rebuilds the race from the start and the
//!   confirmed inputs (the simulation is deterministic, so this repairs
//!   anything but a determinism bug, which it counts).
//!
//! As with the host, time comes in from outside (`now`, ms).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use mp_sim::input::{InputFrame, RESET};
use mp_sim::race::{LevelRuntime, SimEvent, SimState, hash, step};

use crate::host::{HASH_EVERY, TICK_MS, multi_opts};
use crate::proto::{Msg, PlayerInfo, PointsRow, RaceStart, Settings, Slot, VERSION};
use crate::transport::{Channel, NetEvent, Transport};

/// Inputs sent per message: this tick and the ones before (a lost packet
/// costs nothing; SPEC 9.2).
pub const REDUNDANCY: usize = 8;
/// The furthest the client predicts past the host's last confirmed tick
/// (SPEC 9.2's rollback window); beyond it, it waits.
pub const MAX_AHEAD: u32 = 32;
/// Ticks of safety on top of half the round trip.
pub const MARGIN_TICKS: f64 = 2.0;
/// Most ticks stepped in one update (a catch-up after a rejoin is allowed
/// more, see `CATCH_UP`).
pub const MAX_STEPS: u32 = 8;
const CATCH_UP: u32 = 600;
/// Ping every this many ms (faster while the clock is new).
const PING_MS: f64 = 250.0;

/// What the lobby looks like from here.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LobbyView {
    pub settings: Option<Settings>,
    pub players: Vec<PlayerInfo>,
    pub points: Vec<PointsRow>,
    pub raced: u32,
    pub racing: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    Welcome(Slot),
    Rejected(String),
    Lobby,
    RaceStarted,
    RaceEnded,
    Disconnected,
}

/// Counters for the debug overlay and the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetStats {
    pub rollbacks: u32,
    /// Ticks re-simulated by rollbacks.
    pub resimulated: u32,
    pub hashes_checked: u32,
    pub desyncs: u32,
    /// Rebuilds that still disagreed with the host (a determinism bug).
    pub unrepaired: u32,
    /// Round trip, ms (the best recent sample).
    pub rtt: f64,
    /// Ticks this client runs ahead of the host's confirmed tick.
    pub ahead: i64,
    /// Ticks stalled waiting for the host.
    pub stalls: u32,
}

pub struct ClientRace {
    pub start: RaceStart,
    pub lr: Arc<LevelRuntime>,
    /// This player's index in the race.
    pub me: usize,
    n: usize,
    init: SimState,
    /// States after ticks `base..=local`: the front is the last tick whose
    /// inputs are all confirmed and agreed; the back is now.
    ring: VecDeque<SimState>,
    /// The inputs each predicted tick used (ticks `base+1..=local`).
    used: VecDeque<Vec<InputFrame>>,
    /// The host's final inputs, `n` per tick, ticks `1..=confirmed()`.
    confirmed: Vec<InputFrame>,
    /// This player's own inputs, by tick.
    mine: BTreeMap<u32, InputFrame>,
    /// Hashes from the host and of our confirmed states, by tick.
    host_hashes: BTreeMap<u32, u64>,
    our_hashes: BTreeMap<u32, u64>,
    /// Events of ticks simulated for the first time, for the client to show.
    pub events: Vec<SimEvent>,
    emitted_to: u32,
}

impl ClientRace {
    fn base(&self) -> u32 {
        self.ring.front().unwrap().tick
    }
    pub fn local(&self) -> u32 {
        self.ring.back().unwrap().tick
    }
    pub fn confirmed(&self) -> u32 {
        (self.confirmed.len() / self.n) as u32
    }
    /// The state now (predicted).
    pub fn state(&self) -> &SimState {
        self.ring.back().unwrap()
    }
    /// The state a tick before now (for drawing between the two).
    pub fn prev(&self) -> &SimState {
        let k = self.ring.len();
        &self.ring[k.saturating_sub(2)]
    }
    fn confirmed_at(&self, tick: u32) -> Option<&[InputFrame]> {
        let i = (tick as usize).checked_sub(1)? * self.n;
        self.confirmed.get(i..i + self.n)
    }

    /// The inputs to step tick `tick` with: the host's if it has spoken,
    /// otherwise our own and a guess for the others.
    fn frames_for(&self, tick: u32) -> Vec<InputFrame> {
        if let Some(c) = self.confirmed_at(tick) {
            return c.to_vec();
        }
        let last = self.confirmed();
        (0..self.n)
            .map(|h| {
                if h == self.me {
                    self.mine.get(&tick).copied().unwrap_or_default()
                } else if last > 0 {
                    let f = self.confirmed[(last as usize - 1) * self.n + h];
                    InputFrame {
                        flags: f.flags & !RESET,
                        ..f
                    }
                } else {
                    InputFrame::default()
                }
            })
            .collect()
    }

    /// Steps one tick forward from the back of the ring.
    fn step_once(&mut self, record_events: bool) {
        let tick = self.local() + 1;
        let frames = self.frames_for(tick);
        let mut st = self.ring.back().unwrap().clone();
        let mut ev = Vec::new();
        step(&self.lr, &mut st, &frames, &mut ev);
        if record_events && tick > self.emitted_to {
            self.events.extend(ev);
            self.emitted_to = tick;
        }
        self.ring.push_back(st);
        self.used.push_back(frames);
    }
}

pub struct Client<T: Transport> {
    pub net: T,
    pub name: String,
    pub car: String,
    pub color: u32,
    pub slot: Option<Slot>,
    pub lobby: LobbyView,
    pub race: Option<ClientRace>,
    /// A race the host started that this client hasn't built yet: the app
    /// loads the level, then calls [`Client::attach`].
    pub pending: Option<RaceStart>,
    /// When the race's tick 0 starts on the host's clock (ms).
    begin_at: Option<f64>,
    /// The host's inputs that came before the race was built (a rejoin
    /// gets the whole race so far), handed to it when it is.
    early: Vec<InputFrame>,
    /// The tick this client aimed for last update, with its fraction.
    target: f64,
    pub events: Vec<ClientEvent>,
    pub stats: NetStats,
    connected: bool,
    hello_sent: bool,
    // Clock.
    ping_id: u32,
    last_ping: f64,
    pings: BTreeMap<u32, f64>,
    /// Recent (rtt, host clock offset in ms) samples; the best rtt's offset
    /// wins.
    samples: VecDeque<(f64, f64)>,
    offset: Option<f64>,
    inbox: Vec<NetEvent>,
}

impl<T: Transport> Client<T> {
    pub fn new(net: T, name: &str, car: &str, color: u32) -> Client<T> {
        Client {
            net,
            name: name.into(),
            car: car.into(),
            color,
            slot: None,
            lobby: LobbyView::default(),
            race: None,
            pending: None,
            begin_at: None,
            early: Vec::new(),
            target: 0.0,
            events: Vec::new(),
            stats: NetStats::default(),
            connected: false,
            hello_sent: false,
            ping_id: 0,
            last_ping: f64::NEG_INFINITY,
            pings: BTreeMap::new(),
            samples: VecDeque::new(),
            offset: None,
            inbox: Vec::new(),
        }
    }

    pub fn connected(&self) -> bool {
        self.connected
    }

    fn send(&mut self, ch: Channel, m: &Msg) {
        self.net.send(0, ch, &m.encode());
    }

    pub fn set_me(&mut self, name: &str, car: &str, color: u32) {
        self.name = name.into();
        self.car = car.into();
        self.color = color;
        let m = Msg::SetMe {
            name: name.into(),
            car: car.into(),
            color,
        };
        self.send(Channel::Reliable, &m);
    }

    /// The lobby's leader: the connected player with the lowest slot.
    pub fn leader(&self) -> Option<Slot> {
        self.lobby
            .players
            .iter()
            .filter(|p| p.connected)
            .map(|p| p.slot)
            .min()
    }

    pub fn is_leader(&self) -> bool {
        self.slot.is_some() && self.leader() == self.slot
    }

    /// The leader's controls: the settings, start and abort.
    pub fn configure(&mut self, s: Settings) {
        self.send(Channel::Reliable, &Msg::Configure(s));
    }

    pub fn go(&mut self, start: bool) {
        self.send(Channel::Reliable, &Msg::Go(start));
    }

    pub fn ready(&mut self, r: bool) {
        self.send(Channel::Reliable, &Msg::Ready(r));
    }

    pub fn leave(&mut self) {
        self.send(Channel::Reliable, &Msg::Leave);
        self.net.close(0);
        self.connected = false;
        self.race = None;
    }

    /// The race's tick on the host now, as best we know (before tick 0 it
    /// is negative; `None` until the race has begun and the clock is known).
    pub fn host_tick(&self, now: f64) -> Option<f64> {
        Some((now + self.offset? - self.begin_at?) / TICK_MS)
    }

    /// How far the drawn frame is between the previous tick and the current
    /// one (0..1), from where this client is aiming.
    pub fn alpha(&self) -> f64 {
        match &self.race {
            Some(r) if self.target >= r.local() as f64 => {
                (self.target - r.local() as f64).clamp(0.0, 1.0)
            }
            _ => 1.0,
        }
    }

    /// Builds the race the host started, once the app has its level, and
    /// tells the host this player is ready to go. Returns false if there is
    /// no such race or the level differs.
    pub fn attach(&mut self, lr: Arc<LevelRuntime>) -> bool {
        let Some(start) = self.pending.take() else {
            return false;
        };
        if start.settings.level != lr.level.id {
            self.pending = Some(start);
            return false;
        }
        self.begin(start, lr);
        self.send(Channel::Reliable, &Msg::Loaded);
        true
    }

    /// Handles the network, keeps the clock, and in a race steps forward to
    /// where this client should be, asking `input` for this player's
    /// controls once per new tick (with the state it will be applied to).
    pub fn update(&mut self, now: f64, mut input: impl FnMut(&SimState, usize) -> InputFrame) {
        let mut inbox = std::mem::take(&mut self.inbox);
        self.net.poll(&mut inbox);
        for ev in inbox.drain(..) {
            match ev {
                NetEvent::Connected(_) => {
                    self.connected = true;
                }
                NetEvent::Disconnected(_) => {
                    self.connected = false;
                    self.race = None;
                    self.events.push(ClientEvent::Disconnected);
                }
                NetEvent::Message(_, bytes) => {
                    if let Ok(m) = Msg::decode(&bytes) {
                        self.on_msg(m, now);
                    }
                }
            }
        }
        self.inbox = inbox;
        if self.connected && !self.hello_sent {
            self.hello_sent = true;
            let m = Msg::Hello {
                version: VERSION,
                name: self.name.clone(),
                car: self.car.clone(),
                color: self.color,
            };
            self.send(Channel::Reliable, &m);
        }
        if !self.connected {
            return;
        }
        // Ping: quickly at first, then steadily.
        let every = if self.samples.len() < 5 {
            60.0
        } else {
            PING_MS
        };
        if now - self.last_ping >= every {
            self.last_ping = now;
            self.ping_id += 1;
            self.pings.insert(self.ping_id, now);
            if self.pings.len() > 32 {
                self.pings.pop_first();
            }
            let m = Msg::Ping {
                id: self.ping_id,
                t: now,
            };
            self.send(Channel::Reliable, &m);
        }
        self.reconcile();
        self.advance(now, &mut input);
    }

    fn on_msg(&mut self, m: Msg, now: f64) {
        match m {
            Msg::Welcome { slot } => {
                self.slot = Some(slot);
                self.events.push(ClientEvent::Welcome(slot));
            }
            Msg::Reject { reason } => {
                self.events.push(ClientEvent::Rejected(reason));
            }
            Msg::Lobby {
                settings,
                players,
                points,
                raced,
                racing,
            } => {
                self.lobby = LobbyView {
                    settings: Some(settings),
                    players,
                    points,
                    raced,
                    racing,
                };
                self.events.push(ClientEvent::Lobby);
            }
            Msg::Start(start) => {
                // Built when the app has loaded the level (`attach`).
                self.race = None;
                self.begin_at = None;
                self.early.clear();
                self.pending = Some(start);
                self.events.push(ClientEvent::RaceStarted);
            }
            Msg::Begin { at } => self.begin_at = Some(at),
            Msg::Inputs {
                first,
                humans,
                frames,
            } => {
                let Some(r) = &mut self.race else {
                    // Not built yet: keep them in order for when it is.
                    if let Some(p) = &self.pending {
                        let n = p.humans.len();
                        let have = (self.early.len() / n) as u32;
                        if humans as usize == n && first <= have + 1 {
                            let skip = (have + 1 - first) as usize * n;
                            if skip < frames.len() {
                                self.early.extend_from_slice(&frames[skip..]);
                            }
                        }
                    }
                    return;
                };
                if humans as usize != r.n {
                    return;
                }
                let have = r.confirmed();
                let n = r.n;
                // Ordered and reliable: skip any overlap, ignore a gap.
                if first > have + 1 {
                    return;
                }
                let skip = (have + 1 - first) as usize * n;
                if skip < frames.len() {
                    r.confirmed.extend_from_slice(&frames[skip..]);
                }
            }
            Msg::Hash { tick, hash } => {
                if let Some(r) = &mut self.race {
                    r.host_hashes.insert(tick, hash);
                }
            }
            Msg::Pong { id, t, host_ms } => {
                if self.pings.remove(&id).is_some() {
                    let rtt = now - t;
                    let offset = host_ms + rtt / 2.0 - now;
                    self.samples.push_back((rtt, offset));
                    if self.samples.len() > 16 {
                        self.samples.pop_front();
                    }
                    let best = self
                        .samples
                        .iter()
                        .copied()
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .unwrap();
                    self.stats.rtt = best.0;
                    self.offset = Some(best.1);
                }
            }
            Msg::End => {
                self.race = None;
                self.pending = None;
                self.begin_at = None;
                self.events.push(ClientEvent::RaceEnded);
            }
            _ => {}
        }
    }

    fn begin(&mut self, start: RaceStart, lr: Arc<LevelRuntime>) {
        let Some(slot) = self.slot else { return };
        let Some(me) = start.humans.iter().position(|p| p.slot == slot) else {
            return;
        };
        let init = SimState::new_multi(&lr, &multi_opts(&start));
        let n = start.humans.len();
        self.race = Some(ClientRace {
            start,
            lr,
            me,
            n,
            ring: VecDeque::from([init.clone()]),
            init,
            used: VecDeque::new(),
            confirmed: std::mem::take(&mut self.early),
            mine: BTreeMap::new(),
            host_hashes: BTreeMap::new(),
            our_hashes: BTreeMap::new(),
            events: Vec::new(),
            emitted_to: 0,
        });
    }

    /// Takes in the host's inputs: moves the confirmed base forward, rolls
    /// back from the first tick guessed wrong, and checks hashes.
    fn reconcile(&mut self) {
        let Some(r) = &mut self.race else { return };
        let upto = r.confirmed().min(r.local());
        let base = r.base();
        let mut bad = None;
        for tick in base + 1..=upto {
            let k = (tick - base - 1) as usize;
            if Some(&r.used[k][..]) != r.confirmed_at(tick) {
                bad = Some(tick);
                break;
            }
        }
        if let Some(m) = bad {
            // Back to the state before tick m, then forward again.
            let keep = (m - base) as usize; // states base..m-1
            let to = r.local();
            r.ring.truncate(keep);
            r.used.truncate(keep - 1);
            for _ in m..=to {
                r.step_once(false);
            }
            self.stats.rollbacks += 1;
            self.stats.resimulated += to - m + 1;
        }
        // Everything up to `upto` now used the confirmed inputs: move the
        // base there, noting our hashes on the way.
        let r = self.race.as_mut().unwrap();
        while r.base() < upto {
            r.ring.pop_front();
            r.used.pop_front();
            let st = r.ring.front().unwrap();
            if st.tick.is_multiple_of(HASH_EVERY) {
                r.our_hashes.insert(st.tick, hash(st));
            }
        }
        // Compare hashes where both sides have one.
        let ticks: Vec<u32> = r
            .host_hashes
            .keys()
            .copied()
            .filter(|t| r.our_hashes.contains_key(t))
            .collect();
        let mut desync = false;
        for t in ticks {
            let theirs = r.host_hashes.remove(&t).unwrap();
            let ours = r.our_hashes.remove(&t).unwrap();
            self.stats.hashes_checked += 1;
            if theirs != ours {
                desync = true;
            }
        }
        // Old hashes we'll never match (from before a rebuild) go.
        let b = r.base();
        r.our_hashes.retain(|&t, _| t + 10 * HASH_EVERY > b);
        r.host_hashes.retain(|&t, _| t + 10 * HASH_EVERY > b);
        if desync {
            self.stats.desyncs += 1;
            self.rebuild();
        }
    }

    /// Rebuilds the race from its start and every confirmed input, then
    /// predicts back to where we were.
    fn rebuild(&mut self) {
        let Some(r) = &mut self.race else { return };
        let to = r.local();
        let c = r.confirmed();
        let host_hashes = std::mem::take(&mut r.host_hashes);
        r.ring = VecDeque::from([r.init.clone()]);
        r.used.clear();
        r.our_hashes.clear();
        let mut unrepaired = false;
        while r.local() < c {
            r.step_once(false);
            r.ring.pop_front();
            r.used.pop_front();
            let st = r.ring.front().unwrap();
            if let Some(h) = host_hashes.get(&st.tick)
                && *h != hash(st)
            {
                unrepaired = true;
            }
        }
        while r.local() < to {
            r.step_once(false);
        }
        if unrepaired {
            self.stats.unrepaired += 1;
        }
    }

    /// Steps forward to the target tick.
    fn advance(&mut self, now: f64, input: &mut impl FnMut(&SimState, usize) -> InputFrame) {
        let Some(host) = self.host_tick(now) else {
            return;
        };
        let rtt_ticks = self.stats.rtt / TICK_MS;
        let spread = self
            .samples
            .iter()
            .map(|s| s.0)
            .fold(0.0f64, f64::max)
            .min(200.0)
            / TICK_MS
            - rtt_ticks;
        let aim = host + rtt_ticks / 2.0 + MARGIN_TICKS + spread.max(0.0) * 0.5;
        self.target = aim;
        let Some(r) = &mut self.race else { return };
        if aim < 1.0 {
            return;
        }
        let target = aim.floor() as u32;
        let behind = target.saturating_sub(r.local());
        let budget = if behind > 2 * MAX_AHEAD {
            CATCH_UP
        } else {
            MAX_STEPS
        };
        let mut steps = 0;
        let mut sent_from = None;
        while r.local() < target && steps < budget {
            let tick = r.local() + 1;
            // Past the window: wait for the host (unless catching up on
            // confirmed ticks after a rejoin).
            if tick > r.confirmed() + MAX_AHEAD {
                self.stats.stalls += 1;
                break;
            }
            if r.confirmed_at(tick).is_none() {
                let f = input(r.state(), r.me);
                r.mine.insert(tick, f);
                sent_from.get_or_insert(tick);
            }
            r.step_once(true);
            steps += 1;
        }
        self.stats.ahead = r.local() as i64 - r.confirmed() as i64;
        // Send our inputs for the newest ticks, with the few before.
        if let Some(_from) = sent_from {
            let last = r.local();
            let first = last.saturating_sub(REDUNDANCY as u32 - 1).max(1);
            let frames: Vec<InputFrame> = (first..=last)
                .map(|t| r.mine.get(&t).copied().unwrap_or_default())
                .collect();
            let c = r.confirmed();
            r.mine.retain(|&t, _| t + 4 * REDUNDANCY as u32 > c);
            let m = Msg::Input { first, frames };
            self.net.send(0, Channel::Unreliable, &m.encode());
        }
    }
}
