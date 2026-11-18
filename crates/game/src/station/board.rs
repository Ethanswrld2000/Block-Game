//! Working a bench with your hands, not through a menu.
//!
//! Right click a bench (camp mat, tool bench, carpentry bench or table, machination table, cooking pot,
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
use super::float::{self, Floater, Paddle, Tank};
use super::forge::Forge;
use super::particles::{Burst, BurstKind};
use super::models::{self, ToolUse};
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
        StationKind::CarpentryTable => Some("carpentry_table"),
        StationKind::MachinationTable => Some("machination_table"),
        StationKind::CookingPot => Some("cooking_pot"),
        StationKind::CuttingBoard => Some("cutting_board"),
        StationKind::SmokingRack => Some("smoking_rack"),
        StationKind::IronAnvil => Some("iron_anvil"),
        StationKind::SharpeningStone => Some("sharpening_stone"),
        StationKind::MagicalLens => Some("magical_lens"),
        StationKind::Cauldron => Some("cauldron"),
        StationKind::JoinersTable => Some("joiners_table"),
        StationKind::DoorFrame => Some("door_frame"),
        StationKind::GlassKiln => Some("glass_kiln"),
        StationKind::SewingTable => Some("sewing_table"),
        StationKind::ArmorStand => Some("armor_stand"),
        StationKind::BladesmithBench => Some("bladesmith_bench"),
        StationKind::GunsmithBench => Some("gunsmith_bench"),
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
    pub(super) blend: f32,
    /// The board view, kept while gliding back out.
    view: Option<Transform>,
    pub board: Board,
    held: Option<Stack>,
    /// Where the held thing hangs, following the cursor.
    hand: Option<Vec3>,
    /// A knock just happened: things on the board jump.
    bump: f32,
    /// The board changed: rebuild its models.
    pub(super) dirty: bool,
    /// Shown under the bench's name for a moment.
    note: Option<(String, f32)>,
    /// What floats in the pot (cooking pot, cauldron): kept between rebuilds.
    floaters: Vec<Floater>,
    /// Where the ladle's bowl was last frame, while stirring.
    paddle_was: Option<Vec2>,
    /// What the magical lens showed last, until the board changes.
    pub(super) study: Option<String>,
    /// Which page of your things the strip shows, and of the recipe book.
    strip_page: usize,
    pub(super) book_page: usize,
    /// Which of the bench's views you look from.
    view_i: usize,
    /// Something made, coming together.
    pub(super) forge: Option<Forge>,
    /// The bench's tool in your hand, or turning in place.
    pub tool: ToolHand,
}

/// The bench's tool: lifted off the bench (it follows the cursor), how far
/// a swing has got, how far a wheel or knob has turned, and goes done.
#[derive(Default)]
pub struct ToolHand {
    pub held: bool,
    /// The ladle or paddle is down in the pot, stirring.
    stirring: bool,
    /// Where its working end is, in the bench's frame.
    pos: Option<Vec3>,
    /// A swing under way (0..1).
    swing: Option<f32>,
    /// A wheel or knob: how far it has turned, and where it's going.
    turn: f32,
    turn_to: f32,
    /// Stirring or sawing with the button held.
    gesture: Gesture,
    /// The button has been let go since the tool was picked up.
    armed: bool,
    /// Goes worked this frame, for `board_click` to count.
    go: u32,
    /// Screenshots: hold the swing at this point, or press the tool down.
    freeze: Option<f32>,
    down: bool,
}

/// A stir is a full turn round the pot (either way, but not back and
/// forth); a stroke of the saw is `SAW_STROKE` of travel.
#[derive(Default, Clone, Copy, Debug)]
pub(super) struct Gesture {
    last: Option<f32>,
    sum: f32,
}

/// How far the saw travels for one stroke (bench lengths).
pub(super) const SAW_STROKE: f32 = 0.3;

impl Gesture {
    /// The tool's angle round the pot's middle; returns stirs done and how far it moved.
    pub(super) fn stir(&mut self, angle: f32) -> (u32, f32) {
        let mut d = 0.0;
        if let Some(l) = self.last {
            d = (angle - l + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            self.sum += d;
        }
        self.last = Some(angle);
        let n = (self.sum.abs() / std::f32::consts::TAU) as u32;
        self.sum -= self.sum.signum() * std::f32::consts::TAU * n as f32;
        (n, d)
    }

    /// The saw's place along the bench; returns strokes done.
    pub(super) fn saw(&mut self, x: f32) -> u32 {
        if let Some(l) = self.last {
            self.sum += (x - l).abs();
        }
        self.last = Some(x);
        let n = (self.sum / SAW_STROKE) as u32;
        self.sum -= n as f32 * SAW_STROKE;
        n
    }

    /// The button went up: the next press starts afresh (what was done so far stays).
    pub(super) fn lift(&mut self) {
        self.last = None;
    }
}

/// A bench's tool model (child of the station's models): where it rests and how it's worked.
#[derive(Component)]
pub struct BenchTool {
    pub rest: Vec3,
    pub rest_turn: Quat,
    pub how: ToolUse,
}

impl AtBench {
    pub fn open(&self) -> bool {
        self.station.is_some()
    }

