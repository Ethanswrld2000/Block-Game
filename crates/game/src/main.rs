//! Game client entry point. Each system lives in its own plugin.

mod autoshot;
mod bench;
mod blocks;
mod camera;
mod chunk_mesh;
mod clock;
mod clouds;
mod creatures;
mod lod;
mod places;
mod player;
mod precipitation;
mod progress;
mod debug;
mod hands;
mod hud;
mod interact;
mod inventory;
mod item_art;
mod items;
mod save;
mod settings;
mod sky;
mod sound;
mod theme;
mod survival;
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
        .add_plugins(theme::ThemePlugin)
        .add_plugins((
            blocks::BlocksPlugin,
            camera::FlyCameraPlugin,
            save::SavePlugin,
            world::WorldPlugin,
            interact::InteractPlugin,
            debug::DebugOverlayPlugin,
            autoshot::AutoShotPlugin,
        ))
        .add_plugins((
            clock::ClockPlugin,
            weather::WeatherPlugin,
            wind::WindPlugin,
            sky::SkyPlugin,
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
        .add_plugins((creatures::CreaturesPlugin, places::PlacesPlugin, progress::ProgressPlugin, settings::SettingsPlugin, sound::SoundPlugin, bench::BenchPlugin))
        .run();
}
