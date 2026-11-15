//! An urn's tree: right click an urn with an empty hand. Its steps stand in
//! three columns by tier; each shows what it gives, what it still needs (and
//! how much of that you carry), and whether it's done, open or waiting on
//! another step. "Put in" moves what you carry for that step into the urn.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::urns::{State, Step};
use super::{StepDone, UrnBook, Urns};
use crate::camera::Menus;
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
}

#[derive(Component)]
pub(super) struct TreeRoot;

#[derive(Component)]
pub(super) struct TreePanel;

/// "Put in" on a step.
#[derive(Component)]
pub(super) struct PutIn(String);

const TIERS: [&str; 3] = ["Near home", "A trip out", "Far finds"];

pub(super) fn spawn_tree_ui(mut commands: Commands) {
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
            BackgroundColor(Color::srgba(0.0, 0.01, 0.0, 0.6)),
            GlobalZIndex(25),
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn((TreePanel, Node { flex_direction: FlexDirection::Column, ..default() }));
        });
}

/// Esc or Tab closes the tree. `BLOCKGAME_URNTREE=tool` opens one for screenshots.
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

/// Hover and click "Put in".
#[allow(clippy::too_many_arguments)]
pub(super) fn tree_click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    theme: Res<Theme>,
    mut buttons: Query<(&PutIn, &ComputedNode, &UiGlobalTransform, &InheritedVisibility, &mut ImageNode)>,
    (book, items): (Option<Res<UrnBook>>, Res<Items>),
    tree: Res<TreeUi>,
    mut inv: ResMut<Inventory>,
    mut urns: ResMut<Urns>,
    mut done: MessageWriter<StepDone>,
    mut toast: MessageWriter<Toast>,
) {
    let (Some(i), Some(book)) = (tree.open, book) else { return };
    let def = &book.0.urns[i];
    let p = window.physical_cursor_position();
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
            clicked = Some(b.0.clone());
        }
    }
    let Some(step) = clicked else { return };
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
        for f in finished {
            done.write(StepDone(f));
        }
    }
    if put == 0 {
        toast.write(Toast(format!("You carry nothing the {} needs yet", s.title)));
    }
}

/// Build the open tree.
#[allow(clippy::too_many_arguments)]
pub(super) fn tree_build(
    mut commands: Commands,
    theme: Res<Theme>,
    mut tree: ResMut<TreeUi>,
    (book, items, art): (Option<Res<UrnBook>>, Res<Items>, Option<Res<ItemArt>>),
    (urns, inv): (Res<Urns>, Res<Inventory>),
    mut root: Query<&mut Visibility, With<TreeRoot>>,
    panel: Query<(Entity, Option<&Children>), With<TreePanel>>,
) {
    if !tree.dirty {
        return;
    }
    let (Some(book), Some(art)) = (book, art) else { return };
    tree.dirty = false;
    let Ok(mut vis) = root.single_mut() else { return };
    let Ok((panel, kids)) = panel.single() else { return };
    for k in kids.into_iter().flatten() {
        commands.entity(*k).despawn();
    }
    let Some(i) = tree.open else {
        *vis = Visibility::Hidden;
        return;
    };
    *vis = Visibility::Inherited;
    let def = &book.0.urns[i];
    let p = urns.of(&def.name);
    let theme = &*theme;
    commands.entity(panel).insert((
        Node {
            flex_direction: FlexDirection::Column,
            width: Val::Px(1180.0),
            max_width: Val::Percent(97.0),
            max_height: Val::Percent(97.0),
            padding: UiRect::axes(Val::Px(20.0), Val::Px(14.0)),
            row_gap: Val::Px(8.0),
            ..default()
        },
        theme.skin("panel"),
    ));
    commands.entity(panel).with_children(|c| {
        // Title and progress.
        c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::FlexEnd, ..default() }).with_children(|r| {
            r.spawn(Node { column_gap: Val::Px(12.0), align_items: AlignItems::FlexEnd, ..default() }).with_children(|r| {
                r.spawn((theme.bold(sizes::TITLE), Text::new(def.title.clone()), TextColor(colors::HEADING)));
                r.spawn((text(def.station.clone(), sizes::LABEL, colors::MUTED), Node { margin: UiRect::bottom(Val::Px(4.0)), ..default() }));
            });
            let n = def.steps.iter().filter(|s| p.is_done(&s.name)).count();
            r.spawn(text(format!("{n} / {} steps", def.steps.len()), sizes::LABEL, if n == def.steps.len() { colors::GOOD } else { colors::TEXT }));
        });
        c.spawn(text(def.how.clone(), sizes::SMALL, colors::MUTED));
        // The tiers, side by side.
        c.spawn(Node { column_gap: Val::Px(12.0), align_items: AlignItems::FlexStart, ..default() }).with_children(|row| {
            for (t, name) in TIERS.iter().enumerate() {
                let steps: Vec<&Step> = def.steps.iter().filter(|s| s.tier as usize == t + 1).collect();
                if steps.is_empty() {
                    continue;
                }
                row.spawn(Node { flex_direction: FlexDirection::Column, flex_basis: Val::Px(0.0), flex_grow: 1.0, row_gap: Val::Px(6.0), ..default() }).with_children(|col| {
                    col.spawn(text(format!("{} · Tier {}", name, t + 1), sizes::BODY, colors::GOLD));
                    for s in steps {
                        card(col, theme, &items, &art, &inv, def, p, s);
                    }
                });
            }
        });
        c.spawn(text(
            "Hold an item and right click the urn to put it in (Shift: all of it)  ·  Put in: everything you carry for that step  ·  Esc: close",
            sizes::SMALL,
            colors::FAINT,
        ));
    });
}

