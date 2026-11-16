//! NPCs: people with jointed bodies, faces that look around and talk, and
//! dialogue trees.
//!
//! - `data`: kinds from `assets/data/npcs.ron` (looks, voice, tree).
//! - `dialogue`: pure trees from `assets/data/dialogue.ron`.
//! - `motion`: pure springs, walk cycle, idle stance, gestures and gaze.
//! - `voice`: pure speech timing (mouth, talking sound and subtitles in step).
//! - `body`: the jointed model and its animation.
//! - `talk`: conversations (black bars, paused game time, camera, replies).
//!
//! This file: the `Npc` state (gameplay only, nothing about rendering, so a
//! server could run it later), walking with orders (follow you, stay, wander),
//! and Ctrl+Shift+N (Cmd+Shift+N on a Mac) to spawn the test NPC in front of you.
//!
//! Combat later: give `Order` a fighting order and `Npc` health, and let
//! `interact::Strike` reach NPCs the way it reaches creatures.

pub mod body;
pub mod data;
pub mod dialogue;
pub mod motion;
pub mod talk;
pub mod voice;

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::creatures::brain::Rng;
use crate::player::Player;
use crate::world::World;
use dialogue::{Mood, Tree};

pub struct NpcPlugin;

impl Plugin for NpcPlugin {
    fn build(&self, app: &mut App) {
        let kinds = data::load(include_str!("../../../../assets/data/npcs.ron")).unwrap_or_else(|e| panic!("npcs.ron: {e}"));
        let trees = dialogue::load(include_str!("../../../../assets/data/dialogue.ron")).unwrap_or_else(|e| panic!("dialogue.ron: {e}"));
        app.insert_resource(NpcBook { kinds, trees })
            .insert_resource(NpcDice(Rng(0x0DD_BA11_5EED)))
            .add_systems(
                crate::menu::GameUpdate,
                (spawn_key, think, walk).chain().in_set(NpcSet).after(crate::player::PlayerSet),
            )
            .add_plugins((body::BodyPlugin, talk::TalkPlugin));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NpcSet;

/// Every kind and dialogue tree.
#[derive(Resource)]
pub struct NpcBook {
    pub kinds: Vec<data::Kind>,
    pub trees: HashMap<String, Tree>,
}

#[derive(Resource)]
struct NpcDice(Rng);

/// What an NPC is doing when nobody is talking to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    /// Stand about where it is.
    Stay,
    /// Keep a few blocks from you.
    Follow,
    /// Stroll around a spot.
    Wander { home: Vec3 },
}

/// Body width (half) and height, blocks.
pub const HALF: f32 = 0.25;
pub const HEIGHT: f32 = 1.82;
/// Eye height above the feet.
pub const EYES: f32 = 1.63;
const GRAVITY: f32 = 25.0;
const HOP: f32 = 7.6;

#[derive(Component, Debug)]
pub struct Npc {
    pub kind: usize,
    pub vel: Vec3,
    /// Where the body faces (the model turns towards it on a spring).
    pub yaw: f32,
    pub on_ground: bool,
    pub order: Order,
    /// Wanted horizontal velocity.
    pub want: Vec3,
    goal: Option<Vec3>,
    /// Seconds until it picks a new goal (wandering).
    rest: f32,
    /// Following: walking to catch up (stops near you, starts again further out).
    catching_up: bool,
    /// Dialogue flags and replies already picked; kept between talks.
    pub flags: HashSet<String>,
    pub asked: HashSet<(String, usize)>,
    /// Set by `talk` while speaking: mouth (open, wide), how much it is
    /// putting into it (gestures), and the mood of what it says.
    pub mouth: (f32, f32),
    pub speaking: f32,
    pub mood: Mood,
    /// In a conversation: stands still and faces you.
    pub talking: bool,
}

impl Npc {
    pub fn new(kind: usize, yaw: f32) -> Self {
        Self {
            kind,
            vel: Vec3::ZERO,
            yaw,
            on_ground: false,
            order: Order::Stay,
            want: Vec3::ZERO,
            goal: None,
            rest: 0.0,
            catching_up: false,
            flags: HashSet::new(),
            asked: HashSet::new(),
            mouth: (0.0, 0.8),
            speaking: 0.0,
            mood: Mood::Neutral,
            talking: false,
        }
    }
}

/// The yaw that faces along `d` (models face -Z).
pub fn yaw_of(d: Vec3) -> f32 {
    (-d.x).atan2(-d.z)
}

fn solid(world: &World, blocks: &Blocks, p: BlockPos) -> bool {
    world.block(p).is_some_and(|id| blocks.registry.get(id).solid)
}

/// Feet height on the ground near `at`: the first spot from a few blocks up,
/// going down, with solid ground under two free blocks.
fn ground_near(world: &World, blocks: &Blocks, at: Vec3) -> Option<Vec3> {
    let (x, z) = (at.x.floor() as i32, at.z.floor() as i32);
    let top = at.y.floor() as i32 + 4;
    (top - 12..=top).rev().find_map(|y| {
        let free = |dy| !solid(world, blocks, BlockPos::new(x, y + dy, z)) && world.block(BlockPos::new(x, y + dy, z)).is_some();
        (solid(world, blocks, BlockPos::new(x, y - 1, z)) && free(0) && free(1)).then(|| Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5))
    })
}

