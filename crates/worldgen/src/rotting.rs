//! The Rotting Gate and the floors under it.
//!
//! The gate is a squat, mold-covered stone arch placed once on the main
//! island, 300-600 blocks from spawn. Walking into it takes you to a floor of
//! the dungeon below, which is not part of the island: it is laid out fresh
//! for every run, as a pure function of (seed, run number, depth), and built
//! in a pocket of empty sky far from the island (`POCKET`), where nothing else
//! ever generates. The game asks `WorldGen::generate_pocket` for the chunks
//! there instead of the normal terrain, and never saves them.
//!
//! A floor is a grid of rooms joined by corridors: the start room (with the
//! way back up in its west wall), halls, and the boss room (the furthest from
//! the start, with the stair down in one of its free walls). Four depths, each
//! bigger, with more and tougher foes.

use std::collections::{HashMap, VecDeque};

use voxel::chunk::CHUNK_VOLUME;
use voxel::{BlockId, Chunk, ChunkPos, CHUNK_SIZE_I32};

use crate::dungeons::{Aabb, SpawnKind, SpawnPoint};
use crate::trees::Canvas;
use crate::util::{hash2, unit};
use crate::{Land, WorldGen, SEA_LEVEL};

/// How many depths there are.
pub const DEPTHS: u8 = 4;
/// Where the start room's floor is centred: far out in the empty sky, above
/// every island (sky islands stop at y 600) and under the ceiling.
pub const POCKET: [i32; 3] = [60_000, 780, 60_000];
/// Half the size of the pocket, blocks (every floor fits well inside).
const POCKET_HALF: i32 = 1_000;
/// Room cells, blocks apart.
const GRID: i32 = 28;
/// Corridors: half width, height.
const HALL_HALF: i32 = 1;
const HALL_HEIGHT: i32 = 4;

/// Is this column in the pocket where the floors are built?
pub fn in_pocket(x: i32, z: i32) -> bool {
    (x - POCKET[0]).abs() <= POCKET_HALF && (z - POCKET[2]).abs() <= POCKET_HALF
}

/// The gate on the island.
#[derive(Debug, Clone, PartialEq)]
pub struct RottingGate {
    /// Centre of the gate's floor (feet height).
    pub pos: [i32; 3],
    /// Unit step pointing out of the door (x, z).
    pub out: (i32, i32),
    /// Walk into this box and you go down.
    pub door: Aabb,
    /// Where you stand in front of it (feet): coming back up, or after dying below.
    pub outside: [f32; 3],
    /// Everything built (warded).
    pub bounds: Aabb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorKind {
    Start,
    Hall,
    Boss,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FloorRoom {
    pub kind: FloorKind,
    /// The open space inside the walls.
    pub inner: Aabb,
}

/// One floor of one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Floor {
    pub run: u32,
    pub depth: u8,
    pub rooms: Vec<FloorRoom>,
    /// Corridors between rooms (indices into `rooms`).
    pub links: Vec<(usize, usize)>,
    pub spawns: Vec<SpawnPoint>,
    /// Every open box: rooms, corridors, the way out and the stair.
    pub spaces: Vec<Aabb>,
    /// Everything built, walls included.
    pub bounds: Aabb,
    /// Where you arrive (feet), in the start room.
    pub entry: [f32; 3],
    /// Step into this (in the start room's west wall) to go back up to the gate.
    pub way_out: Aabb,
    /// Step into this (in the boss room's wall) to go down a depth.
    pub stair: Aabb,
    pub boss_room: usize,
    /// Feet height of every room.
    pub floor_y: i32,
    torches: Vec<[i32; 3]>,
}

impl Floor {
    /// The room a point (feet) stands in.
    pub fn room_at(&self, p: [f32; 3]) -> Option<usize> {
        let b = p.map(|v| v.floor() as i32);
        self.rooms.iter().position(|r| r.inner.contains(b[0], b[1], b[2]))
    }

    /// A point (feet) is somewhere inside the floor's open space.
    pub fn inside(&self, p: [f32; 3]) -> bool {
        let b = p.map(|v| v.floor() as i32);
        self.spaces.iter().any(|s| s.contains(b[0], b[1], b[2]))
    }

