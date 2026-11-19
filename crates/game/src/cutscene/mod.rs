//! Cutscenes, played in the world (`assets/data/cutscenes.ron`).
//!
//! The real NPCs around you are the actors. When one starts, any actor that isn't
//! there is placed on its mark (or walks to it when it's on its way), then the
//! steps run: lines said with mouth and voice (a recording from `assets/voice/` or
//! the babble), the camera turning or moving, actors walking and facing, waits,
//! fades, an encounter (you fight it out of the cutscene, then it goes on), flags,
//! clues, items, blocks swapped or placed. While it plays: the talk's black bars
//! and subtitle, your input locked and the HUD hidden; the world keeps running.
//! Click ends a line; holding Esc skips the cutscene (what it gives or changes
//! still happens). Each plays once per world (`Story.played`, saved to `story.ron`).
//! The step runner and the trigger checks are pure, in `script`.
//! `BLOCKGAME_CUTSCENE=C02` plays one for screenshots.

pub mod script;

use std::collections::{BTreeSet, HashMap};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use serde::{Deserialize, Serialize};
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::{FlyCamera, Menus};
use crate::encounter::{EncounterBook, EncounterCleared, Encounters, StartEncounter};
use crate::inventory::{grid::Stack, Inventory, Spill};
use crate::items::Items;
use crate::npc::motion::Spring;
use crate::npc::talk::{KeepShown, SceneView, Talk};
use crate::npc::voice::{self, Speech};
use crate::npc::{body::SpawnNpc, ground_near, yaw_of, Npc, NpcBook, Order, CUE_NEAR, EYES};
use crate::player::Player;
use crate::save::SaveDir;
use crate::sound::PlaySound;
use crate::world::{Generator, World};
use script::{Cutscene, Facts, Mark, Runner, Status, Step, Target, PLAYER};

pub struct CutscenePlugin;

impl Plugin for CutscenePlugin {
    fn build(&self, app: &mut App) {
        let list = script::load(include_str!("../../../../assets/data/cutscenes.ron")).unwrap_or_else(|e| panic!("cutscenes.ron: {e}"));
        app.insert_resource(CutsceneBook(list))
            .init_resource::<Story>()
            .init_resource::<Playing>()
            .init_resource::<Places>()
            .add_message::<StartCutscene>()
            .add_systems(crate::menu::GameStartup, spawn_fade)
            .add_systems(crate::menu::GamePostStartup, load)
            .add_systems(crate::menu::GameUpdate, (refresh_places, record, begin, direct, camera, show, apply_edits).chain().after(crate::npc::talk::TalkSet))
            .add_systems(crate::menu::GameUpdate, save_now_and_then)
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
    }
}

const FILE: &str = "story.ron";
/// Actors further than this from you are placed on their mark; nearer ones walk there.
const PLACE_BEYOND: f32 = 24.0;
/// Actors nearer than this are close enough to start where they stand.
const NEARBY: f32 = 12.0;
/// Seconds to wait for actors to walk in before placing them.
const GATHER_MAX: f32 = 8.0;
/// A line stays up this long after it's said.
const LINE_HOLD: f32 = 1.2;
/// Hold Esc this long to skip.
const SKIP_HOLD: f32 = 1.0;
/// How often triggers are checked (seconds).
const CHECK_EVERY: f32 = 0.25;

#[derive(Resource)]
pub struct CutsceneBook(pub Vec<Cutscene>);

/// What the story has done in this world. Saved to `story.ron`.
#[derive(Resource, Debug, Default, Clone, Serialize, Deserialize)]
pub struct Story {
    #[serde(default)]
    pub flags: BTreeSet<String>,
    /// Clues for the journal, in the order found.
    #[serde(default)]
    pub clues: Vec<String>,
    /// Cutscenes played (once per world).
    #[serde(default)]
    pub played: BTreeSet<String>,
    /// Items made at benches and blocks placed (for triggers).
    #[serde(default)]
    pub made: BTreeSet<String>,
    #[serde(default)]
    pub placed: BTreeSet<String>,
    /// NPCs you have talked to (dialogue keys).
    #[serde(default)]
    pub met: BTreeSet<String>,
    /// The day each thing first came true (`flag:x`, `played:x`, `step:x`), for `DayAfter` beats.
    #[serde(default)]
    pub when: std::collections::BTreeMap<String, u32>,
    /// Block changes from cutscenes waiting for their area to be loaded.
    #[serde(default)]
    pub edits: Vec<Edit>,
}

