//! Data-driven biomes (`assets/data/biomes.ron`).

use serde::Deserialize;
use voxel::{BlockId, BlockRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TreeKind {
    None,
    Swamp,
    Oak,
    Pine,
    /// Giant mushrooms (stem and cap).
    Mushroom,
    /// Very large redwoods (2x2 trunks, up to ~46 blocks).
    Redwood,
}

/// Where a biome can appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    /// The main island.
    #[default]
    Main,
    /// The outer islands past the coast (they use `main` biomes if none are listed).
    Outer,
    /// Sky islands.
    Sky,
    /// Open air: the void between islands and the sky above the land.
    Air,
}

/// A biome placed once per world on the main island, whatever the climate there.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Guaranteed {
    /// Radius of the area in blocks (its edge wobbles a little).
    pub radius: f32,
    /// Its centre lies this many blocks from spawn, at least...
    pub near: f32,
    /// ...and at most.
    pub far: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct BiomeSpec {
    name: String,
    title: String,
    temperature: f32,
    humidity: f32,
    min_height: i32,
    max_height: i32,
    surface: String,
    subsurface: String,
    underwater: String,
    trees: TreeKind,
    tree_chance: f32,
    fog: (f32, f32, f32),
    #[serde(default = "one")]
    haze: f32,
    #[serde(default)]
    plants: Vec<PlantSpec>,
    #[serde(default)]
    bushes: f32,
    #[serde(default)]
    region: Region,
    #[serde(default)]
    guaranteed: Option<Guaranteed>,
}

#[derive(Debug, Clone, Deserialize)]
struct PlantSpec {
    block: String,
    chance: f32,
    #[serde(default)]
    shore: bool,
}

/// A ground plant a biome scatters on its surface.
#[derive(Debug, Clone, Copy)]
pub struct Plant {
    pub block: BlockId,
    /// Chance per surface block, before the patch noise (0..1).
    pub chance: f32,
    /// Only by the water: ground at most one block above sea level.
    pub shore: bool,
}

fn one() -> f32 {
    1.0
}

/// A resolved biome.
#[derive(Debug, Clone)]
pub struct Biome {
    pub name: String,
    /// Shown on screen when you arrive (phase 8 adds the title cards).
    pub title: String,
    pub temperature: f32,
    pub humidity: f32,
    pub min_height: i32,
    pub max_height: i32,
    pub surface: BlockId,
    pub subsurface: BlockId,
    pub underwater: BlockId,
    pub trees: TreeKind,
    pub tree_chance: f32,
    pub fog: [f32; 3],
    /// Multiplies how far you can see: below 1 is hazier (the swamp).
    pub haze: f32,
    /// Ground plants, tried in order.
    pub plants: Vec<Plant>,
    /// Chance per column of a leafy bush (a mound of the trees' leaves).
    pub bushes: f32,
    pub region: Region,
    /// Placed once per world on the main island; never picked by climate.
    pub guaranteed: Option<Guaranteed>,
}

#[derive(Debug, Clone)]
pub struct Biomes {
    list: Vec<Biome>,
    /// The default sky island biome (the first `sky` one).
    pub sky: usize,
    pub spawn: usize,
}

impl Biomes {
    pub fn from_ron(text: &str, blocks: &BlockRegistry) -> Result<Self, String> {
        let specs: Vec<BiomeSpec> = ron::from_str(text).map_err(|e| format!("biomes.ron: {e}"))?;
        let block = |biome: &str, name: &str| {
            blocks.id(name).ok_or_else(|| format!("biome '{biome}' uses unknown block '{name}'"))
        };
        let mut list = Vec::new();
        for s in specs {
            let plants = s
                .plants
                .iter()
                .map(|p| Ok(Plant { block: block(&s.name, &p.block)?, chance: p.chance, shore: p.shore }))
                .collect::<Result<Vec<_>, String>>()?;
            list.push(Biome {
                surface: block(&s.name, &s.surface)?,
                subsurface: block(&s.name, &s.subsurface)?,
                underwater: block(&s.name, &s.underwater)?,
                name: s.name,
                title: s.title,
                temperature: s.temperature,
                humidity: s.humidity,
                min_height: s.min_height,
                max_height: s.max_height,
                trees: s.trees,
                tree_chance: s.tree_chance,
                fog: [s.fog.0, s.fog.1, s.fog.2],
                haze: s.haze,
                plants,
                bushes: s.bushes,
                region: s.region,
                guaranteed: s.guaranteed,
            });
        }
        for b in &list {
            if b.guaranteed.is_some() && b.region != Region::Main {
                return Err(format!("biome '{}': only main island biomes can be guaranteed", b.name));
            }
        }
        if !list.iter().any(|b| b.region == Region::Main && b.guaranteed.is_none()) {
            return Err("biomes.ron needs a main island biome".into());
        }
        let sky = list.iter().position(|b| b.region == Region::Sky).ok_or("biomes.ron needs a sky biome")?;
        let spawn = list.iter().position(|b| b.name == "great_swamp").ok_or("biomes.ron needs a 'great_swamp' biome")?;
        Ok(Self { list, sky, spawn })
    }

    pub fn get(&self, i: usize) -> &Biome {
        &self.list[i]
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Is any biome listed for this region?
    pub fn has(&self, region: Region) -> bool {
        self.list.iter().any(|b| b.region == region && b.guaranteed.is_none())
    }

    /// Biomes placed once per world, by index.
    pub fn guaranteed(&self) -> impl Iterator<Item = (usize, Guaranteed)> + '_ {
        self.list.iter().enumerate().filter_map(|(i, b)| b.guaranteed.map(|g| (i, g)))
    }

    /// Pick a main island biome from climate and height above sea level.
    pub fn select(&self, temperature: f32, humidity: f32, height: i32) -> usize {
        self.select_in(Region::Main, temperature, humidity, height).unwrap_or(self.spawn)
    }

    /// Pick a biome of a region from climate and height (above sea level, or above
    /// the island's base off the main island). `None` if the region lists none
    /// that fit. Guaranteed biomes are never picked here.
    pub fn select_in(&self, region: Region, temperature: f32, humidity: f32, height: i32) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (i, b) in self.list.iter().enumerate() {
            if b.region != region || b.guaranteed.is_some() || height < b.min_height || height > b.max_height {
                continue;
            }
            let d = (temperature - b.temperature).powi(2) + (humidity - b.humidity).powi(2);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }
}
