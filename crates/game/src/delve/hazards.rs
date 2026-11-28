//! Traps on a dungeon floor (`worldgen::delve::HazardSpot`), built when the
//! floor is and run here. They hurt foes as much as you.
//!
//! - Spikes push up out of the floor in turn and sink again (the ones at the
//!   bottom of a pit never sink).
//! - Swinging blades on chains sweep across a hall or corridor.
//! - Dart launchers in the walls shoot every few seconds: a dart stings and
//!   poisons (blood drains for a while).
//! - Fire jets in the floor roar up now and then; they light oil and barrels.
//! - Oil slicks burn when fire reaches them (a jet, a blast, a torch in your
//!   hand), spreading cell to cell, then burn away.
//! - Barrels blow up when cut, burnt or caught in another blast (a short fuse
//!   first): they hurt everything near. A kick shoves
//!   one along the floor.
//! - Cracked floor gives way a moment after you step on it, into the pit below.
//! - Pits are just holes (in the floor plan).

use bevy::prelude::*;
use voxel::{BlockId, BlockPos};
use worldgen::delve::Hazard;

use super::{Below, DelveSet};
use crate::camera::FlyCamera;
use crate::combat::Foe;
use crate::combat::kick::Kicking;
use crate::interact::Swung;
use crate::player::{Damage, DamageCause, Player};
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};
use crate::survival::health::Layer;
use crate::survival::{Survival, Toast};
use crate::world::World;

pub struct HazardsPlugin;

impl Plugin for HazardsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Traps>().add_systems(
            crate::menu::GameUpdate,
            (build, run, darts, barrels, poison).chain().after(DelveSet),
        );
    }
}

/// Spikes: a full cycle (seconds) and the part of it they're up.
const SPIKE_CYCLE: f32 = 3.0;
const SPIKES_UP: (f32, f32) = (0.55, 0.9);
/// Fire jets: cycle and the part of it they burn.
const JET_CYCLE: f32 = 4.0;
const JET_ON: (f32, f32) = (0.6, 0.9);
/// Blades: one swing there and back, seconds, and how far out (radians).
const BLADE_CYCLE: f32 = 2.6;
const BLADE_SWING: f32 = 1.05;
/// Darts: seconds between shots and speed (blocks/s).
const DART_EVERY: f32 = 2.4;
const DART_SPEED: f32 = 14.0;
/// Poison from a dart: seconds, and blood lost per second.
const POISON_FOR: f32 = 6.0;
const POISON_DRAIN: f32 = 1.2;
/// Oil: seconds a cell burns, and before it lights the next.
const OIL_BURNS: f32 = 6.0;
const OIL_SPREAD: f32 = 0.35;
/// Barrels: fuse seconds and blast radius.
const FUSE: f32 = 1.4;
const BLAST: f32 = 3.5;
/// Cracked floor: seconds from the first step to the fall.
const COLLAPSE_AFTER: f32 = 0.9;

/// The traps of the floor you're on.
#[derive(Resource, Default)]
pub struct Traps {
    on: Option<(u32, u8)>,
    list: Vec<Trap>,
    darts: Vec<Dart>,
    /// Seconds of poison left.
    pub poison: f32,
    /// Blasts this frame (where), for oil and barrels.
    blasts: Vec<Vec3>,
}

struct Trap {
    kind: Hazard,
    pos: Vec3,
    dir: Vec3,
    len: f32,
    phase: f32,
    fixed: bool,
    /// The part that moves (spikes, blade arm, barrel, jet light).
    part: Option<Entity>,
    /// Seconds before it can hurt you again.
    cool: f32,
    /// Oil: each cell and its fire (0 unlit, > 0 burning seconds, < 0 burnt out).
    cells: Vec<([i32; 3], f32)>,
    /// Barrels: fuse burning (seconds left), sliding speed, gone.
    fuse: Option<f32>,
    vel: Vec3,
    gone: bool,
    /// Cracked floor: seconds since it was stepped on.
    stepped: Option<f32>,
    /// Darts: seconds to the next shot.
    next: f32,
}

struct Dart {
    at: Vec3,
    dir: Vec3,
    left: f32,
    part: Entity,
}

