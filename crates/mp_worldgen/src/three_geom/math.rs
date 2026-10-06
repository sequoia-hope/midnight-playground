//! The parts of three.js r180's math the geometry generators and the world
//! builders use: `Vector2`, `Vector3`, `Quaternion`, `Euler`, `Matrix3`,
//! `Matrix4`, `Box3` and `Sphere` (`src/math/*.js`).
//!
//! The types are `Copy` and their methods return the new value instead of
//! mutating `this`, so `a.clone().addScaledVector(v, s)` is
//! `a.add_scaled_vector(v, s)` and `v.normalize().multiplyScalar(r)` is
//! `v.normalize().multiply_scalar(r)`. The arithmetic in each method is
//! three's, in its order (`normalize` multiplies by the reciprocal of the
//! length, as `divideScalar` does). `+`, `-` and unary `-` are three's
//! `add`, `sub` and `negate`.

use core::f64::consts::PI;
use core::ops::{Add, Neg, Sub};

use mp_math::{js, kernel};

/// three's `MathUtils.clamp`: `Math.max(min, Math.min(max, value))`.
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    js::max(min, js::min(max, value))
}

// ── Vector2 ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector2 {
    pub x: f64,
    pub y: f64,
}

impl Vector2 {
    pub const fn new(x: f64, y: f64) -> Self {
        Vector2 { x, y }
    }

    pub fn add_scaled_vector(self, v: Vector2, s: f64) -> Self {
        Vector2::new(self.x + v.x * s, self.y + v.y * s)
    }

    pub fn multiply_scalar(self, s: f64) -> Self {
        Vector2::new(self.x * s, self.y * s)
    }

    pub fn divide_scalar(self, s: f64) -> Self {
        self.multiply_scalar(1.0 / s)
    }

    pub fn apply_matrix3(self, m: &Matrix3) -> Self {
        let (x, y) = (self.x, self.y);
        let e = &m.elements;
        Vector2::new(e[0] * x + e[3] * y + e[6], e[1] * x + e[4] * y + e[7])
    }

    pub fn dot(self, v: Vector2) -> f64 {
        self.x * v.x + self.y * v.y
    }

    pub fn cross(self, v: Vector2) -> f64 {
        self.x * v.y - self.y * v.x
    }

