//! Blocky clouds drifting with the wind at `CLOUD_Y`, below the sky islands,
//! so the tallest mountains poke through them.
//!
//! Clouds are cells on a grid laid over 2D noise; the weather's `clouds` value
//! sets how much of the sky is covered. The mesh is built on a worker thread
//! around the camera and rebuilt when the camera or the coverage moves on;
//! in between, the wind just slides the whole mesh along.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use fastnoise_lite::{FastNoiseLite, FractalType, NoiseType};

use crate::camera::FlyCamera;
use crate::weather::Conditions;

/// Height of the cloud base.
pub const CLOUD_Y: f32 = 236.0;
/// Size of one cloud cell in blocks.
const CELL: f32 = 12.0;
/// Cells from the centre to the edge of the cloud mesh.
const RADIUS: i32 = 80;
/// Rebuild when the camera has moved this many cells from the last build.
const REBUILD_CELLS: i32 = 8;

pub struct CloudPlugin;

impl Plugin for CloudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_clouds).add_systems(Update, update_clouds);
    }
}

#[derive(Component)]
struct Clouds {
    noise: Arc<FastNoiseLite>,
    /// How far the wind has carried the clouds, in blocks.
    offset: Vec2,
    built_center: Option<IVec2>,
    built_cover: f32,
    task: Option<Task<(Mesh, IVec2, f32)>>,
    material: Handle<StandardMaterial>,
}

fn spawn_clouds(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut noise = FastNoiseLite::with_seed(0x00C1_00D5);
    noise.set_noise_type(Some(NoiseType::OpenSimplex2));
    noise.set_frequency(Some(0.07));
    noise.set_fractal_type(Some(FractalType::FBm));
    noise.set_fractal_octaves(Some(3));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.88),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        ..default()
    });
    commands.spawn((
        Clouds {
            noise: Arc::new(noise),
            offset: Vec2::ZERO,
            built_center: None,
            built_cover: -1.0,
            task: None,
            material: material.clone(),
        },
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, CLOUD_Y, 0.0),
        Visibility::Hidden,
        NotShadowCaster,
    ));
}

fn update_clouds(
    mut commands: Commands,
    time: Res<Time>,
    weather: Res<Conditions>,
    cam: Query<&Transform, (With<FlyCamera>, Without<Clouds>)>,
    mut q: Query<(Entity, &mut Clouds, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Ok(cam), Ok((entity, mut clouds, mut tf, mut vis))) = (cam.single(), q.single_mut()) else { return };
    let wind = Vec2::new(1.0, 0.35).normalize() * weather.wind;
    clouds.offset += wind * time.delta_secs();
    tf.translation = Vec3::new(clouds.offset.x, CLOUD_Y, clouds.offset.y);

    // Collect a finished build.
    if let Some(task) = clouds.task.as_mut()
        && let Some((mesh, center, cover)) = check_ready(task)
    {
        clouds.task = None;
        clouds.built_center = Some(center);
        clouds.built_cover = cover;
        commands.entity(entity).insert(Mesh3d(meshes.add(mesh)));
        *vis = Visibility::Inherited;
    }

    // Start a new build if needed.
    let center = ((Vec2::new(cam.translation.x, cam.translation.z) - clouds.offset) / CELL).floor().as_ivec2();
    let moved = clouds.built_center.is_none_or(|c| (c - center).abs().max_element() >= REBUILD_CELLS);
    let cover = (weather.clouds * 50.0).round() / 50.0;
    if clouds.task.is_none() && (moved || (cover - clouds.built_cover).abs() > 0.01) {
        let noise = clouds.noise.clone();
        clouds.task = Some(AsyncComputeTaskPool::get().spawn(async move { (build(&noise, center, cover), center, cover) }));
    }

    // Shade: white in fair weather, grey before rain.
    if let Some(mut m) = materials.get_mut(&clouds.material) {
        let s = weather.cloud_shade;
        let want = Color::srgba(s, s, s * 1.02, 0.78 + 0.18 * weather.clouds);
        if m.base_color != want {
            m.base_color = want;
        }
    }
}