fn cell_mid(p: [i32; 3]) -> Vec3 {
    Vec3::new(p[0] as f32 + 0.5, p[1] as f32, p[2] as f32 + 0.5)
}

/// Is a body standing with its feet at `feet` touching the cell `c` (feet height)?
fn on_cell(feet: Vec3, c: Vec3) -> bool {
    (feet.x - c.x).abs() < 0.6 && (feet.z - c.z).abs() < 0.6 && (feet.y - c.y).abs() < 0.7
}

/// Is a point inside a standing body (feet at `feet`, 1.8 tall)?
fn in_body(p: Vec3, feet: Vec3, r: f32) -> bool {
    Vec2::new(p.x - feet.x, p.z - feet.z).length() < r && p.y > feet.y - 0.1 && p.y < feet.y + 1.9
}

/// 0..1 of the way through a cycle.
fn cycle(t: f32, period: f32, phase: f32) -> f32 {
    (t / period + phase).rem_euclid(1.0)
}

/// The blade's angle out from straight down at time `t` (pure).
pub fn blade_angle(t: f32, phase: f32) -> f32 {
    BLADE_SWING * (std::f32::consts::TAU * cycle(t, BLADE_CYCLE, phase)).sin()
}

/// Spikes are up (pure).
pub fn spikes_up(t: f32, phase: f32, fixed: bool) -> bool {
    let c = cycle(t, SPIKE_CYCLE, phase);
    fixed || (c >= SPIKES_UP.0 && c < SPIKES_UP.1)
}

/// The fire jet burns (pure).
pub fn jet_on(t: f32, phase: f32) -> bool {
    let c = cycle(t, JET_CYCLE, phase);
    c >= JET_ON.0 && c < JET_ON.1
}

/// How hard a blast hits at a distance (1 at the middle, 0 at the edge; pure).
pub fn blast_falloff(d: f32) -> f32 {
    (1.0 - d / BLAST).clamp(0.0, 1.0)
}

