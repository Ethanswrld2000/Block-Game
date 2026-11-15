//! Urn rules, no Bevy: every station grows around its own urn, and each urn
//! has a tree of steps you fill with items (`assets/data/urns.ron`). A step
//! takes its items a few at a time; when it holds all of them it is done and
//! gives a station, an upgrade, recipes or another urn.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What finishing a step gives.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum Gives {
    /// Places this station by the urn.
    Station(String),
    /// Improves a station or the camp; code asks `has(step)`.
    Upgrade,
    /// Teaches the bench recipes whose `unlock` names this step.
    Recipes,
    /// Gives you this urn to carry and set down.
    Urn(String),
}

impl Gives {
    pub fn label(&self) -> &'static str {
        match self {
            Gives::Station(_) => "Station",
            Gives::Upgrade => "Upgrade",
            Gives::Recipes => "Recipes",
            Gives::Urn(_) => "Urn",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub name: String,
    pub title: String,
    pub tier: u8,
    pub gives: Gives,
    pub needs: BTreeMap<String, u32>,
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub desc: String,
    /// What it does isn't in the game yet.
    #[serde(default)]
    pub later: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UrnDef {
    pub name: String,
    pub title: String,
    pub station: String,
    #[serde(default)]
    pub item: Option<String>,
    pub color: (f32, f32, f32),
    #[serde(default)]
    pub how: String,
    pub steps: Vec<Step>,
}

impl UrnDef {
    pub fn step(&self, name: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.name == name)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UrnData {
    pub urns: Vec<UrnDef>,
}

impl UrnData {
    /// Parse and check: step names unique, `after` within the same urn, `Urn`
    /// gifts name an urn that has an item. `known_item` says whether an item exists.
    pub fn from_ron(text: &str, known_item: impl Fn(&str) -> bool) -> Result<Self, String> {
        let d: UrnData = ron::from_str(text).map_err(|e| format!("urns.ron: {e}"))?;
        let mut seen = std::collections::BTreeSet::new();
        for u in &d.urns {
            if let Some(it) = &u.item
                && !known_item(it)
            {
                return Err(format!("urns.ron: urn '{}' is carried as unknown item '{it}'", u.name));
            }
            for s in &u.steps {
                if !seen.insert(s.name.clone()) {
                    return Err(format!("urns.ron: step '{}' is named twice", s.name));
                }
                for a in &s.after {
                    if u.step(a).is_none() {
                        return Err(format!("urns.ron: step '{}' comes after '{a}', which isn't in the {}", s.name, u.title));
                    }
                }
                for it in s.needs.keys() {
                    if !known_item(it) {
                        return Err(format!("urns.ron: step '{}' needs unknown item '{it}'", s.name));
                    }
                }
                if s.needs.is_empty() || s.needs.values().any(|n| *n == 0) {
                    return Err(format!("urns.ron: step '{}' needs nothing", s.name));
                }
                if let Gives::Urn(other) = &s.gives
                    && !d.urns.iter().any(|x| &x.name == other && x.item.is_some())
                {
                    return Err(format!("urns.ron: step '{}' gives urn '{other}', which has no item to carry", s.name));
                }
            }
        }
        Ok(d)
    }

    pub fn urn(&self, name: &str) -> Option<&UrnDef> {
        self.urns.iter().find(|u| u.name == name)
    }

    /// The urn carried as this item.
    pub fn urn_of_item(&self, item: &str) -> Option<&UrnDef> {
        self.urns.iter().find(|u| u.item.as_deref() == Some(item))
    }

    /// A step and the urn it belongs to.
    pub fn step(&self, name: &str) -> Option<(&UrnDef, &Step)> {
        self.urns.iter().find_map(|u| u.step(name).map(|s| (u, s)))
    }
}

/// Where a step stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Done,
    /// Its `after` steps are done: it takes items.
    Open,
    /// Waiting on another step.
    Locked,
}

/// What one urn holds and has finished. Saved with the world.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// Steps done, in the order they finished.
    pub done: Vec<String>,
    /// Items put in so far, by step.
    pub given: BTreeMap<String, BTreeMap<String, u32>>,
}

impl Progress {
    pub fn is_done(&self, step: &str) -> bool {
        self.done.iter().any(|d| d == step)
    }

    pub fn state(&self, step: &Step) -> State {
        if self.is_done(&step.name) {
            State::Done
        } else if step.after.iter().all(|a| self.is_done(a)) {
            State::Open
        } else {
            State::Locked
        }
    }

    /// How many of an item a step holds so far.
    pub fn given(&self, step: &str, item: &str) -> u32 {
        self.given.get(step).and_then(|g| g.get(item)).copied().unwrap_or(0)
    }

    /// How many more of each item a step still needs.
    pub fn missing(&self, step: &Step) -> BTreeMap<String, u32> {
        if self.is_done(&step.name) {
            return BTreeMap::new();
        }
        step.needs.iter().map(|(it, n)| (it.clone(), n.saturating_sub(self.given(&step.name, it)))).filter(|(_, n)| *n > 0).collect()
    }

    /// How many more of this item the open steps could take.
    pub fn wants(&self, def: &UrnDef, item: &str) -> u32 {
        def.steps.iter().filter(|s| self.state(s) == State::Open).map(|s| self.missing(s).get(item).copied().unwrap_or(0)).sum()
    }

