//! Weather: kinds from `assets/data/weather.ron`, picked by the season's
//! chances, blended smoothly into the `Conditions` everything else reads
//! (clouds, fog, light, rain and snow, wind, lightning).
//!
//! F4 cycles the weather by hand. `BLOCKGAME_WEATHER=rain` starts with a given
//! weather, for screenshots.

use bevy::prelude::*;
use serde::Deserialize;
use voxel::BlockPos;

use crate::camera::FlyCamera;
use crate::clock::{Seasons, WorldClock};
use crate::world::Generator;

/// Real seconds a weather change takes to blend in.
const BLEND_SECONDS: f32 = 30.0;

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        let kinds = WeatherKinds::from_ron(include_str!("../../../assets/data/weather.ron")).unwrap_or_else(|e| panic!("{e}"));
        let start = std::env::var("BLOCKGAME_WEATHER").ok().and_then(|n| kinds.find(&n));
        if let Ok(n) = std::env::var("BLOCKGAME_WEATHER")
            && start.is_none()
        {
            warn!("BLOCKGAME_WEATHER: no weather called '{n}' in weather.ron");
        }
        let first = start.unwrap_or(0);
        let conditions = Conditions::from_kind(&kinds.list[first]);
        app.insert_resource(kinds)
            .insert_resource(WeatherState { current: first, previous: first, blend: 1.0, until: None, forced: start.is_some() })
            .insert_resource(conditions)
            .init_resource::<LocalClimate>()
            .add_systems(Update, (sample_climate, change_weather, blend_conditions, lightning).chain());
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct WeatherKind {
    pub name: String,
    pub title: String,
    pub hours: (f32, f32),
    pub clouds: f32,
    pub cloud_shade: f32,
    pub visibility: f32,
    pub light: f32,
    pub precipitation: f32,
    pub wind: f32,
    pub lightning: bool,
    /// Added to the felt temperature, °C.
    #[serde(default)]
    pub chill: f32,
    /// How breathable the air is, 1 = normal (spores, gale winds).
    #[serde(default = "one")]
    pub air: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Resource, Clone, Debug)]
pub struct WeatherKinds {
    pub list: Vec<WeatherKind>,
}

impl WeatherKinds {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let list: Vec<WeatherKind> = ron::from_str(text).map_err(|e| format!("weather.ron: {e}"))?;
        if list.is_empty() {
            return Err("weather.ron needs at least one weather".into());
        }
        Ok(Self { list })
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.list.iter().position(|k| k.name.eq_ignore_ascii_case(name))
    }
}

#[derive(Resource, Debug)]
pub struct WeatherState {
    pub current: usize,
    pub previous: usize,
    /// 0 = all previous, 1 = all current.
    pub blend: f32,
    /// Total days (see `WorldClock::total_days`) when the weather next changes.
    until: Option<f64>,
    /// Set by `BLOCKGAME_WEATHER`: hold this weather.
    forced: bool,
}

/// The weather as everything else sees it, blended between kinds.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Conditions {
    pub clouds: f32,
    pub cloud_shade: f32,
    pub visibility: f32,
    pub light: f32,
    pub precipitation: f32,
    pub wind: f32,
    pub lightning: bool,
    /// Brightness of the current lightning flash, 0..1.
    pub flash: f32,
    /// Below freezing here: precipitation falls as snow.
    pub snowing: bool,
    /// Felt temperature change, °C, and breathability (see `WeatherKind`).
    pub chill: f32,
    pub air: f32,
}

impl Conditions {
    fn from_kind(k: &WeatherKind) -> Self {
        Self {
            clouds: k.clouds,
            cloud_shade: k.cloud_shade,
            visibility: k.visibility,
            light: k.light,
            precipitation: k.precipitation,
            wind: k.wind,
            lightning: k.lightning,
            flash: 0.0,
            snowing: false,
            chill: k.chill,
            air: k.air,
        }
    }
}

/// The climate around the camera: biome fog colour and haze, temperature with
/// the season applied. Eased so walking between biomes fades smoothly.
#[derive(Resource, Debug, Clone, Copy)]
pub struct LocalClimate {
    pub fog: Vec3,
    pub haze: f32,
    pub temperature: f32,
    target: (Vec3, f32, f32),
    timer: f32,
}

impl Default for LocalClimate {
    fn default() -> Self {
        let fog = Vec3::new(0.46, 0.53, 0.47);
        Self { fog, haze: 0.4, temperature: 0.4, target: (fog, 0.4, 0.4), timer: 0.0 }
    }
}

