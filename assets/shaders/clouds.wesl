// Raymarched clouds in a slab (see game::clouds). Each view ray is walked
// through the slab; density comes from 3D value noise shaped so cloud bases
// are flat and tops are rounded; light is a short look towards the sun.

import bevy_pbr::render::{
    mesh_view_bindings::view,
    forward_io::{VertexOutput, FragmentOutput},
};

struct CloudParams {
    slab: vec4<f32>,
    wind: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> clouds: CloudParams;

fn hash(p: vec3<f32>) -> f32 {
    let q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    let r = q + dot(q, q.yzx + 33.33);
    return fract((r.x + r.y) * r.z);
}

fn value_noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(hash(i), hash(i + vec3<f32>(1.0, 0.0, 0.0)), u.x), mix(hash(i + vec3<f32>(0.0, 1.0, 0.0)), hash(i + vec3<f32>(1.0, 1.0, 0.0)), u.x), u.y);
    let b = mix(mix(hash(i + vec3<f32>(0.0, 0.0, 1.0)), hash(i + vec3<f32>(1.0, 0.0, 1.0)), u.x), mix(hash(i + vec3<f32>(0.0, 1.0, 1.0)), hash(i + vec3<f32>(1.0, 1.0, 1.0)), u.x), u.y);
    return mix(a, b, u.z);
}

fn fbm(p: vec3<f32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var amp = 0.55;
    var q = p;
    for (var i = 0; i < octaves; i++) {
        sum += value_noise(q) * amp;
        q = q * 2.03 + vec3<f32>(17.1, 3.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn density(p: vec3<f32>, octaves: i32) -> f32 {
    let base = clouds.slab.x;
    let top = clouds.slab.y;
    let h = clamp((p.y - base) / (top - base), 0.0, 1.0);
    // Flat bottoms, rounded tops.
    let shape = clamp(h * 6.0, 0.0, 1.0) * clamp((1.0 - h) * 2.2, 0.0, 1.0);
    let q = vec3<f32>(p.x - clouds.wind.x, p.y * 1.8, p.z - clouds.wind.y) * 0.0045;
    let n = fbm(q, octaves);
    let threshold = 0.84 - clouds.slab.z * 0.5;
    return max(n * shape - threshold * mix(1.0, 0.6, shape), 0.0) * 9.0 * clouds.slab.w;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let ro = view.world_position.xyz;
    let rd = normalize(in.world_position.xyz - ro);
    let base = clouds.slab.x;
    let top = clouds.slab.y;
    let is_top = clouds.sun_dir.w > 0.5;
    let below = ro.y < base;
    let above = ro.y > top;
    let inside = !below && !above;
    // Only the plane the ray enters the slab through draws it.
    let mine = select((below || (inside && rd.y < 0.0)), (above || (inside && rd.y > 0.0)), is_top);
    if (!mine || abs(rd.y) < 1e-4) {
        discard;
    }
    let t_base = (base - ro.y) / rd.y;
    let t_top = (top - ro.y) / rd.y;
    var t0 = max(min(t_base, t_top), 0.0);
    let reach = clouds.wind.w;
    let t1 = min(max(t_base, t_top), reach);
    if (t1 <= t0) {
        discard;
    }
    // Fast: fewer steps and less detail; fancy: more of both.
    let fancy = clouds.sun_color.w > 0.5;
    let steps = select(12, 24, fancy);
    let dt = (t1 - t0) / f32(steps);
    t0 += dt * hash(vec3<f32>(in.position.xy, clouds.wind.z * 7.0));
    let sun = clouds.sun_dir.xyz;
    let near = length(ro.xz - (ro + rd * t0).xz) < 900.0;
    let octaves = select(select(2, 3, near), select(3, 4, near), fancy);
    var trans = 1.0;
    var light = vec3<f32>(0.0);
    var first = -1.0;
    for (var i = 0; i < steps; i++) {
        let p = ro + rd * (t0 + dt * f32(i));
        let d = density(p, octaves);
        if (d > 0.002) {
            if (first < 0.0) {
                first = t0 + dt * f32(i);
            }
            // How much cloud lies between here and the sun.
            let toward = density(p + sun * 24.0, 2);
            let lit = exp(-toward * 22.0);
            let h = clamp((p.y - base) / (top - base), 0.0, 1.0);
            let col = clouds.ambient.rgb * (0.55 + 0.45 * h) + clouds.sun_color.rgb * lit * 0.9;
            let a = 1.0 - exp(-d * dt);
            light += trans * a * col;
            trans *= 1.0 - a;
            if (trans < 0.03) {
                break;
            }
        }
    }
    var alpha = 1.0 - trans;
    if (alpha < 0.004) {
        discard;
    }
    // Fade out towards the far edge so the end of the planes never shows.
    let dist = select(t0, first, first > 0.0);
    alpha *= 1.0 - smoothstep(reach * 0.55, reach * 0.95, dist);
    var out: FragmentOutput;
    out.color = vec4<f32>(light / max(1.0 - trans, 1e-3), alpha);
    return out;
}
