//! three.js materials as `mr_scene::MaterialDesc`s (SPEC 5.1, DECISIONS
//! D21, D22, D194).
//!
//! A JS material is an object whose own properties the scene export writes
//! out one by one (`tools/parity/lib/scene-page.js`), in their JS order:
//! three's `Material` base first, then the class's own. The port builds the
//! same parameter map: [`Material::standard`] and the other constructors
//! hold every own property of a fresh three.js r180 material with its
//! default, in that order, and [`Material::set`] is three's `setValues` for
//! one key (a colour property takes a hex number, a CSS string or a
//! [`Color`]; anything else is replaced; an unknown key is ignored, as
//! three ignores it after a warning). So
//!
//! ```text
//! new THREE.MeshStandardMaterial({ color: 0x8a6a4a, roughness: 0.9 })
//! ```
//!
//! is `Material::standard().set("color", 0x8a6a4a).set("roughness", 0.9)`,
//! and its parameters equal the export's (`tests/builders.rs` checks the
//! defaults of every built-in class and a few constructed materials
//! against three).
//!
//! A built-in material's kind follows from its class (`Standard`,
//! `Lambert`, ...). A material with a shader patch is tagged in the JS
//! (`userData.kind`, D22); here [`Material::kind`] does the same, with the
//! patch's closure values as `kind_opts`, and [`Material::uniform`] adds
//! the uniforms the patch creates.
//!
//! Texture parameters hold a [`TextureId`] of the world being built; the
//! scene assembly renumbers them (`object::SceneGraph::finish`).

use mr_scene::{MaterialDesc, MaterialKind};
use serde_json::{Map, Value, json};

use crate::color::Color;
use crate::object::TextureId;

/// A material being built: its description, with texture parameters
/// holding [`TextureId`]s.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub desc: MaterialDesc,
}

/// A value for [`Material::set`], as the JS would pass it in the
/// parameters object.
#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    /// A number. For a colour property, a hex colour (`Math.floor`ed).
    Num(f64),
    Bool(bool),
    Str(String),
    /// A colour given as a `THREE.Color` (copied).
    Color(Color),
    Texture(TextureId),
    /// A `Vector2` (`normalScale`).
    Vec2(f64, f64),
    /// `null`.
    Null,
}

impl From<f64> for Param {
    fn from(v: f64) -> Self {
        Param::Num(v)
    }
}

impl From<i32> for Param {
    fn from(v: i32) -> Self {
        Param::Num(f64::from(v))
    }
}

impl From<u32> for Param {
    fn from(v: u32) -> Self {
        Param::Num(f64::from(v))
    }
}

impl From<bool> for Param {
    fn from(v: bool) -> Self {
        Param::Bool(v)
    }
}

impl From<&str> for Param {
    fn from(v: &str) -> Self {
        Param::Str(v.to_string())
    }
}

impl From<Color> for Param {
    fn from(v: Color) -> Self {
        Param::Color(v)
    }
}

impl From<TextureId> for Param {
    fn from(v: TextureId) -> Self {
        Param::Texture(v)
    }
}

/// A number as the export writes it: finite numbers plain, the others as
/// `{"num": "Infinity"}` (D25).
pub fn num(v: f64) -> Value {
    if v.is_finite() {
        Value::from(v)
    } else if v.is_nan() {
        json!({ "num": "NaN" })
    } else if v > 0.0 {
        json!({ "num": "Infinity" })
    } else {
        json!({ "num": "-Infinity" })
    }
}

/// A colour as the export writes it.
pub fn color_value(c: Color) -> Value {
    json!({ "color": [num(c.r), num(c.g), num(c.b)] })
}

/// A texture reference.
pub fn texture_value(t: TextureId) -> Value {
    json!({ "texture": t.0 })
}

// The default own properties, in three's order.
#[derive(Clone, Copy)]
enum D {
    N(f64),
    B(bool),
    S(&'static str),
    Null,
    Rgb(f64, f64, f64),
    Vec2(f64, f64),
    Euler,
    Defines(&'static [&'static str]),
    Range(f64, f64),
    Inf,
}

impl D {
    fn value(self) -> Value {
        match self {
            D::N(v) => num(v),
            D::B(b) => Value::Bool(b),
            D::S(s) => Value::from(s),
            D::Null => Value::Null,
            D::Rgb(r, g, b) => color_value(Color::new(r, g, b)),
            D::Vec2(x, y) => json!({ "vec": [num(x), num(y)] }),
            D::Euler => json!({ "euler": [num(0.0), num(0.0), num(0.0), "XYZ"] }),
            D::Defines(keys) => {
                let mut m = Map::new();
                for k in keys {
                    m.insert((*k).to_string(), Value::from(""));
                }
                Value::Object(m)
            }
            D::Range(a, b) => json!([num(a), num(b)]),
            D::Inf => num(f64::INFINITY),
        }
    }
}

