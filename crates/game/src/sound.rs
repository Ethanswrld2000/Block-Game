//! Sound: footsteps by the ground's material, mining and breaking, fights,
//! spells and levelling up, and looping wind, rain, night and levitation
//! sounds. Which material sounds like what is in `assets/data/sounds.ron`;
//! the files are in `assets/sounds/`.

use std::collections::HashMap;

use bevy::audio::{AudioSinkPlayback, Volume};
use bevy::prelude::*;
use serde::Deserialize;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::clock::WorldClock;
use crate::creatures::Creature;
use crate::interact::{Mined, Mining, Placed, Strike};
use crate::player::{Damage, MoveMode, Player};
use crate::progress::{Progress, Spells};
use crate::settings::Settings;
use crate::weather::Conditions;
use crate::world::World;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        let spec: SoundSpec = ron::from_str(include_str!("../../../assets/data/sounds.ron")).unwrap_or_else(|e| panic!("sounds.ron: {e}"));
        app.insert_resource(spec)
            .add_message::<PlaySound>()
            .add_systems(Startup, (load_bank, start_loops).chain())
            .add_systems(
                Update,
                (footsteps, mining, fights, spells, level_up, thunder, spirits, play, loops)
                    .chain()
                    .after(crate::creatures::CreatureSet)
                    .after(crate::survival::SurvivalSet),
            );
    }
}

#[derive(Deserialize, Clone, Debug)]
struct MaterialSounds {
    step: String,
    dig: String,
}

#[derive(Resource, Deserialize, Debug)]
struct SoundSpec {
    materials: HashMap<String, MaterialSounds>,
    fallback: MaterialSounds,
    volumes: HashMap<String, f32>,
}

impl SoundSpec {
    fn of(&self, material: &str) -> &MaterialSounds {
        self.materials.get(material).unwrap_or(&self.fallback)
    }

    /// Volume for a sound: by its name, or the part before the first `_`
    /// (`step_hard_1` -> `step`).
    fn volume(&self, name: &str) -> f32 {
        self.volumes.get(name).or_else(|| name.split('_').next().and_then(|p| self.volumes.get(p))).copied().unwrap_or(0.5)
    }
}

/// Play a one-off sound by file name (without `.ogg`).
#[derive(Message, Debug, Clone)]
pub struct PlaySound {
    pub name: String,
    /// Multiplies the sound's own volume.
    pub gain: f32,
}

impl PlaySound {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), gain: 1.0 }
    }
}

#[derive(Resource, Default)]
struct Bank {
    handles: HashMap<String, Handle<AudioSource>>,
}

fn load_bank(mut commands: Commands, assets: Res<AssetServer>) {
    let mut names: Vec<String> = [
        "break", "place", "swing", "hit", "hurt", "creature_hurt", "spirit", "spell_on", "spell_off", "level_up", "thunder",
        "wind_loop", "rain_loop", "levitate_loop", "night_loop", "dig_soft", "dig_hard", "dig_wood", "dig_plant",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for kind in ["soft", "hard", "wood", "plant"] {
        for i in 0..3 {
            names.push(format!("step_{kind}_{i}"));
        }
    }
    let handles = names.into_iter().map(|n| (n.clone(), assets.load(format!("sounds/{n}.ogg")))).collect();
    commands.insert_resource(Bank { handles });
}

/// A looping sound whose volume follows the world.
#[derive(Component, Clone, Copy, PartialEq)]
enum Loop {
    Wind,
    Rain,
    Night,
    Levitate,
}

impl Loop {
    fn file(self) -> &'static str {
        match self {
            Loop::Wind => "wind_loop",
            Loop::Rain => "rain_loop",
            Loop::Night => "night_loop",
            Loop::Levitate => "levitate_loop",
        }
    }
}

fn start_loops(mut commands: Commands, bank: Res<Bank>) {
    for l in [Loop::Wind, Loop::Rain, Loop::Night, Loop::Levitate] {
        if let Some(h) = bank.handles.get(l.file()) {
            commands.spawn((l, AudioPlayer(h.clone()), PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0))));
        }
    }
}

/// Small random variation so repeats don't sound identical.
fn jitter(seed: &mut u64) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    0.9 + (*seed >> 40) as f32 / (1u64 << 24) as f32 * 0.2
}

fn play(mut commands: Commands, mut msgs: MessageReader<PlaySound>, bank: Option<Res<Bank>>, spec: Res<SoundSpec>, settings: Res<Settings>, mut seed: Local<u64>) {
    let Some(bank) = bank else { return };
    if *seed == 0 {
        *seed = 0x5EED_50DD;
    }
    for m in msgs.read() {
        let Some(h) = bank.handles.get(&m.name) else {
            warn_once!("no sound '{}'", m.name);
            continue;
        };
        let v = spec.volume(&m.name) * m.gain * settings.volume;
        if v <= 0.001 {
            continue;
        }
        let speed = jitter(&mut seed);
        commands.spawn((AudioPlayer(h.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(v)).with_speed(speed)));
    }
}

fn material_at(world: &World, blocks: &Blocks, p: BlockPos) -> Option<String> {
    world.block(p).filter(|b| !b.is_air()).map(|b| blocks.registry.get(b).material.clone())
}

