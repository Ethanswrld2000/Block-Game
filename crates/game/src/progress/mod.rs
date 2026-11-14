//! Progression: experience and levels (`xp`), the skill tree (`skills`),
//! spells and the focus that powers them, and the skill tree screen (`ui`).
//!
//! XP comes from mining, hunting, crafting and finding places (amounts in
//! `assets/data/progression.ron`). Each level gives a skill point to spend in
//! the tree (K). Spells: G levitates (Space rises, Ctrl/C sinks), X calls a
//! light, V warms you. They drain focus, which refills while unused.

pub mod skills;
mod ui;
pub mod xp;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::camera::FlyCamera;
use crate::interact::Mined;
use crate::player::{Body, MoveMode, Player};
use crate::save::SaveDir;
use crate::survival::{Survival, SurvivalSet, Toast};
use skills::{Learned, Perks, Spell, Tree};
use xp::{Level, Progression};

pub struct ProgressPlugin;

impl Plugin for ProgressPlugin {
    fn build(&self, app: &mut App) {
        let tree = Tree::from_ron(include_str!("../../../../assets/data/skills.ron")).unwrap_or_else(|e| panic!("{e}"));
        let prog = Progression::from_ron(include_str!("../../../../assets/data/progression.ron")).unwrap_or_else(|e| panic!("{e}"));
        let progress = Progress::new(&prog);
        app.insert_resource(tree)
            .insert_resource(prog)
            .insert_resource(progress)
            .init_resource::<Perks>()
            .init_resource::<Spells>()
            .add_message::<GainXp>()
            .add_systems(crate::menu::GamePostStartup, load_progress)
            .add_systems(
                crate::menu::GameUpdate,
                (xp_from_mining, xp_from_sky, gain_xp, update_perks, cast, apply_to_body, follow_light)
                    .chain()
                    .after(SurvivalSet)
                    .after(crate::interact::InteractSet),
            )
            .add_systems(crate::menu::GameUpdate, save_progress)
            .add_systems(Last, save_progress_on_exit.run_if(crate::menu::in_game))
            .add_plugins(ui::ProgressUiPlugin);
    }
}

/// Earned experience; `why` shows briefly on screen.
#[derive(Message, Debug, Clone)]
pub struct GainXp {
    pub xp: f32,
    pub why: String,
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub level: Level,
    pub learned: Learned,
    pub focus: f32,
    /// Has stood on a sky island.
    #[serde(default)]
    pub sky: bool,
    /// Places found (village, barrow and region keys), each worth XP once.
    #[serde(default)]
    pub found: std::collections::BTreeSet<String>,
}

impl Progress {
    fn new(p: &Progression) -> Self {
        Self { level: Level::default(), learned: Learned { points: p.start_points, ..default() }, focus: BASE_FOCUS, sky: false, found: Default::default() }
    }
}

/// Focus everyone has, before skills.
pub const BASE_FOCUS: f32 = 60.0;
const BASE_REGEN: f32 = 2.0;
/// Focus per second while levitating (before skills).
const LEVITATE_DRAIN: f32 = 1.8;
const LIGHT_DRAIN: f32 = 0.8;
const WARMTH_DRAIN: f32 = 1.2;
/// How much warmer the Warmth spell makes cold feel.
pub const WARMTH_SPELL: f32 = 14.0;
/// Levitation: climbing and sinking speed, blocks/s.
pub const LEVITATE_UP: f32 = 10.0;
pub const LEVITATE_DOWN: f32 = 4.0;

/// The spells running now.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct Spells {
    pub levitate: bool,
    pub light: bool,
    pub warmth: bool,
    /// Focus ran dry mid-air: drift down instead of dropping.
    pub feather: bool,
}

impl Perks {
    pub fn max_focus(&self) -> f32 {
        BASE_FOCUS + self.focus
    }
}

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some()
}

fn xp_from_mining(mut mined: MessageReader<Mined>, blocks: Res<crate::blocks::Blocks>, prog: Res<Progression>, mut gain: MessageWriter<GainXp>) {
    let mut total = 0.0;
    for m in mined.read() {
        let h = blocks.registry.get(m.block).hardness.max(0.0);
        total += (h * prog.xp.mine).min(prog.xp.mine_cap);
    }
    if total > 0.0 {
        gain.write(GainXp { xp: total, why: String::new() });
    }
}

