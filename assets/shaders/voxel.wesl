// Block terrain shader: Bevy's standard PBR lighting and fog, but the base
// colour comes from the block texture array. Greedy-merged quads span many
// blocks, so the UVs run 0..size and `fract` repeats the tile per block.
// uv_b.x carries the texture layer (tile index in blocks.png) in its whole
// part and corner shading in its fraction (`chunk_mesh::pack_layer`); uv_b.y
// is how strongly the season tints the face (leaves 1, grass tops 0.5).

import bevy_pbr::render::{
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var block_textures: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var block_sampler: sampler;
// rgb: the season's foliage colour; a: how far to lean towards it.
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> season: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var<uniform> atmos: Atmos;

// --- Sky-matched fog and ground mist. Kept the same in voxel.wesl and
// lod.wesl; the horizon formula matches sky.wesl at the horizon. Fields: see
// `atmosphere::AtmosParams`.
struct Atmos {
    horizon: vec4<f32>,
    glow: vec4<f32>,
    sun: vec4<f32>,
    mist: vec4<f32>,
}

// The sky's colour at the horizon, looking along `dir`.
fn horizon_color(a: Atmos, dir: vec3<f32>) -> vec3<f32> {
    let flat = normalize(vec3<f32>(dir.x, 0.0, dir.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let sun_flat = normalize(vec3<f32>(a.sun.x, 0.0, a.sun.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let ft = max(dot(flat, sun_flat), 0.0);
    let toward = max(dot(flat, a.sun.xyz), 0.0);
    let col = a.horizon.rgb * (1.0 + a.mist.w * pow(toward, 5.0));
    let glow = a.sun.w * (pow(ft, 3.0) * 0.8 + pow(toward, 12.0) * 0.5);
    return mix(col, a.glow.rgb, clamp(glow, 0.0, 0.9));
}

fn mist_density(a: Atmos, y: f32) -> f32 {
    return a.mist.y * exp(-max(y - a.mist.x, 0.0) / a.mist.z);
}

// Fade `color` at `world` into the sky behind it, through haze and mist.
fn apply_atmosphere(a: Atmos, color: vec3<f32>, world: vec3<f32>, cam: vec3<f32>) -> vec3<f32> {
    let d = world - cam;
    let dist = length(d);
    let dir = d / max(dist, 1e-4);
    let haze = clamp((dist - a.horizon.w) / max(a.glow.w - a.horizon.w, 1.0), 0.0, 1.0);
    // Mist along the ray (Simpson's rule over the heights it passes through).
    let depth = dist / 6.0 * (mist_density(a, cam.y) + 4.0 * mist_density(a, 0.5 * (cam.y + world.y)) + mist_density(a, world.y));
    let mist = 1.0 - exp(-depth);
    let amount = 1.0 - (1.0 - haze) * (1.0 - mist);
    return mix(color, horizon_color(a, dir), amount);
}
// --- end of shared fog

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let layer = u32(floor(in.uv_b.x));
    // Corner shading: 0 in the open, 1 deep in a crease.
    let crease = clamp((fract(in.uv_b.x) - 0.05) / 0.9, 0.0, 1.0);
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
    // Creases go darker: a little for direct light (the classic block look),
    // more for sky light, which a crease really does shut out.
    rgb *= 1.0 - 0.35 * crease;
    pbr_input.material.base_color = vec4<f32>(rgb * pbr_input.material.base_color.rgb, texel.a);
    pbr_input.diffuse_occlusion *= vec3<f32>(1.0 - 0.6 * crease);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Tonemapping and the like (Bevy's fog is off for this material).
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    out.color = vec4<f32>(apply_atmosphere(atmos, out.color.rgb, in.world_position.xyz, view.world_position.xyz), out.color.a);
    return out;
}