    pub fn in_box(b: &Aabb, p: [f32; 3]) -> bool {
        let q = p.map(|v| v.floor() as i32);
        b.contains(q[0], q[1], q[2])
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
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }

    fn chance(&mut self, p: f32) -> bool {
        unit(self.next()) < p
    }
}

/// Lay out one floor: a pure function of the seed, the run and the depth (1..=4).
pub fn floor(seed: u64, run: u32, depth: u8) -> Floor {
    let depth = depth.clamp(1, DEPTHS);
    let d = depth as i32;
    let mut dice = Dice(hash2(seed ^ 0x2077_1E6A, run as i32, d));
    let [px, fy, pz] = POCKET;
    // Cells: i runs east (never west of the start, so its west wall stays free
    // for the way out), j north/south.
    let reach = 1 + d;
    let want = (4 + 2 * d) as usize;
    let dirs = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    let mut cells = vec![(0, 0)];
    let mut index: HashMap<(i32, i32), usize> = HashMap::from([((0, 0), 0)]);
    let mut links = Vec::new();
    for _ in 0..400 {
        if cells.len() >= want {
            break;
        }
        let from = dice.int(0, cells.len() as i32 - 1) as usize;
        let (di, dj) = dirs[dice.int(0, 3) as usize];
        let (i, j) = (cells[from].0 + di, cells[from].1 + dj);
        if !(0..=2 * reach).contains(&i) || !(-reach..=reach).contains(&j) || (i == 0 && j != 0) || index.contains_key(&(i, j)) {
            continue;
        }
        index.insert((i, j), cells.len());
        links.push((from, cells.len()));
        cells.push((i, j));
    }
    // A few loops.
    for (a, &(i, j)) in cells.iter().enumerate() {
        for (di, dj) in [(1, 0), (0, 1)] {
            if let Some(&b) = index.get(&(i + di, j + dj))
                && !links.contains(&(a, b))
                && !links.contains(&(b, a))
                && dice.chance(0.2)
            {
                links.push((a, b));
            }
        }
    }
    // Walk distances from the start.
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
    // The boss room: the furthest room with a wall free of corridors (for the stair).
    let free_side = |n: usize| {
        let (i, j) = cells[n];
        dirs.iter().copied().find(|&(di, dj)| !index.contains_key(&(i + di, j + dj)))
    };
    let boss = (1..cells.len()).filter(|&n| free_side(n).is_some()).max_by_key(|&n| (dist[n], n)).unwrap_or(cells.len() - 1);
    let centre = |n: usize| (px + cells[n].0 * GRID, pz + cells[n].1 * GRID);
    let mut rooms = Vec::new();
    let mut halves = Vec::new();
    let cap = GRID / 2 - 3;
    for n in 0..cells.len() {
        let (cx, cz) = centre(n);
        let (kind, hx, hz, h) = if n == 0 {
            (FloorKind::Start, 5, 5, 6)
        } else if n == boss {
            (FloorKind::Boss, (8 + d).min(cap), (8 + d).min(cap), 10)
        } else {
            (FloorKind::Hall, dice.int(4, (5 + d).min(cap)), dice.int(4, (5 + d).min(cap)), dice.int(5, 7))
        };
        halves.push((hx, hz));
        rooms.push(FloorRoom { kind, inner: b_box((cx - hx, cx + hx), (fy, fy + h - 1), (cz - hz, cz + hz)) });
    }
    let mut spaces: Vec<Aabb> = rooms.iter().map(|r| r.inner).collect();
    for &(a, b) in &links {
        let ((ax, az), (bx, bz)) = (centre(a), centre(b));
        let corridor = if az == bz {
            b_box((ax, bx), (fy, fy + HALL_HEIGHT - 1), (az - HALL_HALF, az + HALL_HALF))
        } else {
            b_box((ax - HALL_HALF, ax + HALL_HALF), (fy, fy + HALL_HEIGHT - 1), (az, bz))
        };
        spaces.push(corridor);
    }
    // The way out: a doorway two deep in the start room's west wall.
    let (sx, sz) = centre(0);
    let way_out = b_box((sx - 7, sx - 6), (fy, fy + 2), (sz - 1, sz + 1));
    spaces.push(way_out);
    // The stair: a passage out of the boss room's free wall, stepping down.
    let (bx, bz) = centre(boss);
    let (ddx, ddz) = free_side(boss).unwrap_or((1, 0));
    let (hx, hz) = halves[boss];
    let mut stair_steps = Vec::new();
    for k in 1..=6 {
        let (x, z) = (bx + ddx * (hx + k), bz + ddz * (hz + k));
        let (x, z) = if ddx != 0 { (x, bz) } else { (bx, z) };
        let y = fy - (k - 1) / 2;
        let b = if ddx != 0 { b_box((x, x), (y, y + 3), (z - 1, z + 1)) } else { b_box((x - 1, x + 1), (y, y + 3), (z, z)) };
        stair_steps.push(b);
    }
    let last = *stair_steps.last().unwrap();
    let stair = last.union(&stair_steps[stair_steps.len() - 2]);
    spaces.extend(stair_steps);
    // Foes: packs in the halls, guards in some corridors, the boss and its guards.
    let mut spawns = Vec::new();
    for (n, room) in rooms.iter().enumerate() {
        let (cx, cz) = centre(n);
        let (hx, hz) = halves[n];
        match room.kind {
            FloorKind::Start => {}
            FloorKind::Boss => {
                spawns.push(SpawnPoint { pos: [cx, fy, cz], kind: SpawnKind::Boss, room: n });
                for g in 0..d {
                    let s = if g % 2 == 0 { 1 } else { -1 };
                    spawns.push(SpawnPoint { pos: [cx - 3, fy, cz + s * (3 + g / 2 * 2)], kind: SpawnKind::Guard, room: n });
                }
            }
            FloorKind::Hall => {
                let count = (1 + d + (hx * hz) / 20).min(8);
                let mut tries = 0;
                while spawns.iter().filter(|s: &&SpawnPoint| s.room == n).count() < count as usize && tries < 50 {
                    tries += 1;
                    let pos = [cx + dice.int(-(hx - 1), hx - 1), fy, cz + dice.int(-(hz - 1), hz - 1)];
                    if !spawns.iter().any(|s| s.pos == pos) {
                        spawns.push(SpawnPoint { pos, kind: SpawnKind::Pack, room: n });
                    }
                }
            }
        }
    }
    for &(a, b) in &links {
        if dice.chance(0.25 + 0.1 * d as f32) {
            let ((ax, az), (bx, bz)) = (centre(a), centre(b));
            let room = if dist[b] >= dist[a] { b } else { a };
            spawns.push(SpawnPoint { pos: [(ax + bx) / 2, fy, (az + bz) / 2], kind: SpawnKind::Guard, room });
        }
    }
    // Torches in the corners of every room.
    let mut torches = Vec::new();
    for r in &rooms {
        let (lo, hi) = (r.inner.min, r.inner.max);
        for (x, z) in [(lo[0], lo[2]), (hi[0], lo[2]), (lo[0], hi[2]), (hi[0], hi[2])] {
            torches.push([x, fy, z]);
        }
    }
    let bounds = spaces.iter().fold(spaces[0], |acc, s| acc.union(&s.grow(1)));
    Floor {
        run,
        depth,
        rooms,
        links,
        spawns,
        spaces,
        bounds,
        entry: [sx as f32 + 0.5, fy as f32, sz as f32 + 0.5],
        way_out,
        stair,
        boss_room: boss,
        floor_y: fy,
        torches,
    }
}

fn b_box(x: (i32, i32), y: (i32, i32), z: (i32, i32)) -> Aabb {
    Aabb { min: [x.0.min(x.1), y.0, z.0.min(z.1)], max: [x.0.max(x.1), y.1, z.0.max(z.1)] }
}

/// Fill a box, clipped to the canvas's chunk.
fn fill(c: &mut Canvas, b: &Aabb, block: BlockId) {
    let lo = c.origin;
    let hi = lo.map(|v| v + CHUNK_SIZE_I32 - 1);
    for y in b.min[1].max(lo[1])..=b.max[1].min(hi[1]) {
        for z in b.min[2].max(lo[2])..=b.max[2].min(hi[2]) {
            for x in b.min[0].max(lo[0])..=b.max[0].min(hi[0]) {
                c.put(x, y, z, block);
            }
        }
    }
}

impl WorldGen {
    /// The Rotting Gate (one per world), if the island has room for it.
    pub fn rotting_gate(&self) -> Option<&RottingGate> {
        self.rotting_gate.as_ref()
    }

