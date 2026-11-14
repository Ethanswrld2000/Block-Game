//! Working a bench by hand: what lies in each slot of its board, and the
//! recipes (`assets/data/benches.ron`) that turn it into something new.
//!
//! Pure rules; `board` puts them on screen. Things are moved one at a time:
//! put into a slot, picked back out, then the bench's tool (the tool bench's
//! strike stone) is worked a few times to make what the slots match.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::inventory::grid::Stack;
use crate::items::Items;

#[derive(Deserialize)]
struct BenchesSpec {
    recipes: Vec<RecipeSpec>,
}

#[derive(Deserialize)]
struct RecipeSpec {
    station: String,
    slots: BTreeMap<String, (String, u16)>,
    work: u8,
    makes: (String, u16),
    #[serde(default)]
    unlock: String,
}

/// One recipe: what each slot must hold, how many times to work the tool, and what it makes.
#[derive(Debug, Clone)]
pub struct Recipe {
    pub station: String,
    pub slots: BTreeMap<String, Stack>,
    pub work: u8,
    pub makes: Stack,
    /// The urn step that teaches it (none: known from the start).
    pub unlock: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BenchData {
    pub recipes: Vec<Recipe>,
    /// Urn steps done that teach recipes; the game keeps this up to date.
    pub learned: std::collections::BTreeSet<String>,
}

/// A slot on a bench's board: its name and how many things fit in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDef {
    pub name: &'static str,
    pub cap: u16,
}

/// The tool bench: a long groove for the handle and a hollow for the material.
pub const TOOL_BENCH_SLOTS: [SlotDef; 2] = [SlotDef { name: "handle", cap: 1 }, SlotDef { name: "material", cap: 4 }];

/// The carpentry bench: a bed for logs and boards, and a vise.
pub const CARPENTRY_SLOTS: [SlotDef; 2] = [SlotDef { name: "bed", cap: 8 }, SlotDef { name: "vise", cap: 4 }];

/// The camp mat: what lies on the mat, and what binds it.
pub const CAMP_MAT_SLOTS: [SlotDef; 2] = [SlotDef { name: "base", cap: 8 }, SlotDef { name: "tie", cap: 8 }];

/// The slots of a bench, by its station name in urns.ron.
pub fn slots_of(station: &str) -> &'static [SlotDef] {
    match station {
        "camp_mat" => &CAMP_MAT_SLOTS,
        "tool_bench" => &TOOL_BENCH_SLOTS,
        "carpentry_bench" => &CARPENTRY_SLOTS,
        _ => &[],
    }
}

impl BenchData {
    pub fn from_ron(text: &str, items: &Items) -> Result<Self, String> {
        let spec: BenchesSpec = ron::from_str(text).map_err(|e| format!("benches.ron: {e}"))?;
        let item = |n: &str| items.id(n).ok_or_else(|| format!("benches.ron: unknown item '{n}'"));
        let mut recipes = Vec::new();
        for r in spec.recipes {
            let defs = slots_of(&r.station);
            if defs.is_empty() {
                return Err(format!("benches.ron: '{}' has no board", r.station));
            }
            let mut slots = BTreeMap::new();
            for (name, (it, n)) in &r.slots {
                let Some(def) = defs.iter().find(|d| d.name == name) else {
                    return Err(format!("benches.ron: the {} has no '{name}' slot", r.station));
                };
                if *n == 0 || *n > def.cap {
                    return Err(format!("benches.ron: {n} {it} don't fit the {name} slot (it holds {})", def.cap));
                }
                slots.insert(name.clone(), Stack::new(item(it)?, *n));
            }
            recipes.push(Recipe { station: r.station, slots, work: r.work.max(1), makes: Stack::new(item(&r.makes.0)?, r.makes.1.max(1)), unlock: (!r.unlock.is_empty()).then_some(r.unlock) });
        }
        Ok(Self { recipes, learned: Default::default() })
    }

