//! Working a bench with your hands, not through a menu.
//!
//! Right click a bench (tool bench, carpentry bench) and the view glides down over
//! its top. Your things lie in a strip along the bottom of the screen: click one
//! to pick it up (Shift: the whole stack); it hangs under the cursor over the
//! board. Click a slot to set it down there, or click what lies in a slot to
//! pick it back up. When the slots match a recipe (`assets/data/benches.ron`),
//! work the bench's tool (knock the strike stone, saw) until the new thing lies
//! on the board, then click it
//! to take it. Right click puts what you hold back; A / D steps to the next
//! bench at the camp; Esc (or walking off) leaves, and everything on the board
//! goes back into your pockets.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::bench::{slots_of, BenchData, Board, SlotDef, Work};
use super::{Station, StationKind};
use crate::camera::{FlyCamera, Menus};
use crate::interact::ray_box;
use crate::inventory::grid::Stack;
use crate::inventory::{Inventory, Spill};
use crate::item_art::ItemArt;
use crate::items::{ItemId, Items};
use crate::player::{MoveMode, Player};
use crate::sound::PlaySound;
use crate::survival::Toast;
use crate::theme::{colors, sizes, Theme};

/// Benches with a board, and their names in fusion.ron / benches.ron.
pub(super) fn board_name(kind: StationKind) -> Option<&'static str> {
    match kind {
        StationKind::ToolBench => Some("tool_bench"),
        StationKind::CarpentryBench => Some("carpentry_bench"),
        StationKind::Urn => None,
    }
}

/// Recipes worked at the benches.
#[derive(Resource, Default)]
pub struct Benches(pub BenchData);

/// You are at a bench: which one, what lies on its board, what you hold.
#[derive(Resource, Default)]
pub struct AtBench {
    pub station: Option<Station>,
    /// The side of the bench you look from (+1 or -1 along its depth).
    side: f32,
    /// 0 = your own eyes, 1 = looking down at the board.
    blend: f32,
    /// The board view, kept while gliding back out.
    view: Option<Transform>,
    pub board: Board,
    held: Option<Stack>,
    /// Where the held thing hangs, following the cursor.
    hand: Option<Vec3>,
    /// A knock just happened: things on the board jump.
    bump: f32,
    /// The board changed: rebuild its models.
    dirty: bool,
    /// Shown under the bench's name for a moment.
    note: Option<(String, f32)>,
}

impl AtBench {
    pub fn open(&self) -> bool {
        self.station.is_some()
    }

    /// Step up to a bench, from where the camera stands.
    pub fn enter(&mut self, st: Station, eye: Vec3) {
        let local = rotation(&st).inverse() * (eye - st.pos);
        self.side = if local.z < 0.0 { -1.0 } else { 1.0 };
        self.view = Some(board_view(&st, self.side));
        self.station = Some(st);
        self.dirty = true;
        self.note = None;
    }
}

/// Where things are on a bench's top, in its own frame (x along its length,
/// z across, y up from its foot).
pub(super) struct Layout {
    /// Slots: name, centre (x, z), half size, the height things lie at, and
    /// how to say where it is ("in the groove").
    slots: &'static [(&'static str, Vec2, Vec2, f32, &'static str)],
    /// The tool you work (strike stone, saw): its box.
    tool: (Vec3, Vec3),
    /// Where a finished thing lies: centre, half size, height.
    made: (Vec2, Vec2, f32),
    /// The view: how high above the top and how far out to the side.
    view: (f32, f32),
    /// "Knock the strike stone", and what one go is called ("knock").
    work: (&'static str, &'static str),
}

const TOOL_BENCH: Layout = Layout {
    slots: &[
        ("handle", Vec2::new(-0.15, -0.12), Vec2::new(0.34, 0.11), 0.833, "in the groove"),
        ("material", Vec2::new(0.32, 0.12), Vec2::new(0.17, 0.17), 0.833, "in the hollow"),
    ],
    tool: (Vec3::new(0.31, 0.82, -0.33), Vec3::new(0.53, 0.96, -0.15)),
    made: (Vec2::new(-0.2, 0.17), Vec2::new(0.28, 0.13), 0.833),
    view: (0.95, 0.62),
    work: ("Knock the strike stone", "knock"),
};

