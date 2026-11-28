//! The Stats tab: read-only numbers in framed cards. Health, the body (food,
//! air, load), gear, temperature, progress and what your skills add up to.

use bevy::prelude::*;

use super::{CONTENT_H, CONTENT_W, Data, text};
use crate::survival::health::{Layer, MAX};
use crate::survival::vitals;
use crate::theme::{Theme, colors, sizes};

const COLS: f32 = 4.0;
const GAP: f32 = 14.0;
const COL_W: f32 = (CONTENT_W - GAP * (COLS - 1.0)) / COLS;
/// Width of a meter's name.
const NAME_W: f32 = 76.0;

/// Health bar colours, by layer.
fn layer_color(l: Layer) -> Color {
    match l {
        Layer::Damage => Color::srgb(0.86, 0.24, 0.22),
        Layer::Fatigue => Color::srgb(0.95, 0.78, 0.25),
        Layer::Blood => Color::srgb(0.62, 0.08, 0.12),
        Layer::Injuries => Color::srgb(0.85, 0.75, 0.6),
    }
}

pub(super) fn build(c: &mut ChildSpawnerCommands, theme: &Theme, d: &Data) {
    let v = &d.s.vitals;
    let env = &d.s.env;
    let items = &*d.items;
    c.spawn(Node {
        column_gap: Val::Px(GAP),
        width: Val::Percent(100.0),
        height: Val::Px(CONTENT_H),
        ..default()
    })
    .with_children(|row| {
        // Health and the body.
        column(row, |col| {
            card(col, theme, "Health", "icon_damage", |k| {
                for l in Layer::ALL {
                    let x = v.health.get(l);
                    bar(
                        k,
                        theme,
                        super::layer_icon(l),
                        l.name(),
                        x / MAX,
                        format!("{x:.0}"),
                        layer_color(l),
                        x < MAX * 0.3,
                    );
                }
                let (s, color) = if v.health.bleeding > 0.0 {
                    (format!("Bleeding {:.1}/s", v.health.bleeding), colors::BAD)
                } else {
                    ("Not bleeding".to_string(), colors::MUTED)
                };
                k.spawn(text(s, sizes::SMALL, color));
            });
            card(col, theme, "Body", "icon_food", |k| {
                bar(
                    k,
                    theme,
                    "icon_food",
                    "Food",
                    v.satiety / 100.0,
                    format!("{:.0}", v.satiety),
                    Color::srgb(0.85, 0.55, 0.25),
                    v.satiety < 25.0,
                );
                bar(
                    k,
                    theme,
                    "icon_air",
                    "Air",
                    v.air / 100.0,
                    format!("{:.0}%", v.air),
                    Color::srgb(0.45, 0.72, 0.95),
                    v.air < 60.0,
                );
                bar(
                    k,
                    theme,
                    "icon_air",
                    "Oxygen",
                    v.oxygen / 100.0,
                    format!("{:.0}%", v.oxygen),
                    Color::srgb(0.6, 0.85, 1.0),
                    v.oxygen < 60.0,
                );
                let load = vitals::load(env.carried);
                bar(
                    k,
                    theme,
                    "icon_load",
                    "Load",
                    load / (vitals::HEAVY_ABOVE * 2.0),
                    format!("{:.0} kg", env.carried),
                    Color::srgb(0.65, 0.68, 0.7),
                    vitals::is_heavy(env.carried),
                );
            });
        });
        // Gear and progress.
        column(row, |col| {
            card(col, theme, "Gear", "icon_injuries", |k| {
                line(
                    k,
                    theme,
                    "icon_warmth",
                    "Clothes warmth",
                    format!(
                        "+{}",
                        crate::settings::temp_diff(d.inv.insulation(items), 0, false)
                    ),
                    colors::TEXT,
                );
                for l in Layer::ALL {
                    let p = d.inv.protection(items, l);
                    bar(
                        k,
                        theme,
                        super::layer_icon(l),
                        l.name(),
                        p / crate::inventory::MAX_PROTECTION,
                        format!("-{:.0}%", p * 100.0),
                        Color::srgb(0.55, 0.7, 0.85),
                        false,
                    );
                }
                line(
                    k,
                    theme,
                    "icon_load",
                    "Gear weight",
                    format!("{:.1} kg", d.inv.mass(items)),
                    colors::TEXT,
                );
            });
            card(col, theme, "Progress", "icon_xp", |k| {
                let p = &d.progress;
                line(
                    k,
                    theme,
                    "icon_xp",
                    "Level",
                    format!("{}", p.level.level),
                    colors::HEADING,
                );
                bar(
                    k,
                    theme,
                    "icon_xp",
                    "XP",
                    p.level.fraction(&d.prog),
                    format!("{:.0}%", p.level.fraction(&d.prog) * 100.0),
                    Color::srgb(0.45, 0.85, 0.4),
                    false,
                );
                line(
                    k,
                    theme,
                    "icon_star",
                    "Skill points",
                    format!("{}", p.learned.points),
                    if p.learned.points > 0 {
                        colors::GOOD
                    } else {
                        colors::TEXT
                    },
                );
                let max = d.perks.max_focus();
                bar(
                    k,
                    theme,
                    "icon_focus",
                    "Focus",
                    p.focus / max.max(1.0),
                    format!("{:.0}/{:.0}", p.focus, max),
                    Color::srgb(0.66, 0.45, 0.88),
                    false,
                );
            });
        });
        // How warm it feels, part by part.
        column(row, |col| {
            card(col, theme, "Temperature", "icon_warmth", |k| {
                let color = if v.feels < vitals::COMFORT_LOW {
                    Color::srgb(0.55, 0.78, 1.0)
                } else if v.feels > vitals::COMFORT_HIGH {
                    colors::BAD
                } else {
                    colors::GOOD
                };
                k.spawn(Node {
                    column_gap: Val::Px(10.0),
                    align_items: AlignItems::Center,
                    margin: UiRect::bottom(Val::Px(4.0)),
                    ..default()
                })
                .with_children(|r| {
                    r.spawn(theme.icon("icon_warmth"));
                    r.spawn((
                        theme.bold(sizes::TITLE),
                        Text::new(crate::settings::temp(v.feels, 0)),
                        TextColor(color),
                    ));
                    r.spawn(text("feels like", sizes::SMALL, colors::MUTED));
                });
                for (what, c) in d.s.temperature.iter().filter(|(_, c)| c.abs() >= 0.5) {
                    k.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        width: Val::Percent(100.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            text(what.clone(), sizes::SMALL, colors::TEXT),
                            Node {
                                max_width: Val::Px(COL_W - 90.0),
                                ..default()
                            },
                        ));
                        r.spawn(text(
                            crate::settings::temp_diff(*c, 0, true),
                            sizes::SMALL,
                            if *c >= 0.0 {
                                Color::srgb(1.0, 0.72, 0.45)
                            } else {
                                Color::srgb(0.55, 0.78, 1.0)
                            },
                        ));
                    });
                }
            });
        });
        // What your skills add up to.
        column(row, |col| {
            card(col, theme, "Skills", "icon_star", |k| {
                let p = &*d.perks;
                let pct = |x: f32| format!("+{:.0}%", x * 100.0);
                for (icon, what, value, any) in [
                    (
                        "icon_load",
                        "Carry",
                        format!("+{:.0} kg", p.carry),
                        p.carry != 0.0,
                    ),
                    ("icon_air", "Thin air", pct(p.air), p.air != 0.0),
                    (
                        "icon_warmth",
                        "Cold",
                        crate::settings::temp_diff(p.cold, 0, false),
                        p.cold != 0.0,
                    ),
                    ("icon_fatigue", "Speed", pct(p.speed), p.speed != 0.0),
                    ("icon_injuries", "Mining", pct(p.mining), p.mining != 0.0),
                    ("icon_sword", "Damage", pct(p.damage), p.damage != 0.0),
                    (
                        "icon_focus",
                        "Focus",
                        format!("+{:.0}", p.focus),
                        p.focus != 0.0,
                    ),
                    (
                        "icon_focus",
                        "Focus regen",
                        format!("+{:.1}/s", p.focus_regen),
                        p.focus_regen != 0.0,
                    ),
                ] {
                    line(
                        k,
                        theme,
                        icon,
                        what,
                        value,
                        if any { colors::GOOD } else { colors::FAINT },
                    );
                }
                let spells: Vec<&str> = p.spells.iter().map(|s| s.title()).collect();
                k.spawn(text(
                    if spells.is_empty() {
                        "No spells yet".to_string()
                    } else {
                        format!("Spells: {}", spells.join(", "))
                    },
                    sizes::SMALL,
                    colors::MUTED,
                ));
            });
        });
    });
}

