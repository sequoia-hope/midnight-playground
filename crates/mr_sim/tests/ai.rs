//! vehicles/AIDriver.js and Kinematic.js: the rivals. Each level's field
//! drives its route to the finish (or once round the cruise loop) on the
//! tarmac and in a sane time, with collisions resolved as Race does it. Also
//! covers overtaking, rubber-banding, holding on the grid, and the track-
//! coordinate car underneath (walls, spin, velocity round trips), and a
//! rival that ends up alongside you, or jammed between you and a wall.
//! Port of `test/unit/ai.test.js`, same assertions; `withSeededRandom(n)`
//! becomes the rivals' generator `mulberry32(n)`.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use std::f64::consts::PI;

use common::{make_vehicle, straight_track};
use mr_math::{Mulberry32, Rng, kernel};
use mr_sim::ai::{AiCtx, AiDriver, AiOpts};
use mr_sim::body::{AgentView, Body, PhysicsBody};
use mr_sim::collisions::resolve_collisions;
use mr_sim::input::Input;
use mr_sim::kinematic::Kinematic;
use mr_sim::physics::{CAR_SPECS, CarPhysics, car_spec};
use mr_track::Track;

const DT: f64 = 1.0 / 60.0;

struct R {
    name: &'static str,
    skill: f64,
    power: f64,
}

/// Rivals as Race.js builds them, on its grid (rows of two, 10 m apart).
fn field(t: &Track, rivals: &[R], rng: &mut dyn Rng) -> Vec<AiDriver> {
    rivals
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let opts = AiOpts {
                skill: Some(r.skill),
                name: r.name,
                power: Some(r.power),
                bias: Some((if i % 2 == 1 { 1.0 } else { -1.0 }) * 0.6),
                line_factor: Some(0.8 + (i % 3) as f64 * 0.08),
            };
            let mut ai = AiDriver::new(make_vehicle("rival", 1400.0), opts, rng);
            let (row, col) = ((i / 2) as f64, i % 2);
            ai.k.s = t.start_s - 5.0 - row * 10.0 - col as f64 * 3.0;
            ai.k.lat = if col == 1 { 2.4 } else { -2.4 };
            ai.write_pos(t);
            ai
        })
        .collect()
}

/// `ctx` for rival `i` of `ais`, with `extra` cars after them.
fn step_field(ais: &mut [AiDriver], t: &Track, player_s: f64, time: f64, rng: &mut dyn Rng) {
    for i in 0..ais.len() {
        let views: Vec<AgentView> = ais.iter().map(|a| a.view()).collect();
        let ctx = AiCtx {
            cars: &views,
            me: Some(i),
            player_s,
            player_prog: None,
            started: true,
            time,
        };
        ais[i].update(t, DT, &ctx, rng);
    }
}

