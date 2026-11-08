//! The player on foot: gravity, jumping, collision with blocks, sprinting,
//! crouching and swimming, plus how the camera rides on top (a weighty
//! head-bob, a dip when you land) and fall damage.
//!
//! F switches between walking and the free-flying camera. The player's state
//! lives in `Player`, separate from the camera, so a server can run the same
//! movement later.

use std::f32::consts::PI;

use bevy::prelude::*;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::world::World;

pub const HALF_WIDTH: f32 = 0.3;
pub const HEIGHT: f32 = 1.8;
pub const CROUCH_HEIGHT: f32 = 1.5;
pub const EYE: f32 = 1.62;
pub const CROUCH_EYE: f32 = 1.27;
const GRAVITY: f32 = 30.0;
const JUMP_SPEED: f32 = 9.2;
const WALK: f32 = 4.4;
const SPRINT: f32 = 6.8;
const CROUCH: f32 = 1.8;
const SWIM: f32 = 2.6;
const MAX_FALL: f32 = 60.0;
/// Landing faster than this hurts (about a 3.5 block drop).
pub const SAFE_LANDING: f32 = 14.0;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MoveMode::Walk)
            .init_resource::<Player>()
            .init_resource::<Body>()
            .add_message::<Damage>()
            .add_systems(
                Update,
                (
                    toggle_mode,
                    walk.run_if(resource_equals(MoveMode::Walk)),
                    ride_camera.run_if(resource_equals(MoveMode::Walk)),
                    follow_fly_camera.run_if(resource_equals(MoveMode::Fly)),
                )
                    .chain()
                    .in_set(PlayerSet)
                    .after(crate::camera::look)
                    .after(crate::camera::fly),
            );
    }
}

/// Movement and camera placement; other systems that need this frame's
/// camera run after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSet;

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveMode {
    Walk,
    Fly,
}

/// Something hurt the player in one blow. `survival` decides which health
/// bar it lands on from the cause.
#[derive(Message, Debug, Clone, Copy)]
pub struct Damage {
    pub amount: f32,
    pub cause: DamageCause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageCause {
    /// A short fall: bruises.
    Fall,
    /// A fall from higher than `HEAVY_FALL` blocks: straight to Injuries.
    HeavyFall,
    /// The cold touch of a spirit: wears down Fatigue.
    Chill,
    /// Stabs and arrows: start at Blood and bleed.
    Piercing,
    /// Clubs, fists, falling rocks.
    Blunt,
}

/// A fall from higher than this many blocks goes straight to Injuries.
pub const HEAVY_FALL: f32 = 12.0;

/// How the body's state changes movement, set by `survival` each frame.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Body {
    /// Multiplies walking, running and swimming speed.
    pub speed: f32,
    /// Multiplies fall damage (heavy loads land harder).
    pub fall: f32,
    /// 0..1: lack of air makes the view sway.
    pub dizzy: f32,
    /// No moving at all (dead, or asleep).
    pub frozen: bool,
}

impl Default for Body {
    fn default() -> Self {
        Self { speed: 1.0, fall: 1.0, dizzy: 0.0, frozen: false }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct Player {
    /// Centre of the feet.
    pub pos: Vec3,
    pub vel: Vec3,
    pub on_ground: bool,
    pub in_water: bool,
    pub head_in_water: bool,
    pub crouching: bool,
    pub sprinting: bool,
    /// Eye height, eased between standing and crouching.
    pub eye: f32,
    /// Footstep cycle (radians) and how strongly the head bobs (0..1.5).
    pub bob_phase: f32,
    pub bob: f32,
    /// Landing dip: camera offset (blocks) and its speed, a damped spring.
    pub dip: f32,
    dip_vel: f32,
    /// The world under the player has loaded at least once.
    pub ready: bool,
    pub last_damage: Option<Damage>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            pos: Vec3::new(0.0, 40.0, 0.0),
            vel: Vec3::ZERO,
            on_ground: false,
            in_water: false,
            head_in_water: false,
            crouching: false,
            sprinting: false,
            eye: EYE,
            bob_phase: 0.0,
            bob: 0.0,
            dip: 0.0,
            dip_vel: 0.0,
            ready: false,
            last_damage: None,
        }
    }
}

impl Player {
    pub fn height(&self) -> f32 {
        if self.crouching { CROUCH_HEIGHT } else { HEIGHT }
    }

