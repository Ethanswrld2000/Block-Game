//! The NPC's jointed body and face, and how they move.
//!
//! The model is boxes on a tree of joints: hips, two legs with knees, a torso
//! that leans and twists, shoulders with elbows, a neck and a head. Every
//! species (`data::Species`: human, rat, mushroom folk) has the same joints
//! with its own boxes and proportions (`Frame`), so all animations fit all
//! of them. Dialogue nodes can start body animations (`motion::act`:
//! wave, nod, sit on the ground, ...), layered over the stance. The face
//! has eye whites with an iris and a black pupil that move around, lids that
//! blink, brows that follow the mood, and a rectangular mouth that opens on
//! each syllable to show the upper and lower teeth.
//!
//! Every joint goes through a spring each frame (`motion::Spring`), so walking,
//! stopping, turning and gesturing all blend on curves. Animation runs on real
//! time, so faces stay alive while a conversation pauses the game.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::dialogue::{Anim, Mood};
use super::motion::{self, Gaze, Pose, Spring};
use super::data::{Looks, On, Species};
use super::{Npc, NpcBook, EYES};
use crate::camera::FlyCamera;
use crate::swing::Limbs;

pub struct BodyPlugin;

impl Plugin for BodyPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SpawnNpc>()
            .add_systems(crate::menu::GameUpdate, (spawn, animate, match_light).chain().after(super::NpcSet).after(super::talk::TalkSet));
    }
}

/// Put an NPC in the world, feet at `at`.
#[derive(Message, Debug)]
pub struct SpawnNpc {
    pub npc: Npc,
    pub at: Vec3,
}

/// Joints and face parts of one NPC, and their animation state.
#[derive(Component)]
pub struct Rig {
    hips: Entity,
    legs: [Entity; 2],
    knees: [Entity; 2],
    torso: Entity,
    shoulders: [Entity; 2],
    elbows: [Entity; 2],
    /// Where each fist is (on the forearm): things held go here.
    pub hands: [Entity; 2],
    head: Entity,
    pupils: [Entity; 2],
    lids: [Entity; 2],
    brows: [Entity; 2],
    mouth: Entity,
    hole: Entity,
    jaw: Entity,
    teeth: [Entity; 2],
    /// A rat's tail (sways).
    tail: Option<Entity>,
    /// Where its joints and face sit.
    frame: Frame,
    // State.
    t: f32,
    phase: f32,
    pace: Spring,
    yaw: Spring,
    joints: [Spring; Pose::JOINTS],
    gaze: Gaze,
    gesture: Spring,
    open: Spring,
    wide: Spring,
    brow: [Spring; 3],
    /// The chest's twist and roll follow the hips a moment later.
    chest: [Spring; 2],
    /// Its own materials and their colours as built, darkened and torch-lit
    /// to match the terrain's light where it stands (`match_light`).
    mats: Vec<(Handle<StandardMaterial>, LinearRgba)>,
    /// Sky and torch brightness now (eased), 0..1.
    lit: Vec2,
    /// Lit by the world's light (off for the inventory picture).
    pub follows_light: bool,
    /// Body animation: the last one asked for, the one playing, seconds
    /// into it, whether a new one waits for it to fade out, and its weight.
    act_seen: Option<(Anim, u32)>,
    shown: Option<Anim>,
    act_t: f32,
    queued: bool,
    act_w: Spring,
}

impl Rig {
    /// Where the body faces now (it turns on a spring).
    pub fn facing(&self) -> f32 {
        self.yaw.x
    }

    /// The joints clothes and armor hang from (`inventory::preview::dress`).
    pub fn anchors(&self) -> Anchors {
        Anchors { head: self.head, torso: self.torso, legs: self.legs, knees: self.knees, shoulders: self.shoulders }
    }
}

/// Joints of a body, for things worn on it. Positions on them are in the
/// same frame as `build`'s parts (blocks; the model faces -Z; [left, right]).
#[derive(Debug, Clone, Copy)]
pub struct Anchors {
    pub head: Entity,
    pub torso: Entity,
    pub legs: [Entity; 2],
    pub knees: [Entity; 2],
    pub shoulders: [Entity; 2],
}

/// Where a species' joints and face sit (blocks, from the feet; the model
/// faces -Z; left is -X).
#[derive(Clone, Copy, Debug)]
struct Frame {
    hip_y: f32,
    hip_x: f32,
    knee: f32,
    shoulder: Vec3,
    elbow: f32,
    hand: f32,
    neck_y: f32,
    /// The face's front surface (head-local z).
    face: f32,
    eye: Vec2,
    brow_y: f32,
    /// The mouth's height and the surface it sits on (head-local y, z).
    mouth: Vec2,
}

