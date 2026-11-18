//! The camera: mouse look always, and free flight when the player presses F
//! (see `player` for walking).
//!
//! Controls: click to capture the mouse, Esc to release it.
//! WASD to move, Space/Shift up/down, Ctrl to go fast, mouse wheel to change
//! the base speed (from a crawl up to about 2000 blocks a second).
//! P switches between first and third person (`View`).

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

pub struct FlyCameraPlugin;

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menus>()
            .insert_resource(View { third: std::env::var("BLOCKGAME_VIEW").is_ok_and(|v| v == "third"), ..default() })
            .add_systems(crate::menu::GameStartup, spawn_camera)
            // Gameplay always sees the camera at the eyes: it is pulled back
            // for drawing only, and put back before the next frame's systems.
            .add_systems(PreUpdate, back_to_eyes.run_if(crate::menu::in_game))
            .add_systems(PostUpdate, pull_back.run_if(crate::menu::in_game).before(bevy::transform::TransformSystems::Propagate))
            .add_systems(crate::menu::GameUpdate, toggle_view)
            .add_systems(
            crate::menu::GameUpdate,
            (look, fly.run_if(resource_equals(crate::player::MoveMode::Fly))).chain(),
        )
        // After clicks are handled, so the click that captures the mouse
        // doesn't also break a block.
        .add_systems(crate::menu::GameUpdate, grab_cursor.after(crate::interact::InteractSet));
    }
}

/// A screen such as the inventory is open: the mouse is free and doesn't turn the view.
#[derive(Resource, Default, Debug)]
pub struct Menus {
    pub open: bool,
    /// Standing still at a bench: no walking.
    pub still: bool,
}

/// First or third person. In third person the camera sits behind your head
/// (closer when a wall is in the way) and your body shows; aiming still goes
/// from your eyes along the view, which passes through the crosshair.
#[derive(Resource, Default, Debug)]
pub struct View {
    pub third: bool,
    /// The camera is pulled back this frame (third person and not at a bench
    /// or in a talk, which place the camera themselves).
    pub pulled: bool,
    /// How far behind the eyes, eased.
    back: f32,
    /// Where the eyes are while the camera is pulled back.
    eye: Option<Vec3>,
}

/// Blocks behind the head in third person.
pub const THIRD_PERSON_BACK: f32 = 4.0;

fn toggle_view(keys: Res<ButtonInput<KeyCode>>, menus: Res<Menus>, mut view: ResMut<View>) {
    if keys.just_pressed(KeyCode::KeyP) && !menus.open {
        view.third = !view.third;
        view.back = 0.0;
    }
}

fn back_to_eyes(mut view: ResMut<View>, mut cam: Query<&mut Transform, With<FlyCamera>>) {
    if let Some(eye) = view.eye.take()
        && let Ok(mut tf) = cam.single_mut()
    {
        tf.translation = eye;
    }
}

fn pull_back(time: Res<Time<Real>>, menus: Res<Menus>, world: Res<crate::world::World>, blocks: Res<crate::blocks::Blocks>, mut view: ResMut<View>, mut cam: Query<&mut Transform, With<FlyCamera>>) {
    let Ok(mut tf) = cam.single_mut() else { return };
    view.pulled = view.third && !menus.still;
    if !view.pulled {
        view.back = 0.0;
        return;
    }
    let eye = tf.translation;
    let back = tf.back().as_vec3();
    // Stop short of the first solid block behind the head.
    let solid = |p: Vec3| world.block(voxel::BlockPos::from_world(p.to_array())).is_some_and(|b| blocks.registry.get(b).solid);
    let mut room = THIRD_PERSON_BACK;
    let mut d = 0.1;
    while d <= THIRD_PERSON_BACK {
        if solid(eye + back * d) {
            room = (d - 0.3).max(0.0);
            break;
        }
        d += 0.1;
    }
    // Snap in when a wall comes between, ease back out when it's gone.
    let ease = 1.0 - (-time.delta_secs() * 6.0).exp();
    view.back = if room < view.back { room } else { view.back + (room - view.back) * ease };
    view.eye = Some(eye);
    tf.translation = eye + back * view.back;
}

