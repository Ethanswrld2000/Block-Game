//! Trees. A tree whose canopy crosses a chunk border is drawn partly by each
//! chunk from the same seed, so the halves line up.

use voxel::chunk::index;
use voxel::{BlockId, CHUNK_SIZE_I32, ChunkPos};

use crate::util::{hash2, hash3, unit};
use crate::{TreeKind, WorldGen};

/// How far a canopy can reach sideways from its trunk (redwoods: 6, plus one).
const REACH: i32 = 7;
/// Radius of a swamp tree's canopy (vines hang one block outside it, hence `REACH`).
const SWAMP_CANOPY: i32 = 3;
/// Tallest a tree reaches above its ground block (redwood: trunk up to 43, tip 3 more).
pub(crate) const TALLEST: i32 = 48;
/// Highest chance any biome uses, for a cheap first filter.
const MAX_CHANCE: f32 = 0.05;
/// Highest bush chance any biome uses.
const MAX_BUSH: f32 = 0.05;

/// Writes blocks into a chunk's dense array, clipped to the chunk.
pub(crate) struct Canvas<'a> {
    pub origin: [i32; 3],
    pub dense: &'a mut [BlockId],
}

impl Canvas<'_> {
    fn idx(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (lx, ly, lz) = (x - self.origin[0], y - self.origin[1], z - self.origin[2]);
        let r = 0..CHUNK_SIZE_I32;
        (r.contains(&lx) && r.contains(&ly) && r.contains(&lz))
            .then(|| index(lx as usize, ly as usize, lz as usize))
    }

    /// Set a block, always.
    pub fn put(&mut self, x: i32, y: i32, z: i32, b: BlockId) {
        if let Some(i) = self.idx(x, y, z) {
            self.dense[i] = b;
        }
    }

    /// Is this spot air? `None` when it's outside the chunk.
    pub fn is_air(&self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.idx(x, y, z).map(|i| self.dense[i].is_air())
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
    let mut canvas = Canvas {
        origin: [o.x, o.y, o.z],
        dense,
    };
    // No trees through the middle of a village.
    let villages = wg.villages_in(
        o.x - REACH,
        o.z - REACH,
        o.x + CHUNK_SIZE_I32 + REACH,
        o.z + CHUNK_SIZE_I32 + REACH,
    );
    // Nor through a dungeon's gatehouse or its path.
    let gates: Vec<_> = wg
        .dungeons_in(
            o.x - REACH,
            o.z - REACH,
            o.x + CHUNK_SIZE_I32 + REACH,
            o.z + CHUNK_SIZE_I32 + REACH,
        )
        .iter()
        .map(|d| d.gate)
        .collect();
    for tz in o.z - REACH..o.z + CHUNK_SIZE_I32 + REACH {
        for tx in o.x - REACH..o.x + CHUNK_SIZE_I32 + REACH {
            let h = hash2(wg.seed ^ 0x7EE5, tx, tz);
            let roll = unit(h);
            let hb = hash2(wg.seed ^ 0xB0_5E, tx, tz);
            let bush = unit(hb);
            if roll >= MAX_CHANCE && bush >= MAX_BUSH {
                continue;
            }
            let col = wg.column(tx, tz);
            let biome = wg.biomes.get(col.biome);
            if col.land == crate::Land::Void
                || col.climate.overhang > 0.02
                || crate::structures::in_village(&villages, tx, tz)
                || gates
                    .iter()
                    .any(|g| (g[0] - tx).abs() <= 11 && (g[2] - tz).abs() <= 11)
            {
                continue;
            }
            let ground = col.height;
            let min_ground = if biome.trees == TreeKind::Swamp {
                col.water
            } else {
                col.water.saturating_add(1)
            };
            if ground < min_ground || ground + TALLEST < o.y || ground > o.y + CHUNK_SIZE_I32 {
                continue;
            }
            if roll < biome.tree_chance {
                draw(wg, &mut canvas, biome.trees, tx, ground, tz, h);
            } else if bush < biome.bushes {
                draw_bush(wg, &mut canvas, biome.trees, tx, ground, tz, hb);
            }
        }
    }
}

