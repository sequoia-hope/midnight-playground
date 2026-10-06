//! Port of `src/world/Sky.js` (roadmap WP 3.5): sky dome, sun/moon light,
//! fog and exposure — all driven by how far along the route the player is,
//! using the level's time-of-day keys (Level 1 runs from afternoon sun to
//! midnight, Level 2 from blue hour to morning).
//!
//! Two layers:
//!
//! - [`SkyParams`] is the time of day as data: the level's keys prepared
//!   (`prepKeys`), [`SkyParams::sample`] (`Sky.sample`) and
//!   [`SkyParams::frame`], everything `Sky.update` computes for a point of
//!   the route (the dome's uniforms, the sun or moon light's colour,
//!   intensity and direction, the hemisphere light, fog, exposure, the
//!   night factor). It needs no scene: [`SkyParams::frame_at`] is "the sky
//!   at distance s" for the client and for headless use.
//! - [`Sky`] is the JS class in a scene: the dome (a `ShaderMaterial` with
//!   kind `SkyDome`, the JS GLSL carried along), the directional light with
//!   its shadow box and its target, the hemisphere light, all scene roots
//!   as three keeps them (not under the world root). [`Sky::update`] is
//!   `Sky.update(dt, s, focus)`: it returns the frame and writes the edits
//!   to the dome's uniforms, the lights and the nodes' positions.
//!
//! The fog patch (`MR_SUN_FOG`: the fog picks up the sun's colour looking
//! toward it, in every lit material) is the renderer's (SPEC 6.1, 6.2).

use mp_math::{clamp, js, kernel, lerp, smoothstep};
use mp_scene::{LightDesc, MaterialKind, NodeType, ShadowCamera, ShadowDesc, three};
use mp_track::{Level, SkyKey, Track};
use serde_json::{Value, json};

use crate::color::Color;
use crate::material::{Material, color_value, num, texture_value};
use crate::object::{Layer, MaterialId, NodeId, Object3D, SceneGraph};
use crate::textures::TextureCache;
use crate::three_geom::{Vector3, sphere_geometry};
use crate::world::{Change, Edit, Handle};

const PI: f64 = std::f64::consts::PI;

/// The level's default moon direction (`track.level.moonDir || [...]`).
pub const MOON_DIR: [f64; 3] = [-0.3, 0.55, 0.7];

/// A time-of-day key with its colours made (`prepKeys`): `{ ...k, zenC,
/// horC, sunC, hemiSC, hemiGC, fogC }`, linear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreparedKey {
    pub key: SkyKey,
    pub zen: Color,
    pub hor: Color,
    pub sun: Color,
    pub hemi_s: Color,
    pub hemi_g: Color,
    pub fog: Color,
}

/// `Sky.sample(p)`: the keys either side of p, smoothstepped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkySample {
    /// Sun elevation, degrees.
    pub sun_el: f64,
    pub sun_i: f64,
    pub hemi_i: f64,
    pub fog_d: f64,
    pub exp: f64,
    pub night: f64,
    pub zen: Color,
    pub hor: Color,
    pub sun: Color,
    pub hemi_s: Color,
    pub hemi_g: Color,
    pub fog: Color,
}

/// Everything `Sky.update` sets for a point of the route, before the
/// focus moves the lights and the dome.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyFrame {
    /// The fraction of the route the time of day was taken at.
    pub p: f64,
    pub key: SkySample,
    /// The night factor n (`sky.night`): 0 day, 1 night.
    pub night: f64,
    // The dome's uniforms.
    pub zenith: Color,
    pub horizon: Color,
    /// `uGround`: the fog colour.
    pub ground: Color,
    pub sun_color: Color,
    pub sun_dir: Vector3,
    pub moon_dir: Vector3,
    pub cloud: f64,
    pub haze: f64,
    /// The directional light: sun by day, crossfading to moonlight once the
    /// sun is gone, kept above the horizon so shadows stay sane. Its
    /// direction from the focus (unit).
    pub light_dir: Vector3,
    pub light_color: Color,
    pub light_intensity: f64,
    pub hemi_color: Color,
    pub hemi_ground: Color,
    pub hemi_intensity: f64,
    /// `FogExp2`.
    pub fog_color: Color,
    pub fog_density: f64,
    /// `renderer.toneMappingExposure`.
    pub exposure: f64,
}

