//! Item quality and traits (`assets/data/traits.ron`). Every weapon, tool,
//! shield and piece of armor can come in one of four qualities on top of its
//! material: plain, fine, masterwork, relic. Better ones hit harder (or keep
//! more off), cost less Fatigue to swing, and carry traits rolled when the
//! item is made or found: minor (reach, speed, lighter, knockback, backstab),
//! major (bleed, frost, burn, lifesteal, guard-breaker) and one relic trait
//! on relics.
//!
//! An item with a quality is its own item, named `base@quality+trait+trait`
//! (`iron_sword@fine+reach`), made on demand by `Items` (so stacks, saves and
//! the hotbar keep working by name). Pure: no Bevy.

use serde::Deserialize;

use crate::creatures::brain::Rng;

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Quality {
    #[default]
    Plain,
    Fine,
    Masterwork,
    Relic,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::Plain, Quality::Fine, Quality::Masterwork, Quality::Relic];

    pub fn key(self) -> &'static str {
        match self {
            Quality::Plain => "plain",
            Quality::Fine => "fine",
            Quality::Masterwork => "masterwork",
            Quality::Relic => "relic",
        }
    }

    pub fn from_key(s: &str) -> Option<Quality> {
        Quality::ALL.into_iter().find(|q| q.key() == s)
    }

    /// One step up (the anvil's upgrade); relics are only found.
    pub fn next(self) -> Option<Quality> {
        match self {
            Quality::Plain => Some(Quality::Fine),
            Quality::Fine => Some(Quality::Masterwork),
            _ => None,
        }
    }
}

/// What one quality does.
#[derive(Deserialize, Debug, Clone)]
pub struct Tier {
    pub quality: Quality,
    pub title: String,
    /// Name and tooltip colour (r, g, b 0..1).
    pub color: (f32, f32, f32),
    /// Share more damage (weapons) or protection (armor, shields).
    pub power: f32,
    /// Share off the Fatigue each swing or block costs.
    pub stamina: f32,
    /// Traits rolled of each rank.
    pub minor: u8,
    pub major: u8,
    pub relic: u8,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rank {
    Minor,
    Major,
    Relic,
}

/// What a trait does.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// Blocks more reach.
    Reach(f32),
    /// Share faster swings.
    Speed(f32),
    /// Share off the Fatigue cost.
    Lighter(f32),
    /// Share harder push.
    Knockback(f32),
    /// Extra backstab multiplier (added to `ai::BACKSTAB`).
    Backstab(f32),
    /// Damage per second for `ailment::BLEED_FOR` seconds.
    Bleed(f32),
    /// Share slower for `ailment::FROST_FOR` seconds.
    Frost(f32),
    /// Damage per second for `ailment::BURN_FOR` seconds.
    Burn(f32),
    /// Share of the damage dealt that heals you.
    Lifesteal(f32),
    /// Every blow breaks a raised guard.
    GuardBreaker,
    /// A perfect parry refills your Fatigue.
    ParryRefill,
    /// A kill sparks to the next foe within 6 blocks for this much.
    ChainSpark(f32),
    /// A parry sends the blow back at this multiple.
    Riposte(f32),
}

#[derive(Deserialize, Debug, Clone)]
pub struct TraitDef {
    pub name: String,
    pub title: String,
    pub rank: Rank,
    pub effect: Effect,
    /// The tooltip line.
    pub text: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Qualities {
    pub tiers: Vec<Tier>,
    pub traits: Vec<TraitDef>,
}

/// Odds of each quality, plain to relic (they needn't add to 1).
pub type Odds = [f32; 4];

impl Qualities {
    pub fn load(text: &str) -> Result<Self, String> {
        let q: Qualities = ron::from_str(text).map_err(|e| format!("traits.ron: {e}"))?;
        for (i, want) in Quality::ALL.iter().enumerate() {
            if q.tiers.get(i).map(|t| t.quality) != Some(*want) {
                return Err(format!("traits.ron: tier {i} should be {want:?}"));
            }
        }
        for t in &q.traits {
            if t.name.contains(['@', '+']) {
                return Err(format!("traits.ron: trait name '{}' can't hold @ or +", t.name));
            }
        }
        Ok(q)
    }

    pub fn tier(&self, q: Quality) -> &Tier {
        &self.tiers[q as usize]
    }

    pub fn trait_named(&self, name: &str) -> Option<usize> {
        self.traits.iter().position(|t| t.name == name)
    }

    /// Roll a quality from odds (plain, fine, masterwork, relic).
    pub fn roll_quality(odds: Odds, rng: &mut Rng) -> Quality {
        let total: f32 = odds.iter().map(|o| o.max(0.0)).sum();
        if total <= 0.0 {
            return Quality::Plain;
        }
        let mut u = rng.unit() * total;
        for (q, o) in Quality::ALL.into_iter().zip(odds) {
            u -= o.max(0.0);
            if u < 0.0 {
                return q;
            }
        }
        Quality::Relic
    }

