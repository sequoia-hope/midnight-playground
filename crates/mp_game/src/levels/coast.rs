//! The Coast Highway (Level 2) built by `mp_worldgen` in the client (roadmap
//! M7, WP 7.1's client half; DECISIONS D700 on). Coast, Beach and Harbor are
//! the level's scenery modules (D530, D533, D540); with the three ported,
//! the client's build is the whole level, numbered as its export (D536), so
//! its animators (D537, D543) run over either scene: the surf's clock and
//! brightness, the lighthouse's beam and glow, the string lights and
//! reflectors, the fishing boats and their running lights, the Ferris wheel,
//! its gondolas and the coaster, the beach surf's maps and opacity, the lamp
//! pools, the signals, the harbour's lamps, lenses, chase bulbs, glow
//! points, boats and breakwater lamps.
//!
//! Nothing else of the client is level-specific: the edits these animators
//! make are the shared path's (`crate::animate`), which gained a plain
//! material's opacity and the attribute edits of `Points` for them (D701).

use mp_worldgen::world::{Scenery, SceneryInfo};

/// Level 2's scenery, named rather than taken from `scenery::PORTED`
/// (D498): Coast, Beach and Harbor; any other module is left out, as
/// `scenery_factory` leaves out an unported one.
pub fn scenery(info: &SceneryInfo) -> Option<Box<dyn Scenery>> {
    match info.name {
        "Coast" => Some(Box::new(mp_worldgen::coast::Coast::new(info))),
        "Beach" => Some(Box::new(mp_worldgen::beach::Beach::new(info))),
        "Harbor" => Some(Box::new(mp_worldgen::harbor::Harbor::new(info))),
        _ => None,
    }
}
