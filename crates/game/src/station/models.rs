//! What each station looks like: a few boxes each, in the station's own frame
//! (x along its length, z across, y up from its foot). Upgrades add parts or
//! change materials. Placeholder shapes until the stations are modelled;
//! `docs/stations/` has the concept sheets they should grow into.

use bevy::prelude::*;

use super::StationKind;

/// Materials, coloured in `mod.rs` (`Mat::look`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mat {
    Wood,
    WoodDark,
    Groove,
    Stone,
    StoneDark,
    Iron,
    Steel,
    Brass,
    Reed,
    Cloth,
    Linen,
    Cord,
    Hide,
    Moss,
    Clay,
    /// What simmers in the cooking pot.
    Broth,
    /// The urn's own clay, its darker foot, and its marks.
    Urn,
    UrnDark,
    MarkLit,
    MarkUnlit,
    /// Glowing things.
    Fire,
    Brew,
    Spirit,
    Crystal,
}

impl Mat {
    /// Colour and glow (emissive strength), given the urn's colour.
    pub fn look(self, urn: (f32, f32, f32)) -> (Color, f32) {
        let c = |r, g, b| Color::srgb(r, g, b);
        match self {
            Mat::Wood => (c(0.5, 0.36, 0.22), 0.0),
            Mat::WoodDark => (c(0.34, 0.24, 0.15), 0.0),
            Mat::Groove => (c(0.22, 0.16, 0.1), 0.0),
            Mat::Stone => (c(0.5, 0.5, 0.52), 0.0),
            Mat::StoneDark => (c(0.36, 0.36, 0.38), 0.0),
            Mat::Iron => (c(0.3, 0.3, 0.32), 0.0),
            Mat::Steel => (c(0.58, 0.62, 0.68), 0.0),
            Mat::Brass => (c(0.72, 0.56, 0.26), 0.0),
            Mat::Reed => (c(0.64, 0.6, 0.36), 0.0),
            Mat::Cloth => (c(0.33, 0.4, 0.22), 0.0),
            Mat::Linen => (c(0.78, 0.72, 0.58), 0.0),
            Mat::Cord => (c(0.72, 0.63, 0.45), 0.0),
            Mat::Hide => (c(0.55, 0.35, 0.21), 0.0),
            Mat::Moss => (c(0.3, 0.45, 0.2), 0.0),
            Mat::Clay => (c(0.62, 0.38, 0.26), 0.0),
            Mat::Broth => (c(0.62, 0.42, 0.16), 0.15),
            Mat::Urn => (c(urn.0, urn.1, urn.2), 0.0),
            Mat::UrnDark => (c(urn.0 * 0.72, urn.1 * 0.7, urn.2 * 0.7), 0.0),
            Mat::MarkLit => (c((urn.0 + 0.6).min(1.0), (urn.1 + 0.55).min(1.0), (urn.2 + 0.4).min(1.0)), 3.0),
            Mat::MarkUnlit => (c(0.25, 0.2, 0.16), 0.0),
            Mat::Fire => (c(1.0, 0.55, 0.18), 4.0),
            Mat::Brew => (c(0.35, 0.9, 0.45), 1.2),
            Mat::Spirit => (c(0.5, 0.8, 1.0), 3.0),
            Mat::Crystal => (c(0.75, 0.95, 1.0), 2.0),
        }
    }
}

impl Mat {
    /// See-through liquids: how opaque they are.
    pub fn alpha(self) -> Option<f32> {
        match self {
            Mat::Broth => Some(0.62),
            Mat::Brew => Some(0.5),
            _ => None,
        }
    }
}

/// One box of a model.
#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub size: Vec3,
    pub at: Vec3,
    pub mat: Mat,
    /// Turned about z (a leaning leg, a tilted tube), radians.
    pub tilt: f32,
}

fn b(sx: f32, sy: f32, sz: f32, x: f32, y: f32, z: f32, mat: Mat) -> Part {
    Part { size: Vec3::new(sx, sy, sz), at: Vec3::new(x, y, z), mat, tilt: 0.0 }
}

fn tilted(p: Part, tilt: f32) -> Part {
    Part { tilt, ..p }
}

/// Four legs under a top of this size.
fn legs(v: &mut Vec<Part>, w: f32, d: f32, h: f32, t: f32, mat: Mat) {
    for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        v.push(b(t, h, t, sx * (w / 2.0 - t), h / 2.0, sz * (d / 2.0 - t), mat));
    }
}

