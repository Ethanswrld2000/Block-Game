//! The story: beats (`assets/data/story.ron`, pure rules in `beats`) that start
//! cutscenes when their conditions hold, the prisoners below the Rotting Gate
//! (Liora on depth 2, Messler in the depth 3 boss room), and the journal (J)
//! listing the clues found. Progress lives in `cutscene::Story` (`story.ron`).

pub mod beats;

use std::collections::BTreeSet;

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::camera::{FlyCamera, Menus};
use crate::cutscene::{Places, Playing, StartCutscene, Story};
use crate::inventory::Inventory;
use crate::items::Items;
use crate::npc::{body::SpawnNpc, talk::Talk, Npc, NpcBook, Order};
use crate::player::Player;
use crate::book::{ink, Book, BookLook, INK_FAINT};
use crate::theme::{colors, sizes, text, NoShadow, Theme};
use beats::StoryData;

pub struct StoryPlugin;

impl Plugin for StoryPlugin {
    fn build(&self, app: &mut App) {
        let data = beats::load(include_str!("../../../../assets/data/story.ron")).unwrap_or_else(|e| panic!("story.ron: {e}"));
        app.insert_resource(StoryBook(data))
            .init_resource::<Journal>()
            .add_systems(crate::menu::GameStartup, spawn_journal)
            .add_systems(crate::menu::GameUpdate, (note, check_beats, prisoners, journal).chain().after(crate::npc::talk::TalkSet));
    }
}

#[derive(Resource)]
pub struct StoryBook(pub StoryData);

/// `Npc.post` of a prisoner below the gate (not saved; it goes when the floor does).
pub const PRISON_POST: u64 = u64::MAX - 3;
/// How often beats are checked (seconds).
const CHECK_EVERY: f32 = 0.5;

/// Remembers who you've met, when things first came true, and the telescope.
#[allow(clippy::too_many_arguments)]
fn note(
    mut story: ResMut<Story>,
    book: Res<StoryBook>,
    talk: Res<Talk>,
    npc_book: Res<NpcBook>,
    npcs: Query<&Npc>,
    clock: Res<crate::clock::WorldClock>,
    urns: Res<crate::station::Urns>,
    scope: Option<Res<crate::station::telescope::Scope>>,
) {
    if let Some(k) = talk.with().and_then(|e| npcs.get(e).ok()).and_then(|n| npc_book.kinds.get(n.kind))
        && !story.met.contains(&k.dialogue)
    {
        story.met.insert(k.dialogue.clone());
    }
    if scope.is_some_and(|s| s.open) && !story.flags.contains("used_telescope") {
        story.flags.insert("used_telescope".into());
    }
    let keys = book.0.dated();
    let (flags, played) = (story.flags.clone(), story.played.clone());
    beats::note_days(&mut story.when, &keys, clock.day, |k| beats::key_true(k, &flags, &played, |s| urns.has(s)));
}

