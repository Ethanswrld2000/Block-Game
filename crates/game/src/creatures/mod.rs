//! Creatures: swamp animals, night hunters, barrow wights, ancestral spirits
//! and villagers. Kinds come from `assets/data/creatures.ron`; the rules that
//! need no Bevy live in `data` and `brain`.
//!
//! Creatures spawn in a ring around you (animals by day, hunters and spirits
//! at night, wights when you come near a barrow) and despawn when you leave.
//! Left click hits the creature under the crosshair with what you hold
//! (items have a `damage`); its drops fall to the ground.
//!
//! Spirits drift through walls at night and touch you with cold (Fatigue).
//! They will not come within a few blocks of a torch, placed or held, and
//! fade away at dawn.

pub mod brain;
pub mod data;
mod villagers;

use std::collections::HashMap;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::clock::WorldClock;
use crate::interact::{Aimed, InteractSet, Strike, Wielded};
use crate::inventory::grid::Stack;
use crate::items::{ItemKind, Items};
use crate::player::{Damage, MoveMode, Player};
use crate::survival::Survival;
use crate::torches::Torches;
use crate::world::{Generator, World};
use brain::Rng;
use data::{Kinds, Temper};

pub use villagers::Villager;

pub struct CreaturesPlugin;

impl Plugin for CreaturesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Dice(Rng(0x5EED_C0FF_EE15_u64)))
            .init_resource::<Barrows>()
            .add_systems(PostStartup, setup)
            .add_systems(Update, aim.before(InteractSet).after(crate::player::PlayerSet))
            .add_systems(
                Update,
                (take_strikes, spawn_around, spawn_wights, think, spirits, walk, tint, despawn_far)
                    .chain()
                    .in_set(CreatureSet)
                    .after(InteractSet),
            )
            .add_systems(Update, look_at_creature.run_if(|| std::env::var_os("BLOCKGAME_LOOK").is_some()))
            .add_plugins(villagers::VillagersPlugin);
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CreatureSet;

/// Gravity on creatures, blocks/s².
const GRAVITY: f32 = 25.0;
/// Upward speed of a hop over a one-block step.
const HOP: f32 = 7.6;
/// Creatures further than this from you are removed (villagers excepted).
const DESPAWN: f32 = 112.0;
/// How many of each sort may be around you at once.
const MAX_ANIMALS: usize = 8;
const MAX_HUNTERS: usize = 4;
const MAX_SPIRITS: usize = 4;

#[derive(Resource)]
pub struct Dice(pub Rng);

#[derive(Component, Debug)]
pub struct Creature {
    pub kind: usize,
    pub health: f32,
    pub vel: Vec3,
    pub yaw: f32,
    pub on_ground: bool,
    /// Wanted horizontal velocity this frame (set by `think`).
    pub want: Vec3,
    /// Where it is wandering to.
    pub goal: Option<Vec3>,
    /// Seconds until it picks a new goal.
    pub think: f32,
    /// Seconds of red tint after a hit.
    pub hurt: f32,
    /// Seconds left fighting back (neutral creatures).
    pub angry: f32,
    /// Seconds left running away.
    pub scared: f32,
    /// Seconds until it can strike again.
    pub cooldown: f32,
    /// 0..1, how present it is (spirits fade in and out).
    pub fade: f32,
    /// Fading away (spirits at dawn).
    pub leaving: bool,
    /// Stay within this distance of a point (wights in their barrow, villagers in their village).
    pub leash: Option<(Vec3, f32)>,
}

impl Creature {
    pub fn new(kind: usize, health: f32, yaw: f32) -> Self {
        Self {
            kind,
            health,
            vel: Vec3::ZERO,
            yaw,
            on_ground: false,
            want: Vec3::ZERO,
            goal: None,
            think: 0.0,
            hurt: 0.0,
            angry: 0.0,
            scared: 0.0,
            cooldown: 1.0,
            fade: 0.0,
            leaving: false,
            leash: None,
        }
    }
}

/// One box of a creature's model, with its usual material.
#[derive(Component)]
struct Part {
    normal: Handle<StandardMaterial>,
}

type ModelPart = (Handle<Mesh>, Handle<StandardMaterial>, Vec3);

/// Meshes and materials for each kind's model.
#[derive(Resource)]
pub struct CreatureAssets {
    parts: Vec<Vec<ModelPart>>,
    hurt: Handle<StandardMaterial>,
}

