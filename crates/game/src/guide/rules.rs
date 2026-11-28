//! The guide's steps (`assets/data/guide.ron`) and what counts as a proper home.
//! Pure: no Bevy systems.

use std::collections::BTreeSet;

use serde::Deserialize;
use voxel::room::Room;
use voxel::BlockPos;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Goal {
    /// In your pockets: (item, count); "a|b" counts any of them.
    Have(Vec<(String, u32)>),
    /// Your bedroll is laid out.
    Camp,
    /// An urn step is done.
    Step(String),
    /// Made at a bench.
    Made(String),
    /// You stand in a proper home (see [`home_missing`]).
    Home,
    /// You have an urn besides the ancestral one: carried or set down.
    Urn,
    /// You have set one down.
    UrnPlaced,
    /// You have slept (Z by your bed).
    Slept,
    /// A story flag is set (a cutscene's `Flag` step, ...).
    Flag(String),
}

/// A waypoint to the nearest of some blocks, until you have enough of an item.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct MarkDef {
    pub name: String,
    /// Block names; a trailing `*` matches every name starting with what comes before it.
    pub blocks: Vec<String>,
    pub until: (String, u32),
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct StepDef {
    pub id: String,
    pub title: String,
    pub text: String,
    pub goal: Goal,
    #[serde(default)]
    pub marks: Vec<MarkDef>,
    #[serde(default)]
    pub cutscene: Option<String>,
    /// A waypoint to a named place (`cutscene::Places`: "gate", "sporrow", ...) while the step is under way.
    #[serde(default)]
    pub place: Option<String>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct GuideData {
    pub steps: Vec<StepDef>,
}

pub fn load(text: &str) -> Result<GuideData, String> {
    let ron = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let data: GuideData = ron.from_str(text).map_err(|e| e.to_string())?;
    let mut seen = BTreeSet::new();
    for s in &data.steps {
        if !seen.insert(s.id.clone()) {
            return Err(format!("step `{}` twice", s.id));
        }
    }
    Ok(data)
}

/// The story flag set when a step is done.
pub fn flag(id: &str) -> String {
    format!("guide:{id}")
}

/// The flag that starts the guide (set at the end of the opening).
pub const ON: &str = "guide_on";
/// Set once every step is done.
pub const DONE: &str = "guide_done";
/// The step that makes the camp mat: until it's done, the guide keeps you from sleeping.
pub const BED: &str = "bed";

/// Whether the guide still holds sleep back: it's under way and the bed isn't made yet.
pub fn bed_missing(flags: &BTreeSet<String>) -> bool {
    flags.contains(ON) && !flags.contains(DONE) && !flags.contains(&flag(BED))
}

/// The step under way: the first not done.
pub fn current<'a>(data: &'a GuideData, flags: &BTreeSet<String>) -> Option<(usize, &'a StepDef)> {
    data.steps.iter().enumerate().find(|(_, s)| !flags.contains(&flag(&s.id)))
}

/// Whether a block name is one a mark looks for.
pub fn matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => pattern == name,
    }
}

/// How many of "a|b|c" you have, given a count for one item name.
pub fn have(spec: &str, count: impl Fn(&str) -> u32) -> u32 {
    spec.split('|').map(|n| count(n.trim())).sum()
}

/// What the game knows for the goals.
#[derive(Default)]
pub struct Facts<'a> {
    pub count: Option<&'a dyn Fn(&str) -> u32>,
    pub camp: bool,
    pub steps: BTreeSet<String>,
    pub made: BTreeSet<String>,
    /// Standing in a closed room with nothing missing.
    pub home: bool,
    pub urn: bool,
    pub urn_placed: bool,
    pub slept: bool,
    pub flags: BTreeSet<String>,
}

pub fn met(goal: &Goal, f: &Facts) -> bool {
    let count = |n: &str| f.count.map_or(0, |c| c(n));
    match goal {
        Goal::Have(list) => list.iter().all(|(item, n)| have(item, count) >= *n),
        Goal::Camp => f.camp,
        Goal::Step(s) => f.steps.contains(s),
        Goal::Made(i) => f.made.contains(i),
        Goal::Home => f.home,
        Goal::Urn => f.urn || f.urn_placed,
        Goal::UrnPlaced => f.urn_placed,
        Goal::Slept => f.slept,
        Goal::Flag(flag) => f.flags.contains(flag),
    }
}

/// What a cell holds, for the home check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub door: bool,
    pub light: bool,
}

