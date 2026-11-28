//! Region goals: a short list of things to do in each region, in order (swamp,
//! forests, desert, snowy mountains, deep caves, sky islands), from
//! `assets/data/journey.ron`. Pure rules in [`rules`].
//!
//! Mining, killing creatures, making things at a bench, where you stand and the
//! places you find all count. Each goal gives XP once. When every goal of the
//! current region is done, the next region's goals come up and a waypoint is set
//! at the nearest ground of that region (found on the `AsyncComputeTaskPool`).
//! The list shows beside the map (M). Saved to `journey.ron`.

pub mod rules;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use worldgen::{Land, WorldGen};

use crate::blocks::Blocks;
use crate::map::{GoalsSlot, Waypoint, Waypoints};
use crate::player::Player;
use crate::save::{SaveDir, read_ron, screenshot_run, write_ron};
use crate::survival::Toast;
use crate::theme::{Theme, colors, sizes, text};
use crate::world::Generator;
use rules::{Event, Journey, JourneyData, Target};

const FILE: &str = "journey.ron";
/// How far out the waypoint search looks, and its step (blocks).
const SEARCH_REACH: i32 = 2000;
const SEARCH_STEP: i32 = 24;

pub struct JourneyPlugin;

impl Plugin for JourneyPlugin {
    fn build(&self, app: &mut App) {
        let data = JourneyData::parse(include_str!("../../../../assets/data/journey.ron"))
            .unwrap_or_else(|e| panic!("journey.ron: {e}"));
        app.insert_resource(Goals(data))
            .init_resource::<Journey>()
            .add_message::<CreatureSlain>()
            .add_systems(crate::menu::GamePostStartup, load)
            .add_systems(
                crate::menu::GameUpdate,
                (count, point, draw).chain().after(crate::player::PlayerSet),
            )
            .add_systems(crate::menu::GameUpdate, save_now_and_then)
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
    }
}

/// The goal list from journey.ron.
#[derive(Resource)]
pub struct Goals(pub JourneyData);

/// A wild creature was killed by the player (its kind's name).
#[derive(Message, Debug, Clone)]
pub struct CreatureSlain(pub String);

fn load(dir: Res<SaveDir>, mut journey: ResMut<Journey>) {
    if screenshot_run() {
        return;
    }
    if let Some(j) = read_ron(&dir, FILE) {
        *journey = j;
    }
}

fn save_now_and_then(
    time: Res<Time<Real>>,
    mut since: Local<f32>,
    dir: Res<SaveDir>,
    journey: Res<Journey>,
) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    write_ron(&dir, FILE, &*journey);
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, journey: Res<Journey>) {
    if exit.read().next().is_some() && !screenshot_run() {
        write_ron(&dir, FILE, &*journey);
    }
}

/// Counts what happened towards the goals; XP and a toast per finished goal and region.
#[allow(clippy::too_many_arguments)]
fn count(
    goals: Res<Goals>,
    mut journey: ResMut<Journey>,
    blocks: Res<Blocks>,
    generator: Option<Res<Generator>>,
    player: Res<Player>,
    progress: Res<crate::progress::Progress>,
    time: Res<Time>,
    mut tick: Local<f32>,
    (mut mined, mut slain, mut crafted): (
        MessageReader<crate::interact::Mined>,
        MessageReader<CreatureSlain>,
        MessageReader<crate::station::board::Crafted>,
    ),
    (mut gain, mut toast): (MessageWriter<crate::progress::GainXp>, MessageWriter<Toast>),
) {
    let data = &goals.0;
    let mut finished = Vec::new();
    // Only touch the resource when something counts, so `draw` redraws only then.
    let record = |j: &mut ResMut<Journey>, ev: Event| {
        let mut probe = j.as_ref().clone();
        let f = probe.record(data, ev);
        if probe != **j {
            **j = probe;
        }
        f
    };
    for m in mined.read() {
        finished.extend(record(
            &mut journey,
            Event::Mined(&blocks.registry.get(m.block).name),
        ));
    }
    for s in slain.read() {
        finished.extend(record(&mut journey, Event::Slain(&s.0)));
    }
    for c in crafted.read() {
        finished.extend(record(&mut journey, Event::Made(&c.0)));
    }
    *tick -= time.delta_secs();
    if *tick <= 0.0
        && let Some(generator) = generator
        && player.ready
    {
        *tick = 1.0;
        let p = player.pos.floor().as_ivec3();
        let biome = &generator.0.biome_at(p.x, p.y, p.z).name;
        finished.extend(record(
            &mut journey,
            Event::At {
                biome,
                y: player.pos.y,
                sky: progress.sky,
            },
        ));
        for key in &progress.found {
            finished.extend(record(&mut journey, Event::Found(key)));
        }
    }
    for (r, g) in finished {
        let goal = &data.regions[r].goals[g];
        gain.write(crate::progress::GainXp {
            xp: goal.xp,
            why: goal.text.clone(),
        });
        toast.write(Toast(format!("Goal done: {}", goal.text)));
    }
    let before = journey.region;
    if !journey
        .current(data)
        .is_some_and(|r| journey.region_done(r))
    {
        return;
    }
    journey.advance(data);
    let done = &data.regions[before].title;
    toast.write(Toast(match journey.current(data) {
        Some(next) => format!("{done} goals done. Next: {}", next.title),
        None => format!("{done} goals done. Every region's goals are done."),
    }));
}

