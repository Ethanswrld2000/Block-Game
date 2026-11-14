//! Survival: layered health, food, the air around you, felt temperature,
//! carried weight, bedroll camping, sleep, death and respawn.
//!
//! The rules live in plain Rust (`health`, `vitals`, `food`) so a server can
//! run them later; the systems here gather the surroundings each frame and
//! feed them in.
//!
//! Debug keys: F7 is a stab (piercing), F8 a club blow (blunt). `BLOCKGAME_VITALS=damage,fatigue,blood,injuries[,food]`
//! sets the bars at start, and `BLOCKGAME_FEELS=-25` / `BLOCKGAME_OXYGEN=50` hold the
//! felt temperature (°C) and the air (% of normal) there, for screenshots.

pub mod camp;
pub mod exposure;
pub mod food;
pub mod health;
mod hud;
pub mod vitals;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::camera::FlyCamera;
use crate::clock::{Seasons, WorldClock};
use crate::interact::{HandAnim, Wielded};
use crate::inventory::Inventory;
use crate::items::{ItemKind, Items};
use crate::player::{Body, Damage, DamageCause, MoveMode, Player, PlayerSet};
use crate::save::SaveDir;
use crate::torches::Torches;
use crate::weather::{Conditions, LocalClimate, WeatherKinds, WeatherState};
use camp::Camp;
use exposure::{Exposure, Scale};
use food::FoodTiers;
use health::Layer;
use vitals::{Env, Vitals};

/// Seconds of holding right click to eat something.
const EAT_SECONDS: f32 = 1.2;
pub const BEDROLL_MASS: f32 = 3.0;

pub struct SurvivalPlugin;