#[test]
fn each_field_drives_the_route_on_the_tarmac() {
    for level in mr_levels::levels() {
        let level = common::level(level.id);
        let t = Track::new(&level).unwrap();
        let mut rng = Mulberry32::new(7);
        let rivals: Vec<R> = if level.rivals.is_empty() {
            vec![R {
                name: "Solo",
                skill: 0.95,
                power: 500.0,
            }]
        } else {
            level
                .rivals
                .iter()
                .map(|r| R {
                    name: r.name,
                    skill: r.skill,
                    power: r.power,
                })
                .collect()
        };
        let mut ais = field(&t, &rivals, &mut rng);
        let goal = if t.is_loop { t.length } else { f64::INFINITY }; // once round a loop
        let mut dist = vec![0.0; ais.len()];
        let mut prev: Vec<f64> = ais.iter().map(|a| a.k.s).collect();
        let (mut time, mut worst_centre, mut worst_wall, mut wher) =
            (0.0, 0.0f64, f64::NEG_INFINITY, String::new());
        while time < 600.0 {
            let ps = ais[0].k.s;
            step_field(&mut ais, &t, ps, time, &mut rng);
            resolve_collisions(&mut ais[..], &t, &mut Vec::new());
            for a in ais.iter_mut() {
                a.write_pos(&t);
            }
            time += 1.0 / 60.0;
            for (i, a) in ais.iter().enumerate() {
                dist[i] += t.ds(prev[i], a.k.s);
                prev[i] = a.k.s;
                let f = t.frame(a.k.s);
                // The centre stays on the tarmac (passing may use the shoulder),
                // and the body never goes through a wall.
                if a.k.lat.abs() - f.hw > worst_centre {
                    worst_centre = a.k.lat.abs() - f.hw;
                    wher = format!("{} at s={:.0}", a.name, a.k.s);
                }
                worst_wall = worst_wall
                    .max(a.k.lat + a.k.half_w() - f.wall_r)
                    .max(-a.k.lat + a.k.half_w() - f.wall_l);
                for (k, x) in [
                    ("x", a.k.v.x),
                    ("y", a.k.v.y),
                    ("z", a.k.v.z),
                    ("yaw", a.k.v.yaw),
                ] {
                    assert!(
                        x.is_finite(),
                        "{}: {}.{k} = {x} at {time:.1} s",
                        level.id,
                        a.name
                    );
                }
            }
            if ais
                .iter()
                .enumerate()
                .all(|(i, a)| a.finished || dist[i] >= goal)
            {
                break;
            }
        }
        if !t.is_loop {
            for a in &ais {
                assert!(
                    a.finished,
                    "{}: {} finished (s {:.0} of {})",
                    level.id, a.name, a.k.s, t.finish_s
                );
                let ft = a.finish_time.unwrap();
                let avg = (t.finish_s - t.start_s) / ft;
                assert!(
                    avg > 25.0 && avg < 69.5,
                    "{}: {}: average {avg:.1} m/s over {ft:.1} s",
                    level.id,
                    a.name
                );
            }
        } else {
            for (i, d) in dist.iter().enumerate() {
                assert!(
                    *d >= goal,
                    "{}: {} went round ({d:.0} of {goal} m)",
                    level.id,
                    ais[i].name
                );
            }
            assert!(time < 300.0, "{}: a lap in {time:.0} s", level.id);
        }
        assert!(
            worst_centre <= 0.0,
            "{}: centre {worst_centre:.2} m off the tarmac ({wher})",
            level.id
        );
        assert!(
            worst_wall <= 0.01,
            "{}: body {worst_wall:.2} m into a wall",
            level.id
        );
    }
}

#[test]
fn with_a_clear_road_a_more_skilful_rival_finishes_sooner() {
    let t = Track::new(&common::level("sierra")).unwrap();
    let mut rng = Mulberry32::new(3);
    let times: Vec<f64> = [0.93, 0.96, 0.99]
        .iter()
        .map(|&skill| {
            let mut a = field(
                &t,
                &[R {
                    name: "X",
                    skill,
                    power: 520.0,
                }],
                &mut rng,
            )
            .remove(0);
            let mut time = 0.0;
            while !a.finished && time < 600.0 {
                let views = [a.view()];
                let ctx = AiCtx {
                    cars: &views,
                    me: Some(0),
                    player_s: a.k.s,
                    player_prog: None,
                    started: true,
                    time,
                };
                a.update(&t, DT, &ctx, &mut rng);
                time += DT;
            }
            a.finish_time.unwrap()
        })
        .collect();
    assert!(
        times[0] > times[1] && times[1] > times[2],
        "finish times {times:?}"
    );
}

