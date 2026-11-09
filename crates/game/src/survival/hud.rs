//! Survival HUD: the four health bars (bottom left), food, air, warmth and
//! load (bottom right), short messages, the sleep fade and the death screen.

use bevy::prelude::*;

use super::camp::{Camp, Sleep};
use super::health::{Layer, MAX, MORTAL_AT};
use super::vitals::{self, CAPACITY, COLD_BELOW, HEAVY_ABOVE, HOT_ABOVE, LIGHT_BELOW};
use super::{Survival, Toast};
use crate::player::Player;
use crate::theme::{self, colors, sizes, Theme};

pub struct SurvivalHudPlugin;

impl Plugin for SurvivalHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn).add_systems(Update, (update_bars, update_meters, toasts, overlays).after(super::SurvivalSet));
    }
}

const BAR_W: f32 = 170.0;
const BAR_H: f32 = 14.0;
/// Food, air, warmth and load bars, and the width of the value after them.
const METER_W: f32 = 130.0;
const VALUE_W: f32 = 92.0;
/// Load gauge runs from 0 to this share of capacity.
const LOAD_SCALE: f32 = 1.2;

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

#[derive(Component)]
struct BarNote(Layer);

#[derive(Component, Clone, Copy, PartialEq)]
enum Meter {
    Food,
    Air,
}

#[derive(Component)]
struct MeterFill(Meter);

#[derive(Component)]
struct MeterRow(Meter);

#[derive(Component)]
struct Marker(Gauge);

#[derive(Component)]
struct GaugeText(Gauge);

#[derive(Clone, Copy, PartialEq)]
enum Gauge {
    Warmth,
    Load,
}

#[derive(Component)]
struct ToastText;

#[derive(Component)]
struct CampText;

#[derive(Component)]
struct Dizzy;

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
    (Node { width: Val::Px(w), height: Val::Px(BAR_H), padding: UiRect::all(Val::Px(3.0)), ..default() }, theme.skin("bar"))
}

/// The bars' contents go in here, so zones and markers line up with the fill.
fn bar_inner() -> Node {
    Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }
}

fn hud_panel(theme: &Theme) -> impl Bundle {
    theme.skin("panel_light")
}

/// The toast's frame, hidden while there's nothing to say.
#[derive(Component)]
struct ToastBox;

fn fill(color: Color) -> impl Bundle {
    (Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(color))
}

fn tick(at: f32) -> impl Bundle {
    (
        Node { position_type: PositionType::Absolute, left: Val::Percent(at * 100.0), width: Val::Px(1.0), height: Val::Percent(100.0), ..default() },
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

fn marker(g: Gauge) -> impl Bundle {
    (
        Marker(g),
        Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), width: Val::Px(3.0), top: Val::Px(-4.0), height: Val::Px(BAR_H + 2.0), ..default() },
        BackgroundColor(Color::WHITE),
    )
}

fn full_screen(color: Color) -> impl Bundle {
    (
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        BackgroundColor(color),
        Pickable::IGNORE,
    )
}

fn spawn(mut commands: Commands, theme: Res<Theme>) {
    let theme = &*theme;
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
                col.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                    row.spawn(theme.icon(match l {
                        Layer::Damage => "icon_damage",
                        Layer::Fatigue => "icon_fatigue",
                        Layer::Blood => "icon_blood",
                        Layer::Injuries => "icon_injuries",
                    }));
                    row.spawn((text(l.name(), sizes::SMALL, colors::TEXT), Node { width: Val::Px(70.0), ..default() }));
                    row.spawn(bar_back(theme, BAR_W)).with_children(|b| {
                        b.spawn(bar_inner()).with_children(|b| {
                            b.spawn((BarFill(l), fill(layer_color(l))));
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
            col.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, width: Val::Percent(100.0), ..default() }).with_children(|row| {
                row.spawn(theme.icon("icon_camp"));
                row.spawn((CampText, text("", sizes::SMALL, colors::MUTED)));
            });
            for (m, name, color, icon) in [(Meter::Food, "Food", Color::srgb(0.9, 0.62, 0.3), "icon_food"), (Meter::Air, "Air", Color::srgb(0.6, 0.85, 1.0), "icon_air")] {
                col.spawn((MeterRow(m), Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() })).with_children(|row| {
                    row.spawn(theme.icon(icon));
                    row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                        b.spawn(bar_inner()).with_children(|b| {
                            b.spawn((MeterFill(m), fill(color)));
                        });
                    });
                    row.spawn((text(name, sizes::SMALL, colors::TEXT), Node { width: Val::Px(VALUE_W), ..default() }));
                });
            }
            // Warmth: cold, comfortable and hot zones with a marker.
            col.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                row.spawn(theme.icon("icon_warmth"));
                row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                    let mut b = b.spawn(bar_inner());
                    b.with_children(|b| {
                    b.spawn(zone(0.0, COLD_BELOW / 100.0, Color::srgba(0.3, 0.55, 1.0, 0.6)));
                    b.spawn(zone(COLD_BELOW / 100.0, HOT_ABOVE / 100.0, Color::srgba(0.4, 0.8, 0.4, 0.35)));
                    b.spawn(zone(HOT_ABOVE / 100.0, 1.0, Color::srgba(1.0, 0.4, 0.2, 0.6)));
                    b.spawn(marker(Gauge::Warmth));
                    });
                });
                row.spawn((GaugeText(Gauge::Warmth), text("", sizes::SMALL, colors::TEXT), Node { width: Val::Px(VALUE_W), ..default() }));
            });
            // Load: light, the sweet spot, heavy.
            col.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                row.spawn(theme.icon("icon_load"));
                row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                    let mut b = b.spawn(bar_inner());
                    b.with_children(|b| {
                    b.spawn(zone(0.0, LIGHT_BELOW / LOAD_SCALE, Color::srgba(0.5, 0.75, 1.0, 0.45)));
                    b.spawn(zone(LIGHT_BELOW / LOAD_SCALE, HEAVY_ABOVE / LOAD_SCALE, Color::srgba(0.4, 0.85, 0.4, 0.55)));
                    b.spawn(zone(HEAVY_ABOVE / LOAD_SCALE, 1.0, Color::srgba(1.0, 0.55, 0.2, 0.55)));
                    b.spawn(tick(1.0 / LOAD_SCALE));
                    b.spawn(marker(Gauge::Load));
                    });
                });
                row.spawn((GaugeText(Gauge::Load), text("", sizes::SMALL, colors::TEXT), Node { width: Val::Px(VALUE_W), ..default() }));
            });
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
            c.spawn((ToastBox, Node { padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)), ..default() }, theme.skin("tooltip"), Visibility::Hidden)).with_children(|b| {
                b.spawn((ToastText, text("", sizes::LABEL, colors::TEXT)));
            });
        });

    commands.spawn((SleepFade, full_screen(Color::srgba(0.0, 0.0, 0.0, 0.0)), GlobalZIndex(5)));
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
            c.spawn((theme.bold(sizes::HUGE), Text::new("You perished"), TextColor(Color::srgb(1.0, 0.85, 0.8))));
            c.spawn((DeathHint, text("", sizes::LABEL, colors::TEXT)));
        });
}