impl Plugin for SurvivalPlugin {
    fn build(&self, app: &mut App) {
        let tiers = FoodTiers::from_ron(include_str!("../../../../assets/data/food_tiers.ron")).unwrap_or_else(|e| panic!("{e}"));
        let exposure = Exposure::from_ron(include_str!("../../../../assets/data/exposure.ron")).unwrap_or_else(|e| panic!("{e}"));
        app.insert_resource(tiers)
            .insert_resource(exposure)
            .init_resource::<Survival>()
            .init_resource::<Camp>()
            .add_message::<Toast>()
            .add_systems(crate::menu::GamePostStartup, (remember_spawn, load_player).chain().after(crate::world::setup_generator))
            .add_systems(
                crate::menu::GameUpdate,
                (debug_keys, take_hits, eat, tick_body, die_and_respawn, set_body)
                    .chain()
                    .in_set(SurvivalSet)
                    .after(PlayerSet),
            )
            .add_systems(crate::menu::GameUpdate, save_player)
            .add_systems(Last, save_player_on_exit.run_if(crate::menu::in_game))
            .add_plugins((camp::CampPlugin, hud::SurvivalHudPlugin));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SurvivalSet;

/// A short message in the middle of the screen.
#[derive(Message, Debug, Clone)]
pub struct Toast(pub String);

#[derive(Resource, Debug, Default)]
pub struct Survival {
    pub vitals: Vitals,
    /// What the body felt this frame (for the HUD).
    pub env: Env,
    /// Everything adding up to `env.feels`: (what, °C). For the HUD breakdown.
    pub temperature: Vec<(String, f32)>,
    /// Eating progress, 0..1.
    pub eating: f32,
    /// When you died; `None` while alive.
    pub dead_for: Option<f32>,
}

/// Where a new world puts you; the respawn point without a camp.
#[derive(Resource, Debug, Clone, Copy)]
pub struct SpawnPoint(pub Vec3);

fn remember_spawn(mut commands: Commands, player: Res<Player>) {
    commands.insert_resource(SpawnPoint(player.pos));
}

fn debug_keys(keys: Res<ButtonInput<KeyCode>>, mut hits: MessageWriter<Damage>) {
    if keys.just_pressed(KeyCode::F7) {
        hits.write(Damage { amount: 20.0, cause: DamageCause::Piercing });
    }
    if keys.just_pressed(KeyCode::F8) {
        hits.write(Damage { amount: 25.0, cause: DamageCause::Blunt });
    }
}

/// Where each kind of blow lands first.
pub fn entry_layer(cause: DamageCause) -> Layer {
    match cause {
        DamageCause::Fall | DamageCause::Blunt => Layer::Damage,
        DamageCause::HeavyFall => Layer::Injuries,
        DamageCause::Piercing => Layer::Blood,
        DamageCause::Chill => Layer::Fatigue,
    }
}

fn take_hits(mut hits: MessageReader<Damage>, inv: Res<Inventory>, items: Res<Items>, mut s: ResMut<Survival>, mut toast: MessageWriter<Toast>) {
    for d in hits.read() {
        let h = &mut s.vitals.health;
        let was_mortal = h.mortal();
        // Armor keeps part of the blow off the bar it lands on.
        let layer = entry_layer(d.cause);
        let amount = d.amount * (1.0 - inv.protection(&items, layer));
        h.hit(layer, amount);
        if d.cause == DamageCause::Piercing {
            // Open wounds keep losing blood for a while.
            h.bleeding += amount * 0.02;
        }
        if let Some(m) = h.mortal()
            && Some(m) != was_mortal
            && !h.is_dead()
        {
            toast.write(Toast(format!("{} is failing: treat it before anything else heals", m.name())));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn eat(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    wielded: Res<Wielded>,
    menus: Res<crate::camera::Menus>,
    items: Res<Items>,
    tiers: Res<FoodTiers>,
    body: Res<Body>,
    mut inv: ResMut<Inventory>,
    mut anim: ResMut<HandAnim>,
    mut s: ResMut<Survival>,
    mut toast: MessageWriter<Toast>,
) {
    let held = wielded.0;
    let food = held.and_then(|i| match items.get(i).kind {
        ItemKind::Food { tier, satiety } => Some((i, tier, satiety)),
        _ => None,
    });
    let (Some((id, tier, satiety)), true, false) = (food, mouse.pressed(MouseButton::Right), body.frozen || menus.open) else {
        s.eating = 0.0;
        anim.eat = 0.0;
        return;
    };
    s.eating += time.delta_secs() / EAT_SECONDS;
    anim.eat = s.eating;
    if s.eating >= 1.0 {
        s.eating = 0.0;
        let Some(t) = tiers.get(tier) else {
            warn!("{} names food tier {tier}, which food_tiers.ron doesn't have", items.get(id).name);
            return;
        };
        if !inv.take(id, 1) {
            return;
        }
        s.vitals.eat(t, satiety);
        toast.write(Toast(format!("Ate {} ({})", items.get(id).title, t.title)));
    }
}

/// Felt temperature, °C: the climate here, day and night, weather, being
/// wet, and fire nearby. Each part goes into `parts` with its share, for the
/// breakdown on the HUD.
#[allow(clippy::too_many_arguments)]
fn feels_like(
    climate: &LocalClimate,
    conditions: &Conditions,
    weather: &str,
    sun_height: f32,
    wet: bool,
    eye: Vec3,
    torches: &Torches,
    holding_torch: bool,
    parts: &mut Vec<(String, f32)>,
) -> f32 {
    let biome = if climate.biome_title.is_empty() { "Climate".to_string() } else { climate.biome_title.clone() };
    parts.push((biome, climate.biome * 30.0));
    parts.push(("Season".into(), climate.season * 30.0));
    parts.push(("Height above ground".into(), climate.altitude * 30.0));
    parts.push((if sun_height >= 0.0 { "Sun" } else { "Night" }.into(), sun_height * 5.0));
    parts.push((format!("Weather: {weather}"), conditions.chill));
    if wet {
        parts.push(("Wet".into(), -6.0));
    }
    let mut fire = 0.0;
    for p in torches.positions() {
        let d = eye.distance(Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.7, p.z as f32 + 0.5));
        fire += (10.0 * (1.0 - d / 6.0)).max(0.0);
    }
    // Fire tops out at +16 all told.
    let placed = fire.min(FIRE_MAX);
    parts.push((if fire > FIRE_MAX { "Torches nearby (max)" } else { "Torches nearby" }.into(), placed));
    if holding_torch {
        parts.push(("Torch in hand".into(), (FIRE_MAX - placed).min(3.0)));
    }
    parts.iter().map(|p| p.1).sum()
}

/// Most warmth that torches can give, °C.
const FIRE_MAX: f32 = 16.0;

#[allow(clippy::too_many_arguments)]
fn tick_body(
    time: Res<Time>,
    mode: Res<MoveMode>,
    player: Res<Player>,
    (climate, conditions, weather, kinds): (Res<LocalClimate>, Res<Conditions>, Res<WeatherState>, Res<WeatherKinds>),
    clock: Res<WorldClock>,
    seasons: Res<Seasons>,
    torches: Res<Torches>,
    camp: Res<Camp>,
    wielded: Res<Wielded>,
    inv: Res<Inventory>,
    items: Res<Items>,
    (perks, spells): (Res<crate::progress::skills::Perks>, Res<crate::progress::Spells>),
    (rules, mut forced): (Res<Exposure>, Local<Option<Forced>>),
    mut s: ResMut<Survival>,
    mut toast: MessageWriter<Toast>,
) {
    let forced = *forced.get_or_insert_with(Forced::from_env);
    let held = wielded.0;
    let holding_torch = held.is_some_and(|i| matches!(items.get(i).kind, ItemKind::Torch));
    let eye = player.pos + Vec3::Y * player.eye;
    let daylight = seasons.blended(seasons.at(&clock), |x| x.daylight);
    let sun = crate::sky::sun_direction(clock.time, daylight).y;
    let carried = inv.mass(&items) + if camp.bedroll.is_none() { BEDROLL_MASS } else { 0.0 };
    let mut parts = std::mem::take(&mut s.temperature);
    parts.clear();
    let weather = kinds.list.get(weather.current).map_or("", |k| k.title.as_str());
    let raw = feels_like(&climate, &conditions, weather, sun, player.in_water, eye, &torches, holding_torch, &mut parts);
    let underwater = player.head_in_water;
    let mut air = if underwater { 0.0 } else { vitals::air_at(eye.y) * conditions.air };
    let dressed = vitals::clothed(raw, inv.insulation(&items));
    parts.push(("Clothes".into(), dressed - raw));
    let mut feels = perks.feel(dressed, vitals::COMFORT_LOW + 6.0);
    parts.push(("Skills".into(), feels - dressed));
    if spells.warmth {
        let before = feels;
        feels = perks_warm(feels);
        parts.push(("Warmth spell".into(), feels - before));
    }
    if let Some(f) = forced.feels {
        parts.push(("BLOCKGAME_FEELS override".into(), f - feels));
        feels = f;
    }
    s.temperature = parts;
    if let Some(o) = forced.oxygen {
        air = o / 100.0;
    }
    let env = Env {
        air,
        breathe: perks.breathe(air),
        underwater,
        feels,
        sprinting: player.sprinting && *mode == MoveMode::Walk,
        carried: perks.carried(carried, vitals::CAPACITY),
    };
    s.env = env;
    // Forced values show at once, even while flying.
    if forced.feels.is_some() {
        s.vitals.feels = env.feels;
    }
    if forced.oxygen.is_some() {
        s.vitals.air = env.air * 100.0;
        s.vitals.oxygen = env.breathe * 100.0;
    }
    // Flying is for looking around: the body waits.
    if *mode == MoveMode::Walk && player.ready {
        let before = warnings(&s.vitals, &rules);
        s.vitals.tick(&env, &rules, time.delta_secs().min(0.25));
        // Say so when cold, heat or thin air gets worse.
        for (now, was) in warnings(&s.vitals, &rules).into_iter().zip(before) {
            if let Some((level, name)) = now
                && was.is_none_or(|w| w.0 < level)
            {
                toast.write(Toast(warning_text(&name)));
            }
        }
    }
}

/// Screenshot and test overrides for the surroundings.
#[derive(Clone, Copy, Default)]
struct Forced {
    feels: Option<f32>,
    oxygen: Option<f32>,
}

impl Forced {
    fn from_env() -> Self {
        let get = |k| std::env::var(k).ok().and_then(|v| v.trim().parse().ok());
        Self { feels: get("BLOCKGAME_FEELS"), oxygen: get("BLOCKGAME_OXYGEN") }
    }
}

/// The temperature and air stages in effect: (level, name).
fn warnings(v: &Vitals, rules: &Exposure) -> [Option<(usize, String)>; 2] {
    [
        rules.temperature(v.feels).map(|(scale, a)| (a.level + if scale == Scale::Heat { 100 } else { 0 }, a.stage.name.clone())),
        rules.stage(Scale::Air, v.oxygen).map(|a| (a.level, a.stage.name.clone())),
    ]
}

fn warning_text(stage: &str) -> String {
    match stage {
        "Chilly" => "You're getting cold: it's wearing you out".into(),
        "Hot" => "You're getting hot: it's wearing you out".into(),
        "Thin air" => "The air is thin here: it's wearing you out".into(),
        other => format!("{other}! Find shelter"),
    }
}

#[allow(clippy::too_many_arguments)]
fn die_and_respawn(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    spawn: Option<Res<SpawnPoint>>,
    camp: Res<Camp>,
    mut s: ResMut<Survival>,
    mut player: ResMut<Player>,
    mut cam: Query<&mut Transform, With<FlyCamera>>,
    mut toast: MessageWriter<Toast>,
) {
    if !s.vitals.health.is_dead() {
        s.dead_for = None;
        return;
    }
    let t = s.dead_for.get_or_insert(0.0);
    *t += time.delta_secs();
    if *t < 2.0 || !(keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::Enter)) {
        return;
    }
    let to = camp.respawn().or(spawn.map(|p| p.0)).unwrap_or(player.pos);
    s.vitals = Vitals { satiety: 60.0, ..Default::default() };
    s.dead_for = None;
    *player = Player::default();
    player.pos = to;
    for mut tf in &mut cam {
        tf.translation = to + Vec3::Y * crate::player::EYE;
    }
    toast.write(Toast(if camp.bedroll.is_some() { "You wake at your camp".into() } else { "You wake at the edge of the swamp".into() }));
}

/// The Warmth spell takes the edge off the cold (but never makes you hot).
fn perks_warm(feels: f32) -> f32 {
    if feels >= vitals::COMFORT_HIGH - 4.0 { feels } else { (feels + crate::progress::WARMTH_SPELL).min(vitals::COMFORT_HIGH - 4.0) }
}

fn set_body(s: Res<Survival>, rules: Res<Exposure>, sleep: Res<camp::Sleep>, mut body: ResMut<Body>) {
    let v = &s.vitals;
    *body = Body {
        speed: v.speed(s.env.carried, &rules),
        fall: vitals::weight_fall(s.env.carried),
        dizzy: v.dizzy(&rules),
        frozen: v.health.is_dead() || sleep.active(),
        ..default()
    };
}

/// What `player.ron` holds.
#[derive(Serialize, Deserialize)]
struct PlayerSave {
    pos: (f32, f32, f32),
    yaw: f32,
    pitch: f32,
    vitals: Vitals,
    camp: Option<camp::Bedroll>,
}

/// Screenshot runs neither load nor save the player, so they start the same.
fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some()
}

fn load_player(
    dir: Res<SaveDir>,
    rules: Res<Exposure>,
    mut s: ResMut<Survival>,
    mut camp: ResMut<Camp>,
    mut player: ResMut<Player>,
    mut cam: Query<(&mut Transform, &mut FlyCamera)>,
) {
    if !screenshot_run()
        && let Ok(text) = std::fs::read_to_string(dir.file("player.ron"))
    {
        match ron::from_str::<PlayerSave>(&text) {
            Ok(save) => {
                player.pos = Vec3::new(save.pos.0, save.pos.1, save.pos.2);
                s.vitals = save.vitals;
                s.vitals.breath = s.vitals.breath.min(rules.breath);
                camp.bedroll = save.camp;
                for (mut tf, mut fc) in &mut cam {
                    tf.translation = player.pos + Vec3::Y * crate::player::EYE;
                    fc.yaw = save.yaw;
                    fc.pitch = save.pitch;
                }
            }
            Err(e) => warn!("player.ron: {e}"),
        }
    }
    if let Ok(spec) = std::env::var("BLOCKGAME_VITALS") {
        let n: Vec<f32> = spec.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        let v = &mut s.vitals;
        for (i, x) in n.iter().take(4).enumerate() {
            v.health.bars[i] = x.clamp(0.0, health::MAX);
        }
        if let Some(x) = n.get(4) {
            v.satiety = *x;
        }
    }
}

fn store(dir: &SaveDir, s: &Survival, camp: &Camp, player: &Player, cam: &FlyCamera) {
    let save = PlayerSave { pos: player.pos.into(), yaw: cam.yaw, pitch: cam.pitch, vitals: s.vitals, camp: camp.bedroll };
    match ron::ser::to_string_pretty(&save, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file("player.ron"), text) {
                warn!("could not save the player: {e}");
            }
        }
        Err(e) => warn!("could not save the player: {e}"),
    }
}

