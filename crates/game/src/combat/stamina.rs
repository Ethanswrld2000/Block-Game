//! Fighting tires you: light cuts, heavy cuts, kicks and blows caught on your
//! blade come out of the Fatigue bar (the same bar sprinting wears down,
//! `survival::vitals`), never below `FLOOR`, like sprinting. Worn down to the
//! floor you're winded: no cuts, no kicks, and a blow on your guard breaks
//! it. Pure rules on `Health`, tested.

use crate::survival::health::{Health, Layer};

/// Fighting never wears Fatigue below this (sprinting stops here too).
pub const FLOOR: f32 = 55.0;

/// Fatigue each action costs.
pub const LIGHT: f32 = 2.0;
pub const HEAVY: f32 = 4.0;
pub const KICK: f32 = 3.0;
/// A cut caught on your guard (a parry costs nothing).
pub const BLOCK_HIT: f32 = 3.0;

/// Fatigue left above the floor.
fn spare(h: &Health) -> f32 {
    h.get(Layer::Fatigue) - FLOOR
}

/// Badly hurt (a layer under half): fighting costs nothing more, so a
/// cost never lands on a deeper layer.
fn free(h: &Health) -> bool {
    h.mortal().is_some()
}

/// Can you fight at all (not worn down to the floor)?
pub fn ready(h: &Health) -> bool {
    free(h) || spare(h) > 0.0
}

/// Pay for an action. False (nothing spent) when winded; with less than the
/// cost left it still goes, down to the floor.
pub fn spend(h: &mut Health, cost: f32) -> bool {
    if !ready(h) {
        return false;
    }
    if !free(h) {
        h.hit(Layer::Fatigue, cost.min(spare(h)));
    }
    true
}

/// A cut caught on your guard. True if the guard holds; false if you're too
/// worn down to hold it (the guard breaks and the cut lands).
pub fn block(h: &mut Health) -> bool {
    if free(h) {
        return true;
    }
    let holds = spare(h) >= BLOCK_HIT;
    h.hit(Layer::Fatigue, BLOCK_HIT.min(spare(h).max(0.0)));
    holds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fighting_wears_fatigue_down_to_the_floor_and_no_further() {
        let mut h = Health::default();
        assert!(spend(&mut h, LIGHT));
        assert_eq!(h.get(Layer::Fatigue), 100.0 - LIGHT);
        let mut n = 1;
        while spend(&mut h, HEAVY) {
            n += 1;
        }
        // About a dozen heavy cuts from rested.
        assert!((10..=14).contains(&n), "{n}");
        assert_eq!(h.get(Layer::Fatigue), FLOOR);
        assert!(!ready(&h) && !spend(&mut h, LIGHT));
        assert_eq!(h.get(Layer::Damage), 100.0, "never spills into another bar");
    }

    #[test]
    fn a_guard_breaks_when_too_worn_to_hold() {
        let mut h = Health::default();
        assert!(block(&mut h));
        h.hit(Layer::Fatigue, h.get(Layer::Fatigue) - FLOOR - 1.0);
        assert!(!block(&mut h));
        assert_eq!(h.get(Layer::Fatigue), FLOOR);
    }

    #[test]
    fn badly_hurt_you_still_fight_and_it_costs_nothing_more() {
        let mut h = Health::default();
        h.hit(Layer::Blood, 60.0);
        assert!(h.mortal().is_some());
        let before = h.get(Layer::Blood);
        assert!(spend(&mut h, HEAVY) && block(&mut h));
        assert_eq!(h.get(Layer::Blood), before);
    }
}
