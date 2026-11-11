//! World generation.
//!
//! Every function here is a pure function of (seed, position), so any chunk
//! can be generated alone, in any order, on any thread.
//!
//! Pipeline per chunk:
//! 1. **Climate** per column (2D, domain-warped): continentalness, erosion,
//!    peaks-and-valleys (ridged), temperature, humidity, detail.
//! 2. **Height** from climate through hand-tuned curves: oceans, coasts, flat
//!    swamp basins, rolling hills and tall ridged mountain ranges.
//! 3. **3D density** near the surface in mountains for overhangs and cliffs.
//! 4. **Biome** per column from data (`biomes.ron`), picking surface blocks.
//! 5. **Caves**: big chambers plus long tunnels, from 3D noise sampled on a
//!    coarse grid and interpolated (cheap).
//! 6. **Ores, trees, villages and barrows (`structures`), sky islands.**

mod biome;
mod islands;
mod plants;
mod structures;
mod trees;
mod util;

pub use biome::{Biome, Biomes, TreeKind};
pub use structures::{person_name, Barrow, Hut, Village, VILLAGE_RADIUS};

use fastnoise_lite::{FastNoiseLite, FractalType};
use voxel::chunk::{index, CHUNK_VOLUME};
use voxel::{BlockId, BlockRegistry, Chunk, ChunkPos, CHUNK_SIZE, CHUNK_SIZE_I32};

use util::{hash3, lerp, noise, smoothstep, spline};

/// Sea level in blocks.
pub const SEA_LEVEL: i32 = 0;
/// Below this, stone turns to deeprock.
pub const DEEP_LEVEL: i32 = -64;
/// The bottom of the world: unbreakable rootstone at and below this height
/// (its top is a little uneven, up to `FLOOR_BUMPS` higher), no caves near it.
pub const WORLD_FLOOR: i32 = -300;
/// How far the rootstone floor's top can rise above `WORLD_FLOOR`.
pub const FLOOR_BUMPS: i32 = 3;
/// The top of the world: nothing generates or can be built at or above this height.
pub const WORLD_CEILING: i32 = 1000;
/// Sky islands float in this band.
pub const SKY_MIN: i32 = 340;
pub const SKY_MAX: i32 = 600;

/// Climate at one column.
#[derive(Debug, Clone, Copy, Default)]
pub struct Climate {
    pub continentalness: f32,
    pub erosion: f32,
    pub ridges: f32,
    pub temperature: f32,
    pub humidity: f32,
    /// How much 3D overhang to apply (0 = plain heightmap).
    pub overhang: f32,
}

/// One column's result: surface height, climate and biome.
#[derive(Debug, Clone, Copy, Default)]
pub struct Column {
    pub height: i32,
    pub climate: Climate,
    pub biome: usize,
}

#[derive(Clone, Copy)]
struct Ids {
    stone: BlockId,
    deeprock: BlockId,
    rootstone: BlockId,
    dirt: BlockId,
    water: BlockId,
    coal: BlockId,
    iron: BlockId,
    log: BlockId,
    leaves: BlockId,
    swamp_log: BlockId,
    swamp_leaves: BlockId,
    skystone: BlockId,
    cloud_moss: BlockId,
    snow: BlockId,
    cobblestone: BlockId,
    planks: BlockId,
    peat: BlockId,
    torch: BlockId,
    chest: BlockId,
}

/// The world generator. Build once, share between threads with `Arc`.
pub struct WorldGen {
    seed: u64,
    warp_x: FastNoiseLite,
    warp_z: FastNoiseLite,
    continents: FastNoiseLite,
    erosion: FastNoiseLite,
    ridges: FastNoiseLite,
    temperature: FastNoiseLite,
    humidity: FastNoiseLite,
    detail: FastNoiseLite,
    overhang: FastNoiseLite,
    cheese: FastNoiseLite,
    tunnel_a: FastNoiseLite,
    tunnel_b: FastNoiseLite,
    island_shape: FastNoiseLite,
    /// Clumps ground plants into patches.
    plant_patch: FastNoiseLite,
    pub biomes: Biomes,
    ids: Ids,
    structures: structures::Cache,
    spawn: [i32; 3],
    /// Centre of the home village, near spawn.
    home: (i32, i32),
}

