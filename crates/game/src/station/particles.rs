//! Pixel particles at the stations: flat pixel-art sprites that always face
//! the camera. Bubbles rise in the cooking pot and the cauldron (more in the
//! cauldron), swell at the surface and pop, throwing drops; every blow,
//! stroke, stir and turn at a bench throws a `Burst` (chips, sparks,
//! sawdust, smoke, splashes, sparkles), and a finished craft pops.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::{models, Station, StationKind, Urns};
use crate::camera::FlyCamera;

/// What a burst throws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BurstKind {
    /// Stone chips (hammerstones, the hammer on the tool bench).
    Chips,
    /// Glowing sparks (the anvil, the grindstone).
    Sparks,
    /// Pale sawdust drifting down.
    Sawdust,
    /// Bits of whatever the knife cut.
    Bits,
    /// Grey smoke puffs rising (the fan).
    Smoke,
    /// Drops of broth flying up.
    Splash,
    /// Drops of the cauldron's glowing brew.
    BrewSplash,
    /// Little stars (the lens, parts drawing together).
    Sparkle,
    /// The pop of something made: a ring of stars and a puff.
    Pop,
    /// A ghost's pale blue wisp, drifting up and fading.
    Wisp,
}

/// Throw some particles at a point in the world.
#[derive(Message, Debug, Clone, Copy)]
pub struct Burst {
    pub at: Vec3,
    pub kind: BurstKind,
    pub n: u32,
}

impl Burst {
    pub fn new(at: Vec3, kind: BurstKind, n: u32) -> Self {
        Self { at, kind, n }
    }
}

/// The pixel sprites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Sprite {
    Bubble,
    Pop,
    Dot,
    Star,
    Puff,
}

/// Sprite pictures as rows of pixels: '#' solid, '+' faint, '.' clear.
fn pixels(s: Sprite) -> &'static [&'static str] {
    match s {
        Sprite::Bubble => &["..####..", ".#++++#.", "#+##+++#", "#+#++++#", "#++++++#", "#++++++#", ".#++++#.", "..####.."],
        Sprite::Pop => &["#..##..#", "........", "........", "#......#", "#......#", "........", "........", "#..##..#"],
        Sprite::Dot => &["##", "##"],
        Sprite::Star => &["..#..", "..#..", "#####", "..#..", "..#.."],
        Sprite::Puff => &[".####.", "######", "######", "######", "######", ".####."],
    }
}