    /// Does the player's box overlap this block?
    pub fn overlaps(&self, b: BlockPos) -> bool {
        let (min, max) = (self.pos - Vec3::new(HALF_WIDTH, 0.0, HALF_WIDTH), self.pos + Vec3::new(HALF_WIDTH, self.height(), HALF_WIDTH));
        let (bx, by, bz) = (b.x as f32, b.y as f32, b.z as f32);
        min.x < bx + 1.0 && max.x > bx && min.y < by + 1.0 && max.y > by && min.z < bz + 1.0 && max.z > bz
    }
}

fn toggle_mode(keys: Res<ButtonInput<KeyCode>>, mut mode: ResMut<MoveMode>, mut player: ResMut<Player>) {
    if keys.just_pressed(KeyCode::KeyF) {
        *mode = match *mode {
            MoveMode::Walk => MoveMode::Fly,
            MoveMode::Fly => MoveMode::Walk,
        };
        player.vel = Vec3::ZERO;
        info!("movement: {:?}", *mode);
    }
}

/// What the world is like at a block, for movement.
#[derive(Clone, Copy, PartialEq)]
enum Cell {
    Open,
    Water,
    Solid,
    /// Not loaded yet: treated as solid so nobody falls out of the world.
    Unknown,
}

fn cell(world: &World, blocks: &Blocks, p: BlockPos, water: Option<voxel::BlockId>) -> Cell {
    match world.block(p) {
        None => Cell::Unknown,
        Some(b) if Some(b) == water => Cell::Water,
        Some(b) if blocks.registry.get(b).solid => Cell::Solid,
        Some(_) => Cell::Open,
    }
}

/// Blocks overlapped by a box.
pub(crate) fn blocks_in(min: Vec3, max: Vec3) -> impl Iterator<Item = BlockPos> {
    let (a, b) = (min.floor().as_ivec3(), (max - Vec3::splat(1e-4)).floor().as_ivec3());
    (a.y..=b.y).flat_map(move |y| (a.z..=b.z).flat_map(move |z| (a.x..=b.x).map(move |x| BlockPos::new(x, y, z))))
}

fn hits(world: &World, blocks: &Blocks, pos: Vec3, height: f32, water: Option<voxel::BlockId>) -> bool {
    let min = pos - Vec3::new(HALF_WIDTH, 0.0, HALF_WIDTH);
    let max = pos + Vec3::new(HALF_WIDTH, height, HALF_WIDTH);
    blocks_in(min, max).any(|b| matches!(cell(world, blocks, b, water), Cell::Solid | Cell::Unknown))
}

