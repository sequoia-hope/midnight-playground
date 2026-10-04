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
// POINTS_FLICKER, FLICKER_BLINK). VERTEX_EXTRA: the patch's own attribute
// (`convert::ATTRIBUTE_EXTRA`). MR_INSTANCED: an InstancedMesh drawn as one
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
#ifdef MR_INSTANCED
    // The instance stream: the InstancedMesh's matrixWorld × instanceMatrix
    // (multiplied on the CPU), then instanceColor (white without one) and
    // receiveShadow.
    @location(9) i_col0: vec4<f32>,
    @location(10) i_col1: vec4<f32>,
    @location(11) i_col2: vec4<f32>,
    @location(12) i_col3: vec4<f32>,
    @location(13) i_color: vec4<f32>,
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
#ifdef MR_INSTANCED
    let world_from_local = mat4x4<f32>(v.i_col0, v.i_col1, v.i_col2, v.i_col3);
    let world = world_from_local * vec4<f32>(v.position, 1.0);
    out.receive = v.i_color.w;
#else
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
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
    return out;
}

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
    let fog_k = globals_at(g::G_FOG).w * 0.4;
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

@fragment
fn fragment(in: VOut, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
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
    let normal_uv = vec2<f32>(dot(material.normal_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.normal_t1.xyz, vec3<f32>(in.uv, 1.0))) + globals_at(g::G_ANIM2).xy;
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
#ifdef POINTS_GLOW
    // City.js's updater: m.color.setScalar(1.6 * smoothstep(0.2, 0.7, night)).
    diffuse_color = vec4<f32>(vec3<f32>(1.6 * t::smooth_step(0.2, 0.7, globals_at(g::G_SKY_PARAMS).x)), diffuse_color.a);
#endif
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
#else
#ifdef USE_MAP
    let map_uv = vec2<f32>(dot(material.map_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.map_t1.xyz, vec3<f32>(in.uv, 1.0)));
    var sampled_diffuse_color = textureSample(map_texture, map_sampler, map_uv);
#ifdef PATCH_SHOULDER
    // Toward the outer edge the gravel texture flattens to its average so
    // the verge meets the terrain without a hard seam.
    sampled_diffuse_color = vec4<f32>(mix(sampled_diffuse_color.rgb, vec3<f32>(0.22, 0.2, 0.17), t::smooth_step(0.55, 0.95, map_uv.x) * 0.8), sampled_diffuse_color.a);
#endif
    diffuse_color *= sampled_diffuse_color;
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
    let damp = globals_at(g::G_ANIM).x * (0.55 + 0.45 * t::smooth_step(0.35, 0.7, big.r)) * (1.0 - edge * 0.7);
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
    diffuse_color *= in.color;
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
    var total_emissive_radiance = material.emissive.rgb;
    if (material.night.z > 1.5) {
        // A light setter's emissiveIntensity (CarModel's head, tail,
        // reverse, accent and siren lenses), set per frame in the globals.
        let k = i32(material.night.w);
        total_emissive_radiance *= globals_at(g::G_LIGHTS + k / 4)[k % 4];
    } else if (material.night.z > 0.5) {
        // World.update: emissiveIntensity = day + (night - day) × n, with
        // the sky's night factor (three's emissive uniform is the colour ×
        // the intensity; `emissive` holds the colour here).
        total_emissive_radiance *= material.night.x + (material.night.y - material.night.x) * globals_at(g::G_SKY_PARAMS).x;
    }
#ifdef USE_EMISSIVEMAP
    total_emissive_radiance *= emissive_sample;
#endif

    var m: t::PhysicalMaterial;
#ifdef LIT_PHYSICAL
    // lights_physical_fragment
    let metalness_factor = material.pbr.y;
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
}
