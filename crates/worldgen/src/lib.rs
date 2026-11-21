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
//!
//! All of it floats (`floating`): one big island around spawn with a rounded,
//! stalactite-hung underside, open void past its coast, and more islands off
//! in the distance.

mod biome;
mod dungeons;
mod floating;
mod islands;
mod landmarks;
mod plants;
mod rotting;
mod structures;
mod trees;
mod util;

pub use biome::{Biome, Biomes, Guaranteed, Region, TreeKind};
pub use dungeons::{Aabb, Dungeon, Room, RoomKind, SpawnKind, SpawnPoint, Veil};
pub use floating::{Land, MAIN_RADIUS};
pub use rotting::{floor as rotting_floor, in_pocket, Floor, FloorKind, FloorRoom, RottingGate, DEPTHS, POCKET};
pub use landmarks::{AuntIsland, Castle, Forge, AUNT_RADIUS, SPORROW_RADIUS};
pub use structures::{person_name, Barrow, Folk, Hut, Village, VILLAGE_RADIUS};

use std::sync::Arc;

use fastnoise_lite::{FastNoiseLite, FractalType};
use voxel::chunk::{index, CHUNK_VOLUME};
use voxel::{BlockId, BlockRegistry, Chunk, ChunkPos, CHUNK_SIZE, CHUNK_SIZE_I32};

use util::{hash3, lerp, noise, smoothstep, spline};

/// Sea level in blocks.
pub const SEA_LEVEL: i32 = 0;
/// Below this, stone turns to deeprock.
pub const DEEP_LEVEL: i32 = -64;
/// The bottom of the void: nothing generates below this height (the lowest
/// island undersides end above it), and falling past it is fatal.
pub const WORLD_FLOOR: i32 = -520;
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
    /// Top of the ground (below `WORLD_FLOOR` in the void).
    pub height: i32,
    pub climate: Climate,
    pub biome: usize,
    /// Which island this column is on, if any.
    pub land: Land,
    /// The island's underside: blocks at or below this are open air.
    pub bottom: i32,
    /// Water fills open blocks up to this height (only on the main island).
    pub water: i32,
}

impl Column {
    /// Open sky all the way down.
    fn void(climate: Climate, biome: usize) -> Self {
        Self { height: WORLD_FLOOR - 1, climate, biome, land: Land::Void, bottom: i32::MAX, water: i32::MIN }
    }
}

#[derive(Clone, Copy)]
struct Ids {
    stone: BlockId,
    deeprock: BlockId,
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
    /// Vines clinging to the north, south, east and west side of their cell.
    vine: [BlockId; 4],
    /// Twigs poking out of the trunk beside them, by the side they're attached to.
    twig: [BlockId; 4],
    pebble: BlockId,
    moss: BlockId,
    mud: BlockId,
    mushroom_stem: BlockId,
    mushroom_cap: BlockId,
    redwood_log: BlockId,
    redwood_leaves: BlockId,
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
    /// Main island coastline (outer islands use it scaled down).
    coast: FastNoiseLite,
    /// Lumps on island undersides.
    underside: FastNoiseLite,
    pub biomes: Biomes,
    ids: Ids,
    structures: structures::Cache,
    spawn: [i32; 3],
    /// Centre of the home village, near spawn.
    home: (i32, i32),
    /// Centre of the main island (the spawn column).
    center: (i32, i32),
    /// The dungeon a short walk from spawn.
    home_dungeon: Option<Arc<dungeons::Dungeon>>,
    /// The Rotting Gate (one per world).
    rotting_gate: Option<rotting::RottingGate>,
    /// Sporrow, Gilm's hut and the castle (one each per world).
    landmarks: Landmarks,
    /// Areas of guaranteed biomes on the main island (one per such biome).
    zones: Vec<Zone>,
}

/// Where a guaranteed biome (`Biome::guaranteed`) lies on the main island.
#[derive(Debug, Clone, Copy)]
pub struct Zone {
    pub biome: usize,
    pub x: i32,
    pub z: i32,
    pub radius: f32,
}

#[derive(Default)]
struct Landmarks {
    sporrow: Option<Arc<Village>>,
    forge: Option<landmarks::Forge>,
    castle: Option<landmarks::Castle>,
    aunt: Option<landmarks::AuntIsland>,
}

