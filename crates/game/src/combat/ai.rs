//! A foe's mind: what it is doing (its tactic), how it picks the next one,
//! and how it takes blows. Pure: no Bevy, so it can be tested and later run
//! on a server.
//!
//! Out of the fight it stands guard near home. Once it sees you it comes
//! for you (pathing round walls), and close in it mixes tactics so no two
//! fights go the same: pressing in to cut, circling and watching, raising
//! its blade, backing off when hurt. Several foes take turns: only two
//! press you at once while the rest circle. It gives up when you get far
//! enough away, or when you'd drag it too far from home.

use super::data::{Role, Style};
use crate::creatures::brain::Rng;

/// Within this many blocks it fights; further out it closes the distance.
pub const ENGAGE: f32 = 5.0;
/// How far out it likes to circle.
pub const CIRCLE_AT: f32 = 3.2;
/// At most this many foes press you at the same time.
pub const PRESSERS: usize = 2;
/// Your guard raised less than this long before a cut lands parries it.
pub const PARRY_WINDOW: f32 = 0.25;
/// How far ahead of it (degrees either side) a blade held up covers.
pub const GUARD_ARC: f32 = 70.0;
/// Seconds a kick leaves it reeling.
pub const KICK_STAGGER: f32 = 1.0;
/// A blow on a foe that hasn't noticed you does this many times the damage.
pub const BACKSTAB: f32 = 3.0;
/// Every this many attacks a foe (not a shooter) swings a heavy, unblockable
/// one: slower, flashing red, and it goes through a raised guard. Brutes'
/// attacks are all heavy.
pub const HEAVY_EVERY: u32 = 4;
/// A heavy attack takes this many times as long.
pub const HEAVY_SLOW: f32 = 1.45;
/// Shooters and menders back off when you get closer than this.
pub const KITE: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tactic {
    /// No one to fight: stands about at home.
    Idle,
    /// Walking back home after giving up.
    Return,
    /// Coming for you from further off.
    Approach,
    /// Circling you at a distance, watching for an opening.
    Circle,
    /// Closing in to cut.
    Press,
    /// Swinging (`Brain::swing` says how far through).
    Attack,
    /// A moment to recover after a swing.
    Recover,
    /// Blade raised to block.
    Guard,
    /// Stepping away.
    Backoff,
    /// Knocked off balance: hit, shoved, parried or its guard broken.
    Stagger,
}

impl Tactic {
    /// Pressing you or in the middle of a swing (counts against `PRESSERS`).
    pub fn pressing(self) -> bool {
        matches!(self, Tactic::Press | Tactic::Attack)
    }
}

#[derive(Debug, Clone)]
pub struct Brain {
    pub tactic: Tactic,
    /// Seconds left in the tactic before it thinks again.
    pub timer: f32,
    /// Knows you are there (seen you and not given up).
    pub aware: bool,
    /// Blocks left before its guard breaks.
    pub poise: f32,
    /// Which way it circles: +1 or -1.
    pub strafe: f32,
    /// How far through a swing (0..1) while attacking.
    pub swing: f32,
    /// Builds when it is hurt and makes it press harder for a while.
    pub fury: f32,
    /// The attack under way is a heavy one (unblockable, telegraphed red).
    pub heavy: bool,
    /// Attacks so far (every `HEAVY_EVERY`th is heavy).
    attacks: u32,
    /// Has it already reacted to your swing under way?
    reacted: bool,
}

/// What it notices this frame.
#[derive(Debug, Clone, Copy)]
pub struct Senses {
    /// Flat distance to you (`f32::MAX` when you can't be fought: dead, flying).
    pub dist: f32,
    /// A clear line to you within its sight.
    pub sees: bool,
    /// Flat distance from home.
    pub from_home: f32,
    /// Health left, 0..1.
    pub health: f32,
    /// You are winding up a swing at it (it is in front of you and in reach).
    pub threatened: bool,
    /// You hold your guard up.
    pub you_guard: bool,
    /// Other foes pressing you right now.
    pub pressing: usize,
}

