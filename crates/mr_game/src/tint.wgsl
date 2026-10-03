// Instance colours (three's `InstancedMesh.instanceColor`) for the stand-in
// materials: the colour rides in the instance's `MeshTag`, 10 bits a channel
// over 0..2 (linear), and multiplies the base colour, as three multiplies the
// vertex colour by it. Otherwise this is Bevy's own StandardMaterial fragment
// (the extended-material pattern).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    mesh_functions::get_tag,
}
#import bevy_pbr::pbr_types

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#endif

fn unpack_tint(tag: u32) -> vec3<f32> {
    return vec3<f32>(
        f32(tag & 1023u),
        f32((tag >> 10u) & 1023u),
        f32((tag >> 20u) & 1023u),
    ) / 511.5;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let tint = unpack_tint(get_tag(in.instance_index));
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * tint, pbr_input.material.base_color.a);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    if (pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
#endif
    return out;
}
