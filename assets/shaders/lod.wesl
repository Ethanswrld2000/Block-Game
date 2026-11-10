// Distant terrain: plain vertex colours, lit like everything else and fogged
// towards the sky like the block terrain (see `atmosphere`).
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
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> atmos: Atmos;

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
    out.color = vec4<f32>(apply_atmosphere(atmos, out.color.rgb, in.world_position.xyz, view.world_position.xyz), out.color.a);
    return out;
}
