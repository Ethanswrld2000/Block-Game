//! One-of-a-kind places, each placed once per world (pure):
//!
//! - **Sporrow**: a small village of giant mushroom houses 60-110 blocks from
//!   the Rotting Gate, home of the mushroom folk. It is a `Village` with
//!   `folk: Folk::Mushroom`, so `villages_in` lists it with the others. No
//!   torches: they fear fire.
//! - **Gilm's hut**: a lone stone hut with a forge just outside Sporrow.
//! - **The castle**: an abandoned castle high in the mountains, a walled
//!   court on a stone plinth with a tower in one corner. A stair winds up
//!   inside the tower to its roof, where the Star Urn stands.
//! - **The aunt's island**: a sky island of its own above the cloud line, with
//!   her stone cottage (chimney on the back wall) and a broken arch facing its door.

use crate::dungeons::Aabb;
use crate::structures::{Folk, Hut, Village};
use crate::trees::Canvas;
use crate::util::{hash2, unit};
use crate::{Land, WorldGen, SEA_LEVEL};
use voxel::BlockId;

/// Sporrow's houses and plaza fit in this radius.
pub const SPORROW_RADIUS: i32 = 22;

/// Gilm's hut, just outside Sporrow.
#[derive(Debug, Clone, PartialEq)]
pub struct Forge {
    /// Centre of its 5x5 footprint, floor level (the ground it stands on + 1).
    pub pos: [i32; 3],
    /// Which way the door faces (a unit step in x or z).
    pub door: (i32, i32),
    /// Where Gilm stands at first (feet), just outside the door.
    pub stand: [f32; 3],
    pub bounds: Aabb,
}

/// The abandoned castle in the mountains.
#[derive(Debug, Clone, PartialEq)]
pub struct Castle {
    /// Centre of the court, on its floor (blocks stand on `floor`, feet at `floor + 1`).
    pub center: [i32; 3],
    pub floor: i32,
    /// Centre column of the tower.
    pub tower: (i32, i32),
    /// Feet height on the tower's roof.
    pub roof: i32,
    /// Where the Star Urn stands (middle of its foot) and where Orrin waits (feet).
    pub urn: [f32; 3],
    pub orrin: [f32; 3],
    pub bounds: Aabb,
}

/// The aunt's island, high above the clouds over the main island.
#[derive(Debug, Clone, PartialEq)]
pub struct AuntIsland {
    /// Centre of the island and the height its top is shaped around.
    pub center: [i32; 3],
    pub radius: f32,
    /// Centre of the cottage's footprint, floor level (feet height inside).
    pub cottage: [i32; 3],
    /// Which way the cottage door faces (a unit step in x or z).
    pub door: (i32, i32),
    /// Middle of the broken arch, on the ground in front of the door.
    pub arch: [i32; 3],
    /// Where the aunt stands at first (feet), just outside her door.
    pub stand: [f32; 3],
    pub bounds: Aabb,
}

/// The aunt's island: this radius, its base this far above `SKY_MIN`, this far from spawn.
pub const AUNT_RADIUS: f32 = 34.0;
const AUNT_BASE: i32 = 40;
const AUNT_FROM_SPAWN: (f32, f32) = (700.0, 1300.0);
/// The cottage is 7 x 7, walls this tall.
const COTTAGE_HALF: i32 = 3;
const COTTAGE_WALL: i32 = 4;
/// The arch stands this far in front of the door.
const ARCH_AHEAD: i32 = 13;

impl AuntIsland {
    pub(crate) fn island(&self) -> crate::islands::Island {
        crate::islands::Island { x: self.center[0], y: self.center[1], z: self.center[2], radius: self.radius, aunt: true }
    }

    /// No trees here: round the cottage, the path and the arch.
    pub(crate) fn keeps_clear(&self, x: i32, z: i32) -> bool {
        let near = |p: [i32; 3], r: i32| (x - p[0]).pow(2) + (z - p[2]).pow(2) <= r * r;
        let (dx, dz) = (x - self.cottage[0], z - self.cottage[2]);
        let along = dx * self.door.0 + dz * self.door.1;
        let across = (dx * self.door.1 - dz * self.door.0).abs();
        near(self.cottage, 9) || near(self.arch, 6) || ((0..=ARCH_AHEAD).contains(&along) && across <= 3)
    }
}

