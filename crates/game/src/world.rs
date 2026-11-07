//! Chunk streaming: generate, mesh, upload and unload chunks around the camera.
//!
//! Generation and meshing run on Bevy's `AsyncComputeTaskPool`. The main
//! thread only schedules work, collects finished results and uploads a
//! limited number of meshes per frame so nothing ever stalls.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, AsyncComputeTaskPool, Task};
use voxel::{BlockId, BlockPos, BlockRegistry, Chunk, ChunkPos, Face, PaddedChunk, Quad, CHUNK_SIZE};
use worldgen::TestTerrain;

use crate::blocks::{Blocks, VoxelMaterial};
use crate::camera::FlyCamera;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ViewDistance { chunks: 12 })
            .init_resource::<World>()
            .init_resource::<WorldStats>()
            .add_systems(Startup, setup_generator)
            .add_systems(
                Update,
                (change_view_distance, schedule_generation, collect_generated, schedule_meshing, collect_meshes, unload_far)
                    .chain()
                    .run_if(|b: Res<Blocks>| b.material.is_some()),
            );
    }
}

/// World seed. Fixed for now; phase 3 adds world creation and saving.
pub const SEED: u64 = 0x005E_ED0F_5A4D;

/// Vertical range of chunks kept loaded (phase 3 makes this follow the player).
pub const MIN_CHUNK_Y: i32 = -2;
pub const MAX_CHUNK_Y: i32 = 5;

/// Concurrent tasks. Enough to keep every core busy without flooding memory.
const MAX_GEN_TASKS: usize = 48;
const MAX_MESH_TASKS: usize = 32;
/// Meshes uploaded to the GPU per frame.
const MAX_UPLOADS_PER_FRAME: usize = 24;

/// Horizontal view radius in chunks. Change in game with `-` and `=`.
#[derive(Resource)]
pub struct ViewDistance {
    pub chunks: i32,
}

#[derive(Resource, Clone)]
struct Generator(Arc<TestTerrain>);

struct LoadedChunk {
    data: Arc<Chunk>,
    entity: Option<Entity>,
    /// Bumped on every change, so a stale mesh result is ignored.
    version: u32,
    meshed_version: Option<u32>,
}

/// All loaded chunks plus in-flight work.
#[derive(Resource, Default)]
pub struct World {
    chunks: HashMap<ChunkPos, LoadedChunk>,
    gen_tasks: HashMap<ChunkPos, Task<Chunk>>,
    mesh_tasks: HashMap<ChunkPos, (u32, Task<Option<Mesh>>)>,
    finished_meshes: Vec<(ChunkPos, u32, Option<Mesh>)>,
    /// Chunks to generate, nearest last (so `pop` takes the nearest).
    gen_queue: Vec<ChunkPos>,
    queue_center: Option<(ChunkPos, i32)>,
}

/// Numbers for the debug overlay.
#[derive(Resource, Default)]
pub struct WorldStats {
    pub loaded: usize,
    pub meshed: usize,
    pub gen_in_flight: usize,
    pub mesh_in_flight: usize,
    pub queued: usize,
    pub chunk_kib: usize,
}

impl World {
    pub fn block(&self, p: BlockPos) -> Option<BlockId> {
        let [x, y, z] = p.local();
        self.chunks.get(&p.chunk()).map(|c| c.data.get(x, y, z))
    }

    /// Change a block and remesh what it touches right away.
    pub fn set_block(&mut self, p: BlockPos, block: BlockId) -> bool {
        let cp = p.chunk();
        let [x, y, z] = p.local();
        let Some(c) = self.chunks.get_mut(&cp) else { return false };
        if c.data.get(x, y, z) == block {
            return false;
        }
        Arc::make_mut(&mut c.data).set(x, y, z, block);
        c.version += 1;
        // Faces on a chunk border belong to the neighbour's mesh too.
        let mut touched = vec![cp];
        for (axis, l) in [x, y, z].into_iter().enumerate() {
            let mut d = [0; 3];
            if l == 0 {
                d[axis] = -1;
            } else if l == CHUNK_SIZE - 1 {
                d[axis] = 1;
            } else {
                continue;
            }
            touched.push(ChunkPos::new(cp.x + d[0], cp.y + d[1], cp.z + d[2]));
        }
        for t in touched {
            if let Some(n) = self.chunks.get_mut(&t) {
                if t != cp {
                    n.version += 1;
                }
                n.meshed_version = None;
            }
        }
        true
    }

