//! Survival HUD: the four health bars (bottom left), food, the air around you,
//! felt temperature and load (bottom right), short messages, the sleep fade
//! and the death screen. The air and temperature meters are always shown and
//! shake harder the worse the exposure stage you're in.

use bevy::prelude::*;

use super::camp::{Camp, Sleep};
use super::exposure::{Exposure, Scale};
use super::health::{Layer, MAX, MORTAL_AT};
use super::vitals::{self, CAPACITY, HEAVY_ABOVE, LIGHT_BELOW};
use super::{Survival, Toast};
use crate::player::Player;
use crate::theme::{self, Theme, colors, sizes};

pub struct SurvivalHudPlugin;

impl Plugin for SurvivalHudPlugin {
    fn build(&self, app: &mut App) {
        // PostStartup: after the weather panel, so this stacks below it top right.
        app.add_systems(crate::menu::GamePostStartup, spawn)
            .add_systems(
                crate::menu::GameUpdate,
                (
                    update_bars,
                    bubble_blood,
                    update_meters,
                    temp_icon,
                    shake,
                    temperature_list,
                    toasts,
                    overlays,
                    cold_vignette,
                )
                    .after(super::SurvivalSet),
            );
    }
}

const BAR_W: f32 = 170.0;
const BAR_H: f32 = 14.0;
/// The health bars are a little taller, so the blood has room to bubble.
const HEALTH_H: f32 = 18.0;
/// Inside a health bar's frame (3 points each side).
const HEALTH_INNER: f32 = HEALTH_H - 6.0;
/// Bubbles kept ready in the blood bar.
const BUBBLES: usize = 8;
/// A lost chunk of a bar stays pale this long before it drains away, s.
const GHOST_HOLD: f32 = 0.45;
/// How fast the pale chunk drains, share of the bar per second.
const GHOST_DRAIN: f32 = 0.6;
/// Food, air, warmth and load bars, and the width of the value after them.
const METER_W: f32 = 130.0;
const VALUE_W: f32 = 150.0;
/// Load gauge runs from 0 to this share of capacity.
const LOAD_SCALE: f32 = 1.2;
/// The temperature gauge's range, °C.
const TEMP_LOW: f32 = -40.0;
const TEMP_HIGH: f32 = 55.0;
/// How far a meter shakes at full strength, px.
const SHAKE_PX: f32 = 3.5;

fn layer_color(l: Layer) -> Color {
    match l {
        Layer::Damage => Color::srgb(0.92, 0.84, 0.62),
        Layer::Fatigue => Color::srgb(0.45, 0.78, 0.62),
        Layer::Blood => Color::srgb(0.85, 0.16, 0.16),
        Layer::Injuries => Color::srgb(0.62, 0.38, 0.85),
    }
}

#[derive(Component)]
struct BarFill(Layer);

/// What a bar just lost, shown pale behind the fill before it drains away.
#[derive(Component)]
struct BarGhost(Layer);

/// A pixel rising through the blood.
#[derive(Component)]
struct Bubble(usize);

/// The temperature meter's picture: thermometer, snowflake or flame.
#[derive(Component)]
struct TempIcon;

#[derive(Component)]
struct BarNote(Layer);

#[derive(Component)]
struct FoodFill;

/// A meter row that shakes with its exposure stage.
#[derive(Component)]
struct Shake(Gauge);

#[derive(Component)]
struct Marker(Gauge);

#[derive(Component)]
struct GaugeText(Gauge);

#[derive(Clone, Copy, PartialEq)]
enum Gauge {
    Temperature,
    Air,
    Load,
}

/// The temperature breakdown, in the top-right column under the weather (F6 shows and hides it).
#[derive(Component)]
struct TempPanel;

#[derive(Component)]
struct TempHeader;

/// One line of the breakdown: its slot, and whether it's the label or the value.
#[derive(Component)]
struct TempLine(usize);

#[derive(Component)]
struct TempLabel(usize);

#[derive(Component)]
struct TempValue(usize);

/// Lines kept ready for the breakdown.
const TEMP_LINES: usize = 14;

#[derive(Component)]
struct ToastText;

#[derive(Component)]
struct CampText;

#[derive(Component)]
struct Dizzy;

