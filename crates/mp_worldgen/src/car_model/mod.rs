//! `src/vehicles/CarModel.js` (roadmap WP 4.1): procedural vehicles, no
//! assets, as engine-neutral scene data.
//!
//! Bodies are lofts: at stations along the car we take the side profile's
//! roof-line and sill-line heights (wheel arches cut into the sill) and
//! sweep a rounded cross-section between them whose half-width varies with
//! height and length (plan-view corner rounding, tumblehome, hips, a
//! character-line ridge, flared arches with a lip). Each face is assigned a
//! material by where it sits (paint, glass, black trim, stripes), which is
//! how windows, pillars and racing stripes come out of the same loft.
//!
//! Everything that sits ON a panel — light units, grilles, vents, panel
//! gaps, window surrounds — is a decal: an outline drawn in a 2D view (top,
//! side, front or rear) and projected onto the loft surface, lifted a few
//! mm along the surface normal. That keeps lamps and shut lines hugging the
//! curved body instead of being boxes stuck through it.
//!
//! Parts are gathered into per-material buckets and merged; the merged
//! geometry is cached per kind/LOD/variant so traffic shares it. Each
//! instance owns only its paint (racers) and its light materials, so
//! brake/head lights work per car. On the high LOD, vertex colours carry
//! detail inside a bucket (lens housings vs lit LED strips, seat leather,
//! soot in exhaust tips); the light materials multiply their emission by
//! it, so one bucket can hold a dim lens and a blazing LED line. The low
//! LOD has no vertex colours (parked cars get re-merged by material in the
//! scenery, which drops them).
//!
//! Frame: origin on the ground centred between the axles, +Z forward, +Y
//! up, right = -X. Left-side wheels sit at +X.
//!
//! The port keeps the JS structure: [`build_vehicle`] is `buildVehicle`,
//! building the model's object tree into a [`SceneGraph`] and returning a
//! [`VehicleModel`], the JS handle: the tree's nodes (root, body, wheels,
//! steer pivots, headlight and siren anchors, the far model), `dims`, the
//! exhausts, the per-instance materials, and the light setters, which
//! return their changes as [`Edit`]s for whoever draws the scene (and
//! [`apply_edits`] applies them to the graph). The JS module's caches
//! (shared materials, stripe and low-detail paints, wheel, caliper, part
//! and far geometry) are a [`CarKit`] kept on the graph
//! ([`SceneGraph::cars`]), since the handles they hold belong to it
//! (DECISIONS D410).

// Index loops stay index loops, and the JS signatures stay (D52, D130).
// `!(l > eps)` is the JS test, which a NaN fails too.
#![allow(
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::neg_cmp_op_on_partial_ord
)]

mod detail;
mod far;
mod kit;
mod specs;
mod wheels;

use std::sync::Arc;

use mp_canvas::Canvas;
use mp_scene::{MaterialKind, NodeType, three};
use serde_json::Value;

use crate::color::Color;
use crate::material::{Material, Param, color_value, num, texture_value};
use crate::object::{GeoId, Image, Layer, MaterialId, NodeId, Object3D, SceneGraph, TextureId};
use crate::textures::{Texture, TextureCache};
use crate::three_geom::{BufferAttribute, BufferGeometry, Euler, Vector3};
use crate::world::{Change, Edit, Handle};

pub use detail::{GlowSpot, SirenLayout};
pub use kit::P3;
pub use wheels::{FrontWheels, RimMat, WheelLayout, WheelType};

use kit::{PI, Parts};
use specs::spec;

/// `VEHICLE_KINDS`: `Object.keys(SPECS)`.
pub const VEHICLE_KINDS: [&str; 13] = [
    "sports",
    "muscle",
    "super",
    "electric",
    "rally",
    "sedan",
    "hatch",
    "van",
    "pickup",
    "boxtruck",
    "tractor",
    "police",
    "policeSuv",
];

/// `RACERS`.
const RACERS: [&str; 5] = ["sports", "muscle", "super", "rally", "electric"];
/// `TRAFFIC_COLORS`.
const TRAFFIC_COLORS: [u32; 10] = [
    0xb8bcc2, 0x2b2f36, 0xe8e6e0, 0x7a1f1f, 0x1f3a5f, 0x4a5a3a, 0x8c7a5a, 0x5f6670, 0x9a9a92,
    0x243048,
];
/// `POLICE_KINDS`.
const POLICE_KINDS: [&str; 2] = ["police", "policeSuv"];
const POLICE_BLACK: u32 = 0x0c0d10;
const POLICE_WHITE: u32 = 0xf4f3ee;

/// A kind's dimensions (`layout.dims`): what physics and collisions read
/// (`mp_sim::dims` holds the same table, SPEC 4.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dims {
    pub length: f64,
    pub width: f64,
    pub height: f64,
    pub wheel_radius: f64,
    pub wheel_base: f64,
    pub track: f64,
}

/// What a kind's spec returns: `{ dims, wheels, head, exhausts, siren }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub dims: Dims,
    pub wheels: WheelLayout,
    pub head: P3,
    pub exhausts: Vec<P3>,
    pub siren: Option<SirenLayout>,
}

/// The variant the part cache keys on: `{ stripes, spoiler, livery }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variant {
    pub stripes: bool,
    pub spoiler: bool,
    /// `livery === 'police'`.
    pub police: bool,
}

