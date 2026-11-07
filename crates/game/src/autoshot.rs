//! Automated screenshot for testing without a person at the keyboard.
//!
//! `BLOCKGAME_SCREENSHOT=out.png cargo run` waits until chunk loading has
//! settled (or 60 seconds), saves a screenshot, then quits.
//! `BLOCKGAME_CAMERA="x,y,z,yaw,pitch"` (degrees) places the camera first,
//! relative to the spawn point if prefixed with `~` (e.g. `~0,~80,~0,45,-30`).
//! `x,y,z,@tx,ty,tz` points the camera at a target instead. A placed camera
//! flies; without one the shot is from the player standing at spawn.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::world::WorldStats;

pub struct AutoShotPlugin;

impl Plugin for AutoShotPlugin {
    fn build(&self, app: &mut App) {
        if let Ok(path) = std::env::var("BLOCKGAME_SCREENSHOT") {
            app.insert_resource(ShotPath(path)).add_systems(Update, take_shot);
        }
        if let Ok(spec) = std::env::var("BLOCKGAME_CAMERA") {
            app.insert_resource(CameraSpec(spec))
                .add_systems(PostStartup, place_camera.after(crate::world::setup_generator));
        }
    }
}

#[derive(Resource)]
struct ShotPath(String);

#[derive(Resource)]
struct CameraSpec(String);

fn place_camera(
    spec: Res<CameraSpec>,
    mut mode: ResMut<crate::player::MoveMode>,
    mut cam: Query<(&mut Transform, &mut crate::camera::FlyCamera)>,
) {
    // A placed camera is a flying one.
    *mode = crate::player::MoveMode::Fly;
    let parts: Vec<&str> = spec.0.split(',').map(str::trim).collect();
    for (mut tf, mut fc) in &mut cam {
        let spawn = tf.translation - Vec3::Y * crate::player::EYE;
        let coord = |i: usize, base: f32| -> f32 {
            let p = parts.get(i).copied().unwrap_or("~0");
            match p.strip_prefix('~') {
                Some(rel) => base + rel.parse::<f32>().unwrap_or(0.0),
                None => p.parse().unwrap_or(base),
            }
        };
        tf.translation = Vec3::new(coord(0, spawn.x), coord(1, spawn.y), coord(2, spawn.z));
        if let Some(tx) = parts.get(3).and_then(|p| p.strip_prefix('@')) {
            // "@x,y,z": look at a point.
            let target = Vec3::new(tx.parse().unwrap_or(0.0), coord(4, 0.0), coord(5, 0.0));
            let d = (target - tf.translation).normalize_or(Vec3::NEG_Z);
            fc.yaw = (-d.x).atan2(-d.z);
            fc.pitch = d.y.asin();
        } else {
            fc.yaw = parts.get(3).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0).to_radians();
            fc.pitch = parts.get(4).and_then(|v| v.parse::<f32>().ok()).unwrap_or(-20.0).to_radians();
        }
        tf.rotation = Quat::from_euler(EulerRot::YXZ, fc.yaw, fc.pitch, 0.0);
    }
}

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
