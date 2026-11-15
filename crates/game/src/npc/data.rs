//! NPC kinds from `assets/data/npcs.ron`: who they are, how they look and
//! sound, and which dialogue tree they use. Pure, no Bevy.

use serde::Deserialize;

/// An sRGB colour, 0..1 per channel.
pub type Rgb = (f32, f32, f32);

#[derive(Deserialize, Clone, Debug)]
pub struct Looks {
    pub skin: Rgb,
    pub hair: Rgb,
    pub shirt: Rgb,
    /// Scarf or collar round the neck.
    pub scarf: Rgb,
    pub trousers: Rgb,
    pub boots: Rgb,
    /// The coloured ring round each pupil.
    pub iris: Rgb,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Voice {
    /// Playback speed of the voice sounds: 1 = as recorded, higher = higher voice.
    pub pitch: f32,
    /// Syllables per second.
    pub rate: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Kind {
    /// Shown in the talk bars.
    pub name: String,
    #[serde(default)]
    pub title: String,
    /// Key of its tree in dialogue.ron.
    pub dialogue: String,
    pub voice: Voice,
    pub looks: Looks,
    /// Walking speed, blocks per second (it jogs at about twice this).
    #[serde(default = "walk_speed")]
    pub walk: f32,
}

fn walk_speed() -> f32 {
    2.4
}

pub fn load(text: &str) -> Result<Vec<Kind>, String> {
    ron::from_str(text).map_err(|e| e.to_string())
}