    /// The recipe the board matches: every slot it names holds that item (at
    /// least that many), and nothing else is on the board. If several match
    /// (2 or 3 cobblestone), the one that uses the most.
    pub fn matching(&self, station: &str, board: &Board) -> Option<&Recipe> {
        self.recipes
            .iter()
            .filter(|r| r.station == station && self.knows(r))
            .filter(|r| {
                r.slots.iter().all(|(s, want)| board.slots.get(s).is_some_and(|have| have.item == want.item && have.count >= want.count))
                    && board.slots.keys().all(|s| r.slots.contains_key(s))
            })
            .max_by_key(|r| r.slots.values().map(|s| s.count as u32).sum::<u32>())
    }

    /// The recipes a bench knows now.
    pub fn for_station<'a>(&'a self, station: &'a str) -> impl Iterator<Item = &'a Recipe> + 'a {
        self.recipes.iter().filter(move |r| r.station == station && self.knows(r))
    }

    /// Whether the urn has taught this recipe (or it needs no teaching).
    pub fn knows(&self, r: &Recipe) -> bool {
        r.unlock.as_ref().is_none_or(|u| self.learned.contains(u))
    }
}

/// What working the bench's tool did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Work {
    /// The slots don't make anything (or the made thing is still lying there).
    Nothing,
    /// One more knock: `done` of `of`.
    Knock { done: u8, of: u8 },
    /// The last knock: the slots were used up and this lies on the board.
    Made(Stack),
}

/// What lies on a bench's board.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Board {
    pub slots: BTreeMap<String, Stack>,
    /// Made and not picked up yet.
    pub made: Option<Stack>,
    /// Knocks so far towards the matching recipe.
    pub work: u8,
}

impl Board {
    /// Put what's in hand into a slot. Returns what's in hand afterwards: the
    /// rest if the slot is full, what was in the slot if it held something else.
    pub fn put(&mut self, slot: SlotDef, held: Stack) -> Option<Stack> {
        self.work = 0;
        match self.slots.get_mut(slot.name) {
            None => {
                let n = held.count.min(slot.cap);
                self.slots.insert(slot.name.to_string(), Stack::new(held.item, n));
                (held.count > n).then(|| Stack::new(held.item, held.count - n))
            }
            Some(have) if have.item == held.item => {
                let n = held.count.min(slot.cap - have.count);
                have.count += n;
                (held.count > n).then(|| Stack::new(held.item, held.count - n))
            }
            Some(have) if held.count <= slot.cap => Some(std::mem::replace(have, held)),
            Some(_) => Some(held),
        }
    }

    /// Pick everything up out of a slot.
    pub fn take(&mut self, slot: &str) -> Option<Stack> {
        let s = self.slots.remove(slot);
        if s.is_some() {
            self.work = 0;
        }
        s
    }

    /// Work the bench's tool once.
    pub fn strike(&mut self, data: &BenchData, station: &str) -> Work {
        if self.made.is_some() {
            return Work::Nothing;
        }
        let Some(r) = data.matching(station, self).cloned() else {
            self.work = 0;
            return Work::Nothing;
        };
        self.work += 1;
        if self.work < r.work {
            return Work::Knock { done: self.work, of: r.work };
        }
        self.work = 0;
        for (s, want) in &r.slots {
            if let Some(have) = self.slots.get_mut(s) {
                have.count -= want.count;
                if have.count == 0 {
                    self.slots.remove(s);
                }
            }
        }
        self.made = Some(r.makes);
        Work::Made(r.makes)
    }

