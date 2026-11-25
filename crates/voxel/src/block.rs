//! Block ids and the data-driven block registry (`assets/data/blocks.ron`).

use serde::Deserialize;
use std::collections::HashMap;

use crate::TileSheet;

/// A block type. Index into the registry. `BlockId::AIR` is always 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u16);

impl BlockId {
    pub const AIR: BlockId = BlockId(0);

    pub fn is_air(self) -> bool {
        self == Self::AIR
    }
}

/// One entry of `blocks.ron`.
#[derive(Debug, Clone, Deserialize)]
struct BlockSpec {
    name: String,
    #[serde(default)]
    top: Option<String>,
    #[serde(default)]
    bottom: Option<String>,
    #[serde(default)]
    side: Option<String>,
    #[serde(default = "yes")]
    solid: bool,
    #[serde(default = "yes")]
    opaque: bool,
    #[serde(default)]
    translucent: bool,
    #[serde(default)]
    foliage: bool,
    #[serde(default = "one")]
    hardness: f32,
    #[serde(default = "generic")]
    material: String,
    #[serde(default)]
    shape: Shape,
    /// For vines and twigs: the side of the cell they cling to.
    #[serde(default)]
    attach: Option<Side>,
    /// For `shape: model`: which box model it is drawn as.
    #[serde(default)]
    model: Option<Model>,
    /// Block light it gives off, 0..=15 (torches, campfires).
    #[serde(default)]
    light: u8,
    /// For `shape: layer`: how many eighths of the cell it fills (1..=8).
    #[serde(default)]
    level: u8,
    /// Logs: the way the grain runs (y = standing, x/z = lying on its side).
    #[serde(default)]
    axis: Axis,
    /// Part of a tree or giant mushroom (`trunk` holds it up, `crown` hangs off
    /// it); a trunk cut through topples the lot (`crate::fell`).
    #[serde(default)]
    tree: Option<TreePart>,
}

/// The direction a log's grain runs: its end tiles (`top`) face both ways along it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    #[default]
    Y,
    Z,
}

impl Axis {
    pub fn index(self) -> usize {
        match self {
            Axis::X => 0,
            Axis::Y => 1,
            Axis::Z => 2,
        }
    }
}

/// What a block is to a tree (see `tree` in blocks.ron).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TreePart {
    /// Logs and stems: they hold the tree up and fall with it.
    Trunk,
    /// Leaves and caps: they fall with the trunk nearest them.
    Crown,
}

/// Small furniture drawn as a few boxes in the chunk's detail mesh
/// (`shape: model`); their facing comes from the solid blocks beside them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    Chair,
    Table,
    /// A door panel across the gap between two walls.
    Door,
    /// The same door swung open against its hinge side.
    DoorOpen,
}

/// One of the four sides of a cell (north is -Z, east is +X).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    North,
    South,
    East,
    West,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::North, Side::South, Side::East, Side::West];

    /// Step from the cell to the neighbour on this side.
    pub fn offset(self) -> [i32; 3] {
        match self {
            Side::North => [0, 0, -1],
            Side::South => [0, 0, 1],
            Side::East => [1, 0, 0],
            Side::West => [-1, 0, 0],
        }
    }
}

fn one() -> f32 {
    1.0
}

fn generic() -> String {
    "generic".into()
}

/// How a block is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// A full textured cube, meshed with the chunk.
    #[default]
    Cube,
    /// Not meshed with the chunk; the game draws it as its own small model
    /// (and gives it a light).
    Torch,
    /// Two crossed see-through quads (grass, reeds, ferns), meshed with the
    /// chunk into their own mesh; the top sways in the wind.
    Plant,
    /// A flat hanging panel against one side of the cell (`attach`); its lower
    /// edge sways. Held up by what it clings to, or by the same vine above.
    Vine,
    /// A short stick poking out of the side it's attached to (`attach`), such
    /// as a twig on a trunk. Snaps off by hand.
    Twig,
    /// A small stone lying on the block below.
    Pebble,
    /// Furniture (`model`: chair, table, door): a few boxes meshed with the
    /// details; it collides as a whole block if `solid`.
    Model,
    /// A flat layer on the block below, `level` eighths of the cell high:
    /// snow cover (drawn with the details) and puddles (`translucent`, drawn
    /// with the water). See `game::cover`.
    Layer,
}

