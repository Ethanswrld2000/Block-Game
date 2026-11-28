//! Airships that only live in a cutscene (`Step::Ship`): yours, with you at the
//! helm, or a court warship. They fly where the steps send them (nose along the
//! way), fire bolts at each other, and burn once hit (fire and smoke puffs
//! trailing). Their positions go into `Places` as `ship:<name>`, so marks,
//! camera targets and orbits can follow them. All of them go when the cutscene ends.

use std::collections::HashMap;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::Places;
use super::script::{ShipLook, ship_key};
use crate::sound::PlaySound;

/// What the cutscene asks of its airships.
#[derive(Message, Debug, Clone)]
pub enum PropCmd {
    Ship {
        name: String,
        at: Vec3,
        dir: Vec3,
        look: ShipLook,
    },
    To {
        name: String,
        to: Vec3,
        secs: f32,
    },
    Fire {
        from: String,
        to: String,
    },
    Remove(String),
    /// The cutscene is over: every ship, bolt and puff goes.
    Clear,
}

/// Seconds a bolt takes to reach its target.
pub const BOLT_SECS: f32 = 0.7;
/// How high above the keel the middle of a ship is (marks and the camera aim there).
const MIDDLE: f32 = 3.0;
/// Seconds between smoke puffs from a burning ship.
const SMOKE_EVERY: f32 = 0.06;
/// The court warship's size against its model.
const COURT_SCALE: f32 = 2.2;

#[derive(Component)]
pub struct PropShip {
    name: String,
    from: Vec3,
    to: Vec3,
    t: f32,
    secs: f32,
    /// The way the bow points.
    dir: Vec3,
    /// Seconds since it was hit; `None` while whole.
    burning: Option<f32>,
    smoke: f32,
}

#[derive(Component)]
pub struct Bolt {
    from: Vec3,
    target: String,
    t: f32,
}

#[derive(Component)]
pub struct Puff {
    vel: Vec3,
    age: f32,
    life: f32,
    size: f32,
}

pub struct Kit {
    cube: Handle<Mesh>,
    fire: Handle<StandardMaterial>,
    smoke: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    bolt: Handle<StandardMaterial>,
}

fn kit(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>) -> Kit {
    let glow = |c: Color, e: LinearRgba| StandardMaterial {
        base_color: c,
        emissive: e,
        unlit: true,
        ..default()
    };
    Kit {
        cube: meshes.add(Cuboid::from_length(1.0)),
        fire: mats.add(glow(
            Color::srgb(1.0, 0.55, 0.15),
            LinearRgba::rgb(8.0, 3.0, 0.6),
        )),
        smoke: mats.add(StandardMaterial {
            base_color: Color::srgb(0.42, 0.42, 0.44),
            perceptual_roughness: 1.0,
            ..default()
        }),
        dark: mats.add(StandardMaterial {
            base_color: Color::srgb(0.16, 0.15, 0.15),
            perceptual_roughness: 1.0,
            ..default()
        }),
        bolt: mats.add(glow(
            Color::srgb(1.0, 0.85, 0.5),
            LinearRgba::rgb(20.0, 12.0, 4.0),
        )),
    }
}

/// The rotation that points a ship's bow (-z) along `dir`, nose up or down with it,
/// listing to one side once it burns.
pub fn heading(dir: Vec3, list: f32) -> Quat {
    let flat = Vec2::new(dir.x, dir.z).length();
    let yaw = crate::npc::yaw_of(dir);
    let pitch = dir.y.atan2(flat.max(1e-4)).clamp(-0.6, 0.6);
    Quat::from_euler(EulerRot::YXZ, yaw, pitch, list)
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A cheap repeatable random number in 0..1.
fn rnd(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 8) as f32 / (1u32 << 24) as f32
}

