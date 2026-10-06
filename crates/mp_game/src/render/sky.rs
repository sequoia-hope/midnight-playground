//! The sky (`src/world/Sky.js`): the time-of-day keys sampled along the
//! route ([`SkyState`], a port of `Sky.sample` and `Sky.update`), which set
//! the dome's uniforms, the sun or moon, the hemisphere light, the fog and
//! the exposure; and the dome itself ([`SkyMaterial`], the SkyDome kind,
//! drawing `sky.wgsl`).
//!
//! World generation will own `Sky.js`'s parameters when the world is
//! generated in Rust (roadmap WP 3.5); until then the client samples the
//! level's keys from `mp_levels` itself (DECISIONS D173).

use super::lighting::{Hemi, Lighting, ShadowParams, SkyUniforms, Sun};
use bevy::math::DVec3;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MeshPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use mp_math::{clamp, kernel, lerp, smoothstep};
use mp_track::{Level, SkyKey};

/// `SRGBToLinear` (three r180 ColorManagement).
pub fn srgb_to_linear(c: f64) -> f64 {
    if c < 0.04045 {
        c * 0.0773993808
    } else {
        kernel::pow(c * 0.9478672986 + 0.0521327014, 2.4)
    }
}

/// `new THREE.Color(hex)`: sRGB hex to linear working colour.
pub fn hex_color(hex: u32) -> [f64; 3] {
    let c = |shift: u32| srgb_to_linear(f64::from((hex >> shift) & 255) / 255.0);
    [c(16), c(8), c(0)]
}

/// `Color.lerp`.
fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// `Vector3.normalize` (`divideScalar(length() || 1)`).
fn normalize(v: DVec3) -> DVec3 {
    let l = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    let l = if l == 0.0 { 1.0 } else { l };
    v / l
}

/// A key with its colours converted (`prepKeys`).
#[derive(Clone, Copy, Debug)]
struct Key {
    s: f64,
    sun_el: f64,
    zen: [f64; 3],
    hor: [f64; 3],
    sun: [f64; 3],
    sun_i: f64,
    hemi_s: [f64; 3],
    hemi_g: [f64; 3],
    hemi_i: f64,
    fog: [f64; 3],
    fog_d: f64,
    exp: f64,
    night: f64,
}

impl From<&SkyKey> for Key {
    fn from(k: &SkyKey) -> Key {
        Key {
            s: k.s,
            sun_el: k.sun_el,
            zen: hex_color(k.zen),
            hor: hex_color(k.hor),
            sun: hex_color(k.sun),
            sun_i: k.sun_i,
            hemi_s: hex_color(k.hemi_s),
            hemi_g: hex_color(k.hemi_g),
            hemi_i: k.hemi_i,
            fog: hex_color(k.fog),
            fog_d: k.fog_d,
            exp: k.exp,
            night: k.night,
        }
    }
}

/// The interpolated key (`Sky.sample`'s `this._k`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sampled {
    pub sun_el: f64,
    pub sun_i: f64,
    pub hemi_i: f64,
    pub fog_d: f64,
    pub exp: f64,
    pub night: f64,
    pub zen: [f64; 3],
    pub hor: [f64; 3],
    pub sun: [f64; 3],
    pub hemi_s: [f64; 3],
    pub hemi_g: [f64; 3],
    pub fog: [f64; 3],
}

/// `Sky`'s state: the keys, the clock, and the time-of-day override.
#[derive(Resource, Clone, Debug)]
pub struct SkyState {
    keys: Vec<Key>,
    pub sun_azimuth: f64,
    pub moon_dir: DVec3,
    /// The Track: a loop pins the time of day at the middle.
    pub is_loop: bool,
    pub length: f64,
    /// `this.time`, advanced by `update`.
    pub time: f64,
    pub night: f64,
    /// `?t=`: a fixed fraction of the route.
    pub override_p: Option<f64>,
}

impl SkyState {
    pub fn new(level: &Level, is_loop: bool, length: f64) -> SkyState {
        let m = level.moon_dir.unwrap_or([-0.3, 0.55, 0.7]);
        SkyState {
            keys: level.sky.iter().map(Key::from).collect(),
            sun_azimuth: level.sun_azimuth,
            moon_dir: normalize(DVec3::new(m[0], m[1], m[2])),
            is_loop,
            length,
            time: 0.0,
            night: 0.0,
            override_p: None,
        }
    }

