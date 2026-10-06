//! The host's session (SPEC 9.1, WP 10.3): the lobby, the authoritative
//! race, the input relay and the session's points table. It runs anywhere
//! a [`Transport`] does: in `mp-host` natively, in a tab later (M11), and in
//! the tests on [`crate::transport::SimNet`].
//!
//! Time comes in from outside (`now`, ms), so the session is deterministic
//! under test and never reads a clock itself.
//!
//! During a race the host steps the simulation on its own clock. For each
//! tick it uses each human's input if it has arrived, and otherwise repeats
//! that human's last one (edges like reset are not repeated). Once a tick is
//! stepped its inputs are final: they are relayed to every client, in
//! order, and a client that guessed differently rolls back. A human who has
//! disconnected is driven by the autopilot (MULTIPLAYER 2.7) until they
//! come back.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use mp_math::Mulberry32;
use mp_sim::input::{AUTOPILOT, AWAY, InputFrame, RESET};
use mp_sim::race::{DT, Human, LevelRuntime, MultiOpts, SimEvent, SimState, hash, results, step};

use crate::proto::{
    GridRule, MAX_PLAYERS, Msg, PlayerInfo, PointsRow, RaceStart, Settings, Slot, VERSION,
};
use crate::transport::{Channel, NetEvent, PeerId, Transport};

/// Builds a level's runtime by id (the host app knows where Seaside's
/// survey data is).
pub type Levels = Rc<dyn Fn(&str) -> Option<Arc<LevelRuntime>>>;

/// Ticks per second.
pub const HZ: f64 = 1.0 / DT;
/// The tick length in ms.
pub const TICK_MS: f64 = DT * 1000.0;
/// Tick 0 begins this long after the Begin message goes out, so it reaches
/// everyone first.
pub const BEGIN_LEAD_MS: f64 = 500.0;
/// The longest the host waits for everyone to load the level before it
/// begins anyway (a slow phone then catches up).
pub const LOAD_WAIT_MS: f64 = 30_000.0;
/// A state hash goes out every this many ticks (SPEC 9.2's snapshot rate).
pub const HASH_EVERY: u32 = 30;
/// At most this many ticks are stepped in one update (a stalled host
/// catches up over several updates rather than all at once).
pub const MAX_CATCH_UP: u32 = 24;
/// Points by finishing place (MULTIPLAYER 4.5).
pub const POINTS: [u32; 8] = [10, 8, 6, 5, 4, 3, 2, 1];

/// The cars a player may pick.
const CARS: [&str; 5] = ["sports", "muscle", "super", "rally", "electric"];

fn car_id(s: &str) -> &'static str {
    CARS.iter().copied().find(|c| *c == s).unwrap_or("sports")
}

struct Player {
    info: PlayerInfo,
    peer: Option<PeerId>,
}

struct Race {
    start: RaceStart,
    lr: Arc<LevelRuntime>,
    st: SimState,
    /// Host time of tick 0's start, ms, once everyone has loaded.
    t0: Option<f64>,
    /// When the Start went out, ms.
    started_at: f64,
    /// Which humans have loaded the level.
    loaded: Vec<bool>,
    /// Each human's inputs from the network, by tick, not yet used.
    pending: Vec<BTreeMap<u32, InputFrame>>,
    /// Each human's last used input (what is repeated when one is late).
    last: Vec<InputFrame>,
    /// The final inputs of every tick stepped: `humans` frames per tick.
    log: Vec<InputFrame>,
    /// The humans' slots, in race order.
    slots: Vec<Slot>,
    over: bool,
}

/// What the host app is told about.
#[derive(Clone, Debug, PartialEq)]
pub enum HostEvent {
    Joined(Slot),
    Left(Slot),
    RaceStarted { race: u32 },
    RaceEnded { race: u32 },
}

pub struct Host<T: Transport> {
    pub net: T,
    levels: Levels,
    pub settings: Settings,
    players: Vec<Player>,
    points: Vec<PointsRow>,
    raced: u32,
    /// The last race's human finishing order (slots), for the grid rules.
    last_order: Vec<Slot>,
    race: Option<Race>,
    seed: Mulberry32,
    /// Events for the host app.
    pub events: Vec<HostEvent>,
    inbox: Vec<NetEvent>,
}

impl<T: Transport> Host<T> {
    /// `seed` makes the races' seeds and random grids (the app passes the
    /// time or a random number; tests a constant).
    pub fn new(net: T, levels: Levels, seed: u32) -> Host<T> {
        Host {
            net,
            levels,
            settings: Settings::default(),
            players: Vec::new(),
            points: Vec::new(),
            raced: 0,
            last_order: Vec::new(),
            race: None,
            seed: Mulberry32::new(seed),
            events: Vec::new(),
            inbox: Vec::new(),
        }
    }

