//! Cutscenes as data (`assets/data/cutscenes.ron`), their triggers and the step
//! runner. Pure: no Bevy systems, so it can be tested on its own.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bevy::math::{IVec3, Vec3};
use serde::Deserialize;

/// Where something is: from an anchor, plus an offset. From `Player` the offset
/// is (right, up, ahead) of where you faced when the cutscene began; from an
/// actor or a place it is plain world x, y, z.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Mark {
    pub from: Anchor,
    #[serde(default)]
    pub offset: (f32, f32, f32),
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Anchor {
    Player,
    /// An actor (NPC dialogue key, e.g. "bram").
    Actor(String),
    /// A named place: camp, gate, sporrow, castle, forge, spawn.
    Place(String),
    /// A cutscene airship (`Step::Ship`), wherever it has flown to (offset in world x, y, z).
    Ship(String),
}

/// What the camera or an actor looks at.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Target {
    Player,
    Actor(String),
    Mark(Mark),
    /// A cutscene airship.
    Ship(String),
}

/// What a cutscene airship looks like.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShipLook {
    /// Your own airship, you at the helm.
    Ours,
    /// A court warship (red envelope).
    Court,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Trigger {
    /// You are within `within` blocks of an NPC (dialogue key), and see it if `sight`.
    Near { npc: String, within: f32, sight: bool },
    /// You come within `within` blocks of a named place.
    Area { place: String, within: f32 },
    /// An encounter (encounters.ron id) was beaten.
    Cleared(String),
    /// The Rotting Gate's boss at this depth was beaten.
    Boss(u8),
    /// You made this item (at a bench).
    Made(String),
    /// You placed this block.
    Placed(String),
    /// The hour is between `from` and `to` (wrapping past midnight) once `flag` is set.
    TimeAfter { flag: String, from: f32, to: f32 },
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum Step {
    /// An actor says a line (lines.ron id). Actor "player" is you.
    Say(String, String),
    /// Turn the camera to look at a target over this many seconds (it keeps looking).
    Camera(Target, f32),
    /// Move the camera to a mark over this many seconds, looking at a target.
    CameraMove(Mark, Target, f32),
    /// An actor walks to a mark (waits till it gets there).
    Walk(String, Mark),
    /// An actor turns to face a target.
    Face(String, Target),
    Wait(f32),
    /// Fade the screen to this much black (0 clear, 1 black) over this many seconds.
    Fade(f32, f32),
    /// Start an encounter and wait until it is beaten.
    Encounter(String),
    Flag(String),
    /// A clue for the journal.
    Clue(String),
    /// Give you an item: name and count.
    Give(String, u16),
    /// Swap every `from` block for `to` within `radius` of a mark.
    Swap { at: Mark, radius: i32, from: String, to: String },
    /// Put a block at a mark.
    Place(Mark, String),
    /// An actor follows you from now on (rescued, recruited).
    Follow(String),
    /// An actor leaves the world for good.
    Remove(String),

    // ---- Free cameras and effects ----
    /// Put the camera at a mark at once, looking straight at a target.
    Cut(Mark, Target),
    /// Circle the camera round a target: distance, height above it, the angle to start
    /// from and how far to turn (degrees; 0 = behind the target as you faced at the start,
    /// positive turns to the right), over seconds. Doesn't wait (follow it with a `Wait`).
    Orbit { around: Target, radius: f32, height: f32, from: f32, turn: f32, secs: f32 },
    /// Fall: the camera drops to a mark, faster and faster, looking at a target, over seconds. Doesn't wait.
    Dive(Mark, Target, f32),
    /// Field of view to this many degrees over seconds (0 = back to the setting). Doesn't wait.
    Zoom(f32, f32),
    /// Shake the camera this hard (about 1 = a big blast) for seconds, dying away. Doesn't wait.
    Shake(f32, f32),
    /// Tip the camera sideways to this many degrees over seconds. Doesn't wait.
    Roll(f32, f32),
    /// A white flash that fades over seconds. Doesn't wait.
    Flash(f32),
    /// Eyelids: 0 shut, 1 open, over seconds. Doesn't wait.
    Lids(f32, f32),
    /// Play a sound (sounds.ron / assets/sounds name).
    Sound(String),

    // ---- Airships (props that only live in the cutscene) ----
    /// An airship appears: name, mark, which way the bow points (degrees from the way you
    /// faced at the start, positive to the right), and its look.
    Ship { name: String, at: Mark, yaw: f32, look: ShipLook },
    /// A ship flies to a mark over seconds, nose along its way. Doesn't wait.
    ShipTo(String, Mark, f32),
    /// One ship fires at another: a bolt flies, the target is hit (blast, then it burns). Doesn't wait.
    Fire(String, String),
    /// A ship goes.
    Unship(String),
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Cutscene {
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// `None`: only started by a story beat, an encounter's `then`, or `BLOCKGAME_CUTSCENE`.
    #[serde(default)]
    pub trigger: Option<Trigger>,
    /// Flags that must be set first.
    #[serde(default)]
    pub needs: Vec<String>,
    /// The actors and the mark each starts on if it isn't nearby.
    #[serde(default)]
    pub actors: BTreeMap<String, Mark>,
    pub steps: Vec<Step>,
}

pub fn load(text: &str) -> Result<Vec<Cutscene>, String> {
    let ron = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let list: Vec<Cutscene> = ron.from_str(text).map_err(|e| e.to_string())?;
    for c in &list {
        for s in &c.steps {
            let who = match s {
                Step::Say(a, _) | Step::Walk(a, _) | Step::Face(a, _) | Step::Follow(a) | Step::Remove(a) => Some(a),
                _ => None,
            };
            if let Some(a) = who
                && a != PLAYER
                && !c.actors.contains_key(a)
            {
                return Err(format!("{}: `{a}` acts but isn't in its actors", c.id));
            }
        }
    }
    Ok(list)
}

/// The actor name for you.
pub const PLAYER: &str = "player";

/// The `Places` key a cutscene airship's position is kept under.
pub fn ship_key(name: &str) -> String {
    format!("ship:{name}")
}

// ---- Triggers ----

/// What the triggers look at, gathered by the game each check.
#[derive(Default)]
pub struct Facts<'a> {
    pub player: Vec3,
    /// NPCs by dialogue key: where, and whether you can see them.
    pub npcs: Vec<(String, Vec3, bool)>,
    pub places: HashMap<String, Vec3>,
    pub cleared: Option<&'a BTreeSet<String>>,
    /// The deepest Rotting Gate depth whose boss is beaten.
    pub boss: u8,
    pub made: Option<&'a BTreeSet<String>>,
    pub placed: Option<&'a BTreeSet<String>>,
    pub hour: f32,
    pub flags: Option<&'a BTreeSet<String>>,
}

fn has(set: Option<&BTreeSet<String>>, x: &str) -> bool {
    set.is_some_and(|s| s.contains(x))
}

pub fn fires(t: &Trigger, f: &Facts) -> bool {
    match t {
        Trigger::Near { npc, within, sight } => f.npcs.iter().any(|(k, p, seen)| k == npc && p.distance(f.player) <= *within && (*seen || !sight)),
        Trigger::Area { place, within } => f.places.get(place).is_some_and(|p| p.distance(f.player) <= *within),
        Trigger::Cleared(id) => has(f.cleared, id),
        Trigger::Boss(d) => f.boss >= *d,
        Trigger::Made(item) => has(f.made, item),
        Trigger::Placed(block) => has(f.placed, block),
        Trigger::TimeAfter { flag, from, to } => has(f.flags, flag) && if from <= to { (*from..*to).contains(&f.hour) } else { f.hour >= *from || f.hour < *to },
    }
}

/// A cutscene that should start now: not played yet, its flags set, its trigger met.
pub fn ready(c: &Cutscene, played: &BTreeSet<String>, f: &Facts) -> bool {
    !played.contains(&c.id) && c.needs.iter().all(|n| has(f.flags, n)) && c.trigger.as_ref().is_some_and(|t| fires(t, f))
}

/// Nothing solid between two points (`solid` says which cells block the view).
pub fn clear_line(from: Vec3, to: Vec3, solid: impl Fn(IVec3) -> bool) -> bool {
    let d = to - from;
    let n = (d.length() / 0.25).ceil().max(1.0) as i32;
    let (start, end) = (from.floor().as_ivec3(), to.floor().as_ivec3());
    (1..n).all(|i| {
        let c = (from + d * (i as f32 / n as f32)).floor().as_ivec3();
        c == start || c == end || !solid(c)
    })
}

// ---- The runner ----

/// A step whose effect stays when the cutscene is skipped.
pub fn lasts(s: &Step) -> bool {
    matches!(s, Step::Flag(_) | Step::Clue(_) | Step::Give(..) | Step::Swap { .. } | Step::Place(..) | Step::Encounter(_) | Step::Follow(_) | Step::Remove(_))
}

/// How a step ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Until {
    /// Straight away (the next step starts in the same frame).
    Now,
    Seconds(f32),
    /// The line has been said (or clicked past).
    LineSaid,
    /// The walking actor got there.
    Arrived,
    /// The encounter is beaten.
    Cleared,
}

