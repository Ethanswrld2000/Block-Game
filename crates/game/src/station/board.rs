//! Working a bench with your hands, not through a menu.
//!
//! Right click a bench (camp mat, tool bench, carpentry bench, cooking pot,
//! cutting board) and the view glides down over its top. Your things lie in a strip along the bottom of the screen: click one
//! to pick it up (Shift: the whole stack); it hangs under the cursor over the
//! board. Click a slot to set it down there, or click what lies in a slot to
//! pick it back up. When the slots match a recipe (`assets/data/benches.ron`),
//! work the bench's tool (knock the strike stone, saw) until the new thing lies
//! on the board, then click it
//! to take it. Right click puts what you hold back; A / D steps to the next
//! bench at the camp; Esc (or walking off) leaves, and everything on the board
//! goes back into your pockets.
//!
//! The cooking pot takes things into the pot in any order (click the pot with
//! something in hand; with an empty hand you take the last kind back out),
//! fuel goes into the fire beside it, and the ladle stirs: what's in the pot
//! swirls round with every stir. The cutting board's knife cuts what lies on it.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::bench::{slots_of, BenchData, Board, SlotDef, SlotKind, Special, Work};
use super::{Edges, Station, StationKind, Urns};
use crate::camera::{FlyCamera, Menus};
use crate::interact::ray_box;
use crate::inventory::grid::Stack;
use crate::inventory::{Inventory, Spill};
use crate::item_art::ItemArt;
use crate::items::{ItemId, Items};
use crate::player::{MoveMode, Player};
use crate::sound::PlaySound;
use crate::survival::Toast;
use crate::theme::{colors, sizes, Theme};

/// Benches with a board, and their names in urns.ron / benches.ron.
pub(super) fn board_name(kind: StationKind) -> Option<&'static str> {
    match kind {
        StationKind::CampMat => Some("camp_mat"),
        StationKind::ToolBench => Some("tool_bench"),
        StationKind::CarpentryBench => Some("carpentry_bench"),
        StationKind::CookingPot => Some("cooking_pot"),
        StationKind::CuttingBoard => Some("cutting_board"),
        StationKind::SmokingRack => Some("smoking_rack"),
        StationKind::IronAnvil => Some("iron_anvil"),
        StationKind::SharpeningStone => Some("sharpening_stone"),
        StationKind::MagicalLens => Some("magical_lens"),
        StationKind::Cauldron => Some("cauldron"),
        _ => None,
    }
}

/// Recipes worked at the benches.
#[derive(Resource, Default)]
pub struct Benches(pub BenchData);

/// You are at a bench: which one, what lies on its board, what you hold.
#[derive(Resource, Default)]
pub struct AtBench {
    pub station: Option<Station>,
    /// The side of the bench you look from (+1 or -1 along its depth).
    side: f32,
    /// 0 = your own eyes, 1 = looking down at the board.
    blend: f32,
    /// The board view, kept while gliding back out.
    view: Option<Transform>,
    pub board: Board,
    held: Option<Stack>,
    /// Where the held thing hangs, following the cursor.
    hand: Option<Vec3>,
    /// A knock just happened: things on the board jump.
    bump: f32,
    /// The board changed: rebuild its models.
    dirty: bool,
    /// Shown under the bench's name for a moment.
    note: Option<(String, f32)>,
    /// How far what's in the pot has swirled round, and where it's going.
    spin: f32,
    spin_to: f32,
    /// What the magical lens showed last, until the board changes.
    study: Option<String>,
}

impl AtBench {
    pub fn open(&self) -> bool {
        self.station.is_some()
    }

    /// Step up to a bench, from where the camera stands.
    pub fn enter(&mut self, st: Station, eye: Vec3) {
        let local = rotation(&st).inverse() * (eye - st.pos);
        self.side = if local.z < 0.0 { -1.0 } else { 1.0 };
        self.view = Some(board_view(&st, self.side));
        self.station = Some(st);
        self.dirty = true;
        self.note = None;
    }
}

/// Where things are on a bench's top, in its own frame (x along its length,
/// z across, y up from its foot).
pub(super) struct Layout {
    /// Slots: name, centre (x, z), half size, the height things lie at, and
    /// how to say where it is ("in the groove").
    slots: &'static [(&'static str, Vec2, Vec2, f32, &'static str)],
    /// The tool you work (strike stone, saw): its box.
    tool: (Vec3, Vec3),
    /// Where a finished thing lies: centre, half size, height.
    made: (Vec2, Vec2, f32),
    /// The view: how high above the top and how far out to the side.
    view: (f32, f32),
    /// "Knock the strike stone", and what one go is called ("knock").
    work: (&'static str, &'static str),
    /// The sound of one go.
    sound: &'static str,
    /// How big things look lying here (1 = as on the tool bench).
    scale: f32,
    /// The middle of the pot, for stations whose slots mix (things swirl round it).
    pot: Vec2,
}

const TOOL_BENCH: Layout = Layout {
    slots: &[
        ("handle", Vec2::new(-0.15, -0.12), Vec2::new(0.34, 0.11), 0.833, "in the groove"),
        ("material", Vec2::new(0.32, 0.12), Vec2::new(0.17, 0.17), 0.833, "in the hollow"),
    ],
    tool: (Vec3::new(0.31, 0.82, -0.33), Vec3::new(0.53, 0.96, -0.15)),
    made: (Vec2::new(-0.2, 0.17), Vec2::new(0.28, 0.13), 0.833),
    view: (0.95, 0.62),
    work: ("Knock the strike stone", "knock"),
    sound: "dig_hard",
    scale: 1.0,
    pot: Vec2::ZERO,
};

const CARPENTRY_BENCH: Layout = Layout {
    slots: &[
        ("bed", Vec2::new(-0.25, 0.0), Vec2::new(0.42, 0.18), 0.846, "on the bed"),
        ("vise", Vec2::new(0.72, 0.0), Vec2::new(0.12, 0.17), 1.04, "in the vise"),
    ],
    tool: (Vec3::new(-0.25, 0.83, -0.38), Vec3::new(0.42, 0.93, -0.22)),
    made: (Vec2::new(0.2, 0.3), Vec2::new(0.25, 0.1), 0.846),
    view: (1.25, 0.8),
    work: ("Saw", "stroke of the saw"),
    sound: "dig_wood",
    scale: 1.0,
    pot: Vec2::ZERO,
};

/// The camp mat lies on the ground: things go on the mat or by the cord, and
/// the flat knapping stone is what you knock.
const CAMP_MAT: Layout = Layout {
    slots: &[
        ("base", Vec2::new(-0.22, 0.05), Vec2::new(0.28, 0.24), 0.035, "on the mat"),
        ("tie", Vec2::new(0.22, -0.2), Vec2::new(0.16, 0.13), 0.035, "by the cord"),
    ],
    tool: (Vec3::new(0.21, 0.0, 0.1), Vec3::new(0.49, 0.16, 0.36)),
    made: (Vec2::new(-0.22, -0.33), Vec2::new(0.26, 0.08), 0.035),
    view: (1.05, 0.6),
    work: ("Knock the knapping stone", "knock"),
    sound: "dig_hard",
    scale: 1.0,
    pot: Vec2::ZERO,
};

/// Where the cooking pot's mouth is, in its own frame (see `models`).
const POT: Vec2 = Vec2::new(0.06, 0.0);
/// The broth's surface, where things float.
const BROTH: f32 = 0.614;