    pub fn racing(&self) -> bool {
        self.race.is_some()
    }

    /// The race's state, while there is one.
    pub fn state(&self) -> Option<&SimState> {
        self.race.as_ref().map(|r| &r.st)
    }

    /// The tick the host has stepped to.
    pub fn tick(&self) -> u32 {
        self.race.as_ref().map_or(0, |r| r.st.tick)
    }

    pub fn players(&self) -> Vec<PlayerInfo> {
        self.players.iter().map(|p| p.info.clone()).collect()
    }

    pub fn points(&self) -> &[PointsRow] {
        &self.points
    }

    /// The final inputs of the current race so far (for replays and tests).
    pub fn input_log(&self) -> Option<(&[InputFrame], usize)> {
        self.race.as_ref().map(|r| (&r.log[..], r.slots.len()))
    }

    /// Changes the lobby settings (the host's own controls).
    pub fn set_settings(&mut self, s: Settings) {
        self.settings = s;
        self.send_lobby();
    }

    /// The leader picks the settings and starts races: the connected player
    /// with the lowest slot (the host app is headless).
    fn is_leader(&self, i: usize) -> bool {
        let lowest = self
            .players
            .iter()
            .filter(|p| p.peer.is_some())
            .map(|p| p.info.slot)
            .min();
        lowest == Some(self.players[i].info.slot)
    }

    fn slot_of(&self, peer: PeerId) -> Option<usize> {
        self.players.iter().position(|p| p.peer == Some(peer))
    }

    /// Handles the network and steps the race up to `now` (ms).
    pub fn update(&mut self, now: f64) {
        let mut inbox = std::mem::take(&mut self.inbox);
        self.net.poll(&mut inbox);
        for ev in inbox.drain(..) {
            match ev {
                NetEvent::Connected(_) => {}
                NetEvent::Disconnected(peer) => self.drop_peer(peer),
                NetEvent::Message(peer, bytes) => match Msg::decode(&bytes) {
                    Ok(m) => self.on_msg(peer, m, now),
                    // A peer speaking nonsense is dropped.
                    Err(_) => {
                        self.net.close(peer);
                        self.drop_peer(peer);
                    }
                },
            }
        }
        self.inbox = inbox;
        self.step_race(now);
    }

    fn on_msg(&mut self, peer: PeerId, m: Msg, now: f64) {
        let me = self.slot_of(peer);
        match (m, me) {
            (
                Msg::Hello {
                    version,
                    name,
                    car,
                    color,
                },
                None,
            ) => self.hello(peer, version, name, car, color, now),
            (Msg::SetMe { name, car, color }, Some(i)) => {
                let p = &mut self.players[i].info;
                p.name = clean_name(&name);
                p.car = car_id(&car).into();
                p.color = color;
                self.send_lobby();
            }
            (Msg::Ready(r), Some(i)) => {
                self.players[i].info.ready = r;
                self.send_lobby();
            }
            (Msg::Input { first, frames }, Some(i)) => self.input(i, first, frames),
            (Msg::Ping { id, t }, _) => {
                self.send(
                    peer,
                    Channel::Reliable,
                    &Msg::Pong {
                        id,
                        t,
                        host_ms: now,
                    },
                );
            }
            (Msg::Loaded, Some(i)) => {
                let slot = self.players[i].info.slot;
                if let Some(r) = &mut self.race
                    && let Some(h) = r.slots.iter().position(|&s| s == slot)
                {
                    r.loaded[h] = true;
                }
            }
            (Msg::Configure(set), Some(i)) if self.is_leader(i) => {
                let mut set = set;
                if !known_level(&set.level) {
                    set.level = self.settings.level.clone();
                }
                self.set_settings(set);
            }
            (Msg::Go(true), Some(i)) if self.is_leader(i) => {
                self.start(now);
            }
            (Msg::Go(false), Some(i)) if self.is_leader(i) => self.abort(),
            (Msg::Leave, Some(_)) => {
                self.net.close(peer);
                self.drop_peer(peer);
            }
            // Anything else out of turn is ignored.
            _ => {}
        }
    }

