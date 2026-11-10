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
}

#[derive(Debug, Clone)]
pub struct Biomes {
    list: Vec<Biome>,
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
            });
        }
        let find = |n: &str| list.iter().position(|b| b.name == n).ok_or_else(|| format!("biomes.ron needs a '{n}' biome"));
        let sky = find("skyreach")?;
        let spawn = find("great_swamp")?;
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

    /// Pick a surface biome from climate and height above sea level.
    pub fn select(&self, temperature: f32, humidity: f32, height: i32) -> usize {
        let mut best = (f32::MAX, 0);
        for (i, b) in self.list.iter().enumerate() {
            if i == self.sky || height < b.min_height || height > b.max_height {
                continue;
            }
            let d = (temperature - b.temperature).powi(2) + (humidity - b.humidity).powi(2);
            if d < best.0 {
                best = (d, i);
            }
        }
        best.1
    }
}