/// Build the floor's traps when the floor changes.
#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    below: Res<Below>,
    mut traps: ResMut<Traps>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
) {
    let here = below.run.map(|r| (r.number, r.floor));
    if traps.on == here {
        return;
    }
    for t in &traps.list {
        if let Some(e) = t.part {
            commands.entity(e).despawn();
        }
    }
    for d in &traps.darts {
        commands.entity(d.part).despawn();
    }
    *traps = Traps {
        on: here,
        poison: traps.poison,
        ..default()
    };
    let Some(floor) = below.floor.clone() else {
        return;
    };
    let iron = materials.add(StandardMaterial {
        base_color: Color::srgb(0.52, 0.52, 0.55),
        metallic: 0.8,
        perceptual_roughness: 0.4,
        ..default()
    });
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.22),
        metallic: 0.6,
        ..default()
    });
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.28, 0.14),
        ..default()
    });
    let band = materials.add(StandardMaterial {
        base_color: Color::srgb(0.7, 0.12, 0.08),
        emissive: LinearRgba::rgb(0.3, 0.02, 0.0),
        ..default()
    });
    let spike = meshes.add(Cone::new(0.08, 0.6));
    for h in &floor.hazards {
        let pos = cell_mid(h.pos);
        let dir = Vec3::new(h.dir.0 as f32, 0.0, h.dir.1 as f32);
        let part = match h.kind {
            Hazard::Spikes => Some(
                commands
                    .spawn((Transform::from_translation(pos), Visibility::default()))
                    .with_children(|c| {
                        for (x, z) in [
                            (-0.22, -0.22),
                            (0.22, -0.22),
                            (-0.22, 0.22),
                            (0.22, 0.22),
                            (0.0, 0.0),
                        ] {
                            c.spawn((
                                Mesh3d(spike.clone()),
                                MeshMaterial3d(iron.clone()),
                                Transform::from_xyz(x, 0.3, z),
                            ));
                        }
                    })
                    .id(),
            ),
            Hazard::Blade => {
                let pivot = pos + Vec3::Y * (h.len as f32 + 0.6);
                Some(
                    commands
                        .spawn((Transform::from_translation(pivot), Visibility::default()))
                        .with_children(|c| {
                            let len = h.len as f32;
                            c.spawn((
                                Mesh3d(meshes.add(Cuboid::new(0.06, len, 0.06))),
                                MeshMaterial3d(dark.clone()),
                                Transform::from_xyz(0.0, -len / 2.0, 0.0),
                            ));
                            // The blade lies across the way it swings.
                            let across = Quat::from_rotation_arc(
                                Vec3::X,
                                Vec3::new(-dir.z, 0.0, dir.x).normalize_or(Vec3::X),
                            );
                            c.spawn((
                                Mesh3d(meshes.add(Cuboid::new(0.08, 0.5, 1.3))),
                                MeshMaterial3d(iron.clone()),
                                Transform::from_xyz(0.0, -len, 0.0).with_rotation(across),
                            ));
                        })
                        .id(),
                )
            }
            Hazard::Barrel => Some(
                commands
                    .spawn((Transform::from_translation(pos), Visibility::default()))
                    .with_children(|c| {
                        c.spawn((
                            Mesh3d(meshes.add(Cylinder::new(0.36, 0.9))),
                            MeshMaterial3d(wood.clone()),
                            Transform::from_xyz(0.0, 0.45, 0.0),
                        ));
                        for y in [0.2, 0.7] {
                            c.spawn((
                                Mesh3d(meshes.add(Cylinder::new(0.38, 0.07))),
                                MeshMaterial3d(band.clone()),
                                Transform::from_xyz(0.0, y, 0.0),
                            ));
                        }
                    })
                    .id(),
            ),
            Hazard::FireJet => Some(
                commands
                    .spawn((
                        Transform::from_translation(pos + Vec3::Y * 0.8),
                        PointLight {
                            color: Color::srgb(1.0, 0.55, 0.2),
                            intensity: 0.0,
                            range: 7.0,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                    ))
                    .id(),
            ),
            _ => None,
        };
        traps.list.push(Trap {
            kind: h.kind,
            pos,
            dir,
            len: h.len as f32,
            phase: h.phase,
            fixed: h.fixed,
            part,
            cool: 0.0,
            cells: h.cells.iter().map(|c| (*c, 0.0)).collect(),
            fuse: None,
            vel: Vec3::ZERO,
            gone: false,
            stepped: None,
            next: h.phase * DART_EVERY,
        });
    }
}