impl Frame {
    fn of(s: Species) -> Self {
        match s {
            Species::Human => Frame {
                hip_y: 0.76,
                hip_x: 0.12,
                knee: 0.40,
                shoulder: Vec3::new(0.31, 0.50, 0.0),
                elbow: 0.25,
                hand: 0.25,
                neck_y: 0.58,
                face: -0.21,
                eye: Vec2::new(0.105, 0.255),
                brow_y: 0.33,
                mouth: Vec2::new(0.10, -0.21),
            },
            // The mouth is at the tip of the snout.
            Species::Rat => Frame {
                hip_y: 0.52,
                hip_x: 0.13,
                knee: 0.28,
                shoulder: Vec3::new(0.27, 0.46, 0.0),
                elbow: 0.18,
                hand: 0.20,
                neck_y: 0.56,
                face: -0.19,
                eye: Vec2::new(0.12, 0.29),
                brow_y: 0.355,
                mouth: Vec2::new(0.085, -0.49),
            },
            Species::Mushroom => Frame {
                hip_y: 0.40,
                hip_x: 0.14,
                knee: 0.22,
                shoulder: Vec3::new(0.33, 0.44, 0.0),
                elbow: 0.20,
                hand: 0.22,
                neck_y: 0.50,
                face: -0.20,
                eye: Vec2::new(0.11, 0.22),
                brow_y: 0.30,
                mouth: Vec2::new(0.085, -0.20),
            },
        }
    }
}

/// Mouth hole height shut and fully open.
const MOUTH_SHUT: f32 = 0.018;
const MOUTH_OPEN: f32 = 0.085;
/// Pupil travel from the eye's centre.
const PUPIL_X: f32 = 0.03;
const PUPIL_Y: f32 = 0.018;

struct Kit<'a> {
    meshes: &'a mut Assets<Mesh>,
    mats: Vec<Handle<StandardMaterial>>,
}

/// Material slots.
const SKIN: usize = 0;
const HAIR: usize = 1;
const SHIRT: usize = 2;
const TROUSERS: usize = 3;
const BOOTS: usize = 4;
const WHITE: usize = 5;
const IRIS: usize = 6;
const BLACK: usize = 7;
const MOUTH: usize = 8;
const TEETH: usize = 9;
/// A mushroom's gills (under the cap).
const GILLS: usize = 10;

fn joint(commands: &mut Commands, parent: Entity, at: Vec3) -> Entity {
    commands.spawn((Transform::from_translation(at), Visibility::default(), ChildOf(parent))).id()
}

impl Kit<'_> {
    fn part(&mut self, commands: &mut Commands, parent: Entity, size: Vec3, center: Vec3, mat: usize) -> Entity {
        commands
            .spawn((Mesh3d(self.meshes.add(Cuboid::from_size(size))), MeshMaterial3d(self.mats[mat].clone()), Transform::from_translation(center), ChildOf(parent)))
            .id()
    }

    /// A small face part: no shadow of its own.
    fn detail(&mut self, commands: &mut Commands, parent: Entity, size: Vec3, center: Vec3, mat: usize) -> Entity {
        let e = self.part(commands, parent, size, center, mat);
        commands.entity(e).insert(NotShadowCaster);
        e
    }
}

fn spawn(mut commands: Commands, mut msgs: MessageReader<SpawnNpc>, book: Res<NpcBook>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    for m in msgs.read() {
        let Some(kind) = book.kinds.get(m.npc.kind) else { continue };
        let (root, rig) = build(&mut commands, &mut meshes, &mut materials, &kind.looks, m.at, m.npc.yaw);
        commands.entity(root).insert((Name::new(kind.name.clone()), clone_npc(&m.npc), rig));
    }
}

