//! The post chain (`src/main.js:61-66`, SPEC 6.1): `UnrealBloomPass(res,
//! 0.38, 0.35, 0.92)` then `OutputPass` (ACES filmic with the exposure,
//! sRGB), on the camera's HDR target. Bevy's own tone mapping is off on the
//! camera; this runs in its place, ahead of Bevy's upscaling to the sRGB
//! surface (which does the sRGB transfer).
//!
//! The bloom keeps three's render targets: a half-size bright pass, five
//! sizes each halved with `Math.round`, a horizontal and a vertical
//! Gaussian per size (kernel radii 3, 5, 7, 9, 11), and the composite into
//! the first horizontal target; the additive blend over the scene is folded
//! into the output pass (`post.wgsl`).

use super::lighting::{G_AMBIENT, Globals};
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer_sized};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::ViewTarget;
use std::num::NonZeroU64;

/// `UnrealBloomPass(resolution, 0.38, 0.35, 0.92)` (main.js).
pub const BLOOM_STRENGTH: f32 = 0.38;
pub const BLOOM_RADIUS: f32 = 0.35;
pub const BLOOM_THRESHOLD: f32 = 0.92;
/// LuminosityHighPass's smoothWidth as UnrealBloomPass sets it.
const SMOOTH_WIDTH: f32 = 0.01;
const N_MIPS: usize = 5;
const KERNEL_SIZES: [u32; N_MIPS] = [3, 5, 7, 9, 11];
const FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// On a camera: draw it through three's post chain.
#[derive(Component, Clone, Copy, Default, ExtractComponent)]
pub struct ThreePost;

#[derive(Component)]
pub struct PostTextures {
    bright: CachedTexture,
    horizontal: Vec<CachedTexture>,
    vertical: Vec<CachedTexture>,
    sizes: Vec<(u32, u32)>,
}

/// `Math.round(x / 2)` for a whole number.
fn half(x: u32) -> u32 {
    x.div_ceil(2)
}

pub fn prepare_post_textures(
    mut commands: Commands,
    mut cache: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ExtractedCamera), With<ThreePost>>,
) {
    for (entity, camera) in &views {
        let Some(size) = camera.physical_viewport_size else {
            continue;
        };
        let mut get = |w: u32, h: u32, label: &'static str| {
            cache.get(
                &device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d {
                        width: w.max(1),
                        height: h.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let (mut rx, mut ry) = (half(size.x), half(size.y));
        let bright = get(rx, ry, "mp_bloom_bright");
        let mut horizontal = Vec::new();
        let mut vertical = Vec::new();
        let mut sizes = Vec::new();
        for _ in 0..N_MIPS {
            horizontal.push(get(rx, ry, "mp_bloom_h"));
            vertical.push(get(rx, ry, "mp_bloom_v"));
            sizes.push((rx.max(1), ry.max(1)));
            rx = half(rx);
            ry = half(ry);
        }
        commands.entity(entity).insert(PostTextures {
            bright,
            horizontal,
            vertical,
            sizes,
        });
    }
}

#[derive(Resource)]
pub struct PostPipelines {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    high_pass: CachedRenderPipelineId,
    blur: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
    output: CachedRenderPipelineId,
}

const UNIFORM_FLOATS: usize = 24;
const UNIFORM_SIZE: u64 = (UNIFORM_FLOATS * 4) as u64;

/// The chain's uniform buffer and bind groups, kept between frames
/// (D457): they change only with the exposure (the buffer's contents) and
/// the textures (a resize, and the view's two post-process textures, which
/// swap every frame), so a frame creates no WebGPU objects here. Creating
/// a buffer and thirteen bind groups every frame made garbage the browser
/// collects (Firefox: a major GC every 20 s or so, "TOO_MUCH_MALLOC").
#[derive(Resource, Default)]
pub struct PostCache {
    buffer: Option<Buffer>,
    data: Vec<u8>,
    /// Bind groups per set of views: source, destination, the bloom's.
    groups: Vec<(Vec<TextureViewId>, Vec<BindGroup>)>,
}

pub fn init_post_pipelines(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
) {
    let tex = || texture_2d(TextureSampleType::Float { filterable: true });
    let layout = BindGroupLayoutDescriptor::new(
        "mp_post_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer_sized(true, NonZeroU64::new(UNIFORM_SIZE)),
                tex(),
                sampler(SamplerBindingType::Filtering),
                tex(),
                tex(),
                tex(),
                tex(),
            ),
        ),
    );
    // three's render targets: linear filtering, clamped.
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..default()
    });
    let shader = bevy::asset::load_embedded_asset!(asset_server.as_ref(), "post.wgsl");
    let pipeline = |entry: &'static str, format: TextureFormat| {
        pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(format!("mp_post_{entry}").into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    };
    commands.insert_resource(PostPipelines {
        high_pass: pipeline("high_pass", FORMAT),
        blur: pipeline("blur", FORMAT),
        composite: pipeline("composite", FORMAT),
        output: pipeline("output", TextureFormat::Rgba16Float),
        layout,
        sampler,
    });
}