/// Spikes, blades, jets, oil and cracked floor.
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::needless_range_loop
)]
fn run(
    time: Res<Time>,
    mut below: ResMut<Below>,
    mut traps: ResMut<Traps>,
    player: Res<Player>,
    mut foes: Query<(&Transform, &mut Foe), Without<FlyCamera>>,
    mut parts: Query<(&mut Transform, Option<&mut PointLight>), (Without<Foe>, Without<FlyCamera>)>,
    (items, wielded, off): (
        Res<crate::items::Items>,
        Res<crate::interact::Wielded>,
        Res<crate::interact::OffHand>,
    ),
    (mut hurt, mut sound, mut bursts, mut toast): (
        MessageWriter<Damage>,
        MessageWriter<PlaySound>,
        MessageWriter<Burst>,
        MessageWriter<Toast>,
    ),
    mut sparks: Local<f32>,
) {
    if below.floor.is_none() {
        return;
    }
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    *sparks -= dt;
    let show = *sparks <= 0.0;
    if show {
        *sparks = 0.12;
    }
    let feet = player.pos;
    let torch = crate::interact::holding_torch(&items, &wielded, &off);
    let blasts = std::mem::take(&mut traps.blasts);
    // Fire this frame (jets and burning oil), for oil and barrels.
    let mut fires: Vec<Vec3> = blasts.clone();
    let Traps { list, .. } = &mut *traps;
    for tr in list.iter_mut() {
        tr.cool = (tr.cool - dt).max(0.0);
        match tr.kind {
            Hazard::Spikes => {
                let up = spikes_up(t, tr.phase, tr.fixed);
                if let Some((mut tf, _)) = tr.part.and_then(|e| parts.get_mut(e).ok()) {
                    let want = if up { 0.0 } else { -0.62 };
                    tf.translation.y += (tr.pos.y + want - tf.translation.y) * (dt * 18.0).min(1.0);
                }
                if !up {
                    continue;
                }
                if on_cell(feet, tr.pos) && tr.cool <= 0.0 {
                    tr.cool = 0.8;
                    hurt.write(Damage {
                        amount: 5.0,
                        cause: DamageCause::Piercing,
                    });
                    sound.write(PlaySound::new("hit").at(tr.pos).speed(1.4));
                }
                for (ftf, mut f) in &mut foes {
                    if on_cell(ftf.translation, tr.pos) {
                        f.health -= 10.0 * dt;
                    }
                }
            }
            Hazard::Blade => {
                let a = blade_angle(t, tr.phase);
                if let Some((mut tf, _)) = tr.part.and_then(|e| parts.get_mut(e).ok()) {
                    let axis = Vec3::new(-tr.dir.z, 0.0, tr.dir.x).normalize_or(Vec3::Z);
                    tf.rotation = Quat::from_axis_angle(axis, -a);
                }
                let pivot = tr.pos + Vec3::Y * (tr.len + 0.6);
                let head = pivot + tr.dir * a.sin() * tr.len - Vec3::Y * a.cos() * tr.len;
                if in_body(head, feet, 0.8) && tr.cool <= 0.0 {
                    tr.cool = 0.7;
                    hurt.write(Damage {
                        amount: 9.0,
                        cause: DamageCause::Piercing,
                    });
                    sound.write(PlaySound::new("clash").at(head).speed(0.8));
                    bursts.write(Burst::new(head, BurstKind::Sparks, 8));
                }
                for (ftf, mut f) in &mut foes {
                    if in_body(head, ftf.translation, 0.8) && f.flinch < 0.5 {
                        f.health -= 14.0;
                        f.flinch = 1.0;
                    }
                }
            }
            Hazard::FireJet => {
                let on = jet_on(t, tr.phase);
                if let Some((_, Some(mut light))) = tr.part.and_then(|e| parts.get_mut(e).ok()) {
                    let want = if on { 90_000.0 } else { 0.0 };
                    if light.intensity != want {
                        light.intensity = want;
                    }
                }
                if !on {
                    continue;
                }
                let h = tr.len.max(2.0);
                if show {
                    for i in 0..(h as i32) {
                        bursts.write(Burst::new(
                            tr.pos + Vec3::Y * (i as f32 + 0.4),
                            BurstKind::Sparks,
                            3,
                        ));
                    }
                    bursts.write(Burst::new(tr.pos + Vec3::Y * h, BurstKind::Smoke, 1));
                }
                for i in 0..(h as i32) {
                    fires.push(tr.pos + Vec3::Y * i as f32);
                }
                let in_column = |f: Vec3| {
                    Vec2::new(f.x - tr.pos.x, f.z - tr.pos.z).length() < 0.7
                        && f.y > tr.pos.y - 1.5
                        && f.y < tr.pos.y + h
                };
                if in_column(feet) && tr.cool <= 0.0 {
                    tr.cool = 0.5;
                    hurt.write(Damage {
                        amount: 4.0,
                        cause: DamageCause::Blunt,
                    });
                }
                for (ftf, mut f) in &mut foes {
                    if in_column(ftf.translation) {
                        f.health -= 8.0 * dt;
                        f.ail.burn(3.0);
                    }
                }
            }
            Hazard::Collapse => {
                if tr
                    .cells
                    .iter()
                    .any(|(c, s)| *s >= 0.0 && on_cell(feet, cell_mid(*c)))
                    && tr.stepped.is_none()
                {
                    tr.stepped = Some(0.0);
                    sound.write(PlaySound::new("dig_hard").at(feet).speed(0.6));
                    toast.write(Toast("The floor cracks under you".into()));
                }
                if let Some(s) = tr.stepped.as_mut() {
                    *s += dt;
                    if show {
                        for (c, _) in &tr.cells {
                            bursts.write(Burst::new(cell_mid(*c), BurstKind::Chips, 1));
                        }
                    }
                    if *s >= COLLAPSE_AFTER {
                        tr.stepped = None;
                        for (c, s) in tr.cells.iter_mut() {
                            *s = -1.0;
                            below.edit([c[0], c[1] - 1, c[2]], BlockId::AIR);
                        }
                        sound.write(PlaySound::new("hit").at(feet).speed(0.5).gain(1.2));
                        bursts.write(Burst::new(feet, BurstKind::Smoke, 12));
                    }
                }
            }
            _ => {}
        }
    }
    // Oil: lit by fire, a blast or a torch in hand; spreads; burns away.
    for tr in list.iter_mut().filter(|t| t.kind == Hazard::Oil) {
        let lit_by = |c: Vec3| {
            fires.iter().any(|f| f.distance(c + Vec3::Y * 0.3) < 1.6) || (torch && on_cell(feet, c))
        };
        let n = tr.cells.len();
        for i in 0..n {
            let (c, s) = tr.cells[i];
            let mid = cell_mid(c);
            if s == 0.0 && lit_by(mid) {
                tr.cells[i].1 = OIL_BURNS;
                sound.write(PlaySound::new("spell_on").at(mid).speed(0.6).gain(0.7));
            }
        }
        for i in 0..n {
            let (c, s) = tr.cells[i];
            if s <= 0.0 {
                continue;
            }
            let mid = cell_mid(c);
            let left = s - dt;
            // Neighbours catch once it has burnt a moment.
            if OIL_BURNS - left > OIL_SPREAD {
                for j in 0..n {
                    let (d, sj) = tr.cells[j];
                    if sj == 0.0 && (d[0] - c[0]).abs() + (d[2] - c[2]).abs() == 1 {
                        tr.cells[j].1 = OIL_BURNS + 0.01;
                    }
                }
            }
            if show {
                bursts.write(Burst::new(mid + Vec3::Y * 0.2, BurstKind::Sparks, 2));
            }
            if on_cell(feet, mid) && tr.cool <= 0.0 {
                tr.cool = 0.5;
                hurt.write(Damage {
                    amount: 3.0,
                    cause: DamageCause::Blunt,
                });
            }
            for (ftf, mut f) in &mut foes {
                if on_cell(ftf.translation, mid) {
                    f.health -= 6.0 * dt;
                    f.ail.burn(3.0);
                }
            }
            fires.push(mid);
            if left <= 0.0 {
                tr.cells[i].1 = -1.0;
                below.edit(c, BlockId::AIR);
            } else {
                tr.cells[i].1 = left;
            }
        }
    }
    // Barrels catch from fire too.
    for tr in list
        .iter_mut()
        .filter(|t| t.kind == Hazard::Barrel && !t.gone && t.fuse.is_none())
    {
        if fires
            .iter()
            .any(|f| f.distance(tr.pos + Vec3::Y * 0.4) < 1.3)
        {
            tr.fuse = Some(FUSE);
        }
    }
}