    fn hello(
        &mut self,
        peer: PeerId,
        version: u16,
        name: String,
        car: String,
        color: u32,
        now: f64,
    ) {
        if version != VERSION {
            self.send(
                peer,
                Channel::Reliable,
                &Msg::Reject {
                    reason: format!("this host speaks version {VERSION}, you speak {version}"),
                },
            );
            self.net.close(peer);
            return;
        }
        let name = clean_name(&name);
        // Someone back after a drop takes their old slot (and car) back.
        let back = self
            .players
            .iter()
            .position(|p| p.peer.is_none() && p.info.name == name);
        let i = match back {
            Some(i) => i,
            None => {
                let live = self.players.iter().filter(|p| p.peer.is_some()).count();
                if live >= MAX_PLAYERS {
                    self.send(
                        peer,
                        Channel::Reliable,
                        &Msg::Reject {
                            reason: "the lobby is full".into(),
                        },
                    );
                    self.net.close(peer);
                    return;
                }
                // A free slot: one never used, or one whose player left
                // and isn't in the current race.
                let in_race = |s: Slot| self.race.as_ref().is_some_and(|r| r.slots.contains(&s));
                let reuse = self
                    .players
                    .iter()
                    .position(|p| p.peer.is_none() && !in_race(p.info.slot));
                let slot = match reuse {
                    Some(k) => self.players[k].info.slot,
                    None => self.players.len() as Slot,
                };
                let info = PlayerInfo {
                    slot,
                    name,
                    car: car_id(&car).into(),
                    color,
                    ready: false,
                    connected: true,
                };
                match reuse {
                    Some(k) => {
                        self.players[k] = Player { info, peer: None };
                        k
                    }
                    None => {
                        self.players.push(Player { info, peer: None });
                        self.players.len() - 1
                    }
                }
            }
        };
        self.players[i].peer = Some(peer);
        self.players[i].info.connected = true;
        let slot = self.players[i].info.slot;
        self.send(peer, Channel::Reliable, &Msg::Welcome { slot });
        self.events.push(HostEvent::Joined(slot));
        // Back into a race they're in: the race, and every input so far,
        // so they can catch up by replaying it.
        if let Some(r) = &self.race
            && r.slots.contains(&slot)
        {
            let start = Msg::Start(r.start.clone());
            let begin = r.t0.map(|at| Msg::Begin { at });
            let n = r.slots.len();
            let log = r.log.clone();
            self.send(peer, Channel::Reliable, &start);
            if let Some(b) = begin {
                self.send(peer, Channel::Reliable, &b);
            }
            for (k, chunk) in log.chunks(n * 600).enumerate() {
                self.send(
                    peer,
                    Channel::Reliable,
                    &Msg::Inputs {
                        first: 1 + (k * 600) as u32,
                        humans: n as u8,
                        frames: chunk.to_vec(),
                    },
                );
            }
        }
        let _ = now;
        self.send_lobby();
    }

    fn drop_peer(&mut self, peer: PeerId) {
        if let Some(i) = self.slot_of(peer) {
            self.players[i].peer = None;
            self.players[i].info.connected = false;
            self.players[i].info.ready = false;
            self.events.push(HostEvent::Left(self.players[i].info.slot));
            self.send_lobby();
        }
    }

    fn input(&mut self, i: usize, first: u32, frames: Vec<InputFrame>) {
        let Some(r) = &mut self.race else { return };
        let slot = self.players[i].info.slot;
        let Some(h) = r.slots.iter().position(|&s| s == slot) else {
            return;
        };
        let done = r.st.tick;
        for (k, f) in frames.into_iter().enumerate() {
            let tick = first.wrapping_add(k as u32);
            // Late (already stepped) or absurdly early inputs are dropped.
            if tick > done && tick <= done + 600 {
                r.pending[h].insert(tick, f);
            }
        }
    }

    /// Starts a race with the connected players (the host's Start button).
    /// Returns false if there's no one to race or the level is unknown.
    pub fn start(&mut self, now: f64) -> bool {
        if self.race.is_some() {
            return false;
        }
        let Some(lr) = (self.levels)(&self.settings.level) else {
            return false;
        };
        let humans: Vec<PlayerInfo> = self
            .players
            .iter()
            .filter(|p| p.peer.is_some())
            .map(|p| p.info.clone())
            .collect();
        if humans.is_empty() {
            return false;
        }
        let seed = (self.seed.next_f64() * 4294967296.0) as u32;
        let grid = self.grid(&humans, seed);
        let start = RaceStart {
            race: self.raced,
            settings: self.settings.clone(),
            seed,
            humans,
            grid,
        };
        let st = SimState::new_multi(&lr, &multi_opts(&start));
        let n = start.humans.len();
        let slots = start.humans.iter().map(|p| p.slot).collect();
        self.broadcast(Channel::Reliable, &Msg::Start(start.clone()));
        self.race = Some(Race {
            start,
            lr,
            st,
            t0: None,
            started_at: now,
            loaded: vec![false; n],
            pending: vec![BTreeMap::new(); n],
            last: vec![InputFrame::default(); n],
            log: Vec::new(),
            slots,
            over: false,
        });
        for p in &mut self.players {
            p.info.ready = false;
        }
        self.events
            .push(HostEvent::RaceStarted { race: self.raced });
        self.send_lobby();
        true
    }