impl WorldGen {
    pub fn new(seed: u64, blocks: &BlockRegistry, biomes_ron: &str) -> Result<Self, String> {
        let b = |n: &str| blocks.id(n).ok_or_else(|| format!("worldgen needs block '{n}'"));
        let ids = Ids {
            stone: b("stone")?,
            deeprock: b("deeprock")?,
            rootstone: b("rootstone")?,
            dirt: b("dirt")?,
            water: b("water")?,
            coal: b("coal_ore")?,
            iron: b("iron_ore")?,
            log: b("log")?,
            leaves: b("leaves")?,
            swamp_log: b("swamp_log")?,
            swamp_leaves: b("swamp_leaves")?,
            skystone: b("skystone")?,
            cloud_moss: b("cloud_moss")?,
            snow: b("snow")?,
            cobblestone: b("cobblestone")?,
            planks: b("planks")?,
            peat: b("peat")?,
            torch: b("torch")?,
            chest: b("chest")?,
        };
        use FractalType::{Ridged, FBm};
        let mut wg = Self {
            seed,
            warp_x: noise(seed, 1, 0.004, 2, FBm),
            warp_z: noise(seed, 2, 0.004, 2, FBm),
            continents: noise(seed, 3, 0.0007, 5, FBm),
            erosion: noise(seed, 4, 0.0015, 4, FBm),
            ridges: noise(seed, 5, 0.0028, 4, Ridged),
            temperature: noise(seed, 6, 0.0009, 3, FBm),
            humidity: noise(seed, 7, 0.0011, 3, FBm),
            detail: noise(seed, 8, 0.035, 3, FBm),
            overhang: noise(seed, 9, 0.022, 3, FBm),
            cheese: noise(seed, 10, 0.014, 2, FBm),
            tunnel_a: noise(seed, 11, 0.012, 1, FBm),
            tunnel_b: noise(seed, 12, 0.012, 1, FBm),
            island_shape: noise(seed, 13, 0.04, 2, FBm),
            plant_patch: noise(seed, 14, 0.045, 2, FBm),
            biomes: Biomes::from_ron(biomes_ron, blocks)?,
            ids,
            structures: structures::Cache::default(),
            spawn: [0, 0, 0],
            home: (0, 0),
        };
        wg.spawn = wg.search_spawn();
        wg.home = wg.search_home();
        Ok(wg)
    }

