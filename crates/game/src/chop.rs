//! Chopping trees: each axe blow on a log shakes the tree. The terrain shader
//! rocks everything above the blow and near it, more the higher it is, and
//! leaves shiver (`shake_offset` in `voxel_vertex.wesl`).
//!
//! Felling: once a trunk is cut through (`voxel::fell` decides what stands),
//! the tree and its crown come out of the world, swing over away from you as
//! one mesh, drop, and land as blocks: logs on their side, leaves and caps
//! round them. Twigs and vines that hung on it fall off as items.

use bevy::prelude::*;
use voxel::fell::{self, Fall};
use voxel::{BlockId, BlockPos, TreePart};

use crate::blocks::{Blocks, VoxelMaterial};
use crate::interact::Mined;
use crate::player::Player;
use crate::world::World;

pub struct ChopPlugin;

impl Plugin for ChopPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Chop>()
            .init_resource::<TreeShake>()
            .init_resource::<Fallout>()
            .add_systems(crate::menu::GameUpdate, (start_shake, push_to_material, fell_trees, swing_falling, send_fallout).chain().after(crate::interact::InteractSet));
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

/// Seconds a tree takes to swing over, before its height is added.
const SWING_BASE: f32 = 0.9;
/// Extra swing seconds per block of height (tall trees fall slower).
const SWING_PER_BLOCK: f32 = 0.06;
const SWING_MOST: f32 = 2.6;
/// Fall acceleration once it has swung over (blocks/s²).
const GRAVITY: f32 = 30.0;

/// A felled tree on its way down, drawn as one mesh until it lands.
#[derive(Component)]
struct Falling {
    fall: Fall,
    t: f32,
    swing: f32,
    /// When it was set down as blocks (the mesh stays until they show).
    landed_at: Option<f32>,
}

/// Longest the falling mesh waits for the landed blocks to be meshed (seconds).
const LAND_WAIT: f32 = 1.0;

/// Blocks felling took out of the world without placing back (crushed logs,
/// twigs and vines that lost their hold), sent as `Mined` so they drop.
#[derive(Resource, Default)]
struct Fallout(Vec<(BlockPos, BlockId)>);

/// Room for a landing block: air, a small plant or loose snow.
fn room_for(world: &World, blocks: &Blocks, p: BlockPos) -> bool {
    world.block(p).is_some_and(|b| {
        let d = blocks.registry.get(b);
        b.is_air() || d.shape.is_detail() || (d.shape == voxel::Shape::Layer && !d.solid)
    })
}

fn is_trunk(blocks: &Blocks, b: BlockId) -> bool {
    blocks.registry.get(b).tree == Some(TreePart::Trunk)
}

/// A log, standing or lying: worth dropping when it can't be placed.
fn is_log(blocks: &Blocks, b: BlockId) -> bool {
    is_trunk(blocks, b) || blocks.registry.get(b).axis != voxel::Axis::Y
}

/// A trunk block was cut: whatever no longer stands starts to fall away from the player.
#[allow(clippy::too_many_arguments)]
fn fell_trees(
    mut commands: Commands,
    mut mined: MessageReader<Mined>,
    mut world: ResMut<World>,
    blocks: Res<Blocks>,
    player: Res<Player>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut fallout: ResMut<Fallout>,
) {
    let cuts: Vec<BlockPos> = mined.read().filter(|m| !m.felled && is_trunk(&blocks, m.block)).map(|m| m.pos).collect();
    for cut in cuts {
        let dir = fell::away(cut, [player.pos.x, player.pos.z]);
        for fall in fell::topple(&blocks.registry, |p| world.block(p), cut, dir) {
            for &(p, _) in &fall.removed {
                world.set_block(p, BlockId::AIR);
            }
            for &(p, b) in &fall.lost {
                if is_log(&blocks, b) {
                    fallout.0.push((p, b));
                }
            }
            let around: Vec<BlockPos> = fall.removed.iter().flat_map(|r| crate::interact::neighbours(r.0)).collect();
            let budget = around.len() + 256;
            crate::interact::drop_unsupported(&mut world, &blocks, around, budget, |q, qb| fallout.0.push((q, qb)));
            let cells: Vec<([i32; 3], BlockId)> = fall.removed.iter().map(|&(p, b)| ([p.x - cut.x, p.y - cut.y, p.z - cut.z], b)).collect();
            let height = cells.iter().map(|c| c.0[1]).max().unwrap_or(0).max(1) as f32;
            let swing = (SWING_BASE + SWING_PER_BLOCK * height).min(SWING_MOST);
            match (crate::chunk_mesh::loose(&cells, &blocks.registry), &blocks.material) {
                (Some(mesh), Some(mat)) => {
                    let falling = Falling { fall, t: 0.0, swing, landed_at: None };
                    let tf = falling.transform();
                    commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d::<VoxelMaterial>(mat.clone()), tf, falling));
                }
                _ => land(&mut world, &blocks, &player, &fall, &mut fallout),
            }
        }
    }
}

