//! What you carry: pockets, a bag on your back, clothes and armor, chests in
//! the world, crafting, and items lying on the ground.
//!
//! Items fill grids by their size (`grid`). The hotbar only holds shortcuts:
//! a slot is "use the torches I have", wherever they are packed.
//!
//! Tab opens the inventory screen (`ui`). F9 is a test key that hands you one
//! of everything. `BLOCKGAME_INVENTORY=1` opens the screen at start, for
//! screenshots.

pub mod crafting;
pub mod drops;
pub mod grid;
pub mod loot;
pub(crate) mod ui;
mod world_items;

pub use world_items::Spill;

use std::collections::{BTreeMap, HashMap};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use voxel::BlockPos;

use crate::interact::{Hotbar, Mined, SLOTS};
use crate::items::{ItemId, ItemKind, Items, WearSlot};
use crate::save::SaveDir;
use crate::survival::health::Layer;
use drops::Drops;
use grid::{Grid, GridSave, Stack};

/// Pockets and belt, always with you.
pub const POCKETS: (u8, u8) = (5, 3);
/// A chest's grid.
pub const CHEST: (u8, u8) = (8, 5);
/// Armor can keep at most this share of a blow off a bar.
pub const MAX_PROTECTION: f32 = 0.75;

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Inventory::empty())
            .init_resource::<Chests>()
            .add_systems(PostStartup, (setup_data, load_inventory).chain())
            .add_systems(
                Update,
                (collect_mined, auto_bind, supply_key).chain().after(crate::interact::InteractSet),
            )
            .add_systems(Update, save_inventory)
            .add_systems(Last, save_inventory_on_exit)
            .add_plugins((ui::InventoryUiPlugin, world_items::WorldItemsPlugin));
    }
}

#[derive(Resource, Debug, Clone)]
pub struct Inventory {
    pub pockets: Grid,
    pub worn: BTreeMap<WearSlot, ItemId>,
    /// The grid of the bag worn on your back.
    pub bag: Option<Grid>,
}

impl Inventory {
    pub fn empty() -> Self {
        Self { pockets: Grid::new(POCKETS.0, POCKETS.1), worn: BTreeMap::new(), bag: None }
    }

    pub fn count(&self, item: ItemId) -> u32 {
        self.pockets.count(item) + self.bag.as_ref().map_or(0, |b| b.count(item))
    }

    /// Use up `n` of an item, only if you have that many.
    pub fn take(&mut self, item: ItemId, n: u32) -> bool {
        if self.count(item) < n {
            return false;
        }
        let got = self.pockets.remove(item, n);
        if got < n
            && let Some(b) = &mut self.bag
        {
            b.remove(item, n - got);
        }
        true
    }

    /// Pack a stack away (pockets first, then the bag). Returns what didn't fit.
    pub fn add(&mut self, items: &Items, stack: Stack) -> Option<Stack> {
        let left = self.pockets.add(items, stack)?;
        match &mut self.bag {
            Some(b) => b.add(items, left),
            None => Some(left),
        }
    }

    /// Everything you carry and wear, kg.
    pub fn mass(&self, items: &Items) -> f32 {
        self.pockets.mass(items)
            + self.bag.as_ref().map_or(0.0, |b| b.mass(items))
            + self.worn.values().map(|i| items.get(*i).mass).sum::<f32>()
    }

    /// Warmth of what you wear, °C.
    pub fn insulation(&self, items: &Items) -> f32 {
        self.worn
            .values()
            .map(|i| match &items.get(*i).kind {
                ItemKind::Wear { insulation, .. } => *insulation,
                _ => 0.0,
            })
            .sum()
    }

    /// Share of a blow landing on `layer` that armor and clothes keep off.
    pub fn protection(&self, items: &Items, layer: Layer) -> f32 {
        let p: f32 = self
            .worn
            .values()
            .map(|i| match &items.get(*i).kind {
                ItemKind::Wear { protection, .. } => protection.get(&layer).copied().unwrap_or(0.0),
                _ => 0.0,
            })
            .sum();
        p.min(MAX_PROTECTION)
    }

