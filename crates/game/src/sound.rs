//! Sound: footsteps by the ground's material, mining and breaking, fights,
//! spells and levelling up, critters calling (panned to where they are), and
//! looping wind, rain and levitation sounds, thunder after lightning, and music now and then. Which material sounds like what is in `assets/data/sounds.ron`;
//! the files are in `assets/sounds/`.
//!
//! Sounds from a place are panned between the ears and fade with distance; a
//! solid block between it and your ears muffles it a little (a low-pass), and
//! with your head under water everything but the music is muffled hard
//! (`Muffled`, `muffle`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::audio::{AddAudioSource, AudioSinkPlayback, Decodable, SpatialListener, SpatialScale, Volume};
use bevy::prelude::*;
use serde::Deserialize;
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::creatures::Creature;
use crate::interact::{Blow, Mined, Placed, Strike};
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
            .add_audio_source::<Muffled>()
            .add_message::<PlaySound>()
            .add_systems(crate::menu::GameStartup, load_bank)
            .add_systems(
                crate::menu::GameUpdate,
                (footsteps, mining, fights, spells, level_up, thunder, spirits, play, loops, muffle, music)
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
    #[serde(default)]
    music: MusicSpec,
}

/// Which tracks play when, and the quiet between them (seconds).
#[derive(Deserialize, Debug, Clone, Default)]
struct MusicSpec {
    day: Vec<String>,
    night: Vec<String>,
    gap: (f32, f32),
    first: f32,
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
    /// Where it comes from, for panning between the ears (the caller fades it with distance).
    pub at: Option<Vec3>,
    /// Part of the ambience: follows the ambience volume setting too.
    pub ambient: bool,
    /// Playback speed (pitch); `None` = a little random jitter.
    pub speed: Option<f32>,
}

impl PlaySound {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), gain: 1.0, at: None, ambient: false, speed: None }
    }

    pub fn gain(mut self, gain: f32) -> Self {
        self.gain = gain;
        self
    }

    /// Play at this speed (and so pitch) instead of a random jitter.
    pub fn speed(mut self, speed: f32) -> Self {
        self.speed = Some(speed);
        self
    }

    /// From this place in the world: panned, faded with distance, muffled behind walls.
    pub fn at(mut self, at: Vec3) -> Self {
        self.at = Some(at);
        self
    }

    /// An ambient sound from a place in the world.
    pub fn ambient_at(name: impl Into<String>, at: Vec3, gain: f32) -> Self {
        Self { name: name.into(), gain, at: Some(at), ambient: true, speed: None }
    }
}

#[derive(Resource, Default)]
struct Bank {
    handles: HashMap<String, Handle<AudioSource>>,
}

fn load_bank(mut commands: Commands, assets: Res<AssetServer>) {
    let mut names: Vec<String> = [
        "break", "place", "swing", "hit", "clash", "hurt", "creature_hurt", "spirit", "spell_on", "spell_off", "level_up", "thunder",
        "wind_loop", "rain_loop", "levitate_loop", "dig_soft", "dig_hard", "dig_wood", "dig_plant",
        "bird_flap", "ui_open", "ui_close", "ui_tab", "ui_pick", "ui_drop", "ui_equip", "ui_draw", "ui_sheathe",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for kind in ["soft", "hard", "wood", "plant"] {
        for i in 0..3 {
            names.push(format!("step_{kind}_{i}"));
        }
    }
    // Critter calls named in creatures.ron load the first time they're played.
    let handles = names.into_iter().map(|n| (n.clone(), assets.load(format!("sounds/{n}.ogg")))).collect();
    commands.insert_resource(Bank { handles });
}

/// A looping sound whose volume follows the world.
#[derive(Component, Clone, Copy, PartialEq)]
enum Loop {
    Wind,
    Rain,
    Levitate,
}

impl Loop {
    fn file(self) -> &'static str {
        match self {
            Loop::Wind => "wind_loop",
            Loop::Rain => "rain_loop",
            Loop::Levitate => "levitate_loop",
        }
    }
}