/// The cooking pot: four quarters of the pot's mouth (one kind of thing in
/// each; clicking anywhere on the pot fills them in turn), the fire's mouth
/// beside it, and the ladle on its stone.
const COOKING_POT: Layout = Layout {
    slots: &[
        ("pot_1", Vec2::new(POT.x - 0.11, -0.11), Vec2::new(0.11, 0.11), BROTH, "in the pot"),
        ("pot_2", Vec2::new(POT.x + 0.11, -0.11), Vec2::new(0.11, 0.11), BROTH, "in the pot"),
        ("pot_3", Vec2::new(POT.x - 0.11, 0.11), Vec2::new(0.11, 0.11), BROTH, "in the pot"),
        ("pot_4", Vec2::new(POT.x + 0.11, 0.11), Vec2::new(0.11, 0.11), BROTH, "in the pot"),
        ("fire", Vec2::new(-0.47, 0.0), Vec2::new(0.17, 0.16), 0.035, "in the fire"),
    ],
    tool: (Vec3::new(0.49, 0.0, -0.2), Vec3::new(0.71, 0.16, 0.22)),
    made: (POT, Vec2::new(0.21, 0.21), BROTH),
    view: (0.85, 0.6),
    work: ("Stir with the ladle", "stir"),
    sound: "stir",
    scale: 0.75,
    pot: POT,
};

/// The cutting board: what you cut lies on the board, the knife along the
/// back, and the cut pieces go into the dish.
const CUTTING_BOARD: Layout = Layout {
    slots: &[("board", Vec2::new(-0.14, -0.02), Vec2::new(0.24, 0.2), 0.48, "on the board")],
    tool: (Vec3::new(0.02, 0.46, 0.15), Vec3::new(0.42, 0.53, 0.24)),
    made: (Vec2::new(0.26, -0.08), Vec2::new(0.13, 0.13), 0.512),
    view: (0.8, 0.55),
    work: ("Cut with the knife", "cut"),
    sound: "knife",
    scale: 0.85,
    pot: Vec2::ZERO,
};

/// The smoking rack: meat lies on the grate, fuel goes into the fire's mouth
/// at the end, and the reed fan on the ground works the smoke.
const SMOKING_RACK: Layout = Layout {
    slots: &[
        ("rack", Vec2::new(-0.05, 0.0), Vec2::new(0.42, 0.17), 1.02, "on the rack"),
        ("fire", Vec2::new(0.65, 0.0), Vec2::new(0.15, 0.15), 0.035, "in the fire"),
    ],
    tool: (Vec3::new(-0.89, 0.0, -0.12), Vec3::new(-0.61, 0.08, 0.28)),
    made: (Vec2::new(-0.05, 0.0), Vec2::new(0.42, 0.17), 1.02),
    view: (0.8, 0.7),
    work: ("Fan the smoke", "wave of the fan"),
    sound: "swing",
    scale: 1.0,
    pot: Vec2::ZERO,
};

/// The iron anvil: metal on the anvil's face, flux in the tray and the handle
/// on the bench beside it, and the hammer on the bench.
const IRON_ANVIL: Layout = Layout {
    slots: &[
        ("anvil", Vec2::new(0.1, 0.0), Vec2::new(0.27, 0.14), 0.78, "on the anvil"),
        ("flux", Vec2::new(-0.6, 0.16), Vec2::new(0.16, 0.08), 0.545, "in the flux tray"),
        ("handle", Vec2::new(-0.6, -0.02), Vec2::new(0.22, 0.09), 0.525, "on the bench"),
    ],
    tool: (Vec3::new(-0.8, 0.5, -0.25), Vec3::new(-0.45, 0.62, -0.13)),
    made: (Vec2::new(0.1, 0.0), Vec2::new(0.27, 0.14), 0.78),
    view: (1.0, 0.65),
    work: ("Strike with the hammer", "blow of the hammer"),
    sound: "dig_hard",
    scale: 1.0,
    pot: Vec2::ZERO,
};

/// The sharpening stone: a tool on the rest in front of the wheel; you turn
/// the wheel, and it comes back sharp where it lay.
const SHARPENING_STONE: Layout = Layout {
    slots: &[("edge", Vec2::new(0.2, 0.13), Vec2::new(0.19, 0.11), 0.53, "on the rest")],
    tool: (Vec3::new(-0.13, 0.52, -0.29), Vec3::new(0.03, 1.04, 0.25)),
    made: (Vec2::new(0.2, 0.13), Vec2::new(0.19, 0.11), 0.53),
    view: (0.8, 0.62),
    work: ("Turn the wheel", "turn of the wheel"),
    sound: "dig_hard",
    scale: 0.8,
    pot: Vec2::ZERO,
};

/// The magical lens: something on the plate under the crystal ring, and the
/// focusing knob on top of the post.
const MAGICAL_LENS: Layout = Layout {
    slots: &[("plate", Vec2::new(0.0, 0.0), Vec2::new(0.2, 0.2), 0.095, "on the plate")],
    tool: (Vec3::new(-0.36, 0.74, -0.24), Vec3::new(-0.24, 0.86, -0.12)),
    made: (Vec2::new(0.0, 0.0), Vec2::new(0.2, 0.2), 0.095),
    view: (0.95, 0.55),
    work: ("Turn the focusing knob", "turn of the knob"),
    sound: "spell_off",
    scale: 1.0,
    pot: Vec2::ZERO,
};

/// Where the cauldron's mouth is, and the brew's surface.
const CAULDRON_AT: Vec2 = Vec2::new(0.08, 0.0);
const BREW: f32 = 0.64;

/// The cauldron: three parts of the brew across its mouth (clicking anywhere
/// on it fills them in turn), the fire's mouth at -x, and the paddle at +x.
const CAULDRON: Layout = Layout {
    slots: &[
        ("brew_1", Vec2::new(CAULDRON_AT.x - 0.17, 0.0), Vec2::new(0.085, 0.25), BREW, "in the cauldron"),
        ("brew_2", Vec2::new(CAULDRON_AT.x, 0.0), Vec2::new(0.085, 0.25), BREW, "in the cauldron"),
        ("brew_3", Vec2::new(CAULDRON_AT.x + 0.17, 0.0), Vec2::new(0.085, 0.25), BREW, "in the cauldron"),
        ("fire", Vec2::new(-0.47, 0.0), Vec2::new(0.15, 0.15), 0.035, "in the fire"),
    ],
    tool: (Vec3::new(0.51, 0.0, -0.2), Vec3::new(0.73, 0.16, 0.22)),
    made: (CAULDRON_AT, Vec2::new(0.25, 0.25), BREW),
    view: (0.85, 0.6),
    work: ("Stir with the paddle", "stir"),
    sound: "stir",
    scale: 0.75,
    pot: CAULDRON_AT,
};

pub(super) fn layout(kind: StationKind) -> &'static Layout {
    match kind {
        StationKind::CampMat => &CAMP_MAT,
        StationKind::CarpentryBench => &CARPENTRY_BENCH,
        StationKind::CookingPot => &COOKING_POT,
        StationKind::CuttingBoard => &CUTTING_BOARD,
        StationKind::SmokingRack => &SMOKING_RACK,
        StationKind::IronAnvil => &IRON_ANVIL,
        StationKind::SharpeningStone => &SHARPENING_STONE,
        StationKind::MagicalLens => &MAGICAL_LENS,
        StationKind::Cauldron => &CAULDRON,
        _ => &TOOL_BENCH,
    }
}

