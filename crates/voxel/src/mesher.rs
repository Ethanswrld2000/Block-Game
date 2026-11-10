//! Binary greedy mesher.
//!
//! 1. For each axis, every column of 34 blocks (32 + a 1-block border from the
//!    neighbouring chunks) becomes a u64 bitmask. Hidden faces are removed for
//!    a whole column at once with a shift and an AND.
//! 2. Visible faces are sorted into 32x32 bit planes, one per slice, face
//!    direction and block type.
//! 3. Each plane is merged into the largest rectangles it can make using
//!    `trailing_zeros` / `trailing_ones`, so a flat field of grass becomes a
//!    handful of quads instead of thousands.
//!
//! 4. Each face also gets corner shading (ambient occlusion): how many of the
//!    three opaque blocks around each corner, in front of the face, crowd it.
//!    Faces only merge where the shading stays correct across the merged quad
//!    (same values, and only along the direction they don't change in).
//!
//! The output is a list of [`Quad`]s; turning those into GPU vertices is the
//! renderer's job.

use crate::{BlockId, CHUNK_SIZE};

/// Padded edge length: the chunk plus one block of each neighbour.
pub const PADDED: usize = CHUNK_SIZE + 2;
pub const PADDED_VOLUME: usize = PADDED * PADDED * PADDED;

/// The six face directions. The discriminant is the index used everywhere
/// (block texture tables, normals).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Face {
    PosX = 0,
    NegX = 1,
    PosY = 2,
    NegY = 3,
    PosZ = 4,
    NegZ = 5,
}

impl Face {
    pub const ALL: [Face; 6] = [Face::PosX, Face::NegX, Face::PosY, Face::NegY, Face::PosZ, Face::NegZ];

    /// 0 = x, 1 = y, 2 = z.
    pub fn axis(self) -> usize {
        self as usize / 2
    }

    pub fn is_positive(self) -> bool {
        (self as usize).is_multiple_of(2)
    }

    pub fn normal(self) -> [i32; 3] {
        let mut n = [0; 3];
        n[self.axis()] = if self.is_positive() { 1 } else { -1 };
        n
    }

    /// The two in-plane axes (u, v) for this face's axis.
    pub fn plane_axes(self) -> (usize, usize) {
        match self.axis() {
            0 => (1, 2), // x faces: u = y, v = z
            1 => (0, 2), // y faces: u = x, v = z
            _ => (0, 1), // z faces: u = x, v = y
        }
    }
}

/// A merged rectangle of faces of one block type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quad {
    pub face: Face,
    pub block: BlockId,
    /// Local position (x, y, z) of the first block cell covered.
    pub origin: [u8; 3],
    /// Size along the face's u axis and v axis, in blocks (1..=32).
    pub size_u: u8,
    pub size_v: u8,
    /// Corner shading, 0 (open) to 3 (in a crease), for the corners
    /// (u0, v0), (u1, v0), (u0, v1), (u1, v1), where u1/v1 is the far edge.
    pub ao: [u8; 4],
}

/// How faces with this corner shading may merge with their neighbours.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Merge {
    /// The same everywhere: merge both ways.
    Both,
    /// Shading changes along u only: stack along v.
    AlongV,
    /// Shading changes along v only: stack along u.
    AlongU,
    /// A lone face.
    None,
}

fn merge_kind(ao: [u8; 4]) -> Merge {
    let along_v = ao[0] == ao[2] && ao[1] == ao[3];
    let along_u = ao[0] == ao[1] && ao[2] == ao[3];
    match (along_u, along_v) {
        (true, true) => Merge::Both,
        (false, true) => Merge::AlongV,
        (true, false) => Merge::AlongU,
        _ => Merge::None,
    }
}

/// Corner shading for one corner from its two side neighbours and the
/// diagonal one: two sides make a full crease whatever the diagonal is.
#[inline]
fn corner_ao(side_u: bool, side_v: bool, diagonal: bool) -> u8 {
    if side_u && side_v { 3 } else { side_u as u8 + side_v as u8 + diagonal as u8 }
}

