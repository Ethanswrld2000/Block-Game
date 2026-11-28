//! Arranging an urn's stations by hand. "Arrange" in an urn's tree lifts the
//! camera out above the urn; every station that urn has placed shows as a
//! blue see-through ghost. Click one to pick it up, it follows the mouse over
//! the ground; Q / E turn it a quarter; click again to set it down. A ghost
//! goes red where it can't stand (in a wall, off the ground, on another
//! station, or too far from the urn). Enter places them all, Esc leaves them
//! as they were. The spots are saved per urn in `Urns.arranged`, in the urn's
//! own frame, so they come along if the urn is moved.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use voxel::BlockPos;

use super::{frame, ground_at, layout, models, Frame, Station, StationKind, StationModels, UrnBook, Urns};
use crate::blocks::Blocks;
use crate::camera::{FlyCamera, Menus};
use crate::interact::ray_box;
use crate::survival::camp::Camp;
use crate::survival::Toast;
use crate::theme::{colors, sizes, text, Theme};
use crate::world::World;

/// How far from the urn (blocks, across the ground) a station may stand.
pub const ARRANGE_RADIUS: f32 = 10.0;
/// Stations snap to this grid (blocks).
const SNAP: f32 = 0.5;

/// The arranging going on, if any.
#[derive(Resource, Default)]
pub struct Arrange {
    /// Asked for from the urn tree (index into urns.ron).
    pub start: Option<usize>,
    /// The urn being arranged.
    pub open: Option<usize>,
    frame: Option<Frame>,
    pieces: Vec<Piece>,
    /// The piece in hand, and where it was picked up from the cursor's ground point.
    held: Option<(usize, Vec3)>,
    /// Where the camera was, and how far it has lifted (0..1).
    home: Option<Transform>,
    view: Transform,
    pub blend: f32,
    /// Other things the pieces must not stand in.
    fixed: Vec<(Vec3, Vec3)>,
    /// Ghosts need spawning (or clearing).
    dirty: bool,
}

/// One station being moved.
#[derive(Debug, Clone)]
struct Piece {
    name: String,
    station: Station,
    fits: bool,
}

/// A ghost of piece `.0`.
#[derive(Component)]
pub(super) struct Ghost(usize);

/// One box of a ghost.
#[derive(Component)]
pub(super) struct GhostPart(usize);

/// The hint along the bottom while arranging (kept up when the HUD steps aside).
#[derive(Component)]
pub struct ArrangeUi;

/// The see-through blue (fits) and red (doesn't) the ghosts are drawn in.
#[derive(Resource)]
pub(super) struct GhostLooks {
    blue: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
}

/// Can a station with these bounds stand here: nothing solid inside it, and
/// clear of the other boxes?
pub fn fits(b: (Vec3, Vec3), others: &[(Vec3, Vec3)], solid: impl Fn(BlockPos) -> bool) -> bool {
    const EPS: f32 = 0.03;
    let (min, max) = (b.0 + Vec3::splat(EPS), b.1 - Vec3::splat(EPS));
    if others.iter().any(|(lo, hi)| min.x < hi.x && max.x > lo.x && min.y < hi.y && max.y > lo.y && min.z < hi.z && max.z > lo.z) {
        return false;
    }
    let (lo, hi) = (min.floor().as_ivec3(), max.floor().as_ivec3());
    for x in lo.x..=hi.x {
        for y in lo.y..=hi.y {
            for z in lo.z..=hi.z {
                if solid(BlockPos::new(x, y, z)) {
                    return false;
                }
            }
        }
    }
    true
}

/// Does this kind get moved round its urn? (The urn stays put, the bedframe
/// is the bedroll's, the golem walks.)
fn movable(kind: StationKind) -> bool {
    !matches!(kind, StationKind::Urn | StationKind::Bedframe | StationKind::Golem)
}