/// Dart launchers and their darts.
#[allow(clippy::too_many_arguments)]
fn darts(
    mut commands: Commands,
    time: Res<Time>,
    mut traps: ResMut<Traps>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<crate::blocks::Blocks>,
    mut foes: Query<(&Transform, &mut Foe)>,
    mut tfs: Query<&mut Transform, Without<Foe>>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    (mut hurt, mut sound, mut toast): (
        MessageWriter<Damage>,
        MessageWriter<PlaySound>,
        MessageWriter<Toast>,
    ),
    mut look: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    if traps.on.is_none() {
        return;
    }
    let dt = time.delta_secs();
    let (mesh, mat) = look
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(0.04, 0.04, 0.4)),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(0.3, 0.5, 0.25),
                    ..default()
                }),
            )
        })
        .clone();
    let mut shots = Vec::new();
    for tr in traps.list.iter_mut().filter(|t| t.kind == Hazard::Darts) {
        tr.next -= dt;
        if tr.next <= 0.0 {
            tr.next = DART_EVERY;
            // Only near you (no darts flying in empty halls).
            if tr.pos.distance(player.pos) < 24.0 {
                shots.push((
                    tr.pos + Vec3::Y * 0.5 + tr.dir * 0.6,
                    tr.dir,
                    tr.len.max(4.0),
                ));
            }
        }
    }
    for (at, dir, len) in shots {
        let part = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(at).looking_to(dir, Vec3::Y),
            ))
            .id();
        sound.write(PlaySound::new("clash").at(at).speed(2.2).gain(0.4));
        traps.darts.push(Dart {
            at,
            dir,
            left: len,
            part,
        });
    }
    let solid = |p: Vec3| {
        world
            .block(BlockPos::new(
                p.x.floor() as i32,
                p.y.floor() as i32,
                p.z.floor() as i32,
            ))
            .is_some_and(|b| blocks.registry.get(b).solid)
    };
    let mut poisoned = false;
    traps.darts.retain_mut(|d| {
        let step = DART_SPEED * dt;
        d.at += d.dir * step;
        d.left -= step;
        if let Ok(mut tf) = tfs.get_mut(d.part) {
            tf.translation = d.at;
        }
        let mut done = d.left <= 0.0 || solid(d.at);
        if !done && in_body(d.at, player.pos, 0.45) {
            hurt.write(Damage {
                amount: 2.0,
                cause: DamageCause::Piercing,
            });
            poisoned = true;
            done = true;
        }
        if !done {
            for (ftf, mut f) in &mut foes {
                if in_body(d.at, ftf.translation, 0.5) {
                    f.health -= 6.0;
                    f.flinch = 1.0;
                    done = true;
                    break;
                }
            }
        }
        if done {
            commands.entity(d.part).despawn();
        }
        !done
    });
    if poisoned {
        if traps.poison <= 0.0 {
            toast.write(Toast("A dart! You're poisoned".into()));
        }
        traps.poison = POISON_FOR;
    }
}

