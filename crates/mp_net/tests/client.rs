//! The client session against a scripted host (raw messages on a
//! [`SimEnd`]): hello and pings, the clock and `host_tick`, inputs that
//! arrive before the race is built, the prediction window, rollback, a
//! forced desync and its rebuild, the inputs it sends, leaving, and the
//! events it reports. One test runs it against the real host for the
//! drawing fraction and the clock in a live race.

use std::sync::Arc;

use mp_net::client::{Client, ClientEvent, MAX_AHEAD, REDUNDANCY};
use mp_net::host::{HASH_EVERY, Host, Levels, TICK_MS, multi_opts};
use mp_net::proto::{AiFill, GridRule, Msg, PlayerInfo, RaceStart, Settings, VERSION};
use mp_net::transport::{Channel, Conditions, NetEvent, SimEnd, SimNet, Transport};
use mp_sim::input::InputFrame;
use mp_sim::race::{LevelRuntime, SimState, hash, step};

fn coast() -> Arc<LevelRuntime> {
    Arc::new(LevelRuntime::new(mp_levels::level_by_id("coast")).unwrap())
}

fn me(slot: u8) -> PlayerInfo {
    PlayerInfo {
        slot,
        name: "Me".into(),
        car: "sports".into(),
        color: 0xff0000,
        ready: false,
        connected: true,
    }
}

/// A one- or two-human race on Coast with no AI.
fn race_start(humans: &[u8]) -> RaceStart {
    RaceStart {
        race: 0,
        settings: Settings {
            level: "coast".into(),
            ai: AiFill::None,
            rubber_band: true,
            ghost: false,
            grid: GridRule::Random,
            races: 0,
        },
        seed: 42,
        humans: humans.iter().map(|&s| me(s)).collect(),
        grid: (0..humans.len() as u8).collect(),
    }
}

/// A scripted host: answers pings on a clock `offset` ms ahead of the
/// client's, and records what the client sends.
struct Script {
    net: SimNet,
    host: SimEnd,
    client: Client<SimEnd>,
    peer: u32,
    now: f64,
    offset: f64,
    got: Vec<(f64, Msg)>,
    disconnected: bool,
}

impl Script {
    fn new(cond: Conditions, offset: f64) -> Script {
        let net = SimNet::new(1);
        let host = net.host();
        let end = net.connect(cond);
        let peer = end.id();
        Script {
            net,
            host,
            client: Client::new(end, "Me", "sports", 0xff0000),
            peer,
            now: 0.0,
            offset,
            got: Vec::new(),
            disconnected: false,
        }
    }

    fn host_ms(&self) -> f64 {
        self.now + self.offset
    }

    fn send(&mut self, m: &Msg) {
        self.host.send(self.peer, Channel::Reliable, &m.encode());
    }

    /// `dt` ms on: the host side reads and answers pings, then the client
    /// updates with `input`.
    fn pump(&mut self, dt: f64, input: impl FnMut(&SimState, usize) -> InputFrame) {
        self.now += dt;
        self.net.advance_to(self.now);
        let mut ev = Vec::new();
        self.host.poll(&mut ev);
        for e in ev {
            match e {
                NetEvent::Message(_, b) => {
                    let m = Msg::decode(&b).unwrap();
                    if let Msg::Ping { id, t } = m {
                        let host_ms = self.host_ms();
                        self.send(&Msg::Pong { id, t, host_ms });
                    }
                    self.got.push((self.now, m));
                }
                NetEvent::Disconnected(_) => self.disconnected = true,
                NetEvent::Connected(_) => {}
            }
        }
        self.client.update(self.now, input);
    }

    fn idle(&mut self, ms: f64, dt: f64) {
        let end = self.now + ms;
        while self.now < end {
            self.pump(dt, |_, _| InputFrame::default());
        }
    }

    /// Welcomes the client to slot 0 and starts a race it then builds.
    fn enter_race(&mut self, start: RaceStart, begin_in: f64) -> f64 {
        self.send(&Msg::Welcome { slot: 0 });
        self.idle(400.0, 5.0);
        self.send(&Msg::Start(start));
        let at = self.host_ms() + begin_in;
        self.send(&Msg::Begin { at });
        while self.client.pending.is_none() {
            assert!(self.now < 10_000.0, "the start arrives");
            self.idle(1.0, 1.0);
        }
        assert!(self.client.attach(coast()));
        at
    }

