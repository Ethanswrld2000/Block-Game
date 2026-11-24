// The dark in an urn's mouth, out in the world: the same mist, ripples and
// motes as the urn's tree opens with (`urn_void.wesl`), worked out from the
// pixel on screen in the same way, so when the tree opens from the mouth the
// dark carries straight on into it. `void_inside`, `hash`, `noise`, `fbm` and
// `palette` are copies from urn_void.wesl and must stay the same.
//
// look: x = the clock (seconds), y = screen's short side (pixels), z = pixel size.
// tint: the urn's colour (linear).
// centre: xy = the mouth's middle on screen (pixels), z = its radius (pixels).

import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};

struct UrnMouth {
    look: vec4<f32>,
    tint: vec4<f32>,
    centre: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> mouth: UrnMouth;

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

// The dark inside the urn at pixel `p` (physical pixels), its middle at `c`,
// `short` the screen's short side, `px` the pixel grid, `t` the clock (seconds),
// `glow` the urn's colour, `radius` how far it has opened (pixels) and `ease`
// how open it is (0..1). Copied in `urn_mouth.wesl`, which draws the same dark
// on the urn's mouth in the world, so the two must stay the same.
fn void_inside(p: vec2<f32>, c: vec2<f32>, short: f32, px: f32, t: f32, glow: vec3<f32>, radius: f32, ease: f32) -> vec3<f32> {
    let rel = (p - c) / short;
    let d = length(p - c);
    let edge = d - radius;
    let r = length(rel);
    let ang = atan2(rel.y, rel.x);
    // Mist turning round the middle, faster near it.
    let swirl = ang + 0.55 / (r + 0.18) + t * 0.12;
    let q = vec2<f32>(cos(swirl), sin(swirl)) * (r * 3.2 + 0.4);
    let mist = fbm(q * 1.6 + vec2<f32>(t * 0.05, -t * 0.03));
    let mist2 = fbm(q * 3.1 - vec2<f32>(t * 0.08, t * 0.02));
    let depth = smoothstep(0.04, 0.95, r);
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
    // The urn's light, faint, just inside the edge.
    let inner = exp(-max(-edge, 0.0) / (short * 0.05)) * (1.0 - ease * 0.8);
    col += glow * 0.12 * inner;
    return col;
}

// A few colour steps, like a painted palette (stepped in screen brightness).
fn palette(col: vec3<f32>) -> vec3<f32> {
    return pow(floor(sqrt(col) * 40.0) / 40.0, vec3<f32>(2.0));
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    let px = max(mouth.look.z, 1.0);
    let p = (floor(in.position.xy / px) + 0.5) * px;
    let col = void_inside(p, mouth.centre.xy, max(mouth.look.y, 1.0), px, mouth.look.x, mouth.tint.rgb, mouth.centre.z, 0.0);
    var out: FragmentOutput;
    out.color = vec4<f32>(palette(col), 1.0);
    return out;
}
