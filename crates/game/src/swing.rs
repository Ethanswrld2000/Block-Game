//! How the arm swings what it holds, and the moment the swing lands.
//!
//! Each item has a swing style (`swing:` in items.ron). A swing is a few
//! poses in time: wind up, strike, a small bounce off what you hit, back to
//! rest. Mining lands one blow at `Swing::contact`, the end of the strike, so
//! the block cracks and the knock sounds exactly when the tool meets it.
//! Pure: no Bevy systems here, `interact` times the blows and `hands` draws
//! the poses.

use bevy::math::{EulerRot, Quat, Vec3};
use serde::Deserialize;

/// How something in the hand is swung.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Swing {
    /// A short punch forward (bare hand, blocks, torches, anything else).
    #[default]
    Jab,
    /// An axe: cocked up and out to the right, then a diagonal cut into the trunk.
    Chop,
    /// A pick: raised straight up over the shoulder, then brought straight down.
    Overhead,
}

#[derive(Clone, Copy)]
enum Ease {
    /// Speeds up into the key (a strike, hardest at the end).
    In,
    /// Slows into the key (a bounce settling).
    Out,
    /// Slow at both ends (winding up, going back to rest).
    InOut,
}

impl Ease {
    fn apply(self, x: f32) -> f32 {
        match self {
            Ease::In => x * x * x,
            Ease::Out => 1.0 - (1.0 - x).powi(2),
            Ease::InOut => x * x * (3.0 - 2.0 * x),
        }
    }
}

/// One pose: when (0..1 of the swing), how far the hand moves (metres in
/// camera space) and turns (radians about x, y, z), and how it eases in.
#[derive(Clone, Copy)]
struct Key {
    t: f32,
    pos: [f32; 3],
    rot: [f32; 3],
    ease: Ease,
}

const fn key(t: f32, pos: [f32; 3], rot: [f32; 3], ease: Ease) -> Key {
    Key { t, pos, rot, ease }
}

const REST: Key = key(0.0, [0.0; 3], [0.0; 3], Ease::InOut);

/// Which key in each list is the contact pose.
const CONTACT_KEY: usize = 2;

// Rotation x < 0 tips the tool forward and down, > 0 lifts it back toward
// you; y > 0 turns it left (toward the middle for the right hand); z > 0
// rolls it left.
const JAB: [Key; 5] = [
    REST,
    key(0.15, [0.0, 0.02, 0.03], [0.25, 0.0, 0.0], Ease::InOut),
    key(0.45, [-0.06, -0.05, -0.13], [-1.1, 0.25, 0.0], Ease::In),
    key(0.55, [-0.055, -0.045, -0.115], [-1.0, 0.23, 0.0], Ease::Out),
    key(1.0, [0.0; 3], [0.0; 3], Ease::InOut),
];

const CHOP: [Key; 5] = [
    REST,
    key(0.38, [0.07, 0.09, 0.05], [0.55, -0.6, -0.45], Ease::InOut),
    key(0.55, [-0.14, 0.01, -0.08], [-0.32, 0.5, 0.25], Ease::In),
    key(0.64, [-0.12, 0.02, -0.07], [-0.22, 0.43, 0.21], Ease::Out),
    key(1.0, [0.0; 3], [0.0; 3], Ease::InOut),
];

const OVERHEAD: [Key; 5] = [
    REST,
    key(0.38, [0.0, 0.11, 0.07], [0.8, 0.0, 0.0], Ease::InOut),
    key(0.55, [-0.12, 0.01, -0.08], [-0.32, 0.18, 0.0], Ease::In),
    key(0.64, [-0.105, 0.02, -0.07], [-0.2, 0.16, 0.0], Ease::Out),
    key(1.0, [0.0; 3], [0.0; 3], Ease::InOut),
];

impl Swing {
    fn keys(self) -> &'static [Key] {
        match self {
            Swing::Jab => &JAB,
            Swing::Chop => &CHOP,
            Swing::Overhead => &OVERHEAD,
        }
    }

    /// Seconds one swing takes. Holding the button swings again and again,
    /// one blow per swing.
    pub fn period(self) -> f32 {
        match self {
            Swing::Jab => 0.32,
            Swing::Chop => 0.42,
            Swing::Overhead => 0.45,
        }
    }

    /// When the blow lands, as a share of the swing (the end of the strike).
    pub fn contact(self) -> f32 {
        self.keys()[CONTACT_KEY].t
    }

    /// The hand's offset and turn `t` (0..1) of the way through a swing.
    pub fn pose(self, t: f32) -> (Vec3, Quat) {
        let keys = self.keys();
        let t = t.clamp(0.0, 1.0);
        let i = keys.iter().position(|k| k.t >= t).unwrap_or(keys.len() - 1).max(1);
        let (a, b) = (keys[i - 1], keys[i]);
        let x = if b.t > a.t { b.ease.apply((t - a.t) / (b.t - a.t)) } else { 1.0 };
        let lerp = |p: [f32; 3], q: [f32; 3]| Vec3::from(p).lerp(Vec3::from(q), x);
        let rot = lerp(a.rot, b.rot);
        (lerp(a.pos, b.pos), Quat::from_euler(EulerRot::XYZ, rot.x, rot.y, rot.z))
    }
}

