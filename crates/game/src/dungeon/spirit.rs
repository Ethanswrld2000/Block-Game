//! Spirit form: the guide's spell, its little cinematic, and what being a
//! spirit means.
//!
//! The cinematic: black bars come in, the view tips down to your hands as they
//! rise into sight, the skin drains to a see-through glowing blue while
//! sparkles swirl round them, they turn over once, then drop away and the view
//! comes back up. From then on your arms are blue glass and the world has a
//! faint blue cast.
//!
//! Rules: only a spirit gets past a dungeon's veil (anyone else is turned back
//! to the path). The spirit starts whole; your body's health waits at the gate
//! and comes back when the spirit form ends: when you walk out of the dungeon
//! and away from the gate, or when the spirit falls inside, which casts you out
//! onto the path in front of the door (no death, no lost camp).
//!
//! Saved to `spirit.ron` (every 30 s and on exit): whether you are a spirit,
//! the dungeon, and your body's waiting health. A run in progress is not saved:
//! you wake at the dungeon's entrance.

use std::sync::Arc;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use serde::{Deserialize, Serialize};
use worldgen::Dungeon;

use super::{gate_dist, LeftDungeon, Nearby};
use crate::camera::{FlyCamera, Menus};
use crate::npc::talk::Talk;
use crate::player::Player;
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};
use crate::survival::health::Health;
use crate::survival::{Survival, Toast};

pub struct SpiritPlugin;

impl Plugin for SpiritPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Spirit>()
            .add_systems(crate::menu::GameStartup, spawn_ui)
            .add_systems(
                crate::menu::GameUpdate,
                (take_spell, cast, cast_hands, hide_for_cast, ward_gate, spirit_look)
                    .chain()
                    .in_set(SpiritSet)
                    .after(crate::npc::talk::TalkSet)
                    .after(crate::player::PlayerSet),
            )
            .add_systems(crate::menu::GameUpdate, fall_inside.in_set(SpiritSet).after(crate::survival::SurvivalSet))
            .add_systems(crate::menu::GamePostStartup, load.after(crate::world::setup_generator).after(crate::survival::load_player))
            .add_systems(crate::menu::GameUpdate, save_now_and_then)
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
    }
}

const FILE: &str = "spirit.ron";

/// What `spirit.ron` holds while you walk as a spirit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedSpirit {
    /// The dungeon's id.
    pub dungeon: Option<u64>,
    /// The body's health, waiting at the gate.
    pub body: Option<Health>,
}

impl Spirit {
    fn to_saved(&self) -> Option<SavedSpirit> {
        self.is_spirit().then(|| SavedSpirit { dungeon: self.dungeon.as_ref().map(|d| d.id), body: self.body })
    }

    /// Back in spirit form from a save (the spirit's own health is the saved player's).
    fn restore(&mut self, saved: SavedSpirit, dungeon: Option<Arc<Dungeon>>) {
        self.form = Form::Spirit;
        self.dungeon = dungeon;
        self.body = saved.body;
    }
}

fn load(dir: Res<crate::save::SaveDir>, generator: Option<Res<crate::world::Generator>>, player: Res<Player>, mut spirit: ResMut<Spirit>) {
    if crate::save::screenshot_run() {
        return;
    }
    let Some(Some(saved)) = crate::save::read_ron::<Option<SavedSpirit>>(&dir, FILE) else { return };
    let _ = &player;
    let dungeon = saved.dungeon.and_then(|id| generator.as_ref()?.0.dungeon(id));
    spirit.restore(saved, dungeon);
}

fn save_now_and_then(time: Res<Time<Real>>, mut since: Local<f32>, dir: Res<crate::save::SaveDir>, spirit: Res<Spirit>) {
    *since += time.delta_secs();
    if *since < 30.0 || crate::save::screenshot_run() {
        return;
    }
    *since = 0.0;
    crate::save::write_ron(&dir, FILE, &spirit.to_saved());
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<crate::save::SaveDir>, spirit: Res<Spirit>) {
    if exit.read().next().is_some() && !crate::save::screenshot_run() {
        crate::save::write_ron(&dir, FILE, &spirit.to_saved());
    }
}

