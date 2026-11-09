//! Creature kinds from `assets/data/creatures.ron`. Plain Rust, no Bevy
//! systems, so a server can read the same file later.

use serde::Deserialize;

use super::skin::{CreatureBox, Layout};
use crate::items::{ItemId, Items};
use crate::player::DamageCause;

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temper {
    /// Runs from you.
    Skittish,
    /// Minds its own business until hit, then fights back.
    Neutral,
    /// Hunts you.
    Hostile,
    /// Drifts through walls at night, kept back by torchlight.
    Spirit,
    /// Lives in a village, keeps a daily schedule.
    Villager,
    /// Small ambient life (birds, frogs, insects): moved by `critters`, not fought.
    Critter,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Day,
    Night,
    Any,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Surface,
    Barrow,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Blunt,
    Piercing,
    Chill,
}

impl From<Cause> for DamageCause {
    fn from(c: Cause) -> Self {
        match c {
            Cause::Blunt => DamageCause::Blunt,
            Cause::Piercing => DamageCause::Piercing,
            Cause::Chill => DamageCause::Chill,
        }
    }
}

#[derive(Deserialize, Debug, Clone, Copy)]
pub struct Attack {
    pub damage: f32,
    pub cause: Cause,
    pub reach: f32,
    pub every: f32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Spawn {
    pub biomes: Vec<String>,
    pub when: When,
    pub at: At,
    pub group: (u8, u8),
    pub weight: f32,
}

/// How a critter gets about.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// Circles high over a wide area in flocks, now and then lands, hops and calls.
    Bird,
    /// Sits on the ground and hops now and then (frogs, crickets).
    Hopper,
    /// Hovers and drifts a little above the ground or water (fireflies, moths, darters).
    Drifter,
}

fn default_call_every() -> (f32, f32) {
    (4.0, 12.0)
}

fn default_hearing() -> f32 {
    32.0
}

/// A critter's ways (`critter:` in creatures.ron). Its `spawn` rule still says where and when.
#[derive(Deserialize, Debug, Clone)]
pub struct CritterSpec {
    pub motion: Motion,
    /// Most of this kind around you at once.
    pub count: usize,
    /// Spawns this far from you, (min, max) blocks; gone beyond 1.6 x max.
    pub ring: (f32, f32),
    /// Sounds it makes (file names in assets/sounds/), one picked per call.
    #[serde(default)]
    pub calls: Vec<String>,
    /// Seconds between calls, (min, max).
    #[serde(default = "default_call_every")]
    pub call_every: (f32, f32),
    /// Heard up to this many blocks away.
    #[serde(default = "default_hearing")]
    pub hearing: f32,
    /// Bird: flying height over the ground. Drifter: hovering height. (min, max) blocks.
    #[serde(default)]
    pub height: (f32, f32),
    /// Hopper: a hop's (along, up) speed in blocks/s.
    #[serde(default)]
    pub hop: (f32, f32),
    /// Seconds between hops (Hopper), perched (Bird) or hovering still (Drifter), (min, max).
    #[serde(default)]
    pub rest: (f32, f32),
    /// Startles when you come this close (half that while crouching); 0 = never.
    #[serde(default)]
    pub shy: f32,
    /// Only spawns within a few blocks of water.
    #[serde(default)]
    pub near_water: bool,
    /// Leaves when the local climate is colder than this (-1..1 like biome temperatures;
    /// below 0 rain turns to snow).
    #[serde(default)]
    pub min_temp: Option<f32>,
    /// Leaves in rain or snow.
    #[serde(default)]
    pub fair_weather: bool,
    /// Drifter: flutters round a torch or other light nearby.
    #[serde(default)]
    pub to_light: bool,
}