    /// Dry, flat ground 50-90 blocks from spawn for the home village.
    fn search_home(&self) -> (i32, i32) {
        let [sx, _, sz] = self.spawn;
        for ring in 0..6 {
            let r = 50.0 + ring as f32 * 8.0;
            for s in 0..16 {
                let a = s as f32 / 16.0 * std::f32::consts::TAU + ring as f32 * 0.3;
                let (x, z) = (sx + (a.cos() * r) as i32, sz + (a.sin() * r) as i32);
                let col = self.column(x, z);
                if col.height >= SEA_LEVEL && col.climate.overhang < 0.01 {
                    return (x, z);
                }
            }
        }
        (sx + 60, sz)
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Climate, height and biome of a column.
    pub fn column(&self, x: i32, z: i32) -> Column {
        let (fx, fz) = (x as f32, z as f32);
        // Domain warp: bends coastlines and ranges so they look organic.
        let wx = fx + self.warp_x.get_noise_2d(fx, fz) * 70.0;
        let wz = fz + self.warp_z.get_noise_2d(fx, fz) * 70.0;

        let c = self.continents.get_noise_2d(wx, wz) * 1.4 + 0.12;
        let e = self.erosion.get_noise_2d(wx, wz) * 1.3;
        // Ridged noise is ~-1..1 with sharp crests at the top; remap to 0..1.
        let r = (self.ridges.get_noise_2d(wx, wz) * 0.5 + 0.5).clamp(0.0, 1.0);
        let t = (self.temperature.get_noise_2d(wx, wz) * 1.5).clamp(-1.0, 1.0);
        let h = (self.humidity.get_noise_2d(wx, wz) * 1.5).clamp(-1.0, 1.0);
        let d = self.detail.get_noise_2d(fx, fz);

        // Base land height from continentalness: deep ocean -> coast -> inland.
        let base = spline(
            &[(-1.2, -60.0), (-0.45, -28.0), (-0.18, -6.0), (-0.05, 0.5), (0.1, 3.0), (0.4, 10.0), (0.8, 22.0), (1.2, 30.0)],
            c,
        );
        let inland = smoothstep(-0.05, 0.35, c);
        // Low erosion = rugged mountains; high erosion = worn-down plains.
        let mountain = smoothstep(0.15, -0.55, e) * inland;
        let hills = (1.0 - mountain) * inland * smoothstep(0.6, -0.2, e);
        let peaks = r * r * r * 300.0 + r * 30.0;
        let mut height = base + mountain * peaks + hills * r * 22.0;

        // The swamp: warm, wet, low and flat. Flatten towards just above sea level.
        let swamp = smoothstep(0.35, 0.65, h) * smoothstep(-0.1, 0.25, t) * (1.0 - mountain) * smoothstep(-0.2, 0.1, c);
        height = lerp(height, SEA_LEVEL as f32 + 0.2 + d * 2.4, swamp * 0.92);

        // Small-scale roughness so nothing looks too smooth; more on mountains.
        height += d * (1.5 + mountain * 7.0 + hills * 2.0);

        // Colder with altitude.
        let t_alt = (t - (height - 40.0).max(0.0) / 220.0).clamp(-1.0, 1.0);
        let height_i = height.floor() as i32;
        let overhang = mountain * smoothstep(0.25, 0.6, r);
        let biome = self.biomes.select(t_alt, h, height_i - SEA_LEVEL);
        Column {
            height: height_i,
            climate: Climate { continentalness: c, erosion: e, ridges: r, temperature: t_alt, humidity: h, overhang },
            biome,
        }
    }

    /// The biome at a position, including the sky island biome high up.
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> &Biome {
        if y > SKY_MIN - 40 && self.island_top(x, z).is_some() {
            return self.biomes.get(self.biomes.sky);
        }
        self.biomes.get(self.column(x, z).biome)
    }

    /// Surface height ignoring overhangs and caves.
    pub fn surface_height(&self, x: i32, z: i32) -> i32 {
        self.column(x, z).height
    }

    /// A spawn point in the Great Swamp, on dry ground, near the origin.
    pub fn find_spawn(&self) -> [i32; 3] {
        self.spawn
    }

    fn search_spawn(&self) -> [i32; 3] {
        let swamp = self.biomes.spawn;
        // Spiral search on a coarse grid, out to ~6 km.
        for ring in 0..200i32 {
            let r = ring * 32;
            let steps = (ring * 8).max(1);
            for s in 0..steps {
                let a = s as f32 / steps as f32 * std::f32::consts::TAU;
                let (x, z) = ((a.cos() * r as f32) as i32, (a.sin() * r as f32) as i32);
                let col = self.column(x, z);
                if col.biome == swamp && col.height > SEA_LEVEL && col.climate.overhang < 0.01 {
                    return [x, col.height + 1, z];
                }
            }
        }
        [0, self.surface_height(0, 0).max(SEA_LEVEL) + 2, 0]
    }

    /// Generate one chunk.
    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let origin = pos.origin();
        let (min_y, max_y) = (origin.y, origin.y + CHUNK_SIZE_I32 - 1);

        if max_y < WORLD_FLOOR {
            return Chunk::filled(self.ids.rootstone);
        }
        let island_chunk = max_y >= SKY_MIN - 80 && min_y <= SKY_MAX;
        if min_y > SKY_MAX || min_y >= WORLD_CEILING {
            return Chunk::filled(BlockId::AIR);
        }

        let mut cols = [[Column::default(); CHUNK_SIZE]; CHUNK_SIZE];
        let mut hi = i32::MIN;
        for (lz, row) in cols.iter_mut().enumerate() {
            for (lx, col) in row.iter_mut().enumerate() {
                *col = self.column(origin.x + lx as i32, origin.z + lz as i32);
                hi = hi.max(col.height);
            }
        }
        // Overhangs reach at most this far above the heightmap; trees ~14.
        let top_reach = hi.max(SEA_LEVEL) + 24;
        let mut dense = vec![BlockId::AIR; CHUNK_VOLUME];
        let mut any = false;

        if min_y <= top_reach {
            any = true;
            self.fill_terrain(pos, &cols, &mut dense);
            trees::place(self, pos, &mut dense);
            structures::place(self, pos, &mut dense);
            plants::place(self, pos, &cols, &mut dense);
        }
        if island_chunk {
            any |= islands::place(self, pos, &mut dense);
        }
        if !any {
            return Chunk::filled(BlockId::AIR);
        }
        Chunk::from_dense(&dense)
    }