    /// Mesh a chunk on this thread (used for player edits, which must be instant).
    pub fn mesh_now(&mut self, pos: ChunkPos, registry: &BlockRegistry, opaque: &[bool]) {
        if let Some(input) = self.mesh_input(pos) {
            let version = self.chunks[&pos].version;
            self.mesh_tasks.remove(&pos);
            self.finished_meshes.insert(0, (pos, version, build_mesh(&input.pad(), registry, opaque)));
        }
    }

    /// True if a loaded chunk's mesh is out of date.
    pub fn needs_mesh(&self, pos: ChunkPos) -> bool {
        self.chunks.get(&pos).is_some_and(|c| c.meshed_version != Some(c.version))
    }

    /// The chunk and its six neighbours, if all that are in range are loaded.
    fn mesh_input(&self, pos: ChunkPos) -> Option<MeshInput> {
        let center = self.chunks.get(&pos)?.data.clone();
        let mut neighbours: [Option<Arc<Chunk>>; 6] = Default::default();
        for face in Face::ALL {
            let n = face.normal();
            let np = ChunkPos::new(pos.x + n[0], pos.y + n[1], pos.z + n[2]);
            if !(MIN_CHUNK_Y..=MAX_CHUNK_Y).contains(&np.y) {
                continue; // outside the loaded band: treat as air
            }
            neighbours[face as usize] = Some(self.chunks.get(&np)?.data.clone());
        }
        Some(MeshInput { center, neighbours })
    }
}

struct MeshInput {
    center: Arc<Chunk>,
    neighbours: [Option<Arc<Chunk>>; 6],
}

impl MeshInput {
    /// Copy the chunk and a 1-block border from each neighbour into a padded array.
    fn pad(&self) -> PaddedChunk {
        let mut p = PaddedChunk::default();
        let n = CHUNK_SIZE;
        if let Some(b) = self.center.is_uniform() {
            for y in 1..=n {
                for z in 1..=n {
                    for x in 1..=n {
                        p.set(x, y, z, b);
                    }
                }
            }
        } else {
            for y in 0..n {
                for z in 0..n {
                    for x in 0..n {
                        p.set(x + 1, y + 1, z + 1, self.center.get(x, y, z));
                    }
                }
            }
        }
        for face in Face::ALL {
            let Some(nb) = &self.neighbours[face as usize] else { continue };
            let axis = face.axis();
            // Padded slice to fill, and the neighbour's layer that touches us.
            let (pad_d, src_d) = if face.is_positive() { (n + 1, 0) } else { (0, n - 1) };
            let (ua, va) = face.plane_axes();
            for u in 0..n {
                for v in 0..n {
                    let mut src = [0; 3];
                    src[axis] = src_d;
                    src[ua] = u;
                    src[va] = v;
                    let mut dst = [0; 3];
                    dst[axis] = pad_d;
                    dst[ua] = u + 1;
                    dst[va] = v + 1;
                    p.set(dst[0], dst[1], dst[2], nb.get(src[0], src[1], src[2]));
                }
            }
        }
        p
    }

    /// Chunks that cannot have any visible face.
    fn trivially_empty(&self, opaque: &[bool]) -> bool {
        match self.center.is_uniform() {
            Some(b) if b.is_air() => true,
            Some(b) if opaque[b.0 as usize] => self
                .neighbours
                .iter()
                .all(|n| n.as_ref().and_then(|c| c.is_uniform()).is_some_and(|nb| opaque[nb.0 as usize])),
            _ => false,
        }
    }
}

fn setup_generator(mut commands: Commands, blocks: Res<Blocks>) {
    commands.insert_resource(Generator(Arc::new(TestTerrain::new(SEED, &blocks.registry))));
}

fn change_view_distance(keys: Res<ButtonInput<KeyCode>>, mut view: ResMut<ViewDistance>) {
    if keys.just_pressed(KeyCode::Equal) {
        view.chunks = (view.chunks + 2).min(32);
    }
    if keys.just_pressed(KeyCode::Minus) {
        view.chunks = (view.chunks - 2).max(4);
    }
}

fn camera_chunk(cam: &Transform) -> ChunkPos {
    BlockPos::from_world(cam.translation.to_array()).chunk()
}