    /// Put something on. Returns what was in that slot before.
    pub fn wear(&mut self, items: &Items, item: ItemId) -> Result<Option<ItemId>, &'static str> {
        let def = items.get(item);
        let Some(slot) = def.wear_slot() else { return Err("You can't wear that") };
        let old = if self.worn.contains_key(&slot) { Some(self.take_off(slot)?) } else { None };
        if let ItemKind::Bag { grid } = def.kind {
            self.bag = Some(Grid::new(grid.0, grid.1));
        }
        self.worn.insert(slot, item);
        Ok(old)
    }

    /// Take something off. A bag has to be empty first.
    pub fn take_off(&mut self, slot: WearSlot) -> Result<ItemId, &'static str> {
        if slot == WearSlot::Back && self.bag.as_ref().is_some_and(|b| !b.is_empty()) {
            return Err("Empty the bag before taking it off");
        }
        let item = self.worn.remove(&slot).ok_or("Nothing worn there")?;
        if slot == WearSlot::Back {
            self.bag = None;
        }
        Ok(item)
    }
}

/// Chests placed in the world and what's in them.
#[derive(Resource, Default, Debug)]
pub struct Chests {
    pub map: HashMap<BlockPos, Grid>,
}

impl Chests {
    pub fn grid(&mut self, p: BlockPos) -> &mut Grid {
        self.map.entry(p).or_insert_with(|| Grid::new(CHEST.0, CHEST.1))
    }
}

/// The starting kit, from `assets/data/start_kit.ron`.
#[derive(Deserialize)]
struct KitSpec {
    wear: Vec<String>,
    carry: Vec<(String, u16)>,
}

pub fn start_kit(items: &Items, text: &str) -> Result<Inventory, String> {
    let spec: KitSpec = ron::from_str(text).map_err(|e| format!("start_kit.ron: {e}"))?;
    let mut inv = Inventory::empty();
    for name in &spec.wear {
        let id = items.id(name).ok_or_else(|| format!("start_kit.ron: no item '{name}'"))?;
        inv.wear(items, id).map_err(|e| format!("start_kit.ron: {name}: {e}"))?;
    }
    for (name, n) in &spec.carry {
        let id = items.id(name).ok_or_else(|| format!("start_kit.ron: no item '{name}'"))?;
        if inv.add(items, Stack::new(id, *n)).is_some() {
            return Err(format!("start_kit.ron: no room for {n} {name}"));
        }
    }
    Ok(inv)
}

fn setup_data(mut commands: Commands, items: Res<Items>, blocks: Res<crate::blocks::Blocks>) {
    let recipes = crafting::Recipes::from_ron(include_str!("../../../../assets/data/recipes.ron"), &items).unwrap_or_else(|e| panic!("{e}"));
    let drops = Drops::from_ron(include_str!("../../../../assets/data/drops.ron"), &blocks.registry, &items).unwrap_or_else(|e| panic!("{e}"));
    commands.insert_resource(recipes);
    commands.insert_resource(drops);
    let loot = loot::Loot::from_ron(include_str!("../../../../assets/data/loot.ron"), &items).unwrap_or_else(|e| panic!("{e}"));
    commands.insert_resource(loot);
}

/// Mined blocks drop their items into your pockets, or onto the ground if full.
#[allow(clippy::too_many_arguments)]
fn collect_mined(
    mut mined: MessageReader<Mined>,
    items: Res<Items>,
    drops: Option<Res<Drops>>,
    mut inv: ResMut<Inventory>,
    mut chests: ResMut<Chests>,
    mut spill: MessageWriter<world_items::Spill>,
    mut roll: Local<u64>,
    mut placed: MessageReader<crate::interact::Placed>,
) {
    // Chests you place start empty (found ones fill with loot when first opened).
    for p in placed.read() {
        if items.for_block(p.block).is_some_and(|i| items.get(i).name == "chest") {
            chests.grid(p.pos);
        }
    }
    let Some(drops) = drops else { return };
    for m in mined.read() {
        let mut got = Vec::new();
        // A broken chest spills what was in it.
        if let Some(g) = chests.map.remove(&m.pos) {
            got.extend(g.items.iter().map(|p| p.stack));
        }
        *roll += 1;
        got.extend(drops.roll(&items, m.block, m.pos, *roll));
        let at = Vec3::new(m.pos.x as f32 + 0.5, m.pos.y as f32 + 0.5, m.pos.z as f32 + 0.5);
        for s in got {
            if let Some(left) = inv.add(&items, s) {
                spill.write(world_items::Spill { stack: left, at });
            }
        }
    }
}

