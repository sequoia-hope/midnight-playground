//! The flat-ground test rig (V1): a vehicle, its definition and a ground,
//! stepped tick by tick with simple test drivers (a speed hold, a settle).
//! The vehicle tests of SPEC 10 run on it, and it is the place to try a
//! definition headlessly before it goes on a track.

use crate::ground::{FlatGround, Ground};
use crate::vehicle::{Controls, TICK, Vehicle, VehicleDef};

pub struct Rig<G: Ground> {
    pub def: VehicleDef,
    pub car: Vehicle,
    pub ground: G,
    /// Ticks stepped.
    pub ticks: u32,
}

impl Rig<FlatGround> {
    /// The vehicle at rest on level ground at the origin, heading +x, after
    /// two seconds to settle on its springs.
    pub fn flat(def: VehicleDef) -> Rig<FlatGround> {
        let car = Vehicle::new(&def, 0.0, 0.0, 0.0, 0.0);
        let mut rig = Rig {
            def,
            car,
            ground: FlatGround::default(),
            ticks: 0,
        };
        rig.run(240, &Controls::default());
        rig.ticks = 0;
        rig
    }
}

impl<G: Ground> Rig<G> {
    pub fn step(&mut self, ctl: &Controls) {
        self.car.step(&self.def, ctl, &self.ground);
        self.ticks += 1;
    }

    pub fn run(&mut self, ticks: u32, ctl: &Controls) {
        for _ in 0..ticks {
            self.step(ctl);
        }
    }

    /// Seconds stepped.
    pub fn time(&self) -> f64 {
        self.ticks as f64 * TICK
    }

    /// The throttle a simple proportional speed hold asks for.
    pub fn hold(&self, speed: f64) -> f64 {
        mp_math::clamp((speed - self.car.speed()) * 0.5, 0.0, 1.0)
    }

    /// Steps with `ctl`, the throttle replaced by the speed hold.
    pub fn step_at(&mut self, ctl: &Controls, speed: f64) {
        let c = Controls {
            throttle: self.hold(speed),
            ..*ctl
        };
        self.step(&c);
    }
}
