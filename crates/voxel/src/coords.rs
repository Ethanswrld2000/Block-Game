//! World, chunk and local block coordinates.

/// Blocks along each edge of a chunk. 32 lets binary meshing use one
/// 64-bit mask per column (32 blocks + a 1-block border each side).
pub const CHUNK_SIZE: usize = 32;
pub const CHUNK_SIZE_I32: i32 = CHUNK_SIZE as i32;

/// A block position in world space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// A chunk position: world block position divided by `CHUNK_SIZE`, rounded down.
/// Chunks stack vertically too, so there is no world height limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The block containing a point in world space (1 unit = 1 block).
    pub fn from_world(p: [f32; 3]) -> Self {
        Self::new(p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32)
    }

    pub fn chunk(self) -> ChunkPos {
        ChunkPos {
            x: self.x.div_euclid(CHUNK_SIZE_I32),
            y: self.y.div_euclid(CHUNK_SIZE_I32),
            z: self.z.div_euclid(CHUNK_SIZE_I32),
        }
    }

    /// Position inside its chunk, each axis in `0..CHUNK_SIZE`.
    pub fn local(self) -> [usize; 3] {
        [
            self.x.rem_euclid(CHUNK_SIZE_I32) as usize,
            self.y.rem_euclid(CHUNK_SIZE_I32) as usize,
            self.z.rem_euclid(CHUNK_SIZE_I32) as usize,
        ]
    }
}

impl ChunkPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// World position of this chunk's minimum corner block.
    pub fn origin(self) -> BlockPos {
        BlockPos::new(
            self.x * CHUNK_SIZE_I32,
            self.y * CHUNK_SIZE_I32,
            self.z * CHUNK_SIZE_I32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_positions_round_down() {
        let p = BlockPos::new(-1, -33, 31);
        assert_eq!(p.chunk(), ChunkPos::new(-1, -2, 0));
        assert_eq!(p.local(), [31, 31, 31]);
    }

    #[test]
    fn origin_round_trips() {
        let c = ChunkPos::new(-3, 2, 7);
        assert_eq!(c.origin().chunk(), c);
        assert_eq!(c.origin().local(), [0, 0, 0]);
    }

    #[test]
    fn from_world_floors() {
        assert_eq!(BlockPos::from_world([-0.5, 1.9, 0.0]), BlockPos::new(-1, 1, 0));
    }
}