pub(super) fn spawn_arrange_ui(mut commands: Commands, theme: Res<Theme>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let ghost = |materials: &mut Assets<StandardMaterial>, c: Color| {
        materials.add(StandardMaterial {
            base_color: c,
            emissive: c.to_linear() * 0.6,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })
    };
    let blue = ghost(&mut materials, Color::srgba(0.35, 0.7, 1.0, 0.38));
    let red = ghost(&mut materials, Color::srgba(1.0, 0.25, 0.2, 0.42));
    commands.insert_resource(GhostLooks { blue, red });
    commands
        .spawn((
            ArrangeUi,
            Node { position_type: PositionType::Absolute, bottom: Val::Px(24.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn((Node { padding: UiRect::axes(Val::Px(18.0), Val::Px(8.0)), ..default() }, theme.skin("panel"))).with_children(|c| {
                c.spawn(text(
                    "Click a station to pick it up, click to set it down  ·  Q / E: turn  ·  Enter: place all  ·  Esc: cancel",
                    sizes::BODY,
                    colors::TEXT,
                ));
            });
        });
}

/// Start arranging when the tree asks for it.
#[allow(clippy::too_many_arguments)]
pub(super) fn arrange_start(
    mut arrange: ResMut<Arrange>,
    book: Option<Res<UrnBook>>,
    (camp, urns, world, blocks): (Res<Camp>, Res<Urns>, Res<World>, Res<Blocks>),
    cam: Single<&Transform, With<FlyCamera>>,
    mut menus: ResMut<Menus>,
    mut toast: MessageWriter<Toast>,
) {
    let Some(i) = arrange.start.take() else { return };
    let Some(book) = book else { return };
    let def = &book.0.urns[i];
    let Some(f) = frame(&camp, &urns, def) else { return };
    let all = layout(&camp, &world, &blocks, &urns, &book.0);
    let mut pieces = Vec::new();
    let mut fixed = Vec::new();
    for st in all {
        if st.urn == i && movable(st.kind) {
            // The step's station name (the key in urns.ron).
            let name = def
                .steps
                .iter()
                .find_map(|s| match &s.gives {
                    super::urns::Gives::Station(n) if StationKind::from_name(n) == Some(st.kind) => Some(n.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            pieces.push(Piece { name, station: st, fits: true });
        } else {
            fixed.push(st.bounds());
        }
    }
    if pieces.is_empty() {
        toast.write(Toast(format!("The {} has no stations to arrange yet", def.title)));
        return;
    }
    // Look down at the urn and its stations from the side you stood on.
    let centre = pieces.iter().map(|p| p.station.pos).fold(f.at, |a, b| a + b) / (pieces.len() + 1) as f32;
    let mut away = cam.translation - centre;
    away.y = 0.0;
    let away = away.try_normalize().unwrap_or(-f.side);
    let eye = centre + away * 7.0 + Vec3::Y * 11.0;
    arrange.view = Transform::from_translation(eye).looking_at(centre, Vec3::Y);
    if arrange.home.is_none() {
        arrange.home = Some(**cam);
    }
    arrange.open = Some(i);
    arrange.frame = Some(f);
    arrange.pieces = pieces;
    arrange.fixed = fixed;
    arrange.held = None;
    arrange.dirty = true;
    menus.open = true;
    menus.still = true;
}

/// Pick up, carry, turn and set down; Enter places all, Esc cancels.
#[allow(clippy::too_many_arguments)]
pub(super) fn arrange_input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform), With<FlyCamera>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut arrange: ResMut<Arrange>,
    book: Option<Res<UrnBook>>,
    (world, blocks): (Res<World>, Res<Blocks>),
    mut urns: ResMut<Urns>,
    mut menus: ResMut<Menus>,
    mut toast: MessageWriter<Toast>,
) {
    let (Some(i), Some(f), Some(book)) = (arrange.open, arrange.frame, book) else { return };
    if !cursor.visible {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
    let close = |arrange: &mut Arrange, menus: &mut Menus, cursor: &mut CursorOptions| {
        arrange.open = None;
        arrange.frame = None;
        arrange.pieces.clear();
        arrange.held = None;
        arrange.dirty = true;
        menus.open = false;
        menus.still = false;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    };
    if keys.just_pressed(KeyCode::Escape) {
        // Used up here, so the pause menu doesn't open too.
        keys.clear_just_pressed(KeyCode::Escape);
        close(&mut arrange, &mut menus, &mut cursor);
        return;
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        if arrange.held.is_some() || arrange.pieces.iter().any(|p| !p.fits) {
            toast.write(Toast("Set every station down somewhere it fits first".into()));
        } else {
            let name = book.0.urns[i].name.clone();
            let spots = arrange.pieces.iter().map(|p| (p.name.clone(), f.arranged(p.station.pos, p.station.turns))).collect();
            urns.arranged.insert(name, spots);
            close(&mut arrange, &mut menus, &mut cursor);
        }
        return;
    }
    // Where the mouse points: a ray from the camera.
    let (camera, gt) = *cam;
    let ray = window.cursor_position().and_then(|p| camera.viewport_to_world(gt, p).ok());
    let on_ground = |ray: Ray3d| -> Option<Vec3> {
        // Across the plane at the urn's foot.
        let d = ray.direction.as_vec3();
        if d.y.abs() < 1e-4 {
            return None;
        }
        let t = (f.at.y - ray.origin.y) / d.y;
        (t > 0.0).then(|| ray.origin + d * t)
    };
    let turn = i32::from(keys.just_pressed(KeyCode::KeyE)) - i32::from(keys.just_pressed(KeyCode::KeyQ));
    match arrange.held {
        Some((k, grab)) => {
            if let Some(g) = ray.and_then(on_ground) {
                let mut at = g + grab;
                at.x = (at.x / SNAP).round() * SNAP;
                at.z = (at.z / SNAP).round() * SNAP;
                at.y = f.at.y;
                // Stand it on the ground there (or leave it level, red, over a gap).
                arrange.pieces[k].station.pos = ground_at(&world, &blocks, at).unwrap_or(at);
            }
            if turn != 0 {
                let t = &mut arrange.pieces[k].station.turns;
                *t = ((*t as i32 + turn).rem_euclid(4)) as u8;
            }
            if mouse.just_pressed(MouseButton::Left) {
                if arrange.pieces[k].fits {
                    arrange.held = None;
                } else {
                    toast.write(Toast("It doesn't fit there".into()));
                }
            }
        }
        None => {
            let hit = ray.and_then(|r| {
                let (o, d) = (r.origin, r.direction.as_vec3());
                arrange.pieces.iter().enumerate().filter_map(|(k, p)| ray_box(o, d, p.station.bounds().0, p.station.bounds().1).map(|t| (k, t))).min_by(|a, b| a.1.total_cmp(&b.1))
            });
            if let Some((k, _)) = hit {
                if turn != 0 {
                    let t = &mut arrange.pieces[k].station.turns;
                    *t = ((*t as i32 + turn).rem_euclid(4)) as u8;
                }
                if mouse.just_pressed(MouseButton::Left) {
                    let g = ray.and_then(on_ground).unwrap_or(arrange.pieces[k].station.pos);
                    let mut grab = arrange.pieces[k].station.pos - g;
                    grab.y = 0.0;
                    arrange.held = Some((k, grab));
                }
            }
        }
    }
    // Which pieces fit where they are now.
    let solid = |p: BlockPos| world.block(p).is_some_and(|b| blocks.registry.get(b).solid);
    let boxes: Vec<(Vec3, Vec3)> = arrange.pieces.iter().map(|p| p.station.bounds()).collect();
    let fitting: Vec<bool> = arrange
        .pieces
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let mut far = p.station.pos - f.at;
            far.y = 0.0;
            let others: Vec<(Vec3, Vec3)> = arrange.fixed.iter().copied().chain(boxes.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, b)| *b)).collect();
            far.length() <= ARRANGE_RADIUS
                && ground_at(&world, &blocks, p.station.pos).is_some_and(|g| (g.y - p.station.pos.y).abs() < 1e-3)
                && fits(boxes[k], &others, solid)
        })
        .collect();
    for (p, ok) in arrange.pieces.iter_mut().zip(fitting) {
        p.fits = ok;
    }
}