use D::*;

/// three's `Material` constructor (the `_alphaTest` accessor last, written
/// `alphaTest` as the export strips the underscore).
const MATERIAL: &[(&str, D)] = &[
    ("blending", N(1.0)),
    ("side", N(0.0)),
    ("vertexColors", B(false)),
    ("opacity", N(1.0)),
    ("transparent", B(false)),
    ("alphaHash", B(false)),
    ("blendSrc", N(204.0)),
    ("blendDst", N(205.0)),
    ("blendEquation", N(100.0)),
    ("blendSrcAlpha", Null),
    ("blendDstAlpha", Null),
    ("blendEquationAlpha", Null),
    ("blendColor", Rgb(0.0, 0.0, 0.0)),
    ("blendAlpha", N(0.0)),
    ("depthFunc", N(3.0)),
    ("depthTest", B(true)),
    ("depthWrite", B(true)),
    ("stencilWriteMask", N(255.0)),
    ("stencilFunc", N(519.0)),
    ("stencilRef", N(0.0)),
    ("stencilFuncMask", N(255.0)),
    ("stencilFail", N(7680.0)),
    ("stencilZFail", N(7680.0)),
    ("stencilZPass", N(7680.0)),
    ("stencilWrite", B(false)),
    ("clippingPlanes", Null),
    ("clipIntersection", B(false)),
    ("clipShadows", B(false)),
    ("shadowSide", Null),
    ("colorWrite", B(true)),
    ("precision", Null),
    ("polygonOffset", B(false)),
    ("polygonOffsetFactor", N(0.0)),
    ("polygonOffsetUnits", N(0.0)),
    ("dithering", B(false)),
    ("alphaToCoverage", B(false)),
    ("premultipliedAlpha", B(false)),
    ("forceSinglePass", B(false)),
    ("allowOverride", B(true)),
    ("visible", B(true)),
    ("toneMapped", B(true)),
    ("alphaTest", N(0.0)),
];

/// `MeshStandardMaterial`'s own properties after `defines`.
const STANDARD: &[(&str, D)] = &[
    ("color", Rgb(1.0, 1.0, 1.0)),
    ("roughness", N(1.0)),
    ("metalness", N(0.0)),
    ("map", Null),
    ("lightMap", Null),
    ("lightMapIntensity", N(1.0)),
    ("aoMap", Null),
    ("aoMapIntensity", N(1.0)),
    ("emissive", Rgb(0.0, 0.0, 0.0)),
    ("emissiveIntensity", N(1.0)),
    ("emissiveMap", Null),
    ("bumpMap", Null),
    ("bumpScale", N(1.0)),
    ("normalMap", Null),
    ("normalMapType", N(0.0)),
    ("normalScale", Vec2(1.0, 1.0)),
    ("displacementMap", Null),
    ("displacementScale", N(1.0)),
    ("displacementBias", N(0.0)),
    ("roughnessMap", Null),
    ("metalnessMap", Null),
    ("alphaMap", Null),
    ("envMap", Null),
    ("envMapRotation", Euler),
    ("envMapIntensity", N(1.0)),
    ("wireframe", B(false)),
    ("wireframeLinewidth", N(1.0)),
    ("wireframeLinecap", S("round")),
    ("wireframeLinejoin", S("round")),
    ("flatShading", B(false)),
    ("fog", B(true)),
];

/// `MeshPhysicalMaterial`'s own properties after the standard ones (its
/// accessor-backed `_anisotropy` ... `_transmission` last).
const PHYSICAL: &[(&str, D)] = &[
    ("anisotropyRotation", N(0.0)),
    ("anisotropyMap", Null),
    ("clearcoatMap", Null),
    ("clearcoatRoughness", N(0.0)),
    ("clearcoatRoughnessMap", Null),
    ("clearcoatNormalScale", Vec2(1.0, 1.0)),
    ("clearcoatNormalMap", Null),
    ("ior", N(1.5)),
    ("iridescenceMap", Null),
    ("iridescenceIOR", N(1.3)),
    ("iridescenceThicknessRange", Range(100.0, 400.0)),
    ("iridescenceThicknessMap", Null),
    ("sheenColor", Rgb(0.0, 0.0, 0.0)),
    ("sheenColorMap", Null),
    ("sheenRoughness", N(1.0)),
    ("sheenRoughnessMap", Null),
    ("transmissionMap", Null),
    ("thickness", N(0.0)),
    ("thicknessMap", Null),
    ("attenuationDistance", Inf),
    ("attenuationColor", Rgb(1.0, 1.0, 1.0)),
    ("specularIntensity", N(1.0)),
    ("specularIntensityMap", Null),
    ("specularColor", Rgb(1.0, 1.0, 1.0)),
    ("specularColorMap", Null),
    ("anisotropy", N(0.0)),
    ("clearcoat", N(0.0)),
    ("dispersion", N(0.0)),
    ("iridescence", N(0.0)),
    ("sheen", N(0.0)),
    ("transmission", N(0.0)),
];

