//! Warded dungeons in the game: the veil across each gate, the guide who
//! stands by it, and the rule that only a spirit passes (see `spirit`).
//!
//! The layout itself is pure worldgen (`worldgen::dungeons`); this plugin keeps
//! a list of the dungeons near you, hangs a shimmering veil in each doorway,
//! puts a guide NPC (the `warden` kind in npcs.ron) beside each path, and
//! turns back anyone who isn't a spirit. Blocks inside every named place are
//! under the spirit ward (`WorldGen::warded`, checked in `interact`).
//!
//! Hooks for enemies (built elsewhere): every `Dungeon` lists `spawns` (where,
//! what kind, which room); `EnteredRoom` fires the first time a spirit walks
//! into a room on a run; `LeftDungeon` fires when the run ends (walked out or fell).

pub mod foes;
pub mod spirit;

use std::collections::HashMap;
use std::sync::Arc;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use worldgen::Dungeon;

use crate::npc::{body::SpawnNpc, Npc, NpcBook, Order};
use crate::player::Player;
use crate::world::{Generator, World};

pub struct DungeonPlugin;

impl Plugin for DungeonPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Nearby>()
            .add_message::<EnteredRoom>()
            .add_message::<LeftDungeon>()
            .add_systems(crate::menu::GamePostStartup, start_at_dungeon.after(crate::world::setup_generator))
            .add_systems(
                crate::menu::GameUpdate,
                (refresh_nearby, place_guides, place_veils, shimmer_veils).chain().after(crate::player::PlayerSet).before(crate::npc::NpcSet),
            )
            .add_plugins((spirit::SpiritPlugin, foes::DungeonFoesPlugin));
    }
}

/// How far around you dungeons are looked up, blocks.
const LOOK: i32 = 180;
/// Guides and veils appear within this distance of a gate, and go beyond `GONE`.
const NEAR: f32 = 120.0;
const GONE: f32 = 160.0;

/// A spirit walked into a room for the first time this run (spawn its enemies).
#[derive(Message, Debug, Clone)]
pub struct EnteredRoom {
    pub dungeon: Arc<Dungeon>,
    pub room: usize,
}

/// A run ended: the spirit walked back out (`fell: false`) or fell inside.
#[derive(Message, Debug, Clone)]
pub struct LeftDungeon {
    pub dungeon: Arc<Dungeon>,
    #[allow(dead_code)] // nothing tells the two apart yet
    pub fell: bool,
}

/// Dungeons whose warded area is near you.
#[derive(Resource, Default)]
pub struct Nearby {
    pub list: Vec<Arc<Dungeon>>,
    tick: f32,
}

impl Nearby {
    /// The dungeon whose gate is closest to a point.
    pub fn nearest(&self, at: Vec3) -> Option<Arc<Dungeon>> {
        self.list.iter().min_by(|a, b| gate_dist(a, at).total_cmp(&gate_dist(b, at))).cloned()
    }
}

/// Horizontal distance from the path in front of a dungeon's door.
pub fn gate_dist(d: &Dungeon, at: Vec3) -> f32 {
    Vec2::new(d.outside[0] - at.x, d.outside[2] - at.z).length()
}

fn refresh_nearby(time: Res<Time>, generator: Option<Res<Generator>>, player: Res<Player>, mut nearby: ResMut<Nearby>) {
    let Some(generator) = generator else { return };
    nearby.tick -= time.delta_secs();
    if nearby.tick > 0.0 {
        return;
    }
    nearby.tick = 1.0;
    let p = player.pos.floor().as_ivec3();
    nearby.list = generator.0.dungeons_in(p.x - LOOK, p.z - LOOK, p.x + LOOK, p.z + LOOK);
}

/// `BLOCKGAME_DUNGEON=1` starts you on the path in front of the home dungeon's
/// gate, facing the door (screenshots and testing).
fn start_at_dungeon(generator: Option<Res<Generator>>, mut player: ResMut<Player>, mut cam: Query<(&mut Transform, &mut crate::camera::FlyCamera)>) {
    let Some(generator) = generator else { return };
    let Some(d) = generator.0.home_dungeon() else { return };
    info!("home dungeon {} at {:?}, {} rooms, door path at {:?}", d.name, d.gate, d.rooms.len(), d.outside);
    if std::env::var_os("BLOCKGAME_DUNGEON").is_none() {
        return;
    }
    player.pos = Vec3::from_array(d.outside) + Vec3::new(d.veil.out.0 as f32, 0.0, d.veil.out.1 as f32) * 2.0;
    for (mut tf, mut fly) in &mut cam {
        tf.translation = player.pos + Vec3::Y * crate::player::EYE;
        fly.yaw = crate::npc::yaw_of(Vec3::new(-d.veil.out.0 as f32, 0.0, -d.veil.out.1 as f32));
        fly.pitch = 0.0;
        tf.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0);
    }
}

