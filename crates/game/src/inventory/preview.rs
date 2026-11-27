//! The live picture of you on the inventory's Equipment tab, and the clothes
//! and armor drawn on bodies.
//!
//! A second body (the same rig as your third-person body, `npc::body`) stands
//! on its own render layer, lit by its own lights, filmed by a camera that
//! draws into an image; the Equipment tab shows that image. It holds what
//! your hands hold and wears what you wear, idles, blinks, looks a little
//! towards the cursor, turns when you drag it, and shifts its weight when you
//! put something on.
//!
//! `Dressed` bodies (this one and your third-person body) get a box per worn
//! piece over the matching body part, in the colour of the item's sprite.

use std::collections::{BTreeMap, HashMap};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

use super::ui::{InvScreen, Tab};
use super::Inventory;
use crate::avatar::PlayerLooks;
use crate::hands::Held;
use crate::interact::{Hand, Wielded};
use crate::item_art::ItemArt;
use crate::items::{ItemId, WearSlot};
use crate::npc::body::{self, Anchors, Drive, Parts, Rig};
use crate::npc::dialogue::Mood;
use crate::npc::motion::wrap;
use crate::swing::Limbs;

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::menu::GameStartup, setup).add_systems(
            crate::menu::GameUpdate,
            (spawn_body, dress, layer_parts, drive_preview).chain().after(crate::interact::InteractSet),
        );
    }
}

/// The render layer the picture's body and lights live on.
const LAYER: usize = 3;
/// Where the picture's body stands (far from everything; it's on its own layer anyway).
const AT: Vec3 = Vec3::new(0.0, -4000.0, 0.0);
/// The image's size in pixels (twice the size it's drawn at).
const SIZE: UVec2 = UVec2::new(940, 584);

/// The image the Equipment tab shows.
#[derive(Resource)]
pub struct PreviewImage(pub Handle<Image>);

#[derive(Component)]
struct PreviewCamera;

#[derive(Component)]
struct PreviewBody;

