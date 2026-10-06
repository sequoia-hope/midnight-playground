//! The environment map (`src/main.js` `refreshEnv`): three.js r180's
//! `PMREMGenerator.fromScene(envScene, 0.04, 0.1, 200)` of the sky dome,
//! ported pass for pass. The sky's six cube faces (256²) are drawn into the
//! cube-UV atlas (768 × 1024, half float), the base level is blurred by a
//! spherical Gaussian of 0.04 rad, then each smaller and rougher level is
//! blurred from the one before (`_applyPMREM`), in latitudinal and
//! longitudinal halves through a ping-pong target. Materials read the atlas
//! with three's `textureCubeUV` (`three_std.wgsl`).
//!
//! The main world asks for a new map by bumping [`EnvRequest::generation`]
//! (the level's sky does so whenever the route's time of day has moved by
//! 2.5 %, as `refreshEnv` does); the render world builds it before the main
//! pass of that frame.
//!
//! One three.js detail is kept on purpose: `_blur` for the base level passes
//! no pole axis, so it uses whatever the blur material last had. A fresh
//! generator has (0, 1, 0); the game reuses its generator, so every later
//! map uses the last axis of the previous run, `_axisDirections[0]`.

use super::material::SharedImages;
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer_sized};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice};
use bevy::render::texture::{FallbackImage, GpuImage};
use std::num::NonZeroU64;

pub const ENV_WIDTH: u32 = 768;
pub const ENV_HEIGHT: u32 = 1024;
pub const ENV_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const CUBE_SIZE: u32 = 256;
const LOD_MAX: u32 = 8;
const LOD_MIN: u32 = 4;
const EXTRA_LOD_SIGMA: [f64; 6] = [0.125, 0.215, 0.35, 0.446, 0.526, 0.582];
const MAX_SAMPLES: usize = 20;
/// `fromScene`'s first blur, in radians (main.js).
const SCENE_SIGMA: f64 = 0.04;

/// The golden-ratio axes of `_axisDirections` (dodecahedron vertices).
fn axis_directions() -> [[f64; 3]; 10] {
    let phi = (1.0 + 5f64.sqrt()) / 2.0;
    let inv = 1.0 / phi;
    [
        [-phi, inv, 0.0],
        [phi, inv, 0.0],
        [-inv, 0.0, phi],
        [inv, 0.0, phi],
        [0.0, phi, -inv],
        [0.0, phi, inv],
        [-1.0, 1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
    ]
}

/// One draw of the generator.
#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    /// Sky face (true) or blur.
    pub sky: bool,
    /// Reads the atlas (else the ping-pong target); blurs only.
    pub from_atlas: bool,
    /// Writes the atlas (else the ping-pong target).
    pub to_atlas: bool,
    /// x, y (rows), width, height.
    pub viewport: [u32; 4],
    /// The shader's `PassUniform`, as f32s in its field order.
    pub uniform: [f32; 32],
}

/// `_createPlanes(lodMax)`: each level's face size and sigma.
fn planes() -> (Vec<u32>, Vec<f64>) {
    let total = (LOD_MAX - LOD_MIN + 1) as usize + EXTRA_LOD_SIGMA.len();
    let mut size_lods = Vec::new();
    let mut sigmas = Vec::new();
    let mut lod = LOD_MAX;
    for i in 0..total {
        let size_lod = 1u32 << lod;
        size_lods.push(size_lod);
        let mut sigma = 1.0 / f64::from(size_lod);
        if i > (LOD_MAX - LOD_MIN) as usize {
            sigma = EXTRA_LOD_SIGMA[i - (LOD_MAX - LOD_MIN) as usize - 1];
        } else if i == 0 {
            sigma = 0.0;
        }
        sigmas.push(sigma);
        if lod > LOD_MIN {
            lod -= 1;
        }
    }
    (size_lods, sigmas)
}

/// `_halfBlur`'s uniforms and viewport.
fn half_blur(
    size_lods: &[u32],
    lod_in: usize,
    lod_out: usize,
    sigma_radians: f64,
    latitudinal: bool,
    pole: [f64; 3],
) -> ([u32; 4], [f32; 32]) {
    const STANDARD_DEVIATIONS: f64 = 3.0;
    let pixels = f64::from(size_lods[lod_in] - 1);
    let radians_per_pixel = std::f64::consts::PI / (2.0 * pixels);
    let sigma_pixels = sigma_radians / radians_per_pixel;
    let samples = 1 + (STANDARD_DEVIATIONS * sigma_pixels).floor() as usize;
    if samples > MAX_SAMPLES {
        warn!("pmrem: sigma {sigma_radians} needs {samples} samples; clipped at {MAX_SAMPLES}");
    }
    let mut weights = [0f64; MAX_SAMPLES];
    let mut sum = 0.0;
    for (i, w) in weights.iter_mut().enumerate() {
        let x = i as f64 / sigma_pixels;
        *w = (-x * x / 2.0).exp();
        if i == 0 {
            sum += *w;
        } else if i < samples {
            sum += 2.0 * *w;
        }
    }
    let mut u = [0f32; 32];
    for (i, w) in weights.iter().enumerate() {
        u[i] = (w / sum) as f32;
    }
    u[20] = pole[0] as f32;
    u[21] = pole[1] as f32;
    u[22] = pole[2] as f32;
    u[24] = samples.min(MAX_SAMPLES) as f32;
    u[25] = if latitudinal { 1.0 } else { 0.0 };
    u[26] = radians_per_pixel as f32;
    u[27] = (f64::from(LOD_MAX) - lod_in as f64) as f32;
    let output_size = size_lods[lod_out];
    let x = 3
        * output_size
        * if lod_out > (LOD_MAX - LOD_MIN) as usize {
            (lod_out - (LOD_MAX - LOD_MIN) as usize) as u32
        } else {
            0
        };
    let y = 4 * (CUBE_SIZE - output_size);
    u[28] = x as f32;
    u[29] = y as f32;
    u[30] = output_size as f32;
    ([x, y, 3 * output_size, 2 * output_size], u)
}

