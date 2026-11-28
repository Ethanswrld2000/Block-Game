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
    f.base.boss = below.as_ref().map_or(0, |b| b.chain_cleared());
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

/// Messler waits in the Rotting Gate's last boss room once its boss is down, and
/// Liora in a fight room half way along the Flooded Crypt's second floor, until
/// found. They go with the floor (a fresh floor brings them back) unless a
/// cutscene took them along.
#[allow(clippy::too_many_arguments)]
fn prisoners(
    mut commands: Commands,
    below: Option<Res<crate::delve::Below>>,
    story: Res<Story>,
    npc_book: Res<NpcBook>,
    npcs: Query<(Entity, &Npc)>,
    mut spawn: MessageWriter<SpawnNpc>,
    mut on: Local<Option<(u32, u8, bool)>>,
) {
    let floor = below.as_ref().and_then(|b| b.floor.clone());
    let run = below.as_ref().and_then(|b| b.run);
    let here = run.map(|r| (r.number, r.floor, r.boss_down));
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
    let (Some(floor), Some(run)) = (floor, run) else { return };
    for (who, chain, at_floor, found, boss_room) in [("liora", 2, 2, "liora_found", false), ("messler", 1, 2, "messler_found", true)] {
        let Some(kind) = npc_book.kinds.iter().position(|k| k.dialogue == who) else { continue };
        if run.chain != Some(chain) || run.floor != at_floor || (boss_room && !run.boss_down) {
            continue;
        }
        if story.flags.contains(found) || npcs.iter().any(|(_, n)| n.kind == kind && n.post != Some(PRISON_POST)) {
            continue;
        }
        let Some(at) = prison_spot(&floor, boss_room) else { continue };
        let mut npc = Npc::new(kind, 0.0);
        npc.order = Order::Stay;
        npc.post = Some(PRISON_POST);
        spawn.write(SpawnNpc { npc, at: Vec3::from_array(at) });
    }
}

/// Where a prisoner waits (feet): the middle of a fight room (or ambush) half way along, or of
/// the boss room (pure).
pub fn prison_spot(floor: &worldgen::Floor, boss_room: bool) -> Option<[f32; 3]> {
    let fights: Vec<usize> = (0..floor.rooms.len()).filter(|&i| matches!(floor.rooms[i].kind, worldgen::RoomType::Fight | worldgen::RoomType::Ambush)).collect();
    let i = if boss_room { floor.boss_room } else { *fights.get(fights.len() / 2)? };
    let r = &floor.rooms.get(i)?.inner;
    // Off the very middle (the boss stood there, and pillars may).
    let mid = floor.room_free_spot(i).unwrap_or([(r.min[0] + r.max[0]) / 2, floor.floor_y, (r.min[2] + r.max[2]) / 2]);
    Some([mid[0] as f32 + 0.5, mid[1] as f32, mid[2] as f32 + 0.5])
}

// ---- The journal ----

/// The guide's journal entries (title, text): each step done ("(done)", no text), then
/// the one under way. Empty until the guide starts.
pub fn guide_entries(data: &crate::guide::rules::GuideData, flags: &BTreeSet<String>) -> Vec<(String, String)> {
    use crate::guide::rules;
    if !flags.contains(rules::ON) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for s in &data.steps {
        if flags.contains(&rules::flag(&s.id)) {
            out.push((format!("{} (done)", s.title), String::new()));
        } else {
            out.push((s.title.clone(), s.text.clone()));
            break;
        }
    }
    out
}

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
    guide: Res<crate::guide::GuideBook>,
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
    let mut clues: Vec<(String, String)> = all.iter().map(|id| data.0.clues.get(id).map_or((id.clone(), String::new()), |c| (c.title.clone(), c.text.clone()))).collect();
    // The guide's steps come first: the ones done, ticked off, then the one under way.
    let guide_steps = guide_entries(&guide.0, &story.flags);
    let progress = guide_steps.len();
    clues.splice(0..0, guide_steps);
    let costs: Vec<usize> = clues.iter().map(|(t, b)| 80 + t.len() + b.len()).collect();
    let pages = crate::book::paginate(&costs, JOURNAL_PAGE);
    book.set_pages(pages.len().max(1));
    let written = all.len() + progress * 1000;
    if j.shown != written {
        // Newly opened, a clue found or a step done: open at the last page written.
        j.shown = written;
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
    fn the_journal_walks_through_the_guide_one_step_at_a_time() {
        let data = crate::guide::rules::load(include_str!("../../../../assets/data/guide.ron")).unwrap();
        let mut flags = BTreeSet::new();
        assert!(guide_entries(&data, &flags).is_empty(), "nothing before the opening");
        flags.insert("guide_on".to_string());
        let e = guide_entries(&data, &flags);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].0, data.steps[0].title);
        flags.insert(crate::guide::rules::flag(&data.steps[0].id));
        let e = guide_entries(&data, &flags);
        assert_eq!(e.len(), 2);
        assert!(e[0].0.ends_with("(done)") && e[1].0 == data.steps[1].title && !e[1].1.is_empty());
    }

    #[test]
    fn prisoners_wait_in_a_fight_room_and_the_boss_room() {
        let tiles = voxel::TileSheet::from_ron(include_str!("../../../../assets/textures/blocks.ron")).unwrap();
        let reg = voxel::BlockRegistry::from_ron(include_str!("../../../../assets/data/blocks.ron"), &tiles).unwrap();
        let d = worldgen::Delves::load(include_str!("../../../../assets/data/dungeons.ron"), include_str!("../../../../assets/data/rooms.ron"), &reg).unwrap();
        for run in 1..6 {
            let plan = worldgen::FloorPlan { seed: 7, dungeon: 2, theme: 1, run, floor: 2, floors: 3, chain: true, done: false };
            let f = worldgen::delve::floor(&d, &plan);
            let spot = prison_spot(&f, false).expect("a fight room");
            let r = f.room_at(spot).expect("in a room");
            assert!(matches!(f.rooms[r].kind, worldgen::RoomType::Fight | worldgen::RoomType::Ambush));
            let boss = prison_spot(&f, true).unwrap();
            assert_eq!(f.room_at(boss), Some(f.boss_room));
        }
    }

}
