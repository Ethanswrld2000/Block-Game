//! Rooms: is a spot closed in on every side by walls, a floor and a roof?
//!
//! Starting at one cell, the check spreads through every open cell it can
//! reach (sideways, up and down), like water filling a room. If walls stop it
//! on every side before it has filled [`MAX_ROOM`] cells, the cells it filled
//! are a room. If it gets out (an open sky, a gap, a hole of leaves or water)
//! it soon passes the limit and the spot is outside.
//!
//! Pure: the caller says what each cell is, so this works on any world.

use std::collections::{HashSet, VecDeque};

use crate::BlockPos;

/// The most open cells a room can hold (about 16 x 16 x 16). Bigger counts as outside.
pub const MAX_ROOM: usize = 4096;

/// What a cell is to the room check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// Stops the fill: part of a room's shell.
    Wall,
    /// The fill passes through it (air, and holes such as leaves, plants, water, torches).
    Open,
    /// Not loaded yet: the room can't be known.
    Unknown,
}

/// A closed room.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Room {
    /// Every open cell inside.
    pub cells: HashSet<BlockPos>,
    /// Every wall cell that touches the inside.
    pub shell: HashSet<BlockPos>,
}

impl Room {
    pub fn contains(&self, p: BlockPos) -> bool {
        self.cells.contains(&p)
    }
}

/// Why a spot is not in a room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotRoom {
    /// The fill got out, or the space is bigger than [`MAX_ROOM`].
    Open,
    /// The start cell is itself a wall.
    InWall,
    /// The fill reached a part of the world that isn't loaded.
    Unknown,
}

const STEPS: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];

/// The room around `start`, if it is closed in.
pub fn find_room(start: BlockPos, cell: impl Fn(BlockPos) -> Cell) -> Result<Room, NotRoom> {
    match cell(start) {
        Cell::Wall => return Err(NotRoom::InWall),
        Cell::Unknown => return Err(NotRoom::Unknown),
        Cell::Open => {}
    }
    let mut room = Room::default();
    room.cells.insert(start);
    let mut queue = VecDeque::from([start]);
    while let Some(p) = queue.pop_front() {
        for [dx, dy, dz] in STEPS {
            let n = BlockPos::new(p.x + dx, p.y + dy, p.z + dz);
            if room.cells.contains(&n) || room.shell.contains(&n) {
                continue;
            }
            match cell(n) {
                Cell::Wall => {
                    room.shell.insert(n);
                }
                Cell::Unknown => return Err(NotRoom::Unknown),
                Cell::Open => {
                    if room.cells.len() >= MAX_ROOM {
                        return Err(NotRoom::Open);
                    }
                    room.cells.insert(n);
                    queue.push_back(n);
                }
            }
        }
    }
    Ok(room)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hollow box of walls from `min` to `max` (inclusive), open inside; outside is open air.
    fn boxed(min: [i32; 3], max: [i32; 3], holes: &[[i32; 3]]) -> impl Fn(BlockPos) -> Cell {
        let holes: Vec<[i32; 3]> = holes.to_vec();
        move |p: BlockPos| {
            let [x, y, z] = [p.x, p.y, p.z];
            let inside = (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&y) && (min[2]..=max[2]).contains(&z);
            let edge = x == min[0] || x == max[0] || y == min[1] || y == max[1] || z == min[2] || z == max[2];
            if inside && edge && !holes.contains(&[x, y, z]) {
                Cell::Wall
            } else {
                Cell::Open
            }
        }
    }

    #[test]
    fn closed_box_is_a_room() {
        let room = find_room(BlockPos::new(2, 1, 2), boxed([0, 0, 0], [4, 4, 4], &[])).unwrap();
        assert_eq!(room.cells.len(), 27);
        // 5x5x5 box minus its 12 edges' and 8 corners' cells that touch nothing inside: faces only.
        assert_eq!(room.shell.len(), 6 * 9);
        assert!(room.contains(BlockPos::new(1, 3, 3)));
        assert!(!room.contains(BlockPos::new(0, 1, 1)));
    }

    #[test]
    fn a_gap_lets_it_out() {
        let roofless = find_room(BlockPos::new(2, 1, 2), boxed([0, 0, 0], [4, 4, 4], &[[2, 4, 2]]));
        assert_eq!(roofless, Err(NotRoom::Open));
        let doorway = find_room(BlockPos::new(2, 1, 2), boxed([0, 0, 0], [4, 4, 4], &[[0, 1, 2]]));
        assert_eq!(doorway, Err(NotRoom::Open));
    }

    #[test]
    fn a_corner_gap_does_not_leak() {
        // Corner cells touch no inside cell, so a missing corner is not a way out.
        let room = find_room(BlockPos::new(2, 1, 2), boxed([0, 0, 0], [4, 4, 4], &[[0, 0, 0]]));
        assert!(room.is_ok());
    }

    #[test]
    fn too_big_is_outside() {
        // 20x20x20 inside: 8000 cells, over the limit.
        let big = find_room(BlockPos::new(5, 5, 5), boxed([0, 0, 0], [21, 21, 21], &[]));
        assert_eq!(big, Err(NotRoom::Open));
        // 16x16x16 inside is exactly the limit and still a room.
        let fits = find_room(BlockPos::new(5, 5, 5), boxed([0, 0, 0], [17, 17, 17], &[]));
        assert_eq!(fits.unwrap().cells.len(), MAX_ROOM);
    }

    #[test]
    fn unloaded_world_is_unknown() {
        let walls = boxed([0, 0, 0], [4, 4, 4], &[[4, 2, 2]]);
        let cell = |p: BlockPos| if p.x > 5 { Cell::Unknown } else { walls(p) };
        assert_eq!(find_room(BlockPos::new(2, 1, 2), cell), Err(NotRoom::Unknown));
    }

    #[test]
    fn starting_in_a_wall() {
        assert_eq!(find_room(BlockPos::new(0, 1, 1), boxed([0, 0, 0], [4, 4, 4], &[])), Err(NotRoom::InWall));
    }

    #[test]
    fn two_rooms_side_by_side_are_separate() {
        // Two 3x3x3 rooms sharing the wall at x = 4.
        let a = boxed([0, 0, 0], [4, 4, 4], &[]);
        let b = boxed([4, 0, 0], [8, 4, 4], &[]);
        let cell = |p: BlockPos| if a(p) == Cell::Wall || b(p) == Cell::Wall { Cell::Wall } else { Cell::Open };
        let left = find_room(BlockPos::new(2, 1, 2), cell).unwrap();
        assert!(!left.contains(BlockPos::new(6, 1, 2)));
        assert!(find_room(BlockPos::new(6, 1, 2), cell).unwrap().contains(BlockPos::new(5, 3, 3)));
    }
}
