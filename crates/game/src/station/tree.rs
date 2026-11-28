//! An urn's tree: right click an urn with an empty hand and you look down into
//! its dark. A hole opens from the urn's mouth over the screen (a UI shader,
//! `assets/shaders/urn_void.wesl`) and the urn's steps rise out of it as a tree
//! growing upward: each step stands above the steps it comes after, joined by
//! lines that light up as you go. Pointing at a step shows, on the right, what
//! finishing it gets you (the station, the upgrade, the recipes it teaches with
//! pictures, or the urn), what it takes and what you carry. "Put in" moves what
//! you carry for that step into the urn.

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::ui_render::prelude::{MaterialNode, UiMaterial};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::urns::{Gives, Progress, State, Step, UrnDef};
use super::{board::Benches, StationKind, StepDone, UrnBook, Urns};
use crate::camera::{FlyCamera, Menus};
use crate::inventory::grid::Stack;
use crate::inventory::Inventory;
use crate::item_art::ItemArt;
use crate::items::Items;
use crate::survival::Toast;
use crate::theme::{colors, sizes, text, Theme};

/// Which urn's tree is open (index into urns.ron).
#[derive(Resource, Default)]
pub struct TreeUi {
    pub open: Option<usize>,
    pub dirty: bool,
    /// The urn's mouth, where the dark opens from (none: the middle of the screen).
    pub at: Option<Vec3>,
    /// The step shown on the right.
    selected: Option<String>,
    detail_dirty: bool,
    /// Seconds since it opened.
    shown: f32,
}

/// The dark behind the tree.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct UrnVoid {
    #[uniform(0)]
    params: VoidParams,
}

#[derive(Clone, Copy, Default, ShaderType)]
struct VoidParams {
    /// The dark's clock, how far open (0..1), pixel size.
    look: Vec4,
    /// The urn's colour; `w` = how much the world outside is dimmed.
    tint: Vec4,
    /// Where the mouth is (0..1 of the screen) and its radius in pixels.
    centre: Vec4,
}

impl UiMaterial for UrnVoid {
    fn fragment_shader() -> ShaderRef {
        "shaders/urn_void.wesl".into()
    }
}

#[derive(Component)]
pub(super) struct TreeRoot;

#[derive(Component)]
pub(super) struct TreePanel;

#[derive(Component)]
pub(super) struct DetailPanel;

/// A step on the tree (hover or click shows it on the right).
#[derive(Component)]
pub(super) struct TreeNode(String);

/// The frame round a step's picture.
#[derive(Component)]
pub(super) struct NodeFrame(pub(super) String);

/// A halo that breathes behind steps you can fill now.
#[derive(Component)]
pub(super) struct Breathe;

/// "Arrange": move the urn's stations round it (`arrange`).
#[derive(Component)]
pub(super) struct ArrangeButton;

/// "Put in" on a step.
#[derive(Component)]
pub(super) struct PutIn(String);

/// How long the dark takes to open, and when the tree shows in it.
const OPEN_SECONDS: f32 = 0.85;
const TREE_AT: f32 = 0.55;

const TIERS: [&str; 3] = ["Near home", "A trip out", "Far finds"];
const NUMERALS: [&str; 3] = ["I", "II", "III"];

/// The tree's area and its steps' boxes, in pixels.
const TREE_W: f32 = 700.0;
const NODE_W: f32 = 150.0;
const NODE_H: f32 = 86.0;
const ROW: f32 = 104.0;
const FRAME: f32 = 56.0;

