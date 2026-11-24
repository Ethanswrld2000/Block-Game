//! The airship, the first machine from the machination table.
//!
//! Hold the `airship` item and right click open ground: it is set down there,
//! bow pointing the way you look. Right click it (or press E looking at it) to
//! board: you stand on its deck and can walk about (WASD, kept inside the
//! rails, carried with the ship). Right click near the helm to take it: the
//! ship answers W / S (ahead, astern), A / D (turn), Space (rise) and Ctrl / C
//! (sink); the mouse still looks around; E lets go of the helm. On deck, E
//! steps off over the nearest rail. A ship nobody flies hovers where it was
//! left. Crouch and right click a ship with an empty hand (not aboard) to pick
//! it back up as an `airship` item. Rules in `flight` (pure); ships, who is at
//! the helm and who is on deck save to `airships.ron`.

pub mod flight;

use bevy::input::keyboard::KeyCode;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use serde::{Deserialize, Serialize};
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::{FlyCamera, Menus};
use crate::interact::{ray_box, Target, Wielded, REACH};
use crate::inventory::Inventory;
use crate::items::{ItemKind, Items};
use crate::player::{Body, MoveMode, Player};
use crate::save::SaveDir;
use crate::survival::Toast;
use crate::world::World;
use flight::{Controls, Machines, Ship, HALF, HEIGHT};
pub use flight::HELM;

pub struct AirshipPlugin;

impl Plugin for AirshipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Airships>()
            .init_resource::<ShipAim>()
            .add_systems(crate::menu::GameStartup, load_ships)
            .add_systems(
                crate::menu::GameUpdate,
                (steer, walk_deck).chain().after(crate::camera::look).before(crate::player::PlayerSet),
            )
            .add_systems(
                crate::menu::GameUpdate,
                (aim, board, set_down, pick_up, save_ships)
                    .chain()
                    .after(crate::player::PlayerSet)
                    .before(crate::interact::InteractSet),
            )
            .add_systems(crate::menu::GameUpdate, draw_ships.after(steer))
            .add_systems(Last, save_ships_on_exit.run_if(crate::menu::in_game));
    }
}

/// The machines' numbers, from `assets/data/machines.ron`.
#[derive(Resource)]
pub struct MachineBook(pub Machines);

/// Every airship in the world, and which one you're flying.
#[derive(Resource, Debug, Default)]
pub struct Airships {
    pub ships: Vec<Ship>,
    /// The ship whose helm you hold.
    pub aboard: Option<usize>,
    /// The ship whose deck you walk, and where on it (in its frame).
    pub deck: Option<(usize, Vec3)>,
}

/// The airship in front of you within reach, and how far its box is.
#[derive(Resource, Debug, Default)]
pub struct ShipAim(pub Option<(usize, f32)>);

/// Walking is off while you're aboard (`player::walk`): at the helm or on deck.
pub fn not_aboard(ships: Option<Res<Airships>>) -> bool {
    ships.is_none_or(|s| s.aboard.is_none() && s.deck.is_none())
}

/// Where you can stand on deck (ship frame, x and z), inside the rails.
pub const DECK_MIN: Vec2 = Vec2::new(-1.0, -3.2);
pub const DECK_MAX: Vec2 = Vec2::new(1.0, 2.9);
/// Walking speed on deck, blocks per second.
const DECK_SPEED: f32 = 3.6;
/// How near the helm (blocks) a right click takes it.
const HELM_REACH: f32 = 1.4;

/// A spot on deck, kept inside the rails, at deck height.
pub fn on_deck(local: Vec3) -> Vec3 {
    Vec3::new(local.x.clamp(DECK_MIN.x, DECK_MAX.x), HELM.y, local.z.clamp(DECK_MIN.y, DECK_MAX.y))
}

/// Just over the rail nearest a spot on deck (ship frame), to step off to.
pub fn over_rail(local: Vec3) -> Vec3 {
    let p = on_deck(local);
    // Sideways rails are much nearer than bow and stern for most of the deck.
    let side = if p.x >= 0.0 { 1.0 } else { -1.0 };
    let to_side = DECK_MAX.x - p.x.abs();
    let (to_bow, to_stern) = (p.z - DECK_MIN.y, DECK_MAX.y - p.z);
    if to_side <= to_bow.min(to_stern) {
        Vec3::new(side * (HALF.x + 0.6), HELM.y, p.z)
    } else if to_bow < to_stern {
        Vec3::new(p.x, HELM.y, -HALF.z - 0.6)
    } else {
        Vec3::new(p.x, HELM.y, HALF.z + 0.6)
    }
}

