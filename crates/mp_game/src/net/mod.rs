//! Multiplayer in the client (roadmap WP 10.7, `docs/rust-port/MULTIPLAYER.md`):
//! the connection to an `mp-host`, the lobby the screens show, and the
//! hand-over to the race.
//!
//! - [`Net`] holds the `mp_net` client. Its transport (a browser WebSocket,
//!   or one on a thread natively) isn't `Send`, so it is a non-send resource
//!   and the systems that touch it run on the main thread.
//! - [`NetView`] is what the screens read: the lobby, who leads, whether a
//!   race is waiting to be loaded. It is refreshed every frame.
//! - [`NetCmds`] is what the screens ask for (join, ready, settings, go).
//!   They are carried out before the next frame.
//!
//! While a race runs, the race's own frame (`play::step`) drives the client
//! through `Session::advance_online`; between races [`frame`] keeps the
//! lobby alive.

use bevy::prelude::*;

use mp_net::client::{Client, ClientEvent, LobbyView, NetStats};
use mp_net::proto::{PlayerInfo, Settings, Slot};
use mp_net::rtc::{RtcNet, RtcOptions, StatusHandle};
use mp_net::signal::{JoinLink, Role};
use mp_net::transport::Transport;

pub mod link;
pub mod tab;
#[cfg(not(target_arch = "wasm32"))]
mod ws_native;
#[cfg(target_arch = "wasm32")]
mod ws_web;

/// The client, over whichever transport this platform has.
pub type NetClient = Client<Box<dyn Transport>>;

/// The connection (non-send: see the module docs).
#[derive(Default)]
pub struct Net {
    pub client: Option<NetClient>,
    /// This tab hosts the session (MULTIPLAYER 8.1): the authority, which
    /// `client` (this tab's own player) joins through the loopback.
    pub tab: Option<tab::TabHost>,
    /// A guest's WebRTC room, for the lobby's status line.
    pub room: Option<StatusHandle>,
}

/// What the screens show of multiplayer. Only written when something
/// changed, so the lobby is rebuilt only then.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct NetView {
    /// A multiplayer session is open (joining, or joined).
    pub active: bool,
    pub connected: bool,
    /// One line for the lobby: connecting, rejected, lost, ...
    pub status: String,
    pub slot: Option<Slot>,
    pub leader: bool,
    pub lobby: LobbyView,
    /// The host started a race: its level, until the client has built it.
    pub pending_level: Option<String>,
    /// The race in hand is over on the host (back to the lobby next).
    pub ended: bool,
    /// The names, cars and colours of the race's humans, in race order
    /// (player `i` of the simulation is `humans[i]`).
    pub humans: Vec<PlayerInfo>,
    /// This tab hosts, and this is the invitation to hand out.
    pub invite: Option<String>,
}

impl NetView {
    pub fn me(&self) -> Option<&PlayerInfo> {
        let s = self.slot?;
        self.lobby.players.iter().find(|p| p.slot == s)
    }
}

/// What the screens ask of the connection.
#[derive(Clone, Debug, PartialEq)]
pub enum NetCmd {
    /// Connect to the host that served this page (web), or `url`: a
    /// host's WebSocket, or an invitation link (WebRTC). `signal` is the
    /// page's own `?signal=`.
    Join {
        url: Option<String>,
        signal: Option<String>,
        name: String,
        car: String,
        color: u32,
    },
    /// Host a game in this tab over WebRTC (MULTIPLAYER 8.1).
    Host {
        signal: Option<String>,
        name: String,
        car: String,
        color: u32,
    },
    Leave,
    SetMe {
        name: String,
        car: String,
        color: u32,
    },
    Ready(bool),
    Configure(Settings),
    Go(bool),
}

#[derive(Resource, Default)]
pub struct NetCmds(pub Vec<NetCmd>);

/// The connection's counters (round trip, rollbacks, desyncs), apart from
/// [`NetView`] because they change every frame.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct NetStatsRes(pub NetStats);

