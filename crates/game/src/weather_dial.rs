//! The dial at the top of the screen. A round sky disc in the middle turns
//! with the time of day (sun at noon, moon at midnight, stars at night), and
//! the ring round it is a weather forecast for the next day: each weather
//! gets its own colour, pattern and picture. The gold pointer at the top is
//! now; what's coming sits clockwise from it and turns up towards the pointer.
//! Drawn by a UI shader (`assets/shaders/weather_dial.wesl`).
//!
//! Storm warning: a minute before severe weather (`severe`) the line under
//! the dial says what's coming and counts down, the ring's rim pulses red, and
//! Bram, if he's nearby, says so.

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::ui_render::prelude::{MaterialNode, UiMaterial};

use crate::clock::{Seasons, WorldClock};
use crate::theme::{self, colors, sizes};
use crate::weather::{
    Conditions, WeatherKind, WeatherKinds, WeatherState, pick_weather, weather_days,
};

pub struct WeatherDialPlugin;

impl Plugin for WeatherDialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy::ui_render::UiMaterialPlugin::<DialMaterial>::default())
            .add_systems(crate::menu::GameStartup, spawn)
            .add_systems(
                crate::menu::GameUpdate,
                (update_dial, warn).after(crate::weather::WeatherSet),
            );
    }
}

/// Size of the dial in points.
pub const DIAL: f32 = 112.0;
/// Space the dial and its warning line take at the top of the screen; other
/// things along the top (the waypoint strip) go below it.
pub const DIAL_SPACE: f32 = 146.0;
/// Real seconds before severe weather that the warning shows.
pub const WARN_SECONDS: f32 = 60.0;
/// How far ahead the ring looks, in days (one full turn).
const AHEAD_DAYS: f64 = 1.0;
/// Most weathers shown on the ring at once (matches the shader).
const SEGMENTS: usize = 6;

/// Severe weather gets a warning: thunder, gales, deep cold, downpours.
pub fn severe(k: &WeatherKind) -> bool {
    k.lightning || k.wind >= 12.0 || k.chill <= -10.0 || k.precipitation >= 1.0
}

/// The weather from now on: `(end, kind)` pairs, `end` in total days, until
/// `now + days`. Worked out the same way `weather::change_weather` picks them,
/// so it is what will come (unless F4 changes it). A held weather is one span.
pub fn forecast(
    kinds: &WeatherKinds,
    seasons: &Seasons,
    seed: u64,
    state: &WeatherState,
    now: f64,
    days: f64,
) -> Vec<(f64, usize)> {
    let horizon = now + days;
    let (Some(mut until), Some(mut next)) = (state.until, state.next) else {
        return vec![(horizon, state.current)];
    };
    let mut out = vec![(until, state.current)];
    while until < horizon && out.len() < SEGMENTS {
        let kind = next;
        let start = until;
        until = start + weather_days(&kinds.list[kind], seed, start);
        let later = WorldClock {
            day: until as u32,
            time: until.fract() as f32,
            speed: 1.0,
        };
        next = pick_weather(
            kinds,
            &seasons.list[seasons.at(&later).current],
            seed,
            until,
            kind,
        );
        out.push((until, kind));
    }
    out
}

/// The severe weather coming within `WARN_SECONDS`, and the seconds left.
pub fn warning(kinds: &WeatherKinds, state: &WeatherState, now: f64) -> Option<(usize, f32)> {
    let next = state.next?;
    let left = state.seconds_left(now)?;
    (left <= WARN_SECONDS && severe(&kinds.list[next]) && !severe(&kinds.list[state.current]))
        .then_some((next, left))
}

/// Picture on the ring (matches `icon_bits` in the shader).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Sun = 0,
    Cloud = 1,
    Rain = 2,
    Snow = 3,
    Bolt = 4,
    Mist = 5,
    Wind = 6,
}

/// Pattern over the ring (matches the shader).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Clear = 0,
    Clouds = 1,
    Rain = 2,
    Snow = 3,
    Storm = 4,
    Mist = 5,
    Wind = 6,
    Frost = 7,
}

