//! What breaking a block gives you, from `assets/data/drops.ron`.

use std::collections::HashMap;

use bevy::prelude::Resource;
use voxel::{BlockId, BlockPos, BlockRegistry};

use super::grid::Stack;
use crate::items::{ItemId, Items};

#[derive(Resource, Debug, Clone)]
pub struct Drops {
    map: HashMap<BlockId, Vec<(ItemId, u16, f32)>>,
}

impl Drops {
    pub fn from_ron(text: &str, blocks: &BlockRegistry, items: &Items) -> Result<Self, String> {
        let spec: HashMap<String, Vec<(String, u16, f32)>> = ron::from_str(text).map_err(|e| format!("drops.ron: {e}"))?;
        let mut map = HashMap::new();
        for (block, list) in spec {
            let b = blocks.id(&block).ok_or_else(|| format!("drops.ron: no block '{block}'"))?;
            let list = list
                .into_iter()
                .map(|(i, n, p)| Ok((items.id(&i).ok_or_else(|| format!("drops.ron: no item '{i}'"))?, n, p)))
                .collect::<Result<Vec<_>, String>>()?;
            map.insert(b, list);
        }
        Ok(Self { map })
    }

    /// What a broken block gives. `roll` varies the chances between breaks.
    pub fn roll(&self, items: &Items, block: BlockId, at: BlockPos, roll: u64) -> Vec<Stack> {
        match self.map.get(&block) {
            None => items.for_block(block).map(|i| vec![Stack::new(i, 1)]).unwrap_or_default(),
            Some(list) => list
                .iter()
                .enumerate()
                .filter(|(k, (_, _, p))| *p >= 1.0 || chance(at, roll, *k as u64) < *p)
                .map(|(_, (i, n, _))| Stack::new(*i, *n))
                .collect(),
        }
    }
}

fn chance(p: BlockPos, roll: u64, k: u64) -> f32 {
    let mut x = (p.x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (p.y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (p.z as u64).wrapping_mul(0x1656_67B1_9E37_79F9)
        ^ roll.wrapping_mul(0x27D4_EB2F_1656_67C5)
        ^ k.wrapping_mul(0x85EB_CA77_C2B2_AE63);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::tests::load;

    #[test]
    fn blocks_drop_items() {
        let (blocks, items) = load();
        let drops = Drops::from_ron(include_str!("../../../../assets/data/drops.ron"), &blocks, &items).unwrap();
        let at = BlockPos::new(3, 4, 5);
        let stone = drops.roll(&items, blocks.expect("stone"), at, 0);
        assert_eq!(stone, vec![Stack::new(items.id("cobblestone").unwrap(), 1)]);
        // Unlisted blocks drop themselves.
        assert_eq!(drops.roll(&items, blocks.expect("log"), at, 0), vec![Stack::new(items.id("log").unwrap(), 1)]);
        assert!(drops.roll(&items, blocks.expect("water"), at, 0).is_empty());
        // Leaves give sticks about half the time.
        let stick = items.id("stick").unwrap();
        let n = (0..1000).filter(|r| drops.roll(&items, blocks.expect("leaves"), at, *r).iter().any(|s| s.item == stick)).count();
        assert!((400..600).contains(&n), "{n}");
    }
}