/// The time of day along a level's route.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyParams {
    pub keys: Vec<PreparedKey>,
    /// Radians around Y; 0 = ahead along +X.
    pub sun_azimuth: f64,
    /// `uMoonDir`, normalised.
    pub moon_dir: Vector3,
    pub is_loop: bool,
    pub length: f64,
}

/// `prepKeys(keys)`.
pub fn prep_keys(keys: &[SkyKey]) -> Vec<PreparedKey> {
    keys.iter()
        .map(|k| PreparedKey {
            key: *k,
            zen: Color::hex(k.zen),
            hor: Color::hex(k.hor),
            sun: Color::hex(k.sun),
            hemi_s: Color::hex(k.hemi_s),
            hemi_g: Color::hex(k.hemi_g),
            fog: Color::hex(k.fog),
        })
        .collect()
}

impl SkyParams {
    /// The level's keys, sun azimuth and moon, on its track.
    pub fn new(level: &Level, track: &Track) -> SkyParams {
        let m = level.moon_dir.unwrap_or(MOON_DIR);
        SkyParams {
            keys: prep_keys(&level.sky),
            sun_azimuth: level.sun_azimuth,
            moon_dir: Vector3::new(m[0], m[1], m[2]).normalize(),
            is_loop: track.is_loop,
            length: track.length,
        }
    }

    /// The fraction of the route the sky shows at distance `s`: loops are
    /// pinned at 0.5, a point-to-point road runs 0 to 1.
    pub fn progress(&self, s: f64) -> f64 {
        if self.is_loop {
            0.5
        } else {
            clamp(s / self.length, 0.0, 1.0)
        }
    }

    /// `sample(p)`: interpolated key at fraction p.
    pub fn sample(&self, p: f64) -> SkySample {
        let k = &self.keys;
        let (mut a, mut b) = (&k[0], &k[k.len() - 1]);
        for i in 0..k.len() - 1 {
            if p >= k[i].key.s && p <= k[i + 1].key.s {
                a = &k[i];
                b = &k[i + 1];
                break;
            }
        }
        let t = smoothstep(a.key.s, b.key.s, p);
        let c = |x: Color, y: Color| {
            let mut o = x;
            o.lerp(y, t);
            o
        };
        SkySample {
            sun_el: lerp(a.key.sun_el, b.key.sun_el, t),
            sun_i: lerp(a.key.sun_i, b.key.sun_i, t),
            hemi_i: lerp(a.key.hemi_i, b.key.hemi_i, t),
            fog_d: lerp(a.key.fog_d, b.key.fog_d, t),
            exp: lerp(a.key.exp, b.key.exp, t),
            night: lerp(a.key.night, b.key.night, t),
            zen: c(a.zen, b.zen),
            hor: c(a.hor, b.hor),
            sun: c(a.sun, b.sun),
            hemi_s: c(a.hemi_s, b.hemi_s),
            hemi_g: c(a.hemi_g, b.hemi_g),
            fog: c(a.fog, b.fog),
        }
    }