/// The spirit's systems (rooms entered, runs ended); `foes` runs after them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpiritSet;

/// Seconds the whole cast takes.
const CAST: f32 = 5.6;
/// When the hands start and finish turning to spirit.
const CHANGE: (f32, f32) = (1.4, 3.6);
/// How far the view tips down to look at the hands (radians).
const LOOK_DOWN: f32 = 0.55;
/// The spirit form ends this far (blocks) from the gate once you're out.
const LET_GO: f32 = 14.0;

const SKIN: Color = Color::srgb(0.86, 0.66, 0.5);
const SLEEVE: Color = Color::srgb(0.32, 0.36, 0.26);
const SPIRIT: Color = Color::srgba(0.45, 0.75, 1.0, 0.42);
const SPIRIT_GLOW: LinearRgba = LinearRgba::rgb(0.18, 0.45, 1.1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Form {
    #[default]
    Body,
    /// The spell is running (the cinematic).
    Casting,
    Spirit,
}

#[derive(Resource, Default)]
pub struct Spirit {
    pub form: Form,
    /// The dungeon the spell was cast at; you come back to its door.
    pub dungeon: Option<Arc<Dungeon>>,
    /// Your body's health, kept at the gate while the spirit walks.
    body: Option<Health>,
    /// Asked for in a talk; starts once the talk has closed.
    queued: bool,
    /// Seconds into the cast (real time).
    t: f32,
    pitch_before: f32,
    hidden: Vec<(Entity, Visibility)>,
    /// Seconds before the "turned back" message can show again.
    turned_back: f32,
    next_sparkle: f32,
}

impl Spirit {
    pub fn is_spirit(&self) -> bool {
        self.form == Form::Spirit
    }

    /// How far the cast is from body (0) to spirit (1).
    fn change(&self) -> f32 {
        match self.form {
            Form::Body => 0.0,
            Form::Spirit => 1.0,
            Form::Casting => smooth((self.t - CHANGE.0) / (CHANGE.1 - CHANGE.0)),
        }
    }

    /// Become a spirit: keep the body's health and start the spirit whole.
    fn begin(&mut self, s: &mut Survival) {
        self.form = Form::Spirit;
        if self.body.is_none() {
            self.body = Some(s.vitals.health);
        }
        s.vitals.health = Health::default();
    }

    /// Start as a spirit at this dungeon (screenshots and testing).
    pub fn enter_as_spirit(&mut self, d: Arc<Dungeon>, s: &mut Survival) {
        self.dungeon = Some(d);
        if self.form != Form::Spirit {
            self.begin(s);
        }
    }

    /// Back into the body: its own health returns.
    fn end(&mut self, s: &mut Survival) {
        self.form = Form::Body;
        if let Some(h) = self.body.take() {
            s.vitals.health = h;
        }
    }
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The black bars and the blue cast over the world.
#[derive(Component)]
struct CastUi;
#[derive(Component)]
struct Tint;
#[derive(Component)]
struct Bar(bool);

fn spawn_ui(mut commands: Commands) {
    commands
        .spawn((CastUi, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, GlobalZIndex(-5), bevy::ui::FocusPolicy::Pass))
        .with_children(|c| {
            c.spawn((Tint, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::NONE)));
            for top in [true, false] {
                let mut node = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(0.0), ..default() };
                if top {
                    node.top = Val::Px(0.0);
                } else {
                    node.bottom = Val::Px(0.0);
                }
                c.spawn((Bar(top), node, BackgroundColor(Color::BLACK)));
            }
        });
}

