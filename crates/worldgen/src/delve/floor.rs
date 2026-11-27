//! Laying out one floor: a pure function of the plan (seed, dungeon, theme,
//! run, floor) and the room data.
//!
//! Rooms sit on a grid of cells `GRID` blocks apart, joined by corridors. Cell
//! 0 is the start room (the way back up is in its west wall). The boss arena is
//! the furthest dead end; its one corridor ends in the floor's locked door,
//! whose key drops in the room furthest from the start once its foes are down.
//! Every floor has one puzzle room (its prize behind a gate), a rest room, a
//! trap gauntlet, and maybe treasure, a coin shop and a secret room behind a
//! cracked wall. Some corridors are trapped. Fight rooms show a reward at their
//! doors (coin, a boon, a heal, or an elite for better loot), and on deeper
//! floors two of them may be sealed together as a section.
//!
//! After the boss: the stair down (not on the last floor), a way out back to
//! the entrance, and on a story dungeon's last floor a passage that comes out
//! near the next dungeon. They are drawn shut (`seals`) and the game opens them.

use std::collections::{HashMap, VecDeque};

use super::{Delves, Hazard, PuzzleKind, RoomType, POCKET};
use crate::dungeons::{Aabb, SpawnKind, SpawnPoint};
use crate::util::{hash2, unit};

/// Room cells, blocks apart.
const GRID: i32 = 28;
/// The widest half size a room can have (rooms.ron).
pub(crate) const CAP: i32 = GRID / 2 - 3;
/// Corridors: half width, height.
const HALL_HALF: i32 = 1;
const HALL_HEIGHT: i32 = 4;
/// Puzzle rooms stay this small so their vault fits behind a wall.
const PUZZLE_CAP: i32 = 8;
/// How deep the puzzle's vault and the boss room's way-out alcoves go.
const VAULT_DEPTH: i32 = 3;
const ALCOVE_DEPTH: i32 = 2;

/// What one floor is built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FloorPlan {
    pub seed: u64,
    /// The dungeon's id.
    pub dungeon: u64,
    pub theme: usize,
    pub run: u32,
    /// 1 = the first floor.
    pub floor: u8,
    pub floors: u8,
    /// A story dungeon: its last floor has a passage out near the next one.
    pub chain: bool,
    /// The dungeon is cleared: its last boss and prize chest don't come back.
    pub done: bool,
}

impl FloorPlan {
    pub fn last(&self) -> bool {
        self.floor >= self.floors
    }
}

/// What clearing a room gives, shown over its doors before you go in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reward {
    /// A handful of old coin.
    Coin,
    /// A boon for the rest of the run (boons.ron).
    Boon,
    /// Your wounds mend.
    Heal,
    /// An elite joins the fight, and drops better loot.
    Elite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChestKind {
    /// The last floor's prize, behind the boss.
    Prize,
    /// A treasure room's hoard.
    Treasure,
    /// Behind the puzzle's gate.
    Puzzle,
    /// In a secret room.
    Secret,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Room {
    pub kind: RoomType,
    /// Its template (index into `Delves::rooms`).
    pub template: usize,
    /// The open space inside the walls.
    pub inner: Aabb,
    /// The sealed section it belongs to, if any (index into `Floor::sections`).
    pub section: Option<usize>,
    pub reward: Option<Reward>,
    /// Waves of foes (Ambush rooms have more than one).
    pub waves: u8,
}

impl Room {
    pub fn middle(&self) -> [i32; 3] {
        [(self.inner.min[0] + self.inner.max[0]) / 2, self.inner.min[1], (self.inner.min[2] + self.inner.max[2]) / 2]
    }
}

/// A corridor between two rooms.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub a: usize,
    pub b: usize,
    pub corridor: Aabb,
    /// It ends in the floor's locked door (on `b`'s side).
    pub locked: bool,
    /// A cracked wall closes it halfway (a secret room or shortcut).
    pub secret: bool,
    /// Traps along it.
    pub trapped: bool,
}

/// Where a corridor opens into a room: one block thick in the room's wall.
/// Lock-in slabs drop here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Doorway {
    pub room: usize,
    pub link: usize,
    pub at: Aabb,
    /// Unit step from the room out into the corridor (x, z).
    pub out: (i32, i32),
}

