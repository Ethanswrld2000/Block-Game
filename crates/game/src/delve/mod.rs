//! Dungeons in the game: one system for every dungeon on the island.
//!
//! A spirit (Brindlespheimer casts the form at the entrance, `dungeon::spirit`)
//! walks through a dungeon's door and a run starts on a floor laid out fresh
//! for it (`worldgen::delve`), built in the pocket far out in empty sky:
//! `world::PocketFloor` tells chunk generation which floor to build there, and
//! those chunks are never saved. Rules (pure) in `rules`.
//!
//! On a floor: foes appear room by room as you come near (`combat::SpawnFoe`,
//! tagged with their room and wave). Fight rooms, ambushes and the boss arena
//! lock you in (`locks`); the room furthest from the start drops the floor's
//! key when it's cleared, and the key opens the locked door to the boss. The
//! boss's fall opens the stair down, the way out (back to the entrance with
//! everything you found) and, on a story dungeon's last floor, the passage
//! out near the next one. Traps are in `hazards`, the puzzle room in `puzzle`,
//! rewards and boons in `boons`. The start room's west doorway always leads
//! back up. Falling below costs what you picked up during the run.
//!
//! Block changes on a floor (shut doors, opened gates, broken cracked walls,
//! fallen floor) are kept in `Below::edits` and put back whenever a chunk
//! there is built again (`apply_edits`).
//!
//! Screenshots and testing: `BLOCKGAME_GATE=1` starts in front of the Rotting
//! Gate, `BLOCKGAME_DELVE=n[:floor]` starts a run in story dungeon n. Every
//! story dungeon's position is logged.

pub mod boons;
pub mod hazards;
pub mod locks;
pub mod puzzle;
pub mod rules;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use voxel::{BlockId, BlockPos};
use worldgen::delve::ChestKind;
use worldgen::{Dungeon, Floor, FloorPlan, SpawnKind, SpawnPoint};

use crate::camera::FlyCamera;
use crate::combat::data::Role;
use crate::combat::{Foe, FoeBook, FoeSlain, SpawnFoe};
use crate::dungeon::spirit::Spirit;
use crate::dungeon::{LeftDungeon, Nearby};
use crate::inventory::grid::Stack;
use crate::inventory::loot::Loot;
use crate::inventory::{Inventory, Spill};
use crate::items::{ItemId, Items};
use crate::player::{MoveMode, Player};
use crate::save::SaveDir;
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};
use crate::survival::{Survival, Toast};
use crate::world::{Generator, PocketFloor, World};
use rules::{Delve, Run, Stair};

pub struct DelvePlugin;

impl Plugin for DelvePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Below>()
            .add_systems(crate::menu::GamePostStartup, (load, start_here).chain().after(crate::world::setup_generator))
            .add_systems(
                crate::menu::GameUpdate,
                (enter, walk_floor, take_key, open_locked, boss_falls, ways, fell_below, rest, apply_edits, brindle_talk)
                    .chain()
                    .in_set(DelveSet)
                    .after(crate::player::PlayerSet)
                    .after(crate::dungeon::spirit::SpiritSet),
            )
            .add_plugins((locks::LocksPlugin, hazards::HazardsPlugin, puzzle::PuzzlePlugin, boons::BoonsPlugin));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DelveSet;

/// Every foe of a run carries `RUN` in its tag's top byte, with its room and
/// wave below (`tag`); the floor's boss also has `BOSS`.
pub const RUN: u32 = 0xDE00_0000;
const BOSS: u32 = 0x0080_0000;

/// A run foe's tag.
pub fn tag(room: usize, wave: u8, boss: bool) -> u32 {
    RUN | if boss { BOSS } else { 0 } | ((room as u32 & 0x7FFF) << 8) | wave as u32
}

/// A tag's room and wave, if it's a run foe's.
pub fn tag_room(t: u32) -> Option<(usize, u8)> {
    (t & 0xFF00_0000 == RUN).then_some((((t >> 8) & 0x7FFF) as usize, (t & 0xFF) as u8))
}

pub fn is_boss(t: u32) -> bool {
    tag_room(t).is_some() && t & BOSS != 0
}

/// What you carried when the run began.
#[derive(Default, Clone)]
struct Brought {
    counts: BTreeMap<ItemId, u32>,
    worn: BTreeMap<crate::items::WearSlot, ItemId>,
}