    pub fn length_sq(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn normalize(self) -> Self {
        self.divide_scalar(js::or(self.length(), 1.0))
    }

    /// The angle in radians with respect to the positive x axis.
    pub fn angle(self) -> f64 {
        kernel::atan2(-self.y, -self.x) + PI
    }

    pub fn distance_to(self, v: Vector2) -> f64 {
        self.distance_to_squared(v).sqrt()
    }

    pub fn distance_to_squared(self, v: Vector2) -> f64 {
        let dx = self.x - v.x;
        let dy = self.y - v.y;
        dx * dx + dy * dy
    }

    pub fn lerp(self, v: Vector2, alpha: f64) -> Self {
        Vector2::new(
            self.x + (v.x - self.x) * alpha,
            self.y + (v.y - self.y) * alpha,
        )
    }

    pub fn rotate_around(self, center: Vector2, angle: f64) -> Self {
        let c = kernel::cos(angle);
        let s = kernel::sin(angle);
        let x = self.x - center.x;
        let y = self.y - center.y;
        Vector2::new(x * c - y * s + center.x, x * s + y * c + center.y)
    }

    /// `equals`: `===` on both components (so -0 equals 0, NaN nothing).
    pub fn equals(self, v: Vector2) -> bool {
        v.x == self.x && v.y == self.y
    }
}

impl Add for Vector2 {
    type Output = Vector2;
    fn add(self, v: Vector2) -> Vector2 {
        Vector2::new(self.x + v.x, self.y + v.y)
    }
}

impl Sub for Vector2 {
    type Output = Vector2;
    fn sub(self, v: Vector2) -> Vector2 {
        Vector2::new(self.x - v.x, self.y - v.y)
    }
}

impl Neg for Vector2 {
    type Output = Vector2;
    fn neg(self) -> Vector2 {
        Vector2::new(-self.x, -self.y)
    }
}

// ── Vector3 ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vector3 { x, y, z }
    }

    pub const fn splat(s: f64) -> Self {
        Vector3 { x: s, y: s, z: s }
    }

    pub fn add_scalar(self, s: f64) -> Self {
        Vector3::new(self.x + s, self.y + s, self.z + s)
    }

    pub fn add_scaled_vector(self, v: Vector3, s: f64) -> Self {
        Vector3::new(self.x + v.x * s, self.y + v.y * s, self.z + v.z * s)
    }

    /// Component-wise product (`multiply`).
    pub fn multiply(self, v: Vector3) -> Self {
        Vector3::new(self.x * v.x, self.y * v.y, self.z * v.z)
    }

    pub fn multiply_scalar(self, s: f64) -> Self {
        Vector3::new(self.x * s, self.y * s, self.z * s)
    }

    pub fn divide_scalar(self, s: f64) -> Self {
        self.multiply_scalar(1.0 / s)
    }

    pub fn apply_euler(self, euler: &Euler) -> Self {
        self.apply_quaternion(Quaternion::from_euler(euler))
    }

    pub fn apply_axis_angle(self, axis: Vector3, angle: f64) -> Self {
        self.apply_quaternion(Quaternion::from_axis_angle(axis, angle))
    }

    pub fn apply_matrix3(self, m: &Matrix3) -> Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        Vector3::new(
            e[0] * x + e[3] * y + e[6] * z,
            e[1] * x + e[4] * y + e[7] * z,
            e[2] * x + e[5] * y + e[8] * z,
        )
    }

    pub fn apply_normal_matrix(self, m: &Matrix3) -> Self {
        self.apply_matrix3(m).normalize()
    }

    pub fn apply_matrix4(self, m: &Matrix4) -> Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        let w = 1.0 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
        Vector3::new(
            (e[0] * x + e[4] * y + e[8] * z + e[12]) * w,
            (e[1] * x + e[5] * y + e[9] * z + e[13]) * w,
            (e[2] * x + e[6] * y + e[10] * z + e[14]) * w,
        )
    }

    pub fn apply_quaternion(self, q: Quaternion) -> Self {
        let (vx, vy, vz) = (self.x, self.y, self.z);
        let (qx, qy, qz, qw) = (q.x, q.y, q.z, q.w);
        let tx = 2.0 * (qy * vz - qz * vy);
        let ty = 2.0 * (qz * vx - qx * vz);
        let tz = 2.0 * (qx * vy - qy * vx);
        Vector3::new(
            vx + qw * tx + qy * tz - qz * ty,
            vy + qw * ty + qz * tx - qx * tz,
            vz + qw * tz + qx * ty - qy * tx,
        )
    }

    pub fn transform_direction(self, m: &Matrix4) -> Self {
        let (x, y, z) = (self.x, self.y, self.z);
        let e = &m.elements;
        Vector3::new(
            e[0] * x + e[4] * y + e[8] * z,
            e[1] * x + e[5] * y + e[9] * z,
            e[2] * x + e[6] * y + e[10] * z,
        )
        .normalize()
    }

    pub fn min(self, v: Vector3) -> Self {
        Vector3::new(
            js::min(self.x, v.x),
            js::min(self.y, v.y),
            js::min(self.z, v.z),
        )
    }

    pub fn max(self, v: Vector3) -> Self {
        Vector3::new(
            js::max(self.x, v.x),
            js::max(self.y, v.y),
            js::max(self.z, v.z),
        )
    }

    pub fn dot(self, v: Vector3) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    pub fn length_sq(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn normalize(self) -> Self {
        self.divide_scalar(js::or(self.length(), 1.0))
    }

    pub fn set_length(self, length: f64) -> Self {
        self.normalize().multiply_scalar(length)
    }

    pub fn lerp(self, v: Vector3, alpha: f64) -> Self {
        Vector3::new(
            self.x + (v.x - self.x) * alpha,
            self.y + (v.y - self.y) * alpha,
            self.z + (v.z - self.z) * alpha,
        )
    }

    pub fn lerp_vectors(v1: Vector3, v2: Vector3, alpha: f64) -> Self {
        Vector3::new(
            v1.x + (v2.x - v1.x) * alpha,
            v1.y + (v2.y - v1.y) * alpha,
            v1.z + (v2.z - v1.z) * alpha,
        )
    }

    /// `crossVectors(this, v)`.
    pub fn cross(self, v: Vector3) -> Self {
        Vector3::cross_vectors(self, v)
    }

    pub fn cross_vectors(a: Vector3, b: Vector3) -> Self {
        let (ax, ay, az) = (a.x, a.y, a.z);
        let (bx, by, bz) = (b.x, b.y, b.z);
        Vector3::new(ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx)
    }

    pub fn angle_to(self, v: Vector3) -> f64 {
        let denominator = (self.length_sq() * v.length_sq()).sqrt();
        if denominator == 0.0 {
            return PI / 2.0;
        }
        let theta = self.dot(v) / denominator;
        kernel::acos(clamp(theta, -1.0, 1.0))
    }

    pub fn distance_to(self, v: Vector3) -> f64 {
        self.distance_to_squared(v).sqrt()
    }

    pub fn distance_to_squared(self, v: Vector3) -> f64 {
        let dx = self.x - v.x;
        let dy = self.y - v.y;
        let dz = self.z - v.z;
        dx * dx + dy * dy + dz * dz
    }

    pub fn set_from_matrix_position(m: &Matrix4) -> Self {
        let e = &m.elements;
        Vector3::new(e[12], e[13], e[14])
    }

    pub fn set_from_matrix_column(m: &Matrix4, index: usize) -> Self {
        Vector3::from_array(&m.elements, index * 4)
    }

    pub fn from_array(array: &[f64], offset: usize) -> Self {
        Vector3::new(array[offset], array[offset + 1], array[offset + 2])
    }

    /// `equals`: `===` on each component.
    pub fn equals(self, v: Vector3) -> bool {
        v.x == self.x && v.y == self.y && v.z == self.z
    }
}

impl Add for Vector3 {
    type Output = Vector3;
    fn add(self, v: Vector3) -> Vector3 {
        Vector3::new(self.x + v.x, self.y + v.y, self.z + v.z)
    }
}

impl Sub for Vector3 {
    type Output = Vector3;
    fn sub(self, v: Vector3) -> Vector3 {
        Vector3::new(self.x - v.x, self.y - v.y, self.z - v.z)
    }
}

impl Neg for Vector3 {
    type Output = Vector3;
    fn neg(self) -> Vector3 {
        Vector3::new(-self.x, -self.y, -self.z)
    }
}

// ── Euler ───────────────────────────────────────────────────────────────

/// The axis order of an [`Euler`]; three's default is `XYZ`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EulerOrder {
    #[default]
    XYZ,
    YXZ,
    ZXY,
    ZYX,
    YZX,
    XZY,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Euler {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub order: EulerOrder,
}

impl Euler {
    /// `new Euler(x, y, z)`, order `XYZ`.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Euler {
            x,
            y,
            z,
            order: EulerOrder::XYZ,
        }
    }

    pub const fn with_order(x: f64, y: f64, z: f64, order: EulerOrder) -> Self {
        Euler { x, y, z, order }
    }
}

