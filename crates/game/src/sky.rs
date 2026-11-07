//! Sun, moon, sky and fog, driven by the clock, the season, the weather and
//! the biome around the camera.
//!
//! One directional light is the sun by day and the moon by night. The sky is a
//! big sphere that follows the camera with its own shader (`shaders/sky.wesl`);
//! its horizon colour is also the fog colour, so far-off land melts into it.

use std::f32::consts::{PI, TAU};

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::blocks::{Blocks, VoxelMaterial};
use crate::camera::FlyCamera;
use crate::clock::{Seasons, WorldClock};
use crate::weather::{Conditions, LocalClimate};

/// Radius of the sky sphere. The camera's far plane is beyond it.
pub const SKY_RADIUS: f32 = 4500.0;
/// Fog never reaches further than this: the edge of the distant terrain.
pub const MAX_VISIBILITY: f32 = 1500.0;
/// How far the sun's path leans away from straight overhead (radians).
const SUN_TILT: f32 = 0.38;
/// The moon's path leans differently, so it crosses a different part of the sky.
const MOON_TILT: f32 = 0.62;
/// Days from one full moon to the next. The moon rises 24/8 = 3 hours later each day.
pub const LUNAR_DAYS: f64 = 8.0;

pub type SkyMaterial = ExtendedMaterial<StandardMaterial, SkyExtension>;

#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct SkyParams {
    pub zenith: Vec4,
    pub horizon: Vec4,
    pub sun_dir: Vec4,
    pub sun_color: Vec4,
    pub moon_dir: Vec4,
    pub extra: Vec4,
    /// Rows of the matrix that turns a view direction into star-map space,
    /// so the stars wheel across the sky around the same axis as the sun.
    pub star_x: Vec4,
    pub star_y: Vec4,
    pub star_z: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SkyExtension {
    #[uniform(100)]
    pub params: SkyParams,
}

impl MaterialExtension for SkyExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/sky.wesl".into()
    }
}

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .insert_resource(ClearColor(Color::srgb(0.46, 0.53, 0.47)))
            .insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 400.0, ..default() })
            .add_systems(Startup, spawn_sky)
            .add_systems(Update, (update_sky, follow_camera, season_tint));
    }
}

#[derive(Component)]
struct Sun;

#[derive(Component)]
struct SkyDome;

