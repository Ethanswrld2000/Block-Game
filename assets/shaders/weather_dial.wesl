// The weather dial at the top of the screen (`game::weather_dial`), drawn on a
// 64-pixel grid so it sits with the pixel-art UI.
//
// Middle: the sky disc. The point at the top is now; going clockwise is later
// today, so the disc turns as the day goes: sun at noon, moon (with its phase)
// at midnight, blue by day, orange at sunrise and sunset, starry at night.
// Ring: the weather for the next day, the same way round: each weather has a
// colour, a moving pattern and a picture. Gold pointer at the top = now.
//
// look:  x = time of day (0..1), y = clock (seconds), z = share of the day the sun is up, w = moon phase.
// state: x = storm warning pulse (0..1), y = lightning flash.
// seg[i]: x = where weather i ends (turns ahead), y = pattern, z = picture, w = where its picture sits (-1: none).
// col[i]: weather i's colour (linear).

import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct Dial {
    look: vec4<f32>,
    state: vec4<f32>,
    seg: array<vec4<f32>, 6>,
    col: array<vec4<f32>, 6>,
}

@group(1) @binding(0) var<uniform> dial: Dial;

const GRID: f32 = 64.0;
const TAU: f32 = 6.28318530718;
const RING_OUT: f32 = 0.95;
const RING_IN: f32 = 0.69;
const DISC: f32 = 0.64;

fn lin(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

// Turn `p` by `a` radians (clockwise on screen, y down).
fn turn(p: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);
}

// Where something `f` turns ahead (clockwise from the top) sits at radius `r`.
fn at_turn(f: f32, r: f32) -> vec2<f32> {
    return vec2<f32>(sin(f * TAU), -cos(f * TAU)) * r;
}

// 7x7 pictures, one bit per pixel, row by row from the top left.
fn icon_bits(icon: i32) -> vec2<u32> {
    switch icon {
        case 0: { return vec2<u32>(3416723720u, 8465u); }   // sun
        case 1: { return vec2<u32>(4025453568u, 3u); }      // cloud
        case 2: { return vec2<u32>(2686443276u, 170u); }    // rain
        case 3: { return vec2<u32>(3488027912u, 8529u); }   // snowflake
        case 4: { return vec2<u32>(2277706800u, 2145u); }   // lightning
        case 5: { return vec2<u32>(264249216u, 248u); }     // mist
        default: { return vec2<u32>(132132784u, 16760u); }  // wind
    }
}

fn icon_on(bits: vec2<u32>, c: vec2<i32>) -> bool {
    if (c.x < 0 || c.y < 0 || c.x > 6 || c.y > 6) {
        return false;
    }
    let i = u32(c.y * 7 + c.x);
    if (i < 32u) {
        return ((bits.x >> i) & 1u) == 1u;
    }
    return ((bits.y >> (i - 32u)) & 1u) == 1u;
}

// Which weather is `f` turns ahead.
fn seg_at(f: f32) -> i32 {
    for (var i = 0; i < 6; i++) {
        if (f < dial.seg[i].x) {
            return i;
        }
    }
    return 5;
}

// The sky at time of day `tod`, sRGB.
fn sky_at(tod: f32) -> vec3<f32> {
    let up = dial.look.z;
    let rise = 0.5 - up * 0.5;
    let sunset = 0.5 + up * 0.5;
    let day = smoothstep(rise - 0.03, rise + 0.03, tod) * (1.0 - smoothstep(sunset - 0.03, sunset + 0.03, tod));
    let night = vec3<f32>(0.10, 0.13, 0.32);
    let noon = vec3<f32>(0.45, 0.72, 0.94);
    var c = mix(night, noon, day);
    let dusk = max(1.0 - abs(tod - rise) / 0.05, 1.0 - abs(tod - sunset) / 0.05);
    c = mix(c, vec3<f32>(0.96, 0.52, 0.30), clamp(dusk, 0.0, 1.0) * 0.85);
    return c;
}

