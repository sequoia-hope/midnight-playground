// three_std: the parts of three.js r180's standard and physical shading the
// game uses, in WGSL (SPEC 6.2). Ported from the r180 shader chunks named on
// each block (`common`, `bsdfs`, `lights_pars_begin`,
// `lights_physical_pars_fragment`, `cube_uv_reflection_fragment`,
// `envmap_physical_pars_fragment`, `shadowmap_pars_fragment`,
// `tonemapping_pars_fragment`), with three's names where the JS patches
// refer to them. Everything here is a function of its arguments: the
// bindings live in `mr::three_bindings`.
//
// Ported from three.js r180 (MIT licence, Copyright © 2010-2025 three.js
// authors).

#define_import_path mr::three_std

const PI: f32 = 3.141592653589793;
const RECIPROCAL_PI: f32 = 0.3183098861837907;
const EPSILON: f32 = 1e-6;

// ── common ─────────────────────────────────────────────────────────────

fn pow2f(x: f32) -> f32 { return x * x; }
fn pow4f(x: f32) -> f32 { let x2 = x * x; return x2 * x2; }
fn max3v(v: vec3<f32>) -> f32 { return max(max(v.x, v.y), v.z); }

// GLSL's smoothstep as the GPUs compute it, also for edge0 > edge1 (which
// the sky and fog code use and WGSL leaves undefined).
fn smooth_step(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn BRDF_Lambert(diffuse_color: vec3<f32>) -> vec3<f32> {
    return RECIPROCAL_PI * diffuse_color;
}

fn F_Schlick(f0: vec3<f32>, f90: f32, dotVH: f32) -> vec3<f32> {
    let fresnel = exp2((-5.55473 * dotVH - 6.98316) * dotVH);
    return f0 * (1.0 - fresnel) + (f90 * fresnel);
}

fn F_Schlick_f(f0: f32, f90: f32, dotVH: f32) -> f32 {
    let fresnel = exp2((-5.55473 * dotVH - 6.98316) * dotVH);
    return f0 * (1.0 - fresnel) + (f90 * fresnel);
}

// ── lights_physical_pars_fragment ──────────────────────────────────────

struct PhysicalMaterial {
    diffuse_color: vec3<f32>,
    roughness: f32,
    specular_color: vec3<f32>,
    f90_specular: f32,
    clearcoat: f32,
    clearcoat_roughness: f32,
    f0_clearcoat: vec3<f32>,
    f90_clearcoat: f32,
    sheen_color: vec3<f32>,
    sheen_roughness: f32,
};

fn V_GGX_SmithCorrelated(alpha: f32, dotNL: f32, dotNV: f32) -> f32 {
    let a2 = pow2f(alpha);
    let gv = dotNL * sqrt(a2 + (1.0 - a2) * pow2f(dotNV));
    let gl = dotNV * sqrt(a2 + (1.0 - a2) * pow2f(dotNL));
    return 0.5 / max(gv + gl, EPSILON);
}

fn D_GGX(alpha: f32, dotNH: f32) -> f32 {
    let a2 = pow2f(alpha);
    let denom = pow2f(dotNH) * (a2 - 1.0) + 1.0;
    return RECIPROCAL_PI * a2 / pow2f(denom);
}

// BRDF_GGX and BRDF_GGX_Clearcoat differ only in which f0, f90 and
// roughness they take.
fn BRDF_GGX(light_dir: vec3<f32>, view_dir: vec3<f32>, normal: vec3<f32>, f0: vec3<f32>, f90: f32, roughness: f32) -> vec3<f32> {
    let alpha = pow2f(roughness);
    let half_dir = normalize(light_dir + view_dir);
    let dotNL = saturate(dot(normal, light_dir));
    let dotNV = saturate(dot(normal, view_dir));
    let dotNH = saturate(dot(normal, half_dir));
    let dotVH = saturate(dot(view_dir, half_dir));
    let F = F_Schlick(f0, f90, dotVH);
    let V = V_GGX_SmithCorrelated(alpha, dotNL, dotNV);
    let D = D_GGX(alpha, dotNH);
    return F * (V * D);
}

fn D_Charlie(roughness: f32, dotNH: f32) -> f32 {
    let alpha = pow2f(roughness);
    let inv_alpha = 1.0 / alpha;
    let cos2h = dotNH * dotNH;
    let sin2h = max(1.0 - cos2h, 0.0078125);
    return (2.0 + inv_alpha) * pow(sin2h, inv_alpha * 0.5) / (2.0 * PI);
}

fn V_Neubelt(dotNV: f32, dotNL: f32) -> f32 {
    return saturate(1.0 / (4.0 * (dotNL + dotNV - dotNL * dotNV)));
}

fn BRDF_Sheen(light_dir: vec3<f32>, view_dir: vec3<f32>, normal: vec3<f32>, sheen_color: vec3<f32>, sheen_roughness: f32) -> vec3<f32> {
    let half_dir = normalize(light_dir + view_dir);
    let dotNL = saturate(dot(normal, light_dir));
    let dotNV = saturate(dot(normal, view_dir));
    let dotNH = saturate(dot(normal, half_dir));
    let D = D_Charlie(sheen_roughness, dotNH);
    let V = V_Neubelt(dotNV, dotNL);
    return sheen_color * (D * V);
}

fn IBLSheenBRDF(normal: vec3<f32>, view_dir: vec3<f32>, roughness: f32) -> f32 {
    let dotNV = saturate(dot(normal, view_dir));
    let r2 = roughness * roughness;
    let a = select(-8.48 * r2 + 14.3 * roughness - 9.95, -339.2 * r2 + 161.4 * roughness - 25.9, roughness < 0.25);
    let b = select(1.97 * r2 - 3.27 * roughness + 0.72, 44.0 * r2 - 23.7 * roughness + 3.26, roughness < 0.25);
    let DG = exp(a * dotNV + b) + select(0.1 * (roughness - 0.25), 0.0, roughness < 0.25);
    return saturate(DG * RECIPROCAL_PI);
}

fn DFGApprox(normal: vec3<f32>, view_dir: vec3<f32>, roughness: f32) -> vec2<f32> {
    let dotNV = saturate(dot(normal, view_dir));
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * dotNV)) * r.x + r.y;
    let fab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return fab;
}