/// Half the castle's walled court.
const COURT: i32 = 10;
/// The tower: 7 x 7, this tall above the court floor.
const TOWER_HALF: i32 = 3;
const TOWER_HEIGHT: i32 = 20;
/// Steps of the stair inside the tower (one block up each), round the inside wall.
const STAIR: [(i32, i32); 16] =
    [(-2, -2), (-1, -2), (0, -2), (1, -2), (2, -2), (2, -1), (2, 0), (2, 1), (2, 2), (1, 2), (0, 2), (-1, 2), (-2, 2), (-2, 1), (-2, 0), (-2, -1)];

impl WorldGen {
    /// Sporrow, near the Rotting Gate (one per world).
    pub fn sporrow(&self) -> Option<&Village> {
        self.landmarks.sporrow.as_deref()
    }

    /// Gilm's hut just outside Sporrow.
    pub fn gilm_forge(&self) -> Option<&Forge> {
        self.landmarks.forge.as_ref()
    }

    /// The abandoned castle in the mountains (one per world).
    pub fn castle(&self) -> Option<&Castle> {
        self.landmarks.castle.as_ref()
    }

    /// The aunt's island (one per world).
    pub fn aunt_island(&self) -> Option<&AuntIsland> {
        self.landmarks.aunt.as_ref()
    }

    /// A spot above the main island, clear of the other sky islands.
    pub(crate) fn search_aunt_island(&self) -> Option<AuntIsland> {
        let [sx, _, sz] = self.spawn;
        let room = AUNT_RADIUS as i32 + crate::islands::MAX_RADIUS + 24;
        for k in 0..96u64 {
            let h = hash2(self.seed ^ 0xA0_47, k as i32, 7);
            let a = unit(h) * std::f32::consts::TAU;
            let r = AUNT_FROM_SPAWN.0 + unit(h.rotate_left(17)) * (AUNT_FROM_SPAWN.1 - AUNT_FROM_SPAWN.0);
            let (x, z) = (sx + (a.cos() * r) as i32, sz + (a.sin() * r) as i32);
            let cells = (x - room).div_euclid(192)..=(x + room).div_euclid(192);
            let crowded = cells.clone().any(|cx| {
                ((z - room).div_euclid(192)..=(z + room).div_euclid(192)).any(|cz| {
                    crate::islands::island_in_cell(self, cx, cz).is_some_and(|i| {
                        let d = (((i.x - x) as f32).powi(2) + ((i.z - z) as f32).powi(2)).sqrt();
                        d < AUNT_RADIUS + i.radius + 24.0
                    })
                })
            });
            if !crowded {
                return Some(self.build_aunt_island(x, z));
            }
        }
        None
    }

    fn build_aunt_island(&self, x: i32, z: i32) -> AuntIsland {
        let center = [x, crate::SKY_MIN + AUNT_BASE, z];
        let isl = crate::islands::Island { x, y: center[1], z, radius: AUNT_RADIUS, aunt: true };
        // The door faces back towards spawn; the cottage sits a little behind the middle.
        let (tx, tz) = (self.spawn[0] - x, self.spawn[2] - z);
        let door = if tx.abs() > tz.abs() { (tx.signum(), 0) } else { (0, tz.signum()) };
        let (cx, cz) = (x - door.0 * 6, z - door.1 * 6);
        let top = |px: i32, pz: i32| crate::islands::span(self, &isl, px, pz).map_or(center[1], |s| s.1);
        let mut hi = i32::MIN;
        for dz in -COTTAGE_HALF..=COTTAGE_HALF {
            for dx in -COTTAGE_HALF..=COTTAGE_HALF {
                hi = hi.max(top(cx + dx, cz + dz));
            }
        }
        let floor = hi + 1;
        let (ax, az) = (cx + door.0 * ARCH_AHEAD, cz + door.1 * ARCH_AHEAD);
        let arch = [ax, top(ax, az) + 1, az];
        let (fx, fz) = (cx + door.0 * (COTTAGE_HALF + 2), cz + door.1 * (COTTAGE_HALF + 2));
        let stand = [fx as f32 + 0.5, (top(fx, fz) + 1).max(floor - 1) as f32, fz as f32 + 0.5];
        let r = AUNT_RADIUS as i32;
        let bounds = b_box((x - r, x + r), (center[1] - 2 * r, floor + COTTAGE_WALL + 8), (z - r, z + r));
        AuntIsland { center, radius: AUNT_RADIUS, cottage: [cx, floor, cz], door, arch, stand, bounds }
    }

