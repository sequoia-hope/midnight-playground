//! The scene's light, fog, environment and exposure as three.js holds them
//! (the sun and its shadow camera, the hemisphere light, `FogExp2`,
//! `scene.environmentIntensity`, `toneMappingExposure`, the sky dome's
//! uniforms), packed into the globals texture every material reads
//! (`three_globals.wgsl`), and the sun's shadow camera handed to Bevy's
//! shadow pass as its one cascade.
//!
//! Whoever owns the scene fills [`Lighting`]: the sky's time of day for a
//! level (`render::sky`), the fixed setup for a material test scene
//! (`matscene`).

use bevy::light::cascade::{Cascade, Cascades};
use bevy::math::{DMat4, DVec3, DVec4};
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;

/// Texels in the globals row (`three_globals.wgsl`).
pub const GLOBALS_WIDTH: usize = 32;
pub const G_SUN_DIR: usize = 0;
pub const G_SUN_COLOR: usize = 1;
pub const G_HEMI_SKY: usize = 2;
pub const G_HEMI_GROUND: usize = 3;
pub const G_HEMI_DIR: usize = 4;
pub const G_FOG: usize = 5;
pub const G_SHADOW_M: usize = 6;
pub const G_SHADOW: usize = 10;
pub const G_AMBIENT: usize = 11;
pub const G_SKY_ZENITH: usize = 12;
pub const G_SKY_HORIZON: usize = 13;
pub const G_SKY_GROUND: usize = 14;
pub const G_SKY_SUN: usize = 15;
pub const G_SKY_SUN_DIR: usize = 16;
pub const G_SKY_MOON_DIR: usize = 17;
pub const G_SKY_PARAMS: usize = 18;
pub const G_SPOT_POS: usize = 19;
pub const G_SPOT_DIR: usize = 20;
pub const G_SPOT_COLOR: usize = 21;
pub const G_SPOT_CONE: usize = 22;
pub const G_POINT_POS: usize = 23;
pub const G_POINT_COLOR: usize = 24;
pub const G_ANIM: usize = 25;
pub const G_ANIM2: usize = 26;

/// The per-frame state the WP 2.4 patches read, which the JS keeps in
/// uniforms its updaters move (`World.js`, `Sea.js`, `desert/glow.js`):
/// one road and one sea per level, so they live with the scene-wide inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anim {
    /// The asphalt's `uWet` (`Road.setNight`).
    pub wet: f64,
    /// The sea's `uTime` and `uOff2`.
    pub sea_time: f64,
    pub sea_off2: [f64; 2],
    /// How far the sea's normal map has scrolled (`normalMap.offset`) since
    /// the export, added to its uv transform.
    pub sea_normal_offset: [f64; 2],
    /// `glowTime` (the desert's flicker clock).
    pub glow_time: f64,
    /// three's pixel ratio: rendered pixels per CSS pixel (points' sizes).
    pub pixel_ratio: f64,
}

impl Default for Anim {
    fn default() -> Self {
        Anim {
            wet: 0.0,
            sea_time: 0.0,
            sea_off2: [0.0; 2],
            sea_normal_offset: [0.0; 2],
            glow_time: 0.0,
            pixel_ratio: 1.0,
        }
    }
}

impl Anim {
    /// One frame of the updaters, after `Sky.update` (`World.update`): the
    /// road's damp follows nightfall (`World.js`: `road.setNight(smoothstep(
    /// 0.55, 1.0, n) * 0.85)`, run even when frozen), the sea's ripples and
    /// foam and the desert's flicker move with `dt` (0 when frozen).
    pub fn advance(&mut self, dt: f64, night: f64) {
        self.wet = smooth(0.55, 1.0, night) * 0.85;
        self.sea_normal_offset[0] += dt * 0.012;
        self.sea_normal_offset[1] += dt * 0.007;
        self.sea_time += dt;
        self.sea_off2[0] -= dt * 0.021;
        self.sea_off2[1] += dt * 0.016;
        self.glow_time += dt;
    }
}

/// `util/math.js` `smoothstep`.
fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A `DirectionalLightShadow` with an orthographic camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowParams {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub near: f64,
    pub far: f64,
    pub bias: f64,
    pub normal_bias: f64,
    pub map_size: u32,
}

impl ShadowParams {
    /// `Sky.js`: a ±70 m box, near 1, far 600, 2048², bias -0.0004, normal
    /// bias 0.6.
    pub const SKY: ShadowParams = ShadowParams {
        left: -70.0,
        right: 70.0,
        top: 70.0,
        bottom: -70.0,
        near: 1.0,
        far: 600.0,
        bias: -0.0004,
        normal_bias: 0.6,
        map_size: 2048,
    };
}