/// The level of detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lod {
    High,
    Low,
}

/// `buildVehicle`'s options. `None` is a property the JS object leaves
/// out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BuildOpts {
    /// `lod`: racers default to high, the rest to low.
    pub lod: Option<Lod>,
    /// `far`: also build the far model (`setFar`).
    pub far: bool,
    /// `color` (hex).
    pub color: Option<u32>,
    /// `seed` (`Math.abs(seed ?? 0)`; the game passes small whole numbers).
    pub seed: u32,
    /// `livery: 'police'` (only the muscle and sports cars take it).
    pub police_livery: bool,
    pub stripes: Option<bool>,
    pub spoiler: Option<bool>,
    /// `stripeColor` (hex).
    pub stripe_color: Option<u32>,
    /// `rim: 'dark'`.
    pub rim_dark: bool,
}

/// One set of shared materials (`mk(vc)`).
#[derive(Clone, Copy, Debug)]
struct MatSet {
    glass: MaterialId,
    trim: MaterialId,
    chrome: MaterialId,
    plate: MaterialId,
    tire: MaterialId,
    rim: MaterialId,
    rim_dark: MaterialId,
    rim_steel: MaterialId,
    rim_tractor: MaterialId,
    rim_white: MaterialId,
    rim_aero: MaterialId,
    rim_chrome: MaterialId,
    #[allow(dead_code)]
    rim_gold: MaterialId,
    brake: MaterialId,
    cargo: MaterialId,
    seat: MaterialId,
    carbon: MaterialId,
}

/// `SHARED`: the 'hi' materials read vertex colours, the 'lo' ones don't.
#[derive(Clone, Copy, Debug)]
struct Shared {
    hi: MatSet,
    lo: MatSet,
    caliper: [MaterialId; 3],
}

/// The cached result of `getParts(kind, lod, variant)`: the merged buckets
/// and the layout (with the siren's glow geometry once made).
#[derive(Clone, Debug)]
struct PartsEntry {
    geoms: Vec<(&'static str, GeoId)>,
    layout: Layout,
    siren_geo: Option<GeoId>,
}

/// The module state of `CarModel.js`: `SHARED`, `stripeMats`, `lowPaints`,
/// `wheelCache`, `caliperCache`, `partsCache`, `farCache` and
/// `farMaterial`, as handles into one [`SceneGraph`] (the one it is kept
/// on, [`SceneGraph::cars`]).
#[derive(Clone, Debug, Default)]
pub struct CarKit {
    shared: Option<Shared>,
    stripe_mats: Vec<(String, MaterialId)>,
    low_paints: Vec<(String, MaterialId)>,
    wheels: Vec<(String, GeoId)>,
    calipers: Vec<(String, GeoId)>,
    parts: Vec<(String, PartsEntry)>,
    far: Vec<(String, GeoId)>,
    far_material: Option<MaterialId>,
}

fn find<T: Copy>(list: &[(String, T)], key: &str) -> Option<T> {
    list.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
}

/// `new THREE.MeshStandardMaterial({ color, metalness, roughness,
/// vertexColors })`.
fn standard(color: u32, metalness: f64, roughness: f64, vc: bool) -> Material {
    Material::standard()
        .set("color", color)
        .set("metalness", metalness)
        .set("roughness", roughness)
        .set("vertexColors", vc)
}

/// `carbonTexture()`: a 2×2 twill carbon weave: tows alternate direction
/// on a diagonal staircase, each shaded across its width so the weave
/// catches the light (`canvasTex(64, ...)`: repeat, sRGB, anisotropy 4).
pub fn carbon_texture() -> Texture {
    let s = 64.0;
    let mut g = Canvas::new(64, 64);
    let n = 8;
    let c = s / n as f64;
    for i in 0..n {
        for j in 0..n {
            let horiz = ((i + j) & 3) < 2;
            let (fi, fj) = (i as f64, j as f64);
            let mut grd = if horiz {
                g.create_linear_gradient(0.0, fj * c, 0.0, (fj + 1.0) * c)
            } else {
                g.create_linear_gradient(fi * c, 0.0, (fi + 1.0) * c, 0.0)
            };
            let b: i32 = if horiz { 44 } else { 26 };
            grd.add_color_stop(0.0, &format!("rgb({},{},{})", b - 16, b - 16, b - 13));
            grd.add_color_stop(0.5, &format!("rgb({},{},{})", b + 16, b + 16, b + 20));
            grd.add_color_stop(1.0, &format!("rgb({},{},{})", b - 16, b - 16, b - 13));
            g.set_fill_style(&grd);
            g.fill_rect(fi * c, fj * c, c, c);
        }
    }
    Texture::from_canvas(&g, true, true, 4.0)
}

impl CarKit {
    /// `shared()`.
    fn shared(&mut self, graph: &mut SceneGraph) -> Shared {
        if let Some(s) = self.shared {
            return s;
        }
        let mut mk = |vc: bool| -> MatSet {
            let glass = if vc {
                Material::physical()
                    .set("color", 0x0a0e14)
                    .set("metalness", 0.15)
                    .set("roughness", 0.04)
                    .set("clearcoat", 1.0)
                    .set("clearcoatRoughness", 0.02)
                    .set("transparent", true)
                    .set("opacity", 0.78)
                    .set("vertexColors", true)
            } else {
                Material::standard()
                    .set("color", 0x151c26)
                    .set("metalness", 0.55)
                    .set("roughness", 0.16)
            };
            let mut add = |m: Material| graph.add_material(m);
            MatSet {
                glass: add(glass),
                trim: add(standard(0x111214, 0.25, 0.55, vc)),
                chrome: add(standard(0x9ea2a8, 1.0, 0.2, vc)),
                plate: add(standard(0xe9e7df, 0.0, 0.6, vc)),
                tire: add(standard(0x161616, 0.0, 0.9, vc)),
                rim: add(standard(0xc4c8ce, 0.95, 0.22, vc)),
                rim_dark: add(standard(0x2c2e33, 0.85, 0.3, vc)),
                rim_steel: add(standard(0x9a9da3, 0.7, 0.38, vc)),
                rim_tractor: add(standard(0xbfa35e, 0.1, 0.75, vc)),
                rim_white: add(standard(0xe8e8e4, 0.3, 0.35, vc)),
                rim_aero: add(standard(0x9aa0a8, 0.9, 0.2, vc)),
                rim_chrome: add(standard(0xc4c8cc, 1.0, 0.15, vc)),
                rim_gold: add(standard(0xc9a14a, 0.9, 0.28, vc)),
                // Brake discs, barrel insides and backing plates share one
                // metal material; vertex colour sets how bright each part
                // is.
                brake: add(standard(0x8a8d92, 0.8, 0.42, vc)),
                cargo: add(standard(0xe4e2dc, 0.1, 0.55, vc)),
                seat: add(standard(0x1c1a18, 0.0, 0.9, vc)),
                carbon: MaterialId(u32::MAX),
            }
        };
        let mut hi = mk(true);
        let mut lo = mk(false);
        let t = carbon_texture();
        let desc = t.desc("", 0);
        let tex: TextureId = graph.add_texture(Image::Own(Arc::new(t)), desc);
        hi.carbon = graph.add_material(
            Material::physical()
                .set("color", 0x8c8c8c)
                .set("map", tex)
                .set("metalness", 0.3)
                .set("roughness", 0.5)
                .set("clearcoat", 0.7)
                .set("clearcoatRoughness", 0.12),
        );
        lo.carbon = graph.add_material(standard(0x202124, 0.3, 0.45, false));
        let caliper = [0xc41d1d, 0xe8b400, 0x1d5fc4]
            .map(|color| graph.add_material(standard(color, 0.3, 0.4, false)));
        let s = Shared { hi, lo, caliper };
        self.shared = Some(s);
        s
    }

