//! The puzzle room of each floor: a gate (drawn shut, a slab) in front of a
//! small vault with a chest. Three kinds (`worldgen::delve::PuzzleKind`):
//!
//! - Plates: pressure plates in the floor and loose stone blocks. A plate is
//!   down while a stone, you or a foe stands on it; all down opens the gate.
//!   Lean into a stone (walk against it) or kick it to shove it one block.
//! - Levers: lamps over the gate, levers on the far wall. Pulling a lever
//!   (right click it) flips the lamps it's tied to; all lit opens the gate.
//! - Timed: one lever opens the gate for a few seconds, past spikes.
//!
//! Solved puzzles stay open for the rest of the floor.

use bevy::prelude::*;
use voxel::BlockId;
use worldgen::delve::{Puzzle, PuzzleKind};

use super::{Below, DelveSet};
use crate::camera::FlyCamera;
use crate::combat::kick::Kicking;
use crate::combat::Foe;
use crate::player::Player;
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};
use crate::survival::Toast;

pub struct PuzzlePlugin;

impl Plugin for PuzzlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PuzzleState>().add_systems(crate::menu::GameUpdate, (build, plates, levers, lamps).chain().after(DelveSet));
    }
}

/// Seconds of leaning on a stone before it moves.
const LEAN: f32 = 0.35;
/// How near (blocks) you must be to pull a lever.
const REACH: f32 = 2.6;

#[derive(Resource, Default)]
pub struct PuzzleState {
    on: Option<(u32, u8)>,
    /// Where the stones are now.
    stones: Vec<[i32; 3]>,
    /// Lamps lit (bit per lamp).
    lit: u8,
    solved: bool,
    /// Timed: seconds the gate stays open still.
    open: f32,
    lean: f32,
    parts: Vec<Entity>,
    /// Lamp entities, in order.
    lamps: Vec<Entity>,
}

/// After pulling lever `i`, the lamps lit (pure).
pub fn pull(p: &Puzzle, lit: u8, i: usize) -> u8 {
    lit ^ p.flips.get(i).copied().unwrap_or(0)
}

/// Every plate has something on it (pure).
pub fn plates_down(p: &Puzzle, weights: &[[i32; 3]]) -> bool {
    !p.plates.is_empty() && p.plates.iter().all(|q| weights.contains(q))
}

/// Where a stone goes when shoved one block that way, if the cell is free (pure).
pub fn shove(from: [i32; 3], dir: (i32, i32), free: impl Fn([i32; 3]) -> bool) -> Option<[i32; 3]> {
    let to = [from[0] + dir.0, from[1], from[2] + dir.1];
    free(to).then_some(to)
}

#[derive(Component)]
struct LampLook(bool);

#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    mut below: ResMut<Below>,
    mut state: ResMut<PuzzleState>,
    blocks: Res<crate::blocks::Blocks>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
) {
    let here = below.run.map(|r| (r.number, r.floor));
    if state.on == here {
        return;
    }
    for e in state.parts.drain(..) {
        commands.entity(e).despawn();
    }
    *state = PuzzleState { on: here, ..default() };
    let Some(p) = below.floor.as_ref().and_then(|f| f.puzzle.clone()) else { return };
    state.lit = p.start;
    state.stones = p.stones.clone();
    if let Some(stone) = blocks.registry.id("dungeon_slab") {
        for s in &p.stones {
            below.edit(*s, stone);
        }
    }
    let wood = materials.add(StandardMaterial { base_color: Color::srgb(0.42, 0.28, 0.16), ..default() });
    let iron = materials.add(StandardMaterial { base_color: Color::srgb(0.35, 0.35, 0.38), metallic: 0.7, ..default() });
    for l in &p.levers {
        let at = Vec3::new(l[0] as f32 + 0.5, l[1] as f32, l[2] as f32 + 0.5);
        let e = commands
            .spawn((Transform::from_translation(at), Visibility::default()))
            .with_children(|c| {
                c.spawn((Mesh3d(meshes.add(Cuboid::new(0.5, 0.3, 0.5))), MeshMaterial3d(iron.clone()), Transform::from_xyz(0.0, 0.15, 0.0)));
                c.spawn((Mesh3d(meshes.add(Cuboid::new(0.08, 0.9, 0.08))), MeshMaterial3d(wood.clone()), Transform::from_xyz(0.0, 0.6, 0.0).with_rotation(Quat::from_rotation_z(0.5))));
            })
            .id();
        state.parts.push(e);
    }
    let lamp = meshes.add(Sphere::new(0.22));
    for (i, l) in p.lamps.iter().enumerate() {
        let on = p.start & (1 << i) != 0;
        let mat = materials.add(StandardMaterial { base_color: Color::srgb(0.3, 0.28, 0.22), ..default() });
        let e = commands
            .spawn((
                LampLook(on),
                Mesh3d(lamp.clone()),
                MeshMaterial3d(mat),
                Transform::from_xyz(l[0] as f32 + 0.5, l[1] as f32 + 0.5, l[2] as f32 + 0.5),
                PointLight { color: Color::srgb(1.0, 0.8, 0.4), intensity: 0.0, range: 4.0, shadow_maps_enabled: false, ..default() },
            ))
            .id();
        state.parts.push(e);
        state.lamps.push(e);
    }
    let _ = PuzzleKind::Plates;
}