/// Ctrl+Shift+N (or Cmd+Shift+N) spawns the test NPC (the first kind) a few
/// blocks in front of you, facing you, following you.
#[allow(clippy::too_many_arguments)]
fn spawn_key(
    keys: Res<ButtonInput<KeyCode>>,
    book: Res<NpcBook>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<Blocks>,
    cam: Single<&Transform, With<FlyCamera>>,
    talk: Res<talk::Talk>,
    mut spawn: MessageWriter<body::SpawnNpc>,
    mut auto_done: Local<bool>,
) {
    let held = |a, b| keys.pressed(a) || keys.pressed(b);
    let combo = held(KeyCode::ControlLeft, KeyCode::ControlRight) || held(KeyCode::SuperLeft, KeyCode::SuperRight);
    let pressed = combo && held(KeyCode::ShiftLeft, KeyCode::ShiftRight) && keys.just_pressed(KeyCode::KeyN);
    // `BLOCKGAME_NPC=3` spawns one that many blocks ahead once the world is in (screenshots).
    let auto = if *auto_done || !player.ready { None } else { std::env::var("BLOCKGAME_NPC").ok() };
    if (!pressed && auto.is_none()) || talk.active() || book.kinds.is_empty() {
        return;
    }
    let ahead = auto.as_deref().and_then(|a| a.parse::<f32>().ok()).unwrap_or(3.0);
    let mut fwd = cam.forward().as_vec3();
    fwd.y = 0.0;
    let fwd = fwd.normalize_or(Vec3::NEG_Z);
    let at = player.pos + fwd * ahead;
    let pos = match ground_near(&world, &blocks, at) {
        Some(p) => p,
        // Screenshots wait for the ground to load; a key press spawns anyway.
        None if auto.is_some() => return,
        None => at,
    };
    *auto_done = true;
    let mut npc = Npc::new(0, yaw_of(-fwd));
    npc.order = Order::Follow;
    npc.flags.insert(dialogue::FOLLOWING.to_string());
    spawn.write(body::SpawnNpc { npc, at: pos });
}

