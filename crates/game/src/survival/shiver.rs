//! Cold on screen: the view shivers and a blue vignette creeps in. Plain Rust with no Bevy.
//!
//! The cold stages in exposure.ron pick how often the shiver comes: Chilly now and then,
//! Freezing often, the last stage (Deadly cold) all the time. The vignette grows with
//! how far the felt temperature is below the first cold stage.

use super::exposure::{Exposure, Scale};

/// Seconds between shivers, (shortest, longest), for Chilly and Freezing.
const WAITS: [(f32, f32); 2] = [(14.0, 26.0), (3.0, 6.0)];
/// Seconds a shiver lasts, (shortest, longest).
const BURST: (f32, f32) = (0.9, 1.6);
/// How hard each stage shivers, 0..1.
const STRENGTH: [f32; 3] = [0.35, 0.65, 1.0];
/// Degrees past the last cold stage at which the vignette is full.
const VIGNETTE_PAST: f32 = 10.0;

/// The cold stage you are in: 0 none, 1 the first (Chilly), counting up.
pub fn cold_level(rules: &Exposure, feels: f32) -> usize {
    rules.stage(Scale::Cold, feels).map_or(0, |a| a.level + 1)
}

/// The blue vignette, 0..1: nothing until the first cold stage, full a little past the last.
pub fn vignette(rules: &Exposure, feels: f32) -> f32 {
    let stages = rules.stages(Scale::Cold);
    let (Some(first), Some(last)) = (stages.first(), stages.last()) else {
        return 0.0;
    };
    let span = first.at - last.at + VIGNETTE_PAST;
    if span <= 0.0 {
        return 0.0;
    }
    ((first.at - feels) / span).clamp(0.0, 1.0)
}

/// When the next shiver comes and how strong the shiver is now.
#[derive(Debug, Clone, bevy::prelude::Resource)]
pub struct Shiver {
    /// 0..1, eased; what the camera shakes by.
    pub amount: f32,
    /// Seconds left in the shiver going on now.
    burst: f32,
    /// Seconds until the next shiver.
    wait: f32,
    seed: u32,
}

impl Default for Shiver {
    fn default() -> Self {
        // The first shiver comes soon after you get cold.
        Self {
            amount: 0.0,
            burst: 0.0,
            wait: 1.5,
            seed: 0x9e37_79b9,
        }
    }
}

impl Shiver {
    /// A number in 0..1 from a small xorshift.
    fn roll(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn between(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + (hi - lo) * self.roll()
    }

    /// Step `dt` seconds at a cold `level` (from `cold_level`).
    pub fn tick(&mut self, dt: f32, level: usize) {
        let last = STRENGTH.len();
        let target = match level {
            0 => {
                self.burst = 0.0;
                self.wait = self.wait.clamp(0.5, 1.5);
                0.0
            }
            l if l >= last => STRENGTH[last - 1],
            l => {
                // Getting colder never leaves you waiting out a long Chilly gap.
                self.wait = self.wait.min(WAITS[l - 1].1);
                if self.burst > 0.0 {
                    self.burst -= dt;
                } else {
                    self.wait -= dt;
                    if self.wait <= 0.0 {
                        self.burst = self.between(BURST);
                        self.wait = self.between(WAITS[l - 1]);
                    }
                }
                if self.burst > 0.0 {
                    STRENGTH[l - 1]
                } else {
                    0.0
                }
            }
        };
        // Comes on quickly, dies away a little slower.
        let rate = if target > self.amount { 8.0 } else { 3.0 };
        self.amount += (target - self.amount) * (rate * dt).min(1.0);
    }
}

/// The camera's shiver at time `t` seconds: (pitch, yaw, roll) in radians. Quick, uneven trembling.
pub fn offset(t: f32, amount: f32) -> (f32, f32, f32) {
    if amount <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let wobble = |a: f32, b: f32, c: f32| {
        ((t * a).sin() + (t * b).sin() * 0.6 + (t * c).sin() * 0.35) / 1.95
    };
    (
        wobble(97.0, 131.0, 61.0) * 0.006 * amount,
        wobble(83.0, 117.0, 53.0) * 0.004 * amount,
        wobble(109.0, 73.0, 149.0) * 0.008 * amount,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survival::exposure::tests::rules;

    /// Seconds out of `secs` that the shiver is noticeably on.
    fn shivering(level: usize, secs: f32) -> f32 {
        let mut s = Shiver::default();
        let dt = 1.0 / 60.0;
        let mut on = 0.0;
        for _ in 0..(secs / dt) as usize {
            s.tick(dt, level);
            if s.amount > 0.1 {
                on += dt;
            }
        }
        on
    }

    #[test]
    fn levels_follow_the_cold_stages() {
        let r = rules();
        assert_eq!(cold_level(&r, 15.0), 0);
        assert_eq!(cold_level(&r, 5.0), 1);
        assert_eq!(cold_level(&r, -10.0), 2);
        assert_eq!(cold_level(&r, -25.0), 3);
    }

    #[test]
    fn colder_shivers_more_often() {
        let none = shivering(0, 120.0);
        let chilly = shivering(1, 120.0);
        let freezing = shivering(2, 120.0);
        let deadly = shivering(3, 120.0);
        assert_eq!(none, 0.0);
        assert!(chilly > 0.0, "chilly should shiver now and then");
        assert!(chilly < 30.0, "chilly shivers only now and then: {chilly}");
        assert!(
            freezing > chilly * 2.0,
            "freezing {freezing} vs chilly {chilly}"
        );
        assert!(freezing < 100.0, "freezing still has gaps: {freezing}");
        assert!(deadly > 119.0, "deadly cold never stops: {deadly}");
    }

    #[test]
    fn warming_up_stops_it() {
        let mut s = Shiver::default();
        for _ in 0..120 {
            s.tick(1.0 / 60.0, 3);
        }
        assert!(s.amount > 0.9);
        for _ in 0..180 {
            s.tick(1.0 / 60.0, 0);
        }
        assert!(s.amount < 0.01);
    }

    #[test]
    fn vignette_grows_with_cold() {
        let r = rules();
        assert_eq!(vignette(&r, 20.0), 0.0);
        assert_eq!(vignette(&r, 8.0), 0.0);
        let freezing = vignette(&r, -4.0);
        let deadly = vignette(&r, -18.0);
        assert!(freezing > 0.0 && freezing < deadly && deadly < 1.0);
        assert_eq!(vignette(&r, -40.0), 1.0);
    }

    #[test]
    fn offset_is_small_and_still_when_warm() {
        assert_eq!(offset(3.0, 0.0), (0.0, 0.0, 0.0));
        for i in 0..200 {
            let (p, y, r) = offset(i as f32 * 0.013, 1.0);
            assert!(p.abs() <= 0.006 && y.abs() <= 0.004 && r.abs() <= 0.008);
        }
    }
}
