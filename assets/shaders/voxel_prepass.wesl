// Block terrain prepass (shadow) vertex shader: Bevy's prepass vertex shader
// with the same wind sway as voxel_vertex.wesl, so swaying leaves don't fall
// in and out of their own shadow. Keep `sway_offset` and `shake_offset` in step with that file.

import bevy_pbr::{
    prepass::io::{Vertex, VertexOutput, decompress_vertex},
    render::{
        mesh_functions,
        view_transformations::position_world_to_clip,
    },
};
import bevy_render::globals::Globals;

// In the prepass, binding 1 of the view group is the globals.
@group(0) @binding(1) var<uniform> globals: Globals;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var<uniform> wind: vec4<f32>;
// A tree shaking from an axe blow (`chop`): xyz = the block hit, w = how hard (1 -> 0).
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var<uniform> shake: vec4<f32>;

const SWAY_STEPS: f32 = 8.0;

fn sway_offset(p: vec3<f32>, s: f32, t: f32) -> vec3<f32> {
    if (s <= 0.0) {
        return vec3<f32>(0.0);
    }
    let dir = vec3<f32>(wind.x, 0.0, wind.y);
    let strength = wind.z;
    let along = dot(p.xz, wind.xy);
    let wave = sin(along * 0.25 - t * (0.8 + strength * 1.6)) * 0.5 + 0.5;
    let lean = strength * (0.12 + 0.3 * wave);
    let ph = p.x * 1.7 + p.z * 1.3 + p.y * 0.9;
    let flutter = vec3<f32>(sin(t * 5.3 + ph), sin(t * 4.1 + ph * 1.3) * 0.4, cos(t * 6.1 + ph * 0.7))
        * (0.025 + 0.05 * strength);
    var off = (dir * lean + flutter) * s;
    off.y -= dot(off.xz, off.xz) * 0.6;
    return off;
}

// How far a vertex moves while a tree shakes: everything above the blow and
// near it rocks back and forth (more the higher it is), leaves shiver too.
fn shake_offset(p: vec3<f32>, s: f32, t: f32) -> vec3<f32> {
    let amt = shake.w;
    if (amt <= 0.0) {
        return vec3<f32>(0.0);
    }
    let r = length(p.xz - shake.xz);
    let h = p.y - shake.y;
    if (r > 5.5 || h < 0.0) {
        return vec3<f32>(0.0);
    }
    let up = clamp(h / 7.0, 0.0, 1.0);
    let near = 1.0 - smoothstep(3.0, 5.5, r);
    let ph = p.x * 2.3 + p.z * 1.9 + p.y * 1.1;
    let rock = vec3<f32>(sin(t * 26.0), 0.0, cos(t * 21.0)) * 0.08 * up;
    let shiver = vec3<f32>(sin(t * 43.0 + ph), sin(t * 37.0 + ph * 1.3) * 0.5, cos(t * 47.0 + ph * 0.7)) * 0.12 * s;
    return (rock + shiver) * amt * amt * near;
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);

    var world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    var s = 0.0;
@if(VERTEX_UVS_B) {
    let k = floor(vertex.uv_b.y * 0.5 + 0.001);
    s = k / SWAY_STEPS;
    world = vec4<f32>(world.xyz + sway_offset(world.xyz, s, globals.time), world.w);
}
    world = vec4<f32>(world.xyz + shake_offset(world.xyz, s, globals.time), world.w);
    out.world_position = world;
    out.position = position_world_to_clip(world.xyz);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}

@if(VERTEX_UVS_A)
    out.uv = vertex.uv;
@if(VERTEX_UVS_B) {
    let kk = floor(vertex.uv_b.y * 0.5 + 0.001);
    out.uv_b = vec2<f32>(vertex.uv_b.x, vertex.uv_b.y - 2.0 * kk);
}

@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS && VERTEX_NORMALS)
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_in.instance_index);

@if(MOTION_VECTOR_PREPASS)
    out.previous_world_position = world;

@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;

@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_in.instance_index, world_from_local[3]);

    return out;
}
