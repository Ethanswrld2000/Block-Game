//! The world clock: 20-minute days, seasons from `assets/data/seasons.ron`.
//!
//! Time of day runs 0..1 from midnight to midnight (0.5 is noon). The clock is
//! saved to `saves/<world>/time.ron` with the autosave and on exit.
//! Hold T to fast-forward. `BLOCKGAME_TIME=hour` or `day:hour` (e.g. `21:18.5`)
//! starts at a given time, for screenshots (the clock stands still while
//! `BLOCKGAME_SCREENSHOT` is set).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::save::SaveDir;

/// Real seconds per in-game day.
pub const DAY_SECONDS: f32 = 20.0 * 60.0;
/// How much faster time runs while T is held.
const FAST_FORWARD: f32 = 120.0;

pub struct ClockPlugin;

impl Plugin for ClockPlugin {
    fn build(&self, app: &mut App) {
        let seasons = Seasons::from_ron(include_str!("../../../assets/data/seasons.ron")).unwrap_or_else(|e| panic!("{e}"));
        app.insert_resource(seasons)
            .insert_resource(SaveTimer(Timer::from_seconds(30.0, TimerMode::Repeating)))
            .add_systems(PreStartup, load_clock)
            .add_systems(First, advance)
            .add_systems(Update, save_clock)
            .add_systems(Last, save_clock_on_exit);
    }
}

#[derive(Resource, Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WorldClock {
    /// Whole days since the world began.
    pub day: u32,
    /// 0..1, midnight to midnight.
    pub time: f32,
    /// Multiplier on the clock speed (T held = fast forward).
    #[serde(skip)]
    pub speed: f32,
}

impl Default for WorldClock {
    fn default() -> Self {
        // A new world starts on a spring morning.
        Self { day: 0, time: 0.3, speed: 1.0 }
    }
}

impl WorldClock {
    /// Days since the world began, with the fraction of today.
    pub fn total_days(&self) -> f64 {
        self.day as f64 + self.time as f64
    }

    pub fn hour(&self) -> f32 {
        self.time * 24.0
    }
}

#[derive(Deserialize)]
struct SeasonsSpec {
    days: u32,
    seasons: Vec<SeasonSpec>,
}

#[derive(Deserialize)]
struct SeasonSpec {
    name: String,
    title: String,
    temperature: f32,
    daylight: f32,
    foliage: Foliage,
    weather: HashMap<String, f32>,
}

#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Foliage {
    pub color: (f32, f32, f32),
    pub amount: f32,
}

#[derive(Clone, Debug)]
pub struct Season {
    #[allow(dead_code)] // for data lookups by name later (festivals, crops)
    pub name: String,
    pub title: String,
    pub temperature: f32,
    pub daylight: f32,
    pub foliage: Foliage,
    /// Weather name -> relative chance.
    pub weather: HashMap<String, f32>,
}

#[derive(Resource, Clone, Debug)]
pub struct Seasons {
    pub days: u32,
    pub list: Vec<Season>,
}

/// Where in the year we are: the current season, the next one, and how far
/// to blend towards it (the last two days of a season fade into the next).
#[derive(Clone, Copy, Debug)]
pub struct SeasonPoint {
    pub current: usize,
    pub next: usize,
    pub day_in_season: u32,
    pub blend: f32,
}