/// Build a jointed body with these looks, feet at `at`, facing `yaw`.
/// Returns its root (no `Rig` on it yet) and the rig to put on it. The
/// player's body (`avatar`) is built the same way.
pub fn build(c: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, l: &Looks, at: Vec3, yaw: f32) -> (Entity, Rig) {
    let flat = |c: (f32, f32, f32), rough: f32| StandardMaterial { base_color: Color::srgb(c.0, c.1, c.2), perceptual_roughness: rough, ..default() };
    let mats = [
        flat(l.skin, 0.8),
        flat(l.hair, 0.9),
        flat(l.shirt, 0.95),
        flat(l.trousers, 0.95),
        flat(l.boots, 0.85),
        flat((0.95, 0.94, 0.90), 0.4),
        flat(l.iris, 0.4),
        flat((0.02, 0.02, 0.025), 0.3),
        flat((0.28, 0.07, 0.07), 0.9),
        flat((0.97, 0.95, 0.88), 0.5),
        flat((l.skin.0 * 0.6 + 0.36, l.skin.1 * 0.6 + 0.33, l.skin.2 * 0.6 + 0.26), 0.9),
    ]
    .into_iter()
    .map(|m| materials.add(m))
    .collect();
    let mut kit = Kit { meshes, mats };
    let f = Frame::of(l.body);
    let root = c.spawn((Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(l.scale)), Visibility::default())).id();

    // Joints: the same for every species, placed by its frame.
    let hips = joint(c, root, Vec3::Y * f.hip_y);
    let torso = joint(c, hips, Vec3::ZERO);
    let neck = joint(c, torso, Vec3::Y * f.neck_y);
    let head = joint(c, neck, Vec3::Y * 0.05);
    let mut legs = [root; 2];
    let mut knees = [root; 2];
    let mut shoulders = [root; 2];
    let mut elbows = [root; 2];
    let mut hands = [root; 2];
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        legs[i] = joint(c, hips, Vec3::X * f.hip_x * side);
        knees[i] = joint(c, legs[i], Vec3::Y * -f.knee);
        shoulders[i] = joint(c, torso, f.shoulder * Vec3::new(side, 1.0, 1.0));
        elbows[i] = joint(c, shoulders[i], Vec3::Y * -f.elbow);
        hands[i] = joint(c, elbows[i], Vec3::Y * -f.hand);
    }
    let joints = Joints { hips, torso, neck, head, legs, knees, shoulders, elbows };
    let tail = match l.body {
        Species::Human => {
            human(c, &mut kit, &joints);
            None
        }
        Species::Rat => Some(rat(c, &mut kit, &joints)),
        Species::Mushroom => {
            mushroom(c, &mut kit, &joints);
            None
        }
    };
    let (face, eye) = (f.face, f.eye);

    // Eyes: white, then iris and pupil on a moving node, then the lid (scaled down from the top).
    let mut pupils = [root; 2];
    let mut lids = [root; 2];
    let mut brows = [root; 2];
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        let x = eye.x * side;
        kit.detail(c, head, Vec3::new(0.13, 0.10, 0.012), Vec3::new(x, eye.y, face - 0.006), WHITE);
        pupils[i] = joint(c, head, Vec3::new(x, eye.y, face - 0.013));
        kit.detail(c, pupils[i], Vec3::new(0.06, 0.07, 0.004), Vec3::ZERO, IRIS);
        kit.detail(c, pupils[i], Vec3::new(0.034, 0.046, 0.004), Vec3::Z * -0.003, BLACK);
        lids[i] = joint(c, head, Vec3::new(x, eye.y + 0.053, face - 0.019));
        kit.detail(c, lids[i], Vec3::new(0.14, 0.106, 0.006), Vec3::Y * -0.053, SKIN);
        brows[i] = joint(c, head, Vec3::new(x, f.brow_y, face - 0.009));
        kit.detail(c, brows[i], Vec3::new(0.14, 0.03, 0.016), Vec3::ZERO, HAIR);
    }

    // Mouth: a dark hole hanging from its top edge, upper teeth under the
    // top edge, lower teeth on the jaw at the bottom edge.
    let mouth = joint(c, head, Vec3::new(0.0, f.mouth.x, f.mouth.y - 0.003));
    let hole = joint(c, mouth, Vec3::ZERO);
    kit.detail(c, hole, Vec3::new(0.17, 1.0, 0.006), Vec3::Y * -0.5, MOUTH);
    let top = kit.detail(c, mouth, Vec3::new(0.15, 0.022, 0.004), Vec3::new(0.0, -0.012, -0.005), TEETH);
    let jaw = joint(c, mouth, Vec3::Y * -MOUTH_SHUT);
    let low = kit.detail(c, jaw, Vec3::new(0.13, 0.02, 0.004), Vec3::new(0.0, 0.011, -0.005), TEETH);

    // Extras (caps, ears, hats, robes), each in its own colour.
    for x in &l.extras {
        let mat = kit.mats.len();
        kit.mats.push(materials.add(flat(x.color, 0.9)));
        let parent = match x.on {
            On::Head => head,
            On::Torso => torso,
            On::Hips => hips,
        };
        kit.part(c, parent, Vec3::new(x.size.0, x.size.1, x.size.2), Vec3::new(x.at.0, x.at.1, x.at.2), mat);
    }

    let seed = (at.x.to_bits() as u64) << 32 ^ at.z.to_bits() as u64 ^ 0x9E37_79B9;
    let mats = kit.mats.iter().map(|h| (h.clone(), materials.get(h).map_or(LinearRgba::WHITE, |m| m.base_color.to_linear()))).collect();
    let rig = Rig {
        hips,
        legs,
        knees,
        torso,
        shoulders,
        elbows,
        hands,
        head,
        pupils,
        lids,
        brows,
        mouth,
        hole,
        jaw,
        teeth: [top, low],
        tail,
        frame: f,
        t: 0.0,
        phase: 0.0,
        pace: Spring::default(),
        yaw: Spring::new(yaw),
        joints: motion::idle(0.0).to_array().map(Spring::new),
        gaze: Gaze::new(seed),
        gesture: Spring::default(),
        open: Spring::default(),
        wide: Spring::new(0.8),
        brow: [Spring::default(); 3],
        chest: [Spring::default(); 2],
        mats,
        lit: Vec2::new(1.0, 0.0),
        follows_light: true,
        act_seen: None,
        shown: None,
        act_t: 0.0,
        queued: false,
        act_w: Spring::new(1.0),
    };
    (root, rig)
}

