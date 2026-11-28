//! The Equipment tab: the live picture of you in the middle (drawn by
//! `inventory::preview`), armor slots on its left, clothes on its right, and
//! the weapon and both hands under it, above a strip of the gear you carry.

use bevy::prelude::*;

use super::{CONTENT_H, Data, Hover, SlotView, Zone, text};
use crate::inventory::grid::Stack;
use crate::items::{ItemKind, WearSlot};
use crate::survival::health::Layer;
use crate::theme::{colors, sizes};

/// The live picture's size on screen.
pub(crate) const PREVIEW: Vec2 = Vec2::new(470.0, 292.0);
/// A gear slot's size.
const SLOT: f32 = 64.0;
const SIDE: f32 = 340.0;

pub(super) fn build(c: &mut ChildSpawnerCommands, view: &SlotView, d: &Data) {
    let theme = view.theme;
    let inv = &*d.inv;
    c.spawn(Node {
        column_gap: Val::Px(16.0),
        width: Val::Percent(100.0),
        height: Val::Px(CONTENT_H),
        ..default()
    })
    .with_children(|row| {
        // Armor, on the left, slots towards the picture.
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Px(SIDE),
            row_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|col| {
            heading(col, theme, "Armor", "icon_injuries", false);
            for slot in [WearSlot::HeadArmor, WearSlot::BodyArmor] {
                gear_row(col, view, d, slot, true);
            }
            // What it all adds up to.
            col.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    margin: UiRect::top(Val::Px(8.0)),
                    ..default()
                },
                theme.skin("card"),
            ))
            .with_children(|st| {
                st.spawn((
                    theme.bold(sizes::BODY),
                    Text::new("Wearing"),
                    TextColor(colors::HEADING),
                ));
                super::icon_line(
                    st,
                    theme,
                    "icon_warmth",
                    format!(
                        "Clothes +{}, feels {}",
                        crate::settings::temp_diff(inv.insulation(&d.items), 0, false),
                        crate::settings::temp(d.s.env.feels, 0)
                    ),
                    colors::TEXT,
                );
                let any = Layer::ALL
                    .iter()
                    .any(|l| inv.protection(&d.items, *l) > 0.0);
                if !any {
                    super::icon_line(st, theme, "icon_injuries", "No armor".into(), colors::MUTED);
                }
                for l in Layer::ALL {
                    let p = inv.protection(&d.items, l);
                    if p > 0.0 {
                        super::icon_line(
                            st,
                            theme,
                            super::layer_icon(l),
                            format!("{} -{:.0}%", l.name(), p * 100.0),
                            colors::GOOD,
                        );
                    }
                }
            });
        });

        // The picture, then the weapon and the hands, then the gear you carry.
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            flex_grow: 1.0,
            row_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|mid| {
            mid.spawn((
                Zone::At(Hover::Preview),
                Node {
                    width: Val::Px(PREVIEW.x),
                    height: Val::Px(PREVIEW.y),
                    ..default()
                },
                theme.skin("inset"),
            ))
            .with_children(|pic| {
                // The plinth and its shadow under the feet, then the picture over it.
                let plinth = theme.size("plinth") * 1.5;
                pic.spawn((
                    theme.image("plinth"),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px((PREVIEW.x - plinth.x) / 2.0),
                        top: Val::Px(PREVIEW.y - plinth.y - 8.0),
                        width: Val::Px(plinth.x),
                        height: Val::Px(plinth.y),
                        ..default()
                    },
                ));
                if let Some(img) = &d.preview {
                    pic.spawn((
                        ImageNode::new(img.0.clone()),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Px(PREVIEW.x),
                            height: Val::Px(PREVIEW.y),
                            ..default()
                        },
                    ));
                }
            });
            mid.spawn(Node {
                column_gap: Val::Px(26.0),
                ..default()
            })
            .with_children(|hands| {
                labelled(
                    hands,
                    view,
                    Hover::Weapon,
                    inv.weapon.map(|w| Stack::new(w, 1)),
                    "Weapon (R)",
                );
                labelled(
                    hands,
                    view,
                    Hover::RightHand,
                    d.wielded.0.map(|w| Stack::new(w, 1)),
                    "Right hand",
                );
                labelled(hands, view, Hover::LeftHand, inv.left, "Left hand");
            });
            // Gear in your pockets, ready to put on.
            let gear: Vec<(usize, Stack)> = inv
                .pockets
                .items
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    let it = d.items.get(p.stack.item);
                    it.wear_slot().is_some() || it.is_weapon()
                })
                .map(|(i, p)| (i, p.stack))
                .collect();
            mid.spawn((
                Node {
                    width: Val::Px(PREVIEW.x),
                    height: Val::Px(52.0),
                    column_gap: Val::Px(4.0),
                    padding: UiRect::all(Val::Px(4.0)),
                    align_items: AlignItems::Center,
                    overflow: Overflow::clip(),
                    ..default()
                },
                theme.skin("inset"),
            ))
            .with_children(|strip| {
                if gear.is_empty() {
                    strip.spawn((
                        text(
                            "No clothes, armor or weapons in your pockets",
                            sizes::SMALL,
                            colors::FAINT,
                        ),
                        Node {
                            margin: UiRect::horizontal(Val::Px(10.0)),
                            ..default()
                        },
                    ));
                }
                for (i, s) in gear {
                    let hovered = view.hover == Some(Hover::Gear(i));
                    strip
                        .spawn((
                            Zone::At(Hover::Gear(i)),
                            super::Lift(Hover::Gear(i)),
                            UiTransform::default(),
                            Node {
                                width: Val::Px(44.0),
                                height: Val::Px(44.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            theme.skin_tinted("item", super::item_bg(hovered)),
                            Outline {
                                width: Val::Px(if hovered { 2.0 } else { 0.0 }),
                                offset: Val::Px(0.0),
                                color: colors::GOLD,
                            },
                        ))
                        .with_children(|q| {
                            super::icon(q, &d.items, view.art, s, 44.0, 44.0, false)
                        });
                }
            });
        });

        // Clothes, on the right.
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Px(SIDE),
            row_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|col| {
            heading(col, theme, "Clothes", "icon_warmth", true);
            for slot in [
                WearSlot::Head,
                WearSlot::Body,
                WearSlot::Legs,
                WearSlot::Feet,
                WearSlot::Back,
            ] {
                gear_row(col, view, d, slot, false);
            }
        });
    });
}