    /// The route fraction the sky shows at `s` (`Sky.update`'s `p`).
    pub fn progress(&self, s: f64) -> f64 {
        self.override_p.unwrap_or(if self.is_loop {
            0.5
        } else {
            clamp(s / self.length, 0.0, 1.0)
        })
    }

    /// `Sky.sample(p)`.
    pub fn sample(&self, p: f64) -> Sampled {
        let k = &self.keys;
        let (mut a, mut b) = (k[0], k[k.len() - 1]);
        for i in 0..k.len() - 1 {
            if p >= k[i].s && p <= k[i + 1].s {
                a = k[i];
                b = k[i + 1];
                break;
            }
        }
        let t = smoothstep(a.s, b.s, p);
        Sampled {
            sun_el: lerp(a.sun_el, b.sun_el, t),
            sun_i: lerp(a.sun_i, b.sun_i, t),
            hemi_i: lerp(a.hemi_i, b.hemi_i, t),
            fog_d: lerp(a.fog_d, b.fog_d, t),
            exp: lerp(a.exp, b.exp, t),
            night: lerp(a.night, b.night, t),
            zen: lerp3(a.zen, b.zen, t),
            hor: lerp3(a.hor, b.hor, t),
            sun: lerp3(a.sun, b.sun, t),
            hemi_s: lerp3(a.hemi_s, b.hemi_s, t),
            hemi_g: lerp3(a.hemi_g, b.hemi_g, t),
            fog: lerp3(a.fog, b.fog, t),
        }
    }

    /// `Sky.update(dt, s, focus)`: the dome's uniforms, the light, the
    /// hemisphere light, the fog and the exposure, written into `out`.
    pub fn update(&mut self, dt: f64, s: f64, focus: DVec3, out: &mut Lighting) -> Sampled {
        self.time += dt;
        let p = self.progress(s);
        let k = self.sample(p);
        self.night = k.night;
        let sun_scale = clamp(k.sun_i / 2.5, 0.25, 1.2) * (1.0 - k.night * 0.8);
        let el = (k.sun_el * std::f64::consts::PI) / 180.0;
        let az = self.sun_azimuth;
        let sun_dir = DVec3::new(
            kernel::cos(el) * kernel::cos(az),
            kernel::sin(el),
            kernel::cos(el) * kernel::sin(az),
        );
        out.sky = SkyUniforms {
            zenith: k.zen,
            horizon: k.hor,
            ground: k.fog,
            sun_color: [
                k.sun[0] * sun_scale,
                k.sun[1] * sun_scale,
                k.sun[2] * sun_scale,
            ],
            sun_dir: [sun_dir.x, sun_dir.y, sun_dir.z],
            moon_dir: [self.moon_dir.x, self.moon_dir.y, self.moon_dir.z],
            night: k.night,
            time: self.time,
            cloud: lerp(0.75, 0.35, k.night),
            // Thicker fog, deeper horizon haze.
            haze: clamp(k.fog_d / 0.00032, 0.45, 1.0) * 0.85,
        };
        // Directional light: sun by day, crossfading to moonlight once the sun
        // is gone. Keep it above the horizon so shadows stay sane.
        let to_moon = smoothstep(0.3, 0.75, k.night);
        let mut light_dir = sun_dir;
        light_dir.y = light_dir.y.max(0.12);
        light_dir += (self.moon_dir - light_dir) * to_moon;
        let light_dir = normalize(light_dir);
        out.sun = Some(Sun {
            color: k.sun,
            intensity: k.sun_i,
            position: focus + light_dir * 300.0,
            target: focus,
            shadow: Some(ShadowParams::SKY),
        });
        out.hemi = Some(Hemi {
            sky: k.hemi_s,
            ground: k.hemi_g,
            intensity: k.hemi_i,
            position: DVec3::Y,
        });
        out.fog = Some((k.fog, k.fog_d));
        out.exposure = k.exp;
        k
    }
}

