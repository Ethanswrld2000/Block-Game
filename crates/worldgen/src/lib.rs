//! World generation.
//!
//! Phase 2 ships a simple test terrain (swampy lowlands, low hills, trees) so
//! chunk loading can be tested at scale. Phase 3 replaces it with the full
//! generator: climate noises, height splines, 3D density, biomes, caves and
//! sky islands.
//!
//! Rule: every function here is a pure function of (seed, position), so any
//! chunk can be generated alone, in any order, on any thread.

use fastnoise_lite::{FastNoiseLite, FractalType, NoiseType};
use voxel::{BlockId, BlockRegistry, Chunk, ChunkPos, CHUNK_SIZE, CHUNK_SIZE_I32};

/// Name of the biome the player spawns in.
pub const SPAWN_BIOME: &str = "The Great Swamp";

/// Sea / swamp water level in blocks.
pub const WATER_LEVEL: i32 = 4;

/// Block ids the generator uses, looked up once from the registry.
#[derive(Clone, Copy, Debug)]
struct Palette {
    stone: BlockId,
    dirt: BlockId,
    grass: BlockId,
    mud: BlockId,
    moss: BlockId,
    peat: BlockId,
    sand: BlockId,
    water: BlockId,
    log: BlockId,
    leaves: BlockId,
}

/// Generator for the test terrain. Cheap to share between threads (`Arc`).
pub struct TestTerrain {
    seed: u64,
    plains: FastNoiseLite,
    hills: FastNoiseLite,
    wet: FastNoiseLite,
    ids: Palette,
}

impl TestTerrain {
    pub fn new(seed: u64, blocks: &BlockRegistry) -> Self {
        let s = seed as i32;
        let noise = |offset: i32, freq: f32, octaves: i32| {
            let mut n = FastNoiseLite::with_seed(s.wrapping_add(offset));
            n.set_noise_type(Some(NoiseType::OpenSimplex2));
            n.set_fractal_type(Some(FractalType::FBm));
            n.set_fractal_octaves(Some(octaves));
            n.set_frequency(Some(freq));
            n
        };
        Self {
            seed,
            plains: noise(0, 0.01, 4),
            hills: noise(101, 0.0025, 3),
            wet: noise(202, 0.02, 2),
            ids: Palette {
                stone: blocks.expect("stone"),
                dirt: blocks.expect("dirt"),
                grass: blocks.expect("grass"),
                mud: blocks.expect("mud"),
                moss: blocks.expect("moss"),
                peat: blocks.expect("peat"),
                sand: blocks.expect("sand"),
                water: blocks.expect("water"),
                log: blocks.expect("swamp_log"),
                leaves: blocks.expect("swamp_leaves"),
            },
        }
    }

    /// Height of the ground surface (the top solid block) at a column.
    pub fn surface_height(&self, x: i32, z: i32) -> i32 {
        let (fx, fz) = (x as f32, z as f32);
        let flat = self.plains.get_noise_2d(fx, fz) * 5.0;
        // Hills rise sharply only where the low-frequency noise is high.
        let h = self.hills.get_noise_2d(fx, fz).max(0.0);
        let hills = h * h * 140.0;
        (WATER_LEVEL as f32 + 1.0 + flat + hills).floor() as i32
    }

    /// Generate one chunk.
    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let origin = pos.origin();
        let min_y = origin.y;
        let max_y = origin.y + CHUNK_SIZE_I32 - 1;

        // Fast paths: chunks far above ground and water, or deep underground.
        let mut col_heights = [[0i32; CHUNK_SIZE]; CHUNK_SIZE];
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for (lz, row) in col_heights.iter_mut().enumerate() {
            for (lx, h) in row.iter_mut().enumerate() {
                *h = self.surface_height(origin.x + lx as i32, origin.z + lz as i32);
                lo = lo.min(*h);
                hi = hi.max(*h);
            }
        }
        let tree_reach = 12; // tallest tree top above the surface
        if min_y > hi.max(WATER_LEVEL) + tree_reach {
            return Chunk::filled(BlockId::AIR);
        }
        if max_y < lo - 4 {
            return Chunk::filled(self.ids.stone);
        }

