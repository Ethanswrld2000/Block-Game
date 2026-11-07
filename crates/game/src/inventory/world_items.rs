//! Items lying in the world: what didn't fit when you mined, what you threw
//! away. They fall to the ground, turn slowly, and jump into your pockets
//! when you walk over them with room to spare. Not saved yet: they vanish
//! when you quit (and after ten minutes).

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use voxel::BlockPos;

use super::grid::Stack;
use super::Inventory;
use crate::blocks::Blocks;
use crate::items::{ItemKind, Items};
use crate::player::{MoveMode, Player};
use crate::world::World;

const PICKUP_RANGE: f32 = 1.6;
const LIFETIME: f32 = 600.0;

pub struct WorldItemsPlugin;

impl Plugin for WorldItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Spill>().add_systems(Update, (spawn_spills, fall_and_spin, pick_up).chain());
    }
}

/// Put a stack on the ground (or in the air, to fall) at a point.
#[derive(Message, Debug, Clone, Copy)]
pub struct Spill {
    pub stack: Stack,
    pub at: Vec3,
}

#[derive(Component)]
struct WorldItem {
    stack: Stack,
    vel: f32,
    age: f32,
}

fn spawn_spills(
    mut commands: Commands,
    mut spills: MessageReader<Spill>,
    items: Res<Items>,
    blocks: Res<Blocks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cube: Local<Option<Handle<Mesh>>>,
) {
    for s in spills.read() {
        let mesh = cube.get_or_insert_with(|| meshes.add(Cuboid::new(0.28, 0.28, 0.28))).clone();
        let item = items.get(s.stack.item);
        // Block items take the colour of their block's texture.
        let color = match (&item.kind, &blocks.tile_colors) {
            (ItemKind::Block(b), Some(tc)) => {
                let c = tc[blocks.registry.get(*b).textures[2] as usize];
                Color::linear_rgb(c.x, c.y, c.z)
            }
            _ => item.color,
        };
        let mat = materials.add(StandardMaterial { base_color: color, perceptual_roughness: 0.9, ..default() });
        commands.spawn((
            WorldItem { stack: s.stack, vel: 0.0, age: 0.0 },
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(s.at),
            NotShadowCaster,
        ));
    }
}

fn fall_and_spin(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<World>,
    blocks: Res<Blocks>,
    mut q: Query<(Entity, &mut WorldItem, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (e, mut wi, mut tf) in &mut q {
        wi.age += dt;
        if wi.age > LIFETIME {
            commands.entity(e).despawn();
            continue;
        }
        tf.rotate_y(dt * 1.2);
        // Rest on the block below; unloaded ground counts as solid.
        let below = BlockPos::from_world((tf.translation - Vec3::Y * 0.15).to_array());
        let solid = world.block(below).is_none_or(|b| blocks.registry.get(b).solid);
        if solid {
            wi.vel = 0.0;
            tf.translation.y = tf.translation.y.max(below.y as f32 + 1.15);
        } else {
            wi.vel = (wi.vel - 20.0 * dt).max(-30.0);
            tf.translation.y += wi.vel * dt;
        }
    }
}

fn pick_up(
    mut commands: Commands,
    mode: Res<MoveMode>,
    player: Res<Player>,
    items: Res<Items>,
    mut inv: ResMut<Inventory>,
    mut q: Query<(Entity, &mut WorldItem, &Transform)>,
) {
    if *mode != MoveMode::Walk || !player.ready {
        return;
    }
    let body = player.pos + Vec3::Y * 0.9;
    for (e, mut wi, tf) in &mut q {
        // A moment's grace so things you throw don't jump straight back.
        if wi.age < 1.5 || tf.translation.distance(body) > PICKUP_RANGE {
            continue;
        }
        // Only mark the inventory changed if something actually went in.
        let left = inv.bypass_change_detection().add(&items, wi.stack);
        if left != Some(wi.stack) {
            inv.set_changed();
        }
        match left {
            None => commands.entity(e).despawn(),
            Some(left) => wi.stack = left,
        }
    }
}