/// Starts the first beat whose conditions hold (when nothing else is going on).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn check_beats(
    time: Res<Time<Real>>,
    mut timer: Local<f32>,
    book: Res<StoryBook>,
    story: Res<Story>,
    playing: Res<Playing>,
    (player, talk, menus, places): (Res<Player>, Res<Talk>, Res<Menus>, Res<Places>),
    (npc_book, npcs): (Res<NpcBook>, Query<(&Transform, &Npc), Without<FlyCamera>>),
    (world, blocks, clock, survival): (Res<crate::world::World>, Res<crate::blocks::Blocks>, Res<crate::clock::WorldClock>, Res<crate::survival::Survival>),
    (urns, inv, items, below, encounters): (Res<crate::station::Urns>, Res<Inventory>, Res<Items>, Option<Res<crate::delve::Below>>, Res<crate::encounter::Encounters>),
    mut start: MessageWriter<StartCutscene>,
) {
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = CHECK_EVERY;
    if playing.busy() || !player.ready || talk.active() || menus.open || menus.still || survival.dead_for.is_some() {
        return;
    }
    let mut f = beats::Facts {
        day: clock.day,
        met: Some(&story.met),
        played: Some(&story.played),
        when: Some(&story.when),
        ..Default::default()
    };
    f.base.player = player.pos;
    f.base.npcs = crate::cutscene::sight(&player, &npc_book, npcs.iter(), &world, &blocks);
    f.base.places = places.0.clone();
    f.base.cleared = Some(&encounters.cleared);
    f.base.boss = below.as_ref().map_or(0, |b| b.delve.cleared);
    f.base.made = Some(&story.made);
    f.base.placed = Some(&story.placed);
    f.base.hour = clock.hour();
    f.base.flags = Some(&story.flags);
    // Urns you have: carried, set down, or growing.
    f.urns = urns.placed.keys().chain(urns.progress.keys()).cloned().collect();
    for p in &inv.pockets.items {
        if let Some(n) = items.get(p.stack.item).name.strip_suffix("_urn") {
            f.urns.insert(n.to_string());
        }
    }
    f.steps = book.0.steps().into_iter().filter(|s| urns.has(s)).collect();
    f.recruited = npcs
        .iter()
        .filter(|(_, n)| n.flags.contains(crate::npc::RECRUITED))
        .filter_map(|(_, n)| npc_book.kinds.get(n.kind).map(|k| k.dialogue.clone()))
        .collect::<BTreeSet<_>>();
    if let Some(b) = beats::next(&book.0.beats, &f) {
        info!("story beat {} -> {}", b.id, b.play);
        start.write(StartCutscene(b.play.clone()));
    }
}

/// Liora waits in a hall on depth 2 and Messler in the depth 3 boss room, until found.
/// They go with the floor (a fresh floor brings them back) unless a cutscene took them along.
#[allow(clippy::too_many_arguments)]
fn prisoners(
    mut commands: Commands,
    below: Option<Res<crate::delve::Below>>,
    story: Res<Story>,
    npc_book: Res<NpcBook>,
    npcs: Query<(Entity, &Npc)>,
    mut spawn: MessageWriter<SpawnNpc>,
    mut on: Local<Option<(u32, u8)>>,
) {
    let floor = below.as_ref().and_then(|b| b.floor.clone());
    let here = floor.as_ref().map(|f| (f.run, f.depth));
    if here == *on {
        return;
    }
    *on = here;
    // The floor changed: the old one's prisoners go.
    for (e, n) in &npcs {
        if n.post == Some(PRISON_POST) {
            commands.entity(e).despawn();
        }
    }
    let Some(floor) = floor else { return };
    for (who, depth, found) in [("liora", 2, "liora_found"), ("messler", 3, "messler_found")] {
        let Some(kind) = npc_book.kinds.iter().position(|k| k.dialogue == who) else { continue };
        if floor.depth != depth || story.flags.contains(found) || npcs.iter().any(|(_, n)| n.kind == kind && n.post != Some(PRISON_POST)) {
            continue;
        }
        let Some(at) = prison_spot(&floor, who == "messler") else { continue };
        let mut npc = Npc::new(kind, 0.0);
        npc.order = Order::Stay;
        npc.post = Some(PRISON_POST);
        spawn.write(SpawnNpc { npc, at: Vec3::from_array(at) });
    }
}

/// Where a prisoner waits (feet): the middle of a hall half way along, or of the boss room (pure).
pub fn prison_spot(floor: &worldgen::Floor, boss_room: bool) -> Option<[f32; 3]> {
    let halls: Vec<usize> = (0..floor.rooms.len()).filter(|&i| floor.rooms[i].kind == worldgen::FloorKind::Hall).collect();
    let i = if boss_room { floor.boss_room } else { *halls.get(halls.len() / 2)? };
    let r = &floor.rooms.get(i)?.inner;
    Some([(r.min[0] + r.max[0]) as f32 / 2.0, floor.floor_y as f32, (r.min[2] + r.max[2]) as f32 / 2.0])
}

// ---- The journal ----

/// J opens the journal: the clues found so far, in a `crate::book` you page through.
#[derive(Resource, Default)]
pub struct Journal {
    pub open: bool,
    shown: usize,
}

