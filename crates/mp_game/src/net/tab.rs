//! The tab as host (MULTIPLAYER 8.1, DECISIONS D1125): this tab runs the
//! authoritative session, `mp_net::host::Host`, the same one `mp-host`
//! runs. Its transport is a `Mux` of the in-process loopback, on which
//! this tab's own player joins like anyone else, and the remote one
//! (WebRTC in the game, the in-process network in the tests). The tab's
//! player joins first, so it holds slot 0 and leads.
//!
//! The host steps on the page's clock in `net::frame` every frame, races
//! included; the tab must stay in the foreground (SPEC 9.7).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use mp_net::client::Client;
use mp_net::host::{Host, Levels, known_level};
use mp_net::rtc::{RtcStatus, StatusHandle};
use mp_net::transport::{Conditions, Mux, SimNet, Transport};
use mp_sim::race::LevelRuntime;

use super::NetClient;

pub struct TabHost {
    pub host: Host<Mux>,
    /// The invitation to hand out.
    pub invite: String,
    status: Option<StatusHandle>,
    /// The loopback (kept so its clock can move; it has no latency).
    _lan: SimNet,
}

impl TabHost {
    /// A host taking players on `remote`, and this tab's own player's
    /// client on the loopback.
    pub fn new(
        remote: Box<dyn Transport>,
        status: Option<StatusHandle>,
        levels: Levels,
        seed: u32,
        invite: String,
        me: (&str, &str, u32),
    ) -> (TabHost, NetClient) {
        let lan = SimNet::new(seed);
        let mut mux = Mux::new();
        mux.add(Box::new(lan.host()));
        mux.add(remote);
        let host = Host::new(mux, levels, seed);
        let client = Client::new(
            Box::new(lan.connect(Conditions::PERFECT)) as Box<dyn Transport>,
            me.0,
            me.1,
            me.2,
        );
        (
            TabHost {
                host,
                invite,
                status,
                _lan: lan,
            },
            client,
        )
    }

    pub fn update(&mut self, now: f64) {
        self.host.update(now);
        self.host.events.clear();
    }

    /// The room's state on the signalling server (`None` without WebRTC).
    pub fn status(&self) -> Option<RtcStatus> {
        self.status.as_ref().map(StatusHandle::get)
    }
}

/// The levels a tab can host: every level of `mp_levels`; Seaside Raceway
/// once its survey has loaded (the menu loads it), which is when the
/// client could race it too.
pub fn levels() -> Levels {
    let cache: RefCell<BTreeMap<String, Arc<LevelRuntime>>> = RefCell::default();
    Rc::new(move |id: &str| {
        if !known_level(id) {
            return None;
        }
        if let Some(lr) = cache.borrow().get(id) {
            return Some(lr.clone());
        }
        let mut level = mp_levels::level_by_id(id);
        if id == "seaside" {
            mp_levels::seaside::prepare(&mut level, crate::levels::seaside::survey()?);
        }
        let lr = Arc::new(LevelRuntime::new(level).ok()?);
        cache.borrow_mut().insert(id.into(), lr.clone());
        Some(lr)
    })
}

#[cfg(test)]
mod tests {
    //! The tab's session with a remote player on the in-process network:
    //! the tab's player leads, both meet in the lobby, and both race to
    //! the results through the game's own race frames, as two tabs would.

    use super::*;
    use crate::net::tests::{FRAME, race_of};
    use mp_net::proto::{AiFill, Settings};

    #[test]
    fn the_tab_leads_and_races_a_remote_player_to_the_results() {
        let remote_net = SimNet::new(11);
        let lr = Arc::new(LevelRuntime::new(mp_levels::level_by_id("coast")).unwrap());
        let lv = lr.clone();
        let (mut tab, mine) = TabHost::new(
            Box::new(remote_net.host()),
            None,
            Rc::new(move |id: &str| (id == "coast").then(|| lv.clone())),
            9,
            "https://x/#join=…".into(),
            ("Host", "sports", 0xd81e36),
        );
        assert_eq!(tab.status(), None);
        let guest: NetClient = Client::new(
            Box::new(remote_net.connect(Conditions {
                latency: 40.0,
                jitter: 10.0,
                loss: 0.02,
            })),
            "Guest",
            "rally",
            0x1f4fd8,
        );
        let mut clients = [mine, guest];
        let mut now = 0.0;
        let mut step = |tab: &mut TabHost, clients: &mut [NetClient; 2], now: &mut f64| {
            *now += FRAME * 1000.0;
            remote_net.advance_to(*now);
            tab.update(*now);
            for c in clients.iter_mut() {
                c.update(*now, |_, _| mp_sim::input::InputFrame::default());
            }
        };
        for _ in 0..60 {
            step(&mut tab, &mut clients, &mut now);
        }
        assert_eq!(clients[0].slot, Some(0), "the tab's player joins first");
        assert!(clients[0].is_leader());
        assert!(!clients[1].is_leader());
        assert_eq!(clients[1].lobby.players.len(), 2);

        clients[0].configure(Settings {
            ai: AiFill::None,
            ghost: true,
            ..Settings::default()
        });
        clients[0].go(true);
        for _ in 0..60 {
            step(&mut tab, &mut clients, &mut now);
            if clients.iter().all(|c| c.pending.is_some()) {
                break;
            }
        }
        let mut races: Vec<_> = clients
            .iter_mut()
            .map(|c| {
                assert!(c.attach(lr.clone()));
                race_of(c, &lr, true)
            })
            .collect();
        let mut done = false;
        for _ in 0..(15 * 60 * 60) {
            now += FRAME * 1000.0;
            remote_net.advance_to(now);
            tab.update(now);
            for (c, r) in clients.iter_mut().zip(races.iter_mut()) {
                if c.race.is_some() {
                    r.frame_online(FRAME, c, now);
                }
            }
            if races.iter().all(|r| r.results.is_some()) {
                done = true;
                break;
            }
        }
        assert!(done, "both reach the results");
        for (c, r) in clients.iter().zip(&races) {
            assert_eq!(c.stats.desyncs, 0, "{}: {:?}", c.name, c.stats);
            let res = r.results.as_ref().unwrap();
            assert_eq!(res.iter().filter(|row| row.human.is_some()).count(), 2);
        }
        // Both saw the same race: the same finishing order.
        let order = |i: usize| -> Vec<Option<usize>> {
            races[i]
                .results
                .as_ref()
                .unwrap()
                .iter()
                .map(|row| row.human)
                .collect()
        };
        assert_eq!(order(0), order(1));
    }

    #[test]
    fn the_levels_a_tab_can_host() {
        let lv = levels();
        assert!(lv("coast").is_some());
        let again = lv("coast").unwrap();
        assert!(Arc::ptr_eq(&again, &lv("coast").unwrap()), "built once");
        assert!(lv("moon").is_none());
        for l in mp_levels::levels() {
            if l.id != "seaside" {
                assert!(lv(l.id).is_some(), "{}", l.id);
            }
        }
    }
}
