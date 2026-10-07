//! Vectors, unit quaternions and poses in `f64`, with nothing but `+ − × ÷`
//! and `sqrt`, which IEEE 754 makes exact on every platform (SPEC 3). No
//! kernel calls are needed here; trigonometry happens in the callers,
//! through `mp_math::kernel`.
//!
//! Axes: world `y` is up. The body frame has `x` forward, `y` up and `z` to
//! the right, which is right-handed and matches the game's heading
//! convention (forward = (cos θ, 0, sin θ), right = (−sin θ, 0, cos θ)).

use core::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    pub const X: Vec3 = Vec3::new(1.0, 0.0, 0.0);
    pub const Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    pub const Z: Vec3 = Vec3::new(0.0, 0.0, 1.0);

    pub const fn new(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length_sq(self) -> f64 {
        self.dot(self)
    }

    pub fn length(self) -> f64 {
        self.length_sq().sqrt()
    }

    /// The unit vector, or zero for a zero vector.
    pub fn normalize(self) -> Vec3 {
        let l = self.length();
        if l > 0.0 {
            self * (1.0 / l)
        } else {
            Vec3::ZERO
        }
    }

    /// Component-wise product (a diagonal matrix times a vector).
    pub fn mul_elem(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    /// The part of `self` perpendicular to the unit vector `n`.
    pub fn reject(self, n: Vec3) -> Vec3 {
        self - n * self.dot(n)
    }

    pub fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }

    /// Appends the bits of each component, for state hashes.
    pub fn hash_into(self, out: &mut Vec<u8>) {
        for c in self.to_array() {
            out.extend_from_slice(&c.to_bits().to_le_bytes());
        }
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f64) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, o: Vec3) {
        *self = *self - o;
    }
}

/// A rotation as a unit quaternion `w + xi + yj + zk`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Quat {
    fn default() -> Quat {
        Quat::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Quat = Quat {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// The rotation by `angle` about the unit `axis`, from the half angle's
    /// cosine and sine (the caller takes them from the kernel).
    pub fn from_half_angle(axis: Vec3, cos_half: f64, sin_half: f64) -> Quat {
        Quat {
            w: cos_half,
            x: axis.x * sin_half,
            y: axis.y * sin_half,
            z: axis.z * sin_half,
        }
    }

    /// The rotation by `angle` about the unit `axis`.
    pub fn from_axis_angle(axis: Vec3, angle: f64) -> Quat {
        let h = angle * 0.5;
        Quat::from_half_angle(axis, mp_math::kernel::cos(h), mp_math::kernel::sin(h))
    }

    /// A heading in the game's convention: forward = (cos yaw, 0, sin yaw).
    /// That is a rotation by −yaw about +y.
    pub fn from_yaw(yaw: f64) -> Quat {
        Quat::from_axis_angle(Vec3::Y, -yaw)
    }

    pub fn conj(self) -> Quat {
        Quat {
            w: self.w,
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }

    pub fn normalize(self) -> Quat {
        let l = (self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        let k = 1.0 / l;
        Quat {
            w: self.w * k,
            x: self.x * k,
            y: self.y * k,
            z: self.z * k,
        }
    }

    /// Rotates `v` (body to world when `self` is the body's orientation).
    pub fn rotate(self, v: Vec3) -> Vec3 {
        // v' = v + 2w(q × v) + 2 q × (q × v), with q the vector part.
        let q = Vec3::new(self.x, self.y, self.z);
        let t = q.cross(v) * 2.0;
        v + t * self.w + q.cross(t)
    }

    /// Rotates `v` by the inverse (world to body).
    pub fn inv_rotate(self, v: Vec3) -> Vec3 {
        self.conj().rotate(v)
    }

    /// One step of `dq/dt = ½ (0, ω) q` with the world-frame angular
    /// velocity `w`, renormalised.
    pub fn integrate(self, w: Vec3, h: f64) -> Quat {
        let k = 0.5 * h;
        let dq = Quat {
            w: -(w.x * self.x + w.y * self.y + w.z * self.z),
            x: w.x * self.w + w.y * self.z - w.z * self.y,
            y: w.y * self.w + w.z * self.x - w.x * self.z,
            z: w.z * self.w + w.x * self.y - w.y * self.x,
        };
        Quat {
            w: self.w + dq.w * k,
            x: self.x + dq.x * k,
            y: self.y + dq.y * k,
            z: self.z + dq.z * k,
        }
        .normalize()
    }

    pub fn to_array(self) -> [f64; 4] {
        [self.x, self.y, self.z, self.w]
    }

    pub fn hash_into(self, out: &mut Vec<u8>) {
        for c in [self.w, self.x, self.y, self.z] {
            out.extend_from_slice(&c.to_bits().to_le_bytes());
        }
    }
}

impl Mul for Quat {
    type Output = Quat;
    fn mul(self, o: Quat) -> Quat {
        Quat {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }
}

/// A pose: a position and an orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iso3 {
    pub pos: Vec3,
    pub rot: Quat,
}

impl Iso3 {
    /// A point in this frame, in world coordinates.
    pub fn transform(&self, p: Vec3) -> Vec3 {
        self.pos + self.rot.rotate(p)
    }
}

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}
