//! The camera: mouse look always, and free flight when the player presses F
//! (see `player` for walking).
//!
//! Controls: click to capture the mouse, Esc to release it.
//! WASD to move, Space/Shift up/down, Ctrl to go fast, mouse wheel to change
//! the base speed (from a crawl up to about 2000 blocks a second).

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

pub struct FlyCameraPlugin;

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Menus>()
            .add_systems(crate::menu::GameStartup, spawn_camera)
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
    }
}
