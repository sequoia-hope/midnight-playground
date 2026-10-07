//! The rigid body and its quaternion: free fall, torque-free rotation, the
//! heading convention.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_math::kernel;
use mp_vdyn::body::RigidBody;
use mp_vdyn::{Quat, Vec3};

const H: f64 = 1.0 / 600.0;

#[test]
fn the_yaw_convention_is_the_games() {
    for yaw in [0.0, 0.3, -1.2, 2.9] {
        let f = Quat::from_yaw(yaw).rotate(Vec3::X);
        assert!((f.x - kernel::cos(yaw)).abs() < 1e-15);
        assert!((f.z - kernel::sin(yaw)).abs() < 1e-15);
        assert!(f.y.abs() < 1e-15);
        // Right = (−sin, 0, cos).
        let r = Quat::from_yaw(yaw).rotate(Vec3::Z);
        assert!((r.x + kernel::sin(yaw)).abs() < 1e-15);
        assert!((r.z - kernel::cos(yaw)).abs() < 1e-15);
    }
    // Rotation and its inverse.
    let q = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0).normalize(), 0.7);
    let v = Vec3::new(0.3, -1.1, 2.0);
    let back = q.inv_rotate(q.rotate(v));
    assert!((back - v).length() < 1e-14);
}

#[test]
fn free_fall_is_semi_implicit_euler() {
    let mut b = RigidBody::default();
    let n = 600;
    for _ in 0..n {
        b.kick(
            Vec3::new(0.0, -9.81, 0.0),
            Vec3::ZERO,
            Vec3::new(1.0, 1.0, 1.0),
            H,
        );
        b.drift(H);
    }
    // v = g·t exactly (up to rounding); y = −g·t²/2 with the scheme's
    // first-order lead of g·h·t/2.
    let t = n as f64 * H;
    assert!((b.vel.y + 9.81 * t).abs() < 1e-10);
    let exact = -0.5 * 9.81 * t * t;
    let lead = -0.5 * 9.81 * H * t;
    assert!((b.pos.y - (exact + lead)).abs() < 1e-9, "{}", b.pos.y);
}

#[test]
fn torque_free_rotation_keeps_momentum_and_a_unit_quaternion() {
    let inertia = Vec3::new(500.0, 2000.0, 1900.0);
    let mut b = RigidBody {
        rot: Quat::from_axis_angle(Vec3::new(0.2, 1.0, -0.4).normalize(), 0.5),
        ang_vel: Vec3::new(0.05, 1.5, -0.03),
        ..RigidBody::default()
    };
    let momentum = |b: &RigidBody| {
        let wb = b.rot.inv_rotate(b.ang_vel);
        b.rot.rotate(inertia.mul_elem(wb))
    };
    let l0 = momentum(&b);
    let e0 = b.kinetic_energy(1.0, inertia);
    for _ in 0..(600 * 20) {
        b.kick(Vec3::ZERO, Vec3::ZERO, inertia, H);
        b.drift(H);
        let q = b.rot;
        let n = q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z;
        assert!((n - 1.0).abs() < 1e-14);
    }
    let l = momentum(&b);
    assert!((l - l0).length() < 1e-3 * l0.length(), "{l:?} vs {l0:?}");
    let e = b.kinetic_energy(1.0, inertia);
    assert!((e - e0).abs() < 1e-3 * e0, "{e} vs {e0}");
}

#[test]
fn a_spin_about_up_turns_the_heading() {
    // ω = −1 rad/s about +y is a yaw rate of +1 (turning right).
    let mut b = RigidBody {
        rot: Quat::from_yaw(0.0),
        ang_vel: Vec3::new(0.0, -1.0, 0.0),
        ..RigidBody::default()
    };
    for _ in 0..60 {
        b.drift(H);
    }
    let f = b.rot.rotate(Vec3::X);
    let yaw = kernel::atan2(f.z, f.x);
    assert!((yaw - 0.1).abs() < 1e-6, "{yaw}");
}