/// The guide's spell, asked for in a talk (or `BLOCKGAME_SPIRIT=cast|1` for screenshots).
#[allow(clippy::too_many_arguments)]
fn take_spell(
    mut talk: ResMut<Talk>,
    nearby: Res<Nearby>,
    player: Res<Player>,
    mut spirit: ResMut<Spirit>,
    mut s: ResMut<Survival>,
    generator: Option<Res<crate::world::Generator>>,
    mut auto_done: Local<bool>,
) {
    if talk.take_cast() && spirit.form == Form::Body {
        spirit.queued = true;
        spirit.dungeon = nearby.nearest(player.pos);
    }
    if !*auto_done && player.ready {
        *auto_done = true;
        let d = nearby.nearest(player.pos).or_else(|| generator.and_then(|g| g.0.rotting_gate()));
        match std::env::var("BLOCKGAME_SPIRIT").as_deref() {
            Ok("cast") => {
                spirit.queued = true;
                spirit.dungeon = d;
            }
            Ok(_) => {
                spirit.dungeon = d;
                spirit.begin(&mut s);
            }
            Err(_) => {}
        }
    }
}

/// Runs the cast: bars, the view looking down and back up, sounds, and the
/// change into a spirit at the end.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn cast(
    real: Res<Time<Real>>,
    talk: Res<Talk>,
    mut spirit: ResMut<Spirit>,
    mut s: ResMut<Survival>,
    mut menus: ResMut<Menus>,
    cam: Single<(&mut Transform, &mut FlyCamera)>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut sounds: MessageWriter<PlaySound>,
    (mut bars, mut tint): (Query<(&Bar, &mut Node)>, Query<&mut BackgroundColor, With<Tint>>),
) {
    let (mut tf, mut fly) = cam.into_inner();
    if spirit.queued && !talk.active() && spirit.form == Form::Body {
        spirit.queued = false;
        spirit.form = Form::Casting;
        spirit.t = 0.0;
        spirit.next_sparkle = CHANGE.0;
        spirit.pitch_before = fly.pitch;
        sounds.write(PlaySound::new("spell_on").gain(0.8).speed(0.7));
    }
    let blue = spirit.change();
    if spirit.form == Form::Casting {
        let before = spirit.t;
        spirit.t += real.delta_secs().min(0.05);
        let t = spirit.t;
        menus.open = true;
        menus.still = true;
        let crossed = |at: f32| before < at && t >= at;
        if crossed(CHANGE.0) {
            sounds.write(PlaySound::new("craft_shimmer").gain(1.0).speed(0.8));
        }
        if crossed(CHANGE.1) {
            sounds.write(PlaySound::new("craft_pop").gain(0.9).speed(0.75));
        }
        // Look down at the hands, then back up.
        let down = smooth(t / 1.1) * (1.0 - smooth((t - 4.4) / 1.0));
        fly.pitch = spirit.pitch_before + (-LOOK_DOWN - spirit.pitch_before) * down;
        tf.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0);
        if t >= CAST {
            spirit.begin(&mut s);
            menus.open = false;
            menus.still = false;
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
    }
    // Bars in and out with the cast.
    let t = spirit.t;
    let bar = if spirit.form == Form::Casting { smooth(t / 0.5) * (1.0 - smooth((t - (CAST - 0.5)) / 0.5)) } else { 0.0 };
    for (b, mut node) in &mut bars {
        node.height = Val::Percent(if b.0 { 11.0 } else { 14.0 } * bar);
    }
    // A blue wash: a flash as the change finishes, then a faint cast while a spirit.
    let flash = if spirit.form == Form::Casting { (-(t - CHANGE.1).powi(2) * 6.0).exp() * 0.35 } else { 0.0 };
    let alpha = (0.07 * blue + flash).min(0.5);
    for mut c in &mut tint {
        c.0 = Color::srgba(0.35, 0.6, 1.0, alpha);
    }
}

/// The two arms you look at during the cast.
#[derive(Component)]
struct CastArm(f32);

/// Their materials, which drain from skin to spirit.
#[derive(Resource)]
struct CastLook {
    skin: Handle<StandardMaterial>,
    sleeve: Handle<StandardMaterial>,
}

