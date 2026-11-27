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
    /// An axe: wound back out to the right, then a flat, heavy swing across into the trunk.
    Chop,
    /// A pick: raised high over the shoulder, then driven straight down.
    Overhead,
    /// A sword: drawn back high on the right, then a quick diagonal cut down
    /// and across, following through. Always a fighting swing, never mining.
    Slash,
    /// A shovel: drawn back and up, thrust down into the ground, then levered
    /// up and the load tossed off to the side.
    Dig,
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
/// One pose: when (0..1 of the swing), the values it reaches, and how it
/// eases into them.
#[derive(Clone, Copy)]
struct Key<const N: usize> {
    t: f32,
    v: [f32; N],
    ease: Ease,
}

/// First-person keys: how far the hand moves (metres in camera space) and
/// turns (radians about x, y, z).
type Hand = Key<6>;

const fn key(t: f32, pos: [f32; 3], rot: [f32; 3], ease: Ease) -> Hand {
    Key { t, v: [pos[0], pos[1], pos[2], rot[0], rot[1], rot[2]], ease }
}

const REST: Hand = key(0.0, [0.0; 3], [0.0; 3], Ease::InOut);

/// Values `t` (0..1) of the way through a list of keys.
fn sample<const N: usize>(keys: &[Key<N>], t: f32) -> [f32; N] {
    let t = t.clamp(0.0, 1.0);
    let i = keys.iter().position(|k| k.t >= t).unwrap_or(keys.len() - 1).max(1);
    let (a, b) = (keys[i - 1], keys[i]);
    let x = if b.t > a.t { b.ease.apply((t - a.t) / (b.t - a.t)) } else { 1.0 };
    std::array::from_fn(|j| a.v[j] + (b.v[j] - a.v[j]) * x)
}

// Rotation x < 0 tips the tool forward and down, > 0 lifts it back toward
// you; y > 0 turns it left (toward the middle for the right hand); z > 0
// rolls it left.
const JAB: [Hand; 5] = [
    REST,
    key(0.15, [0.0, 0.02, 0.03], [0.25, 0.0, 0.0], Ease::InOut),
    key(0.45, [-0.06, -0.05, -0.13], [-1.1, 0.25, 0.0], Ease::In),
    key(0.55, [-0.055, -0.045, -0.115], [-1.0, 0.23, 0.0], Ease::Out),
    key(1.0, [0.0; 3], [0.0; 3], Ease::InOut),
];

/// The axe: a heavy horizontal swing. A small settle, a slow wind back out to
/// the right with the head laid flat, a fast whip across into the trunk, the
/// head biting and jarring back a little, a beat stuck in the wood, then
/// pulled free.
const CHOP: [Hand; 7] = [
    REST,
    key(0.10, [0.01, -0.02, 0.01], [-0.08, 0.06, 0.02], Ease::InOut),
    key(0.42, [0.15, 0.04, 0.10], [-0.35, -1.0, -0.4], Ease::InOut),
    key(0.54, [-0.17, 0.05, -0.15], [-0.38, 1.1, 0.3], Ease::In),
    key(0.60, [-0.145, 0.055, -0.13], [-0.35, 0.96, 0.26], Ease::Out),
    key(0.74, [-0.14, 0.05, -0.125], [-0.34, 0.93, 0.25], Ease::InOut),
    key(1.0, [0.0; 3], [0.0; 3], Ease::InOut),
];

/// The pick: a vertical swing. Raised high over the shoulder with the head
/// tipped back, then driven straight down, a jar off the stone, a beat, and
/// back up to rest.
const OVERHEAD: [Hand; 7] = [
    REST,
    key(0.10, [0.0, -0.02, 0.01], [-0.1, 0.0, 0.0], Ease::InOut),
    key(0.42, [0.02, 0.19, 0.10], [1.25, 0.04, 0.0], Ease::InOut),
    key(0.54, [-0.07, -0.03, -0.13], [-0.62, 0.12, 0.0], Ease::In),
    key(0.60, [-0.065, -0.02, -0.12], [-0.5, 0.11, 0.0], Ease::Out),
    key(0.74, [-0.06, -0.015, -0.115], [-0.47, 0.1, 0.0], Ease::InOut),
    key(1.0, [0.0; 3], [0.0; 3], Ease::InOut),
];