    fn pings(&self) -> Vec<f64> {
        self.got
            .iter()
            .filter_map(|(t, m)| matches!(m, Msg::Ping { .. }).then_some(*t))
            .collect()
    }
}

/// The state after these ticks of these inputs, one human.
fn reference(start: &RaceStart, frames: &[InputFrame]) -> SimState {
    let lr = coast();
    let mut st = SimState::new_multi(&lr, &multi_opts(start));
    let mut ev = Vec::new();
    for f in frames {
        step(&lr, &mut st, &[*f], &mut ev);
    }
    st
}

#[test]
fn the_client_says_hello_once_and_pings_quickly_then_steadily() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    s.idle(2000.0, 10.0);
    let hellos: Vec<&Msg> = s
        .got
        .iter()
        .map(|(_, m)| m)
        .filter(|m| matches!(m, Msg::Hello { .. }))
        .collect();
    assert_eq!(
        hellos,
        [&Msg::Hello {
            version: VERSION,
            name: "Me".into(),
            car: "sports".into(),
            color: 0xff0000
        }]
    );
    assert!(matches!(s.got[0].1, Msg::Hello { .. }), "hello first");
    let p = s.pings();
    let gaps: Vec<f64> = p.windows(2).map(|w| (w[1] - w[0]).round()).collect();
    assert_eq!(&gaps[..4], &[60.0; 4], "{gaps:?}");
    assert!(gaps[5..].iter().all(|&g| g == 250.0), "{gaps:?}");
    assert!(s.client.connected());
}

#[test]
fn the_clock_finds_the_round_trip_and_the_hosts_tick() {
    let mut s = Script::new(
        Conditions {
            latency: 25.0,
            jitter: 0.0,
            loss: 0.0,
        },
        12_345.0,
    );
    s.idle(100.0, 1.0);
    assert_eq!(s.client.host_tick(s.now), None, "no race yet");
    s.idle(500.0, 1.0);
    let rtt = s.client.stats.rtt;
    assert!((50.0..=52.0).contains(&rtt), "{rtt}");
    let at = s.enter_race(race_start(&[0]), 300.0);
    for _ in 0..5 {
        let want = (s.host_ms() - at) / TICK_MS;
        let got = s.client.host_tick(s.now).unwrap();
        assert!((got - want).abs() < 0.5, "host tick {got} vs {want}");
        s.idle(97.0, 1.0);
    }
    s.send(&Msg::End);
    s.idle(60.0, 1.0);
    assert_eq!(s.client.host_tick(s.now), None, "the End forgets the begin");
}

#[test]
fn inputs_before_the_race_is_built_are_kept_in_order() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    s.send(&Msg::Welcome { slot: 0 });
    s.idle(100.0, 10.0);
    let start = race_start(&[0]);
    s.send(&Msg::Start(start.clone()));
    let f = |k: u32| InputFrame {
        steer: k as i16,
        ..InputFrame::default()
    };
    for (first, n, humans) in [
        (1, 10, 1), // 1..=10
        (5, 11, 1), // 5..=15, overlapping
        (30, 5, 1), // a gap: ignored
        (16, 4, 2), // the wrong number of humans: ignored
        (16, 2, 1), // 16..=17
        (3, 2, 1),  // all old: nothing
    ] {
        let frames: Vec<InputFrame> = (first..first + n)
            .flat_map(|t| std::iter::repeat_n(f(t), humans as usize))
            .collect();
        s.send(&Msg::Inputs {
            first,
            humans,
            frames,
        });
    }
    s.idle(10.0, 10.0);
    assert!(s.client.race.is_none() && s.client.pending.is_some());
    assert!(s.client.attach(coast()));
    let r = s.client.race.as_ref().unwrap();
    assert_eq!(r.confirmed(), 17);
    // No begin yet: nothing is stepped, so the inputs wait unused.
    assert_eq!(r.local(), 0);
    let loaded = s.got.iter().filter(|(_, m)| *m == Msg::Loaded).count();
    s.idle(10.0, 10.0);
    let now_loaded = s.got.iter().filter(|(_, m)| *m == Msg::Loaded).count();
    assert_eq!((loaded, now_loaded), (0, 1), "Loaded goes out on attach");

    // A new Start drops what was kept for the old race.
    s.send(&Msg::Start(start.clone()));
    s.send(&Msg::Inputs {
        first: 1,
        humans: 1,
        frames: vec![f(1); 3],
    });
    s.idle(10.0, 10.0);
    assert!(s.client.race.is_none());
    assert!(s.client.attach(coast()));
    assert_eq!(s.client.race.as_ref().unwrap().confirmed(), 3);
}

