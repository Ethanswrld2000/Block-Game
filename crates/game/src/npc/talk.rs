//! Conversations.
//!
//! Look at an NPC within reach and press E. Black bars slide in from the top
//! and bottom, game time stops (`Time<Virtual>` is paused: the clock, weather,
//! creatures and you all hold still, while faces keep moving on real time),
//! the HUD and your arms step aside, and the camera eases round to frame the
//! speaker with a slight zoom.
//!
//! Each line is typed out in step with the voice: one talking sound per
//! syllable at the NPC's pitch, the mouth opening on each one. Click, Space,
//! E or Enter finishes the line or moves on. Replies appear above the bottom
//! bar with a tone tag; ones you picked before are dimmed. Pick with the
//! mouse, the number keys, or W/S and Enter. Esc ends the talk.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::dialogue::{Act, Mood, Offer};
use super::motion::Spring;
use super::voice::{self, Speech};
use super::{Npc, NpcBook, Order, EYES};
use crate::camera::{FlyCamera, Menus};
use crate::sound::PlaySound;
use crate::theme::{self, colors, sizes};

pub struct TalkPlugin;

impl Plugin for TalkPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Talk>().add_systems(crate::menu::GameStartup, spawn_ui).add_systems(
            crate::menu::GameUpdate,
            (prompt, input.in_set(TalkInput), run.in_set(TalkRun), camera, hide_hud, draw)
                .chain()
                .in_set(TalkSet)
                .after(super::NpcSet)
                .after(crate::interact::InteractSet),
        );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TalkSet;

/// Services (`game::cast`) run between these: after a reply is picked, before its node starts.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TalkInput;
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TalkRun;

/// How close you must be to talk.
const REACH: f32 = 4.5;
/// Bar heights at full, percent of the screen.
const TOP_BAR: f32 = 11.0;
const BOTTOM_BAR: f32 = 17.0;
/// Seconds of quiet before the first line, while the bars and camera move.
const LEAD_IN: f32 = 0.45;
/// How much the view zooms in.
const ZOOM: f32 = 0.86;
/// The speaker sits a little right of centre (radians), clear of the replies.
const FRAME_YAW: f32 = 0.10;
const MAX_REPLIES: usize = 8;

#[derive(Resource, Default)]
pub struct Talk {
    with: Option<Entity>,
    node: String,
    line: usize,
    speech: Speech,
    text: String,
    mood: Mood,
    /// Seconds into the line (negative = waiting to start).
    t: f32,
    offers: Vec<Offer>,
    choosing: bool,
    selected: usize,
    leaving: bool,
    bars: Spring,
    base_fov: Option<f32>,
    cam: [Spring; 2],
    /// The time was already paused before (leave it so).
    was_paused: bool,
    hidden: Vec<(Entity, Visibility)>,
    /// A reply asked the NPC to cast the spirit spell (`dungeon::spirit` takes it).
    cast: bool,
    /// The line id being said, and whether it has a recording (else babble).
    line_id: String,
    recorded: bool,
    /// A reply asked for a service (`game::cast` takes it and picks the next node).
    service: Option<String>,
    /// `{key}` in a line is replaced by `vars[key]` (set by services).
    pub vars: std::collections::HashMap<String, String>,
    /// Flags that count for this talk's replies besides the NPC's own (what you can
    /// make, what's in stock today), kept up to date by `game::cast`.
    pub extra: std::collections::HashSet<String>,
    /// A cutscene (`game::cutscene`) is drawing through the talk's bars and subtitle.
    pub scene: Option<SceneView>,
}

/// What a cutscene shows in the talk's bars: how far in they are (0..1), and the
/// speaker's name, title and the part of the line shown so far.
#[derive(Default, Clone, Debug)]
pub struct SceneView {
    pub bars: f32,
    pub name: String,
    pub title: String,
    pub text: String,
}

impl Talk {
    pub fn active(&self) -> bool {
        self.with.is_some()
    }