fn schedule_generation(
    mut world: ResMut<World>,
    generator: Res<Generator>,
    view: Res<ViewDistance>,
    cam: Single<&Transform, With<FlyCamera>>,
) {
    let center = camera_chunk(&cam);
    let r = view.chunks;
    // Rebuild the queue when the camera enters a new chunk column or the view changes.
    let key = (ChunkPos::new(center.x, 0, center.z), r);
    if world.queue_center != Some(key) {
        world.queue_center = Some(key);
        let mut queue = Vec::new();
        for z in -r..=r {
            for x in -r..=r {
                if x * x + z * z > r * r {
                    continue;
                }
                for y in MIN_CHUNK_Y..=MAX_CHUNK_Y {
                    let p = ChunkPos::new(center.x + x, y, center.z + z);
                    if !world.chunks.contains_key(&p) && !world.gen_tasks.contains_key(&p) {
                        queue.push(p);
                    }
                }
            }
        }
        // Nearest last; vertical distance counts extra so the ground near you comes first.
        let dist = |p: &ChunkPos| {
            let (dx, dy, dz) = (p.x - center.x, p.y - center.y, p.z - center.z);
            dx * dx + dz * dz + 2 * dy * dy
        };
        queue.sort_by_key(|p| std::cmp::Reverse(dist(p)));
        world.gen_queue = queue;
    }

    let pool = AsyncComputeTaskPool::get();
    while world.gen_tasks.len() < MAX_GEN_TASKS {
        let Some(p) = world.gen_queue.pop() else { break };
        if world.chunks.contains_key(&p) || world.gen_tasks.contains_key(&p) {
            continue;
        }
        let g = generator.0.clone();
        world.gen_tasks.insert(p, pool.spawn(async move { g.generate(p) }));
    }
}

fn collect_generated(mut world: ResMut<World>) {
    let world = &mut *world;
    let mut done = Vec::new();
    for (p, task) in world.gen_tasks.iter_mut() {
        if let Some(chunk) = check_ready(task) {
            done.push((*p, chunk));
        }
    }
    for (p, chunk) in done {
        world.gen_tasks.remove(&p);
        world.chunks.insert(p, LoadedChunk { data: Arc::new(chunk), entity: None, version: 0, meshed_version: None });
    }
}

fn schedule_meshing(
    mut world: ResMut<World>,
    blocks: Res<Blocks>,
    cam: Single<&Transform, With<FlyCamera>>,
) {
    if world.mesh_tasks.len() >= MAX_MESH_TASKS {
        return;
    }
    let center = camera_chunk(&cam);
    let waiting: HashSet<ChunkPos> = world.finished_meshes.iter().map(|(p, ..)| *p).collect();
    let mut wanted: Vec<ChunkPos> = world
        .chunks
        .iter()
        .filter(|(p, c)| {
            c.meshed_version != Some(c.version) && !world.mesh_tasks.contains_key(p) && !waiting.contains(p)
        })
        .map(|(p, _)| *p)
        .collect();
    let dist = |p: &ChunkPos| {
        let (dx, dy, dz) = (p.x - center.x, p.y - center.y, p.z - center.z);
        dx * dx + dy * dy + dz * dz
    };
    wanted.sort_unstable_by_key(dist);

    let pool = AsyncComputeTaskPool::get();
    for p in wanted {
        if world.mesh_tasks.len() >= MAX_MESH_TASKS {
            break;
        }
        let Some(input) = world.mesh_input(p) else { continue };
        let version = world.chunks[&p].version;
        if input.trivially_empty(&blocks.opaque) {
            world.finished_meshes.push((p, version, None));
            world.chunks.get_mut(&p).unwrap().meshed_version = Some(version);
            continue;
        }
        let (registry, opaque) = (blocks.registry.clone(), blocks.opaque.clone());
        let task = pool.spawn(async move { build_mesh(&input.pad(), &registry, &opaque) });
        world.mesh_tasks.insert(p, (version, task));
    }
}

