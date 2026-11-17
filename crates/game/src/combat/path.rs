//! Finding a way on foot: A* over the block grid, for foes chasing you.
//! Pure: it only asks `solid(x, y, z)` (unloaded = solid).
//!
//! A cell is a place to stand: open at feet and head, solid underneath.
//! Steps go to the 8 cells around, one block up (with room to jump), or
//! down a drop of up to `MAX_DROP` blocks. Diagonals need both sides open
//! so nobody cuts a corner through a wall.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use bevy::math::IVec3;

/// The furthest a walker will drop down in one step.
pub const MAX_DROP: i32 = 3;
/// Cells looked at before giving up (keeps a search well under a millisecond).
pub const BUDGET: usize = 1800;

/// Can a body two blocks tall stand with its feet in `c`?
pub fn standable(solid: &impl Fn(i32, i32, i32) -> bool, c: IVec3) -> bool {
    !solid(c.x, c.y, c.z) && !solid(c.x, c.y + 1, c.z) && solid(c.x, c.y - 1, c.z)
}

/// Where a body would land stepping sideways from `from` into column
/// (`x`, `z`): level, one up, or a short drop down.
fn step_into(solid: &impl Fn(i32, i32, i32) -> bool, from: IVec3, x: i32, z: i32) -> Option<IVec3> {
    let level = IVec3::new(x, from.y, z);
    if standable(solid, level) {
        return Some(level);
    }
    // One up: a block in the way, room above it, and headroom to jump from here.
    let up = level + IVec3::Y;
    if solid(x, from.y, z) && standable(solid, up) && !solid(from.x, from.y + 2, from.z) {
        return Some(up);
    }
    // Down: open at feet and head here, falling until something holds.
    if solid(x, from.y, z) || solid(x, from.y + 1, z) {
        return None;
    }
    (1..=MAX_DROP).map(|d| level - IVec3::Y * d).find(|c| solid(c.x, c.y - 1, c.z) && !solid(c.x, c.y, c.z)).filter(|c| standable(solid, *c))
}

fn open_column(solid: &impl Fn(i32, i32, i32) -> bool, x: i32, y: i32, z: i32) -> bool {
    !solid(x, y, z) && !solid(x, y + 1, z)
}

/// The cells reachable in one step from `c`, and what each step costs.
fn neighbours(solid: &impl Fn(i32, i32, i32) -> bool, c: IVec3) -> impl Iterator<Item = (IVec3, u32)> + '_ {
    const DIRS: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];
    DIRS.into_iter().filter_map(move |(dx, dz)| {
        let diagonal = dx != 0 && dz != 0;
        if diagonal && !(open_column(solid, c.x + dx, c.y, c.z) && open_column(solid, c.x, c.y, c.z + dz)) {
            return None;
        }
        let n = step_into(solid, c, c.x + dx, c.z + dz)?;
        // Diagonals only on the level (no corner-cutting jumps or drops).
        if diagonal && n.y != c.y {
            return None;
        }
        let cost = if diagonal { 14 } else { 10 } + if n.y > c.y { 8 } else { 0 } + (c.y - n.y).max(0) as u32 * 3;
        Some((n, cost))
    })
}

fn guess(a: IVec3, b: IVec3) -> u32 {
    let d = (a - b).abs();
    let (lo, hi) = (d.x.min(d.z), d.x.max(d.z));
    (lo * 14 + (hi - lo) * 10 + d.y * 10) as u32
}

