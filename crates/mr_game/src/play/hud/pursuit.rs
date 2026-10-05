//! `PursuitView.events`, its HUD and camera half (roadmap WP 8.3): what
//! each pursuit event puts on the screen (the centre pops and toasts) and
//! the camera's jolts and snap. The rules' half runs in the simulation
//! (`mr_sim::race::pursuit_events`), the radio lines in `play::radio` (WP
//! 8.4), the rumble in `flow::Race::on_event`, and the smoke and sparks in
//! the effects (WP 8.2).

use crate::play::camera::CameraRig;
use crate::play::flow::Hud;
use mr_sim::pursuit::PursuitEvent;

/// One event, as `PursuitView.events` shows it.
pub fn event(hud: &mut Hud, rig: &mut CameraRig, e: &PursuitEvent) {
    match e {
        PursuitEvent::Pursuit { .. } => hud.center("PURSUIT", 1.4),
        PursuitEvent::Reacquired { .. } => hud.toast("SPOTTED", 1.6),
        PursuitEvent::Cooldown => hud.toast("COOLDOWN — STAY OUT OF SIGHT", 2.0),
        PursuitEvent::Escaped { .. } => hud.center("ESCAPED", 1.6),
        PursuitEvent::Heat { heat } => hud.toast(format!("HEAT LEVEL {heat}"), 1.8),
        PursuitEvent::Takedown {
            by_player: true, ..
        } => {
            hud.center("TAKEDOWN", 1.2);
            rig.bump(1.0);
        }
        PursuitEvent::Roadblock { heavy, .. } => hud.toast(
            if *heavy {
                "HEAVY ROADBLOCK AHEAD"
            } else {
                "ROADBLOCK AHEAD"
            },
            2.0,
        ),
        PursuitEvent::Spikes { .. } => hud.toast("SPIKE STRIP AHEAD", 2.0),
        PursuitEvent::Spiked { player, name, .. } => {
            if *player {
                hud.center("SPIKED!", 1.2);
            } else {
                hud.toast(format!("{} HIT THE SPIKES", name.to_uppercase()), 1.6);
            }
        }
        PursuitEvent::Dodge { spikes } => hud.toast(
            if *spikes {
                "SPIKES DODGED"
            } else {
                "ROADBLOCK DODGED"
            },
            1.6,
        ),
        PursuitEvent::Busted { player, name, .. } => {
            if *player {
                hud.center("BUSTED", 2.0);
            } else {
                hud.toast(format!("{} BUSTED", name.to_uppercase()), 1.8);
            }
        }
        // (PursuitView shows it for whoever is wrecked: only the player is.)
        PursuitEvent::Wrecked { .. } => {
            hud.center("WRECKED", 2.0);
            rig.bump(1.2);
        }
        // `release(e)`: the car is back on the road (the simulation put it
        // there), the camera behind it at once.
        PursuitEvent::Release { player: true, .. } => {
            rig.snap = true;
            hud.toast("BACK IN THE RACE", 1.4);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(e: PursuitEvent) -> (Hud, CameraRig) {
        let mut h = Hud::default();
        let mut rig = CameraRig {
            snap: false,
            ..CameraRig::default()
        };
        event(&mut h, &mut rig, &e);
        (h, rig)
    }

    #[test]
    fn the_texts_and_their_times() {
        let (h, _) = run(PursuitEvent::Heat { heat: 3 });
        assert_eq!(
            (h.toast.as_deref(), h.toast_timer),
            (Some("HEAT LEVEL 3"), 1.8)
        );
        let (h, _) = run(PursuitEvent::Busted {
            racer: 1,
            seconds: 6.0,
            player: false,
            name: "Vex",
        });
        assert_eq!(h.toast.as_deref(), Some("VEX BUSTED"));
        assert_eq!(h.center, None);
        let (h, _) = run(PursuitEvent::Busted {
            racer: 0,
            seconds: 6.0,
            player: true,
            name: "You",
        });
        assert_eq!((h.center.as_deref(), h.center_timer), (Some("BUSTED"), 2.0));
        let (h, _) = run(PursuitEvent::Roadblock {
            s: 0.0,
            heavy: true,
        });
        assert_eq!(h.toast.as_deref(), Some("HEAVY ROADBLOCK AHEAD"));
        let (h, _) = run(PursuitEvent::Cooldown);
        assert_eq!(h.toast.as_deref(), Some("COOLDOWN — STAY OUT OF SIGHT"));
    }

    #[test]
    fn a_release_snaps_the_camera() {
        let (h, rig) = run(PursuitEvent::Release {
            racer: 0,
            player: true,
            spot: Some((100.0, 0.0)),
        });
        assert!(rig.snap);
        assert_eq!(h.toast.as_deref(), Some("BACK IN THE RACE"));
        let (h, rig) = run(PursuitEvent::Release {
            racer: 2,
            player: false,
            spot: None,
        });
        assert!(!rig.snap);
        assert_eq!(h.toast, None);
    }
}
