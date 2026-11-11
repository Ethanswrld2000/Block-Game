//! Rain and snow around the camera.
//!
//! A fixed pool of drops falls through a cylinder around the camera; the
//! weather's precipitation sets how many are shown, and the local temperature
//! picks rain or snow. A drop that hits a block starts again at the top, and a
//! drop whose column is roofed over stays hidden, so caves and houses stay dry.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use voxel::BlockPos;

use crate::camera::FlyCamera;
use crate::weather::Conditions;
use crate::world::World;

const DROPS: usize = 1600;
const AREA_RADIUS: f32 = 30.0;
const ABOVE: f32 = 26.0;
const BELOW: f32 = 14.0;
const RAIN_SPEED: f32 = 30.0;
const SNOW_SPEED: f32 = 2.6;

pub struct PrecipitationPlugin;

impl Plugin for PrecipitationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_drops).add_systems(Update, fall);
    }
}

#[derive(Resource)]
struct DropAssets {
    rain_mesh: Handle<Mesh>,
    snow_mesh: Handle<Mesh>,
    rain_mat: Handle<StandardMaterial>,
    snow_mat: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Drop {
    index: usize,
    snow: bool,
    /// Hidden for this fall because something is overhead.
    covered: bool,
    phase: f32,
}

fn spawn_drops(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let assets = DropAssets {
        rain_mesh: meshes.add(Cuboid::new(0.035, 0.8, 0.035)),
        snow_mesh: meshes.add(Cuboid::new(0.14, 0.14, 0.14)),
        rain_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(0.72, 0.78, 0.9, 0.5),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.3,
            ..default()
        }),
        snow_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.97, 1.0),
            perceptual_roughness: 1.0,
            ..default()
        }),
    };
    for index in 0..DROPS {
        commands.spawn((
            Drop { index, snow: false, covered: true, phase: index as f32 * 0.37 },
            Mesh3d(assets.rain_mesh.clone()),
            MeshMaterial3d(assets.rain_mat.clone()),
            Transform::from_xyz(0.0, -10_000.0, 0.0),
            Visibility::Hidden,
            NotShadowCaster,
        ));
    }
    commands.insert_resource(assets);
}

/// xorshift: fast, good enough for scattering raindrops.
fn rand(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 / (1u64 << 24) as f32
}

fn blocked(world: &World, p: Vec3) -> bool {
    world.block(BlockPos::from_world(p.to_array())).is_some_and(|b| !b.is_air())
}

#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
fn fall(
    time: Res<Time>,
    weather: Res<Conditions>,
    wind: Res<crate::wind::Wind>,
    world: Res<World>,
    assets: Res<DropAssets>,
    cam: Query<&Transform, (With<FlyCamera>, Without<Drop>)>,
    mut drops: Query<(&mut Drop, &mut Transform, &mut Visibility, &mut Mesh3d, &mut MeshMaterial3d<StandardMaterial>)>,
    mut rng: Local<u64>,
) {
    if *rng == 0 {
        *rng = 0x2545_F491_4F6C_DD1D;
    }
    let Ok(cam) = cam.single() else { return };
    let c = cam.translation;
    let dt = time.delta_secs().min(0.25);
    let active = if weather.precipitation > 0.02 { (weather.precipitation * DROPS as f32) as usize } else { 0 };
    let w = wind.velocity();
    let wind = Vec3::new(w.x, 0.0, w.y);
    let t = time.elapsed_secs();

    for (mut d, mut tf, mut vis, mut mesh, mut mat) in &mut drops {
        if d.index >= active {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
                tf.translation.y = -10_000.0;
            }
            continue;
        }
        let vel = if d.snow {
            Vec3::new((t * 1.3 + d.phase).sin() * 0.6, -SNOW_SPEED, (t * 1.1 + d.phase * 1.7).cos() * 0.6) + wind * 0.35
        } else {
            Vec3::new(0.0, -RAIN_SPEED, 0.0) + wind * 0.4
        };
        tf.translation += vel * dt;
        let mut off = tf.translation - c;
        // Blown (or walked) out of the side: come back in on the far side at
        // the same height, so drifting snow keeps filling the air.
        let r2 = off.x * off.x + off.z * off.z;
        if r2 > AREA_RADIUS * AREA_RADIUS * 1.2 && off.y > -1000.0 && r2 < AREA_RADIUS * AREA_RADIUS * 4.0 {
            off.x = -off.x * 0.95;
            off.z = -off.z * 0.95;
            tf.translation = c + off;
        }
        // Rain turned to snow (or back): every drop starts again as the new kind
        // at once, so the two never fall together.
        let switching = d.snow != weather.snowing;
        let gone = switching || off.y < -BELOW || off.y > ABOVE + 6.0 || off.x * off.x + off.z * off.z > AREA_RADIUS * AREA_RADIUS * 1.2;
        let hit = !gone && !d.covered && blocked(&world, tf.translation);
        if gone || hit {
            // Start again somewhere above the camera. A freshly started pool
            // (y far below) scatters through the whole height so it doesn't
            // arrive in one sheet.
            let fresh = off.y < -1000.0 || switching;
            let a = rand(&mut rng) * std::f32::consts::TAU;
            let r = rand(&mut rng).sqrt() * AREA_RADIUS;
            let y = if fresh { -BELOW + rand(&mut rng) * (ABOVE + BELOW) } else { ABOVE - rand(&mut rng) * 4.0 };
            tf.translation = c + Vec3::new(a.cos() * r, y, a.sin() * r);
            if switching {
                d.snow = weather.snowing;
                mesh.0 = if d.snow { assets.snow_mesh.clone() } else { assets.rain_mesh.clone() };
                mat.0 = if d.snow { assets.snow_mat.clone() } else { assets.rain_mat.clone() };
            }
            // Roofed over? Look up to 40 blocks above the start point.
            let start = tf.translation;
            d.covered = (0..40).any(|i| blocked(&world, start + Vec3::Y * i as f32));
            *vis = if d.covered { Visibility::Hidden } else { Visibility::Inherited };
        }
        if !d.snow {
            // Lean raindrops into the wind.
            tf.rotation = Quat::from_rotation_arc(Vec3::NEG_Y, vel.normalize());
        }
    }
}
