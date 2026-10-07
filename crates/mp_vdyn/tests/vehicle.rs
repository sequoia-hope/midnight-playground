//! The rig car against textbook behaviour on flat ground (SPEC 10): static
//! loads, the skidpad (the linear bicycle model, then the grip limit),
//! braking against v²/(2μg) with ABS, a step steer, launches limited by
//! torque and by traction, energy in a frictionless coast and a drop,
//! stillness on a 20 % grade, brake friction, and determinism.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_math::kernel;
use mp_vdyn::cars::rig_car;
use mp_vdyn::rig::Rig;
use mp_vdyn::vehicle::G;
use mp_vdyn::{Controls, FlatGround, PlaneGround, Quat, Vec3, Vehicle, VehicleDef};

fn rig() -> Rig<FlatGround> {
    Rig::flat(rig_car())
}

/// The rig at a steady `speed` on a circle with the steering at `steer`,
/// after `secs` of the speed hold.
fn skidpad(speed: f64, steer: f64, secs: f64) -> Rig<FlatGround> {
    let mut r = rig();
    r.car.set_speed(&r.def, speed);
    let c = Controls {
        steer,
        ..Controls::default()
    };
    for _ in 0..(secs * 120.0) as u32 {
        r.step_at(&c, speed);
    }
    r
}

/// Mean road-wheel angle of the steered axle.
fn front_angle(car: &Vehicle) -> f64 {
    (car.wheels[0].steer + car.wheels[1].steer) / 2.0
}

#[test]
fn static_loads_follow_the_lever_rule() {
    let r = rig();
    let t = r.car.telemetry(&r.def);
    let total: f64 = t.wheels.iter().map(|w| w.load).sum();
    let weight = r.def.mass * G;
    assert!(
        (total - weight).abs() < 1e-3 * weight,
        "{total} vs {weight}"
    );
    let axles = r.def.static_axle_loads();
    let front = t.wheels[0].load + t.wheels[1].load;
    assert!(
        (front - axles[0]).abs() < 0.01 * axles[0],
        "{front} vs {}",
        axles[0]
    );
    // Symmetric, and the springs balanced to zero travel.
    assert!((t.wheels[0].load - t.wheels[1].load).abs() < 1e-6);
    for w in &t.wheels {
        assert!(w.travel.abs() < 1e-3, "travel {}", w.travel);
    }
    // At rest: no motion.
    assert!(r.car.body.vel.length() < 1e-6);
}

#[test]
fn skidpad_linear_range_matches_the_bicycle_model() {
    let def = rig_car();
    let k = def.understeer_gradient();
    assert!(k > 0.0, "the front-heavy car understeers");
    let l = def.wheelbase();
    for steer in [0.01, 0.02, 0.03] {
        let r = skidpad(20.0, steer, 6.0);
        let v = r.car.speed();
        let yaw_rate = r.car.yaw_rate();
        let radius = v / yaw_rate;
        let ay = v * yaw_rate;
        assert!(ay < 0.3 * G, "linear range: {ay}");
        let delta = front_angle(&r.car);
        // δ = L/R + K·a_y.
        let model = l / radius + k * ay;
        assert!(
            (delta - model).abs() < 0.05 * delta,
            "steer {steer}: δ {delta} vs bicycle {model}"
        );
    }
}

#[test]
fn skidpad_peak_lateral_acceleration_is_near_the_tyres_mu() {
    // Steering wound on slowly at a held 20 m/s: the most lateral
    // acceleration the car reaches. Weight transfer and load sensitivity
    // put it a little under μ0·g; the sliding drop does not let it pass.
    let mut r = rig();
    r.car.set_speed(&r.def, 20.0);
    let mut best: f64 = 0.0;
    for tick in 0..(8 * 120) {
        let c = Controls {
            steer: 0.6 * tick as f64 / (8.0 * 120.0),
            ..Controls::default()
        };
        r.step_at(&c, 20.0);
        let t = r.car.telemetry(&r.def);
        best = best.max(t.accel.z);
    }
    let mu = r.def.axles[0].tyre.mu0;
    let ratio = best / (mu * G);
    assert!(
        (0.8..1.0).contains(&ratio),
        "peak a_y {best} = {ratio} μ0·g"
    );
}

#[test]
fn braking_with_abs_is_close_to_v2_over_2_mu_g() {
    let v0 = 30.0;
    let mu = rig_car().axles[0].tyre.mu0;
    let ideal = v0 * v0 / (2.0 * mu * G);
    let stop = |abs: bool| {
        let mut r = rig();
        r.car.set_speed(&r.def, v0);
        let x0 = r.car.body.pos.x;
        let c = Controls {
            brake: 1.0,
            abs,
            ..Controls::default()
        };
        let mut locked_ticks = 0;
        while r.car.speed() > 0.01 && r.ticks < 2400 {
            r.step(&c);
            if r.car.wheels[0].omega == 0.0 && r.car.speed() > 1.0 {
                locked_ticks += 1;
            }
        }
        (r.car.body.pos.x - x0, locked_ticks)
    };
    let (with_abs, _) = stop(true);
    let (locked, locked_ticks) = stop(false);
    assert!(
        with_abs > ideal && with_abs < 1.1 * ideal,
        "ABS: {with_abs} m against {ideal} m"
    );
    // Locked tyres slide on less than the peak: longer.
    assert!(
        locked > with_abs * 1.05,
        "locked {locked} vs ABS {with_abs}"
    );
    // And without ABS the fronts do lock: ω exactly 0 while sliding.
    assert!(locked_ticks > 120, "locked for {locked_ticks} ticks");
}

