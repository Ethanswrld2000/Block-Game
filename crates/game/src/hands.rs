//! Your hand on screen: an arm holding whatever you have selected (a tool, a
//! block, a torch with its light). It swings when used and sways with your
//! steps. Built per `Hand`, so an off-hand can be switched on later.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use voxel::{BlockDef, Face};

use crate::blocks::{Blocks, VoxelMaterial};
use crate::camera::FlyCamera;
use crate::interact::{Hand, HandAnim, Hands, Hotbar, Mining};
use crate::items::{ItemKind, Items};
use crate::player::{MoveMode, Player};
use crate::torches::{Flicker, TorchAssets, TORCH_COLOR, TORCH_LUMENS, TORCH_RANGE};

pub struct HandsPlugin;

impl Plugin for HandsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_hands, update_held, pose_hands).chain().after(crate::interact::InteractSet));
    }
}

#[derive(Component)]
struct HandRoot(Hand);

/// Holds the model of the item in a hand; rebuilt when the item changes.
#[derive(Component)]
struct Held {
    hand: Hand,
    showing: Option<Option<usize>>,
}

const SKIN: Color = Color::srgb(0.86, 0.66, 0.5);
const SLEEVE: Color = Color::srgb(0.32, 0.36, 0.26);

fn base_position(hand: Hand) -> Vec3 {
    match hand {
        Hand::Right => Vec3::new(0.27, -0.27, -0.46),
        Hand::Left => Vec3::new(-0.27, -0.27, -0.46),
    }
}

#[allow(clippy::single_element_loop)] // one hand for now
fn spawn_hands(
    mut commands: Commands,
    cam: Query<Entity, With<FlyCamera>>,
    existing: Query<(), With<HandRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !existing.is_empty() {
        return;
    }
    let Ok(cam) = cam.single() else { return };
    let sleeve = meshes.add(Cuboid::new(0.09, 0.09, 0.34));
    let fist = meshes.add(Cuboid::new(0.075, 0.075, 0.075));
    let sleeve_mat = materials.add(StandardMaterial { base_color: SLEEVE, perceptual_roughness: 1.0, ..default() });
    let skin_mat = materials.add(StandardMaterial { base_color: SKIN, perceptual_roughness: 0.9, ..default() });
    // One hand for now; add Hand::Left here with the off-hand slot.
    for hand in [Hand::Right] {
        commands.entity(cam).with_children(|c| {
            c.spawn((HandRoot(hand), Transform::from_translation(base_position(hand)), Visibility::default())).with_children(|h| {
                h.spawn((Mesh3d(sleeve.clone()), MeshMaterial3d(sleeve_mat.clone()), Transform::from_xyz(0.0, -0.015, 0.2), NotShadowCaster));
                h.spawn((Mesh3d(fist.clone()), MeshMaterial3d(skin_mat.clone()), Transform::default(), NotShadowCaster));
                // Items are held tilted up and forward.
                h.spawn((
                    Held { hand, showing: None },
                    // Models in items.ron are life-size; at arm's length in
                    // front of the eye they read better a bit smaller.
                    Transform::from_xyz(0.0, 0.03, -0.02).with_rotation(Quat::from_rotation_x(0.9)).with_scale(Vec3::splat(0.42)),
                    Visibility::default(),
                ));
            });
        });
    }
}

/// A small cube of a block, textured like the real thing.
fn block_mesh(def: &BlockDef, size: f32) -> Mesh {
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
    hands: Res<Hands>,
    hotbar: Res<Hotbar>,
    items: Res<Items>,
    blocks: Res<Blocks>,
    torch: Option<Res<TorchAssets>>,
    mut held: Query<(Entity, &mut Held)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(voxel_mat), Some(torch)) = (blocks.material.clone(), torch) else { return };
    for (e, mut h) in &mut held {
        let slot = hands.slot(h.hand);
        if h.showing == Some(slot) {
            continue;
        }
        h.showing = Some(slot);
        commands.entity(e).despawn_children();
        let Some(id) = hands.item(h.hand, &hotbar) else { continue };
        let item = items.get(id);
        commands.entity(e).with_children(|c| match &item.kind {
            ItemKind::Block(b) => {
                let mesh = meshes.add(block_mesh(blocks.registry.get(*b), 0.36));
                c.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d::<VoxelMaterial>(voxel_mat.clone()),
                    Transform::from_xyz(0.0, 0.0, -0.12).with_rotation(Quat::from_rotation_x(-0.6) * Quat::from_rotation_y(0.5)),
                    NotShadowCaster,
                ));
            }
            _ => {
                for b in &item.model {
                    let mat = materials.add(StandardMaterial {
                        base_color: Color::srgb(b.color.0, b.color.1, b.color.2),
                        perceptual_roughness: 0.9,
                        ..default()
                    });
                    c.spawn((
                        Mesh3d(meshes.add(Cuboid::new(b.size.0, b.size.1, b.size.2))),
                        MeshMaterial3d(mat),
                        Transform::from_xyz(b.offset.0, b.offset.1, b.offset.2),
                        NotShadowCaster,
                    ));
                }
                if matches!(item.kind, ItemKind::Torch) {
                    // A burning tip and a light that goes where you go.
                    c.spawn((Mesh3d(torch.flame.clone()), MeshMaterial3d(torch.fire.clone()), Transform::from_xyz(0.0, 0.0, -0.33).with_scale(Vec3::splat(0.55)), NotShadowCaster));
                    c.spawn((
                        PointLight { intensity: TORCH_LUMENS * 0.8, range: TORCH_RANGE, color: TORCH_COLOR, shadow_maps_enabled: false, ..default() },
                        Flicker { base: TORCH_LUMENS * 0.8, seed: if h.hand == Hand::Left { 1.0 } else { 2.0 } },
                        Transform::from_xyz(0.0, 0.0, -0.4),
                    ));
                }
            }
        });
    }
}

fn pose_hands(anim: Res<HandAnim>, mining: Res<Mining>, player: Res<Player>, mode: Res<MoveMode>, mut roots: Query<(&HandRoot, &mut Transform)>) {
    let walking = *mode == MoveMode::Walk;
    for (root, mut tf) in &mut roots {
        let hand = root.0;
        let swing = match hand {
            Hand::Left => anim.left,
            Hand::Right => anim.right,
        };
        // 1 -> 0 over a swing: chop down and forward, then back.
        let t = 1.0 - swing;
        let arc = if swing > 0.0 { (t * std::f32::consts::PI).sin() } else { 0.0 };
        let side = if hand == Hand::Right { 1.0 } else { -1.0 };
        // Sway with the steps, opposite to the head so the arms feel heavy.
        let (bob, phase) = if walking { (player.bob, player.bob_phase) } else { (0.0, 0.0) };
        let sway = Vec3::new((phase * 0.5).sin() * 0.018 * bob, -(phase.sin().abs()) * 0.02 * bob, 0.0);
        let lift = if mining.block.is_some() && mining.hand == hand { 0.02 } else { 0.0 };
        tf.translation = base_position(hand) + sway + Vec3::new(-side * 0.06 * arc, lift - 0.05 * arc, -0.12 * arc);
        tf.rotation = Quat::from_rotation_x(-1.1 * arc) * Quat::from_rotation_y(side * 0.25 * arc);
    }
}
