//! Placed torches: a wrapped stick with a flickering layered flame and fire
//! particles (flame licks, embers, a wisp of smoke) for every torch block in
//! the loaded world. Campfires (block `campfire`, also `shape: torch`) are
//! drawn here too: a ring of stones, crossed sticks, a bigger flame; they warm
//! you far more (see `survival`). Sconces (block `sconce`, dungeon walls) are
//! an iron bracket on the wall beside them holding a torch.
//!
//! Placed fires light the terrain through block light (`light` in blocks.ron,
//! `voxel::light`), not point lights: it can't pass through walls and has no
//! limit on how many shine. The held torch keeps a point light (`hands`).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::world::{ShapeEvent, World};

/// Brightness of the held torch's light, lumens.
pub const TORCH_LUMENS: f32 = 150_000.0;
pub const TORCH_RANGE: f32 = 18.0;
pub const TORCH_COLOR: Color = Color::srgb(1.0, 0.68, 0.34);

pub struct TorchPlugin;

impl Plugin for TorchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Torches>()
            .add_systems(crate::menu::GameStartup, torch_assets)
            .add_systems(crate::menu::GameUpdate, (sync_torches, flicker, flames, spawn_fire_particles, move_fire_particles).chain());
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

/// Flame colours from the hot core out to the cooling tips, for the
/// particles (index by age).
const FLAME_STEPS: usize = 8;

