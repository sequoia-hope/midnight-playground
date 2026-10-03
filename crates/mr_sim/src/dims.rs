//! Vehicle dimensions per kind (SPEC 4.3, "Vehicle dimensions"): what
//! `CarModel.js` gives each of its thirteen kinds (`:1254-2010`, the same at
//! every detail level) and `PursuitView.js` the sawhorse. Physics and
//! collisions read them through `Vehicle` (`halfW`, `halfL`, `radius`).
//!
//! Checked against the dump of the JS game's models
//! (`parity/golden/sim/world-data.json`, WP 0.4); the car model port (M4)
//! must agree with this table.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dims {
    pub length: f64,
    pub width: f64,
    pub height: f64,
    pub wheel_radius: f64,
    pub wheel_base: f64,
    /// Wheel track; the sawhorse has none.
    pub track: Option<f64>,
}

const fn d(
    length: f64,
    width: f64,
    height: f64,
    wheel_radius: f64,
    wheel_base: f64,
    track: f64,
) -> Dims {
    Dims {
        length,
        width,
        height,
        wheel_radius,
        wheel_base,
        track: Some(track),
    }
}

/// `VEHICLE_KINDS` in `CarModel.js` order, then the sawhorse.
pub const DIMS: [(&str, Dims); 14] = [
    ("sports", d(4.47, 1.9, 1.25, 0.34, 2.6, 1.62)),
    ("muscle", d(4.86, 1.95, 1.35, 0.36, 2.8, 1.64)),
    ("super", d(4.57, 2.05, 1.14, 0.35, 2.7, 1.72)),
    ("electric", d(4.74, 1.98, 1.28, 0.36, 2.9, 1.7)),
    ("rally", d(4.12, 1.9, 1.6, 0.34, 2.55, 1.62)),
    ("sedan", d(4.74, 1.82, 1.45, 0.33, 2.75, 1.56)),
    ("hatch", d(4.02, 1.75, 1.49, 0.31, 2.5, 1.5)),
    ("van", d(5.03, 1.95, 2.08, 0.34, 3.0, 1.66)),
    ("pickup", d(5.36, 2.0, 1.84, 0.38, 3.3, 1.72)),
    ("boxtruck", d(7.2, 2.44, 3.37, 0.48, 4.2, 1.9)),
    ("tractor", d(3.4, 1.95, 2.54, 0.76, 2.1, 1.5)),
    ("police", d(5.1, 1.9, 1.59, 0.34, 2.9, 1.6)),
    ("policeSuv", d(5.0, 2.0, 2.1, 0.39, 2.95, 1.7)),
    (
        "sawhorse",
        Dims {
            length: 2.4,
            width: 0.5,
            height: 1.0,
            wheel_radius: 0.3,
            wheel_base: 1.0,
            track: None,
        },
    ),
];

/// The dimensions of a kind; `None` for one the game does not build.
pub fn dims(kind: &str) -> Option<Dims> {
    DIMS.iter().find(|(k, _)| *k == kind).map(|(_, d)| *d)
}