impl WorldGen {
    pub fn new(seed: u64, blocks: &BlockRegistry, biomes_ron: &str) -> Result<Self, String> {
        let b = |n: &str| blocks.id(n).ok_or_else(|| format!("worldgen needs block '{n}'"));
        let ids = Ids {
            stone: b("stone")?,
            deeprock: b("deeprock")?,
            vine: [b("vine_north")?, b("vine_south")?, b("vine_east")?, b("vine_west")?],
            twig: [b("twig_north")?, b("twig_south")?, b("twig_east")?, b("twig_west")?],
            pebble: b("pebble")?,
            moss: b("moss")?,
            mud: b("mud")?,
            mushroom_stem: b("mushroom_stem")?,
            mushroom_cap: b("mushroom_cap")?,
            redwood_log: b("redwood_log")?,
            redwood_leaves: b("redwood_leaves")?,
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
            coast: noise(seed, 15, 0.0011, 4, FBm),
            underside: noise(seed, 16, 0.02, 2, FBm),
            biomes: Biomes::from_ron(biomes_ron, blocks)?,
            ids,
            structures: structures::Cache::default(),
            spawn: [0, 0, 0],
            home: (0, 0),
            center: (0, 0),
            home_dungeon: None,
            rotting_gate: None,
            landmarks: Landmarks::default(),
            zones: Vec::new(),
        };
        wg.spawn = wg.search_spawn();
        wg.center = (wg.spawn[0], wg.spawn[2]);
        wg.zones = wg.search_zones();
        wg.home = wg.search_home();
        wg.home_dungeon = wg.search_home_dungeon().map(Arc::new);
        wg.rotting_gate = wg.search_rotting_gate();
        if let Some((v, f)) = wg.search_sporrow() {
            wg.landmarks.sporrow = Some(Arc::new(v));
            wg.landmarks.forge = Some(f);
        }
        wg.landmarks.castle = wg.search_castle();
        wg.landmarks.aunt = wg.search_aunt_island();
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

    /// Centre of the main island.
    pub fn island_center(&self) -> (i32, i32) {
        self.center
    }

    /// Climate, height and biome of a column, on whichever island it is (or void).
    pub fn column(&self, x: i32, z: i32) -> Column {
        let terrain = self.terrain(x, z);
        let e = floating::main_edge(self, x, z);
        if e < 1.0 {
            let d = self.detail.get_noise_2d(x as f32, z as f32);
            let lifted = floating::lift_rim(terrain.height as f32, e, d).floor() as i32;
            let mut col = terrain;
            if lifted != terrain.height {
                col.height = lifted;
                col.biome = self.biomes.select(col.climate.temperature, col.climate.humidity, lifted - SEA_LEVEL);
            }
            if let Some(b) = self.zone_biome(x, z, col.height - SEA_LEVEL) {
                col.biome = b;
            }
            col.bottom = floating::underside(self, x, z, SEA_LEVEL as f32, e, floating::CORE_DEPTH, 1.0);
            col.height = col.height.max(col.bottom + 1);
            return col;
        }
        let Some((isl, e)) = floating::outer_at(self, x, z) else {
            return Column::void(terrain.climate, terrain.biome);
        };
        // Same climate and relief as the main terrain, flattened onto the island.
        let scale = (isl.radius / 150.0).clamp(0.3, 1.0);
        let fall = 1.0 - e.powi(4);
        let relief = (terrain.height - SEA_LEVEL).clamp(-8, 80) as f32 * 0.35 * scale * fall;
        let dome = isl.radius * 0.06 * (1.0 - e * e);
        let top = (isl.y as f32 + dome + relief).floor() as i32;
        let bottom = floating::underside(self, x, z, isl.y as f32, e, isl.radius * 0.9, scale);
        let c = terrain.climate;
        Column {
            height: top.max(bottom + 1),
            climate: Climate { overhang: 0.0, ..c },
            biome: {
                let rel = (top - isl.y).max(2);
                self.biomes.select_in(Region::Outer, c.temperature, c.humidity, rel).unwrap_or_else(|| self.biomes.select(c.temperature, c.humidity, rel))
            },
            land: Land::Outer,
            bottom,
            water: i32::MIN,
        }
    }

    /// The endless terrain the islands are cut from (main island land, no underside).
    fn terrain(&self, x: i32, z: i32) -> Column {
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
            land: Land::Main,
            bottom: WORLD_FLOOR,
            water: SEA_LEVEL,
        }
    }

