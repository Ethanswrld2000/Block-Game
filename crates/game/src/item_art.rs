//! Item pictures, from three sprite sheets in `assets/textures/icons/`:
//! `items.png` (food, materials, clothes, bags, torches), `weapons.png`
//! (tools and weapons) and `blocks.png` (blocks, drawn as little cubes). Each
//! has a `.ron` listing where every item's sprite sits; sprites are
//! `CELL` (20) pixels per inventory cell, so a 2x3 item is 40 x 60.
//! `tools/gen_item_art.py` draws placeholders for items without one.
//!
//! The inventory and hotbar draw the sprite at 2x. In 3D (held in your
//! hand, lying on the ground) each opaque pixel becomes a little slab, so
//! repainting a sprite also changes the model.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use serde::Deserialize;

use crate::items::{ItemId, ItemKind, Items};

/// Sprite pixels per inventory cell.
pub const CELL: u32 = 20;

/// Which sheet an item's sprite is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    Items,
    Weapons,
    Blocks,
}

impl Sheet {
    pub const ALL: [Sheet; 3] = [Sheet::Items, Sheet::Weapons, Sheet::Blocks];

    pub fn of(kind: &ItemKind) -> Sheet {
        match kind {
            ItemKind::Tool { .. } => Sheet::Weapons,
            ItemKind::Block(_) => Sheet::Blocks,
            _ => Sheet::Items,
        }
    }

    fn ron(self) -> &'static str {
        match self {
            Sheet::Items => include_str!("../../../assets/textures/icons/items.ron"),
            Sheet::Weapons => include_str!("../../../assets/textures/icons/weapons.ron"),
            Sheet::Blocks => include_str!("../../../assets/textures/icons/blocks.ron"),
        }
    }

    fn path(self) -> &'static str {
        match self {
            Sheet::Items => "textures/icons/items.png",
            Sheet::Weapons => "textures/icons/weapons.png",
            Sheet::Blocks => "textures/icons/blocks.png",
        }
    }
}

#[derive(Deserialize)]
struct SheetSpec {
    #[allow(dead_code)]
    cell: u32,
    sprites: HashMap<String, (u32, u32, u32, u32)>,
}

/// Where each item's sprite is: the sheet, and its rectangle in pixels.
pub fn layout(items: &Items) -> Vec<Option<(Sheet, URect)>> {
    let specs: Vec<(Sheet, SheetSpec)> =
        Sheet::ALL.iter().map(|&s| (s, ron::from_str(s.ron()).unwrap_or_else(|e| panic!("{}: {e}", s.path())))).collect();
    items
        .ids()
        .map(|id| {
            let item = items.get(id);
            let sheet = Sheet::of(&item.kind);
            let spec = &specs.iter().find(|(s, _)| *s == sheet)?.1;
            let (x, y, w, h) = *spec.sprites.get(&item.name)?;
            Some((sheet, URect::new(x, y, x + w, y + h)))
        })
        .collect()
}

pub struct ItemArtPlugin;

impl Plugin for ItemArtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::menu::GameStartup, setup).add_systems(crate::menu::GameUpdate, build_models);
    }
}

/// The sheets, where each sprite is, and (once the sheets have loaded) a 3D
/// model made from each sprite.
#[derive(Resource)]
pub struct ItemArt {
    sheets: [Handle<Image>; 3],
    sprites: Vec<Option<(Sheet, URect)>>,
    models: Vec<Option<Handle<Mesh>>>,
    materials: [Handle<StandardMaterial>; 3],
    /// The models are built.
    pub ready: bool,
}

impl ItemArt {
    fn sheet(&self, s: Sheet) -> usize {
        Sheet::ALL.iter().position(|&x| x == s).unwrap()
    }

    /// The sprite's size in sheet pixels.
    pub fn size(&self, id: ItemId) -> Option<UVec2> {
        self.sprites.get(id.0 as usize).copied().flatten().map(|(_, r)| r.size())
    }

