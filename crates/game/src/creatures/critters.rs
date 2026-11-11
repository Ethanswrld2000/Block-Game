//! Critters: small life that makes the world sound and feel alive. Flocks of
//! birds circle high over a wide area, now and then land to hop about and call,
//! and burst up again when you come close; frogs hop and croak near water;
//! crickets chirp, fireflies blink and moths flutter round torches at night;
//! darters zip over the water by day.
//!
//! They are `temper: Critter` kinds in `assets/data/creatures.ron` with a
//! `critter: (...)` block (see `data::CritterSpec`), drawn with the same box
//! models and skins as other creatures. Their `spawn` rule says where and when;
//! out of their hours, weather or warmth they leave (birds fly off, the rest
//! fade). They are scenery: you can't hit them and they drop nothing.
//! Hoppers use the creatures' walking physics; birds and drifters fly freely.

use bevy::prelude::*;
use voxel::BlockPos;

use super::brain::{self, Rng};
use super::data::{self, CritterSpec, Kind, Kinds, Motion, Spawn};
use super::{Creature, CreatureAssets, CreatureSet, Dice, Part, flat, lights, spawn_creature};
use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::clock::WorldClock;
use crate::interact::Wielded;
use crate::items::Items;
use crate::player::{MoveMode, Player};
use crate::sound::PlaySound;
use crate::torches::Torches;
use crate::weather::{Conditions, LocalClimate};
use crate::world::{Generator, World};

pub struct CrittersPlugin;

impl Plugin for CrittersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_critters, move_critters, animate, calls).chain().in_set(CreatureSet).after(super::walk));
    }
}

/// Leaving critters (other than birds) only fade once they're this far from you.
const FADE_AWAY: f32 = 16.0;

/// Rain or snow heavier than this sends fair-weather critters away.
const WET: f32 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Bird: circling. Drifter: heading for its goal.
    Fly,
    /// Bird: gliding down to a spot.
    Land,
    /// Bird: on the ground, hopping and calling. Hopper: sitting. Drifter: hovering still.
    Rest,
    /// Hopper: in the air.
    Hop,
    /// Bird: flying off for good.
    Leave,
}

#[derive(Component, Debug)]
pub struct Critter {
    state: State,
    /// Seconds left in this state.
    timer: f32,
    /// Seconds to the next call.
    call: f32,
    /// Bird: middle of its circle (y = the ground there). Others: the spot it stays near.
    home: Vec3,
    /// Bird: circle radius, angle round it, which way round (+1/-1), height over the ground.
    radius: f32,
    angle: f32,
    turn: f32,
    alt: f32,
    /// Bird: how its circle drifts across the land (blocks/s).
    drift: Vec3,
    /// Bird: ground under it. Used to keep clear of hills and trees.
    floor: f32,
    /// Landing spot, hover goal or perch.
    goal: Vec3,
    /// Seconds to the next look at the ground (birds) or small hop on a perch.
    look: f32,
    /// 0..1, for flapping and blinking out of step with the others.
    seed: f32,
}

impl Critter {
    fn new(state: State, home: Vec3, seed: f32) -> Self {
        Self { state, timer: 0.0, call: 0.0, home, radius: 0.0, angle: 0.0, turn: 1.0, alt: 0.0, drift: Vec3::ZERO, floor: home.y, goal: home, look: 0.0, seed }
    }
}

/// Is a critter at home in these conditions? (Pure, for tests.)
pub fn welcome(spec: &CritterSpec, spawn: &Spawn, night: bool, temperature: f32, precipitation: f32) -> bool {
    data::time_ok(spawn.when, night) && spec.min_temp.is_none_or(|m| temperature >= m) && !(spec.fair_weather && precipitation > WET)
}

/// Where a bird's circle puts it: `home.y` is the ground, `alt` the height above it.
pub fn orbit_point(home: Vec3, radius: f32, angle: f32, alt: f32) -> Vec3 {
    Vec3::new(home.x + angle.cos() * radius, home.y + alt, home.z + angle.sin() * radius)
}