/// The picture's motion: how far it's been turned by dragging, how fast it
/// still turns, and a little flourish when gear changes.
#[derive(Default)]
struct Spin {
    yaw: f32,
    vel: f32,
    flourish: f32,
    seen: u32,
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_fill(
        Extent3d { width: SIZE.x, height: SIZE.y, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let handle = images.add(image);
    let layer = RenderLayers::layer(LAYER);
    commands.spawn((
        PreviewCamera,
        Camera3d::default(),
        Camera { order: -1, is_active: false, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
        RenderTarget::Image(handle.clone().into()),
        Projection::Perspective(PerspectiveProjection { fov: 0.5, ..default() }),
        Transform::from_translation(AT + Vec3::new(0.0, 1.0, -5.0)).looking_at(AT + Vec3::new(0.0, 0.92, 0.0), Vec3::Y),
        AmbientLight { color: Color::srgb(0.75, 0.8, 0.95), brightness: 900.0, ..default() },
        layer.clone(),
        Name::new("Inventory picture camera"),
    ));
    // A warm key light from the front and above, a cool fill from the side.
    commands.spawn((
        DirectionalLight { illuminance: 9000.0, color: Color::srgb(1.0, 0.94, 0.84), shadow_maps_enabled: false, ..default() },
        Transform::from_translation(AT + Vec3::new(1.5, 3.0, -3.0)).looking_at(AT + Vec3::Y, Vec3::Y),
        layer.clone(),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 2500.0, color: Color::srgb(0.6, 0.75, 1.0), shadow_maps_enabled: false, ..default() },
        Transform::from_translation(AT + Vec3::new(-3.0, 1.0, 1.0)).looking_at(AT + Vec3::Y, Vec3::Y),
        layer,
    ));
    commands.insert_resource(PreviewImage(handle));
}

fn spawn_body(
    mut commands: Commands,
    looks: Res<PlayerLooks>,
    existing: Query<(), With<PreviewBody>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !existing.is_empty() {
        return;
    }
    let (root, mut rig) = body::build(&mut commands, &mut meshes, &mut materials, &looks.0, AT, 0.0);
    // Lit by its own little stage, not by the world's light where it happens to be.
    rig.follows_light = false;
    commands.spawn((Held::new(Hand::Right), Transform::from_scale(Vec3::splat(1.6)), Visibility::default(), ChildOf(rig.hands[1])));
    commands.spawn((Held::new(Hand::Left), Transform::from_scale(Vec3::splat(1.6)), Visibility::default(), ChildOf(rig.hands[0])));
    let dressed = Dressed::new(rig.anchors());
    commands.entity(root).insert((PreviewBody, Name::new("Inventory picture body"), rig, dressed, RenderLayers::layer(LAYER)));
}

/// Everything under the picture's body goes on its layer (parts, held items
/// and clothes are added later, so this runs every frame).
fn layer_parts(mut commands: Commands, body: Query<Entity, With<PreviewBody>>, children: Query<&Children>, missing: Query<(), Without<RenderLayers>>) {
    let Ok(root) = body.single() else { return };
    for e in children.iter_descendants(root) {
        if missing.contains(e) {
            commands.entity(e).insert(RenderLayers::layer(LAYER));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_preview(
    real: Res<Time<Real>>,
    screen: Res<InvScreen>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    window: Single<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
    wielded: Res<Wielded>,
    mut cam: Query<&mut Camera, With<PreviewCamera>>,
    mut body: Query<(&mut Transform, &mut Rig), With<PreviewBody>>,
    mut parts: Parts,
    mut spin: Local<Spin>,
    mut dragging: Local<bool>,
) {
    let on = screen.open && screen.tab == Tab::Equipment;
    for mut c in &mut cam {
        if c.is_active != on {
            c.is_active = on;
        }
    }
    let Ok((mut tf, mut rig)) = body.single_mut() else { return };
    if !on {
        return;
    }
    let dt = real.delta_secs().min(0.05);
    // Drag the picture to turn it; it coasts to a stop.
    if mouse.just_pressed(MouseButton::Left) {
        *dragging = screen.over_preview;
    }
    if !mouse.pressed(MouseButton::Left) {
        *dragging = false;
    }
    if *dragging {
        spin.vel = motion.delta.x * 0.012 / dt.max(1e-3);
        spin.yaw += motion.delta.x * 0.012;
    } else {
        spin.yaw += spin.vel * dt;
        spin.vel *= (-dt * 4.0).exp();
    }
    // Put something on: a quick turn and a shrug.
    if spin.seen != screen.changed_gear {
        spin.seen = screen.changed_gear;
        spin.flourish = 1.0;
    }
    spin.flourish = (spin.flourish - dt * 1.4).max(0.0);
    let f = spin.flourish;
    let wiggle = (1.0 - f) * std::f32::consts::TAU * 1.2;
    let yaw = wrap(spin.yaw + 0.35 * f * wiggle.sin());

    // The head follows the cursor a little (the camera is straight ahead, at yaw 0).
    let size = window.size();
    let c = window.cursor_position().map_or(Vec2::ZERO, |p| (p / size - 0.5) * 2.0);
    let focus = [wrap(-yaw) - c.x * 0.5, -c.y * 0.3];
    let arms = wielded.0.map(|_| Limbs::hold());
    let drive = Drive {
        yaw,
        turn: 0.12,
        speed: 0.0,
        walk: 4.4,
        game_dt: 0.0,
        focus: Some(focus),
        gesture: 0.5 * f,
        mood: if f > 0.2 { Mood::Happy } else { Mood::Neutral },
        mouth: (0.0, 0.85),
        speaking: 0.0,
        arms,
        sway: [-0.08 * f * wiggle.cos(), 0.0],
        act: None,
        kick: None,
    };
    body::drive(&mut tf, &mut rig, &mut parts, dt, &drive);
}

/// A body that shows what you wear.
#[derive(Component)]
pub(crate) struct Dressed {
    anchors: Anchors,
    showing: Option<BTreeMap<WearSlot, ItemId>>,
}

impl Dressed {
    pub(crate) fn new(anchors: Anchors) -> Self {
        Self { anchors, showing: None }
    }
}

/// One box of a worn piece (children of a body's joints).
#[derive(Component)]
struct WornPart(Entity);

/// Where a worn piece goes: (which joint, box size, box centre), in the rig's frame.
fn pieces(slot: WearSlot, a: &Anchors) -> Vec<(Entity, Vec3, Vec3)> {
    let both = |j: [Entity; 2], size: Vec3, at: Vec3| j.into_iter().map(move |e| (e, size, at));
    match slot {
        WearSlot::Head => vec![(a.head, Vec3::new(0.56, 0.05, 0.52), Vec3::new(0.0, 0.44, 0.0)), (a.head, Vec3::new(0.40, 0.14, 0.38), Vec3::new(0.0, 0.52, 0.02))],
        WearSlot::HeadArmor => vec![
            (a.head, Vec3::new(0.52, 0.20, 0.48), Vec3::new(0.0, 0.41, 0.0)),
            (a.head, Vec3::new(0.05, 0.22, 0.36), Vec3::new(-0.25, 0.24, 0.05)),
            (a.head, Vec3::new(0.05, 0.22, 0.36), Vec3::new(0.25, 0.24, 0.05)),
            (a.head, Vec3::new(0.06, 0.14, 0.03), Vec3::new(0.0, 0.26, -0.235)),
        ],
        WearSlot::Body => {
            let mut v = vec![(a.torso, Vec3::new(0.54, 0.52, 0.32), Vec3::new(0.0, 0.30, 0.0))];
            v.extend(both(a.shoulders, Vec3::new(0.17, 0.30, 0.18), Vec3::new(0.0, -0.11, 0.0)));
            v
        }
        WearSlot::BodyArmor => {
            let mut v = vec![(a.torso, Vec3::new(0.58, 0.40, 0.36), Vec3::new(0.0, 0.34, 0.0))];
            v.extend(both(a.shoulders, Vec3::new(0.21, 0.11, 0.22), Vec3::new(0.0, 0.0, 0.0)));
            v
        }
        WearSlot::Legs => both(a.legs, Vec3::new(0.23, 0.42, 0.25), Vec3::new(0.0, -0.20, 0.0)).chain(both(a.knees, Vec3::new(0.22, 0.26, 0.24), Vec3::new(0.0, -0.12, 0.0))).collect(),
        WearSlot::Feet => both(a.knees, Vec3::new(0.25, 0.13, 0.33), Vec3::new(0.0, -0.31, -0.035)).collect(),
        WearSlot::Back => vec![(a.torso, Vec3::new(0.38, 0.40, 0.16), Vec3::new(0.0, 0.30, 0.23))],
    }
}

/// Put boxes on each `Dressed` body for what you wear, when that changes.
#[allow(clippy::too_many_arguments)]
fn dress(
    mut commands: Commands,
    inv: Res<Inventory>,
    art: Option<Res<ItemArt>>,
    images: Res<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bodies: Query<(Entity, &mut Dressed)>,
    parts: Query<(Entity, &WornPart)>,
    mut mats: Local<HashMap<ItemId, Handle<StandardMaterial>>>,
) {
    let Some(art) = art else { return };
    for (owner, mut d) in &mut bodies {
        if d.showing.as_ref() == Some(&inv.worn) {
            continue;
        }
        // Wait for the sprites to load: they give the colours.
        let mut ready = true;
        for id in inv.worn.values() {
            if mats.contains_key(id) {
                continue;
            }
            match art.average_color(*id, &images) {
                Some(c) => {
                    mats.insert(*id, materials.add(StandardMaterial { base_color: c, perceptual_roughness: 0.9, ..default() }));
                }
                None => ready = false,
            }
        }
        if !ready {
            continue;
        }
        for (e, p) in &parts {
            if p.0 == owner {
                commands.entity(e).despawn();
            }
        }
        for (slot, id) in &inv.worn {
            for (joint, size, at) in pieces(*slot, &d.anchors) {
                commands.spawn((WornPart(owner), Mesh3d(meshes.add(Cuboid::from_size(size))), MeshMaterial3d(mats[id].clone()), Transform::from_translation(at), ChildOf(joint)));
            }
        }
        d.showing = Some(inv.worn.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slot_has_pieces() {
        let e = Entity::PLACEHOLDER;
        let a = Anchors { head: e, torso: e, legs: [e; 2], knees: [e; 2], shoulders: [e; 2] };
        for s in WearSlot::ALL {
            assert!(!pieces(s, &a).is_empty(), "{s:?}");
        }
    }
}