#[test]
fn rivals_wait_on_the_grid_until_the_start() {
    let t = straight_track(2000.0, "freeway");
    let mut rng = Mulberry32::new(1);
    let mut a = field(
        &t,
        &[R {
            name: "X",
            skill: 0.95,
            power: 500.0,
        }],
        &mut rng,
    )
    .remove(0);
    let s = a.k.s;
    for _ in 0..120 {
        let views = [a.view()];
        let ctx = AiCtx {
            cars: &views,
            me: Some(0),
            player_s: s,
            player_prog: None,
            started: false,
            time: 0.0,
        };
        a.update(&t, DT, &ctx, &mut rng);
    }
    assert_eq!(a.k.s, s);
    assert_eq!(a.k.speed, 0.0);
    let views = [a.view()];
    let ctx = AiCtx {
        cars: &views,
        me: Some(0),
        player_s: s,
        player_prog: None,
        started: true,
        time: 0.0,
    };
    a.update(&t, DT, &ctx, &mut rng);
    assert!(a.k.speed > 0.0, "goes at GO");
}

#[test]
fn rubber_band_a_rival_far_ahead_eases_off_one_far_behind_pushes() {
    let t = straight_track(4000.0, "freeway");
    let mut rng = Mulberry32::new(5);
    let mut run = |gap: f64| {
        let mut a = field(
            &t,
            &[R {
                name: "X",
                skill: 0.95,
                power: 500.0,
            }],
            &mut rng,
        )
        .remove(0);
        a.k.s = 500.0;
        a.k.speed = 30.0;
        a.write_pos(&t);
        a.nitro_timer = 1e9; // no nitro in this comparison
        for i in 0..300 {
            let views = [a.view()];
            let ctx = AiCtx {
                cars: &views,
                me: Some(0),
                player_s: a.k.s - gap,
                player_prog: None,
                started: true,
                time: i as f64 * DT,
            };
            a.update(&t, DT, &ctx, &mut rng);
        }
        a.k.speed
    };
    let (level, ahead, behind) = (run(0.0), run(400.0), run(-350.0));
    assert!(
        ahead < level - 2.0,
        "400 m ahead of the player: {ahead:.1} vs {level:.1} m/s"
    );
    assert!(
        behind > level + 2.0,
        "350 m behind: {behind:.1} vs {level:.1} m/s"
    );
}

struct Overtake {
    passed: bool,
    closest: f64,
    need: f64,
    at: f64,
}

/// A rival coming up on a slower car in its lane, on a long straight.
/// Returns how far apart sideways they were while alongside (bodies overlap
/// when that is under the sum of their half-widths), and whether it got by.
fn overtake(speed: f64, block_speed: f64, gap: f64, rng: &mut dyn Rng) -> Overtake {
    let t = straight_track(3000.0, "freeway");
    let mut a = field(
        &t,
        &[R {
            name: "X",
            skill: 0.95,
            power: 500.0,
        }],
        rng,
    )
    .remove(0);
    a.k.s = 200.0;
    a.k.lat = 0.0;
    a.k.speed = speed;
    a.nitro_timer = 1e9;
    a.write_pos(&t);
    let mut block = Kinematic::new(make_vehicle("rival", 1500.0));
    block.s = 200.0 + gap;
    block.lat = 0.0;
    block.speed = block_speed;
    block.write_pos(&t);
    let (mut closest, mut at) = (f64::INFINITY, 0.0);
    let mut i = 0;
    while i < 3000 && a.k.s < block.s + 50.0 {
        let views = [a.view(), block.view()];
        let ctx = AiCtx {
            cars: &views,
            me: Some(0),
            player_s: a.k.s,
            player_prog: None,
            started: true,
            time: i as f64 * DT,
        };
        a.update(&t, DT, &ctx, rng);
        block.speed = block_speed;
        block.advance(&t, DT);
        if (a.k.s - block.s).abs() < a.k.half_l() + block.half_l()
            && (a.k.lat - block.lat).abs() < closest
        {
            closest = (a.k.lat - block.lat).abs();
            at = a.k.speed;
        }
        i += 1;
    }
    Overtake {
        passed: a.k.s > block.s + 50.0,
        closest,
        need: a.k.half_w() + block.half_w(),
        at,
    }
}

