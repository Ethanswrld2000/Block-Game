//! Your hand on screen: an arm holding whatever you have selected (a tool, a
//! block, a torch with its light). It swings when used and sways with your
//! steps. Both hands are built; the left arm shows only while the left-hand
//! slot holds something.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use voxel::{BlockDef, Face};

use crate::blocks::{Blocks, VoxelMaterial};
use crate::camera::FlyCamera;
use crate::interact::{Hand, HandAnim, Mining, OffHand, Wielded};
use crate::item_art::ItemArt;
use crate::items::{ItemId, ItemKind, Items};
use crate::player::{MoveMode, Player};
use crate::swing::Swing;
use crate::torches::{Flicker, TORCH_COLOR, TORCH_LUMENS, TORCH_RANGE};

pub struct HandsPlugin;

impl Plugin for HandsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::menu::GameUpdate, (spawn_hands, update_held, pose_hands).chain().after(crate::interact::InteractSet).after(crate::player::PlayerSet));
    }
}

#[derive(Component)]
pub(crate) struct HandRoot(Hand);

/// Holds the model of the item in a hand; rebuilt when the item changes.
/// The view-model arm has one, and so does the right fist of your body
/// (`avatar`, seen in third person).
#[derive(Component)]
pub(crate) struct Held {
    hand: Hand,
    /// Always this item, whatever you wield (a foe's sword).
    fixed: Option<ItemId>,
    showing: Option<Option<ItemId>>,
}

impl Held {
    pub(crate) fn new(hand: Hand) -> Self {
        Self { hand, fixed: None, showing: None }
    }

    /// A fist that always holds `item` (foes' weapons).
    pub(crate) fn holding(item: ItemId) -> Self {
        Self { hand: Hand::Right, fixed: Some(item), showing: None }
    }
}

/// Metres per sprite pixel for a held item (before the hand's own scale).
const HAND_PX: f32 = 0.0095;


/// Where the fist rests, relative to the eye.
fn base_position(hand: Hand) -> Vec3 {
    match hand {
        Hand::Right => Vec3::new(0.27, -0.27, -0.46),
        Hand::Left => Vec3::new(-0.27, -0.27, -0.46),
    }
}

fn spawn_hands(
    mut commands: Commands,
    cam: Query<Entity, With<FlyCamera>>,
    existing: Query<(), With<HandRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    looks: Res<crate::avatar::PlayerLooks>,
) {
    if !existing.is_empty() {
        return;
    }
    // The same skin and shirt as your body.
    let rgb = |c: (f32, f32, f32)| Color::srgb(c.0, c.1, c.2);
    let Ok(cam) = cam.single() else { return };
    let sleeve = meshes.add(Cuboid::new(0.09, 0.09, 0.34));
    let fist = meshes.add(Cuboid::new(0.075, 0.075, 0.075));
    let sleeve_mat = materials.add(StandardMaterial { base_color: rgb(looks.0.shirt), perceptual_roughness: 1.0, ..default() });
    let skin_mat = materials.add(StandardMaterial { base_color: rgb(looks.0.skin), perceptual_roughness: 0.9, ..default() });
    for hand in [Hand::Right, Hand::Left] {
        commands.entity(cam).with_children(|c| {
            c.spawn((HandRoot(hand), Transform::from_translation(base_position(hand)), Visibility::default())).with_children(|h| {
                h.spawn((Mesh3d(sleeve.clone()), MeshMaterial3d(sleeve_mat.clone()), Transform::from_xyz(0.0, -0.015, 0.2), NotShadowCaster));
                h.spawn((Mesh3d(fist.clone()), MeshMaterial3d(skin_mat.clone()), Transform::default(), NotShadowCaster));
                // Items are held tilted up and forward.
                h.spawn((
                    Held { hand, fixed: None, showing: None },
                    // Items are life-size; at arm's length in front of the
                    // eye they read better a bit smaller.
                    Transform::from_xyz(0.0, 0.03, -0.02).with_rotation(Quat::from_rotation_x(0.9)).with_scale(Vec3::splat(0.42)),
                    Visibility::default(),
                ));
            });
        });
    }
}

