//! Water that flows (pure rules; `game::cover` runs them on the world).
//!
//! Every cell holds 0..=8 eighths of water: air (and a plant or pebble, which
//! water washes away) is 0, `puddle_<n>` is n, a `water` block is 8 (a full
//! source block). Water is never made or lost while it flows, only moved:
//! - if the cell below and all four sides are open air, the cell gives its
//!   water out to all five at once (most of it down);
//! - otherwise it falls first, as much as fits in the cell below;
//! - what is left spreads one eighth at a time to each side that has at least
//!   two eighths less, or to a side it can fall from (downhill), so puddles
//!   run to the low spots and settle when no side differs by more than one.
//!
//! Only cells near a change are looked at (`Flow::next`): a lake only starts
//! to leak when a block next to it changes. Water that falls out of the loaded
//! world (or below the world floor) is gone.

use std::collections::{HashMap, HashSet};
use voxel::{BlockId, BlockPos, BlockRegistry, Shape};

/// What a cell is to water.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// Not loaded.
    Unloaded,
    /// Water can't go in.
    Wall,
    /// Eighths of water in it (0 = open).
    Water(u8),
}

/// Block ids for each water level.
#[derive(Debug, Clone)]
pub struct Waters {
    /// Per block id: its water level, `None` for walls.
    level: Vec<Option<u8>>,
    /// Level -> block (0 air, 1..7 puddles, 8 water).
    block: [BlockId; 9],
}

impl Waters {
    pub fn new(reg: &BlockRegistry) -> Self {
        let water = reg.id("water");
        let puddle: [BlockId; 8] = std::array::from_fn(|i| reg.expect(&format!("puddle_{}", i + 1)));
        let level = reg
            .iter()
            .map(|(id, d)| {
                if id.is_air() || matches!(d.shape, Shape::Plant | Shape::Pebble) {
                    Some(0)
                } else if Some(id) == water {
                    Some(8)
                } else {
                    puddle.iter().position(|p| *p == id).map(|i| i as u8 + 1)
                }
            })
            .collect();
        let mut block = [BlockId::AIR; 9];
        block[1..].copy_from_slice(&puddle);
        if let Some(w) = water {
            block[8] = w;
        }
        Self { level, block }
    }

    /// The cell a block makes (`None` = not loaded).
    pub fn cell(&self, b: Option<BlockId>) -> Cell {
        match b {
            None => Cell::Unloaded,
            Some(b) => self.level.get(b.0 as usize).copied().flatten().map_or(Cell::Wall, Cell::Water),
        }
    }

    /// Eighths of water in a block (0 for anything else).
    pub fn level(&self, b: BlockId) -> u8 {
        self.level.get(b.0 as usize).copied().flatten().unwrap_or(0)
    }

    /// The block for a water level.
    pub fn block(&self, level: u8) -> BlockId {
        self.block[level.min(8) as usize]
    }
}

/// What one pass changed.
#[derive(Debug, Default)]
pub struct Flow {
    /// New water level of every cell that changed.
    pub levels: HashMap<BlockPos, u8>,
    /// Cells with water to look at next pass (moved, or next to a move).
    pub next: HashSet<BlockPos>,
    /// Eighths that fell out of the loaded world.
    pub lost: u32,
}

const SIDES: [[i32; 3]; 4] = [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]];
const AROUND: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];

fn add(p: BlockPos, d: [i32; 3]) -> BlockPos {
    BlockPos::new(p.x + d[0], p.y + d[1], p.z + d[2])
}

/// The cells to wake after a change at `p`: it and its six neighbours.
pub fn around(p: BlockPos) -> impl Iterator<Item = BlockPos> {
    std::iter::once(p).chain(AROUND.into_iter().map(move |d| add(p, d)))
}

