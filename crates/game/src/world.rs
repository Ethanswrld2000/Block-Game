//! Chunk streaming: generate (or load), mesh, upload and unload chunks around
//! the camera.
//!
//! Generation and meshing run on Bevy's `AsyncComputeTaskPool`. The main
//! thread only schedules work, collects finished results and uploads a
//! limited number of meshes per frame so nothing ever stalls.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, AsyncComputeTaskPool, Task};
use voxel::{BlockId, BlockPos, BlockRegistry, Chunk, ChunkPos, Face, PaddedChunk, CHUNK_SIZE};
use worldgen::WorldGen;

use crate::blocks::{Blocks, VoxelMaterial};
use crate::camera::FlyCamera;
use crate::chunk_mesh::{self, ChunkMeshes};
use crate::save::SaveDir;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ViewDistance { chunks: 12, vertical: 5 })
            .init_resource::<World>()
            .init_resource::<WorldStats>()
            .init_resource::<PocketFloor>()
            .add_systems(crate::menu::GamePostStartup, setup_generator)
            .add_systems(
                crate::menu::GameUpdate,
                (
                    change_view_distance,
                    schedule_generation,
                    collect_generated,
                    schedule_meshing,
                    collect_meshes,
                    unload_far,
                )
                    .chain()
                    .run_if(|b: Res<Blocks>| b.material.is_some()),
            );
    }
}

/// The seed every world had before the title screen; folders without a
/// `world.ron` still get it.
pub const SEED: u64 = 0x005E_ED0F_5A4D;

/// The open world's seed (from its `world.ron`, set by the title screen).
#[derive(Resource, Clone, Copy, Debug)]
pub struct WorldSeed(pub u64);

/// Concurrent tasks. Enough to keep every core busy without flooding memory.
const MAX_GEN_TASKS: usize = 48;
const MAX_MESH_TASKS: usize = 32;
/// Meshes uploaded to the GPU per frame.
const MAX_UPLOADS_PER_FRAME: usize = 24;

/// View radius in chunks: horizontal (change in game with `-` and `=`) and vertical.
#[derive(Resource)]
pub struct ViewDistance {
    pub chunks: i32,
    pub vertical: i32,
}

#[derive(Resource, Clone)]
pub struct Generator(pub Arc<WorldGen>);

/// The Rotting Gate floor being played, if any: chunks in the pocket
/// (`worldgen::in_pocket`) are built from it instead of the terrain, and never saved.
#[derive(Resource, Default, Clone)]
pub struct PocketFloor(pub Option<Arc<worldgen::Floor>>);

fn in_pocket(p: ChunkPos) -> bool {
    let o = p.origin();
    worldgen::in_pocket(o.x, o.z)
}

struct LoadedChunk {
    data: Arc<Chunk>,
    entities: Vec<Entity>,
    /// Bumped on every change, so a stale mesh result is ignored.
    version: u32,
    meshed_version: Option<u32>,
    /// A mesh has been put on screen (the distant terrain hides itself here).
    drawn: bool,
    /// Edited by the player since it was last saved.
    modified: bool,
    /// Light from its last mesh (`ChunkMeshes::light`), for lighting NPCs.
    light: Option<Vec<u8>>,
}

type MeshResult = (ChunkPos, u32, ChunkMeshes);

/// All loaded chunks plus in-flight work.
#[derive(Resource, Default)]
pub struct World {
    chunks: HashMap<ChunkPos, LoadedChunk>,
    gen_tasks: HashMap<ChunkPos, Task<Chunk>>,
    mesh_tasks: HashMap<ChunkPos, (u32, Task<ChunkMeshes>)>,
    finished_meshes: Vec<MeshResult>,
    /// Chunks to generate, nearest last (so `pop` takes the nearest).
    gen_queue: Vec<ChunkPos>,
    queue_key: Option<(ChunkPos, i32, i32)>,
    /// Vertical band of chunk y that is kept loaded around the camera.
    y_range: (i32, i32),
    /// Per block id: drawn as its own model (torches), not in the chunk mesh.
    shaped: Arc<Vec<bool>>,
    /// Per block id: stops light (`voxel::light`).
    stops_light: Arc<Vec<bool>>,
    /// Shaped blocks that appeared or went away, for the systems that draw them.
    pub shape_events: Vec<ShapeEvent>,
    /// Every cell `set_block` changed since `game::cover` last looked (water
    /// next to it may start to flow).
    pub changed: Vec<BlockPos>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeEvent {
    Added(BlockPos, BlockId),
    Removed(BlockPos),
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
    pub unsaved: usize,
}

impl World {
    pub fn is_loaded(&self, c: ChunkPos) -> bool {
        self.chunks.contains_key(&c)
    }