    pub(crate) fn search_sporrow(&self) -> Option<(Village, Forge)> {
        let g = self.rotting_gate()?;
        let [gx, _, gz] = g.pos;
        for ring in 0..6 {
            let r = 60.0 + ring as f32 * 10.0;
            for s in 0..24 {
                let a = s as f32 / 24.0 * std::f32::consts::TAU + ring as f32 * 0.4 + unit(self.seed >> 7) * 6.0;
                let (x, z) = (gx + (a.cos() * r) as i32, gz + (a.sin() * r) as i32);
                if let Some(v) = self.build_sporrow(x, z) {
                    // Gilm's hut: out past the houses, on the side away from the gate.
                    let away = ((x - gx) as f32, (z - gz) as f32);
                    for extra in [0.0f32, 0.6, -0.6, 1.2, -1.2] {
                        let b = away.1.atan2(away.0) + extra;
                        let d = (SPORROW_RADIUS + 10) as f32;
                        let (fx, fz) = (x + (b.cos() * d) as i32, z + (b.sin() * d) as i32);
                        if let Some(f) = self.build_forge(fx, fz, (x, z)) {
                            return Some((v, f));
                        }
                    }
                }
            }
        }
        None
    }

    fn flat(&self, x: i32, z: i32, half: i32, max_step: i32) -> Option<(i32, i32)> {
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for dz in -half..=half {
            for dx in -half..=half {
                let c = self.column(x + dx, z + dz);
                if c.land != Land::Main || c.height < c.water || c.height < SEA_LEVEL {
                    return None;
                }
                lo = lo.min(c.height);
                hi = hi.max(c.height);
            }
        }
        (hi - lo <= max_step).then_some((lo, hi))
    }

    fn build_sporrow(&self, x: i32, z: i32) -> Option<Village> {
        let col = self.column(x, z);
        if col.land != Land::Main || col.height < SEA_LEVEL || col.height < col.water || col.climate.overhang > 0.02 {
            return None;
        }
        // Clear of other villages and of the gate.
        let r = SPORROW_RADIUS + 40;
        if self.villages_in(x - r, z - r, x + r, z + r).iter().any(|v| {
            let (dx, dz) = (x - v.center[0], z - v.center[2]);
            dx * dx + dz * dz < (v.radius + SPORROW_RADIUS + 10).pow(2)
        }) {
            return None;
        }
        if let Some(g) = self.rotting_gate() {
            let (dx, dz) = (x - g.pos[0], z - g.pos[2]);
            if dx * dx + dz * dz < (SPORROW_RADIUS + 20).pow(2) {
                return None;
            }
        }
        let area = b_box((x - r, x + r), (col.height - 30, col.height + 10), (z - r, z + r));
        if self.dungeons_in(area.min[0], area.min[2], area.max[0], area.max[2]).iter().any(|d| d.bounds.intersects(&area)) {
            return None;
        }
        let h = hash2(self.seed ^ 0x5B0_0220, x, z);
        let n = 5;
        let turn = unit(h) * std::f32::consts::TAU;
        let mut huts = Vec::new();
        for i in 0..n {
            let a = turn + i as f32 / n as f32 * std::f32::consts::TAU;
            let rr = 11.0 + ((h >> (9 + i * 3)) % 4) as f32;
            let (hx, hz) = (x + (a.cos() * rr).round() as i32, z + (a.sin() * rr).round() as i32);
            let Some((_, hi)) = self.flat(hx, hz, 2, 2) else { continue };
            let (dx, dz) = (x - hx, z - hz);
            let door = if dx.abs() > dz.abs() { (dx.signum(), 0) } else { (0, dz.signum()) };
            let door = if door == (0, 0) { (1, 0) } else { door };
            // Mushroom houses sit on the ground: the floor is the highest ground under them.
            huts.push(Hut { x: hx, z: hz, ground: hi, floor: hi, door });
        }
        if huts.len() < 4 {
            return None;
        }
        Some(Village { id: h | 1, name: "Sporrow".into(), center: [x, col.height, z], radius: SPORROW_RADIUS, huts, folk: Folk::Mushroom })
    }

