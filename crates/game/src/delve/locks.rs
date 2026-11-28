//! Lock-ins: walking into a fight room, an ambush or the boss arena drops stone
//! slabs into its doorways (a boom and dust) until every foe there is down,
//! wave by wave. Rooms sealed together in a section lock as one. The shut doors
//! glow faintly and a count of the foes left shows at the top of the screen.
//! Rest, treasure and puzzle rooms never lock (`rules::locks`). Rooms that
//! don't lock count as cleared once their foes are down.

use bevy::prelude::*;
use voxel::BlockId;
use worldgen::delve::Doorway;

use super::rules::{self, Lock, LockStep};
use super::{spawn_wave, tag_room, Below, DelveSet};
use crate::combat::{Foe, FoeBook, SpawnFoe};
use crate::player::Player;
use crate::sound::PlaySound;
use crate::survival::Toast;
use crate::world::Generator;

pub struct LocksPlugin;

impl Plugin for LocksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LockIn>()
            .add_message::<RoomCleared>()
            .add_systems(crate::menu::GameStartup, spawn_counter)
            .add_systems(crate::menu::GameUpdate, (lock_in, hold, glow, counter).chain().in_set(LocksSet).after(DelveSet));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocksSet;

/// Seconds after a wave is sent before an empty room counts as beaten (the
/// foes take a moment to appear).
const GRACE: f32 = 1.0;

/// The lock you are in, if any.
#[derive(Resource, Default)]
pub struct LockIn {
    pub lock: Option<Lock>,
    doors: Vec<Doorway>,
    /// Seconds since the last wave was sent.
    since: f32,
    /// Foes of the lock still standing (for the counter).
    pub left: usize,
    /// The floor this was on (run, floor).
    on: Option<(u32, u8)>,
    /// Rooms that never lock whose foes have been seen standing.
    seen: std::collections::HashSet<usize>,
}

/// A room (or every room of a section) was cleared: its reward is given (`boons`).
#[derive(Message, Debug, Clone, Copy)]
pub struct RoomCleared(pub usize);

/// Foes of `rooms` (any wave) still standing.
fn standing<'a>(foes: impl Iterator<Item = &'a Foe>, rooms: &[usize]) -> usize {
    foes.filter(|f| tag_room(f.tag).is_some_and(|(r, _)| rooms.contains(&r))).count()
}

#[allow(clippy::too_many_arguments)]
fn lock_in(
    mut below: ResMut<Below>,
    mut lock: ResMut<LockIn>,
    generator: Res<Generator>,
    book: Res<FoeBook>,
    player: Res<Player>,
    blocks: Res<crate::blocks::Blocks>,
    foes: Query<&Foe>,
    mut spawn: MessageWriter<SpawnFoe>,
    mut sound: MessageWriter<PlaySound>,
    mut toast: MessageWriter<Toast>,
    mut cleared: MessageWriter<RoomCleared>,
) {
    let here = below.run.map(|r| (r.number, r.floor));
    if lock.on != here {
        *lock = LockIn { on: here, ..default() };
    }
    let Some(floor) = below.floor.clone() else { return };
    // Rooms that never lock count as cleared once their foes are down.
    let open: Vec<usize> = below
        .spawned
        .iter()
        .map(|(r, _)| *r)
        .filter(|r| !below.cleared.contains(r) && !floor.rooms[*r].kind.locks())
        .collect();
    for r in open {
        // Only once its foes have stood (they take a frame or two to appear).
        if standing(foes.iter(), &[r]) > 0 {
            lock.seen.insert(r);
        } else if lock.seen.contains(&r) {
            below.cleared.insert(r);
            cleared.write(RoomCleared(r));
        }
    }
    if lock.lock.is_some() || !player.ready {
        return;
    }
    let p = player.pos.to_array();
    let Some(room) = floor.room_at(p) else { return };
    // A step past the doorway, so the slab doesn't drop on you.
    let inner = floor.rooms[room].inner.grow(-1);
    let at = p.map(|v| v.floor() as i32);
    if !inner.contains(at[0], at[1], at[2]) {
        return;
    }
    let has_foes = floor.spawns.iter().any(|s| s.room == room);
    if !rules::locks(floor.rooms[room].kind, has_foes, below.cleared.contains(&room)) {
        return;
    }
    let group: Vec<usize> = floor.sealed_with(room).into_iter().filter(|r| !below.cleared.contains(r)).collect();
    let waves = group.iter().map(|&r| floor.rooms[r].waves).max().unwrap_or(1);
    for &r in &group {
        spawn_wave(&mut below, &book, &generator, r, 0, &mut spawn);
    }
    // The boss already fell (a cleared dungeon's last floor): nothing to lock in with.
    if group.iter().all(|&r| floor.spawns.iter().filter(|s| s.room == r).all(|s| s.kind == worldgen::SpawnKind::Boss)) && below.run.is_some_and(|r| r.boss_down) {
        return;
    }
    let Some(slab) = blocks.registry.id("dungeon_slab") else { return };
    let doors = floor.lock_doors(room);
    for d in &doors {
        below.edit_box(&d.at, slab);
    }
    lock.lock = Some(Lock::new(group, waves));
    lock.doors = doors;
    lock.since = 0.0;
    sound.write(PlaySound::new("hit").speed(0.4).gain(1.3));
    toast.write(Toast(if waves > 1 { format!("The doors slam shut. {waves} waves") } else { "The doors slam shut".into() }));
}