/// Blue edges that close in with the cold (`shiver::vignette`); holds the strength last drawn.
#[derive(Component, Default)]
struct ColdVignette(f32);

/// The cold vignette's colour at full strength.
const VIGNETTE_BLUE: Color = Color::srgba(0.35, 0.6, 0.95, 0.0);

/// Clear in the middle, faint blue towards the corners, `strength` 0..1.
/// Stops stay at fixed, increasing positions (out of order they flip the gradient);
/// only the alpha follows the strength.
fn vignette_gradient(strength: f32) -> BackgroundGradient {
    let blue = |a: f32| VIGNETTE_BLUE.with_alpha(a * strength);
    BackgroundGradient::from(RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::FarthestCorner,
        vec![
            ColorStop::percent(blue(0.0), 60.0),
            ColorStop::percent(blue(0.08), 85.0),
            ColorStop::percent(blue(0.22), 100.0),
        ],
    ))
}

#[derive(Component)]
struct SleepFade;

#[derive(Component)]
struct DeathScreen;

#[derive(Component)]
struct DeathHint;

fn text(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    theme::text(s, size, color)
}

/// The frame round a bar; what's inside sits within its 2-point edge.
fn bar_back(theme: &Theme, w: f32) -> impl Bundle {
    (
        Node {
            width: Val::Px(w),
            height: Val::Px(BAR_H),
            padding: UiRect::all(Val::Px(3.0)),
            ..default()
        },
        theme.skin("bar"),
    )
}

/// The bars' contents go in here, so zones and markers line up with the fill.
fn bar_inner() -> Node {
    Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    }
}

fn hud_panel(theme: &Theme) -> impl Bundle {
    theme.skin("panel_light")
}

/// The toast's frame, hidden while there's nothing to say.
#[derive(Component)]
struct ToastBox;

fn fill(color: Color) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(color),
    )
}

fn tick(at: f32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(at * 100.0),
            width: Val::Px(1.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
    )
}

fn zone(from: f32, to: f32, color: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(from * 100.0),
            width: Val::Percent((to - from) * 100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(color),
    )
}

/// Colour of an exposure stage band: deeper is stronger.
fn stage_color(scale: Scale, level: usize) -> Color {
    let a = 0.4 + 0.2 * level as f32;
    match scale {
        Scale::Cold => Color::srgba(0.35 - 0.1 * level as f32, 0.6 - 0.12 * level as f32, 1.0, a),
        Scale::Heat => Color::srgba(1.0, 0.65 - 0.2 * level as f32, 0.2, a),
        Scale::Air => Color::srgba(0.7 - 0.1 * level as f32, 0.45 - 0.12 * level as f32, 0.9, a),
    }
}

const COMFORT: Color = Color::srgba(0.4, 0.8, 0.4, 0.35);

/// Bands for a scale's stages on a gauge from `lo` to `hi`: comfortable from
/// the first stage to `edge`, then each stage out to the next.
fn stage_zones(
    b: &mut ChildSpawnerCommands,
    rules: &Exposure,
    scale: Scale,
    lo: f32,
    hi: f32,
    edge: f32,
) {
    let at = |v: f32| ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    let stages = rules.stages(scale);
    let low_side = scale != Scale::Heat;
    let first = stages.first().map_or(edge, |s| s.at);
    if low_side {
        b.spawn(zone(at(first), at(edge), COMFORT));
    }
    for (i, s) in stages.iter().enumerate() {
        let end = if low_side {
            stages.get(i + 1).map_or(lo, |n| n.at)
        } else {
            stages.get(i + 1).map_or(hi, |n| n.at)
        };
        let (a, z) = if low_side {
            (at(end), at(s.at))
        } else {
            (at(s.at), at(end))
        };
        b.spawn(zone(a, z, stage_color(scale, i)));
    }
}

fn marker(g: Gauge) -> impl Bundle {
    (
        Marker(g),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            width: Val::Px(3.0),
            top: Val::Px(-4.0),
            height: Val::Px(BAR_H + 2.0),
            ..default()
        },
        BackgroundColor(Color::WHITE),
    )
}

fn full_screen(color: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(color),
        Pickable::IGNORE,
    )
}

