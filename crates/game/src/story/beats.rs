//! Story beats (pure): `assets/data/story.ron`. A beat only checks conditions and
//! starts a cutscene; everything that happens is a cutscene step.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::cutscene::script::{self, Trigger};

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Cond {
    /// The world's first day.
    NewWorld,
    /// This day (0 = the first) or later.
    Day(u32),
    /// The hour is between these (wrapping past midnight).
    Hours(f32, f32),
    /// You are within this many blocks of camp (the bedroll, else the ancestral urn).
    AtCamp(f32),
    /// You have this urn (carried, set down or growing): `carpentry`, `gear`, ...
    Urn(String),
    /// This urn step is done (`carpentry_bench`, ...).
    Step(String),
    /// The Rotting Gate's boss at this depth has been beaten.
    Depth(u8),
    /// You have talked to this NPC (dialogue key).
    Met(String),
    /// This NPC has settled by its urn.
    Recruited(String),
    Flag(String),
    /// This cutscene has played.
    Played(String),
    /// A later day than the one `key` first came true: `flag:x`, `played:x` or `step:x`.
    DayAfter(String),
    /// Within `within` blocks of an NPC, and seeing it if `sight`.
    Near { npc: String, within: f32, sight: bool },
    /// Within `within` blocks of a place (camp, gate, sporrow, castle, forge).
    Area { place: String, within: f32 },
    /// You have made this item at a bench.
    Made(String),
    /// You have placed this block.
    Placed(String),
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Beat {
    pub id: String,
    /// All must hold.
    pub when: Vec<Cond>,
    /// The cutscene it plays (once per world).
    pub play: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Clue {
    pub title: String,
    pub text: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct StoryData {
    pub beats: Vec<Beat>,
    /// Clue id -> what the journal says.
    #[serde(default)]
    pub clues: BTreeMap<String, Clue>,
}

pub fn load(text: &str) -> Result<StoryData, String> {
    let ron = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let data: StoryData = ron.from_str(text).map_err(|e| e.to_string())?;
    for b in &data.beats {
        if b.when.is_empty() {
            return Err(format!("beat `{}` has no conditions", b.id));
        }
        for c in &b.when {
            if let Cond::DayAfter(k) = c
                && !["flag:", "played:", "step:"].iter().any(|p| k.starts_with(p))
            {
                return Err(format!("beat `{}`: DayAfter(\"{k}\") needs flag:, played: or step:", b.id));
            }
        }
    }
    Ok(data)
}

impl StoryData {
    /// Urn steps the beats ask about (so the game only checks those).
    pub fn steps(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for c in self.beats.iter().flat_map(|b| &b.when) {
            match c {
                Cond::Step(s) => {
                    out.insert(s.clone());
                }
                Cond::DayAfter(k) if k.starts_with("step:") => {
                    out.insert(k["step:".len()..].to_string());
                }
                _ => {}
            }
        }
        out
    }

    /// The `DayAfter` keys.
    pub fn dated(&self) -> BTreeSet<String> {
        self.beats.iter().flat_map(|b| &b.when).filter_map(|c| if let Cond::DayAfter(k) = c { Some(k.clone()) } else { None }).collect()
    }
}

/// Everything the beats look at.
#[derive(Default)]
pub struct Facts<'a> {
    /// Where you are, NPCs in sight, places, made/placed, hour, flags, deepest boss beaten.
    pub base: script::Facts<'a>,
    pub day: u32,
    pub urns: BTreeSet<String>,
    pub steps: BTreeSet<String>,
    pub recruited: BTreeSet<String>,
    pub met: Option<&'a BTreeSet<String>>,
    pub played: Option<&'a BTreeSet<String>>,
    pub when: Option<&'a BTreeMap<String, u32>>,
}

fn has(set: Option<&BTreeSet<String>>, x: &str) -> bool {
    set.is_some_and(|s| s.contains(x))
}

pub fn holds(c: &Cond, f: &Facts) -> bool {
    let b = &f.base;
    match c {
        Cond::NewWorld => f.day == 0,
        Cond::Day(d) => f.day >= *d,
        Cond::Hours(from, to) => {
            if from <= to {
                (*from..*to).contains(&b.hour)
            } else {
                b.hour >= *from || b.hour < *to
            }
        }
        Cond::AtCamp(r) => b.places.get("camp").is_some_and(|p| p.distance(b.player) <= *r),
        Cond::Urn(n) => f.urns.contains(n),
        Cond::Step(n) => f.steps.contains(n),
        Cond::Depth(d) => b.boss >= *d,
        Cond::Met(k) => has(f.met, k),
        Cond::Recruited(k) => f.recruited.contains(k),
        Cond::Flag(x) => has(b.flags, x),
        Cond::Played(x) => has(f.played, x),
        Cond::DayAfter(k) => f.when.and_then(|w| w.get(k)).is_some_and(|d| f.day > *d),
        Cond::Near { npc, within, sight } => script::fires(&Trigger::Near { npc: npc.clone(), within: *within, sight: *sight }, b),
        Cond::Area { place, within } => script::fires(&Trigger::Area { place: place.clone(), within: *within }, b),
        Cond::Made(x) => script::fires(&Trigger::Made(x.clone()), b),
        Cond::Placed(x) => script::fires(&Trigger::Placed(x.clone()), b),
    }
}

/// The first beat whose cutscene hasn't played and whose conditions all hold.
pub fn next<'a>(beats: &'a [Beat], f: &Facts) -> Option<&'a Beat> {
    beats.iter().find(|b| !has(f.played, &b.play) && b.when.iter().all(|c| holds(c, f)))
}

