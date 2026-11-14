//! The dev menu (F9): hand yourself urns and skip through their trees, to try
//! every station without gathering for it. One row per urn:
//!
//! - Give urn: the urn's item, to set down yourself
//! - Set down: puts it on the ground in front of you
//! - Items: what its next step still needs, into your pockets
//! - Next step / All steps: finish steps (they give what they give)
//! - Reset: empty the urn and forget its steps (a carried urn goes back to nowhere)
//!
//! `BLOCKGAME_DEVMENU=1` opens it for screenshots.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::urns::State;
use super::{placed_facing, StepDone, UrnBook, Urns};
use crate::camera::{FlyCamera, Menus};
use crate::inventory::grid::Stack;
use crate::inventory::{Inventory, Spill};
use crate::items::Items;
use crate::survival::Toast;
use crate::theme::{colors, sizes, text, Theme};

#[derive(Resource, Default)]
pub struct DevMenu {
    pub open: bool,
    dirty: bool,
}

#[derive(Component)]
pub(super) struct DevRoot;

#[derive(Component)]
pub(super) struct DevPanel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Give,
    SetDown,
    Items,
    Next,
    All,
    Reset,
}

const ACTS: [(Act, &str); 6] = [(Act::Give, "Give urn"), (Act::SetDown, "Set down"), (Act::Items, "Items"), (Act::Next, "Next step"), (Act::All, "All steps"), (Act::Reset, "Reset")];

#[derive(Component)]
pub(super) struct DevButton(usize, Act);

pub(super) fn spawn_dev_ui(mut commands: Commands) {
    commands
        .spawn((
            DevRoot,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.45)),
            GlobalZIndex(26),
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn((DevPanel, Node { flex_direction: FlexDirection::Column, ..default() }));
        });
}