/// Move along one axis, stopping flush against blocks. Returns true if blocked.
fn move_axis(world: &World, blocks: &Blocks, p: &mut Player, axis: usize, amount: f32, water: Option<voxel::BlockId>) -> bool {
    if amount == 0.0 {
        return false;
    }
    let mut next = p.pos;
    next[axis] += amount;
    if !hits(world, blocks, next, p.height(), water) {
        p.pos = next;
        return false;
    }
    // Slide up to the face of the block we ran into.
    let extent = if axis == 1 {
        if amount > 0.0 { p.height() } else { 0.0 }
    } else if amount > 0.0 {
        HALF_WIDTH
    } else {
        -HALF_WIDTH
    };
    let edge = p.pos[axis] + extent + amount;
    let face = if amount > 0.0 { edge.floor() } else { edge.floor() + 1.0 };
    let flush = face - extent - amount.signum() * 1e-3;
    let mut snapped = p.pos;
    snapped[axis] = flush;
    if (flush - p.pos[axis]).signum() == amount.signum() && !hits(world, blocks, snapped, p.height(), water) {
        p.pos = snapped;
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn walk(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    world: Res<World>,
    blocks: Res<Blocks>,
    cam: Query<&FlyCamera>,
    body: Res<Body>,
    conditions: Res<crate::weather::Conditions>,
    mut player: ResMut<Player>,
    mut damage: MessageWriter<Damage>,
) {
    let Ok(look) = cam.single() else { return };
    if body.frozen {
        player.vel = Vec3::ZERO;
        player.sprinting = false;
        return;
    }
    let p = &mut *player;
    let water = blocks.registry.id("water");
    let feet = BlockPos::from_world(p.pos.to_array());
    // Wait until the ground under the player has loaded.
    if !p.ready {
        let below = BlockPos::new(feet.x, feet.y - 1, feet.z);
        if world.block(below).is_none() || world.block(feet).is_none() {
            return;
        }
        p.ready = true;
    }
    let dt = time.delta_secs().min(0.1);

    // Where the body is.
    let mid = BlockPos::from_world((p.pos + Vec3::Y * 0.4).to_array());
    let head = BlockPos::from_world((p.pos + Vec3::Y * p.eye).to_array());
    p.in_water = cell(&world, &blocks, mid, water) == Cell::Water;
    p.head_in_water = cell(&world, &blocks, head, water) == Cell::Water;

    // Crouch (and stand back up only if there's room).
    let want_crouch = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::KeyC);
    if want_crouch {
        p.crouching = true;
    } else if p.crouching && !hits(&world, &blocks, p.pos, HEIGHT, water) {
        p.crouching = false;
    }
    let target_eye = if p.crouching { CROUCH_EYE } else { EYE };
    p.eye += (target_eye - p.eye) * (1.0 - (-dt * 14.0).exp());

    // Wanted direction on the ground plane.
    let (sy, cy) = look.yaw.sin_cos();
    let forward = Vec3::new(-sy, 0.0, -cy);
    let right = Vec3::new(cy, 0.0, -sy);
    let mut wish = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) { wish += forward; }
    if keys.pressed(KeyCode::KeyS) { wish -= forward; }
    if keys.pressed(KeyCode::KeyD) { wish += right; }
    if keys.pressed(KeyCode::KeyA) { wish -= right; }
    let wish = wish.normalize_or_zero();
    p.sprinting = keys.pressed(KeyCode::ShiftLeft) && keys.pressed(KeyCode::KeyW) && !p.crouching;
    let speed = if p.in_water {
        SWIM
    } else if p.crouching {
        CROUCH
    } else if p.sprinting {
        SPRINT
    } else {
        WALK
    } * body.speed;
    // Quick to start and stop on the ground; little control in the air.
    let accel = if p.on_ground { 45.0 } else if p.in_water { 12.0 } else { 9.0 };
    let horiz = Vec2::new(p.vel.x, p.vel.z);
    // Strong wind pushes you about when nothing overhead shelters you.
    let gust = (conditions.wind - 8.0).max(0.0) * 0.15;
    let sheltered = gust == 0.0
        || p.in_water
        || (2..26).any(|dy| matches!(cell(&world, &blocks, BlockPos::new(feet.x, feet.y + dy, feet.z), water), Cell::Solid | Cell::Unknown));
    let push = if sheltered { Vec2::ZERO } else { crate::weather::WIND_DIR * gust };
    let target = Vec2::new(wish.x, wish.z) * speed + push;
    let new = horiz.move_towards(target, accel * dt);
    p.vel.x = new.x;
    p.vel.z = new.y;

    // Vertical.
    let jump = keys.pressed(KeyCode::Space);
    if p.in_water {
        // Float, sink slowly, swim up with Space.
        p.vel.y -= GRAVITY * 0.25 * dt;
        p.vel.y *= 1.0 - (dt * 3.0).min(1.0);
        if jump {
            p.vel.y = p.vel.y.max(3.8);
        }
    } else {
        p.vel.y = (p.vel.y - GRAVITY * dt).max(-MAX_FALL);
        if jump && p.on_ground {
            p.vel.y = JUMP_SPEED;
        }
    }

    // Move in small steps so a fast fall can't pass through a block.
    let travel = p.vel * dt;
    let steps = (travel.abs().max_element() / 0.4).ceil().max(1.0) as usize;
    let was_on_ground = p.on_ground;
    let fall_speed = -p.vel.y;
    p.on_ground = false;
    for _ in 0..steps {
        let step = travel / steps as f32;
        if move_axis(&world, &blocks, p, 1, step.y, water) {
            if step.y < 0.0 {
                p.on_ground = true;
            }
            p.vel.y = 0.0;
        }
        for axis in [0, 2] {
            let before = p.pos;
            if move_axis(&world, &blocks, p, axis, step[axis], water) {
                p.vel[axis] = 0.0;
            }
            // Crouching on solid ground: don't step off an edge.
            if p.crouching && was_on_ground && !p.in_water {
                let below = p.pos - Vec3::Y * 0.1;
                if !hits(&world, &blocks, below, 0.05, water) {
                    p.pos = before;
                    p.vel[axis] = 0.0;
                }
            }
        }
    }
    // Still standing on something?
    if !p.on_ground && p.vel.y <= 0.0 && hits(&world, &blocks, p.pos - Vec3::Y * 0.02, 0.01, water) {
        p.on_ground = true;
    }

    // Landing: dip the camera, and hurt on a long fall (water breaks the fall).
    if p.on_ground && !was_on_ground && fall_speed > 3.0 {
        p.dip_vel -= fall_speed * 0.045;
        if fall_speed > SAFE_LANDING && !p.in_water {
            // v^2 = 2 g h
            let height = fall_speed * fall_speed / (2.0 * GRAVITY);
            let cause = if height > HEAVY_FALL { DamageCause::HeavyFall } else { DamageCause::Fall };
            let d = Damage { amount: (fall_speed - SAFE_LANDING) * 1.6 * body.fall, cause };
            p.last_damage = Some(d);
            damage.write(d);
            info!("fall damage {:.1} (landed at {:.1} blocks/s)", d.amount, fall_speed);
        }
    }

    // Head-bob: a footfall every ~1.7 blocks, sharper and heavier when sprinting.
    let ground_speed = Vec2::new(p.vel.x, p.vel.z).length();
    let target_bob = if p.on_ground { (ground_speed / WALK).min(1.5) } else { 0.0 };
    p.bob += (target_bob - p.bob) * (1.0 - (-dt * 8.0).exp());
    p.bob_phase = (p.bob_phase + dt * ground_speed / 1.7 * PI) % (PI * 4.0);
    // Landing dip spring.
    let acc = -140.0 * p.dip - 15.0 * p.dip_vel;
    p.dip_vel += acc * dt;
    p.dip = (p.dip + p.dip_vel * dt).clamp(-0.6, 0.2);
}

