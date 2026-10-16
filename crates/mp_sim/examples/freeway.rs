//! A person-like driver holding the middle of Cruise's freeway on the sim
//! car (DECISIONS D1205): a 0.2 s reaction delay, a point a little ahead to
//! steer for, and a stick (the cornering it wants, as a share of full
//! steering) or keys (pressed past a threshold, ramped as the client ramps
//! them). For each speed and input it tries a range of gains and preview
//! times and reports the best, over four stretches of the freeway: the
//! lateral error's RMS, the worst excursion, and wall hits.
//!
//! `cargo run --release -p mp_sim --example freeway`
use std::collections::VecDeque;

use mp_sim::input::{Input, InputFrame};
use mp_sim::model::{SimCar, sim_def};
use mp_sim::physics::PhysEvent;
use mp_sim::race::{LevelRuntime, PlayerCar};
use pk_math::{clamp, kernel, wrap_angle};

struct Out {
    rms: f64,
    max: f64,
    walls: u32,
    vavg: f64,
}

fn run(t: &mp_track::Track, s0: f64, vt: f64, keys: bool, k: f64, tp: f64) -> Out {
    let mut p = PlayerCar::new("sports");
    p.phys.reset(&mut p.v, t, s0, 0.0);
    p.v.vx = kernel::cos(p.v.yaw) * vt;
    p.v.vz = kernel::sin(p.v.yaw) * vt;
    p.v.speed = vt;
    p.phys.locked = false;
    let mut car = SimCar::new(sim_def("sports", p.spec.mass), &p.v, t);
    let delay = 24;
    let mut q: VecDeque<f64> = VecDeque::from(vec![0.0; delay]);
    let mut key_steer = 0.0f64;
    let (mut se, mut mx, mut n, mut walls, mut vsum) = (0.0, 0.0f64, 0, 0, 0.0);
    let dt = 1.0 / 120.0;
    for _ in 0..(25 * 120) {
        let v = &p.v;
        let sp = kernel::hypot(v.vx, v.vz);
        let lp = (sp * tp).max(15.0);
        let pt = t.point_at(v.s + lp, 0.0);
        let alpha = wrap_angle(kernel::atan2(pt.z - v.z, pt.x - v.x) - v.yaw);
        let ay_req = 2.0 * sp * sp * kernel::sin(alpha) / lp;
        let want = clamp(k * ay_req / 10.0, -1.0, 1.0);
        // The game's positive steer is to the right.
        q.push_back(want);
        let cmd = q.pop_front().unwrap();
        let mut inp = Input {
            analog: !keys,
            ..Input::default()
        };
        if keys {
            let target = if cmd > 0.08 {
                1.0
            } else if cmd < -0.08 {
                -1.0
            } else {
                0.0
            };
            let rate = if target == 0.0 {
                7.0
            } else if target * key_steer < 0.0 {
                9.0
            } else {
                3.6
            };
            key_steer += clamp(target - key_steer, -rate * dt, rate * dt);
            inp.steer = key_steer;
        } else {
            inp.steer = cmd;
        }
        inp.throttle = clamp((vt - sp) * 0.5, 0.0, 1.0);
        inp.brake = if sp > vt + 2.0 {
            clamp((sp - vt - 2.0) * 0.3, 0.0, 1.0)
        } else {
            0.0
        };
        car.update(&mut p.v, &mut p.phys, t, dt, &inp, &InputFrame::default());
        walls += p
            .phys
            .events
            .iter()
            .filter(|e| matches!(e, PhysEvent::Impact { .. }))
            .count() as u32;
        p.phys.events.clear();
        se += p.v.lat * p.v.lat;
        mx = mx.max(p.v.lat.abs());
        vsum += sp;
        n += 1;
    }
    Out {
        rms: (se / n as f64).sqrt(),
        max: mx,
        walls,
        vavg: vsum / n as f64,
    }
}

fn main() {
    let lr = LevelRuntime::new(mp_levels::level_by_id("cruise")).unwrap();
    let t = &*lr.track;
    let starts = [1000.0, 3000.0, 5600.0, 8000.0];
    for keys in [false, true] {
        for vt in [55.0, 60.0, 67.0] {
            let mut best: Option<(f64, f64, f64, Out)> = None;
            for k in [0.4, 0.6, 0.8, 1.0, 1.3, 1.7, 2.2] {
                for tp in [0.6, 0.9, 1.2, 1.6] {
                    let os: Vec<Out> = starts
                        .iter()
                        .map(|&s0| run(t, s0, vt, keys, k, tp))
                        .collect();
                    let m = os.len() as f64;
                    let o = Out {
                        rms: os.iter().map(|o| o.rms).sum::<f64>() / m,
                        max: os.iter().map(|o| o.max).fold(0.0, f64::max),
                        walls: os.iter().map(|o| o.walls).sum(),
                        vavg: os.iter().map(|o| o.vavg).sum::<f64>() / m,
                    };
                    let score = o.rms + 5.0 * o.walls as f64;
                    if best
                        .as_ref()
                        .is_none_or(|b| score < b.3.rms + 5.0 * b.3.walls as f64)
                    {
                        best = Some((k, tp, score, o));
                    }
                }
            }
            let (k, tp, _, o) = best.unwrap();
            println!(
                "{} {vt} m/s: best K {k} Tp {tp}: lat rms {:.2} m, max {:.2} m, walls {}, avg {:.1} m/s",
                if keys { "keys " } else { "stick" },
                o.rms,
                o.max,
                o.walls,
                o.vavg
            );
        }
    }
}
