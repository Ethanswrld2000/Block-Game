// Behind an urn's tree: you look down into the urn's mouth and the dark
// inside opens over the screen. A hole grows from the mouth (`centre.xy`, in
// 0..1 of the screen; `centre.z` its starting radius in pixels) with a rim
// glowing in the urn's colour; inside, slow mist turns round the middle,
// ripples spread as on dark water, and motes sink into the deep. The middle
// stays darkest: that's the bottom of the pot. Drawn on a coarse pixel grid
// (`look.z` pixels) so it sits with the pixel-art UI.
//
// look: x = seconds since it opened, y = how far open (0..1), z = pixel size.
// tint: the urn's colour (linear), a = how much the world outside is dimmed.

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct UrnVoid {
    look: vec4<f32>,
    tint: vec4<f32>,
    centre: vec4<f32>,
}

@group(1) @binding(0) var<uniform> urn: UrnVoid;

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(p0: vec2<f32>) -> f32 {
    var p = p0;
    var s = 0.0;
    var w = 0.5;
    for (var i = 0; i < 4; i++) {
        s += noise(p) * w;
        p = p * 2.03 + vec2<f32>(1.7, 9.2);
        w *= 0.5;
    }
    return s;
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let t = urn.look.x;
    let open = urn.look.y;
    let px = max(urn.look.z, 1.0);
    let size = in.size;
    // Snap to the coarse pixel grid.
    let p = (floor(in.uv * size / px) + 0.5) * px;
    let c = urn.centre.xy * size;
    let short = min(size.x, size.y);
    let rel = (p - c) / short;
    let d = length(p - c);

    // The hole: from the urn's mouth out past the corners.
    let ease = 1.0 - pow(1.0 - clamp(open, 0.0, 1.0), 3.0);
    let far = length(max(c, size - c)) * 1.08;
    let radius = mix(urn.centre.z, far, ease);
    let edge = d - radius;
    if (edge > px * 3.0) {
        // Outside: the world, dimmed as it opens.
        return vec4<f32>(0.0, 0.0, 0.0, urn.tint.a * ease);
    }

    let r = length(rel);
    let ang = atan2(rel.y, rel.x);
    // Mist turning round the middle, faster near it.
    let swirl = ang + 0.55 / (r + 0.18) + t * 0.12;
    let q = vec2<f32>(cos(swirl), sin(swirl)) * (r * 3.2 + 0.4);
    let mist = fbm(q * 1.6 + vec2<f32>(t * 0.05, -t * 0.03));
    let mist2 = fbm(q * 3.1 - vec2<f32>(t * 0.08, t * 0.02));
    let depth = smoothstep(0.04, 0.95, r);
    let glow = urn.tint.rgb;
    // Colours are linear: small numbers are already well lit on screen.
    var col = vec3<f32>(0.0004, 0.0003, 0.0018);
    col += glow * 0.10 * pow(mist, 3.0) * depth;
    col += vec3<f32>(0.012, 0.005, 0.030) * pow(mist2, 2.0) * depth;
    // Ripples on the dark, spreading outward.
    let ripple = pow(abs(sin(r * 26.0 - t * 1.1)), 24.0) * depth * (0.4 + mist);
    col += mix(glow, vec3<f32>(0.4, 0.3, 0.9), 0.4) * ripple * 0.025;
    // Motes sinking into the middle: rings that shrink over time, each with
    // a few single-pixel sparks round it.
    let w = 0.05;
    let fall = r / w + t * 0.3;
    let ring = floor(fall);
    let rc = (ring + 0.5 - t * 0.3) * w;
    let count = max(6.0, floor(6.2832 * rc / w));
    let turn = t * 0.03 + hash(vec2<f32>(ring, 3.1));
    let a = ang / 6.2832 + turn;
    let cell = floor(a * count);
    let h = hash(vec2<f32>(cell, ring));
    if (h > 0.8 && rc > 0.02) {
        let ca = ((cell + 0.25 + 0.5 * hash(vec2<f32>(ring, cell))) / count - turn) * 6.2832;
        let spot = c + vec2<f32>(cos(ca), sin(ca)) * rc * short;
        let near = length(p - spot) < px * 0.9;
        let twinkle = 0.5 + 0.5 * sin(t * (1.5 + h * 3.0) + h * 40.0);
        if (near) {
            col += mix(glow, vec3<f32>(1.0, 0.8, 0.45), 0.5) * twinkle * smoothstep(0.02, 0.3, rc) * 0.7;
        }
    }
    // Rim: a band of the urn's light where the dark meets the world.
    let rim = 1.0 - smoothstep(0.0, px * 2.5, abs(edge + px));
    let inner = exp(-max(-edge, 0.0) / (short * 0.05)) * (1.0 - ease * 0.8);
    col += (glow * 1.2 + vec3<f32>(0.2, 0.15, 0.05)) * rim * (1.0 - ease * 0.5);
    col += glow * 0.12 * inner;
    // A few colour steps, like a painted palette (stepped in screen brightness).
    col = pow(floor(sqrt(col) * 40.0) / 40.0, vec3<f32>(2.0));
    let alpha = 1.0;
    return vec4<f32>(col, alpha);
}