impl Shape {
    /// Small details drawn with the plants mesh (not full cubes, not torches).
    pub fn is_detail(self) -> bool {
        matches!(self, Shape::Plant | Shape::Vine | Shape::Twig | Shape::Pebble)
    }
}

fn yes() -> bool {
    true
}

/// A resolved block definition.
#[derive(Debug, Clone)]
pub struct BlockDef {
    pub name: String,
    /// Texture layer per face, indexed by [`crate::Face`] order: +X, -X, +Y, -Y, +Z, -Z.
    pub textures: [u32; 6],
    pub solid: bool,
    pub opaque: bool,
    /// Drawn in the blended pass (water, ice).
    pub translucent: bool,
    /// Takes the season's colour (autumn leaves, winter frost): every face of a
    /// non-opaque block such as leaves, only the top of an opaque one such as grass.
    pub foliage: bool,
    /// Seconds to break with bare hands, before tool and material speeds.
    pub hardness: f32,
    /// Material class tools are rated against ("stone", "wood", "soil", ...).
    pub material: String,
    pub shape: Shape,
    /// Vines and twigs: the side they cling to.
    pub attach: Option<Side>,
    /// `shape: model`: which model.
    pub model: Option<Model>,
    /// Block light it gives off, 0..=15 (see `crate::light`).
    pub light: u8,
    /// `shape: layer`: eighths of the cell filled, 1..=8 (0 for other shapes).
    pub level: u8,
    /// Logs: the way the grain runs. The mesher turns side tiles to follow it.
    pub axis: Axis,
    /// Part of a tree (`crate::fell`), or not.
    pub tree: Option<TreePart>,
}

impl BlockDef {
    /// Whether it can be part of a room's walls, floor or roof: a solid full
    /// cube that isn't foliage. Leaves, plants, water and torches are holes.
    pub fn is_wall(&self) -> bool {
        self.solid && matches!(self.shape, Shape::Cube | Shape::Model) && self.material != "plant"
    }

    /// Offsets to the blocks that can hold this one up (any one is enough):
    /// the ground for plants and pebbles, the attached side for twigs, and for
    /// vines the attached side or the vine above. Empty = needs no support.
    pub fn supports(&self) -> Vec<[i32; 3]> {
        match (self.shape, self.attach) {
            (Shape::Plant | Shape::Pebble | Shape::Layer, _) => vec![[0, -1, 0]],
            (Shape::Twig, Some(a)) => vec![a.offset()],
            (Shape::Vine, Some(a)) => vec![a.offset(), [0, 1, 0]],
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlockRegistry {
    defs: Vec<BlockDef>,
    by_name: HashMap<String, BlockId>,
}

#[derive(Debug)]
pub enum RegistryError {
    Parse(ron::error::SpannedError),
    UnknownTile { block: String, tile: String },
    Duplicate(String),
    FirstMustBeAir,
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "blocks.ron: {e}"),
            Self::UnknownTile { block, tile } => {
                write!(f, "block '{block}' uses tile '{tile}', which is not in textures/blocks.ron")
            }
            Self::Duplicate(n) => write!(f, "block '{n}' is defined twice"),
            Self::FirstMustBeAir => write!(f, "the first block in blocks.ron must be 'air'"),
        }
    }
}

impl std::error::Error for RegistryError {}