const LAMBERT: &[(&str, D)] = &[
    ("color", Rgb(1.0, 1.0, 1.0)),
    ("map", Null),
    ("lightMap", Null),
    ("lightMapIntensity", N(1.0)),
    ("aoMap", Null),
    ("aoMapIntensity", N(1.0)),
    ("emissive", Rgb(0.0, 0.0, 0.0)),
    ("emissiveIntensity", N(1.0)),
    ("emissiveMap", Null),
    ("bumpMap", Null),
    ("bumpScale", N(1.0)),
    ("normalMap", Null),
    ("normalMapType", N(0.0)),
    ("normalScale", Vec2(1.0, 1.0)),
    ("displacementMap", Null),
    ("displacementScale", N(1.0)),
    ("displacementBias", N(0.0)),
    ("specularMap", Null),
    ("alphaMap", Null),
    ("envMap", Null),
    ("envMapRotation", Euler),
    ("combine", N(0.0)),
    ("reflectivity", N(1.0)),
    ("refractionRatio", N(0.98)),
    ("wireframe", B(false)),
    ("wireframeLinewidth", N(1.0)),
    ("wireframeLinecap", S("round")),
    ("wireframeLinejoin", S("round")),
    ("flatShading", B(false)),
    ("fog", B(true)),
];

const BASIC: &[(&str, D)] = &[
    ("color", Rgb(1.0, 1.0, 1.0)),
    ("map", Null),
    ("lightMap", Null),
    ("lightMapIntensity", N(1.0)),
    ("aoMap", Null),
    ("aoMapIntensity", N(1.0)),
    ("specularMap", Null),
    ("alphaMap", Null),
    ("envMap", Null),
    ("envMapRotation", Euler),
    ("combine", N(0.0)),
    ("reflectivity", N(1.0)),
    ("refractionRatio", N(0.98)),
    ("wireframe", B(false)),
    ("wireframeLinewidth", N(1.0)),
    ("wireframeLinecap", S("round")),
    ("wireframeLinejoin", S("round")),
    ("fog", B(true)),
];

const LINE_BASIC: &[(&str, D)] = &[
    ("color", Rgb(1.0, 1.0, 1.0)),
    ("map", Null),
    ("linewidth", N(1.0)),
    ("linecap", S("round")),
    ("linejoin", S("round")),
    ("fog", B(true)),
];

const SPRITE: &[(&str, D)] = &[
    ("color", Rgb(1.0, 1.0, 1.0)),
    ("map", Null),
    ("alphaMap", Null),
    ("rotation", N(0.0)),
    ("sizeAttenuation", B(true)),
    ("fog", B(true)),
];

const POINTS: &[(&str, D)] = &[
    ("color", Rgb(1.0, 1.0, 1.0)),
    ("map", Null),
    ("alphaMap", Null),
    ("size", N(1.0)),
    ("sizeAttenuation", B(true)),
    ("fog", B(true)),
];

fn params(lists: &[&[(&str, D)]]) -> Map<String, Value> {
    let mut m = Map::new();
    for list in lists {
        for (k, d) in *list {
            m.insert((*k).to_string(), d.value());
        }
    }
    m
}

impl Material {
    fn built_in(kind: MaterialKind, ty: &str, params: Map<String, Value>) -> Material {
        Material {
            desc: MaterialDesc {
                kind,
                kind_opts: None,
                ty: ty.to_string(),
                name: String::new(),
                program_key: None,
                params,
                uniforms: None,
                shader: None,
            },
        }
    }

    /// `new THREE.MeshStandardMaterial()`.
    pub fn standard() -> Material {
        let mut p = params(&[MATERIAL]);
        p.insert("defines".into(), Defines(&["STANDARD"]).value());
        p.extend(params(&[STANDARD]));
        Material::built_in(MaterialKind::Standard, "MeshStandardMaterial", p)
    }