pub fn until(s: &Step) -> Until {
    match s {
        Step::Say(..) => Until::LineSaid,
        Step::Camera(_, t) | Step::CameraMove(_, _, t) | Step::Wait(t) | Step::Fade(_, t) => Until::Seconds(*t),
        Step::Walk(..) => Until::Arrived,
        Step::Encounter(_) => Until::Cleared,
        Step::Face(..)
        | Step::Flag(_)
        | Step::Clue(_)
        | Step::Give(..)
        | Step::Swap { .. }
        | Step::Place(..)
        | Step::Follow(_)
        | Step::Remove(_)
        | Step::Cut(..)
        | Step::Orbit { .. }
        | Step::Dive(..)
        | Step::Zoom(..)
        | Step::Shake(..)
        | Step::Roll(..)
        | Step::Flash(_)
        | Step::Lids(..)
        | Step::Sound(_)
        | Step::Ship { .. }
        | Step::ShipTo(..)
        | Step::Fire(..)
        | Step::Unship(_) => Until::Now,
    }
}

/// Whether the step under way is done (the game fills in what it knows).
#[derive(Debug, Clone, Copy, Default)]
pub struct Status {
    pub line_said: bool,
    pub arrived: bool,
    pub cleared: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Runner {
    /// The step under way (== steps.len() when over).
    pub at: usize,
    /// Seconds into it.
    pub t: f32,
    started: bool,
}

impl Runner {
    /// Time passes. Returns the steps that begin now, in order (the game starts each).
    pub fn tick(&mut self, steps: &[Step], dt: f32, status: Status) -> Vec<usize> {
        let mut begin = Vec::new();
        let mut fresh = false;
        while self.at < steps.len() {
            if !self.started {
                self.started = true;
                self.t = 0.0;
                begin.push(self.at);
                fresh = true;
            } else if !fresh {
                self.t += dt;
            }
            let done = match until(&steps[self.at]) {
                Until::Now => true,
                Until::Seconds(s) => !fresh && self.t >= s,
                Until::LineSaid => !fresh && status.line_said,
                Until::Arrived => !fresh && status.arrived,
                Until::Cleared => !fresh && status.cleared,
            };
            if !done {
                break;
            }
            self.at += 1;
            self.started = false;
        }
        begin
    }

