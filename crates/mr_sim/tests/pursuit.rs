//! game/Pursuit.js and vehicles/PoliceDriver.js: Hot Pursuit on its own.
//! Line of sight, heat, the pursuit/cooldown/escape states, busts and the
//! penalty hold, spawning, and whole sprint levels driven with police on.
//! Port of `test/unit/pursuit.test.js`, same assertions. As in the JS test,
//! the player is an AIDriver standing in for the physics car; the police
//! cars have the game's dimensions (the JS test gives them a generic body).

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use common::{make_vehicle, straight_level};
use mr_math::{Mulberry32, Rng};
use mr_sim::ai::{AiCtx, AiDriver, AiOpts};
use mr_sim::body::{AgentView, Body, BodyId};
use mr_sim::collisions::resolve_collisions;
use mr_sim::field::pagent_id;
use mr_sim::physics::car_spec;
use mr_sim::police::Mode;
use mr_sim::pursuit::{
    EVADE_TIME, HEAT, PAgent, Pursuit, PursuitEvent, PursuitOpts, RacerBody, Racers, State,
    bust_penalty, top_speed,
};
use mr_track::{Level, Police, Track, TrafficRule, seg};

const DT: f64 = 1.0 / 60.0;

/// The racers: the stand-in player and the rivals.
struct TestRacers<'a> {
    p: &'a mut AiDriver,
    /// The stand-in's `phys.spiked`.
    p_spiked: &'a mut f64,
    rivals: &'a mut [AiDriver],
}

fn body_of(a: &AiDriver) -> RacerBody {
    RacerBody {
        s: a.k.s,
        lat: a.k.lat,
        speed_along: a.k.speed,
        half_w: a.k.v.half_w,
        half_l: a.k.v.half_l,
        x: a.k.v.x,
        z: a.k.v.z,
        vx: a.k.v.vx,
        vz: a.k.v.vz,
    }
}

impl Racers for TestRacers<'_> {
    fn body(&self, id: BodyId) -> RacerBody {
        match id {
            BodyId::Player(_) => body_of(self.p),
            BodyId::Rival(i) => body_of(&self.rivals[i]),
            _ => unreachable!(),
        }
    }
    fn ai_finished(&self, id: BodyId) -> bool {
        matches!(id, BodyId::Rival(i) if self.rivals[i].finished)
    }
    fn hold_ai(&mut self, id: BodyId, seconds: f64, lat: f64) {
        if let BodyId::Rival(i) = id {
            self.rivals[i].hold = seconds;
            self.rivals[i].hold_lat = Some(lat);
        }
    }
    fn spike(&mut self, id: BodyId) {
        match id {
            BodyId::Player(_) => *self.p_spiked = 10.0,
            BodyId::Rival(i) => self.rivals[i].spiked = 10.0,
            _ => {}
        }
    }
}

/// A racer body for the player: an AIDriver standing in for the physics car.
fn player_body(t: &Track, s: f64, lat: f64) -> AiDriver {
    let mut rng = Mulberry32::new(1);
    let opts = AiOpts {
        skill: Some(0.95),
        name: "You",
        ..AiOpts::default()
    };
    let mut p = AiDriver::new(make_vehicle("sports", 1350.0), opts, &mut rng);
    p.k.s = s;
    p.k.lat = lat;
    p.write_pos(t);
    p
}

fn level_with(police: Police, traffic: Vec<TrafficRule>) -> Level {
    let mut l = straight_level(3000.0, "freeway");
    l.police = Some(police);
    l.traffic = traffic;
    l
}

fn no_police() -> Police {
    Police {
        heat_cap: Vec::new(),
        los_open_ground: Vec::new(),
    }
}

fn pursuit(t: &Track, level: &Level, heat: f64, rng: &mut dyn Rng) -> Pursuit {
    let opts = PursuitOpts {
        heat,
        max_units: 6.0,
        player_top: 72.0,
        flash: true,
    };
    let mut police = Mulberry32::new(17);
    let mut pu = Pursuit::new(t, level, opts, rng, &mut police);
    pu.set_racers(vec![(BodyId::Player(0), true, false, "You")]);
    pu
}

