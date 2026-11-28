//! Sky islands: flat-topped, rocky-bottomed islands floating high above the
//! world. Placed on a jittered grid so they spread out evenly.

use voxel::{BlockId, ChunkPos, CHUNK_SIZE_I32};

use crate::trees::{draw, Canvas};
use crate::util::{hash2, unit};
use crate::{WorldGen, SKY_MIN};

const CELL: i32 = 192;
pub(crate) const MAX_RADIUS: i32 = 58;
/// Islands at least this wide keep a chest at their middle (`sky_island` in loot.ron).
pub const CHEST_RADIUS: f32 = 24.0;
/// Islands at least this wide also keep a sky shrine, `SHRINE_OFFSET` blocks east of the chest
/// (right click it once for a skill point: `game::shrine`).
pub const SHRINE_RADIUS: f32 = 40.0;
pub const SHRINE_OFFSET: i32 = 5;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Island {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
    pub(crate) radius: f32,
    /// The aunt's island (`landmarks::AuntIsland`), not one of the grid's.
    pub(crate) aunt: bool,
}

pub(crate) fn island_in_cell(wg: &WorldGen, cx: i32, cz: i32) -> Option<Island> {
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
        aunt: false,
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
    if let Some(a) = wg.aunt_island() {
        let i = a.island();
        let r = i.radius as i32 + 1;
        if i.x + r >= x0 && i.x - r <= x1 && i.z + r >= z0 && i.z - r <= z1 {
            out.push(i);
        }
    }
    out
}

/// (bottom, top) of an island's column at (x, z), if the island covers it.
pub(crate) fn span(wg: &WorldGen, isl: &Island, x: i32, z: i32) -> Option<(i32, i32)> {
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

/// The island that covers this column (the highest, if two do).
pub(crate) fn island_at(wg: &WorldGen, x: i32, z: i32) -> Option<Island> {
    islands_near(wg, x, z, x, z).into_iter().filter_map(|i| span(wg, &i, x, z).map(|s| (i, s.1))).max_by_key(|s| s.1).map(|s| s.0)
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
    let mut c = Canvas { origin: [o.x, o.y, o.z], dense };
    let mut any = false;
    let reach = 3;
    for isl in &islands {
        let r = isl.radius as i32 + 8;
        if o.y > isl.y + 30 || o.y + CHUNK_SIZE_I32 < isl.y - r - 10 {
            continue;
        }
        // One biome per island (`sky` biomes in biomes.ron).
        let sky = wg.biome_of_island(isl);
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
                let clear = isl.aunt && wg.aunt_island().is_some_and(|a| a.keeps_clear(x, z));
                if unit(h) < sky.tree_chance && top - bottom > 4 && !clear {
                    draw(wg, &mut c, sky.trees, x, top, z, h);
                    any = true;
                }
            }
        }
    }
    // A chest at the middle of the bigger islands, over any tree there.
    for isl in islands.iter().filter(|i| !i.aunt && i.radius >= CHEST_RADIUS) {
        if let Some((_, top)) = span(wg, isl, isl.x, isl.z) {
            for y in top + 1..=top + 3 {
                c.put(isl.x, y, isl.z, if y == top + 1 { ids.chest } else { BlockId::AIR });
            }
            any = true;
        }
    }
    // A sky shrine on the biggest islands, east of the chest.
    for isl in islands.iter().filter(|i| !i.aunt && i.radius >= SHRINE_RADIUS) {
        let sx = isl.x + SHRINE_OFFSET;
        if let Some((_, top)) = span(wg, isl, sx, isl.z) {
            for y in top + 1..=top + 3 {
                c.put(sx, y, isl.z, if y == top + 1 { ids.sky_shrine } else { BlockId::AIR });
            }
            any = true;
        }
    }
    // The aunt's cottage and the broken arch.
    if let Some(a) = wg.aunt_island()
        && islands.iter().any(|i| i.aunt)
        && a.bounds.min[0] <= x1
        && a.bounds.max[0] >= o.x
        && a.bounds.min[2] <= z1
        && a.bounds.max[2] >= o.z
        && a.bounds.min[1] < o.y + CHUNK_SIZE_I32
        && a.bounds.max[1] >= o.y
    {
        crate::landmarks::draw_aunt_island(wg, &mut c, a);
        any = true;
    }
    let _ = ids.cloud_moss;
    any
}

/// Where the chest on a grid island stands, if this island has one near (x, z).
pub(crate) fn chest_near(wg: &WorldGen, x: i32, z: i32) -> Option<[i32; 3]> {
    islands_near(wg, x, z, x, z).into_iter().filter(|i| !i.aunt && i.radius >= CHEST_RADIUS).find_map(|i| {
        let (_, top) = span(wg, &i, i.x, i.z)?;
        ((i.x - x).abs() <= 1 && (i.z - z).abs() <= 1).then_some([i.x, top + 1, i.z])
    })
}

/// Where the shrine on a grid island stands, if this island has one near (x, z).
#[cfg(test)]
pub(crate) fn shrine_near(wg: &WorldGen, x: i32, z: i32) -> Option<[i32; 3]> {
    islands_near(wg, x, z, x, z).into_iter().filter(|i| !i.aunt && i.radius >= SHRINE_RADIUS).find_map(|i| {
        let sx = i.x + SHRINE_OFFSET;
        let (_, top) = span(wg, &i, sx, i.z)?;
        ((sx - x).abs() <= 1 && (i.z - z).abs() <= 1).then_some([sx, top + 1, i.z])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxel::BlockPos;

    #[test]
    fn the_biggest_islands_keep_a_shrine() {
        let wg = crate::tests::wg();
        let isl = (-20..20)
            .flat_map(|cz| (-20..20).map(move |cx| (cx, cz)))
            .filter_map(|(cx, cz)| island_in_cell(&wg, cx, cz))
            .find(|i| i.radius >= SHRINE_RADIUS + 2.0)
            .expect("an island big enough");
        let [x, y, z] = shrine_near(&wg, isl.x + SHRINE_OFFSET, isl.z).unwrap();
        let p = BlockPos::new(x, y, z);
        let chunk = wg.generate(p.chunk());
        let o = p.chunk().origin();
        assert_eq!(chunk.get((x - o.x) as usize, (y - o.y) as usize, (z - o.z) as usize), wg.ids.sky_shrine);
    }

    #[test]
    fn bigger_islands_keep_a_chest_in_the_middle() {
        let wg = crate::tests::wg();
        let isl = (-20..20)
            .flat_map(|cz| (-20..20).map(move |cx| (cx, cz)))
            .filter_map(|(cx, cz)| island_in_cell(&wg, cx, cz))
            .find(|i| i.radius >= CHEST_RADIUS + 4.0)
            .expect("an island big enough");
        let [x, y, z] = chest_near(&wg, isl.x, isl.z).unwrap();
        assert!(wg.island_chest(x, y, z));
        let p = BlockPos::new(x, y, z);
        let chunk = wg.generate(p.chunk());
        let o = p.chunk().origin();
        assert_eq!(chunk.get((x - o.x) as usize, (y - o.y) as usize, (z - o.z) as usize), wg.ids.chest);
    }
}