impl Layout {
    fn slot(&self, name: &str) -> (Vec2, Vec2, f32, &'static str) {
        self.slots.iter().find(|s| s.0 == name).map_or((Vec2::ZERO, Vec2::ZERO, self.made.2, "on the bench"), |s| (s.1, s.2, s.3, s.4))
    }

    /// The main working height.
    fn top(&self) -> f32 {
        self.made.2
    }

    /// The places things can be set down, each said once ("in the pot or in the fire").
    fn places(&self) -> String {
        let mut out: Vec<&str> = Vec::new();
        for s in self.slots {
            if !out.contains(&s.4) {
                out.push(s.4);
            }
        }
        out.join(" or ")
    }
}

fn rotation(st: &Station) -> Quat {
    if st.turned { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY }
}

fn to_world(st: &Station, local: Vec3) -> Vec3 {
    st.pos + rotation(st) * local
}

/// Looking down at the bench top from the side you stand on.
fn board_view(st: &Station, side: f32) -> Transform {
    let l = layout(st.kind);
    let eye = to_world(st, Vec3::new(0.0, l.top() + l.view.0, side * l.view.1));
    let at = to_world(st, Vec3::new(0.0, l.top(), side * 0.04));
    Transform::from_translation(eye).looking_at(at, Vec3::Y)
}

/// What the cursor is over on the bench.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spot {
    /// The bench's tool (strike stone, saw).
    Tool,
    Slot(SlotDef),
    Made,
}

/// The ray in the bench's frame: what it meets first, and where it crosses the top.
fn spot(st: &Station, name: &str, made: bool, origin: Vec3, dir: Vec3) -> (Option<Spot>, Option<Vec3>) {
    let l = layout(st.kind);
    let inv = rotation(st).inverse();
    let (o, d) = (inv * (origin - st.pos), inv * dir);
    let cross = |y: f32| (d.y < -1e-4).then(|| o + d * ((y - o.y) / d.y));
    let top = cross(l.top());
    if ray_box(o, d, l.tool.0, l.tool.1).is_some() {
        return (Some(Spot::Tool), top.map(|p| to_world(st, p)));
    }
    let inside = |y: f32, c: Vec2, h: Vec2| cross(y).is_some_and(|p| (p.x - c.x).abs() <= h.x && (p.z - c.y).abs() <= h.y);
    let hit = if made && inside(l.made.2, l.made.0, l.made.1) {
        Some(Spot::Made)
    } else {
        slots_of(name)
            .iter()
            .find(|s| {
                let (c, h, y, _) = l.slot(s.name);
                inside(y, c, h)
            })
            .map(|s| Spot::Slot(*s))
    };
    (hit, top.map(|p| to_world(st, p)))
}

/// Something lying on the board, and the height it rests at.
#[derive(Component)]
pub(super) struct BoardModel(f32);

#[derive(Component)]
pub(super) struct HeldModel;

/// Something floating in the pot: it swirls round the pot's middle when stirred.
#[derive(Component)]
pub(super) struct Swirl {
    centre: Vec3,
    off: Vec3,
    rot: Quat,
}

#[derive(Component)]
pub(super) struct BoardUi;

#[derive(Component)]
pub(super) struct Strip;

#[derive(Component)]
pub(super) struct StripCell(ItemId);

#[derive(Component)]
pub(super) struct HintTitle;

#[derive(Component)]
pub(super) struct HintLine;

#[derive(Component)]
pub(super) struct RecipeLines;

pub(super) fn load_benches(mut commands: Commands, items: Res<Items>) {
    match BenchData::from_ron(include_str!("../../../../assets/data/benches.ron"), &items) {
        Ok(d) => commands.insert_resource(Benches(d)),
        Err(e) => {
            error!("{e}");
            commands.insert_resource(Benches::default());
        }
    }
}

/// Put a stack back in the pockets, or at your feet if they're full.
fn give_back(inv: &mut Inventory, items: &Items, spill: &mut MessageWriter<Spill>, at: Vec3, stack: Stack) {
    if let Some(rest) = inv.add(items, stack) {
        spill.write(Spill { stack: rest, at });
    }
}

/// Leave the bench: everything goes back into the pockets.
fn leave(at: &mut AtBench, inv: &mut Inventory, items: &Items, spill: &mut MessageWriter<Spill>, feet: Vec3) {
    for s in at.held.take().into_iter().chain(at.board.clear()) {
        give_back(inv, items, spill, feet, s);
    }
    at.station = None;
    at.hand = None;
    at.dirty = true;
}

/// Glide the camera between your eyes and the board, after the player has placed it.
pub(super) fn board_camera(
    time: Res<Time>,
    mut at: ResMut<AtBench>,
    mut cam: Single<&mut Transform, With<FlyCamera>>,
) {
    let want = if at.open() { 1.0 } else { 0.0 };
    let dt = time.delta_secs().min(0.1);
    let blend = if (at.blend - want).abs() < 1e-3 { want } else { at.blend + (want - at.blend) * (1.0 - (-dt * 7.0).exp()) };
    at.blend = blend;
    if blend <= 0.0 {
        at.view = None;
        return;
    }
    let Some(view) = at.view else { return };
    let k = blend * blend * (3.0 - 2.0 * blend);
    // A knock shakes the view a little.
    let shake = at.bump * at.bump * 0.006 * (time.elapsed_secs() * 70.0).sin();
    cam.translation = cam.translation.lerp(view.translation, k) + Vec3::Y * shake;
    cam.rotation = cam.rotation.slerp(view.rotation, k);
}

/// At the bench only the board shows: the HUD (every other top-level UI node)
/// and your arms are put away, and come back as they were when you leave.
#[allow(clippy::type_complexity)]
pub(super) fn board_hides(
    at: Res<AtBench>,
    mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>, Without<BoardUi>)>,
    mut hands: Query<&mut Visibility, (With<crate::hands::HandRoot>, Without<Node>)>,
    all: Query<Entity, (With<Node>, Without<ChildOf>, Without<BoardUi>)>,
    mut hidden: Local<Vec<(Entity, Visibility)>>,
) {
    let away = at.blend > 0.02;
    if away && hidden.is_empty() {
        for e in &all {
            if let Ok(mut v) = roots.get_mut(e)
                && *v != Visibility::Hidden
            {
                hidden.push((e, *v));
                *v = Visibility::Hidden;
            }
        }
    } else if !away && !hidden.is_empty() {
        for (e, prev) in hidden.drain(..) {
            if let Ok(mut v) = roots.get_mut(e)
                && *v == Visibility::Hidden
            {
                *v = prev;
            }
        }
    }
    let want = if away { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut hands {
        if *v != want {
            *v = want;
        }
    }
}

/// Keys at the bench: Esc leaves, A / D steps to the next bench.
#[allow(clippy::too_many_arguments)]
pub(super) fn board_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut at: ResMut<AtBench>,
    mut menus: ResMut<Menus>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    stations: Query<&Station>,
    (items, player, mode): (Res<Items>, Res<Player>, Res<MoveMode>),
    mut inv: ResMut<Inventory>,
    mut spill: MessageWriter<Spill>,
) {
    let Some(st) = at.station else { return };
    // The bench is gone (bedroll packed up), or you started flying.
    let still_there = stations.iter().any(|s| s.kind == st.kind && s.pos.distance(st.pos) < 0.01);
    let quit = keys.just_pressed(KeyCode::Escape) || !still_there || *mode == MoveMode::Fly;
    if quit {
        leave(&mut at, &mut inv, &items, &mut spill, player.pos + Vec3::Y);
        menus.open = false;
        menus.still = false;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        return;
    }
    let step = keys.just_pressed(KeyCode::KeyD) as i32 - keys.just_pressed(KeyCode::KeyA) as i32;
    if step != 0 {
        // Benches in a line along the camp, ordered by where they stand.
        let mut benches: Vec<Station> = stations.iter().filter(|s| board_name(s.kind).is_some()).copied().collect();
        benches.sort_by(|a, b| (a.pos.x + a.pos.z).total_cmp(&(b.pos.x + b.pos.z)));
        if let Some(i) = benches.iter().position(|s| s.kind == st.kind) {
            let next = benches[(i as i32 + step).rem_euclid(benches.len() as i32) as usize];
            if next.kind != st.kind {
                // What lies on this board goes back first.
                leave(&mut at, &mut inv, &items, &mut spill, player.pos + Vec3::Y);
                let eye = player.pos + Vec3::Y * player.eye;
                at.enter(next, eye);
            }
        }
    }
}

