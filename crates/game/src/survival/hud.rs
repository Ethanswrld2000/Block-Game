//! Survival HUD: the four health bars (bottom left), food, the air around you,
//! felt temperature and load (bottom right), short messages, the sleep fade
//! and the death screen. The air and temperature meters are always shown and
//! shake harder the worse the exposure stage you're in.

use bevy::prelude::*;

use super::camp::{Camp, Sleep};
use super::health::{Layer, MAX, MORTAL_AT};
use super::exposure::{Exposure, Scale};
use super::vitals::{self, CAPACITY, HEAVY_ABOVE, LIGHT_BELOW};
use super::{Survival, Toast};
use crate::player::Player;
use crate::theme::{self, colors, sizes, Theme};

pub struct SurvivalHudPlugin;

impl Plugin for SurvivalHudPlugin {
    fn build(&self, app: &mut App) {
        // PostStartup: after the weather panel, so this stacks below it top right.
        app.add_systems(crate::menu::GamePostStartup, spawn).add_systems(crate::menu::GameUpdate, (update_bars, update_meters, shake, temperature_list, toasts, overlays).after(super::SurvivalSet));
    }
}

const BAR_W: f32 = 170.0;
const BAR_H: f32 = 14.0;
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
fn stage_zones(b: &mut ChildSpawnerCommands, rules: &Exposure, scale: Scale, lo: f32, hi: f32, edge: f32) {
    let at = |v: f32| ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    let stages = rules.stages(scale);
    let low_side = scale != Scale::Heat;
    let first = stages.first().map_or(edge, |s| s.at);
    if low_side {
        b.spawn(zone(at(first), at(edge), COMFORT));
    }
    for (i, s) in stages.iter().enumerate() {
        let end = if low_side { stages.get(i + 1).map_or(lo, |n| n.at) } else { stages.get(i + 1).map_or(hi, |n| n.at) };
        let (a, z) = if low_side { (at(end), at(s.at)) } else { (at(s.at), at(end)) };
        b.spawn(zone(a, z, stage_color(scale, i)));
    }
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

fn spawn(mut commands: Commands, theme: Res<Theme>, rules: Res<Exposure>, top_right: Res<crate::hud::TopRight>) {
    let theme = &*theme;
    let rules = &*rules;
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
            col.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                row.spawn(theme.icon("icon_food"));
                row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                    b.spawn(bar_inner()).with_children(|b| {
                        b.spawn((FoodFill, fill(Color::srgb(0.9, 0.62, 0.3))));
                    });
                });
                row.spawn((text("Food", sizes::SMALL, colors::TEXT), Node { width: Val::Px(VALUE_W), ..default() }));
            });
            // Air: how much oxygen the air here holds, with its stages.
            col.spawn((Shake(Gauge::Air), Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() })).with_children(|row| {
                row.spawn(theme.icon("icon_air"));
                row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                    b.spawn(bar_inner()).with_children(|b| {
                        stage_zones(b, rules, Scale::Air, 0.0, 100.0, 100.0);
                        b.spawn(marker(Gauge::Air));
                    });
                });
                row.spawn((GaugeText(Gauge::Air), text("", sizes::SMALL, colors::TEXT), Node { width: Val::Px(VALUE_W), ..default() }));
            });
            // Felt temperature: cold stages, comfortable, heat stages.
            col.spawn((Shake(Gauge::Temperature), Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() })).with_children(|row| {
                row.spawn(theme.icon("icon_warmth"));
                row.spawn(bar_back(theme, METER_W)).with_children(|b| {
                    b.spawn(bar_inner()).with_children(|b| {
                        let (cold, hot) = rules.comfort();
                        stage_zones(b, rules, Scale::Cold, TEMP_LOW, TEMP_HIGH, hot);
                        stage_zones(b, rules, Scale::Heat, TEMP_LOW, TEMP_HIGH, cold);
                        b.spawn(marker(Gauge::Temperature));
                    });
                });
                row.spawn((GaugeText(Gauge::Temperature), text("", sizes::SMALL, colors::TEXT), Node { width: Val::Px(VALUE_W), ..default() }));
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
                col.spawn((TempLine(i), Node { justify_content: JustifyContent::SpaceBetween, column_gap: Val::Px(16.0), display: Display::None, ..default() })).with_children(|row| {
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
            Gauge::Temperature => ((v.feels - TEMP_LOW) / (TEMP_HIGH - TEMP_LOW), temp.is_some_and(|t| t.1.level > 0)),
            Gauge::Air => (v.air / 100.0, air.is_some_and(|a| a.level > 0)),
            Gauge::Load => (load / LOAD_SCALE, load > HEAVY_ABOVE),
        };
        node.left = Val::Percent(x.clamp(0.0, 1.0) * 100.0 - 0.9);
        bg.0 = if warn { Color::srgb(1.0, 0.35, 0.3) } else { Color::WHITE };
    }
    for (g, mut t, mut color) in &mut texts {
        let (s2, c) = match g.0 {
            Gauge::Temperature => match temp {
                Some((_, a)) => (format!("{} {:.0}°C", a.stage.name, v.feels), if a.level > 0 { colors::BAD } else { colors::TEXT }),
                None => (format!("{:.0}°C", v.feels), colors::TEXT),
            },
            Gauge::Air => {
                let c = if air.is_some_and(|a| a.level > 0) { colors::BAD } else { colors::TEXT };
                if s.env.underwater {
                    if v.breath > 0.0 { (format!("Breath {:.0}s", v.breath.ceil()), colors::TEXT) } else { ("No air!".to_string(), colors::BAD) }
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
fn shake(time: Res<Time>, s: Res<Survival>, rules: Res<Exposure>, mut rows: Query<(&Shake, &mut Node)>) {
    let v = &s.vitals;
    let t = time.elapsed_secs();
    for (sh, mut node) in &mut rows {
        let (strength, seed) = match sh.0 {
            Gauge::Temperature => (rules.temperature(v.feels).map_or(0.0, |(_, a)| a.stage.shake), 0.0),
            Gauge::Air => (rules.stage(Scale::Air, v.oxygen).map_or(0.0, |a| a.stage.shake), 7.3),
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
    mut values: Query<(&TempValue, &mut Text, &mut TextColor), (Without<TempHeader>, Without<TempLabel>)>,
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
    let parts: Vec<&(String, f32)> = s.temperature.iter().enumerate().filter(|(i, p)| *i == 0 || p.1.abs() >= 0.05).map(|(_, p)| p).collect();
    if let Ok(mut t) = header.single_mut() {
        let h = format!("Surroundings {:.1}°C, you feel {:.1}°C", s.env.feels, s.vitals.feels);
        if t.0 != h {
            t.0 = h;
        }
    }
    for (l, mut node) in &mut lines {
        let d = if l.0 < parts.len() { Display::Flex } else { Display::None };
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
            let x = format!("{:+.1}°C", p.1);
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
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn overlays(
    s: Res<Survival>,
    rules: Res<Exposure>,
    sleep: Res<Sleep>,
    camp: Res<Camp>,
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
        *v = if dead { Visibility::Inherited } else { Visibility::Hidden };
    }
    if dead && let Ok(mut t) = hint.single_mut() {
        let wait = s.dead_for.unwrap_or(0.0) < 2.0;
        let place = if camp.bedroll.is_some() { "at your camp" } else { "at the edge of the swamp" };
        t.0 = if wait { String::new() } else { format!("Press Space to wake {place}") };
    }
}
