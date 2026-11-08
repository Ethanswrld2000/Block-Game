//! Built places: stilt villages and the old barrows under the ground.
//!
//! Villages sit one to a 640-block cell (when the land allows), with a home
//! village always a short walk from spawn. Barrows sit one to a 448-block cell,
//! a stair down from a ring of stones into a stone chamber with side rooms.
//! Like trees, each chunk draws the parts of nearby structures that fall in
//! it, from the same seed, so the halves line up. Names come from syllables.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use voxel::{BlockId, ChunkPos, CHUNK_SIZE_I32};

use crate::trees::Canvas;
use crate::util::{hash2, unit};
use crate::{WorldGen, SEA_LEVEL};

const VILLAGE_CELL: i32 = 640;
const VILLAGE_CHANCE: f32 = 0.55;
/// Huts and plaza fit inside this radius around the centre.
pub const VILLAGE_RADIUS: i32 = 30;
const BARROW_CELL: i32 = 448;
const BARROW_CHANCE: f32 = 0.6;

#[derive(Debug, Clone, PartialEq)]
pub struct Hut {
    /// Centre of the 5x5 footprint.
    pub x: i32,
    pub z: i32,
    /// Ground under the centre and the floor (planks) height.
    pub ground: i32,
    pub floor: i32,
    /// Which way the door faces: a unit step in x or z.
    pub door: (i32, i32),
}

impl Hut {
    /// Standing spot just outside the door, on the ground at the foot of the steps.
    pub fn doorstep(&self) -> (i32, i32) {
        let steps = (self.floor - self.ground).max(1) + 2;
        (self.x + self.door.0 * steps, self.z + self.door.1 * steps)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Village {
    /// Stable id (from the cell), for remembering who lives here.
    pub id: u64,
    pub name: String,
    /// Centre of the plaza, on the ground.
    pub center: [i32; 3],
    pub radius: i32,
    pub huts: Vec<Hut>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Barrow {
    pub id: u64,
    pub name: String,
    /// Where the stair starts, at the surface.
    pub entrance: [i32; 3],
    /// Floor centre of the main chamber.
    pub hall: [i32; 3],
    /// Inclusive bounds of everything underground (for "you are inside").
    pub min: [i32; 3],
    pub max: [i32; 3],
}

impl Barrow {
    pub fn contains(&self, x: i32, y: i32, z: i32) -> bool {
        (self.min[0]..=self.max[0]).contains(&x) && (self.min[1]..=self.max[1]).contains(&y) && (self.min[2]..=self.max[2]).contains(&z)
    }
}

type CellCache<T> = Mutex<HashMap<(i32, i32), Option<Arc<T>>>>;

/// Per-cell results, so neighbouring chunks don't redo the work.
#[derive(Default)]
pub(crate) struct Cache {
    villages: CellCache<Village>,
    barrows: CellCache<Barrow>,
}

fn cached<T>(map: &CellCache<T>, key: (i32, i32), make: impl FnOnce() -> Option<T>) -> Option<Arc<T>> {
    if let Some(v) = map.lock().unwrap().get(&key) {
        return v.clone();
    }
    let v = make().map(Arc::new);
    let mut m = map.lock().unwrap();
    if m.len() > 4096 {
        m.clear();
    }
    m.insert(key, v.clone());
    v
}

const VILLAGE_FIRST: [&str; 14] =
    ["Reed", "Mire", "Fen", "Moss", "Heron", "Bog", "Willow", "Peat", "Sedge", "Otter", "Alder", "Rush", "Cran", "Teal"];
const VILLAGE_LAST: [&str; 12] = ["mere", "wick", "holm", "stead", "ford", "by", "watch", "hollow", "marsh", "well", "stilts", "landing"];
const BARROW_KIN: [&str; 10] = ["Ama", "Brannoc", "Cael", "Dunmora", "Elsa", "Fenwyr", "Gorran", "Hild", "Ithel", "Morwen"];
const BARROW_KIND: [&str; 6] = ["Barrow of", "Sleep of", "Long House of", "Hollow of", "Mound of", "Rest of"];
const PERSON_FIRST: [&str; 12] = ["Ae", "Bra", "Cor", "Dun", "El", "Fen", "Gor", "Hal", "Ith", "Mor", "Ny", "Os"];
const PERSON_LAST: [&str; 10] = ["wyn", "dric", "ga", "mund", "ra", "thel", "wen", "ric", "la", "dan"];

fn pick<'a>(list: &[&'a str], h: u64) -> &'a str {
    list[(h % list.len() as u64) as usize]
}