    fn solid_at(&self, x: i32, y: i32, z: i32, col: &Column) -> bool {
        if col.climate.overhang < 0.02 || (y - col.height).abs() > 22 {
            return y <= col.height;
        }
        let n = self.overhang.get_noise_3d(x as f32, y as f32 * 1.4, z as f32);
        (col.height - y) as f32 + n * 22.0 * col.climate.overhang > 0.0
    }

    fn fill_terrain(&self, pos: ChunkPos, cols: &[[Column; CHUNK_SIZE]; CHUNK_SIZE], dense: &mut [BlockId]) {
        let origin = pos.origin();
        let min_y = origin.y;
        let ids = self.ids;
        let caves = CaveField::new(self, pos);
        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                let col = &cols[lz][lx];
                let biome = self.biomes.get(col.biome);
                let (wx, wz) = (origin.x + lx as i32, origin.z + lz as i32);
                // Depth below the nearest air above, scanning down from just above the chunk.
                let mut depth = 0;
                for y in (min_y + CHUNK_SIZE_I32..min_y + CHUNK_SIZE_I32 + 4).rev() {
                    depth = if self.solid_at(wx, y, wz, col) { depth + 1 } else { 0 };
                }
                for ly in (0..CHUNK_SIZE).rev() {
                    let y = min_y + ly as i32;
                    if y <= WORLD_FLOOR + floor_bump(wx, wz) {
                        dense[index(lx, ly, lz)] = ids.rootstone;
                        depth += 1;
                        continue;
                    }
                    let solid = y < WORLD_CEILING && self.solid_at(wx, y, wz, col);
                    let b = if !solid {
                        depth = 0;
                        if y <= SEA_LEVEL { ids.water } else { BlockId::AIR }
                    } else {
                        depth += 1;
                        let underwater = col.height < SEA_LEVEL;
                        let rock = if y < DEEP_LEVEL + (wx ^ wz) % 3 { ids.deeprock } else { ids.stone };
                        let mut b = if depth == 1 {
                            if underwater {
                                biome.underwater
                            } else if biome.name == "highlands" && col.height > 110 {
                                ids.snow
                            } else if col.climate.overhang > 0.02 && y < col.height - 2 {
                                ids.stone // cliff faces
                            } else {
                                biome.surface
                            }
                        } else if depth <= 4 {
                            if underwater { biome.underwater } else if biome.subsurface == ids.dirt && col.climate.overhang > 0.02 { ids.stone } else { biome.subsurface }
                        } else {
                            rock
                        };
                        // Caves: carved below the surface layers, never right under the sea floor.
                        if depth > 4 && y > WORLD_FLOOR + 8 && !(underwater && y > col.height - 12) && caves.is_cave(lx, ly, lz) {
                            b = BlockId::AIR;
                        } else if b == rock {
                            b = self.ore(wx, y, wz, rock);
                        }
                        b
                    };
                    dense[index(lx, ly, lz)] = b;
                }
            }
        }
    }

    fn ore(&self, x: i32, y: i32, z: i32, rock: BlockId) -> BlockId {
        if rock != self.ids.stone {
            return rock;
        }
        // Ores come in small clusters: the cluster cell decides, the block jitters.
        let cell = hash3(self.seed ^ 0x0E, x.div_euclid(3), y.div_euclid(3), z.div_euclid(3));
        let here = hash3(self.seed ^ 0x0F, x, y, z);
        match cell % 1000 {
            0..=9 if !here.is_multiple_of(3) => self.ids.coal,
            10..=14 if y < SEA_LEVEL - 10 && here.is_multiple_of(2) => self.ids.iron,
            _ => rock,
        }
    }

    /// Top of the sky island over this column, if any.
    pub fn island_top(&self, x: i32, z: i32) -> Option<i32> {
        islands::top_at(self, x, z)
    }

    /// (bottom, top) of the sky island over this column, if any.
    pub fn island_span(&self, x: i32, z: i32) -> Option<(i32, i32)> {
        islands::span_at(self, x, z)
    }
}

