//! Experience and levels, from `assets/data/progression.ron`. Plain Rust.

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct XpTable {
    pub mine: f32,
    pub mine_cap: f32,
    pub kill: f32,
    pub craft: f32,
    pub village: f32,
    pub barrow: f32,
    pub region: f32,
    pub sky_island: f32,
}

#[derive(bevy::prelude::Resource, Deserialize, Debug, Clone, Copy)]
pub struct Progression {
    pub base: f32,
    pub step: f32,
    pub points: u32,
    pub start_points: u32,
    pub xp: XpTable,
}

impl Progression {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| format!("progression.ron: {e}"))
    }

    /// XP needed to go from `level` to the next.
    pub fn need(&self, level: u32) -> f32 {
        self.base + self.step * level as f32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Level {
    pub level: u32,
    /// XP into the current level.
    pub xp: f32,
}

impl Default for Level {
    fn default() -> Self {
        Self { level: 1, xp: 0.0 }
    }
}

impl Level {
    /// Add XP; returns how many levels were gained.
    pub fn add(&mut self, p: &Progression, xp: f32) -> u32 {
        self.xp += xp.max(0.0);
        let mut gained = 0;
        while self.xp >= p.need(self.level) {
            self.xp -= p.need(self.level);
            self.level += 1;
            gained += 1;
        }
        gained
    }

    /// 0..1 through the current level.
    pub fn fraction(&self, p: &Progression) -> f32 {
        (self.xp / p.need(self.level)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_get_longer() {
        let p = Progression::from_ron(include_str!("../../../../assets/data/progression.ron")).unwrap();
        let mut l = Level::default();
        assert_eq!(l.add(&p, p.need(1) - 1.0), 0);
        assert_eq!(l.add(&p, 1.0), 1);
        assert_eq!(l.level, 2);
        assert!(p.need(2) > p.need(1));
        // A big pile of XP can pass several levels at once.
        assert!(l.add(&p, 1000.0) >= 3);
        assert!(l.fraction(&p) < 1.0);
    }
}
