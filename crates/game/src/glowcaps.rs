//! Glowcaps light the ground round them at night. Block light is torch-warm
//! and always on, so their cool night glow uses a few of Bevy's point lights
//! instead: a small pool moved to the nearest patches of glowcaps every half
//! second, brightening with the glowcaps themselves (`sky::night_glow`).

use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::{Blocks, VoxelMaterial};
use crate::camera::FlyCamera;
use crate::world::World;

pub struct GlowcapPlugin;

impl Plugin for GlowcapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::menu::GameStartup, spawn_pool)
            .add_systems(crate::menu::GameUpdate, place_lights);
    }
}

/// Lights in the pool.
const LIGHTS: usize = 12;
/// How far round the camera glowcaps are looked for, sideways and up/down.
const SEARCH: i32 = 24;
const SEARCH_Y: i32 = 10;
/// One light per patch: glowcaps this close to one already lit share its light.
const SPREAD: f32 = 3.0;
/// Seconds between looks.
const EVERY: f32 = 0.5;
const LUMENS: f32 = 9_000.0;
const RANGE: f32 = 5.0;
const COLOR: Color = Color::srgb(0.35, 0.85, 0.95);

#[derive(Component)]
struct GlowcapLight;

fn spawn_pool(mut commands: Commands) {
    for _ in 0..LIGHTS {
        commands.spawn((
            GlowcapLight,
            PointLight {
                color: COLOR,
                intensity: 0.0,
                range: RANGE,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

/// The nearest `n` spots to `from`, skipping any within `spread` of one already picked.
fn pick(mut spots: Vec<Vec3>, from: Vec3, n: usize, spread: f32) -> Vec<Vec3> {
    spots.sort_by(|a, b| {
        a.distance_squared(from)
            .total_cmp(&b.distance_squared(from))
    });
    let mut picked: Vec<Vec3> = Vec::with_capacity(n);
    for s in spots {
        if picked.len() == n {
            break;
        }
        if picked
            .iter()
            .all(|p| p.distance_squared(s) >= spread * spread)
        {
            picked.push(s);
        }
    }
    picked
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_lights(
    time: Res<Time>,
    mut wait: Local<f32>,
    mut lit: Local<Vec<Vec3>>,
    cam: Single<&Transform, With<FlyCamera>>,
    world: Res<World>,
    blocks: Res<Blocks>,
    materials: Res<Assets<VoxelMaterial>>,
    mut lights: Query<
        (&mut Transform, &mut PointLight, &mut Visibility),
        (With<GlowcapLight>, Without<FlyCamera>),
    >,
) {
    let strength = blocks
        .material
        .as_ref()
        .and_then(|h| materials.get(h))
        .map_or(0.0, |m| m.extension.glow.y);
    *wait -= time.delta_secs();
    if strength < 0.02 {
        lit.clear();
    } else if *wait <= 0.0 {
        *wait = EVERY;
        lit.clear();
        if let Some(cap) = blocks.registry.id("glowcap") {
            let c = BlockPos::from_world(cam.translation.to_array());
            let found = world.find_blocks(
                BlockPos::new(c.x - SEARCH, c.y - SEARCH_Y, c.z - SEARCH),
                BlockPos::new(c.x + SEARCH, c.y + SEARCH_Y, c.z + SEARCH),
                cap,
            );
            let spots = found
                .into_iter()
                .map(|p| Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.6, p.z as f32 + 0.5))
                .collect();
            *lit = pick(spots, cam.translation, LIGHTS, SPREAD);
        }
    }
    for (i, (mut tf, mut light, mut vis)) in lights.iter_mut().enumerate() {
        let want = match lit.get(i) {
            Some(at) => {
                tf.translation = *at;
                light.intensity = LUMENS * strength;
                Visibility::Inherited
            }
            None => Visibility::Hidden,
        };
        if *vis != want {
            *vis = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_nearest_one_per_patch() {
        let spots = vec![
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(6.0, 0.0, 0.0),
        ];
        // 2.0 is too close to 1.0, so the next is 6.0.
        assert_eq!(
            pick(spots.clone(), Vec3::ZERO, 2, 3.0),
            vec![Vec3::new(1.0, 0.0, 0.0), Vec3::new(6.0, 0.0, 0.0)]
        );
        assert_eq!(pick(spots, Vec3::ZERO, 8, 3.0).len(), 3);
    }
}