/// When each barrow last woke its wights, in days.
#[derive(Resource, Default)]
struct Barrows {
    woke: HashMap<u64, f64>,
}

fn setup(mut commands: Commands, items: Res<Items>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let kinds = Kinds::from_ron(include_str!("../../../../assets/data/creatures.ron"), &items).unwrap_or_else(|e| panic!("{e}"));
    let parts = kinds
        .list
        .iter()
        .map(|k| {
            k.model
                .iter()
                .map(|b| {
                    let color = Color::srgb(b.color.0, b.color.1, b.color.2);
                    let mat = match k.glow {
                        Some(g) => StandardMaterial {
                            base_color: color.with_alpha(0.55),
                            emissive: LinearRgba::from(Color::srgb(g.0, g.1, g.2)) * 2.0,
                            alpha_mode: AlphaMode::Blend,
                            unlit: true,
                            ..default()
                        },
                        None => StandardMaterial { base_color: color, perceptual_roughness: 0.95, ..default() },
                    };
                    (meshes.add(Cuboid::new(b.size.0, b.size.1, b.size.2)), materials.add(mat), Vec3::new(b.offset.0, b.offset.1, b.offset.2))
                })
                .collect()
        })
        .collect();
    let hurt = materials.add(StandardMaterial { base_color: Color::srgb(0.85, 0.15, 0.1), emissive: LinearRgba::rgb(0.4, 0.0, 0.0), ..default() });
    commands.insert_resource(CreatureAssets { parts, hurt });
    commands.insert_resource(kinds);
}

/// Spawn a creature standing at `pos` (feet), facing `yaw`.
pub fn spawn_creature(commands: &mut Commands, kinds: &Kinds, assets: &CreatureAssets, kind: usize, pos: Vec3, yaw: f32) -> Entity {
    let k = kinds.get(kind);
    let mut c = Creature::new(kind, k.health, yaw);
    if k.temper != Temper::Spirit {
        c.fade = 1.0;
    }
    let scale = if k.temper == Temper::Spirit { 0.01 } else { 1.0 };
    let mut e = commands.spawn((
        c,
        Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(scale)),
        Visibility::default(),
    ));
    e.with_children(|p| {
        for (mesh, mat, offset) in &assets.parts[kind] {
            p.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::from_translation(*offset), Part { normal: mat.clone() }, NotShadowCaster));
        }
        if let Some(g) = k.glow {
            p.spawn((
                PointLight { color: Color::srgb(g.0, g.1, g.2), intensity: 40_000.0, range: 8.0, shadow_maps_enabled: false, ..default() },
                Transform::from_xyz(0.0, k.height * 0.6, 0.0),
            ));
        }
    });
    e.id()
}

/// Which creature is under the crosshair, within reach.
fn aim(cam: Single<&Transform, With<FlyCamera>>, kinds: Option<Res<Kinds>>, mode: Res<MoveMode>, q: Query<(Entity, &Transform, &Creature)>, mut aimed: ResMut<Aimed>) {
    let Some(kinds) = kinds else { return };
    let origin = cam.translation;
    let dir = cam.forward().as_vec3();
    let reach = if *mode == MoveMode::Fly { crate::interact::FLY_REACH } else { crate::interact::REACH };
    let mut best: Option<(Entity, f32)> = None;
    for (e, tf, c) in &q {
        if c.fade < 0.5 {
            continue;
        }
        let k = kinds.get(c.kind);
        let half = Vec3::new(k.half, 0.0, k.half);
        let min = tf.translation - half;
        let max = tf.translation + half + Vec3::Y * k.height;
        if let Some(t) = crate::interact::ray_box(origin, dir, min, max)
            && t <= reach
            && best.is_none_or(|(_, b)| t < b)
        {
            best = Some((e, t));
        }
    }
    *aimed = Aimed { entity: best.map(|b| b.0), dist: best.map_or(f32::MAX, |b| b.1) };
}