    fn build_forge(&self, x: i32, z: i32, village: (i32, i32)) -> Option<Forge> {
        let (_, hi) = self.flat(x, z, 3, 2)?;
        let (dx, dz) = (village.0 - x, village.1 - z);
        let door = if dx.abs() > dz.abs() { (dx.signum(), 0) } else { (0, dz.signum()) };
        let floor = hi + 1;
        let stand = [(x + door.0 * 4) as f32 + 0.5, floor as f32, (z + door.1 * 4) as f32 + 0.5];
        Some(Forge { pos: [x, floor, z], door, stand, bounds: b_box((x - 3, x + 3), (floor - 3, floor + 6), (z - 3, z + 3)) })
    }

    /// The highest, level enough ground in the mountains of the main island.
    pub(crate) fn search_castle(&self) -> Option<Castle> {
        let [sx, _, sz] = self.spawn;
        // Cheap first pass: the tallest dry columns on rings out from spawn.
        let mut tall: Vec<(i32, i32, i32)> = Vec::new();
        for ring in 0..34 {
            let r = 250.0 + ring as f32 * 50.0;
            for s in 0..48 {
                let a = s as f32 / 48.0 * std::f32::consts::TAU + ring as f32 * 0.37;
                let (x, z) = (sx + (a.cos() * r) as i32, sz + (a.sin() * r) as i32);
                let c = self.column(x, z);
                if c.land == Land::Main && c.height > SEA_LEVEL + 5 && c.height >= c.water {
                    tall.push((c.height, x, z));
                }
            }
        }
        tall.sort_by_key(|t| std::cmp::Reverse(t.0));
        // Then the highest with room for the court; rougher ground only if nothing smoother is found.
        for step in [16, 24, 40] {
            for &(_, x, z) in tall.iter().take(200) {
                if let Some(c) = self.build_castle(x, z, step) {
                    return Some(c);
                }
            }
        }
        None
    }

    fn build_castle(&self, x: i32, z: i32, step: i32) -> Option<Castle> {
        let (lo, hi) = self.flat(x, z, COURT, step)?;
        let area = b_box((x - COURT - 4, x + COURT + 4), (lo - 2, hi + TOWER_HEIGHT + 6), (z - COURT - 4, z + COURT + 4));
        if self.dungeons_in(area.min[0], area.min[2], area.max[0], area.max[2]).iter().any(|d| d.bounds.intersects(&area))
            || !self.villages_in(area.min[0], area.min[2], area.max[0], area.max[2]).is_empty()
        {
            return None;
        }
        let floor = hi + 1;
        let tower = (x - COURT + TOWER_HALF + 1, z - COURT + TOWER_HALF + 1);
        let roof = floor + TOWER_HEIGHT + 1;
        let urn = [tower.0 as f32 + 0.5, roof as f32, tower.1 as f32 + 0.5];
        let orrin = [tower.0 as f32 + 1.5, roof as f32, tower.1 as f32 + 1.5];
        let bounds = b_box((x - COURT, x + COURT), (lo - 1, roof + 3), (z - COURT, z + COURT));
        Some(Castle { center: [x, floor, z], floor, tower, roof, urn, orrin, bounds })
    }
}

fn b_box(x: (i32, i32), y: (i32, i32), z: (i32, i32)) -> Aabb {
    Aabb { min: [x.0.min(x.1), y.0.min(y.1), z.0.min(z.1)], max: [x.0.max(x.1), y.0.max(y.1), z.0.max(z.1)] }
}

/// Sporrow's plaza and mushroom houses.
pub(crate) fn draw_sporrow(wg: &WorldGen, c: &mut Canvas, v: &Village) {
    let ids = &wg.ids;
    let [cx, _, cz] = v.center;
    // A ring of little mushrooms round the plaza (no fire here).
    for i in 0..8 {
        let a = i as f32 / 8.0 * std::f32::consts::TAU;
        let (x, z) = (cx + (a.cos() * 5.0).round() as i32, cz + (a.sin() * 5.0).round() as i32);
        let g = wg.column(x, z).height;
        let tall = 1 + (i % 2);
        for y in g + 1..=g + tall {
            c.put(x, y, z, ids.mushroom_stem);
        }
        c.put(x, g + tall + 1, z, ids.mushroom_cap);
    }
    for h in &v.huts {
        draw_mushroom_house(wg, c, h);
    }
}