fn EnvironmentBRDF(normal: vec3<f32>, view_dir: vec3<f32>, specular_color: vec3<f32>, f90_specular: f32, roughness: f32) -> vec3<f32> {
    let fab = DFGApprox(normal, view_dir, roughness);
    return specular_color * fab.x + f90_specular * fab.y;
}

struct Scattering {
    single: vec3<f32>,
    multi: vec3<f32>,
};

fn computeMultiscattering(normal: vec3<f32>, view_dir: vec3<f32>, specular_color: vec3<f32>, f90_specular: f32, roughness: f32) -> Scattering {
    let fab = DFGApprox(normal, view_dir, roughness);
    let Fr = specular_color;
    let FssEss = Fr * fab.x + f90_specular * fab.y;
    let Ess = fab.x + fab.y;
    let Ems = 1.0 - Ess;
    let Favg = Fr + (1.0 - Fr) * 0.047619;
    let Fms = FssEss * Favg / (1.0 - Ems * Favg);
    return Scattering(FssEss, Fms * Ems);
}

// ── lights_pars_begin ──────────────────────────────────────────────────

fn getDistanceAttenuation(light_distance: f32, cutoff_distance: f32, decay_exponent: f32) -> f32 {
    var distance_falloff = 1.0 / max(pow(light_distance, decay_exponent), 0.01);
    if (cutoff_distance > 0.0) {
        distance_falloff *= pow2f(saturate(1.0 - pow4f(light_distance / cutoff_distance)));
    }
    return distance_falloff;
}

fn getSpotAttenuation(cone_cosine: f32, penumbra_cosine: f32, angle_cosine: f32) -> f32 {
    return smooth_step(cone_cosine, penumbra_cosine, angle_cosine);
}

fn getHemisphereLightIrradiance(sky_color: vec3<f32>, ground_color: vec3<f32>, direction: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let dotNL = dot(normal, direction);
    let hemi_diffuse_weight = 0.5 * dotNL + 0.5;
    return mix(ground_color, sky_color, hemi_diffuse_weight);
}

// ── cube_uv_reflection_fragment (PMREMGenerator's layout, size 256) ────

const cubeUV_minMipLevel: f32 = 4.0;
const cubeUV_minTileSize: f32 = 16.0;
// PMREMGenerator.fromScene at the default size: lodMax 8, a 768 × 1024 atlas.
const CUBEUV_MAX_MIP: f32 = 8.0;
const CUBEUV_TEXEL_WIDTH: f32 = 0.0013020833333333333;
const CUBEUV_TEXEL_HEIGHT: f32 = 0.0009765625;

fn getFace(direction: vec3<f32>) -> f32 {
    let abs_direction = abs(direction);
    var face = -1.0;
    if (abs_direction.x > abs_direction.z) {
        if (abs_direction.x > abs_direction.y) {
            face = select(3.0, 0.0, direction.x > 0.0);
        } else {
            face = select(4.0, 1.0, direction.y > 0.0);
        }
    } else {
        if (abs_direction.z > abs_direction.y) {
            face = select(5.0, 2.0, direction.z > 0.0);
        } else {
            face = select(4.0, 1.0, direction.y > 0.0);
        }
    }
    return face;
}

fn getUV(direction: vec3<f32>, face: f32) -> vec2<f32> {
    var uv: vec2<f32>;
    if (face == 0.0) {
        uv = vec2<f32>(direction.z, direction.y) / abs(direction.x);
    } else if (face == 1.0) {
        uv = vec2<f32>(-direction.x, -direction.z) / abs(direction.y);
    } else if (face == 2.0) {
        uv = vec2<f32>(-direction.x, direction.y) / abs(direction.z);
    } else if (face == 3.0) {
        uv = vec2<f32>(-direction.z, direction.y) / abs(direction.x);
    } else if (face == 4.0) {
        uv = vec2<f32>(-direction.x, direction.z) / abs(direction.y);
    } else {
        uv = vec2<f32>(direction.x, direction.y) / abs(direction.z);
    }
    return 0.5 * (uv + 1.0);
}