    /// (sky, block) light at `p`, 0..=15, from its chunk's last mesh. Full sky
    /// where a loaded chunk has no light worked out (open air chunks); `None` if not loaded.
    pub fn light_at(&self, p: BlockPos) -> Option<(u8, u8)> {
        let c = self.chunks.get(&p.chunk())?;
        let [x, y, z] = p.local();
        Some(c.light.as_deref().map_or((voxel::light::MAX, 0), |l| voxel::light::coarse_at(l, x, y, z)))
    }

    /// Every `block` in loaded chunks within the box `min..=max`. Chunks that
    /// can't hold it are skipped without looking at their cells.
    pub fn find_blocks(&self, min: BlockPos, max: BlockPos, block: BlockId) -> Vec<BlockPos> {
        let mut found = Vec::new();
        let (cmin, cmax) = (min.chunk(), max.chunk());
        for cx in cmin.x..=cmax.x {
            for cy in cmin.y..=cmax.y {
                for cz in cmin.z..=cmax.z {
                    let cp = ChunkPos { x: cx, y: cy, z: cz };
                    let Some(c) = self.chunks.get(&cp) else { continue };
                    if !c.data.may_contain(block) {
                        continue;
                    }
                    let o = cp.origin();
                    let lo = |m: i32, o: i32| (m - o).clamp(0, CHUNK_SIZE as i32 - 1) as usize;
                    for y in lo(min.y, o.y)..=lo(max.y, o.y) {
                        for z in lo(min.z, o.z)..=lo(max.z, o.z) {
                            for x in lo(min.x, o.x)..=lo(max.x, o.x) {
                                if c.data.get(x, y, z) == block {
                                    found.push(BlockPos::new(o.x + x as i32, o.y + y as i32, o.z + z as i32));
                                }
                            }
                        }
                    }
                }
            }
        }
        found
    }

    pub fn block(&self, p: BlockPos) -> Option<BlockId> {
        let [x, y, z] = p.local();
        self.chunks.get(&p.chunk()).map(|c| c.data.get(x, y, z))
    }

