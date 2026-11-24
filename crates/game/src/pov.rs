//! First-person feel: the field of view widens while you sprint, the hands
//! sink while you fall and are knocked down when you land (then spring back),
//! hop on a jump, trail behind the mouse when you look around, and drop into a
//! low, tilted carry while sprinting. `hands::pose_hands` reads `Pov`; the
//! camera's own bob, sway and landing dip are in `player::ride_camera`.
//! The springs and the sprint ease are pure (`Spring`, `ease_sprint`) and tested.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::camera::{FlyCamera, Menus};
use crate::player::{MoveMode, Player};

/// Degrees added to the field of view at full sprint. Destiny widens it when
/// you sprint; make this negative to narrow it instead.
pub const SPRINT_FOV: f32 = 8.0;

pub struct PovPlugin;

impl Plugin for PovPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pov>().add_systems(
            crate::menu::GameUpdate,
            (track, sprint_fov).chain().after(crate::player::PlayerSet).before(crate::hands::pose_hands),
        );
    }
}

/// A critically-ish damped spring towards a target (a little overshoot, so a
/// knocked hand bounces once and settles).
#[derive(Debug, Default, Clone, Copy)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
}

impl Spring {
    pub fn step(&mut self, target: f32, stiffness: f32, damping: f32, dt: f32) {
        // Small sub-steps keep it steady on a slow frame.
        let n = (dt / 0.008).ceil().max(1.0) as usize;
        let h = dt / n as f32;
        for _ in 0..n {
            self.v += (-(self.x - target) * stiffness - self.v * damping) * h;
            self.x += self.v * h;
        }
    }
}

/// Eases the sprint amount (0..1) towards on or off, about a quarter second.
pub fn ease_sprint(current: f32, sprinting: bool, dt: f32) -> f32 {
    let target = if sprinting { 1.0 } else { 0.0 };
    current + (target - current) * (1.0 - (-dt * 9.0).exp())
}

#[derive(Resource, Default, Debug)]
pub struct Pov {
    /// 0..1, eased: how far into the sprint carry and wide view.
    pub sprint: f32,
    /// Hands' height offset (metres, + up) from jumps, falls and landings.
    pub drop: Spring,
    /// Hands trailing the look (radians: yaw, pitch).
    pub lag_yaw: Spring,
    pub lag_pitch: Spring,
    was_on_ground: bool,
    last_vy: f32,
}

#[allow(clippy::too_many_arguments)]
fn track(
    time: Res<Time>,
    player: Res<Player>,
    mode: Res<MoveMode>,
    menus: Res<Menus>,
    ships: Option<Res<crate::airship::Airships>>,
    motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    cam: Query<&FlyCamera>,
    mut pov: ResMut<Pov>,
) {
    let dt = time.delta_secs().min(0.1);
    if dt <= 0.0 {
        return;
    }
    let p = &*player;
    let on_foot = *mode == MoveMode::Walk && crate::airship::not_aboard(ships) && !menus.still;

    // Sprint: only while actually running along the ground.
    let ground_speed = Vec2::new(p.vel.x, p.vel.z).length();
    let sprinting = on_foot && p.sprinting && p.on_ground && !p.in_water && ground_speed > 5.0;
    pov.sprint = ease_sprint(pov.sprint, sprinting, dt);

    // Falls: the hands sink the faster you drop, a jump lifts them a moment,
    // and landing knocks them down by how hard you hit.
    let mut target = 0.0;
    if on_foot {
        if !p.on_ground && !p.in_water && p.vel.y < 0.0 {
            target = -0.07 * (-p.vel.y / 20.0).min(1.0);
        }
        if p.on_ground && !pov.was_on_ground {
            let hit = (-pov.last_vy).max(0.0);
            if hit > 3.0 {
                pov.drop.v -= (hit * 0.045).min(1.4);
            }
        } else if !p.on_ground && pov.was_on_ground && p.vel.y > 1.0 {
            pov.drop.v -= 0.35;
        }
    }
    pov.drop.step(target, 160.0, 15.0, dt);
    pov.drop.x = pov.drop.x.clamp(-0.16, 0.06);
    pov.was_on_ground = p.on_ground;
    pov.last_vy = p.vel.y;

    // Look sway: the hands lag behind how fast you turn, then catch up.
    let (mut yaw_to, mut pitch_to) = (0.0, 0.0);
    if on_foot && !menus.open && cursor.grab_mode != CursorGrabMode::None {
        let sens = cam.iter().next().map_or(0.002, |c| c.sensitivity);
        let turn = motion.delta * sens / dt;
        yaw_to = (turn.x * 0.012).clamp(-0.09, 0.09);
        pitch_to = (turn.y * 0.012).clamp(-0.07, 0.07);
    }
    pov.lag_yaw.step(yaw_to, 120.0, 17.0, dt);
    pov.lag_pitch.step(pitch_to, 120.0, 17.0, dt);
}

/// Widen (or narrow) the view with the sprint, on top of the FOV setting.
/// Left alone while a bench, talk, cutscene or telescope has the camera.
fn sprint_fov(pov: Res<Pov>, menus: Res<Menus>, settings: Option<Res<crate::settings::Settings>>, mut proj: Query<&mut Projection, With<FlyCamera>>) {
    if menus.still {
        return;
    }
    let base = settings.map_or(70.0, |s| s.fov);
    let k = pov.sprint * pov.sprint * (3.0 - 2.0 * pov.sprint);
    let want = (base + SPRINT_FOV * k).to_radians();
    for mut proj in &mut proj {
        if let Projection::Perspective(p) = &mut *proj
            && (p.fov - want).abs() > 1e-5
        {
            p.fov = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knocked_spring_bounces_back_to_rest() {
        let mut s = Spring::default();
        s.v = -1.0;
        let mut lowest: f32 = 0.0;
        for _ in 0..30 {
            s.step(0.0, 160.0, 15.0, 1.0 / 60.0);
            lowest = lowest.min(s.x);
        }
        assert!(lowest < -0.03, "the hands should visibly drop");
        for _ in 0..90 {
            s.step(0.0, 160.0, 15.0, 1.0 / 60.0);
        }
        assert!(s.x.abs() < 0.002 && s.v.abs() < 0.02, "and settle within two seconds");
    }

    #[test]
    fn spring_is_steady_on_slow_frames() {
        let mut s = Spring { x: -0.1, v: 0.0 };
        s.step(0.0, 160.0, 15.0, 0.1);
        assert!(s.x.abs() <= 0.1);
    }

    #[test]
    fn sprint_eases_in_and_out() {
        let mut k = 0.0;
        for _ in 0..30 {
            k = ease_sprint(k, true, 1.0 / 60.0);
        }
        assert!(k > 0.95);
        for _ in 0..30 {
            k = ease_sprint(k, false, 1.0 / 60.0);
        }
        assert!(k < 0.05);
    }
}
