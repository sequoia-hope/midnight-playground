// The material kinds on three_std (SPEC 6.2): the plain ones (Standard,
// Physical, Lambert, Basic, Line), three.js r180's meshphysical, meshlambert
// and meshbasic programs for the features the game uses; the patched kinds
// of WP 2.4 (Terrain, Asphalt, Shoulder, Markings, Sea), each JS
// `onBeforeCompile` patch ported as a block at the point of three's program
// where the JS splices it (the chunk it replaces is named on each block);
// and three's points program (Points, GlowPoints, FlickerPoints) on
// camera-facing quads. Lighting runs in view
// space as three's does (the geometry roughness term reads view-space normal
// derivatives). Output is linear HDR; the post chain (`render::post`) adds
// the bloom and applies three's ACES and sRGB.
//
// Shader defs (from `ThreeKey`): LIT_PHYSICAL | LIT_LAMBERT | (neither:
// basic); PHYSICAL (MeshPhysicalMaterial: IOR, specular); USE_CLEARCOAT;
// USE_SHEEN; USE_MAP; USE_EMISSIVEMAP; USE_COLOR (vertex colours);
// INSTANCE_COLOR (three's instanceColor, in the instance's tag); USE_FOG;
// ALPHA_TEST; OPAQUE; DOUBLE_SIDED; FLIP_SIDED (BackSide); USE_ENV;
// USE_NORMALMAP (tangent space, three's derivative frame). Patches:
// PATCH_TERRAIN (+ TERRAIN_PACKED, MR_PHOTO), PATCH_ASPHALT, PATCH_SHOULDER,
// PATCH_MARKINGS, PATCH_SEA; POINTS (+ USE_SIZEATTENUATION, POINTS_GLOW,
// POINTS_FLICKER, FLICKER_BLINK). WP 3.9: PATCH_ROCK, PATCH_REFLECTOR,
// SIDING_SIDING | SIDING_BOARDS | SIDING_ROOF, PATCH_CITY, PATCH_TRAFFIC and
// PATCH_SKYGLOW (two ShaderMaterials, their own fragment), SPRITE;
// USE_ALPHAMAP. VERTEX_EXTRA: the patch's own attribute
// (`convert::ATTRIBUTE_EXTRA`), VERTEX_EXTRA2 the traffic streams' second
// one. A material's animated values come from its block in the globals
// when it has one (`material.slots.x`, D490). MR_INSTANCED: an InstancedMesh drawn as one
// entity (`render::instancing`, D450): the world matrix, instanceColor and
// receiveShadow come from the instance stream, not Bevy's mesh uniform.
//
// WGSL wants implicit-derivative texture samples and derivatives in uniform
// control flow, so where a JS patch samples inside a branch (the terrain's
// close grain, rock faces, varnish, photo) the sample is taken before the
// branch and the branch only chooses: the same value wherever a 2×2 quad
// takes the same side, which the JS comments make sure of.

#import bevy_pbr::{
    mesh_functions,
    mesh_bindings::mesh,
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    mesh_view_bindings::{
        view, lights, directional_shadow_textures,
        directional_shadow_textures_comparison_sampler,
    },
}
#import mr::three_std as t
#import mr::three_globals as g

struct ThreeParams {
    // color.rgb, opacity
    diffuse: vec4<f32>,
    // emissive × emissiveIntensity, alphaTest
    emissive: vec4<f32>,
    // roughness, metalness, clearcoat, clearcoatRoughness
    pbr: vec4<f32>,
    // sheenColor × sheen, sheenRoughness
    sheen: vec4<f32>,
    // specularColor, specularIntensity
    specular: vec4<f32>,
    // ior, envMapIntensity (unused: the scene's intensity rules), -, -
    physical: vec4<f32>,
    // map uv transform rows (u' = dot(row0.xyz, (u, v, 1)), v' likewise)
    map_t0: vec4<f32>,
    map_t1: vec4<f32>,
    emissive_t0: vec4<f32>,
    emissive_t1: vec4<f32>,
    // normalMap uv transform rows
    normal_t0: vec4<f32>,
    normal_t1: vec4<f32>,
    // Terrain: uPhotoBox. Sea: normalScale (xy). Points: size, uMinPx, -,
    // the flicker's depth.
    kind0: vec4<f32>,
    // Points: the flicker's 13 × rate, 4.7 × rate, 29 × rate, blink (as the
    // JS writes them into its GLSL, to three decimals).
    kind1: vec4<f32>,    // world.nightMaterials (D455): emissiveIntensity by day and by night,
    // and 1 in z when the material follows nightfall; 2 in z: the
    // intensity is light slot w of the globals (D456).
    night: vec4<f32>,
    // alphaMap uv transform rows
    alpha_t0: vec4<f32>,
    alpha_t1: vec4<f32>,
    // y: 1 when `kind0` holds the kind's uniforms (the material test
    // scenes); z: the scene material's index + 1, which finds its
    // animation block through the globals' block map (0: not a scene
    // material; `crate::animate`, D490).
    slots: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: ThreeParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var map_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var map_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var emissive_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var emissive_sampler: sampler;
// tDetail (the sea's tFoam)
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var detail_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var detail_sampler: sampler;
// tRock (the sea's normalMap)
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var aux_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var aux_sampler: sampler;
// tPhoto, tLoose
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var photo_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var photo_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(15) var loose_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(16) var loose_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var globals_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var env_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var env_sampler: sampler;

// (Not `gl`: naga's GLSL writer turns it into `gl_1`, a name GLSL reserves,
// and the WebGL2 build's shaders fail to compile.)
fn globals_at(i: i32) -> vec4<f32> {
    return textureLoad(globals_texture, vec2<i32>(i, 0), 0);
}

// The material's animation block (D490): what the scenery's animators
// (`World.update`'s updaters) move each frame, written into the globals by
// `crate::animate` instead of into the material. Texel 0: the colour (rgb,
// w 1 when set); 1: the emissive colour (likewise); 2: emissiveIntensity
// (x, y 1 when set), the sprite's rotation (z, w 1 when set); 3: the map's
// offset since the export (xy), the normal map's (zw); 4: the kind's
// uniforms. The material finds its block through the block map, by its
// scene index, so a block is made without editing the material.
fn block_base() -> i32 {
    let id = i32(material.slots.z) - 1;
    if (id < 0) {
        return 0;
    }
    return i32(globals_at(g::G_BLOCK_MAP + id / 4)[id % 4]);
}

fn has_block() -> bool {
    return block_base() > 0;
}

fn block_at(k: i32) -> vec4<f32> {
    return globals_at(block_base() + k);
}

// The kind's animated uniforms: from the block when the animators run,
// else what the JS updater would set, from the scene-wide state (D293).
fn kind_uniforms() -> vec4<f32> {
    if (has_block()) {
        return block_at(4);
    }
    // Fixed values in `kind0` (the material test scenes' overrides).
    if (material.slots.y > 0.5) {
        return material.kind0;
    }
    let n = globals_at(g::G_SKY_PARAMS).x;
#ifdef PATCH_TRAFFIC
    // City.js buildTraffic's updater: uTime += dt, uNight =
    // smoothstep(0.2, 0.7, night), uFogK = world.scene's fog (it has none:
    // 0), uHalfH = the drawing buffer's height / 2.
    return vec4<f32>(globals_at(g::G_ANIM2).z, t::smooth_step(0.2, 0.7, n), 0.0, view.viewport.w * 0.5);
#else ifdef PATCH_SKYGLOW
    // uK = 0.2 × smoothstep(0.3, 0.8, night).
    return vec4<f32>(0.2 * t::smooth_step(0.3, 0.8, n), 0.0, 0.0, 0.0);
#else ifdef PATCH_ASPHALT
    // Road.setNight: uWet.
    return vec4<f32>(globals_at(g::G_ANIM).x, 0.0, 0.0, 0.0);
#else ifdef PATCH_SURF
    // Coast.js: uTime += dt, uBright = lerp(1, 0.32, night).
    return vec4<f32>(globals_at(g::G_ANIM2).z, mix(1.0, 0.32, n), 0.0, 0.0);
#else ifdef PATCH_BEAM
    // uStrength = 0.03 + 0.32 × smoothstep(0.2, 0.8, night).
    return vec4<f32>(0.03 + 0.32 * t::smooth_step(0.2, 0.8, n), 0.0, 0.0, 0.0);
#else ifdef PATCH_POOL
    // Desert.js animate: glowTime = T; poolMat.opacity = smoothstep(0.15,
    // 0.7, night).
    return vec4<f32>(globals_at(g::G_ANIM2).z, t::smooth_step(0.15, 0.7, n), 0.0, 0.0);
#else ifdef PATCH_FLOODBEAM
    // beamMat.opacity = 0.13 × smoothstep(0.3, 0.8, night).
    return vec4<f32>(0.13 * t::smooth_step(0.3, 0.8, n), 0.0, 0.0, 0.0);
#else
    // GlowPoints' uFogK: world.scene's fog, which it has not.
    return vec4<f32>(0.0);
#endif
}

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
#ifdef VERTEX_NORMALS
    @location(1) normal: vec3<f32>,
#endif
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
#ifdef VERTEX_EXTRA
    @location(8) extra: vec4<f32>,
#endif
#ifdef VERTEX_EXTRA2
    @location(14) extra2: vec4<f32>,
#endif
#ifdef MR_INSTANCED
    // The instance stream: the InstancedMesh's matrixWorld × instanceMatrix
    // (multiplied on the CPU), then instanceColor (white without one) and
    // receiveShadow.
    @location(9) i_col0: vec4<f32>,
    @location(10) i_col1: vec4<f32>,
    @location(11) i_col2: vec4<f32>,
    @location(12) i_col3: vec4<f32>,
    @location(13) i_color: vec4<f32>,
    // The geometry's instance-rate attributes (D499).
    @location(15) i_extra: vec4<f32>,
#endif
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    // mvPosition.xyz (three's vViewPosition is its negation)
    @location(0) view_pos: vec3<f32>,
    // vNormal, view space
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // vColor (vertex colour × instance colour)
    @location(3) color: vec4<f32>,
    // vDirectionalShadowCoord[0]
    @location(4) shadow_coord: vec4<f32>,
    @location(5) @interpolate(flat) instance_index: u32,
    // The world position (the terrain's vTPos, the sea's vSeaPos).
    @location(6) world_pos: vec3<f32>,
    // The terrain's vTNorm: mat3(modelMatrix) × objectNormal, normalised.
    @location(7) world_normal: vec3<f32>,
    // vSurf (x), vLane (xyz), vDepth (x); points: vGA or vFl (x).
    @location(8) extra: vec4<f32>,
#ifdef MR_INSTANCED
    // receiveShadow (1 or 0), in place of the mesh uniform's flag.
    @location(9) @interpolate(flat) receive: f32,
#endif
};

// The largest point three's WebGL draws: gl_PointSize is clamped to
// ALIASED_POINT_SIZE_RANGE (Chrome on the dev machine, ANGLE on Vulkan:
// 1 to 2047.9375; phones report less, which only matters for points over
// a few hundred pixels).
const MAX_POINT_SIZE: f32 = 2047.9375;