/// A cutscene's change to the world's blocks, kept until it can be made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Edit {
    Swap { at: [i32; 3], radius: i32, from: String, to: String },
    Place { at: [i32; 3], block: String },
}

/// Named places for marks and `Area` triggers: camp (the bedroll, else the ancestral urn),
/// gate, sporrow, castle, forge. Kept up to date each frame.
#[derive(Resource, Default)]
pub struct Places(pub HashMap<String, Vec3>);

/// Play a cutscene by id (from a story beat or an encounter's `then`).
#[derive(Message, Debug, Clone)]
pub struct StartCutscene(pub String);

/// The cutscene playing.
#[derive(Resource, Default)]
pub struct Playing(Option<Scene>, Vec<String>);

impl Playing {
    pub fn active(&self) -> bool {
        self.0.is_some()
    }

    /// Playing, or one waiting to start.
    pub fn busy(&self) -> bool {
        self.0.is_some() || !self.1.is_empty()
    }
}

struct Scene {
    def: usize,
    runner: Runner,
    /// Seconds spent gathering the actors; `None` once they're all there.
    gathering: Option<f32>,
    actors: HashMap<String, Entity>,
    /// Actors asked to be spawned already.
    spawned: BTreeSet<String>,
    /// Where you stood and faced at the start (marks from `Player` hang off these).
    origin: Vec3,
    forward: Vec3,
    line: Option<Said>,
    look: Option<Target>,
    look_time: f32,
    cam: Option<[Spring; 2]>,
    /// The camera moving: from, to, seconds in, seconds.
    cam_move: Option<(Vec3, Vec3, f32, f32)>,
    /// The fade: from, to, seconds in, seconds.
    fade: (f32, f32, f32, f32),
    walker: Option<String>,
    encounter: Option<String>,
    cleared: bool,
    esc: f32,
    bars: Spring,
    ending: bool,
    /// Input is locked by us (false while you fight an encounter).
    locked: bool,
}

struct Said {
    actor: String,
    id: String,
    speech: Speech,
    text: String,
    name: String,
    title: String,
    t: f32,
    recorded: bool,
    pitch: f32,
    done: bool,
}

impl Scene {
    fn new(def: usize, origin: Vec3, forward: Vec3) -> Self {
        Self {
            def,
            runner: Runner::default(),
            gathering: Some(0.0),
            actors: HashMap::new(),
            spawned: BTreeSet::new(),
            origin,
            forward,
            line: None,
            look: None,
            look_time: 0.8,
            cam: None,
            cam_move: None,
            fade: (0.0, 0.0, 0.0, 0.0),
            walker: None,
            encounter: None,
            cleared: false,
            esc: 0.0,
            bars: Spring::default(),
            ending: false,
            locked: false,
        }
    }

    fn fade_now(&self) -> f32 {
        let (a, b, t, s) = self.fade;
        if s <= 0.0 { b } else { a + (b - a) * (t / s).clamp(0.0, 1.0) }
    }

    fn fighting(&self, steps: &[Step]) -> bool {
        !self.ending && self.gathering.is_none() && matches!(steps.get(self.runner.at), Some(Step::Encounter(_)))
    }
}

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() || std::env::var_os("BLOCKGAME_BENCH").is_some()
}

fn load(mut story: ResMut<Story>, dir: Res<SaveDir>) {
    if screenshot_run() {
        return;
    }
    if let Some(s) = std::fs::read_to_string(dir.file(FILE)).ok().and_then(|t| ron::from_str::<Story>(&t).map_err(|e| warn!("{FILE}: {e}")).ok()) {
        *story = s;
    }
}

#[derive(Component)]
struct FadeUi;

fn spawn_fade(mut commands: Commands) {
    commands.spawn((
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        BackgroundColor(Color::BLACK.with_alpha(0.0)),
        GlobalZIndex(60),
        Pickable::IGNORE,
        KeepShown,
        FadeUi,
    ));
}

/// Remembers what was made and placed, for the `Made` and `Placed` triggers.
fn record(mut story: ResMut<Story>, mut made: MessageReader<crate::station::board::Crafted>, mut placed: MessageReader<crate::interact::Placed>, blocks: Res<Blocks>) {
    for m in made.read() {
        if !story.made.contains(&m.0) {
            story.made.insert(m.0.clone());
        }
    }
    for p in placed.read() {
        let name = &blocks.registry.get(p.block).name;
        if !story.placed.contains(name) {
            story.placed.insert(name.clone());
        }
    }
}