    /// `stripeMat(color, hi)`.
    fn stripe_mat(&mut self, graph: &mut SceneGraph, color: u32, hi: bool) -> MaterialId {
        let key = format!("{color}{}", if hi { 'h' } else { 'l' });
        if let Some(m) = find(&self.stripe_mats, &key) {
            return m;
        }
        let m = graph.add_material(if hi {
            Material::physical()
                .set("color", color)
                .set("metalness", 0.3)
                .set("roughness", 0.35)
                .set("clearcoat", 1.0)
                .set("clearcoatRoughness", 0.05)
                .set("vertexColors", true)
        } else {
            standard(color, 0.2, 0.5, false)
        });
        self.stripe_mats.push((key, m));
        m
    }

    /// `lowPaint(color, rough = 0.5)`.
    fn low_paint(&mut self, graph: &mut SceneGraph, color: u32, rough: f64) -> MaterialId {
        let key = format!("{color}:{rough}");
        if let Some(m) = find(&self.low_paints, &key) {
            return m;
        }
        let m = graph.add_material(standard(color, 0.3, rough, false));
        self.low_paints.push((key, m));
        m
    }

    /// `wheelGeometry(r, w, lod, style)`, cached.
    fn wheel(
        &mut self,
        graph: &mut SceneGraph,
        r: f64,
        w: f64,
        hi: bool,
        style: &WheelLayout,
    ) -> GeoId {
        let key = wheels::wheel_key(r, w, hi, style);
        if let Some(g) = find(&self.wheels, &key) {
            return g;
        }
        let g = graph.add_geometry(wheels::wheel_geometry(r, w, hi, style));
        self.wheels.push((key, g));
        g
    }

    /// `caliperGeom(rimR)`, cached by `rimR.toFixed(3)`.
    fn caliper(&mut self, graph: &mut SceneGraph, rim_r: f64) -> GeoId {
        let key = wheels::caliper_key(rim_r);
        if let Some(g) = find(&self.calipers, &key) {
            return g;
        }
        let g = graph.add_geometry(wheels::caliper_geometry(rim_r));
        self.calipers.push((key, g));
        g
    }