/// The host's WebSocket next to the page: the page's directory plus `ws`,
/// on ws: or wss: to match the page (every URL the client uses is relative
/// to it, SPEC 9.5). Natively, `--join ws://host:port/ws` (`?join=`).
pub fn default_url() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        ws_web::page_url()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// The signalling server for a link or for hosting, in D1124's order: the
/// link's `?signal=`, the page's, the build's, then the one an `mp-host`
/// serves beside the page (web only).
pub fn signal_for(link: Option<&str>, page_param: Option<&str>) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    let beside = ws_web::signal_beside();
    #[cfg(not(target_arch = "wasm32"))]
    let beside: Option<String> = None;
    link::signal_url(link, page_param, link::built_in_signal(), beside.as_deref())
}

/// The page the invitation points at: this one on the web (its query
/// kept), the published game natively.
fn invite_page() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        ws_web::page_href().unwrap_or_else(|| link::PAGES.to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        link::PAGES.to_string()
    }
}

/// An invitation in the page's own address, if it was opened with one.
pub fn page_link() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        ws_web::page_href().filter(|h| h.contains("#") && link::is_link(h))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// A session seed from the OS or the browser's secure randomness.
fn random_seed() -> u32 {
    let mut b = [0u8; 4];
    mp_net::rtc::random_bytes(&mut b);
    u32::from_le_bytes(b)
}

fn connect(url: &str) -> Result<Box<dyn Transport>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        ws_web::WebSocketTransport::open(url).map(|t| Box::new(t) as Box<dyn Transport>)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(Box::new(ws_native::NativeWs::open(url)))
    }
}

/// The host's clock for the session: the app's real time in ms.
pub fn now_ms(time: &Time<Real>) -> f64 {
    time.elapsed_secs_f64() * 1000.0
}

pub fn plugin(app: &mut App) {
    app.insert_non_send(Net::default())
        .init_resource::<NetView>()
        .init_resource::<NetCmds>()
        .init_resource::<NetStatsRes>()
        .add_systems(First, frame.after(bevy::time::TimeSystems));
}

