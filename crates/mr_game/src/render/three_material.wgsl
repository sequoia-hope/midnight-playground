// The plain material kinds (SPEC 6.2: Standard, Physical, Lambert, Basic,
// Line, Points) on three_std: three.js r180's meshphysical, meshlambert and
// meshbasic programs for the features the game uses. Lighting runs in view
// space as three's does (the geometry roughness term reads view-space normal
// derivatives). Output is linear HDR; the post chain (`render::post`) adds
// the bloom and applies three's ACES and sRGB.
//
// Shader defs (from `ThreeKey`): LIT_PHYSICAL | LIT_LAMBERT | (neither:
// basic); PHYSICAL (MeshPhysicalMaterial: IOR, specular); USE_CLEARCOAT;
// USE_SHEEN; USE_MAP; USE_EMISSIVEMAP; USE_COLOR (vertex colours);
// INSTANCE_COLOR (three's instanceColor, in the instance's tag); USE_FOG;
// ALPHA_TEST; OPAQUE; DOUBLE_SIDED; FLIP_SIDED (BackSide); USE_ENV.

#import bevy_pbr::{
    mesh_functions,
    mesh_bindings::mesh,
    mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    mesh_view_bindings::{view, lights, directional_shadow_textures},
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
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: ThreeParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var map_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var map_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var emissive_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var emissive_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var globals_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var env_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var env_sampler: sampler;

fn gl(i: i32) -> vec4<f32> {
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
};

fn unpack_tint(tag: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(tag & 1023u),
        f32((tag >> 10u) & 1023u),
        f32((tag >> 20u) & 1023u),
    ) / 511.5;
}

@vertex
fn vertex(v: Vertex) -> VOut {
    var out: VOut;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    out.view_pos = (view.view_from_world * world).xyz;
    out.clip = view.clip_from_world * world;
    out.instance_index = v.instance_index;
#ifdef VERTEX_NORMALS
    var n = mesh_functions::mesh_normal_local_to_world(v.normal, v.instance_index);
#else
    var n = vec3<f32>(0.0, 0.0, 1.0);
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
    color = vec4<f32>(color.rgb * unpack_tint(mesh_functions::get_tag(v.instance_index)), color.a);
#endif
    out.color = color;
    // shadowmap_vertex: the world position pushed out along the (normalised)
    // world normal by the normal bias, through three's shadowMatrix.
    let shadow = gl(g::G_SHADOW);
    let shadow_world = vec4<f32>(world.xyz + normalize(n) * shadow.y, 1.0);
    let m = mat4x4<f32>(gl(g::G_SHADOW_M), gl(g::G_SHADOW_M + 1), gl(g::G_SHADOW_M + 2), gl(g::G_SHADOW_M + 3));
    out.shadow_coord = m * shadow_world;
    return out;
}

// shadowmap_pars_fragment's texture2DCompare on Bevy's shadow map: the map
// is rendered over three's shadow camera box (`render::shadow`), with
// reversed depth and rows top first, so three's texel and depth are
// recovered from it.
fn texture2DCompare(uv: vec2<f32>, compare: f32, size: f32, layer: i32) -> f32 {
    let n = i32(size);
    let ix = clamp(i32(floor(uv.x * size)), 0, n - 1);
    let iy = clamp(i32(floor(uv.y * size)), 0, n - 1);
#ifdef NO_ARRAY_TEXTURES_SUPPORT
    let stored = textureLoad(directional_shadow_textures, vec2<i32>(ix, n - 1 - iy), 0);
#else
    let stored = textureLoad(directional_shadow_textures, vec2<i32>(ix, n - 1 - iy), layer, 0);
#endif
    let depth = 1.0 - stored;
    return step(compare, depth);
}

