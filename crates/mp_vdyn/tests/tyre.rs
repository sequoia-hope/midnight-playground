//! The brush tyre against its formulas (SPEC 10, unit tests): the force
//! curve and its slope at zero, the sliding limit, combined slip on the
//! friction circle, load sensitivity, the relaxation response to a slip
//! step, the aligning moment, and the spring at a standstill.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_math::kernel;
use mp_vdyn::cars::SPORT_TYRE;
use mp_vdyn::tyre::{brush_force, pneumatic_trail, sliding_mu};
use mp_vdyn::{BrushTyre, FlatGround, HubState, Iso3, Quat, TyreOutput, Vec3};

const H: f64 = 1.0 / 600.0;

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-12)
}

/// A hub at the height that loads the tyre with `fz` (statically), moving
/// at `vx` forward and `vy` to the right, spinning at `omega`.
fn hub(fz: f64, vx: f64, vy: f64, omega: f64) -> HubState {
    let p = SPORT_TYRE;
    HubState {
        pose: Iso3 {
            pos: Vec3::new(0.0, p.radius - fz / p.kz, 0.0),
            rot: Quat::IDENTITY,
        },
        vel: Vec3::new(vx, 0.0, vy),
        ang_vel: Vec3::ZERO,
        omega,
    }
}

/// The free-rolling spin rate at `vx` under load `fz` (ω·r_e = V, with
/// the effective radius r_e = R − δ/3).
fn rolling(fz: f64, vx: f64) -> f64 {
    let p = SPORT_TYRE;
    vx / (p.radius - fz / p.kz / 3.0)
}

/// The tyre after `n` substeps with the hub held at `h`.
fn settle(t: &mut BrushTyre, h: &HubState, n: usize) -> TyreOutput {
    let g = FlatGround::default();
    let mut out = TyreOutput::default();
    for _ in 0..n {
        out = t.step(h, &g, H);
    }
    out
}

/// A tyre with the sliding drop and load sensitivity off, so its forces
/// are the plain brush curve.
fn plain() -> BrushTyre {
    let mut p = SPORT_TYRE;
    p.slide_ratio = 1.0;
    p.k_mu = 0.0;
    BrushTyre::new(p)
}

#[test]
fn slope_at_zero_is_the_slip_stiffness() {
    let c = SPORT_TYRE.stiffness();
    assert!(close(c, 90_000.0, 1e-12), "C = 2·c_p·a² = {c}");
    for s in [1e-7, 1e-6] {
        let (f, _) = brush_force(c, 1.15, 3500.0, s);
        assert!(close(f / s, c, 1e-4), "F/s = {} at s = {s}", f / s);
    }
}

#[test]
fn the_curve_rises_to_mu_fz_and_stays() {
    let (c, mu, fz) = (90_000.0, 1.1, 4000.0);
    let theta = c / (3.0 * mu * fz);
    let mut last = 0.0;
    for k in 0..=400 {
        let s = k as f64 * 0.001;
        let (f, l) = brush_force(c, mu, fz, s);
        assert!(f >= last, "monotonic at s = {s}");
        assert!(f <= mu * fz * (1.0 + 1e-12));
        assert!(close(l, theta * s, 1e-12) || s == 0.0);
        last = f;
    }
    // Exactly μFz from θs = 1 on.
    let (f, _) = brush_force(c, mu, fz, 1.0 / theta);
    assert!(close(f, mu * fz, 1e-12));
    assert_eq!(brush_force(c, mu, fz, 10.0).0, mu * fz);
    assert_eq!(brush_force(c, mu, 0.0, 0.1).0, 0.0);
}

