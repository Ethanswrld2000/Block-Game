//! Stations grow around urns. The ancestral urn stands by your bedroll and
//! moves with it; the others (tool, hearth, star, golem) are carried as items
//! and set down wherever you like (right click the ground), and picked back
//! up by crouching and right clicking them with an empty hand.
//!
//! Each urn has a tree of steps (`assets/data/urns.ron`, rules in `urns`).
//! Hold an item and right click the urn to put it in (Shift: all you carry
//! that it wants); right click with an empty hand to see its tree, where you
//! can also put in what you carry step by step. A finished step places a
//! station by the urn, upgrades one, teaches a bench recipes, or gives you
//! another urn. Marks around each urn's belly light up as its next step fills.
//!
//! The stations are models, not blocks. Benches with a board (camp mat, tool
//! bench, carpentry bench) are worked by hand (`board`); personal storage
//! opens like a chest; the rest show what they will do.
//! F9 opens the dev menu (`dev`) to hand out urns and skip steps.

pub mod bench;
pub mod board;
mod dev;
pub mod models;
mod tree;
pub mod urns;

use std::collections::BTreeMap;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use serde::{Deserialize, Serialize};
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::{FlyCamera, Menus};
use crate::interact::{ray_box, Wielded, REACH};
use crate::inventory::grid::{Grid, Stack};
use crate::inventory::{Chests, Inventory, Spill};
use crate::items::Items;
use crate::save::SaveDir;
use crate::survival::camp::Camp;
use crate::survival::Toast;
use crate::theme::{colors, sizes, Theme};
use crate::world::World;
use models::{Mat, MARKS};
use urns::{Gives, Progress, UrnData};

pub struct StationPlugin;

impl Plugin for StationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Urns>()
            .init_resource::<StationAim>()
            .init_resource::<board::AtBench>()
            .init_resource::<tree::TreeUi>()
            .init_resource::<dev::DevMenu>()
            .add_message::<StepDone>()
            .add_systems(crate::menu::GameStartup, (load_urns, spawn_label, board::spawn_board_ui, board::load_benches, tree::spawn_tree_ui, dev::spawn_dev_ui).chain())
            .add_systems(
                crate::menu::GameUpdate,
                (
                    aim,
                    place_urn,
                    use_station,
                    steps_done,
                    sync_learned,
                    place_stations,
                    show_label,
                    tree::tree_keys,
                    tree::tree_click,
                    tree::tree_build,
                    dev::dev_keys,
                    dev::dev_click,
                    dev::dev_build,
                    board::board_for_screenshot,
                    board::board_keys,
                    board::board_camera,
                    board::board_hides,
                    board::board_click,
                    board::board_models,
                    board::board_ui,
                )
                    .chain()
                    .before(crate::interact::InteractSet)
                    .after(crate::player::PlayerSet),
            )
            .add_systems(crate::menu::GameUpdate, (save_urns, urns_for_screenshot))
            .add_systems(Last, save_urns_on_exit.run_if(crate::menu::in_game));
    }
}

/// The urns and their trees, from `assets/data/urns.ron`.
#[derive(Resource)]
pub struct UrnBook(pub UrnData);

/// Where a carried urn was set down.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    /// The middle of its foot.
    pub pos: (f32, f32, f32),
    /// Its stations line up along x (else along z).
    pub along_x: bool,
    /// Which way is "behind" it, away from where you stood (+1 or -1 on the other axis).
    pub back: f32,
}

/// Every urn's progress and where the carried ones stand. Saved to `urns.ron`.
#[derive(Resource, Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Urns {
    #[serde(default)]
    pub progress: BTreeMap<String, Progress>,
    #[serde(default)]
    pub placed: BTreeMap<String, Placed>,
}

static NOTHING_YET: Progress = Progress { done: Vec::new(), given: BTreeMap::new() };

impl Urns {
    /// Whether a step is done, in any urn.
    pub fn has(&self, step: &str) -> bool {
        self.progress.values().any(|p| p.is_done(step))
    }

    pub fn of(&self, urn: &str) -> &Progress {
        self.progress.get(urn).unwrap_or(&NOTHING_YET)
    }

    pub fn of_mut(&mut self, urn: &str) -> &mut Progress {
        self.progress.entry(urn.to_string()).or_default()
    }
}

