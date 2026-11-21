//! Saving the creatures out in the world (`creatures.ron`): every wild
//! creature (not critters, which come back on their own) with its
//! kind, place, health and leash, and when each barrow last woke its wights.
//! Saved every 30 s and on exit, not in screenshot runs.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::data::{Kinds, Temper};
use super::{spawn_creature, Barrows, Creature, CreatureAssets};
use crate::save::{read_ron, screenshot_run, write_ron, SaveDir};

const FILE: &str = "creatures.ron";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Saved {
    pub creatures: Vec<SavedCreature>,
    /// Barrow id -> the day its wights last woke.
    pub barrows: HashMap<u64, f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedCreature {
    /// Its kind's name in creatures.ron.
    pub kind: String,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub health: f32,
    #[serde(default)]
    pub leash: Option<([f32; 3], f32)>,
}

/// Whether a creature of this temper is saved.
pub fn kept(temper: Temper) -> bool {
    !matches!(temper, Temper::Critter)
}

pub(super) fn load(mut commands: Commands, dir: Res<SaveDir>, kinds: Option<Res<Kinds>>, assets: Option<Res<CreatureAssets>>, mut barrows: ResMut<Barrows>) {
    let (Some(kinds), Some(assets)) = (kinds, assets) else { return };
    if screenshot_run() {
        return;
    }
    let Some(saved) = read_ron::<Saved>(&dir, FILE) else { return };
    barrows.woke = saved.barrows;
    for s in &saved.creatures {
        let Some(kind) = kinds.id(&s.kind) else {
            warn!("{FILE}: no creature kind `{}`", s.kind);
            continue;
        };
        let e = spawn_creature(&mut commands, &kinds, &assets, kind, Vec3::from_array(s.pos), s.yaw);
        let mut c = Creature::new(kind, s.health, s.yaw);
        c.fade = if kinds.get(kind).temper == Temper::Spirit { 0.0 } else { 1.0 };
        c.leash = s.leash.map(|(p, r)| (Vec3::from_array(p), r));
        commands.entity(e).insert(c);
    }
}

type Wild<'a> = (&'a Creature, &'a Transform);

fn store(dir: &SaveDir, kinds: &Kinds, barrows: &Barrows, q: &Query<Wild>) {
    let creatures = q
        .iter()
        .filter(|(c, _)| kept(kinds.get(c.kind).temper) && c.health > 0.0 && !c.leaving)
        .map(|(c, tf)| SavedCreature {
            kind: kinds.get(c.kind).name.clone(),
            pos: tf.translation.to_array(),
            yaw: c.yaw,
            health: c.health,
            leash: c.leash.map(|(p, r)| (p.to_array(), r)),
        })
        .collect();
    write_ron(dir, FILE, &Saved { creatures, barrows: barrows.woke.clone() });
}

pub(super) fn save_now_and_then(
    time: Res<Time<Real>>,
    mut since: Local<f32>,
    dir: Res<SaveDir>,
    kinds: Option<Res<Kinds>>,
    barrows: Res<Barrows>,
    q: Query<Wild>,
) {
    *since += time.delta_secs();
    let Some(kinds) = kinds else { return };
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    store(&dir, &kinds, &barrows, &q);
}

pub(super) fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, kinds: Option<Res<Kinds>>, barrows: Res<Barrows>, q: Query<Wild>) {
    if exit.read().next().is_some()
        && !screenshot_run()
        && let Some(kinds) = kinds
    {
        store(&dir, &kinds, &barrows, &q);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_creatures_round_trip() {
        let mut s = Saved::default();
        s.creatures.push(SavedCreature { kind: "fen_stag".into(), pos: [1.0, 2.0, 3.0], yaw: 0.5, health: 7.0, leash: Some(([0.0, 1.0, 0.0], 10.0)) });
        s.barrows.insert(9, 3.5);
        let text = ron::to_string(&s).unwrap();
        assert_eq!(ron::from_str::<Saved>(&text).unwrap(), s);
        // A file with only creatures still loads.
        assert!(ron::from_str::<Saved>("(creatures: [])").is_ok());
    }

    #[test]
    fn villagers_and_critters_are_not_saved() {
        assert!(!kept(Temper::Critter));
        assert!(kept(Temper::Hostile) && kept(Temper::Spirit) && kept(Temper::Skittish));
    }
}
