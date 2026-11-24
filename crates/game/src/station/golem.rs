//! The golem. Once the golem urn's heart is put in, the stone body on the
//! frame wakes and steps down: it follows you, or stays where you tell it
//! (right click it). With working hands it carries a pack (crouch and right
//! click it) and fights off creatures that come at you or the camp.
//!
//! It is a station that walks: its entity carries a `Station` of kind `Golem`
//! so aiming and right clicks work like any other station. Saved to `golem.ron`.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::models;
use super::{Station, StationKind, UrnBook, Urns};
use crate::blocks::Blocks;
use crate::creatures::data::{Kinds, Temper};
use crate::creatures::Creature;
use crate::interact::Strike;
use crate::player::{MoveMode, Player};
use crate::save::SaveDir;
use crate::world::World;

/// Walking speed, blocks per second.
pub const SPEED: f32 = 4.2;
/// It keeps about this far from you when following.
pub const KEEP: f32 = 2.6;
/// Further than this (or you flew off) and it catches up at once.
pub const CATCH_UP: f32 = 32.0;
/// It goes for creatures this near it (with working hands).
pub const GUARD: f32 = 7.0;
/// Reach of its fists, and the time between blows.
pub const REACH: f32 = 1.8;
pub const BLOW_SECONDS: f32 = 1.1;
pub const DAMAGE: f32 = 9.0;

/// Where the golem is, and whether it follows you.
#[derive(Resource, Debug, Clone, Default, Serialize, Deserialize)]
pub struct Golem {
    pub pos: Option<(f32, f32, f32)>,
    #[serde(default = "yes")]
    pub follow: bool,
    #[serde(skip)]
    yaw: f32,
    #[serde(skip)]
    walked: f32,
    #[serde(skip)]
    cooldown: f32,
}

fn yes() -> bool {
    true
}

/// Where it steps next: towards a goal, stopping `keep` short of it.
pub fn step(from: Vec3, goal: Vec3, keep: f32, dt: f32) -> Vec3 {
    let mut flat = goal - from;
    flat.y = 0.0;
    let d = flat.length();
    if d <= keep {
        return from;
    }
    from + flat / d * (SPEED * dt).min(d - keep)
}

#[derive(Component)]
pub(super) struct GolemBody;

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() || std::env::var_os("BLOCKGAME_BENCH").is_some()
}

pub(super) fn load_golem(mut commands: Commands, dir: Res<SaveDir>) {
    let g = if screenshot_run() { None } else { std::fs::read_to_string(dir.file("golem.ron")).ok().and_then(|t| ron::from_str::<Golem>(&t).ok()) };
    commands.insert_resource(g.unwrap_or(Golem { follow: true, ..default() }));
}

pub(super) fn save_golem_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, golem: Res<Golem>) {
    if exit.read().next().is_some() && !screenshot_run() {
        match ron::ser::to_string_pretty(&*golem, ron::ser::PrettyConfig::default()) {
            Ok(text) => {
                if let Err(e) = std::fs::write(dir.file("golem.ron"), text) {
                    warn!("saving golem.ron: {e}");
                }
            }
            Err(e) => warn!("golem.ron: {e}"),
        }
    }
}