/// A step was finished (by items or the dev menu): hand out what it gives.
#[derive(Message, Debug, Clone)]
pub struct StepDone(pub String);

/// What can stand at a station.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StationKind {
    Urn,
    CampMat,
    ToolBench,
    CarpentryBench,
    PersonalStorage,
    SharpeningStone,
    IronAnvil,
    CookingPot,
    CuttingBoard,
    SmokingRack,
    StoneFridge,
    Telescope,
    MagicalLens,
    Cauldron,
    GolemFrame,
    /// Drawn around the bedroll once the ancestral urn's bedframe is done.
    Bedframe,
}

impl StationKind {
    /// By its name in urns.ron (`gives: Station("...")`).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "camp_mat" => Self::CampMat,
            "tool_bench" => Self::ToolBench,
            "carpentry_bench" => Self::CarpentryBench,
            "personal_storage" => Self::PersonalStorage,
            "sharpening_stone" => Self::SharpeningStone,
            "iron_anvil" => Self::IronAnvil,
            "cooking_pot" => Self::CookingPot,
            "cutting_board" => Self::CuttingBoard,
            "smoking_rack" => Self::SmokingRack,
            "stone_fridge" => Self::StoneFridge,
            "telescope" => Self::Telescope,
            "magical_lens" => Self::MagicalLens,
            "cauldron" => Self::Cauldron,
            "golem_frame" => Self::GolemFrame,
            _ => return None,
        })
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Urn => "Urn",
            Self::CampMat => "Camp Mat",
            Self::ToolBench => "Tool Bench",
            Self::CarpentryBench => "Carpentry Bench",
            Self::PersonalStorage => "Personal Storage",
            Self::SharpeningStone => "Sharpening Stone",
            Self::IronAnvil => "Iron Anvil",
            Self::CookingPot => "Cooking Pot",
            Self::CuttingBoard => "Cutting Board",
            Self::SmokingRack => "Smoking Rack",
            Self::StoneFridge => "Stone Fridge",
            Self::Telescope => "Telescope",
            Self::MagicalLens => "Magical Lens",
            Self::Cauldron => "Cauldron",
            Self::GolemFrame => "Golem Frame",
            Self::Bedframe => "Bedframe",
        }
    }

    /// The urns.ron step that places it.
    fn step(self) -> Option<&'static str> {
        Some(match self {
            Self::CampMat => "camp_mat",
            Self::ToolBench => "tool_bench",
            Self::CarpentryBench => "carpentry_bench",
            Self::PersonalStorage => "personal_storage",
            Self::SharpeningStone => "sharpening_stone",
            Self::IronAnvil => "iron_anvil",
            Self::CookingPot => "cooking_pot",
            Self::CuttingBoard => "cutting_board",
            Self::SmokingRack => "smoking_rack",
            Self::StoneFridge => "stone_fridge",
            Self::Telescope => "telescope",
            Self::MagicalLens => "magical_lens",
            Self::Cauldron => "cauldron",
            Self::GolemFrame => "golem_frame",
            Self::Bedframe => "bedframe",
            Self::Urn => return None,
        })
    }
}

/// A station standing by an urn. `pos` is the middle of its foot.
#[derive(Component, Debug, Clone, Copy)]
pub struct Station {
    pub kind: StationKind,
    pub pos: Vec3,
    /// Turned a quarter (long side along z).
    pub turned: bool,
    /// Which urn it belongs to (index into urns.ron).
    pub urn: usize,
}

impl Station {
    fn bounds(&self) -> (Vec3, Vec3) {
        let mut h = models::half(self.kind);
        if self.turned {
            h = Vec3::new(h.z, h.y, h.x);
        }
        let c = self.pos + Vec3::Y * h.y;
        (c - h, c + h)
    }
}

/// The station under the crosshair and how far away, if it's nearer than
/// anything else you could aim at. `interact` leaves right clicks alone then.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct StationAim(pub Option<(StationKind, f32)>, pub Option<Station>);

/// Everything spawned for the stations, rebuilt when they change.
#[derive(Component)]
struct StationModels;

#[derive(Component)]
struct StationLabel;

