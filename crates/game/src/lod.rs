//! Distant terrain: low-detail tiles out to the edge of the fog, so mountains
//! and sky islands are visible from far away.
//!
//! Each tile covers `TILE` blocks square. Its land is sampled every
//! `LAND_STEP` blocks straight from the world generator (no chunks needed) and
//! drawn as blocky columns in the surface block's average colour; sky islands
//! get a finer grid. Tiles are built on worker threads, nearest first. Where a
//! real chunk is drawn the shader cuts the tiles away so the chunk shows: it
//! reads a mask of drawn chunks on the same 32-block grid, so the cut follows
//! chunk borders exactly and never opens a hole where a chunk is still loading.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use voxel::BlockRegistry;
use worldgen::{WorldGen, SEA_LEVEL};

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::sky::MAX_VISIBILITY;
use crate::world::{camera_chunk, Generator, World};

/// Tile size in blocks.
pub const TILE: i32 = 256;
const LAND_STEP: i32 = 8;
const ISLAND_STEP: i32 = 4;
/// Tiles are kept out to this distance from the camera (blocks).
const RANGE: f32 = MAX_VISIBILITY + TILE as f32 * 0.5;
const MAX_TASKS: usize = 4;

pub type LodMaterial = ExtendedMaterial<StandardMaterial, LodExtension>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct LodExtension {
    /// xyz: the chunk the mask is centred on; w: the mask's half-width in chunks.
    #[uniform(100)]
    pub center: Vec4,
    /// Drawn chunks around `center` (see `World::drawn_mask`); the shader hides the tiles there.
    #[texture(101, sample_type = "u_int")]
    pub mask: Handle<Image>,
    /// Sky-matched fog and ground mist (see `atmosphere`).
    #[uniform(102)]
    pub atmos: crate::atmosphere::AtmosParams,
}

/// Half-width of the drawn-chunk mask in chunks: the largest view distance
/// (32) plus the 2-chunk margin chunks keep before they unload.
const MASK_HALF: i32 = 34;
const MASK_N: u32 = (2 * MASK_HALF + 1) as u32;

impl MaterialExtension for LodExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/lod.wesl".into()
    }
}

pub struct LodPlugin;

impl Plugin for LodPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<LodMaterial>::default())
            .init_resource::<LodTiles>()
            .add_systems(crate::menu::GameUpdate, (update_mask.after(crate::world::unload_far), schedule_tiles, collect_tiles).chain());
    }
}

#[derive(Resource, Default)]
pub struct LodTiles {
    tiles: HashMap<IVec2, Entity>,
    tasks: HashMap<IVec2, Task<Option<Mesh>>>,
    material: Option<Handle<LodMaterial>>,
    mask: Option<Handle<Image>>,
    pub count: usize,
}

impl LodTiles {
    pub fn material(&self) -> Option<&Handle<LodMaterial>> {
        self.material.as_ref()
    }
}