    /// Step up to a bench, from where the camera stands.
    pub fn enter(&mut self, st: Station, eye: Vec3) {
        let local = rotation(&st).inverse() * (eye - st.pos);
        self.side = if local.z < 0.0 { -1.0 } else { 1.0 };
        self.view_i = 0;
        self.view = Some(board_view(&st, self.side, 0));
        self.tool = ToolHand::default();
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
    /// Where you can look from: the first is where you start; W / S switch.
    views: &'static [View],
    /// "Strike with the hammerstone", and what one go is called ("blow").
    pub(super) work: (&'static str, &'static str),
    /// What the bench's tool is called ("hammerstone").
    tool_name: &'static str,
    /// The sound of one go.
    sound: &'static str,
    /// How big things look lying here (1 = as on the tool bench).
    scale: f32,
    /// The middle of the pot, for stations whose slots mix.
    pot: Vec2,
    /// Things lie turned end for end (tools head first to the grindstone's wheel).
    flip: bool,
}

const TOOL_BENCH: Layout = Layout {
    slots: &[
        ("handle", Vec2::new(-0.15, -0.12), Vec2::new(0.34, 0.11), 0.833, "in the groove"),
        ("material", Vec2::new(0.32, 0.12), Vec2::new(0.17, 0.17), 0.833, "in the hollow"),
    ],
    // The hammer on its rack at the +x end.
    tool: (Vec3::new(0.44, 0.97, -0.24), Vec3::new(0.6, 1.16, 0.2)),
    made: (Vec2::new(-0.2, 0.17), Vec2::new(0.28, 0.13), 0.833),
    views: &[view("the bench", Vec3::new(0.0, 0.833, 0.04), Vec3::new(0.0, 0.95, 0.62))],
    work: ("Strike with the hammer", "blow of the hammer"),
    tool_name: "hammer",
    sound: "dig_hard",
    scale: 1.0,
    pot: Vec2::ZERO,
    flip: false,
};

const CARPENTRY_BENCH: Layout = Layout {
    slots: &[
        ("bed", Vec2::new(-0.25, 0.0), Vec2::new(0.42, 0.18), 0.846, "on the bed"),
        ("vise", Vec2::new(0.72, 0.0), Vec2::new(0.12, 0.17), 1.04, "in the vise"),
    ],
    tool: (Vec3::new(-0.25, 0.83, -0.38), Vec3::new(0.42, 0.93, -0.22)),
    made: (Vec2::new(0.2, 0.3), Vec2::new(0.25, 0.1), 0.846),
    views: &[view("the bench", Vec3::new(0.0, 0.846, 0.04), Vec3::new(0.0, 1.25, 0.8))],
    work: ("Saw", "stroke of the saw"),
    tool_name: "saw",
    sound: "dig_wood",
    scale: 1.0,
    pot: Vec2::ZERO,
    flip: false,
};

/// The carpentry table: the wood to join lies on the top, what joins it
/// (pegs, cord, mud) in the tray at the end, and the mallet knocks it together.
const CARPENTRY_TABLE: Layout = Layout {
    slots: &[
        ("top", Vec2::new(-0.2, -0.02), Vec2::new(0.4, 0.2), 0.86, "on the table"),
        ("joint", Vec2::new(0.55, -0.1), Vec2::new(0.14, 0.12), 0.895, "in the tray"),
    ],
    tool: (Vec3::new(0.42, 0.86, 0.12), Vec3::new(0.76, 1.0, 0.32)),
    made: (Vec2::new(-0.2, 0.3), Vec2::new(0.3, 0.1), 0.86),
    views: &[view("the table", Vec3::new(0.0, 0.86, 0.04), Vec3::new(0.0, 1.15, 0.75))],
    work: ("Knock it together with the mallet", "knock of the mallet"),
    tool_name: "mallet",
    sound: "dig_wood",
    scale: 1.0,
    pot: Vec2::ZERO,
    flip: false,
};

/// The machination table: the keel's wood on the slipway in the middle, the
/// rigging and the envelope in trays at the front and back, the engine's metal
/// in the gear box, and the drive wheel at the +x end that you turn.
const MACHINATION_TABLE: Layout = Layout {
    slots: &[
        ("keel", Vec2::new(-0.32, 0.02), Vec2::new(0.36, 0.16), 0.91, "on the slipway"),
        ("rigging", Vec2::new(0.24, 0.22), Vec2::new(0.16, 0.1), 0.925, "in the rigging tray"),
        ("envelope", Vec2::new(0.24, -0.18), Vec2::new(0.16, 0.1), 0.925, "in the envelope tray"),
        ("engine", Vec2::new(-0.32, 0.3), Vec2::new(0.3, 0.08), 0.925, "in the gear box"),
    ],
    // The drive wheel stands on its axle at the +x end.
    tool: (Vec3::new(0.62, 0.88, -0.3), Vec3::new(0.84, 1.44, 0.3)),
    made: (Vec2::new(-0.32, 0.02), Vec2::new(0.36, 0.16), 0.91),
    views: &[view("the table", Vec3::new(-0.05, 0.91, 0.04), Vec3::new(-0.05, 1.3, 0.85))],
    work: ("Turn the drive wheel", "turn of the drive wheel"),
    tool_name: "drive wheel",
    sound: "dig_hard",
    scale: 1.0,
    pot: Vec2::ZERO,
    flip: false,
};

/// The camp mat lies on the ground: things go on the mat or by the cord, and
/// the flat knapping stone is what you knock.
const CAMP_MAT: Layout = Layout {
    slots: &[
        ("base", Vec2::new(-0.22, 0.05), Vec2::new(0.28, 0.24), 0.035, "on the mat"),
        ("tie", Vec2::new(0.22, -0.2), Vec2::new(0.16, 0.13), 0.035, "by the cord"),
    ],
    tool: (Vec3::new(0.21, 0.0, 0.1), Vec3::new(0.49, 0.22, 0.36)),
    made: (Vec2::new(-0.22, -0.33), Vec2::new(0.26, 0.08), 0.035),
    views: &[view("the mat", Vec3::new(0.0, 0.035, 0.04), Vec3::new(0.0, 1.05, 0.6))],
    work: ("Strike with the hammerstone", "blow"),
    tool_name: "hammerstone",
    sound: "dig_hard",
    scale: 1.0,
    pot: Vec2::ZERO,
    flip: false,
};

/// Where the cooking pot's mouth is, in its own frame (see `models`).
const POT: Vec2 = Vec2::new(0.06, 0.0);
/// The broth's surface, where things float.
const BROTH: f32 = 0.48;

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
    views: &[view("the pot", Vec3::new(POT.x, BROTH, 0.04), Vec3::new(0.05, 0.8, 0.42)), view("the fire", Vec3::new(-0.42, 0.035, 0.0), Vec3::new(0.1, 0.85, 0.42))],
    work: ("Stir with the ladle", "stir"),
    tool_name: "ladle",
    sound: "stir",
    scale: 0.75,
    pot: POT,
    flip: false,
};

/// The cutting board: what you cut lies on the board, the knife along the
/// back, and the cut pieces go into the dish.
const CUTTING_BOARD: Layout = Layout {
    slots: &[("board", Vec2::new(-0.14, -0.02), Vec2::new(0.24, 0.2), 0.48, "on the board")],
    tool: (Vec3::new(0.02, 0.46, 0.15), Vec3::new(0.42, 0.53, 0.24)),
    made: (Vec2::new(0.26, -0.08), Vec2::new(0.13, 0.13), 0.512),
    views: &[view("the board", Vec3::new(0.0, 0.512, 0.04), Vec3::new(0.0, 0.8, 0.55))],
    work: ("Cut with the knife", "cut"),
    tool_name: "knife",
    sound: "knife",
    scale: 0.85,
    pot: Vec2::ZERO,
    flip: false,
};

/// The smoking rack: meat lies on the grate, fuel goes into the fire's mouth
/// at the end, and the reed fan on the ground works the smoke.
const SMOKING_RACK: Layout = Layout {
    slots: &[
        ("rack", Vec2::new(-0.05, 0.0), Vec2::new(0.42, 0.17), 1.02, "on the rack"),
        ("fire", Vec2::new(0.65, 0.0), Vec2::new(0.15, 0.15), 0.035, "in the fire"),
    ],
    tool: (Vec3::new(0.38, 0.0, 0.3), Vec3::new(0.8, 0.08, 0.54)),
    made: (Vec2::new(-0.05, 0.0), Vec2::new(0.42, 0.17), 1.02),
    views: &[view("the rack", Vec3::new(0.0, 1.02, 0.04), Vec3::new(0.0, 0.8, 0.7)), view("the fire", Vec3::new(0.62, 0.035, 0.15), Vec3::new(0.0, 0.85, 0.45))],
    work: ("Fan the smoke", "wave of the fan"),
    tool_name: "fan",
    sound: "swing",
    scale: 1.0,
    pot: Vec2::ZERO,
    flip: false,
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
    views: &[view("the anvil", Vec3::new(0.0, 0.78, 0.04), Vec3::new(0.0, 1.0, 0.65))],
    work: ("Strike with the hammer", "blow of the hammer"),
    tool_name: "hammer",
    sound: "dig_hard",
    scale: 1.0,
    pot: Vec2::ZERO,
    flip: false,
};

/// The sharpening stone: a tool on the rest in front of the wheel; you turn
/// the wheel, and it comes back sharp where it lay.
const SHARPENING_STONE: Layout = Layout {
    slots: &[("edge", Vec2::new(0.2, 0.13), Vec2::new(0.19, 0.11), 0.53, "on the rest")],
    tool: (Vec3::new(-0.13, 0.52, -0.29), Vec3::new(0.03, 1.04, 0.25)),
    made: (Vec2::new(0.2, 0.13), Vec2::new(0.19, 0.11), 0.53),
    views: &[view("the wheel", Vec3::new(0.0, 0.53, 0.04), Vec3::new(0.0, 0.8, 0.62))],
    work: ("Turn the wheel", "turn of the wheel"),
    tool_name: "wheel",
    sound: "dig_hard",
    scale: 0.8,
    pot: Vec2::ZERO,
    flip: true,
};

/// The magical lens: something on the plate under the crystal ring, and the
/// focusing dial beside the plate.
const MAGICAL_LENS: Layout = Layout {
    slots: &[("plate", Vec2::new(0.0, 0.0), Vec2::new(0.2, 0.2), 0.095, "on the plate")],
    tool: (Vec3::new(-0.37, 0.08, -0.07), Vec3::new(-0.23, 0.26, 0.07)),
    made: (Vec2::new(0.0, 0.0), Vec2::new(0.2, 0.2), 0.095),
    views: &[view("the plate", Vec3::new(0.0, 0.095, 0.04), Vec3::new(0.0, 0.95, 0.55))],
    work: ("Turn the focusing dial", "turn of the dial"),
    tool_name: "focusing dial",
    sound: "spell_off",
    scale: 1.0,
    pot: Vec2::ZERO,
    flip: false,
};

/// Where the cauldron's mouth is, and the brew's surface.
const CAULDRON_AT: Vec2 = Vec2::new(0.08, 0.0);
const BREW: f32 = 0.56;

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
    views: &[view("the cauldron", Vec3::new(CAULDRON_AT.x, BREW, 0.04), Vec3::new(0.05, 0.85, 0.42)), view("the fire", Vec3::new(-0.42, 0.035, 0.0), Vec3::new(0.1, 0.85, 0.42))],
    work: ("Stir with the paddle", "stir"),
    tool_name: "paddle",
    sound: "stir",
    scale: 0.75,
    pot: CAULDRON_AT,
    flip: false,
};

