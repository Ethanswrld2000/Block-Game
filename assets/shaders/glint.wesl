// The holographic glint on things coming together at a bench: lit like any
// other item, then bands of shifting colour sweep diagonally across it,
// stepped to the item's pixel grid, and the whole thing flashes brighter as
// `glint.x` (0..1) rises. `glint.y` is the time, `glint.zw` the sprite
// sheet's size in pixels.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> glint: vec4<f32>;

// A colour from a hue (0..1), fully saturated.
fn hue(h: f32) -> vec3<f32> {
    let k = vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0);
    return clamp(abs(fract(h + k) * 6.0 - 3.0) - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    let s = glint.x;
    let t = glint.y;
    // Snap to the item's pixels (the sprite sheet has 20 px per cell).
    let px = floor(in.uv * glint.zw);
    // Two bands, crossing, sliding along at different speeds.
    let a = fract((px.x + px.y) / 22.0 - t * 0.9);
    let b = fract((px.x - px.y * 0.6) / 31.0 + t * 0.55);
    let band = pow(1.0 - abs(a * 2.0 - 1.0), 6.0) + 0.6 * pow(1.0 - abs(b * 2.0 - 1.0), 8.0);
    let col = mix(vec3<f32>(0.62, 0.38, 1.0), hue(fract((px.x + px.y) / 90.0 + t * 0.3)), 0.55);
    let lit = out.color.rgb + col * band * s * 2.2 + col * s * s * 0.6;
    out.color = vec4<f32>(lit, out.color.a);
    return out;
}
