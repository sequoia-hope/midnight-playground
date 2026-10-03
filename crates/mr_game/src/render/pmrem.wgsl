// three.js r180's PMREMGenerator.fromScene for the sky (`render::pmrem`):
// the six cube faces of the sky drawn into the cube-UV atlas, then the
// spherical Gaussian blurs (SphericalGaussianBlur and its common vertex
// shader). Each pass draws a full-screen triangle into a viewport; the
// fragment works out from its pixel what three's geometry would have
// interpolated there. Rows keep three's order (row 0 is the first row three
// renders, the bottom of its viewport), so three's viewport origins and
// texture coordinates are used as they are.

#import mr::three_std as t
#import mr::sky

struct PassUniform {
    // SphericalGaussianBlur's weights, four to a vector.
    weights: array<vec4<f32>, 5>,
    // poleAxis
    pole_axis: vec4<f32>,
    // samples, latitudinal (0 or 1), dTheta, mipInt
    params: vec4<f32>,
    // the viewport's x, y (three's rows), the face size, the face (sky pass)
    region: vec4<f32>,
};

@group(0) @binding(0) var<uniform> pass_u: PassUniform;
@group(0) @binding(1) var source_texture: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;
@group(0) @binding(3) var globals_texture: texture_2d<f32>;
@group(0) @binding(4) var noise_texture: texture_2d<f32>;
@group(0) @binding(5) var noise_sampler: sampler;

// _sceneToCubeUV: face i seen by a 90° cube camera at the origin, looking
// along forwardSign[i] on axis i % 3 with upSign[i] up.
@fragment
fn sky_face(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = pass_u.region.z;
    let face = i32(pass_u.region.w);
    let ndc = vec2<f32>(2.0 * (pos.x - pass_u.region.x) / size - 1.0, 2.0 * (pos.y - pass_u.region.y) / size - 1.0);
    var up_sign = 1.0;
    if (face == 1) {
        up_sign = -1.0;
    }
    let forward_sign = select(-1.0, 1.0, face < 3);
    let col = face % 3;
    var up: vec3<f32>;
    var forward: vec3<f32>;
    if (col == 0) {
        up = vec3<f32>(0.0, up_sign, 0.0);
        forward = vec3<f32>(forward_sign, 0.0, 0.0);
    } else if (col == 1) {
        up = vec3<f32>(0.0, 0.0, up_sign);
        forward = vec3<f32>(0.0, forward_sign, 0.0);
    } else {
        up = vec3<f32>(0.0, up_sign, 0.0);
        forward = vec3<f32>(0.0, 0.0, forward_sign);
    }
    // Matrix4.lookAt(eye, target, up): z = eye - target, x = up × z, y = z × x.
    let z = -forward;
    let x = normalize(cross(up, z));
    let y = cross(z, x);
    let dir = x * ndc.x + y * ndc.y - z;
    let u = sky::sky_uniforms(globals_texture);
    return vec4<f32>(sky::sky_color(dir, u, noise_texture, noise_sampler), 1.0);
}

// _getCommonVertexShader's getDirection.
fn get_direction(uv_in: vec2<f32>, face: f32) -> vec3<f32> {
    let uv = 2.0 * uv_in - 1.0;
    var direction = vec3<f32>(uv, 1.0);
    if (face == 0.0) {
        direction = direction.zyx;
    } else if (face == 1.0) {
        direction = direction.xzy;
        direction = vec3<f32>(-direction.x, direction.y, -direction.z);
    } else if (face == 2.0) {
        direction.x *= -1.0;
    } else if (face == 3.0) {
        direction = direction.zyx;
        direction = vec3<f32>(-direction.x, direction.y, -direction.z);
    } else if (face == 4.0) {
        direction = direction.xzy;
        direction = vec3<f32>(-direction.x, -direction.y, direction.z);
    } else if (face == 5.0) {
        direction.z *= -1.0;
    }
    return direction;
}

fn get_sample(theta: f32, axis: vec3<f32>, out_dir: vec3<f32>, mip_int: f32) -> vec3<f32> {
    let cos_theta = cos(theta);
    // Rodrigues' axis-angle rotation
    let sample_direction = out_dir * cos_theta
        + cross(axis, out_dir) * sin(theta)
        + axis * dot(axis, out_dir) * (1.0 - cos_theta);
    return t::bilinearCubeUV(source_texture, source_sampler, sample_direction, mip_int);
}

fn weight(i: i32) -> f32 {
    let v = pass_u.weights[i / 4];
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

@fragment
fn blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    // The lod plane: six faces of size s in a 3 × 2 grid, each face's uv
    // running from -1/(s - 2) to 1 + 1/(s - 2) across it.
    let s = pass_u.region.z;
    let px = pos.x - pass_u.region.x;
    let py = pos.y - pass_u.region.y;
    let fx = floor(px / s);
    let fy = floor(py / s);
    let face = fx + 3.0 * fy;
    let texel = 1.0 / (s - 2.0);
    let uv = vec2<f32>(-texel, -texel) + (1.0 + 2.0 * texel) * vec2<f32>((px - fx * s) / s, (py - fy * s) / s);
    let out_dir = get_direction(uv, face);

    let samples = i32(pass_u.params.x);
    let latitudinal = pass_u.params.y > 0.5;
    let d_theta = pass_u.params.z;
    let mip_int = pass_u.params.w;
    let pole_axis = pass_u.pole_axis.xyz;
    var axis = select(cross(pole_axis, out_dir), pole_axis, latitudinal);
    if (all(axis == vec3<f32>(0.0))) {
        axis = vec3<f32>(out_dir.z, 0.0, -out_dir.x);
    }
    axis = normalize(axis);
    var color = vec3<f32>(0.0);
    color += weight(0) * get_sample(0.0, axis, out_dir, mip_int);
    for (var i = 1; i < 20; i++) {
        if (i >= samples) {
            break;
        }
        let theta = d_theta * f32(i);
        color += weight(i) * get_sample(-1.0 * theta, axis, out_dir, mip_int);
        color += weight(i) * get_sample(theta, axis, out_dir, mip_int);
    }
    return vec4<f32>(color, 1.0);
}