fn spawn_sky(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<SkyMaterial>>) {
    commands.spawn((
        Sun,
        DirectionalLight { illuminance: 9_000.0, shadow_maps_enabled: true, ..default() },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.6), Vec3::Y),
    ));
    commands.spawn((
        SkyDome,
        Mesh3d(meshes.add(Sphere::new(SKY_RADIUS).mesh().uv(48, 24))),
        MeshMaterial3d(materials.add(SkyMaterial {
            base: StandardMaterial {
                unlit: true,
                fog_enabled: false,
                cull_mode: None,
                ..default()
            },
            extension: SkyExtension { params: SkyParams::default() },
        })),
        Transform::default(),
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

/// Direction to the sun for a time of day (0..1) when the sun is up for
/// `daylight` of the day: rises at 0.5 - daylight/2, sets at 0.5 + daylight/2.
pub fn sun_direction(time: f32, daylight: f32) -> Vec3 {
    let rise = 0.5 - daylight * 0.5;
    let set = 0.5 + daylight * 0.5;
    let angle = if (rise..set).contains(&time) {
        (time - rise) / (set - rise) * PI
    } else {
        let night = (time - set).rem_euclid(1.0) / (1.0 - (set - rise));
        PI + night * PI
    };
    Vec3::new(angle.cos(), angle.sin() * SUN_TILT.cos(), angle.sin() * SUN_TILT.sin()).normalize()
}

/// Axis the sky turns around: perpendicular to the sun's path.
fn sky_axis() -> Vec3 {
    Vec3::new(0.0, -SUN_TILT.sin(), SUN_TILT.cos())
}

/// How the sky has turned at this time of day (stars), one turn per day.
fn sky_rotation(time: f32) -> Quat {
    Quat::from_axis_angle(sky_axis(), (time - 0.25) * TAU)
}

/// Moon phase, 0..1: 0 new moon, 0.5 full moon.
pub fn moon_phase(clock: &WorldClock) -> f32 {
    // Start a new world just after full moon so the first nights have light.
    ((clock.total_days() / LUNAR_DAYS + 0.45).rem_euclid(1.0)) as f32
}

/// Share of the moon's face that is lit, 0..1.
pub fn moon_lit(phase: f32) -> f32 {
    (1.0 - (phase * TAU).cos()) * 0.5
}

pub fn moon_phase_name(phase: f32) -> &'static str {
    const NAMES: [&str; 8] = [
        "new moon", "waxing crescent", "first quarter", "waxing gibbous",
        "full moon", "waning gibbous", "last quarter", "waning crescent",
    ];
    NAMES[((phase * 8.0 + 0.5) as usize) % 8]
}

/// Direction to the moon. It runs its own course: each day it lags the sun
/// by a further 1/LUNAR_DAYS of a turn (so it is opposite the sun at full
/// moon, beside it at new moon), on a path tilted differently from the sun's.
pub fn moon_direction(time: f32, phase: f32) -> Vec3 {
    let a = (time - 0.25 - phase) * TAU;
    Vec3::new(a.cos(), a.sin() * MOON_TILT.cos(), a.sin() * MOON_TILT.sin()).normalize()
}

fn srgb(r: f32, g: f32, b: f32) -> Vec3 {
    let c = Color::srgb(r, g, b).to_linear();
    Vec3::new(c.red, c.green, c.blue)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn luma(c: Vec3) -> f32 {
    c.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

#[allow(clippy::too_many_arguments)]
fn update_sky(
    clock: Res<WorldClock>,
    seasons: Res<Seasons>,
    weather: Res<Conditions>,
    climate: Res<LocalClimate>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut sun_q: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut fog_q: Query<&mut DistanceFog>,
    dome: Query<&MeshMaterial3d<SkyMaterial>, With<SkyDome>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let season = seasons.at(&clock);
    let daylight_share = seasons.blended(season, |s| s.daylight);
    let sun = sun_direction(clock.time, daylight_share);
    let phase = moon_phase(&clock);
    let lit = moon_lit(phase);
    let moon = moon_direction(clock.time, phase);

    let day = smoothstep(-0.1, 0.22, sun.y);
    let sunset = smoothstep(-0.22, 0.0, sun.y) * (1.0 - smoothstep(0.0, 0.3, sun.y));
    let cloudy = smoothstep(0.3, 0.95, weather.clouds);
    let dim = 0.45 + 0.55 * weather.light;

    // Sky colours.
    let mut zenith = srgb(0.012, 0.018, 0.05).lerp(srgb(0.3, 0.5, 0.86), day);
    let mut horizon = srgb(0.035, 0.045, 0.085).lerp(srgb(0.68, 0.8, 0.93), day);
    let sunset_color = srgb(1.0, 0.5, 0.22);
    horizon = horizon.lerp(sunset_color * 0.8, sunset * 0.45);
    zenith = zenith.lerp(srgb(0.32, 0.3, 0.55), sunset * 0.35);
    // The local biome tints the haze (green-grey over the swamp, sandy over dunes).
    let biome = Vec3::from_array(climate.fog.to_array()).powf(2.2) * (0.06 + 0.94 * day);
    horizon = horizon.lerp(biome, 0.45);
    // Clouds and rain wash the colour out.
    horizon = horizon.lerp(Vec3::splat(luma(horizon)), cloudy * 0.7);
    zenith = zenith.lerp(horizon * 0.95, cloudy * 0.85);
    zenith *= dim;
    horizon *= dim;
    let flash = weather.flash;
    horizon += Vec3::splat(flash * 0.35);

    clear.0 = Color::linear_rgb(horizon.x, horizon.y, horizon.z);

    // Sun or moon light.
    let sun_amount = smoothstep(-0.02, 0.15, sun.y);
    let moon_amount = smoothstep(0.02, 0.18, moon.y);
    let warm = smoothstep(0.0, 0.35, sun.y);
    if let Ok((mut light, mut tf)) = sun_q.single_mut() {
        let sun_lux = 11_000.0 * sun_amount * weather.light;
        let moon_lux = 300.0 * moon_amount * lit * weather.light.sqrt();
        let (dir, lux, color) = if sun_lux >= moon_lux {
            let c = srgb(1.0, 0.62, 0.38).lerp(srgb(1.0, 0.97, 0.92), warm);
            (sun, sun_lux, c)
        } else {
            (moon, moon_lux, srgb(0.62, 0.72, 1.0))
        };
        light.illuminance = lux;
        light.color = Color::linear_rgb(color.x, color.y, color.z);
        // Point the light away from the sun (or moon); keep it off the
        // horizontal so shadows never stretch to infinity.
        let d = Vec3::new(dir.x, dir.y.max(0.08), dir.z).normalize();
        *tf = Transform::default().looking_to(-d, Vec3::Y);
    }

    // Ambient: sky light, enough to see by under the moon.
    let sky_tint = (zenith * 0.4 + horizon * 0.6).normalize_or_zero() * 1.7;
    ambient.color = Color::linear_rgb(sky_tint.x.min(1.0), sky_tint.y.min(1.0), sky_tint.z.min(1.0));
    // Brighter nights under a full moon that is up.
    let moonlight = 1.0 + 0.8 * lit * moon_amount;
    ambient.brightness = (70.0 * moonlight + 560.0 * day) * (0.55 + 0.45 * weather.light) + flash * 2500.0;

    // Fog.
    let visibility = (weather.visibility * climate.haze).clamp(30.0, MAX_VISIBILITY);
    for mut fog in &mut fog_q {
        fog.color = Color::linear_rgb(horizon.x, horizon.y, horizon.z);
        let glow = sunset_color * (sunset * (1.0 - cloudy));
        fog.directional_light_color = Color::linear_rgba(glow.x, glow.y, glow.z, 0.6 * sunset);
        fog.directional_light_exponent = 12.0;
        fog.falloff = FogFalloff::Linear { start: visibility * 0.2, end: visibility };
    }

    if let Ok(handle) = dome.single()
        && let Some(mut mat) = materials.get_mut(&handle.0)
    {
        let sc = srgb(1.0, 0.55, 0.28).lerp(srgb(1.0, 0.95, 0.85), warm);
        // Inverse rotation, as columns; the shader uses them as rows (transpose = inverse).
        let inv = Mat3::from_quat(sky_rotation(clock.time));
        mat.extension.params = SkyParams {
            zenith: zenith.extend(1.0),
            horizon: horizon.extend(1.0),
            sun_dir: sun.extend(sunset * (1.0 - cloudy * 0.8)),
            sun_color: sc.extend(1.0),
            moon_dir: moon.extend(0.35 + 0.65 * (1.0 - day)),
            extra: Vec4::new((1.0 - smoothstep(-0.3, 0.02, sun.y)) * 1.4, lit, flash * 0.6, cloudy),
            star_x: inv.x_axis.extend(0.0),
            star_y: inv.y_axis.extend(0.0),
            star_z: inv.z_axis.extend(0.0),
        };
    }
}

fn follow_camera(cam: Query<&Transform, (With<FlyCamera>, Without<SkyDome>)>, mut dome: Query<&mut Transform, With<SkyDome>>) {
    if let (Ok(c), Ok(mut d)) = (cam.single(), dome.single_mut()) {
        d.translation = c.translation;
    }
}

/// Tint grass and leaves for the season (see `foliage` in seasons.ron).
fn season_tint(
    clock: Res<WorldClock>,
    seasons: Res<Seasons>,
    blocks: Res<Blocks>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
    mut last: Local<Vec4>,
) {
    let p = seasons.at(&clock);
    let (a, b) = (seasons.list[p.current].foliage, seasons.list[p.next].foliage);
    let ca = Vec3::new(a.color.0, a.color.1, a.color.2);
    let cb = Vec3::new(b.color.0, b.color.1, b.color.2);
    let tint = ca.lerp(cb, p.blend).extend(a.amount + (b.amount - a.amount) * p.blend);
    if (tint - *last).abs().max_element() < 0.005 {
        return;
    }
    let mut done = false;
    for h in [&blocks.material, &blocks.translucent_material].into_iter().flatten() {
        if let Some(mut m) = materials.get_mut(h) {
            m.extension.season = tint;
            done = true;
        }
    }
    if done {
        *last = tint;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_turn_with_the_sun() {
        // At the equinox the sun is a point fixed on the star map: turning
        // the sky carries it exactly along the sun's path.
        for t in [0.3, 0.5, 0.7] {
            let sun_on_map = sky_rotation(t).inverse() * sun_direction(t, 0.5);
            assert!((sun_on_map - Vec3::X).length() < 1e-4, "{t}: {sun_on_map}");
        }
    }

    #[test]
    fn moon_phases_follow_its_place_in_the_sky() {
        // Full moon: opposite the sun. New moon: next to it.
        let full = moon_direction(0.5, 0.5);
        assert!(full.dot(sun_direction(0.5, 0.5)) < -0.9);
        let new = moon_direction(0.5, 0.0);
        assert!(new.dot(sun_direction(0.5, 0.5)) > 0.9);
        assert!(moon_lit(0.5) > 0.99 && moon_lit(0.0) < 0.01);
        assert_eq!(moon_phase_name(0.5), "full moon");
        assert_eq!(moon_phase_name(0.99), "new moon");
        // The moon rises later each day: at the same hour it sits further back.
        let c = |day| WorldClock { day, time: 0.0, speed: 1.0 };
        let (p0, p1) = (moon_phase(&c(0)), moon_phase(&c(1)));
        assert!(((p1 - p0).rem_euclid(1.0) - 1.0 / LUNAR_DAYS as f32).abs() < 1e-4);
    }

    #[test]
    fn sun_rises_and_sets_on_time() {
        // Equinox: up at 06:00, highest at noon, down at 18:00.
        assert!(sun_direction(0.25, 0.5).y.abs() < 1e-4);
        assert!(sun_direction(0.75, 0.5).y.abs() < 1e-4);
        assert!(sun_direction(0.5, 0.5).y > 0.9);
        assert!(sun_direction(0.0, 0.5).y < -0.9);
        // Longer summer day: still up at 07:00 in the evening.
        assert!(sun_direction(19.0 / 24.0, 0.6).y > 0.0);
        assert!(sun_direction(19.0 / 24.0, 0.5).y < 0.0);
    }
}