/// The top of a column: the open block above the highest solid or water block between
/// `from` and `to`, and whether that block is water. Unloaded stretches are skipped;
/// `None` if nothing is found or the top isn't loaded.
pub fn surface(world: &World, blocks: &Blocks, x: i32, z: i32, from: i32, to: i32) -> Option<(i32, bool)> {
    let water = blocks.registry.id("water");
    let mut open_above = false;
    for y in (to..=from).rev() {
        match world.block(BlockPos::new(x, y, z)) {
            None => open_above = false,
            Some(b) if blocks.registry.get(b).solid || Some(b) == water => return open_above.then_some((y + 1, Some(b) == water)),
            Some(_) => open_above = true,
        }
    }
    None
}

/// Is there water at the surface within a few blocks of (x, z)?
fn water_near(world: &World, blocks: &Blocks, x: i32, z: i32, y: i32) -> bool {
    (-1..=1).flat_map(|dx| (-1..=1).map(move |dz| (dx * 3, dz * 3))).any(|(dx, dz)| surface(world, blocks, x + dx, z + dz, y + 3, y - 4).is_some_and(|(_, w)| w))
}

fn solid(world: &World, blocks: &Blocks, p: Vec3) -> bool {
    world.block(BlockPos::from_world(p.to_array())).is_some_and(|b| blocks.registry.get(b).solid)
}

/// Steer towards `target` at `speed`, turning at `agility` (1/s). Returns the new position.
fn steer(c: &mut Creature, pos: Vec3, target: Vec3, speed: f32, agility: f32, dt: f32) -> Vec3 {
    let want = (target - pos).normalize_or_zero() * speed;
    c.vel += (want - c.vel) * (agility * dt).min(1.0);
    pos + c.vel * dt
}

fn face_along(c: &mut Creature, v: Vec3) {
    let f = flat(v);
    if f.length_squared() > 0.01 {
        c.yaw = (-f.x).atan2(-f.z);
    }
}

fn rng_in(rng: &mut Rng, (a, b): (f32, f32)) -> f32 {
    rng.range(a, b.max(a))
}

