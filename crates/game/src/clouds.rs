//! Blocky clouds in three layers, drifting with the wind:
//!
//! - **low** (y 200): a grey deck that only forms in rain, snow and storms;
//! - **middle** (y 236): the puffy blocks, scattered on a clear day, joined up
//!   into a lid when it pours;
//! - **high** (y 300): thin, wide wisps that drift faster, there in most
//!   weather, below the sky islands.
//!
//! How much of the sky each layer covers follows the weather's `clouds`
//! value (`assets/data/weather.ron`): clear days get a few clouds, rain and
//! snow cover the sky. Each layer is cells on a grid laid over 2D noise. Its
//! mesh is built on a worker thread around the camera and rebuilt when the
//! camera or the coverage moves on; in between, the wind slides it along.
//! Cover changes only when the weather does (see `weather`), so between
//! changes the sky holds steady.
//!
//! The low and middle layers are opaque: blended meshes aren't sorted within
//! themselves or reliably against each other, which made faces and layers
//! flicker over one another as the camera turned. Instead of fading out at
//! the edge they thin out, and the fog hides the rest. The thin high wisps
//! stay see-through; they're drawn after the opaque layers, so those hide them.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use fastnoise_lite::{FastNoiseLite, FractalType, NoiseType};

use crate::camera::FlyCamera;
use crate::weather::Conditions;

/// Height of the middle layer's base.
pub const CLOUD_Y: f32 = 236.0;

pub struct CloudPlugin;

impl Plugin for CloudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_clouds).add_systems(Update, update_clouds);
    }
}

/// One cloud layer's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub name: &'static str,
    /// Height of the layer's base.
    pub y: f32,
    /// Size of one cell in blocks.
    pub cell: f32,
    /// Cells from the centre to the edge of the mesh.
    pub radius: i32,
    /// Thickness of a thin and a thick cell, in blocks.
    pub thin: f32,
    pub thick: f32,
    /// Noise frequency (per cell) and seed.
    pub frequency: f32,
    pub seed: i32,
    /// The layer's own cover from the weather's: `(cover - from) / (to - from)`.
    pub from: f32,
    pub to: f32,
    /// How fast it moves compared with the wind.
    pub speed: f32,
    /// How bright it is compared with the weather's cloud shade.
    pub shade: f32,
    /// See-through layers blend (and fade out at the edge); the rest are opaque.
    pub alpha: f32,
}

pub const LAYERS: [Layer; 3] = [
    Layer { name: "low", y: 200.0, cell: 10.0, radius: 90, thin: 5.0, thick: 9.0, frequency: 0.06, seed: 0x10D0, from: 0.55, to: 0.95, speed: 0.8, shade: 0.82, alpha: 1.0 },
    Layer { name: "middle", y: CLOUD_Y, cell: 12.0, radius: 80, thin: 4.0, thick: 8.0, frequency: 0.07, seed: 0x00C1_00D5, from: 0.08, to: 0.85, speed: 1.0, shade: 1.0, alpha: 1.0 },
    Layer { name: "high", y: 300.0, cell: 20.0, radius: 60, thin: 1.0, thick: 2.0, frequency: 0.05, seed: 0x41A7, from: 0.0, to: 1.0, speed: 1.6, shade: 1.05, alpha: 0.8 },
];

impl Layer {
    /// This layer's cover, 0..1, for the weather's cloud cover.
    pub fn cover(&self, weather: f32) -> f32 {
        ((weather - self.from) / (self.to - self.from)).clamp(0.0, 1.0)
    }

    pub fn opaque(&self) -> bool {
        self.alpha >= 1.0
    }
}

/// Rebuild when the camera has moved this many cells from the last build.
const REBUILD_CELLS: i32 = 8;

