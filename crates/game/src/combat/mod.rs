//! Sword fighting and the foes you fight.
//!
//! - `data`: foe kinds from `assets/data/foes.ron`.
//! - `ai`: pure foe minds (tactics, giving up, taking blows).
//! - `path`: pure A* over the block grid, for chasing you round walls.
//!
//! This file: your guard (hold right click with a sword) and shove (Q), foes
//! on the NPC rig holding swords, their walking and pathing, cuts landing
//! both ways (blocked, parried or hurting), ghosts (see-through, shimmering,
//! trailing wisps), death and drops, and the dev keys: Ctrl+Shift+J (Cmd on
//! a Mac) spawns a Hollow Warden, Ctrl+Shift+H a Fen Shade.

pub mod ai;
pub mod data;
pub mod path;
mod saved;

use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::{FlyCamera, Menus};
use crate::creatures::brain::Rng;
use crate::hands::Held;
use crate::interact::{Aimed, Hand, HandAnim, InteractSet, Strike, Swung, Wielded};
use crate::inventory::grid::Stack;
use crate::inventory::Inventory;
use crate::items::Items;
use crate::npc::body::{self, Drive, Parts, Rig};
use crate::npc::dialogue::Mood;
use crate::npc::{yaw_of, EYES, HEIGHT};
use crate::player::{Damage, MoveMode, Player};
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};
use crate::swing::{Limbs, Swing};
use crate::world::World;
use ai::{Brain, Senses, Tactic, Took, Traits};
use data::FoeKind;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        let kinds = data::load(include_str!("../../../../assets/data/foes.ron")).unwrap_or_else(|e| panic!("foes.ron: {e}"));
        app.insert_resource(FoeBook { kinds })
            .insert_resource(FoeDice(Rng(0xF0E5_5EED_1234)))
            .init_resource::<Guard>()
            .init_resource::<ShoveTimer>()
            .add_message::<SpawnFoe>()
            .add_message::<Shoot>()
            .add_message::<FoeSlain>()
            .add_systems(crate::menu::GamePostStartup, saved::load)
            .add_systems(crate::menu::GameUpdate, saved::save_now_and_then)
            .add_systems(Last, saved::save_on_exit.run_if(crate::menu::in_game))
            .add_systems(
                crate::menu::GameUpdate,
                (guard_input, aim_foes).chain().after(crate::creatures::aim).after(crate::player::PlayerSet).before(InteractSet),
            )
            .add_systems(
                crate::menu::GameUpdate,
                (spawn_key, spawn, take_swings, take_strikes, shove, think, shoot, fly_bolts, walk, animate, haunt, fall)
                    .chain()
                    .after(InteractSet)
                    .after(crate::npc::NpcSet),
            );
    }
}

/// How far a sword cut reaches from your eyes.
pub const SWORD_REACH: f32 = 3.0;
/// Half the width of a sword's sweep, degrees.
const SWEEP: f32 = 55.0;
/// Shove: reach, half its arc (degrees), seconds between shoves.
const SHOVE_REACH: f32 = 3.4;
const SHOVE_ARC: f32 = 75.0;
const SHOVE_EVERY: f32 = 2.0;
/// Half the width of a foe's body.
const HALF: f32 = 0.3;
const GRAVITY: f32 = 25.0;
const HOP: f32 = 7.6;
/// How big the sword in a foe's fist is (as `avatar`'s).
const HELD_SCALE: f32 = 1.6;
/// Seconds between path searches while chasing.
const REPATH: f32 = 0.4;
/// Searches stay within this many blocks of the foe.
const PATH_RADIUS: i32 = 28;

#[derive(Resource)]
pub struct FoeBook {
    pub kinds: Vec<FoeKind>,
}

#[derive(Resource)]
struct FoeDice(Rng);

/// Your sword raised to block (hold right click with a sword in hand).
#[derive(Resource, Default, Debug)]
pub struct Guard {
    pub up: bool,
    /// Seconds it has been up (a cut caught in the first moments is parried).
    pub held: f32,
    /// 0..1, eased: how far the arm has come up (for the poses).
    pub blend: f32,
}

#[derive(Resource, Default)]
struct ShoveTimer(f32);

/// Put a foe of a kind in the world, feet at `at`.
#[derive(Message, Debug, Clone, Copy)]
pub struct SpawnFoe {
    pub kind: usize,
    pub at: Vec3,
    /// Scales its health and damage (deeper dungeon floors are tougher). 1 = as in foes.ron.
    pub boost: f32,
    /// Tagged on the foe entity, so whoever spawned it can tell it apart (0 = none).
    pub tag: u32,
    /// Where it keeps to, if not `at` (a foe back from a save).
    pub home: Option<Vec3>,
    /// Its health, if not full (a foe back from a save).
    pub health: Option<f32>,
}

