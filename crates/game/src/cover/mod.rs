//! Snow that piles up while it snows, puddles that fill low spots in the rain,
//! and water that flows.
//!
//! Every couple of seconds a batch of random columns near you is looked at
//! (`assets/data/cover.ron`): where the top of a column is open to the sky,
//! snowfall adds a `snow_layer_<n>` (1..8 eighths, drifting into the lowest
//! spot next to it first; plants and pebbles stay in it, as
//! `snow_layer_<n>+<detail>` blocks) and above freezing it melts slowly, a
//! layer at a time (faster the warmer it is). Rain adds a level of water in
//! low spots (a cell with walls on at least two sides). Puddles stay while it
//! rains; once it stops, water short of a full block dries a level at a time
//! (`Flowing::shallow`, and the columns looked at).
//!
//! Water flows ([`flow`]): `puddle_<n>` is n eighths, `water` is a full eight.
//! Every [`FLOW_EVERY`] seconds the cells near a change move their water
//! (down first, then an eighth at a time sideways), so puddles run downhill.
//! Bodies of more than 10 source blocks never run low, so a pit dug next to a
//! river or the sea fills right up.
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
/// Seconds between drying looks at known puddles (`Flowing::shallow`).
const DRY_EVERY: f32 = 1.0;

pub struct CoverPlugin;

impl Plugin for CoverPlugin {
    fn build(&self, app: &mut App) {
        let cfg: CoverConfig = ron::from_str(include_str!("../../../../assets/data/cover.ron"))
            .unwrap_or_else(|e| panic!("cover.ron: {e}"));
        app.insert_resource(cfg)
            .init_resource::<Flowing>()
            .add_systems(
                crate::menu::GameUpdate,
                (settle.after(WeatherSet), flow_water),
            );
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
    /// Cells known to hold 1..7 eighths of water, which dry after the rain; not saved
    /// (puddles from before a load dry through the columns looked at).
    pub shallow: HashSet<BlockPos>,
}

/// Precipitation strong enough to count.
fn falling(c: &Conditions) -> bool {
    c.precipitation > 0.05
}

/// This moment's chances per look, from the weather. `temperature` is the
/// biome scale (0 = freezing).
pub fn sky_now(cfg: &CoverConfig, c: &Conditions, temperature: f32) -> Sky {
    let falling = falling(c);
    // Snow melts slowly above freezing, faster the warmer it is and in rain.
    let warmth = if c.snowing {
        0.0
    } else {
        (temperature / MELT_FULL).clamp(0.0, 1.0)
    };
    Sky {
        snow: if falling && c.snowing {
            c.precipitation * cfg.snow
        } else {
            0.0
        },
        rain: if falling && !c.snowing {
            c.precipitation * cfg.rain
        } else {
            0.0
        },
        melt: cfg.melt * warmth * if falling { 2.0 } else { 1.0 },
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
        let Some(at) = rules::surface(&get, reg, kinds, x, z, here.y + SEARCH, here.y - SEARCH)
        else {
            continue;
        };
        // Only where the sky reaches (not under roofs or overhangs).
        if world
            .light_at(at)
            .is_none_or(|(s, _)| s < voxel::light::MAX)
        {
            continue;
        }
        changes.extend(rules::step(&get, reg, kinds, at, &sky, &mut seed));
    }
    for (p, b) in changes {
        world.set_block(p, b);
    }
}

/// Wakes the water next to every changed block, dries puddles after the rain,
/// then moves the water that's awake one pass and writes the cells whose level changed.
#[allow(clippy::too_many_arguments)]
fn flow_water(
    time: Res<Time>,
    mut wait: Local<f32>,
    mut dry_wait: Local<f32>,
    mut seed: Local<u64>,
    mut waters: Local<Option<flow::Waters>>,
    blocks: Res<Blocks>,
    cfg: Res<CoverConfig>,
    conditions: Res<Conditions>,
    mut flowing: ResMut<Flowing>,
    mut world: ResMut<World>,
) {
    let waters = waters.get_or_insert_with(|| flow::Waters::new(&blocks.registry));
    if *seed == 0 {
        *seed = 0x2545_F491_4F6C_DD1D ^ time.elapsed().as_nanos() as u64 | 1;
    }
    let changed = std::mem::take(&mut world.changed);
    for p in changed {
        for q in flow::around(p) {
            let level = world.block(q).map_or(0, |b| waters.level(b));
            if level > 0 {
                flowing.active.insert(q);
            }
            if (1..8).contains(&level) {
                flowing.shallow.insert(q);
            }
        }
    }
    // Rain over: puddles dry a level now and then.
    *dry_wait -= time.delta_secs();
    if *dry_wait <= 0.0 {
        *dry_wait = DRY_EVERY;
        if !falling(&conditions) {
            let mut dried = Vec::new();
            flowing.shallow.retain(|p| {
                let level = world.block(*p).map_or(0, |b| waters.level(b));
                if !(1..8).contains(&level) {
                    return false;
                }
                if rules::rand(&mut seed) < cfg.dry {
                    dried.push((*p, waters.block(level - 1)));
                }
                true
            });
            for (p, b) in dried {
                world.set_block(p, b);
                for q in flow::around(p) {
                    if world.block(q).is_some_and(|b| waters.level(b) > 0) {
                        flowing.active.insert(q);
                    }
                }
            }
        }
    }
    *wait -= time.delta_secs();
    if *wait > 0.0 || flowing.active.is_empty() {
        return;
    }
    *wait = FLOW_EVERY;
    let get = |p: BlockPos| waters.cell(world.block(p));
    let moved = flow::pass(
        &get,
        &flowing.active,
        FLOW_BUDGET,
        worldgen::WORLD_FLOOR,
        &mut seed,
    );
    flowing.active = moved.next;
    for (p, level) in moved.levels {
        world.set_block(p, waters.block(level));
        if (1..8).contains(&level) {
            flowing.shallow.insert(p);
        } else {
            flowing.shallow.remove(&p);
        }
    }
    // The pass's own writes are already followed up in `next`.
    world.changed.clear();
}
