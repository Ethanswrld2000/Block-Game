//! Turning mesher quads into Bevy meshes: one opaque mesh, one translucent
//! (water, ice) mesh and one mesh of plants and other small details (crossed
//! quads, vines, twigs, pebbles) per chunk.
//!
//! UV_1.y carries two things for the shader: the season tint strength (0, 0.5
//! or 1) plus `2 * k`, where `k / SWAY_STEPS` is how far the vertex sways in
//! the wind (`voxel_vertex.wesl` splits them apart again).
//!
//! The vertex colour carries the baked light (`voxel::light`): red = sky
//! light, green = block (torch) light, both 0..1. `voxel.wesl` turns them
//! into brightness.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use voxel::light::Light;
use voxel::{BlockRegistry, PaddedChunk, Quad};

/// The meshes for one chunk. Either may be missing if nothing of that kind is visible.
#[derive(Default)]
pub struct ChunkMeshes {
    pub opaque: Option<Mesh>,
    pub translucent: Option<Mesh>,
    /// Grass, reeds and the like: drawn with the opaque material, but cast no shadow.
    pub plants: Option<Mesh>,
}

impl ChunkMeshes {
    pub fn is_empty(&self) -> bool {
        self.opaque.is_none() && self.translucent.is_none() && self.plants.is_none()
    }
}

#[derive(Default)]
struct Buffers {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    layer: Vec<[f32; 2]>,
    light: Vec<[f32; 4]>,
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
                .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.light)
                .with_inserted_indices(Indices::U32(self.idx))
        })
    }
}

/// Sway levels packed into UV_1.y (see the module docs).
pub const SWAY_STEPS: f32 = 8.0;
/// How far leaves sway, against a plant's tip (1).
const LEAF_SWAY: f32 = 0.25;

/// UV_1.y for a season tint and a sway amount (0..1).
pub fn pack_tint_sway(tint: f32, sway: f32) -> f32 {
    tint + 2.0 * (sway.clamp(0.0, 1.0) * SWAY_STEPS).round()
}

/// The inverse of [`pack_tint_sway`], as the shader does it.
#[cfg(test)]
pub fn unpack_tint_sway(packed: f32) -> (f32, f32) {
    let k = (packed * 0.5 + 0.001).floor();
    (packed - 2.0 * k, k / SWAY_STEPS)
}

/// Vertex colour for light in quarter levels (sky, block), as the mesher gives it.
fn light_color(l: [u8; 2]) -> [f32; 4] {
    let full = (voxel::light::MAX * 4) as f32;
    [l[0] as f32 / full, l[1] as f32 / full, 0.0, 1.0]
}

/// Vertex colour for a whole cell's light (plants, details, furniture).
fn cell_color(light: &Light, x: usize, y: usize, z: usize) -> [f32; 4] {
    let (s, b) = light.padded(x, y, z);
    light_color([s * 4, b * 4])
}

/// Mesh a padded chunk. Position, normal, UV (0..size, repeated per block in
/// the shader), UV_1.x = texture layer plus corner shading (see [`pack_layer`]),
/// UV_1.y = season tint and sway (see [`pack_tint_sway`]), colour = light.
pub fn build(chunk: &PaddedChunk, light: &Light, registry: &BlockRegistry, opaque: &[bool]) -> ChunkMeshes {
    let quads = voxel::mesher::mesh_lit(chunk, opaque, Some(light));
    let (mut solid, mut clear) = (Buffers::default(), Buffers::default());
    for q in &quads {
        // Torches and other shaped blocks are drawn as their own models.
        if registry.get(q.block).shape != voxel::Shape::Cube {
            continue;
        }
        let b = if registry.get(q.block).translucent { &mut clear } else { &mut solid };
        push_quad(q, registry, b);
    }
    let mut plants = Buffers::default();
    push_plants(chunk, light, registry, &mut plants);
    ChunkMeshes { opaque: solid.into_mesh(), translucent: clear.into_mesh(), plants: plants.into_mesh() }
}