fn spawn(
    mut commands: Commands,
    theme: Res<Theme>,
    rules: Res<Exposure>,
    top_right: Res<crate::hud::TopRight>,
) {
    let theme = &*theme;
    let rules = &*rules;
    commands.spawn((
        ColdVignette::default(),
        full_screen(Color::NONE),
        vignette_gradient(0.0),
    ));
    commands.spawn((Dizzy, full_screen(Color::srgba(0.02, 0.03, 0.08, 0.0))));

    // Health, outermost bar on top.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(14.0),
            bottom: Val::Px(14.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
            ..default()
        })
        .insert(hud_panel(theme))
        .with_children(|col| {
            for l in Layer::ALL {
                col.spawn(Node {
                    column_gap: Val::Px(8.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(theme.icon(match l {
                        Layer::Damage => "icon_damage",
                        Layer::Fatigue => "icon_fatigue",
                        Layer::Blood => "icon_blood",
                        Layer::Injuries => "icon_injuries",
                    }));
                    row.spawn((
                        text(l.name(), sizes::SMALL, colors::TEXT),
                        Node {
                            width: Val::Px(70.0),
                            ..default()
                        },
                    ));
                    row.spawn((
                        Node {
                            width: Val::Px(BAR_W),
                            height: Val::Px(HEALTH_H),
                            padding: UiRect::all(Val::Px(3.0)),
                            ..default()
                        },
                        theme.skin("bar"),
                    ))
                    .with_children(|b| {
                        b.spawn(bar_inner()).with_children(|b| {
                            b.spawn((
                                BarGhost(l),
                                zone(0.0, 1.0, Color::srgba(1.0, 0.95, 0.85, 0.55)),
                            ));
                            // The fill: the bar sprite's lit top row and shaded bottom,
                            // in the layer's colour, with a bright leading edge.
                            b.spawn((
                                BarFill(l),
                                Node {
                                    position_type: PositionType::Absolute,
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                theme.skin_tinted("fill", layer_color(l)),
                            ))
                            .with_children(|f| {
                                f.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        right: Val::Px(0.0),
                                        width: Val::Px(2.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.4)),
                                ));
                                if l == Layer::Blood {
                                    for i in 0..BUBBLES {
                                        f.spawn((
                                            Bubble(i),
                                            Node {
                                                position_type: PositionType::Absolute,
                                                width: Val::Px(2.0),
                                                height: Val::Px(2.0),
                                                ..default()
                                            },
                                            BackgroundColor(Color::srgba(1.0, 0.62, 0.58, 0.0)),
                                        ));
                                    }
                                }
                            });
                            if l != Layer::Damage {
                                b.spawn(tick(MORTAL_AT));
                            }
                        });
                    });
                    row.spawn((BarNote(l), text("", sizes::SMALL, colors::BAD)));
                });
            }
        });

    // Meters.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            right: Val::Px(14.0),
            bottom: Val::Px(14.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
            align_items: AlignItems::Start,
            ..default()
        })
        .insert(hud_panel(theme))
        .with_children(|col| {
            col.spawn(Node {
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                width: Val::Percent(100.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn(theme.icon("icon_camp"));
                row.spawn((CampText, text("", sizes::SMALL, colors::MUTED)));
            });
            col.spawn(Node {
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn(theme.icon("icon_food"));
                row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                    b.spawn(bar_inner()).with_children(|b| {
                        b.spawn((FoodFill, fill(Color::srgb(0.9, 0.62, 0.3))));
                    });
                });
                row.spawn((
                    text("Food", sizes::SMALL, colors::TEXT),
                    Node {
                        width: Val::Px(VALUE_W),
                        ..default()
                    },
                ));
            });
            // Air: how much oxygen the air here holds, with its stages.
            col.spawn((
                Shake(Gauge::Air),
                Node {
                    column_gap: Val::Px(8.0),
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|row| {
                row.spawn(theme.icon("icon_air"));
                row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                    b.spawn(bar_inner()).with_children(|b| {
                        stage_zones(b, rules, Scale::Air, 0.0, 100.0, 100.0);
                        b.spawn(marker(Gauge::Air));
                    });
                });
                row.spawn((
                    GaugeText(Gauge::Air),
                    text("", sizes::SMALL, colors::TEXT),
                    Node {
                        width: Val::Px(VALUE_W),
                        ..default()
                    },
                ));
            });
            // Felt temperature: cold stages, comfortable, heat stages.
            col.spawn((
                Shake(Gauge::Temperature),
                Node {
                    column_gap: Val::Px(8.0),
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_children(|row| {
                row.spawn((TempIcon, theme.icon("icon_temp")));
                row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                    b.spawn(bar_inner()).with_children(|b| {
                        let (cold, hot) = rules.comfort();
                        stage_zones(b, rules, Scale::Cold, TEMP_LOW, TEMP_HIGH, hot);
                        stage_zones(b, rules, Scale::Heat, TEMP_LOW, TEMP_HIGH, cold);
                        b.spawn(marker(Gauge::Temperature));
                    });
                });
                row.spawn((
                    GaugeText(Gauge::Temperature),
                    text("", sizes::SMALL, colors::TEXT),
                    Node {
                        width: Val::Px(VALUE_W),
                        ..default()
                    },
                ));
            });
            // Load: light, the sweet spot, heavy.
            col.spawn(Node {
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn(theme.icon("icon_load"));
                row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                    let mut b = b.spawn(bar_inner());
                    b.with_children(|b| {
                        b.spawn(zone(
                            0.0,
                            LIGHT_BELOW / LOAD_SCALE,
                            Color::srgba(0.5, 0.75, 1.0, 0.45),
                        ));
                        b.spawn(zone(
                            LIGHT_BELOW / LOAD_SCALE,
                            HEAVY_ABOVE / LOAD_SCALE,
                            Color::srgba(0.4, 0.85, 0.4, 0.55),
                        ));
                        b.spawn(zone(
                            HEAVY_ABOVE / LOAD_SCALE,
                            1.0,
                            Color::srgba(1.0, 0.55, 0.2, 0.55),
                        ));
                        b.spawn(tick(1.0 / LOAD_SCALE));
                        b.spawn(marker(Gauge::Load));
                    });
                });
                row.spawn((
                    GaugeText(Gauge::Load),
                    text("", sizes::SMALL, colors::TEXT),
                    Node {
                        width: Val::Px(VALUE_W),
                        ..default()
                    },
                ));
            });
        });

    // What's making you warm or cold, each with its share.
    commands
        .spawn((
            TempPanel,
            ChildOf(top_right.0),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
                min_width: Val::Px(250.0),
                ..default()
            },
            hud_panel(theme),
        ))
        .with_children(|col| {
            col.spawn((TempHeader, text("", sizes::SMALL, colors::TEXT)));
            for i in 0..TEMP_LINES {
                col.spawn((
                    TempLine(i),
                    Node {
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: Val::Px(16.0),
                        display: Display::None,
                        ..default()
                    },
                ))
                .with_children(|row| {
                    row.spawn((TempLabel(i), text("", sizes::SMALL, colors::MUTED)));
                    row.spawn((TempValue(i), text("", sizes::SMALL, colors::TEXT)));
                });
            }
            col.spawn(text("F6 hides this", sizes::SMALL, colors::MUTED));
        });

    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(26.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|c| {
            c.spawn((
                ToastBox,
                Node {
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                    ..default()
                },
                theme.skin("tooltip"),
                Visibility::Hidden,
            ))
            .with_children(|b| {
                b.spawn((ToastText, text("", sizes::LABEL, colors::TEXT)));
            });
        });

    commands.spawn((
        SleepFade,
        full_screen(Color::srgba(0.0, 0.0, 0.0, 0.0)),
        GlobalZIndex(5),
    ));
    commands
        .spawn((
            DeathScreen,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.25, 0.02, 0.02, 0.6)),
            Visibility::Hidden,
            GlobalZIndex(6),
        ))
        .with_children(|c| {
            c.spawn((
                theme.bold(sizes::HUGE),
                Text::new("You perished"),
                TextColor(Color::srgb(1.0, 0.85, 0.8)),
            ));
            c.spawn((DeathHint, text("", sizes::LABEL, colors::TEXT)));
        });
}

