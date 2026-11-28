//! Dungeon entrances in the game: the veil across each door and the rule that
//! only a spirit passes (see `spirit`). The floors below are `delve`.
//!
//! Entrances are pure worldgen (`worldgen::dungeons`); this plugin keeps a list
//! of the dungeons near you and hangs a shimmering veil in each doorway.
//! Brindlespheimer stands at the entrance nearest you (`cast`). Blocks of every
//! entrance are under the spirit ward (`WorldGen::warded`, checked in `interact`).

pub mod spirit;

use std::sync::Arc;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use worldgen::Dungeon;

use crate::player::Player;
use crate::world::Generator;

pub struct DungeonPlugin;

impl Plugin for DungeonPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Nearby>()
            .add_message::<LeftDungeon>()
            .add_systems(
                crate::menu::GameUpdate,
                (refresh_nearby, place_veils, shimmer_veils)
                    .chain()
                    .after(crate::player::PlayerSet)
                    .before(crate::npc::NpcSet),
            )
            .add_plugins(spirit::SpiritPlugin);
    }
}

/// How far around you dungeons are looked up, blocks.
const LOOK: i32 = 400;
/// Veils appear within this distance of a door, and go beyond `GONE`.
const NEAR: f32 = 120.0;
const GONE: f32 = 160.0;

/// The spirit form ended: walked back out (`fell: false`) or fell below.
#[derive(Message, Debug, Clone)]
pub struct LeftDungeon {
    #[allow(dead_code)]
    pub dungeon: Arc<Dungeon>,
    pub fell: bool,
}

/// Dungeon entrances near you.
#[derive(Resource, Default)]
pub struct Nearby {
    pub list: Vec<Arc<Dungeon>>,
    tick: f32,
}

impl Nearby {
    /// The dungeon whose door is closest to a point.
    pub fn nearest(&self, at: Vec3) -> Option<Arc<Dungeon>> {
        self.list
            .iter()
            .min_by(|a, b| gate_dist(a, at).total_cmp(&gate_dist(b, at)))
            .cloned()
    }
}

/// Horizontal distance from the path in front of a dungeon's door.
pub fn gate_dist(d: &Dungeon, at: Vec3) -> f32 {
    Vec2::new(d.outside[0] - at.x, d.outside[2] - at.z).length()
}

fn refresh_nearby(
    time: Res<Time>,
    generator: Option<Res<Generator>>,
    player: Res<Player>,
    mut nearby: ResMut<Nearby>,
) {
    let Some(generator) = generator else { return };
    nearby.tick -= time.delta_secs();
    if nearby.tick > 0.0 {
        return;
    }
    nearby.tick = 1.0;
    // Down on a floor: keep the list (the entrance is where you come back to).
    if worldgen::in_pocket(player.pos.x as i32, player.pos.z as i32) {
        return;
    }
    let p = player.pos.floor().as_ivec3();
    nearby.list = generator
        .0
        .dungeons_in(p.x - LOOK, p.z - LOOK, p.x + LOOK, p.z + LOOK);
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
        commands.insert_resource(VeilLook {
            mesh: meshes.add(Rectangle::new(1.0, 1.0)),
            sheets,
        });
        return;
    };
    for (e, v) in &veils {
        if nearby
            .list
            .iter()
            .find(|d| d.id == v.0)
            .is_none_or(|d| gate_dist(d, player.pos) > GONE)
        {
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
            .spawn((
                VeilOf(d.id),
                Transform::from_translation(Vec3::from_array(d.veil.center)).with_rotation(turn),
                Visibility::default(),
            ))
            .with_children(|c| {
                // Two sheets a hand apart, shimmering out of step.
                for (i, sheet) in look.sheets.iter().enumerate() {
                    c.spawn((
                        Mesh3d(look.mesh.clone()),
                        MeshMaterial3d(sheet.clone()),
                        Transform::from_xyz(0.0, 0.0, -0.1 - 0.25 * i as f32)
                            .with_scale(Vec3::new(d.veil.width, d.veil.height, 1.0)),
                        NotShadowCaster,
                    ));
                }
                // The veil's own blue light on the doorway round it.
                c.spawn((
                    PointLight {
                        color: Color::srgb(VEIL_COLOR.0, VEIL_COLOR.1, VEIL_COLOR.2),
                        intensity: 60_000.0,
                        range: 9.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
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
            m.base_color = Color::srgba(
                VEIL_COLOR.0,
                VEIL_COLOR.1,
                VEIL_COLOR.2,
                (0.18 + 0.16 * wave) * thin,
            );
            m.emissive = LinearRgba::rgb(0.2, 0.45, 1.0) * (0.7 + 0.8 * wave) * thin;
        }
    }
    if t >= *next_sparkle {
        *next_sparkle = t + 0.35;
        for tf in &veils {
            let r = tf.rotation * Vec3::new((t * 12.9).sin() * 1.3, (t * 7.7).cos() * 1.3, 0.0);
            bursts.write(crate::station::particles::Burst::new(
                tf.translation + r,
                crate::station::particles::BurstKind::Sparkle,
                2,
            ));
        }
    }
}