/// Barrels: cut, kicked, burnt and blown up.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn barrels(
    time: Res<Time>,
    below: Res<Below>,
    mut traps: ResMut<Traps>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<crate::blocks::Blocks>,
    kicking: Res<Kicking>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut swung: MessageReader<Swung>,
    mut foes: Query<(&Transform, &mut Foe), Without<FlyCamera>>,
    mut parts: Query<(&mut Transform, &mut Visibility), (Without<Foe>, Without<FlyCamera>)>,
    (mut hurt, mut sound, mut bursts): (
        MessageWriter<Damage>,
        MessageWriter<PlaySound>,
        MessageWriter<Burst>,
    ),
    mut kick_before: Local<Option<f32>>,
) {
    let dt = time.delta_secs();
    let on_floor = below.floor.is_some();
    // A kick lands at about 0.4 of the way through.
    let kicked = kicking.t.is_some_and(|t| t >= 0.4) && kick_before.is_none_or(|b| b < 0.4);
    *kick_before = kicking.t;
    let cuts: Vec<Swung> = swung.read().copied().collect();
    if !on_floor {
        return;
    }
    let fwd = {
        let f = cam.forward().as_vec3();
        Vec3::new(f.x, 0.0, f.z).normalize_or_zero()
    };
    let solid = |p: Vec3| {
        world
            .block(BlockPos::new(
                p.x.floor() as i32,
                p.y.floor() as i32,
                p.z.floor() as i32,
            ))
            .is_some_and(|b| blocks.registry.get(b).solid)
    };
    let mut blasts = Vec::new();
    for tr in traps
        .list
        .iter_mut()
        .filter(|t| t.kind == Hazard::Barrel && !t.gone)
    {
        let mid = tr.pos + Vec3::Y * 0.45;
        if kicked && mid.distance(player.pos + Vec3::Y * 0.45 + fwd * 0.8) < 1.3 {
            tr.vel = fwd * 6.0;
            sound.write(PlaySound::new("dig_hard").at(mid).speed(0.9));
        }
        for c in &cuts {
            let to = mid - c.origin;
            if to.length() < c.reach + 0.5
                && to.normalize_or_zero().dot(c.dir) > (c.arc.to_radians()).cos()
                && tr.fuse.is_none()
            {
                tr.fuse = Some(FUSE);
            }
        }
        // Sliding along the floor until it meets a wall.
        if tr.vel.length_squared() > 0.01 {
            let next = tr.pos + tr.vel * dt;
            if solid(next + Vec3::Y * 0.5 + tr.vel.normalize() * 0.4) {
                tr.vel = Vec3::ZERO;
            } else {
                tr.pos = next;
                tr.vel *= (1.0 - 2.5 * dt).max(0.0);
            }
        }
        if let Some(f) = tr.fuse.as_mut() {
            *f -= dt;
            bursts.write(Burst::new(mid + Vec3::Y * 0.5, BurstKind::Sparks, 1));
            if *f <= 0.0 {
                tr.gone = true;
                blasts.push(mid);
            }
        }
        if let Some((mut tf, mut vis)) = tr.part.and_then(|e| parts.get_mut(e).ok()) {
            tf.translation = tr.pos;
            if tr.fuse.is_some() {
                // It shakes as the fuse burns down.
                tf.translation.x += (time.elapsed_secs() * 60.0).sin() * 0.03;
            }
            if tr.gone {
                *vis = Visibility::Hidden;
            }
        }
    }
    // Chains: a blast sets off barrels near it.
    for b in &blasts {
        for tr in traps
            .list
            .iter_mut()
            .filter(|t| t.kind == Hazard::Barrel && !t.gone && t.fuse.is_none())
        {
            if tr.pos.distance(*b) < BLAST {
                tr.fuse = Some(0.25);
            }
        }
    }
    for b in &blasts {
        sound.write(PlaySound::new("hit").at(*b).speed(0.35).gain(1.6));
        sound.write(PlaySound::new("dig_hard").at(*b).speed(0.5).gain(1.2));
        bursts.write(Burst::new(*b, BurstKind::Smoke, 24));
        bursts.write(Burst::new(*b, BurstKind::Sparks, 30));
        let k = blast_falloff(b.distance(player.pos + Vec3::Y));
        if k > 0.0 {
            hurt.write(Damage {
                amount: 14.0 * k,
                cause: DamageCause::Blunt,
            });
        }
        for (ftf, mut f) in &mut foes {
            let k = blast_falloff(b.distance(ftf.translation + Vec3::Y));
            if k > 0.0 {
                f.health -= 30.0 * k;
                f.flinch = 1.0;
                f.ail.burn(2.0);
            }
        }
    }
    traps.blasts.extend(blasts);
}