    /// The humans' grid order, as indexes into `humans` (MULTIPLAYER 2.2).
    fn grid(&self, humans: &[PlayerInfo], seed: u32) -> Vec<u8> {
        let n = humans.len();
        let mut order: Vec<u8> = (0..n as u8).collect();
        let by_last = |rev: bool| {
            // Those in the last race in its finishing order (reversed for
            // the reverse grid), newcomers first.
            let mut o: Vec<u8> = (0..n as u8).collect();
            let rank = |i: u8| {
                self.last_order
                    .iter()
                    .position(|&s| s == humans[i as usize].slot)
            };
            o.sort_by_key(|&i| match rank(i) {
                None => (0, 0),
                Some(r) if rev => (1, usize::MAX - r),
                Some(r) => (1, r),
            });
            o
        };
        match self.settings.grid {
            GridRule::Reverse if !self.last_order.is_empty() => by_last(true),
            GridRule::Same if !self.last_order.is_empty() => by_last(false),
            _ => {
                let mut rng = Mulberry32::new(seed ^ 0x9e37_79b9);
                for i in (1..n).rev() {
                    let j = (rng.next_f64() * (i + 1) as f64) as usize;
                    order.swap(i, j);
                }
                order
            }
        }
    }

    fn step_race(&mut self, now: f64) {
        let Some(r) = &mut self.race else { return };
        if r.t0.is_none() {
            // Begin once every connected human has the level loaded, or the
            // wait is over.
            let all = r.slots.iter().zip(&r.loaded).all(|(&s, &l)| {
                l || !self
                    .players
                    .iter()
                    .any(|p| p.info.slot == s && p.peer.is_some())
            });
            if all || now - r.started_at > LOAD_WAIT_MS {
                let at = now + BEGIN_LEAD_MS;
                r.t0 = Some(at);
                self.broadcast(Channel::Reliable, &Msg::Begin { at });
            }
            return;
        }
        let r = self.race.as_mut().unwrap();
        let t0 = r.t0.unwrap();
        let due = ((now - t0) / TICK_MS).floor();
        let mut out = Vec::new();
        let first = r.st.tick + 1;
        let mut ended = false;
        let mut stepped = 0;
        let mut hashes = Vec::new();
        while !r.over && (r.st.tick as f64) < due && stepped < MAX_CATCH_UP {
            let tick = r.st.tick + 1;
            let mut frames = Vec::with_capacity(r.slots.len());
            for h in 0..r.slots.len() {
                let connected = self
                    .players
                    .iter()
                    .any(|p| p.info.slot == r.slots[h] && p.peer.is_some());
                // Drop anything older than this tick, take this tick's.
                while let Some((&t, _)) = r.pending[h].first_key_value() {
                    if t >= tick {
                        break;
                    }
                    r.pending[h].pop_first();
                }
                let f = match r.pending[h].remove(&tick) {
                    Some(f) => f,
                    // Late: repeat the last, without its edges.
                    None => InputFrame {
                        flags: r.last[h].flags & !RESET,
                        ..r.last[h]
                    },
                };
                let f = if connected {
                    InputFrame {
                        flags: f.flags & !AUTOPILOT,
                        ..f
                    }
                } else {
                    InputFrame {
                        flags: AUTOPILOT | (f.flags & AWAY),
                        ..InputFrame::default()
                    }
                };
                r.last[h] = f;
                frames.push(f);
            }
            let from = out.len();
            step(&r.lr, &mut r.st, &frames, &mut out);
            r.log.extend_from_slice(&frames);
            if r.st.tick.is_multiple_of(HASH_EVERY) {
                hashes.push((r.st.tick, hash(&r.st)));
            }
            if out[from..].contains(&SimEvent::Results) {
                ended = true;
                r.over = true;
            }
            stepped += 1;
        }
        if stepped > 0 {
            let n = r.slots.len();
            let frames = r.log[(first as usize - 1) * n..].to_vec();
            let msg = Msg::Inputs {
                first,
                humans: n as u8,
                frames,
            };
            self.broadcast(Channel::Reliable, &msg);
            for (tick, hash) in hashes {
                self.broadcast(Channel::Reliable, &Msg::Hash { tick, hash });
            }
        }
        if ended {
            self.end_race();
        }
    }