/// A footstep every time the head-bob passes a footfall.
#[allow(clippy::too_many_arguments)]
fn footsteps(
    player: Res<Player>,
    mode: Res<MoveMode>,
    world: Res<World>,
    blocks: Res<Blocks>,
    spec: Res<SoundSpec>,
    mut out: MessageWriter<PlaySound>,
    mut last: Local<f32>,
    mut which: Local<usize>,
) {
    let step = (player.bob_phase / std::f32::consts::PI).floor();
    let changed = step != *last;
    *last = step;
    if !changed || *mode != MoveMode::Walk || !player.on_ground || player.in_water {
        return;
    }
    let under = BlockPos::from_world((player.pos - Vec3::Y * 0.2).to_array());
    let Some(material) = material_at(&world, &blocks, under) else { return };
    *which = (*which + 1 + (step as usize % 2)) % 3;
    let gain = if player.crouching { 0.4 } else if player.sprinting { 1.3 } else { 1.0 };
    out.write(PlaySound { name: format!("{}_{}", spec.of(&material).step, *which), gain });
}

/// A knock with each blow while mining; a crunch when the block goes.
#[allow(clippy::too_many_arguments)]
fn mining(
    time: Res<Time>,
    m: Res<Mining>,
    world: Res<World>,
    blocks: Res<Blocks>,
    spec: Res<SoundSpec>,
    mut mined: MessageReader<Mined>,
    mut placed: MessageReader<Placed>,
    mut out: MessageWriter<PlaySound>,
    mut timer: Local<f32>,
) {
    if let Some(p) = m.block {
        *timer -= time.delta_secs();
        if *timer <= 0.0 {
            *timer = crate::interact::SWING_TIME;
            if let Some(material) = material_at(&world, &blocks, p) {
                out.write(PlaySound::new(spec.of(&material).dig.clone()));
            }
        }
    } else {
        *timer = 0.0;
    }
    for _ in mined.read() {
        out.write(PlaySound::new("break"));
    }
    for _ in placed.read() {
        out.write(PlaySound::new("place"));
    }
}

fn fights(mut strikes: MessageReader<Strike>, mut hurts: MessageReader<Damage>, mut out: MessageWriter<PlaySound>) {
    for _ in strikes.read() {
        out.write(PlaySound::new("hit"));
        out.write(PlaySound::new("creature_hurt"));
    }
    for d in hurts.read() {
        if d.amount > 0.5 {
            out.write(PlaySound { name: "hurt".into(), gain: (d.amount / 15.0).clamp(0.4, 1.2) });
        }
    }
}

fn spells(spells: Res<Spells>, mut out: MessageWriter<PlaySound>, mut was: Local<(bool, bool, bool)>) {
    let now = (spells.levitate, spells.light, spells.warmth);
    for (a, b) in [(was.0, now.0), (was.1, now.1), (was.2, now.2)] {
        if !a && b {
            out.write(PlaySound::new("spell_on"));
        } else if a && !b {
            out.write(PlaySound::new("spell_off"));
        }
    }
    *was = now;
}

fn level_up(progress: Res<Progress>, mut out: MessageWriter<PlaySound>, mut last: Local<Option<u32>>) {
    let l = progress.level.level;
    if last.is_some_and(|x| l > x) {
        out.write(PlaySound::new("level_up"));
    }
    *last = Some(l);
}

fn thunder(conditions: Res<Conditions>, mut out: MessageWriter<PlaySound>, mut was: Local<bool>) {
    let flash = conditions.flash > 0.5;
    if flash && !*was {
        out.write(PlaySound::new("thunder"));
    }
    *was = flash;
}

/// Spirits murmur now and then when they're close.
fn spirits(
    time: Res<Time>,
    kinds: Option<Res<crate::creatures::data::Kinds>>,
    player: Res<Player>,
    q: Query<(&Transform, &Creature)>,
    mut out: MessageWriter<PlaySound>,
    mut timer: Local<f32>,
) {
    let Some(kinds) = kinds else { return };
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 5.0;
    let nearest = q
        .iter()
        .filter(|(_, c)| kinds.get(c.kind).temper == crate::creatures::data::Temper::Spirit && c.fade > 0.5)
        .map(|(tf, _)| tf.translation.distance(player.pos))
        .fold(f32::MAX, f32::min);
    if nearest < 24.0 {
        out.write(PlaySound { name: "spirit".into(), gain: 1.0 - nearest / 30.0 });
    }
}

/// Loops follow the weather, the time of day, height and spells.
#[allow(clippy::too_many_arguments)]
fn loops(
    time: Res<Time>,
    conditions: Res<Conditions>,
    clock: Res<WorldClock>,
    player: Res<Player>,
    spells: Res<Spells>,
    settings: Res<Settings>,
    spec: Res<SoundSpec>,
    mut q: Query<(&Loop, &mut AudioSink)>,
) {
    let dt = time.delta_secs();
    let high = ((player.pos.y - 80.0) / 200.0).clamp(0.0, 1.0);
    let rain = if conditions.snowing { 0.0 } else { conditions.precipitation.clamp(0.0, 1.0) };
    let night = if crate::survival::camp::is_night(clock.hour()) { 1.0 - rain * 0.7 } else { 0.0 };
    for (l, mut sink) in &mut q {
        let level = match l {
            Loop::Wind => ((conditions.wind / 20.0).clamp(0.0, 1.0) * 0.8 + high * 0.6).min(1.0),
            Loop::Rain => rain,
            Loop::Night => night,
            Loop::Levitate => {
                if spells.levitate {
                    1.0
                } else {
                    0.0
                }
            }
        };
        let master = if *l == Loop::Levitate { settings.volume } else { settings.volume * settings.ambience };
        let target = level * spec.volume(l.file()) * master;
        let now = sink.volume().to_linear();
        let next = now + (target - now) * (dt * 1.5).min(1.0);
        if (next - now).abs() > 1e-4 {
            sink.set_volume(Volume::Linear(next));
        }
    }
}
