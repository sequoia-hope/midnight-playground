//! The host's session rules, driven message by message over raw
//! [`SimEnd`]s (no client prediction): joining and the full lobby, names,
//! the leader and its handoff, settings, the loading handshake, input
//! handling, hashes, races to the finish and what follows them (points,
//! grid rules, AI fill), aborts and rejoins (MULTIPLAYER 2, 4).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use mp_net::host::{
    BEGIN_LEAD_MS, HASH_EVERY, Host, HostEvent, LOAD_WAIT_MS, Levels, MAX_CATCH_UP, POINTS,
    TICK_MS, clean_name, known_level, multi_opts,
};
use mp_net::proto::{AiFill, GridRule, MAX_PLAYERS, Msg, RaceStart, Settings, VERSION};
use mp_net::transport::{Channel, Conditions, NetEvent, SimEnd, SimNet, Transport};
use mp_sim::autopilot::autopilot;
use mp_sim::input::{AUTOPILOT, AWAY, Input, InputFrame, NITRO, RESET};
use mp_sim::race::{LevelRuntime, SimEvent, SimState, hash, results, step};

thread_local! {
    /// Built levels, shared by every bench on the test's thread.
    static BUILT: RefCell<HashMap<String, Arc<LevelRuntime>>> = RefCell::default();
}

fn levels() -> Levels {
    Rc::new(|id: &str| {
        if !known_level(id) || id == "seaside" {
            return None; // Seaside needs the survey data
        }
        if let Some(lr) = BUILT.with_borrow(|c| c.get(id).cloned()) {
            return Some(lr);
        }
        let lr = Arc::new(LevelRuntime::new(mp_levels::level_by_id(id)).ok()?);
        BUILT.with_borrow_mut(|c| c.insert(id.into(), lr.clone()));
        Some(lr)
    })
}

/// A host and some hand-driven ends on a perfect network (messages arrive
/// at once), with every message each end has received.
struct Bench {
    net: SimNet,
    host: Host<SimEnd>,
    levels: Levels,
    ends: Vec<SimEnd>,
    got: Vec<Vec<Msg>>,
    closed: Vec<bool>,
    now: f64,
}

impl Bench {
    fn new(seed: u32) -> Bench {
        let net = SimNet::new(seed);
        let lv = levels();
        let host = Host::new(net.host(), lv.clone(), seed);
        Bench {
            net,
            host,
            levels: lv,
            ends: Vec::new(),
            got: Vec::new(),
            closed: Vec::new(),
            now: 0.0,
        }
    }

    /// Connects an end and says hello; returns its index.
    fn join(&mut self, name: &str) -> usize {
        let e = self.net.connect(Conditions::PERFECT);
        self.ends.push(e);
        self.got.push(Vec::new());
        self.closed.push(false);
        let i = self.ends.len() - 1;
        self.send(
            i,
            &Msg::Hello {
                version: VERSION,
                name: name.into(),
                car: "rally".into(),
                color: 0x123456,
            },
        );
        self.pump(1.0);
        i
    }

    fn send(&mut self, i: usize, m: &Msg) {
        self.ends[i].send(0, Channel::Reliable, &m.encode());
    }

    /// `dt` ms on: the host updates, every end reads its messages.
    fn pump(&mut self, dt: f64) {
        self.now += dt;
        self.net.advance_to(self.now);
        self.host.update(self.now);
        for (i, e) in self.ends.iter_mut().enumerate() {
            let mut ev = Vec::new();
            e.poll(&mut ev);
            for x in ev {
                match x {
                    NetEvent::Message(_, b) => self.got[i].push(Msg::decode(&b).unwrap()),
                    NetEvent::Disconnected(_) => self.closed[i] = true,
                    NetEvent::Connected(_) => {}
                }
            }
        }
    }

    /// Pumps until the host has stepped to `tick` (at most one update's
    /// catch-up at a time).
    fn run_to_tick(&mut self, tick: u32) {
        while self.host.racing() && self.host.tick() < tick {
            let dt = ((tick - self.host.tick()).min(MAX_CATCH_UP) as f64) * TICK_MS;
            self.pump(dt);
        }
    }

    fn cut(&mut self, i: usize) {
        self.net.cut(&self.ends[i]);
        self.pump(1.0);
    }

    fn slot(&self, i: usize) -> Option<u8> {
        self.got[i].iter().find_map(|m| match m {
            Msg::Welcome { slot } => Some(*slot),
            _ => None,
        })
    }

    fn last_lobby(&self, i: usize) -> Msg {
        self.got[i]
            .iter()
            .rev()
            .find(|m| matches!(m, Msg::Lobby { .. }))
            .cloned()
            .expect("a lobby")
    }