const CARPENTRY_BENCH: Layout = Layout {
    slots: &[
        ("bed", Vec2::new(-0.25, 0.0), Vec2::new(0.42, 0.18), 0.846, "on the bed"),
        ("vise", Vec2::new(0.72, 0.0), Vec2::new(0.12, 0.17), 1.04, "in the vise"),
    ],
    tool: (Vec3::new(-0.25, 0.83, -0.38), Vec3::new(0.42, 0.93, -0.22)),
    made: (Vec2::new(0.2, 0.3), Vec2::new(0.25, 0.1), 0.846),
    view: (1.25, 0.8),
    work: ("Saw", "stroke of the saw"),
};

pub(super) fn layout(kind: StationKind) -> &'static Layout {
    match kind {
        StationKind::CarpentryBench => &CARPENTRY_BENCH,
        _ => &TOOL_BENCH,
    }
}

impl Layout {
    fn slot(&self, name: &str) -> (Vec2, Vec2, f32, &'static str) {
        self.slots.iter().find(|s| s.0 == name).map_or((Vec2::ZERO, Vec2::ZERO, self.made.2, "on the bench"), |s| (s.1, s.2, s.3, s.4))
    }

    /// The main working height.
    fn top(&self) -> f32 {
        self.made.2
    }
}

fn rotation(st: &Station) -> Quat {
    if st.turned { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY }
}

fn to_world(st: &Station, local: Vec3) -> Vec3 {
    st.pos + rotation(st) * local
}

/// Looking down at the bench top from the side you stand on.
fn board_view(st: &Station, side: f32) -> Transform {
    let l = layout(st.kind);
    let eye = to_world(st, Vec3::new(0.0, l.top() + l.view.0, side * l.view.1));
    let at = to_world(st, Vec3::new(0.0, l.top(), side * 0.04));
    Transform::from_translation(eye).looking_at(at, Vec3::Y)
}

/// What the cursor is over on the bench.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spot {
    /// The bench's tool (strike stone, saw).
    Tool,
    Slot(SlotDef),
    Made,
}

/// The ray in the bench's frame: what it meets first, and where it crosses the top.
fn spot(st: &Station, name: &str, made: bool, origin: Vec3, dir: Vec3) -> (Option<Spot>, Option<Vec3>) {
    let l = layout(st.kind);
    let inv = rotation(st).inverse();
    let (o, d) = (inv * (origin - st.pos), inv * dir);
    let cross = |y: f32| (d.y < -1e-4).then(|| o + d * ((y - o.y) / d.y));
    let top = cross(l.top());
    if ray_box(o, d, l.tool.0, l.tool.1).is_some() {
        return (Some(Spot::Tool), top.map(|p| to_world(st, p)));
    }
    let inside = |y: f32, c: Vec2, h: Vec2| cross(y).is_some_and(|p| (p.x - c.x).abs() <= h.x && (p.z - c.y).abs() <= h.y);
    let hit = if made && inside(l.made.2, l.made.0, l.made.1) {
        Some(Spot::Made)
    } else {
        slots_of(name)
            .iter()
            .find(|s| {
                let (c, h, y, _) = l.slot(s.name);
                inside(y, c, h)
            })
            .map(|s| Spot::Slot(*s))
    };
    (hit, top.map(|p| to_world(st, p)))
}

/// Something lying on the board, and the height it rests at.
#[derive(Component)]
pub(super) struct BoardModel(f32);

#[derive(Component)]
pub(super) struct HeldModel;

#[derive(Component)]
pub(super) struct BoardUi;

#[derive(Component)]
pub(super) struct Strip;

#[derive(Component)]
pub(super) struct StripCell(ItemId);

#[derive(Component)]
pub(super) struct HintTitle;

#[derive(Component)]
pub(super) struct HintLine;

#[derive(Component)]
pub(super) struct RecipeLines;

pub(super) fn load_benches(mut commands: Commands, items: Res<Items>) {
    match BenchData::from_ron(include_str!("../../../../assets/data/benches.ron"), &items) {
        Ok(d) => commands.insert_resource(Benches(d)),
        Err(e) => {
            error!("{e}");
            commands.insert_resource(Benches::default());
        }
    }
}

/// Put a stack back in the pockets, or at your feet if they're full.
fn give_back(inv: &mut Inventory, items: &Items, spill: &mut MessageWriter<Spill>, at: Vec3, stack: Stack) {
    if let Some(rest) = inv.add(items, stack) {
        spill.write(Spill { stack: rest, at });
    }
}