    /// The biome at a position: a sky island's high up, open air (an `air` biome)
    /// over the void or far above the land, else the column's.
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> &Biome {
        if y > SKY_MIN - 40
            && let Some(b) = self.island_biome(x, z)
        {
            return b;
        }
        let col = self.column(x, z);
        if col.land == Land::Void || (y > SKY_MIN - 40 && y > col.height + 40) {
            let c = col.climate;
            if let Some(b) = self.biomes.select_in(Region::Air, c.temperature, c.humidity, y - SEA_LEVEL) {
                return self.biomes.get(b);
            }
        }
        self.biomes.get(col.biome)
    }

    /// The biome of the sky island over this column, if any (picked by the
    /// climate under the island's centre, so one island is one biome).
    pub fn island_biome(&self, x: i32, z: i32) -> Option<&Biome> {
        islands::island_at(self, x, z).map(|isl| self.biome_of_island(&isl))
    }

    pub(crate) fn biome_of_island(&self, isl: &islands::Island) -> &Biome {
        let c = self.terrain(isl.x, isl.z).climate;
        let i = self.biomes.select_in(Region::Sky, c.temperature, c.humidity, isl.y - SEA_LEVEL).unwrap_or(self.biomes.sky);
        self.biomes.get(i)
    }

    /// Areas of the guaranteed biomes (e.g. the fungal growth) on the main island.
    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }

    /// The guaranteed biome covering this main island column, if its height fits.
    fn zone_biome(&self, x: i32, z: i32, height: i32) -> Option<usize> {
        self.zones.iter().find_map(|zn| {
            let b = self.biomes.get(zn.biome);
            if height < b.min_height || height > b.max_height {
                return None;
            }
            let (dx, dz) = ((x - zn.x) as f32, (z - zn.z) as f32);
            let wobble = 1.0 + 0.25 * self.coast.get_noise_2d(x as f32 * 6.0, z as f32 * 6.0);
            (dx * dx + dz * dz <= (zn.radius * wobble).powi(2)).then_some(zn.biome)
        })
    }

    /// Place each guaranteed biome once: dry, low ground on the main island,
    /// `near..far` blocks from spawn, with room for its whole radius.
    fn search_zones(&self) -> Vec<Zone> {
        let [sx, _, sz] = self.spawn;
        let mut out: Vec<Zone> = Vec::new();
        for (biome, g) in self.biomes.guaranteed() {
            let b = self.biomes.get(biome);
            let fits = |x: i32, z: i32| {
                let c = self.column(x, z);
                let h = c.height - SEA_LEVEL;
                c.land == Land::Main && h >= b.min_height.max(1) && h <= b.max_height
            };
            let clear = |x: i32, z: i32| out.iter().all(|o| ((o.x - x).pow(2) + (o.z - z).pow(2)) as f32 > (o.radius + g.radius).powi(2));
            let mut best: Option<(usize, i32, i32)> = None;
            for k in 0..160u64 {
                let h = util::hash2(self.seed ^ 0xF0_06, k as i32, biome as i32);
                let a = util::unit(h) * std::f32::consts::TAU;
                let r = g.near + util::unit(h.rotate_left(19)) * (g.far - g.near).max(0.0);
                let (x, z) = (sx + (a.cos() * r) as i32, sz + (a.sin() * r) as i32);
                if floating::main_edge(self, x, z) > 0.7 || !fits(x, z) || !clear(x, z) {
                    continue;
                }
                // Count how much of the area is ground the biome can cover.
                let mut good = 0;
                for i in 0..16 {
                    let t = i as f32 / 16.0 * std::f32::consts::TAU;
                    for f in [0.4, 0.8] {
                        let (px, pz) = (x + (t.cos() * g.radius * f) as i32, z + (t.sin() * g.radius * f) as i32);
                        good += fits(px, pz) as usize;
                    }
                }
                if best.is_none_or(|(n, _, _)| good > n) {
                    best = Some((good, x, z));
                }
                if good >= 26 {
                    break;
                }
            }
            // Fall back to a spot `near` blocks out so the biome always exists.
            let (x, z) = best.map(|(_, x, z)| (x, z)).unwrap_or((sx + g.near as i32, sz));
            out.push(Zone { biome, x, z, radius: g.radius });
        }
        out
    }

    /// Surface height ignoring overhangs and caves.
    pub fn surface_height(&self, x: i32, z: i32) -> i32 {
        self.column(x, z).height
    }

    /// A spawn point in the swamp, on dry ground, near the origin.
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
                let col = self.terrain(x, z);
                if col.biome == swamp && col.height > SEA_LEVEL && col.climate.overhang < 0.01 {
                    return [x, col.height + 1, z];
                }
            }
        }
        [0, self.terrain(0, 0).height.max(SEA_LEVEL) + 2, 0]
    }

    /// Generate one chunk.
    pub fn generate(&self, pos: ChunkPos) -> Chunk {
        let origin = pos.origin();
        let (min_y, max_y) = (origin.y, origin.y + CHUNK_SIZE_I32 - 1);

        if max_y < WORLD_FLOOR {
            return Chunk::filled(BlockId::AIR);
        }
        let island_chunk = max_y >= SKY_MIN - 80 && min_y <= SKY_MAX;
        if min_y > SKY_MAX || min_y >= WORLD_CEILING {
            return Chunk::filled(BlockId::AIR);
        }

        let mut cols = [[Column::default(); CHUNK_SIZE]; CHUNK_SIZE];
        let (mut hi, mut lo) = (i32::MIN, i32::MAX);
        for (lz, row) in cols.iter_mut().enumerate() {
            for (lx, col) in row.iter_mut().enumerate() {
                *col = self.column(origin.x + lx as i32, origin.z + lz as i32);
                if col.land != Land::Void {
                    hi = hi.max(col.height.max(col.water));
                    lo = lo.min(col.bottom);
                }
            }
        }
        // Overhangs reach at most 24 above the heightmap; trees `trees::TALLEST`.
        let top_reach = hi.saturating_add(24.max(trees::TALLEST + 2));
        let mut dense = vec![BlockId::AIR; CHUNK_VOLUME];
        let mut any = false;

        // Trees on land just past the chunk's side may still reach over the void.
        if hi == i32::MIN
            && let Some(near) = self.land_near(origin.x, origin.z)
        {
            (hi, lo) = (near.height.max(near.water), near.height - 4);
        }
        if hi > i32::MIN && min_y <= top_reach && max_y > lo {
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

    /// Some land column within a few blocks around this chunk's columns, if any.
    fn land_near(&self, x0: i32, z0: i32) -> Option<Column> {
        const PAD: i32 = 8;
        let steps = [x0 - PAD, x0 + CHUNK_SIZE_I32 / 2, x0 + CHUNK_SIZE_I32 + PAD];
        let zsteps = [z0 - PAD, z0 + CHUNK_SIZE_I32 / 2, z0 + CHUNK_SIZE_I32 + PAD];
        zsteps.iter().flat_map(|&z| steps.iter().map(move |&x| (x, z))).map(|(x, z)| self.column(x, z)).find(|c| c.land != Land::Void)
    }

    fn solid_at(&self, x: i32, y: i32, z: i32, col: &Column) -> bool {
        if y <= col.bottom {
            return false;
        }
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
                if col.land == Land::Void {
                    continue;
                }
                let biome = self.biomes.get(col.biome);
                let (wx, wz) = (origin.x + lx as i32, origin.z + lz as i32);
                // Depth below the nearest air above, scanning down from just above the chunk.
                let mut depth = 0;
                for y in (min_y + CHUNK_SIZE_I32..min_y + CHUNK_SIZE_I32 + 4).rev() {
                    depth = if self.solid_at(wx, y, wz, col) { depth + 1 } else { 0 };
                }
                for ly in (0..CHUNK_SIZE).rev() {
                    let y = min_y + ly as i32;
                    let solid = y < WORLD_CEILING && self.solid_at(wx, y, wz, col);
                    let b = if !solid {
                        depth = 0;
                        if y <= col.water && y > col.bottom { ids.water } else { BlockId::AIR }
                    } else {
                        depth += 1;
                        let underwater = col.height < col.water;
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
                        if depth > 4 && y > col.bottom + 6 && !(underwater && y > col.height - 12) && caves.is_cave(lx, ly, lz) {
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
    fn high_sky_and_the_void_are_air() {
        let g = wg();
        assert_eq!(g.generate(ChunkPos::new(0, 40, 0)).is_uniform(), Some(BlockId::AIR));
        assert_eq!(g.generate(ChunkPos::new(0, 32, 0)).is_uniform(), Some(BlockId::AIR));
        assert_eq!(g.generate(ChunkPos::new(0, WORLD_FLOOR.div_euclid(32) - 1, 0)).is_uniform(), Some(BlockId::AIR));
        // Under the middle of the island, well below its underside: open air.
        let (cx, cz) = g.island_center();
        let below = (floating::CORE_DEPTH as i32 + 200).div_euclid(32);
        assert_eq!(g.generate(ChunkPos::new(cx.div_euclid(32), -below - 2, cz.div_euclid(32))).is_uniform(), Some(BlockId::AIR));
    }

    #[test]
    fn swamp_has_twigs_vines_and_pebbles() {
        let g = wg();
        let [sx, sy, sz] = g.find_spawn();
        let (mut twigs, mut vines, mut pebbles) = (0, 0, 0);
        for cz in -2..=2 {
            for cx in -2..=2 {
                for cy in -1..=0 {
                    let p = ChunkPos::new(sx.div_euclid(32) + cx, sy.div_euclid(32) + cy, sz.div_euclid(32) + cz);
                    for b in g.generate(p).to_dense() {
                        twigs += g.ids.twig.contains(&b) as u32;
                        vines += g.ids.vine.contains(&b) as u32;
                        pebbles += (b == g.ids.pebble) as u32;
                    }
                }
            }
        }
        assert!(twigs > 5 && vines > 10 && pebbles > 3, "twigs {twigs}, vines {vines}, pebbles {pebbles}");
    }

    #[test]
    fn main_island_is_two_to_three_miles_across() {
        let g = wg();
        let (cx, cz) = g.island_center();
        assert_eq!(g.column(cx, cz).land, Land::Main);
        let reach = |dx: i32, dz: i32| (1..).map(|i| i * 4).find(|d| floating::main_edge(&g, cx + dx * d, cz + dz * d) >= 1.0).unwrap();
        // A mile is 1609 blocks.
        for (w, axis) in [(reach(1, 0) + reach(-1, 0), "x"), (reach(0, 1) + reach(0, -1), "z")] {
            assert!((3219..=4828).contains(&w), "{w} blocks across along {axis}");
        }
        // Past the coast (and not on an outer island): nothing.
        let out = cx + reach(1, 0) + 16;
        let col = g.column(out, cz);
        assert!(col.land != Land::Main, "past the coast is still main island");
    }

    #[test]
    fn coast_holds_its_seas() {
        let g = wg();
        let (cx, cz) = g.island_center();
        for s in 0..360 {
            let a = s as f32 / 360.0 * std::f32::consts::TAU;
            for d in (1800..2400).step_by(4) {
                let (x, z) = (cx + (a.cos() * d as f32) as i32, cz + (a.sin() * d as f32) as i32);
                if (0.97..1.0).contains(&floating::main_edge(&g, x, z)) {
                    let h = g.column(x, z).height;
                    assert!(h >= SEA_LEVEL + 2, "coast at ({x}, {z}) is {h}, below the sea");
                }
            }
        }
    }

    #[test]
    fn underside_is_a_bowl_hung_with_drips() {
        let g = wg();
        let (cx, cz) = g.island_center();
        let middle = g.column(cx, cz).bottom;
        assert!((-460..=-280).contains(&middle), "middle underside at {middle}");
        let near_coast = g.column(cx + 1700, cz).bottom;
        assert!(near_coast > middle + 80, "underside rises towards the coast ({near_coast} vs {middle})");
        // Drips and lobes: the underside of a patch is far from flat.
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for z in (0..96).step_by(2) {
            for x in (0..96).step_by(2) {
                let b = g.column(cx + 300 + x, cz + z).bottom;
                lo = lo.min(b);
                hi = hi.max(b);
            }
        }
        assert!(hi - lo > 40, "underside spans {lo}..{hi}");
        // The generated blocks match the column: air under the underside, ground above it.
        let (x, z) = (cx + 300, cz);
        let col = g.column(x, z);
        let p = ChunkPos::new(x.div_euclid(32), col.bottom.div_euclid(32), z.div_euclid(32));
        let c = g.generate(p).to_dense();
        let (lx, lz, ly) = (x.rem_euclid(32) as usize, z.rem_euclid(32) as usize, col.bottom.rem_euclid(32) as usize);
        assert!(c[index(lx, ly, lz)].is_air(), "underside block is air");
        if ly + 1 < CHUNK_SIZE {
            assert!(!c[index(lx, ly + 1, lz)].is_air(), "block above the underside is solid");
        }
    }

    #[test]
    fn islands_float_in_the_distance() {
        let g = wg();
        let (cx, cz) = g.island_center();
        let mut outer = 0;
        for z in (-6000..6000).step_by(40) {
            for x in (-6000..6000).step_by(40) {
                let col = g.column(cx + x, cz + z);
                if col.land == Land::Outer {
                    outer += 1;
                    assert!(((x * x + z * z) as f32).sqrt() > MAIN_RADIUS * 1.1, "outer island at ({x}, {z}) overlaps the main one");
                    assert!(col.bottom < col.height && col.height < SKY_MIN, "outer island column {} .. {}", col.bottom, col.height);
                }
            }
        }
        assert!(outer > 200, "only {outer} sampled outer island columns");
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
        // Over the main island, expect seas, mountains and several biomes.
        let g = wg();
        let (cx, cz) = g.island_center();
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        let mut seen = std::collections::HashSet::new();
        for z in (-1600..1600).step_by(25) {
            for x in (-1600..1600).step_by(25) {
                if x * x + z * z > 1500 * 1500 {
                    continue;
                }
                let c = g.column(cx + x, cz + z);
                assert_eq!(c.land, Land::Main);
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
        let (cx, cz) = g.island_center();
        let mut air = 0;
        for x in -4..4 {
            for z in -4..4 {
                let c = g.generate(ChunkPos::new(cx.div_euclid(32) + x, -2, cz.div_euclid(32) + z)).to_dense();
                air += c.iter().filter(|b| b.is_air()).count();
            }
        }
        assert!(air > 1000, "some cave air between y -64 and -33 ({air})");
    }

    #[test]
    fn fungal_growth_on_every_main_island() {
        let tiles = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
        let reg = BlockRegistry::from_ron(include_str!("../../../assets/data/blocks.ron"), &tiles).unwrap();
        for seed in [1u64, 42, 777, 9001, 123456] {
            let g = WorldGen::new(seed, &reg, include_str!("../../../assets/data/biomes.ron")).unwrap();
            let z = g.zones().iter().find(|z| g.biomes.get(z.biome).name == "fungal_growth").expect("fungal zone");
            let r = z.radius as i32;
            let mut fungal = 0;
            for dz in (-r..=r).step_by(8) {
                for dx in (-r..=r).step_by(8) {
                    let c = g.column(z.x + dx, z.z + dz);
                    if c.land == Land::Main && g.biomes.get(c.biome).name == "fungal_growth" {
                        fungal += 1;
                    }
                }
            }
            assert!(fungal > 150, "seed {seed}: only {fungal} fungal samples at {:?}", (z.x, z.z));
        }
    }

    #[test]
    fn open_air_and_sky_islands_have_their_own_biomes() {
        let g = wg();
        let (cx, cz) = g.island_center();
        // Far past the coast: open air.
        let mut void = None;
        for d in (2400..6000).step_by(40) {
            if g.column(cx + d, cz).land == Land::Void {
                void = Some(cx + d);
                break;
            }
        }
        let vx = void.expect("void past the coast");
        assert_eq!(g.biome_at(vx, 100, cz).name, "open_sky");
        // High above the ground with no island there: open air too.
        let (x, _, z) = (cx, 0, cz);
        if g.island_top(x, z).is_none() {
            assert_eq!(g.biome_at(x, SKY_MAX, z).name, "open_sky");
        }
        // On a sky island: its sky biome.
        'found: for gz in (-2000..2000).step_by(16) {
            for gx in (-2000..2000).step_by(16) {
                if let Some(top) = g.island_top(cx + gx, cz + gz) {
                    assert_eq!(g.biome_at(cx + gx, top + 1, cz + gz).name, "skyreach");
                    break 'found;
                }
            }
        }
    }
}