fn save_player(
    time: Res<Time>,
    mut timer: Local<f32>,
    dir: Res<SaveDir>,
    s: Res<Survival>,
    camp: Res<Camp>,
    player: Res<Player>,
    cam: Query<&FlyCamera>,
) {
    *timer += time.delta_secs();
    if *timer < 30.0 || screenshot_run() || !player.ready {
        return;
    }
    *timer = 0.0;
    if let Ok(c) = cam.single() {
        store(&dir, &s, &camp, &player, c);
    }
}

fn save_player_on_exit(
    mut exit: MessageReader<AppExit>,
    dir: Res<SaveDir>,
    s: Res<Survival>,
    camp: Res<Camp>,
    player: Res<Player>,
    cam: Query<&FlyCamera>,
) {
    if exit.read().next().is_some()
        && !screenshot_run()
        && player.ready
        && let Ok(c) = cam.single()
    {
        store(&dir, &s, &camp, &player, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_blow_lands_on_the_right_bar() {
        // Plan check: falling, a stab, freezing and starving hit the right bars.
        let mut v = Vitals::default();
        v.health.hit(entry_layer(DamageCause::Fall), 10.0);
        assert_eq!(v.health.bars, [90.0, 100.0, 100.0, 100.0]);
        let mut v = Vitals::default();
        v.health.hit(entry_layer(DamageCause::HeavyFall), 30.0);
        assert_eq!(v.health.bars, [100.0, 100.0, 100.0, 70.0]);
        let mut v = Vitals::default();
        v.health.hit(entry_layer(DamageCause::Piercing), 20.0);
        assert_eq!(v.health.bars, [100.0, 100.0, 80.0, 100.0]);
    }
}