// getShadow with SHADOWMAP_TYPE_PCF_SOFT.
fn getShadow(shadow_coord_in: vec4<f32>) -> f32 {
    let params = gl(g::G_SHADOW);
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

@fragment
fn fragment(in: VOut, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    // normal_fragment_begin (first: the derivatives below need uniform
    // control flow, ahead of any discard).
    let face_direction = select(-1.0, 1.0, is_front);
    var normal = normalize(in.normal);
#ifdef DOUBLE_SIDED
    normal = normal * face_direction;
#endif
    let non_perturbed_normal = normal;
    let dxy = max(abs(dpdx(non_perturbed_normal)), abs(dpdy(non_perturbed_normal)));
    let geometry_roughness = max(max(dxy.x, dxy.y), dxy.z);

    var diffuse_color = material.diffuse;
#ifdef USE_MAP
    let map_uv = vec2<f32>(dot(material.map_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.map_t1.xyz, vec3<f32>(in.uv, 1.0)));
    diffuse_color *= textureSample(map_texture, map_sampler, map_uv);
#endif
#ifdef USE_EMISSIVEMAP
    let emissive_uv = vec2<f32>(dot(material.emissive_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.emissive_t1.xyz, vec3<f32>(in.uv, 1.0)));
    let emissive_sample = textureSample(emissive_texture, emissive_sampler, emissive_uv).rgb;
#endif
    // color_fragment
    diffuse_color *= in.color;
#ifdef ALPHA_TEST
    if (diffuse_color.a < material.emissive.w) {
        discard;
    }
#endif

    var outgoing_light: vec3<f32>;
#ifdef LIT
    var total_emissive_radiance = material.emissive.rgb;
#ifdef USE_EMISSIVEMAP
    total_emissive_radiance *= emissive_sample;
#endif

    var m: t::PhysicalMaterial;
#ifdef LIT_PHYSICAL
    // lights_physical_fragment
    let metalness_factor = material.pbr.y;
    let roughness_factor = material.pbr.x;
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
    let point_color = gl(g::G_POINT_COLOR);
    if (any(point_color.rgb != vec3<f32>(0.0))) {
        let point_pos = gl(g::G_POINT_POS);
        let l_vector = (view.view_from_world * vec4<f32>(point_pos.xyz, 1.0)).xyz - geometry_position;
        var light: Incident;
        light.direction = normalize(l_vector);
        light.color = point_color.rgb * t::getDistanceAttenuation(length(l_vector), point_pos.w, point_color.w);
        light.visible = any(light.color != vec3<f32>(0.0));
        re_direct(light, geometry_normal, geometry_view_dir, m, &r);
    }
    let spot_color = gl(g::G_SPOT_COLOR);
    if (spot_color.w > 0.0) {
        let spot_pos = gl(g::G_SPOT_POS);
        let spot_dir = gl(g::G_SPOT_DIR);
        let cone = gl(g::G_SPOT_CONE);
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
    let sun_dir = gl(g::G_SUN_DIR);
    let sun_color = gl(g::G_SUN_COLOR);
    if (sun_dir.w > 0.0) {
        var light: Incident;
        light.color = sun_color.rgb;
        light.direction = view_dir_of(sun_dir.xyz);
        light.visible = true;
        let receive = (mesh[in.instance_index].flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u;
        if (sun_color.w > 0.0 && receive) {
            light.color *= getShadow(in.shadow_coord);
        }
        re_direct(light, geometry_normal, geometry_view_dir, m, &r);
    }
    var irradiance = gl(g::G_AMBIENT).rgb;
    let hemi_sky = gl(g::G_HEMI_SKY);
    let hemi_dir = gl(g::G_HEMI_DIR);
    if (hemi_sky.w > 0.0) {
        irradiance += t::getHemisphereLightIrradiance(hemi_sky.rgb, gl(g::G_HEMI_GROUND).rgb, view_dir_of(hemi_dir.xyz), geometry_normal);
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

    // opaque_fragment
#ifdef OPAQUE
    diffuse_color.a = 1.0;
#endif
    var out_color = vec4<f32>(outgoing_light, diffuse_color.a);

    // fog_fragment, with the game's sun tint (Sky.js) in lit shaders.
#ifdef USE_FOG
    let fog = gl(g::G_FOG);
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
    return out_color;
}