    fn starts(&self, i: usize) -> Vec<RaceStart> {
        self.got[i]
            .iter()
            .filter_map(|m| match m {
                Msg::Start(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    fn begins(&self, i: usize) -> Vec<f64> {
        self.got[i]
            .iter()
            .filter_map(|m| match m {
                Msg::Begin { at } => Some(*at),
                _ => None,
            })
            .collect()
    }

    fn count(&self, i: usize, f: impl Fn(&Msg) -> bool) -> usize {
        self.got[i].iter().filter(|m| f(m)).count()
    }

    fn rejected(&self, i: usize) -> bool {
        self.got[i].iter().any(|m| matches!(m, Msg::Reject { .. }))
    }

    /// The inputs end `i` has been relayed since the last Start, checked to
    /// be in order with no gaps (overlaps allowed).
    fn relayed(&self, i: usize) -> (usize, Vec<InputFrame>) {
        let from = self.got[i]
            .iter()
            .rposition(|m| matches!(m, Msg::Start(_)))
            .expect("a start");
        let mut n = 0;
        let mut log: Vec<InputFrame> = Vec::new();
        for m in &self.got[i][from..] {
            if let Msg::Inputs {
                first,
                humans,
                frames,
            } = m
            {
                n = *humans as usize;
                let have = (log.len() / n) as u32;
                assert!(*first <= have + 1, "a gap: {first} after {have}");
                let skip = (have + 1 - first) as usize * n;
                if skip < frames.len() {
                    log.extend_from_slice(&frames[skip..]);
                }
            }
        }
        (n, log)
    }

    fn settings(&mut self, s: Settings) {
        self.host.set_settings(s);
        self.pump(1.0);
    }

    /// Starts a race now and has everyone in it load; returns the start.
    fn start_loaded(&mut self) -> RaceStart {
        assert!(self.host.start(self.now), "the race starts");
        self.pump(1.0);
        for i in 0..self.ends.len() {
            if !self.closed[i] {
                self.send(i, &Msg::Loaded);
            }
        }
        self.pump(1.0);
        self.pump(BEGIN_LEAD_MS);
        self.last_start()
    }

    fn last_start(&self) -> RaceStart {
        (0..self.ends.len())
            .filter_map(|i| self.starts(i).last().cloned())
            .max_by_key(|s| s.race)
            .expect("a start")
    }

    /// The end whose player has this slot (the latest such end).
    fn end_of(&self, slot: u8) -> Option<usize> {
        (0..self.ends.len())
            .rev()
            .find(|&i| !self.closed[i] && self.slot(i) == Some(slot))
    }

    /// Runs the race with each connected human on the autopilot (pressing
    /// reset when stuck), `k` ticks per update, until it ends.
    fn drive_to_finish(&mut self, k: u32) {
        let start = self.last_start();
        let lr = (self.levels)(&start.settings.level).unwrap();
        let ends: Vec<Option<usize>> = start.humans.iter().map(|p| self.end_of(p.slot)).collect();
        let mut guard = 0;
        while self.host.racing() {
            let st = self.host.state().unwrap();
            let tick = st.tick;
            let mut out = Vec::new();
            for (h, e) in ends.iter().enumerate() {
                let Some(e) = *e else { continue };
                let mut inp = Input::default();
                autopilot(&mut inp, &st.players[h].v, &lr.track);
                let mut f = InputFrame::quantise(&inp);
                if st.players[h].rules.stuck.unwrap_or(0.0) > 2.0 {
                    f.flags |= RESET;
                }
                out.push((e, f));
            }
            for (e, f) in out {
                if !self.closed[e] {
                    let m = Msg::Input {
                        first: tick + 1,
                        frames: vec![f; k as usize],
                    };
                    self.ends[e].send(0, Channel::Unreliable, &m.encode());
                }
            }
            self.pump(k as f64 * TICK_MS);
            guard += 1;
            assert!(guard < 200_000, "the race ends");
        }
    }
}

/// The race as a client would rebuild it: the start, then the relayed
/// inputs, up to the tick the results came out.
fn replay(levels: &Levels, start: &RaceStart, n: usize, log: &[InputFrame]) -> SimState {
    let lr = levels(&start.settings.level).unwrap();
    let mut st = SimState::new_multi(&lr, &multi_opts(start));
    let mut ev = Vec::new();
    for t in log.chunks(n) {
        ev.clear();
        step(&lr, &mut st, t, &mut ev);
        if ev.contains(&SimEvent::Results) {
            break;
        }
    }
    st
}

fn settings(level: &str, ai: AiFill, grid: GridRule) -> Settings {
    Settings {
        level: level.into(),
        ai,
        rubber_band: true,
        ghost: false,
        grid,
        races: 0,
    }
}

fn frame(steer: i16, throttle: u8, flags: u8) -> InputFrame {
    InputFrame {
        steer,
        throttle,
        brake: 0,
        flags,
    }
}

// ── The lobby ────────────────────────────────────────────────────

#[test]
fn a_full_lobby_turns_the_next_player_away_until_someone_leaves() {
    let mut b = Bench::new(1);
    for k in 0..MAX_PLAYERS {
        let i = b.join(&format!("P{k}"));
        assert_eq!(b.slot(i), Some(k as u8));
    }
    let extra = b.join("Late");
    assert!(b.rejected(extra));
    assert!(b.closed[extra], "the host hangs up");
    assert_eq!(b.slot(extra), None);
    assert_eq!(b.host.players().len(), MAX_PLAYERS);

    // One leaves: the next newcomer gets the freed slot.
    b.send(3, &Msg::Leave);
    b.pump(1.0);
    assert!(b.closed[3]);
    assert!(b.host.events.contains(&HostEvent::Left(3)));
    let again = b.join("Later");
    assert_eq!(b.slot(again), Some(3));
    assert!(!b.rejected(again));
    assert_eq!(b.host.players().len(), MAX_PLAYERS);
    assert_eq!(b.host.players()[3].name, "Later");
}

#[test]
fn a_full_lobby_still_takes_back_a_player_who_dropped() {
    let mut b = Bench::new(2);
    for k in 0..MAX_PLAYERS {
        b.join(&format!("P{k}"));
    }
    b.cut(5);
    // Back under the same name: their own slot.
    let back = b.join("P5");
    assert_eq!(b.slot(back), Some(5));
    assert!(!b.rejected(back));
}

#[test]
fn a_dropped_player_comes_back_to_their_slot_and_loses_their_ready() {
    let mut b = Bench::new(3);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.join("Max");
    b.send(k, &Msg::Ready(true));
    b.pump(1.0);
    assert!(b.host.players()[1].ready);
    b.cut(k);
    let p = &b.host.players()[1];
    assert_eq!((p.connected, p.ready), (false, false));
    assert!(b.host.events.contains(&HostEvent::Left(1)));
    // The others are told.
    let Msg::Lobby { players, .. } = b.last_lobby(a) else {
        unreachable!()
    };
    assert!(!players[1].connected);
    let back = b.join("Kit");
    assert_eq!(b.slot(back), Some(1));
    assert_eq!(b.host.players().len(), 3);
    assert!(b.host.players()[1].connected);
}

#[test]
fn names_cars_and_colours_are_cleaned_on_hello_and_set_me() {
    let mut b = Bench::new(4);
    let a = b.join("\tAnn\u{7}\n");
    let c = b.join(" \n\t ");
    assert_eq!(b.host.players()[0].name, "Ann");
    assert_eq!(b.host.players()[1].name, "Driver");
    assert_eq!(b.host.players()[0].car, "rally");
    b.send(
        a,
        &Msg::SetMe {
            name: "A very long name indeed, far too long".into(),
            car: "tank".into(),
            color: 0xabcdef,
        },
    );
    b.send(
        c,
        &Msg::SetMe {
            name: "Kit".into(),
            car: "electric".into(),
            color: 7,
        },
    );
    b.pump(1.0);
    let ps = b.host.players();
    assert_eq!(ps[0].name, "A very long name");
    assert_eq!(ps[0].car, "sports", "an unknown car is the default");
    assert_eq!(ps[0].color, 0xabcdef);
    assert_eq!(
        (ps[1].name.as_str(), ps[1].car.as_str()),
        ("Kit", "electric")
    );
}

#[test]
fn clean_name_trims_cuts_and_never_returns_empty() {
    let cases = [
        ("Marisol", "Marisol"),
        ("  Kit  ", "Kit"),
        ("a\u{0}b\u{1b}c\u{7f}", "abc"),
        ("", "Driver"),
        ("\n\r\t", "Driver"),
        ("   ", "Driver"),
        ("abcdefghijklmnopqrstuvwxyz", "abcdefghijklmnop"),
        ("éééééééééééééééééééé", "éééééééééééééééé"),
        ("🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎", "🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎🏎"),
        // Cut at 16, then trimmed: no trailing space left behind.
        ("abcdefghijklmno pq", "abcdefghijklmno"),
    ];
    for (raw, want) in cases {
        let got = clean_name(raw);
        assert_eq!(got, want, "{raw:?}");
        assert!(got.chars().count() <= 16 && !got.is_empty());
        assert!(!got.chars().any(char::is_control));
        assert_eq!(clean_name(&got), got, "cleaning is idempotent");
    }
}

#[test]
fn clean_name_trims_before_it_cuts() {
    // "trimmed, at most 16 characters": 20 spaces then "Kit" is "Kit".
    assert_eq!(clean_name(&format!("{}Kit", " ".repeat(20))), "Kit");
}

#[test]
fn out_of_turn_messages_are_ignored_and_nonsense_gets_a_peer_dropped() {
    let mut b = Bench::new(5);
    // Before Hello: SetMe and Ready do nothing, a Ping is still answered.
    let mut e = b.net.connect(Conditions::PERFECT);
    for m in [
        Msg::SetMe {
            name: "x".into(),
            car: "sports".into(),
            color: 0,
        },
        Msg::Ready(true),
        Msg::Ping { id: 4, t: 1.5 },
    ] {
        e.send(0, Channel::Reliable, &m.encode());
    }
    b.now = 250.0;
    b.host.update(b.now);
    assert!(b.host.players().is_empty());
    let mut ev = Vec::new();
    e.poll(&mut ev);
    let pong = ev.iter().find_map(|x| match x {
        NetEvent::Message(_, m) => Msg::decode(m).ok(),
        _ => None,
    });
    assert_eq!(
        pong,
        Some(Msg::Pong {
            id: 4,
            t: 1.5,
            host_ms: 250.0
        })
    );

    // A second Hello from a joined peer is ignored; host-only messages too.
    let a = b.join("Ann");
    b.send(
        a,
        &Msg::Hello {
            version: VERSION,
            name: "Other".into(),
            car: "sports".into(),
            color: 0,
        },
    );
    b.send(a, &Msg::Welcome { slot: 5 });
    b.send(a, &Msg::End);
    b.send(a, &Msg::Loaded);
    b.pump(1.0);
    assert_eq!(b.host.players().len(), 1);
    assert_eq!(b.host.players()[0].name, "Ann");
    assert!(!b.closed[a]);

    // Bytes that aren't a message: dropped.
    b.ends[a].send(0, Channel::Reliable, &[0xff, 1, 2]);
    b.pump(1.0);
    assert!(b.closed[a]);
    assert!(b.host.events.contains(&HostEvent::Left(0)));
    assert!(!b.host.players()[0].connected);
}

// ── The leader and the settings ──────────────────────────────────

#[test]
fn the_lead_passes_to_the_next_slot_when_the_leader_leaves_and_back_on_return() {
    let mut b = Bench::new(6);
    let a = b.join("Ann");
    let k = b.join("Kit");
    let m = b.join("Max");
    let before = b.host.settings.clone();
    let mut s = settings("desert", AiFill::None, GridRule::Same);
    b.send(k, &Msg::Configure(s.clone()));
    b.send(k, &Msg::Go(true));
    b.pump(1.0);
    assert_eq!(b.host.settings, before, "only the leader configures");
    assert!(!b.host.racing(), "only the leader starts");

    b.send(a, &Msg::Leave);
    b.pump(1.0);
    b.send(k, &Msg::Configure(s.clone()));
    b.pump(1.0);
    assert_eq!(b.host.settings, s, "Kit leads now");
    b.send(m, &Msg::Go(true));
    b.pump(1.0);
    assert!(!b.host.racing());
    b.send(k, &Msg::Go(true));
    b.pump(1.0);
    assert!(b.host.racing());
    b.send(m, &Msg::Go(false));
    b.pump(1.0);
    assert!(b.host.racing(), "only the leader aborts");
    b.send(k, &Msg::Go(false));
    b.pump(1.0);
    assert!(!b.host.racing());

    // Ann is back in slot 0 and leads again.
    let a2 = b.join("Ann");
    assert_eq!(b.slot(a2), Some(0));
    s.ghost = true;
    b.send(k, &Msg::Configure(s.clone()));
    b.pump(1.0);
    assert!(!b.host.settings.ghost);
    b.send(a2, &Msg::Configure(s.clone()));
    b.pump(1.0);
    assert!(b.host.settings.ghost);
}

#[test]
fn the_lead_passes_on_when_the_leader_drops_mid_race() {
    let mut b = Bench::new(7);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    b.start_loaded();
    b.run_to_tick(60);
    b.send(k, &Msg::Go(false));
    b.pump(1.0);
    assert!(b.host.racing());
    b.cut(a);
    b.send(k, &Msg::Go(false));
    b.pump(1.0);
    assert!(!b.host.racing(), "Kit leads and aborts");
    assert!(b.got[k].contains(&Msg::End));
}

#[test]
fn an_unknown_level_is_refused_but_the_rest_of_the_settings_apply() {
    let mut b = Bench::new(8);
    let a = b.join("Ann");
    let mut s = settings("moon", AiFill::To8, GridRule::Same);
    s.ghost = true;
    s.races = 4;
    b.send(a, &Msg::Configure(s));
    b.pump(1.0);
    let got = &b.host.settings;
    assert_eq!(got.level, "coast", "the level stays");
    assert_eq!(
        (got.ai, got.ghost, got.grid, got.races),
        (AiFill::To8, true, GridRule::Same, 4)
    );
    let Msg::Lobby { settings, .. } = b.last_lobby(a) else {
        unreachable!()
    };
    assert_eq!(&settings, &b.host.settings, "everyone is told");
}

#[test]
fn a_level_that_cannot_be_built_does_not_start() {
    let mut b = Bench::new(9);
    let a = b.join("Ann");
    // Seaside is a real level, but this host has no survey data for it.
    b.send(
        a,
        &Msg::Configure(settings("seaside", AiFill::To6, GridRule::Random)),
    );
    b.pump(1.0);
    assert_eq!(b.host.settings.level, "seaside");
    b.send(a, &Msg::Go(true));
    b.pump(1.0);
    assert!(!b.host.racing());
    assert!(!b.host.start(b.now));
    assert!(b.starts(a).is_empty());
    b.send(
        a,
        &Msg::Configure(settings("streets", AiFill::To6, GridRule::Random)),
    );
    b.send(a, &Msg::Go(true));
    b.pump(1.0);
    assert!(b.host.racing());
}

#[test]
fn a_race_needs_someone_to_race_and_only_one_runs_at_a_time() {
    let mut b = Bench::new(10);
    assert!(!b.host.start(0.0), "an empty lobby can't race");
    let a = b.join("Ann");
    b.cut(a);
    assert!(!b.host.start(b.now), "nor can one of only the departed");
    let k = b.join("Kit");
    assert!(b.host.start(b.now));
    assert!(!b.host.start(b.now));
    b.send(k, &Msg::Go(true));
    b.pump(1.0);
    assert_eq!(b.starts(k).len(), 1);
    assert!(b.host.events.contains(&HostEvent::RaceStarted { race: 0 }));
}

#[test]
fn ai_fill_tops_the_field_up_to_six_or_eight_never_past_eight() {
    // (fill, humans, rivals)
    let cases = [
        (AiFill::None, 1, 0),
        (AiFill::None, 4, 0),
        (AiFill::To6, 1, 5),
        (AiFill::To6, 2, 4),
        (AiFill::To6, 6, 0),
        (AiFill::To6, 7, 0),
        (AiFill::To8, 1, 7),
        (AiFill::To8, 3, 5),
        (AiFill::To8, 8, 0),
    ];
    for (ai, humans, rivals) in cases {
        let mut b = Bench::new(11);
        for k in 0..humans {
            b.join(&format!("P{k}"));
        }
        b.settings(settings("coast", ai, GridRule::Random));
        assert!(b.host.start(b.now));
        let st = b.host.state().unwrap();
        assert_eq!(st.players.len(), humans, "{ai:?} {humans}");
        assert_eq!(st.rivals.len(), rivals, "{ai:?} with {humans} humans");
    }
}

#[test]
fn a_start_resets_ready_and_tells_everyone_the_same_race() {
    let mut b = Bench::new(12);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.send(a, &Msg::Ready(true));
    b.send(k, &Msg::Ready(true));
    b.pump(1.0);
    b.settings(settings("coast", AiFill::To6, GridRule::Random));
    assert!(b.host.start(b.now));
    b.pump(1.0);
    assert!(b.host.players().iter().all(|p| !p.ready));
    let (sa, sk) = (b.starts(a), b.starts(k));
    assert_eq!(sa, sk);
    let s = &sa[0];
    assert_eq!(s.race, 0);
    assert_eq!(s.settings, b.host.settings);
    assert_eq!(
        s.humans.iter().map(|p| p.slot).collect::<Vec<_>>(),
        [0, 1],
        "humans in slot order"
    );
    let mut g = s.grid.clone();
    g.sort_unstable();
    assert_eq!(g, [0, 1], "the grid is an order of the humans");
    let Msg::Lobby { racing, .. } = b.last_lobby(a) else {
        unreachable!()
    };
    assert!(racing);
}

#[test]
fn the_random_grid_is_a_seeded_permutation_that_varies_by_seed() {
    let start_with = |seed: u32| {
        let mut b = Bench::new(seed);
        for k in 0..4 {
            b.join(&format!("P{k}"));
        }
        b.settings(settings("coast", AiFill::None, GridRule::Random));
        assert!(b.host.start(b.now));
        b.pump(1.0);
        b.last_start()
    };
    let mut seen = Vec::new();
    for seed in 0..12 {
        let s = start_with(seed);
        let mut sorted = s.grid.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, [0, 1, 2, 3]);
        assert_eq!(start_with(seed), s, "the same seed, the same race");
        if !seen.contains(&s.grid) {
            seen.push(s.grid);
        }
    }
    assert!(seen.len() > 3, "random grids differ: {seen:?}");
}

// ── Loading and the clock ────────────────────────────────────────

#[test]
fn the_race_begins_when_everyone_has_loaded() {
    let mut b = Bench::new(13);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    assert!(b.host.start(b.now));
    b.send(a, &Msg::Loaded);
    b.pump(5000.0);
    assert!(b.begins(a).is_empty(), "Kit is still loading");
    assert_eq!(b.host.tick(), 0);
    b.send(k, &Msg::Loaded);
    b.pump(1.0);
    let at = b.begins(k);
    assert_eq!(at, [b.now + BEGIN_LEAD_MS]);
    assert_eq!(b.begins(a), at);
    // Nothing is stepped before tick 0's time, then the clock rules.
    b.pump(BEGIN_LEAD_MS - 1.0);
    assert_eq!(b.host.tick(), 0);
    b.pump(1.0 + 10.5 * TICK_MS);
    assert_eq!(b.host.tick(), 10);
    // Once only.
    b.pump(100.0);
    assert_eq!(b.begins(a).len(), 1);
}

#[test]
fn a_player_who_never_loads_holds_the_race_up_for_load_wait_ms_at_most() {
    let mut b = Bench::new(14);
    let a = b.join("Ann");
    b.join("Kit");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    let t = b.now;
    assert!(b.host.start(t));
    b.send(a, &Msg::Loaded);
    b.pump(1.0);
    while b.now < t + LOAD_WAIT_MS - 1000.0 {
        b.pump(1000.0);
    }
    b.pump(t + LOAD_WAIT_MS - b.now);
    assert!(b.begins(a).is_empty(), "not before the wait is over");
    b.pump(1.0);
    assert_eq!(b.begins(a), [b.now + BEGIN_LEAD_MS]);
}

#[test]
fn a_player_who_drops_while_loading_does_not_hold_the_race_up() {
    let mut b = Bench::new(15);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    assert!(b.host.start(b.now));
    b.send(a, &Msg::Loaded);
    b.pump(100.0);
    assert!(b.begins(a).is_empty());
    b.cut(k);
    assert_eq!(b.begins(a).len(), 1);
}

#[test]
fn a_stalled_host_catches_up_max_catch_up_ticks_per_update() {
    let mut b = Bench::new(16);
    b.join("Ann");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    b.start_loaded();
    let t0 = b.now;
    b.pump(1000.0);
    assert_eq!(b.host.tick(), MAX_CATCH_UP);
    b.pump(0.0);
    assert_eq!(b.host.tick(), 2 * MAX_CATCH_UP);
    for _ in 0..10 {
        b.pump(0.0);
    }
    let due = ((b.now - t0) / TICK_MS).floor() as u32;
    assert_eq!(b.host.tick(), due, "caught up, and no further");
    b.pump(0.0);
    assert_eq!(b.host.tick(), due);
}

// ── Inputs and hashes ────────────────────────────────────────────

#[test]
fn inputs_are_used_on_time_repeated_when_late_and_relayed_in_order() {
    let mut b = Bench::new(17);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    assert!(b.host.start(b.now));
    b.pump(1.0);
    let f1 = frame(100, 200, NITRO | RESET | AUTOPILOT);
    let f3 = frame(-5, 9, RESET);
    let fk = frame(7, 255, AWAY);
    // Sent before tick 0, out of order, on the lossy channel.
    for (e, first, f) in [(a, 3, f3), (a, 1, f1), (k, 1, fk)] {
        let m = Msg::Input {
            first,
            frames: vec![f],
        };
        b.ends[e].send(0, Channel::Unreliable, &m.encode());
    }
    b.send(a, &Msg::Loaded);
    b.send(k, &Msg::Loaded);
    b.pump(1.0);
    b.pump(BEGIN_LEAD_MS);
    b.run_to_tick(4);
    let (log, n) = b.host.input_log().unwrap();
    assert_eq!(n, 2);
    let ann = |t: usize| log[(t - 1) * 2];
    let kit = |t: usize| log[(t - 1) * 2 + 1];
    // A client can't claim the autopilot; the reset edge isn't repeated.
    assert_eq!(ann(1), frame(100, 200, NITRO | RESET));
    assert_eq!(ann(2), frame(100, 200, NITRO));
    assert_eq!(ann(3), f3);
    assert_eq!(ann(4), frame(-5, 9, 0));
    assert!((1..=4).all(|t| kit(t) == fk));

    // Late inputs (already stepped) change nothing.
    let before = log.to_vec();
    let m = Msg::Input {
        first: 1,
        frames: vec![frame(1, 1, 0); 4],
    };
    b.ends[a].send(0, Channel::Unreliable, &m.encode());
    b.pump(0.0);
    assert_eq!(&b.host.input_log().unwrap().0[..8], &before[..]);

    // Absurdly early ones (more than 600 ticks ahead) are dropped; ones
    // just inside the window are kept.
    let now = b.host.tick();
    let far = frame(3333, 1, 0);
    let near = frame(2222, 2, 0);
    for (first, f) in [(now + 601, far), (now + 600, near)] {
        let m = Msg::Input {
            first,
            frames: vec![f],
        };
        b.ends[a].send(0, Channel::Unreliable, &m.encode());
    }
    b.pump(0.0);
    b.run_to_tick(now + 601);
    let (log, _) = b.host.input_log().unwrap();
    let ann = |t: u32| log[(t as usize - 1) * 2];
    assert_eq!(ann(now + 600), near);
    assert_eq!(
        ann(now + 601),
        near,
        "the far one was dropped, the near repeated"
    );

    // What everyone was relayed is exactly the host's log.
    let log = log.to_vec();
    for e in [a, k] {
        assert_eq!(b.relayed(e), (2, log.clone()));
    }
}

#[test]
fn a_dropped_human_is_put_on_the_autopilot_keeping_only_away() {
    let mut b = Bench::new(18);
    b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    b.start_loaded();
    let fk = frame(7, 255, AWAY | NITRO);
    let m = Msg::Input {
        first: 1,
        frames: vec![fk; 20],
    };
    b.ends[k].send(0, Channel::Unreliable, &m.encode());
    b.run_to_tick(10);
    b.cut(k);
    let from = b.host.tick() + 1;
    b.run_to_tick(from + 30);
    let (log, _) = b.host.input_log().unwrap();
    for t in from..=from + 30 {
        assert_eq!(
            log[(t as usize - 1) * 2 + 1],
            frame(0, 0, AUTOPILOT | AWAY),
            "tick {t}"
        );
    }
    assert_eq!(log[(10 - 1) * 2 + 1], fk);
}

#[test]
fn hashes_go_out_every_hash_every_ticks_after_their_inputs_and_match_a_replay() {
    let mut b = Bench::new(19);
    let a = b.join("Ann");
    b.join("Kit");
    b.settings(settings("sierra", AiFill::To6, GridRule::Random));
    let start = b.start_loaded();
    b.run_to_tick(HASH_EVERY * 5 + 7);
    let mut confirmed = 0u32;
    let mut hashes = Vec::new();
    for m in &b.got[a] {
        match m {
            Msg::Inputs { first, frames, .. } => confirmed = first - 1 + frames.len() as u32 / 2,
            Msg::Hash { tick, hash } => {
                assert!(*tick <= confirmed, "hash {tick} before its inputs");
                hashes.push((*tick, *hash));
            }
            _ => {}
        }
    }
    let ticks: Vec<u32> = hashes.iter().map(|h| h.0).collect();
    assert_eq!(ticks, (1..=5).map(|k| k * HASH_EVERY).collect::<Vec<_>>());
    let (n, log) = b.relayed(a);
    let lr = (b.levels)("sierra").unwrap();
    let mut st = SimState::new_multi(&lr, &multi_opts(&start));
    let mut ev = Vec::new();
    for (t, frames) in log.chunks(n).enumerate() {
        step(&lr, &mut st, frames, &mut ev);
        if let Some(h) = hashes.iter().find(|h| h.0 == t as u32 + 1) {
            assert_eq!(hash(&st), h.1, "tick {}", h.0);
        }
    }
}

// ── Joining and leaving during a race ────────────────────────────

#[test]
fn a_newcomer_waits_in_the_lobby_while_a_race_runs() {
    let mut b = Bench::new(20);
    let a = b.join("Ann");
    b.join("Kit");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    b.start_loaded();
    b.run_to_tick(30);
    let c = b.join("Cat");
    assert_eq!(b.slot(c), Some(2));
    assert!(b.starts(c).is_empty(), "no race for the newcomer");
    let Msg::Lobby { racing, .. } = b.last_lobby(c) else {
        unreachable!()
    };
    assert!(racing);
    // Their inputs and Loaded are ignored; the race stays two-handed.
    b.send(c, &Msg::Loaded);
    let m = Msg::Input {
        first: 31,
        frames: vec![frame(1, 2, 0); 8],
    };
    b.send(c, &m);
    b.run_to_tick(60);
    assert_eq!(b.host.input_log().unwrap().1, 2);
    assert!(
        b.got[c].iter().any(|m| matches!(m, Msg::Inputs { .. })),
        "they do see the race's inputs go by"
    );
    b.send(a, &Msg::Go(false));
    b.pump(1.0);
    assert!(b.host.start(b.now));
    b.pump(1.0);
    assert_eq!(b.starts(c).len(), 1);
    assert_eq!(b.last_start().humans.len(), 3);
}

#[test]
fn a_player_who_drops_mid_race_rejoins_with_the_whole_race_so_far() {
    let mut b = Bench::new(21);
    b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::To6, GridRule::Random));
    let start = b.start_loaded();
    let at = b.begins(k)[0];
    b.run_to_tick(700);
    b.cut(k);
    b.run_to_tick(800);
    let k2 = b.join("Kit");
    assert_eq!(b.slot(k2), Some(1));
    // The same race, its tick 0, and every input so far in 600-tick chunks.
    assert_eq!(b.starts(k2), std::slice::from_ref(&start));
    assert_eq!(b.begins(k2), [at]);
    let firsts: Vec<u32> = b.got[k2]
        .iter()
        .filter_map(|m| match m {
            Msg::Inputs { first, .. } => Some(*first),
            _ => None,
        })
        .collect();
    assert_eq!(&firsts[..2], &[1, 601]);
    let host_log = b.host.input_log().unwrap().0.to_vec();
    let (n, log) = b.relayed(k2);
    assert_eq!(n, 2);
    assert_eq!(log, host_log);
    // Driven by the autopilot while away, by them again once back.
    assert_eq!(log[(750 - 1) * 2 + 1].flags & AUTOPILOT, AUTOPILOT);
    let back = b.host.tick() + 1;
    let m = Msg::Input {
        first: back,
        frames: vec![frame(0, 255, 0); 24],
    };
    b.ends[k2].send(0, Channel::Unreliable, &m.encode());
    b.run_to_tick(back + 10);
    let (log, _) = b.host.input_log().unwrap();
    assert_eq!(log[(back as usize - 1) * 2 + 1], frame(0, 255, 0));
}

#[test]
fn a_player_who_drops_before_the_begin_rejoins_and_gets_it_later() {
    let mut b = Bench::new(22);
    let a = b.join("Ann");
    let k = b.join("Kit");
    let m = b.join("Max");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    assert!(b.host.start(b.now));
    b.send(a, &Msg::Loaded);
    b.pump(1.0);
    b.cut(k);
    assert!(b.begins(a).is_empty(), "Max is still loading");
    let k2 = b.join("Kit");
    assert_eq!(b.starts(k2).len(), 1);
    assert!(b.begins(k2).is_empty());
    b.send(k2, &Msg::Loaded);
    b.send(m, &Msg::Loaded);
    b.pump(1.0);
    assert_eq!(b.begins(k2).len(), 1);
    assert_eq!(b.begins(k2), b.begins(a));
}

#[test]
fn an_abort_ends_the_race_with_no_points_and_no_count() {
    let mut b = Bench::new(23);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::To6, GridRule::Reverse));
    b.start_loaded();
    b.run_to_tick(200);
    b.cut(k);
    b.send(a, &Msg::Go(false));
    b.pump(1.0);
    assert!(!b.host.racing());
    assert!(b.got[a].contains(&Msg::End));
    assert!(b.host.points().is_empty());
    assert!(
        !b.host
            .events
            .iter()
            .any(|e| matches!(e, HostEvent::RaceEnded { .. }))
    );
    let Msg::Lobby { raced, racing, .. } = b.last_lobby(a) else {
        unreachable!()
    };
    assert_eq!((raced, racing), (0, false));
    // Kit dropped during the aborted race and keeps their place for now.
    assert_eq!(b.host.players().len(), 2);
    // The next race is still race 0.
    assert!(b.host.start(b.now));
    b.pump(1.0);
    assert_eq!(b.last_start().race, 0);
}

