//! Encounters: a fight set up by the story (`assets/data/encounters.ron`).
//!
//! Each has an id, the foes (foes.ron kinds and counts), where they come from
//! (around you, at a place, or dropped from an airship that comes down), and
//! what beating them all does (set a flag, play a cutscene). A cutscene step
//! or a story beat starts one (`StartEncounter`). One runs at a time. If you
//! die it resets: its foes go, and all of them come back once you return.
//! The one in progress is saved (`encounter.ron`) and starts over on load.
//! Pure rules in `rules`.

pub mod rules;

use std::collections::BTreeSet;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::{Foe, FoeBook, FoeSlain, SpawnFoe};
use crate::player::Player;
use crate::save::SaveDir;
use rules::{Def, Phase, Running, From};

pub struct EncounterPlugin;

impl Plugin for EncounterPlugin {
    fn build(&self, app: &mut App) {
        let defs = rules::load(include_str!("../../../../assets/data/encounters.ron")).unwrap_or_else(|e| panic!("encounters.ron: {e}"));
        app.insert_resource(EncounterBook(defs))
            .init_resource::<Encounters>()
            .add_message::<StartEncounter>()
            .add_message::<EncounterCleared>()
            .add_message::<DropShip>()
            .add_systems(crate::menu::GamePostStartup, load)
            .add_systems(crate::menu::GameUpdate, (start, run, ships).chain().after(crate::player::PlayerSet))
            .add_systems(crate::menu::GameUpdate, save_now_and_then)
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
    }
}

/// Tag on every encounter foe (`SpawnFoe::tag`).
pub const TAG: u32 = 0x0E9C;
const FILE: &str = "encounter.ron";

#[derive(Resource)]
pub struct EncounterBook(pub Vec<Def>);

/// The encounter in progress and the ones beaten. Saved to `encounter.ron`.
#[derive(Resource, Debug, Default, Clone, Serialize, Deserialize)]
pub struct Encounters {
    #[serde(default)]
    pub running: Option<Running>,
    #[serde(default)]
    pub cleared: BTreeSet<String>,
}

/// Start an encounter by id, around this spot (where its `at` place doesn't say otherwise).
#[derive(Message, Debug, Clone)]
pub struct StartEncounter {
    pub id: String,
    pub at: Vec3,
}

/// All of an encounter's foes are down.
#[derive(Message, Debug, Clone)]
pub struct EncounterCleared(pub String);

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() || std::env::var_os("BLOCKGAME_BENCH").is_some()
}

fn load(mut enc: ResMut<Encounters>, dir: Res<SaveDir>) {
    if screenshot_run() {
        return;
    }
    if let Some(e) = std::fs::read_to_string(dir.file(FILE)).ok().and_then(|t| ron::from_str::<Encounters>(&t).map_err(|e| warn!("{FILE}: {e}")).ok()) {
        *enc = e;
        // The foes weren't saved: the fight starts over.
        if let Some(r) = &mut enc.running {
            r.restart();
        }
    }
}

fn start(mut msgs: MessageReader<StartEncounter>, book: Res<EncounterBook>, mut enc: ResMut<Encounters>, player: Res<Player>, mut auto: Local<bool>) {
    // `BLOCKGAME_ENCOUNTER=bog_ambush` starts one round you once the world is in (testing).
    let forced = (!*auto && player.ready).then(|| std::env::var("BLOCKGAME_ENCOUNTER").ok()).flatten().map(|id| StartEncounter { id, at: player.pos });
    if player.ready {
        *auto = true;
    }
    for m in msgs.read().cloned().chain(forced) {
        let Some(def) = book.0.iter().find(|d| d.id == m.id) else {
            warn!("no encounter `{}`", m.id);
            continue;
        };
        if enc.running.is_some() {
            warn!("encounter `{}` asked for while another runs", m.id);
            continue;
        }
        info!("encounter {} starts", m.id);
        enc.running = Some(Running::start(def, m.at.to_array()));
    }
}

