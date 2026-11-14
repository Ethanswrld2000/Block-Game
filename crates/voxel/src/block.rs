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
}

impl BlockDef {
    /// Offsets to the blocks that can hold this one up (any one is enough):
    /// the ground for plants and pebbles, the attached side for twigs, and for
    /// vines the attached side or the vine above. Empty = needs no support.
    pub fn supports(&self) -> Vec<[i32; 3]> {
        match (self.shape, self.attach) {
            (Shape::Plant | Shape::Pebble, _) => vec![[0, -1, 0]],
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
            defs.push(BlockDef {
                name: s.name,
                textures: [side, side, top, bottom, side, side],
                solid: s.solid,
                opaque: s.opaque,
                translucent: s.translucent,
                foliage: s.foliage,
                hardness: s.hardness,
                material: s.material,
                shape: s.shape,
                attach: s.attach,
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