/// Where each step stands: (level from the root up, place in its row, steps in
/// its row). A step's level is one above the highest step it comes after; a
/// row is ordered by where the steps it comes after stand, so lines cross
/// little.
pub fn tree_layout(def: &UrnDef) -> Vec<(usize, usize, usize)> {
    let n = def.steps.len();
    let index = |name: &str| def.steps.iter().position(|s| s.name == name);
    let mut level = vec![0usize; n];
    // Steps only come after steps of the same urn; settle in at most n passes.
    for _ in 0..n {
        let mut changed = false;
        for (i, s) in def.steps.iter().enumerate() {
            let l = s.after.iter().filter_map(|a| index(a)).map(|j| level[j] + 1).max().unwrap_or(0);
            if l != level[i] && l <= n {
                level[i] = l;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let levels = level.iter().copied().max().map_or(0, |m| m + 1);
    let mut x = vec![0.5f32; n];
    let mut out = vec![(0, 0, 1); n];
    for l in 0..levels {
        let mut row: Vec<usize> = (0..n).filter(|&i| level[i] == l).collect();
        let key = |i: usize| {
            let ps: Vec<f32> = def.steps[i].after.iter().filter_map(|a| index(a)).map(|j| x[j]).collect();
            if ps.is_empty() { 0.5 } else { ps.iter().sum::<f32>() / ps.len() as f32 }
        };
        let keys: Vec<f32> = row.iter().map(|&i| key(i)).collect();
        let mut order: Vec<usize> = (0..row.len()).collect();
        order.sort_by(|&a, &b| keys[a].total_cmp(&keys[b]).then(row[a].cmp(&row[b])));
        row = order.into_iter().map(|k| row[k]).collect();
        let count = row.len();
        for (slot, &i) in row.iter().enumerate() {
            x[i] = (slot as f32 + 0.5) / count as f32;
            out[i] = (l, slot, count);
        }
    }
    out
}

pub(super) fn spawn_tree_ui(mut commands: Commands, mut mats: ResMut<Assets<UrnVoid>>) {
    commands
        .spawn((
            TreeRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            MaterialNode(mats.add(UrnVoid { params: VoidParams::default() })),
            GlobalZIndex(25),
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn((TreePanel, Node { flex_direction: FlexDirection::Column, ..default() }, UiTransform::default(), Visibility::Hidden));
        });
}

/// Esc or Tab closes the tree. `BLOCKGAME_URNTREE=tool` opens one for screenshots
/// (`BLOCKGAME_URNOPEN=0.4` holds the opening that far in, 0..1).
#[allow(clippy::too_many_arguments)]
pub(super) fn tree_keys(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    book: Option<Res<UrnBook>>,
    mut tree: ResMut<TreeUi>,
    mut menus: ResMut<Menus>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    inv: Res<Inventory>,
    urns: Res<Urns>,
    mut forced: Local<bool>,
) {
    if !*forced {
        *forced = true;
        if let (Ok(v), Some(book)) = (std::env::var("BLOCKGAME_URNTREE"), &book) {
            tree.open = book.0.urns.iter().position(|u| u.name == v);
            tree.dirty = true;
            menus.open = tree.open.is_some();
        }
    }
    if tree.open.is_none() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Tab) {
        // Used up here, so the pause menu or the inventory don't open too.
        keys.clear_just_pressed(KeyCode::Escape);
        keys.clear_just_pressed(KeyCode::Tab);
        tree.open = None;
        tree.at = None;
        tree.dirty = true;
        menus.open = false;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        return;
    }
    if !cursor.visible {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
    if inv.is_changed() || urns.is_changed() {
        tree.dirty = true;
    }
}

/// Open the dark from the urn's mouth, show the tree in it, breathe the open steps.
#[allow(clippy::too_many_arguments)]
pub(super) fn tree_animate(
    time: Res<Time<Real>>,
    mut tree: ResMut<TreeUi>,
    book: Option<Res<UrnBook>>,
    cam: Single<(&Camera, &GlobalTransform), With<FlyCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    root: Query<&MaterialNode<UrnVoid>, With<TreeRoot>>,
    mut mats: ResMut<Assets<UrnVoid>>,
    mut panel: Query<(&mut Visibility, &mut UiTransform), With<TreePanel>>,
    mut halos: Query<&mut ImageNode, With<Breathe>>,
    mut last: Local<Option<usize>>,
    mut hold: Local<Option<Option<f32>>>,
) {
    let hold = *hold.get_or_insert_with(|| std::env::var("BLOCKGAME_URNOPEN").ok().and_then(|v| v.parse().ok()));
    if *last != tree.open {
        *last = tree.open;
        tree.shown = 0.0;
    }
    let (Some(i), Some(book)) = (tree.open, book) else { return };
    tree.shown += time.delta_secs();
    let t = tree.shown;
    let open = hold.unwrap_or((t / OPEN_SECONDS).min(1.0));
    // The mouth on screen, and how wide it looks from here.
    let (camera, ct) = *cam;
    let size = window.size();
    let (mut centre, mut radius) = (Vec2::new(0.5, 0.55), 24.0);
    if let Some(at) = tree.at
        && let Ok(p) = camera.world_to_viewport(ct, at)
    {
        centre = p / size.max(Vec2::ONE);
        if let Ok(q) = camera.world_to_viewport(ct, at + ct.right() * super::grab::MOUTH_HALF) {
            radius = (q - p).length().clamp(6.0, 200.0) * window.scale_factor();
        }
    }
    let c = book.0.urns[i].color;
    let glow = LinearRgba::from(Color::srgb(c.0, c.1, c.2));
    if let Ok(node) = root.single()
        && let Some(mut m) = mats.get_mut(&node.0)
    {
        m.params = VoidParams {
            // The same clock as the mouth's dark in the world (`grab`), so it carries on unbroken.
            look: Vec4::new(super::grab::void_clock(&time), open, 4.0 * window.scale_factor(), 0.0),
            tint: Vec4::new(glow.red, glow.green, glow.blue, 0.55),
            centre: Vec4::new(centre.x, centre.y, radius, 0.0),
        };
    }
    if let Ok((mut vis, mut tf)) = panel.single_mut() {
        let k: f32 = ((open - TREE_AT) / (1.0 - TREE_AT)).clamp(0.0, 1.0);
        let want = if open >= TREE_AT { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        // Rises a little out of the dark as it shows.
        let ease = 1.0 - (1.0 - k).powi(3);
        tf.scale = Vec2::splat(0.94 + 0.06 * ease);
        tf.translation = Val2::px(0.0, 24.0 * (1.0 - ease));
    }
    let breath = 0.55 + 0.45 * (t * 2.4).sin();
    for mut img in &mut halos {
        img.color = img.color.with_alpha(breath);
    }
}

/// Point at a step to see it; click "Put in".
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn tree_click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    theme: Res<Theme>,
    mut buttons: Query<(Option<&PutIn>, &ComputedNode, &UiGlobalTransform, &InheritedVisibility, &mut ImageNode), (Or<(With<PutIn>, With<ArrangeButton>)>, Without<NodeFrame>)>,
    mut arrange: ResMut<super::arrange::Arrange>,
    nodes: Query<(&TreeNode, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
    mut frames: Query<(&NodeFrame, &mut ImageNode), Without<PutIn>>,
    (book, items): (Option<Res<UrnBook>>, Res<Items>),
    mut tree: ResMut<TreeUi>,
    mut inv: ResMut<Inventory>,
    mut urns: ResMut<Urns>,
    mut done: MessageWriter<StepDone>,
    mut toast: MessageWriter<Toast>,
    mut fly: MessageWriter<super::fly_in::FlyIn>,
) {
    let (Some(i), Some(book)) = (tree.open, book) else { return };
    let def = &book.0.urns[i];
    let p = window.physical_cursor_position();
    // Pointing at a step shows it (it stays shown when the cursor leaves).
    let mut over_step = None;
    for (n, node, tf, vis) in &nodes {
        if vis.get() && p.is_some_and(|p| node.contains_point(*tf, p)) {
            over_step = Some(n.0.clone());
        }
    }
    if let Some(s) = &over_step
        && tree.selected.as_ref() != Some(s)
    {
        tree.selected = Some(s.clone());
        tree.detail_dirty = true;
    }
    for (f, mut img) in &mut frames {
        let want = if over_step.as_ref() == Some(&f.0) {
            "slot_hover"
        } else if tree.selected.as_ref() == Some(&f.0) {
            "slot_selected"
        } else {
            "slot"
        };
        let skin = theme.image(want);
        if img.rect != skin.rect {
            img.rect = skin.rect;
        }
    }
    let mut clicked = None;
    for (b, node, tf, vis, mut img) in &mut buttons {
        let over = vis.get() && p.is_some_and(|p| node.contains_point(*tf, p));
        let want = if over && mouse.pressed(MouseButton::Left) {
            "button_down"
        } else if over {
            "button_hover"
        } else {
            "button"
        };
        let skin = theme.image(want);
        if img.rect != skin.rect {
            *img = skin;
        }
        if over && mouse.just_pressed(MouseButton::Left) {
            clicked = Some(b.map(|b| b.0.clone()));
        }
    }
    let Some(step) = clicked else { return };
    let Some(step) = step else {
        // Arrange: the tree closes and the camera lifts out over the urn.
        tree.open = None;
        tree.at = None;
        tree.dirty = true;
        arrange.start = Some(i);
        return;
    };
    let Some(s) = def.step(&step) else { return };
    let mut put = 0;
    let missing = urns.of(&def.name).missing(s);
    for (name, want) in missing {
        let Some(id) = items.id(&name) else { continue };
        let n = inv.count(id).min(want);
        if n == 0 || !inv.take(id, n) {
            continue;
        }
        let (went, finished) = urns.of_mut(&def.name).offer(def, &name, n, Some(&step));
        if went < n {
            inv.add(&items, Stack::new(id, (n - went) as u16));
        }
        put += went;
        if went > 0 {
            // It flies up from below the screen into its picture.
            fly.write(super::fly_in::FlyIn { item: name.clone(), step: step.clone() });
        }
        for f in finished {
            done.write(StepDone(f));
        }
    }
    if put == 0 {
        toast.write(Toast(format!("You carry nothing the {} needs yet", s.title)));
    }
}

/// What a step's picture is: the urn or the first thing it teaches as an item
/// sprite, else a UI icon.
enum Picture {
    Item(Stack),
    Icon(&'static str),
}

fn picture(s: &Step, book: &UrnBook, benches: &Benches, items: &Items) -> Picture {
    match &s.gives {
        Gives::Urn(u) => match book.0.urn(u).and_then(|u| u.item.as_deref()).and_then(|n| items.id(n)) {
            Some(id) => Picture::Item(Stack::new(id, 1)),
            None => Picture::Icon("icon_star"),
        },
        Gives::Recipes => match benches.0.recipes.iter().find(|r| r.taught_by(&s.name)) {
            Some(r) => Picture::Item(Stack::new(r.makes.item, 1)),
            None => Picture::Icon("icon_star"),
        },
        Gives::Station(_) => Picture::Icon("icon_camp"),
        Gives::Upgrade => Picture::Icon("icon_star"),
    }
}

fn draw_picture(c: &mut ChildSpawnerCommands, theme: &Theme, items: &Items, art: &ItemArt, pic: &Picture, px: f32, dim: bool) {
    match pic {
        Picture::Item(stack) => {
            c.spawn(Node { width: Val::Px(px), height: Val::Px(px), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() })
                .with_children(|i| crate::inventory::ui::icon(i, items, art, *stack, px, px, false));
        }
        Picture::Icon(name) => {
            let tint = if dim { Color::srgba(0.6, 0.6, 0.65, 0.7) } else { Color::WHITE };
            c.spawn((theme.image(name).with_color(tint), Node { width: Val::Px(px * 0.8), height: Val::Px(px * 0.8), ..default() }));
        }
    }
}

/// Build the open tree, or just the step shown on the right.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn tree_build(
    mut commands: Commands,
    theme: Res<Theme>,
    mut tree: ResMut<TreeUi>,
    (book, items, art, benches): (Option<Res<UrnBook>>, Res<Items>, Option<Res<ItemArt>>, Option<Res<Benches>>),
    (urns, inv): (Res<Urns>, Res<Inventory>),
    mut root: Query<&mut Visibility, (With<TreeRoot>, Without<TreePanel>)>,
    panel: Query<(Entity, Option<&Children>), With<TreePanel>>,
    detail: Query<(Entity, Option<&Children>), With<DetailPanel>>,
) {
    if !tree.dirty && !tree.detail_dirty {
        return;
    }
    let (Some(book), Some(art), Some(benches)) = (book, art, benches) else { return };
    let Ok(mut vis) = root.single_mut() else { return };
    let Some(i) = tree.open else {
        tree.dirty = false;
        tree.detail_dirty = false;
        *vis = Visibility::Hidden;
        if let Ok((_, kids)) = panel.single() {
            for k in kids.into_iter().flatten() {
                commands.entity(*k).despawn();
            }
        }
        return;
    };
    let def = &book.0.urns[i];
    let p = urns.of(&def.name);
    let theme = &*theme;
    // Show the first step you can fill (else the first not done) unless one is picked.
    if tree.selected.as_deref().and_then(|s| def.step(s)).is_none() {
        tree.selected = def
            .steps
            .iter()
            .find(|s| p.state(s) == State::Open)
            .or_else(|| def.steps.iter().find(|s| p.state(s) != State::Done))
            .or(def.steps.first())
            .map(|s| s.name.clone());
    }
    let selected = tree.selected.clone().unwrap_or_default();
    if !tree.dirty {
        // Only the right side changed.
        tree.detail_dirty = false;
        if let Ok((d, kids)) = detail.single() {
            for k in kids.into_iter().flatten() {
                commands.entity(*k).despawn();
            }
            commands.entity(d).with_children(|c| details(c, theme, &items, &art, &benches, &book, &inv, def, p, &selected));
        }
        return;
    }
    tree.dirty = false;
    tree.detail_dirty = false;
    *vis = Visibility::Inherited;
    let Ok((panel, kids)) = panel.single() else { return };
    for k in kids.into_iter().flatten() {
        commands.entity(*k).despawn();
    }
    let spots = tree_layout(def);
    let levels = spots.iter().map(|s| s.0 + 1).max().unwrap_or(1);
    let tree_h = levels as f32 * ROW;
    // Centre of a step's box in the tree area (root at the bottom).
    let centre = |k: usize| {
        let (l, slot, count) = spots[k];
        Vec2::new(TREE_W * (slot as f32 + 0.5) / count as f32, tree_h - ROW * (l as f32 + 0.5))
    };
    commands.entity(panel).insert(Node {
        flex_direction: FlexDirection::Column,
        width: Val::Px(1160.0),
        max_width: Val::Percent(97.0),
        max_height: Val::Percent(97.0),
        row_gap: Val::Px(6.0),
        ..default()
    });
    commands.entity(panel).with_children(|c| {
        // Title and progress.
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::FlexEnd, ..default() }).with_children(|r| {
            r.spawn(Node { column_gap: Val::Px(12.0), align_items: AlignItems::FlexEnd, ..default() }).with_children(|r| {
                r.spawn((theme.bold(sizes::TITLE), Text::new(def.title.clone()), TextColor(colors::HEADING)));
                r.spawn((text(def.station.clone(), sizes::LABEL, colors::MUTED), Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() }));
            });
            let n = def.steps.iter().filter(|s| p.is_done(&s.name)).count();
            // Arrange shows once a step has placed a station by the urn.
            let placed = def.steps.iter().any(|s| p.is_done(&s.name) && matches!(&s.gives, Gives::Station(_)));
            r.spawn(Node { column_gap: Val::Px(16.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                if placed {
                    r.spawn((
                        ArrangeButton,
                        Node { padding: UiRect::axes(Val::Px(14.0), Val::Px(3.0)), justify_content: JustifyContent::Center, ..default() },
                        theme.skin("button"),
                        children![text("Arrange", sizes::LABEL, colors::TEXT)],
                    ));
                }
                r.spawn(text(format!("{n} / {} steps", def.steps.len()), sizes::LABEL, if n == def.steps.len() { colors::GOOD } else { colors::TEXT }));
            });
        });
        c.spawn(text(def.how.clone(), sizes::SMALL, colors::MUTED));
        c.spawn(Node { column_gap: Val::Px(20.0), align_items: AlignItems::Center, ..default() }).with_children(|row| {
            // The tree: lines first, then the steps over them.
            row.spawn(Node { width: Val::Px(TREE_W), height: Val::Px(tree_h), flex_shrink: 0.0, ..default() }).with_children(|t| {
                for (k, s) in def.steps.iter().enumerate() {
                    for a in &s.after {
                        let Some(j) = def.steps.iter().position(|x| &x.name == a) else { continue };
                        // From the top of the earlier step's picture to the foot of this one's title.
                        let from = centre(j) - Vec2::Y * (NODE_H * 0.5 - 2.0);
                        let to = centre(k) + Vec2::Y * (NODE_H * 0.5 - 2.0);
                        let color = match (p.is_done(a), p.state(s)) {
                            (true, State::Done) => colors::GOLD,
                            (true, _) => glow_of(def).with_alpha(0.9),
                            _ => Color::srgba(0.45, 0.42, 0.55, 0.45),
                        };
                        line(t, from, to, color);
                    }
                }
                for (k, s) in def.steps.iter().enumerate() {
                    let at = centre(k);
                    step_node(t, theme, &items, &art, &book, &benches, def, p, s, at, s.name == selected);
                }
            });
            row.spawn((
                DetailPanel,
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    min_height: Val::Px(tree_h.min(420.0)),
                    padding: UiRect::axes(Val::Px(18.0), Val::Px(14.0)),
                    row_gap: Val::Px(6.0),
                    ..default()
                },
                theme.skin("panel"),
            ))
            .with_children(|c| details(c, theme, &items, &art, &benches, &book, &inv, def, p, &selected));
        });
        c.spawn(text(
            "Point at a step to see what it gives  ·  Put in: everything you carry for that step  ·  Hold an item and right click the urn to put it in  ·  Arrange: move its stations  ·  Esc: close",
            sizes::SMALL,
            colors::FAINT,
        ));
    });
}

fn glow_of(def: &UrnDef) -> Color {
    let (r, g, b) = def.color;
    // The urn's clay, brightened into the glow of its marks.
    let c = Vec3::new(r, g, b);
    let c = c / c.max_element().max(0.01);
    Color::srgb(0.4 + 0.6 * c.x, 0.4 + 0.6 * c.y, 0.4 + 0.6 * c.z)
}

/// A line between two points in the tree area, 4 px wide.
fn line(c: &mut ChildSpawnerCommands, from: Vec2, to: Vec2, color: Color) {
    let d = to - from;
    let len = d.length();
    let mid = (from + to) * 0.5;
    c.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(mid.x - len * 0.5), top: Val::Px(mid.y - 2.0), width: Val::Px(len), height: Val::Px(4.0), ..default() },
        UiTransform { rotation: Rot2::radians(d.y.atan2(d.x)), ..default() },
        BackgroundColor(color),
    ));
}

