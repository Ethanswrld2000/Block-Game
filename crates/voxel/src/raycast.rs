//! Voxel ray traversal (Amanatides & Woo) for picking the block under the crosshair.

use crate::BlockPos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RayHit {
    pub block: BlockPos,
    /// The empty block in front of the hit face: where a placed block goes.
    pub before: BlockPos,
}

/// Step through blocks from `origin` along `dir` up to `max_dist`, returning the
/// first block for which `is_solid` is true.
pub fn raycast(origin: [f32; 3], dir: [f32; 3], max_dist: f32, mut is_solid: impl FnMut(BlockPos) -> bool) -> Option<RayHit> {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if len == 0.0 {
        return None;
    }
    let d = [dir[0] / len, dir[1] / len, dir[2] / len];
    let mut cell = [origin[0].floor() as i32, origin[1].floor() as i32, origin[2].floor() as i32];
    let mut step = [0i32; 3];
    let mut t_max = [f32::INFINITY; 3];
    let mut t_delta = [f32::INFINITY; 3];
    for i in 0..3 {
        if d[i] > 0.0 {
            step[i] = 1;
            t_max[i] = (cell[i] as f32 + 1.0 - origin[i]) / d[i];
            t_delta[i] = 1.0 / d[i];
        } else if d[i] < 0.0 {
            step[i] = -1;
            t_max[i] = (origin[i] - cell[i] as f32) / -d[i];
            t_delta[i] = -1.0 / d[i];
        }
    }
    let mut prev = cell;
    let mut t = 0.0;
    while t <= max_dist {
        let pos = BlockPos::new(cell[0], cell[1], cell[2]);
        if is_solid(pos) {
            return Some(RayHit { block: pos, before: BlockPos::new(prev[0], prev[1], prev[2]) });
        }
        prev = cell;
        let i = if t_max[0] < t_max[1] {
            if t_max[0] < t_max[2] { 0 } else { 2 }
        } else if t_max[1] < t_max[2] {
            1
        } else {
            2
        };
        cell[i] += step[i];
        t = t_max[i];
        t_max[i] += t_delta[i];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_floor_below() {
        let hit = raycast([0.5, 5.5, 0.5], [0.0, -1.0, 0.0], 10.0, |p| p.y <= 0).unwrap();
        assert_eq!(hit.block, BlockPos::new(0, 0, 0));
        assert_eq!(hit.before, BlockPos::new(0, 1, 0));
    }

    #[test]
    fn diagonal_hit_and_range_limit() {
        let wall = |p: BlockPos| p.x == 3;
        let hit = raycast([0.5, 0.5, 0.5], [1.0, 0.2, 0.1], 10.0, wall).unwrap();
        assert_eq!(hit.block.x, 3);
        assert_eq!(hit.before.x, 2);
        assert!(raycast([0.5, 0.5, 0.5], [1.0, 0.0, 0.0], 2.0, wall).is_none());
    }
}