/// Things you can hold get a hotbar slot when they first turn up.
fn auto_bind(inv: Res<Inventory>, items: Res<Items>, mut hotbar: ResMut<Hotbar>) {
    if !inv.is_changed() {
        return;
    }
    let mut kinds: Vec<ItemId> = inv.pockets.items.iter().chain(inv.bag.iter().flat_map(|b| b.items.iter())).map(|p| p.stack.item).collect();
    kinds.dedup();
    for id in kinds {
        let holdable = matches!(items.get(id).kind, ItemKind::Tool { .. } | ItemKind::Block(_) | ItemKind::Torch | ItemKind::Food { .. });
        if !holdable || hotbar.slots.contains(&Some(id)) {
            continue;
        }
        if let Some(free) = hotbar.slots.iter().position(Option::is_none) {
            hotbar.slots[free] = Some(id);
        }
    }
}

/// F9: one stack of everything (test key). What doesn't fit lands at your feet.
fn supply_key(
    keys: Res<ButtonInput<KeyCode>>,
    items: Res<Items>,
    player: Res<crate::player::Player>,
    mut inv: ResMut<Inventory>,
    mut spill: MessageWriter<world_items::Spill>,
    mut toast: MessageWriter<crate::survival::Toast>,
) {
    if !keys.just_pressed(KeyCode::F9) {
        return;
    }
    let mut dropped = 0;
    for id in items.ids() {
        let n = items.get(id).stack.min(8);
        if let Some(left) = inv.add(&items, Stack::new(id, n)) {
            dropped += 1;
            spill.write(world_items::Spill { stack: left, at: player.pos + Vec3::Y * 0.5 });
        }
    }
    toast.write(crate::survival::Toast(format!("Test supplies: one of everything ({dropped} kinds on the ground)")));
}

/// What `inventory.ron` holds.
#[derive(Serialize, Deserialize, Default)]
struct InventorySave {
    pockets: GridSave,
    worn: Vec<(WearSlot, String)>,
    bag: Option<GridSave>,
    hotbar: Vec<Option<String>>,
    selected: Option<usize>,
}

#[derive(Serialize, Deserialize, Default)]
struct ChestsSave {
    chests: Vec<((i32, i32, i32), GridSave)>,
}

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some()
}

#[allow(clippy::too_many_arguments)]
fn load_inventory(
    dir: Res<SaveDir>,
    items: Res<Items>,
    mut inv: ResMut<Inventory>,
    mut chests: ResMut<Chests>,
    mut hotbar: ResMut<Hotbar>,
    mut hands: ResMut<crate::interact::Hands>,
) {
    let kit = || start_kit(&items, include_str!("../../../../assets/data/start_kit.ron")).unwrap_or_else(|e| panic!("{e}"));
    let saved = if screenshot_run() {
        None
    } else {
        std::fs::read_to_string(dir.file("inventory.ron")).ok().and_then(|t| match ron::from_str::<InventorySave>(&t) {
            Ok(s) => Some(s),
            Err(e) => {
                warn!("inventory.ron: {e}");
                None
            }
        })
    };
    let Some(save) = saved else {
        *inv = kit();
        return;
    };
    let mut new = Inventory::empty();
    for (slot, name) in &save.worn {
        if let Some(id) = items.id(name)
            && items.get(id).wear_slot() == Some(*slot)
        {
            let _ = new.wear(&items, id);
        }
    }
    let (pockets, mut spill) = Grid::from_save(&save.pockets, &items, POCKETS.0, POCKETS.1);
    new.pockets = pockets;
    if let (Some(b), Some(bs)) = (new.bag.as_ref().map(|g| (g.w, g.h)), &save.bag) {
        let (g, s) = Grid::from_save(bs, &items, b.0, b.1);
        new.bag = Some(g);
        spill.extend(s);
    }
    for s in spill {
        if new.add(&items, s).is_some() {
            warn!("inventory.ron: no room for {} {}", s.count, items.get(s.item).name);
        }
    }
    *inv = new;
    for (i, name) in save.hotbar.iter().take(SLOTS).enumerate() {
        hotbar.slots[i] = name.as_deref().and_then(|n| items.id(n));
    }
    if let Some(sel) = save.selected.filter(|s| *s < SLOTS) {
        hands.right = Some(sel);
    }
    if let Ok(text) = std::fs::read_to_string(dir.file("chests.ron")) {
        match ron::from_str::<ChestsSave>(&text) {
            Ok(cs) => {
                for ((x, y, z), g) in cs.chests {
                    chests.map.insert(BlockPos::new(x, y, z), Grid::from_save(&g, &items, CHEST.0, CHEST.1).0);
                }
            }
            Err(e) => warn!("chests.ron: {e}"),
        }
    }
}