#[test]
fn attach_needs_a_pending_race_on_the_same_level() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    assert!(!s.client.attach(coast()), "nothing to attach");
    s.send(&Msg::Welcome { slot: 0 });
    let mut start = race_start(&[0]);
    start.settings.level = "desert".into();
    s.send(&Msg::Start(start));
    s.idle(10.0, 10.0);
    assert!(!s.client.attach(coast()), "the wrong level");
    assert!(
        s.client.pending.is_some(),
        "still waiting for the right one"
    );
    assert!(s.client.race.is_none());
    assert!(s.client.events.contains(&ClientEvent::RaceStarted));
}

#[test]
fn prediction_stops_max_ahead_ticks_past_the_hosts_word() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    s.enter_race(race_start(&[0]), 0.0);
    s.idle(2000.0, 16.0);
    let r = s.client.race.as_ref().unwrap();
    assert_eq!(r.confirmed(), 0);
    assert_eq!(r.local(), MAX_AHEAD);
    assert!(s.client.stats.stalls > 0);
    assert_eq!(s.client.stats.ahead, MAX_AHEAD as i64);
    let a = s.client.alpha();
    assert!((0.0..=1.0).contains(&a), "{a}");
}

#[test]
fn the_client_sends_its_newest_inputs_with_a_few_before_them() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    s.enter_race(race_start(&[0]), 0.0);
    // Each tick's input names the tick it is for.
    let input = |st: &SimState, _me: usize| InputFrame {
        steer: st.tick as i16 + 1,
        throttle: 7,
        ..InputFrame::default()
    };
    for _ in 0..20 {
        s.pump(16.0, input);
    }
    let sent: Vec<(u32, Vec<InputFrame>)> = s
        .got
        .iter()
        .filter_map(|(_, m)| match m {
            Msg::Input { first, frames } => Some((*first, frames.clone())),
            _ => None,
        })
        .collect();
    assert!(sent.len() > 5);
    let mut newest = 0;
    for (first, frames) in &sent {
        assert!(*first >= 1 && !frames.is_empty() && frames.len() <= REDUNDANCY);
        for (k, f) in frames.iter().enumerate() {
            assert_eq!(
                f.steer as u32,
                first + k as u32,
                "tick {}",
                first + k as u32
            );
        }
        let last = first + frames.len() as u32 - 1;
        assert!(last > newest, "each message carries a new tick");
        newest = last;
    }
    assert_eq!(newest, s.client.race.as_ref().unwrap().local());
}

#[test]
fn a_wrong_guess_rolls_back_to_the_hosts_inputs() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    let start = race_start(&[0]);
    s.enter_race(start.clone(), 0.0);
    let gas = InputFrame {
        throttle: 255,
        ..InputFrame::default()
    };
    for _ in 0..10 {
        s.pump(16.0, |_, _| gas);
    }
    let local = s.client.race.as_ref().unwrap().local();
    assert!(local >= 10);
    // The host never got them: it confirms coasting for the first 10.
    s.send(&Msg::Inputs {
        first: 1,
        humans: 1,
        frames: vec![InputFrame::default(); 10],
    });
    s.pump(0.0, |_, _| gas);
    let r = s.client.race.as_ref().unwrap();
    assert_eq!(s.client.stats.rollbacks, 1);
    assert_eq!(s.client.stats.resimulated, local);
    // Now: ten coasting ticks, then our own throttle since.
    let mut frames = vec![InputFrame::default(); 10];
    frames.resize(r.local() as usize, gas);
    assert_eq!(hash(r.state()), hash(&reference(&start, &frames)));
    assert_eq!(r.prev().tick + 1, r.state().tick);
    // Confirming what was guessed rolls nothing back.
    let n = r.local();
    s.send(&Msg::Inputs {
        first: 11,
        humans: 1,
        frames: vec![gas; n as usize - 10],
    });
    s.pump(0.0, |_, _| gas);
    assert_eq!(s.client.stats.rollbacks, 1);
}