fn open_gate(below: &mut Below, p: &Puzzle) {
    below.edit_box(&p.gate, BlockId::AIR);
}

fn shut_gate(below: &mut Below, p: &Puzzle, slab: Option<BlockId>) {
    if let Some(s) = slab {
        below.edit_box(&p.gate, s);
    }
}

/// Plates and the stones pushed onto them.
#[allow(clippy::too_many_arguments)]
fn plates(
    time: Res<Time>,
    mut below: ResMut<Below>,
    mut state: ResMut<PuzzleState>,
    player: Res<Player>,
    blocks: Res<crate::blocks::Blocks>,
    world: Res<crate::world::World>,
    kicking: Res<Kicking>,
    cam: Single<&Transform, With<FlyCamera>>,
    foes: Query<&Transform, (With<Foe>, Without<FlyCamera>)>,
    (mut sound, mut toast, mut bursts): (MessageWriter<PlaySound>, MessageWriter<Toast>, MessageWriter<Burst>),
    mut kick_before: Local<Option<f32>>,
) {
    let kicked = kicking.t.is_some_and(|t| t >= 0.4) && kick_before.is_none_or(|b| b < 0.4);
    *kick_before = kicking.t;
    let Some(p) = below.floor.as_ref().and_then(|f| f.puzzle.clone()) else { return };
    if p.kind != PuzzleKind::Plates || state.solved {
        return;
    }
    let (Some(stone), Some(plate)) = (blocks.registry.id("dungeon_slab"), blocks.registry.id("pressure_plate")) else { return };
    let feet = player.pos;
    let cell = |v: Vec3| [v.x.floor() as i32, (v.y + 0.2).floor() as i32, v.z.floor() as i32];
    // Lean on a stone (walk into it) or kick it: it moves one block away from you.
    let fwd = cam.forward().as_vec3();
    let dir = if fwd.x.abs() > fwd.z.abs() { (fwd.x.signum() as i32, 0) } else { (0, fwd.z.signum() as i32) };
    let ahead = cell(feet + Vec3::new(dir.0 as f32, 0.0, dir.1 as f32) * 0.8);
    let leaning = state.stones.iter().position(|s| *s == ahead).filter(|_| player.vel.xz().length() < 1.0 && Vec2::new(player.vel.x, player.vel.z).dot(Vec2::new(dir.0 as f32, dir.1 as f32)) >= 0.0);
    let kick_hit = kicked.then(|| state.stones.iter().position(|s| *s == ahead || *s == cell(feet + Vec3::new(dir.0 as f32, 0.0, dir.1 as f32) * 1.6))).flatten();
    if leaning.is_some() && kick_hit.is_none() {
        state.lean += time.delta_secs();
    } else {
        state.lean = 0.0;
    }
    let push = kick_hit.or(leaning.filter(|_| state.lean >= LEAN));
    if let Some(i) = push {
        state.lean = 0.0;
        let from = state.stones[i];
        let room = below.floor.as_ref().map(|f| f.rooms[p.room].inner);
        let free = |q: [i32; 3]| {
            room.is_some_and(|r| r.contains(q[0], q[1], q[2]))
                && world.block(voxel::BlockPos::new(q[0], q[1], q[2])).is_some_and(|b| b == BlockId::AIR || b == plate)
                && world.block(voxel::BlockPos::new(q[0], q[1] - 1, q[2])).is_some_and(|b| blocks.registry.get(b).solid)
        };
        if let Some(to) = shove(from, dir, free) {
            state.stones[i] = to;
            below.edit(from, if p.plates.contains(&from) { plate } else { BlockId::AIR });
            below.edit(to, stone);
            let at = Vec3::new(to[0] as f32 + 0.5, to[1] as f32, to[2] as f32 + 0.5);
            sound.write(PlaySound::new("dig_hard").at(at).speed(0.6));
            bursts.write(Burst::new(at, BurstKind::Chips, 4));
        }
    }
    // Weights on the plates: stones, you, foes.
    let mut weights = state.stones.clone();
    weights.push(cell(feet));
    weights.extend(foes.iter().map(|tf| cell(tf.translation)));
    if plates_down(&p, &weights) {
        state.solved = true;
        open_gate(&mut below, &p);
        sound.write(PlaySound::new("craft_pop").speed(0.7).gain(1.1));
        toast.write(Toast("Every plate sinks. The gate grinds open".into()));
    }
}

