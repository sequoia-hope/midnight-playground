//! vehicles/Traffic.js and Collisions.js. Traffic is the civilian cars kept
//! around the player: they spawn ahead where a level's rules allow, keep to
//! lanes, stay off the grid and past-the-finish stretch, and give way. The
//! collisions are the car-to-car contacts: separating the cars, the impulse,
//! the spin and the events Race reacts to. Port of
//! `test/unit/traffic.test.js`, same assertions.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use common::{make_vehicle, straight_level, straight_track};
use mr_math::Mulberry32;
use mr_sim::body::{AgentView, Body, PhysicsBody};
use mr_sim::collisions::resolve_collisions;
use mr_sim::kinematic::Kinematic;
use mr_sim::traffic::{Agent, Traffic, TrafficCar, activate};
use mr_track::{Track, TrafficRule};

const DT: f64 = 1.0 / 30.0;

/// A stand-in for the player: a body driving down the road at `speed`.
fn player(s: f64, speed: f64) -> AgentView {
    AgentView {
        s,
        lat: 0.0,
        dir: 1,
        half_w: 0.95,
        half_l: 2.3,
        speed_along: speed,
        gap_lat: None,
        ..AgentView::default()
    }
}

#[test]
fn traffic_spawns_ahead_keeps_to_the_road_and_to_its_rules() {
    for l in mr_levels::levels() {
        let level = common::level(l.id);
        let t = Track::new(&level).unwrap();
        let mut rng = Mulberry32::new(99);
        let mut traffic = Traffic::new(&level, None, 22, &mut rng);
        // Every kind a rule can ask for has cars in the pool.
        for r in &level.traffic {
            for (k, _) in &r.mix {
                assert!(
                    traffic.pool_of(k).is_some_and(|p| !p.is_empty()),
                    "{}: pool has {k}",
                    l.id
                );
            }
        }
        let mut p = player(t.start_s, 35.0);
        let (mut seen, mut max_active) = (0, 0);
        let mut spawned_at: Vec<Option<f64>> = vec![None; traffic.cars.len()];
        let last = if t.is_loop {
            t.length * 0.9
        } else {
            t.finish_s - 50.0
        };
        while p.s < last {
            p.s += p.speed_along * DT;
            let mut agents = vec![Agent::Other(p)];
            agents.extend(
                traffic
                    .cars
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.active)
                    .map(|(i, _)| Agent::Traffic(i)),
            );
            traffic.update(&t, DT, p.s, &agents, 0.0, p.s, &mut rng);
            let active: Vec<usize> = (0..traffic.cars.len())
                .filter(|&i| traffic.cars[i].active)
                .collect();
            max_active = max_active.max(
                active
                    .iter()
                    .filter(|&&i| !traffic.cars[i].opposite)
                    .count(),
            );
            for &i in &active {
                let c: &TrafficCar = &traffic.cars[i];
                if spawned_at[i].is_none() {
                    spawned_at[i] = Some(c.k.s);
                    seen += 1;
                }
                for (k, x) in [
                    ("x", c.k.v.x),
                    ("y", c.k.v.y),
                    ("z", c.k.v.z),
                    ("yaw", c.k.v.yaw),
                    ("speed", c.k.speed),
                ] {
                    assert!(x.is_finite(), "{}.{k} not finite", c.kind_name);
                }
                if c.k.free_lat {
                    continue;
                }
                let f = t.frame(c.k.s);
                assert!(
                    c.k.lat + c.k.half_w() <= f.wall_r + 0.01
                        && -c.k.lat + c.k.half_w() <= f.wall_l + 0.01,
                    "{}: {} inside the walls at s={:.0} (lat {:.2})",
                    l.id,
                    c.kind_name,
                    c.k.s,
                    c.k.lat
                );
                assert!(
                    c.k.lat.abs() < f.hw,
                    "{}: {} on the tarmac at s={:.0}",
                    l.id,
                    c.kind_name,
                    c.k.s
                );
                assert!(
                    c.k.speed >= 0.0 && c.k.speed < 45.0,
                    "{} speed {}",
                    c.kind_name,
                    c.k.speed
                );
            }
            for (i, c) in traffic.cars.iter().enumerate() {
                if !c.active {
                    spawned_at[i] = None;
                }
            }
        }
        // A closed circuit (every zone's mix empty) has no traffic at all.
        if level.traffic.iter().all(|r| r.mix.is_empty()) {
            assert_eq!(seen, 0, "no traffic on a closed circuit");
        } else {
            assert!(seen > 10, "{}: {seen} cars came and went", l.id);
        }
        assert!(
            max_active <= 22,
            "at most count cars at once ({max_active})"
        );
    }
}

