//! Run rules for dungeons (pure, no Bevy).
//!
//! A run starts when a spirit walks through a dungeon's door. It begins on the
//! first floor whose boss you haven't beaten (floor 1 again once the whole
//! dungeon is cleared: its last boss and prize chest never come back, but the
//! rest does). A floor's boss falling opens its ways on: the stair down, or the
//! way out back to the entrance with everything you found. The last floor's
//! boss clears the dungeon; a story dungeon's boss drops its urn the first
//! time, and its passage leads out near the next one. Falling below costs
//! everything you picked up during the run (what you brought in stays).
//!
//! Inside a floor, fight rooms lock (`Lock`): walking in shuts the doors of the
//! room (or its whole sealed section) until every foe there is down, wave by
//! wave. Boons (`boons.ron`) gathered from rooms stack until you leave.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// What the world remembers of one dungeon.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remembered {
    /// The deepest floor whose boss has fallen.
    pub cleared: u8,
    /// Its last boss has fallen: it's cleared for good.
    pub done: bool,
}

/// What the world remembers of every dungeon (saved to `delve.ron`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Delve {
    /// Runs started so far (each run's floors are laid out from its number).
    pub runs: u32,
    /// By dungeon id.
    #[serde(default)]
    pub dungeons: BTreeMap<u64, Remembered>,
    /// Story dungeons whose boss has dropped its urn (once per world).
    #[serde(default)]
    pub urns: BTreeSet<u8>,
    /// Old saves (the Rotting Gate on its own): the deepest depth beaten.
    #[serde(default, skip_serializing)]
    cleared: u8,
}

/// The urn (items.ron name) each story dungeon's last boss drops the first time it falls.
pub fn urn_for(chain: u8) -> Option<&'static str> {
    match chain {
        1 => Some("carpentry_urn"),
        2 => Some("gear_urn"),
        3 => Some("hearth_urn"),
        4 => Some("vehicle_urn"),
        _ => None,
    }
}

/// The boss of a floor (foes.ron name): the theme's warden on every floor but
/// the last, the theme's own boss there.
pub fn boss_for<'a>(warden: &'a str, boss: &'a str, floor: u8, floors: u8) -> &'a str {
    if floor >= floors { boss } else { warden }
}

/// The run being played.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Run {
    pub number: u32,
    pub dungeon: u64,
    /// Its place in the story, if any.
    pub chain: Option<u8>,
    pub floor: u8,
    pub floors: u8,
    /// This floor's boss is down (its ways out are open).
    pub boss_down: bool,
    /// The boss's key has opened the stair down.
    pub unlocked: bool,
}

/// What happened at the stair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stair {
    /// The boss still stands: it's sealed.
    Sealed,
    /// Down to this floor.
    Down(u8),
}

impl Delve {
    /// Bring an old save's Rotting Gate progress over (once).
    pub fn upgrade(&mut self, gate: u64, floors: u8) {
        if self.cleared > 0 {
            let r = self.dungeons.entry(gate).or_default();
            r.cleared = r.cleared.max(self.cleared.min(floors));
            r.done |= self.cleared >= floors;
            self.cleared = 0;
        }
    }

    pub fn of(&self, dungeon: u64) -> Remembered {
        self.dungeons.get(&dungeon).copied().unwrap_or_default()
    }

    /// The floor a new run of this dungeon starts on.
    pub fn start_floor(&self, dungeon: u64, floors: u8) -> u8 {
        let r = self.of(dungeon);
        if r.done {
            1
        } else {
            (r.cleared + 1).clamp(1, floors.max(1))
        }
    }

    /// Start a run.
    pub fn begin(&mut self, dungeon: u64, chain: Option<u8>, floors: u8) -> Run {
        self.runs += 1;
        let floor = self.start_floor(dungeon, floors);
        Run {
            number: self.runs,
            dungeon,
            chain,
            floor,
            floors,
            boss_down: false,
            unlocked: false,
        }
    }

