// Sky dome: a gradient from horizon to zenith, sunset glow, sun and moon
// discs, and stars at night. Drawn on a huge sphere around the camera with
// fog off; the horizon colour matches the fog so distant land fades into it.

import bevy_pbr::render::{
    mesh_view_bindings::view,
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
};

struct SkyParams {
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    // xyz: direction to the sun; w: how much sunset glow (0..1)
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    // xyz: direction to the moon; w: moon brightness
    moon_dir: vec4<f32>,
    // x: star brightness, y: star rotation (radians), z: lightning flash, w: cloudiness
    extra: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: SkyParams;

fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    let r = q + dot(q, q.yxz + 33.33);
    return fract((r.x + r.y) * r.z);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    let dir = normalize(in.world_position.xyz - view.world_position.xyz);
    let up = dir.y;

    // Gradient: horizon colour low down, zenith colour overhead.
    let t = pow(clamp(up, 0.0, 1.0), 0.45);
    var col = mix(sky.horizon.rgb, sky.zenith.rgb, t);
    // Below the horizon: a slightly darker version of the horizon.
    col = mix(col, sky.horizon.rgb * 0.85, clamp(-up * 4.0, 0.0, 1.0));

    // Sunset glow around the sun, strongest near the horizon.
    let sun = sky.sun_dir.xyz;
    let toward = max(dot(dir, sun), 0.0);
    let flat_toward = max(dot(normalize(vec3<f32>(dir.x, 0.0, dir.z) + 1e-5), normalize(vec3<f32>(sun.x, 0.0, sun.z) + 1e-5)), 0.0);
    let low = 1.0 - clamp(abs(up) * 2.5, 0.0, 1.0);
    let glow = sky.sun_dir.w * (pow(flat_toward, 3.0) * low * 0.8 + pow(toward, 12.0) * 0.5);
    col = mix(col, sky.sun_color.rgb, clamp(glow, 0.0, 0.9));

    let clear = 1.0 - sky.extra.w;

    // Stars, slowly turning with the night.
    let a = sky.extra.y;
    let rd = vec3<f32>(dir.x * cos(a) - dir.y * sin(a), dir.x * sin(a) + dir.y * cos(a), dir.z);
    let cell = floor(rd * 220.0);
    let h = hash3(cell);
    if (h > 0.9975 && up > -0.05) {
        let twinkle = 0.6 + 0.4 * hash3(cell + 7.0);
        col += vec3<f32>(0.9, 0.92, 1.0) * sky.extra.x * twinkle * clear * clamp(up * 6.0, 0.0, 1.0);
    }

    // Sun disc with a soft halo; hidden behind thick cloud.
    let sd = dot(dir, sun);
    let disc = smoothstep(0.99955, 0.99975, sd);
    let halo = pow(toward, 400.0) * 0.6 + pow(toward, 60.0) * 0.15;
    col += sky.sun_color.rgb * (disc * 6.0 + halo) * (0.15 + 0.85 * clear) * step(-0.02, sun.y + 0.02);

    // Moon: a pale disc with darker patches, and a faint halo.
    let moon = sky.moon_dir.xyz;
    let md = dot(dir, moon);
    let mdisc = smoothstep(0.99935, 0.9995, md);
    let patches = 0.8 + 0.2 * hash3(floor(dir * 2600.0));
    col = mix(col, vec3<f32>(0.85, 0.88, 0.95) * patches * sky.moon_dir.w * 1.6, mdisc * clear);
    col += vec3<f32>(0.5, 0.55, 0.7) * pow(max(md, 0.0), 300.0) * 0.25 * sky.moon_dir.w * clear;

    // Lightning lights up the whole sky.
    col += vec3<f32>(0.75, 0.8, 1.0) * sky.extra.z;

    var out: FragmentOutput;
    // Tonemapping etc. to match the rest of the scene (fog is off on this material).
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(col, 1.0));
    return out;
}
