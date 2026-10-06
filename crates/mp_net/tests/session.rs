//! The host and clients together on the in-process network (WP 10.3, 10.4):
//! the lobby, races over good and bad links, drop-outs and rejoins, the
//! points table. Time is virtual, so every run is the same for a seed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mp_net::client::{Client, ClientEvent};
use mp_net::host::{Host, HostEvent, Levels, TICK_MS};
use mp_net::proto::{AiFill, GridRule, Settings, VERSION};
use mp_net::transport::{Conditions, SimEnd, SimNet, Transport};
use mp_sim::autopilot::autopilot;
use mp_sim::input::{Input, InputFrame, RESET};
use mp_sim::race::{LevelRuntime, SimState, hash};

fn levels() -> Levels {
    let cache: RefCell<HashMap<String, Rc<LevelRuntime>>> = RefCell::default();
    Rc::new(move |id: &str| {
        if id == "seaside" {
            return None; // needs the survey data; not used here
        }
        let mut c = cache.borrow_mut();
        if let Some(lr) = c.get(id) {
            return Some(lr.clone());
        }
        let lr = Rc::new(LevelRuntime::new(mp_levels::level_by_id(id)).ok()?);
        c.insert(id.into(), lr.clone());
        Some(lr)
    })
}

/// The autopilot, pressing reset when stuck, as a player would.
fn driver(lr: &LevelRuntime) -> impl FnMut(&SimState, usize) -> InputFrame + '_ {
    move |st: &SimState, me: usize| {
        let mut inp = Input::default();
        autopilot(&mut inp, &st.players[me].v, &lr.track);
        let mut f = InputFrame::quantise(&inp);
        if st.players[me].rules.stuck.unwrap_or(0.0) > 2.0 {
            f.flags |= RESET;
        }
        f
    }
}

struct World {
    net: SimNet,
    host: Host<SimEnd>,
    clients: Vec<Client<SimEnd>>,
    now: f64,
    levels: Levels,
}

impl World {
    fn new(seed: u32, links: &[Conditions]) -> World {
        let net = SimNet::new(seed);
        let lv = levels();
        let host = Host::new(net.host(), lv.clone(), seed);
        let clients = links
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                Client::new(
                    net.connect(c),
                    lv.clone(),
                    &format!("Driver {i}"),
                    "sports",
                    0x111111 * i as u32,
                )
            })
            .collect();
        World {
            net,
            host,
            clients,
            now: 0.0,
            levels: lv,
        }
    }

    /// One frame of everyone's loop: `dt` ms of time, the network, the host,
    /// then each client (driving on the autopilot).
    fn frame(&mut self, dt: f64) {
        self.now += dt;
        self.net.advance_to(self.now);
        self.host.update(self.now);
        let now = self.now;
        for c in &mut self.clients {
            let lr = c.race.as_ref().map(|r| r.lr.clone());
            match lr {
                Some(lr) => c.update(now, driver(&lr)),
                None => c.update(now, |_, _| InputFrame::default()),
            }
        }
    }

    /// Frames of varying length (60 Hz with jitter) for `ms`.
    fn run(&mut self, ms: f64) {
        let end = self.now + ms;
        let mut k = 0u32;
        while self.now < end {
            let dt = 1000.0 / 60.0 + [0.0, 2.1, -1.7, 4.0, -3.2][k as usize % 5];
            self.frame(dt);
            k += 1;
        }
    }

    fn start(&mut self, settings: Settings) {
        self.host.set_settings(settings);
        self.run(300.0);
        assert!(self.host.start(self.now), "the race starts");
        self.run(100.0);
    }
}

fn settings(level: &str) -> Settings {
    Settings {
        level: level.into(),
        ai: AiFill::To6,
        rubber_band: true,
        ghost: false,
        grid: GridRule::Reverse,
        races: 0,
    }
}

