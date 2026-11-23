//! Block-based light: sky light and block light (torches), 0..=15 per cell.
//!
//! Both spread by flood fill through cells that don't stop light, losing one
//! level per step, so light goes round corners and into cave mouths but never
//! through a wall. Sky light starts at full strength in every cell with open
//! sky straight above it; block light starts at each glowing block's `light`.
//!
//! Light is worked out per chunk when it is meshed, over the chunk plus
//! [`MARGIN`] blocks on every side (taken from its 26 neighbours), so light
//! from up to 15 blocks away reaches its faces. The mesher bakes it into the
//! corners of each face; the shader scales sunlight and sky light by the sky
//! level and adds torch light by the block level. Day and night need no
//! remeshing: the sky level only says how open a spot is to the sky.

use crate::{BlockId, Chunk, CHUNK_SIZE};

/// Full light.
pub const MAX: u8 = 15;
/// Blocks taken from the neighbours on every side. Light runs at most 15
/// steps, so this covers every source that can reach the chunk's faces.
pub const MARGIN: usize = 16;
/// Edge of the region worked out per chunk.
pub const SIZE: usize = CHUNK_SIZE + 2 * MARGIN;
pub const VOLUME: usize = SIZE * SIZE * SIZE;

/// Per block id: whether it stops light, and the light it gives off.
#[derive(Debug, Clone, Default)]
pub struct Tables {
    pub blocks: Vec<bool>,
    pub emit: Vec<u8>,
}

