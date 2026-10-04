//! The client's Bevy plugins: `DefaultPlugins` for the features `mr_game`
//! turns on, in the same order, without Bevy's 2D sprites (DECISIONS D672).
//!
//! `bevy_ui_render` forces the `bevy_sprite` and `bevy_sprite_render`
//! features on, and `DefaultPlugins` then adds `SpritePlugin` and
//! `SpriteRenderPlugin`: sprites, 2D meshes, colour materials, tilemaps and
//! 2D text, none of which the client draws. A plugin a group holds is linked
//! even when disabled (the group keeps it as a `Box<dyn Plugin>`), so the
//! only way to leave that code out of the wasm (about 0.25 MB after gzip)
//! is a group that never names them. The UI needs two things from them,
//! which [`UiSpriteSupport`] provides: the texture atlas assets and the
//! image events that tell its bind groups an image changed.
//!
//! **When you turn on a Bevy feature that brings a plugin** (gizmos, audio,
//! picking, UI widgets, ...), add its plugin here at its place in
//! `bevy_internal`'s `default_plugins.rs`; `DefaultPlugins` would have added
//! it by itself.

use bevy::app::{PluginGroup, PluginGroupBuilder};
use bevy::prelude::*;
use bevy::render::{ExtractSchedule, RenderApp};
use bevy::sprite_render::{SpriteAssetEvents, extract_sprite_events};

/// `DefaultPlugins` (Bevy 0.19.1, the features in `Cargo.toml`) less
/// `SpritePlugin` and `SpriteRenderPlugin`.
pub struct ClientPlugins;

impl PluginGroup for ClientPlugins {
    fn build(self) -> PluginGroupBuilder {
        let group = PluginGroupBuilder::start::<Self>()
            .add(bevy::app::PanicHandlerPlugin)
            .add(bevy::log::LogPlugin::default())
            .add(bevy::app::TaskPoolPlugin::default())
            .add(bevy::diagnostic::FrameCountPlugin)
            .add(bevy::time::TimePlugin)
            .add(bevy::transform::TransformPlugin)
            .add(bevy::diagnostic::DiagnosticsPlugin)
            .add(bevy::input::InputPlugin)
            .add(bevy::window::WindowPlugin::default())
            .add(bevy::a11y::AccessibilityPlugin);
        #[cfg(not(target_arch = "wasm32"))]
        let group = group.add(bevy::app::TerminalCtrlCHandlerPlugin);
        let group = group
            .add(bevy::asset::AssetPlugin::default())
            .add(bevy::winit::WinitPlugin::default())
            .add(bevy::render::RenderPlugin::default())
            .add(bevy::image::ImagePlugin::default())
            .add(bevy::mesh::MeshPlugin)
            .add(bevy::camera::CameraPlugin)
            .add(bevy::light::LightPlugin);
        #[cfg(not(target_arch = "wasm32"))]
        let group = group.add(bevy::render::pipelined_rendering::PipelinedRenderingPlugin);
        group
            .add(bevy::core_pipeline::CorePipelinePlugin)
            // Where SpritePlugin and SpriteRenderPlugin were.
            .add(UiSpriteSupport)
            .add(bevy::text::TextPlugin)
            .add(bevy::ui::UiPlugin)
            .add(bevy::ui_render::UiRenderPlugin)
            .add(bevy::pbr::PbrPlugin::default())
            .add(bevy::state::app::StatesPlugin)
    }
}

/// What `UiRenderPlugin` reads from the sprite plugins: `Assets<
/// TextureAtlasLayout>` (SpritePlugin adds `TextureAtlasPlugin`) and, in the
/// render world, `SpriteAssetEvents` filled each frame by
/// `extract_sprite_events` (SpriteRenderPlugin), with which it drops the
/// bind groups of images that changed.
struct UiSpriteSupport;

impl Plugin for UiSpriteSupport {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<bevy::image::TextureAtlasPlugin>() {
            app.add_plugins(bevy::image::TextureAtlasPlugin);
        }
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .init_resource::<SpriteAssetEvents>()
                .add_systems(ExtractSchedule, extract_sprite_events);
        }
    }
}