fn refresh_places(mut out: ResMut<Places>, generator: Option<Res<Generator>>, camp: Res<crate::survival::camp::Camp>, urns: Res<crate::station::Urns>) {
    let m = &mut out.0;
    let v = |p: [i32; 3]| Vec3::new(p[0] as f32 + 0.5, p[1] as f32, p[2] as f32 + 0.5);
    match (&camp.bedroll, urns.placed.get("ancestral")) {
        (Some(b), _) => {
            m.insert("camp".into(), b.center());
        }
        (None, Some(u)) => {
            m.insert("camp".into(), Vec3::new(u.pos.0, u.pos.1, u.pos.2));
        }
        (None, None) => {
            m.remove("camp");
        }
    }
    if !m.contains_key("gate")
        && let Some(g) = generator
    {
        if let Some(gate) = g.0.rotting_gate() {
            m.insert("gate".into(), Vec3::from_array(gate.outside));
        }
        if let Some(s) = g.0.sporrow() {
            m.insert("sporrow".into(), v(s.center));
        }
        if let Some(k) = g.0.castle() {
            m.insert("castle".into(), v(k.center));
        }
        if let Some(f) = g.0.gilm_forge() {
            m.insert("forge".into(), v(f.pos));
        }
    }
}

fn key_of<'a>(book: &'a NpcBook, npc: &Npc) -> Option<&'a str> {
    book.kinds.get(npc.kind).map(|k| k.dialogue.as_str())
}

#[derive(SystemParam)]
struct Triggers<'w> {
    places: Res<'w, Places>,
    below: Option<Res<'w, crate::delve::Below>>,
    encounters: Res<'w, Encounters>,
    clock: Res<'w, crate::clock::WorldClock>,
    survival: Res<'w, crate::survival::Survival>,
    world: Res<'w, World>,
    blocks: Res<'w, Blocks>,
    enc_book: Res<'w, EncounterBook>,
}

/// Starts a cutscene: asked for, after an encounter, `BLOCKGAME_CUTSCENE`, or a trigger met.
#[allow(clippy::too_many_arguments)]
fn begin(
    time: Res<Time<Real>>,
    mut timer: Local<f32>,
    mut forced: Local<bool>,
    book: Res<CutsceneBook>,
    npc_book: Res<NpcBook>,
    story: Res<Story>,
    mut playing: ResMut<Playing>,
    (player, talk, menus): (Res<Player>, Res<Talk>, Res<Menus>),
    cam: Single<&Transform, With<FlyCamera>>,
    npcs: Query<(&Transform, &Npc), Without<FlyCamera>>,
    mut asked: MessageReader<StartCutscene>,
    mut cleared: MessageReader<EncounterCleared>,
    t: Triggers,
) {
    let mut want: Vec<String> = asked.read().map(|m| m.0.clone()).collect();
    for c in cleared.read() {
        if let Some(next) = t.enc_book.0.iter().find(|d| d.id == c.0).and_then(|d| d.then.cutscene.clone()) {
            want.push(next);
        }
    }
    if !*forced && player.ready {
        *forced = true;
        if let Ok(id) = std::env::var("BLOCKGAME_CUTSCENE") {
            want.push(id);
        }
    }
    for w in want {
        if !playing.1.contains(&w) {
            playing.1.push(w);
        }
    }
    if playing.active() || !player.ready || talk.active() || t.survival.dead_for.is_some() {
        return;
    }
    let start = |id: &str| book.0.iter().position(|c| c.id == id);
    let mut pick = None;
    while pick.is_none() && !playing.1.is_empty() {
        let id = playing.1.remove(0);
        pick = start(&id);
        if pick.is_none() {
            warn!("no cutscene `{id}`");
        }
    }
    *timer -= time.delta_secs();
    if pick.is_none() && *timer <= 0.0 && !menus.open {
        *timer = CHECK_EVERY;
        let seen = sight(&player, &npc_book, npcs.iter(), &t.world, &t.blocks);
        let facts = Facts {
            player: player.pos,
            npcs: seen,
            places: t.places.0.clone(),
            cleared: Some(&t.encounters.cleared),
            boss: t.below.as_ref().map_or(0, |b| b.delve.cleared),
            made: Some(&story.made),
            placed: Some(&story.placed),
            hour: t.clock.hour(),
            flags: Some(&story.flags),
        };
        pick = book.0.iter().position(|c| script::ready(c, &story.played, &facts));
    }
    let Some(def) = pick else { return };
    info!("cutscene {} starts", book.0[def].id);
    playing.0 = Some(Scene::new(def, player.pos, cam.forward().as_vec3()));
}

