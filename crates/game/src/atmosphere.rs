//! Sky color, sun and fog. Phase 4 replaces this with the full day-night
//! cycle, biome- and altitude-tinted fog and the dynamic skybox.

use bevy::prelude::*;

/// Swamp haze: the colour the starting area's fog and sky fade towards.
pub const SWAMP_FOG: Color = Color::srgb(0.46, 0.53, 0.47);

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(SWAMP_FOG))
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.75, 0.85, 0.8),
                brightness: 400.0,
                ..default()
            })
            .add_systems(Startup, spawn_sun);
    }
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 9_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::new(-0.4, -1.0, -0.6), Vec3::Y),
    ));
}

/// Distance fog for the player camera, added by the camera plugin.
pub fn swamp_fog() -> DistanceFog {
    DistanceFog {
        color: SWAMP_FOG,
        // Far enough to see the loaded world; phase 4 ties this to biome and weather.
        falloff: FogFalloff::Linear { start: 120.0, end: 380.0 },
        ..default()
    }
}