/// `terrainDetailTexture()` (the dome's `tNoise`) as a Bevy image, from
/// `mp_worldgen`'s port, for a scene that has no dome of its own.
pub fn noise_image() -> Option<Image> {
    let t = mp_worldgen::textures::terrain_detail_texture();
    let mut scene = mp_scene::Scene::default();
    scene.buffers.push(mp_scene::Buffer {
        item_size: 4,
        normalized: false,
        data: mp_scene::BufferData::U8(t.rgba.clone()),
    });
    let desc = t.desc("terrainDetail", 0);
    crate::convert::build_image(&scene, &desc)
}

/// The SkyDome kind: `sky.wgsl` on the dome sphere (BackSide, at the far
/// plane, no depth write, no fog, no shadows).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct SkyMaterial {
    #[texture(10, sample_type = "float", filterable = false)]
    pub globals: Handle<Image>,
    #[texture(13)]
    #[sampler(14)]
    pub noise: Handle<Image>,
}

impl Material for SkyMaterial {
    fn vertex_shader() -> ShaderRef {
        "embedded://mp_game/render/sky_dome.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "embedded://mp_game/render/sky_dome.wgsl".into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.mesh_key.contains(MeshPipelineKey::DEPTH_PREPASS) {
            return Ok(());
        }
        descriptor.primitive.cull_mode = Some(Face::Front);
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(false);
            ds.depth_compare = Some(CompareFunction::GreaterEqual);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colours_are_three_srgb() {
        let c = hex_color(0xffffff);
        assert_eq!(c, [1.0, 1.0, 1.0]);
        let c = hex_color(0x808080);
        assert!(c[0] == 0.21586050010324417, "{c:?}");
    }

    /// The sky at the start of each level against the export's dome
    /// uniforms and lights (taken at s = 0, the same point), when the
    /// export is in the parity cache.
    #[test]
    fn sky_matches_the_export_at_the_start() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Ok(dir) = mp_scene::cache::scenes_dir(&root) else {
            return;
        };
        for level in mp_levels::levels() {
            if level.id == "seaside" {
                continue; // needs its survey for the Track; the sky is a loop's
            }
            let path = dir.join(format!("{}.mrscene", level.id));
            let Ok(bytes) = std::fs::read(&path) else {
                eprintln!("skipping {}: no export in the cache", level.id);
                continue;
            };
            let scene = mp_scene::read(&bytes).expect("scene");
            let track = mp_track::Track::new(&level).expect("track");
            let mut sky = SkyState::new(&level, track.is_loop, track.length);
            let mut out = Lighting::default();
            sky.update(0.0, 0.0, DVec3::ZERO, &mut out);
            let dome = scene
                .materials
                .iter()
                .find(|m| m.kind == mp_scene::MaterialKind::SkyDome)
                .expect("a dome");
            let near = |name: &str, want: [f64; 3]| {
                let have = dome.color(name).unwrap_or_else(|| {
                    let v = dome.get(name).and_then(|v| v.get("vec")).unwrap();
                    let a = v.as_array().unwrap();
                    [0, 1, 2].map(|i| a[i].as_f64().unwrap())
                });
                for i in 0..3 {
                    assert!(
                        (have[i] - want[i]).abs() < 1e-12,
                        "{} {name}: {have:?} vs {want:?}",
                        level.id
                    );
                }
            };
            near("uZenith", out.sky.zenith);
            near("uHorizon", out.sky.horizon);
            near("uGround", out.sky.ground);
            near("uSunColor", out.sky.sun_color);
            near("uSunDir", out.sky.sun_dir);
            near("uMoonDir", out.sky.moon_dir);
            for (name, v) in [
                ("uNight", out.sky.night),
                ("uCloud", out.sky.cloud),
                ("uHaze", out.sky.haze),
            ] {
                let have = dome.number(name).unwrap();
                assert!(
                    (have - v).abs() < 1e-12,
                    "{} {name}: {have} vs {v}",
                    level.id
                );
            }
            let env = scene.environment.as_ref().unwrap();
            assert!((env.tone_mapping_exposure - out.exposure).abs() < 1e-12);
            let fog = env.fog.as_ref().unwrap();
            let (fc, fd) = out.fog.unwrap();
            assert!((fog.density.unwrap() - fd).abs() < 1e-12);
            assert!((0..3).all(|i| (fog.color[i] - fc[i]).abs() < 1e-12));
        }
    }
}