    /// Level, dry ground on the main island 300-600 blocks from spawn, clear
    /// of villages and dungeons.
    pub(crate) fn search_rotting_gate(&self) -> Option<RottingGate> {
        let [sx, _, sz] = self.spawn;
        for ring in 0..11 {
            let r = 300.0 + ring as f32 * 30.0;
            for s in 0..32 {
                let a = s as f32 / 32.0 * std::f32::consts::TAU + ring as f32 * 0.7 + unit(self.seed) * 6.0;
                let (x, z) = (sx + (a.cos() * r) as i32, sz + (a.sin() * r) as i32);
                if let Some(g) = self.build_rotting_gate(x, z) {
                    return Some(g);
                }
            }
        }
        None
    }

    fn build_rotting_gate(&self, x: i32, z: i32) -> Option<RottingGate> {
        let col = self.column(x, z);
        if col.land != Land::Main || col.height < SEA_LEVEL + 1 || col.height < col.water || col.climate.overhang > 0.02 {
            return None;
        }
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for dz in -5..=5 {
            for dx in -5..=5 {
                let c = self.column(x + dx, z + dz);
                if c.land != Land::Main || c.height < c.water {
                    return None;
                }
                lo = lo.min(c.height);
                hi = hi.max(c.height);
            }
        }
        if hi - lo > 3 {
            return None;
        }
        let g = col.height + 1;
        let bounds = b_box((x - 5, x + 5), (lo - 1, g + 6), (z - 5, z + 5));
        let near = |b: &Aabb| b.intersects(&bounds.grow(6));
        if self.villages_in(x - 40, z - 40, x + 40, z + 40).iter().any(|v| {
            let (dx, dz) = (x - v.center[0], z - v.center[2]);
            dx * dx + dz * dz < (v.radius + 12) * (v.radius + 12)
        }) || self.dungeons_in(x - 8, z - 8, x + 8, z + 8).iter().any(|d| near(&d.bounds))
        {
            return None;
        }
        // Face back towards spawn.
        let (tx, tz) = (self.spawn[0] - x, self.spawn[2] - z);
        let out = if tx.abs() > tz.abs() { (tx.signum(), 0) } else { (0, tz.signum()) };
        let out = if out == (0, 0) { (1, 0) } else { out };
        // The chamber inside the arch: walk to its back half to go down.
        let (bx, bz) = (x - out.0, z - out.1);
        let door = if out.0 != 0 { b_box((bx, bx - out.0), (g, g + 2), (z - 1, z + 1)) } else { b_box((x - 1, x + 1), (g, g + 2), (bz, bz - out.1)) };
        let outside = [(x + out.0 * 6) as f32 + 0.5, g as f32, (z + out.1 * 6) as f32 + 0.5];
        Some(RottingGate { pos: [x, g, z], out, door, outside, bounds })
    }

