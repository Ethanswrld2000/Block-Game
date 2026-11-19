//! The cast in the world: who stands where, and what they do for you.
//!
//! - Brindlespheimer stands at the Rotting Gate (respawned there, not saved).
//! - Gilm turns up at his hut outside Sporrow, Orrin on the castle tower and the aunt
//!   outside her cottage on her island above the clouds, once per world (`cast.ron` remembers it; from then on `npc::people` saves them).
//! - The Star Urn is set on the castle tower once per world.
//! - `services`: Orrin's weather reading, Messler's lessons and cooking remarks,
//!   Gilm's and Liora's commissions, Wren's trades.
//! - `wren`: Wren lands by the vehicle urn every other day.

pub mod services;
pub mod wren;

use std::collections::BTreeSet;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::npc::{body::SpawnNpc, ground_near, yaw_of, Npc, NpcBook, Order};
use crate::player::Player;
use crate::save::SaveDir;
use crate::world::{Generator, World};

pub struct CastPlugin;

impl Plugin for CastPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cast>()
            .add_systems(crate::menu::GamePostStartup, (load, set_star_urn).chain().after(crate::world::setup_generator))
            .add_systems(crate::menu::GameUpdate, (place_cast, keep_brindlespheimer).before(crate::npc::NpcSet).after(crate::player::PlayerSet))
            .add_systems(crate::menu::GameUpdate, save_now_and_then)
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game))
            .add_plugins((services::ServicesPlugin, wren::WrenPlugin));
    }
}

const FILE: &str = "cast.ron";
/// `Npc.post` of Brindlespheimer at the gate (so he isn't saved and is put back there).
pub const GATE_POST: u64 = u64::MAX - 1;
/// People are put in place when you come this close.
const NEAR: f32 = 96.0;
/// Brindlespheimer goes when you are this far from the gate.
const GONE: f32 = 160.0;

/// Who and what has been put in the world already (once per world). Saved to `cast.ron`.
#[derive(Resource, Debug, Default, Clone, Serialize, Deserialize)]
pub struct Cast {
    #[serde(default)]
    pub placed: BTreeSet<String>,
}

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() || std::env::var_os("BLOCKGAME_BENCH").is_some()
}

fn load(mut commands: Commands, dir: Res<SaveDir>, generator: Option<Res<Generator>>) {
    let cast = if screenshot_run() {
        Cast::default()
    } else {
        std::fs::read_to_string(dir.file(FILE)).ok().and_then(|t| ron::from_str(&t).map_err(|e| warn!("{FILE}: {e}")).ok()).unwrap_or_default()
    };
    commands.insert_resource(cast);
    let Some(g) = generator else { return };
    if let Some(v) = g.0.sporrow() {
        info!("Sporrow at {:?}", v.center);
    }
    if let Some(f) = g.0.gilm_forge() {
        info!("Gilm's hut at {:?}", f.pos);
    }
    if let Some(k) = g.0.castle() {
        info!("castle at {:?}, tower roof at {:?}", k.center, [k.tower.0, k.roof, k.tower.1]);
    }
    if let Some(a) = g.0.aunt_island() {
        info!("the aunt's island at {:?}, her cottage at {:?}", a.center, a.cottage);
    }
}

/// The Star Urn stands on the castle tower until you take it.
fn set_star_urn(mut cast: ResMut<Cast>, mut urns: ResMut<crate::station::Urns>, generator: Option<Res<Generator>>) {
    let Some(k) = generator.as_ref().and_then(|g| g.0.castle()) else { return };
    if !cast.placed.insert("star_urn".into()) || urns.placed.contains_key("star") || urns.progress.contains_key("star") {
        return;
    }
    let [x, y, z] = k.urn;
    urns.placed.insert("star".into(), crate::station::Placed { pos: (x, y, z), along_x: true, back: 1.0 });
}