/// Orders become a wanted velocity and facing.
fn think(time: Res<Time>, player: Res<Player>, book: Res<NpcBook>, mut dice: ResMut<NpcDice>, mut q: Query<(&mut Transform, &mut Npc)>) {
    let dt = time.delta_secs().min(0.05);
    if dt <= 0.0 {
        return;
    }
    for (mut tf, mut npc) in &mut q {
        let walk = book.kinds.get(npc.kind).map_or(2.4, |k| k.walk);
        let pos = tf.translation;
        let mut to_player = player.pos - pos;
        to_player.y = 0.0;
        let dist = to_player.length();
        npc.want = Vec3::ZERO;
        if npc.talking {
            npc.yaw = yaw_of(to_player);
            continue;
        }
        match npc.order {
            Order::Stay => {}
            Order::Follow => {
                // Fell far behind (or you flew off): catch up by appearing behind you.
                if dist > 40.0 {
                    tf.translation = player.pos - to_player.normalize_or_zero() * 2.0;
                    npc.vel = Vec3::ZERO;
                    continue;
                }
                if dist > 4.2 {
                    npc.catching_up = true;
                } else if dist < 2.6 {
                    npc.catching_up = false;
                }
                if npc.catching_up {
                    // Jog when far, slowing as it nears you.
                    let speed = if dist > 8.0 { walk * 1.9 } else { walk * (0.7 + 0.3 * ((dist - 2.6) / 2.0).clamp(0.0, 1.0)) };
                    npc.want = to_player / dist * speed;
                }
            }
            Order::Wander { home } => {
                npc.rest -= dt;
                match npc.goal {
                    Some(g) if Vec2::new(g.x - pos.x, g.z - pos.z).length() > 0.6 && npc.rest > -12.0 => {
                        let d = Vec3::new(g.x - pos.x, 0.0, g.z - pos.z);
                        npc.want = d.normalize_or_zero() * walk * 0.75;
                    }
                    Some(_) => {
                        npc.goal = None;
                        npc.rest = dice.0.range(2.0, 6.0);
                    }
                    None if npc.rest <= 0.0 => {
                        let a = dice.0.range(0.0, std::f32::consts::TAU);
                        let r = dice.0.range(2.0, 7.0);
                        npc.goal = Some(home + Vec3::new(a.cos() * r, 0.0, a.sin() * r));
                        npc.rest = 0.0;
                    }
                    None => {}
                }
            }
        }
        if npc.want.length_squared() > 0.01 {
            npc.yaw = yaw_of(npc.want);
        } else if dist < 7.0 {
            // Standing near you: turn the body only when the head can't reach.
            let off = motion::wrap(yaw_of(to_player) - npc.yaw);
            if off.abs() > motion::HEAD_YAW * 0.9 {
                npc.yaw = yaw_of(to_player);
            }
        }
    }
}

/// Walking physics: eases towards the wanted velocity (no sudden starts or
/// stops), gravity, hops up one-block steps.
fn walk(time: Res<Time>, world: Res<World>, blocks: Res<Blocks>, mut q: Query<(&mut Transform, &mut Npc)>) {
    let dt = time.delta_secs().min(0.05);
    if dt <= 0.0 {
        return;
    }
    for (mut tf, mut npc) in &mut q {
        if world.block(BlockPos::from_world(tf.translation.to_array())).is_none() {
            continue;
        }
        let blend = if npc.on_ground { (6.0 * dt).min(1.0) } else { (1.5 * dt).min(1.0) };
        let want = npc.want;
        npc.vel.x += (want.x - npc.vel.x) * blend;
        npc.vel.z += (want.z - npc.vel.z) * blend;
        npc.vel.y = (npc.vel.y - GRAVITY * dt).max(-40.0);
        let mut pos = tf.translation;
        let mut bumped = false;
        for axis in [0usize, 2] {
            let mut next = pos;
            next[axis] += npc.vel[axis] * dt;
            if crate::creatures::blocked(&world, &blocks, next, HALF, HEIGHT) {
                bumped = true;
                npc.vel[axis] = 0.0;
            } else {
                pos = next;
            }
        }
        let mut next = pos;
        next.y += npc.vel.y * dt;
        npc.on_ground = false;
        if crate::creatures::blocked(&world, &blocks, next, HALF, HEIGHT) {
            if npc.vel.y < 0.0 {
                let snapped = Vec3::new(next.x, next.y.ceil(), next.z);
                if !crate::creatures::blocked(&world, &blocks, snapped, HALF, HEIGHT) {
                    pos = snapped;
                }
                npc.on_ground = true;
            }
            npc.vel.y = 0.0;
        } else {
            pos = next;
        }
        if bumped && npc.on_ground && want.length_squared() > 0.01 {
            npc.vel.y = HOP;
        }
        tf.translation = pos;
    }
}