fn load_urns(mut commands: Commands, items: Res<Items>, dir: Res<SaveDir>) {
    let book = match UrnData::from_ron(include_str!("../../../../assets/data/urns.ron"), |n| items.id(n).is_some()) {
        Ok(d) => d,
        Err(e) => panic!("{e}"),
    };
    let mut urns = Urns::default();
    if !screenshot_run()
        && let Ok(text) = std::fs::read_to_string(dir.file("urns.ron"))
    {
        match ron::from_str::<Urns>(&text) {
            Ok(u) => urns = u,
            Err(e) => warn!("urns.ron: {e}"),
        }
    }
    // `BLOCKGAME_URN=camp_mat,tool_urn,tool_bench` starts with those steps done (screenshots).
    if let Ok(v) = std::env::var("BLOCKGAME_URN") {
        for step in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match book.step(step) {
                Some((u, _)) => urns.of_mut(&u.name.clone()).finish(step),
                None => warn!("BLOCKGAME_URN: no step '{step}'"),
            }
        }
    }
    commands.insert_resource(urns);
    commands.insert_resource(UrnBook(book));
}

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() || std::env::var_os("BLOCKGAME_BENCH").is_some()
}

fn store(dir: &SaveDir, urns: &Urns) {
    match ron::ser::to_string_pretty(urns, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file("urns.ron"), text) {
                warn!("saving urns.ron: {e}");
            }
        }
        Err(e) => warn!("urns.ron: {e}"),
    }
}

fn save_urns(dir: Res<SaveDir>, urns: Res<Urns>) {
    if urns.is_changed() && !urns.is_added() && !screenshot_run() {
        store(&dir, &urns);
    }
}

fn save_urns_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, urns: Res<Urns>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &urns);
    }
}

/// Screenshots: `BLOCKGAME_CAMP=1` unrolls the bedroll a few blocks in front
/// of you as soon as you stand on the ground; `BLOCKGAME_URNS=tool,hearth`
/// sets those urns down in a row further out (or around `BLOCKGAME_URNS_AT`).
fn urns_for_screenshot(
    player: Res<crate::player::Player>,
    cam: Single<&FlyCamera>,
    mut camp: ResMut<Camp>,
    mut urns: ResMut<Urns>,
    mut done: Local<bool>,
) {
    // `BLOCKGAME_URNS_AT=x,y,z` puts the row there instead (facing -z), for shots taken flying.
    let at: Option<Vec3> = std::env::var("BLOCKGAME_URNS_AT").ok().and_then(|v| {
        let n: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        (n.len() == 3).then(|| Vec3::new(n[0], n[1], n[2]))
    });
    if *done || (!player.on_ground && at.is_none()) {
        return;
    }
    *done = true;
    let (sy, cy) = cam.yaw.sin_cos();
    let forward = if at.is_some() { Vec3::NEG_Z } else { Vec3::new(-sy, 0.0, -cy) };
    if std::env::var_os("BLOCKGAME_CAMP").is_some() {
        let at = player.pos + forward * 3.5;
        camp.bedroll = Some(crate::survival::camp::Bedroll {
            pos: (at.x.round(), player.pos.y.floor(), at.z.round()),
            along_x: forward.x.abs() <= forward.z.abs(),
        });
    }
    if let Ok(v) = std::env::var("BLOCKGAME_URNS") {
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        let names: Vec<&str> = v.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        for (i, name) in names.iter().enumerate() {
            let middle = at.unwrap_or(player.pos + forward * 7.0);
            let at = middle + right * ((i as f32 - (names.len() as f32 - 1.0) / 2.0) * 7.0);
            urns.placed.insert(name.to_string(), placed_facing(at, forward));
        }
    }
}

/// An urn set down at `at`, its stations spread across your view and behind it.
fn placed_facing(at: Vec3, forward: Vec3) -> Placed {
    let along_x = forward.z.abs() >= forward.x.abs();
    let back = if along_x { forward.z.signum() } else { forward.x.signum() };
    Placed { pos: (at.x, at.y, at.z), along_x, back: if back == 0.0 { 1.0 } else { back } }
}