#[allow(clippy::too_many_arguments)]
fn hold(
    time: Res<Time>,
    mut below: ResMut<Below>,
    mut lock: ResMut<LockIn>,
    generator: Res<Generator>,
    book: Res<FoeBook>,
    foes: Query<&Foe>,
    mut spawn: MessageWriter<SpawnFoe>,
    mut sound: MessageWriter<PlaySound>,
    mut toast: MessageWriter<Toast>,
    mut cleared: MessageWriter<RoomCleared>,
) {
    lock.since += time.delta_secs();
    let Some(mut l) = lock.lock.clone() else { return };
    let left = standing(foes.iter(), &l.rooms);
    lock.left = left;
    if lock.since < GRACE {
        return;
    }
    match l.step(left) {
        LockStep::Hold => {}
        LockStep::Wave(w) => {
            for &r in &l.rooms {
                spawn_wave(&mut below, &book, &generator, r, w, &mut spawn);
            }
            lock.since = 0.0;
            sound.write(PlaySound::new("clash").speed(0.6));
            toast.write(Toast(format!("Wave {} of {}", w + 1, l.waves)));
        }
        LockStep::Open => {
            for d in &lock.doors {
                below.edit_box(&d.at, BlockId::AIR);
            }
            for &r in &l.rooms {
                below.cleared.insert(r);
                cleared.write(RoomCleared(r));
            }
            sound.write(PlaySound::new("craft_pop").speed(0.6).gain(1.1));
            toast.write(Toast("The slabs grind back up".into()));
            lock.lock = None;
            lock.doors.clear();
            return;
        }
    }
    lock.lock = Some(l);
}

/// A faint light on each shut door.
#[derive(Component)]
struct DoorGlow;

fn glow(mut commands: Commands, lock: Res<LockIn>, glows: Query<Entity, With<DoorGlow>>) {
    if !lock.is_changed() {
        return;
    }
    for e in &glows {
        commands.entity(e).despawn();
    }
    if lock.lock.is_none() {
        return;
    }
    for d in &lock.doors {
        let a = &d.at;
        let mid = Vec3::new((a.min[0] + a.max[0] + 1) as f32 / 2.0, a.min[1] as f32 + 1.5, (a.min[2] + a.max[2] + 1) as f32 / 2.0);
        // On the room's side of the slab.
        let inward = -Vec3::new(d.out.0 as f32, 0.0, d.out.1 as f32) * 0.9;
        commands.spawn((
            DoorGlow,
            PointLight { color: Color::srgb(1.0, 0.45, 0.25), intensity: 18_000.0, range: 4.0, shadow_maps_enabled: false, ..default() },
            Transform::from_translation(mid + inward),
        ));
    }
}

/// The count of foes left, at the top of the screen.
#[derive(Component)]
struct Counter;

fn spawn_counter(mut commands: Commands) {
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, top: Val::Px(118.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
            Visibility::Hidden,
            Counter,
        ))
        .with_child((Text::new(""), TextFont { font_size: FontSize::Px(crate::theme::sizes::HEADING), ..default() }, TextColor(Color::srgb(1.0, 0.75, 0.6))));
}

fn counter(lock: Res<LockIn>, mut ui: Query<(&mut Visibility, &Children), With<Counter>>, mut texts: Query<&mut Text>) {
    for (mut vis, kids) in &mut ui {
        let want = if lock.lock.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        vis.set_if_neq(want);
        let Some(l) = &lock.lock else { continue };
        let line = if l.waves > 1 { format!("Foes left: {}   Wave {} of {}", lock.left, l.wave + 1, l.waves) } else { format!("Foes left: {}", lock.left) };
        for k in kids.iter() {
            if let Ok(mut t) = texts.get_mut(k)
                && t.0 != line
            {
                t.0 = line.clone();
            }
        }
    }
}