// ── Quaternion ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Default for Quaternion {
    fn default() -> Self {
        Quaternion::IDENTITY
    }
}

impl Quaternion {
    pub const IDENTITY: Quaternion = Quaternion {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Quaternion { x, y, z, w }
    }

    /// `setFromEuler`.
    pub fn from_euler(euler: &Euler) -> Self {
        let (x, y, z) = (euler.x, euler.y, euler.z);
        let c1 = kernel::cos(x / 2.0);
        let c2 = kernel::cos(y / 2.0);
        let c3 = kernel::cos(z / 2.0);
        let s1 = kernel::sin(x / 2.0);
        let s2 = kernel::sin(y / 2.0);
        let s3 = kernel::sin(z / 2.0);
        match euler.order {
            EulerOrder::XYZ => Quaternion::new(
                s1 * c2 * c3 + c1 * s2 * s3,
                c1 * s2 * c3 - s1 * c2 * s3,
                c1 * c2 * s3 + s1 * s2 * c3,
                c1 * c2 * c3 - s1 * s2 * s3,
            ),
            EulerOrder::YXZ => Quaternion::new(
                s1 * c2 * c3 + c1 * s2 * s3,
                c1 * s2 * c3 - s1 * c2 * s3,
                c1 * c2 * s3 - s1 * s2 * c3,
                c1 * c2 * c3 + s1 * s2 * s3,
            ),
            EulerOrder::ZXY => Quaternion::new(
                s1 * c2 * c3 - c1 * s2 * s3,
                c1 * s2 * c3 + s1 * c2 * s3,
                c1 * c2 * s3 + s1 * s2 * c3,
                c1 * c2 * c3 - s1 * s2 * s3,
            ),
            EulerOrder::ZYX => Quaternion::new(
                s1 * c2 * c3 - c1 * s2 * s3,
                c1 * s2 * c3 + s1 * c2 * s3,
                c1 * c2 * s3 - s1 * s2 * c3,
                c1 * c2 * c3 + s1 * s2 * s3,
            ),
            EulerOrder::YZX => Quaternion::new(
                s1 * c2 * c3 + c1 * s2 * s3,
                c1 * s2 * c3 + s1 * c2 * s3,
                c1 * c2 * s3 - s1 * s2 * c3,
                c1 * c2 * c3 - s1 * s2 * s3,
            ),
            EulerOrder::XZY => Quaternion::new(
                s1 * c2 * c3 - c1 * s2 * s3,
                c1 * s2 * c3 - s1 * c2 * s3,
                c1 * c2 * s3 + s1 * s2 * c3,
                c1 * c2 * c3 + s1 * s2 * s3,
            ),
        }
    }

    /// `setFromAxisAngle` (the axis is assumed normalised).
    pub fn from_axis_angle(axis: Vector3, angle: f64) -> Self {
        let half_angle = angle / 2.0;
        let s = kernel::sin(half_angle);
        Quaternion::new(axis.x * s, axis.y * s, axis.z * s, kernel::cos(half_angle))
    }

    /// `setFromRotationMatrix` (the upper 3×3 is a pure rotation).
    pub fn from_rotation_matrix(m: &Matrix4) -> Self {
        let te = &m.elements;
        let (m11, m12, m13) = (te[0], te[4], te[8]);
        let (m21, m22, m23) = (te[1], te[5], te[9]);
        let (m31, m32, m33) = (te[2], te[6], te[10]);
        let trace = m11 + m22 + m33;
        if trace > 0.0 {
            let s = 0.5 / (trace + 1.0).sqrt();
            Quaternion::new((m32 - m23) * s, (m13 - m31) * s, (m21 - m12) * s, 0.25 / s)
        } else if m11 > m22 && m11 > m33 {
            let s = 2.0 * (1.0 + m11 - m22 - m33).sqrt();
            Quaternion::new(0.25 * s, (m12 + m21) / s, (m13 + m31) / s, (m32 - m23) / s)
        } else if m22 > m33 {
            let s = 2.0 * (1.0 + m22 - m11 - m33).sqrt();
            Quaternion::new((m12 + m21) / s, 0.25 * s, (m23 + m32) / s, (m13 - m31) / s)
        } else {
            let s = 2.0 * (1.0 + m33 - m11 - m22).sqrt();
            Quaternion::new((m13 + m31) / s, (m23 + m32) / s, 0.25 * s, (m21 - m12) / s)
        }
    }

    /// `setFromUnitVectors` (both assumed normalised).
    pub fn from_unit_vectors(v_from: Vector3, v_to: Vector3) -> Self {
        let mut r = v_from.dot(v_to) + 1.0;
        let q = if r < 1e-8 {
            // the epsilon value has been discussed in #31286
            r = 0.0;
            if v_from.x.abs() > v_from.z.abs() {
                Quaternion::new(-v_from.y, v_from.x, 0.0, r)
            } else {
                Quaternion::new(0.0, -v_from.z, v_from.y, r)
            }
        } else {
            Quaternion::new(
                v_from.y * v_to.z - v_from.z * v_to.y,
                v_from.z * v_to.x - v_from.x * v_to.z,
                v_from.x * v_to.y - v_from.y * v_to.x,
                r,
            )
        };
        q.normalize()
    }

    pub fn angle_to(self, q: Quaternion) -> f64 {
        2.0 * kernel::acos(clamp(self.dot(q), -1.0, 1.0).abs())
    }

    /// `invert` (= `conjugate`, for a unit quaternion).
    pub fn invert(self) -> Self {
        self.conjugate()
    }

    pub fn conjugate(self) -> Self {
        Quaternion::new(-self.x, -self.y, -self.z, self.w)
    }