/// Draw a tree whose trunk starts on top of the block at (x, ground, z).
pub(crate) fn draw(
    wg: &WorldGen,
    c: &mut Canvas,
    kind: TreeKind,
    x: i32,
    ground: i32,
    z: i32,
    h: u64,
) {
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
            for dy in -2..=1 {
                let r = if dy == 1 || dy == -2 {
                    SWAMP_CANOPY - 1
                } else {
                    SWAMP_CANOPY
                };
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
            // Swamp trees are the most overgrown.
            twigs(wg, c, x, ground + 2, top - 3, z, h, 0.14);
            canopy_vines(wg, c, x, top - 1, z, SWAMP_CANOPY, h, 0.55, 5);
            trunk_vines(wg, c, x, top - 2, ground + 1, z, h, 0.5);
        }
        TreeKind::Oak => {
            let top = ground + 7 + var % 4;
            for y in ground + 1..=top {
                c.put(x, y, z, ids.log);
            }
            // A full, rounded crown.
            for dy in -4..=1 {
                let r = match dy {
                    -4 | 0 => 2,
                    1 => 1,
                    _ => 3,
                };
                disc(c, x, top + dy, z, r, ids.leaves);
            }
            disc(c, x, top + 2, z, 1, ids.leaves);
            tufts(c, x, top - 2, z, 4, h, ids.leaves);
            tufts(c, x, top - 4, z, 3, h.rotate_left(7), ids.leaves);
            twigs(wg, c, x, ground + 2, top - 4, z, h, 0.12);
            canopy_vines(wg, c, x, top - 2, z, 2, h, 0.3, 3);
            trunk_vines(wg, c, x, top - 4, ground + 1, z, h, 0.15);
        }
        TreeKind::Pine => {
            let top = ground + 11 + var % 6;
            for y in ground + 1..=top {
                c.put(x, y, z, ids.log);
            }
            // Thick tiers widening towards the bottom.
            for y in (ground + 3..=top + 1).rev() {
                let down = top + 1 - y;
                let base = (1 + down / 3).min(3);
                let r = if down % 3 == 0 { base - 1 } else { base };
                disc(c, x, y, z, r, ids.leaves);
            }
            c.fill(x, top + 2, z, ids.leaves);
            twigs(wg, c, x, ground + 1, ground + 5, z, h, 0.15);
            trunk_vines(wg, c, x, ground + 4, ground + 1, z, h, 0.1);
        }
        TreeKind::Redwood => {
            // A 2x2 trunk (x..x+1, z..z+1), rooted a little into the ground,
            // bare for over half its height, then a deep cone of thick tiers.
            let top = ground + 30 + var % 14;
            for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                for y in ground - 2..=top - 3 {
                    c.put(x + dx, y, z + dz, ids.redwood_log);
                }
            }
            // Buttress roots flaring out at the base.
            for (i, (dx, dz)) in [
                (-1, 0),
                (2, 1),
                (0, 2),
                (1, -1),
                (-1, 1),
                (2, 0),
                (1, 2),
                (0, -1),
            ]
            .into_iter()
            .enumerate()
            {
                let hgt = ((h >> (i * 2 + 8)) & 3) as i32;
                for y in ground - 1..=ground + hgt {
                    c.put(x + dx, y, z + dz, ids.redwood_log);
                }
            }
            let start = ground + (top - ground) * 11 / 20;
            for y in start..=top + 2 {
                let t = (y - start) as f32 / (top + 2 - start) as f32;
                let tier = if (top - y).rem_euclid(4) == 0 {
                    1.0
                } else {
                    0.0
                };
                let r = (6.0 * (1.0 - t).powf(0.8) + tier).clamp(1.0, 6.0);
                disc2(c, x, y, z, r, ids.redwood_leaves);
            }
            c.fill(x, top + 3, z, ids.redwood_leaves);
            c.fill(x + 1, top + 3, z + 1, ids.redwood_leaves);
        }
        TreeKind::Mushroom => {
            // A pale stem under a broad cap with a rim hanging one block down.
            // Mostly mid-sized, a few giants.
            let giant = var % 5 == 0;
            let top = ground + if giant { 11 + var % 5 } else { 4 + var % 6 };
            let r = if giant { 3 } else { 2 };
            for y in ground + 1..=top {
                c.put(x, y, z, ids.mushroom_stem);
            }
            disc(c, x, top + 1, z, r, ids.mushroom_cap);
            disc(c, x, top + 2, z, r - 1, ids.mushroom_cap);
            for dz in -r..=r {
                for dx in -r..=r {
                    let d = dx * dx + dz * dz;
                    if d <= r * r + 1 && d > (r - 1) * (r - 1) + 1 {
                        c.fill(x + dx, top, z + dz, ids.mushroom_cap);
                    }
                }
            }
        }
    }
}

/// Sides in the order of `Ids::vine` / `Ids::twig`, with the step from the
/// trunk or canopy edge out to the cell that clings to it.
const SIDES: [(usize, i32, i32); 4] = [
    // A cell south of the trunk clings to its north side, and so on.
    (0, 0, 1),  // north-attached, sits at +Z
    (1, 0, -1), // south-attached, sits at -Z
    (2, -1, 0), // east-attached, sits at -X
    (3, 1, 0),  // west-attached, sits at +X
];

