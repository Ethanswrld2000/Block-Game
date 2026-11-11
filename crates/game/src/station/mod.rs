//! The sleeping station: your bedroll, the ancestral urn beside it, and the
//! stations the urn places as it fills.
//!
//! Unrolling the bedroll (B) sets the urn down next to it. Right click the urn
//! with an item in hand to put one in (Shift: the whole stack); each item gives
//! fusion (`assets/data/fusion.ron`). When the urn holds enough for the next
//! upgrade it spends it and places that station beside the urn. Rolling the
//! bedroll up packs everything; unrolling it sets it all down again. Marks
//! around the urn's rim light up as the next upgrade fills.
//!
//! The stations are models, not blocks, so they move with your camp. Looking at
//! one shows its name and what the urn needs next.

pub mod fusion;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use voxel::BlockPos;

use crate::blocks::Blocks;
use crate::camera::{FlyCamera, Menus};
use crate::interact::{ray_box, Wielded, REACH};
use crate::inventory::Inventory;
use crate::items::Items;
use crate::save::SaveDir;
use crate::survival::camp::Camp;
use crate::survival::Toast;
use crate::theme::{colors, sizes, Theme};
use crate::world::World;
use fusion::{FusionData, Urn};

pub struct StationPlugin;

impl Plugin for StationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UrnState>()
            .init_resource::<StationAim>()
            .add_systems(PreStartup, load_fusion)
            .add_systems(Startup, (load_urn, spawn_label))
            .add_systems(
                Update,
                (aim, use_station, place_stations, show_label).chain().before(crate::interact::InteractSet).after(crate::player::PlayerSet),
            )
            .add_systems(Update, (save_urn, camp_for_screenshot))
            .add_systems(Last, save_urn_on_exit);
    }
}

/// Fusion data, from `assets/data/fusion.ron`.
#[derive(Resource)]
pub struct Fusion(pub FusionData);

/// The urn's contents, saved to `urn.ron`.
#[derive(Resource, Default)]
pub struct UrnState(pub Urn);

/// What can stand at the sleeping station.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationKind {
    Urn,
    ToolBench,
    CarpentryBench,
}

impl StationKind {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "tool_bench" => Some(Self::ToolBench),
            "carpentry_bench" => Some(Self::CarpentryBench),
            _ => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Urn => "Ancestral Urn",
            Self::ToolBench => "Tool Bench",
            Self::CarpentryBench => "Carpentry Bench",
        }
    }

    /// Half size of its box, for aiming.
    fn half(self) -> Vec3 {
        match self {
            Self::Urn => Vec3::new(0.32, 0.45, 0.32),
            Self::ToolBench => Vec3::new(0.6, 0.45, 0.4),
            Self::CarpentryBench => Vec3::new(0.85, 0.45, 0.45),
        }
    }
}

/// A station standing at the camp. `pos` is the middle of its foot.
#[derive(Component, Debug, Clone, Copy)]
pub struct Station {
    pub kind: StationKind,
    pub pos: Vec3,
    /// Turned a quarter (long side along z).
    pub turned: bool,
}

