//! Turning mesher quads into Bevy meshes: one opaque mesh and one translucent
//! (water, ice) mesh per chunk.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use voxel::{BlockRegistry, PaddedChunk, Quad};

/// The meshes for one chunk. Either may be missing if nothing of that kind is visible.
#[derive(Default)]
pub struct ChunkMeshes {
    pub opaque: Option<Mesh>,
    pub translucent: Option<Mesh>,
}

impl ChunkMeshes {
    pub fn is_empty(&self) -> bool {
        self.opaque.is_none() && self.translucent.is_none()
    }
}

#[derive(Default)]
struct Buffers {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    layer: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

impl Buffers {
    fn into_mesh(self) -> Option<Mesh> {
        (!self.idx.is_empty()).then(|| {
            Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, self.layer)
                .with_inserted_indices(Indices::U32(self.idx))
        })
    }
}

/// Mesh a padded chunk. Position, normal, UV (0..size, repeated per block in
/// the shader) and UV_1.x = texture layer.
pub fn build(chunk: &PaddedChunk, registry: &BlockRegistry, opaque: &[bool]) -> ChunkMeshes {
    let quads = voxel::mesher::mesh(chunk, opaque);
    let (mut solid, mut clear) = (Buffers::default(), Buffers::default());
    for q in &quads {
        let b = if registry.get(q.block).translucent { &mut clear } else { &mut solid };
        push_quad(q, registry, b);
    }
    ChunkMeshes { opaque: solid.into_mesh(), translucent: clear.into_mesh() }
}

fn push_quad(q: &Quad, registry: &BlockRegistry, b: &mut Buffers) {
    let axis = q.face.axis();
    let (ua, va) = q.face.plane_axes();
    let mut base = [q.origin[0] as f32, q.origin[1] as f32, q.origin[2] as f32];
    if q.face.is_positive() {
        base[axis] += 1.0;
    }
    let (su, sv) = (q.size_u as f32, q.size_v as f32);
    let corner = |du: f32, dv: f32| {
        let mut c = base;
        c[ua] += du;
        c[va] += dv;
        c
    };
    let def = registry.get(q.block);
    // Water surfaces sit a little lower than a full block, like a real waterline.
    let lower_top = def.translucent && !def.solid && q.face == voxel::Face::PosY;
    let corners = [corner(0.0, 0.0), corner(su, 0.0), corner(su, sv), corner(0.0, sv)];
    let normal = q.face.normal().map(|v| v as f32);
    let tex = def.textures[q.face as usize] as f32;
    let start = b.pos.len() as u32;
    for mut c in corners {
        if lower_top {
            c[1] -= 0.12;
        }
        b.pos.push(c);
        b.nor.push(normal);
        // Texture coordinates from position: upright on walls, and continuous
        // across a merged quad so `fract` tiles it once per block.
        b.uv.push(match axis {
            0 => [c[2], -c[1]],
            1 => [c[0], c[2]],
            _ => [c[0], -c[1]],
        });
        b.layer.push([tex, 0.0]);
    }
    // u x v points along +axis for x and z faces, and along -axis for y faces.
    let flip = (axis == 1) == q.face.is_positive();
    if flip {
        b.idx.extend_from_slice(&[start, start + 2, start + 1, start, start + 3, start + 2]);
    } else {
        b.idx.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
    }
}
