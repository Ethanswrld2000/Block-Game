//! Named places and their title cards: walking into a village, down into a
//! barrow, or across into a new region puts its name on screen for a few
//! seconds.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::clock::{Seasons, WorldClock};
use crate::player::Player;
use crate::world::Generator;

pub struct PlacesPlugin;

impl Plugin for PlacesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Here>()
            .init_resource::<Card>()
            .add_message::<ShowCard>()
            .add_systems(Startup, spawn_card)
            .add_systems(Update, (notice, show, fade).chain());
    }
}

/// Seconds a card takes to fade in, stays, and fades out.
const FADE_IN: f32 = 0.8;
const HOLD: f32 = 3.5;
const FADE_OUT: f32 = 1.4;
/// A region must stay the same this long before its card shows.
const SETTLE: f32 = 3.0;
/// The same place doesn't announce itself again within this many seconds.
const AGAIN: f64 = 90.0;

/// Put a title card on screen.
#[derive(Message, Debug, Clone)]
pub struct ShowCard {
    pub title: String,
    pub subtitle: String,
}

#[derive(Debug, Clone, PartialEq)]
enum Place {
    Village(u64),
    Barrow(u64),
    Wild,
}

/// Where you are, and what was announced when.
#[derive(Resource, Default)]
struct Here {
    place: Option<Place>,
    biome: Option<String>,
    /// The biome you're standing in and since when.
    seen_biome: Option<(String, f32)>,
    shown: HashMap<String, f64>,
    tick: f32,
}

#[derive(Resource, Default)]
pub struct Card {
    /// Seconds since the card appeared; `None` when there is none.
    age: Option<f32>,
}

impl Card {
    /// A title card is on screen (the debug overlay steps aside).
    pub fn up(&self) -> bool {
        self.age.is_some()
    }
}

#[derive(Component)]
struct CardRoot;
#[derive(Component)]
struct CardTitle;
#[derive(Component)]
struct CardSub;
#[derive(Component)]
struct CardRule;

