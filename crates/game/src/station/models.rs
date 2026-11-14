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
            Mat::Urn => (c(urn.0, urn.1, urn.2), 0.0),
            Mat::UrnDark => (c(urn.0 * 0.72, urn.1 * 0.7, urn.2 * 0.7), 0.0),
            Mat::MarkLit => (c((urn.0 + 0.6).min(1.0), (urn.1 + 0.55).min(1.0), (urn.2 + 0.4).min(1.0)), 3.0),
            Mat::MarkUnlit => (c(0.25, 0.2, 0.16), 0.0),
            Mat::Fire => (c(1.0, 0.55, 0.18), 4.0),
            Mat::Brew => (c(0.35, 0.9, 0.45), 2.5),
            Mat::Spirit => (c(0.5, 0.8, 1.0), 3.0),
            Mat::Crystal => (c(0.75, 0.95, 1.0), 2.0),
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

/// Marks around the urn's belly.
pub const MARKS: u32 = 10;

/// Half size of each station's box, for aiming (before turning).
pub fn half(kind: StationKind) -> Vec3 {
    match kind {
        StationKind::Urn => Vec3::new(0.32, 0.45, 0.32),
        StationKind::CampMat => Vec3::new(0.6, 0.1, 0.45),
        StationKind::ToolBench => Vec3::new(0.6, 0.45, 0.4),
        StationKind::CarpentryBench => Vec3::new(0.85, 0.45, 0.45),
        StationKind::PersonalStorage => Vec3::new(0.5, 0.4, 0.3),
        StationKind::SharpeningStone => Vec3::new(0.45, 0.5, 0.3),
        StationKind::IronAnvil => Vec3::new(0.35, 0.4, 0.25),
        StationKind::CookingPot => Vec3::new(0.4, 0.5, 0.4),
        StationKind::CuttingBoard => Vec3::new(0.4, 0.3, 0.25),
        StationKind::SmokingRack => Vec3::new(0.6, 0.65, 0.3),
        StationKind::StoneFridge => Vec3::new(0.45, 0.45, 0.35),
        StationKind::Telescope => Vec3::new(0.45, 0.65, 0.3),
        StationKind::MagicalLens => Vec3::new(0.3, 0.6, 0.2),
        StationKind::Cauldron => Vec3::new(0.4, 0.4, 0.4),
        StationKind::GolemFrame => Vec3::new(0.6, 0.85, 0.4),
        StationKind::Bedframe => Vec3::new(0.5, 0.15, 1.0),
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
                b(0.2, 0.03, 0.18, 0.35, 0.145, 0.23, StoneDark),
                b(0.16, 0.05, 0.16, 0.3, 0.055, -0.15, Cord),
            ]);
        }
        StationKind::ToolBench => {
            legs(&mut v, 1.06, 0.7, 0.72, 0.1, WoodDark);
            v.extend([
                b(1.2, 0.1, 0.8, 0.0, 0.77, 0.0, Wood),
                b(0.62, 0.012, 0.14, -0.15, 0.826, -0.12, Groove),
                b(0.26, 0.012, 0.26, 0.32, 0.826, 0.12, Groove),
                b(0.22, 0.14, 0.18, 0.42, 0.89, -0.24, Stone),
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
                b(0.5, 0.012, 0.11, 0.15, 0.852, -0.3, Stone),
                b(0.16, 0.06, 0.09, -0.16, 0.87, -0.3, WoodDark),
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
            legs(&mut v, 0.8, 0.5, 0.45, 0.08, WoodDark);
            v.extend([
                b(0.8, 0.06, 0.5, 0.0, 0.48, 0.0, Wood),
                b(0.14, 0.52, 0.52, 0.0, 0.78, 0.0, Stone),
                b(0.16, 0.36, 0.36, 0.0, 0.78, 0.0, StoneDark),
                b(0.5, 0.05, 0.05, 0.0, 0.78, 0.0, Iron),
                b(0.05, 0.2, 0.05, 0.27, 0.7, 0.0, Iron),
                b(0.3, 0.1, 0.3, -0.25, 0.56, 0.0, Clay),
            ]);
        }
        StationKind::IronAnvil => {
            let metal = if has("alchemical_anvil") || has("steel_anvil") { Steel } else { Iron };
            v.extend([
                b(0.5, 0.42, 0.46, 0.0, 0.21, 0.0, WoodDark),
                b(0.34, 0.1, 0.26, 0.0, 0.47, 0.0, metal),
                b(0.2, 0.14, 0.16, 0.0, 0.59, 0.0, metal),
                b(0.56, 0.12, 0.24, 0.0, 0.72, 0.0, metal),
                b(0.16, 0.08, 0.12, 0.35, 0.72, 0.0, metal),
                b(0.24, 0.05, 0.05, -0.12, 0.81, 0.08, WoodDark),
                b(0.08, 0.08, 0.1, -0.24, 0.81, 0.08, Iron),
            ]);
            if has("alchemical_anvil") {
                for x in [-0.18, 0.0, 0.18] {
                    v.push(b(0.05, 0.05, 0.01, x, 0.72, 0.125, Spirit));
                    v.push(b(0.05, 0.05, 0.01, x, 0.72, -0.125, Spirit));
                }
            }
        }
        StationKind::CookingPot => {
            for i in 0..8 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                v.push(b(0.14, 0.1, 0.14, a.cos() * 0.32, 0.05, a.sin() * 0.32, Stone));
            }
            v.extend([
                b(0.24, 0.08, 0.24, 0.0, 0.04, 0.0, WoodDark),
                b(0.12, 0.16, 0.12, 0.0, 0.12, 0.0, Fire),
                b(0.44, 0.32, 0.44, 0.0, 0.46, 0.0, Clay),
                b(0.34, 0.04, 0.34, 0.0, 0.63, 0.0, Groove),
                b(0.03, 0.4, 0.03, 0.0, 0.86, 0.0, Cord),
                b(0.9, 0.05, 0.05, 0.0, 1.06, 0.0, WoodDark),
            ]);
            for sx in [-1.0, 1.0] {
                v.push(tilted(b(0.06, 1.1, 0.06, sx * 0.38, 0.52, 0.0, Wood), sx * 0.12));
            }
        }
        StationKind::CuttingBoard => {
            v.extend([
                b(0.6, 0.44, 0.44, 0.0, 0.22, 0.0, WoodDark),
                b(0.7, 0.06, 0.46, 0.0, 0.47, 0.0, Wood),
                b(0.22, 0.02, 0.06, 0.12, 0.51, 0.12, Stone),
                b(0.12, 0.03, 0.05, -0.04, 0.515, 0.12, WoodDark),
                b(0.14, 0.05, 0.1, -0.18, 0.52, -0.08, Hide),
            ]);
        }
        StationKind::SmokingRack => {
            v.extend([
                b(0.07, 1.25, 0.07, -0.56, 0.62, 0.0, Wood),
                b(0.07, 1.25, 0.07, 0.56, 0.62, 0.0, Wood),
                b(1.2, 0.05, 0.05, 0.0, 1.2, 0.0, WoodDark),
                b(1.2, 0.05, 0.05, 0.0, 0.9, 0.0, WoodDark),
                b(0.5, 0.12, 0.4, 0.0, 0.06, 0.0, StoneDark),
                b(0.36, 0.06, 0.28, 0.0, 0.14, 0.0, Fire),
            ]);
            for x in [-0.36, -0.12, 0.12, 0.36] {
                v.push(b(0.1, 0.22, 0.04, x, 1.03, 0.0, Hide));
                v.push(b(0.1, 0.18, 0.04, x + 0.06, 0.75, 0.0, Hide));
            }
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
            v.extend([
                b(0.36, 0.08, 0.3, 0.0, 0.04, 0.0, StoneDark),
                b(0.06, 0.6, 0.06, 0.0, 0.38, 0.0, Brass),
                b(0.5, 0.06, 0.06, 0.0, 0.7, 0.0, Brass),
                b(0.5, 0.06, 0.06, 0.0, 1.16, 0.0, Brass),
                b(0.06, 0.46, 0.06, -0.22, 0.93, 0.0, Brass),
                b(0.06, 0.46, 0.06, 0.22, 0.93, 0.0, Brass),
                b(0.38, 0.4, 0.02, 0.0, 0.93, 0.0, Crystal),
            ]);
        }
        StationKind::Cauldron => {
            let metal = if has("steel_cauldron") { Steel } else { Iron };
            let s = if has("steel_cauldron") { 1.15 } else { 1.0 };
            for (x, z) in [(-0.22, -0.2), (0.22, -0.2), (0.0, 0.24)] {
                v.push(b(0.07, 0.2, 0.07, x * s, 0.1, z * s, metal));
            }
            v.extend([
                b(0.62 * s, 0.42 * s, 0.62 * s, 0.0, 0.2 + 0.21 * s, 0.0, metal),
                b(0.7 * s, 0.06, 0.7 * s, 0.0, 0.2 + 0.42 * s, 0.0, metal),
                b(0.5 * s, 0.03, 0.5 * s, 0.0, 0.19 + 0.42 * s, 0.0, Brew),
                b(0.2, 0.08, 0.2, 0.0, 0.04, 0.0, Fire),
            ]);
        }
        StationKind::GolemFrame => {
            v.extend([
                b(0.07, 1.7, 0.07, -0.55, 0.85, -0.3, Wood),
                b(0.07, 1.7, 0.07, 0.55, 0.85, -0.3, Wood),
                b(1.2, 0.06, 0.06, 0.0, 1.66, -0.3, WoodDark),
                b(0.5, 0.3, 0.36, 0.0, 0.15, 0.0, StoneDark),
                b(0.6, 0.6, 0.4, 0.0, 0.62, 0.0, Stone),
                b(0.36, 0.3, 0.32, 0.0, 1.1, 0.0, Stone),
                b(0.18, 0.5, 0.2, -0.42, 0.62, 0.0, Stone),
                b(0.18, 0.5, 0.2, 0.42, 0.62, 0.0, Stone),
            ]);
            let core = if has("golem_heart") { Spirit } else { MarkUnlit };
            v.push(b(0.14, 0.14, 0.02, 0.0, 0.7, 0.205, core));
            v.push(b(0.06, 0.05, 0.02, -0.08, 1.12, 0.165, core));
            v.push(b(0.06, 0.05, 0.02, 0.08, 1.12, 0.165, core));
            if has("golem_hands") {
                v.push(b(0.22, 0.16, 0.24, -0.42, 0.3, 0.0, Iron));
                v.push(b(0.22, 0.16, 0.24, 0.42, 0.3, 0.0, Iron));
            }
            // Cords holding it up until it wakes.
            if !has("golem_heart") {
                v.push(b(0.03, 0.3, 0.03, -0.3, 1.5, -0.1, Cord));
                v.push(b(0.03, 0.3, 0.03, 0.3, 1.5, -0.1, Cord));
            }
        }
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
