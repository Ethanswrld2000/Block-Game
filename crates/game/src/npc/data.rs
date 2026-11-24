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
    pub trousers: Rgb,
    /// Boots; on a rat, the bare pink skin of its paws, ears, nose and tail.
    pub boots: Rgb,
    /// The coloured ring round each pupil.
    pub iris: Rgb,
    /// What it is: the shape of its body and face.
    #[serde(default)]
    pub body: Species,
    /// Size of the whole body (0.55 = one block tall, for mushroom folk and rats).
    #[serde(default = "one")]
    pub scale: f32,
    /// Extra boxes: a mushroom cap, ears, a snout, a tail, a hat, a robe.
    #[serde(default)]
    pub extras: Vec<Extra>,
    /// No expression: the brows stay flat and the hands still, whatever it says.
    #[serde(default)]
    pub blank: bool,
}

fn one() -> f32 {
    1.0
}

/// The kind of body (`npc::body::build` shapes each its own way; the joints are
/// the same, so every animation works on all of them).
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Species {
    #[default]
    Human,
    /// Short legs, a pear-shaped body, a long snout with whiskers, big round ears, a tail.
    Rat,
    /// Mushroom folk: stubby legs, a stem of a body, a cap (`hair` colour) with gills under it.
    Mushroom,
}

/// Which part of the body an extra box rides on.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum On {
    Head,
    Torso,
    Hips,
}

/// A box added to a body (blocks, in the part's own frame at full size; the model faces -Z).
#[derive(Deserialize, Clone, Debug)]
pub struct Extra {
    pub on: On,
    pub size: (f32, f32, f32),
    pub at: (f32, f32, f32),
    pub color: Rgb,
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
    /// The urn it belongs to (urns.ron name). Asked to follow, it settles by that urn
    /// once you lead it there, and wanders near it from then on (recruited).
    #[serde(default)]
    pub urn: Option<String>,
}

fn walk_speed() -> f32 {
    2.4
}

pub fn load(text: &str) -> Result<Vec<Kind>, String> {
    ron::from_str(text).map_err(|e| e.to_string())
}