/// A sound with its own low-pass filter, whose cutoff can change while it
/// plays (`Filter` holds the other end). Made per sound from a loaded `AudioSource`.
#[derive(Asset, TypePath)]
struct Muffled {
    source: AudioSource,
    cutoff: Arc<AtomicU32>,
}

impl Decodable for Muffled {
    type Decoder = LowPass<<AudioSource as Decodable>::Decoder>;

    fn decoder(&self) -> Self::Decoder {
        LowPass::new(self.source.decoder(), self.cutoff.clone())
    }
}

/// The game's end of a playing sound's filter: where it comes from (if
/// anywhere) and how much the blocks in the way muffle it, eased.
#[derive(Component)]
struct Filter {
    cutoff: Arc<AtomicU32>,
    at: Option<Vec3>,
    blocked: f32,
}

/// Cutoff (Hz) with a wall between you and the sound: a slight muffle.
const WALL_CUTOFF: f32 = 1800.0;
/// Cutoff (Hz) with your head under water.
const WATER_CUTOFF: f32 = 450.0;
/// No filter at all (cutoff 0 = off).
const OPEN: f32 = 0.0;

/// Two one-pole low-passes in a row (12 dB per octave), per channel. The
/// cutoff is read from `cutoff` (f32 bits, Hz; 0 = off) every few samples.
struct LowPass<S> {
    inner: S,
    cutoff: Arc<AtomicU32>,
    alpha: f32,
    state: [[f32; 2]; 8],
    channel: usize,
    count: u32,
}

impl<S: bevy::audio::Source> LowPass<S> {
    fn new(inner: S, cutoff: Arc<AtomicU32>) -> Self {
        let mut f = Self { inner, cutoff, alpha: 1.0, state: [[0.0; 2]; 8], channel: 0, count: 0 };
        f.refresh();
        f
    }

    fn refresh(&mut self) {
        let hz = f32::from_bits(self.cutoff.load(Ordering::Relaxed));
        self.alpha = lowpass_alpha(hz, self.inner.sample_rate().get() as f32);
    }
}

/// The one-pole smoothing factor for a cutoff at a sample rate (1 = pass everything).
fn lowpass_alpha(cutoff: f32, rate: f32) -> f32 {
    if cutoff <= 0.0 || cutoff >= rate * 0.5 {
        return 1.0;
    }
    1.0 - (-std::f32::consts::TAU * cutoff / rate).exp()
}

impl<S: bevy::audio::Source> Iterator for LowPass<S> {
    type Item = bevy::audio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let x = self.inner.next()?;
        if self.count == 0 {
            self.refresh();
        }
        self.count = (self.count + 1) % 256;
        let channels = (self.inner.channels().get() as usize).clamp(1, 8);
        self.channel = (self.channel + 1) % channels;
        let [a, b] = &mut self.state[self.channel];
        *a += self.alpha * (x - *a);
        *b += self.alpha * (*a - *b);
        Some(*b)
    }
}

impl<S: bevy::audio::Source> bevy::audio::Source for LowPass<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> bevy::audio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> bevy::audio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.inner.total_duration()
    }
}

/// A filtered player for a loaded sound, and the filter's handle.
fn muffled(assets: &mut Assets<Muffled>, source: &AudioSource, at: Option<Vec3>) -> (AudioPlayer<Muffled>, Filter) {
    let cutoff = Arc::new(AtomicU32::new(OPEN.to_bits()));
    let h = assets.add(Muffled { source: source.clone(), cutoff: cutoff.clone() });
    (AudioPlayer(h), Filter { cutoff, at, blocked: 0.0 })
}

/// Whether a solid block stands between the ears and a sound (the block the
/// sound comes from doesn't count: a dig knock comes from inside its block).
fn walled(world: &World, blocks: &Blocks, ears: Vec3, at: Vec3) -> bool {
    let to = at - ears;
    let dist = to.length();
    if dist < 1.0 {
        return false;
    }
    let source = BlockPos::from_world(at.to_array());
    voxel::raycast::raycast(ears.to_array(), to.to_array(), dist, |p| {
        p != source && world.block(p).is_some_and(|b| {
            let def = blocks.registry.get(b);
            def.solid && def.opaque
        })
    })
    .is_some()
}