#[allow(clippy::type_complexity)]
fn update_bars(
    time: Res<Time<Real>>,
    s: Res<Survival>,
    mut ghosts_at: Local<[(f32, f32); 4]>,
    mut fills: Query<(&BarFill, &mut Node), Without<BarGhost>>,
    mut ghosts: Query<(&BarGhost, &mut Node), Without<BarFill>>,
    mut notes: Query<(&BarNote, &mut Text)>,
) {
    let h = &s.vitals.health;
    let mortal = h.mortal();
    let now = time.elapsed_secs();
    for (f, mut node) in &mut fills {
        let w = Val::Percent(h.get(f.0) / MAX * 100.0);
        if node.width != w {
            node.width = w;
        }
    }
    // The pale chunk: jumps to a gain at once, holds a loss a moment, then drains.
    for (i, l) in Layer::ALL.into_iter().enumerate() {
        let (ghost, lost_at) = &mut ghosts_at[i];
        let v = h.get(l) / MAX;
        if v >= *ghost {
            *ghost = v;
            *lost_at = now;
        } else if now - *lost_at > GHOST_HOLD {
            *ghost = (*ghost - GHOST_DRAIN * time.delta_secs()).max(v);
        }
    }
    for (g, mut node) in &mut ghosts {
        let i = Layer::ALL.iter().position(|l| *l == g.0).unwrap_or(0);
        let w = Val::Percent(ghosts_at[i].0 * 100.0);
        if node.width != w {
            node.width = w;
        }
    }
    for (n, mut t) in &mut notes {
        let mut note = String::new();
        if mortal == Some(n.0) {
            note.push_str("MORTAL");
        } else if mortal.is_some_and(|m| n.0 < m) {
            note.push_str("can't heal");
        }
        if n.0 == Layer::Blood && h.bleeding > 0.01 {
            if !note.is_empty() {
                note.push_str(", ");
            }
            note.push_str("bleeding");
        }
        if t.0 != note {
            t.0 = note;
        }
    }
}

