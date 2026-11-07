// Distant terrain: plain vertex colours, lit and fogged like everything else.
// Inside the loaded area the real chunks are drawn, so this mesh cuts itself
// out there (near.x = loaded radius, near.y = loaded height above and below).

import bevy_pbr::render::{
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> near: vec4<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let d = in.world_position.xyz - view.world_position.xyz;
    if (dot(d.xz, d.xz) < near.x * near.x && abs(d.y) < near.y) {
        discard;
    }
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
