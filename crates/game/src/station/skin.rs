//! Station textures: `assets/textures/stations.png` holds one 32 px tile per
//! material (`stations.ron`: name -> tile index; placeholder from
//! `tools/gen_station_art.py`). Once the sheet loads it is cut into one
//! repeating texture per material, and every station box gets UVs at world
//! scale (`box_mesh`): one tile per block on every face, whatever the box's
//! size, so nothing stretches and the pixels match the terrain's.
//!
//! Tiles are painted in real colours (the box's material is white over them),
//! except `urn`, which is light grey and tinted by each urn's colour.
//! Materials without a tile (fire, brew, broth, crystal, spirit, marks) stay
//! flat colours.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::Deserialize;

use super::models::Mat;

/// Tiles per block: one 32 px tile covers one block, like the terrain.
pub const TILES_PER_BLOCK: f32 = 1.0;

#[derive(Deserialize)]
struct SheetSpec {
    tile_size: u32,
    columns: u32,
    tiles: HashMap<String, u32>,
}

/// The sheet, and once it has loaded, one texture per material name.
#[derive(Resource)]
pub struct StationArt {
    sheet: Handle<Image>,
    spec: SheetSpec,
    tiles: Option<HashMap<String, Handle<Image>>>,
}

impl StationArt {
    pub fn ready(&self) -> bool {
        self.tiles.is_some()
    }

    fn tile(&self, mat: Mat) -> Option<Handle<Image>> {
        self.tiles.as_ref()?.get(mat.tile_name()?).cloned()
    }
}

impl Mat {
    /// Its tile's name in stations.ron; `None` = a flat colour.
    pub fn tile_name(self) -> Option<&'static str> {
        Some(match self {
            Mat::Wood => "wood",
            Mat::WoodDark => "wood_dark",
            Mat::Groove => "groove",
            Mat::Stone => "stone",
            Mat::StoneDark => "stone_dark",
            Mat::Iron => "iron",
            Mat::Steel => "steel",
            Mat::Brass => "brass",
            Mat::Reed => "reed",
            Mat::Cloth => "cloth",
            Mat::Linen => "linen",
            Mat::Cord => "cord",
            Mat::Hide => "hide",
            Mat::Moss => "moss",
            Mat::Clay => "clay",
            Mat::Urn | Mat::UrnDark => "urn",
            Mat::Broth | Mat::MarkLit | Mat::MarkUnlit | Mat::Fire | Mat::Brew | Mat::Spirit | Mat::Crystal => return None,
        })
    }

    /// Tinted by the urn's colour (its tile is grey).
    fn tinted(self) -> bool {
        matches!(self, Mat::Urn | Mat::UrnDark)
    }
}

pub fn load_station_art(mut commands: Commands, assets: Res<AssetServer>) {
    let spec: SheetSpec = ron::from_str(include_str!("../../../../assets/textures/stations.ron"))
        .unwrap_or_else(|e| panic!("assets/textures/stations.ron: {e}"));
    commands.insert_resource(StationArt { sheet: assets.load("textures/stations.png"), spec, tiles: None });
}

/// Once stations.png is loaded, cut it into one repeating texture per tile.
pub fn slice_station_art(art: Option<ResMut<StationArt>>, mut images: ResMut<Assets<Image>>) {
    let Some(mut art) = art else { return };
    if art.ready() {
        return;
    }
    let Some(sheet) = images.get(&art.sheet) else { return };
    let rgba = sheet.clone().try_into_dynamic().expect("stations.png must be an image").to_rgba8();
    let t = art.spec.tile_size;
    let levels = t.ilog2() + 1;
    let mut tiles = HashMap::new();
    for (name, &i) in &art.spec.tiles {
        let (cx, cy) = ((i % art.spec.columns) * t, (i / art.spec.columns) * t);
        if cx + t > rgba.width() || cy + t > rgba.height() {
            warn!("stations.ron: tile {name} ({i}) is outside stations.png");
            continue;
        }
        let mut tile = Vec::with_capacity((t * t * 4) as usize);
        for y in 0..t {
            for x in 0..t {
                tile.extend_from_slice(&rgba.get_pixel(cx + x, cy + y).0);
            }
        }
        let mut image = Image::new(
            Extent3d { width: t, height: t, depth_or_array_layers: 1 },
            TextureDimension::D2,
            vec![0; (t * t * 4) as usize],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.data = Some(crate::blocks::mip_chain(&tile, t, levels).concat());
        image.texture_descriptor.mip_level_count = levels;
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mipmap_filter: ImageFilterMode::Linear,
            ..ImageSamplerDescriptor::nearest()
        });
        tiles.insert(name.clone(), images.add(image));
    }
    art.tiles = Some(tiles);
}