/// The first time you stand on a sky island.
fn xp_from_sky(
    player: Res<Player>,
    mode: Res<MoveMode>,
    generator: Option<Res<crate::world::Generator>>,
    prog: Res<Progression>,
    mut progress: ResMut<Progress>,
    mut gain: MessageWriter<GainXp>,
) {
    let Some(generator) = generator else { return };
    if progress.sky || *mode != MoveMode::Walk || !player.on_ground || player.pos.y < 300.0 {
        return;
    }
    let (x, z) = (player.pos.x.floor() as i32, player.pos.z.floor() as i32);
    if generator.0.island_span(x, z).is_some() {
        progress.sky = true;
        gain.write(GainXp { xp: prog.xp.sky_island, why: "First steps on a sky island".into() });
    }
}

fn gain_xp(mut gains: MessageReader<GainXp>, prog: Res<Progression>, mut progress: ResMut<Progress>, mut toast: MessageWriter<Toast>, s: Res<Survival>) {
    for g in gains.read() {
        if s.dead_for.is_some() {
            continue;
        }
        if !g.why.is_empty() {
            toast.write(Toast(format!("{} (+{:.0} XP)", g.why, g.xp)));
        }
        let p = &mut *progress;
        let up = p.level.add(&prog, g.xp);
        if up > 0 {
            p.learned.points += up * prog.points;
            toast.write(Toast(format!("Level {}! {} skill point{} to spend (K)", p.level.level, p.learned.points, if p.learned.points == 1 { "" } else { "s" })));
        }
    }
}

fn update_perks(tree: Res<Tree>, progress: Res<Progress>, mut perks: ResMut<Perks>) {
    if progress.is_changed() {
        let p = progress.learned.perks(&tree);
        if *perks != p {
            *perks = p;
        }
    }
}

/// Spell keys and the focus they burn.
#[allow(clippy::too_many_arguments)]
fn cast(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    menus: Res<crate::camera::Menus>,
    mode: Res<MoveMode>,
    perks: Res<Perks>,
    player: Res<Player>,
    s: Res<Survival>,
    mut spells: ResMut<Spells>,
    mut progress: ResMut<Progress>,
    mut toast: MessageWriter<Toast>,
) {
    let dt = time.delta_secs();
    let dead = s.dead_for.is_some();
    if !menus.open && !dead && *mode == MoveMode::Walk {
        for (key, spell) in [(KeyCode::KeyG, Spell::Levitation), (KeyCode::KeyX, Spell::Light), (KeyCode::KeyV, Spell::Warmth)] {
            if !keys.just_pressed(key) {
                continue;
            }
            if !perks.spells.contains(&spell) {
                toast.write(Toast(format!("You haven't learned {} (K for skills)", spell.title())));
                continue;
            }
            let on = match spell {
                Spell::Levitation => &mut spells.levitate,
                Spell::Light => &mut spells.light,
                Spell::Warmth => &mut spells.warmth,
            };
            if !*on && progress.focus < 3.0 {
                toast.write(Toast("Not enough focus".into()));
                continue;
            }
            *on = !*on;
        }
    }
    if dead {
        *spells = Spells::default();
    }
    let max = perks.max_focus();
    let drain = if spells.levitate { LEVITATE_DRAIN * (1.0 - perks.levitate_cost.min(0.8)) } else { 0.0 }
        + if spells.light { LIGHT_DRAIN } else { 0.0 }
        + if spells.warmth { WARMTH_DRAIN } else { 0.0 };
    let p = progress.bypass_change_detection();
    // Like the clock, focus stands still during a screenshot run.
    let drain = if screenshot_run() { 0.0 } else { drain };
    if drain > 0.0 {
        p.focus -= drain * dt;
    } else {
        p.focus = (p.focus + (BASE_REGEN + perks.focus_regen) * dt).min(max);
    }
    p.focus = p.focus.min(max);
    if p.focus <= 0.0 {
        p.focus = 0.0;
        if spells.levitate {
            spells.feather = true;
            toast.write(Toast("Your focus is spent: drifting down".into()));
        }
        spells.levitate = false;
        spells.light = false;
        spells.warmth = false;
    }
    // Feather fall lasts until you land.
    if spells.feather && (player.on_ground || player.in_water) {
        spells.feather = false;
    }
}

/// Skills and spells change how the body moves.
fn apply_to_body(perks: Res<Perks>, spells: Res<Spells>, mut body: ResMut<Body>) {
    body.speed *= 1.0 + perks.speed;
    body.levitate = spells.levitate;
    body.feather = spells.feather;
}