#[test]
fn when_everyone_has_gone_the_race_ends_and_the_session_starts_over() {
    let mut b = Bench::new(31);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::To6, GridRule::Reverse));
    b.start_loaded();
    b.run_to_tick(200);
    b.cut(k);
    assert!(b.host.racing(), "Ann is still racing");
    b.send(a, &Msg::Leave);
    b.pump(1.0);
    assert!(!b.host.racing(), "no one left to race");
    assert!(b.host.points().is_empty(), "an ended race scores nothing");
    // Both keep their places in the lobby.
    assert_eq!(b.host.players().len(), 2);
    // Kit comes back to the lobby, not to a race.
    let n = b.join("Kit");
    assert_eq!(b.slot(n), Some(1));
    let Msg::Lobby {
        racing,
        raced,
        players,
        ..
    } = b.last_lobby(n)
    else {
        unreachable!()
    };
    assert_eq!((racing, raced, players.len()), (false, 0, 2));
}

#[test]
fn a_player_who_leaves_on_purpose_comes_back_as_a_newcomer() {
    let mut b = Bench::new(32);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    b.start_loaded();
    b.run_to_tick(100);
    b.send(k, &Msg::Leave);
    b.pump(1.0);
    assert!(b.closed[k]);
    assert!(b.host.racing(), "Ann races on");
    // Pressing Multiplayer again: the lobby, waiting for the next race,
    // not Kit's old car mid-race (a dropped connection would take it back).
    let k2 = b.join("Kit");
    assert_eq!(b.slot(k2), Some(2));
    assert!(b.starts(k2).is_empty(), "no race for the newcomer");
    let Msg::Lobby { racing, .. } = b.last_lobby(k2) else {
        unreachable!()
    };
    assert!(racing);
    // The leader ends the race for everyone; the next one has them both.
    b.send(a, &Msg::Go(false));
    b.pump(1.0);
    assert!(!b.host.racing());
    assert!(b.got[k2].contains(&Msg::End));
    assert!(b.host.start(b.now));
    b.pump(1.0);
    assert_eq!(b.starts(k2).len(), 1);
}

