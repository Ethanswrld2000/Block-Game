//! Opening an urn by hand: right click an urn with an empty hand and both
//! hands reach out and take hold of its shoulders while you lean your face in
//! over its mouth; as you come close the urn's tree opens out of the mouth
//! (`tree`). The mouth itself is drawn with the same dark as the tree's
//! background (`assets/shaders/urn_mouth.wesl`, a copy of `urn_void.wesl`'s
//! pattern worked out per screen pixel), so the dark in the pot simply grows
//! over the screen. Closing the tree lets go and leans back.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::window::PrimaryWindow;

use super::tree::TreeUi;
use crate::camera::{FlyCamera, Menus};

/// Seconds to reach out and lean in, and to let go and lean back.
const REACH_SECONDS: f32 = 0.55;
const RELEASE_SECONDS: f32 = 0.35;
/// How far into the lean the tree starts to open.
const OPEN_AT: f32 = 0.8;

/// The urn's mouth: height above its base, and half its width (`models::parts`).
pub const MOUTH_Y: f32 = 0.836;
pub const MOUTH_HALF: f32 = 0.1;
/// Where the face ends up: back from the mouth (the way you came) and above it.
const LEAN_BACK: f32 = 0.28;
const LEAN_UP: f32 = 0.5;
/// Where the hands take hold: the urn's shoulders, either side.
const GRIP_Y: f32 = 0.6;
const GRIP_SIDE: f32 = 0.25;

/// Hands on an urn.
#[derive(Resource, Default)]
pub struct Grab {
    /// The urn (index into urns.ron) and its base.
    urn: Option<(usize, Vec3)>,
    /// 0..1 through the reach (linear; eased where it's used).
    t: f32,
    /// The tree has been opened from this grab.
    opened: bool,
    /// Letting go.
    releasing: bool,
    /// Where the face leans to, fixed when the grab starts.
    view: Transform,
    /// Each hand's grip (right, left) in the view's frame.
    pub grips: [Vec3; 2],
}

impl Grab {
    /// Reach for the urn standing at `base`, from the eye at `eye`.
    pub fn start(&mut self, urn: usize, base: Vec3, eye: Vec3) {
        let view = lean_view(eye, base + Vec3::Y * MOUTH_Y);
        let to_view = view.to_matrix().inverse();
        let grips = grip_points(&view, base).map(|g| to_view.transform_point3(g));
        *self = Self {
            urn: Some((urn, base)),
            t: 0.0,
            opened: false,
            releasing: false,
            view,
            grips,
        };
    }