/// Blows land: knockback, anger or fear, and death with drops.
#[allow(clippy::too_many_arguments)]
fn take_strikes(
    mut commands: Commands,
    mut strikes: MessageReader<Strike>,
    kinds: Option<Res<Kinds>>,
    mut q: Query<(&Transform, &mut Creature, Option<&Villager>)>,
    mut spill: MessageWriter<crate::inventory::Spill>,
    mut toast: MessageWriter<crate::survival::Toast>,
    mut dice: ResMut<Dice>,
    (mut gain, prog): (MessageWriter<crate::progress::GainXp>, Res<crate::progress::xp::Progression>),
) {
    let Some(kinds) = kinds else { return };
    for s in strikes.read() {
        let Ok((tf, mut c, villager)) = q.get_mut(s.entity) else { continue };
        if c.health <= 0.0 {
            continue;
        }
        let k = kinds.get(c.kind);
        c.health -= s.damage;
        c.hurt = 0.3;
        c.vel += s.dir * 6.0 + Vec3::Y * if k.temper == Temper::Spirit { 0.0 } else { 4.5 };
        c.on_ground = false;
        match k.temper {
            Temper::Neutral | Temper::Hostile | Temper::Spirit => c.angry = 20.0,
            Temper::Skittish | Temper::Villager => c.scared = 8.0,
        }
        if c.health <= 0.0 {
            let at = tf.translation + Vec3::Y * (k.height * 0.5);
            for (item, n, p) in &k.drops {
                if *p >= 1.0 || dice.0.unit() < *p {
                    spill.write(crate::inventory::Spill { stack: Stack::new(*item, *n), at });
                }
            }
            let msg = match villager {
                Some(v) => format!("{} of {} is dead", v.name, v.village_name),
                None => format!("{} slain", k.title),
            };
            toast.write(crate::survival::Toast(msg));
            if villager.is_none() {
                gain.write(crate::progress::GainXp { xp: k.health * prog.xp.kill, why: String::new() });
            }
            commands.entity(s.entity).despawn();
        }
    }
}

/// Feet height of open ground in a column near `y`: a solid block with two
/// open, dry blocks above it. `None` if there is none or it isn't loaded.
pub fn ground_near(world: &World, blocks: &Blocks, x: i32, z: i32, from: i32, to: i32, dry: bool) -> Option<i32> {
    let water = blocks.registry.id("water");
    let open = |y: i32| world.block(BlockPos::new(x, y, z)).is_some_and(|b| !blocks.registry.get(b).solid && (!dry || Some(b) != water));
    (to..=from).rev().find(|&y| {
        world.block(BlockPos::new(x, y, z)).is_some_and(|b| blocks.registry.get(b).solid) && open(y + 1) && open(y + 2)
    }).map(|y| y + 1)
}

/// Light sources a spirit keeps away from: placed torches, a torch in your hand, the Light spell.
fn lights(torches: &Torches, player: &Player, wielded: &Wielded, items: &Items, spell: bool) -> Vec<Vec3> {
    let mut l: Vec<Vec3> = torches.positions().map(|p| Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5)).collect();
    if spell || wielded.0.is_some_and(|i| matches!(items.get(i).kind, ItemKind::Torch)) {
        l.push(player.pos + Vec3::Y);
    }
    l
}

