//! Something made at a bench comes together: the parts that went into it
//! lift and drift towards where it will lie, sparkling, while a holographic
//! glint (`assets/shaders/glint.wesl`) flashes across them, a little
//! stronger each flash; then they pop into the thing made, which swells in
//! with the glint fading off it.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use super::board::{AtBench, BoardModel, MadeModel, PartOf};
use super::particles::{Burst, BurstKind};
use crate::sound::PlaySound;

pub type GlintMaterial = ExtendedMaterial<StandardMaterial, GlintExtension>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GlintExtension {
    /// x: strength 0..1, y: time, zw: the sprite sheet's size in pixels.
    #[uniform(100)]
    pub glint: Vec4,
}

impl MaterialExtension for GlintExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/glint.wesl".into()
    }
}

/// Seconds the parts draw together, and the made thing takes to swell in.
const GATHER: f32 = 1.5;
const APPEAR: f32 = 0.55;
/// Sparkles while they gather, a second.
const SPARKLE_EVERY: f32 = 0.12;

/// A craft coming together.
#[derive(Debug, Clone)]
pub(super) struct Forge {
    /// Where the made thing will lie (world).
    pub to: Vec3,
    /// The models (slot, which of the stack) of what went into it.
    pub used: Vec<(String, usize)>,
    pub t: f32,
    /// The parts have popped; the made thing is swelling in.
    pub popped: bool,
    sparkle: f32,
}

impl Forge {
    pub fn new(to: Vec3, used: Vec<(String, usize)>) -> Self {
        Self { to, used, t: 0.0, popped: false, sparkle: 0.0 }
    }
}

/// A model wearing the glint: its own look to go back to, and where it started.
#[derive(Component)]
pub(super) struct Glint {
    base: Handle<StandardMaterial>,
    mat: Handle<GlintMaterial>,
    from: Vec3,
    scale: f32,
}

/// The glint strength while gathering: flashes that grow until it's full.
pub(super) fn gather_strength(t: f32) -> f32 {
    let k = (t / GATHER).clamp(0.0, 1.0);
    let flash = 0.5 + 0.5 * (t * 14.0 * (1.0 + k)).sin().abs();
    (k * k * (3.0 - 2.0 * k) * (0.45 + 0.55 * flash)).clamp(0.0, 1.0)
}

/// Overshoot then settle (0..1 -> 0..1).
fn ease_out_back(k: f32) -> f32 {
    let c = 1.9;
    1.0 + (c + 1.0) * (k - 1.0).powi(3) + c * (k - 1.0).powi(2)
}

/// Put the glint on a model.
#[allow(clippy::too_many_arguments)]
fn wear(
    commands: &mut Commands,
    e: Entity,
    base: &MeshMaterial3d<StandardMaterial>,
    tf: &Transform,
    std: &Assets<StandardMaterial>,
    images: &Assets<Image>,
    glints: &mut Assets<GlintMaterial>,
    strength: f32,
) {
    let Some(m) = std.get(&base.0) else { return };
    let size = m.base_color_texture.as_ref().and_then(|i| images.get(i)).map_or(Vec2::splat(256.0), |i| i.size_f32());
    let mat = glints.add(GlintMaterial { base: m.clone(), extension: GlintExtension { glint: Vec4::new(strength, 0.0, size.x, size.y) } });
    commands.entity(e).remove::<MeshMaterial3d<StandardMaterial>>().insert((MeshMaterial3d(mat.clone()), Glint { base: base.0.clone(), mat, from: tf.translation, scale: tf.scale.x }));
}

