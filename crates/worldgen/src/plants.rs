//! Ground plants: tall grass, reeds, ferns and flowers on top of the surface,
//! picked from the biome's `plants` list (biomes.ron) and clumped into patches
//! by a noise field. Runs after trees and structures, and only fills air
//! above the biome's own surface block, so nothing grows on huts, trunks or
//! cliff faces.

use voxel::chunk::index;
use voxel::{BlockId, ChunkPos, CHUNK_SIZE, CHUNK_SIZE_I32};

use crate::util::{hash2, smoothstep, unit};
use crate::{Column, WorldGen, SEA_LEVEL};

pub(crate) fn place(wg: &WorldGen, pos: ChunkPos, cols: &[[Column; CHUNK_SIZE]; CHUNK_SIZE], dense: &mut [BlockId]) {
    let o = pos.origin();
    let villages = wg.villages_in(o.x, o.z, o.x + CHUNK_SIZE_I32, o.z + CHUNK_SIZE_I32);
    for (lz, row) in cols.iter().enumerate() {
        for (lx, col) in row.iter().enumerate() {
            let biome = wg.biomes.get(col.biome);
            if biome.plants.is_empty() || col.climate.overhang > 0.02 || col.height < SEA_LEVEL {
                continue;
            }
            // The plant goes in the block above the ground.
            let ly = col.height + 1 - o.y;
            if !(0..CHUNK_SIZE_I32).contains(&ly) {
                continue;
            }
            let at = index(lx, ly as usize, lz);
            if !dense[at].is_air() {
                continue;
            }
            // The ground must be the biome's surface (not a path, hut or snow cap).
            // When it's in the chunk below, trust the column.
            if ly > 0 && dense[index(lx, ly as usize - 1, lz)] != biome.surface {
                continue;
            }
            let (x, z) = (o.x + lx as i32, o.z + lz as i32);
            if crate::structures::in_village(&villages, x, z) {
                continue;
            }
            let patch = smoothstep(-0.45, 0.35, wg.plant_patch.get_noise_2d(x as f32, z as f32)) * 1.5;
            let h = hash2(wg.seed ^ 0x9_1A47, x, z);
            for (i, p) in biome.plants.iter().enumerate() {
                if p.shore && col.height > SEA_LEVEL + 1 {
                    continue;
                }
                // Reeds by the water grow thick wherever there is shore.
                let chance = if p.shore { p.chance } else { p.chance * patch };
                if unit(h.rotate_left(i as u32 * 13)) < chance {
                    dense[at] = p.block;
                    break;
                }
            }
        }
    }
}
