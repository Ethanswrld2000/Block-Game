//! Saving the player's block edits.
//!
//! Only chunks the player changed are saved; everything else regenerates
//! identically from the seed. Files live in `saves/<world>/chunks/` next to
//! where the game runs. Edited chunks are written when they unload, every 30
//! seconds, and when the game closes.

use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use voxel::{BlockRegistry, Chunk, ChunkPos};

use crate::blocks::Blocks;
use crate::world::{World, SEED};

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        let dir = SaveDir::new("world1");
        app.insert_resource(dir)
            .insert_resource(AutosaveTimer(Timer::from_seconds(30.0, TimerMode::Repeating)))
            .add_systems(Update, autosave)
            .add_systems(Last, save_on_exit);
    }
}

#[derive(Resource, Clone)]
pub struct SaveDir {
    root: Arc<PathBuf>,
    chunks: Arc<PathBuf>,
}

#[derive(Resource)]
struct AutosaveTimer(Timer);

impl SaveDir {
    fn new(world: &str) -> Self {
        let root = std::env::var("BLOCKGAME_SAVES").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("saves"));
        let dir = root.join(world);
        let chunks = dir.join("chunks");
        if let Err(e) = std::fs::create_dir_all(&chunks) {
            warn!("cannot create save folder {}: {e}", chunks.display());
        }
        let meta = dir.join("world.ron");
        if !meta.exists() {
            let _ = std::fs::write(&meta, format!("(seed: {SEED}, version: 1)\n"));
        }
        Self { root: Arc::new(dir), chunks: Arc::new(chunks) }
    }

    /// A file shared by all worlds, next to the world folders (`settings.ron`).
    pub fn shared_file(&self, name: &str) -> PathBuf {
        self.root.parent().map_or_else(|| PathBuf::from(name), |p| p.join(name))
    }

    /// A file in the world's folder, such as `time.ron`.
    pub fn file(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn path(&self, p: ChunkPos) -> PathBuf {
        self.chunks.join(format!("{}.{}.{}.chunk", p.x, p.y, p.z))
    }

    /// A saved chunk, if the player ever changed it.
    pub fn load(&self, p: ChunkPos, blocks: &BlockRegistry) -> Option<Chunk> {
        let bytes = std::fs::read(self.path(p)).ok()?;
        let chunk = voxel::save::decode(&bytes, blocks);
        if chunk.is_none() {
            warn!("save file for chunk {p:?} is damaged; regenerating it");
        }
        chunk
    }

    pub fn store(&self, p: ChunkPos, chunk: &Chunk, blocks: &BlockRegistry) {
        let bytes = voxel::save::encode(chunk, blocks);
        // Write then rename, so a crash mid-write never leaves a half file.
        let path = self.path(p);
        let tmp = path.with_extension("tmp");
        if let Err(e) = std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &path)) {
            warn!("could not save chunk {p:?}: {e}");
        }
    }
}

fn save_all(world: &mut World, dir: &SaveDir, blocks: &Blocks) -> usize {
    let modified = world.take_modified();
    let n = modified.len();
    for (p, chunk) in modified {
        dir.store(p, &chunk, &blocks.registry);
    }
    n
}

fn autosave(time: Res<Time>, mut timer: ResMut<AutosaveTimer>, mut world: ResMut<World>, dir: Res<SaveDir>, blocks: Res<Blocks>) {
    if timer.0.tick(time.delta()).just_finished() {
        let n = save_all(&mut world, &dir, &blocks);
        if n > 0 {
            info!("autosaved {n} chunks");
        }
    }
}

fn save_on_exit(mut exit: MessageReader<AppExit>, mut world: ResMut<World>, dir: Res<SaveDir>, blocks: Res<Blocks>) {
    if exit.read().next().is_some() {
        let n = save_all(&mut world, &dir, &blocks);
        info!("saved {n} chunks on exit");
    }
}