#[allow(clippy::too_many_arguments)]
fn step_node(
    c: &mut ChildSpawnerCommands,
    theme: &Theme,
    items: &Items,
    art: &ItemArt,
    book: &UrnBook,
    benches: &Benches,
    def: &UrnDef,
    p: &Progress,
    s: &Step,
    at: Vec2,
    selected: bool,
) {
    let state = p.state(s);
    let title_color = match state {
        State::Done => colors::GOLD,
        State::Open => colors::TEXT,
        State::Locked => colors::FAINT,
    };
    c.spawn((
        TreeNode(s.name.clone()),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x - NODE_W * 0.5),
            top: Val::Px(at.y - NODE_H * 0.5),
            width: Val::Px(NODE_W),
            height: Val::Px(NODE_H),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(2.0),
            ..default()
        },
    ))
    .with_children(|n| {
        n.spawn(Node { width: Val::Px(FRAME), height: Val::Px(FRAME), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() })
            .with_children(|f| {
                // Steps you can fill now breathe a halo in the urn's light.
                if state == State::Open {
                    f.spawn((
                        Breathe,
                        theme.image("node_halo").with_color(glow_of(def)),
                        Node { position_type: PositionType::Absolute, left: Val::Px(-14.0), top: Val::Px(-14.0), width: Val::Px(FRAME + 28.0), height: Val::Px(FRAME + 28.0), ..default() },
                    ));
                }
                let frame = if selected { "slot_selected" } else { "slot" };
                let tint = match state {
                    State::Done => colors::GOLD,
                    State::Open => Color::WHITE,
                    State::Locked => Color::srgba(0.55, 0.55, 0.6, 0.8),
                };
                f.spawn((
                    NodeFrame(s.name.clone()),
                    theme.image(frame).with_color(tint),
                    Node { width: Val::Px(FRAME), height: Val::Px(FRAME), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                ))
                .with_children(|f| draw_picture(f, theme, items, art, &picture(s, book, benches, items), 40.0, state == State::Locked));
                // Done or locked marks.
                let mark = match state {
                    State::Done => Some("icon_check"),
                    State::Locked => Some("icon_lock"),
                    State::Open => None,
                };
                if let Some(m) = mark {
                    f.spawn((theme.image(m), Node { position_type: PositionType::Absolute, right: Val::Px(-8.0), bottom: Val::Px(-6.0), width: Val::Px(20.0), height: Val::Px(20.0), ..default() }));
                }
            });
        n.spawn((theme.bold(sizes::SMALL), Text::new(s.title.clone()), TextColor(title_color), TextLayout::justify(Justify::Center)));
    });
}

