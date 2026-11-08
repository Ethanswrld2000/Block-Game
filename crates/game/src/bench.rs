//! A repeatable performance run: `BLOCKGAME_BENCH=60 cargo run --release`
//! sets the view distance to 16 chunks, waits for loading to settle, then
//! flies a fixed route (out over the swamp, up through the clouds to the sky
//! islands, back down) for that many seconds, turning as it goes. It logs
//! frame-time statistics every 10 seconds and at the end, then quits.
//! Settings and saves are left alone.

use bevy::prelude::*;

use crate::camera::FlyCamera;
use crate::world::{ViewDistance, WorldStats};

pub struct BenchPlugin;

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        if let Some(secs) = std::env::var("BLOCKGAME_BENCH").ok().and_then(|v| v.parse::<f32>().ok()) {
            app.insert_resource(Bench { secs, state: State::Loading, frames: Vec::new(), start: Vec3::ZERO, t: 0.0, idle: 0, report_at: 10.0 })
                .add_systems(Update, run.after(crate::settings::SettingsPlugin::APPLY));
        }
    }
}

#[derive(PartialEq)]
enum State {
    Loading,
    Flying,
}

#[derive(Resource)]
struct Bench {
    secs: f32,
    state: State,
    /// Frame times in ms since the last report.
    frames: Vec<f32>,
    start: Vec3,
    t: f32,
    idle: u32,
    report_at: f32,
}

fn stats(frames: &mut [f32]) -> String {
    if frames.is_empty() {
        return "no frames".into();
    }
    frames.sort_by(f32::total_cmp);
    let n = frames.len();
    let mean = frames.iter().sum::<f32>() / n as f32;
    let p99 = frames[((n as f32 * 0.99) as usize).min(n - 1)];
    let worst = frames[n - 1];
    let slow = frames.iter().filter(|f| **f > 1000.0 / 58.0).count();
    format!(
        "{n} frames, avg {:.1} fps ({mean:.2} ms), 1% low {:.1} fps ({p99:.2} ms), worst {worst:.1} ms, {:.1}% of frames under 58 fps",
        1000.0 / mean,
        1000.0 / p99,
        slow as f32 * 100.0 / n as f32
    )
}

#[allow(clippy::too_many_arguments)]
fn run(
    time: Res<Time>,
    stats_res: Res<WorldStats>,
    mut bench: ResMut<Bench>,
    mut view: ResMut<ViewDistance>,
    mut mode: ResMut<crate::player::MoveMode>,
    mut cam: Single<(&mut Transform, &mut FlyCamera)>,
    mut exit: MessageWriter<AppExit>,
    mut all: Local<Vec<f32>>,
) {
    let b = &mut *bench;
    if view.chunks != 16 {
        view.chunks = 16;
    }
    *mode = crate::player::MoveMode::Fly;
    match b.state {
        State::Loading => {
            let idle = stats_res.loaded > 0 && stats_res.queued == 0 && stats_res.gen_in_flight == 0 && stats_res.mesh_in_flight == 0;
            b.idle = if idle { b.idle + 1 } else { 0 };
            if b.idle > 60 || time.elapsed_secs() > 120.0 {
                b.state = State::Flying;
                b.start = cam.0.translation;
                info!("bench: loaded {} chunks, flying for {} s", stats_res.loaded, b.secs);
            }
        }
        State::Flying => {
            let dt = time.delta_secs();
            b.t += dt;
            b.frames.push(dt * 1000.0);
            all.push(dt * 1000.0);
            // The route: a third along the ground, a third climbing to the
            // islands, a third coming back down while turning.
            let f = (b.t / b.secs).min(1.0);
            let out = Vec3::new(1.0, 0.0, 0.35).normalize() * 22.0 * b.t;
            let climb = if f < 1.0 / 3.0 {
                30.0
            } else if f < 2.0 / 3.0 {
                30.0 + (f - 1.0 / 3.0) * 3.0 * 420.0
            } else {
                450.0 - (f - 2.0 / 3.0) * 3.0 * 380.0
            };
            let (tf, fc) = &mut *cam;
            tf.translation = Vec3::new(b.start.x + out.x, b.start.y + climb, b.start.z + out.z);
            fc.yaw = -1.2 + b.t * 0.25;
            fc.pitch = -0.25 + (b.t * 0.3).sin() * 0.2;
            tf.rotation = Quat::from_euler(EulerRot::YXZ, fc.yaw, fc.pitch, 0.0);
            if b.t >= b.report_at {
                b.report_at += 10.0;
                let mut frames = std::mem::take(&mut b.frames);
                info!("bench at {:.0} s (y {:.0}, {} chunks drawn): {}", b.t, tf.translation.y, stats_res.meshed, stats(&mut frames));
            }
            if b.t >= b.secs {
                info!("bench done: {}", stats(&mut all));
                exit.write(AppExit::Success);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stats_read_right() {
        let mut f = vec![16.0; 99];
        f.push(40.0);
        let s = super::stats(&mut f);
        assert!(s.contains("100 frames") && s.contains("worst 40.0"), "{s}");
    }
}
