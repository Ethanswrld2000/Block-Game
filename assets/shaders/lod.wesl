// Distant terrain: plain vertex colours, lit and fogged like everything else.
// Where a real chunk is drawn this mesh cuts itself out, so the chunk shows.
// `mask` holds the drawn chunks around chunk `center.xyz`: one texel per
// column of chunks ((2 * center.w + 1) square), one bit per vertical offset
// (bit dy + 15, see `world::MASK_UP`). The cut follows the chunk grid exactly.

import bevy_pbr::render::{
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> center: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var mask: texture_2d<u32>;

fn chunk_drawn(p: vec3<f32>) -> bool {
    let off = vec3<i32>(floor(p / 32.0)) - vec3<i32>(center.xyz);
    let half = i32(center.w);
    if (abs(off.x) > half || abs(off.z) > half || abs(off.y) > 15) {
        return false;
    }
    let bits = textureLoad(mask, vec2<i32>(off.x + half, off.z + half), 0).r;
    return ((bits >> u32(off.y + 15)) & 1u) == 1u;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    // Step half a block into the column this face belongs to, so a wall lying
    // on a chunk border is judged by its own chunk.
    if (chunk_drawn(in.world_position.xyz - normalize(in.world_normal) * 0.5)) {
        discard;
    }
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