/// The right side: what the shown step gives, what it takes, and putting it in.
#[allow(clippy::too_many_arguments)]
fn details(
    c: &mut ChildSpawnerCommands,
    theme: &Theme,
    items: &Items,
    art: &ItemArt,
    benches: &Benches,
    book: &UrnBook,
    inv: &Inventory,
    def: &UrnDef,
    p: &Progress,
    name: &str,
) {
    let Some(s) = def.step(name) else { return };
    let state = p.state(s);
    let tier = (s.tier as usize).clamp(1, 3);
    c.spawn((theme.bold(sizes::HEADING), Text::new(s.title.clone()), TextColor(colors::HEADING)));
    let later = if s.later { "  ·  not in the game yet" } else { "" };
    c.spawn(text(format!("Tier {}  ·  {}  ·  {}{later}", NUMERALS[tier - 1], TIERS[tier - 1], s.gives.label()), sizes::SMALL, colors::MUTED));
    // What you get.
    c.spawn(text("YOU GET", sizes::TINY, colors::GOLD));
    c.spawn((Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(10.0)), row_gap: Val::Px(6.0), ..default() }, theme.skin("card")))
        .with_children(|g| {
            let (head, line) = match &s.gives {
                Gives::Station(k) => {
                    let t = StationKind::from_name(k).map_or("a station", |k| k.title());
                    (format!("New station: {t}"), format!("The {t} appears beside the urn."))
                }
                Gives::Upgrade => ("Upgrade".to_string(), String::new()),
                Gives::Recipes => {
                    let at: Vec<&str> = {
                        let mut v: Vec<&str> = benches.0.recipes.iter().filter(|r| r.taught_by(&s.name)).filter_map(|r| StationKind::from_name(&r.station).map(|k| k.title())).collect();
                        v.dedup();
                        v
                    };
                    let line = if at.is_empty() { String::new() } else { format!("Made at the {}.", at.join(" and the ")) };
                    ("New recipes".to_string(), line)
                }
                Gives::Urn(u) => {
                    let other = book.0.urn(u);
                    let t = other.map_or("an urn", |o| o.title.as_str());
                    let st = other.map_or("", |o| o.station.as_str());
                    (format!("The {t}"), format!("Carry it and set it down in a room to start the {st}."))
                }
            };
            g.spawn(Node { column_gap: Val::Px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                r.spawn(Node { width: Val::Px(44.0), height: Val::Px(44.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, flex_shrink: 0.0, ..default() })
                    .with_children(|f| draw_picture(f, theme, items, art, &picture(s, book, benches, items), 40.0, false));
                r.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|r| {
                    r.spawn((theme.bold(sizes::BODY), Text::new(head), TextColor(colors::TEXT)));
                    if !line.is_empty() {
                        r.spawn(text(line, sizes::SMALL, colors::MUTED));
                    }
                });
            });
            if matches!(s.gives, Gives::Recipes) {
                // Every recipe it teaches, as pictures.
                g.spawn(Node { column_gap: Val::Px(8.0), row_gap: Val::Px(4.0), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|r| {
                    let mut seen = Vec::new();
                    for rec in benches.0.recipes.iter().filter(|r| r.taught_by(&s.name)) {
                        if seen.contains(&rec.makes.item) {
                            continue;
                        }
                        seen.push(rec.makes.item);
                        r.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                            r.spawn(Node { width: Val::Px(30.0), height: Val::Px(30.0), ..default() })
                                .with_children(|i| crate::inventory::ui::icon(i, items, art, Stack::new(rec.makes.item, 1), 30.0, 30.0, false));
                            r.spawn(text(items.get(rec.makes.item).title.clone(), sizes::SMALL, colors::TEXT));
                        });
                    }
                });
            }
            if !s.desc.is_empty() {
                g.spawn(text(s.desc.clone(), sizes::SMALL, colors::MUTED));
            }
        });
    // What it takes.
    if state != State::Done {
        c.spawn(text("IT TAKES", sizes::TINY, colors::GOLD));
        c.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() }).with_children(|r| {
            for (item, need) in &s.needs {
                let Some(id) = items.id(item) else { continue };
                let have = p.given(&s.name, item).min(*need);
                let carry = inv.count(id);
                let full = have >= *need;
                r.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }).with_children(|r| {
                    let px = super::fly_in::ICON;
                    r.spawn((super::fly_in::NeedIcon(item.clone()), Node { width: Val::Px(px), height: Val::Px(px), flex_shrink: 0.0, ..default() }, UiTransform::default()))
                        .with_children(|i| crate::inventory::ui::icon(i, items, art, Stack::new(id, 1), px, px, false));
                    r.spawn((text(items.get(id).title.clone(), sizes::SMALL, colors::TEXT), Node { flex_grow: 1.0, ..default() }));
                    r.spawn(text(format!("{have}/{need}"), sizes::SMALL, if full { colors::GOOD } else { colors::TEXT }));
                    let carry_text = if full { String::new() } else { format!("you carry {carry}") };
                    r.spawn((text(carry_text, sizes::SMALL, if carry > 0 { colors::GOOD } else { colors::FAINT }), Node { width: Val::Px(110.0), ..default() }));
                });
            }
        });
    }
    // Put in, or where it stands.
    c.spawn(Node { margin: UiRect::top(Val::Px(6.0)), column_gap: Val::Px(10.0), align_items: AlignItems::Center, ..default() }).with_children(|r| match state {
        State::Open => {
            r.spawn((
                PutIn(s.name.clone()),
                Node { padding: UiRect::axes(Val::Px(18.0), Val::Px(4.0)), justify_content: JustifyContent::Center, ..default() },
                theme.skin("button"),
                children![text("Put in", sizes::BODY, colors::TEXT)],
            ));
        }
        State::Done => {
            r.spawn(text("Done", sizes::BODY, colors::GOOD));
        }
        State::Locked => {
            let wait: Vec<String> = s.after.iter().filter(|a| !p.is_done(a)).filter_map(|a| def.step(a)).map(|x| x.title.clone()).collect();
            r.spawn(text(format!("After {}", wait.join(" and ")), sizes::BODY, colors::FAINT));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::station::urns::UrnData;

    fn book() -> UrnData {
        UrnData::from_ron(include_str!("../../../../assets/data/urns.ron"), |_| true).unwrap()
    }

    #[test]
    fn steps_stand_above_what_they_come_after() {
        for u in &book().urns {
            let spots = tree_layout(u);
            for (k, s) in u.steps.iter().enumerate() {
                assert!(spots[k].1 < spots[k].2);
                for a in &s.after {
                    let j = u.steps.iter().position(|x| &x.name == a).unwrap();
                    assert!(spots[k].0 > spots[j].0, "{} under {}", s.name, a);
                }
            }
        }
    }

    #[test]
    fn no_two_steps_share_a_spot() {
        for u in &book().urns {
            let spots = tree_layout(u);
            let mut seen: Vec<(usize, usize)> = spots.iter().map(|s| (s.0, s.1)).collect();
            seen.sort();
            seen.dedup();
            assert_eq!(seen.len(), u.steps.len(), "{}", u.name);
        }
    }

    #[test]
    fn every_tree_fits() {
        for u in &book().urns {
            let spots = tree_layout(u);
            let levels = spots.iter().map(|s| s.0 + 1).max().unwrap_or(0);
            let widest = spots.iter().map(|s| s.2).max().unwrap_or(0);
            assert!(levels as f32 * ROW <= 560.0, "{} is {levels} levels tall", u.name);
            assert!(widest as f32 * NODE_W <= TREE_W, "{} is {widest} wide", u.name);
        }
    }
}
