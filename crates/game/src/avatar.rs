//! Your body: the same jointed model as the NPCs (`npc::body`), with your
//! looks from `assets/data/player.ron`. It shows in third person (P, see
//! `camera::View`): it walks with you, its head follows where you look, and
//! its arms hold and swing what you hold with the same timing as the
//! first-person arm (`swing::Swing::limbs`), so blows land together.

use bevy::prelude::*;
use serde::Deserialize;

use crate::camera::{FlyCamera, View};
use crate::hands::Held;
use crate::interact::{Hand, HandAnim, Wielded};
use crate::items::Items;
use crate::npc::body::{self, Drive, Parts, Rig};
use crate::npc::data::Looks;
use crate::npc::dialogue::Mood;
use crate::npc::motion::{self, wrap, Spring};
use crate::player::{MoveMode, Player};
use crate::swing::{Limbs, Swing};

pub struct AvatarPlugin;

impl Plugin for AvatarPlugin {
    fn build(&self, app: &mut App) {
        let file: PlayerFile = ron::from_str(include_str!("../../../assets/data/player.ron")).unwrap_or_else(|e| panic!("player.ron: {e}"));
        app.insert_resource(PlayerLooks(file.looks))
            .add_systems(crate::menu::GameUpdate, (spawn_body, animate).chain().after(crate::player::PlayerSet).after(crate::interact::InteractSet));
    }
}

#[derive(Deserialize)]
struct PlayerFile {
    looks: Looks,
}

/// Your colours (also used for the first-person arm).
#[derive(Resource)]
pub struct PlayerLooks(pub Looks);

/// The root of your body.
#[derive(Component)]
pub struct PlayerBody;

/// How big a held item is on the body, against the first-person sprite scale.
const HELD_SCALE: f32 = 1.6;
/// Walking speed the stride is measured against (`player`'s walk).
const WALK: f32 = 4.4;
/// Standing still, the body turns to where you look once the head has turned
/// this far (Minecraft: 50°).
const TURN_AT: f32 = 50.0 * std::f32::consts::PI / 180.0;
/// Seconds the body takes to turn to its new facing (quicker than an NPC's
/// amble, close to Minecraft's).
const TURN_TIME: f32 = 0.13;
/// Lean (radians) per block/s² of speeding up or slowing down, and the most.
const LEAN_PER_ACCEL: f32 = 0.007;
const MAX_LEAN: f32 = 0.14;

fn spawn_body(
    mut commands: Commands,
    looks: Res<PlayerLooks>,
    player: Res<Player>,
    existing: Query<(), With<PlayerBody>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !existing.is_empty() || !player.ready {
        return;
    }
    let (root, rig) = body::build(&mut commands, &mut meshes, &mut materials, &looks.0, player.pos, 0.0);
    commands.spawn((Held::new(Hand::Right), Transform::from_scale(Vec3::splat(HELD_SCALE)), Visibility::default(), ChildOf(rig.hands[1])));
    commands.spawn((Held::new(Hand::Left), Transform::from_scale(Vec3::splat(HELD_SCALE)), Visibility::default(), ChildOf(rig.hands[0])));
    let dressed = crate::inventory::preview::Dressed::new(rig.anchors());
    commands.entity(root).insert((PlayerBody, Name::new("Player body"), rig, dressed, Visibility::Hidden));
}

#[allow(clippy::too_many_arguments)]
fn animate(
    real: Res<Time<Real>>,
    game: Res<Time>,
    player: Res<Player>,
    mode: Res<MoveMode>,
    view: Res<View>,
    anim: Res<HandAnim>,
    wielded: Res<Wielded>,
    items: Res<Items>,
    cam: Single<&FlyCamera>,
    guard: Res<crate::combat::Guard>,
    kicking: Res<crate::combat::kick::Kicking>,
    mut frozen: Local<Option<Option<f32>>>,
    mut facing: Local<f32>,
    mut weight: Local<(Vec2, [Spring; 2])>,
    mut body: Query<(&mut Transform, &mut Rig, &mut Visibility), With<PlayerBody>>,
    mut parts: Parts,
) {
    let Ok((mut tf, mut rig, mut vis)) = body.single_mut() else { return };
    let want = if view.pulled { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    if !view.pulled {
        return;
    }
    tf.translation = player.pos;

    // Arms: swinging, holding something ready, or hanging free.
    let frozen = *frozen.get_or_insert_with(crate::hands::frozen_swing);
    let held = wielded.0.map(|i| items.get(i).swing);
    let arm = anim.arm(Hand::Right);
    let arms = match (frozen, arm.t) {
        (Some(t), _) => Some(held.unwrap_or(Swing::Jab).limbs(t)),
        (None, Some(t)) => Some(arm.style.limbs(t)),
        (None, None) => match anim.charge_level() {
            Some(k) => Some(crate::swing::charge_limbs(k)),
            None => held.map(|_| Limbs::hold().lerp(Limbs::guard(), guard.blend)),
        },
    };

    // Facing (Minecraft-style): moving, the body turns toward where you go,
    // at most 75° from where you look, and keeps facing forward when you
    // back up; working, it squares up to what you hit; standing, the head
    // turns first and the body follows once it has turned 50°.
    let walking = *mode == MoveMode::Walk;
    let vel = if walking { Vec2::new(player.vel.x, player.vel.z) } else { Vec2::ZERO };
    let speed = vel.length();
    if arm.t.is_some() || frozen.is_some() {
        *facing = cam.yaw;
    } else if speed > 0.3 {
        *facing = motion::moving_facing(cam.yaw, crate::npc::yaw_of(Vec3::new(vel.x, 0.0, vel.y)));
    } else if wrap(cam.yaw - *facing).abs() > TURN_AT {
        *facing = cam.yaw;
    }

    // Weight: lean into speeding up, rock back when stopping, tip into turns.
    let gdt = game.delta_secs().min(0.05);
    let (prev, springs) = &mut *weight;
    let accel = if gdt > 0.0 { (vel - *prev) / gdt } else { Vec2::ZERO };
    *prev = vel;
    let yaw = rig.facing();
    let ahead = Vec2::new(-yaw.sin(), -yaw.cos());
    let left = Vec2::new(-yaw.cos(), yaw.sin());
    let lean = (accel.dot(ahead) * LEAN_PER_ACCEL).clamp(-MAX_LEAN, MAX_LEAN);
    let tip = (accel.dot(left) * LEAN_PER_ACCEL * 0.6).clamp(-MAX_LEAN, MAX_LEAN);
    let dt = real.delta_secs().min(0.05);
    let sway = [springs[0].step(lean, 0.25, dt), springs[1].step(tip, 0.25, dt)];

    let drive = Drive {
        yaw: *facing,
        turn: TURN_TIME,
        speed,
        walk: WALK,
        game_dt: gdt,
        focus: Some([wrap(cam.yaw - rig.facing()), cam.pitch]),
        gesture: 0.0,
        mood: Mood::Neutral,
        mouth: (0.0, 0.85),
        speaking: 0.0,
        arms,
        sway,
        act: None,
        kick: kicking.t,
    };
    body::drive(&mut tf, &mut rig, &mut parts, dt, &drive);
}