/// Sets every playing sound's filter: hard under water, a little for each sound
/// with a wall between it and your ears (eased so it doesn't click).
fn muffle(
    time: Res<Time>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<Blocks>,
    ears: Query<&GlobalTransform, With<SpatialListener>>,
    mut q: Query<&mut Filter>,
) {
    let ears = ears.iter().next().map_or(player.pos, |g| g.translation());
    let ease = (time.delta_secs() * 8.0).min(1.0);
    for mut f in &mut q {
        if let Some(at) = f.at {
            let target = if walled(&world, &blocks, ears, at) { 1.0 } else { 0.0 };
            f.blocked += (target - f.blocked) * ease;
        }
        let cutoff = if player.head_in_water {
            WATER_CUTOFF
        } else if f.blocked > 0.01 {
            // Eases from fully open (20 kHz) down to the wall cutoff.
            20000.0 * (WALL_CUTOFF / 20000.0).powf(f.blocked)
        } else {
            OPEN
        };
        f.cutoff.store(cutoff.to_bits(), Ordering::Relaxed);
    }
}

/// Positioned sounds (not ambient ones, whose callers fade them) are full
/// within `NEAR` blocks of your ears and gone at `FAR`.
const NEAR: f32 = 4.0;
const FAR: f32 = 40.0;

fn distance_fade(d: f32) -> f32 {
    (1.0 - (d - NEAR) / (FAR - NEAR)).clamp(0.0, 1.0)
}

/// Small random variation so repeats don't sound identical.
fn jitter(seed: &mut u64) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    0.9 + (*seed >> 40) as f32 / (1u64 << 24) as f32 * 0.2
}

/// Spatial sounds are only panned: positions are shrunk so far that the built-in
/// fall-off (1 / distance²) never bites, and callers fade them with distance themselves.
const PAN_ONLY: SpatialScale = SpatialScale::new(0.001);

