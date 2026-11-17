//! Pure motion rules for NPC bodies: springs that ease every joint towards
//! where it should be (so nothing starts or stops dead), the walk cycle, the
//! idle stance and talking gestures, and the gaze (head and pupils).
//! No Bevy here; `body` turns a `Pose` into joint transforms.

use std::f32::consts::{PI, TAU};

use crate::creatures::brain::Rng;

/// A critically damped spring: `x` follows a moving target on a smooth curve,
/// with no overshoot, about `time` seconds behind.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
}

impl Spring {
    pub fn new(x: f32) -> Self {
        Self { x, v: 0.0 }
    }

    pub fn step(&mut self, target: f32, time: f32, dt: f32) -> f32 {
        let omega = 2.0 / time.max(1e-4);
        let k = omega * dt;
        let decay = 1.0 / (1.0 + k + 0.48 * k * k + 0.235 * k * k * k);
        let change = self.x - target;
        let temp = (self.v + omega * change) * dt;
        self.v = (self.v - omega * temp) * decay;
        self.x = target + (change + temp) * decay;
        self.x
    }

    /// Like `step`, for an angle: goes the short way round.
    pub fn step_angle(&mut self, target: f32, time: f32, dt: f32) -> f32 {
        let near = self.x + wrap(target - self.x);
        self.step(near, time, dt);
        self.x = wrap(self.x);
        self.x
    }
}

/// An angle in -π..π.
pub fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// Every joint of a body, in radians (and blocks for `bob`). Left is index 0.
///
/// Signs: `hip`/`shoulder` > 0 swings the limb forward, `knee` > 0 bends it
/// back, `elbow` > 0 bends the forearm up, `arm_out` > 0 lifts the arm away
/// from the side, `lean` > 0 bends the chest forward, `twist` > 0 turns the
/// shoulders left, `roll` > 0 drops the left hip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub bob: f32,
    pub roll: f32,
    pub lean: f32,
    pub twist: f32,
    pub hip: [f32; 2],
    pub knee: [f32; 2],
    pub shoulder: [f32; 2],
    pub arm_out: [f32; 2],
    pub elbow: [f32; 2],
}

impl Pose {
    pub const JOINTS: usize = 14;

    pub fn to_array(self) -> [f32; Self::JOINTS] {
        [
            self.bob,
            self.roll,
            self.lean,
            self.twist,
            self.hip[0],
            self.hip[1],
            self.knee[0],
            self.knee[1],
            self.shoulder[0],
            self.shoulder[1],
            self.arm_out[0],
            self.arm_out[1],
            self.elbow[0],
            self.elbow[1],
        ]
    }

    pub fn from_array(a: [f32; Self::JOINTS]) -> Self {
        Self {
            bob: a[0],
            roll: a[1],
            lean: a[2],
            twist: a[3],
            hip: [a[4], a[5]],
            knee: [a[6], a[7]],
            shoulder: [a[8], a[9]],
            arm_out: [a[10], a[11]],
            elbow: [a[12], a[13]],
        }
    }

    pub fn lerp(self, other: Pose, t: f32) -> Pose {
        let (a, b) = (self.to_array(), other.to_array());
        Pose::from_array(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t))
    }

    pub fn add(self, other: Pose) -> Pose {
        let (a, b) = (self.to_array(), other.to_array());
        Pose::from_array(std::array::from_fn(|i| a[i] + b[i]))
    }
}

/// Standing: weight shifting slowly from foot to foot, breathing, arms hanging
/// a little away from the body with soft elbows. `t` is seconds.
pub fn idle(t: f32) -> Pose {
    let breath = (t * 1.7).sin();
    let shift = (t * 0.37).sin();
    Pose {
        bob: 0.006 * breath,
        roll: 0.025 * shift,
        lean: 0.02 + 0.01 * breath,
        twist: 0.03 * (t * 0.23).sin(),
        hip: [0.02 * shift, -0.02 * shift],
        knee: [0.04 + 0.04 * shift.max(0.0), 0.04 + 0.04 * (-shift).max(0.0)],
        shoulder: [0.03 * breath, 0.03 * (t * 1.7 + 0.6).sin()],
        arm_out: [0.07 + 0.015 * breath, 0.07 + 0.015 * breath],
        elbow: [0.14, 0.16],
    }
}

/// Hip to sole, blocks (the body's leg length).
pub const LEG: f32 = 0.76;

/// How far each leg swings from the hip (radians) at `pace` 0..2: bigger
/// strides as it speeds up into a jog.
fn swing_of(pace: f32) -> f32 {
    let a = pace.clamp(0.0, 2.0);
    0.55 + 0.25 * (a - 1.0).max(0.0)
}

