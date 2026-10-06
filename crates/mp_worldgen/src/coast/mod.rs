//! `src/world/Coast.js` (roadmap WP 7.1): Hollow Point, the cliff road of
//! Level 2, at blue hour turning to dawn. `coast/kit.js` is [`kit`].
//!
//! The sea is on the left of the road. Along the cliff base an animated surf
//! ribbon follows the real shoreline (found by marching out from the road to
//! where the rendered terrain meets the water); sea stacks and rocks stand in
//! the surf with foam rings of their own, and a natural arch stands off the
//! point. Monterey cypress lean inland away from the wind, coastal scrub and
//! ice plant cover the tops, a lighthouse on its headland sweeps a beam over
//! the water, and fishing boats sit offshore with their running lights on.
//! The start has a gantry and a surfers' coffee pull-out; a vista pull-out
//! looks out from the bluff.
//!
//! The port keeps the JS structure: [`Coast::plan`] cuts the pull-outs and
//! the lighthouse headland into the hillside; [`Coast::build`] runs
//! `findShore`, `buildFoam`, `buildSeaRocks`, `buildArch`,
//! `buildVegetation`, `buildGrass`, `buildCrags`, `buildOutcrops`,
//! `buildLighthouse`, `buildStartArea`, `buildPullout`, `buildVista`,
//! `buildSigns`, `buildPoles`, `buildDelineators` and `buildBoats` in that
//! order into the group `coast`. Its updaters (the lighthouse's beam, the
//! coffee shack's string lights, the reflectors, the boats, the surf's
//! clock) are [`Animator`](crate::world::Animator)s in the JS order. The
//! surf and the beam are `ShaderMaterial`s tagged `Surf` and
//! `LighthouseBeam`; their GLSL is carried verbatim and drawn by the
//! renderer.

// Index loops stay index loops, and the JS signatures stay (D52, D130).
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod kit;

use mp_canvas::Canvas;
use mp_math::{Mulberry32, Noise2D, clamp, fbm, js, kernel, lerp, smoothstep};
use mp_scene::{MaterialKind, NodeType, three};
use mp_track::Track;

use crate::car_model::{self, BuildOpts, Lod};
use crate::color::Color;
use crate::material::{Material, color_value, num};
use crate::object::{GeoId, Layer, MaterialId, NodeId, SceneGraph};
use crate::terrain::Terrain;
use crate::textures::TextureCache;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, ExtrudeOptions, Matrix4, Quaternion, Shape, Vector3,
    box_geometry, capsule_geometry, circle_geometry, cone_geometry, cylinder_geometry,
    extrude_geometry, icosahedron_geometry, merge_geometries, plane_geometry, sphere_geometry,
    torus_geometry,
};
use crate::world::{Animator, Change, Edit, Handle, Scenery, SceneryInfo, UpdateCtx, World};

use kit::{
    FONT, Item, Rect, SignAtlas, SurfaceSampler, bake_static, canvas_tex, colorize, diamond,
    instanced, merged_mesh, panel, placed, prep, rock_geometry, rock_material, round_rect,
};

const PI: f64 = core::f64::consts::PI;

/// Metres of road per instancing chunk.
const CHUNK: f64 = 720.0;

/// The JS writes a random yaw as `6.3` (and the boats' `6.28`), not 2π.
const TURN: f64 = 6.3;
#[allow(clippy::approx_constant)]
const TURN_BOAT: f64 = 6.28;

const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const DOUBLE_SIDE: f64 = three::DOUBLE_SIDE as f64;
const ADDITIVE: f64 = three::ADDITIVE_BLENDING as f64;

// ── Shaders ─────────────────────────────────────────────────────────────

const FOAM_VERT: &str = "\nvarying vec2 vUv;\nvarying float vPh;\n#include <fog_pars_vertex>\nvoid main() {\n  vUv = uv;\n  vec4 p = vec4(position, 1.0);\n  #ifdef USE_INSTANCING\n    p = instanceMatrix * p;\n    vPh = instanceMatrix[3].x * 0.13 + instanceMatrix[3].z * 0.071;\n  #else\n    vPh = 0.0;\n  #endif\n  vec4 mvPosition = modelViewMatrix * p;\n  gl_Position = projectionMatrix * mvPosition;\n  #include <fog_vertex>\n}";

const FOAM_FRAG: &str = "\nuniform float uTime;\nuniform float uBright;\nuniform float uSwell;\nuniform vec3 uColor;\nvarying vec2 vUv;\nvarying float vPh;\n#include <fog_pars_fragment>\nfloat h21(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }\nfloat vn(vec2 p) {\n  vec2 i = floor(p), f = fract(p);\n  vec2 u = f * f * (3.0 - 2.0 * f);\n  return mix(mix(h21(i), h21(i + vec2(1, 0)), u.x), mix(h21(i + vec2(0, 1)), h21(i + vec2(1, 1)), u.x), u.y);\n}\nvoid main() {\n  float v = vUv.y;            // 0 = against the rock, 1 = open water\n  float t = uTime + vPh;\n  float n = vn(vec2(vUv.x * 2.3, v * 3.0 - t * 0.35)) * 0.6 + vn(vec2(vUv.x * 7.0 + t * 0.21, v * 9.0 - t * 0.5)) * 0.4;\n  // Swell lines rolling in toward the rock, breaking into foam.\n  float band = fract(v * 2.1 + t * 0.2);\n  float swell = smoothstep(0.0, 0.07, band) * (1.0 - smoothstep(0.07, 0.33, band));\n  // Surge: the contact foam breathes with the waves.\n  float surge = 0.7 + 0.3 * sin(t * 1.3 + vUv.x * 0.7);\n  float contact = 1.0 - smoothstep(0.08, 0.42 * surge, v + (n - 0.5) * 0.45);\n  float lace = smoothstep(0.35, 0.8, n) * (1.0 - smoothstep(0.3, 0.9, v));\n  float foam = max(contact * (0.45 + 0.7 * n), max(swell * smoothstep(0.4, 0.75, n) * (1.0 - v) * uSwell, lace * 0.45));\n  foam *= smoothstep(0.0, 0.05, v) * (1.0 - smoothstep(0.8, 1.0, v));\n  // Rings (uSwell < 0.5) break into ragged arcs so they don't read as circles.\n  if (uSwell < 0.5) foam *= smoothstep(0.35, 0.65, vn(vec2(vUv.x * 1.3 + vPh * 3.0, t * 0.15))) * 0.75;\n  gl_FragColor = vec4(uColor * uBright, clamp(foam, 0.0, 1.0) * 0.8);\n  #include <fog_fragment>\n}";

const BEAM_VERT: &str = "\nvarying float vAlong;\nvarying float vDepth;\nvarying vec3 vN;\nvarying vec3 vV;\n#include <fog_pars_vertex>\nvoid main() {\n  vAlong = 1.0 - uv.y;        // 0 at the lamp, 1 at the far end\n  vec4 mvPosition = modelViewMatrix * vec4(position, 1.0);\n  vDepth = -mvPosition.z;\n  vN = normalize(normalMatrix * normal);\n  vV = normalize(-mvPosition.xyz);\n  gl_Position = projectionMatrix * mvPosition;\n  #include <fog_vertex>\n}";

const BEAM_FRAG: &str = "\nuniform vec3 uColor;\nuniform float uStrength;\nvarying float vAlong;\nvarying float vDepth;\nvarying vec3 vN;\nvarying vec3 vV;\n#include <fog_pars_fragment>\nvoid main() {\n  vec3 n = vN / max(length(vN), 1e-4);\n  vec3 v = vV / max(length(vV), 1e-4);\n  float core = pow(clamp(abs(dot(n, v)), 1e-4, 1.0), 1.6);\n  float along = clamp(vAlong, 0.0, 1.0);\n  float fall = pow(max(1.0 - along, 1e-4), 2.0) * smoothstep(0.0, 0.04, along);\n  // Fade out as the cone sweeps over the camera: up close it would read as\n  // a flat sheet, not a shaft of light in haze.\n  float near = smoothstep(15.0, 120.0, vDepth);\n  float a = clamp(core * fall * near * uStrength, 0.0, 2.0);\n  vec3 c = uColor * a;\n  #ifdef USE_FOG\n    #ifdef FOG_EXP2\n      float fogF = 1.0 - exp(-fogDensity * fogDensity * vFogDepth * vFogDepth);\n    #else\n      float fogF = smoothstep(fogNear, fogFar, vFogDepth);\n    #endif\n    c *= 1.0 - fogF * 0.8;\n  #endif\n  gl_FragColor = vec4(c, 1.0);\n}";

/// `THREE.UniformsLib.fog` as `UniformsUtils.merge` copies it, first in a
/// `ShaderMaterial`'s uniforms. The renderer overwrites the colour and
/// density from the scene's fog every frame it draws (DECISIONS D532).
pub fn fog_uniforms(m: Material) -> Material {
    m.uniform("fogDensity", num(0.00025))
        .uniform("fogNear", num(1.0))
        .uniform("fogFar", num(2000.0))
        .uniform("fogColor", color_value(Color::new(1.0, 1.0, 1.0)))
}

/// `foamMaterial(swell = 1)`: the animated surf (kind `Surf`).
fn foam_material(swell: f64) -> Material {
    fog_uniforms(Material::shader().kind(MaterialKind::Surf, None))
        .shader_source(FOAM_VERT, FOAM_FRAG)
        .uniform("uTime", num(0.0))
        .uniform("uBright", num(1.0))
        .uniform("uSwell", num(swell))
        .uniform("uColor", color_value(Color::hex(0xeef4f5)))
        .set("transparent", true)
        .set("depthWrite", false)
        .set("fog", true)
        .set("polygonOffset", true)
        .set("polygonOffsetFactor", -2.0)
        .set("polygonOffsetUnits", -2.0)
}