#[test]
fn load_sensitivity() {
    let p = SPORT_TYRE;
    assert_eq!(p.mu_at(p.fz0), p.mu0);
    assert!(close(p.mu_at(2.0 * p.fz0), p.mu0 * (1.0 - p.k_mu), 1e-12));
    // The grip per unit load falls as the load rises.
    let mut t_light = BrushTyre::new(p);
    let mut t_heavy = BrushTyre::new(p);
    let light = settle(
        &mut t_light,
        &hub(2000.0, 20.0, -6.0, rolling(2000.0, 20.0)),
        600,
    );
    let heavy = settle(
        &mut t_heavy,
        &hub(6000.0, 20.0, -6.0, rolling(6000.0, 20.0)),
        600,
    );
    let k_light = light.info.fy / light.info.load;
    let k_heavy = heavy.info.fy / heavy.info.load;
    assert!(k_light > k_heavy && k_heavy > 0.0, "{k_light} {k_heavy}");
    // Floor: never below a fifth of μ0.
    assert!(close(p.mu_at(1e9), p.mu0 * 0.2, 1e-12));
}

#[test]
fn pure_side_slip_in_steady_state_is_the_brush_curve() {
    let p = SPORT_TYRE;
    let fz = 3500.0;
    for tan_a in [0.005, 0.02, 0.05, 0.1] {
        let vx = 20.0;
        let mut t = plain();
        let out = settle(&mut t, &hub(fz, vx, tan_a * vx, rolling(fz, vx)), 2000);
        let load = out.info.load;
        assert!(close(load, fz, 1e-6), "load {load}");
        // Steady state: the deflection is σ·tanα exactly.
        assert!(close(t.qy, p.relax_lat * tan_a, 1e-9), "qy {}", t.qy);
        let (f, _) = brush_force(p.stiffness(), p.mu0, load, tan_a);
        assert!(close(-out.info.fy, f, 1e-6), "{} vs {f}", out.info.fy);
        assert!(out.info.fx.abs() < 1e-6 * f);
    }
}

#[test]
fn combined_slip_lies_on_the_friction_circle() {
    let p = SPORT_TYRE;
    let fz = 3500.0;
    let vx = 20.0;
    // Sliding hard in every direction: |F| = μFz (the sliding drop off),
    // and the force opposes the slide.
    for k in 0..12 {
        let phi = k as f64 * core::f64::consts::PI / 6.0;
        let (cx, sy) = (kernel::cos(phi), kernel::sin(phi));
        let slip = 0.5;
        // A slide of `slip` relative to the rolling speed, in direction φ.
        let vr = vx / (1.0 + slip * cx);
        let mut t = plain();
        let out = settle(&mut t, &hub(fz, vx, vr * slip * sy, rolling(fz, vr)), 3000);
        let f = (out.info.fx * out.info.fx + out.info.fy * out.info.fy).sqrt();
        assert!(close(f, p.mu0 * out.info.load, 1e-6), "φ {phi}: {f}");
        let dot = out.info.fx * cx + out.info.fy * sy;
        assert!(dot < -0.999 * f, "φ {phi}: force opposes the slide");
    }
    // In the linear range, isotropic: the same force for the same theoretical
    // slip, longitudinal or lateral.
    let s = 0.01;
    let mut lon = plain();
    let vr = vx / (1.0 + s);
    let a = settle(&mut lon, &hub(fz, vx, 0.0, rolling(fz, vr)), 3000);
    let mut lat = plain();
    let b = settle(&mut lat, &hub(fz, vx, s * vx, rolling(fz, vx)), 3000);
    assert!(
        close(-a.info.fx, -b.info.fy, 1e-3),
        "{} {}",
        a.info.fx,
        b.info.fy
    );
}

#[test]
fn sliding_friction_falls_with_slide_speed() {
    assert_eq!(sliding_mu(1.2, 0.8, 4.0, 0.0), 1.2);
    assert!(close(sliding_mu(1.2, 0.8, 4.0, 4.0), 1.2 * 0.9, 1e-12));
    assert!(sliding_mu(1.2, 0.8, 4.0, 1e6) < 1.2 * 0.8 * 1.000001);
    // A locked wheel at speed slides on less than the peak.
    let p = SPORT_TYRE;
    let mut t = BrushTyre::new(p);
    let out = settle(&mut t, &hub(3500.0, 25.0, 0.0, 0.0), 3000);
    assert!(out.info.sliding == 1.0);
    let mu = -out.info.fx / out.info.load;
    assert!(mu < p.mu0 * 0.85 && mu > p.mu0 * p.slide_ratio, "μ {mu}");
}