#[derive(Component)]
struct Clouds {
    layer: Layer,
    noise: Arc<FastNoiseLite>,
    /// How far the wind has carried the clouds, in blocks.
    offset: Vec2,
    built_center: Option<IVec2>,
    built_cover: f32,
    task: Option<Task<(Mesh, IVec2, f32)>>,
    material: Handle<StandardMaterial>,
}

fn spawn_clouds(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    for layer in LAYERS {
        let mut noise = FastNoiseLite::with_seed(layer.seed);
        noise.set_noise_type(Some(NoiseType::OpenSimplex2));
        noise.set_frequency(Some(layer.frequency));
        noise.set_fractal_type(Some(FractalType::FBm));
        noise.set_fractal_octaves(Some(3));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, layer.alpha),
            alpha_mode: if layer.opaque() { AlphaMode::Opaque } else { AlphaMode::Blend },
            perceptual_roughness: 1.0,
            reflectance: 0.0,
            ..default()
        });
        commands.spawn((
            Name::new(format!("clouds ({})", layer.name)),
            Clouds { layer, noise: Arc::new(noise), offset: Vec2::ZERO, built_center: None, built_cover: -1.0, task: None, material: material.clone() },
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, layer.y, 0.0),
            Visibility::Hidden,
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn update_clouds(
    mut commands: Commands,
    time: Res<Time>,
    weather: Res<Conditions>,
    settings: Res<crate::settings::Settings>,
    cam: Query<&Transform, (With<FlyCamera>, Without<Clouds>)>,
    mut q: Query<(Entity, &mut Clouds, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(cam) = cam.single() else { return };
    // The Clouds setting; `BLOCKGAME_NO_CLOUDS=1` turns them off (to measure what they cost).
    let on = settings.clouds > 0 && std::env::var_os("BLOCKGAME_NO_CLOUDS").is_none();
    let wind = Vec2::new(1.0, 0.35).normalize() * weather.wind;
    for (entity, mut clouds, mut tf, mut vis) in &mut q {
        let layer = clouds.layer;
        clouds.offset += wind * layer.speed * time.delta_secs();
        tf.translation = Vec3::new(clouds.offset.x, layer.y, clouds.offset.y);

        // Collect a finished build.
        if let Some(task) = clouds.task.as_mut()
            && let Some((mesh, center, cover)) = check_ready(task)
        {
            clouds.task = None;
            clouds.built_center = Some(center);
            clouds.built_cover = cover;
            // A clear layer builds an empty mesh; draw nothing rather than an
            // empty mesh (the renderer warns about those).
            if mesh.count_vertices() == 0 {
                commands.entity(entity).remove::<Mesh3d>();
            } else {
                commands.entity(entity).insert(Mesh3d(meshes.add(mesh)));
            }
        }
        let cover = (layer.cover(weather.clouds) * 50.0).round() / 50.0;
        let want = if on && clouds.built_center.is_some() && clouds.built_cover > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if !on {
            continue;
        }

        // Start a new build if needed.
        let center = ((Vec2::new(cam.translation.x, cam.translation.z) - clouds.offset) / layer.cell).floor().as_ivec2();
        let moved = clouds.built_center.is_none_or(|c| (c - center).abs().max_element() >= REBUILD_CELLS);
        if clouds.task.is_none() && (moved || (cover - clouds.built_cover).abs() > 0.01) {
            let noise = clouds.noise.clone();
            clouds.task = Some(AsyncComputeTaskPool::get().spawn(async move { (build(&noise, &layer, center, cover), center, cover) }));
        }

        // Shade: white in fair weather, grey before rain.
        if let Some(mut m) = materials.get_mut(&clouds.material) {
            let s = (weather.cloud_shade * layer.shade).min(1.0);
            let want = Color::srgba(s, s, s * 1.02, layer.alpha);
            if m.base_color != want {
                m.base_color = want;
            }
        }
    }
}

/// Thickness of the cloud in a cell, 0 if clear sky.
fn thickness(noise: &FastNoiseLite, layer: &Layer, x: i32, z: i32, cover: f32) -> f32 {
    if cover <= 0.0 {
        return 0.0;
    }
    let n = noise.get_noise_2d(x as f32, z as f32) * 0.5 + 0.5;
    let threshold = 0.72 - cover * 0.5;
    if n < threshold {
        0.0
    } else if n < threshold + 0.1 {
        layer.thin
    } else {
        layer.thick
    }
}

/// The share of cells with cloud in them, for the tests.
#[cfg(test)]
fn coverage(layer: &Layer, cover: f32) -> f32 {
    let mut noise = FastNoiseLite::with_seed(layer.seed);
    noise.set_noise_type(Some(NoiseType::OpenSimplex2));
    noise.set_frequency(Some(layer.frequency));
    noise.set_fractal_type(Some(FractalType::FBm));
    noise.set_fractal_octaves(Some(3));
    let n = 120;
    let mut hit = 0;
    for z in 0..n {
        for x in 0..n {
            if thickness(&noise, layer, x, z, cover) > 0.0 {
                hit += 1;
            }
        }
    }
    hit as f32 / (n * n) as f32
}

fn build(noise: &FastNoiseLite, layer: &Layer, center: IVec2, cover: f32) -> Mesh {
    let radius = layer.radius;
    let cell = layer.cell;
    let n = (radius * 2 + 3) as usize;
    let mut h = vec![0.0f32; n * n];
    let idx = |dx: i32, dz: i32| ((dz + radius + 1) as usize) * n + (dx + radius + 1) as usize;
    for dz in -radius - 1..=radius + 1 {
        for dx in -radius - 1..=radius + 1 {
            let r2 = dx * dx + dz * dz;
            if r2 <= (radius + 1) * (radius + 1) {
                // Opaque layers thin out over the outer part instead of fading.
                let edge = if layer.opaque() { (((r2 as f32).sqrt() / radius as f32 - 0.55) / 0.45).clamp(0.0, 1.0) } else { 0.0 };
                h[idx(dx, dz)] = thickness(noise, layer, center.x + dx, center.y + dz, cover * (1.0 - edge));
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
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let r2 = (dx * dx + dz * dz) as f32;
            if r2 > (radius * radius) as f32 {
                continue;
            }
            let t = h[idx(dx, dz)];
            if t == 0.0 {
                continue;
            }
            // Fade out towards the edge of the mesh.
            let d = r2.sqrt() / radius as f32;
            let alpha = 1.0 - ((d - 0.7) / 0.3).clamp(0.0, 1.0);
            let (x0, z0) = ((center.x + dx) as f32 * cell, (center.y + dz) as f32 * cell);
            let (x1, z1) = (x0 + cell, z0 + cell);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Cloud cover in `weather.ron` for clear skies and for rain.
    const CLEAR: f32 = 0.22;
    const RAIN: f32 = 0.9;

    #[test]
    fn clear_days_have_some_clouds_and_rain_covers_the_sky() {
        let [low, mid, high] = LAYERS;
        // Clear: no low deck, a scattering of puffs and some wisps.
        assert_eq!(low.cover(CLEAR), 0.0);
        let puffs = coverage(&mid, mid.cover(CLEAR));
        assert!(puffs > 0.05 && puffs < 0.35, "clear-day puffs cover {puffs}");
        assert!(coverage(&high, high.cover(CLEAR)) > 0.05);
        // Rain: the low deck forms and the middle layer closes up.
        assert!(coverage(&low, low.cover(RAIN)) > 0.4);
        let lid = coverage(&mid, mid.cover(RAIN));
        assert!(lid > 0.75, "rain covers only {lid}");
    }

    #[test]
    fn layers_sit_below_the_sky_islands() {
        for l in LAYERS {
            assert!(l.y + l.thick < 340.0, "{}", l.name);
        }
        assert!(LAYERS.windows(2).all(|w| w[0].y < w[1].y));
    }
}
