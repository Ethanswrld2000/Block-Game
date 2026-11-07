//! Placed torches: a stick, a flame and a warm flickering light for every
//! torch block in the loaded world.

use std::collections::HashMap;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::world::{ShapeEvent, World};

/// Brightness of a torch, lumens. Bright enough to light a room at night.
pub const TORCH_LUMENS: f32 = 150_000.0;
pub const TORCH_RANGE: f32 = 18.0;
pub const TORCH_COLOR: Color = Color::srgb(1.0, 0.68, 0.34);

pub struct TorchPlugin;

impl Plugin for TorchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Torches>()
            .add_systems(Startup, torch_assets)
            .add_systems(Update, (sync_torches, flicker).chain());
    }
}

#[derive(Resource, Default)]
struct Torches {
    placed: HashMap<BlockPos, Entity>,
}

#[derive(Resource, Clone)]
pub struct TorchAssets {
    pub stick: Handle<Mesh>,
    pub flame: Handle<Mesh>,
    pub wood: Handle<StandardMaterial>,
    pub fire: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct Flicker {
    pub base: f32,
    pub seed: f32,
}

fn torch_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(TorchAssets {
        stick: meshes.add(Cuboid::new(0.12, 0.6, 0.12)),
        flame: meshes.add(Cuboid::new(0.16, 0.16, 0.16)),
        wood: materials.add(StandardMaterial { base_color: Color::srgb(0.42, 0.28, 0.15), perceptual_roughness: 1.0, ..default() }),
        fire: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.75, 0.3),
            emissive: LinearRgba::rgb(6.0, 3.2, 0.9),
            unlit: true,
            ..default()
        }),
    });
}

/// A torch model with its light, as children of `parent`.
pub fn torch_model(parent: &mut ChildSpawnerCommands, a: &TorchAssets, seed: f32) {
    parent.spawn((Mesh3d(a.stick.clone()), MeshMaterial3d(a.wood.clone()), Transform::from_xyz(0.0, 0.3, 0.0), NotShadowCaster));
    parent.spawn((Mesh3d(a.flame.clone()), MeshMaterial3d(a.fire.clone()), Transform::from_xyz(0.0, 0.66, 0.0), NotShadowCaster));
    parent.spawn((
        PointLight { intensity: TORCH_LUMENS, range: TORCH_RANGE, color: TORCH_COLOR, shadow_maps_enabled: false, ..default() },
        Flicker { base: TORCH_LUMENS, seed },
        Transform::from_xyz(0.0, 0.8, 0.0),
    ));
}

fn sync_torches(
    mut commands: Commands,
    mut world: ResMut<World>,
    blocks: Res<Blocks>,
    assets: Option<Res<TorchAssets>>,
    mut torches: ResMut<Torches>,
) {
    let Some(assets) = assets else { return };
    let events = std::mem::take(&mut world.shape_events);
    for e in events {
        match e {
            ShapeEvent::Added(p, _) => {
                if let Some(old) = torches.placed.remove(&p) {
                    commands.entity(old).despawn();
                }
                let solid = |q: BlockPos| world.block(q).is_some_and(|b| blocks.registry.get(b).solid);
                // Stand on the block below; otherwise lean out from a wall.
                let base = Vec3::new(p.x as f32 + 0.5, p.y as f32, p.z as f32 + 0.5);
                let mut tf = Transform::from_translation(base);
                if !solid(BlockPos::new(p.x, p.y - 1, p.z)) {
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        if solid(BlockPos::new(p.x + dx, p.y, p.z + dz)) {
                            let out = Vec3::new(-dx as f32, 0.0, -dz as f32);
                            tf.translation = base + Vec3::new(dx as f32, 0.0, dz as f32) * 0.38 + Vec3::Y * 0.15;
                            tf.rotation = Quat::from_axis_angle(Vec3::Y.cross(out).normalize(), 0.45);
                            break;
                        }
                    }
                }
                let seed = (p.x * 31 + p.y * 17 + p.z * 7) as f32;
                let e = commands.spawn((tf, Visibility::default())).with_children(|c| torch_model(c, &assets, seed)).id();
                torches.placed.insert(p, e);
            }
            ShapeEvent::Removed(p) => {
                if let Some(e) = torches.placed.remove(&p) {
                    commands.entity(e).despawn();
                }
            }
        }
    }
    // Torches in chunks that unloaded.
    let gone: Vec<BlockPos> = torches.placed.keys().filter(|p| !world.is_loaded(p.chunk())).copied().collect();
    for p in gone {
        if let Some(e) = torches.placed.remove(&p) {
            commands.entity(e).despawn();
        }
    }
}

fn flicker(time: Res<Time>, mut lights: Query<(&mut PointLight, &Flicker)>) {
    let t = time.elapsed_secs();
    for (mut l, f) in &mut lights {
        let n = (t * 9.0 + f.seed).sin() * 0.5 + (t * 23.0 + f.seed * 1.7).sin() * 0.3 + (t * 3.1 + f.seed).sin() * 0.2;
        l.intensity = f.base * (1.0 + n * 0.08);
    }
}