fn column(row: &mut ChildSpawnerCommands, f: impl FnOnce(&mut ChildSpawnerCommands)) {
    row.spawn(Node {
        flex_direction: FlexDirection::Column,
        width: Val::Px(COL_W),
        row_gap: Val::Px(GAP),
        ..default()
    })
    .with_children(f);
}

fn card(
    p: &mut ChildSpawnerCommands,
    theme: &Theme,
    title: &str,
    icon: &str,
    f: impl FnOnce(&mut ChildSpawnerCommands),
) {
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(5.0),
            padding: UiRect::all(Val::Px(12.0)),
            width: Val::Percent(100.0),
            ..default()
        },
        theme.skin("card"),
    ))
    .with_children(|k| {
        k.spawn(Node {
            column_gap: Val::Px(8.0),
            align_items: AlignItems::Center,
            margin: UiRect::bottom(Val::Px(2.0)),
            ..default()
        })
        .with_children(|h| {
            h.spawn(theme.icon(icon));
            h.spawn((
                theme.bold(sizes::LABEL),
                Text::new(title),
                TextColor(colors::HEADING),
            ));
        });
        k.spawn((
            Node {
                height: Val::Px(4.0),
                margin: UiRect::bottom(Val::Px(2.0)),
                ..default()
            },
            theme.skin("divider"),
        ));
        f(k);
    });
}