#[derive(Component)]
struct SpellLight;

/// The Light spell: a soft light floating over your shoulder.
fn follow_light(
    mut commands: Commands,
    time: Res<Time>,
    spells: Res<Spells>,
    cam: Single<&Transform, (With<FlyCamera>, Without<SpellLight>)>,
    mut q: Query<(Entity, &mut Transform), With<SpellLight>>,
) {
    let want = cam.translation + cam.right() * 0.7 + Vec3::Y * 0.9;
    match (spells.light, q.single_mut()) {
        (true, Ok((_, mut tf))) => {
            let t = time.elapsed_secs();
            let bob = Vec3::Y * (t * 2.0).sin() * 0.08;
            tf.translation = tf.translation.lerp(want + bob, (time.delta_secs() * 6.0).min(1.0));
        }
        (true, Err(_)) => {
            commands.spawn((
                SpellLight,
                PointLight { color: Color::srgb(0.75, 0.88, 1.0), intensity: 90_000.0, range: 16.0, shadow_maps_enabled: false, ..default() },
                Transform::from_translation(want),
            ));
        }
        (false, Ok((e, _))) => commands.entity(e).despawn(),
        (false, Err(_)) => {}
    }
}

fn load_progress(dir: Res<SaveDir>, tree: Res<Tree>, mut progress: ResMut<Progress>, mut perks: ResMut<Perks>, mut spells: ResMut<Spells>) {
    if !screenshot_run()
        && let Ok(text) = std::fs::read_to_string(dir.file("progress.ron"))
    {
        match ron::from_str::<Progress>(&text) {
            Ok(p) => *progress = p,
            Err(e) => warn!("progress.ron: {e}"),
        }
    }
    // `BLOCKGAME_SKILLS=n` hands out n extra points and learns the root and Mystic (for screenshots).
    if let Ok(n) = std::env::var("BLOCKGAME_SKILLS").map(|v| v.parse::<u32>().unwrap_or(0)) {
        progress.learned.points += n;
        let _ = progress.learned.take(&tree, skills::Node::Major(tree.root));
        if let Some(m) = tree.majors.iter().position(|m| m.name == "mystic") {
            let _ = progress.learned.take(&tree, skills::Node::Major(m));
        }
    }
    *perks = progress.learned.perks(&tree);
    // `BLOCKGAME_CAST=levitate,light,warmth` starts with those spells running (for screenshots).
    if let Ok(list) = std::env::var("BLOCKGAME_CAST") {
        spells.levitate = list.contains("levitate");
        spells.light = list.contains("light");
        spells.warmth = list.contains("warmth");
    }
}

fn store(dir: &SaveDir, p: &Progress) {
    match ron::ser::to_string_pretty(p, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file("progress.ron"), text) {
                warn!("saving progress.ron: {e}");
            }
        }
        Err(e) => warn!("progress.ron: {e}"),
    }
}

fn save_progress(time: Res<Time>, mut timer: Local<f32>, dir: Res<SaveDir>, p: Res<Progress>) {
    *timer += time.delta_secs();
    if *timer < 30.0 || screenshot_run() {
        return;
    }
    *timer = 0.0;
    store(&dir, &p);
}

fn save_progress_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, p: Res<Progress>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Balance: with the first two nodes (Wanderer, Mystic) a full focus bar
    /// lifts you from sea level to the lowest sky islands.
    #[test]
    fn levitation_reaches_the_sky_islands() {
        let tree = Tree::from_ron(include_str!("../../../../assets/data/skills.ron")).unwrap();
        let prog = Progression::from_ron(include_str!("../../../../assets/data/progression.ron")).unwrap();
        let mut p = Progress::new(&prog);
        p.learned.points += prog.points; // level 2
        p.learned.take(&tree, skills::Node::Major(tree.root)).unwrap();
        let mystic = tree.majors.iter().position(|m| m.name == "mystic").unwrap();
        p.learned.take(&tree, skills::Node::Major(mystic)).unwrap();
        let perks = p.learned.perks(&tree);
        assert!(perks.spells.contains(&Spell::Levitation));
        let seconds = perks.max_focus() / (LEVITATE_DRAIN * (1.0 - perks.levitate_cost));
        let climb = seconds * LEVITATE_UP;
        let lowest_island_top = (worldgen::SKY_MIN + 40) as f32;
        assert!(climb > lowest_island_top - worldgen::SEA_LEVEL as f32, "climbs {climb:.0} blocks");
    }
}