/// Thickness of the cloud in a cell, 0 if clear sky.
fn thickness(noise: &FastNoiseLite, x: i32, z: i32, cover: f32) -> f32 {
    let n = noise.get_noise_2d(x as f32, z as f32) * 0.5 + 0.5;
    let threshold = 0.74 - cover * 0.46;
    if n < threshold {
        0.0
    } else if n < threshold + 0.1 {
        4.0
    } else {
        8.0
    }
}

fn build(noise: &FastNoiseLite, center: IVec2, cover: f32) -> Mesh {
    let n = (RADIUS * 2 + 3) as usize;
    let mut h = vec![0.0f32; n * n];
    let idx = |dx: i32, dz: i32| ((dz + RADIUS + 1) as usize) * n + (dx + RADIUS + 1) as usize;
    for dz in -RADIUS - 1..=RADIUS + 1 {
        for dx in -RADIUS - 1..=RADIUS + 1 {
            if dx * dx + dz * dz <= (RADIUS + 1) * (RADIUS + 1) {
                h[idx(dx, dz)] = thickness(noise, center.x + dx, center.y + dz, cover);
            }
        }
    }

    let (mut pos, mut nor, mut col, mut ind) = (Vec::new(), Vec::new(), Vec::new(), Vec::<u32>::new());
    let mut quad = |corners: [Vec3; 4], normal: Vec3, alpha: f32| {
        let s = pos.len() as u32;
        for c in corners {
            pos.push(c.to_array());
            nor.push(normal.to_array());
            col.push([1.0, 1.0, 1.0, alpha]);
        }
        ind.extend_from_slice(&[s, s + 1, s + 2, s, s + 2, s + 3]);
    };
    for dz in -RADIUS..=RADIUS {
        for dx in -RADIUS..=RADIUS {
            let r2 = (dx * dx + dz * dz) as f32;
            if r2 > (RADIUS * RADIUS) as f32 {
                continue;
            }
            let t = h[idx(dx, dz)];
            if t == 0.0 {
                continue;
            }
            // Fade out towards the edge of the mesh.
            let d = r2.sqrt() / RADIUS as f32;
            let alpha = 1.0 - ((d - 0.7) / 0.3).clamp(0.0, 1.0);
            let (x0, z0) = ((center.x + dx) as f32 * CELL, (center.y + dz) as f32 * CELL);
            let (x1, z1) = (x0 + CELL, z0 + CELL);
            quad([Vec3::new(x0, t, z0), Vec3::new(x0, t, z1), Vec3::new(x1, t, z1), Vec3::new(x1, t, z0)], Vec3::Y, alpha);
            quad([Vec3::new(x0, 0.0, z0), Vec3::new(x1, 0.0, z0), Vec3::new(x1, 0.0, z1), Vec3::new(x0, 0.0, z1)], Vec3::NEG_Y, alpha);
            // Walls where the neighbour is thinner.
            let sides = [
                (h[idx(dx + 1, dz)], Vec3::X, [Vec3::new(x1, 0.0, z0), Vec3::new(x1, 1.0, z0), Vec3::new(x1, 1.0, z1), Vec3::new(x1, 0.0, z1)]),
                (h[idx(dx - 1, dz)], Vec3::NEG_X, [Vec3::new(x0, 0.0, z1), Vec3::new(x0, 1.0, z1), Vec3::new(x0, 1.0, z0), Vec3::new(x0, 0.0, z0)]),
                (h[idx(dx, dz + 1)], Vec3::Z, [Vec3::new(x1, 0.0, z1), Vec3::new(x1, 1.0, z1), Vec3::new(x0, 1.0, z1), Vec3::new(x0, 0.0, z1)]),
                (h[idx(dx, dz - 1)], Vec3::NEG_Z, [Vec3::new(x0, 0.0, z0), Vec3::new(x0, 1.0, z0), Vec3::new(x1, 1.0, z0), Vec3::new(x1, 0.0, z0)]),
            ];
            for (other, normal, c) in sides {
                if other < t {
                    let lift = |v: Vec3| Vec3::new(v.x, if v.y == 0.0 { other } else { t }, v.z);
                    quad(c.map(lift), normal, alpha);
                }
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(ind))
}
