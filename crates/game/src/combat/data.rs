//! Foe kinds from `assets/data/foes.ron`: how they look (the NPC rig's
//! colours), what they wield, how hard they hit and how they fight.
//! Plain data, no Bevy.

use serde::Deserialize;

use crate::creatures::data::Cause;
use crate::npc::data::Looks;

/// How a foe fights: the knobs its brain (`ai`) weighs.
#[derive(Deserialize, Debug, Clone, Copy)]
pub struct Style {
    /// 0..1: how keen it is to close in and press attacks.
    pub aggression: f32,
    /// 0..1: how much it circles, watches and backs off.
    pub caution: f32,
    /// 0..1: how likely it is to raise its blade when you swing at it.
    pub guard: f32,
    /// Blows it can block before its guard breaks.
    pub poise: f32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct FoeKind {
    pub name: String,
    pub title: String,
    /// A spirit: see-through and blue, shimmering, trailing wisps.
    #[serde(default)]
    pub ghost: bool,
    pub health: f32,
    /// What it holds (an item name from items.ron).
    pub weapon: String,
    pub damage: f32,
    pub cause: Cause,
    /// Blocks from its feet a cut reaches.
    pub reach: f32,
    /// Seconds one of its swings takes (wind up to rest); slower than yours,
    /// so you can read it coming.
    pub swing: f32,
    pub walk: f32,
    pub run: f32,
    /// Notices you within this many blocks (with a clear line).
    pub sight: f32,
    /// Gives up once you are this far off.
    pub forget: f32,
    /// Never chases further than this from where it started.
    pub leash: f32,
    pub style: Style,
    /// (item, count, chance 0..1) dropped when it falls.
    #[serde(default)]
    pub drops: Vec<(String, u16, f32)>,
    pub looks: Looks,
}

pub fn load(text: &str) -> Result<Vec<FoeKind>, String> {
    ron::from_str(text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn foes_ron_loads_and_makes_sense() {
        let kinds = super::load(include_str!("../../../../assets/data/foes.ron")).unwrap();
        assert!(kinds.iter().any(|k| k.ghost) && kinds.iter().any(|k| !k.ghost));
        for k in &kinds {
            assert!(k.health > 0.0 && k.damage > 0.0 && k.swing > 0.3, "{}", k.name);
            assert!(k.sight < k.forget && k.leash > k.sight, "{}", k.name);
            assert!((0.0..=1.0).contains(&k.style.aggression) && (0.0..=1.0).contains(&k.style.caution), "{}", k.name);
        }
    }
}