fn draw_mushroom_house(wg: &WorldGen, c: &mut Canvas, h: &Hut) {
    let ids = &wg.ids;
    let f = h.floor;
    // Fill under the floor so it doesn't float on uneven ground.
    for dz in -2..=2i32 {
        for dx in -2..=2i32 {
            let g = wg.column(h.x + dx, h.z + dz).height;
            for y in g + 1..=f {
                c.put(h.x + dx, y, h.z + dz, ids.mud);
            }
        }
    }
    // A round stem room: the 5x5 less its corners, three high, a door facing the plaza.
    for dz in -2..=2i32 {
        for dx in -2..=2i32 {
            if dx.abs() == 2 && dz.abs() == 2 {
                continue;
            }
            let edge = dx.abs() == 2 || dz.abs() == 2;
            for y in f + 1..=f + 3 {
                let door = (dx, dz) == (h.door.0 * 2, h.door.1 * 2) && y <= f + 2;
                let b = if edge && !door { ids.mushroom_stem } else { BlockId::AIR };
                c.put(h.x + dx, y, h.z + dz, b);
            }
        }
    }
    // The cap: a wide layer, then a narrower one on top.
    for dz in -3..=3i32 {
        for dx in -3..=3i32 {
            if dx.abs() + dz.abs() <= 5 {
                c.put(h.x + dx, f + 4, h.z + dz, ids.mushroom_cap);
            }
            if dx.abs() <= 2 && dz.abs() <= 2 && dx.abs() + dz.abs() <= 3 {
                c.put(h.x + dx, f + 5, h.z + dz, ids.mushroom_cap);
            }
        }
    }
}

/// Gilm's hut: stone walls, a plank roof, a forge with a chimney.
pub(crate) fn draw_forge(wg: &WorldGen, c: &mut Canvas, f: &Forge) {
    let ids = &wg.ids;
    let [x, fl, z] = f.pos;
    for dz in -2..=2i32 {
        for dx in -2..=2i32 {
            let g = wg.column(x + dx, z + dz).height;
            for y in g + 1..fl {
                c.put(x + dx, y, z + dz, ids.cobblestone);
            }
            c.put(x + dx, fl - 1, z + dz, ids.cobblestone);
            let edge = dx.abs() == 2 || dz.abs() == 2;
            for y in fl..fl + 3 {
                let door = (dx, dz) == (f.door.0 * 2, f.door.1 * 2) && y <= fl + 1;
                let window = (dx == 0 || dz == 0) && y == fl + 1 && !door && (dx, dz) != (-f.door.0 * 2, -f.door.1 * 2);
                let b = if edge && !door && !window { ids.cobblestone } else { BlockId::AIR };
                c.put(x + dx, y, z + dz, b);
            }
            c.put(x + dx, fl + 3, z + dz, ids.planks);
        }
    }
    // The forge: against the back wall, a coal bed under a stone hood and chimney.
    let (bx, bz) = (x - f.door.0, z - f.door.1);
    c.put(bx, fl, bz, ids.coal);
    let (kx, kz) = (x - f.door.0 * 2, z - f.door.1 * 2);
    for y in fl + 3..fl + 6 {
        c.put(kx, y, kz, ids.cobblestone);
    }
    // Steps up to the door.
    let (sx, sz) = (x + f.door.0 * 3, z + f.door.1 * 3);
    let g = wg.column(sx, sz).height;
    for y in g + 1..fl {
        c.put(sx, y, sz, ids.cobblestone);
    }
    for y in fl..fl + 3 {
        c.put(sx, y, sz, BlockId::AIR);
    }
}

