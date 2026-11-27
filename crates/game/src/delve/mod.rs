//! The Rotting Gate in the game: walk into it to start a run on a floor laid
//! out fresh for that run (`worldgen::rotting`), fight through, take the stair
//! down once the boss falls, or walk back out through the start room's west
//! doorway. Rules (pure) in `rules`.
//!
//! The floor is built in the pocket (`worldgen::POCKET`), far out in empty
//! sky: `world::PocketFloor` tells chunk generation which floor to build there,
//! and those chunks are never saved. Foes are spawned room by room as you walk
//! in (`combat::SpawnFoe`, tagged so they can be cleared when you leave);
//! deeper floors make them tougher (`SpawnFoe::boost`). Dying below costs what
//! you picked up this run and wakes you outside the gate (`survival::RespawnAt`).
//!
//! Screenshots and testing: `BLOCKGAME_GATE=1` starts in front of the gate,
//! `BLOCKGAME_DELVE=n` starts a run at depth n. The gate's position is logged.

pub mod rules;

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use worldgen::{Floor, SpawnKind};

use crate::camera::FlyCamera;
use crate::combat::{Foe, FoeBook, FoeSlain, SpawnFoe};
use crate::inventory::grid::Stack;
use crate::inventory::loot::Loot;
use crate::inventory::{Inventory, Spill};
use crate::items::{ItemId, Items};
use crate::player::{MoveMode, Player};
use crate::save::SaveDir;
use crate::survival::{RespawnAt, Survival, Toast};
use crate::world::{Generator, PocketFloor, World};
use rules::{Delve, Run, Stair};

pub struct DelvePlugin;

impl Plugin for DelvePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Below>()
            .add_systems(crate::menu::GamePostStartup, (load, start_here).chain().after(crate::world::setup_generator))
            .add_systems(crate::menu::GameUpdate, (enter_gate, walk_floor, boss_falls, die_below).chain().after(crate::player::PlayerSet));
    }
}

/// Foes spawned on a floor carry this tag; the boss carries `BOSS`.
const TAG: u32 = 0xDE17;
const BOSS: u32 = 0xDE18;

/// What you carried when the run began.
#[derive(Default, Clone)]
struct Brought {
    counts: BTreeMap<ItemId, u32>,
    worn: BTreeMap<crate::items::WearSlot, ItemId>,
}

/// The gate's saved state and the run in progress.
#[derive(Resource, Default)]
pub(crate) struct Below {
    pub(crate) delve: Delve,
    run: Option<Run>,
    pub(crate) floor: Option<Arc<Floor>>,
    filled: HashSet<usize>,
    brought: Brought,
    /// Seconds before the gate takes you again (after coming back up).
    cooldown: f32,
    /// Seconds before a sealed-stair or similar note repeats.
    note: f32,
}

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() || std::env::var_os("BLOCKGAME_BENCH").is_some()
}

fn load(mut below: ResMut<Below>, dir: Res<SaveDir>, generator: Res<Generator>) {
    if !screenshot_run()
        && let Some(d) = std::fs::read_to_string(dir.file("delve.ron")).ok().and_then(|t| ron::from_str::<Delve>(&t).ok())
    {
        below.delve = d;
    }
    if let Some(g) = generator.0.rotting_gate() {
        info!("the Rotting Gate is at {:?}", g.pos);
    }
}

fn save(delve: &Delve, dir: &SaveDir) {
    if screenshot_run() {
        return;
    }
    match ron::ser::to_string_pretty(delve, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file("delve.ron"), text) {
                warn!("saving delve.ron: {e}");
            }
        }
        Err(e) => warn!("delve.ron: {e}"),
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