/// Keep each kind of critter topped up around you while it's welcome.
#[allow(clippy::too_many_arguments)]
fn spawn_critters(
    mut commands: Commands,
    time: Res<Time>,
    (kinds, assets): (Option<Res<Kinds>>, Option<Res<CreatureAssets>>),
    generator: Option<Res<Generator>>,
    (world, blocks): (Res<World>, Res<Blocks>),
    (clock, climate, conditions): (Res<WorldClock>, Res<LocalClimate>, Res<Conditions>),
    (player, mode): (Res<Player>, Res<MoveMode>),
    q: Query<&Creature, With<Critter>>,
    mut dice: ResMut<Dice>,
    mut timer: Local<f32>,
) {
    let (Some(kinds), Some(assets), Some(generator)) = (kinds, assets, generator) else { return };
    *timer -= time.delta_secs();
    if *timer > 0.0 || !(player.ready || *mode == MoveMode::Fly) {
        return;
    }
    *timer = 0.4;
    let night = brain::spirit_hours(clock.hour());
    let mut counts = vec![0usize; kinds.list.len()];
    for c in &q {
        counts[c.kind] += 1;
    }
    let rng = &mut dice.0;
    let py = player.pos.y.floor() as i32;
    for (kind, k) in kinds.list.iter().enumerate() {
        let (Some(spec), Some(sp)) = (&k.critter, &k.spawn) else { continue };
        if counts[kind] >= spec.count || !welcome(spec, sp, night, climate.temperature, conditions.precipitation) {
            continue;
        }
        // One try per kind per tick; misses (wrong biome, no water nearby) just wait for the next.
        let a = rng.range(0.0, std::f32::consts::TAU);
        let d = rng_in(rng, spec.ring);
        let (x, z) = ((player.pos.x + a.cos() * d).floor() as i32, (player.pos.z + a.sin() * d).floor() as i32);
        let Some((y, wet)) = surface(&world, &blocks, x, z, py + 40, py - 40) else { continue };
        if !sp.lives_in(&generator.0.biome_at(x, y, z).name) {
            continue;
        }
        let ok = match spec.motion {
            Motion::Bird => true,
            _ if spec.near_water => wet || water_near(&world, &blocks, x, z, y),
            Motion::Hopper => !wet,
            Motion::Drifter => true,
        };
        if !ok {
            continue;
        }
        let (lo, hi) = sp.group;
        let n = (lo + (rng.next() % (hi.saturating_sub(lo) as u64 + 1)) as u8).min((spec.count - counts[kind]) as u8);
        let ground = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
        // A flock shares one circle, spaced out along it.
        let (radius, turn, angle, alt) = (rng.range(10.0, 26.0), if rng.unit() < 0.5 { 1.0 } else { -1.0 }, rng.range(0.0, std::f32::consts::TAU), rng_in(rng, spec.height));
        let drift = Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)).normalize_or_zero() * rng.range(0.3, 1.2);
        for i in 0..n {
            let (pos, critter) = match spec.motion {
                Motion::Bird => {
                    let mut c = Critter::new(State::Fly, ground, rng.unit());
                    c.radius = radius + rng.range(-2.5, 2.5);
                    c.angle = angle - turn * i as f32 * 0.35;
                    c.turn = turn;
                    c.alt = alt + rng.range(-1.5, 1.5);
                    c.drift = drift;
                    c.timer = rng.range(8.0, 25.0);
                    (orbit_point(c.home, c.radius, c.angle, c.alt), c)
                }
                Motion::Hopper | Motion::Drifter => {
                    let (gx, gz) = if i == 0 { (x, z) } else { (x + rng.range(-3.0, 3.0) as i32, z + rng.range(-3.0, 3.0) as i32) };
                    let Some((gy, w)) = surface(&world, &blocks, gx, gz, y + 3, y - 3) else { continue };
                    if spec.motion == Motion::Hopper && w && !spec.near_water {
                        continue;
                    }
                    let feet = Vec3::new(gx as f32 + 0.5, gy as f32, gz as f32 + 0.5);
                    if spec.motion == Motion::Hopper {
                        let mut c = Critter::new(State::Rest, feet, rng.unit());
                        c.timer = rng_in(rng, spec.rest);
                        (feet, c)
                    } else {
                        let at = feet + Vec3::Y * rng_in(rng, spec.height);
                        (at, Critter::new(State::Fly, at, rng.unit()))
                    }
                }
            };
            let mut critter = critter;
            critter.call = rng_in(rng, spec.call_every);
            let yaw = rng.range(0.0, std::f32::consts::TAU);
            let e = spawn_creature(&mut commands, &kinds, &assets, kind, pos, yaw);
            let mut c = Creature::new(kind, k.health, yaw);
            // Birds come in already flying (far off); the small ones grow in.
            c.fade = if spec.motion == Motion::Bird { 1.0 } else { 0.0 };
            let scale = c.fade.max(0.01);
            commands.entity(e).insert((c, critter, Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(scale))));
            counts[kind] += 1;
        }
    }
}

/// Everything a critter reacts to, gathered once a frame.
struct Around {
    night: bool,
    temperature: f32,
    precipitation: f32,
    player: Vec3,
    crouching: bool,
    lights: Vec<Vec3>,
    t: f32,
    dt: f32,
}

