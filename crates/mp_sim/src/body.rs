//! What other code reads from, and does to, any car on the road (SPEC 4.4:
//! the `Body` trait replaces the JS's duck typing).
//!
//! The JS passes lists of car objects (`ctx.cars`, `agents`) and reads
//! `o.s`, `o.lat`, `o.dir`, `o.speedAlong`, `o.halfW`, `o.halfL` and
//! `o.gapLat` from each, live: a rival updated earlier in the tick is seen
//! where it now is. Here such a list is a slice of [`AgentView`]s built from
//! the current state just before it is read, and a car's identity (`o ===
//! this`) is its index in that list.

use mp_track::Track;

use crate::vehicle::Vehicle;

/// Which body a view is of: its pool and index there (SPEC 4.3, "a car's
/// identity is its index in its pool"). `Police` counts units, then
/// roadblock cars, as one list (as the trace does).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BodyId {
    Player(usize),
    Rival(usize),
    Traffic(usize),
    Police(usize),
    Sawhorse(usize),
    /// A body outside the pools (tests' stand-ins).
    #[default]
    Anon,
}

impl BodyId {
    /// The trace's body reference: player i, rival 100 + i, traffic
    /// 200 + i, police 300 + i, sawhorse 500 + i; none -1.
    pub fn trace_ref(self) -> i32 {
        match self {
            BodyId::Player(i) => i as i32,
            BodyId::Rival(i) => 100 + i as i32,
            BodyId::Traffic(i) => 200 + i as i32,
            BodyId::Police(i) => 300 + i as i32,
            BodyId::Sawhorse(i) => 500 + i as i32,
            BodyId::Anon => -1,
        }
    }
}

/// The fields of a car that other cars' drivers read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AgentView {
    pub id: BodyId,
    /// A police car (`o.police`).
    pub police: bool,
    /// A traffic car (`o.kinematicOnly`).
    pub kinematic_only: bool,
    pub s: f64,
    pub lat: f64,
    /// +1 with the race, -1 oncoming.
    pub dir: i32,
    pub half_w: f64,
    pub half_l: f64,
    pub speed_along: f64,
    /// A roadblock piece: the lateral position of its gap.
    pub gap_lat: Option<f64>,
}

/// A body the collision pass can push: `x, z, yaw, mass, halfL, halfW` and
/// `velocity()/setVelocity()/translate()/addSpin()`.
pub trait Body {
    fn v(&self) -> &Vehicle;
    fn mass(&self) -> f64;
    /// Traffic: two of these touching only collide if one is `crashy`.
    fn kinematic_only(&self) -> bool {
        false
    }
    /// Police units collide even with traffic.
    fn crashy(&self) -> bool {
        false
    }
    fn velocity(&self) -> (f64, f64);
    fn set_velocity(&mut self, vx: f64, vz: f64);
    fn translate(&mut self, t: &Track, dx: f64, dz: f64);
    fn add_spin(&mut self, w: f64);
    fn view(&self) -> AgentView;
}

/// Adapter so the player's free-physics car looks like any other body
/// (`PhysicsBody`).
pub struct PhysicsBody<'a> {
    pub v: &'a mut Vehicle,
}

impl Body for PhysicsBody<'_> {
    fn v(&self) -> &Vehicle {
        self.v
    }
    fn mass(&self) -> f64 {
        self.v.mass
    }
    fn velocity(&self) -> (f64, f64) {
        (self.v.vx, self.v.vz)
    }
    fn set_velocity(&mut self, vx: f64, vz: f64) {
        self.v.vx = vx;
        self.v.vz = vz;
    }
    fn translate(&mut self, _t: &Track, dx: f64, dz: f64) {
        self.v.x += dx;
        self.v.z += dz;
    }
    fn add_spin(&mut self, w: f64) {
        self.v.yaw_rate += w;
    }
    fn view(&self) -> AgentView {
        player_view(self.v)
    }
}

/// The player as other cars see it (`PhysicsBody`'s getters, `dir` 1).
pub fn player_view(v: &Vehicle) -> AgentView {
    AgentView {
        id: BodyId::Anon,
        police: false,
        kinematic_only: false,
        s: v.s,
        lat: v.lat,
        dir: 1,
        half_w: v.half_w,
        half_l: v.half_l,
        speed_along: v.speed,
        gap_lat: None,
    }
}

/// The bodies of one collision pass, in agent order.
pub trait BodySet {
    fn count(&self) -> usize;
    fn body(&self, i: usize) -> &dyn Body;
    fn body_mut(&mut self, i: usize) -> &mut dyn Body;
}

impl<B: Body> BodySet for [B] {
    fn count(&self) -> usize {
        self.len()
    }
    fn body(&self, i: usize) -> &dyn Body {
        &self[i]
    }
    fn body_mut(&mut self, i: usize) -> &mut dyn Body {
        &mut self[i]
    }
}

impl BodySet for [&mut dyn Body] {
    fn count(&self) -> usize {
        self.len()
    }
    fn body(&self, i: usize) -> &dyn Body {
        &*self[i]
    }
    fn body_mut(&mut self, i: usize) -> &mut dyn Body {
        &mut *self[i]
    }
}
