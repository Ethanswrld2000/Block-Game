//! The getting-started guide (`assets/data/guide.ron`, rules in `rules`): day one with
//! Bram. After the opening sets `guide_on`, one step at a time is under way: sticks and
//! stones, plant fiber, a bedroll laid out, the camp mat, a rough axe, a campfire, raw
//! wood, the block crafter and wood slats, the first night by the fire, and the way to
//! the Rotting Gate. Bram walks over and says some steps as they come up (their
//! cutscenes). The journal (J) lists them; finishing one ticks it off (story flag
//! `guide:<id>`), says the next in a toast, and points waypoints (`map::Waypoints::guide`)
//! at the nearest blocks the step asks for, found in the loaded world around you, or at
//! the step's `place`. In a `Home` step (none at the moment), standing in a closed room
//! tells you what it still lacks.

pub mod rules;

use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::Menus;
use crate::cutscene::{Playing, StartCutscene, Story};
use crate::inventory::Inventory;
use crate::items::Items;
use crate::map::{Waypoint, Waypoints};
use crate::player::Player;
use crate::survival::Toast;
use crate::world::World;
use rules::GuideData;

pub struct GuidePlugin;

impl Plugin for GuidePlugin {
    fn build(&self, app: &mut App) {
        let data = rules::load(include_str!("../../../../assets/data/guide.ron")).unwrap_or_else(|e| panic!("guide.ron: {e}"));
        app.insert_resource(GuideBook(data)).add_systems(crate::menu::GameUpdate, guide.after(crate::npc::talk::TalkSet));
    }
}

#[derive(Resource)]
pub struct GuideBook(pub GuideData);

/// How often the goals are checked (seconds).
const CHECK_EVERY: f32 = 0.5;
/// How often the waypoints look again for the nearest blocks (seconds).
const LOOK_EVERY: f32 = 2.0;
/// How far round you the waypoints look (blocks), first near, then further.
const SEARCH: [i32; 2] = [40, 96];
/// Up and down from your feet.
const SEARCH_UP: i32 = 24;
/// Seconds between "this room still needs..." toasts.
const HOME_TOAST_EVERY: f32 = 8.0;

/// The block nearest `from`, among `found`.
pub fn nearest(from: Vec3, found: impl IntoIterator<Item = BlockPos>) -> Option<BlockPos> {
    found.into_iter().min_by(|a, b| {
        let d = |p: &BlockPos| Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5).distance_squared(from);
        d(a).total_cmp(&d(b))
    })
}