    /// Change a block and mark what it touches for remeshing.
    pub fn set_block(&mut self, p: BlockPos, block: BlockId) -> bool {
        let cp = p.chunk();
        let [x, y, z] = p.local();
        let Some(c) = self.chunks.get_mut(&cp) else { return false };
        let old = c.data.get(x, y, z);
        if old == block {
            return false;
        }
        if self.shaped.get(old.0 as usize).copied().unwrap_or(false) {
            self.shape_events.push(ShapeEvent::Removed(p));
        }
        if self.shaped.get(block.0 as usize).copied().unwrap_or(false) {
            self.shape_events.push(ShapeEvent::Added(p, block));
        }
        let Some(c) = self.chunks.get_mut(&cp) else { return false };
        Arc::make_mut(&mut c.data).set(x, y, z, block);
        if self.changed.len() < 65_536 {
            self.changed.push(p);
        }
        c.version += 1;
        c.modified = true;
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
        // Light reaches 15 blocks: the neighbours on the near side of each
        // axis; and if the block opened or shut a way for the sky, the chunks below too.
        let stops = |b: BlockId| self.stops_light.get(b.0 as usize).copied().unwrap_or(false);
        let sky = stops(old) != stops(block);
        let near = |l: usize| -> (i32, i32) {
            if l < voxel::light::MARGIN { (-1, 0) } else { (0, 1) }
        };
        let ((x0, x1), (z0, z1)) = (near(x), near(z));
        let (y0, y1) = near(y);
        let y0 = if sky { -1 - SKY_REACH } else { y0 };
        for dy in y0..=y1 {
            for dz in z0..=z1 {
                for dx in x0..=x1 {
                    touched.push(ChunkPos::new(cp.x + dx, cp.y + dy, cp.z + dz));
                }
            }
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
    pub fn mesh_now(&mut self, pos: ChunkPos, blocks: &Blocks) {
        if let Some(input) = self.mesh_input(pos) {
            let version = self.chunks[&pos].version;
            self.mesh_tasks.remove(&pos);
            self.finished_meshes.insert(0, (pos, version, input.build(&blocks.registry, &blocks.opaque, &blocks.light)));
        }
    }

    /// True if a loaded chunk's mesh is out of date.
    pub fn needs_mesh(&self, pos: ChunkPos) -> bool {
        self.chunks.get(&pos).is_some_and(|c| c.meshed_version != Some(c.version))
    }

    /// Drop every loaded or loading chunk in the pocket, so they are built
    /// again from the new floor. Returns the entities to despawn.
    pub fn forget_pocket(&mut self) -> Vec<Entity> {
        let gone: Vec<ChunkPos> = self.chunks.keys().filter(|p| in_pocket(**p)).copied().collect();
        let mut out = Vec::new();
        for p in gone {
            if let Some(c) = self.chunks.remove(&p) {
                out.extend(c.entities);
            }
            self.mesh_tasks.remove(&p);
        }
        self.gen_tasks.retain(|p, _| !in_pocket(*p));
        self.finished_meshes.retain(|(p, _, _)| !in_pocket(*p));
        self.queue_key = None;
        out
    }

    /// Chunks edited since they were last saved, and mark them saved.
    /// (Pocket chunks are never saved: each run builds its floor fresh.)
    pub fn take_modified(&mut self) -> Vec<(ChunkPos, Arc<Chunk>)> {
        self.chunks
            .iter_mut()
            .filter(|(p, c)| c.modified && !in_pocket(**p))
            .map(|(p, c)| {
                c.modified = false;
                (*p, c.data.clone())
            })
            .collect()
    }

    /// The chunk, its 26 neighbours and the chunks over them (for sky light),
    /// if all that are in the loaded band are loaded.
    fn mesh_input(&self, pos: ChunkPos) -> Option<MeshInput> {
        let in_band = |y: i32| y >= self.y_range.0 && y <= self.y_range.1;
        let mut chunks: [Option<Arc<Chunk>>; 27] = Default::default();
        for dy in -1..=1 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let np = ChunkPos::new(pos.x + dx, pos.y + dy, pos.z + dz);
                    if (dx, dy, dz) != (0, 0, 0) && !in_band(np.y) {
                        continue; // outside the loaded band: treat as air
                    }
                    chunks[voxel::light::neighbour_index(dx, dy, dz)] = Some(self.chunks.get(&np)?.data.clone());
                }
            }
        }
        let mut above: [Vec<Arc<Chunk>>; 9] = Default::default();
        for dz in -1..=1 {
            for dx in -1..=1 {
                for up in 2..2 + SKY_REACH {
                    let np = ChunkPos::new(pos.x + dx, pos.y + up, pos.z + dz);
                    if !in_band(np.y) {
                        break; // open sky assumed above the band
                    }
                    above[voxel::light::column_index(dx, dz)].push(self.chunks.get(&np)?.data.clone());
                }
            }
        }
        Some(MeshInput { chunks, above })
    }

    /// A chunk arrived or changed: mark the meshed chunks whose light it can
    /// change (its neighbours and, if `sky`, the chunks below it) for remeshing.
    fn touch_light(&mut self, c: ChunkPos, sky: bool) {
        let lowest = if sky { -1 - SKY_REACH } else { -1 };
        for dy in lowest..=1 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    if (dx, dy, dz) == (0, 0, 0) {
                        continue;
                    }
                    if let Some(n) = self.chunks.get_mut(&ChunkPos::new(c.x + dx, c.y + dy, c.z + dz))
                        && n.meshed_version.is_some()
                    {
                        n.version += 1;
                        n.meshed_version = None;
                    }
                }
            }
        }
    }
}

