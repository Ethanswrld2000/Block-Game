//! Game client entry point. Each system lives in its own plugin.

mod airship;
mod atmosphere;
mod avatar;
mod autoshot;
mod bench;
mod blocks;
mod camera;
mod chunk_mesh;
mod clock;
mod clouds;
mod combat;
mod creatures;
mod lod;
mod menu;
mod npc;
mod places;
mod player;
mod precipitation;
mod progress;
mod debug;
mod delve;
mod dungeon;
mod hands;
mod hud;
mod cast;
mod chop;
mod interact;
mod inventory;
mod item_art;
mod items;
mod lines;
mod cutscene;
mod story;
mod encounter;
mod save;
mod settings;
mod sky;
mod sound;
mod station;
mod theme;
mod survival;
mod swing;
mod torches;
mod weather;
mod weather_hud;
mod wind;
mod world;

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Block Game".into(),
                        ..default()
                    }),
                    ..default()
                })
                // Pixel-art textures: no blurring between texels.
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugins((theme::ThemePlugin, menu::MenuPlugin))
        .add_plugins((
            blocks::BlocksPlugin,
            camera::FlyCameraPlugin,
            save::SavePlugin,
            world::WorldPlugin,
            interact::InteractPlugin,
            chop::ChopPlugin,
            debug::DebugOverlayPlugin,
            autoshot::AutoShotPlugin,
        ))
        .add_plugins((
            clock::ClockPlugin,
            weather::WeatherPlugin,
            wind::WindPlugin,
            sky::SkyPlugin,
            atmosphere::AtmospherePlugin,
            clouds::CloudPlugin,
            weather_hud::WeatherHudPlugin,
            precipitation::PrecipitationPlugin,
            lod::LodPlugin,
        ))
        .add_plugins((
            items::ItemsPlugin,
            item_art::ItemArtPlugin,
            player::PlayerPlugin,
            hands::HandsPlugin,
            torches::TorchPlugin,
            hud::HudPlugin,
            survival::SurvivalPlugin,
            inventory::InventoryPlugin,
        ))
        .add_plugins((creatures::CreaturesPlugin, places::PlacesPlugin, progress::ProgressPlugin, station::StationPlugin, airship::AirshipPlugin, settings::SettingsPlugin, sound::SoundPlugin, bench::BenchPlugin, npc::NpcPlugin, avatar::AvatarPlugin, combat::CombatPlugin, dungeon::DungeonPlugin))
        .add_plugins((delve::DelvePlugin, cast::CastPlugin, lines::LinesPlugin, encounter::EncounterPlugin, cutscene::CutscenePlugin, story::StoryPlugin))
        .run();
    menu::relaunch_to_title();
}