/// NPCs near you by dialogue key: where they stand, and whether you can see them.
pub(crate) fn sight<'a>(player: &Player, book: &NpcBook, npcs: impl Iterator<Item = (&'a Transform, &'a Npc)>, world: &World, blocks: &Blocks) -> Vec<(String, Vec3, bool)> {
    let eye = player.pos + Vec3::Y * player.eye;
    let solid = |c: IVec3| world.block(BlockPos::new(c.x, c.y, c.z)).is_some_and(|id| blocks.registry.get(id).solid);
    npcs.filter(|(tf, _)| tf.translation.distance(player.pos) < 48.0)
        .filter_map(|(tf, n)| {
            let k = key_of(book, n)?;
            let head = tf.translation + Vec3::Y * (EYES - 0.1);
            Some((k.to_string(), tf.translation, script::clear_line(eye, head, solid)))
        })
        .collect()
}

#[derive(SystemParam)]
struct Effects<'w> {
    story: ResMut<'w, Story>,
    inv: ResMut<'w, Inventory>,
    items: Res<'w, Items>,
    world: Res<'w, World>,
    blocks: Res<'w, Blocks>,
    spill: MessageWriter<'w, Spill>,
    encounter: MessageWriter<'w, StartEncounter>,
}

impl Effects<'_> {
    /// The lasting part of a step (also done for skipped steps).
    fn apply(&mut self, step: &Step, player: Vec3, at: &dyn Fn(&Mark) -> Option<Vec3>) {
        match step {
            Step::Flag(f) => {
                self.story.flags.insert(f.clone());
            }
            Step::Clue(c) => {
                if !self.story.clues.contains(c) {
                    self.story.clues.push(c.clone());
                }
            }
            Step::Give(name, n) => match self.items.id(name) {
                Some(id) => {
                    if let Some(left) = self.inv.add(&self.items, Stack::new(id, *n)) {
                        self.spill.write(Spill { stack: left, at: player + Vec3::Y });
                    }
                }
                None => warn!("cutscene gives unknown item `{name}`"),
            },
            // Block changes wait (saved) until their area is loaded: `apply_edits`.
            Step::Swap { at: m, radius, from, to } => match at(m) {
                Some(c) => self.story.edits.push(Edit::Swap { at: c.floor().as_ivec3().to_array(), radius: *radius, from: from.clone(), to: to.clone() }),
                None => warn!("cutscene swap: no spot"),
            },
            Step::Place(m, block) => match at(m) {
                Some(c) => self.story.edits.push(Edit::Place { at: c.floor().as_ivec3().to_array(), block: block.clone() }),
                None => warn!("cutscene place: no spot"),
            },
            Step::Encounter(id) => {
                self.encounter.write(StartEncounter { id: id.clone(), at: player });
            }
            _ => {}
        }
    }
}

