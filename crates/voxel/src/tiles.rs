//! The block spritesheet's index file (`assets/textures/blocks.ron`).

use serde::Deserialize;
use std::collections::BTreeMap;

/// Maps tile names to their index in the 32x32-tile spritesheet.
#[derive(Debug, Clone, Deserialize)]
pub struct TileSheet {
    pub tile_size: u32,
    pub columns: u32,
    pub tiles: BTreeMap<String, u32>,
}

impl TileSheet {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    pub fn index(&self, name: &str) -> Option<u32> {
        self.tiles.get(name).copied()
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
    fn uv_rect_of_second_row() {
        let sheet = TileSheet { tile_size: 32, columns: 16, tiles: BTreeMap::new() };
        let [u0, v0, u1, v1] = sheet.uv_rect(17, 16);
        assert_eq!((u0, v0, u1, v1), (1.0 / 16.0, 1.0 / 16.0, 2.0 / 16.0, 2.0 / 16.0));
    }
}
