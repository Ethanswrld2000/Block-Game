//! F3 debug overlay: frame rate, position, chunk streaming stats, target block.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::interact::{Hotbar, Target};
use crate::world::{Generator, ViewDistance, World, WorldStats};

pub struct DebugOverlayPlugin;

impl Plugin for DebugOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_overlay)
            .add_systems(Update, (toggle_overlay, update_overlay));
    }
}

#[derive(Component)]
struct DebugText;

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        DebugText,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(15.0), ..default() },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(8.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
    ));
    // Crosshair.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: Val::Px(4.0),
            height: Val::Px(4.0),
            margin: UiRect { left: Val::Px(-2.0), top: Val::Px(-2.0), ..default() },
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
    ));
}

fn toggle_overlay(keys: Res<ButtonInput<KeyCode>>, mut q: Query<&mut Visibility, With<DebugText>>) {
    if keys.just_pressed(KeyCode::F3) {
        for mut v in &mut q {
            v.toggle_visible_hidden();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_overlay(
    diagnostics: Res<DiagnosticsStore>,
    cam: Query<&Transform, With<FlyCamera>>,
    stats: Res<WorldStats>,
    view: Res<ViewDistance>,
    target: Res<Target>,
    world: Res<World>,
    blocks: Res<Blocks>,
    hotbar: Option<Res<Hotbar>>,
    generator: Option<Res<Generator>>,
    mut text: Query<&mut Text, With<DebugText>>,
) {
    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let ms = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let Ok(tf) = cam.single() else { return };
    let p = tf.translation;
    let block = BlockPos::from_world(p.to_array());
    let chunk = block.chunk();
    let name = |id: voxel::BlockId| blocks.registry.get(id).name.as_str();
    let looking = match target.hit {
        Some(h) => {
            let b = world.block(h.block).map(name).unwrap_or("?");
            format!("{b} at {} {} {}", h.block.x, h.block.y, h.block.z)
        }
        None => "nothing".into(),
    };
    let biome = generator.map(|g| g.0.biome_at(block.x, block.y, block.z).title.clone()).unwrap_or_default();
    let selected = hotbar.map(|h| format!("[{}] {}", h.selected + 1, name(h.slots[h.selected]))).unwrap_or_default();
    for mut t in &mut text {
        t.0 = format!(
            "{fps:.0} fps  ({ms:.2} ms)\n\
             pos   {:.1} {:.1} {:.1}   chunk {} {} {}\n\
             biome {biome}\n\
             chunks {} loaded, {} drawn, {} queued, {} unsaved\n\
             tasks  {} generating, {} meshing   data {} KiB\n\
             view   {} chunks (- / =)\n\
             target {looking}\n\
             place  {selected}  (keys 1-9)\n\
             F3 hide | click: capture mouse | Esc: release\n\
             left click: break | right click: place",
            p.x, p.y, p.z, chunk.x, chunk.y, chunk.z,
            stats.loaded, stats.meshed, stats.queued, stats.unsaved,
            stats.gen_in_flight, stats.mesh_in_flight, stats.chunk_kib,
            view.chunks,
        );
    }
}