/// Keep the shader's mask of drawn chunks up to date (only re-sent when it changes).
fn update_mask(
    world: Res<World>,
    mut lod: ResMut<LodTiles>,
    mut materials: ResMut<Assets<LodMaterial>>,
    mut images: ResMut<Assets<Image>>,
    cam: Query<&Transform, With<FlyCamera>>,
    mut last: Local<(Option<IVec3>, Vec<u32>)>,
) {
    let Ok(cam) = cam.single() else { return };
    let c = camera_chunk(cam);
    let center = IVec3::new(c.x, c.y, c.z);
    let mask = world.drawn_mask(c, MASK_HALF);
    let bytes = |m: &[u32]| m.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>();
    let Some(h) = lod.material.clone() else {
        let image = Image::new(
            Extent3d { width: MASK_N, height: MASK_N, depth_or_array_layers: 1 },
            TextureDimension::D2,
            bytes(&mask),
            TextureFormat::R32Uint,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        lod.mask = Some(images.add(image));
        lod.material = Some(materials.add(LodMaterial {
            base: StandardMaterial { perceptual_roughness: 1.0, reflectance: 0.05, fog_enabled: false, ..default() },
            extension: LodExtension { center: center.as_vec3().extend(MASK_HALF as f32), mask: lod.mask.clone().unwrap(), atmos: default() },
        }));
        *last = (Some(center), mask);
        return;
    };
    if last.0 == Some(center) && last.1 == mask {
        return;
    }
    // Change the image and the centre in the same frame so they always agree.
    if let Some(mut image) = lod.mask.as_ref().and_then(|m| images.get_mut(m)) {
        image.data = Some(bytes(&mask));
    }
    if let Some(mut m) = materials.get_mut(&h) {
        m.extension.center = center.as_vec3().extend(MASK_HALF as f32);
    }
    *last = (Some(center), mask);
}

fn tile_of(p: Vec3) -> IVec2 {
    IVec2::new((p.x / TILE as f32).floor() as i32, (p.z / TILE as f32).floor() as i32)
}

fn tile_distance(t: IVec2, p: Vec3) -> f32 {
    let c = (t.as_vec2() + 0.5) * TILE as f32;
    // Distance to the nearest point of the tile.
    let d = (Vec2::new(p.x, p.z) - c).abs() - Vec2::splat(TILE as f32 * 0.5);
    d.max(Vec2::ZERO).length()
}

fn schedule_tiles(
    mut commands: Commands,
    mut lod: ResMut<LodTiles>,
    generator: Option<Res<Generator>>,
    blocks: Res<Blocks>,
    cam: Query<&Transform, With<FlyCamera>>,
) {
    let (Some(g), Some(colors), Ok(cam)) = (generator, blocks.tile_colors.clone(), cam.single()) else { return };
    let p = cam.translation;
    let center = tile_of(p);
    let reach = (RANGE / TILE as f32).ceil() as i32 + 1;

    // Drop tiles that fell out of range.
    let far: Vec<IVec2> = lod.tiles.keys().filter(|t| tile_distance(**t, p) > RANGE + TILE as f32).copied().collect();
    for t in far {
        if let Some(e) = lod.tiles.remove(&t) {
            commands.entity(e).despawn();
        }
    }
    lod.tasks.retain(|t, _| tile_distance(*t, p) <= RANGE + TILE as f32);

    // Start the nearest missing tiles.
    if lod.tasks.len() >= MAX_TASKS {
        return;
    }
    let mut wanted = Vec::new();
    for dz in -reach..=reach {
        for dx in -reach..=reach {
            let t = center + IVec2::new(dx, dz);
            if tile_distance(t, p) <= RANGE && !lod.tiles.contains_key(&t) && !lod.tasks.contains_key(&t) {
                wanted.push(t);
            }
        }
    }
    wanted.sort_by(|a, b| tile_distance(*a, p).total_cmp(&tile_distance(*b, p)));
    let pool = AsyncComputeTaskPool::get();
    for t in wanted.into_iter().take(MAX_TASKS - lod.tasks.len()) {
        let (g, reg, colors) = (g.0.clone(), blocks.registry.clone(), colors.clone());
        lod.tasks.insert(t, pool.spawn(async move { build_tile(&g, &reg, &colors, t) }));
    }
}

fn collect_tiles(mut commands: Commands, mut lod: ResMut<LodTiles>, mut meshes: ResMut<Assets<Mesh>>) {
    let Some(mat) = lod.material.clone() else { return };
    let mut done = Vec::new();
    for (t, task) in lod.tasks.iter_mut() {
        if let Some(mesh) = check_ready(task) {
            done.push((*t, mesh));
        }
    }
    for (t, mesh) in done {
        lod.tasks.remove(&t);
        let tf = Transform::from_xyz((t.x * TILE) as f32, 0.0, (t.y * TILE) as f32);
        let e = match mesh {
            Some(m) => commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(mat.clone()), tf, NotShadowCaster)).id(),
            // Nothing to draw (can't happen for land, but keep the slot filled).
            None => commands.spawn(tf).id(),
        };
        lod.tiles.insert(t, e);
    }
    lod.count = lod.tiles.len();
}

/// One column of a tile: solid from `bottom` to `top` (block units), with colours.
#[derive(Clone, Copy)]
struct Col {
    bottom: f32,
    top: f32,
    top_color: Vec3,
    side_color: Vec3,
    bottom_color: Vec3,
}

#[derive(Default)]
struct MeshBuf {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl MeshBuf {
    fn quad(&mut self, c: [Vec3; 4], n: Vec3, color: Vec3) {
        let s = self.pos.len() as u32;
        for v in c {
            self.pos.push(v.to_array());
            self.nor.push(n.to_array());
            self.col.push([color.x, color.y, color.z, 1.0]);
        }
        self.idx.extend_from_slice(&[s, s + 1, s + 2, s, s + 2, s + 3]);
    }