/// How a weather looks on the ring: colour (sRGB), pattern and picture. Worked
/// out from its numbers in weather.ron, so new kinds get a look of their own.
pub fn look(k: &WeatherKind, snowing: bool) -> ([f32; 3], Pattern, Icon) {
    let sky = [0.47, 0.72, 0.93];
    let grey = [0.55, 0.58, 0.64];
    let cover = (k.clouds * (1.0 - k.light * 0.6)).clamp(0.0, 1.0);
    let mut col = [0.0; 3];
    for i in 0..3 {
        col[i] = sky[i] + (grey[i] - sky[i]) * cover;
    }
    let dark = 1.0 - (1.0 - k.cloud_shade) * 0.8;
    let shade = |c: [f32; 3], d: f32| [c[0] * d, c[1] * d, c[2] * d];
    if k.lightning {
        return ([0.30, 0.24, 0.42], Pattern::Storm, Icon::Bolt);
    }
    if k.precipitation > 0.0 && k.wind < 15.0 {
        return if snowing {
            ([0.78, 0.84, 0.92], Pattern::Snow, Icon::Snow)
        } else {
            (
                shade([0.32, 0.46, 0.68], dark.max(0.7)),
                Pattern::Rain,
                Icon::Rain,
            )
        };
    }
    if k.wind >= 15.0 {
        return ([0.36, 0.56, 0.58], Pattern::Wind, Icon::Wind);
    }
    if k.chill <= -10.0 {
        return ([0.62, 0.86, 0.96], Pattern::Frost, Icon::Snow);
    }
    if k.visibility < 150.0 {
        return ([0.74, 0.76, 0.78], Pattern::Mist, Icon::Mist);
    }
    if k.clouds >= 0.5 {
        return (shade(col, dark), Pattern::Clouds, Icon::Cloud);
    }
    ([0.98, 0.80, 0.38], Pattern::Clear, Icon::Sun)
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct DialMaterial {
    #[uniform(0)]
    params: DialParams,
}

#[derive(Clone, Copy, Default, ShaderType)]
struct DialParams {
    /// Time of day (0..1), a clock in seconds, share of the day the sun is up, moon phase.
    look: Vec4,
    /// Warning pulse (0..1), lightning flash, -, -.
    state: Vec4,
    /// Per weather on the ring: where it ends (turns ahead, 0..1), pattern, picture, where its picture sits (-1: none).
    seg: [Vec4; SEGMENTS],
    /// Per weather: colour (linear).
    col: [Vec4; SEGMENTS],
}

impl UiMaterial for DialMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/weather_dial.wesl".into()
    }
}

#[derive(Component)]
struct Dial;

#[derive(Component)]
struct WarnLine;

