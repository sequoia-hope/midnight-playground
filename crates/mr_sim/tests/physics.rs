//! vehicles/CarPhysics.js: the player's driving model on a straight, flat
//! test road for every car in CAR_SPECS. It checks the launch, top speed,
//! braking into reverse, which way steering turns, nitro, the handbrake
//! drift, the countdown lock and the walls, and the README's claims about
//! the cars. Port of `test/unit/physics.test.js`, same assertions; the
//! README table check is dropped (DEVIATIONS.md), its claims are kept.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use std::sync::{Arc, OnceLock};

use mr_math::kernel::hypot;
use mr_sim::dims::Dims;
use mr_sim::input::Input;
use mr_sim::physics::{ANALOG_LOCK, CAR_SPECS, CarPhysics, MOTOR_MAX, PhysEvent, car_spec};
use mr_sim::vehicle::Vehicle;
use mr_track::{Level, Mode, Route, Track, Zone, seg};

const DT: f64 = 1.0 / 60.0;

/// A dead-straight, flat road (x grows along it; +z is to the right):
/// `straightTrack` in test/unit/support/sim.js.
pub fn straight_track(length: f64, road: &'static str) -> Track {
    let level = Level {
        id: "test-straight",
        mode: Mode::Race,
        num: "",
        title: "",
        desc: "",
        laps: None,
        lap_length: None,
        start_height: Some(0.0),
        start_heading: Some(0.0),
        start_x: None,
        start_z: None,
        finish_runoff: Some(180.0),
        elevation_smooth: None,
        route: Route::Segments(vec![seg(length, 0.0, 0.0).zone(0).road(road)]),
        elevation: None,
        ground: None,
        loose_ground: None,
        sea_y: None,
        zones: vec![Zone {
            key: "test",
            name: "TEST",
            sub: "",
            landform: "valley",
            scenery: "",
            color: "",
            blend: None,
            blend_offset: None,
        }],
        sky: Vec::new(),
        sun_azimuth: 0.0,
        moon_dir: None,
        traffic_paint: None,
        traffic: Vec::new(),
        police: None,
        rivals: Vec::new(),
    };
    Track::new(&level).unwrap()
}

fn track() -> Arc<Track> {
    static T: OnceLock<Arc<Track>> = OnceLock::new();
    T.get_or_init(|| Arc::new(straight_track(14000.0, "freeway")))
        .clone()
}

/// A car with no model: the dimensions physics and AI read (`makeVehicle`).
pub fn make_vehicle(kind: &'static str, mass: f64) -> Vehicle {
    let (length, width, wheel_base) = match kind {
        "sports" => (4.47, 1.9, 2.6),
        "muscle" => (4.86, 1.95, 2.8),
        "super" => (4.57, 2.05, 2.7),
        "electric" => (4.74, 1.98, 2.9),
        "rally" => (4.12, 1.9, 2.55),
        _ => (4.6, 1.95, 2.7),
    };
    let dims = Dims {
        length,
        width,
        height: 1.3,
        wheel_radius: 0.34,
        wheel_base,
        track: None,
    };
    Vehicle::new(dims, kind, mass, "", 0xffffff)
}

fn input() -> Input {
    Input::default()
}

fn speed_of(v: &Vehicle) -> f64 {
    hypot(v.vx, v.vz)
}

struct Car {
    v: Vehicle,
    phys: CarPhysics,
    t: Arc<Track>,
}

fn car(kind: &'static str, s: f64, lat: f64, speed: f64) -> Car {
    let spec = car_spec(kind).unwrap();
    let mut v = make_vehicle(kind, spec.mass);
    let mut phys = CarPhysics::new(&v, spec);
    let t = track();
    phys.reset(&mut v, &t, s, lat);
    v.vx = speed; // the test road runs along +x
    Car { v, phys, t }
}

fn drive(c: &mut Car, inp: Input, seconds: f64, mut each: impl FnMut(&Car)) {
    let mut t = 0.0;
    while t < seconds {
        c.phys.update(&mut c.v, &c.t, DT, &inp);
        each(c);
        t += DT;
    }
}

