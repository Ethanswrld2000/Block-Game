//! Every voiced line, from `assets/data/lines.ron` (the doc "Legacy: Voice Script"
//! plus the cast's dialogue trees, whose nodes name line ids).
//!
//! A line plays `assets/voice/<id>.ogg` when that file exists; otherwise the
//! speaker's babble (`npc::voice`) stands in. The subtitle always shows.
//! Barks are lines with a `trigger`: `Lines::bark(speaker, trigger, ..)` picks
//! one at random; the `Bark` message says one (a toast plus the voice).

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;
use serde::Deserialize;

use crate::sound::PlaySound;
use crate::survival::Toast;

pub struct LinesPlugin;

impl Plugin for LinesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load())
            .add_message::<Bark>()
            .init_resource::<Babble>()
            .add_systems(crate::menu::GameUpdate, (say_barks, babble).chain());
    }
}

pub fn load() -> Lines {
    Lines::from_ron(include_str!("../../../assets/data/lines.ron"))
        .unwrap_or_else(|e| panic!("lines.ron: {e}"))
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Line {
    pub id: String,
    /// An NPC's dialogue key (npcs.ron), or a story name (`nayphen` is you).
    pub speaker: String,
    pub text: String,
    /// Barks only: when it may be said.
    #[serde(default)]
    pub trigger: Option<String>,
    /// For the actor and the cutscene.
    #[serde(default)]
    pub direction: Option<String>,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct Lines {
    pub list: Vec<Line>,
    by_id: HashMap<String, usize>,
}

impl Lines {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let ron = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
        let list: Vec<Line> = ron.from_str(text).map_err(|e| e.to_string())?;
        let mut by_id = HashMap::new();
        for (i, l) in list.iter().enumerate() {
            if by_id.insert(l.id.clone(), i).is_some() {
                return Err(format!("line id `{}` is used twice", l.id));
            }
        }
        Ok(Self { list, by_id })
    }

    pub fn get(&self, id: &str) -> Option<&Line> {
        self.by_id.get(id).map(|&i| &self.list[i])
    }

    /// The text to show for an id; anything that isn't an id shows as it is.
    pub fn text<'a>(&'a self, id: &'a str) -> &'a str {
        self.get(id).map_or(id, |l| l.text.as_str())
    }

    /// Every bark of a speaker for a trigger.
    pub fn barks(&self, speaker: &str, trigger: &str) -> Vec<&Line> {
        self.list
            .iter()
            .filter(|l| l.speaker == speaker && l.trigger.as_deref() == Some(trigger))
            .collect()
    }

    /// One bark at random (`roll` is any random number).
    pub fn bark(&self, speaker: &str, trigger: &str, roll: u64) -> Option<&Line> {
        let all = self.barks(speaker, trigger);
        (!all.is_empty()).then(|| all[(roll % all.len() as u64) as usize])
    }
}

/// How a line sounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Voice {
    /// The recording (asset path).
    Recorded(String),
    /// No recording yet: the speaker's babble.
    Babble,
}

/// The recording for a line if `exists` says its file is there (pure).
pub fn voice_for(id: &str, exists: impl Fn(&str) -> bool) -> Voice {
    let path = format!("voice/{id}.ogg");
    if exists(&path) {
        Voice::Recorded(path)
    } else {
        Voice::Babble
    }
}

/// Is this asset file on disk (under `BEVY_ASSET_ROOT`/assets)?
pub fn asset_exists(path: &str) -> bool {
    let root = std::env::var_os("BEVY_ASSET_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    root.join("assets").join(path).is_file()
}

/// A line's voice: the recording if there is one.
pub fn voice(id: &str) -> Voice {
    voice_for(id, asset_exists)
}

/// Say a bark: a toast with the line, and its voice.
#[derive(Message, Debug, Clone)]
pub struct Bark {
    pub speaker: String,
    pub trigger: String,
    /// Where the speaker stands (for panning the voice).
    pub at: Option<Vec3>,
}

/// Babble syllables waiting to play: (seconds left, vowel, pitch, where).
#[derive(Resource, Default)]
pub struct Babble(pub Vec<(f32, usize, f32, Option<Vec3>)>);

impl Babble {
    /// Queue the babble for a line spoken at `rate` syllables a second.
    pub fn line(&mut self, text: &str, rate: f32, pitch: f32, at: Option<Vec3>) {
        let speech = crate::npc::voice::speak(text, rate);
        for s in &speech.syllables {
            self.0.push((s.start, s.vowel, pitch, at));
        }
    }
}

/// Play a line's recording now, if it has one. True when it did.
pub fn play_recording(
    commands: &mut Commands,
    server: &AssetServer,
    id: &str,
    volume: f32,
    at: Option<Vec3>,
) -> bool {
    let Voice::Recorded(path) = voice(id) else {
        return false;
    };
    let playback = PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(volume));
    match at {
        Some(at) => commands.spawn((
            AudioPlayer::<AudioSource>(server.load(path)),
            playback.with_spatial(true),
            Transform::from_translation(at),
        )),
        None => commands.spawn((AudioPlayer::<AudioSource>(server.load(path)), playback)),
    };
    true
}