/// The passes of `fromScene(sky, 0.04)` at the default size. `fresh`: the
/// generator's first run (see the module notes on the pole axis).
pub fn plan(fresh: bool) -> Vec<Pass> {
    let mut out = Vec::new();
    // _sceneToCubeUV: the six faces of the base level.
    for face in 0..6u32 {
        let col = face % 3;
        let vp = [
            col * CUBE_SIZE,
            if face > 2 { CUBE_SIZE } else { 0 },
            CUBE_SIZE,
            CUBE_SIZE,
        ];
        let mut u = [0f32; 32];
        u[28] = vp[0] as f32;
        u[29] = vp[1] as f32;
        u[30] = CUBE_SIZE as f32;
        u[31] = face as f32;
        out.push(Pass {
            sky: true,
            from_atlas: false,
            to_atlas: true,
            viewport: vp,
            uniform: u,
        });
    }
    let (size_lods, sigmas) = planes();
    let axes = axis_directions();
    let mut blur = |lod_in: usize, lod_out: usize, sigma: f64, pole: [f64; 3]| {
        let (vp, u) = half_blur(&size_lods, lod_in, lod_out, sigma, true, pole);
        out.push(Pass {
            sky: false,
            from_atlas: true,
            to_atlas: false,
            viewport: vp,
            uniform: u,
        });
        let (vp, u) = half_blur(&size_lods, lod_out, lod_out, sigma, false, pole);
        out.push(Pass {
            sky: false,
            from_atlas: false,
            to_atlas: true,
            viewport: vp,
            uniform: u,
        });
    };
    // fromScene's sigma: the pole axis is the blur material's current one.
    blur(
        0,
        0,
        SCENE_SIGMA,
        if fresh { [0.0, 1.0, 0.0] } else { axes[0] },
    );
    // _applyPMREM
    let n = size_lods.len();
    for i in 1..n {
        let sigma = (sigmas[i] * sigmas[i] - sigmas[i - 1] * sigmas[i - 1]).sqrt();
        let pole = axes[(n - i - 1) % axes.len()];
        blur(i - 1, i, sigma, pole);
    }
    out
}

/// The main world's request for an environment map.
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct EnvRequest {
    /// Bumped to ask for a new map (0: none yet).
    pub generation: u32,
    /// The sky's noise texture (`terrainDetailTexture`, the dome's `tNoise`).
    pub noise: Option<Handle<Image>>,
    /// Build as a fresh generator would (the material test scenes make one
    /// per page); otherwise only the first build is fresh, as the game's.
    pub fresh: bool,
}

/// The last generation the render world has built, for the main world to
/// wait on (the material test scenes).
pub static ENV_DONE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The atlas as an image every material binds: written by the render world.
pub fn env_image() -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: ENV_WIDTH,
            height: ENV_HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0u8; 8],
        ENV_FORMAT,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
        ..default()
    });
    image
}

#[derive(Resource)]
pub struct PmremPipelines {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    sky: CachedRenderPipelineId,
    blur: CachedRenderPipelineId,
}

const UNIFORM_SIZE: u64 = 128;

pub fn init_pmrem_pipelines(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "mp_pmrem_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer_sized(true, NonZeroU64::new(UNIFORM_SIZE)),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    let shader = bevy::asset::load_embedded_asset!(asset_server.as_ref(), "pmrem.wgsl");
    let pipeline = |entry: &'static str| {
        pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(format!("mp_pmrem_{entry}").into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format: ENV_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    };
    commands.insert_resource(PmremPipelines {
        sky: pipeline("sky_face"),
        blur: pipeline("blur"),
        layout,
        sampler,
    });
}

/// What the render world has built.
#[derive(Resource, Default)]
pub struct PmremState {
    done: u32,
    runs: u32,
    pingpong: Option<(Texture, TextureView)>,
}

