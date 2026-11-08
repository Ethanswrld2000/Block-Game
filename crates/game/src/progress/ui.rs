//! The XP bar and focus meter on the HUD, and the skill tree screen (K): a
//! map you drag to pan and scroll to zoom. Click a node to learn it.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::skills::{Node as SkillNode, Perks, Tree};
use super::xp::Progression;
use super::{Progress, Spells};
use crate::camera::Menus;

pub struct ProgressUiPlugin;

impl Plugin for ProgressUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TreeView>()
            .add_systems(Startup, (spawn_hud, spawn_screen))
            .add_systems(Update, (update_hud, open_close, pan_zoom, click, layout).chain());
    }
}

fn text(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font_size: bevy::text::FontSize::Px(size), ..default() }, TextColor(color), TextShadow { offset: Vec2::splat(1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.85) })
}

#[derive(Component)]
struct XpFill;
#[derive(Component)]
struct LevelText;
#[derive(Component)]
struct FocusPanel;
#[derive(Component)]
struct FocusFill;
#[derive(Component)]
struct SpellText;

const XP_W: f32 = 660.0;
const FOCUS_W: f32 = 170.0;

fn spawn_hud(mut commands: Commands) {
    // XP: a thin bar just above the hotbar, the level to its left.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(60.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|r| {
            r.spawn((LevelText, text("1", 13.0, Color::srgb(0.95, 0.85, 0.45))));
            r.spawn((
                Node { width: Val::Px(XP_W), height: Val::Px(4.0), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            ))
            .with_children(|b| {
                b.spawn((XpFill, Node { width: Val::Px(0.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgb(0.95, 0.8, 0.3))));
            });
        });
    // Focus: above the health bars, once you know a spell.
    commands
        .spawn((
            FocusPanel,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(14.0),
                bottom: Val::Px(172.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                row.spawn(text("Focus", 13.0, Color::srgb(0.75, 0.85, 1.0)));
                row.spawn((Node { width: Val::Px(FOCUS_W), height: Val::Px(8.0), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5))))
                    .with_children(|b| {
                        b.spawn((FocusFill, Node { width: Val::Px(FOCUS_W), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgb(0.45, 0.65, 1.0))));
                    });
            });
            p.spawn((SpellText, text("", 12.0, Color::srgb(0.8, 0.88, 1.0))));
        });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_hud(
    prog: Res<Progression>,
    progress: Res<Progress>,
    perks: Res<Perks>,
    spells: Res<Spells>,
    mut xp: Query<&mut Node, (With<XpFill>, Without<FocusFill>)>,
    mut focus: Query<&mut Node, (With<FocusFill>, Without<XpFill>)>,
    mut level: Query<&mut Text, (With<LevelText>, Without<SpellText>)>,
    mut spell_text: Query<&mut Text, (With<SpellText>, Without<LevelText>)>,
    mut panel: Query<&mut Visibility, With<FocusPanel>>,
) {
    for mut n in &mut xp {
        n.width = Val::Px(XP_W * progress.level.fraction(&prog));
    }
    for mut t in &mut level {
        let s = if progress.learned.points > 0 {
            format!("Lv {}  ({} pt, K)", progress.level.level, progress.learned.points)
        } else {
            format!("Lv {}", progress.level.level)
        };
        if t.0 != s {
            t.0 = s;
        }
    }
    let known = !perks.spells.is_empty();
    for mut v in &mut panel {
        let want = if known { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    for mut n in &mut focus {
        n.width = Val::Px(FOCUS_W * (progress.focus / perks.max_focus()).clamp(0.0, 1.0));
    }
    let mut on = Vec::new();
    for (active, name, key) in [(spells.levitate, "Levitating", "G"), (spells.light, "Light", "X"), (spells.warmth, "Warmth", "V")] {
        if active {
            on.push(format!("{name} ({key})"));
        }
    }
    if spells.feather {
        on.push("Drifting down".into());
    }
    let s = if on.is_empty() {
        perks.spells.iter().map(|s| format!("{}: {}", match s {
            super::Spell::Levitation => "G",
            super::Spell::Light => "X",
            super::Spell::Warmth => "V",
        }, s.title())).collect::<Vec<_>>().join("  ")
    } else {
        on.join("  ")
    };
    for mut t in &mut spell_text {
        if t.0 != s {
            t.0 = s.clone();
        }
    }
}

/// Pan and zoom of the tree map, and what the mouse is over.
#[derive(Resource)]
struct TreeView {
    open: bool,
    /// Map point at the centre of the screen.
    pan: Vec2,
    zoom: f32,
    hover: Option<SkillNode>,
    dragged: f32,
    message: String,
    built: bool,
}

impl Default for TreeView {
    fn default() -> Self {
        Self { open: false, pan: Vec2::new(0.0, -160.0), zoom: 0.7, hover: None, dragged: 0.0, message: String::new(), built: false }
    }
}

#[derive(Component)]
struct Screen;
#[derive(Component)]
struct MapNode(SkillNode);
#[derive(Component)]
struct MapLabel(SkillNode);
#[derive(Component)]
struct MapEdge {
    a: SkillNode,
    b: SkillNode,
}
#[derive(Component)]
struct EdgeLabel {
    a: SkillNode,
    b: SkillNode,
}
#[derive(Component)]
struct Info;
#[derive(Component)]
struct Header;

fn spawn_screen(mut commands: Commands, tree: Res<Tree>) {
    commands
        .spawn((
            Screen,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), overflow: Overflow::clip(), ..default() },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.06, 0.985)),
            GlobalZIndex(20),
            Visibility::Hidden,
        ))
        .with_children(|s| {
            // Edges first so nodes draw over them.
            let mut edges = Vec::new();
            for &(a, b, _) in &tree.links {
                edges.push((SkillNode::Major(a), SkillNode::Major(b), true));
            }
            for m in 0..tree.majors.len() {
                for i in 0..super::skills::RING {
                    edges.push((SkillNode::Major(m), SkillNode::Minor(m, i), false));
                }
            }
            for (a, b, major) in &edges {
                s.spawn((
                    MapEdge { a: *a, b: *b },
                    Node { position_type: PositionType::Absolute, height: Val::Px(if *major { 4.0 } else { 2.0 }), ..default() },
                    BackgroundColor(Color::srgba(0.5, 0.5, 0.55, 0.5)),
                    UiTransform::default(),
                ));
            }
            for &(a, b, w) in &tree.links {
                let (a, b) = (SkillNode::Major(a), SkillNode::Major(b));
                s.spawn((EdgeLabel { a, b }, Node { position_type: PositionType::Absolute, ..default() }, text(w.to_string(), 13.0, Color::srgb(0.9, 0.8, 0.55))));
            }
            for n in tree.nodes() {
                s.spawn((
                    MapNode(n),
                    Node {
                        position_type: PositionType::Absolute,
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::MAX,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.2, 0.2, 0.22)),
                    BorderColor::all(Color::srgb(0.4, 0.4, 0.45)),
                ))
                .with_children(|c| {
                    if let SkillNode::Minor(m, i) = n {
                        c.spawn(text(tree.majors[m].ring[i].cost.to_string(), 11.0, Color::WHITE));
                    }
                });
                if let SkillNode::Major(m) = n {
                    s.spawn((MapLabel(n), Node { position_type: PositionType::Absolute, ..default() }, text(tree.majors[m].title.clone(), 16.0, Color::srgb(0.95, 0.92, 0.85))));
                }
            }
            s.spawn((
                Header,
                Node { position_type: PositionType::Absolute, top: Val::Px(14.0), left: Val::Px(18.0), ..default() },
                text("", 18.0, Color::srgb(0.95, 0.88, 0.6)),
            ));
            s.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(14.0),
                    left: Val::Px(18.0),
                    max_width: Val::Px(560.0),
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ))
            .with_children(|p| {
                p.spawn((Info, text("", 15.0, Color::srgb(0.9, 0.9, 0.9))));
            });
        });
}

