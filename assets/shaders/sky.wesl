// Sky dome: a three-stop gradient (horizon, mid sky, zenith), haze and glow
// around the sun, sunrise and sunset colour across the sky, sun and moon
// discs, and stars at night (with a faint band across them when `sky::MILKY_WAY`
// is on). Drawn on a huge
// sphere around the camera with fog off. At the horizon it is exactly the
// colour the terrain fogs into (`horizon_color` in voxel.wesl and lod.wesl),
// so distant land melts into the sky behind it.

import bevy_pbr::render::{
    mesh_view_bindings::{view, globals},
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
    // x: daytime haze towards the sun, y: star band brightness,
    // z: fog veil (thick fog or mist hides the sky, sun, moon and stars),
    // w: telescope raised (0..1), which brings out fainter stars
    haze: vec4<f32>,
    // Rainbow: xyz the point it circles (opposite the sun), w strength (0 = none)
    bow: vec4<f32>,
    // Constellation stars in star-map space: xyz direction, w brightness (0 = none).
    named: array<vec4<f32>, 64>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: SkyParams;

fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    let r = q + dot(q, q.yxz + 33.33);
    return fract((r.x + r.y) * r.z);
}

// Stars are scattered over a grid of cells in star-map space; a cell holds a
// star when its hash is under STAR_DENSITY and its jittered point lies within
// half a cell of the sphere STAR_SCALE across, so every star belongs to one
// cell and the 3x3x3 cells around a direction find every star near it.
const STAR_SCALE: f32 = 60.0;
const STAR_DENSITY: f32 = 0.05;

// One star: a soft round point about one to two pixels across (`px` is the
// angle one pixel covers, so it stays a point when the telescope zooms in),
// `d` is the angle to its centre.
fn star_light(d: f32, bright: f32, px: f32) -> f32 {
    // A little bigger on the sky for bright stars, which shows when magnified.
    let sigma = max(px * (0.7 + 0.4 * sqrt(bright)), 0.00045 * sqrt(bright));
    let core = exp(-(d * d) / (sigma * sigma));
    // A faint glow round the brightest.
    let g = d / (sigma * 3.5);
    let glow = exp(-g * g) * 0.06 * max(bright - 1.0, 0.0);
    return bright * core + glow;
}

fn star_field(rd: vec3<f32>, px: f32, t: f32, low: f32) -> vec3<f32> {
    let p = rd * STAR_SCALE;
    let base = floor(p);
    var sum = vec3<f32>(0.0);
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            for (var z = -1; z <= 1; z++) {
                let cell = base + vec3<f32>(f32(x), f32(y), f32(z));
                if (hash3(cell) > STAR_DENSITY) { continue; }
                let sp = cell + vec3<f32>(hash3(cell + 11.3), hash3(cell + 23.7), hash3(cell + 41.1));
                let r = length(sp);
                if (abs(r - STAR_SCALE) > 0.5) { continue; }
                let d = length(rd - sp / r);
                if (d > 0.02) { continue; }
                // Most stars faint, a few bright.
                let m = hash3(cell + 5.9);
                let bright = 0.12 + 2.2 * pow(m, 6.0);
                // Colour: warm orange through white to pale blue.
                let c = hash3(cell + 3.3);
                var tint = mix(vec3<f32>(1.0, 0.78, 0.58), vec3<f32>(1.0, 0.97, 0.94), smoothstep(0.0, 0.35, c));
                tint = mix(tint, vec3<f32>(0.72, 0.82, 1.0), smoothstep(0.65, 1.0, c));
                // Twinkle: stronger low in the sky, where the air is thicker.
                let w = hash3(cell + 9.1);
                let tw = sin(t * (1.7 + 3.1 * w) + w * 40.0) * sin(t * (2.9 + 2.3 * m) + c * 31.0);
                let twinkle = 1.0 + (0.15 + 0.35 * low) * tw;
                sum += tint * star_light(d, bright, px) * twinkle;
            }
        }
    }
    return sum;
}

// Rainbow colours across a band: x 0 = violet (inner edge), 1 = red (outer).
fn spectrum(x: f32) -> vec3<f32> {
    let h = (1.0 - clamp(x, 0.0, 1.0)) * 0.78 * 6.0;
    return clamp(vec3<f32>(abs(h - 3.0) - 1.0, 2.0 - abs(h - 2.0), 2.0 - abs(h - 4.0)), vec3<f32>(0.0), vec3<f32>(1.0));
}