/// The shovel: a small settle, drawn back with the blade lifted, a hard
/// thrust forward and down into the ground (contact), a jar, then levered up
/// and rolled to toss the load aside, and back to rest.
const DIG: [Hand; 7] = [
    REST,
    key(0.10, [0.0, -0.02, 0.01], [-0.06, 0.0, 0.0], Ease::InOut),
    key(0.36, [0.02, 0.06, 0.08], [0.5, 0.05, 0.0], Ease::InOut),
    key(0.50, [-0.03, -0.10, -0.16], [-0.9, 0.1, 0.0], Ease::In),
    key(0.58, [-0.03, -0.09, -0.15], [-0.85, 0.1, 0.0], Ease::Out),
    key(0.78, [0.0, 0.04, -0.08], [0.35, 0.2, 0.35], Ease::InOut),
    key(1.0, [0.0; 3], [0.0; 3], Ease::InOut),
];

/// The sword: a quick settle, drawn back high over the right shoulder with
/// the blade laid back, a fast cut down and across to the left, a short
/// follow-through, back to rest. Lighter and quicker than the axe.
const SLASH: [Hand; 6] = [
    REST,
    key(0.08, [0.01, -0.01, 0.01], [-0.05, 0.04, 0.0], Ease::InOut),
    key(0.38, [0.14, 0.12, 0.08], [0.55, -0.85, -0.7], Ease::InOut),
    key(0.50, [-0.17, -0.07, -0.14], [-0.65, 1.05, 0.45], Ease::In),
    key(0.62, [-0.15, -0.09, -0.11], [-0.7, 0.98, 0.45], Ease::Out),
    key(1.0, [0.0; 3], [0.0; 3], Ease::InOut),
];

/// Holding a sword up across the body to block: blade tipped up and to the
/// left, in front of the face.
const GUARD: [f32; 6] = [-0.13, 0.1, 0.03, 0.5, 1.15, 0.9];

/// The body's arms while holding and swinging (third person), all radians.
/// Signs as `npc::motion::Pose`; `yaw` > 0 turns the right arm in across the
/// chest; `wrist` is the held item's tip down (pitch) and roll about the forearm.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limbs {
    pub twist: f32,
    pub lean: f32,
    /// Right arm.
    pub shoulder: f32,
    pub arm_out: f32,
    pub yaw: f32,
    pub elbow: f32,
    /// Left arm (it comes along on two-handed swings).
    pub left_shoulder: f32,
    pub left_elbow: f32,
    pub wrist: [f32; 2],
}

type Body = Key<10>;

const fn limbs(t: f32, v: [f32; 10], ease: Ease) -> Body {
    Key { t, v, ease }
}

/// [twist, lean, shoulder, arm_out, yaw, elbow, left_shoulder, left_elbow, wrist pitch, wrist roll]
const HOLD: [f32; 10] = [0.0, 0.0, 0.3, 0.08, 0.0, 0.75, 0.0, 0.15, 0.0, 0.0];

const BODY_JAB: [Body; 5] = [
    limbs(0.0, HOLD, Ease::InOut),
    limbs(0.15, [-0.1, 0.0, 0.5, 0.1, 0.0, 1.4, 0.0, 0.15, 0.2, 0.0], Ease::InOut),
    limbs(0.45, [0.2, 0.1, 1.45, 0.05, 0.25, 0.1, -0.2, 0.3, 0.0, 0.0], Ease::In),
    limbs(0.55, [0.18, 0.09, 1.35, 0.05, 0.22, 0.2, -0.18, 0.3, 0.0, 0.0], Ease::Out),
    limbs(1.0, HOLD, Ease::InOut),
];

