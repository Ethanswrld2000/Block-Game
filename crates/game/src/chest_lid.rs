//! Opening a chest by hand: right click a chest and your right hand reaches
//! out to the front of its lid and pushes it up on its back hinge while you
//! lean in over it; as you come close the chest's contents open on screen
//! (`interact::OpenChest`, the inventory's Chest tab). Closing the screen lets
//! the lid fall shut and you lean back.
//!
//! The chest block is drawn with the chunk (`chunk_mesh::model_boxes`, a box
//! and a lid). While the lid moves the block is `chest_open` (the box alone)
//! and the lid is a model of its own, hinged on the side away from you.

use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::{Blocks, VoxelMaterial};
use crate::camera::{FlyCamera, Menus};
use crate::interact::OpenChest;
use crate::inventory::ui::InvScreen;
use crate::sound::PlaySound;
use crate::world::World;

/// Seconds to reach out, push the lid up and lean in; and to let it fall back.
const REACH_SECONDS: f32 = 0.75;
const RELEASE_SECONDS: f32 = 0.45;
/// How far into the reach the chest's screen opens.
const OPEN_AT: f32 = 0.85;
/// How far the lid swings up (radians).
const LID_OPEN: f32 = 1.85;
/// The hand gets to the lid in this part of the reach, then pushes it up.
const HAND_FIRST: f32 = 0.3;
/// Seconds the swinging lid stays up after the chest is shut again (until the
/// chunk has been meshed with its own lid back on).
const LINGER: f32 = 0.35;

pub struct ChestLidPlugin;

impl Plugin for ChestLidPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChestOpen>().add_message::<ReachChest>().add_systems(
            crate::menu::GameUpdate,
            (reach, swing).chain().after(crate::interact::InteractSet).after(crate::player::PlayerSet).before(crate::hands::pose_hands),
        );
    }
}

/// Right click on a chest (from `interact`).
#[derive(Message, Debug, Clone, Copy)]
pub struct ReachChest(pub BlockPos);

/// A chest being opened by hand.
#[derive(Resource, Default)]
pub struct ChestOpen {
    at: Option<BlockPos>,
    /// 0..1 through the reach (linear).
    t: f32,
    opened: bool,
    releasing: bool,
    /// Where the face leans to.
    view: Transform,
    /// The lid's hinge (world) and the way from it to the lid's front (toward you).
    hinge: Vec3,
    front: Vec3,
    /// The swinging lid.
    lid: Option<Entity>,
    /// Seconds left before the swinging lid goes (after the chest is shut).
    linger: f32,
    /// The right hand's grip on the lid's front edge, in the view's frame.
    pub grip: Vec3,
}

impl ChestOpen {
    /// How far the right hand is out at the lid (0..1, eased).
    pub fn blend(&self) -> f32 {
        if self.at.is_none() || self.linger > 0.0 { 0.0 } else { smooth(self.t / HAND_FIRST) }
    }

    /// How far up the lid is (radians).
    fn lid_angle(&self) -> f32 {
        smooth((self.t - HAND_FIRST) / (1.0 - HAND_FIRST)) * LID_OPEN
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The side of the chest you stand on: a unit step along x or z.
pub fn facing_side(eye: Vec3, centre: Vec3) -> Vec3 {
    let d = eye - centre;
    if d.x.abs() >= d.z.abs() { Vec3::X * d.x.signum() } else { Vec3::Z * d.z.signum() }
}

/// Where the face leans: in front of the chest, over it, looking down into it.
pub fn lean_view(base: Vec3, front: Vec3) -> Transform {
    let inside = base + Vec3::new(0.5, 0.35, 0.5);
    Transform::from_translation(inside + front * 0.95 + Vec3::Y * 1.05).looking_at(inside, Vec3::Y)
}

/// The lid's hinge for a chest whose low corner is `base`, opened from `front`.
fn hinge_of(base: Vec3, front: Vec3) -> Vec3 {
    let (lo, hi) = crate::chunk_mesh::CHEST_LID;
    base + Vec3::new(0.5, lo[1], 0.5) - front * (hi[0] - lo[0]) / 2.0
}

/// The lid turned up by `angle` about its hinge.
fn lid_rotation(front: Vec3, angle: f32) -> Quat {
    Quat::from_axis_angle(front.cross(Vec3::Y).normalize(), angle)
}

/// Start reaching for a chest; set the lid going.
#[allow(clippy::too_many_arguments)]
fn reach(
    mut commands: Commands,
    mut asks: MessageReader<ReachChest>,
    mut open: ResMut<ChestOpen>,
    mut world: ResMut<World>,
    blocks: Res<Blocks>,
    mut menus: ResMut<Menus>,
    mut meshes: ResMut<Assets<Mesh>>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut sound: MessageWriter<PlaySound>,
) {
    let Some(ask) = asks.read().last().copied() else { return };
    if open.at.is_some() {
        return;
    }
    let (Some(chest), Some(lidless), Some(material)) = (blocks.registry.id("chest"), blocks.registry.id("chest_open"), blocks.material.clone()) else { return };
    let p = ask.0;
    let base = Vec3::new(p.x as f32, p.y as f32, p.z as f32);
    let front = facing_side(cam.translation, base + Vec3::splat(0.5));
    let hinge = hinge_of(base, front);
    // The lid as a model of its own, hinged at the back, in the chest's light.
    let tile = blocks.registry.get(chest).textures[voxel::Face::PosX as usize];
    let light = world.light_at(p).unwrap_or((voxel::light::MAX, 0));
    let Some(mesh) = crate::chunk_mesh::boxes_mesh(&[crate::chunk_mesh::CHEST_LID], tile, light) else { return };
    let lid = commands
        .spawn((Transform::from_translation(hinge), Visibility::default()))
        .with_children(|c| {
            c.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d::<VoxelMaterial>(material), Transform::from_translation(base - hinge)));
        })
        .id();
    if world.block(p) == Some(chest) && world.set_block(p, lidless) {
        crate::interact::remesh_around(&mut world, &blocks, p);
    }
    *open = ChestOpen { at: Some(p), t: 0.0, opened: false, releasing: false, view: lean_view(base, front), hinge, front, lid: Some(lid), linger: 0.0, grip: Vec3::ZERO };
    menus.still = true;
    sound.write(PlaySound::new("ui_open").at(base + Vec3::splat(0.5)).speed(0.8));
}