/// What `airships.ron` holds.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    ships: Vec<SavedShip>,
    #[serde(default)]
    aboard: Option<usize>,
    /// The ship and spot (its frame) you stand on, if on deck.
    #[serde(default)]
    deck: Option<(usize, (f32, f32, f32))>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedShip {
    pos: (f32, f32, f32),
    yaw: f32,
}

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() || std::env::var_os("BLOCKGAME_BENCH").is_some()
}

fn load_ships(mut commands: Commands, dir: Res<SaveDir>) {
    let book = match Machines::from_ron(include_str!("../../../../assets/data/machines.ron")) {
        Ok(m) => m,
        Err(e) => panic!("{e}"),
    };
    commands.insert_resource(MachineBook(book));
    let mut ships = Airships::default();
    if !screenshot_run()
        && let Ok(text) = std::fs::read_to_string(dir.file("airships.ron"))
    {
        match ron::from_str::<Saved>(&text) {
            Ok(s) => {
                ships.ships = s.ships.iter().map(|p| Ship::new(Vec3::new(p.pos.0, p.pos.1, p.pos.2), p.yaw)).collect();
                ships.aboard = s.aboard.filter(|i| *i < ships.ships.len());
                ships.deck = s.deck.filter(|(i, _)| *i < ships.ships.len() && ships.aboard.is_none()).map(|(i, p)| (i, Vec3::new(p.0, p.1, p.2)));
            }
            Err(e) => warn!("airships.ron: {e}"),
        }
    }
    commands.insert_resource(ships);
}

fn store(dir: &SaveDir, ships: &Airships) {
    let saved = Saved {
        ships: ships.ships.iter().map(|s| SavedShip { pos: (s.pos.x, s.pos.y, s.pos.z), yaw: s.yaw }).collect(),
        aboard: ships.aboard,
        deck: ships.deck.map(|(i, p)| (i, (p.x, p.y, p.z))),
    };
    match ron::ser::to_string_pretty(&saved, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file("airships.ron"), text) {
                warn!("saving airships.ron: {e}");
            }
        }
        Err(e) => warn!("airships.ron: {e}"),
    }
}

/// Saved when a ship is set down or boarded, and every few seconds while one flies.
#[allow(clippy::type_complexity)]
fn save_ships(time: Res<Time>, dir: Res<SaveDir>, ships: Res<Airships>, mut wait: Local<f32>, mut last: Local<(usize, Option<usize>, Option<usize>)>) {
    if screenshot_run() || ships.is_added() {
        return;
    }
    *wait -= time.delta_secs();
    let key = (ships.ships.len(), ships.aboard, ships.deck.map(|d| d.0));
    if key != *last || (ships.aboard.is_some() && *wait <= 0.0) {
        *last = key;
        *wait = 5.0;
        store(&dir, &ships);
    }
}

fn save_ships_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, ships: Res<Airships>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &ships);
    }
}

/// Is this block something a ship bumps into? Unloaded chunks don't stop it.
fn solid_in(world: &World, blocks: &Blocks) -> impl Fn(IVec3) -> bool {
    move |p| world.block(BlockPos::new(p.x, p.y, p.z)).is_some_and(|b| blocks.registry.get(b).solid)
}