/// An icon, a name, and a value at the right.
fn line(
    p: &mut ChildSpawnerCommands,
    theme: &Theme,
    icon: &str,
    what: &str,
    value: String,
    color: Color,
) {
    p.spawn(Node {
        column_gap: Val::Px(8.0),
        align_items: AlignItems::Center,
        width: Val::Percent(100.0),
        ..default()
    })
    .with_children(|r| {
        r.spawn(theme.icon(icon));
        r.spawn((
            text(what, sizes::BODY, colors::TEXT),
            Node {
                flex_grow: 1.0,
                ..default()
            },
        ));
        r.spawn(text(value, sizes::BODY, color));
    });
}

/// An icon, a name, a meter and its value. `warn` turns the value red.
#[allow(clippy::too_many_arguments)]
fn bar(
    p: &mut ChildSpawnerCommands,
    theme: &Theme,
    icon: &str,
    what: &str,
    frac: f32,
    value: String,
    fill: Color,
    warn: bool,
) {
    p.spawn(Node {
        column_gap: Val::Px(8.0),
        align_items: AlignItems::Center,
        width: Val::Percent(100.0),
        ..default()
    })
    .with_children(|r| {
        r.spawn(theme.icon(icon));
        r.spawn((
            text(what, sizes::BODY, colors::TEXT),
            Node {
                width: Val::Px(NAME_W),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        r.spawn((
            Node {
                flex_grow: 1.0,
                height: Val::Px(16.0),
                padding: UiRect::all(Val::Px(3.0)),
                ..default()
            },
            theme.skin("bar"),
        ))
        .with_children(|b| {
            b.spawn((
                Node {
                    width: Val::Percent(frac.clamp(0.0, 1.0) * 100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                theme.skin_tinted("fill", fill),
            ));
        });
        r.spawn((
            text(
                value,
                sizes::SMALL,
                if warn { colors::BAD } else { colors::TEXT },
            ),
            Node {
                width: Val::Px(50.0),
                flex_shrink: 0.0,
                ..default()
            },
            TextLayout::justify(Justify::Right),
        ));
    });
}