#[test]
fn a_step_steer_settles_on_the_bicycle_models_yaw_rate() {
    let v = 25.0;
    let steer = 0.02;
    let mut r = rig();
    r.car.set_speed(&r.def, v);
    let c = Controls {
        steer,
        ..Controls::default()
    };
    let mut rates = Vec::new();
    for _ in 0..(4 * 120) {
        r.step_at(&c, v);
        rates.push(r.car.yaw_rate());
    }
    let last = *rates.last().unwrap();
    let def = &r.def;
    let delta = front_angle(&r.car);
    let speed = r.car.speed();
    let model = speed * delta / (def.wheelbase() + def.understeer_gradient() * speed * speed);
    assert!((last - model).abs() < 0.05 * model, "{last} vs {model}");
    // Quick and well damped: 90 % within 0.3 s, overshoot under 10 %.
    let t90 = rates.iter().position(|&w| w > 0.9 * last).unwrap() as f64 / 120.0;
    assert!(t90 < 0.3, "t90 {t90}");
    let peak = rates.iter().cloned().fold(0.0, f64::max);
    assert!(peak < 1.1 * last, "overshoot {}", peak / last);
}

/// Seconds from 0 to `v` at a fixed throttle.
fn time_to(def: &VehicleDef, throttle: f64, v: f64) -> f64 {
    let mut r = Rig::flat(def.clone());
    let c = Controls {
        throttle,
        ..Controls::default()
    };
    while r.car.speed() < v && r.ticks < 120 * 20 {
        r.step(&c);
    }
    r.time()
}

#[test]
fn a_launch_is_limited_by_torque_then_by_traction() {
    let def = rig_car();
    let a = &def.axles[1];
    let rr = a.tyre.c_rr * G;
    // Torque-limited: a = T/(m·r) less rolling resistance (and a little
    // drag by 10 m/s).
    let t = 0.3 * def.drive.max_torque;
    let expect = t / (def.mass * a.tyre.radius) - rr;
    let got = 10.0 / time_to(&def, 0.3, 10.0);
    assert!((got - expect).abs() < 0.05 * expect, "{got} vs {expect}");

    // Traction-limited, rear drive: a = μ·g·l_f / (L − μ·h). The best
    // fixed throttle gets close; full throttle spins the tyres and is
    // slower.
    let mu = a.tyre.mu0;
    let l = def.wheelbase();
    let l_f = def.axles[0].x;
    let h = -def.axles[0].hub_y + a.tyre.radius;
    let limit = mu * G * l_f / (l - mu * h);
    let best = [0.6, 0.65, 0.7, 0.75, 0.8]
        .iter()
        .map(|&th| 15.0 / time_to(&def, th, 15.0))
        .fold(0.0, f64::max);
    assert!(
        best > 0.82 * limit && best < limit,
        "best {best} vs limit {limit}"
    );
    let full = 15.0 / time_to(&def, 1.0, 15.0);
    assert!(full < 0.95 * best, "wheelspin {full} vs {best}");
    // And the rear tyres spin.
    let mut r = Rig::flat(def.clone());
    r.run(
        60,
        &Controls {
            throttle: 1.0,
            ..Controls::default()
        },
    );
    let tm = r.car.telemetry(&r.def);
    assert!(tm.wheels[2].slip_ratio > 0.3, "{}", tm.wheels[2].slip_ratio);
}

/// The rig car with no rolling resistance and no air.
fn lossless() -> VehicleDef {
    let mut def = rig_car();
    def.aero.cda = 0.0;
    def.aero.cla_front = 0.0;
    def.aero.cla_rear = 0.0;
    for a in def.axles.iter_mut() {
        a.tyre.c_rr = 0.0;
    }
    def
}

#[test]
fn energy_never_grows_in_a_frictionless_coast() {
    let mut r = Rig::flat(lossless());
    r.car.set_speed(&r.def, 30.0);
    let e0 = r.car.energy(&r.def);
    for _ in 0..(60 * 120) {
        r.step(&Controls::default());
        let e = r.car.energy(&r.def);
        assert!(e <= e0 + 1.0, "at {}: {} J more", r.time(), e - e0);
    }
    // And it keeps its speed: nothing takes energy but the tyres' damping.
    assert!(r.car.speed() > 29.9, "{}", r.car.speed());
}

