//! A grid of cells that items fill by their size, the way a real bag packs:
//! a pickaxe takes 2x3, a berry 1x1, and anything can be turned sideways.
//! Plain Rust (no Bevy systems), so a server can check moves later.

use serde::{Deserialize, Serialize};

use crate::items::{ItemId, Items};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    pub item: ItemId,
    pub count: u16,
}

impl Stack {
    pub fn new(item: ItemId, count: u16) -> Self {
        Self { item, count }
    }
}

/// A stack at a cell (its top-left corner), maybe turned sideways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub stack: Stack,
    pub x: u8,
    pub y: u8,
    pub rotated: bool,
}

/// Cells an item covers, (width, height), turned or not.
pub fn dims(items: &Items, item: ItemId, rotated: bool) -> (u8, u8) {
    let (w, h) = items.get(item).size;
    if rotated { (h, w) } else { (w, h) }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub w: u8,
    pub h: u8,
    pub items: Vec<Placed>,
}

impl Grid {
    pub fn new(w: u8, h: u8) -> Self {
        Self { w, h, items: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The item covering a cell.
    pub fn at(&self, items: &Items, x: u8, y: u8) -> Option<usize> {
        self.items.iter().position(|p| {
            let (w, h) = dims(items, p.stack.item, p.rotated);
            x >= p.x && x < p.x + w && y >= p.y && y < p.y + h
        })
    }

    /// Would an item fit with its corner at (x, y)? `ignore` skips one placed item.
    pub fn fits(&self, items: &Items, item: ItemId, x: u8, y: u8, rotated: bool, ignore: Option<usize>) -> bool {
        let (w, h) = dims(items, item, rotated);
        if x as u16 + w as u16 > self.w as u16 || y as u16 + h as u16 > self.h as u16 {
            return false;
        }
        !self.items.iter().enumerate().any(|(i, p)| {
            if Some(i) == ignore {
                return false;
            }
            let (pw, ph) = dims(items, p.stack.item, p.rotated);
            x < p.x + pw && p.x < x + w && y < p.y + ph && p.y < y + h
        })
    }

    /// Put a stack exactly here, if it fits.
    pub fn place(&mut self, items: &Items, stack: Stack, x: u8, y: u8, rotated: bool) -> bool {
        if stack.count == 0 || !self.fits(items, stack.item, x, y, rotated, None) {
            return false;
        }
        self.items.push(Placed { stack, x, y, rotated });
        true
    }

    /// The first free spot for an item, trying it upright then sideways.
    pub fn free_spot(&self, items: &Items, item: ItemId) -> Option<(u8, u8, bool)> {
        for rotated in [false, true] {
            for y in 0..self.h {
                for x in 0..self.w {
                    if self.fits(items, item, x, y, rotated, None) {
                        return Some((x, y, rotated));
                    }
                }
            }
        }
        None
    }

    /// Add a stack wherever it goes: topping up stacks of the same item
    /// first, then into free space. Returns what didn't fit.
    pub fn add(&mut self, items: &Items, mut stack: Stack) -> Option<Stack> {
        let max = items.get(stack.item).stack;
        for p in self.items.iter_mut().filter(|p| p.stack.item == stack.item) {
            let room = max.saturating_sub(p.stack.count).min(stack.count);
            p.stack.count += room;
            stack.count -= room;
        }
        while stack.count > 0 {
            let Some((x, y, rotated)) = self.free_spot(items, stack.item) else { return Some(stack) };
            let n = stack.count.min(max);
            self.items.push(Placed { stack: Stack::new(stack.item, n), x, y, rotated });
            stack.count -= n;
        }
        None
    }

    pub fn count(&self, item: ItemId) -> u32 {
        self.items.iter().filter(|p| p.stack.item == item).map(|p| p.stack.count as u32).sum()
    }

    /// Take up to `n` of an item, smallest stacks first. Returns how many.
    pub fn remove(&mut self, item: ItemId, n: u32) -> u32 {
        let mut left = n;
        let mut order: Vec<usize> = (0..self.items.len()).filter(|i| self.items[*i].stack.item == item).collect();
        order.sort_by_key(|i| self.items[*i].stack.count);
        for i in order {
            if left == 0 {
                break;
            }
            let p = &mut self.items[i];
            let take = (p.stack.count as u32).min(left);
            p.stack.count -= take as u16;
            left -= take;
        }
        self.items.retain(|p| p.stack.count > 0);
        n - left
    }

    pub fn mass(&self, items: &Items) -> f32 {
        self.items.iter().map(|p| items.get(p.stack.item).mass * p.stack.count as f32).sum()
    }

    pub fn to_save(&self, items: &Items) -> GridSave {
        GridSave {
            w: self.w,
            h: self.h,
            items: self
                .items
                .iter()
                .map(|p| PlacedSave { item: items.get(p.stack.item).name.clone(), count: p.stack.count, x: p.x, y: p.y, rotated: p.rotated })
                .collect(),
        }
    }

    /// Rebuild from a save. Unknown items are skipped; items that no longer
    /// fit where they were are packed in elsewhere, or returned.
    pub fn from_save(save: &GridSave, items: &Items, w: u8, h: u8) -> (Self, Vec<Stack>) {
        let mut g = Grid::new(w, h);
        let mut spill = Vec::new();
        for p in &save.items {
            let Some(id) = items.id(&p.item) else { continue };
            let stack = Stack::new(id, p.count.min(items.get(id).stack));
            if !g.place(items, stack, p.x, p.y, p.rotated)
                && let Some(left) = g.add(items, stack)
            {
                spill.push(left);
            }
        }
        (g, spill)
    }
}

/// A grid as saved: items by name, so saves survive reordering items.ron.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GridSave {
    pub w: u8,
    pub h: u8,
    pub items: Vec<PlacedSave>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlacedSave {
    pub item: String,
    pub count: u16,
    pub x: u8,
    pub y: u8,
    #[serde(default)]
    pub rotated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::tests::load;

    #[test]
    fn items_take_their_size_and_rotate() {
        let (_, items) = load();
        let pick = items.id("stone_pickaxe").unwrap(); // 2x3
        let mut g = Grid::new(4, 2);
        // Upright it's too tall for a 2-high grid; sideways it fits.
        assert!(!g.fits(&items, pick, 0, 0, false, None));
        assert!(g.fits(&items, pick, 0, 0, true, None));
        assert_eq!(g.add(&items, Stack::new(pick, 1)), None);
        assert!(g.items[0].rotated);
        // 3x2 used; a second pickaxe has no room.
        assert_eq!(g.add(&items, Stack::new(pick, 1)), Some(Stack::new(pick, 1)));
        assert_eq!(g.at(&items, 2, 1), Some(0));
        assert_eq!(g.at(&items, 3, 0), None);
    }

    #[test]
    fn stacks_top_up_then_spill() {
        let (_, items) = load();
        let dirt = items.id("dirt").unwrap(); // stacks of 32
        let mut g = Grid::new(2, 1);
        assert_eq!(g.add(&items, Stack::new(dirt, 40)), None);
        assert_eq!(g.items.len(), 2);
        assert_eq!(g.add(&items, Stack::new(dirt, 30)), Some(Stack::new(dirt, 6)));
        assert_eq!(g.count(dirt), 64);
        assert_eq!(g.remove(dirt, 50), 50);
        assert_eq!(g.count(dirt), 14);
        assert_eq!(g.items.len(), 1);
    }

    #[test]
    fn saves_round_trip() {
        let (_, items) = load();
        let mut g = Grid::new(5, 4);
        g.add(&items, Stack::new(items.id("stone_axe").unwrap(), 1));
        g.add(&items, Stack::new(items.id("torch").unwrap(), 20));
        let (back, spill) = Grid::from_save(&g.to_save(&items), &items, 5, 4);
        assert!(spill.is_empty());
        assert_eq!(back, g);
    }
}