/// Two crossed quads per plant block, each drawn from both sides. Normals
/// point up so a tuft is lit like the ground it stands on. The top edge sways.
fn push_plants(chunk: &PaddedChunk, light: &Light, registry: &BlockRegistry, b: &mut Buffers) {
    let n = voxel::CHUNK_SIZE;
    for y in 1..=n {
        for z in 1..=n {
            for x in 1..=n {
                let id = chunk.get(x, y, z);
                if id.is_air() {
                    continue;
                }
                let def = registry.get(id);
                let cell = [(x - 1) as f32, (y - 1) as f32, (z - 1) as f32];
                let lit = cell_color(light, x, y, z);
                let before = b.pos.len();
                // Whatever this cell adds is lit by the cell's own light.
                let fill = |b: &mut Buffers| b.light.resize(b.pos.len().max(before), lit);
                match def.shape {
                    voxel::Shape::Plant => {}
                    voxel::Shape::Vine | voxel::Shape::Twig | voxel::Shape::Pebble => {
                        push_detail(def, cell, [x as u32, y as u32, z as u32], b);
                        fill(b);
                        continue;
                    }
                    voxel::Shape::Model => {
                        let solid = |dx: i32, dz: i32| {
                            let n = chunk.get((x as i32 + dx) as usize, y, (z as i32 + dz) as usize);
                            !n.is_air() && registry.get(n).is_wall()
                        };
                        let walls = Walls { east: solid(1, 0), west: solid(-1, 0), south: solid(0, 1), north: solid(0, -1) };
                        if let Some(m) = def.model {
                            for (lo, hi) in model_boxes(m, walls) {
                                push_box(b, cell, lo, hi, def.textures[voxel::Face::PosX as usize] as f32, 0.0);
                            }
                        }
                        fill(b);
                        continue;
                    }
                    _ => continue,
                }
                let tex = def.textures[voxel::Face::PosY as usize] as f32;
                let tint = if def.foliage { 1.0 } else { 0.0 };
                // A little jitter so a field of grass isn't a grid.
                let h = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663) ^ (z as u32).wrapping_mul(83_492_791);
                let jx = ((h & 0xFF) as f32 / 255.0 - 0.5) * 0.3;
                let jz = (((h >> 8) & 0xFF) as f32 / 255.0 - 0.5) * 0.3;
                let tall = 0.85 + ((h >> 16) & 0xFF) as f32 / 255.0 * 0.25;
                let (cx, cy, cz) = ((x - 1) as f32 + 0.5 + jx, (y - 1) as f32, (z - 1) as f32 + 0.5 + jz);
                let r = 0.45;
                for (dx, dz) in [(r, r), (r, -r)] {
                    let a = [cx - dx, cy, cz - dz];
                    let c = [cx + dx, cy, cz + dz];
                    // Bottom left, bottom right, top right, top left.
                    let corners = [a, c, [c[0], cy + tall, c[2]], [a[0], cy + tall, a[2]]];
                    let uvs = [[0.0, 0.999], [0.999, 0.999], [0.999, 0.0], [0.0, 0.0]];
                    let sway = [0.0, 0.0, 1.0, 1.0];
                    let start = b.pos.len() as u32;
                    for i in 0..4 {
                        b.pos.push(corners[i]);
                        b.nor.push([0.0, 1.0, 0.0]);
                        b.uv.push(uvs[i]);
                        b.layer.push([pack_layer(tex, 0), pack_tint_sway(tint, sway[i])]);
                    }
                    b.idx.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
                    b.idx.extend_from_slice(&[start, start + 2, start + 1, start, start + 3, start + 2]);
                }
                fill(b);
            }
        }
    }
}

