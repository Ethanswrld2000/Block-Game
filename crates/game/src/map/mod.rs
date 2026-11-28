//! The map (M) and waypoints.
//!
//! M opens a top-down map of the land round you: 2048 blocks across, painted
//! from the world generator (`paint`, pure, run on the `AsyncComputeTaskPool`)
//! in each biome's colour, lighter on high ground, blue where there is water
//! and dark over the void. It is painted again when you open it more than a
//! quarter of its width away from where it was last painted. Your place is the
//! red mark; waypoints are the gold ones.
//!
//! On the map: click to put a waypoint there, Space marks where you stand,
//! right click a waypoint to take it away. Out in the world, waypoints in front
//! of you show along the top of the screen with their distance.
//! Waypoints are saved to `waypoints.ron`. `BLOCKGAME_MAP=1` opens the map for screenshots.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use serde::{Deserialize, Serialize};
use worldgen::{Land, WorldGen};

use crate::camera::{FlyCamera, Menus};
use crate::player::Player;
use crate::save::{SaveDir, read_ron, screenshot_run, write_ron};
use crate::theme::{Theme, colors, sizes, text};
use crate::world::Generator;

/// Pixels across the map picture, and blocks per pixel.
pub const PIXELS: u32 = 256;
pub const STEP: i32 = 8;
/// Blocks across the map.
pub const SPAN: f32 = (PIXELS as i32 * STEP) as f32;
/// How big the map is drawn on screen, % of the window's height.
const SHOWN: f32 = 72.0;
/// Waypoints show along the top of the screen within this angle of where you look (degrees).
const STRIP_ANGLE: f32 = 60.0;
const FILE: &str = "waypoints.ron";

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Waypoints>()
            .init_resource::<MapView>()
            .add_systems(crate::menu::GameStartup, spawn_ui)
            .add_systems(crate::menu::GamePostStartup, load)
            .add_systems(
                crate::menu::GameUpdate,
                (open_close, paint_map, place_marks, click_map, strip)
                    .chain()
                    .after(crate::player::PlayerSet),
            )
            .add_systems(crate::menu::GameUpdate, save_now_and_then)
            .add_systems(Last, save_on_exit.run_if(crate::menu::in_game));
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waypoint {
    pub name: String,
    pub pos: [f32; 3],
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Waypoints {
    pub list: Vec<Waypoint>,
    /// Numbers the next waypoint's name.
    pub next: u32,
    /// Where the guide points (nearest sticks, stones...): not saved, worked out again.
    #[serde(skip)]
    pub guide: Vec<Waypoint>,
}

impl Waypoints {
    pub fn add(&mut self, pos: Vec3) {
        self.next += 1;
        self.list.push(Waypoint {
            name: format!("Waypoint {}", self.next),
            pos: pos.to_array(),
        });
    }

    /// The waypoint nearest `at` (flat), within `reach` blocks.
    pub fn nearest(&self, at: Vec2, reach: f32) -> Option<usize> {
        self.list
            .iter()
            .enumerate()
            .map(|(i, w)| (i, Vec2::new(w.pos[0], w.pos[2]).distance(at)))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

/// One map pixel's colour (RGBA) for a column of the world.
pub fn pixel(g: &WorldGen, x: i32, z: i32) -> [u8; 4] {
    let c = g.column(x, z);
    if c.land == Land::Void {
        return [16, 18, 26, 255];
    }
    if c.water > c.height {
        let depth = ((c.water - c.height) as f32 / 24.0).clamp(0.0, 1.0);
        return [
            (40.0 - 20.0 * depth) as u8,
            (90.0 - 40.0 * depth) as u8,
            (150.0 - 30.0 * depth) as u8,
            255,
        ];
    }
    let b = g.biomes.get(c.biome);
    let shade = (0.75 + c.height as f32 / 300.0).clamp(0.45, 1.35);
    let ch = |k: usize| (b.fog[k] * 255.0 * shade).clamp(0.0, 255.0) as u8;
    [ch(0), ch(1), ch(2), 255]
}

/// The map picture round (`cx`, `cz`): `PIXELS` x `PIXELS`, north (-z) up, RGBA.
pub fn paint(g: &WorldGen, cx: i32, cz: i32) -> Vec<u8> {
    let half = PIXELS as i32 * STEP / 2;
    let mut out = Vec::with_capacity((PIXELS * PIXELS * 4) as usize);
    for py in 0..PIXELS as i32 {
        for px in 0..PIXELS as i32 {
            out.extend_from_slice(&pixel(
                g,
                cx - half + px * STEP + STEP / 2,
                cz - half + py * STEP + STEP / 2,
            ));
        }
    }
    out
}

/// Where a world point falls on the map, 0..1 across and down (outside = off the map).
pub fn on_map(center: Vec2, at: Vec2) -> Vec2 {
    (at - center) / SPAN + Vec2::splat(0.5)
}

/// The world point under a spot on the map (0..1 across and down).
pub fn from_map(center: Vec2, uv: Vec2) -> Vec2 {
    center + (uv - Vec2::splat(0.5)) * SPAN
}

/// Angle (degrees, + to the right) of `to` from where you look, `yaw` as `FlyCamera.yaw`.
pub fn bearing(from: Vec3, yaw: f32, to: Vec3) -> f32 {
    let d = Vec2::new(to.x - from.x, to.z - from.z);
    let fwd = Vec2::new(-yaw.sin(), -yaw.cos());
    let right = Vec2::new(-fwd.y, fwd.x);
    right.dot(d).atan2(fwd.dot(d)).to_degrees()
}

#[derive(Resource, Default)]
struct MapView {
    open: bool,
    /// The centre of the painted picture (x, z), if any.
    center: Option<Vec2>,
    image: Option<Handle<Image>>,
    task: Option<Task<(Vec2, Vec<u8>)>>,
}

#[derive(Component)]
struct MapUi;
#[derive(Component)]
struct MapPicture;
#[derive(Component)]
struct MapMarks;
#[derive(Component)]
struct MapNote;
#[derive(Component)]
struct Strip;

/// The column beside the map where `journey` lists the region goals.
#[derive(Component)]
pub struct GoalsSlot;

fn spawn_ui(mut commands: Commands, theme: Res<Theme>) {
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            Visibility::Hidden,
            GlobalZIndex(30),
            MapUi,
        ))
        .with_children(|root| {
            root.spawn((Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(20.0), Val::Px(16.0)), row_gap: Val::Px(10.0), ..default() }, theme.skin("panel")))
                .with_children(|p| {
                    p.spawn((theme.bold(sizes::TITLE), Text::new("Map"), TextColor(colors::HEADING)));
                    p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: Val::Px(18.0), ..default() }).with_children(|row| {
                        row.spawn((Node { width: Val::Vh(SHOWN), height: Val::Vh(SHOWN), ..default() }, BackgroundColor(Color::srgb(0.06, 0.07, 0.1)), MapPicture)).with_children(|m| {
                            m.spawn((Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, MapMarks));
                        });
                        row.spawn((Node { flex_direction: FlexDirection::Column, width: Val::Px(300.0), row_gap: Val::Px(4.0), ..default() }, GoalsSlot));
                    });
                    p.spawn((text("", sizes::SMALL, colors::MUTED), MapNote));
                    p.spawn(text("Click: add waypoint   Space: mark where you stand   Right click: remove   M or Esc: close", sizes::SMALL, colors::MUTED));
                });
        });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(crate::weather_dial::DIAL_SPACE),
            left: Val::Percent(20.0),
            width: Val::Percent(60.0),
            height: Val::Px(40.0),
            ..default()
        },
        Strip,
    ));
}

