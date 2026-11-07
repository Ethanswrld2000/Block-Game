//! Items you can hold, from `assets/data/items.ron`: tools, placeable blocks
//! and torches, plus how fast each tool breaks each material.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::Deserialize;
use voxel::{BlockDef, BlockId, BlockRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    #[serde(default)]
    model: Vec<ModelBox>,
}

fn default_mass() -> f32 {
    0.5
}

#[derive(Deserialize)]
enum KindSpec {
    Tool { speeds: HashMap<String, f32> },
    Block(String),
    Torch,
    Food { tier: u8, satiety: f32 },
}

/// One box of a held item's model, in hand space (metres, -Z points away).
#[derive(Deserialize, Debug, Clone, Copy)]
pub struct ModelBox {
    pub size: (f32, f32, f32),
    pub offset: (f32, f32, f32),
    pub color: (f32, f32, f32),
}

#[derive(Debug, Clone)]
pub enum ItemKind {
    Tool { speeds: HashMap<String, f32> },
    Block(BlockId),
    Torch,
    /// Eaten by holding right click. `tier` is a tier in food_tiers.ron.
    Food { tier: u8, satiety: f32 },
}

#[derive(Debug, Clone)]
pub struct Item {
    pub name: String,
    pub title: String,
    pub kind: ItemKind,
    /// Weight in kg.
    pub mass: f32,
    pub model: Vec<ModelBox>,
}

#[derive(Resource, Debug, Clone)]
pub struct Items {
    hand: HashMap<String, f32>,
    list: Vec<Item>,
}

impl Items {
    pub fn from_ron(text: &str, blocks: &BlockRegistry) -> Result<Self, String> {
        let spec: ItemsSpec = ron::from_str(text).map_err(|e| format!("items.ron: {e}"))?;
        let mut list = Vec::new();
        for s in spec.items {
            let kind = match s.kind {
                KindSpec::Tool { speeds } => ItemKind::Tool { speeds },
                KindSpec::Block(b) => ItemKind::Block(
                    blocks.id(&b).ok_or_else(|| format!("item '{}' places unknown block '{b}'", s.name))?,
                ),
                KindSpec::Torch => ItemKind::Torch,
                KindSpec::Food { tier, satiety } => ItemKind::Food { tier, satiety },
            };
            if list.iter().any(|i: &Item| i.name == s.name) {
                return Err(format!("item '{}' is defined twice", s.name));
            }
            list.push(Item { name: s.name, title: s.title, kind, mass: s.mass, model: s.model });
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
        Some(block.hardness / self.speed(held, &block.material).max(0.01))
    }
}

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load_items.after(crate::blocks::load_blocks));
    }
}

fn load_items(mut commands: Commands, blocks: Res<crate::blocks::Blocks>) {
    let items = Items::from_ron(include_str!("../../../assets/data/items.ron"), &blocks.registry).unwrap_or_else(|e| panic!("{e}"));
    commands.insert_resource(items);
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxel::TileSheet;

    fn load() -> (BlockRegistry, Items) {
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
        assert!(items.break_time(axe, log).unwrap() < items.break_time(pick, log).unwrap());
        // A torch in the hand mines like a bare hand.
        assert_eq!(items.break_time(items.id("torch"), stone), Some(hand));
    }

    #[test]
    fn water_cannot_be_broken_and_torches_break_instantly() {
        let (blocks, items) = load();
        assert_eq!(items.break_time(None, blocks.get(blocks.expect("water"))), None);
        assert_eq!(items.break_time(None, blocks.get(blocks.expect("torch"))), Some(0.0));
    }
}
