//! From `test/unit/seaside.test.js` (DECISIONS D56): loose run-off slows the
//! car; paved run-off and the tarmac do not. Same assertions.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mp_math::kernel::hypot;
use mp_sim::dims::Dims;
use mp_sim::input::Input;
use mp_sim::physics::{CarPhysics, car_spec};
use mp_sim::vehicle::Vehicle;
use mp_track::Track;

const DT: f64 = 1.0 / 60.0;

#[test]
fn loose_run_off_slows_the_car_paved_run_off_and_the_tarmac_do_not() {
    let level = common::level("seaside");
    let t = Track::new(&level).unwrap();
    let loose = level.loose_ground.clone().unwrap();
    let spec = car_spec("sports").unwrap();
    let run = |s: f64, lat: f64| {
        let dims = Dims {
            length: 4.47,
            width: 1.9,
            height: 1.3,
            wheel_radius: 0.34,
            wheel_base: 2.6,
            track: None,
        };
        let mut v = Vehicle::new(dims, "sports", spec.mass, "", 0);
        let mut phys = CarPhysics::new(&v, spec);
        phys.reset(&mut v, &t, s, lat);
        let f = t.frame(s);
        v.vx = f.fx * 40.0;
        v.vz = f.fz * 40.0;
        for _ in 0..60 {
            phys.update(&mut v, &t, DT, &Input::default());
        }
        (hypot(v.vx, v.vz), phys.off_track.unwrap_or(0.0))
    };
    // Straight-ish spots 5 m off the tarmac, on dirt and on asphalt.
    let spot = |want: f64| -> usize {
        let mut s = 100;
        while s < t.n - 100 {
            if (t.k_smooth[s] as f64).abs() > 1.0 / 400.0
                || (t.wall_r[s] as f64) < t.hw[s] as f64 + 9.0
            {
                s += 7;
                continue;
            }
            let mut ok = true;
            let mut d = 0;
            while d < 45 && ok {
                let f = t.frame((s + d) as f64);
                let p = t.point_at((s + d) as f64, f.hw + 5.0);
                ok = (loose(p.x, p.z) - want).abs() < 0.05;
                d += 3;
            }
            if ok {
                return s;
            }
            s += 7;
        }
        panic!(
            "no {} run-off found",
            if want != 0.0 { "loose" } else { "paved" }
        );
    };
    let (s_dirt, s_paved) = (spot(1.0), spot(0.0));
    let dirt = run(s_dirt as f64, t.hw[s_dirt] as f64 + 5.0);
    let paved = run(s_paved as f64, t.hw[s_paved] as f64 + 5.0);
    let tarmac = run(s_dirt as f64, 0.0);
    let tarmac2 = run(s_paved as f64, 0.0);
    assert_eq!(tarmac.1, 0.0, "on the tarmac");
    assert!(dirt.1 > 0.95, "out on the dirt ({})", dirt.1);
    assert_eq!(paved.1, 0.0, "paved run-off is as good as the road");
    assert!(
        tarmac.0 - dirt.0 > 4.0,
        "after a second: {:.1} m/s on the tarmac, {:.1} on the dirt",
        tarmac.0,
        dirt.0
    );
    assert!(
        (tarmac2.0 - paved.0).abs() < 0.6,
        "{:.1} m/s on the tarmac, {:.1} on the paved run-off",
        tarmac2.0,
        paved.0
    );
}