    /// Generate a chunk in the pocket for one floor (the game calls this
    /// instead of `generate` for chunks where `in_pocket` holds).
    pub fn generate_pocket(&self, pos: ChunkPos, f: &Floor) -> Chunk {
        let origin = pos.origin();
        let chunk = Aabb { min: [origin.x, origin.y, origin.z], max: [origin.x + CHUNK_SIZE_I32 - 1, origin.y + CHUNK_SIZE_I32 - 1, origin.z + CHUNK_SIZE_I32 - 1] };
        if !chunk.intersects(&f.bounds) {
            return Chunk::filled(BlockId::AIR);
        }
        let ids = &self.ids;
        let mut dense = vec![BlockId::AIR; CHUNK_VOLUME];
        let mut c = Canvas { origin: chunk.min, dense: &mut dense };
        // Shells of old stone with a cobbled floor, then the air carved out.
        for s in &f.spaces {
            let shell = s.grow(1);
            if !shell.intersects(&chunk) {
                continue;
            }
            fill(&mut c, &shell, ids.deeprock);
        }
        for s in &f.spaces {
            fill(&mut c, &Aabb { min: [s.min[0], s.min[1] - 1, s.min[2]], max: [s.max[0], s.min[1] - 1, s.max[2]] }, ids.cobblestone);
        }
        for s in &f.spaces {
            fill(&mut c, s, BlockId::AIR);
        }
        // Mold: moss and mud creep over the walls, ceilings and floors.
        let seed = self.seed ^ (f.run as u64) << 8 ^ f.depth as u64;
        for y in chunk.min[1]..=chunk.max[1] {
            for z in chunk.min[2]..=chunk.max[2] {
                for x in chunk.min[0]..=chunk.max[0] {
                    if c.is_air(x, y, z) != Some(false) {
                        continue;
                    }
                    let h = hash2(seed ^ (y as u64).wrapping_mul(0x9E37), x, z);
                    let u = unit(h);
                    if u < 0.18 + 0.03 * f.depth as f32 {
                        c.put(x, y, z, ids.moss);
                    } else if u < 0.26 + 0.03 * f.depth as f32 {
                        c.put(x, y, z, ids.mud);
                    }
                }
            }
        }
        for t in &f.torches {
            c.put(t[0], t[1], t[2], ids.torch);
        }
        Chunk::from_dense(&dense)
    }
}

/// Draw the gate where it falls in this chunk.
pub(crate) fn draw_gate(wg: &WorldGen, c: &mut Canvas, g: &RottingGate) {
    let ids = &wg.ids;
    let [x, y, z] = g.pos;
    // A foundation down to the ground, then a squat mossy arch.
    for dz in -4..=4 {
        for dx in -4..=4 {
            let ground = wg.column(x + dx, z + dz).height;
            for yy in ground.min(y - 2)..y {
                c.put(x + dx, yy, z + dz, ids.cobblestone);
            }
        }
    }
    let shell = b_box((x - 3, x + 3), (y, y + 4), (z - 3, z + 3));
    fill(c, &shell, ids.cobblestone);
    // Hollow chamber and the doorway out the front.
    fill(c, &b_box((x - 1, x + 1), (y, y + 2), (z - 1, z + 1)), BlockId::AIR);
    let (ox, oz) = g.out;
    for k in 2..=3 {
        let (dx, dz) = (x + ox * k, z + oz * k);
        let b = if ox != 0 { b_box((dx, dx), (y, y + 2), (z - 1, z + 1)) } else { b_box((x - 1, x + 1), (y, y + 2), (dz, dz)) };
        fill(c, &b, BlockId::AIR);
    }
    // A dark hole in the floor at the back, rimmed in deeprock.
    fill(c, &b_box((x - 1, x + 1), (y - 1, y - 1), (z - 1, z + 1)), ids.deeprock);
    // Mold over everything: moss on the roof and running down the walls.
    fill(c, &b_box((x - 4, x + 4), (y + 5, y + 5), (z - 4, z + 4)), ids.moss);
    fill(c, &b_box((x - 2, x + 2), (y + 6, y + 6), (z - 2, z + 2)), ids.moss);
    for dz in -3..=3 {
        for dx in -3..=3 {
            for yy in y..=y + 4 {
                if c.is_air(x + dx, yy, z + dz) == Some(false) && unit(hash2(wg.seed ^ 0x601D ^ yy as u64, x + dx, z + dz)) < 0.4 {
                    c.put(x + dx, yy, z + dz, if yy % 2 == 0 { ids.moss } else { ids.mud });
                }
            }
        }
    }
    // Two torches flank the doorway.
    let (sx, sz) = (-oz, ox);
    for s in [-2, 2] {
        c.put(x + ox * 4 + sx * s, y, z + oz * 4 + sz * s, ids.torch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floors_are_pure_and_change_per_run() {
        assert_eq!(floor(7, 1, 1), floor(7, 1, 1));
        assert_ne!(floor(7, 1, 1).rooms, floor(7, 2, 1).rooms);
        assert_ne!(floor(7, 1, 1).rooms, floor(8, 1, 1).rooms);
    }

    #[test]
    fn deeper_floors_are_bigger_and_harder() {
        for run in 0..20 {
            let f: Vec<Floor> = (1..=DEPTHS).map(|d| floor(99, run, d)).collect();
            for w in f.windows(2) {
                assert!(w[1].rooms.len() > w[0].rooms.len(), "run {run}");
                assert!(w[1].spawns.len() > w[0].spawns.len(), "run {run}");
            }
        }
    }

    #[test]
    fn every_floor_has_its_parts_and_they_connect() {
        for run in 0..40 {
            for d in 1..=DEPTHS {
                let f = floor(3, run, d);
                assert_eq!(f.rooms[0].kind, FloorKind::Start);
                assert_eq!(f.rooms.iter().filter(|r| r.kind == FloorKind::Boss).count(), 1);
                assert_eq!(f.rooms[f.boss_room].kind, FloorKind::Boss);
                assert_eq!(f.spawns.iter().filter(|s| s.kind == SpawnKind::Boss).count(), 1);
                assert!(f.spawns.iter().all(|s| s.room != 0), "nothing waits in the start room");
                assert_eq!(f.room_at(f.entry), Some(0));
                assert!(f.inside(f.entry));
                // Every room is reached from the start.
                let mut seen = vec![false; f.rooms.len()];
                seen[0] = true;
                let mut stack = vec![0];
                while let Some(a) = stack.pop() {
                    for &(p, q) in &f.links {
                        let b = if p == a { q } else if q == a { p } else { continue };
                        if !seen[b] {
                            seen[b] = true;
                            stack.push(b);
                        }
                    }
                }
                assert!(seen.iter().all(|s| *s), "run {run} depth {d}");
                // The way out and the stair open off their rooms.
                assert!(f.way_out.grow(1).intersects(&f.rooms[0].inner));
                assert!(f.stair.min[1] < f.floor_y, "the stair goes down");
                assert!(f.bounds.min[0] >= POCKET[0] - POCKET_HALF && f.bounds.max[0] <= POCKET[0] + POCKET_HALF);
                assert!(f.bounds.max[1] < crate::WORLD_CEILING && f.bounds.min[1] > crate::SKY_MAX);
            }
        }
    }

    #[test]
    fn spawns_stand_on_open_floor() {
        let f = floor(11, 4, 3);
        for s in &f.spawns {
            assert!(f.inside([s.pos[0] as f32 + 0.5, s.pos[1] as f32, s.pos[2] as f32 + 0.5]), "{s:?}");
        }
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;

    #[test]
    fn the_gate_stands_300_to_600_blocks_out_and_draws() {
        let wg = crate::tests::wg();
        let g = wg.rotting_gate().expect("a gate in the default world");
        let [sx, _, sz] = wg.spawn;
        let d = (((g.pos[0] - sx).pow(2) + (g.pos[2] - sz).pow(2)) as f32).sqrt();
        assert!((290.0..=610.0).contains(&d), "{d}");
        assert!(wg.warded(g.pos[0], g.pos[1] + 4, g.pos[2]));
        assert!(g.door.contains(g.door.min[0], g.door.min[1], g.door.min[2]));
        // The arch is built in the chunk around it, with its chamber open.
        let c = wg.generate(voxel::BlockPos::new(g.pos[0], g.pos[1] + 4, g.pos[2]).chunk());
        let any_stone = (0..CHUNK_VOLUME).any(|i| c.to_dense()[i] == wg.ids.cobblestone || c.to_dense()[i] == wg.ids.moss);
        assert!(any_stone);
        assert!(wg.generate(voxel::BlockPos::new(POCKET[0], POCKET[1], POCKET[2]).chunk()).to_dense().iter().all(|b| b.is_air()), "the pocket is empty sky");
    }

    #[test]
    fn pocket_chunks_build_the_floor() {
        let wg = crate::tests::wg();
        let f = floor(42, 1, 1);
        let [x, y, z] = f.entry.map(|v| v.floor() as i32);
        let c = wg.generate_pocket(voxel::BlockPos::new(x, y, z).chunk(), &f);
        let o = voxel::BlockPos::new(x, y, z).chunk().origin();
        let at = |x: i32, y: i32, z: i32| c.get((x - o.x) as usize, (y - o.y) as usize, (z - o.z) as usize);
        assert!(at(x, y, z).is_air() && at(x, y + 1, z).is_air(), "room to stand");
        assert!(!at(x, y - 1, z).is_air(), "a floor under you");
    }
}
