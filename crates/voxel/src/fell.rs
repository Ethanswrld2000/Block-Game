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
/// How high a felled crown may lie in any column, above the lowest block that
/// landed there: the rest of a big crown breaks up as it hits the ground.
pub const CROWN_HEAP: i32 = 2;

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

/// The crown blocks that belong to `trunk`: those it reaches through other
/// crown blocks (within [`CROWN_REACH`]) before any other tree's trunk does.
/// Every trunk block standing near it counts as another tree (the stump
/// below a cut too), so touching crowns split between their own trunks.
fn crown_of(part: &impl Fn(BlockPos) -> Option<TreePart>, trunk: &HashSet<BlockPos>) -> Vec<BlockPos> {
    let r = CROWN_REACH as i32;
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    for p in trunk {
        for (k, v) in [p.x, p.y, p.z].into_iter().enumerate() {
            lo[k] = lo[k].min(v - r);
            hi[k] = hi[k].max(v + r);
        }
    }
    // Ours first, so a crown block as near to both stays with the tree that fell.
    let mut owner: HashMap<BlockPos, bool> = trunk.iter().map(|&p| (p, true)).collect();
    let mut queue: VecDeque<(BlockPos, u32, bool)> = trunk.iter().map(|&p| (p, 0, true)).collect();
    for y in lo[1]..=hi[1] {
        for z in lo[2]..=hi[2] {
            for x in lo[0]..=hi[0] {
                let p = BlockPos::new(x, y, z);
                if !trunk.contains(&p) && part(p) == Some(TreePart::Trunk) {
                    owner.insert(p, false);
                    queue.push_back((p, 0, false));
                }
            }
        }
    }
    let mut crown = Vec::new();
    while let Some((p, d, ours)) = queue.pop_front() {
        if d >= CROWN_REACH {
            continue;
        }
        for n in around(p) {
            if owner.contains_key(&n) || part(n) != Some(TreePart::Crown) {
                continue;
            }
            owner.insert(n, ours);
            if ours {
                crown.push(n);
            }
            queue.push_back((n, d + 1, ours));
        }
    }
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

/// A block on its way down.
struct Turned {
    /// Where it stood, and what it was.
    from: BlockPos,
    was: BlockId,
    /// Where it lands turned over (before settling), and what it turns into.
    to: BlockPos,
    block: BlockId,
    trunk: bool,
}

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
        columns.entry((to.x, to.z)).or_default().push(Turned { from: p, to, block: turned_block(reg, b, dir), was: b, trunk: is_trunk });
    }
    let (mut placed, mut lost) = (Vec::new(), Vec::new());
    let mut drop = i32::MAX;
    for column in columns.values() {
        let has_trunk = column.iter().any(|c| c.trunk);
        let all_free = |cells: &[&Turned], dy: i32| cells.iter().all(|c| free(add(c.to, [0, dy, 0])));
        // What decides how far the column moves: its logs, pushed up out of
        // the ground until they fit; with no logs, the crown blocks that fit
        // where they land (crown is never pushed up: what's in the ground goes).
        let (lift, movers): (Option<i32>, Vec<&Turned>) = if has_trunk {
            let logs: Vec<&Turned> = column.iter().filter(|c| c.trunk).collect();
            ((0..=MAX_LIFT).find(|&k| all_free(&logs, k)), logs)
        } else {
            (Some(0), column.iter().filter(|c| free(c.to)).collect())
        };
        let shift = match lift {
            Some(k) if !movers.is_empty() => {
                // Then drop as far as those can go; whatever else ends up in
                // the way (leaves under a log, another tree) is crushed.
                let mut d = 0;
                while d < MAX_DROP && all_free(&movers, k - d - 1) {
                    d += 1;
                }
                if has_trunk {
                    drop = drop.min((d - k).max(0));
                }
                k - d
            }
            _ => {
                if has_trunk {
                    drop = 0;
                }
                0
            }
        };
        let mut kept: Vec<(BlockPos, &Turned)> = Vec::new();
        for c in column {
            let at = add(c.to, [0, shift, 0]);
            if free(at) {
                kept.push((at, c));
            } else {
                lost.push((c.from, c.was));
            }
        }
        // A felled crown spreads on the ground rather than standing in a heap.
        let base = kept.iter().map(|k| k.0.y).min().unwrap_or(0);
        for (at, c) in kept {
            if !c.trunk && at.y > base + CROWN_HEAP {
                lost.push((c.from, c.was));
            } else {
                placed.push((at, c.block));
            }
        }
    }
    if drop == i32::MAX {
        drop = 0;
    }
    Fall { pivot: cut, dir, removed, placed, lost, drop }
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
    fn a_big_crown_lies_low_instead_of_piling_up() {
        let reg = test_registry();
        let mut w = World::flat(&reg);
        // A tall trunk under a big ball of leaves.
        for y in 1..=10 {
            w.set(BlockPos::new(0, y, 0), reg.expect("log"));
        }
        for y in 7..=15 {
            for z in -4..=4_i32 {
                for x in -4..=4_i32 {
                    let p = BlockPos::new(x, y, z);
                    if (x, z) != (0, 0) || y > 10 {
                        w.set(p, reg.expect("leaves"));
                    }
                }
            }
        }
        let cut = BlockPos::new(0, 1, 0);
        w.set(cut, BlockId::AIR);
        let f = &topple(&reg, |p| w.get(p), cut, [1, 0, 0])[0];
        let leaves = reg.expect("leaves");
        assert!(f.placed.iter().filter(|p| p.1 == leaves).all(|p| p.0.y <= 1 + CROWN_HEAP), "a heap of leaves");
        assert!(f.placed.iter().any(|p| p.1 == leaves), "some leaves still lie there");
        // Nothing lands twice in one spot.
        let at: HashSet<BlockPos> = f.placed.iter().map(|p| p.0).collect();
        assert_eq!(at.len(), f.placed.len());
    }

    #[test]
    fn touching_crowns_split_between_their_trees() {
        let reg = test_registry();
        let mut w = World::flat(&reg);
        // Two trees 3 apart whose leaves run together; the right one is taller.
        w.tree(&reg, 0, 0, 6);
        w.tree(&reg, 3, 0, 8);
        for y in 5..=7 {
            for x in -2..=5 {
                w.set(BlockPos::new(x, y, 0), reg.expect("leaves"));
            }
        }
        for y in 1..=6 {
            w.set(BlockPos::new(0, y, 0), reg.expect("log"));
        }
        let cut = BlockPos::new(3, 1, 0);
        w.set(cut, BlockId::AIR);
        let falls = topple(&reg, |p| w.get(p), cut, [1, 0, 0]);
        assert_eq!(falls.len(), 1, "only the cut tree falls");
        let f = &falls[0];
        assert!(f.removed.iter().all(|r| r.0.x != 0), "took the other tree's trunk");
        // Leaves right beside the standing trunk stay on it.
        for y in 5..=6 {
            assert!(f.removed.iter().all(|r| r.0 != BlockPos::new(1, y, 0) && r.0 != BlockPos::new(-1, y, 0)), "y {y}");
        }
    }

    #[test]
    fn lying_logs_never_fall_again() {
        let reg = test_registry();
        let mut w = World::flat(&reg);
        // A felled log over a gap: cutting it drops nothing more.
        for x in 1..=6 {
            w.set(BlockPos::new(x, 3, 0), reg.expect("log_x"));
        }
        w.set(BlockPos::new(1, 2, 0), reg.expect("stone"));
        w.set(BlockPos::new(1, 1, 0), reg.expect("stone"));
        let cut = BlockPos::new(2, 3, 0);
        w.set(cut, BlockId::AIR);
        assert!(topple(&reg, |p| w.get(p), cut, [1, 0, 0]).is_empty());
        assert!(reg.get(reg.expect("log_z")).tree.is_none());
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