    /// The spirit spell was asked for in this talk (once).
    pub fn take_cast(&mut self) -> bool {
        std::mem::take(&mut self.cast)
    }

    /// Who you are talking to.
    pub fn with(&self) -> Option<Entity> {
        self.with
    }

    /// A service asked for by the reply just picked (once).
    pub fn take_service(&mut self) -> Option<String> {
        self.service.take()
    }

    /// Go on to a node of the tree (it starts on the next run).
    pub fn go_to(&mut self, node: &str) {
        self.node = node.to_string();
        self.line = 0;
        self.choosing = false;
        self.t = -0.25;
        self.speech = Speech::default();
        self.text.clear();
    }
}

#[derive(Component)]
struct TalkUi;

/// A root UI node the talk (and cutscenes) leave showing when they hide the HUD.
#[derive(Component)]
pub struct KeepShown;
#[derive(Component)]
struct TopBar;
#[derive(Component)]
struct BottomBar;
#[derive(Component)]
struct NameText;
#[derive(Component)]
struct TitleText;
#[derive(Component)]
struct LineText;
#[derive(Component)]
struct HintText;
#[derive(Component)]
struct Replies;
#[derive(Component)]
struct Reply(usize);
#[derive(Component)]
struct ReplyTone(usize);
#[derive(Component)]
struct ReplyText(usize);
#[derive(Component)]
struct Prompt;

fn spawn_ui(mut commands: Commands, theme: Res<theme::Theme>) {
    let bar = |height_pct: f32| Node { width: Val::Percent(100.0), height: Val::Percent(height_pct), flex_shrink: 0.0, overflow: Overflow::clip(), ..default() };
    commands
        .spawn((
            TalkUi,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), flex_direction: FlexDirection::Column, ..default() },
            GlobalZIndex(15),
            Visibility::Hidden,
        ))
        .with_children(|ui| {
            ui.spawn((
                TopBar,
                Node { padding: UiRect::axes(Val::Percent(6.0), Val::Px(14.0)), align_items: AlignItems::FlexEnd, column_gap: Val::Px(14.0), ..bar(0.0) },
                BackgroundColor(Color::BLACK),
            ))
            .with_children(|b| {
                b.spawn((NameText, Text::new(""), theme.bold(sizes::TITLE), TextColor(colors::HEADING)));
                b.spawn((TitleText, theme::text("", sizes::LABEL, colors::MUTED), Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() }));
            });
            // Replies, between the bars on the left.
            ui.spawn((
                Replies,
                Node {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::FlexEnd,
                    padding: UiRect { left: Val::Percent(6.0), bottom: Val::Px(18.0), ..default() },
                    row_gap: Val::Px(4.0),
                    ..default()
                },
            ))
            .with_children(|r| {
                for i in 0..MAX_REPLIES {
                    r.spawn((
                        Reply(i),
                        Node { padding: UiRect::axes(Val::Px(14.0), Val::Px(5.0)), column_gap: Val::Px(10.0), align_items: AlignItems::Center, align_self: AlignSelf::FlexStart, display: Display::None, ..default() },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                    ))
                    .with_children(|row| {
                        row.spawn((ReplyTone(i), theme::text("", sizes::SMALL, colors::GOLD)));
                        row.spawn((ReplyText(i), theme::text("", sizes::LABEL, colors::TEXT)));
                    });
                }
            });
            ui.spawn((
                BottomBar,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::axes(Val::Percent(14.0), Val::Px(8.0)),
                    ..bar(0.0)
                },
                BackgroundColor(Color::BLACK),
            ))
            .with_children(|b| {
                b.spawn((LineText, theme::text("", sizes::LABEL, colors::TEXT), TextLayout::justify(Justify::Center)));
                b.spawn((HintText, theme::text("", sizes::SMALL, colors::FAINT), Node { margin: UiRect::top(Val::Px(6.0)), ..default() }));
            });
        });
    // "E  Talk to ..." under the crosshair.
    commands.spawn((
        Prompt,
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), top: Val::Percent(55.0), justify_content: JustifyContent::Center, ..default() },
        Visibility::Hidden,
    ))
    .with_children(|p| {
        p.spawn(theme::text("", sizes::LABEL, colors::TEXT));
    });
}