#[derive(Resource, Clone)]
pub struct TorchAssets {
    pub stick: Handle<Mesh>,
    pub flame: Handle<Mesh>,
    pub wood: Handle<StandardMaterial>,
    /// The wrapped, charred torch head on the stick.
    pub torch_stick: Handle<StandardMaterial>,
    /// Flame layers: pale core at the base, orange body, red tip.
    pub fire: [Handle<StandardMaterial>; 3],
    pub stone: Handle<Mesh>,
    pub stone_mat: Handle<StandardMaterial>,
    /// Wrought iron (sconce brackets).
    pub iron: Handle<StandardMaterial>,
    pub log: Handle<Mesh>,
    /// Particles: one quad, flame colours by age, embers, smoke.
    pub quad: Handle<Mesh>,
    pub flame_steps: Vec<Handle<StandardMaterial>>,
    pub ember: Handle<StandardMaterial>,
    pub smoke: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct Flicker {
    pub base: f32,
    pub seed: f32,
}

/// A flame layer that wavers: `size` is its resting scale.
#[derive(Component)]
struct Flame {
    seed: f32,
    size: f32,
    rest: Vec3,
}

/// A placed fire that throws particles: how many a second and how wide.
#[derive(Component)]
struct FireSource {
    big: bool,
    /// Where the flame sits, in the fire's own frame.
    tip: Vec3,
    /// Particles owed (fractions carry over).
    due: f32,
}

/// The torch stick's picture: 4 x 20 pixels, top row first. Charred, glowing
/// top; a wrap of cloth bound twice; plain wood below.
fn stick_image() -> Image {
    // k charred, e ember, c cloth, d cloth shadow, w wood, v dark wood grain.
    const ROWS: [&str; 20] = [
        "kekk", "ekke", "kcck", "cccd", "dccd", "cdcc", "cccd", "wvww", "wwvw", "wwwv", "vwww", "wwvw", "wvww", "wwwv", "wvww",
        "vwww", "wwvw", "wwwv", "wvww", "wwvw",
    ];
    let px = |c: char| -> [u8; 4] {
        match c {
            'k' => [38, 26, 20, 255],
            'e' => [196, 82, 28, 255],
            'c' => [178, 148, 104, 255],
            'd' => [134, 106, 72, 255],
            'v' => [86, 58, 34, 255],
            _ => [118, 82, 48, 255],
        }
    };
    let data: Vec<u8> = ROWS.iter().flat_map(|r| r.chars().flat_map(px)).collect();
    Image::new(Extent3d { width: 4, height: 20, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

/// Flame colour at `k` (0 = fresh, hot and pale; 1 = a cooling red tip), HDR.
fn flame_color(k: f32) -> LinearRgba {
    let hot = Vec3::new(1.5, 1.0, 0.25);
    let mid = Vec3::new(1.2, 0.33, 0.03);
    let tip = Vec3::new(0.6, 0.07, 0.015);
    let c = if k < 0.5 { hot.lerp(mid, k * 2.0) } else { mid.lerp(tip, (k - 0.5) * 2.0) };
    LinearRgba::new(c.x, c.y, c.z, 1.0 - 0.6 * k)
}

fn torch_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    // Unlit: drawn in exactly this (bright, so bloom catches it) colour.
    let fire = |m: &mut Assets<StandardMaterial>, c: LinearRgba| m.add(StandardMaterial { base_color: Color::LinearRgba(c), unlit: true, ..default() });
    let glow = |m: &mut Assets<StandardMaterial>, c: LinearRgba| {
        m.add(StandardMaterial {
            base_color: Color::LinearRgba(c),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            double_sided: true,
            cull_mode: None,
            fog_enabled: false,
            ..default()
        })
    };
    let flame_steps = (0..FLAME_STEPS).map(|i| glow(&mut materials, flame_color(i as f32 / (FLAME_STEPS - 1) as f32))).collect();
    let assets = TorchAssets {
        stick: meshes.add(Cuboid::new(0.12, 0.6, 0.12)),
        flame: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        stone: meshes.add(Cuboid::new(0.2, 0.14, 0.2)),
        stone_mat: materials.add(StandardMaterial { base_color: Color::srgb(0.45, 0.44, 0.42), perceptual_roughness: 1.0, ..default() }),
        iron: materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.2, 0.22), perceptual_roughness: 0.6, metallic: 0.6, ..default() }),
        log: meshes.add(Cuboid::new(0.1, 0.1, 0.7)),
        wood: materials.add(StandardMaterial { base_color: Color::srgb(0.42, 0.28, 0.15), perceptual_roughness: 1.0, ..default() }),
        torch_stick: materials.add(StandardMaterial {
            base_color_texture: Some(images.add(stick_image())),
            perceptual_roughness: 1.0,
            // The head glows a little from the fire on it.
            emissive: LinearRgba::rgb(0.25, 0.1, 0.03),
            ..default()
        }),
        fire: [
            fire(&mut materials, LinearRgba::rgb(1.7, 1.25, 0.4)),
            fire(&mut materials, LinearRgba::rgb(1.25, 0.34, 0.025)),
            fire(&mut materials, LinearRgba::rgb(0.75, 0.11, 0.015)),
        ],
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        flame_steps,
        ember: glow(&mut materials, LinearRgba::new(2.2, 0.7, 0.08, 1.0)),
        smoke: materials.add(StandardMaterial {
            base_color: Color::srgba(0.16, 0.15, 0.14, 0.28),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
    };
    commands.insert_resource(assets);
}

/// A flame of three stacked boxes, `size` across, centred near `at`: a pale
/// core showing under an orange body, and a small red tip on top.
fn flame_layers(parent: &mut ChildSpawnerCommands, a: &TorchAssets, at: Vec3, size: f32, seed: f32) {
    for (i, (k, lift)) in [(0.62, -0.22), (0.8, 0.12), (0.4, 0.62)].into_iter().enumerate() {
        let rest = at + Vec3::Y * lift * size;
        parent.spawn((
            Mesh3d(a.flame.clone()),
            MeshMaterial3d(a.fire[i].clone()),
            Transform::from_translation(rest).with_scale(Vec3::splat(size * k)),
            Flame { seed: seed + i as f32 * 3.1, size: size * k, rest },
            NotShadowCaster,
        ));
    }
}

/// A torch model, as children of `parent`.
pub fn torch_model(parent: &mut ChildSpawnerCommands, a: &TorchAssets, seed: f32) {
    parent.spawn((Mesh3d(a.stick.clone()), MeshMaterial3d(a.torch_stick.clone()), Transform::from_xyz(0.0, 0.3, 0.0), NotShadowCaster));
    flame_layers(parent, a, Vec3::new(0.0, 0.68, 0.0), 0.19, seed);
}

/// A campfire model, as children of `parent` (standing on the block's floor).
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
    flame_layers(parent, a, Vec3::new(0.0, 0.3, 0.0), 0.36, seed);
    flame_layers(parent, a, Vec3::new(0.05, 0.5, -0.03), 0.18, seed + 7.0);
}

/// A wall sconce, as children of `parent`: its frame has the wall at -Z (half
/// a block back) and +Z pointing out into the room.
pub fn sconce_model(parent: &mut ChildSpawnerCommands, a: &TorchAssets, seed: f32) {
    let iron = |size: Vec3, at: Vec3| (Mesh3d(a.flame.clone()), MeshMaterial3d(a.iron.clone()), Transform::from_translation(at).with_scale(size), NotShadowCaster);
    // A plate on the wall, an arm out from it, a cup round the torch.
    parent.spawn(iron(Vec3::new(0.2, 0.34, 0.04), Vec3::new(0.0, 0.42, -0.48)));
    parent.spawn(iron(Vec3::new(0.05, 0.05, 0.3), Vec3::new(0.0, 0.36, -0.33)));
    parent.spawn(iron(Vec3::new(0.16, 0.06, 0.16), Vec3::new(0.0, 0.42, -0.18)));
    let tilt = Quat::from_rotation_x(0.22);
    parent.spawn((Mesh3d(a.stick.clone()), MeshMaterial3d(a.torch_stick.clone()), Transform::from_xyz(0.0, 0.55, -0.2).with_rotation(tilt).with_scale(Vec3::splat(0.75)), NotShadowCaster));
    flame_layers(parent, a, Vec3::new(0.0, 0.84, -0.14), 0.16, seed);
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
                    let e = commands
                        .spawn((Transform::from_translation(at), Visibility::default(), FireSource { big: true, tip: Vec3::Y * 0.35, due: 0.0 }))
                        .with_children(|c| campfire_model(c, &assets, seed))
                        .id();
                    torches.placed.insert(p, (e, Fire::Campfire));
                    continue;
                }
                let solid = |q: BlockPos| world.block(q).is_some_and(|b| blocks.registry.get(b).solid);
                if blocks.registry.get(block).name == "sconce" {
                    // Against the first wall beside it, facing out.
                    let (dx, dz) = [(1, 0), (-1, 0), (0, 1), (0, -1)].into_iter().find(|&(dx, dz)| solid(BlockPos::new(p.x + dx, p.y, p.z + dz))).unwrap_or((0, -1));
                    let out = Vec3::new(-dx as f32, 0.0, -dz as f32);
                    let tf = Transform::from_xyz(p.x as f32 + 0.5, p.y as f32, p.z as f32 + 0.5).with_rotation(Quat::from_rotation_y(out.x.atan2(out.z)));
                    let e = commands
                        .spawn((tf, Visibility::default(), FireSource { big: false, tip: Vec3::new(0.0, 0.88, -0.14), due: 0.0 }))
                        .with_children(|c| sconce_model(c, &assets, seed))
                        .id();
                    torches.placed.insert(p, (e, Fire::Torch));
                    continue;
                }
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
                let e = commands
                    .spawn((tf, Visibility::default(), FireSource { big: false, tip: Vec3::Y * 0.7, due: 0.0 }))
                    .with_children(|c| torch_model(c, &assets, seed))
                    .id();
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

fn flicker(time: Res<Time>, mut lights: Query<(&mut PointLight, &Flicker)>) {
    let t = time.elapsed_secs();
    for (mut l, f) in &mut lights {
        let n = (t * 9.0 + f.seed).sin() * 0.5 + (t * 23.0 + f.seed * 1.7).sin() * 0.3 + (t * 3.1 + f.seed).sin() * 0.2;
        l.intensity = f.base * (1.0 + n * 0.08);
    }
}

/// Flame layers waver: they stretch up and squeeze, sway a little and flicker in size.
fn flames(time: Res<Time>, mut q: Query<(&Flame, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (f, mut tf) in &mut q {
        let a = (t * 11.0 + f.seed).sin() * 0.5 + (t * 27.0 + f.seed * 2.3).sin() * 0.3 + (t * 5.0 + f.seed * 0.7).sin() * 0.2;
        let up = 1.15 + 0.25 * a;
        let wide = 1.0 - 0.12 * a;
        tf.scale = Vec3::new(f.size * wide, f.size * up, f.size * wide);
        tf.translation = f.rest + Vec3::new((t * 6.0 + f.seed).sin(), 0.0, (t * 7.3 + f.seed * 1.3).cos()) * f.size * 0.06 + Vec3::Y * f.size * 0.08 * a;
        tf.rotation = Quat::from_rotation_y(0.6 + 0.25 * (t * 1.7 + f.seed).sin());
    }
}

/// One fire particle: a camera-facing square.
#[derive(Component)]
struct FireParticle {
    kind: Spark,
    vel: Vec3,
    age: f32,
    life: f32,
    size: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Spark {
    /// A lick of flame: rises fast, shrinks, cools from pale yellow to red.
    Flame,
    /// A glowing ember that drifts up and winks out.
    Ember,
    /// A faint grey puff above the flame.
    Smoke,
}

/// Only fires this near the camera throw particles.
const PARTICLE_NEAR: f32 = 28.0;
/// At most this many fire particles at once.
const MAX_PARTICLES: usize = 900;

fn rnd(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

/// Particles a second for a fire: (flames, embers, smoke).
fn rates(big: bool) -> (f32, f32, f32) {
    if big { (70.0, 4.0, 3.0) } else { (34.0, 1.2, 1.0) }
}

fn spawn_fire_particles(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<TorchAssets>>,
    cam: Query<&GlobalTransform, With<FlyCamera>>,
    mut fires: Query<(&GlobalTransform, &mut FireSource)>,
    live: Query<(), With<FireParticle>>,
    mut seed: Local<u32>,
) {
    let (Some(a), Ok(cam)) = (assets, cam.single()) else { return };
    if *seed == 0 {
        *seed = 0x9e37_79b9;
    }
    let dt = time.delta_secs().min(0.1);
    let eye = cam.translation();
    let mut room = MAX_PARTICLES.saturating_sub(live.iter().count());
    for (tf, mut fire) in &mut fires {
        let at = tf.transform_point(fire.tip);
        if at.distance_squared(eye) > PARTICLE_NEAR * PARTICLE_NEAR {
            fire.due = 0.0;
            continue;
        }
        let (flames, embers, smoke) = rates(fire.big);
        fire.due += (flames + embers + smoke) * dt;
        let width = if fire.big { 0.22 } else { 0.06 };
        while fire.due >= 1.0 && room > 0 {
            fire.due -= 1.0;
            room -= 1;
            let pick = rnd(&mut seed) * (flames + embers + smoke);
            let kind = if pick < flames {
                Spark::Flame
            } else if pick < flames + embers {
                Spark::Ember
            } else {
                Spark::Smoke
            };
            let off = Vec3::new(rnd(&mut seed) - 0.5, rnd(&mut seed) * 0.5 - 0.25, rnd(&mut seed) - 0.5) * width * 2.0;
            let scale: f32 = if fire.big { 1.7 } else { 1.0 };
            let (vel, life, size, mat, start) = match kind {
                Spark::Flame => (
                    Vec3::new(-off.x * 1.5, 0.9 + rnd(&mut seed) * 0.6, -off.z * 1.5) * scale.sqrt(),
                    0.28 + rnd(&mut seed) * 0.2,
                    (0.07 + rnd(&mut seed) * 0.05) * scale,
                    a.flame_steps[0].clone(),
                    at + off,
                ),
                Spark::Ember => (
                    Vec3::new(rnd(&mut seed) - 0.5, 1.2 + rnd(&mut seed) * 1.0, rnd(&mut seed) - 0.5) * Vec3::new(0.8, 1.0, 0.8),
                    0.9 + rnd(&mut seed) * 1.2,
                    0.025 + rnd(&mut seed) * 0.015,
                    a.ember.clone(),
                    at + off,
                ),
                Spark::Smoke => (
                    Vec3::new((rnd(&mut seed) - 0.5) * 0.2, 0.5 + rnd(&mut seed) * 0.3, (rnd(&mut seed) - 0.5) * 0.2),
                    1.4 + rnd(&mut seed) * 0.8,
                    (0.1 + rnd(&mut seed) * 0.05) * scale,
                    a.smoke.clone(),
                    at + off + Vec3::Y * 0.25 * scale,
                ),
            };
            commands.spawn((
                FireParticle { kind, vel, age: 0.0, life, size },
                Mesh3d(a.quad.clone()),
                MeshMaterial3d(mat),
                Transform::from_translation(start).with_scale(Vec3::splat(size)),
                NotShadowCaster,
            ));
        }
        fire.due = fire.due.min(4.0);
    }
}

fn move_fire_particles(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<TorchAssets>>,
    cam: Query<&GlobalTransform, With<FlyCamera>>,
    mut parts: Query<(Entity, &mut FireParticle, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let (Some(a), Ok(cam)) = (assets, cam.single()) else { return };
    let dt = time.delta_secs().min(0.1);
    let t = time.elapsed_secs();
    let face = cam.rotation();
    for (e, mut p, mut tf, mut mat) in &mut parts {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(e).despawn();
            continue;
        }
        let k = p.age / p.life;
        tf.rotation = face;
        match p.kind {
            Spark::Flame => {
                // Licks pull in towards the middle and speed up as they rise.
                p.vel.y += 1.2 * dt;
                p.vel.x *= (-4.0 * dt).exp();
                p.vel.z *= (-4.0 * dt).exp();
                tf.scale = Vec3::splat(p.size * (1.0 - k * 0.75));
                let step = ((k * FLAME_STEPS as f32) as usize).min(FLAME_STEPS - 1);
                if mat.0 != a.flame_steps[step] {
                    mat.0 = a.flame_steps[step].clone();
                }
            }
            Spark::Ember => {
                // Wander as they rise, slow down, and wink out at the end.
                let ph = p.size * 977.0;
                p.vel.x += (t * 5.0 + ph).sin() * 1.5 * dt;
                p.vel.z += (t * 4.3 + ph).cos() * 1.5 * dt;
                p.vel.y *= (-0.8 * dt).exp();
                let wink = if k > 0.8 { 1.0 - (k - 0.8) * 5.0 } else { 1.0 };
                tf.scale = Vec3::splat(p.size * wink.max(0.0));
            }
            Spark::Smoke => {
                p.vel *= (-0.5 * dt).exp();
                tf.scale = Vec3::splat(p.size * (1.0 + k * 2.5));
            }
        }
        let v = p.vel;
        tf.translation += v * dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flames_cool_from_pale_to_red() {
        let (hot, tip) = (flame_color(0.0), flame_color(1.0));
        assert!(hot.green / hot.red > tip.green / tip.red, "the fresh flame is yellower");
        assert!(hot.alpha > tip.alpha, "tips fade");
    }

    #[test]
    fn campfires_throw_more_than_torches() {
        let (t, c) = (rates(false), rates(true));
        assert!(c.0 > t.0 && c.1 > t.1);
    }
}