    fn end_race(&mut self) {
        let Some(r) = self.race.take() else { return };
        let res = results(&r.st);
        // The humans' finishing order, for the next grid.
        self.last_order = res
            .iter()
            .filter_map(|row| row.human.map(|h| r.slots[h]))
            .collect();
        // Points, humans and AI alike.
        let before: Vec<(Option<Slot>, String)> = self
            .points
            .iter()
            .map(|p| (p.slot, p.name.clone()))
            .collect();
        for (place, row) in res.iter().enumerate() {
            let pts = POINTS.get(place).copied().unwrap_or(0);
            let (slot, name) = match row.human {
                Some(h) => {
                    let slot = r.slots[h];
                    let name = self
                        .players
                        .iter()
                        .find(|p| p.info.slot == slot)
                        .map_or_else(|| format!("P{}", slot + 1), |p| p.info.name.clone());
                    (Some(slot), name)
                }
                None => (None, row.name.to_string()),
            };
            let finished = !row.estimated;
            let entry = self.points.iter_mut().find(|p| {
                if slot.is_some() {
                    p.slot == slot
                } else {
                    p.slot.is_none() && p.name == name
                }
            });
            let add = if finished { pts } else { 0 };
            match entry {
                Some(e) => {
                    e.points += add;
                    e.name = name;
                }
                None => self.points.push(PointsRow {
                    slot,
                    name,
                    points: add,
                    moved: 0,
                }),
            }
        }
        // Sort (stable) by points and note who moved.
        self.points.sort_by_key(|a| std::cmp::Reverse(a.points));
        for (k, p) in self.points.iter_mut().enumerate() {
            let was = before.iter().position(|(s, n)| {
                if p.slot.is_some() {
                    *s == p.slot
                } else {
                    s.is_none() && *n == p.name
                }
            });
            p.moved = was.map_or(0, |w| (w as i64 - k as i64).clamp(-127, 127) as i8);
        }
        self.raced += 1;
        self.events
            .push(HostEvent::RaceEnded { race: r.start.race });
        // Players who left during the race and didn't come back are gone.
        self.players.retain(|p| p.peer.is_some());
        self.broadcast(Channel::Reliable, &Msg::End);
        self.send_lobby();
    }

    /// Ends the race now (the host's Abort), with no points.
    pub fn abort(&mut self) {
        if self.race.take().is_some() {
            self.broadcast(Channel::Reliable, &Msg::End);
            self.send_lobby();
        }
    }

    fn send_lobby(&mut self) {
        let m = Msg::Lobby {
            settings: self.settings.clone(),
            players: self.players(),
            points: self.points.clone(),
            raced: self.raced,
            racing: self.race.is_some(),
        };
        self.broadcast(Channel::Reliable, &m);
    }

    fn send(&mut self, peer: PeerId, ch: Channel, m: &Msg) {
        self.net.send(peer, ch, &m.encode());
    }

    fn broadcast(&mut self, ch: Channel, m: &Msg) {
        let b = m.encode();
        let peers: Vec<PeerId> = self.players.iter().filter_map(|p| p.peer).collect();
        for p in peers {
            self.net.send(p, ch, &b);
        }
    }
}

/// The simulation's setup for a race (shared with the clients, which build
/// the same state from the same message).
pub fn multi_opts(s: &RaceStart) -> MultiOpts {
    MultiOpts {
        seed: s.seed,
        humans: s
            .humans
            .iter()
            .map(|p| Human {
                car: car_id(&p.car),
                color: Some(p.color),
            })
            .collect(),
        grid: s.grid.iter().map(|&g| g as usize).collect(),
        field: Some(s.settings.ai.field()),
        rubber_band: s.settings.rubber_band,
        ghost: s.settings.ghost,
    }
}

/// Whether `id` is one of the game's levels (`level_by_id` falls back to
/// the first for anything else, which a lobby must not do quietly).
pub fn known_level(id: &str) -> bool {
    mp_levels::levels().iter().any(|l| l.id == id)
}

/// A name as the lobby shows it: trimmed, at most 16 characters, never
/// empty, no control characters.
pub fn clean_name(s: &str) -> String {
    let t: String = s
        .chars()
        .filter(|c| !c.is_control())
        .take(16)
        .collect::<String>()
        .trim()
        .to_string();
    if t.is_empty() { "Driver".into() } else { t }
}
