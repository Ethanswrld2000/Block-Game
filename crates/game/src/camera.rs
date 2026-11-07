//! Free-fly debug camera. The walking player controller arrives in phase 5.
//!
//! Controls: click to capture the mouse, Esc to release it.
//! WASD to move, Space/Shift up/down, Ctrl to go fast.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

pub struct FlyCameraPlugin;

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, (grab_cursor, look, fly).chain());
    }
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
        Self { speed: 12.0, fast_multiplier: 6.0, sensitivity: 0.002, yaw: -0.6, pitch: -0.45 }
    }
}

fn spawn_camera(mut commands: Commands) {
    let cam = FlyCamera::default();
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 40.0, 0.0)
            .with_rotation(Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0)),
        Projection::Perspective(PerspectiveProjection { far: crate::sky::SKY_RADIUS * 1.2, ..default() }),
        DistanceFog { color: Color::srgb(0.46, 0.53, 0.47), ..default() },
        cam,
    ));
}

pub fn grab_cursor(
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if mouse.just_pressed(MouseButton::Left) {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

fn look(
    motion: Res<AccumulatedMouseMotion>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    mut q: Query<(&mut Transform, &mut FlyCamera)>,
) {
    if cursor.grab_mode == CursorGrabMode::None || motion.delta == Vec2::ZERO {
        return;
    }
    for (mut tf, mut cam) in &mut q {
        cam.yaw -= motion.delta.x * cam.sensitivity;
        cam.pitch = (cam.pitch - motion.delta.y * cam.sensitivity).clamp(-1.54, 1.54);
        tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    }
}

fn fly(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut q: Query<(&mut Transform, &FlyCamera)>) {
    for (mut tf, cam) in &mut q {
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
