//! Pipeline warm-up (SPEC 6.3, roadmap WP 2.6): Playground Kit's
//! `pk_shell::warmup` (D1234) over this game's material key, with the
//! scene marker on every stand-in and the instance stream on an instanced
//! material's (its pipelines take the instance stream, D450).

use crate::loader::SceneEntity;
use crate::render::ThreeMaterial;
use crate::render::instancing::{self, InstanceStream, Instances};
use crate::render::material::ThreeKey;
use bevy::prelude::*;
use bevy::render::batching::NoAutomaticBatching;
use std::sync::Arc;

pub use pk_shell::warmup::{Layout, WarmUp, end_warm_up};

/// The race HUD's text styles (`play::hud`'s `text`: bold, these sizes).
/// A new HUD size must be added here too (D459).
pub const HUD_TEXT_SIZES: [f32; 7] = [84.0, 44.0, 30.0, 26.0, 22.0, 17.0, 15.0];

/// The combinations a scene draws (`pk_shell::warmup::Combos` keyed by
/// [`ThreeKey`]).
#[derive(Default)]
pub struct Combos(pk_shell::warmup::Combos<ThreeMaterial, ThreeKey>);

impl Combos {
    /// Notes that `material` (whose key is `key`) is drawn on a mesh of
    /// `layout`, casting a shadow or not.
    pub fn note(
        &mut self,
        key: ThreeKey,
        material: &Handle<ThreeMaterial>,
        layout: &Layout,
        casts: bool,
    ) {
        let instanced = key.instanced;
        self.0.note(key, material, layout, casts, instanced);
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Spawns one stand-in per combination.
    pub fn spawn(&self, commands: &mut Commands, meshes: &mut Assets<Mesh>) -> usize {
        self.spawn_each(commands, meshes).len()
    }

    /// [`Combos::spawn`], returning the stand-ins.
    pub fn spawn_each(&self, commands: &mut Commands, meshes: &mut Assets<Mesh>) -> Vec<Entity> {
        let mut stand_in = Vec::new();
        instancing::push_instance(
            &mut stand_in,
            &bevy::math::DMat4::from_translation(bevy::math::DVec3::new(
                0.0,
                f64::from(pk_shell::warmup::FAR_BELOW),
                0.0,
            )),
            [1.0; 3],
            false,
        );
        let stream = Instances(Arc::new(InstanceStream::new(&stand_in)));
        self.0.spawn_each(commands, meshes, &mut |e, instanced| {
            e.insert(SceneEntity);
            if instanced {
                e.insert((stream.clone(), NoAutomaticBatching));
            }
        })
    }
}

/// Lays out every printable ASCII character in each of the race HUD's text
/// styles during the warm-up (D459).
pub fn spawn_glyphs(commands: &mut Commands) {
    pk_shell::warmup::prewarm_glyphs(commands, &HUD_TEXT_SIZES, &mut |e| {
        e.insert(SceneEntity);
    });
}

/// A menu node off screen during the warm-up (D1005).
pub fn spawn_ui(commands: &mut Commands) {
    pk_shell::warmup::prewarm_ui(commands, &mut |e| {
        e.insert(SceneEntity);
    });
}
