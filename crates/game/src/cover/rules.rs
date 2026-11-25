//! Pure rules for snow cover and puddles (see the module above).

use std::collections::HashMap;
use voxel::{BlockId, BlockPos, BlockRegistry, Shape};

use super::flow::Waters;

/// The blocks cover is made of.
#[derive(Debug, Clone)]
pub struct Kinds {
    /// Per block id: snow level and what stands in the snow (air if nothing).
    snow: Vec<Option<(u8, BlockId)>>,
    /// (level, what stands in it) -> snow block.
    snowy: HashMap<(u8, BlockId), BlockId>,
    /// Water levels.
    pub water: Waters,
}

impl Kinds {
    pub fn new(reg: &BlockRegistry) -> Self {
        let mut snowy = HashMap::new();
        let snow = reg
            .iter()
            .map(|(id, d)| {
                (d.shape == Shape::Layer && !d.translucent).then(|| {
                    let s = (d.level, d.inside.unwrap_or(BlockId::AIR));
                    snowy.insert(s, id);
                    s
                })
            })
            .collect();
        Self { snow, snowy, water: Waters::new(reg) }
    }

    /// Snow level of a block, 0 if it isn't snow.
    pub fn snow_level(&self, b: BlockId) -> u8 {
        self.snow.get(b.0 as usize).copied().flatten().map_or(0, |s| s.0)
    }

    /// Water level of a block below a full block (a puddle), 0 otherwise.
    pub fn puddle_level(&self, b: BlockId) -> u8 {
        self.water.level(b) % 8
    }

    /// What stands in the cell apart from snow (air, a plant or a pebble).
    fn under_snow(&self, b: BlockId) -> BlockId {
        self.snow.get(b.0 as usize).copied().flatten().map_or(b, |s| s.1)
    }

    /// Snow can fill up around it (a plant or pebble).
    pub fn fills_around(&self, b: BlockId) -> bool {
        !b.is_air() && self.snowy.contains_key(&(1, b))
    }