/// `pu.update(DT, { cars: [P, ...pu.bodies()], time })`.
fn update(
    pu: &mut Pursuit,
    t: &Track,
    p: &mut AiDriver,
    spiked: &mut f64,
    time: f64,
    rng: &mut dyn Rng,
) {
    let mut cars = vec![PAgent::Other(AgentView {
        id: BodyId::Player(0),
        ..p.view()
    })];
    cars.extend(pu.bodies());
    let mut racers = TestRacers {
        p,
        p_spiked: spiked,
        rivals: &mut [],
    };
    pu.update(t, DT, &cars, &mut racers, time, true, rng);
}

/// `resolveCollisions([P, ...pu.bodies()], hits)`.
fn collide(pu: &mut Pursuit, t: &Track, p: &mut AiDriver) -> usize {
    let bodies = pu.bodies();
    let n = pu.units.len();
    let Pursuit {
        units,
        block_cars,
        sawhorses,
        ..
    } = pu;
    let mut police: Vec<Option<&mut dyn Body>> = units
        .iter_mut()
        .chain(block_cars.iter_mut())
        .map(|u| Some(u as &mut dyn Body))
        .collect();
    let mut saws: Vec<Option<&mut dyn Body>> = sawhorses
        .iter_mut()
        .map(|b| Some(b as &mut dyn Body))
        .collect();
    let mut list: Vec<&mut dyn Body> = vec![p];
    for b in &bodies {
        match *b {
            PAgent::Police(i) => list.push(police[i].take().unwrap()),
            PAgent::Sawhorse(i) => list.push(saws[i].take().unwrap()),
            PAgent::Other(_) => {}
        }
    }
    let _ = n;
    let mut hits = Vec::new();
    resolve_collisions(&mut list[..], t, &mut hits);
    hits.len()
}

/// A 90° corner between two straights.
fn corner_track() -> (Track, Level) {
    let mut l = straight_level(800.0, "valley");
    l.route = mr_track::Route::Segments(vec![
        seg(800.0, 0.0, 0.0).zone(0).road("valley"),
        seg(120.0, 90.0, 0.0),
        seg(1500.0, 0.0, 0.0),
    ]);
    l.police = Some(no_police());
    (Track::new(&l).unwrap(), l)
}

#[test]
fn top_speed_is_where_the_engine_meets_the_drag() {
    let v = top_speed(&car_spec("sports").unwrap());
    assert!(v > 65.0 && v < 80.0, "sports top {v:.1} m/s");
    assert!(top_speed(&car_spec("rally").unwrap()) <= 71.5);
}

#[test]
fn line_of_sight_range_corners_and_open_ground() {
    let (t, l) = corner_track();
    let mut rng = Mulberry32::new(1);
    let pu = pursuit(&t, &l, 1.0, &mut rng);
    assert!(pu.can_see(&t, 100.0, 350.0), "straight, 250 m");
    assert!(!pu.can_see(&t, 100.0, 450.0), "beyond 300 m");
    assert!(!pu.can_see(&t, 700.0, 960.0), "round the 90° corner");
    assert!(pu.can_see(&t, 800.0, 850.0), "half way into the corner");
    let mut lo = l.clone();
    lo.police = Some(Police {
        heat_cap: Vec::new(),
        los_open_ground: vec![true],
    });
    let open = pursuit(&t, &lo, 1.0, &mut rng);
    assert!(open.can_see(&t, 700.0, 960.0), "open ground sees round it");
}

#[test]
fn line_of_sight_a_tunnel_hides_you_unless_both_are_in_it() {
    let sierra = common::level("sierra");
    let t = Track::new(&sierra).unwrap();
    let mut rng = Mulberry32::new(1);
    let pu = pursuit(&t, &sierra, 1.0, &mut rng);
    let g = t
        .tags
        .iter()
        .find(|x| x.tag == "tunnel")
        .expect("sierra has a tunnel");
    assert!(
        !pu.can_see(&t, g.s0 - 60.0, g.s0 + 60.0),
        "outside looking in"
    );
    assert!(pu.can_see(&t, g.s0 + 20.0, g.s0 + 120.0), "both inside");
}

