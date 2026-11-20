//! Warded dungeons: a stone gatehouse on the surface with one doorway, a long
//! stair down, then a network of large rooms joined by wide corridors, all on
//! one floor deep under the ground. The last room (the furthest walk from the
//! stair) is the warden's hall: the biggest, where the boss stands.
//!
//! Everything is a pure function of the seed and the gate position, like the
//! other structures: each chunk draws the parts of nearby dungeons that fall in
//! it. A dungeon lists `spaces` (the open boxes you can walk in, for "you are
//! inside"), `spawns` (where enemies stand; the game spawns them) and `bounds`
//! (the whole warded box: no block in it can be broken or placed).
//!
//! The layout grows on a grid of 30-block cells in the dungeon's own frame
//! (u runs away from the door, v across), so a dungeon can face any of the four
//! directions and boxes stay axis-aligned.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use voxel::BlockId;

use crate::structures::{cached, pick, BARROW_KIN};
use crate::trees::Canvas;
use crate::util::{hash2, unit};
use crate::{Land, WorldGen, SEA_LEVEL};

/// One dungeon per cell of this size (when the land allows), plus one near spawn.
const CELL: i32 = 1024;
const CHANCE: f32 = 0.7;
/// Room grid: cells along u (away from the door) and across (v = -SIDE..=SIDE).
const GRID: i32 = 30;
const ALONG: i32 = 5;
const SIDE: i32 = 2;
/// How far a dungeon reaches from its gate, any direction (for lookups).
const REACH: i32 = 200;
/// Corridors: inner half-width and height.
const HALL_HALF: i32 = 1;
const HALL_HEIGHT: i32 = 4;
/// Regular dungeons keep this far from spawn (the home dungeon is nearer).
const CLEAR_OF_SPAWN: i32 = 700;

const DUNGEON_KIND: [&str; 6] = ["Sunken Halls of", "Drowned Vaults of", "Hollow Court of", "Warded Deep of", "Silent Cloister of", "Mourning Keep of"];

/// An inclusive box of blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aabb {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

impl Aabb {
    pub fn contains(&self, x: i32, y: i32, z: i32) -> bool {
        (self.min[0]..=self.max[0]).contains(&x) && (self.min[1]..=self.max[1]).contains(&y) && (self.min[2]..=self.max[2]).contains(&z)
    }

    pub fn grow(&self, by: i32) -> Self {
        Self { min: self.min.map(|v| v - by), max: self.max.map(|v| v + by) }
    }

    pub fn union(&self, o: &Aabb) -> Self {
        Self { min: [0, 1, 2].map(|i| self.min[i].min(o.min[i])), max: [0, 1, 2].map(|i| self.max[i].max(o.max[i])) }
    }