/// The joints a species' boxes hang on.
struct Joints {
    hips: Entity,
    torso: Entity,
    neck: Entity,
    head: Entity,
    legs: [Entity; 2],
    knees: [Entity; 2],
    shoulders: [Entity; 2],
    elbows: [Entity; 2],
}

/// A person: shirt, trousers, boots, hair round the head.
fn human(c: &mut Commands, kit: &mut Kit, j: &Joints) {
    for i in 0..2 {
        kit.part(c, j.legs[i], Vec3::new(0.21, 0.40, 0.23), Vec3::Y * -0.20, TROUSERS);
        kit.part(c, j.knees[i], Vec3::new(0.20, 0.30, 0.22), Vec3::Y * -0.14, TROUSERS);
        kit.part(c, j.knees[i], Vec3::new(0.22, 0.10, 0.30), Vec3::new(0.0, -0.31, -0.03), BOOTS);
        kit.part(c, j.shoulders[i], Vec3::new(0.15, 0.28, 0.16), Vec3::Y * -0.11, SHIRT);
        kit.part(c, j.elbows[i], Vec3::new(0.13, 0.22, 0.14), Vec3::Y * -0.10, SKIN);
        kit.part(c, j.elbows[i], Vec3::new(0.14, 0.10, 0.15), Vec3::Y * -0.25, SKIN);
    }
    kit.part(c, j.torso, Vec3::new(0.50, 0.56, 0.28), Vec3::Y * 0.28, SHIRT);
    kit.part(c, j.torso, Vec3::new(0.52, 0.06, 0.30), Vec3::Y * 0.08, BOOTS);
    kit.part(c, j.neck, Vec3::new(0.16, 0.08, 0.16), Vec3::Y * 0.02, SKIN);
    let head = j.head;
    kit.part(c, head, Vec3::new(0.46, 0.42, 0.42), Vec3::Y * 0.21, SKIN);
    kit.part(c, head, Vec3::new(0.48, 0.08, 0.44), Vec3::new(0.0, 0.43, 0.0), HAIR);
    kit.part(c, head, Vec3::new(0.48, 0.30, 0.08), Vec3::new(0.0, 0.28, 0.20), HAIR);
    for side in [-1.0f32, 1.0] {
        kit.part(c, head, Vec3::new(0.03, 0.20, 0.30), Vec3::new(0.235 * side, 0.31, 0.06), HAIR);
    }
    kit.detail(c, head, Vec3::new(0.46, 0.05, 0.03), Vec3::new(0.0, 0.385, -0.215), HAIR);
    kit.detail(c, head, Vec3::new(0.08, 0.10, 0.05), Vec3::new(0.0, 0.18, -0.235), SKIN);
}

/// A rat: short legs on long pink feet, a pear-shaped body, a skull with a
/// long snout, pink nose, whiskers, big round ears and a long pink tail
/// (returned: its joint, which `drive` sways). Fur is `skin`, bare skin `boots`.
fn rat(c: &mut Commands, kit: &mut Kit, j: &Joints) -> Entity {
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        kit.part(c, j.legs[i], Vec3::new(0.22, 0.28, 0.24), Vec3::Y * -0.14, TROUSERS);
        kit.part(c, j.knees[i], Vec3::new(0.18, 0.18, 0.20), Vec3::Y * -0.09, TROUSERS);
        kit.part(c, j.knees[i], Vec3::new(0.16, 0.06, 0.34), Vec3::new(0.0, -0.21, -0.07), BOOTS);
        kit.part(c, j.shoulders[i], Vec3::new(0.12, 0.20, 0.13), Vec3::Y * -0.08, SHIRT);
        kit.part(c, j.elbows[i], Vec3::new(0.10, 0.15, 0.11), Vec3::Y * -0.07, SKIN);
        kit.part(c, j.elbows[i], Vec3::new(0.11, 0.06, 0.12), Vec3::Y * -0.17, BOOTS);
        // Ears: round, pink inside.
        kit.part(c, j.head, Vec3::new(0.22, 0.22, 0.04), Vec3::new(0.19 * side, 0.46, 0.02), SKIN);
        kit.detail(c, j.head, Vec3::new(0.14, 0.14, 0.02), Vec3::new(0.19 * side, 0.46, -0.012), BOOTS);
        // Whiskers.
        kit.detail(c, j.head, Vec3::new(0.22, 0.008, 0.008), Vec3::new(0.17 * side, 0.13, -0.42), WHITE);
        kit.detail(c, j.head, Vec3::new(0.20, 0.008, 0.008), Vec3::new(0.16 * side, 0.10, -0.41), WHITE);
    }
    kit.part(c, j.torso, Vec3::new(0.54, 0.44, 0.40), Vec3::Y * 0.22, SHIRT);
    kit.part(c, j.torso, Vec3::new(0.44, 0.16, 0.32), Vec3::Y * 0.50, SHIRT);
    kit.part(c, j.neck, Vec3::new(0.24, 0.08, 0.24), Vec3::Y * 0.02, SKIN);
    let head = j.head;
    kit.part(c, head, Vec3::new(0.42, 0.36, 0.38), Vec3::Y * 0.20, SKIN);
    kit.part(c, head, Vec3::new(0.26, 0.18, 0.20), Vec3::new(0.0, 0.13, -0.28), SKIN);
    kit.part(c, head, Vec3::new(0.17, 0.13, 0.12), Vec3::new(0.0, 0.11, -0.43), SKIN);
    kit.detail(c, head, Vec3::new(0.08, 0.06, 0.03), Vec3::new(0.0, 0.15, -0.50), BOOTS);
    // Tail: from the rump, back and drooping in three steps.
    let tail = joint(c, j.hips, Vec3::new(0.0, 0.06, 0.19));
    kit.part(c, tail, Vec3::new(0.07, 0.07, 0.30), Vec3::new(0.0, 0.0, 0.15), BOOTS);
    kit.part(c, tail, Vec3::new(0.06, 0.06, 0.26), Vec3::new(0.0, -0.05, 0.41), BOOTS);
    kit.part(c, tail, Vec3::new(0.05, 0.05, 0.22), Vec3::new(0.0, -0.11, 0.63), BOOTS);
    tail
}

