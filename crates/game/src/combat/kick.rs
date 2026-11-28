//! The kick (Q): one foe close in front is booted back hard, reeling with its
//! guard broken. Kicked into a wall it slams and goes down for longer and
//! takes a knock. It wears Fatigue down (`stamina`); winded, you can't kick.
//!
//! In first person a leg swings up into view (`LegView`); in third person your
//! body's right leg kicks (`Drive::kick`, from `leg_body`). The poses are pure
//! (`leg_view`, `leg_body`) and tested.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::stamina;
use super::{ahead_of, ai, feel, flat, traits, Foe, FoeBook};
use crate::camera::{FlyCamera, Menus};
use crate::player::{MoveMode, Player};
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};

/// Seconds a kick takes, and when in it the foot lands (0..1).
pub const KICK_TIME: f32 = 0.5;
pub const CONTACT: f32 = 0.38;
/// Seconds from one kick to the next.
const COOLDOWN: f32 = 0.75;
/// How far the foot reaches (from your feet) and half its arc, degrees.
const REACH: f32 = 2.6;
const ARC: f32 = 50.0;
/// Damage of the kick itself, and of being slammed into a wall.
const DAMAGE: f32 = 4.0;
const SLAM_DAMAGE: f32 = 8.0;
/// How hard it sends a foe back (blocks/s).
const SHOVE: f32 = 11.0;
/// Seconds after a kick that hitting a wall counts as a slam.
pub const SLAM_WINDOW: f32 = 0.6;
/// The key.
pub const KEY: KeyCode = KeyCode::KeyQ;

/// A kick under way.
#[derive(Resource, Default, Debug)]
pub struct Kicking {
    /// How far through, 0..1; `None` when not kicking.
    pub t: Option<f32>,
    cooldown: f32,
}

#[derive(Component)]
struct LegView;

pub struct KickPlugin;

impl Plugin for KickPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Kicking>().add_systems(crate::menu::GameUpdate, (spawn_leg, pose_leg).chain().after(crate::player::PlayerSet));
    }
}

/// One key: when (0..1) and the values there; eased smoothly between keys.
type Key<const N: usize> = (f32, [f32; N]);

fn sample<const N: usize>(keys: &[Key<N>], t: f32) -> [f32; N] {
    let t = t.clamp(0.0, 1.0);
    let i = keys.iter().position(|k| k.0 >= t).unwrap_or(keys.len() - 1).max(1);
    let (a, b) = (keys[i - 1], keys[i]);
    let x = if b.0 > a.0 { (t - a.0) / (b.0 - a.0) } else { 1.0 };
    let x = x * x * (3.0 - 2.0 * x);
    std::array::from_fn(|j| a.1[j] + (b.1[j] - a.1[j]) * x)
}

/// Third person: [hip, knee, lean] of the kicking (right) leg. Chambered
/// (knee drawn up), snapped out straight at contact, held, put back down.
const BODY: [Key<3>; 5] = [
    (0.0, [0.0, 0.0, 0.0]),
    (0.22, [1.25, 1.9, -0.08]),
    (CONTACT, [1.5, 0.05, -0.28]),
    (0.58, [1.35, 0.2, -0.22]),
    (1.0, [0.0, 0.0, 0.0]),
];

/// First person: the leg's knee position (camera space) and pitch. It comes
/// up from below the view, chambered, then straight out ahead.
const VIEW: [Key<4>; 5] = [
    (0.0, [0.14, -1.35, -0.25, 1.0]),
    (0.22, [0.12, -0.78, -0.42, 1.05]),
    (CONTACT, [0.07, -0.62, -0.62, 0.22]),
    (0.58, [0.08, -0.66, -0.58, 0.3]),
    (1.0, [0.14, -1.35, -0.25, 1.0]),
];

/// The body's right leg `t` (0..1) through a kick: (hip, knee, lean back).
pub fn leg_body(t: f32) -> (f32, f32, f32) {
    let v = sample(&BODY, t);
    (v[0], v[1], v[2])
}

/// The first-person leg `t` (0..1) through a kick: knee position and turn.
pub fn leg_view(t: f32) -> (Vec3, Quat) {
    let v = sample(&VIEW, t);
    (Vec3::new(v[0], v[1], v[2]), Quat::from_rotation_x(v[3]))
}