/// Fly the ship you're aboard and keep you at its helm.
#[allow(clippy::too_many_arguments)]
fn steer(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    (menus, body, mode): (Res<Menus>, Res<Body>, Res<MoveMode>),
    book: Option<Res<MachineBook>>,
    (world, blocks): (Res<World>, Res<Blocks>),
    mut ships: ResMut<Airships>,
    mut player: ResMut<Player>,
    mut cam: Single<&mut FlyCamera>,
) {
    // Free flight leaves the ship behind.
    if *mode == MoveMode::Fly {
        ships.deck = None;
    }
    let (Some(book), Some(i)) = (book, ships.aboard) else { return };
    if *mode == MoveMode::Fly {
        ships.aboard = None;
        flight::moor(&mut ships.ships[i]);
        return;
    }
    let mut c = Controls::default();
    if !menus.open && !menus.still && !body.frozen {
        let axis = |plus: bool, minus: bool| (plus as i32 - minus as i32) as f32;
        c.ahead = axis(keys.pressed(KeyCode::KeyW), keys.pressed(KeyCode::KeyS));
        c.turn = axis(keys.pressed(KeyCode::KeyA), keys.pressed(KeyCode::KeyD));
        c.lift = axis(keys.pressed(KeyCode::Space), keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::KeyC));
    }
    let dt = time.delta_secs().min(0.1);
    let ship = &mut ships.ships[i];
    let before = ship.yaw;
    flight::step(ship, c, &book.0.airship, dt, &solid_in(&world, &blocks));
    // You turn with the ship.
    cam.yaw += ship.yaw - before;
    let p = &mut *player;
    p.pos = ship.to_world(HELM);
    p.vel = Vec3::ZERO;
    p.on_ground = true;
    p.in_water = false;
    p.head_in_water = false;
    p.crouching = false;
    p.sprinting = false;
    p.bob = 0.0;
    p.ready = true;
}

/// Walk about the deck of the ship you stand on: WASD by where you look, kept
/// inside the rails; you go where the ship goes.
#[allow(clippy::too_many_arguments)]
fn walk_deck(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    (menus, body): (Res<Menus>, Res<Body>),
    mut ships: ResMut<Airships>,
    mut player: ResMut<Player>,
    cam: Single<&FlyCamera>,
) {
    let Some((i, mut local)) = ships.deck else { return };
    let Some(ship) = ships.ships.get(i).copied() else {
        ships.deck = None;
        return;
    };
    if !menus.open && !menus.still && !body.frozen {
        let axis = |plus: bool, minus: bool| (plus as i32 - minus as i32) as f32;
        let fwd = axis(keys.pressed(KeyCode::KeyW), keys.pressed(KeyCode::KeyS));
        let side = axis(keys.pressed(KeyCode::KeyD), keys.pressed(KeyCode::KeyA));
        // Where you look, in the ship's frame.
        let yaw = cam.yaw - ship.yaw;
        let (f, r) = (Vec3::new(-yaw.sin(), 0.0, -yaw.cos()), Vec3::new(yaw.cos(), 0.0, -yaw.sin()));
        let step = (f * fwd + r * side).normalize_or_zero() * DECK_SPEED * time.delta_secs().min(0.1);
        local = on_deck(local + step);
    }
    ships.deck = Some((i, local));
    let p = &mut *player;
    p.pos = ship.to_world(local);
    p.vel = Vec3::ZERO;
    p.on_ground = true;
    p.in_water = false;
    p.head_in_water = false;
    p.ready = true;
}