fn sprite_image(s: Sprite) -> Image {
    let rows = pixels(s);
    let (w, h) = (rows[0].len() as u32, rows.len() as u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for r in rows {
        for c in r.chars() {
            let a = match c {
                '#' => 255,
                '+' => 70,
                _ => 0,
            };
            data.extend_from_slice(&[255, 255, 255, a]);
        }
    }
    // Default sampler: nearest, like all the game's pixel art.
    Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
}

/// One quad mesh, the sprite pictures, and a material per sprite and colour.
#[derive(Resource)]
pub(super) struct Pixels {
    quad: Handle<Mesh>,
    images: HashMap<Sprite, Handle<Image>>,
    materials: HashMap<(Sprite, [u16; 4]), Handle<StandardMaterial>>,
}

impl Pixels {
    fn material(&mut self, assets: &mut Assets<StandardMaterial>, s: Sprite, c: LinearRgba) -> Handle<StandardMaterial> {
        let key = (s, [c.red, c.green, c.blue, c.alpha].map(|v| (v * 100.0) as u16));
        let image = self.images[&s].clone();
        self.materials
            .entry(key)
            .or_insert_with(|| {
                assets.add(StandardMaterial { base_color: c.into(), base_color_texture: Some(image), unlit: true, alpha_mode: AlphaMode::Blend, double_sided: true, cull_mode: None, ..default() })
            })
            .clone()
    }
}

pub(super) fn setup_pixels(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>) {
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    let images = [Sprite::Bubble, Sprite::Pop, Sprite::Dot, Sprite::Star, Sprite::Puff].into_iter().map(|s| (s, images.add(sprite_image(s)))).collect();
    commands.insert_resource(Pixels { quad, images, materials: HashMap::new() });
}

/// How a particle moves.
#[derive(Debug, Clone, Copy)]
enum Motion {
    /// Flies, falls with `grav`, slows with `drag`.
    Free { grav: f32, drag: f32 },
    /// A bubble rising to the surface at this height; then it pops.
    Bubble { surface: f32, cauldron: bool },
}

#[derive(Component)]
pub(super) struct Particle {
    vel: Vec3,
    age: f32,
    life: f32,
    size: (f32, f32),
    motion: Motion,
}

/// A small fast random number in 0..1.
fn rnd(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

fn spread(seed: &mut u32, k: f32) -> Vec3 {
    Vec3::new(rnd(seed) * 2.0 - 1.0, rnd(seed) * 2.0 - 1.0, rnd(seed) * 2.0 - 1.0) * k
}

/// The look and flight of one particle of a burst.
fn burst_particle(kind: BurstKind, seed: &mut u32) -> (Sprite, LinearRgba, Vec3, f32, (f32, f32), Motion) {
    let up = |seed: &mut u32, k: f32| Vec3::new(rnd(seed) * 2.0 - 1.0, 0.6 + rnd(seed), rnd(seed) * 2.0 - 1.0) * k;
    let free = |grav, drag| Motion::Free { grav, drag };
    match kind {
        BurstKind::Chips => {
            let g = 0.35 + rnd(seed) * 0.2;
            (Sprite::Dot, LinearRgba::rgb(g, g, g * 1.05), up(seed, 1.1), 0.6, (0.012, 0.01), free(6.0, 1.0))
        }
        BurstKind::Sparks => (Sprite::Dot, LinearRgba::rgb(4.0, 1.8, 0.4), up(seed, 1.6), 0.45, (0.01, 0.004), free(5.0, 1.5)),
        BurstKind::Sawdust => (Sprite::Dot, LinearRgba::rgb(0.75, 0.6, 0.38), up(seed, 0.5), 1.1, (0.01, 0.008), free(1.2, 3.0)),
        BurstKind::Bits => (Sprite::Dot, LinearRgba::rgb(0.7, 0.32, 0.25), up(seed, 0.7), 0.55, (0.012, 0.01), free(5.0, 1.0)),
        BurstKind::Smoke => {
            let g = 0.5 + rnd(seed) * 0.15;
            (Sprite::Puff, LinearRgba::new(g, g, g, 0.55), Vec3::Y * 0.35 + spread(seed, 0.15), 1.4, (0.03, 0.09), free(-0.1, 1.5))
        }
        BurstKind::Splash => (Sprite::Dot, LinearRgba::new(0.75, 0.52, 0.22, 0.85), up(seed, 0.8), 0.5, (0.01, 0.008), free(4.5, 0.5)),
        BurstKind::BrewSplash => (Sprite::Dot, LinearRgba::new(0.5, 2.0, 0.7, 0.9), up(seed, 0.8), 0.5, (0.01, 0.008), free(4.5, 0.5)),
        BurstKind::Sparkle => (Sprite::Star, LinearRgba::rgb(1.6, 1.5, 2.4), spread(seed, 0.25) + Vec3::Y * 0.1, 0.8, (0.025, 0.0), free(-0.2, 2.0)),
        BurstKind::Wisp => {
            let c = LinearRgba::new(0.35 + rnd(seed) * 0.2, 0.75, 2.2, 0.45);
            (Sprite::Puff, c, Vec3::Y * (0.35 + rnd(seed) * 0.3) + spread(seed, 0.12), 1.2, (0.025, 0.07), free(-0.15, 1.2))
        }
        BurstKind::Pop => {
            // A flat ring outward, a little up.
            let a = rnd(seed) * std::f32::consts::TAU;
            let v = Vec3::new(a.cos(), 0.4 + rnd(seed) * 0.5, a.sin()) * (0.8 + rnd(seed) * 0.6);
            let c = if rnd(seed) < 0.5 { LinearRgba::rgb(3.0, 2.4, 0.8) } else { LinearRgba::rgb(1.8, 1.4, 3.0) };
            (Sprite::Star, c, v, 0.7, (0.035, 0.0), free(1.0, 3.0))
        }
    }
}

/// Throw bursts.
pub(super) fn spawn_bursts(mut commands: Commands, mut bursts: MessageReader<Burst>, pixels: Option<ResMut<Pixels>>, mut materials: ResMut<Assets<StandardMaterial>>, mut seed: Local<u32>) {
    let Some(mut pixels) = pixels else {
        bursts.clear();
        return;
    };
    if *seed == 0 {
        *seed = 0x2545_f491;
    }
    for b in bursts.read() {
        for _ in 0..b.n {
            let (sprite, color, vel, life, size, motion) = burst_particle(b.kind, &mut seed);
            let mat = pixels.material(&mut materials, sprite, color);
            commands.spawn((
                Particle { vel, age: 0.0, life: life * (0.75 + rnd(&mut seed) * 0.5), size, motion },
                Mesh3d(pixels.quad.clone()),
                MeshMaterial3d(mat),
                Transform::from_translation(b.at + spread(&mut seed, 0.03)).with_scale(Vec3::splat(size.0)),
                NotShadowCaster,
            ));
        }
        // The pop also leaves a puff where the thing appears.
        if b.kind == BurstKind::Pop {
            let mat = pixels.material(&mut materials, Sprite::Puff, LinearRgba::new(1.0, 0.95, 0.85, 0.7));
            for _ in 0..6 {
                commands.spawn((
                    Particle { vel: spread(&mut seed, 0.3), age: 0.0, life: 0.45, size: (0.05, 0.14), motion: Motion::Free { grav: 0.0, drag: 4.0 } },
                    Mesh3d(pixels.quad.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_translation(b.at).with_scale(Vec3::splat(0.05)),
                    NotShadowCaster,
                ));
            }
        }
    }
}

/// Bubbles a second.
const POT_RATE: f32 = 2.5;
const CAULDRON_RATE: f32 = 7.0;
/// Only pots this near the camera bubble.
const NEAR: f32 = 24.0;
/// At most this many particles at once.
const MAX: usize = 400;

/// Start new bubbles in pots near the camera.
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_bubbles(
    mut commands: Commands,
    time: Res<Time>,
    pixels: Option<ResMut<Pixels>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    urns: Res<Urns>,
    stations: Query<&Station>,
    cam: Single<&Transform, With<FlyCamera>>,
    live: Query<(), With<Particle>>,
    mut seed: Local<u32>,
) {
    let Some(mut pixels) = pixels else { return };
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
        let Some((mid, half)) = models::liquid(st.kind, &has) else { continue };
        let rate = if cauldron { CAULDRON_RATE } else { POT_RATE };
        let color = if cauldron { LinearRgba::new(0.7, 2.2, 0.9, 0.95) } else { LinearRgba::new(1.0, 0.9, 0.7, 0.9) };
        let mat = pixels.material(&mut materials, Sprite::Bubble, color);
        // Several a frame when the frame is long.
        let mut n = rate * dt;
        while n > 0.0 && room > 0 {
            if rnd(&mut seed) >= n {
                break;
            }
            n -= 1.0;
            room -= 1;
            let off = Vec3::new((rnd(&mut seed) * 2.0 - 1.0) * half * 0.85, -0.04 - rnd(&mut seed) * 0.1, (rnd(&mut seed) * 2.0 - 1.0) * half * 0.85);
            let rot = if st.turned { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
            let at = st.pos + rot * (mid + off);
            let speed = if cauldron { 0.22 } else { 0.16 };
            commands.spawn((
                Particle { vel: Vec3::Y * speed, age: 0.0, life: 3.0, size: (0.016, 0.034), motion: Motion::Bubble { surface: st.pos.y + mid.y, cauldron } },
                Mesh3d(pixels.quad.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(at).with_scale(Vec3::splat(0.016)),
                NotShadowCaster,
            ));
        }
    }
}

/// Seconds a bubble shows its burst ring before it's gone.
const POP_SHOW: f32 = 0.09;

/// Particles fly, fall, grow or shrink, and face the camera; bubbles pop.
#[allow(clippy::type_complexity)]
pub(super) fn move_particles(
    mut commands: Commands,
    time: Res<Time>,
    pixels: Option<ResMut<Pixels>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cam: Single<&Transform, (With<FlyCamera>, Without<Particle>)>,
    mut parts: Query<(Entity, &mut Particle, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    mut bursts: MessageWriter<Burst>,
) {
    let Some(mut pixels) = pixels else { return };
    let dt = time.delta_secs().min(0.1);
    let face = cam.rotation;
    for (e, mut p, mut tf, mut mat) in &mut parts {
        p.age += dt;
        tf.rotation = face;
        match p.motion {
            Motion::Free { grav, drag } => {
                p.vel.y -= grav * dt;
                p.vel *= (-drag * dt).exp();
                tf.translation += p.vel * dt;
                let k = (p.age / p.life).min(1.0);
                tf.scale = Vec3::splat(p.size.0 + (p.size.1 - p.size.0) * k);
                if p.age >= p.life {
                    commands.entity(e).despawn();
                }
            }
            Motion::Bubble { surface, cauldron } => {
                if p.vel.y > 0.0 {
                    // Rising, growing as it goes, until it meets the surface.
                    tf.translation.y += p.vel.y * dt;
                    tf.scale = Vec3::splat((tf.scale.x + dt * 0.02).min(p.size.1));
                    if tf.translation.y >= surface {
                        tf.translation.y = surface + 0.01;
                        p.vel = Vec3::ZERO;
                        p.age = 0.0;
                        let color = if cauldron { LinearRgba::new(0.7, 2.2, 0.9, 0.95) } else { LinearRgba::new(1.0, 0.9, 0.7, 0.9) };
                        mat.0 = pixels.material(&mut materials, Sprite::Pop, color);
                        tf.scale = Vec3::splat(p.size.1 * 1.4);
                        let kind = if cauldron { BurstKind::BrewSplash } else { BurstKind::Splash };
                        bursts.write(Burst::new(tf.translation, kind, if cauldron { 3 } else { 1 }));
                    } else if p.age > p.life {
                        commands.entity(e).despawn();
                    }
                } else if p.age >= POP_SHOW {
                    commands.entity(e).despawn();
                }
            }
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

    #[test]
    fn sprite_pictures_are_square_grids() {
        for s in [Sprite::Bubble, Sprite::Pop, Sprite::Dot, Sprite::Star, Sprite::Puff] {
            let rows = pixels(s);
            assert!(rows.iter().all(|r| r.len() == rows.len()), "{s:?}");
        }
    }
}
