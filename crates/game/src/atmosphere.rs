//! The look of the world on top of plain lighting: fog that takes the sky's
//! colour, ground mist, colour grading and bloom.
//!
//! - **Sky-matched fog.** Terrain (block chunks, water, distant LOD tiles) is
//!   fogged by its own shader code instead of Bevy's distance fog: it fades
//!   into the sky's horizon colour *in the direction you look*, warm and bright
//!   towards the sun, so far mountains melt into exactly the sky behind them.
//!   `sky::update_sky` fills [`Atmos::params`] with the same numbers the sky
//!   dome uses; the formula lives in `voxel.wesl`, `lod.wesl` and `sky.wesl`.
//! - **Ground mist.** Height fog, thick at the waterline and thinning upwards,
//!   integrated along each view ray; from weather, biome and dawn.
//! - **Colour grading.** Contrast, saturation, warmth and exposure from
//!   `assets/data/atmosphere.ron` by season, weather, biome and night, on
//!   Bevy's `ColorGrading`. The camera renders in HDR with bloom, so torches,
//!   the sun and fireflies glow.

use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;
use bevy::render::view::ColorGrading;
use serde::Deserialize;
use std::collections::HashMap;
use voxel::BlockPos;

use crate::blocks::{Blocks, VoxelMaterial};
use crate::camera::FlyCamera;
use crate::clock::{Seasons, WorldClock};
use crate::lod::{LodMaterial, LodTiles};
use crate::weather::{WeatherKinds, WeatherState};
use crate::world::Generator;

/// Height of the bottom of the ground mist (just above the swamp's water).
const MIST_BASE: f32 = worldgen::SEA_LEVEL as f32 + 2.0;
/// Mist density per block at its base, for mist 1.
const MIST_DENSITY: f32 = 0.016;

/// What the terrain shaders need to fog like the sky. Same layout as `Atmos`
/// in `voxel.wesl` and `lod.wesl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub struct AtmosParams {
    /// rgb: the sky's colour at the horizon (linear); w: where fog starts (blocks).
    pub horizon: Vec4,
    /// rgb: colour of the sun's glow at the horizon; w: where fog is complete.
    pub glow: Vec4,
    /// xyz: direction to the sun; w: how much sunrise/sunset glow.
    pub sun: Vec4,
    /// x: height of the mist's base, y: density there (per block), z: how many
    /// blocks it takes to thin by e; w: daytime haze brightening towards the sun.
    pub mist: Vec4,
}

/// The atmosphere this frame. The sky fills the colours; this module adds the
/// mist and pushes it all to the materials.
#[derive(Resource, Default, Debug)]
pub struct Atmos {
    pub params: AtmosParams,
    /// Sunrise and sunset colour for the season (linear), read by the sky.
    pub sunset: Vec3,
    /// 0 by day, 1 at night.
    pub night: f32,
    /// 1 around sunrise, 0 otherwise (morning mist).
    pub dawn: f32,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct Grade {
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub warmth: f32,
    pub mist: f32,
    pub sunset: Option<(f32, f32, f32)>,
}

impl Default for Grade {
    fn default() -> Self {
        Self { exposure: 0.0, contrast: 1.0, saturation: 1.0, warmth: 0.0, mist: 0.0, sunset: None }
    }
}

impl Grade {
    /// Both grades applied: factors multiply, offsets add.
    pub fn and(self, o: Grade) -> Grade {
        Grade {
            exposure: self.exposure + o.exposure,
            contrast: self.contrast * o.contrast,
            saturation: self.saturation * o.saturation,
            warmth: self.warmth + o.warmth,
            mist: self.mist + o.mist,
            sunset: o.sunset.or(self.sunset),
        }
    }

    /// Part of the way from `self` to `o`.
    pub fn lerp(self, o: Grade, t: f32) -> Grade {
        let l = |a: f32, b: f32| a + (b - a) * t;
        let sunset = match (self.sunset, o.sunset) {
            (Some(a), Some(b)) => Some((l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))),
            (a, b) => if t < 0.5 { a } else { b },
        };
        Grade {
            exposure: l(self.exposure, o.exposure),
            contrast: l(self.contrast, o.contrast),
            saturation: l(self.saturation, o.saturation),
            warmth: l(self.warmth, o.warmth),
            mist: l(self.mist, o.mist),
            sunset,
        }
    }
}

/// `assets/data/atmosphere.ron`.
#[derive(Resource, Deserialize, Clone, Debug)]
pub struct Looks {
    pub base: Grade,
    pub night: Grade,
    pub dawn_mist: f32,
    pub seasons: HashMap<String, Grade>,
    pub weather: HashMap<String, Grade>,
    pub biomes: HashMap<String, Grade>,
}

