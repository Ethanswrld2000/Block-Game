//! Farming: sow seeds on soil and they grow, stage by stage, into a harvest.
//!
//! Crops are in `assets/data/crops.ron`; each stage is a plant block, and the
//! first stage's item (`barley_seeds`, `tuber_eye`) sows it (`interact` only
//! lets plants go on soil). A sown crop is remembered here (`Fields`, saved to
//! `crops.ron`) with when it was sown and whether water is near (it grows
//! faster); every couple of seconds loaded crops are set to the stage they
//! have reached. Breaking a crop gives what drops.ron lists for its stage.
//! Seeds come from tall grass (barley) and ferns (tuber eyes).
//! Snapped twigs grow back too (`regrow`).

mod regrow;

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use voxel::{BlockId, BlockPos};

use crate::blocks::Blocks;
use crate::clock::WorldClock;
use crate::interact::Placed;
use crate::save::{read_ron, screenshot_run, write_ron, SaveDir};
use crate::world::World;

const FILE: &str = "crops.ron";
/// Water within this many blocks (sideways) speeds a crop up.
const WET_REACH: i32 = 4;

pub struct FarmingPlugin;

impl Plugin for FarmingPlugin {
    fn build(&self, app: &mut App) {
        let crops = load_crops(include_str!("../../../../assets/data/crops.ron")).unwrap_or_else(|e| panic!("crops.ron: {e}"));
        regrow::plugin(app);
        app.insert_resource(CropBook(crops))
            .init_resource::<Fields>()
            .add_systems(crate::menu::GamePostStartup, load)
            .add_systems(crate::menu::GameUpdate, (sow, grow, save_now_and_then).chain().after(crate::interact::InteractSet))
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
    }
}

/// One crop kind from crops.ron.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Crop {
    pub name: String,
    pub stages: Vec<String>,
    pub days: f64,
    #[serde(default = "one")]
    pub wet: f64,
}

fn one() -> f64 {
    1.0
}

pub fn load_crops(text: &str) -> Result<Vec<Crop>, String> {
    let list: Vec<Crop> = ron::from_str(text).map_err(|e| e.to_string())?;
    for c in &list {
        if c.stages.len() < 2 || c.days <= 0.0 {
            return Err(format!("crop `{}` needs two or more stages and days above 0", c.name));
        }
    }
    Ok(list)
}

impl Crop {
    /// The stage reached `age` days after sowing.
    pub fn stage(&self, age: f64, wet: bool) -> usize {
        let speed = if wet { self.wet } else { 1.0 };
        ((age.max(0.0) * speed / self.days).floor() as usize).min(self.stages.len() - 1)
    }
}

#[derive(Resource)]
pub struct CropBook(pub Vec<Crop>);

impl CropBook {
    /// The crop and stage a block is, if it is one.
    fn find(&self, blocks: &Blocks, b: BlockId) -> Option<(usize, usize)> {
        let name = &blocks.registry.get(b).name;
        self.0.iter().enumerate().find_map(|(i, c)| c.stages.iter().position(|s| s == name).map(|st| (i, st)))
    }
}

/// A crop in the ground.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sown {
    pub crop: usize,
    /// `WorldClock::total_days` when sown.
    pub at: f64,
    pub wet: bool,
}

#[derive(Resource, Default)]
pub struct Fields(pub HashMap<BlockPos, Sown>);

/// As saved: position, crop name, when sown, wet.
type SavedCrop = ([i32; 3], String, f64, bool);

fn sow(
    mut placed: MessageReader<Placed>,
    book: Res<CropBook>,
    blocks: Res<Blocks>,
    world: Res<World>,
    clock: Res<WorldClock>,
    mut fields: ResMut<Fields>,
) {
    for p in placed.read() {
        let Some((crop, 0)) = book.find(&blocks, p.block) else { continue };
        let water = blocks.registry.id("water");
        let mut wet = false;
        'look: for dx in -WET_REACH..=WET_REACH {
            for dz in -WET_REACH..=WET_REACH {
                for dy in -2..=0 {
                    if world.block(BlockPos::new(p.pos.x + dx, p.pos.y + dy, p.pos.z + dz)) == water {
                        wet = true;
                        break 'look;
                    }
                }
            }
        }
        fields.0.insert(p.pos, Sown { crop, at: clock.total_days(), wet });
    }
}

