//! The material kinds on three_std (SPEC 6.2): the plain ones (Standard,
//! Physical, Lambert and Basic, Line drawn as basic), the patched kinds of
//! WP 2.4 (Terrain, Asphalt, Shoulder, Markings, Sea: each JS
//! `onBeforeCompile` patch as a block of shader code at the same point of
//! `three_material.wgsl`, selected by [`Patch`]), points (Points,
//! GlowPoints, FlickerPoints) drawn as camera-facing quads, and Level 1's
//! remaining kinds (WP 3.9: TriplanarRock, Reflector, Siding, CityFacade,
//! the TrafficStreams and SkyGlow shaders, Sprite). A JS material's
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
use mp_scene::{MaterialDesc, MaterialKind, Scene, TextureDesc, three};

/// The JS patch a material carries, ported into `three_material.wgsl`
/// (WP 2.4). `None` draws the plain built-in material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Patch {
    #[default]
    None,
    /// `TerrainMesh.js` `patchTriplanar`: `packed` (the four-scale detail
    /// texture; the game always passes it) and `photo` (Seaside's drape).
    Terrain { packed: bool, photo: bool },
    /// `Road.js` `patchAsphalt`.
    Asphalt,
    /// `Road.js`'s shoulder patch (gravel fading to its average).
    Shoulder,
    /// `Road.js` `patchMarkings`.
    Markings,
    /// `Sea.js` `patchMaterial`.
    Sea,
    /// A `PointsMaterial` drawn as quads (SPEC 6.2 "Points"):
    /// `sizeAttenuation`, and the kind's patch.
    Points { attenuate: bool, mode: PointsMode },
    /// `Mountain.js` `rockMaterial`: triplanar strata from `tRock` in world
    /// space, instancing-aware (WP 3.9).
    Rock,
    /// `Mountain.js`'s post reflectors: the emissive takes the instance
    /// colour.
    Reflector,
    /// `Valley.js` `surfaceDetail`: lap siding, barn boards or shingles in
    /// world space.
    Siding(SidingMode),
    /// `city/cityTextures.js` `patchCityMaterial`: the building atlas by
    /// cell, lit windows, glass, shopfronts and light spill.
    City,
    /// `City.js` `buildTraffic`: a `ShaderMaterial` on points, head- and
    /// tail-lights sliding along the streets in the vertex shader.
    Traffic,
    /// `City.js` `buildSkyGlow`: a `ShaderMaterial`, the warm haze over the
    /// city.
    SkyGlow,
    /// three's `SpriteMaterial`: a camera-facing quad (the waterfall's
    /// spray).
    Sprite { attenuate: bool },
    /// `harbor/textures.js` `containerMaterial`: the atlas row by the
    /// instance's `aVar`, the body colour from the instance colour.
    Container,
    /// `Beach.js` `stuccoMaterial`: a world-space triplanar grain.
    Stucco,
    /// `Coast.js` `foamMaterial`: a `ShaderMaterial`, the surf against the
    /// rocks and the rings round them.
    Surf,
    /// `Coast.js`'s lighthouse beam: a `ShaderMaterial`, additive.
    Beam,
    /// `desert/parts.js` `sandstoneMaterial`: triplanar strata and varnish.
    Sandstone,
    /// `desert/glow.js` `flickerPools`: additive pools flickering by the
    /// instance's `ph` and `fl`.
    Pool,
    /// `Desert.js`'s flood-light cones: fading where edge-on.
    FloodBeam,
    /// `streets/facades.js` `facadeMaterial`: the façade atlas by cell, lit
    /// windows, the street's bounce light.
    StreetFacade,
    /// `streets/textures.js` `patchStreetAtlas`: shop fronts and rowhouses.
    StreetAtlas,
    /// `streets/props.js` `ambientPatch`: emissive in proportion to albedo.
    Ambient,
    /// `streets/props.js` `neonFlicker`: the signs' hum, buzz and drop-outs.
    Neon,
    /// `streets/props.js` `buildSteam`: a `ShaderMaterial` on points, puffs
    /// rising from the vents.
    Steam,
    /// `game/Effects.js` `Particles`: a `ShaderMaterial` on points, the
    /// tyre smoke and the sparks: per point size, alpha and colour, the
    /// texture, fog (WP 4.4).
    Particles,
    /// `game/Effects.js` `SkidMarks`: a `ShaderMaterial`, dark quads with a
    /// per-vertex alpha (WP 4.4).
    Skid,
    /// `vehicles/CarModel.js` `glowMaterial`: a `ShaderMaterial`, the
    /// siren's additive glow billboards, turned to the camera, pulled
    /// towards it and held at a minimum screen size (WP 8.1). The corner,
    /// `aBlue` and `aSize` ride in the extra attribute; `uRed` and `uBlue`
    /// are `setSiren`'s colours × light slots `kind0.x` and `kind0.y`
    /// (`play::police`), `uMin` is `kind0.z`.
    PoliceGlow,
}