/// Carries out the screens' commands, keeps the lobby going between races,
/// and refreshes [`NetView`].
pub fn frame(
    mut net: NonSendMut<Net>,
    mut view_res: ResMut<NetView>,
    mut cmds: ResMut<NetCmds>,
    mut stats: ResMut<NetStatsRes>,
    time: Res<Time<Real>>,
) {
    let now = now_ms(&time);
    let mut v = view_res.clone();
    let view = &mut v;
    for c in cmds.0.drain(..) {
        match c {
            NetCmd::Join {
                url,
                signal,
                name,
                car,
                color,
            } => {
                if let Some(code) = url.as_deref().filter(|u| link::is_link(u)) {
                    // An invitation: WebRTC to the host, through the
                    // signalling server (MULTIPLAYER 8.1, item 6).
                    let Some(jl) = JoinLink::parse(code) else {
                        continue;
                    };
                    let Some(sig) = signal_for(Some(code), signal.as_deref()) else {
                        view.status =
                            "This link names no signalling server, and this build has none".into();
                        continue;
                    };
                    let rtc = RtcNet::open(RtcOptions {
                        signal: sig,
                        link: jl,
                        role: Role::Guest,
                        ice: None,
                    });
                    net.room = Some(rtc.status_handle());
                    net.tab = None;
                    net.client = Some(Client::new(Box::new(rtc), &name, &car, color));
                    *view = NetView {
                        active: true,
                        status: "Reaching the signalling server…".into(),
                        ..NetView::default()
                    };
                    continue;
                }
                let Some(url) = url.or_else(default_url) else {
                    view.status = "No host to join: open the game from the host's address".into();
                    continue;
                };
                match connect(&url) {
                    Ok(t) => {
                        net.room = None;
                        net.tab = None;
                        net.client = Some(Client::new(t, &name, &car, color));
                        *view = NetView {
                            active: true,
                            status: format!("Connecting to {url}…"),
                            ..NetView::default()
                        };
                        let _ = &url;
                    }
                    Err(e) => view.status = format!("Can't connect: {e}"),
                }
            }
            NetCmd::Host {
                signal,
                name,
                car,
                color,
            } => {
                let Some(sig) = signal_for(None, signal.as_deref()) else {
                    view.status =
                        "No signalling server: open the game with ?signal=wss://…/".into();
                    continue;
                };
                let jl = mp_net::rtc::new_link();
                let invite = link::invite_url(&invite_page(), &sig, link::built_in_signal(), &jl);
                let rtc = RtcNet::open(RtcOptions {
                    signal: sig,
                    link: jl,
                    role: Role::Host,
                    ice: None,
                });
                let status = rtc.status_handle();
                let (host, me) = tab::TabHost::new(
                    Box::new(rtc),
                    Some(status),
                    tab::levels(),
                    random_seed(),
                    invite.clone(),
                    (&name, &car, color),
                );
                net.room = None;
                net.tab = Some(host);
                net.client = Some(me);
                *view = NetView {
                    active: true,
                    status: "Opening the room…".into(),
                    invite: Some(invite),
                    ..NetView::default()
                };
            }
            NetCmd::Leave => {
                if let Some(c) = net.client.as_mut() {
                    c.leave();
                }
                net.client = None;
                net.tab = None;
                net.room = None;
                *view = NetView::default();
            }
            NetCmd::SetMe { name, car, color } => {
                if let Some(c) = net.client.as_mut() {
                    c.set_me(&name, &car, color);
                }
            }
            NetCmd::Ready(r) => {
                if let Some(c) = net.client.as_mut() {
                    c.ready(r);
                }
            }
            NetCmd::Configure(s) => {
                if let Some(c) = net.client.as_mut() {
                    c.configure(s);
                }
            }
            NetCmd::Go(g) => {
                if let Some(c) = net.client.as_mut() {
                    c.go(g);
                }
            }
        }
    }
    // The tab's host steps on the page's clock every frame, races included
    // (its own player's client is driven like any other below).
    if let Some(t) = net.tab.as_mut() {
        t.update(now);
    }
    let room = net
        .tab
        .as_ref()
        .and_then(tab::TabHost::status)
        .map(|s| (true, s))
        .or_else(|| net.room.as_ref().map(|h| (false, h.get())));
    let Some(c) = net.client.as_mut() else {
        view_res.set_if_neq(v);
        return;
    };
    // Between races the lobby needs pings and messages; in a race the
    // race's frame drives the client.
    if c.race.is_none() {
        c.update(now, |_, _| mp_sim::input::InputFrame::default());
    }
    for e in c.events.drain(..) {
        match e {
            ClientEvent::Welcome(_) => view.status = String::new(),
            ClientEvent::Rejected(r) => view.status = format!("The host said no: {r}"),
            ClientEvent::Disconnected => {
                view.status = "Lost the host".into();
                view.connected = false;
            }
            ClientEvent::RaceStarted => view.ended = false,
            ClientEvent::RaceEnded => view.ended = true,
            ClientEvent::Lobby => {}
        }
    }
    // The room's progress, until the host has welcomed us (or failed).
    if let Some((hosting, s)) = room
        && (view.status.is_empty() || link::is_room_line(&view.status))
    {
        let failed = matches!(s, mp_net::rtc::RtcStatus::Failed(_));
        if failed || !c.connected() || hosting {
            view.status = link::status_line(hosting, &s);
        }
    }
    view.connected = c.connected();
    view.slot = c.slot;
    view.leader = c.is_leader();
    view.lobby = c.lobby.clone();
    stats.0 = c.stats;
    view.pending_level = c.pending.as_ref().map(|p| p.settings.level.clone());
    if let Some(p) = &c.pending {
        view.humans = p.humans.clone();
    } else if let Some(r) = &c.race {
        view.humans = r.start.humans.clone();
    }
    view_res.set_if_neq(v);
}

#[cfg(test)]
pub(crate) mod tests {
    //! The whole client path without graphics: a host and two game clients
    //! on the in-process network, each driving a real `Race` through
    //! `frame_online` on the autopilot, from the lobby to the results; the
    //! [`frame`] system on its own (commands, events, the view); and
    //! [`Lan`], the host and clients the other modules' online tests use.

    use std::sync::Arc;

    use mp_net::host::Host;
    use mp_net::proto::{AiFill, Settings};
    use mp_net::transport::{Conditions, SimNet};
    use mp_sim::race::LevelRuntime;