#[derive(Default)]
struct State {
    check: f32,
    look: f32,
    home_toast: f32,
    /// The step whose cutscene was asked for.
    asked: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn guide(
    time: Res<Time<Real>>,
    book: Res<GuideBook>,
    mut story: ResMut<Story>,
    (player, menus, playing): (Res<Player>, Res<Menus>, Res<Playing>),
    (inv, items, world, blocks): (Res<Inventory>, Res<Items>, Res<World>, Res<Blocks>),
    (camp, urns, sleep): (Res<crate::survival::camp::Camp>, Res<crate::station::Urns>, Res<crate::survival::camp::Sleep>),
    places: Res<crate::cutscene::Places>,
    mut waypoints: ResMut<Waypoints>,
    (mut toast, mut start): (MessageWriter<Toast>, MessageWriter<StartCutscene>),
    mut st: Local<State>,
) {
    let dt = time.delta_secs();
    st.check -= dt;
    st.look -= dt;
    st.home_toast -= dt;
    let on = story.flags.contains(rules::ON) && !story.flags.contains(rules::DONE);
    if !on || !player.ready {
        if !waypoints.guide.is_empty() {
            waypoints.guide.clear();
        }
        return;
    }
    if st.check > 0.0 || playing.active() {
        return;
    }
    st.check = CHECK_EVERY;
    let Some((i, step)) = rules::current(&book.0, &story.flags) else {
        story.flags.insert(rules::DONE.to_string());
        return;
    };

    // Is it done?
    let count = |name: &str| items.id(name).map_or(0, |id| inv.count(id));
    let here = BlockPos::from_world((player.pos + Vec3::Y * 0.5).to_array());
    let mut missing = None;
    if step.goal == rules::Goal::Home {
        missing = crate::station::room_at(&world, &blocks, here).ok().map(|room| home_missing(&room, &world, &blocks));
    }
    let mut f = rules::Facts { count: Some(&count), camp: camp.bedroll.is_some(), home: missing.as_ref().is_some_and(|m| m.is_empty()), ..Default::default() };
    if let rules::Goal::Step(s) = &step.goal
        && urns.has(s)
    {
        f.steps.insert(s.clone());
    }
    f.made = story.made.clone();
    f.slept = sleep.active();
    if let rules::Goal::Flag(flag) = &step.goal
        && story.flags.contains(flag)
    {
        f.flags.insert(flag.clone());
    }
    f.urn_placed = urns.placed.keys().any(|k| k != "ancestral");
    f.urn = inv.pockets.items.iter().any(|p| items.get(p.stack.item).name.ends_with("_urn"));
    if rules::met(&step.goal, &f) {
        story.flags.insert(rules::flag(&step.id));
        waypoints.guide.clear();
        st.look = 0.0;
        match book.0.steps.get(i + 1) {
            Some(next) => {
                toast.write(Toast(format!("Done: {}. Next: {} (J for the journal)", step.title, next.title)));
            }
            None => {
                story.flags.insert(rules::DONE.to_string());
                toast.write(Toast(format!("Done: {}. You're on your own from here", step.title)));
            }
        }
        return;
    }

    // Its cutscene, once.
    if let Some(c) = &step.cutscene
        && !story.played.contains(c)
        && st.asked.as_deref() != Some(step.id.as_str())
        && !menus.open
    {
        st.asked = Some(step.id.clone());
        start.write(StartCutscene(c.clone()));
    }

    // In a closed room that isn't a home yet: say what it lacks.
    if let Some(m) = &missing
        && !m.is_empty()
        && st.home_toast <= 0.0
    {
        st.home_toast = HOME_TOAST_EVERY;
        toast.write(Toast(format!("This room holds, but it still needs {}", m.join(" and "))));
    }

    // Point at the nearest blocks it asks for.
    if st.look > 0.0 {
        return;
    }
    st.look = LOOK_EVERY;
    let mut marks = Vec::new();
    if let Some(p) = step.place.as_ref().and_then(|name| places.0.get(name)) {
        marks.push(Waypoint { name: step.title.clone(), pos: p.to_array() });
    }
    for m in &step.marks {
        if rules::have(&m.until.0, count) >= m.until.1 {
            continue;
        }
        let ids: Vec<_> = blocks.registry.iter().filter(|(_, d)| m.blocks.iter().any(|b| rules::matches(b, &d.name))).map(|(id, _)| id).collect();
        let found = SEARCH.iter().find_map(|r| {
            let (lo, hi) = (BlockPos::new(here.x - r, here.y - SEARCH_UP, here.z - r), BlockPos::new(here.x + r, here.y + SEARCH_UP, here.z + r));
            nearest(player.pos, ids.iter().flat_map(|id| world.find_blocks(lo, hi, *id)))
        });
        if let Some(p) = found {
            marks.push(Waypoint { name: m.name.clone(), pos: [p.x as f32 + 0.5, p.y as f32, p.z as f32 + 0.5] });
        }
    }
    if waypoints.guide != marks {
        waypoints.guide = marks;
    }
}

/// What a closed room still lacks to be a home (a door in its walls, a light inside).
pub fn home_missing(room: &voxel::room::Room, world: &World, blocks: &Blocks) -> Vec<&'static str> {
    rules::home_missing(room, |p| {
        world.block(p).map_or(rules::Cell::default(), |b| {
            let d = blocks.registry.get(b);
            rules::Cell { door: matches!(d.model, Some(voxel::Model::Door)), light: d.light > 0 }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_block_wins() {
        let from = Vec3::new(0.5, 10.0, 0.5);
        let found = [BlockPos::new(20, 10, 0), BlockPos::new(-3, 11, 2), BlockPos::new(0, 40, 0)];
        assert_eq!(nearest(from, found), Some(BlockPos::new(-3, 11, 2)));
        assert_eq!(nearest(from, []), None);
    }
}