#[allow(clippy::too_many_arguments)]
fn say_barks(
    mut commands: Commands,
    mut barks: MessageReader<Bark>,
    lines: Res<Lines>,
    book: Res<crate::npc::NpcBook>,
    server: Res<AssetServer>,
    settings: Option<Res<crate::settings::Settings>>,
    mut babble: ResMut<Babble>,
    mut toast: MessageWriter<Toast>,
    time: Res<Time<Real>>,
) {
    for b in barks.read() {
        let roll = (time.elapsed_secs_f64() * 1000.0) as u64 ^ 0x9E37_79B9;
        let Some(line) = lines.bark(&b.speaker, &b.trigger, roll) else {
            continue;
        };
        let kind = book.kinds.iter().find(|k| k.dialogue == b.speaker);
        let name = kind.map_or(b.speaker.clone(), |k| k.name.clone());
        toast.write(Toast(format!("{name}: \"{}\"", line.text)));
        let volume = settings.as_ref().map_or(1.0, |s| s.volume);
        if !play_recording(&mut commands, &server, &line.id, volume, b.at) {
            let (rate, pitch) = kind.map_or((12.0, 1.0), |k| (k.voice.rate, k.voice.pitch));
            babble.line(&line.text, rate, pitch, b.at);
        }
    }
}

/// Plays queued babble syllables when their time comes.
fn babble(time: Res<Time<Real>>, mut q: ResMut<Babble>, mut sounds: MessageWriter<PlaySound>) {
    let dt = time.delta_secs();
    q.0.retain_mut(|(t, vowel, pitch, at)| {
        *t -= dt;
        if *t > 0.0 {
            return true;
        }
        sounds.write(PlaySound {
            at: *at,
            ..PlaySound::new(format!("voice_{vowel}")).speed(*pitch)
        });
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_load_and_look_up() {
        let lines = load();
        assert_eq!(
            lines.get("C02_03").map(|l| l.speaker.as_str()),
            Some("bram")
        );
        assert!(lines.text("C03_07").starts_with("If you"));
        // Not an id: shown as it is.
        assert_eq!(lines.text("Just words."), "Just words.");
    }

    #[test]
    fn barks_by_speaker_and_trigger() {
        let lines = load();
        let all = lines.barks("messler", "talk");
        assert_eq!(all.len(), 2);
        for roll in 0..6 {
            let b = lines.bark("messler", "talk", roll).unwrap();
            assert!(all.contains(&b));
        }
        assert_ne!(
            lines.bark("messler", "talk", 0),
            lines.bark("messler", "talk", 1),
            "a different pick"
        );
        assert_eq!(
            lines.bark("messler", "food_burnt", 9).unwrap().id,
            "BARK_MESSLER_01"
        );
        assert!(lines.bark("messler", "no_such_trigger", 0).is_none());
        assert!(lines.bark("nobody", "talk", 0).is_none());
    }

    #[test]
    fn a_recording_plays_else_the_babble() {
        assert_eq!(
            voice_for("C02_03", |p| p == "voice/C02_03.ogg"),
            Voice::Recorded("voice/C02_03.ogg".into())
        );
        assert_eq!(
            voice_for("C02_04", |p| p == "voice/C02_03.ogg"),
            Voice::Babble
        );
        // Nothing is recorded yet.
        assert_eq!(voice("C02_03"), Voice::Babble);
    }

    #[test]
    fn every_dialogue_line_is_in_lines_ron() {
        let lines = load();
        let trees =
            crate::npc::dialogue::load(include_str!("../../../assets/data/dialogue.ron")).unwrap();
        for (key, t) in &trees {
            for (id, n) in &t.nodes {
                for s in &n.say {
                    let l = lines
                        .get(s)
                        .unwrap_or_else(|| panic!("{key}/{id}: `{s}` is not in lines.ron"));
                    assert_eq!(&l.speaker, key, "{s}");
                }
            }
        }
    }
}
