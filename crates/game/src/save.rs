//! Saving the player's block edits, and the list of worlds.
//!
//! Only chunks the player changed are saved; everything else regenerates
//! identically from the seed. Each world is a folder `saves/<folder>/` next to
//! where the game runs (override with `BLOCKGAME_SAVES`), with `world.ron`
//! (name, seed, when it was made and last played) and `chunks/`. Edited
//! chunks are written when they unload, every 30 seconds, and when the game
//! closes. The title screen (`menu`) lists, creates and opens worlds.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use voxel::{BlockRegistry, Chunk, ChunkPos};

use crate::blocks::Blocks;
use crate::world::{World, SEED};

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        // `SaveDir` is inserted by the title screen when a world is opened.
        app.insert_resource(AutosaveTimer(Timer::from_seconds(30.0, TimerMode::Repeating)))
            .add_systems(crate::menu::GameUpdate, autosave)
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
    }
}

/// The folder holding every world (`saves`, or `BLOCKGAME_SAVES`).
pub fn saves_root() -> PathBuf {
    std::env::var("BLOCKGAME_SAVES").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("saves"))
}

/// `world.ron`: what the title screen shows about a world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldMeta {
    /// Shown in the list; the folder name if empty (worlds from before names).
    pub name: String,
    pub seed: u64,
    pub version: u32,
    /// Seconds since 1970.
    pub created: u64,
    pub last_played: u64,
}

impl Default for WorldMeta {
    fn default() -> Self {
        // The fixed seed every world had before the title screen.
        Self { name: String::new(), seed: SEED, version: 1, created: 0, last_played: 0 }
    }
}

/// A world found in the saves folder.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedWorld {
    pub folder: String,
    pub meta: WorldMeta,
}

impl SavedWorld {
    pub fn name(&self) -> &str {
        if self.meta.name.is_empty() { &self.folder } else { &self.meta.name }
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn read_meta(dir: &Path) -> Option<WorldMeta> {
    let text = std::fs::read_to_string(dir.join("world.ron")).ok()?;
    match ron::from_str::<WorldMeta>(&text) {
        Ok(m) => Some(m),
        Err(e) => {
            warn!("{}: {e}", dir.join("world.ron").display());
            None
        }
    }
}

fn write_meta(dir: &Path, meta: &WorldMeta) {
    let text = ron::ser::to_string_pretty(meta, ron::ser::PrettyConfig::default()).unwrap_or_default();
    if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(dir.join("world.ron"), text)) {
        warn!("saving {}: {e}", dir.join("world.ron").display());
    }
}

/// Every world in `root`, most recently played first.
pub fn list_worlds(root: &Path) -> Vec<SavedWorld> {
    let mut worlds: Vec<SavedWorld> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("world.ron").is_file())
        .filter_map(|e| {
            let folder = e.file_name().to_str()?.to_string();
            let meta = read_meta(&e.path())?;
            Some(SavedWorld { folder, meta })
        })
        .collect();
    worlds.sort_by(|a, b| b.meta.last_played.cmp(&a.meta.last_played).then_with(|| a.folder.cmp(&b.folder)));
    worlds
}

/// A folder name for a world called `name`: lower case letters, digits and
/// dashes, made unique with a number if `taken` says so.
pub fn folder_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut base = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            base.push(c);
        } else if !base.is_empty() && !base.ends_with('-') {
            base.push('-');
        }
    }
    let base = base.trim_end_matches('-');
    let base: String = if base.is_empty() { "world".into() } else { base.chars().take(40).collect() };
    if !taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|f| !taken(f)).unwrap_or(base)
}

/// The seed typed on the new world screen: a number is used as it is (a minus
/// sign is fine), any other text is hashed, and a blank field means "pick one".
pub fn seed_from_text(text: &str) -> Option<u64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(n) = t.parse::<u64>() {
        return Some(n);
    }
    if let Ok(n) = t.parse::<i64>() {
        return Some(n as u64);
    }
    // FNV-1a: the same words always give the same world.
    Some(t.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)))
}

