//! Raymarched clouds in a slab from `CLOUD_Y` to `CLOUD_TOP`, below the sky
//! islands, so the tallest mountains poke through them.
//!
//! Two big horizontal planes follow the camera, one on the slab's floor and
//! one on its ceiling. The fragment shader (`assets/shaders/clouds.wesl`)
//! walks each view ray through the slab, sampling 3D noise for density and
//! looking towards the sun for light. Whichever plane the ray enters the slab
//! through draws it (from inside the slab, the floor draws downward rays and
//! the ceiling upward ones). The weather sets the coverage and the shade; the
//! wind carries the noise along.

use bevy::light::NotShadowCaster;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::camera::FlyCamera;
use crate::clock::{Seasons, WorldClock};
use crate::weather::Conditions;

/// Height of the cloud base and top.
pub const CLOUD_Y: f32 = 236.0;
pub const CLOUD_TOP: f32 = 290.0;
/// How far out clouds are drawn.
const REACH: f32 = 4000.0;

pub type CloudMaterial = ExtendedMaterial<StandardMaterial, CloudExtension>;

#[derive(ShaderType, Debug, Clone, Copy, Default)]
pub struct CloudParams {
    /// x: base, y: top, z: coverage 0..1, w: density
    pub slab: Vec4,
    /// xy: wind offset, z: time, w: how far to draw
    pub wind: Vec4,
    /// xyz: direction to the sun (or moon at night), w: 1 for the ceiling plane
    pub sun_dir: Vec4,
    /// w: 1 for fancy (more steps and detail)
    pub sun_color: Vec4,
    pub ambient: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CloudExtension {
    #[uniform(100)]
    pub params: CloudParams,
}

impl MaterialExtension for CloudExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/clouds.wesl".into()
    }
}

pub struct CloudPlugin;

impl Plugin for CloudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<CloudMaterial>::default())
            .insert_resource(Wind(Vec2::ZERO))
            .add_systems(Startup, spawn_clouds)
            .add_systems(Update, update_clouds);
    }
}

#[derive(Resource)]
struct Wind(Vec2);

/// One of the two planes; `top` is the ceiling.
#[derive(Component)]
struct CloudPlane {
    top: bool,
}

fn spawn_clouds(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<CloudMaterial>>) {
    let mesh = meshes.add(Plane3d::default().mesh().size(REACH * 2.0, REACH * 2.0));
    for top in [false, true] {
        let material = materials.add(CloudMaterial {
            base: StandardMaterial {
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                double_sided: true,
                fog_enabled: false,
                ..default()
            },
            extension: CloudExtension { params: CloudParams::default() },
        });
        commands.spawn((
            CloudPlane { top },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, if top { CLOUD_TOP } else { CLOUD_Y }, 0.0),
            NotShadowCaster,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn update_clouds(
    time: Res<Time>,
    weather: Res<Conditions>,
    clock: Res<WorldClock>,
    seasons: Res<Seasons>,
    mut wind: ResMut<Wind>,
    cam: Query<&Transform, (With<FlyCamera>, Without<CloudPlane>)>,
    mut q: Query<(&CloudPlane, &mut Transform, &MeshMaterial3d<CloudMaterial>, &mut Visibility)>,
    mut materials: ResMut<Assets<CloudMaterial>>,
    settings: Res<crate::settings::Settings>,
) {
    let Ok(cam) = cam.single() else { return };
    // The Clouds setting; `BLOCKGAME_NO_CLOUDS=1` turns them off (to measure what they cost).
    let quality = if std::env::var_os("BLOCKGAME_NO_CLOUDS").is_some() { 0 } else { settings.clouds };
    wind.0 += crate::weather::WIND_DIR * (weather.wind.max(1.5)) * time.delta_secs();
    let daylight = seasons.blended(seasons.at(&clock), |s| s.daylight);
    let sun = crate::sky::sun_direction(clock.time, daylight);
    // By day the sun lights the clouds; by night the moon, faintly.
    let day = (sun.y * 4.0 + 0.3).clamp(0.0, 1.0);
    let light_dir = if sun.y > -0.1 { sun } else { Vec3::new(-sun.x, -sun.y, -sun.z) };
    let low = (1.0 - sun.y.abs() * 4.0).clamp(0.0, 1.0) * day;
    let shade = weather.cloud_shade;
    let sun_col = Vec3::new(1.0, 0.97, 0.92).lerp(Vec3::new(1.0, 0.62, 0.38), low) * day + Vec3::new(0.12, 0.14, 0.2) * (1.0 - day);
    let ambient = Vec3::new(0.62, 0.68, 0.78) * day * shade + Vec3::new(0.035, 0.04, 0.06) * (1.0 - day);
    let flash = weather.flash;
    for (plane, mut tf, mat, mut vis) in &mut q {
        let want = if quality == 0 { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
        if quality == 0 {
            continue;
        }
        // Follow the camera, snapped so the plane never swims.
        tf.translation.x = (cam.translation.x / 64.0).round() * 64.0;
        tf.translation.z = (cam.translation.z / 64.0).round() * 64.0;
        let Some(mut m) = materials.get_mut(&mat.0) else { continue };
        m.extension.params = CloudParams {
            slab: Vec4::new(CLOUD_Y, CLOUD_TOP, weather.clouds.clamp(0.0, 1.0), 0.035 + weather.precipitation.clamp(0.0, 1.0) * 0.03),
            wind: Vec4::new(wind.0.x, wind.0.y, time.elapsed_secs(), REACH),
            sun_dir: light_dir.normalize().extend(if plane.top { 1.0 } else { 0.0 }),
            sun_color: (sun_col * shade + Vec3::splat(flash * 2.0)).extend(if quality >= 2 { 1.0 } else { 0.0 }),
            ambient: (ambient + Vec3::splat(flash)).extend(1.0),
        };
    }
}