#[test]
fn a_parked_unit_spots_a_speeding_racer_and_starts_a_pursuit() {
    let mut l = straight_level(4000.0, "valley");
    l.police = Some(no_police());
    l.traffic = vec![TrafficRule {
        gap: [100.0, 200.0],
        mix: Vec::new(),
        oncoming: 0.0,
        speed: [14.0, 18.0],
        opposite: None,
    }];
    let t = Track::new(&l).unwrap();
    let mut rng = Mulberry32::new(3);
    let mut pu = pursuit(&t, &l, 1.0, &mut rng);
    let mut p = player_body(&t, 300.0, 0.0);
    let mut spiked = 0.0;
    p.k.speed = 40.0;
    let mut started = false;
    let mut i = 0;
    while i < 60 * 40 && !started {
        p.k.s += p.k.speed * DT;
        p.write_pos(&t);
        update(&mut pu, &t, &mut p, &mut spiked, i as f64 * DT, &mut rng);
        started = pu
            .events
            .iter()
            .any(|e| matches!(e, PursuitEvent::Pursuit { .. }));
        i += 1;
    }
    assert!(started, "pursuit started");
    assert_eq!(pu.state, State::Pursuit);
    assert!(
        pu.units
            .iter()
            .any(|u| u.active && u.mode == Mode::Chase && u.target == Some(0))
    );
}

#[test]
fn stopping_with_two_units_beside_you_is_a_bust_then_a_hold_and_a_release() {
    let l = level_with(no_police(), Vec::new());
    let t = Track::new(&l).unwrap();
    let mut rng = Mulberry32::new(3);
    let mut pu = pursuit(&t, &l, 2.0, &mut rng);
    let mut p = player_body(&t, 800.0, 0.0);
    let mut spiked = 0.0;
    p.k.speed = 0.0;
    pu.state = State::Pursuit;
    pu.activate(&t, 0, 793.0, 0.0, 0.0, Mode::Chase, 1);
    pu.units[0].target = Some(0);
    pu.activate(&t, 1, 800.0, 3.0, 0.0, Mode::Chase, 1);
    pu.units[1].target = Some(0);
    let (mut bust_at, mut released) = (None, None);
    for i in 0..60 * 30 {
        let time = i as f64 * DT;
        p.write_pos(&t);
        update(&mut pu, &t, &mut p, &mut spiked, time, &mut rng);
        collide(&mut pu, &t, &mut p);
        for e in &pu.events {
            if matches!(e, PursuitEvent::Busted { .. }) {
                bust_at = Some(time);
            }
            if let PursuitEvent::Release { spot, .. } = e {
                released = Some((time, spot.unwrap()));
            }
        }
        pu.events.clear();
        if released.is_some() {
            break;
        }
    }
    let bust_at = bust_at.expect("busted");
    assert!(bust_at < 5.0, "busted at {bust_at}");
    assert_eq!(pu.busts, 1);
    let (rt, spot) = released.expect("released");
    let held = rt - bust_at;
    assert!((held - bust_penalty(2)).abs() < 0.1, "held {held:.2} s");
    // The release spot is clear of the units that made the arrest.
    for u in &pu.units[..2] {
        if u.active {
            assert!(
                spot.0 > u.k.s + u.k.half_l(),
                "release ahead of unit at {:.1}",
                u.k.s
            );
        }
    }
    assert_eq!(pu.state, State::Patrol, "the pursuit ends with the bust");
    assert!(pu.racers[0].grace > 0.0, "grace period after release");
}

#[test]
fn out_of_sight_round_a_corner_you_escape_in_evade_time() {
    let (t, l) = corner_track();
    for heat in [1.0, 3.0] {
        let mut rng = Mulberry32::new(3);
        let mut pu = pursuit(&t, &l, heat, &mut rng);
        let mut p = player_body(&t, 1050.0, 0.0);
        let mut spiked = 0.0;
        pu.state = State::Pursuit;
        pu.activate(&t, 0, 700.0, 0.0, 0.0, Mode::Chase, 1);
        pu.units[0].target = Some(0);
        let (mut cooldown_at, mut escaped_at) = (None, None);
        let mut i = 0;
        while i < 60 * 40 && escaped_at.is_none() {
            let time = i as f64 * DT;
            // Hold 300+ m of road and the corner between you.
            p.k.s = pu.units[0].k.s + 350.0;
            p.write_pos(&t);
            pu.units[0].k.speed = pu.units[0].k.speed.min(5.0);
            update(&mut pu, &t, &mut p, &mut spiked, time, &mut rng);
            for e in &pu.events {
                if matches!(e, PursuitEvent::Cooldown) {
                    cooldown_at = Some(time);
                }
                if matches!(e, PursuitEvent::Escaped { .. }) {
                    escaped_at = Some(time);
                }
            }
            pu.events.clear();
            i += 1;
        }
        let took = escaped_at.expect("escaped") - cooldown_at.expect("cooldown");
        assert!(
            (took - EVADE_TIME[pu.heat as usize]).abs() < 1.0,
            "heat {heat}: escaped after {took:.2} s"
        );
    }
}