/// `surfaceDetail`'s `mode` (`kind_opts.mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SidingMode {
    #[default]
    Siding,
    Boards,
    Roof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PointsMode {
    /// The built-in `PointsMaterial`.
    #[default]
    Plain,
    /// `City.js` `glowPointsMaterial`: per-point size, a minimum pixel
    /// size, a gentler fog.
    Glow,
    /// `desert/glow.js` `flickerPoints`: a per-point phase and the shared
    /// clock; `blink` is the hard on/off chase.
    Flicker { blink: bool },
}

fn kind_opt(m: &MaterialDesc, k: &str) -> Option<f64> {
    let v = m.kind_opts.as_ref()?.get(k)?;
    v.as_f64().or_else(|| v.as_bool().map(f64::from))
}

impl Patch {
    /// The patch of a material, from its kind tag and options.
    pub fn of(m: &MaterialDesc) -> Patch {
        let flag = |k: &str| kind_opt(m, k).unwrap_or(0.0) != 0.0;
        let points = |mode| Patch::Points {
            attenuate: m.boolean("sizeAttenuation").unwrap_or(true),
            mode,
        };
        match m.kind {
            MaterialKind::Terrain => Patch::Terrain {
                packed: flag("packed"),
                photo: flag("packed") && flag("photo"),
            },
            MaterialKind::Asphalt => Patch::Asphalt,
            MaterialKind::Shoulder => Patch::Shoulder,
            MaterialKind::Markings => Patch::Markings,
            MaterialKind::Sea => Patch::Sea,
            MaterialKind::GlowPoints => points(PointsMode::Glow),
            MaterialKind::FlickerPoints => points(PointsMode::Flicker {
                blink: kind_opt(m, "blink").unwrap_or(0.0) > 0.0,
            }),
            MaterialKind::TriplanarRock => Patch::Rock,
            MaterialKind::Reflector => Patch::Reflector,
            MaterialKind::Siding => Patch::Siding(
                match m
                    .kind_opts
                    .as_ref()
                    .and_then(|o| o.get("mode"))
                    .and_then(|v| v.as_str())
                {
                    Some("boards") => SidingMode::Boards,
                    Some("roof") => SidingMode::Roof,
                    _ => SidingMode::Siding,
                },
            ),
            MaterialKind::CityFacade => Patch::City,
            MaterialKind::TrafficStreams => Patch::Traffic,
            MaterialKind::SkyGlow => Patch::SkyGlow,
            MaterialKind::Sprite => Patch::Sprite {
                attenuate: m.boolean("sizeAttenuation").unwrap_or(true),
            },
            MaterialKind::ContainerAtlas => Patch::Container,
            MaterialKind::Stucco => Patch::Stucco,
            MaterialKind::Surf => Patch::Surf,
            MaterialKind::LighthouseBeam => Patch::Beam,
            MaterialKind::Sandstone => Patch::Sandstone,
            MaterialKind::GroundPool => Patch::Pool,
            MaterialKind::FloodBeam => Patch::FloodBeam,
            MaterialKind::StreetFacade => Patch::StreetFacade,
            MaterialKind::StreetAtlas => Patch::StreetAtlas,
            MaterialKind::AmbientProp => Patch::Ambient,
            MaterialKind::Neon => Patch::Neon,
            MaterialKind::Steam => Patch::Steam,
            MaterialKind::Particles => Patch::Particles,
            MaterialKind::SkidMarks => Patch::Skid,
            MaterialKind::PoliceGlow => Patch::PoliceGlow,
            _ if m.ty == "PointsMaterial" => points(PointsMode::Plain),
            _ => Patch::None,
        }
    }
}

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
    /// three's instanceColor: in the instance stream when `instanced`,
    /// else in the entity's tag (the material test scenes).
    pub instance_color: bool,
    /// An `InstancedMesh`: one entity, its instances in a vertex buffer
    /// stepped per instance (`render::instancing`, D450).
    pub instanced: bool,
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
    /// `polygonOffsetFactor` as a slope-scaled depth bias (positive toward
    /// the camera; the race's effects, D803). Other materials keep the
    /// constant alone (D103).
    pub depth_slope: i32,
    /// A tie-break among transparent objects at the same distance (D808):
    /// three breaks such ties by object id, so the race's effects, which
    /// share a sort point, rank in the order `Effects` makes them; 0 for the
    /// scene's materials, whose ties keep the scene's order (D810). Nothing
    /// else moves a material in the sort: three never sorts by its polygon
    /// offset.
    pub sort_rank: u8,
    /// The JS patch (WP 2.4), or points.
    pub patch: Patch,
    /// A tangent-space normal map (the sea's), with three's derivative
    /// frame (no tangents).
    pub normal_map: bool,
    /// `alphaMap` (`alphamap_fragment`: its green channel), bound in the
    /// `photo` slot, which only the terrain uses otherwise.
    pub alpha_map: bool,
    /// `flatShading`: the normal from the position's derivatives.
    pub flat_shading: bool,
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
    /// The normal map's uv transform rows.
    pub normal_t0: Vec4,
    pub normal_t1: Vec4,
    /// Per patch: Terrain `uPhotoBox`; Sea `normalScale` (xy); Points
    /// `size`, `uMinPx`, and the flicker's rate and depth; City `uGround`
    /// (x); Traffic `uTime`, `uNight`, `uFogK`, `uHalfH` as exported;
    /// SkyGlow `uK`, `uGround`, `uH`; Sprite `rotation` (x); Particles
    /// `uScale` (x).
    pub kind0: Vec4,
    /// Points: the flicker's blink rate.
    pub kind1: Vec4,
    /// `world.nightMaterials` (D455): `emissiveIntensity` day and night
    /// values, and 1 in z when the material follows nightfall; the shader
    /// scales `emissive` by `day + (night - day) × n` with the sky's night
    /// factor from the globals. 2 in z: by light slot w of the globals
    /// (`lighting::MaterialLights`, D456). Zero (the default) leaves
    /// `emissive` as it is. 3 in z (three's own unlit materials): the
    /// opacity is light slot w instead (the race's headlight pools, D803).
    pub night: Vec4,
    /// The alpha map's uv transform rows.
    pub alpha_t0: Vec4,
    pub alpha_t1: Vec4,
    /// x: the first texel of the material's animation block in the
    /// globals (`lighting::G_BLOCKS`, `crate::animate`), 0 for none: the
    /// values the scenery's animators move (colours, `emissiveIntensity`,
    /// texture offsets, a kind's uniforms), read by the shader instead of
    /// these parameters, so no material is edited per frame (D490).
    pub slots: Vec4,
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
    /// The packed detail texture (`tDetail`; the sea's `tFoam`).
    #[texture(5)]
    #[sampler(6)]
    pub detail: Option<Handle<Image>>,
    /// The terrain's `tRock`, or the sea's `normalMap`.
    #[texture(7)]
    #[sampler(8)]
    pub aux: Option<Handle<Image>>,
    /// Seaside's draped photo (`tPhoto`) and loose-ground mask (`tLoose`).
    #[texture(13)]
    #[sampler(14)]
    pub photo: Option<Handle<Image>>,
    #[texture(15)]
    #[sampler(16)]
    pub loose: Option<Handle<Image>>,
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