/// A vine panel, a twig or a pebble in the cell whose low corner is `cell`.
/// `key` varies the placement a little from block to block.
fn push_detail(def: &voxel::BlockDef, cell: [f32; 3], key: [u32; 3], b: &mut Buffers) {
    let h = key[0].wrapping_mul(73_856_093) ^ key[1].wrapping_mul(19_349_663) ^ key[2].wrapping_mul(83_492_791);
    let jit = |shift: u32| ((h >> shift) & 0xFF) as f32 / 255.0 - 0.5;
    let tint = if def.foliage { 1.0 } else { 0.0 };
    let side_tex = def.textures[voxel::Face::PosX as usize] as f32;
    let top_tex = def.textures[voxel::Face::PosY as usize] as f32;
    let off = def.attach.map_or([0, 0, 0], |a| a.offset());
    match def.shape {
        voxel::Shape::Vine => {
            // A panel just off the wall it clings to, seen from both sides; the
            // lower edge sways a little.
            let gap = 0.04;
            let (axis, toward) = if off[0] != 0 { (0, off[0]) } else { (2, off[2]) };
            let plane = if toward > 0 { 1.0 - gap } else { gap };
            let other = if axis == 0 { 2 } else { 0 };
            let corner = |u: f32, v: f32| {
                let mut c = cell;
                c[axis] += plane;
                c[other] += u;
                c[1] += v;
                c
            };
            let corners = [corner(0.0, 0.0), corner(1.0, 0.0), corner(1.0, 1.0), corner(0.0, 1.0)];
            let mut normal = [0.0; 3];
            normal[axis] = -(toward as f32);
            let uvs = [[0.0, 0.999], [0.999, 0.999], [0.999, 0.0], [0.0, 0.0]];
            let sway = [0.5, 0.5, 0.0, 0.0];
            let start = b.pos.len() as u32;
            for i in 0..4 {
                b.pos.push(corners[i]);
                b.nor.push(normal);
                b.uv.push(uvs[i]);
                b.layer.push([pack_layer(top_tex, 0), pack_tint_sway(tint, sway[i])]);
            }
            b.idx.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
            b.idx.extend_from_slice(&[start, start + 2, start + 1, start, start + 3, start + 2]);
        }
        voxel::Shape::Twig => {
            // A stick from the wall towards the middle of the cell, a little
            // above or below half height.
            let t = 0.13;
            let len = 0.5 + jit(0) * 0.1;
            let y = 0.45 + jit(8) * 0.3;
            let along = 0.5 + jit(16) * 0.4;
            let (mut lo, mut hi) = ([0.0; 3], [0.0; 3]);
            lo[1] = y;
            hi[1] = y + t;
            for axis in [0, 2] {
                if off[axis] != 0 {
                    // From the attached wall inwards.
                    let (a, z) = if off[axis] > 0 { (1.0 - len, 1.0) } else { (0.0, len) };
                    lo[axis] = a;
                    hi[axis] = z;
                } else {
                    lo[axis] = along - t / 2.0;
                    hi[axis] = along + t / 2.0;
                }
            }
            push_box(b, cell, lo, hi, side_tex, tint);
        }
        voxel::Shape::Pebble => {
            let (w, d, hgt) = (0.26 + jit(0) * 0.08, 0.2 + jit(8) * 0.08, 0.1 + jit(24) * 0.04);
            let (cx, cz) = (0.5 + jit(16) * 0.4, 0.5 + jit(20) * 0.4);
            push_box(b, cell, [cx - w / 2.0, 0.0, cz - d / 2.0], [cx + w / 2.0, hgt, cz + d / 2.0], top_tex, tint);
        }
        _ => {}
    }
}

/// Which sides of a cell have a wall block beside them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Walls {
    pub east: bool,
    pub west: bool,
    pub south: bool,
    pub north: bool,
}

/// The boxes (`lo`, `hi` within the cell, 0..1) a piece of furniture is made of.
/// A door spans the gap between walls on two opposite sides (east-west if
/// those are there, else north-south); open, it lies against its hinge side.
/// A chair's back goes against a wall beside it (north if none).
pub fn model_boxes(m: voxel::Model, walls: Walls) -> Vec<([f32; 3], [f32; 3])> {
    use voxel::Model;
    let leg = |x: f32, z: f32, top: f32| ([x, 0.0, z], [x + 0.12, top, z + 0.12]);
    let t = 0.14;
    // A door across x (thin in z) unless walls stand north and south only.
    let across_x = (walls.east || walls.west) || !(walls.north || walls.south);
    match m {
        Model::Table => vec![([0.0, 0.84, 0.0], [1.0, 0.98, 1.0]), leg(0.06, 0.06, 0.84), leg(0.82, 0.06, 0.84), leg(0.06, 0.82, 0.84), leg(0.82, 0.82, 0.84)],
        Model::Chair => {
            let mut v = vec![([0.14, 0.46, 0.14], [0.86, 0.56, 0.86]), leg(0.16, 0.16, 0.46), leg(0.72, 0.16, 0.46), leg(0.16, 0.72, 0.46), leg(0.72, 0.72, 0.46)];
            let back = if walls.north {
                ([0.14, 0.56, 0.14], [0.86, 1.0, 0.24])
            } else if walls.east {
                ([0.76, 0.56, 0.14], [0.86, 1.0, 0.86])
            } else if walls.south {
                ([0.14, 0.56, 0.76], [0.86, 1.0, 0.86])
            } else if walls.west {
                ([0.14, 0.56, 0.14], [0.24, 1.0, 0.86])
            } else {
                ([0.14, 0.56, 0.14], [0.86, 1.0, 0.24])
            };
            v.push(back);
            v
        }
        Model::Door if across_x => vec![([0.0, 0.0, 0.5 - t / 2.0], [1.0, 1.0, 0.5 + t / 2.0])],
        Model::Door => vec![([0.5 - t / 2.0, 0.0, 0.0], [0.5 + t / 2.0, 1.0, 1.0])],
        // Swung open on the west (or north) edge, standing along the other axis.
        Model::DoorOpen if across_x => vec![([0.0, 0.0, 0.0], [t, 1.0, 1.0])],
        Model::DoorOpen => vec![([0.0, 0.0, 0.0], [1.0, 1.0, t])],
    }
}