/// Lift the camera out above the urn, and back down to your eyes after.
pub(super) fn arrange_camera(time: Res<Time<Real>>, mut arrange: ResMut<Arrange>, mut cam: Single<&mut Transform, With<FlyCamera>>) {
    let Some(home) = arrange.home else { return };
    let want = if arrange.open.is_some() { 1.0 } else { 0.0 };
    let dt = time.delta_secs().min(0.1);
    let blend = if (arrange.blend - want).abs() < 1e-3 { want } else { arrange.blend + (want - arrange.blend) * (1.0 - (-dt * 5.0).exp()) };
    arrange.blend = blend;
    let k = blend * blend * (3.0 - 2.0 * blend);
    cam.translation = home.translation.lerp(arrange.view.translation, k);
    cam.rotation = home.rotation.slerp(arrange.view.rotation, k);
    if blend <= 0.0 {
        arrange.home = None;
    }
}

/// Draw the ghosts, hide the real stations while they're being moved, show the hint.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn arrange_ghosts(
    mut commands: Commands,
    mut arrange: ResMut<Arrange>,
    looks: Option<Res<GhostLooks>>,
    urns: Res<Urns>,
    mut meshes: ResMut<Assets<Mesh>>,
    ghosts: Query<Entity, With<Ghost>>,
    mut roots: Query<(&Ghost, &mut Transform)>,
    mut parts: Query<(&GhostPart, &mut MeshMaterial3d<StandardMaterial>)>,
    mut real: Query<(&Station, &mut Visibility), (With<StationModels>, Without<ArrangeUi>)>,
    mut hint: Query<&mut Visibility, (With<ArrangeUi>, Without<StationModels>)>,
) {
    let Some(looks) = looks else { return };
    if arrange.dirty {
        arrange.dirty = false;
        for e in &ghosts {
            commands.entity(e).despawn();
        }
        let has = |s: &str| urns.has(s);
        for (k, p) in arrange.pieces.iter().enumerate() {
            let st = p.station;
            commands
                .spawn((Ghost(k), Transform::from_translation(st.pos).with_rotation(st.rotation()), Visibility::default()))
                .with_children(|c| {
                    // The bench's own boxes, then its tool where it rests.
                    let mut boxes: Vec<(models::Part, Vec3, Quat)> = models::parts(st.kind, &has, 0).into_iter().map(|b| (b, Vec3::ZERO, Quat::IDENTITY)).collect();
                    if let Some(tool) = models::tool(st.kind) {
                        boxes.extend(tool.parts.into_iter().map(|b| (b, tool.rest, tool.rest_turn)));
                    }
                    for (b, at, turn) in boxes {
                        c.spawn((
                            GhostPart(k),
                            Mesh3d(meshes.add(Cuboid::new(b.size.x, b.size.y, b.size.z))),
                            MeshMaterial3d(looks.blue.clone()),
                            Transform::from_translation(at + turn * b.at).with_rotation(turn * Quat::from_rotation_z(b.tilt)),
                            NotShadowCaster,
                        ));
                    }
                });
        }
    }
    let held = arrange.held.map(|h| h.0);
    for (g, mut tf) in &mut roots {
        let Some(p) = arrange.pieces.get(g.0) else { continue };
        // The one in hand hangs a little above the ground.
        let lift = if held == Some(g.0) { 0.15 } else { 0.0 };
        tf.translation = p.station.pos + Vec3::Y * lift;
        tf.rotation = p.station.rotation();
    }
    for (g, mut mat) in &mut parts {
        let Some(p) = arrange.pieces.get(g.0) else { continue };
        let want = if p.fits { &looks.blue } else { &looks.red };
        if mat.0 != *want {
            mat.0 = want.clone();
        }
    }
    for (st, mut vis) in &mut real {
        let want = if arrange.open == Some(st.urn) && movable(st.kind) { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
    }
    let want = if arrange.open.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut hint {
        if *v != want {
            *v = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(pos: Vec3, turns: u8) -> Station {
        Station { kind: StationKind::ToolBench, pos, turns, urn: 0 }
    }

    #[test]
    fn a_station_fits_on_open_ground_only() {
        let floor = |p: BlockPos| p.y < 0;
        let b = st(Vec3::new(2.5, 0.0, 2.5), 0).bounds();
        assert!(fits(b, &[], floor));
        // A wall block inside it.
        assert!(!fits(b, &[], |p: BlockPos| p.y < 0 || p == BlockPos::new(2, 0, 2)));
        // Sunk into the ground.
        assert!(!fits(st(Vec3::new(2.5, -0.5, 2.5), 0).bounds(), &[], floor));
    }

    #[test]
    fn stations_dont_stand_in_each_other() {
        let floor = |p: BlockPos| p.y < 0;
        let a = st(Vec3::new(0.0, 0.0, 0.0), 0).bounds();
        assert!(!fits(st(Vec3::new(0.5, 0.0, 0.0), 0).bounds(), &[a], floor));
        assert!(fits(st(Vec3::new(1.5, 0.0, 0.0), 0).bounds(), &[a], floor));
        // Touching side to side is fine.
        let w = models::half(StationKind::ToolBench).x * 2.0;
        assert!(fits(st(Vec3::new(w, 0.0, 0.0), 0).bounds(), &[a], floor));
    }

    #[test]
    fn a_quarter_turn_swaps_the_footprint() {
        let h = models::half(StationKind::ToolBench);
        let (lo, hi) = st(Vec3::ZERO, 1).bounds();
        assert!((hi.x - lo.x - h.z * 2.0).abs() < 1e-5);
        assert!((hi.z - lo.z - h.x * 2.0).abs() < 1e-5);
    }

    #[test]
    fn arranged_spots_round_trip_through_the_urn_frame() {
        let f = Frame { at: Vec3::new(10.0, 4.0, -3.0), along: Vec3::Z, side: Vec3::NEG_X, turns: 1 };
        let a = f.arranged(Vec3::new(8.0, 4.0, 0.5), 3);
        let (pos, turns) = f.spot(&a);
        assert!((pos - Vec3::new(8.0, 4.0, 0.5)).length() < 1e-5);
        assert_eq!(turns, 3);
        assert_eq!(a.turns, 2);
    }
}