    pub fn intersects(&self, o: &Aabb) -> bool {
        (0..3).all(|i| self.min[i] <= o.max[i] && o.min[i] <= self.max[i])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomKind {
    /// The first room at the foot of the stair; nothing spawns here.
    Entry,
    Hall,
    /// The furthest room: the biggest, with the boss.
    Warden,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Room {
    pub kind: RoomKind,
    /// The open space inside the walls.
    pub inner: Aabb,
}

/// What kind of enemy a spawn point wants (the combat side picks the creature).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnKind {
    /// One standing watch in a corridor or by the boss.
    Guard,
    /// One of a group in a room.
    Pack,
    /// The boss of the warden's hall.
    Boss,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnPoint {
    /// The block the enemy's feet stand in.
    pub pos: [i32; 3],
    pub kind: SpawnKind,
    /// The room it belongs to (corridor guards belong to the room they lead into).
    pub room: usize,
}

/// The shimmering veil across the doorway: only spirits pass it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Veil {
    /// Centre of the doorway's opening.
    pub center: [f32; 3],
    /// Unit step pointing out of the dungeon (x, z).
    pub out: (i32, i32),
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dungeon {
    pub id: u64,
    pub name: String,
    /// Centre of the gatehouse floor (feet height).
    pub gate: [i32; 3],
    /// Where you stand on the path in front of the door, outside the veil
    /// (feet). Turned-back walkers and spirits cast out land here.
    pub outside: [f32; 3],
    /// Where the guide stands beside the path (feet).
    pub guide: [f32; 3],
    pub veil: Veil,
    /// Feet height of every room and corridor.
    pub floor: i32,
    pub rooms: Vec<Room>,
    /// Corridors between rooms (indices into `rooms`).
    pub links: Vec<(usize, usize)>,
    pub spawns: Vec<SpawnPoint>,
    /// Open boxes past the veil: gatehouse, stair, corridors, rooms.
    pub spaces: Vec<Aabb>,
    /// Everything built, walls included: the spirit ward covers it.
    pub bounds: Aabb,
    /// Local frame: u runs along `dir` (into the dungeon), v along `side`.
    dir: (i32, i32),
    side: (i32, i32),
}

impl Dungeon {
    /// A block is under the ward (can't be broken or built on).
    pub fn warded(&self, x: i32, y: i32, z: i32) -> bool {
        self.bounds.contains(x, y, z)
    }

    /// A point (feet) is past the veil, somewhere inside.
    pub fn inside(&self, p: [f32; 3]) -> bool {
        let b = p.map(|v| v.floor() as i32);
        self.spaces.iter().any(|s| s.contains(b[0], b[1], b[2]))
    }

    /// The room a point (feet) stands in.
    pub fn room_at(&self, p: [f32; 3]) -> Option<usize> {
        let b = p.map(|v| v.floor() as i32);
        self.rooms.iter().position(|r| r.inner.contains(b[0], b[1], b[2]))
    }

    fn world(&self, u: i32, v: i32) -> (i32, i32) {
        frame_world(self.gate, self.dir, self.side, u, v)
    }
}

fn frame_world(gate: [i32; 3], dir: (i32, i32), side: (i32, i32), u: i32, v: i32) -> (i32, i32) {
    (gate[0] + u * dir.0 + v * side.0, gate[2] + u * dir.1 + v * side.1)
}

/// A box given in the local frame (inclusive u, v and y ranges).
fn local_box(gate: [i32; 3], dir: (i32, i32), side: (i32, i32), u: (i32, i32), v: (i32, i32), y: (i32, i32)) -> Aabb {
    let a = frame_world(gate, dir, side, u.0, v.0);
    let b = frame_world(gate, dir, side, u.1, v.1);
    Aabb { min: [a.0.min(b.0), y.0, a.1.min(b.1)], max: [a.0.max(b.0), y.1, a.1.max(b.1)] }
}

/// A small deterministic dice for laying out one dungeon.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A whole number in lo..=hi.
    fn int(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }

    fn chance(&mut self, p: f32) -> bool {
        unit(self.next()) < p
    }
}

impl WorldGen {
    /// Dungeons whose warded area touches the rectangle (inclusive block coordinates).
    pub fn dungeons_in(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> Vec<Arc<Dungeon>> {
        let mut out = Vec::new();
        let touches = |d: &Dungeon| d.bounds.min[0] <= x1 && d.bounds.max[0] >= x0 && d.bounds.min[2] <= z1 && d.bounds.max[2] >= z0;
        if let Some(d) = &self.home_dungeon
            && touches(d)
        {
            out.push(d.clone());
        }
        let [sx, _, sz] = self.spawn;
        for cz in (z0 - REACH).div_euclid(CELL)..=(z1 + REACH).div_euclid(CELL) {
            for cx in (x0 - REACH).div_euclid(CELL)..=(x1 + REACH).div_euclid(CELL) {
                let h = hash2(self.seed ^ 0xD0D6, cx, cz);
                if unit(h) >= CHANCE {
                    continue;
                }
                let span = CELL - 2 * REACH;
                let x = cx * CELL + REACH + (h >> 8) as i32 % span;
                let z = cz * CELL + REACH + (h >> 24) as i32 % span;
                if x + REACH < x0 || x - REACH > x1 || z + REACH < z0 || z - REACH > z1 {
                    continue;
                }
                if ((x - sx) as i64).pow(2) + ((z - sz) as i64).pow(2) < (CLEAR_OF_SPAWN as i64).pow(2) {
                    continue;
                }
                if let Some(d) = cached(&self.structures.dungeons, (cx, cz), || self.build_dungeon(h, x, z))
                    && touches(&d)
                {
                    out.push(d);
                }
            }
        }
        out
    }

    /// The dungeon a short walk from spawn.
    pub fn home_dungeon(&self) -> Option<Arc<Dungeon>> {
        self.home_dungeon.clone()
    }

    /// Ground 170-290 blocks from spawn, not towards `WorldGen::home`, where a
    /// whole dungeon fits under the island.
    pub(crate) fn search_home_dungeon(&self) -> Option<Dungeon> {
        let [sx, _, sz] = self.spawn;
        let village = ((self.home.1 - sz) as f32).atan2((self.home.0 - sx) as f32);
        for ring in 0..5 {
            let r = 170.0 + ring as f32 * 30.0;
            for s in 0..24 {
                let a = village + std::f32::consts::PI + (s as f32 / 24.0 - 0.5) * 1.5 * std::f32::consts::PI;
                let (x, z) = (sx + (a.cos() * r) as i32, sz + (a.sin() * r) as i32);
                if let Some(d) = self.build_dungeon(hash2(self.seed ^ 0xD0D7, x, z), x, z) {
                    return Some(d);
                }
            }
        }
        None
    }

    /// Is this block protected by a spirit ward (inside a dungeon, barrow or village)?
    pub fn warded(&self, x: i32, y: i32, z: i32) -> bool {
        self.dungeons_in(x, z, x, z).iter().any(|d| d.warded(x, y, z))
            || self.barrows_in(x, z, x, z).iter().any(|b| b.warded(x, y, z))
            || self.villages_in(x, z, x, z).iter().any(|v| v.warded(x, y, z))
            || self.rotting_gate().is_some_and(|g| g.bounds.contains(x, y, z))
            || crate::rotting::in_pocket(x, z)
    }

    pub(crate) fn build_dungeon(&self, h: u64, gx: i32, gz: i32) -> Option<Dungeon> {
        let col = self.column(gx, gz);
        if col.land != Land::Main || col.height < SEA_LEVEL + 1 || col.climate.overhang > 0.02 {
            return None;
        }
        let mut dice = Dice(h);
        let dirs = [(1, 0), (0, 1), (-1, 0), (0, -1)];
        let dir = dirs[dice.int(0, 3) as usize];
        let side = (-dir.1, dir.0);
        let g = col.height + 1;
        let gate = [gx, g, gz];
        let at = |u: i32, v: i32| frame_world(gate, dir, side, u, v);
        // The gatehouse and its path need fairly level, dry ground.
        let mut lo = i32::MAX;
        let mut hi = i32::MIN;
        for u in [-8, -4, 0, 3] {
            for v in [-3, 0, 3] {
                let (x, z) = at(u, v);
                let c = self.column(x, z);
                if c.land != Land::Main || c.height < c.water {
                    return None;
                }
                lo = lo.min(c.height);
                hi = hi.max(c.height);
            }
        }
        if hi - lo > 4 {
            return None;
        }
        let depth = dice.int(20, 27);
        let floor = g - depth;
        // A room cell is usable when solid island surrounds it on every side,
        // with ground over its ceiling and island under its floor.
        let centre = |i: i32, j: i32| (depth + 2 + GRID / 2 + i * GRID, j * GRID);
        let usable = |i: i32, j: i32| {
            let (cu, cv) = centre(i, j);
            [(0, 0), (-13, -13), (-13, 13), (13, -13), (13, 13)].iter().all(|(du, dv)| {
                let (x, z) = at(cu + du, cv + dv);
                let c = self.column(x, z);
                c.land == Land::Main && c.bottom < floor - 6 && c.height > floor + 14
            })
        };
        // The stair runs down under the ground too.
        let (sx, sz) = at(depth, 0);
        let sc = self.column(sx, sz);
        if sc.land != Land::Main || sc.bottom > floor - 6 || !usable(0, 0) {
            return None;
        }
        // Grow the rooms: pick a room, try a neighbouring cell.
        let mut cells = vec![(0, 0)];
        let mut index: HashMap<(i32, i32), usize> = HashMap::from([((0, 0), 0)]);
        let mut links = Vec::new();
        let want = dice.int(7, 10) as usize;
        let mut checked: HashMap<(i32, i32), bool> = HashMap::new();
        for _ in 0..300 {
            if cells.len() >= want {
                break;
            }
            let from = dice.int(0, cells.len() as i32 - 1) as usize;
            let (di, dj) = dirs[dice.int(0, 3) as usize];
            let (i, j) = (cells[from].0 + di, cells[from].1 + dj);
            if !(0..ALONG).contains(&i) || !(-SIDE..=SIDE).contains(&j) || index.contains_key(&(i, j)) {
                continue;
            }
            if !*checked.entry((i, j)).or_insert_with(|| usable(i, j)) {
                continue;
            }
            index.insert((i, j), cells.len());
            links.push((from, cells.len()));
            cells.push((i, j));
        }
        if cells.len() < 4 {
            return None;
        }
        // A few extra corridors make loops, so it's not one long line.
        for (a, &(i, j)) in cells.iter().enumerate() {
            for (di, dj) in [(1, 0), (0, 1)] {
                if let Some(&b) = index.get(&(i + di, j + dj))
                    && !links.contains(&(a, b))
                    && !links.contains(&(b, a))
                    && dice.chance(0.25)
                {
                    links.push((a, b));
                }
            }
        }
        // The warden's hall: the room furthest from the stair.
        let mut dist = vec![usize::MAX; cells.len()];
        dist[0] = 0;
        let mut queue = VecDeque::from([0]);
        while let Some(a) = queue.pop_front() {
            for &(p, q) in &links {
                let b = if p == a { q } else if q == a { p } else { continue };
                if dist[b] == usize::MAX {
                    dist[b] = dist[a] + 1;
                    queue.push_back(b);
                }
            }
        }
        let warden = (1..cells.len()).max_by_key(|&i| (dist[i], i)).unwrap_or(1);
        let lbox = |u: (i32, i32), v: (i32, i32), y: (i32, i32)| local_box(gate, dir, side, u, v, y);
        let mut rooms = Vec::new();
        let mut room_local = Vec::new();
        for (n, &(i, j)) in cells.iter().enumerate() {
            let (cu, cv) = centre(i, j);
            let (kind, ru, rv, height) = if n == 0 {
                (RoomKind::Entry, dice.int(6, 8), dice.int(6, 8), 7)
            } else if n == warden {
                (RoomKind::Warden, 12, 12, 11)
            } else {
                (RoomKind::Hall, dice.int(6, 11), dice.int(6, 11), dice.int(6, 9))
            };
            room_local.push((cu, cv, ru, rv));
            rooms.push(Room { kind, inner: lbox((cu - ru, cu + ru), (cv - rv, cv + rv), (floor, floor + height - 1)) });
        }
        // Open spaces: gatehouse, stair, the corridor from the stair, corridors, rooms.
        let mut spaces = vec![lbox((-2, 2), (-2, 2), (g, g + 3))];
        for k in 0..depth {
            spaces.push(lbox((k, k), (-HALL_HALF, HALL_HALF), (g - k, g - k + 3)));
        }
        spaces.push(lbox((depth, room_local[0].0), (-HALL_HALF, HALL_HALF), (floor, floor + HALL_HEIGHT - 1)));
        let corridor = |a: usize, b: usize| {
            let (au, av, _, _) = room_local[a];
            let (bu, bv, _, _) = room_local[b];
            if av == bv {
                lbox((au.min(bu), au.max(bu)), (av - HALL_HALF, av + HALL_HALF), (floor, floor + HALL_HEIGHT - 1))
            } else {
                lbox((au - HALL_HALF, au + HALL_HALF), (av.min(bv), av.max(bv)), (floor, floor + HALL_HEIGHT - 1))
            }
        };
        for &(a, b) in &links {
            spaces.push(corridor(a, b));
        }
        spaces.extend(rooms.iter().map(|r| r.inner));
        // Enemies: packs in the halls, guards in some corridors, the boss and
        // two guards in the warden's hall.
        let mut spawns = Vec::new();
        let pillar = |n: usize, du: i32, dv: i32| {
            let (_, _, ru, rv) = room_local[n];
            ru >= 8 && rv >= 8 && du.abs() == ru - 3 && dv.abs() == rv - 3
        };
        for (n, room) in rooms.iter().enumerate() {
            let (cu, cv, ru, rv) = room_local[n];
            let put = |du: i32, dv: i32| {
                let (x, z) = at(cu + du, cv + dv);
                [x, floor, z]
            };
            match room.kind {
                RoomKind::Entry => {}
                RoomKind::Warden => {
                    spawns.push(SpawnPoint { pos: put(0, 0), kind: SpawnKind::Boss, room: n });
                    spawns.push(SpawnPoint { pos: put(-4, -5), kind: SpawnKind::Guard, room: n });
                    spawns.push(SpawnPoint { pos: put(-4, 5), kind: SpawnKind::Guard, room: n });
                }
                RoomKind::Hall => {
                    let count = (2 + (ru * rv) / 30).min(6);
                    let mut tries = 0;
                    let mut placed = 0;
                    while placed < count && tries < 40 {
                        tries += 1;
                        let (du, dv) = (dice.int(-(ru - 2), ru - 2), dice.int(-(rv - 2), rv - 2));
                        let pos = put(du, dv);
                        if pillar(n, du, dv) || spawns.iter().any(|s: &SpawnPoint| s.pos == pos) {
                            continue;
                        }
                        spawns.push(SpawnPoint { pos, kind: SpawnKind::Pack, room: n });
                        placed += 1;
                    }
                }
            }
        }
        for &(a, b) in &links {
            if dice.chance(0.5) {
                let (au, av, _, _) = room_local[a];
                let (bu, bv, _, _) = room_local[b];
                let (x, z) = at((au + bu) / 2, (av + bv) / 2);
                let room = if dist[b] >= dist[a] { b } else { a };
                spawns.push(SpawnPoint { pos: [x, floor, z], kind: SpawnKind::Guard, room });
            }
        }
        // The gatehouse, its pillars and the path in front of it.
        let house = lbox((-3, 3), (-3, 3), (g - 1, g + 5));
        let path = lbox((-8, -3), (-2, 2), (lo - 1, g + 4));
        let mut bounds = spaces.iter().fold(house.union(&path), |acc, s| acc.union(&s.grow(1)));
        bounds.min[1] = bounds.min[1].min(lo - 1);
        // Keep clear of villages.
        if !self.villages_in(bounds.min[0] - 8, bounds.min[2] - 8, bounds.max[0] + 8, bounds.max[2] + 8).is_empty() {
            return None;
        }
        let (ox, oz) = at(-6, 0);
        let (wx, wz) = at(-5, -3);
        let guide_y = self.column(wx, wz).height + 1;
        let (vx, vz) = at(-3, 0);
        let name = format!("The {} {}", pick(&DUNGEON_KIND, h >> 13), pick(&BARROW_KIN, h >> 29));
        Some(Dungeon {
            id: h,
            name,
            gate,
            outside: [ox as f32 + 0.5, g as f32, oz as f32 + 0.5],
            guide: [wx as f32 + 0.5, guide_y as f32, wz as f32 + 0.5],
            veil: Veil { center: [vx as f32 + 0.5, g as f32 + 1.5, vz as f32 + 0.5], out: (-dir.0, -dir.1), width: 3.0, height: 3.0 },
            floor,
            rooms,
            links,
            spawns,
            spaces,
            bounds,
            dir,
            side,
        })
    }
}

/// Fill a box with a block, clipped to the chunk.
fn fill(c: &mut Canvas, b: &Aabb, block: BlockId) {
    let lo = c.origin;
    let hi = lo.map(|v| v + voxel::CHUNK_SIZE_I32 - 1);
    for y in b.min[1].max(lo[1])..=b.max[1].min(hi[1]) {
        for z in b.min[2].max(lo[2])..=b.max[2].min(hi[2]) {
            for x in b.min[0].max(lo[0])..=b.max[0].min(hi[0]) {
                c.put(x, y, z, block);
            }
        }
    }
}

/// Draw the parts of a dungeon that fall in this chunk.
pub(crate) fn draw(wg: &WorldGen, c: &mut Canvas, d: &Dungeon) {
    let ids = &wg.ids;
    let chunk = Aabb { min: c.origin, max: c.origin.map(|v| v + voxel::CHUNK_SIZE_I32 - 1) };
    let g = d.gate[1];
    let lbox = |u: (i32, i32), v: (i32, i32), y: (i32, i32)| local_box(d.gate, d.dir, d.side, u, v, y);
    // Shells first: every open space gets a deeprock skin and a stone floor;
    // the air is carved afterwards so corridors open into the rooms.
    for s in &d.spaces {
        let shell = s.grow(1);
        if !shell.intersects(&chunk) {
            continue;
        }
        fill(c, &shell, ids.deeprock);
        fill(c, &Aabb { min: [shell.min[0], shell.min[1], shell.min[2]], max: [shell.max[0], shell.min[1], shell.max[2]] }, ids.cobblestone);
    }
    // The gatehouse: stone walls on a foundation down to the ground, a roof,
    // and two tall deeprock pillars by the door.
    let house = lbox((-3, 3), (-3, 3), (g - 1, g + 4));
    if house.grow(3).intersects(&chunk) {
        for u in -3..=3 {
            for v in -3..=3 {
                let (x, z) = d.world(u, v);
                let ground = wg.column(x, z).height;
                for y in ground.min(g - 2)..g - 1 {
                    c.put(x, y, z, ids.cobblestone);
                }
            }
        }
        fill(c, &house, ids.cobblestone);
        fill(c, &lbox((-4, 4), (-4, 4), (g + 5, g + 5)), ids.deeprock);
        for v in [-2, 2] {
            fill(c, &lbox((-4, -4), (v, v), (g - 1, g + 6)), ids.deeprock);
        }
    }
    // The path to the door: cleared, paved, filled underneath.
    for u in -8..=-4 {
        for v in -2..=2 {
            let (x, z) = d.world(u, v);
            if !chunk.grow(1).contains(x, chunk.min[1], z) {
                continue;
            }
            let ground = wg.column(x, z).height;
            for y in ground.min(g - 2)..g - 1 {
                c.put(x, y, z, ids.cobblestone);
            }
            c.put(x, g - 1, z, ids.cobblestone);
            for y in g..=g + 4 {
                c.put(x, y, z, BlockId::AIR);
            }
        }
    }
    // Air: the doorway (where the veil hangs) and every open space.
    fill(c, &lbox((-3, -3), (-1, 1), (g, g + 2)), BlockId::AIR);
    for s in &d.spaces {
        if s.intersects(&chunk) {
            fill(c, s, BlockId::AIR);
        }
    }
    // Pillars in the big rooms and torches in the corners.
    for r in &d.rooms {
        if !r.inner.grow(1).intersects(&chunk) {
            continue;
        }
        let (w, h) = (r.inner.max[0] - r.inner.min[0], r.inner.max[2] - r.inner.min[2]);
        if w >= 16 && h >= 16 {
            for (x, z) in [(r.inner.min[0] + 3, r.inner.min[2] + 3), (r.inner.min[0] + 3, r.inner.max[2] - 3), (r.inner.max[0] - 3, r.inner.min[2] + 3), (r.inner.max[0] - 3, r.inner.max[2] - 3)] {
                fill(c, &Aabb { min: [x, r.inner.min[1], z], max: [x, r.inner.max[1], z] }, ids.deeprock);
            }
        }
        let y = r.inner.min[1];
        c.put(r.inner.min[0], y, r.inner.min[2], ids.torch);
        c.put(r.inner.max[0], y, r.inner.max[2], ids.torch);
        if r.kind == RoomKind::Warden {
            c.put(r.inner.min[0], y, r.inner.max[2], ids.torch);
            c.put(r.inner.max[0], y, r.inner.min[2], ids.torch);
        }
    }
    // Two torches in the gatehouse, clear of the stair.
    for v in [-2, 2] {
        let (x, z) = d.world(-2, v);
        c.put(x, g, z, ids.torch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::wg;

    fn home() -> (WorldGen, Arc<Dungeon>) {
        let g = wg();
        let d = g.home_dungeon().expect("a home dungeon near spawn");
        (g, d)
    }

    #[test]
    fn home_dungeon_is_near_spawn_with_big_rooms() {
        let (g, d) = home();
        let [sx, _, sz] = g.find_spawn();
        let dist = (((d.gate[0] - sx).pow(2) + (d.gate[2] - sz).pow(2)) as f32).sqrt();
        assert!((150.0..320.0).contains(&dist), "{} is {dist} blocks from spawn", d.name);
        assert!(d.rooms.len() >= 4, "{} rooms", d.rooms.len());
        assert_eq!(d.rooms.iter().filter(|r| r.kind == RoomKind::Warden).count(), 1);
        assert_eq!(d.rooms[0].kind, RoomKind::Entry);
        for r in &d.rooms {
            let w = r.inner.max[0] - r.inner.min[0] + 1;
            let l = r.inner.max[2] - r.inner.min[2] + 1;
            assert!(w >= 13 && l >= 13, "room {w}x{l} is too small");
        }
        // Deterministic.
        assert_eq!(*g.home_dungeon().unwrap(), *d);
        eprintln!("{} at {:?}, {} rooms, {} spawns, floor {}", d.name, d.gate, d.rooms.len(), d.spawns.len(), d.floor);
    }

    #[test]
    fn every_room_is_reachable_from_the_stair() {
        let (_, d) = home();
        let mut seen = vec![false; d.rooms.len()];
        seen[0] = true;
        let mut changed = true;
        while changed {
            changed = false;
            for &(a, b) in &d.links {
                if seen[a] != seen[b] {
                    seen[a] = true;
                    seen[b] = true;
                    changed = true;
                }
            }
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn spawns_stand_inside_on_open_floor() {
        let (_, d) = home();
        assert!(d.spawns.iter().any(|s| s.kind == SpawnKind::Boss));
        assert!(!d.spawns.iter().any(|s| d.rooms[s.room].kind == RoomKind::Entry), "nothing spawns in the entry room");
        for s in &d.spawns {
            let p = [s.pos[0] as f32 + 0.5, s.pos[1] as f32, s.pos[2] as f32 + 0.5];
            assert!(d.inside(p), "{s:?} is not in an open space");
            assert!(d.warded(s.pos[0], s.pos[1], s.pos[2]));
        }
    }

    #[test]
    fn one_way_in_past_the_veil() {
        let (g, d) = home();
        // The path in front of the door is outside; one step through the doorway is inside.
        assert!(!d.inside(d.outside));
        let (ox, oz) = d.veil.out;
        let past = [d.veil.center[0] - ox as f32 * 1.0, d.gate[1] as f32, d.veil.center[2] - oz as f32 * 1.0];
        assert!(d.inside(past));
        // Walking from the door to the warden's hall stays inside the open spaces:
        // the gatehouse, stair and first corridor are joined.
        let entry = &d.rooms[0].inner;
        assert!(d.inside([(entry.min[0] + entry.max[0]) as f32 / 2.0 + 0.5, d.floor as f32, (entry.min[2] + entry.max[2]) as f32 / 2.0 + 0.5]));
        // Blocks are as described: the doorway open, the gate floor stone, room air open.
        let block = |x: i32, y: i32, z: i32| {
            let p = voxel::BlockPos::new(x, y, z);
            let l = p.local();
            g.generate(p.chunk()).get(l[0], l[1], l[2])
        };
        let door = d.veil.center.map(|v| v.floor() as i32);
        assert_eq!(block(door[0], d.gate[1], door[2]), BlockId::AIR, "doorway");
        let (cx, cz) = ((entry.min[0] + entry.max[0]) / 2, (entry.min[2] + entry.max[2]) / 2);
        assert_eq!(block(cx, d.floor, cz), BlockId::AIR, "entry room air");
        assert_eq!(block(cx, d.floor - 1, cz), g.ids.cobblestone, "entry room floor");
        // The rest of the gatehouse wall beside the door is solid.
        let (sx, sz) = d.world(-3, -2);
        assert_ne!(block(sx, d.gate[1] + 1, sz), BlockId::AIR, "gatehouse wall");
    }

    #[test]
    fn named_places_are_warded() {
        let (g, d) = home();
        let r = &d.rooms[1].inner;
        assert!(g.warded(r.min[0] - 1, r.min[1], r.min[2]), "room wall");
        assert!(g.warded(d.gate[0], d.gate[1] + 5, d.gate[2]), "gatehouse roof");
        // Open country is not.
        let [sx, sy, sz] = g.find_spawn();
        assert!(!g.warded(sx, sy - 1, sz));
    }

    #[test]
    fn dungeons_are_scattered_over_the_island() {
        let g = wg();
        let (cx, cz) = g.island_center();
        let r = crate::MAIN_RADIUS as i32;
        let all = g.dungeons_in(cx - r, cz - r, cx + r, cz + r);
        assert!(all.len() >= 4, "{} dungeons", all.len());
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(!a.bounds.intersects(&b.bounds), "{} overlaps {}", a.name, b.name);
            }
        }
    }
}
