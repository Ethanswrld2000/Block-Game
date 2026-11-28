//! The world floats. Everything you live on is one big island, about 2.5 miles
//! (4000 blocks) across, centred on spawn; past its rim there is only open sky
//! down to the void. Smaller islands drift off into the distance around it.
//!
//! The main island keeps the normal terrain (climate, mountains, swamp, caves);
//! this module only decides where land is (`edge`), lifts the rim so seas stay
//! inside, and shapes the underside: a rounded bowl with hanging lobes and
//! stalactite-like drips (`underside`). Outer islands reuse the same coastline
//! noise (scaled down), the same climate and biomes, and the same underside.

use crate::util::{hash2, smoothstep, unit};
use crate::{SEA_LEVEL, WorldGen};

/// Radius of the main island in blocks (before the coastline wobble).
pub const MAIN_RADIUS: f32 = 2000.0;
/// How far the coastline pushes in or out (share of the radius).
const COAST_WOBBLE: f32 = 0.14;
/// Thickness of the main island below sea level at its centre.
pub(crate) const CORE_DEPTH: f32 = 300.0;
/// Thickness at the rim.
const RIM_DEPTH: f32 = 18.0;
/// Outer islands sit on a jittered grid of this many blocks.
const OUTER_CELL: i32 = 520;
const OUTER_MAX_RADIUS: f32 = 260.0;
/// Clear sky between the main island's farthest coast and any outer island.
const OUTER_GAP: f32 = 120.0;

/// What a column belongs to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Land {
    /// Open sky: nothing in this column.
    #[default]
    Void,
    /// The island you live on.
    Main,
    /// One of the islands out in the distance.
    Outer,
}

/// An island out past the main one.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Outer {
    pub x: i32,
    pub z: i32,
    /// Base height of its top.
    pub y: i32,
    pub radius: f32,
    /// Offsets its coastline noise so no two look alike.
    salt: f32,
}

/// 0 at the main island's centre, 1 on its coast, above 1 out in the void.
pub(crate) fn main_edge(wg: &WorldGen, x: i32, z: i32) -> f32 {
    let (dx, dz) = ((x - wg.center.0) as f32, (z - wg.center.1) as f32);
    let n = wg.coast.get_noise_2d(x as f32, z as f32);
    (dx * dx + dz * dz).sqrt() / (MAIN_RADIUS * (1.0 + COAST_WOBBLE * n))
}

/// The bottom of an island column (the lowest solid block is one above it).
/// `top` is the height the island hangs from, `e` the edge share (0 centre,
/// 1 coast), `depth` its thickness at the centre and `scale` sizes the lobes
/// and drips (1 for the main island).
pub(crate) fn underside(
    wg: &WorldGen,
    x: i32,
    z: i32,
    top: f32,
    e: f32,
    depth: f32,
    scale: f32,
) -> i32 {
    // A rounded bowl: steep near the coast, flat in the middle.
    let bowl = (1.0 - e * e).max(0.0).sqrt();
    let rim = (RIM_DEPTH * scale).max(4.0);
    let lumps = wg.underside.get_noise_2d(x as f32, z as f32);
    let mut b = top - rim - (depth - rim).max(0.0) * bowl - lumps * 8.0 * scale * bowl;
    // Hanging shapes are shorter out at the coast.
    b -= hanging(wg, x, z, scale) * (0.35 + 0.65 * bowl);
    b.floor() as i32
}

/// How far rounded lobes and stalactite drips hang below the bowl here.
fn hanging(wg: &WorldGen, x: i32, z: i32, scale: f32) -> f32 {
    // Big rounded lobes, then thin drips (which can hang from the lobes).
    let lobe = bumps(
        wg.seed ^ 0x10BE,
        x,
        z,
        (80.0 * scale).max(24.0),
        0.75,
        |h| {
            let r = (24.0 + unit(h.rotate_left(8)) * 24.0) * scale;
            let len = (30.0 + unit(h.rotate_left(29)) * 50.0) * scale;
            (r.max(5.0), len)
        },
        |q| (1.0 - q * q).powf(0.6),
    );
    let drip = bumps(
        wg.seed ^ 0xD819,
        x,
        z,
        (20.0 * scale).max(8.0),
        0.85,
        |h| {
            let r = ((3.0 + unit(h.rotate_left(8)) * 6.0) * scale).max(2.0);
            (r, r * (3.0 + unit(h.rotate_left(29)) * 3.0))
        },
        |q| {
            // Fat and rounded at the tip, thinning towards the base.
            let t = 1.0 - q.powf(1.5);
            t * t
        },
    );
    lobe + drip
}