    use super::*;
    use crate::play::flow::{Race, Setup};
    use crate::play::touch::TouchControls;

    #[test]
    fn two_game_clients_race_to_the_results() {
        let net = SimNet::new(3);
        let lr = Arc::new(LevelRuntime::new(mp_levels::level_by_id("coast")).unwrap());
        let lv = lr.clone();
        let mut host = Host::new(
            net.host(),
            std::rc::Rc::new(move |id: &str| (id == "coast").then(|| lv.clone())),
            3,
        );
        let mut clients: Vec<NetClient> = (0..2)
            .map(|i| {
                Client::new(
                    Box::new(net.connect(Conditions::LAN)) as Box<dyn Transport>,
                    &format!("Driver {i}"),
                    "sports",
                    0xd81e36,
                )
            })
            .collect();
        let mut races: Vec<Option<Race>> = vec![None, None];
        let mut now = 0.0;
        let dt = 1.0 / 60.0;
        let frame = |host: &mut Host<_>,
                     clients: &mut Vec<NetClient>,
                     races: &mut Vec<Option<Race>>,
                     now: &mut f64| {
            *now += dt * 1000.0;
            net.advance_to(*now);
            host.update(*now);
            for (c, race) in clients.iter_mut().zip(races.iter_mut()) {
                if c.pending.is_some() {
                    assert!(c.attach(lr.clone()));
                    let r = c.race.as_ref().unwrap();
                    let opts = mp_sim::race::RaceOpts {
                        car: r.state().players[r.me].v.kind,
                        seed: r.start.seed,
                        pursuit: false,
                        heat: 1.0,
                    };
                    *race = Some(Race::online(
                        lr.clone(),
                        r.state().clone(),
                        r.me,
                        Setup {
                            opts,
                            autodrive: true,
                            touch: false,
                        },
                        TouchControls::default(),
                    ));
                }
                match race {
                    Some(r) if c.race.is_some() => r.frame_online(dt, c, *now),
                    _ => c.update(*now, |_, _| mp_sim::input::InputFrame::default()),
                }
            }
        };
        for _ in 0..30 {
            frame(&mut host, &mut clients, &mut races, &mut now);
        }
        let leader = (0..2).find(|&i| clients[i].is_leader()).unwrap();
        clients[leader].configure(Settings {
            ai: AiFill::None,
            ghost: true,
            ..Settings::default()
        });
        clients[leader].go(true);
        let mut done = false;
        for _ in 0..(15 * 60 * 60) {
            frame(&mut host, &mut clients, &mut races, &mut now);
            if races
                .iter()
                .all(|r| r.as_ref().is_some_and(|r| r.results.is_some()))
            {
                done = true;
                break;
            }
        }
        assert!(done, "both clients reach the results");
        let mes: Vec<usize> = races.iter().map(|r| r.as_ref().unwrap().me()).collect();
        assert_ne!(mes[0], mes[1]);
        for (c, r) in clients.iter().zip(&races) {
            let r = r.as_ref().unwrap();
            assert!(r.online_now());
            assert_eq!(c.stats.desyncs, 0, "{:?}", c.stats);
            let res = r.results.as_ref().unwrap();
            assert_eq!(res.iter().filter(|row| row.human.is_some()).count(), 2);
            assert!(res.iter().any(|row| row.human == Some(r.me())));
        }
    }

    // ── The shared harness ─────────────────────────────────────────────

    /// One rendered frame, s.
    pub(crate) const FRAME: f64 = 1.0 / 60.0;

    /// A host and game clients on the in-process network (LAN links),
    /// racing Coast Highway. The tests drive the clients themselves; the
    /// host and the clock move with [`Lan::host_frame`].
    pub(crate) struct Lan {
        pub net: SimNet,
        pub host: Host<mp_net::transport::SimEnd>,
        pub clients: Vec<NetClient>,
        pub lr: Arc<LevelRuntime>,
        pub now: f64,
    }

