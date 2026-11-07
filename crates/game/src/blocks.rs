//! Block registry and the terrain material.
//!
//! At startup the spritesheet is cut into a texture array (one layer per
//! 32x32 tile) so each block face can repeat its tile across a merged quad.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use voxel::{BlockRegistry, TileSheet};

pub type VoxelMaterial = ExtendedMaterial<StandardMaterial, VoxelExtension>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct VoxelExtension {
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub textures: Handle<Image>,
}

impl MaterialExtension for VoxelExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/voxel.wesl".into()
    }
}

/// Shared block data. `material` is `None` until the spritesheet has loaded.
#[derive(Resource)]
pub struct Blocks {
    pub registry: Arc<BlockRegistry>,
    pub opaque: Arc<Vec<bool>>,
    pub tiles: TileSheet,
    sheet: Handle<Image>,
    pub material: Option<Handle<VoxelMaterial>>,
    /// Blended, double-sided material for water and ice.
    pub translucent_material: Option<Handle<VoxelMaterial>>,
}

pub struct BlocksPlugin;

impl Plugin for BlocksPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<VoxelMaterial>::default())
            .add_systems(PreStartup, load_blocks)
            .add_systems(Update, build_material.run_if(|b: Res<Blocks>| b.material.is_none()));
    }
}

fn load_blocks(mut commands: Commands, assets: Res<AssetServer>) {
    let tiles = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron"))
        .expect("assets/textures/blocks.ron is invalid");
    let registry = BlockRegistry::from_ron(include_str!("../../../assets/data/blocks.ron"), &tiles)
        .unwrap_or_else(|e| panic!("{e}"));
    commands.insert_resource(Blocks {
        opaque: Arc::new(registry.opaque_table()),
        registry: Arc::new(registry),
        tiles,
        sheet: assets.load("textures/blocks.png"),
        material: None,
        translucent_material: None,
    });
}

/// Once blocks.png is loaded, restack its tiles into a texture array.
fn build_material(
    mut blocks: ResMut<Blocks>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<VoxelMaterial>>,
) {
    let Some(sheet) = images.get(&blocks.sheet) else { return };
    let rgba = sheet.clone().try_into_dynamic().expect("blocks.png must be an image").to_rgba8();
    let t = blocks.tiles.tile_size;
    let cols = rgba.width() / t;
    let rows = rgba.height() / t;
    let layers = cols * rows;
    let mut data = Vec::with_capacity((t * t * 4 * layers) as usize);
    for layer in 0..layers {
        let (cx, cy) = ((layer % cols) * t, (layer / cols) * t);
        for y in 0..t {
            for x in 0..t {
                data.extend_from_slice(&rgba.get_pixel(cx + x, cy + y).0);
            }
        }
    }
    let mut array = Image::new(
        Extent3d { width: t, height: t, depth_or_array_layers: layers },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    array.sampler = ImageSampler::nearest();
    let textures = images.add(array);

    blocks.material = Some(materials.add(VoxelMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.95,
            reflectance: 0.08,
            ..default()
        },
        extension: VoxelExtension { textures: textures.clone() },
    }));
    blocks.translucent_material = Some(materials.add(VoxelMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.15,
            reflectance: 0.5,
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: VoxelExtension { textures },
    }));
}