fn collect_meshes(
    mut commands: Commands,
    mut world: ResMut<World>,
    mut meshes: ResMut<Assets<Mesh>>,
    blocks: Res<Blocks>,
    mut stats: ResMut<WorldStats>,
) {
    let world = &mut *world;
    let mut ready = Vec::new();
    for (p, (version, task)) in world.mesh_tasks.iter_mut() {
        if let Some(mesh) = check_ready(task) {
            ready.push((*p, *version, mesh));
        }
    }
    for (p, ..) in &ready {
        world.mesh_tasks.remove(p);
    }
    world.finished_meshes.extend(ready);

    let material = blocks.material.clone().unwrap();
    // Edits (put at the front by mesh_now) go first. Empty results cost nothing;
    // real meshes are capped per frame and the rest wait for the next frame.
    let mut uploads = 0;
    let mut later = Vec::new();
    for (p, version, mesh) in std::mem::take(&mut world.finished_meshes) {
        if mesh.is_some() && uploads >= MAX_UPLOADS_PER_FRAME {
            later.push((p, version, mesh));
            continue;
        }
        let Some(chunk) = world.chunks.get_mut(&p) else { continue };
        if version != chunk.version {
            continue; // changed while meshing; a newer mesh is coming
        }
        chunk.meshed_version = Some(version);
        if let Some(e) = chunk.entity.take() {
            commands.entity(e).despawn();
        }
        if let Some(mesh) = mesh {
            uploads += 1;
            let origin = p.origin();
            chunk.entity = Some(
                commands
                    .spawn((
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d::<VoxelMaterial>(material.clone()),
                        Transform::from_xyz(origin.x as f32, origin.y as f32, origin.z as f32),
                    ))
                    .id(),
            );
        }
    }
    world.finished_meshes = later;

    stats.loaded = world.chunks.len();
    stats.meshed = world.chunks.values().filter(|c| c.entity.is_some()).count();
    stats.gen_in_flight = world.gen_tasks.len();
    stats.mesh_in_flight = world.mesh_tasks.len();
    stats.queued = world.gen_queue.len();
    stats.chunk_kib = world.chunks.values().map(|c| c.data.heap_bytes()).sum::<usize>() / 1024;
}

fn unload_far(
    mut commands: Commands,
    mut world: ResMut<World>,
    view: Res<ViewDistance>,
    cam: Single<&Transform, With<FlyCamera>>,
) {
    let center = camera_chunk(&cam);
    let r = view.chunks + 2;
    let far = |p: &ChunkPos| {
        let (dx, dz) = (p.x - center.x, p.z - center.z);
        dx * dx + dz * dz > r * r
    };
    let gone: Vec<ChunkPos> = world.chunks.keys().filter(|p| far(p)).copied().collect();
    for p in gone {
        if let Some(c) = world.chunks.remove(&p)
            && let Some(e) = c.entity
        {
            commands.entity(e).despawn();
        }
        world.mesh_tasks.remove(&p); // dropping a task cancels it
    }
    world.gen_tasks.retain(|p, _| !far(p));
}

/// Quads -> a Bevy mesh. Position, normal, UV (0..size, repeated per block in
/// the shader) and UV_1.x = texture layer. `None` if nothing is visible.
fn build_mesh(chunk: &PaddedChunk, registry: &BlockRegistry, opaque: &[bool]) -> Option<Mesh> {
    let quads = voxel::mesher::mesh(chunk, opaque);
    if quads.is_empty() {
        return None;
    }
    let n = quads.len();
    let (mut pos, mut nor, mut uv, mut layer) =
        (Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4));
    let mut idx = Vec::with_capacity(n * 6);
    for q in &quads {
        push_quad(q, registry, &mut pos, &mut nor, &mut uv, &mut layer, &mut idx);
    }
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, layer)
            .with_inserted_indices(Indices::U32(idx)),
    )
}

#[allow(clippy::too_many_arguments)]
fn push_quad(
    q: &Quad,
    registry: &BlockRegistry,
    pos: &mut Vec<[f32; 3]>,
    nor: &mut Vec<[f32; 3]>,
    uv: &mut Vec<[f32; 2]>,
    layer: &mut Vec<[f32; 2]>,
    idx: &mut Vec<u32>,
) {
    let axis = q.face.axis();
    let (ua, va) = q.face.plane_axes();
    let mut base = [q.origin[0] as f32, q.origin[1] as f32, q.origin[2] as f32];
    if q.face.is_positive() {
        base[axis] += 1.0;
    }
    let (su, sv) = (q.size_u as f32, q.size_v as f32);
    let corner = |du: f32, dv: f32| {
        let mut c = base;
        c[ua] += du;
        c[va] += dv;
        c
    };
    let corners = [corner(0.0, 0.0), corner(su, 0.0), corner(su, sv), corner(0.0, sv)];
    let normal = q.face.normal().map(|v| v as f32);
    let tex = registry.get(q.block).textures[q.face as usize] as f32;
    let start = pos.len() as u32;
    for c in corners {
        pos.push(c);
        nor.push(normal);
        // Texture coordinates from position: upright on walls, and continuous
        // across a merged quad so `fract` tiles it once per block.
        uv.push(match axis {
            0 => [c[2], -c[1]],
            1 => [c[0], c[2]],
            _ => [c[0], -c[1]],
        });
        layer.push([tex, 0.0]);
    }
    // u x v points along +axis for x and z faces, and along -axis for y faces.
    let flip = (axis == 1) == q.face.is_positive();
    if flip {
        idx.extend_from_slice(&[start, start + 2, start + 1, start, start + 3, start + 2]);
    } else {
        idx.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
    }
}