    pub fn dot(self, v: Quaternion) -> f64 {
        self.x * v.x + self.y * v.y + self.z * v.z + self.w * v.w
    }

    pub fn length_sq(self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt()
    }

    pub fn normalize(self) -> Self {
        let mut l = self.length();
        if l == 0.0 {
            Quaternion::IDENTITY
        } else {
            l = 1.0 / l;
            Quaternion::new(self.x * l, self.y * l, self.z * l, self.w * l)
        }
    }

    /// `this × q`.
    pub fn multiply(self, q: Quaternion) -> Self {
        Quaternion::multiply_quaternions(self, q)
    }

    /// `q × this`.
    pub fn premultiply(self, q: Quaternion) -> Self {
        Quaternion::multiply_quaternions(q, self)
    }

    pub fn multiply_quaternions(a: Quaternion, b: Quaternion) -> Self {
        let (qax, qay, qaz, qaw) = (a.x, a.y, a.z, a.w);
        let (qbx, qby, qbz, qbw) = (b.x, b.y, b.z, b.w);
        Quaternion::new(
            qax * qbw + qaw * qbx + qay * qbz - qaz * qby,
            qay * qbw + qaw * qby + qaz * qbx - qax * qbz,
            qaz * qbw + qaw * qbz + qax * qby - qay * qbx,
            qaw * qbw - qax * qbx - qay * qby - qaz * qbz,
        )
    }

    pub fn slerp(self, qb: Quaternion, t: f64) -> Self {
        if t == 0.0 {
            return self;
        }
        if t == 1.0 {
            return qb;
        }
        let (x, y, z, w) = (self.x, self.y, self.z, self.w);
        let mut cos_half_theta = w * qb.w + x * qb.x + y * qb.y + z * qb.z;
        let mut this = if cos_half_theta < 0.0 {
            cos_half_theta = -cos_half_theta;
            Quaternion::new(-qb.x, -qb.y, -qb.z, -qb.w)
        } else {
            qb
        };
        if cos_half_theta >= 1.0 {
            return Quaternion::new(x, y, z, w);
        }
        let sqr_sin_half_theta = 1.0 - cos_half_theta * cos_half_theta;
        if sqr_sin_half_theta <= f64::EPSILON {
            let s = 1.0 - t;
            this.w = s * w + t * this.w;
            this.x = s * x + t * this.x;
            this.y = s * y + t * this.y;
            this.z = s * z + t * this.z;
            return this.normalize();
        }
        let sin_half_theta = sqr_sin_half_theta.sqrt();
        let half_theta = kernel::atan2(sin_half_theta, cos_half_theta);
        let ratio_a = kernel::sin((1.0 - t) * half_theta) / sin_half_theta;
        let ratio_b = kernel::sin(t * half_theta) / sin_half_theta;
        Quaternion::new(
            x * ratio_a + this.x * ratio_b,
            y * ratio_a + this.y * ratio_b,
            z * ratio_a + this.z * ratio_b,
            w * ratio_a + this.w * ratio_b,
        )
    }
}

// ── Matrix3 ─────────────────────────────────────────────────────────────

/// A 3×3 matrix, `elements` column-major as in three.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix3 {
    pub elements: [f64; 9],
}

impl Default for Matrix3 {
    fn default() -> Self {
        Matrix3::IDENTITY
    }
}

impl Matrix3 {
    pub const IDENTITY: Matrix3 = Matrix3 {
        elements: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };

    /// `set(n11, n12, ...)`: arguments row by row.
    #[allow(clippy::too_many_arguments)]
    pub const fn set(
        n11: f64,
        n12: f64,
        n13: f64,
        n21: f64,
        n22: f64,
        n23: f64,
        n31: f64,
        n32: f64,
        n33: f64,
    ) -> Self {
        Matrix3 {
            elements: [n11, n21, n31, n12, n22, n32, n13, n23, n33],
        }
    }

    pub fn set_from_matrix4(m: &Matrix4) -> Self {
        let me = &m.elements;
        Matrix3::set(
            me[0], me[4], me[8], me[1], me[5], me[9], me[2], me[6], me[10],
        )
    }

    pub fn multiply_matrices(a: &Matrix3, b: &Matrix3) -> Self {
        let ae = &a.elements;
        let be = &b.elements;
        let (a11, a12, a13) = (ae[0], ae[3], ae[6]);
        let (a21, a22, a23) = (ae[1], ae[4], ae[7]);
        let (a31, a32, a33) = (ae[2], ae[5], ae[8]);
        let (b11, b12, b13) = (be[0], be[3], be[6]);
        let (b21, b22, b23) = (be[1], be[4], be[7]);
        let (b31, b32, b33) = (be[2], be[5], be[8]);
        let mut te = [0.0; 9];
        te[0] = a11 * b11 + a12 * b21 + a13 * b31;
        te[3] = a11 * b12 + a12 * b22 + a13 * b32;
        te[6] = a11 * b13 + a12 * b23 + a13 * b33;
        te[1] = a21 * b11 + a22 * b21 + a23 * b31;
        te[4] = a21 * b12 + a22 * b22 + a23 * b32;
        te[7] = a21 * b13 + a22 * b23 + a23 * b33;
        te[2] = a31 * b11 + a32 * b21 + a33 * b31;
        te[5] = a31 * b12 + a32 * b22 + a33 * b32;
        te[8] = a31 * b13 + a32 * b23 + a33 * b33;
        Matrix3 { elements: te }
    }