/// The golem's stone body: asleep on its frame, or awake (glowing heart and
/// eyes), with iron hands once it has them. Faces `face` along z (+1 or -1).
fn golem_body(v: &mut Vec<Part>, awake: bool, hands: bool, face: f32) {
    use Mat::*;
    v.extend([
        b(0.18, 0.3, 0.2, -0.14, 0.15, 0.0, StoneDark),
        b(0.18, 0.3, 0.2, 0.14, 0.15, 0.0, StoneDark),
        b(0.6, 0.6, 0.4, 0.0, 0.62, 0.0, Stone),
        b(0.36, 0.3, 0.32, 0.0, 1.1, 0.0, Stone),
        b(0.18, 0.5, 0.2, -0.42, 0.62, 0.0, Stone),
        b(0.18, 0.5, 0.2, 0.42, 0.62, 0.0, Stone),
    ]);
    let core = if awake { Spirit } else { MarkUnlit };
    v.push(b(0.14, 0.14, 0.02, 0.0, 0.7, face * 0.205, core));
    v.push(b(0.06, 0.05, 0.02, -0.08, 1.12, face * 0.165, core));
    v.push(b(0.06, 0.05, 0.02, 0.08, 1.12, face * 0.165, core));
    if hands {
        v.push(b(0.22, 0.16, 0.24, -0.42, 0.3, 0.0, Iron));
        v.push(b(0.22, 0.16, 0.24, 0.42, 0.3, 0.0, Iron));
        // The pack on its back.
        v.push(b(0.46, 0.4, 0.18, 0.0, 0.7, -face * 0.29, Hide));
    }
}

/// An open pot standing on `foot` (the middle of its base): a floor, four
/// walls, a lip round the top, and liquid up to `surface`.
#[allow(clippy::too_many_arguments)]
fn hollow(v: &mut Vec<Part>, foot: Vec3, w: f32, h: f32, t: f32, mat: Mat, surface: f32, liquid: Mat) {
    let (x, y, z) = (foot.x, foot.y, foot.z);
    v.push(b(w, t, w, x, y + t / 2.0, z, mat));
    let mid = y + h / 2.0;
    let o = w / 2.0 - t / 2.0;
    v.push(b(w, h, t, x, mid, z - o, mat));
    v.push(b(w, h, t, x, mid, z + o, mat));
    v.push(b(t, h, w - 2.0 * t, x - o, mid, z, mat));
    v.push(b(t, h, w - 2.0 * t, x + o, mid, z, mat));
    let (lw, lt) = (w + 0.06, 0.07);
    let lo = lw / 2.0 - lt / 2.0;
    let top = y + h - 0.02;
    v.push(b(lw, 0.04, lt, x, top, z - lo, mat));
    v.push(b(lw, 0.04, lt, x, top, z + lo, mat));
    v.push(b(lt, 0.04, lw - 2.0 * lt, x - lo, top, z, mat));
    v.push(b(lt, 0.04, lw - 2.0 * lt, x + lo, top, z, mat));
    let inner = w - 2.0 * t - 0.004;
    let floor = y + t;
    // A dark, sooty floor, so the liquid's colour shows.
    v.push(b(inner, 0.004, inner, x, floor + 0.002, z, Mat::Groove));
    v.push(b(inner, surface - floor, inner, x, (surface + floor) / 2.0, z, liquid));
}

/// The cooking pot's broth surface, in its own frame.
pub const POT_SURFACE: f32 = 0.5;

/// The cauldron's brew surface (the steel one is bigger).
pub fn cauldron_surface(has: &dyn Fn(&str) -> bool) -> f32 {
    let s = if has("steel_cauldron") { 1.15 } else { 1.0 };
    0.2 + 0.45 * s - 0.07
}

/// The liquid in a station's pot: the middle of its surface and its half width.
pub fn liquid(kind: StationKind, has: &dyn Fn(&str) -> bool) -> Option<(Vec3, f32)> {
    match kind {
        StationKind::CookingPot => Some((Vec3::new(0.06, POT_SURFACE, 0.0), 0.2)),
        StationKind::Cauldron => {
            let s = if has("steel_cauldron") { 1.15 } else { 1.0 };
            Some((Vec3::new(0.08, cauldron_surface(has), 0.0), 0.31 * s - 0.07))
        }
        _ => None,
    }
}

/// How a bench's tool is worked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolUse {
    /// Lift it and bring it down where you click (hammers, stones).
    Strike,
    /// Lift it and chop down (the knife).
    Cut,
    /// Lift it and wave it (the fan).
    Fan,
    /// Lift it, hold the button and draw it back and forth.
    Saw,
    /// Lift it, hold the button and stir round the pot.
    Stir,
    /// It stays where it is and turns about this axis when clicked (the wheel, the dial).
    Turn(Vec3),
}

impl ToolUse {
    /// Lifted off the bench, or turned in place.
    pub fn lifts(self) -> bool {
        !matches!(self, ToolUse::Turn(_))
    }
}