    pub fn over(&self, steps: &[Step]) -> bool {
        self.at >= steps.len()
    }

    /// Skip the rest. Returns the steps whose effects must still happen
    /// (flags, clues, items, blocks, encounters), in order.
    pub fn skip(&mut self, steps: &[Step]) -> Vec<usize> {
        let from = if self.started { self.at + 1 } else { self.at };
        self.at = steps.len();
        (from..steps.len())
            .filter(|&i| lasts(&steps[i]))
            .collect()
    }
}

/// A mark's point: `facing` is the yaw-free forward of the player at the start.
pub fn resolve(m: &Mark, player: Vec3, forward: Vec3, actor: impl Fn(&str) -> Option<Vec3>, place: impl Fn(&str) -> Option<Vec3>) -> Option<Vec3> {
    let (a, b, c) = m.offset;
    match &m.from {
        Anchor::Player => {
            let fwd = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z);
            let right = Vec3::new(-fwd.z, 0.0, fwd.x);
            Some(player + right * a + Vec3::Y * b + fwd * c)
        }
        Anchor::Actor(n) => actor(n).map(|p| p + Vec3::new(a, b, c)),
        Anchor::Place(n) => place(n).map(|p| p + Vec3::new(a, b, c)),
        Anchor::Ship(n) => place(&ship_key(n)).map(|p| p + Vec3::new(a, b, c)),
    }
}