const BODY_CHOP: [Body; 7] = [
    limbs(0.0, HOLD, Ease::InOut),
    limbs(0.10, [0.05, 0.03, 0.25, 0.08, 0.05, 0.85, 0.05, 0.2, 0.0, 0.0], Ease::InOut),
    limbs(0.42, [-0.85, -0.04, 1.35, 0.35, -1.35, 0.55, 0.9, 0.9, -1.3, -1.4], Ease::InOut),
    limbs(0.54, [0.55, 0.14, 1.45, 0.05, 0.6, 0.1, 1.2, 0.35, -1.3, -1.4], Ease::In),
    limbs(0.60, [0.45, 0.12, 1.42, 0.05, 0.5, 0.15, 1.15, 0.35, -1.25, -1.4], Ease::Out),
    limbs(0.74, [0.43, 0.12, 1.4, 0.05, 0.48, 0.16, 1.1, 0.35, -1.25, -1.4], Ease::InOut),
    limbs(1.0, HOLD, Ease::InOut),
];

const BODY_OVERHEAD: [Body; 7] = [
    limbs(0.0, HOLD, Ease::InOut),
    limbs(0.10, [0.0, 0.04, 0.2, 0.08, 0.0, 0.85, 0.0, 0.15, 0.0, 0.0], Ease::InOut),
    limbs(0.42, [-0.15, -0.15, 2.6, 0.15, 0.2, 0.3, 2.3, 0.6, 0.4, 0.0], Ease::InOut),
    limbs(0.54, [0.1, 0.38, 1.2, 0.05, 0.25, 0.1, 0.8, 0.3, -1.1, 0.0], Ease::In),
    limbs(0.60, [0.08, 0.33, 1.28, 0.05, 0.24, 0.12, 0.85, 0.3, -1.05, 0.0], Ease::Out),
    limbs(0.74, [0.08, 0.32, 1.3, 0.05, 0.23, 0.13, 0.85, 0.3, -1.05, 0.0], Ease::InOut),
    limbs(1.0, HOLD, Ease::InOut),
];

const BODY_SLASH: [Body; 6] = [
    limbs(0.0, HOLD, Ease::InOut),
    limbs(0.08, [0.04, 0.02, 0.3, 0.08, 0.04, 0.8, 0.0, 0.15, 0.0, 0.0], Ease::InOut),
    limbs(0.38, [-0.7, -0.06, 2.3, 0.45, -0.9, 0.9, 0.2, 0.3, 0.5, -1.0], Ease::InOut),
    limbs(0.50, [0.6, 0.16, 1.1, 0.05, 0.75, 0.1, -0.1, 0.3, -1.2, -1.2], Ease::In),
    limbs(0.62, [0.55, 0.18, 0.95, 0.05, 0.7, 0.15, -0.1, 0.3, -1.25, -1.2], Ease::Out),
    limbs(1.0, HOLD, Ease::InOut),
];

const BODY_DIG: [Body; 7] = [
    limbs(0.0, HOLD, Ease::InOut),
    limbs(0.10, [0.0, 0.03, 0.25, 0.08, 0.0, 0.85, 0.3, 0.4, 0.0, 0.0], Ease::InOut),
    limbs(0.36, [-0.2, -0.05, 0.6, 0.1, 0.1, 1.2, 0.9, 0.9, 0.6, 0.0], Ease::InOut),
    limbs(0.50, [0.15, 0.35, 0.9, 0.05, 0.2, 0.15, 0.7, 0.3, -1.2, 0.0], Ease::In),
    limbs(0.58, [0.13, 0.33, 0.92, 0.05, 0.2, 0.18, 0.72, 0.3, -1.15, 0.0], Ease::Out),
    limbs(0.78, [0.35, 0.1, 1.3, 0.1, 0.4, 0.5, 1.1, 0.6, -0.3, -0.6], Ease::InOut),
    limbs(1.0, HOLD, Ease::InOut),
];

/// Blocking: the sword arm up and across, blade raised in front of the face.
const BODY_GUARD: [f32; 10] = [0.3, 0.06, 1.55, 0.12, 0.85, 1.25, 0.4, 0.9, -0.35, -1.45];

impl Limbs {
    fn from_array(v: [f32; 10]) -> Self {
        Self { twist: v[0], lean: v[1], shoulder: v[2], arm_out: v[3], yaw: v[4], elbow: v[5], left_shoulder: v[6], left_elbow: v[7], wrist: [v[8], v[9]] }
    }

    /// Arms holding something out in front, ready to use it.
    pub fn hold() -> Self {
        Self::from_array(HOLD)
    }