const SHADER: &str = "embedded://mp_game/render/three_material.wgsl";

/// The sort distance a step of `ThreeKey::sort_rank` adds (metres along the
/// view): above f32's resolution at the distances a race sees (0.002 at
/// 20 km), below anything that separates two objects that are not tied.
pub const SORT_STEP: f32 = 0.01;

/// The shadow pass's vertex shader for an InstancedMesh
/// (`three_prepass_instanced.wgsl`), set in `specialize`, which has no
/// asset server: hence a fixed handle (`render::ThreeRenderPlugin` loads it).
pub const INSTANCED_PREPASS_SHADER: Handle<Shader> =
    bevy::asset::uuid_handle!("6b1f0a52-3c1e-4d47-9b0e-2f6c8e1d4a90");

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
        "embedded://mp_game/render/three_prepass.wgsl".into()
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
        // Bevy adds this to a transparent item's sort distance and uses it
        // for nothing else (the pipeline's bias is set in `specialize`):
        // three's sort never sees the polygon offset (D808, D810).
        f32::from(self.key.sort_rank) * SORT_STEP
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let k = key.bind_group_data;
        if key.mesh_key.contains(MeshPipelineKey::DEPTH_PREPASS) {
            // The shadow pass: three draws the shadow side, and keeps the
            // alpha test (`three_prepass.wgsl`).
            descriptor.primitive.cull_mode = face(k.shadow_side);
            if k.instanced {
                // Bevy's prepass vertex shader, with the instance's matrix.
                descriptor.vertex.shader = INSTANCED_PREPASS_SHADER;
                descriptor
                    .vertex
                    .buffers
                    .push(super::instancing::instance_layout(false));
            }
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
        // A patch's own attribute (aSurf, aLane, aDepth, gsize, ph, and the
        // points' quad corners) at location 8, beside Bevy's standard ones.
        if layout.0.contains(crate::convert::ATTRIBUTE_EXTRA) {
            let mut attrs = vec![Mesh::ATTRIBUTE_POSITION.at_shader_location(0)];
            for (a, at) in [
                (Mesh::ATTRIBUTE_NORMAL, 1),
                (Mesh::ATTRIBUTE_UV_0, 2),
                (Mesh::ATTRIBUTE_UV_1, 3),
                (Mesh::ATTRIBUTE_TANGENT, 4),
                (Mesh::ATTRIBUTE_COLOR, 5),
            ] {
                if layout.0.contains(a) {
                    attrs.push(a.at_shader_location(at));
                }
            }
            attrs.push(crate::convert::ATTRIBUTE_EXTRA.at_shader_location(8));
            // The traffic streams' second attribute (`aDir`, `aPar.z`).
            if layout.0.contains(crate::convert::ATTRIBUTE_EXTRA2) {
                attrs.push(crate::convert::ATTRIBUTE_EXTRA2.at_shader_location(14));
                defs.push("VERTEX_EXTRA2".into());
            }
            descriptor.vertex.buffers = vec![layout.0.get_layout(&attrs)?];
            defs.push("VERTEX_EXTRA".into());
        }
        if k.instanced {
            descriptor
                .vertex
                .buffers
                .push(super::instancing::instance_layout(true));
            defs.push("MP_INSTANCED".into());
        }
        // three's own materials (no kind patch): an animator's opacity is the
        // block's texel 4.x (`crate::animate`, D701).
        if matches!(
            k.patch,
            Patch::None
                | Patch::Sprite { .. }
                | Patch::Points {
                    mode: PointsMode::Plain,
                    ..
                }
        ) {
            defs.push("PLAIN_ANIM".into());
        }
        match k.patch {
            Patch::None => {}
            Patch::Terrain { packed, photo } => {
                defs.push("PATCH_TERRAIN".into());
                if packed {
                    defs.push("TERRAIN_PACKED".into());
                }
                if photo {
                    defs.push("MR_PHOTO".into());
                }
            }
            Patch::Asphalt => defs.push("PATCH_ASPHALT".into()),
            Patch::Shoulder => defs.push("PATCH_SHOULDER".into()),
            Patch::Markings => defs.push("PATCH_MARKINGS".into()),
            Patch::Sea => defs.push("PATCH_SEA".into()),
            Patch::Points { attenuate, mode } => {
                defs.push("POINTS".into());
                if attenuate {
                    defs.push("USE_SIZEATTENUATION".into());
                }
                match mode {
                    PointsMode::Plain => {}
                    PointsMode::Glow => defs.push("POINTS_GLOW".into()),
                    PointsMode::Flicker { blink } => {
                        defs.push("POINTS_FLICKER".into());
                        if blink {
                            defs.push("FLICKER_BLINK".into());
                        }
                    }
                }
            }
            Patch::Rock => defs.push("PATCH_ROCK".into()),
            Patch::Reflector => defs.push("PATCH_REFLECTOR".into()),
            Patch::Siding(mode) => defs.push(
                match mode {
                    SidingMode::Siding => "SIDING_SIDING",
                    SidingMode::Boards => "SIDING_BOARDS",
                    SidingMode::Roof => "SIDING_ROOF",
                }
                .into(),
            ),
            Patch::City => defs.push("PATCH_CITY".into()),
            Patch::Traffic => defs.push("PATCH_TRAFFIC".into()),
            Patch::SkyGlow => defs.push("PATCH_SKYGLOW".into()),
            Patch::Sprite { attenuate } => {
                defs.push("SPRITE".into());
                if attenuate {
                    defs.push("USE_SIZEATTENUATION".into());
                }
            }
            Patch::Container => defs.push("PATCH_CONTAINER".into()),
            Patch::Stucco => defs.push("PATCH_STUCCO".into()),
            Patch::Surf => defs.push("PATCH_SURF".into()),
            Patch::Beam => defs.push("PATCH_BEAM".into()),
            Patch::Sandstone => defs.push("PATCH_SANDSTONE".into()),
            Patch::Pool => defs.push("PATCH_POOL".into()),
            Patch::FloodBeam => defs.push("PATCH_FLOODBEAM".into()),
            Patch::StreetFacade => defs.push("PATCH_SFACADE".into()),
            Patch::StreetAtlas => defs.push("PATCH_SATLAS".into()),
            Patch::Ambient => defs.push("PATCH_AMBIENT".into()),
            Patch::Neon => defs.push("PATCH_NEON".into()),
            Patch::Steam => defs.push("PATCH_STEAM".into()),
            Patch::Particles => defs.push("PATCH_PARTICLES".into()),
            Patch::Skid => defs.push("PATCH_SKID".into()),
            Patch::PoliceGlow => defs.push("PATCH_POLICEGLOW".into()),
        }
        if k.normal_map {
            defs.push("USE_NORMALMAP".into());
        }
        if k.alpha_map {
            defs.push("USE_ALPHAMAP".into());
        }
        if k.flat_shading {
            defs.push("FLAT_SHADED".into());
        }
        if matches!(k.patch, Patch::StreetFacade | Patch::StreetAtlas) {
            defs.push("STREET_WINDOWS".into());
        }
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
                three::CUSTOM_BLENDING => Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::OneMinusSrcAlpha,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::OneMinusSrcAlpha,
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
            ds.bias.slope_scale = k.depth_slope as f32;
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
        "MeshBasicMaterial" | "LineBasicMaterial" | "PointsMaterial" | "SpriteMaterial" => {
            Some(Model::Basic)
        }
        _ => None,
    }
}