#[test]
fn heat_rises_with_the_meter_and_stops_at_the_zone_cap() {
    let l = level_with(
        Police {
            heat_cap: vec![3],
            los_open_ground: Vec::new(),
        },
        Vec::new(),
    );
    let t = Track::new(&l).unwrap();
    let mut rng = Mulberry32::new(3);
    let mut pu = pursuit(&t, &l, 1.0, &mut rng);
    let mut p = player_body(&t, 500.0, 0.0);
    let mut spiked = 0.0;
    let racers = TestRacers {
        p: &mut p,
        p_spiked: &mut spiked,
        rivals: &mut [],
    };
    pu.state = State::Pursuit;
    pu.heat_up(&t, &racers, 1.05);
    assert_eq!(pu.heat, 2);
    pu.heat_up(&t, &racers, 5.0);
    assert_eq!(pu.heat, 3, "capped");
    assert!(pu.heat_meter < 1.0);
    pu.state = State::Patrol;
    let before = pu.heat_meter;
    pu.heat_up(&t, &racers, 0.5);
    assert_eq!(pu.heat_meter, before, "no heat out of a pursuit");
}

#[test]
fn reinforcements_never_more_active_units_than_the_heat_allows() {
    let mut l = straight_level(6000.0, "freeway");
    l.police = Some(no_police());
    let t = Track::new(&l).unwrap();
    for heat in [1.0, 3.0, 5.0] {
        let mut rng = Mulberry32::new(3);
        let mut pu = pursuit(&t, &l, heat, &mut rng);
        let mut p = player_body(&t, 800.0, 0.0);
        let mut spiked = 0.0;
        pu.state = State::Pursuit;
        pu.activate(&t, 0, 760.0, 0.0, 30.0, Mode::Chase, 1);
        pu.units[0].target = Some(0);
        p.k.speed = 30.0;
        let mut most = 0;
        for i in 0..60 * 60 {
            p.k.s += p.k.speed * DT;
            p.write_pos(&t);
            update(&mut pu, &t, &mut p, &mut spiked, i as f64 * DT, &mut rng);
            most = most.max(pu.active_count());
        }
        assert!(
            most <= HEAT[pu.heat as usize].units,
            "heat {heat}: {most} units"
        );
        assert!(
            most >= 2.min(HEAT[heat as usize].units),
            "heat {heat}: reinforcements came ({most})"
        );
    }
}

#[test]
fn a_spike_strip_shreds_the_tyres_of_a_racer_that_crosses_it() {
    let l = level_with(no_police(), Vec::new());
    let t = Track::new(&l).unwrap();
    let mut rng = Mulberry32::new(3);
    let mut pu = pursuit(&t, &l, 4.0, &mut rng);
    let mut p = player_body(&t, 500.0, 0.0);
    let mut spiked = 0.0;
    pu.state = State::Pursuit;
    {
        let racers = TestRacers {
            p: &mut p,
            p_spiked: &mut spiked,
            rivals: &mut [],
        };
        pu.place_spikes(&t, &racers, 600.0);
    }
    let sp = pu.spikes.clone().unwrap();
    p.k.lat = (sp.lat0 + sp.lat1) / 2.0;
    p.k.speed = 40.0;
    let mut seen = false;
    for i in 0..60 * 4 {
        p.k.s += p.k.speed * DT;
        p.write_pos(&t);
        update(&mut pu, &t, &mut p, &mut spiked, i as f64 * DT, &mut rng);
        seen |= pu
            .events
            .iter()
            .any(|e| matches!(e, PursuitEvent::Spiked { player: true, .. }));
    }
    assert!(spiked > 0.0);
    assert!(seen);
}