    /// The block for `level` eighths of snow around `inside` (0 = just `inside`).
    pub fn snow_at(&self, level: u8, inside: BlockId) -> BlockId {
        if level == 0 {
            return inside;
        }
        self.snowy.get(&(level, inside)).or_else(|| self.snowy.get(&(level, BlockId::AIR))).copied().unwrap_or(BlockId::AIR)
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
    /// Dry a still puddle under four eighths a level.
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

/// The cell where cover goes in column (x, z), searching down from `top` to
/// `bottom`: the snow, puddle, plant or pebble on top of the column, or the
/// air over its top block. `None` if the top is something else (water, a
/// torch, a vine), nothing is found, or the column isn't loaded.
pub fn surface(get: &impl Fn(BlockPos) -> Option<BlockId>, reg: &BlockRegistry, kinds: &Kinds, x: i32, z: i32, top: i32, bottom: i32) -> Option<BlockPos> {
    for y in (bottom..=top).rev() {
        let p = BlockPos::new(x, y, z);
        let b = get(p)?;
        if b.is_air() {
            continue;
        }
        if kinds.snow_level(b) > 0 || kinds.puddle_level(b) > 0 || kinds.fills_around(b) {
            return Some(p);
        }
        let d = reg.get(b);
        return (d.shape == Shape::Cube && !d.translucent).then(|| BlockPos::new(x, y + 1, z));
    }
    None
}

/// What one look at the cover cell `at` changes, given the weather's chances.
/// `moving` says whether water there is still flowing (it doesn't dry then).
/// Water only gathers and dries here; `super::flow` moves it.
pub fn step(
    get: &impl Fn(BlockPos) -> Option<BlockId>,
    reg: &BlockRegistry,
    kinds: &Kinds,
    at: BlockPos,
    sky: &Sky,
    moving: &impl Fn(BlockPos) -> bool,
    seed: &mut u64,
) -> Vec<(BlockPos, BlockId)> {
    let Some(here) = get(at) else { return Vec::new() };
    let Some(under) = get(below(at)) else { return Vec::new() };
    let snow = kinds.snow_level(here);
    let water = kinds.puddle_level(here);
    // Snow goes in air, on snow, or round a plant or pebble.
    let takes_snow = |b: BlockId| b.is_air() || kinds.snow_level(b) > 0 || kinds.fills_around(b);

    // Snowfall: a layer goes on the lowest spot here or beside here.
    if sky.snow > 0.0 && rand(seed) < sky.snow {
        if !takes_snow(here) || !holds_snow(reg, under) {
            return Vec::new();
        }
        let mut best = (at, snow, here);
        for d in SIDES {
            let n = add(at, d);
            let (Some(nb), Some(nu)) = (get(n), get(below(n))) else { continue };
            let level = kinds.snow_level(nb);
            if takes_snow(nb) && holds_snow(reg, nu) && level < best.1 {
                best = (n, level, nb);
            }
        }
        if best.1 >= 8 {
            return Vec::new();
        }
        return vec![(best.0, kinds.snow_at(best.1 + 1, kinds.under_snow(best.2)))];
    }
    if snow > 0 {
        if sky.melt > 0.0 && rand(seed) < sky.melt {
            return vec![(at, kinds.snow_at(snow - 1, kinds.under_snow(here)))];
        }
        return Vec::new();
    }

    // Rain: water gathers where walls hold it on two sides or more (`flow` runs it from there).
    let mut level = water;
    if sky.rain > 0.0 && (here.is_air() || water > 0) && reg.get(under).is_wall() && rand(seed) < sky.rain {
        let walls = SIDES.iter().filter(|d| get(add(at, **d)).is_some_and(|b| reg.get(b).is_wall())).count();
        if water > 0 || walls >= 2 {
            level += 1;
        }
    }
    // A shallow puddle that has stopped running dries a little.
    if water > 0 && water < 4 && level == water && sky.dry > 0.0 && !moving(at) && rand(seed) < sky.dry {
        level -= 1;
    }
    if level == water {
        return Vec::new();
    }
    vec![(at, kinds.water.block(level))]
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
                let changes = step(&get, reg, kinds, at, sky, &|_| false, &mut seed);
                for (p, b) in changes {
                    self.cells.insert(p, b);
                }
            }
        }
    }

    fn setup() -> (BlockRegistry, Kinds, Grid) {
        let reg = registry();
        let kinds = Kinds::new(&reg);
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
        let snowing = Sky { snow: 1.0, ..Default::default() };
        g.run(&reg, &kinds, &snowing, &cols, 49 * 3);
        // Spread out: no column is more than a couple of layers off another.
        let levels: Vec<u8> = cols.iter().map(|&(x, z)| kinds.snow_level(g.get(BlockPos::new(x, 0, z)).unwrap())).collect();
        let (lo, hi) = (*levels.iter().min().unwrap(), *levels.iter().max().unwrap());
        assert!(lo >= 1 && hi - lo <= 2, "levels {lo}..{hi}");
        g.run(&reg, &kinds, &snowing, &cols, 49 * 20);
        assert!(cols.iter().all(|&(x, z)| kinds.snow_level(g.get(BlockPos::new(x, 0, z)).unwrap()) == 8), "tops out at 8");
        assert!(cols.iter().all(|&(x, z)| g.get(BlockPos::new(x, 1, z)) == Some(BlockId::AIR)), "never more than a block");
        let thaw = Sky { melt: 1.0, ..Default::default() };
        g.run(&reg, &kinds, &thaw, &cols, 49 * 8);
        assert!(cols.iter().all(|&(x, z)| g.get(BlockPos::new(x, 0, z)) == Some(BlockId::AIR)));
    }

    #[test]
    fn snow_fills_up_around_plants_and_pebbles_and_leaves_them_when_it_melts() {
        let (reg, kinds, mut g) = setup();
        let things = [(0, 0, "reeds"), (1, 0, "pebble"), (0, 1, "bog_bloom")];
        for (x, z, n) in things {
            g.cells.insert(BlockPos::new(x, 0, z), reg.expect(n));
        }
        let cols = area(2);
        g.run(&reg, &kinds, &Sky { snow: 1.0, ..Default::default() }, &cols, 25 * 4);
        for (x, z, n) in things {
            let b = g.get(BlockPos::new(x, 0, z)).unwrap();
            assert!(kinds.snow_level(b) > 0, "{n} snowed in");
            assert_eq!(reg.get(b).inside, Some(reg.expect(n)), "{n} still there");
        }
        g.run(&reg, &kinds, &Sky { melt: 1.0, ..Default::default() }, &cols, 25 * 8);
        for (x, z, n) in things {
            assert_eq!(g.get(BlockPos::new(x, 0, z)), Some(reg.expect(n)), "{n} back after the thaw");
        }
    }

    #[test]
    fn no_cover_on_water() {
        let (reg, kinds, mut g) = setup();
        g.cells.insert(BlockPos::new(0, -1, 0), reg.expect("water"));
        let get = |p: BlockPos| g.get(p);
        assert_eq!(surface(&get, &reg, &kinds, 0, 0, 10, -10), None);
    }

    #[test]
    fn rain_fills_a_pit_but_not_flat_ground() {
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
        g.run(&reg, &kinds, &rain, &pit, 4 * 20);
        assert_eq!(g.get(BlockPos::new(0, -1, 0)), Some(reg.expect("water")), "a full pit is a water block");
    }

    #[test]
    fn only_still_shallow_puddles_dry() {
        let (reg, kinds, mut g) = setup();
        let p = BlockPos::new(0, -1, 0);
        let dry = Sky { dry: 1.0, ..Default::default() };
        let mut seed = 99;
        for (level, moving, dries) in [(3, false, true), (4, false, false), (2, true, false)] {
            g.cells.insert(p, kinds.water.block(level));
            let get = |q: BlockPos| g.get(q);
            let out = step(&get, &reg, &kinds, p, &dry, &|_| moving, &mut seed);
            assert_eq!(!out.is_empty(), dries, "level {level}, moving {moving}");
            if dries {
                assert_eq!(out[0].1, kinds.water.block(level - 1));
            }
        }
    }
}