/// The way a yaw (degrees, positive to the right) turns from `forward` (flat).
pub fn turned(forward: Vec3, degrees: f32) -> Vec3 {
    let fwd = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z);
    let right = Vec3::new(-fwd.z, 0.0, fwd.x);
    let a = degrees.to_radians();
    fwd * a.cos() + right * a.sin()
}

/// Where an orbiting camera is `k` (0..1) of the way round: `from`/`turn` in degrees,
/// 0 = behind the centre as seen along `forward`, positive to the right.
pub fn orbit_point(center: Vec3, forward: Vec3, radius: f32, height: f32, from: f32, turn: f32, k: f32) -> Vec3 {
    let behind = -turned(forward, from + turn * k.clamp(0.0, 1.0));
    center + behind * radius + Vec3::Y * height
}

/// How far a fall has come at `k` (0..1) of its time: slow at first, fastest at the end.
pub fn fall(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps() -> Vec<Step> {
        vec![
            Step::Flag("a".into()),
            Step::Say("bram".into(), "C02_01".into()),
            Step::Wait(1.0),
            Step::Clue("b".into()),
            Step::Encounter("x".into()),
            Step::Flag("c".into()),
        ]
    }

    #[test]
    fn runs_steps_in_order_and_waits_for_each() {
        let s = steps();
        let mut r = Runner::default();
        // The flag is instant: the line starts in the same frame.
        assert_eq!(r.tick(&s, 0.1, Status::default()), vec![0, 1]);
        assert_eq!(r.tick(&s, 0.1, Status::default()), Vec::<usize>::new(), "waits for the line");
        assert_eq!(r.tick(&s, 0.1, Status { line_said: true, ..default() }), vec![2]);
        assert_eq!(r.tick(&s, 0.6, Status::default()), Vec::<usize>::new());
        // The wait ends; the clue is instant; the encounter waits.
        assert_eq!(r.tick(&s, 0.6, Status::default()), vec![3, 4]);
        // A "cleared" left over from before doesn't end the step that just began...
        assert_eq!(r.tick(&s, 0.1, Status::default()), Vec::<usize>::new());
        assert_eq!(r.tick(&s, 0.1, Status { cleared: true, ..default() }), vec![5]);
        assert!(r.over(&s));
    }

    fn default() -> Status {
        Status::default()
    }

    #[test]
    fn a_new_step_never_takes_the_last_ones_status() {
        let s = vec![Step::Say("a".into(), "1".into()), Step::Say("a".into(), "2".into())];
        let mut r = Runner::default();
        r.tick(&s, 0.1, Status::default());
        // The first line said: the second begins, and isn't said in the same breath.
        assert_eq!(r.tick(&s, 0.1, Status { line_said: true, ..default() }), vec![1]);
        assert!(!r.over(&s));
    }

    #[test]
    fn skipping_keeps_what_lasts() {
        let s = steps();
        let mut r = Runner::default();
        r.tick(&s, 0.1, Status::default());
        // Skipped during the line: the clue, the encounter and the last flag still happen.
        assert_eq!(r.skip(&s), vec![3, 4, 5]);
        assert!(r.over(&s));
    }

    #[test]
    fn triggers() {
        let flags: BTreeSet<String> = ["met_bram".to_string()].into();
        let cleared: BTreeSet<String> = ["raid_camp".to_string()].into();
        let made: BTreeSet<String> = ["airship".to_string()].into();
        let mut f = Facts { player: Vec3::ZERO, hour: 7.0, boss: 1, flags: Some(&flags), cleared: Some(&cleared), made: Some(&made), ..Default::default() };
        f.npcs.push(("gilm".into(), Vec3::new(5.0, 0.0, 0.0), false));
        f.places.insert("sporrow".into(), Vec3::new(20.0, 0.0, 0.0));
        let near = |sight| Trigger::Near { npc: "gilm".into(), within: 6.0, sight };
        assert!(fires(&near(false), &f));
        assert!(!fires(&near(true), &f), "can't see him");
        f.npcs[0].2 = true;
        assert!(fires(&near(true), &f));
        assert!(!fires(&Trigger::Area { place: "sporrow".into(), within: 10.0 }, &f));
        assert!(fires(&Trigger::Area { place: "sporrow".into(), within: 25.0 }, &f));
        assert!(fires(&Trigger::Cleared("raid_camp".into()), &f));
        assert!(fires(&Trigger::Boss(1), &f) && !fires(&Trigger::Boss(2), &f));
        assert!(fires(&Trigger::Made("airship".into()), &f) && !fires(&Trigger::Placed("planks".into()), &f));
        assert!(fires(&Trigger::TimeAfter { flag: "met_bram".into(), from: 6.0, to: 10.0 }, &f));
        assert!(!fires(&Trigger::TimeAfter { flag: "met_bram".into(), from: 20.0, to: 5.0 }, &f));
        f.hour = 23.0;
        assert!(fires(&Trigger::TimeAfter { flag: "met_bram".into(), from: 20.0, to: 5.0 }, &f), "past midnight");
        assert!(!fires(&Trigger::TimeAfter { flag: "other".into(), from: 20.0, to: 5.0 }, &f));
        // Once per world, and only with its flags.
        let c = Cutscene { id: "C05".into(), title: String::new(), trigger: Some(near(true)), needs: vec![], actors: BTreeMap::new(), steps: vec![] };
        assert!(ready(&c, &BTreeSet::new(), &f));
        assert!(!ready(&c, &["C05".to_string()].into(), &f));
        let c = Cutscene { needs: vec!["nope".into()], ..c };
        assert!(!ready(&c, &BTreeSet::new(), &f));
    }

    #[test]
    fn sight_lines() {
        let wall = |c: IVec3| c.x == 3;
        assert!(!clear_line(Vec3::new(0.5, 1.5, 0.5), Vec3::new(6.5, 1.5, 0.5), wall));
        assert!(clear_line(Vec3::new(0.5, 1.5, 0.5), Vec3::new(2.5, 1.5, 0.5), wall));
    }

    #[test]
    fn marks() {
        let p = Vec3::new(10.0, 5.0, 10.0);
        let m = Mark { from: Anchor::Player, offset: (1.0, 0.0, 3.0) };
        // Facing -z: ahead is -z, right is +x.
        assert_eq!(resolve(&m, p, Vec3::NEG_Z, |_| None, |_| None), Some(Vec3::new(11.0, 5.0, 7.0)));
        let m = Mark { from: Anchor::Place("camp".into()), offset: (0.0, 1.0, 0.0) };
        assert_eq!(resolve(&m, p, Vec3::NEG_Z, |_| None, |_| Some(Vec3::ONE)), Some(Vec3::new(1.0, 2.0, 1.0)));
        assert_eq!(resolve(&m, p, Vec3::NEG_Z, |_| None, |_| None), None);
    }

    #[test]
    fn orbits_go_round_and_falls_speed_up() {
        let c = Vec3::new(0.0, 50.0, 0.0);
        // Facing -z, angle 0 is behind (+z), 90 to the right... seen from the left (-x).
        let p0 = orbit_point(c, Vec3::NEG_Z, 10.0, 2.0, 0.0, 180.0, 0.0);
        assert!((p0 - Vec3::new(0.0, 52.0, 10.0)).length() < 1e-4);
        let half = orbit_point(c, Vec3::NEG_Z, 10.0, 2.0, 0.0, 180.0, 0.5);
        assert!((half - Vec3::new(-10.0, 52.0, 0.0)).length() < 1e-3);
        let end = orbit_point(c, Vec3::NEG_Z, 10.0, 2.0, 0.0, 180.0, 1.0);
        assert!((end - Vec3::new(0.0, 52.0, -10.0)).length() < 1e-3);
        // Every point keeps its distance.
        assert!(((half - c - Vec3::Y * 2.0).length() - 10.0).abs() < 1e-3);
        // A fall covers a quarter of the way in half the time, and all of it at the end.
        assert_eq!(fall(0.5), 0.25);
        assert_eq!(fall(1.0), 1.0);
        // Ships are found as places.
        let m = Mark { from: Anchor::Ship("ours".into()), offset: (0.0, 3.0, 0.0) };
        let place = |k: &str| (k == "ship:ours").then_some(Vec3::new(5.0, 60.0, 5.0));
        assert_eq!(resolve(&m, Vec3::ZERO, Vec3::NEG_Z, |_| None, place), Some(Vec3::new(5.0, 63.0, 5.0)));
    }

    /// The opening: the ships fly, ours is shot down, you fall, black, wake, Bram talks,
    /// and the guide starts.
    #[test]
    fn the_opening_has_its_parts() {
        let list = load(include_str!("../../../../assets/data/cutscenes.ron")).unwrap();
        let c = list.iter().find(|c| c.id == "C01").unwrap();
        let has = |f: &dyn Fn(&Step) -> bool| c.steps.iter().position(f);
        let ours = has(&|s| matches!(s, Step::Ship { look: ShipLook::Ours, .. })).expect("our ship");
        let court = has(&|s| matches!(s, Step::Ship { look: ShipLook::Court, .. })).expect("the court ship");
        let orbit = has(&|s| matches!(s, Step::Orbit { .. })).expect("the camera circles");
        let fire = has(&|s| matches!(s, Step::Fire(a, b) if a == "court" && b == "ours")).expect("shot down");
        let dive = has(&|s| matches!(s, Step::Dive(..))).expect("the fall");
        let black = dive + c.steps[dive..].iter().position(|s| matches!(s, Step::Fade(x, t) if *x >= 1.0 && *t <= 0.01)).expect("black");
        let lids = has(&|s| matches!(s, Step::Lids(x, _) if *x >= 1.0)).expect("eyes open");
        let bram = has(&|s| matches!(s, Step::Say(a, _) if a == "bram")).expect("Bram talks");
        let guide = has(&|s| matches!(s, Step::Flag(f) if f == "guide_on")).expect("the guide starts");
        assert!(ours < orbit && court < fire && orbit < fire && fire < dive && dive < black && black < lids && lids < bram && bram < guide);
        assert!(has(&|s| matches!(s, Step::Zoom(..))).is_some() && has(&|s| matches!(s, Step::Shake(..))).is_some());
        // The black comes as the fall ends, before the ground: only a wait between.
        let Step::Dive(_, _, secs) = &c.steps[dive] else { unreachable!() };
        let waited: f32 = c.steps[dive + 1..black].iter().map(|s| if let Step::Wait(t) = s { *t } else { 0.0 }).sum();
        assert!(waited < *secs && waited > secs - 0.3, "black just before the ground");
        assert!(c.steps[dive + 1..black].iter().all(|s| until(s) == Until::Now || matches!(s, Step::Wait(_))));
    }

    #[test]
    fn the_data_file_loads_and_names_real_lines() {
        let list = load(include_str!("../../../../assets/data/cutscenes.ron")).expect("cutscenes.ron");
        let lines = crate::lines::load();
        for c in &list {
            for s in &c.steps {
                if let Step::Say(_, id) = s {
                    assert!(lines.get(id).is_some(), "{}: no line {id}", c.id);
                }
            }
        }
        // C15 has two actors.
        let c15 = list.iter().find(|c| c.id == "C15").expect("C15");
        assert_eq!(c15.actors.len(), 2);
    }
}