/// Chunks over the 3x3x3 neighbourhood looked through for sky light: a spot
/// with this many chunks (plus part of one) of open air above counts as
/// under open sky.
const SKY_REACH: i32 = 3;

struct MeshInput {
    chunks: [Option<Arc<Chunk>>; 27],
    above: [Vec<Arc<Chunk>>; 9],
}

impl MeshInput {
    fn center(&self) -> &Chunk {
        self.chunks[voxel::light::neighbour_index(0, 0, 0)].as_deref().expect("mesh input has its chunk")
    }

    /// Light the chunk and mesh it.
    fn build(&self, registry: &BlockRegistry, opaque: &[bool], tables: &voxel::light::Tables) -> ChunkMeshes {
        let hood = voxel::light::Neighbourhood {
            chunks: std::array::from_fn(|i| self.chunks[i].as_deref()),
            above: std::array::from_fn(|i| self.above[i].iter().map(|c| &**c).collect()),
        };
        let light = voxel::light::compute(&hood, tables);
        chunk_mesh::build(&self.pad(), &light, registry, opaque)
    }

    /// Copy the chunk and a 1-block border from each face neighbour into a padded array.
    fn pad(&self) -> PaddedChunk {
        let mut p = PaddedChunk::default();
        let n = CHUNK_SIZE;
        let dense = self.center().to_dense();
        for y in 0..n {
            for z in 0..n {
                for x in 0..n {
                    p.set(x + 1, y + 1, z + 1, dense[voxel::chunk::index(x, y, z)]);
                }
            }
        }
        for face in Face::ALL {
            let d = face.normal();
            let Some(nb) = &self.chunks[voxel::light::neighbour_index(d[0], d[1], d[2])] else { continue };
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
        match self.center().is_uniform() {
            Some(b) if b.is_air() => true,
            Some(b) if opaque[b.0 as usize] => Face::ALL.iter().all(|f| {
                let d = f.normal();
                self.chunks[voxel::light::neighbour_index(d[0], d[1], d[2])]
                    .as_ref()
                    .and_then(|c| c.is_uniform())
                    .is_some_and(|nb| opaque[nb.0 as usize])
            }),
            _ => false,
        }
    }
}

pub fn setup_generator(
    mut commands: Commands,
    blocks: Res<Blocks>,
    seed: Res<WorldSeed>,
    mut world: ResMut<World>,
    mut player: ResMut<crate::player::Player>,
    mut cam: Query<&mut Transform, With<FlyCamera>>,
) {
    let wg = WorldGen::new(seed.0, &blocks.registry, include_str!("../../../assets/data/biomes.ron"))
        .unwrap_or_else(|e| panic!("{e}"));
    let [x, y, z] = wg.find_spawn();
    info!("spawn at {x} {y} {z} in {}", wg.biome_at(x, y, z).title);
    for z in wg.zones() {
        info!("{} at {} {} (radius {})", wg.biomes.get(z.biome).title, z.x, z.z, z.radius);
    }
    world.shaped = Arc::new(blocks.registry.iter().map(|(_, d)| d.shape == voxel::Shape::Torch).collect());
    world.stops_light = Arc::new(blocks.light.blocks.clone());
    player.pos = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
    for mut tf in &mut cam {
        tf.translation = player.pos + Vec3::Y * crate::player::EYE;
    }
    commands.insert_resource(Generator(Arc::new(wg)));
}

fn change_view_distance(keys: Res<ButtonInput<KeyCode>>, mut view: ResMut<ViewDistance>) {
    if keys.just_pressed(KeyCode::Equal) {
        view.chunks = (view.chunks + 2).min(32);
    }
    if keys.just_pressed(KeyCode::Minus) {
        view.chunks = (view.chunks - 2).max(4);
    }
}

/// Vertical offsets the drawn-chunk mask holds: -MASK_UP..=MASK_UP (31 bits).
pub const MASK_UP: i32 = 15;

impl World {
    /// Which chunks around `center` are drawn, for the distant-terrain shader:
    /// a `(2 * half + 1)^2` grid of columns (x fastest), each a bit mask of
    /// vertical offsets (bit `dy + MASK_UP`). Shares the chunk grid exactly, so
    /// the distant terrain is cut away only where real chunks cover it.
    pub fn drawn_mask(&self, center: ChunkPos, half: i32) -> Vec<u32> {
        let n = 2 * half + 1;
        let mut mask = vec![0u32; (n * n) as usize];
        for (p, c) in &self.chunks {
            let (dx, dy, dz) = (p.x - center.x, p.y - center.y, p.z - center.z);
            if c.drawn && dx.abs() <= half && dz.abs() <= half && dy.abs() <= MASK_UP {
                mask[((dz + half) * n + dx + half) as usize] |= 1 << (dy + MASK_UP);
            }
        }
        mask
    }
}

pub fn camera_chunk(cam: &Transform) -> ChunkPos {
    BlockPos::from_world(cam.translation.to_array()).chunk()
}

fn schedule_generation(
    mut world: ResMut<World>,
    generator: Res<Generator>,
    blocks: Res<Blocks>,
    save: Res<SaveDir>,
    view: Res<ViewDistance>,
    pocket: Res<PocketFloor>,
    cam: Single<&Transform, With<FlyCamera>>,
) {
    let center = camera_chunk(&cam);
    let (r, rv) = (view.chunks, view.vertical);
    world.y_range = (center.y - rv, center.y + rv);
    // Rebuild the queue when the camera enters a new chunk or the view changes.
    let key = (center, r, rv);
    if world.queue_key != Some(key) {
        world.queue_key = Some(key);
        let mut queue = Vec::new();
        for z in -r..=r {
            for x in -r..=r {
                if x * x + z * z > r * r {
                    continue;
                }
                for y in -rv..=rv {
                    let p = ChunkPos::new(center.x + x, center.y + y, center.z + z);
                    if !world.chunks.contains_key(&p) && !world.gen_tasks.contains_key(&p) {
                        queue.push(p);
                    }
                }
            }
        }
        // Nearest last; vertical distance counts extra so your level comes first.
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
        let (g, reg, dir) = (generator.0.clone(), blocks.registry.clone(), save.clone());
        if in_pocket(p) {
            let floor = pocket.0.clone();
            world.gen_tasks.insert(p, pool.spawn(async move { floor.map_or_else(|| Chunk::filled(voxel::BlockId::AIR), |f| g.generate_pocket(p, &f)) }));
            continue;
        }
        // A saved chunk (one the player changed) wins over a freshly generated one.
        world.gen_tasks.insert(p, pool.spawn(async move { dir.load(p, &reg).unwrap_or_else(|| g.generate(p)) }));
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
        // Find any torches (or other shaped blocks) so they get drawn.
        let shaped: Vec<BlockId> = (0..world.shaped.len()).filter(|&i| world.shaped[i]).map(|i| BlockId(i as u16)).collect();
        if shaped.iter().any(|b| chunk.may_contain(*b)) {
            let o = p.origin();
            let dense = chunk.to_dense();
            for (i, b) in dense.iter().enumerate() {
                if shaped.contains(b) {
                    let (x, z, y) = (i % CHUNK_SIZE, (i / CHUNK_SIZE) % CHUNK_SIZE, i / (CHUNK_SIZE * CHUNK_SIZE));
                    let bp = BlockPos::new(o.x + x as i32, o.y + y as i32, o.z + z as i32);
                    world.shape_events.push(ShapeEvent::Added(bp, *b));
                }
            }
        }
        world.chunks.insert(
            p,
            LoadedChunk { data: Arc::new(chunk), entities: Vec::new(), version: 0, meshed_version: None, drawn: false, modified: false, light: None },
        );
        // Neighbours meshed while this chunk was outside the vertical band
        // treated it as air (and open sky); remesh them now that the real blocks are here.
        world.touch_light(p, true);
    }
}

fn schedule_meshing(mut world: ResMut<World>, blocks: Res<Blocks>, cam: Single<&Transform, With<FlyCamera>>) {
    if world.mesh_tasks.len() >= MAX_MESH_TASKS {
        return;
    }
    let center = camera_chunk(&cam);
    let waiting: HashSet<ChunkPos> = world.finished_meshes.iter().map(|(p, ..)| *p).collect();
    let mut wanted: Vec<ChunkPos> = world
        .chunks
        .iter()
        .filter(|(p, c)| c.meshed_version != Some(c.version) && !world.mesh_tasks.contains_key(p) && !waiting.contains(p))
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
            world.finished_meshes.push((p, version, ChunkMeshes::default()));
            continue;
        }
        let (registry, opaque, tables) = (blocks.registry.clone(), blocks.opaque.clone(), blocks.light.clone());
        let task = pool.spawn(async move { input.build(&registry, &opaque, &tables) });
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