    impl Lan {
        pub fn new(names: &[&str]) -> Lan {
            let net = SimNet::new(7);
            let lr = Arc::new(LevelRuntime::new(mp_levels::level_by_id("coast")).unwrap());
            let lv = lr.clone();
            let host = Host::new(
                net.host(),
                std::rc::Rc::new(move |id: &str| (id == "coast").then(|| lv.clone())),
                3,
            );
            let clients = names
                .iter()
                .map(|n| {
                    Client::new(
                        Box::new(net.connect(Conditions::LAN)) as Box<dyn Transport>,
                        n,
                        "sports",
                        0xd81e36,
                    )
                })
                .collect();
            Lan {
                net,
                host,
                clients,
                lr,
                now: 0.0,
            }
        }

        /// The clock, the network and the host by one frame.
        pub fn host_frame(&mut self) {
            self.now += FRAME * 1000.0;
            self.net.advance_to(self.now);
            self.host.update(self.now);
        }

        /// One frame with every client between races.
        pub fn lobby_frame(&mut self) {
            self.host_frame();
            let now = self.now;
            for c in &mut self.clients {
                c.update(now, |_, _| mp_sim::input::InputFrame::default());
            }
        }

        /// Half a second of lobby; the leader's index.
        pub fn settle(&mut self) -> usize {
            for _ in 0..30 {
                self.lobby_frame();
            }
            (0..self.clients.len())
                .find(|&i| self.clients[i].is_leader())
                .expect("a leader")
        }

        /// The leader starts a race with `s`; every client builds it and
        /// gets the game's `Race` for it.
        pub fn start(&mut self, s: Settings, autodrive: bool) -> Vec<Race> {
            let leader = self.settle();
            self.clients[leader].configure(s);
            self.clients[leader].go(true);
            for _ in 0..120 {
                self.lobby_frame();
                if self.clients.iter().all(|c| c.pending.is_some()) {
                    break;
                }
            }
            let lr = self.lr.clone();
            self.clients
                .iter_mut()
                .map(|c| {
                    assert!(c.attach(lr.clone()), "the race is built");
                    race_of(c, &lr, autodrive)
                })
                .collect()
        }
    }

    /// The game's `Race` for the race a client has attached.
    pub(crate) fn race_of(c: &NetClient, lr: &Arc<LevelRuntime>, autodrive: bool) -> Race {
        let r = c.race.as_ref().expect("attached");
        let opts = mp_sim::race::RaceOpts {
            car: r.state().players[r.me].v.kind,
            seed: r.start.seed,
            pursuit: false,
            heat: 1.0,
        };
        Race::online(
            lr.clone(),
            r.state().clone(),
            r.me,
            Setup {
                opts,
                autodrive,
                touch: false,
            },
            TouchControls::default(),
        )
    }

    // ── The `frame` system ─────────────────────────────────────────────

    use bevy::ecs::system::RunSystemOnce;

    fn world(client: Option<NetClient>) -> World {
        let mut w = World::new();
        w.insert_non_send(Net {
            client,
            ..Net::default()
        });
        w.init_resource::<NetView>();
        w.init_resource::<NetCmds>();
        w.init_resource::<NetStatsRes>();
        w.insert_resource(Time::<Real>::default());
        w
    }

    fn run(w: &mut World, cmds: Vec<NetCmd>) -> NetView {
        w.resource_mut::<NetCmds>().0.extend(cmds);
        w.run_system_once(frame).unwrap();
        assert!(
            w.resource::<NetCmds>().0.is_empty(),
            "every command is done"
        );
        w.resource::<NetView>().clone()
    }

    fn client_of(w: &mut World) -> &mut NetClient {
        w.non_send_mut::<Net>()
            .into_inner()
            .client
            .as_mut()
            .unwrap()
    }