/// The directional light (sun or moon).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    /// Linear colour; three's uniform is colour × intensity.
    pub color: [f64; 3],
    pub intensity: f64,
    pub position: DVec3,
    pub target: DVec3,
    pub shadow: Option<ShadowParams>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hemi {
    pub sky: [f64; 3],
    pub ground: [f64; 3],
    pub intensity: f64,
    /// The light's position: its direction is this, normalised.
    pub position: DVec3,
}

/// The sky dome's uniforms (`Sky.js` `this.uniforms`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SkyUniforms {
    pub zenith: [f64; 3],
    pub horizon: [f64; 3],
    pub ground: [f64; 3],
    pub sun_color: [f64; 3],
    pub sun_dir: [f64; 3],
    pub moon_dir: [f64; 3],
    pub night: f64,
    pub time: f64,
    pub cloud: f64,
    pub haze: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub color: [f64; 3],
    pub intensity: f64,
    pub position: DVec3,
    pub target: DVec3,
    pub distance: f64,
    pub decay: f64,
    pub angle: f64,
    pub penumbra: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub color: [f64; 3],
    pub intensity: f64,
    pub position: DVec3,
    pub distance: f64,
    pub decay: f64,
}

/// Everything scene-wide that three's shaders read.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Lighting {
    pub sun: Option<Sun>,
    pub hemi: Option<Hemi>,
    /// `FogExp2` colour and density.
    pub fog: Option<([f64; 3], f64)>,
    /// `scene.environmentIntensity` with an environment; 0 without.
    pub env_intensity: f64,
    /// `renderer.toneMappingExposure`.
    pub exposure: f64,
    pub sky: SkyUniforms,
    pub spot: Option<Spot>,
    pub point: Option<Point>,
    /// The shadow map is on (the JS "high quality" setting).
    pub shadows: bool,
    /// The patches' per-frame state (WP 2.4).
    pub anim: Anim,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            sun: None,
            hemi: None,
            fog: None,
            env_intensity: 0.0,
            exposure: 1.0,
            sky: SkyUniforms::default(),
            spot: None,
            point: None,
            shadows: true,
            anim: Anim::default(),
        }
    }
}

/// three's `Matrix4.lookAt(eye, target, up)` with the eye as translation: a
/// camera's `matrixWorld` after `camera.lookAt(target)` (x right, y up,
/// looking down -z).
pub fn look_at(eye: DVec3, target: DVec3, up: DVec3) -> DMat4 {
    let mut z = eye - target;
    if z.length_squared() == 0.0 {
        z.z = 1.0;
    }
    z = z.normalize();
    let mut x = up.cross(z);
    if x.length_squared() == 0.0 {
        if up.z.abs() == 1.0 {
            z.x += 0.0001;
        } else {
            z.z += 0.0001;
        }
        z = z.normalize();
        x = up.cross(z);
    }
    x = x.normalize();
    let y = z.cross(x);
    DMat4::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), eye.extend(1.0))
}

/// `Matrix4.makeOrthographic` (WebGL depth, -1..1).
pub fn orthographic(l: f64, r: f64, t: f64, b: f64, near: f64, far: f64) -> DMat4 {
    let w = 1.0 / (r - l);
    let h = 1.0 / (t - b);
    let p = 1.0 / (far - near);
    DMat4::from_cols(
        DVec4::new(2.0 * w, 0.0, 0.0, 0.0),
        DVec4::new(0.0, 2.0 * h, 0.0, 0.0),
        DVec4::new(0.0, 0.0, -2.0 * p, 0.0),
        DVec4::new(-(r + l) * w, -(t + b) * h, -(far + near) * p, 1.0),
    )
}

impl Sun {
    /// The shadow camera's world matrix (`DirectionalLightShadow
    /// .updateMatrices`: at the light, looking at the target, y up).
    pub fn shadow_camera(&self) -> DMat4 {
        look_at(self.position, self.target, DVec3::Y)
    }

    /// three's `shadow.matrix`: texture space (0..1, depth 0 at near) from
    /// world space.
    pub fn shadow_matrix(&self, s: &ShadowParams) -> DMat4 {
        let bias = DMat4::from_cols(
            DVec4::new(0.5, 0.0, 0.0, 0.0),
            DVec4::new(0.0, 0.5, 0.0, 0.0),
            DVec4::new(0.0, 0.0, 0.5, 0.0),
            DVec4::new(0.5, 0.5, 0.5, 1.0),
        );
        bias * orthographic(s.left, s.right, s.top, s.bottom, s.near, s.far)
            * self.shadow_camera().inverse()
    }