    /// `new THREE.MeshPhysicalMaterial()`.
    pub fn physical() -> Material {
        let mut p = params(&[MATERIAL]);
        p.insert("defines".into(), Defines(&["STANDARD", "PHYSICAL"]).value());
        p.extend(params(&[STANDARD, PHYSICAL]));
        Material::built_in(MaterialKind::Physical, "MeshPhysicalMaterial", p)
    }

    /// `new THREE.MeshLambertMaterial()`.
    pub fn lambert() -> Material {
        let p = params(&[MATERIAL, LAMBERT]);
        Material::built_in(MaterialKind::Lambert, "MeshLambertMaterial", p)
    }

    /// `new THREE.MeshBasicMaterial()`.
    pub fn basic() -> Material {
        let p = params(&[MATERIAL, BASIC]);
        Material::built_in(MaterialKind::Basic, "MeshBasicMaterial", p)
    }

    /// `new THREE.LineBasicMaterial()`.
    pub fn line_basic() -> Material {
        let p = params(&[MATERIAL, LINE_BASIC]);
        Material::built_in(MaterialKind::Line, "LineBasicMaterial", p)
    }

    /// `new THREE.SpriteMaterial()` (transparent by default).
    pub fn sprite() -> Material {
        let mut p = params(&[MATERIAL, SPRITE]);
        p.insert("transparent".into(), Value::Bool(true));
        Material::built_in(MaterialKind::Sprite, "SpriteMaterial", p)
    }

    /// `new THREE.PointsMaterial()`.
    pub fn points() -> Material {
        let p = params(&[MATERIAL, POINTS]);
        Material::built_in(MaterialKind::Points, "PointsMaterial", p)
    }

    /// `setValues({ [key]: value })`.
    pub fn set(mut self, key: &str, value: impl Into<Param>) -> Material {
        self.set_value(key, value.into());
        self
    }

    /// [`Material::set`] in place.
    pub fn set_value(&mut self, key: &str, value: Param) {
        // MeshPhysicalMaterial's `reflectivity` is an accessor over `ior`.
        if key == "reflectivity" && self.desc.ty == "MeshPhysicalMaterial" {
            if let Param::Num(r) = value {
                let ior = (1.0 + 0.4 * r) / (1.0 - 0.4 * r);
                self.desc.params.insert("ior".into(), num(ior));
            }
            return;
        }
        let Some(current) = self.desc.params.get_mut(key) else {
            // three: "'key' is not a property of THREE.<type>", ignored.
            return;
        };
        let is_color = current.get("color").is_some();
        *current = if is_color {
            // `currentValue.set(newValue)`
            let mut c = Color::new(1.0, 1.0, 1.0);
            let old = &current["color"];
            if let (Some(r), Some(g), Some(b)) = (old[0].as_f64(), old[1].as_f64(), old[2].as_f64())
            {
                c = Color::new(r, g, b);
            }
            match value {
                Param::Num(h) => {
                    c.set_hex(h);
                }
                Param::Str(s) => {
                    c.set_style(&s);
                }
                Param::Color(v) => c = v,
                _ => {}
            }
            color_value(c)
        } else {
            match value {
                Param::Num(v) => num(v),
                Param::Bool(b) => Value::Bool(b),
                Param::Str(s) => Value::String(s),
                Param::Color(c) => color_value(c),
                Param::Texture(t) => texture_value(t),
                Param::Vec2(x, y) => json!({ "vec": [num(x), num(y)] }),
                Param::Null => Value::Null,
            }
        };
    }

    /// `material.name = name`.
    pub fn name(mut self, name: &str) -> Material {
        self.desc.name = name.to_string();
        self
    }

    /// `userData.kind` (and `kindOpts`): the shader patch this material
    /// carries (D21, D22).
    pub fn kind(mut self, kind: MaterialKind, opts: Option<Value>) -> Material {
        self.desc.kind = kind;
        self.desc.kind_opts = opts;
        self
    }

    /// `customProgramCacheKey = () => key`.
    pub fn program_key(mut self, key: &str) -> Material {
        self.desc.program_key = Some(key.to_string());
        self
    }

    /// A uniform the patch (or a `ShaderMaterial`) creates, valued as the
    /// export writes it ([`num`], [`color_value`], [`texture_value`]).
    pub fn uniform(mut self, name: &str, value: Value) -> Material {
        self.desc
            .uniforms
            .get_or_insert_with(Map::new)
            .insert(name.to_string(), value);
        self
    }

    /// A parameter's value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.desc.params.get(key)
    }

    /// A number parameter.
    pub fn number(&self, key: &str) -> Option<f64> {
        self.desc.number(key)
    }

    /// A colour parameter.
    pub fn color(&self, key: &str) -> Option<Color> {
        self.desc.color(key).map(|[r, g, b]| Color::new(r, g, b))
    }
}
