//! Pure motion rules for NPC bodies: springs that ease every joint towards
//! where it should be (so nothing starts or stops dead), the walk cycle, the
//! idle stance and talking gestures, and the gaze (head and pupils).
//! No Bevy here; `body` turns a `Pose` into joint transforms.

use std::f32::consts::{PI, TAU};

use super::dialogue::Anim;
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

/// Every joint of a body, in radians (and blocks for `bob` and `shift`). Left is index 0.
///
/// Signs: `hip`/`shoulder` > 0 swings the limb forward, `knee` > 0 bends it
/// back, `elbow` > 0 bends the forearm up, `arm_out` > 0 lifts the arm away
/// from the side, `lean` > 0 bends the chest forward, `twist` > 0 turns the
/// shoulders left, `roll` > 0 drops the left hip, `nod` > 0 tips the head
/// up, `shake` > 0 turns it left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub bob: f32,
    /// Hips moved sideways (blocks, > 0 to the left): the weight shifting
    /// over the planted foot.
    pub shift: f32,
    pub roll: f32,
    pub lean: f32,
    pub twist: f32,
    pub hip: [f32; 2],
    pub knee: [f32; 2],
    pub shoulder: [f32; 2],
    pub arm_out: [f32; 2],
    pub elbow: [f32; 2],
    pub nod: f32,
    pub shake: f32,
}

impl Pose {
    pub const JOINTS: usize = 17;

    pub fn to_array(self) -> [f32; Self::JOINTS] {
        [
            self.bob,
            self.shift,
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
            self.nod,
            self.shake,
        ]
    }