/// A small box inside a cell (`lo`/`hi` from 0 to 1), textured with the part of
/// the tile under each face so it looks carved from the block.
fn push_box(b: &mut Buffers, cell: [f32; 3], lo: [f32; 3], hi: [f32; 3], tex: f32, tint: f32) {
    for face in voxel::Face::ALL {
        let axis = face.axis();
        let (ua, va) = face.plane_axes();
        let at = if face.is_positive() { hi[axis] } else { lo[axis] };
        let corner = |u: f32, v: f32| {
            let mut c = [0.0; 3];
            c[axis] = at;
            c[ua] = u;
            c[va] = v;
            c
        };
        let local = [corner(lo[ua], lo[va]), corner(hi[ua], lo[va]), corner(hi[ua], hi[va]), corner(lo[ua], hi[va])];
        let normal = face.normal().map(|v| v as f32);
        let start = b.pos.len() as u32;
        for c in local {
            b.pos.push([cell[0] + c[0], cell[1] + c[1], cell[2] + c[2]]);
            b.nor.push(normal);
            // Sample the tile where this face sits (flip v so up is up on the sides).
            let v = if va == 1 { 1.0 - c[va] } else { c[va] };
            b.uv.push([c[ua].clamp(0.0, 0.999), v.clamp(0.0, 0.999)]);
            b.layer.push([pack_layer(tex, 0), pack_tint_sway(tint, 0.0)]);
        }
        // Wind counter-clockwise seen from outside: u x v points along +X, -Y
        // and +Z for the three plane layouts, so flip where the face is the other way.
        if face.is_positive() == (axis != 1) {
            b.idx.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        } else {
            b.idx.extend_from_slice(&[start, start + 2, start + 1, start, start + 3, start + 2]);
        }
    }
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
    let lit = [q.light[0], q.light[1], q.light[3], q.light[2]];
    // Leaves (see-through foliage) rustle a little.
    let sway = if def.foliage && !def.opaque { LEAF_SWAY } else { 0.0 };
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
        b.layer.push([pack_layer(tex, ao[i]), pack_tint_sway(foliage, sway)]);
        b.light.push(light_color(lit[i]));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doors_span_the_gap_between_walls_and_swing_against_the_hinge() {
        let ew = Walls { east: true, west: true, ..Walls::default() };
        let ns = Walls { north: true, south: true, ..Walls::default() };
        let door = |w| model_boxes(voxel::Model::Door, w)[0];
        // Between east and west walls: wide in x, thin in z.
        let (lo, hi) = door(ew);
        assert!(hi[0] - lo[0] > 0.9 && hi[2] - lo[2] < 0.2);
        let (lo, hi) = door(ns);
        assert!(hi[2] - lo[2] > 0.9 && hi[0] - lo[0] < 0.2);
        // Open, it turns the other way.
        let (lo, hi) = model_boxes(voxel::Model::DoorOpen, ew)[0];
        assert!(hi[2] - lo[2] > 0.9 && hi[0] - lo[0] < 0.2);
        for m in [voxel::Model::Chair, voxel::Model::Table, voxel::Model::Door, voxel::Model::DoorOpen] {
            for (lo, hi) in model_boxes(m, ew) {
                assert!((0..3).all(|k| 0.0 <= lo[k] && lo[k] < hi[k] && hi[k] <= 1.0), "{m:?}");
            }
        }
    }

    #[test]
    fn tint_and_sway_pack_and_unpack() {
        for tint in [0.0, 0.5, 1.0] {
            for k in 0..=SWAY_STEPS as u32 {
                let sway = k as f32 / SWAY_STEPS;
                let (t, s) = unpack_tint_sway(pack_tint_sway(tint, sway));
                assert!((t - tint).abs() < 1e-4 && (s - sway).abs() < 1e-4, "{tint} {sway} -> {t} {s}");
            }
        }
        assert_eq!(pack_tint_sway(0.5, 0.0), 0.5);
    }

    #[test]
    fn box_faces_point_outward() {
        let mut b = Buffers::default();
        push_box(&mut b, [3.0, 4.0, 5.0], [0.2, 0.0, 0.3], [0.6, 0.1, 0.5], 1.0, 0.0);
        assert_eq!(b.idx.len(), 36);
        for tri in b.idx.chunks(3) {
            let [a, c, d] = [tri[0], tri[1], tri[2]].map(|i| Vec3::from(b.pos[i as usize]));
            let n = (c - a).cross(d - a).normalize();
            let want = Vec3::from(b.nor[tri[0] as usize]);
            assert!(n.dot(want) > 0.99, "triangle normal {n} vs face normal {want}");
        }
    }
}
