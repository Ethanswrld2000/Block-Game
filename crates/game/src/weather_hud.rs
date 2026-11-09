//! The weather panel (first in `hud::TopRight`): what the weather is now, what comes next and
//! how long until it does, with a bar for how much of this weather is left.
//! In the last half minute the line turns gold while the clouds roll over.

use bevy::prelude::*;

use crate::clock::WorldClock;
use crate::theme::{self, colors, sizes, Theme};
use crate::weather::{Conditions, WeatherKinds, WeatherState};

pub struct WeatherHudPlugin;

impl Plugin for WeatherHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn).add_systems(Update, update.after(crate::weather::WeatherSet));
    }
}

const BAR_W: f32 = 170.0;
const BAR_H: f32 = 14.0;

#[derive(Component)]
struct Title;

#[derive(Component)]
struct Next;

#[derive(Component)]
struct Left;

fn spawn(mut commands: Commands, theme: Res<Theme>, top_right: Res<crate::hud::TopRight>) {
    commands
        .spawn((
            Name::new("weather panel"),
            ChildOf(top_right.0),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
                align_items: AlignItems::Start,
                ..default()
            },
            theme.skin("panel_light"),
        ))
        .with_children(|col| {
            col.spawn((Title, Text::new(""), theme.bold(sizes::LABEL), TextColor(colors::HEADING)));
            col.spawn((Next, theme::text("", sizes::SMALL, colors::MUTED)));
            col.spawn((Node { width: Val::Px(BAR_W), height: Val::Px(BAR_H), padding: UiRect::all(Val::Px(3.0)), ..default() }, theme.skin("bar")))
                .with_children(|b| {
                    b.spawn((Left, Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgb(0.55, 0.72, 0.9))));
                });
        });
}

/// `m:ss`, or `h:mm:ss` past an hour.
pub fn clock_text(seconds: f32) -> String {
    let s = seconds.max(0.0).ceil() as u32;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// The weather's name, saying snow where rain falls below freezing.
pub fn title(name: &str, title: &str, snowing: bool, precipitation: f32) -> String {
    if !snowing || precipitation <= 0.0 {
        return title.to_string();
    }
    match name {
        "rain" => "Snow".into(),
        "storm" => "Snowstorm".into(),
        _ => format!("{title} (snow)"),
    }
}

#[allow(clippy::type_complexity)]
fn update(
    clock: Res<WorldClock>,
    state: Res<WeatherState>,
    kinds: Res<WeatherKinds>,
    c: Res<Conditions>,
    mut title_q: Query<&mut Text, (With<Title>, Without<Next>)>,
    mut next_q: Query<(&mut Text, &mut TextColor), (With<Next>, Without<Title>)>,
    mut left_q: Query<&mut Node, With<Left>>,
) {
    let now = clock.total_days();
    let k = &kinds.list[state.current];
    let name = title(&k.name, &k.title, c.snowing, k.precipitation);
    for mut t in &mut title_q {
        if t.0 != name {
            t.0 = name.clone();
        }
    }
    let (line, color, share) = match (state.next, state.seconds_left(now), state.seconds_total()) {
        (Some(n), Some(left), Some(total)) => {
            let next = &kinds.list[n];
            let next_name = title(&next.name, &next.title, c.snowing, next.precipitation);
            if state.cloud_lead(now) > 0.0 || left <= crate::weather::CLOUD_LEAD_SECONDS {
                (format!("{next_name} rolling in  {}", clock_text(left)), colors::GOLD, left / total.max(1.0))
            } else {
                (format!("{next_name} in {}  (of {})", clock_text(left), clock_text(total)), colors::MUTED, left / total.max(1.0))
            }
        }
        _ => ("holding (F4 to change)".to_string(), colors::FAINT, 1.0),
    };
    for (mut t, mut col) in &mut next_q {
        if t.0 != line {
            t.0 = line.clone();
        }
        if col.0 != color {
            col.0 = color;
        }
    }
    for mut node in &mut left_q {
        let w = Val::Percent(share.clamp(0.0, 1.0) * 100.0);
        if node.width != w {
            node.width = w;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_reads_like_a_clock() {
        assert_eq!(clock_text(0.0), "0:00");
        assert_eq!(clock_text(59.2), "1:00");
        assert_eq!(clock_text(605.0), "10:05");
        assert_eq!(clock_text(3725.0), "1:02:05");
    }

    #[test]
    fn rain_below_freezing_is_snow() {
        assert_eq!(title("rain", "Rain", true, 0.7), "Snow");
        assert_eq!(title("rain", "Rain", false, 0.7), "Rain");
        assert_eq!(title("clear", "Clear", true, 0.0), "Clear");
        assert_eq!(title("gale", "Gale", true, 0.2), "Gale (snow)");
    }
}