impl BlockRegistry {
    pub fn from_ron(text: &str, tiles: &TileSheet) -> Result<Self, RegistryError> {
        let specs: Vec<BlockSpec> = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
            .map_err(RegistryError::Parse)?;
        if specs.first().map(|s| s.name.as_str()) != Some("air") {
            return Err(RegistryError::FirstMustBeAir);
        }
        let mut defs = Vec::with_capacity(specs.len());
        let mut by_name = HashMap::new();
        for (i, s) in specs.into_iter().enumerate() {
            let tile = |name: &Option<String>| -> Result<u32, RegistryError> {
                match name {
                    None => Ok(0),
                    Some(t) => tiles.index(t).ok_or_else(|| RegistryError::UnknownTile {
                        block: s.name.clone(),
                        tile: t.clone(),
                    }),
                }
            };
            let top = tile(&s.top)?;
            let bottom = if s.bottom.is_some() { tile(&s.bottom)? } else { top };
            let side = if s.side.is_some() { tile(&s.side)? } else { top };
            if by_name.insert(s.name.clone(), BlockId(i as u16)).is_some() {
                return Err(RegistryError::Duplicate(s.name));
            }
            // End tiles face along the grain: +axis gets `top`, -axis `bottom`.
            let textures = match s.axis {
                Axis::X => [top, bottom, side, side, side, side],
                Axis::Y => [side, side, top, bottom, side, side],
                Axis::Z => [side, side, side, side, top, bottom],
            };
            defs.push(BlockDef {
                name: s.name,
                textures,
                solid: s.solid,
                opaque: s.opaque,
                translucent: s.translucent,
                foliage: s.foliage,
                hardness: s.hardness,
                material: s.material,
                shape: s.shape,
                attach: s.attach,
                model: s.model,
                light: s.light.min(crate::light::MAX),
                level: if s.shape == Shape::Layer { s.level.clamp(1, 8) } else { 0 },
                axis: s.axis,
                tree: s.tree,
            });
        }
        Ok(Self { defs, by_name })
    }

    pub fn get(&self, id: BlockId) -> &BlockDef {
        &self.defs[id.0 as usize]
    }

    pub fn id(&self, name: &str) -> Option<BlockId> {
        self.by_name.get(name).copied()
    }

    /// Like [`Self::id`] but panics with a clear message. For names hard-coded in the game.
    pub fn expect(&self, name: &str) -> BlockId {
        self.id(name).unwrap_or_else(|| panic!("block '{name}' missing from blocks.ron"))
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (BlockId, &BlockDef)> {
        self.defs.iter().enumerate().map(|(i, d)| (BlockId(i as u16), d))
    }

    /// Per-block opaque flags, indexed by id. Cheap lookup table for the mesher.
    pub fn opaque_table(&self) -> Vec<bool> {
        self.defs.iter().map(|d| d.opaque).collect()
    }

    /// Per-block tables for the light flood fill (`crate::light`).
    pub fn light_tables(&self) -> crate::light::Tables {
        crate::light::Tables {
            // Opaque blocks and shut doors stop light; glass, leaves, water and plants let it through.
            blocks: self.defs.iter().map(|d| d.opaque || (d.solid && d.model == Some(Model::Door))).collect(),
            emit: self.defs.iter().map(|d| d.light).collect(),
        }
    }
}

#[cfg(test)]
pub(crate) fn test_registry() -> BlockRegistry {
    let tiles = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
    BlockRegistry::from_ron(include_str!("../../../assets/data/blocks.ron"), &tiles).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_game_blocks() {
        let reg = test_registry();
        assert_eq!(reg.id("air"), Some(BlockId::AIR));
        let grass = reg.get(reg.expect("grass"));
        assert_ne!(grass.textures[2], grass.textures[3], "grass top and bottom differ");
        assert!(!reg.get(reg.expect("water")).solid);
        assert!(!reg.get(reg.expect("leaves")).opaque);
        assert!(reg.get(reg.expect("water")).translucent);
        assert!(!reg.get(reg.expect("leaves")).translucent);
    }

    #[test]
    fn unknown_tile_is_an_error() {
        let tiles = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
        let err = BlockRegistry::from_ron(r#"[(name: "air"), (name: "x", top: "nope")]"#, &tiles);
        assert!(matches!(err, Err(RegistryError::UnknownTile { .. })));
    }
}