/// Where bubble `i` is at time `t` (seconds), with `bleeding` (0 = calm):
/// its place along the blood (0..1), how far up it has risen (0 at the
/// bottom, 1 at the top) and how bright it is. Rises in whole 2-point steps;
/// bleeding makes more of them and quicker. Pure.
pub(crate) fn bubble(i: usize, t: f32, bleeding: f32) -> Option<(f32, f32, f32)> {
    let busy = (bleeding * 4.0).clamp(0.0, 1.0);
    // Calm: three bubbles about; bleeding: all of them, faster.
    let shown = 3 + ((BUBBLES - 3) as f32 * busy).round() as usize;
    if i >= shown {
        return None;
    }
    let period = (1.3 + 0.37 * (i % 3) as f32 + 0.21 * (i % 2) as f32) / (1.0 + busy);
    let phase = i as f32 * 0.618;
    let cycle = (t / period + phase).floor();
    let frac = t / period + phase - cycle;
    // A new spot along the bar each time round.
    let h = ((i as f32 * 12.9898 + cycle * 78.233).sin() * 43758.547)
        .fract()
        .abs();
    let x = 0.06 + 0.88 * h;
    // Steps of 2 points up the inside of the bar.
    let rows = ((HEALTH_INNER - 2.0) / 2.0).floor();
    let rise = (frac * (rows + 1.0)).floor().min(rows) / rows;
    // Brightest low down, fading as it nears the top, gone at the surface.
    let alpha = if frac > 0.9 { 0.0 } else { 0.85 - 0.5 * rise };
    Some((x, rise, alpha))
}

/// Pixels bubble up through the blood bar.
fn bubble_blood(
    time: Res<Time<Real>>,
    s: Res<Survival>,
    mut q: Query<(&Bubble, &mut Node, &mut BackgroundColor)>,
) {
    let t = time.elapsed_secs();
    let h = &s.vitals.health;
    let level = h.get(Layer::Blood) / MAX;
    for (b, mut node, mut bg) in &mut q {
        let (x, rise, a) = match bubble(b.0, t, h.bleeding) {
            // Too little blood left to bubble in.
            Some(v) if level > 0.04 => v,
            _ => (0.0, 0.0, 0.0),
        };
        let (l, top) = (
            Val::Percent(x * 100.0),
            Val::Px(HEALTH_INNER - 2.0 - rise * (HEALTH_INNER - 2.0)),
        );
        if node.left != l || node.top != top {
            node.left = l;
            node.top = top;
        }
        if bg.0.alpha() != a {
            bg.0.set_alpha(a);
        }
    }
}

