//! Dungeon entrances on the island. There is one kind of dungeon: an entrance
//! here, with a shimmering veil in its doorway (only a spirit passes), and its
//! floors built in the pocket (`crate::delve`).
//!
//! The five story dungeons come in a fixed order (`dungeons.ron` `chain`):
//! the Rotting Gate (a squat mossy arch 300-600 blocks from spawn), then three
//! gatehouses spaced along the way from the Gate to the mountain castle, and
//! last the castle's vault, a stone house in the corner of its court. The
//! others are gatehouses scattered about one per 1024-block cell, each with a
//! theme picked by its seed and 1-4 floors, more the further out it is.
//!
//! Everything is a pure function of the seed. A dungeon lists its `door` (walk
//! into it to go down), `outside` (where you stand on the path), `guide`
//! (where Brindlespheimer stands) and `bounds` (the warded box).

use std::sync::Arc;

use voxel::BlockId;

use crate::delve::fill;
use crate::structures::{BARROW_KIN, cached};
use crate::trees::Canvas;
use crate::util::{hash2, unit};
use crate::{Land, SEA_LEVEL, WorldGen};

/// One dungeon per cell of this size (when the land allows).
const CELL: i32 = 1024;
const CHANCE: f32 = 0.7;
/// How far an entrance reaches from its centre, any direction (for lookups).
const REACH: i32 = 16;
/// Scattered dungeons keep this far from spawn (the story ones are placed on their own).
const CLEAR_OF_SPAWN: i32 = 700;
/// Ids of scattered dungeons have this bit (the story dungeons are 1-5).
const GRID_ID: u64 = 1 << 40;

/// An inclusive box of blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aabb {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

impl Aabb {
    pub fn contains(&self, x: i32, y: i32, z: i32) -> bool {
        (self.min[0]..=self.max[0]).contains(&x)
            && (self.min[1]..=self.max[1]).contains(&y)
            && (self.min[2]..=self.max[2]).contains(&z)
    }

    pub fn grow(&self, by: i32) -> Self {
        Self {
            min: self.min.map(|v| v - by),
            max: self.max.map(|v| v + by),
        }
    }

    pub fn union(&self, o: &Aabb) -> Self {
        Self {
            min: [0, 1, 2].map(|i| self.min[i].min(o.min[i])),
            max: [0, 1, 2].map(|i| self.max[i].max(o.max[i])),
        }
    }

    pub fn intersects(&self, o: &Aabb) -> bool {
        (0..3).all(|i| self.min[i] <= o.max[i] && o.min[i] <= self.max[i])
    }
}

