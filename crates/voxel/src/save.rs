//! Chunk save format.
//!
//! `BGC1` magic, then the block names the chunk uses (so saves survive new
//! blocks being added to `blocks.ron`), then LZ4-compressed u16 indices into
//! that name list, one per block.

use crate::chunk::CHUNK_VOLUME;
use crate::{BlockId, BlockRegistry, Chunk};

const MAGIC: &[u8; 4] = b"BGC1";

pub fn encode(chunk: &Chunk, blocks: &BlockRegistry) -> Vec<u8> {
    let dense = chunk.to_dense();
    let mut ids: Vec<BlockId> = Vec::new();
    let mut idx = Vec::with_capacity(CHUNK_VOLUME * 2);
    for b in &dense {
        let i = match ids.iter().position(|x| x == b) {
            Some(i) => i,
            None => {
                ids.push(*b);
                ids.len() - 1
            }
        };
        idx.extend_from_slice(&(i as u16).to_le_bytes());
    }
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&(ids.len() as u16).to_le_bytes());
    for id in &ids {
        let name = blocks.get(*id).name.as_bytes();
        out.push(name.len() as u8);
        out.extend_from_slice(name);
    }
    out.extend_from_slice(&lz4_flex::compress_prepend_size(&idx));
    out
}

/// Decode a saved chunk. Blocks no longer in the registry become air.
pub fn decode(data: &[u8], blocks: &BlockRegistry) -> Option<Chunk> {
    let rest = data.strip_prefix(MAGIC)?;
    let (n, mut rest) = rest.split_first_chunk::<2>()?;
    let mut ids = Vec::new();
    for _ in 0..u16::from_le_bytes(*n) {
        let (&len, r) = rest.split_first()?;
        let (name, r) = r.split_at_checked(len as usize)?;
        ids.push(blocks.id(std::str::from_utf8(name).ok()?).unwrap_or(BlockId::AIR));
        rest = r;
    }
    let idx = lz4_flex::decompress_size_prepended(rest).ok()?;
    if idx.len() != CHUNK_VOLUME * 2 {
        return None;
    }
    let dense: Option<Vec<BlockId>> =
        idx.as_chunks::<2>().0.iter().map(|b| ids.get(u16::from_le_bytes([b[0], b[1]]) as usize).copied()).collect();
    Some(Chunk::from_dense(&dense?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::test_registry;

    #[test]
    fn round_trip() {
        let reg = test_registry();
        let mut c = Chunk::filled(reg.expect("stone"));
        c.set(1, 2, 3, reg.expect("water"));
        c.set(31, 31, 31, BlockId::AIR);
        let bytes = encode(&c, &reg);
        assert!(bytes.len() < 2000, "compresses well ({} bytes)", bytes.len());
        assert_eq!(decode(&bytes, &reg).unwrap().to_dense(), c.to_dense());
    }

    #[test]
    fn rejects_garbage() {
        let reg = test_registry();
        assert!(decode(b"nope", &reg).is_none());
        assert!(decode(b"BGC1\x01\x00\x05stone\x00", &reg).is_none());
    }
}
