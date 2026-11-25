//! Felling: cut through a tree's trunk and everything it held up topples over,
//! away from whoever cut it, and settles on the ground.
//!
//! Blocks say what they are to a tree with `tree: trunk|crown` in blocks.ron.
//! A trunk is every trunk block joined to the cut (corners count); it stands if
//! any of its blocks rests on solid ground that isn't part of a tree. A falling
//! trunk takes the crown blocks (leaves, caps) that are nearer to it than to
//! any other trunk, up to [`CROWN_REACH`] steps out.
//!
//! The lot turns a quarter turn about the cut, so standing logs lie down
//! (`<log>_x` / `<log>_z` in blocks.ron), then each column of it drops to the
//! lowest spot it can reach. Pure: works on a block lookup, so a server can
//! run it too.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::{Axis, BlockId, BlockPos, BlockRegistry, Shape, TreePart};

/// Trunks bigger than this don't fall (a log cabin, not a tree).
pub const MAX_TRUNK: usize = 4096;
/// How many steps from its trunk a crown block can be and still fall with it.
pub const CROWN_REACH: u32 = 8;
/// How far a column may be pushed up out of the ground it fell into.
const MAX_LIFT: i32 = 6;
/// How far a column may drop after falling over.
const MAX_DROP: i32 = 64;

/// One trunk falling over.
#[derive(Debug, Clone, PartialEq)]
pub struct Fall {
    /// The cut: the trunk turns about its middle.
    pub pivot: BlockPos,
    /// Which way it falls: one step along x or z.
    pub dir: [i32; 3],
    /// Every block taken away, where it stood.
    pub removed: Vec<(BlockPos, BlockId)>,
    /// Where they end up (turned on their side) once settled.
    pub placed: Vec<(BlockPos, BlockId)>,
    /// Blocks with nowhere to go (crushed into the ground), where they stood.
    pub lost: Vec<(BlockPos, BlockId)>,
    /// How far the whole lot can drop together after turning over (every
    /// column of logs drops at least this far), for drawing the fall.
    pub drop: i32,
}

/// The way a tree cut at `cut` falls when cut from `from` (x, z): straight
/// away from the cutter, along whichever of x or z is nearer.
pub fn away(cut: BlockPos, from: [f32; 2]) -> [i32; 3] {
    let dx = cut.x as f32 + 0.5 - from[0];
    let dz = cut.z as f32 + 0.5 - from[1];
    if dx.abs() >= dz.abs() {
        [if dx >= 0.0 { 1 } else { -1 }, 0, 0]
    } else {
        [0, 0, if dz >= 0.0 { 1 } else { -1 }]
    }
}

fn add(p: BlockPos, d: [i32; 3]) -> BlockPos {
    BlockPos::new(p.x + d[0], p.y + d[1], p.z + d[2])
}

const SIDES: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];

/// The 26 cells around `p`.
fn around(p: BlockPos) -> impl Iterator<Item = BlockPos> {
    (-1..=1).flat_map(move |dy| (-1..=1).flat_map(move |dz| (-1..=1).map(move |dx| [dx, dy, dz]))).filter(|d| *d != [0, 0, 0]).map(move |d| add(p, d))
}

/// The trunk `cut` was taken out of has come apart: everything next to it that
/// no longer stands, falling in `dir`. `get` reads the world after the cut
/// (`None` = not loaded; nothing falls that might reach there).
pub fn topple(reg: &BlockRegistry, get: impl Fn(BlockPos) -> Option<BlockId>, cut: BlockPos, dir: [i32; 3]) -> Vec<Fall> {
    let part = |p: BlockPos| get(p).and_then(|b| reg.get(b).tree);
    let mut seen = HashSet::new();
    let mut falls = Vec::new();
    for d in SIDES {
        let start = add(cut, d);
        if seen.contains(&start) || part(start) != Some(TreePart::Trunk) {
            continue;
        }
        let Some(trunk) = trunk_from(&part, start) else { continue };
        seen.extend(trunk.iter().copied());
        if stands(reg, &get, &trunk) {
            continue;
        }
        let crown = crown_of(&part, &trunk);
        falls.push(fall(reg, &get, cut, dir, &trunk, &crown));
    }
    falls
}

