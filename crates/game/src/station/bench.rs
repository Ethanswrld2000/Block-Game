//! Working a bench by hand: what lies in each slot of its board, and the
//! recipes (`assets/data/benches.ron`) that turn it into something new.
//!
//! Pure rules; `board` puts them on screen. Things are moved one at a time:
//! put into a slot, picked back out, then the bench's tool (the tool bench's
//! strike stone, the pot's ladle, the cutting board's knife) is worked a few
//! times to make what the slots match.
//!
//! Most slots hold one named thing each. A cooking pot is different: whatever
//! goes into the pot mixes (`SlotKind::Mix`, any order, recipes list a `mix`),
//! and the fire under it burns fuel (`SlotKind::Fire`, recipes ask for `heat`,
//! each fuel is worth some heat in benches.ron `fuel`).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::inventory::grid::Stack;
use crate::items::{ItemId, Items};

#[derive(Deserialize)]
struct BenchesSpec {
    #[serde(default)]
    fuel: BTreeMap<String, u32>,
    recipes: Vec<RecipeSpec>,
}

#[derive(Deserialize)]
struct RecipeSpec {
    station: String,
    #[serde(default)]
    slots: BTreeMap<String, (String, u16)>,
    #[serde(default)]
    mix: Vec<(String, u16)>,
    #[serde(default)]
    heat: u32,
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
    /// What goes into the pot, in any order (stations with `Mix` slots).
    pub mix: Vec<Stack>,
    /// How much heat the fire needs (stations with a `Fire` slot).
    pub heat: u32,
    pub work: u8,
    pub makes: Stack,
    /// The urn step that teaches it (none: known from the start).
    pub unlock: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BenchData {
    pub recipes: Vec<Recipe>,
    /// What burns, and how much heat one of it gives.
    pub fuel: BTreeMap<ItemId, u32>,
    /// Urn steps done that teach recipes; the game keeps this up to date.
    pub learned: std::collections::BTreeSet<String>,
}

/// How a slot takes part in a recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// Holds the one thing a recipe names for it.
    Plain,
    /// One of several that together make up a pot: recipes list a `mix`, any order.
    Mix,
    /// The fire: holds fuel; recipes ask for `heat`.
    Fire,
}

/// A slot on a bench's board: its name and how many things fit in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotDef {
    pub name: &'static str,
    pub cap: u16,
    pub kind: SlotKind,
}

const fn plain(name: &'static str, cap: u16) -> SlotDef {
    SlotDef { name, cap, kind: SlotKind::Plain }
}

/// The tool bench: a long groove for the handle and a hollow for the material.
pub const TOOL_BENCH_SLOTS: [SlotDef; 2] = [plain("handle", 1), plain("material", 4)];

/// The carpentry bench: a bed for logs and boards, and a vise.
pub const CARPENTRY_SLOTS: [SlotDef; 2] = [plain("bed", 8), plain("vise", 4)];

/// The camp mat: what lies on the mat, and what binds it.
pub const CAMP_MAT_SLOTS: [SlotDef; 2] = [plain("base", 8), plain("tie", 8)];

/// The cooking pot: up to four kinds of things in the pot, and the fire under it.
pub const COOKING_POT_SLOTS: [SlotDef; 5] = [
    SlotDef { name: "pot_1", cap: 6, kind: SlotKind::Mix },
    SlotDef { name: "pot_2", cap: 6, kind: SlotKind::Mix },
    SlotDef { name: "pot_3", cap: 6, kind: SlotKind::Mix },
    SlotDef { name: "pot_4", cap: 6, kind: SlotKind::Mix },
    SlotDef { name: "fire", cap: 8, kind: SlotKind::Fire },
];

/// The cutting board: one thing on the board at a time, cut with the knife.
pub const CUTTING_BOARD_SLOTS: [SlotDef; 1] = [plain("board", 4)];

/// The slots of a bench, by its station name in urns.ron.
pub fn slots_of(station: &str) -> &'static [SlotDef] {
    match station {
        "camp_mat" => &CAMP_MAT_SLOTS,
        "tool_bench" => &TOOL_BENCH_SLOTS,
        "carpentry_bench" => &CARPENTRY_SLOTS,
        "cooking_pot" => &COOKING_POT_SLOTS,
        "cutting_board" => &CUTTING_BOARD_SLOTS,
        _ => &[],
    }
}

fn kind_of(station: &str, slot: &str) -> SlotKind {
    slots_of(station).iter().find(|d| d.name == slot).map_or(SlotKind::Plain, |d| d.kind)
}

/// How many of each thing lie in a station's mix slots, all slots together.
fn mixed(station: &str, board: &Board) -> BTreeMap<ItemId, u32> {
    let mut out = BTreeMap::new();
    for (s, st) in &board.slots {
        if kind_of(station, s) == SlotKind::Mix {
            *out.entry(st.item).or_insert(0) += st.count as u32;
        }
    }
    out
}

