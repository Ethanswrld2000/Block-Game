//! Wind streaks: thin white lines that blow past around you so you can see
//! which way the wind goes. Each one is a short trail drawn with gizmos: its
//! head rides the wind (gusts included) with a gentle curl, and the line fades
//! from nothing at the tail to its head, fading in and out over its life.
//! More of them the harder it blows; none under a roof.

use bevy::prelude::*;
use voxel::BlockPos;

use crate::camera::FlyCamera;
use crate::sky::Sun;
use crate::wind::Wind;
use crate::world::World;

/// Most streaks at once.
const STREAKS: usize = 36;
/// Points kept along each trail, and seconds between them.
const TRAIL_POINTS: usize = 14;
const TRAIL_STEP: f32 = 0.035;
/// Where streaks start, blocks from the camera (sideways) and around eye height.
const NEAR: f32 = 5.0;
const FAR: f32 = 22.0;
/// Wind (blocks per second, gust included) where streaks start to show, and where all of them do.
const SHOW_FROM: f32 = 2.0;
const SHOW_ALL: f32 = 22.0;

pub struct WindStreaksPlugin;

impl Plugin for WindStreaksPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<StreakGizmos>()
            .add_systems(crate::menu::GameStartup, setup)
            .add_systems(
                crate::menu::GameUpdate,
                (blow_streaks, draw_streaks)
                    .chain()
                    .after(crate::weather::WeatherSet),
            );
    }
}

#[derive(Default, Reflect, GizmoConfigGroup)]
struct StreakGizmos;

#[derive(Clone, Default)]
struct Streak {
    alive: bool,
    age: f32,
    life: f32,
    head: Vec3,
    /// Phase and size of its curl.
    phase: f32,
    curl: f32,
    trail: Vec<Vec3>,
    since_point: f32,
}

#[derive(Resource)]
struct Streaks {
    list: Vec<Streak>,
    rng: u64,
}

fn setup(mut commands: Commands, mut config: ResMut<GizmoConfigStore>) {
    let (c, _) = config.config_mut::<StreakGizmos>();
    c.line.width = 1.5;
    commands.insert_resource(Streaks {
        list: vec![Streak::default(); STREAKS],
        rng: 0x9E37_79B9_7F4A_7C15,
    });
}

/// xorshift, for scattering streaks.
fn rand(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 / (1u64 << 24) as f32
}

/// How many streaks show at this wind (blocks per second, gust included).
pub fn streak_count(wind: f32) -> usize {
    let t = ((wind - SHOW_FROM) / (SHOW_ALL - SHOW_FROM)).clamp(0.0, 1.0);
    (t.sqrt() * STREAKS as f32).round() as usize
}

/// 0..1 brightness over a streak's life: fades in, then out.
pub fn life_fade(age: f32, life: f32) -> f32 {
    if life <= 0.0 {
        return 0.0;
    }
    (age / life * std::f32::consts::PI).sin().clamp(0.0, 1.0)
}

fn open_air(world: &World, p: Vec3) -> bool {
    let at = BlockPos::from_world(p.to_array());
    (0..24).all(|dy| {
        world
            .block(BlockPos::new(at.x, at.y + dy, at.z))
            .is_some_and(|b| b.is_air())
    })
}

fn blow_streaks(
    time: Res<Time>,
    wind: Res<Wind>,
    world: Res<World>,
    cam: Query<&Transform, With<FlyCamera>>,
    mut streaks: ResMut<Streaks>,
) {
    let Ok(cam) = cam.single() else { return };
    let dt = time.delta_secs().min(0.25);
    let speed = wind.gusting();
    let dir = wind.dir();
    let flow = Vec3::new(dir.x, 0.0, dir.y);
    let side = Vec3::new(-dir.y, 0.0, dir.x);
    let want = streak_count(speed);
    let Streaks { list, rng } = &mut *streaks;
    let alive = list.iter().filter(|s| s.alive).count();
    let mut to_start = want.saturating_sub(alive);
    for s in list.iter_mut() {
        if !s.alive {
            // Start a few each frame, not all at once.
            if to_start == 0 || rand(rng) > 0.15 {
                continue;
            }
            let a = rand(rng) * std::f32::consts::TAU;
            let r = NEAR + rand(rng) * (FAR - NEAR);
            let p = cam.translation + Vec3::new(a.cos() * r, -2.0 + rand(rng) * 8.0, a.sin() * r);
            if !open_air(&world, p) {
                continue;
            }
            to_start -= 1;
            *s = Streak {
                alive: true,
                age: 0.0,
                life: 1.2 + rand(rng) * 1.4,
                head: p,
                phase: rand(rng) * std::f32::consts::TAU,
                curl: 0.4 + rand(rng) * 0.8,
                trail: vec![p],
                since_point: 0.0,
            };
            continue;
        }
        s.age += dt;
        if s.age >= s.life || (s.head - cam.translation).length() > FAR * 1.6 {
            s.alive = false;
            s.trail.clear();
            continue;
        }
        // Ride the wind, curling a little sideways and up and down.
        let t = s.age * 3.0 + s.phase;
        let vel = flow * speed
            + (side * t.cos() * 1.2 + Vec3::Y * (t * 0.7).sin() * 0.8)
                * s.curl
                * speed.sqrt()
                * 0.5;
        s.head += vel * dt;
        s.since_point += dt;
        if s.since_point >= TRAIL_STEP {
            s.since_point = 0.0;
            s.trail.push(s.head);
            if s.trail.len() > TRAIL_POINTS {
                s.trail.remove(0);
            }
        }
    }
}

fn draw_streaks(
    streaks: Res<Streaks>,
    sun: Query<&Transform, With<Sun>>,
    mut gizmos: Gizmos<StreakGizmos>,
) {
    // Dimmer at night: the lines aren't lit, so take the sun's height.
    let up = sun
        .single()
        .map(|t| (-t.forward().y).clamp(0.0, 1.0))
        .unwrap_or(1.0);
    let bright = 0.25 + 0.75 * (up * 4.0).min(1.0);
    for s in streaks
        .list
        .iter()
        .filter(|s| s.alive && !s.trail.is_empty())
    {
        let fade = life_fade(s.age, s.life) * 0.55;
        let n = s.trail.len() + 1;
        let points = s
            .trail
            .iter()
            .copied()
            .chain(std::iter::once(s.head))
            .enumerate()
            .map(|(i, p)| {
                let a = fade * i as f32 / (n - 1).max(1) as f32;
                (p, Color::srgba(bright, bright, bright * 1.02, a))
            });
        gizmos.linestrip_gradient(points);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_streaks_in_stronger_wind() {
        assert_eq!(streak_count(0.5), 0, "still air shows none");
        assert!(streak_count(3.0) > 0, "a light breeze shows a few");
        assert!(streak_count(3.0) < streak_count(12.0) && streak_count(12.0) < streak_count(24.0));
        assert_eq!(streak_count(60.0), STREAKS);
    }

    #[test]
    fn streaks_fade_in_and_out() {
        assert_eq!(life_fade(0.0, 2.0), 0.0);
        assert!((life_fade(1.0, 2.0) - 1.0).abs() < 1e-5);
        assert!(life_fade(2.0, 2.0) < 1e-5);
    }
}