/// Remember today for each dated key that is true now and wasn't before.
pub fn note_days(when: &mut BTreeMap<String, u32>, keys: &BTreeSet<String>, day: u32, true_now: impl Fn(&str) -> bool) {
    for k in keys {
        if !when.contains_key(k) && true_now(k) {
            when.insert(k.clone(), day);
        }
    }
}

/// Is a dated key true: `flag:x`, `played:x`, `step:x`.
pub fn key_true(key: &str, flags: &BTreeSet<String>, played: &BTreeSet<String>, step: impl Fn(&str) -> bool) -> bool {
    if let Some(x) = key.strip_prefix("flag:") {
        flags.contains(x)
    } else if let Some(x) = key.strip_prefix("played:") {
        played.contains(x)
    } else if let Some(x) = key.strip_prefix("step:") {
        step(x)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec3;

    fn beat(when: Vec<Cond>, play: &str) -> Beat {
        Beat { id: play.to_lowercase(), when, play: play.into() }
    }

    #[test]
    fn conditions() {
        let flags: BTreeSet<String> = ["fed_messler".to_string()].into();
        let played: BTreeSet<String> = ["C10".to_string()].into();
        let met: BTreeSet<String> = ["bram".to_string()].into();
        let mut f = Facts { day: 3, met: Some(&met), played: Some(&played), ..Default::default() };
        f.base.flags = Some(&flags);
        f.base.hour = 22.0;
        f.base.boss = 1;
        f.base.player = Vec3::new(3.0, 0.0, 0.0);
        f.base.places.insert("camp".into(), Vec3::ZERO);
        f.urns.insert("carpentry".into());
        f.steps.insert("carpentry_bench".into());
        f.recruited.insert("orrin".into());
        assert!(!holds(&Cond::NewWorld, &f));
        assert!(holds(&Cond::Day(3), &f) && !holds(&Cond::Day(4), &f));
        assert!(holds(&Cond::Hours(20.0, 4.0), &f), "night wraps past midnight");
        assert!(!holds(&Cond::Hours(5.0, 11.0), &f));
        assert!(holds(&Cond::AtCamp(5.0), &f) && !holds(&Cond::AtCamp(2.0), &f));
        assert!(holds(&Cond::Urn("carpentry".into()), &f) && !holds(&Cond::Urn("gear".into()), &f));
        assert!(holds(&Cond::Step("carpentry_bench".into()), &f));
        assert!(holds(&Cond::Depth(1), &f) && !holds(&Cond::Depth(3), &f));
        assert!(holds(&Cond::Met("bram".into()), &f) && !holds(&Cond::Met("gilm".into()), &f));
        assert!(holds(&Cond::Recruited("orrin".into()), &f));
        assert!(holds(&Cond::Flag("fed_messler".into()), &f));
        assert!(holds(&Cond::Played("C10".into()), &f));
        f.day = 0;
        assert!(holds(&Cond::NewWorld, &f));
    }

    #[test]
    fn day_after_counts_from_when_it_first_came_true() {
        let flags = BTreeSet::new();
        let played = BTreeSet::new();
        let keys: BTreeSet<String> = ["step:carpentry_bench".to_string()].into();
        let mut when = BTreeMap::new();
        // Not done yet: nothing noted.
        note_days(&mut when, &keys, 4, |k| key_true(k, &flags, &played, |_| false));
        assert!(when.is_empty());
        // Done on day 5.
        note_days(&mut when, &keys, 5, |k| key_true(k, &flags, &played, |_| true));
        // Later checks keep the first day.
        note_days(&mut when, &keys, 6, |k| key_true(k, &flags, &played, |_| true));
        assert_eq!(when.get("step:carpentry_bench"), Some(&5));
        let c = Cond::DayAfter("step:carpentry_bench".into());
        let at = |day| Facts { day, when: Some(&when), ..Default::default() };
        assert!(!holds(&c, &at(5)), "not the same day");
        assert!(holds(&c, &at(6)));
    }

    #[test]
    fn a_beat_plays_once_when_all_hold() {
        let beats = vec![beat(vec![Cond::Flag("a".into()), Cond::Day(2)], "C05"), beat(vec![Cond::Day(1)], "C06")];
        let flags: BTreeSet<String> = ["a".to_string()].into();
        let mut played = BTreeSet::new();
        let mut f = Facts { day: 1, ..Default::default() };
        f.base.flags = Some(&flags);
        f.played = Some(&played);
        // C05 needs day 2: C06 goes first.
        assert_eq!(next(&beats, &f).map(|b| b.play.as_str()), Some("C06"));
        played.insert("C06".to_string());
        let mut f = Facts { day: 2, played: Some(&played), ..Default::default() };
        f.base.flags = Some(&flags);
        assert_eq!(next(&beats, &f).map(|b| b.play.as_str()), Some("C05"));
        played.insert("C05".to_string());
        let mut f = Facts { day: 2, played: Some(&played), ..Default::default() };
        f.base.flags = Some(&flags);
        assert_eq!(next(&beats, &f), None, "each plays once");
    }

    #[test]
    fn story_ron_loads_and_names_real_cutscenes_and_clues() {
        let data = load(include_str!("../../../../assets/data/story.ron")).expect("story.ron");
        let scenes = script::load(include_str!("../../../../assets/data/cutscenes.ron")).expect("cutscenes.ron");
        for b in &data.beats {
            assert!(scenes.iter().any(|c| c.id == b.play), "beat {}: no cutscene {}", b.id, b.play);
        }
        // C01-C16 are each started by a beat or by an encounter.
        let encounters = include_str!("../../../../assets/data/encounters.ron");
        for n in 1..=16 {
            let id = format!("C{n:02}");
            assert!(scenes.iter().any(|c| c.id == id), "no cutscene {id}");
            assert!(data.beats.iter().any(|b| b.play == id) || encounters.contains(&format!("cutscene: \"{id}\"")), "nothing starts {id}");
        }
        // Every clue a cutscene gives has journal text.
        for c in &scenes {
            for s in &c.steps {
                if let script::Step::Clue(id) = s {
                    assert!(data.clues.contains_key(id), "{}: clue {id} has no text", c.id);
                }
            }
        }
    }
}
