//! Lightning bolts you can see. Each strike (`Conditions::strikes`, from
//! `weather`) draws a jagged bolt with forks from the low cloud deck down to
//! the ground, as far off as `Conditions::strike_far` (the thunder in `sound`
//! uses the same distance). It glows while the flash lasts, lights up again
//! when the strike flickers, and is gone when the flash fades.
//!
//! The shape is pure (`bolt_shape`); the mesh is ribbons turned towards the
//! camera, a hot core inside a soft glow, drawn additively with fog off so a
//! bolt shows through the rain. Bloom does the rest.
//! `BLOCKGAME_BOLT=1` (or a distance 0..1) holds a bolt lit for screenshots.

use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::camera::FlyCamera;
use crate::clouds::LAYERS;
use crate::weather::{Conditions, WeatherSet};
use crate::world::Generator;

/// Nearest and furthest a strike lands from the camera, in blocks.
const NEAR: f32 = 70.0;
const FAR: f32 = 750.0;
/// Share of strikes put somewhere in front of you, so you see most of them.
const AHEAD: f32 = 0.65;
/// Colour of the bolt (linear, bright enough for bloom).
const CORE: [f32; 3] = [6.0, 6.5, 9.0];

pub struct LightningPlugin;

impl Plugin for LightningPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::menu::GameStartup, setup)
            .add_systems(crate::menu::GameUpdate, (strike, glow).chain().after(WeatherSet));
    }
}

#[derive(Resource)]
struct BoltMaterial(Handle<StandardMaterial>);

#[derive(Component)]
struct Bolt;

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        fog_enabled: false,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(BoltMaterial(mat));
}

/// One stroke of a bolt: a line of points, how wide its core is (blocks) and
/// how bright it is (the main channel 1, forks less).
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub points: Vec<Vec3>,
    pub width: f32,
    pub bright: f32,
}

/// Small xorshift random numbers, 0..1.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn signed(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

/// A bolt from `top` down to `bottom`: a main channel that zigzags in short
/// steps, wandering a little off the straight line, with forks that branch off
/// downwards and outwards (and now and then fork again). `width` is the main
/// channel's core width in blocks.
pub fn bolt_shape(seed: u64, top: Vec3, bottom: Vec3, width: f32) -> Vec<Stroke> {
    let mut rng = Rng(seed | 1);
    let drop = top - bottom;
    let length = drop.length().max(1.0);
    let steps = ((length / 7.0) as usize).clamp(6, 80);
    let mut main = Vec::with_capacity(steps + 1);
    let mut wander = Vec3::ZERO;
    let mut forks = Vec::new();
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let straight = top.lerp(bottom, t);
        if i > 0 && i < steps {
            wander += Vec3::new(rng.signed(), 0.0, rng.signed()) * 3.5;
            wander *= 0.85;
        }
        // Pinned at both ends; free to wander in between.
        let p = straight + wander * (1.0 - (2.0 * t - 1.0).powi(8)) + Vec3::new(0.0, rng.signed() * 1.5, 0.0) * f32::from(i > 0 && i < steps);
        main.push(p);
        if i > 1 && t < 0.75 && rng.next() < 0.22 {
            forks.push((p, length * (0.15 + 0.25 * rng.next()), 0.5, 0.55));
        }
    }
    let mut strokes = vec![Stroke { points: main, width, bright: 1.0 }];
    while let Some((start, len, w, bright)) = forks.pop() {
        let mut dir = Vec3::new(rng.signed(), -(0.8 + rng.next()), rng.signed()).normalize();
        let n = ((len / 5.0) as usize).clamp(3, 30);
        let mut p = start;
        let mut points = vec![p];
        for _ in 0..n {
            dir = (dir + Vec3::new(rng.signed(), rng.signed() * 0.3, rng.signed()) * 0.45).normalize();
            dir.y = dir.y.min(-0.3);
            p += dir.normalize() * (len / n as f32);
            if p.y < bottom.y {
                break;
            }
            points.push(p);
            if bright > 0.3 && rng.next() < 0.12 {
                forks.push((p, len * 0.5, w * 0.6, bright * 0.6));
            }
        }
        if points.len() > 1 {
            strokes.push(Stroke { points, width: width * w, bright });
        }
    }
    strokes
}