#[test]
fn a_roadblock_spans_the_road_with_a_gap_you_fit_through() {
    for heat in [3.0, 5.0] {
        let l = level_with(no_police(), Vec::new());
        let t = Track::new(&l).unwrap();
        let mut rng = Mulberry32::new(3);
        let mut pu = pursuit(&t, &l, heat, &mut rng);
        pu.state = State::Pursuit;
        pu.place_roadblock(&t, 1000.0, &mut rng);
        let rb = pu.roadblock.clone().unwrap();
        assert!(rb.cars.len() >= 2);
        let f = t.frame(1000.0);
        assert!(
            rb.gap_lat > -f.wall_l && rb.gap_lat < f.wall_r,
            "gap on the road"
        );
        for &c in &rb.cars {
            let c = &pu.block_cars[c];
            assert!(
                (c.k.lat - rb.gap_lat).abs() > c.k.half_w() + 1.0,
                "car at {:.2} clear of the gap {:.2}",
                c.k.lat,
                rb.gap_lat
            );
            assert!(c.k.lat.abs() < f.wall_l.max(f.wall_r), "inside the walls");
        }
        assert_eq!(
            pu.sawhorses.iter().all(|b| b.active),
            heat >= 5.0,
            "heavy roadblocks close the gap with barriers"
        );
    }
}