/// The dungeons' saved state and the run in progress.
#[derive(Resource, Default)]
pub struct Below {
    pub delve: Delve,
    pub run: Option<Run>,
    pub floor: Option<Arc<Floor>>,
    pub dungeon: Option<Arc<Dungeon>>,
    /// (room, wave) whose foes have been sent.
    pub(crate) spawned: HashSet<(usize, u8)>,
    /// Rooms whose foes are all down (they never lock again this floor).
    pub cleared: HashSet<usize>,
    /// Blocks changed on this floor (kept when a chunk there is built again).
    pub edits: HashMap<BlockPos, BlockId>,
    /// The key lies on the floor here, waiting to be picked up.
    key_waiting: Option<Vec3>,
    brought: Brought,
    /// Seconds before a door takes you again (after coming back up).
    cooldown: f32,
    /// Seconds before a sealed-way note repeats.
    note: f32,
    /// Ids of the story dungeons by place (for `chain_cleared`).
    pub chain_ids: Vec<(u8, u64)>,
}

impl Below {
    /// The loot table a chest in the pocket rolls from, if it's a floor's chest.
    pub fn chest_table(&self, p: BlockPos) -> Option<&'static str> {
        let f = self.floor.as_ref()?;
        let c = f.chests.iter().find(|c| c.pos == [p.x, p.y, p.z])?;
        Some(match c.kind {
            ChestKind::Prize => "dungeon_prize",
            ChestKind::Treasure => "dungeon_treasure",
            ChestKind::Puzzle => "dungeon_puzzle",
            ChestKind::Secret => "dungeon_secret",
        })
    }

    /// The furthest story dungeon cleared (story beats).
    pub fn chain_cleared(&self) -> u8 {
        self.delve.chain_cleared(&self.chain_ids)
    }

    /// Change a block on this floor (and keep it changed).
    pub fn edit(&mut self, p: [i32; 3], b: BlockId) {
        self.edits.insert(BlockPos::new(p[0], p[1], p[2]), b);
    }

    pub fn edit_box(&mut self, b: &worldgen::Aabb, block: BlockId) {
        for y in b.min[1]..=b.max[1] {
            for z in b.min[2]..=b.max[2] {
                for x in b.min[0]..=b.max[0] {
                    self.edit([x, y, z], block);
                }
            }
        }
    }
}

fn load(mut below: ResMut<Below>, dir: Res<SaveDir>, generator: Res<Generator>) {
    if !crate::save::screenshot_run()
        && let Some(d) = crate::save::read_ron::<Delve>(&dir, "delve.ron")
    {
        below.delve = d;
    }
    let g = &generator.0;
    below.chain_ids = (1..=5u8).filter_map(|n| g.chain_dungeon(n).map(|d| (n, d.id))).collect();
    if let Some(gate) = g.rotting_gate() {
        below.delve.upgrade(gate.id, gate.floors);
    }
    for n in 1..=5u8 {
        if let Some(d) = g.chain_dungeon(n) {
            info!("story dungeon {n}: {} ({}, {} floors) at {:?}", d.name, g.delves.themes[d.theme].def.title, d.floors, d.gate);
        }
    }
}

fn save(delve: &Delve, dir: &SaveDir) {
    if !crate::save::screenshot_run() {
        crate::save::write_ron(dir, "delve.ron", delve);
    }
}

fn put_at(player: &mut Player, cam: &mut Query<(&mut Transform, &mut FlyCamera)>, to: Vec3) {
    player.pos = to;
    player.vel = Vec3::ZERO;
    // Wait for the ground to load before moving again.
    player.ready = false;
    for (mut tf, _) in cam.iter_mut() {
        tf.translation = to + Vec3::Y * crate::player::EYE;
    }
}

/// Turn the view toward a point.
fn face(cam: &mut Query<(&mut Transform, &mut FlyCamera)>, at: Vec3) {
    for (mut tf, mut fly) in cam.iter_mut() {
        let d = at - tf.translation;
        fly.yaw = (-d.x).atan2(-d.z);
        fly.pitch = d.y.atan2(Vec2::new(d.x, d.z).length());
        tf.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0);
    }
}