/// The aunt's cottage (stone, a plank roof, a chimney on the back wall, a lit hearth)
/// and the broken arch in front of it.
pub(crate) fn draw_aunt_island(wg: &WorldGen, c: &mut Canvas, a: &AuntIsland) {
    let ids = &wg.ids;
    let isl = a.island();
    let top = |x: i32, z: i32| crate::islands::span(wg, &isl, x, z).map(|s| s.1);
    let [x, fl, z] = a.cottage;
    let (d, back) = (a.door, (-a.door.0, -a.door.1));
    let n = COTTAGE_HALF;
    for dz in -n..=n {
        for dx in -n..=n {
            let (px, pz) = (x + dx, z + dz);
            // Foundation down to the island's top.
            let g = top(px, pz).unwrap_or(fl - 1);
            for y in g + 1..fl {
                c.put(px, y, pz, ids.cobblestone);
            }
            c.put(px, fl - 1, pz, ids.planks);
            let edge = dx.abs() == n || dz.abs() == n;
            for y in fl..fl + COTTAGE_WALL {
                let door = (dx, dz) == (d.0 * n, d.1 * n) && y <= fl + 1;
                let window = (dx == 0 || dz == 0) && y == fl + 1 && !door && (dx, dz) != (back.0 * n, back.1 * n);
                let b = if edge && !door && !window { ids.cobblestone } else { BlockId::AIR };
                c.put(px, y, pz, b);
            }
            // Roof: one layer of planks, a second smaller one on top.
            c.put(px, fl + COTTAGE_WALL, pz, ids.planks);
            if dx.abs() < n && dz.abs() < n {
                c.put(px, fl + COTTAGE_WALL + 1, pz, ids.planks);
            }
            for y in fl + COTTAGE_WALL + 2..fl + COTTAGE_WALL + 4 {
                c.put(px, y, pz, BlockId::AIR);
            }
        }
    }
    // Hearth against the back wall under the chimney, with a torch for its fire.
    let (hx, hz) = (x + back.0 * (n - 1), z + back.1 * (n - 1));
    c.put(hx, fl, hz, ids.cobblestone);
    c.put(hx, fl + 1, hz, ids.torch);
    let (kx, kz) = (x + back.0 * n, z + back.1 * n);
    for y in fl + COTTAGE_WALL..fl + COTTAGE_WALL + 4 {
        c.put(kx, y, kz, ids.cobblestone);
    }
    // A stone path from the door to the arch, with steps up to the door.
    for s in n + 1..ARCH_AHEAD {
        let (px, pz) = (x + d.0 * s, z + d.1 * s);
        if let Some(g) = top(px, pz) {
            let step = g.max(fl - 1 - (s - n - 1));
            for y in g..=step {
                c.put(px, y, pz, ids.cobblestone);
            }
            for y in step + 1..step + 4 {
                c.put(px, y, pz, BlockId::AIR);
            }
        }
    }
    // The broken arch: across the path, one pillar whole with a stub of the span,
    // the other snapped off low.
    let side = (d.1, d.0);
    let [ax, ay, az] = a.arch;
    let stone = |px: i32, py: i32, pz: i32| if hash2(wg.seed ^ 0xA4C4, px * 31 + py, pz).is_multiple_of(5) { ids.moss } else { ids.cobblestone };
    for (s, height) in [(-3, 7), (3, 3)] {
        let (px, pz) = (ax + side.0 * s, az + side.1 * s);
        let g = top(px, pz).unwrap_or(ay - 1);
        for y in g + 1..ay + height {
            c.put(px, y, pz, stone(px, y, pz));
        }
    }
    for s in -3..=0 {
        let (px, pz) = (ax + side.0 * s, az + side.1 * s);
        let y = ay + 7 - (s + 3) / 3;
        c.put(px, y, pz, stone(px, y, pz));
    }
    // Fallen pieces of the span on the ground.
    for (s, f) in [(2, 1), (1, 2)] {
        let (px, pz) = (ax + side.0 * s + d.0 * f, az + side.1 * s + d.1 * f);
        if let Some(g) = top(px, pz) {
            c.put(px, g + 1, pz, stone(px, g + 1, pz));
        }
    }
}