/// The material for a box of `mat`, given the urn's colour: its tile when
/// there is one (and the sheet has loaded), else a flat colour.
pub fn material(mat: Mat, urn: (f32, f32, f32), art: Option<&StationArt>) -> StandardMaterial {
    let (base, glow) = mat.look(urn);
    let lin = base.to_linear();
    let alpha = mat.alpha();
    let texture = art.and_then(|a| a.tile(mat));
    let color = match (&texture, mat.tinted()) {
        (Some(_), false) => Color::WHITE,
        _ => base,
    };
    StandardMaterial {
        base_color: color.with_alpha(alpha.unwrap_or(1.0)),
        base_color_texture: texture,
        emissive: LinearRgba::rgb(lin.red * glow, lin.green * glow, lin.blue * glow),
        perceptual_roughness: if alpha.is_some() {
            0.08
        } else if matches!(mat, Mat::Steel | Mat::Brass | Mat::Iron) {
            0.5
        } else {
            0.95
        },
        reflectance: if alpha.is_some() { 0.6 } else { 0.5 },
        alpha_mode: if alpha.is_some() { AlphaMode::Blend } else { AlphaMode::Opaque },
        ..default()
    }
}

/// Where a vertex of a box of `size` (centred on 0) falls on its tile, in
/// tiles: measured from the box's corner along the face, so the same
/// stretch of world shows the same number of pixels on every box.
pub fn uv_at(pos: Vec3, normal: Vec3, size: Vec3) -> [f32; 2] {
    let p = (pos + size * 0.5) * TILES_PER_BLOCK;
    let (u, v) = if normal.x.abs() > 0.5 {
        (if normal.x > 0.0 { size.z * TILES_PER_BLOCK - p.z } else { p.z }, -p.y)
    } else if normal.y.abs() > 0.5 {
        (p.x, if normal.y > 0.0 { p.z } else { -p.z })
    } else {
        (if normal.z > 0.0 { p.x } else { size.x * TILES_PER_BLOCK - p.x }, -p.y)
    };
    [u, v]
}

/// A box of this size with world-scale UVs (`uv_at`).
pub fn box_mesh(size: Vec3) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::new(size.x, size.y, size.z));
    let (Some(VertexAttributeValues::Float32x3(pos)), Some(VertexAttributeValues::Float32x3(nor))) =
        (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_NORMAL))
    else {
        return mesh;
    };
    let uvs: Vec<[f32; 2]> = pos.iter().zip(nor).map(|(p, n)| uv_at(Vec3::from(*p), Vec3::from(*n), size)).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every face spans exactly as many tiles as the box is blocks long.
    #[test]
    fn uvs_follow_world_size_not_box_size() {
        let size = Vec3::new(1.4, 0.1, 0.6);
        let mesh = box_mesh(size);
        let Some(VertexAttributeValues::Float32x3(nor)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!() };
        for face in 0..6 {
            let n = Vec3::from(nor[face * 4]);
            let us: Vec<f32> = (0..4).map(|i| uv[face * 4 + i][0]).collect();
            let vs: Vec<f32> = (0..4).map(|i| uv[face * 4 + i][1]).collect();
            let span = |v: &[f32]| v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min);
            let (w, h) = if n.x.abs() > 0.5 {
                (size.z, size.y)
            } else if n.y.abs() > 0.5 {
                (size.x, size.z)
            } else {
                (size.x, size.y)
            };
            assert!((span(&us) - w * TILES_PER_BLOCK).abs() < 1e-5, "face {n}: u span {}", span(&us));
            assert!((span(&vs) - h * TILES_PER_BLOCK).abs() < 1e-5, "face {n}: v span {}", span(&vs));
        }
    }

    /// Every tile in stations.ron is inside the sheet, and every textured
    /// material has one.
    #[test]
    fn every_textured_material_has_a_tile() {
        let spec: SheetSpec = ron::from_str(include_str!("../../../../assets/textures/stations.ron")).unwrap();
        let img = image::open(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/textures/stations.png")).unwrap();
        let rows = img.height() / spec.tile_size;
        for (name, &i) in &spec.tiles {
            assert!(i / spec.columns < rows && spec.columns * spec.tile_size <= img.width(), "{name} outside the sheet");
        }
        use Mat::*;
        for m in [Wood, WoodDark, Groove, Stone, StoneDark, Iron, Steel, Brass, Reed, Cloth, Linen, Cord, Hide, Moss, Clay, Urn, UrnDark] {
            assert!(spec.tiles.contains_key(m.tile_name().unwrap()), "{m:?} has no tile");
        }
    }
}
