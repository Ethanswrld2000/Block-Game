//! Breaking and placing blocks.
//!
//! Left click breaks, right click places the selected block. Keys 1-9 pick
//! the block to place. A wireframe box marks the targeted block.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use voxel::raycast::raycast;
use voxel::{BlockId, BlockPos};

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::world::World;

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>()
            .add_systems(Startup, setup_hotbar)
            .add_systems(
                Update,
                (pick_block, find_target, edit_blocks, draw_target)
                    .chain()
                    .before(crate::camera::grab_cursor),
            );
    }
}

/// How far the player can reach, in blocks.
pub const REACH: f32 = 8.0;

/// Hotbar contents for now. Phase 7 replaces this with the real inventory.
const HOTBAR: [&str; 9] = ["dirt", "stone", "cobblestone", "planks", "log", "leaves", "moss", "sand", "skystone"];

#[derive(Resource)]
pub struct Hotbar {
    pub slots: Vec<BlockId>,
    pub selected: usize,
}

#[derive(Resource, Default)]
pub struct Target {
    pub hit: Option<voxel::raycast::RayHit>,
}

fn setup_hotbar(mut commands: Commands, blocks: Res<Blocks>) {
    let slots = HOTBAR.iter().map(|n| blocks.registry.expect(n)).collect();
    commands.insert_resource(Hotbar { slots, selected: 0 });
}

fn pick_block(keys: Res<ButtonInput<KeyCode>>, mut hotbar: ResMut<Hotbar>) {
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5,
        KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
    ];
    for (i, k) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*k) && i < hotbar.slots.len() {
            hotbar.selected = i;
        }
    }
}

fn find_target(world: Res<World>, cam: Single<&Transform, With<FlyCamera>>, mut target: ResMut<Target>) {
    let origin = cam.translation.to_array();
    let dir = cam.forward().as_vec3().to_array();
    target.hit = raycast(origin, dir, REACH, |p| world.block(p).is_some_and(|b| !b.is_air()));
}

fn edit_blocks(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    target: Res<Target>,
    hotbar: Res<Hotbar>,
    blocks: Res<Blocks>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut world: ResMut<World>,
) {
    // Clicks only edit while the mouse is captured (the first click captures it).
    if cursor.grab_mode == CursorGrabMode::None {
        return;
    }
    let Some(hit) = target.hit else { return };
    let changed = if mouse.just_pressed(MouseButton::Left) {
        world.set_block(hit.block, BlockId::AIR).then_some(hit.block)
    } else if mouse.just_pressed(MouseButton::Right) {
        let eye = BlockPos::from_world(cam.translation.to_array());
        if hit.before == eye || world.block(hit.before).is_none_or(|b| !b.is_air()) {
            None
        } else {
            world.set_block(hit.before, hotbar.slots[hotbar.selected]).then_some(hit.before)
        }
    } else {
        None
    };
    if let Some(p) = changed {
        // Remesh the edited chunk and any neighbour sharing the face, immediately.
        let c = p.chunk();
        for d in [[0, 0, 0], [1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
            let n = voxel::ChunkPos::new(c.x + d[0], c.y + d[1], c.z + d[2]);
            if world.needs_mesh(n) {
                world.mesh_now(n, &blocks.registry, &blocks.opaque);
            }
        }
    }
}

fn draw_target(target: Res<Target>, mut gizmos: Gizmos) {
    if let Some(hit) = target.hit {
        let p = hit.block;
        let center = Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5);
        gizmos.cube(Transform::from_translation(center).with_scale(Vec3::splat(1.003)), Color::srgba(0.05, 0.05, 0.05, 0.9));
    }
}
