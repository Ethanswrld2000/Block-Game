//! The NPC's jointed body and face, and how they move.
//!
//! The model is boxes on a tree of joints: hips, two legs with knees, a torso
//! that leans and twists, shoulders with elbows, a neck and a head. The face
//! has eye whites with an iris and a black pupil that move around, lids that
//! blink, brows that follow the mood, and a rectangular mouth that opens on
//! each syllable to show the upper and lower teeth.
//!
//! Every joint goes through a spring each frame (`motion::Spring`), so walking,
//! stopping, turning and gesturing all blend on curves. Animation runs on real
//! time, so faces stay alive while a conversation pauses the game.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::dialogue::Mood;
use super::motion::{self, Gaze, Pose, Spring};
use super::data::{Looks, On};
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

/// Where the parts sit (blocks, from the feet; the model faces -Z). Left is -X.
const HIP_Y: f32 = 0.76;
const HIP_X: f32 = 0.12;
const SHOULDER: Vec3 = Vec3::new(0.31, 0.50, 0.0);
const NECK_Y: f32 = 0.58;
/// The face's front surface (head-local z).
const FACE: f32 = -0.21;
const EYE: Vec2 = Vec2::new(0.105, 0.255);
const BROW_Y: f32 = 0.33;
const MOUTH_Y: f32 = 0.10;
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
const SCARF: usize = 3;
const TROUSERS: usize = 4;
const BOOTS: usize = 5;
const WHITE: usize = 6;
const IRIS: usize = 7;
const BLACK: usize = 8;
const MOUTH: usize = 9;
const TEETH: usize = 10;

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
        flat(l.scarf, 0.95),
        flat(l.trousers, 0.95),
        flat(l.boots, 0.85),
        flat((0.95, 0.94, 0.90), 0.4),
        flat(l.iris, 0.4),
        flat((0.02, 0.02, 0.025), 0.3),
        flat((0.28, 0.07, 0.07), 0.9),
        flat((0.97, 0.95, 0.88), 0.5),
    ]
    .into_iter()
    .map(|m| materials.add(m))
    .collect();
    let mut kit = Kit { meshes, mats };
    let flat_colors: Vec<LinearRgba> = kit.mats.iter().map(|h| materials.get(h).map_or(LinearRgba::WHITE, |m| m.base_color.to_linear())).collect();
    let root = c.spawn((Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::splat(l.scale)), Visibility::default())).id();

    // Legs.
    let hips = joint(c, root, Vec3::Y * HIP_Y);
    let mut legs = [root; 2];
    let mut knees = [root; 2];
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        legs[i] = joint(c, hips, Vec3::X * HIP_X * side);
        kit.part(c, legs[i], Vec3::new(0.21, 0.40, 0.23), Vec3::Y * -0.20, TROUSERS);
        knees[i] = joint(c, legs[i], Vec3::Y * -0.40);
        kit.part(c, knees[i], Vec3::new(0.20, 0.30, 0.22), Vec3::Y * -0.14, TROUSERS);
        kit.part(c, knees[i], Vec3::new(0.22, 0.10, 0.30), Vec3::new(0.0, -0.31, -0.03), BOOTS);
    }

    // Torso, scarf and arms.
    let torso = joint(c, hips, Vec3::ZERO);
    kit.part(c, torso, Vec3::new(0.50, 0.56, 0.28), Vec3::Y * 0.28, SHIRT);
    kit.part(c, torso, Vec3::new(0.52, 0.06, 0.30), Vec3::Y * 0.08, BOOTS);
    kit.part(c, torso, Vec3::new(0.36, 0.09, 0.32), Vec3::Y * 0.56, SCARF);
    kit.part(c, torso, Vec3::new(0.10, 0.18, 0.03), Vec3::new(0.08, 0.44, -0.15), SCARF);
    let mut shoulders = [root; 2];
    let mut elbows = [root; 2];
    let mut hands = [root; 2];
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        shoulders[i] = joint(c, torso, SHOULDER * Vec3::new(side, 1.0, 1.0));
        kit.part(c, shoulders[i], Vec3::new(0.15, 0.28, 0.16), Vec3::Y * -0.11, SHIRT);
        elbows[i] = joint(c, shoulders[i], Vec3::Y * -0.25);
        kit.part(c, elbows[i], Vec3::new(0.13, 0.22, 0.14), Vec3::Y * -0.10, SKIN);
        kit.part(c, elbows[i], Vec3::new(0.14, 0.10, 0.15), Vec3::Y * -0.25, SKIN);
        hands[i] = joint(c, elbows[i], Vec3::Y * -0.25);
    }

    // Neck and head.
    let neck = joint(c, torso, Vec3::Y * NECK_Y);
    kit.part(c, neck, Vec3::new(0.16, 0.08, 0.16), Vec3::Y * 0.02, SKIN);
    let head = joint(c, neck, Vec3::Y * 0.05);
    kit.part(c, head, Vec3::new(0.46, 0.42, 0.42), Vec3::Y * 0.21, SKIN);
    kit.part(c, head, Vec3::new(0.48, 0.08, 0.44), Vec3::new(0.0, 0.43, 0.0), HAIR);
    kit.part(c, head, Vec3::new(0.48, 0.30, 0.08), Vec3::new(0.0, 0.28, 0.20), HAIR);
    for side in [-1.0f32, 1.0] {
        kit.part(c, head, Vec3::new(0.03, 0.20, 0.30), Vec3::new(0.235 * side, 0.31, 0.06), HAIR);
    }
    kit.detail(c, head, Vec3::new(0.46, 0.05, 0.03), Vec3::new(0.0, 0.385, FACE - 0.005), HAIR);
    kit.detail(c, head, Vec3::new(0.08, 0.10, 0.05), Vec3::new(0.0, 0.18, FACE - 0.025), SKIN);

    // Eyes: white, then iris and pupil on a moving node, then the lid (scaled down from the top).
    let mut pupils = [root; 2];
    let mut lids = [root; 2];
    let mut brows = [root; 2];
    for (i, side) in [-1.0f32, 1.0].into_iter().enumerate() {
        let x = EYE.x * side;
        kit.detail(c, head, Vec3::new(0.13, 0.10, 0.012), Vec3::new(x, EYE.y, FACE - 0.006), WHITE);
        pupils[i] = joint(c, head, Vec3::new(x, EYE.y, FACE - 0.013));
        kit.detail(c, pupils[i], Vec3::new(0.06, 0.07, 0.004), Vec3::ZERO, IRIS);
        kit.detail(c, pupils[i], Vec3::new(0.034, 0.046, 0.004), Vec3::Z * -0.003, BLACK);
        lids[i] = joint(c, head, Vec3::new(x, EYE.y + 0.053, FACE - 0.019));
        kit.detail(c, lids[i], Vec3::new(0.14, 0.106, 0.006), Vec3::Y * -0.053, SKIN);
        brows[i] = joint(c, head, Vec3::new(x, BROW_Y, FACE - 0.009));
        kit.detail(c, brows[i], Vec3::new(0.14, 0.03, 0.016), Vec3::ZERO, HAIR);
    }

    // Mouth: a dark hole hanging from its top edge, upper teeth under the
    // top edge, lower teeth on the jaw at the bottom edge.
    let mouth = joint(c, head, Vec3::new(0.0, MOUTH_Y, FACE - 0.003));
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
    let mut colors = flat_colors;
    colors.extend(l.extras.iter().map(|x| Color::srgb(x.color.0, x.color.1, x.color.2).to_linear()));
    let mats = kit.mats.iter().cloned().zip(colors).collect();
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
    };
    (root, rig)
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

    // Pose: the stance (idle and talking hands) eases on springs; the walk
    // cycle goes straight on top, scaled by the eased pace, so the legs keep
    // their full swing and timing.
    let energy = rig.gesture.step(d.gesture, 0.35, dt);
    let stance = motion::idle(t).lerp(Pose::default(), pace.min(1.0)).add(motion::gesture(t, energy));
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
    set!(rig.hips, |t| {
        t.translation.y = HIP_Y + pose.bob;
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
        t.rotation = Quat::from_rotation_y(look.head_yaw - twist) * Quat::from_rotation_x(look.head_pitch + pose.lean + nod) * Quat::from_rotation_z(tilt);
    });

    // Eyes.
    for (e, side) in rig.pupils.into_iter().zip([-1.0, 1.0]) {
        let p = Vec3::new(EYE.x * side - look.pupil[0] * PUPIL_X, EYE.y + look.pupil[1] * PUPIL_Y, FACE - 0.013);
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
        let y = BROW_Y + lift + if side < 0.0 { left } else { 0.0 };
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