    /// What `update` computes at fraction p.
    pub fn frame(&self, p: f64) -> SkyFrame {
        let k = self.sample(p);
        let mut sun_color = k.sun;
        sun_color.multiply_scalar(clamp(k.sun_i / 2.5, 0.25, 1.2) * (1.0 - k.night * 0.8));
        let el = (k.sun_el * PI) / 180.0;
        let az = self.sun_azimuth;
        let sun_dir = Vector3::new(
            kernel::cos(el) * kernel::cos(az),
            kernel::sin(el),
            kernel::cos(el) * kernel::sin(az),
        );
        // Directional light: sun by day, crossfading to moonlight once the
        // sun is gone. Keep it above the horizon so shadows stay sane.
        let to_moon = smoothstep(0.3, 0.75, k.night);
        let mut light_dir = sun_dir;
        light_dir.y = js::max(light_dir.y, 0.12);
        let light_dir = light_dir.lerp(self.moon_dir, to_moon).normalize();
        SkyFrame {
            p,
            key: k,
            night: k.night,
            zenith: k.zen,
            horizon: k.hor,
            ground: k.fog,
            sun_color,
            sun_dir,
            moon_dir: self.moon_dir,
            cloud: lerp(0.75, 0.35, k.night),
            // Thicker fog, deeper horizon haze.
            haze: clamp(k.fog_d / 0.00032, 0.45, 1.0) * 0.85,
            light_dir,
            light_color: k.sun,
            light_intensity: k.sun_i,
            hemi_color: k.hemi_s,
            hemi_ground: k.hemi_g,
            hemi_intensity: k.hemi_i,
            fog_color: k.fog,
            fog_density: k.fog_d,
            exposure: k.exp,
        }
    }

    /// The sky at distance `s` along the route (`override`, `?t=`, is a
    /// fraction given instead).
    pub fn frame_at(&self, s: f64, override_p: Option<f64>) -> SkyFrame {
        self.frame(override_p.unwrap_or_else(|| self.progress(s)))
    }
}

fn rgb(c: Color) -> [f64; 3] {
    [c.r, c.g, c.b]
}

/// The sky in a scene: `new Sky(scene, renderer, track, keys, sunAzimuth)`.
#[derive(Clone, Debug)]
pub struct Sky {
    pub params: SkyParams,
    /// `this.time`: seconds the sky has run (the clouds drift with it).
    pub time: f64,
    /// `this.night`.
    pub night: f64,
    /// Set to a fraction to pin time of day (`?t=`).
    pub override_p: Option<f64>,
    pub dome: NodeId,
    pub material: MaterialId,
    /// The directional light (`this.sun`) and its target.
    pub sun: NodeId,
    pub target: NodeId,
    pub hemi: NodeId,
    /// `this.dome.position`, which stays where it was without a focus.
    pub dome_position: [f64; 3],
}