#[test]
fn players_join_the_lobby_and_see_each_other() {
    let mut w = World::new(1, &[Conditions::LAN; 3]);
    w.run(500.0);
    let ps = w.host.players();
    assert_eq!(ps.len(), 3);
    // Slots go in the order the hellos arrive.
    let mut slots: Vec<u8> = w.clients.iter().map(|c| c.slot.unwrap()).collect();
    for c in &w.clients {
        assert_eq!(c.lobby.players.len(), 3);
        assert!(c.events.contains(&ClientEvent::Welcome(c.slot.unwrap())));
    }
    slots.sort_unstable();
    assert_eq!(slots, vec![0, 1, 2]);
    w.clients[1].set_me("Kit", "rally", 0xee5a12);
    w.clients[1].ready(true);
    w.run(200.0);
    let kit = w.clients[1].slot.unwrap();
    let p = w.clients[0]
        .lobby
        .players
        .iter()
        .find(|p| p.slot == kit)
        .unwrap();
    assert_eq!(
        (p.name.as_str(), p.car.as_str(), p.ready),
        ("Kit", "rally", true)
    );
}

#[test]
fn the_first_player_leads_the_lobby_and_starts_the_race() {
    let mut w = World::new(9, &[Conditions::LAN; 3]);
    w.run(500.0);
    let leader = (0..3).find(|&i| w.clients[i].is_leader()).unwrap();
    let other = (leader + 1) % 3;
    assert_eq!(w.clients.iter().filter(|c| c.is_leader()).count(), 1);
    // Only the leader's word counts.
    w.clients[other].go(true);
    w.run(300.0);
    assert!(!w.host.racing());
    let mut s = settings("desert");
    s.ai = AiFill::None;
    w.clients[leader].configure(s.clone());
    w.run(300.0);
    assert_eq!(w.clients[other].lobby.settings.as_ref(), Some(&s));
    w.clients[leader].go(true);
    w.run(2500.0);
    assert!(w.host.racing());
    assert!(w.clients.iter().all(|c| c.race.is_some()));
    assert!(w.host.state().unwrap().rivals.is_empty());
    w.clients[leader].go(false);
    w.run(300.0);
    assert!(!w.host.racing());
    assert!(w.clients.iter().all(|c| c.race.is_none()));
    // An unknown level is refused.
    let mut bad = s.clone();
    bad.level = "moon".into();
    w.clients[leader].configure(bad);
    w.run(300.0);
    assert_eq!(w.host.settings.level, "desert");
}

#[test]
fn a_wrong_version_is_turned_away_politely() {
    let net = SimNet::new(1);
    let mut host = Host::new(net.host(), levels(), 1);
    let mut end = net.connect(Conditions::PERFECT);
    let hello = mp_net::proto::Msg::Hello {
        version: VERSION + 1,
        name: "old".into(),
        car: "sports".into(),
        color: 0,
    };
    end.send(0, mp_net::transport::Channel::Reliable, &hello.encode());
    host.update(1.0);
    let mut ev = Vec::new();
    end.poll(&mut ev);
    assert!(
        ev.iter()
            .any(|e| matches!(e, mp_net::transport::NetEvent::Message(_, b)
        if matches!(mp_net::proto::Msg::decode(b), Ok(mp_net::proto::Msg::Reject { .. }))))
    );
    assert!(host.players().is_empty());
}

/// Every client agrees with the host on every hash it checks, however bad
/// its link, and the bad links roll back.
fn race_and_check(seed: u32, links: &[Conditions], level: &str, secs: f64) -> World {
    let mut w = World::new(seed, links);
    w.run(500.0);
    w.start(settings(level));
    w.run(secs * 1000.0);
    let tick = w.host.tick();
    assert!(tick as f64 > secs * 100.0, "the host ran: tick {tick}");
    for (i, c) in w.clients.iter().enumerate() {
        let s = c.stats;
        assert!(
            s.hashes_checked >= (secs * 3.0) as u32,
            "client {i} checked {}",
            s.hashes_checked
        );
        assert_eq!((s.desyncs, s.unrepaired), (0, 0), "client {i}: {s:?}");
        let r = c.race.as_ref().expect("still racing");
        assert!(r.local() >= tick.saturating_sub(5), "client {i} keeps up");
        assert!(r.local() <= r.confirmed() + mp_net::client::MAX_AHEAD);
    }
    w
}

#[test]
fn a_lan_race_stays_in_sync() {
    let w = race_and_check(2, &[Conditions::LAN; 4], "coast", 20.0);
    for c in &w.clients {
        // A few ms of link, plus up to a frame each way before it's read.
        assert!(c.stats.rtt < 50.0, "{}", c.stats.rtt);
    }
}