impl Station {
    fn bounds(&self) -> (Vec3, Vec3) {
        let mut h = self.kind.half();
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
pub struct StationAim(pub Option<(StationKind, f32)>);

/// Everything spawned for the camp, rebuilt when it changes.
#[derive(Component)]
struct CampModels;

#[derive(Component)]
struct StationLabel;

fn load_fusion(mut commands: Commands) {
    match FusionData::from_ron(include_str!("../../../../assets/data/fusion.ron")) {
        Ok(d) => commands.insert_resource(Fusion(d)),
        Err(e) => error!("{e}"),
    }
}

fn screenshot_run() -> bool {
    std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() || std::env::var_os("BLOCKGAME_BENCH").is_some()
}

fn load_urn(dir: Res<SaveDir>, mut urn: ResMut<UrnState>) {
    if !screenshot_run()
        && let Ok(text) = std::fs::read_to_string(dir.file("urn.ron"))
    {
        match ron::from_str::<Urn>(&text) {
            Ok(u) => urn.0 = u,
            Err(e) => warn!("urn.ron: {e}"),
        }
    }
    // `BLOCKGAME_URN=tool_bench,earth=25` starts with those upgrades done and
    // that much fusion in the urn (screenshots).
    if let Ok(v) = std::env::var("BLOCKGAME_URN") {
        for part in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            match part.split_once('=') {
                Some((k, n)) => {
                    urn.0.fusion.insert(k.to_string(), n.parse().unwrap_or(0.0));
                }
                None => urn.0.done.push(part.to_string()),
            }
        }
    }
}

fn store(dir: &SaveDir, urn: &Urn) {
    match ron::ser::to_string_pretty(urn, ron::ser::PrettyConfig::default()) {
        Ok(text) => {
            if let Err(e) = std::fs::write(dir.file("urn.ron"), text) {
                warn!("saving urn.ron: {e}");
            }
        }
        Err(e) => warn!("urn.ron: {e}"),
    }
}

fn save_urn(dir: Res<SaveDir>, urn: Res<UrnState>) {
    if urn.is_changed() && !urn.is_added() && !screenshot_run() {
        store(&dir, &urn.0);
    }
}

fn save_urn_on_exit(mut exit: MessageReader<AppExit>, dir: Res<SaveDir>, urn: Res<UrnState>) {
    if exit.read().next().is_some() && !screenshot_run() {
        store(&dir, &urn.0);
    }
}

/// `BLOCKGAME_CAMP=1` unrolls the bedroll a few blocks in front of you as soon
/// as you stand on the ground (screenshots).
fn camp_for_screenshot(
    player: Res<crate::player::Player>,
    cam: Single<&FlyCamera>,
    mut camp: ResMut<Camp>,
    mut done: Local<bool>,
) {
    if *done || std::env::var_os("BLOCKGAME_CAMP").is_none() || !player.on_ground {
        return;
    }
    *done = true;
    let (sy, cy) = cam.yaw.sin_cos();
    let forward = Vec3::new(-sy, 0.0, -cy);
    let at = player.pos + forward * 3.5;
    camp.bedroll = Some(crate::survival::camp::Bedroll {
        pos: (at.x.round(), player.pos.y.floor(), at.z.round()),
        along_x: forward.x.abs() <= forward.z.abs(),
    });
}

/// Where each station stands around the bedroll: the urn on the far side from
/// where you wake, the benches along the bedroll on either side of the urn.
fn layout(camp: &Camp, world: &World, blocks: &Blocks, urn: &Urn, data: &FusionData) -> Vec<Station> {
    let Some(b) = camp.bedroll else { return Vec::new() };
    let (along, side) = if b.along_x { (Vec3::X, Vec3::Z) } else { (Vec3::Z, Vec3::X) };
    let c = b.center();
    let urn_at = c - side * 1.6;
    let spots = [urn_at + along * 1.7, urn_at - along * 2.0, urn_at - side * 1.6, urn_at - side * 1.6 + along * 1.8];
    let mut out = vec![Station { kind: StationKind::Urn, pos: ground(world, blocks, urn_at), turned: false }];
    let placed = urn.done.iter().filter_map(|d| data.upgrades.iter().find(|u| &u.name == d)).filter_map(|u| StationKind::from_name(&u.station));
    for (kind, spot) in placed.zip(spots) {
        out.push(Station { kind, pos: ground(world, blocks, spot), turned: !b.along_x });
    }
    out
}

/// The top of the ground under a spot, near the camp's height.
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

/// What the camp's models were last built for: bedroll spot, upgrades done, marks lit.
type CampKey = (Option<(i32, i32, i32, bool)>, usize, u32);

/// Keep the camp's models in step with the bedroll and the urn.
#[allow(clippy::too_many_arguments)]
fn place_stations(
    mut commands: Commands,
    camp: Res<Camp>,
    urn: Res<UrnState>,
    fusion: Option<Res<Fusion>>,
    world: Res<World>,
    blocks: Res<Blocks>,
    existing: Query<Entity, With<CampModels>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut last: Local<Option<CampKey>>,
) {
    let Some(fusion) = fusion else { return };
    let key = camp.bedroll.map(|b| ((b.pos.0 * 2.0) as i32, (b.pos.1 * 2.0) as i32, (b.pos.2 * 2.0) as i32, b.along_x));
    let marks = (urn.0.progress(&fusion.0) * MARKS as f32).floor() as u32;
    let now = (key, urn.0.done.len(), marks);
    if *last == Some(now) {
        return;
    }
    *last = Some(now);
    for e in &existing {
        commands.entity(e).despawn();
    }
    let mut mat = |c: Color, glow: f32| {
        let lin = c.to_linear();
        materials.add(StandardMaterial {
            base_color: c,
            emissive: LinearRgba::rgb(lin.red * glow, lin.green * glow, lin.blue * glow),
            perceptual_roughness: 0.95,
            ..default()
        })
    };
    let clay = mat(Color::srgb(0.58, 0.36, 0.24), 0.0);
    let clay_dark = mat(Color::srgb(0.42, 0.25, 0.17), 0.0);
    let wood = mat(Color::srgb(0.5, 0.36, 0.22), 0.0);
    let wood_dark = mat(Color::srgb(0.34, 0.24, 0.15), 0.0);
    let groove = mat(Color::srgb(0.22, 0.16, 0.1), 0.0);
    let stone = mat(Color::srgb(0.5, 0.5, 0.52), 0.0);
    let next_color = urn.0.next(&fusion.0).and_then(|u| u.needs.keys().next()).and_then(|k| fusion.0.kind(k)).map_or((0.8, 0.8, 0.6), |k| k.color);
    let lit = mat(Color::srgb(next_color.0, next_color.1, next_color.2), 3.0);
    let unlit = mat(Color::srgb(0.25, 0.2, 0.16), 0.0);
    let mut cube = |x: f32, y: f32, z: f32| meshes.add(Cuboid::new(x, y, z));
    let mut parts = |kind: StationKind| -> Vec<(Handle<Mesh>, Handle<StandardMaterial>, Vec3)> {
        match kind {
            StationKind::Urn => {
                let mut v = vec![
                    (cube(0.42, 0.1, 0.42), clay_dark.clone(), Vec3::new(0.0, 0.05, 0.0)),
                    (cube(0.56, 0.36, 0.56), clay.clone(), Vec3::new(0.0, 0.28, 0.0)),
                    (cube(0.44, 0.16, 0.44), clay.clone(), Vec3::new(0.0, 0.54, 0.0)),
                    (cube(0.3, 0.14, 0.3), clay_dark.clone(), Vec3::new(0.0, 0.69, 0.0)),
                    (cube(0.4, 0.06, 0.4), clay.clone(), Vec3::new(0.0, 0.79, 0.0)),
                    (cube(0.2, 0.02, 0.2), groove.clone(), Vec3::new(0.0, 0.825, 0.0)),
                ];
                // Marks around the belly, lit as the next upgrade fills.
                for i in 0..MARKS {
                    let a = i as f32 / MARKS as f32 * std::f32::consts::TAU;
                    let (s, c) = a.sin_cos();
                    // On the square belly's faces: push each mark out to the nearest face.
                    let k = 0.29 / s.abs().max(c.abs());
                    let m = if i < marks { lit.clone() } else { unlit.clone() };
                    v.push((cube(0.07, 0.07, 0.07), m, Vec3::new(s * k, 0.3, c * k)));
                }
                v
            }
            StationKind::ToolBench => vec![
                // Legs, top, the two carved slots (handle, material) and a strike stone.
                (cube(0.1, 0.72, 0.1), wood_dark.clone(), Vec3::new(-0.48, 0.36, -0.3)),
                (cube(0.1, 0.72, 0.1), wood_dark.clone(), Vec3::new(0.48, 0.36, -0.3)),
                (cube(0.1, 0.72, 0.1), wood_dark.clone(), Vec3::new(-0.48, 0.36, 0.3)),
                (cube(0.1, 0.72, 0.1), wood_dark.clone(), Vec3::new(0.48, 0.36, 0.3)),
                (cube(1.2, 0.1, 0.8), wood.clone(), Vec3::new(0.0, 0.77, 0.0)),
                (cube(0.62, 0.012, 0.14), groove.clone(), Vec3::new(-0.15, 0.826, -0.12)),
                (cube(0.26, 0.012, 0.26), groove.clone(), Vec3::new(0.32, 0.826, 0.12)),
                (cube(0.22, 0.14, 0.18), stone.clone(), Vec3::new(0.42, 0.89, -0.24)),
            ],
            StationKind::CarpentryBench => vec![
                (cube(0.12, 0.72, 0.12), wood_dark.clone(), Vec3::new(-0.7, 0.36, -0.32)),
                (cube(0.12, 0.72, 0.12), wood_dark.clone(), Vec3::new(0.7, 0.36, -0.32)),
                (cube(0.12, 0.72, 0.12), wood_dark.clone(), Vec3::new(-0.7, 0.36, 0.32)),
                (cube(0.12, 0.72, 0.12), wood_dark.clone(), Vec3::new(0.7, 0.36, 0.32)),
                (cube(1.7, 0.12, 0.9), wood.clone(), Vec3::new(0.0, 0.78, 0.0)),
                // A vise at one end and a saw slot along the front.
                (cube(0.2, 0.2, 0.3), wood_dark.clone(), Vec3::new(0.72, 0.94, 0.0)),
                (cube(1.0, 0.012, 0.06), groove.clone(), Vec3::new(-0.2, 0.846, 0.3)),
            ],
        }
    };
    for st in layout(&camp, &world, &blocks, &urn.0, &fusion.0) {
        let rot = if st.turned { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
        commands.spawn((CampModels, st, Transform::from_translation(st.pos).with_rotation(rot), Visibility::default())).with_children(|c| {
            for (mesh, m, at) in parts(st.kind) {
                c.spawn((Mesh3d(mesh), MeshMaterial3d(m), Transform::from_translation(at), NotShadowCaster));
            }
        });
    }
}

/// Marks around the urn.
const MARKS: u32 = 10;

fn aim(cam: Single<&Transform, With<FlyCamera>>, stations: Query<&Station>, mut aimed: ResMut<StationAim>) {
    let (o, d) = (cam.translation, cam.forward().as_vec3());
    aimed.0 = stations
        .iter()
        .filter_map(|s| {
            let (min, max) = s.bounds();
            ray_box(o, d, min, max).filter(|t| *t <= REACH).map(|t| (s.kind, t))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
}

/// Right click a station: the urn takes what you hold; benches come next.
#[allow(clippy::too_many_arguments)]
fn use_station(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    menus: Res<Menus>,
    aimed: Res<StationAim>,
    target: Res<crate::interact::Target>,
    cam: Single<&Transform, With<FlyCamera>>,
    fusion: Option<Res<Fusion>>,
    wielded: Res<Wielded>,
    items: Res<Items>,
    mut inv: ResMut<Inventory>,
    mut urn: ResMut<UrnState>,
    mut toast: MessageWriter<Toast>,
) {
    if !mouse.just_pressed(MouseButton::Right) || cursor.grab_mode == CursorGrabMode::None || menus.open {
        return;
    }
    let Some((kind, dist)) = aimed.0 else { return };
    // A block in front of the station takes the click instead.
    if let Some(hit) = target.hit {
        let min = Vec3::new(hit.block.x as f32, hit.block.y as f32, hit.block.z as f32);
        if ray_box(cam.translation, cam.forward().as_vec3(), min, min + Vec3::ONE).is_some_and(|t| t < dist) {
            return;
        }
    }
    let Some(fusion) = fusion else { return };
    match kind {
        StationKind::Urn => {
            let Some(item) = wielded.0 else {
                toast.write(Toast(urn_summary(&urn.0, &fusion.0)));
                return;
            };
            let def = items.get(item);
            if fusion.0.of(&def.name).is_none() {
                toast.write(Toast(format!("The urn won't take {}", def.title)));
                return;
            }
            let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            let n = if shift { inv.count(item) } else { 1 };
            if n == 0 || !inv.take(item, n) {
                return;
            }
            urn.0.deposit(&fusion.0, &def.name, n);
            let done = urn.0.upgrade(&fusion.0);
            if done.is_empty() {
                toast.write(Toast(format!("{} x{n} into the urn. {}", def.title, next_line(&urn.0, &fusion.0))));
            }
            for d in done {
                let title = fusion.0.upgrades.iter().find(|u| u.name == d).map_or(d.clone(), |u| u.title.clone());
                toast.write(Toast(format!("The urn places a {title} beside it")));
            }
        }
        StationKind::ToolBench | StationKind::CarpentryBench => {
            toast.write(Toast(format!("The {} isn't ready to work yet", kind.title())));
        }
    }
}

/// "Earth 4 / 10 for the Tool Bench", or that everything is built.
fn next_line(urn: &Urn, data: &FusionData) -> String {
    match urn.next(data) {
        None => "Every upgrade is done".into(),
        Some(u) => {
            let parts: Vec<String> = u
                .needs
                .iter()
                .map(|(k, n)| format!("{} {:.0} / {:.0}", data.kind(k).map_or(k.as_str(), |x| x.title.as_str()), urn.get(k).floor(), n))
                .collect();
            format!("{} for the {}", parts.join(", "), u.title)
        }
    }
}

fn urn_summary(urn: &Urn, data: &FusionData) -> String {
    let held: Vec<String> = data.kinds.iter().filter(|k| urn.get(&k.name) > 0.0).map(|k| format!("{} {:.1}", k.title, urn.get(&k.name))).collect();
    let held = if held.is_empty() { "Empty".to_string() } else { held.join(", ") };
    format!("Ancestral Urn: {held}. {}", next_line(urn, data))
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

/// The name of the station you look at, and for the urn what it needs next.
fn show_label(
    aimed: Res<StationAim>,
    urn: Res<UrnState>,
    fusion: Option<Res<Fusion>>,
    menus: Res<Menus>,
    mut label: Query<(&mut Visibility, &Children), With<StationLabel>>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    let Ok((mut vis, kids)) = label.single_mut() else { return };
    let Some(fusion) = fusion else { return };
    let (title, line) = match aimed.0 {
        Some((kind, _)) if !menus.open => {
            let line = match kind {
                StationKind::Urn => format!("{}  ·  right click to put in what you hold", next_line(&urn.0, &fusion.0)),
                _ => "Coming soon: work it with your hands".to_string(),
            };
            (kind.title().to_string(), line)
        }
        _ => {
            *vis = Visibility::Hidden;
            return;
        }
    };
    *vis = Visibility::Inherited;
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