/// Twigs poking out of the trunk between `y0` and `y1`: right click one for a stick.
#[allow(clippy::too_many_arguments)]
fn twigs(wg: &WorldGen, c: &mut Canvas, x: i32, y0: i32, y1: i32, z: i32, h: u64, chance: f32) {
    for y in y0..=y1 {
        for (i, dx, dz) in SIDES {
            if unit(hash3(h ^ 0x7_1C5, x + dx, y, z + dz)) < chance {
                c.fill(x + dx, y, z + dz, wg.ids.twig[i]);
            }
        }
    }
}

/// Vines hanging down the outside of the canopy, from the leaf ring of radius
/// `r` at height `y`.
#[allow(clippy::too_many_arguments)]
fn canopy_vines(
    wg: &WorldGen,
    c: &mut Canvas,
    x: i32,
    y: i32,
    z: i32,
    r: i32,
    h: u64,
    chance: f32,
    longest: i32,
) {
    for (i, dx, dz) in SIDES {
        for along in -1..=1 {
            let k = hash3(h ^ 0x51_4E, i as i32, along, 0);
            if unit(k) >= chance {
                continue;
            }
            // The outside cell beside the canopy edge.
            let (vx, vz) = if dx != 0 {
                (x + dx * (r + 1), z + along)
            } else {
                (x + along, z + dz * (r + 1))
            };
            hang(c, vx, y, vz, 1 + (k >> 8) as i32 % longest, wg.ids.vine[i]);
        }
    }
}

/// Vines climbing down the trunk from `top` to `bottom`.
#[allow(clippy::too_many_arguments)]
fn trunk_vines(
    wg: &WorldGen,
    c: &mut Canvas,
    x: i32,
    top: i32,
    bottom: i32,
    z: i32,
    h: u64,
    chance: f32,
) {
    for (i, dx, dz) in SIDES {
        let k = hash3(h ^ 0xB1_7E, i as i32, 1, 0);
        if unit(k) < chance {
            let len = 2 + (k >> 8) as i32 % (top - bottom + 1).max(1);
            hang(c, x + dx, top, z + dz, len, wg.ids.vine[i]);
        }
    }
}

/// A strand of vine from `y` down, stopping at anything in the way.
fn hang(c: &mut Canvas, x: i32, y: i32, z: i32, len: i32, vine: BlockId) {
    for j in 0..len {
        if c.is_air(x, y - j, z) == Some(false) {
            break;
        }
        c.fill(x, y - j, z, vine);
    }
}

/// Extra leaf clumps sticking out of the canopy so it reads less like a box.
fn tufts(c: &mut Canvas, x: i32, y: i32, z: i32, r: i32, h: u64, leaves: BlockId) {
    for (i, dx, dz) in SIDES {
        let k = hash3(h ^ 0x70_F7, i as i32, 2, 0);
        if unit(k) < 0.6 {
            let along = (k >> 8) as i32 % 3 - 1;
            let (tx, tz) = if dx != 0 {
                (x + dx * r, z + along)
            } else {
                (x + along, z + dz * r)
            };
            c.fill(tx, y, tz, leaves);
            if (k >> 12) & 1 == 1 {
                c.fill(tx, y - 1, tz, leaves);
            }
        }
    }
}

/// A disc of radius `r` around the middle of a 2x2 trunk at (x..x+1, z..z+1).
fn disc2(c: &mut Canvas, x: i32, y: i32, z: i32, r: f32, b: BlockId) {
    let n = r.ceil() as i32 + 1;
    for dz in -n..=n {
        for dx in -n..=n {
            let (fx, fz) = (dx as f32 - 0.5, dz as f32 - 0.5);
            if fx * fx + fz * fz <= r * r + 0.5 {
                c.fill(x + dx, y, z + dz, b);
            }
        }
    }
}

/// A leafy bush sitting on the ground: a mound of the biome's tree leaves.
fn draw_bush(wg: &WorldGen, c: &mut Canvas, kind: TreeKind, x: i32, ground: i32, z: i32, h: u64) {
    let ids = wg.ids;
    let leaves = match kind {
        TreeKind::Swamp => ids.swamp_leaves,
        TreeKind::Redwood => ids.redwood_leaves,
        _ => ids.leaves,
    };
    let big = h & 1 == 1;
    c.fill(x, ground + 1, z, leaves);
    if big {
        c.fill(x, ground + 2, z, leaves);
    }
    let r = if big { 2 } else { 1 };
    for dz in -r..=r {
        for dx in -r..=r {
            let d = dx * dx + dz * dz;
            if d == 0 || d > r * r + 1 {
                continue;
            }
            let k = unit(hash3(h, dx, 0, dz));
            if k < if d <= 2 { 0.8 } else { 0.45 } {
                c.fill(x + dx, ground + 1, z + dz, leaves);
                if big && d <= 2 && k < 0.35 {
                    c.fill(x + dx, ground + 2, z + dz, leaves);
                }
            }
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
