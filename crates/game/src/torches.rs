//! Placed torches: a stick, a flame and a warm flickering light for every
//! torch block in the loaded world. Campfires (block `campfire`, also
//! `shape: torch`) are drawn here too: a ring of stones, crossed sticks, a
//! bigger flame and light; they warm you far more (see `survival`).

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
/// A campfire's light.
pub const CAMPFIRE_LUMENS: f32 = 420_000.0;
pub const CAMPFIRE_RANGE: f32 = 26.0;

pub struct TorchPlugin;

impl Plugin for TorchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Torches>()
            .add_systems(crate::menu::GameStartup, torch_assets)
            .add_systems(crate::menu::GameUpdate, (sync_torches, share_lights, flicker).chain());
    }
}

#[derive(Resource, Default)]
pub struct Torches {
    placed: HashMap<BlockPos, (Entity, Fire)>,
}

/// What kind of fire a torch-shaped block is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fire {
    Torch,
    Campfire,
}

impl Torches {
    /// Where the loaded torches and campfires are (fire warms you up, see `survival`).
    pub fn positions(&self) -> impl Iterator<Item = BlockPos> + '_ {
        self.placed.keys().copied()
    }

    /// Every loaded fire and its kind.
    pub fn fires(&self) -> impl Iterator<Item = (BlockPos, Fire)> + '_ {
        self.placed.iter().map(|(p, (_, f))| (*p, *f))
    }
}

#[derive(Resource, Clone)]
pub struct TorchAssets {
    pub stick: Handle<Mesh>,
    pub flame: Handle<Mesh>,
    pub wood: Handle<StandardMaterial>,
    pub fire: Handle<StandardMaterial>,
    pub stone: Handle<Mesh>,
    pub stone_mat: Handle<StandardMaterial>,
    pub log: Handle<Mesh>,
    pub big_flame: Handle<Mesh>,
}

#[derive(Component)]
pub struct Flicker {
    pub base: f32,
    pub seed: f32,
}

/// A placed torch's or campfire's light. Only the `MAX_LIGHTS` nearest the
/// camera shine (the furthest few fade out), so a wall of torches costs no
/// more than a handful; the flames still show.
#[derive(Component)]
pub struct FireLight(f32);

/// Placed fire lights that shine at once.
pub const MAX_LIGHTS: usize = 24;
/// The furthest this many of them fade out instead of popping.
const FADE_LIGHTS: usize = 6;

/// How bright the light ranked `rank` (0 = nearest the camera) shines, 0..1.
pub fn light_share(rank: usize) -> f32 {
    if rank >= MAX_LIGHTS {
        0.0
    } else {
        ((MAX_LIGHTS - rank) as f32 / FADE_LIGHTS as f32).min(1.0)
    }
}