fn unpack_tint(tag: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(tag & 1023u),
        f32((tag >> 10u) & 1023u),
        f32((tag >> 20u) & 1023u),
    ) / 511.5;
}

#ifdef MR_INSTANCED
// The normal through the instance's matrix: its inverse transpose (the
// cofactors over the determinant, whose sign is all that survives the
// normalisation), as Bevy's mesh_normal_local_to_world uses for an entity.
fn instance_normal(world_from_local: mat4x4<f32>, normal: vec3<f32>) -> vec3<f32> {
    if (all(normal == vec3<f32>(0.0))) {
        return normal;
    }
    let a = world_from_local[0].xyz;
    let b = world_from_local[1].xyz;
    let c = world_from_local[2].xyz;
    let cof = mat3x3<f32>(cross(b, c), cross(c, a), cross(a, b));
    let s = select(1.0, -1.0, dot(a, cof[0]) < 0.0);
    return normalize(cof * normal * s);
}
#endif

@vertex
fn vertex(v: Vertex) -> VOut {
    var out: VOut;
    var local_pos = v.position;
#ifdef PATCH_TRAFFIC
    // City.js buildTraffic: t = fract(aPar.x + uTime × aPar.y), and the
    // light slides along its block: position + aDir × t.
    let tu = kind_uniforms();
    let a_par = vec3<f32>(v.extra.x, v.extra.y, v.extra2.w);
    let traffic_t = fract(a_par.x + tu.x * a_par.y);
    local_pos += v.extra2.xyz * traffic_t;
#endif
#ifdef MR_INSTANCED
    let world_from_local = mat4x4<f32>(v.i_col0, v.i_col1, v.i_col2, v.i_col3);
    let world = world_from_local * vec4<f32>(local_pos, 1.0);
    out.receive = v.i_color.w;
#else
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(local_pos, 1.0));
#endif
    out.view_pos = (view.view_from_world * world).xyz;
    out.clip = view.clip_from_world * world;
    out.instance_index = v.instance_index;
    out.world_pos = world.xyz;
    var extra = vec4<f32>(0.0);
#ifdef VERTEX_EXTRA
    extra = v.extra;
#endif
    out.extra = extra;
#ifdef PATCH_CONTAINER
#ifdef MR_INSTANCED
    // containerMaterial: the instance's atlas row (aVar).
    out.extra.x = v.i_extra.x;
#endif
#endif
#ifdef PATCH_SURF
    // foamVert: vPh from the instance's translation (the rings; the
    // InstancedMesh sits at the origin, so its world matrix is the
    // instance's); 0 for the shore ribbon.
#ifdef MR_INSTANCED
    out.extra.x = world_from_local[3].x * 0.13 + world_from_local[3].z * 0.071;
#endif
#endif
#ifdef VERTEX_NORMALS
#ifdef MR_INSTANCED
    var n = instance_normal(world_from_local, v.normal);
#else
    var n = mesh_functions::mesh_normal_local_to_world(v.normal, v.instance_index);
#endif
    out.world_normal = normalize((world_from_local * vec4<f32>(v.normal, 0.0)).xyz);
#else
    var n = vec3<f32>(0.0, 0.0, 1.0);
    out.world_normal = n;
#endif
#ifdef FLIP_SIDED
    n = -n;
#endif
    out.normal = normalize((view.view_from_world * vec4<f32>(n, 0.0)).xyz);
#ifdef VERTEX_UVS_A
    out.uv = v.uv;
#else
    out.uv = vec2<f32>(0.0);
#endif
    var color = vec4<f32>(1.0);
#ifdef VERTEX_COLORS
    color = v.color;
#endif
#ifdef INSTANCE_COLOR
#ifdef MR_INSTANCED
    color = vec4<f32>(color.rgb * v.i_color.rgb, color.a);
#else
    color = vec4<f32>(color.rgb * unpack_tint(mesh_functions::get_tag(v.instance_index)), color.a);
#endif
#endif
    out.color = color;
    // shadowmap_vertex: the world position pushed out along the (normalised)
    // world normal by the normal bias, through three's shadowMatrix.
    let shadow = globals_at(g::G_SHADOW);
    let shadow_world = vec4<f32>(world.xyz + normalize(n) * shadow.y, 1.0);
    let m = mat4x4<f32>(globals_at(g::G_SHADOW_M), globals_at(g::G_SHADOW_M + 1), globals_at(g::G_SHADOW_M + 2), globals_at(g::G_SHADOW_M + 3));
    out.shadow_coord = m * shadow_world;
#ifdef POINTS
    points_vertex(&out, extra);
#endif
#ifdef PATCH_TRAFFIC
    traffic_vertex(&out, extra, a_par, traffic_t, tu);
#endif
#ifdef PATCH_POOL
    // flickerPools at fog_vertex: vFl from the instance's ph and fl.
#ifdef MR_INSTANCED
    {
        let pu = kind_uniforms().x;
        let ph = v.i_extra.x;
        let fl = v.i_extra.y;
        let f_ = sin(pu * 13.0 + ph * 6.3) * sin(pu * 4.7 + ph * 2.1) + 0.4 * sin(pu * 29.0 + ph * 3.7);
        out.extra.x = 1.0 - fl + fl * clamp(0.5 + 0.5 * f_, 0.0, 1.0);
    }
#else
    // Drawn without the instance stream (the material test scene): no
    // flicker (fl 0).
    out.extra.x = 1.0;
#endif
#endif
#ifdef PATCH_FLOODBEAM
    // Desert.js's beam cones at fog_vertex: vFace = |dot(normalize(
    // normalMatrix · mat3(instanceMatrix) · normal), normalize(-mvPosition))|.
    out.extra.x = abs(dot(normalize((view.view_from_world * vec4<f32>(out.world_normal, 0.0)).xyz), normalize(-out.view_pos)));
#endif
#ifdef PATCH_BEAM
    // beamVert: vAlong = 1 - uv.y; vV = normalize(-mvPosition), carried as
    // it is interpolated (in `world_normal`, which the beam has no use for).
    out.extra.x = 1.0 - out.uv.y;
    out.world_normal = normalize(-out.view_pos);
#endif
#ifdef SPRITE
    sprite_vertex(&out, world_from_local, v.position);
#endif
    return out;
}

// A point of `point_size` pixels as its quad: the centre outside the clip
// volume draws nothing (GL ES 3.0 2.13.1; Bevy's depth is reversed, so in
// front of near is z <= w), else the corner (extra.zw) is pushed out in
// clip space, with gl_PointCoord's corner as the uv (y down).
fn point_quad(out: ptr<function, VOut>, point_size: f32, corner: vec2<f32>) {
    var clip = (*out).clip;
    if (any(abs(clip.xy) > vec2<f32>(clip.w)) || clip.w <= 0.0 || clip.z > clip.w) {
        (*out).clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return;
    }
    clip = vec4<f32>(clip.xy + corner * point_size / view.viewport.zw * clip.w, clip.zw);
    (*out).clip = clip;
    (*out).uv = corner * 0.5 + 0.5;
}

#ifdef PATCH_TRAFFIC
// City.js buildTraffic's vertex shader after the slide: the size from the
// distance, fades at the block ends and near the camera, head or tail
// colour (vCol in `color`, vA in `extra.x`).
fn traffic_vertex(out: ptr<function, VOut>, extra: vec4<f32>, a_par: vec3<f32>, tt: f32, u: vec4<f32>) {
    let d = max(-(*out).view_pos.z, 0.1);
    // projectionMatrix[1][1]: Bevy's perspective has three's y scale.
    let raw = 1.5 * view.clip_from_view[1][1] * u.w / d;
    let point_size = clamp(max(raw, 1.5), 1.0, MAX_POINT_SIZE);
    let v_a = sqrt(min(1.0, raw / 1.5)) * exp(-d * u.z) * t::smooth_step(25.0, 70.0, d)
        * t::smooth_step(0.0, 0.06, tt) * t::smooth_step(1.0, 0.94, tt);
    (*out).color = vec4<f32>(select(vec3<f32>(1.0, 0.88, 0.66), vec3<f32>(1.0, 0.1, 0.04), a_par.z > 0.5), 1.0);
    (*out).extra = vec4<f32>(v_a, 0.0, 0.0, 0.0);
    point_quad(out, point_size, extra.zw);
}
#endif

#ifdef SPRITE
// sprite_vert: the quad in view space around the sprite's centre, scaled
// by the node's scale, turned by the material's rotation (center 0.5, 0.5).
fn sprite_vertex(out: ptr<function, VOut>, model: mat4x4<f32>, position: vec3<f32>) {
    var mv = view.view_from_world * model[3];
    var scale = vec2<f32>(length(model[0].xyz), length(model[1].xyz));
#ifndef USE_SIZEATTENUATION
    scale *= -mv.z;
#endif
    let aligned = position.xy * scale;
    var rotation = material.kind0.x;
    if (has_block() && block_at(2).w > 0.5) {
        rotation = block_at(2).z;
    }
    let c = cos(rotation);
    let s = sin(rotation);
    mv = vec4<f32>(mv.xy + vec2<f32>(c * aligned.x - s * aligned.y, s * aligned.x + c * aligned.y), mv.zw);
    (*out).view_pos = mv.xyz;
    (*out).clip = view.clip_from_view * mv;
    (*out).world_pos = (view.world_from_view * mv).xyz;
}
#endif

#ifdef POINTS
// points_vert, with GlowPoints' and FlickerPoints' patches: the point's size
// in pixels, then the quad pushed out from the centre in clip space by its
// corner (extra.zw), with gl_PointCoord's corner as the uv (y down, as
// map_particle_fragment flips it back: uv = (pc.x, 1 - pc.y)).
fn points_vertex(out: ptr<function, VOut>, extra: vec4<f32>) {
    let mv_z = (*out).view_pos.z;
    let pixel_ratio = globals_at(g::G_ANIM2).w;
    // refreshUniformsPoints: size × pixelRatio; scale = height × 0.5 (CSS).
    var point_size = material.kind0.x * pixel_ratio;
    var aux = 1.0;
#ifdef POINTS_GLOW
    // gl_PointSize = size * gsize;
    point_size = point_size * extra.x;
#endif
#ifdef USE_SIZEATTENUATION
    let scale = view.viewport.w / pixel_ratio * 0.5;
    point_size *= scale / -mv_z;
#endif
#ifdef POINTS_GLOW
    // At logdepthbuf_vertex: a minimum pixel size, dimmer when clamped, and
    // a gentler fog (uFogK = fog density × 0.4, City.js's updater).
    let g_raw = point_size;
    point_size = max(g_raw, material.kind0.y);
    // uFogK: world.scene's fog density × 0.4, which is 0 (City.js reads the
    // root group's fog, and it has none).
    let fog_k = kind_uniforms().x;
    aux = sqrt(g_raw / point_size) * exp(-max(-mv_z, 0.0) * fog_k);
#endif
#ifdef POINTS_FLICKER
    // At fog_vertex.
    let u_time = globals_at(g::G_ANIM2).z;
    let ph = extra.x;
#ifdef FLICKER_BLINK
    aux = step(0.5, fract(u_time * material.kind1.w + ph * 0.3333));
#else
    let f_ = sin(u_time * material.kind1.x + ph * 6.3) * sin(u_time * material.kind1.y + ph * 2.1)
        + 0.4 * sin(u_time * material.kind1.z + ph * 3.7);
    let depth = material.kind0.w;
    aux = 1.0 - depth + depth * clamp(0.5 + 0.5 * f_, 0.0, 1.0);
#endif
    point_size *= 0.75 + 0.25 * aux;
#endif
    point_size = clamp(point_size, 1.0, MAX_POINT_SIZE);
    var clip = (*out).clip;
    // A point whose centre is outside the clip volume is not drawn (GL ES
    // 3.0 2.13.1); Bevy's depth is reversed, so in front of near is z <= w.
    if (any(abs(clip.xy) > vec2<f32>(clip.w)) || clip.w <= 0.0 || clip.z > clip.w) {
        (*out).clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        return;
    }
    let corner = extra.zw;
    clip = vec4<f32>(clip.xy + corner * point_size / view.viewport.zw * clip.w, clip.zw);
    (*out).clip = clip;
    (*out).uv = corner * 0.5 + 0.5;
    (*out).extra = vec4<f32>(aux, 0.0, 0.0, 0.0);
}
#endif