/// Where everything stands: each urn, and the stations its finished steps placed.
fn layout(camp: &Camp, world: &World, blocks: &Blocks, urns: &Urns, book: &UrnData) -> Vec<Station> {
    let mut out = Vec::new();
    for (i, def) in book.urns.iter().enumerate() {
        let progress = urns.of(&def.name);
        let (urn_at, spots, turned): (Vec3, Vec<Vec3>, bool) = if def.item.is_none() {
            // The ancestral urn: on the far side of the bedroll from where you
            // wake, its stations along the bedroll on either side.
            let Some(b) = camp.bedroll else { continue };
            let (along, side) = if b.along_x { (Vec3::X, Vec3::Z) } else { (Vec3::Z, Vec3::X) };
            let c = b.center();
            let urn_at = c - side * 1.6;
            if urns.has("bedframe") {
                out.push(Station { kind: StationKind::Bedframe, pos: c, turned: b.along_x, urn: i });
            }
            let spots = vec![urn_at + along * 1.7, urn_at - along * 2.0, urn_at - side * 1.6, urn_at - side * 1.6 + along * 1.8];
            (urn_at, spots, !b.along_x)
        } else {
            let Some(p) = urns.placed.get(&def.name) else { continue };
            let (along, side) = if p.along_x { (Vec3::X, Vec3::Z * p.back) } else { (Vec3::Z, Vec3::X * p.back) };
            let urn_at = Vec3::new(p.pos.0, p.pos.1, p.pos.2);
            let spots = vec![
                urn_at + along * 1.9,
                urn_at - along * 1.9,
                urn_at + side * 1.9,
                urn_at + side * 1.9 + along * 2.3,
                urn_at + side * 1.9 - along * 2.3,
                urn_at + side * 3.8,
            ];
            (urn_at, spots, !p.along_x)
        };
        out.push(Station { kind: StationKind::Urn, pos: ground(world, blocks, urn_at), turned: false, urn: i });
        let placed = progress.done.iter().filter_map(|d| def.step(d)).filter_map(|s| match &s.gives {
            Gives::Station(name) => StationKind::from_name(name),
            _ => None,
        });
        for (kind, spot) in placed.zip(spots) {
            out.push(Station { kind, pos: ground(world, blocks, spot), turned, urn: i });
        }
    }
    out
}

/// The top of the ground under a spot, near the given height.
fn ground(world: &World, blocks: &Blocks, at: Vec3) -> Vec3 {
    let (x, z) = (at.x.floor() as i32, at.z.floor() as i32);
    let y0 = at.y.floor() as i32;
    for y in (y0 - 3..=y0 + 2).rev() {
        let solid = |y| world.block(BlockPos::new(x, y, z)).is_some_and(|b| blocks.registry.get(b).solid);
        if solid(y) && !solid(y + 1) {
            return Vec3::new(at.x, (y + 1) as f32, at.z);
        }
    }
    at
}

/// Keep the station models in step with the camp and the urns.
#[allow(clippy::too_many_arguments)]
fn place_stations(
    mut commands: Commands,
    camp: Res<Camp>,
    urns: Res<Urns>,
    book: Option<Res<UrnBook>>,
    world: Res<World>,
    blocks: Res<Blocks>,
    existing: Query<Entity, With<StationModels>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut last: Local<String>,
) {
    let Some(book) = book else { return };
    let stations = layout(&camp, &world, &blocks, &urns, &book.0);
    let lit: Vec<u32> = book.0.urns.iter().map(|u| (urns.of(&u.name).next_fill(u) * MARKS as f32).floor() as u32).collect();
    // Rebuild when anything moved (the ground under a spot can still be loading) or a mark lit.
    let key = format!("{:?}{lit:?}{:?}", stations.iter().map(|s| (s.kind, (s.pos * 4.0).round().as_ivec3(), s.turned)).collect::<Vec<_>>(), urns.progress);
    if *last == key {
        return;
    }
    *last = key;
    for e in &existing {
        commands.entity(e).despawn();
    }
    let mut cache: BTreeMap<(usize, u8), Handle<StandardMaterial>> = BTreeMap::new();
    let mut cubes: BTreeMap<[u32; 3], Handle<Mesh>> = BTreeMap::new();
    let has = |s: &str| urns.has(s);
    for st in stations {
        let color = book.0.urns[st.urn].color;
        let rot = if st.turned { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
        let parts = models::parts(st.kind, &has, lit[st.urn]);
        commands.spawn((StationModels, st, Transform::from_translation(st.pos).with_rotation(rot), Visibility::default())).with_children(|c| {
            for p in parts {
                let mat = cache
                    .entry((st.urn, p.mat as u8))
                    .or_insert_with(|| {
                        let (base, glow) = p.mat.look(color);
                        let lin = base.to_linear();
                        materials.add(StandardMaterial {
                            base_color: base,
                            emissive: LinearRgba::rgb(lin.red * glow, lin.green * glow, lin.blue * glow),
                            perceptual_roughness: if matches!(p.mat, Mat::Steel | Mat::Brass | Mat::Iron) { 0.5 } else { 0.95 },
                            ..default()
                        })
                    })
                    .clone();
                let key = [p.size.x.to_bits(), p.size.y.to_bits(), p.size.z.to_bits()];
                let mesh = cubes.entry(key).or_insert_with(|| meshes.add(Cuboid::new(p.size.x, p.size.y, p.size.z))).clone();
                c.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::from_translation(p.at).with_rotation(Quat::from_rotation_z(p.tilt)), NotShadowCaster));
            }
        });
    }
}