    /// The floor's boss fell (or there was none: a cleared dungeon's last floor).
    /// True when this cleared the whole dungeon for the first time.
    pub fn boss_down(&mut self, run: &mut Run) -> bool {
        run.boss_down = true;
        let r = self.dungeons.entry(run.dungeon).or_default();
        r.cleared = r.cleared.max(run.floor);
        let first = run.floor >= run.floors && !r.done;
        r.done |= run.floor >= run.floors;
        first
    }

    /// The urn a story dungeon's boss drops, the first time only.
    pub fn claim_urn(&mut self, chain: u8) -> Option<&'static str> {
        let urn = urn_for(chain)?;
        self.urns.insert(chain).then_some(urn)
    }

    /// Step onto the stair.
    pub fn stair(&mut self, run: &mut Run) -> Stair {
        if !run.boss_down || run.floor >= run.floors {
            return Stair::Sealed;
        }
        run.floor += 1;
        run.boss_down = false;
        run.unlocked = false;
        Stair::Down(run.floor)
    }

    /// Is this dungeon cleared for good?
    pub fn done(&self, dungeon: u64) -> bool {
        self.of(dungeon).done
    }

    /// The furthest story dungeon cleared (0 = none), given their ids by place.
    pub fn chain_cleared(&self, ids: &[(u8, u64)]) -> u8 {
        ids.iter()
            .filter(|(_, id)| self.done(*id))
            .map(|(n, _)| *n)
            .max()
            .unwrap_or(0)
    }
}

/// How many of each thing to take away when you fall below: whatever you
/// carry now beyond what you carried in.
pub fn lost_on_death<K: Ord + Clone>(
    brought: &BTreeMap<K, u32>,
    now: &BTreeMap<K, u32>,
) -> BTreeMap<K, u32> {
    now.iter()
        .filter_map(|(k, &n)| {
            let had = brought.get(k).copied().unwrap_or(0);
            (n > had).then(|| (k.clone(), n - had))
        })
        .collect()
}

/// A room (or sealed section) you are locked into.
#[derive(Debug, Clone, PartialEq)]
pub struct Lock {
    /// The rooms locked together.
    pub rooms: Vec<usize>,
    /// The wave under way (0 first) and how many there are.
    pub wave: u8,
    pub waves: u8,
}

/// What the lock does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStep {
    /// Foes still stand: stay locked.
    Hold,
    /// This wave is down: bring on the next.
    Wave(u8),
    /// All down: the doors open.
    Open,
}

impl Lock {
    pub fn new(rooms: Vec<usize>, waves: u8) -> Self {
        Self {
            rooms,
            wave: 0,
            waves: waves.max(1),
        }
    }

    /// `left`: foes of this lock still standing.
    pub fn step(&mut self, left: usize) -> LockStep {
        if left > 0 {
            return LockStep::Hold;
        }
        if self.wave + 1 < self.waves {
            self.wave += 1;
            return LockStep::Wave(self.wave);
        }
        LockStep::Open
    }
}

