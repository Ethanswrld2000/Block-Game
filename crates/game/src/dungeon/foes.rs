//! Foes in the dungeons: each run fills the rooms from the dungeon's spawn
//! points as the spirit goes. Walking into a room fills that room and the rooms
//! its corridors lead to (so foes are already standing there when you arrive);
//! the run ending clears every foe of that dungeon, so the next run starts fresh.
//!
//! Which foe stands where: guards and the warden's hall boss are hollows, packs
//! mix hollows and shades, and the boss is a shade (kinds in `assets/data/foes.ron`).

use std::collections::HashSet;

use bevy::prelude::*;
use worldgen::{Dungeon, SpawnKind};

use super::{EnteredRoom, LeftDungeon};
use crate::combat::{Foe, FoeBook, SpawnFoe};

pub struct DungeonFoesPlugin;

impl Plugin for DungeonFoesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Run>().add_systems(crate::menu::GameUpdate, (fill_rooms, clear_run).chain().after(super::spirit::SpiritSet));
    }
}

/// The run in progress: which dungeon, and which rooms already have their foes.
#[derive(Resource, Default)]
struct Run {
    dungeon: Option<u64>,
    filled: HashSet<usize>,
}

/// The foe kind (by name in foes.ron) for a spawn point.
pub fn kind_for(kind: SpawnKind, pos: [i32; 3]) -> &'static str {
    match kind {
        SpawnKind::Guard => "hollow",
        SpawnKind::Boss => "shade",
        SpawnKind::Pack => {
            if (pos[0] ^ pos[2]).rem_euclid(3) == 0 {
                "shade"
            } else {
                "hollow"
            }
        }
    }
}

/// The rooms to fill when a spirit walks into `room`: it and its neighbours.
pub fn rooms_to_fill(d: &Dungeon, room: usize) -> Vec<usize> {
    let mut out = vec![room];
    for &(a, b) in &d.links {
        if a == room {
            out.push(b);
        } else if b == room {
            out.push(a);
        }
    }
    out
}

fn fill_rooms(mut entered: MessageReader<EnteredRoom>, book: Res<FoeBook>, mut run: ResMut<Run>, mut spawn: MessageWriter<SpawnFoe>) {
    for e in entered.read() {
        let d = &e.dungeon;
        if run.dungeon != Some(d.id) {
            run.dungeon = Some(d.id);
            run.filled.clear();
        }
        for room in rooms_to_fill(d, e.room) {
            if !run.filled.insert(room) {
                continue;
            }
            for s in d.spawns.iter().filter(|s| s.room == room) {
                let Some(kind) = book.kinds.iter().position(|k| k.name == kind_for(s.kind, s.pos)) else { continue };
                spawn.write(SpawnFoe::new(kind, Vec3::new(s.pos[0] as f32 + 0.5, s.pos[1] as f32, s.pos[2] as f32 + 0.5)));
            }
        }
    }
}

/// The run is over (walked out or fell): its foes go.
fn clear_run(mut commands: Commands, mut left: MessageReader<LeftDungeon>, mut run: ResMut<Run>, foes: Query<(Entity, &Foe)>) {
    for l in left.read() {
        if run.dungeon == Some(l.dungeon.id) {
            run.dungeon = None;
            run.filled.clear();
        }
        for (e, foe) in &foes {
            if l.dungeon.inside(foe.home.to_array()) {
                commands.entity(e).despawn();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spawn_point_names_a_real_foe() {
        let kinds = crate::combat::data::load(include_str!("../../../../assets/data/foes.ron")).expect("foes.ron");
        for kind in [SpawnKind::Guard, SpawnKind::Pack, SpawnKind::Boss] {
            for x in 0..6 {
                let name = kind_for(kind, [x, 0, 1]);
                assert!(kinds.iter().any(|k| k.name == name), "no foe `{name}`");
            }
        }
        // Packs mix both kinds.
        let mixed: HashSet<_> = (0..12).map(|x| kind_for(SpawnKind::Pack, [x, 0, 0])).collect();
        assert_eq!(mixed.len(), 2);
    }
}