/// A seed from the clock, for a blank seed field.
pub fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
    // splitmix64, so seeds made a moment apart look nothing alike.
    let mut z = nanos.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) % 10_000_000_000
}

/// Makes a new world folder in `root` and returns its folder name.
pub fn create_world(root: &Path, name: &str, seed: u64) -> String {
    let folder = folder_name(name, |f| root.join(f).exists());
    let now = now_secs();
    write_meta(&root.join(&folder), &WorldMeta { name: name.trim().to_string(), seed, version: 1, created: now, last_played: now });
    folder
}

/// "just now", "5 minutes ago", "3 days ago".
pub fn ago(then: u64, now: u64) -> String {
    if then == 0 {
        return "a while ago".into();
    }
    let s = now.saturating_sub(then);
    let (n, unit) = match s {
        0..60 => return "just now".into(),
        60..3600 => (s / 60, "minute"),
        3600..86_400 => (s / 3600, "hour"),
        86_400..2_592_000 => (s / 86_400, "day"),
        _ => (s / 2_592_000, "month"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

#[derive(Resource, Clone)]
pub struct SaveDir {
    root: Arc<PathBuf>,
    chunks: Arc<PathBuf>,
}

#[derive(Resource)]
struct AutosaveTimer(Timer);

impl SaveDir {
    /// Opens (or makes) the world in `saves/<folder>/`, marks it played now and
    /// returns its details. A folder without `world.ron` gets the old fixed seed.
    pub fn open(folder: &str) -> (Self, WorldMeta) {
        let dir = saves_root().join(folder);
        let chunks = dir.join("chunks");
        if let Err(e) = std::fs::create_dir_all(&chunks) {
            warn!("cannot create save folder {}: {e}", chunks.display());
        }
        let mut meta = read_meta(&dir).unwrap_or_default();
        if meta.name.is_empty() {
            meta.name = folder.to_string();
        }
        let now = now_secs();
        if meta.created == 0 {
            meta.created = now;
        }
        meta.last_played = now;
        // Screenshot runs leave the saves as they were.
        if std::env::var_os("BLOCKGAME_SCREENSHOT").is_none() || !dir.join("world.ron").exists() {
            write_meta(&dir, &meta);
        }
        (Self { root: Arc::new(dir), chunks: Arc::new(chunks) }, meta)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names() {
        assert_eq!(folder_name("  Marsh Home! ", |_| false), "marsh-home");
        assert_eq!(folder_name("???", |_| false), "world");
        assert_eq!(folder_name("Fen", |f| f == "fen" || f == "fen-2"), "fen-3");
    }

    #[test]
    fn seeds() {
        assert_eq!(seed_from_text(""), None);
        assert_eq!(seed_from_text(" 42 "), Some(42));
        assert_eq!(seed_from_text("-1"), Some(u64::MAX));
        assert_eq!(seed_from_text("bog"), seed_from_text("bog"));
        assert_ne!(seed_from_text("bog"), seed_from_text("fen"));
    }

    #[test]
    fn old_world_files_still_load() {
        let m: WorldMeta = ron::from_str("(seed: 404, version: 1)\n").unwrap();
        assert_eq!((m.seed, m.name.as_str(), m.last_played), (404, "", 0));
    }

    #[test]
    fn worlds_are_listed_newest_first() {
        let root = std::env::temp_dir().join(format!("blockgame-test-{}", random_seed()));
        let a = create_world(&root, "Alpha", 1);
        let b = create_world(&root, "Alpha", 2);
        assert_eq!((a.as_str(), b.as_str()), ("alpha", "alpha-2"));
        write_meta(&root.join(&a), &WorldMeta { name: "Alpha".into(), seed: 1, last_played: now_secs() + 10, ..default() });
        let list = list_worlds(&root);
        assert_eq!(list.iter().map(|w| w.folder.as_str()).collect::<Vec<_>>(), ["alpha", "alpha-2"]);
        assert_eq!(list[1].meta.seed, 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ago_reads_well() {
        assert_eq!(ago(100, 130), "just now");
        assert_eq!(ago(100, 3700), "1 hour ago");
        assert_eq!(ago(1, 3 * 86_400 + 5), "3 days ago");
    }
}