    /// Blocky columns on an n x n grid (with a one-cell border of neighbours
    /// around it, so walls are only drawn where they can be seen).
    fn columns(&mut self, grid: &[Option<Col>], n: i32, step: f32) {
        let at = |x: i32, z: i32| grid[((z + 1) * (n + 2) + x + 1) as usize];
        for z in 0..n {
            for x in 0..n {
                let Some(c) = at(x, z) else { continue };
                let (x0, z0) = (x as f32 * step, z as f32 * step);
                let (x1, z1) = (x0 + step, z0 + step);
                let (b, t) = (c.bottom, c.top);
                self.quad([Vec3::new(x0, t, z0), Vec3::new(x0, t, z1), Vec3::new(x1, t, z1), Vec3::new(x1, t, z0)], Vec3::Y, c.top_color);
                if b > -1000.0 {
                    self.quad([Vec3::new(x0, b, z0), Vec3::new(x1, b, z0), Vec3::new(x1, b, z1), Vec3::new(x0, b, z1)], Vec3::NEG_Y, c.bottom_color);
                }
                let walls = [
                    (at(x + 1, z), Vec3::X, [Vec3::new(x1, 0.0, z0), Vec3::new(x1, 1.0, z0), Vec3::new(x1, 1.0, z1), Vec3::new(x1, 0.0, z1)]),
                    (at(x - 1, z), Vec3::NEG_X, [Vec3::new(x0, 0.0, z1), Vec3::new(x0, 1.0, z1), Vec3::new(x0, 1.0, z0), Vec3::new(x0, 0.0, z0)]),
                    (at(x, z + 1), Vec3::Z, [Vec3::new(x1, 0.0, z1), Vec3::new(x1, 1.0, z1), Vec3::new(x0, 1.0, z1), Vec3::new(x0, 0.0, z1)]),
                    (at(x, z - 1), Vec3::NEG_Z, [Vec3::new(x0, 0.0, z0), Vec3::new(x0, 1.0, z0), Vec3::new(x1, 1.0, z0), Vec3::new(x1, 0.0, z0)]),
                ];
                for (other, normal, quad) in walls {
                    // The exposed parts of [b, t]: above the neighbour, and below it.
                    let mut spans = Vec::with_capacity(2);
                    match other {
                        None => spans.push((b.max(-200.0), t)),
                        Some(o) => {
                            if o.top < t {
                                spans.push((o.top.max(b), t));
                            }
                            if o.bottom > b && b > -1000.0 {
                                spans.push((b, o.bottom.min(t)));
                            }
                        }
                    }
                    for (lo, hi) in spans {
                        if hi > lo {
                            let q = quad.map(|v| Vec3::new(v.x, if v.y == 0.0 { lo } else { hi }, v.z));
                            self.quad(q, normal, c.side_color);
                        }
                    }
                }
            }
        }
    }

    fn into_mesh(self) -> Option<Mesh> {
        (!self.idx.is_empty()).then(|| {
            Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
                .with_inserted_indices(Indices::U32(self.idx))
        })
    }
}

fn build_tile(g: &WorldGen, reg: &BlockRegistry, colors: &Arc<Vec<Vec3>>, tile: IVec2) -> Option<Mesh> {
    let (ox, oz) = (tile.x * TILE, tile.y * TILE);
    let color = |b: voxel::BlockId, face: usize| colors.get(reg.get(b).textures[face] as usize).copied().unwrap_or(Vec3::splat(0.5));
    let water = reg.id("water").map(|w| color(w, 2)).unwrap_or(Vec3::new(0.1, 0.2, 0.4));
    let deep = Vec3::new(0.01, 0.035, 0.08);
    let mut buf = MeshBuf::default();

    // Land, one block lower than the real surface so real chunks always cover it.
    let n = TILE / LAND_STEP;
    let mut grid = Vec::with_capacity(((n + 2) * (n + 2)) as usize);
    for z in -1..=n {
        for x in -1..=n {
            let (wx, wz) = (ox + x * LAND_STEP + LAND_STEP / 2, oz + z * LAND_STEP + LAND_STEP / 2);
            let col = g.column(wx, wz);
            let biome = g.biomes.get(col.biome);
            let c = if col.height < SEA_LEVEL {
                let depth = (SEA_LEVEL - col.height) as f32;
                let tint = water.lerp(deep, (depth / 30.0).clamp(0.0, 1.0));
                Col { bottom: -10_000.0, top: SEA_LEVEL as f32 - 0.2, top_color: tint, side_color: tint, bottom_color: tint }
            } else {
                Col {
                    bottom: -10_000.0,
                    top: col.height as f32,
                    top_color: color(biome.surface, 2),
                    side_color: color(biome.subsurface, 0) * 0.9,
                    bottom_color: Vec3::ZERO,
                }
            };
            grid.push(Some(c));
        }
    }
    buf.columns(&grid, n, LAND_STEP as f32);

    // Sky islands, on a finer grid.
    let sky = g.biomes.get(g.biomes.sky);
    let skystone = reg.id("skystone").map(|b| color(b, 2)).unwrap_or(Vec3::splat(0.6));
    let n = TILE / ISLAND_STEP;
    let mut grid = Vec::with_capacity(((n + 2) * (n + 2)) as usize);
    let mut any = false;
    for z in -1..=n {
        for x in -1..=n {
            let (wx, wz) = (ox + x * ISLAND_STEP + ISLAND_STEP / 2, oz + z * ISLAND_STEP + ISLAND_STEP / 2);
            let c = g.island_span(wx, wz).map(|(b, t)| {
                any = true;
                Col {
                    bottom: b as f32,
                    top: t as f32 + 1.0,
                    top_color: color(sky.surface, 2),
                    side_color: color(sky.subsurface, 0).lerp(skystone, 0.5),
                    bottom_color: skystone * 0.8,
                }
            });
            grid.push(c);
        }
    }
    if any {
        let mut islands = MeshBuf::default();
        islands.columns(&grid, n, ISLAND_STEP as f32);
        let base = buf.pos.len() as u32;
        buf.pos.extend(islands.pos);
        buf.nor.extend(islands.nor);
        buf.col.extend(islands.col);
        buf.idx.extend(islands.idx.into_iter().map(|i| i + base));
    }
    buf.into_mesh()
}
