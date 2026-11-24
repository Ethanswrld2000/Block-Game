//! Pure rules for snow cover and puddles (see the module above).

use voxel::{BlockId, BlockPos, BlockRegistry, Shape};

/// The blocks cover is made of.
#[derive(Debug, Clone)]
pub struct Kinds {
    /// `snow_layer_1..=8`: index = level - 1.
    pub snow: [BlockId; 8],
    /// `puddle_1..=8`: index = level - 1.
    pub puddle: [BlockId; 8],
    pub water: Option<BlockId>,
    /// Small plants snow covers.
    pub buries: Vec<BlockId>,
}

impl Kinds {
    pub fn new(reg: &BlockRegistry, buries: &[String]) -> Self {
        Self {
            snow: std::array::from_fn(|i| reg.expect(&format!("snow_layer_{}", i + 1))),
            puddle: std::array::from_fn(|i| reg.expect(&format!("puddle_{}", i + 1))),
            water: reg.id("water"),
            buries: buries.iter().filter_map(|n| reg.id(n)).collect(),
        }
    }

    /// Snow level of a block, 0 if it isn't a snow layer.
    pub fn snow_level(&self, b: BlockId) -> u8 {
        self.snow.iter().position(|s| *s == b).map_or(0, |i| i as u8 + 1)
    }

    /// Water level of a block, 0 if it isn't a puddle.
    pub fn puddle_level(&self, b: BlockId) -> u8 {
        self.puddle.iter().position(|s| *s == b).map_or(0, |i| i as u8 + 1)
    }

    fn snow_at(&self, level: u8) -> BlockId {
        if level == 0 { BlockId::AIR } else { self.snow[level as usize - 1] }
    }

    fn puddle_at(&self, level: u8) -> BlockId {
        if level == 0 { BlockId::AIR } else { self.puddle[level as usize - 1] }
    }
}

/// Chances per look at a column (0..1).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sky {
    /// Add a snow layer.
    pub snow: f32,
    /// Add a level of water in a low spot.
    pub rain: f32,
    /// Melt a snow layer.
    pub melt: f32,
    /// Dry a puddle a level.
    pub dry: f32,
}

impl Sky {
    /// Nothing can change.
    pub fn is_still(&self) -> bool {
        self.snow <= 0.0 && self.rain <= 0.0 && self.melt <= 0.0 && self.dry <= 0.0
    }
}

/// xorshift, 0..1.
pub fn rand(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 / (1u64 << 24) as f32
}

const SIDES: [[i32; 3]; 4] = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];

fn add(p: BlockPos, d: [i32; 3]) -> BlockPos {
    BlockPos::new(p.x + d[0], p.y + d[1], p.z + d[2])
}

fn below(p: BlockPos) -> BlockPos {
    BlockPos::new(p.x, p.y - 1, p.z)
}

/// Snow can lie on it: a solid cube that isn't see-through (ground, leaves, logs).
fn holds_snow(reg: &BlockRegistry, b: BlockId) -> bool {
    let d = reg.get(b);
    d.solid && d.shape == Shape::Cube && !d.translucent
}

/// Water can stand on it: a wall block (ground, stone, planks; not leaves).
fn holds_water(reg: &BlockRegistry, b: BlockId) -> bool {
    reg.get(b).is_wall()
}

/// The cell where cover goes in column (x, z), searching down from `top` to
/// `bottom`: the snow layer, puddle or small plant on top of the column, or
/// the air over its top block. `None` if the top is something else (water,
/// a torch, a vine), nothing is found, or the column isn't loaded.
pub fn surface(get: &impl Fn(BlockPos) -> Option<BlockId>, reg: &BlockRegistry, kinds: &Kinds, x: i32, z: i32, top: i32, bottom: i32) -> Option<BlockPos> {
    for y in (bottom..=top).rev() {
        let p = BlockPos::new(x, y, z);
        let b = get(p)?;
        if b.is_air() {
            continue;
        }
        if kinds.snow_level(b) > 0 || kinds.puddle_level(b) > 0 || kinds.buries.contains(&b) {
            return Some(p);
        }
        let d = reg.get(b);
        return (d.shape == Shape::Cube && !d.translucent).then(|| BlockPos::new(x, y + 1, z));
    }
    None
}

