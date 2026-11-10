// Sky dome: a three-stop gradient (horizon, mid sky, zenith), haze and glow
// around the sun, sunrise and sunset colour across the sky, sun and moon
// discs, and stars with a faint band across them at night. Drawn on a huge
// sphere around the camera with fog off. At the horizon it is exactly the
// colour the terrain fogs into (`horizon_color` in voxel.wesl and lod.wesl),
// so distant land melts into the sky behind it.

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
    // x: star brightness, y: share of the moon lit, z: lightning flash, w: cloudiness
    extra: vec4<f32>,
    // Rows turning a view direction into star-map space (the sky's daily turn).
    star_x: vec4<f32>,
    star_y: vec4<f32>,
    star_z: vec4<f32>,
    mid: vec4<f32>,
    // x: daytime haze towards the sun, y: star band brightness
    haze: vec4<f32>,
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

    // Gradient: horizon haze low down, the mid sky, the zenith overhead.
    let t = clamp(up, 0.0, 1.0);
    var col = mix(sky.horizon.rgb, sky.mid.rgb, smoothstep(0.0, 0.2, t));
    col = mix(col, sky.zenith.rgb, smoothstep(0.1, 0.85, t));
    // Below the horizon: a slightly darker version of the horizon.
    col = mix(col, sky.horizon.rgb * 0.85, clamp(-up * 4.0, 0.0, 1.0));

    let sun = sky.sun_dir.xyz;
    let toward = max(dot(dir, sun), 0.0);
    let flat_dir = normalize(vec3<f32>(dir.x, 0.0, dir.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let flat_sun = normalize(vec3<f32>(sun.x, 0.0, sun.z) + vec3<f32>(1e-5, 0.0, 0.0));
    let flat_toward = max(dot(flat_dir, flat_sun), 0.0);
    let low = 1.0 - clamp(abs(up) * 2.5, 0.0, 1.0);
    // Daytime haze: the sky whitens towards the sun.
    col *= 1.0 + sky.haze.x * pow(toward, 5.0);
    // Sunrise and sunset: glow around the sun, strongest near the horizon...
    let glow = sky.sun_dir.w * (pow(flat_toward, 3.0) * low * 0.8 + pow(toward, 12.0) * 0.5);
    col = mix(col, sky.sun_color.rgb, clamp(glow, 0.0, 0.9));
    // ...warm light spreading up the sun's half of the sky, and a pink band
    // low over the opposite horizon. Both are zero at the horizon itself, so
    // the fog still matches.
    let above = smoothstep(0.0, 0.12, up);
    let spread = sky.sun_dir.w * pow(flat_toward, 1.5) * above * (1.0 - smoothstep(0.2, 0.9, up)) * 0.3;
    col = mix(col, sky.sun_color.rgb * 0.8, clamp(spread, 0.0, 1.0));
    let away = max(-dot(flat_dir, flat_sun), 0.0);
    let belt = sky.sun_dir.w * away * away * above * (1.0 - smoothstep(0.08, 0.3, up)) * 0.35;
    col = mix(col, vec3<f32>(0.62, 0.36, 0.46) * max(dot(sky.horizon.rgb, vec3<f32>(0.33)), 0.05) * 2.2, clamp(belt, 0.0, 1.0));

    let clear = 1.0 - sky.extra.w;

    // Stars, turning across the sky around the same axis as the sun.
    let rd = vec3<f32>(dot(sky.star_x.xyz, dir), dot(sky.star_y.xyz, dir), dot(sky.star_z.xyz, dir));
    // A faint band of distant stars across the sky, in blocky patches.
    let band_n = normalize(vec3<f32>(0.35, 0.82, 0.45));
    let band = exp(-pow(dot(rd, band_n) / 0.16, 2.0));
    let mottle = 0.55 + 0.45 * hash3(floor(rd * 28.0));
    col += vec3<f32>(0.42, 0.46, 0.65) * 0.03 * band * mottle * sky.haze.y * clear * clamp(up * 6.0, 0.0, 1.0);
    let cell = floor(rd * 220.0);
    let h = hash3(cell);
    if (h > 0.9975 - 0.003 * band && up > -0.05) {
        let twinkle = 0.6 + 0.4 * hash3(cell + 7.0);
        col += vec3<f32>(0.9, 0.92, 1.0) * sky.extra.x * twinkle * clear * clamp(up * 6.0, 0.0, 1.0);
    }

    // Sun disc with a soft halo; hidden behind thick cloud.
    let sd = dot(dir, sun);
    let disc = smoothstep(0.99955, 0.99975, sd);
    let halo = pow(toward, 400.0) * 0.6 + pow(toward, 60.0) * 0.15;
    col += sky.sun_color.rgb * (disc * 6.0 + halo) * (0.15 + 0.85 * clear) * step(-0.02, sun.y + 0.02);

    // Moon: a ball lit by the sun, so it shows its phase. Darker patches,
    // a faint glow on the unlit side, and a halo that grows with the phase.
    let moon = sky.moon_dir.xyz;
    let md = dot(dir, moon);
    let mcos = 0.99945;
    if (md > mcos - 0.0002) {
        var refv = vec3<f32>(0.0, 1.0, 0.0);
        if (abs(moon.y) > 0.99) { refv = vec3<f32>(0.0, 0.0, 1.0); }
        let tx = normalize(cross(moon, refv));
        let ty = cross(tx, moon);
        let off = dir - moon * md;
        let uv = vec2<f32>(dot(off, tx), dot(off, ty)) / sqrt(1.0 - mcos * mcos);
        let r2 = dot(uv, uv);
        if (r2 < 1.0) {
            // Surface normal of the side facing us.
            let n = tx * uv.x + ty * uv.y - moon * sqrt(1.0 - r2);
            let lit = smoothstep(-0.04, 0.08, dot(n, sun));
            let patches = 0.78 + 0.22 * hash3(floor(n * 9.0 + 3.0));
            let face = vec3<f32>(0.86, 0.88, 0.95) * patches * (0.05 + 0.95 * lit) * 1.6;
            let edge = smoothstep(1.0, 0.9, r2);
            col = mix(col, face * sky.moon_dir.w, edge * clear * (0.5 + 0.5 * lit));
        }
    }
    col += vec3<f32>(0.5, 0.55, 0.7) * pow(max(md, 0.0), 300.0) * 0.25 * sky.moon_dir.w * sky.extra.y * clear;

    // Lightning lights up the whole sky.
    col += vec3<f32>(0.75, 0.8, 1.0) * sky.extra.z;

    var out: FragmentOutput;
    // Tonemapping etc. to match the rest of the scene (fog is off on this material).
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(col, 1.0));
    return out;
}