struct Perf {
    t100: Option<f64>,
    top: f64,
    end: Car,
}

/// 0–100 km/h time, and top speed after a long run flat out.
fn perf(kind: &'static str) -> &'static Perf {
    static P: OnceLock<Vec<(&'static str, Perf)>> = OnceLock::new();
    let all = P.get_or_init(|| {
        CAR_SPECS
            .iter()
            .map(|&(kind, _)| {
                let mut c = car(kind, 100.0, 0.0, 0.0);
                let (mut t100, mut top, mut t) = (None, 0.0f64, 0.0);
                drive(
                    &mut c,
                    Input {
                        throttle: 1.0,
                        ..input()
                    },
                    100.0,
                    |c| {
                        t += DT;
                        let sp = speed_of(&c.v);
                        if t100.is_none() && sp >= 100.0 / 3.6 {
                            t100 = Some(t);
                        }
                        top = top.max(sp);
                    },
                );
                (kind, Perf { t100, top, end: c })
            })
            .collect()
    });
    &all.iter().find(|(k, _)| *k == kind).unwrap().1
}

#[test]
fn accelerates_from_rest_and_tops_out_at_a_plausible_speed() {
    for (kind, spec) in CAR_SPECS {
        let p = perf(kind);
        let t100 = p.t100.unwrap();
        assert!(
            t100 > 1.5 && t100 < 6.0,
            "{kind}: 0–100 km/h in {t100:.2} s"
        );
        let kmh = p.top * 3.6;
        assert!(
            kmh > 200.0 && kmh < 320.0,
            "{kind}: top speed {kmh:.0} km/h"
        );
        // Held flat out on a straight, it goes straight.
        assert!(
            p.end.v.lat.abs() < 0.01,
            "{kind}: stayed on the centreline (lat {})",
            p.end.v.lat
        );
        assert!(p.end.v.yaw.abs() < 1e-6);
        if let Some(vmax) = spec.vmax {
            assert!(
                p.top <= vmax + 0.5,
                "{kind}: the limiter holds ({:.1} m/s)",
                p.top
            );
        }
        // The gearbox worked its way up (the electric car has one gear).
        assert_eq!(p.end.phys.gear, if spec.electric { 1 } else { 6 });
    }
}

#[test]
fn brakes_to_a_stop() {
    for (kind, _) in CAR_SPECS {
        let mut c = car(kind, 100.0, 0.0, 30.0);
        let s0 = c.v.s;
        let (mut stopped_at, mut t) = (None, 0.0);
        drive(
            &mut c,
            Input {
                brake: 1.0,
                ..input()
            },
            4.0,
            |c| {
                t += DT;
                if stopped_at.is_none() && c.v.speed <= 0.05 {
                    stopped_at = Some(t);
                }
            },
        );
        assert!(
            stopped_at.is_some_and(|s| s < 3.0),
            "{kind}: stopped from 30 m/s in {stopped_at:?} s"
        );
        // Braking at ~15 m/s² needs about 30 m.
        assert!(
            c.v.s - s0 < 45.0,
            "{kind}: stopping distance {:.1} m",
            c.v.s - s0
        );
        assert!(
            c.v.brake_light == 0.0 || c.v.speed <= 0.5,
            "brake lights go off once stopped"
        );
    }
}

/// README controls: "S / ↓  Brake, then reverse".
#[test]
fn holding_the_brake_from_a_standstill_reverses() {
    for (kind, _) in CAR_SPECS {
        let mut c = car(kind, 100.0, 0.0, 0.0);
        drive(
            &mut c,
            Input {
                brake: 1.0,
                ..input()
            },
            2.0,
            |_| {},
        );
        assert!(
            c.v.speed < -2.0,
            "{kind}: rolling backwards after 2 s on the brake ({:.2} m/s)",
            c.v.speed
        );
        assert_eq!(c.phys.gear, -1, "in reverse");
        // Reverse is limited.
        drive(
            &mut c,
            Input {
                brake: 1.0,
                ..input()
            },
            6.0,
            |_| {},
        );
        assert!(
            c.v.speed > -14.0,
            "{kind}: reverse speed {:.1} m/s",
            c.v.speed
        );
        // Throttle takes it out of reverse.
        drive(
            &mut c,
            Input {
                throttle: 1.0,
                ..input()
            },
            4.0,
            |_| {},
        );
        assert!(c.v.speed > 0.0 && c.phys.gear >= 1, "forwards again");
    }
}

#[test]
fn steering_right_turns_right_left_turns_left() {
    for (kind, _) in CAR_SPECS {
        // Positive yaw turns right; the road's right is +z here.
        let mut r = car(kind, 100.0, 0.0, 20.0);
        drive(
            &mut r,
            Input {
                throttle: 0.4,
                steer: 1.0,
                ..input()
            },
            1.0,
            |_| {},
        );
        assert!(r.v.yaw > 0.1, "{kind}: yaw {:.3}", r.v.yaw);
        assert!(r.v.lat > 0.5, "{kind}: moved right (lat {:.2})", r.v.lat);
        let mut l = car(kind, 100.0, 0.0, 20.0);
        drive(
            &mut l,
            Input {
                throttle: 0.4,
                steer: -1.0,
                ..input()
            },
            1.0,
            |_| {},
        );
        assert!(l.v.yaw < -0.1 && l.v.lat < -0.5, "mirror image");
        assert!(
            (l.v.yaw + r.v.yaw).abs() < 1e-6,
            "left and right are symmetric"
        );
        // Standing still, the wheel alone doesn't turn the car.
        let mut s = car(kind, 100.0, 0.0, 0.0);
        drive(
            &mut s,
            Input {
                steer: 1.0,
                ..input()
            },
            1.0,
            |_| {},
        );
        assert_eq!(s.v.yaw, 0.0);
    }
}

#[test]
fn nitro_adds_speed_and_burns_the_tank() {
    for (kind, _) in CAR_SPECS {
        let mut a = car(kind, 100.0, 0.0, 25.0);
        let mut b = car(kind, 100.0, 0.0, 25.0);
        let tank = b.phys.nitro;
        let mut active = false;
        drive(
            &mut a,
            Input {
                throttle: 1.0,
                ..input()
            },
            2.0,
            |_| {},
        );
        drive(
            &mut b,
            Input {
                throttle: 1.0,
                nitro: true,
                ..input()
            },
            2.0,
            |c| active |= c.phys.nitro_active,
        );
        assert!(active, "nitroActive while boosting");
        assert!(
            speed_of(&b.v) > speed_of(&a.v) + 5.0,
            "{kind}: with nitro {:.1} vs {:.1} m/s",
            speed_of(&b.v),
            speed_of(&a.v)
        );
        assert!(b.phys.nitro < tank, "the tank drains");
        // An empty tank does nothing.
        let mut e = car(kind, 100.0, 0.0, 25.0);
        e.phys.nitro = 0.0;
        drive(
            &mut e,
            Input {
                throttle: 1.0,
                nitro: true,
                ..input()
            },
            2.0,
            |_| {},
        );
        assert!((speed_of(&e.v) - speed_of(&a.v)).abs() < 1e-6);
        // Nitro needs the throttle.
        let mut n = car(kind, 100.0, 0.0, 25.0);
        drive(
            &mut n,
            Input {
                nitro: true,
                ..input()
            },
            1.0,
            |_| {},
        );
        assert!(!n.phys.nitro_active);
    }
}

/// A thumb stick, tilt or gamepad asks for a share of the grip; keys ask for
/// a wheel angle (and are ramped by Input). At speed a fifth of the stick
/// used to turn as hard as the tyres allow, which made a phone twitchy.
#[test]
fn analogue_steering_spans_the_grip_at_any_speed_keys_keep_their_lock() {
    let yaw_after = |speed: f64, steer: f64, analog: bool| {
        let mut c = car("sports", 100.0, 0.0, speed);
        drive(
            &mut c,
            Input {
                throttle: 1.0,
                steer,
                analog,
                ..input()
            },
            0.3,
            |_| {},
        );
        c.v.yaw_rate.abs()
    };
    let spec = car_spec("sports").unwrap();
    for speed in [30.0, 45.0, 60.0] {
        let limit = spec.grip * 1.5 / speed; // the grip-limited turn rate
        // Keys: a fifth of the lock is already at the limit.
        assert!(
            yaw_after(speed, 0.2, false) > limit * 0.9,
            "keys at {speed} m/s"
        );
        // Stick: a fifth asks for a fifth (and a bit), half for half, full for all of it.
        let (fifth, half, full) = (
            yaw_after(speed, 0.2, true),
            yaw_after(speed, 0.5, true),
            yaw_after(speed, 1.0, true),
        );
        assert!(
            (fifth / limit - 0.2 * ANALOG_LOCK).abs() < 0.06,
            "stick 0.2 at {speed} m/s: {:.2} of the grip",
            fifth / limit
        );
        assert!(
            (half / limit - 0.5 * ANALOG_LOCK).abs() < 0.1,
            "stick 0.5 at {speed} m/s: {:.2} of the grip",
            half / limit
        );
        assert!(
            full > limit * 0.9,
            "full stick reaches the grip at {speed} m/s ({:.2})",
            full / limit
        );
    }
    // At parking speeds the stick has the full wheel lock, as keys do.
    assert!((yaw_after(5.0, 1.0, true) - yaw_after(5.0, 1.0, false)).abs() < 1e-9);
}

#[test]
fn handbrake_at_speed_starts_a_drift_and_drifting_fills_the_nitro_tank() {
    let mut c = car("sports", 100.0, 0.0, 30.0);
    c.phys.nitro = 0.2;
    let mut drifted = false;
    drive(
        &mut c,
        Input {
            throttle: 1.0,
            steer: 1.0,
            handbrake: true,
            ..input()
        },
        0.6,
        |c| drifted |= c.phys.drifting,
    );
    assert!(drifted, "drifting");
    assert!(c.phys.slip.abs() > 0.18, "slip angle {:.2}", c.phys.slip);
    assert!(c.phys.skid > 0.3, "tyres skid");
    let before = c.phys.nitro;
    drive(
        &mut c,
        Input {
            throttle: 1.0,
            steer: 1.0,
            ..input()
        },
        0.5,
        |_| {},
    );
    assert!(
        c.phys.nitro > before,
        "nitro {before:.3} → {:.3}",
        c.phys.nitro
    );
}

#[test]
fn the_car_is_held_still_during_the_countdown() {
    let mut c = car("sports", 100.0, 0.0, 0.0);
    c.phys.locked = true;
    let x = c.v.x;
    drive(
        &mut c,
        Input {
            throttle: 1.0,
            ..input()
        },
        2.0,
        |_| {},
    );
    assert_eq!(c.v.x, x);
    assert_eq!(speed_of(&c.v), 0.0);
    assert!(
        c.phys.rpm > 5000.0,
        "revving on the line ({:.0} rpm)",
        c.phys.rpm
    );
    c.phys.locked = false;
    drive(
        &mut c,
        Input {
            throttle: 1.0,
            ..input()
        },
        1.0,
        |_| {},
    );
    assert!(speed_of(&c.v) > 5.0, "goes at GO");
}

#[test]
fn lifting_off_coasts_down_and_it_rolls_to_a_stop() {
    let mut c = car("sports", 100.0, 0.0, 20.0);
    drive(&mut c, input(), 3.0, |_| {});
    let sp = speed_of(&c.v);
    assert!(sp < 18.0 && sp > 5.0, "coasting {sp:.1} m/s after 3 s");
    drive(&mut c, input(), 30.0, |_| {});
    assert!(
        c.v.speed >= 0.0 && c.v.speed < 1e-3,
        "stopped, not creeping backwards ({})",
        c.v.speed
    );
}

#[test]
fn the_walls_keep_the_car_on_the_road_with_an_impact_event() {
    for (kind, _) in CAR_SPECS {
        let mut c = car(kind, 100.0, 0.0, 35.0);
        let mut max_lat = 0.0f64;
        drive(
            &mut c,
            Input {
                throttle: 1.0,
                steer: 1.0,
                ..input()
            },
            4.0,
            |c| max_lat = max_lat.max(c.v.lat),
        );
        let wall = c.t.wall_r[100] as f64;
        assert!(
            max_lat <= wall - c.v.half_w + 0.01,
            "{kind}: lat {max_lat:.2} with the wall at {wall:.2}"
        );
        assert!(
            c.phys
                .events
                .iter()
                .any(|e| matches!(e, PhysEvent::Impact { .. })),
            "{kind}: hit the wall"
        );
        for (k, x) in [
            ("x", c.v.x),
            ("z", c.v.z),
            ("vx", c.v.vx),
            ("vz", c.v.vz),
            ("yaw", c.v.yaw),
        ] {
            assert!(x.is_finite(), "{kind}: {k} finite");
        }
    }
}

/// Pressed into the right-hand wall at `rel` to the road, spinning at
/// `yaw_rate`, for one frame.
fn wall_frame(rel: f64, yaw_rate: f64, steer: f64) -> f64 {
    let mut c = car("sports", 100.0, 0.0, 0.0);
    let ext = c.v.half_w * mr_math::kernel::cos(rel) + c.v.half_l * mr_math::kernel::sin(rel).abs();
    let t = c.t.clone();
    c.phys
        .reset(&mut c.v, &t, 100.0, t.wall_r[100] as f64 - ext);
    c.v.yaw = rel;
    c.v.vx = 25.0;
    c.v.vz = 3.0;
    c.v.yaw_rate = yaw_rate;
    c.phys.update(
        &mut c.v,
        &t,
        DT,
        &Input {
            throttle: 1.0,
            steer,
            ..input()
        },
    );
    assert!(c.phys.scrape > 0.0, "on the wall");
    c.v.yaw_rate
}

#[test]
fn the_wall_never_fights_steering_off_it_but_a_tail_slap_still_kills_the_spin() {
    // Nose in, turning back out at full lock: the turn carries on.
    let out = wall_frame(0.3, -0.6, -1.0);
    assert!(out < -0.5, "steering off the wall: yaw rate {out:.2}");
    // Tail swinging into the wall: the wall stops it.
    let slap = wall_frame(-0.4, -2.0, 0.0);
    assert!(slap > -1.0, "tail slap: yaw rate {slap:.2}");
}

#[test]
fn the_ends_of_a_point_to_point_road_stop_the_car() {
    let mut c = car("sports", 20.0, 0.0, -15.0);
    drive(&mut c, input(), 3.0, |_| {});
    assert!(c.v.s >= 0.5, "didn't back off the start (s {:.2})", c.v.s);
}

#[test]
fn the_electric_motor_has_one_gear_a_power_meter_rev_counter_and_regen() {
    let mut c = car("electric", 100.0, 0.0, 30.0);
    drive(
        &mut c,
        Input {
            throttle: 1.0,
            ..input()
        },
        1.0,
        |_| {},
    );
    assert_eq!(c.phys.gear, 1);
    assert!(c.phys.rpm > 0.0 && c.phys.rpm <= MOTOR_MAX * 1.02);
    assert!(c.phys.power_out > 0.0, "drawing power");
    c.phys.nitro = 0.2;
    let tank = c.phys.nitro;
    drive(
        &mut c,
        Input {
            brake: 0.5,
            ..input()
        },
        1.0,
        |_| {},
    );
    assert!(
        c.phys.regen > 0.2 && c.phys.power_out < 0.0,
        "braking regenerates"
    );
    assert!(c.phys.nitro > tank, "regen charges the boost tank");
}

#[test]
fn the_turbo_builds_boost_under_throttle_and_dumps_it_on_a_lift() {
    let mut c = car("rally", 100.0, 0.0, 20.0);
    drive(
        &mut c,
        Input {
            throttle: 1.0,
            ..input()
        },
        2.0,
        |_| {},
    );
    assert!(c.phys.boost > 0.5, "boost {:.2}", c.phys.boost);
    drive(&mut c, input(), 0.5, |_| {});
    assert!(c.phys.boost < 0.1, "after lifting {:.2}", c.phys.boost);
}

#[test]
fn the_same_inputs_give_the_same_drive() {
    let run = || {
        let mut c = car("muscle", 100.0, 0.0, 10.0);
        let t = c.t.clone();
        for i in 0..600 {
            let inp = Input {
                throttle: 1.0,
                steer: mr_math::kernel::sin(i as f64 / 40.0),
                handbrake: i % 200 < 30,
                ..input()
            };
            c.phys.update(&mut c.v, &t, DT, &inp);
        }
        [c.v.x, c.v.z, c.v.yaw, c.phys.nitro]
    };
    assert_eq!(run(), run());
}

/// At a steady 70 m/s every frame should move the car speed × frame time, at
/// any refresh rate and with the timing jitter a browser adds. The camera and
/// every other car move by the frame's time; a car that moves 0, 1 or 3 physics
/// steps' worth instead lurches against them.
#[test]
fn the_car_moves_by_each_frames_own_time_at_any_refresh_rate() {
    let mut seed = 7.0f64;
    let mut rand = || {
        seed = (seed * 16807.0) % 2147483647.0;
        seed / 2147483647.0
    };
    for hz in [60.0, 90.0, 120.0, 144.0] {
        let mut c = car("super", 100.0, 0.0, 70.0);
        let t = c.t.clone();
        let mut worst = 0.0f64;
        for _ in 0..400 {
            let dt = (1.0 + (rand() - 0.5) * 0.06) / hz; // ±3 % jitter
            let (x0, v0) = (c.v.x, c.v.vx);
            c.phys.update(
                &mut c.v,
                &t,
                dt,
                &Input {
                    throttle: 0.4,
                    ..input()
                },
            );
            let expect = ((v0 + c.v.vx) / 2.0) * dt;
            worst = worst.max((c.v.x - x0 - expect).abs() / expect);
        }
        assert!(
            worst < 0.01,
            "{hz} Hz: a frame's move was off by {:.1} %",
            worst * 100.0
        );
    }
}

/// README "Cars": Kestrel RS "launches hard … but its top speed is lower";
/// Ion Arc has "the quickest launch" and "a lower top speed". (The check
/// that the README's table lists every car is dropped: it reads README text.)
#[test]
fn readme_claims_about_the_cars_hold() {
    let mut fastest: Vec<_> = CAR_SPECS
        .iter()
        .map(|(k, _)| (*k, perf(k).t100.unwrap()))
        .collect();
    fastest.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    assert_eq!(fastest[0].0, "electric", "quickest 0–100: {fastest:?}");
    for k in ["sports", "muscle", "super"] {
        assert!(
            perf("rally").t100 < perf(k).t100,
            "Kestrel launches harder than {k}"
        );
        assert!(
            perf("rally").top < perf(k).top,
            "Kestrel's top speed is below {k}'s"
        );
        assert!(
            perf("electric").top < perf(k).top,
            "Ion Arc's top speed is below {k}'s"
        );
    }
}

/// From `levels.test.js` (DECISIONS D56): every rival drives a car in
/// CAR_SPECS.
#[test]
fn rivals_drive_cars_in_car_specs() {
    for l in mr_levels::levels() {
        for r in &l.rivals {
            assert!(
                car_spec(r.kind).is_some(),
                "{}: {}: kind '{}'",
                l.id,
                r.name,
                r.kind
            );
        }
    }
}