/// Cave noise sampled every 4 blocks and interpolated: ~50x cheaper than per block.
struct CaveField {
    cheese: Vec<f32>,
    tunnel: Vec<f32>,
}

const CAVE_STEP: usize = 4;
const CAVE_N: usize = CHUNK_SIZE / CAVE_STEP + 1;

impl CaveField {
    fn new(wg: &WorldGen, pos: ChunkPos) -> Self {
        let o = pos.origin();
        let mut cheese = vec![0.0; CAVE_N * CAVE_N * CAVE_N];
        let mut tunnel = vec![0.0; CAVE_N * CAVE_N * CAVE_N];
        for iy in 0..CAVE_N {
            for iz in 0..CAVE_N {
                for ix in 0..CAVE_N {
                    let x = (o.x + (ix * CAVE_STEP) as i32) as f32;
                    let y = (o.y + (iy * CAVE_STEP) as i32) as f32;
                    let z = (o.z + (iz * CAVE_STEP) as i32) as f32;
                    let i = ix + iz * CAVE_N + iy * CAVE_N * CAVE_N;
                    cheese[i] = wg.cheese.get_noise_3d(x, y * 1.7, z);
                    let a = wg.tunnel_a.get_noise_3d(x, y * 1.3, z);
                    let b = wg.tunnel_b.get_noise_3d(x, y * 1.3, z);
                    tunnel[i] = a * a + b * b;
                }
            }
        }
        Self { cheese, tunnel }
    }

    fn sample(field: &[f32], lx: usize, ly: usize, lz: usize) -> f32 {
        let (ix, iy, iz) = (lx / CAVE_STEP, ly / CAVE_STEP, lz / CAVE_STEP);
        let s = CAVE_STEP as f32;
        let (fx, fy, fz) = ((lx % CAVE_STEP) as f32 / s, (ly % CAVE_STEP) as f32 / s, (lz % CAVE_STEP) as f32 / s);
        let at = |x: usize, y: usize, z: usize| field[x + z * CAVE_N + y * CAVE_N * CAVE_N];
        let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let x00 = l(at(ix, iy, iz), at(ix + 1, iy, iz), fx);
        let x10 = l(at(ix, iy + 1, iz), at(ix + 1, iy + 1, iz), fx);
        let x01 = l(at(ix, iy, iz + 1), at(ix + 1, iy, iz + 1), fx);
        let x11 = l(at(ix, iy + 1, iz + 1), at(ix + 1, iy + 1, iz + 1), fx);
        l(l(x00, x10, fy), l(x01, x11, fy), fz)
    }