/// Every trunk block joined to `start`, or `None` if it runs into unloaded
/// ground or is too big to be a tree.
fn trunk_from(part: &impl Fn(BlockPos) -> Option<TreePart>, start: BlockPos) -> Option<HashSet<BlockPos>> {
    let mut set = HashSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(p) = queue.pop_front() {
        for n in around(p) {
            if set.contains(&n) {
                continue;
            }
            if part(n) == Some(TreePart::Trunk) {
                if set.len() >= MAX_TRUNK {
                    return None;
                }
                set.insert(n);
                queue.push_back(n);
            }
        }
    }
    Some(set)
}

/// Whether any block of the trunk rests on solid ground that isn't tree
/// (or on ground not loaded, to be safe).
fn stands(reg: &BlockRegistry, get: &impl Fn(BlockPos) -> Option<BlockId>, trunk: &HashSet<BlockPos>) -> bool {
    trunk.iter().any(|&p| {
        let below = add(p, [0, -1, 0]);
        if trunk.contains(&below) {
            return false;
        }
        match get(below) {
            None => true,
            Some(b) => {
                let def = reg.get(b);
                def.solid && def.tree.is_none()
            }
        }
    })
}

/// Steps through crown blocks from `seeds`, up to [`CROWN_REACH`], keeping to
/// `within` if given. Also returns the other trunk blocks the crown touches.
fn crown_steps(
    part: &impl Fn(BlockPos) -> Option<TreePart>,
    seeds: &HashSet<BlockPos>,
    within: Option<&HashMap<BlockPos, u32>>,
) -> (HashMap<BlockPos, u32>, HashSet<BlockPos>) {
    let mut dist: HashMap<BlockPos, u32> = HashMap::new();
    let mut others = HashSet::new();
    let mut queue: VecDeque<(BlockPos, u32)> = seeds.iter().map(|&p| (p, 0)).collect();
    while let Some((p, d)) = queue.pop_front() {
        for n in around(p) {
            if seeds.contains(&n) || dist.contains_key(&n) {
                continue;
            }
            match part(n) {
                Some(TreePart::Crown) if d < CROWN_REACH && within.is_none_or(|w| w.contains_key(&n)) => {
                    dist.insert(n, d + 1);
                    queue.push_back((n, d + 1));
                }
                Some(TreePart::Trunk) => {
                    others.insert(n);
                }
                _ => {}
            }
        }
    }
    (dist, others)
}

/// The crown blocks nearer to `trunk` than to any other trunk.
fn crown_of(part: &impl Fn(BlockPos) -> Option<TreePart>, trunk: &HashSet<BlockPos>) -> Vec<BlockPos> {
    let (own, others) = crown_steps(part, trunk, None);
    let (theirs, _) = if others.is_empty() { (HashMap::new(), HashSet::new()) } else { crown_steps(part, &others, Some(&own)) };
    let mut crown: Vec<BlockPos> = own.iter().filter(|(p, d)| theirs.get(p).is_none_or(|t| **d <= *t)).map(|(p, _)| *p).collect();
    crown.sort_by_key(|p| (p.x, p.y, p.z));
    crown
}

/// Turn an offset from the cut a quarter turn so that up points along `dir`.
pub fn turn(o: [i32; 3], dir: [i32; 3]) -> [i32; 3] {
    let [x, y, z] = o;
    match dir {
        [1, 0, 0] => [y, -x, z],
        [-1, 0, 0] => [-y, x, z],
        [0, 0, 1] => [x, -z, y],
        _ => [x, z, -y],
    }
}