#[test]
fn a_hash_mismatch_is_counted_and_repaired_by_a_rebuild() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    let start = race_start(&[0]);
    s.enter_race(start.clone(), 0.0);
    let ticks = 2 * HASH_EVERY;
    let frames = vec![InputFrame::default(); ticks as usize];
    let good = |t: u32| hash(&reference(&start, &frames[..t as usize]));
    s.send(&Msg::Inputs {
        first: 1,
        humans: 1,
        frames: frames.clone(),
    });
    s.send(&Msg::Hash {
        tick: HASH_EVERY,
        hash: good(HASH_EVERY) ^ 1,
    });
    s.send(&Msg::Hash {
        tick: ticks,
        hash: good(ticks),
    });
    while s.client.race.as_ref().unwrap().local() < ticks + 5 {
        s.pump(16.0, |_, _| InputFrame::default());
    }
    let st = s.client.stats;
    assert!(st.hashes_checked >= 1);
    assert_eq!(st.desyncs, 1);
    assert_eq!(st.unrepaired, 0, "the rebuild agrees with the host at 60");
    // The rebuilt race is the right one.
    let r = s.client.race.as_ref().unwrap();
    let all = vec![InputFrame::default(); r.local() as usize];
    assert_eq!(hash(r.state()), hash(&reference(&start, &all)));
}

/// The host's later hash still disagrees after the rebuild: a determinism
/// bug, which the rebuild counts.
#[test]
fn a_mismatch_the_rebuild_cannot_fix_is_counted_as_unrepaired() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    s.enter_race(race_start(&[0]), 0.0);
    s.send(&Msg::Inputs {
        first: 1,
        humans: 1,
        frames: vec![InputFrame::default(); 2 * HASH_EVERY as usize],
    });
    for k in 1..=2 {
        s.send(&Msg::Hash {
            tick: k * HASH_EVERY,
            hash: 12345,
        });
    }
    // Small steps: the first check (at 30) comes well before 60.
    while s.client.race.as_ref().unwrap().local() < 2 * HASH_EVERY + 5 {
        s.pump(8.0, |_, _| InputFrame::default());
    }
    let st = s.client.stats;
    assert_eq!((st.desyncs, st.unrepaired), (1, 1), "{st:?}");
}

#[test]
fn matching_hashes_are_checked_and_pass() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    let start = race_start(&[0]);
    s.enter_race(start.clone(), 0.0);
    let frames = vec![InputFrame::default(); 3 * HASH_EVERY as usize];
    s.send(&Msg::Inputs {
        first: 1,
        humans: 1,
        frames: frames.clone(),
    });
    for k in 1..=3 {
        let t = k * HASH_EVERY;
        s.send(&Msg::Hash {
            tick: t,
            hash: hash(&reference(&start, &frames[..t as usize])),
        });
    }
    while s.client.race.as_ref().unwrap().local() < 3 * HASH_EVERY {
        s.pump(16.0, |_, _| InputFrame::default());
    }
    s.pump(16.0, |_, _| InputFrame::default());
    assert_eq!(s.client.stats.hashes_checked, 3);
    assert_eq!(s.client.stats.desyncs, 0);
}

#[test]
fn the_client_follows_the_lobby_and_knows_the_leader() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    s.send(&Msg::Welcome { slot: 1 });
    let mut away = me(0);
    away.connected = false;
    let players = vec![away, me(1), me(2)];
    s.send(&Msg::Lobby {
        settings: Settings::default(),
        players: players.clone(),
        points: vec![],
        raced: 3,
        racing: true,
    });
    s.idle(10.0, 10.0);
    let c = &s.client;
    assert_eq!(c.slot, Some(1));
    assert_eq!(c.lobby.players, players);
    assert_eq!((c.lobby.raced, c.lobby.racing), (3, true));
    assert_eq!(c.lobby.settings, Some(Settings::default()));
    assert_eq!(c.leader(), Some(1), "the lowest connected slot");
    assert!(c.is_leader());
    assert_eq!(
        c.events,
        [ClientEvent::Welcome(1), ClientEvent::Lobby],
        "in order"
    );
    s.send(&Msg::Reject {
        reason: "the lobby is full".into(),
    });
    s.idle(10.0, 10.0);
    assert_eq!(
        s.client.events.last(),
        Some(&ClientEvent::Rejected("the lobby is full".into()))
    );
}

