//! Wind: one direction and strength that the clouds, rain, snow, gusts that
//! push you, grass and leaves all follow.
//!
//! The strength eases towards the weather's `wind` (weather.ron) over about
//! twenty seconds. The direction never jumps: it wanders slowly by itself
//! (two long, slow swings) and each new weather veers it a little, eased in
//! over a couple of minutes, so a gale can come from a new quarter but the
//! clouds turn gradually. Clouds drift with the square root of the wind
//! (`Wind::cloud_drift`), so a gale moves them faster but not wildly.
//!
//! Foliage sways in the vertex shader (`assets/shaders/voxel_vertex.wesl`);
//! this module hands it the direction and how hard to sway through the voxel
//! material's `wind` uniform. Gusts come and go on top of the steady wind.

use bevy::prelude::*;

use crate::blocks::{Blocks, VoxelMaterial};
use crate::weather::{Conditions, WeatherSet, WeatherState};
use crate::world::Generator;

/// Seconds for the wind's strength to follow a weather change (about 63%).
const SPEED_EASE: f32 = 20.0;
/// Seconds for the wind to swing round to a new weather's quarter.
const VEER_EASE: f32 = 100.0;
/// Most a new weather veers the wind from its usual quarter, radians.
const MAX_VEER: f32 = 0.9;
/// Cloud drift in blocks per second is this times the square root of the wind.
const CLOUD_DRIFT: f32 = 1.0;
/// Wind (blocks per second) at which plants lean as far as they go.
const FULL_SWAY_WIND: f32 = 16.0;

pub struct WindPlugin;

impl Plugin for WindPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Wind>().add_systems(crate::menu::GameUpdate, (blow, push_to_material).chain().after(WeatherSet));
    }
}

/// The wind everything reads.
#[derive(Resource, Debug, Clone)]
pub struct Wind {
    /// Direction it blows towards, radians in the x-z plane (0 = +x, π/2 = +z).
    pub angle: f32,
    /// Blocks per second.
    pub speed: f32,
    /// 0..1, how gusty it is right now.
    pub gust: f32,
    /// Seconds of wind so far (drives the slow wander and the gusts).
    time: f32,
    /// The world's usual quarter, from the seed.
    base: f32,
    /// How far the current weather veers it from `base`, eased.
    veer: f32,
    /// The weather the veer was picked for.
    weather: Option<(usize, u64)>,
    started: bool,
}

impl Default for Wind {
    fn default() -> Self {
        Self { angle: 0.33, speed: 0.0, gust: 0.0, time: 0.0, base: 0.33, veer: 0.0, weather: None, started: false }
    }
}

impl Wind {
    /// Unit direction it blows towards, as (x, z).
    pub fn dir(&self) -> Vec2 {
        Vec2::from_angle(self.angle)
    }

    /// Wind velocity, blocks per second, as (x, z).
    pub fn velocity(&self) -> Vec2 {
        self.dir() * self.speed
    }

    /// How fast the clouds drift, blocks per second (before each layer's own speed).
    pub fn cloud_drift(&self) -> f32 {
        cloud_drift(self.speed)
    }

    /// How hard foliage sways: 0 still, 1 a strong wind, a bit more in gusts.
    pub fn sway(&self) -> f32 {
        sway(self.speed) * (1.0 + 0.5 * self.gust)
    }

    /// Compass point the wind comes *from* ("NE"), with north as -z.
    pub fn compass(&self) -> &'static str {
        const POINTS: [&str; 8] = ["E", "SE", "S", "SW", "W", "NW", "N", "NE"];
        // `angle` points where it blows to; it comes from the opposite side.
        let from = (self.angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU);
        POINTS[((from / std::f32::consts::FRAC_PI_4).round() as usize) % 8]
    }
}

pub fn cloud_drift(wind: f32) -> f32 {
    CLOUD_DRIFT * wind.max(0.0).sqrt()
}

pub fn sway(wind: f32) -> f32 {
    (wind / FULL_SWAY_WIND).clamp(0.08, 1.3)
}

/// The slow wander of the wind's direction, radians, `t` seconds in. Two
/// slow swings (20 and 8 minutes); it never turns faster than ~0.6° a second.
pub fn wander(t: f32) -> f32 {
    use std::f32::consts::TAU;
    0.9 * (t * TAU / 1200.0).sin() + 0.4 * (t * TAU / 480.0 + 2.1).sin()
}