    pub fn determinant(&self) -> f64 {
        let te = &self.elements;
        let (a, b, c) = (te[0], te[1], te[2]);
        let (d, e, f) = (te[3], te[4], te[5]);
        let (g, h, i) = (te[6], te[7], te[8]);
        a * e * i - a * f * h - b * d * i + b * f * g + c * d * h - c * e * g
    }

    pub fn invert(&self) -> Self {
        let te = &self.elements;
        let (n11, n21, n31) = (te[0], te[1], te[2]);
        let (n12, n22, n32) = (te[3], te[4], te[5]);
        let (n13, n23, n33) = (te[6], te[7], te[8]);
        let t11 = n33 * n22 - n32 * n23;
        let t12 = n32 * n13 - n33 * n12;
        let t13 = n23 * n12 - n22 * n13;
        let det = n11 * t11 + n21 * t12 + n31 * t13;
        if det == 0.0 {
            return Matrix3 { elements: [0.0; 9] };
        }
        let det_inv = 1.0 / det;
        Matrix3 {
            elements: [
                t11 * det_inv,
                (n31 * n23 - n33 * n21) * det_inv,
                (n32 * n21 - n31 * n22) * det_inv,
                t12 * det_inv,
                (n33 * n11 - n31 * n13) * det_inv,
                (n31 * n12 - n32 * n11) * det_inv,
                t13 * det_inv,
                (n21 * n13 - n23 * n11) * det_inv,
                (n22 * n11 - n21 * n12) * det_inv,
            ],
        }
    }

    pub fn transpose(&self) -> Self {
        let mut m = self.elements;
        m.swap(1, 3);
        m.swap(2, 6);
        m.swap(5, 7);
        Matrix3 { elements: m }
    }

    /// `getNormalMatrix(matrix4)`: the inverse transpose of its upper 3×3.
    pub fn get_normal_matrix(m: &Matrix4) -> Self {
        Matrix3::set_from_matrix4(m).invert().transpose()
    }
}

// ── Matrix4 ─────────────────────────────────────────────────────────────

/// A 4×4 matrix, `elements` column-major as in three.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix4 {
    pub elements: [f64; 16],
}

impl Default for Matrix4 {
    fn default() -> Self {
        Matrix4::IDENTITY
    }
}