/// Whole sprint levels with the police on: rivals as the pack, a stand-in
/// player driving the racing line, collisions resolved as the Race does.
#[test]
fn a_race_with_the_police_on_runs_clean() {
    for l in mr_levels::levels()
        .into_iter()
        .filter(|l| l.police.is_some())
    {
        let t = Track::new(&l).unwrap();
        let mut ai_rng = Mulberry32::new(11);
        let mut ais: Vec<AiDriver> = l
            .rivals
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let opts = AiOpts {
                    skill: Some(r.skill),
                    name: r.name,
                    power: Some(r.power),
                    bias: Some((if i % 2 == 1 { 1.0 } else { -1.0 }) * 0.6),
                    line_factor: None,
                };
                let mut ai = AiDriver::new(make_vehicle("rival", 1400.0), opts, &mut ai_rng);
                ai.k.s = t.start_s - 5.0 - (i / 2) as f64 * 10.0;
                ai.k.lat = if i % 2 == 1 { 2.4 } else { -2.4 };
                ai.write_pos(&t);
                ai
            })
            .collect();
        let mut p = player_body(&t, t.start_s - 25.0, -2.4);
        p.skill = 0.97;
        let mut spiked = 0.0;
        let mut rng = Mulberry32::new(5);
        let opts = PursuitOpts {
            heat: 1.0,
            max_units: 6.0,
            player_top: 72.0,
            flash: true,
        };
        let mut police_rng = Mulberry32::new(17);
        let mut pu = Pursuit::new(&t, &l, opts, &mut rng, &mut police_rng);
        let mut list = vec![(BodyId::Player(0), true, false, "You")];
        list.extend(
            ais.iter()
                .enumerate()
                .map(|(i, a)| (BodyId::Rival(i), false, true, a.name)),
        );
        pu.set_racers(list);
        let mut seen_pursuit = false;
        let (mut time, mut max_heat, mut worst_wall) = (0.0, 1, f64::NEG_INFINITY);
        while time < 420.0 && !p.finished {
            let mut cars: Vec<PAgent> = vec![PAgent::Other(AgentView {
                id: BodyId::Player(0),
                ..p.view()
            })];
            cars.extend(ais.iter().enumerate().map(|(i, a)| {
                PAgent::Other(AgentView {
                    id: BodyId::Rival(i),
                    ..a.view()
                })
            }));
            cars.extend(pu.bodies());
            let views: Vec<AgentView> = cars.iter().map(|c| pu.view_of(c)).collect();
            let ctx = |me| AiCtx {
                cars: &views,
                me: Some(me),
                player_s: p.k.s,
                player_prog: None,
                started: true,
                time,
            };
            let c0 = ctx(0);
            p.update(&t, DT, &c0, &mut ai_rng);
            for (i, a) in ais.iter_mut().enumerate() {
                let views: Vec<AgentView> = {
                    let mut v = views.clone();
                    v[0] = AgentView {
                        id: BodyId::Player(0),
                        ..p.view()
                    };
                    v
                };
                let c = AiCtx {
                    cars: &views,
                    me: Some(1 + i),
                    player_s: p.k.s,
                    player_prog: None,
                    started: true,
                    time,
                };
                a.update(&t, DT, &c, &mut ai_rng);
            }
            // The pursuit reads the cars as they now are.
            let mut cars: Vec<PAgent> = vec![PAgent::Other(AgentView {
                id: BodyId::Player(0),
                ..p.view()
            })];
            cars.extend(ais.iter().enumerate().map(|(i, a)| {
                PAgent::Other(AgentView {
                    id: BodyId::Rival(i),
                    ..a.view()
                })
            }));
            cars.extend(pu.bodies());
            {
                let mut racers = TestRacers {
                    p: &mut p,
                    p_spiked: &mut spiked,
                    rivals: &mut ais,
                };
                pu.update(&t, DT, &cars, &mut racers, time, true, &mut rng);
            }
            // Collisions over [P, ...ais, ...pu.bodies()], then the pursuit's hit rules.
            let bodies = pu.bodies();
            let mut ids: Vec<BodyId> = vec![BodyId::Player(0)];
            ids.extend((0..ais.len()).map(BodyId::Rival));
            ids.extend(bodies.iter().map(pagent_id));
            let mut hits = Vec::new();
            {
                let Pursuit {
                    units,
                    block_cars,
                    sawhorses,
                    ..
                } = &mut pu;
                let mut police: Vec<Option<&mut dyn Body>> = units
                    .iter_mut()
                    .chain(block_cars.iter_mut())
                    .map(|u| Some(u as &mut dyn Body))
                    .collect();
                let mut saws: Vec<Option<&mut dyn Body>> = sawhorses
                    .iter_mut()
                    .map(|b| Some(b as &mut dyn Body))
                    .collect();
                let mut list: Vec<&mut dyn Body> = vec![&mut p];
                for a in ais.iter_mut() {
                    list.push(a);
                }
                for b in &bodies {
                    match *b {
                        PAgent::Police(i) => list.push(police[i].take().unwrap()),
                        PAgent::Sawhorse(i) => list.push(saws[i].take().unwrap()),
                        PAgent::Other(_) => {}
                    }
                }
                resolve_collisions(&mut list[..], &t, &mut hits);
            }
            for h in hits {
                let (a, b) = (ids[h.a], ids[h.b]);
                let vel = |id: BodyId, p: &AiDriver, ais: &[AiDriver], pu: &Pursuit| match id {
                    BodyId::Player(_) => p.velocity(),
                    BodyId::Rival(i) => ais[i].velocity(),
                    BodyId::Police(i) => pu.police(i).velocity(),
                    BodyId::Sawhorse(i) => pu.sawhorses[i].velocity(),
                    _ => (0.0, 0.0),
                };
                let (va, vb) = (vel(a, &p, &ais, &pu), vel(b, &p, &ais, &pu));
                let plat = p.k.lat;
                let mut racers = TestRacers {
                    p: &mut p,
                    p_spiked: &mut spiked,
                    rivals: &mut ais,
                };
                pu.on_hit(&t, &mut racers, a, b, h.strength, va, vb, plat, &mut rng);
            }
            seen_pursuit |= pu
                .events
                .iter()
                .any(|e| matches!(e, PursuitEvent::Pursuit { .. }));
            pu.events.clear();
            max_heat = max_heat.max(pu.heat);
            for b in pu.bodies() {
                let v = pu.view_of(&b);
                let (vv, police_unit) = match b {
                    PAgent::Police(i) => {
                        (pu.police(i).v().clone(), pu.police(i).mode != Mode::Block)
                    }
                    PAgent::Sawhorse(i) => (pu.sawhorses[i].v().clone(), false),
                    PAgent::Other(_) => continue,
                };
                for (k, x) in [("x", vv.x), ("y", vv.y), ("z", vv.z), ("yaw", vv.yaw)] {
                    assert!(x.is_finite(), "{}: unit {k} = {x} at {time:.1} s", l.id);
                }
                if police_unit {
                    let f = t.frame(v.s);
                    worst_wall = worst_wall
                        .max(v.lat + v.half_w - f.wall_r)
                        .max(-v.lat + v.half_w - f.wall_l);
                }
            }
            time += DT;
        }
        assert!(
            p.finished,
            "{}: the player finished (s {:.0} of {:.0}, {time:.0} s)",
            l.id, p.k.s, t.finish_s
        );
        assert!(seen_pursuit, "{}: a pursuit started", l.id);
        assert!(
            worst_wall < 0.3,
            "{}: units stay inside the walls (worst {worst_wall:.2} m)",
            l.id
        );
        assert!(max_heat <= *pu.heat_cap.iter().max().unwrap());
        assert!(
            pu.units
                .iter()
                .filter(|u| u.active && u.mode == Mode::Chase)
                .count()
                == 0
                || p.finished
        );
    }
}
