//! Desert Run built by `mr_worldgen` in the client (roadmap WP 7.3's client
//! half; DECISIONS D720 on). `Desert` is the level's only scenery module
//! (D550), so the client's build is the whole level, numbered as its export
//! (D551), and its animator (D553, D555) runs over either scene: the glow
//! clocks and the flicker colours, the pools' and beams' night opacity, the
//! train, its headlight sprites and its spot light, the tumbleweeds.

use crate::render::lighting::Lighting;
use bevy::math::DVec3;
use mr_worldgen::world::{Scenery, SceneryInfo};

/// Desert Run's scenery, named rather than taken from `scenery::PORTED`
/// (D498): `Desert` alone.
pub fn scenery(info: &SceneryInfo) -> Option<Box<dyn Scenery>> {
    match info.name {
        "Desert" => Some(Box::new(mr_worldgen::desert::Desert::new(info))),
        _ => None,
    }
}

/// A light's edit this frame: its colour (linear) and intensity.
pub type LightEdit = ([f64; 3], f64);

/// The scene's spot light (the train's headlight beam, the one the loader
/// takes into `Lighting`, D553) follows the animator: its colour and
/// intensity from its `Light` edit, its position from its node and its
/// direction from its target's node (`beam.target`, the next sibling), as
/// three reads `matrixWorld` and `target.matrixWorld` each frame.
/// `position` and `target` are the two nodes' world positions when they
/// moved this frame. The spot is rewritten only when something changed, so
/// a frozen frame leaves `Lighting` alone.
pub fn follow_spot(
    lighting: &mut Lighting,
    light: Option<LightEdit>,
    position: Option<DVec3>,
    target: Option<DVec3>,
) {
    let Some(cur) = lighting.spot else { return };
    let mut next = cur;
    if let Some((color, intensity)) = light {
        next.color = color;
        next.intensity = intensity;
    }
    if let Some(p) = position {
        next.position = p;
    }
    if let Some(t) = target {
        next.target = t;
    }
    if next != cur {
        lighting.spot = Some(next);
    }
}