fn open_close(
    keys: Res<ButtonInput<KeyCode>>,
    mut view: ResMut<MapView>,
    mut menus: ResMut<Menus>,
    mut ui: Single<&mut Visibility, With<MapUi>>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut forced: Local<bool>,
) {
    let auto = !*forced && std::env::var_os("BLOCKGAME_MAP").is_some();
    *forced = true;
    let toggle = auto || keys.just_pressed(KeyCode::KeyM);
    if view.open && (toggle || keys.just_pressed(KeyCode::Escape)) {
        view.open = false;
        menus.open = false;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    } else if !view.open && toggle && !menus.open {
        view.open = true;
        menus.open = true;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
    let want = if view.open {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if **ui != want {
        **ui = want;
    }
}

fn paint_map(
    mut commands: Commands,
    mut view: ResMut<MapView>,
    generator: Option<Res<Generator>>,
    player: Res<Player>,
    mut images: ResMut<Assets<Image>>,
    picture: Single<Entity, With<MapPicture>>,
) {
    if let Some(task) = view.task.as_mut() {
        if let Some((center, data)) = check_ready(task) {
            view.task = None;
            view.center = Some(center);
            let image = Image::new(
                Extent3d {
                    width: PIXELS,
                    height: PIXELS,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                data,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            let handle = images.add(image);
            if let Some(old) = view.image.replace(handle.clone()) {
                images.remove(&old);
            }
            commands.entity(*picture).insert(ImageNode::new(handle));
        }
        return;
    }
    let Some(generator) = generator else { return };
    let here = Vec2::new(player.pos.x, player.pos.z);
    let stale = view.center.is_none_or(|c| c.distance(here) > SPAN / 4.0);
    if view.open && stale {
        let g: Arc<WorldGen> = generator.0.clone();
        let center = (here / STEP as f32).round() * STEP as f32;
        view.task = Some(
            AsyncComputeTaskPool::get()
                .spawn(async move { (center, paint(&g, center.x as i32, center.y as i32)) }),
        );
    }
}

/// A mark on the map, at `uv` (0..1).
fn mark(uv: Vec2, color: Color, size: f32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(uv.x * 100.0),
            top: Val::Percent(uv.y * 100.0),
            margin: UiRect {
                left: Val::Px(-size / 2.0),
                top: Val::Px(-size / 2.0),
                ..default()
            },
            width: Val::Px(size),
            height: Val::Px(size),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(color),
        BorderColor::all(Color::BLACK),
    )
}

#[allow(clippy::too_many_arguments)]
fn place_marks(
    mut commands: Commands,
    view: Res<MapView>,
    waypoints: Res<Waypoints>,
    player: Res<Player>,
    marks: Single<Entity, With<MapMarks>>,
    mut note: Single<&mut Text, With<MapNote>>,
    mut last: Local<Option<(Vec2, Vec2, usize)>>,
) {
    if !view.open {
        return;
    }
    let Some(center) = view.center else {
        note.0 = "Drawing the map...".into();
        return;
    };
    let here = Vec2::new(player.pos.x, player.pos.z);
    // Redraw when you move a pixel, the map changes or the waypoints do.
    let key = ((here / STEP as f32).floor(), center, waypoints.list.len());
    if *last == Some(key) && !waypoints.is_changed() {
        return;
    }
    *last = Some(key);
    note.0 = format!(
        "You are at {:.0}, {:.0}, {:.0}",
        player.pos.x, player.pos.y, player.pos.z
    );
    commands.entity(*marks).despawn_related::<Children>();
    commands.entity(*marks).with_children(|m| {
        for w in &waypoints.list {
            let uv = on_map(center, Vec2::new(w.pos[0], w.pos[2]));
            if uv.cmplt(Vec2::ZERO).any() || uv.cmpgt(Vec2::ONE).any() {
                continue;
            }
            m.spawn(mark(uv, Color::srgb(1.0, 0.82, 0.25), 12.0));
            m.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(uv.x * 100.0),
                    top: Val::Percent(uv.y * 100.0),
                    margin: UiRect {
                        left: Val::Px(9.0),
                        top: Val::Px(-9.0),
                        ..default()
                    },
                    ..default()
                },
                text(w.name.clone(), sizes::SMALL, colors::TEXT),
            ));
        }
        m.spawn(mark(
            on_map(center, here).clamp(Vec2::ZERO, Vec2::ONE),
            Color::srgb(0.95, 0.2, 0.15),
            14.0,
        ));
    });
}