/// Clicks at the bench: the strip, the slots, the strike stone.
#[allow(clippy::too_many_arguments)]
pub(super) fn board_click(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &Transform), With<FlyCamera>>,
    cells: Query<(&StripCell, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
    (items, benches, player): (Res<Items>, Res<Benches>, Res<Player>),
    mut at: ResMut<AtBench>,
    mut inv: ResMut<Inventory>,
    (mut spill, mut sound, mut toast, mut gain): (MessageWriter<Spill>, MessageWriter<PlaySound>, MessageWriter<Toast>, MessageWriter<crate::progress::GainXp>),
    prog: Option<Res<crate::progress::xp::Progression>>,
    (mut urns, mut edges): (ResMut<Urns>, ResMut<Edges>),
    (recipes, drops, blocks): (Option<Res<crate::inventory::crafting::Recipes>>, Option<Res<crate::inventory::drops::Drops>>, Res<crate::blocks::Blocks>),
) {
    let Some(st) = at.station else { return };
    let Some(name) = board_name(st.kind) else { return };
    let feet = player.pos + Vec3::Y;
    if mouse.just_pressed(MouseButton::Right) {
        if let Some(h) = at.held.take() {
            give_back(&mut inv, &items, &mut spill, feet, h);
        }
        return;
    }
    if !mouse.just_pressed(MouseButton::Left) || at.blend < 0.6 {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // The strip of your things.
    if let Some(phys) = window.physical_cursor_position()
        && let Some((cell, ..)) = cells.iter().find(|(_, n, t, v)| v.get() && n.contains_point(**t, phys))
    {
        let item = cell.0;
        if let Some(h) = at.held
            && h.item != item
        {
            give_back(&mut inv, &items, &mut spill, feet, h);
            at.held = None;
        }
        let have = at.held.map_or(0, |h| h.count);
        let room = items.get(item).stack.saturating_sub(have) as u32;
        let n = if shift { inv.count(item).min(room) } else { 1.min(room) };
        if n > 0 && inv.take(item, n) {
            at.held = Some(Stack::new(item, have + n as u16));
            sound.write(PlaySound::new("dig_plant").gain(0.4));
        }
        return;
    }
    let Some(pos) = window.cursor_position() else { return };
    let (cam, tf) = *camera;
    let Ok(ray) = cam.viewport_to_world(&GlobalTransform::from(*tf), pos) else { return };
    let (hit, _) = spot(&st, name, at.board.made.is_some(), ray.origin, ray.direction.as_vec3());
    match hit {
        Some(Spot::Tool) => {
            let special = benches.0.matching(name, &at.board).map(|r| r.special);
            let work = at.board.strike(&benches.0, name);
            match (work, special) {
                (Work::Made(s), Some(Special::Sharpen)) => {
                    sound.write(PlaySound::new("dig_hard").gain(0.9));
                    sound.write(PlaySound::new("spell_on").gain(0.4));
                    at.dirty = true;
                    if let Some(sh) = benches.0.sharpen {
                        edges.left.insert(items.get(s.item).name.clone(), sh.seconds);
                        edges.speed = sh.speed;
                        let line = format!("Your {} is sharp: {:.1}x as fast for {:.0} minutes", items.get(s.item).title, sh.speed, sh.seconds / 60.0);
                        at.note = Some((line.clone(), 4.0));
                        toast.write(Toast(line));
                    }
                }
                (Work::Made(s), Some(Special::Study)) => {
                    sound.write(PlaySound::new("spell_on").gain(0.6));
                    at.dirty = true;
                    let item_name = items.get(s.item).name.clone();
                    let new = urns.studied.insert(item_name.clone());
                    let brews: Vec<String> = benches
                        .0
                        .recipes
                        .iter()
                        .filter(|r| r.taught_by(&format!("lens:{item_name}")) && r.unlock.as_deref().is_some_and(|u| !u.contains('+')))
                        .map(|r| items.get(r.makes.item).title.clone())
                        .collect();
                    let mut line = format!("You studied the {}", items.get(s.item).title);
                    if new && !brews.is_empty() {
                        line = format!("{line}: the cauldron can brew {} now", brews.join(", "));
                        gain.write(crate::progress::GainXp { xp: 15.0, why: String::new() });
                    }
                    at.note = Some((line.clone(), 4.0));
                    toast.write(Toast(line));
                    at.study = Some(study_text(s.item, &items, &benches.0, recipes.as_deref(), drops.as_deref(), &blocks));
                }
                (Work::Nothing, _) => {
                    sound.write(PlaySound::new("hit").gain(0.35));
                    let line = if at.board.made.is_some() {
                        "Take what you made first".to_string()
                    } else if let Some((r, have)) = benches.0.needs_heat(name, &at.board) {
                        format!("The fire is too low for {}: put more fuel in it ({have} of {} heat)", items.get(r.makes.item).title, r.heat)
                    } else {
                        "That doesn't make anything here".to_string()
                    };
                    at.note = Some((line, 2.5));
                }
                (Work::Knock { .. }, _) => {
                    sound.write(PlaySound::new(layout(st.kind).sound).gain(0.9));
                    if !matches!(st.kind, StationKind::CookingPot | StationKind::Cauldron | StationKind::MagicalLens | StationKind::SmokingRack) {
                        sound.write(PlaySound::new("hit").gain(0.5));
                    }
                    at.bump = 1.0;
                    at.spin_to += 1.7;
                    at.dirty = true;
                }
                (Work::Made(s), _) => {
                    sound.write(PlaySound::new(layout(st.kind).sound).gain(0.9));
                    sound.write(PlaySound::new("place").gain(0.9));
                    at.bump = 1.0;
                    at.spin_to += 1.7;
                    at.dirty = true;
                    at.note = Some((format!("You made a {}", items.get(s.item).title), 3.0));
                    toast.write(Toast(format!("Made a {}", items.get(s.item).title)));
                    if let Some(p) = &prog {
                        gain.write(crate::progress::GainXp { xp: p.xp.craft, why: String::new() });
                    }
                }
            }
        }
        Some(Spot::Made) => {
            if let Some(s) = at.board.made.take() {
                give_back(&mut inv, &items, &mut spill, feet, s);
                sound.write(PlaySound::new("dig_plant").gain(0.5));
                at.dirty = true;
                at.note = None;
            }
        }
        Some(Spot::Slot(def)) => {
            let defs = slots_of(name);
            match (at.held.take(), def.kind) {
                (Some(h), SlotKind::Fire) if !benches.0.is_fuel(h.item) => {
                    at.held = Some(h);
                    at.note = Some((format!("{} won't burn", items.get(h.item).title), 2.0));
                    return;
                }
                (Some(h), SlotKind::Mix) => {
                    at.held = at.board.put_mix(defs, h);
                    if at.held == Some(h) {
                        at.note = Some(("The pot holds four kinds of things at most: take one out first".into(), 2.5));
                        return;
                    }
                }
                (None, SlotKind::Mix) => at.held = at.board.take_mix(defs),
                (Some(h), _) => at.held = at.board.put(def, h),
                (None, _) => at.held = at.board.take(def.name),
            }
            let s = match def.kind {
                SlotKind::Mix => PlaySound::new("stir").gain(0.35),
                SlotKind::Fire => PlaySound::new("dig_wood").gain(0.45),
                SlotKind::Plain => PlaySound::new("dig_hard").gain(0.45),
            };
            sound.write(s);
            at.dirty = true;
            at.study = None;
        }
        None => {}
    }
}

/// How big a thing lies on the board: long things fill the groove, small ones the hollow.
fn px_for(size: UVec2, scale: f32) -> f32 {
    let long = size.max_element() as f32;
    if long > 24.0 { 0.56 / long * scale } else { 0.15 / long * scale }
}

/// Where the `i`th of `n` small things lies, around a slot's middle: two
/// rows of up to four.
fn spread(i: usize, n: usize) -> Vec2 {
    let cols = n.min(4);
    let rows = n.div_ceil(4);
    let (cx, cy) = ((i % 4) as f32, (i / 4) as f32);
    Vec2::new((cx - (cols as f32 - 1.0) / 2.0) * 0.12, (cy - (rows as f32 - 1.0) / 2.0) * 0.13 + if i.is_multiple_of(2) { -0.02 } else { 0.02 })
}

/// Lying flat on the board, the sprite's top along the bench's length.
fn flat() -> Quat {
    Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// Models of what lies on the board, and of what you hold.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn board_models(
    mut commands: Commands,
    time: Res<Time>,
    mut at: ResMut<AtBench>,
    art: Option<Res<ItemArt>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &Transform), With<FlyCamera>>,
    mut models: Query<(Entity, &mut Transform, &BoardModel, Option<&Swirl>), Without<FlyCamera>>,
    mut held_model: Query<(Entity, &mut Transform), (With<HeldModel>, Without<BoardModel>, Without<FlyCamera>)>,
    mut showing: Local<Option<Stack>>,
) {
    let Some(art) = art else { return };
    let dt = time.delta_secs().min(0.1);
    at.bump = (at.bump - dt * 3.5).max(0.0);
    let swirling = (at.spin_to - at.spin).abs() > 1e-3;
    at.spin = if swirling { at.spin + (at.spin_to - at.spin) * (1.0 - (-dt * 4.0).exp()) } else { at.spin_to };
    let scale = at.station.map_or(1.0, |st| layout(st.kind).scale);
    let spawn = |commands: &mut Commands, item: ItemId, at_world: Vec3, rot: Quat, marker: bool| -> Option<Entity> {
        let (mesh, mat) = art.model(item)?;
        let px = px_for(art.size(item)?, scale);
        let tf = Transform::from_translation(at_world + Vec3::Y * (px * 0.5 + 0.004)).with_rotation(rot).with_scale(Vec3::splat(px));
        let mut e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), tf, NotShadowCaster));
        if marker {
            e.insert(BoardModel(tf.translation.y));
        } else {
            e.insert(HeldModel);
        }
        Some(e.id())
    };
    // The board: rebuilt when it changes; things hop after a knock.
    if at.dirty {
        at.dirty = false;
        for (e, ..) in &models {
            commands.entity(e).despawn();
        }
        if let Some(st) = at.station {
            let r = rotation(&st) * flat();
            let l = layout(st.kind);
            let defs = slots_of(board_name(st.kind).unwrap_or_default());
            let spin = Quat::from_rotation_y(at.spin);
            for (slot, stack) in &at.board.slots {
                let (c, _, y, _) = l.slot(slot);
                // In the pot: closer to the middle, and turned by the stirring so far.
                let in_pot = defs.iter().any(|d| d.name == slot && d.kind == SlotKind::Mix);
                let c = if in_pot { l.pot + (c - l.pot) * 0.7 } else { c };
                // Several small things side by side, a little turned.
                let n = stack.count.min(8) as usize;
                // Long things (sticks) lie side by side across the slot, small ones in rows.
                let long = art.size(stack.item).is_some_and(|z| z.max_element() > 24);
                for i in 0..n {
                    let (off, turn) = if n == 1 {
                        (Vec2::ZERO, Quat::IDENTITY)
                    } else if long {
                        (Vec2::new(0.0, (i as f32 - (n as f32 - 1.0) / 2.0) * 0.07), Quat::IDENTITY)
                    } else {
                        (spread(i, n), Quat::from_rotation_y(i as f32 * 1.3))
                    };
                    let off = if in_pot { off * 0.6 } else { off };
                    let place = to_world(&st, Vec3::new(c.x + off.x, y, c.y + off.y));
                    if in_pot {
                        let centre = to_world(&st, Vec3::new(l.pot.x, y, l.pot.y));
                        let (o, rot) = (place - centre, turn * r);
                        if let Some(e) = spawn(&mut commands, stack.item, centre + spin * o, spin * rot, true) {
                            commands.entity(e).insert(Swirl { centre, off: o, rot });
                        }
                    } else {
                        spawn(&mut commands, stack.item, place, turn * r, true);
                    }
                }
            }
            if let Some(m) = at.board.made {
                let (c, _, y) = l.made;
                spawn(&mut commands, m.item, to_world(&st, Vec3::new(c.x, y, c.y)), r, true);
            }
        }
    } else if at.bump > 0.0 || swirling {
        let hop = (at.bump * std::f32::consts::PI).sin() * 0.03 * at.bump;
        let spin = Quat::from_rotation_y(at.spin);
        for (_, mut t, base, swirl) in &mut models {
            if let Some(sw) = swirl {
                // Floating things bob less and go round instead.
                let p = sw.centre + spin * sw.off;
                t.translation = Vec3::new(p.x, base.0 + hop * 0.3, p.z);
                t.rotation = spin * sw.rot;
            } else {
                t.translation.y = base.0 + hop;
            }
        }
    }
    // What you hold hangs under the cursor, just above the board.
    if *showing != at.held || at.station.is_none() {
        for (e, ..) in &held_model {
            commands.entity(e).despawn();
        }
        *showing = None;
    }
    let (Some(st), Some(h)) = (at.station, at.held) else {
        at.hand = None;
        return;
    };
    let (cam, tf) = *camera;
    let Some(target) = window
        .cursor_position()
        .and_then(|p| cam.viewport_to_world(&GlobalTransform::from(*tf), p).ok())
        .and_then(|ray| {
            let (o, d) = (ray.origin, ray.direction.as_vec3());
            let y = st.pos.y + layout(st.kind).top() + 0.1;
            (d.y < -1e-4).then(|| o + d * ((y - o.y) / d.y))
        })
    else {
        return;
    };
    let hand = at.hand.map_or(target, |p| p.lerp(target, 1.0 - (-dt * 16.0).exp()));
    at.hand = Some(hand);
    // Tilted the way it's being dragged.
    let lean = (target - hand) * 4.0;
    let rot = Quat::from_rotation_z(-lean.x.clamp(-0.5, 0.5)) * Quat::from_rotation_x(lean.z.clamp(-0.5, 0.5)) * rotation(&st) * flat();
    if *showing != Some(h) {
        spawn(&mut commands, h.item, hand, rot, false);
        *showing = Some(h);
    } else if let Some((_, mut t)) = held_model.iter_mut().next() {
        let px = t.scale.x;
        t.translation = hand + Vec3::Y * (px * 0.5);
        t.rotation = rot;
    }
}