#[test]
fn traffic_keeps_clear_of_the_grid_and_the_stretch_past_the_finish() {
    let level = common::level("sierra");
    let t = Track::new(&level).unwrap();
    let mut rng = Mulberry32::new(99);
    let mut traffic = Traffic::new(&level, None, 22, &mut rng);
    let mut s = 0.0;
    while s < t.start_s + 250.0 {
        assert!(!traffic.spawn(&t, s, &mut rng), "no spawn at {s}");
        s += 10.0;
    }
    let mut s = t.finish_s + 110.0;
    while s < t.length {
        assert!(!traffic.spawn(&t, s, &mut rng), "no spawn at {s}");
        s += 10.0;
    }
    let mut any = false;
    let mut s = 1000.0;
    while s < 1400.0 && !any {
        any = traffic.spawn(&t, s, &mut rng);
        s += 7.0;
    }
    assert!(any, "but spawns out on the route");
}

#[test]
fn the_desert_lake_bed_has_no_traffic() {
    let level = common::level("desert");
    let t = Track::new(&level).unwrap();
    let mut rng = Mulberry32::new(99);
    let mut traffic = Traffic::new(&level, None, 22, &mut rng);
    let lake = &t.zones[2];
    let mut s = lake.s0 + 5.0;
    while s < lake.s1.min(t.finish_s) {
        assert!(!traffic.spawn(&t, s, &mut rng));
        s += 13.0;
    }
}

#[test]
fn a_traffic_car_slows_for_a_slower_car_ahead_in_its_lane() {
    let mut level = straight_level(3000.0, "freeway");
    level.traffic = vec![TrafficRule {
        gap: [100.0, 200.0],
        mix: vec![("sedan", 1.0)],
        oncoming: 0.0,
        speed: [25.0, 25.0],
        opposite: None,
    }];
    let t = Track::new(&level).unwrap();
    let mut rng = Mulberry32::new(99);
    let mut traffic = Traffic::new(&level, None, 22, &mut rng);
    let ci = traffic.pool_of("sedan").unwrap()[0];
    activate(&mut traffic.cars[ci], 500.0, 3.0, 1);
    traffic.cars[ci].cruise = 25.0;
    traffic.cars[ci].k.speed = 25.0;
    traffic.cars[ci].k.write_pos(&t);
    let mut slow = AgentView {
        s: 530.0,
        lat: 3.0,
        dir: 1,
        speed_along: 8.0,
        half_w: 0.95,
        half_l: 2.3,
        gap_lat: None,
        ..AgentView::default()
    };
    traffic.max_active = 1;
    for _ in 0..150 {
        slow.s += 8.0 * DT;
        traffic.update(
            &t,
            DT,
            500.0,
            &[Agent::Other(slow), Agent::Traffic(ci)],
            0.0,
            500.0,
            &mut rng,
        );
    }
    let car = &traffic.cars[ci];
    assert!(car.k.speed < 12.0, "slowed to {:.1} m/s", car.k.speed);
    assert!(slow.s - car.k.s > 5.0, "no rear-ending");
    assert_eq!(car.k.v.brake_light, 1.0, "brake lights on");
}

fn kin(t: &Track, s: f64, lat: f64, speed: f64) -> Kinematic {
    let mut c = Kinematic::new(make_vehicle("rival", 1400.0));
    c.s = s;
    c.lat = lat;
    c.speed = speed;
    c.frame(t);
    c.write_pos(t);
    c
}

#[test]
fn collisions_push_overlapping_cars_apart_and_trade_momentum() {
    let t = straight_track(1000.0, "freeway");
    let mut cars = [kin(&t, 100.0, 0.0, 30.0), kin(&t, 103.5, 0.0, 10.0)];
    let p0 = cars[0].speed + cars[1].speed;
    let mut events = Vec::new();
    resolve_collisions(&mut cars[..], &t, &mut events);
    assert_eq!(events.len(), 1);
    let e = events[0];
    assert!(
        e.strength > 0.3 && e.strength <= 1.0,
        "strength {}",
        e.strength
    );
    let [a, b] = &mut cars;
    assert!(
        a.speed < 30.0 && b.speed > 10.0,
        "rear-ended: {:.1}, {:.1}",
        a.speed,
        b.speed
    );
    assert!(
        (a.speed + b.speed - p0).abs() < 1e-6,
        "equal masses: momentum is conserved"
    );
    assert!(b.s - a.s > 3.5, "pushed apart");
    // Separated: a second pass finds no contact.
    a.write_pos(&t);
    b.write_pos(&t);
    let mut again = Vec::new();
    resolve_collisions(&mut cars[..], &t, &mut again);
    assert_eq!(again.len(), 0);
}

