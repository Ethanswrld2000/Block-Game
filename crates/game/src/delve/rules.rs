//! Run rules for the Rotting Gate (pure, no Bevy).
//!
//! A run starts when you walk into the gate. It begins at the deepest depth
//! you have unlocked: clearing a depth's boss unlocks starting the next run
//! from the depth below it. The boss's fall also opens the stair down. Dying
//! below costs you everything you picked up during the run (what you brought
//! in stays with you) and you wake outside the gate.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub const DEPTHS: u8 = worldgen::DEPTHS;

/// What the world remembers of the gate (saved to `delve.ron`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Delve {
    /// Runs started so far (each run's floors are laid out from its number).
    pub runs: u32,
    /// The deepest depth ever reached.
    pub deepest: u8,
    /// The deepest depth whose boss has been beaten.
    pub cleared: u8,
    /// Depths whose boss has already dropped its urn (once per world).
    #[serde(default)]
    pub urns: BTreeSet<u8>,
}

/// Each depth's boss (foes.ron name).
pub fn boss_for(depth: u8) -> &'static str {
    match depth {
        0 | 1 => "bog_warden",
        2 => "spore_matron",
        3 => "gloam_stalker",
        _ => "rotting_king",
    }
}

/// The urn (items.ron name) each depth's boss drops the first time it falls.
pub fn urn_for(depth: u8) -> Option<&'static str> {
    match depth {
        1 => Some("carpentry_urn"),
        2 => Some("gear_urn"),
        3 => Some("hearth_urn"),
        4 => Some("vehicle_urn"),
        _ => None,
    }
}

/// The run being played.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Run {
    pub number: u32,
    pub depth: u8,
    /// This floor's boss is down (the stair is open).
    pub boss_down: bool,
}

/// What happened at the stair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stair {
    /// The boss still stands: the stair is sealed.
    Sealed,
    /// Down to this depth.
    Down(u8),
    /// That was the last depth: the run is done.
    Done,
}

impl Delve {
    /// The depth a new run starts at.
    pub fn start_depth(&self) -> u8 {
        (self.cleared + 1).clamp(1, DEPTHS)
    }

    /// Start a run.
    pub fn begin(&mut self) -> Run {
        self.runs += 1;
        let depth = self.start_depth();
        self.deepest = self.deepest.max(depth);
        Run { number: self.runs, depth, boss_down: false }
    }

    /// The floor's boss fell.
    pub fn boss_down(&mut self, run: &mut Run) {
        run.boss_down = true;
        self.cleared = self.cleared.max(run.depth);
    }

    /// The urn this depth's boss drops, the first time only.
    pub fn claim_urn(&mut self, depth: u8) -> Option<&'static str> {
        let urn = urn_for(depth)?;
        self.urns.insert(depth).then_some(urn)
    }

    /// Step onto the stair.
    pub fn stair(&mut self, run: &mut Run) -> Stair {
        if !run.boss_down {
            return Stair::Sealed;
        }
        if run.depth >= DEPTHS {
            return Stair::Done;
        }
        run.depth += 1;
        run.boss_down = false;
        self.deepest = self.deepest.max(run.depth);
        Stair::Down(run.depth)
    }
}

/// How many of each thing to take away when you die below: whatever you
/// carry now beyond what you carried in.
pub fn lost_on_death<K: Ord + Clone>(brought: &BTreeMap<K, u32>, now: &BTreeMap<K, u32>) -> BTreeMap<K, u32> {
    now.iter()
        .filter_map(|(k, &n)| {
            let had = brought.get(k).copied().unwrap_or(0);
            (n > had).then(|| (k.clone(), n - had))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_start_at_the_first_depth_until_a_boss_falls() {
        let mut d = Delve::default();
        let mut r = d.begin();
        assert_eq!((r.number, r.depth), (1, 1));
        assert_eq!(d.stair(&mut r), Stair::Sealed, "sealed while the boss stands");
        // Dying or leaving before the boss changes nothing.
        let r2 = d.begin();
        assert_eq!((r2.number, r2.depth), (2, 1));
        d.boss_down(&mut r);
        assert_eq!(d.cleared, 1);
        assert_eq!(d.start_depth(), 2, "the next run may start a depth down");
        assert_eq!(d.stair(&mut r), Stair::Down(2));
        assert_eq!(d.deepest, 2);
        assert!(!r.boss_down, "a new floor, a new boss");
        assert_eq!(d.stair(&mut r), Stair::Sealed);
    }

    #[test]
    fn the_last_depth_ends_the_run() {
        let mut d = Delve { runs: 5, deepest: 4, cleared: 3, ..Default::default() };
        let mut r = d.begin();
        assert_eq!(r.depth, 4);
        d.boss_down(&mut r);
        assert_eq!(d.stair(&mut r), Stair::Done);
        assert_eq!(d.start_depth(), 4, "never past the deepest depth");
    }

    #[test]
    fn dying_costs_only_what_you_picked_up() {
        let brought = BTreeMap::from([("sword", 1), ("bread", 4), ("coin", 2)]);
        // Ate two bread, found 5 coins and a dagger.
        let now = BTreeMap::from([("sword", 1), ("bread", 2), ("coin", 7), ("dagger", 1)]);
        let lost = lost_on_death(&brought, &now);
        assert_eq!(lost, BTreeMap::from([("coin", 5), ("dagger", 1)]));
    }

    #[test]
    fn saves_round_trip() {
        let d = Delve { runs: 3, deepest: 2, cleared: 1, urns: BTreeSet::from([1]) };
        let back: Delve = ron::from_str(&ron::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
        // Saves from before the urn drops still load.
        let old: Delve = ron::from_str("(runs: 3, deepest: 2, cleared: 1)").unwrap();
        assert!(old.urns.is_empty());
    }

    #[test]
    fn each_boss_drops_its_urn_once_per_world() {
        let mut d = Delve::default();
        assert_eq!(d.claim_urn(1), Some("carpentry_urn"));
        assert_eq!(d.claim_urn(1), None, "only the first time");
        assert_eq!(d.claim_urn(2), Some("gear_urn"));
        assert_eq!(d.claim_urn(3), Some("hearth_urn"));
        assert_eq!(d.claim_urn(4), Some("vehicle_urn"));
        assert_eq!(d.claim_urn(4), None);
        let names: BTreeSet<_> = (1..=DEPTHS).map(boss_for).collect();
        assert_eq!(names.len(), DEPTHS as usize, "one boss per depth");
    }
}
