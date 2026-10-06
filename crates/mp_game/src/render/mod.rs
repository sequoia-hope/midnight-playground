//! Rendering (SPEC 6, roadmap WP 2.3 and 2.4): the `three_std` shading
//! library, the plain material kinds and the patched terrain, road and sea
//! kinds, points as quads, the sky dome, the environment map from the
//! sky, the shadow map over three's box, fog, and the post chain, so a
//! scene is lit and shaded as three.js r180 shades the JS game.
//!
//! - [`lighting`]: the scene-wide light, fog, exposure and sky state
//!   ([`Lighting`]), packed into one small texture every material reads,
//!   and the sun's shadow camera given to Bevy's shadow pass.
//! - [`material`]: [`ThreeMaterial`], the Standard, Physical, Lambert and
//!   Basic kinds, the Terrain, Asphalt, Shoulder, Markings and Sea patches
//!   and the points (`three_material.wgsl` on `three_std.wgsl`).
//! - [`sky`]: `Sky.js`'s time of day ([`SkyState`]) and the SkyDome kind.
//! - [`pmrem`]: the environment map, three's PMREM of the sky.
//! - [`post`]: UnrealBloomPass and OutputPass.
//! - [`instancing`]: an `InstancedMesh` as one entity and one instanced
//!   draw, as three draws it.
//! - [`frame`]: Bevy's `render_system` without its empty submission.
//! - [`sort`]: the transparent pass in three's order (`renderOrder`, then
//!   depth, D810).

pub mod frame;
pub mod instancing;
pub mod lighting;
pub mod material;
pub mod pmrem;
pub mod post;
pub mod sky;
pub mod sort;

pub use lighting::Lighting;
pub use material::{SharedImages, ThreeMaterial};
pub use sky::{SkyMaterial, SkyState};

use bevy::core_pipeline::Core3dSystems;
use bevy::core_pipeline::schedule::Core3d;
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::light::SimulationLightSystems;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponentPlugin;
use bevy::render::extract_resource::ExtractResourcePlugin;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use lighting::{GLOBALS_WIDTH, Globals};

/// The globals row as an image (RGBA32F, one row); its contents are
/// written by the render world every frame.
fn globals_image() -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: GLOBALS_WIDTH as u32,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0u8; 16],
        TextureFormat::Rgba32Float,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    image
}

/// Writes this frame's globals into their texture.
fn write_globals(
    globals: Option<Res<Globals>>,
    shared: Option<Res<SharedImages>>,
    images: Res<RenderAssets<GpuImage>>,
    queue: Res<RenderQueue>,
) {
    let (Some(globals), Some(shared)) = (globals, shared) else {
        return;
    };
    let Some(gpu) = images.get(&shared.globals) else {
        return;
    };
    let mut bytes = Vec::with_capacity(GLOBALS_WIDTH * 16);
    for t in &globals.0 {
        for x in t {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
    }
    queue.write_texture(
        TexelCopyTextureInfo {
            texture: &gpu.texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        &bytes,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some((GLOBALS_WIDTH * 16) as u32),
            rows_per_image: None,
        },
        Extent3d {
            width: GLOBALS_WIDTH as u32,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
}

pub struct ThreeRenderPlugin;

impl Plugin for ThreeRenderPlugin {
    fn build(&self, app: &mut App) {
        bevy::shader::load_shader_library!(app, "three_std.wgsl");
        bevy::shader::load_shader_library!(app, "three_globals.wgsl");
        bevy::shader::load_shader_library!(app, "sky.wgsl");
        bevy::asset::embedded_asset!(app, "three_material.wgsl");
        bevy::asset::embedded_asset!(app, "three_prepass.wgsl");
        bevy::asset::embedded_asset!(app, "sky_dome.wgsl");
        bevy::asset::embedded_asset!(app, "pmrem.wgsl");
        bevy::asset::embedded_asset!(app, "post.wgsl");
        bevy::asset::load_internal_asset!(
            app,
            material::INSTANCED_PREPASS_SHADER,
            "three_prepass_instanced.wgsl",
            Shader::from_wgsl
        );

        let shared = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            SharedImages {
                globals: images.add(globals_image()),
                env: images.add(pmrem::env_image()),
            }
        };
        app.insert_resource(shared)
            .init_resource::<Lighting>()
            .init_resource::<lighting::MaterialLights>()
            .init_resource::<pmrem::EnvRequest>()
            .insert_resource(Globals([[0.0; 4]; GLOBALS_WIDTH]))
            .add_plugins((
                MaterialPlugin::<ThreeMaterial>::default(),
                MaterialPlugin::<SkyMaterial>::default(),
                ExtractResourcePlugin::<Globals>::default(),
                ExtractResourcePlugin::<SharedImages>::default(),
                ExtractResourcePlugin::<pmrem::EnvRequest>::default(),
                ExtractComponentPlugin::<post::ThreePost>::default(),
                instancing::InstancingPlugin,
            ))
            .add_systems(Startup, lighting::spawn_sun)
            .add_systems(
                PostUpdate,
                (
                    (lighting::pack_globals, lighting::sync_sun)
                        .before(SimulationLightSystems::UpdateDirectionalLightCascades),
                    lighting::override_cascades
                        .after(SimulationLightSystems::UpdateDirectionalLightCascades)
                        .before(SimulationLightSystems::UpdateLightFrusta),
                ),
            );

        sort::plugin(app);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<pmrem::PmremState>()
            .init_resource::<post::PostCache>()
            .add_systems(
                RenderStartup,
                (pmrem::init_pmrem_pipelines, post::init_post_pipelines),
            )
            .add_systems(
                Render,
                (
                    write_globals.in_set(RenderSystems::PrepareResources),
                    post::prepare_post_textures.in_set(RenderSystems::PrepareResources),
                ),
            )
            .add_systems(Core3d, pmrem::build_env.in_set(Core3dSystems::Prepass))
            .add_systems(
                Core3d,
                post::three_post
                    .before(tonemapping)
                    .in_set(Core3dSystems::PostProcess),
            );
    }

    fn finish(&self, app: &mut App) {
        frame::replace_render_system(app);
    }
}
