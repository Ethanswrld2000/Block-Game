//! Game client entry point. Each system lives in its own plugin.

mod atmosphere;
mod autoshot;
mod blocks;
mod camera;
mod debug;
mod interact;
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
            atmosphere::AtmospherePlugin,
            camera::FlyCameraPlugin,
            world::WorldPlugin,
            interact::InteractPlugin,
            debug::DebugOverlayPlugin,
            autoshot::AutoShotPlugin,
        ))
        .run();
}