/// Everything a run's start, floor change and end touch.
#[derive(bevy::ecs::system::SystemParam)]
struct Travel<'w, 's> {
    commands: Commands<'w, 's>,
    generator: Res<'w, Generator>,
    below: ResMut<'w, Below>,
    player: ResMut<'w, Player>,
    cam: Query<'w, 's, (&'static mut Transform, &'static mut FlyCamera)>,
    world: ResMut<'w, World>,
    pocket: ResMut<'w, PocketFloor>,
    chests: ResMut<'w, crate::inventory::Chests>,
    foes: Query<'w, 's, (Entity, &'static Foe)>,
}

impl Travel<'_, '_> {
    /// Start a run of a dungeon on its first open floor.
    fn begin(&mut self, d: Arc<Dungeon>, floor: Option<u8>, inv: &Inventory, items: &Items) {
        let mut run = self.below.delve.begin(d.id, d.chain, d.floors);
        if let Some(f) = floor {
            run.floor = f.clamp(1, d.floors);
        }
        self.below.brought = Brought { counts: items.ids().map(|i| (i, inv.count(i))).filter(|(_, n)| *n > 0).collect(), worn: inv.worn.clone() };
        self.below.run = Some(run);
        self.below.dungeon = Some(d);
        self.go_to_floor();
    }

    /// Build the run's current floor and put you at its entry.
    fn go_to_floor(&mut self) {
        let (Some(run), Some(d)) = (self.below.run, self.below.dungeon.clone()) else { return };
        let g = &self.generator.0;
        let plan = FloorPlan {
            seed: g.seed(),
            dungeon: d.id,
            theme: d.theme,
            run: run.number,
            floor: run.floor,
            floors: run.floors,
            chain: d.chain.is_some_and(|n| n < 5),
            done: self.below.delve.done(d.id),
        };
        let floor = Arc::new(worldgen::delve::floor(&g.delves, &plan));
        self.pocket.0 = Some(floor.clone());
        for e in self.world.forget_pocket() {
            self.commands.entity(e).despawn();
        }
        self.despawn_foes();
        // Chests on the last floor's spots start over.
        self.chests.map.retain(|p, _| !worldgen::in_pocket(p.x, p.z));
        put_at(&mut self.player, &mut self.cam, Vec3::from_array(floor.entry));
        face(&mut self.cam, Vec3::from_array(floor.entry) + Vec3::new(1.0, crate::player::EYE, 0.0));
        let below = &mut self.below;
        below.spawned.clear();
        below.cleared.clear();
        below.edits.clear();
        below.key_waiting = None;
        // A cleared dungeon's last floor has no boss: its ways out stand open.
        if plan.done && plan.last() {
            let mut r = run;
            below.delve.boss_down(&mut r);
            below.run = Some(r);
        }
        below.floor = Some(floor);
    }

    fn despawn_foes(&mut self) {
        for (e, f) in &self.foes {
            if tag_room(f.tag).is_some() {
                self.commands.entity(e).despawn();
            }
        }
    }

