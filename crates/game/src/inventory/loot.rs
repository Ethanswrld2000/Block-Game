//! What a found chest holds the first time it is opened, from `assets/data/loot.ron`.

use std::collections::HashMap;

use bevy::prelude::Resource;

use super::grid::{Grid, Stack};
use crate::items::{ItemId, Items};

#[derive(Resource, Debug, Clone)]
pub struct Loot {
    tables: HashMap<String, Vec<(ItemId, u16, u16, f32)>>,
}

impl Loot {
    pub fn from_ron(text: &str, items: &Items) -> Result<Self, String> {
        let spec: HashMap<String, Vec<(String, u16, u16, f32)>> = ron::from_str(text).map_err(|e| format!("loot.ron: {e}"))?;
        let mut tables = HashMap::new();
        for (name, list) in spec {
            let list = list
                .into_iter()
                .map(|(i, a, b, p)| Ok((items.id(&i).ok_or_else(|| format!("loot.ron: no item '{i}'"))?, a.min(b), a.max(b), p)))
                .collect::<Result<Vec<_>, String>>()?;
            tables.insert(name, list);
        }
        Ok(Self { tables })
    }

    /// Fill a chest from a table. `rand` gives numbers in 0..1.
    pub fn fill(&self, table: &str, items: &Items, grid: &mut Grid, mut rand: impl FnMut() -> f32) {
        for (item, lo, hi, p) in self.tables.get(table).into_iter().flatten() {
            if rand() >= *p {
                continue;
            }
            let n = lo + ((rand() * (hi - lo + 1) as f32) as u16).min(hi - lo);
            // Whatever doesn't fit is simply not there.
            let _ = grid.add(items, Stack::new(*item, n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barrow_chests_hold_coins() {
        let (_, items) = crate::items::tests::load();
        let loot = Loot::from_ron(include_str!("../../../../assets/data/loot.ron"), &items).unwrap();
        let mut g = Grid::new(super::super::CHEST.0, super::super::CHEST.1);
        let mut x = 0.0f32;
        loot.fill("barrow", &items, &mut g, || {
            x = (x + 0.37) % 1.0;
            x
        });
        assert!(g.count(items.id("old_coin").unwrap()) >= 3);
    }
}