/// Every couple of seconds: loaded crops move on to the stage they have reached;
/// crops that are gone (broken, built over) are forgotten.
#[allow(clippy::too_many_arguments)]
fn grow(time: Res<Time>, mut wait: Local<f32>, book: Res<CropBook>, blocks: Res<Blocks>, clock: Res<WorldClock>, mut world: ResMut<World>, mut fields: ResMut<Fields>) {
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    *wait = 2.0;
    let now = clock.total_days();
    let mut gone = Vec::new();
    let mut changes = Vec::new();
    for (pos, s) in &fields.0 {
        if !world.is_loaded(pos.chunk()) {
            continue;
        }
        let Some(crop) = book.0.get(s.crop) else {
            gone.push(*pos);
            continue;
        };
        let block = world.block(*pos);
        // Under snow a crop keeps its place but doesn't grow.
        let snowed = block.and_then(|b| blocks.registry.get(b).inside);
        let here = snowed.or(block).and_then(|b| book.find(&blocks, b));
        match here {
            Some(_) if snowed.is_some() => {}
            Some((c, stage)) if c == s.crop => {
                let want = crop.stage(now - s.at, s.wet);
                if want > stage
                    && let Some(id) = blocks.registry.id(&crop.stages[want])
                {
                    changes.push((*pos, id));
                }
            }
            _ => gone.push(*pos),
        }
    }
    for p in gone {
        fields.0.remove(&p);
    }
    for (p, id) in changes {
        if world.set_block(p, id) {
            crate::interact::remesh_around(&mut world, &blocks, p);
        }
    }
}

fn load(dir: Res<SaveDir>, book: Res<CropBook>, mut fields: ResMut<Fields>) {
    if screenshot_run() {
        return;
    }
    let list: Vec<SavedCrop> = read_ron(&dir, FILE).unwrap_or_default();
    for (p, name, at, wet) in list {
        if let Some(crop) = book.0.iter().position(|c| c.name == name) {
            fields.0.insert(BlockPos::new(p[0], p[1], p[2]), Sown { crop, at, wet });
        }
    }
}

fn store(dir: &SaveDir, book: &CropBook, fields: &Fields) {
    let list: Vec<SavedCrop> =
        fields.0.iter().filter_map(|(p, s)| Some(([p.x, p.y, p.z], book.0.get(s.crop)?.name.clone(), s.at, s.wet))).collect();
    write_ron(dir, FILE, &list);
}

fn save_now_and_then(time: Res<Time<Real>>, mut since: Local<f32>, dir: Res<SaveDir>, book: Res<CropBook>, fields: Res<Fields>) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    store(&dir, &book, &fields);
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, book: Res<CropBook>, fields: Res<Fields>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &book, &fields);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crops() -> Vec<Crop> {
        load_crops(include_str!("../../../../assets/data/crops.ron")).expect("crops.ron")
    }

    #[test]
    fn crops_grow_stage_by_stage_and_faster_by_water() {
        let barley = &crops()[0];
        assert_eq!(barley.stage(0.0, false), 0);
        assert_eq!(barley.stage(barley.days * 1.01, false), 1);
        assert_eq!(barley.stage(barley.days * 50.0, false), barley.stages.len() - 1);
        // Water near: ripe sooner.
        let ripe = barley.days * (barley.stages.len() - 1) as f64;
        assert!(barley.stage(ripe * 0.7, true) > barley.stage(ripe * 0.7, false));
    }

    #[test]
    fn every_crop_stage_is_a_plant_block_and_sown_by_an_item() {
        let blocks = voxel::BlockRegistry::from_ron(include_str!("../../../../assets/data/blocks.ron"), &voxel::TileSheet::from_ron(include_str!("../../../../assets/textures/blocks.ron")).unwrap()).unwrap();
        let items = include_str!("../../../../assets/data/items.ron");
        let drops = include_str!("../../../../assets/data/drops.ron");
        for c in crops() {
            for s in &c.stages {
                let id = blocks.id(s).unwrap_or_else(|| panic!("no block `{s}`"));
                assert_eq!(blocks.get(id).shape, voxel::Shape::Plant, "{s}");
                assert!(drops.contains(&format!("\"{s}\"")), "drops.ron has nothing for {s}");
            }
            assert!(items.contains(&format!("Block(\"{}\")", c.stages[0])), "no item sows {}", c.name);
        }
    }
}