/// Should walking into `room` lock it? Only rooms that lock, that still have
/// foes to come, and not already cleared.
pub fn locks(kind: worldgen::RoomType, has_foes: bool, cleared: bool) -> bool {
    kind.locks() && has_foes && !cleared
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen::RoomType;

    #[test]
    fn runs_start_on_the_first_floor_until_a_boss_falls() {
        let mut d = Delve::default();
        let mut r = d.begin(1, Some(1), 2);
        assert_eq!((r.number, r.floor), (1, 1));
        assert_eq!(
            d.stair(&mut r),
            Stair::Sealed,
            "sealed while the boss stands"
        );
        let r2 = d.begin(1, Some(1), 2);
        assert_eq!(
            (r2.number, r2.floor),
            (2, 1),
            "leaving before the boss changes nothing"
        );
        assert!(!d.boss_down(&mut r));
        assert_eq!(
            d.start_floor(1, 2),
            2,
            "the next run may start a floor down"
        );
        r.unlocked = true;
        assert_eq!(d.stair(&mut r), Stair::Down(2));
        assert!(
            !r.boss_down && !r.unlocked,
            "a new floor, a new boss and key"
        );
        assert!(d.boss_down(&mut r), "the last boss clears it");
        assert!(d.done(1));
        assert_eq!(
            d.stair(&mut r),
            Stair::Sealed,
            "no stair past the last floor"
        );
        assert_eq!(
            d.start_floor(1, 2),
            1,
            "a cleared dungeon starts from the top again"
        );
        assert!(!d.boss_down(&mut r), "only the first time counts");
        // Other dungeons are their own.
        assert_eq!(d.start_floor(9, 3), 1);
    }

    #[test]
    fn story_dungeons_drop_their_urns_once() {
        let mut d = Delve::default();
        assert_eq!(d.claim_urn(1), Some("carpentry_urn"));
        assert_eq!(d.claim_urn(1), None);
        assert_eq!(d.claim_urn(2), Some("gear_urn"));
        assert_eq!(d.claim_urn(3), Some("hearth_urn"));
        assert_eq!(d.claim_urn(4), Some("vehicle_urn"));
        assert_eq!(
            d.claim_urn(5),
            None,
            "the star urn waits on the castle's tower"
        );
        let ids = [(1, 10), (2, 20), (3, 30)];
        assert_eq!(d.chain_cleared(&ids), 0);
        let mut r = d.begin(20, Some(2), 1);
        d.boss_down(&mut r);
        assert_eq!(d.chain_cleared(&ids), 2);
    }

    #[test]
    fn bosses_by_floor() {
        assert_eq!(boss_for("bog_warden", "rotting_king", 1, 2), "bog_warden");
        assert_eq!(boss_for("bog_warden", "rotting_king", 2, 2), "rotting_king");
    }

    #[test]
    fn dying_costs_only_what_you_picked_up() {
        let brought = BTreeMap::from([("sword", 1), ("bread", 4), ("coin", 2)]);
        let now = BTreeMap::from([("sword", 1), ("bread", 2), ("coin", 7), ("dagger", 1)]);
        assert_eq!(
            lost_on_death(&brought, &now),
            BTreeMap::from([("coin", 5), ("dagger", 1)])
        );
    }

    #[test]
    fn saves_round_trip_and_old_ones_carry_over() {
        let mut d = Delve {
            runs: 3,
            ..Default::default()
        };
        d.dungeons.insert(
            1,
            Remembered {
                cleared: 1,
                done: false,
            },
        );
        let back: Delve = ron::from_str(&ron::to_string(&d).unwrap()).unwrap();
        assert_eq!(back, d);
        let mut old: Delve =
            ron::from_str("(runs: 3, deepest: 4, cleared: 4, urns: [1, 2])").unwrap();
        old.upgrade(1, 2);
        assert!(old.done(1));
        assert_eq!(old.urns, BTreeSet::from([1, 2]));
    }

    #[test]
    fn locks_hold_through_every_wave() {
        let mut l = Lock::new(vec![3, 4], 3);
        assert_eq!(l.step(2), LockStep::Hold);
        assert_eq!(l.step(0), LockStep::Wave(1));
        assert_eq!(l.step(1), LockStep::Hold);
        assert_eq!(l.step(0), LockStep::Wave(2));
        assert_eq!(l.step(0), LockStep::Open);
        assert!(locks(RoomType::Fight, true, false));
        assert!(
            !locks(RoomType::Fight, true, true),
            "cleared rooms stay open"
        );
        for k in [
            RoomType::Rest,
            RoomType::Treasure,
            RoomType::Puzzle,
            RoomType::Shop,
        ] {
            assert!(!locks(k, true, false), "{k:?} never locks");
        }
    }
}