#[test]
fn the_race_defining_messages_go_to_each_player_once() {
    let mut b = Bench::new(28);
    let a = b.join("Ann");
    let k = b.join("Kit");
    b.settings(settings("coast", AiFill::None, GridRule::Random));
    b.start_loaded();
    b.run_to_tick(100);
    for e in [a, k] {
        assert_eq!(b.count(e, |m| matches!(m, Msg::Start(_))), 1);
        assert_eq!(b.count(e, |m| matches!(m, Msg::Begin { .. })), 1);
        assert_eq!(b.count(e, |m| matches!(m, Msg::Welcome { .. })), 1);
    }
}

// ── Races to the finish ──────────────────────────────────────────

/// The points each row should get from a race: by place, finishers only,
/// humans by slot and AI by name.
fn race_points(st: &SimState, start: &RaceStart) -> Vec<(Option<u8>, String, u32)> {
    results(st)
        .iter()
        .enumerate()
        .map(|(place, r)| {
            let pts = if r.estimated { 0 } else { POINTS[place] };
            match r.human {
                Some(h) => (
                    Some(start.humans[h].slot),
                    start.humans[h].name.clone(),
                    pts,
                ),
                None => (None, r.name.to_string(), pts),
            }
        })
        .collect()
}

/// The humans' finishing order (slots).
fn finish_order(st: &SimState, start: &RaceStart) -> Vec<u8> {
    results(st)
        .iter()
        .filter_map(|r| r.human.map(|h| start.humans[h].slot))
        .collect()
}