/// Ground covered by one full stride (two steps) at `pace`: the planted foot
/// sweeps from `swing` in front to `swing` behind, so the feet keep their grip
/// when the cycle advances by distance travelled over this.
pub fn stride(pace: f32) -> f32 {
    // Bent knees shorten the reach a little.
    2.0 * 2.0 * LEG * swing_of(pace.max(1.0)).sin() * 0.9
}

/// One stride of walking (`phase` in radians, a full turn = two steps) at
/// `pace` 0 (standing) .. 1 (walking) .. 2 (jogging).
pub fn walk(phase: f32, pace: f32) -> Pose {
    let a = pace.clamp(0.0, 2.0);
    let swing = swing_of(a);
    let run = (a - 1.0).max(0.0);
    let (s, c) = phase.sin_cos();
    // A leg bends at the knee while it swings forward (cos > 0 for the left).
    let lift = |c: f32| c.max(0.0).powf(0.8);
    // Weight: the body drops hard as each heel lands (twice per stride) and
    // rises over the planted leg.
    let step = (2.0 * phase).cos();
    let drop = -(-step).max(0.0).powi(2);
    Pose {
        bob: a * (0.025 * step + 0.03 * drop) - 0.03 * run,
        roll: a * 0.05 * s,
        lean: a * 0.07 + 0.08 * run,
        twist: -a * (0.12 + 0.05 * run) * s,
        hip: [a * swing * s, -a * swing * s],
        knee: [a * (0.12 + (0.65 + 0.35 * run) * lift(c)), a * (0.12 + (0.65 + 0.35 * run) * lift(-c))],
        // Arms swing against the legs, the forearm trailing a little.
        shoulder: [-a * (0.4 + 0.15 * run) * s, a * (0.4 + 0.15 * run) * s],
        arm_out: [a.min(1.0) * 0.06 + a * 0.02, a.min(1.0) * 0.06 + a * 0.02],
        elbow: [a * (0.2 + 0.3 * run + 0.25 * (-s).max(0.0)), a * (0.2 + 0.3 * run + 0.25 * s.max(0.0))],
    }
}

/// Where the body faces while moving, Minecraft-style: it turns toward the
/// way you are going, but never more than `BODY_REACH` from where the head
/// looks; walking backwards it keeps facing forwards (the legs walk back).
/// `look` is the head's yaw, `going` the yaw of the movement.
pub fn moving_facing(look: f32, going: f32) -> f32 {
    let mut off = wrap(going - look);
    if off.abs() > BACKWARDS {
        off = wrap(off + PI);
    }
    look + off.clamp(-BODY_REACH, BODY_REACH)
}

/// The body turns at most this far from the head (Minecraft uses 75°).
pub const BODY_REACH: f32 = 75.0 * PI / 180.0;
/// Moving more than this far off the look direction counts as backwards.
const BACKWARDS: f32 = 95.0 * PI / 180.0;

/// Hands that move while talking, added on top of the stance. `energy` 0..1
/// is how much the speaker is putting into it; `t` is seconds.
pub fn gesture(t: f32, energy: f32) -> Pose {
    let e = energy.clamp(0.0, 1.0);
    // Two slow waves so the beat never quite repeats.
    let w1 = (t * 2.3).sin();
    let w2 = (t * 1.3 + 1.1).sin();
    Pose {
        lean: e * 0.03 * w2,
        twist: e * 0.05 * w1,
        shoulder: [e * (0.25 + 0.12 * w2), e * (0.45 + 0.18 * w1)],
        arm_out: [e * 0.1, e * (0.12 + 0.06 * w2)],
        elbow: [e * (0.6 + 0.2 * w1), e * (0.95 + 0.3 * w2)],
        ..Pose::default()
    }
}

/// How far the head turns before the pupils and shoulders take the rest.
pub const HEAD_YAW: f32 = 1.05;
pub const HEAD_PITCH: f32 = 0.55;
/// Pupil travel: an angle this far off-centre puts the pupil at the eye's edge.
pub const PUPIL_REACH: f32 = 0.45;