impl SpawnFoe {
    pub fn new(kind: usize, at: Vec3) -> Self {
        Self { kind, at, boost: 1.0, tag: 0, home: None, health: None }
    }
}

/// A foe fell (just before it is despawned).
#[derive(Message, Debug, Clone, Copy)]
pub struct FoeSlain {
    pub tag: u32,
    /// Where it fell (its middle).
    pub at: Vec3,
}

/// A ranged foe looses a bolt.
#[derive(Message, Debug, Clone, Copy)]
struct Shoot {
    from: Vec3,
    vel: Vec3,
    damage: f32,
    cause: crate::creatures::data::Cause,
    color: (f32, f32, f32),
}

/// A bolt in flight: hits you (or your raised blade) or the first wall.
#[derive(Component)]
pub struct Bolt {
    vel: Vec3,
    damage: f32,
    cause: crate::creatures::data::Cause,
    life: f32,
}

#[derive(Component, Debug)]
pub struct Foe {
    pub kind: usize,
    pub health: f32,
    pub brain: Brain,
    pub vel: Vec3,
    /// Where the body faces.
    pub yaw: f32,
    pub on_ground: bool,
    /// Where it started; it won't chase far from here.
    pub home: Vec3,
    want: Vec3,
    way: Vec<IVec3>,
    way_to: Option<IVec3>,
    repath: f32,
    bumped: bool,
    /// Eased 0..1: how far its blade is raised.
    guard: f32,
    /// Health and damage scale (`SpawnFoe::boost`).
    pub boost: f32,
    /// `SpawnFoe::tag`.
    pub tag: u32,
}

/// A spirit's look: its own materials (made see-through and blue once its
/// body is built) and the clock that makes them shimmer.
#[derive(Component, Default)]
pub struct Ghost {
    mats: Vec<Handle<StandardMaterial>>,
    t: f32,
    wisp: f32,
}