/// The block a log becomes when turned so up points along `dir`: its grain
/// axis swaps with the fall's axis (`log` <-> `log_x` / `log_z`). Blocks
/// without lying versions stay as they are.
pub fn turned_block(reg: &BlockRegistry, b: BlockId, dir: [i32; 3]) -> BlockId {
    let def = reg.get(b);
    let along = if dir[0] != 0 { Axis::X } else { Axis::Z };
    let to = match def.axis {
        Axis::Y => along,
        a if a == along => Axis::Y,
        a => a,
    };
    if to == def.axis {
        return b;
    }
    let base = match def.axis {
        Axis::Y => def.name.as_str(),
        _ => def.name.strip_suffix("_x").or_else(|| def.name.strip_suffix("_z")).unwrap_or(&def.name),
    };
    let want = match to {
        Axis::X => format!("{base}_x"),
        Axis::Y => base.to_string(),
        Axis::Z => format!("{base}_z"),
    };
    reg.id(&want).unwrap_or(b)
}

/// A block on its way down: where it stood, where it lands turned over,
/// what it turns into, and whether it is trunk.
type Turned = (BlockPos, BlockPos, BlockId, bool);

fn fall(reg: &BlockRegistry, get: &impl Fn(BlockPos) -> Option<BlockId>, cut: BlockPos, dir: [i32; 3], trunk: &HashSet<BlockPos>, crown: &[BlockPos]) -> Fall {
    let mut cells: Vec<BlockPos> = trunk.iter().copied().chain(crown.iter().copied()).collect();
    cells.sort_by_key(|p| (p.x, p.y, p.z));
    let removed: Vec<(BlockPos, BlockId)> = cells.iter().map(|&p| (p, get(p).unwrap_or(BlockId::AIR))).collect();
    let gone: HashSet<BlockPos> = cells.iter().copied().collect();
    // Room for a falling block: air, a small plant or loose snow, or where the tree itself stood.
    let free = |p: BlockPos| {
        gone.contains(&p)
            || get(p).is_some_and(|b| {
                let d = reg.get(b);
                b.is_air() || d.shape.is_detail() || (d.shape == Shape::Layer && !d.solid)
            })
    };
    // Each block turned over, grouped by the column it lands in.
    let mut columns: BTreeMap<(i32, i32), Vec<Turned>> = BTreeMap::new();
    for &(p, b) in &removed {
        let o = turn([p.x - cut.x, p.y - cut.y, p.z - cut.z], dir);
        let to = add(cut, o);
        let is_trunk = reg.get(b).tree == Some(TreePart::Trunk);
        columns.entry((to.x, to.z)).or_default().push((p, to, turned_block(reg, b, dir), is_trunk));
    }
    let (mut placed, mut lost) = (Vec::new(), Vec::new());
    let mut drop = i32::MAX;
    for column in columns.values() {
        // Lift the column out of the ground until its logs (or, with none,
        // all of it) fit.
        let has_trunk = column.iter().any(|c| c.3);
        let fits = |k: i32| column.iter().filter(|c| c.3 || !has_trunk).all(|c| free(add(c.1, [0, k, 0])));
        let lift = (0..=MAX_LIFT).find(|&k| fits(k));
        let Some(k) = lift else {
            for &(p, to, b, _) in column {
                if free(to) {
                    placed.push((to, b));
                } else {
                    lost.push((p, removed_id(&removed, p)));
                }
            }
            if has_trunk {
                drop = 0;
            }
            continue;
        };
        // Then drop it as far as its logs can go (crown blocks too, with no
        // logs); leaves and caps caught under a log are crushed.
        let holds = |c: &&Turned| c.3 || !has_trunk;
        let mut d = 0;
        while d < MAX_DROP && column.iter().filter(holds).all(|c| free(add(c.1, [0, k - d - 1, 0]))) {
            d += 1;
        }
        for &(p, to, b, _) in column {
            let at = add(to, [0, k - d, 0]);
            if free(at) {
                placed.push((at, b));
            } else {
                lost.push((p, removed_id(&removed, p)));
            }
        }
        if has_trunk {
            drop = drop.min((d - k).max(0));
        }
    }
    if drop == i32::MAX {
        drop = 0;
    }
    Fall { pivot: cut, dir, removed, placed, lost, drop }
}