/// Where a body looks: the head turns on a slow spring, the pupils dart
/// ahead on a quick one, the eyes blink. With nothing to look at it glances
/// about by itself.
#[derive(Clone, Debug)]
pub struct Gaze {
    /// Head yaw / pitch relative to the body.
    pub head: [Spring; 2],
    /// Pupil offset, -1..1 across the eye.
    pub pupils: [Spring; 2],
    /// 0 = open, 1 = shut.
    pub blink: f32,
    /// Where it wants to look (yaw, pitch) when idle.
    wander: [f32; 2],
    /// Pupils' own small darts around the target.
    dart: [f32; 2],
    next_wander: f32,
    next_dart: f32,
    next_blink: f32,
    blink_t: f32,
    double: bool,
    rng: Rng,
}

pub struct Look {
    pub head_yaw: f32,
    pub head_pitch: f32,
    /// How much the shoulders join in (radians of twist).
    pub twist: f32,
    pub pupil: [f32; 2],
    pub blink: f32,
}

impl Gaze {
    pub fn new(seed: u64) -> Self {
        Self {
            head: [Spring::default(); 2],
            pupils: [Spring::default(); 2],
            blink: 0.0,
            wander: [0.0; 2],
            dart: [0.0; 2],
            next_wander: 1.0,
            next_dart: 0.5,
            next_blink: 2.0,
            blink_t: -1.0,
            double: false,
            rng: Rng(seed | 1),
        }
    }

    /// `focus` = (yaw, pitch) of what it is looking at, relative to the body.
    pub fn step(&mut self, dt: f32, focus: Option<[f32; 2]>) -> Look {
        // Idle glances: somewhere new every couple of seconds, mostly ahead.
        self.next_wander -= dt;
        if self.next_wander <= 0.0 {
            self.next_wander = self.rng.range(1.2, 4.0);
            let ahead = self.rng.unit() < 0.4;
            self.wander = if ahead {
                [0.0, self.rng.range(-0.08, 0.05)]
            } else {
                [self.rng.range(-0.9, 0.9), self.rng.range(-0.3, 0.2)]
            };
        }
        // Small darts of the eyes, even when fixed on something.
        self.next_dart -= dt;
        if self.next_dart <= 0.0 {
            self.next_dart = self.rng.range(0.35, 1.6);
            self.dart = [self.rng.range(-0.12, 0.12), self.rng.range(-0.08, 0.08)];
        }
        let want = focus.unwrap_or(self.wander);
        let yaw = want[0].clamp(-HEAD_YAW * 1.6, HEAD_YAW * 1.6);
        let pitch = want[1].clamp(-1.0, 1.0);
        // The head is slower than the eyes; when idle it's lazier still.
        let lazy = if focus.is_some() { 0.32 } else { 0.55 };
        let head_yaw = self.head[0].step(yaw.clamp(-HEAD_YAW, HEAD_YAW), lazy, dt);
        let head_pitch = self.head[1].step(pitch.clamp(-HEAD_PITCH, HEAD_PITCH), lazy, dt);
        // Pupils cover what the head hasn't turned yet: they lead.
        let px = ((yaw - head_yaw) / PUPIL_REACH + self.dart[0]).clamp(-1.0, 1.0);
        let py = ((pitch - head_pitch) / PUPIL_REACH + self.dart[1]).clamp(-1.0, 1.0);
        let pupil = [self.pupils[0].step(px, 0.05, dt), self.pupils[1].step(py, 0.05, dt)];
        let twist = (head_yaw.abs() - 0.6).max(0.0) * head_yaw.signum() * 0.5;
        Look { head_yaw, head_pitch, twist, pupil, blink: self.step_blink(dt) }
    }

