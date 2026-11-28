//! Pure rules for region goals: the data in `assets/data/journey.ron` and what
//! the player has done towards it.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// What a goal asks for.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum When {
    /// Break any of these blocks (`count` times).
    Mine(Vec<String>),
    /// Kill a creature of this kind (`count` times).
    Slay(String),
    /// Make this item at a bench (`count` times).
    Make(String),
    /// Stand in any of these biomes.
    Reach(Vec<String>),
    /// Find a place whose key starts with this (`barrow`, `dungeon`).
    Find(String),
    /// Go below this height.
    Below(f32),
    /// Stand on a sky island.
    Sky,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Goal {
    pub key: String,
    pub text: String,
    pub when: When,
    #[serde(default = "one")]
    pub count: u32,
    pub xp: f32,
}

fn one() -> u32 {
    1
}

/// Where the waypoint to a region points.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub enum Target {
    /// No waypoint (the region is everywhere, like the deep caves).
    #[default]
    None,
    /// The nearest main-island ground in any of these biomes.
    Biomes(Vec<String>),
    /// The nearest sky island.
    SkyIsland,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Region {
    pub name: String,
    pub title: String,
    /// One line on what this region asks of you.
    pub hint: String,
    #[serde(default)]
    pub target: Target,
    pub goals: Vec<Goal>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JourneyData {
    pub regions: Vec<Region>,
}

impl JourneyData {
    pub fn parse(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }
}

/// Something that happened that a goal may count.
#[derive(Debug, Clone, Copy)]
pub enum Event<'a> {
    Mined(&'a str),
    Slain(&'a str),
    Made(&'a str),
    /// Where you are now: biome name, height, on a sky island.
    At {
        biome: &'a str,
        y: f32,
        sky: bool,
    },
    /// A place you've found (its key, like `barrow:12`).
    Found(&'a str),
}

impl When {
    /// How much this event adds to a goal of this kind.
    fn counts(&self, ev: Event) -> bool {
        match (self, ev) {
            (When::Mine(blocks), Event::Mined(b)) => blocks.iter().any(|x| x == b),
            (When::Slay(kind), Event::Slain(k)) => kind == k,
            (When::Make(item), Event::Made(i)) => item == i,
            (When::Reach(biomes), Event::At { biome, .. }) => biomes.iter().any(|x| x == biome),
            (When::Below(h), Event::At { y, .. }) => y < *h,
            (When::Sky, Event::At { sky, .. }) => sky,
            (When::Find(prefix), Event::Found(key)) => {
                key.split(':').next() == Some(prefix.as_str())
            }
            _ => false,
        }
    }
}

/// What the player has done (saved to `journey.ron`).
#[derive(bevy::prelude::Resource, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Journey {
    /// Progress on goals not yet done, by key.
    pub counts: BTreeMap<String, u32>,
    pub done: BTreeSet<String>,
    /// The region being worked on (index into `JourneyData::regions`); past the end = all done.
    pub region: usize,
    /// Regions a waypoint has been set for.
    #[serde(default)]
    pub pointed: BTreeSet<String>,
}

impl Journey {
    /// Count an event towards every goal not yet done (any region, so nothing done early is lost).
    /// Returns the goals it finished, as (region, goal) indices.
    pub fn record(&mut self, data: &JourneyData, ev: Event) -> Vec<(usize, usize)> {
        let mut finished = Vec::new();
        for (r, region) in data.regions.iter().enumerate() {
            for (g, goal) in region.goals.iter().enumerate() {
                if self.done.contains(&goal.key) || !goal.when.counts(ev) {
                    continue;
                }
                let n = self.counts.entry(goal.key.clone()).or_insert(0);
                *n += 1;
                if *n >= goal.count {
                    self.counts.remove(&goal.key);
                    self.done.insert(goal.key.clone());
                    finished.push((r, g));
                }
            }
        }
        finished
    }

    /// How far along a goal is (done, out of).
    pub fn progress(&self, goal: &Goal) -> (u32, u32) {
        if self.done.contains(&goal.key) {
            (goal.count, goal.count)
        } else {
            (self.counts.get(&goal.key).copied().unwrap_or(0), goal.count)
        }
    }

    pub fn region_done(&self, region: &Region) -> bool {
        region.goals.iter().all(|g| self.done.contains(&g.key))
    }

    /// Moves on past every finished region. Returns the regions finished on the way.
    pub fn advance(&mut self, data: &JourneyData) -> Vec<usize> {
        let mut passed = Vec::new();
        while let Some(r) = data.regions.get(self.region)
            && self.region_done(r)
        {
            passed.push(self.region);
            self.region += 1;
        }
        passed
    }

    pub fn current<'a>(&self, data: &'a JourneyData) -> Option<&'a Region> {
        data.regions.get(self.region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> JourneyData {
        JourneyData::parse(include_str!("../../../../assets/data/journey.ron"))
            .expect("journey.ron")
    }

    #[test]
    fn data_parses_and_keys_are_unique() {
        let d = data();
        assert!(d.regions.len() >= 4);
        let mut keys = BTreeSet::new();
        for r in &d.regions {
            assert!(!r.goals.is_empty(), "{} has no goals", r.name);
            for g in &r.goals {
                assert!(keys.insert(g.key.clone()), "goal key {} twice", g.key);
                assert!(g.count >= 1 && g.xp > 0.0);
            }
        }
    }

    #[test]
    fn names_in_the_data_exist() {
        let d = data();
        let blocks = include_str!("../../../../assets/data/blocks.ron");
        let items = include_str!("../../../../assets/data/items.ron");
        let biomes = include_str!("../../../../assets/data/biomes.ron");
        let creatures = include_str!("../../../../assets/data/creatures.ron");
        let has = |text: &str, name: &str| text.contains(&format!("name: \"{name}\""));
        for r in &d.regions {
            if let Target::Biomes(list) = &r.target {
                for b in list {
                    assert!(has(biomes, b), "biome {b}");
                }
            }
            for g in &r.goals {
                match &g.when {
                    When::Mine(list) => list
                        .iter()
                        .for_each(|b| assert!(has(blocks, b), "block {b}")),
                    When::Slay(k) => assert!(has(creatures, k), "creature {k}"),
                    When::Make(i) => assert!(has(items, i), "item {i}"),
                    When::Reach(list) => list
                        .iter()
                        .for_each(|b| assert!(has(biomes, b), "biome {b}")),
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn counting_finishes_goals_and_regions_in_order() {
        let d = JourneyData::parse(
            r#"(regions: [
                (name: "a", title: "A", hint: "", goals: [
                    (key: "reeds", text: "", when: Mine(["reeds"]), count: 2, xp: 1.0),
                    (key: "boar", text: "", when: Slay("marsh_boar"), xp: 1.0),
                ]),
                (name: "b", title: "B", hint: "", target: Biomes(["dunes"]), goals: [
                    (key: "desert", text: "", when: Reach(["dunes"]), xp: 1.0),
                    (key: "deep", text: "", when: Below(-100.0), xp: 1.0),
                    (key: "barrow", text: "", when: Find("barrow"), xp: 1.0),
                ]),
            ])"#,
        )
        .unwrap();
        let mut j = Journey::default();
        // Something from a later region counts early.
        assert_eq!(
            j.record(
                &d,
                Event::At {
                    biome: "dunes",
                    y: 0.0,
                    sky: false
                }
            ),
            vec![(1, 0)]
        );
        assert!(j.record(&d, Event::Mined("reeds")).is_empty());
        assert_eq!(j.progress(&d.regions[0].goals[0]), (1, 2));
        assert!(j.advance(&d).is_empty());
        assert_eq!(j.record(&d, Event::Mined("reeds")), vec![(0, 0)]);
        // Done goals don't count again.
        assert!(j.record(&d, Event::Mined("reeds")).is_empty());
        j.record(&d, Event::Slain("marsh_boar"));
        assert_eq!(j.advance(&d), vec![0]);
        assert_eq!(j.current(&d).unwrap().name, "b");
        assert!(j.record(&d, Event::Found("dungeon:3")).is_empty());
        assert_eq!(j.record(&d, Event::Found("barrow:7")), vec![(1, 2)]);
        assert!(
            j.record(
                &d,
                Event::At {
                    biome: "x",
                    y: -50.0,
                    sky: false
                }
            )
            .is_empty()
        );
        j.record(
            &d,
            Event::At {
                biome: "x",
                y: -150.0,
                sky: false,
            },
        );
        assert_eq!(j.advance(&d), vec![1]);
        assert!(j.current(&d).is_none());
    }
}