fn traits(k: &FoeKind) -> Traits {
    Traits { style: k.style, reach: k.reach, sight: k.sight, forget: k.forget, leash: k.leash, swing: k.swing, contact: Swing::Slash.contact(), ranged: k.ranged.is_some() }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Is `to` within `arc` degrees either side of the flat direction `ahead`?
fn ahead_of(ahead: Vec3, to: Vec3, arc: f32) -> bool {
    let (a, t) = (flat(ahead).normalize_or_zero(), flat(to).normalize_or_zero());
    t == Vec3::ZERO || a.dot(t) >= arc.to_radians().cos()
}

fn solid(world: &World, blocks: &Blocks, p: BlockPos) -> bool {
    world.block(p).is_none_or(|b| blocks.registry.get(b).solid)
}

/// Raise or lower your guard.
fn guard_input(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    menus: Res<Menus>,
    wielded: Res<Wielded>,
    items: Res<Items>,
    anim: Res<HandAnim>,
    mut guard: ResMut<Guard>,
) {
    let dt = time.delta_secs();
    let sword = wielded.0.is_some_and(|i| items.get(i).swing == Swing::Slash);
    // Not mid-cut: finish the swing first.
    let up = sword && !menus.open && mouse.pressed(MouseButton::Right) && anim.arm(Hand::Right).t.is_none();
    guard.held = if up { guard.held + dt } else { 0.0 };
    guard.up = up;
    let want = if up { 1.0 } else { 0.0 };
    guard.blend += (want - guard.blend) * (dt * 16.0).min(1.0);
}

/// A foe under the crosshair nearer than any creature takes your clicks.
fn aim_foes(cam: Single<&Transform, With<FlyCamera>>, mode: Res<MoveMode>, q: Query<(Entity, &Transform), With<Foe>>, mut aimed: ResMut<Aimed>) {
    let origin = cam.translation;
    let dir = cam.forward().as_vec3();
    let reach = if *mode == MoveMode::Fly { crate::interact::FLY_REACH } else { crate::interact::REACH };
    for (e, tf) in &q {
        let half = Vec3::new(HALF, 0.0, HALF);
        if let Some(t) = crate::interact::ray_box(origin, dir, tf.translation - half, tf.translation + half + Vec3::Y * HEIGHT)
            && t <= reach
            && t < aimed.dist
        {
            *aimed = Aimed { entity: Some(e), dist: t };
        }
    }
}

/// Ctrl+Shift+J (Cmd+Shift+J) spawns a Hollow Warden a few blocks ahead,
/// Ctrl+Shift+H a Fen Shade. If you have no sword yet, one goes in your
/// pockets and on the hotbar so you can fight it.
#[allow(clippy::too_many_arguments)]
fn spawn_key(
    keys: Res<ButtonInput<KeyCode>>,
    book: Res<FoeBook>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<Blocks>,
    items: Res<Items>,
    cam: Single<&Transform, With<FlyCamera>>,
    (mut inv, mut hotbar, mut hands): (ResMut<Inventory>, ResMut<crate::interact::Hotbar>, ResMut<crate::interact::Hands>),
    mut spawn: MessageWriter<SpawnFoe>,
) {
    let held = |a, b| keys.pressed(a) || keys.pressed(b);
    let combo = (held(KeyCode::ControlLeft, KeyCode::ControlRight) || held(KeyCode::SuperLeft, KeyCode::SuperRight)) && held(KeyCode::ShiftLeft, KeyCode::ShiftRight);
    let name = match () {
        _ if combo && keys.just_pressed(KeyCode::KeyJ) => "hollow",
        _ if combo && keys.just_pressed(KeyCode::KeyH) => "shade",
        _ => return,
    };
    let Some(kind) = book.kinds.iter().position(|k| k.name == name) else { return };
    let fwd = flat(cam.forward().as_vec3()).normalize_or(Vec3::NEG_Z);
    let at = player.pos + fwd * 6.0;
    let (x, z) = (at.x.floor() as i32, at.z.floor() as i32);
    let y = crate::creatures::ground_near(&world, &blocks, x, z, at.y as i32 - 8, at.y as i32 + 6, false).map_or(at.y, |y| y as f32);
    spawn.write(SpawnFoe::new(kind, Vec3::new(x as f32 + 0.5, y, z as f32 + 0.5)));

    // A sword to test with.
    let has_sword = items.ids().any(|i| items.get(i).swing == Swing::Slash && inv.count(i) > 0);
    if !has_sword && let Some(sword) = items.id("iron_sword") {
        inv.add(&items, Stack::new(sword, 1));
        let slot = hotbar.slots.iter().position(|s| s.is_none()).unwrap_or(0);
        hotbar.slots[slot] = Some(sword);
        hands.right = Some(slot);
        hands.drawn = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    mut commands: Commands,
    mut msgs: MessageReader<SpawnFoe>,
    book: Res<FoeBook>,
    items: Res<Items>,
    player: Res<Player>,
    mut dice: ResMut<FoeDice>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for m in msgs.read() {
        let Some(k) = book.kinds.get(m.kind) else { continue };
        let yaw = yaw_of(player.pos - m.at);
        let (root, rig) = body::build(&mut commands, &mut meshes, &mut materials, &k.looks, m.at, yaw);
        if let Some(weapon) = items.id(&k.weapon) {
            commands.spawn((Held::holding(weapon), Transform::from_scale(Vec3::splat(HELD_SCALE)), Visibility::default(), ChildOf(rig.hands[1])));
        }
        let foe = Foe {
            kind: m.kind,
            health: m.health.unwrap_or(k.health * m.boost),
            brain: Brain::new(&k.style, dice.0.next()),
            vel: Vec3::ZERO,
            yaw,
            on_ground: false,
            home: m.home.unwrap_or(m.at),
            want: Vec3::ZERO,
            way: Vec::new(),
            way_to: None,
            repath: 0.0,
            bumped: false,
            guard: 0.0,
            boost: m.boost,
            tag: m.tag,
        };
        commands.entity(root).insert((Name::new(k.title.clone()), foe, rig));
        if k.ghost {
            commands.entity(root).insert(Ghost::default());
            // A faint blue glow about it.
            commands.spawn((
                PointLight { intensity: 18_000.0, range: 6.0, color: Color::srgb(0.45, 0.7, 1.0), shadow_maps_enabled: false, ..default() },
                Transform::from_xyz(0.0, 1.2, 0.0),
                ChildOf(root),
            ));
        }
    }
}

/// What a blow on a foe does: blocked on its blade, its guard broken, or a wound.
#[allow(clippy::too_many_arguments)]
fn hit_foe(tf: &Transform, foe: &mut Foe, k: &FoeKind, from: Vec3, damage: f32, dice: &mut Rng, sound: &mut MessageWriter<PlaySound>, bursts: &mut MessageWriter<Burst>) {
    let to_you = flat(from - tf.translation);
    let facing = ahead_of(Vec3::new(-foe.yaw.sin(), 0.0, -foe.yaw.cos()), to_you, ai::GUARD_ARC);
    let chest = tf.translation + Vec3::Y * 1.3 + to_you.normalize_or_zero() * 0.4;
    let push = -to_you.normalize_or_zero();
    match foe.brain.struck(facing, &traits(k), dice) {
        Took::Blocked => {
            sound.write(PlaySound::new("clash").gain(0.9));
            bursts.write(Burst::new(chest, BurstKind::Sparks, 10));
            foe.vel += push * 2.5;
        }
        Took::GuardBroken => {
            sound.write(PlaySound::new("clash").speed(0.8));
            sound.write(PlaySound::new("hit"));
            bursts.write(Burst::new(chest, BurstKind::Sparks, 18));
            foe.vel += push * 5.0 + Vec3::Y * 2.5;
        }
        Took::Hurt => {
            foe.health -= damage;
            foe.vel += push * 5.0 + Vec3::Y * 3.0;
            foe.on_ground = false;
            if k.ghost {
                sound.write(PlaySound::new("spirit").speed(1.7).gain(0.7));
                bursts.write(Burst::new(chest, BurstKind::Wisp, 10));
            } else {
                sound.write(PlaySound::new("creature_hurt").speed(0.75));
                bursts.write(Burst::new(chest, BurstKind::Bits, 6));
            }
        }
    }
}

/// Your sword cuts land: every foe in the sweep in front of you; a creature
/// you aim at takes it as a strike.
#[allow(clippy::too_many_arguments)]
fn take_swings(
    mut swung: MessageReader<Swung>,
    book: Res<FoeBook>,
    aimed: Res<Aimed>,
    creatures: Query<(), With<crate::creatures::Creature>>,
    mut foes: Query<(Entity, &Transform, &mut Foe)>,
    mut dice: ResMut<FoeDice>,
    mut strikes: MessageWriter<Strike>,
    mut sound: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
) {
    for s in swung.read() {
        sound.write(PlaySound::new("swing").speed(1.25).gain(0.7));
        for (_, tf, mut foe) in &mut foes {
            let mid = tf.translation + Vec3::Y * 1.0;
            let to = mid - s.origin;
            if flat(to).length() > s.reach + HALF || to.y.abs() > 2.2 || !ahead_of(s.dir, to, SWEEP) {
                continue;
            }
            let k = &book.kinds[foe.kind];
            hit_foe(tf, &mut foe, k, s.origin, s.damage, &mut dice.0, &mut sound, &mut bursts);
        }
        if let Some(e) = aimed.entity
            && creatures.contains(e)
            && aimed.dist <= s.reach
        {
            strikes.write(Strike { entity: e, damage: s.damage, dir: flat(s.dir).normalize_or_zero() });
        }
    }
}

/// Blows from anything but a sword (an axe, a fist) on the foe you aim at.
#[allow(clippy::too_many_arguments)]
fn take_strikes(
    mut strikes: MessageReader<Strike>,
    book: Res<FoeBook>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut foes: Query<(Entity, &Transform, &mut Foe)>,
    mut dice: ResMut<FoeDice>,
    mut sound: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
) {
    for s in strikes.read() {
        let Ok((_, tf, mut foe)) = foes.get_mut(s.entity) else { continue };
        let k = &book.kinds[foe.kind];
        hit_foe(tf, &mut foe, k, cam.translation, s.damage, &mut dice.0, &mut sound, &mut bursts);
    }
}

/// Q shoves every foe close in front of you back and off balance.
#[allow(clippy::too_many_arguments)]
fn shove(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    menus: Res<Menus>,
    book: Res<FoeBook>,
    player: Res<Player>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut timer: ResMut<ShoveTimer>,
    mut anim: ResMut<HandAnim>,
    mut foes: Query<(&Transform, &mut Foe)>,
    mut sound: MessageWriter<PlaySound>,
) {
    timer.0 -= time.delta_secs();
    if menus.open || !keys.just_pressed(KeyCode::KeyQ) || timer.0 > 0.0 {
        return;
    }
    timer.0 = SHOVE_EVERY;
    anim.start(Hand::Right, Swing::Jab);
    sound.write(PlaySound::new("swing").speed(0.7));
    let ahead = cam.forward().as_vec3();
    let mut any = false;
    for (tf, mut foe) in &mut foes {
        let to = tf.translation - player.pos;
        if flat(to).length() > SHOVE_REACH || to.y.abs() > 2.0 || !ahead_of(ahead, to, SHOVE_ARC) {
            continue;
        }
        let k = &book.kinds[foe.kind];
        foe.brain.shoved(&traits(k));
        foe.vel = flat(to).normalize_or(flat(ahead)) * 9.0 + Vec3::Y * 3.5;
        foe.on_ground = false;
        any = true;
    }
    if any {
        sound.write(PlaySound::new("hit").speed(0.8));
    }
}

/// Foe minds: notice you, pick tactics, find a way to you, swing; their
/// cuts land on you or on your blade.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn think(
    time: Res<Time>,
    book: Res<FoeBook>,
    player: Res<Player>,
    mode: Res<MoveMode>,
    survival: Res<crate::survival::Survival>,
    (world, blocks): (Res<World>, Res<Blocks>),
    (cam, anim, guard): (Single<&Transform, With<FlyCamera>>, Res<HandAnim>, Res<Guard>),
    mut dice: ResMut<FoeDice>,
    mut foes: Query<(Entity, &Transform, &mut Foe)>,
    mut hits: MessageWriter<Damage>,
    mut sound: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
    mut shots: MessageWriter<Shoot>,
) {
    let dt = time.delta_secs().min(0.05);
    if dt <= 0.0 {
        return;
    }
    // Nobody fights a flying or dead player.
    let fightable = *mode == MoveMode::Walk && survival.dead_for.is_none() && player.ready;
    let eye = cam.translation;
    let ahead = cam.forward().as_vec3();
    // Your cut under way, before it lands: foes in its path see it coming.
    let arm = anim.arm(Hand::Right);
    let winding = arm.style == Swing::Slash && arm.t.is_some_and(|t| t < arm.style.contact());
    let pressing = foes.iter().filter(|(_, _, f)| f.brain.tactic.pressing()).count();
    let spots: Vec<(Entity, Vec3)> = foes.iter().map(|(e, tf, _)| (e, tf.translation)).collect();

    for (e, tf, mut foe) in &mut foes {
        let k = &book.kinds[foe.kind];
        let t = traits(k);
        let pos = tf.translation;
        let to = player.pos - pos;
        let dist = if fightable { flat(to).length() } else { f32::MAX };
        let eyes = pos + Vec3::Y * EYES;
        let sees = fightable && dist < k.sight && to.y.abs() < 8.0 && {
            let d = eye - eyes;
            voxel::raycast::raycast(eyes.to_array(), d.normalize_or_zero().to_array(), d.length(), |p| solid(&world, &blocks, p)).is_none()
        };
        let threatened = winding && flat(pos - player.pos).length() < SWORD_REACH + 0.8 && ahead_of(ahead, pos - player.pos, SWEEP);
        let senses = Senses {
            dist,
            sees,
            from_home: flat(pos - foe.home).length(),
            health: foe.health / k.health,
            threatened,
            you_guard: guard.up,
            pressing: pressing - foe.brain.tactic.pressing() as usize,
        };
        let lands = foe.brain.step(&senses, &t, &mut dice.0, dt);

        // A shooter looses its bolt at your chest instead of cutting (unless it also cuts and you're close).
        let melee_reach = k.close.unwrap_or(k.reach);
        if lands
            && let Some(r) = k.ranged.filter(|_| k.close.is_none_or(|c| dist > c))
        {
            if fightable && sees {
                let from = pos + Vec3::Y * 1.45;
                let target = player.pos + Vec3::Y * 1.1;
                let vel = (target - from).normalize_or_zero() * r.speed;
                shots.write(Shoot { from, vel, damage: k.damage * foe.boost, cause: k.cause, color: r.color });
                sound.write(PlaySound::new("knife").speed(0.7));
            }
        } else if lands
            && fightable
            && dist <= melee_reach + 0.35
            && to.y.abs() < 2.0
            && ahead_of(Vec3::new(-foe.yaw.sin(), 0.0, -foe.yaw.cos()), to, 60.0)
        {
            if guard.up && ahead_of(ahead, -to, ai::GUARD_ARC) {
                let parried = guard.held < ai::PARRY_WINDOW;
                foe.brain.blocked(parried);
                foe.vel += -flat(to).normalize_or_zero() * if parried { 6.0 } else { 3.0 };
                sound.write(PlaySound::new("clash").speed(if parried { 1.2 } else { 1.0 }));
                bursts.write(Burst::new(pos + to * 0.5 + Vec3::Y * 1.4, BurstKind::Sparks, if parried { 22 } else { 10 }));
            } else {
                hits.write(Damage { amount: k.damage * foe.boost, cause: k.cause.into() });
                sound.write(PlaySound::new("hit"));
            }
        }

        // Where to go for this tactic.
        let toward = flat(to).normalize_or_zero();
        let tactic = foe.brain.tactic;
        foe.want = match tactic {
            Tactic::Idle | Tactic::Stagger | Tactic::Recover => Vec3::ZERO,
            Tactic::Return => {
                let home = foe.home;
                follow(&mut foe, &world, &blocks, pos, home, k.walk, dt)
            }
            Tactic::Approach => follow(&mut foe, &world, &blocks, pos, player.pos, k.run, dt),
            Tactic::Press if dist < 2.5 => toward * k.run * 0.8,
            Tactic::Press => follow(&mut foe, &world, &blocks, pos, player.pos, k.run * 0.9, dt),
            Tactic::Circle => {
                if foe.bumped {
                    foe.brain.strafe = -foe.brain.strafe;
                }
                let side = Vec3::new(-toward.z, 0.0, toward.x) * foe.brain.strafe;
                let close = toward * ((dist - ai::CIRCLE_AT) * 0.8).clamp(-1.0, 1.0);
                (side + close).normalize_or_zero() * k.walk
            }
            Tactic::Guard => toward * ((dist - 2.0) * 0.6).clamp(-0.6, 0.6),
            Tactic::Backoff => -toward * k.walk * 1.1,
            // A step in behind the cut, then planted.
            Tactic::Attack if !t.ranged && (0.2..t.contact).contains(&foe.brain.swing) => toward * k.run * 0.55,
            Tactic::Attack => Vec3::ZERO,
        };
        // Don't crowd into each other.
        for (other, at) in &spots {
            let d = flat(pos - *at);
            let l = d.length();
            if *other != e && l < 1.1 && l > 1e-3 {
                foe.want += d / l * (1.1 - l) * 3.0;
            }
        }
        // Face you while fighting (committed once a cut is under way);
        // otherwise face where it goes.
        let committed = tactic == Tactic::Attack && foe.brain.swing > 0.35;
        if foe.brain.aware && dist < f32::MAX && !committed {
            foe.yaw = yaw_of(to);
        } else if !foe.brain.aware && foe.want.length_squared() > 0.05 {
            foe.yaw = yaw_of(foe.want);
        }
        foe.guard += ((tactic == Tactic::Guard) as u8 as f32 - foe.guard) * (dt * 14.0).min(1.0);
    }
}

/// The wanted velocity to walk towards `goal` along a found way.
fn follow(foe: &mut Foe, world: &World, blocks: &Blocks, pos: Vec3, goal: Vec3, speed: f32, dt: f32) -> Vec3 {
    let cell = |p: Vec3| IVec3::new(p.x.floor() as i32, (p.y + 0.01).floor() as i32, p.z.floor() as i32);
    let (here, there) = (cell(pos), cell(goal));
    foe.repath -= dt;
    let moved = foe.way_to.is_none_or(|w| (w - there).abs().max_element() > 1);
    if foe.repath <= 0.0 || moved || foe.way.is_empty() {
        foe.way = path::find(|x, y, z| solid(world, blocks, BlockPos::new(x, y, z)), here, there, PATH_RADIUS);
        foe.way_to = Some(there);
        foe.repath = REPATH;
    }
    // Drop waypoints already reached.
    while let Some(next) = foe.way.first() {
        let c = Vec3::new(next.x as f32 + 0.5, next.y as f32, next.z as f32 + 0.5);
        if flat(c - pos).length() < 0.4 && (c.y - pos.y).abs() < 1.2 {
            foe.way.remove(0);
        } else {
            break;
        }
    }
    let aim = foe.way.first().map_or(goal, |n| Vec3::new(n.x as f32 + 0.5, n.y as f32, n.z as f32 + 0.5));
    let d = flat(aim - pos);
    if d.length() < 0.15 { Vec3::ZERO } else { d.normalize() * speed }
}

/// Walking: eases to the wanted velocity, gravity, hops up one-block steps.
fn walk(time: Res<Time>, world: Res<World>, blocks: Res<Blocks>, mut q: Query<(&mut Transform, &mut Foe)>) {
    let dt = time.delta_secs().min(0.05);
    if dt <= 0.0 {
        return;
    }
    for (mut tf, mut foe) in &mut q {
        if world.block(BlockPos::from_world(tf.translation.to_array())).is_none() {
            continue;
        }
        let staggered = foe.brain.tactic == Tactic::Stagger;
        let blend = if foe.on_ground && !staggered { (8.0 * dt).min(1.0) } else { (1.5 * dt).min(1.0) };
        let want = foe.want;
        foe.vel.x += (want.x - foe.vel.x) * blend;
        foe.vel.z += (want.z - foe.vel.z) * blend;
        foe.vel.y = (foe.vel.y - GRAVITY * dt).max(-40.0);
        let mut pos = tf.translation;
        let mut bumped = false;
        for axis in [0usize, 2] {
            let mut next = pos;
            next[axis] += foe.vel[axis] * dt;
            if crate::creatures::blocked(&world, &blocks, next, HALF, HEIGHT) {
                bumped = true;
                foe.vel[axis] = 0.0;
            } else {
                pos = next;
            }
        }
        let mut next = pos;
        next.y += foe.vel.y * dt;
        foe.on_ground = false;
        if crate::creatures::blocked(&world, &blocks, next, HALF, HEIGHT) {
            if foe.vel.y < 0.0 {
                let snapped = Vec3::new(next.x, next.y.ceil(), next.z);
                if !crate::creatures::blocked(&world, &blocks, snapped, HALF, HEIGHT) {
                    pos = snapped;
                }
                foe.on_ground = true;
            }
            foe.vel.y = 0.0;
        } else {
            pos = next;
        }
        if bumped && foe.on_ground && want.length_squared() > 0.01 {
            foe.vel.y = HOP;
        }
        foe.bumped = bumped;
        tf.translation = pos;
    }
}

/// Bodies: walking, facing, and the sword arm (held ready, raised to
/// block, or cutting on the same keys as yours).
fn animate(real: Res<Time<Real>>, game: Res<Time>, book: Res<FoeBook>, cam: Single<&GlobalTransform, With<FlyCamera>>, mut foes: Query<(&mut Transform, &Foe, &mut Rig)>, mut parts: Parts) {
    let dt = real.delta_secs().min(0.05);
    let gdt = game.delta_secs().min(0.05);
    let eye = cam.translation();
    for (mut tf, foe, mut rig) in &mut foes {
        let k = &book.kinds[foe.kind];
        let d = eye - (tf.translation + Vec3::Y * EYES);
        let focus = foe.brain.aware.then(|| [crate::npc::motion::wrap(yaw_of(d) - rig.facing()), d.y.atan2(Vec2::new(d.x, d.z).length())]);
        let tactic = foe.brain.tactic;
        let arms = if tactic == Tactic::Attack { Swing::Slash.limbs(foe.brain.swing.min(1.0)) } else { Limbs::hold().lerp(Limbs::guard(), foe.guard) };
        let sway = match tactic {
            Tactic::Stagger => [-0.28, 0.08],
            Tactic::Guard => [0.08, 0.0],
            _ => [0.0; 2],
        };
        let drive = Drive {
            yaw: foe.yaw,
            turn: 0.14,
            speed: Vec2::new(foe.vel.x, foe.vel.z).length(),
            walk: k.walk,
            game_dt: gdt,
            focus,
            gesture: 0.0,
            mood: if foe.brain.aware { Mood::Angry } else { Mood::Neutral },
            mouth: (0.0, 0.85),
            speaking: 0.0,
            arms: Some(arms),
            sway,
        };
        body::drive(&mut tf, &mut rig, &mut parts, dt, &drive);
    }
}

/// Ghosts: their body goes see-through and blue once built, then shimmers
/// (each part's glow and see-through-ness ripple out of step) and trails
/// wisps that drift up and fade.
#[allow(clippy::too_many_arguments)]
fn haunt(
    time: Res<Time>,
    mut ghosts: Query<(Entity, &Transform, &mut Ghost)>,
    children: Query<&Children>,
    held: Query<(), With<Held>>,
    mesh_mats: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bursts: MessageWriter<Burst>,
    mut seed: Local<u32>,
) {
    let dt = time.delta_secs();
    if *seed == 0 {
        *seed = 0x51ad_e5ed;
    }
    let mut rnd = || {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        (*seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for (root, tf, mut g) in &mut ghosts {
        if g.mats.is_empty() {
            // Its own body's materials (not the sword's, which is shared).
            let mut stack = vec![root];
            while let Some(e) = stack.pop() {
                if let Ok(m) = mesh_mats.get(e) {
                    g.mats.push(m.0.clone());
                }
                if let Ok(c) = children.get(e) {
                    stack.extend(c.iter().filter(|c| !held.contains(*c)));
                }
            }
            for h in &g.mats {
                if let Some(mut m) = materials.get_mut(h) {
                    let c = m.base_color.to_srgba();
                    m.base_color = Color::srgba(c.red * 0.6 + 0.25, c.green * 0.6 + 0.35, c.blue * 0.5 + 0.5, 0.4);
                    m.alpha_mode = AlphaMode::Blend;
                }
            }
        }
        g.t += dt;
        for (i, h) in g.mats.iter().enumerate() {
            let p = i as f32 * 1.7;
            let alpha = 0.3 + 0.13 * (g.t * 3.6 + p).sin() + 0.05 * (g.t * 11.0 + p * 2.3).sin();
            let glow = 0.5 + 0.45 * ((g.t * 2.2 + p * 0.6).sin() * 0.5 + 0.5);
            if let Some(mut m) = materials.get_mut(h) {
                m.base_color.set_alpha(alpha.clamp(0.12, 0.6));
                m.emissive = LinearRgba::rgb(0.25, 0.6, 1.6) * glow;
            }
        }
        g.wisp -= dt;
        while g.wisp <= 0.0 {
            g.wisp += 0.07;
            let at = tf.translation + Vec3::new(rnd() * 0.6 - 0.3, rnd() * 1.8, rnd() * 0.6 - 0.3);
            bursts.write(Burst::new(at, BurstKind::Wisp, 1));
        }
    }
}

/// Foes that fall: drops, a toast, XP, and a last puff (ghosts burst into wisps).
#[allow(clippy::too_many_arguments)]
fn fall(
    mut commands: Commands,
    book: Res<FoeBook>,
    items: Res<Items>,
    q: Query<(Entity, &Transform, &Foe)>,
    mut dice: ResMut<FoeDice>,
    mut spill: MessageWriter<crate::inventory::Spill>,
    mut toast: MessageWriter<crate::survival::Toast>,
    (mut gain, prog): (MessageWriter<crate::progress::GainXp>, Res<crate::progress::xp::Progression>),
    mut bursts: MessageWriter<Burst>,
    mut slain: MessageWriter<FoeSlain>,
) {
    for (e, tf, foe) in &q {
        if foe.health > 0.0 {
            continue;
        }
        let k = &book.kinds[foe.kind];
        let at = tf.translation + Vec3::Y;
        slain.write(FoeSlain { tag: foe.tag, at });
        for (name, n, p) in &k.drops {
            if let Some(item) = items.id(name)
                && (*p >= 1.0 || dice.0.unit() < *p)
            {
                spill.write(crate::inventory::Spill { stack: Stack::new(item, *n), at });
            }
        }
        bursts.write(Burst::new(at, if k.ghost { BurstKind::Wisp } else { BurstKind::Smoke }, if k.ghost { 40 } else { 14 }));
        toast.write(crate::survival::Toast(format!("{} slain", k.title)));
        gain.write(crate::progress::GainXp { xp: k.health * foe.boost * prog.xp.kill, why: String::new() });
        commands.entity(e).despawn();
    }
}

/// Bolts loosed this frame become glowing darts.
fn shoot(mut commands: Commands, mut shots: MessageReader<Shoot>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    for s in shots.read() {
        let (r, g, b) = s.color;
        let mat = materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), emissive: LinearRgba::rgb(r * 3.0, g * 3.0, b * 3.0), ..default() });
        let mesh = meshes.add(Cuboid::new(0.12, 0.12, 0.45));
        commands.spawn((
            Bolt { vel: s.vel, damage: s.damage, cause: s.cause, life: 4.0 },
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(s.from).looking_to(s.vel.normalize_or(Vec3::Z), Vec3::Y),
            bevy::light::NotShadowCaster,
        ));
    }
}