    /// `getParts(kind, lod, variant)`, cached.
    fn parts(&mut self, graph: &mut SceneGraph, kind: &str, hi: bool, v: Variant) -> usize {
        let key = format!("{kind}|{}|{v:?}", if hi { "high" } else { "low" });
        if let Some(i) = self.parts.iter().position(|(k, _)| *k == key) {
            return i;
        }
        let mut p = Parts::new(hi);
        let layout = spec(kind).expect("a known kind")(&mut p, hi, v);
        let geoms = p
            .build()
            .into_iter()
            .map(|(b, g)| (b, graph.add_geometry(g)))
            .collect();
        self.parts.push((
            key,
            PartsEntry {
                geoms,
                layout,
                siren_geo: None,
            },
        ));
        self.parts.len() - 1
    }
}

/// `lightMat(params, hi)`: a light material whose emission is scaled by
/// the vertex colour (and whose albedo ignores it), so lens housings stay
/// dim while LED strips blaze.
fn light_mat(
    color: u32,
    metalness: Option<f64>,
    emissive: u32,
    intensity: f64,
    roughness: f64,
    hi: bool,
) -> Material {
    let mut m = Material::standard().set("color", color);
    if let Some(v) = metalness {
        m = m.set("metalness", v);
    }
    m = m
        .set("emissive", emissive)
        .set("emissiveIntensity", intensity)
        .set("roughness", roughness)
        .set("vertexColors", hi);
    if hi {
        // MaterialKind for the scene export.
        m = m
            .kind(MaterialKind::CarLight, None)
            .program_key("car-light-vc")
            .uniform("clippingPlanes", Value::Null);
    }
    m
}

/// The vertex shader of the siren glow (`glowMaterial`), as the JS writes
/// it.
const GLOW_VERT: &str = "
      attribute vec2 corner;
      attribute float aBlue;
      attribute float aSize;
      uniform float uMin;
      varying vec2 vUv;
      varying float vBlue;
      varying float vFar;
      void main() {
        vUv = corner * 0.5 + 0.5;
        vBlue = aBlue;
        vec4 mv = modelViewMatrix * vec4(position, 1.0);
        float d = length(mv.xyz);
        mv.xyz *= max(0.05, d - min(1.0 + d * d * 0.0015, d * 0.5)) / d;
        vec4 clip = projectionMatrix * mv;
        vec2 hs = vec2(projectionMatrix[0][0], projectionMatrix[1][1]) * (0.5 * aSize) / clip.w;
        float grow = max(1.0, uMin / hs.y);
        vFar = clamp((grow - 1.0) / 1.5, 0.0, 1.0);
        clip.xy += corner * hs * grow * clip.w;
        gl_Position = clip;
      }";

/// The fragment shader of the siren glow.
const GLOW_FRAG: &str = "
      uniform sampler2D map;
      uniform vec3 uRed;
      uniform vec3 uBlue;
      varying vec2 vUv;
      varying float vBlue;
      varying float vFar;
      void main() {
        // Held at its minimum size far away: a harder, brighter core.
        float a = pow(texture2D(map, vUv).a, 2.0 - vFar);
        gl_FragColor = vec4(mix(uRed, uBlue, vBlue) * a * (1.0 + 1.5 * vFar), 1.0);
        #include <tonemapping_fragment>
        #include <colorspace_fragment>
      }";

/// `glowGeometry(spots)`: additive glow billboards (one quad per spot, one
/// draw call per car). The vertex shader turns each quad to the camera,
/// pulls it towards the camera so the car's own roof doesn't cut it
/// (further when far away, where the road would otherwise hide its lower
/// half at grazing angles), and never lets it shrink below a minimum
/// screen size, so a unit still reads as police a few hundred metres away.
fn glow_geometry(spots: &[GlowSpot]) -> BufferGeometry {
    let mut pos = Vec::new();
    let mut corner = Vec::new();
    let mut blue = Vec::new();
    let mut size = Vec::new();
    let mut idx = Vec::new();
    for (i, s) in spots.iter().enumerate() {
        for c in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            pos.extend(s.p);
            corner.extend(c);
            blue.push(s.blue);
            size.push(s.size);
        }
        let i = i as u32;
        idx.extend([i * 4, i * 4 + 1, i * 4 + 2, i * 4, i * 4 + 2, i * 4 + 3]);
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("corner", BufferAttribute::from_f64(&corner, 2));
    g.set_attribute("aBlue", BufferAttribute::from_f64(&blue, 1));
    g.set_attribute("aSize", BufferAttribute::from_f64(&size, 1));
    g.set_index(&idx);
    g.compute_bounding_sphere();
    let grow = mp_math::js::max_n(&spots.iter().map(|s| s.size).collect::<Vec<_>>());
    if let Some(bs) = &mut g.bounding_sphere {
        bs.radius += grow;
    }
    g
}

/// `glowMaterial()`.
fn glow_material(graph: &mut SceneGraph, textures: &mut TextureCache) -> MaterialId {
    let map = graph.cached_texture(&textures.glow_texture(), Layer::Main, "");
    let black = color_value(Color::new(0.0, 0.0, 0.0));
    graph.add_material(
        Material::shader()
            // MaterialKind for the scene export.
            .kind(MaterialKind::PoliceGlow, None)
            .shader_source(GLOW_VERT, GLOW_FRAG)
            .uniform("map", texture_value(map))
            .uniform("uRed", black.clone())
            .uniform("uBlue", black)
            .uniform("uMin", num(0.02))
            .set("transparent", true)
            .set("depthWrite", false)
            .set("blending", three::ADDITIVE_BLENDING as f64),
    )
}

/// A siren mode for `setSiren(mode, t)`: 'off' | 'flash' | 'steady' |
/// 'disabled' (true/false mean flash/off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SirenMode {
    Off,
    Flash,
    Steady,
    Disabled,
}