fn update_bars(s: Res<Survival>, mut fills: Query<(&BarFill, &mut Node)>, mut notes: Query<(&BarNote, &mut Text)>) {
    let h = &s.vitals.health;
    let mortal = h.mortal();
    for (f, mut node) in &mut fills {
        node.width = Val::Percent(h.get(f.0) / MAX * 100.0);
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

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_meters(
    s: Res<Survival>,
    camp: Res<Camp>,
    player: Res<Player>,
    mut fills: Query<(&MeterFill, &mut Node), (Without<Marker>, Without<MeterRow>)>,
    mut rows: Query<(&MeterRow, &mut Node), (Without<MeterFill>, Without<Marker>)>,
    mut markers: Query<(&Marker, &mut Node, &mut BackgroundColor), (Without<MeterFill>, Without<MeterRow>)>,
    mut texts: Query<(&GaugeText, &mut Text), Without<CampText>>,
    mut camp_text: Query<&mut Text, (With<CampText>, Without<GaugeText>)>,
) {
    let v = &s.vitals;
    for (f, mut node) in &mut fills {
        let x = match f.0 {
            Meter::Food => v.satiety,
            Meter::Air => v.air,
        };
        node.width = Val::Percent(x.clamp(0.0, 100.0));
    }
    for (r, mut node) in &mut rows {
        // The air meter only shows when you're short of it.
        if r.0 == Meter::Air {
            let d = if v.air < 99.5 { Display::Flex } else { Display::None };
            if node.display != d {
                node.display = d;
            }
        }
    }
    let load = vitals::load(s.env.carried);
    for (m, mut node, mut bg) in &mut markers {
        let (x, warn) = match m.0 {
            Gauge::Warmth => (v.warmth / 100.0, v.too_cold() || v.too_hot()),
            Gauge::Load => (load / LOAD_SCALE, load > HEAVY_ABOVE),
        };
        node.left = Val::Percent(x.clamp(0.0, 1.0) * 100.0 - 0.9);
        bg.0 = if warn { Color::srgb(1.0, 0.35, 0.3) } else { Color::WHITE };
    }
    for (g, mut t) in &mut texts {
        let s2 = match g.0 {
            Gauge::Warmth => {
                let state = if v.too_cold() {
                    "freezing "
                } else if v.too_hot() {
                    "too hot "
                } else {
                    ""
                };
                format!("{state}{:.0}°C", s.env.feels)
            }
            Gauge::Load => {
                let pct = (vitals::weight_speed(s.env.carried) - 1.0) * 100.0;
                if pct.abs() >= 0.5 {
                    format!("{:.0}/{:.0} kg {pct:+.0}%", s.env.carried, CAPACITY)
                } else {
                    format!("{:.0}/{:.0} kg", s.env.carried, CAPACITY)
                }
            }
        };
        if t.0 != s2 {
            t.0 = s2;
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
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

fn overlays(
    s: Res<Survival>,
    sleep: Res<Sleep>,
    camp: Res<Camp>,
    mut dizzy: Query<&mut BackgroundColor, (With<Dizzy>, Without<SleepFade>)>,
    mut fade: Query<&mut BackgroundColor, (With<SleepFade>, Without<Dizzy>)>,
    mut death: Query<&mut Visibility, With<DeathScreen>>,
    mut hint: Query<&mut Text, With<DeathHint>>,
) {
    if let Ok(mut bg) = dizzy.single_mut() {
        bg.0.set_alpha(s.vitals.dizzy() * 0.55);
    }
    if let Ok(mut bg) = fade.single_mut() {
        bg.0.set_alpha(sleep.darkness());
    }
    let dead = s.vitals.health.is_dead();
    if let Ok(mut v) = death.single_mut() {
        *v = if dead { Visibility::Inherited } else { Visibility::Hidden };
    }
    if dead && let Ok(mut t) = hint.single_mut() {
        let wait = s.dead_for.unwrap_or(0.0) < 2.0;
        let place = if camp.bedroll.is_some() { "at your camp" } else { "at the edge of the swamp" };
        t.0 = if wait { String::new() } else { format!("Press Space to wake {place}") };
    }
}