/// Mushroom folk: stubby legs, a stem of a body, a pale stem head with no
/// hair under a wide cap (`hair` colour) with gills beneath it.
fn mushroom(c: &mut Commands, kit: &mut Kit, j: &Joints) {
    for i in 0..2 {
        kit.part(c, j.legs[i], Vec3::new(0.22, 0.22, 0.24), Vec3::Y * -0.11, TROUSERS);
        kit.part(c, j.knees[i], Vec3::new(0.21, 0.10, 0.22), Vec3::Y * -0.05, TROUSERS);
        kit.part(c, j.knees[i], Vec3::new(0.24, 0.09, 0.30), Vec3::new(0.0, -0.135, -0.03), BOOTS);
        kit.part(c, j.shoulders[i], Vec3::new(0.14, 0.22, 0.15), Vec3::Y * -0.10, SHIRT);
        kit.part(c, j.elbows[i], Vec3::new(0.12, 0.16, 0.13), Vec3::Y * -0.08, SKIN);
        kit.part(c, j.elbows[i], Vec3::new(0.13, 0.09, 0.14), Vec3::Y * -0.20, SKIN);
    }
    kit.part(c, j.torso, Vec3::new(0.56, 0.48, 0.40), Vec3::Y * 0.26, SHIRT);
    kit.part(c, j.torso, Vec3::new(0.62, 0.08, 0.46), Vec3::Y * 0.04, TROUSERS);
    kit.part(c, j.torso, Vec3::new(0.58, 0.05, 0.42), Vec3::Y * 0.12, BOOTS);
    kit.part(c, j.neck, Vec3::new(0.34, 0.08, 0.30), Vec3::Y * 0.02, SKIN);
    let head = j.head;
    kit.part(c, head, Vec3::new(0.44, 0.40, 0.40), Vec3::Y * 0.20, SKIN);
    kit.detail(c, head, Vec3::new(0.08, 0.08, 0.04), Vec3::new(0.0, 0.15, -0.22), SKIN);
    kit.part(c, head, Vec3::new(0.74, 0.03, 0.70), Vec3::Y * 0.42, GILLS);
    kit.part(c, head, Vec3::new(0.84, 0.22, 0.80), Vec3::Y * 0.54, HAIR);
    kit.part(c, head, Vec3::new(0.66, 0.10, 0.62), Vec3::Y * 0.70, HAIR);
}

fn clone_npc(n: &Npc) -> Npc {
    let mut out = Npc::new(n.kind, n.yaw);
    out.order = n.order;
    out.flags = n.flags.clone();
    out.asked = n.asked.clone();
    out.post = n.post;
    out
}

/// Brows for a mood: (lift, inner end up, left extra lift).
fn brows_of(mood: Mood) -> [f32; 3] {
    match mood {
        Mood::Neutral => [0.0, 0.0, 0.0],
        Mood::Happy => [0.008, -0.05, 0.0],
        Mood::Curious => [0.004, 0.0, 0.018],
        Mood::Worried => [0.006, 0.3, 0.0],
        Mood::Sad => [-0.002, 0.35, 0.0],
        Mood::Angry => [-0.012, -0.35, 0.0],
        Mood::Surprised => [0.024, 0.08, 0.0],
    }
}

/// How much the hands move while saying something in this mood.
fn energy_of(mood: Mood) -> f32 {
    match mood {
        Mood::Angry => 1.0,
        Mood::Surprised | Mood::Happy => 0.8,
        Mood::Curious | Mood::Neutral => 0.6,
        Mood::Worried => 0.45,
        Mood::Sad => 0.25,
    }
}

/// The parts of every body (joints and boxes), moved by `drive`.
pub type Parts<'w, 's> = Query<'w, 's, (&'static mut Transform, &'static mut Visibility), Without<Rig>>;

