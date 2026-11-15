//! On-screen game UI: the hotbar (shortcuts to what you carry, with counts
//! and pictures), the name of what you hold, and a red flash when you get
//! hurt.

use bevy::prelude::*;

use crate::interact::{Hands, Hotbar, Wielded, SLOTS};
use crate::inventory::grid::Stack;
use crate::inventory::ui::icon;
use crate::item_art::ItemArt;
use crate::inventory::Inventory;
use crate::items::Items;
use crate::player::Damage;
use crate::theme::{self, colors, sizes, Theme};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        // The top-right column: panels there (the weather, ...) stack in it
        // instead of each placing itself, so they never overlap.
        let top_right = app
            .world_mut()
            .spawn((
                Name::new("top right"),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(14.0),
                    top: Val::Px(14.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::End,
                    row_gap: Val::Px(8.0),
                    ..default()
                },
            ))
            .id();
        app.insert_resource(TopRight(top_right));
        app.add_systems(crate::menu::GamePostStartup, spawn_hud).add_systems(crate::menu::GameUpdate, (update_hotbar, hurt_flash));
    }
}

/// Hotbar slot size in points.
/// The column at the top right of the screen: spawn a panel with
/// `ChildOf(top_right.0)` (and no position of its own) to stack it there.
#[derive(Resource, Clone, Copy)]
pub struct TopRight(pub Entity);

pub const SLOT: f32 = 58.0;

#[derive(Component)]
struct Slot(usize);

/// Holds a slot's picture and count; rebuilt when the slot changes.
#[derive(Component)]
struct SlotContent(usize);

#[derive(Component)]
struct HandsLine;

#[derive(Component)]
struct HurtFlash;

fn spawn_hud(mut commands: Commands, hotbar: Option<Res<Hotbar>>, theme: Res<Theme>) {
    if hotbar.is_none() {
        return;
    }
    commands.spawn((
        HurtFlash,
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        BackgroundColor(Color::srgba(0.7, 0.0, 0.0, 0.0)),
        Pickable::IGNORE,
    ));
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|root| {
            root.spawn((HandsLine, theme::text("", sizes::LABEL, colors::TEXT), Node { margin: UiRect::bottom(Val::Px(20.0)), ..default() }));
            root.spawn((Node { column_gap: Val::Px(2.0), padding: UiRect::all(Val::Px(6.0)), ..default() }, theme.skin("panel_light"))).with_children(|row| {
                for i in 0..SLOTS {
                    row.spawn((Slot(i), Node { width: Val::Px(SLOT), height: Val::Px(SLOT), ..default() }, theme.skin("slot"))).with_children(|s| {
                        // The picture, with the slot number over its top-left corner.
                        s.spawn((SlotContent(i), Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }));
                        s.spawn((
                            theme::text(format!("{}", i + 1), sizes::SMALL, colors::MUTED),
                            Node { position_type: PositionType::Absolute, left: Val::Px(5.0), top: Val::Px(2.0), ..default() },
                            GlobalZIndex(1),
                        ));
                    });
                }
            });
        });
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_hotbar(
    mut commands: Commands,
    theme: Res<Theme>,
    (hands, hotbar, items, inv, wielded): (Res<Hands>, Res<Hotbar>, Res<Items>, Res<Inventory>, Res<Wielded>),
    art: Option<Res<ItemArt>>,
    mut slots: Query<(&Slot, &mut ImageNode)>,
    contents: Query<(Entity, &SlotContent)>,
    mut line: Query<&mut Text, With<HandsLine>>,
    mut ready: Local<bool>,
) {
    let Some(art) = art else { return };
    if *ready && !(hands.is_changed() || hotbar.is_changed() || inv.is_changed() || wielded.is_changed()) {
        return;
    }
    *ready = true;
    for (e, c) in &contents {
        commands.entity(e).despawn_related::<Children>();
        let Some(id) = hotbar.slots.get(c.0).copied().flatten() else { continue };
        let n = inv.count(id);
        commands.entity(e).with_children(|p| {
            // Dim the picture when you have none left.
            p.spawn(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }).with_children(|q| {
                icon(q, &items, &art, Stack::new(id, n.clamp(1, u16::MAX as u32) as u16), SLOT, SLOT, false);
            });
            if n == 0 {
                p.spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6))));
            }
        });
    }
    for (s, mut img) in &mut slots {
        *img = theme.image(if hands.right == Some(s.0) { "slot_selected" } else { "slot" });
    }
    if let Ok(mut l) = line.single_mut() {
        l.0 = match wielded.0 {
            Some(i) => items.get(i).title.clone(),
            None => "Empty hand   (Tab: inventory)".into(),
        };
    }
}

fn hurt_flash(time: Res<Time>, mut hits: MessageReader<Damage>, mut flash: Query<&mut BackgroundColor, With<HurtFlash>>) {
    let Ok(mut bg) = flash.single_mut() else { return };
    let mut a = bg.0.alpha();
    for d in hits.read() {
        a = (a + 0.25 + d.amount * 0.03).min(0.6);
    }
    a = (a - time.delta_secs() * 0.8).max(0.0);
    bg.0.set_alpha(a);
}