#[allow(clippy::too_many_arguments)]
fn cast_hands(
    mut commands: Commands,
    mut spirit: ResMut<Spirit>,
    look: Option<Res<CastLook>>,
    cam: Single<(Entity, &Transform), With<FlyCamera>>,
    mut arms: Query<(Entity, &CastArm, &mut Transform), Without<FlyCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bursts: MessageWriter<Burst>,
) {
    let (cam_e, cam_tf) = *cam;
    if spirit.form != Form::Casting {
        for (e, ..) in &arms {
            commands.entity(e).despawn();
        }
        return;
    }
    let Some(look) = look else {
        let blend = |m: &mut Assets<StandardMaterial>, c: Color| m.add(StandardMaterial { base_color: c, perceptual_roughness: 0.9, alpha_mode: AlphaMode::Blend, ..default() });
        let skin = blend(&mut materials, SKIN);
        let sleeve = blend(&mut materials, SLEEVE);
        commands.insert_resource(CastLook { skin, sleeve });
        return;
    };
    if arms.is_empty() {
        let sleeve = meshes.add(Cuboid::new(0.09, 0.09, 0.34));
        let fist = meshes.add(Cuboid::new(0.085, 0.05, 0.1));
        for side in [-1.0f32, 1.0] {
            commands.entity(cam_e).with_children(|c| {
                c.spawn((CastArm(side), Transform::from_xyz(0.2 * side, -0.6, -0.45), Visibility::default())).with_children(|a| {
                    a.spawn((Mesh3d(sleeve.clone()), MeshMaterial3d(look.sleeve.clone()), Transform::from_xyz(0.0, -0.015, 0.2), NotShadowCaster));
                    a.spawn((Mesh3d(fist.clone()), MeshMaterial3d(look.skin.clone()), Transform::default(), NotShadowCaster));
                });
            });
        }
    }
    let t = spirit.t;
    // Up into view, held palms up while they change, turned over once, then down.
    let rise = smooth((t - 0.3) / 1.0) * (1.0 - smooth((t - 4.5) / 0.8));
    let turn = smooth((t - CHANGE.1) / 0.8) * std::f32::consts::PI;
    let breathe = (t * 2.2).sin() * 0.008;
    for (_, arm, mut tf) in &mut arms {
        let side = arm.0;
        tf.translation = Vec3::new(0.17 * side, -0.6 + 0.42 * rise + breathe, -0.42);
        tf.rotation = Quat::from_rotation_x(0.75 * rise) * Quat::from_rotation_z(side * (0.25 - turn)) * Quat::from_rotation_y(-side * 0.18);
    }
    let blue = spirit.change();
    let mix = |a: Color, b: Color| -> Color {
        let (a, b) = (a.to_linear(), b.to_linear());
        Color::LinearRgba(LinearRgba::new(a.red + (b.red - a.red) * blue, a.green + (b.green - a.green) * blue, a.blue + (b.blue - a.blue) * blue, a.alpha + (b.alpha - a.alpha) * blue))
    };
    let flicker = 0.75 + 0.25 * (t * 23.0).sin() * (t * 7.0).cos();
    for (h, from) in [(&look.skin, SKIN), (&look.sleeve, SLEEVE)] {
        if let Some(mut m) = materials.get_mut(h) {
            m.base_color = mix(from, SPIRIT);
            m.emissive = SPIRIT_GLOW * blue * flicker;
        }
    }
    // Sparkles swirl round the hands while they change.
    if (CHANGE.0..CHANGE.1 + 0.4).contains(&t) && t >= spirit.next_sparkle {
        spirit.next_sparkle = t + 0.1;
        for side in [-1.0f32, 1.0] {
            let local = Vec3::new(0.17 * side + (t * 9.0 * side).cos() * 0.08, -0.18 + (t * 5.0).sin() * 0.05, -0.42 + (t * 9.0).sin() * 0.06);
            bursts.write(Burst::new(cam_tf.transform_point(local), BurstKind::Sparkle, 2));
        }
    }
}