fn spread(seed: &mut u32) -> Vec3 {
    Vec3::new(rnd(seed) - 0.5, rnd(seed) - 0.5, rnd(seed) - 0.5) * 2.0
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run(
    mut commands: Commands,
    time: Res<Time>,
    mut cmds: MessageReader<PropCmd>,
    mut places: ResMut<Places>,
    (mut meshes, mut mats): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    looks: Res<crate::avatar::PlayerLooks>,
    mut ships: Query<(Entity, &mut PropShip, &mut Transform), (Without<Bolt>, Without<Puff>)>,
    mut bolts: Query<(Entity, &mut Bolt, &mut Transform), (Without<PropShip>, Without<Puff>)>,
    mut puffs: Query<(Entity, &mut Puff, &mut Transform), (Without<PropShip>, Without<Bolt>)>,
    mut sounds: MessageWriter<PlaySound>,
    (mut kit_cell, mut seed): (Local<Option<Kit>>, Local<u32>),
) {
    let dt = time.delta_secs().min(0.05);
    let kit = kit_cell.get_or_insert_with(|| kit(&mut meshes, &mut mats));
    let mut gone: Vec<String> = Vec::new();
    let mut new_ships: HashMap<String, (Entity, Vec3)> = HashMap::new();
    for c in cmds.read() {
        match c {
            PropCmd::Ship {
                name,
                at,
                dir,
                look,
            } => {
                for (e, s, _) in &ships {
                    if &s.name == name {
                        commands.entity(e).despawn();
                    }
                }
                let tf = Transform::from_translation(*at).with_rotation(heading(*dir, 0.0));
                let e = match look {
                    ShipLook::Ours => {
                        let e =
                            crate::airship::build_model(&mut commands, &mut meshes, &mut mats, tf);
                        // You, at the helm, facing the bow.
                        let (body, _) = crate::npc::body::build(
                            &mut commands,
                            &mut meshes,
                            &mut mats,
                            &looks.0,
                            crate::airship::HELM,
                            crate::npc::yaw_of(Vec3::NEG_Z),
                        );
                        commands.entity(body).insert(ChildOf(e));
                        e
                    }
                    ShipLook::Court => crate::cast::wren::build_ship(
                        &mut commands,
                        &mut meshes,
                        &mut mats,
                        tf.with_scale(Vec3::ONE),
                        COURT_SCALE,
                        Color::srgb(0.55, 0.12, 0.12),
                    ),
                };
                commands.entity(e).insert(PropShip {
                    name: name.clone(),
                    from: *at,
                    to: *at,
                    t: 0.0,
                    secs: 0.0,
                    dir: *dir,
                    burning: None,
                    smoke: 0.0,
                });
                new_ships.insert(name.clone(), (e, *at));
                places.0.insert(ship_key(name), *at + Vec3::Y * MIDDLE);
            }
            PropCmd::To { name, to, secs } => {
                for (_, mut s, tf) in &mut ships {
                    if &s.name == name {
                        s.from = tf.translation;
                        s.to = *to;
                        s.t = 0.0;
                        s.secs = secs.max(0.01);
                    }
                }
            }
            PropCmd::Fire { from, to } => {
                let at = ships
                    .iter()
                    .find(|(_, s, _)| &s.name == from)
                    .map(|(_, _, tf)| tf.translation + Vec3::Y * MIDDLE)
                    .or_else(|| new_ships.get(from).map(|(_, p)| *p));
                if let Some(at) = at {
                    commands.spawn((
                        Bolt {
                            from: at,
                            target: to.clone(),
                            t: 0.0,
                        },
                        Mesh3d(kit.cube.clone()),
                        MeshMaterial3d(kit.bolt.clone()),
                        Transform::from_translation(at).with_scale(Vec3::splat(0.5)),
                        NotShadowCaster,
                        PointLight {
                            color: Color::srgb(1.0, 0.7, 0.35),
                            intensity: 400_000.0,
                            range: 30.0,
                            ..default()
                        },
                    ));
                    sounds.write(PlaySound::new("gunshot").speed(0.55));
                }
            }
            PropCmd::Remove(name) => {
                for (e, s, _) in &ships {
                    if &s.name == name {
                        commands.entity(e).despawn();
                        gone.push(name.clone());
                    }
                }
            }
            PropCmd::Clear => {
                for (e, s, _) in &ships {
                    commands.entity(e).despawn();
                    gone.push(s.name.clone());
                }
                for (e, ..) in &bolts {
                    commands.entity(e).despawn();
                }
                for (e, ..) in &puffs {
                    commands.entity(e).despawn();
                }
            }
        }
    }

    // Ships fly, burn and trail smoke.
    let mut spots: HashMap<String, Vec3> = HashMap::new();
    for (_, mut s, mut tf) in &mut ships {
        if gone.contains(&s.name) {
            continue;
        }
        if s.secs > 0.0 && s.t < s.secs {
            s.t = (s.t + dt).min(s.secs);
            let k = s.t / s.secs;
            let before = tf.translation;
            tf.translation = s.from.lerp(s.to, smooth(k));
            let d = tf.translation - before;
            if d.length_squared() > 1e-6 {
                s.dir = s
                    .dir
                    .lerp(d.normalize(), (dt * 3.0).min(1.0))
                    .normalize_or(s.dir);
            }
        }
        let list = s
            .burning
            .map_or(0.0, |b| (b * 0.25).min(0.35) + (b * 2.3).sin() * 0.03);
        tf.rotation = heading(s.dir, list);
        let middle = tf.translation + tf.rotation * Vec3::Y * MIDDLE * tf.scale.y;
        spots.insert(s.name.clone(), middle);
        if let Some(b) = s.burning.as_mut() {
            *b += dt;
            s.smoke += dt;
            while s.smoke >= SMOKE_EVERY {
                s.smoke -= SMOKE_EVERY;
                let at = middle + tf.rotation * Vec3::new(0.0, 2.0, 1.5) + spread(&mut seed) * 0.8;
                let fire = rnd(&mut seed) < 0.35;
                let (mat, life, size) = if fire {
                    (kit.fire.clone(), 0.5, 0.9)
                } else if rnd(&mut seed) < 0.5 {
                    (kit.dark.clone(), 3.0, 1.6)
                } else {
                    (kit.smoke.clone(), 3.5, 2.0)
                };
                commands.spawn((
                    Puff {
                        vel: Vec3::Y * 1.6 + spread(&mut seed) * 0.6,
                        age: 0.0,
                        life,
                        size,
                    },
                    Mesh3d(kit.cube.clone()),
                    MeshMaterial3d(mat),
                    Transform::from_translation(at).with_scale(Vec3::splat(0.2)),
                    NotShadowCaster,
                ));
            }
        }
    }
    for n in &gone {
        places.0.remove(&ship_key(n));
    }
    for (n, p) in &spots {
        places.0.insert(ship_key(n), *p);
    }

    // Bolts fly; a hit blasts and sets the target burning.
    let mut hit: Vec<(String, Vec3)> = Vec::new();
    for (e, mut b, mut tf) in &mut bolts {
        b.t += dt;
        let Some(to) = spots.get(&b.target).copied() else {
            commands.entity(e).despawn();
            continue;
        };
        let k = (b.t / BOLT_SECS).min(1.0);
        tf.translation = b.from.lerp(to, k) + Vec3::Y * (k * (1.0 - k) * 4.0);
        if k >= 1.0 {
            commands.entity(e).despawn();
            hit.push((b.target.clone(), to));
        }
    }
    for (name, at) in hit {
        for (_, mut s, _) in &mut ships {
            if s.name == name && s.burning.is_none() {
                s.burning = Some(0.0);
            }
        }
        sounds.write(PlaySound::new("thunder").speed(1.4));
        sounds.write(PlaySound::new("break").speed(0.6));
        for i in 0..36 {
            let fire = i % 3 != 0;
            let (mat, life, size) = if fire {
                (kit.fire.clone(), 0.6, 1.4)
            } else {
                (kit.dark.clone(), 2.2, 2.2)
            };
            commands.spawn((
                Puff {
                    vel: spread(&mut seed) * 9.0,
                    age: 0.0,
                    life,
                    size,
                },
                Mesh3d(kit.cube.clone()),
                MeshMaterial3d(mat),
                Transform::from_translation(at + spread(&mut seed)).with_scale(Vec3::splat(0.3)),
                NotShadowCaster,
            ));
        }
        commands.spawn((
            Puff {
                vel: Vec3::ZERO,
                age: 0.0,
                life: 0.5,
                size: 0.0,
            },
            Transform::from_translation(at),
            PointLight {
                color: Color::srgb(1.0, 0.6, 0.25),
                intensity: 6_000_000.0,
                range: 60.0,
                ..default()
            },
        ));
    }

    // Puffs drift, swell and shrink away.
    for (e, mut p, mut tf) in &mut puffs {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(e).despawn();
            continue;
        }
        let k = p.age / p.life;
        tf.translation += p.vel * dt;
        p.vel *= 1.0 - (dt * 2.0).min(1.0);
        let grow = if k < 0.25 {
            k / 0.25
        } else {
            1.0 - (k - 0.25) / 0.75 * 0.85
        };
        tf.scale = Vec3::splat((p.size * grow).max(0.01));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ships_point_their_bow_the_way_they_go() {
        // The bow is -z in the model.
        let bow = |d: Vec3| heading(d, 0.0) * Vec3::NEG_Z;
        assert!((bow(Vec3::X) - Vec3::X).length() < 1e-4);
        assert!((bow(Vec3::NEG_Z) - Vec3::NEG_Z).length() < 1e-4);
        // Going down: the nose dips.
        assert!(bow(Vec3::new(0.0, -1.0, -2.0)).y < -0.3);
    }
}