/// The waypoint search for the current region, if one is running.
#[derive(Default)]
struct Search(Option<(String, Task<Option<Vec3>>)>);

/// Sets a waypoint at the nearest ground of the current region, once per region.
fn point(
    goals: Res<Goals>,
    mut journey: ResMut<Journey>,
    generator: Option<Res<Generator>>,
    player: Res<Player>,
    mut waypoints: ResMut<Waypoints>,
    mut toast: MessageWriter<Toast>,
    mut search: Local<Search>,
) {
    if let Some((name, task)) = &mut search.0 {
        let Some(found) = check_ready(task) else {
            return;
        };
        let name = name.clone();
        search.0 = None;
        journey.pointed.insert(name.clone());
        let Some(region) = goals.0.regions.iter().find(|r| r.name == name) else {
            return;
        };
        match found {
            Some(at) => {
                waypoints.list.push(Waypoint {
                    name: region.title.clone(),
                    pos: at.to_array(),
                });
                toast.write(Toast(format!("Waypoint set: {}", region.title)));
            }
            None => info!("journey: no {} within {SEARCH_REACH} blocks", region.title),
        }
        return;
    }
    let Some(region) = journey.current(&goals.0) else {
        return;
    };
    if region.target == Target::None || journey.pointed.contains(&region.name) || !player.ready {
        return;
    }
    let Some(generator) = generator else { return };
    let (wg, target, from) = (generator.0.clone(), region.target.clone(), player.pos);
    let task = AsyncComputeTaskPool::get().spawn(async move { nearest(&wg, &target, from) });
    search.0 = Some((region.name.clone(), task));
}

/// The nearest spot of `target` round `from`, searched ring by ring outward.
pub fn nearest(wg: &WorldGen, target: &Target, from: Vec3) -> Option<Vec3> {
    let ids: Vec<usize> = match target {
        Target::None => return None,
        Target::Biomes(names) => (0..wg.biomes.len())
            .filter(|&i| names.contains(&wg.biomes.get(i).name))
            .collect(),
        Target::SkyIsland => Vec::new(),
    };
    let (cx, cz) = (
        (from.x / SEARCH_STEP as f32).round() as i32,
        (from.z / SEARCH_STEP as f32).round() as i32,
    );
    for ring in 0..=SEARCH_REACH / SEARCH_STEP {
        let mut best: Option<(f32, Vec3)> = None;
        for i in -ring..=ring {
            for (dx, dz) in [(i, -ring), (i, ring), (-ring, i), (ring, i)] {
                let (x, z) = ((cx + dx) * SEARCH_STEP, (cz + dz) * SEARCH_STEP);
                let at = match target {
                    Target::SkyIsland => wg
                        .island_top(x, z)
                        .map(|top| Vec3::new(x as f32, top as f32 + 1.0, z as f32)),
                    _ => {
                        let c = wg.column(x, z);
                        (c.land == Land::Main && c.height >= c.water && ids.contains(&c.biome))
                            .then(|| Vec3::new(x as f32, c.height as f32 + 1.0, z as f32))
                    }
                };
                if let Some(at) = at {
                    let d = Vec2::new(at.x - from.x, at.z - from.z).length();
                    if best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, at));
                    }
                }
            }
        }
        if let Some((_, at)) = best {
            return Some(at);
        }
    }
    None
}

/// Fills the goal list beside the map when the journey changes.
fn draw(
    mut commands: Commands,
    goals: Res<Goals>,
    journey: Res<Journey>,
    theme: Res<Theme>,
    slot: Query<Entity, With<GoalsSlot>>,
    added: Query<(), Added<GoalsSlot>>,
) {
    let Ok(slot) = slot.single() else { return };
    if !journey.is_changed() && added.is_empty() {
        return;
    }
    commands.entity(slot).despawn_related::<Children>();
    commands.entity(slot).with_children(|p| {
        p.spawn((
            theme.bold(sizes::HEADING),
            Text::new("Goals"),
            TextColor(colors::HEADING),
        ));
        let Some(region) = journey.current(&goals.0) else {
            p.spawn(text(
                "Every region's goals are done.",
                sizes::SMALL,
                colors::MUTED,
            ));
            return;
        };
        p.spawn(text(region.title.clone(), sizes::LABEL, colors::GOLD));
        p.spawn((
            text(region.hint.clone(), sizes::SMALL, colors::MUTED),
            Node {
                margin: UiRect::bottom(Val::Px(6.0)),
                ..default()
            },
        ));
        for goal in &region.goals {
            let (n, of) = journey.progress(goal);
            let done = n >= of;
            let line = match (done, of) {
                (true, _) => format!("[x] {}", goal.text),
                (false, 1) => format!("[ ] {}", goal.text),
                (false, _) => format!("[ ] {} ({n}/{of})", goal.text),
            };
            p.spawn(text(
                line,
                sizes::SMALL,
                if done { colors::GOOD } else { colors::TEXT },
            ));
        }
        let left = goals.0.regions.len() - journey.region - 1;
        if left > 0 {
            p.spawn((
                text(
                    format!(
                        "{left} more region{} after this",
                        if left == 1 { "" } else { "s" }
                    ),
                    sizes::SMALL,
                    colors::FAINT,
                ),
                Node {
                    margin: UiRect::top(Val::Px(6.0)),
                    ..default()
                },
            ));
        }
    });
}