#[allow(clippy::too_many_arguments)]
fn click_map(
    view: Res<MapView>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    picture: Single<(&ComputedNode, &UiGlobalTransform), With<MapPicture>>,
    player: Res<Player>,
    generator: Option<Res<Generator>>,
    mut waypoints: ResMut<Waypoints>,
) {
    if !view.open {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        waypoints.add(player.pos);
    }
    let Some(center) = view.center else { return };
    let (node, tf) = *picture;
    let k = node.inverse_scale_factor();
    let rect = Rect::from_center_size(tf.translation * k, node.size() * k);
    let Some(cursor) = window.cursor_position().filter(|c| rect.contains(*c)) else {
        return;
    };
    let at = from_map(center, (cursor - rect.min) / rect.size());
    if mouse.just_pressed(MouseButton::Left) {
        let y = generator.map_or(player.pos.y, |g| {
            g.0.column(at.x as i32, at.y as i32).height as f32 + 1.0
        });
        waypoints.add(Vec3::new(at.x, y, at.y));
    } else if mouse.just_pressed(MouseButton::Right)
        && let Some(i) = waypoints.nearest(at, SPAN / PIXELS as f32 * 8.0)
    {
        waypoints.list.remove(i);
    }
}

/// Waypoints in front of you, along the top of the screen.
#[allow(clippy::too_many_arguments)]
fn strip(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut wait: Local<f32>,
    waypoints: Res<Waypoints>,
    view: Res<MapView>,
    menus: Res<Menus>,
    player: Res<Player>,
    cam: Single<&FlyCamera>,
    root: Single<(Entity, &mut Visibility), With<Strip>>,
) {
    let (root, mut vis) = root.into_inner();
    let want =
        if menus.open || view.open || (waypoints.list.is_empty() && waypoints.guide.is_empty()) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    if *vis != want {
        *vis = want;
    }
    *wait -= time.delta_secs();
    if want == Visibility::Hidden || *wait > 0.0 {
        return;
    }
    // Ten times a second is smooth enough and keeps text layout cheap.
    *wait = 0.1;
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|s| {
        let gold = Color::srgb(1.0, 0.82, 0.25);
        let guide = Color::srgb(0.45, 0.9, 1.0);
        for (w, color) in waypoints
            .list
            .iter()
            .map(|w| (w, gold))
            .chain(waypoints.guide.iter().map(|w| (w, guide)))
        {
            let to = Vec3::from_array(w.pos);
            let a = bearing(player.pos, cam.yaw, to);
            if a.abs() > STRIP_ANGLE {
                continue;
            }
            let x = 50.0 + a / STRIP_ANGLE * 50.0;
            let d = Vec2::new(to.x - player.pos.x, to.z - player.pos.z).length();
            s.spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(x),
                width: Val::Px(200.0),
                margin: UiRect::left(Val::Px(-100.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|m| {
                m.spawn((
                    Node {
                        width: Val::Px(8.0),
                        height: Val::Px(8.0),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(color),
                    BorderColor::all(Color::BLACK),
                ));
                m.spawn(text(
                    format!("{} {:.0}m", w.name, d),
                    sizes::SMALL,
                    colors::TEXT,
                ));
            });
        }
    });
}

fn load(dir: Res<SaveDir>, mut waypoints: ResMut<Waypoints>) {
    if screenshot_run() {
        return;
    }
    if let Some(w) = read_ron(&dir, FILE) {
        *waypoints = w;
    }
}

fn save_now_and_then(
    time: Res<Time<Real>>,
    mut since: Local<f32>,
    dir: Res<SaveDir>,
    waypoints: Res<Waypoints>,
) {
    *since += time.delta_secs();
    if *since < 30.0 || screenshot_run() {
        return;
    }
    *since = 0.0;
    write_ron(&dir, FILE, &*waypoints);
}

fn save_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, waypoints: Res<Waypoints>) {
    if exit.read().next().is_some() && !screenshot_run() {
        write_ron(&dir, FILE, &*waypoints);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_points_go_there_and_back() {
        let c = Vec2::new(100.0, -40.0);
        assert_eq!(on_map(c, c), Vec2::splat(0.5));
        let p = Vec2::new(-300.0, 500.0);
        assert!(from_map(c, on_map(c, p)).distance(p) < 0.01);
        // North (-z) is up.
        assert!(on_map(c, c + Vec2::new(0.0, -100.0)).y < 0.5);
    }

    #[test]
    fn bearings_are_left_and_right_of_where_you_look() {
        // Yaw 0 looks along -z.
        assert!(bearing(Vec3::ZERO, 0.0, Vec3::new(0.0, 0.0, -10.0)).abs() < 0.01);
        assert!(bearing(Vec3::ZERO, 0.0, Vec3::new(10.0, 0.0, -10.0)) > 0.0);
        assert!(bearing(Vec3::ZERO, 0.0, Vec3::new(-10.0, 0.0, -10.0)) < 0.0);
    }

    #[test]
    fn waypoints_are_numbered_and_found() {
        let mut w = Waypoints::default();
        w.add(Vec3::new(10.0, 0.0, 10.0));
        w.add(Vec3::new(500.0, 0.0, 0.0));
        assert_eq!(w.list[1].name, "Waypoint 2");
        assert_eq!(w.nearest(Vec2::new(12.0, 9.0), 16.0), Some(0));
        assert_eq!(w.nearest(Vec2::new(200.0, 0.0), 16.0), None);
        let text = ron::to_string(&w).unwrap();
        assert_eq!(ron::from_str::<Waypoints>(&text).unwrap(), w);
    }
}
