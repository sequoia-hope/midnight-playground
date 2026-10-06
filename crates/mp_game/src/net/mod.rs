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
use mp_net::transport::Transport;

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
    /// Connect to the host that served this page (web), or `url`.
    Join {
        url: Option<String>,
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
                name,
                car,
                color,
            } => {
                let Some(url) = url.or_else(default_url) else {
                    view.status = "No host to join: open the game from the host's address".into();
                    continue;
                };
                match connect(&url) {
                    Ok(t) => {
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
            NetCmd::Leave => {
                if let Some(c) = net.client.as_mut() {
                    c.leave();
                }
                net.client = None;
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
mod tests {
    //! The whole client path without graphics: a host and two game clients
    //! on the in-process network, each driving a real `Race` through
    //! `frame_online` on the autopilot, from the lobby to the results.

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
}