fn aim(cam: Single<&Transform, With<FlyCamera>>, stations: Query<&Station>, mut aimed: ResMut<StationAim>) {
    let (o, d) = (cam.translation, cam.forward().as_vec3());
    let best = stations
        .iter()
        .filter(|s| s.kind != StationKind::Bedframe)
        .filter_map(|s| {
            let (min, max) = s.bounds();
            ray_box(o, d, min, max).filter(|t| *t <= REACH).map(|t| (*s, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    aimed.0 = best.map(|(s, t)| (s.kind, t));
    aimed.1 = best.map(|(s, _)| s);
}

/// Right click the ground with an urn in hand: set it down there.
#[allow(clippy::too_many_arguments)]
fn place_urn(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    menus: Res<Menus>,
    aimed: Res<StationAim>,
    target: Res<crate::interact::Target>,
    cam: Single<&Transform, With<FlyCamera>>,
    (book, items, wielded): (Option<Res<UrnBook>>, Res<Items>, Res<Wielded>),
    (world, blocks): (Res<World>, Res<Blocks>),
    mut inv: ResMut<Inventory>,
    mut urns: ResMut<Urns>,
    mut toast: MessageWriter<Toast>,
) {
    if !mouse.just_pressed(MouseButton::Right) || cursor.grab_mode == CursorGrabMode::None || menus.open {
        return;
    }
    let (Some(book), Some(item), Some(hit)) = (book, wielded.0, target.hit) else { return };
    let Some(def) = book.0.urn_of_item(&items.get(item).name) else { return };
    if aimed.0.is_some() {
        return;
    }
    if hit.before.y != hit.block.y + 1 {
        toast.write(Toast(format!("Set the {} down on top of the ground", def.title)));
        return;
    }
    let free = world.block(hit.before).is_none_or(|b| !blocks.registry.get(b).solid);
    if !free || !inv.take(item, 1) {
        return;
    }
    let at = Vec3::new(hit.before.x as f32 + 0.5, hit.before.y as f32, hit.before.z as f32 + 0.5);
    let mut forward = cam.forward().as_vec3();
    forward.y = 0.0;
    urns.placed.insert(def.name.clone(), placed_facing(at, forward));
    toast.write(Toast(format!("Set down the {}. Right click it with an empty hand to see its tree", def.title)));
}

/// Right click a station: an urn takes what you hold (or shows its tree);
/// benches open their boards; storage opens.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn use_station(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    (mut menus, mut at_bench, mode, player): (ResMut<Menus>, ResMut<board::AtBench>, Res<crate::player::MoveMode>, Res<crate::player::Player>),
    (aimed, target, cam): (Res<StationAim>, Res<crate::interact::Target>, Single<&Transform, With<FlyCamera>>),
    (book, wielded, items): (Option<Res<UrnBook>>, Res<Wielded>, Res<Items>),
    (mut inv, mut chests, mut urns, mut tree): (ResMut<Inventory>, ResMut<Chests>, ResMut<Urns>, ResMut<tree::TreeUi>),
    (mut toast, mut done, mut open_chest, mut spill): (MessageWriter<Toast>, MessageWriter<StepDone>, MessageWriter<crate::interact::OpenChest>, MessageWriter<Spill>),
) {
    if !mouse.just_pressed(MouseButton::Right) || cursor.grab_mode == CursorGrabMode::None || menus.open {
        return;
    }
    let (Some((kind, dist)), Some(st)) = (aimed.0, aimed.1) else { return };
    // A block in front of the station takes the click instead.
    if let Some(hit) = target.hit {
        let min = Vec3::new(hit.block.x as f32, hit.block.y as f32, hit.block.z as f32);
        if ray_box(cam.translation, cam.forward().as_vec3(), min, min + Vec3::ONE).is_some_and(|t| t < dist) {
            return;
        }
    }
    let Some(book) = book else { return };
    let def = &book.0.urns[st.urn];
    match kind {
        StationKind::Urn => {
            let Some(item) = wielded.0 else {
                if player.crouching
                    && let Some(it) = def.item.as_deref().and_then(|n| items.id(n))
                {
                    // Pick the urn back up; what it holds stays with it.
                    urns.placed.remove(&def.name);
                    if let Some(rest) = inv.add(&items, Stack::new(it, 1)) {
                        spill.write(Spill { stack: rest, at: player.pos + Vec3::Y });
                    }
                    toast.write(Toast(format!("Picked up the {}", def.title)));
                    return;
                }
                tree.open = Some(st.urn);
                tree.dirty = true;
                menus.open = true;
                return;
            };
            let name = items.get(item).name.clone();
            let title = items.get(item).title.clone();
            let progress = urns.of(&def.name);
            let wants = progress.wants(def, &name);
            if wants == 0 {
                let next = progress.next(def).map_or("Every step is done".to_string(), |s| format!("Next: {}", needs_line(&items, progress, s)));
                toast.write(Toast(format!("The {} doesn't need {title} now. {next}", def.title)));
                return;
            }
            let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let n = if shift { inv.count(item).min(wants) } else { 1 };
            if n == 0 || !inv.take(item, n) {
                return;
            }
            let (put, finished) = urns.of_mut(&def.name).offer(def, &name, n, None);
            if put < n {
                inv.add(&items, Stack::new(item, (n - put) as u16));
            }
            if finished.is_empty() {
                let p = urns.of(&def.name);
                let next = p.next(def).map_or(String::new(), |s| format!("  {}: {}", s.title, needs_line(&items, p, s)));
                toast.write(Toast(format!("{title} x{put} into the {}.{next}", def.title)));
            }
            for s in finished {
                done.write(StepDone(s));
            }
        }
        StationKind::CampMat | StationKind::ToolBench | StationKind::CarpentryBench => {
            if *mode == crate::player::MoveMode::Fly {
                toast.write(Toast("Land to work at the bench".into()));
                return;
            }
            at_bench.enter(st, player.pos + Vec3::Y * player.eye);
            menus.open = true;
            menus.still = true;
        }
        StationKind::PersonalStorage => {
            let at = BlockPos::new(crate::inventory::STORAGE_AT.0, crate::inventory::STORAGE_AT.1, crate::inventory::STORAGE_AT.2);
            let want = if urns.has("storage_shelf") { crate::inventory::STORAGE_BIG } else { crate::inventory::CHEST };
            let grid = chests.map.entry(at).or_insert_with(|| Grid::new(want.0, want.1));
            if (grid.w, grid.h) != want {
                // The shelf came (or the save loaded at full size): move everything into a grid of the right size.
                let old = std::mem::replace(grid, Grid::new(want.0, want.1));
                for p in old.items {
                    if !grid.place(&items, p.stack, p.x, p.y, p.rotated)
                        && let Some(rest) = grid.add(&items, p.stack)
                    {
                        spill.write(Spill { stack: rest, at: player.pos + Vec3::Y });
                    }
                }
            }
            open_chest.write(crate::interact::OpenChest(at));
        }
        _ => {
            let line = kind.step().and_then(|s| book.0.step(s)).map_or(String::new(), |(_, s)| s.desc.clone());
            toast.write(Toast(format!("{}: {line} Using it by hand comes in a later update.", kind.title())));
        }
    }
}

/// "Sticks 2/4, Plant Fiber 0/4".
pub fn needs_line(items: &Items, p: &Progress, step: &urns::Step) -> String {
    step.needs
        .iter()
        .map(|(it, n)| {
            let title = items.id(it).map_or(it.as_str(), |i| items.get(i).title.as_str());
            format!("{title} {}/{n}", p.given(&step.name, it).min(*n))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Hand out what finished steps give.
#[allow(clippy::too_many_arguments)]
fn steps_done(
    mut done: MessageReader<StepDone>,
    book: Option<Res<UrnBook>>,
    benches: Res<board::Benches>,
    items: Res<Items>,
    player: Res<crate::player::Player>,
    mut inv: ResMut<Inventory>,
    mut toast: MessageWriter<Toast>,
    (mut spill, mut gain): (MessageWriter<Spill>, MessageWriter<crate::progress::GainXp>),
) {
    let Some(book) = book else { return };
    for StepDone(name) in done.read() {
        let Some((urn, step)) = book.0.step(name) else { continue };
        let line = match &step.gives {
            Gives::Station(_) => format!("The {} places a {} beside it", urn.title, step.title),
            Gives::Upgrade => format!("{} done", step.title),
            Gives::Recipes => {
                let taught: Vec<String> =
                    benches.0.recipes.iter().filter(|r| r.unlock.as_deref() == Some(name.as_str())).map(|r| items.get(r.makes.item).title.clone()).collect();
                if taught.is_empty() { format!("{} done", step.title) } else { format!("{}: you can now make {}", step.title, taught.join(", ")) }
            }
            Gives::Urn(other) => {
                let Some(def) = book.0.urn(other) else { continue };
                if let Some(it) = def.item.as_deref().and_then(|n| items.id(n))
                    && let Some(rest) = inv.add(&items, Stack::new(it, 1))
                {
                    spill.write(Spill { stack: rest, at: player.pos + Vec3::Y });
                }
                format!("The {} gives you the {}. Right click the ground to set it down", urn.title, def.title)
            }
        };
        toast.write(Toast(line));
        gain.write(crate::progress::GainXp { xp: 10.0 * step.tier as f32, why: step.title.clone() });
    }
}

/// Benches know the recipes finished steps teach.
fn sync_learned(urns: Res<Urns>, mut benches: ResMut<board::Benches>) {
    if !urns.is_changed() && !benches.is_added() {
        return;
    }
    let learned: std::collections::BTreeSet<String> = urns.progress.values().flat_map(|p| p.done.iter().cloned()).collect();
    if benches.0.learned != learned {
        benches.0.learned = learned;
    }
}

fn spawn_label(mut commands: Commands, theme: Res<Theme>) {
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), top: Val::Percent(54.0), justify_content: JustifyContent::Center, ..default() },
            Visibility::Hidden,
            StationLabel,
        ))
        .with_children(|c| {
            c.spawn((Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)), ..default() }, theme.skin("tooltip")))
                .with_children(|c| {
                    c.spawn((theme.bold(sizes::BODY), Text::new(""), TextColor(colors::HEADING)));
                    c.spawn((crate::theme::text("", sizes::SMALL, colors::TEXT), Node::default()));
                });
        });
}