    /// Everything on the board, to go back into the pockets.
    pub fn clear(&mut self) -> Vec<Stack> {
        self.work = 0;
        let mut out: Vec<Stack> = std::mem::take(&mut self.slots).into_values().collect();
        out.extend(self.made.take());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> (Items, BenchData) {
        let (_, items) = crate::items::tests::load();
        let data = BenchData::from_ron(include_str!("../../../../assets/data/benches.ron"), &items).unwrap();
        (items, data)
    }

    #[test]
    fn a_stick_and_two_rocks_make_the_first_axe() {
        let (items, data) = data();
        let id = |n| items.id(n).unwrap();
        let [base, tie] = CAMP_MAT_SLOTS;
        let mut b = Board::default();
        assert_eq!(b.put(tie, Stack::new(id("stick"), 1)), None);
        assert_eq!(b.put(base, Stack::new(id("small_rock"), 1)), None);
        // One rock isn't enough.
        assert_eq!(b.strike(&data, "camp_mat"), Work::Nothing);
        assert_eq!(b.put(base, Stack::new(id("small_rock"), 1)), None);
        let r = data.matching("camp_mat", &b).unwrap();
        for k in 1..r.work {
            assert_eq!(b.strike(&data, "camp_mat"), Work::Knock { done: k, of: r.work });
        }
        let made = Stack::new(id("rough_axe"), 1);
        assert_eq!(b.strike(&data, "camp_mat"), Work::Made(made));
        assert!(b.slots.is_empty());
        // It lies there until picked up; working again does nothing.
        assert_eq!(b.strike(&data, "camp_mat"), Work::Nothing);
        assert_eq!(b.clear(), vec![made]);
    }

    #[test]
    fn the_most_that_fits_wins() {
        let (items, mut data) = data();
        data.learned.insert("improved_tool_bench".into());
        let id = |n| items.id(n).unwrap();
        let [handle, material] = TOOL_BENCH_SLOTS;
        let mut b = Board::default();
        b.put(handle, Stack::new(id("stick"), 1));
        b.put(material, Stack::new(id("cobblestone"), 2));
        assert_eq!(data.matching("tool_bench", &b).unwrap().makes.item, id("stone_axe"));
        b.put(material, Stack::new(id("cobblestone"), 1));
        assert_eq!(data.matching("tool_bench", &b).unwrap().makes.item, id("stone_pickaxe"));
    }

    #[test]
    fn locked_recipes_wait_for_the_urn() {
        let (items, mut data) = data();
        let id = |n| items.id(n).unwrap();
        let [handle, material] = TOOL_BENCH_SLOTS;
        let mut b = Board::default();
        b.put(handle, Stack::new(id("stick"), 1));
        b.put(material, Stack::new(id("flint"), 2));
        assert!(data.matching("tool_bench", &b).is_none(), "the spear needs the improved tool bench");
        assert!(!data.for_station("tool_bench").any(|r| r.makes.item == id("flint_spear")));
        data.learned.insert("improved_tool_bench".into());
        assert_eq!(data.matching("tool_bench", &b).unwrap().makes.item, id("flint_spear"));
    }

    #[test]
    fn logs_saw_into_planks() {
        let (items, data) = data();
        let id = |n| items.id(n).unwrap();
        let [bed, _] = CARPENTRY_SLOTS;
        let mut b = Board::default();
        b.put(bed, Stack::new(id("log"), 1));
        let made = loop {
            if let Work::Made(s) = b.strike(&data, "carpentry_bench") {
                break s;
            }
        };
        assert_eq!(made, Stack::new(id("planks"), 4));
    }

    #[test]
    fn slots_fill_to_their_size_and_swap() {
        let (items, _) = data();
        let id = |n| items.id(n).unwrap();
        let [handle, material] = TOOL_BENCH_SLOTS;
        let mut b = Board::default();
        // The groove takes one stick; the rest stays in hand.
        assert_eq!(b.put(handle, Stack::new(id("stick"), 3)), Some(Stack::new(id("stick"), 2)));
        assert_eq!(b.put(material, Stack::new(id("small_rock"), 6)), Some(Stack::new(id("small_rock"), 2)));
        // Something else swaps with what's there.
        assert_eq!(b.put(handle, Stack::new(id("small_rock"), 1)), Some(Stack::new(id("stick"), 1)));
        assert_eq!(b.take("handle"), Some(Stack::new(id("small_rock"), 1)));
        assert_eq!(b.take("handle"), None);
    }

    #[test]
    fn leftovers_stay_and_strays_spoil_the_recipe() {
        let (items, data) = data();
        let id = |n| items.id(n).unwrap();
        let [handle, material] = TOOL_BENCH_SLOTS;
        let mut b = Board::default();
        b.put(handle, Stack::new(id("small_rock"), 1));
        b.put(material, Stack::new(id("small_rock"), 3));
        assert!(data.matching("tool_bench", &b).is_none());
        b.put(handle, Stack::new(id("stick"), 1));
        b.put(material, Stack::new(id("small_rock"), 1));
        while !matches!(b.strike(&data, "tool_bench"), Work::Made(_)) {}
        assert_eq!(b.slots.get("material"), Some(&Stack::new(id("small_rock"), 1)));
    }
}