/// `BLOCKGAME_GATE=1` / `BLOCKGAME_DELVE=n`, and never wake up in the pocket
/// with no run (a save made below).
#[allow(clippy::too_many_arguments)]
fn start_here(
    mut commands: Commands,
    generator: Res<Generator>,
    mut below: ResMut<Below>,
    mut player: ResMut<Player>,
    mut cam: Query<(&mut Transform, &mut FlyCamera)>,
    (mut world, mut pocket): (ResMut<World>, ResMut<PocketFloor>),
    (inv, items): (Res<Inventory>, Res<Items>),
) {
    let Some(g) = generator.0.rotting_gate() else { return };
    let outside = Vec3::from_array(g.outside);
    if let Some(d) = std::env::var("BLOCKGAME_DELVE").ok().and_then(|v| v.parse::<u8>().ok()) {
        below.delve.cleared = d.saturating_sub(1).min(rules::DEPTHS - 1);
        begin(&mut commands, &generator, &mut below, &mut player, &mut cam, &mut world, &mut pocket, &inv, &items);
        return;
    }
    let in_pocket = worldgen::in_pocket(player.pos.x as i32, player.pos.z as i32);
    if std::env::var_os("BLOCKGAME_GATE").is_some() || in_pocket {
        put_at(&mut player, &mut cam, outside);
        let at = Vec3::from_array(g.pos.map(|v| v as f32)) + Vec3::Y * 1.5;
        face(&mut cam, at);
    }
}

/// Start a run: lay out its first floor and go down.
#[allow(clippy::too_many_arguments)]
fn begin(
    commands: &mut Commands,
    generator: &Generator,
    below: &mut Below,
    player: &mut Player,
    cam: &mut Query<(&mut Transform, &mut FlyCamera)>,
    world: &mut World,
    pocket: &mut PocketFloor,
    inv: &Inventory,
    items: &Items,
) {
    let run = below.delve.begin();
    below.brought = Brought { counts: items.ids().map(|i| (i, inv.count(i))).filter(|(_, n)| *n > 0).collect(), worn: inv.worn.clone() };
    below.run = Some(run);
    go_to_floor(commands, generator, below, player, cam, world, pocket);
}

/// Build the run's current floor and put you at its entry.
fn go_to_floor(
    commands: &mut Commands,
    generator: &Generator,
    below: &mut Below,
    player: &mut Player,
    cam: &mut Query<(&mut Transform, &mut FlyCamera)>,
    world: &mut World,
    pocket: &mut PocketFloor,
) {
    let Some(run) = below.run else { return };
    let floor = Arc::new(worldgen::rotting_floor(generator.0.seed(), run.number, run.depth));
    pocket.0 = Some(floor.clone());
    for e in world.forget_pocket() {
        commands.entity(e).despawn();
    }
    put_at(player, cam, Vec3::from_array(floor.entry));
    face(cam, Vec3::from_array(floor.entry) + Vec3::new(1.0, crate::player::EYE, 0.0));
    below.floor = Some(floor);
    below.filled.clear();
}

/// Leave the floors: back to the gate, the foes cleared away.
fn end_run(
    commands: &mut Commands,
    generator: &Generator,
    below: &mut Below,
    player: &mut Player,
    cam: &mut Query<(&mut Transform, &mut FlyCamera)>,
    foes: &Query<(Entity, &Foe)>,
    pocket: &mut PocketFloor,
) {
    below.run = None;
    below.floor = None;
    below.filled.clear();
    below.cooldown = 2.0;
    pocket.0 = None;
    for (e, f) in foes {
        if f.tag == TAG || f.tag == BOSS {
            commands.entity(e).despawn();
        }
    }
    if let Some(g) = generator.0.rotting_gate() {
        put_at(player, cam, Vec3::from_array(g.outside));
    }
}

/// Walk into the gate's chamber to go down.
#[allow(clippy::too_many_arguments)]
fn enter_gate(
    mut commands: Commands,
    time: Res<Time>,
    generator: Res<Generator>,
    mode: Res<MoveMode>,
    mut below: ResMut<Below>,
    mut player: ResMut<Player>,
    mut cam: Query<(&mut Transform, &mut FlyCamera)>,
    (mut world, mut pocket): (ResMut<World>, ResMut<PocketFloor>),
    (inv, items, dir): (Res<Inventory>, Res<Items>, Res<SaveDir>),
    mut toast: MessageWriter<Toast>,
) {
    below.cooldown = (below.cooldown - time.delta_secs()).max(0.0);
    let Some(g) = generator.0.rotting_gate() else { return };
    if below.run.is_some() || below.cooldown > 0.0 || *mode != MoveMode::Walk || !player.ready {
        return;
    }
    if !Floor::in_box(&g.door, player.pos.to_array()) {
        return;
    }
    begin(&mut commands, &generator, &mut below, &mut player, &mut cam, &mut world, &mut pocket, &inv, &items);
    save(&below.delve, &dir);
    let run = below.run.unwrap();
    toast.write(Toast(format!("You go down through the Rotting Gate: depth {} (run {})", run.depth, run.number)));
}