/// What a body is doing this frame; `drive` turns it into joint motion.
pub struct Drive {
    /// Where the body faces (it turns towards it on a spring) and about how
    /// many seconds the turn takes.
    pub yaw: f32,
    pub turn: f32,
    /// Horizontal speed and the walking speed it is measured against.
    pub speed: f32,
    pub walk: f32,
    /// Game-time step, for the stride (stops while the game is paused).
    pub game_dt: f32,
    /// (yaw, pitch) to look at, relative to the body; `None` glances about.
    pub focus: Option<[f32; 2]>,
    /// Hand movement while talking (0..1) and the mood shown on the face.
    pub gesture: f32,
    pub mood: Mood,
    /// Mouth (open, wide) and how much it is speaking (0..1).
    pub mouth: (f32, f32),
    pub speaking: f32,
    /// Arms holding something or swinging it (they take over from the stance).
    pub arms: Option<Limbs>,
    /// Extra lean forward and roll to the left (radians), e.g. from speeding
    /// up, stopping or turning.
    pub sway: [f32; 2],
    /// A body animation and its number (a new number starts it again).
    pub act: Option<(Anim, u32)>,
}

fn animate(real: Res<Time<Real>>, game: Res<Time>, book: Res<NpcBook>, cam: Single<&GlobalTransform, With<FlyCamera>>, mut npcs: Query<(&mut Transform, &Npc, &mut Rig)>, mut parts: Parts) {
    let dt = real.delta_secs().min(0.05);
    let gdt = game.delta_secs().min(0.05);
    let eye = cam.translation();
    for (mut tf, npc, mut rig) in &mut npcs {
        // Where to look: at you when near or talking, else idle glances.
        let head_at = tf.translation + Vec3::Y * EYES;
        let d = eye - head_at;
        let near = npc.talking || d.length() < 10.0;
        let yaw = rig.yaw.x;
        let focus = near.then(|| {
            let flat = Vec2::new(d.x, d.z).length();
            [motion::wrap(super::yaw_of(d) - yaw), d.y.atan2(flat)]
        });
        let blank = book.kinds.get(npc.kind).is_some_and(|k| k.looks.blank);
        let drive_with = Drive {
            yaw: npc.yaw,
            turn: 0.28,
            speed: Vec2::new(npc.vel.x, npc.vel.z).length(),
            walk: book.kinds.get(npc.kind).map_or(2.4, |k| k.walk),
            game_dt: gdt,
            focus,
            gesture: if blank { 0.0 } else { npc.speaking * energy_of(npc.mood) },
            mood: if blank { Mood::Neutral } else { npc.mood },
            mouth: npc.mouth,
            speaking: npc.speaking,
            arms: None,
            sway: [0.0; 2],
            act: npc.act,
        };
        drive(&mut tf, &mut rig, &mut parts, dt, &drive_with);
    }
}

