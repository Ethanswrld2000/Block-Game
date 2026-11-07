//! Palette-compressed 32x32x32 chunk storage.
//!
//! Most chunks in a big world are entirely air or entirely stone, so those
//! are stored as a single value. Mixed chunks store a small palette of the
//! block types they use and bit-packed indices into it: a chunk with up to
//! 16 block types costs 16 KiB instead of 64 KiB.

use crate::{BlockId, CHUNK_SIZE};

pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

#[derive(Clone, Debug)]
pub enum Chunk {
    /// Every block is the same.
    Uniform(BlockId),
    Paletted(PalettedChunk),
}

#[derive(Clone, Debug)]
pub struct PalettedChunk {
    palette: Vec<BlockId>,
    bits: u32,
    words: Vec<u64>,
}

/// Index of a local position. Layout is x fastest, then z, then y, so a
/// horizontal layer is contiguous.
#[inline]
pub fn index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(x < CHUNK_SIZE && y < CHUNK_SIZE && z < CHUNK_SIZE);
    x + z * CHUNK_SIZE + y * CHUNK_SIZE * CHUNK_SIZE
}

impl Default for Chunk {
    fn default() -> Self {
        Chunk::Uniform(BlockId::AIR)
    }
}

impl Chunk {
    pub fn filled(block: BlockId) -> Self {
        Chunk::Uniform(block)
    }

    /// Build from a dense array (as world generation produces), compressing it.
    pub fn from_dense(blocks: &[BlockId]) -> Self {
        assert_eq!(blocks.len(), CHUNK_VOLUME);
        let first = blocks[0];
        if blocks.iter().all(|&b| b == first) {
            return Chunk::Uniform(first);
        }
        let mut palette: Vec<BlockId> = Vec::new();
        let mut lookup = std::collections::HashMap::new();
        let idx: Vec<u32> = blocks
            .iter()
            .map(|&b| {
                *lookup.entry(b).or_insert_with(|| {
                    palette.push(b);
                    (palette.len() - 1) as u32
                })
            })
            .collect();
        let bits = bits_for(palette.len());
        let mut p = PalettedChunk { palette, bits, words: vec![0; words_for(bits)] };
        for (i, v) in idx.into_iter().enumerate() {
            p.write(i, v);
        }
        Chunk::Paletted(p)
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockId {
        match self {
            Chunk::Uniform(b) => *b,
            Chunk::Paletted(p) => p.palette[p.read(index(x, y, z)) as usize],
        }
    }

    pub fn set(&mut self, x: usize, y: usize, z: usize, block: BlockId) {
        let i = index(x, y, z);
        if let Chunk::Uniform(b) = *self {
            if b == block {
                return;
            }
            // Expand to a 1-bit palette: everything is index 0 = old block.
            *self = Chunk::Paletted(PalettedChunk { palette: vec![b], bits: 1, words: vec![0; words_for(1)] });
        }
        let Chunk::Paletted(p) = self else { unreachable!() };
        let pi = match p.palette.iter().position(|&b| b == block) {
            Some(pi) => pi as u32,
            None => {
                p.palette.push(block);
                let needed = bits_for(p.palette.len());
                if needed > p.bits {
                    p.repack(needed);
                }
                (p.palette.len() - 1) as u32
            }
        };
        p.write(i, pi);
    }

    /// Expand to one `BlockId` per block (for meshing and generation).
    pub fn to_dense(&self) -> Vec<BlockId> {
        match self {
            Chunk::Uniform(b) => vec![*b; CHUNK_VOLUME],
            Chunk::Paletted(p) => (0..CHUNK_VOLUME).map(|i| p.palette[p.read(i) as usize]).collect(),
        }
    }

    /// True if any block in the chunk might be `block` (checks the palette,
    /// so it is cheap; a palette can keep a block that was since removed).
    pub fn may_contain(&self, block: BlockId) -> bool {
        match self {
            Chunk::Uniform(b) => *b == block,
            Chunk::Paletted(p) => p.palette.contains(&block),
        }
    }

    pub fn is_uniform(&self) -> Option<BlockId> {
        match self {
            Chunk::Uniform(b) => Some(*b),
            Chunk::Paletted(_) => None,
        }
    }

    /// Approximate heap memory used, for the debug overlay.
    pub fn heap_bytes(&self) -> usize {
        match self {
            Chunk::Uniform(_) => 0,
            Chunk::Paletted(p) => p.words.len() * 8 + p.palette.len() * 2,
        }
    }
}

fn bits_for(palette_len: usize) -> u32 {
    // Powers of two so entries never straddle two u64 words.
    match palette_len {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        17..=256 => 8,
        _ => 16,
    }
}

fn words_for(bits: u32) -> usize {
    CHUNK_VOLUME * bits as usize / 64
}

impl PalettedChunk {
    #[inline]
    fn read(&self, i: usize) -> u32 {
        let per_word = 64 / self.bits as usize;
        let shift = (i % per_word) as u32 * self.bits;
        ((self.words[i / per_word] >> shift) & ((1u64 << self.bits) - 1)) as u32
    }

    #[inline]
    fn write(&mut self, i: usize, v: u32) {
        let per_word = 64 / self.bits as usize;
        let shift = (i % per_word) as u32 * self.bits;
        let mask = ((1u64 << self.bits) - 1) << shift;
        let w = &mut self.words[i / per_word];
        *w = (*w & !mask) | ((v as u64) << shift);
    }

    fn repack(&mut self, bits: u32) {
        let old = std::mem::replace(self, PalettedChunk { palette: Vec::new(), bits, words: vec![0; words_for(bits)] });
        self.palette = old.palette.clone();
        for i in 0..CHUNK_VOLUME {
            self.write(i, old.read(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_until_changed() {
        let mut c = Chunk::filled(BlockId(1));
        c.set(3, 4, 5, BlockId(1));
        assert_eq!(c.is_uniform(), Some(BlockId(1)));
        c.set(3, 4, 5, BlockId(2));
        assert_eq!(c.is_uniform(), None);
        assert_eq!(c.get(3, 4, 5), BlockId(2));
        assert_eq!(c.get(0, 0, 0), BlockId(1));
    }

    #[test]
    fn palette_grows_through_bit_widths() {
        let mut c = Chunk::default();
        for i in 0..40u16 {
            c.set(i as usize % 32, i as usize / 32, 7, BlockId(i + 1));
        }
        for i in 0..40u16 {
            assert_eq!(c.get(i as usize % 32, i as usize / 32, 7), BlockId(i + 1));
        }
        assert_eq!(c.get(31, 31, 31), BlockId::AIR);
    }

    #[test]
    fn dense_round_trip() {
        let mut dense = vec![BlockId::AIR; CHUNK_VOLUME];
        for (i, b) in dense.iter_mut().enumerate() {
            *b = BlockId((i % 5) as u16);
        }
        let c = Chunk::from_dense(&dense);
        assert_eq!(c.to_dense(), dense);
        assert!(c.heap_bytes() <= CHUNK_VOLUME / 2 + 16, "4-bit packing");
    }
}
