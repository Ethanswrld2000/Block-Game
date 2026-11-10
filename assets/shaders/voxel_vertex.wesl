// Block terrain vertex shader: Bevy's mesh vertex shader, plus wind. Leaves
// and the tops of plants sway: they lean along the wind, a wave of gusts runs
// across the land in the wind's direction, and they flutter a little.
//
// uv_b.y comes in as tint + 2 * k (see `chunk_mesh::pack_tint_sway`), where
// k / 8 is how far this vertex sways; it goes out as the plain tint, so the
// fragment shader (voxel.wesl) sees what it always did. The sway itself is a
// function of world position and time, so corners shared by neighbouring
// leaf faces move together and never crack apart. voxel_prepass.wesl does
// the same for shadows; keep `sway_offset` in step with it.

import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::globals,
};

// xy: wind direction (x, z); z: how hard to sway (0..~1.3).
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> wind: vec4<f32>;

const SWAY_STEPS: f32 = 8.0;

// How far a vertex at world position `p` with sway `s` (0..1) moves at time `t`.
fn sway_offset(p: vec3<f32>, s: f32, t: f32) -> vec3<f32> {
    if (s <= 0.0) {
        return vec3<f32>(0.0);
    }
    let dir = vec3<f32>(wind.x, 0.0, wind.y);
    let strength = wind.z;
    // A wave of gusts rolling across the land with the wind.
    let along = dot(p.xz, wind.xy);
    let wave = sin(along * 0.25 - t * (0.8 + strength * 1.6)) * 0.5 + 0.5;
    let lean = strength * (0.12 + 0.3 * wave);
    // Quick flutter, different for every spot.
    let ph = p.x * 1.7 + p.z * 1.3 + p.y * 0.9;
    let flutter = vec3<f32>(sin(t * 5.3 + ph), sin(t * 4.1 + ph * 1.3) * 0.4, cos(t * 6.1 + ph * 0.7))
        * (0.025 + 0.05 * strength);
    var off = (dir * lean + flutter) * s;
    // Bending keeps the stem about the same length: the tip dips as it leans.
    off.y -= dot(off.xz, off.xz) * 0.6;
    return off;
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);

    let packed = vertex.uv_b.y;
    let k = floor(packed * 0.5 + 0.001);
    let tint = packed - 2.0 * k;

    var world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    world = vec4<f32>(world.xyz + sway_offset(world.xyz, k / SWAY_STEPS, globals.time), world.w);
    out.world_position = world;
    out.position = position_world_to_clip(world.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_in.instance_index);
    out.uv = vertex.uv;
    out.uv_b = vec2<f32>(vertex.uv_b.x, tint);

@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;

@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index, world_from_local[3]);

    return out;
}