#[test]
fn a_finished_race_scores_by_place_and_sends_everyone_back_to_the_lobby() {
    let mut b = Bench::new(24);
    let a = b.join("Ann");
    let k = b.join("Kit");
    let m = b.join("Max");
    b.settings(settings("coast", AiFill::To6, GridRule::Reverse));
    let start = b.start_loaded();
    b.run_to_tick(300);
    b.cut(m); // Max drops out; the autopilot finishes for him.
    b.drive_to_finish(4);
    let (n, log) = b.relayed(a);
    assert_eq!(b.relayed(k), (n, log.clone()));
    let st = replay(&b.levels, &start, n, &log);
    assert_eq!(
        st.tick as usize,
        log.len() / n,
        "the race ended on the results"
    );

    assert!(b.host.events.contains(&HostEvent::RaceEnded { race: 0 }));
    assert!(b.got[a].contains(&Msg::End) && b.got[k].contains(&Msg::End));
    let pts = b.host.points().to_vec();
    assert_eq!(pts.len(), 6, "three humans and three AI");
    let mut want = race_points(&st, &start);
    want.sort_by_key(|w| std::cmp::Reverse(w.2));
    let got: Vec<(Option<u8>, String, u32)> = pts
        .iter()
        .map(|p| (p.slot, p.name.clone(), p.points))
        .collect();
    assert_eq!(got, want);
    assert!(
        pts.iter().all(|p| p.moved == 0),
        "a first race moves no one"
    );
    assert!(pts.iter().filter(|p| p.points > 0).count() >= 3);

    // Max didn't come back, so he's gone from the lobby, not the table.
    assert_eq!(b.host.players().len(), 2);
    assert!(pts.iter().any(|p| p.slot == Some(2) && p.name == "Max"));
    let Msg::Lobby {
        raced,
        racing,
        points,
        ..
    } = b.last_lobby(a)
    else {
        unreachable!()
    };
    assert_eq!((raced, racing), (1, false));
    assert_eq!(points, pts);
}

