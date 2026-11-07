// Block terrain shader: Bevy's standard PBR lighting and fog, but the base
// colour comes from the block texture array. Greedy-merged quads span many
// blocks, so the UVs run 0..size and `fract` repeats the tile per block.
// uv_b.x carries the texture layer (tile index in blocks.png).

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var block_textures: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var block_sampler: sampler;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let layer = u32(in.uv_b.x + 0.5);
    let texel = textureSample(block_textures, block_sampler, fract(in.uv), layer);
    // Cut-out for leaves and plants (alpha 0); water and ice keep their alpha
    // and are drawn by the blended material.
    if texel.a < 0.1 {
        discard;
    }
    pbr_input.material.base_color = vec4<f32>(texel.rgb * pbr_input.material.base_color.rgb, texel.a);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Fog, tonemapping and the like.
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