/// Flat ring on the water, uv.y = 0 at the inner (rock) edge → 1 outside
/// (`ringGeometry(inner = 1, outer = 2.1, seg = 28)`).
fn ring_geometry(inner: f64, outer: f64, seg: usize) -> BufferGeometry {
    let mut pos = Vec::new();
    let mut uv = Vec::new();
    let mut idx = Vec::new();
    let segf = seg as f64;
    for i in 0..=seg {
        let fi = i as f64;
        let a = (fi / segf) * PI * 2.0;
        let c = kernel::cos(a);
        let s = kernel::sin(a);
        pos.extend_from_slice(&[c * inner, 0.0, s * inner, c * outer, 0.0, s * outer]);
        uv.extend_from_slice(&[fi / segf * 6.0, 0.0, fi / segf * 6.0, 1.0]);
    }
    for i in 0..seg as u32 {
        let a = i * 2;
        idx.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
    g.set_index(&idx);
    g
}

/// `new THREE.CylinderGeometry(rt, rb, h, radial, 1, open)`.
fn cyl(rt: f64, rb: f64, h: f64, radial: f64, open: bool) -> BufferGeometry {
    cylinder_geometry(rt, rb, h, radial, 1.0, open, 0.0, PI * 2.0)
}

fn boxg(w: f64, h: f64, d: f64) -> BufferGeometry {
    box_geometry(w, h, d, 1.0, 1.0, 1.0)
}

fn merge(parts: &[BufferGeometry]) -> BufferGeometry {
    let refs: Vec<&BufferGeometry> = parts.iter().collect();
    merge_geometries(&refs, false).expect("the parts share their attributes")
}

/// Windswept Monterey cypress, leaning toward local +X (inland).
fn cypress_geometry(variant: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(900 + variant);
    let mut parts = Vec::new();
    let mut trunk = prep(cyl(0.22, 0.38, 3.2, 6.0, true));
    trunk.translate(0.0, 1.6, 0.0);
    parts.push(colorize(trunk, 0x4a3a2c));
    let mut upper = prep(cyl(0.14, 0.24, 3.6, 5.0, true));
    upper.translate(0.0, 1.8, 0.0);
    upper.rotate_z(-0.62);
    upper.translate(0.2, 2.9, 0.0);
    parts.push(colorize(upper, 0x4a3a2c));
    // A second limb forking off the trunk toward the lee side.
    let mut limb = prep(cyl(0.1, 0.17, 3.0, 5.0, true));
    limb.translate(0.0, 1.5, 0.0);
    limb.rotate_z(-0.95);
    limb.rotate_y(0.5);
    limb.translate(0.1, 2.6, 0.2);
    parts.push(colorize(limb, 0x4a3a2c));
    // Foliage in flat, wind-sheared shelves, darker underneath and in the
    // middle of the crown, lighter on the windward crest.
    const GREENS: [u32; 5] = [0x2a3f2c, 0x2c4430, 0x33503a, 0x3a5538, 0x415e3c];
    let blobs = 6 + variant % 2;
    for k in 0..blobs {
        let mut b = prep(icosahedron_geometry(1.0, 0.0));
        let f = k as f64 / (blobs - 1) as f64;
        let sx = lerp(2.4, 1.4, f) * lerp(0.9, 1.25, rng.next_f64());
        let sy = lerp(0.75, 0.5, f) * lerp(0.85, 1.15, rng.next_f64());
        let sz = lerp(2.0, 1.2, f) * lerp(0.85, 1.2, rng.next_f64());
        b.scale(sx, sy, sz);
        b.rotate_y(rng.next_f64() * 3.0);
        let tx = lerp(-0.8, 3.9, f) + (rng.next_f64() - 0.5) * 0.8;
        let ty = lerp(5.0, 6.2, f) + (rng.next_f64() - 0.5) * 0.6 + (k % 2) as f64 * 0.35;
        let tz = (rng.next_f64() - 0.5) * 2.0;
        b.translate(tx, ty, tz);
        let gi = js::min(
            (GREENS.len() - 1) as f64,
            ((k % 2) * 2) as f64 + (rng.next_f64() * 3.0).floor(),
        );
        parts.push(colorize(b, GREENS[gi as usize]));
    }
    // An underlayer of shade below the shelves.
    let mut u = prep(icosahedron_geometry(1.0, 0.0));
    u.scale(2.8, 0.45, 1.8);
    u.translate(1.4, 4.6, 0.0);
    parts.push(colorize(u, 0x1f3024));
    merge(&parts)
}

/// Soft scrub blob: a lumpy low-poly sphere.
fn bush_geometry() -> BufferGeometry {
    let mut b = icosahedron_geometry(1.0, 1.0);
    let n = Noise2D::new(17);
    let p = b.get_attribute_mut("position").expect("position");
    for i in 0..p.count() {
        let (x, y, z) = (p.get_x(i), p.get_y(i), p.get_z(i));
        let r = 1.0 + n.noise(x * 1.7 + z, y * 1.9) * 0.18;
        p.set_xyz(i, x * r, js::max(y * r * 0.62, -0.15), z * r);
    }
    b.delete_attribute("uv");
    b.compute_vertex_normals();
    b.translate(0.0, 0.2, 0.0);
    colorize(b, 0xffffff)
}

/// Monterey pine: a taller, straighter trunk under an irregular dome of
/// dark needle clumps.
fn pine_geometry(variant: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(700 + variant);
    let mut parts = Vec::new();
    let mut trunk = prep(cyl(0.2, 0.42, 8.5, 6.0, true));
    trunk.translate(0.0, 4.25, 0.0);
    parts.push(colorize(trunk, 0x4d3b2d));
    for k in 0..3 {
        let k = k as f64;
        let mut br = prep(cyl(0.06, 0.12, 3.0, 4.0, true));
        br.translate(0.0, 1.5, 0.0);
        br.rotate_z(0.9 + rng.next_f64() * 0.4);
        br.rotate_y(k * 2.1 + rng.next_f64());
        br.translate(0.0, 5.2 + k * 1.1, 0.0);
        parts.push(colorize(br, 0x4d3b2d));
    }
    const GREENS: [u32; 5] = [0x243a2a, 0x2d4631, 0x33503a, 0x283f2c, 0x3a5a3c];
    let n = 8;
    for k in 0..n {
        let mut b = prep(icosahedron_geometry(1.0, 0.0));
        let a = (k as f64 / n as f64) * PI * 2.0 + rng.next_f64() * 0.6;
        let r = if k == 0 {
            0.0
        } else {
            lerp(1.2, 2.9, rng.next_f64())
        };
        let y = if k == 0 {
            11.2
        } else {
            lerp(7.4, 10.6, rng.next_f64())
        };
        let sx = lerp(1.5, 2.3, rng.next_f64());
        let sy = lerp(0.9, 1.3, rng.next_f64());
        let sz = lerp(1.5, 2.3, rng.next_f64());
        b.scale(sx, sy, sz);
        b.rotate_y(rng.next_f64() * 3.0);
        b.translate(kernel::cos(a) * r, y, kernel::sin(a) * r);
        parts.push(colorize(b, GREENS[k % GREENS.len()]));
    }
    merge(&parts)
}

/// Grass tuft: a fan of thin blades, dark at the root and pale at the tip
/// (instance colour tints it from green to straw).
fn tuft_geometry() -> BufferGeometry {
    let mut pos = Vec::new();
    let mut colr = Vec::new();
    let mut rng = Mulberry32::new(41);
    let n = 9;
    for k in 0..n {
        let a = (k as f64 / n as f64) * PI * 2.0 + rng.next_f64() * 0.5;
        let lean = lerp(0.25, 0.6, rng.next_f64());
        let h = lerp(0.55, 1.0, rng.next_f64());
        let w = 0.07;
        let ca = kernel::cos(a);
        let sa = kernel::sin(a);
        let tx = ca * lean * h;
        let tz = sa * lean * h;
        pos.extend_from_slice(&[-sa * w, 0.0, ca * w, sa * w, 0.0, -ca * w, tx, h, tz]);
        colr.extend_from_slice(&[0.35, 0.36, 0.25, 0.35, 0.36, 0.25, 1.0, 1.0, 0.9]);
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("color", BufferAttribute::from_f64(&colr, 3));
    g.compute_vertex_normals();
    // Blades light like the ground under them, not like thin cards.
    let nrm = g.get_attribute_mut("normal").expect("normal");
    for i in 0..nrm.count() {
        nrm.set_xyz(i, 0.0, 1.0, 0.0);
    }
    g
}

/// Sea stack: stacked strata blocks, each stepped back from the one below,
/// with a scrubby cap. Unit height, about unit width; vertex colours carry
/// the cap (the rock itself is tinted per instance).
fn stack_geometry(seed: u32) -> BufferGeometry {
    let mut rng = Mulberry32::new(seed);
    let mut parts = Vec::new();
    let tiers = 4;
    let mut y = 0.0;
    let mut w = 1.0;
    for k in 0..tiers {
        let h = if k == 0 {
            0.36
        } else {
            lerp(0.18, 0.26, rng.next_f64())
        };
        let mut g = rock_geometry(seed * 7 + k, 1.0, 1.0, 0.28);
        let sx = w * lerp(0.95, 1.1, rng.next_f64());
        let sz = w * lerp(0.8, 1.0, rng.next_f64());
        g.scale(sx, h * 0.62, sz);
        g.rotate_y(rng.next_f64() * 6.3);
        let tx = (rng.next_f64() - 0.5) * 0.15;
        let tz = (rng.next_f64() - 0.5) * 0.15;
        g.translate(tx, y + h * 0.3, tz);
        parts.push(colorize(g, 0xffffff));
        y += h * 0.8;
        w *= lerp(0.72, 0.88, rng.next_f64());
    }
    let mut cap = prep(icosahedron_geometry(1.0, 1.0));
    cap.scale(w * 0.95, 0.07, w * 0.85);
    cap.translate(0.0, y + 0.02, 0.0);
    parts.push(colorize(cap, 0x6a7a4a));
    let mut g = merge(&parts);
    g.scale(1.0, 1.0 / (y + 0.05), 1.0);
    g
}

/// Crag: a blocky slab of cliff rock whose faces step in horizontal
/// ledges, set against steep ground so the terrain reads as sculpted cliff.
fn crag_geometry(seed: u32) -> BufferGeometry {
    let mut g = icosahedron_geometry(1.0, 1.0);
    let n = Noise2D::new(seed);
    let pos = g.get_attribute_mut("position").expect("position");
    for i in 0..pos.count() {
        let (x, y, z) = (pos.get_x(i), pos.get_y(i), pos.get_z(i));
        // Ledges: pull the radius in and out in bands of height.
        let band = ((y + 1.0) * 2.5).floor();
        let r = 1.0 + n.noise(x * 1.3 + band * 3.1, z * 1.3) * 0.28 + (band % 2.0) * 0.08;
        pos.set_xyz(
            i,
            x * r,
            js::max(-0.8, y) * (0.9 + (band % 2.0) * 0.1),
            z * r * 0.75,
        );
    }
    g.delete_attribute("uv");
    g.compute_vertex_normals();
    g
}

// ── The module ──────────────────────────────────────────────────────────

/// `{ x, z, r }`: no scenery within r of (x, z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exclusion {
    pub x: f64,
    pub z: f64,
    pub r: f64,
}

/// A pull-out cut into the hillside by `plan()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    pub s: f64,
    pub side: f64,
    pub x: f64,
    pub z: f64,
    pub y: f64,
}

/// The lighthouse's headland pad (`ground`: the rendered height there, once
/// built).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighthouse {
    pub s: f64,
    pub x: f64,
    pub z: f64,
    pub y: f64,
    pub ground: Option<f64>,
}

/// The sea arch, once built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arch {
    pub x: f64,
    pub z: f64,
    pub yaw: f64,
}

/// A point of the shoreline: where the march from the road met the water,
/// with its seaward normal and (once split into runs) its tangent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShorePoint {
    pub s: f64,
    pub d: f64,
    pub x: f64,
    pub z: f64,
    pub nx: f64,
    pub nz: f64,
    pub tx: f64,
    pub tz: f64,
}

/// The Hollow Point scenery module.
pub struct Coast {
    pub zone: usize,
    pub label: &'static str,
    pub exclusions: Vec<Exclusion>,
    /// `zEnd`: where the zone ends (`zoneStart[zone + 1] ?? length`).
    pub z_end: f64,
    pub pullout: Option<Spot>,
    pub lighthouse: Option<Lighthouse>,
    pub vista: Option<Spot>,
    pub arch: Option<Arch>,
    /// The group `coast`, once built.
    pub group: Option<NodeId>,
    pub stack_count: usize,
    pub tree_count: usize,
    pub bush_count: usize,
    pub grass_count: usize,
    pub crag_count: usize,
}

impl Coast {
    /// `new Coast({ zone })`.
    pub fn new(info: &SceneryInfo) -> Coast {
        Coast {
            zone: info.zone,
            label: "Carving the coast",
            exclusions: Vec::new(),
            z_end: 0.0,
            pullout: None,
            lighthouse: None,
            vista: None,
            arch: None,
            group: None,
            stack_count: 0,
            tree_count: 0,
            bush_count: 0,
            grass_count: 0,
            crag_count: 0,
        }
    }
}

/// A typed-array read as the JS makes it (past the end is `undefined`).
fn at(a: &[f32], i: usize) -> f64 {
    a.get(i).map_or(f64::NAN, |&v| f64::from(v))
}

impl Scenery for Coast {
    fn name(&self) -> &str {
        "Coast"
    }

    fn label(&self) -> Option<&str> {
        Some(self.label)
    }

    /// Pull-outs and the lighthouse headland.
    fn plan(&mut self, world: &mut World) -> Result<(), String> {
        let t = world.track.as_ref().ok_or("the route is surveyed first")?;
        let terrain = world.terrain.as_mut().ok_or("no terrain")?;
        self.z_end = t
            .zone_start
            .get(self.zone + 1)
            .map_or(t.length, |&s| s as f64);

        // Surfers' coffee pull-out beside the start, cut into the hillside.
        let s0 = 30.0;
        let wr = at(&t.wall_r, t.idx(s0));
        let pp = t.point_at(s0, wr + 14.0);
        let pullout = Spot {
            s: s0,
            side: 1.0,
            x: pp.x,
            z: pp.z,
            y: t.surface_y(s0, wr) - 0.12,
        };
        terrain.add_flatten(pp.x, pp.z, 13.0, 16.0, Some(pullout.y));
        self.pullout = Some(pullout);
        self.exclusions.push(Exclusion {
            x: pp.x,
            z: pp.z,
            r: 20.0,
        });

        // Lighthouse on a headland pad below road level, seaward of the road.
        if let Some(lh) = t.tag("lighthouse").first() {
            let s = js::round((lh.s0 + lh.s1) / 2.0) + 20.0;
            let wl = at(&t.wall_l, t.idx(s));
            let p = t.point_at(s, -(wl + 58.0));
            let l = Lighthouse {
                s,
                x: p.x,
                z: p.z,
                y: at(&t.py, t.idx(s)) - 8.0,
                ground: None,
            };
            terrain.add_flatten(p.x, p.z, 24.0, 34.0, Some(l.y));
            // A neck of land joining the headland to the cliff the road runs on.
            for (f, dy) in [(0.4, -4.5), (0.62, -6.5)] {
                let q = t.point_at(s + (f - 0.5) * 8.0, -(wl + 58.0 * f));
                terrain.add_flatten(q.x, q.z, 9.0, 16.0, Some(at(&t.py, t.idx(s)) + dy));
            }
            self.lighthouse = Some(l);
            self.exclusions.push(Exclusion {
                x: p.x,
                z: p.z,
                r: 27.0,
            });
        }

        // Vista pull-out on the bluff, seaward.
        if let Some(bl) = t.tag("bluff").first() {
            let s = js::round((bl.s0 + bl.s1) / 2.0);
            let wl = at(&t.wall_l, t.idx(s));
            let p = t.point_at(s, -(wl + 11.0));
            let v = Spot {
                s,
                side: -1.0,
                x: p.x,
                z: p.z,
                y: t.surface_y(s, -wl) - 0.15,
            };
            terrain.add_flatten(p.x, p.z, 10.0, 12.0, Some(v.y));
            self.vista = Some(v);
            self.exclusions.push(Exclusion {
                x: p.x,
                z: p.z,
                r: 14.0,
            });
        }
        Ok(())
    }

    fn build(&mut self, world: &mut World) -> Result<(), String> {
        let World {
            level,
            track,
            terrain,
            graph,
            textures,
            road,
            root,
            animators,
            ..
        } = world;
        let track = track.as_ref().ok_or("the route is surveyed first")?;
        let terrain = terrain.as_ref().ok_or("no terrain")?;
        let road = road.as_ref().ok_or("the road is built first")?;
        let segments: Vec<(f64, f64)> = level
            .segments()
            .map(|s| s.iter().map(|g| (g.len, g.turn)).collect())
            .unwrap_or_default();
        let sea_y = level.sea_y.unwrap_or(0.0);
        let group = graph.group("coast");
        let n_chunks = ((self.z_end + 200.0) / CHUNK).ceil() as usize + 1;
        let foam_mat = graph.add_material(foam_material(1.0));
        let ring_mat = graph.add_material(foam_material(0.15));
        let rock_mat = {
            let m = rock_material(graph, textures);
            graph.add_material(m)
        };
        let arch_mat = {
            let m = rock_material(graph, textures).set("color", 0xc4a98a);
            graph.add_material(m)
        };
        let mut b = Build {
            t: track,
            terrain,
            side_l: &road.side_l,
            side_r: &road.side_r,
            surf: SurfaceSampler::new(terrain),
            sea_y,
            z_end: self.z_end,
            zone: self.zone,
            n_chunks,
            exclusions: &self.exclusions,
            graph,
            textures,
            group,
            animators: Vec::new(),
            foam_mat,
            ring_mat,
            rock_mat,
            arch_mat,
            shore_runs: Vec::new(),
        };

        b.find_shore();
        b.build_foam();
        self.stack_count = b.build_sea_rocks();
        self.arch = b.build_arch();
        let (trees, bushes) = b.build_vegetation();
        self.tree_count = trees;
        self.bush_count = bushes;
        self.grass_count = b.build_grass();
        self.crag_count = b.build_crags();
        b.build_outcrops();
        if let Some(l) = &mut self.lighthouse {
            b.build_lighthouse(l);
        }
        b.build_start_area();
        if let Some(d) = self.pullout {
            b.build_pullout(&d);
        }
        if let Some(v) = self.vista {
            b.build_vista(&v);
        }
        b.build_signs(&segments, self.vista.as_ref(), self.lighthouse.as_ref());
        b.build_poles();
        b.build_delineators();
        b.build_boats();

        // Surf and foam brighten with the dawn; everything animates off one
        // clock.
        let mut time = 0.0;
        b.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                time += u.dt;
                for m in [foam_mat, ring_mat] {
                    out.push(Edit {
                        target: Handle::Material(m),
                        change: Change::Number {
                            prop: "uTime",
                            value: time,
                        },
                    });
                    out.push(Edit {
                        target: Handle::Material(m),
                        change: Change::Number {
                            prop: "uBright",
                            value: lerp(1.0, 0.32, u.night),
                        },
                    });
                }
            }));

        let new_animators = std::mem::take(&mut b.animators);
        graph.add(*root, group);
        animators.extend(new_animators);
        self.group = Some(group);
        Ok(())
    }
}

/// The scenery being built: what `this` holds during `build()`.
struct Build<'a> {
    t: &'a Track,
    terrain: &'a Terrain,
    /// `track.sideL` / `sideR` (the road's classification).
    side_l: &'a [u8],
    side_r: &'a [u8],
    surf: SurfaceSampler<'a>,
    sea_y: f64,
    z_end: f64,
    zone: usize,
    n_chunks: usize,
    exclusions: &'a [Exclusion],
    graph: &'a mut SceneGraph,
    textures: &'a mut TextureCache,
    group: NodeId,
    animators: Vec<Box<dyn Animator>>,
    foam_mat: MaterialId,
    ring_mat: MaterialId,
    rock_mat: MaterialId,
    arch_mat: MaterialId,
    shore_runs: Vec<Vec<ShorePoint>>,
}