/// `_getSeparableBlurMaterial`'s coefficients.
fn coefficients(kernel_radius: u32) -> [f32; 12] {
    let mut c = [0f32; 12];
    let k = f64::from(kernel_radius);
    for (i, v) in c.iter_mut().enumerate().take(kernel_radius as usize) {
        let x = i as f64;
        *v = (0.39894 * (-0.5 * x * x / (k * k)).exp() / k) as f32;
    }
    c
}

/// The chain for one view.
pub fn three_post(
    view: ViewQuery<(&ViewTarget, &PostTextures), With<ThreePost>>,
    pipelines: Option<Res<PostPipelines>>,
    pipeline_cache: Res<PipelineCache>,
    globals: Option<Res<Globals>>,
    queue: Res<RenderQueue>,
    mut cache: ResMut<PostCache>,
    mut ctx: RenderContext,
) {
    let (target, textures) = view.into_inner();
    let Some(pipelines) = pipelines else { return };
    let (Some(high_pass), Some(blur), Some(composite), Some(output)) = (
        pipeline_cache.get_render_pipeline(pipelines.high_pass),
        pipeline_cache.get_render_pipeline(pipelines.blur),
        pipeline_cache.get_render_pipeline(pipelines.composite),
        pipeline_cache.get_render_pipeline(pipelines.output),
    ) else {
        return;
    };
    let exposure = globals.map_or(1.0, |g| g.0[G_AMBIENT][3]);
    let device = ctx.render_device().clone();
    let post = target.post_process_write();

    // The passes: uniforms, inputs, output.
    struct Step<'a> {
        pipeline: &'a RenderPipeline,
        uniform: [f32; UNIFORM_FLOATS],
        inputs: [&'a TextureView; 5],
        output: &'a TextureView,
    }
    let bright = &textures.bright.default_view;
    let h = |i: usize| &textures.horizontal[i].default_view;
    let v = |i: usize| &textures.vertical[i].default_view;
    // Unused inputs are bound to the scene, which no bloom pass writes.
    let fill = post.source;
    let mut steps: Vec<Step> = Vec::new();
    let mut u = [0f32; UNIFORM_FLOATS];
    u[0] = BLOOM_THRESHOLD;
    u[1] = SMOOTH_WIDTH;
    u[2] = exposure;
    steps.push(Step {
        pipeline: high_pass,
        uniform: u,
        inputs: [post.source, fill, fill, fill, fill],
        output: bright,
    });
    for (i, &kernel) in KERNEL_SIZES.iter().enumerate() {
        let (rx, ry) = textures.sizes[i];
        let input = if i == 0 { bright } else { v(i - 1) };
        let mut u = [0f32; UNIFORM_FLOATS];
        u[4] = 1.0 / rx as f32;
        u[5] = 1.0 / ry as f32;
        u[10] = kernel as f32;
        u[12..24].copy_from_slice(&coefficients(kernel));
        let mut ux = u;
        ux[6] = 1.0;
        steps.push(Step {
            pipeline: blur,
            uniform: ux,
            inputs: [input, fill, fill, fill, fill],
            output: h(i),
        });
        let mut uy = u;
        uy[7] = 1.0;
        steps.push(Step {
            pipeline: blur,
            uniform: uy,
            inputs: [h(i), fill, fill, fill, fill],
            output: v(i),
        });
    }
    let mut u = [0f32; UNIFORM_FLOATS];
    u[8] = BLOOM_STRENGTH;
    u[9] = BLOOM_RADIUS;
    steps.push(Step {
        pipeline: composite,
        uniform: u,
        inputs: [v(0), v(1), v(2), v(3), v(4)],
        output: h(0),
    });
    let mut u = [0f32; UNIFORM_FLOATS];
    u[2] = exposure;
    steps.push(Step {
        pipeline: output,
        uniform: u,
        inputs: [post.source, h(0), v(1), v(1), v(1)],
        output: post.destination,
    });

    let a = u64::from(device.limits().min_uniform_buffer_offset_alignment).max(1);
    let align = UNIFORM_SIZE.div_ceil(a) * a;
    let mut data = vec![0u8; (align * steps.len() as u64) as usize];
    for (i, s) in steps.iter().enumerate() {
        let at = (align * i as u64) as usize;
        for (k, x) in s.uniform.iter().enumerate() {
            data[at + 4 * k..at + 4 * k + 4].copy_from_slice(&x.to_le_bytes());
        }
    }
    let cache = &mut *cache;
    if cache
        .buffer
        .as_ref()
        .is_none_or(|b| b.size() != data.len() as u64)
    {
        cache.buffer = Some(device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("mp_post_uniforms"),
            contents: &data,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        }));
        cache.groups.clear();
        cache.data.clone_from(&data);
    } else if cache.data != data {
        if let Some(b) = &cache.buffer {
            queue.write_buffer(b, 0, &data);
        }
        cache.data.clone_from(&data);
    }
    let Some(buffer) = cache.buffer.clone() else {
        return;
    };
    let key: Vec<TextureViewId> = [post.source.id(), post.destination.id(), bright.id()]
        .into_iter()
        .chain((0..N_MIPS).flat_map(|i| [h(i).id(), v(i).id()]))
        .collect();
    if !cache.groups.iter().any(|(k, _)| *k == key) {
        let layout = pipeline_cache.get_bind_group_layout(&pipelines.layout);
        let groups = steps
            .iter()
            .map(|s| {
                device.create_bind_group(
                    "mp_post_bind_group",
                    &layout,
                    &BindGroupEntries::sequential((
                        BufferBinding {
                            buffer: &buffer,
                            offset: 0,
                            size: NonZeroU64::new(UNIFORM_SIZE),
                        },
                        s.inputs[0],
                        &pipelines.sampler,
                        s.inputs[1],
                        s.inputs[2],
                        s.inputs[3],
                        s.inputs[4],
                    )),
                )
            })
            .collect();
        // Two entries in use (the swapping textures); older ones are from
        // textures since replaced.
        if cache.groups.len() >= 4 {
            cache.groups.clear();
        }
        cache.groups.push((key.clone(), groups));
    }
    let Some((_, groups)) = cache.groups.iter().find(|(k, _)| *k == key) else {
        return;
    };
    let encoder = ctx.command_encoder();
    encoder.push_debug_group("mp_post");
    for (i, (s, group)) in steps.iter().zip(groups).enumerate() {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("mp_post_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: s.output,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(s.pipeline);
        pass.set_bind_group(0, group, &[(align * i as u64) as u32]);
        pass.draw(0..3, 0..1);
    }
    encoder.pop_debug_group();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloom_sizes_round_as_three() {
        // 1280 × 800: 640 × 400, 320 × 200, 160 × 100, 80 × 50, 40 × 25.
        let mut x = half(1280);
        let mut sizes = vec![];
        for _ in 0..5 {
            sizes.push(x);
            x = half(x);
        }
        assert_eq!(sizes, vec![640, 320, 160, 80, 40]);
        assert_eq!(half(25), 13);
        let c = coefficients(3);
        assert!((c[0] - 0.39894 / 3.0).abs() < 1e-7 && c[3] == 0.0);
    }
}
