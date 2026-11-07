//! Food tiers from `assets/data/food_tiers.ron`: what eating each tier heals.

use std::collections::HashMap;

use serde::Deserialize;

use super::health::Layer;

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct Buff {
    pub speed: f32,
    pub seconds: f32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct FoodTier {
    pub tier: u8,
    pub title: String,
    #[serde(default)]
    pub heals: HashMap<Layer, f32>,
    #[serde(default)]
    pub ignore_cap: bool,
    #[serde(default)]
    pub buff: Option<Buff>,
}

#[derive(bevy::prelude::Resource, Debug, Clone)]
pub struct FoodTiers {
    list: Vec<FoodTier>,
}

impl FoodTiers {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let list: Vec<FoodTier> = ron::from_str(text).map_err(|e| format!("food_tiers.ron: {e}"))?;
        Ok(Self { list })
    }

    pub fn get(&self, tier: u8) -> Option<&FoodTier> {
        self.list.iter().find(|t| t.tier == tier)
    }
}