/// A hazard placed on the floor (the game builds and runs it).
#[derive(Debug, Clone, PartialEq)]
pub struct HazardSpot {
    pub kind: Hazard,
    /// Spikes, barrels, fire vents, pits, oil: the cell (feet height); darts:
    /// the wall cell the darts come out of; blades: the cell under the pivot.
    pub pos: [i32; 3],
    /// Darts: the way they fly; blades: the way they swing.
    pub dir: (i32, i32),
    /// Darts: how far they fly; blades: the chain's length; fire: the flame's height.
    pub len: i32,
    /// The room it's in (`None` in a corridor).
    pub room: Option<usize>,
    /// 0..1: when in its cycle it starts, so a row of spikes runs in turn.
    pub phase: f32,
    /// Spikes that never go down (the bottom of a pit).
    pub fixed: bool,
    /// Oil and collapsing floor: every cell it covers.
    pub cells: Vec<[i32; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Puzzle {
    pub room: usize,
    pub kind: PuzzleKind,
    /// Plates: the plates (feet cells), and the stone blocks to push onto them.
    pub plates: Vec<[i32; 3]>,
    pub stones: Vec<[i32; 3]>,
    /// Levers (the cell they stand in); Timed has one.
    pub levers: Vec<[i32; 3]>,
    /// Levers: which lamps each lever flips (bit i = lamp i), and which burn at the start.
    pub flips: Vec<u8>,
    pub start: u8,
    /// The lamps over the gate.
    pub lamps: Vec<[i32; 3]>,
    /// The gate (drawn shut) and the vault behind it.
    pub gate: Aabb,
    pub vault: Aabb,
    /// Timed: seconds the gate stays open.
    pub open_for: f32,
}

impl Puzzle {
    /// Every lamp lit.
    pub fn all_lit(&self) -> u8 {
        ((1u16 << self.lamps.len()) - 1) as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChestSpot {
    pub pos: [i32; 3],
    pub kind: ChestKind,
    pub room: usize,
}

/// One floor of one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Floor {
    pub plan: FloorPlan,
    pub rooms: Vec<Room>,
    pub links: Vec<Link>,
    pub doors: Vec<Doorway>,
    pub spawns: Vec<SpawnPoint>,
    pub hazards: Vec<HazardSpot>,
    pub puzzle: Option<Puzzle>,
    pub chests: Vec<ChestSpot>,
    /// Rooms sealed together: walking into the first shuts the section's outer doors.
    pub sections: Vec<Vec<usize>>,
    /// The room whose fall drops the floor's key, and where (feet).
    pub key_room: usize,
    pub key_at: [i32; 3],
    /// The locked door (in the boss room's wall).
    pub locked: Aabb,
    /// Cracked walls you can break.
    pub cracked: Vec<[i32; 3]>,
    /// Every open box: rooms, corridors, the ways out, the stair, alcoves.
    pub spaces: Vec<Aabb>,
    /// Holes under the floor (pits, cracked floor).
    pub pits: Vec<Aabb>,
    /// Everything built, walls included.
    pub bounds: Aabb,
    /// Where you arrive (feet), in the start room.
    pub entry: [f32; 3],
    /// Step into this (in the start room's west wall) to go back up.
    pub way_out: Aabb,
    /// Step into this to go down a floor (none on the last floor).
    pub stair: Option<Aabb>,
    /// Step into this to leave with what you found (in the boss room).
    pub exit: Aabb,
    /// A story dungeon's last floor: out near the next dungeon.
    pub passage: Option<Aabb>,
    /// Drawn shut until the boss falls (the stair's mouth, the exit, the passage).
    pub seals: Vec<Aabb>,
    pub boss_room: usize,
    /// Feet height of every room.
    pub floor_y: i32,
    pub campfire: Option<[i32; 3]>,
    /// Where the coin merchant stands.
    pub shop: Option<[i32; 3]>,
    pub(crate) torches: Vec<[i32; 3]>,
    pub(crate) pillars: Vec<Aabb>,
    /// Pit steps (a block to climb out on).
    pub(crate) steps: Vec<[i32; 3]>,
}

impl Floor {
    /// The room a point (feet) stands in.
    pub fn room_at(&self, p: [f32; 3]) -> Option<usize> {
        let b = p.map(|v| v.floor() as i32);
        self.rooms.iter().position(|r| r.inner.contains(b[0], b[1], b[2]) || r.inner.contains(b[0], b[1] + 1, b[2]))
    }

    /// A point (feet) is somewhere inside the floor's open space.
    pub fn inside(&self, p: [f32; 3]) -> bool {
        let b = p.map(|v| v.floor() as i32);
        self.spaces.iter().chain(&self.pits).any(|s| s.contains(b[0], b[1], b[2]))
    }

    pub fn in_box(b: &Aabb, p: [f32; 3]) -> bool {
        let q = p.map(|v| v.floor() as i32);
        b.contains(q[0], q[1], q[2])
    }

    /// The doorways of a room.
    pub fn doors_of(&self, room: usize) -> impl Iterator<Item = &Doorway> {
        self.doors.iter().filter(move |d| d.room == room)
    }

    /// The rooms locked together with this one: its section, or just itself.
    pub fn sealed_with(&self, room: usize) -> Vec<usize> {
        match self.rooms[room].section {
            Some(s) => self.sections[s].clone(),
            None => vec![room],
        }
    }

    /// The doorways that shut when `room` locks: every doorway of its section
    /// that leads out of the section (not between two of its rooms).
    pub fn lock_doors(&self, room: usize) -> Vec<Doorway> {
        let group = self.sealed_with(room);
        self.doors
            .iter()
            .filter(|d| group.contains(&d.room))
            .filter(|d| {
                let l = &self.links[d.link];
                let other = if l.a == d.room { l.b } else { l.a };
                !group.contains(&other)
            })
            .copied()
            .collect()
    }

    /// An open cell (feet) in a room near its middle but not on it: no pillar,
    /// pit, hazard or chest there.
    pub fn room_free_spot(&self, room: usize) -> Option<[i32; 3]> {
        let r = &self.rooms.get(room)?.inner;
        let y = r.min[1];
        let mid = [(r.min[0] + r.max[0]) / 2 + 2, y, (r.min[2] + r.max[2]) / 2];
        let busy = |x: i32, z: i32| {
            self.pillars.iter().any(|p| p.contains(x, y, z))
                || self.pits.iter().any(|p| p.contains(x, y - 1, z))
                || self.hazards.iter().any(|h| h.pos == [x, y, z] || h.cells.contains(&[x, y, z]))
                || self.chests.iter().any(|c| c.pos == [x, y, z])
        };
        (r.min[2] + 1..r.max[2])
            .flat_map(|z| (r.min[0] + 1..r.max[0]).map(move |x| (x, z)))
            .filter(|&(x, z)| !busy(x, z))
            .min_by_key(|&(x, z)| (x - mid[0]).abs() + (z - mid[2]).abs())
            .map(|(x, z)| [x, y, z])
    }

    /// The links leading into a room (for showing its reward at the far end).
    pub fn ways_into(&self, room: usize) -> impl Iterator<Item = &Doorway> {
        self.doors.iter().filter(move |d| {
            let l = &self.links[d.link];
            d.room != room && (l.a == room || l.b == room) && !l.secret
        })
    }
}

/// A small deterministic dice.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn int(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }

    fn unit(&mut self) -> f32 {
        unit(self.next())
    }

    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }
}

pub(crate) fn b_box(x: (i32, i32), y: (i32, i32), z: (i32, i32)) -> Aabb {
    Aabb { min: [x.0.min(x.1), y.0.min(y.1), z.0.min(z.1)], max: [x.0.max(x.1), y.0.max(y.1), z.0.max(z.1)] }
}

const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// A box in a room's wall on one side, `depth` blocks out, `half` blocks
/// either side of the middle and `high` blocks tall.
fn wall_box(inner: &Aabb, side: (i32, i32), from: i32, depth: i32, half: i32, high: i32) -> Aabb {
    let (cx, cz) = ((inner.min[0] + inner.max[0]) / 2, (inner.min[2] + inner.max[2]) / 2);
    let y = (inner.min[1], inner.min[1] + high - 1);
    match side {
        (1, 0) => b_box((inner.max[0] + from, inner.max[0] + from + depth - 1), y, (cz - half, cz + half)),
        (-1, 0) => b_box((inner.min[0] - from, inner.min[0] - from - depth + 1), y, (cz - half, cz + half)),
        (0, 1) => b_box((cx - half, cx + half), y, (inner.max[2] + from, inner.max[2] + from + depth - 1)),
        _ => b_box((cx - half, cx + half), y, (inner.min[2] - from, inner.min[2] - from - depth + 1)),
    }
}

/// Lay out one floor.
#[allow(clippy::needless_range_loop)]
pub fn floor(book: &Delves, plan: &FloorPlan) -> Floor {
    let d = plan.floor.max(1) as i32;
    let theme = plan.theme.min(book.themes.len() - 1);
    let mut dice = Dice(hash2(plan.seed ^ 0x2077_1E6A ^ plan.dungeon.rotate_left(17), plan.run as i32, d));
    let [px, fy, pz] = POCKET;
    // Cells: i runs east (never west of the start, so its west wall stays free
    // for the way out), j north/south.
    let reach = 2 + d / 2;
    let want = (5 + 2 * d).min(13) as usize;
    let mut cells = vec![(0, 0)];
    let mut index: HashMap<(i32, i32), usize> = HashMap::from([((0, 0), 0)]);
    let mut tree = Vec::new();
    for _ in 0..600 {
        if cells.len() >= want {
            break;
        }
        let from = dice.int(0, cells.len() as i32 - 1) as usize;
        let (di, dj) = DIRS[dice.int(0, 3) as usize];
        let (i, j) = (cells[from].0 + di, cells[from].1 + dj);
        if !(0..=2 * reach).contains(&i) || !(-reach..=reach).contains(&j) || (i == 0 && j != 0) || index.contains_key(&(i, j)) {
            continue;
        }
        index.insert((i, j), cells.len());
        tree.push((from, cells.len()));
        cells.push((i, j));
    }
    let dist = walk(cells.len(), &tree);
    let free_cell = |n: usize, cells: &[(i32, i32)], index: &HashMap<(i32, i32), usize>| {
        let (i, j) = cells[n];
        DIRS.iter().copied().filter(|&(di, dj)| !index.contains_key(&(i + di, j + dj)) && (0..=2 * reach + 1).contains(&(i + di))).collect::<Vec<_>>()
    };
    let degree = |n: usize| tree.iter().filter(|&&(a, b)| a == n || b == n).count();
    // The boss: the furthest dead end (with a free cell beside it for the stair).
    let boss = (1..cells.len())
        .filter(|&n| degree(n) == 1 && (plan.last() || !free_cell(n, &cells, &index).is_empty()))
        .max_by_key(|&n| (dist[n], n))
        .unwrap_or(cells.len() - 1);
    let stair_side = (!plan.last()).then(|| free_cell(boss, &cells, &index).first().copied().unwrap_or((1, 0)));
    let stair_cell = stair_side.map(|(di, dj)| (cells[boss].0 + di, cells[boss].1 + dj));
    // A few loops (never into the boss room: its only way in is the locked door).
    let mut pairs = tree.clone();
    for a in 0..cells.len() {
        let (i, j) = cells[a];
        for (di, dj) in [(1, 0), (0, 1)] {
            if let Some(&b) = index.get(&(i + di, j + dj))
                && a != boss
                && b != boss
                && !pairs.contains(&(a, b))
                && !pairs.contains(&(b, a))
                && dice.chance(0.2)
            {
                pairs.push((a, b));
            }
        }
    }
    // Which rooms get which part. The key waits in the furthest room from the start.
    let n = cells.len();
    let mut kinds = vec![RoomType::Fight; n];
    kinds[0] = RoomType::Start;
    kinds[boss] = RoomType::Boss;
    let sides_used = |r: usize, pairs: &[(usize, usize)], cells: &[(i32, i32)]| {
        pairs
            .iter()
            .filter_map(|&(a, b)| {
                let o = if a == r { b } else if b == r { a } else { return None };
                Some((cells[o].0 - cells[r].0, cells[o].1 - cells[r].1))
            })
            .collect::<Vec<_>>()
    };
    let mut others: Vec<usize> = (1..n).filter(|&r| r != boss).collect();
    others.sort_by_key(|&r| (std::cmp::Reverse(dist[r]), r));
    let key_room = others.first().copied().unwrap_or(0);
    let mut rest: Vec<usize> = others.iter().copied().filter(|&r| r != key_room).collect();
    // Shuffle the rest, then hand out the parts in turn.
    for i in (1..rest.len()).rev() {
        rest.swap(i, dice.int(0, i as i32) as usize);
    }
    if key_room != 0 {
        kinds[key_room] = if dice.chance(0.5) { RoomType::Ambush } else { RoomType::Fight };
    }
    // The puzzle needs a wall with nothing behind it for its vault.
    if let Some(p) = rest.iter().position(|&r| sides_used(r, &pairs, &cells).len() < 4) {
        let r = rest.remove(p);
        kinds[r] = RoomType::Puzzle;
    }
    let mut hand = vec![RoomType::Rest, RoomType::Trap];
    if dice.chance(0.7) {
        hand.push(RoomType::Treasure);
    }
    if dice.chance(0.4) {
        hand.push(RoomType::Shop);
    }
    for (k, r) in hand.into_iter().zip(rest.clone()) {
        kinds[r] = k;
        rest.retain(|&x| x != r);
    }
    for &r in &rest {
        if dice.chance(0.25) {
            kinds[r] = RoomType::Ambush;
        }
    }
    // A secret room behind a cracked wall, off a fight or trap room.
    let mut secret_pair = None;
    if dice.chance(0.6) {
        let hosts: Vec<usize> = (1..n).filter(|&r| matches!(kinds[r], RoomType::Fight | RoomType::Trap | RoomType::Ambush)).collect();
        'find: for &h in &hosts {
            for side in free_cell(h, &cells, &index) {
                let c = (cells[h].0 + side.0, cells[h].1 + side.1);
                if Some(c) == stair_cell || c.0 < 0 || c.1.abs() > reach + 1 || (c.0 == 0 && c.1 != 0) {
                    continue;
                }
                index.insert(c, cells.len());
                secret_pair = Some((h, cells.len()));
                cells.push(c);
                kinds.push(RoomType::Secret);
                break 'find;
            }
        }
    }
    if let Some(p) = secret_pair {
        pairs.push(p);
    }
    let n = cells.len();
    let centre = |r: usize| (px + cells[r].0 * GRID, pz + cells[r].1 * GRID);
    // The rooms themselves, sized by their template.
    let mut rooms = Vec::new();
    for r in 0..n {
        let kind = kinds[r];
        let template = book.pick_room(kind, theme, plan.floor, dice.unit());
        let t = &book.rooms[template];
        let cap = if kind == RoomType::Puzzle { PUZZLE_CAP } else { CAP };
        let hx = dice.int(t.half.0 .0, t.half.0 .1).min(cap);
        let hz = dice.int(t.half.1 .0, t.half.1 .1).min(cap);
        let h = dice.int(t.height.0, t.height.1);
        let (cx, cz) = centre(r);
        let waves = if t.foes.1 == 0 { 0 } else { t.waves.max(1) };
        rooms.push(Room { kind, template, inner: b_box((cx - hx, cx + hx), (fy, fy + h - 1), (cz - hz, cz + hz)), section: None, reward: None, waves });
    }
    // Corridors and their doorways.
    let mut links = Vec::new();
    let mut doors = Vec::new();
    let mut spaces: Vec<Aabb> = rooms.iter().map(|r| r.inner).collect();
    for &(a, b) in &pairs {
        let ((ax, az), (bx, bz)) = (centre(a), centre(b));
        let corridor = if az == bz {
            b_box((ax, bx), (fy, fy + HALL_HEIGHT - 1), (az - HALL_HALF, az + HALL_HALF))
        } else {
            b_box((ax - HALL_HALF, ax + HALL_HALF), (fy, fy + HALL_HEIGHT - 1), (az, bz))
        };
        let li = links.len();
        let secret = Some((a, b)) == secret_pair;
        links.push(Link { a, b, corridor, locked: false, secret, trapped: false });
        spaces.push(corridor);
        for (r, o) in [(a, b), (b, a)] {
            let side = ((cells[o].0 - cells[r].0).signum(), (cells[o].1 - cells[r].1).signum());
            let at = wall_box(&rooms[r].inner, side, 1, 1, HALL_HALF, HALL_HEIGHT);
            doors.push(Doorway { room: r, link: li, at, out: side });
        }
    }
    // The locked door: where the boss room's corridor meets its wall.
    let boss_link = links.iter().position(|l| l.a == boss || l.b == boss).unwrap_or(0);
    links[boss_link].locked = true;
    let locked = doors.iter().find(|d| d.link == boss_link && d.room == boss).map_or(rooms[boss].inner, |d| d.at);
    // Cracked walls across the secret corridor, halfway.
    let mut cracked = Vec::new();
    if let Some(l) = links.iter().find(|l| l.secret) {
        let c = &l.corridor;
        let along_x = c.max[0] - c.min[0] > c.max[2] - c.min[2];
        let (ra, rb) = (&rooms[l.a].inner, &rooms[l.b].inner);
        if along_x {
            let lo = ra.max[0].min(rb.max[0]) + 1;
            let hi = ra.min[0].max(rb.min[0]) - 1;
            let x = (lo + hi) / 2;
            for y in c.min[1]..=c.max[1] {
                for z in c.min[2]..=c.max[2] {
                    cracked.push([x, y, z]);
                }
            }
        } else {
            let lo = ra.max[2].min(rb.max[2]) + 1;
            let hi = ra.min[2].max(rb.min[2]) - 1;
            let z = (lo + hi) / 2;
            for y in c.min[1]..=c.max[1] {
                for x in c.min[0]..=c.max[0] {
                    cracked.push([x, y, z]);
                }
            }
        }
    }
    // The way back up: a doorway two deep in the start room's west wall.
    let (sx, sz) = centre(0);
    let way_out = b_box((rooms[0].inner.min[0] - 2, rooms[0].inner.min[0] - 1), (fy, fy + 2), (sz - 1, sz + 1));
    spaces.push(way_out);
    // The boss room's ways on: the stair down, the way out, the passage.
    let used = sides_used(boss, &pairs, &cells);
    let mut free_walls: Vec<(i32, i32)> = DIRS.iter().copied().filter(|s| !used.contains(s) && Some(*s) != stair_side).collect();
    let binner = rooms[boss].inner;
    let mut seals = Vec::new();
    let stair = stair_side.map(|(ddx, ddz)| {
        let (bx, bz) = centre(boss);
        let (hx, hz) = ((binner.max[0] - binner.min[0]) / 2, (binner.max[2] - binner.min[2]) / 2);
        let mut steps = Vec::new();
        for k in 1..=6 {
            let y = fy - (k - 1) / 2;
            let b = if ddx != 0 {
                let x = bx + ddx * (hx + k);
                b_box((x, x), (y, y + 3), (bz - 1, bz + 1))
            } else {
                let z = bz + ddz * (hz + k);
                b_box((bx - 1, bx + 1), (y, y + 3), (z, z))
            };
            steps.push(b);
        }
        seals.push(steps[0]);
        let s = steps[steps.len() - 1].union(&steps[steps.len() - 2]);
        spaces.extend(steps);
        s
    });
    let exit_side = free_walls.first().copied().unwrap_or((0, 1));
    free_walls.retain(|s| *s != exit_side);
    let exit = wall_box(&binner, exit_side, 1, ALCOVE_DEPTH, 1, 3);
    spaces.push(exit);
    seals.push(wall_box(&binner, exit_side, 1, 1, 1, 3));
    let passage = (plan.chain && plan.last()).then(|| {
        let side = free_walls.first().copied().unwrap_or((-exit_side.0, -exit_side.1));
        let p = wall_box(&binner, side, 1, ALCOVE_DEPTH, 1, 3);
        spaces.push(p);
        seals.push(wall_box(&binner, side, 1, 1, 1, 3));
        p
    });
    // Sections: on deeper floors, two linked fight rooms sealed together.
    let mut sections = Vec::new();
    if d >= 2 {
        for l in &links {
            let (a, b) = (l.a, l.b);
            if sections.len() < 1 + (d as usize - 2) / 2
                && kinds[a] == RoomType::Fight
                && kinds[b] == RoomType::Fight
                && a != key_room
                && b != key_room
                && rooms[a].section.is_none()
                && rooms[b].section.is_none()
                && !l.secret
                && dice.chance(0.6)
            {
                rooms[a].section = Some(sections.len());
                rooms[b].section = Some(sections.len());
                sections.push(vec![a, b]);
            }
        }
    }
    // Rewards at the doors of fight rooms.
    for r in rooms.iter_mut() {
        if matches!(r.kind, RoomType::Fight | RoomType::Ambush) {
            let u = dice.unit();
            r.reward = Some(if u < 0.4 {
                Reward::Coin
            } else if u < 0.7 {
                Reward::Boon
            } else if u < 0.85 {
                Reward::Heal
            } else {
                Reward::Elite
            });
        }
    }
    // Free floor cells per room, for hazards, foes and things.
    let mut taken: Vec<[i32; 3]> = Vec::new();
    let mut hazards = Vec::new();
    let mut pits = Vec::new();
    let mut steps = Vec::new();
    let mut pillars = Vec::new();
    let near_door = |p: [i32; 3], room: usize, doors: &[Doorway]| {
        doors.iter().filter(|dw| dw.room == room).any(|dw| {
            let c = [(dw.at.min[0] + dw.at.max[0]) / 2, (dw.at.min[2] + dw.at.max[2]) / 2];
            (p[0] - c[0]).abs() <= 3 && (p[2] - c[1]).abs() <= 3
        }) || (room == 0 && p[0] <= rooms[0].inner.min[0] + 2 && (p[2] - sz).abs() <= 2)
    };
    let mut chests = Vec::new();
    let mut spawns = Vec::new();
    let mut campfire = None;
    let mut shop = None;
    let mut puzzle = None;
    for r in 0..n {
        let inner = rooms[r].inner;
        let t = &book.rooms[rooms[r].template];
        let mid = rooms[r].middle();
        // Pillars in a grid, clear of the middle and the doors.
        if t.pillars {
            for x in (inner.min[0] + 3..=inner.max[0] - 3).step_by(4) {
                for z in (inner.min[2] + 3..=inner.max[2] - 3).step_by(4) {
                    let p = [x, fy, z];
                    if (x - mid[0]).abs() <= 1 && (z - mid[2]).abs() <= 1 || near_door(p, r, &doors) {
                        continue;
                    }
                    pillars.push(b_box((x, x), (fy, inner.max[1]), (z, z)));
                    taken.push(p);
                }
            }
        }
        // A sunken middle with spikes at the bottom and a step out.
        if t.pit {
            let h = 1 + i32::from((inner.max[0] - inner.min[0]) >= 18);
            let pit = b_box((mid[0] - h, mid[0] + h), (fy - 2, fy - 1), (mid[2] - h + 1, mid[2] + h + 1));
            pits.push(pit);
            steps.push([pit.min[0], fy - 2, pit.min[2]]);
            for x in pit.min[0]..=pit.max[0] {
                for z in pit.min[2]..=pit.max[2] {
                    taken.push([x, fy, z]);
                    if [x, fy - 2, z] != [pit.min[0], fy - 2, pit.min[2]] {
                        hazards.push(HazardSpot { kind: Hazard::Spikes, pos: [x, fy - 2, z], dir: (0, 0), len: 1, room: Some(r), phase: 0.0, fixed: true, cells: vec![] });
                    }
                }
            }
        }
        let free_cells = |taken: &[[i32; 3]], margin: i32| {
            let mut v = Vec::new();
            for x in inner.min[0] + margin..=inner.max[0] - margin {
                for z in inner.min[2] + margin..=inner.max[2] - margin {
                    let p = [x, fy, z];
                    if !taken.contains(&p) && !near_door(p, r, &doors) {
                        v.push(p);
                    }
                }
            }
            v
        };
        match rooms[r].kind {
            RoomType::Rest => {
                campfire = Some(mid);
                taken.push(mid);
            }
            RoomType::Shop => {
                shop = Some(mid);
                taken.push(mid);
            }
            RoomType::Treasure | RoomType::Secret => {
                let kind = if rooms[r].kind == RoomType::Treasure { ChestKind::Treasure } else { ChestKind::Secret };
                chests.push(ChestSpot { pos: mid, kind, room: r });
                taken.push(mid);
                // The hoard's trap rings the chest.
                for p in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
                    taken.push([mid[0] + p[0], fy, mid[2] + p[1]]);
                }
            }
            RoomType::Puzzle => {
                let used = sides_used(r, &pairs, &cells);
                let side = DIRS.iter().copied().find(|s| !used.contains(s)).unwrap_or((0, -1));
                let gate = wall_box(&inner, side, 1, 1, 1, 3);
                let vault = wall_box(&inner, side, 2, VAULT_DEPTH - 1, 1, 3);
                spaces.push(gate);
                spaces.push(vault);
                seals.push(gate);
                let cv = [(vault.min[0] + vault.max[0]) / 2, fy, (vault.min[2] + vault.max[2]) / 2];
                chests.push(ChestSpot { pos: cv, kind: ChestKind::Puzzle, room: r });
                let kinds_ok = &book.themes[theme].def.puzzles;
                let kind = if kinds_ok.is_empty() { PuzzleKind::Plates } else { kinds_ok[dice.int(0, kinds_ok.len() as i32 - 1) as usize] };
                // Lamps over the gate, along the wall.
                let along = (-side.1, side.0);
                let gc = [(gate.min[0] + gate.max[0]) / 2, (gate.min[2] + gate.max[2]) / 2];
                let lamp_y = fy + 3;
                let mut p = Puzzle { room: r, kind, plates: vec![], stones: vec![], levers: vec![], flips: vec![], start: 0, lamps: vec![], gate, vault, open_for: 0.0 };
                // Keep the strip in front of the gate clear.
                for k in 1..=2 {
                    for w in -1..=1 {
                        taken.push([gc[0] - side.0 * k + along.0 * w, fy, gc[1] - side.1 * k + along.1 * w]);
                    }
                }
                match kind {
                    PuzzleKind::Plates => {
                        let count = 2 + i32::from(d >= 3) as usize;
                        let mut spots = free_cells(&taken, 2);
                        for _ in 0..count {
                            if spots.is_empty() {
                                break;
                            }
                            let s = spots.swap_remove(dice.int(0, spots.len() as i32 - 1) as usize);
                            p.plates.push(s);
                            taken.push(s);
                            spots.retain(|q| (q[0] - s[0]).abs() + (q[2] - s[2]).abs() > 2);
                        }
                        // One stone fewer than plates: you stand on the last.
                        let mut spots = free_cells(&taken, 2);
                        for _ in 1..p.plates.len() {
                            if spots.is_empty() {
                                break;
                            }
                            let s = spots.swap_remove(dice.int(0, spots.len() as i32 - 1) as usize);
                            p.stones.push(s);
                            taken.push(s);
                            spots.retain(|q| (q[0] - s[0]).abs() + (q[2] - s[2]).abs() > 1);
                        }
                    }
                    PuzzleKind::Levers => {
                        let count = 3 + i32::from(d >= 2) as usize;
                        // Levers along the far wall, lamps over the gate.
                        let far = [mid[0] - side.0 * ((inner.max[0] - inner.min[0]) / 2 - 1), fy, mid[2] - side.1 * ((inner.max[2] - inner.min[2]) / 2 - 1)];
                        for k in 0..count as i32 {
                            let off = (k - count as i32 / 2) * 2;
                            let l = [far[0] + along.0 * off, fy, far[2] + along.1 * off];
                            p.levers.push(l);
                            taken.push(l);
                            p.lamps.push([gc[0] + along.0 * (k - count as i32 / 2), lamp_y, gc[1] + along.1 * (k - count as i32 / 2)]);
                        }
                        // Each lever flips its own lamp and one or two others.
                        for k in 0..count {
                            let mut m = 1u8 << k;
                            m |= 1 << ((k + 1) % count);
                            if dice.chance(0.4) {
                                m |= 1 << ((k + count - 1) % count);
                            }
                            p.flips.push(m);
                        }
                        // Start from all lit and pull a few at random: always solvable, never solved.
                        let all = p.all_lit();
                        let mut s = all;
                        for _ in 0..8 {
                            if s != all && dice.chance(0.3) {
                                break;
                            }
                            s ^= p.flips[dice.int(0, count as i32 - 1) as usize];
                        }
                        if s == all {
                            s ^= p.flips[0];
                        }
                        p.start = s;
                    }
                    PuzzleKind::Timed => {
                        let far = [mid[0] - side.0 * ((inner.max[0] - inner.min[0]) / 2 - 1), fy, mid[2] - side.1 * ((inner.max[2] - inner.min[2]) / 2 - 1)];
                        p.levers.push(far);
                        taken.push(far);
                        let run = ((far[0] - gc[0]).abs() + (far[2] - gc[1]).abs()) as f32;
                        p.open_for = run / 5.0 + 1.6;
                        // Spikes across the run make it harder.
                        let span = (far[0] - gc[0]).abs().max((far[2] - gc[1]).abs());
                        for k in (3..span - 1).step_by(3) {
                            let c = [far[0] + side.0 * k, fy, far[2] + side.1 * k];
                            for w in -1..=1 {
                                let s = [c[0] + along.0 * w, fy, c[2] + along.1 * w];
                                if !taken.contains(&s) {
                                    hazards.push(HazardSpot { kind: Hazard::Spikes, pos: s, dir: (0, 0), len: 1, room: Some(r), phase: k as f32 * 0.17, fixed: false, cells: vec![] });
                                    taken.push(s);
                                }
                            }
                        }
                    }
                }
                puzzle = Some(p);
            }
            _ => {}
        }
        // Hazards from the template.
        for &(h, lo, hi) in &t.hazards {
            let count = dice.int(lo as i32, hi.max(lo) as i32);
            for c in 0..count {
                let h = if h == Hazard::Any { book.pick_hazard(theme, dice.unit()) } else { h };
                if let Some(spot) = place_hazard(h, r, &rooms[r], &mut dice, &free_cells(&taken, 1), c as f32 * 0.21, &mut pits, &mut steps, &rooms[r].kind)
                    && !doors.iter().any(|dw| dw.at.grow(1).contains(spot.pos[0], spot.pos[1], spot.pos[2]))
                {
                    taken.extend(spot.cells.iter().copied());
                    taken.push(spot.pos);
                    hazards.push(spot);
                }
            }
        }
        // Foes, wave by wave.
        let boost = (d - 1) / 2;
        let waves = rooms[r].waves;
        if rooms[r].kind == RoomType::Boss {
            let boss_here = !(plan.last() && plan.done);
            if boss_here {
                // In the middle, or as near it as the arena allows (a pit may be there).
                let pos = free_cells(&taken, 2).into_iter().min_by_key(|q| (q[0] - mid[0]).abs() + (q[2] - mid[2]).abs()).unwrap_or(mid);
                spawns.push(SpawnPoint { pos, kind: SpawnKind::Boss, room: r, wave: 0, elite: false });
                taken.push(pos);
            }
            let guards = if boss_here { d.min(3) } else { 2 + d };
            let mut spots = free_cells(&taken, 2);
            for _ in 0..guards {
                if spots.is_empty() {
                    break;
                }
                let s = spots.swap_remove(dice.int(0, spots.len() as i32 - 1) as usize);
                spawns.push(SpawnPoint { pos: s, kind: SpawnKind::Guard, room: r, wave: 0, elite: false });
                taken.push(s);
            }
            if plan.last() && !plan.done {
                let pos = [mid[0] + exit_side.0 * ((binner.max[0] - binner.min[0]) / 2 - 2), fy, mid[2] + exit_side.1 * ((binner.max[2] - binner.min[2]) / 2 - 2)];
                chests.push(ChestSpot { pos, kind: ChestKind::Prize, room: r });
            }
        } else if waves > 0 {
            let elite_room = rooms[r].reward == Some(Reward::Elite) || dice.chance(t.elite);
            for w in 0..waves {
                let count = dice.int(t.foes.0 as i32, t.foes.1 as i32) + boost;
                let mut spots = free_cells(&taken, 1);
                for k in 0..count {
                    if spots.is_empty() {
                        break;
                    }
                    let s = spots.swap_remove(dice.int(0, spots.len() as i32 - 1) as usize);
                    let kind = if k == 0 && count >= 3 { SpawnKind::Guard } else { SpawnKind::Pack };
                    spawns.push(SpawnPoint { pos: s, kind, room: r, wave: w, elite: elite_room && w + 1 == waves && k == 0 });
                    spots.retain(|q| q != &s);
                }
            }
        }
    }
    // Trapped corridors: one or two, never the locked one or the secret one.
    let mut trapped = 0;
    for li in 0..links.len() {
        let l = &links[li];
        if trapped > (d as usize) / 3 || l.locked || l.secret || l.a == 0 || l.b == 0 || !dice.chance(0.35) {
            continue;
        }
        let c = l.corridor;
        let along_x = c.max[0] - c.min[0] > c.max[2] - c.min[2];
        let (ra, rb) = (&rooms[l.a].inner, &rooms[l.b].inner);
        let (lo, hi) = if along_x { (ra.max[0].min(rb.max[0]) + 2, ra.min[0].max(rb.min[0]) - 2) } else { (ra.max[2].min(rb.max[2]) + 2, ra.min[2].max(rb.min[2]) - 2) };
        if hi - lo < 2 {
            continue;
        }
        let mid_c = if along_x { (c.min[2] + c.max[2]) / 2 } else { (c.min[0] + c.max[0]) / 2 };
        let cell = |k: i32, w: i32| if along_x { [k, fy, mid_c + w] } else { [mid_c + w, fy, k] };
        let pick = book.pick_hazard(theme, dice.unit());
        let kind = match pick {
            Hazard::Blade | Hazard::Darts | Hazard::FireJet => pick,
            _ => Hazard::Spikes,
        };
        let dir = if along_x { (1, 0) } else { (0, 1) };
        match kind {
            Hazard::Blade => {
                let k = (lo + hi) / 2;
                hazards.push(HazardSpot { kind, pos: cell(k, 0), dir, len: HALL_HEIGHT - 1, room: None, phase: dice.unit(), fixed: false, cells: vec![] });
            }
            Hazard::Darts => {
                let k = (lo + hi) / 2;
                let wall = if along_x { [k, fy + 1, c.min[2] - 1] } else { [c.min[0] - 1, fy + 1, k] };
                hazards.push(HazardSpot { kind, pos: wall, dir: if along_x { (0, 1) } else { (1, 0) }, len: 2 * HALL_HALF + 1, room: None, phase: dice.unit(), fixed: false, cells: vec![] });
            }
            Hazard::FireJet => {
                for (i, k) in (lo..=hi).step_by(3).enumerate() {
                    hazards.push(HazardSpot { kind, pos: cell(k, 0), dir: (0, 0), len: 3, room: None, phase: i as f32 * 0.25, fixed: false, cells: vec![] });
                }
            }
            _ => {
                for (i, k) in (lo..=hi).enumerate() {
                    if i % 3 == 2 {
                        continue;
                    }
                    for w in -HALL_HALF..=HALL_HALF {
                        hazards.push(HazardSpot { kind: Hazard::Spikes, pos: cell(k, w), dir: (0, 0), len: 1, room: None, phase: i as f32 * 0.15, fixed: false, cells: vec![] });
                    }
                }
            }
        }
        links[li].trapped = true;
        trapped += 1;
    }
    // Torches in the corners of every room.
    let mut torches = Vec::new();
    for r in &rooms {
        let (lo, hi) = (r.inner.min, r.inner.max);
        for (x, z) in [(lo[0], lo[2]), (hi[0], lo[2]), (lo[0], hi[2]), (hi[0], hi[2])] {
            torches.push([x, fy, z]);
        }
    }
    let key_at = {
        let m = rooms[key_room].middle();
        if taken.contains(&m) { [m[0] + 1, fy, m[2]] } else { m }
    };
    let bounds = spaces.iter().chain(&pits).fold(spaces[0], |acc, s| acc.union(&s.grow(1)));
    Floor {
        plan: *plan,
        rooms,
        links,
        doors,
        spawns,
        hazards,
        puzzle,
        chests,
        sections,
        key_room,
        key_at,
        locked,
        cracked,
        spaces,
        pits,
        bounds,
        entry: [sx as f32 + 0.5, fy as f32, sz as f32 + 0.5],
        way_out,
        stair,
        exit,
        passage,
        seals,
        boss_room: boss,
        floor_y: fy,
        campfire,
        shop,
        torches,
        pillars,
        steps,
    }
}