#[allow(clippy::too_many_arguments)]
fn card(c: &mut ChildSpawnerCommands, theme: &Theme, items: &Items, art: &ItemArt, inv: &Inventory, def: &super::urns::UrnDef, p: &super::urns::Progress, s: &Step) {
    let state = p.state(s);
    let (title_color, note) = match state {
        State::Done => (colors::GOOD, "Done".to_string()),
        State::Open => (colors::HEADING, String::new()),
        State::Locked => {
            let wait: Vec<String> = s.after.iter().filter(|a| !p.is_done(a)).filter_map(|a| def.step(a)).map(|x| x.title.clone()).collect();
            (colors::FAINT, format!("After {}", wait.join(" and ")))
        }
    };
    c.spawn((Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)), row_gap: Val::Px(2.0), ..default() }, theme.skin("panel_light")))
        .with_children(|c| {
            c.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                r.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                    r.spawn((theme.bold(sizes::BODY), Text::new(s.title.clone()), TextColor(title_color)));
                    let tag = if s.later { format!("{} · coming", s.gives.label()) } else { s.gives.label().to_string() };
                    r.spawn(text(tag.to_uppercase(), sizes::TINY, colors::MUTED));
                });
                if state == State::Open {
                    r.spawn((
                        PutIn(s.name.clone()),
                        Node { padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)), justify_content: JustifyContent::Center, ..default() },
                        theme.skin("button"),
                        children![text("Put in", sizes::SMALL, colors::TEXT)],
                    ));
                } else if !note.is_empty() {
                    r.spawn(text(note.clone(), sizes::SMALL, title_color));
                }
            });
            if state != State::Done {
                c.spawn((text(s.desc.clone(), sizes::SMALL, colors::MUTED), Node::default()));
                // What it needs: picture, put in / needed, and what you carry.
                c.spawn(Node { column_gap: Val::Px(10.0), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|r| {
                    for (name, need) in &s.needs {
                        let Some(id) = items.id(name) else { continue };
                        let have = p.given(&s.name, name).min(*need);
                        let carry = inv.count(id);
                        r.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(2.0), ..default() }).with_children(|r| {
                            r.spawn(Node { width: Val::Px(30.0), height: Val::Px(30.0), ..default() }).with_children(|i| {
                                crate::inventory::ui::icon(i, items, art, Stack::new(id, 1), 30.0, 30.0, false);
                            });
                            let full = have >= *need;
                            r.spawn(text(format!("{have}/{need}"), sizes::SMALL, if full { colors::GOOD } else { colors::TEXT }));
                            if !full && carry > 0 && state == State::Open {
                                r.spawn(text(format!("+{}", carry.min(need - have)), sizes::SMALL, colors::GOOD));
                            }
                        });
                    }
                });
            }
        });
}