/// `sirenLevels(mode, t)`: red/blue levels (0..1) for a siren mode at time
/// t (s). 'flash' is an alternating quad-flash: a 0.8 s cycle, red bursting
/// four times in the first half and blue in the second (2.5 bursts a
/// second).
pub fn siren_levels(mode: SirenMode, t: f64) -> (f64, f64) {
    match mode {
        SirenMode::Flash => {
            let ph = (((t * 1.25) % 1.0) + 1.0) % 1.0;
            let burst = if (ph * 8.0) % 1.0 < 0.55 { 1.0 } else { 0.0 };
            if ph < 0.5 { (burst, 0.0) } else { (0.0, burst) }
        }
        SirenMode::Steady => (0.55, 0.55),
        SirenMode::Disabled => (0.3, 0.0),
        SirenMode::Off => (0.0, 0.0),
    }
}

/// The siren's parts: the glow billboards, their material and the anchor
/// for the game's shared flash light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Siren {
    pub glow: NodeId,
    pub glow_material: MaterialId,
    pub anchor: NodeId,
    pub red: MaterialId,
    pub blue: MaterialId,
}

/// The far model: the mesh and everything it stands in for.
#[derive(Clone, Debug, PartialEq)]
pub struct Far {
    pub mesh: NodeId,
    pub near: Vec<NodeId>,
}

/// `buildVehicle`'s handle.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleModel {
    pub root: NodeId,
    pub body: NodeId,
    /// `[fl, fr, rl, rr]`; left = +X.
    pub wheels: Vec<NodeId>,
    pub steer_pivots: Vec<NodeId>,
    pub kind: String,
    pub dims: Dims,
    pub exhausts: Vec<Vector3>,
    pub headlight_anchor: NodeId,
    pub paint: MaterialId,
    pub head: MaterialId,
    pub tail: MaterialId,
    pub rev: MaterialId,
    /// The light accents (the electric car's blades), which flare under
    /// boost.
    pub accent: Option<MaterialId>,
    pub siren: Option<Siren>,
    pub far: Option<Far>,
    brake: f64,
    lights: f64,
    lr: f64,
    lb: f64,
    is_far: bool,
}

fn number(m: MaterialId, prop: &'static str, value: f64) -> Edit {
    Edit {
        target: Handle::Material(m),
        change: Change::Number { prop, value },
    }
}

fn visible(n: NodeId, v: bool) -> Edit {
    Edit {
        target: Handle::Node(n),
        change: Change::Visible(v),
    }
}

impl VehicleModel {
    /// `updateTail()`.
    fn update_tail(&self) -> Edit {
        let base = 0.3 + self.lights * 0.9;
        number(
            self.tail,
            "emissiveIntensity",
            base + (4.0 - base) * self.brake,
        )
    }

    /// `setBrake(v)`.
    pub fn set_brake(&mut self, v: f64) -> Vec<Edit> {
        self.brake = mp_math::js::min(1.0, mp_math::js::max(0.0, v));
        vec![self.update_tail()]
    }

    /// `setHeadlights(v)`.
    pub fn set_headlights(&mut self, v: f64) -> Vec<Edit> {
        self.lights = mp_math::js::min(1.0, mp_math::js::max(0.0, v));
        vec![
            number(self.head, "emissiveIntensity", 0.3 + self.lights * 2.7),
            self.update_tail(),
        ]
    }

    /// `setReverse(on)`.
    pub fn set_reverse(&mut self, on: bool) -> Vec<Edit> {
        vec![number(
            self.rev,
            "emissiveIntensity",
            if on { 2.5 } else { 0.0 },
        )]
    }

    /// `setBoost(v)`: the accents flare (nothing without accents).
    pub fn set_boost(&mut self, v: f64) -> Vec<Edit> {
        match self.accent {
            Some(a) => vec![number(
                a,
                "emissiveIntensity",
                1.4 + 5.0 * mp_math::js::min(1.0, mp_math::js::max(0.0, v)),
            )],
            None => Vec::new(),
        }
    }

    /// `setSiren(mode, t = 0)`: lens emission, glow billboards and their
    /// visibility (nothing on a car without a siren, where the JS handle
    /// has no `setSiren`).
    pub fn set_siren(&mut self, mode: SirenMode, t: f64) -> Vec<Edit> {
        let Some(s) = self.siren else {
            return Vec::new();
        };
        let (lr, lb) = siren_levels(mode, t);
        self.lr = lr;
        self.lb = lb;
        let color = |prop: &'static str, rgb: [f64; 3]| Edit {
            target: Handle::Material(s.glow_material),
            change: Change::Color { prop, rgb },
        };
        vec![
            number(s.red, "emissiveIntensity", lr * 6.0),
            number(s.blue, "emissiveIntensity", lb * 8.0),
            color("uRed", [2.4 * lr, 0.12 * lr, 0.05 * lr]),
            color("uBlue", [0.1 * lb, 0.35 * lb, 3.2 * lb]),
            visible(s.glow, lr + lb > 0.0),
        ]
    }

    /// `sirenColor()`: `{ r, b }`.
    pub fn siren_color(&self) -> (f64, f64) {
        (self.lr, self.lb)
    }

    /// `setFar(on)`: swap the far model in (nothing without one).
    pub fn set_far(&mut self, on: bool) -> Vec<Edit> {
        let Some(far) = &self.far else {
            return Vec::new();
        };
        if on == self.is_far {
            return Vec::new();
        }
        self.is_far = on;
        let mut out = vec![visible(far.mesh, on)];
        out.extend(far.near.iter().map(|&o| visible(o, !on)));
        out
    }

    /// `isFar`.
    pub fn is_far(&self) -> bool {
        self.is_far
    }
}

