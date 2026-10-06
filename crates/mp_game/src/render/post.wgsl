// The post chain of `src/main.js` (`render::post`): three.js r180's
// UnrealBloomPass (LuminosityHighPassShader, the separable Gaussian blur at
// five sizes, the composite) and OutputPass (ACES filmic with the exposure,
// then sRGB). The bloom's additive blend over the scene (CopyShader with
// AdditiveBlending, SRC_ALPHA × src + dst, the composite's alpha being
// bloomStrength × Σ factors) is folded into the output pass; sRGB encoding
// is the surface's.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import mr::three_std as t

struct PostUniform {
    // x: luminosityThreshold, y: smoothWidth, z: toneMappingExposure, w: -
    a: vec4<f32>,
    // blur: invSize.xy, direction.xy
    b: vec4<f32>,
    // composite: bloomStrength, bloomRadius, kernel radius (blur), -
    c: vec4<f32>,
    // blur coefficients (gaussianCoefficients), four to a vector
    coefficients: array<vec4<f32>, 3>,
};

@group(0) @binding(0) var<uniform> u: PostUniform;
@group(0) @binding(1) var t0: texture_2d<f32>;
@group(0) @binding(2) var s0: sampler;
@group(0) @binding(3) var t1: texture_2d<f32>;
@group(0) @binding(4) var t2: texture_2d<f32>;
@group(0) @binding(5) var t3: texture_2d<f32>;
@group(0) @binding(6) var t4: texture_2d<f32>;

fn luminance(rgb: vec3<f32>) -> f32 {
    let weights = vec3<f32>(0.2126, 0.7152, 0.0722);
    return dot(weights, rgb);
}

// LuminosityHighPassShader (defaultColor black, defaultOpacity 0).
@fragment
fn high_pass(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(t0, s0, in.uv);
    let v = luminance(texel.xyz);
    let output_color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    let alpha = t::smooth_step(u.a.x, u.a.x + u.a.y, v);
    return mix(output_color, texel, alpha);
}

fn coefficient(i: i32) -> f32 {
    let v = u.coefficients[i / 4];
    let k = i % 4;
    if (k == 0) {
        return v.x;
    } else if (k == 1) {
        return v.y;
    } else if (k == 2) {
        return v.z;
    }
    return v.w;
}

// _getSeparableBlurMaterial.
@fragment
fn blur(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let kernel_radius = i32(u.c.z);
    let inv_size = u.b.xy;
    let direction = u.b.zw;
    var weight_sum = coefficient(0);
    var diffuse_sum = textureSampleLevel(t0, s0, in.uv, 0.0).rgb * weight_sum;
    for (var i = 1; i < 11; i++) {
        if (i >= kernel_radius) {
            break;
        }
        let x = f32(i);
        let w = coefficient(i);
        let uv_offset = direction * inv_size * x;
        let sample1 = textureSampleLevel(t0, s0, in.uv + uv_offset, 0.0).rgb;
        let sample2 = textureSampleLevel(t0, s0, in.uv - uv_offset, 0.0).rgb;
        diffuse_sum += (sample1 + sample2) * w;
        weight_sum += 2.0 * w;
    }
    return vec4<f32>(diffuse_sum / weight_sum, 1.0);
}

fn lerp_bloom_factor(factor: f32) -> f32 {
    let mirror_factor = 1.2 - factor;
    return mix(factor, mirror_factor, u.c.y);
}

// _getCompositeMaterial (tints all white).
@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return u.c.x * (
        lerp_bloom_factor(1.0) * textureSample(t0, s0, in.uv) +
        lerp_bloom_factor(0.8) * textureSample(t1, s0, in.uv) +
        lerp_bloom_factor(0.6) * textureSample(t2, s0, in.uv) +
        lerp_bloom_factor(0.4) * textureSample(t3, s0, in.uv) +
        lerp_bloom_factor(0.2) * textureSample(t4, s0, in.uv));
}

// The bloom blended over the scene, then OutputPass: ACES filmic with the
// exposure. The surface is sRGB, so the sRGB transfer is the hardware's.
@fragment
fn output(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(t0, s0, in.uv);
    let bloom = textureSample(t1, s0, in.uv);
    // AdditiveBlending: src × src.a + dst, src.a being the composite's alpha.
    let rgb = scene.rgb + bloom.rgb * bloom.a;
    return vec4<f32>(t::ACESFilmicToneMapping(rgb, u.a.z), 1.0);
}