    /// The same camera as a Bevy cascade: Bevy's reversed depth (1 at near,
    /// 0 at far, linear), so the map holds 1 - three's depth.
    pub fn cascade(&self, s: &ShadowParams) -> Cascade {
        let world_from_cascade = self.shadow_camera();
        let w = 1.0 / (s.right - s.left);
        let h = 1.0 / (s.top - s.bottom);
        let p = 1.0 / (s.far - s.near);
        let clip_from_cascade = DMat4::from_cols(
            DVec4::new(2.0 * w, 0.0, 0.0, 0.0),
            DVec4::new(0.0, 2.0 * h, 0.0, 0.0),
            DVec4::new(0.0, 0.0, p, 0.0),
            DVec4::new(
                -(s.right + s.left) * w,
                -(s.top + s.bottom) * h,
                s.far * p,
                1.0,
            ),
        );
        let clip_from_world = clip_from_cascade * world_from_cascade.inverse();
        Cascade {
            world_from_cascade: world_from_cascade.as_mat4(),
            clip_from_cascade: clip_from_cascade.as_mat4(),
            clip_from_world: clip_from_world.as_mat4(),
            texel_size: ((s.right - s.left) / f64::from(s.map_size)) as f32,
        }
    }
}

fn v3(a: [f64; 3], w: f64) -> [f32; 4] {
    [a[0] as f32, a[1] as f32, a[2] as f32, w as f32]
}

fn scaled(c: [f64; 3], k: f64) -> [f64; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn dv(v: DVec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

impl Lighting {
    /// The globals row (`three_globals.wgsl`).
    pub fn pack(&self) -> [[f32; 4]; GLOBALS_WIDTH] {
        let mut g = [[0f32; 4]; GLOBALS_WIDTH];
        if let Some(sun) = &self.sun {
            let dir = (sun.position - sun.target).normalize_or_zero();
            g[G_SUN_DIR] = v3(dv(dir), 1.0);
            let casts = self.shadows && sun.shadow.is_some();
            g[G_SUN_COLOR] = v3(
                scaled(sun.color, sun.intensity),
                if casts { 1.0 } else { 0.0 },
            );
            if let Some(s) = &sun.shadow {
                let m = sun.shadow_matrix(s).as_mat4();
                for (i, c) in [m.x_axis, m.y_axis, m.z_axis, m.w_axis].iter().enumerate() {
                    g[G_SHADOW_M + i] = c.to_array();
                }
                g[G_SHADOW] = [s.bias as f32, s.normal_bias as f32, s.map_size as f32, 1.0];
            }
        }
        if let Some(h) = &self.hemi {
            g[G_HEMI_SKY] = v3(scaled(h.sky, h.intensity), 1.0);
            g[G_HEMI_GROUND] = v3(scaled(h.ground, h.intensity), 0.0);
            g[G_HEMI_DIR] = v3(dv(h.position.normalize_or_zero()), self.env_intensity);
        } else {
            g[G_HEMI_DIR] = [0.0, 1.0, 0.0, self.env_intensity as f32];
        }
        if let Some((c, d)) = self.fog {
            g[G_FOG] = v3(c, d);
        }
        g[G_AMBIENT] = [0.0, 0.0, 0.0, self.exposure as f32];
        let s = &self.sky;
        g[G_SKY_ZENITH] = v3(s.zenith, 0.0);
        g[G_SKY_HORIZON] = v3(s.horizon, 0.0);
        g[G_SKY_GROUND] = v3(s.ground, 0.0);
        g[G_SKY_SUN] = v3(s.sun_color, 0.0);
        g[G_SKY_SUN_DIR] = v3(s.sun_dir, 0.0);
        g[G_SKY_MOON_DIR] = v3(s.moon_dir, 0.0);
        g[G_SKY_PARAMS] = [s.night as f32, s.time as f32, s.cloud as f32, s.haze as f32];
        if let Some(sp) = &self.spot {
            g[G_SPOT_POS] = v3(dv(sp.position), sp.distance);
            g[G_SPOT_DIR] = v3(dv((sp.target - sp.position).normalize_or_zero()), sp.decay);
            g[G_SPOT_COLOR] = v3(scaled(sp.color, sp.intensity), 1.0);
            // SpotLight uniforms: coneCos = cos(angle), penumbraCos =
            // cos(angle × (1 - penumbra)).
            g[G_SPOT_CONE] = [
                sp.angle.cos() as f32,
                (sp.angle * (1.0 - sp.penumbra)).cos() as f32,
                0.0,
                0.0,
            ];
        }
        if let Some(p) = &self.point {
            g[G_POINT_POS] = v3(dv(p.position), p.distance);
            g[G_POINT_COLOR] = v3(scaled(p.color, p.intensity), p.decay);
        }
        let a = &self.anim;
        g[G_ANIM] = [
            a.wet as f32,
            a.sea_time as f32,
            a.sea_off2[0] as f32,
            a.sea_off2[1] as f32,
        ];
        g[G_ANIM2] = [
            a.sea_normal_offset[0] as f32,
            a.sea_normal_offset[1] as f32,
            a.glow_time as f32,
            a.pixel_ratio as f32,
        ];
        g
    }
}

/// The globals row as the render world receives it.
#[derive(Resource, Clone, ExtractResource)]
pub struct Globals(pub [[f32; 4]; GLOBALS_WIDTH]);

pub fn pack_globals(lighting: Res<Lighting>, mut globals: ResMut<Globals>) {
    globals.0 = lighting.pack();
}

/// The Bevy directional light that renders three's shadow map (its colour
/// and brightness are unused: the shaders read the sun from the globals).
#[derive(Component)]
pub struct ThreeSun;

pub fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 0.0,
            shadow_maps_enabled: false,
            ..default()
        },
        bevy::light::CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 0.1,
            maximum_distance: 1.0e6,
            first_cascade_far_bound: 1.0e6,
            overlap_proportion: 0.0,
        }
        .build(),
        Transform::from_xyz(0.0, 1.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
        ThreeSun,
    ));
}