/// On a floor: rooms fill with foes as you reach them, the stair, the way out.
#[allow(clippy::too_many_arguments)]
fn walk_floor(
    mut commands: Commands,
    time: Res<Time>,
    generator: Res<Generator>,
    book: Res<FoeBook>,
    mut below: ResMut<Below>,
    mut player: ResMut<Player>,
    mut cam: Query<(&mut Transform, &mut FlyCamera)>,
    (mut world, mut pocket, dir): (ResMut<World>, ResMut<PocketFloor>, Res<SaveDir>),
    foes: Query<(Entity, &Foe)>,
    mut spawn: MessageWriter<SpawnFoe>,
    mut toast: MessageWriter<Toast>,
) {
    below.note = (below.note - time.delta_secs()).max(0.0);
    let (Some(mut run), Some(floor)) = (below.run, below.floor.clone()) else { return };
    if !player.ready {
        return;
    }
    let at = player.pos.to_array();
    // Fill the room you're in and the ones its corridors lead to.
    if let Some(room) = floor.room_at(at) {
        let mut rooms = vec![room];
        for &(a, b) in &floor.links {
            if a == room {
                rooms.push(b);
            } else if b == room {
                rooms.push(a);
            }
        }
        let boost = 1.0 + 0.35 * (run.depth as f32 - 1.0);
        for r in rooms {
            if !below.filled.insert(r) {
                continue;
            }
            for s in floor.spawns.iter().filter(|s| s.room == r) {
                let name = kind_for(s.kind, s.pos, run.depth);
                let Some(kind) = book.kinds.iter().position(|k| k.name == name) else { continue };
                let boss = s.kind == SpawnKind::Boss;
                if boss && run.boss_down {
                    continue;
                }
                spawn.write(SpawnFoe {
                    kind,
                    at: Vec3::new(s.pos[0] as f32 + 0.5, s.pos[1] as f32, s.pos[2] as f32 + 0.5),
                    // Each depth's boss is made for its depth (foes.ron).
                    boost: if boss { 1.0 } else { boost },
                    tag: if boss { BOSS } else { TAG },
                    home: None,
                    health: None,
                    elite: false,
                });
            }
        }
    }
    // The way back up.
    if Floor::in_box(&floor.way_out, at) {
        end_run(&mut commands, &generator, &mut below, &mut player, &mut cam, &foes, &mut pocket);
        toast.write(Toast("You climb back out of the Rotting Gate, keeping what you found".into()));
        return;
    }
    // The stair down.
    if Floor::in_box(&floor.stair, at) {
        match below.delve.stair(&mut run) {
            Stair::Sealed => {
                if below.note <= 0.0 {
                    below.note = 3.0;
                    toast.write(Toast("The stair is sealed while the floor's warden stands".into()));
                }
            }
            Stair::Down(depth) => {
                below.run = Some(run);
                save(&below.delve, &dir);
                for (e, f) in &foes {
                    if f.tag == TAG || f.tag == BOSS {
                        commands.entity(e).despawn();
                    }
                }
                go_to_floor(&mut commands, &generator, &mut below, &mut player, &mut cam, &mut world, &mut pocket);
                toast.write(Toast(format!("Down to depth {depth}")));
            }
            Stair::Done => {
                save(&below.delve, &dir);
                end_run(&mut commands, &generator, &mut below, &mut player, &mut cam, &foes, &mut pocket);
                toast.write(Toast("You have reached the bottom of the Rotting Gate, and climb back out".into()));
            }
        }
    }
}

/// The foe kind (by name in foes.ron) for a spawn point on a floor.
pub fn kind_for(kind: SpawnKind, pos: [i32; 3], depth: u8) -> &'static str {
    let roll = (pos[0] ^ pos[2].wrapping_mul(3)).rem_euclid(5);
    match kind {
        SpawnKind::Boss => rules::boss_for(depth),
        SpawnKind::Guard => {
            if roll < 3 {
                "rot_brute"
            } else {
                "mire_skitter"
            }
        }
        SpawnKind::Pack => match roll {
            0 | 1 => "mire_skitter",
            2 | 3 => "sporecaster",
            _ => "rot_brute",
        },
    }
}