    /// How far the hands and face are in (0..1, eased).
    pub fn blend(&self) -> f32 {
        if self.urn.is_none() {
            return 0.0;
        }
        smooth(self.t)
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The face over the urn's mouth: back from it the way you came, above it,
/// looking down into it.
pub fn lean_view(eye: Vec3, mouth: Vec3) -> Transform {
    let flat = Vec3::new(eye.x - mouth.x, 0.0, eye.z - mouth.z);
    let back = if flat.length_squared() < 1e-6 {
        Vec3::Z
    } else {
        flat.normalize()
    };
    Transform::from_translation(mouth + back * LEAN_BACK + Vec3::Y * LEAN_UP)
        .looking_at(mouth, Vec3::Y)
}

/// Where each hand (right, left) takes hold of the urn standing at `base`,
/// seen from `view` (world).
pub fn grip_points(view: &Transform, base: Vec3) -> [Vec3; 2] {
    let r = view.right().as_vec3();
    let side = Vec3::new(r.x, 0.0, r.z).normalize_or(Vec3::X);
    let at = base + Vec3::Y * GRIP_Y;
    [at + side * GRIP_SIDE, at - side * GRIP_SIDE]
}

/// Reach, lean, open the tree; let go once it's closed.
pub(super) fn grab_update(
    time: Res<Time<Real>>,
    mut grab: ResMut<Grab>,
    mut tree: ResMut<TreeUi>,
    mut menus: ResMut<Menus>,
    arrange: Res<super::arrange::Arrange>,
    mut cam: Single<&mut Transform, With<FlyCamera>>,
) {
    let Some((urn, base)) = grab.urn else { return };
    let dt = time.delta_secs().min(0.1);
    if grab.opened && tree.open.is_none() {
        grab.releasing = true;
    }
    if grab.releasing {
        // Arranging takes the camera at once.
        let arranging = arrange.start.is_some() || arrange.open.is_some();
        grab.t = if arranging {
            0.0
        } else {
            grab.t - dt / RELEASE_SECONDS
        };
        if grab.t <= 0.0 {
            grab.urn = None;
            if !arranging {
                menus.still = false;
            }
            return;
        }
    } else {
        grab.t = (grab.t + dt / REACH_SECONDS).min(1.0);
        if !grab.opened && grab.t >= OPEN_AT {
            grab.opened = true;
            tree.open = Some(urn);
            tree.at = Some(base + Vec3::Y * MOUTH_Y);
            tree.dirty = true;
        }
    }
    let k = smooth(grab.t);
    cam.translation = cam.translation.lerp(grab.view.translation, k);
    cam.rotation = cam.rotation.slerp(grab.view.rotation, k);
}

/// The dark in an urn's mouth (see the module notes).
pub type MouthMaterial = ExtendedMaterial<StandardMaterial, MouthExtension>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct MouthExtension {
    #[uniform(100)]
    pub mouth: MouthParams,
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct MouthParams {
    /// Clock (seconds), screen's short side (pixels), pixel size.
    pub look: Vec4,
    /// The urn's colour (linear).
    pub tint: Vec4,
    /// The mouth's middle on screen and its radius (pixels).
    pub centre: Vec4,
}

impl MaterialExtension for MouthExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/urn_mouth.wesl".into()
    }
}

/// An urn's mouth, drawn with `MouthMaterial`.
#[derive(Component)]
pub struct Mouth;

/// A new mouth material in the urn's colour.
pub fn mouth_material(color: (f32, f32, f32)) -> MouthMaterial {
    let glow = LinearRgba::from(Color::srgb(color.0, color.1, color.2));
    ExtendedMaterial {
        base: StandardMaterial {
            unlit: true,
            ..default()
        },
        extension: MouthExtension {
            mouth: MouthParams {
                tint: Vec4::new(glow.red, glow.green, glow.blue, 1.0),
                ..default()
            },
        },
    }
}

/// The clock the urn's dark runs on, shared by the mouth and the tree.
pub fn void_clock(time: &Time<Real>) -> f32 {
    time.elapsed_secs_wrapped()
}

/// Keep each mouth's dark lined up with the screen, as the tree's is.
pub(super) fn mouth_look(
    time: Res<Time<Real>>,
    cam: Single<(&Camera, &GlobalTransform), With<FlyCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mouths: Query<(&GlobalTransform, &MeshMaterial3d<MouthMaterial>), With<Mouth>>,
    mut mats: ResMut<Assets<MouthMaterial>>,
) {
    let (camera, ct) = *cam;
    let scale = window.scale_factor();
    let short = window.physical_width().min(window.physical_height()) as f32;
    let t = void_clock(&time);
    for (gt, mat) in &mouths {
        let at = gt.translation();
        let (Ok(p), Ok(q)) = (
            camera.world_to_viewport(ct, at),
            camera.world_to_viewport(ct, at + ct.right() * MOUTH_HALF),
        ) else {
            continue;
        };
        let Some(mut m) = mats.get_mut(&mat.0) else {
            continue;
        };
        let mouth = &mut m.extension.mouth;
        mouth.look = Vec4::new(t, short, 4.0 * scale, 0.0);
        mouth.centre = Vec4::new(p.x * scale, p.y * scale, (q - p).length() * scale, 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_leans_over_the_mouth_from_where_you_stood() {
        let mouth = Vec3::new(0.0, MOUTH_Y, 0.0);
        let view = lean_view(Vec3::new(2.0, 1.6, 0.0), mouth);
        // On your side of the urn, above it, looking down into the mouth.
        assert!(view.translation.x > 0.0 && view.translation.y > mouth.y);
        let to_mouth = (mouth - view.translation).normalize();
        assert!(view.forward().dot(to_mouth) > 0.999);
    }

    #[test]
    fn the_hands_hold_either_side_in_view() {
        let base = Vec3::new(5.0, 10.0, -3.0);
        let view = lean_view(Vec3::new(5.0, 11.6, 0.0), base + Vec3::Y * MOUTH_Y);
        let to_view = view.to_matrix().inverse();
        let [r, l] = grip_points(&view, base).map(|g| to_view.transform_point3(g));
        // Right hand on the right, left on the left, both ahead of the eye.
        assert!(r.x > 0.1 && l.x < -0.1);
        assert!(r.z < 0.0 && l.z < 0.0);
        assert!((r.y - l.y).abs() < 1e-4);
    }
}
