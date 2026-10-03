//! The plain material kinds on three_std (SPEC 6.2): Standard, Physical,
//! Lambert and Basic, and Line and Points drawn as basic. A JS material's
//! parameters become a [`ThreeMaterial`]: its uniforms, its maps, and a
//! [`ThreeKey`] that picks the shader variant, culling, blending and depth
//! state the way three's `WebGLPrograms` and `WebGLState` do.

use bevy::math::Vec4;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MeshPipelineKey};
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction, Face,
    RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::{ShaderDefVal, ShaderRef};
use mr_scene::{MaterialDesc, Scene, TextureDesc, three};

/// three's lighting model for a material type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Model {
    /// MeshStandardMaterial and MeshPhysicalMaterial.
    #[default]
    Physical,
    /// MeshLambertMaterial.
    Lambert,
    /// MeshBasicMaterial, LineBasicMaterial, PointsMaterial: unlit.
    Basic,
}

/// The shader variant and fixed-function state (three's program parameters
/// and `WebGLState.setMaterial`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ThreeKey {
    pub model: Model,
    /// MeshPhysicalMaterial (IOR and specular).
    pub physical: bool,
    pub clearcoat: bool,
    pub sheen: bool,
    pub map: bool,
    pub emissive_map: bool,
    pub fog: bool,
    pub alpha_test: bool,
    /// `transparent: false` with normal blending: alpha written as 1.
    pub opaque: bool,
    /// three's instanceColor, carried in the instance's tag.
    pub instance_color: bool,
    /// `side`: FrontSide, BackSide or DoubleSide.
    pub side: u32,
    /// `shadowSide` (three's default: the opposite of `side`, double stays
    /// double).
    pub shadow_side: u32,
    /// `NoBlending`, `NormalBlending` (when transparent) or `AdditiveBlending`.
    pub blending: u32,
    pub depth_write: bool,
    pub depth_test: bool,
    /// `polygonOffset` as a constant depth bias.
    pub depth_bias: i32,
}

/// The material's uniforms (`three_material.wgsl`'s `ThreeParams`).
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct ThreeParams {
    pub diffuse: Vec4,
    pub emissive: Vec4,
    pub pbr: Vec4,
    pub sheen: Vec4,
    pub specular: Vec4,
    pub physical: Vec4,
    pub map_t0: Vec4,
    pub map_t1: Vec4,
    pub emissive_t0: Vec4,
    pub emissive_t1: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(ThreeKey)]
pub struct ThreeMaterial {
    #[uniform(0)]
    pub params: ThreeParams,
    #[texture(1)]
    #[sampler(2)]
    pub map: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub emissive_map: Option<Handle<Image>>,
    #[texture(
        10,
        sample_type = "float",
        filterable = false,
        visibility(vertex, fragment)
    )]
    pub globals: Handle<Image>,
    #[texture(11)]
    #[sampler(12)]
    pub env: Handle<Image>,
    pub key: ThreeKey,
}

impl From<&ThreeMaterial> for ThreeKey {
    fn from(m: &ThreeMaterial) -> ThreeKey {
        m.key
    }
}

const SHADER: &str = "embedded://mr_game/render/three_material.wgsl";

fn face(side: u32) -> Option<Face> {
    match side {
        three::BACK_SIDE => Some(Face::Front),
        three::DOUBLE_SIDE => None,
        _ => Some(Face::Back),
    }
}