/// What kind of foe a spawn point wants (the game picks the creature).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnKind {
    /// One standing watch (a tougher kind).
    Guard,
    /// One of a group.
    Pack,
    /// The floor's boss.
    Boss,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnPoint {
    /// The block the foe's feet stand in.
    pub pos: [i32; 3],
    pub kind: SpawnKind,
    /// The room it belongs to.
    pub room: usize,
    /// Which wave it comes in (0 first).
    pub wave: u8,
    pub elite: bool,
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

/// How the entrance looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// A stone gatehouse with a stair going down at the back.
    Gatehouse,
    /// The Rotting Gate: a squat mossy arch.
    Arch,
    /// The castle's vault: a stone house in the court.
    Vault,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dungeon {
    pub id: u64,
    pub name: String,
    /// Index into `Delves::themes`.
    pub theme: usize,
    pub floors: u8,
    /// Its place in the story (1-5), if it's one of those.
    pub chain: Option<u8>,
    pub look: Look,
    /// Centre of the entrance's floor (feet height).
    pub gate: [i32; 3],
    /// Walk into this and you go down.
    pub door: Aabb,
    /// Where you stand on the path in front of the door (feet).
    pub outside: [f32; 3],
    /// Where Brindlespheimer stands beside the path (feet).
    pub guide: [f32; 3],
    pub veil: Veil,
    /// Everything built: the spirit ward covers it.
    pub bounds: Aabb,
    /// Local frame: u runs along `dir` (into the entrance), v along `side`.
    dir: (i32, i32),
    side: (i32, i32),
}

impl Dungeon {
    /// A block is under the ward (can't be broken or built on).
    pub fn warded(&self, x: i32, y: i32, z: i32) -> bool {
        self.bounds.contains(x, y, z)
    }

    /// Past the veil: inside the entrance, where the way down starts.
    pub fn inside(&self, p: [f32; 3]) -> bool {
        let b = p.map(|v| v.floor() as i32);
        self.door.contains(b[0], b[1], b[2])
    }

    fn world(&self, u: i32, v: i32) -> (i32, i32) {
        frame_world(self.gate, self.dir, self.side, u, v)
    }
}

fn frame_world(gate: [i32; 3], dir: (i32, i32), side: (i32, i32), u: i32, v: i32) -> (i32, i32) {
    (
        gate[0] + u * dir.0 + v * side.0,
        gate[2] + u * dir.1 + v * side.1,
    )
}

/// A box given in the local frame (inclusive u, v and y ranges).
fn local_box(
    gate: [i32; 3],
    dir: (i32, i32),
    side: (i32, i32),
    u: (i32, i32),
    v: (i32, i32),
    y: (i32, i32),
) -> Aabb {
    let a = frame_world(gate, dir, side, u.0, v.0);
    let b = frame_world(gate, dir, side, u.1, v.1);
    Aabb {
        min: [a.0.min(b.0), y.0.min(y.1), a.1.min(b.1)],
        max: [a.0.max(b.0), y.0.max(y.1), a.1.max(b.1)],
    }
}

fn b_box(x: (i32, i32), y: (i32, i32), z: (i32, i32)) -> Aabb {
    Aabb {
        min: [x.0.min(x.1), y.0.min(y.1), z.0.min(z.1)],
        max: [x.0.max(x.1), y.0.max(y.1), z.0.max(z.1)],
    }
}

impl WorldGen {
    /// Dungeons whose warded area touches the rectangle (inclusive block coordinates).
    pub fn dungeons_in(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> Vec<Arc<Dungeon>> {
        let touches = |d: &Dungeon| {
            d.bounds.min[0] <= x1
                && d.bounds.max[0] >= x0
                && d.bounds.min[2] <= z1
                && d.bounds.max[2] >= z0
        };
        let mut out: Vec<Arc<Dungeon>> = self
            .chain
            .iter()
            .flatten()
            .filter(|d| touches(d))
            .cloned()
            .collect();
        for cz in (z0 - REACH).div_euclid(CELL)..=(z1 + REACH).div_euclid(CELL) {
            for cx in (x0 - REACH).div_euclid(CELL)..=(x1 + REACH).div_euclid(CELL) {
                if let Some(d) = self.grid_dungeon(cx, cz)
                    && touches(&d)
                {
                    out.push(d);
                }
            }
        }
        out
    }

    /// The scattered dungeon of one cell, if it has one.
    fn grid_dungeon(&self, cx: i32, cz: i32) -> Option<Arc<Dungeon>> {
        let h = hash2(self.seed ^ 0xD0D6, cx, cz);
        if unit(h) >= CHANCE {
            return None;
        }
        let span = CELL - 2 * 200;
        let x = cx * CELL + 200 + ((h >> 8) % span as u64) as i32;
        let z = cz * CELL + 200 + ((h >> 24) % span as u64) as i32;
        let [sx, _, sz] = self.spawn;
        let far = ((((x - sx) as i64).pow(2) + ((z - sz) as i64).pow(2)) as f64).sqrt() as i32;
        if far < CLEAR_OF_SPAWN {
            return None;
        }
        let id = GRID_ID | (((cx + 0x8000) as u64) << 16) | (cz + 0x8000) as u64;
        cached(&self.structures.dungeons, (cx, cz), || {
            let theme = (h >> 33) as usize % self.delves.themes.len();
            let floors =
                (1 + (far - CLEAR_OF_SPAWN) / 450).clamp(1, crate::delve::MAX_FLOORS as i32) as u8;
            let t = &self.delves.themes[theme].def;
            let place = &t.place[(h >> 13) as usize % t.place.len().max(1)];
            let name = format!(
                "The {place} of {}",
                BARROW_KIN[(h >> 29) as usize % BARROW_KIN.len()]
            );
            self.build_gatehouse(h, x, z, id, name, theme, floors, None)
        })
    }

    /// A dungeon by its id (story dungeons 1-5, or a scattered one).
    pub fn dungeon(&self, id: u64) -> Option<Arc<Dungeon>> {
        if id & GRID_ID != 0 {
            let cx = ((id >> 16) & 0xFFFF) as i32 - 0x8000;
            let cz = (id & 0xFFFF) as i32 - 0x8000;
            return self.grid_dungeon(cx, cz).filter(|d| d.id == id);
        }
        self.chain.iter().flatten().find(|d| d.id == id).cloned()
    }

    /// The n-th story dungeon (1 = the Rotting Gate, 5 = the castle's vault).
    pub fn chain_dungeon(&self, n: u8) -> Option<Arc<Dungeon>> {
        self.chain.get(n as usize - 1).cloned().flatten()
    }

    /// The Rotting Gate: the first story dungeon.
    pub fn rotting_gate(&self) -> Option<Arc<Dungeon>> {
        self.chain_dungeon(1)
    }

    /// Is this block protected by a spirit ward (a dungeon entrance, a barrow,
    /// a village, or anywhere in the pocket)?
    pub fn warded(&self, x: i32, y: i32, z: i32) -> bool {
        self.dungeons_in(x, z, x, z)
            .iter()
            .any(|d| d.warded(x, y, z))
            || self
                .barrows_in(x, z, x, z)
                .iter()
                .any(|b| b.warded(x, y, z))
            || self
                .villages_in(x, z, x, z)
                .iter()
                .any(|v| v.warded(x, y, z))
            || crate::delve::in_pocket(x, z)
    }

    /// The story dungeons, placed once per world (`new`): the Gate, three along
    /// the way to the castle, the castle's vault.
    pub(crate) fn search_chain(&mut self) {
        let chain = self.delves.chain.clone();
        let theme = |i: usize| self.delves.theme_named(&chain[i].theme).unwrap_or(0);
        let info = |i: usize| (theme(i), chain[i].floors, chain[i].name.clone());
        let mut list: Vec<Option<Arc<Dungeon>>> = Vec::new();
        if !chain.is_empty() {
            let (t, f, n) = info(0);
            list.push(self.search_rotting_gate(t, f, n).map(Arc::new));
        }
        self.chain = list;
        // Sporrow and the castle are placed by the Gate (`landmarks`).
        if let Some((v, f)) = self.search_sporrow() {
            self.landmarks.sporrow = Some(Arc::new(v));
            self.landmarks.forge = Some(f);
        }
        self.landmarks.castle = self.search_castle();
        let [sx, _, sz] = self.spawn;
        let from = self
            .chain
            .first()
            .cloned()
            .flatten()
            .map_or((sx, sz), |g| (g.gate[0], g.gate[2]));
        let to = self
            .landmarks
            .castle
            .as_ref()
            .map_or((sx + 1200, sz + 300), |c| (c.center[0], c.center[2]));
        let middle = chain.len().saturating_sub(1);
        for i in 1..middle {
            let t = i as f32 / middle as f32;
            let (x, z) = (
                from.0 + ((to.0 - from.0) as f32 * t) as i32,
                from.1 + ((to.1 - from.1) as f32 * t) as i32,
            );
            let (th, fl, name) = info(i);
            let found = self
                .search_gatehouse_near(x, z, i as u64 + 1, name, th, fl, i as u8 + 1)
                .map(Arc::new);
            self.chain.push(found);
        }
        if middle >= 1 {
            let (th, fl, name) = info(middle);
            let vault = self.landmarks.castle.clone().map(|k| {
                Arc::new(self.build_vault(&k, middle as u64 + 1, name, th, fl, middle as u8 + 1))
            });
            self.chain.push(vault);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn search_gatehouse_near(
        &self,
        x: i32,
        z: i32,
        id: u64,
        name: String,
        theme: usize,
        floors: u8,
        chain: u8,
    ) -> Option<Dungeon> {
        for ring in 0..12 {
            let r = ring as f32 * 18.0;
            for s in 0..(1 + ring * 6) {
                let a =
                    s as f32 / (1 + ring * 6) as f32 * std::f32::consts::TAU + ring as f32 * 0.9;
                let (px, pz) = (x + (a.cos() * r) as i32, z + (a.sin() * r) as i32);
                let h = hash2(self.seed ^ 0xC4A1 ^ id, px, pz);
                let Some(d) =
                    self.build_gatehouse(h, px, pz, id, name.clone(), theme, floors, Some(chain))
                else {
                    continue;
                };
                let area = d.bounds.grow(60);
                if self
                    .dungeons_in(area.min[0], area.min[2], area.max[0], area.max[2])
                    .iter()
                    .any(|o| o.bounds.intersects(&area))
                {
                    continue;
                }
                if self
                    .landmarks
                    .castle
                    .as_ref()
                    .is_some_and(|c| c.bounds.intersects(&area))
                    || self.landmarks.sporrow.as_ref().is_some_and(|v| {
                        let (dx, dz) = (px - v.center[0], pz - v.center[2]);
                        dx * dx + dz * dz < (v.radius + 30).pow(2)
                    })
                {
                    continue;
                }
                return Some(d);
            }
        }
        None
    }

    /// A stone gatehouse facing a random way, on fairly level, dry ground.
    #[allow(clippy::too_many_arguments)]
    fn build_gatehouse(
        &self,
        h: u64,
        gx: i32,
        gz: i32,
        id: u64,
        name: String,
        theme: usize,
        floors: u8,
        chain: Option<u8>,
    ) -> Option<Dungeon> {
        let col = self.column(gx, gz);
        if col.land != Land::Main || col.height < SEA_LEVEL + 1 || col.climate.overhang > 0.02 {
            return None;
        }
        let dirs = [(1, 0), (0, 1), (-1, 0), (0, -1)];
        let dir = dirs[(h >> 3) as usize % 4];
        let side = (-dir.1, dir.0);
        let g = col.height + 1;
        let gate = [gx, g, gz];
        let at = |u: i32, v: i32| frame_world(gate, dir, side, u, v);
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
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
        let lbox =
            |u: (i32, i32), v: (i32, i32), y: (i32, i32)| local_box(gate, dir, side, u, v, y);
        let house = lbox((-4, 4), (-4, 4), (lo - 1, g + 6));
        let path = lbox((-8, -3), (-2, 2), (lo - 1, g + 4));
        let bounds = house.union(&path).union(&lbox((0, 2), (-1, 1), (g - 5, g)));
        if !self
            .villages_in(
                bounds.min[0] - 8,
                bounds.min[2] - 8,
                bounds.max[0] + 8,
                bounds.max[2] + 8,
            )
            .is_empty()
        {
            return None;
        }
        let (ox, oz) = at(-6, 0);
        let (wx, wz) = at(-6, 3);
        let guide_y = self.column(wx, wz).height + 1;
        let (vx, vz) = at(-3, 0);
        Some(Dungeon {
            id,
            name,
            theme,
            floors,
            chain,
            look: Look::Gatehouse,
            gate,
            door: lbox((0, 2), (-1, 1), (g - 4, g + 2)),
            outside: [ox as f32 + 0.5, g as f32, oz as f32 + 0.5],
            guide: [wx as f32 + 0.5, guide_y as f32, wz as f32 + 0.5],
            veil: Veil {
                center: [vx as f32 + 0.5, g as f32 + 1.5, vz as f32 + 0.5],
                out: (-dir.0, -dir.1),
                width: 3.0,
                height: 3.0,
            },
            bounds,
            dir,
            side,
        })
    }

    /// Level, dry ground on the main island 300-600 blocks from spawn, clear
    /// of villages and dungeons: the Rotting Gate.
    fn search_rotting_gate(&self, theme: usize, floors: u8, name: String) -> Option<Dungeon> {
        let [sx, _, sz] = self.spawn;
        for ring in 0..11 {
            let r = 300.0 + ring as f32 * 30.0;
            for s in 0..32 {
                let a = s as f32 / 32.0 * std::f32::consts::TAU
                    + ring as f32 * 0.7
                    + unit(self.seed) * 6.0;
                let (x, z) = (sx + (a.cos() * r) as i32, sz + (a.sin() * r) as i32);
                if let Some(g) = self.build_rotting_gate(x, z, theme, floors, name.clone()) {
                    return Some(g);
                }
            }
        }
        None
    }

    fn build_rotting_gate(
        &self,
        x: i32,
        z: i32,
        theme: usize,
        floors: u8,
        name: String,
    ) -> Option<Dungeon> {
        let col = self.column(x, z);
        if col.land != Land::Main
            || col.height < SEA_LEVEL + 1
            || col.height < col.water
            || col.climate.overhang > 0.02
        {
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
        if self
            .villages_in(x - 40, z - 40, x + 40, z + 40)
            .iter()
            .any(|v| {
                let (dx, dz) = (x - v.center[0], z - v.center[2]);
                dx * dx + dz * dz < (v.radius + 12) * (v.radius + 12)
            })
            || self
                .dungeons_in(x - 8, z - 8, x + 8, z + 8)
                .iter()
                .any(|d| near(&d.bounds))
        {
            return None;
        }
        // Face back towards spawn.
        let (tx, tz) = (self.spawn[0] - x, self.spawn[2] - z);
        let out = if tx.abs() > tz.abs() {
            (tx.signum(), 0)
        } else {
            (0, tz.signum())
        };
        let out = if out == (0, 0) { (1, 0) } else { out };
        let dir = (-out.0, -out.1);
        let side = (-dir.1, dir.0);
        // The chamber inside the arch: walk to its back half to go down.
        let (bx, bz) = (x - out.0, z - out.1);
        let door = if out.0 != 0 {
            b_box((bx, bx - out.0), (g - 1, g + 2), (z - 1, z + 1))
        } else {
            b_box((x - 1, x + 1), (g - 1, g + 2), (bz, bz - out.1))
        };
        let outside = [
            (x + out.0 * 6) as f32 + 0.5,
            g as f32,
            (z + out.1 * 6) as f32 + 0.5,
        ];
        let guide = [
            outside[0] + side.0 as f32 * 2.5 + out.0 as f32,
            g as f32,
            outside[2] + side.1 as f32 * 2.5 + out.1 as f32,
        ];
        let veil = Veil {
            center: [
                (x + out.0 * 3) as f32 + 0.5,
                g as f32 + 1.5,
                (z + out.1 * 3) as f32 + 0.5,
            ],
            out,
            width: 3.0,
            height: 3.0,
        };
        Some(Dungeon {
            id: 1,
            name,
            theme,
            floors,
            chain: Some(1),
            look: Look::Arch,
            gate: [x, g, z],
            door,
            outside,
            guide,
            veil,
            bounds,
            dir,
            side,
        })
    }

    /// The castle's vault: a small stone house in the court's far corner from the tower.
    fn build_vault(
        &self,
        k: &crate::Castle,
        id: u64,
        name: String,
        theme: usize,
        floors: u8,
        chain: u8,
    ) -> Dungeon {
        let [x, fl, z] = k.center;
        // The tower stands in the court's (-, -) corner; the vault in the (+, +) one.
        let gate = [x + 6, fl, z + 6];
        let dir = (1, 0);
        let side = (0, 1);
        let lbox =
            |u: (i32, i32), v: (i32, i32), y: (i32, i32)| local_box(gate, dir, side, u, v, y);
        let (ox, oz) = frame_world(gate, dir, side, -5, 0);
        let (wx, wz) = frame_world(gate, dir, side, -5, -3);
        Dungeon {
            id,
            name,
            theme,
            floors,
            chain: Some(chain),
            look: Look::Vault,
            gate,
            door: lbox((0, 1), (-1, 1), (fl - 4, fl + 2)),
            outside: [ox as f32 + 0.5, fl as f32, oz as f32 + 0.5],
            guide: [wx as f32 + 0.5, fl as f32, wz as f32 + 0.5],
            veil: Veil {
                center: [
                    (gate[0] - 2) as f32 + 0.5,
                    fl as f32 + 1.5,
                    gate[2] as f32 + 0.5,
                ],
                out: (-1, 0),
                width: 3.0,
                height: 3.0,
            },
            bounds: lbox((-2, 2), (-2, 2), (fl - 6, fl + 5)),
            dir,
            side,
        }
    }
}

/// Draw the parts of an entrance that fall in this chunk.
pub(crate) fn draw(wg: &WorldGen, c: &mut Canvas, d: &Dungeon) {
    match d.look {
        Look::Gatehouse => draw_gatehouse(wg, c, d),
        Look::Arch => draw_arch(wg, c, d),
        Look::Vault => draw_vault(wg, c, d),
    }
}

fn draw_gatehouse(wg: &WorldGen, c: &mut Canvas, d: &Dungeon) {
    let ids = &wg.ids;
    let g = d.gate[1];
    let lbox =
        |u: (i32, i32), v: (i32, i32), y: (i32, i32)| local_box(d.gate, d.dir, d.side, u, v, y);
    // Stone walls on a foundation down to the ground, a roof, and two tall
    // deeprock pillars by the door.
    for u in -3..=3 {
        for v in -3..=3 {
            let (x, z) = d.world(u, v);
            let ground = wg.column(x, z).height;
            for y in ground.min(g - 2)..g - 1 {
                c.put(x, y, z, ids.cobblestone);
            }
        }
    }
    fill(c, &lbox((-3, 3), (-3, 3), (g - 1, g + 4)), ids.cobblestone);
    fill(c, &lbox((-4, 4), (-4, 4), (g + 5, g + 5)), ids.deeprock);
    for v in [-2, 2] {
        fill(c, &lbox((-4, -4), (v, v), (g - 1, g + 6)), ids.deeprock);
    }
    // The path to the door: cleared, paved, filled underneath.
    for u in -8..=-4 {
        for v in -2..=2 {
            let (x, z) = d.world(u, v);
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
    // Inside: the chamber, the doorway (where the veil hangs), and a dark
    // stairwell at the back going down.
    fill(c, &lbox((-2, 2), (-2, 2), (g, g + 3)), BlockId::AIR);
    fill(c, &lbox((-3, -3), (-1, 1), (g, g + 2)), BlockId::AIR);
    fill(c, &lbox((0, 2), (-1, 1), (g - 5, g - 1)), BlockId::AIR);
    fill(c, &lbox((-1, 3), (-2, 2), (g - 6, g - 6)), ids.deeprock);
    for k in 0..3 {
        fill(c, &lbox((k, k), (-1, 1), (g - 5, g - 2 - k)), ids.deeprock);
    }
    for v in [-2, 2] {
        let (x, z) = d.world(-2, v);
        c.put(x, g, z, ids.torch);
    }
}

/// The Rotting Gate: a squat mossy arch with a dark hole at the back.
fn draw_arch(wg: &WorldGen, c: &mut Canvas, d: &Dungeon) {
    let ids = &wg.ids;
    let [x, y, z] = d.gate;
    for dz in -4..=4 {
        for dx in -4..=4 {
            let ground = wg.column(x + dx, z + dz).height;
            for yy in ground.min(y - 2)..y {
                c.put(x + dx, yy, z + dz, ids.cobblestone);
            }
        }
    }
    fill(
        c,
        &b_box((x - 3, x + 3), (y, y + 4), (z - 3, z + 3)),
        ids.cobblestone,
    );
    fill(
        c,
        &b_box((x - 1, x + 1), (y, y + 2), (z - 1, z + 1)),
        BlockId::AIR,
    );
    let (ox, oz) = d.veil.out;
    for k in 2..=3 {
        let (dx, dz) = (x + ox * k, z + oz * k);
        let b = if ox != 0 {
            b_box((dx, dx), (y, y + 2), (z - 1, z + 1))
        } else {
            b_box((x - 1, x + 1), (y, y + 2), (dz, dz))
        };
        fill(c, &b, BlockId::AIR);
    }
    // A dark hole in the floor at the back, rimmed in deeprock.
    fill(
        c,
        &b_box((x - 1, x + 1), (y - 1, y - 1), (z - 1, z + 1)),
        ids.deeprock,
    );
    fill(c, &d.door, BlockId::AIR);
    fill(
        c,
        &b_box((x - 2, x + 2), (y - 3, y - 3), (z - 2, z + 2)),
        ids.deeprock,
    );
    // Mold over everything: moss on the roof and running down the walls.
    fill(
        c,
        &b_box((x - 4, x + 4), (y + 5, y + 5), (z - 4, z + 4)),
        ids.moss,
    );
    fill(
        c,
        &b_box((x - 2, x + 2), (y + 6, y + 6), (z - 2, z + 2)),
        ids.moss,
    );
    for dz in -3..=3 {
        for dx in -3..=3 {
            for yy in y..=y + 4 {
                if c.is_air(x + dx, yy, z + dz) == Some(false)
                    && unit(hash2(wg.seed ^ 0x601D ^ yy as u64, x + dx, z + dz)) < 0.4
                {
                    c.put(
                        x + dx,
                        yy,
                        z + dz,
                        if yy % 2 == 0 { ids.moss } else { ids.mud },
                    );
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

/// The castle's vault house, in the court.
fn draw_vault(wg: &WorldGen, c: &mut Canvas, d: &Dungeon) {
    let ids = &wg.ids;
    let g = d.gate[1];
    let lbox =
        |u: (i32, i32), v: (i32, i32), y: (i32, i32)| local_box(d.gate, d.dir, d.side, u, v, y);
    fill(c, &lbox((-2, 2), (-2, 2), (g, g + 3)), ids.cobblestone);
    fill(c, &lbox((-2, 2), (-2, 2), (g + 4, g + 4)), ids.deeprock);
    fill(c, &lbox((-1, 1), (-1, 1), (g, g + 2)), BlockId::AIR);
    fill(c, &lbox((-2, -2), (-1, 1), (g, g + 2)), BlockId::AIR);
    fill(c, &d.door, BlockId::AIR);
    fill(c, &lbox((0, 1), (-1, 1), (g - 5, g - 5)), ids.deeprock);
    for v in [-1, 1] {
        let (x, z) = d.world(-1, v);
        c.put(x, g, z, ids.torch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::wg;

    #[test]
    fn the_story_dungeons_lead_from_the_gate_to_the_castle() {
        let g = wg();
        let gate = g.rotting_gate().expect("the Rotting Gate");
        let [sx, _, sz] = g.spawn;
        let d = (((gate.gate[0] - sx).pow(2) + (gate.gate[2] - sz).pow(2)) as f32).sqrt();
        assert!((290.0..=610.0).contains(&d), "{d}");
        assert_eq!(gate.chain, Some(1));
        assert_eq!(gate.floors, 2);
        assert_eq!(gate.look, Look::Arch);
        let castle = g.castle().expect("a castle");
        let vault = g.chain_dungeon(5).expect("the castle's vault");
        assert!(
            castle
                .bounds
                .contains(vault.gate[0], vault.gate[1], vault.gate[2])
        );
        for n in 1..=5 {
            let d = g
                .chain_dungeon(n)
                .unwrap_or_else(|| panic!("story dungeon {n}"));
            assert_eq!(d.chain, Some(n));
            assert_eq!(g.dungeon(d.id).as_deref(), Some(&*d));
            assert!(g.warded(d.gate[0], d.gate[1] + 1, d.gate[2]));
            assert!(d.inside([
                d.door.min[0] as f32 + 0.5,
                d.door.min[1] as f32 + 1.0,
                d.door.min[2] as f32 + 0.5
            ]));
            assert!(!d.inside(d.outside));
            eprintln!(
                "{} ({}) at {:?}",
                d.name, g.delves.themes[d.theme].def.title, d.gate
            );
        }
        // Each one along the way is further from the Gate than the last.
        let far = |n: u8| {
            let d = g.chain_dungeon(n).unwrap();
            (((d.gate[0] - gate.gate[0]).pow(2) + (d.gate[2] - gate.gate[2]).pow(2)) as f32).sqrt()
        };
        assert!(far(2) < far(3) && far(3) < far(4));
    }

    #[test]
    fn scattered_dungeons_have_themes_floors_and_ids() {
        let g = wg();
        let (cx, cz) = g.island_center();
        let r = crate::MAIN_RADIUS as i32;
        let all: Vec<_> = g
            .dungeons_in(cx - r, cz - r, cx + r, cz + r)
            .into_iter()
            .filter(|d| d.chain.is_none())
            .collect();
        assert!(all.len() >= 3, "{} dungeons", all.len());
        for d in &all {
            assert!((1..=4).contains(&d.floors));
            assert!(d.theme < g.delves.themes.len());
            assert_eq!(g.dungeon(d.id).as_deref(), Some(&**d));
        }
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(
                    !a.bounds.intersects(&b.bounds),
                    "{} overlaps {}",
                    a.name,
                    b.name
                );
            }
        }
        // Blocks: the gatehouse's doorway is open, its wall beside it is not.
        let d = &all[0];
        let block = |x: i32, y: i32, z: i32| {
            let p = voxel::BlockPos::new(x, y, z);
            let l = p.local();
            g.generate(p.chunk()).get(l[0], l[1], l[2])
        };
        let door = d.veil.center.map(|v| v.floor() as i32);
        assert_eq!(block(door[0], d.gate[1], door[2]), BlockId::AIR, "doorway");
        let (sx, sz) = d.world(-3, -2);
        assert_ne!(block(sx, d.gate[1] + 1, sz), BlockId::AIR, "gatehouse wall");
    }
}
