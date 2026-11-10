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
/// the shader), UV_1.x = texture layer plus corner shading (see [`pack_layer`]),
/// UV_1.y = season tint strength.
pub fn build(chunk: &PaddedChunk, registry: &BlockRegistry, opaque: &[bool]) -> ChunkMeshes {
    let quads = voxel::mesher::mesh(chunk, opaque);
    let (mut solid, mut clear) = (Buffers::default(), Buffers::default());
    for q in &quads {
        // Torches and other shaped blocks are drawn as their own models.
        if registry.get(q.block).shape != voxel::Shape::Cube {
            continue;
        }
        let b = if registry.get(q.block).translucent { &mut clear } else { &mut solid };
        push_quad(q, registry, b);
    }
    ChunkMeshes { opaque: solid.into_mesh(), translucent: clear.into_mesh() }
}

/// UV_1.x carries the texture layer in its whole part and the corner shading
/// (0 open .. 1 deep crease) in its fraction, so shading costs no extra vertex
/// data. The layer is the same on all four corners, so `floor` in the shader
/// gets it back exactly; the 0.05 keeps interpolation from dipping below it.
pub fn pack_layer(layer: f32, ao: u8) -> f32 {
    layer + 0.05 + 0.9 * (ao as f32 / 3.0)
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
    // UV_1.y: how strongly the season tints this face (see `foliage` in
    // blocks.ron): fully for leaves, half for the top of grass and moss.
    let foliage = match (def.foliage, def.opaque) {
        (true, false) => 1.0,
        (true, true) if q.face == voxel::Face::PosY => 0.5,
        _ => 0.0,
    };
    // Corner shading in `corners` order; none on water and ice.
    let ao = if def.translucent { [0; 4] } else { [q.ao[0], q.ao[1], q.ao[3], q.ao[2]] };
    let start = b.pos.len() as u32;
    for (i, mut c) in corners.into_iter().enumerate() {
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
        b.layer.push([pack_layer(tex, ao[i]), foliage]);
    }
    // u x v points along +axis for x and z faces, and along -axis for y faces.
    let flip = (axis == 1) == q.face.is_positive();
    // Split along the diagonal that keeps a dark corner in its own triangle,
    // so shading doesn't smear across the face.
    let other = ao[0] as u32 + ao[2] as u32 > ao[1] as u32 + ao[3] as u32;
    let s = start;
    b.idx.extend_from_slice(&match (flip, other) {
        (false, false) => [s, s + 1, s + 2, s, s + 2, s + 3],
        (true, false) => [s, s + 2, s + 1, s, s + 3, s + 2],
        (false, true) => [s, s + 1, s + 3, s + 1, s + 2, s + 3],
        (true, true) => [s, s + 3, s + 1, s + 1, s + 3, s + 2],
    });
}
