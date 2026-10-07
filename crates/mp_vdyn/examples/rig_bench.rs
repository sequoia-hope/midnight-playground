//! The cost of one tick of a Tier 1 car on the flat rig (SPEC 1.1, 5:
//! "measured, not assumed"). It reads the clock, so it lives outside `src/`.
//!
//!   cargo run --release -p mp_vdyn --example rig_bench

use std::time::Instant;

use mp_vdyn::Controls;
use mp_vdyn::cars::rig_car;
use mp_vdyn::rig::Rig;

fn main() {
    let mut r = Rig::flat(rig_car());
    let ticks = 120 * 600;
    let t0 = Instant::now();
    for tick in 0..ticks {
        let t = tick as f64 / 120.0;
        let c = Controls {
            throttle: 0.6,
            steer: 0.3 * mp_math::kernel::sin(t * 0.7),
            ..Controls::default()
        };
        r.step_at(&c, 30.0);
    }
    let dt = t0.elapsed().as_secs_f64();
    println!(
        "{} ticks ({} substeps each) in {:.3} s: {:.2} µs per tick; hash {:#018x}",
        ticks,
        r.def.substeps,
        dt,
        dt / ticks as f64 * 1e6,
        r.car.hash()
    );
}
