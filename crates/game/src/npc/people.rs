//! Saving the people in the world (`people.ron`): every NPC but the gate
//! guides (those stand at their posts again on their own), with its order,
//! its dialogue flags (who is recruited: flag `recruited`) and where it is.
//! People who keep a post (gate guides, Brindlespheimer, the visiting Wren)
//! are put back by their own code, but what they remember of your talks is
//! kept per post in `posts.ron` and given back whenever they appear.
//! Saved every 30 s and on exit, not in screenshot runs.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{Npc, NpcBook, Order};
use crate::save::SaveDir;

pub struct PeoplePlugin;

impl Plugin for PeoplePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Posts>()
            .add_systems(crate::menu::GamePostStartup, load)
            .add_systems(crate::menu::GameUpdate, ((restore_posts, remember_posts).chain(), save_now_and_then))
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
    }
}

const FILE: &str = "people.ron";
const POSTS_FILE: &str = "posts.ron";

/// What each posted NPC remembers of your talks, by its post.
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Posts(pub BTreeMap<u64, Memory>);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Memory {
    pub flags: BTreeSet<String>,
    pub asked: BTreeSet<(String, usize)>,
}

/// A posted NPC that has just appeared gets back what it remembered.
fn restore_posts(posts: Res<Posts>, mut q: Query<&mut Npc, Added<Npc>>) {
    for mut npc in &mut q {
        if let Some(m) = npc.post.and_then(|p| posts.0.get(&p)) {
            npc.flags = m.flags.iter().cloned().collect();
            npc.asked = m.asked.iter().cloned().collect();
        }
    }
}

/// Keeps what posted NPCs remember, so it outlasts them.
fn remember_posts(mut posts: ResMut<Posts>, q: Query<&Npc, Changed<Npc>>) {
    for npc in &q {
        let Some(post) = npc.post else { continue };
        if npc.flags.is_empty() && npc.asked.is_empty() {
            continue;
        }
        let m = Memory { flags: npc.flags.iter().cloned().collect(), asked: npc.asked.iter().cloned().collect() };
        if posts.0.get(&post) != Some(&m) {
            posts.0.insert(post, m);
        }
    }
}

/// One NPC as saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    /// Its kind's name in npcs.ron.
    pub kind: String,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub order: SavedOrder,
    #[serde(default)]
    pub flags: BTreeSet<String>,
    #[serde(default)]
    pub asked: BTreeSet<(String, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SavedOrder {
    Stay,
    Follow,
    Wander([f32; 3]),
}

/// An NPC to save (`None` for the gate guides, which aren't saved).
pub fn to_saved(npc: &Npc, kind: &str, pos: Vec3) -> Option<Saved> {
    if npc.post.is_some() {
        return None;
    }
    let order = match npc.order {
        Order::Stay => SavedOrder::Stay,
        Order::Follow => SavedOrder::Follow,
        // A wander spot not picked yet (NaN) is where it stands.
        Order::Wander { home } => SavedOrder::Wander(if home.is_nan() { pos.to_array() } else { home.to_array() }),
    };
    Some(Saved { kind: kind.to_string(), pos: pos.to_array(), yaw: npc.yaw, order, flags: npc.flags.iter().cloned().collect(), asked: npc.asked.iter().cloned().collect() })
}

/// An NPC back from a save (`kind` is its index in npcs.ron).
pub fn from_saved(s: &Saved, kind: usize) -> Npc {
    let mut npc = Npc::new(kind, s.yaw);
    npc.order = match s.order {
        SavedOrder::Stay => Order::Stay,
        SavedOrder::Follow => Order::Follow,
        SavedOrder::Wander(home) => Order::Wander { home: Vec3::from_array(home) },
    };
    npc.flags = s.flags.iter().cloned().collect();
    npc.asked = s.asked.iter().cloned().collect();
    npc
}

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() || std::env::var_os("BLOCKGAME_BENCH").is_some()
}

fn load(dir: Res<SaveDir>, book: Res<NpcBook>, mut posts: ResMut<Posts>, mut spawn: MessageWriter<super::body::SpawnNpc>) {
    if screenshot_run() {
        return;
    }
    if let Some(p) = crate::save::read_ron::<Posts>(&dir, POSTS_FILE) {
        *posts = p;
    }
    let Ok(text) = std::fs::read_to_string(dir.file(FILE)) else { return };
    let list: Vec<Saved> = match ron::from_str(&text) {
        Ok(l) => l,
        Err(e) => {
            warn!("{FILE}: {e}");
            return;
        }
    };
    for s in &list {
        let Some(kind) = book.kinds.iter().position(|k| k.name == s.kind) else {
            warn!("{FILE}: no NPC kind `{}`", s.kind);
            continue;
        };
        spawn.write(super::body::SpawnNpc { npc: from_saved(s, kind), at: Vec3::from_array(s.pos) });
    }
}

fn store(dir: &SaveDir, book: &NpcBook, posts: &Posts, q: &Query<(&Transform, &Npc)>) {
    crate::save::write_ron(dir, POSTS_FILE, posts);
    let list: Vec<Saved> = q.iter().filter_map(|(tf, n)| to_saved(n, &book.kinds.get(n.kind)?.name, tf.translation)).collect();
    match ron::ser::to_string_pretty(&list, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file(FILE), text) {
                warn!("saving {FILE}: {e}");
            }
        }
        Err(e) => warn!("{FILE}: {e}"),
    }
}

fn save_now_and_then(time: Res<Time<Real>>, mut since: Local<f32>, dir: Res<SaveDir>, book: Res<NpcBook>, posts: Res<Posts>, q: Query<(&Transform, &Npc)>) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    store(&dir, &book, &posts, &q);
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, book: Res<NpcBook>, posts: Res<Posts>, q: Query<(&Transform, &Npc)>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &book, &posts, &q);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_memories_round_trip() {
        let mut posts = Posts::default();
        posts.0.insert(u64::MAX - 1, Memory { flags: ["met".to_string()].into(), asked: [("start".to_string(), 2)].into() });
        let text = ron::to_string(&posts).unwrap();
        assert_eq!(ron::from_str::<Posts>(&text).unwrap(), posts);
    }

    #[test]
    fn people_and_who_is_recruited_round_trip() {
        let home = Vec3::new(4.0, 70.0, -9.0);
        let mut npc = Npc::new(3, 1.2);
        npc.order = Order::Wander { home };
        npc.flags.insert(super::super::RECRUITED.to_string());
        npc.asked.insert(("greet".into(), 1));
        let saved = to_saved(&npc, "Gilm", Vec3::new(5.0, 70.0, -8.0)).unwrap();
        let text = ron::ser::to_string_pretty(&vec![saved.clone()], ron::ser::PrettyConfig::default()).unwrap();
        let back: Vec<Saved> = ron::from_str(&text).unwrap();
        assert_eq!(back, vec![saved]);
        let again = from_saved(&back[0], 3);
        assert_eq!(again.order, Order::Wander { home });
        assert!(again.flags.contains(super::super::RECRUITED));
        assert!(again.asked.contains(&("greet".to_string(), 1)));
        // A follower stays a follower.
        npc.order = Order::Follow;
        assert_eq!(from_saved(&to_saved(&npc, "Gilm", home).unwrap(), 3).order, Order::Follow);
        // Gate guides aren't saved.
        npc.post = Some(7);
        assert!(to_saved(&npc, "Ysolde Marrow", home).is_none());
    }
}