pub(super) fn spawn_board_ui(mut commands: Commands, theme: Res<Theme>) {
    commands
        .spawn((
            BoardUi,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), flex_direction: FlexDirection::Column, justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, padding: UiRect::all(Val::Px(12.0)), ..default() },
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)), row_gap: Val::Px(2.0), ..default() }, theme.skin("tooltip")))
                .with_children(|c| {
                    c.spawn((HintTitle, theme.bold(sizes::HEADING), Text::new(""), TextColor(colors::HEADING)));
                    c.spawn((HintLine, crate::theme::text("", sizes::BODY, colors::TEXT), Node::default()));
                });
            root.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(6.0), ..default() }).with_children(|c| {
                c.spawn((RecipeLines, crate::theme::text("", sizes::SMALL, colors::MUTED), Node::default()));
                c.spawn((Strip, Node { column_gap: Val::Px(2.0), padding: UiRect::all(Val::Px(6.0)), ..default() }, theme.skin("panel_light")));
                c.spawn((
                    crate::theme::text("Click: pick up / set down   ·   Shift: whole stack   ·   Right click: put back   ·   A / D: next bench   ·   Esc: leave", sizes::SMALL, colors::FAINT),
                    Node::default(),
                ));
            });
        });
}

const CELL: f32 = 46.0;