    fn is_cave(&self, lx: usize, ly: usize, lz: usize) -> bool {
        Self::sample(&self.cheese, lx, ly, lz) > 0.58 || Self::sample(&self.tunnel, lx, ly, lz) < 0.006
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxel::TileSheet;

    pub(crate) fn wg() -> WorldGen {
        let tiles = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
        let reg = BlockRegistry::from_ron(include_str!("../../../assets/data/blocks.ron"), &tiles).unwrap();
        WorldGen::new(42, &reg, include_str!("../../../assets/data/biomes.ron")).unwrap()
    }

    #[test]
    fn deterministic() {
        let g = wg();
        let p = ChunkPos::new(3, 0, -2);
        assert_eq!(g.generate(p).to_dense(), g.generate(p).to_dense());
    }

    #[test]
    fn high_sky_is_air_and_floor_is_solid() {
        let g = wg();
        assert_eq!(g.generate(ChunkPos::new(0, 40, 0)).is_uniform(), Some(BlockId::AIR));
        assert_eq!(g.generate(ChunkPos::new(0, -11, 0)).is_uniform(), Some(g.ids.rootstone));
        assert_eq!(g.generate(ChunkPos::new(0, 32, 0)).is_uniform(), Some(BlockId::AIR));
    }

    #[test]
    fn floor_is_rootstone_with_a_bumpy_top() {
        let g = wg();
        // Chunk y -10 spans -320..=-289 and holds the floor.
        let c = g.generate(ChunkPos::new(0, -10, 0)).to_dense();
        let origin = ChunkPos::new(0, -10, 0).origin().y;
        for lz in 0..CHUNK_SIZE {
            for lx in 0..CHUNK_SIZE {
                for ly in 0..CHUNK_SIZE {
                    let y = origin + ly as i32;
                    let b = c[index(lx, ly, lz)];
                    if y <= WORLD_FLOOR {
                        assert_eq!(b, g.ids.rootstone, "floor at y {y}");
                    } else if y > WORLD_FLOOR + FLOOR_BUMPS {
                        assert_ne!(b, g.ids.rootstone, "rootstone above the floor at y {y}");
                    }
                }
            }
        }
    }

    #[test]
    fn spawn_is_in_the_swamp() {
        let g = wg();
        let [x, y, z] = g.find_spawn();
        assert_eq!(g.biome_at(x, y, z).name, "great_swamp");
        assert!(y >= SEA_LEVEL);
    }

    #[test]
    fn world_has_variety() {
        // Over a 6 km square, expect oceans, mountains and several biomes.
        let g = wg();
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        let mut seen = std::collections::HashSet::new();
        for z in (-3000..3000).step_by(50) {
            for x in (-3000..3000).step_by(50) {
                let c = g.column(x, z);
                lo = lo.min(c.height);
                hi = hi.max(c.height);
                seen.insert(c.biome);
            }
        }
        assert!(lo < -20, "deep water exists (lowest {lo})");
        assert!(hi > 150, "tall mountains exist (highest {hi})");
        assert!(seen.len() >= 6, "at least six biomes, saw {}", seen.len());
    }

    #[test]
    fn caves_exist_underground() {
        let g = wg();
        let mut air = 0;
        for x in -4..4 {
            for z in -4..4 {
                let c = g.generate(ChunkPos::new(x, -2, z)).to_dense();
                air += c.iter().filter(|b| b.is_air()).count();
            }
        }
        assert!(air > 1000, "some cave air between y -64 and -33 ({air})");
    }
}

/// How many blocks the rootstone floor rises above `WORLD_FLOOR` in this column (0..=FLOOR_BUMPS).
fn floor_bump(x: i32, z: i32) -> i32 {
    let h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (z as u32).wrapping_mul(0x85EB_CA77);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    ((h >> 13) % (FLOOR_BUMPS as u32 + 1)) as i32
}