/// The largest bump over (x, z) from a jittered grid of `cell` blocks: each
/// cell has one (with `chance`) of `size(hash) = (radius, length)`, shaped by
/// `profile(distance / radius)` (1 at the middle, 0 at the radius).
fn bumps(
    seed: u64,
    x: i32,
    z: i32,
    cell: f32,
    chance: f32,
    size: impl Fn(u64) -> (f32, f32),
    profile: impl Fn(f32) -> f32,
) -> f32 {
    let c = cell as i32;
    let (cx, cz) = (x.div_euclid(c), z.div_euclid(c));
    let mut best = 0.0f32;
    for gz in cz - 1..=cz + 1 {
        for gx in cx - 1..=cx + 1 {
            let h = hash2(seed, gx, gz);
            if unit(h) >= chance {
                continue;
            }
            let px = gx as f32 * cell + unit(h.rotate_left(17)) * cell;
            let pz = gz as f32 * cell + unit(h.rotate_left(41)) * cell;
            let (r, len) = size(h);
            let d = ((x as f32 - px).powi(2) + (z as f32 - pz).powi(2)).sqrt();
            if d < r {
                best = best.max(len * profile(d / r));
            }
        }
    }
    best
}

/// The outer island in grid cell (cx, cz), if it has one.
fn outer_in_cell(wg: &WorldGen, cx: i32, cz: i32) -> Option<Outer> {
    let h = hash2(wg.seed ^ 0x00F7_EA15, cx, cz);
    if unit(h) > 0.6 {
        return None;
    }
    let j = |shift: u32| unit(h.rotate_left(shift));
    let radius = 50.0 + j(29) * j(53) * (OUTER_MAX_RADIUS - 50.0);
    let margin = (radius * 1.3) as i32;
    let span = (OUTER_CELL - 2 * margin).max(1);
    let x = cx * OUTER_CELL + margin + (j(11) * span as f32) as i32;
    let z = cz * OUTER_CELL + margin + (j(23) * span as f32) as i32;
    // Keep clear of the main island.
    let (dx, dz) = ((x - wg.center.0) as f32, (z - wg.center.1) as f32);
    if (dx * dx + dz * dz).sqrt() < MAIN_RADIUS * (1.0 + COAST_WOBBLE) + radius * 1.3 + OUTER_GAP {
        return None;
    }
    Some(Outer {
        x,
        z,
        y: -140 + (j(37) * 380.0) as i32,
        radius,
        salt: j(47) * 9000.0,
    })
}

/// The outer island over (x, z) and the edge share there (below 1 = on it).
pub(crate) fn outer_at(wg: &WorldGen, x: i32, z: i32) -> Option<(Outer, f32)> {
    let (cx, cz) = (x.div_euclid(OUTER_CELL), z.div_euclid(OUTER_CELL));
    for gz in cz - 1..=cz + 1 {
        for gx in cx - 1..=cx + 1 {
            let Some(o) = outer_in_cell(wg, gx, gz) else {
                continue;
            };
            // The main island's coastline, shrunk to this island's size.
            let k = MAIN_RADIUS / o.radius;
            let (dx, dz) = ((x - o.x) as f32, (z - o.z) as f32);
            let n = wg.coast.get_noise_2d(dx * k + o.salt, dz * k - o.salt);
            let e = (dx * dx + dz * dz).sqrt() / (o.radius * (1.0 + 0.25 * n));
            if e < 1.0 {
                return Some((o, e));
            }
        }
    }
    None
}

/// The main island's height at its rim: lifted so seas never spill off the edge.
pub(crate) fn lift_rim(height: f32, e: f32, detail: f32) -> f32 {
    let rim = smoothstep(0.84, 0.97, e);
    let shore = SEA_LEVEL as f32 + 2.0 + detail.abs() * 3.0;
    if rim > 0.0 && height < shore {
        height + (shore - height) * rim
    } else {
        height
    }
}