/// Plays each `PlaySound` once its file has loaded (until then it waits, for
/// up to `LOAD_WAIT` seconds).
#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut msgs: MessageReader<PlaySound>,
    bank: Option<ResMut<Bank>>,
    server: Res<AssetServer>,
    sources: Res<Assets<AudioSource>>,
    mut muffled_sounds: ResMut<Assets<Muffled>>,
    spec: Res<SoundSpec>,
    settings: Res<Settings>,
    player: Res<Player>,
    ears: Query<&GlobalTransform, With<SpatialListener>>,
    mut waiting: Local<Vec<(PlaySound, f32)>>,
    mut seed: Local<u64>,
) {
    const LOAD_WAIT: f32 = 5.0;
    let Some(mut bank) = bank else { return };
    if *seed == 0 {
        *seed = 0x5EED_50DD;
    }
    let ears = ears.iter().next().map_or(player.pos, |g| g.translation());
    let dt = time.delta_secs();
    let mut queue: Vec<(PlaySound, f32)> = std::mem::take(&mut *waiting);
    queue.extend(msgs.read().map(|m| (m.clone(), 0.0)));
    for (m, age) in queue {
        let mut v = spec.volume(&m.name) * m.gain * settings.volume;
        if m.ambient {
            v *= settings.ambience;
        } else if let Some(at) = m.at {
            v *= distance_fade(at.distance(ears));
        }
        if v <= 0.001 {
            continue;
        }
        let h = bank.handles.entry(m.name.clone()).or_insert_with(|| server.load(format!("sounds/{}.ogg", m.name))).clone();
        let Some(source) = sources.get(&h) else {
            if age < LOAD_WAIT {
                waiting.push((m, age + dt));
            }
            continue;
        };
        let speed = m.speed.unwrap_or_else(|| jitter(&mut seed));
        let playback = PlaybackSettings::DESPAWN.with_volume(Volume::Linear(v)).with_speed(speed);
        let (sound, filter) = muffled(&mut muffled_sounds, source, m.at);
        match m.at {
            Some(at) => commands.spawn((sound, filter, playback.with_spatial(true).with_spatial_scale(PAN_ONLY), Transform::from_translation(at))),
            None => commands.spawn((sound, filter, playback)),
        };
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
    out.write(PlaySound::new(format!("{}_{}", spec.of(&material).step, *which)).gain(gain));
}

/// A knock with each blow while mining, the moment it lands; a crunch when the block goes.
fn mining(
    blocks: Res<Blocks>,
    spec: Res<SoundSpec>,
    mut blows: MessageReader<Blow>,
    mut mined: MessageReader<Mined>,
    mut placed: MessageReader<Placed>,
    mut out: MessageWriter<PlaySound>,
) {
    for b in blows.read() {
        let material = &blocks.registry.get(b.block).material;
        // From the block, so a blow off to one side sounds from that side.
        let at = Vec3::new(b.pos.x as f32, b.pos.y as f32, b.pos.z as f32) + Vec3::splat(0.5);
        out.write(PlaySound { at: Some(at), ..PlaySound::new(spec.of(material).dig.clone()) });
    }
    for _ in mined.read() {
        out.write(PlaySound::new("break"));
    }
    for _ in placed.read() {
        out.write(PlaySound::new("place"));
    }
}

fn fights(mut strikes: MessageReader<Strike>, mut hurts: MessageReader<Damage>, bodies: Query<&GlobalTransform>, mut out: MessageWriter<PlaySound>) {
    for s in strikes.read() {
        let at = bodies.get(s.entity).ok().map(|g| g.translation() + Vec3::Y * 0.5);
        out.write(PlaySound { at, ..PlaySound::new("hit") });
        out.write(PlaySound { at, ..PlaySound::new("creature_hurt") });
    }
    for d in hurts.read() {
        if d.amount > 0.5 {
            out.write(PlaySound::new("hurt").gain((d.amount / 15.0).clamp(0.4, 1.2)));
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

/// Thunder follows each lightning strike after a delay, as if the strike were
/// as far off as its bolt (`Conditions::strike_far`): the further, the later,
/// quieter and deeper.
fn thunder(time: Res<Time>, conditions: Res<Conditions>, mut out: MessageWriter<PlaySound>, mut state: Local<(u32, Vec<(f32, f32)>)>) {
    let (seen, pending) = &mut *state;
    if conditions.strikes != *seen {
        *seen = conditions.strikes;
        let far = conditions.strike_far;
        pending.push((thunder_delay(far), far));
    }
    let dt = time.delta_secs();
    pending.retain_mut(|(wait, far)| {
        *wait -= dt;
        if *wait > 0.0 {
            return true;
        }
        out.write(PlaySound::new("thunder").gain(1.0 - 0.6 * *far).speed(1.05 - 0.3 * *far));
        false
    });
}

/// Seconds from the flash to the thunder for a strike `far` (0 close, 1 a few km off).
fn thunder_delay(far: f32) -> f32 {
    0.3 + far.clamp(0.0, 1.0) * 7.0
}

/// One music track now and then: day or night tracks by the sun, quiet between.
#[allow(clippy::too_many_arguments)]
fn music(
    mut commands: Commands,
    time: Res<Time<Real>>,
    spec: Res<SoundSpec>,
    settings: Res<Settings>,
    clock: Res<crate::clock::WorldClock>,
    bank: Option<ResMut<Bank>>,
    server: Res<AssetServer>,
    mut playing: Query<&mut AudioSink, With<Music>>,
    mut state: Local<(Option<f32>, usize, u64)>,
) {
    let Some(mut bank) = bank else { return };
    let (wait, count, seed) = &mut *state;
    if *seed == 0 {
        *seed = 0xB0_6A11;
    }
    let level = spec.volume("music") * settings.volume * settings.music;
    if let Some(mut sink) = playing.iter_mut().next() {
        sink.set_volume(Volume::Linear(level));
        return;
    }
    let wait = wait.get_or_insert(spec.music.first);
    *wait -= time.delta_secs();
    if *wait > 0.0 {
        return;
    }
    let (lo, hi) = spec.music.gap;
    *wait = lo + (hi - lo) * ((jitter(seed) - 0.9) * 5.0);
    let night = crate::creatures::brain::spirit_hours(clock.hour());
    let list = if night && !spec.music.night.is_empty() { &spec.music.night } else { &spec.music.day };
    if list.is_empty() || level <= 0.001 {
        return;
    }
    let name = &list[*count % list.len()];
    *count += 1;
    let h = bank.handles.entry(name.clone()).or_insert_with(|| server.load(format!("sounds/{name}.ogg"))).clone();
    commands.spawn((Music, AudioPlayer(h), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(level))));
}

/// The music track playing.
#[derive(Component)]
struct Music;

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
        out.write(PlaySound::new("spirit").gain(1.0 - nearest / 30.0));
    }
}

/// Loops follow the weather, height and spells.
#[allow(clippy::too_many_arguments)]
fn loops(
    time: Res<Time>,
    conditions: Res<Conditions>,
    player: Res<Player>,
    spells: Res<Spells>,
    settings: Res<Settings>,
    spec: Res<SoundSpec>,
    mut commands: Commands,
    bank: Option<Res<Bank>>,
    sources: Res<Assets<AudioSource>>,
    mut muffled_sounds: ResMut<Assets<Muffled>>,
    mut started: Local<[bool; 3]>,
    mut q: Query<(&Loop, &mut AudioSink)>,
) {
    // Each loop starts once its file has loaded (it needs the samples for its filter).
    if let Some(bank) = bank {
        for (i, l) in [Loop::Wind, Loop::Rain, Loop::Levitate].into_iter().enumerate() {
            if let Some(source) = bank.handles.get(l.file()).and_then(|h| sources.get(h))
                && !started[i]
            {
                started[i] = true;
                let (sound, filter) = muffled(&mut muffled_sounds, source, None);
                commands.spawn((l, sound, filter, PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0))));
            }
        }
    }
    let dt = time.delta_secs();
    let high = ((player.pos.y - 80.0) / 200.0).clamp(0.0, 1.0);
    let rain = if conditions.snowing { 0.0 } else { conditions.precipitation.clamp(0.0, 1.0) };
    for (l, mut sink) in &mut q {
        let level = match l {
            Loop::Wind => ((conditions.wind / 20.0).clamp(0.0, 1.0) * 0.8 + high * 0.6).min(1.0),
            Loop::Rain => rain,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_pass_cuts_highs_and_keeps_lows() {
        let rate = 44100.0;
        assert_eq!(lowpass_alpha(OPEN, rate), 1.0);
        let wall = lowpass_alpha(WALL_CUTOFF, rate);
        let water = lowpass_alpha(WATER_CUTOFF, rate);
        assert!(water < wall && wall < 1.0);
        // Run a tone through the two-stage filter and measure what comes out.
        let gain = |alpha: f32, hz: f32| {
            let (mut a, mut b, mut peak) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..44100 {
                let x = (std::f32::consts::TAU * hz * i as f32 / rate).sin();
                a += alpha * (x - a);
                b += alpha * (a - b);
                if i > 22050 {
                    peak = peak.max(b.abs());
                }
            }
            peak
        };
        assert!(gain(water, 100.0) > 0.8, "lows pass under water");
        assert!(gain(water, 4000.0) < 0.05, "highs gone under water");
        assert!(gain(wall, 4000.0) > gain(water, 4000.0) * 5.0, "a wall muffles less than water");
    }

    #[test]
    fn positioned_sounds_fade_with_distance() {
        assert_eq!(distance_fade(2.0), 1.0);
        assert!(distance_fade(20.0) > 0.0 && distance_fade(20.0) < 1.0);
        assert_eq!(distance_fade(FAR + 1.0), 0.0);
    }

    #[test]
    fn far_thunder_comes_later() {
        assert!(thunder_delay(0.0) < 1.0);
        assert!(thunder_delay(1.0) > thunder_delay(0.5));
    }

    #[test]
    fn sounds_ron_lists_music_that_exists() {
        let spec: SoundSpec = ron::from_str(include_str!("../../../assets/data/sounds.ron")).unwrap();
        assert!(!spec.music.day.is_empty() && !spec.music.night.is_empty());
        for name in spec.music.day.iter().chain(&spec.music.night) {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds").join(format!("{name}.ogg"));
            assert!(path.is_file(), "missing {}", path.display());
        }
        assert!(spec.music.gap.0 <= spec.music.gap.1);
    }
}
