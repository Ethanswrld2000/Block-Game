//! Bubbles in the cooking pot and the cauldron: they rise through the
//! see-through broth or brew, swell at the surface and pop, throwing a few
//! drops up out of the pot. The cauldron boils harder.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::{Station, StationKind, Urns, models};
use crate::camera::FlyCamera;

/// Bubbles a second.
const POT_RATE: f32 = 2.5;
const CAULDRON_RATE: f32 = 7.0;
/// Only pots this near the camera bubble.
const NEAR: f32 = 24.0;
/// At most this many bubbles and drops at once.
const MAX: usize = 160;
/// Seconds a bubble swells on the surface before it pops.
const SWELL: f32 = 0.22;

#[derive(Component)]
pub(super) struct Bubble {
    /// The surface height (world).
    surface: f32,
    age: f32,
    /// Rising (`false`) or swelling on the surface.
    up: bool,
    /// Drops thrown by the pop.
    drops: u32,
    cauldron: bool,
}

#[derive(Component)]
pub(super) struct Splash {
    vel: Vec3,
    surface: f32,
    age: f32,
}

/// The mesh and the two looks, made once.
#[derive(Resource)]
pub(super) struct BubbleLook {
    mesh: Handle<Mesh>,
    pot: Handle<StandardMaterial>,
    cauldron: Handle<StandardMaterial>,
}

pub(super) fn setup_bubbles(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(
        Sphere::new(1.0)
            .mesh()
            .ico(1)
            .unwrap_or_else(|_| Sphere::new(1.0).mesh().uv(8, 6)),
    );
    let pot = materials.add(StandardMaterial {
        base_color: Color::srgba(0.92, 0.82, 0.62, 0.55),
        perceptual_roughness: 0.1,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let cauldron = materials.add(StandardMaterial {
        base_color: Color::srgba(0.6, 1.0, 0.7, 0.6),
        emissive: LinearRgba::rgb(0.3, 1.4, 0.5),
        perceptual_roughness: 0.1,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    commands.insert_resource(BubbleLook {
        mesh,
        pot,
        cauldron,
    });
}

/// A small fast random number in 0..1.
fn rnd(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

/// Start new bubbles in pots near the camera.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn spawn_bubbles(
    mut commands: Commands,
    time: Res<Time>,
    look: Option<Res<BubbleLook>>,
    urns: Res<Urns>,
    stations: Query<&Station>,
    cam: Single<&Transform, With<FlyCamera>>,
    live: Query<(), Or<(With<Bubble>, With<Splash>)>>,
    mut seed: Local<u32>,
) {
    let Some(look) = look else { return };
    if *seed == 0 {
        *seed = 0x9e37_79b9;
    }
    let dt = time.delta_secs().min(0.1);
    let mut room = MAX.saturating_sub(live.iter().count());
    let has = |s: &str| urns.has(s);
    for st in &stations {
        let cauldron = match st.kind {
            StationKind::CookingPot => false,
            StationKind::Cauldron => true,
            _ => continue,
        };
        if st.pos.distance(cam.translation) > NEAR {
            continue;
        }
        let Some((mid, half)) = models::liquid(st.kind, &has) else {
            continue;
        };
        let rate = if cauldron { CAULDRON_RATE } else { POT_RATE };
        // Several a frame when the frame is long.
        let mut n = rate * dt;
        while n > 0.0 && room > 0 {
            if rnd(&mut seed) >= n {
                break;
            }
            n -= 1.0;
            room -= 1;
            let off = Vec3::new(
                (rnd(&mut seed) * 2.0 - 1.0) * half * 0.85,
                -0.04 - rnd(&mut seed) * 0.1,
                (rnd(&mut seed) * 2.0 - 1.0) * half * 0.85,
            );
            let rot = if st.turned {
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
            } else {
                Quat::IDENTITY
            };
            let at = st.pos + rot * (mid + off);
            let drops = if cauldron {
                2 + (rnd(&mut seed) * 2.0) as u32
            } else {
                (rnd(&mut seed) * 2.5) as u32
            };
            commands.spawn((
                Bubble {
                    surface: st.pos.y + mid.y,
                    age: 0.0,
                    up: false,
                    drops,
                    cauldron,
                },
                Mesh3d(look.mesh.clone()),
                MeshMaterial3d(if cauldron {
                    look.cauldron.clone()
                } else {
                    look.pot.clone()
                }),
                Transform::from_translation(at).with_scale(Vec3::splat(0.008)),
                NotShadowCaster,
            ));
        }
    }
}

/// Bubbles rise, swell and pop; drops fly up and fall back.
pub(super) fn move_bubbles(
    mut commands: Commands,
    time: Res<Time>,
    look: Option<Res<BubbleLook>>,
    mut bubbles: Query<(Entity, &mut Bubble, &mut Transform), Without<Splash>>,
    mut drops: Query<(Entity, &mut Splash, &mut Transform), Without<Bubble>>,
    mut seed: Local<u32>,
) {
    let Some(look) = look else { return };
    if *seed == 0 {
        *seed = 0x2545_f491;
    }
    let dt = time.delta_secs().min(0.1);
    for (e, mut b, mut tf) in &mut bubbles {
        b.age += dt;
        if !b.up {
            // Rising, growing as it goes.
            tf.translation.y += dt * if b.cauldron { 0.22 } else { 0.16 };
            let r = (tf.scale.x + dt * 0.03).min(0.02);
            tf.scale = Vec3::splat(r);
            if tf.translation.y >= b.surface {
                tf.translation.y = b.surface;
                b.up = true;
                b.age = 0.0;
            }
            if b.age > 3.0 {
                commands.entity(e).despawn();
            }
            continue;
        }
        // A dome on the surface, swelling, then pop.
        let k = (b.age / SWELL).min(1.0);
        tf.scale = Vec3::new(0.02 + 0.016 * k, 0.012 + 0.014 * k, 0.02 + 0.016 * k);
        if b.age >= SWELL {
            commands.entity(e).despawn();
            for _ in 0..b.drops {
                let a = rnd(&mut seed) * std::f32::consts::TAU;
                let out = 0.1 + rnd(&mut seed) * 0.25;
                let vel = Vec3::new(a.cos() * out, 0.7 + rnd(&mut seed) * 0.6, a.sin() * out);
                commands.spawn((
                    Splash {
                        vel,
                        surface: b.surface,
                        age: 0.0,
                    },
                    Mesh3d(look.mesh.clone()),
                    MeshMaterial3d(if b.cauldron {
                        look.cauldron.clone()
                    } else {
                        look.pot.clone()
                    }),
                    Transform::from_translation(tf.translation + Vec3::Y * 0.01)
                        .with_scale(Vec3::splat(0.007)),
                    NotShadowCaster,
                ));
            }
        }
    }
    for (e, mut d, mut tf) in &mut drops {
        d.age += dt;
        d.vel.y -= 4.5 * dt;
        tf.translation += d.vel * dt;
        if (tf.translation.y < d.surface && d.vel.y < 0.0) || d.age > 1.5 {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_numbers_stay_in_range() {
        let mut s = 0x9e37_79b9;
        for _ in 0..1000 {
            let r = rnd(&mut s);
            assert!((0.0..1.0).contains(&r));
        }
    }
}