/// A chunk plus a 1-block border, as dense ids. Index with [`padded_index`];
/// coordinates run 0..34, where 1..=32 is the chunk itself.
pub struct PaddedChunk {
    pub blocks: Vec<BlockId>,
}

#[inline]
pub fn padded_index(x: usize, y: usize, z: usize) -> usize {
    x + z * PADDED + y * PADDED * PADDED
}

impl Default for PaddedChunk {
    fn default() -> Self {
        Self { blocks: vec![BlockId::AIR; PADDED_VOLUME] }
    }
}

impl PaddedChunk {
    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
        self.blocks[padded_index(x, y, z)]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, b: BlockId) {
        self.blocks[padded_index(x, y, z)] = b;
    }
}

/// Mesh one padded chunk. `opaque[id]` says whether a block hides faces behind it.
pub fn mesh(chunk: &PaddedChunk, opaque: &[bool]) -> Vec<Quad> {
    let mut quads = Vec::new();
    // Planes reused between slices: (block, corner shading, rows[u] = bits along v).
    let mut planes: Vec<(BlockId, [u8; 4], [u32; CHUNK_SIZE])> = Vec::new();
    let occludes = |x: usize, y: usize, z: usize| opaque[chunk.get(x, y, z).0 as usize];

    for axis in 0..3 {
        // Column masks for this axis: index [u * PADDED + v] over padded coords.
        let mut solid = vec![0u64; PADDED * PADDED];
        let mut opaq = vec![0u64; PADDED * PADDED];
        let (ua, va) = Face::ALL[axis * 2].plane_axes();
        for u in 0..PADDED {
            for v in 0..PADDED {
                let (mut s, mut o) = (0u64, 0u64);
                for d in 0..PADDED {
                    let mut p = [0; 3];
                    p[axis] = d;
                    p[ua] = u;
                    p[va] = v;
                    let b = chunk.get(p[0], p[1], p[2]);
                    if !b.is_air() {
                        s |= 1 << d;
                        if opaque[b.0 as usize] {
                            o |= 1 << d;
                        }
                    }
                }
                solid[u * PADDED + v] = s;
                opaq[u * PADDED + v] = o;
            }
        }

        for face in [Face::ALL[axis * 2], Face::ALL[axis * 2 + 1]] {
            let positive = face.is_positive();
            // visible[d][u] = bits along v, for slices d in 0..32.
            let mut slices = [[0u32; CHUNK_SIZE]; CHUNK_SIZE];
            let mut any = false;
            for u in 1..=CHUNK_SIZE {
                for v in 1..=CHUNK_SIZE {
                    let s = solid[u * PADDED + v];
                    let o = opaq[u * PADDED + v];
                    let neighbour_opaque = if positive { o >> 1 } else { o << 1 };
                    let neighbour_solid = if positive { s >> 1 } else { s << 1 };
                    // Inner 32 bits only (padded coords 1..=32).
                    let mut vis = s & !neighbour_opaque & 0x1_FFFF_FFFE;
                    // Transparent next to the same transparent block (water, leaves): hide.
                    let mut check = vis & neighbour_solid;
                    while check != 0 {
                        let d = check.trailing_zeros() as usize;
                        check &= check - 1;
                        let nd = if positive { d + 1 } else { d - 1 };
                        let at = |dd: usize| {
                            let mut p = [0; 3];
                            p[axis] = dd;
                            p[ua] = u;
                            p[va] = v;
                            chunk.get(p[0], p[1], p[2])
                        };
                        if at(d) == at(nd) {
                            vis &= !(1 << d);
                        }
                    }
                    let mut bits = vis >> 1;
                    while bits != 0 {
                        let d = bits.trailing_zeros() as usize;
                        bits &= bits - 1;
                        slices[d][u - 1] |= 1 << (v - 1);
                        any = true;
                    }
                }
            }
            if !any {
                continue;
            }

            for (d, rows) in slices.iter().enumerate() {
                if rows.iter().all(|&r| r == 0) {
                    continue;
                }
                // Split this slice's visible faces by block type and corner shading.
                planes.clear();
                // The layer in front of these faces, in padded coordinates.
                let front = if positive { d + 2 } else { d };
                for (u, &row) in rows.iter().enumerate() {
                    let mut bits = row;
                    while bits != 0 {
                        let v = bits.trailing_zeros() as usize;
                        bits &= bits - 1;
                        let mut p = [0; 3];
                        p[axis] = d + 1;
                        p[ua] = u + 1;
                        p[va] = v + 1;
                        let b = chunk.get(p[0], p[1], p[2]);
                        let at = |du: isize, dv: isize| {
                            let mut q = [0usize; 3];
                            q[axis] = front;
                            q[ua] = (u as isize + 1 + du) as usize;
                            q[va] = (v as isize + 1 + dv) as usize;
                            occludes(q[0], q[1], q[2])
                        };
                        let (nu, pu, nv, pv) = (at(-1, 0), at(1, 0), at(0, -1), at(0, 1));
                        let ao = [
                            corner_ao(nu, nv, at(-1, -1)),
                            corner_ao(pu, nv, at(1, -1)),
                            corner_ao(nu, pv, at(-1, 1)),
                            corner_ao(pu, pv, at(1, 1)),
                        ];
                        let plane = match planes.iter_mut().find(|(pb, pa, _)| *pb == b && *pa == ao) {
                            Some(pl) => pl,
                            None => {
                                planes.push((b, ao, [0; CHUNK_SIZE]));
                                planes.last_mut().unwrap()
                            }
                        };
                        plane.2[u] |= 1 << v;
                    }
                }
                for (block, ao, plane) in planes.iter_mut() {
                    greedy_plane(plane, merge_kind(*ao), |u, v, su, sv| {
                        let mut origin = [0u8; 3];
                        origin[axis] = d as u8;
                        origin[ua] = u as u8;
                        origin[va] = v as u8;
                        quads.push(Quad { face, block: *block, origin, size_u: su as u8, size_v: sv as u8, ao: *ao });
                    });
                }
            }
        }
    }
    quads
}