/// The castle: a plinth, a ruined wall round the court with a gateway, the tower.
pub(crate) fn draw_castle(wg: &WorldGen, c: &mut Canvas, k: &Castle) {
    let ids = &wg.ids;
    let [x, fl, z] = k.center;
    let wall = |px: i32, py: i32, pz: i32| {
        // Moss creeps over the old stone.
        if hash2(wg.seed ^ 0xCA57, px * 31 + py, pz).is_multiple_of(7) { ids.moss } else { ids.cobblestone }
    };
    // Which way the gateway faces: back towards spawn.
    let (tx, tz) = (wg.spawn[0] - x, wg.spawn[2] - z);
    let gate = if tx.abs() > tz.abs() { (tx.signum(), 0) } else { (0, tz.signum()) };
    for dz in -COURT..=COURT {
        for dx in -COURT..=COURT {
            let (px, pz) = (x + dx, z + dz);
            let g = wg.column(px, pz).height;
            for y in g + 1..fl {
                c.put(px, y, pz, ids.cobblestone);
            }
            c.put(px, fl - 1, pz, wall(px, fl - 1, pz));
            for y in fl..fl + 26 {
                c.put(px, y, pz, BlockId::AIR);
            }
            let edge = dx.abs() == COURT || dz.abs() == COURT;
            if !edge {
                continue;
            }
            let in_gate = (gate.0 != 0 && dx == gate.0 * COURT && dz.abs() <= 1) || (gate.1 != 0 && dz == gate.1 * COURT && dx.abs() <= 1);
            if in_gate {
                continue;
            }
            // Ruined: the wall is lower in places.
            let worn = (hash2(wg.seed ^ 0xC0DE, px, pz) % 5) as i32;
            let top = fl + 3 + if worn == 0 { -2 } else { 0 } + i32::from((dx + dz) % 2 == 0 && worn != 0);
            for y in fl..=top {
                c.put(px, y, pz, wall(px, y, pz));
            }
        }
    }
    // The tower.
    let (cx, cz) = k.tower;
    let top = fl + TOWER_HEIGHT;
    for dz in -TOWER_HALF..=TOWER_HALF {
        for dx in -TOWER_HALF..=TOWER_HALF {
            let (px, pz) = (cx + dx, cz + dz);
            let edge = dx.abs() == TOWER_HALF || dz.abs() == TOWER_HALF;
            for y in fl..top {
                let door = edge && y <= fl + 1 && ((dz == TOWER_HALF && dx == 0) || (dx == TOWER_HALF && dz == 0));
                let window = edge && (dx == 0 || dz == 0) && y > fl + 3 && (y - fl) % 6 == 0;
                c.put(px, y, pz, if edge && !door && !window { wall(px, y, pz) } else { BlockId::AIR });
            }
            // The roof, open over the last steps of the stair (they come out on top).
            let over_stair = STAIR[..4].contains(&(dx, dz));
            c.put(px, top, pz, if over_stair { BlockId::AIR } else { wall(px, top, pz) });
            // Battlements.
            if edge && (dx + dz).rem_euclid(2) == 0 {
                c.put(px, top + 1, pz, wall(px, top + 1, pz));
            }
        }
    }
    // The stair: one step up per block round the inside wall, round and round.
    for i in 0..TOWER_HEIGHT {
        let (sx, sz) = STAIR[i as usize % STAIR.len()];
        c.put(cx + sx, fl + i, cz + sz, ids.cobblestone);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sporrow_lies_near_the_gate_with_gilm_outside() {
        let wg = crate::tests::wg();
        let g = wg.rotting_gate().expect("a gate");
        let v = wg.sporrow().expect("Sporrow");
        let d = (((v.center[0] - g.pos[0]).pow(2) + (v.center[2] - g.pos[2]).pow(2)) as f32).sqrt();
        assert!((55.0..125.0).contains(&d), "Sporrow is {d} blocks from the gate");
        assert_eq!(v.folk, Folk::Mushroom);
        assert!(v.huts.len() >= 4);
        // Listed with the villages, and warded like them.
        assert!(wg.villages_in(v.center[0], v.center[2], v.center[0], v.center[2]).iter().any(|w| w.name == "Sporrow"));
        assert!(wg.warded(v.center[0], v.center[1] + 1, v.center[2]));
        let f = wg.gilm_forge().expect("Gilm's hut");
        let fd = (((f.pos[0] - v.center[0]).pow(2) + (f.pos[2] - v.center[2]).pow(2)) as f32).sqrt();
        assert!(fd > SPORROW_RADIUS as f32, "Gilm lives outside the village ({fd})");
        assert!(fd < (SPORROW_RADIUS + 20) as f32, "but not far ({fd})");
        // The houses are in the chunks.
        let h = &v.huts[0];
        let chunk = wg.generate(voxel::BlockPos::new(h.x, h.floor + 4, h.z).chunk());
        let cap = wg.ids.mushroom_cap;
        assert!(chunk.to_dense().contains(&cap), "a mushroom cap in the chunk");
    }

    #[test]
    fn the_castle_stands_high_with_the_tower_roof_open() {
        let wg = crate::tests::wg();
        let k = wg.castle().expect("a castle");
        assert!(k.floor > SEA_LEVEL + 30, "in the mountains (floor {})", k.floor);
        let col = wg.column(k.center[0], k.center[2]);
        assert_eq!(col.land, Land::Main);
        // The roof is solid under the urn, and there's air to stand in.
        let at = |x: i32, y: i32, z: i32| {
            let p = voxel::BlockPos::new(x, y, z);
            let chunk = wg.generate(p.chunk());
            let o = p.chunk().origin();
            chunk.get((x - o.x) as usize, (y - o.y) as usize, (z - o.z) as usize)
        };
        let (tx, tz) = k.tower;
        assert!(!at(tx, k.roof - 1, tz).is_air(), "roof under the urn");
        assert!(at(tx, k.roof, tz).is_air(), "air over the roof");
        assert!(at(tx, k.roof + 1, tz).is_air());
        // The stair's first step is in.
        assert!(!at(tx + STAIR[0].0, k.floor, tz + STAIR[0].1).is_air());
    }

    #[test]
    fn every_new_world_gets_both() {
        let tiles = voxel::TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
        let reg = voxel::BlockRegistry::from_ron(include_str!("../../../assets/data/blocks.ron"), &tiles).unwrap();
        for seed in [1u64, 2, 3, 7, 99, 1234, 98765] {
            let wg = WorldGen::new(seed, &reg, include_str!("../../../assets/data/biomes.ron")).unwrap();
            assert!(wg.sporrow().is_some() && wg.gilm_forge().is_some(), "seed {seed}: no Sporrow");
            assert!(wg.castle().is_some(), "seed {seed}: no castle");
            assert!(wg.aunt_island().is_some(), "seed {seed}: no aunt's island");
        }
    }

    #[test]
    fn the_aunt_lives_above_the_clouds_by_a_broken_arch() {
        let wg = crate::tests::wg();
        let a = wg.aunt_island().expect("the aunt's island");
        assert!(a.cottage[1] > 300 + 40, "above the highest clouds (floor {})", a.cottage[1]);
        let d = (((a.center[0] - wg.spawn[0]).pow(2) + (a.center[2] - wg.spawn[2]).pow(2)) as f32).sqrt();
        assert!((AUNT_FROM_SPAWN.0 - 1.0..AUNT_FROM_SPAWN.1 + 1.0).contains(&d), "{d} blocks from spawn");
        // Its own island: the sky island there is this one.
        assert_eq!(wg.island_top(a.center[0], a.center[2]), crate::islands::span(&wg, &a.island(), a.center[0], a.center[2]).map(|s| s.1));
        let at = |x: i32, y: i32, z: i32| {
            let p = voxel::BlockPos::new(x, y, z);
            let chunk = wg.generate(p.chunk());
            let o = p.chunk().origin();
            chunk.get((x - o.x) as usize, (y - o.y) as usize, (z - o.z) as usize)
        };
        let [x, fl, z] = a.cottage;
        assert_eq!(at(x, fl - 1, z), wg.ids.planks, "floor");
        assert!(at(x, fl, z).is_air() && at(x, fl + 1, z).is_air(), "room to stand inside");
        let (dx, dz) = (x + a.door.0 * COTTAGE_HALF, z + a.door.1 * COTTAGE_HALF);
        assert!(at(dx, fl, dz).is_air() && at(dx, fl + 1, dz).is_air(), "a doorway");
        assert_eq!(at(x + a.door.0 * (COTTAGE_HALF + 1), fl + 1, z + a.door.1 * (COTTAGE_HALF + 1)), BlockId::AIR);
        // The chimney stands over the back wall, above the roof.
        let (kx, kz) = (x - a.door.0 * COTTAGE_HALF, z - a.door.1 * COTTAGE_HALF);
        assert!(!at(kx, fl + COTTAGE_WALL + 3, kz).is_air(), "chimney");
        // The arch: one pillar standing tall, the other broken off low.
        let side = (a.door.1, a.door.0);
        let [ax, ay, az] = a.arch;
        assert!(!at(ax - side.0 * 3, ay + 6, az - side.1 * 3).is_air(), "the whole pillar");
        assert!(at(ax + side.0 * 3, ay + 4, az + side.1 * 3).is_air(), "the broken one");
        assert!(at(ax, ay + 1, az).is_air(), "the way through");
        // Where the aunt stands is on the ground in front of her door.
        let s = a.stand.map(|v| v.floor() as i32);
        assert!(at(s[0], s[1], s[2]).is_air() && !at(s[0], s[1] - 1, s[2]).is_air(), "she has ground to stand on");
    }
}