/// A bench's tool: where it rests (its working end, or what it turns
/// about), its boxes around that point, and how it's worked.
pub struct Tool {
    pub rest: Vec3,
    /// How it lies (or hangs) at rest.
    pub rest_turn: Quat,
    pub parts: Vec<Part>,
    pub how: ToolUse,
}

/// The tool bench's hammer, around the middle of its head: the head stands
/// up and down, the handle runs along +x (as every lifted tool's does).
fn hammer() -> Vec<Part> {
    use Mat::*;
    vec![b(0.07, 0.13, 0.08, 0.0, 0.0, 0.0, Iron), b(0.3, 0.035, 0.035, 0.18, 0.0, 0.0, WoodDark)]
}

/// Where the tool bench's hammer hangs: across two pegs on the rack at the
/// bench's +x end, head toward the front (+z), handle toward the back.
pub const HAMMER_REST: Vec3 = Vec3::new(0.49, 1.07, 0.13);

/// The tool of a bench, in the station's frame.
pub fn tool(kind: StationKind) -> Option<Tool> {
    use Mat::*;
    if kind == StationKind::ToolBench {
        return Some(Tool { rest: HAMMER_REST, rest_turn: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2), parts: hammer(), how: ToolUse::Strike });
    }
    let (rest, parts, how) = match kind {
        // A hammerstone resting on the knapping stone.
        StationKind::CampMat => (Vec3::new(0.35, 0.165, 0.23), vec![b(0.16, 0.09, 0.13, 0.35, 0.175, 0.23, StoneDark)], ToolUse::Strike),
        StationKind::CarpentryBench => {
            (Vec3::new(0.15, 0.852, -0.3), vec![b(0.5, 0.012, 0.11, 0.15, 0.852, -0.3, Stone), b(0.16, 0.06, 0.09, -0.16, 0.87, -0.3, WoodDark)], ToolUse::Saw)
        }
        // A wooden mallet lying on the table by the tray.
        StationKind::CarpentryTable => {
            (Vec3::new(0.6, 0.92, 0.22), vec![b(0.14, 0.12, 0.12, 0.6, 0.92, 0.22, WoodDark), b(0.04, 0.04, 0.24, 0.6, 0.89, 0.06, Wood)], ToolUse::Strike)
        }
        // The drive wheel and its crank handle turn on an axle at the +x end.
        StationKind::MachinationTable => (
            Vec3::new(0.74, 1.16, 0.0),
            vec![
                b(0.06, 0.5, 0.5, 0.74, 1.16, 0.0, Brass),
                b(0.07, 0.36, 0.36, 0.74, 1.16, 0.0, WoodDark),
                b(0.08, 0.5, 0.06, 0.74, 1.16, 0.0, Iron),
                b(0.08, 0.06, 0.5, 0.74, 1.16, 0.0, Iron),
                b(0.18, 0.05, 0.05, 0.8, 1.16, 0.0, Iron),
                b(0.05, 0.05, 0.12, 0.81, 1.36, 0.05, WoodDark),
            ],
            ToolUse::Turn(Vec3::X),
        ),
        // Ladles and paddles: the bowl end is where they're held from.
        StationKind::CookingPot => (Vec3::new(0.6, 0.105, -0.13), vec![b(0.04, 0.03, 0.3, 0.6, 0.095, 0.05, Wood), b(0.11, 0.05, 0.1, 0.6, 0.105, -0.13, WoodDark)], ToolUse::Stir),
        StationKind::Cauldron => {
            (Vec3::new(0.62, 0.095, -0.14), vec![b(0.04, 0.03, 0.3, 0.62, 0.095, 0.05, Wood), b(0.1, 0.03, 0.12, 0.62, 0.095, -0.14, WoodDark)], ToolUse::Stir)
        }
        StationKind::CuttingBoard => {
            (Vec3::new(0.28, 0.4875, 0.19), vec![b(0.12, 0.035, 0.045, 0.1, 0.4975, 0.19, WoodDark), b(0.24, 0.015, 0.06, 0.28, 0.4875, 0.19, StoneDark)], ToolUse::Cut)
        }
        // The reed fan lies by the fire's mouth.
        StationKind::SmokingRack => (Vec3::new(0.66, 0.01, 0.42), vec![b(0.24, 0.02, 0.2, 0.66, 0.01, 0.42, Reed), b(0.18, 0.03, 0.03, 0.48, 0.02, 0.42, Wood)], ToolUse::Fan),
        StationKind::IronAnvil => {
            (Vec3::new(-0.75, 0.565, -0.19), vec![b(0.26, 0.05, 0.05, -0.6, 0.55, -0.19, WoodDark), b(0.08, 0.08, 0.1, -0.75, 0.565, -0.19, Iron)], ToolUse::Strike)
        }
        // The wheel and its crank turn on the axle.
        StationKind::SharpeningStone => (
            Vec3::new(-0.05, 0.78, -0.02),
            vec![
                b(0.14, 0.52, 0.52, -0.05, 0.78, -0.02, Stone),
                b(0.16, 0.36, 0.36, -0.05, 0.78, -0.02, StoneDark),
                b(0.5, 0.05, 0.05, -0.05, 0.78, -0.02, Iron),
                b(0.05, 0.2, 0.05, 0.22, 0.7, -0.02, Iron),
                b(0.05, 0.05, 0.1, 0.22, 0.62, 0.02, WoodDark),
            ],
            ToolUse::Turn(Vec3::X),
        ),
        // The focusing dial sits on a short stand beside the plate, facing up.
        StationKind::MagicalLens => (
            Vec3::new(-0.3, 0.22, 0.0),
            vec![b(0.13, 0.04, 0.13, -0.3, 0.22, 0.0, Brass), b(0.11, 0.03, 0.03, -0.3, 0.255, 0.0, Groove), b(0.03, 0.035, 0.03, -0.3, 0.255, 0.05, Crystal)],
            ToolUse::Turn(Vec3::Y),
        ),
        _ => return None,
    };
    let parts = parts.into_iter().map(|p| Part { at: p.at - rest, ..p }).collect();
    Some(Tool { rest, rest_turn: Quat::IDENTITY, parts, how })
}