/// Did a swing that went from `before` to `after` (shares of the swing, 0..1)
/// pass its contact moment? Counts once, at the frame that crosses it.
pub fn lands(swing: Swing, before: f32, after: f32) -> bool {
    let c = swing.contact();
    before < c && after >= c
}

/// Blows needed to break something that takes `seconds` of steady work, if
/// each swing does one blow's share of it. Total time comes out close to the
/// old continuous mining: the first blow lands at the contact moment.
pub fn blow_share(swing: Swing, seconds: f32) -> f32 {
    if seconds <= 0.0 { 1.0 } else { swing.period() / seconds }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Swing; 3] = [Swing::Jab, Swing::Chop, Swing::Overhead];

    fn pitch(q: Quat) -> f32 {
        q.to_euler(EulerRot::XYZ).0
    }

    #[test]
    fn swings_start_and_end_at_rest() {
        for s in ALL {
            for t in [0.0, 1.0] {
                let (p, q) = s.pose(t);
                assert!(p.length() < 1e-5 && q.angle_between(Quat::IDENTITY) < 1e-5, "{s:?} at {t}");
            }
        }
    }

    #[test]
    fn keys_are_in_order() {
        for s in ALL {
            let k = s.keys();
            assert!(k.windows(2).all(|w| w[0].t < w[1].t), "{s:?}");
            assert!(s.contact() > 0.0 && s.contact() < 1.0);
        }
    }

    #[test]
    fn the_blow_lands_at_the_deepest_point_of_the_strike() {
        // The tool is furthest forward and down exactly at contact: the
        // moment the hit is counted is the moment it looks like it hits.
        for s in ALL {
            let deepest = (0..=1000).map(|i| i as f32 / 1000.0).min_by(|a, b| pitch(s.pose(*a).1).total_cmp(&pitch(s.pose(*b).1))).unwrap();
            assert!((deepest - s.contact()).abs() < 0.002, "{s:?}: deepest at {deepest}, blow at {}", s.contact());
        }
    }

    #[test]
    fn strikes_speed_up_into_contact() {
        for s in ALL {
            let c = s.contact();
            let speed = |t: f32| (pitch(s.pose(t).1) - pitch(s.pose(t - 0.01).1)).abs();
            assert!(speed(c) > speed(c - 0.08), "{s:?} should hit hardest at contact");
        }
    }

    #[test]
    fn pick_swings_straight_down_and_axe_across() {
        // A pick only turns in a little toward the crosshair; an axe comes
        // across from the right.
        let yaw = |s: Swing, t| s.pose(t).1.to_euler(EulerRot::XYZ).1;
        assert!((yaw(Swing::Overhead, 0.38) - yaw(Swing::Overhead, 0.55)).abs() < 0.25);
        assert!(yaw(Swing::Chop, 0.55) - yaw(Swing::Chop, 0.38) > 0.8);
        // Both are raised above rest before they come down.
        for s in [Swing::Chop, Swing::Overhead] {
            assert!(s.pose(0.38).0.y > 0.05 && s.pose(s.contact()).0.y < s.pose(0.38).0.y - 0.05, "{s:?}");
        }
    }

    #[test]
    fn one_blow_per_swing() {
        for s in ALL {
            let mut n = 0;
            let mut t = 0.0;
            while t < 1.0 {
                let next = t + 1.0 / 60.0 / s.period();
                n += lands(s, t, next) as u32;
                t = next;
            }
            assert_eq!(n, 1, "{s:?}");
        }
    }

    #[test]
    fn breaking_takes_about_as_long_as_before() {
        // Log with the stone axe: 2.5 hardness / 4 speed = 0.625 s.
        let s = Swing::Chop;
        let seconds = 0.625;
        let blows = (1.0 / blow_share(s, seconds)).ceil();
        let time = (blows - 1.0) * s.period() + s.contact() * s.period();
        assert!((time - seconds).abs() < 0.25, "{time}");
        assert_eq!(blow_share(s, 0.0), 1.0);
    }
}