/// Q: start a kick; at contact the foe in front takes it. Also deals the
/// slam of any foe a kick sent into a wall (`Foe::slam`, set by `walk`).
#[allow(clippy::too_many_arguments)]
pub(super) fn kick(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    (menus, mode): (Res<Menus>, Res<MoveMode>),
    book: Res<FoeBook>,
    player: Res<Player>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut kicking: ResMut<Kicking>,
    mut survival: ResMut<crate::survival::Survival>,
    mut foes: Query<(&Transform, &mut Foe)>,
    mut sound: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
    mut impact: MessageWriter<feel::Impact>,
) {
    let dt = time.delta_secs();

    // Kicked into a wall.
    for (tf, mut foe) in &mut foes {
        if !foe.slam {
            continue;
        }
        foe.slam = false;
        foe.kicked = 0.0;
        foe.health -= SLAM_DAMAGE;
        foe.brain.slammed();
        foe.flinch = 1.0;
        let at = tf.translation + Vec3::Y * 1.0;
        sound.write(PlaySound::new("hit").speed(0.6).at(at));
        sound.write(PlaySound::new("dig_hard").speed(0.8).at(at));
        bursts.write(Burst::new(at, BurstKind::Chips, 12));
        impact.write(feel::Impact { strength: 0.7 });
    }

    kicking.cooldown -= dt;
    let before = kicking.t;
    if let Some(t) = kicking.t {
        let next = t + dt / KICK_TIME;
        kicking.t = (next < 1.0).then_some(next);
    }
    if kicking.t.is_none()
        && kicking.cooldown <= 0.0
        && keys.just_pressed(KEY)
        && !menus.open
        && !menus.still
        && *mode == MoveMode::Walk
        && stamina::spend(&mut survival.vitals.health, stamina::KICK)
    {
        kicking.t = Some(0.0);
        kicking.cooldown = COOLDOWN;
        sound.write(PlaySound::new("swing").speed(0.6));
        return;
    }

    // The foot lands: the nearest foe in front takes it.
    let landed = matches!((before, kicking.t), (Some(a), b) if a < CONTACT && b.is_none_or(|b| b >= CONTACT));
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
    let backstab = if foe.brain.aware { 1.0 } else { ai::BACKSTAB };
    foe.health -= DAMAGE * backstab;
    foe.brain.kicked(&traits(k));
    let dir = flat(tf.translation - player.pos).normalize_or(flat(ahead).normalize_or(Vec3::NEG_Z));
    foe.vel = dir * SHOVE + Vec3::Y * 3.0;
    foe.on_ground = false;
    foe.flinch = 1.0;
    foe.kicked = SLAM_WINDOW;
    let at = tf.translation + Vec3::Y * 1.0;
    sound.write(PlaySound::new("hit").speed(0.8).at(at));
    bursts.write(Burst::new(at, BurstKind::Smoke, 6));
    impact.write(feel::Impact { strength: 0.6 });
}

/// The first-person leg: shin and boot under the camera, hidden until a kick.
fn spawn_leg(
    mut commands: Commands,
    cam: Single<Entity, With<FlyCamera>>,
    have: Query<(), With<LegView>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !have.is_empty() {
        return;
    }
    let cloth = materials.add(StandardMaterial { base_color: Color::srgb(0.24, 0.2, 0.16), perceptual_roughness: 0.95, ..default() });
    let leather = materials.add(StandardMaterial { base_color: Color::srgb(0.13, 0.1, 0.08), perceptual_roughness: 0.8, ..default() });
    commands
        .spawn((LegView, Transform::default(), Visibility::Hidden, ChildOf(*cam)))
        .with_children(|leg| {
            // The shin runs forward (-z) from the knee; the boot is at its end.
            leg.spawn((Mesh3d(meshes.add(Cuboid::new(0.2, 0.2, 0.62))), MeshMaterial3d(cloth), Transform::from_xyz(0.0, 0.0, -0.31), NotShadowCaster));
            leg.spawn((Mesh3d(meshes.add(Cuboid::new(0.24, 0.3, 0.26))), MeshMaterial3d(leather), Transform::from_xyz(0.0, 0.04, -0.72), NotShadowCaster));
        });
}

fn pose_leg(kicking: Res<Kicking>, view: Res<crate::camera::View>, mut leg: Query<(&mut Transform, &mut Visibility), With<LegView>>) {
    for (mut tf, mut vis) in &mut leg {
        let want = match kicking.t {
            Some(t) if !view.pulled => {
                (tf.translation, tf.rotation) = leg_view(t);
                Visibility::Inherited
            }
            _ => Visibility::Hidden,
        };
        if *vis != want {
            *vis = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leg_is_out_furthest_when_the_foot_lands() {
        let reach = |t: f32| -leg_view(t).0.z;
        let furthest = (0..=200).map(|i| i as f32 / 200.0).max_by(|a, b| reach(*a).total_cmp(&reach(*b))).unwrap();
        assert!((furthest - CONTACT).abs() < 0.01, "{furthest}");
        // Out of view at rest, both ends.
        assert!(leg_view(0.0).0.y < -1.0 && leg_view(1.0).0.y < -1.0);
        // The body's leg: chambered (knee bent) before, straight at contact.
        let (_, chamber, _) = leg_body(0.22);
        let (hip, knee, lean) = leg_body(CONTACT);
        assert!(chamber > 1.5 && knee < 0.2 && hip > 1.3 && lean < 0.0);
        assert_eq!(leg_body(0.0), (0.0, 0.0, 0.0));
        assert_eq!(leg_body(1.0), (0.0, 0.0, 0.0));
    }
}