/// The strip of your things, the bench's name and what to do next.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn board_ui(
    mut commands: Commands,
    time: Res<Time>,
    theme: Res<Theme>,
    mut at: ResMut<AtBench>,
    (items, inv, benches): (Res<Items>, Res<Inventory>, Res<Benches>),
    art: Option<Res<ItemArt>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut root: Query<&mut Visibility, With<BoardUi>>,
    strip: Query<(Entity, Option<&Children>), With<Strip>>,
    mut cells: Query<(&StripCell, &ComputedNode, &UiGlobalTransform, &mut ImageNode)>,
    mut texts: ParamSet<(Query<&mut Text, With<HintTitle>>, Query<&mut Text, With<HintLine>>, Query<&mut Text, With<RecipeLines>>)>,
    mut built: Local<Option<Vec<(ItemId, u32)>>>,
) {
    let Ok(mut vis) = root.single_mut() else { return };
    let Some(st) = at.station else {
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
        *built = None;
        return;
    };
    if *vis != Visibility::Inherited {
        *vis = Visibility::Inherited;
    }
    let Some(art) = art else { return };
    // Your things, one cell per kind, in pocket order.
    let mut list: Vec<(ItemId, u32)> = Vec::new();
    for p in &inv.pockets.items {
        if !list.iter().any(|(i, _)| *i == p.stack.item) {
            list.push((p.stack.item, inv.count(p.stack.item)));
        }
    }
    list.truncate(20);
    if built.as_ref() != Some(&list)
        && let Ok((strip, kids)) = strip.single()
    {
        for k in kids.into_iter().flatten() {
            commands.entity(*k).despawn();
        }
        commands.entity(strip).with_children(|row| {
            if list.is_empty() {
                row.spawn((crate::theme::text("Your pockets are empty", sizes::SMALL, colors::MUTED), Node { margin: UiRect::all(Val::Px(8.0)), ..default() }));
            }
            for (item, n) in &list {
                row.spawn((StripCell(*item), Node { width: Val::Px(CELL), height: Val::Px(CELL), ..default() }, theme.skin("slot"))).with_children(|c| {
                    crate::inventory::ui::icon(c, &items, &art, Stack::new(*item, (*n).min(u16::MAX as u32) as u16), CELL, CELL, false);
                    if *n > 1 {
                        c.spawn((
                            crate::theme::text(format!("{n}"), sizes::SMALL, colors::TEXT),
                            Node { position_type: PositionType::Absolute, right: Val::Px(4.0), bottom: Val::Px(0.0), ..default() },
                            GlobalZIndex(1),
                        ));
                    }
                });
            }
        });
        *built = Some(list);
    }
    // Hover.
    let phys = window.physical_cursor_position();
    for (_, node, tf, mut img) in &mut cells {
        let want = if phys.is_some_and(|p| node.contains_point(*tf, p)) { "slot_hover" } else { "slot" };
        let skin = theme.image(want);
        if img.rect != skin.rect {
            *img = skin;
        }
    }
    // What to do next.
    let name = board_name(st.kind).unwrap_or("");
    if let Some((_, left)) = &mut at.note {
        *left -= time.delta_secs();
    }
    if at.note.as_ref().is_some_and(|(_, left)| *left <= 0.0) {
        at.note = None;
    }
    let line = if let Some((n, _)) = &at.note {
        n.clone()
    } else if let Some(m) = at.board.made {
        format!("Click the {} to take it", items.get(m.item).title)
    } else if let Some(r) = benches.0.matching(name, &at.board) {
        let what = match r.special {
            Special::Sharpen => format!("sharpen the {}", items.get(r.makes.item).title),
            Special::Study => format!("study the {}", items.get(r.makes.item).title),
            Special::None => format!("make {}", a_or_some(&items, r.makes)),
        };
        format!("{} to {what}  ({} / {})", layout(st.kind).work.0, at.board.work, r.work)
    } else if let Some((r, have)) = benches.0.needs_heat(name, &at.board) {
        format!("Put fuel in the fire to make {}  ({have} of {} heat)", a_or_some(&items, r.makes), r.heat)
    } else if let Some(h) = at.held {
        if matches!(st.kind, StationKind::CookingPot | StationKind::Cauldron) && !benches.0.is_fuel(h.item) {
            format!("Put it {}", layout(st.kind).slots[0].4)
        } else {
            format!("Set it down {}", layout(st.kind).places())
        }
    } else {
        "Pick something from your pockets below".to_string()
    };
    let mut recipes: Vec<String> = benches
        .0
        .for_station(name)
        .map(|r| {
            let mut parts: Vec<String> = r
                .slots
                .iter()
                .map(|(slot, s)| {
                    let where_ = layout(st.kind).slot(slot).3;
                    format!("{} {where_}", a_or_some(&items, *s).to_lowercase())
                })
                .collect();
            if !r.mix.is_empty() {
                let things: Vec<String> = r.mix.iter().map(|s| a_or_some(&items, *s).to_lowercase()).collect();
                parts.push(format!("{} {}", and_list(&things), layout(st.kind).slots[0].4));
            }
            if r.heat > 0 {
                parts.push(format!("fuel worth {} heat in the fire", r.heat));
            }
            let (_, go) = layout(st.kind).work;
            let gos = if r.work == 1 { format!("1 {go}") } else { format!("{} {}", r.work, plural(go)) };
            let made = if r.makes.count > 1 { format!("{} x{}", items.get(r.makes.item).title, r.makes.count) } else { items.get(r.makes.item).title.clone() };
            format!("{made}: {}, then {gos}", and_list(&parts))
        })
        .collect();
    match st.kind {
        StationKind::SharpeningStone => {
            if let Some(sh) = benches.0.sharpen {
                recipes.push(format!(
                    "Any tool on the rest, then {} {}: it works {:.1}x as fast for {:.0} minutes",
                    sh.work,
                    plural(layout(st.kind).work.1),
                    sh.speed,
                    sh.seconds / 60.0
                ));
            }
        }
        StationKind::MagicalLens => match &at.study {
            Some(t) => recipes.push(t.clone()),
            None => recipes.push(
                "Anything on the plate, then focus the lens: it shows what the thing is made of, and teaches the cauldron the draughts it goes into".into(),
            ),
        },
        StationKind::Cauldron if recipes.is_empty() => recipes.push("The cauldron knows no draughts yet: study their parts under the magical lens".into()),
        _ => {}
    }
    if matches!(st.kind, StationKind::CookingPot | StationKind::SmokingRack | StationKind::Cauldron) && !benches.0.fuel.is_empty() {
        let mut fuel: Vec<(u32, &str)> = benches.0.fuel.iter().map(|(i, h)| (*h, items.get(*i).title.as_str())).collect();
        fuel.sort();
        let fuel: Vec<String> = fuel.iter().map(|(h, t)| format!("{t} {h}")).collect();
        recipes.push(format!("Heat from each fuel: {}", fuel.join(" · ")));
    }
    for (q, s) in [(0, st.kind.title().to_string()), (1, line), (2, recipes.join("\n"))] {
        let set = |t: &mut Text| {
            if t.0 != s {
                t.0 = s.clone();
            }
        };
        match q {
            0 => texts.p0().iter_mut().for_each(|mut t| set(&mut t)),
            1 => texts.p1().iter_mut().for_each(|mut t| set(&mut t)),
            _ => texts.p2().iter_mut().for_each(|mut t| set(&mut t)),
        }
    }
}

