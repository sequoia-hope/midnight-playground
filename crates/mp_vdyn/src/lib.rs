//! Vehicle dynamics for Midnight Playground (docs/vehicle-dynamics/SPEC.md):
//! the rigid body, suspension, wheels, the brush tyre, the `Ground` trait
//! and vehicle definitions. It depends only on `mp_math` and knows nothing
//! of tracks, races or levels; `mp_sim` adapts its track to [`Ground`].
//!
//! The rules of the simulation crates hold here (SPEC 1.1): no engine,
//! clock, threads, global state or hash maps; all values `f64`; every
//! inexact function through `mp_math::kernel`. The same inputs give the
//! same bits on every platform.

#![forbid(unsafe_code)]
// The wheels' loops index several parallel arrays (wheels, forces, hub
// speeds); index loops read plainest there.
#![allow(clippy::needless_range_loop)]

pub mod body;
pub mod cars;
pub mod drivetrain;
pub mod ground;
pub mod math;
pub mod rig;
pub mod tyre;
pub mod vehicle;

pub use ground::{Collider, FlatGround, Ground, GroundSample, PlaneGround, SdfSample, Surface};
pub use math::{Aabb, Iso3, Quat, Vec3};
pub use tyre::{BrushParams, BrushTyre, ContactInfo, HubState, Tyre, TyreOutput};
pub use vehicle::{Controls, Telemetry, Vehicle, VehicleDef, WheelTelemetry};