/// One pass over `active` (at most `budget` cells; the rest wait for the next
/// pass). `get` reads the world; the pass sees its own changes as it goes, so
/// the total stays the same. `floor` is the lowest y water can be at.
pub fn pass(get: &impl Fn(BlockPos) -> Cell, active: &HashSet<BlockPos>, budget: usize, floor: i32, seed: &mut u64) -> Flow {
    let mut flow = Flow::default();
    // Lowest first, so a falling column moves together.
    let mut cells: Vec<BlockPos> = active.iter().copied().collect();
    cells.sort_unstable_by_key(|p| (p.y, p.x, p.z));
    if cells.len() > budget {
        flow.next.extend(cells.drain(budget..));
    }
    let mut moved: Vec<BlockPos> = Vec::new();
    for p in cells {
        let look = |q: BlockPos, levels: &HashMap<BlockPos, u8>| levels.get(&q).map_or_else(|| get(q), |l| Cell::Water(*l));
        let Cell::Water(start) = look(p, &flow.levels) else { continue };
        if start == 0 {
            continue;
        }
        let mut l = start;
        let down = BlockPos::new(p.x, p.y - 1, p.z);
        let below = look(down, &flow.levels);
        let sides = SIDES.map(|d| add(p, d));
        let set = |q: BlockPos, v: u8, levels: &mut HashMap<BlockPos, u8>, moved: &mut Vec<BlockPos>| {
            levels.insert(q, v);
            moved.push(q);
        };

        // Open all round: out to the five at once.
        if below == Cell::Water(0) && sides.iter().all(|q| look(*q, &flow.levels) == Cell::Water(0)) && p.y > floor {
            let (share, extra) = (l / 5, l % 5);
            if share > 0 {
                for q in sides {
                    set(q, share, &mut flow.levels, &mut moved);
                }
            }
            set(down, share + extra, &mut flow.levels, &mut moved);
            set(p, 0, &mut flow.levels, &mut moved);
            continue;
        }
        // Down first.
        match below {
            Cell::Water(b) if b < 8 && p.y > floor => {
                let m = l.min(8 - b);
                set(down, b + m, &mut flow.levels, &mut moved);
                l -= m;
            }
            Cell::Unloaded => {
                flow.lost += l as u32;
                l = 0;
            }
            _ if p.y <= floor => {
                flow.lost += l as u32;
                l = 0;
            }
            _ => {}
        }
        // Then an eighth to each side that's lower, or that it can fall from.
        let first = (super::rules::rand(seed) * 4.0) as usize % 4;
        for k in 0..4 {
            if l == 0 {
                break;
            }
            let q = sides[(first + k) % 4];
            let Cell::Water(n) = look(q, &flow.levels) else { continue };
            let falls = matches!(look(BlockPos::new(q.x, q.y - 1, q.z), &flow.levels), Cell::Water(b) if b < 8);
            if n + 1 < l || (n < l && falls) {
                set(q, n + 1, &mut flow.levels, &mut moved);
                l -= 1;
            }
        }
        if l != start {
            set(p, l, &mut flow.levels, &mut moved);
        }
    }
    for p in moved {
        for q in around(p) {
            let wet = flow.levels.get(&q).map_or_else(|| get(q), |l| Cell::Water(*l));
            if matches!(wet, Cell::Water(l) if l > 0) {
                flow.next.insert(q);
            }
        }
    }
    flow
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box world: `walls` everywhere listed, water levels in `water`, open air elsewhere
    /// inside x/z -8..8, y -8..8; outside is unloaded.
    struct Pool {
        walls: HashSet<BlockPos>,
        water: HashMap<BlockPos, u8>,
    }

    impl Pool {
        fn new() -> Self {
            Self { walls: HashSet::new(), water: HashMap::new() }
        }

        fn floor(&mut self, y: i32) {
            for x in -8..=8 {
                for z in -8..=8 {
                    self.walls.insert(BlockPos::new(x, y, z));
                }
            }
        }

        fn get(&self, p: BlockPos) -> Cell {
            if p.x.abs() > 8 || p.z.abs() > 8 || p.y.abs() > 8 {
                Cell::Unloaded
            } else if self.walls.contains(&p) {
                Cell::Wall
            } else {
                Cell::Water(self.water.get(&p).copied().unwrap_or(0))
            }
        }

        fn total(&self) -> u32 {
            self.water.values().map(|l| *l as u32).sum()
        }

        /// Runs passes until nothing moves (or `max`); returns how many it took.
        fn settle(&mut self, mut active: HashSet<BlockPos>, max: usize) -> usize {
            let mut seed = 0x2545_F491_4F6C_DD1D;
            for i in 0..max {
                if active.is_empty() {
                    return i;
                }
                let flow = pass(&|p| self.get(p), &active, 10_000, -100, &mut seed);
                for (p, l) in flow.levels {
                    if l == 0 {
                        self.water.remove(&p);
                    } else {
                        self.water.insert(p, l);
                    }
                }
                active = flow.next;
            }
            max
        }
    }

    fn level(p: &Pool, x: i32, y: i32, z: i32) -> u8 {
        p.water.get(&BlockPos::new(x, y, z)).copied().unwrap_or(0)
    }

    #[test]
    fn water_falls_then_spreads_and_keeps_its_volume() {
        let mut p = Pool::new();
        p.floor(-1);
        // A full block in the air beside a wall (so not open all round), over a floor.
        p.walls.insert(BlockPos::new(1, 2, 0));
        p.water.insert(BlockPos::new(0, 2, 0), 8);
        let start = p.total();
        let passes = p.settle(HashSet::from([BlockPos::new(0, 2, 0)]), 500);
        assert!(passes < 500, "settles");
        assert_eq!(p.total(), start, "no water made or lost");
        assert_eq!(level(&p, 0, 2, 0), 0, "fell");
        // Spread out over the floor: no two neighbours differ by more than one.
        for (q, l) in &p.water {
            assert_eq!(q.y, 0, "lies on the floor");
            for d in SIDES {
                let n = level(&p, q.x + d[0], 0, q.z + d[2]);
                assert!(l.abs_diff(n) <= 1, "{q:?} {l} vs {n}");
            }
        }
    }

    #[test]
    fn open_all_round_gives_its_water_out_at_once() {
        let mut p = Pool::new();
        p.water.insert(BlockPos::new(0, 4, 0), 8);
        let flow = pass(&|q| p.get(q), &HashSet::from([BlockPos::new(0, 4, 0)]), 100, -100, &mut 7);
        assert_eq!(flow.levels[&BlockPos::new(0, 4, 0)], 0);
        assert_eq!(flow.levels[&BlockPos::new(0, 3, 0)], 4, "most goes down");
        for d in SIDES {
            assert_eq!(flow.levels[&add(BlockPos::new(0, 4, 0), d)], 1);
        }
        assert_eq!(flow.levels.values().map(|l| *l as u32).sum::<u32>(), 8);
    }

    #[test]
    fn a_lake_leaks_into_a_new_hole_and_keeps_its_volume() {
        let mut p = Pool::new();
        p.floor(-3);
        // A lake two deep in x -8..-1, walled off at x 0, a dry pit beyond it.
        for x in -8..=8 {
            for z in -8..=8 {
                for y in -2..=-1 {
                    if x < 0 {
                        p.water.insert(BlockPos::new(x, y, z), 8);
                    } else {
                        p.walls.insert(BlockPos::new(x, y, z));
                    }
                }
            }
        }
        let start = p.total();
        // Dig a hole at x 0 next to the lake's lower layer.
        let hole = BlockPos::new(0, -2, 0);
        p.walls.remove(&hole);
        let passes = p.settle(around(hole).collect(), 2000);
        assert!(passes < 2000, "settles");
        assert_eq!(p.total(), start, "volume kept");
        assert!(level(&p, 0, -2, 0) >= 7, "the hole filled");
        assert!(p.water.values().any(|l| *l < 8), "the lake went down a little");
    }

    #[test]
    fn a_thin_puddle_runs_off_an_edge_downhill() {
        let mut p = Pool::new();
        p.floor(-3);
        // A ledge at y -1 for x <= 0; lower ground beyond.
        for x in -8..=0 {
            for z in -8..=8 {
                for y in -2..=-1 {
                    p.walls.insert(BlockPos::new(x, y, z));
                }
            }
        }
        p.water.insert(BlockPos::new(0, 0, 0), 1);
        p.settle(HashSet::from([BlockPos::new(0, 0, 0)]), 200);
        assert_eq!(level(&p, 0, 0, 0), 0, "ran off");
        assert_eq!(p.total(), 1);
        assert!(p.water.keys().all(|q| q.y == -2), "lies on the low ground");
    }

    #[test]
    fn water_falling_out_of_the_loaded_world_is_gone() {
        let mut p = Pool::new();
        p.water.insert(BlockPos::new(0, -8, 0), 3);
        p.walls.insert(BlockPos::new(1, -8, 0));
        let flow = pass(&|q| p.get(q), &HashSet::from([BlockPos::new(0, -8, 0)]), 100, -100, &mut 7);
        assert_eq!(flow.lost, 3);
        assert_eq!(flow.levels[&BlockPos::new(0, -8, 0)], 0);
    }

    #[test]
    fn levels_map_to_blocks_and_back() {
        let tiles = voxel::TileSheet::from_ron(include_str!("../../../../assets/textures/blocks.ron")).unwrap();
        let reg = BlockRegistry::from_ron(include_str!("../../../../assets/data/blocks.ron"), &tiles).unwrap();
        let w = Waters::new(&reg);
        assert_eq!(w.block(8), reg.expect("water"));
        assert_eq!(w.block(0), BlockId::AIR);
        for l in 0..=8 {
            assert_eq!(w.level(w.block(l)), l);
        }
        assert_eq!(w.cell(Some(reg.expect("tall_grass"))), Cell::Water(0), "washed away");
        assert_eq!(w.cell(Some(reg.expect("dirt"))), Cell::Wall);
        assert_eq!(w.cell(Some(reg.expect("snow_layer_2"))), Cell::Wall);
        assert_eq!(w.cell(None), Cell::Unloaded);
    }
}