/// Leave the bench: everything goes back into the pockets.
fn leave(at: &mut AtBench, inv: &mut Inventory, items: &Items, spill: &mut MessageWriter<Spill>, feet: Vec3) {
    for s in at.held.take().into_iter().chain(at.board.clear()) {
        give_back(inv, items, spill, feet, s);
    }
    at.station = None;
    at.hand = None;
    at.dirty = true;
}

/// Glide the camera between your eyes and the board, after the player has placed it.
pub(super) fn board_camera(
    time: Res<Time>,
    mut at: ResMut<AtBench>,
    mut cam: Single<&mut Transform, With<FlyCamera>>,
) {
    let want = if at.open() { 1.0 } else { 0.0 };
    let dt = time.delta_secs().min(0.1);
    let blend = if (at.blend - want).abs() < 1e-3 { want } else { at.blend + (want - at.blend) * (1.0 - (-dt * 7.0).exp()) };
    at.blend = blend;
    if blend <= 0.0 {
        at.view = None;
        return;
    }
    let Some(view) = at.view else { return };
    let k = blend * blend * (3.0 - 2.0 * blend);
    // A knock shakes the view a little.
    let shake = at.bump * at.bump * 0.006 * (time.elapsed_secs() * 70.0).sin();
    cam.translation = cam.translation.lerp(view.translation, k) + Vec3::Y * shake;
    cam.rotation = cam.rotation.slerp(view.rotation, k);
}

/// At the bench only the board shows: the HUD (every other top-level UI node)
/// and your arms are put away, and come back as they were when you leave.
#[allow(clippy::type_complexity)]
pub(super) fn board_hides(
    at: Res<AtBench>,
    mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>, Without<BoardUi>)>,
    mut hands: Query<&mut Visibility, (With<crate::hands::HandRoot>, Without<Node>)>,
    all: Query<Entity, (With<Node>, Without<ChildOf>, Without<BoardUi>)>,
    mut hidden: Local<Vec<(Entity, Visibility)>>,
) {
    let away = at.blend > 0.02;
    if away && hidden.is_empty() {
        for e in &all {
            if let Ok(mut v) = roots.get_mut(e)
                && *v != Visibility::Hidden
            {
                hidden.push((e, *v));
                *v = Visibility::Hidden;
            }
        }
    } else if !away && !hidden.is_empty() {
        for (e, prev) in hidden.drain(..) {
            if let Ok(mut v) = roots.get_mut(e)
                && *v == Visibility::Hidden
            {
                *v = prev;
            }
        }
    }
    let want = if away { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut hands {
        if *v != want {
            *v = want;
        }
    }
}

/// Keys at the bench: Esc leaves, A / D steps to the next bench.
#[allow(clippy::too_many_arguments)]
pub(super) fn board_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut at: ResMut<AtBench>,
    mut menus: ResMut<Menus>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    stations: Query<&Station>,
    (items, player, mode): (Res<Items>, Res<Player>, Res<MoveMode>),
    mut inv: ResMut<Inventory>,
    mut spill: MessageWriter<Spill>,
) {
    let Some(st) = at.station else { return };
    // The bench is gone (bedroll packed up), or you started flying.
    let still_there = stations.iter().any(|s| s.kind == st.kind && s.pos.distance(st.pos) < 0.01);
    let quit = keys.just_pressed(KeyCode::Escape) || !still_there || *mode == MoveMode::Fly;
    if quit {
        leave(&mut at, &mut inv, &items, &mut spill, player.pos + Vec3::Y);
        menus.open = false;
        menus.still = false;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        return;
    }
    let step = keys.just_pressed(KeyCode::KeyD) as i32 - keys.just_pressed(KeyCode::KeyA) as i32;
    if step != 0 {
        // Benches in a line along the camp, ordered by where they stand.
        let mut benches: Vec<Station> = stations.iter().filter(|s| board_name(s.kind).is_some()).copied().collect();
        benches.sort_by(|a, b| (a.pos.x + a.pos.z).total_cmp(&(b.pos.x + b.pos.z)));
        if let Some(i) = benches.iter().position(|s| s.kind == st.kind) {
            let next = benches[(i as i32 + step).rem_euclid(benches.len() as i32) as usize];
            if next.kind != st.kind {
                // What lies on this board goes back first.
                leave(&mut at, &mut inv, &items, &mut spill, player.pos + Vec3::Y);
                let eye = player.pos + Vec3::Y * player.eye;
                at.enter(next, eye);
            }
        }
    }
}

