//! Block and chunk data shared by the client and, later, a server.
//!
//! Nothing in this crate knows about rendering or Bevy. Keep it that way:
//! it is what makes multiplayer possible later without a rewrite.

pub mod block;
pub mod chunk;
pub mod coords;
pub mod mesher;
pub mod raycast;
pub mod save;
pub mod tiles;

pub use block::{BlockDef, BlockId, BlockRegistry, Shape};
pub use chunk::Chunk;
pub use coords::{BlockPos, ChunkPos, CHUNK_SIZE, CHUNK_SIZE_I32};
pub use mesher::{Face, PaddedChunk, Quad};
pub use tiles::TileSheet;
