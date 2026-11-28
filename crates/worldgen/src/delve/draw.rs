//! Building a floor's blocks, chunk by chunk, in its theme.

use voxel::chunk::CHUNK_VOLUME;
use voxel::{BlockId, Chunk, ChunkPos, CHUNK_SIZE_I32};

use super::floor::HangKind;
use super::{Floor, Hazard};
use crate::dungeons::Aabb;
use crate::trees::Canvas;
use crate::util::{hash2, unit};
use crate::WorldGen;

/// Fill a box, clipped to the canvas's chunk.
pub(crate) fn fill(c: &mut Canvas, b: &Aabb, block: BlockId) {
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
    /// Generate a chunk in the pocket for one floor (the game calls this
    /// instead of `generate` for chunks where `in_pocket` holds).
    pub fn generate_pocket(&self, pos: ChunkPos, f: &Floor) -> Chunk {
        let origin = pos.origin();
        let chunk = Aabb { min: [origin.x, origin.y, origin.z], max: [origin.x + CHUNK_SIZE_I32 - 1, origin.y + CHUNK_SIZE_I32 - 1, origin.z + CHUNK_SIZE_I32 - 1] };
        if !chunk.intersects(&f.bounds) {
            return Chunk::filled(BlockId::AIR);
        }
        let d = &self.delves;
        let theme = &d.themes[f.plan.theme.min(d.themes.len() - 1)];
        let parts = d.parts;
        let mut dense = vec![BlockId::AIR; CHUNK_VOLUME];
        let mut c = Canvas { origin: chunk.min, dense: &mut dense };
        // Shells of the theme's wall round every space and pit, a floor under
        // the spaces, then the air carved out.
        for s in f.spaces.iter().chain(&f.pits) {
            let shell = s.grow(1);
            if shell.intersects(&chunk) {
                fill(&mut c, &shell, theme.wall);
            }
        }
        for s in &f.spaces {
            fill(&mut c, &Aabb { min: [s.min[0], s.min[1] - 1, s.min[2]], max: [s.max[0], s.min[1] - 1, s.max[2]] }, theme.floor);
        }
        for s in f.spaces.iter().chain(&f.pits) {
            if s.intersects(&chunk) {
                fill(&mut c, s, BlockId::AIR);
            }
        }
        for s in &f.pits {
            fill(&mut c, &Aabb { min: [s.min[0], s.min[1] - 1, s.min[2]], max: [s.max[0], s.min[1] - 1, s.max[2]] }, theme.floor);
        }
        // Walled-in corners.
        for s in &f.solids {
            if s.intersects(&chunk) {
                fill(&mut c, s, theme.wall);
            }
        }
        // Growth: the theme's moss, mud, roots or frost creeps over walls, floors and ceilings.
        let seed = self.seed ^ (f.plan.run as u64) << 8 ^ f.plan.floor as u64 ^ f.plan.dungeon.rotate_left(5);
        if !theme.growth.is_empty() {
            for y in chunk.min[1]..=chunk.max[1] {
                for z in chunk.min[2]..=chunk.max[2] {
                    for x in chunk.min[0]..=chunk.max[0] {
                        if c.is_air(x, y, z) != Some(false) {
                            continue;
                        }
                        let u = unit(hash2(seed ^ (y as u64).wrapping_mul(0x9E37), x, z));
                        let share = theme.def.growth_share + 0.02 * f.plan.floor as f32;
                        if u < share {
                            let k = ((u / share) * theme.growth.len() as f32) as usize;
                            c.put(x, y, z, theme.growth[k.min(theme.growth.len() - 1)]);
                        }
                    }
                }
            }
        }
        // Pools left in low spots (a flooded crypt): a puddle here and there on the floor.
        if let Some(pool) = theme.pools {
            for r in &f.rooms {
                if !r.inner.grow(1).intersects(&chunk) {
                    continue;
                }
                for z in r.inner.min[2] + 1..r.inner.max[2] {
                    for x in r.inner.min[0] + 1..r.inner.max[0] {
                        let y = r.inner.min[1];
                        if unit(hash2(seed ^ 0x9001, x / 3, z / 3)) < 0.22 && c.is_air(x, y, z) == Some(true) && c.is_air(x, y - 1, z) == Some(false) {
                            c.put(x, y, z, pool);
                        }
                    }
                }
            }
        }
        for p in &f.pillars {
            fill(&mut c, p, theme.pillar);
        }
        // Steps out of pits and holes.
        for s in &f.steps {
            c.put(s[0], s[1], s[2], theme.floor);
        }
        // Hazard blocks: oil slicks, cracked floor.
        for h in &f.hazards {
            match h.kind {
                Hazard::Oil => {
                    for p in &h.cells {
                        c.put(p[0], p[1], p[2], parts.oil);
                    }
                }
                Hazard::Collapse => {
                    for p in &h.cells {
                        c.put(p[0], p[1] - 1, p[2], parts.cracked_floor);
                    }
                }
                _ => {}
            }
        }
        // The puzzle: plates, and the lamps' sockets.
        if let Some(p) = &f.puzzle {
            for q in &p.plates {
                c.put(q[0], q[1], q[2], parts.plate);
            }
        }
        // Things hanging from the ceilings.
        for h in &f.hangings {
            let block = match h.kind {
                HangKind::Chain => parts.chain,
                HangKind::Drip => theme.wall,
            };
            for k in 0..h.len {
                let y = h.top[1] - k;
                if c.is_air(h.top[0], y, h.top[2]) != Some(false) {
                    c.put(h.top[0], y, h.top[2], block);
                }
            }
        }
        // The seals the game opens.
        for s in &f.seals {
            fill(&mut c, s, parts.slab);
        }
        // Chests, the campfire, the wall sconces.
        for ch in &f.chests {
            c.put(ch.pos[0], ch.pos[1], ch.pos[2], parts.chest);
        }
        if let Some(p) = f.campfire {
            c.put(p[0], p[1], p[2], parts.campfire);
        }
        for t in &f.sconces {
            if c.is_air(t[0], t[1], t[2]) == Some(true) || c.is_air(t[0], t[1], t[2]).is_none() {
                c.put(t[0], t[1], t[2], parts.sconce);
            }
        }
        // The start room's way back up: a cobbled arch.
        let w = &f.way_out;
        fill(&mut c, &Aabb { min: [w.min[0], w.min[1] - 1, w.min[2]], max: [w.max[0], w.min[1] - 1, w.max[2]] }, parts.cobblestone);
        Chunk::from_dense(&dense)
    }
}

#[cfg(test)]
mod tests {
    use crate::delve::{floor, FloorPlan};

    #[test]
    fn pocket_chunks_build_the_floor() {
        let wg = crate::tests::wg();
        let f = floor(&wg.delves, &FloorPlan { seed: 42, dungeon: 1, theme: 0, run: 1, floor: 1, floors: 2, chain: true, done: false });
        let [x, y, z] = f.entry.map(|v| v.floor() as i32);
        let c = wg.generate_pocket(voxel::BlockPos::new(x, y, z).chunk(), &f);
        let o = voxel::BlockPos::new(x, y, z).chunk().origin();
        let at = |x: i32, y: i32, z: i32| c.get((x - o.x) as usize, (y - o.y) as usize, (z - o.z) as usize);
        assert!(at(x, y, z).is_air() && at(x, y + 1, z).is_air(), "room to stand");
        assert!(!at(x, y - 1, z).is_air(), "a floor under you");
    }
}
