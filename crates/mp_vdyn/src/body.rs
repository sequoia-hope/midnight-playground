//! A rigid body with six degrees of freedom, integrated with semi-implicit
//! (symplectic) Euler: velocities from forces first, then the pose from the
//! new velocities, the orientation a unit quaternion renormalised every
//! step (SPEC 3, 4.1). Only `+ − × ÷` and `sqrt`.

use crate::math::{Quat, Vec3};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RigidBody {
    /// Centre of mass, world frame.
    pub pos: Vec3,
    /// Body to world.
    pub rot: Quat,
    /// Linear velocity of the centre of mass, world frame.
    pub vel: Vec3,
    /// Angular velocity, world frame.
    pub ang_vel: Vec3,
}

impl RigidBody {
    /// Velocity of the body's material point at `r` from the centre of mass
    /// (world frame offset).
    pub fn point_vel(&self, r: Vec3) -> Vec3 {
        self.vel + self.ang_vel.cross(r)
    }

    /// Updates the velocities from a linear acceleration (world) and a
    /// torque about the centre of mass (world), with the principal moments
    /// of inertia `inertia` in the body frame (Euler's equations, the
    /// gyroscopic term explicit).
    pub fn kick(&mut self, accel: Vec3, torque: Vec3, inertia: Vec3, h: f64) {
        self.vel += accel * h;
        let wb = self.rot.inv_rotate(self.ang_vel);
        let tb = self.rot.inv_rotate(torque);
        let lb = inertia.mul_elem(wb);
        let rhs = tb - wb.cross(lb);
        let wb = wb
            + Vec3::new(
                rhs.x / inertia.x * h,
                rhs.y / inertia.y * h,
                rhs.z / inertia.z * h,
            );
        self.ang_vel = self.rot.rotate(wb);
    }

    /// Moves the pose by the current velocities.
    pub fn drift(&mut self, h: f64) {
        self.pos += self.vel * h;
        self.rot = self.rot.integrate(self.ang_vel, h);
    }

    /// Kinetic energy, with the principal moments in the body frame.
    pub fn kinetic_energy(&self, mass: f64, inertia: Vec3) -> f64 {
        let wb = self.rot.inv_rotate(self.ang_vel);
        0.5 * mass * self.vel.length_sq() + 0.5 * wb.dot(inertia.mul_elem(wb))
    }

    pub fn hash_into(&self, out: &mut Vec<u8>) {
        self.pos.hash_into(out);
        self.rot.hash_into(out);
        self.vel.hash_into(out);
        self.ang_vel.hash_into(out);
    }
}