/// Clicks at the bench: the strip, the slots, the strike stone.
#[allow(clippy::too_many_arguments)]
pub(super) fn board_click(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &Transform), With<FlyCamera>>,
    cells: Query<(&StripCell, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
    (items, benches, player): (Res<Items>, Res<Benches>, Res<Player>),
    mut at: ResMut<AtBench>,
    mut inv: ResMut<Inventory>,
    (mut spill, mut sound, mut toast, mut gain): (MessageWriter<Spill>, MessageWriter<PlaySound>, MessageWriter<Toast>, MessageWriter<crate::progress::GainXp>),
    prog: Option<Res<crate::progress::xp::Progression>>,
) {
    let Some(st) = at.station else { return };
    let Some(name) = board_name(st.kind) else { return };
    let feet = player.pos + Vec3::Y;
    if mouse.just_pressed(MouseButton::Right) {
        if let Some(h) = at.held.take() {
            give_back(&mut inv, &items, &mut spill, feet, h);
        }
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) || at.blend < 0.6 {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // The strip of your things.
    if let Some(phys) = window.physical_cursor_position()
        && let Some((cell, ..)) = cells.iter().find(|(_, n, t, v)| v.get() && n.contains_point(**t, phys))
    {
        let item = cell.0;
        if let Some(h) = at.held
            && h.item != item
        {
            give_back(&mut inv, &items, &mut spill, feet, h);
            at.held = None;
        }
        let have = at.held.map_or(0, |h| h.count);
        let room = items.get(item).stack.saturating_sub(have) as u32;
        let n = if shift { inv.count(item).min(room) } else { 1.min(room) };
        if n > 0 && inv.take(item, n) {
            at.held = Some(Stack::new(item, have + n as u16));
            sound.write(PlaySound::new("dig_plant").gain(0.4));
        }
        return;
    }
    let Some(pos) = window.cursor_position() else { return };
    let (cam, tf) = *camera;
    let Ok(ray) = cam.viewport_to_world(&GlobalTransform::from(*tf), pos) else { return };
    let (hit, _) = spot(&st, name, at.board.made.is_some(), ray.origin, ray.direction.as_vec3());
    match hit {
        Some(Spot::Tool) => match at.board.strike(&benches.0, name) {
            Work::Nothing => {
                sound.write(PlaySound::new("hit").gain(0.35));
                let line = if at.board.made.is_some() { "Take what you made first" } else { "That doesn't make anything here" };
                at.note = Some((line.into(), 2.0));
            }
            Work::Knock { .. } => {
                sound.write(PlaySound::new("dig_hard_0").gain(0.9));
                sound.write(PlaySound::new("hit").gain(0.5));
                at.bump = 1.0;
                at.dirty = true;
            }
            Work::Made(s) => {
                sound.write(PlaySound::new("hit").gain(0.8));
                sound.write(PlaySound::new("place").gain(0.9));
                at.bump = 1.0;
                at.dirty = true;
                at.note = Some((format!("You made a {}", items.get(s.item).title), 3.0));
                toast.write(Toast(format!("Made a {}", items.get(s.item).title)));
                if let Some(p) = &prog {
                    gain.write(crate::progress::GainXp { xp: p.xp.craft, why: String::new() });
                }
            }
        },
        Some(Spot::Made) => {
            if let Some(s) = at.board.made.take() {
                give_back(&mut inv, &items, &mut spill, feet, s);
                sound.write(PlaySound::new("dig_plant").gain(0.5));
                at.dirty = true;
                at.note = None;
            }
        }
        Some(Spot::Slot(def)) => {
            match at.held.take() {
                Some(h) => at.held = at.board.put(def, h),
                None => at.held = at.board.take(def.name),
            }
            sound.write(PlaySound::new("dig_hard_1").gain(0.45));
            at.dirty = true;
        }
        None => {}
    }
}

/// How big a thing lies on the board: long things fill the groove, small ones the hollow.
fn px_for(size: UVec2) -> f32 {
    let long = size.max_element() as f32;
    if long > 24.0 { 0.56 / long } else { 0.15 / long }
}

/// Where the `i`th of `n` small things lies, around a slot's middle: two
/// rows of up to four.
fn spread(i: usize, n: usize) -> Vec2 {
    let cols = n.min(4);
    let rows = n.div_ceil(4);
    let (cx, cy) = ((i % 4) as f32, (i / 4) as f32);
    Vec2::new((cx - (cols as f32 - 1.0) / 2.0) * 0.12, (cy - (rows as f32 - 1.0) / 2.0) * 0.13 + if i.is_multiple_of(2) { -0.02 } else { 0.02 })
}

/// Lying flat on the board, the sprite's top along the bench's length.
fn flat() -> Quat {
    Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// Models of what lies on the board, and of what you hold.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn board_models(
    mut commands: Commands,
    time: Res<Time>,
    mut at: ResMut<AtBench>,
    art: Option<Res<ItemArt>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &Transform), With<FlyCamera>>,
    mut models: Query<(Entity, &mut Transform, &BoardModel), Without<FlyCamera>>,
    mut held_model: Query<(Entity, &mut Transform), (With<HeldModel>, Without<BoardModel>, Without<FlyCamera>)>,
    mut showing: Local<Option<Stack>>,
) {
    let Some(art) = art else { return };
    let dt = time.delta_secs().min(0.1);
    at.bump = (at.bump - dt * 3.5).max(0.0);
    let spawn = |commands: &mut Commands, item: ItemId, at_world: Vec3, rot: Quat, marker: bool| -> Option<Entity> {
        let (mesh, mat) = art.model(item)?;
        let px = px_for(art.size(item)?);
        let tf = Transform::from_translation(at_world + Vec3::Y * (px * 0.5 + 0.004)).with_rotation(rot).with_scale(Vec3::splat(px));
        let mut e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), tf, NotShadowCaster));
        if marker {
            e.insert(BoardModel(tf.translation.y));
        } else {
            e.insert(HeldModel);
        }
        Some(e.id())
    };
    // The board: rebuilt when it changes; things hop after a knock.
    if at.dirty {
        at.dirty = false;
        for (e, ..) in &models {
            commands.entity(e).despawn();
        }
        if let Some(st) = at.station {
            let r = rotation(&st) * flat();
            let l = layout(st.kind);
            for (slot, stack) in &at.board.slots {
                let (c, _, y, _) = l.slot(slot);
                // Several small things side by side, a little turned.
                let n = stack.count.min(8) as usize;
                // Long things (sticks) lie side by side across the slot, small ones in rows.
                let long = art.size(stack.item).is_some_and(|z| z.max_element() > 24);
                for i in 0..n {
                    let (off, turn) = if n == 1 {
                        (Vec2::ZERO, Quat::IDENTITY)
                    } else if long {
                        (Vec2::new(0.0, (i as f32 - (n as f32 - 1.0) / 2.0) * 0.07), Quat::IDENTITY)
                    } else {
                        (spread(i, n), Quat::from_rotation_y(i as f32 * 1.3))
                    };
                    spawn(&mut commands, stack.item, to_world(&st, Vec3::new(c.x + off.x, y, c.y + off.y)), turn * r, true);
                }
            }
            if let Some(m) = at.board.made {
                let (c, _, y) = l.made;
                spawn(&mut commands, m.item, to_world(&st, Vec3::new(c.x, y, c.y)), r, true);
            }
        }
    } else if at.bump > 0.0 {
        let hop = (at.bump * std::f32::consts::PI).sin() * 0.03 * at.bump;
        for (_, mut t, base) in &mut models {
            t.translation.y = base.0 + hop;
        }
    }
    // What you hold hangs under the cursor, just above the board.
    if *showing != at.held || at.station.is_none() {
        for (e, ..) in &held_model {
            commands.entity(e).despawn();
        }
        *showing = None;
    }
    let (Some(st), Some(h)) = (at.station, at.held) else {
        at.hand = None;
        return;
    };
    let (cam, tf) = *camera;
    let Some(target) = window
        .cursor_position()
        .and_then(|p| cam.viewport_to_world(&GlobalTransform::from(*tf), p).ok())
        .and_then(|ray| {
            let (o, d) = (ray.origin, ray.direction.as_vec3());
            let y = st.pos.y + layout(st.kind).top() + 0.1;
            (d.y < -1e-4).then(|| o + d * ((y - o.y) / d.y))
        })
    else {
        return;
    };
    let hand = at.hand.map_or(target, |p| p.lerp(target, 1.0 - (-dt * 16.0).exp()));
    at.hand = Some(hand);
    // Tilted the way it's being dragged.
    let lean = (target - hand) * 4.0;
    let rot = Quat::from_rotation_z(-lean.x.clamp(-0.5, 0.5)) * Quat::from_rotation_x(lean.z.clamp(-0.5, 0.5)) * rotation(&st) * flat();
    if *showing != Some(h) {
        spawn(&mut commands, h.item, hand, rot, false);
        *showing = Some(h);
    } else if let Some((_, mut t)) = held_model.iter_mut().next() {
        let px = t.scale.x;
        t.translation = hand + Vec3::Y * (px * 0.5);
        t.rotation = rot;
    }
}