/// Its fixed numbers (from the kind).
#[derive(Debug, Clone, Copy)]
pub struct Traits {
    pub style: Style,
    pub reach: f32,
    pub sight: f32,
    pub forget: f32,
    pub leash: f32,
    /// Seconds per swing.
    pub swing: f32,
    /// When in its swing the cut lands (0..1).
    pub contact: f32,
    /// Shoots from up to `reach` away instead of closing in to cut.
    pub ranged: bool,
    /// Its part in a group fight.
    pub role: Role,
    /// Swings faster (a frenzied elite): multiplier on `swing`.
    pub haste: f32,
}

/// What a blow on it did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Took {
    /// Caught on its blade.
    Blocked,
    /// Caught, but that was one too many: it reels open.
    GuardBroken,
    /// It got hurt.
    Hurt,
}

impl Brain {
    pub fn new(style: &Style, seed: u64) -> Self {
        Self {
            tactic: Tactic::Idle,
            timer: 0.0,
            aware: false,
            poise: style.poise,
            strafe: if seed & 1 == 0 { 1.0 } else { -1.0 },
            swing: 0.0,
            fury: 0.0,
            heavy: false,
            attacks: 0,
            reacted: false,
        }
    }

    /// Start an attack: heavy (unblockable) if it's a brute, or every
    /// `HEAVY_EVERY`th cut of anything that cuts.
    fn attack(&mut self, t: &Traits) {
        self.attacks += 1;
        self.heavy =
            !t.ranged && (t.role == Role::Brute || self.attacks.is_multiple_of(HEAVY_EVERY));
        self.set(Tactic::Attack, 0.0);
    }

    /// Seconds its swing under way takes.
    pub fn swing_time(&self, t: &Traits) -> f32 {
        t.swing * t.haste * if self.heavy { HEAVY_SLOW } else { 1.0 }
    }

    /// How red a heavy attack's wind-up glows, 0..1 (rising to the blow).
    pub fn telegraph(&self, t: &Traits) -> f32 {
        if self.tactic == Tactic::Attack && self.heavy && self.swing < t.contact {
            (self.swing / t.contact).clamp(0.0, 1.0).sqrt()
        } else {
            0.0
        }
    }

    fn set(&mut self, tactic: Tactic, timer: f32) {
        self.tactic = tactic;
        self.timer = timer;
        if tactic == Tactic::Attack {
            self.swing = 0.0;
        }
    }

    /// Think for `dt` seconds. True on the frame its cut lands.
    pub fn step(&mut self, s: &Senses, t: &Traits, rng: &mut Rng, dt: f32) -> bool {
        self.timer -= dt;
        self.fury = (self.fury - dt * 0.08).max(0.0);
        if !s.threatened {
            self.reacted = false;
        }

        // Gone too far, or you're out of reach: give up and go home.
        if self.aware && (s.dist > t.forget || s.from_home > t.leash) {
            self.aware = false;
            if !matches!(self.tactic, Tactic::Attack | Tactic::Stagger) {
                self.set(Tactic::Return, 0.0);
            }
        }
        if !self.aware {
            // Back home it can notice you again (not while walking home past
            // the leash, or it would turn round at once).
            let home = s.from_home < t.leash * 0.6;
            if home && s.sees && s.dist < t.sight {
                self.aware = true;
                self.set(Tactic::Approach, 0.0);
            } else if self.tactic == Tactic::Attack
                || (self.tactic == Tactic::Stagger && self.timer > 0.0)
            {
                // Let it finish what it was doing.
            } else if s.from_home > 2.0 {
                self.tactic = Tactic::Return;
            } else {
                self.tactic = Tactic::Idle;
            }
            if self.tactic != Tactic::Attack {
                return false;
            }
        }

        match self.tactic {
            Tactic::Attack => {
                let before = self.swing;
                self.swing += dt / self.swing_time(t);
                let lands = before < t.contact && self.swing >= t.contact;
                if self.swing >= 1.0 {
                    // Eager ones are back at you sooner.
                    let rest = rng.range(0.35, 0.75) * (1.25 - t.style.aggression * 0.6);
                    self.set(Tactic::Recover, rest);
                }
                return lands;
            }
            Tactic::Stagger | Tactic::Recover | Tactic::Guard => {
                if self.timer <= 0.0 {
                    self.decide(s, t, rng);
                }
                return false;
            }
            _ => {}
        }

        // Your swing coming at it: once per swing, maybe raise its blade or
        // step back out of reach.
        if s.threatened && !self.reacted {
            self.reacted = true;
            let r = rng.unit();
            if r < t.style.guard {
                self.set(Tactic::Guard, rng.range(0.5, 1.0));
                return false;
            }
            if r < t.style.guard + t.style.caution * 0.35 {
                self.set(Tactic::Backoff, rng.range(0.3, 0.55));
                return false;
            }
        }

        match self.tactic {
            // A shooter looses as soon as it has a clear line within range.
            Tactic::Approach | Tactic::Circle | Tactic::Press
                if t.ranged && s.sees && s.dist <= t.reach =>
            {
                self.attack(t)
            }
            Tactic::Press if s.dist <= t.reach => self.attack(t),
            Tactic::Approach if s.dist <= ENGAGE => self.decide(s, t, rng),
            Tactic::Circle if s.dist > ENGAGE + 2.0 => self.set(Tactic::Approach, 0.0),
            Tactic::Idle | Tactic::Return => self.set(Tactic::Approach, 0.0),
            _ if self.timer <= 0.0 => self.decide(s, t, rng),
            _ => {}
        }
        false
    }