fn spawn_card(mut commands: Commands) {
    commands
        .spawn((
            CardRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(9.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(8.0),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(5),
        ))
        .with_children(|c| {
            c.spawn((CardTitle, Text::new(""), TextFont { font_size: bevy::text::FontSize::Px(46.0), ..default() }, TextColor(Color::NONE), TextShadow { offset: Vec2::splat(2.0), color: Color::NONE }));
            c.spawn((CardRule, Node { width: Val::Px(220.0), height: Val::Px(2.0), ..default() }, BackgroundColor(Color::NONE)));
            c.spawn((CardSub, Text::new(""), TextFont { font_size: bevy::text::FontSize::Px(20.0), ..default() }, TextColor(Color::NONE), TextShadow { offset: Vec2::splat(1.5), color: Color::NONE }));
        });
}

fn lower_the(title: &str) -> String {
    match title.strip_prefix("The ") {
        Some(rest) => format!("the {rest}"),
        None => title.to_string(),
    }
}

/// Work out where you are and announce new places.
#[allow(clippy::too_many_arguments)]
fn notice(
    time: Res<Time>,
    generator: Option<Res<Generator>>,
    player: Res<Player>,
    mode: Res<crate::player::MoveMode>,
    clock: Res<WorldClock>,
    seasons: Res<Seasons>,
    mut here: ResMut<Here>,
    mut cards: MessageWriter<ShowCard>,
    (mut progress, prog, mut gain): (ResMut<crate::progress::Progress>, Res<crate::progress::xp::Progression>, MessageWriter<crate::progress::GainXp>),
) {
    let Some(generator) = generator else { return };
    let wg = &generator.0;
    here.tick -= time.delta_secs();
    if here.tick > 0.0 || !(player.ready || *mode == crate::player::MoveMode::Fly) {
        return;
    }
    here.tick = 0.4;
    let now = time.elapsed_secs_f64();
    let p = player.pos.floor().as_ivec3();
    let (x, y, z) = (p.x, p.y, p.z);
    let biome = wg.biome_at(x, y, z);
    let barrow = wg.barrows_in(x, z, x, z).into_iter().find(|b| b.contains(x, y, z));
    let village = wg.villages_in(x, z, x, z).into_iter().find(|v| {
        let (dx, dz) = (x - v.center[0], z - v.center[2]);
        dx * dx + dz * dz <= v.radius * v.radius && (y - v.center[1]).abs() < 24
    });
    let (place, card) = if let Some(b) = barrow {
        (Place::Barrow(b.id), Some((format!("barrow:{}", b.id), b.name.clone(), "A barrow of the old dead".to_string())))
    } else if let Some(v) = village {
        (Place::Village(v.id), Some((format!("village:{}", v.id), v.name.clone(), format!("A village of {}", lower_the(&biome.title)))))
    } else {
        (Place::Wild, None)
    };
    if here.place.as_ref() != Some(&place) {
        here.place = Some(place.clone());
        if let Some((key, title, subtitle)) = card
            && here.shown.get(&key).is_none_or(|t| now - t > AGAIN)
        {
            if progress.found.insert(key.clone()) {
                let xp = if key.starts_with("barrow") { prog.xp.barrow } else { prog.xp.village };
                gain.write(crate::progress::GainXp { xp, why: format!("Found {title}") });
            }
            here.shown.insert(key, now);
            cards.write(ShowCard { title, subtitle });
            // Arriving in a village counts as arriving in its region too.
            here.biome = Some(biome.name.clone());
        }
    }
    // Regions announce themselves once you've been in one a little while.
    let t = time.elapsed_secs();
    match &here.seen_biome {
        Some((name, _)) if *name == biome.name => {}
        _ => here.seen_biome = Some((biome.name.clone(), t)),
    }
    if place == Place::Wild
        && let Some((name, since)) = here.seen_biome.clone()
        && t - since > SETTLE
        && here.biome.as_ref() != Some(&name)
    {
        here.biome = Some(name.clone());
        let key = format!("biome:{name}");
        if here.shown.get(&key).is_none_or(|t| now - t > AGAIN) {
            if progress.found.insert(key.clone()) && progress.found.len() > 1 {
                // (The region you start in isn't a find.)
                gain.write(crate::progress::GainXp { xp: prog.xp.region, why: format!("Reached {}", biome.title) });
            }
            here.shown.insert(key, now);
            let sp = seasons.at(&clock);
            let season = &seasons.list[sp.current].title;
            cards.write(ShowCard { title: biome.title.clone(), subtitle: format!("{season}, day {}", sp.day_in_season + 1) });
        }
    }
}

fn show(
    mut msgs: MessageReader<ShowCard>,
    mut card: ResMut<Card>,
    mut root: Query<&mut Visibility, With<CardRoot>>,
    mut title: Query<&mut Text, (With<CardTitle>, Without<CardSub>)>,
    mut sub: Query<&mut Text, (With<CardSub>, Without<CardTitle>)>,
) {
    let Some(m) = msgs.read().last() else { return };
    info!("title card: {} ({})", m.title, m.subtitle);
    card.age = Some(0.0);
    for mut v in &mut root {
        *v = Visibility::Inherited;
    }
    for mut t in &mut title {
        t.0 = m.title.clone();
    }
    for mut t in &mut sub {
        t.0 = m.subtitle.clone();
    }
}

#[allow(clippy::type_complexity)]
fn fade(
    time: Res<Time>,
    mut card: ResMut<Card>,
    mut root: Query<&mut Visibility, With<CardRoot>>,
    mut texts: Query<(&mut TextColor, &mut TextShadow, Has<CardTitle>)>,
    mut rule: Query<&mut BackgroundColor, With<CardRule>>,
) {
    let Some(age) = card.age.as_mut() else { return };
    *age += time.delta_secs();
    // Screenshot runs keep the card up so it can be seen.
    if std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() {
        *age = age.min(FADE_IN + HOLD * 0.5);
    }
    let a = *age;
    let alpha = if a < FADE_IN {
        a / FADE_IN
    } else if a < FADE_IN + HOLD {
        1.0
    } else {
        1.0 - (a - FADE_IN - HOLD) / FADE_OUT
    };
    if alpha <= 0.0 {
        card.age = None;
        for mut v in &mut root {
            *v = Visibility::Hidden;
        }
        return;
    }
    for (mut color, mut shadow, is_title) in &mut texts {
        color.0 = if is_title { Color::srgba(0.98, 0.94, 0.82, alpha) } else { Color::srgba(0.85, 0.85, 0.8, alpha * 0.9) };
        shadow.color = Color::srgba(0.0, 0.0, 0.0, 0.8 * alpha);
    }
    for mut bg in &mut rule {
        bg.0 = Color::srgba(0.95, 0.9, 0.75, 0.6 * alpha);
    }
}