#[derive(Component)]
pub struct FlyCamera {
    pub speed: f32,
    pub fast_multiplier: f32,
    pub sensitivity: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self { speed: 12.0, fast_multiplier: 5.0, sensitivity: 0.002, yaw: -0.6, pitch: -0.45 }
    }
}

fn spawn_camera(mut commands: Commands) {
    let cam = FlyCamera::default();
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 40.0, 0.0)
            .with_rotation(Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0)),
        Projection::Perspective(PerspectiveProjection { near: 0.05, far: crate::sky::SKY_RADIUS * 1.2, ..default() }),
        DistanceFog { color: Color::srgb(0.46, 0.53, 0.47), ..default() },
        // HDR so bright things (sun, torches, fireflies) can bloom; grading
        // and bloom are set by `atmosphere`.
        bevy::camera::Hdr,
        tonemapping(),
        bevy::render::view::ColorGrading::default(),
        crate::atmosphere::bloom(),
        // Ears for panned sounds (critter calls).
        bevy::audio::SpatialListener::new(0.3),
        cam,
    ));
}

/// The tone curve. `BLOCKGAME_TONEMAP=tony|agx|aces|filmic|reinhard` to compare.
fn tonemapping() -> bevy::render::view::Tonemapping {
    use bevy::render::view::Tonemapping as T;
    match std::env::var("BLOCKGAME_TONEMAP").as_deref() {
        Ok("tony") => T::TonyMcMapface,
        Ok("agx") => T::AgX,
        Ok("aces") => T::AcesFitted,
        Ok("filmic") => T::BlenderFilmic,
        Ok("reinhard") => T::ReinhardLuminance,
        _ => T::TonyMcMapface,
    }
}

pub fn grab_cursor(
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    menus: Res<Menus>,
) {
    if menus.open {
        if cursor.grab_mode != CursorGrabMode::None {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    // Esc opens the pause menu (see `settings`), which frees the mouse.
    let _ = keys;
}

pub fn look(
    motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    menus: Res<Menus>,
    mut q: Query<(&mut Transform, &mut FlyCamera)>,
) {
    if menus.open || cursor.grab_mode == CursorGrabMode::None || motion.delta == Vec2::ZERO {
        return;
    }
    for (mut tf, mut cam) in &mut q {
        cam.yaw -= motion.delta.x * cam.sensitivity;
        cam.pitch = (cam.pitch - motion.delta.y * cam.sensitivity).clamp(-1.54, 1.54);
        tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    }
}

/// Slowest and fastest base flying speeds, blocks per second.
pub const MIN_SPEED: f32 = 2.0;
pub const MAX_SPEED: f32 = 400.0;

pub fn fly(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    mut q: Query<(&mut Transform, &mut FlyCamera)>,
) {
    for (mut tf, mut cam) in &mut q {
        if scroll.delta.y != 0.0 {
            // Each notch is 25% faster or slower.
            let notches = scroll.delta.y.clamp(-5.0, 5.0);
            cam.speed = (cam.speed * 1.25f32.powf(notches)).clamp(MIN_SPEED, MAX_SPEED);
        }
        let forward = tf.forward().as_vec3();
        let right = tf.right().as_vec3();
        let mut dir = Vec3::ZERO;
        if keys.pressed(KeyCode::KeyW) { dir += forward; }
        if keys.pressed(KeyCode::KeyS) { dir -= forward; }
        if keys.pressed(KeyCode::KeyD) { dir += right; }
        if keys.pressed(KeyCode::KeyA) { dir -= right; }
        if keys.pressed(KeyCode::Space) { dir += Vec3::Y; }
        if keys.pressed(KeyCode::ShiftLeft) { dir -= Vec3::Y; }
        let mut speed = cam.speed;
        if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::SuperLeft) {
            speed *= cam.fast_multiplier;
        }
        tf.translation += dir.normalize_or_zero() * speed * time.delta_secs();
        // The world ends at its ceiling; flying stops just under it.
        tf.translation.y = tf.translation.y.min(worldgen::WORLD_CEILING as f32);
    }
}