/// What one look at the cover cell `at` changes, given the weather's chances.
pub fn step(get: &impl Fn(BlockPos) -> Option<BlockId>, reg: &BlockRegistry, kinds: &Kinds, at: BlockPos, sky: &Sky, seed: &mut u64) -> Vec<(BlockPos, BlockId)> {
    let Some(here) = get(at) else { return Vec::new() };
    let Some(under) = get(below(at)) else { return Vec::new() };
    let snow = kinds.snow_level(here);
    let water = kinds.puddle_level(here);

    // Snowfall: a layer goes on the lowest spot here or beside here.
    if sky.snow > 0.0 && rand(seed) < sky.snow {
        if water > 0 || !(here.is_air() || snow > 0 || kinds.buries.contains(&here)) || !holds_snow(reg, under) {
            return Vec::new();
        }
        let mut best = (at, snow);
        for d in SIDES {
            let n = add(at, d);
            let (Some(nb), Some(nu)) = (get(n), get(below(n))) else { continue };
            let level = kinds.snow_level(nb);
            let takes = nb.is_air() || level > 0 || kinds.buries.contains(&nb);
            if takes && holds_snow(reg, nu) && level < best.1 {
                best = (n, level);
            }
        }
        if best.1 >= 8 {
            return Vec::new();
        }
        return vec![(best.0, kinds.snow_at(best.1 + 1))];
    }
    if snow > 0 {
        if sky.melt > 0.0 && rand(seed) < sky.melt {
            return vec![(at, kinds.snow_at(snow - 1))];
        }
        return Vec::new();
    }

    // Rain: water gathers where walls hold it on two sides or more.
    let mut level = water;
    if sky.rain > 0.0 && (here.is_air() || water > 0) && holds_water(reg, under) && rand(seed) < sky.rain {
        let walls = SIDES.iter().filter(|d| get(add(at, **d)).is_some_and(|b| reg.get(b).is_wall())).count();
        if water > 0 || walls >= 2 {
            level = (level + 1).min(8);
        }
    }
    if level == 0 {
        return Vec::new();
    }
    if sky.dry > 0.0 && rand(seed) < sky.dry {
        level -= 1;
    }
    let mut out = Vec::new();
    // Water runs: into real water, off an edge, or to a lower neighbour (one move per look).
    let first = (rand(seed) * 4.0) as usize;
    for k in 0..4 {
        if level == 0 {
            break;
        }
        let n = add(at, SIDES[(first + k) % 4]);
        let Some(nb) = get(n) else { continue };
        if Some(nb) == kinds.water {
            level -= 1;
            break;
        }
        let nl = kinds.puddle_level(nb);
        if nb.is_air() {
            let Some(nu) = get(below(n)) else { continue };
            if !holds_water(reg, nu) && kinds.puddle_level(nu) == 0 {
                // Spills over the edge and is gone.
                level -= 1;
                break;
            }
            if holds_water(reg, nu) && level >= 2 {
                out.push((n, kinds.puddle_at(1)));
                level -= 1;
                break;
            }
        } else if nl > 0 && nl + 1 < level {
            out.push((n, kinds.puddle_at(nl + 1)));
            level -= 1;
            break;
        }
    }
    if level != water {
        out.push((at, kinds.puddle_at(level)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn registry() -> BlockRegistry {
        let tiles = voxel::TileSheet::from_ron(include_str!("../../../../assets/textures/blocks.ron")).unwrap();
        BlockRegistry::from_ron(include_str!("../../../../assets/data/blocks.ron"), &tiles).unwrap()
    }

    /// A world of `ground` at y < 0 with `holes` dug out of it (air), everything else air.
    struct Grid {
        cells: HashMap<BlockPos, BlockId>,
        ground: BlockId,
    }

    impl Grid {
        fn get(&self, p: BlockPos) -> Option<BlockId> {
            Some(*self.cells.get(&p).unwrap_or(if p.y < 0 { &self.ground } else { &BlockId::AIR }))
        }

        fn run(&mut self, reg: &BlockRegistry, kinds: &Kinds, sky: &Sky, cols: &[(i32, i32)], looks: usize) {
            let mut seed = 0x1234_5678_9ABC_DEF1;
            for i in 0..looks {
                let (x, z) = cols[i % cols.len()];
                let get = |p: BlockPos| self.get(p);
                let Some(at) = surface(&get, reg, kinds, x, z, 10, -10) else { continue };
                let changes = step(&get, reg, kinds, at, sky, &mut seed);
                for (p, b) in changes {
                    self.cells.insert(p, b);
                }
            }
        }
    }

    fn setup() -> (BlockRegistry, Kinds, Grid) {
        let reg = registry();
        let kinds = Kinds::new(&reg, &["tall_grass".to_string()]);
        let grid = Grid { cells: HashMap::new(), ground: reg.expect("dirt") };
        (reg, kinds, grid)
    }

    fn area(r: i32) -> Vec<(i32, i32)> {
        (-r..=r).flat_map(|x| (-r..=r).map(move |z| (x, z))).collect()
    }

    #[test]
    fn snow_piles_up_evenly_to_a_full_layer_and_melts_away() {
        let (reg, kinds, mut g) = setup();
        let cols = area(3);
        g.cells.insert(BlockPos::new(1, 0, 1), reg.expect("tall_grass"));
        let snowing = Sky { snow: 1.0, ..Default::default() };
        g.run(&reg, &kinds, &snowing, &cols, 49 * 3);
        // Spread out: no column is more than a couple of layers off another.
        let levels: Vec<u8> = cols.iter().map(|&(x, z)| kinds.snow_level(g.get(BlockPos::new(x, 0, z)).unwrap())).collect();
        let (lo, hi) = (*levels.iter().min().unwrap(), *levels.iter().max().unwrap());
        assert!(lo >= 1 && hi - lo <= 2, "levels {lo}..{hi}");
        // Grass got buried.
        assert!(kinds.snow_level(g.get(BlockPos::new(1, 0, 1)).unwrap()) > 0);
        g.run(&reg, &kinds, &snowing, &cols, 49 * 20);
        assert!(cols.iter().all(|&(x, z)| g.get(BlockPos::new(x, 0, z)) == Some(kinds.snow[7])), "tops out at 8");
        assert!(cols.iter().all(|&(x, z)| g.get(BlockPos::new(x, 1, z)) == Some(BlockId::AIR)), "never more than a block");
        let thaw = Sky { melt: 1.0, ..Default::default() };
        g.run(&reg, &kinds, &thaw, &cols, 49 * 8);
        assert!(cols.iter().all(|&(x, z)| g.get(BlockPos::new(x, 0, z)) == Some(BlockId::AIR)));
    }

    #[test]
    fn no_cover_on_water() {
        let (reg, kinds, mut g) = setup();
        g.cells.insert(BlockPos::new(0, -1, 0), reg.expect("water"));
        let get = |p: BlockPos| g.get(p);
        assert_eq!(surface(&get, &reg, &kinds, 0, 0, 10, -10), None);
    }

    #[test]
    fn rain_fills_a_pit_but_not_flat_ground_and_it_dries_after() {
        let (reg, kinds, mut g) = setup();
        // A 2x2 pit one deep.
        let pit = [(0, 0), (1, 0), (0, 1), (1, 1)];
        for (x, z) in pit {
            g.cells.insert(BlockPos::new(x, -1, z), BlockId::AIR);
        }
        let rain = Sky { rain: 1.0, ..Default::default() };
        let cols = area(4);
        g.run(&reg, &kinds, &rain, &cols, 81 * 6);
        for (x, z) in pit {
            assert!(kinds.puddle_level(g.get(BlockPos::new(x, -1, z)).unwrap()) >= 3, "pit fills");
        }
        let flat = cols.iter().filter(|&&(x, z)| g.get(BlockPos::new(x, 0, z)) != Some(BlockId::AIR)).count();
        assert_eq!(flat, 0, "flat ground stays dry");
        let dry = Sky { dry: 1.0, ..Default::default() };
        g.run(&reg, &kinds, &dry, &pit, 4 * 20);
        for (x, z) in pit {
            assert_eq!(g.get(BlockPos::new(x, -1, z)), Some(BlockId::AIR));
        }
    }

    #[test]
    fn water_runs_off_an_edge() {
        let (reg, kinds, mut g) = setup();
        // A trench (walls north and south) open at its east end onto a drop.
        for x in 0..3 {
            g.cells.insert(BlockPos::new(x, -1, 0), BlockId::AIR);
        }
        for y in -5..=-1 {
            g.cells.insert(BlockPos::new(3, y, 0), BlockId::AIR);
        }
        g.cells.insert(BlockPos::new(2, -1, 0), kinds.puddle[3]);
        let dry_still = Sky::default();
        g.run(&reg, &kinds, &dry_still, &[(2, 0)], 20);
        let total: u32 = (0..3).map(|x| kinds.puddle_level(g.get(BlockPos::new(x, -1, 0)).unwrap()) as u32).sum();
        assert!(total < 4, "some ran off ({total} left)");
    }
}