/// Push the lid up and lean in; open the chest's screen; let it fall shut once
/// the screen is closed.
#[allow(clippy::too_many_arguments)]
fn swing(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut open: ResMut<ChestOpen>,
    mut world: ResMut<World>,
    blocks: Res<Blocks>,
    screen: Res<InvScreen>,
    mut menus: ResMut<Menus>,
    mut cam: Single<&mut Transform, With<FlyCamera>>,
    mut lids: Query<&mut Transform, Without<FlyCamera>>,
    mut show: MessageWriter<OpenChest>,
    mut sound: MessageWriter<PlaySound>,
) {
    let Some(p) = open.at else { return };
    let dt = time.delta_secs().min(0.1);
    if open.linger > 0.0 {
        open.linger -= dt;
        if open.linger <= 0.0 {
            if let Some(e) = open.lid.take() {
                commands.entity(e).despawn();
            }
            open.at = None;
        }
        return;
    }
    if open.opened && !screen.open {
        open.releasing = true;
    }
    if open.releasing {
        open.t -= dt / RELEASE_SECONDS;
        if open.t <= 0.0 {
            open.t = 0.0;
            // Shut: the chest gets its own lid back, the moving one goes once that shows.
            if let (Some(chest), Some(lidless)) = (blocks.registry.id("chest"), blocks.registry.id("chest_open"))
                && world.block(p) == Some(lidless)
                && world.set_block(p, chest)
            {
                crate::interact::remesh_around(&mut world, &blocks, p);
            }
            open.linger = LINGER;
            menus.still = false;
            let at = Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5);
            sound.write(PlaySound::new("hit").at(at).speed(1.4).gain(0.5));
            if let Some(e) = open.lid
                && let Ok(mut tf) = lids.get_mut(e)
            {
                tf.rotation = Quat::IDENTITY;
            }
            return;
        }
    } else {
        open.t = (open.t + dt / REACH_SECONDS).min(1.0);
        if !open.opened && open.t >= OPEN_AT {
            open.opened = true;
            show.write(OpenChest(p));
        }
    }
    let angle = open.lid_angle();
    let turn = lid_rotation(open.front, angle);
    if let Some(e) = open.lid
        && let Ok(mut tf) = lids.get_mut(e)
    {
        tf.rotation = turn;
    }
    // Lean in over the chest.
    let k = smooth(open.t);
    cam.translation = cam.translation.lerp(open.view.translation, k);
    cam.rotation = cam.rotation.slerp(open.view.rotation, k);
    // The hand on the lid's front edge, a little under it, as it goes up.
    let (lo, hi) = crate::chunk_mesh::CHEST_LID;
    let edge = open.hinge + turn * (open.front * (hi[0] - lo[0]) + Vec3::Y * 0.02);
    open.grip = cam.to_matrix().inverse().transform_point3(edge);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lid_swings_up_away_from_you() {
        let base = Vec3::new(4.0, 10.0, -2.0);
        for eye in [Vec3::new(8.0, 11.6, -1.5), Vec3::new(4.5, 11.6, -6.0)] {
            let front = facing_side(eye, base + Vec3::splat(0.5));
            let hinge = hinge_of(base, front);
            // The hinge is on the far side; the front edge starts toward you and rises.
            assert!((hinge - eye).length() > (hinge + front - eye).length());
            let closed = hinge + lid_rotation(front, 0.0) * front;
            let up = hinge + lid_rotation(front, 1.2) * front;
            assert!(up.y > closed.y + 0.5, "{eye}: {closed} -> {up}");
        }
    }

    #[test]
    fn you_lean_in_over_the_open_chest() {
        let base = Vec3::ZERO;
        let view = lean_view(base, Vec3::X);
        assert!(view.translation.x > 1.0 && view.translation.y > 1.0);
        assert!(view.forward().y < -0.5, "looking down into it");
    }
}