// The atlas keeps three's row order (row 0 is the first row three renders,
// the bottom of its viewport), so three's texture coordinates read it as
// they are.
fn bilinearCubeUV(env_map: texture_2d<f32>, env_sampler: sampler, direction: vec3<f32>, mip_int_in: f32) -> vec3<f32> {
    var face = getFace(direction);
    let filter_int = max(cubeUV_minMipLevel - mip_int_in, 0.0);
    let mip_int = max(mip_int_in, cubeUV_minMipLevel);
    let face_size = exp2(mip_int);
    var uv = getUV(direction, face) * (face_size - 2.0) + 1.0;
    if (face > 2.0) {
        uv.y += face_size;
        face -= 3.0;
    }
    uv.x += face * face_size;
    uv.x += filter_int * 3.0 * cubeUV_minTileSize;
    uv.y += 4.0 * (exp2(CUBEUV_MAX_MIP) - face_size);
    uv.x *= CUBEUV_TEXEL_WIDTH;
    uv.y *= CUBEUV_TEXEL_HEIGHT;
    return textureSampleLevel(env_map, env_sampler, uv, 0.0).rgb;
}

fn roughnessToMip(roughness: f32) -> f32 {
    let cubeUV_r0: f32 = 1.0;
    let cubeUV_m0: f32 = -2.0;
    let cubeUV_r1: f32 = 0.8;
    let cubeUV_m1: f32 = -1.0;
    let cubeUV_r4: f32 = 0.4;
    let cubeUV_m4: f32 = 2.0;
    let cubeUV_r5: f32 = 0.305;
    let cubeUV_m5: f32 = 3.0;
    let cubeUV_r6: f32 = 0.21;
    let cubeUV_m6: f32 = 4.0;
    var mip = 0.0;
    if (roughness >= cubeUV_r1) {
        mip = (cubeUV_r0 - roughness) * (cubeUV_m1 - cubeUV_m0) / (cubeUV_r0 - cubeUV_r1) + cubeUV_m0;
    } else if (roughness >= cubeUV_r4) {
        mip = (cubeUV_r1 - roughness) * (cubeUV_m4 - cubeUV_m1) / (cubeUV_r1 - cubeUV_r4) + cubeUV_m1;
    } else if (roughness >= cubeUV_r5) {
        mip = (cubeUV_r4 - roughness) * (cubeUV_m5 - cubeUV_m4) / (cubeUV_r4 - cubeUV_r5) + cubeUV_m4;
    } else if (roughness >= cubeUV_r6) {
        mip = (cubeUV_r5 - roughness) * (cubeUV_m6 - cubeUV_m5) / (cubeUV_r5 - cubeUV_r6) + cubeUV_m5;
    } else {
        mip = -2.0 * log2(1.16 * roughness);
    }
    return mip;
}

fn textureCubeUV(env_map: texture_2d<f32>, env_sampler: sampler, sample_dir: vec3<f32>, roughness: f32) -> vec4<f32> {
    let mip = clamp(roughnessToMip(roughness), -2.0, CUBEUV_MAX_MIP);
    let mip_f = fract(mip);
    let mip_int = floor(mip);
    let color0 = bilinearCubeUV(env_map, env_sampler, sample_dir, mip_int);
    if (mip_f == 0.0) {
        return vec4<f32>(color0, 1.0);
    }
    let color1 = bilinearCubeUV(env_map, env_sampler, sample_dir, mip_int + 1.0);
    return vec4<f32>(mix(color0, color1, mip_f), 1.0);
}

// ── tonemapping_pars_fragment, colorspace_pars_fragment ────────────────

fn RRTAndODTFit(v: vec3<f32>) -> vec3<f32> {
    let a = v * (v + 0.0245786) - 0.000090537;
    let b = v * (0.983729 * v + 0.4329510) + 0.238081;
    return a / b;
}

fn ACESFilmicToneMapping(color_in: vec3<f32>, tone_mapping_exposure: f32) -> vec3<f32> {
    let aces_input = mat3x3<f32>(
        vec3<f32>(0.59719, 0.07600, 0.02840),
        vec3<f32>(0.35458, 0.90834, 0.13383),
        vec3<f32>(0.04823, 0.01566, 0.83777),
    );
    let aces_output = mat3x3<f32>(
        vec3<f32>(1.60475, -0.10208, -0.00327),
        vec3<f32>(-0.53108, 1.10813, -0.07276),
        vec3<f32>(-0.07367, -0.00605, 1.07602),
    );
    var color = color_in * (tone_mapping_exposure / 0.6);
    color = aces_input * color;
    color = RRTAndODTFit(color);
    color = aces_output * color;
    return saturate(color);
}

fn sRGBTransferOETF(value: vec3<f32>) -> vec3<f32> {
    return select(pow(value, vec3<f32>(0.41666)) * 1.055 - vec3<f32>(0.055), value * 12.92, value <= vec3<f32>(0.0031308));
}
