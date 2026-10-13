//! Step-steer sweep on the flat rig (D1203): hold a speed, apply a fraction
//! of the player's grip-scaled lock, and report what the car does after
//! 3 s (yaw rate against the kinematic one, lateral g, each axle's slip
//! angle and sliding share). The lock mirrors `mp_sim`'s
//! `SimCar::steer_max`, which cannot be called from here.
//!
//! `cargo run --release -p mp_vdyn --example steer_sweep -- [rear_a]
//! [rear_mu] [lock_frac] [throttle]`: the Vento with its rear tyre's
//! half-patch and peak friction overridden, full lock asking `lock_frac`
//! of the front's peak (0.9), and a fixed throttle instead of the speed
//! hold.

use mp_vdyn::cars::vento_gt;
use mp_vdyn::rig::Rig;
use mp_vdyn::vehicle::G;
use mp_vdyn::{Controls, VehicleDef};

/// Full lock asks the front tyres for `lock_frac` of their peak force.
fn steer_max(def: &VehicleDef, speed: f64, lock_frac: f64) -> f64 {
    let loads = def.static_axle_loads();
    let v = speed.abs().max(1.0);
    // The lateral acceleration the front axle holds at `lock_frac` of
    // its peak, and each axle's slip angle carrying its share of it
    // (the brush curve inverted: 1 − (1−λ)³ = f).
    let slip = |i: usize, f: f64| {
        let t = &def.axles[i].tyre;
        let fz = loads[i] / 2.0;
        let f = f.clamp(0.0, 1.0);
        let l = 1.0 - (1.0 - f).cbrt();
        (l * 3.0 * t.mu_at(fz) * fz / t.stiffness()).atan()
    };
    let t0 = &def.axles[0].tyre;
    let ay = lock_frac * t0.mu_at(loads[0] / 2.0) * G;
    let t1 = &def.axles[1].tyre;
    let f_r = ay / (t1.mu_at(loads[1] / 2.0) * G);
    let want = def.wheelbase() * ay / (v * v) + slip(0, lock_frac) - slip(1, f_r);
    want.clamp(0.0, def.axles[0].steer_lock)
}

fn main() {
    let args: Vec<f64> = std::env::args()
        .skip(1)
        .map(|a| a.parse().unwrap())
        .collect();
    let mut def = vento_gt();
    if let Some(&a) = args.first() {
        def.axles[1].tyre.a = a;
    }
    if let Some(&mu) = args.get(1) {
        def.axles[1].tyre.mu0 = mu;
    }
    let lock_frac = args.get(2).copied().unwrap_or(0.9);
    let throttle = args.get(3).copied();
    def.balance_springs();
    let k = def.understeer_gradient();
    println!(
        "understeer gradient {:.4} rad per m/s^2 = {:.2} deg/g",
        k,
        (k * G).to_degrees()
    );
    println!(
        "speed  frac  lock(deg)  yaw(deg/s) kin(deg/s)  ay(g)  slipF(deg) slipR(deg) slideF slideR  throttle  speedEnd"
    );
    for &speed in &[10.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0] {
        for &frac in &[0.3, 0.6, 1.0] {
            let mut r = Rig::flat(def.clone());
            r.car.set_speed(&r.def, speed);
            let lock = steer_max(&r.def, speed, lock_frac);
            let ctl = Controls {
                steer: frac * lock / r.def.axles[0].steer_lock,
                abs: true,
                tc: true,
                ..Controls::default()
            };
            let mut thr = 0.0;
            for _ in 0..360 {
                match throttle {
                    Some(t) => {
                        thr = t;
                        r.step(&Controls { throttle: t, ..ctl });
                    }
                    None => {
                        thr = r.hold(speed);
                        r.step_at(&ctl, speed);
                    }
                }
            }
            let tm = r.car.telemetry(&r.def);
            let delta = frac * lock;
            let kin = speed * delta.tan() / r.def.wheelbase();
            let sf = (tm.wheels[0].slip_angle + tm.wheels[1].slip_angle) / 2.0;
            let sr = (tm.wheels[2].slip_angle + tm.wheels[3].slip_angle) / 2.0;
            let slf = tm.wheels[0].sliding.max(tm.wheels[1].sliding);
            let slr = tm.wheels[2].sliding.max(tm.wheels[3].sliding);
            println!(
                "{:5.0}  {:.1}   {:6.2}    {:7.1}    {:7.1}   {:5.2}   {:7.2}   {:7.2}    {:.2}   {:.2}    {:.2}     {:.1}",
                speed,
                frac,
                delta.to_degrees(),
                tm.yaw_rate.to_degrees(),
                kin.to_degrees(),
                tm.accel.z / G,
                sf.to_degrees(),
                sr.to_degrees(),
                slf,
                slr,
                thr,
                tm.speed
            );
        }
    }
}
