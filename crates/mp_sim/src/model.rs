//! Which model drives a player's car (docs/vehicle-dynamics/SPEC.md 8.1).
//!
//! The spec's sketch makes `PlayerCar.phys` itself the enum. Here
//! `PlayerCar.phys` stays a `CarPhysics` (DECISIONS D1140): it is the body
//! view's half that the race, the HUD, the audio and the client read
//! (gear, rpm, nitro, drifting, slip, events, locked, damage, ...), the
//! arcade model runs on it exactly as before, and a sim car will write the
//! same fields every tick (SPEC 4.7). `PlayerCar.model` says which model
//! moves the car.

/// The model that moves a player's car.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum VehicleModel {
    /// Today's handling: `CarPhysics` on the `Vehicle` (Tier 0).
    #[default]
    Arcade,
}