    let (Some(opaque_mat), Some(water_mat)) = (blocks.material.clone(), blocks.translucent_material.clone()) else {
        return;
    };
    // Edits (put at the front by mesh_now) go first. Empty results cost nothing;
    // real meshes are capped per frame and the rest wait for the next frame.
    let mut uploads = 0;
    let mut later = Vec::new();
    for (p, version, mut m) in std::mem::take(&mut world.finished_meshes) {
        if !m.is_empty() && uploads >= MAX_UPLOADS_PER_FRAME {
            later.push((p, version, m));
            continue;
        }
        let Some(chunk) = world.chunks.get_mut(&p) else { continue };
        if version != chunk.version {
            continue; // changed while meshing; a newer mesh is coming
        }
        chunk.meshed_version = Some(version);
        chunk.drawn = true;
        chunk.light = m.light.take();
        for e in chunk.entities.drain(..) {
            commands.entity(e).despawn();
        }
        let origin = p.origin();
        let tf = Transform::from_xyz(origin.x as f32, origin.y as f32, origin.z as f32);
        for (mesh, mat) in [(m.opaque, &opaque_mat), (m.translucent, &water_mat)] {
            if let Some(mesh) = mesh {
                uploads += 1;
                chunk.entities.push(
                    commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d::<VoxelMaterial>(mat.clone()), tf)).id(),
                );
            }
        }
        // Plants cast no shadow: their crossed quads would shadow as solid squares.
        if let Some(mesh) = m.plants {
            uploads += 1;
            chunk.entities.push(
                commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d::<VoxelMaterial>(opaque_mat.clone()), tf, bevy::light::NotShadowCaster)).id(),
            );
        }
    }
    world.finished_meshes = later;

    stats.loaded = world.chunks.len();
    stats.meshed = world.chunks.values().filter(|c| !c.entities.is_empty()).count();
    stats.gen_in_flight = world.gen_tasks.len();
    stats.mesh_in_flight = world.mesh_tasks.len();
    stats.queued = world.gen_queue.len();
    stats.chunk_kib = world.chunks.values().map(|c| c.data.heap_bytes()).sum::<usize>() / 1024;
    stats.unsaved = world.chunks.values().filter(|c| c.modified).count();
}

pub fn unload_far(
    mut commands: Commands,
    mut world: ResMut<World>,
    view: Res<ViewDistance>,
    blocks: Res<Blocks>,
    save: Res<SaveDir>,
    cam: Single<&Transform, With<FlyCamera>>,
) {
    let center = camera_chunk(&cam);
    let (r, rv) = (view.chunks + 2, view.vertical + 2);
    let far = |p: &ChunkPos| {
        let (dx, dy, dz) = (p.x - center.x, p.y - center.y, p.z - center.z);
        dx * dx + dz * dz > r * r || dy.abs() > rv
    };
    let gone: Vec<ChunkPos> = world.chunks.keys().filter(|p| far(p)).copied().collect();
    for p in gone {
        if let Some(c) = world.chunks.remove(&p) {
            for e in c.entities {
                commands.entity(e).despawn();
            }
            if c.modified && !in_pocket(p) {
                save.store(p, &c.data, &blocks.registry);
            }
        }
        world.mesh_tasks.remove(&p); // dropping a task cancels it
    }
    world.gen_tasks.retain(|p, _| !far(p));
}