fn removed_id(removed: &[(BlockPos, BlockId)], p: BlockPos) -> BlockId {
    removed.iter().find(|r| r.0 == p).map_or(BlockId::AIR, |r| r.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::test_registry;

    /// A flat world: dirt up to y 0 from -20 to 20, air above, nothing loaded past it.
    struct World {
        blocks: HashMap<BlockPos, BlockId>,
    }

    impl World {
        fn flat(reg: &BlockRegistry) -> Self {
            let mut blocks = HashMap::new();
            for x in -20..=20 {
                for z in -20..=20 {
                    blocks.insert(BlockPos::new(x, 0, z), reg.expect("dirt"));
                }
            }
            World { blocks }
        }
        fn set(&mut self, p: BlockPos, b: BlockId) {
            self.blocks.insert(p, b);
        }
        fn get(&self, p: BlockPos) -> Option<BlockId> {
            if p.x.abs() > 20 || p.z.abs() > 20 || p.y < -10 || p.y > 60 {
                return None;
            }
            let ground = if p.y <= 0 { self.blocks[&BlockPos::new(0, 0, 0)] } else { BlockId::AIR };
            Some(self.blocks.get(&p).copied().unwrap_or(ground))
        }
        /// A trunk from y 1 to `height` at (x, z) with a ball of leaves round its top.
        fn tree(&mut self, reg: &BlockRegistry, x: i32, z: i32, height: i32) {
            for y in 1..=height {
                self.set(BlockPos::new(x, y, z), reg.expect("log"));
            }
            for dy in -1..=1 {
                for dz in -2..=2_i32 {
                    for dx in -2..=2_i32 {
                        let p = BlockPos::new(x + dx, height + dy, z + dz);
                        if (dx, dz) != (0, 0) && dx.abs() + dz.abs() <= 3 {
                            self.set(p, reg.expect("leaves"));
                        }
                    }
                }
            }
            self.set(BlockPos::new(x, height + 1, z), reg.expect("leaves"));
        }
    }

    #[test]
    fn falls_away_from_the_cutter() {
        let cut = BlockPos::new(0, 1, 0);
        assert_eq!(away(cut, [-3.0, 0.2]), [1, 0, 0]);
        assert_eq!(away(cut, [3.0, 0.2]), [-1, 0, 0]);
        assert_eq!(away(cut, [0.3, -4.0]), [0, 0, 1]);
        assert_eq!(away(cut, [0.6, 5.0]), [0, 0, -1]);
        for dir in [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]] {
            assert_eq!(turn([0, 1, 0], dir), dir, "up turns to the fall");
            assert_eq!(turn(dir, dir), [0, -1, 0], "the fall side turns down");
        }
    }

    #[test]
    fn logs_lie_down_along_the_fall_and_stand_back_up() {
        let reg = test_registry();
        let log = reg.expect("log");
        let (lx, lz) = (reg.expect("log_x"), reg.expect("log_z"));
        assert_eq!(turned_block(&reg, log, [1, 0, 0]), lx);
        assert_eq!(turned_block(&reg, log, [0, 0, -1]), lz);
        assert_eq!(turned_block(&reg, lx, [-1, 0, 0]), log);
        assert_eq!(turned_block(&reg, lx, [0, 0, 1]), lx, "turning about its own grain");
        assert_eq!(turned_block(&reg, reg.expect("mushroom_stem"), [1, 0, 0]), reg.expect("mushroom_stem"));
        // The lying log shows its rings on the ends.
        let (d, l) = (reg.get(lx), reg.get(log));
        assert_eq!(d.textures[0], l.textures[2]);
        assert_eq!(d.textures[2], l.textures[0]);
    }

    #[test]
    fn a_cut_tree_falls_over_and_lies_on_the_ground() {
        let reg = test_registry();
        let mut w = World::flat(&reg);
        w.tree(&reg, 0, 0, 6);
        let cut = BlockPos::new(0, 1, 0);
        w.set(cut, BlockId::AIR);
        let falls = topple(&reg, |p| w.get(p), cut, [1, 0, 0]);
        assert_eq!(falls.len(), 1);
        let f = &falls[0];
        assert_eq!(f.removed.iter().filter(|r| r.1 == reg.expect("log")).count(), 5);
        // The trunk lies along +x on the ground, as lying logs.
        let lx = reg.expect("log_x");
        let logs: Vec<BlockPos> = f.placed.iter().filter(|p| p.1 == lx).map(|p| p.0).collect();
        assert_eq!(logs, (1..=5).map(|x| BlockPos::new(x, 1, 0)).collect::<Vec<_>>());
        // Every leaf that stood went somewhere, nothing floats over air or sits in the ground.
        assert_eq!(f.placed.len() + f.lost.len(), f.removed.len());
        let at: HashSet<BlockPos> = f.placed.iter().map(|p| p.0).collect();
        for (p, _) in &f.placed {
            assert!(p.y >= 1, "{p:?} in the ground");
            let below = add(*p, [0, -1, 0]);
            assert!(below.y == 0 || at.contains(&below), "{p:?} floats");
            assert!(p.x >= -2, "fell the wrong way: {p:?}");
        }
    }

    #[test]
    fn a_tree_still_on_its_stump_stands() {
        let reg = test_registry();
        let mut w = World::flat(&reg);
        w.tree(&reg, 0, 0, 6);
        // Take a leaf, or a block from beside the trunk: nothing falls.
        w.set(BlockPos::new(1, 6, 0), BlockId::AIR);
        assert!(topple(&reg, |p| w.get(p), BlockPos::new(1, 6, 0), [1, 0, 0]).is_empty());
        // A 2x2 trunk cut through one column still stands on the others.
        let mut w = World::flat(&reg);
        for (x, z) in [(5, 5), (6, 5), (5, 6), (6, 6)] {
            for y in 1..=8 {
                w.set(BlockPos::new(x, y, z), reg.expect("redwood_log"));
            }
        }
        w.set(BlockPos::new(5, 1, 5), BlockId::AIR);
        assert!(topple(&reg, |p| w.get(p), BlockPos::new(5, 1, 5), [1, 0, 0]).is_empty());
    }

    #[test]
    fn a_cut_high_up_drops_the_top_and_keeps_the_stump() {
        let reg = test_registry();
        let mut w = World::flat(&reg);
        w.tree(&reg, 0, 0, 8);
        let cut = BlockPos::new(0, 3, 0);
        w.set(cut, BlockId::AIR);
        let falls = topple(&reg, |p| w.get(p), cut, [0, 0, -1]);
        assert_eq!(falls.len(), 1, "only the top falls");
        let f = &falls[0];
        assert!(f.removed.iter().all(|r| r.0.y > 3 || r.1 != reg.expect("log")), "the stump stays");
        assert!(f.drop >= 1, "the top drops to the ground after turning over");
        let lz = reg.expect("log_z");
        let logs: Vec<BlockPos> = f.placed.iter().filter(|p| p.1 == lz).map(|p| p.0).collect();
        assert_eq!(logs.len(), 5);
        assert!(logs.iter().all(|p| p.y == 1 && p.x == 0 && p.z < 0), "{logs:?}");
    }

    #[test]
    fn neighbours_keep_their_own_leaves() {
        let reg = test_registry();
        let mut w = World::flat(&reg);
        w.tree(&reg, 0, 0, 6);
        w.tree(&reg, 4, 0, 6);
        let cut = BlockPos::new(0, 1, 0);
        w.set(cut, BlockId::AIR);
        let f = &topple(&reg, |p| w.get(p), cut, [-1, 0, 0])[0];
        // Leaves right by the standing tree stay with it.
        assert!(f.removed.iter().all(|r| r.0.x <= 2), "{:?}", f.removed.iter().map(|r| r.0).filter(|p| p.x > 2).collect::<Vec<_>>());
        assert!(f.removed.iter().all(|r| r.0.x != 4));
    }

    #[test]
    fn nothing_lands_in_unloaded_ground() {
        let reg = test_registry();
        let mut w = World::flat(&reg);
        // A trunk running past the loaded edge.
        for y in 1..=6 {
            w.set(BlockPos::new(20, y, 0), reg.expect("log"));
        }
        w.set(BlockPos::new(20, 1, 0), BlockId::AIR);
        let falls = topple(&reg, |p| w.get(p), BlockPos::new(20, 1, 0), [1, 0, 0]);
        for f in &falls {
            assert!(f.placed.iter().all(|p| p.0.x <= 20), "placed into unloaded ground");
        }
    }
}