/// Gathers the actors, runs the steps, says the lines, ends the cutscene.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn direct(
    time: Res<Time>,
    book: Res<CutsceneBook>,
    npc_book: Res<NpcBook>,
    mut playing: ResMut<Playing>,
    (player, mut menus, keys, mouse): (Res<Player>, ResMut<Menus>, Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    mut npcs: Query<(Entity, &mut Transform, &mut Npc), Without<FlyCamera>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    (mut spawn, mut sounds, mut cleared): (MessageWriter<SpawnNpc>, MessageWriter<PlaySound>, MessageReader<EncounterCleared>),
    (mut commands, server, settings, places): (Commands, Res<AssetServer>, Option<Res<crate::settings::Settings>>, Res<Places>),
    mut fx: Effects,
) {
    let dt = time.delta_secs().min(0.05);
    let won: Vec<String> = cleared.read().map(|c| c.0.clone()).collect();
    let Some(scene) = playing.0.as_mut() else { return };
    let def = &book.0[scene.def];
    let steps = &def.steps;
    let place_at = &places.0;
    let mut where_is: HashMap<String, Vec3> = HashMap::new();
    for (e, tf, _) in &npcs {
        if let Some((k, _)) = scene.actors.iter().find(|(_, a)| **a == e) {
            where_is.insert(k.clone(), tf.translation);
        }
    }
    let (origin, forward) = (scene.origin, scene.forward);
    let resolve = |m: &Mark, where_is: &HashMap<String, Vec3>| script::resolve(m, origin, forward, |a| where_is.get(a).copied(), |p| place_at.get(p).copied());

    // Lock input (not while you fight an encounter).
    let lock = !scene.fighting(steps);
    if lock != scene.locked {
        scene.locked = lock;
        menus.open = lock;
        menus.still = lock;
        if !lock {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
    } else if lock {
        menus.open = true;
        menus.still = true;
    }

    // Gather the actors first.
    if let Some(waited) = scene.gathering.as_mut() {
        *waited += dt;
        let late = *waited > GATHER_MAX;
        let mut ready = true;
        for (key, mark) in &def.actors {
            let found = npcs
                .iter()
                .filter(|(_, _, n)| key_of(&npc_book, n) == Some(key.as_str()))
                .min_by(|a, b| a.1.translation.distance(player.pos).total_cmp(&b.1.translation.distance(player.pos)))
                .map(|(e, _, _)| e);
            let spot = resolve(mark, &where_is).and_then(|p| ground_near(&fx.world, &fx.blocks, p + Vec3::Y * 2.0).or(Some(p)));
            let Some(e) = found else {
                ready = false;
                if scene.spawned.insert(key.clone()) {
                    match (npc_book.kinds.iter().position(|k| &k.dialogue == key), spot) {
                        (Some(kind), Some(at)) => {
                            let mut npc = Npc::new(kind, yaw_of(player.pos - at));
                            npc.order = Order::Stay;
                            spawn.write(SpawnNpc { npc, at });
                        }
                        _ => warn!("cutscene {}: can't bring in `{key}`", def.id),
                    }
                } else if late {
                    // Never came (no such NPC): go on without it.
                    continue;
                }
                continue;
            };
            scene.actors.insert(key.clone(), e);
            let Ok((_, mut tf, mut npc)) = npcs.get_mut(e) else { continue };
            let d = tf.translation.distance(player.pos);
            let Some(spot) = spot else { continue };
            if d > PLACE_BEYOND || (late && d > NEARBY) {
                tf.translation = spot;
                npc.vel = Vec3::ZERO;
                npc.cue = Some(spot);
            } else if d > NEARBY {
                npc.cue = Some(spot);
                ready = false;
            } else if npc.cue.is_none() {
                npc.cue = Some(tf.translation);
            }
        }
        // Wait for the walkers.
        for e in scene.actors.values() {
            if let Ok((_, tf, npc)) = npcs.get(*e)
                && let Some(c) = npc.cue
                && Vec2::new(c.x - tf.translation.x, c.z - tf.translation.z).length() > CUE_NEAR * 2.0
            {
                ready = false;
            }
        }
        if ready || late {
            scene.gathering = None;
            scene.bars = Spring::default();
        } else {
            return;
        }
    }

    // How the step under way is doing.
    for w in &won {
        if scene.encounter.as_deref() == Some(w.as_str()) {
            scene.cleared = true;
        }
    }
    let arrived = scene.walker.as_ref().and_then(|a| scene.actors.get(a)).and_then(|e| npcs.get(*e).ok()).is_none_or(|(_, tf, npc)| {
        npc.cue.is_none_or(|c| Vec2::new(c.x - tf.translation.x, c.z - tf.translation.z).length() <= CUE_NEAR * 1.6)
    });
    if mouse.just_pressed(MouseButton::Left)
        && let Some(l) = scene.line.as_mut()
    {
        l.done = true;
    }
    let status = Status { line_said: scene.line.as_ref().is_none_or(|l| l.done), arrived, cleared: scene.cleared };

    // Hold Esc to skip.
    scene.esc = if keys.pressed(KeyCode::Escape) && !scene.ending { scene.esc + dt } else { 0.0 };
    let player_pos = player.pos;
    let mut begun = Vec::new();
    if !scene.ending {
        if scene.esc >= SKIP_HOLD {
            for i in scene.runner.skip(steps) {
                if !cast(&steps[i], &scene.actors, &mut npcs, &mut commands) {
                    fx.apply(&steps[i], player_pos, &|m| resolve(m, &where_is));
                }
            }
            scene.ending = true;
        } else {
            begun = scene.runner.tick(steps, dt, status);
        }
    }

    for i in begun {
        let step = &steps[i];
        match step {
            Step::Say(actor, id) => {
                let line = npc_book.lines.get(id);
                let text = npc_book.lines.text(id).to_string();
                let speaker = line.map_or(actor.as_str(), |l| l.speaker.as_str());
                let kind = npc_book.kinds.iter().find(|k| k.dialogue == speaker);
                let name = kind.map_or_else(|| capitalised(speaker), |k| k.name.clone());
                let title = kind.map_or(String::new(), |k| k.title.clone());
                let (pitch, rate) = kind.map_or((1.0, 6.0), |k| (k.voice.pitch, k.voice.rate));
                let recorded = matches!(crate::lines::voice(id), crate::lines::Voice::Recorded(_));
                if actor != PLAYER {
                    scene.look = Some(Target::Actor(actor.clone()));
                    scene.look_time = 0.6;
                }
                scene.line = Some(Said { actor: actor.clone(), id: id.clone(), speech: voice::speak(&text, rate), text, name, title, t: -0.25, recorded, pitch, done: false });
            }
            Step::Camera(target, secs) => {
                scene.look = Some(target.clone());
                scene.look_time = *secs;
            }
            Step::CameraMove(mark, target, secs) => {
                if let Some(to) = resolve(mark, &where_is) {
                    let from = scene.cam_move.map_or(player.pos + Vec3::Y * player.eye, |(a, b, t, s)| a.lerp(b, smooth(t / s.max(0.01))));
                    scene.cam_move = Some((from, to, 0.0, secs.max(0.01)));
                }
                scene.look = Some(target.clone());
                scene.look_time = *secs;
            }
            Step::Walk(actor, mark) => {
                let to = resolve(mark, &where_is).map(|p| ground_near(&fx.world, &fx.blocks, p + Vec3::Y * 2.0).unwrap_or(p));
                if let (Some(e), Some(to)) = (scene.actors.get(actor), to)
                    && let Ok((_, _, mut npc)) = npcs.get_mut(*e)
                {
                    npc.cue = Some(to);
                    npc.face = None;
                }
                scene.walker = Some(actor.clone());
            }
            Step::Face(actor, target) => {
                let at = match target {
                    Target::Player => Some(player.pos),
                    Target::Actor(a) => where_is.get(a).copied(),
                    Target::Mark(m) => resolve(m, &where_is),
                };
                if let (Some(e), Some(at)) = (scene.actors.get(actor), at)
                    && let Ok((_, _, mut npc)) = npcs.get_mut(*e)
                {
                    npc.face = Some(at);
                }
            }
            Step::Fade(to, secs) => {
                scene.fade = (scene.fade_now(), *to, 0.0, *secs);
            }
            Step::Encounter(id) => {
                scene.encounter = Some(id.clone());
                scene.cleared = false;
                fx.apply(step, player_pos, &|m| resolve(m, &where_is));
            }
            Step::Wait(_) => {}
            Step::Follow(_) | Step::Remove(_) => {
                cast(step, &scene.actors, &mut npcs, &mut commands);
            }
            _ => fx.apply(step, player_pos, &|m| resolve(m, &where_is)),
        }
    }

    // The line being said: voice, mouth, and done after a moment.
    if let Some(l) = scene.line.as_mut() {
        let before = l.t;
        l.t += dt;
        let speaker = scene.actors.get(&l.actor).copied();
        let at = speaker.and_then(|e| npcs.get(e).ok()).map(|(_, tf, _)| tf.translation + Vec3::Y * EYES);
        if l.recorded && before < 0.0 && l.t >= 0.0 {
            let volume = settings.as_ref().map_or(1.0, |s| s.volume);
            crate::lines::play_recording(&mut commands, &server, &l.id, volume, at);
        }
        if !l.recorded {
            for s in l.speech.starting(before, l.t) {
                let lilt = 1.0 + 0.07 * ((s.start * 7.3).sin() + 0.5 * (s.start * 2.1).cos());
                sounds.write(PlaySound { at, ..PlaySound::new(format!("voice_{}", s.vowel)).speed(l.pitch * lilt) });
            }
        }
        let speaking = !l.done && l.t >= 0.0 && l.t < l.speech.length;
        if let Some(e) = speaker
            && let Ok((_, _, mut npc)) = npcs.get_mut(e)
        {
            npc.mouth = if speaking { l.speech.mouth(l.t) } else { (0.0, 0.8) };
            npc.speaking = if speaking { 1.0 } else { 0.0 };
        }
        if l.t >= l.speech.length + LINE_HOLD {
            l.done = true;
        }
    }
    scene.fade.2 += dt;
    if let Some(m) = scene.cam_move.as_mut() {
        m.2 += dt;
    }

    if !scene.ending && scene.runner.over(steps) {
        scene.ending = true;
    }
    if scene.ending {
        scene.line = None;
        if scene.fade.1 > 0.0 {
            scene.fade = (scene.fade_now(), 0.0, 0.0, 0.6);
        }
        if scene.bars.x < 0.02 && scene.fade_now() < 0.02 {
            // Over: everyone back to what they were doing.
            for e in scene.actors.values() {
                if let Ok((_, _, mut npc)) = npcs.get_mut(*e) {
                    npc.cue = None;
                    npc.face = None;
                    npc.mouth = (0.0, 0.8);
                    npc.speaking = 0.0;
                }
            }
            if scene.locked {
                menus.open = false;
                menus.still = false;
                cursor.grab_mode = CursorGrabMode::Locked;
                cursor.visible = false;
            }
            info!("cutscene {} done", def.id);
            fx.story.played.insert(def.id.clone());
            playing.0 = None;
        }
    }
}

/// Steps that change an actor for good: follow you, or leave the world. False for other steps.
fn cast(step: &Step, actors: &HashMap<String, Entity>, npcs: &mut Query<(Entity, &mut Transform, &mut Npc), Without<FlyCamera>>, commands: &mut Commands) -> bool {
    match step {
        Step::Follow(a) => {
            if let Some(e) = actors.get(a)
                && let Ok((_, _, mut npc)) = npcs.get_mut(*e)
            {
                npc.order = Order::Follow;
                npc.post = None;
                npc.flags.insert(crate::npc::dialogue::FOLLOWING.to_string());
            }
            true
        }
        Step::Remove(a) => {
            if let Some(e) = actors.get(a) {
                commands.entity(*e).despawn();
            }
            true
        }
        _ => false,
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Turns (and moves) the camera to what the cutscene looks at.
#[allow(clippy::too_many_arguments)]
fn camera(
    time: Res<Time<Real>>,
    book: Res<CutsceneBook>,
    mut playing: ResMut<Playing>,
    player: Res<Player>,
    npcs: Query<&Transform, (With<Npc>, Without<FlyCamera>)>,
    cam: Single<(&mut Transform, &mut FlyCamera)>,
    places: Res<Places>,
) {
    let Some(scene) = playing.0.as_mut() else { return };
    if scene.gathering.is_some() && scene.look.is_none() || scene.fighting(&book.0[scene.def].steps) {
        scene.cam = None;
        return;
    }
    let dt = time.delta_secs().min(0.05);
    let (mut tf, mut fly) = cam.into_inner();
    if let Some((a, b, t, s)) = scene.cam_move {
        tf.translation = a.lerp(b, smooth(t / s));
    }
    let at = |key: &str| scene.actors.get(key).and_then(|e| npcs.get(*e).ok()).map(|t| t.translation);
    let place_at = &places.0;
    let point = match &scene.look {
        None => return,
        Some(Target::Player) => Some(player.pos + Vec3::Y * 1.4),
        Some(Target::Actor(a)) => at(a).map(|p| p + Vec3::Y * (EYES - 0.08)),
        Some(Target::Mark(m)) => script::resolve(m, scene.origin, scene.forward, |a| at(a), |p| place_at.get(p).copied()),
    };
    let Some(point) = point else { return };
    let d = point - tf.translation;
    if d.length_squared() < 0.01 {
        return;
    }
    let yaw = yaw_of(d);
    let pitch = d.y.atan2(Vec2::new(d.x, d.z).length()).clamp(-1.2, 1.2);
    let springs = scene.cam.get_or_insert_with(|| [Spring { x: fly.yaw, v: 0.0 }, Spring { x: fly.pitch, v: 0.0 }]);
    let ease = (scene.look_time * 0.35).max(0.15);
    let y = springs[0].step_angle(yaw, ease, dt);
    let p = springs[1].step(pitch, ease, dt);
    fly.yaw += crate::npc::motion::wrap(y - fly.yaw);
    fly.pitch = p;
    tf.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0);
}

/// Bars and subtitle (through the talk's), and the fade.
fn show(time: Res<Time<Real>>, book: Res<CutsceneBook>, mut playing: ResMut<Playing>, mut talk: ResMut<Talk>, mut fade: Single<&mut BackgroundColor, With<FadeUi>>) {
    let dt = time.delta_secs().min(0.05);
    let Some(scene) = playing.0.as_mut() else {
        if talk.scene.is_some() {
            talk.scene = None;
        }
        if fade.0.alpha() > 0.0 {
            fade.0 = Color::BLACK.with_alpha(0.0);
        }
        return;
    };
    let fighting = scene.fighting(&book.0[scene.def].steps);
    let target = if scene.ending || fighting { 0.0 } else { 1.0 };
    let bars = scene.bars.step(target, 0.16, dt);
    fade.0 = Color::BLACK.with_alpha(scene.fade_now().clamp(0.0, 1.0));
    if fighting {
        talk.scene = None;
        return;
    }
    let (name, title, text) = match &scene.line {
        Some(l) => {
            let n = if l.done { l.text.chars().count() } else if l.t < 0.0 { 0 } else { l.speech.shown(l.t) };
            (l.name.clone(), l.title.clone(), l.text.chars().take(n).collect())
        }
        None => Default::default(),
    };
    talk.scene = Some(SceneView { bars, name, title, text });
}

/// Makes the cutscenes' block changes once their area is loaded (they wait in `Story.edits`, saved).
fn apply_edits(time: Res<Time<Real>>, mut timer: Local<f32>, mut story: ResMut<Story>, mut world: ResMut<World>, blocks: Res<Blocks>) {
    *timer -= time.delta_secs();
    if *timer > 0.0 || story.edits.is_empty() {
        return;
    }
    *timer = 0.5;
    let reg = &blocks.registry;
    story.edits.retain(|e| match e {
        Edit::Swap { at, radius, from, to } => {
            let (Some(a), Some(b)) = (reg.id(from), reg.id(to)) else {
                warn!("cutscene swap: no block `{from}` or `{to}`");
                return false;
            };
            let r = *radius;
            let c = IVec3::from_array(*at);
            // Every chunk in the box must be there.
            let step = voxel::CHUNK_SIZE as i32;
            let span = |lo: i32, hi: i32| (lo..=hi).step_by(step as usize).chain(std::iter::once(hi));
            for x in span(c.x - r, c.x + r) {
                for y in span(c.y - r, c.y + r) {
                    for z in span(c.z - r, c.z + r) {
                        if world.block(BlockPos::new(x, y, z)).is_none() {
                            return true;
                        }
                    }
                }
            }
            for x in -r..=r {
                for y in -r..=r {
                    for z in -r..=r {
                        let p = BlockPos::new(c.x + x, c.y + y, c.z + z);
                        if world.block(p) == Some(a) {
                            world.set_block(p, b);
                        }
                    }
                }
            }
            false
        }
        Edit::Place { at, block } => {
            let Some(id) = reg.id(block) else {
                warn!("cutscene place: no block `{block}`");
                return false;
            };
            let p = BlockPos::new(at[0], at[1], at[2]);
            if world.block(p).is_none() {
                return true;
            }
            world.set_block(p, id);
            false
        }
    });
}

fn store(dir: &SaveDir, story: &Story) {
    match ron::ser::to_string_pretty(story, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file(FILE), text) {
                warn!("saving {FILE}: {e}");
            }
        }
        Err(e) => warn!("{FILE}: {e}"),
    }
}

