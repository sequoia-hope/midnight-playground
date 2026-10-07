//! Vehicle definitions as Rust data (SPEC 8.2). V1 has the rig's car: a
//! mid-weight rear-drive sports car with the Vento GT's mass and wheelbase,
//! tuned by physical reasoning (ride frequencies, damping ratios, brake
//! balance), not yet by driving.

use crate::math::Vec3;
use crate::tyre::BrushParams;
use crate::vehicle::{AeroDef, AxleDef, DriveDef, SuspensionDef, VehicleDef};

/// A street-legal performance tyre, 0.33 m radius.
pub const SPORT_TYRE: BrushParams = BrushParams {
    radius: 0.33,
    kz: 250_000.0,
    cz: 400.0,
    // C = 2·c_p·a² = 90 kN per unit slip, over a 9 cm half-patch.
    cp: 90_000.0 / (2.0 * 0.09 * 0.09),
    a: 0.09,
    mu0: 1.15,
    fz0: 3500.0,
    k_mu: 0.1,
    slide_ratio: 0.8,
    v_slide: 4.0,
    relax_lat: 0.4,
    relax_long: 0.15,
    damp_lat: 8000.0,
    damp_long: 3000.0,
    v_low: 2.0,
    c_rr: 0.012,
};

/// The test rig's car: 1350 kg, 2.6 m wheelbase, 52 % on the front axle,
/// centre of mass 0.5 m up, rear drive, 300 kW.
pub fn rig_car() -> VehicleDef {
    let l = 2.6;
    let front_share = 0.52;
    let cg_h = 0.5;
    let r = SPORT_TYRE.radius;
    let susp = |k: f64| SuspensionDef {
        k,
        free: 0.0,
        c_bump: 2200.0,
        c_rebound: 3600.0,
        travel_min: -0.09,
        travel_max: 0.08,
        stop_k: 300_000.0,
    };
    let axle = AxleDef {
        x: 0.0,
        half_track: 0.8,
        hub_y: r - cg_h,
        susp: susp(32_000.0),
        arb: 0.0,
        roll_centre: 0.05,
        steer_lock: 0.0,
        ackermann: 0.0,
        drive: 0.0,
        brake: 0.0,
        handbrake: 0.0,
        spin_inertia: 1.2,
        unsprung: 40.0,
        tyre: SPORT_TYRE,
    };
    let mut def = VehicleDef {
        name: "Rig GT",
        mass: 1350.0,
        inertia: Vec3::new(520.0, 2100.0, 1950.0),
        axles: vec![
            AxleDef {
                x: l * (1.0 - front_share),
                arb: 20_000.0,
                steer_lock: 0.6,
                ackermann: 0.5,
                brake: 2400.0,
                ..axle
            },
            AxleDef {
                x: -l * front_share,
                susp: susp(30_000.0),
                arb: 10_000.0,
                roll_centre: 0.1,
                drive: 1.0,
                brake: 1300.0,
                handbrake: 2500.0,
                ..axle
            },
        ],
        drive: DriveDef {
            max_torque: 4000.0,
            max_power: 300_000.0,
        },
        aero: AeroDef {
            rho: 1.225,
            cda: 0.65,
            cla_front: 0.3,
            cla_rear: 0.4,
        },
        steer_rate: 3.2,
        substeps: 5,
    };
    def.balance_springs();
    def
}
