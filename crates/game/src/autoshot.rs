//! Automated screenshot for testing without a person at the keyboard.
//!
//! `BLOCKGAME_SCREENSHOT=out.png cargo run` waits until chunk loading has
//! settled (or 60 seconds), saves a screenshot, then quits.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::world::WorldStats;

pub struct AutoShotPlugin;

impl Plugin for AutoShotPlugin {
    fn build(&self, app: &mut App) {
        if let Ok(path) = std::env::var("BLOCKGAME_SCREENSHOT") {
            app.insert_resource(ShotPath(path)).add_systems(Update, take_shot);
        }
    }
}

#[derive(Resource)]
struct ShotPath(String);

fn take_shot(
    mut commands: Commands,
    time: Res<Time>,
    stats: Res<WorldStats>,
    path: Res<ShotPath>,
    mut state: Local<(u32, Option<f32>)>,
    mut exit: MessageWriter<AppExit>,
) {
    let t = time.elapsed_secs();
    match state.1 {
        None => {
            // "Settled": nothing queued or in flight for 30 frames in a row.
            let idle = stats.loaded > 0 && stats.queued == 0 && stats.gen_in_flight == 0 && stats.mesh_in_flight == 0;
            state.0 = if idle { state.0 + 1 } else { 0 };
            if state.0 > 30 || t > 60.0 {
                info!("autoshot: {} chunks loaded, {} drawn, saving {}", stats.loaded, stats.meshed, path.0);
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.0.clone()));
                state.1 = Some(t);
            }
        }
        Some(shot_at) if t - shot_at > 2.0 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