#[test]
fn after_a_race_the_reverse_grid_starts_the_winner_last_and_newcomers_first() {
    let mut b = Bench::new(25);
    let a = b.join("Ann");
    b.join("Kit");
    b.join("Max");
    b.settings(settings("coast", AiFill::None, GridRule::Reverse));
    let start = b.start_loaded();
    b.drive_to_finish(4);
    let (n, log) = b.relayed(a);
    let order = finish_order(&replay(&b.levels, &start, n, &log), &start);
    assert_eq!(order.len(), 3);
    b.join("Zed"); // slot 3, new to the session
    assert!(b.host.start(b.now));
    b.pump(1.0);
    let next = b.last_start();
    assert_eq!(next.race, 1);
    let grid: Vec<u8> = next
        .grid
        .iter()
        .map(|&g| next.humans[g as usize].slot)
        .collect();
    let mut want = vec![3];
    want.extend(order.iter().rev());
    assert_eq!(grid, want, "finished {order:?}");
}

#[test]
fn points_add_up_over_races_note_who_moved_and_the_same_grid_follows_the_result() {
    let mut b = Bench::new(26);
    let a = b.join("Ann");
    b.join("Kit");
    b.join("Max");
    b.settings(settings("coast", AiFill::None, GridRule::Same));
    let mut totals: Vec<(Option<u8>, String, u32)> = Vec::new();
    let mut ranks: Vec<Vec<(Option<u8>, String)>> = Vec::new();
    let mut last_order = Vec::new();
    for race in 0..2 {
        let start = b.start_loaded();
        assert_eq!(start.race, race);
        if race == 1 {
            // Same: the humans in the last race's finishing order.
            let grid: Vec<u8> = start
                .grid
                .iter()
                .map(|&g| start.humans[g as usize].slot)
                .collect();
            assert_eq!(grid, last_order);
        }
        b.drive_to_finish(4);
        let (n, log) = b.relayed(a);
        let st = replay(&b.levels, &start, n, &log);
        last_order = finish_order(&st, &start);
        for (slot, name, p) in race_points(&st, &start) {
            let same = |t: &&mut (Option<u8>, String, u32)| {
                if slot.is_some() {
                    t.0 == slot
                } else {
                    t.0.is_none() && t.1 == name
                }
            };
            match totals.iter_mut().find(same) {
                Some(t) => t.2 += p,
                None => totals.push((slot, name, p)),
            }
        }
        // Stable, by points.
        totals.sort_by_key(|t| std::cmp::Reverse(t.2));
        let got: Vec<(Option<u8>, String, u32)> = b
            .host
            .points()
            .iter()
            .map(|p| (p.slot, p.name.clone(), p.points))
            .collect();
        assert_eq!(got, totals, "after race {race}");
        ranks.push(totals.iter().map(|t| (t.0, t.1.clone())).collect());
        if race == 1 {
            let moved: Vec<i8> = b.host.points().iter().map(|p| p.moved).collect();
            let want: Vec<i8> = ranks[1]
                .iter()
                .enumerate()
                .map(|(k, id)| {
                    ranks[0]
                        .iter()
                        .position(|x| x == id)
                        .map_or(0, |w| w as i8 - k as i8)
                })
                .collect();
            assert_eq!(moved, want);
        }
    }
    let Msg::Lobby { raced, .. } = b.last_lobby(a) else {
        unreachable!()
    };
    assert_eq!(raced, 2);
}

#[test]
fn a_set_number_of_races_ends_the_session() {
    let mut b = Bench::new(27);
    let a = b.join("Ann");
    let mut s = settings("coast", AiFill::None, GridRule::Reverse);
    s.races = 1;
    b.settings(s);
    b.start_loaded();
    b.drive_to_finish(4);
    assert!(!b.host.racing());
    // The session's one race is run: the final standings stand until the
    // leader starts again, which begins a new session.
    assert!(b.host.session_over());
    assert!(!b.host.points().is_empty());
    b.send(a, &Msg::Go(true));
    b.pump(1.0);
    assert!(b.host.racing());
    assert!(!b.host.session_over());
    assert!(b.host.points().is_empty(), "a new session's table");
}