// shadowmap_pars_fragment's texture2DCompare on Bevy's shadow map: the map
// is rendered over three's shadow camera box (`render::shadow`), with
// reversed depth and rows top first, so three's texel and depth are
// recovered from it.
fn texture2DCompare(uv: vec2<f32>, compare: f32, size: f32, layer: i32) -> f32 {
    let n = i32(size);
    let ix = clamp(i32(floor(uv.x * size)), 0, n - 1);
    let iy = clamp(i32(floor(uv.y * size)), 0, n - 1);
#ifdef NO_ARRAY_TEXTURES_SUPPORT
    // WebGL2 (GLSL ES 3.0) cannot texelFetch a depth texture, so the same
    // texel is read through Bevy's comparison sampler (GreaterEqual) at its
    // centre, where the bilinear weights are (1, 0, 0, 0): it answers
    // `1 - compare >= stored`, which is `compare <= depth`, three's step.
    let centre = (vec2<f32>(f32(ix), f32(n - 1 - iy)) + 0.5) / size;
    return textureSampleCompareLevel(
        directional_shadow_textures,
        directional_shadow_textures_comparison_sampler,
        centre,
        1.0 - compare,
    );
#else
    let stored = textureLoad(directional_shadow_textures, vec2<i32>(ix, n - 1 - iy), layer, 0);
    let depth = 1.0 - stored;
    return step(compare, depth);
#endif
}

// getShadow with SHADOWMAP_TYPE_PCF_SOFT.
fn getShadow(shadow_coord_in: vec4<f32>) -> f32 {
    let params = globals_at(g::G_SHADOW);
    let shadow_map_size = params.z;
    let layer = i32(lights.directional_lights[0].depth_texture_base_index);
    var shadow = 1.0;
    var coord = shadow_coord_in.xyz / shadow_coord_in.w;
    coord.z += params.x;
    let in_frustum = coord.x >= 0.0 && coord.x <= 1.0 && coord.y >= 0.0 && coord.y <= 1.0;
    let frustum_test = in_frustum && coord.z <= 1.0;
    if (frustum_test) {
        let texel_size = vec2<f32>(1.0) / shadow_map_size;
        let dx = texel_size.x;
        let dy = texel_size.y;
        var uv = coord.xy;
        let f = fract(uv * shadow_map_size + 0.5);
        uv -= f * texel_size;
        let z = coord.z;
        let s = shadow_map_size;
        shadow = (
            texture2DCompare(uv, z, s, layer) +
            texture2DCompare(uv + vec2<f32>(dx, 0.0), z, s, layer) +
            texture2DCompare(uv + vec2<f32>(0.0, dy), z, s, layer) +
            texture2DCompare(uv + texel_size, z, s, layer) +
            mix(texture2DCompare(uv + vec2<f32>(-dx, 0.0), z, s, layer),
                texture2DCompare(uv + vec2<f32>(2.0 * dx, 0.0), z, s, layer),
                f.x) +
            mix(texture2DCompare(uv + vec2<f32>(-dx, dy), z, s, layer),
                texture2DCompare(uv + vec2<f32>(2.0 * dx, dy), z, s, layer),
                f.x) +
            mix(texture2DCompare(uv + vec2<f32>(0.0, -dy), z, s, layer),
                texture2DCompare(uv + vec2<f32>(0.0, 2.0 * dy), z, s, layer),
                f.y) +
            mix(texture2DCompare(uv + vec2<f32>(dx, -dy), z, s, layer),
                texture2DCompare(uv + vec2<f32>(dx, 2.0 * dy), z, s, layer),
                f.y) +
            mix(mix(texture2DCompare(uv + vec2<f32>(-dx, -dy), z, s, layer),
                    texture2DCompare(uv + vec2<f32>(2.0 * dx, -dy), z, s, layer),
                    f.x),
                mix(texture2DCompare(uv + vec2<f32>(-dx, 2.0 * dy), z, s, layer),
                    texture2DCompare(uv + vec2<f32>(2.0 * dx, 2.0 * dy), z, s, layer),
                    f.x),
                f.y)
        ) * (1.0 / 9.0);
    }
    return mix(1.0, shadow, params.w);
}

fn view_dir_of(world_dir: vec3<f32>) -> vec3<f32> {
    return normalize((view.view_from_world * vec4<f32>(world_dir, 0.0)).xyz);
}

// inverseTransformDirection( dir, viewMatrix )
fn world_dir_of(view_dir: vec3<f32>) -> vec3<f32> {
    return normalize((view.world_from_view * vec4<f32>(view_dir, 0.0)).xyz);
}

struct Incident {
    color: vec3<f32>,
    direction: vec3<f32>,
    visible: bool,
};

struct Reflected {
    direct_diffuse: vec3<f32>,
    direct_specular: vec3<f32>,
    indirect_diffuse: vec3<f32>,
    indirect_specular: vec3<f32>,
    clearcoat_specular: vec3<f32>,
    sheen_specular: vec3<f32>,
};

// RE_Direct_Physical (with the clearcoat and sheen terms, which three keeps
// in globals), or RE_Direct_Lambert.
fn re_direct(light: Incident, normal: vec3<f32>, view_dir: vec3<f32>, m: t::PhysicalMaterial, r: ptr<function, Reflected>) {
    let dotNL = saturate(dot(normal, light.direction));
    let irradiance = dotNL * light.color;
#ifdef LIT_PHYSICAL
#ifdef USE_CLEARCOAT
    // geometryClearcoatNormal is the unperturbed normal (no clearcoat map).
    let dotNLcc = saturate(dot(normal, light.direction));
    let cc_irradiance = dotNLcc * light.color;
    (*r).clearcoat_specular += cc_irradiance * t::BRDF_GGX(light.direction, view_dir, normal, m.f0_clearcoat, m.f90_clearcoat, m.clearcoat_roughness);
#endif
#ifdef USE_SHEEN
    (*r).sheen_specular += irradiance * t::BRDF_Sheen(light.direction, view_dir, normal, m.sheen_color, m.sheen_roughness);
#endif
    (*r).direct_specular += irradiance * t::BRDF_GGX(light.direction, view_dir, normal, m.specular_color, m.f90_specular, m.roughness);
#endif
    (*r).direct_diffuse += irradiance * t::BRDF_Lambert(m.diffuse_color);
}

// ── WP 2.4 patches ──────────────────────────────────────────────────────

#ifdef PATCH_TERRAIN
// TerrainMesh.js patchTriplanar, at map_fragment: what the ground is and
// its detail, as the factors the later chunks take.
struct Terrain {
    // diffuseColor.rgb *= tex * 1.3 (map_fragment)
    tex: vec3<f32>,
    // diffuseColor.rgb *= vc (color_fragment)
    vc: vec3<f32>,
    // the bump height (normal_fragment_maps)
    h: f32,
    // for the roughness tweak (roughnessmap_fragment)
    sandy: f32,
    fine_n: f32,
};