#[derive(Component)]
struct JournalUi;

/// Rough size of a page in characters, to split the clues into pages.
const JOURNAL_PAGE: usize = 520;

fn spawn_journal(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(30),
            JournalUi,
        ))
        .with_children(|root| {
            root.spawn(text("Left / Right or wheel   turn the page        J or Esc   close", sizes::SMALL, colors::MUTED));
            crate::book::build(root, BookLook { sides: 2, page: Vec2::new(330.0, 430.0), wheel: true }, ());
        });
}

/// J (or `BLOCKGAME_JOURNAL=1`, `=all` for every clue, for screenshots) opens and closes it; the clues are listed in the order found.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn journal(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut j: ResMut<Journal>,
    mut menus: ResMut<Menus>,
    story: Res<Story>,
    data: Res<StoryBook>,
    playing: Res<Playing>,
    theme: Res<Theme>,
    ui: Single<(&mut Visibility, &Children), With<JournalUi>>,
    mut books: Query<&mut Book>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut forced: Local<bool>,
) {
    let combo = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let auto = !*forced && std::env::var_os("BLOCKGAME_JOURNAL").is_some();
    *forced = true;
    let toggle = auto || (keys.just_pressed(KeyCode::KeyJ) && !combo);
    if j.open && (toggle || keys.just_pressed(KeyCode::Escape)) {
        j.open = false;
        menus.open = false;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    } else if !j.open && toggle && !menus.open && !playing.active() {
        j.open = true;
        menus.open = true;
        j.shown = usize::MAX;
    }
    let (mut vis, kids) = ui.into_inner();
    let want = if j.open { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    let Some(book_e) = kids.iter().find(|e| books.contains(*e)) else { return };
    let Ok(mut book) = books.get_mut(book_e) else { return };
    if !j.open {
        return;
    }
    // The clues, split into pages by how much they say.
    // `BLOCKGAME_JOURNAL=all` shows every clue (screenshots).
    let all: Vec<String> = if std::env::var("BLOCKGAME_JOURNAL").is_ok_and(|v| v == "all") { data.0.clues.keys().cloned().collect() } else { story.clues.clone() };
    let clues: Vec<(String, String)> = all.iter().map(|id| data.0.clues.get(id).map_or((id.clone(), String::new()), |c| (c.title.clone(), c.text.clone()))).collect();
    let costs: Vec<usize> = clues.iter().map(|(t, b)| 80 + t.len() + b.len()).collect();
    let pages = crate::book::paginate(&costs, JOURNAL_PAGE);
    book.set_pages(pages.len().max(1));
    if j.shown != all.len() {
        // Newly opened or a clue was found: open at the last page written.
        j.shown = all.len();
        book.open_at(usize::MAX);
        book.refill();
    }
    if !book.take_refill() {
        return;
    }
    for (page_e, n) in book.shown() {
        commands.entity(page_e).despawn_related::<Children>();
        commands.entity(page_e).with_children(|l| {
            if n == 0 {
                l.spawn((theme.bold(sizes::TITLE), Text::new("Journal"), TextColor(crate::book::INK), NoShadow));
                if clues.is_empty() {
                    l.spawn(ink("Nothing written down yet.", sizes::BODY, INK_FAINT));
                }
            }
            for (title, body) in pages.get(n).map_or(&clues[0..0], |r| &clues[r.clone()]) {
                l.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), margin: UiRect::bottom(Val::Px(8.0)), ..default() }).with_children(|c| {
                    c.spawn((theme.bold(sizes::LABEL), Text::new(title.clone()), TextColor(crate::book::INK), NoShadow));
                    c.spawn(ink(body.clone(), sizes::BODY, INK_FAINT));
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cutscene::script::{self, Runner, Status, Step};
    use crate::encounter::rules;

    /// The raid: C11 plays, ends on encounter raid_camp, which drops court soldiers from
    /// an airship; beating them all plays C12, which buries Bram and burns Sporrow.
    #[test]
    fn the_raid_runs_start_to_finish() {
        let data = beats::load(include_str!("../../../../assets/data/story.ron")).unwrap();
        let scenes = script::load(include_str!("../../../../assets/data/cutscenes.ron")).unwrap();
        let encounters = rules::load(include_str!("../../../../assets/data/encounters.ron")).unwrap();
        // The night after Messler's story, at camp.
        let played: BTreeSet<String> = ["C10".to_string()].into();
        let mut f = beats::Facts { day: 4, played: Some(&played), ..Default::default() };
        f.base.hour = 22.0;
        f.base.places.insert("camp".into(), Vec3::ZERO);
        f.base.player = Vec3::new(2.0, 0.0, 0.0);
        assert_eq!(beats::next(&data.beats, &f).map(|b| b.play.as_str()), Some("C11"));
        // C11 runs to its encounter and waits there.
        let c11 = scenes.iter().find(|c| c.id == "C11").unwrap();
        let mut r = Runner::default();
        let mut started = Vec::new();
        let said = Status { line_said: true, arrived: true, cleared: false };
        for _ in 0..100 {
            started.extend(r.tick(&c11.steps, 0.5, said));
        }
        let fight = started.iter().find_map(|&i| if let Step::Encounter(id) = &c11.steps[i] { Some(id.clone()) } else { None }).expect("C11 starts an encounter");
        assert_eq!(fight, "raid_camp");
        assert!(!r.over(&c11.steps), "waits for the fight");
        // The encounter: an airship comes down, court soldiers drop, all fall.
        let def = encounters.iter().find(|d| d.id == fight).unwrap();
        assert!(matches!(def.from, rules::From::Airship(_)));
        assert!(def.foes.iter().all(|(k, _)| k == "court_soldier"));
        let foes = include_str!("../../../../assets/data/foes.ron");
        assert!(foes.contains("name: \"court_soldier\"") && foes.contains("close: Some("), "soldiers shoot and cut");
        let mut run = rules::Running::start(def, [0.0; 3]);
        assert_eq!(run.tick(0.1, true), Some(rules::Go::Ship));
        let mut spawned = false;
        for _ in 0..100 {
            spawned |= run.tick(0.1, true) == Some(rules::Go::Spawn);
        }
        assert!(spawned);
        assert!(run.slain(run.total), "all beaten");
        // C11 ends; C12 follows.
        r.tick(&c11.steps, 0.1, Status { cleared: true, ..said });
        assert!(r.over(&c11.steps));
        let next = def.then.cutscene.clone().expect("C12 follows");
        let c12 = scenes.iter().find(|c| c.id == next).unwrap();
        let mut r = Runner::default();
        let mut done = Vec::new();
        for _ in 0..200 {
            done.extend(r.tick(&c12.steps, 0.5, said));
        }
        assert!(r.over(&c12.steps));
        let did = |want: &dyn Fn(&Step) -> bool| done.iter().any(|&i| want(&c12.steps[i]));
        assert!(did(&|s| matches!(s, Step::Remove(a) if a == "bram")), "Bram leaves the ancestral urn");
        assert!(did(&|s| matches!(s, Step::Place(_, b) if b == "grave")), "his grave");
        assert!(did(&|s| matches!(s, Step::Swap { to, .. } if to == "burnt_stem")) && did(&|s| matches!(s, Step::Swap { to, .. } if to == "burnt_cap")), "Sporrow burnt");
        let blocks = include_str!("../../../../assets/data/blocks.ron");
        for b in ["burnt_stem", "burnt_cap", "grave"] {
            assert!(blocks.contains(&format!("name: \"{b}\"")), "no block {b}");
        }
    }

    #[test]
    fn prisoners_wait_in_a_hall_and_the_boss_room() {
        let f = worldgen::rotting_floor(7, 1, 2);
        let hall = prison_spot(&f, false).expect("a hall");
        let r = f.room_at(hall).expect("in a room");
        assert_eq!(f.rooms[r].kind, worldgen::FloorKind::Hall);
        let f = worldgen::rotting_floor(7, 1, 3);
        let boss = prison_spot(&f, true).unwrap();
        assert_eq!(f.room_at(boss), Some(f.boss_room));
    }
}
