//! Snow that piles up while it snows, puddles that fill low spots in the rain,
//! and water that flows.
//!
//! Every couple of seconds a batch of random columns near you is looked at
//! (`assets/data/cover.ron`): where the top of a column is open to the sky,
//! snowfall adds a `snow_layer_<n>` (1..8 eighths, drifting into the lowest
//! spot next to it first; plants and pebbles stay in it, as
//! `snow_layer_<n>+<detail>` blocks) and above freezing it melts slowly, a
//! layer at a time (faster the warmer it is). Rain adds a level of water in
//! low spots (a cell with walls on at least two sides). By day, without rain,
//! a puddle under four eighths that has stopped running dries a level now and then.
//!
//! Water flows ([`flow`]): `puddle_<n>` is n eighths, `water` is a full eight.
//! Every [`FLOW_EVERY`] seconds the cells near a change move their water
//! (down first, then an eighth at a time sideways), so a lake leaks into a
//! hole dug next to it and puddles run downhill, without making or losing water.
//! The rules are in [`rules`] and [`flow`]; this only picks the cells and writes
//! the changes, so cover only builds where chunks are loaded. Changed chunks
//! are saved like any edit.

pub mod flow;
pub mod rules;

use std::collections::HashSet;

use bevy::prelude::*;
use serde::Deserialize;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::clock::WorldClock;
use crate::weather::{Conditions, LocalClimate, WeatherSet};
use crate::world::World;
use rules::{Kinds, Sky};

/// How far above and below you a column is searched for its top.
const SEARCH: i32 = 48;
/// Seconds between water passes.
pub const FLOW_EVERY: f32 = 0.15;
/// Most cells moved per pass (the rest wait for the next).
const FLOW_BUDGET: usize = 4096;
/// Biome temperature (about -1..1, 0 = freezing) at which snow melts at the full `melt` rate (about 10 °C).
const MELT_FULL: f32 = 0.33;
/// Hours of the day puddles dry in.
const DAYTIME: std::ops::Range<f32> = 6.0..18.0;

pub struct CoverPlugin;

impl Plugin for CoverPlugin {
    fn build(&self, app: &mut App) {
        let cfg: CoverConfig = ron::from_str(include_str!("../../../../assets/data/cover.ron")).unwrap_or_else(|e| panic!("cover.ron: {e}"));
        app.insert_resource(cfg).init_resource::<Flowing>().add_systems(crate::menu::GameUpdate, (settle.after(WeatherSet), flow_water));
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
}

/// Water cells still moving (looked at every water pass).
#[derive(Resource, Debug, Default)]
pub struct Flowing {
    pub active: HashSet<BlockPos>,
}

/// This moment's chances per look, from the weather. `temperature` is the
/// biome scale (0 = freezing); `day` = the sun is up.
pub fn sky_now(cfg: &CoverConfig, c: &Conditions, temperature: f32, day: bool) -> Sky {
    let falling = c.precipitation > 0.05;
    // Snow melts slowly above freezing, faster the warmer it is and in rain.
    let warmth = if c.snowing { 0.0 } else { (temperature / MELT_FULL).clamp(0.0, 1.0) };
    Sky {
        snow: if falling && c.snowing { c.precipitation * cfg.snow } else { 0.0 },
        rain: if falling && !c.snowing { c.precipitation * cfg.rain } else { 0.0 },
        melt: cfg.melt * warmth * if falling { 2.0 } else { 1.0 },
        dry: if falling || !day { 0.0 } else { cfg.dry },
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
    clock: Res<WorldClock>,
    flowing: Res<Flowing>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut world: ResMut<World>,
) {
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    *wait = cfg.every;
    let sky = sky_now(&cfg, &conditions, climate.temperature, DAYTIME.contains(&clock.hour()));
    if sky.is_still() {
        return;
    }
    let here = BlockPos::from_world(cam.translation.to_array());
    if worldgen::in_pocket(here.x, here.z) {
        return;
    }
    let kinds = kinds.get_or_insert_with(|| Kinds::new(&blocks.registry));
    if *seed == 0 {
        *seed = 0x9E37_79B9_7F4A_7C15 ^ time.elapsed().as_nanos() as u64 | 1;
    }
    let reg = &blocks.registry;
    let get = |p: BlockPos| world.block(p);
    let mut changes = Vec::new();
    for _ in 0..cfg.columns {
        let dx = ((rules::rand(&mut seed) * 2.0 - 1.0) * cfg.radius as f32) as i32;
        let dz = ((rules::rand(&mut seed) * 2.0 - 1.0) * cfg.radius as f32) as i32;
        if dx * dx + dz * dz > cfg.radius * cfg.radius {
            continue;
        }
        let (x, z) = (here.x + dx, here.z + dz);
        let Some(at) = rules::surface(&get, reg, kinds, x, z, here.y + SEARCH, here.y - SEARCH) else { continue };
        // Only where the sky reaches (not under roofs or overhangs).
        if world.light_at(at).is_none_or(|(s, _)| s < voxel::light::MAX) {
            continue;
        }
        changes.extend(rules::step(&get, reg, kinds, at, &sky, &|p| flowing.active.contains(&p), &mut seed));
    }
    for (p, b) in changes {
        world.set_block(p, b);
    }
}

/// Wakes the water next to every changed block, then moves the water that's
/// awake one pass and writes the cells whose level changed.
fn flow_water(
    time: Res<Time>,
    mut wait: Local<f32>,
    mut seed: Local<u64>,
    mut waters: Local<Option<flow::Waters>>,
    blocks: Res<Blocks>,
    mut flowing: ResMut<Flowing>,
    mut world: ResMut<World>,
) {
    let waters = waters.get_or_insert_with(|| flow::Waters::new(&blocks.registry));
    let changed = std::mem::take(&mut world.changed);
    for p in changed {
        for q in flow::around(p) {
            if world.block(q).is_some_and(|b| waters.level(b) > 0) {
                flowing.active.insert(q);
            }
        }
    }
    *wait -= time.delta_secs();
    if *wait > 0.0 || flowing.active.is_empty() {
        return;
    }
    *wait = FLOW_EVERY;
    if *seed == 0 {
        *seed = 0x2545_F491_4F6C_DD1D ^ time.elapsed().as_nanos() as u64 | 1;
    }
    let get = |p: BlockPos| waters.cell(world.block(p));
    let moved = flow::pass(&get, &flowing.active, FLOW_BUDGET, worldgen::WORLD_FLOOR, &mut seed);
    flowing.active = moved.next;
    for (p, level) in moved.levels {
        world.set_block(p, waters.block(level));
    }
    // The pass's own writes are already followed up in `next`.
    world.changed.clear();
}