/// Rank placed fire lights by distance to the camera and dim all but the nearest.
fn share_lights(
    time: Res<Time>,
    mut wait: Local<f32>,
    cam: Query<&GlobalTransform, With<crate::camera::FlyCamera>>,
    mut lights: Query<(&GlobalTransform, &mut FireLight, &mut Visibility)>,
) {
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    *wait = 0.25;
    let Ok(cam) = cam.single() else { return };
    let eye = cam.translation();
    let mut ranked: Vec<(f32, usize)> = lights.iter().enumerate().map(|(i, (tf, _, _))| (tf.translation().distance_squared(eye), i)).collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut rank_of = vec![0; ranked.len()];
    for (rank, (_, i)) in ranked.iter().enumerate() {
        rank_of[*i] = rank;
    }
    for (i, (_, mut light, mut vis)) in lights.iter_mut().enumerate() {
        light.0 = light_share(rank_of[i]);
        let want = if light.0 > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

fn torch_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(TorchAssets {
        stick: meshes.add(Cuboid::new(0.12, 0.6, 0.12)),
        flame: meshes.add(Cuboid::new(0.16, 0.16, 0.16)),
        stone: meshes.add(Cuboid::new(0.2, 0.14, 0.2)),
        stone_mat: materials.add(StandardMaterial { base_color: Color::srgb(0.45, 0.44, 0.42), perceptual_roughness: 1.0, ..default() }),
        log: meshes.add(Cuboid::new(0.1, 0.1, 0.7)),
        big_flame: meshes.add(Cuboid::new(0.3, 0.3, 0.3)),
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
        FireLight(1.0),
        Transform::from_xyz(0.0, 0.8, 0.0),
    ));
}

/// A campfire model with its light, as children of `parent` (standing on the block's floor).
pub fn campfire_model(parent: &mut ChildSpawnerCommands, a: &TorchAssets, seed: f32) {
    for i in 0..8 {
        let ang = i as f32 / 8.0 * std::f32::consts::TAU;
        parent.spawn((
            Mesh3d(a.stone.clone()),
            MeshMaterial3d(a.stone_mat.clone()),
            Transform::from_xyz(ang.cos() * 0.36, 0.07, ang.sin() * 0.36).with_rotation(Quat::from_rotation_y(-ang)),
        ));
    }
    for i in 0..3 {
        let yaw = i as f32 / 3.0 * std::f32::consts::PI;
        parent.spawn((
            Mesh3d(a.log.clone()),
            MeshMaterial3d(a.wood.clone()),
            Transform::from_xyz(0.0, 0.12, 0.0).with_rotation(Quat::from_rotation_y(yaw) * Quat::from_rotation_x(0.35)),
        ));
    }
    parent.spawn((Mesh3d(a.big_flame.clone()), MeshMaterial3d(a.fire.clone()), Transform::from_xyz(0.0, 0.3, 0.0).with_rotation(Quat::from_rotation_y(0.8)), NotShadowCaster));
    parent.spawn((Mesh3d(a.flame.clone()), MeshMaterial3d(a.fire.clone()), Transform::from_xyz(0.03, 0.52, -0.02), NotShadowCaster));
    parent.spawn((
        PointLight { intensity: CAMPFIRE_LUMENS, range: CAMPFIRE_RANGE, color: TORCH_COLOR, shadow_maps_enabled: false, ..default() },
        Flicker { base: CAMPFIRE_LUMENS, seed },
        FireLight(1.0),
        Transform::from_xyz(0.0, 0.7, 0.0),
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
            ShapeEvent::Added(p, block) => {
                if let Some((old, _)) = torches.placed.remove(&p) {
                    commands.entity(old).despawn();
                }
                let seed = (p.x * 31 + p.y * 17 + p.z * 7) as f32;
                if blocks.registry.get(block).name == "campfire" {
                    let at = Vec3::new(p.x as f32 + 0.5, p.y as f32, p.z as f32 + 0.5);
                    let e = commands.spawn((Transform::from_translation(at), Visibility::default())).with_children(|c| campfire_model(c, &assets, seed)).id();
                    torches.placed.insert(p, (e, Fire::Campfire));
                    continue;
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
                let e = commands.spawn((tf, Visibility::default())).with_children(|c| torch_model(c, &assets, seed)).id();
                torches.placed.insert(p, (e, Fire::Torch));
            }
            ShapeEvent::Removed(p) => {
                if let Some((e, _)) = torches.placed.remove(&p) {
                    commands.entity(e).despawn();
                }
            }
        }
    }
    // Torches in chunks that unloaded.
    let gone: Vec<BlockPos> = torches.placed.keys().filter(|p| !world.is_loaded(p.chunk())).copied().collect();
    for p in gone {
        if let Some((e, _)) = torches.placed.remove(&p) {
            commands.entity(e).despawn();
        }
    }
}

fn flicker(time: Res<Time>, mut lights: Query<(&mut PointLight, &Flicker, Option<&FireLight>)>) {
    let t = time.elapsed_secs();
    for (mut l, f, share) in &mut lights {
        let n = (t * 9.0 + f.seed).sin() * 0.5 + (t * 23.0 + f.seed * 1.7).sin() * 0.3 + (t * 3.1 + f.seed).sin() * 0.2;
        l.intensity = f.base * (1.0 + n * 0.08) * share.map_or(1.0, |s| s.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_nearest_lights_shine() {
        assert_eq!(light_share(0), 1.0);
        assert_eq!(light_share(MAX_LIGHTS - FADE_LIGHTS), 1.0);
        assert!(light_share(MAX_LIGHTS - 2) > 0.0 && light_share(MAX_LIGHTS - 2) < 1.0);
        assert_eq!(light_share(MAX_LIGHTS), 0.0);
        assert_eq!(light_share(500), 0.0);
    }
}