/// The NPC under the crosshair within reach, if any.
fn aimed(cam: &Transform, npcs: &Query<(Entity, &Transform, &mut Npc), Without<FlyCamera>>) -> Option<Entity> {
    let (o, d) = (cam.translation, cam.forward().as_vec3());
    npcs.iter()
        .filter_map(|(e, tf, _)| {
            let (min, max) = super::body::talk_box(tf.translation);
            crate::interact::ray_box(o, d, min, max).filter(|&t| t <= REACH).map(|t| (e, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e)
}

/// Show "E  Talk to ..." when looking at someone; E starts the talk.
#[allow(clippy::too_many_arguments)]
fn prompt(
    keys: Res<ButtonInput<KeyCode>>,
    book: Res<NpcBook>,
    menus: Res<Menus>,
    mut talk: ResMut<Talk>,
    mut vtime: ResMut<Time<Virtual>>,
    cam: Single<(&Transform, &FlyCamera, &Projection)>,
    mut npcs: Query<(Entity, &Transform, &mut Npc), Without<FlyCamera>>,
    prompt: Single<(&mut Visibility, &Children), With<Prompt>>,
    mut texts: Query<&mut Text>,
) {
    let (mut vis, kids) = prompt.into_inner();
    // `BLOCKGAME_TALK=1` talks to the nearest NPC in reach (screenshots).
    let auto = std::env::var_os("BLOCKGAME_TALK").is_some();
    let nearest = || {
        npcs.iter().map(|(e, tf, _)| (e, tf.translation.distance(cam.0.translation))).filter(|(_, d)| *d < REACH * 2.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(e, _)| e)
    };
    let target = if talk.active() || menus.open { None } else { aimed(cam.0, &npcs).or_else(|| if auto { nearest() } else { None }) };
    let want = if target.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    let Some(e) = target else { return };
    let Ok((_, _, mut npc)) = npcs.get_mut(e) else { return };
    let Some(kind) = book.kinds.get(npc.kind) else { return };
    if let Some(mut t) = kids.first().and_then(|k| texts.get_mut(*k).ok()) {
        let s = format!("E   Talk to {}", kind.name);
        if t.0 != s {
            t.0 = s;
        }
    }
    let start = keys.just_pressed(KeyCode::KeyE) || auto;
    if !start {
        return;
    }
    let Some(tree) = book.trees.get(&kind.dialogue) else { return };
    // Begin: the NPC stops and turns to you; time stops.
    npc.talking = true;
    npc.vel = Vec3::ZERO;
    let node = tree.opening(&npc.flags).to_string();
    tree.enter(&node, &mut npc.flags);
    talk.with = Some(e);
    talk.vars.clear();
    talk.leaving = false;
    talk.choosing = false;
    talk.was_paused = vtime.is_paused();
    vtime.pause();
    let (_, fly, proj) = *cam;
    talk.cam = [Spring::new(fly.yaw), Spring::new(fly.pitch)];
    talk.base_fov = match proj {
        Projection::Perspective(p) => Some(p.fov),
        _ => None,
    };
    begin_node(&mut talk, &book, &npc, node);
}

fn begin_node(talk: &mut Talk, book: &NpcBook, npc: &Npc, node: String) {
    talk.node = node;
    talk.line = 0;
    talk.choosing = false;
    begin_line(talk, book, npc, -LEAD_IN);
}

fn begin_line(talk: &mut Talk, book: &NpcBook, npc: &Npc, t: f32) {
    let Some(kind) = book.kinds.get(npc.kind) else { return };
    let Some(node) = book.trees.get(&kind.dialogue).and_then(|tr| tr.nodes.get(&talk.node)) else { return };
    let id = node.say.get(talk.line).map_or("", |s| s.as_str());
    talk.text = fill(book.lines.text(id), &talk.vars);
    talk.recorded = crate::lines::voice(id) != crate::lines::Voice::Babble;
    talk.line_id = id.to_string();
    talk.mood = node.mood;
    talk.speech = voice::speak(&talk.text, kind.voice.rate);
    talk.t = t;
    talk.offers = Vec::new();
    talk.selected = 0;
}

/// `{key}` replaced by `vars[key]` (pure).
pub fn fill(line: &str, vars: &std::collections::HashMap<String, String>) -> String {
    let mut out = line.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// Keys and clicks during a talk.
#[allow(clippy::too_many_arguments)]
fn input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    book: Res<NpcBook>,
    mut talk: ResMut<Talk>,
    mut npcs: Query<&mut Npc>,
    window: Single<&Window, With<PrimaryWindow>>,
    rows: Query<(&Reply, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
) {
    let Some(e) = talk.with else { return };
    if talk.leaving {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        talk.leaving = true;
        return;
    }
    // Nothing until the first line starts (so the E that opened it doesn't skip it).
    if talk.t < 0.0 {
        return;
    }
    let Ok(mut npc) = npcs.get_mut(e) else {
        talk.leaving = true;
        return;
    };
    let go = keys.any_just_pressed([KeyCode::Space, KeyCode::KeyE, KeyCode::Enter, KeyCode::NumpadEnter]);
    if talk.choosing {
        let n = talk.offers.len();
        if n == 0 {
            return;
        }
        if keys.any_just_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
            talk.selected = (talk.selected + 1) % n;
        }
        if keys.any_just_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
            talk.selected = (talk.selected + n - 1) % n;
        }
        let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8];
        let mut pick = digits.iter().take(n).position(|k| keys.just_pressed(*k));
        // The mouse picks what it's over.
        if let Some(p) = window.physical_cursor_position()
            && let Some((r, ..)) = rows.iter().find(|(r, node, tf, v)| r.0 < n && v.get() && node.contains_point(**tf, p))
        {
            talk.selected = r.0;
            if mouse.just_pressed(MouseButton::Left) {
                pick = Some(r.0);
            }
        }
        if go {
            pick = Some(talk.selected);
        }
        if let Some(i) = pick {
            choose(&mut talk, &book, &mut npc, i);
        }
        return;
    }
    if go || mouse.just_pressed(MouseButton::Left) {
        if talk.t < talk.speech.length {
            // Finish the line at once.
            talk.t = talk.speech.length;
        } else {
            next_line(&mut talk, &book, &npc);
        }
    }
}

/// After a line: the next line, the replies, the next node or the end.
fn next_line(talk: &mut Talk, book: &NpcBook, npc: &Npc) {
    let Some(kind) = book.kinds.get(npc.kind) else { return };
    let Some(tree) = book.trees.get(&kind.dialogue) else { return };
    let Some(node) = tree.nodes.get(&talk.node) else {
        talk.leaving = true;
        return;
    };
    if talk.line + 1 < node.say.len() {
        talk.line += 1;
        begin_line(talk, book, npc, -0.15);
        return;
    }
    let flags: std::collections::HashSet<String> = npc.flags.union(&talk.extra).cloned().collect();
    let offers = tree.offers(&talk.node, &flags, &npc.asked);
    if !offers.is_empty() {
        talk.offers = offers;
        talk.choosing = true;
        talk.selected = 0;
        return;
    }
    match node.then.clone() {
        Some(next) => {
            // Entering the next node sets its flags; needs the NPC mutably, done in `run`.
            talk.node = next;
            talk.line = 0;
            talk.choosing = false;
            talk.t = -0.2;
            talk.speech = Speech::default();
            talk.text.clear();
        }
        None => talk.leaving = true,
    }
}

fn choose(talk: &mut Talk, book: &NpcBook, npc: &mut Npc, i: usize) {
    let Some(offer) = talk.offers.get(i).cloned() else { return };
    let Some(kind) = book.kinds.get(npc.kind) else { return };
    let Some(tree) = book.trees.get(&kind.dialogue) else { return };
    let node = talk.node.clone();
    let mut asked = std::mem::take(&mut npc.asked);
    let out = tree.pick(&node, offer.index, &mut npc.flags, &mut asked);
    npc.asked = asked;
    match out.act {
        Some(Act::Follow) => npc.order = Order::Follow,
        Some(Act::Stay) => npc.order = Order::Stay,
        Some(Act::Wander) => npc.order = Order::Wander { home: Vec3::NAN },
        Some(Act::Spirit) => talk.cast = true,
        None => {}
    }
    if let Some(service) = out.service {
        // `game::cast` does it and sends the talk on (this node again if nothing does).
        talk.service = Some(service);
        let node = talk.node.clone();
        talk.go_to(&node);
        return;
    }
    match out.next {
        Some(next) => {
            tree.enter(&next, &mut npc.flags);
            begin_node(talk, book, npc, next);
            talk.t = -0.25;
        }
        None => talk.leaving = true,
    }
}

/// Runs the line: time, voice sounds, the mouth, and the bars in and out.
#[allow(clippy::too_many_arguments)]
fn run(
    real: Res<Time<Real>>,
    book: Res<NpcBook>,
    mut talk: ResMut<Talk>,
    mut vtime: ResMut<Time<Virtual>>,
    mut menus: ResMut<Menus>,
    mut npcs: Query<(&Transform, &mut Npc), Without<FlyCamera>>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut sounds: MessageWriter<PlaySound>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    (mut commands, server, settings): (Commands, Res<AssetServer>, Option<Res<crate::settings::Settings>>),
) {
    let dt = real.delta_secs().min(0.05);
    let Some(e) = talk.with else { return };
    menus.open = true;
    menus.still = true;
    let target = if talk.leaving { 0.0 } else { 1.0 };
    let bars = talk.bars.step(target, 0.16, dt);
    let Ok((tf, mut npc)) = npcs.get_mut(e) else {
        talk.leaving = true;
        return;
    };
    // Face you (its own walking is paused with the game).
    npc.yaw = super::yaw_of(cam.translation - tf.translation);
    if talk.leaving {
        npc.mouth = (0.0, 0.8);
        npc.speaking = 0.0;
        if bars < 0.02 {
            // Done: everything back as it was.
            npc.talking = false;
            if let Order::Wander { home } = npc.order
                && home.is_nan()
            {
                npc.order = Order::Wander { home: tf.translation };
            }
            talk.with = None;
            talk.bars = Spring::default();
            if !talk.was_paused {
                vtime.unpause();
            }
            menus.open = false;
            menus.still = false;
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
        return;
    }
    // A `then` hop into a new node: enter it and start its first line.
    if talk.text.is_empty() && !talk.choosing {
        let node = talk.node.clone();
        if let Some(tree) = book.kinds.get(npc.kind).and_then(|k| book.trees.get(&k.dialogue)) {
            tree.enter(&node, &mut npc.flags);
        }
        begin_node(&mut talk, &book, &npc, node);
        talk.t = -0.2;
    }
    // `BLOCKGAME_TALK=replies` skips to the replies (screenshots).
    if !talk.choosing && std::env::var("BLOCKGAME_TALK").is_ok_and(|v| v == "replies") {
        talk.t = talk.speech.length;
        next_line(&mut talk, &book, &npc);
        return;
    }
    let before = talk.t;
    talk.t += dt;
    let pitch = book.kinds.get(npc.kind).map_or(1.0, |k| k.voice.pitch);
    if talk.recorded && before < 0.0 && talk.t >= 0.0 {
        let volume = settings.as_ref().map_or(1.0, |s| s.volume);
        crate::lines::play_recording(&mut commands, &server, &talk.line_id, volume, Some(tf.translation + Vec3::Y * EYES));
    }
    let recorded = talk.recorded;
    for s in talk.speech.starting(before, talk.t).filter(|_| !recorded) {
        // A little pitch movement per syllable, like speech has.
        let lilt = 1.0 + 0.07 * ((s.start * 7.3).sin() + 0.5 * (s.start * 2.1).cos());
        sounds.write(PlaySound { at: Some(tf.translation + Vec3::Y * EYES), ..PlaySound::new(format!("voice_{}", s.vowel)).speed(pitch * lilt) });
    }
    let speaking = talk.t >= 0.0 && talk.t < talk.speech.length;
    npc.mouth = if speaking { talk.speech.mouth(talk.t) } else { (0.0, 0.8) };
    npc.speaking = if speaking { 1.0 } else { 0.0 };
    npc.mood = talk.mood;
    // The last line of a node done: show the replies straight away.
    if !talk.choosing && talk.t >= talk.speech.length + 0.15 {
        let last = book.kinds.get(npc.kind).and_then(|k| book.trees.get(&k.dialogue)).and_then(|t| t.nodes.get(&talk.node)).is_some_and(|n| talk.line + 1 >= n.say.len() && !n.choices.is_empty());
        if last {
            next_line(&mut talk, &book, &npc);
        }
    }
}

/// Ease the view round to frame the speaker and zoom in a little.
fn camera(real: Res<Time<Real>>, mut talk: ResMut<Talk>, npcs: Query<&Transform, (With<Npc>, Without<FlyCamera>)>, cam: Single<(&mut Transform, &mut FlyCamera, &mut Projection)>) {
    let Some(e) = talk.with else { return };
    let dt = real.delta_secs().min(0.05);
    let (mut tf, mut fly, mut proj) = cam.into_inner();
    let bars = talk.bars.x;
    if let Projection::Perspective(p) = &mut *proj
        && let Some(base) = talk.base_fov
    {
        p.fov = base * (1.0 + (ZOOM - 1.0) * bars);
    }
    let Ok(at) = npcs.get(e) else { return };
    if talk.leaving {
        return;
    }
    let d = at.translation + Vec3::Y * (EYES - 0.08) - tf.translation;
    let yaw = super::yaw_of(d) + FRAME_YAW;
    let pitch = d.y.atan2(Vec2::new(d.x, d.z).length()).clamp(-1.2, 1.2);
    let y = talk.cam[0].step_angle(yaw, 0.4, dt);
    let p = talk.cam[1].step(pitch, 0.4, dt);
    // Keep the camera's own yaw continuous (no jump of 2π when we hand it back).
    fly.yaw += super::motion::wrap(y - fly.yaw);
    fly.pitch = p;
    tf.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0);
}

/// Put the HUD and your arms away for the talk; bring them back after.
#[allow(clippy::type_complexity)]
fn hide_hud(
    mut talk: ResMut<Talk>,
    mut roots: Query<(Entity, &mut Visibility), (With<Node>, Without<ChildOf>, Without<TalkUi>, Without<Prompt>, Without<KeepShown>)>,
    mut hands: Query<&mut Visibility, (With<crate::hands::HandRoot>, Without<Node>)>,
    mut ui: Single<&mut Visibility, (With<TalkUi>, Without<crate::hands::HandRoot>, Without<Prompt>)>,
) {
    let on = talk.active() || talk.scene.is_some();
    let want = if on { Visibility::Inherited } else { Visibility::Hidden };
    if **ui != want {
        **ui = want;
    }
    if on && talk.hidden.is_empty() {
        for (e, mut v) in &mut roots {
            if *v != Visibility::Hidden {
                talk.hidden.push((e, *v));
                *v = Visibility::Hidden;
            }
        }
    } else if !on && !talk.hidden.is_empty() {
        for (e, prev) in std::mem::take(&mut talk.hidden) {
            if let Ok((_, mut v)) = roots.get_mut(e)
                && *v == Visibility::Hidden
            {
                *v = prev;
            }
        }
    }
    if on {
        for mut v in &mut hands {
            *v = Visibility::Hidden;
        }
    }
}

/// Bars, name, the typed line and the replies.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    talk: Res<Talk>,
    book: Res<NpcBook>,
    npcs: Query<&Npc>,
    mut top: Single<&mut Node, (With<TopBar>, Without<BottomBar>)>,
    mut bottom: Single<&mut Node, (With<BottomBar>, Without<TopBar>)>,
    mut texts: ParamSet<(
        Query<&mut Text, With<NameText>>,
        Query<&mut Text, With<TitleText>>,
        Query<&mut Text, With<LineText>>,
        Query<&mut Text, With<HintText>>,
        Query<(&ReplyTone, &mut Text)>,
        Query<(&ReplyText, &mut Text, &mut TextColor)>,
    )>,
    mut rows: Query<(&Reply, &mut Node, &mut BackgroundColor), (Without<TopBar>, Without<BottomBar>)>,
) {
    let set = |t: &mut Text, s: &str| {
        if t.0 != s {
            t.0 = s.to_string();
        }
    };
    let Some(e) = talk.with else {
        // A cutscene: bars and the subtitle, no replies.
        let v = talk.scene.clone().unwrap_or_default();
        let bars = v.bars.clamp(0.0, 1.0);
        top.height = Val::Percent(TOP_BAR * bars);
        bottom.height = Val::Percent(BOTTOM_BAR * bars);
        if talk.scene.is_none() {
            return;
        }
        for mut t in &mut texts.p0() {
            set(&mut t, &v.name);
        }
        for mut t in &mut texts.p1() {
            set(&mut t, &v.title);
        }
        for mut t in &mut texts.p2() {
            set(&mut t, &v.text);
        }
        for mut t in &mut texts.p3() {
            set(&mut t, "Click   next line      Hold Esc   skip");
        }
        for (_, mut node, _) in &mut rows {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
        return;
    };
    let bars = talk.bars.x.clamp(0.0, 1.0);
    top.height = Val::Percent(TOP_BAR * bars);
    bottom.height = Val::Percent(BOTTOM_BAR * bars);
    let Some(kind) = npcs.get(e).ok().and_then(|n| book.kinds.get(n.kind)) else { return };
    for mut t in &mut texts.p0() {
        set(&mut t, &kind.name);
    }
    for mut t in &mut texts.p1() {
        set(&mut t, &kind.title);
    }
    let shown: String = talk.text.chars().take(if talk.t < 0.0 { 0 } else { talk.speech.shown(talk.t) }).collect();
    for mut t in &mut texts.p2() {
        set(&mut t, &shown);
    }
    let hint = if talk.leaving || talk.t < talk.speech.length {
        ""
    } else if talk.choosing {
        "Pick a reply: click, 1-8, or W / S and Enter.   Esc leaves"
    } else {
        "Space   continue"
    };
    for mut t in &mut texts.p3() {
        set(&mut t, hint);
    }
    let show = talk.choosing && !talk.leaving;
    for (ReplyTone(i), mut t) in &mut texts.p4() {
        let s = talk.offers.get(*i).and_then(|o| o.tone.as_ref()).map(|t| format!("[{t}]")).unwrap_or_default();
        set(&mut t, &s);
    }
    for (ReplyText(i), mut t, mut color) in &mut texts.p5() {
        let Some(o) = talk.offers.get(*i) else { continue };
        set(&mut t, &format!("{}.  {}", i + 1, o.text));
        let c = if *i == talk.selected {
            colors::HEADING
        } else if o.asked {
            colors::FAINT
        } else {
            colors::TEXT
        };
        if color.0 != c {
            color.0 = c;
        }
    }
    for (Reply(i), mut node, mut bg) in &mut rows {
        let d = if show && *i < talk.offers.len() { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
        let a = if *i == talk.selected { 0.8 } else { 0.5 };
        bg.0 = Color::srgba(0.0, 0.0, 0.0, a);
    }
}