/// Keep the numbers of creatures around you topped up.
#[allow(clippy::too_many_arguments)]
fn spawn_around(
    mut commands: Commands,
    time: Res<Time>,
    kinds: Option<Res<Kinds>>,
    assets: Option<Res<CreatureAssets>>,
    generator: Option<Res<Generator>>,
    world: Res<World>,
    blocks: Res<Blocks>,
    clock: Res<WorldClock>,
    (player, mode): (Res<Player>, Res<MoveMode>),
    torches: Res<Torches>,
    (wielded, spells): (Res<Wielded>, Res<crate::progress::Spells>),
    items: Res<Items>,
    q: Query<&Creature>,
    mut dice: ResMut<Dice>,
    mut timer: Local<f32>,
) {
    let (Some(kinds), Some(assets), Some(generator)) = (kinds, assets, generator) else { return };
    *timer -= time.delta_secs();
    if *timer > 0.0 || !(player.ready || *mode == MoveMode::Fly) {
        return;
    }
    *timer = 1.5;
    let hour = clock.hour();
    let night = brain::spirit_hours(hour);
    let mut counts = [0usize; 3];
    for c in &q {
        match kinds.get(c.kind).temper {
            Temper::Skittish | Temper::Neutral => counts[0] += 1,
            Temper::Hostile => counts[1] += 1,
            Temper::Spirit => counts[2] += 1,
            Temper::Villager => {}
        }
    }
    let lights = lights(&torches, &player, &wielded, &items, spells.light);
    let rng = &mut dice.0;
    // A few tries per tick; many land on water or unloaded ground.
    for _ in 0..4 {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let dist = rng.range(22.0, 60.0);
        let x = (player.pos.x + angle.cos() * dist).floor() as i32;
        let z = (player.pos.z + angle.sin() * dist).floor() as i32;
        let py = player.pos.y.floor() as i32;
        let Some(y) = ground_near(&world, &blocks, x, z, py + 24, py - 32, true) else { continue };
        // Not in the middle of a village.
        if !generator.0.villages_in(x, z, x, z).is_empty() {
            continue;
        }
        let biome = &generator.0.biome_at(x, y, z).name;
        let full = |t: Temper| match t {
            Temper::Skittish | Temper::Neutral => counts[0] >= MAX_ANIMALS,
            Temper::Hostile => counts[1] >= MAX_HUNTERS,
            Temper::Spirit => counts[2] >= MAX_SPIRITS,
            Temper::Villager => true,
        };
        let list: Vec<(usize, f32)> = kinds.candidates(biome, night).into_iter().filter(|(i, _)| !full(kinds.get(*i).temper)).collect();
        let Some(kind) = data::pick(&list, rng.unit()) else { continue };
        let k = kinds.get(kind);
        let at = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
        if k.temper == Temper::Spirit && brain::flee_light(at, lights.iter().copied()).is_some() {
            continue;
        }
        let (lo, hi) = k.spawn.as_ref().map_or((1, 1), |s| s.group);
        let n = lo + (rng.next() % (hi.saturating_sub(lo) as u64 + 1)) as u8;
        for i in 0..n {
            let (dx, dz) = (rng.range(-3.0, 3.0) * (i > 0) as u8 as f32, rng.range(-3.0, 3.0) * (i > 0) as u8 as f32);
            let (gx, gz) = ((at.x + dx).floor() as i32, (at.z + dz).floor() as i32);
            let Some(gy) = ground_near(&world, &blocks, gx, gz, y + 3, y - 3, true) else { continue };
            let pos = Vec3::new(gx as f32 + 0.5, gy as f32 + if k.temper == Temper::Spirit { 0.6 } else { 0.0 }, gz as f32 + 0.5);
            spawn_creature(&mut commands, &kinds, &assets, kind, pos, rng.range(0.0, std::f32::consts::TAU));
        }
        match k.temper {
            Temper::Skittish | Temper::Neutral => counts[0] += n as usize,
            Temper::Hostile => counts[1] += n as usize,
            Temper::Spirit => counts[2] += n as usize,
            Temper::Villager => {}
        }
    }
}