/// Put the HUD and your usual arm away during the cast; bring them back after.
#[allow(clippy::type_complexity)]
fn hide_for_cast(
    mut spirit: ResMut<Spirit>,
    mut roots: Query<(Entity, &mut Visibility), (With<Node>, Without<ChildOf>, Without<CastUi>)>,
    mut hands: Query<&mut Visibility, (With<crate::hands::HandRoot>, Without<Node>)>,
) {
    let on = spirit.form == Form::Casting;
    if on && spirit.hidden.is_empty() {
        for (e, mut v) in &mut roots {
            if *v != Visibility::Hidden {
                spirit.hidden.push((e, *v));
                *v = Visibility::Hidden;
            }
        }
    } else if !on && !spirit.hidden.is_empty() {
        for (e, prev) in std::mem::take(&mut spirit.hidden) {
            if let Ok((_, mut v)) = roots.get_mut(e)
                && *v == Visibility::Hidden
            {
                *v = prev;
            }
        }
        for mut v in &mut hands {
            *v = Visibility::Inherited;
        }
    }
    if on {
        for mut v in &mut hands {
            *v = Visibility::Hidden;
        }
    }
}

/// Only spirits pass the veil (walking through it starts a run, `delve`); the
/// spirit form ends when you walk back out and away from the gate.
#[allow(clippy::too_many_arguments)]
fn ward_gate(
    time: Res<Time>,
    nearby: Res<Nearby>,
    mut spirit: ResMut<Spirit>,
    mut s: ResMut<Survival>,
    mut player: ResMut<Player>,
    mut cam: Query<&mut Transform, With<FlyCamera>>,
    mut toast: MessageWriter<Toast>,
    mut left: MessageWriter<LeftDungeon>,
) {
    spirit.turned_back -= time.delta_secs();
    // Down on a floor: the run is under way.
    if worldgen::in_pocket(player.pos.x as i32, player.pos.z as i32) {
        return;
    }
    let at = player.pos.to_array();
    let inside = nearby.list.iter().find(|d| d.inside(at)).cloned();
    match (spirit.form, inside) {
        (Form::Spirit, Some(d)) => {
            // A spirit cast at one gate can walk into any dungeon.
            if spirit.dungeon.as_ref().is_none_or(|k| k.id != d.id) {
                spirit.dungeon = Some(d);
            }
        }
        (Form::Spirit, None) => {
            let far = spirit.dungeon.as_ref().is_none_or(|d| gate_dist(d, player.pos) > LET_GO);
            if far {
                if let Some(d) = spirit.dungeon.clone() {
                    left.write(LeftDungeon { dungeon: d, fell: false });
                }
                spirit.end(&mut s);
                toast.write(Toast("Your spirit settles back into your body".into()));
            }
        }
        (_, Some(d)) => {
            // Not a spirit: the ward turns you back onto the path.
            put_at(&mut player, &mut cam, Vec3::from_array(d.outside));
            if spirit.turned_back <= 0.0 {
                spirit.turned_back = 3.0;
                toast.write(Toast("The ward turns you back. Only a spirit may pass".into()));
            }
        }
        (_, None) => {}
    }
}

fn put_at(player: &mut Player, cam: &mut Query<&mut Transform, With<FlyCamera>>, to: Vec3) {
    player.pos = to;
    player.vel = Vec3::ZERO;
    for mut tf in cam.iter_mut() {
        tf.translation = to + Vec3::Y * crate::player::EYE;
    }
}

