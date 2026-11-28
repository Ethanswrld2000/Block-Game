//! What a found chest holds the first time it is opened, from `assets/data/loot.ron`.
//! Weapons, tools, shields and armor from a table with `quality` odds come out
//! with a rolled quality and traits (`quality`).

use std::collections::HashMap;

use bevy::prelude::Resource;
use serde::Deserialize;

use super::grid::{Grid, Stack};
use crate::items::{ItemId, Items};
use crate::quality::Odds;

#[derive(Deserialize)]
struct LootSpec {
    tables: HashMap<String, Vec<(String, u16, u16, f32)>>,
    #[serde(default)]
    quality: HashMap<String, (f32, f32, f32, f32)>,
}

#[derive(Resource, Debug, Clone)]
pub struct Loot {
    tables: HashMap<String, Vec<(ItemId, u16, u16, f32)>>,
    quality: HashMap<String, Odds>,
    items: Items,
}

impl Loot {
    pub fn from_ron(text: &str, items: &Items) -> Result<Self, String> {
        let spec: LootSpec = ron::from_str(text).map_err(|e| format!("loot.ron: {e}"))?;
        let quality = spec
            .quality
            .into_iter()
            .map(|(k, (a, b, c, d))| (k, [a, b, c, d]))
            .collect();
        let mut tables = HashMap::new();
        for (name, list) in spec.tables {
            let list = list
                .into_iter()
                .map(|(i, a, b, p)| {
                    Ok((
                        items
                            .id(&i)
                            .ok_or_else(|| format!("loot.ron: no item '{i}'"))?,
                        a.min(b),
                        a.max(b),
                        p,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            tables.insert(name, list);
        }
        Ok(Self {
            tables,
            quality,
            items: items.clone(),
        })
    }

    /// Fill a chest from a table. `rand` gives numbers in 0..1.
    pub fn fill(&self, table: &str, items: &Items, grid: &mut Grid, rand: impl FnMut() -> f32) {
        for stack in self.roll(table, rand) {
            // Whatever doesn't fit is simply not there.
            let _ = grid.add(items, stack);
        }
    }

    /// Roll a table: the stacks it gives this time. `rand` gives numbers in 0..1.
    pub fn roll(&self, table: &str, mut rand: impl FnMut() -> f32) -> Vec<Stack> {
        let mut out = Vec::new();
        for (item, lo, hi, p) in self.tables.get(table).into_iter().flatten() {
            if rand() >= *p {
                continue;
            }
            let n = lo + ((rand() * (hi - lo + 1) as f32) as u16).min(hi - lo);
            let item = match self.quality.get(table) {
                Some(odds) if self.items.get(*item).takes_quality() => {
                    let mut rng =
                        crate::creatures::brain::Rng(((rand() * u32::MAX as f32) as u64) << 8 | 1);
                    self.items.roll(*item, *odds, &mut rng)
                }
                _ => *item,
            };
            out.push(Stack::new(item, n));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn barrow_chests_hold_coins() {
        let (_, items) = crate::items::tests::load();
        let loot =
            Loot::from_ron(include_str!("../../../../assets/data/loot.ron"), &items).unwrap();
        let mut g = Grid::new(super::super::CHEST.0, super::super::CHEST.1);
        let mut x = 0.0f32;
        loot.fill("barrow", &items, &mut g, || {
            x = (x + 0.37) % 1.0;
            x
        });
        assert!(g.count(items.id("old_coin").unwrap()) >= 3);
    }

    #[test]
    fn warden_loot_comes_out_better_than_plain() {
        let (_, items) = crate::items::tests::load();
        let loot =
            Loot::from_ron(include_str!("../../../../assets/data/loot.ron"), &items).unwrap();
        let mut rng = crate::creatures::brain::Rng(3);
        let mut better = 0;
        for _ in 0..200 {
            for s in loot.roll("rotting_warden", || rng.unit()) {
                let it = items.get(s.item);
                if it.takes_quality() {
                    assert_ne!(it.quality, crate::quality::Quality::Plain, "{}", it.name);
                    better += 1;
                }
            }
        }
        assert!(better > 50);
    }
}
