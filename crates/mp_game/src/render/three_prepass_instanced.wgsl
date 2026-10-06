// The shadow pass's vertex stage for an InstancedMesh drawn as one entity
// (`render::instancing`, DECISIONS D450): Bevy 0.19.1's prepass vertex
// shader (`bevy_pbr/src/prepass/prepass.wgsl`) for what the shadow pass
// uses (position, uv and colour for the alpha test, the unclipped-depth
// emulation), with the world matrix read from the instance stream
// (three's `instanceMatrix`, the InstancedMesh's `matrixWorld` folded in)
// instead of Bevy's mesh uniform. The fragment stage is unchanged
// (`three_prepass.wgsl`, or none).

#import bevy_pbr::{
    prepass_io::VertexOutput,
    view_transformations::position_world_to_clip,
}

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(1) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(2) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(7) color: vec4<f32>,
#endif
    @location(9) i_col0: vec4<f32>,
    @location(10) i_col1: vec4<f32>,
    @location(11) i_col2: vec4<f32>,
    @location(12) i_col3: vec4<f32>,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mat4x4<f32>(v.i_col0, v.i_col1, v.i_col2, v.i_col3);
    out.world_position = world_from_local * vec4<f32>(v.position, 1.0);
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0); // Clamp depth to avoid clipping
#endif
#ifdef VERTEX_UVS_A
    out.uv = v.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = v.uv_b;
#endif
#ifdef VERTEX_COLORS
    out.color = v.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = v.instance_index;
#endif
    return out;
}