/// Walk distances from cell 0 over the links.
fn walk(n: usize, links: &[(usize, usize)]) -> Vec<usize> {
    let mut dist = vec![usize::MAX; n];
    dist[0] = 0;
    let mut queue = VecDeque::from([0]);
    while let Some(a) = queue.pop_front() {
        for &(p, q) in links {
            let b = if p == a { q } else if q == a { p } else { continue };
            if dist[b] == usize::MAX {
                dist[b] = dist[a] + 1;
                queue.push_back(b);
            }
        }
    }
    dist
}

/// Put one hazard in a room, on its free cells.
#[allow(clippy::too_many_arguments)]
fn place_hazard(
    kind: Hazard,
    r: usize,
    room: &Room,
    dice: &mut Dice,
    free: &[[i32; 3]],
    phase: f32,
    pits: &mut Vec<Aabb>,
    steps: &mut Vec<[i32; 3]>,
    room_kind: &RoomType,
) -> Option<HazardSpot> {
    if free.is_empty() {
        return None;
    }
    let inner = room.inner;
    let fy = inner.min[1];
    let pick = |dice: &mut Dice| free[dice.int(0, free.len() as i32 - 1) as usize];
    let spot = |pos, dir, len, cells| HazardSpot { kind, pos, dir, len, room: Some(r), phase, fixed: false, cells };
    let has = |p: [i32; 3]| free.contains(&p);
    Some(match kind {
        Hazard::Spikes => {
            // A 2x2 plate of spikes (a whole run of them in a trap gauntlet).
            let p = pick(dice);
            let mut cells = vec![p];
            for q in [[p[0] + 1, fy, p[2]], [p[0], fy, p[2] + 1], [p[0] + 1, fy, p[2] + 1]] {
                if has(q) {
                    cells.push(q);
                }
            }
            if *room_kind == RoomType::Trap {
                cells.truncate(2);
            }
            HazardSpot { cells, ..spot(p, (0, 0), 1, vec![]) }
        }
        Hazard::Barrel | Hazard::FireJet => spot(pick(dice), (0, 0), 3, vec![]),
        Hazard::Oil => {
            let p = pick(dice);
            let mut cells = vec![];
            for dx in -2..=2 {
                for dz in -2..=2 {
                    let q = [p[0] + dx, fy, p[2] + dz];
                    if dx * dx + dz * dz <= 4 + dice.int(0, 2) && has(q) {
                        cells.push(q);
                    }
                }
            }
            spot(p, (0, 0), 2, cells)
        }
        Hazard::Pit => {
            let p = pick(dice);
            let cells: Vec<[i32; 3]> = [[0, 0], [1, 0], [0, 1], [1, 1]].iter().map(|o| [p[0] + o[0], fy, p[2] + o[1]]).filter(|q| has(*q)).collect();
            if cells.len() < 4 {
                return None;
            }
            let pit = b_box((p[0], p[0] + 1), (fy - 2, fy - 1), (p[2], p[2] + 1));
            pits.push(pit);
            steps.push([p[0], fy - 2, p[2]]);
            spot(p, (0, 0), 2, cells)
        }
        Hazard::Collapse => {
            // A row of cracked tiles over a hole; the first has a step under it.
            let p = pick(dice);
            let dir = if dice.chance(0.5) { (1, 0) } else { (0, 1) };
            let cells: Vec<[i32; 3]> = (0..3).map(|k| [p[0] + dir.0 * k, fy, p[2] + dir.1 * k]).filter(|q| has(*q)).collect();
            if cells.len() < 2 {
                return None;
            }
            for c in &cells {
                pits.push(b_box((c[0], c[0]), (fy - 2, fy - 2), (c[2], c[2])));
            }
            steps.push([cells[0][0], fy - 2, cells[0][2]]);
            spot(p, dir, 2, cells)
        }
        Hazard::Darts => {
            // A dart wall: from one side wall straight across.
            let along_x = dice.chance(0.5);
            let p = pick(dice);
            let (pos, dir, len) = if along_x {
                ([inner.min[0] - 1, fy + 1, p[2]], (1, 0), inner.max[0] - inner.min[0] + 1)
            } else {
                ([p[0], fy + 1, inner.min[2] - 1], (0, 1), inner.max[2] - inner.min[2] + 1)
            };
            spot(pos, dir, len, vec![])
        }
        Hazard::Blade => {
            let p = pick(dice);
            let dir = if dice.chance(0.5) { (1, 0) } else { (0, 1) };
            spot(p, dir, inner.max[1] - fy, vec![])
        }
        Hazard::Any => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delve::{test_delves, POCKET_HALF};

    fn plan(run: u32, floor: u8, floors: u8) -> FloorPlan {
        FloorPlan { seed: 7, dungeon: 1, theme: 0, run, floor, floors, chain: true, done: false }
    }

    #[test]
    fn floors_are_pure_and_change_per_run() {
        let d = test_delves();
        assert_eq!(floor(&d, &plan(1, 1, 2)), floor(&d, &plan(1, 1, 2)));
        assert_ne!(floor(&d, &plan(1, 1, 2)).rooms, floor(&d, &plan(2, 1, 2)).rooms);
        let other = FloorPlan { dungeon: 99, ..plan(1, 1, 2) };
        assert_ne!(floor(&d, &plan(1, 1, 2)).rooms, floor(&d, &other).rooms);
    }

    #[test]
    fn every_floor_has_its_parts_and_they_connect() {
        let d = test_delves();
        for theme in 0..d.themes.len() {
            for run in 0..30 {
                for fl in 1..=4u8 {
                    let p = FloorPlan { theme, ..plan(run, fl, 4) };
                    let f = floor(&d, &p);
                    let count = |k: RoomType| f.rooms.iter().filter(|r| r.kind == k).count();
                    assert_eq!(f.rooms[0].kind, RoomType::Start);
                    assert_eq!(count(RoomType::Boss), 1);
                    assert_eq!(f.rooms[f.boss_room].kind, RoomType::Boss);
                    assert_eq!(count(RoomType::Puzzle), 1, "run {run} floor {fl}");
                    assert_eq!(count(RoomType::Rest), 1);
                    assert!(count(RoomType::Trap) >= 1);
                    assert!(count(RoomType::Secret) <= 1);
                    assert_eq!(f.spawns.iter().filter(|s| s.kind == SpawnKind::Boss).count(), 1);
                    assert!(f.spawns.iter().all(|s| s.room != 0), "nothing waits in the start room");
                    assert_eq!(f.room_at(f.entry), Some(0));
                    // Every room is reached from the start, and the boss only through the locked door.
                    let mut seen = vec![false; f.rooms.len()];
                    seen[0] = true;
                    let mut stack = vec![0];
                    while let Some(a) = stack.pop() {
                        for l in &f.links {
                            let b = if l.a == a { l.b } else if l.b == a { l.a } else { continue };
                            if !seen[b] {
                                seen[b] = true;
                                stack.push(b);
                            }
                        }
                    }
                    assert!(seen.iter().all(|s| *s), "run {run} floor {fl}");
                    let into_boss: Vec<_> = f.links.iter().filter(|l| l.a == f.boss_room || l.b == f.boss_room).collect();
                    assert_eq!(into_boss.len(), 1);
                    assert!(into_boss[0].locked);
                    assert_ne!(f.key_room, f.boss_room);
                    assert!(f.rooms[f.key_room].inner.contains(f.key_at[0], f.key_at[1], f.key_at[2]));
                    // The ways on.
                    assert_eq!(f.stair.is_some(), fl < 4);
                    assert!(f.passage.is_some() == (fl == 4));
                    assert!(f.exit.grow(1).intersects(&f.rooms[f.boss_room].inner));
                    assert!(f.bounds.min[0] >= POCKET[0] - POCKET_HALF && f.bounds.max[0] <= POCKET[0] + POCKET_HALF);
                    assert!(f.bounds.max[1] < crate::WORLD_CEILING && f.bounds.min[1] > crate::SKY_MAX);
                    // Hazards never sit in a doorway.
                    for h in f.hazards.iter().filter(|h| h.room.is_some()) {
                        for dw in &f.doors {
                            assert!(!dw.at.contains(h.pos[0], h.pos[1], h.pos[2]));
                        }
                    }
                    // The puzzle can be solved.
                    let pz = f.puzzle.as_ref().unwrap();
                    match pz.kind {
                        PuzzleKind::Plates => assert_eq!(pz.stones.len() + 1, pz.plates.len()),
                        PuzzleKind::Levers => {
                            assert_ne!(pz.start, pz.all_lit());
                            assert!(crate::delve::floor::solvable(pz));
                        }
                        PuzzleKind::Timed => assert!(pz.levers.len() == 1 && pz.open_for > 1.0),
                    }
                }
            }
        }
    }

    #[test]
    fn deeper_floors_are_bigger_and_harder() {
        let d = test_delves();
        let (mut rooms, mut foes) = ([0; 4], [0; 4]);
        for run in 0..30 {
            for fl in 1..=4u8 {
                let f = floor(&d, &plan(run, fl, 4));
                rooms[fl as usize - 1] += f.rooms.len();
                foes[fl as usize - 1] += f.spawns.len();
            }
        }
        for w in 0..3 {
            assert!(rooms[w + 1] > rooms[w] && foes[w + 1] > foes[w], "{rooms:?} {foes:?}");
        }
    }

    #[test]
    fn spawns_and_hazards_stand_on_open_floor() {
        let d = test_delves();
        for run in 0..10 {
            let f = floor(&d, &plan(run, 3, 4));
            for s in &f.spawns {
                assert!(f.inside([s.pos[0] as f32 + 0.5, s.pos[1] as f32, s.pos[2] as f32 + 0.5]), "{s:?}");
            }
            for h in f.hazards.iter().filter(|h| h.kind != Hazard::Darts) {
                assert!(f.inside([h.pos[0] as f32 + 0.5, h.pos[1] as f32, h.pos[2] as f32 + 0.5]), "{h:?}");
            }
        }
    }

    #[test]
    fn a_cleared_dungeon_keeps_its_last_boss_and_prize_away() {
        let d = test_delves();
        let p = FloorPlan { done: true, ..plan(3, 2, 2) };
        let f = floor(&d, &p);
        assert!(!f.spawns.iter().any(|s| s.kind == SpawnKind::Boss));
        assert!(!f.chests.iter().any(|c| c.kind == ChestKind::Prize));
        let f = floor(&d, &plan(3, 2, 2));
        assert!(f.chests.iter().any(|c| c.kind == ChestKind::Prize));
        // Not the last floor: its warden always stands.
        let f = floor(&d, &FloorPlan { done: true, ..plan(3, 1, 2) });
        assert!(f.spawns.iter().any(|s| s.kind == SpawnKind::Boss));
    }

    #[test]
    fn lock_ins_shut_only_the_way_out_of_a_section() {
        let d = test_delves();
        let f = (0..200).map(|run| floor(&d, &plan(run, 4, 4))).find(|f| !f.sections.is_empty()).expect("a floor with a section");
        let s = &f.sections[0];
        let doors = f.lock_doors(s[0]);
        assert!(!doors.is_empty());
        assert_eq!(doors, f.lock_doors(s[1]), "both rooms seal the same doors");
        for dw in &doors {
            let l = &f.links[dw.link];
            assert!(!(s.contains(&l.a) && s.contains(&l.b)), "the door between them stays open");
        }
    }
}

/// Can the lever puzzle be solved from its start? (Every lamp lit by some set of pulls.)
pub fn solvable(p: &Puzzle) -> bool {
    let n = p.flips.len();
    (0u32..1 << n).any(|pulls| {
        let mut s = p.start;
        for (i, f) in p.flips.iter().enumerate() {
            if pulls & (1 << i) != 0 {
                s ^= f;
            }
        }
        s == p.all_lit()
    })
}