impl Build<'_> {
    fn add(&mut self, n: NodeId) {
        self.graph.add(self.group, n);
    }

    fn mat(&mut self, m: Material) -> MaterialId {
        self.graph.add_material(m)
    }

    fn geo(&mut self, g: BufferGeometry) -> GeoId {
        self.graph.add_geometry(g)
    }

    fn add_instanced(
        &mut self,
        geo: GeoId,
        mat: MaterialId,
        items: &[Item],
        cast: bool,
        receive: bool,
    ) -> NodeId {
        let n = instanced(self.graph, geo, mat, items, cast, receive);
        self.add(n);
        n
    }

    fn add_merged(
        &mut self,
        geos: Vec<BufferGeometry>,
        mat: MaterialId,
        cast: bool,
        receive: bool,
    ) {
        if let Some(n) = merged_mesh(self.graph, geos, mat, cast, receive) {
            self.add(n);
        }
    }

    /// `excluded(x, z, pad = 0)`.
    fn excluded(&self, x: f64, z: f64, pad: f64) -> bool {
        self.exclusions.iter().any(|e| {
            kernel::pow(x - e.x, 2.0) + kernel::pow(z - e.z, 2.0) < kernel::pow(e.r + pad, 2.0)
        })
    }

    fn clear_of_road(&self, x: f64, z: f64, radius: f64) -> bool {
        let info = self.terrain.road_info(x, z);
        if !info.near {
            return info.d > radius + 12.0;
        }
        let i = self.t.idx(info.s);
        let wall = js::max(at(&self.t.wall_l, i), at(&self.t.wall_r, i));
        info.d - radius >= wall + 0.6
    }

    /// `inZone(x, need = 0.5)`.
    fn in_zone(&self, x: f64, need: f64) -> bool {
        self.terrain.zone_weights(x).as_slice()[self.zone] >= need
    }

    fn chunk_of(&self, s: f64) -> usize {
        clamp((s / CHUNK).floor(), 0.0, (self.n_chunks - 1) as f64) as usize
    }

    // ── Shoreline: march seaward from the road until the rendered ground
    // dips under the water. ──────────────────────────────────────────────

    fn find_shore(&mut self) {
        let t = self.t;
        let sea = self.sea_y;
        let mut shore: Vec<Option<ShorePoint>> = Vec::new();
        let mut s = 0.0;
        while s < self.z_end + 180.0 {
            let sf = t.frame(s);
            let wl = sf.wall_l;
            let mut prev_d = wl + 3.0;
            let mut prev_h = self
                .surf
                .sample(sf.x - sf.rx * prev_d, sf.z - sf.rz * prev_d)
                .h;
            let mut hit = None;
            let mut d = wl + 7.0;
            while d < 520.0 {
                let x = sf.x - sf.rx * d;
                let z = sf.z - sf.rz * d;
                let h = self.surf.sample(x, z).h;
                if h < sea + 0.2 {
                    // Refine between prevD and d.
                    let mut a = prev_d;
                    let mut bb = d;
                    let mut _ha = prev_h;
                    for _ in 0..4 {
                        let m = (a + bb) / 2.0;
                        let hm = self.surf.sample(sf.x - sf.rx * m, sf.z - sf.rz * m).h;
                        if hm < sea + 0.2 {
                            bb = m;
                        } else {
                            a = m;
                            _ha = hm;
                        }
                    }
                    hit = Some((a + bb) / 2.0);
                    break;
                }
                prev_d = d;
                prev_h = h;
                d += 6.0;
            }
            // A ray hitting water behind a sea stack gives a far-off point;
            // keep it, the segment split below handles jumps.
            shore.push(hit.map(|hit| ShorePoint {
                s,
                d: hit,
                x: sf.x - sf.rx * hit,
                z: sf.z - sf.rz * hit,
                nx: -sf.rx,
                nz: -sf.rz,
                tx: 0.0,
                tz: 0.0,
            }));
            s += 5.0;
        }
        // Split into continuous runs.
        let mut runs: Vec<Vec<ShorePoint>> = Vec::new();
        let mut run: Vec<ShorePoint> = Vec::new();
        for p in shore {
            let jump = match (p, run.last()) {
                (Some(p), Some(last)) => kernel::hypot(p.x - last.x, p.z - last.z) > 22.0,
                _ => false,
            };
            if p.is_none() || jump {
                if run.len() > 3 {
                    runs.push(std::mem::take(&mut run));
                }
                run = p.into_iter().collect();
            } else if let Some(p) = p {
                run.push(p);
            }
        }
        if run.len() > 3 {
            runs.push(run);
        }
        // Recompute seaward normals from the shoreline itself.
        for r in &mut runs {
            let n = r.len();
            for i in 0..n {
                let a = r[i.saturating_sub(2)];
                let b = r[(i + 2).min(n - 1)];
                let mut tx = b.x - a.x;
                let mut tz = b.z - a.z;
                let l = js::or(kernel::hypot(tx, tz), 1.0);
                tx /= l;
                tz /= l;
                let mut nx = -tz;
                let mut nz = tx;
                if nx * r[i].nx + nz * r[i].nz < 0.0 {
                    nx = -nx;
                    nz = -nz;
                }
                r[i].nx = nx;
                r[i].nz = nz;
                r[i].tx = tx;
                r[i].tz = tz;
            }
        }
        self.shore_runs = runs;
    }

    fn build_foam(&mut self) {
        let mut pos = Vec::new();
        let mut uv = Vec::new();
        let mut idx: Vec<u32> = Vec::new();
        let y = self.sea_y + 0.1;
        for r in &self.shore_runs {
            let base = (pos.len() / 3) as u32;
            let mut u = 0.0;
            for (i, p) in r.iter().enumerate() {
                if i > 0 {
                    u += kernel::hypot(p.x - r[i - 1].x, p.z - r[i - 1].z);
                }
                let inner = 3.5;
                let outer = 13.0;
                pos.extend_from_slice(&[
                    p.x - p.nx * inner,
                    y,
                    p.z - p.nz * inner,
                    p.x + p.nx * outer,
                    y,
                    p.z + p.nz * outer,
                ]);
                uv.extend_from_slice(&[u / 9.0, 0.0, u / 9.0, 1.0]);
            }
            for i in 0..r.len() as u32 - 1 {
                let a = base + i * 2;
                idx.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
            }
        }
        if idx.is_empty() {
            return;
        }
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
        g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
        g.set_index(&idx);
        g.compute_bounding_sphere();
        let g = self.geo(g);
        let m = self.graph.mesh(g, self.foam_mat);
        {
            let o = self.graph.get_mut(m);
            o.render_order = 2.0;
            o.frustum_culled = true;
        }
        self.add(m);
    }

    // ── Sea stacks and surf rocks, each with a foam ring ────────────────

    fn build_sea_rocks(&mut self) -> usize {
        let mut rng = Mulberry32::new(3131);
        let terrain = self.terrain;
        let sea = self.sea_y;
        let mut stacks: Vec<Item> = Vec::new();
        let mut small: Vec<Item> = Vec::new();
        let mut rings: Vec<Item> = Vec::new();
        const PALETTE: [u32; 5] = [0xc9ad8c, 0xb89c7e, 0xa89484, 0xd2b99a, 0x9e8f82];
        let pick = |rng: &mut Mulberry32| {
            PALETTE[(rng.next_f64() * PALETTE.len() as f64).floor() as usize]
        };
        let mut last_stack = -1e9;
        let runs = std::mem::take(&mut self.shore_runs);
        for r in &runs {
            for p in r {
                // Big stacks, spaced out along the coast.
                if p.s - last_stack > 55.0 && rng.next_f64() < 0.22 {
                    let off = lerp(28.0, 170.0, kernel::pow(rng.next_f64(), 1.4));
                    let x = p.x + p.nx * off + p.tx * (rng.next_f64() - 0.5) * 30.0;
                    let z = p.z + p.nz * off + p.tz * (rng.next_f64() - 0.5) * 30.0;
                    let bed = terrain.height_at(x, z);
                    if bed < sea - 3.0 {
                        let w = lerp(7.0, 16.0, rng.next_f64());
                        let hgt = lerp(10.0, 36.0, kernel::pow(rng.next_f64(), 1.3));
                        let sz = w * lerp(0.7, 1.2, rng.next_f64());
                        let ry = rng.next_f64() * TURN;
                        let col = pick(&mut rng);
                        let bb = lerp(0.8, 1.05, rng.next_f64());
                        stacks.push(Item {
                            ry,
                            col: Some(col),
                            b: Some(bb),
                            ..Item::at(x, bed, z, w, hgt - bed + 1.0, sz)
                        });
                        let ry = rng.next_f64() * TURN;
                        rings.push(Item {
                            ry,
                            ..Item::at(x, sea + 0.12, z, w * 0.92, 1.0, w * 0.92)
                        });
                        last_stack = p.s;
                    }
                }
                // Surf rocks strewn along the waterline.
                let n = if rng.next_f64() < 0.6 {
                    1.0 + (rng.next_f64() * 3.0).floor()
                } else {
                    0.0
                };
                let mut k = 0.0;
                while k < n {
                    k += 1.0;
                    let off = lerp(-1.0, 26.0, kernel::pow(rng.next_f64(), 1.6));
                    let x = p.x + p.nx * off + p.tx * (rng.next_f64() - 0.5) * 8.0;
                    let z = p.z + p.nz * off + p.tz * (rng.next_f64() - 0.5) * 8.0;
                    let bed = self.surf.sample(x, z).h;
                    let size = lerp(1.2, 4.5, kernel::pow(rng.next_f64(), 1.7));
                    if bed > sea + 1.5 {
                        continue;
                    }
                    let sx = size * lerp(0.8, 1.4, rng.next_f64());
                    let sy = size * lerp(0.7, 1.3, rng.next_f64());
                    let ry = rng.next_f64() * TURN;
                    let col = pick(&mut rng);
                    let bb = lerp(0.7, 1.0, rng.next_f64());
                    small.push(Item {
                        ry,
                        col: Some(col),
                        b: Some(bb),
                        ..Item::at(x, js::min(bed, sea - 0.4), z, sx, sy, size)
                    });
                    if size > 2.3 && rng.next_f64() < 0.6 {
                        let ry = rng.next_f64() * TURN;
                        rings.push(Item {
                            ry,
                            ..Item::at(x, sea + 0.12, z, size * 0.95, 1.0, size * 0.95)
                        });
                    }
                }
            }
        }
        self.shore_runs = runs;
        // Two stack shapes; a vertex-coloured copy of the rock material
        // shows their green caps.
        if !stacks.is_empty() {
            let m = rock_material(self.graph, self.textures)
                .set("vertexColors", true)
                .program_key("coast-rock-vc");
            let stack_mat = self.mat(m);
            let half = stacks.len().div_ceil(2);
            let g71 = self.geo(stack_geometry(71));
            self.add_instanced(g71, stack_mat, &stacks[..half], true, true);
            if stacks.len() > half {
                let g73 = self.geo(stack_geometry(73));
                self.add_instanced(g73, stack_mat, &stacks[half..], true, true);
            }
        }
        if !small.is_empty() {
            let g = self.geo(rock_geometry(72, 0.0, 0.8, 0.4));
            self.add_instanced(g, self.rock_mat, &small, false, true);
        }
        if !rings.is_empty() {
            let g = self.geo(ring_geometry(0.85, 2.4, 28));
            let im = self.add_instanced(g, self.ring_mat, &rings, false, false);
            self.graph.get_mut(im).render_order = 2.0;
        }
        stacks.len()
    }

    // ── Natural sea arch off the point ──────────────────────────────────

    fn build_arch(&mut self) -> Option<Arch> {
        let tag = self.t.tag("arch").first().map(|t| (t.s0, t.s1))?;
        if self.shore_runs.is_empty() {
            return None;
        }
        let s_mid = (tag.0 + tag.1) / 2.0;
        let mut best: Option<ShorePoint> = None;
        for r in &self.shore_runs {
            for p in r {
                if best.is_none_or(|b| (p.s - s_mid).abs() < (b.s - s_mid).abs()) {
                    best = Some(*p);
                }
            }
        }
        let best = best?;
        // Walk out until the water is deep enough to stand an arch in.
        let mut off = 55.0;
        let mut x = 0.0;
        let mut z = 0.0;
        while off < 260.0 {
            x = best.x + best.nx * off;
            z = best.z + best.nz * off;
            if self.terrain.height_at(x, z) < self.sea_y - 6.0 {
                break;
            }
            off += 10.0;
        }
        let n = Noise2D::new(55);
        let r = 17.0;
        let tube = 6.2;
        let mut torus = torus_geometry(r, tube, 9.0, 30.0, PI);
        torus.scale(1.0, 1.25, 1.2);
        let mut g = torus.to_non_indexed();
        {
            let p = g.get_attribute_mut("position").expect("position");
            for i in 0..p.count() {
                let (px, py, pz) = (p.get_x(i), p.get_y(i), p.get_z(i));
                let d = n.noise(px * 0.09 + pz * 0.05, py * 0.1) * 2.2
                    + n.noise(px * 0.3, py * 0.3 + pz * 0.2) * 0.8;
                let l = js::or(kernel::hypot(px, py), 1.0);
                p.set_xyz(
                    i,
                    px + (px / l) * d,
                    py + (py / l) * d + if py > 20.0 { -1.2 } else { 0.0 },
                    pz * (1.0 + d * 0.05),
                );
            }
        }
        g.delete_attribute("uv");
        g.compute_vertex_normals();
        // Legs down to the sea bed, and a rubble apron.
        let mut parts = vec![g];
        for side in [-1i32, 1] {
            let sf = f64::from(side);
            let mut leg = rock_geometry((80 + side) as u32, 1.0, 1.0, 0.3);
            leg.scale(tube * 1.35, 16.0, tube * 1.5);
            leg.translate(sf * r, -10.0, 0.0);
            parts.push(leg);
            for k in 0..3i32 {
                let kf = f64::from(k);
                let mut rb = rock_geometry((90 + k + side * 7) as u32, 0.0, 0.8, 0.45);
                rb.scale(3.0 + kf, 2.5 + kf * 0.6, 3.5 + kf);
                rb.translate(sf * (r + 7.0 + kf * 3.0), -1.2, (kf - 1.0) * 6.0);
                parts.push(rb);
            }
        }
        for l in &mut parts[1..] {
            if l.has_attribute("uv") {
                l.delete_attribute("uv");
            }
        }
        let merged = merge(&parts);
        let yaw = kernel::atan2(-best.tz, best.tx);
        let g = self.geo(placed(&merged, x, self.sea_y - 1.5, z, yaw));
        let mesh = self.graph.mesh(g, self.arch_mat);
        {
            let o = self.graph.get_mut(mesh);
            o.cast_shadow = true;
            o.receive_shadow = true;
            o.matrix_auto_update = false;
        }
        self.add(mesh);
        // Foam where the legs meet the water.
        let mut rings = Vec::new();
        for side in [-1.0, 1.0] {
            rings.push(Item {
                ry: side,
                ..Item::at(
                    x + kernel::cos(yaw) * side * r,
                    self.sea_y + 0.12,
                    z - kernel::sin(yaw) * side * r,
                    tube * 1.6,
                    1.0,
                    tube * 1.6,
                )
            });
        }
        let rg = self.geo(ring_geometry(0.85, 2.4, 28));
        let im = self.add_instanced(rg, self.ring_mat, &rings, false, false);
        self.graph.get_mut(im).render_order = 2.0;
        Some(Arch { x, z, yaw })
    }

    // ── Cypress, scrub and ice plant ────────────────────────────────────

    fn build_vegetation(&mut self) -> (usize, usize) {
        let t = self.t;
        let terrain = self.terrain;
        let mut rng = Mulberry32::new(2024);
        let n2 = &terrain.noise2;
        let mut cypress: [Vec<Vec<Item>>; 2] = [
            vec![Vec::new(); self.n_chunks],
            vec![Vec::new(); self.n_chunks],
        ];
        let mut bushes: Vec<Vec<Item>> = vec![Vec::new(); self.n_chunks];
        let b = t.bounds;
        const BUSH_COLS: [u32; 7] = [
            0x5f6e4c, 0x6b7458, 0x4d5a36, 0x6e5a3c, 0x55663e, 0x7a8068, 0x4a5638,
        ];
        const ICE_COLS: [u32; 5] = [0x5f7d3a, 0xa8587c, 0x9c6a86, 0x7c8f3c, 0x6d8a40];
        let pick =
            |rng: &mut Mulberry32, a: &[u32]| a[(rng.next_f64() * a.len() as f64).floor() as usize];
        let x_end = terrain.x1() + 250.0;
        let mut x = b.min_x - 380.0;
        while x < x_end {
            let mut z = b.min_z - 380.0;
            while z < b.max_z + 380.0 {
                let cur_z = z;
                z += 7.0;
                let px = x + (rng.next_f64() - 0.5) * 6.5;
                let pz = cur_z + (rng.next_f64() - 0.5) * 6.5;
                let r = rng.next_f64();
                if !self.in_zone(px, 0.35) {
                    continue;
                }
                let f = terrain.far(px, pz);
                if f.d > 360.0 || f.s > self.z_end + 120.0 {
                    continue;
                }
                let sea_side = f.side < 0.0;
                let sv = self.surf.sample(px, pz);
                if sv.h < self.sea_y + 3.0 {
                    continue;
                }
                let road_y = at(&t.py, t.idx(f.s));
                // Sea side: only the cliff tops, not the faces.
                if sea_side && (sv.h < road_y - 25.0 || sv.slope > 0.45) {
                    continue;
                }
                if sv.slope > 1.4 {
                    continue;
                }
                if self.excluded(px, pz, 2.0) {
                    continue;
                }
                let clump = smoothstep(-0.15, 0.35, fbm(n2, px / 230.0 + 3.1, pz / 230.0, 3));
                // Cypress: groves on the land side and scattered on the
                // headlands.
                let pc = if sv.slope > 0.75 {
                    0.0
                } else {
                    (if sea_side { 0.03 } else { 0.16 })
                        * (0.25 + clump)
                        * (1.0 - smoothstep(0.4, 0.75, sv.slope))
                };
                if r < pc {
                    if !self.clear_of_road(px, pz, 4.5) {
                        continue;
                    }
                    // Lean inland: local +X along the road's right vector
                    // (away from sea).
                    let i = t.idx(f.s);
                    // Pines stand in the sheltered groves inland; cypress
                    // take the wind.
                    let v = if !sea_side && f.d > 25.0 && rng.next_f64() < 0.45 {
                        1
                    } else {
                        0
                    };
                    let yaw = if v == 1 {
                        rng.next_f64() * TURN
                    } else {
                        kernel::atan2(-at(&t.rz, i), at(&t.rx, i)) + (rng.next_f64() - 0.5) * 0.6
                    };
                    let sc = if v == 1 {
                        lerp(0.85, 1.35, rng.next_f64())
                    } else {
                        lerp(0.8, 1.6, kernel::pow(rng.next_f64(), 1.2))
                    };
                    let sy = sc * lerp(0.85, 1.15, rng.next_f64());
                    let bb = lerp(0.75, 1.1, rng.next_f64());
                    let c = self.chunk_of(f.s);
                    cypress[v][c].push(Item {
                        ry: yaw,
                        col: Some(0xffffff),
                        b: Some(bb),
                        ..Item::at(px, sv.h - 0.3, pz, sc, sy, sc)
                    });
                    continue;
                }
                // Scrub everywhere it can cling; ice plant mats nearer the
                // edges.
                let pb = (0.14 + clump * 0.14)
                    * (if sea_side { 1.1 } else { 0.8 })
                    * (1.0 - smoothstep(0.7, 1.4, sv.slope) * 0.7)
                    * (if f.d < 70.0 { 1.5 } else { 1.0 });
                if r < pc + pb {
                    if !self.clear_of_road(px, pz, 1.5) {
                        continue;
                    }
                    let ice = sea_side && rng.next_f64() < 0.55;
                    let col = if ice {
                        pick(&mut rng, &ICE_COLS)
                    } else {
                        pick(&mut rng, &BUSH_COLS)
                    };
                    let sc = if ice {
                        lerp(1.2, 2.6, rng.next_f64())
                    } else {
                        lerp(0.6, 1.8, kernel::pow(rng.next_f64(), 1.3))
                    };
                    let steep = smoothstep(0.35, 0.8, sv.slope);
                    let sx = sc * lerp(0.8, 1.3, rng.next_f64());
                    let sy = if ice {
                        sc * 0.4
                    } else {
                        sc * lerp(lerp(0.6, 1.0, rng.next_f64()), 1.25, steep)
                    };
                    let ry = rng.next_f64() * TURN;
                    let bb = lerp(0.8, 1.05, rng.next_f64());
                    let c = self.chunk_of(f.s);
                    bushes[c].push(Item {
                        ry,
                        col: Some(col),
                        b: Some(bb),
                        ..Item::at(
                            px,
                            sv.h - (if ice { 0.3 } else { 0.12 }) - steep * sc * 0.45,
                            pz,
                            sx,
                            sy,
                            sc,
                        )
                    });
                }
            }
            x += 7.0;
        }
        let tree_mat = self.mat(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.9)
                .set("flatShading", true),
        );
        let geos = [self.geo(cypress_geometry(1)), self.geo(pine_geometry(1))];
        let mut trees = 0;
        for (v, chunks) in cypress.iter().enumerate() {
            for items in chunks {
                if items.is_empty() {
                    continue;
                }
                trees += items.len();
                self.add_instanced(geos[v], tree_mat, items, true, true);
            }
        }
        // Roadside scrub: a denser band along the landward verge and cut
        // slopes.
        let mut s = 10.0;
        while s < self.z_end + 60.0 {
            let cur = s;
            s += 2.2;
            let _i = t.idx(cur);
            if rng.next_f64() > 0.8 {
                continue;
            }
            let f = t.frame(cur + (rng.next_f64() - 0.5) * 2.0);
            let lat = f.wall_r + 1.2 + kernel::pow(rng.next_f64(), 1.6) * 34.0;
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let sv = self.surf.sample(x, z);
            if sv.slope > 1.6
                || sv.h > f.y + 22.0
                || self.excluded(x, z, 1.0)
                || !self.clear_of_road(x, z, 1.2)
            {
                continue;
            }
            let sc = lerp(0.5, 1.4, kernel::pow(rng.next_f64(), 1.4));
            // On a slope a flat bush juts out like a shelf: keep it round and
            // sunk in.
            let steep = smoothstep(0.35, 0.8, sv.slope);
            let sx = sc * lerp(0.8, 1.2, rng.next_f64());
            let sy = sc * lerp(lerp(0.55, 0.95, rng.next_f64()), 1.25, steep);
            let ry = rng.next_f64() * TURN;
            let col = pick(&mut rng, &BUSH_COLS);
            let bb = lerp(0.8, 1.05, rng.next_f64());
            let c = self.chunk_of(cur);
            bushes[c].push(Item {
                ry,
                col: Some(col),
                b: Some(bb),
                ..Item::at(x, sv.h - 0.15 - steep * sc * 0.45, z, sx, sy, sc)
            });
            // Ice plant spilling over the seaward verge beyond the guardrail.
            if rng.next_f64() < 0.35 {
                let lat_l = -(f.wall_l + 1.0 + rng.next_f64() * 5.0);
                let xl = f.x + f.rx * lat_l;
                let zl = f.z + f.rz * lat_l;
                let sv = self.surf.sample(xl, zl);
                if sv.h > f.y - 4.0
                    && sv.slope < 0.9
                    && !self.excluded(xl, zl, 0.0)
                    && self.clear_of_road(xl, zl, 1.0)
                {
                    let sc2 = lerp(0.9, 2.0, rng.next_f64());
                    let sz = sc2 * lerp(0.7, 1.2, rng.next_f64());
                    let ry = rng.next_f64() * TURN;
                    let col = pick(&mut rng, &ICE_COLS);
                    let bb = lerp(0.8, 1.0, rng.next_f64());
                    bushes[c].push(Item {
                        ry,
                        col: Some(col),
                        b: Some(bb),
                        ..Item::at(xl, sv.h - 0.3, zl, sc2, sc2 * 0.4, sz)
                    });
                }
            }
        }
        let bush_mat = self.mat(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.95),
        );
        let bgeo = self.geo(bush_geometry());
        let mut nb = 0;
        for items in &bushes {
            if !items.is_empty() {
                nb += items.len();
                self.add_instanced(bgeo, bush_mat, items, false, true);
            }
        }
        (trees, nb)
    }

    // ── Grass: tufts along both verges and over the cliff tops ──────────

    fn build_grass(&mut self) -> usize {
        let t = self.t;
        let mut rng = Mulberry32::new(7373);
        let mut chunks: Vec<Vec<Item>> = vec![Vec::new(); self.n_chunks];
        const COLS: [u32; 8] = [
            0x8a9a5a, 0x9aa070, 0xc8b47a, 0xb8a868, 0xa8a060, 0x9aa88a, 0x7d8c50, 0xd2bf86,
        ];
        let mut s = 6.0;
        while s < self.z_end + 80.0 {
            let cur = s;
            s += 1.1;
            let f = t.frame(cur + (rng.next_f64() - 0.5) * 0.8);
            let side = if rng.next_f64() < 0.55 { 1.0 } else { -1.0 };
            let wall = if side > 0.0 { f.wall_r } else { f.wall_l };
            // Thick on the verge, thinning out up the slopes.
            let lat = side * (wall + 1.2 + kernel::pow(rng.next_f64(), 2.2) * 38.0);
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let sv = self.surf.sample(x, z);
            if sv.slope > 1.05 || sv.h < self.sea_y + 2.0 || (sv.h - f.y).abs() > 14.0 {
                continue;
            }
            if self.excluded(x, z, 0.5) || !self.clear_of_road(x, z, 0.4) {
                continue;
            }
            // A clump: many small tufts of one grass, the odd poppy among them.
            let n = 3.0 + (rng.next_f64() * 6.0).floor();
            let base = COLS[(rng.next_f64() * COLS.len() as f64).floor() as usize];
            let mut k = 0.0;
            while k < n {
                k += 1.0;
                let r = rng.next_f64() * 2.4;
                let a = rng.next_f64() * 6.3;
                let px = x + kernel::cos(a) * r;
                let pz = z + kernel::sin(a) * r;
                let sc = lerp(0.5, 1.05, rng.next_f64());
                let c = if rng.next_f64() < 0.03 {
                    0xe8902a
                } else if rng.next_f64() < 0.7 {
                    base
                } else {
                    COLS[(rng.next_f64() * COLS.len() as f64).floor() as usize]
                };
                let y = self.surf.sample(px, pz).h - 0.05;
                let sy = sc * lerp(0.7, 1.2, rng.next_f64());
                let ry = rng.next_f64() * TURN;
                let bb = lerp(0.8, 1.1, rng.next_f64());
                let ch = self.chunk_of(cur);
                chunks[ch].push(Item {
                    ry,
                    col: Some(c),
                    b: Some(bb),
                    ..Item::at(px, y, pz, sc, sy, sc)
                });
            }
        }
        let mat = self.mat(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 1.0)
                .set("side", DOUBLE_SIDE),
        );
        let geo = self.geo(tuft_geometry());
        let mut n = 0;
        for items in &chunks {
            if !items.is_empty() {
                n += items.len();
                self.add_instanced(geo, mat, items, false, true);
            }
        }
        n
    }

    // ── Crags: ledged slabs of rock on every steep face, seaward cliffs
    // and landward cuttings alike, so the height field reads as carved
    // cliff ──

    fn build_crags(&mut self) -> usize {
        let t = self.t;
        let mut rng = Mulberry32::new(5151);
        let mut chunks: Vec<Vec<Item>> = vec![Vec::new(); self.n_chunks];
        const PALETTE: [u32; 6] = [0xc9ad8c, 0xb89c7e, 0xa89484, 0xd2b99a, 0x9e8f82, 0xb5a08a];
        let pick = |rng: &mut Mulberry32| {
            PALETTE[(rng.next_f64() * PALETTE.len() as f64).floor() as usize]
        };
        let mut s = 0.0;
        while s < self.z_end + 120.0 {
            let f = t.frame(s + (rng.next_f64() - 0.5) * 6.0);
            for side in [-1.0, 1.0] {
                let wall = if side > 0.0 { f.wall_r } else { f.wall_l };
                let mut d = wall + 3.0;
                'march: while d < 260.0 {
                    'body: {
                        let x = f.x + f.rx * side * d;
                        let z = f.z + f.rz * side * d;
                        let sv = self.surf.sample(x, z);
                        if sv.h < self.sea_y - 2.0 {
                            break 'march; // out over the water
                        }
                        if sv.slope < 0.85 || rng.next_f64() > smoothstep(0.85, 1.7, sv.slope) * 0.8
                        {
                            break 'body;
                        }
                        if !self.in_zone(x, 0.4)
                            || self.excluded(x, z, 4.0)
                            || !self.clear_of_road(x, z, 3.5)
                        {
                            break 'body;
                        }
                        // Face downhill; sink the slab back into the slope.
                        let gl = sv.slope;
                        let dx = -sv.gx / gl;
                        let dz = -sv.gz / gl;
                        // Seaward faces already show their strata: there the
                        // slabs sit deeper, as ledges rather than boulders.
                        let sea = side < 0.0;
                        let w = lerp(5.0, 14.0, kernel::pow(rng.next_f64(), 1.3));
                        let hgt = lerp(2.5, if sea { 5.0 } else { 7.0 }, rng.next_f64());
                        let dep = lerp(2.2, 4.0, rng.next_f64());
                        let back = dep * if sea { 0.8 } else { 0.55 };
                        let ry = kernel::atan2(dx, dz) + (rng.next_f64() - 0.5) * 0.4;
                        let col = pick(&mut rng);
                        let bb = lerp(0.78, 1.05, rng.next_f64());
                        let c = self.chunk_of(s);
                        chunks[c].push(Item {
                            ry,
                            col: Some(col),
                            b: Some(bb),
                            ..Item::at(x - dx * back, sv.h - hgt * 0.25, z - dz * back, w, hgt, dep)
                        });
                    }
                    d += lerp(6.0, 11.0, rng.next_f64());
                }
            }
            s += 9.0;
        }
        // The sea cliffs are near vertical, so a march across them barely
        // touches the face: dress them from the shoreline up instead, with
        // buttresses of stacked ledges pushed into the face.
        let runs = std::mem::take(&mut self.shore_runs);
        for run in &runs {
            let mut i = 0;
            while i < run.len() {
                let p = run[i];
                i += 2;
                if rng.next_f64() < 0.3 {
                    continue;
                }
                let mut top = -1e9;
                for d in [4.0, 9.0, 15.0] {
                    top = js::max(top, self.surf.sample(p.x - p.nx * d, p.z - p.nz * d).h);
                }
                if top < self.sea_y + 8.0 {
                    continue;
                }
                let ry = kernel::atan2(p.nx, p.nz) + (rng.next_f64() - 0.5) * 0.5;
                let mut y = self.sea_y - 2.0;
                while y < top - 3.0 {
                    'body: {
                        let w = lerp(6.0, 13.0, rng.next_f64());
                        let hgt = lerp(3.0, 6.5, rng.next_f64());
                        let dep = lerp(3.0, 5.0, rng.next_f64());
                        let inset =
                            dep * lerp(0.35, 0.65, rng.next_f64()) + (rng.next_f64() - 0.5) * 1.5;
                        let x = p.x - p.nx * inset + p.tx * (rng.next_f64() - 0.5) * 4.0;
                        let z = p.z - p.nz * inset + p.tz * (rng.next_f64() - 0.5) * 4.0;
                        if self.excluded(x, z, 2.0) || !self.clear_of_road(x, z, 3.0) {
                            break 'body;
                        }
                        let col = pick(&mut rng);
                        let bb = lerp(0.75, 1.0, rng.next_f64());
                        let c = self.chunk_of(p.s);
                        chunks[c].push(Item {
                            ry,
                            col: Some(col),
                            b: Some(bb),
                            ..Item::at(x, y, z, w, hgt, dep)
                        });
                    }
                    y += lerp(5.0, 9.0, rng.next_f64());
                }
            }
        }
        self.shore_runs = runs;
        let geos = [self.geo(crag_geometry(61)), self.geo(crag_geometry(62))];
        let mut n = 0;
        for items in &chunks {
            if items.is_empty() {
                continue;
            }
            n += items.len();
            let half = items.len().div_ceil(2);
            self.add_instanced(geos[0], self.rock_mat, &items[..half], false, true);
            if items.len() > half {
                self.add_instanced(geos[1], self.rock_mat, &items[half..], false, true);
            }
        }
        n
    }

    // ── Rock outcrops on the landward slopes, boulders at the rock walls ─

    fn build_outcrops(&mut self) {
        let t = self.t;
        let terrain = self.terrain;
        let mut rng = Mulberry32::new(4545);
        let mut chunks: Vec<Vec<Item>> = vec![Vec::new(); self.n_chunks];
        const PALETTE: [u32; 6] = [0xc9ad8c, 0xb89c7e, 0xa89484, 0xd2b99a, 0x9e8f82, 0x8e8074];
        let pick = |rng: &mut Mulberry32| {
            PALETTE[(rng.next_f64() * PALETTE.len() as f64).floor() as usize]
        };
        let b = t.bounds;
        let mut x = b.min_x - 350.0;
        while x < terrain.x1() + 200.0 {
            let mut z = b.min_z - 350.0;
            while z < b.max_z + 350.0 {
                let cur_z = z;
                z += 12.0;
                let px = x + (rng.next_f64() - 0.5) * 10.0;
                let pz = cur_z + (rng.next_f64() - 0.5) * 10.0;
                let r0 = rng.next_f64();
                if r0 > 0.45 || !self.in_zone(px, 0.5) {
                    continue;
                }
                let f = terrain.far(px, pz);
                if f.side < 0.0 || f.d < 14.0 || f.d > 320.0 || f.s > self.z_end + 100.0 {
                    continue;
                }
                let sv = self.surf.sample(px, pz);
                if sv.slope < 0.8 || r0 > 0.25 * smoothstep(0.8, 1.5, sv.slope) {
                    continue;
                }
                if !self.clear_of_road(px, pz, 6.0) {
                    continue;
                }
                let size = lerp(2.5, 8.0, kernel::pow(rng.next_f64(), 1.5))
                    * if f.d < 50.0 { 0.6 } else { 1.0 };
                let sx = size * lerp(1.0, 1.7, rng.next_f64());
                let sy = size * lerp(0.6, 1.0, rng.next_f64());
                let ry = rng.next_f64() * TURN;
                let col = pick(&mut rng);
                let bb = lerp(0.75, 1.05, rng.next_f64());
                let c = self.chunk_of(f.s);
                chunks[c].push(Item {
                    ry,
                    col: Some(col),
                    b: Some(bb),
                    ..Item::at(px, sv.h - size * 0.35, pz, sx, sy, size)
                });
            }
            x += 12.0;
        }
        // Boulders heaped at the foot of roadside rock walls.
        let mut s = 20.0;
        while s < self.z_end {
            let i = t.idx(s);
            for side in [-1.0, 1.0] {
                let kind = if side < 0.0 {
                    self.side_l[i]
                } else {
                    self.side_r[i]
                };
                if kind != 1 || rng.next_f64() > 0.35 {
                    continue;
                }
                let f = t.frame(s + (rng.next_f64() - 0.5) * 3.0);
                let wall = if side < 0.0 { f.wall_l } else { f.wall_r };
                let size = lerp(0.6, 2.2, kernel::pow(rng.next_f64(), 1.4));
                let lat = side * (wall + 0.7 + size);
                let x = f.x + f.rx * lat;
                let z = f.z + f.rz * lat;
                if !self.clear_of_road(x, z, size) {
                    continue;
                }
                let sv = self.surf.sample(x, z);
                let sx = size * lerp(1.0, 1.6, rng.next_f64());
                let sy = size * lerp(0.6, 1.1, rng.next_f64());
                let col = pick(&mut rng);
                let bb = lerp(0.75, 1.0, rng.next_f64());
                let c = self.chunk_of(s);
                chunks[c].push(Item {
                    ry: kernel::atan2(f.fx, f.fz),
                    col: Some(col),
                    b: Some(bb),
                    ..Item::at(x, sv.h - size * 0.3, z, sx, sy, size)
                });
            }
            s += 4.0;
        }
        let geo = self.geo(rock_geometry(33, 1.0, 0.72, 0.42));
        for items in &chunks {
            if !items.is_empty() {
                self.add_instanced(geo, self.rock_mat, items, true, true);
            }
        }
    }

    // ── Lighthouse ──────────────────────────────────────────────────────

    fn build_lighthouse(&mut self, l: &mut Lighthouse) {
        let t = self.t;
        let sf = t.frame(l.s);
        let y = self.surf.sample(l.x, l.z).h;
        l.ground = Some(y);
        let hh = 19.0;
        // Tower: stacked bands, white and red.
        let mut bands = Vec::new();
        let n_b = 6.0;
        for k in 0..6 {
            let kf = f64::from(k);
            let r0 = lerp(2.7, 1.9, kf / n_b);
            let r1 = lerp(2.7, 1.9, (kf + 1.0) / n_b);
            let mut c = cyl(r1, r0, hh / n_b, 20.0, true);
            c.translate(0.0, hh / n_b * (kf + 0.5), 0.0);
            bands.push(colorize(c, if k % 2 == 1 { 0xb3262c } else { 0xf2efe8 }));
        }
        let mut base = cyl(3.2, 3.4, 1.2, 20.0, false);
        base.translate(0.0, 0.4, 0.0);
        bands.push(colorize(base, 0xd8d2c6));
        let mut gallery = cyl(2.7, 2.5, 0.35, 20.0, false);
        gallery.translate(0.0, hh + 0.15, 0.0);
        bands.push(colorize(gallery, 0x2a2d31));
        let mut rail = torus_geometry(2.6, 0.05, 4.0, 24.0, PI * 2.0);
        rail.rotate_x(PI / 2.0);
        rail.translate(0.0, hh + 1.2, 0.0);
        bands.push(colorize(rail, 0x2a2d31));
        for k in 0..12 {
            let a = f64::from(k) / 12.0 * PI * 2.0;
            let mut post = boxg(0.06, 0.9, 0.06);
            post.translate(kernel::cos(a) * 2.6, hh + 0.75, kernel::sin(a) * 2.6);
            bands.push(colorize(post, 0x2a2d31));
        }
        let mut dome = sphere_geometry(1.55, 16.0, 8.0, 0.0, PI * 2.0, 0.0, PI / 2.0);
        dome.translate(0.0, hh + 2.85, 0.0);
        bands.push(colorize(dome, 0x8a1c20));
        let mut vent = cone_geometry(0.3, 0.9, 8.0, 1.0, false, 0.0, PI * 2.0);
        vent.translate(0.0, hh + 4.6, 0.0);
        bands.push(colorize(vent, 0x2a2d31));
        let tower_mat = self.mat(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.55)
                .set("metalness", 0.05),
        );
        let geos: Vec<BufferGeometry> = bands
            .into_iter()
            .map(|g| {
                let mut h = if g.index.is_some() {
                    g.to_non_indexed()
                } else {
                    g
                };
                if h.has_attribute("uv") {
                    h.delete_attribute("uv");
                }
                h.compute_vertex_normals();
                placed(&h, l.x, y, l.z, 0.0)
            })
            .collect();
        let tg = self.geo(merge(&geos));
        let tower = self.graph.mesh(tg, tower_mat);
        {
            let o = self.graph.get_mut(tower);
            o.cast_shadow = true;
            o.receive_shadow = true;
            o.matrix_auto_update = false;
        }
        self.add(tower);
        // Lantern room glass.
        let lantern_mat = self.mat(
            Material::standard()
                .set("color", 0xfff2c8)
                .set("emissive", 0xffe2a0)
                .set("emissiveIntensity", 1.2)
                .set("roughness", 0.1)
                .set("transparent", true)
                .set("opacity", 0.9),
        );
        self.graph
            .add_night(Some(lantern_mat), "emissiveIntensity", 1.2, 5.5);
        let mut lg = cyl(1.35, 1.35, 1.9, 16.0, false);
        lg.translate(l.x, y + hh + 1.35, l.z);
        let lg = self.geo(lg);
        let lantern = self.graph.mesh(lg, lantern_mat);
        self.add(lantern);
        // Keeper's cottage beside the tower, facing the road.
        let to_road = kernel::atan2(sf.rx, sf.rz); // local +z points roughly toward the road
        let wall = self.mat(
            Material::standard()
                .set("color", 0xf1ede4)
                .set("roughness", 0.85),
        );
        let roof = self.mat(
            Material::standard()
                .set("color", 0x9b2a24)
                .set("roughness", 0.8),
        );
        let glass = self.mat(
            Material::standard()
                .set("color", 0x3a3226)
                .set("roughness", 0.25)
                .set("emissive", 0xffc27a)
                .set("emissiveIntensity", 0.4),
        );
        self.graph
            .add_night(Some(glass), "emissiveIntensity", 0.3, 1.8);
        let cx = l.x + sf.fx * 9.0 + sf.rx * 3.0;
        let cz = l.z + sf.fz * 9.0 + sf.rz * 3.0;
        let p = |mut geo: BufferGeometry, lx: f64, ly: f64, lz: f64| {
            geo.translate(lx, ly, lz);
            placed(&geo, cx, y, cz, to_road)
        };
        let wg = vec![
            p(boxg(9.0, 3.6, 6.0), 0.0, 1.8, 0.0),
            p(boxg(2.2, 5.2, 1.2), 3.0, 2.6, -2.2),
        ];
        let mut rs = Shape::new();
        rs.move_to(-3.4, 0.0);
        rs.line_to(0.0, 2.1);
        rs.line_to(3.4, 0.0);
        rs.close_path();
        let mut rg = extrude_geometry(&[rs], &ExtrudeOptions::flat(9.6));
        rg.translate(0.0, 0.0, -4.8);
        rg.rotate_y(PI / 2.0);
        let rgeos = vec![p(rg, 0.0, 3.6, 0.0)];
        let mut gg = Vec::new();
        for lx in [-3.0, -1.0, 2.5] {
            gg.push(p(plane_geometry(1.1, 1.3, 1.0, 1.0), lx, 2.0, 3.02));
        }
        gg.push(p(plane_geometry(1.0, 2.1, 1.0, 1.0), 0.8, 1.05, 3.02));
        for (geos2, mat, cast) in [(wg, wall, true), (rgeos, roof, true), (gg, glass, false)] {
            self.add_merged(geos2, mat, cast, true);
        }

        // Rotating beam: twin cones from the lantern, plus a glow.
        let beam_len = 170.0;
        let mut beam_geo = cone_geometry(6.0, beam_len, 24.0, 1.0, true, 0.0, PI * 2.0);
        beam_geo.translate(0.0, -beam_len / 2.0, 0.0); // apex at origin
        beam_geo.rotate_x(PI / 2.0); // extends along −Z
        let beam_mat = self.mat(
            fog_uniforms(Material::shader().kind(MaterialKind::LighthouseBeam, None))
                .shader_source(BEAM_VERT, BEAM_FRAG)
                .uniform("uColor", color_value(Color::new(1.0, 0.93, 0.78)))
                .uniform("uStrength", num(0.5))
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", ADDITIVE)
                .set("side", DOUBLE_SIDE)
                .set("fog", true),
        );
        let pivot = self.graph.group("");
        let pivot_pos = Vector3::new(l.x, y + hh + 1.35, l.z);
        self.graph.get_mut(pivot).position = pivot_pos;
        let bg = self.geo(beam_geo);
        let b1 = self.graph.mesh(bg, beam_mat);
        let b2 = self.graph.mesh(bg, beam_mat);
        {
            let o = self.graph.get_mut(b1);
            o.set_rotation(&Euler::new(-0.03, 0.0, 0.0)); // aimed a touch down at the water
            o.frustum_culled = false;
        }
        {
            let o = self.graph.get_mut(b2);
            o.set_rotation(&Euler::new(-0.03, PI, 0.0));
            o.frustum_culled = false;
        }
        self.graph.add(pivot, b1);
        self.graph.add(pivot, b2);
        let glow_tex = self
            .graph
            .cached_texture(&self.textures.glow_texture(), Layer::Main, "");
        let glow_mat = self.mat(
            Material::sprite()
                .set("map", glow_tex)
                .set("color", 0xffe7b0)
                .set("transparent", true)
                .set("blending", ADDITIVE)
                .set("depthWrite", false),
        );
        let sg = self.geo(crate::mountain::sprite_geometry());
        let glow = self.graph.drawable(NodeType::Sprite, sg, glow_mat);
        {
            let o = self.graph.get_mut(glow);
            o.scale = Vector3::new(9.0, 9.0, 9.0);
            o.position = pivot_pos;
        }
        self.add(pivot);
        self.add(glow);
        let mut ry = 0.0;
        self.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                ry += u.dt * 0.9;
                let q = Quaternion::from_euler(&Euler::new(0.0, ry, 0.0));
                out.push(Edit {
                    target: Handle::Node(pivot),
                    change: Change::Transform {
                        position: [pivot_pos.x, pivot_pos.y, pivot_pos.z],
                        quaternion: [q.x, q.y, q.z, q.w],
                        scale: [1.0, 1.0, 1.0],
                    },
                });
                out.push(Edit {
                    target: Handle::Material(beam_mat),
                    change: Change::Number {
                        prop: "uStrength",
                        value: 0.03 + 0.32 * smoothstep(0.2, 0.8, u.night),
                    },
                });
                out.push(Edit {
                    target: Handle::Material(glow_mat),
                    change: Change::Number {
                        prop: "opacity",
                        value: 0.25 + 0.75 * u.night,
                    },
                });
                let sc = 6.0 + 8.0 * u.night;
                out.push(Edit {
                    target: Handle::Node(glow),
                    change: Change::Transform {
                        position: [pivot_pos.x, pivot_pos.y, pivot_pos.z],
                        quaternion: [0.0, 0.0, 0.0, 1.0],
                        scale: [sc, sc, sc],
                    },
                });
            }));
    }

    // ── Start gantry: HOLLOW POINT ──────────────────────────────────────

    fn build_start_area(&mut self) {
        let t = self.t;
        let s = t.start_s;
        let sf = t.frame(s);
        let wl = at(&t.wall_l, t.idx(s));
        let wr = at(&t.wall_r, t.idx(s));
        let yaw = kernel::atan2(sf.fx, sf.fz);
        let steel = self.mat(
            Material::standard()
                .set("color", 0x23282e)
                .set("metalness", 0.7)
                .set("roughness", 0.35),
        );
        let mut geos = Vec::new();
        const H: f64 = 7.2;
        for lat in [-(wl + 1.25), wr + 1.25] {
            let x = sf.x + sf.rx * lat;
            let z = sf.z + sf.rz * lat;
            let y = sf.y - lat * sf.bank;
            for off in [-0.35, 0.35] {
                let mut up = boxg(0.16, H + 1.5, 0.16);
                up.translate(0.0, (H + 1.5) / 2.0 - 1.2, off);
                geos.push(placed(&up, x, y, z, yaw));
            }
            for k in 0..7 {
                let mut rung = boxg(0.08, 0.08, 0.7);
                rung.translate(0.0, 0.5 + f64::from(k) * 1.0, 0.0);
                geos.push(placed(&rung, x, y, z, yaw));
            }
            let foot = boxg(1.1, 0.5, 1.4);
            geos.push(placed(&foot, x, y - 0.05, z, yaw));
        }
        let span = wl + wr + 2.5;
        let cx = sf.x + sf.rx * ((wr - wl) / 2.0);
        let cz = sf.z + sf.rz * ((wr - wl) / 2.0);
        for dy in [H, H - 0.9] {
            let mut beam = boxg(span, 0.18, 0.18);
            beam.translate(0.0, sf.y + dy, 0.0);
            geos.push(placed(&beam, cx, 0.0, cz, yaw));
        }
        self.add_merged(geos, steel, true, true);

        let bw = span - 1.6;
        let bh = bw * 160.0 / 1024.0;
        for front in [true, false] {
            let c = banner_canvas(front);
            let tex = canvas_tex(self.graph, &c);
            let mat = self.mat(
                Material::standard()
                    .set("map", tex)
                    .set("roughness", 0.7)
                    .set("emissive", 0xffffff)
                    .set("emissiveMap", tex)
                    .set("emissiveIntensity", 0.2),
            );
            self.graph
                .add_night(Some(mat), "emissiveIntensity", 0.15, 0.7);
            let mut g = plane_geometry(bw, bh, 1.0, 1.0);
            g.rotate_y(if front { PI } else { 0.0 });
            g.translate(
                0.0,
                sf.y + H + 0.1 - bh / 2.0,
                if front { -0.05 } else { 0.05 },
            );
            let geo = self.geo(placed(&g, cx, 0.0, cz, yaw));
            let mesh = self.graph.mesh(geo, mat);
            self.graph.get_mut(mesh).matrix_auto_update = false;
            self.add(mesh);
        }
    }

    /// A gravel pad's material: `gravelTexture().clone()` repeated.
    fn gravel(&mut self, color: u32, repeat: f64) -> MaterialId {
        let cached = self
            .graph
            .cached_texture(&self.textures.gravel_texture(), Layer::Main, "");
        let map = self.graph.clone_texture(cached);
        crate::mountain::canvas::set_repeat(self.graph, map, repeat, repeat);
        self.mat(
            Material::standard()
                .set("map", map)
                .set("roughness", 1.0)
                .set("color", color)
                .set("polygonOffset", true)
                .set("polygonOffsetFactor", -1.0),
        )
    }

    /// `buildVehicle(kind, { color, seed, lod: 'low' })`, placed and
    /// turned, its headlights off; `boards` adds the surfboards to its body.
    fn parked_vehicle(
        &mut self,
        kind: &str,
        color: u32,
        seed: u32,
        p: Vector3,
        yaw: f64,
        boards: bool,
    ) {
        let Some(mut v) = car_model::build_vehicle(
            self.graph,
            self.textures,
            kind,
            &BuildOpts {
                color: Some(color),
                seed,
                lod: Some(Lod::Low),
                ..BuildOpts::default()
            },
        ) else {
            return;
        };
        {
            let o = self.graph.get_mut(v.root);
            o.position = p;
            o.set_rotation(&Euler::new(0.0, yaw, 0.0));
        }
        let edits = v.set_headlights(0.0);
        car_model::apply_edits(self.graph, &edits);
        if boards {
            let group = self.graph.group("");
            const COLS: [u32; 3] = [0xf2c14e, 0xff6b6b, 0xfafafa];
            let h = v.dims.height;
            for (k, col) in COLS.iter().enumerate() {
                let k = k as f64;
                let g = self.geo(capsule_geometry(0.28, 2.3, 3.0, 8.0, 1.0));
                let m = self.mat(
                    Material::standard()
                        .set("color", *col)
                        .set("roughness", 0.4),
                );
                let b = self.graph.mesh(g, m);
                let o = self.graph.get_mut(b);
                o.scale = Vector3::new(1.0, 1.0, 0.18);
                o.set_rotation(&Euler::new(PI / 2.0, 0.0, 0.0));
                o.position = Vector3::new((k - 1.0) * 0.62, h + 0.12 + k * 0.05, 0.0);
                self.graph.add(group, b);
            }
            self.graph.add(v.body, group);
        }
        let baked = bake_static(self.graph, v.root);
        self.add(baked);
    }

    // ── Coffee shack and a surf van at the start pull-out ───────────────

    fn build_pullout(&mut self, d: &Spot) {
        let t = self.t;
        let sf = t.frame(d.s);
        let y = d.y;
        let yaw = kernel::atan2(-sf.rx * d.side, -sf.rz * d.side); // local +z faces the road
        let mut g0 = circle_geometry(13.0, 28.0, 0.0, PI * 2.0);
        g0.rotate_x(-PI / 2.0);
        g0.translate(d.x, y + 0.06, d.z);
        let gravel = self.gravel(0xb9ae9c, 5.0);
        let g0 = self.geo(g0);
        let pad = self.graph.mesh(g0, gravel);
        self.graph.get_mut(pad).receive_shadow = true;
        self.add(pad);

        let wood = self.mat(
            Material::standard()
                .set("color", 0x8a6446)
                .set("roughness", 0.9),
        );
        let roof_m = self.mat(
            Material::standard()
                .set("color", 0x3f4a4f)
                .set("roughness", 0.7)
                .set("metalness", 0.3),
        );
        let paint = self.mat(
            Material::standard()
                .set("color", 0x2ec4b6)
                .set("roughness", 0.6),
        );
        let warm = self.mat(
            Material::standard()
                .set("color", 0x3a2e22)
                .set("emissive", 0xffb866)
                .set("emissiveIntensity", 0.6)
                .set("roughness", 0.4),
        );
        self.graph
            .add_night(Some(warm), "emissiveIntensity", 0.35, 1.0);
        let bx = d.x - sf.rx * 2.0 - sf.fx * 3.0;
        let bz = d.z - sf.rz * 2.0 - sf.fz * 3.0;
        let p = |mut geo: BufferGeometry, lx: f64, ly: f64, lz: f64| {
            geo.translate(lx, ly, lz);
            placed(&geo, bx, y, bz, yaw)
        };
        let mut wg = vec![p(boxg(5.0, 3.0, 3.6), 0.0, 1.5, 0.0)];
        // Shed roof, sloping back.
        let mut roof_g = boxg(6.0, 0.18, 4.8);
        roof_g.rotate_x(-0.16);
        let rgeos = vec![p(roof_g, 0.0, 3.35, 0.2)];
        let pg = vec![
            p(boxg(5.05, 0.45, 3.65), 0.0, 2.75, 0.0),
            p(boxg(3.4, 0.12, 0.7), 0.0, 1.1, 2.1),
        ];
        let mut side = plane_geometry(0.9, 0.8, 1.0, 1.0);
        side.rotate_y(-PI / 2.0);
        let lit = vec![
            p(plane_geometry(3.0, 1.2, 1.0, 1.0), 0.0, 1.85, 1.82),
            p(side, -2.52, 1.9, 0.0),
        ];
        // Picnic table.
        wg.push(p(boxg(2.2, 0.08, 0.9), 4.5, 0.8, 3.5));
        wg.push(p(boxg(2.2, 0.06, 0.35), 4.5, 0.48, 2.75));
        wg.push(p(boxg(2.2, 0.06, 0.35), 4.5, 0.48, 4.25));
        for lx in [3.6, 5.4] {
            wg.push(p(boxg(0.1, 0.8, 1.6), lx, 0.4, 3.5));
        }
        for (geos, mat, cast) in [
            (wg, wood, true),
            (rgeos, roof_m, true),
            (pg, paint, true),
            (lit, warm, false),
        ] {
            self.add_merged(geos, mat, cast, true);
        }
        // Sign over the window.
        let c = coffee_sign_canvas();
        let tex = canvas_tex(self.graph, &c);
        let sign_mat = self.mat(
            Material::standard()
                .set("map", tex)
                .set("emissive", 0xffffff)
                .set("emissiveMap", tex)
                .set("emissiveIntensity", 0.5)
                .set("roughness", 0.5),
        );
        self.graph
            .add_night(Some(sign_mat), "emissiveIntensity", 0.3, 1.8);
        let sg = self.geo(p(plane_geometry(4.4, 1.1, 1.0, 1.0), 0.0, 3.25, 1.95));
        let sm = self.graph.mesh(sg, sign_mat);
        self.add(sm);

        // String lights between two posts along the front.
        let mut bulbs = Vec::new();
        let post_a = Vector3::new(-4.0, 3.2, 4.2);
        let post_b = Vector3::new(7.0, 3.2, 5.2);
        let mut poles = Vec::new();
        for pp in [post_a, post_b] {
            poles.push(p(cyl(0.06, 0.06, 3.3, 5.0, false), pp.x, 1.65, pp.z));
        }
        self.add_merged(poles, wood, false, true);
        for k in 0..=14 {
            let f = f64::from(k) / 14.0;
            let lx = lerp(post_a.x, post_b.x, f);
            let lz = lerp(post_a.z, post_b.z, f);
            let ly = 3.1 - kernel::sin(f * PI) * 0.6;
            let v = Vector3::new(lx, ly, lz).apply_axis_angle(UP, yaw);
            bulbs.push(Item::at(bx + v.x, y + v.y, bz + v.z, 1.0, 1.0, 1.0));
        }
        let bulb_mat = self.mat(Material::basic().set("color", Color::new(3.5, 2.4, 1.2)));
        self.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                let mut c = Color::new(0.0, 0.0, 0.0);
                c.set_rgb(3.5, 2.4, 1.2).multiply_scalar(0.25 + u.night);
                out.push(Edit {
                    target: Handle::Material(bulb_mat),
                    change: Change::Color {
                        prop: "color",
                        rgb: [c.r, c.g, c.b],
                    },
                });
            }));
        let bgeo = self.geo(icosahedron_geometry(0.09, 0.0));
        self.add_instanced(bgeo, bulb_mat, &bulbs, false, false);

        // Vintage surf van with boards on the roof.
        let lp = Vector3::new(-6.5, 0.0, 4.5).apply_axis_angle(UP, yaw);
        self.parked_vehicle(
            "van",
            0x9fd8cf,
            21,
            Vector3::new(d.x + lp.x, y + 0.05, d.z + lp.z),
            yaw + 1.3,
            true,
        );
    }

    // ── Vista pull-out on the bluff ─────────────────────────────────────

    fn build_vista(&mut self, l: &Spot) {
        let t = self.t;
        let sf = t.frame(l.s);
        let out = (sf.rx * l.side, sf.rz * l.side);
        let yaw = kernel::atan2(out.0, out.1);
        let y = l.y;
        let gravel = self.gravel(0xa99f90, 4.0);
        let mut g0 = circle_geometry(9.6, 28.0, 0.0, PI * 2.0);
        g0.rotate_x(-PI / 2.0);
        g0.translate(l.x, y + 0.06, l.z);
        let g0 = self.geo(g0);
        let pad = self.graph.mesh(g0, gravel);
        self.graph.get_mut(pad).receive_shadow = true;
        self.add(pad);
        let mut stones = Vec::new();
        let mut a = -1.3;
        while a <= 1.3 {
            let r = 9.2;
            let mut bxg = boxg(1.2, 0.75 + kernel::sin(a * 11.0).abs() * 0.15, 0.7);
            bxg.rotate_y(a);
            bxg.translate(kernel::sin(a) * r, 0.37, kernel::cos(a) * r);
            stones.push(placed(&bxg, l.x, y, l.z, yaw));
            a += 0.1;
        }
        let stone_mat = {
            let m = rock_material(self.graph, self.textures).set("color", 0xcdbfa8);
            self.mat(m)
        };
        self.add_merged(stones, stone_mat, true, true);
        let metal = self.mat(
            Material::standard()
                .set("color", 0x3d6b6b)
                .set("metalness", 0.6)
                .set("roughness", 0.4),
        );
        let wood = self.mat(
            Material::standard()
                .set("color", 0x6d5038)
                .set("roughness", 0.9),
        );
        let p = |mut geo: BufferGeometry, lx: f64, ly: f64, lz: f64| {
            geo.translate(lx, ly, lz);
            placed(&geo, l.x, y, l.z, yaw)
        };
        let mg = vec![
            p(cyl(0.08, 0.12, 1.1, 8.0, false), 1.5, 0.55, 8.2),
            p(boxg(0.5, 0.35, 0.7), 1.5, 1.25, 8.2),
        ];
        let mut wg = vec![
            p(boxg(2.2, 0.08, 0.5), -2.5, 0.5, 7.2),
            p(boxg(2.2, 0.5, 0.08), -2.5, 0.8, 7.45),
        ];
        for lx in [-3.4, -1.6] {
            wg.push(p(boxg(0.1, 0.5, 0.5), lx, 0.25, 7.2));
        }
        self.add_merged(mg, metal, true, true);
        self.add_merged(wg, wood, true, true);
        let lp = Vector3::new(3.5, 0.0, 2.0).apply_axis_angle(UP, yaw);
        self.parked_vehicle(
            "hatch",
            0xd9a441,
            8,
            Vector3::new(l.x + lp.x, y + 0.05, l.z + lp.z),
            yaw - 0.2,
            false,
        );
    }

    // ── Road signs ──────────────────────────────────────────────────────

    fn build_signs(
        &mut self,
        segs: &[(f64, f64)],
        vista: Option<&Spot>,
        lighthouse: Option<&Lighthouse>,
    ) {
        let t = self.t;
        let z_end = self.z_end;
        let mut atlas = SignAtlas::new();
        struct Sign {
            s: f64,
            rect: Rect,
            w: f64,
            h: f64,
            height: f64,
            side: f64,
            extra: Option<(Rect, f64, f64)>,
        }
        let mut signs: Vec<Sign> = Vec::new();
        let tag_d = |mut r: Rect| {
            r.diamond = true;
            r
        };
        let curve = |atlas: &mut SignAtlas, dir: f64| {
            tag_d(atlas.add(256.0, 256.0, |g, w, h| {
                diamond(g, w, h, |g2, cx, cy| {
                    g2.save();
                    g2.translate(cx, cy);
                    g2.scale(dir, 1.0);
                    g2.set_line_width(17.0);
                    g2.set_line_cap("butt");
                    g2.begin_path();
                    g2.move_to(-8.0, 60.0);
                    g2.line_to(-8.0, 8.0);
                    g2.quadratic_curve_to(-8.0, -26.0, 26.0, -34.0);
                    g2.stroke();
                    g2.begin_path();
                    g2.move_to(18.0, -60.0);
                    g2.line_to(54.0, -34.0);
                    g2.line_to(16.0, -8.0);
                    g2.close_path();
                    g2.fill();
                    g2.restore();
                })
            }))
        };
        let text_diamond = |atlas: &mut SignAtlas, lines: &[&str], size: f64| {
            tag_d(atlas.add(256.0, 256.0, |g, w, h| {
                diamond(g, w, h, |g2, cx, cy| {
                    g2.set_text_align("center");
                    g2.set_text_baseline("middle");
                    g2.set_font(&format!("bold {size}px {FONT}"));
                    let n = lines.len() as f64;
                    for (i, l) in lines.iter().enumerate() {
                        g2.fill_text(l, cx, cy + (i as f64 - (n - 1.0) / 2.0) * size * 1.05);
                    }
                })
            }))
        };
        let mph = |atlas: &mut SignAtlas, n: u32| {
            atlas.add(200.0, 240.0, |g, w, h| {
                panel(
                    g,
                    w,
                    h,
                    "#f5c518",
                    "#111",
                    "#111",
                    &[&n.to_string(), "MPH"],
                    &[110.0, 52.0],
                )
            })
        };
        let curve_l = curve(&mut atlas, -1.0);
        let curve_r = curve(&mut atlas, 1.0);
        let adv25 = mph(&mut atlas, 25);
        let adv35 = mph(&mut atlas, 35);
        // Warn before every sharp bend: sharpness from the laid segments.
        let mut s0 = 0.0;
        for &(len, turn) in segs {
            if s0 > z_end {
                break;
            }
            let sharp = turn.abs() / len;
            if sharp > 0.36 && s0 > 120.0 {
                let plate = if sharp > 0.55 {
                    Some(adv25)
                } else if sharp > 0.42 {
                    Some(adv35)
                } else {
                    None
                };
                signs.push(Sign {
                    s: s0 - 70.0,
                    rect: if turn < 0.0 { curve_l } else { curve_r },
                    w: 1.2,
                    h: 1.2,
                    height: if plate.is_some() { 2.5 } else { 2.2 },
                    side: 1.0,
                    extra: plate.map(|r| (r, 0.75, 0.9)),
                });
            }
            s0 += len;
        }
        // Falling rocks where the land side is a long rock face.
        let falling = text_diamond(&mut atlas, &["FALLING", "ROCKS"], 44.0);
        let mut run_start = -1.0;
        let mut placed_fr = 0;
        let mut s = 100.0;
        while s < z_end && placed_fr < 3 {
            let rock = self.side_r[t.idx(s)] == 1;
            if rock {
                if run_start < 0.0 {
                    run_start = s;
                }
                if s - run_start > 90.0 {
                    signs.push(Sign {
                        s: run_start - 40.0,
                        rect: falling,
                        w: 1.2,
                        h: 1.2,
                        height: 2.4,
                        side: 1.0,
                        extra: None,
                    });
                    placed_fr += 1;
                    run_start = 1e9;
                }
            } else if run_start != 1e9 {
                run_start = -1.0;
            }
            if run_start == 1e9 && self.side_r[t.idx(s)] != 1 {
                run_start = -1.0;
            }
            s += 5.0;
        }
        // Speed limit, turnouts, vista, distances.
        let lim = atlas.add(200.0, 256.0, |g, w, h| {
            panel(
                g,
                w,
                h,
                "#f8f8f4",
                "#111",
                "#111",
                &["SPEED", "LIMIT", "45"],
                &[44.0, 44.0, 96.0],
            )
        });
        let mut add = |s: f64, rect: Rect, w: f64, h: f64, height: f64| {
            signs.push(Sign {
                s,
                rect,
                w,
                h,
                height,
                side: 1.0,
                extra: None,
            })
        };
        add(230.0, lim, 0.9, 1.15, 2.1);
        let turnouts = atlas.add(512.0, 256.0, |g, w, h| {
            panel(
                g,
                w,
                h,
                "#f8f8f4",
                "#111",
                "#111",
                &["SLOWER TRAFFIC", "USE TURNOUTS"],
                &[60.0, 60.0],
            )
        });
        add(1100.0, turnouts, 2.4, 1.2, 2.0);
        if let Some(v) = vista {
            let v1 = atlas.add(512.0, 256.0, |g, w, h| {
                panel(
                    g,
                    w,
                    h,
                    "#5a3a22",
                    "#f3ead8",
                    "#f3ead8",
                    &["VISTA POINT", "1/4 MILE"],
                    &[66.0, 54.0],
                )
            });
            add(v.s - 420.0, v1, 2.4, 1.2, 2.0);
            let v2 = atlas.add(512.0, 256.0, |g, w, h| {
                panel(
                    g,
                    w,
                    h,
                    "#5a3a22",
                    "#f3ead8",
                    "#f3ead8",
                    &["VISTA POINT"],
                    &[70.0],
                );
                g.set_fill_style("#f3ead8");
                g.begin_path();
                g.move_to(96.0, 200.0);
                g.line_to(60.0, 178.0);
                g.line_to(96.0, 156.0);
                g.close_path();
                g.fill();
                g.fill_rect(96.0, 172.0, 60.0, 12.0);
            });
            add(v.s - 90.0, v2, 2.4, 1.2, 2.0);
        }
        if let Some(l) = lighthouse {
            let lh = atlas.add(512.0, 256.0, |g, w, h| {
                panel(
                    g,
                    w,
                    h,
                    "#5a3a22",
                    "#f3ead8",
                    "#f3ead8",
                    &["HOLLOW POINT", "LIGHT STATION"],
                    &[58.0, 52.0],
                )
            });
            add(l.s - 260.0, lh, 2.4, 1.2, 2.0);
        }
        let miles = |s: f64| js::max(1.0, js::round((z_end - s) / 1609.0));
        let port_s = t.tag("bridge").first().map_or(z_end + 3000.0, |g| g.s0);
        let dist = atlas.add(512.0, 256.0, |g, w, h| {
            panel(g, w, h, "#0b6b3a", "#fff", "#fff", &[], &[]);
            g.set_fill_style("#fff");
            g.set_font(&format!("bold 58px {FONT}"));
            g.set_text_baseline("middle");
            g.set_text_align("left");
            g.fill_text("Seabright", 36.0, 88.0);
            g.fill_text("Port Meridian", 36.0, 172.0);
            g.set_text_align("right");
            g.fill_text(&format!("{}", miles(300.0)), w - 36.0, 88.0);
            g.fill_text(
                &format!("{}", js::max(2.0, js::round((port_s - 300.0) / 1609.0))),
                w - 36.0,
                172.0,
            );
        });
        add(300.0, dist, 3.2, 1.6, 2.2);
        let dist2 = atlas.add(512.0, 160.0, |g, w, h| {
            panel(g, w, h, "#0b6b3a", "#fff", "#fff", &[], &[]);
            g.set_fill_style("#fff");
            g.set_font(&format!("bold 60px {FONT}"));
            g.set_text_baseline("middle");
            g.set_text_align("left");
            g.fill_text("Seabright", 36.0, 82.0);
            g.set_text_align("right");
            g.fill_text(&format!("{}", miles(z_end - 1800.0)), w - 36.0, 82.0);
        });
        add(z_end - 1800.0, dist2, 3.2, 1.0, 2.2);

        let tex = canvas_tex(self.graph, &atlas.canvas);
        let face_mat = self.mat(
            Material::standard()
                .set("map", tex)
                .set("roughness", 0.45)
                .set("metalness", 0.1)
                .set("emissive", 0xffffff)
                .set("emissiveMap", tex)
                .set("emissiveIntensity", 0.3)
                .set("alphaTest", 0.5),
        );
        self.graph
            .add_night(Some(face_mat), "emissiveIntensity", 0.04, 0.55);
        let metal_mat = self.mat(
            Material::standard()
                .set("color", 0x8d9197)
                .set("metalness", 0.6)
                .set("roughness", 0.45),
        );
        let mut faces: Vec<BufferGeometry> = Vec::new();
        let mut metal: Vec<BufferGeometry> = Vec::new();
        let plate = |faces: &mut Vec<BufferGeometry>,
                     metal: &mut Vec<BufferGeometry>,
                     rect: &Rect,
                     w: f64,
                     h: f64,
                     x: f64,
                     y: f64,
                     z: f64,
                     yaw: f64| {
            let mut g = plane_geometry(w, h, 1.0, 1.0);
            let uv = g.get_attribute_mut("uv").expect("uv");
            for k in 0..uv.count() {
                let (u, v) = (uv.get_x(k), uv.get_y(k));
                uv.set_xy(k, lerp(rect.u0, rect.u1, u), lerp(rect.v0, rect.v1, v));
            }
            faces.push(placed(&g, x, y, z, yaw));
            let mut back = if rect.diamond {
                let mut b = plane_geometry(w * 0.69, h * 0.69, 1.0, 1.0);
                b.rotate_z(PI / 4.0);
                b
            } else {
                plane_geometry(w, h, 1.0, 1.0)
            };
            back.rotate_y(PI);
            back.translate(0.0, 0.0, -0.02);
            metal.push(placed(&back, x, y, z, yaw));
        };
        for sg in &signs {
            if sg.s < 5.0 || sg.s > z_end {
                continue;
            }
            let f = t.frame(sg.s);
            let i = t.idx(sg.s);
            let wall = if sg.side > 0.0 {
                at(&t.wall_r, i)
            } else {
                at(&t.wall_l, i)
            };
            let kind = if sg.side > 0.0 {
                self.side_r[i]
            } else {
                self.side_l[i]
            };
            let lat = sg.side * (wall + if kind == 1 { 0.75 } else { 1.1 } + sg.w * 0.5);
            let x = f.x + f.rx * lat;
            let z = f.z + f.rz * lat;
            let road_y = f.y - lat * f.bank;
            let hh = self.surf.sample(x, z);
            let base = clamp(hh.h, road_y - 4.0, road_y + 0.6);
            let yaw = kernel::atan2(-f.fx, -f.fz) + sg.side * 0.12;
            let top = road_y + sg.height + sg.h;
            let bottom = road_y + sg.height;
            if let Some((r, ew, eh)) = &sg.extra {
                plate(
                    &mut faces,
                    &mut metal,
                    r,
                    *ew,
                    *eh,
                    x,
                    bottom - 0.08 - eh / 2.0,
                    z,
                    yaw,
                );
            }
            plate(
                &mut faces,
                &mut metal,
                &sg.rect,
                sg.w,
                sg.h,
                x,
                road_y + sg.height + sg.h / 2.0,
                z,
                yaw,
            );
            let post_h = top - 0.1 - base;
            let mut post = cylinder_geometry(0.045, 0.045, post_h, 6.0, 1.0, false, 0.0, PI * 2.0);
            post.translate(0.0, base + post_h / 2.0, 0.0);
            let dx = -kernel::sin(yaw) * 0.05;
            let dz = -kernel::cos(yaw) * 0.05;
            if sg.w > 2.0 {
                let off = sg.w * 0.32;
                let px2 = kernel::cos(yaw) * off;
                let pz2 = -kernel::sin(yaw) * off;
                metal.push(placed(&post, x + dx + px2, 0.0, z + dz + pz2, yaw));
                metal.push(placed(&post, x + dx - px2, 0.0, z + dz - pz2, yaw));
            } else {
                metal.push(placed(&post, x + dx, 0.0, z + dz, yaw));
            }
        }
        self.add_merged(faces, face_mat, false, true);
        self.add_merged(metal, metal_mat, true, true);
    }

    // ── Wooden utility poles on the land side, with sagging wires ───────

    fn build_poles(&mut self) {
        let t = self.t;
        let mut poles = Vec::new();
        let mut wires: Vec<f64> = Vec::new();
        let mut prev: Option<[(f64, f64, f64); 2]> = None;
        let mut s = 90.0;
        while s < self.z_end + 60.0 {
            let sf = t.frame(s);
            s += 46.0;
            let lat = sf.wall_r + 3.2;
            let x = sf.x + sf.rx * lat;
            let z = sf.z + sf.rz * lat;
            let road_y = sf.y - lat * sf.bank;
            let g = self.surf.sample(x, z).h;
            if g > road_y + 5.0
                || g < road_y - 6.0
                || !self.clear_of_road(x, z, 0.3)
                || self.excluded(x, z, 0.0)
            {
                prev = None;
                continue;
            }
            let yaw = kernel::atan2(sf.fx, sf.fz);
            poles.push(Item {
                ry: yaw,
                ..Item::at(x, g - 0.3, z, 1.0, 1.0, 1.0)
            });
            let top = g - 0.3 + 9.4;
            let ends =
                [-0.95, 0.95].map(|o| (x + kernel::cos(yaw) * o, top, z - kernel::sin(yaw) * o));
            if let Some(pv) = prev {
                for k in 0..2 {
                    let a = pv[k];
                    let b = ends[k];
                    let span = kernel::hypot(b.0 - a.0, b.2 - a.2);
                    let sag = span * 0.018;
                    for j in 0..8 {
                        let f0 = f64::from(j) / 8.0;
                        let f1 = f64::from(j + 1) / 8.0;
                        let y0 = lerp(a.1, b.1, f0) - kernel::sin(f0 * PI) * sag;
                        let y1 = lerp(a.1, b.1, f1) - kernel::sin(f1 * PI) * sag;
                        wires.extend_from_slice(&[
                            lerp(a.0, b.0, f0),
                            y0,
                            lerp(a.2, b.2, f0),
                            lerp(a.0, b.0, f1),
                            y1,
                            lerp(a.2, b.2, f1),
                        ]);
                    }
                }
            }
            prev = Some(ends);
        }
        if poles.is_empty() {
            return;
        }
        let mut pole = cyl(0.12, 0.17, 10.0, 6.0, false);
        pole.translate(0.0, 5.0, 0.0);
        let mut arm = boxg(2.3, 0.12, 0.12);
        arm.translate(0.0, 9.4, 0.0);
        let mut a = pole.to_non_indexed();
        let mut b = arm.to_non_indexed();
        a.delete_attribute("uv");
        b.delete_attribute("uv");
        let geo = self.geo(merge(&[a, b]));
        let mat = self.mat(
            Material::standard()
                .set("color", 0x5a4432)
                .set("roughness", 0.9),
        );
        self.add_instanced(geo, mat, &poles, true, true);
        let mut lg = BufferGeometry::new();
        lg.set_attribute("position", BufferAttribute::from_f64(&wires, 3));
        let lg = self.geo(lg);
        let lm = self.mat(Material::line_basic().set("color", 0x1b1d20));
        let lines = self.graph.drawable(NodeType::LineSegments, lg, lm);
        self.graph.get_mut(lines).matrix_auto_update = false;
        self.add(lines);
    }

    // ── Delineator posts along both verges, reflectors glinting at dawn,
    // and yellow call boxes now and then ──────────────────────────────────

    fn build_delineators(&mut self) {
        let t = self.t;
        let mut posts = Vec::new();
        let mut refl = Vec::new();
        let mut boxes = Vec::new();
        let mut s = 40.0;
        while s < self.z_end + 40.0 {
            let sf = t.frame(s);
            for side in [-1.0, 1.0] {
                let wall = if side > 0.0 { sf.wall_r } else { sf.wall_l };
                let lat = side * (wall + 0.45);
                let x = sf.x + sf.rx * lat;
                let z = sf.z + sf.rz * lat;
                if self.excluded(x, z, 0.0) {
                    continue;
                }
                let y = sf.y - lat * sf.bank;
                let yaw = kernel::atan2(sf.fx, sf.fz);
                posts.push(Item {
                    ry: yaw,
                    ..Item::at(x, y - 0.2, z, 1.0, 1.0, 1.0)
                });
                refl.push(Item {
                    ry: yaw,
                    col: Some(if side > 0.0 { 0xffffff } else { 0xffb020 }),
                    ..Item::at(x - sf.fx * 0.06, y + 0.85, z - sf.fz * 0.06, 1.0, 1.0, 1.0)
                });
            }
            if s % 760.0 < 38.0 && s > 300.0 {
                let lat = sf.wall_r + 1.3;
                boxes.push(Item {
                    ry: kernel::atan2(-sf.rx, -sf.rz),
                    ..Item::at(
                        sf.x + sf.rx * lat,
                        sf.y - lat * sf.bank - 0.1,
                        sf.z + sf.rz * lat,
                        1.0,
                        1.0,
                        1.0,
                    )
                });
            }
            s += 38.0;
        }
        if posts.is_empty() {
            return;
        }
        let mut pg = boxg(0.1, 1.3, 0.1);
        pg.translate(0.0, 0.65, 0.0);
        let mut band = boxg(0.105, 0.2, 0.105);
        band.translate(0.0, 1.1, 0.0);
        let post = merge(&[colorize(prep(pg), 0xf0f0ea), colorize(prep(band), 0x1a1a1a)]);
        let mut cb = boxg(0.5, 0.75, 0.35);
        cb.translate(0.0, 1.1, 0.0);
        let mut cbp = boxg(0.12, 1.1, 0.12);
        cbp.translate(0.0, 0.55, 0.0);
        let call_box = merge(&[colorize(prep(cb), 0xe0b52b), colorize(prep(cbp), 0x6a6a6a)]);
        let mat = self.mat(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.6),
        );
        let post = self.geo(post);
        self.add_instanced(post, mat, &posts, true, true);
        if !boxes.is_empty() {
            let cbg = self.geo(call_box);
            self.add_instanced(cbg, mat, &boxes, true, true);
        }
        // Reflectors: bright at blue hour (headlights would catch them).
        let rm = self.mat(Material::basic().set("color", 0xffffff));
        self.animators
            .push(Box::new(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
                let v = 0.6 + 1.6 * u.night;
                out.push(Edit {
                    target: Handle::Material(rm),
                    change: Change::Color {
                        prop: "color",
                        rgb: [v, v, v],
                    },
                });
            }));
        let rg = self.geo(boxg(0.07, 0.16, 0.02));
        self.add_instanced(rg, rm, &refl, false, false);
    }

    // ── Fishing boats offshore with running lights ──────────────────────

    fn build_boats(&mut self) {
        let t = self.t;
        let terrain = self.terrain;
        let mut rng = Mulberry32::new(88);
        let mut boats: Vec<Boat> = Vec::new();
        const COLS: [u32; 4] = [0xe8e4da, 0x2f5f8a, 0xb23a2e, 0xe8e4da];
        let mut k = 0;
        while k < 9 && boats.len() < 7 {
            let kf = f64::from(k);
            let s = lerp(200.0, self.z_end - 200.0, (kf + rng.next_f64() * 0.6) / 9.0);
            let f = t.frame(s);
            let off = lerp(420.0, 1500.0, rng.next_f64());
            let x = f.x - f.rx * off + f.fx * (rng.next_f64() - 0.5) * 200.0;
            let z = f.z - f.rz * off + f.fz * (rng.next_f64() - 0.5) * 200.0;
            let col = COLS[(k % 4) as usize];
            k += 1;
            if terrain.height_at(x, z) > self.sea_y - 6.0 {
                continue;
            }
            let yaw = rng.next_f64() * TURN_BOAT;
            let ph = rng.next_f64() * TURN_BOAT;
            boats.push(Boat { x, z, yaw, ph, col });
        }
        if boats.is_empty() {
            return;
        }
        // Hull (pointed bow), cabin, mast — one geometry with vertex colours.
        let mut shape = Shape::new();
        shape.move_to(-5.5, -1.6);
        shape.line_to(3.4, -1.8);
        shape.line_to(5.6, 0.0);
        shape.line_to(3.4, 1.8);
        shape.line_to(-5.5, 1.6);
        shape.close_path();
        let mut hull = extrude_geometry(&[shape], &ExtrudeOptions::flat(1.7));
        hull.rotate_x(-PI / 2.0);
        hull.translate(0.0, -0.9, 0.0);
        let mut parts = vec![colorize(prep(hull), 0xffffff)];
        let mut cabin = prep(boxg(3.2, 1.9, 2.5));
        cabin.translate(-1.2, 1.75, 0.0);
        parts.push(colorize(cabin, 0xe9e2d0));
        let mut mast = prep(cyl(0.07, 0.09, 6.0, 5.0, false));
        mast.translate(0.8, 3.8, 0.0);
        parts.push(colorize(mast, 0x555a60));
        let mut boom = prep(cyl(0.05, 0.05, 5.0, 5.0, false));
        boom.rotate_z(1.0);
        boom.translate(2.6, 3.2, 0.0);
        parts.push(colorize(boom, 0x555a60));
        let geo = self.geo(merge(&parts));
        let mat = self.mat(
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.6),
        );
        let im = self.graph.instanced_mesh(geo, mat, boats.len() as u32);
        {
            let inst = self
                .graph
                .get_mut(im)
                .instances
                .as_mut()
                .expect("instanced");
            for (k, b) in boats.iter().enumerate() {
                inst.set_color_at(k, Color::hex(b.col));
            }
        }
        self.graph.get_mut(im).frustum_culled = false;
        self.add(im);
        // Running lights as fixed-size points so they read at a kilometre.
        let nl = boats.len() * 3;
        let lpos = vec![0.0f32; nl * 3];
        let mut lcol = vec![0.0f32; nl * 3];
        const LCOL: [[f32; 3]; 3] = [[1.6, 1.5, 1.3], [1.8, 0.2, 0.15], [0.2, 1.7, 0.5]];
        for k in 0..nl {
            lcol[k * 3..k * 3 + 3].copy_from_slice(&LCOL[k % 3]);
        }
        let mut lg = BufferGeometry::new();
        lg.set_attribute("position", BufferAttribute::from_f32(lpos, 3));
        lg.set_attribute("color", BufferAttribute::from_f32(lcol, 3));
        let glow = self
            .graph
            .cached_texture(&self.textures.glow_texture(), Layer::Main, "");
        let lmat = self.mat(
            Material::points()
                .set("size", 5.0)
                .set("sizeAttenuation", false)
                .set("map", glow)
                .set("vertexColors", true)
                .set("transparent", true)
                .set("depthWrite", false)
                .set("blending", ADDITIVE),
        );
        let lg = self.geo(lg);
        let pts = self.graph.drawable(NodeType::Points, lg, lmat);
        self.graph.get_mut(pts).frustum_culled = false;
        self.add(pts);
        let sea_y = self.sea_y;
        let mut anim = Boats {
            boats,
            im,
            lg,
            lmat,
            sea_y,
            time: 0.0,
        };
        // `update(0, 0.8)` at build: the poses written into the scene.
        let mut edits = Vec::new();
        anim.update(
            &UpdateCtx {
                dt: 0.0,
                night: 0.8,
                camera: None,
                s: 0.0,
            },
            &mut edits,
        );
        for e in edits {
            match e.change {
                Change::InstanceMatrix { index, matrix } => {
                    let inst = self
                        .graph
                        .get_mut(im)
                        .instances
                        .as_mut()
                        .expect("instanced");
                    inst.matrices[index as usize * 16..][..16].copy_from_slice(&matrix);
                }
                Change::Attribute {
                    name,
                    offset,
                    values,
                } => {
                    let a = self
                        .graph
                        .geometry_mut(lg)
                        .get_attribute_mut(name)
                        .expect("an attribute");
                    for (i, v) in values.into_iter().enumerate() {
                        a.set_raw(offset + i, f64::from(v));
                    }
                }
                Change::Number { prop, value } => {
                    self.graph
                        .material_mut(lmat)
                        .set_value(prop, crate::material::Param::Num(value));
                }
                _ => {}
            }
        }
        self.animators.push(Box::new(anim));
    }
}