    pub fn from_array(a: [f32; Self::JOINTS]) -> Self {
        Self {
            bob: a[0],
            shift: a[1],
            roll: a[2],
            lean: a[3],
            twist: a[4],
            hip: [a[5], a[6]],
            knee: [a[7], a[8]],
            shoulder: [a[9], a[10]],
            arm_out: [a[11], a[12]],
            elbow: [a[13], a[14]],
            nod: a[15],
            shake: a[16],
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

    pub fn scale(self, k: f32) -> Pose {
        Pose::from_array(self.to_array().map(|x| x * k))
    }
}

/// A body animation (`dialogue::Anim`) `t` seconds in, added on top of the
/// stance. `leg` is the body's hip height (blocks), so sitting puts any body
/// on the ground. One-offs ease in and out within `Anim::length`; held ones
/// ease in and stay (`body` fades them out when they stop).
pub fn act(anim: Anim, t: f32, leg: f32) -> Pose {
    let len = anim.length();
    // 0 -> 1 over the first `rise` seconds, back to 0 over the last `rise`.
    let env = |rise: f32| {
        let up = smooth(t / rise);
        if anim.held() {
            up
        } else {
            up * smooth((len - t) / rise)
        }
    };
    match anim {
        Anim::Wave => {
            // Hand up beside the head, swinging side to side.
            let e = env(0.4);
            let sweep = (t * 2.6 * TAU).sin() * smooth((t - 0.2) / 0.2);
            Pose {
                twist: -0.08,
                shoulder: [0.0, 0.25],
                arm_out: [0.0, 2.35 + 0.28 * sweep],
                elbow: [0.0, 0.35],
                nod: 0.05,
                ..Pose::default()
            }
            .scale(e)
        }
        Anim::Nod => Pose {
            nod: -0.24 * (t * 1.8 * TAU).sin().abs(),
            ..Pose::default()
        }
        .scale(env(0.15)),
        Anim::HeadShake => Pose {
            shake: 0.34 * (t * 2.0 * TAU).sin(),
            nod: -0.05,
            ..Pose::default()
        }
        .scale(env(0.15)),
        Anim::Shrug => {
            let e = env(0.3);
            Pose {
                arm_out: [0.35, 0.35],
                shoulder: [0.35, 0.35],
                elbow: [1.2, 1.2],
                nod: 0.08,
                roll: 0.04,
                ..Pose::default()
            }
            .scale(e)
        }
        Anim::Bow => Pose {
            lean: 0.65,
            nod: -0.35,
            shoulder: [-0.12, -0.12],
            knee: [0.08, 0.08],
            ..Pose::default()
        }
        .scale(env(0.45)),
        Anim::Point => Pose {
            twist: -0.15,
            shoulder: [0.0, 1.45],
            arm_out: [0.0, 0.08],
            elbow: [0.0, -0.1],
            ..Pose::default()
        }
        .scale(env(0.3)),
        Anim::Cheer => {
            let e = env(0.4);
            let bounce = (t * 2.2 * TAU).sin();
            Pose {
                arm_out: [2.5 + 0.15 * bounce, 2.5 + 0.15 * bounce],
                elbow: [0.25, 0.25],
                nod: 0.15,
                bob: 0.03 * bounce.max(0.0),
                ..Pose::default()
            }
            .scale(e)
        }
        Anim::Give => {
            // Reach out, hold it there to be taken, then the arm comes back.
            Pose {
                lean: 0.12,
                twist: -0.06,
                shoulder: [0.1, 1.35],
                arm_out: [0.0, -0.08],
                elbow: [0.1, 0.2],
                nod: -0.12,
                ..Pose::default()
            }
            .scale(env(0.4))
        }
        Anim::Sit => {
            // Thighs a little above level, shins down to the feet, hips
            // dropped until the seat is on the ground; hands on the knees.
            Pose {
                bob: -0.71 * leg,
                lean: -0.04,
                hip: [1.9, 1.9],
                knee: [1.4, 1.4],
                shoulder: [0.55, 0.55],
                arm_out: [-0.02, -0.02],
                elbow: [0.3, 0.3],
                ..Pose::default()
            }
            .scale(env(0.6))
        }
        Anim::Think => Pose {
            // Right hand to the chin, the left arm across under it.
            shoulder: [0.35, 0.55],
            arm_out: [-0.22, -0.3],
            elbow: [1.35, 2.25],
            nod: -0.12,
            ..Pose::default()
        }
        .scale(env(0.4)),
        Anim::Stand => Pose::default(),
    }
}

/// 0 below 0, 1 above 1, smooth in between.
fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Standing: weight shifting slowly from foot to foot, breathing, arms hanging
/// a little away from the body with soft elbows. `t` is seconds.
pub fn idle(t: f32) -> Pose {
    let breath = (t * 1.7).sin();
    let shift = (t * 0.37).sin();
    Pose {
        bob: 0.006 * breath,
        shift: -0.012 * shift,
        roll: 0.025 * shift,
        lean: 0.02 + 0.01 * breath,
        twist: 0.03 * (t * 0.23).sin(),
        hip: [0.02 * shift, -0.02 * shift],
        knee: [
            0.04 + 0.04 * shift.max(0.0),
            0.04 + 0.04 * (-shift).max(0.0),
        ],
        shoulder: [0.03 * breath, 0.03 * (t * 1.7 + 0.6).sin()],
        arm_out: [0.07 + 0.015 * breath, 0.07 + 0.015 * breath],
        elbow: [0.14, 0.16],
        ..Pose::default()
    }
}

/// How far each leg swings from the hip (radians) at `pace` 0..2: a little
/// longer as it speeds up into a jog.
fn swing_of(pace: f32) -> f32 {
    let a = pace.clamp(0.0, 2.0);
    0.46 + 0.18 * (a - 1.0).max(0.0)
}

/// Ground covered by one full stride (two steps) at `pace`, blocks. Long,
/// unhurried strides: about two strides a second at walking speed. The feet
/// reach a little less than this, which reads as a relaxed glide rather than
/// a quick shuffle.
pub fn stride(pace: f32) -> f32 {
    1.95 + 0.45 * (pace.clamp(1.0, 2.0) - 1.0)
}

/// One stride of walking (`phase` in radians, a full turn = two steps) at
/// `pace` 0 (standing) .. 1 (walking) .. 2 (jogging).
///
/// Everything moves on smooth curves with no corners: the knee folds as the
/// leg comes through and straightens as it reaches forward; the hips sway
/// over the planted foot and roll with it; the chest counter-turns against
/// the legs and the arms swing a beat behind them, the forearms trailing, so
/// the body flows instead of moving as one block. No up-and-down bob.
pub fn walk(phase: f32, pace: f32) -> Pose {
    let a = pace.clamp(0.0, 2.0);
    let run = (a - 1.0).max(0.0);
    let swing = swing_of(a);
    let (s, c) = phase.sin_cos();
    // The upper body trails the legs a little.
    let (ls, lc) = (phase - 0.35).sin_cos();
    // Knee: folds most as the leg passes under the body coming forward
    // (cos = 1 for the left), straight-ish as it plants. Smooth both ways.
    let fold = |c: f32| ((1.0 + c) * 0.5).powi(3);
    let knee = 0.6 + 0.35 * run;
    // Forearm bends more on the back swing.
    let curl = |x: f32| (1.0 - x) * 0.5;
    let arm = 0.3 + 0.15 * run;
    Pose {
        bob: -0.02 * run,
        shift: -a * 0.025 * ls,
        roll: a * 0.035 * ls,
        lean: a * 0.035 + 0.06 * run,
        twist: -a * (0.11 + 0.04 * run) * ls,
        hip: [a * swing * s, -a * swing * s],
        knee: [a * (0.08 + knee * fold(c)), a * (0.08 + knee * fold(-c))],
        shoulder: [-a * arm * ls, a * arm * ls],
        arm_out: [a.min(1.0) * 0.06 + a * 0.02, a.min(1.0) * 0.06 + a * 0.02],
        elbow: [
            a * (0.18 + 0.3 * run + 0.22 * curl(lc)),
            a * (0.18 + 0.3 * run + 0.22 * curl(-lc)),
        ],
        ..Pose::default()
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
        let pupil = [
            self.pupils[0].step(px, 0.05, dt),
            self.pupils[1].step(py, 0.05, dt),
        ];
        let twist = (head_yaw.abs() - 0.6).max(0.0) * head_yaw.signum() * 0.5;
        Look {
            head_yaw,
            head_pitch,
            twist,
            pupil,
            blink: self.step_blink(dt),
        }
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
                self.next_blink = if self.double {
                    0.08
                } else {
                    self.rng.range(1.8, 5.5)
                };
            }
        }
        // Lids close fast and open a little slower.
        self.blink = if self.blink_t < 0.0 {
            0.0
        } else {
            let u = self.blink_t / BLINK;
            if u < 0.4 {
                (u / 0.4).powf(0.7)
            } else {
                1.0 - ((u - 0.4) / 0.6).powf(1.4)
            }
        };
        self.blink
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hip to sole, blocks (the body's leg length).
    const LEG: f32 = 0.76;

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
        assert!(
            a.hip[0] > 0.3 && a.hip[1] < -0.3,
            "left forward, right back"
        );
        assert!(
            a.shoulder[0] < 0.0 && a.shoulder[1] > 0.0,
            "arms swing against the legs"
        );
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
    fn walking_is_unhurried_and_level() {
        // About two strides a second at walking speed (4.4 blocks/s), a bit
        // quicker jogging; the feet still reach most of the way.
        let per_second = |speed: f32, pace: f32| speed / stride(pace);
        assert!((1.9..2.5).contains(&per_second(4.4, 1.0)));
        assert!(per_second(6.8, 1.55) > per_second(4.4, 1.0));
        let reach = 4.0 * LEG * swing_of(1.0).sin();
        assert!(reach > stride(1.0) * 0.6, "feet reach most of the stride");
        // Walking doesn't bob the body up and down.
        for i in 0..100 {
            assert_eq!(walk(i as f32 * 0.07, 1.0).bob, 0.0);
        }
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
        assert!(
            last.pupil[0].abs() < 0.35,
            "pupils come back near the middle"
        );
        // Too far round for the head alone: it stops at its limit.
        for _ in 0..240 {
            last = g.step(1.0 / 60.0, Some([2.5, 0.0]));
        }
        assert!(last.head_yaw <= HEAD_YAW + 1e-3);
        assert!(last.pupil[0] > 0.9, "the eyes look the rest of the way");
    }

    #[test]
    fn one_off_animations_start_and_end_at_rest() {
        for a in [
            Anim::Wave,
            Anim::Nod,
            Anim::HeadShake,
            Anim::Shrug,
            Anim::Bow,
            Anim::Point,
            Anim::Cheer,
            Anim::Give,
        ] {
            assert_eq!(act(a, 0.0, LEG), Pose::default(), "{a:?} starts at rest");
            assert!(
                act(a, a.length(), LEG)
                    .to_array()
                    .iter()
                    .all(|x| x.abs() < 1e-4),
                "{a:?} ends at rest"
            );
            let mid = act(a, a.length() * 0.4, LEG).to_array();
            assert!(mid.iter().any(|x| x.abs() > 0.05), "{a:?} moves");
            // No snapping from frame to frame.
            for i in 0..200 {
                let t = a.length() * i as f32 / 200.0;
                let (x, y) = (
                    act(a, t, LEG).to_array(),
                    act(a, t + 1.0 / 60.0, LEG).to_array(),
                );
                for j in 0..Pose::JOINTS {
                    assert!((x[j] - y[j]).abs() < 0.2, "{a:?} joint {j} jumps at {t}");
                }
            }
        }
    }

    #[test]
    fn sitting_holds_and_puts_the_seat_down() {
        let p = act(Anim::Sit, 5.0, LEG);
        assert_eq!(p, act(Anim::Sit, 50.0, LEG), "held");
        // Hip height after the drop: low, but still above the ground.
        let seat = LEG + p.bob;
        assert!((0.1..0.35).contains(&seat), "seat at {seat}");
        assert!(p.hip[0] > 1.5, "thighs forward");
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