impl Seasons {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let spec: SeasonsSpec = ron::from_str(text).map_err(|e| format!("seasons.ron: {e}"))?;
        if spec.seasons.is_empty() || spec.days == 0 {
            return Err("seasons.ron needs at least one season and days > 0".into());
        }
        let list = spec
            .seasons
            .into_iter()
            .map(|s| Season {
                name: s.name,
                title: s.title,
                temperature: s.temperature,
                daylight: s.daylight,
                foliage: s.foliage,
                weather: s.weather,
            })
            .collect();
        Ok(Self { days: spec.days, list })
    }

    pub fn at(&self, clock: &WorldClock) -> SeasonPoint {
        let n = self.list.len() as u32;
        let current = (clock.day / self.days) % n;
        let day_in_season = clock.day % self.days;
        let into = day_in_season as f32 + clock.time;
        let fade = 2.0f32.min(self.days as f32);
        let blend = ((into - (self.days as f32 - fade)) / fade).clamp(0.0, 1.0);
        SeasonPoint { current: current as usize, next: ((current + 1) % n) as usize, day_in_season, blend }
    }

    /// A season value blended towards the next season.
    pub fn blended(&self, p: SeasonPoint, f: impl Fn(&Season) -> f32) -> f32 {
        let (a, b) = (f(&self.list[p.current]), f(&self.list[p.next]));
        a + (b - a) * p.blend
    }
}

#[derive(Resource)]
struct SaveTimer(Timer);

fn load_clock(mut commands: Commands, dir: Res<SaveDir>) {
    let mut clock: WorldClock = std::fs::read_to_string(dir.file("time.ron"))
        .ok()
        .and_then(|t| ron::from_str(&t).ok())
        .unwrap_or_default();
    clock.speed = 1.0;
    clock.time = clock.time.rem_euclid(1.0);
    if let Ok(spec) = std::env::var("BLOCKGAME_TIME") {
        let (day, hour) = match spec.split_once(':') {
            Some((d, h)) => (d.trim().parse().ok(), h.trim().parse::<f32>().ok()),
            None => (None, spec.trim().parse::<f32>().ok()),
        };
        if let Some(d) = day {
            clock.day = d;
        }
        if let Some(h) = hour {
            clock.time = (h / 24.0).rem_euclid(1.0);
        }
    }
    commands.insert_resource(clock);
}

fn advance(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut clock: ResMut<WorldClock>, mut frozen: Local<Option<bool>>) {
    // Test screenshots hold the clock so the picture shows the time asked for.
    if *frozen.get_or_insert_with(|| std::env::var_os("BLOCKGAME_SCREENSHOT").is_some()) {
        return;
    }
    clock.speed = if keys.pressed(KeyCode::KeyT) { FAST_FORWARD } else { 1.0 };
    clock.time += time.delta_secs() * clock.speed / DAY_SECONDS;
    while clock.time >= 1.0 {
        clock.time -= 1.0;
        clock.day += 1;
    }
}

fn store(clock: &WorldClock, dir: &SaveDir) {
    let text = format!("(day: {}, time: {:.5})\n", clock.day, clock.time);
    if let Err(e) = std::fs::write(dir.file("time.ron"), text) {
        warn!("could not save the clock: {e}");
    }
}

fn save_clock(time: Res<Time>, mut timer: ResMut<SaveTimer>, clock: Res<WorldClock>, dir: Res<SaveDir>) {
    if timer.0.tick(time.delta()).just_finished() {
        store(&clock, &dir);
    }
}

fn save_clock_on_exit(mut exit: MessageReader<AppExit>, clock: Res<WorldClock>, dir: Res<SaveDir>) {
    if exit.read().next().is_some() {
        store(&clock, &dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seasons() -> Seasons {
        Seasons::from_ron(include_str!("../../../assets/data/seasons.ron")).unwrap()
    }

    #[test]
    fn seasons_file_parses() {
        let s = seasons();
        assert_eq!(s.list.len(), 4);
        assert_eq!(s.days, 10);
    }

    #[test]
    fn season_turns_every_ten_days_and_wraps() {
        let s = seasons();
        let at = |day| s.at(&WorldClock { day, time: 0.5, speed: 1.0 });
        assert_eq!(at(0).current, 0);
        assert_eq!(at(9).current, 0);
        assert_eq!(at(10).current, 1);
        assert_eq!(at(39).current, 3);
        assert_eq!(at(40).current, 0);
        assert_eq!(at(3).blend, 0.0);
        assert!(at(9).blend > 0.7);
    }
}