/// A villager's name, from a seed.
pub fn person_name(h: u64) -> String {
    format!("{}{}", pick(&PERSON_FIRST, h >> 7), pick(&PERSON_LAST, h >> 19))
}

impl WorldGen {
    /// Villages whose area touches the rectangle (inclusive block coordinates).
    pub fn villages_in(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> Vec<Arc<Village>> {
        let r = VILLAGE_RADIUS + 2;
        let mut out = Vec::new();
        for cz in (z0 - r).div_euclid(VILLAGE_CELL)..=(z1 + r).div_euclid(VILLAGE_CELL) {
            for cx in (x0 - r).div_euclid(VILLAGE_CELL)..=(x1 + r).div_euclid(VILLAGE_CELL) {
                let Some((x, z)) = self.village_center(cx, cz) else { continue };
                if x + r < x0 || x - r > x1 || z + r < z0 || z - r > z1 {
                    continue;
                }
                if let Some(v) = cached(&self.structures.villages, (cx, cz), || self.build_village(cx, cz, x, z)) {
                    out.push(v);
                }
            }
        }
        out
    }

    /// The village a short walk from spawn.
    pub fn home_village(&self) -> Option<Arc<Village>> {
        let (x, z) = self.home;
        self.villages_in(x, z, x, z).into_iter().next()
    }

    /// Barrows whose underground area touches the rectangle.
    pub fn barrows_in(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> Vec<Arc<Barrow>> {
        let r = 40;
        let mut out = Vec::new();
        for cz in (z0 - r).div_euclid(BARROW_CELL)..=(z1 + r).div_euclid(BARROW_CELL) {
            for cx in (x0 - r).div_euclid(BARROW_CELL)..=(x1 + r).div_euclid(BARROW_CELL) {
                let h = hash2(self.seed ^ 0xBA22, cx, cz);
                if unit(h) >= BARROW_CHANCE {
                    continue;
                }
                let x = cx * BARROW_CELL + 64 + (h >> 8) as i32 % (BARROW_CELL - 128);
                let z = cz * BARROW_CELL + 64 + (h >> 24) as i32 % (BARROW_CELL - 128);
                if x + r < x0 || x - r > x1 || z + r < z0 || z - r > z1 {
                    continue;
                }
                if let Some(b) = cached(&self.structures.barrows, (cx, cz), || self.build_barrow(h, x, z)) {
                    out.push(b);
                }
            }
        }
        out
    }

    /// Where the village in a cell would stand, before checking the land.
    fn village_center(&self, cx: i32, cz: i32) -> Option<(i32, i32)> {
        let home = (self.home.0.div_euclid(VILLAGE_CELL), self.home.1.div_euclid(VILLAGE_CELL));
        if (cx, cz) == home {
            return Some(self.home);
        }
        // Leave room around the home village.
        if (cx - home.0).abs() <= 1 && (cz - home.1).abs() <= 1 {
            return None;
        }
        let h = hash2(self.seed ^ 0x7111, cx, cz);
        if unit(h) >= VILLAGE_CHANCE {
            return None;
        }
        let span = VILLAGE_CELL - 2 * 96;
        Some((cx * VILLAGE_CELL + 96 + (h >> 8) as i32 % span, cz * VILLAGE_CELL + 96 + (h >> 24) as i32 % span))
    }

    fn build_village(&self, cx: i32, cz: i32, x: i32, z: i32) -> Option<Village> {
        let col = self.column(x, z);
        let biome = &self.biomes.get(col.biome).name;
        if col.height < SEA_LEVEL - 2 || col.climate.overhang > 0.02 || matches!(biome.as_str(), "deep_sea" | "frost_peaks" | "highlands" | "skyreach") {
            return None;
        }
        let h = hash2(self.seed ^ 0x7112, cx, cz);
        let name = format!("{}{}", pick(&VILLAGE_FIRST, h >> 3), pick(&VILLAGE_LAST, h >> 17));
        let n = 4 + (h >> 29) as usize % 4;
        let turn = unit(h >> 5) * std::f32::consts::TAU;
        let mut huts = Vec::new();
        for i in 0..n {
            let a = turn + i as f32 / n as f32 * std::f32::consts::TAU;
            let r = 14.0 + ((h >> (33 + i * 3)) % 6) as f32;
            let (hx, hz) = (x + (a.cos() * r).round() as i32, z + (a.sin() * r).round() as i32);
            let mut ground = i32::MIN;
            let mut low = i32::MAX;
            for dz in -2..=2 {
                for dx in -2..=2 {
                    let g = self.column(hx + dx, hz + dz).height;
                    ground = ground.max(g);
                    low = low.min(g);
                }
            }
            // A hut on a cliff edge would float: skip it.
            if ground - low > 4 {
                continue;
            }
            let center_ground = self.column(hx, hz).height;
            let floor = ground.max(SEA_LEVEL) + 2;
            let (dx, dz) = (x - hx, z - hz);
            let door = if dx.abs() > dz.abs() { (dx.signum(), 0) } else { (0, dz.signum().max(-1)) };
            let door = if door == (0, 0) { (1, 0) } else { door };
            huts.push(Hut { x: hx, z: hz, ground: center_ground, floor, door });
        }
        if huts.len() < 3 {
            return None;
        }
        Some(Village { id: h, name, center: [x, col.height.max(SEA_LEVEL), z], radius: VILLAGE_RADIUS, huts })
    }

    fn build_barrow(&self, h: u64, x: i32, z: i32) -> Option<Barrow> {
        let col = self.column(x, z);
        if col.height < SEA_LEVEL + 1 || col.climate.overhang > 0.02 {
            return None;
        }
        let depth = 14 + (h >> 40) as i32 % 8;
        let surface = col.height;
        // The stair runs +x from the entrance, one step down per block.
        let hall = [x + depth + 7, surface - depth, z];
        let name = format!("The {} {}", pick(&BARROW_KIND, h >> 11), pick(&BARROW_KIN, h >> 23));
        Some(Barrow {
            id: h,
            name,
            entrance: [x, surface + 1, z],
            hall,
            min: [x - 1, hall[1] - 1, z - 14],
            max: [hall[0] + 14, surface + 1, z + 14],
        })
    }
}

/// Draw the parts of villages and barrows that fall in this chunk.
pub(crate) fn place(wg: &WorldGen, pos: ChunkPos, dense: &mut [BlockId]) {
    let o = pos.origin();
    let (x0, z0, x1, z1) = (o.x, o.z, o.x + CHUNK_SIZE_I32 - 1, o.z + CHUNK_SIZE_I32 - 1);
    let mut canvas = Canvas { origin: [o.x, o.y, o.z], dense };
    let (lo, hi) = (o.y, o.y + CHUNK_SIZE_I32 - 1);
    for v in wg.villages_in(x0, z0, x1, z1) {
        let top = v.huts.iter().map(|h| h.floor + 6).max().unwrap_or(0).max(v.center[1] + 4);
        let bottom = v.huts.iter().map(|h| h.ground).min().unwrap_or(0).min(v.center[1]) - 2;
        if top < lo || bottom > hi {
            continue;
        }
        draw_village(wg, &mut canvas, &v);
    }
    for b in wg.barrows_in(x0, z0, x1, z1) {
        if b.max[1] + 2 < lo || b.min[1] > hi {
            continue;
        }
        draw_barrow(wg, &mut canvas, &b);
    }
}

/// Is this column inside any village (trees keep out)?
pub(crate) fn in_village(villages: &[Arc<Village>], x: i32, z: i32) -> bool {
    villages.iter().any(|v| {
        let (dx, dz) = (x - v.center[0], z - v.center[2]);
        dx * dx + dz * dz <= (v.radius + 3) * (v.radius + 3)
    })
}

fn draw_village(wg: &WorldGen, c: &mut Canvas, v: &Village) {
    let ids = &wg.ids;
    let [cx, cy, cz] = v.center;
    // Plaza: a stone fire ring with a peat fire, four torch posts.
    for dz in -1..=1 {
        for dx in -1..=1 {
            let b = if dx == 0 && dz == 0 { ids.peat } else { ids.cobblestone };
            c.put(cx + dx, cy, cz + dz, b);
            for y in cy + 1..cy + 4 {
                c.put(cx + dx, y, cz + dz, BlockId::AIR);
            }
        }
    }
    for (dx, dz) in [(4, 4), (-4, 4), (4, -4), (-4, -4)] {
        let g = wg.column(cx + dx, cz + dz).height.max(SEA_LEVEL);
        for y in g + 1..=g + 2 {
            c.put(cx + dx, y, cz + dz, ids.log);
        }
        c.put(cx + dx, g + 3, cz + dz, ids.torch);
    }
    for (i, h) in v.huts.iter().enumerate() {
        draw_hut(wg, c, h, i == 0);
    }
}

fn draw_hut(wg: &WorldGen, c: &mut Canvas, h: &Hut, elder: bool) {
    let ids = &wg.ids;
    let f = h.floor;
    // Stilts down to the ground (or the swamp bed).
    for (dx, dz) in [(2, 2), (-2, 2), (2, -2), (-2, -2)] {
        let g = wg.column(h.x + dx, h.z + dz).height;
        for y in g + 1..f {
            c.put(h.x + dx, y, h.z + dz, ids.swamp_log);
        }
    }
    for dz in -2..=2i32 {
        for dx in -2..=2i32 {
            let edge = dx.abs() == 2 || dz.abs() == 2;
            let corner = dx.abs() == 2 && dz.abs() == 2;
            c.put(h.x + dx, f, h.z + dz, ids.planks);
            for y in f + 1..=f + 3 {
                let b = if corner {
                    ids.log
                } else if edge {
                    // Door: the middle of the side facing the plaza, two high.
                    let door = (dx, dz) == (h.door.0 * 2, h.door.1 * 2) && y <= f + 2;
                    // Windows: middle of the other sides, at eye height.
                    let window = (dx == 0 || dz == 0) && y == f + 2;
                    if door || window { BlockId::AIR } else { ids.planks }
                } else {
                    BlockId::AIR
                };
                c.put(h.x + dx, y, h.z + dz, b);
            }
        }
    }
    // Roof: an overhanging layer and a ridge.
    for dz in -3..=3i32 {
        for dx in -3..=3i32 {
            c.put(h.x + dx, f + 4, h.z + dz, ids.swamp_log);
            if dx.abs() <= 1 && dz.abs() <= 1 {
                c.put(h.x + dx, f + 5, h.z + dz, ids.swamp_leaves);
            }
        }
    }
    // Steps from the door down to the ground.
    let (sx, sz) = h.door;
    let mut y = f;
    let mut k = 3;
    while y > h.ground && k < 12 {
        let (x, z) = (h.x + sx * k, h.z + sz * k);
        c.put(x, y, z, ids.planks);
        for clear in y + 1..y + 4 {
            c.put(x, clear, z, BlockId::AIR);
        }
        y -= 1;
        k += 1;
    }
    // A torch inside, and the elder keeps a chest.
    c.put(h.x - sz - sx, f + 1, h.z - sx - sz, ids.torch);
    if elder {
        c.put(h.x + sz - sx, f + 1, h.z + sx - sz, ids.chest);
    }
}

fn draw_barrow(wg: &WorldGen, c: &mut Canvas, b: &Barrow) {
    let ids = &wg.ids;
    let [ex, ey, ez] = b.entrance;
    // A ring of standing stones around the way down.
    for (dx, dz) in [(-3, -3), (-3, 3), (3, -3), (3, 3), (-4, 0), (0, -4), (0, 4)] {
        let g = wg.column(ex + dx, ez + dz).height;
        for y in g + 1..=g + 2 {
            c.put(ex + dx, y, ez + dz, ids.cobblestone);
        }
    }
    // The stair: two wide, three high, one step down per block along +x.
    let [hx, hy, hz] = b.hall;
    let mut y = ey - 1;
    let mut x = ex;
    while y > hy {
        for dz in 0..=1 {
            c.put(x, y - 1, ez + dz, ids.cobblestone);
            for clear in y..y + 3 {
                c.put(x, clear, ez + dz, BlockId::AIR);
            }
        }
        x += 1;
        y -= 1;
    }
    // A short corridor at the bottom to the hall.
    while x < hx - 5 {
        for dz in 0..=1 {
            c.put(x, hy - 1, ez + dz, ids.cobblestone);
            for clear in hy..hy + 3 {
                c.put(x, clear, ez + dz, BlockId::AIR);
            }
        }
        x += 1;
    }
    // The hall: 11 x 11, five high, stone walls, deeprock floor, log pillars.
    room(c, ids.cobblestone, ids.deeprock, hx, hy, hz, 5, 5);
    for (dx, dz) in [(-3, -3), (-3, 3), (3, -3), (3, 3)] {
        for y in hy..hy + 5 {
            c.put(hx + dx, y, hz + dz, ids.log);
        }
    }
    // Side rooms north and south, through short passages.
    for side in [-1, 1] {
        let rz = hz + side * 10;
        room(c, ids.cobblestone, ids.deeprock, hx, hy, rz, 2, 4);
        for k in 6..8 {
            for y in hy..hy + 3 {
                c.put(hx, y, hz + side * k, BlockId::AIR);
            }
        }
        c.put(hx + 1, hy, rz + side, ids.chest);
    }
    // The hall's grave goods and two torches.
    c.put(hx + 4, hy, hz, ids.chest);
    c.put(hx - 4, hy, hz - 4, ids.torch);
    c.put(hx - 4, hy, hz + 4, ids.torch);
}

/// A hollow room centred at (x, z) with its floor surface at y: walls `wall`,
/// floor `floor`, inner half-width `r`, inner height `height`.
#[allow(clippy::too_many_arguments)]
fn room(c: &mut Canvas, wall: BlockId, floor: BlockId, x: i32, y: i32, z: i32, r: i32, height: i32) {
    for dz in -r - 1..=r + 1 {
        for dx in -r - 1..=r + 1 {
            let inside = dx.abs() <= r && dz.abs() <= r;
            c.put(x + dx, y - 1, z + dz, floor);
            c.put(x + dx, y + height, z + dz, wall);
            for yy in y..y + height {
                c.put(x + dx, yy, z + dz, if inside { BlockId::AIR } else { wall });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::wg;

    #[test]
    fn home_village_is_near_spawn_and_named() {
        let g = wg();
        let [sx, _, sz] = g.find_spawn();
        let v = g.home_village().expect("a home village");
        let d = (((v.center[0] - sx).pow(2) + (v.center[2] - sz).pow(2)) as f32).sqrt();
        assert!(d < 140.0, "home village {} is {d} blocks from spawn", v.name);
        eprintln!("home village {} at {:?}, spawn {sx},{sz}, huts {:?}", v.name, v.center, v.huts.iter().map(|h| (h.x, h.floor, h.z)).collect::<Vec<_>>());
        assert!(v.huts.len() >= 3 && !v.name.is_empty());
        // Deterministic.
        assert_eq!(g.home_village().unwrap().name, v.name);
    }

    #[test]
    fn villages_and_barrows_are_scattered() {
        let g = wg();
        let villages = g.villages_in(-4000, -4000, 4000, 4000);
        let barrows = g.barrows_in(-4000, -4000, 4000, 4000);
        assert!(villages.len() >= 10, "{} villages", villages.len());
        assert!(barrows.len() >= 20, "{} barrows", barrows.len());
        let mut names: Vec<_> = villages.iter().map(|v| v.name.clone()).collect();
        names.sort();
        names.dedup();
        assert!(names.len() >= villages.len() / 2);
    }

    #[test]
    fn huts_are_built_in_the_chunks() {
        let g = wg();
        let v = g.home_village().unwrap();
        let h = &v.huts[0];
        let p = voxel::BlockPos::new(h.x, h.floor, h.z);
        let chunk = g.generate(p.chunk());
        let local = p.local();
        assert_eq!(chunk.get(local[0], local[1], local[2]), g.ids.planks, "hut floor at {:?}", p);
    }
}