/// Turns Bevy's shadow pass on or off with three's, and points the light.
pub fn sync_sun(
    lighting: Res<Lighting>,
    mut q: Query<(&mut DirectionalLight, &mut Transform), With<ThreeSun>>,
) {
    let Ok((mut light, mut t)) = q.single_mut() else {
        return;
    };
    let on = lighting.shadows && lighting.sun.is_some_and(|s| s.shadow.is_some());
    if light.shadow_maps_enabled != on {
        light.shadow_maps_enabled = on;
    }
    if let Some(sun) = &lighting.sun {
        let m = sun.shadow_camera().as_mat4();
        let next = Transform::from_matrix(m);
        if *t != next {
            *t = next;
        }
    }
}

/// Replaces the cascade Bevy fitted to the camera with three's fixed box
/// (`render::lighting`'s `Sun::cascade`), so the map covers what three's
/// does. Runs between Bevy's cascade build and its light frusta.
pub fn override_cascades(lighting: Res<Lighting>, mut q: Query<&mut Cascades, With<ThreeSun>>) {
    let Some(sun) = &lighting.sun else { return };
    let Some(s) = &sun.shadow else { return };
    let c = sun.cascade(s);
    for mut cascades in &mut q {
        for v in cascades.cascades.values_mut() {
            *v = vec![c.clone()];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_matrix_and_cascade_agree() {
        let sun = Sun {
            color: [1.0; 3],
            intensity: 3.0,
            position: DVec3::new(4.0, 6.0, 3.0),
            target: DVec3::ZERO,
            shadow: Some(ShadowParams {
                left: -5.0,
                right: 5.0,
                top: 5.0,
                bottom: -5.0,
                near: 0.5,
                far: 30.0,
                bias: -0.0004,
                normal_bias: 0.6,
                map_size: 2048,
            }),
        };
        let s = sun.shadow.unwrap();
        let m = sun.shadow_matrix(&s);
        let c = sun.cascade(&s);
        for p in [
            DVec3::ZERO,
            DVec3::new(1.0, -1.0, 2.0),
            DVec3::new(-3.0, 0.5, 1.0),
        ] {
            let t = m * p.extend(1.0);
            let b = c.clip_from_world.as_dmat4() * p.extend(1.0);
            // x the same; y flipped in texture space; depth reversed.
            assert!((t.x - (b.x * 0.5 + 0.5)).abs() < 1e-5);
            assert!((t.y - (b.y * 0.5 + 0.5)).abs() < 1e-5);
            assert!((t.z - (1.0 - b.z)).abs() < 1e-5, "{} {}", t.z, b.z);
        }
        // The light's own position is at the near plane's centre.
        let t = m * sun.position.extend(1.0);
        assert!((t.x - 0.5).abs() < 1e-9 && (t.y - 0.5).abs() < 1e-9);
    }
}