/// The temperature meter's picture follows how it feels: a snowflake in any
/// cold stage, a flame in any heat stage, a thermometer in between.
fn temp_icon(
    theme: Res<Theme>,
    s: Res<Survival>,
    rules: Res<Exposure>,
    mut shown: Local<&'static str>,
    mut q: Query<&mut ImageNode, With<TempIcon>>,
) {
    let want = match rules.temperature(s.vitals.feels) {
        Some((Scale::Cold, _)) => "icon_cold",
        Some((Scale::Heat, _)) => "icon_warmth",
        _ => "icon_temp",
    };
    if *shown == want {
        return;
    }
    *shown = want;
    for mut img in &mut q {
        *img = theme.image(want);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_meters(
    s: Res<Survival>,
    rules: Res<Exposure>,
    camp: Res<Camp>,
    player: Res<Player>,
    mut fills: Query<&mut Node, (With<FoodFill>, Without<Marker>)>,
    mut markers: Query<(&Marker, &mut Node, &mut BackgroundColor), Without<FoodFill>>,
    mut texts: Query<(&GaugeText, &mut Text, &mut TextColor), Without<CampText>>,
    mut camp_text: Query<&mut Text, (With<CampText>, Without<GaugeText>)>,
) {
    let v = &s.vitals;
    for mut node in &mut fills {
        node.width = Val::Percent(v.satiety.clamp(0.0, 100.0));
    }
    let temp = rules.temperature(v.feels);
    let air = rules.stage(Scale::Air, v.oxygen);
    let load = vitals::load(s.env.carried);
    for (m, mut node, mut bg) in &mut markers {
        let (x, warn) = match m.0 {
            Gauge::Temperature => (
                (v.feels - TEMP_LOW) / (TEMP_HIGH - TEMP_LOW),
                temp.is_some_and(|t| t.1.level > 0),
            ),
            Gauge::Air => (v.air / 100.0, air.is_some_and(|a| a.level > 0)),
            Gauge::Load => (load / LOAD_SCALE, load > HEAVY_ABOVE),
        };
        node.left = Val::Percent(x.clamp(0.0, 1.0) * 100.0 - 0.9);
        bg.0 = if warn {
            Color::srgb(1.0, 0.35, 0.3)
        } else {
            Color::WHITE
        };
    }
    for (g, mut t, mut color) in &mut texts {
        let (s2, c) = match g.0 {
            Gauge::Temperature => match temp {
                Some((_, a)) => (
                    format!("{} {}", a.stage.name, crate::settings::temp(v.feels, 0)),
                    if a.level > 0 {
                        colors::BAD
                    } else {
                        colors::TEXT
                    },
                ),
                None => (crate::settings::temp(v.feels, 0), colors::TEXT),
            },
            Gauge::Air => {
                let c = if air.is_some_and(|a| a.level > 0) {
                    colors::BAD
                } else {
                    colors::TEXT
                };
                if s.env.underwater {
                    if v.breath > 0.0 {
                        (format!("Breath {:.0}s", v.breath.ceil()), colors::TEXT)
                    } else {
                        ("No air!".to_string(), colors::BAD)
                    }
                } else {
                    match air {
                        Some(a) => (format!("{} {:.0}%", a.stage.name, v.air), c),
                        None => (format!("Air {:.0}%", v.air), c),
                    }
                }
            }
            Gauge::Load => {
                let pct = (vitals::weight_speed(s.env.carried) - 1.0) * 100.0;
                let text = if pct.abs() >= 0.5 {
                    format!("{:.0}/{:.0} kg {pct:+.0}%", s.env.carried, CAPACITY)
                } else {
                    format!("{:.0}/{:.0} kg", s.env.carried, CAPACITY)
                };
                (text, colors::TEXT)
            }
        };
        if t.0 != s2 {
            t.0 = s2;
        }
        if color.0 != c {
            color.0 = c;
        }
    }
    if let Ok(mut t) = camp_text.single_mut() {
        let s2 = match camp.bedroll {
            None => "B: make camp".to_string(),
            Some(b) => {
                let d = b.center().distance(player.pos);
                if camp.near(player.pos) {
                    "At camp: Z sleep, B pack up".to_string()
                } else {
                    format!("Camp {d:.0} blocks away")
                }
            }
        };
        if t.0 != s2 {
            t.0 = s2;
        }
    }
}

/// Exposure meters shake: a little in the first stage, hard in the worst.
fn shake(
    time: Res<Time>,
    s: Res<Survival>,
    rules: Res<Exposure>,
    mut rows: Query<(&Shake, &mut Node)>,
) {
    let v = &s.vitals;
    let t = time.elapsed_secs();
    for (sh, mut node) in &mut rows {
        let (strength, seed) = match sh.0 {
            Gauge::Temperature => (
                rules
                    .temperature(v.feels)
                    .map_or(0.0, |(_, a)| a.stage.shake),
                0.0,
            ),
            Gauge::Air => (
                rules
                    .stage(Scale::Air, v.oxygen)
                    .map_or(0.0, |a| a.stage.shake),
                7.3,
            ),
            Gauge::Load => (0.0, 0.0),
        };
        // Shivering: fast and jittery, quicker the stronger it is.
        let f = 18.0 + 20.0 * strength;
        let a = strength * SHAKE_PX;
        let x = ((t * f + seed).sin() * 0.7 + (t * f * 1.7 + seed * 3.0).sin() * 0.3) * a;
        let y = ((t * f * 1.3 + seed * 2.0).cos() * 0.6 + (t * f * 2.3).sin() * 0.4) * a * 0.6;
        let (l, tp) = (Val::Px(x.round()), Val::Px(y.round()));
        if node.left != l || node.top != tp {
            node.left = l;
            node.top = tp;
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn temperature_list(
    keys: Res<ButtonInput<KeyCode>>,
    s: Res<Survival>,
    mut shown: Local<Option<bool>>,
    mut panel: Query<&mut Node, (With<TempPanel>, Without<TempLine>)>,
    mut lines: Query<(&TempLine, &mut Node), Without<TempPanel>>,
    mut header: Query<&mut Text, (With<TempHeader>, Without<TempLabel>, Without<TempValue>)>,
    mut labels: Query<(&TempLabel, &mut Text), (Without<TempHeader>, Without<TempValue>)>,
    mut values: Query<
        (&TempValue, &mut Text, &mut TextColor),
        (Without<TempHeader>, Without<TempLabel>),
    >,
) {
    let on = shown.get_or_insert(true);
    if keys.just_pressed(KeyCode::F6) {
        *on = !*on;
    }
    let d = if *on { Display::Flex } else { Display::None };
    if let Ok(mut n) = panel.single_mut()
        && n.display != d
    {
        n.display = d;
    }
    if !*on {
        return;
    }
    // Hide what adds nothing, except the biome (always the first line).
    let parts: Vec<&(String, f32)> = s
        .temperature
        .iter()
        .enumerate()
        .filter(|(i, p)| *i == 0 || p.1.abs() >= 0.05)
        .map(|(_, p)| p)
        .collect();
    if let Ok(mut t) = header.single_mut() {
        let h = format!(
            "Surroundings {}, you feel {}",
            crate::settings::temp(s.env.feels, 1),
            crate::settings::temp(s.vitals.feels, 1)
        );
        if t.0 != h {
            t.0 = h;
        }
    }
    for (l, mut node) in &mut lines {
        let d = if l.0 < parts.len() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != d {
            node.display = d;
        }
    }
    for (l, mut t) in &mut labels {
        if let Some(p) = parts.get(l.0)
            && t.0 != p.0
        {
            t.0 = p.0.clone();
        }
    }
    for (v, mut t, mut c) in &mut values {
        if let Some(p) = parts.get(v.0) {
            let x = crate::settings::temp_diff(p.1, 1, true);
            if t.0 != x {
                t.0 = x;
            }
            let col = if p.1 > 0.05 {
                Color::srgb(1.0, 0.72, 0.45)
            } else if p.1 < -0.05 {
                Color::srgb(0.6, 0.78, 1.0)
            } else {
                colors::TEXT
            };
            if c.0 != col {
                c.0 = col;
            }
        }
    }
}

fn toasts(
    time: Res<Time>,
    mut msgs: MessageReader<Toast>,
    s: Res<Survival>,
    mut q: Query<&mut Text, With<ToastText>>,
    mut frame: Query<(&mut Visibility, &mut ImageNode), With<ToastBox>>,
    mut age: Local<f32>,
) {
    let Ok(mut t) = q.single_mut() else { return };
    for m in msgs.read() {
        t.0 = m.0.clone();
        *age = 0.0;
    }
    *age += time.delta_secs();
    if s.eating > 0.0 {
        t.0 = format!("Eating... {:.0}%", s.eating * 100.0);
        *age = 2.5;
    }
    // Shown for 3 seconds, then gone.
    let show = *age < 3.5 && !t.0.is_empty();
    if let Ok((mut v, _)) = frame.single_mut() {
        let want = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
    }
}

fn cold_vignette(
    s: Res<Survival>,
    rules: Res<Exposure>,
    mut q: Query<(&mut ColdVignette, &mut BackgroundGradient)>,
) {
    let Ok((mut drawn, mut grad)) = q.single_mut() else {
        return;
    };
    let v = &s.vitals;
    let want = if v.health.is_dead() {
        0.0
    } else {
        super::shiver::vignette(&rules, v.feels)
    };
    if (want - drawn.0).abs() > 0.004 || (want == 0.0 && drawn.0 != 0.0) {
        drawn.0 = want;
        *grad = vignette_gradient(want);
    }
}

#[allow(clippy::too_many_arguments)]
fn overlays(
    s: Res<Survival>,
    rules: Res<Exposure>,
    sleep: Res<Sleep>,
    (camp, respawn): (Res<Camp>, Res<super::RespawnAt>),
    mut dizzy: Query<&mut BackgroundColor, (With<Dizzy>, Without<SleepFade>)>,
    mut fade: Query<&mut BackgroundColor, (With<SleepFade>, Without<Dizzy>)>,
    mut death: Query<&mut Visibility, With<DeathScreen>>,
    mut hint: Query<&mut Text, With<DeathHint>>,
) {
    if let Ok(mut bg) = dizzy.single_mut() {
        bg.0.set_alpha(s.vitals.dizzy(&rules) * 0.55);
    }
    if let Ok(mut bg) = fade.single_mut() {
        bg.0.set_alpha(sleep.darkness());
    }
    let dead = s.vitals.health.is_dead();
    if let Ok(mut v) = death.single_mut() {
        *v = if dead {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if dead && let Ok(mut t) = hint.single_mut() {
        let wait = s.dead_for.unwrap_or(0.0) < 2.0;
        let place = if respawn.0.is_some() {
            "outside the Rotting Gate"
        } else if camp.bedroll.is_some() {
            "at your camp"
        } else {
            "at the edge of the swamp"
        };
        t.0 = if wait {
            String::new()
        } else {
            format!("Press Space to wake {place}")
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubbles_rise_in_whole_steps_and_bleeding_adds_more() {
        let rows = ((HEALTH_INNER - 2.0) / 2.0).floor();
        let mut seen = 0;
        for k in 0..400 {
            let t = k as f32 * 0.037;
            for i in 0..BUBBLES {
                if let Some((x, rise, a)) = bubble(i, t, 0.0) {
                    seen += 1;
                    assert!((0.0..=1.0).contains(&x));
                    assert!((0.0..=1.0).contains(&rise) && (0.0..=1.0).contains(&a));
                    // On a 2-point row, never between.
                    let r = rise * rows;
                    assert!((r - r.round()).abs() < 1e-4, "{rise}");
                }
            }
        }
        assert!(seen > 0);
        let calm = (0..BUBBLES)
            .filter(|&i| bubble(i, 1.0, 0.0).is_some())
            .count();
        let bleeding = (0..BUBBLES)
            .filter(|&i| bubble(i, 1.0, 0.5).is_some())
            .count();
        assert_eq!(calm, 3);
        assert_eq!(bleeding, BUBBLES);
    }
}