fn heading(
    p: &mut ChildSpawnerCommands,
    theme: &crate::theme::Theme,
    title: &str,
    icon: &str,
    left: bool,
) {
    p.spawn(Node {
        column_gap: Val::Px(8.0),
        align_items: AlignItems::Center,
        justify_content: if left {
            JustifyContent::FlexStart
        } else {
            JustifyContent::FlexEnd
        },
        height: Val::Px(30.0),
        ..default()
    })
    .with_children(|h| {
        h.spawn(theme.icon(icon));
        h.spawn((
            theme.bold(sizes::HEADING),
            Text::new(title),
            TextColor(colors::HEADING),
        ));
    });
}

/// A slot with a small label under it.
fn labelled(
    p: &mut ChildSpawnerCommands,
    view: &SlotView,
    at: Hover,
    stack: Option<Stack>,
    label: &str,
) {
    p.spawn(Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: Val::Px(2.0),
        ..default()
    })
    .with_children(|c| {
        view.slot(c, at, stack, SLOT);
        c.spawn(text(label, sizes::SMALL, colors::MUTED));
    });
}

/// One worn slot: the slot, and beside it what goes there, what's in it and
/// what it gives. Armor rows sit mirrored, so every slot faces the picture.
fn gear_row(
    p: &mut ChildSpawnerCommands,
    view: &SlotView,
    d: &Data,
    slot: WearSlot,
    mirrored: bool,
) {
    let worn = d.inv.worn.get(&slot).copied();
    let (name, detail) = match worn {
        Some(id) => {
            let it = d.items.get(id);
            let mut parts = Vec::new();
            match &it.kind {
                ItemKind::Wear {
                    insulation,
                    protection,
                    cooling,
                    air,
                    ..
                } => {
                    if *insulation > 0.0 {
                        parts.push(format!(
                            "+{}",
                            crate::settings::temp_diff(*insulation, 0, false)
                        ));
                    }
                    if *cooling > 0.0 {
                        parts.push(format!(
                            "heat -{}",
                            crate::settings::temp_diff(*cooling, 0, false)
                        ));
                    }
                    if *air > 0.0 {
                        parts.push(format!("air +{:.0}%", air * 100.0));
                    }
                    for l in Layer::ALL {
                        if let Some(v) = protection.get(&l) {
                            parts.push(format!("{} -{:.0}%", l.name(), v * 100.0));
                        }
                    }
                }
                ItemKind::Bag { grid } => parts.push(format!("holds {}x{}", grid.0, grid.1)),
                _ => {}
            }
            (it.title.clone(), parts.join("  "))
        }
        None => ("empty".to_string(), String::new()),
    };
    let label = match slot {
        WearSlot::Back => "Back (bag)",
        s => s.title(),
    };
    p.spawn(Node {
        column_gap: Val::Px(12.0),
        align_items: AlignItems::Center,
        flex_direction: if mirrored {
            FlexDirection::RowReverse
        } else {
            FlexDirection::Row
        },
        ..default()
    })
    .with_children(|r| {
        view.slot(r, Hover::Slot(slot), worn.map(|w| Stack::new(w, 1)), SLOT);
        r.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: if mirrored {
                AlignItems::FlexEnd
            } else {
                AlignItems::FlexStart
            },
            flex_grow: 1.0,
            ..default()
        })
        .with_children(|t| {
            t.spawn(text(label, sizes::SMALL, colors::MUTED));
            t.spawn(text(
                name,
                sizes::BODY,
                if worn.is_some() {
                    colors::TEXT
                } else {
                    colors::FAINT
                },
            ));
            if !detail.is_empty() {
                t.spawn(text(detail, sizes::SMALL, colors::GOOD));
            }
        });
    });
}