    /// Natively there is no page to take the host's address from: Join
    /// without `--join` says why and opens nothing.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn joining_with_no_host_address_says_so_and_opens_nothing() {
        let mut w = world(None);
        let v = run(
            &mut w,
            vec![NetCmd::Join {
                url: None,
                signal: None,
                name: "Ann".into(),
                car: "sports".into(),
                color: 0xd81e36,
            }],
        );
        assert_eq!(
            v.status,
            "No host to join: open the game from the host's address"
        );
        assert!(!v.active && !v.connected);
        assert!(w.non_send::<Net>().client.is_none());
        // The commands that need a connection do nothing without one.
        let v = run(
            &mut w,
            vec![
                NetCmd::Ready(true),
                NetCmd::Go(true),
                NetCmd::Configure(Settings::default()),
                NetCmd::SetMe {
                    name: "B".into(),
                    car: "rally".into(),
                    color: 0,
                },
            ],
        );
        assert!(!v.active && v.slot.is_none());
    }

    /// Host online: the tab hosts, its own player joins through the
    /// loopback and leads at once (no WebRTC peer needed for that), and
    /// the lobby has the invitation, naming the signalling server it was
    /// given. The status line shows the room's state (how soon an
    /// unreachable server fails is up to the WebRTC library's retries);
    /// Leave closes the host too.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn hosting_in_the_tab_leads_its_own_lobby_and_hands_out_a_link() {
        let mut w = world(None);
        let host = NetCmd::Host {
            signal: Some("wss://signal.invalid/".into()),
            name: "Ann".into(),
            car: "sports".into(),
            color: 0xd81e36,
        };
        let mut v = run(&mut w, vec![host]);
        assert!(v.active);
        let invite = v.invite.clone().expect("an invitation");
        assert!(invite.starts_with(link::PAGES), "{invite}");
        assert!(
            invite.contains("?signal=wss%3A%2F%2Fsignal.invalid%2F#join="),
            "{invite}"
        );
        assert!(link::is_link(&invite));
        assert!(w.non_send::<Net>().tab.is_some());
        for _ in 0..200 {
            v = run(&mut w, vec![]);
            if v.slot.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(v.slot, Some(0), "the tab's player holds slot 0");
        assert!(v.leader && v.connected);
        assert_eq!(v.lobby.players.len(), 1);
        assert_eq!(v.me().map(|p| p.name.as_str()), Some("Ann"));
        assert!(
            v.status.is_empty() || link::is_room_line(&v.status),
            "{}",
            v.status
        );
        let v = run(&mut w, vec![NetCmd::Leave]);
        assert!(!v.active && v.invite.is_none());
        assert!(w.non_send::<Net>().tab.is_none());
    }

    /// An invitation, natively, with no signalling server in it, in
    /// `--signal` or built in: it says so and opens nothing.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_link_with_no_signalling_server_says_so() {
        if link::built_in_signal().is_some() {
            return; // a build with MP_SIGNAL_URL always has one
        }
        let mut w = world(None);
        let jl = JoinLink::parse("AAAAAAAAAAAAAAAA.AAAAAAAAAAAAAAAAAAAAAA").unwrap();
        let v = run(
            &mut w,
            vec![NetCmd::Join {
                url: Some(format!("https://x/#{}", jl.fragment())),
                signal: None,
                name: "Ann".into(),
                car: "sports".into(),
                color: 0xd81e36,
            }],
        );
        assert!(v.status.contains("no signalling server"), "{}", v.status);
        assert!(w.non_send::<Net>().client.is_none());
    }

    /// The view follows the lobby (connected, slot, leader, players), the
    /// screens' commands reach the host, and a started race shows as the
    /// level to load with the humans in race order.
    #[test]
    fn the_view_follows_the_lobby_and_the_start() {
        let mut lan = Lan::new(&["Ann", "Bob"]);
        let mut w = world(Some(lan.clients.remove(0)));
        let step = |lan: &mut Lan, w: &mut World, cmds: Vec<NetCmd>| {
            lan.host_frame();
            let v = run(w, cmds);
            let now = lan.now;
            for c in &mut lan.clients {
                c.update(now, |_, _| mp_sim::input::InputFrame::default());
            }
            v
        };
        let mut v = NetView::default();
        for _ in 0..30 {
            v = step(&mut lan, &mut w, vec![]);
        }
        assert!(v.connected);
        assert_eq!(v.status, "");
        assert!(v.slot.is_some());
        assert!(v.leader, "the first to join leads");
        assert_eq!(v.lobby.players.len(), 2);
        assert_eq!(v.me().map(|p| p.name.as_str()), Some("Ann"));
        assert_eq!(v.pending_level, None);
        assert!(v.humans.is_empty());

        // Ready and a new car reach the host, and come back in the lobby.
        step(
            &mut lan,
            &mut w,
            vec![
                NetCmd::Ready(true),
                NetCmd::SetMe {
                    name: "Ann".into(),
                    car: "rally".into(),
                    color: 0x1f4fd8,
                },
            ],
        );
        for _ in 0..10 {
            v = step(&mut lan, &mut w, vec![]);
        }
        let me = v.me().unwrap();
        assert!(me.ready);
        assert_eq!((me.car.as_str(), me.color), ("rally", 0x1f4fd8));

        let s = Settings {
            ai: AiFill::None,
            ..Settings::default()
        };
        step(
            &mut lan,
            &mut w,
            vec![NetCmd::Configure(s.clone()), NetCmd::Go(true)],
        );
        for _ in 0..30 {
            v = step(&mut lan, &mut w, vec![]);
            if v.pending_level.is_some() {
                break;
            }
        }
        assert_eq!(v.pending_level.as_deref(), Some("coast"));
        assert_eq!(v.lobby.settings.as_ref().map(|s| s.ai), Some(AiFill::None));
        assert!(!v.ended);
        let names: Vec<&str> = v.humans.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert!(
            names.contains(&"Ann") && names.contains(&"Bob"),
            "{names:?}"
        );
    }

    /// What the client hears becomes the lobby's status line, and the race
    /// in hand's end; Leave drops the connection and clears everything.
    #[test]
    fn host_events_become_the_status_line_and_leave_clears_it() {
        let mut lan = Lan::new(&["Ann"]);
        let mut w = world(Some(lan.clients.remove(0)));
        for _ in 0..10 {
            lan.host_frame();
            run(&mut w, vec![]);
        }
        let say = |w: &mut World, e: ClientEvent| {
            client_of(w).events.push(e);
            run(w, vec![])
        };
        let v = say(&mut w, ClientEvent::Rejected("the lobby is full".into()));
        assert_eq!(v.status, "The host said no: the lobby is full");
        assert_eq!(say(&mut w, ClientEvent::Welcome(0)).status, "");
        assert!(say(&mut w, ClientEvent::RaceEnded).ended);
        assert!(!say(&mut w, ClientEvent::RaceStarted).ended);
        let v = say(&mut w, ClientEvent::Lobby);
        assert_eq!(v.status, "", "a lobby update says nothing");
        assert!(v.connected);
        // The link drops.
        client_of(&mut w).net.close(0);
        let v = run(&mut w, vec![]);
        assert_eq!(v.status, "Lost the host");
        assert!(!v.connected);

        let v = run(&mut w, vec![NetCmd::Leave]);
        assert_eq!(v, NetView::default());
        assert!(w.non_send::<Net>().client.is_none());
    }

    /// Leaving tells the host: the other player sees the lobby without
    /// them, and leads.
    #[test]
    fn leaving_frees_the_lead_for_the_next_player() {
        let mut lan = Lan::new(&["Ann", "Bob"]);
        let leader = lan.settle();
        assert_eq!(leader, 0);
        let mut w = world(Some(lan.clients.remove(0)));
        run(&mut w, vec![NetCmd::Leave]);
        for _ in 0..30 {
            lan.lobby_frame();
        }
        let bob = &lan.clients[0];
        assert!(bob.is_leader());
        assert!(
            bob.lobby
                .players
                .iter()
                .all(|p| p.name == "Bob" || !p.connected),
            "{:?}",
            bob.lobby.players
        );
    }

    /// The view is written only when it changed, so the lobby screen is
    /// rebuilt only then.
    #[test]
    fn an_unchanged_view_is_not_marked_changed() {
        let mut w = world(None);
        run(&mut w, vec![]);
        let t0 = w.resource_ref::<NetView>().last_changed();
        w.increment_change_tick();
        run(&mut w, vec![]);
        assert_eq!(w.resource_ref::<NetView>().last_changed(), t0);
        run(&mut w, vec![NetCmd::Leave]);
        assert_eq!(
            w.resource_ref::<NetView>().last_changed(),
            t0,
            "leaving with nothing open changes nothing"
        );
    }
}