#[test]
fn relaxation_follows_a_slip_step_with_time_constant_sigma_over_v() {
    let p = SPORT_TYRE;
    let vx = 20.0;
    let tan_a = 0.01;
    let tau = p.relax_lat / vx;
    let mut t = plain();
    let h = hub(3500.0, vx, tan_a * vx, rolling(3500.0, vx));
    let steady = p.relax_lat * tan_a;
    let n_tau = (tau / H).round() as usize;
    settle(&mut t, &h, n_tau);
    let frac = t.qy / steady;
    // 1 − 1/e = 0.632; implicit Euler at this step lags by about 2 %.
    assert!((0.60..0.64).contains(&frac), "after τ: {frac}");
    settle(&mut t, &h, 4 * n_tau);
    let frac = t.qy / steady;
    assert!((0.98..1.0).contains(&frac), "after 5τ: {frac}");
}

#[test]
fn the_aligning_moment_has_the_brush_trail() {
    let p = SPORT_TYRE;
    assert!(close(pneumatic_trail(p.a, 0.0), p.a / 3.0, 1e-12));
    assert_eq!(pneumatic_trail(p.a, 1.0), 0.0);
    assert_eq!(pneumatic_trail(p.a, 2.0), 0.0);
    let mut last = p.a / 3.0;
    for k in 1..100 {
        let t = pneumatic_trail(p.a, k as f64 / 100.0);
        assert!(t < last && t > 0.0);
        last = t;
    }
    // On the tyre: Mz = t·Fy, turning the wheel toward its path.
    let vx = 20.0;
    let mut t = plain();
    let out = settle(
        &mut t,
        &hub(3500.0, vx, 0.001 * vx, rolling(3500.0, vx)),
        2000,
    );
    let theta = p.stiffness() / (3.0 * p.mu0 * out.info.load);
    let trail = pneumatic_trail(p.a, theta * 0.001);
    assert!(close(out.info.aligning / out.info.fy, trail, 1e-6));
    assert!(close(trail, p.a / 3.0, 0.02));
    // Sliding to the right, the force points left and the moment turns
    // the wheel's nose right (about −y), toward the velocity.
    assert!(out.info.fy < 0.0 && out.info.aligning < 0.0);
}

#[test]
fn at_a_standstill_the_tyre_is_a_spring() {
    let p = SPORT_TYRE;
    let mut t = plain();
    let fz = 3500.0;
    // Push the patch 1 mm to the right over 0.1 s, then hold.
    let push = hub(fz, 0.0, 0.01, 0.0);
    settle(&mut t, &push, 60);
    let held = settle(&mut t, &hub(fz, 0.0, 0.0, 0.0), 600);
    // A spring of stiffness C/σ for small deflections (the brush curve's
    // own softening aside).
    assert!(close(t.qy, 0.001, 1e-9), "deflection {}", t.qy);
    let (f, _) = brush_force(p.stiffness(), p.mu0, held.info.load, 0.001 / p.relax_lat);
    assert!(close(-held.info.fy, f, 1e-9), "{} vs {f}", held.info.fy);
    assert!(close(f, p.stiffness() / p.relax_lat * 0.001, 0.02));
    // And it never creeps: the deflection holds without motion.
    let later = settle(&mut t, &hub(fz, 0.0, 0.0, 0.0), 6000);
    assert_eq!(later.info.fy, held.info.fy);
}

#[test]
fn vertical_load_is_the_carcass_spring() {
    let p = SPORT_TYRE;
    let mut t = BrushTyre::new(p);
    let out = settle(&mut t, &hub(4000.0, 0.0, 0.0, 0.0), 1);
    assert!(close(out.info.load, 4000.0, 1e-9));
    assert!(close(out.force.y, 4000.0, 1e-9));
    // Off the ground: nothing.
    let mut h = hub(4000.0, 0.0, 0.0, 0.0);
    h.pose.pos.y = p.radius + 0.01;
    let out = settle(&mut t, &h, 1);
    assert!(!out.info.in_contact && out.force == Vec3::ZERO);
}
