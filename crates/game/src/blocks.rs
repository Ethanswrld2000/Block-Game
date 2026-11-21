//! Block registry and the terrain material.
//!
//! At startup the spritesheet is cut into a texture array (one layer per
//! 32x32 tile) so each block face can repeat its tile across a merged quad.
//! Each layer gets its own mipmaps (`mip_chain`), so far-off ground doesn't
//! shimmer, and tiles listed under `variants` in blocks.ron get a different
//! version (and turn) per block in the shader (`VoxelExtension::variants`).

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
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
    /// Season colour for grass and leaves: rgb, and a = how strongly to apply it.
    #[uniform(102)]
    pub season: Vec4,
    /// Wind for swaying leaves and plants: xy = direction (x, z), z = how hard
    /// (see `wind::Wind::sway`).
    #[uniform(103)]
    pub wind: Vec4,
    /// Sky-matched fog and ground mist (see `atmosphere`).
    #[uniform(104)]
    pub atmos: crate::atmosphere::AtmosParams,
    /// A tree shaking from an axe blow: xyz = where it was hit, w = how hard
    /// (fades from 1 to 0; see `chop`).
    #[uniform(105)]
    pub shake: Vec4,
    /// Per tile index: which other versions it has and whether it turns
    /// (`voxel::tiles::pack_variant`, four per vector).
    #[uniform(106)]
    pub variants: [UVec4; voxel::tiles::VARIANT_SLOTS / 4],
    /// Glowing plants (glowcaps): x = their texture layer, y = how bright
    /// (0 by day, up to 1 at night; see `sky::night_glow`).
    #[uniform(107)]
    pub glow: Vec4,
}

impl MaterialExtension for VoxelExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/voxel_vertex.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/voxel.wesl".into()
    }

    /// Shadows sway with the leaves, so leaves don't shadow themselves.
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/voxel_prepass.wesl".into()
    }
}

/// The glow uniform's starting value: the glowcap's texture layer, dark.
fn glow_layer(tiles: &TileSheet) -> Vec4 {
    Vec4::new(tiles.index("glowcap").map_or(-1.0, |i| i as f32), 0.0, 0.0, 0.0)
}

/// Shared block data. `material` is `None` until the spritesheet has loaded.
#[derive(Resource)]
pub struct Blocks {
    pub registry: Arc<BlockRegistry>,
    pub opaque: Arc<Vec<bool>>,
    pub tiles: TileSheet,
    pub sheet: Handle<Image>,
    pub material: Option<Handle<VoxelMaterial>>,
    /// Blended, double-sided material for water and ice.
    pub translucent_material: Option<Handle<VoxelMaterial>>,
    /// Average linear colour of each tile, for distant terrain.
    pub tile_colors: Option<Arc<Vec<Vec3>>>,
}

pub struct BlocksPlugin;

impl Plugin for BlocksPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<VoxelMaterial>::default())
            .add_systems(crate::menu::GamePreStartup, load_blocks)
            .add_systems(crate::menu::GameUpdate, build_material.run_if(|b: Res<Blocks>| b.material.is_none()));
    }
}

pub fn load_blocks(mut commands: Commands, assets: Res<AssetServer>) {
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
        tile_colors: None,
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
    let levels = t.ilog2() + 1;
    let mut data = Vec::with_capacity((t * t * 4 * layers * 2) as usize);
    let mut averages = Vec::with_capacity(layers as usize);
    for layer in 0..layers {
        let (cx, cy) = ((layer % cols) * t, (layer / cols) * t);
        let (mut sum, mut n) = (Vec3::ZERO, 0.0);
        let mut tile = Vec::with_capacity((t * t * 4) as usize);
        for y in 0..t {
            for x in 0..t {
                let px = rgba.get_pixel(cx + x, cy + y).0;
                tile.extend_from_slice(&px);
                if px[3] > 128 {
                    let c = Color::srgb_u8(px[0], px[1], px[2]).to_linear();
                    sum += Vec3::new(c.red, c.green, c.blue);
                    n += 1.0;
                }
            }
        }
        averages.push(if n > 0.0 { sum / n } else { Vec3::splat(0.5) });
        // Layer-major: every mip of this layer, then the next layer.
        for level in mip_chain(&tile, t, levels) {
            data.extend_from_slice(&level);
        }
    }
    blocks.tile_colors = Some(Arc::new(averages));
    let mut array = Image::new(
        Extent3d { width: t, height: t, depth_or_array_layers: layers },
        TextureDimension::D2,
        vec![0; (t * t * 4 * layers) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    array.data = Some(data);
    array.texture_descriptor.mip_level_count = levels;
    // Crisp pixels up close, blended between mip levels far away.
    array.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mipmap_filter: ImageFilterMode::Linear,
        ..ImageSamplerDescriptor::nearest()
    });
    let textures = images.add(array);

    let table = blocks.tiles.variant_table().unwrap_or_else(|e| panic!("assets/textures/blocks.ron: {e}"));
    let mut variants = [UVec4::ZERO; voxel::tiles::VARIANT_SLOTS / 4];
    for (i, v) in variants.iter_mut().enumerate() {
        *v = UVec4::new(table[i * 4], table[i * 4 + 1], table[i * 4 + 2], table[i * 4 + 3]);
    }

    blocks.material = Some(materials.add(VoxelMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.95,
            reflectance: 0.08,
            // The shader fogs towards the sky itself (see `atmosphere`).
            fog_enabled: false,
            ..default()
        },
        extension: VoxelExtension { textures: textures.clone(), season: Vec4::ZERO, wind: Vec4::ZERO, atmos: default(), shake: Vec4::ZERO, variants, glow: glow_layer(&blocks.tiles) },
    }));
    blocks.translucent_material = Some(materials.add(VoxelMaterial {
        base: StandardMaterial {
            perceptual_roughness: 0.15,
            reflectance: 0.5,
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            cull_mode: None,
            fog_enabled: false,
            ..default()
        },
        extension: VoxelExtension { textures, season: Vec4::ZERO, wind: Vec4::ZERO, atmos: default(), shake: Vec4::ZERO, variants, glow: glow_layer(&blocks.tiles) },
    }));
}