#[test]
fn a_rival_steers_round_a_stopped_car_it_sees_in_time() {
    let mut rng = Mulberry32::new(9);
    let r = overtake(30.0, 0.0, 60.0, &mut rng);
    assert!(r.passed, "got past");
    assert!(
        r.closest > r.need,
        "passed {:.2} m off-centre (needs {:.2})",
        r.closest,
        r.need
    );
}

/// The look-ahead is a fixed 45 m, which at 60+ m/s leaves well under a
/// second to move over, and the rival doesn't brake when it can't.
#[test]
fn at_racing_speed_a_rival_gets_round_a_slow_or_stopped_car_without_hitting_it() {
    let mut rng = Mulberry32::new(9);
    for (speed, block_speed, gap) in [(60.0, 25.0, 400.0), (60.0, 0.0, 400.0)] {
        let r = overtake(speed, block_speed, gap, &mut rng);
        assert!(
            r.closest > r.need,
            "rival at {:.0} m/s passing a car doing {block_speed} m/s: {:.2} m apart sideways, bodies overlap under {:.2}",
            r.at,
            r.closest,
            r.need
        );
    }
}

struct Alongside {
    a: AiDriver,
    car: Kinematic,
    contacts: usize,
    closest: f64,
}

/// A car level with a rival, held at a steady 30 m/s; the rival's line
/// (bias) runs straight through it. Returns the rival, how often they
/// touched over `seconds`, and how far apart sideways they were while level.
fn alongside(car_lat: f64, rival_lat: f64, bias: f64, seconds: f64) -> Alongside {
    let t = straight_track(3000.0, "freeway");
    let mut rng = Mulberry32::new(1);
    let opts = AiOpts {
        skill: Some(0.95),
        name: "X",
        bias: Some(bias),
        line_factor: Some(0.0),
        power: None,
    };
    let mut a = AiDriver::new(make_vehicle("rival", 1400.0), opts, &mut rng);
    a.k.s = 300.0;
    a.k.lat = rival_lat;
    a.k.speed = 30.0;
    a.nitro_timer = 1e9;
    a.write_pos(&t);
    let mut car = Kinematic::new(make_vehicle("rival", 1400.0));
    car.s = 300.0;
    car.lat = car_lat;
    car.speed = 30.0;
    car.write_pos(&t);
    let (mut contacts, mut closest) = (0, f64::INFINITY);
    let mut i = 0.0;
    while i < seconds / DT {
        let views = [a.view(), car.view()];
        let ctx = AiCtx {
            cars: &views,
            me: Some(0),
            player_s: car.s,
            player_prog: None,
            started: true,
            time: i * DT,
        };
        a.update(&t, DT, &ctx, &mut rng);
        car.speed = 30.0;
        car.lat_vel = 0.0;
        car.advance(&t, DT);
        let mut hits = Vec::new();
        let mut bodies: [&mut dyn Body; 2] = [&mut a, &mut car];
        resolve_collisions(&mut bodies[..], &t, &mut hits);
        contacts += hits.len();
        a.write_pos(&t);
        car.write_pos(&t);
        if (a.k.s - car.s).abs() < a.k.half_l() + car.half_l() {
            closest = closest.min((a.k.lat - car.lat).abs());
        }
        i += 1.0;
    }
    Alongside {
        a,
        car,
        contacts,
        closest,
    }
}

/// findBlock only sees cars ahead, so a rival used to steer for its line
/// straight through a car beside it, and lean on it every frame.
#[test]
fn a_rival_alongside_keeps_off_your_side_instead_of_steering_into_you() {
    let r = alongside(3.0, 0.6, 6.0, 3.0);
    assert_eq!(r.contacts, 0, "touched {} frames", r.contacts);
    assert!(
        r.closest > r.a.k.half_w() + r.car.half_w(),
        "{:.2} m apart sideways while level",
        r.closest
    );
}