/// Put the camera at the player's eyes, with bob, sway and landing dip.
fn ride_camera(time: Res<Time>, player: Res<Player>, body: Res<Body>, mut cam: Query<(&mut Transform, &FlyCamera)>) {
    let Ok((mut tf, look)) = cam.single_mut() else { return };
    let p = &*player;
    let (s, b) = (p.bob_phase.sin(), p.bob);
    // Down on each footfall (|sin|), side to side once per two steps.
    let bob_y = -(s.abs()) * 0.075 * b;
    let sway = (p.bob_phase * 0.5).sin() * 0.04 * b;
    // Short of air: a slow drunken lean.
    let t = time.elapsed_secs();
    let dizzy_roll = (t * 0.9).sin() * 0.09 * body.dizzy;
    let dizzy_yaw = (t * 0.6).sin() * 0.05 * body.dizzy;
    let roll = (p.bob_phase * 0.5).sin() * 0.012 * b + dizzy_roll;
    let rotation = Quat::from_euler(EulerRot::YXZ, look.yaw + dizzy_yaw, look.pitch + s.abs() * 0.006 * b, roll);
    let right = rotation * Vec3::X;
    tf.translation = p.pos + Vec3::Y * (p.eye + bob_y + p.dip) + Vec3::new(right.x, 0.0, right.z) * sway;
    tf.rotation = rotation;
}

/// In free-fly mode the player's body goes where the camera goes.
fn follow_fly_camera(mut player: ResMut<Player>, cam: Query<&Transform, With<FlyCamera>>) {
    if let Ok(tf) = cam.single() {
        player.pos = tf.translation - Vec3::Y * player.eye;
        player.on_ground = false;
        player.bob = 0.0;
        player.dip = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_overlap() {
        let p = Player { pos: Vec3::new(0.5, 10.0, 0.5), ..default() };
        assert!(p.overlaps(BlockPos::new(0, 10, 0)));
        assert!(p.overlaps(BlockPos::new(0, 11, 0)));
        assert!(!p.overlaps(BlockPos::new(0, 12, 0)));
        assert!(!p.overlaps(BlockPos::new(0, 9, 0)));
        assert!(!p.overlaps(BlockPos::new(1, 10, 0)));
    }

    #[test]
    fn box_covers_the_right_blocks() {
        let n = blocks_in(Vec3::new(0.2, 0.0, 0.2), Vec3::new(0.8, 1.8, 0.8)).count();
        assert_eq!(n, 2);
        let n = blocks_in(Vec3::new(-0.3, 0.0, -0.3), Vec3::new(0.3, 1.8, 0.3)).count();
        assert_eq!(n, 8);
    }

    #[test]
    fn safe_landing_is_a_few_blocks() {
        // v^2 = 2 g h
        let h = SAFE_LANDING * SAFE_LANDING / (2.0 * GRAVITY);
        assert!((3.0..4.5).contains(&h), "{h}");
        let jump = JUMP_SPEED * JUMP_SPEED / (2.0 * GRAVITY);
        assert!((1.1..1.6).contains(&jump), "jump {jump}");
    }
}
