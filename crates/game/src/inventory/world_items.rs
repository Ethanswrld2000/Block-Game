//! Items lying in the world: what didn't fit when you mined, what you threw
//! away. They fall to the ground, turn slowly, and jump into your pockets
//! when you walk over them with room to spare. They vanish after ten minutes
//! lying there; until then they are saved to `ground_items.ron` (every 30 s
//! and on exit) with how long they have lain.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use voxel::BlockPos;

use super::grid::Stack;
use super::Inventory;
use crate::blocks::Blocks;
use crate::item_art::ItemArt;
use crate::items::{ItemKind, Items};
use crate::player::{MoveMode, Player};
use crate::save::{read_ron, screenshot_run, write_ron, SaveDir};
use crate::world::World;

const PICKUP_RANGE: f32 = 1.6;
const LIFETIME: f32 = 600.0;

pub struct WorldItemsPlugin;

impl Plugin for WorldItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Spill>()
            .add_systems(crate::menu::GamePostStartup, load)
            .add_systems(crate::menu::GameUpdate, (spawn_spills, fall_and_spin, pick_up, save_now_and_then).chain())
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
    }
}

/// Put a stack on the ground (or in the air, to fall) at a point.
#[derive(Message, Debug, Clone, Copy)]
pub struct Spill {
    pub stack: Stack,
    pub at: Vec3,
}

/// Items back from the save, spawned with `spawn_spills` along with the spills.
#[derive(Resource, Default)]
struct Restored(Vec<(Stack, Vec3, f32)>);

const FILE: &str = "ground_items.ron";

/// One stack on the ground as saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SavedItem {
    /// Item name in items.ron.
    item: String,
    count: u16,
    at: [f32; 3],
    /// Seconds it has lain there.
    age: f32,
}

fn load(mut commands: Commands, dir: Res<SaveDir>, items: Res<Items>) {
    if screenshot_run() {
        return;
    }
    let list: Vec<SavedItem> = read_ron(&dir, FILE).unwrap_or_default();
    let restored = list
        .into_iter()
        .filter_map(|s| match items.id(&s.item) {
            Some(id) => Some((Stack::new(id, s.count), Vec3::from_array(s.at), s.age)),
            None => {
                warn!("{FILE}: no item `{}`", s.item);
                None
            }
        })
        .collect();
    commands.insert_resource(Restored(restored));
}

fn store(dir: &SaveDir, items: &Items, q: &Query<(&WorldItem, &Transform)>) {
    let list: Vec<SavedItem> =
        q.iter().map(|(wi, tf)| SavedItem { item: items.get(wi.stack.item).name.clone(), count: wi.stack.count, at: tf.translation.to_array(), age: wi.age }).collect();
    write_ron(dir, FILE, &list);
}

fn save_now_and_then(time: Res<Time<Real>>, mut since: Local<f32>, dir: Res<SaveDir>, items: Res<Items>, q: Query<(&WorldItem, &Transform)>) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    store(&dir, &items, &q);
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, items: Res<Items>, q: Query<(&WorldItem, &Transform)>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &items, &q);
    }
}

#[derive(Component)]
struct WorldItem {
    stack: Stack,
    vel: f32,
    age: f32,
}

#[allow(clippy::too_many_arguments)]
fn spawn_spills(
    mut commands: Commands,
    mut spills: MessageReader<Spill>,
    restored: Option<ResMut<Restored>>,
    items: Res<Items>,
    blocks: Res<Blocks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    art: Option<Res<ItemArt>>,
    mut cube: Local<Option<Handle<Mesh>>>,
    mut grey: Local<Option<Handle<StandardMaterial>>>,
    mut block_cubes: Local<std::collections::HashMap<voxel::BlockId, Handle<Mesh>>>,
) {
    let back = restored.map(|mut r| std::mem::take(&mut r.0)).unwrap_or_default();
    let fresh = spills.read().map(|s| (s.stack, s.at, 0.0));
    for (stack, at, age) in back.into_iter().chain(fresh) {
        let s = Spill { stack, at };
        let item = items.get(s.stack.item);
        let mut e = commands.spawn((WorldItem { stack: s.stack, vel: 0.0, age }, Transform::from_translation(s.at), NotShadowCaster));
        match (&item.kind, &blocks.material) {
            // A block looks like a small copy of itself, every face textured.
            (ItemKind::Block(b), Some(voxel_mat)) => {
                let mesh = block_cubes.entry(*b).or_insert_with(|| meshes.add(crate::hands::block_mesh(blocks.registry.get(*b), DROP_SIZE))).clone();
                e.insert((Mesh3d(mesh), MeshMaterial3d(voxel_mat.clone())));
            }
            // Anything else is its sprite, made solid, standing up.
            _ => match (art.as_ref().and_then(|a| a.model(s.stack.item)), art.as_ref().and_then(|a| a.size(s.stack.item))) {
                (Some((mesh, mat)), Some(size)) => {
                    let k = (DROP_SIZE * 1.6 / size.max_element() as f32).min(0.016);
                    e.with_child((Mesh3d(mesh), MeshMaterial3d(mat), Transform::from_scale(Vec3::splat(k)), NotShadowCaster));
                    e.insert(Visibility::default());
                }
                _ => {
                    let mesh = cube.get_or_insert_with(|| meshes.add(Cuboid::new(DROP_SIZE, DROP_SIZE, DROP_SIZE))).clone();
                    let mat = grey.get_or_insert_with(|| materials.add(Color::srgb(0.5, 0.45, 0.38))).clone();
                    e.insert((Mesh3d(mesh), MeshMaterial3d(mat)));
                }
            },
        }
        // A torch on the ground still burns: a dimmer light than one in hand.
        if matches!(item.kind, ItemKind::Torch) {
            e.insert(Visibility::default()).with_child((
                PointLight { intensity: crate::torches::TORCH_LUMENS * 0.35, range: crate::torches::TORCH_RANGE * 0.6, color: crate::torches::TORCH_COLOR, shadow_maps_enabled: false, ..default() },
                crate::torches::Flicker { base: crate::torches::TORCH_LUMENS * 0.35, seed: at.x + at.z },
                Transform::from_xyz(0.0, 0.3, 0.0),
            ));
        }
    }
}

/// Edge of a dropped item's cube, in blocks.
const DROP_SIZE: f32 = 0.28;

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
        // Rest on the block below; unloaded ground counts as solid. Look a bit
        // deeper than the resting height (1.15 over the block), so a resting
        // item doesn't find air, drop a frame and get pushed back up (jitter).
        let below = BlockPos::from_world((tf.translation - Vec3::Y * 0.25).to_array());
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
