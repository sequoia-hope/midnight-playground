//! Random driving around the open world's junction with the whole race
//! state ticking (docs/vision/peninsula.md, stage 3): the player placed
//! anywhere near the mouth of Highway 84, at any heading and speed, with
//! random controls, must keep the simulation finite and quick.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mp_sim::assist::{Assist, steer_assist};
use mp_sim::input::{Input, InputFrame};
use mp_sim::race::{LevelRuntime, RaceOpts, SimState, standings, step};
use pk_math::kernel;
use std::f64::consts::PI;

/// A small LCG, so the cases are the same everywhere.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
}

#[test]
fn random_driving_around_the_junction_stays_finite() {
    let lr = LevelRuntime::new(mp_levels::level_by_id("southcoast")).unwrap();
    let t = std::sync::Arc::clone(&lr.track);
    let b = &t.branches[0];
    let mut rng = Lcg(7);
    for case in 0..60 {
        let physics = match case % 3 {
            0 => mp_sim::model::Physics::Arcade,
            1 => mp_sim::model::Physics::Player,
            _ => mp_sim::model::Physics::All,
        };
        let mut st = SimState::new(
            &lr,
            RaceOpts {
                car: "sports",
                seed: case as u32 + 1,
                pursuit: case % 2 == 0,
                heat: 1.0,
                physics,
            },
        );
        // Place the player near the mouth: on either road, or the land.
        let (x, z) = if case % 4 == 0 {
            let p = b
                .track
                .point_at(rng.range(0.0, 200.0), rng.range(-30.0, 30.0));
            (p.x, p.z)
        } else {
            let p = t.point_at(b.s + rng.range(-200.0, 200.0), rng.range(-120.0, 120.0));
            (p.x, p.z)
        };
        let yaw = rng.range(-PI, PI);
        let speed = rng.range(0.0, 45.0);
        {
            let p = &mut st.players[0];
            p.v.x = x;
            p.v.z = z;
            p.v.yaw = yaw;
            p.v.vx = kernel::cos(yaw) * speed;
            p.v.vz = kernel::sin(yaw) * speed;
            p.v.project_onto(&t);
            p.v.y = t
                .road(p.v.road)
                .ground_y(p.v.x, p.v.z, p.v.road_s, p.v.road_lat);
        }
        st.race.countdown = 0.0;
        let mut events = Vec::new();
        // No clock in wasm under Node (std's Instant panics there).
        #[cfg(not(target_arch = "wasm32"))]
        let start = std::time::Instant::now();
        let mut inp = Input::default();
        for tick in 0..(20 * 120) {
            if tick % 30 == 0 {
                inp = Input {
                    steer: rng.range(-1.0, 1.0),
                    throttle: if rng.next() < 0.6 { rng.next() } else { 0.0 },
                    brake: if rng.next() < 0.2 { rng.next() } else { 0.0 },
                    handbrake: rng.next() < 0.05,
                    ..Input::default()
                };
            }
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
            let v = &st.players[0].v;
            for c in [v.x, v.y, v.z, v.s, v.lat, v.road_s, v.road_lat, v.yaw] {
                assert!(c.is_finite(), "case {case} tick {tick}: {v:?}");
            }
        }
        let v = &st.players[0].v;
        #[cfg(not(target_arch = "wasm32"))]
        let took = start.elapsed().as_secs_f64();
        #[cfg(target_arch = "wasm32")]
        let took = 0.0;
        eprintln!(
            "case {case} {physics:?} pursuit {}: from ({x:.0},{z:.0}) yaw {yaw:.2} speed {speed:.0}: ends road {} s {:.0} lat {:.0} road_s {:.0} in {took:.2} s",
            case % 2 == 0,
            v.road,
            v.s,
            v.lat,
            v.road_s
        );
        assert!(took < 5.0, "case {case} took {took} s for 20 s of race");
    }
}