/// Levers (and the timed gate).
#[allow(clippy::too_many_arguments)]
fn levers(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    menus: Res<crate::camera::Menus>,
    mut below: ResMut<Below>,
    mut state: ResMut<PuzzleState>,
    player: Res<Player>,
    blocks: Res<crate::blocks::Blocks>,
    cam: Single<&Transform, With<FlyCamera>>,
    (mut sound, mut toast): (MessageWriter<PlaySound>, MessageWriter<Toast>),
) {
    let Some(p) = below.floor.as_ref().and_then(|f| f.puzzle.clone()) else { return };
    if p.kind == PuzzleKind::Plates || state.solved {
        return;
    }
    let slab = blocks.registry.id("dungeon_slab");
    // Timed: the gate shuts again unless you made it through.
    if p.kind == PuzzleKind::Timed && state.open > 0.0 {
        state.open -= time.delta_secs();
        let past = p.vault.grow(1).contains(player.pos.x.floor() as i32, player.pos.y.floor() as i32, player.pos.z.floor() as i32);
        if past {
            state.solved = true;
            toast.write(Toast("You made it through".into()));
        } else if state.open <= 0.0 {
            shut_gate(&mut below, &p, slab);
            sound.write(PlaySound::new("hit").speed(0.45).gain(1.1));
            toast.write(Toast("Too slow. The gate drops shut".into()));
        }
    }
    if !mouse.just_pressed(MouseButton::Right) || menus.open {
        return;
    }
    let eye = cam.translation;
    let look = cam.forward().as_vec3();
    let Some(i) = p.levers.iter().position(|l| {
        let at = Vec3::new(l[0] as f32 + 0.5, l[1] as f32 + 0.6, l[2] as f32 + 0.5);
        at.distance(eye) < REACH + 1.0 && (at - eye).normalize_or_zero().dot(look) > 0.85
    }) else {
        return;
    };
    sound.write(PlaySound::new("dig_hard").speed(1.3).gain(0.7));
    match p.kind {
        PuzzleKind::Levers => {
            state.lit = pull(&p, state.lit, i);
            if state.lit == p.all_lit() {
                state.solved = true;
                open_gate(&mut below, &p);
                sound.write(PlaySound::new("craft_pop").speed(0.7).gain(1.1));
                toast.write(Toast("Every lamp burns. The gate grinds open".into()));
            }
        }
        PuzzleKind::Timed => {
            if state.open <= 0.0 {
                state.open = p.open_for;
                open_gate(&mut below, &p);
                toast.write(Toast(format!("The gate opens. Run! {:.0} seconds", p.open_for)));
            }
        }
        PuzzleKind::Plates => {}
    }
}

/// Lamps show which are lit.
fn lamps(state: Res<PuzzleState>, mut q: Query<(&mut LampLook, &mut PointLight, &MeshMaterial3d<StandardMaterial>)>, mut materials: ResMut<Assets<StandardMaterial>>) {
    if !state.is_changed() {
        return;
    }
    for (i, e) in state.lamps.iter().enumerate() {
        let on = state.lit & (1 << i) != 0 || state.solved;
        let Ok((mut look, mut light, mat)) = q.get_mut(*e) else { continue };
        look.0 = on;
        light.intensity = if on { 30_000.0 } else { 0.0 };
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.base_color = if on { Color::srgb(1.0, 0.85, 0.45) } else { Color::srgb(0.3, 0.28, 0.22) };
            m.emissive = if on { LinearRgba::rgb(2.0, 1.4, 0.4) } else { LinearRgba::BLACK };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn puzzle(kind: PuzzleKind) -> Puzzle {
        let gate = worldgen::Aabb { min: [0, 0, 0], max: [0, 1, 0] };
        Puzzle { room: 0, kind, plates: vec![[1, 0, 1], [3, 0, 1]], stones: vec![[1, 0, 3]], levers: vec![[0, 0, 5]; 3], flips: vec![0b011, 0b110, 0b100], start: 0b001, lamps: vec![[0, 3, 0]; 3], gate, vault: gate, open_for: 3.0 }
    }

    #[test]
    fn levers_flip_their_lamps() {
        let p = puzzle(PuzzleKind::Levers);
        let lit = pull(&p, p.start, 0);
        assert_eq!(lit, 0b010);
        assert_eq!(pull(&p, lit, 0), p.start, "pulling twice undoes it");
    }

    #[test]
    fn plates_need_a_weight_each_and_stones_stop_at_walls() {
        let p = puzzle(PuzzleKind::Plates);
        assert!(!plates_down(&p, &[[1, 0, 1]]));
        assert!(plates_down(&p, &[[1, 0, 1], [3, 0, 1], [9, 9, 9]]));
        assert_eq!(shove([1, 0, 3], (0, -1), |_| true), Some([1, 0, 2]));
        assert_eq!(shove([1, 0, 3], (0, -1), |_| false), None);
    }
}