    /// Leave the floors: back to the entrance (or `to`), the foes cleared away.
    fn end(&mut self, to: Option<Vec3>) {
        let d = self.below.dungeon.clone();
        let below = &mut self.below;
        below.run = None;
        below.floor = None;
        below.spawned.clear();
        below.cleared.clear();
        below.edits.clear();
        below.key_waiting = None;
        below.cooldown = 3.0;
        self.pocket.0 = None;
        self.despawn_foes();
        if let Some(at) = to.or(d.map(|d| Vec3::from_array(d.outside))) {
            put_at(&mut self.player, &mut self.cam, at);
        }
    }
}

/// `BLOCKGAME_GATE=1` / `BLOCKGAME_DELVE=n[:floor]`, and never wake up in the
/// pocket with no run (a save made below).
fn start_here(mut t: Travel, (inv, items): (Res<Inventory>, Res<Items>), mut spirit: ResMut<Spirit>, mut s: ResMut<Survival>) {
    let Some(gate) = t.generator.0.rotting_gate() else { return };
    if let Ok(v) = std::env::var("BLOCKGAME_DELVE") {
        let mut parts = v.split(':').map(|p| p.parse::<u8>().ok());
        let n = parts.next().flatten().unwrap_or(1).clamp(1, 5);
        let floor = parts.next().flatten();
        if let Some(d) = t.generator.0.chain_dungeon(n) {
            spirit.enter_as_spirit(d.clone(), &mut s);
            t.begin(d, floor, &inv, &items);
            return;
        }
    }
    let in_pocket = worldgen::in_pocket(t.player.pos.x as i32, t.player.pos.z as i32);
    if std::env::var_os("BLOCKGAME_GATE").is_some() || in_pocket {
        put_at(&mut t.player, &mut t.cam, Vec3::from_array(gate.outside));
        let at = Vec3::from_array(gate.gate.map(|v| v as f32)) + Vec3::Y * 1.5;
        face(&mut t.cam, at);
    }
}

/// A spirit walking through a dungeon's door goes down.
#[allow(clippy::too_many_arguments)]
fn enter(
    mut t: Travel,
    time: Res<Time>,
    mode: Res<MoveMode>,
    nearby: Res<Nearby>,
    spirit: Res<Spirit>,
    (inv, items, dir): (Res<Inventory>, Res<Items>, Res<SaveDir>),
    mut toast: MessageWriter<Toast>,
    mut cards: MessageWriter<crate::places::ShowCard>,
) {
    t.below.cooldown = (t.below.cooldown - time.delta_secs()).max(0.0);
    t.below.note = (t.below.note - time.delta_secs()).max(0.0);
    if t.below.run.is_some() || t.below.cooldown > 0.0 || *mode != MoveMode::Walk || !t.player.ready || !spirit.is_spirit() {
        return;
    }
    let at = t.player.pos.to_array();
    let Some(d) = nearby.list.iter().find(|d| d.inside(at)).cloned() else { return };
    t.begin(d.clone(), None, &inv, &items);
    save(&t.below.delve, &dir);
    let run = t.below.run.unwrap();
    let theme = &t.generator.0.delves.themes[d.theme].def;
    cards.write(crate::places::ShowCard { title: d.name.clone(), subtitle: format!("{} . Floor {} of {}", theme.title, run.floor, run.floors) });
    toast.write(Toast(format!("You go down into {}: floor {} of {}", d.name, run.floor, run.floors)));
}

/// The foe kind (index into foes.ron) for a spawn point, from the theme's foes by role.
pub fn kind_for(book: &FoeBook, theme: &worldgen::delve::ThemeDef, s: &SpawnPoint, floor: u8, floors: u8) -> Option<usize> {
    let find = |name: &str| book.kinds.iter().position(|k| k.name == name);
    if s.kind == SpawnKind::Boss {
        return find(rules::boss_for(&theme.warden, &theme.boss, floor, floors));
    }
    let list: Vec<usize> = theme.foes.iter().filter_map(|n| find(n)).collect();
    let want = |r: &Role| match s.kind {
        SpawnKind::Guard => matches!(r, Role::Brute | Role::Shield),
        _ => !matches!(r, Role::Brute),
    };
    let fit: Vec<usize> = list.iter().copied().filter(|&k| want(&book.kinds[k].role)).collect();
    let pool = if fit.is_empty() { &list } else { &fit };
    let roll = (s.pos[0].wrapping_mul(7) ^ s.pos[2].wrapping_mul(13) ^ ((s.wave as i32) * 5)).rem_euclid(pool.len().max(1) as i32) as usize;
    pool.get(roll).copied()
}

/// Send the foes of a room's wave (once).
pub(crate) fn spawn_wave(below: &mut Below, book: &FoeBook, generator: &Generator, room: usize, wave: u8, spawn: &mut MessageWriter<SpawnFoe>) {
    let (Some(run), Some(floor), Some(d)) = (below.run, below.floor.clone(), below.dungeon.clone()) else { return };
    if !below.spawned.insert((room, wave)) {
        return;
    }
    let theme = &generator.0.delves.themes[d.theme].def;
    let boost = 1.0 + 0.25 * (run.floor as f32 - 1.0) + 0.1 * (run.floors as f32 - 1.0);
    for s in floor.spawns.iter().filter(|s| s.room == room && s.wave == wave) {
        let boss = s.kind == SpawnKind::Boss;
        if boss && run.boss_down {
            continue;
        }
        let Some(kind) = kind_for(book, theme, s, run.floor, run.floors) else { continue };
        spawn.write(SpawnFoe {
            kind,
            at: Vec3::new(s.pos[0] as f32 + 0.5, s.pos[1] as f32, s.pos[2] as f32 + 0.5),
            // Bosses are made for their floor (foes.ron).
            boost: if boss { 1.0 + 0.1 * (run.floors as f32 - 1.0) } else { boost },
            tag: tag(room, wave, boss),
            home: None,
            health: None,
            elite: s.elite,
        });
    }
}

/// On a floor: rooms that don't lock (traps, treasure) fill as you come near.
fn walk_floor(mut below: ResMut<Below>, generator: Res<Generator>, book: Res<FoeBook>, player: Res<Player>, mut spawn: MessageWriter<SpawnFoe>) {
    let Some(floor) = below.floor.clone() else { return };
    if !player.ready {
        return;
    }
    let Some(room) = floor.room_at(player.pos.to_array()) else { return };
    let mut rooms = vec![room];
    for l in floor.links.iter().filter(|l| !l.secret) {
        if l.a == room {
            rooms.push(l.b);
        } else if l.b == room {
            rooms.push(l.a);
        }
    }
    for r in rooms {
        // Rooms that lock fill as the doors slam (`locks`), so nothing waits outside them.
        if !below.cleared.contains(&r) && !floor.rooms[r].kind.locks() {
            spawn_wave(&mut below, &book, &generator, r, 0, &mut spawn);
        }
    }
}

/// The key: it drops where the key room was, once that room is cleared.
#[derive(Component)]
struct KeyItem;

#[allow(clippy::too_many_arguments)]
fn take_key(
    mut commands: Commands,
    time: Res<Time>,
    mut below: ResMut<Below>,
    player: Res<Player>,
    keys: Query<(Entity, &mut Transform), With<KeyItem>>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut sound: MessageWriter<PlaySound>,
    mut toast: MessageWriter<Toast>,
) {
    let (Some(floor), Some(run)) = (below.floor.clone(), below.run) else {
        for (e, _) in &keys {
            commands.entity(e).despawn();
        }
        return;
    };
    // The key room cleared (or never held a foe): the key appears.
    let has_foes = floor.spawns.iter().any(|s| s.room == floor.key_room);
    if !run.key && !run.unlocked && below.key_waiting.is_none() && keys.is_empty() && (below.cleared.contains(&floor.key_room) || (!has_foes && floor.room_at(player.pos.to_array()) == Some(floor.key_room))) {
        let at = Vec3::new(floor.key_at[0] as f32 + 0.5, floor.key_at[1] as f32 + 0.9, floor.key_at[2] as f32 + 0.5);
        below.key_waiting = Some(at);
        let gold = materials.add(StandardMaterial { base_color: Color::srgb(0.95, 0.75, 0.25), emissive: LinearRgba::rgb(1.6, 1.1, 0.3), metallic: 0.8, ..default() });
        commands
            .spawn((KeyItem, Transform::from_translation(at), Visibility::default()))
            .with_children(|c| {
                c.spawn((Mesh3d(meshes.add(Torus::new(0.09, 0.16))), MeshMaterial3d(gold.clone()), Transform::from_xyz(0.0, 0.22, 0.0).with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))));
                c.spawn((Mesh3d(meshes.add(Cuboid::new(0.07, 0.42, 0.07))), MeshMaterial3d(gold.clone()), Transform::from_xyz(0.0, -0.05, 0.0)));
                c.spawn((Mesh3d(meshes.add(Cuboid::new(0.16, 0.06, 0.06))), MeshMaterial3d(gold), Transform::from_xyz(0.08, -0.2, 0.0)));
                c.spawn((PointLight { color: Color::srgb(1.0, 0.8, 0.4), intensity: 25_000.0, range: 5.0, shadow_maps_enabled: false, ..default() }, Transform::default()));
            });
        sound.write(PlaySound::new("craft_pop").at(at).speed(1.2));
        toast.write(Toast("A key falls to the floor".into()));
    }
    let t = time.elapsed_secs();
    for (e, mut tf) in keys {
        let Some(at) = below.key_waiting else {
            commands.entity(e).despawn();
            continue;
        };
        tf.translation = at + Vec3::Y * (t * 2.0).sin() * 0.12;
        tf.rotation = Quat::from_rotation_y(t * 1.5);
        if (player.pos + Vec3::Y).distance(at) < 1.6 {
            below.key_waiting = None;
            if let Some(r) = below.run.as_mut() {
                r.key = true;
            }
            commands.entity(e).despawn();
            sound.write(PlaySound::new("ui_pick").gain(1.0));
            toast.write(Toast("You take the floor's key. It opens the door to the boss".into()));
        }
    }
}