impl BenchData {
    pub fn from_ron(text: &str, items: &Items) -> Result<Self, String> {
        let spec: BenchesSpec = ron::from_str(text).map_err(|e| format!("benches.ron: {e}"))?;
        let item = |n: &str| items.id(n).ok_or_else(|| format!("benches.ron: unknown item '{n}'"));
        let mut fuel = BTreeMap::new();
        for (n, heat) in &spec.fuel {
            if *heat == 0 {
                return Err(format!("benches.ron: fuel '{n}' gives no heat"));
            }
            fuel.insert(item(n)?, *heat);
        }
        let mut recipes = Vec::new();
        for r in spec.recipes {
            let defs = slots_of(&r.station);
            if defs.is_empty() {
                return Err(format!("benches.ron: '{}' has no board", r.station));
            }
            let mut slots = BTreeMap::new();
            for (name, (it, n)) in &r.slots {
                let Some(def) = defs.iter().find(|d| d.name == name && d.kind == SlotKind::Plain) else {
                    return Err(format!("benches.ron: the {} has no '{name}' slot", r.station));
                };
                if *n == 0 || *n > def.cap {
                    return Err(format!("benches.ron: {n} {it} don't fit the {name} slot (it holds {})", def.cap));
                }
                slots.insert(name.clone(), Stack::new(item(it)?, *n));
            }
            let pots: Vec<&SlotDef> = defs.iter().filter(|d| d.kind == SlotKind::Mix).collect();
            if !r.mix.is_empty() && pots.is_empty() {
                return Err(format!("benches.ron: the {} has no pot to mix in", r.station));
            }
            if r.mix.len() > pots.len() {
                return Err(format!("benches.ron: the {} pot holds {} kinds of things, not {}", r.station, pots.len(), r.mix.len()));
            }
            let mut mix: Vec<Stack> = Vec::new();
            for (it, n) in &r.mix {
                let id = item(it)?;
                let cap = pots.iter().map(|d| d.cap).min().unwrap_or(0);
                if *n == 0 || *n > cap || mix.iter().any(|s| s.item == id) {
                    return Err(format!("benches.ron: {n} {it} in the {} (each kind once, up to {cap})", r.station));
                }
                mix.push(Stack::new(id, *n));
            }
            let fire = defs.iter().find(|d| d.kind == SlotKind::Fire);
            if r.heat > 0 {
                let Some(f) = fire else {
                    return Err(format!("benches.ron: the {} has no fire for heat", r.station));
                };
                let best = fuel.values().max().copied().unwrap_or(0) * f.cap as u32;
                if r.heat > best {
                    return Err(format!("benches.ron: heat {} is more than a full fire gives ({best})", r.heat));
                }
            }
            if slots.is_empty() && mix.is_empty() {
                return Err(format!("benches.ron: a {} recipe uses nothing", r.station));
            }
            recipes.push(Recipe {
                station: r.station,
                slots,
                mix,
                heat: r.heat,
                work: r.work.max(1),
                makes: Stack::new(item(&r.makes.0)?, r.makes.1.max(1)),
                unlock: (!r.unlock.is_empty()).then_some(r.unlock),
            });
        }
        Ok(Self { recipes, fuel, learned: Default::default() })
    }

    /// The heat the fire slot's fuel gives.
    pub fn heat_of(&self, station: &str, board: &Board) -> u32 {
        board.slots.iter().filter(|(s, _)| kind_of(station, s) == SlotKind::Fire).map(|(_, f)| self.fuel.get(&f.item).copied().unwrap_or(0) * f.count as u32).sum()
    }

    /// How many of the fuel in the fire a recipe burns.
    fn burns(&self, station: &str, r: &Recipe, board: &Board) -> u16 {
        let per = board.slots.iter().find(|(s, _)| kind_of(station, s) == SlotKind::Fire).and_then(|(_, f)| self.fuel.get(&f.item)).copied().unwrap_or(1);
        r.heat.div_ceil(per) as u16
    }

    /// Whether the board holds what a recipe needs and nothing else (the fire
    /// may hold more than it needs; `heat: false` leaves the fire out).
    fn fits(&self, station: &str, r: &Recipe, board: &Board, heat: bool) -> bool {
        let plain_ok = r.slots.iter().all(|(s, want)| board.slots.get(s).is_some_and(|have| have.item == want.item && have.count >= want.count))
            && board.slots.keys().all(|s| kind_of(station, s) != SlotKind::Plain || r.slots.contains_key(s));
        let pot = mixed(station, board);
        let mix_ok = pot.len() == r.mix.len() && r.mix.iter().all(|want| pot.get(&want.item).is_some_and(|n| *n >= want.count as u32));
        let fire_ok = !heat || r.heat == 0 || self.heat_of(station, board) >= r.heat;
        plain_ok && mix_ok && fire_ok
    }