/// Bolts fly straight; they stop in walls, on your raised blade, or in you.
#[allow(clippy::too_many_arguments)]
fn fly_bolts(
    mut commands: Commands,
    time: Res<Time>,
    (world, blocks): (Res<World>, Res<Blocks>),
    player: Res<Player>,
    cam: Single<&Transform, (With<FlyCamera>, Without<Bolt>)>,
    guard: Res<Guard>,
    mut bolts: Query<(Entity, &mut Transform, &mut Bolt)>,
    mut hits: MessageWriter<Damage>,
    mut sound: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
) {
    let dt = time.delta_secs().min(0.05);
    for (e, mut tf, mut b) in &mut bolts {
        b.life -= dt;
        let next = tf.translation + b.vel * dt;
        let body = player.pos + Vec3::Y * 0.9;
        let d = next - body;
        if Vec2::new(d.x, d.z).length() < 0.45 && d.y.abs() < 1.0 {
            let ahead = cam.forward().as_vec3();
            if guard.up && ahead_of(ahead, -b.vel, ai::GUARD_ARC) {
                sound.write(PlaySound::new("clash").speed(1.3));
                bursts.write(Burst::new(next, BurstKind::Sparks, 8));
            } else {
                hits.write(Damage { amount: b.damage, cause: b.cause.into() });
                sound.write(PlaySound::new("hit"));
            }
            commands.entity(e).despawn();
            continue;
        }
        if b.life <= 0.0 || solid(&world, &blocks, BlockPos::from_world(next.to_array())) {
            bursts.write(Burst::new(tf.translation, BurstKind::Smoke, 6));
            commands.entity(e).despawn();
            continue;
        }
        tf.translation = next;
    }
}
