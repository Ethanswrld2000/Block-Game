//! Game client entry point. Each system lives in its own plugin.

mod airship;
mod atmosphere;
mod autoshot;
mod avatar;
mod bench;
mod blocks;
mod book;
mod camera;
mod cast;
mod chest_lid;
mod chop;
mod chunk_mesh;
mod clock;
mod clouds;
mod combat;
mod cover;
mod creatures;
mod cutscene;
mod debug;
mod delve;
mod dungeon;
mod encounter;
mod farming;
mod glowcaps;
mod guide;
mod hands;
mod hud;
mod interact;
mod inventory;
mod item_art;
mod items;
mod journey;
mod lightning;
mod lines;
mod lod;
mod map;
mod menu;
mod npc;
mod pickups;
mod places;
mod player;
mod pov;
mod precipitation;
mod progress;
mod quality;
mod save;
mod settings;
mod shrine;
mod sky;
mod sound;
mod station;
mod story;
mod survival;
mod swing;
mod theme;
mod torches;
mod weather;
mod weather_dial;
mod weather_hud;
mod wind;
mod wind_streaks;
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
            wind_streaks::WindStreaksPlugin,
            sky::SkyPlugin,
            atmosphere::AtmospherePlugin,
            clouds::CloudPlugin,
            weather_hud::WeatherHudPlugin,
            weather_dial::WeatherDialPlugin,
            precipitation::PrecipitationPlugin,
            cover::CoverPlugin,
            lightning::LightningPlugin,
            lod::LodPlugin,
        ))
        .add_plugins((
            items::ItemsPlugin,
            item_art::ItemArtPlugin,
            player::PlayerPlugin,
            hands::HandsPlugin,
            pov::PovPlugin,
            torches::TorchPlugin,
            chest_lid::ChestLidPlugin,
            glowcaps::GlowcapPlugin,
            hud::HudPlugin,
            survival::SurvivalPlugin,
            inventory::InventoryPlugin,
        ))
        .add_plugins((
            creatures::CreaturesPlugin,
            places::PlacesPlugin,
            progress::ProgressPlugin,
            station::StationPlugin,
            airship::AirshipPlugin,
            settings::SettingsPlugin,
            sound::SoundPlugin,
            bench::BenchPlugin,
            npc::NpcPlugin,
            avatar::AvatarPlugin,
            combat::CombatPlugin,
            dungeon::DungeonPlugin,
        ))
        .add_plugins((
            delve::DelvePlugin,
            cast::CastPlugin,
            lines::LinesPlugin,
            encounter::EncounterPlugin,
            cutscene::CutscenePlugin,
            story::StoryPlugin,
            map::MapPlugin,
            farming::FarmingPlugin,
            book::BookPlugin,
            journey::JourneyPlugin,
            guide::GuidePlugin,
            shrine::ShrinePlugin,
            pickups::PickupsPlugin,
        ))
        .run();
    menu::relaunch_to_title();
}