#ifdef TERRAIN_PACKED
fn terrain_map(p: vec3<f32>, t_norm: vec3<f32>, v_color: vec3<f32>, v_surf: f32) -> Terrain {
    var o: Terrain;
    let mr_dist = length(p - view.world_position);
    let mr_near = 1.0 - t::smooth_step(14.0, 55.0, mr_dist);
    let mr_mid = 1.0 - t::smooth_step(90.0, 420.0, mr_dist);
    let tn = normalize(t_norm);
    var tw = pow(abs(tn), vec3<f32>(4.0));
    tw /= (tw.x + tw.y + tw.z);
    let dA = textureSample(detail_texture, detail_sampler, p.xz / 9.0);
    let dB = textureSample(detail_texture, detail_sampler, p.xz / 61.0 + 0.37);
    let dM = textureSample(detail_texture, detail_sampler, p.xz / 900.0 + 0.11);
    // Close grain and the side (rock) samples only where they show; both
    // conditions are smooth over the screen so the branches stay coherent.
    let dC_sample = textureSample(detail_texture, detail_sampler, p.xz / 2.3 + 0.61);
    let dC = select(vec4<f32>(0.5), dC_sample, mr_near > 0.0);
    let sx_sample = textureSample(aux_texture, aux_sampler, vec2<f32>(p.z / 23.0, p.y / 15.0 + dB.r * 0.35)).rgb;
    let sz_sample = textureSample(aux_texture, aux_sampler, vec2<f32>(p.x / 23.0, p.y / 15.0 + dB.a * 0.35)).rgb;
    let vs = textureSample(detail_texture, detail_sampler, vec2<f32>((p.x + p.z) / 7.0, p.y / 55.0));
    var sx = vec3<f32>(0.85);
    var sz = vec3<f32>(0.85);
    var varnish = 0.0;
    if (tw.x + tw.z > 0.004) {
        sx = sx_sample;
        sz = sz_sample;
        // Desert varnish on red rock: dark streaks running down the face
        // (the noise stretched vertically), only on warm-red faces.
        let red = t::smooth_step(0.35, 0.65, (v_color.r - v_color.g) / (v_color.r + 0.02));
        if (red > 0.0) {
            varnish = t::smooth_step(0.45, 0.8, vs.a) * red;
        }
    }
    // What kind of ground this is, read from the vertex colour (or the
    // photo, where there is one).
    var vc = v_color;
    var asph = 0.0;
#ifdef MR_PHOTO
    let box = material.kind0;
    let mr_pu = (p.xz - box.xy) / (box.zw - box.xy);
    let mr_pin = t::smooth_step(0.0, 80.0, min(min(p.x - box.x, box.z - p.x), min(p.z - box.y, box.w - p.z)));
    let photo = textureSample(photo_texture, photo_sampler, mr_pu).rgb;
    let loose = textureSample(loose_texture, loose_sampler, mr_pu).r;
    if (mr_pin > 0.0) {
        vc = mix(v_color, photo, mr_pin);
        asph = (1.0 - loose) * mr_pin;
    }
#endif
    let grassy = t::smooth_step(0.08, 0.32, (vc.g - max(vc.r, vc.b)) / (vc.g + 0.02));
    let sandy = t::smooth_step(0.3, 0.6, (vc.r - vc.b) / (vc.r + 0.02)) * (1.0 - grassy);
    let fine_n = mix(dA.g, dC.g, mr_near);
    // Grass: tufts and darker clumps; sand: soft grain and wind ripples;
    // dirt/gravel (the rest): pebbles with dark gaps.
    let grass_t = mix(1.0, 0.72 + 0.52 * fine_n, mr_mid) * (0.9 + 0.2 * dB.a);
    let rip = sin(dot(p.xz, vec2<f32>(1.9, 1.1)) + dB.r * 9.0) * 0.5 + 0.5;
    let sand_t = 1.0 + ((fine_n - 0.5) * 0.14 + (rip - 0.5) * 0.08 * mr_near) * mr_mid;
    let dirt_t = mix(1.0, 0.9 + (dC.b - 0.45) * 0.3 * mr_near + (fine_n - 0.5) * 0.22, mr_mid);
    // Paved yards and lots: aggregate speckle, oil stains and slab joints
    // (5 m concrete panels) that fade out before they can alias.
    let paved = clamp(v_surf, 0.0, 1.0);
    let slab = p.xz / vec2<f32>(5.0, 4.0);
    let sj = abs(fract(slab) - 0.5);
    let jw = fwidth(slab.x) + fwidth(slab.y);
    let joint = 1.0 - (1.0 - t::smooth_step(0.0, jw * 1.5 + 0.012, 0.5 - max(sj.x, sj.y))) * 0.3 * (1.0 - t::smooth_step(0.08, 0.3, jw));
    let slab_tone = 0.94 + 0.12 * fract(sin(dot(floor(slab), vec2<f32>(12.9898, 78.233))) * 43758.5453);
    let paved_t = mix(1.0, (0.92 + (fine_n - 0.5) * 0.18) * slab_tone * joint, mr_mid) * (1.0 - t::smooth_step(0.55, 0.8, dB.a) * 0.18);
    // Asphalt (paved run-off, service roads): fine, even grain.
    let asph_t = mix(1.0, 0.95 + (fine_n - 0.5) * 0.14 + (dC.b - 0.5) * 0.06 * mr_near, mr_mid);
    asph *= 1.0 - paved;
    let ground_t = mix(mix(mix(mix(dirt_t, sand_t, sandy), grass_t, grassy), asph_t, asph), paved_t, paved);
    let top = vec3<f32>((dA.r * 0.6 + dB.r * 0.6) * ground_t);
    var side = mix(vec3<f32>(0.85), sx * tw.x / max(tw.x + tw.z, 1e-3) + sz * tw.z / max(tw.x + tw.z, 1e-3), 0.75) * (0.75 + 0.5 * dA.r);
    side *= 1.0 - varnish * 0.38;
    var tex = top * tw.y + side * (tw.x + tw.z);
    // Macro: broad brightness and hue drift so the same parcel of colour
    // doesn't repeat, strongest in the mid distance where tiling shows.
    let mac = dM.a - 0.5;
    tex *= (1.0 + mac * 0.28) * vec3<f32>(1.0 + mac * 0.06, 1.0, 1.0 - mac * 0.1);
    o.tex = tex * 1.3;
    o.vc = vc;
    o.h = ((dC.b * (1.0 - grassy) * (1.0 - sandy) * 0.6 + fine_n * 0.35) * mr_near * tw.y * 0.02 + dA.r * mr_mid * 0.12) * (1.0 - max(paved, asph) * 0.8);
    o.sandy = sandy;
    o.fine_n = fine_n;
    return o;
}
#else
// Without the packed texture: the map from above at two scales, the rock
// from the sides.
fn terrain_map(p: vec3<f32>, t_norm: vec3<f32>, v_color: vec3<f32>, v_surf: f32) -> Terrain {
    var o: Terrain;
    var tw = pow(abs(normalize(t_norm)), vec3<f32>(4.0));
    tw /= (tw.x + tw.y + tw.z);
    let top = textureSample(map_texture, map_sampler, p.xz / 9.0).rgb;
    let top2 = textureSample(map_texture, map_sampler, p.xz / 61.0 + 0.37).rgb;
    let sx = textureSample(aux_texture, aux_sampler, vec2<f32>(p.z / 23.0, p.y / 15.0 + top2.r * 0.35)).rgb;
    let sz = textureSample(aux_texture, aux_sampler, vec2<f32>(p.x / 23.0, p.y / 15.0 + top2.g * 0.35)).rgb;
    let side = mix(vec3<f32>(0.85), sx * tw.x / max(tw.x + tw.z, 1e-3) + sz * tw.z / max(tw.x + tw.z, 1e-3), 0.75) * (0.75 + 0.5 * top.r);
    let tex = (top * 0.6 + top2 * 0.6) * tw.y + side * (tw.x + tw.z);
    o.tex = tex * 1.3;
    o.vc = v_color;
    o.h = 0.0;
    o.sandy = 0.0;
    o.fine_n = 0.0;
    return o;
}
#endif
#endif

#ifdef PATCH_ASPHALT
// Road.js HASH_GLSL: cheap lattice hash (no sin, fine on phones).
fn mrHash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}
#endif

// ────────────────────────────────────────────────────────────────────────

