//! Saving the foes out in the world (`foes.ron`): kind, place, home, health
//! and boost. Foes that belong to something that starts over on load are left
//! out: encounter and Rotting Gate foes (they have a `tag`) and the foes of a
//! warded dungeon run (the run's rooms fill again as the spirit walks in).
//! Saved every 30 s and on exit, not in screenshot runs.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{Foe, FoeBook, SpawnFoe};
use crate::dungeon::Nearby;
use crate::save::{SaveDir, read_ron, screenshot_run, write_ron};

const FILE: &str = "foes.ron";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedFoe {
    /// Its kind's name in foes.ron.
    pub kind: String,
    pub pos: [f32; 3],
    pub home: [f32; 3],
    pub health: f32,
    pub boost: f32,
}

pub(super) fn load(dir: Res<SaveDir>, book: Res<FoeBook>, mut spawn: MessageWriter<SpawnFoe>) {
    if screenshot_run() {
        return;
    }
    let list: Vec<SavedFoe> = read_ron(&dir, FILE).unwrap_or_default();
    for s in &list {
        let Some(kind) = book.kinds.iter().position(|k| k.name == s.kind) else {
            warn!("{FILE}: no foe kind `{}`", s.kind);
            continue;
        };
        spawn.write(SpawnFoe {
            boost: s.boost,
            home: Some(Vec3::from_array(s.home)),
            health: Some(s.health),
            ..SpawnFoe::new(kind, Vec3::from_array(s.pos))
        });
    }
}

/// Whether a foe is saved: not tagged and not part of a dungeon run.
pub fn kept(foe: &Foe, nearby: &Nearby) -> bool {
    foe.tag == 0 && foe.health > 0.0 && !nearby.list.iter().any(|d| d.inside(foe.home.to_array()))
}

fn store(dir: &SaveDir, book: &FoeBook, nearby: &Nearby, q: &Query<(&Foe, &Transform)>) {
    let list: Vec<SavedFoe> = q
        .iter()
        .filter(|(f, _)| kept(f, nearby))
        .filter_map(|(f, tf)| {
            Some(SavedFoe {
                kind: book.kinds.get(f.kind)?.name.clone(),
                pos: tf.translation.to_array(),
                home: f.home.to_array(),
                health: f.health,
                boost: f.boost,
            })
        })
        .collect();
    write_ron(dir, FILE, &list);
}

pub(super) fn save_now_and_then(
    time: Res<Time<Real>>,
    mut since: Local<f32>,
    dir: Res<SaveDir>,
    book: Res<FoeBook>,
    nearby: Res<Nearby>,
    q: Query<(&Foe, &Transform)>,
) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    store(&dir, &book, &nearby, &q);
}

pub(super) fn save_on_exit(
    mut exit: MessageReader<AppExit>,
    dir: Res<SaveDir>,
    book: Res<FoeBook>,
    nearby: Res<Nearby>,
    q: Query<(&Foe, &Transform)>,
) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &book, &nearby, &q);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_foes_round_trip() {
        let list = vec![SavedFoe {
            kind: "hollow".into(),
            pos: [1.0, 2.0, 3.0],
            home: [0.0, 2.0, 0.0],
            health: 12.5,
            boost: 1.0,
        }];
        let text = ron::to_string(&list).unwrap();
        assert_eq!(ron::from_str::<Vec<SavedFoe>>(&text).unwrap(), list);
    }
}
