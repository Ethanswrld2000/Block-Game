//! Twigs grow back: a twig snapped off a trunk returns after `REGROW_DAYS`
//! game days, if its cell is still empty and the trunk it hung on is still
//! there. Snapped twigs are remembered in `Twigs`, saved to `twigs.ron`.

use std::collections::HashMap;

use bevy::prelude::*;
use voxel::{BlockId, BlockPos};

use crate::blocks::Blocks;
use crate::clock::WorldClock;
use crate::interact::Mined;
use crate::save::{read_ron, screenshot_run, write_ron, SaveDir};
use crate::world::World;

const FILE: &str = "twigs.ron";
/// Game days before a snapped twig grows back.
pub const REGROW_DAYS: f64 = 2.0;

/// Snapped twigs waiting to grow back: where, which block, when snapped (game days).
#[derive(Resource, Default)]
pub struct Twigs(HashMap<BlockPos, (BlockId, f64)>);

/// Whether a twig snapped at `at` is due back at `now`.
pub fn due(at: f64, now: f64) -> bool {
    now - at >= REGROW_DAYS
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Twigs>()
        .add_systems(crate::menu::GamePostStartup, load)
        .add_systems(crate::menu::GameUpdate, (note, regrow, save_now_and_then).chain().after(crate::interact::InteractSet))
        .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
}

fn note(mut mined: MessageReader<Mined>, blocks: Res<Blocks>, clock: Res<WorldClock>, mut twigs: ResMut<Twigs>) {
    for m in mined.read() {
        if blocks.registry.get(m.block).shape == voxel::Shape::Twig {
            twigs.0.insert(m.pos, (m.block, clock.total_days()));
        }
    }
}

/// Every few seconds: loaded twigs that are due come back if their cell is
/// empty and their trunk still stands; ones that can't are forgotten.
fn regrow(time: Res<Time>, mut wait: Local<f32>, blocks: Res<Blocks>, clock: Res<WorldClock>, mut world: ResMut<World>, mut twigs: ResMut<Twigs>) {
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    *wait = 5.0;
    let now = clock.total_days();
    let ready: Vec<(BlockPos, BlockId)> =
        twigs.0.iter().filter(|(p, (_, at))| due(*at, now) && world.is_loaded(p.chunk())).map(|(p, (b, _))| (*p, *b)).collect();
    for (p, b) in ready {
        twigs.0.remove(&p);
        let held = blocks.registry.get(b).supports().iter().any(|o| {
            world.block(BlockPos::new(p.x + o[0], p.y + o[1], p.z + o[2])).is_some_and(|s| blocks.registry.get(s).is_wall())
        });
        if world.block(p) == Some(BlockId::AIR) && held && world.set_block(p, b) {
            crate::interact::remesh_around(&mut world, &blocks, p);
        }
    }
}

type Saved = Vec<([i32; 3], String, f64)>;

fn load(dir: Res<SaveDir>, blocks: Res<Blocks>, mut twigs: ResMut<Twigs>) {
    if screenshot_run() {
        return;
    }
    let list: Saved = read_ron(&dir, FILE).unwrap_or_default();
    for (p, name, at) in list {
        if let Some(b) = blocks.registry.id(&name) {
            twigs.0.insert(BlockPos::new(p[0], p[1], p[2]), (b, at));
        }
    }
}

fn store(dir: &SaveDir, blocks: &Blocks, twigs: &Twigs) {
    let list: Saved = twigs.0.iter().map(|(p, (b, at))| ([p.x, p.y, p.z], blocks.registry.get(*b).name.clone(), *at)).collect();
    write_ron(dir, FILE, &list);
}

fn save_now_and_then(time: Res<Time<Real>>, mut since: Local<f32>, dir: Res<SaveDir>, blocks: Res<Blocks>, twigs: Res<Twigs>) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    store(&dir, &blocks, &twigs);
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, blocks: Res<Blocks>, twigs: Res<Twigs>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &blocks, &twigs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twigs_wait_two_days() {
        assert!(!due(10.0, 11.5));
        assert!(due(10.0, 12.0));
    }
}
