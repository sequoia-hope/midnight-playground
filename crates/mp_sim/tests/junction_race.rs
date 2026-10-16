//! The whole race state (rivals, traffic, police, standings) keeps ticking
//! while the player drives off Highway 1 onto Highway 84 (stage 3 of
//! docs/vision/peninsula.md): what the client runs when the owner turns
//! left at San Gregorio.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mp_sim::assist::{Assist, steer_assist};
use mp_sim::input::{Input, InputFrame};
use mp_sim::race::{LevelRuntime, RaceOpts, SimState, standings, step};
use pk_math::{clamp, kernel, wrap_angle};
use std::f64::consts::PI;

const DT: f64 = 1.0 / 120.0;

#[test]
fn the_race_keeps_ticking_while_the_player_drives_up_84() {
    for physics in [
        mp_sim::model::Physics::Arcade,
        mp_sim::model::Physics::Player,
        mp_sim::model::Physics::All,
    ] {
        eprintln!("physics {physics:?}");
        drive(physics);
    }
}

fn drive(physics: mp_sim::model::Physics) {
    let lr = LevelRuntime::new(mp_levels::level_by_id("southcoast")).unwrap();
    let t = std::sync::Arc::clone(&lr.track);
    let b = &t.branches[0];
    let mut st = SimState::new(
        &lr,
        RaceOpts {
            car: "sports",
            seed: 1,
            pursuit: true,
            heat: 1.0,
            physics,
        },
    );
    let mut events = Vec::new();
    // Through the countdown, then turn round and drive to the junction.
    let mut el = 0.0;
    let mut switched = false;
    while el < 240.0 {
        let v = &st.players[0].v;
        let aim = if v.road == 0 {
            if v.s > b.s + 150.0 {
                t.point_at(v.s - 40.0, 0.0)
            } else {
                b.track.point_at(8.0, 0.0)
            }
        } else {
            b.track.point_at(v.road_s + 30.0, 0.0)
        };
        let alpha = wrap_angle(kernel::atan2(aim.z - v.z, aim.x - v.x) - v.yaw);
        let speed = kernel::hypot(v.vx, v.vz);
        let fast = v.road == 0 && v.s > b.s + 200.0;
        let (want, reverse) = if fast { (22.0, false) } else { (14.0, false) };
        let _ = reverse;
        let inp = Input {
            steer: clamp(alpha * 2.5, -1.0, 1.0),
            throttle: if speed < want { 0.6 } else { 0.0 },
            brake: if speed > want + 2.0 { 0.3 } else { 0.0 },
            ..Input::default()
        };
        // At the start, turn round: full lock and throttle until facing back.
        let inp = if el < 1.0 {
            Input {
                throttle: 1.0,
                ..Input::default()
            }
        } else if !switched && v.road == 0 && v.s > b.s + 200.0 {
            let f = t.frame(v.s);
            let heading = kernel::atan2(f.fz, f.fx);
            let facing_back = wrap_angle(v.yaw - heading).abs() > PI * 0.8;
            if facing_back {
                inp
            } else {
                Input {
                    steer: 1.0,
                    throttle: 0.7,
                    ..Input::default()
                }
            }
        } else {
            inp
        };
        // The client's steering help runs on the controls before the tick
        // (the phone's default), on the road under the car.
        let mut helped = inp;
        {
            let p = &st.players[0];
            steer_assist(&mut helped, &p.v, &p.phys, &t, Assist::Strong);
        }
        let frame = InputFrame::quantise(&helped);
        step(&lr, &mut st, &[frame], &mut events);
        events.clear();
        let _ = standings(&st);
        if st.players[0].v.road == 1 && !switched {
            switched = true;
            eprintln!("onto 84 at {el:.1} s");
        }
        if switched && st.players[0].v.road_s > 800.0 {
            break;
        }
        el += DT;
    }
    let v = &st.players[0].v;
    eprintln!(
        "end: road {} road_s {:.1} s {:.1} lat {:.1} at {el:.1} s",
        v.road, v.road_s, v.s, v.lat
    );
    assert!(switched, "the car got onto 84");
    assert!(v.road_s > 800.0, "well up 84: {}", v.road_s);
}