#[test]
fn the_end_of_a_race_returns_to_the_lobby() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    s.enter_race(race_start(&[0]), 0.0);
    s.idle(100.0, 16.0);
    assert!(s.client.race.is_some());
    s.send(&Msg::End);
    s.idle(16.0, 16.0);
    assert!(s.client.race.is_none() && s.client.pending.is_none());
    assert_eq!(s.client.events.last(), Some(&ClientEvent::RaceEnded));
    assert_eq!(s.client.alpha(), 1.0);
}

#[test]
fn leaving_tells_the_host_and_hangs_up() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    s.enter_race(race_start(&[0]), 0.0);
    s.idle(100.0, 16.0);
    s.client.leave();
    assert!(!s.client.connected());
    assert!(s.client.race.is_none());
    s.idle(500.0, 16.0);
    assert!(s.disconnected);
    assert_eq!(
        s.got.last().map(|m| &m.1),
        Some(&Msg::Leave),
        "nothing after the Leave"
    );
}

#[test]
fn a_lost_connection_ends_the_race_and_is_reported() {
    let mut s = Script::new(Conditions::PERFECT, 0.0);
    s.enter_race(race_start(&[0]), 0.0);
    s.idle(100.0, 16.0);
    s.host.close(s.peer);
    s.idle(16.0, 16.0);
    assert!(!s.client.connected());
    assert!(s.client.race.is_none());
    assert_eq!(s.client.events.last(), Some(&ClientEvent::Disconnected));
    let n = s.got.len();
    s.idle(500.0, 16.0);
    assert_eq!(s.got.len(), n, "a dead client sends nothing");
}

/// Against the real host on a jittery link: the drawing fraction stays in
/// [0, 1], the clock's tick tracks the host's, and the client stays a
/// little ahead of it.
#[test]
fn in_a_live_race_alpha_stays_in_range_and_the_clock_tracks_the_host() {
    let net = SimNet::new(3);
    let lr = coast();
    let lr2 = lr.clone();
    let levels: Levels = std::rc::Rc::new(move |id: &str| (id == "coast").then(|| lr2.clone()));
    let mut host = Host::new(net.host(), levels, 3);
    let link = Conditions {
        latency: 30.0,
        jitter: 10.0,
        loss: 0.05,
    };
    let mut c = Client::new(net.connect(link), "Me", "sports", 1);
    let mut now = 0.0;
    let mut frame = |host: &mut Host<SimEnd>, c: &mut Client<SimEnd>, dt: f64| {
        now += dt;
        net.advance_to(now);
        host.update(now);
        if c.pending.is_some() {
            c.attach(lr.clone());
        }
        c.update(now, |_, _| InputFrame {
            throttle: 200,
            ..InputFrame::default()
        });
        now
    };
    let mut t = 0.0;
    for _ in 0..30 {
        t = frame(&mut host, &mut c, 16.0);
    }
    host.set_settings(Settings {
        level: "coast".into(),
        ai: AiFill::To6,
        ..Settings::default()
    });
    assert!(host.start(t));
    let mut k = 0;
    while host.tick() < 1200 {
        let dt = [16.7, 13.1, 20.4, 16.0, 9.9][k % 5];
        k += 1;
        let now = frame(&mut host, &mut c, dt);
        let a = c.alpha();
        assert!((0.0..=1.0).contains(&a), "alpha {a}");
        if host.tick() > 100 {
            let ht = c.host_tick(now).unwrap();
            assert!(
                (ht - host.tick() as f64).abs() < 4.0,
                "{ht} vs {}",
                host.tick()
            );
            let r = c.race.as_ref().unwrap();
            assert!(r.local() >= host.tick(), "ahead of the host");
            assert!(r.local() <= r.confirmed() + MAX_AHEAD);
        }
    }
    assert_eq!(c.stats.desyncs, 0);
    assert!(c.stats.hashes_checked >= 30);
    assert!(c.events.contains(&ClientEvent::RaceStarted));
    assert!(!c.race.as_ref().unwrap().events.is_empty(), "sim events");
}