fn store(dir: &SaveDir, items: &Items, inv: &Inventory, chests: &Chests, hotbar: &Hotbar, hands: &crate::interact::Hands) {
    let save = InventorySave {
        pockets: inv.pockets.to_save(items),
        worn: inv.worn.iter().map(|(s, i)| (*s, items.get(*i).name.clone())).collect(),
        bag: inv.bag.as_ref().map(|b| b.to_save(items)),
        hotbar: hotbar.slots.iter().map(|s| s.map(|i| items.get(i).name.clone())).collect(),
        selected: hands.right,
    };
    let cs = ChestsSave { chests: chests.map.iter().map(|(p, g)| ((p.x, p.y, p.z), g.to_save(items))).collect() };
    let pretty = ron::ser::PrettyConfig::default();
    for (name, text) in [("inventory.ron", ron::ser::to_string_pretty(&save, pretty.clone())), ("chests.ron", ron::ser::to_string_pretty(&cs, pretty))] {
        match text {
            Ok(t) => {
                if let Err(e) = std::fs::write(dir.file(name), t) {
                    warn!("could not save {name}: {e}");
                }
            }
            Err(e) => warn!("could not save {name}: {e}"),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn save_inventory(
    time: Res<Time>,
    mut timer: Local<f32>,
    dir: Res<SaveDir>,
    items: Res<Items>,
    inv: Res<Inventory>,
    chests: Res<Chests>,
    hotbar: Res<Hotbar>,
    hands: Res<crate::interact::Hands>,
) {
    *timer += time.delta_secs();
    if *timer < 30.0 || screenshot_run() {
        return;
    }
    *timer = 0.0;
    store(&dir, &items, &inv, &chests, &hotbar, &hands);
}

#[allow(clippy::too_many_arguments)]
fn save_inventory_on_exit(
    mut exit: MessageReader<AppExit>,
    dir: Res<SaveDir>,
    items: Res<Items>,
    inv: Res<Inventory>,
    chests: Res<Chests>,
    hotbar: Res<Hotbar>,
    hands: Res<crate::interact::Hands>,
) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &items, &inv, &chests, &hotbar, &hands);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::tests::load;

    fn kit(items: &Items) -> Inventory {
        start_kit(items, include_str!("../../../../assets/data/start_kit.ron")).unwrap()
    }

    #[test]
    fn start_kit_fits_and_is_dressed() {
        let (_, items) = load();
        let inv = kit(&items);
        assert!(inv.bag.is_some());
        assert_eq!(inv.count(items.id("torch").unwrap()), 8);
        assert!(inv.insulation(&items) > 4.0);
        assert!(inv.mass(&items) > 5.0 && inv.mass(&items) < 20.0, "{}", inv.mass(&items));
    }

    #[test]
    fn bag_must_be_empty_to_take_off() {
        let (_, items) = load();
        let mut inv = kit(&items);
        inv.bag.as_mut().unwrap().add(&items, Stack::new(items.id("dirt").unwrap(), 3));
        assert!(inv.take_off(WearSlot::Back).is_err());
        let bag = inv.bag.take().unwrap();
        inv.bag = Some(Grid::new(bag.w, bag.h));
        assert_eq!(inv.take_off(WearSlot::Back), Ok(items.id("reed_satchel").unwrap()));
        assert!(inv.bag.is_none());
    }

    #[test]
    fn dressing_for_the_cold_and_armor() {
        let (_, items) = load();
        let mut inv = kit(&items);
        let before = inv.insulation(&items);
        let old = inv.wear(&items, items.id("fur_coat").unwrap()).unwrap();
        assert_eq!(old, items.id("linen_shirt"));
        assert!(inv.insulation(&items) >= before + 9.0);
        assert_eq!(inv.protection(&items, Layer::Blood), 0.0);
        inv.wear(&items, items.id("hide_jerkin").unwrap()).unwrap();
        assert!(inv.protection(&items, Layer::Blood) > 0.25);
    }

    #[test]
    fn take_is_all_or_nothing_across_pockets_and_bag() {
        let (_, items) = load();
        let mut inv = kit(&items);
        let dirt = items.id("dirt").unwrap();
        assert_eq!(inv.add(&items, Stack::new(dirt, 200)), None);
        assert_eq!(inv.count(dirt), 200);
        assert!(!inv.take(dirt, 201));
        assert!(inv.take(dirt, 150));
        assert_eq!(inv.count(dirt), 50);
    }
}