impl Sky {
    /// The dome, the lights and the shadow box, added to the scene as
    /// roots (`scene.add`), with the JS constructor's initial values.
    pub fn new(
        graph: &mut SceneGraph,
        textures: &mut TextureCache,
        level: &Level,
        track: &Track,
    ) -> Sky {
        let params = SkyParams::new(level, track);
        let noise = graph.cached_texture(&textures.terrain_detail_texture(), Layer::Main, "");
        let white = color_value(Color::new(1.0, 1.0, 1.0));
        let m = params.moon_dir;
        let mat = Material::shader()
            .kind(MaterialKind::SkyDome, None)
            .shader_source(SKY_VERT, SKY_FRAG)
            .uniform("uZenith", white.clone())
            .uniform("uHorizon", white.clone())
            .uniform("uGround", white.clone())
            .uniform("uSunColor", white)
            .uniform("uSunDir", vec3(Vector3::new(0.0, 1.0, 0.0)))
            .uniform("uMoonDir", vec3(m))
            .uniform("uNight", num(0.0))
            .uniform("uTime", num(0.0))
            .uniform("uCloud", num(0.7))
            .uniform("uHaze", num(0.8))
            .uniform("tNoise", texture_value(noise))
            .set("side", f64::from(three::BACK_SIDE))
            .set("depthWrite", false)
            .set("depthTest", true)
            .set("fog", false);
        let material = graph.add_material(mat);
        let geo = graph.add_geometry(sphere_geometry(1.0, 48.0, 24.0, 0.0, 2.0 * PI, 0.0, PI));
        let dome = graph.mesh(geo, material);
        {
            let o = graph.get_mut(dome);
            o.frustum_culled = false;
            o.render_order = -10.0;
            o.scale = Vector3::new(5000.0, 5000.0, 5000.0);
        }
        graph.add_root(dome);

        // new THREE.DirectionalLight(0xffffff, 2.5), casting the one shadow.
        let mut sun = Object3D::new(NodeType::DirectionalLight);
        sun.position = Vector3::new(0.0, 1.0, 0.0);
        sun.cast_shadow = true;
        sun.light = Some(LightDesc {
            node: 0,
            ty: NodeType::DirectionalLight,
            color: [1.0, 1.0, 1.0],
            intensity: 2.5,
            cast_shadow: true,
            ground_color: None,
            distance: None,
            decay: None,
            angle: None,
            penumbra: None,
            target: Some([0.0, 0.0, 0.0]),
            shadow: Some(ShadowDesc {
                map_size: [2048.0, 2048.0],
                bias: -0.0004,
                normal_bias: 0.6,
                radius: 1.0,
                blur_samples: 8.0,
                camera: ShadowCamera {
                    ty: "OrthographicCamera".into(),
                    near: 1.0,
                    far: 600.0,
                    left: Some(-70.0),
                    right: Some(70.0),
                    top: Some(70.0),
                    bottom: Some(-70.0),
                    fov: None,
                },
            }),
        });
        let sun = graph.object(sun);
        let target = graph.object(Object3D::new(NodeType::Object3D));
        graph.add_root(sun);
        graph.add_root(target);

        // new THREE.HemisphereLight(0xffffff, 0x444444, 1).
        let mut hemi = Object3D::new(NodeType::HemisphereLight);
        hemi.position = Vector3::new(0.0, 1.0, 0.0);
        hemi.light = Some(LightDesc {
            node: 0,
            ty: NodeType::HemisphereLight,
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            cast_shadow: false,
            ground_color: Some(rgb(Color::hex(0x444444))),
            distance: None,
            decay: None,
            angle: None,
            penumbra: None,
            target: None,
            shadow: None,
        });
        let hemi = graph.object(hemi);
        graph.add_root(hemi);

        Sky {
            params,
            time: 0.0,
            night: 0.0,
            override_p: None,
            dome,
            material,
            sun,
            target,
            hemi,
            dome_position: [0.0, 0.0, 0.0],
        }
    }