    /// Arms raised to block with a sword.
    pub fn guard() -> Self {
        Self::from_array(BODY_GUARD)
    }

    /// `self` blended `k` (0..1) of the way to `other`.
    pub fn lerp(self, other: Self, k: f32) -> Self {
        let (a, b) = (self.to_array(), other.to_array());
        Self::from_array(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k))
    }

    fn to_array(self) -> [f32; 10] {
        [self.twist, self.lean, self.shoulder, self.arm_out, self.yaw, self.elbow, self.left_shoulder, self.left_elbow, self.wrist[0], self.wrist[1]]
    }
}

/// The first-person hand's offset and turn while blocking with a sword.
pub fn guard_pose() -> (Vec3, Quat) {
    let v = GUARD;
    (Vec3::new(v[0], v[1], v[2]), Quat::from_euler(EulerRot::XYZ, v[3], v[4], v[5]))
}

impl Swing {
    fn keys(self) -> &'static [Hand] {
        match self {
            Swing::Jab => &JAB,
            Swing::Chop => &CHOP,
            Swing::Overhead => &OVERHEAD,
            Swing::Slash => &SLASH,
            Swing::Dig => &DIG,
        }
    }

    fn body_keys(self) -> &'static [Body] {
        match self {
            Swing::Jab => &BODY_JAB,
            Swing::Chop => &BODY_CHOP,
            Swing::Overhead => &BODY_OVERHEAD,
            Swing::Slash => &BODY_SLASH,
            Swing::Dig => &BODY_DIG,
        }
    }

    /// Which key is the contact pose (the end of the strike).
    fn contact_key(self) -> usize {
        match self {
            Swing::Jab => 2,
            Swing::Chop | Swing::Overhead | Swing::Slash | Swing::Dig => 3,
        }
    }

    /// Seconds one swing takes. Holding the button swings again and again,
    /// one blow per swing.
    pub fn period(self) -> f32 {
        match self {
            Swing::Jab => 0.32,
            Swing::Chop => 0.55,
            Swing::Overhead => 0.58,
            Swing::Slash => 0.42,
            Swing::Dig => 0.6,
        }
    }

    /// When the blow lands, as a share of the swing (the end of the strike).
    pub fn contact(self) -> f32 {
        self.keys()[self.contact_key()].t
    }

    /// The hand's offset and turn `t` (0..1) of the way through a swing.
    pub fn pose(self, t: f32) -> (Vec3, Quat) {
        let v = sample(self.keys(), t);
        (Vec3::new(v[0], v[1], v[2]), Quat::from_euler(EulerRot::XYZ, v[3], v[4], v[5]))
    }

    /// The body's arms `t` (0..1) of the way through a swing (third person).
    /// Shares its timing with `pose`, so both land at `contact`.
    pub fn limbs(self, t: f32) -> Limbs {
        Limbs::from_array(sample(self.body_keys(), t))
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

    const ALL: [Swing; 5] = [Swing::Jab, Swing::Chop, Swing::Overhead, Swing::Slash, Swing::Dig];

    fn pitch(q: Quat) -> f32 {
        q.to_euler(EulerRot::XYZ).0
    }

    fn yaw(q: Quat) -> f32 {
        q.to_euler(EulerRot::XYZ).1
    }

    /// The angle each swing drives along: down for jabs and picks, across
    /// (toward the middle) for the axe. Smaller = further into the strike.
    fn drive(s: Swing, t: f32) -> f32 {
        let q = s.pose(t).1;
        if matches!(s, Swing::Chop | Swing::Slash) { -yaw(q) } else { pitch(q) }
    }

    #[test]
    fn swings_start_and_end_at_rest() {
        for s in ALL {
            for t in [0.0, 1.0] {
                let (p, q) = s.pose(t);
                assert!(p.length() < 1e-5 && q.angle_between(Quat::IDENTITY) < 1e-5, "{s:?} at {t}");
                let (a, b) = (sample(s.body_keys(), t), HOLD);
                assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5), "{s:?} body at {t}");
            }
        }
    }

    #[test]
    fn keys_are_in_order() {
        for s in ALL {
            let k = s.keys();
            assert!(k.windows(2).all(|w| w[0].t < w[1].t), "{s:?}");
            assert!(s.contact() > 0.0 && s.contact() < 1.0);
            // The body's keys share the hand's timing.
            let b = s.body_keys();
            assert_eq!(k.len(), b.len(), "{s:?}");
            assert!(k.iter().zip(b).all(|(a, b)| a.t == b.t), "{s:?}");
        }
    }

    #[test]
    fn the_blow_lands_at_the_deepest_point_of_the_strike() {
        // The tool is furthest into its strike exactly at contact: the
        // moment the hit is counted is the moment it looks like it hits.
        for s in ALL {
            let deepest = (0..=1000).map(|i| i as f32 / 1000.0).min_by(|a, b| drive(s, *a).total_cmp(&drive(s, *b))).unwrap();
            assert!((deepest - s.contact()).abs() < 0.002, "{s:?}: deepest at {deepest}, blow at {}", s.contact());
        }
    }

    #[test]
    fn strikes_speed_up_into_contact() {
        for s in ALL {
            let c = s.contact();
            let speed = |t: f32| (drive(s, t) - drive(s, t - 0.01)).abs();
            assert!(speed(c) > speed(c - 0.08), "{s:?} should hit hardest at contact");
            // Winding up is slower than the strike.
            assert!(speed(c) > 2.0 * speed(0.3), "{s:?}");
        }
    }

    #[test]
    fn pick_swings_down_and_axe_swings_across() {
        let windup = 0.42;
        // The pick comes from high above straight down: little turn, a big drop.
        let (p0, q0) = Swing::Overhead.pose(windup);
        let (p1, q1) = Swing::Overhead.pose(Swing::Overhead.contact());
        assert!((yaw(q0) - yaw(q1)).abs() < 0.25);
        assert!(p0.y - p1.y > 0.2 && pitch(q0) - pitch(q1) > 1.8);
        // The axe comes across flat: a wide sweep, hardly any drop.
        let (p0, q0) = Swing::Chop.pose(windup);
        let (p1, q1) = Swing::Chop.pose(Swing::Chop.contact());
        assert!(yaw(q1) - yaw(q0) > 2.0);
        assert!(p0.x - p1.x > 0.3 && (p0.y - p1.y).abs() < 0.08);
        // The body does the same: the axe twists the torso across, the pick
        // raises the arm high and brings it down.
        let (a, b) = (Swing::Chop.limbs(windup), Swing::Chop.limbs(Swing::Chop.contact()));
        assert!(b.twist - a.twist > 1.0 && b.yaw - a.yaw > 1.2 && (b.shoulder - a.shoulder).abs() < 0.3);
        let (a, b) = (Swing::Overhead.limbs(windup), Swing::Overhead.limbs(Swing::Overhead.contact()));
        assert!(a.shoulder - b.shoulder > 1.2 && (b.twist - a.twist).abs() < 0.4);
    }

    #[test]
    fn sword_cuts_down_and_across_quicker_than_the_axe() {
        let (p0, q0) = Swing::Slash.pose(0.38);
        let (p1, q1) = Swing::Slash.pose(Swing::Slash.contact());
        // Across (a big turn) and down (a drop), not flat like the axe.
        assert!(yaw(q1) - yaw(q0) > 1.5 && p0.y - p1.y > 0.15 && pitch(q0) - pitch(q1) > 0.8);
        assert!(Swing::Slash.period() < Swing::Chop.period());
        // The guard holds the blade up and across.
        let (_, g) = guard_pose();
        assert!(yaw(g) > 0.8);
        let l = Limbs::hold().lerp(Limbs::guard(), 1.0);
        assert_eq!(l, Limbs::guard());
    }

    #[test]
    fn heavy_tools_bite_and_hold() {
        // After the hit the head stays near where it struck for a beat
        // before it comes back: chunky, not a bounce straight back to rest.
        for s in [Swing::Chop, Swing::Overhead] {
            let c = s.contact();
            let near = (drive(s, c + 0.15) - drive(s, c)).abs();
            let back = (drive(s, 0.0) - drive(s, c)).abs();
            assert!(near < back * 0.25, "{s:?}");
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