impl Looks {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| format!("assets/data/atmosphere.ron: {e}"))
    }

    fn season(&self, name: &str) -> Grade {
        self.seasons.get(name).copied().unwrap_or_default()
    }

    fn weather(&self, name: &str) -> Grade {
        self.weather.get(name).copied().unwrap_or_default()
    }
}

/// The grade in effect, eased so walking between biomes fades smoothly.
#[derive(Resource, Default)]
struct LocalLook {
    biome: Grade,
    target: Grade,
    timer: f32,
}

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        let looks = Looks::from_ron(include_str!("../../../assets/data/atmosphere.ron")).unwrap_or_else(|e| panic!("{e}"));
        app.insert_resource(looks)
            .init_resource::<Atmos>()
            .init_resource::<LocalLook>()
            .add_systems(
                crate::menu::GameUpdate,
                (
                    season_sunset.before(crate::sky::update_sky),
                    (sample_biome, apply).chain().after(crate::sky::update_sky),
                ),
            );
    }
}

/// The season's sunset colour, for the sky.
fn season_sunset(clock: Res<WorldClock>, seasons: Res<Seasons>, looks: Res<Looks>, mut atmos: ResMut<Atmos>) {
    let p = seasons.at(&clock);
    let a = looks.season(&seasons.list[p.current].name);
    let b = looks.season(&seasons.list[p.next].name);
    let c = a.lerp(b, p.blend).sunset.unwrap_or((1.0, 0.5, 0.22));
    let lin = Color::srgb(c.0, c.1, c.2).to_linear();
    atmos.sunset = Vec3::new(lin.red, lin.green, lin.blue);
}

fn sample_biome(
    time: Res<Time>,
    generator: Option<Res<Generator>>,
    looks: Res<Looks>,
    cam: Query<&Transform, With<FlyCamera>>,
    mut look: ResMut<LocalLook>,
) {
    let dt = time.delta_secs();
    look.timer -= dt;
    if look.timer <= 0.0
        && let (Some(g), Ok(tf)) = (generator, cam.single())
    {
        look.timer = 0.5;
        let p = BlockPos::from_world(tf.translation.to_array());
        let biome = g.0.biome_at(p.x, p.y, p.z);
        look.target = looks.biomes.get(&biome.name).copied().unwrap_or_default();
    }
    let k = 1.0 - (-dt / 3.0).exp();
    look.biome = look.biome.lerp(look.target, k);
}

/// Soft bloom: only really bright things (the sun, torch flames, fireflies,
/// lightning) glow, so the pixel art stays crisp. `BLOCKGAME_NO_BLOOM=1` turns
/// it off for comparisons.
pub fn bloom() -> Bloom {
    let intensity = if std::env::var("BLOCKGAME_NO_BLOOM").is_ok() { 0.0 } else { 0.12 };
    Bloom {
        intensity,
        low_frequency_boost: 0.4,
        prefilter: BloomPrefilter { threshold: 1.1, threshold_softness: 0.5 },
        composite_mode: BloomCompositeMode::Additive,
        ..Bloom::NATURAL
    }
}

/// Combine every grade into the one in effect now.
pub fn current_grade(looks: &Looks, season: Grade, weather: Grade, biome: Grade, night: f32) -> Grade {
    let day = looks.base.and(season).and(weather).and(biome);
    // Night replaces some of the day's look rather than adding to it: colour
    // drains away whatever the season.
    let mut g = day.lerp(day.and(looks.night), night);
    g.saturation = g.saturation.max(0.0);
    g
}