/// Foes below drop the dungeon materials (loot.ron). The boss falls: the
/// stair opens, the next run may start a depth down, and the first time each
/// depth's boss falls it drops its urn.
#[allow(clippy::too_many_arguments)]
fn boss_falls(
    mut slain: MessageReader<FoeSlain>,
    mut below: ResMut<Below>,
    dir: Res<SaveDir>,
    (items, loot): (Res<Items>, Res<Loot>),
    mut dice: Local<u32>,
    mut spill: MessageWriter<Spill>,
    mut toast: MessageWriter<Toast>,
) {
    for s in slain.read() {
        if s.tag != BOSS && s.tag != TAG {
            continue;
        }
        let mut rand = || {
            *dice = dice.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (*dice >> 8) as f32 / (1u32 << 24) as f32
        };
        for stack in loot.roll(if s.tag == BOSS { "rotting_warden" } else { "rotting" }, &mut rand) {
            spill.write(Spill { stack, at: s.at });
        }
        if s.tag != BOSS {
            continue;
        }
        let Some(mut run) = below.run else { continue };
        below.delve.boss_down(&mut run);
        below.run = Some(run);
        if let Some(urn) = below.delve.claim_urn(run.depth)
            && let Some(item) = items.id(urn)
        {
            spill.write(Spill { stack: Stack::new(item, 1), at: s.at });
            toast.write(Toast(format!("The warden drops the {}", items.get(item).title)));
        }
        save(&below.delve, &dir);
        toast.write(Toast(if run.depth < rules::DEPTHS {
            format!("The warden of depth {} falls. The stair down is open", run.depth)
        } else {
            "The last warden falls. The stair leads back up".into()
        }));
    }
}

/// Dying below: lose what you picked up this run, wake outside the gate.
#[allow(clippy::too_many_arguments)]
fn die_below(
    mut commands: Commands,
    generator: Res<Generator>,
    s: Res<Survival>,
    mut below: ResMut<Below>,
    (mut inv, items): (ResMut<Inventory>, Res<Items>),
    mut respawn: ResMut<RespawnAt>,
    foes: Query<(Entity, &Foe)>,
    mut toast: MessageWriter<Toast>,
) {
    if below.run.is_none() || !s.vitals.health.is_dead() {
        return;
    }
    let now: BTreeMap<ItemId, u32> = items.ids().map(|i| (i, inv.count(i))).filter(|(_, n)| *n > 0).collect();
    let lost = rules::lost_on_death(&below.brought.counts, &now);
    let mut gone = 0;
    for (item, n) in &lost {
        if inv.take(*item, *n) {
            gone += n;
        }
    }
    // Clothes and armor put on below come off too.
    let worn: Vec<_> = inv.worn.iter().map(|(s, i)| (*s, *i)).collect();
    for (slot, item) in worn {
        if below.brought.worn.get(&slot) != Some(&item) {
            inv.worn.remove(&slot);
            gone += 1;
        }
    }
    below.run = None;
    below.floor = None;
    below.filled.clear();
    below.cooldown = 4.0;
    // (The floor stays built until you wake; the next run replaces it.)
    for (e, f) in &foes {
        if f.tag == TAG || f.tag == BOSS {
            commands.entity(e).despawn();
        }
    }
    respawn.0 = generator.0.rotting_gate().map(|g| Vec3::from_array(g.outside));
    toast.write(Toast(if gone > 0 { format!("You fell below. {gone} things you found there are lost") } else { "You fell below".into() }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_floor_spawn_names_a_real_foe() {
        let kinds = crate::combat::data::load(include_str!("../../../../assets/data/foes.ron")).expect("foes.ron");
        let mut seen = HashSet::new();
        for kind in [SpawnKind::Guard, SpawnKind::Pack, SpawnKind::Boss] {
            for x in 0..10 {
                let name = kind_for(kind, [x, 0, x * 7], (x % 4 + 1) as u8);
                assert!(kinds.iter().any(|k| k.name == name), "no foe `{name}`");
                seen.insert(name);
            }
        }
        for name in ["sporecaster", "rot_brute", "mire_skitter", "bog_warden", "spore_matron", "gloam_stalker", "rotting_king"] {
            assert!(seen.contains(name), "{name} never appears");
        }
        assert!(kinds.iter().any(|k| k.name == "sporecaster" && k.ranged.is_some()));
    }
}