/// A PIT: nudging the car ahead on one rear corner shoves its tail aside,
/// so its nose swings the other way (and the pusher's nose with it).
#[test]
fn an_off_centre_hit_spins_the_cars_mirror_symmetrically() {
    let t = straight_track(1000.0, "freeway");
    let hit = |side: f64| {
        let mut cars = [
            kin(&t, 100.0, -0.9 * side, 30.0),
            kin(&t, 103.0, 0.9 * side, 5.0),
        ];
        let mut ev = Vec::new();
        resolve_collisions(&mut cars[..], &t, &mut ev);
        assert_eq!(ev.len(), 1, "they touch");
        cars
    };
    let (r, l) = (hit(1.0), hit(-1.0));
    // b is to the right and hit on its left rear: shoved right, nose left.
    assert!(r[1].lat_vel > 1.0, "shoved right ({:.2})", r[1].lat_vel);
    assert!(
        r[1].spin_rate < -0.1,
        "nose swings left ({:.2})",
        r[1].spin_rate
    );
    assert!(r[1].stunned > 0.0, "stunned");
    assert!(
        (r[1].spin_rate + l[1].spin_rate).abs() < 1e-9
            && (r[0].spin_rate + l[0].spin_rate).abs() < 1e-9,
        "mirror image"
    );
}

/// Two cars leaning on each other (door to door, or one pinned to a wall)
/// touch every frame at a crawl. That pushes them apart but mustn't spin
/// them, or it drowns out the steering.
#[test]
fn leaning_on_another_car_pushes_it_but_does_not_spin_it() {
    let t = straight_track(1000.0, "freeway");
    let mut a = kin(&t, 100.0, 0.0, 30.0);
    a.lat_vel = 0.6;
    a.frame(&t);
    a.write_pos(&t);
    let mut cars = [a, kin(&t, 102.0, 1.9, 30.0)];
    let mut ev = Vec::new();
    resolve_collisions(&mut cars[..], &t, &mut ev);
    assert_eq!(ev.len(), 1, "they touch");
    assert!(
        cars[1].lat_vel > 0.1,
        "pushed aside ({:.2})",
        cars[1].lat_vel
    );
    assert_eq!(cars[0].spin_rate, 0.0);
    assert_eq!(cars[1].spin_rate, 0.0);
}

#[test]
fn collisions_skip_far_apart_cars_a_car_flying_over_and_traffic_among_itself() {
    let t = straight_track(1000.0, "freeway");
    let mk = |s: f64, lat: f64| kin(&t, s, lat, 20.0);
    let mut ev = Vec::new();
    resolve_collisions(&mut [mk(100.0, 0.0), mk(120.0, 0.0)][..], &t, &mut ev);
    assert_eq!(ev.len(), 0, "far apart");
    let (low, mut high) = (mk(200.0, 0.0), mk(201.0, 0.0));
    high.v.y += 3.0;
    resolve_collisions(&mut [low, high][..], &t, &mut ev);
    assert_eq!(ev.len(), 0, "one over the other");
    // Traffic cars (kinematicOnly) pass through each other.
    let tc = |k: Kinematic| TrafficCar {
        k,
        kind_name: "sedan",
        active: true,
        cruise: 20.0,
        lane_lat: 0.0,
        crashed: 0.0,
        opposite: false,
        lane: None,
    };
    let mut t1 = mk(300.0, 0.0);
    t1.speed = 30.0; // closing on t2
    let mut pair = [tc(t1), tc(mk(301.0, 0.0))];
    resolve_collisions(&mut pair[..], &t, &mut ev);
    assert_eq!(ev.len(), 0, "traffic cars pass through each other");
    assert_eq!(pair[0].k.s, 300.0, "untouched");
}

#[test]
fn the_player_body_collides_like_any_other_car() {
    let t = straight_track(1000.0, "freeway");
    let mut v = make_vehicle("sports", 1350.0);
    v.place(&t, 100.0, 0.0, 0.0);
    v.vx = 30.0;
    {
        let body = PhysicsBody { v: &mut v };
        assert_eq!(body.view().s, 100.0);
        assert_eq!(body.view().speed_along, 0.0, "speed comes from the vehicle");
    }
    let mut k = kin(&t, 103.6, 0.3, 15.0);
    let mut ev = Vec::new();
    {
        let mut pb = PhysicsBody { v: &mut v };
        let mut bodies: [&mut dyn Body; 2] = [&mut pb, &mut k];
        resolve_collisions(&mut bodies[..], &t, &mut ev);
    }
    assert_eq!(ev.len(), 1);
    assert!(v.vx < 30.0, "the player lost speed ({:.1})", v.vx);
    assert!(k.speed > 15.0, "and shoved the other car");
    assert!(v.yaw_rate != 0.0, "and got a spin");
}