/// Builds the environment map when the main world asked for a new one.
/// Runs in the camera's graph ahead of its main pass.
#[allow(clippy::too_many_arguments)]
pub fn build_env(
    req: Option<Res<EnvRequest>>,
    shared: Option<Res<SharedImages>>,
    images: Res<RenderAssets<GpuImage>>,
    fallback: Res<FallbackImage>,
    pipelines: Option<Res<PmremPipelines>>,
    pipeline_cache: Res<PipelineCache>,
    mut state: ResMut<PmremState>,
    mut ctx: RenderContext,
) {
    let (Some(req), Some(shared), Some(pipelines)) = (req, shared, pipelines) else {
        return;
    };
    if req.generation == 0 || req.generation == state.done {
        return;
    }
    let (Some(sky_pipeline), Some(blur_pipeline)) = (
        pipeline_cache.get_render_pipeline(pipelines.sky),
        pipeline_cache.get_render_pipeline(pipelines.blur),
    ) else {
        return;
    };
    let (Some(atlas), Some(globals)) = (images.get(&shared.env), images.get(&shared.globals))
    else {
        return;
    };
    let noise = req.noise.as_ref().and_then(|h| images.get(h));
    let (noise_view, noise_sampler) = match noise {
        Some(n) => (&n.texture_view, &n.sampler),
        None => (&fallback.d2.texture_view, &fallback.d2.sampler),
    };
    let device = ctx.render_device().clone();
    if state.pingpong.is_none() {
        let t = device.create_texture(&TextureDescriptor {
            label: Some("mp_pmrem_pingpong"),
            size: Extent3d {
                width: ENV_WIDTH,
                height: ENV_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: ENV_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let v = t.create_view(&TextureViewDescriptor::default());
        state.pingpong = Some((t, v));
    }
    let Some((_, pingpong)) = &state.pingpong else {
        return;
    };
    let passes = plan(state.runs == 0 || req.fresh);
    let a = u64::from(device.limits().min_uniform_buffer_offset_alignment).max(1);
    let align = UNIFORM_SIZE.div_ceil(a) * a;
    let mut data = vec![0u8; (align * passes.len() as u64) as usize];
    for (i, p) in passes.iter().enumerate() {
        let at = (align * i as u64) as usize;
        for (k, v) in p.uniform.iter().enumerate() {
            data[at + 4 * k..at + 4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("mp_pmrem_uniforms"),
        contents: &data,
        usage: BufferUsages::UNIFORM,
    });
    let binding = BufferBinding {
        buffer: &buffer,
        offset: 0,
        size: NonZeroU64::new(UNIFORM_SIZE),
    };
    let layout = pipeline_cache.get_bind_group_layout(&pipelines.layout);
    let bind = |source: &TextureView| {
        device.create_bind_group(
            "mp_pmrem_bind_group",
            &layout,
            &BindGroupEntries::sequential((
                binding.clone(),
                source,
                &pipelines.sampler,
                &globals.texture_view,
                noise_view,
                noise_sampler,
            )),
        )
    };
    let from_atlas = bind(&atlas.texture_view);
    let from_pingpong = bind(pingpong);
    let encoder = ctx.command_encoder();
    encoder.push_debug_group("mp_pmrem");
    for (i, p) in passes.iter().enumerate() {
        let target = if p.to_atlas {
            &atlas.texture_view
        } else {
            pingpong
        };
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("mp_pmrem_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(if p.sky { sky_pipeline } else { blur_pipeline });
        let group = if p.sky || !p.from_atlas {
            &from_pingpong
        } else {
            &from_atlas
        };
        pass.set_bind_group(0, group, &[(align * i as u64) as u32]);
        let [x, y, w, h] = p.viewport;
        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, w, h);
        pass.draw(0..3, 0..1);
    }
    encoder.pop_debug_group();
    state.done = req.generation;
    state.runs += 1;
    ENV_DONE.store(req.generation, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_follows_three() {
        let (sizes, sigmas) = planes();
        assert_eq!(sizes, vec![256, 128, 64, 32, 16, 16, 16, 16, 16, 16, 16]);
        assert_eq!(sigmas[0], 0.0);
        assert_eq!(sigmas[1], 1.0 / 128.0);
        assert_eq!(sigmas[5], 0.125);
        let p = plan(true);
        // Six faces, the base blur, ten level blurs, each blur in two halves.
        assert_eq!(p.len(), 6 + 2 + 10 * 2);
        // The base blur: 20 samples (0.04 rad at 255 pixels per quarter turn).
        assert_eq!(p[6].uniform[24], 20.0);
        assert_eq!(p[6].uniform[21], 1.0);
        assert_ne!(plan(false)[6].uniform[20], 0.0);
        // The last extra level sits at x = 3 × 16 × 6, y = 4 × (256 - 16).
        let last = p.last().unwrap();
        assert_eq!(last.viewport, [288, 960, 48, 32]);
        // Weights sum to one over the taps used.
        let w = &p[8].uniform;
        let n = w[24] as usize;
        let s: f32 = w[0] + 2.0 * w[1..n].iter().sum::<f32>();
        assert!((s - 1.0).abs() < 1e-5);
    }
}
