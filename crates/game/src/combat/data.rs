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

/// A foe's part in a group fight (`ai` weighs its tactics by it; `combat`
/// moves it by it).
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    /// Fodder in packs: simple cuts, a weak guard.
    #[default]
    Grunt,
    /// Blocks everything from the front (a tower shield); stands in front of
    /// shooters. Needs a heavy, a kick, a hammer or a flank.
    Shield,
    /// Shoots from range and keeps its distance.
    Archer,
    /// Hits hard and slow: every attack is heavy (unblockable, flashing red).
    Brute,
    /// Fast: circles round to your back while the others hold you.
    Skirmisher,
    /// Keeps back and mends the others (`MEND`): kill it first.
    Support,
}

/// What makes an elite stronger than the rest of its kind (one each). Elites
/// also have more health (`ELITE_HEALTH`), hit harder (`ELITE_DAMAGE`), stand
/// a little bigger, glow gold and drop better loot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Elite {
    /// Its blows land half again as hard.
    Fierce,
    /// Foes near it take half damage (a ward round it).
    Warded,
    /// It swings a third faster.
    Frenzied,
}

pub const ELITE_HEALTH: f32 = 1.8;
pub const ELITE_DAMAGE: f32 = 1.25;
pub const ELITE_SCALE: f32 = 1.12;
/// How far an elite's ward reaches.
pub const WARD_REACH: f32 = 6.0;

impl Elite {
    pub const ALL: [Elite; 3] = [Elite::Fierce, Elite::Warded, Elite::Frenzied];

    /// Pick one from a number in 0..1.
    pub fn pick(u: f32) -> Elite {
        Elite::ALL[((u * 3.0) as usize).min(2)]
    }

    pub fn title(self) -> &'static str {
        match self {
            Elite::Fierce => "Fierce",
            Elite::Warded => "Warded",
            Elite::Frenzied => "Frenzied",
        }
    }

    /// Multiplier on its damage (with `ELITE_DAMAGE`).
    pub fn damage(self) -> f32 {
        ELITE_DAMAGE * if self == Elite::Fierce { 1.5 } else { 1.0 }
    }

    /// Multiplier on how long its swings take.
    pub fn haste(self) -> f32 {
        if self == Elite::Frenzied { 0.67 } else { 1.0 }
    }
}

/// A foe that fights from afar: its "cut" throws a bolt that flies at you
/// (`reach` is then how far it shoots from).
#[derive(Deserialize, Debug, Clone, Copy)]
pub struct Ranged {
    /// Blocks per second the bolt flies.
    pub speed: f32,
    /// Colour of the bolt (r, g, b 0..1).
    pub color: (f32, f32, f32),
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
    /// Shoots instead of cutting.
    #[serde(default)]
    pub ranged: Option<Ranged>,
    /// A shooter that cuts instead when you are this close (court soldiers).
    #[serde(default)]
    pub close: Option<f32>,
    /// Its part in a group fight.
    #[serde(default)]
    pub role: Role,
    /// What it holds in its left hand (a shield bearer's shield).
    #[serde(default)]
    pub offhand: Option<String>,
    /// A boss: its phases and named attacks (`combat::boss`).
    #[serde(default)]
    pub boss: Option<BossDef>,
}

/// A boss's phases and attacks.
#[derive(Deserialize, Debug, Clone)]
pub struct BossDef {
    /// Health shares (0..1, highest first) where a new phase begins.
    pub phases: Vec<f32>,
    pub attacks: Vec<BossAttack>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct BossAttack {
    /// Shown on the boss bar while it comes.
    pub name: String,
    /// The first phase (0 = from the start) that uses it.
    #[serde(default)]
    pub phase: u8,
    /// Seconds between uses.
    pub every: f32,
    /// Seconds of warning before it lands.
    pub wind: f32,
    pub kind: AttackKind,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum AttackKind {
    /// A blow to the ground: hurts everything within `radius` on the ground.
    Slam { radius: f32, damage: f32 },
    /// A fan of `count` bolts at you.
    Volley { count: u8, damage: f32 },
    /// A rush straight at you for a second.
    Charge { speed: f32, damage: f32 },
    /// Calls `count` foes of `kind` (foes.ron) to its side.
    Summon { kind: String, count: u8 },
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
        // Every role has a kind, and shield bearers carry a shield.
        for r in [super::Role::Grunt, super::Role::Shield, super::Role::Archer, super::Role::Brute, super::Role::Skirmisher, super::Role::Support] {
            assert!(kinds.iter().any(|k| k.role == r), "{r:?}");
        }
        assert!(kinds.iter().filter(|k| k.role == super::Role::Shield).all(|k| k.offhand.is_some()));
    }

    #[test]
    fn elites_pick_every_kind() {
        use super::Elite;
        let seen: std::collections::HashSet<_> = (0..30).map(|i| Elite::pick(i as f32 / 30.0)).collect();
        assert_eq!(seen.len(), 3);
        assert_eq!(Elite::pick(1.0), Elite::Frenzied);
        assert!(Elite::Fierce.damage() > Elite::Warded.damage() && Elite::Frenzied.haste() < 1.0);
    }
}