/// What the magical lens shows about a thing: what makes it, where it's
/// found, what it goes into, and the draughts it teaches the cauldron.
fn study_text(
    item: ItemId,
    items: &Items,
    benches: &BenchData,
    recipes: Option<&crate::inventory::crafting::Recipes>,
    drops: Option<&crate::inventory::drops::Drops>,
    blocks: &crate::blocks::Blocks,
) -> String {
    let title = |i: ItemId| items.get(i).title.clone();
    let station = |s: &str| StationKind::from_name(s).map_or(s.to_string(), |k| k.title().to_string());
    let mut lines = vec![format!("The flux of the {}", title(item))];
    let mut made: Vec<String> = Vec::new();
    for r in benches.recipes.iter().filter(|r| r.special == Special::None && r.makes.item == item) {
        let parts: Vec<String> = r.slots.values().chain(&r.mix).map(|s| a_or_some(items, *s).to_lowercase()).collect();
        let line = format!("{}: {}", station(&r.station), and_list(&parts));
        if !made.contains(&line) {
            made.push(line);
        }
    }
    for r in recipes.map(|r| r.list.as_slice()).unwrap_or_default().iter().filter(|r| r.makes.item == item) {
        let parts: Vec<String> = r.needs.iter().map(|s| a_or_some(items, *s).to_lowercase()).collect();
        made.push(format!("By hand: {}", and_list(&parts)));
    }
    if !made.is_empty() {
        lines.push(format!("Made from: {}", made.join(" · ")));
    }
    let mut found: Vec<String> = drops.map(|d| d.sources(item)).unwrap_or_default().iter().map(|b| blocks.registry.get(*b).name.replace('_', " ")).collect();
    if let crate::items::ItemKind::Block(b) = items.get(item).kind {
        found.push(format!("{} blocks", blocks.registry.get(b).name.replace('_', " ")));
    }
    if !found.is_empty() {
        lines.push(format!("Found in: {}", found.join(", ")));
    }
    let mut into: Vec<String> = Vec::new();
    for r in benches.recipes.iter().filter(|r| r.special == Special::None && r.slots.values().chain(&r.mix).any(|s| s.item == item)) {
        let t = title(r.makes.item);
        if !into.contains(&t) {
            into.push(t);
        }
    }
    for r in recipes.map(|r| r.list.as_slice()).unwrap_or_default().iter().filter(|r| r.needs.iter().any(|s| s.item == item)) {
        let t = title(r.makes.item);
        if !into.contains(&t) {
            into.push(t);
        }
    }
    if !into.is_empty() {
        lines.push(format!("Goes into: {}", into.join(", ")));
    }
    if made.is_empty() && into.is_empty() && lines.len() == 1 {
        lines.push("Nothing you know of is made from it".into());
    }
    lines.join("\n")
}