/// Walk up to the locked door with the key and it opens.
fn open_locked(
    mut below: ResMut<Below>,
    blocks: Res<crate::blocks::Blocks>,
    player: Res<Player>,
    mut sound: MessageWriter<PlaySound>,
    mut toast: MessageWriter<Toast>,
    mut bursts: MessageWriter<Burst>,
) {
    let (Some(floor), Some(run)) = (below.floor.clone(), below.run) else { return };
    if run.unlocked {
        return;
    }
    let l = floor.locked;
    let mid = Vec3::new((l.min[0] + l.max[0]) as f32 / 2.0 + 0.5, l.min[1] as f32 + 1.0, (l.min[2] + l.max[2]) as f32 / 2.0 + 0.5);
    if player.pos.distance(mid) > 3.2 {
        return;
    }
    if !run.key {
        if below.note <= 0.0 {
            below.note = 4.0;
            toast.write(Toast("Locked. The key is somewhere on this floor".into()));
        }
        return;
    }
    below.edit_box(&l, BlockId::AIR);
    if let Some(r) = below.run.as_mut() {
        r.key = false;
        r.unlocked = true;
    }
    let _ = &blocks;
    sound.write(PlaySound::new("clash").at(mid).speed(0.5));
    bursts.write(Burst::new(mid, BurstKind::Chips, 10));
    toast.write(Toast("The key turns. The door swings open".into()));
}

