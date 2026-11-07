//! On-screen game UI: the hotbar with what you hold, and a red flash
//! when you get hurt.

use bevy::prelude::*;

use crate::interact::{Hands, Hotbar, PAGE};
use crate::items::Items;
use crate::player::Damage;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_hud).add_systems(Update, (update_hotbar, hurt_flash));
    }
}

#[derive(Component)]
struct Slot(usize);

#[derive(Component)]
struct SlotTag(usize);

#[derive(Component)]
struct SlotTitle(usize);

#[derive(Component)]
struct HandsLine;

#[derive(Component)]
struct HurtFlash;

const RIGHT: Color = Color::srgb(1.0, 0.82, 0.3);

fn spawn_hud(mut commands: Commands, hotbar: Option<Res<Hotbar>>) {
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
            bottom: Val::Px(10.0),
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|root| {
            root.spawn((
                HandsLine,
                Text::new(""),
                TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() },
                TextColor(Color::WHITE),
                TextShadow::default(),
            ));
            root.spawn(Node { column_gap: Val::Px(4.0), ..default() }).with_children(|row| {
                for i in 0..PAGE {
                    row.spawn((
                        Slot(i),
                        Node {
                            width: Val::Px(70.0),
                            height: Val::Px(46.0),
                            border: UiRect::all(Val::Px(2.0)),
                            padding: UiRect::all(Val::Px(3.0)),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.15)),
                    ))
                    .with_children(|s| {
                        s.spawn((
                            SlotTag(i),
                            Text::new(format!("{}", i + 1)),
                            TextFont { font_size: bevy::text::FontSize::Px(11.0), ..default() },
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                        ));
                        s.spawn((
                            SlotTitle(i),
                            Text::new(""),
                            TextFont { font_size: bevy::text::FontSize::Px(11.0), ..default() },
                            TextColor(Color::WHITE),
                        ));
                    });
                }
            });
        });
}

#[allow(clippy::type_complexity)]
fn update_hotbar(
    hands: Res<Hands>,
    hotbar: Res<Hotbar>,
    items: Res<Items>,
    mut slots: Query<(&Slot, &mut BorderColor, &mut Visibility)>,
    mut tags: Query<(&SlotTag, &mut Text, &mut TextColor), (Without<HandsLine>, Without<SlotTitle>)>,
    mut titles: Query<(&SlotTitle, &mut Text), (Without<HandsLine>, Without<SlotTag>)>,
    mut line: Query<&mut Text, (With<HandsLine>, Without<SlotTag>, Without<SlotTitle>)>,
) {
    if !hands.is_changed() {
        return;
    }
    let page = hands.right.unwrap_or(0) / PAGE * PAGE;
    for (t, mut text) in &mut titles {
        text.0 = hotbar.slots.get(page + t.0).map(|i| items.get(*i).title.clone()).unwrap_or_default();
    }
    for (s, mut border, mut vis) in &mut slots {
        *vis = if page + s.0 < hotbar.slots.len() { Visibility::Inherited } else { Visibility::Hidden };
        let c = if hands.right == Some(page + s.0) {
            RIGHT
        } else {
            Color::srgba(1.0, 1.0, 1.0, 0.15)
        };
        *border = BorderColor::all(c);
    }
    for (t, mut text, mut color) in &mut tags {
        let (label, c) = if hands.right == Some(page + t.0) {
            (format!("{}", t.0 + 1), RIGHT)
        } else {
            (format!("{}", t.0 + 1), Color::srgba(1.0, 1.0, 1.0, 0.7))
        };
        text.0 = label;
        color.0 = c;
    }
    let name = |h: Option<crate::items::ItemId>| h.map(|i| items.get(i).title.clone()).unwrap_or_else(|| "empty hand".into());
    if let Ok(mut l) = line.single_mut() {
        let pages = hotbar.slots.len().div_ceil(PAGE);
        l.0 = format!("{}   (page {}/{pages}, wheel for more)", name(hands.item(crate::interact::Hand::Right, &hotbar)), page / PAGE + 1);
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