/// A small cube of a block, textured like the real thing.
pub(crate) fn block_mesh(def: &BlockDef, size: f32) -> Mesh {
    let (mut pos, mut nor, mut uv, mut layer, mut idx) = (vec![], vec![], vec![], vec![], vec![]);
    let h = size * 0.5;
    for face in Face::ALL {
        let n = face.normal().map(|v| v as f32);
        let axis = face.axis();
        let (ua, va) = face.plane_axes();
        let tex = def.textures[face as usize] as f32;
        let start = pos.len() as u32;
        for (du, dv) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            let mut c = [-h, -h, -h];
            c[axis] = if face.is_positive() { h } else { -h };
            c[ua] = -h + du * size;
            c[va] = -h + dv * size;
            pos.push(c);
            nor.push(n);
            uv.push(if axis == 1 { [du, dv] } else { [du, 1.0 - dv] });
            layer.push([tex, 0.0]);
        }
        let flip = (axis == 1) == face.is_positive();
        if flip {
            idx.extend_from_slice(&[start, start + 2, start + 1, start, start + 3, start + 2]);
        } else {
            idx.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, layer)
        .with_inserted_indices(Indices::U32(idx))
}

#[allow(clippy::too_many_arguments)]
fn update_held(
    mut commands: Commands,
    (wielded, off): (Res<Wielded>, Res<OffHand>),
    items: Res<Items>,
    blocks: Res<Blocks>,
    art: Option<Res<ItemArt>>,
    mut held: Query<(Entity, &mut Held)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(voxel_mat), Some(art)) = (blocks.material.clone(), art) else { return };
    if !art.ready {
        return;
    }
    for (e, mut h) in &mut held {
        let id = match h.fixed {
            Some(item) => Some(item),
            None if h.hand == Hand::Right => wielded.0,
            None => off.0,
        };
        if h.showing == Some(id) {
            continue;
        }
        h.showing = Some(id);
        commands.entity(e).despawn_children();
        let Some(id) = id else { continue };
        let item = items.get(id);
        commands.entity(e).with_children(|c| match &item.kind {
            ItemKind::Block(b) => {
                let mesh = meshes.add(block_mesh(blocks.registry.get(*b), 0.26));
                c.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d::<VoxelMaterial>(voxel_mat.clone()),
                    Transform::from_xyz(0.0, 0.0, -0.12).with_rotation(Quat::from_rotation_x(-0.6) * Quat::from_rotation_y(0.5)),
                    NotShadowCaster,
                ));
            }
            _ => {
                // The item's sprite, made solid: held near the bottom (the
                // end of a handle), pointing forward.
                let id_size = art.size(id).unwrap_or(UVec2::splat(20)).as_vec2();
                let grip = if id_size.y >= id_size.x * 1.5 { id_size.y * 0.2 } else { id_size.y * 0.5 };
                // Small things (a handful of berries) are drawn a bit bigger
                // so they show above the hotbar.
                let px = HAND_PX * (36.0 / id_size.max_element()).max(1.0);
                let to_center = (id_size.y / 2.0 - grip) * px;
                let place = Transform::from_xyz(0.0, 0.0, -to_center).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)).with_scale(Vec3::splat(px));
                match art.model(id) {
                    Some((mesh, mat)) => {
                        c.spawn((Mesh3d(mesh), MeshMaterial3d(mat), place, NotShadowCaster));
                    }
                    None => {
                        c.spawn((Mesh3d(meshes.add(Cuboid::new(0.16, 0.12, 0.2))), MeshMaterial3d(materials.add(Color::srgb(0.5, 0.45, 0.38))), Transform::from_xyz(0.0, 0.0, -0.12), NotShadowCaster));
                    }
                }
                if matches!(item.kind, ItemKind::Torch) {
                    // A light that goes where you go, just past the flame.
                    let tip = -(id_size.y - grip) * px;
                    c.spawn((
                        PointLight { intensity: TORCH_LUMENS * 0.8, range: TORCH_RANGE, color: TORCH_COLOR, shadow_maps_enabled: false, ..default() },
                        Flicker { base: TORCH_LUMENS * 0.8, seed: if h.hand == Hand::Left { 1.0 } else { 2.0 } },
                        Transform::from_xyz(0.0, 0.25, tip - 0.7),
                    ));
                }
            }
        });
    }
}