/// Wights rise in a barrow's hall when you come near, at most once a day.
#[allow(clippy::too_many_arguments)]
fn spawn_wights(
    mut commands: Commands,
    kinds: Option<Res<Kinds>>,
    assets: Option<Res<CreatureAssets>>,
    generator: Option<Res<Generator>>,
    world: Res<World>,
    blocks: Res<Blocks>,
    clock: Res<WorldClock>,
    player: Res<Player>,
    mut barrows: ResMut<Barrows>,
    mut dice: ResMut<Dice>,
) {
    let (Some(kinds), Some(assets), Some(generator)) = (kinds, assets, generator) else { return };
    let Some(wight) = kinds.id("barrow_wight") else { return };
    let (px, pz) = (player.pos.x as i32, player.pos.z as i32);
    for b in generator.0.barrows_in(px - 40, pz - 40, px + 40, pz + 40) {
        let hall = Vec3::new(b.hall[0] as f32 + 0.5, b.hall[1] as f32, b.hall[2] as f32 + 0.5);
        if hall.distance(player.pos) > 40.0 || barrows.woke.get(&b.id).is_some_and(|t| clock.total_days() - t < 1.0) {
            continue;
        }
        let (lo, hi) = kinds.get(wight).spawn.as_ref().map_or((3, 3), |s| s.group);
        let mut spawned = 0;
        for _ in 0..hi.max(lo) {
            let (x, z) = (b.hall[0] + dice.0.range(-4.0, 4.0) as i32, b.hall[2] + dice.0.range(-4.0, 4.0) as i32);
            if let Some(y) = ground_near(&world, &blocks, x, z, b.hall[1] + 3, b.hall[1] - 3, true) {
                let e = spawn_creature(&mut commands, &kinds, &assets, wight, Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5), dice.0.range(0.0, std::f32::consts::TAU));
                let mut c = Creature::new(wight, kinds.get(wight).health, 0.0);
                c.fade = 1.0;
                c.leash = Some((hall, 10.0));
                commands.entity(e).insert(c);
                spawned += 1;
            }
        }
        // Wait for the hall to load before marking it.
        if spawned > 0 {
            barrows.woke.insert(b.id, clock.total_days());
        }
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Wander, flee, chase and strike (everything but spirits and villagers'
/// schedules, which have their own systems).
#[allow(clippy::too_many_arguments)]
fn think(
    time: Res<Time>,
    kinds: Option<Res<Kinds>>,
    player: Res<Player>,
    mode: Res<MoveMode>,
    s: Res<Survival>,
    mut hits: MessageWriter<Damage>,
    mut q: Query<(&Transform, &mut Creature), Without<Villager>>,
    mut dice: ResMut<Dice>,
) {
    let Some(kinds) = kinds else { return };
    let dt = time.delta_secs();
    // Nobody hunts a flying or dead player.
    let target = (*mode == MoveMode::Walk && s.dead_for.is_none() && player.ready).then_some(player.pos);
    for (tf, mut c) in &mut q {
        let k = kinds.get(c.kind);
        c.hurt = (c.hurt - dt).max(0.0);
        c.angry = (c.angry - dt).max(0.0);
        c.scared = (c.scared - dt).max(0.0);
        c.cooldown = (c.cooldown - dt).max(0.0);
        c.think -= dt;
        if k.temper == Temper::Spirit {
            continue;
        }
        let pos = tf.translation;
        let to = target.map(|t| t - pos);
        let dist = to.map_or(f32::MAX, |v| flat(v).length());
        let leashed = |p: Vec3, c: &Creature| c.leash.is_none_or(|(at, r)| flat(p - at).length() < r);
        let hunting = match k.temper {
            Temper::Hostile => dist < 18.0,
            Temper::Neutral => c.angry > 0.0 && dist < 30.0,
            _ => false,
        } && to.is_some_and(|v| v.y.abs() < 6.0);
        let fleeing = c.scared > 0.0 || (k.temper == Temper::Skittish && (dist < 4.0 || (dist < 11.0 && !player.crouching)));
        if fleeing && let Some(to) = to {
            c.want = -flat(to).normalize_or_zero() * k.run;
            c.goal = None;
        } else if hunting && let Some(to) = to {
            let reach = k.attack.map_or(1.5, |a| a.reach);
            c.want = if dist > reach * 0.7 && leashed(pos + flat(to).normalize_or_zero(), &c) {
                flat(to).normalize_or_zero() * k.run
            } else {
                Vec3::ZERO
            };
            c.yaw = (-to.x).atan2(-to.z);
            if let Some(a) = k.attack
                && dist < a.reach + k.half
                && c.cooldown <= 0.0
            {
                hits.write(Damage { amount: a.damage, cause: a.cause.into() });
                c.cooldown = a.every;
            }
        } else {
            wander(&mut c, pos, k.speed, &mut dice.0);
        }
    }
}

/// Amble to random nearby spots, with pauses.
pub fn wander(c: &mut Creature, pos: Vec3, speed: f32, rng: &mut Rng) {
    if c.think <= 0.0 {
        c.think = rng.range(3.0, 8.0);
        c.goal = (rng.unit() < 0.6).then(|| {
            let (center, r) = c.leash.unwrap_or((pos, 9.0));
            let a = rng.range(0.0, std::f32::consts::TAU);
            center + Vec3::new(a.cos(), 0.0, a.sin()) * rng.range(2.0, r)
        });
    }
    walk_to(c, pos, speed);
}

/// Head for the goal, if any.
pub fn walk_to(c: &mut Creature, pos: Vec3, speed: f32) {
    c.want = match c.goal {
        Some(g) if flat(g - pos).length() > 0.8 => flat(g - pos).normalize() * speed,
        _ => {
            c.goal = None;
            Vec3::ZERO
        }
    };
}

/// Spirits: drift through everything towards you at night, shy from torches, fade at dawn.
#[allow(clippy::too_many_arguments)]
fn spirits(
    mut commands: Commands,
    time: Res<Time>,
    kinds: Option<Res<Kinds>>,
    clock: Res<WorldClock>,
    player: Res<Player>,
    mode: Res<MoveMode>,
    s: Res<Survival>,
    torches: Res<Torches>,
    (wielded, spells): (Res<Wielded>, Res<crate::progress::Spells>),
    items: Res<Items>,
    mut hits: MessageWriter<Damage>,
    mut q: Query<(Entity, &mut Transform, &mut Creature)>,
    mut dice: ResMut<Dice>,
) {
    let Some(kinds) = kinds else { return };
    let dt = time.delta_secs();
    let night = brain::spirit_hours(clock.hour());
    let lights = lights(&torches, &player, &wielded, &items, spells.light);
    let target = (*mode == MoveMode::Walk && s.dead_for.is_none() && player.ready).then_some(player.pos + Vec3::Y * 0.3);
    let t = time.elapsed_secs();
    for (e, mut tf, mut c) in &mut q {
        let k = kinds.get(c.kind);
        if k.temper != Temper::Spirit {
            continue;
        }
        if !night {
            c.leaving = true;
        }
        c.fade = if c.leaving { c.fade - dt / 3.0 } else { (c.fade + dt / 2.0).min(1.0) };
        if c.fade <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let pos = tf.translation;
        let mut want = if let Some(away) = brain::flee_light(pos, lights.iter().copied()) {
            away * k.run
        } else if let Some(goal) = target.filter(|g| g.distance(pos) < 48.0 && !c.leaving) {
            let to = goal - pos;
            let dist = to.length();
            if let Some(a) = k.attack
                && dist < a.reach + k.half
                && c.cooldown <= 0.0
            {
                hits.write(Damage { amount: a.damage, cause: a.cause.into() });
                c.cooldown = a.every;
            }
            // Close in, but stay an arm's length off rather than inside you.
            let pace = if dist < 10.0 { k.run } else { k.speed };
            let mut v = if flat(to).length() > 1.4 { flat(to).normalize() * pace } else { Vec3::ZERO };
            v.y = to.y.clamp(-1.0, 1.0) * k.speed;
            v
        } else {
            wander(&mut c, pos, k.speed, &mut dice.0);
            c.want
        };
        if c.leaving {
            want = Vec3::Y * 0.8;
        }
        // Knockback fades out; no gravity, no walls.
        c.vel *= (1.0 - 4.0 * dt).max(0.0);
        let bob = (t * 1.7 + e.index().index() as f32).sin() * 0.25;
        tf.translation += (want + c.vel) * dt + Vec3::Y * bob * dt;
        let face = flat(want);
        if face.length_squared() > 0.01 {
            c.yaw = (-face.x).atan2(-face.z);
        }
        tf.rotation = tf.rotation.slerp(Quat::from_rotation_y(c.yaw), (dt * 4.0).min(1.0));
        tf.scale = Vec3::splat(c.fade.max(0.01));
    }
}

/// Is a creature's box at `pos` inside anything solid? Unloaded counts as solid.
fn blocked(world: &World, blocks: &Blocks, pos: Vec3, half: f32, height: f32) -> bool {
    let min = pos - Vec3::new(half, 0.0, half);
    let max = pos + Vec3::new(half, height, half);
    crate::player::blocks_in(min, max).any(|b| world.block(b).is_none_or(|id| blocks.registry.get(id).solid))
}

/// Walking physics for everything but spirits: gravity, hops up steps, floating in water.
fn walk(time: Res<Time>, kinds: Option<Res<Kinds>>, world: Res<World>, blocks: Res<Blocks>, mut q: Query<(&mut Transform, &mut Creature, &Visibility)>) {
    let Some(kinds) = kinds else { return };
    let dt = time.delta_secs().min(0.05);
    let water = blocks.registry.id("water");
    for (mut tf, mut c, vis) in &mut q {
        let k = kinds.get(c.kind);
        if k.temper == Temper::Spirit || *vis == Visibility::Hidden {
            continue;
        }
        // Not loaded under it yet: wait.
        if world.block(BlockPos::from_world(tf.translation.to_array())).is_none() {
            continue;
        }
        let mid = tf.translation + Vec3::Y * (k.height * 0.4);
        let swimming = water.is_some() && world.block(BlockPos::from_world(mid.to_array())) == water;
        let speed = if swimming { 0.6 } else { 1.0 };
        // Steer towards the wanted velocity; knockback dies down.
        let blend = if c.on_ground || swimming { (10.0 * dt).min(1.0) } else { (1.5 * dt).min(1.0) };
        let target = c.want * speed;
        c.vel.x += (target.x - c.vel.x) * blend;
        c.vel.z += (target.z - c.vel.z) * blend;
        if swimming {
            c.vel.y += (1.6 - c.vel.y) * (3.0 * dt).min(1.0);
        } else {
            c.vel.y = (c.vel.y - GRAVITY * dt).max(-40.0);
        }
        let mut pos = tf.translation;
        let mut bumped = false;
        for axis in [0usize, 2] {
            let mut next = pos;
            next[axis] += c.vel[axis] * dt;
            if blocked(&world, &blocks, next, k.half, k.height) {
                bumped = true;
                c.vel[axis] = 0.0;
            } else {
                pos = next;
            }
        }
        let mut next = pos;
        next.y += c.vel.y * dt;
        c.on_ground = false;
        if blocked(&world, &blocks, next, k.half, k.height) {
            if c.vel.y < 0.0 {
                let snapped = Vec3::new(next.x, next.y.ceil(), next.z);
                if !blocked(&world, &blocks, snapped, k.half, k.height) {
                    pos = snapped;
                }
                c.on_ground = true;
            }
            c.vel.y = 0.0;
        } else {
            pos = next;
        }
        // Ran into a step: hop up it.
        if bumped && (c.on_ground || swimming) && c.want.length_squared() > 0.01 {
            c.vel.y = HOP;
        }
        tf.translation = pos;
        let face = flat(c.want);
        if face.length_squared() > 0.01 && c.angry <= 0.0 {
            c.yaw = (-face.x).atan2(-face.z);
        }
        tf.rotation = tf.rotation.slerp(Quat::from_rotation_y(c.yaw), (dt * 8.0).min(1.0));
    }
}

/// Hit creatures flash red.
fn tint(assets: Option<Res<CreatureAssets>>, creatures: Query<&Creature>, mut parts: Query<(&ChildOf, &Part, &mut MeshMaterial3d<StandardMaterial>)>) {
    let Some(assets) = assets else { return };
    for (parent, part, mut mat) in &mut parts {
        let Ok(c) = creatures.get(parent.parent()) else { continue };
        let want = if c.hurt > 0.0 { &assets.hurt } else { &part.normal };
        if mat.0 != *want {
            mat.0 = want.clone();
        }
    }
}

type Wild = (With<Creature>, Without<Villager>);

/// `BLOCKGAME_LOOK=<creature>`: keep the camera turned to the nearest one (for screenshots).
fn look_at_creature(
    kinds: Option<Res<Kinds>>,
    q: Query<(&Transform, &Creature), Without<FlyCamera>>,
    mut cam: Single<(&mut Transform, &mut FlyCamera)>,
    mut name: Local<Option<String>>,
) {
    let Some(kinds) = kinds else { return };
    let name = name.get_or_insert_with(|| std::env::var("BLOCKGAME_LOOK").unwrap_or_default());
    let Some(kind) = kinds.id(name) else { return };
    let eye = cam.0.translation;
    let nearest = q.iter().filter(|(_, c)| c.kind == kind).map(|(tf, _)| tf.translation + Vec3::Y * kinds.get(kind).height * 0.5).min_by(|a, b| a.distance(eye).total_cmp(&b.distance(eye)));
    if let Some(p) = nearest {
        let d = (p - eye).normalize_or(Vec3::NEG_Z);
        cam.1.yaw = (-d.x).atan2(-d.z);
        cam.1.pitch = d.y.asin();
        cam.0.rotation = Quat::from_euler(EulerRot::YXZ, cam.1.yaw, cam.1.pitch, 0.0);
    }
}

fn despawn_far(mut commands: Commands, player: Res<Player>, q: Query<(Entity, &Transform), Wild>) {
    for (e, tf) in &q {
        if flat(tf.translation - player.pos).length() > DESPAWN {
            commands.entity(e).despawn();
        }
    }
}
