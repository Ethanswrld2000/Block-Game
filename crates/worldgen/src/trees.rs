//! Trees. A tree whose canopy crosses a chunk border is drawn partly by each
//! chunk from the same seed, so the halves line up.

use voxel::chunk::index;
use voxel::{BlockId, ChunkPos, CHUNK_SIZE_I32};

use crate::util::{hash2, unit};
use crate::{TreeKind, WorldGen, SEA_LEVEL};

/// How far a canopy can reach sideways from its trunk.
const REACH: i32 = 3;
/// Tallest a tree reaches above its ground block (pine: trunk up to 16, tip 2 more).
const TALLEST: i32 = 20;
/// Highest chance any biome uses, for a cheap first filter.
const MAX_CHANCE: f32 = 0.05;

/// Writes blocks into a chunk's dense array, clipped to the chunk.
pub(crate) struct Canvas<'a> {
    pub origin: [i32; 3],
    pub dense: &'a mut [BlockId],
}

impl Canvas<'_> {
    fn idx(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (lx, ly, lz) = (x - self.origin[0], y - self.origin[1], z - self.origin[2]);
        let r = 0..CHUNK_SIZE_I32;
        (r.contains(&lx) && r.contains(&ly) && r.contains(&lz)).then(|| index(lx as usize, ly as usize, lz as usize))
    }

    /// Set a block, always.
    pub fn put(&mut self, x: i32, y: i32, z: i32, b: BlockId) {
        if let Some(i) = self.idx(x, y, z) {
            self.dense[i] = b;
        }
    }

    /// Set a block only where there is air (leaves never replace terrain).
    pub fn fill(&mut self, x: i32, y: i32, z: i32, b: BlockId) {
        if let Some(i) = self.idx(x, y, z)
            && self.dense[i].is_air()
        {
            self.dense[i] = b;
        }
    }
}

pub(crate) fn place(wg: &WorldGen, pos: ChunkPos, dense: &mut [BlockId]) {
    let o = pos.origin();
    let mut canvas = Canvas { origin: [o.x, o.y, o.z], dense };
    // No trees through the middle of a village.
    let villages = wg.villages_in(o.x - REACH, o.z - REACH, o.x + CHUNK_SIZE_I32 + REACH, o.z + CHUNK_SIZE_I32 + REACH);
    for tz in o.z - REACH..o.z + CHUNK_SIZE_I32 + REACH {
        for tx in o.x - REACH..o.x + CHUNK_SIZE_I32 + REACH {
            let h = hash2(wg.seed ^ 0x7EE5, tx, tz);
            let roll = unit(h);
            if roll >= MAX_CHANCE {
                continue;
            }
            let col = wg.column(tx, tz);
            let biome = wg.biomes.get(col.biome);
            if roll >= biome.tree_chance || col.climate.overhang > 0.02 || crate::structures::in_village(&villages, tx, tz) {
                continue;
            }
            let ground = col.height;
            let min_ground = if biome.trees == TreeKind::Swamp { SEA_LEVEL } else { SEA_LEVEL + 1 };
            if ground < min_ground || ground + TALLEST < o.y || ground > o.y + CHUNK_SIZE_I32 {
                continue;
            }
            draw(wg, &mut canvas, biome.trees, tx, ground, tz, h);
        }
    }
}

/// Draw a tree whose trunk starts on top of the block at (x, ground, z).
pub(crate) fn draw(wg: &WorldGen, c: &mut Canvas, kind: TreeKind, x: i32, ground: i32, z: i32, h: u64) {
    let ids = wg.ids;
    let var = (h >> 20) as i32;
    match kind {
        TreeKind::None => {}
        TreeKind::Swamp => {
            let top = ground + 9 + var % 5;
            for y in ground + 1..=top {
                c.put(x, y, z, ids.swamp_log);
            }
            // Roots spreading into the mud.
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                if (var >> (dx + 2 + (dz + 1) * 3)) & 1 == 1 {
                    c.put(x + dx, ground + 1, z + dz, ids.swamp_log);
                }
            }
            for dy in -1..=1 {
                let r = if dy == 1 { REACH - 1 } else { REACH };
                disc(c, x, top + dy, z, r, ids.swamp_leaves);
            }
            // Hanging leaves under the canopy edge.
            for (dx, dz) in [(3i32, 0i32), (-3, 0), (0, 3), (0, -3), (2, 2), (-2, -2)] {
                if (var >> (dx.abs() + dz.abs())) & 1 == 1 {
                    c.fill(x + dx, top - 2, z + dz, ids.swamp_leaves);
                    // Some strands hang a block further.
                    if (var >> (dx.abs() + dz.abs() + 6)) & 1 == 1 {
                        c.fill(x + dx, top - 3, z + dz, ids.swamp_leaves);
                    }
                }
            }
        }
        TreeKind::Oak => {
            let top = ground + 7 + var % 4;
            for y in ground + 1..=top {
                c.put(x, y, z, ids.log);
            }
            for dy in -3..=1 {
                let r = if dy >= 1 || dy == -3 { 1 } else { 2 };
                disc(c, x, top + dy, z, r, ids.leaves);
            }
            c.fill(x, top + 2, z, ids.leaves);
        }
        TreeKind::Pine => {
            let top = ground + 11 + var % 6;
            for y in ground + 1..=top {
                c.put(x, y, z, ids.log);
            }
            // Layers narrowing towards the tip.
            let mut r = 0;
            for y in (ground + 4..=top + 1).rev() {
                disc(c, x, y, z, r, ids.leaves);
                r = if r >= 2 { 1 } else { r + 1 };
                if y < ground + 6 {
                    r = r.min(2);
                }
            }
            c.fill(x, top + 2, z, ids.leaves);
        }
    }
}

fn disc(c: &mut Canvas, x: i32, y: i32, z: i32, r: i32, b: BlockId) {
    for dz in -r..=r {
        for dx in -r..=r {
            if dx * dx + dz * dz <= r * r + 1 {
                c.fill(x + dx, y, z + dz, b);
            }
        }
    }
}