#[test]
fn a_rival_boxed_in_between_you_and_the_wall_drops_back_behind_you() {
    let w = straight_track(3000.0, "freeway").wall_l[300] as f64;
    let r = alongside(-w + 3.4, -w + 1.2, 4.0, 3.0);
    assert!(
        r.car.s - r.a.k.s > r.a.k.half_l() + r.car.half_l(),
        "dropped back {:.1} m",
        r.car.s - r.a.k.s
    );
    assert!(r.contacts < 10, "touched {} frames", r.contacts);
}

/// The bug this guards against: nosed into a wall at speed with a rival
/// jammed between your rear quarter and the wall, the rival's shove (its
/// line runs through you) spun your nose back into the wall every frame. You
/// ground along the wall at full lock, unable to turn off it.
#[test]
fn pinned_to_a_wall_by_a_rival_at_your_rear_quarter_you_can_still_steer_off_it() {
    let t = straight_track(3000.0, "freeway");
    let w = t.wall_r[300] as f64;
    for (kind, _) in CAR_SPECS {
        let spec = car_spec(kind).unwrap();
        let mut v = make_vehicle(kind, spec.mass);
        let mut phys = CarPhysics::new(&v, spec);
        let rel = 0.35; // nose into the right-hand wall
        let lat = w - v.half_w * kernel::cos(rel) - v.half_l * kernel::sin(rel);
        phys.reset(&mut v, &t, 300.0, lat);
        v.yaw = rel;
        v.vx = 30.0;
        phys.drifting = true;
        let mut rng = Mulberry32::new(1);
        let opts = AiOpts {
            skill: Some(0.95),
            name: "X",
            ..AiOpts::default()
        };
        let mut a = AiDriver::new(make_vehicle("rival", 1400.0), opts, &mut rng);
        a.k.s = 297.5;
        a.k.lat = w - a.k.half_w() - 0.15;
        a.k.speed = 30.0;
        a.k.spin = 0.8;
        a.k.stunned = 1.5;
        a.write_pos(&t);
        let mut time = 0.0;
        while w - v.lat - v.half_w < 2.0 && time < 5.0 {
            phys.update(
                &mut v,
                &t,
                DT,
                &Input {
                    throttle: 1.0,
                    steer: -1.0,
                    ..Input::default()
                },
            );
            let views = [mr_sim::body::player_view(&v), a.view()];
            let ctx = AiCtx {
                cars: &views,
                me: Some(1),
                player_s: v.s,
                player_prog: None,
                started: true,
                time,
            };
            a.update(&t, DT, &ctx, &mut rng);
            let mut pb = PhysicsBody { v: &mut v };
            let mut bodies: [&mut dyn Body; 2] = [&mut pb, &mut a];
            resolve_collisions(&mut bodies[..], &t, &mut Vec::new());
            a.write_pos(&t);
            time += DT;
        }
        assert!(time < 1.5, "{kind}: 2 m off the wall after {time:.2} s");
    }
}