/// Marks around the urn's belly.
pub const MARKS: u32 = 10;

/// Half size of each station's box, for aiming (before turning).
pub fn half(kind: StationKind) -> Vec3 {
    match kind {
        StationKind::Urn => Vec3::new(0.32, 0.45, 0.32),
        StationKind::CampMat => Vec3::new(0.6, 0.1, 0.45),
        StationKind::ToolBench => Vec3::new(0.6, 0.45, 0.4),
        StationKind::CarpentryBench => Vec3::new(0.85, 0.45, 0.45),
        StationKind::CarpentryTable => Vec3::new(0.85, 0.5, 0.45),
        StationKind::MachinationTable => Vec3::new(0.9, 0.72, 0.5),
        StationKind::PersonalStorage => Vec3::new(0.5, 0.4, 0.3),
        StationKind::SharpeningStone => Vec3::new(0.45, 0.5, 0.3),
        StationKind::IronAnvil => Vec3::new(0.86, 0.42, 0.3),
        StationKind::CookingPot => Vec3::new(0.72, 0.32, 0.36),
        StationKind::CuttingBoard => Vec3::new(0.43, 0.27, 0.27),
        StationKind::SmokingRack => Vec3::new(0.9, 0.6, 0.3),
        StationKind::StoneFridge => Vec3::new(0.45, 0.45, 0.35),
        StationKind::Telescope => Vec3::new(0.45, 0.65, 0.3),
        StationKind::MagicalLens => Vec3::new(0.36, 0.42, 0.26),
        StationKind::Cauldron => Vec3::new(0.72, 0.36, 0.42),
        StationKind::GolemFrame => Vec3::new(0.6, 0.85, 0.4),
        StationKind::Bedframe => Vec3::new(0.5, 0.15, 1.0),
        StationKind::Golem => Vec3::new(0.45, 0.8, 0.3),
    }
}