/// A fishing boat offshore.
#[derive(Clone, Copy, Debug)]
struct Boat {
    x: f64,
    z: f64,
    yaw: f64,
    ph: f64,
    col: u32,
}

/// The boats' updater: each boat bobs and rolls on the swell, its three
/// running lights carried with it.
struct Boats {
    boats: Vec<Boat>,
    im: NodeId,
    lg: GeoId,
    lmat: MaterialId,
    sea_y: f64,
    time: f64,
}

/// The running lights' positions in a boat's frame.
const LOCAL: [[f64; 3]; 3] = [[0.8, 6.9, 0.0], [-1.2, 2.4, -1.3], [-1.2, 2.4, 1.3]];

impl Animator for Boats {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        self.time += u.dt;
        let time = self.time;
        let mut lpos = vec![0.0f32; self.boats.len() * 9];
        for (k, b) in self.boats.iter().enumerate() {
            let y = self.sea_y + kernel::sin(time * 0.8 + b.ph) * 0.22;
            let e = Euler::new(
                kernel::sin(time * 0.6 + b.ph) * 0.05,
                b.yaw + kernel::sin(time * 0.1 + b.ph) * 0.05,
                kernel::sin(time * 0.7 + b.ph * 2.0) * 0.03,
            );
            let q = Quaternion::from_euler(&e);
            let m4 = Matrix4::compose(Vector3::new(b.x, y, b.z), q, Vector3::new(1.0, 1.0, 1.0));
            out.push(Edit {
                target: Handle::Node(self.im),
                change: Change::InstanceMatrix {
                    index: k as u32,
                    matrix: m4.elements.map(|v| v as f32),
                },
            });
            for j in 0..3 {
                let v = Vector3::new(LOCAL[j][0], LOCAL[j][1], LOCAL[j][2]).apply_matrix4(&m4);
                let o = (k * 3 + j) * 3;
                lpos[o] = v.x as f32;
                lpos[o + 1] = v.y as f32;
                lpos[o + 2] = v.z as f32;
            }
        }
        out.push(Edit {
            target: Handle::Geometry(self.lg),
            change: Change::Attribute {
                name: "position",
                offset: 0,
                values: lpos,
            },
        });
        out.push(Edit {
            target: Handle::Material(self.lmat),
            change: Change::Number {
                prop: "opacity",
                value: 0.35 + 0.65 * u.night,
            },
        });
    }
}