/// The way from `start` to `goal` (both feet cells), first step first,
/// `start` left out. If the goal can't be reached within the budget (or
/// within `radius` of the start), the way to the reachable cell closest to
/// it, so the walker still gets as near as it can. Empty when already there
/// or boxed in.
pub fn find(solid: impl Fn(i32, i32, i32) -> bool, start: IVec3, goal: IVec3, radius: i32) -> Vec<IVec3> {
    if start == goal {
        return Vec::new();
    }
    let mut open = BinaryHeap::new();
    let mut came: HashMap<IVec3, (IVec3, u32)> = HashMap::new();
    came.insert(start, (start, 0));
    open.push(Reverse((guess(start, goal), 0u32, [start.x, start.y, start.z])));
    let mut best = (guess(start, goal), start);
    let mut looked = 0;
    while let Some(Reverse((_, g, c))) = open.pop() {
        let c = IVec3::from_array(c);
        if c == goal {
            best = (0, c);
            break;
        }
        if came.get(&c).is_some_and(|(_, known)| *known < g) {
            continue;
        }
        looked += 1;
        if looked > BUDGET {
            break;
        }
        for (n, cost) in neighbours(&solid, c) {
            if (n - start).abs().max_element() > radius {
                continue;
            }
            let ng = g + cost;
            if came.get(&n).is_none_or(|(_, known)| ng < *known) {
                came.insert(n, (c, ng));
                let h = guess(n, goal);
                if h < best.0 {
                    best = (h, n);
                }
                open.push(Reverse((ng + h, ng, [n.x, n.y, n.z])));
            }
        }
    }
    let mut way = Vec::new();
    let mut c = best.1;
    while c != start {
        way.push(c);
        c = came[&c].0;
    }
    way.reverse();
    way
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A flat floor at y = -1 plus these extra solid blocks.
    fn world(extra: &[(i32, i32, i32)]) -> impl Fn(i32, i32, i32) -> bool {
        let extra: HashSet<(i32, i32, i32)> = extra.iter().copied().collect();
        move |x, y, z| y < 0 || extra.contains(&(x, y, z))
    }

    #[test]
    fn walks_straight_on_open_ground() {
        let way = find(world(&[]), IVec3::ZERO, IVec3::new(6, 0, 0), 24);
        assert_eq!(way.len(), 6);
        assert_eq!(*way.last().unwrap(), IVec3::new(6, 0, 0));
    }

    #[test]
    fn goes_round_a_wall() {
        // A wall two high across x = 3, from z = -4 to 4.
        let wall: Vec<_> = (-4..=4).flat_map(|z| [(3, 0, z), (3, 1, z)]).collect();
        let solid = world(&wall);
        let way = find(&solid, IVec3::ZERO, IVec3::new(6, 0, 0), 24);
        assert_eq!(*way.last().unwrap(), IVec3::new(6, 0, 0));
        assert!(way.iter().all(|c| standable(&solid, *c)));
        assert!(way.iter().any(|c| c.z.abs() >= 5), "should go round the end of the wall");
    }

    #[test]
    fn climbs_a_step_and_drops_down() {
        // A one-block step at x = 2..4, then the floor again.
        let step: Vec<_> = (2..=4).flat_map(|x| (-2..=2).map(move |z| (x, 0, z))).collect();
        let way = find(world(&step), IVec3::ZERO, IVec3::new(7, 0, 0), 24);
        assert_eq!(*way.last().unwrap(), IVec3::new(7, 0, 0));
        assert!(way.iter().any(|c| c.y == 1), "goes over the step");
    }

    #[test]
    fn never_cuts_corners() {
        let solid = world(&[(1, 0, 0), (1, 1, 0)]);
        let way = find(&solid, IVec3::ZERO, IVec3::new(1, 0, 1), 24);
        // Can't go diagonally past the block at (1, 0, 0) in one step.
        assert_eq!(way.first(), Some(&IVec3::new(0, 0, 1)));
    }

    #[test]
    fn boxed_in_gets_as_close_as_it_can() {
        // The goal is sealed in a ring of walls three high.
        let mut ring = Vec::new();
        for x in 8..=12 {
            for z in -2..=2 {
                if x == 8 || x == 12 || z == -2 || z == 2 {
                    ring.extend([(x, 0, z), (x, 1, z), (x, 2, z)]);
                }
            }
        }
        let way = find(world(&ring), IVec3::ZERO, IVec3::new(10, 0, 0), 24);
        let end = *way.last().unwrap();
        assert_eq!(end, IVec3::new(7, 0, 0), "stops at the wall nearest the goal");
    }
}