    /// Pick the next tactic, weighing its style, how hurt it is and how many
    /// others are already pressing you.
    pub fn decide(&mut self, s: &Senses, t: &Traits, rng: &mut Rng) {
        if s.dist > ENGAGE {
            self.set(Tactic::Approach, 0.6);
            return;
        }
        let st = &t.style;
        let aggression = (st.aggression + self.fury * 0.4).min(1.3);
        let crowded = if s.pressing >= PRESSERS { 0.1 } else { 1.0 };
        let wary = if s.you_guard { 0.6 } else { 1.0 };
        // Shooters and menders keep their distance; a shield bearer keeps
        // its shield up; a skirmisher circles (to your back, see `combat`).
        let keeps_off = matches!(t.role, Role::Archer | Role::Support) && s.dist < KITE;
        let (press, circle, guard, back) = match t.role {
            Role::Shield => (0.8, 0.6, 1.4, 0.0),
            Role::Skirmisher => (1.0, 1.6, 0.4, 1.0),
            Role::Brute => (1.3, 0.5, 0.0, 0.5),
            _ => (1.0, 1.0, 1.0, 1.0),
        };
        let weights = [
            (Tactic::Press, aggression * crowded * wary * press),
            (
                Tactic::Circle,
                (st.caution * 0.9 + 0.15 + if s.pressing >= PRESSERS { 0.6 } else { 0.0 }) * circle,
            ),
            (
                Tactic::Guard,
                (st.guard * st.caution * 0.5 + if t.role == Role::Shield { 0.4 } else { 0.0 })
                    * guard,
            ),
            (
                Tactic::Backoff,
                (st.caution * (1.0 - s.health) * 1.2 + if s.dist < 1.3 { 0.3 } else { 0.0 }) * back
                    + if keeps_off { 2.5 } else { 0.0 },
            ),
        ];
        let total: f32 = weights.iter().map(|w| w.1).sum();
        let mut r = rng.unit() * total;
        let mut pick = Tactic::Circle;
        for (tactic, w) in weights {
            if r < w {
                pick = tactic;
                break;
            }
            r -= w;
        }
        match pick {
            Tactic::Press => self.set(Tactic::Press, 2.5),
            Tactic::Circle => {
                if rng.unit() < 0.35 {
                    self.strafe = -self.strafe;
                }
                self.set(Tactic::Circle, rng.range(1.0, 2.4));
            }
            Tactic::Guard => self.set(Tactic::Guard, rng.range(0.6, 1.2)),
            _ => self.set(Tactic::Backoff, rng.range(0.4, 0.8)),
        }
    }