/// Wake, walk, follow, fight.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn golem_walk(
    mut commands: Commands,
    time: Res<Time>,
    urns: Res<Urns>,
    book: Option<Res<UrnBook>>,
    (player, mode, world, blocks): (Res<Player>, Res<MoveMode>, Res<World>, Res<Blocks>),
    mut golem: ResMut<Golem>,
    frames: Query<&Station, Without<GolemBody>>,
    mut body: Query<(Entity, &mut Station, &mut Transform), With<GolemBody>>,
    creatures: Query<(Entity, &Transform, &Creature), Without<GolemBody>>,
    kinds: Option<Res<Kinds>>,
    mut strikes: MessageWriter<Strike>,
    (mut meshes, mut materials, art): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>, Option<Res<super::skin::StationArt>>),
) {
    let Some(book) = book else { return };
    let awake = urns.has("golem_heart");
    if !awake {
        for (e, ..) in &body {
            commands.entity(e).despawn();
        }
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let g = golem.bypass_change_detection();
    // It wakes on its frame.
    let mut pos = match g.pos {
        Some(p) => Vec3::new(p.0, p.1, p.2),
        None => {
            let Some(f) = frames.iter().find(|s| s.kind == StationKind::GolemFrame) else { return };
            f.pos + Vec3::new(0.0, 0.0, 1.0)
        }
    };
    if g.follow {
        let far = pos.distance(player.pos) > CATCH_UP;
        if far && *mode == MoveMode::Walk && player.on_ground {
            pos = player.pos - Vec3::new(KEEP, 0.0, 0.0);
        } else if !far {
            let next = step(pos, player.pos, KEEP, dt);
            if next != pos {
                let d = next - pos;
                g.yaw = (-d.x).atan2(-d.z);
                g.walked += d.length();
            }
            pos = next;
        }
    }
    pos = super::ground(&world, &blocks, pos);
    // Working hands: go for what comes at you or the camp.
    g.cooldown = (g.cooldown - dt).max(0.0);
    if urns.has("golem_hands")
        && let Some(kinds) = &kinds
    {
        let foe = creatures
            .iter()
            .filter(|(_, _, c)| matches!(kinds.get(c.kind).temper, Temper::Hostile | Temper::Spirit) || c.angry > 0.0)
            .map(|(e, t, _)| (e, t.translation, t.translation.distance(pos)))
            .filter(|(.., d)| *d < GUARD)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((e, at, d)) = foe {
            if d > REACH {
                pos = super::ground(&world, &blocks, step(pos, at, REACH * 0.8, dt));
            } else if g.cooldown <= 0.0 {
                g.cooldown = BLOW_SECONDS;
                let dir = (at - pos).with_y(0.0).normalize_or(Vec3::X);
                strikes.write(Strike { entity: e, damage: DAMAGE, dir, yours: false });
            }
            let d = at - pos;
            g.yaw = (-d.x).atan2(-d.z);
        }
    }
    g.pos = Some((pos.x, pos.y, pos.z));
    let station = Station { kind: StationKind::Golem, pos, turns: 0, urn: book.0.urns.iter().position(|u| u.name == "golem").unwrap_or(0) };
    let bob = (g.walked * 3.0).sin().abs() * 0.05;
    let tf = Transform::from_translation(pos + Vec3::Y * bob).with_rotation(Quat::from_rotation_y(g.yaw));
    if let Ok((_, mut st, mut t)) = body.single_mut() {
        *st = station;
        *t = tf;
        return;
    }
    // Build its body from the station model's parts.
    let color = book.0.urns[station.urn].color;
    let hands = urns.has("golem_hands");
    commands.spawn((GolemBody, station, tf, Visibility::default())).with_children(|c| {
        let parts = models::parts(StationKind::Golem, &|s| s == "golem_hands" && hands, 0);
        if let Some((at, color, lumens)) = models::glow_light(&parts, color) {
            c.spawn((PointLight { color, intensity: lumens, range: models::GLOW_RANGE, shadow_maps_enabled: false, ..default() }, Transform::from_translation(at)));
        }
        for p in parts {
            let mat = materials.add(super::skin::material(p.mat, color, art.as_deref()));
            let mesh = meshes.add(super::skin::box_mesh(p.size));
            c.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::from_translation(p.at), NotShadowCaster));
        }
    });
}

/// Hands put on after it woke: rebuild its body.
pub(super) fn golem_rebuild(
    urns: Res<Urns>,
    art: Option<Res<super::skin::StationArt>>,
    mut commands: Commands,
    body: Query<Entity, With<GolemBody>>,
    mut had: Local<(bool, bool)>,
) {
    // Hands put on, or the station textures arrived.
    let now = (urns.has("golem_hands"), art.is_some_and(|a| a.ready()));
    if now != *had {
        *had = now;
        for e in &body {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_walks_up_and_stops_short() {
        let mut p = Vec3::ZERO;
        let goal = Vec3::new(10.0, 3.0, 0.0);
        for _ in 0..200 {
            p = step(p, goal, KEEP, 0.05);
        }
        assert!((p.x - (10.0 - KEEP)).abs() < 1e-3, "{p}");
        assert_eq!(p.y, 0.0, "the ground decides its height");
        // One step is never faster than it walks.
        assert!(step(Vec3::ZERO, goal, KEEP, 0.1).length() <= SPEED * 0.1 + 1e-4);
    }
}