// The weather ring at cell `p` (-1..1, y down), `f` turns ahead, `band` 0..1 across the ring.
fn ring(p: vec2<f32>, f: f32, band: f32) -> vec3<f32> {
    let i = seg_at(f);
    let s = dial.seg[i];
    var c = pow(dial.col[i].rgb, vec3<f32>(1.0 / 2.2));
    let t = dial.look.y;
    // Pattern in the ring's own frame, so it turns with the dial.
    let q = turn(p, dial.look.x * TAU) * 32.0;
    let cell = floor(q);
    let pattern = i32(s.y);
    switch pattern {
        case 0: {
            // Clear: a warm glow, brighter outward, with slow glints.
            c = c * (0.9 + band * 0.2);
            if (hash(cell) > 0.96 && fract(t * 0.5 + hash(cell + 3.0)) < 0.3) { c = vec3<f32>(1.0, 0.97, 0.8); }
        }
        case 1: {
            // Clouds: soft lumps drifting.
            let n = noise(q * 0.22 + vec2<f32>(t * 0.3, 0.0));
            c = mix(c * 0.85, c * 1.25, step(0.5, n));
        }
        case 2: {
            // Rain: streaks falling.
            let d = (cell.x + cell.y + floor(t * 6.0)) % 4.0;
            if (abs(d) < 0.5 && hash(vec2<f32>(cell.x - cell.y, 1.0)) > 0.3) { c = vec3<f32>(0.70, 0.82, 0.98); }
        }
        case 3: {
            // Snow: flakes drifting down.
            let fall = cell + vec2<f32>(0.0, -floor(t * 2.0));
            if (hash(fall) > 0.86) { c = vec3<f32>(1.0); }
        }
        case 4: {
            // Storm: rain, and the ring flashes with the lightning.
            let d = (cell.x + cell.y + floor(t * 8.0)) % 3.0;
            if (abs(d) < 0.5 && hash(vec2<f32>(cell.x - cell.y, 2.0)) > 0.4) { c = vec3<f32>(0.55, 0.55, 0.75); }
            c = mix(c, vec3<f32>(1.0, 0.95, 0.7), dial.state.y * 0.6);
        }
        case 5: {
            // Mist: pale bands drifting round.
            let n = noise(vec2<f32>(q.x * 0.12 + t * 0.4, q.y * 0.5));
            c = mix(c * 0.9, vec3<f32>(0.93, 0.94, 0.95), smoothstep(0.45, 0.7, n));
        }
        case 6: {
            // Wind: dashes racing along the ring.
            let along = f * 90.0 + t * 3.0;
            let lane = floor(band * 4.0);
            if (fract(along * 0.25 + hash(vec2<f32>(lane, 5.0))) < 0.35 && fract(band * 4.0) > 0.4) { c = vec3<f32>(0.85, 0.95, 0.95); }
        }
        default: {
            // Frost: icy glints.
            if (hash(cell) > 0.9 && fract(t * 0.7 + hash(cell + 7.0)) < 0.4) { c = vec3<f32>(1.0); }
            c = c * (0.92 + band * 0.12);
        }
    }
    // A thin dark line where one weather gives way to the next.
    let start = select(0.0, dial.seg[max(i - 1, 0)].x, i > 0);
    if (i > 0 && (f - start) * TAU * 0.82 * 32.0 < 1.0) { c = vec3<f32>(0.16, 0.12, 0.10); }
    return c;
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let cell = floor(in.uv * GRID);
    let p = (cell + 0.5 - GRID * 0.5) / (GRID * 0.5);
    let r = length(p);
    let f = fract(atan2(p.x, -p.y) / TAU);
    let tod = dial.look.x;
    let t = dial.look.y;
    let outline = vec3<f32>(0.16, 0.12, 0.10);

    // The pointer at the top: a gold wedge over the ring (red while a storm is coming).
    let warn = dial.state.x;
    let wedge = (-0.66 - p.y) * 0.42;
    if (p.y < -0.62 && p.y > -1.0 && abs(p.x) < wedge + 0.06) {
        if (abs(p.x) < wedge - 0.01 && p.y > -0.97) {
            let gold = mix(vec3<f32>(1.0, 0.80, 0.32), vec3<f32>(1.0, 0.36, 0.28), warn);
            return vec4<f32>(lin(gold * select(1.0, 0.8, p.x > 0.0)), 1.0);
        }
        return vec4<f32>(lin(outline), 1.0);
    }

    if (r > 1.0) {
        return vec4<f32>(0.0);
    }
    // Rim, pulsing red during a storm warning.
    if (r > RING_OUT) {
        return vec4<f32>(lin(mix(outline, vec3<f32>(0.95, 0.25, 0.2), warn)), 1.0);
    }

    // The weather ring, with each weather's picture.
    if (r > RING_IN) {
        var c = ring(p, f, (r - RING_IN) / (RING_OUT - RING_IN));
        for (var i = 0; i < 6; i++) {
            let mid = dial.seg[i].w;
            if (mid < 0.0) { continue; }
            let centre = floor(at_turn(mid, (RING_IN + RING_OUT) * 0.5) * GRID * 0.5 + GRID * 0.5);
            let local = vec2<i32>(cell - centre) + vec2<i32>(3, 3);
            let bits = icon_bits(i32(dial.seg[i].z));
            if (icon_on(bits, local)) {
                c = select(vec3<f32>(0.98, 0.97, 0.92), vec3<f32>(1.0, 0.88, 0.3), i32(dial.seg[i].z) == 4 || i32(dial.seg[i].z) == 0);
            } else if (icon_on(bits, local + vec2<i32>(1, 0)) || icon_on(bits, local - vec2<i32>(1, 0)) || icon_on(bits, local + vec2<i32>(0, 1)) || icon_on(bits, local - vec2<i32>(0, 1))) {
                c = outline;
            }
        }
        return vec4<f32>(lin(c), 1.0);
    }
    if (r > DISC) {
        return vec4<f32>(lin(outline), 1.0);
    }

    // The sky disc: time of day at this angle, turning with the day.
    let here = fract(tod + f);
    var c = sky_at(here);
    // Stars where it's night, fixed to the disc so they turn with it.
    let q = floor(turn(p, tod * TAU) * 32.0);
    let dark = 1.0 - clamp(length(c - vec3<f32>(0.10, 0.13, 0.32)) * 4.0, 0.0, 1.0);
    if (hash(q) > 0.93 && dark > 0.5) {
        c = mix(c, vec3<f32>(0.95, 0.95, 1.0), 0.5 + 0.5 * sin(t * 2.0 + hash(q + 1.0) * TAU));
    }
    // Shading: lighter towards the middle, like a bowl of sky.
    c = c * mix(1.08, 0.82, r / DISC);

    // Sun at noon, with rays.
    let sun = p - at_turn(fract(0.5 - tod), 0.36);
    let sd = length(sun);
    if (sd < 0.12) {
        c = select(vec3<f32>(1.0, 0.86, 0.36), vec3<f32>(1.0, 0.66, 0.22), sd > 0.09);
    } else if (sd < 0.19) {
        let a = fract(atan2(sun.x, sun.y) / TAU * 8.0 + tod * 4.0);
        if (abs(a - 0.5) < 0.17) { c = vec3<f32>(1.0, 0.8, 0.3); }
    }
    // Moon at midnight, lit by its phase.
    let moon = (p - at_turn(fract(-tod), 0.36)) / 0.11;
    if (length(moon) < 1.0) {
        let phase = dial.look.w;
        let k = cos(phase * TAU);
        let mx = select(-moon.x, moon.x, phase < 0.5);
        let lit = mx > k * sqrt(max(1.0 - moon.y * moon.y, 0.0));
        c = select(vec3<f32>(0.22, 0.25, 0.40), vec3<f32>(0.93, 0.92, 0.84), lit);
        if (lit && hash(floor(moon * 3.0)) > 0.8) { c = vec3<f32>(0.80, 0.79, 0.74); }
    }
    // Brass hub.
    if (r < 0.11) {
        c = select(vec3<f32>(0.80, 0.62, 0.30), outline, r > 0.08);
        if (p.x < -0.02 && p.y < -0.02 && r < 0.07) { c = vec3<f32>(0.95, 0.80, 0.45); }
    }
    return vec4<f32>(lin(c), 1.0);
}