/// Applies a model's edits to the graph it was built in, as the JS objects
/// take them: a number to the uniform of that name if the material has one,
/// else to the parameter; a colour likewise; visibility to the node.
pub fn apply_edits(graph: &mut SceneGraph, edits: &[Edit]) {
    for e in edits {
        match (&e.target, &e.change) {
            (Handle::Material(m), Change::Number { prop, value }) => {
                let mat = graph.material_mut(*m);
                if let Some(u) = &mut mat.desc.uniforms
                    && let Some(slot) = u.get_mut(*prop)
                {
                    *slot = num(*value);
                } else {
                    mat.set_value(prop, Param::Num(*value));
                }
            }
            (Handle::Material(m), Change::Color { prop, rgb }) => {
                let c = Color::new(rgb[0], rgb[1], rgb[2]);
                let mat = graph.material_mut(*m);
                if let Some(u) = &mut mat.desc.uniforms
                    && let Some(slot) = u.get_mut(*prop)
                {
                    *slot = color_value(c);
                } else {
                    mat.set_value(prop, Param::Color(c));
                }
            }
            (Handle::Node(n), Change::Visible(v)) => graph.get_mut(*n).visible = *v,
            (t, c) => panic!("a car model makes no edit {t:?} {c:?}"),
        }
    }
}

fn object3d(graph: &mut SceneGraph, name: &str) -> NodeId {
    let mut o = Object3D::new(NodeType::Object3D);
    o.name = name.to_string();
    graph.object(o)
}

/// `buildVehicle(kind, opts)`: the model's object tree, built into
/// `graph` (not added to any parent: `root` is the JS `handle.root`).
/// `None` for an unknown kind (the JS throws).
pub fn build_vehicle(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    kind: &str,
    opts: &BuildOpts,
) -> Option<VehicleModel> {
    spec(kind)?;
    let mut kit = std::mem::take(&mut graph.cars);
    let m = kit.build_vehicle(graph, textures, kind, opts);
    graph.cars = kit;
    Some(m)
}