/// Run the craft coming together.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn forge(
    mut commands: Commands,
    time: Res<Time>,
    mut at: ResMut<AtBench>,
    plain: Query<(Entity, &Transform, &MeshMaterial3d<StandardMaterial>, Has<MadeModel>, Option<&PartOf>), (With<BoardModel>, Without<Glint>)>,
    mut glinting: Query<(Entity, &mut Transform, &Glint)>,
    std: Res<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    mut glints: ResMut<Assets<GlintMaterial>>,
    mut sound: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
    mut hold: Local<Option<Option<f32>>>,
) {
    let Some(mut f) = at.forge.take().filter(|_| at.station.is_some()) else {
        at.forge = None;
        // Left mid-craft: whatever still glints goes back to its own look.
        for (e, _, g) in &glinting {
            commands.entity(e).remove::<(Glint, MeshMaterial3d<GlintMaterial>)>().insert(MeshMaterial3d(g.base.clone()));
        }
        return;
    };
    let dt = time.delta_secs().min(0.1);
    let now = time.elapsed_secs();
    if f.t == 0.0 && !f.popped {
        // Everything on the board that goes into it takes the glint.
        for (e, tf, base, _, part) in &plain {
            if part.is_some_and(|p| f.used.contains(&p.0)) {
                wear(&mut commands, e, base, tf, &std, &images, &mut glints, 0.0);
            }
        }
        sound.write(PlaySound::new("craft_shimmer").gain(1.0));
    }
    f.t += dt;
    // Screenshots: `BLOCKGAME_FORGE=0.8` holds the gathering there.
    if let Some(hold) = *hold.get_or_insert_with(|| std::env::var("BLOCKGAME_FORGE").ok().and_then(|v| v.parse().ok()))
        && !f.popped
    {
        f.t = f.t.min(hold);
    }
    if !f.popped {
        let k = (f.t / GATHER).clamp(0.0, 1.0);
        let s = gather_strength(f.t);
        // Up and in, circling the middle, closing faster at the end.
        let pull = k * k * (3.0 - 2.0 * k);
        let mid = f.to + Vec3::Y * (0.08 + 0.1 * (k * std::f32::consts::PI).sin());
        let n = glinting.iter().count().max(1) as f32;
        for (i, (_, mut tf, g)) in glinting.iter_mut().enumerate() {
            let a = now * (2.0 + 4.0 * k) + i as f32 / n * std::f32::consts::TAU;
            let orbit = Vec3::new(a.cos(), 0.0, a.sin()) * 0.05 * (1.0 - pull) * pull.min(0.5) * 2.0;
            tf.translation = g.from.lerp(mid, pull) + orbit;
            tf.scale = Vec3::splat(g.scale * (1.0 - 0.35 * pull * pull));
            tf.rotate_y(dt * 6.0 * k);
            if let Some(mut m) = glints.get_mut(&g.mat) {
                m.extension.glint.x = s;
                m.extension.glint.y = now;
            }
        }
        f.sparkle -= dt;
        if f.sparkle <= 0.0 {
            f.sparkle = SPARKLE_EVERY;
            bursts.write(Burst::new(mid, BurstKind::Sparkle, 2 + (k * 3.0) as u32));
        }
        if f.t >= GATHER {
            for (e, ..) in &glinting {
                commands.entity(e).despawn();
            }
            bursts.write(Burst::new(mid, BurstKind::Pop, 24));
            sound.write(PlaySound::new("craft_pop").gain(1.0));
            f.popped = true;
            f.t = 0.0;
            // Lay the board out again: the made thing appears.
            at.dirty = true;
        }
        at.forge = Some(f);
        return;
    }
    // The made thing swells in, the glint fading off it.
    for (e, tf, base, made, _) in &plain {
        if made {
            wear(&mut commands, e, base, tf, &std, &images, &mut glints, 1.0);
        }
    }
    let k = (f.t / APPEAR).clamp(0.0, 1.0);
    for (e, mut tf, g) in &mut glinting {
        tf.scale = Vec3::splat(g.scale * ease_out_back(k).max(0.02));
        if let Some(mut m) = glints.get_mut(&g.mat) {
            m.extension.glint.x = 1.0 - k;
            m.extension.glint.y = now;
        }
        if k >= 1.0 {
            commands.entity(e).remove::<(Glint, MeshMaterial3d<GlintMaterial>)>().insert(MeshMaterial3d(g.base.clone()));
        }
    }
    at.forge = if k >= 1.0 { None } else { Some(f) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_glint_grows_in_flashes_to_full() {
        assert_eq!(gather_strength(0.0), 0.0);
        // Flashes: it doesn't only rise.
        let s: Vec<f32> = (0..=150).map(|i| gather_strength(i as f32 * GATHER / 150.0)).collect();
        assert!(s.windows(2).any(|w| w[1] < w[0]), "flashes");
        // Stronger late than early.
        let early: f32 = s[..50].iter().sum();
        let late: f32 = s[100..].iter().sum();
        assert!(late > early * 3.0);
        assert!(s.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn the_made_thing_overshoots_then_settles() {
        assert!(ease_out_back(0.0).abs() < 1e-5);
        assert!((ease_out_back(1.0) - 1.0).abs() < 1e-5);
        assert!((0..100).any(|i| ease_out_back(i as f32 / 100.0) > 1.05));
    }
}