/// The name of the station you look at, and for an urn what it needs next.
#[allow(clippy::too_many_arguments)]
fn show_label(
    aimed: Res<StationAim>,
    urns: Res<Urns>,
    book: Option<Res<UrnBook>>,
    items: Res<Items>,
    menus: Res<Menus>,
    mut label: Query<(&mut Visibility, &Children), With<StationLabel>>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    let Ok((mut vis, kids)) = label.single_mut() else { return };
    let Some(book) = book else { return };
    let (title, line) = match aimed.1 {
        Some(st) if !menus.open => {
            let def = &book.0.urns[st.urn];
            match st.kind {
                StationKind::Urn => {
                    let p = urns.of(&def.name);
                    let next = match p.next(def) {
                        Some(s) => format!("{}: {}", s.title, needs_line(&items, p, s)),
                        None => "Every step is done".into(),
                    };
                    (format!("{} · {}", def.title, def.station), format!("{next}\nHold an item and right click to put it in · empty hand: see the tree"))
                }
                StationKind::CampMat | StationKind::ToolBench | StationKind::CarpentryBench => (st.kind.title().to_string(), "Right click to work it with your hands".into()),
                StationKind::PersonalStorage => (st.kind.title().to_string(), "Right click to open".into()),
                k => {
                    let desc = k.step().and_then(|s| book.0.step(s)).map_or(String::new(), |(_, s)| s.desc.clone());
                    (k.title().to_string(), desc)
                }
            }
        }
        _ => {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            return;
        }
    };
    if *vis != Visibility::Inherited {
        *vis = Visibility::Inherited;
    }
    let Some(panel) = kids.first() else { return };
    let Ok(lines) = children.get(*panel) else { return };
    for (e, s) in lines.iter().zip([title, line]) {
        if let Ok(mut t) = texts.get_mut(e)
            && t.0 != s
        {
            t.0 = s;
        }
    }
}