#ifdef PATCH_CITY
// patchCityMaterial's per-building hash.
fn c_hash(p_in: vec2<f32>) -> f32 {
    var p = fract(p_in * vec2<f32>(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

// Shopfront zone: 1 on the ground floor of walls (not glass crowns, roofs,
// sheds).
fn shop_zone(ci: i32, hgt: f32, ny: f32) -> f32 {
    let wall_cell = ci <= 5 || ci == 9;
    return select(0.0, 1.0, wall_cell && abs(ny) < 0.5 && hgt < 4.6);
}
#endif

// three's fog_fragment for FogExp2 (the ShaderMaterials' own).
fn fog_factor_of(depth: f32) -> f32 {
    let fog = globals_at(g::G_FOG);
    return 1.0 - exp(-fog.w * fog.w * depth * depth);
}

#ifdef PATCH_SURF
fn h21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn vn(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(h21(i), h21(i + vec2<f32>(1.0, 0.0)), u.x), mix(h21(i + vec2<f32>(0.0, 1.0)), h21(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

// Coast.js foamFrag: swell lines rolling in toward the rock, breaking into
// foam; the contact foam breathing with the waves; rings broken into arcs.
fn surf_fragment(in: VOut) -> vec4<f32> {
    let u = kind_uniforms();
    let u_swell = material.kind0.z;
    let v = in.uv.y;            // 0 = against the rock, 1 = open water
    let tt = u.x + in.extra.x;
    let n = vn(vec2<f32>(in.uv.x * 2.3, v * 3.0 - tt * 0.35)) * 0.6 + vn(vec2<f32>(in.uv.x * 7.0 + tt * 0.21, v * 9.0 - tt * 0.5)) * 0.4;
    let band = fract(v * 2.1 + tt * 0.2);
    let swell = t::smooth_step(0.0, 0.07, band) * (1.0 - t::smooth_step(0.07, 0.33, band));
    let surge = 0.7 + 0.3 * sin(tt * 1.3 + in.uv.x * 0.7);
    let contact = 1.0 - t::smooth_step(0.08, 0.42 * surge, v + (n - 0.5) * 0.45);
    let lace = t::smooth_step(0.35, 0.8, n) * (1.0 - t::smooth_step(0.3, 0.9, v));
    var foam = max(contact * (0.45 + 0.7 * n), max(swell * t::smooth_step(0.4, 0.75, n) * (1.0 - v) * u_swell, lace * 0.45));
    foam *= t::smooth_step(0.0, 0.05, v) * (1.0 - t::smooth_step(0.8, 1.0, v));
    if (u_swell < 0.5) {
        foam *= t::smooth_step(0.35, 0.65, vn(vec2<f32>(in.uv.x * 1.3 + in.extra.x * 3.0, tt * 0.15))) * 0.75;
    }
    var c = material.kind1.rgb * u.y;
#ifdef USE_FOG
    c = mix(c, globals_at(g::G_FOG).rgb, fog_factor_of(-in.view_pos.z));
#endif
    return vec4<f32>(c, clamp(foam, 0.0, 1.0) * 0.8);
}
#endif

#ifdef PATCH_BEAM
// Coast.js beamFrag: a bright core seen side on, falling off along the
// beam, faded close to the camera and in the fog.
fn beam_fragment(in: VOut) -> vec4<f32> {
    let n = in.normal / max(length(in.normal), 1e-4);
    let v = in.world_normal / max(length(in.world_normal), 1e-4);
    let core = pow(clamp(abs(dot(n, v)), 1e-4, 1.0), 1.6);
    let along = clamp(in.extra.x, 0.0, 1.0);
    let fall = pow(max(1.0 - along, 1e-4), 2.0) * t::smooth_step(0.0, 0.04, along);
    let depth = -in.view_pos.z;
    let near = t::smooth_step(15.0, 120.0, depth);
    let a = clamp(core * fall * near * kind_uniforms().x, 0.0, 2.0);
    var c = material.kind1.rgb * a;
#ifdef USE_FOG
    c *= 1.0 - fog_factor_of(depth) * 0.8;
#endif
    return vec4<f32>(c, 1.0);
}
#endif

@fragment
fn fragment(in: VOut, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
#ifdef PATCH_TRAFFIC
    // City.js buildTraffic's fragment shader.
    let q = in.uv - 0.5;
    let traffic_a = exp(-dot(q, q) * 14.0) * in.extra.x * kind_uniforms().y;
    return vec4<f32>(in.color.rgb * 2.2, traffic_a);
#else ifdef PATCH_SKYGLOW
    // City.js buildSkyGlow's fragment shader (uGround, uH as exported).
    let glow_h = max(in.world_pos.y - material.kind0.y, 0.0);
    let glow_g = exp(-glow_h / material.kind0.z);
    let glow_col = mix(vec3<f32>(0.5, 0.26, 0.16), vec3<f32>(0.22, 0.14, 0.3), t::smooth_step(0.0, 2.5 * material.kind0.z, glow_h));
    return vec4<f32>(glow_col * glow_g * kind_uniforms().x, 1.0);
#else ifdef PATCH_SURF
    return surf_fragment(in);
#else ifdef PATCH_BEAM
    return beam_fragment(in);
#else
    // normal_fragment_begin (first: the derivatives below need uniform
    // control flow, ahead of any discard).
    let face_direction = select(-1.0, 1.0, is_front);
    var normal = normalize(in.normal);
#ifdef DOUBLE_SIDED
    normal = normal * face_direction;
#endif
#ifdef USE_NORMALMAP
    // getTangentFrame( - vViewPosition, normal, vNormalMapUv ); the sea's
    // normal map scrolls (Sea.js), which `G_ANIM2.xy` adds to its offset.
    // The sea's normal map scrolls with the scene-wide clock (D293).
    var normal_offset = vec2<f32>(0.0);
#ifdef PATCH_SEA
    normal_offset = globals_at(g::G_ANIM2).xy;
#endif
    if (has_block()) {
        // The animators move the normal map's offset (D490).
        normal_offset = block_at(3).zw;
    }
    let normal_uv = vec2<f32>(dot(material.normal_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.normal_t1.xyz, vec3<f32>(in.uv, 1.0))) + normal_offset;
    // GLSL's dFdy runs up the window, WGSL's dpdy down the framebuffer: the
    // frame is built from GL's (it is not invariant under that flip; the
    // terrain's bump is).
    let q0 = dpdx(in.view_pos);
    let q1 = -dpdy(in.view_pos);
    let st0 = dpdx(normal_uv);
    let st1 = -dpdy(normal_uv);
    let q1perp = cross(q1, normal);
    let q0perp = cross(normal, q0);
    let tan_t = q1perp * st0.x + q0perp * st1.x;
    let tan_b = q1perp * st0.y + q0perp * st1.y;
    let tan_det = max(dot(tan_t, tan_t), dot(tan_b, tan_b));
    let tan_scale = select(inverseSqrt(tan_det), 0.0, tan_det == 0.0);
    var tbn = mat3x3<f32>(tan_t * tan_scale, tan_b * tan_scale, normal);
#ifdef DOUBLE_SIDED
    tbn[0] *= face_direction;
    tbn[1] *= face_direction;
#endif
#endif
    let non_perturbed_normal = normal;
    let dxy = max(abs(dpdx(non_perturbed_normal)), abs(dpdy(non_perturbed_normal)));
    let geometry_roughness = max(max(dxy.x, dxy.y), dxy.z);

    var diffuse_color = material.diffuse;
#ifdef PATCH_POOL
    // The pools' and the flood beams' opacity follows the night (their
    // updater's, or the animators').
    diffuse_color.a = kind_uniforms().y;
#endif
#ifdef PATCH_FLOODBEAM
    diffuse_color.a = kind_uniforms().x;
#endif
#ifdef POINTS_GLOW
    // City.js's updater: m.color.setScalar(1.6 * smoothstep(0.2, 0.7, night)).
    diffuse_color = vec4<f32>(vec3<f32>(1.6 * t::smooth_step(0.2, 0.7, globals_at(g::G_SKY_PARAMS).x)), diffuse_color.a);
#endif
    // An animator's colour (D490).
    var map_offset = vec2<f32>(0.0);
    if (has_block()) {
        let b0 = block_at(0);
        if (b0.w > 0.5) {
            diffuse_color = vec4<f32>(b0.rgb, diffuse_color.a);
        }
        map_offset = block_at(3).xy;
    }
    // roughnessmap_fragment's roughnessFactor (the patches change it).
    var roughness_factor = material.pbr.x;
#ifdef PATCH_TERRAIN
    // map_fragment: the triplanar ground (the map itself is not sampled).
    let terrain = terrain_map(in.world_pos, in.world_normal, in.color.rgb, in.extra.x);
    diffuse_color = vec4<f32>(diffuse_color.rgb * terrain.tex, diffuse_color.a);
    // normal_fragment_maps: mrPerturb(-vViewPosition, normal, mrH,
    // faceDirection), a bump from a scalar height via screen-space
    // derivatives (as three's perturbNormalArb, without a bump map).
    let bump_sx = dpdx(in.view_pos);
    let bump_sy = dpdy(in.view_pos);
    let bump_dh = vec2<f32>(dpdx(terrain.h), dpdy(terrain.h));
#else ifdef PATCH_CONTAINER
    // containerMaterial, replacing map_fragment: the atlas row by aVar
    // (vMapUv.y = (vMapUv.y + aVar) × 0.25), read later as a mask.
    let container_uv = vec2<f32>(dot(material.map_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.map_t1.xyz, vec3<f32>(in.uv, 1.0))) + map_offset;
    let c_t = textureSample(map_texture, map_sampler, vec2<f32>(container_uv.x, (container_uv.y + in.extra.x) * 0.25));
#else ifdef PATCH_CITY
    // patchCityMaterial at map_fragment: the atlas cell from the `cell`
    // attribute (its fraction a per-building seed), sampled with the
    // gradients of the raw uv (no seams at the cell's wrap).
    let c_idx = i32(floor(in.extra.x + 0.5));
    let c_seed = floor((fract(in.extra.x + 0.5) - 0.1) * 80.0) * 1.37 + 3.0;
    var atlas_uv = vec2<f32>((f32(c_idx) + fract(in.uv.x)) / 10.0, fract(in.uv.y));
    atlas_uv.x = clamp(atlas_uv.x, (f32(c_idx) + 0.004) / 10.0, (f32(c_idx) + 0.996) / 10.0);
    let atlas_dx = dpdx(in.uv) * vec2<f32>(1.0 / 10.0, 1.0);
    let atlas_dy = dpdy(in.uv) * vec2<f32>(1.0 / 10.0, 1.0);
    let c_ny = in.world_normal.y;
    let c_hgt = in.world_pos.y - material.kind0.x;
    let c_shop = shop_zone(c_idx, c_hgt, c_ny);
    // CTILE[cIdx].x (toFixed(2)): metres along the wall.
    var c_tile = array<f32, 10>(24.0, 21.0, 28.0, 18.0, 31.2, 16.0, 16.0, 24.0, 24.0, 18.0);
    let c_m = in.uv.x * c_tile[clamp(c_idx, 0, 9)];
    let c_shop_roll = c_hash(vec2<f32>(floor(c_m / 7.0), c_seed));
#ifdef USE_MAP
    diffuse_color *= textureSampleGrad(map_texture, map_sampler, atlas_uv, atlas_dx, atlas_dy);
#endif
    let c_glass = textureSampleGrad(detail_texture, detail_sampler, atlas_uv, atlas_dx, atlas_dy).r * (1.0 - c_shop);
    if (c_shop > 0.5) {
        // Shop glass, fascia above, plinth below; roller shutters on
        // closed shops.
        let in_glass = step(0.35, c_hgt) * step(c_hgt, 3.3);
        let fascia = step(3.4, c_hgt) * step(c_hgt, 4.35);
        var dc = select(vec3<f32>(0.32, 0.33, 0.34) * (0.8 + 0.2 * step(0.5, fract(c_hgt * 6.0))), vec3<f32>(0.05, 0.06, 0.07), c_shop_roll < 0.68);
        dc = mix(vec3<f32>(0.16, 0.15, 0.14), dc, in_glass);
        dc = mix(dc, vec3<f32>(0.1, 0.1, 0.12), fascia);
        diffuse_color = vec4<f32>(dc, diffuse_color.a);
    }
    // Derivatives for the emissive below, here in uniform control flow.
    var c_grid = array<vec2<f32>, 10>(
        vec2<f32>(8.0, 20.0), vec2<f32>(6.0, 16.0), vec2<f32>(10.0, 26.0), vec2<f32>(5.0, 14.0), vec2<f32>(12.0, 30.0),
        vec2<f32>(4.0, 10.0), vec2<f32>(1.0, 1.0), vec2<f32>(8.0, 12.0), vec2<f32>(4.0, 1.0), vec2<f32>(6.0, 12.0),
    );
    let c_wp = in.uv * c_grid[clamp(c_idx, 0, 9)];
    let c_fw = max(fwidth(c_wp.x), fwidth(c_wp.y));
    let c_mfw = fwidth(c_m);
    let c_em_map = textureSampleGrad(emissive_texture, emissive_sampler, atlas_uv, atlas_dx, atlas_dy).rgb;
#else
#ifdef USE_MAP
    let map_uv = vec2<f32>(dot(material.map_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.map_t1.xyz, vec3<f32>(in.uv, 1.0))) + map_offset;
    var sampled_diffuse_color = textureSample(map_texture, map_sampler, map_uv);
#ifdef PATCH_SHOULDER
    // Toward the outer edge the gravel texture flattens to its average so
    // the verge meets the terrain without a hard seam.
    sampled_diffuse_color = vec4<f32>(mix(sampled_diffuse_color.rgb, vec3<f32>(0.22, 0.2, 0.17), t::smooth_step(0.55, 0.95, map_uv.x) * 0.8), sampled_diffuse_color.a);
#endif
    diffuse_color *= sampled_diffuse_color;
#endif
#ifdef PATCH_SANDSTONE
    // desert/parts.js sandstoneMaterial, replacing map_fragment: strata
    // from tRock and the desert varnish streaks down steep faces.
    {
        let rn = normalize(in.world_normal);
        var w_ = pow(abs(rn), vec3<f32>(3.0));
        w_ /= (w_.x + w_.y + w_.z);
        let rp = in.world_pos;
        let a_ = textureSample(aux_texture, aux_sampler, vec2<f32>(rp.z * 0.07, rp.y * 0.16)).rgb;
        let b_ = textureSample(aux_texture, aux_sampler, vec2<f32>(rp.x * 0.07, rp.y * 0.16)).rgb;
        let c_ = textureSample(aux_texture, aux_sampler, rp.xz * 0.09).rgb;
        let st_ = textureSample(detail_texture, detail_sampler, vec2<f32>((rp.x + rp.z) * 0.11, rp.y * 0.006)).r;
        let stp_ = 1.0 - abs(rn.y);
        diffuse_color = vec4<f32>(diffuse_color.rgb * (a_ * w_.x + b_ * w_.z + c_ * w_.y) * 1.35 * (1.0 - 0.45 * t::smooth_step(0.7, 0.86, st_) * stp_ * stp_), diffuse_color.a);
    }
#endif
#ifdef PATCH_ROCK
    // Mountain.js rockMaterial, replacing map_fragment: triplanar strata
    // from tRock in world space (vRP, vRN with the instance's matrix).
    {
        var w_ = pow(abs(normalize(in.world_normal)), vec3<f32>(3.0));
        w_ /= (w_.x + w_.y + w_.z);
        let rp = in.world_pos;
        let a_ = textureSample(aux_texture, aux_sampler, vec2<f32>(rp.z * 0.09, rp.y * 0.14)).rgb;
        let b_ = textureSample(aux_texture, aux_sampler, vec2<f32>(rp.x * 0.09, rp.y * 0.14)).rgb;
        let c_ = textureSample(aux_texture, aux_sampler, rp.xz * 0.11).rgb;
        diffuse_color = vec4<f32>(diffuse_color.rgb * (a_ * w_.x + b_ * w_.z + c_ * w_.y) * 1.3, diffuse_color.a);
    }
#endif
#endif
#ifdef PATCH_ASPHALT
    // After map_fragment: wheel paths and an oil stripe down each lane, big
    // slow tonal drift, the odd repair patch, dusty edges, and at night a
    // damp sheen. vRoad = uv * 4 (lateral m, distance along the road m);
    // vLane = origin, lane width, half width.
    let lat = in.uv.x * 4.0;
    let along = in.uv.y * 4.0;
    let lane = in.extra.xyz;
    let lp = (lat - lane.x) / lane.y;
    let dc = (fract(lp) - 0.5) * lane.y;             // metres from the lane centre
    let wq = (abs(dc) - 0.85) / 0.32;
    let wheel = exp(-wq * wq);
    let oil = exp(-(dc * dc) / 0.1225);
    let big = textureSample(detail_texture, detail_sampler, vec2<f32>(lat * 0.021, along * 0.0045));
    let mid = textureSample(detail_texture, detail_sampler, vec2<f32>(lat * 0.09, along * 0.03) + 0.4);
    // Repair patches: whole cells of a coarse grid, a few percent of them.
    let pc = vec2<f32>((lat + 40.0) / 2.6, along / 7.0);
    let pf = fract(pc);
    let patch_a = step(mrHash(floor(pc)), 0.045) * step(0.12, pf.x) * step(pf.x, 0.88) * step(0.1, pf.y) * step(pf.y, 0.9);
    let edge = t::smooth_step(lane.z - 0.9, lane.z, abs(lat));
    let tone = (0.86 + 0.28 * big.a) * (0.93 + 0.14 * mid.r);
    var road_rgb = diffuse_color.rgb * (tone * (1.0 - wheel * 0.1 - oil * 0.12) * mix(1.0, 0.7, patch_a));
    road_rgb = mix(road_rgb, road_rgb * vec3<f32>(1.25, 1.2, 1.12), edge * 0.6);
    let damp = kind_uniforms().x * (0.55 + 0.45 * t::smooth_step(0.35, 0.7, big.r)) * (1.0 - edge * 0.7);
    road_rgb *= 1.0 - damp * 0.25;
    diffuse_color = vec4<f32>(road_rgb, diffuse_color.a);
    let mr_rough = -wheel * 0.08 - patch_a * 0.06 - damp * 0.36 * (0.7 + 0.3 * wheel);
#endif
#ifdef USE_EMISSIVEMAP
    let emissive_uv = vec2<f32>(dot(material.emissive_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.emissive_t1.xyz, vec3<f32>(in.uv, 1.0)));
    let emissive_sample = textureSample(emissive_texture, emissive_sampler, emissive_uv).rgb;
#endif
    // color_fragment
#ifdef PATCH_TERRAIN
    diffuse_color = vec4<f32>(diffuse_color.rgb * terrain.vc, diffuse_color.a);
#else
#ifdef PATCH_CONTAINER
    // Replacing color_fragment: the body colour (the instance's) where the
    // mask's red is, light grey markings in green, rust in blue.
    {
        let body = in.color.rgb;
        let sh = pow(c_t.r, 2.2) * 1.9;
        diffuse_color = vec4<f32>(diffuse_color.rgb * mix(body * sh, vec3<f32>(0.8), c_t.g * 0.9) * mix(vec3<f32>(1.0), vec3<f32>(0.42, 0.26, 0.16), c_t.b * 0.8), diffuse_color.a);
    }
#else
    diffuse_color *= in.color;
#endif
#endif
#ifdef PATCH_STUCCO
    // Beach.js stuccoMaterial, after color_fragment: a triplanar grain and
    // a broad mottling in world space.
    {
        var w_ = abs(normalize(in.world_normal));
        w_ /= (w_.x + w_.y + w_.z);
        let gp = in.world_pos;
        let g_ = textureSample(detail_texture, detail_sampler, gp.zy * 0.45).r * w_.x + textureSample(detail_texture, detail_sampler, gp.xz * 0.45).r * w_.y + textureSample(detail_texture, detail_sampler, gp.xy * 0.45).r * w_.z;
        let f_ = textureSample(detail_texture, detail_sampler, gp.xz * 0.06 + gp.y * 0.02).r;
        diffuse_color = vec4<f32>(diffuse_color.rgb * (0.8 + 0.34 * g_) * (0.9 + 0.16 * f_), diffuse_color.a);
    }
#endif
    // Valley.js surfaceDetail, after color_fragment (vWP, vWN: the world
    // position and normal).
#ifdef SIDING_SIDING
    {
        // Horizontal lap siding: a shadow line under each board's lip.
        let p_ = in.world_pos.y / 0.23;
        let f_ = fract(p_);
        let a_ = (1.0 - t::smooth_step(0.25, 0.6, fwidth(p_))) * (1.0 - abs(in.world_normal.y));
        diffuse_color = vec4<f32>(diffuse_color.rgb * (1.0 - a_ * (0.22 * t::smooth_step(0.78, 1.0, f_) - 0.05 * f_)), diffuse_color.a);
    }
#endif
#ifdef SIDING_BOARDS
    {
        // Vertical boards with grooves and a little plank-to-plank tone.
        let u_ = select(in.world_pos.x, in.world_pos.z, abs(in.world_normal.x) > abs(in.world_normal.z)) / 0.32;
        let f_ = fract(u_);
        let a_ = (1.0 - t::smooth_step(0.25, 0.6, fwidth(u_))) * (1.0 - abs(in.world_normal.y));
        let h_ = fract(sin(floor(u_) * 91.7) * 4373.1);
        let g_ = t::smooth_step(0.0, 0.08, f_) * t::smooth_step(1.0, 0.92, f_);
        diffuse_color = vec4<f32>(diffuse_color.rgb * (1.0 - a_ * (0.3 * (1.0 - g_) + 0.12 * h_)), diffuse_color.a);
    }
#endif
#ifdef SIDING_ROOF
    {
        // Shingle courses: a dark line per course and staggered tile tones.
        let p_ = in.world_pos.y / 0.17;
        let f_ = fract(p_);
        let q_ = (in.world_pos.x + in.world_pos.z) / 0.45 + floor(p_) * 0.5;
        let a_ = (1.0 - t::smooth_step(0.25, 0.6, fwidth(p_))) * t::smooth_step(0.1, 0.4, abs(in.world_normal.y));
        let h_ = fract(sin(floor(q_) * 12.9 + floor(p_) * 78.2) * 43758.5);
        diffuse_color = vec4<f32>(diffuse_color.rgb * (1.0 - a_ * (0.3 * t::smooth_step(0.75, 1.0, f_) + 0.14 * h_)), diffuse_color.a);
    }
#endif
#ifdef USE_ALPHAMAP
    // alphamap_fragment (the photo slot holds the alpha map).
    {
        let alpha_uv = vec2<f32>(dot(material.alpha_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.alpha_t1.xyz, vec3<f32>(in.uv, 1.0))) + map_offset;
        diffuse_color.a *= textureSample(photo_texture, photo_sampler, alpha_uv).g;
    }
#endif
#ifdef PATCH_MARKINGS
    // Paint wear: fade the paint toward asphalt in blotches and where tyres
    // cross (vMark = uv: side × 8, s).
    let w1 = textureSample(detail_texture, detail_sampler, vec2<f32>(in.uv.x * 0.12, in.uv.y * 0.35));
    let w2 = textureSample(detail_texture, detail_sampler, vec2<f32>(in.uv.x * 0.5, in.uv.y * 0.05) + 0.3);
    let wear = t::smooth_step(0.42, 0.85, w1.g * 0.6 + w2.a * 0.6) * 0.7;
    diffuse_color = vec4<f32>(mix(diffuse_color.rgb, vec3<f32>(0.05, 0.05, 0.052), wear), diffuse_color.a);
#endif
#ifdef PATCH_SEA
    // After color_fragment: surf lines rolling up the beach where the water
    // is shallow (vDepth), foam where it meets the ground.
    let sea_dist = length(in.world_pos - view.world_position);
    let u_time = globals_at(g::G_ANIM).y;
    let fn_ = textureSample(detail_texture, detail_sampler, in.world_pos.xz / 9.0 + vec2<f32>(u_time * 0.012, u_time * 0.004));
    let v_depth = in.extra.x;
    let surf_zone = 1.0 - t::smooth_step(0.1, 2.4, v_depth);
    let roll = sin(v_depth * 4.2 - u_time * 1.3 + fn_.a * 5.0) * 0.5 + 0.5;
    var foam = t::smooth_step(0.55, 0.9, fn_.g * 0.55 + roll * 0.45 + surf_zone * 0.35) * surf_zone;
    foam = max(foam, (1.0 - t::smooth_step(0.0, 0.35, v_depth)) * (0.45 + 0.4 * fn_.g));
    foam = clamp(foam, 0.0, 1.0);
    diffuse_color = vec4<f32>(mix(diffuse_color.rgb, vec3<f32>(0.82, 0.88, 0.9), foam * 0.85), max(diffuse_color.a, foam * 0.92));
    // The two counter-scrolling normal scales, sampled here (uniform
    // control flow) for normal_fragment_maps below.
    let sea_n1 = textureSample(aux_texture, aux_sampler, normal_uv).xyz * 2.0 - 1.0;
    let sea_n2 = textureSample(aux_texture, aux_sampler, normal_uv * 3.3 + globals_at(g::G_ANIM).zw).xyz * 2.0 - 1.0;
#endif
#ifdef PATCH_POOL
    // Before alphatest_fragment: diffuseColor.rgb *= vFl; the opacity
    // follows the night (the updater's).
    diffuse_color = vec4<f32>(diffuse_color.rgb * in.extra.x, diffuse_color.a);
#endif
#ifdef PATCH_FLOODBEAM
    // Before alphatest_fragment: diffuseColor.rgb *= vFace²; the opacity
    // follows the night.
    diffuse_color = vec4<f32>(diffuse_color.rgb * in.extra.x * in.extra.x, diffuse_color.a);
#endif
#ifdef POINTS_FLICKER
    // At alphatest_fragment: diffuseColor.rgb *= vFl.
    diffuse_color = vec4<f32>(diffuse_color.rgb * in.extra.x, diffuse_color.a);
#endif
#ifdef ALPHA_TEST
    if (diffuse_color.a < material.emissive.w) {
        discard;
    }
#endif

    // roughnessmap_fragment's patches.
#ifdef PATCH_TERRAIN
    roughness_factor = clamp(roughness_factor - terrain.sandy * 0.08 * (1.0 - terrain.fine_n), 0.0, 1.0);
#endif
#ifdef PATCH_ASPHALT
    roughness_factor = clamp(roughness_factor + mr_rough, 0.25, 1.0);
#endif
#ifdef PATCH_SEA
    roughness_factor = mix(roughness_factor, 0.85, foam);
#endif
#ifdef PATCH_CITY
    // roughnessmap_fragment, metalnessmap_fragment: the glass is smooth and
    // a little metallic.
    roughness_factor = mix(roughness_factor, 0.12, c_glass);
#endif

    // normal_fragment_maps
#ifdef PATCH_TERRAIN
    {
        let r1 = cross(bump_sy, normal);
        let r2 = cross(normal, bump_sx);
        let det = dot(bump_sx, r1) * face_direction;
        if (abs(det) >= 1e-12) {
            let grad = sign(det) * (bump_dh.x * r1 + bump_dh.y * r2);
            normal = normalize(abs(det) * normal - grad);
        }
    }
#endif
#ifdef PATCH_SEA
    {
        var map_n = vec3<f32>(sea_n1.xy + sea_n2.xy * 0.6, 1.0);
        let scaled = map_n.xy * material.kind0.xy * mix(1.0, 0.3, t::smooth_step(30.0, 900.0, sea_dist)) * (1.0 - foam * 0.7);
        map_n = vec3<f32>(scaled, map_n.z);
        normal = normalize(tbn * map_n);
    }
#else
#ifdef USE_NORMALMAP
    {
        var map_n = textureSample(aux_texture, aux_sampler, normal_uv).xyz * 2.0 - 1.0;
        map_n = vec3<f32>(map_n.xy * material.kind0.xy, map_n.z);
        normal = normalize(tbn * map_n);
    }
#endif
#endif

    var outgoing_light: vec3<f32>;
#ifdef LIT
    var emissive_color = material.emissive.rgb;
    var emissive_intensity = 1.0;
    if (material.night.z > 1.5) {
        // A light setter's emissiveIntensity (CarModel's head, tail,
        // reverse, accent and siren lenses), set per frame in the globals.
        let k = i32(material.night.w);
        emissive_intensity = globals_at(g::G_LIGHTS + k / 4)[k % 4];
    } else if (material.night.z > 0.5) {
        // World.update: emissiveIntensity = day + (night - day) × n, with
        // the sky's night factor (three's emissive uniform is the colour ×
        // the intensity; `emissive` holds the colour here).
        emissive_intensity = material.night.x + (material.night.y - material.night.x) * globals_at(g::G_SKY_PARAMS).x;
    }
    if (has_block()) {
        // An animator's emissive colour or emissiveIntensity (D490). A
        // material with a block holds the colour alone in `emissive` and
        // its intensity in `night` (day = night when it does not follow
        // nightfall).
        let b1 = block_at(1);
        let b2 = block_at(2);
        if (b1.w > 0.5) {
            emissive_color = b1.rgb;
        }
        if (b2.y > 0.5) {
            emissive_intensity = b2.x;
        }
    }
    var total_emissive_radiance = emissive_color * emissive_intensity;
#ifdef PATCH_CITY
    // patchCityMaterial, replacing emissivemap_fragment: lit windows by
    // floor and room, glass reflections, street-light spill, shopfronts.
    var c_em = c_em_map;
    {
        let wp = c_wp;
        let wi = floor(wp);
        let wf = fract(wp);
        let occ = mix(0.08, 0.55, pow(c_hash(vec2<f32>(c_seed, 1.3)), 1.3));
        let fr = c_hash(vec2<f32>(wi.y, c_seed));
        let f_state = select(select(occ, 0.95, fr > 0.93), 0.0, fr < 0.25);
        let room_w = 2.0 + floor(c_hash(vec2<f32>(wi.y + 7.0, c_seed)) * 4.0);
        let room = floor((wi.x + floor(c_hash(vec2<f32>(c_seed, wi.y + 3.0)) * 4.0)) / room_w);
        let rh = c_hash(vec2<f32>(room, wi.y) + c_seed * 1.7);
        let lit = step(rh, f_state);
        var c_warm_k = array<f32, 10>(0.35, 0.55, 0.2, 0.8, 0.15, 0.75, 0.5, 0.25, 0.6, 0.85);
        let warm_b = clamp(c_warm_k[clamp(c_idx, 0, 9)] + (c_hash(vec2<f32>(c_seed, 5.0)) - 0.5) * 0.5, 0.0, 1.0);
        let tp = c_hash(vec2<f32>(room * 1.3, wi.y + 11.0) + c_seed);
        let c_warm = vec3<f32>(1.0, 0.7, 0.4);
        let c_neut = vec3<f32>(1.0, 0.9, 0.74);
        let c_cool = vec3<f32>(0.68, 0.84, 1.0);
        var wc = c_cool;
        if (tp < warm_b) {
            wc = mix(c_warm, c_neut, tp / max(warm_b, 0.01) * 0.6);
        } else if (tp < 0.5 + warm_b * 0.5) {
            wc = c_neut;
        }
        var inten = 0.5 + 0.65 * c_hash(vec2<f32>(room, wi.y + 2.0) + c_seed);
        // Ceiling lights: brighter toward the top of the pane; some
        // blinds, some half-drawn curtains.
        inten *= 0.72 + 0.42 * t::smooth_step(0.1, 0.95, wf.y);
        // Fine detail fades out before it can alias into sparkle.
        var fw = c_fw;
        let bl = c_hash(vec2<f32>(room + 5.0, wi.y) + c_seed * 3.1);
        if (bl < 0.22) {
            inten *= mix(0.75, 0.5 + 0.5 * step(0.4, fract(wf.y * 7.0)), 1.0 - t::smooth_step(0.02, 0.07, fw));
        } else if (bl < 0.36) {
            inten *= mix(0.65, mix(0.3, 1.0, step(wf.x, 0.55)), 1.0 - t::smooth_step(0.1, 0.3, fw));
        }
        var win = wc * inten * lit;
        // Beyond ~1 window per pixel, fall back to the building's average
        // glow: cross-fade to the baked pattern (scaled by this building's
        // occupancy), which mip-filters cleanly.
        fw = t::smooth_step(0.2, 0.5, fw);
        win = mix(win * c_glass, c_em * clamp(occ * 2.4, 0.35, 1.3), fw);
        // Dark glass mirrors the orange city glow low down and the night
        // above.
        let c_v = normalize(-in.view_pos);
        let rw = world_dir_of(reflect(-c_v, normal));
        let fres = 0.1 + 0.9 * pow(1.0 - clamp(dot(c_v, normal), 0.0, 1.0), 4.0);
        let env = select(vec3<f32>(0.06, 0.05, 0.045), mix(vec3<f32>(0.16, 0.1, 0.1), vec3<f32>(0.015, 0.02, 0.04), t::smooth_step(0.0, 0.5, rw.y)), rw.y > 0.0);
        let glass_k = select(0.55, 1.0, c_idx == 2 || c_idx == 4 || c_idx == 7);
        let refl = env * min(fres, 0.7) * glass_k * 0.75 * (1.0 - lit * (1.0 - fw));
        let win_cell = c_idx <= 5 || c_idx == 7 || c_idx == 9;
        c_em = select(c_em, vec3<f32>(0.007, 0.008, 0.011) + win + c_glass * refl, win_cell);
        // Street-light spill washing the lower floors.
        c_em += (1.0 - c_glass) * (1.0 - c_shop) * vec3<f32>(0.5, 0.36, 0.22) * 0.2 * exp(-max(c_hgt, 0.0) / 7.0) * step(abs(c_ny), 0.5);
        if (c_shop > 0.5) {
            let in_glass = step(0.35, c_hgt) * step(c_hgt, 3.3);
            let fascia = step(3.4, c_hgt) * step(c_hgt, 4.35);
            let pick = fract(c_shop_roll * 7.13);
            var sc = vec3<f32>(1.0, 0.45, 0.12);
            if (pick < 0.5) {
                sc = vec3<f32>(1.0, 0.68, 0.36);
            } else if (pick < 0.7) {
                sc = vec3<f32>(0.8, 0.9, 1.0);
            } else if (pick < 0.8) {
                sc = vec3<f32>(1.0, 0.25, 0.6);
            } else if (pick < 0.9) {
                sc = vec3<f32>(0.2, 0.8, 0.9);
            }
            let open = step(c_shop_roll, 0.68);
            let mfw = c_mfw;
            let mull = mix(0.95, step(0.05, fract(c_m / 1.75)), 1.0 - t::smooth_step(0.08, 0.25, mfw));
            // Brightest at the ceiling and mid-shop; darker counters and
            // displays below, a dark awning strip on top, shelving between.
            let sx = fract(c_m / 7.0);
            var glow = (0.5 + 0.5 * t::smooth_step(0.3, 3.0, c_hgt)) * (0.65 + 0.35 * sin(sx * 3.14159)) * mull;
            glow *= mix(1.0, 0.4, step(c_hgt, 1.05));
            glow *= mix(1.0, 0.7 + 0.6 * c_hash(vec2<f32>(floor(c_m * 1.3), c_seed)), 1.0 - t::smooth_step(0.05, 0.15, mfw));
            glow *= 1.0 - 0.85 * step(2.95, c_hgt);
            var shop = sc * 0.6 * glow * in_glass * open;
            // Fascia sign: blocky "lettering" in a saturated colour.
            let letters = mix(0.65, step(0.35, c_hash(vec2<f32>(floor(c_m * 2.6), floor(c_hgt * 4.0) + c_seed))), 1.0 - t::smooth_step(0.03, 0.1, mfw));
            var sign_c = vec3<f32>(1.0, 0.75, 0.25);
            if (pick < 0.5) {
                sign_c = vec3<f32>(1.0, 0.25, 0.4);
            } else if (pick < 0.75) {
                sign_c = vec3<f32>(0.25, 0.8, 1.0);
            }
            shop += fascia * step(c_shop_roll, 0.8) * mix(sign_c * 0.25, sign_c * 1.3, letters) * step(0.12, fract(c_m / 7.0)) * step(fract(c_m / 7.0), 0.88);
            // Closed shops: a dim security light over the shutter.
            shop += in_glass * (1.0 - open) * vec3<f32>(0.9, 0.8, 0.6) * 0.1 * t::smooth_step(2.4, 3.3, c_hgt);
            let lod = t::smooth_step(0.5, 2.0, mfw);
            c_em = mix(shop, sc * 0.22, lod);
        }
    }
    total_emissive_radiance *= c_em;
#else
#ifdef USE_EMISSIVEMAP
    total_emissive_radiance *= emissive_sample;
#endif
#endif
    // PATCH_REFLECTOR: Mountain.js means the reflector's emissive to take
    // the instance colour (`#ifdef USE_INSTANCING_COLOR totalEmissiveRadiance
    // *= vColor`), but three r180 defines USE_INSTANCING_COLOR in the vertex
    // shader only (the fragment shader gets USE_COLOR), so in the game the
    // patch does nothing and the reflectors glow white: ported as it draws
    // (D494).

    var m: t::PhysicalMaterial;
#ifdef LIT_PHYSICAL
    // lights_physical_fragment
    var metalness_factor = material.pbr.y;
#ifdef PATCH_CITY
    metalness_factor = mix(metalness_factor, 0.6, c_glass);
#endif
    m.diffuse_color = diffuse_color.rgb * (1.0 - metalness_factor);
    m.roughness = max(roughness_factor, 0.0525);
    m.roughness += geometry_roughness;
    m.roughness = min(m.roughness, 1.0);
#ifdef PHYSICAL
    let ior = material.physical.x;
    let specular_intensity_factor = material.specular.w;
    let specular_color_factor = material.specular.rgb;
    m.f90_specular = mix(specular_intensity_factor, 1.0, metalness_factor);
    m.specular_color = mix(min(vec3<f32>(t::pow2f((ior - 1.0) / (ior + 1.0))) * specular_color_factor, vec3<f32>(1.0)) * specular_intensity_factor, diffuse_color.rgb, metalness_factor);
#else
    m.specular_color = mix(vec3<f32>(0.04), diffuse_color.rgb, metalness_factor);
    m.f90_specular = 1.0;
#endif
#ifdef USE_CLEARCOAT
    m.clearcoat = saturate(material.pbr.z);
    m.clearcoat_roughness = max(material.pbr.w, 0.0525);
    m.f0_clearcoat = vec3<f32>(0.04);
    m.f90_clearcoat = 1.0;
    m.clearcoat_roughness += geometry_roughness;
    m.clearcoat_roughness = min(m.clearcoat_roughness, 1.0);
#endif
#ifdef USE_SHEEN
    m.sheen_color = material.sheen.rgb;
    m.sheen_roughness = clamp(material.sheen.w, 0.07, 1.0);
#endif
#else
    // lights_lambert_fragment
    m.diffuse_color = diffuse_color.rgb;
#endif

    // lights_fragment_begin
    let geometry_position = in.view_pos;
    let geometry_normal = normal;
    let geometry_view_dir = normalize(-in.view_pos);
    var r: Reflected;
    r.direct_diffuse = vec3<f32>(0.0);
    r.direct_specular = vec3<f32>(0.0);
    r.indirect_diffuse = vec3<f32>(0.0);
    r.indirect_specular = vec3<f32>(0.0);
    r.clearcoat_specular = vec3<f32>(0.0);
    r.sheen_specular = vec3<f32>(0.0);

    // The point light (three loops point lights first, then spots, then
    // directional lights).
    let point_color = globals_at(g::G_POINT_COLOR);
    if (any(point_color.rgb != vec3<f32>(0.0))) {
        let point_pos = globals_at(g::G_POINT_POS);
        let l_vector = (view.view_from_world * vec4<f32>(point_pos.xyz, 1.0)).xyz - geometry_position;
        var light: Incident;
        light.direction = normalize(l_vector);
        light.color = point_color.rgb * t::getDistanceAttenuation(length(l_vector), point_pos.w, point_color.w);
        light.visible = any(light.color != vec3<f32>(0.0));
        re_direct(light, geometry_normal, geometry_view_dir, m, &r);
    }
    let spot_color = globals_at(g::G_SPOT_COLOR);
    if (spot_color.w > 0.0) {
        let spot_pos = globals_at(g::G_SPOT_POS);
        let spot_dir = globals_at(g::G_SPOT_DIR);
        let cone = globals_at(g::G_SPOT_CONE);
        let l_vector = (view.view_from_world * vec4<f32>(spot_pos.xyz, 1.0)).xyz - geometry_position;
        var light: Incident;
        light.direction = normalize(l_vector);
        // three's spotLight.direction points from the target to the light.
        let angle_cos = dot(light.direction, view_dir_of(-spot_dir.xyz));
        let spot_attenuation = t::getSpotAttenuation(cone.x, cone.y, angle_cos);
        if (spot_attenuation > 0.0) {
            light.color = spot_color.rgb * spot_attenuation * t::getDistanceAttenuation(length(l_vector), spot_pos.w, spot_dir.w);
        } else {
            light.color = vec3<f32>(0.0);
        }
        re_direct(light, geometry_normal, geometry_view_dir, m, &r);
    }
    let sun_dir = globals_at(g::G_SUN_DIR);
    let sun_color = globals_at(g::G_SUN_COLOR);
    if (sun_dir.w > 0.0) {
        var light: Incident;
        light.color = sun_color.rgb;
        light.direction = view_dir_of(sun_dir.xyz);
        light.visible = true;
#ifdef MR_INSTANCED
        let receive = in.receive > 0.5;
#else
        let receive = (mesh[in.instance_index].flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u;
#endif
        if (sun_color.w > 0.0 && receive) {
            light.color *= getShadow(in.shadow_coord);
        }
        re_direct(light, geometry_normal, geometry_view_dir, m, &r);
    }
    var irradiance = globals_at(g::G_AMBIENT).rgb;
    let hemi_sky = globals_at(g::G_HEMI_SKY);
    let hemi_dir = globals_at(g::G_HEMI_DIR);
    if (hemi_sky.w > 0.0) {
        irradiance += t::getHemisphereLightIrradiance(hemi_sky.rgb, globals_at(g::G_HEMI_GROUND).rgb, view_dir_of(hemi_dir.xyz), geometry_normal);
    }

#ifdef LIT_PHYSICAL
    // lights_fragment_maps: the scene's environment (MeshStandardMaterial
    // only), prefiltered from the sky.
    var ibl_irradiance = vec3<f32>(0.0);
    var radiance = vec3<f32>(0.0);
    var clearcoat_radiance = vec3<f32>(0.0);
    let env_intensity = hemi_dir.w;
    if (env_intensity > 0.0) {
        let world_normal = world_dir_of(geometry_normal);
        ibl_irradiance += t::PI * t::textureCubeUV(env_texture, env_sampler, world_normal, 1.0).rgb * env_intensity;
        var reflect_vec = reflect(-geometry_view_dir, geometry_normal);
        reflect_vec = normalize(mix(reflect_vec, geometry_normal, m.roughness * m.roughness));
        reflect_vec = world_dir_of(reflect_vec);
        radiance += t::textureCubeUV(env_texture, env_sampler, reflect_vec, m.roughness).rgb * env_intensity;
#ifdef USE_CLEARCOAT
        var cc_vec = reflect(-geometry_view_dir, geometry_normal);
        cc_vec = normalize(mix(cc_vec, geometry_normal, m.clearcoat_roughness * m.clearcoat_roughness));
        cc_vec = world_dir_of(cc_vec);
        clearcoat_radiance += t::textureCubeUV(env_texture, env_sampler, cc_vec, m.clearcoat_roughness).rgb * env_intensity;
#endif
    }
    // lights_fragment_end: RE_IndirectDiffuse_Physical, RE_IndirectSpecular_Physical
    r.indirect_diffuse += irradiance * t::BRDF_Lambert(m.diffuse_color);
#ifdef USE_CLEARCOAT
    r.clearcoat_specular += clearcoat_radiance * t::EnvironmentBRDF(geometry_normal, geometry_view_dir, m.f0_clearcoat, m.f90_clearcoat, m.clearcoat_roughness);
#endif
#ifdef USE_SHEEN
    r.sheen_specular += ibl_irradiance * m.sheen_color * t::IBLSheenBRDF(geometry_normal, geometry_view_dir, m.sheen_roughness);
#endif
    let scattering = t::computeMultiscattering(geometry_normal, geometry_view_dir, m.specular_color, m.f90_specular, m.roughness);
    let cosine_weighted_irradiance = ibl_irradiance * t::RECIPROCAL_PI;
    let total_scattering = scattering.single + scattering.multi;
    let diffuse = m.diffuse_color * (1.0 - max(max(total_scattering.r, total_scattering.g), total_scattering.b));
    r.indirect_specular += radiance * scattering.single;
    r.indirect_specular += scattering.multi * cosine_weighted_irradiance;
    r.indirect_diffuse += diffuse * cosine_weighted_irradiance;

    let total_diffuse = r.direct_diffuse + r.indirect_diffuse;
    let total_specular = r.direct_specular + r.indirect_specular;
    outgoing_light = total_diffuse + total_specular + total_emissive_radiance;
#ifdef USE_SHEEN
    let sheen_energy_comp = 1.0 - 0.157 * t::max3v(m.sheen_color);
    outgoing_light = outgoing_light * sheen_energy_comp + r.sheen_specular;
#endif
#ifdef USE_CLEARCOAT
    let dotNVcc = saturate(dot(geometry_normal, geometry_view_dir));
    let Fcc = t::F_Schlick(m.f0_clearcoat, m.f90_clearcoat, dotNVcc);
    outgoing_light = outgoing_light * (1.0 - m.clearcoat * Fcc) + r.clearcoat_specular * m.clearcoat;
#endif
#else
    // RE_IndirectDiffuse_Lambert
    r.indirect_diffuse += irradiance * t::BRDF_Lambert(m.diffuse_color);
    outgoing_light = r.direct_diffuse + r.indirect_diffuse + total_emissive_radiance;
#endif
#else
    // meshbasic: no lighting.
    outgoing_light = diffuse_color.rgb;
#endif

#ifdef PATCH_SEA
    // Replacing opaque_fragment: fresnel opacity (see-through looking down,
    // mirror-like toward the horizon) and a sharp sun/moon glitter (^256)
    // on the ripple facets.
    {
        let sea_v = normalize(-in.view_pos);
        var sea_f = 1.0 - clamp(dot(sea_v, normal), 0.0, 1.0);
        sea_f *= sea_f;
        sea_f *= sea_f;
        diffuse_color.a = mix(diffuse_color.a, 1.0, sea_f * 0.8);
        let sun_dir_g = globals_at(g::G_SUN_DIR);
        if (sun_dir_g.w > 0.0) {
            var glit = clamp(dot(reflect(-sea_v, normal), view_dir_of(sun_dir_g.xyz)), 0.0, 1.0);
            glit *= glit; glit *= glit; glit *= glit; glit *= glit;
            glit *= glit; glit *= glit; glit *= glit; glit *= glit;
            outgoing_light += min(globals_at(g::G_SUN_COLOR).rgb, vec3<f32>(4.0)) * glit * 3.0 * (1.0 - foam) * t::smooth_step(1500.0, 200.0, sea_dist);
        }
    }
#endif
    // opaque_fragment
#ifdef OPAQUE
    diffuse_color.a = 1.0;
#endif
    var out_color = vec4<f32>(outgoing_light, diffuse_color.a);

    // fog_fragment, with the game's sun tint (Sky.js) in lit shaders.
#ifdef USE_FOG
    let fog = globals_at(g::G_FOG);
    let fog_depth = -in.view_pos.z;
    let fog_factor = 1.0 - exp(-fog.w * fog.w * fog_depth * fog_depth);
    var fog_tint = fog.rgb;
#ifdef LIT
    if (sun_dir.w > 0.0) {
        let fog_view = in.view_pos;
        let fog_sun = clamp(dot(fog_view, view_dir_of(sun_dir.xyz)) / max(length(fog_view), 1e-3), 0.0, 1.0);
        let fog_sun4 = fog_sun * fog_sun * fog_sun * fog_sun;
        var fog_sun24 = fog_sun4 * fog_sun4 * fog_sun4;
        fog_sun24 *= fog_sun24;
        fog_tint += min(sun_color.rgb, vec3<f32>(4.0)) * (fog_sun4 * 0.035 + fog_sun24 * 0.07);
    }
#endif
    out_color = vec4<f32>(mix(out_color.rgb, fog_tint, fog_factor), out_color.a);
#endif
#ifdef POINTS_GLOW
    // Before premultiplied_alpha_fragment: gl_FragColor.a *= vGA.
    out_color.a *= in.extra.x;
#endif
    return out_color;
#endif
}