/// Merge set bits of a 32x32 plane into rectangles, only in the directions
/// `merge` allows. Calls `emit(u, v, size_u, size_v)`.
fn greedy_plane(plane: &mut [u32; CHUNK_SIZE], merge: Merge, mut emit: impl FnMut(usize, usize, usize, usize)) {
    let (grow_u, grow_v) = match merge {
        Merge::Both => (true, true),
        Merge::AlongU => (true, false),
        Merge::AlongV => (false, true),
        Merge::None => (false, false),
    };
    for u in 0..CHUNK_SIZE {
        while plane[u] != 0 {
            let v = plane[u].trailing_zeros() as usize;
            let run = if grow_v { (plane[u] >> v).trailing_ones() as usize } else { 1 };
            let mask = if run == 32 { u32::MAX } else { ((1u32 << run) - 1) << v };
            plane[u] &= !mask;
            let mut size_u = 1;
            while grow_u && u + size_u < CHUNK_SIZE && plane[u + size_u] & mask == mask {
                plane[u + size_u] &= !mask;
                size_u += 1;
            }
            emit(u, v, size_u, run);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STONE: BlockId = BlockId(1);
    const GLASS: BlockId = BlockId(2);
    const OPAQUE: [bool; 3] = [false, true, false];

    fn area(quads: &[Quad]) -> usize {
        quads.iter().map(|q| q.size_u as usize * q.size_v as usize).sum()
    }

    #[test]
    fn empty_chunk_has_no_quads() {
        assert!(mesh(&PaddedChunk::default(), &OPAQUE).is_empty());
    }

    #[test]
    fn single_block_has_six_faces() {
        let mut c = PaddedChunk::default();
        c.set(5, 5, 5, STONE);
        let q = mesh(&c, &OPAQUE);
        assert_eq!(q.len(), 6);
        assert!(q.iter().all(|q| q.origin == [4, 4, 4]));
    }

    #[test]
    fn full_layer_merges_into_six_quads() {
        let mut c = PaddedChunk::default();
        for x in 1..=32 {
            for z in 1..=32 {
                c.set(x, 1, z, STONE);
            }
        }
        let q = mesh(&c, &OPAQUE);
        assert_eq!(q.len(), 6, "top, bottom and four edges, each one quad");
        let top = q.iter().find(|q| q.face == Face::PosY).unwrap();
        assert_eq!((top.size_u, top.size_v), (32, 32));
        assert_eq!(area(&q), 32 * 32 * 2 + 32 * 4);
    }

    #[test]
    fn neighbour_border_hides_faces() {
        let mut c = PaddedChunk::default();
        c.set(1, 1, 1, STONE);
        c.set(0, 1, 1, STONE); // block in the -X neighbour chunk
        let q = mesh(&c, &OPAQUE);
        assert_eq!(q.len(), 5);
        assert!(!q.iter().any(|q| q.face == Face::NegX));
    }

    #[test]
    fn same_transparent_blocks_hide_shared_faces() {
        let mut c = PaddedChunk::default();
        c.set(3, 3, 3, GLASS);
        c.set(4, 3, 3, GLASS);
        let q = mesh(&c, &OPAQUE);
        // Two cubes side by side merge into a 2x1x1 box: 6 faces, no inner ones.
        assert_eq!(q.len(), 6);
        assert_eq!(area(&q), 10);
    }

    #[test]
    fn stone_behind_glass_still_visible() {
        let mut c = PaddedChunk::default();
        c.set(3, 3, 3, STONE);
        c.set(4, 3, 3, GLASS);
        let q = mesh(&c, &OPAQUE);
        assert!(q.iter().any(|q| q.block == STONE && q.face == Face::PosX));
        assert!(!q.iter().any(|q| q.block == GLASS && q.face == Face::NegX));
    }

    #[test]
    fn open_faces_have_no_shading() {
        let mut c = PaddedChunk::default();
        c.set(5, 5, 5, STONE);
        assert!(mesh(&c, &OPAQUE).iter().all(|q| q.ao == [0; 4]));
    }

    #[test]
    fn floor_by_a_wall_is_shaded_along_the_wall() {
        // A 32x32 floor with a wall along x = 1 standing on it.
        let mut c = PaddedChunk::default();
        for x in 1..=32 {
            for z in 1..=32 {
                c.set(x, 1, z, STONE);
            }
        }
        for z in 0..=33 {
            c.set(1, 2, z, STONE);
        }
        let tops: Vec<_> = mesh(&c, &OPAQUE).into_iter().filter(|q| q.face == Face::PosY && q.origin[1] == 0).collect();
        // The strip touching the wall (x = 1) is one quad, dark on its wall side only.
        let strip: Vec<_> = tops.iter().filter(|q| q.origin[0] == 1).collect();
        assert_eq!(strip.len(), 1, "{tops:?}");
        assert_eq!((strip[0].size_u, strip[0].size_v), (1, 32));
        assert_eq!(strip[0].ao, [2, 0, 2, 0]);
        // The rest of the floor is open and merges as before.
        let open: Vec<_> = tops.iter().filter(|q| q.origin[0] > 1).collect();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].ao, [0; 4]);
        assert_eq!(area(&tops), 31 * 32);
    }

    #[test]
    fn crease_corner_is_fully_dark() {
        assert_eq!(corner_ao(true, true, false), 3);
        assert_eq!(corner_ao(true, false, true), 2);
        assert_eq!(corner_ao(false, false, true), 1);
        assert_eq!(corner_ao(false, false, false), 0);
    }

    #[test]
    fn different_blocks_do_not_merge() {
        let mut c = PaddedChunk::default();
        c.set(1, 1, 1, STONE);
        c.set(2, 1, 1, GLASS);
        let tops: Vec<_> = mesh(&c, &OPAQUE).into_iter().filter(|q| q.face == Face::PosY).collect();
        assert_eq!(tops.len(), 2);
    }
}
