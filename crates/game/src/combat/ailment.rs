//! What a weapon's traits leave on a foe after a blow: bleeding and burning
//! (damage over time) and frost (slower). A fresh blow refreshes them; the
//! strongest one of each kind counts. Pure, tested.

/// Seconds each lasts after the last blow that gave it.
pub const BLEED_FOR: f32 = 4.0;
pub const BURN_FOR: f32 = 3.0;
pub const FROST_FOR: f32 = 3.0;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ailments {
    /// Damage per second, and seconds left.
    pub bleed: (f32, f32),
    pub burn: (f32, f32),
    /// Share slower, and seconds left.
    pub frost: (f32, f32),
}

impl Ailments {
    pub fn bleed(&mut self, dps: f32) {
        self.bleed = (self.bleed.0.max(dps), BLEED_FOR);
    }

    pub fn burn(&mut self, dps: f32) {
        self.burn = (self.burn.0.max(dps), BURN_FOR);
    }

    pub fn frost(&mut self, slow: f32) {
        self.frost = (self.frost.0.max(slow.clamp(0.0, 0.9)), FROST_FOR);
    }

    /// How fast it can move (1 = as usual).
    pub fn pace(&self) -> f32 {
        if self.frost.1 > 0.0 { 1.0 - self.frost.0 } else { 1.0 }
    }

    pub fn bleeding(&self) -> bool {
        self.bleed.1 > 0.0
    }

    pub fn burning(&self) -> bool {
        self.burn.1 > 0.0
    }

    pub fn frozen(&self) -> bool {
        self.frost.1 > 0.0
    }

    /// Runs them on by `dt` seconds; returns the damage they did.
    pub fn tick(&mut self, dt: f32) -> f32 {
        let mut hurt = 0.0;
        for (dps, left) in [&mut self.bleed, &mut self.burn] {
            if *left > 0.0 {
                hurt += *dps * dt.min(*left);
                *left -= dt;
                if *left <= 0.0 {
                    *dps = 0.0;
                    *left = 0.0;
                }
            }
        }
        if self.frost.1 > 0.0 {
            self.frost.1 -= dt;
            if self.frost.1 <= 0.0 {
                self.frost = (0.0, 0.0);
            }
        }
        hurt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bleeding_does_its_damage_then_stops() {
        let mut a = Ailments::default();
        a.bleed(2.0);
        let mut total = 0.0;
        for _ in 0..600 {
            total += a.tick(1.0 / 60.0);
        }
        assert!((total - 2.0 * BLEED_FOR).abs() < 0.05, "{total}");
        assert!(!a.bleeding());
        assert_eq!(a.tick(1.0), 0.0);
    }

    #[test]
    fn frost_slows_for_a_while_and_the_strongest_counts() {
        let mut a = Ailments::default();
        a.frost(0.4);
        a.frost(0.2);
        assert!((a.pace() - 0.6).abs() < 1e-6);
        a.tick(FROST_FOR + 0.1);
        assert_eq!(a.pace(), 1.0);
        // A fresh blow refreshes a burn.
        a.burn(3.0);
        a.tick(2.0);
        a.burn(1.0);
        assert_eq!(a.burn, (3.0, BURN_FOR));
    }
}