/// The lighting model a material draws with: its built-in type's, or for
/// the `ShaderMaterial` kinds ported as blocks of `three_material.wgsl`
/// (TrafficStreams, SkyGlow: unlit, their own colour), basic.
pub fn model_of_material(m: &MaterialDesc) -> Option<Model> {
    model_of(&m.ty).or(match m.kind {
        MaterialKind::TrafficStreams
        | MaterialKind::Steam
        | MaterialKind::SkyGlow
        | MaterialKind::Surf
        | MaterialKind::LighthouseBeam
        | MaterialKind::Particles
        | MaterialKind::SkidMarks
        | MaterialKind::PoliceGlow => Some(Model::Basic),
        _ => None,
    })
}

/// A JS material as a [`ThreeMaterial`]: its built-in type with its patch
/// where [`Patch`] has it (a patched kind not ported yet draws without its
/// patch, as a stand-in). `images`
/// are the scene's textures as Bevy images; `instance_color` says the
/// geometry is an InstancedMesh with instanceColor.
pub fn three_material(
    scene: &Scene,
    m: &MaterialDesc,
    images: &[Option<Handle<Image>>],
    shared: &SharedImages,
    instance_color: bool,
) -> Option<ThreeMaterial> {
    let model = model_of_material(m)?;
    let num = |n: &str, d: f64| m.number(n).unwrap_or(d);
    let flag = |n: &str, d: bool| m.boolean(n).unwrap_or(d);
    let col = |n: &str, d: [f64; 3]| m.color(n).unwrap_or(d);
    let tex = |name: &str| {
        let i = m.texture(name)?;
        let h = images.get(i as usize).cloned().flatten()?;
        Some((h, scene.textures.get(i as usize)?))
    };
    let patch = Patch::of(m);
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
    // PointsMaterial's map is the point sprite, through its uvTransform
    // (map_particle_fragment), which is the same matrix.
    let map = tex("map");
    if let Some((_, t)) = &map {
        (p.map_t0, p.map_t1) = uv_rows(t);
    }
    // The patches' own textures and uniforms (WP 2.4).
    let mut detail = None;
    let mut aux = None;
    let mut photo = None;
    let mut loose = None;
    let mut normal_map = false;
    let vec = |n: &str| -> Option<Vec<f64>> {
        let v = m.get(n)?.get("vec")?.as_array()?;
        v.iter().map(serde_json::Value::as_f64).collect()
    };
    match patch {
        Patch::Terrain { packed, photo: ph } => {
            aux = tex("tRock").map(|(h, _)| h);
            if packed {
                detail = tex("tDetail").map(|(h, _)| h);
            }
            if ph {
                photo = tex("tPhoto").map(|(h, _)| h);
                loose = tex("tLoose").map(|(h, _)| h);
                if let Some(b) = vec("uPhotoBox") {
                    let g = |i: usize| b.get(i).copied().unwrap_or(0.0) as f32;
                    p.kind0 = Vec4::new(g(0), g(1), g(2), g(3));
                }
            }
        }
        Patch::Asphalt | Patch::Markings => {
            detail = tex("tDetail").map(|(h, _)| h);
        }
        Patch::Sea => {
            detail = tex("tFoam").map(|(h, _)| h);
            if let Some((h, t)) = tex("normalMap") {
                aux = Some(h);
                (p.normal_t0, p.normal_t1) = uv_rows(t);
                normal_map = true;
                let s = vec("normalScale").unwrap_or_else(|| vec![1.0, 1.0]);
                p.kind0 = Vec4::new(
                    s.first().copied().unwrap_or(1.0) as f32,
                    s.get(1).copied().unwrap_or(1.0) as f32,
                    0.0,
                    0.0,
                );
            }
        }
        Patch::Points { mode, .. } => {
            p.kind0 = Vec4::new(
                num("size", 1.0) as f32,
                m.number("uMinPx").unwrap_or(0.0) as f32,
                kind_opt(m, "rate").unwrap_or(1.0) as f32,
                kind_opt(m, "depth").unwrap_or(0.35) as f32,
            );
            if let PointsMode::Flicker { .. } = mode {
                // flickerPoints writes these into its GLSL with toFixed(3).
                let fixed3 = |x: f64| ((x * 1000.0).round() / 1000.0) as f32;
                let rate = kind_opt(m, "rate").unwrap_or(1.0);
                p.kind1 = Vec4::new(
                    fixed3(13.0 * rate),
                    fixed3(4.7 * rate),
                    fixed3(29.0 * rate),
                    fixed3(kind_opt(m, "blink").unwrap_or(0.0)),
                );
                p.kind0.w = fixed3(kind_opt(m, "depth").unwrap_or(0.35));
            }
        }
        Patch::Rock => {
            aux = tex("tRock").map(|(h, _)| h);
        }
        Patch::City => {
            // uMask in the detail slot; the map and emissive map are the
            // plain ones, sampled by cell (`atlasUv`).
            detail = tex("uMask").map(|(h, _)| h);
            p.kind0 = Vec4::new(m.number("uGround").unwrap_or(0.0) as f32, 0.0, 0.0, 0.0);
        }
        Patch::Traffic => {
            let u = |n: &str, d: f64| m.number(n).unwrap_or(d) as f32;
            p.kind0 = Vec4::new(
                u("uTime", 0.0),
                u("uNight", 1.0),
                u("uFogK", 0.0),
                u("uHalfH", 360.0),
            );
        }
        Patch::SkyGlow => {
            let u = |n: &str| m.number(n).unwrap_or(0.0) as f32;
            p.kind0 = Vec4::new(u("uK"), u("uGround"), u("uH"), 0.0);
        }
        Patch::Sprite { .. } => {
            p.kind0 = Vec4::new(num("rotation", 0.0) as f32, 0.0, 0.0, 0.0);
        }
        Patch::Stucco => {
            detail = tex("tGrain").map(|(h, _)| h);
        }
        Patch::Sandstone => {
            aux = tex("tRock").map(|(h, _)| h);
            detail = tex("tDetail").map(|(h, _)| h);
        }
        Patch::Pool => {
            p.kind0 = Vec4::new(
                m.number("uTime").unwrap_or(0.0) as f32,
                num("opacity", 1.0) as f32,
                0.0,
                0.0,
            );
        }
        Patch::FloodBeam => {
            p.kind0 = Vec4::new(num("opacity", 1.0) as f32, 0.0, 0.0, 0.0);
        }
        Patch::Ambient => {
            // ambientPatch writes its rgb into the GLSL with toFixed(4).
            let rgb = m
                .kind_opts
                .as_ref()
                .and_then(|o| o.get("rgb"))
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .map(|x| x.as_f64().unwrap_or(0.0))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let f4 = |i: usize| ((rgb.get(i).copied().unwrap_or(0.0) * 1e4).round() / 1e4) as f32;
            p.kind0 = Vec4::new(f4(0), f4(1), f4(2), 0.0);
        }
        Patch::Neon => {
            p.kind0 = Vec4::new(m.number("uNTime").unwrap_or(0.0) as f32, 0.0, 0.0, 0.0);
        }
        Patch::Steam => {
            let u = |n: &str, d: f64| m.number(n).unwrap_or(d) as f32;
            p.kind0 = Vec4::new(u("uTime", 0.0), u("uScale", 600.0), 0.0, 0.0);
        }
        Patch::Surf | Patch::Beam => {
            let u = |n: &str, d: f64| m.number(n).unwrap_or(d) as f32;
            p.kind0 = if patch == Patch::Surf {
                Vec4::new(u("uTime", 0.0), u("uBright", 1.0), u("uSwell", 1.0), 0.0)
            } else {
                Vec4::new(u("uStrength", 0.5), 0.0, 0.0, 0.0)
            };
            let c = col("uColor", [1.0; 3]);
            p.kind1 = Vec4::new(c[0] as f32, c[1] as f32, c[2] as f32, 0.0);
        }
        Patch::None => {
            // A plain material's tangent-space normal map (Valley's creek),
            // as the sea's (three's derivative frame).
            if lit && let Some((h, t)) = tex("normalMap") {
                aux = Some(h);
                (p.normal_t0, p.normal_t1) = uv_rows(t);
                normal_map = true;
                let s = vec("normalScale").unwrap_or_else(|| vec![1.0, 1.0]);
                p.kind0 = Vec4::new(
                    s.first().copied().unwrap_or(1.0) as f32,
                    s.get(1).copied().unwrap_or(1.0) as f32,
                    0.0,
                    0.0,
                );
            }
        }
        Patch::Particles => {
            let u = |n: &str, d: f64| m.number(n).unwrap_or(d) as f32;
            p.kind0 = Vec4::new(u("uScale", 400.0), 0.0, 0.0, 0.0);
        }
        Patch::PoliceGlow => {
            // No light slots yet (-1: dark until `play::police` gives the
            // car its two); `uMin`.
            p.kind0 = Vec4::new(-1.0, -1.0, num("uMin", 0.02) as f32, 0.0);
        }
        Patch::Shoulder
        | Patch::Skid
        | Patch::Reflector
        | Patch::Siding(_)
        | Patch::Container
        | Patch::StreetFacade
        | Patch::StreetAtlas => {}
    }
    // alphaMap (alphamap_fragment), in the photo slot (the terrain is the
    // only other user, and has none).
    let mut alpha_map = false;
    if !matches!(patch, Patch::Terrain { .. })
        && let Some((h, t)) = tex("alphaMap")
    {
        photo = Some(h);
        (p.alpha_t0, p.alpha_t1) = uv_rows(t);
        alpha_map = true;
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
    } else if given_blending == three::CUSTOM_BLENDING
        && !(num("blendSrc", 204.0) == 201.0 && num("blendDst", 205.0) == 205.0)
    {
        // The one custom blending the exports use is One, OneMinusSrcAlpha
        // (the steam, D501); another would draw as normal blending.
        three::NORMAL_BLENDING
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
        instanced: false,
        side,
        shadow_side,
        blending,
        depth_write: flag("depthWrite", true),
        depth_test: flag("depthTest", true),
        depth_bias,
        depth_slope: 0,
        sort_rank: 0,
        patch,
        normal_map,
        alpha_map,
        flat_shading: flag("flatShading", false),
    };
    Some(ThreeMaterial {
        params: p,
        map: map.map(|(h, _)| h),
        emissive_map: emissive_map.map(|(h, _)| h),
        detail,
        aux,
        photo,
        loose,
        globals: shared.globals.clone(),
        env: shared.env.clone(),
        key,
    })
}