    /// The sprite as a UI image.
    pub fn image(&self, id: ItemId) -> Option<ImageNode> {
        let (sheet, r) = self.sprites.get(id.0 as usize).copied().flatten()?;
        Some(ImageNode { image: self.sheets[self.sheet(sheet)].clone(), rect: Some(r.as_rect()), ..default() })
    }

    /// The 3D model made from the sprite, one unit per pixel, centred, the
    /// sprite's up along +Y and its face towards +Z.
    pub fn model(&self, id: ItemId) -> Option<(Handle<Mesh>, Handle<StandardMaterial>)> {
        let mesh = self.models.get(id.0 as usize).cloned().flatten()?;
        let (sheet, _) = self.sprites[id.0 as usize]?;
        Some((mesh, self.materials[self.sheet(sheet)].clone()))
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>, items: Res<Items>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let sprites = layout(&items);
    for id in items.ids() {
        let item = items.get(id);
        match sprites[id.0 as usize] {
            None => warn!("item '{}' has no sprite; run tools/gen_item_art.py", item.name),
            Some((_, r)) if r.size() != UVec2::new(item.size.0 as u32, item.size.1 as u32) * CELL => {
                warn!("item '{}' takes {}x{} cells, so its sprite should be {}x{} px", item.name, item.size.0, item.size.1, item.size.0 as u32 * CELL, item.size.1 as u32 * CELL)
            }
            _ => {}
        }
    }
    let sheets = Sheet::ALL.map(|s| assets.load(s.path()));
    let materials = sheets.clone().map(|image| {
        materials.add(StandardMaterial { base_color_texture: Some(image), perceptual_roughness: 0.9, reflectance: 0.2, ..default() })
    });
    let models = vec![None; sprites.len()];
    commands.insert_resource(ItemArt { sheets, sprites, models, materials, ready: false });
}

fn build_models(art: Option<ResMut<ItemArt>>, images: Res<Assets<Image>>, mut meshes: ResMut<Assets<Mesh>>) {
    let Some(mut art) = art else { return };
    if art.ready {
        return;
    }
    let Some(sheets) = art.sheets.iter().map(|h| images.get(h)).collect::<Option<Vec<_>>>() else { return };
    let models = art
        .sprites
        .iter()
        .map(|s| {
            let (sheet, r) = (*s)?;
            let img = sheets[Sheet::ALL.iter().position(|&x| x == sheet).unwrap()];
            let size = img.size();
            let solid = |x: i32, y: i32| {
                x >= 0
                    && y >= 0
                    && (x as u32) < r.width()
                    && (y as u32) < r.height()
                    && img.get_color_at(r.min.x + x as u32, r.min.y + y as u32).is_ok_and(|c| c.alpha() > 0.5)
            };
            Some(meshes.add(extrude(r, size, solid)))
        })
        .collect();
    art.models = models;
    art.ready = true;
}

/// Thickness of a sprite's model, in pixels.
const DEPTH: f32 = 1.5;

/// A sprite turned into a model: the front and back faces of each row of
/// opaque pixels, and a side wherever an opaque pixel meets an empty one.
/// One unit per pixel, centred on the sprite.
pub fn extrude(r: URect, sheet: UVec2, solid: impl Fn(i32, i32) -> bool) -> Mesh {
    let (w, h) = (r.width() as i32, r.height() as i32);
    let (sw, sh) = (sheet.x as f32, sheet.y as f32);
    let (mut pos, mut nor, mut uv, mut idx) = (Vec::<[f32; 3]>::new(), Vec::<[f32; 3]>::new(), Vec::<[f32; 2]>::new(), Vec::<u32>::new());
    // Sprite pixel (x, y) (y down) to model space (y up), centred.
    let at = |x: f32, y: f32, z: f32| [x - w as f32 / 2.0, h as f32 / 2.0 - y, z];
    let tex = |x: f32, y: f32| [(r.min.x as f32 + x) / sw, (r.min.y as f32 + y) / sh];
    let mut quad = |corners: [[f32; 3]; 4], uvs: [[f32; 2]; 4], n: [f32; 3]| {
        let start = pos.len() as u32;
        pos.extend(corners);
        uv.extend(uvs);
        nor.extend([n; 4]);
        idx.extend([start, start + 1, start + 2, start, start + 2, start + 3]);
    };
    let z = DEPTH / 2.0;
    for y in 0..h {
        let mut x = 0;
        while x < w {
            if !solid(x, y) {
                x += 1;
                continue;
            }
            let x0 = x;
            while x < w && solid(x, y) {
                x += 1;
            }
            let (a, b, yf) = (x0 as f32, x as f32, y as f32);
            let uvs = [tex(a, yf + 1.0), tex(b, yf + 1.0), tex(b, yf), tex(a, yf)];
            // Front (counter-clockwise seen from +Z) and back.
            quad([at(a, yf + 1.0, z), at(b, yf + 1.0, z), at(b, yf, z), at(a, yf, z)], uvs, [0.0, 0.0, 1.0]);
            quad([at(b, yf + 1.0, -z), at(a, yf + 1.0, -z), at(a, yf, -z), at(b, yf, -z)], [uvs[1], uvs[0], uvs[3], uvs[2]], [0.0, 0.0, -1.0]);
        }
    }
    for y in 0..h {
        for x in 0..w {
            if !solid(x, y) {
                continue;
            }
            let (xf, yf) = (x as f32, y as f32);
            let c = tex(xf + 0.5, yf + 0.5);
            let uvs = [c; 4];
            if !solid(x - 1, y) {
                quad([at(xf, yf + 1.0, -z), at(xf, yf + 1.0, z), at(xf, yf, z), at(xf, yf, -z)], uvs, [-1.0, 0.0, 0.0]);
            }
            if !solid(x + 1, y) {
                quad([at(xf + 1.0, yf + 1.0, z), at(xf + 1.0, yf + 1.0, -z), at(xf + 1.0, yf, -z), at(xf + 1.0, yf, z)], uvs, [1.0, 0.0, 0.0]);
            }
            if !solid(x, y - 1) {
                quad([at(xf, yf, z), at(xf + 1.0, yf, z), at(xf + 1.0, yf, -z), at(xf, yf, -z)], uvs, [0.0, 1.0, 0.0]);
            }
            if !solid(x, y + 1) {
                quad([at(xf, yf + 1.0, -z), at(xf + 1.0, yf + 1.0, -z), at(xf + 1.0, yf + 1.0, z), at(xf, yf + 1.0, z)], uvs, [0.0, -1.0, 0.0]);
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_has_a_sprite_of_its_size() {
        let (_, items) = crate::items::tests::load();
        let sprites = layout(&items);
        for id in items.ids() {
            let item = items.get(id);
            let (_, r) = sprites[id.0 as usize].unwrap_or_else(|| panic!("{} has no sprite (run tools/gen_item_art.py)", item.name));
            assert_eq!(r.size(), UVec2::new(item.size.0 as u32 * CELL, item.size.1 as u32 * CELL), "{}: sprite size", item.name);
        }
    }

    #[test]
    fn sprites_sit_inside_their_sheets() {
        for sheet in Sheet::ALL {
            let spec: SheetSpec = ron::from_str(sheet.ron()).unwrap();
            let png = std::fs::read(format!("{}/../../assets/{}", env!("CARGO_MANIFEST_DIR"), sheet.path())).unwrap();
            // Width and height from the PNG header.
            let (w, h) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
            for (name, (x, y, sw, sh)) in spec.sprites {
                assert!(x + sw <= w && y + sh <= h, "{name} runs off {}", sheet.path());
            }
        }
    }

    #[test]
    fn a_square_extrudes_to_a_box() {
        let m = extrude(URect::new(0, 0, 2, 2), UVec2::new(2, 2), |x, y| (0..2).contains(&x) && (0..2).contains(&y));
        // Two rows front and back, plus 8 side quads round the edge.
        assert_eq!(m.count_vertices(), (4 + 8) * 4);
    }
}
