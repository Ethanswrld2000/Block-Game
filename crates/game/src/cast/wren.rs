//! Wren Calloway's visits: on even days (`here`) she lands a small flying
//! machine by the vehicle urn, steps out and trades (`services`, stock from
//! `trades.ron`); the next day she climbs back in and flies off. Neither she
//! nor the machine is saved: she lands again on the next even day.
//! Not while a Wren you recruited lives in the world already.

use bevy::prelude::*;

use crate::clock::WorldClock;
use crate::npc::{body::SpawnNpc, ground_near, yaw_of, Npc, NpcBook, Order};
use crate::player::Player;
use crate::station::Urns;
use crate::world::World;

pub struct WrenPlugin;

impl Plugin for WrenPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::menu::GameUpdate, (visit, fly).chain().before(crate::npc::NpcSet).after(crate::player::PlayerSet));
    }
}

/// `Npc.post` of the visiting Wren (not saved).
pub const WREN_POST: u64 = u64::MAX - 2;
/// She comes only while you are this close to the urn...
const NEAR: f32 = 128.0;
/// ...and goes (with no show) when you are further than this.
const GONE: f32 = 200.0;
/// Seconds to come down, and to climb away.
const LANDING: f32 = 6.0;
const HEIGHT: f32 = 30.0;

/// Wren is visiting on this day (pure).
pub fn here(day: u32) -> bool {
    day.is_multiple_of(2)
}