    /// `update(dt, s, focus)`: the frame at the player's distance `s`, and
    /// the changes to the dome's uniforms, the lights (the sun placed 300 m
    /// from the focus along the light direction, its target at the focus)
    /// and the dome (centred on the focus). Without a focus the lights and
    /// the dome stay where they are.
    pub fn update(
        &mut self,
        dt: f64,
        s: f64,
        focus: Option<[f64; 3]>,
        out: &mut Vec<Edit>,
    ) -> SkyFrame {
        self.time += dt;
        let k = self.params.frame_at(s, self.override_p);
        self.night = k.night;
        let m = Handle::Material(self.material);
        let col = |prop: &'static str, c: Color| Edit {
            target: m,
            change: Change::Color { prop, rgb: rgb(c) },
        };
        let number = |prop: &'static str, value: f64| Edit {
            target: m,
            change: Change::Number { prop, value },
        };
        let vector = |prop: &'static str, v: Vector3| Edit {
            target: m,
            change: Change::Vector {
                prop,
                value: vec![v.x, v.y, v.z],
            },
        };
        out.push(col("uZenith", k.zenith));
        out.push(col("uHorizon", k.horizon));
        out.push(col("uGround", k.ground));
        out.push(col("uSunColor", k.sun_color));
        out.push(number("uNight", k.night));
        out.push(number("uTime", self.time));
        out.push(number("uCloud", k.cloud));
        out.push(number("uHaze", k.haze));
        out.push(vector("uSunDir", k.sun_dir));
        out.push(Edit {
            target: Handle::Node(self.sun),
            change: Change::Light {
                color: rgb(k.light_color),
                intensity: k.light_intensity,
                ground_color: None,
            },
        });
        let place = |node: NodeId, p: [f64; 3]| Edit {
            target: Handle::Node(node),
            change: Change::Transform {
                position: p,
                quaternion: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
        };
        if let Some(f) = focus {
            let d = k.light_dir;
            out.push(place(
                self.sun,
                [f[0] + d.x * 300.0, f[1] + d.y * 300.0, f[2] + d.z * 300.0],
            ));
            out.push(place(self.target, f));
            self.dome_position = f;
        }
        out.push(Edit {
            target: Handle::Node(self.hemi),
            change: Change::Light {
                color: rgb(k.hemi_color),
                intensity: k.hemi_intensity,
                ground_color: Some(rgb(k.hemi_ground)),
            },
        });
        out.push(Edit {
            target: Handle::Node(self.dome),
            change: Change::Transform {
                position: self.dome_position,
                quaternion: [0.0, 0.0, 0.0, 1.0],
                scale: [5000.0, 5000.0, 5000.0],
            },
        });
        k
    }

    /// Writes a sky update into the graph being built (the scene a build
    /// hands over shows the sky as it was last updated).
    pub fn apply(&self, graph: &mut SceneGraph, edits: &[Edit]) {
        for e in edits {
            match (&e.target, &e.change) {
                (Handle::Material(id), Change::Color { prop, rgb }) => {
                    set_uniform(
                        graph,
                        *id,
                        prop,
                        color_value(Color::new(rgb[0], rgb[1], rgb[2])),
                    );
                }
                (Handle::Material(id), Change::Number { prop, value }) => {
                    set_uniform(graph, *id, prop, num(*value));
                }
                (Handle::Material(id), Change::Vector { prop, value }) => {
                    let v: Vec<Value> = value.iter().map(|&x| num(x)).collect();
                    set_uniform(graph, *id, prop, json!({ "vec": v }));
                }
                (
                    Handle::Node(n),
                    Change::Light {
                        color,
                        intensity,
                        ground_color,
                    },
                ) => {
                    let o = graph.get_mut(*n);
                    if let Some(l) = &mut o.light {
                        l.color = *color;
                        l.intensity = *intensity;
                        if ground_color.is_some() {
                            l.ground_color = *ground_color;
                        }
                    }
                }
                (Handle::Node(n), Change::Transform { position, .. }) => {
                    let p = Vector3::new(position[0], position[1], position[2]);
                    graph.get_mut(*n).position = p;
                    if *n == self.target
                        && let Some(l) = &mut graph.get_mut(self.sun).light
                    {
                        l.target = Some(*position);
                    }
                }
                _ => {}
            }
        }
    }
}

fn vec3(v: Vector3) -> Value {
    json!({ "vec": [num(v.x), num(v.y), num(v.z)] })
}

fn set_uniform(graph: &mut SceneGraph, id: MaterialId, prop: &str, v: Value) {
    if let Some(u) = &mut graph.material_mut(id).desc.uniforms
        && let Some(slot) = u.get_mut(prop)
    {
        *slot = v;
    }
}

/// `skyVert`: the dome at the far plane (`xyww`).
pub const SKY_VERT: &str = r#"
varying vec3 vDir;
void main() {
  vDir = normalize(position);
  vec4 p = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  gl_Position = p.xyww;
}"#;

/// `skyFrag`. Everything is procedural: a gradient with a sun-side glow and
/// horizon haze that matches the fog, two cloud layers (puffy cumulus shaded
/// toward the sun, and thin cirrus streaks), sun disc, moon with maria, stars
/// and a faint Milky Way. Hashes avoid sin() so it stays cheap on phone GPUs.
pub const SKY_FRAG: &str = r#"
uniform vec3 uZenith, uHorizon, uGround, uSunColor;
uniform vec3 uSunDir, uMoonDir;
uniform float uNight, uTime, uCloud, uHaze;
varying vec3 vDir;

float hash(vec3 p) { p = fract(p * 0.3183099 + 0.1); p *= 17.0; return fract(p.x * p.y * p.z * (p.x + p.y + p.z)); }
float hash2(vec2 p) { vec3 p3 = fract(vec3(p.xyx) * 0.1031); p3 += dot(p3, p3.yzx + 33.33); return fract((p3.x + p3.y) * p3.z); }
float vnoise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash2(i), hash2(i + vec2(1, 0)), u.x), mix(hash2(i + vec2(0, 1)), hash2(i + vec2(1, 1)), u.x), u.y);
}
// Cloud noise from the shared tileable noise texture (broad noise in A,
// multi-octave detail in R): two fetches instead of dozens of hashes.
uniform sampler2D tNoise;
float cloudN(vec2 q) {
  float a = texture2D(tNoise, q * 0.05).a;
  float b = (texture2D(tNoise, q * 0.23 + 0.31).r - 0.59) * 2.4;
  return a * 0.62 + b * 0.38;
}

