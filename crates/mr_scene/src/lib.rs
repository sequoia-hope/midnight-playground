//! Scene data: meshes, instances, materials, textures, nodes and lights as
//! plain data, and the `.mrscene` reader and writer (SPEC 5.1). Both the world
//! generator and the client depend on it; it depends on nothing of theirs.
//!
//! The data mirrors what three.js holds in the JS game, field for field, so
//! a scene exported from the game (`tools/parity/scene-export.mjs`) and one
//! built by `mr_worldgen` can be compared and drawn the same way:
//!
//! - **Buffers** are the typed arrays (vertex attributes, indices, instance
//!   matrices and colours, texture pixels), shared by index as three.js
//!   shares the arrays themselves.
//! - **Meshes** are geometries: named attributes (custom ones keep their JS
//!   names: `aLane`, `aSurf`, `cell`, `aVar`, `ndata`, `ph`, ...), an
//!   optional index, groups for multi-material meshes, draw range and the
//!   bounds three culls with.
//! - **Nodes** are the object tree (groups, meshes, instanced meshes,
//!   points, lines, sprites, lights) with local and world matrices.
//! - **Materials** carry a [`MaterialKind`] (one per distinct shader of the
//!   JS game, SPEC 6.2) and their three.js parameters as JSON values.
//! - Colours are linear, as three.js holds them; texture pixels are as the
//!   canvas or data array stores them, with `flip_y` saying whether the GPU
//!   upload flips the rows (SPEC 5.2).
//!
//! Numeric constants (wrap modes, filters, blending, sides, formats) keep
//! three.js r180's values; [`three`] names the ones in use.
//!
//! The file format is described in `FORMAT.md` beside this crate.

#![forbid(unsafe_code)]

pub mod digest;
mod file;
mod num;

pub use file::{MAGIC, VERSION, read, read_file, write, write_file};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One scene: a level's world, or a set of models.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// Where the scene came from (level, query, generator); free-form.
    pub meta: Value,
    pub buffers: Vec<Buffer>,
    pub meshes: Vec<MeshDesc>,
    pub instances: Vec<InstanceDesc>,
    pub materials: Vec<MaterialDesc>,
    pub textures: Vec<TextureDesc>,
    pub nodes: Vec<NodeDesc>,
    /// Nodes without a parent, in order.
    pub roots: Vec<u32>,
    pub lights: Vec<LightDesc>,
    /// Material properties that follow nightfall (`world.nightMaterials`):
    /// `value = day + (night - day) * n`.
    pub night_params: Vec<NightParam>,
    /// Fog, exposure and camera at the moment of export.
    pub environment: Option<Environment>,
}

/// A typed array.
#[derive(Clone, Debug, PartialEq)]
pub struct Buffer {
    /// Components per element (3 for a position, 16 for a matrix, 4 for an
    /// RGBA pixel).
    pub item_size: u32,
    /// Integer data read as 0..1 (or -1..1) by the GPU.
    pub normalized: bool,
    pub data: BufferData,
}

impl Buffer {
    /// Number of elements (`data.len() / item_size`).
    pub fn count(&self) -> usize {
        self.data.len() / self.item_size.max(1) as usize
    }
}

/// The element type and values of a [`Buffer`], as the JS typed array.
#[derive(Clone, Debug, PartialEq)]
pub enum BufferData {
    F32(Vec<f32>),
    F64(Vec<f64>),
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
    I8(Vec<i8>),
    I16(Vec<i16>),
    I32(Vec<i32>),
}

/// The component type of a buffer, as named in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Component {
    F32,
    F64,
    U8,
    U16,
    U32,
    I8,
    I16,
    I32,
}

impl Component {
    pub fn size(self) -> usize {
        match self {
            Component::U8 | Component::I8 => 1,
            Component::U16 | Component::I16 => 2,
            Component::F32 | Component::U32 | Component::I32 => 4,
            Component::F64 => 8,
        }
    }
}

