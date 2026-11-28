//! Shields. A shield in the left hand is raised with right click (whatever
//! the right hand holds, unless it places or eats something): it covers a
//! wider arc than a blade, a blow caught on it costs less Fatigue (`Shield::
//! cost`), it stops bolts, and raised in time it parries like a blade.
//! Left click with it raised bashes: the shield shoves out and the foe in
//! front reels with its guard broken, and one bashed into a wall slams like a
//! kick. Better shields (quality) take less and bash harder.
//!
//! The pose of the bash is pure (`bash_reach`), tested.

use bevy::prelude::*;

use super::{ahead_of, flat, stamina, traits, Foe, FoeBook, Guard};
use crate::camera::{FlyCamera, Menus};
use crate::items::Items;
use crate::player::Player;
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};

/// Seconds a bash takes, and when in it the shield lands (0..1).
pub const BASH_TIME: f32 = 0.38;
pub const CONTACT: f32 = 0.4;
/// Seconds from one bash to the next.
const COOLDOWN: f32 = 0.65;
/// Fatigue a bash costs.
pub const BASH_COST: f32 = 3.0;
/// How far it reaches from your feet, and half its arc (degrees).
const REACH: f32 = 2.4;
const ARC: f32 = 50.0;
/// How hard it sends a foe back (blocks/s).
const SHOVE: f32 = 8.0;

/// How far out the shield is shoved, 0..1, `t` (0..1) through a bash: out
/// fast to the contact, held a moment, drawn back.
pub fn bash_reach(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < CONTACT {
        let x = t / CONTACT;
        x * x
    } else if t < 0.6 {
        1.0
    } else {
        let x = (t - 0.6) / 0.4;
        1.0 - x * x * (3.0 - 2.0 * x)
    }
}

/// Left click behind a raised shield: bash the foe in front.
#[allow(clippy::too_many_arguments)]
pub(super) fn bash(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    menus: Res<Menus>,
    items: Res<Items>,
    book: Res<FoeBook>,
    player: Res<Player>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut guard: ResMut<Guard>,
    mut survival: ResMut<crate::survival::Survival>,
    mut foes: Query<(&Transform, &mut Foe)>,
    mut sound: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
    mut impact: MessageWriter<super::feel::Impact>,
    mut cooldown: Local<f32>,
) {
    let dt = time.delta_secs();
    *cooldown -= dt;
    let before = guard.bash;
    if let Some(t) = guard.bash {
        let next = t + dt / BASH_TIME;
        guard.bash = (next < 1.0).then_some(next);
    }
    let Some(shield) = guard.shield.filter(|_| guard.up).and_then(|i| items.get(i).shield.map(|s| (i, s))) else {
        guard.bash = None;
        return;
    };
    if guard.bash.is_none()
        && *cooldown <= 0.0
        && mouse.just_pressed(MouseButton::Left)
        && !menus.open
        && stamina::spend(&mut survival.vitals.health, BASH_COST * items.get(shield.0).stamina)
    {
        guard.bash = Some(0.0);
        *cooldown = COOLDOWN;
        sound.write(PlaySound::new("swing").speed(0.7));
        return;
    }
    let landed = matches!((before, guard.bash), (Some(a), b) if a < CONTACT && b.is_none_or(|b| b >= CONTACT));
    if !landed {
        return;
    }
    let ahead = cam.forward().as_vec3();
    let target = foes
        .iter_mut()
        .filter(|(tf, _)| {
            let to = tf.translation - player.pos;
            flat(to).length() <= REACH + super::HALF && to.y.abs() < 1.5 && ahead_of(ahead, to, ARC)
        })
        .min_by(|a, b| flat(a.0.translation - player.pos).length().total_cmp(&flat(b.0.translation - player.pos).length()));
    let Some((tf, mut foe)) = target else { return };
    let k = &book.kinds[foe.kind];
    let backstab = if foe.brain.aware { 1.0 } else { super::ai::BACKSTAB };
    foe.health -= shield.1.bash * backstab;
    foe.brain.kicked(&traits(k));
    let dir = flat(tf.translation - player.pos).normalize_or(flat(ahead).normalize_or(Vec3::NEG_Z));
    foe.vel = dir * SHOVE + Vec3::Y * 2.0;
    foe.on_ground = false;
    foe.flinch = 1.0;
    foe.kicked = super::kick::SLAM_WINDOW;
    let at = tf.translation + Vec3::Y * 1.2;
    sound.write(PlaySound::new("clash").speed(0.65).at(at));
    sound.write(PlaySound::new("hit").speed(0.75).at(at));
    bursts.write(Burst::new(at, BurstKind::Chips, 8));
    impact.write(super::feel::Impact { strength: 0.65 });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shield_is_furthest_out_when_it_lands() {
        assert_eq!(bash_reach(0.0), 0.0);
        assert_eq!(bash_reach(CONTACT), 1.0);
        assert!(bash_reach(0.5) == 1.0 && bash_reach(1.0) < 1e-6);
        // Out quicker than it comes back.
        assert!(bash_reach(CONTACT * 0.5) < 0.5);
    }
}
