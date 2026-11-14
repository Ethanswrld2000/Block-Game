//! Chopping trees: each axe blow on a log shakes the tree. The terrain shader
//! rocks everything above the blow and near it, more the higher it is, and
//! leaves shiver (`shake_offset` in `voxel_vertex.wesl`).

use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::{Blocks, VoxelMaterial};

pub struct ChopPlugin;

impl Plugin for ChopPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Chop>().init_resource::<TreeShake>().add_systems(Update, (start_shake, push_to_material).chain().after(crate::interact::InteractSet));
    }
}

/// An axe blow landed on a log (sent at the bottom of the swing).
#[derive(Message, Debug, Clone, Copy)]
pub struct Chop(pub BlockPos);

/// The tree shaking now: where it was hit, and how hard (1 -> 0).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct TreeShake {
    pub at: Vec3,
    pub amount: f32,
}

/// Seconds a shake takes to die away.
const SHAKE_TIME: f32 = 0.7;

impl TreeShake {
    pub fn hit(&mut self, p: BlockPos) {
        self.at = Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5);
        self.amount = 1.0;
    }

    pub fn tick(&mut self, dt: f32) {
        self.amount = (self.amount - dt / SHAKE_TIME).max(0.0);
    }
}

fn start_shake(time: Res<Time>, mut chops: MessageReader<Chop>, mut shake: ResMut<TreeShake>) {
    shake.tick(time.delta_secs());
    if let Some(c) = chops.read().last() {
        shake.hit(c.0);
    }
}

fn push_to_material(shake: Res<TreeShake>, blocks: Res<Blocks>, mut materials: ResMut<Assets<VoxelMaterial>>, mut last: Local<Vec4>) {
    let want = shake.at.extend(shake.amount);
    if want == *last {
        return;
    }
    for h in [&blocks.material, &blocks.translucent_material].into_iter().flatten() {
        if let Some(mut m) = materials.get_mut(h) {
            m.extension.shake = want;
        }
    }
    *last = want;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blow_shakes_then_settles() {
        let mut s = TreeShake::default();
        s.hit(BlockPos::new(3, 10, -2));
        assert_eq!(s.amount, 1.0);
        assert_eq!(s.at, Vec3::new(3.5, 10.5, -1.5));
        s.tick(SHAKE_TIME * 0.5);
        assert!((s.amount - 0.5).abs() < 1e-4);
        s.tick(SHAKE_TIME);
        assert_eq!(s.amount, 0.0);
    }
}
