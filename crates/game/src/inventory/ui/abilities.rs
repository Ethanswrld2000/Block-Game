//! The Abilities tab: your spells as cards on the left, and the skill tree
//! on the right (`progress::ui` draws the tree over `TreeArea`).

use bevy::prelude::*;

use super::{CONTENT_H, Data, TreeArea, text};
use crate::progress::skills::Spell;
use crate::progress::{LEVITATE_DRAIN, LIGHT_DRAIN, WARMTH_DRAIN};
use crate::theme::{Theme, colors, sizes};

const LIST_W: f32 = 300.0;

pub(super) fn build(c: &mut ChildSpawnerCommands, theme: &Theme, d: &Data) {
    c.spawn(Node {
        column_gap: Val::Px(16.0),
        width: Val::Percent(100.0),
        height: Val::Px(CONTENT_H),
        ..default()
    })
    .with_children(|row| {
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            width: Val::Px(LIST_W),
            row_gap: Val::Px(10.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|col| {
            col.spawn(Node {
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                height: Val::Px(30.0),
                ..default()
            })
            .with_children(|h| {
                h.spawn(theme.icon("icon_focus"));
                h.spawn((
                    theme.bold(sizes::HEADING),
                    Text::new("Spells"),
                    TextColor(colors::HEADING),
                ));
            });
            // Focus, which every spell burns.
            let max = d.perks.max_focus();
            col.spawn(Node {
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|r| {
                r.spawn(text("Focus", sizes::BODY, colors::TEXT));
                r.spawn((
                    Node {
                        width: Val::Px(170.0),
                        height: Val::Px(16.0),
                        padding: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    theme.skin("bar"),
                ))
                .with_children(|b| {
                    b.spawn((
                        Node {
                            width: Val::Percent(
                                (d.progress.focus / max.max(1.0)).clamp(0.0, 1.0) * 100.0,
                            ),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        theme.skin_tinted("fill", Color::srgb(0.66, 0.45, 0.88)),
                    ));
                });
                r.spawn(text(
                    format!("{:.0}/{:.0}", d.progress.focus, max),
                    sizes::SMALL,
                    colors::MUTED,
                ));
            });
            for (spell, key, icon, drain, on) in [
                (
                    Spell::Levitation,
                    "G",
                    "icon_levitate",
                    LEVITATE_DRAIN * (1.0 - d.perks.levitate_cost.min(0.8)),
                    d.spells.levitate,
                ),
                (Spell::Light, "X", "icon_light", LIGHT_DRAIN, d.spells.light),
                (
                    Spell::Warmth,
                    "V",
                    "icon_warmth",
                    WARMTH_DRAIN,
                    d.spells.warmth,
                ),
            ] {
                let known = d.perks.spells.contains(&spell);
                let skin = if on { "slot_glow" } else { "card" };
                col.spawn((
                    Node {
                        column_gap: Val::Px(12.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::all(Val::Px(10.0)),
                        ..default()
                    },
                    theme.skin(skin),
                ))
                .with_children(|k| {
                    // The key, as a little keycap.
                    k.spawn((
                        Node {
                            width: Val::Px(34.0),
                            height: Val::Px(34.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            flex_shrink: 0.0,
                            ..default()
                        },
                        theme.skin(if known { "button" } else { "button_off" }),
                    ))
                    .with_children(|b| {
                        b.spawn((
                            theme.bold(sizes::LABEL),
                            Text::new(key),
                            TextColor(if known { colors::TEXT } else { colors::FAINT }),
                        ));
                    });
                    let mut ic = theme.image(icon);
                    if !known {
                        ic.color = Color::srgba(1.0, 1.0, 1.0, 0.35);
                    }
                    k.spawn((
                        ic,
                        Node {
                            width: Val::Px(24.0),
                            height: Val::Px(24.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    k.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|t| {
                        t.spawn((
                            theme.bold(sizes::BODY),
                            Text::new(spell.title()),
                            TextColor(if known {
                                colors::HEADING
                            } else {
                                colors::FAINT
                            }),
                        ));
                        let (s, c) = match (known, on) {
                            (true, true) => (format!("On, {drain:.1} focus/s"), colors::GOOD),
                            (true, false) => (format!("{drain:.1} focus/s"), colors::MUTED),
                            (false, _) => ("Learn it in the tree".to_string(), colors::FAINT),
                        };
                        t.spawn(text(s, sizes::SMALL, c));
                    });
                    if !known {
                        k.spawn((
                            theme.image("icon_lock"),
                            Node {
                                width: Val::Px(24.0),
                                height: Val::Px(24.0),
                                margin: UiRect::left(Val::Auto),
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ));
                    }
                });
            }
            let pts = d.progress.learned.points;
            col.spawn(Node {
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                margin: UiRect::top(Val::Px(6.0)),
                ..default()
            })
            .with_children(|r| {
                r.spawn(theme.icon("icon_star"));
                r.spawn(text(
                    format!(
                        "Level {}, {pts} skill point{} to spend",
                        d.progress.level.level,
                        if pts == 1 { "" } else { "s" }
                    ),
                    sizes::BODY,
                    if pts > 0 { colors::GOOD } else { colors::MUTED },
                ));
            });
        });
        // The tree goes here.
        row.spawn((
            TreeArea,
            Node {
                flex_grow: 1.0,
                height: Val::Px(CONTENT_H),
                ..default()
            },
            theme.skin("inset"),
        ));
    });
}
