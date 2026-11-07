//! Sky islands: flat-topped, rocky-bottomed islands floating high above the
//! world. Placed on a jittered grid so they spread out evenly.

use voxel::{BlockId, ChunkPos, CHUNK_SIZE_I32};

use crate::trees::{draw, Canvas};
use crate::util::{hash2, unit};
use crate::{TreeKind, WorldGen, SKY_MIN};

const CELL: i32 = 192;
const MAX_RADIUS: i32 = 58;

#[derive(Clone, Copy, Debug)]
struct Island {
    x: i32,
    y: i32,
    z: i32,
    radius: f32,
}

fn island_in_cell(wg: &WorldGen, cx: i32, cz: i32) -> Option<Island> {
    let h = hash2(wg.seed ^ 0x15_1A_4D, cx, cz);
    if unit(h) > 0.55 {
        return None;
    }
    let j = |shift: u32| unit(h.rotate_left(shift));
    Some(Island {
        x: cx * CELL + 32 + (j(11) * (CELL - 64) as f32) as i32,
        z: cz * CELL + 32 + (j(23) * (CELL - 64) as f32) as i32,
        y: SKY_MIN + 40 + (j(37) * 180.0) as i32,
        radius: 16.0 + j(49) * (MAX_RADIUS - 16) as f32,
    })
}

fn islands_near(wg: &WorldGen, x0: i32, z0: i32, x1: i32, z1: i32) -> Vec<Island> {
    let mut out = Vec::new();
    for cz in (z0 - MAX_RADIUS).div_euclid(CELL)..=(z1 + MAX_RADIUS).div_euclid(CELL) {
        for cx in (x0 - MAX_RADIUS).div_euclid(CELL)..=(x1 + MAX_RADIUS).div_euclid(CELL) {
            if let Some(i) = island_in_cell(wg, cx, cz) {
                out.push(i);
            }
        }
    }
    out
}

/// (bottom, top) of an island's column at (x, z), if the island covers it.
fn span(wg: &WorldGen, isl: &Island, x: i32, z: i32) -> Option<(i32, i32)> {
    let (dx, dz) = ((x - isl.x) as f32, (z - isl.z) as f32);
    let n = wg.island_shape.get_noise_2d(x as f32, z as f32);
    let dist = (dx * dx + dz * dz).sqrt() + n * isl.radius * 0.22;
    if dist >= isl.radius {
        return None;
    }
    let t = dist / isl.radius;
    let top = isl.y as f32 + isl.radius * 0.16 * (1.0 - t * t) + n * 2.0;
    let bottom = isl.y as f32 - isl.radius * 0.95 * (1.0 - t).powf(1.3) - n.abs() * 6.0;
    Some((bottom.floor() as i32, top.floor() as i32))
}

/// (bottom, top) of whichever island covers this column.
pub(crate) fn span_at(wg: &WorldGen, x: i32, z: i32) -> Option<(i32, i32)> {
    islands_near(wg, x, z, x, z).iter().filter_map(|i| span(wg, i, x, z)).max_by_key(|s| s.1)
}

/// Top surface of whichever island covers this column.
pub(crate) fn top_at(wg: &WorldGen, x: i32, z: i32) -> Option<i32> {
    islands_near(wg, x, z, x, z).iter().filter_map(|i| span(wg, i, x, z)).map(|(_, t)| t).max()
}

/// Draw the islands that touch this chunk. Returns true if any block was set.
pub(crate) fn place(wg: &WorldGen, pos: ChunkPos, dense: &mut [BlockId]) -> bool {
    let o = pos.origin();
    let (x1, z1) = (o.x + CHUNK_SIZE_I32 - 1, o.z + CHUNK_SIZE_I32 - 1);
    let islands = islands_near(wg, o.x, o.z, x1, z1);
    if islands.is_empty() {
        return false;
    }
    let ids = wg.ids;
    let sky = wg.biomes.get(wg.biomes.sky);
    let mut c = Canvas { origin: [o.x, o.y, o.z], dense };
    let mut any = false;
    let reach = 3;
    for isl in &islands {
        let r = isl.radius as i32 + 8;
        if o.y > isl.y + 30 || o.y + CHUNK_SIZE_I32 < isl.y - r - 10 {
            continue;
        }
        for z in (o.z - reach).max(isl.z - r)..=(z1 + reach).min(isl.z + r) {
            for x in (o.x - reach).max(isl.x - r)..=(x1 + reach).min(isl.x + r) {
                let Some((bottom, top)) = span(wg, isl, x, z) else { continue };
                let inside = x >= o.x && x <= x1 && z >= o.z && z <= z1;
                if inside {
                    for y in bottom.max(o.y)..=top.min(o.y + CHUNK_SIZE_I32 - 1) {
                        let b = if y == top { sky.surface } else if y >= top - 2 { ids.dirt } else { ids.skystone };
                        c.put(x, y, z, b);
                        any = true;
                    }
                }
                // Trees on the island top (their canopy may spill into this chunk).
                let h = hash2(wg.seed ^ 0x15_7EE5, x, z);
                if unit(h) < sky.tree_chance && top - bottom > 4 {
                    draw(wg, &mut c, TreeKind::Oak, x, top, z, h);
                    any = true;
                }
            }
        }
    }
    let _ = ids.cloud_moss;
    any
}