impl Falling {
    /// Seconds from the end of the swing until it lands.
    fn drop_time(&self) -> f32 {
        (2.0 * self.fall.drop as f32 / GRAVITY).sqrt()
    }

    /// Where the mesh is now: turned about the middle of the cut (speeding up
    /// as it goes over), then dropped.
    fn transform(&self) -> Transform {
        let d = self.fall.dir;
        let dir = Vec3::new(d[0] as f32, 0.0, d[2] as f32);
        let k = (self.t / self.swing).clamp(0.0, 1.0);
        let turn = Quat::from_axis_angle(Vec3::Y.cross(dir).normalize(), std::f32::consts::FRAC_PI_2 * k * k);
        let after = (self.t - self.swing).max(0.0);
        let dropped = (0.5 * GRAVITY * after * after).min(self.fall.drop as f32);
        let p = self.fall.pivot;
        let centre = Vec3::new(p.x as f32, p.y as f32, p.z as f32) + Vec3::splat(0.5);
        Transform { translation: centre - turn * Vec3::splat(0.5) - Vec3::Y * dropped, rotation: turn, ..default() }
    }
}

fn swing_falling(
    mut commands: Commands,
    time: Res<Time>,
    mut trees: Query<(Entity, &mut Falling, &mut Transform)>,
    mut world: ResMut<World>,
    blocks: Res<Blocks>,
    player: Res<Player>,
    mut fallout: ResMut<Fallout>,
) {
    for (e, mut tree, mut tf) in &mut trees {
        tree.t += time.delta_secs();
        if tree.landed_at.is_none() {
            *tf = tree.transform();
            if tree.t >= tree.swing + tree.drop_time() {
                land(&mut world, &blocks, &player, &tree.fall, &mut fallout);
                tree.landed_at = Some(tree.t);
            }
            continue;
        }
        // Keep drawing the tree until the ground it landed on has been
        // remeshed in the background, so nothing blinks out (but not forever).
        let waited = tree.t - tree.landed_at.unwrap_or(tree.t);
        let pending = tree.fall.placed.iter().any(|(p, _)| world.needs_mesh(p.chunk()));
        if !pending || waited > LAND_WAIT {
            commands.entity(e).despawn();
        }
    }
}

/// Set the fallen tree down. A spot taken meanwhile (by you, or something
/// placed there) loses that block; a log lost that way still drops. The
/// chunks remesh in the background (no meshing on this frame).
fn land(world: &mut World, blocks: &Blocks, player: &Player, fall: &Fall, fallout: &mut Fallout) {
    for &(p, b) in &fall.placed {
        if room_for(world, blocks, p) && !player.overlaps(p) && world.set_block(p, b) {
            continue;
        }
        if is_log(blocks, b) {
            fallout.0.push((p, b));
        }
    }
}

fn send_fallout(mut fallout: ResMut<Fallout>, mut mined: MessageWriter<Mined>) {
    for (pos, block) in fallout.0.drain(..) {
        mined.write(Mined { pos, block, felled: true });
    }
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

    #[test]
    fn the_falling_mesh_ends_where_the_blocks_land() {
        // Each turned cell's middle must line up with `fell::turn`, then drop.
        for dir in [[1, 0, 0], [-1, 0, 0], [0, 0, 1], [0, 0, -1]] {
            let pivot = BlockPos::new(4, 10, -7);
            let fall = Fall { pivot, dir, removed: vec![], placed: vec![], lost: vec![], drop: 3 };
            let mut f = Falling { fall, t: 0.0, swing: 1.5, landed_at: None };
            let middle = |f: &Falling, o: [i32; 3]| f.transform().transform_point(Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32) + 0.5);
            let o = [1, 4, -2];
            let start = middle(&f, o);
            assert!((start - Vec3::new(5.5, 14.5, -8.5)).length() < 1e-4, "{start}");
            f.t = f.swing + f.drop_time();
            let t = fell::turn(o, dir);
            let want = Vec3::new((pivot.x + t[0]) as f32, (pivot.y + t[1] - 3) as f32, (pivot.z + t[2]) as f32) + 0.5;
            assert!((middle(&f, o) - want).length() < 1e-3, "{dir:?}: {} vs {want}", middle(&f, o));
        }
    }
}