fn save_now_and_then(time: Res<Time<Real>>, mut since: Local<f32>, dir: Res<SaveDir>, story: Res<Story>) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    store(&dir, &story);
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, story: Res<Story>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &story);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_for_speakers_without_an_npc() {
        assert_eq!(capitalised("nayphen"), "Nayphen");
        assert_eq!(smooth(0.0), 0.0);
        assert_eq!(smooth(1.0), 1.0);
    }

    /// C15: two actors gathered from marks, walked in, facing, lines in order.
    #[test]
    fn a_cutscene_with_two_actors_runs_through() {
        let list = script::load(include_str!("../../../../assets/data/cutscenes.ron")).unwrap();
        let c = list.iter().find(|c| c.id == "C15").unwrap();
        let origin = Vec3::new(0.0, 10.0, 0.0);
        let at = |m: &Mark| script::resolve(m, origin, Vec3::NEG_Z, |_| None, |_| None).unwrap();
        // Wren starts on the right, Liora on the left, both ahead of you.
        let (w, l) = (at(&c.actors["wren"]), at(&c.actors["liora"]));
        assert!(w.x > 0.0 && l.x < 0.0 && w.z < 0.0 && l.z < 0.0);
        let mut r = Runner::default();
        let mut said = Vec::new();
        let mut walked = Vec::new();
        let mut status = Status::default();
        for _ in 0..200 {
            for i in r.tick(&c.steps, 0.1, status) {
                match &c.steps[i] {
                    Step::Say(a, id) => said.push((a.clone(), id.clone())),
                    Step::Walk(a, _) => walked.push(a.clone()),
                    _ => {}
                }
            }
            // Each walk arrives and each line is said a tick later.
            status = Status { line_said: true, arrived: true, cleared: false };
            if r.over(&c.steps) {
                break;
            }
        }
        assert!(r.over(&c.steps));
        assert_eq!(walked, vec!["wren", "liora"]);
        let who: Vec<&str> = said.iter().map(|(a, _)| a.as_str()).collect();
        assert_eq!(who, vec!["wren", "player", "wren", "liora"]);
        assert!(matches!(c.steps.last(), Some(Step::Flag(f)) if f == "airship_built"));
    }
}
