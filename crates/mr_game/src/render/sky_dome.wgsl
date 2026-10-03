// The SkyDome kind (`Sky.js`, skyVert and the dome ShaderMaterial): the
// direction is the unit sphere's own position, and the dome is drawn at the
// far plane (three's `gl_Position = p.xyww`; Bevy's depth is reversed, so
// far is 0). BackSide, no depth write, no fog.

#import bevy_pbr::{mesh_functions, mesh_view_bindings::view}
#import mr::sky

@group(#{MATERIAL_BIND_GROUP}) @binding(10) var globals_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var noise_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var noise_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) dir: vec3<f32>,
};

@vertex
fn vertex(v: Vertex) -> VOut {
    var out: VOut;
    out.dir = normalize(v.position);
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    let p = view.clip_from_world * world;
    out.clip = vec4<f32>(p.xy, 0.0, p.w);
    return out;
}

@fragment
fn fragment(in: VOut) -> @location(0) vec4<f32> {
    let u = sky::sky_uniforms(globals_texture);
    return vec4<f32>(sky::sky_color(in.dir, u, noise_texture, noise_sampler), 1.0);
}