/// A guide stands beside the path of every dungeon near you.
#[allow(clippy::too_many_arguments)]
fn place_guides(
    mut commands: Commands,
    time: Res<Time>,
    nearby: Res<Nearby>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<crate::blocks::Blocks>,
    book: Res<NpcBook>,
    npcs: Query<(Entity, &Npc)>,
    mut spawn: MessageWriter<SpawnNpc>,
    mut pending: Local<HashMap<u64, f32>>,
) {
    let Some(kind) = book.kinds.iter().position(|k| k.dialogue == "warden") else { return };
    let now = time.elapsed_secs();
    pending.retain(|_, t| now - *t < 3.0);
    // Guides of dungeons you left behind go.
    for (e, npc) in &npcs {
        if npc.kind == kind
            && let Some(id) = npc.post
            && nearby.list.iter().find(|d| d.id == id).is_none_or(|d| gate_dist(d, player.pos) > GONE)
        {
            commands.entity(e).despawn();
        }
    }
    for d in &nearby.list {
        if gate_dist(d, player.pos) > NEAR || pending.contains_key(&d.id) || npcs.iter().any(|(_, n)| n.kind == kind && n.post == Some(d.id)) {
            continue;
        }
        let want = Vec3::from_array(d.guide);
        // Wait for the ground there to load.
        let Some(at) = crate::npc::ground_near(&world, &blocks, want + Vec3::Y * 2.0) else { continue };
        let face = Vec3::from_array(d.outside) - at;
        let mut npc = Npc::new(kind, crate::npc::yaw_of(face));
        npc.order = Order::Stay;
        npc.post = Some(d.id);
        spawn.write(SpawnNpc { npc, at });
        pending.insert(d.id, now);
    }
}

/// The veil hanging in a dungeon's doorway.
#[derive(Component)]
struct VeilOf(u64);

/// One shared material for all veils (it shimmers in `shimmer_veils`).
#[derive(Resource)]
struct VeilLook {
    mesh: Handle<Mesh>,
    sheets: [Handle<StandardMaterial>; 2],
}

const VEIL_COLOR: (f32, f32, f32) = (0.45, 0.72, 1.0);

#[allow(clippy::too_many_arguments)]
fn place_veils(
    mut commands: Commands,
    nearby: Res<Nearby>,
    player: Res<Player>,
    look: Option<Res<VeilLook>>,
    veils: Query<(Entity, &VeilOf)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(look) = look else {
        let sheet = |m: &mut Assets<StandardMaterial>| {
            m.add(StandardMaterial {
                base_color: Color::srgba(VEIL_COLOR.0, VEIL_COLOR.1, VEIL_COLOR.2, 0.3),
                emissive: LinearRgba::rgb(0.25, 0.55, 1.3),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                double_sided: true,
                cull_mode: None,
                ..default()
            })
        };
        let sheets = [sheet(&mut materials), sheet(&mut materials)];
        commands.insert_resource(VeilLook { mesh: meshes.add(Rectangle::new(1.0, 1.0)), sheets });
        return;
    };
    for (e, v) in &veils {
        if nearby.list.iter().find(|d| d.id == v.0).is_none_or(|d| gate_dist(d, player.pos) > GONE) {
            commands.entity(e).despawn();
        }
    }
    for d in &nearby.list {
        if gate_dist(d, player.pos) > NEAR || veils.iter().any(|(_, v)| v.0 == d.id) {
            continue;
        }
        let out = Vec3::new(d.veil.out.0 as f32, 0.0, d.veil.out.1 as f32);
        let turn = Quat::from_rotation_arc(Vec3::Z, out);
        commands
            .spawn((VeilOf(d.id), Transform::from_translation(Vec3::from_array(d.veil.center)).with_rotation(turn), Visibility::default()))
            .with_children(|c| {
                // Two sheets a hand apart, shimmering out of step.
                for (i, sheet) in look.sheets.iter().enumerate() {
                    c.spawn((
                        Mesh3d(look.mesh.clone()),
                        MeshMaterial3d(sheet.clone()),
                        Transform::from_xyz(0.0, 0.0, -0.1 - 0.25 * i as f32).with_scale(Vec3::new(d.veil.width, d.veil.height, 1.0)),
                        NotShadowCaster,
                    ));
                }
                // The veil's own blue light on the doorway round it.
                c.spawn((
                    PointLight { color: Color::srgb(VEIL_COLOR.0, VEIL_COLOR.1, VEIL_COLOR.2), intensity: 60_000.0, range: 9.0, shadow_maps_enabled: false, ..default() },
                    Transform::from_xyz(0.0, 0.0, 0.6),
                ));
            });
    }
}

/// Veils ripple in brightness and throw the odd sparkle; they thin for a spirit.
fn shimmer_veils(
    time: Res<Time>,
    spirit: Res<spirit::Spirit>,
    look: Option<Res<VeilLook>>,
    veils: Query<&Transform, With<VeilOf>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bursts: MessageWriter<crate::station::particles::Burst>,
    mut next_sparkle: Local<f32>,
) {
    let Some(look) = look else { return };
    let t = time.elapsed_secs();
    let thin = if spirit.is_spirit() { 0.45 } else { 1.0 };
    for (i, h) in look.sheets.iter().enumerate() {
        let phase = t * (1.7 + i as f32 * 0.6) + i as f32 * 2.1;
        let wave = 0.5 + 0.5 * phase.sin();
        if let Some(mut m) = materials.get_mut(h) {
            m.base_color = Color::srgba(VEIL_COLOR.0, VEIL_COLOR.1, VEIL_COLOR.2, (0.18 + 0.16 * wave) * thin);
            m.emissive = LinearRgba::rgb(0.2, 0.45, 1.0) * (0.7 + 0.8 * wave) * thin;
        }
    }
    if t >= *next_sparkle {
        *next_sparkle = t + 0.35;
        for tf in &veils {
            let r = tf.rotation * Vec3::new((t * 12.9).sin() * 1.3, (t * 7.7).cos() * 1.3, 0.0);
            bursts.write(crate::station::particles::Burst::new(tf.translation + r, crate::station::particles::BurstKind::Sparkle, 2));
        }
    }
}