/// "a, b and c".
fn and_list(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// "a Stick", "2 Small Rocks".
fn a_or_some(items: &Items, s: Stack) -> String {
    let title = &items.get(s.item).title;
    if s.count == 1 { format!("a {title}") } else { format!("{} {}", s.count, plural(title)) }
}

fn plural(s: &str) -> String {
    match s.split_once(' ') {
        // "stroke of the saw" -> "strokes of the saw"
        Some((head, rest)) if rest.starts_with("of ") => format!("{} {rest}", plural(head)),
        _ if s.ends_with('s') => s.to_string(),
        _ => format!("{s}s"),
    }
}

/// `BLOCKGAME_BOARD=stick,small_rock*2` steps up to the tool bench as soon as
/// it stands, with those in its slots in order (one more lies there made;
/// `-` leaves a slot empty; `BLOCKGAME_BOARD=carpentry:log` for the carpentry
/// bench, `mat:small_rock*2,stick` for the camp mat, `pot:raw_meat,-,-,-,stick*2`
/// for the cooking pot (four parts of the pot, then the fire), `cut:raw_meat`
/// for the cutting board, any other bench by its name (`cauldron:moss*2,spirit_ember,-,coal`);
/// screenshots; needs the
/// bench standing: `BLOCKGAME_CAMP=1 BLOCKGAME_URN=camp_mat` for the mat,
/// `BLOCKGAME_URNS=tool BLOCKGAME_URN=tool_bench` for the tool bench).
#[allow(clippy::too_many_arguments)]
pub(super) fn board_for_screenshot(
    stations: Query<&Station>,
    items: Res<Items>,
    player: Res<Player>,
    mut at: ResMut<AtBench>,
    mut menus: ResMut<Menus>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Ok(v) = std::env::var("BLOCKGAME_BOARD") else {
        *done = true;
        return;
    };
    let (kind, v) = match v.split_once(':') {
        Some(("carpentry", rest)) => (StationKind::CarpentryBench, rest.to_string()),
        Some(("mat", rest)) => (StationKind::CampMat, rest.to_string()),
        Some(("pot", rest)) => (StationKind::CookingPot, rest.to_string()),
        Some(("cut", rest)) => (StationKind::CuttingBoard, rest.to_string()),
        // Any other bench by its urns.ron name: `smoking_rack:raw_meat*2,peat`.
        Some((k, rest)) if StationKind::from_name(k).and_then(board_name).is_some() => (StationKind::from_name(k).unwrap_or(StationKind::ToolBench), rest.to_string()),
        _ => (StationKind::ToolBench, v.clone()),
    };
    let Some(st) = stations.iter().find(|s| s.kind == kind) else { return };
    *done = true;
    at.enter(*st, player.pos + Vec3::Y * player.eye);
    menus.open = true;
    menus.still = true;
    let defs = slots_of(board_name(kind).unwrap_or_default());
    for (i, part) in v.split(',').map(str::trim).filter(|s| !s.is_empty()).enumerate() {
        let (name, n) = part.split_once('*').map_or((part, 1), |(a, b)| (a, b.parse().unwrap_or(1)));
        // "-" leaves a slot empty.
        let Some(id) = items.id(name) else { continue };
        if let Some(def) = defs.get(i) {
            at.board.put(*def, Stack::new(id, n));
        } else {
            at.board.made = Some(Stack::new(id, n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_looks_down_at_the_board_from_your_side() {
        for turned in [false, true] {
            let st = Station { kind: StationKind::ToolBench, pos: Vec3::new(10.0, 5.0, -3.0), turned, urn: 0 };
            for side in [-1.0, 1.0] {
                let v = board_view(&st, side);
                let f = v.forward().as_vec3();
                assert!(f.y < -0.6, "looks down: {f}");
                // Looking through the middle of the view lands on the board.
                let (_, top) = spot(&st, "tool_bench", false, v.translation, f);
                let top = top.unwrap();
                assert!((top - to_world(&st, Vec3::new(0.0, layout(st.kind).top(), side * 0.04))).length() < 0.05, "{top}");
            }
        }
    }

    #[test]
    fn clicks_find_slots_and_the_stone() {
        let st = Station { kind: StationKind::ToolBench, pos: Vec3::new(0.0, 0.0, 0.0), turned: true, urn: 0 };
        let down = Vec3::NEG_Y;
        let from = |x: f32, z: f32| to_world(&st, Vec3::new(x, 2.0, z));
        let name = |s: Option<Spot>| match s {
            Some(Spot::Slot(d)) => d.name,
            Some(Spot::Tool) => "stone",
            Some(Spot::Made) => "made",
            None => "none",
        };
        assert_eq!(name(spot(&st, "tool_bench", false, from(-0.3, -0.12), down).0), "handle");
        assert_eq!(name(spot(&st, "tool_bench", false, from(0.32, 0.15), down).0), "material");
        assert_eq!(name(spot(&st, "tool_bench", false, from(0.42, -0.24), down).0), "stone");
        assert_eq!(name(spot(&st, "tool_bench", true, from(-0.3, 0.2), down).0), "made");
        assert_eq!(name(spot(&st, "tool_bench", false, from(-0.3, 0.2), down).0), "none");
        let st = Station { kind: StationKind::CarpentryBench, ..st };
        assert_eq!(name(spot(&st, "carpentry_bench", false, from(-0.4, 0.05), down).0), "bed");
        assert_eq!(name(spot(&st, "carpentry_bench", false, from(0.72, 0.1), down).0), "vise");
        assert_eq!(name(spot(&st, "carpentry_bench", false, from(0.1, -0.3), down).0), "stone");
        assert_eq!(name(spot(&st, "carpentry_bench", true, from(0.2, 0.32), down).0), "made");
        let st = Station { kind: StationKind::CampMat, ..st };
        assert_eq!(name(spot(&st, "camp_mat", false, from(-0.3, 0.1), down).0), "base");
        assert_eq!(name(spot(&st, "camp_mat", false, from(0.25, -0.2), down).0), "tie");
        assert_eq!(name(spot(&st, "camp_mat", false, from(0.35, 0.23), down).0), "stone");
        assert_eq!(name(spot(&st, "camp_mat", true, from(-0.2, -0.33), down).0), "made");
        let st = Station { kind: StationKind::CookingPot, ..st };
        for (x, z) in [(0.0, -0.1), (0.15, -0.1), (0.0, 0.15), (0.2, 0.15)] {
            assert!(name(spot(&st, "cooking_pot", false, from(x, z), down).0).starts_with("pot_"), "{x} {z}");
        }
        assert_eq!(name(spot(&st, "cooking_pot", false, from(-0.5, 0.05), down).0), "fire");
        assert_eq!(name(spot(&st, "cooking_pot", false, from(0.6, -0.1), down).0), "stone");
        assert_eq!(name(spot(&st, "cooking_pot", true, from(0.1, 0.1), down).0), "made");
        let st = Station { kind: StationKind::CuttingBoard, ..st };
        assert_eq!(name(spot(&st, "cutting_board", false, from(-0.2, 0.0), down).0), "board");
        assert_eq!(name(spot(&st, "cutting_board", false, from(0.3, 0.19), down).0), "stone");
        assert_eq!(name(spot(&st, "cutting_board", true, from(0.26, -0.1), down).0), "made");
        assert_eq!(name(spot(&st, "cutting_board", false, from(0.26, -0.1), down).0), "none");
        let at = |kind, x: f32, z: f32, made: bool| {
            let st = Station { kind, ..st };
            name(spot(&st, board_name(kind).unwrap(), made, from(x, z), down).0)
        };
        assert_eq!(at(StationKind::SmokingRack, -0.2, 0.05, false), "rack");
        assert_eq!(at(StationKind::SmokingRack, 0.65, 0.0, false), "fire");
        assert_eq!(at(StationKind::SmokingRack, -0.75, 0.0, false), "stone");
        assert_eq!(at(StationKind::IronAnvil, 0.1, 0.0, false), "anvil");
        assert_eq!(at(StationKind::IronAnvil, -0.55, 0.17, false), "flux");
        assert_eq!(at(StationKind::IronAnvil, -0.5, -0.03, false), "handle");
        assert_eq!(at(StationKind::IronAnvil, -0.65, -0.19, false), "stone");
        assert_eq!(at(StationKind::SharpeningStone, 0.22, 0.14, false), "edge");
        assert_eq!(at(StationKind::SharpeningStone, -0.05, 0.0, false), "stone");
        assert_eq!(at(StationKind::MagicalLens, 0.05, 0.05, false), "plate");
        assert_eq!(at(StationKind::MagicalLens, -0.3, -0.18, false), "stone");
        assert!(at(StationKind::Cauldron, 0.1, 0.1, false).starts_with("brew_"));
        assert_eq!(at(StationKind::Cauldron, -0.47, 0.0, false), "fire");
        assert_eq!(at(StationKind::Cauldron, 0.62, 0.0, false), "stone");
        assert_eq!(at(StationKind::Cauldron, 0.1, 0.1, true), "made");
    }

    #[test]
    fn every_slot_has_a_place_on_its_bench() {
        for kind in [
            StationKind::CampMat,
            StationKind::ToolBench,
            StationKind::CarpentryBench,
            StationKind::CookingPot,
            StationKind::CuttingBoard,
            StationKind::SmokingRack,
            StationKind::IronAnvil,
            StationKind::SharpeningStone,
            StationKind::MagicalLens,
            StationKind::Cauldron,
        ] {
            for s in slots_of(board_name(kind).unwrap()) {
                assert!(layout(kind).slots.iter().any(|l| l.0 == s.name), "{kind:?} {}", s.name);
            }
        }
    }

    #[test]
    fn small_things_spread_without_overlapping() {
        for n in 2..=8 {
            let p: Vec<Vec2> = (0..n).map(|i| spread(i, n)).collect();
            for a in 0..n {
                for b in a + 1..n {
                    assert!(p[a].distance(p[b]) > 0.09, "{n}: {a} {b}");
                }
            }
        }
    }
}
