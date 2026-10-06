// The shadow pass of an alpha-tested three_std material: three's depth
// material keeps the alpha test (with the map's alpha), so cut-out leaves
// and fences cast cut-out shadows.

#import bevy_pbr::prepass_io::VertexOutput

struct ThreeParams {
    diffuse: vec4<f32>,
    emissive: vec4<f32>,
    pbr: vec4<f32>,
    sheen: vec4<f32>,
    specular: vec4<f32>,
    physical: vec4<f32>,
    map_t0: vec4<f32>,
    map_t1: vec4<f32>,
    emissive_t0: vec4<f32>,
    emissive_t1: vec4<f32>,
    normal_t0: vec4<f32>,
    normal_t1: vec4<f32>,
    kind0: vec4<f32>,
    kind1: vec4<f32>,
    night: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: ThreeParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var map_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var map_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) {
    var alpha = material.diffuse.a;
#ifdef USE_MAP
#ifdef VERTEX_UVS_A
    let uv = vec2<f32>(dot(material.map_t0.xyz, vec3<f32>(in.uv, 1.0)), dot(material.map_t1.xyz, vec3<f32>(in.uv, 1.0)));
    alpha *= textureSample(map_texture, map_sampler, uv).a;
#endif
#endif
#ifdef VERTEX_COLORS
    alpha *= in.color.a;
#endif
#ifdef ALPHA_TEST
    if (alpha < material.emissive.w) {
        discard;
    }
#endif
}