#[test]
fn energy_never_grows_after_a_drop() {
    let def = lossless();
    let mut car = Vehicle::new(&def, 0.0, 0.0, 0.0, 0.0);
    car.body.pos.y += 0.3;
    // A little roll and pitch, so all four corners land apart.
    car.body.rot = Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.5).normalize(), 0.05);
    let g = FlatGround::default();
    let e0 = car.energy(&def);
    let mut last = e0;
    for tick in 0..(5 * 120) {
        car.step(&def, &Controls::default(), &g);
        let e = car.energy(&def);
        assert!(
            e <= e0 + 1.0,
            "tick {tick}: {} J more than at the top",
            e - e0
        );
        last = e;
    }
    // Settled: the bounce has been damped away (with no rolling
    // resistance the car may still roll, but it no longer bounces).
    assert!(last < e0 - 0.5 * def.mass * G * 0.3);
    assert!(car.body.vel.y.abs() < 1e-3, "{:?}", car.body.vel);
    assert!(car.body.ang_vel.length() < 1e-3, "{:?}", car.body.ang_vel);
}

#[test]
fn parked_on_a_20_percent_grade_with_the_brake_on_it_holds_still() {
    let def = rig_car();
    let slope = PlaneGround::grade_x(0.2);
    let mut car = Vehicle::new(&def, 0.0, 0.0, 0.0, 0.0);
    // Nose uphill, pitched to the slope.
    car.body.rot = Quat::from_axis_angle(Vec3::Z, kernel::atan(0.2));
    let hold = Controls {
        brake: 1.0,
        ..Controls::default()
    };
    for _ in 0..(3 * 120) {
        car.step(&def, &hold, &slope);
    }
    let start = car.body.pos;
    let mut most: f64 = 0.0;
    for _ in 0..(60 * 120) {
        car.step(&def, &hold, &slope);
        most = most.max((car.body.pos - start).length());
        for w in &car.wheels {
            assert_eq!(w.omega, 0.0, "the brake holds every wheel");
        }
    }
    // No creep and no jitter: under a tenth of a millimetre in a minute.
    assert!(most < 1e-4, "moved {most} m");
    assert!(car.body.vel.length() < 1e-6);

    // Without the brake it rolls back down.
    for _ in 0..(3 * 120) {
        car.step(&def, &Controls::default(), &slope);
    }
    assert!(car.speed() < -1.0, "{}", car.speed());
}

#[test]
fn brakes_are_friction_not_a_negative_torque() {
    // Throttle under the brake's torque at a standstill: the wheels stay
    // exactly still, with no chatter and no creep.
    let mut r = rig();
    let c = Controls {
        throttle: 0.5,
        brake: 1.0,
        ..Controls::default()
    };
    for _ in 0..(5 * 120) {
        r.step(&c);
        for w in &r.car.wheels {
            assert_eq!(w.omega, 0.0);
        }
    }
    assert!(r.car.body.vel.length() < 1e-6);
    // Braking gently to a stop: the brake never drives a wheel backwards.
    // The car rocks back on its tyres as it stops, and the rear tyres'
    // push may briefly beat a light pedal, but only by a few milliradians.
    let mut r = rig();
    r.car.set_speed(&r.def, 10.0);
    let mut back = vec![0.0; 4];
    for _ in 0..(4 * 120) {
        let before: Vec<f64> = r.car.wheels.iter().map(|w| w.omega).collect();
        r.step(&Controls {
            brake: 0.3,
            ..Controls::default()
        });
        for (i, w) in r.car.wheels.iter().enumerate() {
            if w.omega < 0.0 {
                back[i] -= w.omega / 120.0;
            }
            // While the wheel still rolls, the brake never reverses it.
            if before[i] > 1.0 {
                assert!(w.omega > 0.0);
            }
        }
    }
    for b in back {
        assert!(b < 0.005, "turned back {b} rad");
    }
    assert!(r.car.speed().abs() < 1e-3, "{}", r.car.speed());
}

/// A scripted drive: launch, a slalom, a brake, a handbrake turn.
fn scripted() -> Vehicle {
    let mut r = rig();
    for tick in 0..(10 * 120u32) {
        let t = tick as f64 / 120.0;
        let c = Controls {
            throttle: if t < 4.0 { 0.7 } else { 0.2 },
            steer: if t > 2.0 {
                0.3 * kernel::sin(t * 2.0)
            } else {
                0.0
            },
            brake: if (6.0..7.0).contains(&t) { 0.8 } else { 0.0 },
            handbrake: if (7.5..8.0).contains(&t) { 1.0 } else { 0.0 },
            abs: t < 6.5,
        };
        r.step(&c);
    }
    r.car
}

#[test]
fn the_same_inputs_give_the_same_bits() {
    let a = scripted();
    let b = scripted();
    assert_eq!(a, b);
    assert_eq!(a.hash(), b.hash());
    // The hash on native and wasm (CI runs this crate's tests in Node)
    // must both be this one.
    assert_eq!(a.hash(), 0xc36cb015fdff512d, "hash {:#018x}", a.hash());
}