#[derive(Deserialize, Debug, Clone)]
struct KindSpec {
    name: String,
    title: String,
    temper: Temper,
    health: f32,
    speed: f32,
    run: f32,
    size: (f32, f32),
    model: Vec<CreatureBox>,
    #[serde(default)]
    skin: Option<String>,
    #[serde(default)]
    skins: Vec<String>,
    #[serde(default)]
    attack: Option<Attack>,
    #[serde(default)]
    spawn: Option<Spawn>,
    drops: Vec<(String, u16, f32)>,
    #[serde(default)]
    glow: Option<(f32, f32, f32)>,
    #[serde(default)]
    critter: Option<CritterSpec>,
}

#[derive(Debug, Clone)]
pub struct Kind {
    pub name: String,
    pub title: String,
    pub temper: Temper,
    pub health: f32,
    pub speed: f32,
    pub run: f32,
    /// Half the body's width, and its height.
    pub half: f32,
    pub height: f32,
    pub model: Vec<CreatureBox>,
    /// Skin PNGs (names under `assets/textures/creatures/`), one picked per creature.
    /// Empty: drawn in the boxes' flat colours.
    pub skins: Vec<String>,
    /// Where each box sits in the skins.
    pub layout: Layout,
    pub attack: Option<Attack>,
    pub spawn: Option<Spawn>,
    pub drops: Vec<(ItemId, u16, f32)>,
    pub glow: Option<(f32, f32, f32)>,
    pub critter: Option<CritterSpec>,
}

#[derive(bevy::prelude::Resource, Debug, Clone)]
pub struct Kinds {
    pub list: Vec<Kind>,
}

impl Kinds {
    pub fn from_ron(text: &str, items: &Items) -> Result<Self, String> {
        let specs: Vec<KindSpec> = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
            .map_err(|e| format!("creatures.ron: {e}"))?;
        let mut list = Vec::new();
        for s in specs {
            let drops = s
                .drops
                .iter()
                .map(|(i, n, p)| items.id(i).map(|id| (id, *n, *p)).ok_or_else(|| format!("creatures.ron: '{}' drops unknown item '{i}'", s.name)))
                .collect::<Result<Vec<_>, String>>()?;
            if s.model.is_empty() {
                return Err(format!("creatures.ron: '{}' has no model", s.name));
            }
            let mut skins: Vec<String> = s.skin.into_iter().collect();
            skins.extend(s.skins);
            if (s.temper == Temper::Critter) != s.critter.is_some() {
                return Err(format!("creatures.ron: '{}' needs both `temper: Critter` and `critter: (...)`, or neither", s.name));
            }
            let layout = Layout::new(&s.model);
            list.push(Kind {
                name: s.name,
                title: s.title,
                temper: s.temper,
                health: s.health,
                speed: s.speed,
                run: s.run,
                half: s.size.0 * 0.5,
                height: s.size.1,
                model: s.model,
                skins,
                layout,
                attack: s.attack,
                spawn: s.spawn,
                drops,
                glow: s.glow,
                critter: s.critter,
            });
        }
        Ok(Self { list })
    }

    pub fn id(&self, name: &str) -> Option<usize> {
        self.list.iter().position(|k| k.name == name)
    }

    pub fn get(&self, id: usize) -> &Kind {
        &self.list[id]
    }

    /// Kinds that can appear on the surface here and now, with their weights
    /// (critters have their own spawner; a weight of 0 turns a kind off).
    pub fn candidates(&self, biome: &str, night: bool) -> Vec<(usize, f32)> {
        self.list
            .iter()
            .enumerate()
            .filter_map(|(i, k)| {
                let s = k.spawn.as_ref().filter(|s| s.weight > 0.0 && k.critter.is_none())?;
                (time_ok(s.when, night) && s.at == At::Surface && s.lives_in(biome)).then_some((i, s.weight))
            })
            .collect()
    }
}

/// Does a `when` rule fit the time of day?
pub fn time_ok(when: When, night: bool) -> bool {
    match when {
        When::Day => !night,
        When::Night => night,
        When::Any => true,
    }
}

impl Spawn {
    pub fn lives_in(&self, biome: &str) -> bool {
        self.biomes.is_empty() || self.biomes.iter().any(|b| b == biome)
    }
}