/// `BLOCKGAME_SWING=0.55` holds the right arm still that far through a
/// swing of what it holds, for screenshots of the swing.
pub(crate) fn frozen_swing() -> Option<f32> {
    std::env::var("BLOCKGAME_SWING").ok()?.parse().ok()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn pose_hands(
    anim: Res<HandAnim>,
    pov: Res<crate::pov::Pov>,
    mining: Res<Mining>,
    player: Res<Player>,
    mode: Res<MoveMode>,
    (wielded, off): (Res<Wielded>, Res<OffHand>),
    items: Res<Items>,
    view: Res<crate::camera::View>,
    guard: Res<crate::combat::Guard>,
    grab: Res<crate::station::grab::Grab>,
    mut frozen: Local<Option<Option<f32>>>,
    mut roots: Query<(&HandRoot, &mut Transform, &mut Visibility)>,
) {
    let walking = *mode == MoveMode::Walk;
    let frozen = *frozen.get_or_insert_with(frozen_swing);
    for (root, mut tf, mut vis) in &mut roots {
        let hand = root.0;
        // In third person your body holds things instead; the left arm only
        // comes up while it holds something.
        // Both come up to hold an urn (`station::grab`).
        let held = grab.blend();
        let shown = !view.pulled && (hand == Hand::Right || off.0.is_some() || held > 0.001);
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let arm = anim.arm(hand);
        let (style, t) = match (frozen, hand) {
            (Some(t), Hand::Right) => (wielded.0.map_or(Swing::Jab, |i| items.get(i).swing), Some(t)),
            _ => (arm.style, arm.t),
        };
        // Wind up, strike, bounce off, back to rest (see `swing`).
        let (mut swing_pos, mut swing_rot) = t.map_or((Vec3::ZERO, Quat::IDENTITY), |t| style.pose(t));
        // A sword raised to block (`combat::Guard`).
        if hand == Hand::Right && guard.blend > 0.001 {
            let (gp, gr) = crate::swing::guard_pose();
            swing_pos = swing_pos.lerp(gp, guard.blend);
            swing_rot = swing_rot.slerp(gr, guard.blend);
        }
        let side = if hand == Hand::Right { 1.0 } else { -1.0 };
        let mirror = Vec3::new(side, 1.0, 1.0);
        let swing_rot = if hand == Hand::Right { swing_rot } else { Quat::from_xyzw(swing_rot.x, -swing_rot.y, -swing_rot.z, swing_rot.w) };
        // Sway with the steps, opposite to the head so the arms feel heavy.
        let (bob, phase) = if walking { (player.bob, player.bob_phase) } else { (0.0, 0.0) };
        let sway = Vec3::new((phase * 0.5).sin() * 0.018 * bob, -(phase.sin().abs()) * 0.02 * bob, 0.0);
        let lift = if mining.block.is_some() && mining.hand == hand { 0.02 } else { 0.0 };
        // Eating: bring the food up toward the mouth and bob while chewing.
        let e = if anim.eat_hand == hand { (anim.eat * 4.0).min(1.0) } else { 0.0 };
        let chew = (anim.eat * 40.0).sin() * 0.012 * e;
        let to_mouth = Vec3::new(-side * 0.2, 0.12 + chew, 0.12) * e;
        // Motion feel (`pov`), eased out while the hand is busy (a swing,
        // a guard, eating) so those poses stay as they are.
        let busy = if t.is_some() || e > 0.0 || (hand == Hand::Right && guard.blend > 0.001) { 1.0 } else { 0.0 };
        let calm = 1.0 - busy;
        // Sprint carry: low, pulled in and tipped down, pumping with the stride.
        let k = pov.sprint * calm;
        let pump = (phase * 0.5).sin() * k;
        let sprint_pos = Vec3::new(-side * 0.04 * k, -0.06 * k + pump * 0.015 * side, 0.03 * k);
        let sprint_rot = Quat::from_rotation_x(-0.45 * k + pump * 0.08 * side) * Quat::from_rotation_z(side * 0.35 * k) * Quat::from_rotation_y(side * 0.2 * k);
        // Falls and landings: the hands sink, get knocked down, spring back.
        let drop = pov.drop.x;
        let drop_rot = Quat::from_rotation_x(drop * 2.5);
        // Look sway: trail the turn a little, sideways and in angle.
        let (ly, lp) = (pov.lag_yaw.x * calm, pov.lag_pitch.x * calm);
        let lag_pos = Vec3::new(-ly * 0.25, lp * 0.25, 0.0);
        let lag_rot = Quat::from_rotation_y(ly) * Quat::from_rotation_x(lp) * Quat::from_rotation_z(-ly * 0.6);
        tf.translation = base_position(hand) + sway + to_mouth + Vec3::Y * (lift + drop) + swing_pos * mirror + sprint_pos + lag_pos;
        tf.rotation = lag_rot * drop_rot * sprint_rot * swing_rot * Quat::from_rotation_x(0.5 * e) * Quat::from_rotation_y(side * 0.5 * e);
        // Holding an urn by its shoulders: the hands go to its sides, the
        // arms angled out and the fists turned in against the clay.
        if held > 0.001 {
            let grip = grab.grips[if hand == Hand::Right { 0 } else { 1 }];
            let hold = Quat::from_rotation_y(side * 0.45) * Quat::from_rotation_z(side * 0.5) * Quat::from_rotation_x(-0.2);
            tf.translation = tf.translation.lerp(grip, held);
            tf.rotation = tf.rotation.slerp(hold, held);
        }
    }
}