    /// It is struck; `facing` = you are inside its guard's arc; `heavy` = a
    /// charged cut, which breaks a raised guard outright. Returns what the
    /// blow did (the caller deals the damage on `Hurt`).
    pub fn struck(&mut self, facing: bool, heavy: bool, t: &Traits, rng: &mut Rng) -> Took {
        self.aware = true;
        // A shield bearer's shield is always up to the front (unless it's
        // reeling or mid-swing): get round it, kick it or break it.
        if t.role == Role::Shield
            && facing
            && !matches!(self.tactic, Tactic::Stagger | Tactic::Attack)
        {
            self.tactic = Tactic::Guard;
            self.timer = self.timer.max(0.4);
        }
        if self.tactic == Tactic::Guard && facing && heavy {
            self.poise = t.style.poise;
            self.set(Tactic::Stagger, 1.3);
            return Took::GuardBroken;
        }
        if self.tactic == Tactic::Guard && facing {
            self.poise -= 1.0;
            if self.poise <= 0.0 {
                self.poise = t.style.poise;
                self.set(Tactic::Stagger, 1.1);
                return Took::GuardBroken;
            }
            // Blocked: the bold ones answer at once.
            if rng.unit() < t.style.aggression * 0.6 {
                self.attack(t);
            } else {
                self.timer = self.timer.max(0.35);
            }
            return Took::Blocked;
        }
        self.fury = (self.fury + 0.5).min(1.5);
        self.set(Tactic::Stagger, if heavy { 0.8 } else { 0.3 });
        Took::Hurt
    }

    /// Kicked: knocked back reeling, guard broken, whatever it was doing.
    pub fn kicked(&mut self, t: &Traits) {
        self.aware = true;
        self.poise = t.style.poise;
        self.set(Tactic::Stagger, KICK_STAGGER);
    }

    /// Slammed into a wall by a kick: down for longer.
    pub fn slammed(&mut self) {
        self.set(Tactic::Stagger, KICK_STAGGER + 0.8);
    }