/// The parts of a station. `has` says which upgrades are done; `lit` is how
/// many of the urn's marks glow.
pub fn parts(kind: StationKind, has: &dyn Fn(&str) -> bool, lit: u32) -> Vec<Part> {
    use Mat::*;
    let mut v = Vec::new();
    match kind {
        StationKind::Urn => {
            v.extend([
                b(0.42, 0.1, 0.42, 0.0, 0.05, 0.0, UrnDark),
                b(0.56, 0.36, 0.56, 0.0, 0.28, 0.0, Urn),
                b(0.44, 0.16, 0.44, 0.0, 0.54, 0.0, Urn),
                b(0.3, 0.14, 0.3, 0.0, 0.69, 0.0, UrnDark),
                b(0.4, 0.06, 0.4, 0.0, 0.79, 0.0, Urn),
                b(0.2, 0.02, 0.2, 0.0, 0.825, 0.0, Groove),
            ]);
            for i in 0..MARKS {
                let a = i as f32 / MARKS as f32 * std::f32::consts::TAU;
                let (s, c) = a.sin_cos();
                // Pushed out to the nearest face of the square belly.
                let k = 0.29 / s.abs().max(c.abs());
                v.push(b(0.07, 0.07, 0.07, s * k, 0.3, c * k, if i < lit { MarkLit } else { MarkUnlit }));
            }
        }
        StationKind::CampMat => {
            // A woven mat with a cord-bound edge, a coil of cord and a flat knapping stone.
            v.extend([
                b(1.2, 0.03, 0.9, 0.0, 0.015, 0.0, Reed),
                b(1.2, 0.035, 0.05, 0.0, 0.018, 0.43, Cord),
                b(1.2, 0.035, 0.05, 0.0, 0.018, -0.43, Cord),
                b(0.28, 0.12, 0.26, 0.35, 0.07, 0.23, Stone),
                b(0.16, 0.05, 0.16, 0.3, 0.055, -0.15, Cord),
            ]);
        }
        StationKind::ToolBench => {
            legs(&mut v, 1.06, 0.7, 0.72, 0.1, WoodDark);
            v.extend([b(1.2, 0.1, 0.8, 0.0, 0.77, 0.0, Wood), b(0.62, 0.012, 0.14, -0.15, 0.826, -0.12, Groove), b(0.26, 0.012, 0.26, 0.32, 0.826, 0.12, Groove)]);
            // The hammer's rack at the +x end: a board with two pegs (the hammer is the `tool`).
            v.extend([
                b(0.04, 0.34, 0.44, 0.565, 0.99, 0.0, WoodDark),
                b(0.1, 0.03, 0.03, 0.52, 1.03, -0.1, Groove),
                b(0.1, 0.03, 0.03, 0.52, 1.03, 0.08, Groove),
            ]);
            if has("improved_tool_bench") {
                // A tool rack along the back with pegs and a bench vise.
                v.extend([
                    b(0.08, 0.6, 0.08, -0.55, 1.12, -0.36, WoodDark),
                    b(0.08, 0.6, 0.08, 0.55, 1.12, -0.36, WoodDark),
                    b(1.18, 0.08, 0.06, 0.0, 1.32, -0.36, Wood),
                    b(0.05, 0.05, 0.12, -0.3, 1.26, -0.3, Groove),
                    b(0.05, 0.05, 0.12, 0.0, 1.26, -0.3, Groove),
                    b(0.05, 0.05, 0.12, 0.3, 1.26, -0.3, Groove),
                    b(0.16, 0.16, 0.2, -0.5, 0.9, 0.28, StoneDark),
                ]);
            }
        }
        StationKind::CarpentryBench => {
            legs(&mut v, 1.52, 0.76, 0.72, 0.12, WoodDark);
            v.extend([
                b(1.7, 0.12, 0.9, 0.0, 0.78, 0.0, Wood),
                b(0.2, 0.2, 0.3, 0.72, 0.94, 0.0, WoodDark),
                b(0.04, 0.03, 0.32, 0.72, 1.05, 0.0, Groove),
                b(1.0, 0.012, 0.06, -0.2, 0.846, 0.3, Groove),
            ]);
        }
        StationKind::CarpentryTable => {
            // A wide joiner's table: thick top, a stretcher between the legs,
            // a peg tray at +x and a back board with pegs and a square.
            legs(&mut v, 1.6, 0.84, 0.8, 0.12, WoodDark);
            v.extend([
                b(1.7, 0.06, 0.9, 0.0, 0.83, 0.0, Wood),
                b(1.4, 0.06, 0.06, 0.0, 0.2, 0.0, WoodDark),
                b(0.32, 0.04, 0.28, 0.55, 0.875, -0.1, WoodDark),
                b(1.7, 0.32, 0.05, 0.0, 1.02, -0.43, WoodDark),
                b(0.05, 0.05, 0.08, -0.5, 1.08, -0.38, Groove),
                b(0.05, 0.05, 0.08, -0.2, 1.08, -0.38, Groove),
                b(0.22, 0.03, 0.03, 0.25, 1.06, -0.39, Iron),
                b(0.03, 0.14, 0.03, 0.15, 1.0, -0.39, Iron),
            ]);
        }
        StationKind::MachinationTable => {
            // A heavy drafting table: iron-shod legs, a slipway groove down the
            // middle, trays front and back, a gear box at the back edge, a
            // drafting board with a plan pinned up, and the wheel's axle post.
            legs(&mut v, 1.6, 0.9, 0.86, 0.12, WoodDark);
            v.extend([
                b(1.7, 0.06, 1.0, 0.0, 0.86, 0.0, Wood),
                b(1.4, 0.06, 0.06, 0.0, 0.2, 0.0, Iron),
                b(0.76, 0.012, 0.3, -0.32, 0.896, 0.02, Groove),
                b(0.34, 0.04, 0.22, 0.24, 0.905, 0.22, WoodDark),
                b(0.34, 0.04, 0.22, 0.24, 0.905, -0.18, WoodDark),
                b(0.64, 0.05, 0.18, -0.32, 0.905, 0.32, Iron),
                b(0.08, 0.06, 0.08, -0.5, 0.94, 0.32, Brass),
                b(0.1, 0.07, 0.1, -0.36, 0.945, 0.32, Brass),
                b(0.06, 0.05, 0.06, -0.22, 0.94, 0.32, Brass),
                b(1.2, 0.5, 0.05, -0.2, 1.17, -0.48, WoodDark),
                b(0.9, 0.36, 0.01, -0.2, 1.17, -0.45, Linen),
                b(0.5, 0.012, 0.012, -0.25, 1.22, -0.444, Groove),
                b(0.3, 0.1, 0.012, -0.25, 1.32, -0.444, Hide),
                b(0.06, 0.3, 0.06, 0.74, 1.03, 0.0, Iron),
            ]);
        }
        StationKind::PersonalStorage => {
            v.extend([
                b(1.0, 0.45, 0.6, 0.0, 0.225, 0.0, Wood),
                b(1.04, 0.12, 0.64, 0.0, 0.5, 0.0, WoodDark),
                b(0.12, 0.1, 0.04, 0.0, 0.4, 0.32, Brass),
            ]);
            for sx in [-1.0, 1.0] {
                v.push(b(0.06, 0.46, 0.62, sx * 0.48, 0.24, 0.0, Brass));
            }
            if has("storage_shelf") {
                v.extend([
                    b(0.06, 0.5, 0.36, -0.47, 0.81, -0.12, WoodDark),
                    b(0.06, 0.5, 0.36, 0.47, 0.81, -0.12, WoodDark),
                    b(1.0, 0.05, 0.36, 0.0, 0.8, -0.12, Wood),
                    b(1.0, 0.05, 0.36, 0.0, 1.04, -0.12, Wood),
                ]);
            }
        }
        StationKind::SharpeningStone => {
            // A grinding wheel on a table, a crank, a pot of water, and a rest
            // in front of the wheel where the tool lies (`board::layout`).
            legs(&mut v, 0.8, 0.5, 0.45, 0.08, WoodDark);
            v.extend([
                b(0.8, 0.06, 0.5, 0.0, 0.48, 0.0, Wood),
                // Bearings for the wheel's axle (the wheel turns: `tool`).
                b(0.04, 0.3, 0.06, -0.2, 0.66, -0.02, WoodDark),
                b(0.04, 0.3, 0.06, 0.1, 0.66, -0.02, WoodDark),
                b(0.18, 0.12, 0.14, -0.3, 0.57, -0.15, Clay),
                b(0.3, 0.02, 0.18, 0.22, 0.52, 0.14, WoodDark),
            ]);
        }
        StationKind::IronAnvil => {
            // The anvil on its stump (+x), and a low bench (-x) with the
            // hammer, a flux tray and room for a handle.
            let metal = if has("alchemical_anvil") || has("steel_anvil") { Steel } else { Iron };
            v.extend([
                b(0.5, 0.42, 0.46, 0.1, 0.21, 0.0, WoodDark),
                b(0.34, 0.1, 0.26, 0.1, 0.47, 0.0, metal),
                b(0.2, 0.14, 0.16, 0.1, 0.59, 0.0, metal),
                b(0.56, 0.12, 0.28, 0.1, 0.72, 0.0, metal),
                b(0.16, 0.08, 0.12, 0.45, 0.72, 0.0, metal),
            ]);
            for (x, z) in [(-0.8, -0.2), (-0.4, -0.2), (-0.8, 0.2), (-0.4, 0.2)] {
                v.push(b(0.06, 0.5, 0.06, x, 0.25, z, WoodDark));
            }
            v.extend([b(0.5, 0.05, 0.5, -0.6, 0.5, 0.0, Wood), b(0.3, 0.02, 0.14, -0.6, 0.535, 0.16, Clay)]);
            if has("alchemical_anvil") {
                for x in [-0.08, 0.1, 0.28] {
                    v.push(b(0.05, 0.05, 0.01, x, 0.72, 0.145, Spirit));
                    v.push(b(0.05, 0.05, 0.01, x, 0.72, -0.145, Spirit));
                }
            }
        }
        StationKind::CookingPot => {
            // A clay pot sitting on a ring of stones over the fire. The fire runs
            // out from under it into a stone-lined mouth (-x) where the fuel goes
            // (`board::layout`); the ladle rests on a flat stone (+x).
            let (px, r) = (0.06, 0.33);
            v.push(b(0.56, 0.02, 0.5, px, 0.01, 0.0, StoneDark));
            for i in 0..8 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                if i == 4 {
                    continue; // the fire's mouth
                }
                v.push(b(0.14, 0.12, 0.14, px + a.cos() * r, 0.06, a.sin() * r, Stone));
            }
            for x in [-0.36, -0.54] {
                for z in [-0.18, 0.18] {
                    v.push(b(0.15, 0.1, 0.1, x, 0.05, z, Stone));
                }
            }
            v.extend([
                b(0.08, 0.1, 0.28, -0.66, 0.05, 0.0, StoneDark),
                b(0.36, 0.03, 0.24, -0.45, 0.015, 0.0, Fire),
                b(0.2, 0.1, 0.2, px, 0.06, 0.0, Fire),
                b(0.4, 0.04, 0.4, px, 0.13, 0.0, Groove),
                // The ladle's stone (the ladle itself is the `tool`).
                b(0.2, 0.08, 0.34, 0.6, 0.04, 0.0, StoneDark),
            ]);
            // The pot is open: walls round a see-through broth (`Mat::alpha`).
            hollow(&mut v, Vec3::new(px, 0.14, 0.0), 0.54, 0.46, 0.05, Clay, POT_SURFACE, Broth);
        }
        StationKind::CuttingBoard => {
            // A stump with a wide board on it, a flint knife along the back and a
            // clay dish the cut pieces go into.
            v.extend([
                b(0.6, 0.42, 0.42, 0.0, 0.21, 0.0, WoodDark),
                b(0.84, 0.06, 0.52, 0.0, 0.45, 0.0, Wood),
                b(0.24, 0.03, 0.24, 0.26, 0.495, -0.08, Clay),
                b(0.18, 0.01, 0.18, 0.26, 0.511, -0.08, Groove),
            ]);
        }
        StationKind::SmokingRack => {
            // A grate on four posts over a long peat fire whose mouth sticks
            // out at +x; a reed fan lies at -x to work the smoke.
            for (x, z) in [(-0.52, -0.2), (0.52, -0.2), (-0.52, 0.2), (0.52, 0.2)] {
                v.push(b(0.06, 1.0, 0.06, x, 0.5, z, Wood));
            }
            for x in [-0.52, 0.52] {
                v.push(b(0.06, 0.05, 0.46, x, 0.98, 0.0, WoodDark));
            }
            for z in [-0.14, -0.05, 0.05, 0.14] {
                v.push(b(1.04, 0.025, 0.025, 0.0, 1.0, z, Wood));
            }
            v.extend([
                b(0.9, 0.1, 0.42, 0.05, 0.05, 0.0, StoneDark),
                b(0.7, 0.03, 0.3, 0.1, 0.11, 0.0, Fire),
                b(0.3, 0.1, 0.06, 0.65, 0.05, -0.17, StoneDark),
                b(0.3, 0.1, 0.06, 0.65, 0.05, 0.17, StoneDark),
                b(0.06, 0.1, 0.4, 0.83, 0.05, 0.0, StoneDark),
                b(0.24, 0.03, 0.24, 0.65, 0.015, 0.0, Fire),
            ]);
        }
        StationKind::StoneFridge => {
            v.extend([
                b(0.9, 0.86, 0.7, 0.0, 0.43, 0.0, Stone),
                b(0.7, 0.62, 0.04, 0.0, 0.43, 0.36, StoneDark),
                b(0.06, 0.18, 0.04, 0.26, 0.45, 0.39, Iron),
                b(0.94, 0.06, 0.74, 0.0, 0.89, 0.0, Moss),
            ]);
        }
        StationKind::Telescope => {
            let long = if has("improved_telescope") { 1.2 } else { 0.85 };
            for sx in [-1.0f32, 1.0] {
                v.push(tilted(b(0.05, 0.9, 0.05, sx * 0.18, 0.43, 0.0, WoodDark), sx * 0.22));
            }
            v.push(b(0.05, 0.9, 0.05, 0.0, 0.43, -0.16, WoodDark));
            v.push(tilted(b(long, 0.14, 0.14, 0.05, 0.98, 0.0, Brass), 0.45));
            let tip = Vec3::new(0.05 + long / 2.0 * 0.45f32.cos(), 0.98 + long / 2.0 * 0.45f32.sin(), 0.0);
            let lens = if has("diamond_lens") { Crystal } else { Linen };
            v.push(tilted(b(0.04, 0.16, 0.16, tip.x, tip.y, tip.z, lens), 0.45));
        }
        StationKind::MagicalLens => {
            // A plate on a stone base; a brass arm holds a ring of crystal
            // over it, and a dial beside the plate focuses it.
            v.extend([
                b(0.7, 0.08, 0.5, 0.0, 0.04, 0.0, StoneDark),
                b(0.4, 0.015, 0.4, 0.0, 0.0875, 0.0, Linen),
                // The dial's stand (the dial turns: `tool`).
                b(0.05, 0.12, 0.05, -0.3, 0.14, 0.0, Brass),
                b(0.05, 0.7, 0.05, -0.3, 0.43, -0.18, Brass),
                b(0.3, 0.04, 0.04, -0.15, 0.6, -0.18, Brass),
                b(0.04, 0.04, 0.2, 0.0, 0.6, -0.08, Brass),
                b(0.32, 0.02, 0.03, 0.0, 0.6, -0.16, Crystal),
                b(0.32, 0.02, 0.03, 0.0, 0.6, 0.16, Crystal),
                b(0.03, 0.02, 0.32, -0.16, 0.6, 0.0, Crystal),
                b(0.03, 0.02, 0.32, 0.16, 0.6, 0.0, Crystal),
            ]);
        }
        StationKind::Cauldron => {
            // An iron pot on three legs over a fire whose mouth runs out at -x;
            // the paddle rests on a stone at +x.
            let metal = if has("steel_cauldron") { Steel } else { Iron };
            let s = if has("steel_cauldron") { 1.15 } else { 1.0 };
            let cx = 0.08;
            for (x, z) in [(-0.22, -0.2), (0.22, -0.2), (0.0, 0.24)] {
                v.push(b(0.07, 0.2, 0.07, cx + x * s, 0.1, z * s, metal));
            }
            // An open pot: walls round a see-through brew (`Mat::alpha`).
            let surface = cauldron_surface(has);
            hollow(&mut v, Vec3::new(cx, 0.2, 0.0), 0.62 * s, 0.45 * s, 0.05, metal, surface, Brew);
            v.extend([
                b(0.2, 0.08, 0.2, cx, 0.04, 0.0, Fire),
                b(0.15, 0.1, 0.1, -0.45, 0.05, -0.17, Stone),
                b(0.15, 0.1, 0.1, -0.45, 0.05, 0.17, Stone),
                b(0.08, 0.1, 0.28, -0.62, 0.05, 0.0, StoneDark),
                b(0.3, 0.03, 0.24, -0.45, 0.015, 0.0, Fire),
                b(0.2, 0.08, 0.34, 0.62, 0.04, 0.0, StoneDark),
            ]);
        }
        StationKind::GolemFrame => {
            v.extend([
                b(0.07, 1.7, 0.07, -0.55, 0.85, -0.3, Wood),
                b(0.07, 1.7, 0.07, 0.55, 0.85, -0.3, Wood),
                b(1.2, 0.06, 0.06, 0.0, 1.66, -0.3, WoodDark),
            ]);
            if has("golem_heart") {
                // It has walked off: only the scaffold and its cut cords are left.
                v.push(b(0.03, 0.12, 0.03, -0.3, 1.58, -0.1, Cord));
                v.push(b(0.03, 0.12, 0.03, 0.3, 1.58, -0.1, Cord));
            } else {
                golem_body(&mut v, false, false, 1.0);
                v.push(b(0.03, 0.3, 0.03, -0.3, 1.5, -0.1, Cord));
                v.push(b(0.03, 0.3, 0.03, 0.3, 1.5, -0.1, Cord));
            }
        }
        StationKind::Golem => golem_body(&mut v, true, has("golem_hands"), -1.0),
        StationKind::Bedframe => {
            // Rails around the bedroll (0.9 x 1.9, x across, z along), posts at the corners.
            let w = if has("double_bed") { 2.0 } else { 1.04 };
            let off = (w - 1.04) / 2.0;
            v.extend([
                b(w, 0.1, 0.08, off, 0.12, 1.0, Wood),
                b(w, 0.22, 0.08, off, 0.2, -1.0, WoodDark),
                b(0.08, 0.1, 2.0, -0.52, 0.12, 0.0, Wood),
                b(0.08, 0.1, 2.0, w - 0.52, 0.12, 0.0, Wood),
            ]);
            for (x, z) in [(-0.52, -1.0), (w - 0.52, -1.0), (-0.52, 1.0), (w - 0.52, 1.0)] {
                v.push(b(0.1, 0.3, 0.1, x, 0.15, z, WoodDark));
            }
            if has("double_bed") {
                v.push(b(0.9, 0.12, 1.9, 0.98, 0.06, 0.0, Cloth));
                v.push(b(0.9, 0.06, 1.0, 0.98, 0.15, 0.35, Hide));
                v.push(b(0.72, 0.14, 0.36, 0.98, 0.14, -0.72, Linen));
            }
        }
    }
    v
}