pub(super) fn layout(kind: StationKind) -> &'static Layout {
    match kind.base() {
        StationKind::CampMat => &CAMP_MAT,
        StationKind::CarpentryBench => &CARPENTRY_BENCH,
        StationKind::CarpentryTable => &CARPENTRY_TABLE,
        StationKind::MachinationTable => &MACHINATION_TABLE,
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

/// One way of looking at a bench: the point looked at and where the eye is
/// from it, in the bench's frame (z is turned to the side you stand on).
pub(super) struct View {
    name: &'static str,
    at: Vec3,
    eye: Vec3,
}

const fn view(name: &'static str, at: Vec3, eye: Vec3) -> View {
    View { name, at, eye }
}

fn rotation(st: &Station) -> Quat {
    if st.turned { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY }
}

fn to_world(st: &Station, local: Vec3) -> Vec3 {
    st.pos + rotation(st) * local
}

/// Looking down at the bench from the side you stand on, from one of its views.
fn board_view(st: &Station, side: f32, i: usize) -> Transform {
    let l = layout(st.kind);
    let v = &l.views[i.min(l.views.len() - 1)];
    let flip = Vec3::new(1.0, 1.0, side);
    let at = v.at * flip;
    let eye = to_world(st, at + v.eye * flip);
    Transform::from_translation(eye).looking_at(to_world(st, at), Vec3::Y)
}

impl AtBench {
    /// The point the current view looks at, in the bench's frame.
    fn focus(&self) -> Option<Vec3> {
        let st = self.station?;
        let l = layout(st.kind);
        Some(l.views[self.view_i.min(l.views.len() - 1)].at * Vec3::new(1.0, 1.0, self.side))
    }
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

/// What was made, lying on the board.
#[derive(Component)]
pub(super) struct MadeModel;

/// Which slot a model on the board is in, and which of its stack.
#[derive(Component)]
pub(super) struct PartOf(pub (String, usize));

#[derive(Component)]
pub(super) struct HeldModel;

/// Something floating in the pot: its place in `AtBench::floaters`, and
/// how far its middle stands above the waterline.
#[derive(Component)]
pub(super) struct Floating {
    key: (String, usize),
    above: f32,
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

/// The arrows at the strip's ends: -1 back a page, +1 on.
#[derive(Component)]
pub(super) struct StripArrow(i32);

/// The row of view buttons ("the pot", "the fire").
#[derive(Component)]
pub(super) struct ViewRow;

#[derive(Component)]
pub(super) struct ViewButton(usize);

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
    // Glide to the chosen view.
    if let Some(st) = at.station {
        let want = board_view(&st, at.side, at.view_i);
        let f = 1.0 - (-dt * 6.0).exp();
        at.view = Some(at.view.map_or(want, |v| Transform::from_translation(v.translation.lerp(want.translation, f)).with_rotation(v.rotation.slerp(want.rotation, f))));
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
    scope: Res<super::telescope::Scope>,
    mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>, Without<BoardUi>, Without<super::telescope::ScopeUi>)>,
    mut hands: Query<&mut Visibility, (With<crate::hands::HandRoot>, Without<Node>)>,
    all: Query<Entity, (With<Node>, Without<ChildOf>, Without<BoardUi>, Without<super::telescope::ScopeUi>)>,
    mut hidden: Local<Vec<(Entity, Visibility)>>,
) {
    // Looking through the telescope puts the HUD away too.
    let away = at.blend > 0.02 || scope.open;
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
    let n = layout(st.kind).views.len();
    let look = keys.just_pressed(KeyCode::KeyS) as i32 - keys.just_pressed(KeyCode::KeyW) as i32;
    if look != 0 && n > 1 {
        at.view_i = (at.view_i as i32 + look).rem_euclid(n as i32) as usize;
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

/// What came of stirring the cooking pot (Messler has something to say about it).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cooked {
    /// It made something.
    Made,
    /// Nothing cooked: no recipe, or the fire too low.
    Failed,
    /// Stirred over a lit fire with nothing in the pot that makes anything.
    Burnt,
}

/// Clicks at the bench: the strip, the slots, the strike stone.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn board_click(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &Transform), With<FlyCamera>>,
    cells: Query<(&StripCell, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
    view_buttons: Query<(&ViewButton, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
    (arrows, scroll): (Query<(&StripArrow, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>, Res<bevy::input::mouse::AccumulatedMouseScroll>),
    (items, benches, player): (Res<Items>, Res<Benches>, Res<Player>),
    mut at: ResMut<AtBench>,
    mut inv: ResMut<Inventory>,
    (mut spill, mut sound, mut toast, mut gain): (MessageWriter<Spill>, MessageWriter<PlaySound>, MessageWriter<Toast>, MessageWriter<crate::progress::GainXp>),
    (prog, mut bursts, mut cooked): (Option<Res<crate::progress::xp::Progression>>, MessageWriter<Burst>, MessageWriter<Cooked>),
    (mut urns, mut edges): (ResMut<Urns>, ResMut<Edges>),
    (recipes, drops, blocks): (Option<Res<crate::inventory::crafting::Recipes>>, Option<Res<crate::inventory::drops::Drops>>, Res<crate::blocks::Blocks>),
) {
    let Some(st) = at.station else { return };
    let Some(name) = board_name(st.kind) else {
        return;
    };
    let feet = player.pos + Vec3::Y;
    let how = models::tool(st.kind).map_or(ToolUse::Strike, |t| t.how);
    // Where the tool meets the work, for the bits it throws.
    let focus = at.focus().unwrap_or(Vec3::Y * layout(st.kind).top());
    let meets = to_world(&st, at.tool.pos.filter(|_| at.tool.held).map_or(focus, |p| Vec3::new(p.x, focus.y + 0.02, p.z)));
    // The tool came down on the bench (or the wheel turned): one go.
    for _ in 0..std::mem::take(&mut at.tool.go) {
        let special = benches.0.matching(name, &at.board).map(|r| r.special);
        let before = at.board.slots.clone();
        let work = at.board.strike(&benches.0, name);
        if !matches!(work, Work::Nothing) {
            bursts.write(Burst::new(meets, burst_of(st.kind), 7));
        }
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
                if st.kind == StationKind::CookingPot && at.board.made.is_none() {
                    let lit = benches.0.heat_of(name, &at.board) > 0;
                    let low = benches.0.needs_heat(name, &at.board).is_some();
                    cooked.write(if lit && !low { Cooked::Burnt } else { Cooked::Failed });
                }
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
                at.dirty = true;
            }
            (Work::Made(s), _) => {
                if st.kind == StationKind::CookingPot {
                    cooked.write(Cooked::Made);
                }
                sound.write(PlaySound::new(layout(st.kind).sound).gain(0.9));
                // The parts come together into it (`forge`), which lays the board out again.
                let used: Vec<(String, usize)> = before
                    .iter()
                    .flat_map(|(slot, was)| {
                        let left = at.board.slots.get(slot).filter(|n| n.item == was.item).map_or(0, |n| n.count.min(8) as usize);
                        (left..was.count.min(8) as usize).map(move |i| (slot.clone(), i))
                    })
                    .collect();
                let has = |n: &str| urns.has(n);
                let (c, _, y) = layout(st.kind).made;
                let to = match models::liquid(st.kind, &has) {
                    Some((mid, _)) => to_world(&st, Vec3::new(c.x, mid.y + 0.04, c.y)),
                    None => to_world(&st, Vec3::new(c.x, y + 0.05, c.y)),
                };
                at.forge = Some(Forge::new(to, used));
                // Keep the parts' models where they are: `forge` moves them, then lays the board out.
                at.dirty = false;
                at.note = Some((format!("You made a {}", items.get(s.item).title), 3.0));
                toast.write(Toast(format!("Made a {}", items.get(s.item).title)));
                if let Some(p) = &prog {
                    gain.write(crate::progress::GainXp { xp: p.xp.craft, why: String::new() });
                }
            }
        }
    }
    if mouse.just_pressed(MouseButton::Right) {
        if at.tool.held {
            at.tool.held = false;
            sound.write(PlaySound::new("dig_wood").gain(0.35));
        } else if let Some(h) = at.held.take() {
            give_back(&mut inv, &items, &mut spill, feet, h);
        }
        return;
    }
    // The wheel turns the strip's pages.
    if scroll.delta.y != 0.0 && at.blend > 0.6 {
        let step = if scroll.delta.y > 0.0 { -1 } else { 1 };
        at.strip_page = (at.strip_page as i32 + step).max(0) as usize;
    }
    // Nothing to pick up or put down while a craft comes together.
    if !mouse.just_pressed(MouseButton::Left) || at.blend < 0.6 || at.forge.is_some() {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let phys = window.physical_cursor_position();
    // The view buttons.
    if let Some(phys) = phys
        && let Some((b, ..)) = view_buttons.iter().find(|(_, n, t, v)| v.get() && n.contains_point(**t, phys))
    {
        at.view_i = b.0;
        return;
    }
    // The strip's arrows.
    if let Some(phys) = phys
        && let Some((a, ..)) = arrows.iter().find(|(_, n, t, v)| v.get() && n.contains_point(**t, phys))
    {
        at.strip_page = (at.strip_page as i32 + a.0).max(0) as usize;
        return;
    }
    // The strip of your things.
    if let Some(phys) = phys
        && let Some((cell, ..)) = cells.iter().find(|(_, n, t, v)| v.get() && n.contains_point(**t, phys))
    {
        // The tool goes back on the bench first.
        at.tool.held = false;
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
    let Some(pos) = window.cursor_position() else {
        return;
    };
    let (cam, tf) = *camera;
    let Ok(ray) = cam.viewport_to_world(&GlobalTransform::from(*tf), pos) else {
        return;
    };
    let (hit, _) = spot(&st, name, at.board.made.is_some(), ray.origin, ray.direction.as_vec3());
    // With the tool in hand, a click works it (or puts it back where it rests).
    if at.tool.held {
        if hit == Some(Spot::Tool) {
            at.tool.held = false;
            sound.write(PlaySound::new("dig_wood").gain(0.35));
        } else if matches!(how, ToolUse::Strike | ToolUse::Cut | ToolUse::Fan) && at.tool.swing.is_none() {
            at.tool.swing = Some(0.0);
        }
        return;
    }
    match hit {
        Some(Spot::Tool) if how.lifts() => {
            // Pick the tool up; what you held goes back to your pockets.
            if let Some(h) = at.held.take() {
                give_back(&mut inv, &items, &mut spill, feet, h);
            }
            at.tool.held = true;
            at.tool.armed = false;
            at.tool.swing = None;
            at.tool.gesture = Gesture::default();
            sound.write(PlaySound::new("dig_wood").gain(0.4));
        }
        Some(Spot::Tool) => {
            // A wheel or knob turns where it is.
            at.tool.turn_to += std::f32::consts::FRAC_PI_2;
            at.tool.go += 1;
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

/// What a go at each bench throws about.
fn burst_of(kind: StationKind) -> BurstKind {
    match kind {
        StationKind::SewingTable | StationKind::ArmorStand => BurstKind::Bits,
        StationKind::GlassKiln => BurstKind::Sparks,
        k if k.base() != k => burst_of(k.base()),
        StationKind::IronAnvil | StationKind::SharpeningStone | StationKind::MachinationTable => BurstKind::Sparks,
        StationKind::CarpentryBench | StationKind::CarpentryTable => BurstKind::Sawdust,
        StationKind::CuttingBoard => BurstKind::Bits,
        StationKind::SmokingRack => BurstKind::Smoke,
        StationKind::CookingPot => BurstKind::Splash,
        StationKind::Cauldron => BurstKind::BrewSplash,
        StationKind::MagicalLens => BurstKind::Sparkle,
        _ => BurstKind::Chips,
    }
}

/// How big a thing floats in the pot: never taller than a hand's width.
fn pot_px(size: UVec2, scale: f32) -> f32 {
    px_for(size, scale).min(0.15 / size.max_element() as f32)
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
    urns: Res<Urns>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &Transform), With<FlyCamera>>,
    mut models: Query<(Entity, &mut Transform, &BoardModel, Option<&Floating>), Without<FlyCamera>>,
    mut held_model: Query<(Entity, &mut Transform), (With<HeldModel>, Without<BoardModel>, Without<FlyCamera>)>,
    (mut showing, mut splash, mut bursts): (Local<Option<Stack>>, Local<f32>, MessageWriter<Burst>),
) {
    let Some(art) = art else { return };
    let dt = time.delta_secs().min(0.1);
    at.bump = (at.bump - dt * 3.5).max(0.0);
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
    let has = |s: &str| urns.has(s);
    // The pot's inside, for benches whose things float.
    let liquid = at.station.and_then(|st| models::liquid(st.kind, &has));
    // The board: rebuilt when it changes; things hop after a knock.
    if at.dirty {
        at.dirty = false;
        for (e, ..) in &models {
            commands.entity(e).despawn();
        }
        let mut floating: Vec<Floater> = Vec::new();
        if let Some(st) = at.station {
            let flip = if layout(st.kind).flip { Quat::from_rotation_y(std::f32::consts::PI) } else { Quat::IDENTITY };
            let r = rotation(&st) * flip * flat();
            let l = layout(st.kind);
            let defs = slots_of(board_name(st.kind).unwrap_or_default());
            // Things in the pot float upright, kept where they were.
            let mut float = |commands: &mut Commands, key: (String, usize), item: ItemId, start: Vec2| {
                let (Some((mid, half)), Some(size)) = (liquid, art.size(item)) else { return };
                let px = pot_px(size, scale);
                let (w, h) = (size.x as f32 * px, size.y as f32 * px);
                let r = (w.max(h) * 0.4).clamp(0.03, 0.07);
                let mut f = at.floaters.iter().find(|f| f.key == key).cloned().unwrap_or_else(|| Floater::new(key.clone(), start, r));
                f.r = r;
                let lim = half - r;
                f.p = Vec2::new(f.p.x.clamp(mid.x - lim, mid.x + lim), f.p.y.clamp(mid.z - lim, mid.z + lim));
                floating.push(f);
                if let Some((mesh, mat)) = art.model(item) {
                    let mut e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::from_scale(Vec3::splat(px)), NotShadowCaster, BoardModel(0.0), PartOf(key.clone()), Floating { key: key.clone(), above: h * 0.2 }));
                    if key.0 == "made" {
                        e.insert(MadeModel);
                    }
                }
            };
            for (slot, stack) in &at.board.slots {
                let (c, _, y, _) = l.slot(slot);
                let in_pot = defs.iter().any(|d| d.name == slot && d.kind == SlotKind::Mix);
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
                    if in_pot && liquid.is_some() {
                        float(&mut commands, (slot.clone(), i), stack.item, c + off * 0.6);
                    } else {
                        if let Some(e) = spawn(&mut commands, stack.item, to_world(&st, Vec3::new(c.x + off.x, y, c.y + off.y)), turn * r, true) {
                            commands.entity(e).insert(PartOf((slot.clone(), i)));
                        }
                    }
                }
            }
            if let Some(m) = at.board.made {
                let (c, _, y) = l.made;
                if liquid.is_some() {
                    float(&mut commands, ("made".into(), 0), m.item, c);
                } else {
                    if let Some(e) = spawn(&mut commands, m.item, to_world(&st, Vec3::new(c.x, y, c.y)), r, true) {
                        commands.entity(e).insert(MadeModel);
                    }
                }
            }
        }
        at.floaters = floating;
    } else if at.bump > 0.0 && at.forge.is_none() {
        let hop = (at.bump * std::f32::consts::PI).sin() * 0.03 * at.bump;
        for (_, mut t, base, fl) in &mut models {
            if fl.is_none() {
                t.translation.y = base.0 + hop;
            }
        }
    }
    // The pot: the ladle pushes things about; they bob upright, facing you.
    if let (Some(st), Some((mid, half))) = (at.station, liquid) {
        let paddle = match (at.tool.stirring, at.tool.pos) {
            (true, Some(p)) => {
                let now = Vec2::new(p.x, p.z);
                let vel = at.paddle_was.map_or(Vec2::ZERO, |w| (now - w) / dt.max(1e-3));
                at.paddle_was = Some(now);
                Some(Paddle { at: now, vel, r: 0.05 })
            }
            _ => {
                at.paddle_was = None;
                None
            }
        };
        let tank = Tank { centre: Vec2::new(mid.x, mid.z), half };
        float::step(&mut at.floaters, tank, paddle, dt);
        // The ladle splashes when it's pushed along fast.
        if let Some(pd) = paddle {
            *splash -= dt * pd.vel.length().min(2.0);
            if *splash <= 0.0 {
                *splash = 0.12;
                let kind = if st.kind == StationKind::Cauldron { BurstKind::BrewSplash } else { BurstKind::Splash };
                bursts.write(Burst::new(to_world(&st, Vec3::new(pd.at.x, mid.y + 0.01, pd.at.y)), kind, 2));
            }
        }
        // Parts coming together are moved by `forge`.
        let gathering = at.forge.as_ref().is_some_and(|f| !f.popped);
        let eye = camera.1.translation;
        let turn = rotation(&st);
        let t = time.elapsed_secs();
        for (_, mut tf, _, fl) in models.iter_mut().filter(|_| !gathering) {
            let Some(fl) = fl else { continue };
            let Some((i, f)) = at.floaters.iter().enumerate().find(|(_, f)| f.key == fl.key) else { continue };
            let bob = (t * 2.1 + i as f32 * 1.7).sin() * 0.004;
            let water = to_world(&st, Vec3::new(f.p.x, mid.y + bob, f.p.y));
            // Facing the eye, its top leaning where it was tipped.
            let to_eye = eye - water;
            let face = Quat::from_rotation_y(to_eye.x.atan2(to_eye.z));
            let lean = turn * Vec3::new(f.lean.x, 0.0, f.lean.y);
            let tip = if lean.length() > 1e-4 { Quat::from_axis_angle(Vec3::new(lean.z, 0.0, -lean.x).normalize(), lean.length()) } else { Quat::IDENTITY };
            tf.rotation = tip * face;
            tf.translation = water + tip * Vec3::Y * fl.above;
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
            let y = st.pos.y + at.focus().map_or(layout(st.kind).top(), |f| f.y) + 0.1;
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

/// Where the cursor points, in the bench's frame, where it crosses height `y`.
fn cursor_on(st: &Station, window: &Window, cam: &Camera, tf: &Transform, y: f32) -> Option<Vec3> {
    let ray = cam.viewport_to_world(&GlobalTransform::from(*tf), window.cursor_position()?).ok()?;
    let inv = rotation(st).inverse();
    let (o, d) = (inv * (ray.origin - st.pos), inv * ray.direction.as_vec3());
    (d.y < -1e-4).then(|| o + d * ((y - o.y) / d.y))
}

/// How high over the view's middle the tool hangs while you hold it.
fn lift_of(how: ToolUse) -> f32 {
    match how {
        ToolUse::Strike => 0.16,
        ToolUse::Cut => 0.1,
        ToolUse::Fan => 0.14,
        ToolUse::Saw => 0.07,
        ToolUse::Stir => 0.1,
        ToolUse::Turn(_) => 0.0,
    }
}

/// How the tool is held: hammers with the handle up, the ladle upright.
fn held_turn(how: ToolUse) -> Quat {
    match how {
        ToolUse::Strike => Quat::from_rotation_z(0.5),
        // The blade stands on its edge, the handle a little up.
        ToolUse::Cut => Quat::from_rotation_z(-0.2) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        // Upright, the handle leaning off to the side so you see it.
        ToolUse::Stir => Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(-1.15),
        _ => Quat::IDENTITY,
    }
}

/// Where a swing is at `t` (0..1): how far up from the hanging height
/// (`lift` above the surface: -lift = touching it) and how far the tool is
/// turned back. It meets the surface at `SWING_HIT`.
pub(super) fn swing_pose(how: ToolUse, t: f32, lift: f32) -> (f32, f32) {
    let ease = |x: f32| x * x * (3.0 - 2.0 * x);
    match how {
        ToolUse::Fan => (0.0, (t * std::f32::consts::TAU).sin() * 0.9),
        // The knife: up quick, a moment's hold, then it drops straight down
        // and stops dead on the board before lifting off again.
        ToolUse::Cut => {
            let up = 0.08;
            if t < 0.25 {
                (up * ease(t / 0.25), 0.0)
            } else if t < 0.42 {
                (up, 0.0)
            } else if t < SWING_HIT {
                let k = (t - 0.42) / (SWING_HIT - 0.42);
                (up + (-lift - up) * k, 0.0)
            } else if t < 0.75 {
                (-lift, 0.0)
            } else {
                (-lift * (1.0 - ease((t - 0.75) / 0.25)), 0.0)
            }
        }
        _ => {
            let up = 0.12;
            if t < 0.35 {
                let k = ease(t / 0.35);
                (up * k, 0.6 * k)
            } else if t < SWING_HIT {
                let k = (t - 0.35) / (SWING_HIT - 0.35);
                (up + (-lift - up) * k * k, 0.6 * (1.0 - k))
            } else {
                let k = ease((t - SWING_HIT) / (1.0 - SWING_HIT));
                (-lift * (1.0 - k) + (1.0 - k) * k * 0.05, 0.0)
            }
        }
    }
}

/// The point of a swing where the tool lands (one go).
pub(super) const SWING_HIT: f32 = 0.5;
/// Seconds a swing takes.
const SWING_SECONDS: f32 = 0.42;

/// The bench's tool: it rests on the bench, follows the cursor while you
/// hold it, swings down when you click, stirs and saws while the button is
/// held, and wheels and knobs turn in place.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn board_tool(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &Transform), With<FlyCamera>>,
    mut at: ResMut<AtBench>,
    stations: Query<&Station>,
    mut tools: Query<(&BenchTool, &ChildOf, &mut Transform), Without<FlyCamera>>,
    mut sound: MessageWriter<PlaySound>,
) {
    let dt = time.delta_secs().min(0.1);
    let f = |rate: f32| 1.0 - (-dt * rate).exp();
    let here = at.station;
    // Turning wheels and knobs ease round.
    let t = &mut at.tool;
    t.turn += (t.turn_to - t.turn) * f(8.0);
    for (tool, parent, mut tf) in &mut tools {
        let Ok(st) = stations.get(parent.parent()) else {
            continue;
        };
        let mine = here.is_some_and(|h| h.kind == st.kind && h.pos.distance(st.pos) < 0.01);
        let (pos, rot) = match tool.how {
            ToolUse::Turn(axis) if mine => (tool.rest, Quat::from_axis_angle(axis, at.tool.turn)),
            _ if !mine || !at.tool.held => {
                // Back where it rests.
                let p = tf.translation.lerp(tool.rest, f(14.0));
                (p, tf.rotation.slerp(tool.rest_turn, f(14.0)))
            }
            how => {
                let Some(focus) = at.focus() else { continue };
                let lift = lift_of(how);
                let (cam, ctf) = *camera;
                let pressed = (mouse.pressed(MouseButton::Left) && at.tool.armed && at.blend > 0.6) || at.tool.down;
                if !mouse.pressed(MouseButton::Left) {
                    at.tool.armed = true;
                }
                // Stirring and sawing press the tool down into the work.
                let low = match how {
                    ToolUse::Stir if pressed => focus.y - 0.07,
                    ToolUse::Saw if pressed => focus.y + 0.015,
                    _ => focus.y + lift,
                };
                let mut target = cursor_on(st, &window, cam, ctf, low).unwrap_or(focus + Vec3::Y * lift);
                target.y = low;
                // The ladle stays inside the pot while it stirs.
                let pot = layout(st.kind).pot;
                if how == ToolUse::Stir && pressed {
                    let r = models::liquid(st.kind, &|_| false).map_or(0.18, |(_, w)| w - 0.06);
                    let off = Vec2::new(target.x - pot.x, target.z - pot.y);
                    let off = if off.length() > r { off.normalize() * r } else { off };
                    target = Vec3::new(pot.x + off.x, low, pot.y + off.y);
                }
                let prev = at.tool.pos.unwrap_or(tf.translation);
                let mut p = prev.lerp(target, f(18.0));
                let mut turn = held_turn(how);
                // The swing: up, down onto the bench (one go), and back.
                if let Some(s) = at.tool.freeze.or(at.tool.swing) {
                    // Down until its underside meets the work.
                    let under = match how {
                        ToolUse::Strike => 0.06,
                        // Half the blade's height: its edge meets the board.
                        ToolUse::Cut => 0.03,
                        _ => 0.01,
                    };
                    let (dy, back) = swing_pose(how, s, lift - under);
                    p.y += dy;
                    turn = match how {
                        ToolUse::Fan => Quat::from_rotation_x(back) * turn,
                        ToolUse::Cut => Quat::from_rotation_z(-back) * turn,
                        _ => Quat::from_rotation_z(back) * turn,
                    };
                    if at.tool.freeze.is_none() {
                        let next = s + dt / SWING_SECONDS;
                        if s < SWING_HIT && next >= SWING_HIT {
                            at.tool.go += 1;
                            if how == ToolUse::Fan {
                                sound.write(PlaySound::new("swing").gain(0.5));
                            }
                        }
                        at.tool.swing = (next < 1.0).then_some(next);
                    }
                }
                at.tool.stirring = how == ToolUse::Stir && pressed;
                // Stirring and sawing count while the button is held.
                if pressed && at.tool.freeze.is_none() && !at.tool.down {
                    match how {
                        ToolUse::Stir => {
                            let (n, _) = at.tool.gesture.stir((p.z - pot.y).atan2(p.x - pot.x));
                            at.tool.go += n;
                        }
                        ToolUse::Saw => {
                            let n = at.tool.gesture.saw(p.x);
                            at.tool.go += n;
                        }
                        _ => {}
                    }
                } else {
                    at.tool.gesture.lift();
                }
                // Leaning the way it's dragged.
                let lean = (target - p) * 4.0;
                if how != ToolUse::Stir || !pressed {
                    turn = Quat::from_rotation_z(-lean.x.clamp(-0.4, 0.4)) * Quat::from_rotation_x(lean.z.clamp(-0.4, 0.4)) * turn;
                }
                if how == ToolUse::Stir && pressed {
                    p = target.lerp(p, 0.3);
                }
                at.tool.pos = Some(p);
                (p, turn)
            }
        };
        tf.translation = pos;
        tf.rotation = rot;
    }
    if !at.tool.held {
        at.tool.pos = None;
        at.tool.swing = None;
        at.tool.stirring = false;
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
                    c.spawn((ViewRow, Node { column_gap: Val::Px(6.0), margin: UiRect::top(Val::Px(4.0)), align_items: AlignItems::Center, ..default() }));
                });
            root.spawn(Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: Val::Px(6.0), ..default() }).with_children(|c| {
                c.spawn((Strip, Node { column_gap: Val::Px(2.0), padding: UiRect::all(Val::Px(6.0)), ..default() }, theme.skin("panel_light")));
                c.spawn((
                    crate::theme::text("Click: pick up / set down   ·   Click the tool to lift it   ·   Shift: whole stack   ·   Right click: put back   ·   A / D: next bench   ·   Esc: leave", sizes::SMALL, colors::FAINT),
                    Node::default(),
                ));
            });
        });
}

const CELL: f32 = 46.0;
/// Kinds of things on one page of the strip.
const STRIP_PAGE: usize = 16;

/// Your things for the strip, one entry per kind: those this bench uses
/// (in its recipes, its fuel, any tool at the grindstone, anything under the
/// lens) first, then the rest, each in pocket order.
fn strip_list(inv: &Inventory, items: &Items, benches: &BenchData, kind: StationKind, name: &str) -> Vec<(ItemId, u32)> {
    let mut list: Vec<(ItemId, u32)> = Vec::new();
    for p in &inv.pockets.items {
        if !list.iter().any(|(i, _)| *i == p.stack.item) {
            list.push((p.stack.item, inv.count(p.stack.item)));
        }
    }
    let fire = slots_of(name).iter().any(|s| s.kind == SlotKind::Fire);
    let uses = |item: ItemId| {
        kind == StationKind::MagicalLens
            || (kind == StationKind::SharpeningStone && matches!(items.get(item).kind, crate::items::ItemKind::Tool { .. }))
            || (fire && benches.is_fuel(item))
            || benches.recipes.iter().filter(|r| r.station == name && benches.knows(r)).any(|r| r.slots.values().chain(&r.mix).any(|s| s.item == item))
    };
    // A stable sort keeps pocket order inside each group.
    list.sort_by_key(|(i, _)| !uses(*i));
    list
}

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
    mut cells: Query<(&StripCell, &ComputedNode, &UiGlobalTransform, &mut ImageNode), Without<ViewButton>>,
    (view_row, mut view_buttons): (Query<(Entity, Option<&Children>), With<ViewRow>>, Query<(&ViewButton, &ComputedNode, &UiGlobalTransform, &mut ImageNode)>),
    mut views_built: Local<Option<StationKind>>,
    mut texts: ParamSet<(Query<&mut Text, With<HintTitle>>, Query<&mut Text, With<HintLine>>)>,
    mut built: Local<Option<(Vec<(ItemId, u32)>, usize, usize)>>,
) {
    let Ok(mut vis) = root.single_mut() else {
        return;
    };
    let Some(st) = at.station else {
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
        *built = None;
        *views_built = None;
        return;
    };
    // The view buttons, for benches with more than one view.
    if *views_built != Some(st.kind)
        && let Ok((row, kids)) = view_row.single()
    {
        for k in kids.into_iter().flatten() {
            commands.entity(*k).despawn();
        }
        let views = layout(st.kind).views;
        if views.len() > 1 {
            commands.entity(row).with_children(|r| {
                r.spawn((crate::theme::text("Look at", sizes::SMALL, colors::MUTED), Node::default()));
                for (i, v) in views.iter().enumerate() {
                    r.spawn((ViewButton(i), Node { padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)), ..default() }, theme.skin("button"))).with_children(|b| {
                        b.spawn((crate::theme::text(v.name, sizes::SMALL, colors::TEXT), Node::default()));
                    });
                }
                r.spawn((crate::theme::text("(W / S)", sizes::SMALL, colors::FAINT), Node::default()));
            });
        }
        *views_built = Some(st.kind);
    }
    let phys = window.physical_cursor_position();
    for (b, node, tf, mut img) in &mut view_buttons {
        let want = if b.0 == at.view_i {
            "button_down"
        } else if phys.is_some_and(|p| node.contains_point(*tf, p)) {
            "button_hover"
        } else {
            "button"
        };
        let skin = theme.image(want);
        if img.rect != skin.rect {
            *img = skin;
        }
    }
    if *vis != Visibility::Inherited {
        *vis = Visibility::Inherited;
    }
    let Some(art) = art else { return };
    // Your things, one cell per kind: what this bench can use first, then
    // the rest in pocket order, a page at a time.
    let name = board_name(st.kind).unwrap_or("");
    let list = strip_list(&inv, &items, &benches.0, st.kind, name);
    let pages = list.len().div_ceil(STRIP_PAGE).max(1);
    at.strip_page = at.strip_page.min(pages - 1);
    let page = at.strip_page;
    let key = (list.clone(), page, pages);
    if built.as_ref() != Some(&key)
        && let Ok((strip, kids)) = strip.single()
    {
        let list: Vec<(ItemId, u32)> = list.into_iter().skip(page * STRIP_PAGE).take(STRIP_PAGE).collect();
        for k in kids.into_iter().flatten() {
            commands.entity(*k).despawn();
        }
        commands.entity(strip).with_children(|row| {
            if list.is_empty() {
                row.spawn((crate::theme::text("Your pockets are empty", sizes::SMALL, colors::MUTED), Node { margin: UiRect::all(Val::Px(8.0)), ..default() }));
            }
            if pages > 1 {
                row.spawn((StripArrow(-1), Node { width: Val::Px(26.0), height: Val::Px(CELL), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, theme.skin("button")))
                    .with_children(|b| {
                        b.spawn(crate::theme::text("<", sizes::HEADING, colors::TEXT));
                    });
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
            if pages > 1 {
                row.spawn((StripArrow(1), Node { width: Val::Px(26.0), height: Val::Px(CELL), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, theme.skin("button")))
                    .with_children(|b| {
                        b.spawn(crate::theme::text(">", sizes::HEADING, colors::TEXT));
                    });
                row.spawn((crate::theme::text(format!("{} / {pages}", page + 1), sizes::SMALL, colors::MUTED), Node { margin: UiRect::left(Val::Px(6.0)), align_self: AlignSelf::Center, ..default() }));
            }
        });
        *built = Some(key);
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
        let l = layout(st.kind);
        let how = models::tool(st.kind).map_or(ToolUse::Strike, |t| t.how);
        let count = format!("({} / {})", at.board.work, r.work);
        match how {
            _ if at.tool.held => {
                let doing = match how {
                    ToolUse::Stir => "Hold the button and stir round and round",
                    ToolUse::Saw => "Hold the button and saw back and forth",
                    ToolUse::Fan => "Click to fan the smoke",
                    ToolUse::Cut => "Click to cut",
                    _ => "Click to bring it down",
                };
                format!("{doing} to {what}  {count}")
            }
            ToolUse::Turn(_) => format!("{}: click it to {what}  {count}", l.work.0),
            _ => format!("{} to {what}: click the {} to pick it up  {count}", l.work.0, l.tool_name),
        }
    } else if let Some((r, have)) = benches.0.needs_heat(name, &at.board) {
        format!("Put fuel in the fire to make {}  ({have} of {} heat)", a_or_some(&items, r.makes), r.heat)
    } else if at.tool.held {
        format!("You hold the {}: put things {} first (right click puts it back)", layout(st.kind).tool_name, layout(st.kind).places())
    } else if let Some(h) = at.held {
        let l = layout(st.kind);
        let fire = l.views.iter().position(|v| v.name == "the fire");
        if matches!(st.kind, StationKind::CookingPot | StationKind::Cauldron) && !benches.0.is_fuel(h.item) {
            format!("Put it {}", l.slots[0].4)
        } else if benches.0.is_fuel(h.item)
            && let Some(i) = fire
            && i != at.view_i
        {
            "It burns: look at the fire (W / S) and put it in".to_string()
        } else {
            format!("Set it down {}", layout(st.kind).places())
        }
    } else {
        "Pick something from your pockets below".to_string()
    };
    for (q, s) in [(0, st.kind.title().to_string()), (1, line)] {
        let set = |t: &mut Text| {
            if t.0 != s {
                t.0 = s.clone();
            }
        };
        match q {
            0 => texts.p0().iter_mut().for_each(|mut t| set(&mut t)),
            _ => texts.p1().iter_mut().for_each(|mut t| set(&mut t)),
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
pub(super) fn a_or_some(items: &Items, s: Stack) -> String {
    let title = &items.get(s.item).title;
    if s.count == 1 { format!("a {title}") } else { format!("{} {}", s.count, plural(title)) }
}

pub(super) fn plural(s: &str) -> String {
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
    benches: Res<Benches>,
    player: Res<Player>,
    mut at: ResMut<AtBench>,
    mut menus: ResMut<Menus>,
    (mut done, mut forge_in): (Local<bool>, Local<Option<u32>>),
) {
    // `BLOCKGAME_FORGE`: work the board once its models are laid out.
    if let Some(n) = forge_in.as_mut() {
        *n = n.saturating_sub(1);
        if *n == 0 {
            *forge_in = None;
            let st = at.station;
            at.tool.go = st.and_then(|s| board_name(s.kind)).and_then(|n| benches.0.matching(n, &at.board)).map_or(0, |r| r.work as u32);
        }
    }
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
    let Some(st) = stations.iter().find(|s| s.kind == kind) else {
        return;
    };
    *done = true;
    at.enter(*st, player.pos + Vec3::Y * player.eye);
    menus.open = true;
    menus.still = true;
    // `BLOCKGAME_BOARDVIEW=1` looks from the second view (the fire);
    // `BLOCKGAME_BOARDTOOL=lift` holds the tool, `=down` presses it into the
    // work (stirring, sawing), a number freezes a swing that far through.
    at.view_i = std::env::var("BLOCKGAME_BOARDVIEW").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    if let Ok(t) = std::env::var("BLOCKGAME_BOARDTOOL") {
        at.tool.held = true;
        at.tool.down = t == "down";
        at.tool.freeze = t.parse().ok();
    }
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
    // `BLOCKGAME_FORGE=0.8` works the board until it makes something, and
    // holds the parts coming together that many seconds in.
    if std::env::var("BLOCKGAME_FORGE").is_ok() {
        *forge_in = Some(10);
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
                let v = board_view(&st, side, 0);
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
        assert_eq!(name(spot(&st, "tool_bench", false, from(0.52, 0.0), down).0), "stone");
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
        assert_eq!(at(StationKind::SmokingRack, 0.66, 0.42, false), "stone");
        assert_eq!(at(StationKind::IronAnvil, 0.1, 0.0, false), "anvil");
        assert_eq!(at(StationKind::IronAnvil, -0.55, 0.17, false), "flux");
        assert_eq!(at(StationKind::IronAnvil, -0.5, -0.03, false), "handle");
        assert_eq!(at(StationKind::IronAnvil, -0.65, -0.19, false), "stone");
        assert_eq!(at(StationKind::SharpeningStone, 0.22, 0.14, false), "edge");
        assert_eq!(at(StationKind::SharpeningStone, -0.05, 0.0, false), "stone");
        assert_eq!(at(StationKind::MagicalLens, 0.05, 0.05, false), "plate");
        assert_eq!(at(StationKind::MagicalLens, -0.3, 0.0, false), "stone");
        assert!(at(StationKind::Cauldron, 0.1, 0.1, false).starts_with("brew_"));
        assert_eq!(at(StationKind::Cauldron, -0.47, 0.0, false), "fire");
        assert_eq!(at(StationKind::Cauldron, 0.62, 0.0, false), "stone");
        assert_eq!(at(StationKind::Cauldron, 0.1, 0.1, true), "made");
    }

    #[test]
    fn a_full_turn_round_the_pot_is_one_stir() {
        let mut g = Gesture::default();
        let mut stirs = 0;
        for i in 0..=40 {
            stirs += g.stir(i as f32 / 40.0 * std::f32::consts::TAU).0;
        }
        assert_eq!(stirs, 1);
        // Back and forth doesn't stir.
        let mut g = Gesture::default();
        let wobble: u32 = (0..200).map(|i| g.stir(((i as f32) * 0.5).sin()).0).sum();
        assert_eq!(wobble, 0);
        // The other way round counts too.
        let mut g = Gesture::default();
        let back: u32 = (0..=80).map(|i| g.stir(-(i as f32) / 40.0 * std::f32::consts::TAU).0).sum();
        assert_eq!(back, 2);
    }

    #[test]
    fn sawing_counts_strokes_while_the_button_is_held() {
        let mut g = Gesture::default();
        let mut n = 0;
        for i in 0..=20 {
            n += g.saw(i as f32 * 0.031);
        }
        assert_eq!(n, 2, "0.6 of travel");
        // Letting go and pressing again elsewhere isn't a stroke.
        g.lift();
        assert_eq!(g.saw(-5.0), 0);
    }

    #[test]
    fn a_swing_goes_up_lands_on_the_work_and_comes_back() {
        for how in [ToolUse::Strike, ToolUse::Cut] {
            let lift = 0.12;
            assert!(swing_pose(how, 0.3, lift).0 > 0.05, "{how:?} up");
            assert!((swing_pose(how, SWING_HIT, lift).0 + lift).abs() < 1e-4, "{how:?} lands");
            assert!(swing_pose(how, 1.0, lift).0.abs() < 1e-4, "{how:?} back");
        }
    }

    #[test]
    fn every_bench_has_its_tool_and_a_view() {
        for kind in [
            StationKind::CampMat,
            StationKind::ToolBench,
            StationKind::CarpentryBench,
            StationKind::CarpentryTable,
            StationKind::MachinationTable,
            StationKind::CookingPot,
            StationKind::CuttingBoard,
            StationKind::SmokingRack,
            StationKind::IronAnvil,
            StationKind::SharpeningStone,
            StationKind::MagicalLens,
            StationKind::Cauldron,
            StationKind::JoinersTable,
            StationKind::DoorFrame,
            StationKind::GlassKiln,
            StationKind::SewingTable,
            StationKind::ArmorStand,
            StationKind::BladesmithBench,
            StationKind::GunsmithBench,
        ] {
            let t = models::tool(kind).unwrap();
            let l = layout(kind);
            assert!(!l.views.is_empty());
            // The tool lies where a click picks it up.
            let (lo, hi) = l.tool;
            assert!(t.rest.cmpge(lo - 0.02).all() && t.rest.cmple(hi + 0.02).all(), "{kind:?}: {} not in {lo} {hi}", t.rest);
        }
    }

    #[test]
    fn every_slot_has_a_place_on_its_bench() {
        for kind in [
            StationKind::CampMat,
            StationKind::ToolBench,
            StationKind::CarpentryBench,
            StationKind::CarpentryTable,
            StationKind::MachinationTable,
            StationKind::CookingPot,
            StationKind::CuttingBoard,
            StationKind::SmokingRack,
            StationKind::IronAnvil,
            StationKind::SharpeningStone,
            StationKind::MagicalLens,
            StationKind::Cauldron,
            StationKind::JoinersTable,
            StationKind::DoorFrame,
            StationKind::GlassKiln,
            StationKind::SewingTable,
            StationKind::ArmorStand,
            StationKind::BladesmithBench,
            StationKind::GunsmithBench,
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
