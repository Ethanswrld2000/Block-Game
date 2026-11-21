// Block terrain shader: Bevy's standard PBR lighting and fog, but the base
// colour comes from the block texture array. Greedy-merged quads span many
// blocks, so the UVs run 0..size and `fract` repeats the tile per block.
// uv_b.x carries the texture layer (tile index in blocks.png) in its whole
// part and corner shading in its fraction (`chunk_mesh::pack_layer`); uv_b.y
// is how strongly the season tints the face (leaves 1, grass tops 0.5).

import bevy_pbr::render::{
    mesh_view_bindings::{view, globals},
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var block_textures: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var block_sampler: sampler;
// rgb: the season's foliage colour; a: how far to lean towards it.
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> season: vec4<f32>;
// xy: wind direction (x, z); z: how hard (see voxel_vertex.wesl).
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> wind: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var<uniform> atmos: Atmos;
// x: the glowcap's texture layer; y: how bright it glows (0 by day, 1 at night).
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var<uniform> plant_glow: vec4<f32>;
// Per tile index (four per vector): bits 0-9 first extra version, 10-13 how
// many versions with the base, bit 14 turn (`voxel::tiles::pack_variant`).
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var<uniform> variants: array<vec4<u32>, 64>;

fn cell_hash(c: vec3<i32>) -> u32 {
    var h = (bitcast<u32>(c.x) * 73856093u) ^ (bitcast<u32>(c.y) * 19349663u) ^ (bitcast<u32>(c.z) * 83492791u);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    return h ^ (h >> 16u);
}

// A quarter turn `r` times, around the middle of the tile.
fn turn(v: vec2<f32>, r: u32) -> vec2<f32> {
    switch r {
        case 1u: { return vec2<f32>(-v.y, v.x); }
        case 2u: { return -v; }
        case 3u: { return vec2<f32>(v.y, -v.x); }
        default: { return v; }
    }
}

// The block's tile, in this block's version and turn. Greedy quads span many
// blocks, so the block comes from the world position, and the mip level from
// the unbroken quad UVs (`fract` would jump a level at every block edge).
fn sample_tile(base: u32, uv: vec2<f32>, world: vec3<f32>, normal: vec3<f32>) -> vec4<f32> {
    let packed = variants[base / 4u][base % 4u];
    let count = (packed >> 10u) & 15u;
    let h = cell_hash(vec3<i32>(floor(world - normal * 0.5)));
    var layer = base;
    if count > 1u {
        let k = h % count;
        if k > 0u {
            layer = (packed & 1023u) + k - 1u;
        }
    }
    var r = 0u;
    if ((packed >> 14u) & 1u) == 1u {
        r = (h >> 20u) & 3u;
    }
    let p = turn(fract(uv) - vec2<f32>(0.5), r) + vec2<f32>(0.5);
    return textureSampleGrad(block_textures, block_sampler, p, layer, turn(dpdx(uv), r), turn(dpdy(uv), r));
}

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

// Water surface: gentle waves drifting with the wind plus small ripples. Only
// the lighting normal moves (greedy quads have too few corners to bend), so
// the sun's glints and the shading roll across the water. Returns the slope
// (d height / dx, d height / dz) at `p` (world x, z) and time `t`.
fn water_slope(p: vec2<f32>, t: f32) -> vec2<f32> {
    let w = normalize(wind.xy + vec2<f32>(1e-4, 0.0));
    let side = vec2<f32>(-w.y, w.x);
    let calm = 0.6 + 0.4 * clamp(wind.z, 0.0, 1.0);
    // (direction, wavelength in blocks, height, speed)
    var slope = vec2<f32>(0.0);
    let dirs = array<vec2<f32>, 4>(w, normalize(w + side * 0.7), normalize(w - side * 0.9), side);
    let lens = vec4<f32>(7.0, 4.3, 3.1, 1.7);
    let amps = vec4<f32>(0.09, 0.06, 0.04, 0.02);
    let speeds = vec4<f32>(0.9, 1.2, 1.5, 2.1);
    for (var i = 0; i < 4; i++) {
        let k = 6.2831853 / lens[i];
        let phase = dot(dirs[i], p) * k - t * speeds[i];
        slope += dirs[i] * cos(phase) * amps[i] * k;
    }
    // Small ripples crossing each other, different at every spot.
    let r1 = p * 2.3 + vec2<f32>(t * 0.7, -t * 0.5);
    let r2 = p.yx * 1.9 + vec2<f32>(-t * 0.6, t * 0.8);
    slope += vec2<f32>(cos(r1.x + sin(r1.y)), cos(r1.y + sin(r1.x))) * 0.03
        + vec2<f32>(cos(r2.y + sin(r2.x)), cos(r2.x + sin(r2.y))) * 0.05;
    return slope * calm;
}

// Water tops sit 0.12 below the block (`chunk_mesh`); ice tops don't.
fn is_water_top(world: vec3<f32>, normal: vec3<f32>, alpha: f32) -> bool {
    let f = fract(world.y);
    return alpha < 0.98 && normal.y > 0.9 && f > 0.86 && f < 0.9;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let layer = u32(floor(in.uv_b.x));
    // Corner shading: 0 in the open, 1 deep in a crease.
    let crease = clamp((fract(in.uv_b.x) - 0.05) / 0.9, 0.0, 1.0);
    let texel = sample_tile(layer, in.uv, in.world_position.xyz, in.world_normal);
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
    // Glowcaps light up at night (bloom picks the glow up).
    if plant_glow.y > 0.0 && plant_glow.x >= 0.0 && layer == u32(plant_glow.x) {
        // (alpha 0: not scaled by the camera exposure.)
        pbr_input.material.emissive = vec4<f32>(rgb * 3.0 * plant_glow.y, 0.0);
    }
    let water = is_water_top(in.world_position.xyz, in.world_normal, texel.a);
    var wave_n = vec3<f32>(0.0, 1.0, 0.0);
    if water {
        let slope = water_slope(in.world_position.xz, globals.time);
        wave_n = normalize(vec3<f32>(-slope.x, 1.0, -slope.y));
        pbr_input.N = select(-wave_n, wave_n, is_front);
        pbr_input.world_normal = pbr_input.N;
    }

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Tonemapping and the like (Bevy's fog is off for this material).
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    if water && is_front {
        // The sky mirrored in the moving surface, more at grazing angles, and
        // the sun glinting off the wave faces turned towards it.
        let v = normalize(in.world_position.xyz - view.world_position.xyz);
        let r = reflect(v, wave_n);
        let fresnel = 0.04 + 0.96 * pow(1.0 - max(dot(-v, wave_n), 0.0), 5.0);
        let sky = horizon_color(atmos, vec3<f32>(r.x, 0.0, r.z));
        let glint = pow(max(dot(r, normalize(atmos.sun.xyz)), 0.0), 180.0) * 2.0 * max(atmos.sun.y, 0.0);
        let rgb = mix(out.color.rgb, sky, clamp(fresnel * 0.7, 0.0, 0.6)) + vec3<f32>(glint);
        out.color = vec4<f32>(rgb, max(out.color.a, clamp(fresnel, 0.0, 1.0)));
    }
    out.color = vec4<f32>(apply_atmosphere(atmos, out.color.rgb, in.world_position.xyz, view.world_position.xyz), out.color.a);
    return out;
}