/// Gustiness, 0..1, `t` seconds in: a couple of slow beats that line up now and then.
pub fn gusts(t: f32) -> f32 {
    let a = (t * 0.37).sin() * 0.5 + 0.5;
    let b = (t * 0.13 + 1.0).sin() * 0.5 + 0.5;
    (a * b).powf(1.5)
}

fn hash(a: u64, b: u64) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    x ^= x >> 32;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

fn blow(time: Res<Time>, conditions: Res<Conditions>, state: Res<WeatherState>, generator: Option<Res<Generator>>, mut wind: ResMut<Wind>) {
    let dt = time.delta_secs().min(0.25);
    let seed = generator.map(|g| g.0.seed()).unwrap_or(1);
    if !wind.started {
        wind.started = true;
        wind.base = hash(seed, 0x57_1ED) * std::f32::consts::TAU;
        wind.speed = conditions.wind;
    }
    // Each weather (and each time it comes round again) veers the wind a little.
    let key = (state.current, (state.since * 24.0) as u64);
    let target = (hash(seed ^ key.1, key.0 as u64 + 7) * 2.0 - 1.0) * MAX_VEER;
    if wind.weather.is_none() {
        wind.veer = target;
    }
    wind.weather = Some(key);
    wind.time += dt;
    wind.veer += (target - wind.veer) * (1.0 - (-dt / VEER_EASE).exp());
    wind.speed += (conditions.wind - wind.speed) * (1.0 - (-dt / SPEED_EASE).exp());
    wind.angle = wind.base + wind.veer + wander(wind.time);
    wind.gust = gusts(wind.time);
}

/// Hand the terrain shader the wind: (x, z) direction, sway strength.
fn push_to_material(wind: Res<Wind>, blocks: Res<Blocks>, mut materials: ResMut<Assets<VoxelMaterial>>, mut last: Local<Vec4>) {
    let d = wind.dir();
    let want = Vec4::new(d.x, d.y, wind.sway(), 0.0);
    if (want - *last).abs().max_element() < 0.01 {
        return;
    }
    let mut done = false;
    for h in [&blocks.material, &blocks.translucent_material].into_iter().flatten() {
        if let Some(mut m) = materials.get_mut(h) {
            m.extension.wind = want;
            done = true;
        }
    }
    if done {
        *last = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wind_turns_slowly() {
        // Never more than about half a degree a second, however long it blows.
        let mut t = 0.0;
        while t < 3600.0 {
            let rate = (wander(t + 1.0) - wander(t)).abs();
            assert!(rate < 0.65f32.to_radians(), "turns {}° in a second at {t}", rate.to_degrees());
            t += 1.0;
        }
        // A new weather's veer comes in over minutes, not seconds.
        let step = MAX_VEER * 2.0 * (1.0 - (-1.0f32 / VEER_EASE).exp());
        assert!(step < 1.1f32.to_radians(), "{}", step.to_degrees());
    }

    #[test]
    fn gales_move_clouds_faster_but_not_wildly() {
        let (clear, storm, gale) = (cloud_drift(3.0), cloud_drift(12.0), cloud_drift(24.0));
        assert!(clear < storm && storm < gale);
        assert!(clear > 1.0, "clear-day clouds still drift: {clear}");
        assert!(gale < 6.0, "gale clouds race at {gale} blocks/s");
    }

    #[test]
    fn foliage_sways_more_in_stronger_wind() {
        assert!(sway(0.0) > 0.0, "a little breeze always");
        assert!(sway(3.0) < sway(12.0) && sway(12.0) < sway(24.0));
        assert!(sway(100.0) <= 1.3);
        for t in 0..1000 {
            let g = gusts(t as f32 * 0.7);
            assert!((0.0..=1.0).contains(&g));
        }
    }

    #[test]
    fn compass_names_where_it_comes_from() {
        let w = |angle| Wind { angle, ..default() };
        // Blowing towards +x (east) means it comes from the west.
        assert_eq!(w(0.0).compass(), "W");
        // Blowing towards -z (north) comes from the south.
        assert_eq!(w(-std::f32::consts::FRAC_PI_2).compass(), "S");
    }
}