/// Ribbons along each stroke, turned to face `eye`: a narrow hot core and a
/// wide soft glow, bright in the middle and fading to nothing at the edges.
fn bolt_mesh(strokes: &[Stroke], eye: Vec3) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for s in strokes {
        for (half, bright) in [(s.width * 0.5, 1.0), (s.width * 3.0, 0.22)] {
            let base = positions.len() as u32;
            let n = s.points.len();
            for i in 0..n {
                let p = s.points[i];
                let along = (s.points[(i + 1).min(n - 1)] - s.points[i.saturating_sub(1)]).normalize_or(Vec3::Y);
                let side = along.cross(eye - p).normalize_or(Vec3::X) * half;
                // The forks thin out towards their tips.
                let tip = if s.bright < 1.0 { 1.0 - 0.6 * i as f32 / n as f32 } else { 1.0 };
                let c = bright * s.bright * tip;
                positions.extend([(p - side * tip).to_array(), p.to_array(), (p + side * tip).to_array()]);
                colors.extend([[0.0; 4], [c, c, c, 1.0], [0.0; 4]]);
            }
            for i in 0..n as u32 - 1 {
                let (a, b) = (base + i * 3, base + (i + 1) * 3);
                for (x, y) in [(0, 1), (1, 2)] {
                    indices.extend([a + x, b + x, a + y, a + y, b + x, b + y]);
                }
            }
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// A new strike: clear the old bolt and draw a new one.
#[allow(clippy::too_many_arguments)]
fn strike(
    mut commands: Commands,
    conditions: Res<Conditions>,
    cam: Query<&Transform, With<FlyCamera>>,
    generator: Option<Res<Generator>>,
    mut meshes: ResMut<Assets<Mesh>>,
    material: Option<Res<BoltMaterial>>,
    bolts: Query<Entity, With<Bolt>>,
    mut seen: Local<u32>,
) {
    if conditions.strikes == *seen {
        return;
    }
    let (Ok(tf), Some(material)) = (cam.single(), material) else { return };
    *seen = conditions.strikes;
    for e in &bolts {
        commands.entity(e).despawn();
    }
    let eye = tf.translation;
    let seed = (conditions.strikes as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ eye.x.to_bits() as u64 ^ ((eye.z.to_bits() as u64) << 32);
    let mut rng = Rng(seed | 1);
    // Where it lands: mostly somewhere ahead of you.
    let forward = (tf.forward().as_vec3() * Vec3::new(1.0, 0.0, 1.0)).normalize_or(Vec3::NEG_Z);
    let base = forward.z.atan2(forward.x);
    let angle = if crate::weather::held_bolt().is_some() {
        base
    } else if rng.next() < AHEAD { base + rng.signed() * 0.9 } else { rng.next() * std::f32::consts::TAU };
    let dist = NEAR + (FAR - NEAR) * conditions.strike_far.clamp(0.0, 1.0);
    let at = eye + Vec3::new(angle.cos(), 0.0, angle.sin()) * dist;
    let ground = generator
        .map(|g| {
            let col = g.0.column(at.x.floor() as i32, at.z.floor() as i32);
            if col.land == worldgen::Land::Void { eye.y - 300.0 } else { col.height as f32 + 1.0 }
        })
        .unwrap_or(eye.y - 40.0);
    // From the low cloud deck, or well above high ground.
    let top_y = LAYERS[0].y.max(ground + 90.0);
    let top = Vec3::new(at.x + rng.signed() * 25.0, top_y, at.z + rng.signed() * 25.0);
    let bottom = Vec3::new(at.x, ground, at.z);
    // Wider far away, so it stays a few pixels across.
    let width = 0.5 + dist * 0.0025;
    let strokes = bolt_shape(seed, top, bottom, width);
    debug!("lightning strikes {dist:.0} blocks off at {bottom}");
    commands.spawn((
        Bolt,
        Mesh3d(meshes.add(bolt_mesh(&strokes, eye))),
        MeshMaterial3d(material.0.clone()),
        Transform::IDENTITY,
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

/// The bolt glows with the flash; gone once the flash has faded.
fn glow(
    mut commands: Commands,
    conditions: Res<Conditions>,
    material: Option<Res<BoltMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    bolts: Query<Entity, With<Bolt>>,
) {
    let Some(material) = material else { return };
    let f = conditions.flash;
    if let Some(mut m) = materials.get_mut(&material.0) {
        let k = f * f;
        let c = Color::linear_rgb(CORE[0] * k, CORE[1] * k, CORE[2] * k);
        if m.base_color != c {
            m.base_color = c;
        }
    }
    if f <= 0.0 {
        for e in &bolts {
            commands.entity(e).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bolt_runs_from_cloud_to_ground_with_forks() {
        let top = Vec3::new(0.0, 200.0, 0.0);
        let bottom = Vec3::new(10.0, 64.0, 5.0);
        for seed in 1..20 {
            let s = bolt_shape(seed, top, bottom, 1.0);
            let main = &s[0].points;
            assert_eq!(main.first(), Some(&top));
            assert!(main.last().unwrap().distance(bottom) < 0.01);
            assert!(s.iter().all(|k| k.points.iter().all(|p| p.y >= bottom.y - 0.01 && p.y <= top.y + 2.0)));
        }
        assert!((1..20).any(|seed| bolt_shape(seed, top, bottom, 1.0).len() > 1));
    }
}