impl Matrix4 {
    pub const IDENTITY: Matrix4 = Matrix4 {
        elements: [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    };

    /// `set(n11, n12, ...)`: arguments row by row.
    #[allow(clippy::too_many_arguments)]
    pub const fn set(
        n11: f64,
        n12: f64,
        n13: f64,
        n14: f64,
        n21: f64,
        n22: f64,
        n23: f64,
        n24: f64,
        n31: f64,
        n32: f64,
        n33: f64,
        n34: f64,
        n41: f64,
        n42: f64,
        n43: f64,
        n44: f64,
    ) -> Self {
        Matrix4 {
            elements: [
                n11, n21, n31, n41, n12, n22, n32, n42, n13, n23, n33, n43, n14, n24, n34, n44,
            ],
        }
    }

    pub fn make_basis(x_axis: Vector3, y_axis: Vector3, z_axis: Vector3) -> Self {
        Matrix4::set(
            x_axis.x, y_axis.x, z_axis.x, 0.0, x_axis.y, y_axis.y, z_axis.y, 0.0, x_axis.z,
            y_axis.z, z_axis.z, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn extract_rotation(m: &Matrix4) -> Self {
        let me = &m.elements;
        let scale_x = 1.0 / Vector3::set_from_matrix_column(m, 0).length();
        let scale_y = 1.0 / Vector3::set_from_matrix_column(m, 1).length();
        let scale_z = 1.0 / Vector3::set_from_matrix_column(m, 2).length();
        Matrix4 {
            elements: [
                me[0] * scale_x,
                me[1] * scale_x,
                me[2] * scale_x,
                0.0,
                me[4] * scale_y,
                me[5] * scale_y,
                me[6] * scale_y,
                0.0,
                me[8] * scale_z,
                me[9] * scale_z,
                me[10] * scale_z,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
            ],
        }
    }

    /// `makeRotationFromEuler`.
    pub fn make_rotation_from_euler(euler: &Euler) -> Self {
        let mut te = [0.0; 16];
        let (x, y, z) = (euler.x, euler.y, euler.z);
        let a = kernel::cos(x);
        let b = kernel::sin(x);
        let c = kernel::cos(y);
        let d = kernel::sin(y);
        let e = kernel::cos(z);
        let f = kernel::sin(z);
        match euler.order {
            EulerOrder::XYZ => {
                let (ae, af, be, bf) = (a * e, a * f, b * e, b * f);
                te[0] = c * e;
                te[4] = -c * f;
                te[8] = d;
                te[1] = af + be * d;
                te[5] = ae - bf * d;
                te[9] = -b * c;
                te[2] = bf - ae * d;
                te[6] = be + af * d;
                te[10] = a * c;
            }
            EulerOrder::YXZ => {
                let (ce, cf, de, df) = (c * e, c * f, d * e, d * f);
                te[0] = ce + df * b;
                te[4] = de * b - cf;
                te[8] = a * d;
                te[1] = a * f;
                te[5] = a * e;
                te[9] = -b;
                te[2] = cf * b - de;
                te[6] = df + ce * b;
                te[10] = a * c;
            }
            EulerOrder::ZXY => {
                let (ce, cf, de, df) = (c * e, c * f, d * e, d * f);
                te[0] = ce - df * b;
                te[4] = -a * f;
                te[8] = de + cf * b;
                te[1] = cf + de * b;
                te[5] = a * e;
                te[9] = df - ce * b;
                te[2] = -a * d;
                te[6] = b;
                te[10] = a * c;
            }
            EulerOrder::ZYX => {
                let (ae, af, be, bf) = (a * e, a * f, b * e, b * f);
                te[0] = c * e;
                te[4] = be * d - af;
                te[8] = ae * d + bf;
                te[1] = c * f;
                te[5] = bf * d + ae;
                te[9] = af * d - be;
                te[2] = -d;
                te[6] = b * c;
                te[10] = a * c;
            }
            EulerOrder::YZX => {
                let (ac, ad, bc, bd) = (a * c, a * d, b * c, b * d);
                te[0] = c * e;
                te[4] = bd - ac * f;
                te[8] = bc * f + ad;
                te[1] = f;
                te[5] = a * e;
                te[9] = -b * e;
                te[2] = -d * e;
                te[6] = ad * f + bc;
                te[10] = ac - bd * f;
            }
            EulerOrder::XZY => {
                let (ac, ad, bc, bd) = (a * c, a * d, b * c, b * d);
                te[0] = c * e;
                te[4] = -f;
                te[8] = d * e;
                te[1] = ac * f + bd;
                te[5] = a * e;
                te[9] = ad * f - bc;
                te[2] = bc * f - ad;
                te[6] = b * e;
                te[10] = bd * f + ac;
            }
        }
        // te[3], te[7], te[11], te[12], te[13], te[14] stay 0.
        te[15] = 1.0;
        Matrix4 { elements: te }
    }

    /// `makeRotationFromQuaternion`: `compose(0, q, 1)`.
    pub fn make_rotation_from_quaternion(q: Quaternion) -> Self {
        Matrix4::compose(Vector3::new(0.0, 0.0, 0.0), q, Vector3::splat(1.0))
    }

    /// `lookAt(eye, target, up)`: writes only the rotation part of `self`,
    /// as three does.
    pub fn look_at(&mut self, eye: Vector3, target: Vector3, up: Vector3) {
        let te = &mut self.elements;
        let mut z = eye - target;
        if z.length_sq() == 0.0 {
            z.z = 1.0;
        }
        z = z.normalize();
        let mut x = Vector3::cross_vectors(up, z);
        if x.length_sq() == 0.0 {
            if up.z.abs() == 1.0 {
                z.x += 0.0001;
            } else {
                z.z += 0.0001;
            }
            z = z.normalize();
            x = Vector3::cross_vectors(up, z);
        }
        x = x.normalize();
        let y = Vector3::cross_vectors(z, x);
        te[0] = x.x;
        te[4] = y.x;
        te[8] = z.x;
        te[1] = x.y;
        te[5] = y.y;
        te[9] = z.y;
        te[2] = x.z;
        te[6] = y.z;
        te[10] = z.z;
    }

    /// `this × m`.
    pub fn multiply(&self, m: &Matrix4) -> Self {
        Matrix4::multiply_matrices(self, m)
    }

    /// `m × this`.
    pub fn premultiply(&self, m: &Matrix4) -> Self {
        Matrix4::multiply_matrices(m, self)
    }

    pub fn multiply_matrices(a: &Matrix4, b: &Matrix4) -> Self {
        let ae = &a.elements;
        let be = &b.elements;
        let (a11, a12, a13, a14) = (ae[0], ae[4], ae[8], ae[12]);
        let (a21, a22, a23, a24) = (ae[1], ae[5], ae[9], ae[13]);
        let (a31, a32, a33, a34) = (ae[2], ae[6], ae[10], ae[14]);
        let (a41, a42, a43, a44) = (ae[3], ae[7], ae[11], ae[15]);
        let (b11, b12, b13, b14) = (be[0], be[4], be[8], be[12]);
        let (b21, b22, b23, b24) = (be[1], be[5], be[9], be[13]);
        let (b31, b32, b33, b34) = (be[2], be[6], be[10], be[14]);
        let (b41, b42, b43, b44) = (be[3], be[7], be[11], be[15]);
        let mut te = [0.0; 16];
        te[0] = a11 * b11 + a12 * b21 + a13 * b31 + a14 * b41;
        te[4] = a11 * b12 + a12 * b22 + a13 * b32 + a14 * b42;
        te[8] = a11 * b13 + a12 * b23 + a13 * b33 + a14 * b43;
        te[12] = a11 * b14 + a12 * b24 + a13 * b34 + a14 * b44;
        te[1] = a21 * b11 + a22 * b21 + a23 * b31 + a24 * b41;
        te[5] = a21 * b12 + a22 * b22 + a23 * b32 + a24 * b42;
        te[9] = a21 * b13 + a22 * b23 + a23 * b33 + a24 * b43;
        te[13] = a21 * b14 + a22 * b24 + a23 * b34 + a24 * b44;
        te[2] = a31 * b11 + a32 * b21 + a33 * b31 + a34 * b41;
        te[6] = a31 * b12 + a32 * b22 + a33 * b32 + a34 * b42;
        te[10] = a31 * b13 + a32 * b23 + a33 * b33 + a34 * b43;
        te[14] = a31 * b14 + a32 * b24 + a33 * b34 + a34 * b44;
        te[3] = a41 * b11 + a42 * b21 + a43 * b31 + a44 * b41;
        te[7] = a41 * b12 + a42 * b22 + a43 * b32 + a44 * b42;
        te[11] = a41 * b13 + a42 * b23 + a43 * b33 + a44 * b43;
        te[15] = a41 * b14 + a42 * b24 + a43 * b34 + a44 * b44;
        Matrix4 { elements: te }
    }

    pub fn multiply_scalar(&self, s: f64) -> Self {
        let mut te = self.elements;
        for v in te.iter_mut() {
            *v *= s;
        }
        Matrix4 { elements: te }
    }

    pub fn determinant(&self) -> f64 {
        let te = &self.elements;
        let (n11, n12, n13, n14) = (te[0], te[4], te[8], te[12]);
        let (n21, n22, n23, n24) = (te[1], te[5], te[9], te[13]);
        let (n31, n32, n33, n34) = (te[2], te[6], te[10], te[14]);
        let (n41, n42, n43, n44) = (te[3], te[7], te[11], te[15]);
        n41 * (n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33
            + n12 * n24 * n33
            + n13 * n22 * n34
            - n12 * n23 * n34)
            + n42
                * (n11 * n23 * n34 - n11 * n24 * n33 + n14 * n21 * n33 - n13 * n21 * n34
                    + n13 * n24 * n31
                    - n14 * n23 * n31)
            + n43
                * (n11 * n24 * n32 - n11 * n22 * n34 - n14 * n21 * n32
                    + n12 * n21 * n34
                    + n14 * n22 * n31
                    - n12 * n24 * n31)
            + n44
                * (-n13 * n22 * n31 - n11 * n23 * n32 + n11 * n22 * n33 + n13 * n21 * n32
                    - n12 * n21 * n33
                    + n12 * n23 * n31)
    }

    pub fn transpose(&self) -> Self {
        let mut te = self.elements;
        te.swap(1, 4);
        te.swap(2, 8);
        te.swap(6, 9);
        te.swap(3, 12);
        te.swap(7, 13);
        te.swap(11, 14);
        Matrix4 { elements: te }
    }

    pub fn set_position(&self, x: f64, y: f64, z: f64) -> Self {
        let mut te = self.elements;
        te[12] = x;
        te[13] = y;
        te[14] = z;
        Matrix4 { elements: te }
    }

    pub fn invert(&self) -> Self {
        let te = &self.elements;
        let (n11, n21, n31, n41) = (te[0], te[1], te[2], te[3]);
        let (n12, n22, n32, n42) = (te[4], te[5], te[6], te[7]);
        let (n13, n23, n33, n43) = (te[8], te[9], te[10], te[11]);
        let (n14, n24, n34, n44) = (te[12], te[13], te[14], te[15]);
        let t11 =
            n23 * n34 * n42 - n24 * n33 * n42 + n24 * n32 * n43 - n22 * n34 * n43 - n23 * n32 * n44
                + n22 * n33 * n44;
        let t12 =
            n14 * n33 * n42 - n13 * n34 * n42 - n14 * n32 * n43 + n12 * n34 * n43 + n13 * n32 * n44
                - n12 * n33 * n44;
        let t13 =
            n13 * n24 * n42 - n14 * n23 * n42 + n14 * n22 * n43 - n12 * n24 * n43 - n13 * n22 * n44
                + n12 * n23 * n44;
        let t14 =
            n14 * n23 * n32 - n13 * n24 * n32 - n14 * n22 * n33 + n12 * n24 * n33 + n13 * n22 * n34
                - n12 * n23 * n34;
        let det = n11 * t11 + n21 * t12 + n31 * t13 + n41 * t14;
        if det == 0.0 {
            return Matrix4 {
                elements: [0.0; 16],
            };
        }
        let det_inv = 1.0 / det;
        let mut o = [0.0; 16];
        o[0] = t11 * det_inv;
        o[1] = (n24 * n33 * n41 - n23 * n34 * n41 - n24 * n31 * n43
            + n21 * n34 * n43
            + n23 * n31 * n44
            - n21 * n33 * n44)
            * det_inv;
        o[2] = (n22 * n34 * n41 - n24 * n32 * n41 + n24 * n31 * n42
            - n21 * n34 * n42
            - n22 * n31 * n44
            + n21 * n32 * n44)
            * det_inv;
        o[3] = (n23 * n32 * n41 - n22 * n33 * n41 - n23 * n31 * n42
            + n21 * n33 * n42
            + n22 * n31 * n43
            - n21 * n32 * n43)
            * det_inv;
        o[4] = t12 * det_inv;
        o[5] = (n13 * n34 * n41 - n14 * n33 * n41 + n14 * n31 * n43
            - n11 * n34 * n43
            - n13 * n31 * n44
            + n11 * n33 * n44)
            * det_inv;
        o[6] = (n14 * n32 * n41 - n12 * n34 * n41 - n14 * n31 * n42
            + n11 * n34 * n42
            + n12 * n31 * n44
            - n11 * n32 * n44)
            * det_inv;
        o[7] = (n12 * n33 * n41 - n13 * n32 * n41 + n13 * n31 * n42
            - n11 * n33 * n42
            - n12 * n31 * n43
            + n11 * n32 * n43)
            * det_inv;
        o[8] = t13 * det_inv;
        o[9] = (n14 * n23 * n41 - n13 * n24 * n41 - n14 * n21 * n43
            + n11 * n24 * n43
            + n13 * n21 * n44
            - n11 * n23 * n44)
            * det_inv;
        o[10] = (n12 * n24 * n41 - n14 * n22 * n41 + n14 * n21 * n42
            - n11 * n24 * n42
            - n12 * n21 * n44
            + n11 * n22 * n44)
            * det_inv;
        o[11] = (n13 * n22 * n41 - n12 * n23 * n41 - n13 * n21 * n42
            + n11 * n23 * n42
            + n12 * n21 * n43
            - n11 * n22 * n43)
            * det_inv;
        o[12] = t14 * det_inv;
        o[13] = (n13 * n24 * n31 - n14 * n23 * n31 + n14 * n21 * n33
            - n11 * n24 * n33
            - n13 * n21 * n34
            + n11 * n23 * n34)
            * det_inv;
        o[14] = (n14 * n22 * n31 - n12 * n24 * n31 - n14 * n21 * n32
            + n11 * n24 * n32
            + n12 * n21 * n34
            - n11 * n22 * n34)
            * det_inv;
        o[15] = (n12 * n23 * n31 - n13 * n22 * n31 + n13 * n21 * n32
            - n11 * n23 * n32
            - n12 * n21 * n33
            + n11 * n22 * n33)
            * det_inv;
        Matrix4 { elements: o }
    }

    pub fn make_translation(x: f64, y: f64, z: f64) -> Self {
        Matrix4::set(
            1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, y, 0.0, 0.0, 1.0, z, 0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn make_rotation_x(theta: f64) -> Self {
        let c = kernel::cos(theta);
        let s = kernel::sin(theta);
        Matrix4::set(
            1.0, 0.0, 0.0, 0.0, 0.0, c, -s, 0.0, 0.0, s, c, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn make_rotation_y(theta: f64) -> Self {
        let c = kernel::cos(theta);
        let s = kernel::sin(theta);
        Matrix4::set(
            c, 0.0, s, 0.0, 0.0, 1.0, 0.0, 0.0, -s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn make_rotation_z(theta: f64) -> Self {
        let c = kernel::cos(theta);
        let s = kernel::sin(theta);
        Matrix4::set(
            c, -s, 0.0, 0.0, s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    /// `makeRotationAxis` (the axis is assumed normalised).
    pub fn make_rotation_axis(axis: Vector3, angle: f64) -> Self {
        // Based on http://www.gamedev.net/reference/articles/article1199.asp
        let c = kernel::cos(angle);
        let s = kernel::sin(angle);
        let t = 1.0 - c;
        let (x, y, z) = (axis.x, axis.y, axis.z);
        let tx = t * x;
        let ty = t * y;
        Matrix4::set(
            tx * x + c,
            tx * y - s * z,
            tx * z + s * y,
            0.0,
            tx * y + s * z,
            ty * y + c,
            ty * z - s * x,
            0.0,
            tx * z - s * y,
            ty * z + s * x,
            t * z * z + c,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }

    pub fn make_scale(x: f64, y: f64, z: f64) -> Self {
        Matrix4::set(
            x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn compose(position: Vector3, quaternion: Quaternion, scale: Vector3) -> Self {
        let (x, y, z, w) = (quaternion.x, quaternion.y, quaternion.z, quaternion.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        let (sx, sy, sz) = (scale.x, scale.y, scale.z);
        Matrix4 {
            elements: [
                (1.0 - (yy + zz)) * sx,
                (xy + wz) * sx,
                (xz - wy) * sx,
                0.0,
                (xy - wz) * sy,
                (1.0 - (xx + zz)) * sy,
                (yz + wx) * sy,
                0.0,
                (xz + wy) * sz,
                (yz - wx) * sz,
                (1.0 - (xx + yy)) * sz,
                0.0,
                position.x,
                position.y,
                position.z,
                1.0,
            ],
        }
    }

    /// `decompose`: position, quaternion and scale.
    pub fn decompose(&self) -> (Vector3, Quaternion, Vector3) {
        let te = &self.elements;
        let mut sx = Vector3::new(te[0], te[1], te[2]).length();
        let sy = Vector3::new(te[4], te[5], te[6]).length();
        let sz = Vector3::new(te[8], te[9], te[10]).length();
        let det = self.determinant();
        if det < 0.0 {
            sx = -sx;
        }
        let position = Vector3::new(te[12], te[13], te[14]);
        let mut m1 = *self;
        let inv_sx = 1.0 / sx;
        let inv_sy = 1.0 / sy;
        let inv_sz = 1.0 / sz;
        m1.elements[0] *= inv_sx;
        m1.elements[1] *= inv_sx;
        m1.elements[2] *= inv_sx;
        m1.elements[4] *= inv_sy;
        m1.elements[5] *= inv_sy;
        m1.elements[6] *= inv_sy;
        m1.elements[8] *= inv_sz;
        m1.elements[9] *= inv_sz;
        m1.elements[10] *= inv_sz;
        let quaternion = Quaternion::from_rotation_matrix(&m1);
        (position, quaternion, Vector3::new(sx, sy, sz))
    }
}

// ── Box3 and Sphere ─────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box3 {
    pub min: Vector3,
    pub max: Vector3,
}

impl Default for Box3 {
    fn default() -> Self {
        Box3::EMPTY
    }
}

impl Box3 {
    /// `new Box3()` and `makeEmpty()`: +∞ to -∞.
    pub const EMPTY: Box3 = Box3 {
        min: Vector3::splat(f64::INFINITY),
        max: Vector3::splat(f64::NEG_INFINITY),
    };

    pub fn is_empty(&self) -> bool {
        self.max.x < self.min.x || self.max.y < self.min.y || self.max.z < self.min.z
    }

    pub fn get_center(&self) -> Vector3 {
        if self.is_empty() {
            Vector3::new(0.0, 0.0, 0.0)
        } else {
            (self.min + self.max).multiply_scalar(0.5)
        }
    }

    pub fn get_size(&self) -> Vector3 {
        if self.is_empty() {
            Vector3::new(0.0, 0.0, 0.0)
        } else {
            self.max - self.min
        }
    }

    pub fn expand_by_point(&mut self, point: Vector3) {
        self.min = self.min.min(point);
        self.max = self.max.max(point);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sphere {
    pub center: Vector3,
    pub radius: f64,
}

impl Default for Sphere {
    /// `new Sphere()`: centre 0, radius -1 (empty).
    fn default() -> Self {
        Sphere {
            center: Vector3::new(0.0, 0.0, 0.0),
            radius: -1.0,
        }
    }
}