fn sample_climate(
    time: Res<Time>,
    generator: Option<Res<Generator>>,
    clock: Res<WorldClock>,
    seasons: Res<Seasons>,
    cam: Query<&Transform, With<FlyCamera>>,
    mut climate: ResMut<LocalClimate>,
) {
    let dt = time.delta_secs();
    climate.timer -= dt;
    if climate.timer <= 0.0
        && let (Some(g), Ok(tf)) = (generator, cam.single())
    {
        climate.timer = 0.5;
        let p = BlockPos::from_world(tf.translation.to_array());
        let biome = g.0.biome_at(p.x, p.y, p.z);
        let col = g.0.column(p.x, p.z);
        let season = seasons.blended(seasons.at(&clock), |s| s.temperature);
        // Colder with height above the ground too (sky islands, mountain air).
        let altitude = ((p.y - col.height.max(0)) as f32 / 400.0).max(0.0);
        let temperature = biome.temperature.min(col.climate.temperature) + season - altitude;
        climate.target = (Vec3::from(biome.fog), biome.haze, temperature);
    }
    let k = 1.0 - (-dt / 3.0).exp();
    let (fog, haze, temp) = climate.target;
    climate.fog = climate.fog.lerp(fog, k);
    climate.haze += (haze - climate.haze) * k;
    climate.temperature += (temp - climate.temperature) * k;
}

/// Small deterministic random numbers, so a world's weather repeats.
fn hash(a: u64, b: u64) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

fn change_weather(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    clock: Res<WorldClock>,
    seasons: Res<Seasons>,
    kinds: Res<WeatherKinds>,
    generator: Option<Res<Generator>>,
    mut state: ResMut<WeatherState>,
) {
    let now = clock.total_days();
    let seed = generator.map(|g| g.0.seed()).unwrap_or(1);
    let mut next = None;
    if keys.just_pressed(KeyCode::F4) {
        next = Some((state.current + 1) % kinds.list.len());
        state.forced = false;
    } else if !state.forced && state.until.is_none_or(|u| now >= u) {
        // Pick by the season's chances. The roll depends on the seed and the
        // hour, so the same world has the same weather.
        let season = &seasons.list[seasons.at(&clock).current];
        let weights: Vec<f32> = kinds.list.iter().map(|k| season.weather.get(&k.name).copied().unwrap_or(0.0)).collect();
        let total: f32 = weights.iter().sum();
        let slot = (now * 24.0) as u64;
        if state.until.is_some() && total > 0.0 {
            let mut roll = hash(seed, slot) * total;
            let mut pick = 0;
            for (i, w) in weights.iter().enumerate() {
                if roll < *w {
                    pick = i;
                    break;
                }
                roll -= w;
            }
            next = Some(pick);
        }
        let k = &kinds.list[next.unwrap_or(state.current)];
        let hours = k.hours.0 + (k.hours.1 - k.hours.0) * hash(seed ^ 0xAB, slot);
        state.until = Some(now + hours as f64 / 24.0);
    }
    if let Some(n) = next
        && n != state.current
    {
        info!("weather: {} -> {}", kinds.list[state.current].name, kinds.list[n].name);
        state.previous = state.current;
        state.current = n;
        state.blend = 0.0;
    }
    state.blend = (state.blend + time.delta_secs() / BLEND_SECONDS).min(1.0);
}

fn blend_conditions(
    state: Res<WeatherState>,
    kinds: Res<WeatherKinds>,
    climate: Res<LocalClimate>,
    mut c: ResMut<Conditions>,
) {
    let (a, b) = (&kinds.list[state.previous], &kinds.list[state.current]);
    let t = state.blend * state.blend * (3.0 - 2.0 * state.blend);
    let mix = |x: f32, y: f32| x + (y - x) * t;
    c.clouds = mix(a.clouds, b.clouds);
    c.cloud_shade = mix(a.cloud_shade, b.cloud_shade);
    // Blend visibility in log space so mist rolls in gradually.
    c.visibility = (mix(a.visibility.ln(), b.visibility.ln())).exp();
    c.light = mix(a.light, b.light);
    c.precipitation = mix(a.precipitation, b.precipitation);
    c.wind = mix(a.wind, b.wind);
    c.chill = mix(a.chill, b.chill);
    c.air = mix(a.air, b.air);
    c.lightning = b.lightning && state.blend > 0.5;
    c.snowing = climate.temperature < 0.0;
}

fn lightning(time: Res<Time>, mut c: ResMut<Conditions>, mut next: Local<f32>, mut t: Local<f32>) {
    let dt = time.delta_secs();
    *t += dt;
    c.flash = (c.flash - dt * 4.0).max(0.0);
    if !c.lightning {
        *next = 0.0;
        return;
    }
    if *next == 0.0 {
        *next = *t + 4.0 + hash(t.to_bits() as u64, 7) * 14.0;
    }
    if *t >= *next {
        c.flash = 1.0;
        // Often a second flicker a moment later.
        *next = if hash(t.to_bits() as u64, 3) < 0.5 { *t + 0.18 } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_file_parses_and_seasons_name_real_weather() {
        let kinds = WeatherKinds::from_ron(include_str!("../../../assets/data/weather.ron")).unwrap();
        let seasons = Seasons::from_ron(include_str!("../../../assets/data/seasons.ron")).unwrap();
        for s in &seasons.list {
            for name in s.weather.keys() {
                assert!(kinds.find(name).is_some(), "season {} names unknown weather {name}", s.name);
            }
        }
        for k in &kinds.list {
            assert!(k.visibility > 0.0 && k.hours.0 <= k.hours.1, "{}", k.name);
        }
    }

    #[test]
    fn hash_is_in_unit_range() {
        for i in 0..1000 {
            let h = hash(42, i);
            assert!((0.0..1.0).contains(&h));
        }
    }
}