impl CarKit {
    fn build_vehicle(
        &mut self,
        graph: &mut SceneGraph,
        textures: &mut TextureCache,
        kind: &str,
        opts: &BuildOpts,
    ) -> VehicleModel {
        let sh = self.shared(graph);
        let racer = RACERS.contains(&kind);
        let hi = opts.lod.unwrap_or(if racer { Lod::High } else { Lod::Low }) == Lod::High;
        let mm = if hi { sh.hi } else { sh.lo };
        let seed = opts.seed;
        // Police: the patrol kinds, or a racer body in police livery
        // (interceptor).
        let livery = opts.police_livery && (kind == "muscle" || kind == "sports");
        let police = POLICE_KINDS.contains(&kind) || livery;
        let color = opts.color.unwrap_or(if police {
            POLICE_BLACK
        } else if kind == "tractor" {
            [0x9a3b2b, 0x3f6b3a, 0x8f7a3a][(seed % 3) as usize]
        } else {
            TRAFFIC_COLORS[(seed % TRAFFIC_COLORS.len() as u32) as usize]
        });
        let variant = Variant {
            stripes: opts
                .stripes
                .unwrap_or(!livery && (kind == "muscle" || kind == "rally")),
            spoiler: opts.spoiler.unwrap_or(!livery),
            police: livery,
        };
        let pi = self.parts(graph, kind, hi, variant);
        let geoms = self.parts[pi].1.geoms.clone();
        let layout = self.parts[pi].1.layout.clone();
        let has = |b: &str| geoms.iter().any(|(k, _)| *k == b);

        // Per-instance materials.
        let paint = if hi {
            // Paint finish per racer: metallic flake under clearcoat, a
            // solid gloss for the muscle car and rally car, a pearl sheen
            // on the supercar.
            let (metalness, roughness, sheen) = match if police { "police" } else { kind } {
                "sports" => (0.5, 0.36, None),
                "muscle" => (0.15, 0.28, None),
                "super" => (0.45, 0.3, Some(0.6)),
                "rally" => (0.1, 0.3, None),
                "electric" => (0.35, 0.36, None),
                "police" => (0.2, 0.3, None),
                _ => (0.4, 0.4, None),
            };
            let mut m = Material::physical()
                .set("color", color)
                .set("metalness", metalness)
                .set("roughness", roughness)
                .set("clearcoat", 1.0)
                .set("clearcoatRoughness", 0.09)
                .set("vertexColors", true);
            if let Some(sheen) = sheen {
                let mut c = Color::hex(color);
                c.lerp(Color::hex(0xffffff), 0.6);
                m = m
                    .set("sheen", sheen)
                    .set("sheenRoughness", 0.35)
                    .set("sheenColor", c);
            }
            graph.add_material(m)
        } else {
            self.low_paint(graph, color, if kind == "tractor" { 0.85 } else { 0.5 })
        };
        let head = graph.add_material(light_mat(
            if hi { 0x9aa0a8 } else { 0xd8d8d0 },
            Some(if hi { 0.85 } else { 0.0 }),
            0xfff1d6,
            0.3,
            if hi { 0.12 } else { 0.2 },
            hi,
        ));
        let tail = graph.add_material(light_mat(
            if hi { 0x5c0707 } else { 0x4a0606 },
            Some(if hi { 0.3 } else { 0.0 }),
            0xff1712,
            0.3,
            if hi { 0.12 } else { 0.3 },
            hi,
        ));
        let rev = graph.add_material(light_mat(0x9a9a9a, None, 0xffffff, 0.0, 0.3, hi));
        // Light accents (the electric car's blades), which flare under
        // boost.
        let accent = has("accent").then(|| {
            graph.add_material(
                Material::standard()
                    .set("color", 0x0b2a33)
                    .set("emissive", 0x3fe4ff)
                    .set("emissiveIntensity", 1.4)
                    .set("roughness", 0.3)
                    .set("vertexColors", hi),
            )
        });
        let c = Color::hex(color);
        let stripe_color = opts.stripe_color.unwrap_or(if police {
            POLICE_WHITE
        } else if c.r + c.g + c.b > 2.4 {
            0x151515
        } else {
            0xf1efe8
        });
        // Siren lenses (police only): per instance, like the head/tail
        // lights.
        let light_red = has("lightRed").then(|| {
            graph.add_material(light_mat(
                if hi { 0x7a0c0c } else { 0x5a0808 },
                Some(if hi { 0.1 } else { 0.0 }),
                0xff1a0c,
                0.0,
                0.15,
                hi,
            ))
        });
        let light_blue = has("lightBlue").then(|| {
            graph.add_material(light_mat(
                if hi { 0x0c1c8a } else { 0x0a1668 },
                Some(if hi { 0.1 } else { 0.0 }),
                0x1840ff,
                0.0,
                0.15,
                hi,
            ))
        });
        let stripe = self.stripe_mat(graph, stripe_color, hi);
        let mat_for = |b: &str| -> MaterialId {
            match b {
                "paint" => paint,
                "head" => head,
                "tail" => tail,
                "rev" => rev,
                "accent" => accent.expect("an accent material"),
                "lightRed" => light_red.expect("a red lens"),
                "lightBlue" => light_blue.expect("a blue lens"),
                "glass" => mm.glass,
                "trim" => mm.trim,
                "chrome" => mm.chrome,
                "plate" => mm.plate,
                "cargo" => mm.cargo,
                "seat" => mm.seat,
                "carbon" => mm.carbon,
                "stripe" => stripe,
                _ => mm.trim,
            }
        };

        let root = graph.group(&format!("vehicle:{kind}"));
        let body = graph.group("body");
        graph.add(root, body);
        for &(bucket, geom) in &geoms {
            let mesh = graph.mesh(geom, mat_for(bucket));
            let o = graph.get_mut(mesh);
            o.cast_shadow = ![
                "head",
                "tail",
                "rev",
                "accent",
                "glass",
                "lightRed",
                "lightBlue",
            ]
            .contains(&bucket);
            o.name = bucket.to_string();
            // Transparent glass after the body so the interior shows
            // through it.
            if bucket == "glass" && hi {
                o.render_order = 1.0;
            }
            graph.add(body, mesh);
        }

        // Wheels: [fl, fr, rl, rr]; left = +X. Front wheels hang off steer
        // pivots.
        let wl = layout.wheels;
        let rim_mat = match wl.rim_mat {
            Some(RimMat::Chrome) => mm.rim_chrome,
            Some(RimMat::White) => mm.rim_white,
            Some(RimMat::Aero) => mm.rim_aero,
            Some(RimMat::Tractor) => mm.rim_tractor,
            Some(RimMat::Dark) => mm.rim_dark,
            None if opts.rim_dark => mm.rim_dark,
            None if racer => {
                if seed % 2 == 1 {
                    mm.rim_dark
                } else {
                    mm.rim
                }
            }
            None => mm.rim_steel,
        };
        let wheel_mats = [mm.tire, rim_mat, mm.brake];
        let style = if hi {
            wl
        } else {
            WheelLayout {
                ty: if racer { WheelType::Racer } else { wl.ty },
                ..wl
            }
        };
        let make_wheel = |kit: &mut CarKit, graph: &mut SceneGraph, r: f64, w: f64, sx: f64| {
            let wheel = object3d(graph, "wheel");
            graph
                .get_mut(wheel)
                .user_data
                .insert("radius".into(), num(r));
            let g = kit.wheel(graph, r, w, hi, &style);
            let mesh = graph.multi_mesh(g, &wheel_mats);
            let o = graph.get_mut(mesh);
            o.cast_shadow = true;
            if sx < 0.0 {
                o.set_rotation(&Euler::new(0.0, PI, 0.0)); // rim face outboard on the right side
            }
            graph.add(wheel, mesh);
            wheel
        };
        // Calipers sit on the (non-rotating) hub carriers: steer pivots at
        // the front, fixed points at the rear.
        let cal_mat = sh.caliper[wl.caliper.unwrap_or(0)];
        let add_caliper =
            |kit: &mut CarKit, graph: &mut SceneGraph, parent: NodeId, r: f64, w: f64, sx: f64| {
                let g = kit.caliper(graph, r * wl.rim_frac);
                let cal = graph.mesh(g, cal_mat);
                let o = graph.get_mut(cal);
                o.position = Vector3::new(sx * -w * 0.09, 0.0, 0.0);
                if sx < 0.0 {
                    o.set_rotation(&Euler::new(0.0, PI, 0.0));
                }
                graph.add(parent, cal);
            };
        let front = wl.front.unwrap_or(FrontWheels {
            r: wl.r,
            w: wl.w,
            track: wl.track,
        });
        let mut wheels = Vec::new();
        let mut steer_pivots = Vec::new();
        for sx in [1.0, -1.0] {
            let pivot = object3d(graph, "steerPivot");
            graph.get_mut(pivot).position = Vector3::new((sx * front.track) / 2.0, front.r, wl.z_f);
            let wheel = make_wheel(self, graph, front.r, front.w, sx);
            graph.add(pivot, wheel);
            if hi && racer {
                add_caliper(self, graph, pivot, front.r, front.w, sx);
            }
            graph.add(root, pivot);
            steer_pivots.push(pivot);
            wheels.push(wheel);
        }
        let rear_w = wl.rear_w.unwrap_or(wl.w);
        for sx in [1.0, -1.0] {
            let wheel = make_wheel(self, graph, wl.r, rear_w, sx);
            let p = Vector3::new((sx * wl.track) / 2.0, wl.r, wl.z_r);
            graph.get_mut(wheel).position = p;
            graph.add(root, wheel);
            wheels.push(wheel);
            if hi && racer {
                let hub = object3d(graph, "");
                graph.get_mut(hub).position = p;
                graph.add(root, hub);
                add_caliper(self, graph, hub, wl.r, rear_w, sx);
            }
        }

        let headlight_anchor = object3d(graph, "headlightAnchor");
        graph.get_mut(headlight_anchor).position =
            Vector3::new(layout.head[0], layout.head[1], layout.head[2]);
        graph.add(body, headlight_anchor);

        let mut handle = VehicleModel {
            root,
            body,
            wheels,
            steer_pivots,
            kind: kind.to_string(),
            dims: layout.dims,
            exhausts: layout
                .exhausts
                .iter()
                .map(|p| Vector3::new(p[0], p[1], p[2]))
                .collect(),
            headlight_anchor,
            paint,
            head,
            tail,
            rev,
            accent,
            siren: None,
            far: None,
            brake: 0.0,
            lights: 0.0,
            lr: 0.0,
            lb: 0.0,
            is_far: false,
        };

        // Siren: lens emission, glow billboards and the anchor for the
        // game's shared flash light, all driven by setSiren(mode, t).
        if let Some(siren) = &layout.siren {
            let geo = match self.parts[pi].1.siren_geo {
                Some(g) => g,
                None => {
                    let g = graph.add_geometry(glow_geometry(&siren.glows));
                    self.parts[pi].1.siren_geo = Some(g);
                    g
                }
            };
            let glow_mat = glow_material(graph, textures);
            let siren_glow = graph.mesh(geo, glow_mat);
            {
                let o = graph.get_mut(siren_glow);
                o.name = "sirenGlow".into();
                o.render_order = 2.0;
            }
            graph.add(body, siren_glow);
            let siren_anchor = object3d(graph, "sirenAnchor");
            graph.get_mut(siren_anchor).position =
                Vector3::new(siren.anchor[0], siren.anchor[1], siren.anchor[2]);
            graph.add(body, siren_anchor);
            handle.siren = Some(Siren {
                glow: siren_glow,
                glow_material: glow_mat,
                anchor: siren_anchor,
                red: light_red.expect("a red lens"),
                blue: light_blue.expect("a blue lens"),
            });
            let edits = handle.set_siren(SirenMode::Off, 0.0);
            apply_edits(graph, &edits);
        }
        if opts.far {
            let key = format!(
                "{kind}|{}|{variant:?}|{}",
                if hi { "high" } else { "low" },
                rim_mat.0
            );
            far::add_far_lod(self, graph, &mut handle, &key);
        }
        handle
    }
}

/// The hook Mountain's parked cars take (`mountain::ParkedCars`):
/// `buildVehicle(kind, { color, seed, lod: 'low' })` then
/// `setHeadlights(0)`, returning the car's root.
pub fn parked_car(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    kind: &str,
    color: u32,
    seed: u32,
) -> Option<NodeId> {
    let mut v = build_vehicle(
        graph,
        textures,
        kind,
        &BuildOpts {
            color: Some(color),
            seed,
            lod: Some(Lod::Low),
            ..BuildOpts::default()
        },
    )?;
    let edits = v.set_headlights(0.0);
    apply_edits(graph, &edits);
    Some(v.root)
}

/// `triangleCount(obj)`: the triangle count of a built vehicle (budget
/// checks / debugging).
pub fn triangle_count(graph: &SceneGraph, root: NodeId) -> f64 {
    let mut tris = 0.0;
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let o = graph.get(id);
        if o.ty.is_mesh()
            && let Some(g) = o.geometry
        {
            let g = graph.geometry(g);
            tris += match &g.index {
                Some(i) => i.count() as f64,
                None => g.vertex_count() as f64,
            } / 3.0;
        }
        stack.extend(o.children.iter().rev());
    }
    tris
}