/// The boss falls: its ways on open, the floor is remembered as cleared, and
/// the last floor's boss clears the dungeon (a story dungeon's boss drops its urn).
#[allow(clippy::too_many_arguments)]
fn boss_falls(
    mut slain: MessageReader<FoeSlain>,
    mut below: ResMut<Below>,
    dir: Res<SaveDir>,
    blocks: Res<crate::blocks::Blocks>,
    (items, loot): (Res<Items>, Res<Loot>),
    mut waypoints: ResMut<crate::map::Waypoints>,
    generator: Res<Generator>,
    mut dice: Local<u32>,
    (mut spill, mut toast, mut gain): (MessageWriter<Spill>, MessageWriter<Toast>, MessageWriter<crate::progress::GainXp>),
) {
    let _ = &blocks;
    for s in slain.read() {
        if tag_room(s.tag).is_none() {
            continue;
        }
        let mut rand = || {
            *dice = dice.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (*dice >> 8) as f32 / (1u32 << 24) as f32
        };
        for stack in loot.roll(if is_boss(s.tag) { "rotting_warden" } else { "rotting" }, &mut rand) {
            spill.write(Spill { stack, at: s.at });
        }
        if !is_boss(s.tag) {
            continue;
        }
        let (Some(mut run), Some(floor), Some(d)) = (below.run, below.floor.clone(), below.dungeon.clone()) else { continue };
        let first = below.delve.boss_down(&mut run);
        below.run = Some(run);
        open_ways(&mut below, &floor);
        gain.write(crate::progress::GainXp { xp: 20.0 * run.floor as f32, why: "boss".into() });
        if first {
            toast.write(Toast(format!("{} is cleared", d.name)));
            waypoints.list.push(crate::map::Waypoint { name: format!("Cleared: {}", d.name), pos: d.outside });
            if let Some(n) = d.chain {
                if let Some(urn) = below.delve.claim_urn(n)
                    && let Some(item) = items.id(urn)
                {
                    spill.write(Spill { stack: Stack::new(item, 1), at: s.at });
                    toast.write(Toast(format!("The boss drops the {}", items.get(item).title)));
                }
                if let Some(next) = generator.0.chain_dungeon(n + 1) {
                    waypoints.list.push(crate::map::Waypoint { name: next.name.clone(), pos: next.outside });
                    if n < 4 {
                        toast.write(Toast(format!("A passage opens in the wall: it leads out near {}", next.name)));
                    }
                }
            }
        } else {
            toast.write(Toast(if run.floor < run.floors { "The boss falls. The stair down and the way out are open".into() } else { "The boss falls. The way out is open".to_string() }));
        }
        save(&below.delve, &dir);
    }
}

/// Unseal the boss room's ways on (the stair's mouth, the exit, the passage).
fn open_ways(below: &mut Below, floor: &Floor) {
    let puzzle_gate = floor.puzzle.as_ref().map(|p| p.gate);
    for s in floor.seals.iter().filter(|s| Some(**s) != puzzle_gate) {
        below.edit_box(s, BlockId::AIR);
    }
}