/// A chunk and what's around it.
pub struct Neighbourhood<'a> {
    /// The 3x3x3 chunks around the one being lit, at [`neighbour_index`].
    /// `None` = air (outside the loaded band).
    pub chunks: [Option<&'a Chunk>; 27],
    /// For each of the 3x3 columns ([`column_index`]): the chunks above the top
    /// layer, going up. Open sky is assumed above the last one.
    pub above: [Vec<&'a Chunk>; 9],
}

/// Index into [`Neighbourhood::chunks`] for an offset of -1..=1 on each axis.
pub fn neighbour_index(dx: i32, dy: i32, dz: i32) -> usize {
    ((dx + 1) + (dz + 1) * 3 + (dy + 1) * 9) as usize
}

/// Index into [`Neighbourhood::above`] for a column offset of -1..=1.
pub fn column_index(dx: i32, dz: i32) -> usize {
    ((dx + 1) + (dz + 1) * 3) as usize
}

#[inline]
pub fn region_index(x: usize, y: usize, z: usize) -> usize {
    x + z * SIZE + y * SIZE * SIZE
}

/// Sky and block light over one chunk's region.
pub struct Light {
    pub sky: Vec<u8>,
    pub block: Vec<u8>,
    /// Cells that stop light.
    pub stops: Vec<bool>,
}

impl Light {
    /// Light everywhere: what a chunk with nothing around it would get.
    pub fn full() -> Self {
        Self { sky: vec![MAX; VOLUME], block: vec![0; VOLUME], stops: vec![false; VOLUME] }
    }

    /// (sky, block) at a padded-chunk position (`mesher::PaddedChunk`, 0..34).
    /// A cell that stops light but still shows faces around it (a shut door)
    /// takes the brightest of its neighbours, one level down.
    pub fn padded(&self, x: usize, y: usize, z: usize) -> (u8, u8) {
        let (rx, ry, rz) = (x + MARGIN - 1, y + MARGIN - 1, z + MARGIN - 1);
        let i = region_index(rx, ry, rz);
        if !self.stops[i] {
            return (self.sky[i], self.block[i]);
        }
        let (mut s, mut b) = (0u8, 0u8);
        for n in neighbours(rx, ry, rz) {
            if !self.stops[n] {
                s = s.max(self.sky[n]);
                b = b.max(self.block[n]);
            }
        }
        (s.saturating_sub(1), b.saturating_sub(1))
    }
}

/// The in-bounds face neighbours of a region cell.
#[inline]
fn neighbours(x: usize, y: usize, z: usize) -> impl Iterator<Item = usize> {
    let i = region_index(x, y, z);
    let s = SIZE;
    let ss = SIZE * SIZE;
    [
        (x + 1 < s).then(|| i + 1),
        (x > 0).then(|| i - 1),
        (z + 1 < s).then(|| i + s),
        (z > 0).then(|| i - s),
        (y + 1 < s).then(|| i + ss),
        (y > 0).then(|| i - ss),
    ]
    .into_iter()
    .flatten()
}

/// Light the chunk in the middle of `n`.
pub fn compute(n: &Neighbourhood, t: &Tables) -> Light {
    let mut stops = vec![false; VOLUME];
    let mut block = vec![0u8; VOLUME];
    let stops_light = |b: BlockId| t.blocks.get(b.0 as usize).copied().unwrap_or(true);
    let emits = |b: BlockId| t.emit.get(b.0 as usize).copied().unwrap_or(0);
    let mut queue: Vec<u32> = Vec::new();

    // Copy the blocks in: which cells stop light, which glow.
    // Per axis, chunk offset d covers region cells r = local + 32 d + MARGIN.
    let span = |d: i32| -> (usize, usize) {
        match d {
            -1 => (CHUNK_SIZE - MARGIN, CHUNK_SIZE),
            0 => (0, CHUNK_SIZE),
            _ => (0, MARGIN),
        }
    };
    let to_region = |l: usize, d: i32| (l as i32 + CHUNK_SIZE as i32 * d + MARGIN as i32) as usize;
    for dy in -1..=1 {
        for dz in -1..=1 {
            for dx in -1..=1 {
                let Some(c) = n.chunks[neighbour_index(dx, dy, dz)] else { continue };
                let (x0, x1) = span(dx);
                let (y0, y1) = span(dy);
                let (z0, z1) = span(dz);
                if let Some(b) = c.is_uniform() {
                    let (st, em) = (stops_light(b), emits(b));
                    if !st && em == 0 {
                        continue;
                    }
                    for y in y0..y1 {
                        for z in z0..z1 {
                            for x in x0..x1 {
                                let i = region_index(to_region(x, dx), to_region(y, dy), to_region(z, dz));
                                stops[i] = st;
                                if em > 0 {
                                    block[i] = em;
                                    queue.push(i as u32);
                                }
                            }
                        }
                    }
                    continue;
                }
                for y in y0..y1 {
                    for z in z0..z1 {
                        for x in x0..x1 {
                            let b = c.get(x, y, z);
                            if b.is_air() {
                                continue;
                            }
                            let i = region_index(to_region(x, dx), to_region(y, dy), to_region(z, dz));
                            stops[i] = stops_light(b);
                            let em = emits(b);
                            if em > 0 {
                                block[i] = em;
                                queue.push(i as u32);
                            }
                        }
                    }
                }
            }
        }
    }
    spread(&mut block, &stops, &mut queue);

    // Sky: which region columns see open sky above the region's top.
    let mut sky = vec![0u8; VOLUME];
    for dz in -1..=1 {
        for dx in -1..=1 {
            let top = n.chunks[neighbour_index(dx, 1, dz)];
            let above = &n.above[column_index(dx, dz)];
            let (x0, x1) = span(dx);
            let (z0, z1) = span(dz);
            for z in z0..z1 {
                for x in x0..x1 {
                    // The part of the top chunk above the region, then the chunks over it.
                    let covered = top.is_some_and(|c| column_blocked(c, x, z, MARGIN, &stops_light))
                        || above.iter().any(|c| column_blocked(c, x, z, 0, &stops_light));
                    if covered {
                        continue;
                    }
                    let (rx, rz) = (to_region(x, dx), to_region(z, dz));
                    for y in (0..SIZE).rev() {
                        let i = region_index(rx, y, rz);
                        if stops[i] {
                            break;
                        }
                        sky[i] = MAX;
                    }
                }
            }
        }
    }
    // Spread sideways and down from the edges of the sunlit cells.
    queue.clear();
    for y in 0..SIZE {
        for z in 0..SIZE {
            for x in 0..SIZE {
                let i = region_index(x, y, z);
                if sky[i] == MAX && neighbours(x, y, z).any(|j| !stops[j] && sky[j] < MAX - 1) {
                    queue.push(i as u32);
                }
            }
        }
    }
    spread(&mut sky, &stops, &mut queue);
    Light { sky, block, stops }
}

/// Whether anything in column (x, z) of `c`, from local height `from` up, stops light.
fn column_blocked(c: &Chunk, x: usize, z: usize, from: usize, stops: &impl Fn(BlockId) -> bool) -> bool {
    if let Some(b) = c.is_uniform() {
        return stops(b);
    }
    (from..CHUNK_SIZE).any(|y| {
        let b = c.get(x, y, z);
        !b.is_air() && stops(b)
    })
}

/// Flood fill outward from the queued cells, one level less per step.
fn spread(level: &mut [u8], stops: &[bool], queue: &mut Vec<u32>) {
    let mut head = 0;
    while head < queue.len() {
        let i = queue[head] as usize;
        head += 1;
        let l = level[i];
        if l <= 1 {
            continue;
        }
        let (x, z, y) = (i % SIZE, (i / SIZE) % SIZE, i / (SIZE * SIZE));
        for j in neighbours(x, y, z) {
            if !stops[j] && level[j] < l - 1 {
                level[j] = l - 1;
                queue.push(j as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STONE: BlockId = BlockId(1);
    const TORCH: BlockId = BlockId(2);

    fn tables() -> Tables {
        Tables { blocks: vec![false, true, false], emit: vec![0, 0, 14] }
    }

    fn hood<'a>(chunks: [Option<&'a Chunk>; 27]) -> Neighbourhood<'a> {
        Neighbourhood { chunks, above: Default::default() }
    }

    /// Light at a position in the middle chunk.
    fn at(l: &Light, x: usize, y: usize, z: usize) -> (u8, u8) {
        l.padded(x + 1, y + 1, z + 1)
    }

    #[test]
    fn open_air_is_fully_sky_lit() {
        let air = Chunk::filled(BlockId::AIR);
        let l = compute(&hood([Some(&air); 27]), &tables());
        assert_eq!(at(&l, 5, 5, 5), (MAX, 0));
    }

    #[test]
    fn sealed_room_is_dark_and_a_torch_lights_it() {
        // Solid stone all round, a hollow 7x7x7 room in the middle chunk.
        let stone = Chunk::filled(STONE);
        let mut mid = Chunk::filled(STONE);
        for y in 10..17 {
            for z in 10..17 {
                for x in 10..17 {
                    mid.set(x, y, z, BlockId::AIR);
                }
            }
        }
        let mut chunks = [Some(&stone); 27];
        chunks[neighbour_index(0, 0, 0)] = Some(&mid);
        let l = compute(&hood(chunks), &tables());
        assert_eq!(at(&l, 13, 13, 13), (0, 0), "no sky, no torch: dark");

        let mut lit = mid.clone();
        lit.set(10, 10, 10, TORCH);
        chunks[neighbour_index(0, 0, 0)] = Some(&lit);
        let l = compute(&hood(chunks), &tables());
        assert_eq!(at(&l, 10, 10, 10).1, 14);
        assert_eq!(at(&l, 11, 10, 10).1, 13);
        assert_eq!(at(&l, 13, 10, 10).1, 11, "falls one level per step");
        assert_eq!(at(&l, 16, 16, 16).1, 0, "18 steps away: out of reach");
        assert_eq!(at(&l, 16, 16, 16).0, 0);
    }

    #[test]
    fn sky_comes_down_a_shaft_and_round_the_corner() {
        // Stone below y 20 in the middle chunk, air above; a 1x1 shaft down
        // to y 5, then a tunnel along x.
        let air = Chunk::filled(BlockId::AIR);
        let stone = Chunk::filled(STONE);
        let mut mid = Chunk::filled(BlockId::AIR);
        for y in 0..20 {
            for z in 0..32 {
                for x in 0..32 {
                    mid.set(x, y, z, STONE);
                }
            }
        }
        for y in 5..20 {
            mid.set(16, y, 16, BlockId::AIR);
        }
        for x in 17..28 {
            mid.set(x, 5, 16, BlockId::AIR);
        }
        let mut chunks = [Some(&air); 27];
        for dz in -1..=1 {
            for dx in -1..=1 {
                chunks[neighbour_index(dx, -1, dz)] = Some(&stone);
                if (dx, dz) != (0, 0) {
                    chunks[neighbour_index(dx, 0, dz)] = Some(&stone);
                }
            }
        }
        chunks[neighbour_index(0, 0, 0)] = Some(&mid);
        let l = compute(&hood(chunks), &tables());
        assert_eq!(at(&l, 16, 5, 16).0, MAX, "straight down the shaft");
        assert_eq!(at(&l, 20, 5, 16).0, MAX - 4, "round the corner it fades");
        assert_eq!(at(&l, 10, 10, 10).0, 0, "inside the rock");
    }

    #[test]
    fn a_roof_above_the_region_shades_it() {
        let air = Chunk::filled(BlockId::AIR);
        let stone = Chunk::filled(STONE);
        let mut n = hood([Some(&air); 27]);
        for a in n.above.iter_mut() {
            a.push(&air);
            a.push(&stone);
        }
        let l = compute(&n, &tables());
        assert_eq!(at(&l, 5, 5, 5).0, 0);
    }
}