    /// The recipe the board matches: every slot it names holds that item (at
    /// least that many), and nothing else is on the board. If several match
    /// (2 or 3 cobblestone), the one that uses the most.
    pub fn matching(&self, station: &str, board: &Board) -> Option<&Recipe> {
        self.recipes
            .iter()
            .filter(|r| r.station == station && self.knows(r) && self.fits(station, r, board, true))
            .max_by_key(|r| r.slots.values().chain(&r.mix).map(|s| s.count as u32).sum::<u32>())
    }

    /// A recipe the board would make if only the fire were hotter: it, and the heat there is.
    pub fn needs_heat(&self, station: &str, board: &Board) -> Option<(&Recipe, u32)> {
        if self.matching(station, board).is_some() {
            return None;
        }
        let r = self
            .recipes
            .iter()
            .filter(|r| r.station == station && self.knows(r) && r.heat > 0 && self.fits(station, r, board, false))
            .max_by_key(|r| r.slots.values().chain(&r.mix).map(|s| s.count as u32).sum::<u32>())?;
        Some((r, self.heat_of(station, board)))
    }

    /// Whether this burns in a fire.
    pub fn is_fuel(&self, item: ItemId) -> bool {
        self.fuel.contains_key(&item)
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

    /// Put what's in hand into the pot: onto the same thing if it's there,
    /// else into an empty part of it. Returns what's left in hand.
    pub fn put_mix(&mut self, defs: &[SlotDef], held: Stack) -> Option<Stack> {
        let pots: Vec<SlotDef> = defs.iter().filter(|d| d.kind == SlotKind::Mix).copied().collect();
        let mut rest = Some(held);
        let same = pots.iter().filter(|d| self.slots.get(d.name).is_some_and(|s| s.item == held.item));
        let empty = pots.iter().filter(|d| !self.slots.contains_key(d.name));
        let order: Vec<SlotDef> = same.chain(empty).copied().collect();
        for d in order {
            let Some(h) = rest else { break };
            if self.slots.get(d.name).is_some_and(|s| s.item != h.item) {
                continue;
            }
            rest = self.put(d, h);
        }
        rest
    }

    /// Pick the last kind of thing back out of the pot.
    pub fn take_mix(&mut self, defs: &[SlotDef]) -> Option<Stack> {
        let d = defs.iter().rev().find(|d| d.kind == SlotKind::Mix && self.slots.contains_key(d.name))?;
        self.take(d.name)
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
        let burn = data.burns(station, &r, self);
        for (s, want) in &r.slots {
            self.use_up(s, want.count);
        }
        for want in &r.mix {
            let mut left = want.count;
            for d in slots_of(station).iter().filter(|d| d.kind == SlotKind::Mix) {
                if left > 0 && self.slots.get(d.name).is_some_and(|s| s.item == want.item) {
                    let n = left.min(self.slots[d.name].count);
                    self.use_up(d.name, n);
                    left -= n;
                }
            }
        }
        if r.heat > 0
            && let Some(d) = slots_of(station).iter().find(|d| d.kind == SlotKind::Fire)
        {
            self.use_up(d.name, burn);
        }
        self.made = Some(r.makes);
        Work::Made(r.makes)
    }

    fn use_up(&mut self, slot: &str, n: u16) {
        if let Some(have) = self.slots.get_mut(slot) {
            have.count = have.count.saturating_sub(n);
            if have.count == 0 {
                self.slots.remove(slot);
            }
        }
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

    fn cook(b: &mut Board, data: &BenchData, station: &str) -> Stack {
        for _ in 0..20 {
            if let Work::Made(s) = b.strike(data, station) {
                return s;
            }
        }
        panic!("nothing cooked: {b:?}");
    }

    #[test]
    fn the_pot_mixes_in_any_order_and_burns_only_what_it_needs() {
        let (items, data) = data();
        let id = |n| items.id(n).unwrap();
        let fire = COOKING_POT_SLOTS[4];
        let mut b = Board::default();
        // Berries first, then strips, then reeds: order doesn't matter.
        assert_eq!(b.put_mix(&COOKING_POT_SLOTS, Stack::new(id("marsh_berries"), 1)), None);
        assert_eq!(b.put_mix(&COOKING_POT_SLOTS, Stack::new(id("meat_strips"), 3)), None);
        assert_eq!(b.put_mix(&COOKING_POT_SLOTS, Stack::new(id("chopped_reeds"), 2)), None);
        // No fire yet: stirring does nothing, and says why.
        assert_eq!(b.strike(&data, "cooking_pot"), Work::Nothing);
        let (r, have) = data.needs_heat("cooking_pot", &b).unwrap();
        assert_eq!((r.makes.item, have), (id("bog_stew"), 0));
        // Two sticks aren't enough (3 heat); a peat is.
        b.put(fire, Stack::new(id("stick"), 2));
        assert!(data.matching("cooking_pot", &b).is_none());
        assert_eq!(data.needs_heat("cooking_pot", &b).unwrap().1, 2);
        assert_eq!(b.put(fire, Stack::new(id("peat"), 2)), Some(Stack::new(id("stick"), 2)), "peat swaps the sticks out");
        assert!(data.matching("cooking_pot", &b).is_some());
        assert_eq!(cook(&mut b, &data, "cooking_pot"), Stack::new(id("bog_stew"), 2));
        // The pot is empty; one peat (3 heat) of the two burned.
        assert!(b.slots.keys().all(|k| k == "fire"));
        assert_eq!(b.slots.get("fire"), Some(&Stack::new(id("peat"), 1)));
    }

    #[test]
    fn strays_in_the_pot_spoil_it() {
        let (items, data) = data();
        let id = |n| items.id(n).unwrap();
        let mut b = Board::default();
        b.put(COOKING_POT_SLOTS[4], Stack::new(id("coal"), 1));
        b.put_mix(&COOKING_POT_SLOTS, Stack::new(id("raw_meat"), 1));
        assert_eq!(data.matching("cooking_pot", &b).unwrap().makes.item, id("roast_meat"));
        b.put_mix(&COOKING_POT_SLOTS, Stack::new(id("small_rock"), 1));
        assert!(data.matching("cooking_pot", &b).is_none());
        assert!(data.needs_heat("cooking_pot", &b).is_none());
        // Taking the last thing out of the pot fixes it.
        assert_eq!(b.take_mix(&COOKING_POT_SLOTS), Some(Stack::new(id("small_rock"), 1)));
        assert!(data.matching("cooking_pot", &b).is_some());
    }

    #[test]
    fn the_pot_stacks_like_things_and_holds_four_kinds() {
        let (items, _) = data();
        let id = |n| items.id(n).unwrap();
        let mut b = Board::default();
        for n in ["raw_meat", "stick", "small_rock", "flint"] {
            assert_eq!(b.put_mix(&COOKING_POT_SLOTS, Stack::new(id(n), 1)), None);
        }
        // A fifth kind doesn't fit; more of one already there does, up to the slot's size.
        assert_eq!(b.put_mix(&COOKING_POT_SLOTS, Stack::new(id("clay"), 1)), Some(Stack::new(id("clay"), 1)));
        assert_eq!(b.put_mix(&COOKING_POT_SLOTS, Stack::new(id("stick"), 7)), Some(Stack::new(id("stick"), 2)));
        assert_eq!(b.slots.get("pot_2"), Some(&Stack::new(id("stick"), 6)));
        assert!(!b.slots.contains_key("fire"), "nothing spills into the fire");
    }

    #[test]
    fn cutting_first_makes_more_food() {
        let (items, data) = data();
        let id = |n| items.id(n).unwrap();
        let satiety = |s: Stack| match items.get(s.item).kind {
            crate::items::ItemKind::Food { satiety, .. } => satiety * s.count as f32,
            _ => 0.0,
        };
        let mut b = Board::default();
        b.put(CUTTING_BOARD_SLOTS[0], Stack::new(id("raw_meat"), 1));
        let strips = cook(&mut b, &data, "cutting_board");
        assert_eq!(strips, Stack::new(id("meat_strips"), 3));
        let mut pot = Board::default();
        pot.put(COOKING_POT_SLOTS[4], Stack::new(id("stick"), 2));
        pot.put_mix(&COOKING_POT_SLOTS, strips);
        let cut = cook(&mut pot, &data, "cooking_pot");
        let mut whole = Board::default();
        whole.put(COOKING_POT_SLOTS[4], Stack::new(id("stick"), 2));
        whole.put_mix(&COOKING_POT_SLOTS, Stack::new(id("raw_meat"), 1));
        let roast = cook(&mut whole, &data, "cooking_pot");
        assert!(satiety(cut) > satiety(roast), "{cut:?} vs {roast:?}");
    }

    #[test]
    fn every_hearth_recipe_can_be_made_from_things_that_exist() {
        let (_, data) = data();
        for r in data.recipes.iter().filter(|r| r.station == "cooking_pot") {
            assert!(r.heat > 0, "a pot recipe with no fire");
            assert!(!r.mix.is_empty());
        }
        assert!(data.recipes.iter().any(|r| r.station == "cutting_board"));
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