pub(super) fn spawn_board_ui(mut commands: Commands, theme: Res<Theme>) {
    commands
        .spawn((
            BoardUi,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), flex_direction: FlexDirection::Column, justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, padding: UiRect::all(Val::Px(12.0)), ..default() },
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)), row_gap: Val::Px(2.0), ..default() }, theme.skin("tooltip")))
                .with_children(|c| {
                    c.spawn((HintTitle, theme.bold(sizes::HEADING), Text::new(""), TextColor(colors::HEADING)));
                    c.spawn((HintLine, crate::theme::text("", sizes::BODY, colors::TEXT), Node::default()));
                });
            root.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(6.0), ..default() }).with_children(|c| {
                c.spawn((RecipeLines, crate::theme::text("", sizes::SMALL, colors::MUTED), Node::default()));
                c.spawn((Strip, Node { column_gap: Val::Px(2.0), padding: UiRect::all(Val::Px(6.0)), ..default() }, theme.skin("panel_light")));
                c.spawn((
                    crate::theme::text("Click: pick up / set down   ·   Shift: whole stack   ·   Right click: put back   ·   A / D: next bench   ·   Esc: leave", sizes::SMALL, colors::FAINT),
                    Node::default(),
                ));
            });
        });
}

const CELL: f32 = 46.0;