#[test]
fn a_bad_network_rolls_back_and_still_agrees() {
    let bad = Conditions {
        latency: 60.0,
        jitter: 40.0,
        loss: 0.1,
    };
    let w = race_and_check(3, &[Conditions::LAN, bad, bad], "sierra", 20.0);
    let rb: u32 = w.clients.iter().map(|c| c.stats.rollbacks).sum();
    assert!(rb > 0, "guesses were wrong now and then");
}

/// The confirmed state a client computes is the host's, bit for bit.
#[test]
fn clients_reach_the_hosts_exact_state() {
    let mut w = race_and_check(4, &[Conditions::LAN; 2], "streets", 10.0);
    // Freeze the clients' inputs (the host repeats the last), let
    // everything arrive, then compare a confirmed tick on both sides.
    for _ in 0..30 {
        w.now += TICK_MS;
        w.net.advance_to(w.now + 1000.0);
        w.host.update(w.now);
    }
    w.net.advance_to(w.now + 1000.0);
    let host_hash = hash(w.host.state().unwrap());
    let host_tick = w.host.tick();
    let (log, n) = w.host.input_log().unwrap();
    let log = log.to_vec();
    // Re-simulate the host's log from the start, as a client would.
    let start = w.clients[0].race.as_ref().unwrap().start.clone();
    let lr = (w.levels)(&start.settings.level).unwrap();
    let mut st = SimState::new_multi(&lr, &mp_net::host::multi_opts(&start));
    let mut ev = Vec::new();
    for t in 0..host_tick as usize {
        mp_sim::race::step(&lr, &mut st, &log[t * n..t * n + n], &mut ev);
    }
    assert_eq!(hash(&st), host_hash);
}

#[test]
fn a_dropped_player_is_driven_by_the_ai_and_can_come_back() {
    let mut w = World::new(5, &[Conditions::LAN; 3]);
    w.run(500.0);
    w.start(settings("coast"));
    w.run(8000.0);
    // Player 2's link dies.
    let slot = w.clients[2].slot.unwrap();
    w.net.cut(&w.clients[2].net);
    w.run(4000.0);
    assert!(w.host.events.contains(&HostEvent::Left(slot)));
    let st = w.host.state().unwrap();
    let me = w.clients[2].race.as_ref().map_or(slot as usize, |r| r.me);
    let s_then = st.players[me].v.s;
    w.run(4000.0);
    assert!(
        w.host.state().unwrap().players[me].v.s > s_then + 50.0,
        "the autopilot drives the dropped car on"
    );
    // They come back under the same name: same slot, the race replayed.
    let end = w.net.connect(Conditions::LAN);
    let mut c = Client::new(end, w.levels.clone(), "Driver 2", "sports", 0x222222);
    let now = w.now;
    c.update(now, |_, _| InputFrame::default());
    w.clients[2] = c;
    w.run(6000.0);
    let c = &w.clients[2];
    assert_eq!(c.slot, Some(slot));
    let r = c.race.as_ref().expect("back in the race");
    assert!(
        r.local() + 10 >= w.host.tick(),
        "caught up: {} vs {}",
        r.local(),
        w.host.tick()
    );
    assert_eq!(c.stats.desyncs, 0);
}

/// Long: a whole race with eight players on mixed links, to the results and
/// the points. `cargo test -p mp_net --release -- --ignored`.
#[test]
#[ignore]
fn eight_players_race_to_the_finish_and_score() {
    let mut links = vec![Conditions::LAN; 4];
    links.extend(
        [Conditions {
            latency: 40.0,
            jitter: 30.0,
            loss: 0.05,
        }; 4],
    );
    let mut w = World::new(6, &links);
    w.run(500.0);
    w.start(settings("desert"));
    let mut t = 0.0;
    while w.host.racing() && t < 900_000.0 {
        w.run(5000.0);
        t += 5000.0;
    }
    assert!(!w.host.racing(), "the race ended");
    assert!(
        w.host
            .events
            .iter()
            .any(|e| matches!(e, HostEvent::RaceEnded { .. }))
    );
    let pts = w.host.points();
    assert_eq!(pts.len(), 8, "eight humans, no AI with a full field");
    assert!(pts.iter().map(|p| p.points).sum::<u32>() > 0);
    for c in &w.clients {
        assert_eq!(c.stats.desyncs, 0);
        assert!(c.events.contains(&ClientEvent::RaceEnded));
        assert_eq!(c.lobby.points, pts);
    }
}
