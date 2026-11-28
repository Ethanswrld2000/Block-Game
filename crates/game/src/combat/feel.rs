//! How hits feel: a brief hit stop (the game slows for a few hundredths of a
//! second when your blow lands, longer for a guard break or a kill) and a camera
//! kick (a small nod when you hit, a sharper jolt when you're hurt). Pure parts
//! (`stop_for`, `Kick::step`) are tested.

use bevy::prelude::*;

use crate::player::Damage;

/// Your blow landed. `strength` 0..1: a blocked hit is light, a kill is full.
#[derive(Message, Debug, Clone, Copy)]
pub struct Impact {
    pub strength: f32,
}

/// A perfect parry: the world slows for a moment so you can punish.
#[derive(Message, Debug, Clone, Copy)]
pub struct SlowMo;

/// Game speed and real seconds of a perfect parry's slow-down.
pub const SLOW_SPEED: f32 = 0.3;
pub const SLOW_FOR: f32 = 0.6;

/// Game speed during a hit stop.
const STOP_SPEED: f32 = 0.04;

/// Real seconds a hit of this strength stops the game for.
pub fn stop_for(strength: f32) -> f32 {
    0.03 + 0.07 * strength.clamp(0.0, 1.0)
}

#[derive(Resource, Default)]
pub struct HitStop {
    left: f32,
    /// Real seconds of slow-down left.
    slow: f32,
    /// We slowed the game (so we're the one to put it back).
    slowed: bool,
}

/// Camera kick: pitch and roll offsets (radians) on a damped spring, added by `player::ride_camera`.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Kick {
    pub pitch: f32,
    pub roll: f32,
    pitch_vel: f32,
    roll_vel: f32,
}

impl Kick {
    pub fn push(&mut self, pitch: f32, roll: f32) {
        self.pitch_vel += pitch;
        self.roll_vel += roll;
    }

    /// Springs back to rest.
    pub fn step(&mut self, dt: f32) {
        let dt = dt.min(0.05);
        for (x, v) in [
            (&mut self.pitch, &mut self.pitch_vel),
            (&mut self.roll, &mut self.roll_vel),
        ] {
            *v += (-260.0 * *x - 24.0 * *v) * dt;
            *x = (*x + *v * dt).clamp(-0.08, 0.08);
        }
    }
}

pub struct FeelPlugin;

impl Plugin for FeelPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Impact>()
            .add_message::<SlowMo>()
            .init_resource::<HitStop>()
            .init_resource::<Kick>()
            .add_systems(crate::menu::GameUpdate, feel.after(super::fall_set()));
    }
}

#[allow(clippy::too_many_arguments)]
fn feel(
    real: Res<Time<Real>>,
    mut virt: ResMut<Time<Virtual>>,
    mut stop: ResMut<HitStop>,
    mut kick: ResMut<Kick>,
    mut impacts: MessageReader<Impact>,
    mut hurts: MessageReader<Damage>,
    mut slows: MessageReader<SlowMo>,
    mut flip: Local<bool>,
) {
    let dt = real.delta_secs();
    if slows.read().count() > 0 {
        stop.slow = SLOW_FOR;
    }
    for i in impacts.read() {
        stop.left = stop.left.max(stop_for(i.strength));
        *flip = !*flip;
        kick.push(
            -0.35 * i.strength,
            if *flip { 0.15 } else { -0.15 } * i.strength,
        );
    }
    for d in hurts.read() {
        let s = (d.amount / 10.0).clamp(0.2, 1.0);
        *flip = !*flip;
        kick.push(1.1 * s, if *flip { 0.9 } else { -0.9 } * s);
    }
    kick.step(dt);
    stop.left -= dt;
    stop.slow -= dt;
    let speed = game_speed(stop.left, stop.slow);
    if speed < 1.0 {
        if !stop.slowed || virt.relative_speed() != speed {
            virt.set_relative_speed(speed);
            stop.slowed = true;
        }
        return;
    }
    if stop.slowed {
        virt.set_relative_speed(1.0);
        stop.slowed = false;
    }
}

/// Game speed with `stop` seconds of hit stop and `slow` of slow-down left:
/// the slow-down eases back to full speed over its last third.
pub fn game_speed(stop: f32, slow: f32) -> f32 {
    if stop > 0.0 {
        return STOP_SPEED;
    }
    if slow <= 0.0 {
        return 1.0;
    }
    let ease = (slow / (SLOW_FOR / 3.0)).min(1.0);
    1.0 - (1.0 - SLOW_SPEED) * ease
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parry_slows_the_world_then_eases_back() {
        assert_eq!(game_speed(0.0, 0.0), 1.0);
        assert_eq!(game_speed(0.0, SLOW_FOR), SLOW_SPEED);
        assert!(
            game_speed(0.0, SLOW_FOR * 0.1) > SLOW_SPEED && game_speed(0.0, SLOW_FOR * 0.1) < 1.0
        );
        // A hit stop inside it still stops.
        assert_eq!(game_speed(0.05, SLOW_FOR), STOP_SPEED);
    }

    #[test]
    fn harder_hits_stop_longer_but_briefly() {
        assert!(stop_for(1.0) > stop_for(0.3));
        assert!(stop_for(5.0) <= 0.1 + 1e-6);
        assert!(stop_for(0.0) > 0.0);
    }

    #[test]
    fn kick_springs_back_to_rest() {
        let mut k = Kick::default();
        k.push(1.0, -0.5);
        let mut peak: f32 = 0.0;
        for _ in 0..120 {
            k.step(1.0 / 60.0);
            peak = peak.max(k.pitch.abs());
        }
        assert!(peak > 0.01 && peak <= 0.08);
        assert!(k.pitch.abs() < 1e-3 && k.roll.abs() < 1e-3);
    }
}