/// The strip of your things, the bench's name and what to do next.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn board_ui(
    mut commands: Commands,
    time: Res<Time>,
    theme: Res<Theme>,
    mut at: ResMut<AtBench>,
    (items, inv, benches): (Res<Items>, Res<Inventory>, Res<Benches>),
    art: Option<Res<ItemArt>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut root: Query<&mut Visibility, With<BoardUi>>,
    strip: Query<(Entity, Option<&Children>), With<Strip>>,
    mut cells: Query<(&StripCell, &ComputedNode, &UiGlobalTransform, &mut ImageNode)>,
    mut texts: ParamSet<(Query<&mut Text, With<HintTitle>>, Query<&mut Text, With<HintLine>>, Query<&mut Text, With<RecipeLines>>)>,
    mut built: Local<Option<Vec<(ItemId, u32)>>>,
) {
    let Ok(mut vis) = root.single_mut() else { return };
    let Some(st) = at.station else {
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
        *built = None;
        return;
    };
    if *vis != Visibility::Inherited {
        *vis = Visibility::Inherited;
    }
    let Some(art) = art else { return };
    // Your things, one cell per kind, in pocket order.
    let mut list: Vec<(ItemId, u32)> = Vec::new();
    for p in &inv.pockets.items {
        if !list.iter().any(|(i, _)| *i == p.stack.item) {
            list.push((p.stack.item, inv.count(p.stack.item)));
        }
    }
    list.truncate(20);
    if built.as_ref() != Some(&list)
        && let Ok((strip, kids)) = strip.single()
    {
        for k in kids.into_iter().flatten() {
            commands.entity(*k).despawn();
        }
        commands.entity(strip).with_children(|row| {
            if list.is_empty() {
                row.spawn((crate::theme::text("Your pockets are empty", sizes::SMALL, colors::MUTED), Node { margin: UiRect::all(Val::Px(8.0)), ..default() }));
            }
            for (item, n) in &list {
                row.spawn((StripCell(*item), Node { width: Val::Px(CELL), height: Val::Px(CELL), ..default() }, theme.skin("slot"))).with_children(|c| {
                    crate::inventory::ui::icon(c, &items, &art, Stack::new(*item, (*n).min(u16::MAX as u32) as u16), CELL, CELL, false);
                    if *n > 1 {
                        c.spawn((
                            crate::theme::text(format!("{n}"), sizes::SMALL, colors::TEXT),
                            Node { position_type: PositionType::Absolute, right: Val::Px(4.0), bottom: Val::Px(0.0), ..default() },
                            GlobalZIndex(1),
                        ));
                    }
                });
            }
        });
        *built = Some(list);
    }
    // Hover.
    let phys = window.physical_cursor_position();
    for (_, node, tf, mut img) in &mut cells {
        let want = if phys.is_some_and(|p| node.contains_point(*tf, p)) { "slot_hover" } else { "slot" };
        let skin = theme.image(want);
        if img.rect != skin.rect {
            *img = skin;
        }
    }
    // What to do next.
    let name = board_name(st.kind).unwrap_or("");
    if let Some((_, left)) = &mut at.note {
        *left -= time.delta_secs();
    }
    if at.note.as_ref().is_some_and(|(_, left)| *left <= 0.0) {
        at.note = None;
    }
    let line = if let Some((n, _)) = &at.note {
        n.clone()
    } else if let Some(m) = at.board.made {
        format!("Click the {} to take it", items.get(m.item).title)
    } else if let Some(r) = benches.0.matching(name, &at.board) {
        format!("{} to make {}  ({} / {})", layout(st.kind).work.0, a_or_some(&items, r.makes), at.board.work, r.work)
    } else if at.held.is_some() {
        let places: Vec<&str> = layout(st.kind).slots.iter().map(|s| s.4).collect();
        format!("Set it down {}", places.join(" or "))
    } else {
        "Pick something from your pockets below".to_string()
    };
    let recipes: Vec<String> = benches
        .0
        .for_station(name)
        .map(|r| {
            let parts: Vec<String> = r
                .slots
                .iter()
                .map(|(slot, s)| {
                    let where_ = layout(st.kind).slot(slot).3;
                    format!("{} {where_}", a_or_some(&items, *s).to_lowercase())
                })
                .collect();
            let (_, go) = layout(st.kind).work;
            let gos = if r.work == 1 { format!("1 {go}") } else { format!("{} {}", r.work, plural(go)) };
            let made = if r.makes.count > 1 { format!("{} x{}", items.get(r.makes.item).title, r.makes.count) } else { items.get(r.makes.item).title.clone() };
            format!("{made}: {}, then {gos}", parts.join(" and "))
        })
        .collect();
    for (q, s) in [(0, st.kind.title().to_string()), (1, line), (2, recipes.join("\n"))] {
        let set = |t: &mut Text| {
            if t.0 != s {
                t.0 = s.clone();
            }
        };
        match q {
            0 => texts.p0().iter_mut().for_each(|mut t| set(&mut t)),
            1 => texts.p1().iter_mut().for_each(|mut t| set(&mut t)),
            _ => texts.p2().iter_mut().for_each(|mut t| set(&mut t)),
        }
    }
}

