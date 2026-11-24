//! Snow that piles up while it snows and puddles that fill low spots in the rain.
//!
//! Every couple of seconds a batch of random columns near you is looked at
//! (`assets/data/cover.ron`): where the top of a column is open to the sky,
//! snowfall adds a `snow_layer_<n>` (1..8 eighths, drifting into the lowest
//! spot next to it first) and warm weather melts it away a layer at a time.
//! Rain adds `puddle_<n>` water levels (1..8) in low spots (a cell with walls
//! on at least two sides), water runs to lower neighbours, off edges and into
//! real water, and puddles dry up once the rain stops. The rules are in
//! [`rules`]; this only picks the columns and writes the changes, so cover
//! only builds where chunks are loaded. Changed chunks are saved like any edit.

pub mod rules;

use bevy::prelude::*;
use serde::Deserialize;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::weather::{Conditions, LocalClimate, WeatherSet, SNOW_STOPS_ABOVE};
use crate::world::World;
use rules::{Kinds, Sky};

/// How far above and below you a column is searched for its top.
const SEARCH: i32 = 48;

pub struct CoverPlugin;

impl Plugin for CoverPlugin {
    fn build(&self, app: &mut App) {
        let cfg: CoverConfig = ron::from_str(include_str!("../../../../assets/data/cover.ron")).unwrap_or_else(|e| panic!("cover.ron: {e}"));
        app.insert_resource(cfg).add_systems(crate::menu::GameUpdate, settle.after(WeatherSet));
    }
}

/// `assets/data/cover.ron`.
#[derive(Resource, Debug, Clone, Deserialize)]
pub struct CoverConfig {
    pub every: f32,
    pub columns: u32,
    pub radius: i32,
    pub snow: f32,
    pub melt: f32,
    pub rain: f32,
    pub dry: f32,
    pub buries: Vec<String>,
}

/// This moment's chances per look, from the weather.
pub fn sky_now(cfg: &CoverConfig, c: &Conditions, temperature: f32) -> Sky {
    let falling = c.precipitation > 0.05;
    let warm = !c.snowing && temperature > SNOW_STOPS_ABOVE;
    Sky {
        snow: if falling && c.snowing { c.precipitation * cfg.snow } else { 0.0 },
        rain: if falling && !c.snowing { c.precipitation * cfg.rain } else { 0.0 },
        melt: if warm { cfg.melt * if falling { 2.0 } else { 1.0 } } else { 0.0 },
        dry: if falling { 0.0 } else { cfg.dry },
    }
}

#[allow(clippy::too_many_arguments)]
fn settle(
    time: Res<Time>,
    mut wait: Local<f32>,
    mut seed: Local<u64>,
    mut kinds: Local<Option<Kinds>>,
    cfg: Res<CoverConfig>,
    blocks: Res<Blocks>,
    conditions: Res<Conditions>,
    climate: Res<LocalClimate>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut world: ResMut<World>,
) {
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    *wait = cfg.every;
    let sky = sky_now(&cfg, &conditions, climate.temperature);
    if sky.is_still() {
        return;
    }
    let here = BlockPos::from_world(cam.translation.to_array());
    if worldgen::in_pocket(here.x, here.z) {
        return;
    }
    let kinds = kinds.get_or_insert_with(|| Kinds::new(&blocks.registry, &cfg.buries));
    if *seed == 0 {
        *seed = 0x9E37_79B9_7F4A_7C15 ^ time.elapsed().as_nanos() as u64 | 1;
    }
    let reg = &blocks.registry;
    let get = |p: BlockPos| world.block(p);
    let mut changes = Vec::new();
    for _ in 0..cfg.columns {
        let dx = ((rules::rand(&mut *seed) * 2.0 - 1.0) * cfg.radius as f32) as i32;
        let dz = ((rules::rand(&mut *seed) * 2.0 - 1.0) * cfg.radius as f32) as i32;
        if dx * dx + dz * dz > cfg.radius * cfg.radius {
            continue;
        }
        let (x, z) = (here.x + dx, here.z + dz);
        let Some(at) = rules::surface(&get, reg, kinds, x, z, here.y + SEARCH, here.y - SEARCH) else { continue };
        // Only where the sky reaches (not under roofs or overhangs).
        if world.light_at(at).is_none_or(|(s, _)| s < voxel::light::MAX) {
            continue;
        }
        changes.extend(rules::step(&get, reg, kinds, at, &sky, &mut *seed));
    }
    for (p, b) in changes {
        world.set_block(p, b);
    }
}