/// Move one body for a frame (real-time `dt`): turn, walk, stance, gaze,
/// arms, face.
pub fn drive(tf: &mut Transform, rig: &mut Rig, parts: &mut Parts, dt: f32, d: &Drive) {
    rig.t += dt;
    let t = rig.t;
    // Turn the whole body towards where it faces.
    let yaw = rig.yaw.step_angle(d.yaw, d.turn, dt);
    tf.rotation = Quat::from_rotation_y(yaw);

    // Walking: pace eases in and out; the stride advances with the distance
    // covered, one stride per `motion::stride`, so the feet don't slide.
    let pace = rig.pace.step((d.speed / d.walk.max(0.1)).min(2.0), 0.12, dt);
    rig.phase = (rig.phase + d.speed * d.game_dt / motion::stride(pace) * std::f32::consts::TAU) % std::f32::consts::TAU;

    let look = rig.gaze.step(dt, d.focus);
    let act = step_act(rig, d.act, dt);

    // Pose: the stance (idle and talking hands, and any body animation)
    // eases on springs; the walk cycle goes straight on top, scaled by the
    // eased pace, so the legs keep their full swing and timing. Talking
    // hands quieten while an animation has the arms.
    let busy = if rig.shown.is_some() { rig.act_w.x } else { 0.0 };
    let energy = rig.gesture.step(d.gesture, 0.35, dt) * (1.0 - 0.7 * busy);
    let stance = motion::idle(t).lerp(Pose::default(), pace.min(1.0)).add(motion::gesture(t, energy)).add(act);
    let mut arr = stance.to_array();
    for (i, s) in rig.joints.iter_mut().enumerate() {
        arr[i] = s.step(arr[i], 0.07, dt);
    }
    let mut pose = Pose::from_array(arr).add(motion::walk(rig.phase, pace));
    // Follow-through: the chest's turn lags the hips a touch, and it rocks
    // against them, so the upper body flows rather than moving locked to the legs.
    pose.twist = rig.chest[0].step(pose.twist, 0.06, dt);
    let chest_roll = rig.chest[1].step(-pose.roll * 0.6, 0.08, dt);
    pose.lean += d.sway[0];
    pose.roll += d.sway[1];
    // Arms at work: straight from the swing's keys (no spring, so the blow
    // lands on time), the walk's arm swing only faintly under them.
    let mut arm_yaw = 0.0;
    let mut wrist = [0.0; 2];
    if let Some(a) = d.arms {
        pose.twist += a.twist;
        pose.lean += a.lean;
        pose.shoulder[1] = a.shoulder + pose.shoulder[1] * 0.15;
        pose.arm_out[1] = a.arm_out;
        pose.elbow[1] = a.elbow;
        pose.shoulder[0] = a.left_shoulder + pose.shoulder[0] * (1.0 - (a.left_shoulder.abs() * 2.0).min(1.0));
        pose.elbow[0] = pose.elbow[0].max(a.left_elbow);
        arm_yaw = a.yaw;
        wrist = a.wrist;
    }
    let twist = pose.twist + look.twist;

    macro_rules! set {
        ($e:expr, |$t:ident| $body:expr) => {
            if let Ok((mut $t, _)) = parts.get_mut($e) {
                $body;
            }
        };
    }
    let f = rig.frame;
    set!(rig.hips, |t| {
        t.translation.y = f.hip_y + pose.bob;
        t.translation.x = pose.shift;
        t.rotation = Quat::from_rotation_z(pose.roll);
    });
    for i in 0..2 {
        let side = if i == 0 { -1.0 } else { 1.0 };
        let (hip, knee) = (pose.hip[i], pose.knee[i]);
        // Legs stay upright under a rolling pelvis.
        set!(rig.legs[i], |t| t.rotation = Quat::from_rotation_z(-pose.roll) * Quat::from_rotation_x(hip));
        set!(rig.knees[i], |t| t.rotation = Quat::from_rotation_x(-knee));
        let (sh, out, el) = (pose.shoulder[i], pose.arm_out[i], pose.elbow[i]);
        let turn = if i == 1 { arm_yaw } else { 0.0 };
        set!(rig.shoulders[i], |t| t.rotation = Quat::from_rotation_y(turn) * Quat::from_rotation_z(out * side) * Quat::from_rotation_x(sh));
        set!(rig.elbows[i], |t| t.rotation = Quat::from_rotation_x(el));
    }
    // The held item: tip down (pitch) and rolled about the forearm.
    set!(rig.hands[1], |t| t.rotation = Quat::from_rotation_y(wrist[1]) * Quat::from_rotation_x(wrist[0]));
    set!(rig.torso, |t| t.rotation = Quat::from_rotation_z(chest_roll) * Quat::from_rotation_y(twist) * Quat::from_rotation_x(-pose.lean));
    // The head looks where the gaze says, undoing the torso's twist and lean,
    // with a small nod while talking and a tilt when curious.
    let nod = 0.035 * energy * (t * 5.3).sin();
    let tilt = if d.mood == Mood::Curious { 0.12 } else { 0.0 } + 0.02 * (t * 0.7).sin();
    set!(rig.head, |t| {
        t.rotation = Quat::from_rotation_y(look.head_yaw - twist + pose.shake) * Quat::from_rotation_x(look.head_pitch + pose.lean + nod + pose.nod) * Quat::from_rotation_z(tilt);
    });
    // A tail sways lazily, and more as it walks.
    if let Some(tail) = rig.tail {
        let swish = (0.22 + 0.2 * pace.min(1.5)) * (t * (1.3 + pace)).sin();
        set!(tail, |t| t.rotation = Quat::from_rotation_y(swish) * Quat::from_rotation_x(-0.15 + 0.1 * pace.min(1.0)));
    }

    // Eyes.
    for (e, side) in rig.pupils.into_iter().zip([-1.0, 1.0]) {
        let p = Vec3::new(f.eye.x * side - look.pupil[0] * PUPIL_X, f.eye.y + look.pupil[1] * PUPIL_Y, f.face - 0.013);
        set!(e, |t| t.translation = p);
    }
    // Lids: also half-lower when looking down.
    let lid = (look.blink + (-look.pupil[1]).max(0.0) * 0.25).clamp(0.0, 1.0);
    for e in rig.lids {
        if let Ok((mut t, mut v)) = parts.get_mut(e) {
            t.scale.y = lid.max(0.02);
            let want = if lid < 0.04 { Visibility::Hidden } else { Visibility::Inherited };
            if *v != want {
                *v = want;
            }
        }
    }
    // Brows.
    let target = brows_of(d.mood);
    let lift = rig.brow[0].step(target[0] + 0.006 * energy * (t * 3.1).sin().max(0.0), 0.15, dt);
    let inner = rig.brow[1].step(target[1], 0.15, dt);
    let left = rig.brow[2].step(target[2], 0.15, dt);
    for (e, side) in rig.brows.into_iter().zip([-1.0f32, 1.0]) {
        let y = f.brow_y + lift + if side < 0.0 { left } else { 0.0 };
        if let Ok((mut t, _)) = parts.get_mut(e) {
            t.translation.y = y;
            t.rotation = Quat::from_rotation_z(inner * -side);
        }
    }

    // Mouth: open on each syllable; at rest a happy face shows its teeth a little.
    let rest = match d.mood {
        Mood::Happy => 0.12,
        Mood::Surprised => 0.25,
        _ => 0.0,
    };
    let open = rig.open.step(d.mouth.0.max(rest * (1.0 - d.speaking)), 0.035, dt).clamp(0.0, 1.2);
    let wide = rig.wide.step(if d.speaking > 0.0 { d.mouth.1 } else { 0.85 + rest }, 0.06, dt);
    let h = MOUTH_SHUT + (MOUTH_OPEN - MOUTH_SHUT) * open;
    set!(rig.mouth, |t| t.scale.x = wide.clamp(0.4, 1.3));
    set!(rig.hole, |t| t.scale.y = h);
    set!(rig.jaw, |t| t.translation.y = -h);
    // Teeth show once the lips part.
    let shown = if h > MOUTH_SHUT * 1.6 { Visibility::Inherited } else { Visibility::Hidden };
    for e in rig.teeth {
        if let Ok((_, mut v)) = parts.get_mut(e)
            && *v != shown
        {
            *v = shown;
        }
    }
}