/// A dart's poison drains blood for a while.
fn poison(time: Res<Time>, mut traps: ResMut<Traps>, mut s: ResMut<Survival>, below: Res<Below>) {
    if traps.poison <= 0.0 {
        return;
    }
    let dt = time.delta_secs();
    traps.poison = if below.run.is_some() {
        traps.poison - dt
    } else {
        0.0
    };
    s.vitals.health.hit(Layer::Blood, POISON_DRAIN * dt);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spikes_rise_in_turn_and_pit_spikes_stay_up() {
        let ups: Vec<bool> = (0..30)
            .map(|i| spikes_up(i as f32 * 0.1, 0.0, false))
            .collect();
        assert!(ups.iter().any(|u| *u) && ups.iter().any(|u| !*u));
        assert!((0..30).all(|i| spikes_up(i as f32 * 0.1, 0.3, true)));
        // A row offset by phase isn't up all at once.
        assert!((0..30).any(
            |i| spikes_up(i as f32 * 0.1, 0.0, false) != spikes_up(i as f32 * 0.1, 0.5, false)
        ));
    }

    #[test]
    fn blades_swing_both_ways_and_blasts_fade() {
        let a: Vec<f32> = (0..26).map(|i| blade_angle(i as f32 * 0.1, 0.0)).collect();
        assert!(a.iter().any(|v| *v > 0.9) && a.iter().any(|v| *v < -0.9));
        assert_eq!(blast_falloff(0.0), 1.0);
        assert_eq!(blast_falloff(BLAST + 1.0), 0.0);
        assert!(jet_on(JET_CYCLE * 0.7, 0.0) && !jet_on(JET_CYCLE * 0.2, 0.0));
    }
}