#[allow(clippy::too_many_arguments)]
fn apply(
    clock: Res<WorldClock>,
    seasons: Res<Seasons>,
    kinds: Res<WeatherKinds>,
    state: Res<WeatherState>,
    looks: Res<Looks>,
    look: Res<LocalLook>,
    mut atmos: ResMut<Atmos>,
    blocks: Res<Blocks>,
    lod: Res<LodTiles>,
    mut voxel_materials: ResMut<Assets<VoxelMaterial>>,
    mut lod_materials: ResMut<Assets<LodMaterial>>,
    mut cam: Query<(&Transform, &mut ColorGrading), With<FlyCamera>>,
    mut last: Local<AtmosParams>,
) {
    let p = seasons.at(&clock);
    let season = looks.season(&seasons.list[p.current].name).lerp(looks.season(&seasons.list[p.next].name), p.blend);
    let t = state.blend * state.blend * (3.0 - 2.0 * state.blend);
    let weather = looks.weather(&kinds.list[state.previous].name).lerp(looks.weather(&kinds.list[state.current].name), t);
    let grade = current_grade(&looks, season, weather, look.biome, atmos.night);

    let Ok((tf, mut grading)) = cam.single_mut() else { return };
    // Mist: weather, biome and morning, and none deep underground.
    let amount = (grade.mist + looks.dawn_mist * atmos.dawn).max(0.0);
    let underground = 1.0 - ((tf.translation.y - (MIST_BASE - 30.0)) / 22.0).clamp(0.0, 1.0);
    atmos.params.mist.x = MIST_BASE;
    atmos.params.mist.y = amount * MIST_DENSITY * (1.0 - underground);
    atmos.params.mist.z = 7.0 + 6.0 * amount.min(1.5);

    set_grading(&mut grading, grade);

    // Re-send to the GPU only when it visibly changes.
    let params = atmos.params;
    let moved = (params.horizon - last.horizon).abs().max_element() > 0.0015
        || (params.glow - last.glow).abs().max_element() > 0.0015
        || (params.sun - last.sun).abs().max_element() > 0.002
        || (params.mist - last.mist).abs().max_element() > 0.0002;
    if !moved {
        return;
    }
    let mut sent = false;
    for h in [&blocks.material, &blocks.translucent_material].into_iter().flatten() {
        if let Some(mut m) = voxel_materials.get_mut(h) {
            m.extension.atmos = params;
            sent = true;
        }
    }
    if let Some(h) = lod.material()
        && let Some(mut m) = lod_materials.get_mut(h)
    {
        m.extension.atmos = params;
    }
    if sent {
        *last = params;
    }
}

fn set_grading(g: &mut ColorGrading, grade: Grade) {
    if g.global.exposure != grade.exposure {
        g.global.exposure = grade.exposure;
    }
    g.global.temperature = grade.warmth;
    // Contrast as a power curve pivoting on mid grey: out = 0.18 (in / 0.18)^k.
    // Bevy's own `contrast` pivots linearly on 0.5 and clips everything dark
    // to black, which wipes out night scenes. In ASC CDL terms (out =
    // (in * gain)^(1 / gamma)) that is gamma = 1 / k, gain = 0.18^(1/k - 1).
    let k = grade.contrast.max(0.1);
    let gain = MID_GREY.powf(1.0 / k - 1.0);
    for s in [&mut g.shadows, &mut g.midtones, &mut g.highlights] {
        s.saturation = grade.saturation;
        s.contrast = 1.0;
        s.gamma = 1.0 / k;
        s.gain = gain;
    }
}

/// The grey that contrast turns around (linear).
const MID_GREY: f32 = 0.18;

/// What [`set_grading`] does to a linear value, for tests.
#[cfg(test)]
fn graded(x: f32, contrast: f32) -> f32 {
    let k = contrast;
    (x * MID_GREY.powf(1.0 / k - 1.0)).powf(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn looks() -> Looks {
        Looks::from_ron(include_str!("../../../assets/data/atmosphere.ron")).unwrap()
    }

    #[test]
    fn data_names_exist() {
        let l = looks();
        let seasons = crate::clock::Seasons::from_ron(include_str!("../../../assets/data/seasons.ron")).unwrap();
        for name in l.seasons.keys() {
            assert!(seasons.list.iter().any(|s| &s.name == name), "atmosphere.ron: no season {name}");
        }
        let kinds = WeatherKinds::from_ron(include_str!("../../../assets/data/weather.ron")).unwrap();
        for name in l.weather.keys() {
            assert!(kinds.find(name).is_some(), "atmosphere.ron: no weather {name}");
        }
        let biomes = include_str!("../../../assets/data/biomes.ron");
        for name in l.biomes.keys() {
            assert!(biomes.contains(&format!("name: \"{name}\"")), "atmosphere.ron: no biome {name}");
        }
    }

    #[test]
    fn night_drains_colour() {
        let l = looks();
        let day = current_grade(&l, Grade::default(), Grade::default(), Grade::default(), 0.0);
        let night = current_grade(&l, Grade::default(), Grade::default(), Grade::default(), 1.0);
        assert!(night.saturation < day.saturation);
        assert!(night.warmth < day.warmth);
    }

    #[test]
    fn contrast_keeps_mid_grey_and_dark_detail() {
        assert!((graded(MID_GREY, 1.2) - MID_GREY).abs() < 1e-5);
        assert!(graded(0.5, 1.2) > 0.5);
        // Dark values get darker but never clip to black.
        let night = graded(0.01, 1.2);
        assert!(night < 0.01 && night > 0.003, "{night}");
    }

    #[test]
    fn grades_combine() {
        let a = Grade { contrast: 1.1, mist: 0.2, ..default() };
        let b = Grade { contrast: 0.5, mist: 0.3, ..default() };
        let c = a.and(b);
        assert!((c.contrast - 0.55).abs() < 1e-6 && (c.mist - 0.5).abs() < 1e-6);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
    }
}
