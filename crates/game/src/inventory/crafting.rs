//! Crafting recipes from `assets/data/recipes.ron`.

use bevy::prelude::Resource;
use serde::Deserialize;

use super::grid::Stack;
use super::Inventory;
use crate::items::{ItemId, Items};

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Station {
    /// Anywhere.
    #[default]
    Hand,
    /// By your bedroll.
    Camp,
}

#[derive(Deserialize)]
struct RecipeSpec {
    makes: (String, u16),
    needs: Vec<(String, u16)>,
    #[serde(default)]
    at: Station,
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub makes: Stack,
    pub needs: Vec<Stack>,
    pub at: Station,
}

#[derive(Resource, Debug, Clone)]
pub struct Recipes {
    pub list: Vec<Recipe>,
}

impl Recipes {
    pub fn from_ron(text: &str, items: &Items) -> Result<Self, String> {
        let specs: Vec<RecipeSpec> = ron::from_str(text).map_err(|e| format!("recipes.ron: {e}"))?;
        let id = |n: &str| -> Result<ItemId, String> { items.id(n).ok_or_else(|| format!("recipes.ron: no item '{n}'")) };
        let mut list = Vec::new();
        for s in specs {
            let makes = Stack::new(id(&s.makes.0)?, s.makes.1.max(1));
            let needs = s.needs.iter().map(|(n, c)| Ok(Stack::new(id(n)?, *c))).collect::<Result<Vec<_>, String>>()?;
            list.push(Recipe { makes, needs, at: s.at });
        }
        Ok(Self { list })
    }
}

impl Recipe {
    pub fn have_parts(&self, inv: &Inventory) -> bool {
        self.needs.iter().all(|n| inv.count(n.item) >= n.count as u32)
    }

    /// Use up the parts and pack the result. Returns what didn't fit, or
    /// `None` if the parts are missing.
    pub fn craft(&self, inv: &mut Inventory, items: &Items) -> Option<Option<Stack>> {
        if !self.have_parts(inv) {
            return None;
        }
        for n in &self.needs {
            inv.take(n.item, n.count as u32);
        }
        Some(inv.add(items, self.makes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::tests::load;

    #[test]
    fn craft_a_basic_kit_from_raw_materials() {
        let (_, items) = load();
        let recipes = Recipes::from_ron(include_str!("../../../../assets/data/recipes.ron"), &items).unwrap();
        let find = |name: &str| recipes.list.iter().find(|r| items.get(r.makes.item).name == name).unwrap();
        let mut inv = Inventory::empty();
        let id = |n: &str| items.id(n).unwrap();
        // Logs from trees, stone from the ground, fiber from the grass.
        inv.add(&items, Stack::new(id("log"), 3));
        inv.add(&items, Stack::new(id("cobblestone"), 3));
        inv.add(&items, Stack::new(id("plant_fiber"), 12));
        assert!(!find("stone_pickaxe").have_parts(&inv));
        for name in ["planks", "stick", "cordage", "stone_pickaxe"] {
            assert!(find(name).craft(&mut inv, &items).is_some(), "{name}");
        }
        assert_eq!(inv.count(id("stone_pickaxe")), 1);
        assert_eq!(inv.count(id("cobblestone")), 0);
        // Fiber left for a hat; not enough for a cloak.
        assert!(find("reed_hat").have_parts(&inv));
        assert!(!find("reed_cloak").have_parts(&inv));
    }
}