/// Gilm at his hut, Orrin on the castle tower, the aunt at her cottage: once each,
/// when you first come near.
#[allow(clippy::too_many_arguments)]
fn place_cast(
    time: Res<Time>,
    mut timer: Local<f32>,
    mut cast: ResMut<Cast>,
    generator: Option<Res<Generator>>,
    book: Res<NpcBook>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<crate::blocks::Blocks>,
    mut spawn: MessageWriter<SpawnNpc>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 || !player.ready {
        return;
    }
    *timer = 1.0;
    let Some(g) = generator else { return };
    let mut spots: Vec<(&str, Vec3, Vec3, bool)> = Vec::new();
    if let Some(f) = g.0.gilm_forge() {
        let door = Vec3::new(f.pos[0] as f32 + 0.5, f.pos[1] as f32, f.pos[2] as f32 + 0.5);
        spots.push(("gilm", Vec3::from_array(f.stand), door, true));
    }
    if let Some(k) = g.0.castle() {
        spots.push(("orrin", Vec3::from_array(k.orrin), Vec3::from_array(k.urn), false));
    }
    if let Some(a) = g.0.aunt_island() {
        let arch = Vec3::new(a.arch[0] as f32 + 0.5, a.arch[1] as f32, a.arch[2] as f32 + 0.5);
        spots.push(("aunt", Vec3::from_array(a.stand), arch, false));
    }
    for (who, at, look, wander) in spots {
        if cast.placed.contains(who) || Vec2::new(at.x - player.pos.x, at.z - player.pos.z).length() > NEAR {
            continue;
        }
        let Some(kind) = book.kinds.iter().position(|k| k.dialogue == who) else { continue };
        let Some(feet) = ground_near(&world, &blocks, at + Vec3::Y * 2.0) else { continue };
        let mut npc = Npc::new(kind, yaw_of(look - feet));
        npc.order = if wander { Order::Wander { home: feet } } else { Order::Stay };
        spawn.write(SpawnNpc { npc, at: feet });
        cast.placed.insert(who.to_string());
        info!("{who} is at {feet}");
    }
}

/// Brindlespheimer stands beside the Rotting Gate whenever you are near it.
#[allow(clippy::too_many_arguments)]
fn keep_brindlespheimer(
    mut commands: Commands,
    generator: Option<Res<Generator>>,
    book: Res<NpcBook>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<crate::blocks::Blocks>,
    npcs: Query<(Entity, &Npc)>,
    mut spawn: MessageWriter<SpawnNpc>,
    mut asked: Local<f32>,
    time: Res<Time>,
) {
    let Some(g) = generator.as_ref().and_then(|g| g.0.rotting_gate()) else { return };
    let outside = Vec3::from_array(g.outside);
    let dist = Vec2::new(outside.x - player.pos.x, outside.z - player.pos.z).length();
    let here: Vec<Entity> = npcs.iter().filter(|(_, n)| n.post == Some(GATE_POST)).map(|(e, _)| e).collect();
    if dist > GONE {
        for e in here {
            commands.entity(e).despawn();
        }
        return;
    }
    *asked -= time.delta_secs();
    if !here.is_empty() || dist > NEAR || *asked > 0.0 || !player.ready {
        return;
    }
    let Some(kind) = book.kinds.iter().position(|k| k.dialogue == "brindlespheimer") else { return };
    // Beside the path, a little off to one side of the door.
    let side = Vec3::new(-g.out.1 as f32, 0.0, g.out.0 as f32);
    let want = outside + side * 2.5 + Vec3::new(g.out.0 as f32, 0.0, g.out.1 as f32);
    let Some(at) = ground_near(&world, &blocks, want + Vec3::Y * 2.0) else { return };
    let mut npc = Npc::new(kind, yaw_of(Vec3::new(g.out.0 as f32, 0.0, g.out.1 as f32)));
    npc.order = Order::Stay;
    npc.post = Some(GATE_POST);
    spawn.write(SpawnNpc { npc, at });
    *asked = 3.0;
}

fn store(dir: &SaveDir, cast: &Cast) {
    match ron::ser::to_string_pretty(cast, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file(FILE), text) {
                warn!("saving {FILE}: {e}");
            }
        }
        Err(e) => warn!("{FILE}: {e}"),
    }
}

fn save_now_and_then(time: Res<Time<Real>>, mut since: Local<f32>, dir: Res<SaveDir>, cast: Res<Cast>) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    store(&dir, &cast);
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, cast: Res<Cast>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &cast);
    }
}