    fn step_blink(&mut self, dt: f32) -> f32 {
        const BLINK: f32 = 0.16;
        if self.blink_t < 0.0 {
            self.next_blink -= dt;
            if self.next_blink <= 0.0 {
                self.blink_t = 0.0;
            }
        } else {
            self.blink_t += dt;
            if self.blink_t >= BLINK {
                self.blink_t = -1.0;
                // Now and then a second blink straight after.
                self.double = !self.double && self.rng.unit() < 0.2;
                self.next_blink = if self.double { 0.08 } else { self.rng.range(1.8, 5.5) };
            }
        }
        // Lids close fast and open a little slower.
        self.blink = if self.blink_t < 0.0 {
            0.0
        } else {
            let u = self.blink_t / BLINK;
            if u < 0.4 { (u / 0.4).powf(0.7) } else { 1.0 - ((u - 0.4) / 0.6).powf(1.4) }
        };
        self.blink
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_eases_without_overshoot() {
        let mut s = Spring::new(0.0);
        let mut last = 0.0;
        for _ in 0..120 {
            let x = s.step(1.0, 0.2, 1.0 / 60.0);
            assert!(x >= last - 1e-6, "never backs off");
            assert!(x <= 1.0 + 1e-4, "never overshoots");
            last = x;
        }
        assert!((last - 1.0).abs() < 0.01);
        // Starts slowly: the first frame moves less than a straight lerp would.
        let mut s = Spring::new(0.0);
        assert!(s.step(1.0, 0.2, 1.0 / 60.0) < 0.1);
    }

    #[test]
    fn angles_take_the_short_way() {
        let mut s = Spring::new(3.0);
        for _ in 0..200 {
            s.step_angle(-3.0, 0.1, 1.0 / 60.0);
        }
        // Through π, not back through 0.
        assert!((wrap(s.x - -3.0)).abs() < 0.01);
        let mut s = Spring::new(3.0);
        s.step_angle(-3.0, 0.1, 1.0 / 60.0);
        assert!(s.x.abs() > 2.9);
    }

    #[test]
    fn walk_is_smooth_and_alternates() {
        let a = walk(PI / 2.0, 1.0);
        assert!(a.hip[0] > 0.3 && a.hip[1] < -0.3, "left forward, right back");
        assert!(a.shoulder[0] < 0.0 && a.shoulder[1] > 0.0, "arms swing against the legs");
        // Standing pace is no motion at all.
        let still = walk(1.3, 0.0);
        assert_eq!(still.hip, [0.0, 0.0]);
        // Small steps of phase make small changes of pose: no snapping.
        for i in 0..400 {
            let p = i as f32 * 0.02;
            let (x, y) = (walk(p, 1.0).to_array(), walk(p + 0.02, 1.0).to_array());
            for j in 0..Pose::JOINTS {
                assert!((x[j] - y[j]).abs() < 0.05, "joint {j} jumps at phase {p}");
            }
        }
    }

    #[test]
    fn stride_matches_the_feet() {
        // While a foot is planted it moves back as fast as the body moves
        // forward: one stride's sweep of both feet covers `stride`.
        for pace in [1.0f32, 1.5, 2.0] {
            let sweep = 2.0 * 2.0 * LEG * swing_of(pace).sin();
            let st = stride(pace);
            assert!(st > sweep * 0.85 && st <= sweep, "{pace}: {st} vs {sweep}");
        }
        assert!(stride(2.0) > stride(1.0), "longer strides when jogging");
    }

    #[test]
    fn body_turns_toward_a_strafe_but_not_past_its_reach() {
        let look = 0.3;
        // Straight ahead: no turn.
        assert!((moving_facing(look, look) - look).abs() < 1e-5);
        // Diagonal forward-left: the body faces the way it goes.
        assert!((moving_facing(look, look + 0.7) - (look + 0.7)).abs() < 1e-5);
        // Sideways: turned as far as it may.
        assert!((moving_facing(look, look + PI / 2.0) - (look + BODY_REACH)).abs() < 1e-5);
        assert!((moving_facing(look, look - PI / 2.0) - (look - BODY_REACH)).abs() < 1e-5);
        // Backwards: still facing forwards; back-left turns the body right a little.
        assert!((wrap(moving_facing(look, look + PI) - look)).abs() < 1e-5);
        let back_left = moving_facing(look, look + PI - 0.5);
        assert!((wrap(back_left - (look - 0.5))).abs() < 1e-5);
    }

    #[test]
    fn gaze_turns_head_and_leads_with_the_eyes() {
        let mut g = Gaze::new(7);
        let first = g.step(1.0 / 60.0, Some([0.8, 0.0]));
        // Eyes are already on the way, the head has barely moved.
        assert!(first.pupil[0] > 0.0);
        assert!(first.head_yaw < 0.1);
        let mut last = first;
        for _ in 0..180 {
            last = g.step(1.0 / 60.0, Some([0.8, 0.0]));
        }
        assert!((last.head_yaw - 0.8).abs() < 0.02, "head ends up there");
        assert!(last.pupil[0].abs() < 0.35, "pupils come back near the middle");
        // Too far round for the head alone: it stops at its limit.
        for _ in 0..240 {
            last = g.step(1.0 / 60.0, Some([2.5, 0.0]));
        }
        assert!(last.head_yaw <= HEAD_YAW + 1e-3);
        assert!(last.pupil[0] > 0.9, "the eyes look the rest of the way");
    }

    #[test]
    fn eyes_blink_by_themselves() {
        let mut g = Gaze::new(3);
        let mut shut = 0;
        for _ in 0..(60 * 20) {
            if g.step(1.0 / 60.0, None).blink > 0.9 {
                shut += 1;
            }
        }
        assert!(shut > 3, "blinked over 20 seconds");
    }
}