/// The stair down, the way out (start room or after the boss), and the passage.
#[allow(clippy::too_many_arguments)]
fn ways(
    mut t: Travel,
    (dir, mode): (Res<SaveDir>, Res<MoveMode>),
    mut toast: MessageWriter<Toast>,
    mut cards: MessageWriter<crate::places::ShowCard>,
) {
    let (Some(mut run), Some(floor), Some(d)) = (t.below.run, t.below.floor.clone(), t.below.dungeon.clone()) else { return };
    if !t.player.ready || *mode != MoveMode::Walk {
        return;
    }
    let at = t.player.pos.to_array();
    if Floor::in_box(&floor.way_out, at) {
        t.end(None);
        toast.write(Toast(format!("You climb back out of {}, keeping what you found", d.name)));
        return;
    }
    if !run.boss_down {
        return;
    }
    if Floor::in_box(&floor.exit, at) {
        t.end(None);
        toast.write(Toast(format!("You leave {} with everything you found", d.name)));
        return;
    }
    if let Some(p) = floor.passage
        && Floor::in_box(&p, at)
    {
        let next = d.chain.and_then(|n| t.generator.0.chain_dungeon(n + 1));
        let to = next.as_ref().map(|n| Vec3::from_array(n.outside) + Vec3::new(n.veil.out.0 as f32, 0.0, n.veil.out.1 as f32) * 6.0);
        t.end(to);
        if let Some(n) = next {
            cards.write(crate::places::ShowCard { title: n.name.clone(), subtitle: "The passage comes out here".into() });
        }
        return;
    }
    if let Some(s) = floor.stair
        && Floor::in_box(&s, at)
        && let Stair::Down(f) = t.below.delve.stair(&mut run)
    {
        t.below.run = Some(run);
        save(&t.below.delve, &dir);
        t.go_to_floor();
        cards.write(crate::places::ShowCard { title: d.name.clone(), subtitle: format!("Floor {f} of {}", run.floors) });
    }
}

/// Falling below (the spirit is cast out): lose what you picked up this run.
fn fell_below(
    mut t: Travel,
    mut left: MessageReader<LeftDungeon>,
    (mut inv, items): (ResMut<Inventory>, Res<Items>),
    mut toast: MessageWriter<Toast>,
) {
    let fell = left.read().any(|l| l.fell);
    if !fell || t.below.run.is_none() {
        return;
    }
    let now: BTreeMap<ItemId, u32> = items.ids().map(|i| (i, inv.count(i))).filter(|(_, n)| *n > 0).collect();
    let lost = rules::lost_on_death(&t.below.brought.counts, &now);
    let mut gone = 0;
    for (item, n) in &lost {
        if inv.take(*item, *n) {
            gone += n;
        }
    }
    // Clothes and armor put on below come off too.
    let worn: Vec<_> = inv.worn.iter().map(|(s, i)| (*s, *i)).collect();
    for (slot, item) in worn {
        if t.below.brought.worn.get(&slot) != Some(&item) {
            inv.worn.remove(&slot);
            gone += 1;
        }
    }
    // The spirit has already been cast out to the entrance.
    let at = t.player.pos;
    t.end(Some(at));
    toast.write(Toast(if gone > 0 { format!("Your spirit fell below. {gone} things you found there are lost") } else { "Your spirit fell below".into() }));
}

/// A rest room's campfire mends you while you sit by it.
fn rest(time: Res<Time>, below: Res<Below>, player: Res<Player>, mut s: ResMut<Survival>, mut told: Local<bool>, mut toast: MessageWriter<Toast>) {
    use crate::survival::health::Layer;
    let Some(fire) = below.floor.as_ref().and_then(|f| f.campfire) else {
        *told = false;
        return;
    };
    let at = Vec3::new(fire[0] as f32 + 0.5, fire[1] as f32, fire[2] as f32 + 0.5);
    if player.pos.distance(at) > 4.5 {
        return;
    }
    let dt = time.delta_secs();
    let h = &mut s.vitals.health;
    h.bleeding = (h.bleeding - dt * 0.5).max(0.0);
    h.heal_any(Layer::Injuries, 1.5 * dt);
    h.heal(Layer::Blood, 3.0 * dt);
    h.heal(Layer::Fatigue, 5.0 * dt);
    h.heal(Layer::Damage, 8.0 * dt);
    if !*told {
        *told = true;
        toast.write(Toast("The fire's warmth mends you".into()));
    }
}

