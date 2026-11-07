//! Game client entry point. Each system lives in its own plugin.

mod autoshot;
mod blocks;
mod camera;
mod chunk_mesh;
mod clock;
mod clouds;
mod lod;
mod player;
mod precipitation;
mod debug;
mod hands;
mod hud;
mod interact;
mod items;
mod save;
mod sky;
mod torches;
mod weather;
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
            sky::SkyPlugin,
            clouds::CloudPlugin,
            precipitation::PrecipitationPlugin,
            lod::LodPlugin,
        ))
        .add_plugins((
            items::ItemsPlugin,
            player::PlayerPlugin,
            hands::HandsPlugin,
            torches::TorchPlugin,
            hud::HudPlugin,
        ))
        .run();
}