/// Pick one entry by weight, with `r` in 0..1.
pub fn pick(list: &[(usize, f32)], r: f32) -> Option<usize> {
    let total: f32 = list.iter().map(|(_, w)| w).sum();
    if total <= 0.0 {
        return None;
    }
    let mut x = r * total;
    for (i, w) in list {
        if x < *w {
            return Some(*i);
        }
        x -= w;
    }
    list.last().map(|(i, _)| *i)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn load() -> Kinds {
        let (_, items) = crate::items::tests::load();
        Kinds::from_ron(include_str!("../../../../assets/data/creatures.ron"), &items).unwrap()
    }

    #[test]
    fn creatures_load_and_drop_real_items() {
        let k = load();
        for name in ["marsh_boar", "fen_stag", "bog_lurker", "ancestral_spirit", "barrow_wight", "villager"] {
            assert!(k.id(name).is_some(), "{name}");
        }
        let stag = k.get(k.id("fen_stag").unwrap());
        assert!(!stag.drops.is_empty() && stag.temper == Temper::Skittish);
    }

    #[test]
    fn hunters_at_night_and_animals_by_day() {
        let k = load();
        let lurker = k.id("bog_lurker").unwrap();
        let boar = k.id("marsh_boar").unwrap();
        let day = k.candidates("great_swamp", false);
        let night = k.candidates("great_swamp", true);
        assert!(day.iter().any(|(i, _)| *i == boar) && !day.iter().any(|(i, _)| *i == lurker));
        assert!(night.iter().any(|(i, _)| *i == lurker) && !night.iter().any(|(i, _)| *i == boar));
        // Wights only come from barrows, never the open surface.
        let wight = k.id("barrow_wight").unwrap();
        assert!(!night.iter().any(|(i, _)| *i == wight));
        // Spirits are turned off for now (weight 0) but still walk only at night.
        let spirit = k.get(k.id("ancestral_spirit").unwrap());
        assert_eq!(spirit.spawn.as_ref().unwrap().when, When::Night);
        assert!(!night.iter().chain(&day).any(|(i, _)| k.get(*i).name == "ancestral_spirit"));
        // Critters never come from the creature ring.
        assert!(night.iter().chain(&day).all(|(i, _)| k.get(*i).critter.is_none()));
    }

    #[test]
    fn critters_are_listed_and_sound_real() {
        let k = load();
        let sounds = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds");
        for name in ["marsh_finch", "reed_frog", "dusk_cricket", "firefly", "lamp_moth", "bog_darter"] {
            let c = k.get(k.id(name).unwrap_or_else(|| panic!("{name}")));
            let spec = c.critter.as_ref().unwrap();
            assert!(c.temper == Temper::Critter && c.spawn.is_some() && spec.count > 0 && spec.ring.0 < spec.ring.1, "{name}");
            for s in &spec.calls {
                assert!(sounds.join(format!("{s}.ogg")).exists(), "{name} calls {s}.ogg, which doesn't exist");
            }
        }
    }

    #[test]
    fn every_listed_skin_exists_and_fits_its_layout() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures/creatures");
        for k in &load().list {
            for s in &k.skins {
                let (w, h) = image::image_dimensions(dir.join(format!("{s}.png"))).unwrap_or_else(|e| panic!("{s}.png: {e}"));
                // Painted at a higher resolution is fine, as long as it's the same shape.
                assert!(w % k.layout.width == 0 && w * k.layout.height == h * k.layout.width, "{s}.png is {w}x{h}, layout is {}x{}", k.layout.width, k.layout.height);
            }
        }
    }

    #[test]
    fn weighted_pick() {
        let l = [(0, 1.0), (1, 3.0)];
        assert_eq!(pick(&l, 0.1), Some(0));
        assert_eq!(pick(&l, 0.5), Some(1));
        assert_eq!(pick(&l, 0.999), Some(1));
        assert_eq!(pick(&[], 0.5), None);
    }
}