/// A spirit that falls is cast out onto the path in front of the door, and
/// wakes in its body.
#[allow(clippy::too_many_arguments)]
fn fall_inside(
    mut spirit: ResMut<Spirit>,
    mut s: ResMut<Survival>,
    mut player: ResMut<Player>,
    mut cam: Query<&mut Transform, With<FlyCamera>>,
    mut cards: MessageWriter<crate::places::ShowCard>,
    mut left: MessageWriter<LeftDungeon>,
    mut sounds: MessageWriter<PlaySound>,
) {
    if spirit.form != Form::Spirit || !s.vitals.health.is_dead() {
        return;
    }
    spirit.end(&mut s);
    s.dead_for = None;
    sounds.write(PlaySound::new("spell_off").gain(0.9).speed(0.7));
    let Some(d) = spirit.dungeon.clone() else { return };
    put_at(&mut player, &mut cam, Vec3::from_array(d.outside));
    cards.write(crate::places::ShowCard { title: "Cast out".into(), subtitle: format!("Your spirit fell in {}", d.name) });
    left.write(LeftDungeon { dungeon: d, fell: true });
}

/// While a spirit: your arm is blue glass (its usual materials, tinted and put back after).
#[allow(clippy::type_complexity)]
fn spirit_look(
    spirit: Res<Spirit>,
    roots: Query<&Children, With<crate::hands::HandRoot>>,
    parts: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut was: Local<Vec<(AssetId<StandardMaterial>, Color, AlphaMode, LinearRgba)>>,
) {
    let on = spirit.is_spirit();
    if on == !was.is_empty() {
        return;
    }
    if on {
        for children in &roots {
            for c in children.iter() {
                let Ok(m) = parts.get(c) else { continue };
                let id = m.0.id();
                if was.iter().any(|w| w.0 == id) {
                    continue;
                }
                if let Some(mut mat) = materials.get_mut(id) {
                    was.push((id, mat.base_color, mat.alpha_mode, mat.emissive));
                    // Solid glowing blue, not blended: see-through boxes of the
                    // arm sorted against each other as you moved and flickered.
                    mat.base_color = SPIRIT.with_alpha(1.0);
                    mat.alpha_mode = AlphaMode::Opaque;
                    mat.emissive = SPIRIT_GLOW;
                }
            }
        }
    } else {
        for (id, color, alpha, glow) in was.drain(..) {
            if let Some(mut mat) = materials.get_mut(id) {
                mat.base_color = color;
                mat.alpha_mode = alpha;
                mat.emissive = glow;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survival::health::Layer;

    #[test]
    fn spirit_form_round_trips_through_the_save() {
        let mut spirit = Spirit::default();
        assert_eq!(spirit.to_saved(), None);
        let mut s = Survival::default();
        spirit.begin(&mut s);
        let saved = spirit.to_saved().expect("a spirit is saved");
        let text = ron::to_string(&Some(saved)).unwrap();
        let back: Option<SavedSpirit> = ron::from_str(&text).unwrap();
        let mut loaded = Spirit::default();
        loaded.restore(back.unwrap(), None);
        assert!(loaded.is_spirit());
        assert_eq!(loaded.body, spirit.body);
    }

    #[test]
    fn the_body_keeps_its_wounds_while_the_spirit_walks() {
        let mut s = Survival::default();
        s.vitals.health.bars[Layer::Damage as usize] = 20.0;
        let mut spirit = Spirit::default();
        spirit.begin(&mut s);
        assert!(spirit.is_spirit());
        assert_eq!(s.vitals.health.bars, Health::default().bars, "the spirit starts whole");
        s.vitals.health.bars[Layer::Injuries as usize] = 0.0;
        assert!(s.vitals.health.is_dead());
        spirit.end(&mut s);
        assert!(!s.vitals.health.is_dead(), "falling as a spirit doesn't kill the body");
        assert_eq!(s.vitals.health.bars[Layer::Damage as usize], 20.0, "the body's own hurt comes back");
        assert_eq!(spirit.form, Form::Body);
    }

    #[test]
    fn the_change_runs_from_body_to_spirit() {
        let mut spirit = Spirit { form: Form::Casting, ..default() };
        spirit.t = 0.0;
        assert_eq!(spirit.change(), 0.0);
        spirit.t = (CHANGE.0 + CHANGE.1) / 2.0;
        assert!((spirit.change() - 0.5).abs() < 0.01);
        spirit.t = CAST;
        assert_eq!(spirit.change(), 1.0);
    }
}
