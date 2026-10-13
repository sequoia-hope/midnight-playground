//! The Vento GT's cornering limit on the flat rig: at each speed the
//! steering winds on slowly while a speed hold keeps the pace, and the
//! highest lateral acceleration held for a third of a second is the limit
//! (D1206). Tyres and aero can be overridden to try a setup:
//!
//! `MUF=1.4 MUR=1.5 CLF=0.9 CLR=1.1 CDA=0.85 cargo run --release -p mp_vdyn
//! --example grip_probe`

use mp_vdyn::Controls;
use mp_vdyn::cars::vento_gt;
use mp_vdyn::rig::Rig;
use mp_vdyn::vehicle::{G, TICK};

fn env(name: &str) -> Option<f64> {
    std::env::var(name).ok().and_then(|s| s.parse().ok())
}

fn main() {
    let mut def = vento_gt();
    if let Some(m) = env("MUF") {
        def.axles[0].tyre.mu0 = m;
    }
    if let Some(m) = env("MUR") {
        def.axles[1].tyre.mu0 = m;
    }
    if let Some(c) = env("CLF") {
        def.aero.cla_front = c;
    }
    if let Some(c) = env("CLR") {
        def.aero.cla_rear = c;
    }
    if let Some(c) = env("CDA") {
        def.aero.cda = c;
    }
    def.balance_springs();
    println!(
        "mu {:.2}/{:.2}  ClA {:.2}/{:.2}  CdA {:.2}",
        def.axles[0].tyre.mu0,
        def.axles[1].tyre.mu0,
        def.aero.cla_front,
        def.aero.cla_rear,
        def.aero.cda
    );
    println!("speed  max ay(g)  at speed  wheel(deg)  rear slip(deg)  ends");
    let ramp = 12.0;
    for &speed in &[20.0, 30.0, 40.0, 50.0, 60.0, 67.0] {
        let mut r = Rig::flat(def.clone());
        r.car.set_speed(&r.def, speed);
        let lock = r.def.axles[0].steer_lock;
        // At speed only a small wheel angle matters: ramp to what a
        // kinematic 2 g turn needs and 8° of slip, or full lock.
        let top = (r.def.wheelbase() * 2.0 * G / (speed * speed) + 0.14).min(lock);
        let (mut ay_lp, mut best, mut at_v, mut at_d, mut at_sr) = (0.0, 0.0f64, 0.0, 0.0, 0.0);
        let mut end = "held";
        let n = (ramp / TICK) as usize;
        for i in 0..n {
            let d = top * i as f64 / n as f64;
            let ctl = Controls {
                steer: d / lock,
                abs: true,
                tc: true,
                ..Controls::default()
            };
            r.step_at(&ctl, speed);
            let tm = r.car.telemetry(&r.def);
            ay_lp += (tm.accel.z.abs() - ay_lp) * (TICK / 0.33);
            let sr = (tm.wheels[2].slip_angle + tm.wheels[3].slip_angle) / 2.0;
            if ay_lp > best {
                best = ay_lp;
                at_v = tm.speed;
                at_d = d;
                at_sr = sr;
            }
            if sr.abs() > 0.2 {
                end = "spun";
                break;
            }
            if tm.speed < speed * 0.8 {
                end = "slowed";
                break;
            }
        }
        println!(
            "{:5.0}   {:6.2}    {:6.1}    {:6.2}      {:6.2}       {}",
            speed,
            best / G,
            at_v,
            at_d.to_degrees(),
            at_sr.to_degrees(),
            end
        );
    }
    // Braking: full brake with ABS from 67 m/s to 20 m/s, and the top
    // speed after 40 s flat out.
    let mut r = Rig::flat(def.clone());
    r.car.set_speed(&r.def, 67.0);
    let brake = Controls {
        brake: 1.0,
        abs: true,
        ..Controls::default()
    };
    let t0 = r.time();
    let mut fast = None;
    while r.car.speed() > 20.0 && r.time() - t0 < 20.0 {
        r.step(&brake);
        if fast.is_none() && r.car.speed() < 45.0 {
            fast = Some(r.time() - t0);
        }
    }
    let t = r.time() - t0;
    let t45 = fast.unwrap_or(t);
    println!(
        "braking 67→20 m/s: {:.2} g average ({:.2} g from 67 to 45)",
        (67.0 - 20.0) / t / G,
        (67.0 - 45.0) / t45 / G
    );
    let mut r = Rig::flat(def);
    let flat = Controls {
        throttle: 1.0,
        tc: true,
        ..Controls::default()
    };
    r.run((40.0 / TICK) as u32, &flat);
    println!("top speed after 40 s: {:.1} m/s", r.car.speed());
}