// How much of a band shows at x (0..1 across it), soft at both edges.
fn band(x: f32) -> f32 {
    if (x <= 0.0 || x >= 1.0) { return 0.0; }
    return sin(x * 3.14159265);
}

// A bow 40-42.5° round the point opposite the sun (violet inside, red outside),
// a faint second bow at 50-54° with its colours the other way round, a little
// light inside the main bow and a darker band between the two. Added to `col`,
// scaled by how bright the sky is there; stops at the horizon.
fn rainbow(col: vec3<f32>, dir: vec3<f32>, up: f32) -> vec3<f32> {
    let deg = degrees(acos(clamp(dot(dir, sky.bow.xyz), -1.0, 1.0)));
    let k = sky.bow.w * smoothstep(-0.01, 0.03, up);
    let first = (deg - 40.2) / 2.4;
    let second = (54.0 - deg) / 3.8;
    let bows = spectrum(first) * band(first) + spectrum(second) * band(second) * 0.35;
    let inside = (1.0 - smoothstep(37.0, 40.6, deg)) * 0.06;
    let between = smoothstep(42.0, 43.0, deg) * (1.0 - smoothstep(49.5, 50.5, deg));
    let light = max(dot(col, vec3<f32>(0.3, 0.55, 0.15)), 0.02);
    return col * (1.0 - 0.1 * between * k) + (bows * 0.6 + vec3<f32>(inside)) * light * k;
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
    // A faint band of distant stars across the sky, in blocky patches. Its
    // strength is haze.y, which is 0 while `sky::MILKY_WAY` is off.
    let band_n = normalize(vec3<f32>(0.35, 0.82, 0.45));
    let band = exp(-pow(dot(rd, band_n) / 0.16, 2.0)) * sky.haze.y;
    let mottle = 0.55 + 0.45 * hash3(floor(rd * 28.0));
    col += vec3<f32>(0.42, 0.46, 0.65) * 0.03 * band * mottle * clear * clamp(up * 6.0, 0.0, 1.0);
    // Angle one pixel covers here (shrinks when the telescope zooms in).
    let px = max(length(dpdx(dir)), length(dpdy(dir)));
    if (up > -0.05 && sky.extra.x > 0.0) {
        let starlight = sky.extra.x * (1.0 + 1.2 * sky.haze.w) * clear * clamp(up * 6.0, 0.0, 1.0);
        let low = 1.0 - smoothstep(0.0, 0.5, up);
        var stars = star_field(rd, px, globals.time, low);
        // The constellations: the same stars the telescope charts.
        for (var i = 0; i < 64; i++) {
            let s = sky.named[i];
            if (s.w <= 0.0) { continue; }
            let d = length(rd - s.xyz);
            if (d < 0.02) {
                let tw = 1.0 + (0.1 + 0.3 * low) * sin(globals.time * (2.0 + f32(i % 5) * 0.6) + f32(i) * 1.7);
                stars += vec3<f32>(1.0, 0.96, 0.9) * star_light(d, s.w, px) * tw;
            }
        }
        col += stars * starlight;
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

    if (sky.bow.w > 0.0) {
        col = rainbow(col, dir, up);
    }

    // Thick fog: the whole sky turns to the colour the land fogs into in this
    // direction (the horizon, with the same haze and glow), so nothing shows
    // through above the fog either.
    let flat_to_sun = max(dot(flat_dir, sun), 0.0);
    var fogged = sky.horizon.rgb * (1.0 + sky.haze.x * pow(flat_to_sun, 5.0));
    let fog_glow = sky.sun_dir.w * (pow(flat_toward, 3.0) * 0.8 + pow(flat_to_sun, 12.0) * 0.5);
    fogged = mix(fogged, sky.sun_color.rgb, clamp(fog_glow, 0.0, 0.9));
    col = mix(col, fogged, sky.haze.z);

    // Lightning lights up the whole sky.
    col += vec3<f32>(0.75, 0.8, 1.0) * sky.extra.z;

    var out: FragmentOutput;
    // Tonemapping etc. to match the rest of the scene (fog is off on this material).
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(col, 1.0));
    return out;
}