/// What a closed room still needs to be a proper home: a door in its walls and a
/// light (torch, campfire) inside. Empty = it's a home.
pub fn home_missing(room: &Room, cell: impl Fn(BlockPos) -> Cell) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if !room.shell.iter().any(|p| cell(*p).door) {
        missing.push("a door");
    }
    if !room.cells.iter().any(|p| cell(*p).light) {
        missing.push("a torch");
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxel::room::{find_room, Cell as RoomCell};

    #[test]
    fn day_one_walks_from_sticks_to_the_first_sleep_and_the_gate() {
        let data = load(include_str!("../../../../assets/data/guide.ron")).expect("guide.ron");
        let first = &data.steps[0];
        assert!(matches!(&first.goal, Goal::Have(l) if l.iter().any(|(i, _)| i == "stick") && l.iter().any(|(i, _)| i == "small_rock")));
        assert!(first.marks.iter().any(|m| m.blocks.iter().any(|b| matches(b, "twig_north"))), "points to twigs");
        assert!(first.marks.iter().any(|m| m.blocks.iter().any(|b| matches(b, "pebble"))), "points to pebbles");
        let ids: Vec<_> = data.steps.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["gather", "fiber", "camp", BED, "axe", "fire", "wood", "block_crafter", "slats", "sleep", "gate"]);
        let at = |id: &str| &data.steps[ids.iter().position(|i| *i == id).unwrap()];
        assert_eq!(at("camp").goal, Goal::Camp);
        assert_eq!(at(BED).goal, Goal::Step("camp_mat".into()), "the urn weaves the camp mat");
        assert_eq!(at("fire").goal, Goal::Made("campfire".into()));
        assert_eq!(at("block_crafter").goal, Goal::Step("block_crafter".into()));
        assert_eq!(at("slats").goal, Goal::Made("wood_slats".into()));
        assert_eq!(at("sleep").goal, Goal::Slept, "the first night by the fire");
        assert_eq!(data.steps.last().unwrap().place.as_deref(), Some("gate"), "ends pointing at the Rotting Gate");
        assert!(!data.steps.iter().any(|s| s.goal == Goal::Home), "no home this early");
        // Every cutscene a step names exists.
        let scenes = include_str!("../../../../assets/data/cutscenes.ron");
        for s in &data.steps {
            if let Some(c) = &s.cutscene {
                assert!(scenes.contains(&format!("id: \"{c}\"")), "no cutscene {c}");
            }
        }
        // Steps go one at a time.
        let mut flags = BTreeSet::new();
        assert_eq!(current(&data, &flags).map(|(i, _)| i), Some(0));
        flags.insert(flag(&data.steps[0].id));
        assert_eq!(current(&data, &flags).map(|(i, _)| i), Some(1));
    }

    #[test]
    fn sleep_waits_for_the_bed_while_the_guide_runs() {
        let mut flags = BTreeSet::new();
        assert!(!bed_missing(&flags), "old worlds without the guide sleep as before");
        flags.insert(ON.to_string());
        assert!(bed_missing(&flags));
        flags.insert(flag(BED));
        assert!(!bed_missing(&flags));
        let done: BTreeSet<String> = [ON, DONE].iter().map(|s| s.to_string()).collect();
        assert!(!bed_missing(&done));
    }

    #[test]
    fn goals() {
        let count = |n: &str| match n {
            "stick" => 5,
            "swamp_log" => 3,
            "log" => 2,
            _ => 0,
        };
        let f = Facts { count: Some(&count), ..Default::default() };
        assert!(met(&Goal::Have(vec![("stick".into(), 5)]), &f));
        assert!(!met(&Goal::Have(vec![("stick".into(), 5), ("small_rock".into(), 1)]), &f));
        assert!(met(&Goal::Have(vec![("log|swamp_log".into(), 4)]), &f), "logs of any kind add up");
        assert!(!met(&Goal::UrnPlaced, &f));
        let f = Facts { urn_placed: true, ..Default::default() };
        assert!(met(&Goal::Urn, &f) && met(&Goal::UrnPlaced, &f));
        assert!(!met(&Goal::Slept, &f));
        assert!(met(&Goal::Slept, &Facts { slept: true, ..Default::default() }));
        assert!(!met(&Goal::Flag("met_bram".into()), &f));
        assert!(met(&Goal::Flag("met_bram".into()), &Facts { flags: ["met_bram".to_string()].into(), ..Default::default() }));
        assert!(matches("twig_*", "twig_east") && !matches("twig_*", "log") && matches("pebble", "pebble"));
    }

    /// A 3x3x3 hollow box: a home once it has a door in a wall and a torch inside.
    #[test]
    fn a_home_needs_a_door_and_a_torch() {
        let wall = |p: BlockPos| p.x.abs() == 2 || p.y.abs() == 2 || p.z.abs() == 2;
        let room = find_room(BlockPos::new(0, 0, 0), |p| if p.x.abs() > 3 || p.y.abs() > 3 || p.z.abs() > 3 { RoomCell::Unknown } else if wall(p) { RoomCell::Wall } else { RoomCell::Open }).unwrap();
        let door = BlockPos::new(2, 0, 0);
        let torch = BlockPos::new(1, -1, 1);
        let bare = |_| Cell::default();
        assert_eq!(home_missing(&room, bare), vec!["a door", "a torch"]);
        let with_door = |p: BlockPos| Cell { door: p == door, light: false };
        assert_eq!(home_missing(&room, with_door), vec!["a torch"]);
        let both = |p: BlockPos| Cell { door: p == door, light: p == torch };
        assert!(home_missing(&room, both).is_empty());
        // A torch outside the walls doesn't count.
        let outside = |p: BlockPos| Cell { door: p == door, light: p == BlockPos::new(3, 0, 0) };
        assert_eq!(home_missing(&room, outside), vec!["a torch"]);
    }
}