fn open_close(
    keys: Res<ButtonInput<KeyCode>>,
    mut view: ResMut<TreeView>,
    mut menus: ResMut<Menus>,
    mut screen: Query<&mut Visibility, With<Screen>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let forced = std::env::var_os("BLOCKGAME_SKILLTREE").is_some();
    let mut want = view.open;
    if keys.just_pressed(KeyCode::KeyK) {
        want = !view.open;
    } else if view.open && keys.just_pressed(KeyCode::Escape) {
        want = false;
    }
    if forced && !view.built {
        want = true;
    }
    view.built = true;
    // Another screen (the inventory) is up.
    if want && !view.open && menus.open {
        return;
    }
    if want == view.open {
        return;
    }
    view.open = want;
    menus.open = want;
    for mut v in &mut screen {
        *v = if want { Visibility::Inherited } else { Visibility::Hidden };
    }
    if !want {
        view.message.clear();
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

fn to_screen(view: &TreeView, size: Vec2, p: (f32, f32)) -> Vec2 {
    size * 0.5 + (Vec2::new(p.0, p.1) - view.pan) * view.zoom
}

fn pan_zoom(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window, With<PrimaryWindow>>,
    tree: Res<Tree>,
    mut view: ResMut<TreeView>,
) {
    if !view.open {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        view.dragged = 0.0;
    }
    if mouse.pressed(MouseButton::Left) && motion.delta != Vec2::ZERO {
        let z = view.zoom;
        view.pan -= motion.delta / z;
        view.dragged += motion.delta.length();
    }
    let size = window.size();
    if scroll.delta.y != 0.0 {
        // Zoom about the cursor.
        let cursor = window.cursor_position().unwrap_or(size * 0.5);
        let before = view.pan + (cursor - size * 0.5) / view.zoom;
        view.zoom = (view.zoom * (1.0 + scroll.delta.y.signum() * 0.12)).clamp(0.35, 2.5);
        view.pan = before - (cursor - size * 0.5) / view.zoom;
    }
    let cursor = window.cursor_position();
    view.hover = cursor.and_then(|c| {
        tree.nodes()
            .map(|n| (n, to_screen(&view, size, tree.pos(n)).distance(c)))
            .filter(|(n, d)| *d < radius(*n, view.zoom) + 3.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n)
    });
}

fn radius(n: SkillNode, zoom: f32) -> f32 {
    (if matches!(n, SkillNode::Major(_)) { 22.0 } else { 12.0 }) * zoom.max(0.6)
}

fn click(mouse: Res<ButtonInput<MouseButton>>, tree: Res<Tree>, mut view: ResMut<TreeView>, mut progress: ResMut<Progress>) {
    if !view.open || !mouse.just_released(MouseButton::Left) || view.dragged > 6.0 {
        return;
    }
    let Some(n) = view.hover else { return };
    view.message = match progress.learned.take(&tree, n) {
        Ok(()) => format!("Learned {}", tree.title(n)),
        Err(e) => format!("{}: {e}", tree.title(n)),
    };
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn layout(
    window: Single<&Window, With<PrimaryWindow>>,
    tree: Res<Tree>,
    view: Res<TreeView>,
    progress: Res<Progress>,
    mut nodes: Query<(&MapNode, &mut Node, &mut BackgroundColor, &mut BorderColor), (Without<MapEdge>, Without<MapLabel>, Without<EdgeLabel>)>,
    mut labels: Query<(&MapLabel, &mut Node), (Without<MapNode>, Without<MapEdge>, Without<EdgeLabel>)>,
    mut edges: Query<(&MapEdge, &mut Node, &mut UiTransform, &mut BackgroundColor), (Without<MapNode>, Without<MapLabel>, Without<EdgeLabel>)>,
    mut edge_labels: Query<(&EdgeLabel, &mut Node), (Without<MapNode>, Without<MapLabel>, Without<MapEdge>)>,
    mut texts: ParamSet<(Query<&mut Text, With<Info>>, Query<&mut Text, With<Header>>)>,
) {
    if !view.open {
        return;
    }
    let size = window.size();
    let learned = &progress.learned;
    let at = |n: SkillNode| to_screen(&view, size, tree.pos(n));
    for (m, mut node, mut bg, mut border) in &mut nodes {
        let r = radius(m.0, view.zoom);
        let c = at(m.0);
        node.left = Val::Px(c.x - r);
        node.top = Val::Px(c.y - r);
        node.width = Val::Px(r * 2.0);
        node.height = Val::Px(r * 2.0);
        let taken = learned.taken.contains(&m.0);
        let closed = tree.closed(&learned.taken, m.0);
        let cost = tree.cost(&learned.taken, m.0);
        let (fill, edge) = if taken {
            (Color::srgb(0.85, 0.65, 0.2), Color::srgb(1.0, 0.9, 0.5))
        } else if closed {
            (Color::srgb(0.22, 0.08, 0.08), Color::srgb(0.4, 0.15, 0.15))
        } else if cost.is_some_and(|c| c <= learned.points) {
            (Color::srgb(0.18, 0.32, 0.22), Color::srgb(0.45, 0.95, 0.55))
        } else if cost.is_some() {
            (Color::srgb(0.16, 0.2, 0.3), Color::srgb(0.45, 0.55, 0.8))
        } else {
            (Color::srgb(0.12, 0.12, 0.14), Color::srgb(0.3, 0.3, 0.33))
        };
        let hover = view.hover == Some(m.0);
        bg.0 = fill;
        *border = BorderColor::all(if hover { Color::WHITE } else { edge });
    }
    for (l, mut node) in &mut labels {
        let c = at(l.0);
        node.left = Val::Px(c.x - 50.0);
        node.top = Val::Px(c.y + radius(l.0, view.zoom) + 4.0);
        node.width = Val::Px(100.0);
        node.justify_content = JustifyContent::Center;
    }
    for (e, mut node, mut tf, mut bg) in &mut edges {
        let (a, b) = (at(e.a), at(e.b));
        let d = b - a;
        let len = d.length();
        let mid = (a + b) * 0.5;
        node.width = Val::Px(len);
        node.left = Val::Px(mid.x - len * 0.5);
        node.top = Val::Px(mid.y - 1.0);
        tf.rotation = Rot2::radians(d.y.atan2(d.x));
        let lit = learned.taken.contains(&e.a) && learned.taken.contains(&e.b);
        bg.0 = if lit { Color::srgba(0.95, 0.75, 0.3, 0.9) } else { Color::srgba(0.5, 0.5, 0.58, 0.45) };
    }
    for (e, mut node) in &mut edge_labels {
        let mid = (at(e.a) + at(e.b)) * 0.5;
        node.left = Val::Px(mid.x + 6.0);
        node.top = Val::Px(mid.y - 18.0);
    }
    let info = match view.hover {
        Some(n) => {
            let gives = tree.gives(n).iter().map(|g| g.describe()).collect::<Vec<_>>().join(", ");
            let state = match learned.check(&tree, n) {
                Ok(c) => format!("click to learn for {c} point{}", if c == 1 { "" } else { "s" }),
                Err(e) => e.to_string(),
            };
            format!("{}: {gives}\n{state}", tree.title(n))
        }
        None if !view.message.is_empty() => view.message.clone(),
        None => "Drag to move, scroll to zoom, click a node to learn it. Edge numbers are costs; taking a ring node closes its neighbours.".into(),
    };
    for mut t in &mut texts.p0() {
        if t.0 != info {
            t.0 = info.clone();
        }
    }
    let header = format!("Skills: level {}, {} point{} to spend   (K or Esc to close)", progress.level.level, learned.points, if learned.points == 1 { "" } else { "s" });
    for mut t in &mut texts.p1() {
        if t.0 != header {
            t.0 = header.clone();
        }
    }
}