    /// Its cut was caught on your blade. A parry (your guard went up just in
    /// time) leaves it reeling; a plain block just knocks it back a step.
    pub fn blocked(&mut self, parried: bool) {
        if parried {
            self.set(Tactic::Stagger, 1.0);
        } else {
            self.set(Tactic::Recover, 0.55);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shooter_looses_from_range_with_a_clear_line() {
        let mut t = traits(0.3, 0.8, 0.1);
        t.ranged = true;
        t.reach = 12.0;
        let mut b = Brain::new(&t.style, 7);
        let mut rng = Rng(3);
        let mut s = Senses {
            dist: 10.0,
            sees: true,
            from_home: 0.0,
            health: 1.0,
            threatened: false,
            you_guard: false,
            pressing: 0,
        };
        b.step(&s, &t, &mut rng, 0.05);
        // Noticed you; next it shoots from where it stands.
        b.step(&s, &t, &mut rng, 0.05);
        assert_eq!(b.tactic, Tactic::Attack, "shoots at 10 blocks");
        let mut landed = false;
        for _ in 0..40 {
            landed |= b.step(&s, &t, &mut rng, 0.05);
        }
        assert!(landed, "the shot goes off");
        // Out of sight it has to come closer instead.
        let mut b = Brain::new(&t.style, 7);
        s.sees = false;
        b.aware = true;
        b.set(Tactic::Approach, 0.0);
        b.step(&s, &t, &mut rng, 0.05);
        assert_ne!(b.tactic, Tactic::Attack);
    }

    fn traits(aggression: f32, caution: f32, guard: f32) -> Traits {
        Traits {
            style: Style {
                aggression,
                caution,
                guard,
                poise: 2.0,
            },
            reach: 2.3,
            sight: 18.0,
            forget: 28.0,
            leash: 40.0,
            swing: 0.8,
            contact: 0.5,
            ranged: false,
            role: Role::Grunt,
            haste: 1.0,
        }
    }

    fn near(dist: f32) -> Senses {
        Senses {
            dist,
            sees: true,
            from_home: 1.0,
            health: 1.0,
            threatened: false,
            you_guard: false,
            pressing: 0,
        }
    }

    fn run(b: &mut Brain, s: &Senses, t: &Traits, rng: &mut Rng, secs: f32) -> u32 {
        let mut hits = 0;
        for _ in 0..(secs * 60.0) as usize {
            hits += b.step(s, t, rng, 1.0 / 60.0) as u32;
        }
        hits
    }

    #[test]
    fn notices_you_in_sight_and_ignores_you_out_of_it() {
        let t = traits(0.5, 0.5, 0.5);
        let mut rng = Rng(7);
        let mut b = Brain::new(&t.style, 1);
        b.step(&near(30.0), &t, &mut rng, 0.1);
        assert!(!b.aware && b.tactic == Tactic::Idle);
        let mut hidden = near(10.0);
        hidden.sees = false;
        b.step(&hidden, &t, &mut rng, 0.1);
        assert!(!b.aware);
        b.step(&near(10.0), &t, &mut rng, 0.1);
        assert!(b.aware && b.tactic == Tactic::Approach);
    }

    #[test]
    fn gives_up_when_you_get_far_away_or_it_strays_from_home() {
        let t = traits(0.5, 0.5, 0.5);
        let mut rng = Rng(7);
        let mut b = Brain::new(&t.style, 1);
        b.step(&near(10.0), &t, &mut rng, 0.1);
        // You ran off while it was 6 blocks from home: it walks back.
        let mut gone = near(29.0);
        gone.from_home = 6.0;
        b.step(&gone, &t, &mut rng, 0.1);
        assert!(!b.aware && b.tactic == Tactic::Return);
        // Home again, it stands about.
        gone.from_home = 1.0;
        b.step(&gone, &t, &mut rng, 0.1);
        assert_eq!(b.tactic, Tactic::Idle);
        let mut b = Brain::new(&t.style, 1);
        b.step(&near(10.0), &t, &mut rng, 0.1);
        let mut far_home = near(10.0);
        far_home.from_home = 41.0;
        b.step(&far_home, &t, &mut rng, 0.1);
        assert!(!b.aware && b.tactic == Tactic::Return);
        // Walking home it doesn't turn straight back even though it sees you.
        far_home.from_home = 30.0;
        b.step(&far_home, &t, &mut rng, 0.1);
        assert_eq!(b.tactic, Tactic::Return);
    }

    #[test]
    fn mixes_tactics_up_close() {
        let t = traits(0.6, 0.5, 0.5);
        let mut rng = Rng(42);
        let mut b = Brain::new(&t.style, 1);
        let mut seen = std::collections::HashSet::new();
        let mut hits = 0;
        for i in 0..6000 {
            // Hover around its reach so it can both cut and circle.
            let s = near(if (i / 90) % 2 == 0 { 2.0 } else { 4.0 });
            hits += b.step(&s, &t, &mut rng, 1.0 / 60.0) as u32;
            seen.insert(format!("{:?}", b.tactic));
        }
        for want in ["Press", "Attack", "Recover", "Circle", "Guard"] {
            assert!(seen.contains(want), "never used {want}: {seen:?}");
        }
        assert!(hits > 5, "{hits}");
    }

    #[test]
    fn aggressive_ones_cut_more_often_than_cautious_ones() {
        let count = |a, c| {
            let t = traits(a, c, 0.3);
            let mut rng = Rng(9);
            let mut b = Brain::new(&t.style, 1);
            run(&mut b, &near(2.0), &t, &mut rng, 60.0)
        };
        assert!(count(0.95, 0.1) > count(0.2, 0.9) * 2);
    }

    #[test]
    fn crowds_take_turns() {
        let t = traits(0.9, 0.2, 0.2);
        let mut rng = Rng(3);
        let mut b = Brain::new(&t.style, 1);
        let mut s = near(3.0);
        s.pressing = PRESSERS;
        let mut presses = 0;
        for _ in 0..200 {
            b.decide(&s, &t, &mut rng);
            presses += (b.tactic == Tactic::Press) as u32;
        }
        assert!(presses < 40, "{presses}");
    }

    #[test]
    fn a_cut_lands_once_per_swing() {
        let t = traits(0.5, 0.5, 0.5);
        let mut rng = Rng(5);
        let mut b = Brain::new(&t.style, 1);
        b.aware = true;
        b.set(Tactic::Attack, 0.0);
        let hits = run(&mut b, &near(2.0), &t, &mut rng, t.swing - 0.02);
        assert_eq!(hits, 1);
        b.step(&near(2.0), &t, &mut rng, 0.05);
        assert_eq!(b.tactic, Tactic::Recover);
    }

    #[test]
    fn guarding_blocks_until_its_poise_runs_out() {
        let t = traits(0.0, 0.5, 1.0);
        let mut rng = Rng(5);
        let mut b = Brain::new(&t.style, 1);
        b.set(Tactic::Guard, 5.0);
        assert_eq!(b.struck(true, false, &t, &mut rng), Took::Blocked);
        assert_eq!(b.struck(true, false, &t, &mut rng), Took::GuardBroken);
        assert_eq!(b.tactic, Tactic::Stagger);
        // From behind the guard doesn't help.
        b.set(Tactic::Guard, 5.0);
        assert_eq!(b.struck(false, false, &t, &mut rng), Took::Hurt);
        // A charged cut breaks a raised guard at once.
        b.set(Tactic::Guard, 1.0);
        assert_eq!(b.struck(true, true, &t, &mut rng), Took::GuardBroken);
        assert_eq!(b.tactic, Tactic::Stagger);
        // A hit interrupts its swing.
        b.set(Tactic::Attack, 0.0);
        assert_eq!(b.struck(true, false, &t, &mut rng), Took::Hurt);
        assert_eq!(b.tactic, Tactic::Stagger);
    }

    #[test]
    fn every_fourth_cut_is_heavy_and_brutes_always_are() {
        let t = traits(0.9, 0.1, 0.0);
        let mut rng = Rng(5);
        let mut b = Brain::new(&t.style, 1);
        let heavies: Vec<bool> = (0..8)
            .map(|_| {
                b.attack(&t);
                b.heavy
            })
            .collect();
        assert_eq!(heavies.iter().filter(|h| **h).count(), 2);
        assert!(heavies[3] && heavies[7]);
        // A heavy is slower and glows red as it winds up, none once it lands.
        assert!(b.swing_time(&t) > t.swing * 1.3);
        b.swing = t.contact * 0.5;
        assert!(b.telegraph(&t) > 0.5);
        b.swing = t.contact + 0.01;
        assert_eq!(b.telegraph(&t), 0.0);
        let mut brute = t;
        brute.role = Role::Brute;
        b.attack(&brute);
        assert!(b.heavy);
        // Shooters never swing heavies.
        let mut shooter = t;
        shooter.ranged = true;
        for _ in 0..8 {
            b.attack(&shooter);
            assert!(!b.heavy);
        }
        let _ = run(&mut b, &near(2.0), &t, &mut rng, 0.1);
    }

    #[test]
    fn a_shield_bearer_blocks_from_the_front_but_not_the_back() {
        let mut t = traits(0.5, 0.5, 0.0);
        t.role = Role::Shield;
        t.style.poise = 4.0;
        let mut rng = Rng(5);
        let mut b = Brain::new(&t.style, 1);
        b.set(Tactic::Circle, 2.0);
        assert_eq!(b.struck(true, false, &t, &mut rng), Took::Blocked);
        b.set(Tactic::Circle, 2.0);
        assert_eq!(b.struck(false, false, &t, &mut rng), Took::Hurt);
        b.set(Tactic::Press, 2.0);
        assert_eq!(b.struck(true, true, &t, &mut rng), Took::GuardBroken);
        // It mostly keeps the shield up.
        let mut guards = 0;
        for _ in 0..200 {
            b.decide(&near(2.5), &t, &mut rng);
            guards += (b.tactic == Tactic::Guard) as u32;
        }
        assert!(guards > 60, "{guards}");
    }

    #[test]
    fn shooters_and_menders_keep_their_distance() {
        let mut t = traits(0.3, 0.5, 0.1);
        t.role = Role::Archer;
        let mut rng = Rng(8);
        let mut b = Brain::new(&t.style, 1);
        let mut backs = 0;
        for _ in 0..200 {
            b.decide(&near(2.5), &t, &mut rng);
            backs += (b.tactic == Tactic::Backoff) as u32;
        }
        assert!(backs > 100, "{backs}");
    }

    #[test]
    fn your_swing_makes_a_guarder_raise_its_blade() {
        let t = traits(0.3, 0.3, 1.0);
        let mut rng = Rng(5);
        let mut b = Brain::new(&t.style, 1);
        b.aware = true;
        b.set(Tactic::Circle, 2.0);
        let mut s = near(3.0);
        s.threatened = true;
        b.step(&s, &t, &mut rng, 0.016);
        assert_eq!(b.tactic, Tactic::Guard);
    }
}