#[derive(Component)]
pub struct Machine {
    /// Where it stands on the ground.
    ground: Vec3,
    state: Flight,
    /// Wren has stepped out.
    out: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Flight {
    Landing(f32),
    Parked,
    Leaving(f32),
}

/// The machine's height above its spot while landing (`t` 0..1): eases in (pure).
pub fn landing_height(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    HEIGHT * (1.0 - t) * (1.0 - t)
}

#[allow(clippy::too_many_arguments)]
fn visit(
    mut commands: Commands,
    clock: Res<WorldClock>,
    urns: Res<Urns>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<crate::blocks::Blocks>,
    book: Res<NpcBook>,
    npcs: Query<(Entity, &Npc)>,
    mut machines: Query<(Entity, &mut Machine)>,
    (mut meshes, mut mats): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut spawn: MessageWriter<SpawnNpc>,
) {
    let Some(kind) = book.kinds.iter().position(|k| k.dialogue == "wren") else { return };
    let urn = urns.placed.get("vehicle").map(|p| (Vec3::new(p.pos.0, p.pos.1, p.pos.2), p));
    let dist = urn.map_or(f32::MAX, |(u, _)| Vec2::new(u.x - player.pos.x, u.z - player.pos.z).length());
    let visitor: Vec<Entity> = npcs.iter().filter(|(_, n)| n.post == Some(WREN_POST)).map(|(e, _)| e).collect();
    // Out of sight, or the urn picked up: gone without a show.
    if dist > GONE {
        for e in visitor.iter().copied().chain(machines.iter().map(|(e, _)| e)) {
            commands.entity(e).despawn();
        }
        return;
    }
    if !here(clock.day) {
        // Next day: back in and away.
        for e in visitor {
            commands.entity(e).despawn();
        }
        for (_, mut m) in &mut machines {
            if m.state == Flight::Parked {
                m.state = Flight::Leaving(0.0);
            }
        }
        return;
    }
    let settled = npcs.iter().any(|(_, n)| n.kind == kind && n.post.is_none());
    let Some((urn_at, placed)) = urn else { return };
    if !machines.is_empty() || settled || dist > NEAR || !player.ready {
        // Landed: Wren steps out beside it.
        for (_, mut m) in &mut machines {
            if m.state == Flight::Parked && !m.out && visitor.is_empty() {
                let side = if placed.along_x { Vec3::X } else { Vec3::Z };
                let Some(at) = ground_near(&world, &blocks, m.ground + side * 2.2 + Vec3::Y * 2.0) else { continue };
                let mut npc = Npc::new(kind, yaw_of(player.pos - at));
                npc.order = Order::Stay;
                npc.post = Some(WREN_POST);
                spawn.write(SpawnNpc { npc, at });
                m.out = true;
            }
        }
        return;
    }
    // In front of the urn, where you stood to set it down.
    let front = if placed.along_x { Vec3::Z * placed.back } else { Vec3::X * placed.back };
    let Some(ground) = ground_near(&world, &blocks, urn_at - front * 5.0 + Vec3::Y * 2.0) else { return };
    let wood = mats.add(StandardMaterial { base_color: Color::srgb(0.45, 0.3, 0.18), perceptual_roughness: 0.9, ..default() });
    let cloth = mats.add(StandardMaterial { base_color: Color::srgb(0.85, 0.78, 0.6), perceptual_roughness: 0.95, ..default() });
    let brass = mats.add(StandardMaterial { base_color: Color::srgb(0.78, 0.6, 0.25), metallic: 0.6, perceptual_roughness: 0.4, ..default() });
    let mut part = |size: Vec3, at: Vec3| (Mesh3d(meshes.add(Cuboid::from_size(size))), Transform::from_translation(at));
    let (hull, gondola, envelope, prop, rope) =
        (part(Vec3::new(1.4, 0.5, 2.2), Vec3::Y * 0.25), part(Vec3::new(1.0, 0.5, 1.2), Vec3::Y * 0.75), part(Vec3::new(1.6, 1.1, 2.8), Vec3::Y * 2.6), part(Vec3::new(1.2, 0.12, 0.08), Vec3::new(0.0, 0.5, 1.2)), part(Vec3::new(0.06, 1.6, 0.06), Vec3::Y * 1.3));
    commands
        .spawn((
            Machine { ground, state: Flight::Landing(0.0), out: false },
            Transform::from_translation(ground + Vec3::Y * HEIGHT).with_rotation(Quat::from_rotation_y(yaw_of(front))),
            Visibility::Inherited,
        ))
        .with_children(|m| {
            m.spawn((hull, MeshMaterial3d(wood.clone())));
            m.spawn((gondola, MeshMaterial3d(wood)));
            m.spawn((envelope, MeshMaterial3d(cloth)));
            m.spawn((prop, MeshMaterial3d(brass.clone()), Propeller));
            for (x, z) in [(-0.6, -0.9), (0.6, -0.9), (-0.6, 0.9), (0.6, 0.9)] {
                let mut r = rope.clone();
                r.1.translation += Vec3::new(x, 0.0, z);
                m.spawn((r, MeshMaterial3d(brass.clone())));
            }
        });
}

#[derive(Component)]
struct Propeller;

/// Comes down, sits, climbs away; the propeller turns while it flies.
fn fly(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Machine, &mut Transform, &Children)>, mut props: Query<&mut Transform, (With<Propeller>, Without<Machine>)>) {
    let dt = time.delta_secs();
    for (e, mut m, mut tf, kids) in &mut q {
        let (y, spin) = match m.state {
            Flight::Landing(t) => {
                let t = t + dt / LANDING;
                m.state = if t >= 1.0 { Flight::Parked } else { Flight::Landing(t) };
                (landing_height(t), true)
            }
            Flight::Parked => (0.0, false),
            Flight::Leaving(t) => {
                let t = t + dt / LANDING;
                if t >= 1.0 {
                    commands.entity(e).despawn();
                    continue;
                }
                m.state = Flight::Leaving(t);
                (HEIGHT * t * t * 1.5, true)
            }
        };
        tf.translation = m.ground + Vec3::Y * y;
        if spin {
            for k in kids.iter() {
                if let Ok(mut p) = props.get_mut(k) {
                    p.rotate_z(dt * 25.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_other_day() {
        assert!(here(0) && here(2) && here(10));
        assert!(!here(1) && !here(3));
    }

    #[test]
    fn lands_softly() {
        assert_eq!(landing_height(0.0), HEIGHT);
        assert_eq!(landing_height(1.0), 0.0);
        assert!(landing_height(0.9) < 1.0);
    }
}