impl Material for ThreeMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "embedded://mr_game/render/three_prepass.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        let k = &self.key;
        if k.blending != three::NO_BLENDING {
            AlphaMode::Blend
        } else if k.alpha_test {
            AlphaMode::Mask(self.params.emissive.w)
        } else {
            AlphaMode::Opaque
        }
    }

    fn depth_bias(&self) -> f32 {
        self.key.depth_bias as f32
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let k = key.bind_group_data;
        if key.mesh_key.contains(MeshPipelineKey::DEPTH_PREPASS) {
            // The shadow pass: three draws the shadow side, and keeps the
            // alpha test (`three_prepass.wgsl`).
            descriptor.primitive.cull_mode = face(k.shadow_side);
            if let Some(f) = descriptor.fragment.as_mut() {
                for (on, name) in [(k.map, "USE_MAP"), (k.alpha_test, "ALPHA_TEST")] {
                    if on {
                        f.shader_defs.push(name.into());
                    }
                }
            }
            return Ok(());
        }
        let mut defs: Vec<ShaderDefVal> = Vec::new();
        match k.model {
            Model::Physical => {
                defs.push("LIT".into());
                defs.push("LIT_PHYSICAL".into());
            }
            Model::Lambert => {
                defs.push("LIT".into());
                defs.push("LIT_LAMBERT".into());
            }
            Model::Basic => {}
        }
        for (on, name) in [
            (k.physical, "PHYSICAL"),
            (k.clearcoat, "USE_CLEARCOAT"),
            (k.sheen, "USE_SHEEN"),
            (k.map, "USE_MAP"),
            (k.emissive_map, "USE_EMISSIVEMAP"),
            (k.fog, "USE_FOG"),
            (k.alpha_test, "ALPHA_TEST"),
            (k.opaque, "OPAQUE"),
            (k.instance_color, "INSTANCE_COLOR"),
            (k.side == three::DOUBLE_SIDE, "DOUBLE_SIDED"),
            (k.side == three::BACK_SIDE, "FLIP_SIDED"),
        ] {
            if on {
                defs.push(name.into());
            }
        }
        descriptor.vertex.shader_defs.extend(defs.iter().cloned());
        if let Some(f) = descriptor.fragment.as_mut() {
            f.shader_defs.extend(defs);
            // WebGLState.setBlending (premultipliedAlpha false).
            let blend = match k.blending {
                three::ADDITIVE_BLENDING => Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                }),
                three::NORMAL_BLENDING => Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::OneMinusSrcAlpha,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::OneMinusSrcAlpha,
                        operation: BlendOperation::Add,
                    },
                }),
                _ => None,
            };
            for t in f.targets.iter_mut().flatten() {
                t.blend = blend;
            }
        }
        descriptor.primitive.cull_mode = face(k.side);
        if let Some(ds) = descriptor.depth_stencil.as_mut() {
            ds.depth_write_enabled = Some(k.depth_write);
            ds.depth_compare = Some(if k.depth_test {
                CompareFunction::GreaterEqual
            } else {
                CompareFunction::Always
            });
            ds.bias.constant = k.depth_bias;
        }
        Ok(())
    }
}

/// The images every three_std material binds.
#[derive(Resource, Clone, ExtractResource)]
pub struct SharedImages {
    /// The globals row (`render::lighting`).
    pub globals: Handle<Image>,
    /// The environment's cube-UV atlas (`render::pmrem`).
    pub env: Handle<Image>,
}

/// three's `Matrix3.setUvTransform` for a texture, as the two rows the
/// shader dots with (u, v, 1).
pub fn uv_rows(t: &TextureDesc) -> (Vec4, Vec4) {
    let [tx, ty] = t.offset;
    let [sx, sy] = t.repeat;
    let [cx, cy] = t.center;
    let (s, c) = (t.rotation.sin(), t.rotation.cos());
    (
        Vec4::new(
            (sx * c) as f32,
            (sx * s) as f32,
            (-sx * (c * cx + s * cy) + cx + tx) as f32,
            0.0,
        ),
        Vec4::new(
            (-sy * s) as f32,
            (sy * c) as f32,
            (-sy * (-s * cx + c * cy) + cy + ty) as f32,
            0.0,
        ),
    )
}

/// The lighting model for a three.js material type, if it is a built-in one
/// this shader draws.
pub fn model_of(ty: &str) -> Option<Model> {
    match ty {
        "MeshStandardMaterial" | "MeshPhysicalMaterial" => Some(Model::Physical),
        "MeshLambertMaterial" => Some(Model::Lambert),
        "MeshBasicMaterial" | "LineBasicMaterial" | "PointsMaterial" => Some(Model::Basic),
        _ => None,
    }
}