/// Which ship the view points at, within reach.
fn aim(cam: Single<&Transform, With<FlyCamera>>, ships: Res<Airships>, mut aimed: ResMut<ShipAim>) {
    aimed.0 = None;
    if ships.aboard.is_some() {
        return;
    }
    if ships.deck.is_some() {
        return;
    }
    let (o, d) = (cam.translation, cam.forward().as_vec3());
    aimed.0 = ships
        .ships
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let inv = s.rotation().inverse();
            let (lo, ld) = (inv * (o - s.pos), inv * d);
            ray_box(lo, ld, Vec3::new(-HALF.x, 0.0, -HALF.z), Vec3::new(HALF.x, HEIGHT, HALF.z)).filter(|t| *t <= REACH).map(|t| (i, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
}

/// Right click a ship (or E looking at it) to board; E steps off.
#[allow(clippy::too_many_arguments)]
fn board(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    (menus, body): (Res<Menus>, Res<Body>),
    (aimed, target, cam): (Res<ShipAim>, Res<Target>, Single<&Transform, With<FlyCamera>>),
    wielded: Res<Wielded>,
    mut mode: ResMut<MoveMode>,
    mut ships: ResMut<Airships>,
    mut player: ResMut<Player>,
    mut toast: MessageWriter<Toast>,
) {
    if cursor.grab_mode == CursorGrabMode::None || menus.open || body.frozen {
        return;
    }
    // At the helm: E lets go and you stand on deck behind it.
    if let Some(i) = ships.aboard {
        if keys.just_pressed(KeyCode::KeyE) {
            ships.aboard = None;
            flight::moor(&mut ships.ships[i]);
            ships.deck = Some((i, on_deck(HELM)));
            toast.write(Toast("You let go of the helm. E steps off over the rail".into()));
        }
        return;
    }
    // On deck: right click by the helm takes it; E steps off over the nearest rail.
    if let Some((i, local)) = ships.deck {
        if mouse.just_pressed(MouseButton::Right) && local.distance(HELM) <= HELM_REACH {
            ships.deck = None;
            ships.aboard = Some(i);
            toast.write(Toast("At the helm: W / S ahead and astern, A / D turn, Space rise, Ctrl sink. E lets go".into()));
        } else if keys.just_pressed(KeyCode::KeyE) {
            ships.deck = None;
            player.pos = ships.ships[i].to_world(over_rail(local));
            player.vel = Vec3::ZERO;
            player.on_ground = false;
            toast.write(Toast("You step off the airship".into()));
        }
        return;
    }
    let Some((i, dist)) = aimed.0 else { return };
    // A block nearer than the ship takes the click.
    let block_dist = target.hit.and_then(|h| {
        let min = Vec3::new(h.block.x as f32, h.block.y as f32, h.block.z as f32);
        ray_box(cam.translation, cam.forward().as_vec3(), min, min + Vec3::ONE)
    });
    if block_dist.is_some_and(|b| b < dist) {
        return;
    }
    // Crouching with an empty hand picks the ship up (`pick_up`).
    if player.crouching && wielded.0.is_none() {
        return;
    }
    if mouse.just_pressed(MouseButton::Right) || keys.just_pressed(KeyCode::KeyE) {
        *mode = MoveMode::Walk;
        // Onto the deck where you climb aboard.
        let ship = ships.ships[i];
        let local = on_deck(ship.rotation().inverse() * (player.pos - ship.pos));
        ships.deck = Some((i, local));
        toast.write(Toast("Aboard. Walk to the helm and right click to fly. E steps off".into()));
    }
}

/// Right click open ground with a machine in hand: set it down there.
#[allow(clippy::too_many_arguments)]
fn set_down(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    menus: Res<Menus>,
    (target, wielded, items, aimed): (Res<Target>, Res<Wielded>, Res<Items>, Res<ShipAim>),
    cam: Single<&FlyCamera>,
    (world, blocks): (Res<World>, Res<Blocks>),
    mut inv: ResMut<Inventory>,
    mut ships: ResMut<Airships>,
    mut toast: MessageWriter<Toast>,
) {
    if !mouse.just_pressed(MouseButton::Right) || cursor.grab_mode == CursorGrabMode::None || menus.open {
        return;
    }
    let (Some(item), Some(hit)) = (wielded.0, target.hit) else { return };
    let ItemKind::Machine(machine) = &items.get(item).kind else { return };
    if machine != "airship" || ships.aboard.is_some() || aimed.0.is_some() {
        return;
    }
    if hit.before.y != hit.block.y + 1 {
        toast.write(Toast("Set the airship down on top of the ground".into()));
        return;
    }
    let ship = Ship::new(Vec3::new(hit.before.x as f32 + 0.5, hit.before.y as f32, hit.before.z as f32 + 0.5), cam.yaw);
    if flight::hits(ship.pos, ship.yaw, &solid_in(&world, &blocks)) {
        toast.write(Toast(format!("Not enough room: the airship needs about {} x {} blocks of open ground, {} high", 4, 9, HEIGHT.ceil())));
        return;
    }
    if !inv.take(item, 1) {
        return;
    }
    ships.ships.push(ship);
    toast.write(Toast("Airship set down. Right click it to board".into()));
}

/// Crouch and right click a ship with an empty hand: it goes back into your
/// pockets as the `airship` item (if there's room).
#[allow(clippy::too_many_arguments)]
fn pick_up(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    menus: Res<Menus>,
    (aimed, wielded, items): (Res<ShipAim>, Res<Wielded>, Res<Items>),
    player: Res<Player>,
    mut inv: ResMut<Inventory>,
    mut ships: ResMut<Airships>,
    mut toast: MessageWriter<Toast>,
) {
    if !mouse.just_pressed(MouseButton::Right) || cursor.grab_mode == CursorGrabMode::None || menus.open || !player.crouching || wielded.0.is_some() {
        return;
    }
    let Some((i, _)) = aimed.0 else { return };
    if ships.aboard.is_some() || ships.deck.is_some() {
        return;
    }
    let Some(item) = items.id("airship") else { return };
    if inv.add(&items, crate::inventory::grid::Stack::new(item, 1)).is_some() {
        toast.write(Toast("No room in your pockets for the airship".into()));
        return;
    }
    ships.ships.remove(i);
    toast.write(Toast("You fold the airship up and take it with you".into()));
}

/// A ship's model; its index in `Airships.ships`.
#[derive(Component)]
struct ShipModel(usize);

/// The propeller at the stern, spun by the engine.
#[derive(Component)]
pub struct Propeller;

/// Keep a model on every ship, where the ship is, its propeller turning.
#[allow(clippy::too_many_arguments)]
fn draw_ships(
    mut commands: Commands,
    time: Res<Time>,
    ships: Res<Airships>,
    mut models: Query<(Entity, &ShipModel, &mut Transform), Without<Propeller>>,
    mut props: Query<(&ChildOf, &mut Transform), With<Propeller>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut spin: Local<Vec<f32>>,
) {
    if models.iter().count() != ships.ships.len() {
        for (e, ..) in &models {
            commands.entity(e).despawn();
        }
        for (i, s) in ships.ships.iter().enumerate() {
            let tf = Transform::from_translation(s.pos).with_rotation(s.rotation());
            let e = build_model(&mut commands, &mut meshes, &mut materials, tf);
            commands.entity(e).insert(ShipModel(i));
        }
        return;
    }
    spin.resize(ships.ships.len(), 0.0);
    let dt = time.delta_secs();
    for (e, m, mut tf) in &mut models {
        let Some(s) = ships.ships.get(m.0) else { continue };
        *tf = Transform::from_translation(s.pos).with_rotation(s.rotation());
        // Idles while someone is at the helm, spins up with speed.
        let flying = ships.aboard == Some(m.0);
        let rate = if flying { 4.0 + s.vel.length() * 1.6 } else { 0.0 };
        spin[m.0] = (spin[m.0] + rate * dt) % std::f32::consts::TAU;
        for (parent, mut ptf) in &mut props {
            if parent.parent() == e {
                ptf.rotation = Quat::from_rotation_z(spin[m.0]);
            }
        }
    }
}

/// The ship's boxes, in its own frame (bow toward -z, keel bottom at y 0). Also used
/// for the cutscene airship (`cutscene::props`).
pub fn build_model(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, tf: Transform) -> Entity {
    let mut mat = |r: f32, g: f32, b: f32, metal: bool| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            perceptual_roughness: if metal { 0.45 } else { 0.92 },
            metallic: if metal { 0.6 } else { 0.0 },
            ..default()
        })
    };
    let wood = mat(0.5, 0.36, 0.22, false);
    let dark = mat(0.34, 0.24, 0.15, false);
    let deck = mat(0.62, 0.47, 0.3, false);
    let hide = mat(0.7, 0.55, 0.38, false);
    let stripe = mat(0.55, 0.22, 0.16, false);
    let cord = mat(0.72, 0.63, 0.45, false);
    let iron = mat(0.3, 0.3, 0.32, true);
    let brass = mat(0.72, 0.56, 0.26, true);
    #[rustfmt::skip]
    let parts: Vec<(Vec3, Vec3, Handle<StandardMaterial>)> = vec![
        // Hull: keel, sides, bow and stern.
        (Vec3::new(1.4, 0.3, 5.6), Vec3::new(0.0, 0.15, 0.0), dark.clone()),
        (Vec3::new(2.6, 0.7, 6.4), Vec3::new(0.0, 0.65, 0.0), wood.clone()),
        (Vec3::new(1.8, 0.6, 0.8), Vec3::new(0.0, 0.75, -3.6), wood.clone()),
        (Vec3::new(0.9, 0.5, 0.5), Vec3::new(0.0, 0.85, -4.05), dark.clone()),
        // Deck and rails.
        (Vec3::new(2.4, 0.06, 6.2), Vec3::new(0.0, 1.0, 0.0), deck.clone()),
        (Vec3::new(0.1, 0.4, 6.4), Vec3::new(1.25, 1.2, 0.0), dark.clone()),
        (Vec3::new(0.1, 0.4, 6.4), Vec3::new(-1.25, 1.2, 0.0), dark.clone()),
        (Vec3::new(2.6, 0.4, 0.1), Vec3::new(0.0, 1.2, 3.15), dark.clone()),
        // The helm: post and wheel, in front of where you stand.
        (Vec3::new(0.12, 0.9, 0.12), Vec3::new(0.0, 1.45, 1.0), dark.clone()),
        (Vec3::new(0.7, 0.7, 0.06), Vec3::new(0.0, 1.95, 0.95), brass.clone()),
        (Vec3::new(0.5, 0.5, 0.07), Vec3::new(0.0, 1.95, 0.95), dark.clone()),
        // The engine at the stern, under the propeller.
        (Vec3::new(0.9, 0.6, 0.7), Vec3::new(0.0, 0.7, 3.45), iron.clone()),
        (Vec3::new(0.16, 0.16, 0.4), Vec3::new(0.0, 0.95, 3.85), iron.clone()),
        // Ropes up to the envelope.
        (Vec3::new(0.05, 3.3, 0.05), Vec3::new(1.15, 2.85, 2.6), cord.clone()),
        (Vec3::new(0.05, 3.3, 0.05), Vec3::new(-1.15, 2.85, 2.6), cord.clone()),
        (Vec3::new(0.05, 3.3, 0.05), Vec3::new(1.15, 2.85, -2.6), cord.clone()),
        (Vec3::new(0.05, 3.3, 0.05), Vec3::new(-1.15, 2.85, -2.6), cord.clone()),
        // The envelope: a long hide bag with rounded-off ends, a stripe and tail fins.
        (Vec3::new(3.2, 2.2, 7.2), Vec3::new(0.0, 5.5, 0.0), hide.clone()),
        (Vec3::new(2.6, 1.7, 0.4), Vec3::new(0.0, 5.5, -3.8), hide.clone()),
        (Vec3::new(2.6, 1.7, 0.4), Vec3::new(0.0, 5.5, 3.8), hide.clone()),
        (Vec3::new(3.26, 0.2, 7.26), Vec3::new(0.0, 5.5, 0.0), stripe.clone()),
        (Vec3::new(0.06, 0.9, 0.9), Vec3::new(0.0, 6.3, 3.6), stripe.clone()),
        (Vec3::new(1.6, 0.06, 0.8), Vec3::new(0.0, 5.5, 4.0), stripe.clone()),
    ];
    let blade = meshes.add(Cuboid::new(1.5, 0.16, 0.05));
    let cross = meshes.add(Cuboid::new(0.16, 1.5, 0.05));
    commands
        .spawn((tf, Visibility::default()))
        .with_children(|c| {
        for (size, at, m) in parts {
            c.spawn((Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))), MeshMaterial3d(m), Transform::from_translation(at)));
        }
        c.spawn((Propeller, Transform::from_xyz(0.0, 0.95, 4.08), Visibility::default())).with_children(|p| {
            p.spawn((Mesh3d(blade), MeshMaterial3d(wood.clone()), Transform::default(), NotShadowCaster));
            p.spawn((Mesh3d(cross), MeshMaterial3d(wood.clone()), Transform::default(), NotShadowCaster));
        });
        })
        .id()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_deck_keeps_you_inside_the_rails_and_steps_off_over_the_nearest() {
        let p = on_deck(Vec3::new(5.0, 9.0, -9.0));
        assert_eq!(p, Vec3::new(DECK_MAX.x, HELM.y, DECK_MIN.y));
        // Near the starboard rail mid-ship: off to that side, outside the hull.
        let off = over_rail(Vec3::new(0.9, 0.0, 0.0));
        assert!(off.x > HALF.x);
        // Near the bow: off over the bow.
        let off = over_rail(Vec3::new(0.0, 0.0, -3.1));
        assert!(off.z < -HALF.z);
        assert!(HELM.distance(on_deck(HELM)) < 1e-4);
    }
}