        let p = self.ids;
        let mut dense = vec![BlockId::AIR; voxel::chunk::CHUNK_VOLUME];
        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                let (wx, wz) = (origin.x + lx as i32, origin.z + lz as i32);
                let h = col_heights[lz][lx];
                let wet = self.wet.get_noise_2d(wx as f32, wz as f32);
                for ly in 0..CHUNK_SIZE {
                    let y = min_y + ly as i32;
                    let b = if y > h {
                        if y <= WATER_LEVEL { p.water } else { BlockId::AIR }
                    } else if y < h - 4 {
                        p.stone
                    } else if y == h {
                        if h < WATER_LEVEL {
                            p.mud
                        } else if h == WATER_LEVEL {
                            if wet > 0.2 { p.moss } else { p.mud }
                        } else if h > 60 {
                            p.stone
                        } else if h <= WATER_LEVEL + 2 && wet > 0.0 {
                            p.moss
                        } else if h <= WATER_LEVEL + 1 && wet < -0.5 {
                            p.sand
                        } else {
                            p.grass
                        }
                    } else if h <= WATER_LEVEL + 1 {
                        p.peat
                    } else {
                        p.dirt
                    };
                    dense[voxel::chunk::index(lx, ly, lz)] = b;
                }
            }
        }

        self.place_trees(pos, &mut dense);
        Chunk::from_dense(&dense)
    }

    /// Swamp trees: tall trunks with a wide flat canopy. Trees whose canopy
    /// crosses the chunk border are drawn partly by each chunk, so they line up.
    fn place_trees(&self, pos: ChunkPos, dense: &mut [BlockId]) {
        const REACH: i32 = 3; // canopy radius
        let origin = pos.origin();
        let in_chunk = |x: i32, y: i32, z: i32| -> Option<usize> {
            let (lx, ly, lz) = (x - origin.x, y - origin.y, z - origin.z);
            let r = 0..CHUNK_SIZE_I32;
            (r.contains(&lx) && r.contains(&ly) && r.contains(&lz))
                .then(|| voxel::chunk::index(lx as usize, ly as usize, lz as usize))
        };
        for tz in origin.z - REACH..origin.z + CHUNK_SIZE_I32 + REACH {
            for tx in origin.x - REACH..origin.x + CHUNK_SIZE_I32 + REACH {
                let r = hash(self.seed, tx, tz);
                if r % 1000 >= 6 {
                    continue; // ~0.6% of columns
                }
                let h = self.surface_height(tx, tz);
                if !(WATER_LEVEL - 1..=WATER_LEVEL + 12).contains(&h) {
                    continue;
                }
                let trunk = 6 + (r >> 10) as i32 % 4;
                let top = h + trunk;
                if top + 2 < origin.y || h > origin.y + CHUNK_SIZE_I32 {
                    continue;
                }
                for y in h + 1..=top {
                    if let Some(i) = in_chunk(tx, y, tz) {
                        dense[i] = self.ids.log;
                    }
                }
                for dy in -1..=1 {
                    let radius = if dy == 1 { REACH - 1 } else { REACH };
                    for dz in -radius..=radius {
                        for dx in -radius..=radius {
                            if dx * dx + dz * dz > radius * radius + 1 {
                                continue;
                            }
                            if let Some(i) = in_chunk(tx + dx, top + dy, tz + dz)
                                && dense[i].is_air()
                            {
                                dense[i] = self.ids.leaves;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Deterministic 64-bit hash of a column, for scattering features.
fn hash(seed: u64, x: i32, z: i32) -> u64 {
    let mut h = seed ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^ (h >> 33)
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxel::TileSheet;

    fn terrain() -> TestTerrain {
        let tiles = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
        let reg = BlockRegistry::from_ron(include_str!("../../../assets/data/blocks.ron"), &tiles).unwrap();
        TestTerrain::new(42, &reg)
    }

    #[test]
    fn deterministic() {
        let t = terrain();
        let a = t.generate(ChunkPos::new(3, 0, -2)).to_dense();
        let b = t.generate(ChunkPos::new(3, 0, -2)).to_dense();
        assert_eq!(a, b);
    }

    #[test]
    fn sky_is_air_and_depths_are_stone() {
        let t = terrain();
        assert_eq!(t.generate(ChunkPos::new(0, 20, 0)).is_uniform(), Some(BlockId::AIR));
        assert!(t.generate(ChunkPos::new(0, -5, 0)).is_uniform().is_some());
    }

    #[test]
    fn surface_chunk_is_mixed() {
        let t = terrain();
        assert!(t.generate(ChunkPos::new(0, 0, 0)).is_uniform().is_none());
    }
}