/// Spawns, counts the fallen, resets on death, finishes.
#[allow(clippy::too_many_arguments)]
fn run(
    mut commands: Commands,
    time: Res<Time>,
    book: Res<EncounterBook>,
    foes_book: Option<Res<FoeBook>>,
    mut enc: ResMut<Encounters>,
    player: Res<Player>,
    survival: Res<crate::survival::Survival>,
    foes: Query<(Entity, &Foe)>,
    mut slain: MessageReader<FoeSlain>,
    (mut spawn, mut cleared, mut ship): (MessageWriter<SpawnFoe>, MessageWriter<EncounterCleared>, MessageWriter<DropShip>),
    mut flags: Option<ResMut<crate::cutscene::Story>>,
) {
    let fallen = slain.read().filter(|s| s.tag == TAG).count() as u32;
    let Some(r) = enc.running.as_mut() else { return };
    let Some(def) = book.0.iter().find(|d| d.id == r.id) else {
        enc.running = None;
        return;
    };
    // You died: its foes go, and it starts over when you come back.
    if survival.dead_for.is_some() && r.phase == Phase::Fighting {
        for (e, f) in &foes {
            if f.tag == TAG {
                commands.entity(e).despawn();
            }
        }
        r.died();
        return;
    }
    let near = Vec3::from_array(r.at).distance(player.pos) < rules::RETURN;
    let alive = survival.dead_for.is_none() && player.ready;
    match r.tick(time.delta_secs(), alive && near) {
        Some(rules::Go::Ship) => {
            ship.write(DropShip { over: Vec3::from_array(r.at) });
        }
        Some(rules::Go::Spawn) => {
            let Some(fb) = foes_book else { return };
            let at = Vec3::from_array(r.at);
            for (i, (kind, n)) in def.foes.iter().enumerate() {
                let Some(k) = fb.kinds.iter().position(|f| &f.name == kind) else {
                    warn!("encounter {}: no foe kind `{kind}`", def.id);
                    continue;
                };
                for j in 0..*n {
                    let a = (i as f32 * 2.1 + j as f32) * 1.3;
                    let ring = match def.from {
                        From::Around(r) => r,
                        From::Place(_, r) => r,
                        From::Airship(_) => 2.5,
                    };
                    let p = at + Vec3::new(a.cos(), 0.0, a.sin()) * ring + Vec3::Y * 2.0;
                    spawn.write(SpawnFoe { kind: k, at: p, boost: 1.0, tag: TAG });
                }
            }
        }
        None => {}
    }
    if fallen > 0 && r.slain(fallen) {
        info!("encounter {} cleared", def.id);
        if let (Some(f), Some(story)) = (&def.then.flag, flags.as_mut()) {
            story.flags.insert(f.clone());
        }
        cleared.write(EncounterCleared(def.id.clone()));
        let id = def.id.clone();
        enc.cleared.insert(id);
        enc.running = None;
    }
}

/// An airship coming down over a spot to drop an encounter's foes, then climbing away.
#[derive(Message, Debug, Clone, Copy)]
pub struct DropShip {
    pub over: Vec3,
}

#[derive(Component)]
struct Ship {
    over: Vec3,
    t: f32,
}

fn ships(
    mut commands: Commands,
    time: Res<Time>,
    mut drops: MessageReader<DropShip>,
    mut q: Query<(Entity, &mut Ship, &mut Transform)>,
    (mut meshes, mut mats): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
) {
    for d in drops.read() {
        let tf = Transform::from_translation(d.over + Vec3::Y * 40.0).with_scale(Vec3::ONE);
        let e = crate::cast::wren::build_ship(&mut commands, &mut meshes, &mut mats, tf, 2.2, Color::srgb(0.55, 0.12, 0.12));
        commands.entity(e).insert(Ship { over: d.over, t: 0.0 });
    }
    for (e, mut s, mut tf) in &mut q {
        s.t += time.delta_secs();
        let y = rules::ship_height(s.t);
        match y {
            Some(y) => tf.translation = s.over + Vec3::Y * y,
            None => commands.entity(e).despawn(),
        }
    }
}

fn store(dir: &SaveDir, enc: &Encounters) {
    match ron::ser::to_string_pretty(enc, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file(FILE), text) {
                warn!("saving {FILE}: {e}");
            }
        }
        Err(e) => warn!("{FILE}: {e}"),
    }
}

fn save_now_and_then(time: Res<Time<Real>>, mut since: Local<f32>, dir: Res<SaveDir>, enc: Res<Encounters>) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    store(&dir, &enc);
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, enc: Res<Encounters>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &enc);
    }
}
