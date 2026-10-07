//! The Vento GT (V2's car) on the rig: the engine, the automatic gearbox,
//! traction control, top speed, reverse, and the chassis against a wall.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_vdyn::cars::vento_gt;
use mp_vdyn::rig::Rig;
use mp_vdyn::{Aabb, Collider, Controls, FlatGround, Ground, GroundSample, SdfSample, Vec3};

fn full(tc: bool) -> Controls {
    Controls {
        throttle: 1.0,
        tc,
        ..Controls::default()
    }
}

#[test]
fn launches_through_the_gears_with_traction_control() {
    let mut r = Rig::flat(vento_gt());
    let mut shifts = 0;
    let mut t100 = None;
    let mut top_rpm: f64 = 0.0;
    while r.time() < 30.0 {
        r.step(&full(true));
        let e = r.car.engine.as_ref().unwrap();
        if e.shifted > 0 {
            shifts += 1;
        }
        top_rpm = top_rpm.max(e.rpm);
        if t100.is_none() && r.car.speed() > 100.0 / 3.6 {
            t100 = Some(r.time());
        }
    }
    let t100 = t100.unwrap();
    // A 280 kW, 1350 kg rear-drive car: 0 to 100 km/h in four to six
    // seconds, all six gears, never past the limiter by more than a tick's
    // worth.
    assert!((3.8..6.0).contains(&t100), "0-100 in {t100} s");
    assert_eq!(shifts, 5);
    assert_eq!(r.car.engine.as_ref().unwrap().gear, 6);
    assert!(top_rpm < 7900.0, "{top_rpm}");
    // Near its top speed after 30 s: drag balances about 280 kW.
    let v = r.car.speed();
    assert!((70.0..92.0).contains(&v), "{} km/h", v * 3.6);
}

#[test]
fn traction_control_beats_wheelspin() {
    let time_to_30 = |tc: bool| {
        let mut r = Rig::flat(vento_gt());
        while r.car.speed() < 30.0 && r.time() < 20.0 {
            r.step(&full(tc));
        }
        r.time()
    };
    let with = time_to_30(true);
    let without = time_to_30(false);
    assert!(with < without, "TC {with} s, none {without} s");
}

#[test]
fn reverse_through_neutral_and_back() {
    let mut r = Rig::flat(vento_gt());
    let down = Controls {
        shift: -1,
        ..Controls::default()
    };
    r.step(&down);
    assert_eq!(r.car.engine.as_ref().unwrap().gear, 0);
    r.run(30, &Controls::default());
    r.step(&down);
    assert_eq!(r.car.engine.as_ref().unwrap().gear, -1);
    r.run(30, &Controls::default());
    r.run(
        240,
        &Controls {
            throttle: 0.4,
            ..Controls::default()
        },
    );
    assert!(r.car.speed() < -2.0, "{}", r.car.speed());
    // At idle in gear with the brake on, nothing creeps.
    let mut r = Rig::flat(vento_gt());
    r.run(
        600,
        &Controls {
            brake: 1.0,
            ..Controls::default()
        },
    );
    assert!(r.car.body.vel.length() < 1e-6);
}

/// Flat ground with a wall along z = `z`, facing −z.
struct Walled {
    z: f64,
}

impl Ground for Walled {
    fn height(&self, x: f64, z: f64) -> GroundSample {
        FlatGround::default().height(x, z)
    }
    fn sdf(&self, p: Vec3) -> SdfSample {
        FlatGround::default().sdf(p)
    }
    fn colliders(&self, _aabb: Aabb, out: &mut Vec<Collider>) {
        out.push(Collider::Plane {
            point: Vec3::new(0.0, 0.0, self.z),
            normal: Vec3::new(0.0, 0.0, -1.0),
        });
    }
}

#[test]
fn the_chassis_hits_a_wall_and_glances_off() {
    let def = vento_gt();
    let mut car = mp_vdyn::Vehicle::new(&def, 0.0, 0.0, 0.0, 0.35);
    car.set_speed(&def, 25.0);
    let g = Walled { z: 6.0 };
    let mut impact: f64 = 0.0;
    let mut deepest: f64 = 0.0;
    let mut spun: f64 = 0.0;
    for _ in 0..(3 * 120) {
        car.step(&def, &Controls::default(), &g);
        impact = impact.max(car.contact.impact);
        // How far the box's corners went past the wall.
        for (sx, sz) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let c = car.body.pos
                + car.body.rot.rotate(Vec3::new(
                    sx * def.chassis.half.x,
                    0.0,
                    sz * def.chassis.half.z,
                ));
            deepest = deepest.max(c.z - 6.0);
        }
        spun = spun.max(car.yaw_rate().abs());
    }
    // It hit at about 25·sin(0.35) ≈ 8.6 m/s into the wall, went in a
    // little, and is now moving along it or away, with the speed scrubbed
    // and a spin from the corner strike.
    assert!((5.0..10.0).contains(&impact), "impact {impact}");
    assert!(deepest < 0.25, "{deepest} m into the wall");
    assert!(car.body.vel.z <= 0.5, "{:?}", car.body.vel);
    assert!(car.speed() < 24.0 && car.speed() > 5.0, "{}", car.speed());
    assert!(spun > 0.3, "yaw rate {spun}");
}
