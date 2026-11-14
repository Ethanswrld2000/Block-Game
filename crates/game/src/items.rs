//! Items, from `assets/data/items.ron`: tools, placeable blocks, torches,
//! food, clothes and armor, bags and crafting materials. Each item has a size
//! in inventory cells, a stack size and a weight.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use voxel::{BlockDef, BlockId, BlockRegistry};

use crate::survival::health::Layer;
use crate::swing::Swing;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub u16);

#[derive(Deserialize)]
struct ItemsSpec {
    hand: HashMap<String, f32>,
    items: Vec<ItemSpec>,
}

#[derive(Deserialize)]
struct ItemSpec {
    name: String,
    title: String,
    kind: KindSpec,
    #[serde(default = "default_mass")]
    mass: f32,
    /// Cells taken in a grid, (width, height).
    #[serde(default = "one_by_one")]
    size: (u8, u8),
    #[serde(default)]
    stack: Option<u16>,
    /// Damage dealt when you hit a creature with it.
    #[serde(default)]
    damage: Option<f32>,
    /// How it is swung (see `swing`).
    #[serde(default)]
    swing: Swing,
}

fn default_mass() -> f32 {
    0.5
}

fn one_by_one() -> (u8, u8) {
    (1, 1)
}

#[derive(Deserialize)]
enum KindSpec {
    Tool { speeds: HashMap<String, f32> },
    Block(String),
    Torch,
    Food { tier: u8, satiety: f32 },
    Wear {
        slot: WearSlot,
        #[serde(default)]
        insulation: f32,
        #[serde(default)]
        protection: HashMap<Layer, f32>,
    },
    Bag { grid: (u8, u8) },
    Material,
}

/// Where something is worn. Clothes and armor go on separate layers.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WearSlot {
    Head,
    HeadArmor,
    Body,
    BodyArmor,
    Legs,
    Feet,
    /// The bag on your back.
    Back,
}

impl WearSlot {
    pub const ALL: [WearSlot; 7] =
        [WearSlot::Head, WearSlot::HeadArmor, WearSlot::Body, WearSlot::BodyArmor, WearSlot::Legs, WearSlot::Feet, WearSlot::Back];