#[test]
fn kinematic_car_walls_spin_decay_and_velocity_round_trips() {
    let t = straight_track(1000.0, "freeway");
    let mut c = Kinematic::new(make_vehicle("rival", 1400.0));
    c.s = 100.0;
    c.lat = 0.0;
    c.speed = 20.0;
    c.frame(&t);
    c.write_pos(&t);
    // Velocity in world space and back.
    let (vx, vz) = c.velocity();
    assert!(
        (vx - 20.0).abs() < 1e-6 && vz.abs() < 1e-6,
        "moving along +x"
    );
    c.set_velocity(15.0, 2.0);
    assert!((c.speed - 15.0).abs() < 1e-6 && (c.lat_vel - 2.0).abs() < 1e-6);
    // Shoved sideways hard: the wall stops it and it bounces back off.
    c.lat_vel = 30.0;
    for _ in 0..60 {
        c.advance(&t, DT);
    }
    let lim = t.wall_r[100] as f64 - c.half_w() - 0.15;
    assert!(c.lat <= lim + 1e-6, "lat {:.2} ≤ {lim:.2}", c.lat);
    assert!(c.lat_vel <= 0.0, "bounced off the wall");
    // A spin decays back to straight.
    c.add_spin(3.0);
    assert!(c.stunned > 0.0);
    for _ in 0..600 {
        c.advance(&t, DT);
    }
    assert!(
        c.spin.abs() < 0.05 && c.stunned == 0.0,
        "spin {:.3}",
        c.spin
    );
    // translate() moves it in world space.
    c.frame(&t);
    let (s, lat) = (c.s, c.lat);
    c.translate(&t, 3.0, -1.0);
    assert!((c.s - s - 3.0).abs() < 1e-6 && (c.lat - lat + 1.0).abs() < 1e-6);
    assert!(
        (c.v.x - t.point_at(c.s, c.lat).x).abs() < 1e-3,
        "the body follows"
    );
    // Oncoming cars face the other way.
    c.dir = -1;
    c.lat_vel = 0.0;
    c.write_pos(&t);
    assert!(
        (c.v.yaw.abs() - PI).abs() < 0.05,
        "oncoming yaw {:.2}",
        c.v.yaw
    );
}

#[test]
fn kinematic_car_stays_on_a_point_to_point_road_and_wraps_round_a_loop() {
    let t = straight_track(500.0, "freeway");
    let mut c = Kinematic::new(make_vehicle("rival", 1400.0));
    c.s = 480.0;
    c.speed = 30.0;
    for _ in 0..120 {
        c.advance(&t, DT);
    }
    assert!(c.s <= t.road_end() - 1.0, "clamped at the end (s {})", c.s);
    let lp = Track::new(&common::level("cruise")).unwrap();
    let mut d = Kinematic::new(make_vehicle("rival", 1400.0));
    d.s = lp.length - 5.0;
    d.speed = 30.0;
    for _ in 0..60 {
        d.advance(&lp, DT);
    }
    assert!(d.s >= 0.0 && d.s < 40.0, "wrapped to {:.1}", d.s);
}

/// From `seaside.test.js` (DECISIONS D56): a rival a lap ahead but just
/// behind the player on the road is leading by miles, so it must not get
/// the catch-up boost.
#[test]
fn seaside_rivals_rubber_band_on_race_progress_not_on_lap_position() {
    let t = Track::new(&common::level("seaside")).unwrap();
    let mut rng = Mulberry32::new(1);
    let mut mk = || {
        let opts = AiOpts {
            skill: Some(0.97),
            power: Some(520.0),
            ..AiOpts::default()
        };
        let mut a = AiDriver::new(make_vehicle("rival", 1400.0), opts, &mut rng);
        a.k.s = 1000.0;
        a.k.lat = 0.0;
        a.k.speed = 30.0;
        a.write_pos(&t);
        a
    };
    let (mut ahead, mut level) = (mk(), mk());
    ahead.prog = Some(1000.0 + t.n as f64);
    level.prog = Some(1000.0);
    let mut rng = Mulberry32::new(2);
    for _ in 0..240 {
        for a in [&mut ahead, &mut level] {
            let views = [a.view()];
            let ctx = AiCtx {
                cars: &views,
                me: Some(0),
                player_s: 1300.0,
                player_prog: Some(1300.0),
                started: true,
                time: 10.0,
            };
            a.update(&t, DT, &ctx, &mut rng);
        }
    }
    // Same road, same start: the one far ahead in the race eases off.
    assert_eq!(
        ahead.prog,
        Some(1000.0 + t.n as f64),
        "progress is the race's to count"
    );
    assert!(
        level.k.s - ahead.k.s > 5.0,
        "leader eased off: {:.0} vs {:.0}",
        ahead.k.s,
        level.k.s
    );
}