    /// Roll the traits an item of quality `q` gets: as many of each rank as
    /// its tier says, never the same one twice.
    pub fn roll_traits(&self, q: Quality, rng: &mut Rng) -> Vec<usize> {
        let tier = self.tier(q);
        let mut out = Vec::new();
        for (rank, n) in [(Rank::Minor, tier.minor), (Rank::Major, tier.major), (Rank::Relic, tier.relic)] {
            let mut pool: Vec<usize> = (0..self.traits.len()).filter(|i| self.traits[*i].rank == rank).collect();
            for _ in 0..n {
                if pool.is_empty() {
                    break;
                }
                let i = (rng.next() % pool.len() as u64) as usize;
                out.push(pool.swap_remove(i));
            }
        }
        out.sort();
        out
    }

    /// The name of an item of `base` with quality `q` and traits.
    pub fn name(&self, base: &str, q: Quality, traits: &[usize]) -> String {
        if q == Quality::Plain && traits.is_empty() {
            return base.to_string();
        }
        let mut s = format!("{base}@{}", q.key());
        for t in traits {
            s.push('+');
            s.push_str(&self.traits[*t].name);
        }
        s
    }

    /// Read `base@quality+trait..` back: (base, quality, traits).
    pub fn parse<'a>(&self, name: &'a str) -> Option<(&'a str, Quality, Vec<usize>)> {
        let (base, rest) = name.split_once('@')?;
        let mut parts = rest.split('+');
        let q = Quality::from_key(parts.next()?)?;
        let mut traits = parts.map(|t| self.trait_named(t)).collect::<Option<Vec<_>>>()?;
        traits.sort();
        traits.dedup();
        Some((base, q, traits))
    }

    /// Everything the traits add up to.
    pub fn sum(&self, traits: &[usize]) -> Tuning {
        let mut t = Tuning::default();
        for e in traits.iter().filter_map(|i| self.traits.get(*i)).map(|d| d.effect) {
            match e {
                Effect::Reach(x) => t.reach += x,
                Effect::Speed(x) => t.speed += x,
                Effect::Lighter(x) => t.lighter += x,
                Effect::Knockback(x) => t.knockback += x,
                Effect::Backstab(x) => t.backstab += x,
                _ => {}
            }
        }
        t
    }
}

/// The plain numbers traits change, summed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tuning {
    pub reach: f32,
    pub speed: f32,
    pub lighter: f32,
    pub knockback: f32,
    pub backstab: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> Qualities {
        Qualities::load(include_str!("../../../assets/data/traits.ron")).unwrap()
    }

    #[test]
    fn traits_ron_loads_with_traits_of_every_rank() {
        let q = load();
        for r in [Rank::Minor, Rank::Major, Rank::Relic] {
            assert!(q.traits.iter().filter(|t| t.rank == r).count() >= 3, "{r:?}");
        }
        // Better tiers are better.
        for w in q.tiers.windows(2) {
            assert!(w[1].power > w[0].power && w[1].stamina >= w[0].stamina);
        }
        assert_eq!(q.tier(Quality::Plain).power, 0.0);
    }

    #[test]
    fn rolled_traits_follow_the_tier() {
        let q = load();
        let mut rng = Rng(77);
        for _ in 0..200 {
            assert!(q.roll_traits(Quality::Plain, &mut rng).is_empty());
            let t = q.roll_traits(Quality::Masterwork, &mut rng);
            assert_eq!(t.len(), 2);
            assert_eq!(t.iter().filter(|i| q.traits[**i].rank == Rank::Minor).count(), 1);
            assert_eq!(t.iter().filter(|i| q.traits[**i].rank == Rank::Major).count(), 1);
            let r = q.roll_traits(Quality::Relic, &mut rng);
            assert_eq!(r.iter().filter(|i| q.traits[**i].rank == Rank::Relic).count(), 1);
        }
    }

    #[test]
    fn names_round_trip() {
        let q = load();
        let mut rng = Rng(5);
        let traits = q.roll_traits(Quality::Relic, &mut rng);
        let name = q.name("iron_sword", Quality::Relic, &traits);
        assert_eq!(q.parse(&name), Some(("iron_sword", Quality::Relic, traits)));
        assert_eq!(q.name("iron_sword", Quality::Plain, &[]), "iron_sword");
        assert_eq!(q.parse("iron_sword"), None);
        assert_eq!(q.parse("iron_sword@shiny"), None);
    }

    #[test]
    fn odds_pick_qualities() {
        let mut rng = Rng(9);
        let mut seen = [0; 4];
        for _ in 0..4000 {
            seen[Qualities::roll_quality([0.5, 0.3, 0.15, 0.05], &mut rng) as usize] += 1;
        }
        assert!(seen[0] > seen[1] && seen[1] > seen[2] && seen[2] > seen[3] && seen[3] > 100, "{seen:?}");
        assert_eq!(Qualities::roll_quality([0.0; 4], &mut rng), Quality::Plain);
    }
}