    pub fn title(self) -> &'static str {
        match self {
            WearSlot::Head => "Head",
            WearSlot::HeadArmor => "Helmet",
            WearSlot::Body => "Body",
            WearSlot::BodyArmor => "Body armor",
            WearSlot::Legs => "Legs",
            WearSlot::Feet => "Feet",
            WearSlot::Back => "Back",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ItemKind {
    Tool { speeds: HashMap<String, f32> },
    Block(BlockId),
    Torch,
    /// Eaten by holding right click. `tier` is a tier in food_tiers.ron.
    Food { tier: u8, satiety: f32 },
    /// Clothes and armor: warmth in °C, and the share of a blow kept off each health bar.
    Wear { slot: WearSlot, insulation: f32, protection: HashMap<Layer, f32> },
    /// Worn on the back; holds a grid of this size.
    Bag { grid: (u8, u8) },
    /// Crafting materials.
    Material,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub name: String,
    pub title: String,
    pub kind: ItemKind,
    /// Weight in kg, per item.
    pub mass: f32,
    /// Cells in a grid, (width, height), unrotated.
    pub size: (u8, u8),
    /// How many fit in one stack.
    pub stack: u16,
    /// Damage when you hit a creature with it.
    pub damage: f32,
    /// How the arm swings it; mining blows land at the swing's contact.
    pub swing: Swing,
}

/// Damage of a bare-handed blow.
pub const FIST_DAMAGE: f32 = 2.0;

impl Item {
    pub fn wear_slot(&self) -> Option<WearSlot> {
        match self.kind {
            ItemKind::Wear { slot, .. } => Some(slot),
            ItemKind::Bag { .. } => Some(WearSlot::Back),
            _ => None,
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct Items {
    hand: HashMap<String, f32>,
    list: Vec<Item>,
}

impl Items {
    pub fn from_ron(text: &str, blocks: &BlockRegistry) -> Result<Self, String> {
        let spec: ItemsSpec = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
            .map_err(|e| format!("items.ron: {e}"))?;
        let mut list = Vec::new();
        for s in spec.items {
            let kind = match s.kind {
                KindSpec::Tool { speeds } => ItemKind::Tool { speeds },
                KindSpec::Block(b) => ItemKind::Block(
                    blocks.id(&b).ok_or_else(|| format!("item '{}' places unknown block '{b}'", s.name))?,
                ),
                KindSpec::Torch => ItemKind::Torch,
                KindSpec::Food { tier, satiety } => ItemKind::Food { tier, satiety },
                KindSpec::Wear { slot, insulation, protection } => {
                    if slot == WearSlot::Back {
                        return Err(format!("item '{}': things worn on the back are Bag items", s.name));
                    }
                    ItemKind::Wear { slot, insulation, protection }
                }
                KindSpec::Bag { grid } => ItemKind::Bag { grid },
                KindSpec::Material => ItemKind::Material,
            };
            if list.iter().any(|i: &Item| i.name == s.name) {
                return Err(format!("item '{}' is defined twice", s.name));
            }
            if s.size.0 == 0 || s.size.1 == 0 {
                return Err(format!("item '{}' has an empty size", s.name));
            }
            let stack = s.stack.unwrap_or(match kind {
                ItemKind::Block(_) => 32,
                ItemKind::Torch => 16,
                ItemKind::Food { .. } => 8,
                ItemKind::Material => 32,
                _ => 1,
            });
            let damage = s.damage.unwrap_or(match kind {
                ItemKind::Tool { .. } => 4.0,
                _ => FIST_DAMAGE,
            });
            list.push(Item { name: s.name, title: s.title, kind, mass: s.mass, size: s.size, stack: stack.max(1), damage, swing: s.swing });
        }
        Ok(Self { hand: spec.hand, list })
    }

    pub fn get(&self, id: ItemId) -> &Item {
        &self.list[id.0 as usize]
    }

    pub fn id(&self, name: &str) -> Option<ItemId> {
        self.list.iter().position(|i| i.name == name).map(|i| ItemId(i as u16))
    }

    pub fn ids(&self) -> impl Iterator<Item = ItemId> {
        (0..self.list.len()).map(|i| ItemId(i as u16))
    }

    /// The item that places this block, if any.
    pub fn for_block(&self, block: BlockId) -> Option<ItemId> {
        self.list.iter().position(|i| matches!(i.kind, ItemKind::Block(b) if b == block)).map(|i| ItemId(i as u16))
    }

    /// Mining speed of what is in the hand (nothing, a tool, or anything
    /// else, which works like a bare hand) against a material.
    pub fn speed(&self, held: Option<ItemId>, material: &str) -> f32 {
        let hand = self.hand.get(material).copied().unwrap_or(1.0);
        match held.map(|i| &self.get(i).kind) {
            Some(ItemKind::Tool { speeds }) => speeds.get(material).copied().unwrap_or(hand).max(hand),
            _ => hand,
        }
    }

    /// Seconds to break a block, or `None` if it can't be broken.
    pub fn break_time(&self, held: Option<ItemId>, block: &BlockDef) -> Option<f32> {
        if block.hardness < 0.0 {
            return None;
        }
        // A speed of 0 means this can't cut it at all (logs by hand).
        let speed = self.speed(held, &block.material);
        (speed > 0.0).then(|| block.hardness / speed)
    }
}

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::menu::GamePreStartup, load_items.after(crate::blocks::load_blocks));
    }
}

fn load_items(mut commands: Commands, blocks: Res<crate::blocks::Blocks>) {
    let items = Items::from_ron(include_str!("../../../assets/data/items.ron"), &blocks.registry).unwrap_or_else(|e| panic!("{e}"));
    commands.insert_resource(items);
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use voxel::TileSheet;

    pub fn load() -> (BlockRegistry, Items) {
        let tiles = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
        let blocks = BlockRegistry::from_ron(include_str!("../../../assets/data/blocks.ron"), &tiles).unwrap();
        let items = Items::from_ron(include_str!("../../../assets/data/items.ron"), &blocks).unwrap();
        (blocks, items)
    }

    #[test]
    fn right_tool_is_faster() {
        let (blocks, items) = load();
        let stone = blocks.get(blocks.expect("stone"));
        let log = blocks.get(blocks.expect("log"));
        let pick = items.id("stone_pickaxe");
        let axe = items.id("stone_axe");
        let hand = items.break_time(None, stone).unwrap();
        let picked = items.break_time(pick, stone).unwrap();
        assert!(picked < 1.0 && hand > 8.0, "pick {picked}, hand {hand}");
        // Logs need an axe: not bare hands, not a pickaxe.
        assert!(items.break_time(axe, log).is_some_and(|t| t < 1.0));
        assert_eq!(items.break_time(pick, log), None);
        assert_eq!(items.break_time(None, log), None);
        // A torch in the hand mines like a bare hand.
        assert_eq!(items.break_time(items.id("torch"), stone), Some(hand));
    }

    #[test]
    fn water_cannot_be_broken_and_torches_break_instantly() {
        let (blocks, items) = load();
        assert_eq!(items.break_time(None, blocks.get(blocks.expect("water"))), None);
        assert_eq!(items.break_time(None, blocks.get(blocks.expect("torch"))), Some(0.0));
    }

    #[test]
    fn wearables_and_bags() {
        let (_, items) = load();
        let satchel = items.get(items.id("reed_satchel").unwrap());
        assert_eq!(satchel.wear_slot(), Some(WearSlot::Back));
        let coat = items.get(items.id("fur_coat").unwrap());
        assert!(matches!(coat.kind, ItemKind::Wear { slot: WearSlot::Body, insulation, .. } if insulation > 8.0));
    }
}
