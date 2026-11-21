//! The block spritesheet's index file (`assets/textures/blocks.ron`).

use serde::Deserialize;
use std::collections::BTreeMap;

/// Maps tile names to their index in the 32x32-tile spritesheet.
#[derive(Debug, Clone, Deserialize)]
pub struct TileSheet {
    pub tile_size: u32,
    pub columns: u32,
    pub tiles: BTreeMap<String, u32>,
    /// Extra looks for a tile, picked per block so the ground doesn't show a grid.
    #[serde(default)]
    pub variants: BTreeMap<String, TileVariants>,
}

/// Other versions of one tile. The terrain shader picks one per block from the
/// block's position (the base tile counts as one of them).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TileVariants {
    /// Tile names; they must sit at consecutive indices in the sheet.
    #[serde(default)]
    pub tiles: Vec<String>,
    /// Also turn the tile by a random quarter turn per block (only for tiles
    /// without a light direction painted in, like grass tops).
    #[serde(default)]
    pub turn: bool,
}

/// Layers in the variant table the terrain shader reads (a 16 x 16 sheet).
pub const VARIANT_SLOTS: usize = 256;

/// One packed entry per tile index for the terrain shader: bits 0-9 = index of
/// the first extra version, 10-13 = how many versions in all (base included),
/// bit 14 = turn. Tiles without variants are 0.
pub fn pack_variant(first: u32, count: u32, turn: bool) -> u32 {
    (first & 0x3ff) | ((count & 0xf) << 10) | ((turn as u32) << 14)
}

impl TileSheet {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn index(&self, name: &str) -> Option<u32> {
        self.tiles.get(name).copied()
    }

    /// The packed variant table (see `pack_variant`), one entry per tile index.
    pub fn variant_table(&self) -> Result<Vec<u32>, String> {
        let mut table = vec![0; VARIANT_SLOTS];
        for (base, v) in &self.variants {
            let at = self.index(base).ok_or_else(|| format!("variants: no tile called {base}"))?;
            let mut first = None;
            for (k, name) in v.tiles.iter().enumerate() {
                let i = self.index(name).ok_or_else(|| format!("variants of {base}: no tile called {name}"))?;
                let start = *first.get_or_insert(i);
                if i != start + k as u32 {
                    return Err(format!("variants of {base}: {name} must follow the one before it in the sheet"));
                }
            }
            let count = v.tiles.len() as u32 + 1;
            if count > 15 {
                return Err(format!("variants of {base}: at most 14 extra tiles"));
            }
            let slot = table.get_mut(at as usize).ok_or_else(|| format!("variants: {base} is past tile {VARIANT_SLOTS}"))?;
            *slot = pack_variant(first.unwrap_or(0), count, v.turn);
        }
        Ok(table)
    }

    /// UV rectangle `[u0, v0, u1, v1]` of a tile within a sheet of `rows` rows.
    pub fn uv_rect(&self, index: u32, rows: u32) -> [f32; 4] {
        let (col, row) = (index % self.columns, index / self.columns);
        let (w, h) = (1.0 / self.columns as f32, 1.0 / rows as f32);
        [col as f32 * w, row as f32 * h, (col + 1) as f32 * w, (row + 1) as f32 * h]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_generated_sheet() {
        let text = include_str!("../../../assets/textures/blocks.ron");
        let sheet = TileSheet::from_ron(text).unwrap();
        assert_eq!(sheet.tile_size, 32);
        assert_eq!(sheet.index("grass_top"), Some(0));
        assert!(sheet.index("torch").is_some());
    }

    #[test]
    fn generated_variants_are_valid() {
        let sheet = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
        let table = sheet.variant_table().unwrap();
        let stone = table[sheet.index("stone").unwrap() as usize];
        assert!((stone >> 10) & 0xf > 1, "stone has variants");
    }

    #[test]
    fn variants_pack_and_must_be_consecutive() {
        let text = r#"(tile_size: 32, columns: 16, tiles: {"a": 0, "a2": 5, "a3": 6, "b": 1, "b2": 8},
            variants: {"a": (tiles: ["a2", "a3"], turn: true), "b": (tiles: ["b2"])})"#;
        let sheet = TileSheet::from_ron(text).unwrap();
        let table = sheet.variant_table().unwrap();
        assert_eq!(table[0], pack_variant(5, 3, true));
        assert_eq!(table[1], pack_variant(8, 2, false));
        assert_eq!(table[2], 0);
        let gap = r#"(tile_size: 32, columns: 16, tiles: {"a": 0, "a2": 5, "a3": 7}, variants: {"a": (tiles: ["a2", "a3"])})"#;
        assert!(TileSheet::from_ron(gap).unwrap().variant_table().is_err());
    }

    #[test]
    fn uv_rect_of_second_row() {
        let sheet = TileSheet { tile_size: 32, columns: 16, tiles: BTreeMap::new(), variants: BTreeMap::new() };
        let [u0, v0, u1, v1] = sheet.uv_rect(17, 16);
        assert_eq!((u0, v0, u1, v1), (1.0 / 16.0, 1.0 / 16.0, 2.0 / 16.0, 2.0 / 16.0));
    }
}