/// A JS material as a [`ThreeMaterial`]: the plain version of its built-in
/// type (a patched kind draws without its patch, as a stand-in). `images`
/// are the scene's textures as Bevy images; `instance_color` says the
/// geometry is an InstancedMesh with instanceColor.
pub fn three_material(
    scene: &Scene,
    m: &MaterialDesc,
    images: &[Option<Handle<Image>>],
    shared: &SharedImages,
    instance_color: bool,
) -> Option<ThreeMaterial> {
    let model = model_of(&m.ty)?;
    let num = |n: &str, d: f64| m.number(n).unwrap_or(d);
    let flag = |n: &str, d: bool| m.boolean(n).unwrap_or(d);
    let col = |n: &str, d: [f64; 3]| m.color(n).unwrap_or(d);
    let tex = |name: &str| {
        let i = m.params.get(name).and_then(|_| m.texture(name))?;
        let h = images.get(i as usize).cloned().flatten()?;
        Some((h, scene.textures.get(i as usize)?))
    };
    let physical = m.ty == "MeshPhysicalMaterial";
    let c = col("color", [1.0; 3]);
    let opacity = num("opacity", 1.0);
    let lit = model != Model::Basic;
    let (e, ei) = if lit {
        (col("emissive", [0.0; 3]), num("emissiveIntensity", 1.0))
    } else {
        ([0.0; 3], 0.0)
    };
    let alpha_test = num("alphaTest", 0.0);
    let clearcoat = if physical { num("clearcoat", 0.0) } else { 0.0 };
    let sheen = if physical { num("sheen", 0.0) } else { 0.0 };
    let sheen_color = col("sheenColor", [0.0; 3]);
    let mut p = ThreeParams {
        diffuse: Vec4::new(c[0] as f32, c[1] as f32, c[2] as f32, opacity as f32),
        emissive: Vec4::new(
            (e[0] * ei) as f32,
            (e[1] * ei) as f32,
            (e[2] * ei) as f32,
            alpha_test as f32,
        ),
        pbr: Vec4::new(
            num("roughness", 1.0) as f32,
            num("metalness", 0.0) as f32,
            clearcoat as f32,
            num("clearcoatRoughness", 0.0) as f32,
        ),
        // refreshUniformsPhysical: sheenColor × sheen.
        sheen: Vec4::new(
            (sheen_color[0] * sheen) as f32,
            (sheen_color[1] * sheen) as f32,
            (sheen_color[2] * sheen) as f32,
            num("sheenRoughness", 1.0) as f32,
        ),
        specular: {
            let s = col("specularColor", [1.0; 3]);
            Vec4::new(
                s[0] as f32,
                s[1] as f32,
                s[2] as f32,
                num("specularIntensity", 1.0) as f32,
            )
        },
        physical: Vec4::new(num("ior", 1.5) as f32, 1.0, 0.0, 0.0),
        ..ThreeParams::default()
    };
    // PointsMaterial's map is a point sprite; points draw one pixel for now.
    let map = if m.ty == "PointsMaterial" {
        None
    } else {
        tex("map")
    };
    if let Some((_, t)) = &map {
        (p.map_t0, p.map_t1) = uv_rows(t);
    }
    let emissive_map = if lit { tex("emissiveMap") } else { None };
    if let Some((_, t)) = &emissive_map {
        (p.emissive_t0, p.emissive_t1) = uv_rows(t);
    }
    let side = num("side", 0.0) as u32;
    let shadow_side = m
        .number("shadowSide")
        .map(|s| s as u32)
        .unwrap_or(match side {
            three::FRONT_SIDE => three::BACK_SIDE,
            three::BACK_SIDE => three::FRONT_SIDE,
            s => s,
        });
    let transparent = flag("transparent", false);
    let given_blending = num("blending", 1.0) as u32;
    // setMaterial: normal blending applies only to transparent materials.
    let blending = if given_blending == three::NORMAL_BLENDING && !transparent {
        three::NO_BLENDING
    } else {
        given_blending
    };
    let depth_bias = if flag("polygonOffset", false) {
        // three's offset is (factor, units), negative toward the camera;
        // Bevy's bias is a constant, positive toward the camera (D103).
        let f = num("polygonOffsetFactor", 0.0);
        let u = num("polygonOffsetUnits", 0.0);
        (-(f + u) * 32.0) as i32
    } else {
        0
    };
    let key = ThreeKey {
        model,
        physical,
        clearcoat: clearcoat > 0.0,
        sheen: sheen > 0.0,
        map: map.is_some(),
        emissive_map: emissive_map.is_some(),
        fog: flag("fog", true),
        alpha_test: alpha_test > 0.0,
        opaque: !transparent && given_blending == three::NORMAL_BLENDING,
        instance_color,
        side,
        shadow_side,
        blending,
        depth_write: flag("depthWrite", true),
        depth_test: flag("depthTest", true),
        depth_bias,
    };
    Some(ThreeMaterial {
        params: p,
        map: map.map(|(h, _)| h),
        emissive_map: emissive_map.map(|(h, _)| h),
        globals: shared.globals.clone(),
        env: shared.env.clone(),
        key,
    })
}
