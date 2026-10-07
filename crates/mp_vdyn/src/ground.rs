//! What a vehicle drives on (SPEC 6). `mp_vdyn` knows nothing about tracks:
//! `mp_sim::ground::TrackGround` implements this for the road ribbon, and
//! the test rig's [`FlatGround`] and [`PlaneGround`] are here.

use crate::math::{Aabb, Vec3};

/// A surface's properties. `mu` scales the tyre's friction, `rolling` its
/// rolling resistance; `loose` is 0 on tarmac and up to 1 on dirt and grass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub mu: f64,
    pub rolling: f64,
    pub loose: f64,
    pub id: u16,
}

impl Surface {
    pub const TARMAC: Surface = Surface {
        mu: 1.0,
        rolling: 1.0,
        loose: 0.0,
        id: 0,
    };
}

impl Default for Surface {
    fn default() -> Surface {
        Surface::TARMAC
    }
}

/// The ground under a point: its height, the unit normal there (pointing
/// up, out of the ground) and the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundSample {
    pub y: f64,
    pub normal: Vec3,
    pub surface: Surface,
}

/// A signed distance to the surface (negative inside), the outward normal
/// and the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SdfSample {
    pub dist: f64,
    pub normal: Vec3,
    pub surface: Surface,
}

/// A static obstacle near the vehicle, for chassis contact (V2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Collider {
    /// The half-space behind a plane: `point` on it, `normal` pointing out
    /// of the obstacle (toward where the vehicle may be).
    Plane { point: Vec3, normal: Vec3 },
}

pub trait Ground {
    /// Height and normal under (x, z), and the surface there. Enough for a
    /// single-ray brush tyre.
    fn height(&self, x: f64, z: f64) -> GroundSample;
    /// Signed distance from `p` to the surface (negative inside), with the
    /// outward normal and the surface. For soft tyres and multi-point
    /// contact.
    fn sdf(&self, p: Vec3) -> SdfSample;
    /// Static obstacles near a box (walls, rocks), for chassis contact.
    fn colliders(&self, aabb: Aabb, out: &mut Vec<Collider>);
}

/// Level ground at height `y`, one surface everywhere.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatGround {
    pub y: f64,
    pub surface: Surface,
}

impl Default for FlatGround {
    fn default() -> FlatGround {
        FlatGround {
            y: 0.0,
            surface: Surface::TARMAC,
        }
    }
}

impl Ground for FlatGround {
    fn height(&self, _x: f64, _z: f64) -> GroundSample {
        GroundSample {
            y: self.y,
            normal: Vec3::Y,
            surface: self.surface,
        }
    }

    fn sdf(&self, p: Vec3) -> SdfSample {
        SdfSample {
            dist: p.y - self.y,
            normal: Vec3::Y,
            surface: self.surface,
        }
    }

    fn colliders(&self, _aabb: Aabb, _out: &mut Vec<Collider>) {}
}

/// An infinite tilted plane through `point` with the unit `normal` (its
/// `y` must be positive): the rig's slope for the stillness tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneGround {
    pub point: Vec3,
    pub normal: Vec3,
    pub surface: Surface,
}

impl PlaneGround {
    /// A slope rising along +x with the given grade (rise over run, so 0.2
    /// is a 20 % grade), through the origin.
    pub fn grade_x(grade: f64) -> PlaneGround {
        PlaneGround {
            point: Vec3::ZERO,
            normal: Vec3::new(-grade, 1.0, 0.0).normalize(),
            surface: Surface::TARMAC,
        }
    }
}

impl Ground for PlaneGround {
    fn height(&self, x: f64, z: f64) -> GroundSample {
        let n = self.normal;
        let y = self.point.y - (n.x * (x - self.point.x) + n.z * (z - self.point.z)) / n.y;
        GroundSample {
            y,
            normal: n,
            surface: self.surface,
        }
    }

    fn sdf(&self, p: Vec3) -> SdfSample {
        SdfSample {
            dist: (p - self.point).dot(self.normal),
            normal: self.normal,
            surface: self.surface,
        }
    }

    fn colliders(&self, _aabb: Aabb, _out: &mut Vec<Collider>) {}
}