/// Fly, hop and drift; leave when unwelcome.
#[allow(clippy::too_many_arguments)]
fn move_critters(
    mut commands: Commands,
    time: Res<Time>,
    kinds: Option<Res<Kinds>>,
    (world, blocks): (Res<World>, Res<Blocks>),
    (clock, climate, conditions): (Res<WorldClock>, Res<LocalClimate>, Res<Conditions>),
    player: Res<Player>,
    (torches, wielded, items, spells): (Res<Torches>, Res<Wielded>, Res<Items>, Res<crate::progress::Spells>),
    mut sounds: MessageWriter<PlaySound>,
    mut q: Query<(Entity, &mut Transform, &mut Creature, &mut Critter)>,
    mut dice: ResMut<Dice>,
) {
    let Some(kinds) = kinds else { return };
    let around = Around {
        night: brain::spirit_hours(clock.hour()),
        temperature: climate.temperature,
        precipitation: conditions.precipitation,
        player: player.pos,
        crouching: player.crouching,
        lights: lights(&torches, &player, &wielded, &items, spells.light),
        t: time.elapsed_secs(),
        dt: time.delta_secs().min(0.05),
    };
    let rng = &mut dice.0;
    for (e, mut tf, mut c, mut cr) in &mut q {
        let k = kinds.get(c.kind);
        let (Some(spec), Some(sp)) = (&k.critter, &k.spawn) else { continue };
        if flat(tf.translation - around.player).length() > spec.ring.1 * 1.6 {
            commands.entity(e).despawn();
            continue;
        }
        if !c.leaving && !welcome(spec, sp, around.night, around.temperature, around.precipitation) {
            c.leaving = true;
            if spec.motion == Motion::Bird {
                cr.state = State::Leave;
                cr.timer = 15.0;
                cr.drift = flat(tf.translation - around.player).normalize_or(Vec3::X);
            }
        }
        let shy = spec.shy * if around.crouching { 0.5 } else { 1.0 };
        let startled = shy > 0.0 && tf.translation.distance(around.player) < shy;
        let pos = tf.translation;
        let next = match spec.motion {
            Motion::Bird => bird(&mut c, &mut cr, k, spec, pos, startled, &around, &world, &blocks, rng, &mut sounds),
            Motion::Hopper => {
                hopper(&mut c, &mut cr, spec, pos, startled, &around, &world, &blocks, rng);
                pos // `walk` moves it
            }
            Motion::Drifter => drifter(&mut c, &mut cr, k, spec, pos, startled, &around, &world, &blocks, rng),
        };
        // Birds leave by flying off; the small ones fade, but only out of your way, so
        // none vanish in front of you.
        let near = flat(tf.translation - around.player).length() < FADE_AWAY;
        if c.leaving && spec.motion != Motion::Bird && !near {
            c.fade -= around.dt / 1.5;
        } else {
            c.fade = (c.fade + around.dt).min(1.0);
        }
        if c.fade <= 0.0 || (cr.state == State::Leave && cr.timer <= 0.0) {
            commands.entity(e).despawn();
            continue;
        }
        tf.translation = next;
        tf.scale = Vec3::splat(c.fade.max(0.01));
        if spec.motion != Motion::Hopper {
            tf.rotation = tf.rotation.slerp(Quat::from_rotation_y(c.yaw), (around.dt * 6.0).min(1.0));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn bird(
    c: &mut Creature,
    cr: &mut Critter,
    k: &Kind,
    spec: &CritterSpec,
    pos: Vec3,
    startled: bool,
    around: &Around,
    world: &World,
    blocks: &Blocks,
    rng: &mut Rng,
    sounds: &mut MessageWriter<PlaySound>,
) -> Vec3 {
    let dt = around.dt;
    cr.timer -= dt;
    let take_off = |c: &mut Creature, cr: &mut Critter, rng: &mut Rng, sounds: &mut MessageWriter<PlaySound>| {
        cr.state = State::Fly;
        cr.timer = rng.range(12.0, 35.0);
        let away = flat(pos - around.player).normalize_or(Vec3::X);
        c.vel = Vec3::Y * 5.0 + away * 2.0;
        // Pick the circle up again from where it is.
        cr.angle = (pos.z - cr.home.z).atan2(pos.x - cr.home.x);
        let d = pos.distance(around.player);
        if d < 24.0 {
            sounds.write(PlaySound::ambient_at("bird_flap", pos, 1.0 - d / 24.0));
        }
    };
    match cr.state {
        State::Fly => {
            cr.look -= dt;
            if cr.look <= 0.0 {
                cr.look = 1.5;
                let (hx, hz) = (cr.home.x.floor() as i32, cr.home.z.floor() as i32);
                let py = around.player.y as i32;
                if let Some((y, _)) = surface(world, blocks, hx, hz, py + 64, py - 64) {
                    cr.home.y = y as f32;
                }
                if let Some((y, _)) = surface(world, blocks, pos.x.floor() as i32, pos.z.floor() as i32, py + 64, py - 64) {
                    cr.floor = y as f32;
                }
                // Wander the land, but don't stray beyond where you are.
                let back = flat(around.player - cr.home);
                if back.length() > spec.ring.1 * 0.9 {
                    cr.drift = back.normalize() * 1.5;
                } else if rng.unit() < 0.05 {
                    cr.drift = Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)).normalize_or_zero() * rng.range(0.3, 1.2);
                }
            }
            cr.home += cr.drift * dt;
            cr.angle += cr.turn * k.speed / cr.radius.max(4.0) * dt;
            let mut target = orbit_point(cr.home, cr.radius, cr.angle + cr.turn * 0.5, cr.alt + (around.t * 0.4 + cr.seed * 6.0).sin() * 1.5);
            target.y = target.y.max(cr.floor + 5.0);
            let next = steer(c, pos, target, k.speed, 1.8, dt);
            face_along(c, c.vel);
            // Now and then come down somewhere under the circle.
            if cr.timer <= 0.0 && !c.leaving {
                cr.timer = rng.range(8.0, 22.0);
                let a = rng.range(0.0, std::f32::consts::TAU);
                let r = rng.range(0.0, cr.radius);
                let (x, z) = ((cr.home.x + a.cos() * r).floor() as i32, (cr.home.z + a.sin() * r).floor() as i32);
                let py = around.player.y as i32;
                if rng.unit() < 0.6
                    && let Some((y, false)) = surface(world, blocks, x, z, py + 64, py - 64)
                {
                    let spot = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
                    if spot.distance(around.player) > spec.shy * 1.5 {
                        cr.state = State::Land;
                        cr.goal = spot;
                    }
                }
            }
            next
        }
        State::Land => {
            let to = cr.goal - pos;
            if startled {
                take_off(c, cr, rng, sounds);
                return pos;
            }
            if to.length() < 0.3 {
                cr.state = State::Rest;
                cr.timer = rng_in(rng, spec.rest);
                cr.call = cr.call.min(rng.range(0.5, 2.0));
                c.vel = Vec3::ZERO;
                return cr.goal;
            }
            // Glide in, slowing as it gets close.
            let speed = k.speed.min((to.length() * 1.2).max(1.5));
            let next = steer(c, pos, cr.goal, speed, 3.0, dt);
            face_along(c, c.vel);
            next
        }
        State::Rest => {
            // Little hops and turns on the spot.
            cr.look -= dt;
            if cr.look <= 0.0 {
                cr.look = rng.range(0.8, 3.0);
                c.yaw += rng.range(-1.2, 1.2);
                c.vel.y = 2.5;
            }
            let mut next = pos;
            next.y += c.vel.y * dt;
            c.vel.y -= 25.0 * dt;
            if next.y <= cr.goal.y {
                next.y = cr.goal.y;
                c.vel.y = 0.0;
            }
            let perch_gone = !solid(world, blocks, cr.goal - Vec3::Y * 0.5);
            if cr.timer <= 0.0 || startled || perch_gone || c.leaving {
                take_off(c, cr, rng, sounds);
            }
            next
        }
        State::Leave | State::Hop => {
            let next = steer(c, pos, pos + cr.drift * 10.0 + Vec3::Y * 4.0, k.run, 1.5, dt);
            face_along(c, c.vel);
            next
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn hopper(c: &mut Creature, cr: &mut Critter, spec: &CritterSpec, pos: Vec3, startled: bool, around: &Around, world: &World, blocks: &Blocks, rng: &mut Rng) {
    let dt = around.dt;
    cr.timer -= dt;
    let water = blocks.registry.id("water");
    let swimming = water.is_some() && world.block(BlockPos::from_world((pos + Vec3::Y * 0.1).to_array())) == water;
    match cr.state {
        State::Hop => {
            // Landed: sit still.
            if (c.on_ground || swimming) && cr.timer < 0.0 {
                cr.state = State::Rest;
                cr.timer = rng_in(rng, spec.rest);
                c.want = Vec3::ZERO;
            }
        }
        _ => {
            if (c.on_ground || swimming) && (cr.timer <= 0.0 || (startled && cr.timer > 0.3)) {
                let dir = if startled {
                    flat(pos - around.player).normalize_or(Vec3::X)
                } else if flat(cr.home - pos).length() > 6.0 {
                    flat(cr.home - pos).normalize()
                } else {
                    let a = rng.range(0.0, std::f32::consts::TAU);
                    Vec3::new(a.cos(), 0.0, a.sin())
                };
                let far = if startled { 1.4 } else { rng.range(0.7, 1.1) };
                c.want = dir * spec.hop.0 * far;
                c.vel = c.want + Vec3::Y * spec.hop.1;
                c.on_ground = false;
                c.yaw = (-dir.x).atan2(-dir.z);
                cr.state = State::Hop;
                // Give it a moment to leave the ground before checking for landing.
                cr.timer = 0.15;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drifter(
    c: &mut Creature,
    cr: &mut Critter,
    k: &Kind,
    spec: &CritterSpec,
    pos: Vec3,
    startled: bool,
    around: &Around,
    world: &World,
    blocks: &Blocks,
    rng: &mut Rng,
) -> Vec3 {
    let dt = around.dt;
    cr.timer -= dt;
    // Moths make for the nearest light close by.
    let light = spec.to_light.then(|| around.lights.iter().copied().filter(|l| l.distance(pos) < 16.0).min_by(|a, b| a.distance(pos).total_cmp(&b.distance(pos)))).flatten();
    let pick = |cr: &mut Critter, rng: &mut Rng| {
        cr.goal = if let Some(l) = light {
            let a = rng.range(0.0, std::f32::consts::TAU);
            // Keep out of your face when the light is the torch in your hand.
            let held = l.distance(around.player + Vec3::Y) < 0.5;
            let r = if held { rng.range(1.8, 3.0) } else { rng.range(0.4, 1.3) };
            l + Vec3::new(a.cos(), 0.0, a.sin()) * r + Vec3::Y * rng.range(-0.5, 0.7)
        } else {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let p = cr.home + Vec3::new(a.cos(), 0.0, a.sin()) * rng.range(0.5, 5.0);
            let y0 = cr.home.y as i32;
            match surface(world, blocks, p.x.floor() as i32, p.z.floor() as i32, y0 + 4, y0 - 8) {
                Some((y, _)) => Vec3::new(p.x, y as f32 + rng_in(rng, spec.height), p.z),
                None => pos,
            }
        };
        if solid(world, blocks, cr.goal) {
            cr.goal = pos;
        }
        cr.state = State::Fly;
        cr.timer = 6.0;
    };
    if startled && cr.state != State::Hop {
        let away = flat(pos - around.player).normalize_or(Vec3::X);
        cr.goal = pos + away * 4.0 + Vec3::Y * 0.5;
        cr.state = State::Hop; // darting away; picks a new goal once there
        cr.timer = 1.5;
    }
    let (speed, agility) = match (cr.state, light.is_some()) {
        (State::Hop, _) | (_, true) => (k.run, 8.0),
        _ => (k.speed, 3.0),
    };
    match cr.state {
        State::Rest => {
            // Hover: hang in the air with a little jitter.
            c.vel *= (1.0 - 6.0 * dt).max(0.0);
            c.vel += Vec3::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)) * dt * 2.0;
            if cr.timer <= 0.0 {
                pick(cr, rng);
            }
            pos + c.vel * dt
        }
        _ => {
            let near = cr.goal.distance(pos) < 0.35;
            if near || cr.timer <= 0.0 {
                if near && spec.rest.1 > 0.0 && light.is_none() {
                    cr.state = State::Rest;
                    cr.timer = rng_in(rng, spec.rest);
                } else {
                    pick(cr, rng);
                }
            }
            // A wavering path rather than a straight line.
            let wobble = Vec3::new((around.t * 2.3 + cr.seed * 9.0).sin(), (around.t * 3.1 + cr.seed * 5.0).sin() * 0.6, (around.t * 1.9 + cr.seed * 7.0).cos()) * speed * 0.35;
            let next = steer(c, pos, cr.goal + wobble * 0.3, speed, agility, dt) + wobble * dt;
            face_along(c, c.vel);
            if solid(world, blocks, next) { pos } else { next }
        }
    }
}

/// Wings flap (named `wing_*` boxes swing up and down at their inner edge) and
/// glowing boxes blink.
fn animate(time: Res<Time>, kinds: Option<Res<Kinds>>, critters: Query<(&Creature, &Critter)>, mut parts: Query<(&ChildOf, &Part, &mut Transform, &mut Visibility)>) {
    let Some(kinds) = kinds else { return };
    let t = time.elapsed_secs();
    for (parent, part, mut tf, mut vis) in &mut parts {
        let Ok((c, cr)) = critters.get(parent.parent()) else { continue };
        let k = kinds.get(c.kind);
        let Some(spec) = &k.critter else { continue };
        let b = &k.model[part.index];
        let offset = Vec3::new(b.offset.0, b.offset.1, b.offset.2);
        if b.name.starts_with("wing") {
            let phase = t + cr.seed * 10.0;
            let angle = match (spec.motion, cr.state) {
                (Motion::Bird, State::Rest) => 0.0,
                (Motion::Bird, State::Land) => (phase * 9.0).sin() * 0.35 + 0.3,
                // Flap, with stretches of gliding.
                (Motion::Bird, _) => {
                    let gliding = (phase * 0.45).sin() > 0.35 && cr.state == State::Fly;
                    if gliding { 0.08 } else { (phase * 15.0).sin() * 0.85 }
                }
                (_, State::Rest) if spec.motion == Motion::Hopper => 0.0,
                _ => (phase * 38.0).sin() * 0.7,
            };
            let side = b.offset.0.signum();
            let pivot = Vec3::new(offset.x - side * b.size.0 * 0.5, offset.y, offset.z);
            let rot = Quat::from_rotation_z(angle * side);
            tf.translation = pivot + rot * (offset - pivot);
            tf.rotation = rot;
        }
        if b.glow {
            // Slow pulses, each firefly on its own beat.
            let beat = (t * (1.2 + cr.seed * 1.4) + cr.seed * 20.0).sin();
            let want = if beat > 0.35 { Visibility::Inherited } else { Visibility::Hidden };
            if *vis != want {
                *vis = want;
            }
        }
    }
}

/// Calls: chirps, croaks and trills, panned to where they come from and fading
/// with distance. Birds mostly call from the ground.
fn calls(
    time: Res<Time>,
    kinds: Option<Res<Kinds>>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut q: Query<(&Transform, &Creature, &mut Critter)>,
    mut sounds: MessageWriter<PlaySound>,
    mut dice: ResMut<Dice>,
    mut budget: Local<f32>,
) {
    let Some(kinds) = kinds else { return };
    let dt = time.delta_secs();
    // No more than a handful of calls a second, however many critters are about.
    *budget = (*budget + dt * 6.0).min(3.0);
    let rng = &mut dice.0;
    let ear = cam.translation;
    for (tf, c, mut cr) in &mut q {
        let Some(spec) = &kinds.get(c.kind).critter else { continue };
        cr.call -= dt;
        if cr.call > 0.0 {
            continue;
        }
        cr.call = rng_in(rng, spec.call_every);
        let in_mood = match (spec.motion, cr.state) {
            (Motion::Bird, State::Rest) => true,
            (Motion::Bird, State::Fly) => rng.unit() < 0.25,
            (Motion::Bird, _) => false,
            _ => true,
        };
        let d = tf.translation.distance(ear);
        if !in_mood || c.leaving || c.fade < 0.5 || d > spec.hearing || *budget < 1.0 {
            continue;
        }
        let Some(name) = rng.pick(&spec.calls) else { continue };
        let gain = (1.0 - d / spec.hearing).powf(1.5);
        if gain > 0.02 {
            *budget -= 1.0;
            sounds.write(PlaySound::ambient_at(name.clone(), tf.translation, gain));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critters_keep_their_hours_and_weather() {
        let k = super::super::data::tests::load();
        let get = |n: &str| k.get(k.id(n).unwrap());
        let ok = |n: &str, night: bool, temp: f32, rain: f32| {
            let c = get(n);
            welcome(c.critter.as_ref().unwrap(), c.spawn.as_ref().unwrap(), night, temp, rain)
        };
        assert!(ok("marsh_finch", false, 0.4, 0.0) && !ok("marsh_finch", true, 0.4, 0.0));
        assert!(!ok("marsh_finch", false, 0.4, 0.8), "birds leave in the rain");
        assert!(ok("firefly", true, 0.4, 0.0) && !ok("firefly", false, 0.4, 0.0));
        assert!(!ok("firefly", true, -0.2, 0.0), "no fireflies in the cold");
        assert!(ok("dusk_cricket", true, 0.4, 0.0) && !ok("dusk_cricket", false, 0.4, 0.0));
        assert!(ok("reed_frog", true, 0.4, 0.8) && ok("reed_frog", false, 0.4, 0.0), "frogs any time, rain or not");
    }

    #[test]
    fn critters_live_in_every_biome_for_now() {
        let k = super::super::data::tests::load();
        for c in k.list.iter().filter(|c| c.critter.is_some()) {
            for biome in ["great_swamp", "meadow", "pine_hills", "dunes", "highlands", "frost_peaks", "skyreach"] {
                assert!(c.spawn.as_ref().unwrap().lives_in(biome), "{} doesn't live in {biome}", c.name);
            }
        }
    }

    #[test]
    fn critters_let_you_come_close() {
        let k = super::super::data::tests::load();
        for c in k.list.iter().filter_map(|c| c.critter.as_ref().map(|s| (&c.name, s))) {
            assert_eq!(c.1.shy, 0.0, "{} startles when you come near", c.0);
        }
    }

    #[test]
    fn orbit_goes_round_home() {
        let home = Vec3::new(10.0, 5.0, -4.0);
        for i in 0..8 {
            let p = orbit_point(home, 12.0, i as f32, 20.0);
            assert!((flat(p - home).length() - 12.0).abs() < 1e-3 && (p.y - 25.0).abs() < 1e-3);
        }
    }

    #[test]
    fn every_critter_with_wings_names_them_left_and_right() {
        let k = super::super::data::tests::load();
        for kind in k.list.iter().filter(|k| k.critter.is_some()) {
            for b in kind.model.iter().filter(|b| b.name.starts_with("wing")) {
                assert!(b.offset.0 != 0.0, "{}: {} sits on the middle line, so it can't swing to a side", kind.name, b.name);
            }
        }
    }
}