void main() {
  vec3 d = normalize(vDir);
  float h = d.y;
  float sd = dot(d, uSunDir);
  float sunAmt = max(sd, 0.0);
  float t = pow(clamp(h, 0.0, 1.0), 0.45);
  vec3 col = mix(uHorizon, uZenith, t);
  // Sun-side glow: a broad warm wash along the horizon plus a tighter halo.
  float sun2 = sunAmt * sunAmt;
  col += uSunColor * ((sun2 * sun2 * sun2) * 0.35 * (1.0 - t) + sun2 * 0.08 * (1.0 - t) * (1.0 - t));
  // Twilight arch: while the sun is just below the horizon, a warm band
  // hugs the horizon on its side of the sky (and a faint pink one opposite,
  // the Belt of Venus), so blue hour isn't a flat gradient.
  float twi = smoothstep(-0.22, -0.02, uSunDir.y) * (1.0 - smoothstep(0.03, 0.12, uSunDir.y));
  if (twi > 0.0) {
    float az = dot(normalize(d.xz + vec2(1e-4)), normalize(uSunDir.xz + vec2(1e-4)));
    float low = 1.0 - smoothstep(0.0, 0.28, max(h, 0.0));
    col += vec3(1.0, 0.5, 0.3) * twi * low * low * pow(max(az, 0.0), 3.0) * 0.16;
    col += vec3(0.5, 0.3, 0.42) * twi * (1.0 - smoothstep(0.02, 0.2, abs(h - 0.08))) * max(-az, 0.0) * 0.05;
  }
  // Horizon haze: the band just above the horizon fades into the fog colour
  // (plus the same sun tint the fog chunk adds), so fogged far terrain meets
  // the sky without a seam.
  vec3 hazeCol = uGround + uSunColor * (sun2 * sun2 * 0.06 + pow(sunAmt, 24.0) * 0.1);
  float haze = exp(-max(h, 0.0) * 16.0) * uHaze;
  col = mix(col, hazeCol, haze);
  // Below the horizon fade to the fog/ground tone.
  col = mix(col, hazeCol, smoothstep(0.0, -0.06, h));

  // Clouds.
  float cloudCover = 0.0;
  if (h > 0.0 && uCloud > 0.01) {
    vec2 cp = d.xz / (h + 0.09);
    vec2 wind = vec2(uTime * 0.005, uTime * 0.0018);
    float fade = smoothstep(0.0, 0.14, h);
    // Cumulus: domain-warped fbm, shaded by comparing density one step
    // toward the sun (thinner toward the sun = lit side).
    vec2 q = cp * 1.35 + wind;
    q += (texture2D(tNoise, q * 0.021 + 0.7).a - 0.5) * 1.2;
    float n = cloudN(q);
    vec2 toSun = normalize(uSunDir.xz + vec2(1e-4)) * 0.3;
    float n2 = cloudN(q + toSun);
    float cov = mix(0.6, 0.42, uCloud);
    float dens = smoothstep(cov, cov + 0.22, n);
    float lit = clamp(0.55 + (n - n2) * 5.0, 0.0, 1.0);
    // Base (shadowed) and lit tones: sky-tinted greys by day, sun-coloured
    // at dawn/dusk, deep blue at night with moonlit edges.
    vec3 shadowC = mix(uZenith, uHorizon, 0.55) * 0.72 + 0.03;
    vec3 litC = uSunColor * 0.95 + uHorizon * 0.35 + 0.04;
    float day = 1.0 - uNight;
    vec3 cc = mix(shadowC, litC, lit * mix(0.35, 1.0, day));
    // Silver lining: thin edges near the sun glow.
    cc += uSunColor * pow(sunAmt, 10.0) * (1.0 - dens) * 1.6;
    cc = mix(cc, uHorizon * 0.35 + vec3(0.02, 0.025, 0.05) + vec3(0.1, 0.11, 0.14) * lit * pow(max(dot(d, uMoonDir), 0.0), 6.0), uNight * 0.85);
    // Distant clouds sink into the haze.
    cc = mix(cc, hazeCol, (1.0 - smoothstep(0.02, 0.35, h)) * 0.6);
    // Cirrus: high thin streaks, stretched along the wind.
    vec2 cq = d.xz / (h + 0.3);
    vec2 cs = vec2(cq.x * 1.2 + cq.y * 0.4, cq.y * 5.0 - cq.x * 1.5) + wind * 2.5;
    float ci = texture2D(tNoise, cs * 0.06 + 0.13).a * 0.7 + (texture2D(tNoise, cs * 0.25).r - 0.59) * 0.8;
    ci = smoothstep(0.55, 0.85, ci) * 0.4 * (1.0 - dens);
    vec3 ciC = mix(uHorizon * 1.05 + 0.05, uSunColor * 0.8 + uHorizon * 0.4, pow(sunAmt, 2.0));
    ciC = mix(ciC, uHorizon * 0.5, uNight * 0.8);
    col = mix(col, ciC, ci * uCloud * fade);
    cloudCover = clamp((dens * min(uCloud * 1.25, 1.0) + ci * uCloud) * fade, 0.0, 1.0);
    col = mix(col, cc, dens * min(uCloud * 1.25, 1.0) * fade);
  }

  // Sun disc and glow.
  col += uSunColor * (smoothstep(0.9993, 0.9997, sd) * 6.0 + pow(sunAmt, 180.0) * 0.8 + pow(sunAmt, 32.0) * 0.18) * step(-0.02, h) * (1.0 - cloudCover * 0.8);
  // Moon: disc with dusky maria, plus a soft halo.
  float md = dot(d, uMoonDir);
  float disc = smoothstep(0.99955, 0.99975, md);
  float maria = 0.0;
  if (md > 0.9995) maria = vnoise(d.xz * 1400.0 + d.y * 900.0) * 0.35 + vnoise(d.xz * 3100.0) * 0.15;
  col += vec3(0.85, 0.9, 1.0) * uNight * (1.0 - cloudCover * 0.7) * (disc * (2.4 - maria * 1.6) + pow(max(md, 0.0), 400.0) * 0.25 + pow(max(md, 0.0), 30.0) * 0.05);
  // Stars (twinkling, slightly tinted) and the Milky Way band.
  if (uNight > 0.5 && h > 0.0) {
    float nightSky = smoothstep(0.5, 0.95, uNight) * smoothstep(0.0, 0.25, h);
    vec3 sp = d * 420.0;
    vec3 cell = floor(sp);
    float r = hash(cell);
    float star = step(0.9965, r) * smoothstep(0.55, 0.0, length(fract(sp) - 0.5));
    float tw = 0.7 + 0.3 * sin(uTime * 3.0 + r * 60.0);
    vec3 tint = mix(vec3(1.0, 0.85, 0.7), vec3(0.75, 0.85, 1.0), fract(r * 91.0));
    // A band across the sky, tilted: faint glow plus a denser star field.
    float bq = dot(d, normalize(vec3(0.35, 0.45, -0.82))) * 5.0;
    float band = exp(-bq * bq);
    float mw = band * (0.5 + texture2D(tNoise, d.xz / (h + 0.4) * 0.4 + d.y).r) * 0.5;
    float faint = step(0.985 - band * 0.01, hash(floor(d * 900.0))) * 0.35;
    col += (tint * star * tw * 1.4 + vec3(0.8, 0.85, 1.0) * faint * band + vec3(0.07, 0.075, 0.1) * mw) * nightSky * (1.0 - cloudCover);
  }
  gl_FragColor = vec4(col, 1.0);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}"#;