/// "a Stick", "2 Small Rocks".
fn a_or_some(items: &Items, s: Stack) -> String {
    let title = &items.get(s.item).title;
    if s.count == 1 { format!("a {title}") } else { format!("{} {}", s.count, plural(title)) }
}

fn plural(s: &str) -> String {
    match s.split_once(' ') {
        // "stroke of the saw" -> "strokes of the saw"
        Some((head, rest)) if rest.starts_with("of ") => format!("{} {rest}", plural(head)),
        _ if s.ends_with('s') => s.to_string(),
        _ => format!("{s}s"),
    }
}

/// `BLOCKGAME_BOARD=stick,small_rock*2` steps up to the tool bench as soon as
/// it stands, with those in its slots in order (a third lies there made;
/// `-` leaves a slot empty; `BLOCKGAME_BOARD=carpentry:log` for the carpentry
/// bench; screenshots; needs `BLOCKGAME_CAMP=1` and `BLOCKGAME_URN=tool_bench`).
#[allow(clippy::too_many_arguments)]
pub(super) fn board_for_screenshot(
    stations: Query<&Station>,
    items: Res<Items>,
    player: Res<Player>,
    mut at: ResMut<AtBench>,
    mut menus: ResMut<Menus>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Ok(v) = std::env::var("BLOCKGAME_BOARD") else {
        *done = true;
        return;
    };
    let (kind, v) = match v.split_once(':') {
        Some(("carpentry", rest)) => (StationKind::CarpentryBench, rest.to_string()),
        _ => (StationKind::ToolBench, v.clone()),
    };
    let Some(st) = stations.iter().find(|s| s.kind == kind) else { return };
    *done = true;
    at.enter(*st, player.pos + Vec3::Y * player.eye);
    menus.open = true;
    menus.still = true;
    let defs = slots_of(board_name(kind).unwrap_or_default());
    for (i, part) in v.split(',').map(str::trim).filter(|s| !s.is_empty()).enumerate() {
        let (name, n) = part.split_once('*').map_or((part, 1), |(a, b)| (a, b.parse().unwrap_or(1)));
        // "-" leaves a slot empty.
        let Some(id) = items.id(name) else { continue };
        if let Some(def) = defs.get(i) {
            at.board.put(*def, Stack::new(id, n));
        } else {
            at.board.made = Some(Stack::new(id, n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_looks_down_at_the_board_from_your_side() {
        for turned in [false, true] {
            let st = Station { kind: StationKind::ToolBench, pos: Vec3::new(10.0, 5.0, -3.0), turned };
            for side in [-1.0, 1.0] {
                let v = board_view(&st, side);
                let f = v.forward().as_vec3();
                assert!(f.y < -0.6, "looks down: {f}");
                // Looking through the middle of the view lands on the board.
                let (_, top) = spot(&st, "tool_bench", false, v.translation, f);
                let top = top.unwrap();
                assert!((top - to_world(&st, Vec3::new(0.0, layout(st.kind).top(), side * 0.04))).length() < 0.05, "{top}");
            }
        }
    }

    #[test]
    fn clicks_find_slots_and_the_stone() {
        let st = Station { kind: StationKind::ToolBench, pos: Vec3::new(0.0, 0.0, 0.0), turned: true };
        let down = Vec3::NEG_Y;
        let from = |x: f32, z: f32| to_world(&st, Vec3::new(x, 2.0, z));
        let name = |s: Option<Spot>| match s {
            Some(Spot::Slot(d)) => d.name,
            Some(Spot::Tool) => "stone",
            Some(Spot::Made) => "made",
            None => "none",
        };
        assert_eq!(name(spot(&st, "tool_bench", false, from(-0.3, -0.12), down).0), "handle");
        assert_eq!(name(spot(&st, "tool_bench", false, from(0.32, 0.15), down).0), "material");
        assert_eq!(name(spot(&st, "tool_bench", false, from(0.42, -0.24), down).0), "stone");
        assert_eq!(name(spot(&st, "tool_bench", true, from(-0.3, 0.2), down).0), "made");
        assert_eq!(name(spot(&st, "tool_bench", false, from(-0.3, 0.2), down).0), "none");
        let st = Station { kind: StationKind::CarpentryBench, ..st };
        assert_eq!(name(spot(&st, "carpentry_bench", false, from(-0.4, 0.05), down).0), "bed");
        assert_eq!(name(spot(&st, "carpentry_bench", false, from(0.72, 0.1), down).0), "vise");
        assert_eq!(name(spot(&st, "carpentry_bench", false, from(0.1, -0.3), down).0), "stone");
        assert_eq!(name(spot(&st, "carpentry_bench", true, from(0.2, 0.32), down).0), "made");
    }

    #[test]
    fn every_slot_has_a_place_on_its_bench() {
        for kind in [StationKind::ToolBench, StationKind::CarpentryBench] {
            for s in slots_of(board_name(kind).unwrap()) {
                assert!(layout(kind).slots.iter().any(|l| l.0 == s.name), "{kind:?} {}", s.name);
            }
        }
    }

    #[test]
    fn small_things_spread_without_overlapping() {
        for n in 2..=8 {
            let p: Vec<Vec2> = (0..n).map(|i| spread(i, n)).collect();
            for a in 0..n {
                for b in a + 1..n {
                    assert!(p[a].distance(p[b]) > 0.09, "{n}: {a} {b}");
                }
            }
        }
    }
}