// ── Canvas pictures ─────────────────────────────────────────────────────

/// `bannerTex(front)`'s canvas: front says HOLLOW POINT, back (seen when
/// looking back) START.
pub fn banner_canvas(front: bool) -> Canvas {
    let mut g = Canvas::new(1024, 160);
    g.set_fill_style("#0a1016");
    g.fill_rect(0.0, 0.0, 1024.0, 160.0);
    let sq = 20.0;
    let mut y = 0.0;
    while y < 160.0 {
        let mut x = 0.0;
        while x < 120.0 {
            g.set_fill_style(if ((x + y) / sq) % 2.0 != 0.0 {
                "#f2f2f2"
            } else {
                "#111"
            });
            g.fill_rect(x, y, sq, sq);
            g.fill_rect(1024.0 - 120.0 + x, y, sq, sq);
            x += sq;
        }
        y += sq;
    }
    let mut grd = g.create_linear_gradient(120.0, 0.0, 904.0, 0.0);
    grd.add_color_stop(0.0, "#2ec4b6");
    grd.add_color_stop(1.0, "#ff9a3c");
    g.set_fill_style(&grd);
    g.fill_rect(120.0, 0.0, 784.0, 8.0);
    g.fill_rect(120.0, 152.0, 784.0, 8.0);
    g.set_fill_style("#fff");
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_font(&format!("italic 900 84px {FONT}"));
    g.fill_text(if front { "HOLLOW POINT" } else { "START" }, 512.0, 70.0);
    g.set_font(&format!("bold 26px {FONT}"));
    g.set_fill_style("#7fe3d8");
    g.fill_text(
        if front {
            "STAGE 2 · COAST HIGHWAY"
        } else {
            "HOLLOW POINT"
        },
        512.0,
        132.0,
    );
    g
}

/// The coffee shack's sign over the window.
pub fn coffee_sign_canvas() -> Canvas {
    let mut g = Canvas::new(512, 128);
    g.set_fill_style("#0f1a1c");
    round_rect(&mut g, 4.0, 4.0, 504.0, 120.0, 16.0);
    g.fill();
    g.set_stroke_style("#2ec4b6");
    g.set_line_width(5.0);
    round_rect(&mut g, 12.0, 12.0, 488.0, 104.0, 12.0);
    g.stroke();
    g.set_fill_style("#ffe2b8");
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_font(&format!("italic 900 54px {FONT}"));
    g.fill_text("HOLLOW PT. COFFEE", 256.0, 56.0);
    g.set_font(&format!("bold 24px {FONT}"));
    g.set_fill_style("#7fe3d8");
    g.fill_text("SURF · COFFEE · BAIT", 256.0, 100.0);
    g
}
