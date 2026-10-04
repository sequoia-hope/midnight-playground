//! Downtown Streets built by `mr_worldgen` in the client (roadmap WP 7.2's
//! client half; DECISIONS D720 on). `Streets` is the level's only scenery
//! module (D610), so the client's build is the whole level, numbered as its
//! export (D613).

use mr_worldgen::world::{Scenery, SceneryInfo};

/// Downtown Streets' scenery, named rather than taken from
/// `scenery::PORTED` (D498): `Streets` alone.
pub fn scenery(info: &SceneryInfo) -> Option<Box<dyn Scenery>> {
    match info.name {
        "Streets" => Some(Box::new(mr_worldgen::streets::Streets::new(info))),
        _ => None,
    }
}
