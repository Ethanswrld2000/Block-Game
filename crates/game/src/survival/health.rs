//! Layered health: four bars, outermost first, Damage -> Fatigue -> Blood ->
//! Injuries. You die when Injuries reaches 0.
//!
//! - A hit starts at its entry layer (normal hits at Damage, stabs at Blood,
//!   heavy blunt trauma at Injuries, cold and exhaustion at Fatigue) and any
//!   overflow spills into the next bar down.
//! - Mortal rule: if a bar below Damage drops under half, that layer is
//!   mortal, every bar above it is emptied, and new hits land on it.
//! - Healing cap: while a layer is mortal nothing above it can be restored.
//!   Getting the mortal layer back over half lifts the cap.
//!
//! Plain Rust with no Bevy, so a server can run the same rules.

use serde::{Deserialize, Serialize};

pub const MAX: f32 = 100.0;
/// Below this share of a bar, the layer is mortal.
pub const MORTAL_AT: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Layer {
    Damage = 0,
    Fatigue = 1,
    Blood = 2,
    Injuries = 3,
}

impl Layer {
    pub const ALL: [Layer; 4] = [Layer::Damage, Layer::Fatigue, Layer::Blood, Layer::Injuries];

    pub fn name(self) -> &'static str {
        match self {
            Layer::Damage => "Damage",
            Layer::Fatigue => "Fatigue",
            Layer::Blood => "Blood",
            Layer::Injuries => "Injuries",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Health {
    /// Indexed by `Layer as usize`, each 0..=MAX.
    pub bars: [f32; 4],
    /// Blood lost per second from open wounds.
    pub bleeding: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self { bars: [MAX; 4], bleeding: 0.0 }
    }
}

impl Health {
    pub fn get(&self, l: Layer) -> f32 {
        self.bars[l as usize]
    }

    pub fn is_dead(&self) -> bool {
        self.bars[Layer::Injuries as usize] <= 0.0
    }

    /// The deepest layer (below Damage) that is under half, if any.
    pub fn mortal(&self) -> Option<Layer> {
        [Layer::Injuries, Layer::Blood, Layer::Fatigue].into_iter().find(|l| self.get(*l) < MAX * MORTAL_AT)
    }

    /// Empty every bar above the mortal layer.
    fn apply_mortal_rule(&mut self) {
        if let Some(m) = self.mortal() {
            for l in 0..m as usize {
                self.bars[l] = 0.0;
            }
        }
    }

    /// Take a hit that starts at `entry`. Returns how much was absorbed.
    pub fn hit(&mut self, entry: Layer, amount: f32) -> f32 {
        if amount <= 0.0 || self.is_dead() {
            return 0.0;
        }
        // With a mortal layer, hits land there (or deeper, if they start deeper).
        let start = self.mortal().map_or(entry, |m| m.max(entry)) as usize;
        let mut left = amount;
        for l in start..4 {
            let take = self.bars[l].min(left);
            self.bars[l] -= take;
            left -= take;
            if left <= 0.0 {
                break;
            }
        }
        self.apply_mortal_rule();
        amount - left
    }

    /// Restore a layer, unless the healing cap blocks it. Returns how much it rose.
    pub fn heal(&mut self, layer: Layer, amount: f32) -> f32 {
        if self.is_dead() || amount <= 0.0 {
            return 0.0;
        }
        if let Some(m) = self.mortal()
            && layer < m
        {
            return 0.0;
        }
        let before = self.get(layer);
        self.bars[layer as usize] = (before + amount).min(MAX);
        self.bars[layer as usize] - before
    }

    /// Restore a layer even past the cap (the best food and treatments).
    pub fn heal_any(&mut self, layer: Layer, amount: f32) {
        if !self.is_dead() {
            self.bars[layer as usize] = (self.get(layer) + amount).min(MAX);
        }
    }

    /// Open wounds lose blood over time; this is a Blood-layer hit.
    pub fn bleed(&mut self, dt: f32) {
        if self.bleeding > 0.0 {
            self.hit(Layer::Blood, self.bleeding * dt);
            // Wounds close slowly on their own.
            self.bleeding = (self.bleeding - dt * 0.02).max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_hits_drain_damage_then_spill_down() {
        let mut h = Health::default();
        h.hit(Layer::Damage, 30.0);
        assert_eq!(h.bars, [70.0, 100.0, 100.0, 100.0]);
        h.hit(Layer::Damage, 90.0);
        assert_eq!(h.bars, [0.0, 80.0, 100.0, 100.0]);
    }

    #[test]
    fn piercing_starts_at_blood() {
        let mut h = Health::default();
        h.hit(Layer::Blood, 20.0);
        assert_eq!(h.bars, [100.0, 100.0, 80.0, 100.0]);
    }

    #[test]
    fn mortal_rule_empties_the_bars_above() {
        // The plan's example: Blood falls to 45%, so Damage and Fatigue go to 0.
        let mut h = Health::default();
        h.hit(Layer::Blood, 55.0);
        assert_eq!(h.mortal(), Some(Layer::Blood));
        assert_eq!(h.bars, [0.0, 0.0, 45.0, 100.0]);
        // Every new hit lands on Blood, even a normal one.
        h.hit(Layer::Damage, 5.0);
        assert_eq!(h.bars, [0.0, 0.0, 40.0, 100.0]);
        // A deeper hit still starts deeper.
        h.hit(Layer::Injuries, 10.0);
        assert_eq!(h.get(Layer::Injuries), 90.0);
    }

    #[test]
    fn healing_cap_until_the_mortal_layer_is_treated() {
        let mut h = Health::default();
        h.hit(Layer::Blood, 55.0);
        assert_eq!(h.heal(Layer::Damage, 50.0), 0.0);
        assert_eq!(h.heal(Layer::Fatigue, 50.0), 0.0);
        // Treat Blood back over half: now Fatigue (empty) is the deepest
        // layer under half, so it must be rested before Damage heals.
        h.heal(Layer::Blood, 20.0);
        assert_eq!(h.mortal(), Some(Layer::Fatigue));
        assert_eq!(h.heal(Layer::Damage, 10.0), 0.0);
        h.heal(Layer::Fatigue, 60.0);
        assert_eq!(h.mortal(), None);
        assert_eq!(h.heal(Layer::Damage, 10.0), 10.0);
    }

    #[test]
    fn death_when_injuries_empty() {
        let mut h = Health::default();
        h.hit(Layer::Injuries, 100.0);
        assert!(h.is_dead());
        let mut h = Health::default();
        h.hit(Layer::Damage, 400.0);
        assert!(h.is_dead());
    }

    #[test]
    fn bleeding_drains_blood() {
        let mut h = Health { bleeding: 2.0, ..Default::default() };
        for _ in 0..10 {
            h.bleed(1.0);
        }
        assert!(h.get(Layer::Blood) < 82.0 && h.get(Layer::Blood) > 78.0);
        assert!(h.bleeding < 2.0);
    }
}