/// Mip levels of one square sRGB RGBA tile, `levels` of them starting with the
/// tile itself. Colours average in linear light, weighted by alpha so cut-out
/// pixels don't bleed their colour in. Cut-out tiles (alpha only 0 or 255) keep
/// the same share of pixels over the shader's cut-off at every level, so plants
/// and leaves don't fade away or fill in with distance.
pub fn mip_chain(tile: &[u8], size: u32, levels: u32) -> Vec<Vec<u8>> {
    const CUTOFF: f32 = 0.1; // `discard` below this alpha in voxel.wesl
    let cutout = tile.chunks(4).all(|p| p[3] == 0 || p[3] == 255);
    let coverage = |a: &[f32]| a.iter().filter(|&&x| x >= CUTOFF).count() as f32 / a.len() as f32;
    let to_lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let to_srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0 + 0.5) as u8
    };
    // Linear rgb + alpha, as floats.
    let mut cur: Vec<[f32; 4]> =
        tile.chunks(4).map(|p| [to_lin(p[0]), to_lin(p[1]), to_lin(p[2]), p[3] as f32 / 255.0]).collect();
    let base_alpha: Vec<f32> = cur.iter().map(|p| p[3]).collect();
    let target = coverage(&base_alpha);
    let mut out = vec![tile.to_vec()];
    let mut n = size as usize;
    for _ in 1..levels {
        let m = (n / 2).max(1);
        let mut next = vec![[0.0f32; 4]; m * m];
        for y in 0..m {
            for x in 0..m {
                let (mut rgb, mut wsum, mut plain, mut a) = ([0.0f32; 3], 0.0, [0.0f32; 3], 0.0);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = cur[((y * 2 + dy).min(n - 1)) * n + (x * 2 + dx).min(n - 1)];
                    for c in 0..3 {
                        rgb[c] += p[c] * p[3];
                        plain[c] += p[c] * 0.25;
                    }
                    wsum += p[3];
                    a += p[3] * 0.25;
                }
                let rgb = if wsum > 0.0 { rgb.map(|c| c / wsum) } else { plain };
                next[y * m + x] = [rgb[0], rgb[1], rgb[2], a];
            }
        }
        cur = next;
        n = m;
        let mut alpha: Vec<f32> = cur.iter().map(|p| p[3]).collect();
        if cutout && target > 0.0 && target < 1.0 {
            // Scale alpha so the same share of pixels stays over the cut-off.
            let (mut lo, mut hi) = (0.0f32, 64.0f32);
            for _ in 0..24 {
                let mid = (lo + hi) / 2.0;
                let scaled: Vec<f32> = alpha.iter().map(|a| (a * mid).min(1.0)).collect();
                if coverage(&scaled) < target { lo = mid } else { hi = mid }
            }
            alpha = alpha.iter().map(|a| (a * hi).min(1.0)).collect();
        }
        out.push(
            cur.iter()
                .zip(&alpha)
                .flat_map(|(p, &a)| [to_srgb(p[0]), to_srgb(p[1]), to_srgb(p[2]), (a * 255.0 + 0.5) as u8])
                .collect(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_chain_sizes_and_colours() {
        let tile: Vec<u8> = (0..32 * 32).flat_map(|i| if i % 2 == 0 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        let chain = mip_chain(&tile, 32, 6);
        let sizes: Vec<usize> = chain.iter().map(|l| l.len() / 4).collect();
        assert_eq!(sizes, vec![1024, 256, 64, 16, 4, 1]);
        let last = &chain[5];
        // Half red, half blue in linear light.
        assert!((last[0] as i32 - 188).abs() <= 2 && (last[2] as i32 - 188).abs() <= 2 && last[3] == 255);
    }

    #[test]
    fn cut_out_tiles_keep_their_coverage() {
        // A quarter of the pixels solid green, the rest cut out (white underneath).
        let tile: Vec<u8> = (0..32 * 32)
            .flat_map(|i| if (i % 32) % 2 == 0 && (i / 32) % 2 == 0 { [0, 200, 0, 255] } else { [255, 255, 255, 0] })
            .collect();
        let chain = mip_chain(&tile, 32, 6);
        for level in &chain[1..] {
            let px = level.len() / 4;
            let kept = level.chunks(4).filter(|p| p[3] as f32 / 255.0 >= 0.1).count();
            assert!(kept as f32 / px as f32 >= 0.25 - 1e-3, "coverage held");
            // No white bleeding in from the cut-out pixels.
            assert!(level.chunks(4).all(|p| p[0] < 10 && p[1] > 150));
        }
    }
}
