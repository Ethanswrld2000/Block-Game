// Block terrain shader: Bevy's standard PBR lighting and fog, but the base
// colour comes from the block texture array. Greedy-merged quads span many
// blocks, so the UVs run 0..size and `fract` repeats the tile per block.
// uv_b.x carries the texture layer (tile index in blocks.png); uv_b.y is how
// strongly the season tints the face (leaves 1, grass tops 0.5).

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var block_textures: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var block_sampler: sampler;
// rgb: the season's foliage colour; a: how far to lean towards it.
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> season: vec4<f32>;

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
    var rgb = texel.rgb;
    // Leaves (uv_b.y = 1) and grass tops (0.5) take the season's colour,
    // keeping their shading.
    if (in.uv_b.y > 0.25) {
        let luma = dot(rgb, vec3<f32>(0.3, 0.55, 0.15));
        rgb = mix(rgb, season.rgb * luma * 2.4, season.a * in.uv_b.y);
    }
    pbr_input.material.base_color = vec4<f32>(rgb * pbr_input.material.base_color.rgb, texel.a);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Fog, tonemapping and the like.
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