/// F9 opens and closes the dev menu; Esc closes it.
pub(super) fn dev_keys(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut dev: ResMut<DevMenu>,
    mut menus: ResMut<Menus>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    (urns, inv): (Res<Urns>, Res<Inventory>),
    mut forced: Local<bool>,
) {
    if !*forced {
        *forced = true;
        if std::env::var_os("BLOCKGAME_DEVMENU").is_some() {
            dev.open = true;
            dev.dirty = true;
            menus.open = true;
        }
    }
    let toggle = keys.just_pressed(KeyCode::F9);
    if dev.open && (toggle || keys.just_pressed(KeyCode::Escape)) {
        keys.clear_just_pressed(KeyCode::Escape);
        dev.open = false;
        dev.dirty = true;
        menus.open = false;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        return;
    }
    if toggle && !menus.open {
        dev.open = true;
        dev.dirty = true;
        menus.open = true;
    }
    if dev.open {
        if !cursor.visible {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
        if urns.is_changed() || inv.is_changed() {
            dev.dirty = true;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dev_click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    theme: Res<Theme>,
    mut buttons: Query<(&DevButton, &ComputedNode, &UiGlobalTransform, &InheritedVisibility, &mut ImageNode)>,
    (book, items, dev): (Option<Res<UrnBook>>, Res<Items>, Res<DevMenu>),
    (player, cam): (Res<crate::player::Player>, Single<&Transform, With<FlyCamera>>),
    (mut inv, mut urns): (ResMut<Inventory>, ResMut<Urns>),
    (mut done, mut spill, mut toast): (MessageWriter<StepDone>, MessageWriter<Spill>, MessageWriter<Toast>),
) {
    let Some(book) = book else { return };
    if !dev.open {
        return;
    }
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
            clicked = Some((b.0, b.1));
        }
    }
    let Some((i, act)) = clicked else { return };
    let def = &book.0.urns[i];
    let mut give = |inv: &mut Inventory, name: &str, n: u32| {
        let Some(id) = items.id(name) else { return };
        let mut left = n;
        while left > 0 {
            let k = left.min(items.get(id).stack as u32).max(1);
            if let Some(rest) = inv.add(&items, Stack::new(id, k as u16)) {
                spill.write(Spill { stack: rest, at: player.pos + Vec3::Y });
            }
            left -= k;
        }
    };
    match act {
        Act::Give => {
            if let Some(it) = &def.item {
                give(&mut inv, it, 1);
                toast.write(Toast(format!("Dev: gave you the {}", def.title)));
            }
        }
        Act::SetDown => {
            if def.item.is_some() {
                let mut f = cam.forward().as_vec3();
                f.y = 0.0;
                let f = f.normalize_or(Vec3::NEG_Z);
                urns.placed.insert(def.name.clone(), placed_facing(player.pos + f * 3.0, f));
                toast.write(Toast(format!("Dev: set the {} down in front of you", def.title)));
            }
        }
        Act::Items => {
            let p = urns.of(&def.name).clone();
            match p.next(def) {
                Some(s) => {
                    for (name, n) in p.missing(s) {
                        give(&mut inv, &name, n);
                    }
                    toast.write(Toast(format!("Dev: gave you what the {} needs", s.title)));
                }
                None => {
                    toast.write(Toast(format!("Dev: the {} has no open step", def.title)));
                }
            }
        }
        Act::Next | Act::All => loop {
            let p = urns.of_mut(&def.name);
            let Some(s) = def.steps.iter().find(|s| p.state(s) == State::Open) else { break };
            p.finish(&s.name);
            done.write(StepDone(s.name.clone()));
            if act == Act::Next {
                break;
            }
        },
        Act::Reset => {
            urns.progress.remove(&def.name);
            urns.placed.remove(&def.name);
            toast.write(Toast(format!("Dev: reset the {}", def.title)));
        }
    }
}

pub(super) fn dev_build(
    mut commands: Commands,
    theme: Res<Theme>,
    mut dev: ResMut<DevMenu>,
    (book, urns): (Option<Res<UrnBook>>, Res<Urns>),
    mut root: Query<&mut Visibility, With<DevRoot>>,
    panel: Query<(Entity, Option<&Children>), With<DevPanel>>,
) {
    if !dev.dirty {
        return;
    }
    let Some(book) = book else { return };
    dev.dirty = false;
    let (Ok(mut vis), Ok((panel, kids))) = (root.single_mut(), panel.single()) else { return };
    for k in kids.into_iter().flatten() {
        commands.entity(*k).despawn();
    }
    if !dev.open {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Inherited;
    let theme = &*theme;
    commands.entity(panel).insert((Node { flex_direction: FlexDirection::Column, padding: UiRect::axes(Val::Px(20.0), Val::Px(14.0)), row_gap: Val::Px(8.0), ..default() }, theme.skin("panel")));
    commands.entity(panel).with_children(|c| {
        c.spawn((theme.bold(sizes::TITLE), Text::new("Dev: Urns"), TextColor(colors::HEADING)));
        c.spawn(text("Hand yourself urns and skip steps to test the stations. F9 or Esc closes.", sizes::SMALL, colors::MUTED));
        for (i, def) in book.0.urns.iter().enumerate() {
            let p = urns.of(&def.name);
            let n = def.steps.iter().filter(|s| p.is_done(&s.name)).count();
            let where_ = if def.item.is_none() {
                "by the bedroll"
            } else if urns.placed.contains_key(&def.name) {
                "set down"
            } else {
                "not set down"
            };
            c.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }).with_children(|r| {
                r.spawn(Node { flex_direction: FlexDirection::Column, width: Val::Px(250.0), ..default() }).with_children(|t| {
                    t.spawn((theme.bold(sizes::BODY), Text::new(format!("{} · {}", def.title, def.station)), TextColor(colors::TEXT)));
                    let next = p.next(def).map_or("all done".to_string(), |s| format!("next: {}", s.title));
                    t.spawn(text(format!("{n}/{} steps, {where_}, {next}", def.steps.len()), sizes::TINY, colors::MUTED));
                });
                for (act, label) in ACTS {
                    let off = def.item.is_none() && matches!(act, Act::Give | Act::SetDown);
                    if off {
                        r.spawn((Node { width: Val::Px(112.0), height: Val::Px(36.0), ..default() },));
                        continue;
                    }
                    r.spawn((
                        DevButton(i, act),
                        Node { width: Val::Px(112.0), height: Val::Px(36.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                        theme.skin("button"),
                        children![text(label, sizes::SMALL, colors::TEXT)],
                    ));
                }
            });
        }
    });
}