impl BufferData {
    pub fn len(&self) -> usize {
        match self {
            BufferData::F32(v) => v.len(),
            BufferData::F64(v) => v.len(),
            BufferData::U8(v) => v.len(),
            BufferData::U16(v) => v.len(),
            BufferData::U32(v) => v.len(),
            BufferData::I8(v) => v.len(),
            BufferData::I16(v) => v.len(),
            BufferData::I32(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn component(&self) -> Component {
        match self {
            BufferData::F32(_) => Component::F32,
            BufferData::F64(_) => Component::F64,
            BufferData::U8(_) => Component::U8,
            BufferData::U16(_) => Component::U16,
            BufferData::U32(_) => Component::U32,
            BufferData::I8(_) => Component::I8,
            BufferData::I16(_) => Component::I16,
            BufferData::I32(_) => Component::I32,
        }
    }

    /// Element `i` as an f64 (exact for every component type).
    pub fn get(&self, i: usize) -> f64 {
        match self {
            BufferData::F32(v) => f64::from(v[i]),
            BufferData::F64(v) => v[i],
            BufferData::U8(v) => f64::from(v[i]),
            BufferData::U16(v) => f64::from(v[i]),
            BufferData::U32(v) => f64::from(v[i]),
            BufferData::I8(v) => f64::from(v[i]),
            BufferData::I16(v) => f64::from(v[i]),
            BufferData::I32(v) => f64::from(v[i]),
        }
    }

    /// The values as little-endian bytes, as the JS typed array holds them.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len() * self.component().size());
        match self {
            BufferData::F32(v) => v.iter().for_each(|x| out.extend(x.to_le_bytes())),
            BufferData::F64(v) => v.iter().for_each(|x| out.extend(x.to_le_bytes())),
            BufferData::U8(v) => out.extend_from_slice(v),
            BufferData::U16(v) => v.iter().for_each(|x| out.extend(x.to_le_bytes())),
            BufferData::U32(v) => v.iter().for_each(|x| out.extend(x.to_le_bytes())),
            BufferData::I8(v) => v.iter().for_each(|x| out.extend(x.to_le_bytes())),
            BufferData::I16(v) => v.iter().for_each(|x| out.extend(x.to_le_bytes())),
            BufferData::I32(v) => v.iter().for_each(|x| out.extend(x.to_le_bytes())),
        }
        out
    }

    /// Parses `len` elements of `component` from little-endian bytes.
    pub fn from_le_bytes(component: Component, bytes: &[u8]) -> BufferData {
        fn take<const N: usize, T>(b: &[u8], f: fn([u8; N]) -> T) -> Vec<T> {
            b.chunks_exact(N)
                .map(|c| f(c.try_into().expect("chunks_exact gives N bytes")))
                .collect()
        }
        match component {
            Component::F32 => BufferData::F32(take(bytes, f32::from_le_bytes)),
            Component::F64 => BufferData::F64(take(bytes, f64::from_le_bytes)),
            Component::U8 => BufferData::U8(bytes.to_vec()),
            Component::U16 => BufferData::U16(take(bytes, u16::from_le_bytes)),
            Component::U32 => BufferData::U32(take(bytes, u32::from_le_bytes)),
            Component::I8 => BufferData::I8(take(bytes, i8::from_le_bytes)),
            Component::I16 => BufferData::I16(take(bytes, i16::from_le_bytes)),
            Component::I32 => BufferData::I32(take(bytes, i32::from_le_bytes)),
        }
    }

    pub fn as_f32(&self) -> Option<&[f32]> {
        match self {
            BufferData::F32(v) => Some(v),
            _ => None,
        }
    }
}

/// A geometry (three.js `BufferGeometry`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshDesc {
    pub name: String,
    /// In the geometry's insertion order.
    pub attributes: Vec<AttributeRef>,
    /// Buffer of the index, if indexed.
    pub index: Option<u32>,
    /// Draw groups (`geometry.groups`), for multi-material meshes.
    pub groups: Vec<GroupDesc>,
    pub draw_range: DrawRange,
    /// `geometry.boundingBox` as set (min xyz, max xyz), or none.
    #[serde(with = "num::opt_seq")]
    pub bounding_box: Option<Vec<f64>>,
    /// `geometry.boundingSphere` as set (centre xyz, radius). Some builders
    /// widen it on purpose so the object is never culled.
    #[serde(with = "num::opt_seq")]
    pub bounding_sphere: Option<Vec<f64>>,
}

impl MeshDesc {
    pub fn attribute(&self, name: &str) -> Option<&AttributeRef> {
        self.attributes.iter().find(|a| a.name == name)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttributeRef {
    /// The JS attribute name.
    pub name: String,
    /// Index into [`Scene::buffers`].
    pub accessor: u32,
    /// An `InstancedBufferAttribute`: one element per instance.
    #[serde(default, skip_serializing_if = "is_false")]
    pub instanced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_per_attribute: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupDesc {
    pub start: u32,
    #[serde(with = "num::one")]
    pub count: f64,
    pub material_index: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawRange {
    pub start: u32,
    /// None: to the end (three's `Infinity`).
    pub count: Option<u32>,
}

/// An `InstancedMesh`'s per-instance data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceDesc {
    pub node: u32,
    /// Instances drawn (`mesh.count`).
    pub count: u32,
    /// Instances allocated (the matrices buffer may hold more than `count`).
    pub capacity: u32,
    /// Buffer of 4×4 column-major matrices (f32, item size 16).
    pub matrices: u32,
    /// Buffer of linear RGB colours (`instanceColor`), if any.
    pub colors: Option<u32>,
    #[serde(with = "num::opt_seq")]
    pub bounding_sphere: Option<Vec<f64>>,
}

/// One material: its shader ([`MaterialKind`]) and parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDesc {
    pub kind: MaterialKind,
    /// Options a kind's JS patch takes from its closure rather than from the
    /// material (`Siding`'s mode, `FlickerPoints`' rate, depth and blink,
    /// `AmbientProp`'s colour, `Terrain`'s packed detail and photo).
    pub kind_opts: Option<Value>,
    /// The three.js class (`MeshStandardMaterial`, `ShaderMaterial`, ...).
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
    /// `customProgramCacheKey()`, where the JS sets one.
    pub program_key: Option<String>,
    /// The material's own properties, in JS order. Plain values as JSON; a
    /// colour is `{"color": [r, g, b]}` (linear), a vector `{"vec": [..]}`, a
    /// matrix `{"mat": [..]}` (column-major), an Euler `{"euler": [x, y, z,
    /// order]}`, a texture `{"texture": index}`, a non-finite number
    /// `{"num": "Infinity"}`.
    pub params: Map<String, Value>,
    /// A `ShaderMaterial`'s uniforms, or the uniforms an `onBeforeCompile`
    /// patch adds to a built-in material (values as in `params`).
    pub uniforms: Option<Map<String, Value>>,
    /// A `ShaderMaterial`'s GLSL.
    pub shader: Option<ShaderSource>,
}

impl MaterialDesc {
    /// A parameter, or else a uniform, by name.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.params
            .get(name)
            .or_else(|| self.uniforms.as_ref().and_then(|u| u.get(name)))
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        self.get(name).and_then(Value::as_f64)
    }

    pub fn boolean(&self, name: &str) -> Option<bool> {
        self.get(name).and_then(Value::as_bool)
    }

    /// A linear colour.
    pub fn color(&self, name: &str) -> Option<[f64; 3]> {
        let c = self.get(name)?.get("color")?.as_array()?;
        Some([
            c.first()?.as_f64()?,
            c.get(1)?.as_f64()?,
            c.get(2)?.as_f64()?,
        ])
    }

    /// A texture index.
    pub fn texture(&self, name: &str) -> Option<u32> {
        texture_ref(self.get(name)?)
    }

    /// Every texture the parameters and uniforms refer to, in order of
    /// first mention (parameters first).
    pub fn textures(&self) -> Vec<u32> {
        fn walk(v: &Value, out: &mut Vec<u32>) {
            if let Some(t) = texture_ref(v) {
                if !out.contains(&t) {
                    out.push(t);
                }
                return;
            }
            match v {
                Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
                Value::Object(o) => o.values().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        self.params.values().for_each(|v| walk(v, &mut out));
        if let Some(u) = &self.uniforms {
            u.values().for_each(|v| walk(v, &mut out));
        }
        out
    }
}

fn texture_ref(v: &Value) -> Option<u32> {
    let o = v.as_object()?;
    if o.len() != 1 {
        return None;
    }
    o.get("texture")?
        .as_u64()
        .and_then(|t| u32::try_from(t).ok())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShaderSource {
    pub vertex: String,
    pub fragment: String,
}

/// One variant per distinct shader in the JS game (SPEC 6.2). The JS tags
/// each patched or custom material with its kind (`material.userData.kind`);
/// built-in materials without a patch get the plain kinds at the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialKind {
    /// `world/TerrainMesh.js` `patchTriplanar`.
    Terrain,
    /// `world/Road.js` `patchAsphalt`.
    Asphalt,
    /// `world/Road.js`, the shoulder's gravel fade.
    Shoulder,
    /// `world/Road.js` `patchMarkings`.
    Markings,
    /// `world/Sea.js` `patchMaterial`.
    Sea,
    /// `world/Sky.js`, the dome's ShaderMaterial.
    SkyDome,
    /// `vehicles/CarModel.js` `lightMat` (high detail).
    CarLight,
    /// `vehicles/CarModel.js` `glowMaterial`.
    PoliceGlow,
    /// `world/Mountain.js` and `world/coast/kit.js` `rockMaterial`.
    TriplanarRock,
    /// `world/desert/parts.js` `sandstoneMaterial`.
    Sandstone,
    /// `world/Beach.js`, triplanar stucco grain.
    Stucco,
    /// `world/Valley.js` `surfaceDetail` (siding, boards or roof).
    Siding,
    /// `world/city/cityTextures.js` `patchAtlasMaterial` (no caller in the
    /// game at present).
    CityAtlas,
    /// `world/city/cityTextures.js` `patchCityMaterial`.
    CityFacade,
    /// `world/streets/textures.js` `patchStreetAtlas`.
    StreetAtlas,
    /// `world/streets/facades.js` `facadeMaterial`.
    StreetFacade,
    /// `world/harbor/textures.js` `containerMaterial`.
    ContainerAtlas,
    /// `world/City.js` `glowPointsMaterial`.
    GlowPoints,
    /// `world/desert/glow.js` `flickerPoints`.
    FlickerPoints,
    /// `world/desert/glow.js` `flickerPools`.
    GroundPool,
    /// `world/streets/props.js` `neonFlicker`.
    Neon,
    /// `world/streets/props.js` `ambientPatch`.
    AmbientProp,
    /// `world/City.js`, the traffic light streams.
    TrafficStreams,
    /// `world/City.js`, the sky-glow dome.
    SkyGlow,
    /// `world/Coast.js` `foamMaterial`.
    Surf,
    /// `world/Coast.js`, the lighthouse beam.
    LighthouseBeam,
    /// `world/streets/props.js` `buildSteam`.
    Steam,
    /// `world/Desert.js`, the floodlight beams.
    FloodBeam,
    /// `world/Mountain.js`, the post reflectors.
    Reflector,
    /// `game/Effects.js` `Particles`.
    Particles,
    /// `game/Effects.js` `SkidMarks`.
    SkidMarks,
    /// Built-in `MeshStandardMaterial`.
    Standard,
    /// Built-in `MeshPhysicalMaterial`.
    Physical,
    /// Built-in `MeshLambertMaterial`.
    Lambert,
    /// Built-in `MeshBasicMaterial`.
    Basic,
    /// Built-in `LineBasicMaterial`.
    Line,
    /// Built-in `SpriteMaterial`.
    Sprite,
    /// Built-in `PointsMaterial` (not in SPEC 6.2's table; DECISIONS D21).
    Points,
}

/// A texture: pixels, sampler and upload flags.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureDesc {
    pub name: String,
    pub source: TextureSource,
    /// The file an image texture was loaded from (repo-relative).
    pub url: Option<String>,
    pub width: u32,
    pub height: u32,
    /// 4 for RGBA, 1 for R.
    pub channels: u32,
    /// Buffer of the pixels, top row first as the canvas or data array holds
    /// them; several textures may share one.
    pub pixels: u32,
    /// three.js format and type constants.
    pub format: u32,
    #[serde(rename = "type")]
    pub ty: u32,
    /// Whether the upload flips the rows (true for canvas textures).
    pub flip_y: bool,
    /// `"srgb"`, `"srgb-linear"` or `""` (none).
    pub color_space: String,
    pub premultiply_alpha: bool,
    pub unpack_alignment: u32,
    pub generate_mipmaps: bool,
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub mag_filter: u32,
    pub min_filter: u32,
    pub anisotropy: f64,
    pub offset: [f64; 2],
    pub repeat: [f64; 2],
    pub rotation: f64,
    pub center: [f64; 2],
    pub matrix_auto_update: bool,
    /// The UV channel it samples.
    pub channel: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureSource {
    /// Drawn on a canvas (`CanvasTexture`).
    Canvas,
    /// A `DataTexture`.
    Data,
    /// A decoded image file (Seaside's aerial photo).
    Image,
}

/// A node of the object tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDesc {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: NodeType,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    /// Local matrix, column-major (three's `matrix.elements`).
    #[serde(with = "num::arr16")]
    pub matrix: [f64; 16],
    /// World matrix at export, column-major.
    #[serde(with = "num::arr16")]
    pub matrix_world: [f64; 16],
    pub visible: bool,
    pub matrix_auto_update: bool,
    pub frustum_culled: bool,
    pub render_order: f64,
    pub cast_shadow: bool,
    pub receive_shadow: bool,
    pub layers: u32,
    /// The plain values of `userData`.
    pub user_data: Map<String, Value>,
    /// Geometry, for meshes, points, lines and sprites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<u32>,
    /// One material, or one per group when `multi_material`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_material: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instances: Option<u32>,
    /// A sprite's centre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<u32>,
}

/// three.js object types the scenes use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeType {
    Group,
    Object3D,
    Mesh,
    InstancedMesh,
    Points,
    LineSegments,
    Line,
    LineLoop,
    Sprite,
    DirectionalLight,
    HemisphereLight,
    SpotLight,
    PointLight,
    AmbientLight,
}

impl NodeType {
    /// three's `isMesh`: drawn as triangles.
    pub fn is_mesh(self) -> bool {
        matches!(self, NodeType::Mesh | NodeType::InstancedMesh)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightDesc {
    pub node: u32,
    #[serde(rename = "type")]
    pub ty: NodeType,
    pub color: [f64; 3],
    pub intensity: f64,
    pub cast_shadow: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_color: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decay: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub penumbra: Option<f64>,
    /// World position of the light's target (directional and spot lights).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<ShadowDesc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowDesc {
    pub map_size: [f64; 2],
    pub bias: f64,
    pub normal_bias: f64,
    pub radius: f64,
    pub blur_samples: f64,
    pub camera: ShadowCamera,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadowCamera {
    #[serde(rename = "type")]
    pub ty: String,
    pub near: f64,
    pub far: f64,
    pub left: Option<f64>,
    pub right: Option<f64>,
    pub top: Option<f64>,
    pub bottom: Option<f64>,
    pub fov: Option<f64>,
}

/// A material property that follows nightfall (`world.addNight`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NightParam {
    pub material: u32,
    pub prop: String,
    pub day: f64,
    pub night: f64,
}

/// The renderer state the scene was exported under.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub fog: Option<FogDesc>,
    pub tone_mapping: u32,
    pub tone_mapping_exposure: f64,
    pub environment_intensity: f64,
    /// The sky's night factor n.
    pub night: f64,
    pub camera: CameraDesc,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FogDesc {
    #[serde(rename = "type")]
    pub ty: String,
    pub color: [f64; 3],
    pub density: Option<f64>,
    pub near: Option<f64>,
    pub far: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraDesc {
    pub position: [f64; 3],
    pub quaternion: [f64; 4],
    pub fov: f64,
    pub near: f64,
    pub far: f64,
    pub aspect: f64,
}

/// three.js r180 constants that appear in the scenes.
pub mod three {
    pub const REPEAT_WRAPPING: u32 = 1000;
    pub const CLAMP_TO_EDGE_WRAPPING: u32 = 1001;
    pub const MIRRORED_REPEAT_WRAPPING: u32 = 1002;
    pub const NEAREST_FILTER: u32 = 1003;
    pub const LINEAR_FILTER: u32 = 1006;
    pub const LINEAR_MIPMAP_LINEAR_FILTER: u32 = 1008;
    pub const UNSIGNED_BYTE_TYPE: u32 = 1009;
    pub const RGBA_FORMAT: u32 = 1023;
    pub const RED_FORMAT: u32 = 1028;
    pub const FRONT_SIDE: u32 = 0;
    pub const BACK_SIDE: u32 = 1;
    pub const DOUBLE_SIDE: u32 = 2;
    pub const NO_BLENDING: u32 = 0;
    pub const NORMAL_BLENDING: u32 = 1;
    pub const ADDITIVE_BLENDING: u32 = 2;
    pub const CUSTOM_BLENDING: u32 = 5;
    pub const ACES_FILMIC_TONE_MAPPING: u32 = 4;
}

fn is_false(b: &bool) -> bool {
    !*b
}
