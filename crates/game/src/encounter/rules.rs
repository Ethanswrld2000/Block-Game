//! Encounter rules (pure): the data, and a running encounter's life.

use serde::{Deserialize, Serialize};

/// You must be this close to a reset encounter's spot for its foes to come back.
pub const RETURN: f32 = 48.0;
/// Seconds after you are back (and alive) before a reset encounter's foes come again.
pub const REGROUP: f32 = 3.0;
/// Seconds the airship takes to come down before the foes drop.
pub const DESCENT: f32 = 5.0;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Def {
    pub id: String,
    /// foes.ron kinds and how many of each.
    pub foes: Vec<(String, u32)>,
    /// Where they come from.
    pub from: From,
    #[serde(default)]
    pub then: Then,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum From {
    /// In a ring this many blocks around where it starts (you, or a cutscene's mark).
    Around(f32),
    /// Around a named place (`cutscene::Places`: camp, gate, sporrow, castle, forge).
    Place(String, f32),
    /// An airship comes down over the spot and drops them (ring radius).
    Airship(f32),
}

/// What beating them all does.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct Then {
    #[serde(default)]
    pub flag: Option<String>,
    #[serde(default)]
    pub cutscene: Option<String>,
}

pub fn load(text: &str) -> Result<Vec<Def>, String> {
    let ron =
        ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let defs: Vec<Def> = ron.from_str(text).map_err(|e| e.to_string())?;
    for d in &defs {
        if d.foes.is_empty() || d.foes.iter().all(|(_, n)| *n == 0) {
            return Err(format!("encounter `{}` has no foes", d.id));
        }
    }
    Ok(defs)
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// Waiting this long before they come (an airship coming down, or regrouping after a reset).
    Coming(f32),
    /// Reset by your death: waits for you to come back alive.
    Reset,
    Fighting,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Running {
    pub id: String,
    /// Where it happens.
    pub at: [f32; 3],
    pub total: u32,
    pub left: u32,
    pub phase: Phase,
    /// Drops from an airship.
    pub ship: bool,
    /// The ship has been sent for this wave.
    #[serde(default)]
    pub ship_sent: bool,
}

/// What to do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Go {
    /// Send the airship down.
    Ship,
    /// Spawn every foe.
    Spawn,
}

impl Running {
    pub fn start(def: &Def, at: [f32; 3]) -> Self {
        let total = def.foes.iter().map(|(_, n)| n).sum();
        let ship = matches!(def.from, From::Airship(_));
        Self {
            id: def.id.clone(),
            at,
            total,
            left: total,
            phase: Phase::Coming(0.0),
            ship,
            ship_sent: false,
        }
    }

    /// Time passes; `ready` = you are alive and near enough. Says when to send the ship or spawn.
    pub fn tick(&mut self, dt: f32, ready: bool) -> Option<Go> {
        match self.phase {
            Phase::Fighting => None,
            Phase::Reset => {
                if ready {
                    self.phase = Phase::Coming(-REGROUP);
                }
                None
            }
            Phase::Coming(t) => {
                if !ready {
                    return None;
                }
                if self.ship && !self.ship_sent && t >= 0.0 {
                    self.ship_sent = true;
                    self.phase = Phase::Coming(t + dt);
                    return Some(Go::Ship);
                }
                let need = if self.ship { DESCENT } else { 0.0 };
                if t >= need {
                    self.phase = Phase::Fighting;
                    self.left = self.total;
                    return Some(Go::Spawn);
                }
                self.phase = Phase::Coming(t + dt);
                None
            }
        }
    }

    /// `n` of its foes fell. True when that was the last.
    pub fn slain(&mut self, n: u32) -> bool {
        if self.phase != Phase::Fighting {
            return false;
        }
        self.left = self.left.saturating_sub(n);
        self.left == 0
    }

    /// You died: everything resets, and comes again when you're back.
    pub fn died(&mut self) {
        self.phase = Phase::Reset;
        self.left = self.total;
        self.ship_sent = false;
    }

    /// Loaded from a save: the foes weren't saved, so it comes again.
    pub fn restart(&mut self) {
        self.died();
    }
}

/// The drop ship's height over its spot `t` seconds after it was sent; `None` once gone (pure).
pub fn ship_height(t: f32) -> Option<f32> {
    let hover = 7.0;
    if t < DESCENT {
        let k = t / DESCENT;
        Some(hover + (40.0 - hover) * (1.0 - k) * (1.0 - k))
    } else if t < DESCENT + 2.5 {
        Some(hover)
    } else if t < DESCENT + 9.0 {
        let k = (t - DESCENT - 2.5) / 6.5;
        Some(hover + 60.0 * k * k)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(from: From) -> Def {
        Def {
            id: "test".into(),
            foes: vec![("rot_brute".into(), 2), ("mire_skitter".into(), 1)],
            from,
            then: Then::default(),
        }
    }

    #[test]
    fn spawns_then_clears_when_all_fall() {
        let mut r = Running::start(&def(From::Around(6.0)), [0.0; 3]);
        assert_eq!(r.total, 3);
        // Not while you are away or dead.
        assert_eq!(r.tick(0.1, false), None);
        assert_eq!(r.tick(0.1, true), Some(Go::Spawn));
        assert_eq!(r.tick(0.1, true), None, "once");
        assert!(!r.slain(1));
        assert!(!r.slain(1));
        assert!(r.slain(1), "the last one");
    }

    #[test]
    fn dying_resets_it() {
        let mut r = Running::start(&def(From::Around(6.0)), [0.0; 3]);
        r.tick(0.1, true);
        r.slain(2);
        r.died();
        assert_eq!(r.left, 3);
        // Kills while reset don't count.
        assert!(!r.slain(3));
        // Back and alive: a short wait, then all of them again.
        assert_eq!(r.tick(0.1, false), None);
        assert_eq!(r.tick(0.1, true), None);
        let mut got = None;
        for _ in 0..40 {
            got = got.or(r.tick(0.1, true));
        }
        assert_eq!(got, Some(Go::Spawn));
        assert_eq!(r.left, 3);
    }

    #[test]
    fn airship_comes_down_before_they_drop() {
        let mut r = Running::start(&def(From::Airship(3.0)), [0.0; 3]);
        assert_eq!(r.tick(0.1, true), Some(Go::Ship));
        let mut spawned_at = None;
        for i in 0..100 {
            if r.tick(0.1, true) == Some(Go::Spawn) {
                spawned_at = Some(i as f32 * 0.1);
                break;
            }
        }
        let t = spawned_at.expect("they drop");
        assert!(t >= DESCENT - 0.2, "only once the ship is down ({t})");
        assert_eq!(ship_height(0.0), Some(40.0));
        assert!(ship_height(DESCENT + 1.0).unwrap() < 8.0);
        assert_eq!(ship_height(30.0), None);
    }

    #[test]
    fn the_data_file_loads_with_real_foes() {
        let defs =
            load(include_str!("../../../../assets/data/encounters.ron")).expect("encounters.ron");
        let foes = include_str!("../../../../assets/data/foes.ron");
        for d in &defs {
            for (k, _) in &d.foes {
                assert!(
                    foes.contains(&format!("name: \"{k}\"")),
                    "{}: no foe {k}",
                    d.id
                );
            }
        }
    }
}