/// Keep the floor's changed blocks changed (chunks there are built again from
/// the plan whenever they load).
fn apply_edits(time: Res<Time>, mut tick: Local<f32>, below: Res<Below>, mut world: ResMut<World>, blocks: Res<crate::blocks::Blocks>, mut bursts: MessageWriter<Burst>, mut sound: MessageWriter<PlaySound>) {
    *tick -= time.delta_secs();
    if *tick > 0.0 || below.edits.is_empty() {
        return;
    }
    *tick = 0.15;
    let slab = blocks.registry.id("dungeon_slab");
    let mut boomed = false;
    for (p, b) in &below.edits {
        if let Some(now) = world.block(*p)
            && now != *b
            && world.set_block(*p, *b)
        {
            crate::interact::remesh_around(&mut world, &blocks, *p);
            // A slab dropping into a doorway: dust and a boom.
            if Some(*b) == slab && !boomed {
                boomed = true;
                let at = Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5);
                sound.write(PlaySound::new("hit").at(at).speed(0.45).gain(1.2));
                bursts.write(Burst::new(at, BurstKind::Smoke, 10));
            }
        }
    }
}

/// While you talk to Brindlespheimer: what he says of the dungeon at hand.
fn brindle_talk(
    mut talk: ResMut<crate::npc::talk::Talk>,
    below: Res<Below>,
    nearby: Res<Nearby>,
    player: Res<Player>,
    generator: Res<Generator>,
    book: Res<FoeBook>,
) {
    if !talk.active() {
        return;
    }
    let Some(d) = nearby.nearest(player.pos) else { return };
    let theme = &generator.0.delves.themes[d.theme].def;
    let boss = book.kinds.iter().find(|k| k.name == theme.boss).map_or(theme.boss.clone(), |k| k.title.clone());
    let hint: Vec<&str> = theme
        .puzzles
        .iter()
        .map(|p| match p {
            worldgen::delve::PuzzleKind::Plates => "Plates in the floor want weight. The stone blocks move if you lean on them, or kick them.",
            worldgen::delve::PuzzleKind::Levers => "Lamps over a gate, levers on the far wall. Each lever flips more than one lamp. Count before you pull.",
            worldgen::delve::PuzzleKind::Timed => "Some gates only stay open while you run for them. So run.",
        })
        .collect();
    let vars = [("dungeon", d.name.clone()), ("floors", d.floors.to_string()), ("boss", boss), ("hint", hint.join(" "))];
    for (k, v) in vars {
        if talk.vars.get(k) != Some(&v) {
            talk.vars.insert(k.into(), v);
        }
    }
    let want = format!("theme:{}", theme.name);
    talk.extra.retain(|f| !f.starts_with("theme:") || *f == want);
    talk.extra.insert(want);
    if below.delve.done(d.id) {
        talk.extra.insert("cleared_here".into());
    } else {
        talk.extra.remove("cleared_here");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_carry_room_wave_and_boss() {
        let t = tag(12, 2, true);
        assert_eq!(tag_room(t), Some((12, 2)));
        assert!(is_boss(t));
        assert!(!is_boss(tag(3, 0, false)));
        assert_eq!(tag_room(0), None);
        assert_eq!(tag_room(0xC0DE), None);
    }

    #[test]
    fn every_theme_spawns_real_foes_of_the_right_role() {
        let kinds = crate::combat::data::load(include_str!("../../../../assets/data/foes.ron")).expect("foes.ron");
        let book = FoeBook { kinds };
        let tiles = voxel::TileSheet::from_ron(include_str!("../../../../assets/textures/blocks.ron")).unwrap();
        let reg = voxel::BlockRegistry::from_ron(include_str!("../../../../assets/data/blocks.ron"), &tiles).unwrap();
        let d = worldgen::Delves::load(include_str!("../../../../assets/data/dungeons.ron"), include_str!("../../../../assets/data/rooms.ron"), &reg).unwrap();
        for t in &d.themes {
            for name in t.def.foes.iter().chain([&t.def.warden, &t.def.boss]) {
                assert!(book.kinds.iter().any(|k| &k.name == name), "{}: no foe `{name}`", t.def.name);
            }
            for (kind, wave) in [(SpawnKind::Guard, 0), (SpawnKind::Pack, 1), (SpawnKind::Boss, 0)] {
                for x in 0..12 {
                    let s = SpawnPoint { pos: [x, 0, x * 3], kind, room: 1, wave, elite: false };
                    let k = kind_for(&book, &t.def, &s, 2, 2).expect("a kind");
                    if kind == SpawnKind::Boss {
                        assert_eq!(book.kinds[k].name, t.def.boss);
                    }
                    if kind == SpawnKind::Pack {
                        assert_ne!(book.kinds[k].role, Role::Brute);
                    }
                }
            }
        }
    }
}