    /// Put up to `count` of an item in. It goes to the open steps in tree order
    /// (only to `only` if given), so nothing is wasted: a step that finishes
    /// may open the next, which takes the rest. Returns how many went in and
    /// the steps finished, in order.
    pub fn offer(&mut self, def: &UrnDef, item: &str, count: u32, only: Option<&str>) -> (u32, Vec<String>) {
        let mut left = count;
        let mut finished = Vec::new();
        loop {
            let mut moved = false;
            for s in &def.steps {
                if left == 0 {
                    break;
                }
                if only.is_some_and(|o| o != s.name) || self.state(s) != State::Open {
                    continue;
                }
                let want = self.missing(s).get(item).copied().unwrap_or(0);
                if want == 0 {
                    continue;
                }
                let n = want.min(left);
                *self.given.entry(s.name.clone()).or_default().entry(item.to_string()).or_insert(0) += n;
                left -= n;
                moved = true;
                if self.missing(s).is_empty() {
                    self.finish(&s.name);
                    finished.push(s.name.clone());
                }
            }
            if !moved || left == 0 {
                break;
            }
        }
        (count - left, finished)
    }

    /// Mark a step done (also the dev menu's way to skip ahead).
    pub fn finish(&mut self, step: &str) {
        if !self.is_done(step) {
            self.done.push(step.to_string());
        }
        self.given.remove(step);
    }

    /// The first open step in tree order.
    pub fn next<'a>(&self, def: &'a UrnDef) -> Option<&'a Step> {
        def.steps.iter().find(|s| self.state(s) == State::Open)
    }

    /// How full the next open step is, 0-1 (by item count).
    pub fn next_fill(&self, def: &UrnDef) -> f32 {
        let Some(s) = self.next(def) else { return 1.0 };
        let total: u32 = s.needs.values().sum();
        let have: u32 = s.needs.iter().map(|(it, n)| self.given(&s.name, it).min(*n)).sum();
        have as f32 / total.max(1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> UrnData {
        UrnData::from_ron(include_str!("../../../../assets/data/urns.ron"), |_| true).unwrap()
    }

    #[test]
    fn the_data_file_loads() {
        let d = data();
        assert_eq!(d.urns[0].name, "ancestral");
        assert!(d.urns[0].item.is_none(), "the ancestral urn comes with the bedroll");
        for u in &d.urns[1..] {
            let giver = d.urns.iter().flat_map(|x| &x.steps).find(|s| s.gives == Gives::Urn(u.name.clone()));
            assert!(giver.is_some(), "nothing gives the {}", u.title);
        }
    }

    #[test]
    fn items_fill_open_steps_and_finish_them() {
        let d = data();
        let anc = d.urn("ancestral").unwrap();
        let mut p = Progress::default();
        assert_eq!(p.next(anc).unwrap().name, "camp_mat");
        // The tool urn is locked: sticks only go to the camp mat.
        let (n, done) = p.offer(anc, "stick", 10, None);
        assert_eq!((n, done.len()), (4, 0), "the mat takes 4 sticks, the rest stay with you");
        let (n, done) = p.offer(anc, "plant_fiber", 9, None);
        assert_eq!(n, 4);
        assert_eq!(done, vec!["camp_mat".to_string()]);
        assert!(p.is_done("camp_mat"));
        assert_eq!(p.state(anc.step("tool_urn").unwrap()), State::Open);
        assert!(p.given.get("camp_mat").is_none(), "a finished step forgets its items");
    }

    #[test]
    fn a_finished_step_passes_the_rest_on() {
        let d = data();
        let anc = d.urn("ancestral").unwrap();
        let mut p = Progress::default();
        p.offer(anc, "plant_fiber", 4, None);
        // 4 sticks finish the mat, which opens the tool urn: it takes 6 more.
        let (n, done) = p.offer(anc, "stick", 12, None);
        assert_eq!(n, 10);
        assert_eq!(done, vec!["camp_mat".to_string()]);
        assert_eq!(p.given("tool_urn", "stick"), 6);
        assert_eq!(p.wants(anc, "stick"), 0);
    }

    #[test]
    fn only_puts_into_one_step() {
        let d = data();
        let tool = d.urn("tool").unwrap();
        let mut p = Progress::default();
        p.finish("tool_bench");
        // Planks... no: cordage fits only the improved bench; flint fits the
        // carpentry bench (2) and the improved bench (3).
        assert_eq!(p.wants(tool, "flint"), 5);
        let (n, _) = p.offer(tool, "flint", 5, Some("improved_tool_bench"));
        assert_eq!(n, 3);
        assert_eq!(p.given("carpentry_bench", "flint"), 0);
    }

    #[test]
    fn bad_data_is_caught() {
        let bad = r#"(urns: [(name: "a", title: "A", station: "S", color: (1.0, 1.0, 1.0), steps: [
            (name: "x", title: "X", tier: 1, gives: Upgrade, needs: { "stick": 1 }, after: ["nope"]),
        ])])"#;
        assert!(UrnData::from_ron(bad, |_| true).is_err());
        let unknown = r#"(urns: [(name: "a", title: "A", station: "S", color: (1.0, 1.0, 1.0), steps: [
            (name: "x", title: "X", tier: 1, gives: Upgrade, needs: { "unobtainium": 1 }),
        ])])"#;
        assert!(UrnData::from_ron(unknown, |i| i != "unobtainium").is_err());
    }

    #[test]
    fn next_fill_counts_items() {
        let d = data();
        let anc = d.urn("ancestral").unwrap();
        let mut p = Progress::default();
        assert_eq!(p.next_fill(anc), 0.0);
        p.offer(anc, "stick", 4, None);
        assert!((p.next_fill(anc) - 0.5).abs() < 1e-6);
    }
}