fn spawn(mut commands: Commands, mut mats: ResMut<Assets<DialMaterial>>) {
    commands
        .spawn((
            Name::new("weather dial"),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(6.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                ..default()
            },
        ))
        .with_children(|c| {
            c.spawn((
                Dial,
                Node {
                    width: Val::Px(DIAL),
                    height: Val::Px(DIAL),
                    ..default()
                },
                MaterialNode(mats.add(DialMaterial {
                    params: DialParams::default(),
                })),
            ));
            c.spawn((
                WarnLine,
                theme::text("", sizes::SMALL, colors::BAD),
                Visibility::Hidden,
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn update_dial(
    time: Res<Time<Real>>,
    clock: Res<WorldClock>,
    seasons: Res<Seasons>,
    kinds: Res<WeatherKinds>,
    state: Res<WeatherState>,
    c: Res<Conditions>,
    generator: Option<Res<crate::world::Generator>>,
    dial: Query<&MaterialNode<DialMaterial>, With<Dial>>,
    mut mats: ResMut<Assets<DialMaterial>>,
) {
    let Ok(node) = dial.single() else { return };
    let Some(mut mat) = mats.get_mut(&node.0) else {
        return;
    };
    let now = clock.total_days();
    let seed = generator.map(|g| g.0.seed()).unwrap_or(1);
    let season = seasons.at(&clock);
    let daylight = seasons.blended(season, |s| s.daylight);
    let t = time.elapsed_secs();
    let p = &mut mat.params;
    p.look = Vec4::new(clock.time, t, daylight, crate::sky::moon_phase(&clock));
    let pulse = if warning(&kinds, &state, now).is_some() {
        0.5 + 0.5 * (t * 6.0).sin()
    } else {
        0.0
    };
    p.state = Vec4::new(pulse, c.flash, 0.0, 0.0);
    p.seg = [Vec4::new(2.0, 0.0, 0.0, -1.0); SEGMENTS];
    let mut start = 0.0;
    for (i, (end, kind)) in forecast(&kinds, &seasons, seed, &state, now, AHEAD_DAYS)
        .into_iter()
        .enumerate()
    {
        let end = ((end - now) / AHEAD_DAYS).clamp(0.0, 1.0) as f32;
        let (rgb, pattern, icon) = look(&kinds.list[kind], c.snowing);
        // A picture in the middle of the part that shows, if there is room for it.
        let mid = if end - start >= 0.07 {
            (start + end) * 0.5
        } else {
            -1.0
        };
        p.seg[i] = Vec4::new(
            if i + 1 == SEGMENTS { 2.0 } else { end },
            pattern as u32 as f32,
            icon as u32 as f32,
            mid,
        );
        p.col[i] = Color::srgb(rgb[0], rgb[1], rgb[2]).to_linear().to_vec4();
        start = end;
        if end >= 1.0 {
            p.seg[i].x = 2.0;
            break;
        }
    }
}

/// The warning line, and Bram's word when it starts.
#[allow(clippy::too_many_arguments)]
fn warn(
    time: Res<Time<Real>>,
    clock: Res<WorldClock>,
    kinds: Res<WeatherKinds>,
    state: Res<WeatherState>,
    c: Res<Conditions>,
    player: Res<crate::player::Player>,
    book: Res<crate::npc::NpcBook>,
    npcs: Query<(&crate::npc::Npc, &Transform)>,
    mut line: Query<(&mut Text, &mut TextColor, &mut Visibility), With<WarnLine>>,
    mut bark: MessageWriter<crate::lines::Bark>,
    mut was: Local<Option<usize>>,
) {
    let Ok((mut text, mut color, mut vis)) = line.single_mut() else {
        return;
    };
    let now = clock.total_days();
    let Some((next, left)) = warning(&kinds, &state, now) else {
        *was = None;
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
        return;
    };
    let k = &kinds.list[next];
    let name = crate::weather_hud::title(&k.name, &k.title, c.snowing, k.precipitation);
    let s = format!(
        "Storm warning: {name} in {}",
        crate::weather_hud::clock_text(left)
    );
    if text.0 != s {
        text.0 = s;
    }
    let a = 0.65 + 0.35 * (time.elapsed_secs() * 6.0).sin().abs();
    color.0 = colors::BAD.with_alpha(a);
    if *vis != Visibility::Inherited {
        *vis = Visibility::Inherited;
    }
    if *was != Some(next) {
        *was = Some(next);
        let bram = npcs.iter().find(|(n, tf)| {
            book.kinds.get(n.kind).is_some_and(|k| k.dialogue == "bram")
                && tf.translation.distance(player.pos) <= 40.0
        });
        if let Some((_, tf)) = bram {
            bark.write(crate::lines::Bark {
                speaker: "bram".into(),
                trigger: "storm_coming".into(),
                at: Some(tf.translation + Vec3::Y),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::DAY_SECONDS;

    fn kinds() -> WeatherKinds {
        WeatherKinds::from_ron(include_str!("../../../assets/data/weather.ron")).unwrap()
    }

    #[test]
    fn storms_and_gales_are_severe_clear_is_not() {
        let k = kinds();
        let is = |n: &str| severe(&k.list[k.find(n).unwrap()]);
        assert!(is("storm") && is("gale") && is("cold_snap"));
        assert!(!is("clear") && !is("rain") && !is("overcast") && !is("mist"));
    }

    #[test]
    fn every_weather_gets_its_own_look() {
        let k = kinds();
        let looks: Vec<_> = k.list.iter().map(|w| look(w, false)).collect();
        for (i, a) in looks.iter().enumerate() {
            for b in &looks[i + 1..] {
                assert!(a.1 != b.1, "two weathers share the pattern {:?}", a.1);
            }
        }
        assert_eq!(look(&k.list[k.find("rain").unwrap()], true).2, Icon::Snow);
    }

    #[test]
    fn forecast_follows_the_weather_and_covers_a_day() {
        let k = kinds();
        let seasons = Seasons::from_ron(include_str!("../../../assets/data/seasons.ron")).unwrap();
        let mut state = WeatherState::new(0, false);
        state.since = 2.0;
        state.until = Some(2.1);
        state.next = Some(k.find("storm").unwrap());
        let f = forecast(&k, &seasons, 7, &state, 2.05, 1.0);
        assert_eq!(f[0], (2.1, 0));
        assert_eq!(f[1].1, state.next.unwrap());
        assert!(f.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 != w[1].1));
        assert!(f.last().unwrap().0 >= 3.05 || f.len() == SEGMENTS);
        // Warned a minute before the storm, not before.
        let minute = WARN_SECONDS as f64 / DAY_SECONDS as f64;
        assert!(warning(&k, &state, 2.1 - minute * 0.5).is_some());
        assert!(warning(&k, &state, 2.1 - minute * 1.5).is_none());
        // A held weather is one span.
        state.until = None;
        state.next = None;
        assert_eq!(
            forecast(&k, &seasons, 7, &state, 2.05, 1.0),
            vec![(3.05, 0)]
        );
    }
}