/// The body animation's pose this frame. A new one asked for waits for the
/// one playing to fade out; a one-off ends by itself; `Stand` or none lets a
/// held one fade.
fn step_act(rig: &mut Rig, asked: Option<(Anim, u32)>, dt: f32) -> Pose {
    if asked != rig.act_seen {
        rig.act_seen = asked;
        rig.queued = true;
    }
    let fading = rig.queued && rig.shown.is_some();
    rig.act_w.step(if fading { 0.0 } else { 1.0 }, 0.3, dt);
    if rig.queued && (!fading || rig.act_w.x < 0.03) {
        rig.queued = false;
        rig.shown = asked.map(|(a, _)| a).filter(|a| *a != Anim::Stand);
        rig.act_t = 0.0;
        rig.act_w = Spring::new(1.0);
    }
    let Some(a) = rig.shown else { return Pose::default() };
    rig.act_t += dt;
    if !a.held() && rig.act_t >= a.length() {
        rig.shown = None;
        return Pose::default();
    }
    motion::act(a, rig.act_t, rig.frame.hip_y).scale(rig.act_w.x)
}

/// Feet position and the box you aim at to talk (for `talk`).
pub fn talk_box(at: Vec3) -> (Vec3, Vec3) {
    (at - Vec3::new(0.35, 0.0, 0.35), at + Vec3::new(0.35, super::HEIGHT, 0.35))
}

#[cfg(test)]
mod spawn_tests {
    use super::*;

    #[test]
    fn a_spawned_npc_keeps_its_post() {
        // A gate guide that lost its post looked unguarded, so another spawned on top of it.
        let mut n = Npc::new(1, 0.3);
        n.post = Some(42);
        assert_eq!(clone_npc(&n).post, Some(42));
    }
}

/// Torch light on a body: colour and strength at full level, matching
/// `TORCH_RGB * TORCH_STRENGTH` in voxel.wesl at the camera's exposure.
const TORCH_GLOW: Vec3 = Vec3::new(2.2, 1.36, 0.66);

/// Light level 0..15 to brightness, as `level_brightness` in voxel.wesl.
pub fn level_brightness(level: u8) -> f32 {
    let x = (level as f32 / voxel::light::MAX as f32).clamp(0.0, 1.0);
    x / (4.0 - 3.0 * x)
}

/// Bodies take the terrain's light where their head is: their colours are
/// dimmed by the sky light (so sun and sky light fade in a cave like they
/// do on the walls) and torch light glows on them warm. Eased so walking
/// past a torch doesn't pop; materials change only when the light does.
fn match_light(
    time: Res<Time>,
    world: Res<crate::world::World>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(&GlobalTransform, &mut Rig), Without<crate::combat::Ghost>>,
) {
    let k = 1.0 - (-6.0 * time.delta_secs()).exp();
    for (tf, mut rig) in &mut rigs {
        if !rig.follows_light {
            continue;
        }
        let head = tf.translation() + Vec3::Y * 1.2 * tf.scale().y;
        let Some((sky, block)) = world.light_at(voxel::BlockPos::from_world(head.to_array())) else { continue };
        let want = Vec2::new(level_brightness(sky), level_brightness(block));
        let before = rig.lit;
        rig.lit = before + (want - before) * k;
        if (rig.lit - before).abs().max_element() < 0.0005 {
            continue; // settled: leave the materials alone
        }
        let (s, t) = (rig.lit.x, rig.lit.y);
        for (h, base) in &rig.mats {
            let Some(mut m) = materials.get_mut(h) else { continue };
            m.base_color = Color::LinearRgba(LinearRgba::new(base.red * s, base.green * s, base.blue * s, base.alpha));
            // The sky's shine on it fades too (0.5 is Bevy's default, which every part is built with).
            m.reflectance = 0.5 * s;
            m.emissive = LinearRgba::rgb(base.red * TORCH_GLOW.x * t, base.green * TORCH_GLOW.y * t, base.blue * TORCH_GLOW.z * t);
        }
    }
}
