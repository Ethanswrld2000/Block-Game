//! F3 debug overlay: frame rate, position, chunk streaming stats, target block.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::clock::{Seasons, WorldClock};
use crate::lod::LodTiles;
use crate::weather::{Conditions, WeatherKinds, WeatherState};
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

#[derive(bevy::ecs::system::SystemParam)]
struct SkyInfo<'w> {
    clock: Res<'w, WorldClock>,
    seasons: Res<'w, Seasons>,
    weather: Res<'w, WeatherState>,
    kinds: Res<'w, WeatherKinds>,
    conditions: Res<'w, Conditions>,
    lod: Res<'w, LodTiles>,
}

impl SkyInfo<'_> {
    fn describe(&self) -> String {
        let h = self.clock.hour();
        let p = self.seasons.at(&self.clock);
        let season = &self.seasons.list[p.current];
        let mut weather = self.kinds.list[self.weather.current].title.clone();
        if self.conditions.precipitation > 0.05 && self.conditions.snowing {
            weather.push_str(" (snow)");
        }
        let fast = if self.clock.speed > 1.0 { "  >> fast" } else { "" };
        let moon = crate::sky::moon_phase_name(crate::sky::moon_phase(&self.clock));
        format!(
            "time   day {}  {:02}:{:02}  {} (day {} of {}), {moon}{fast}\nweather {weather}  (F4 next)  far tiles {}",
            self.clock.day + 1,
            h as u32,
            ((h.fract()) * 60.0) as u32,
            season.title,
            p.day_in_season + 1,
            self.seasons.days,
            self.lod.count,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn update_overlay(
    diagnostics: Res<DiagnosticsStore>,
    cam: Query<(&Transform, &FlyCamera)>,
    stats: Res<WorldStats>,
    view: Res<ViewDistance>,
    target: Res<Target>,
    world: Res<World>,
    blocks: Res<Blocks>,
    hotbar: Option<Res<Hotbar>>,
    generator: Option<Res<Generator>>,
    sky: SkyInfo,
    mut text: Query<&mut Text, With<DebugText>>,
) {
    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let ms = diagnostics.get(&FrameTimeDiagnosticsPlugin::FRAME_TIME).and_then(|d| d.smoothed()).unwrap_or(0.0);
    let Ok((tf, fc)) = cam.single() else { return };
    let cam_speed = fc.speed;
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
             {}\n\
             chunks {} loaded, {} drawn, {} queued, {} unsaved\n\
             tasks  {} generating, {} meshing   data {} KiB\n\
             view   {} chunks (- / =)   fly speed {:.0} (wheel, Ctrl x5)\n\
             target {looking}\n\
             place  {selected}  (keys 1-9)\n\
             F3 hide | click: capture mouse | Esc: release | hold T: fast time\n\
             left click: break | right click: place",
            p.x, p.y, p.z, chunk.x, chunk.y, chunk.z,
            sky.describe(),
            stats.loaded, stats.meshed, stats.queued, stats.unsaved,
            stats.gen_in_flight, stats.mesh_in_flight, stats.chunk_kib,
            view.chunks,
            cam_speed,
        );
    }
}
