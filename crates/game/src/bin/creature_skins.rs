//! Writes paintable skins for the creatures in `assets/data/creatures.ron`.
//!
//! `cargo run --bin creature_skins` (add `--force` to overwrite painted skins, or
//! creature names to do only those). For every skin a creature lists:
//!
//! - `assets/textures/creatures/<skin>.png` is written if it doesn't exist yet: each
//!   box's faces filled with its colour and a little speckle, ready to paint over.
//! - `assets/textures/creatures/guides/<creature>.png` is always rewritten: the
//!   same size, transparent, with each face outlined and lettered (T top, U under,
//!   F front, B back, L left, R right). Put it on a layer above the skin while painting.
//!
//! The layout comes from `game::creatures::skin`, the same code the game uses.

#[path = "../creatures/skin.rs"]
#[allow(dead_code)]
mod skin;

use std::path::{Path, PathBuf};

use image::{Rgba, RgbaImage};
use serde::Deserialize;
use skin::{CreatureBox, Face, Layout, Rect};

#[derive(Deserialize)]
struct Kind {
    name: String,
    model: Vec<CreatureBox>,
    #[serde(default)]
    skin: Option<String>,
    #[serde(default)]
    skins: Vec<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let force = args.iter().any(|a| a == "--force");
    let only: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let assets = std::env::var_os("BEVY_ASSET_ROOT").map(|r| PathBuf::from(r).join("assets")).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
    let assets = assets.canonicalize().unwrap_or(assets);
    let text = std::fs::read_to_string(assets.join("data/creatures.ron")).expect("read creatures.ron");
    let kinds: Vec<Kind> = ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&text)
        .unwrap_or_else(|e| panic!("creatures.ron: {e}"));
    let dir = assets.join("textures/creatures");
    std::fs::create_dir_all(dir.join("guides")).expect("create textures/creatures/guides");
    for k in kinds.iter().filter(|k| only.is_empty() || only.contains(&&k.name)) {
        let skins: Vec<&String> = k.skin.iter().chain(&k.skins).collect();
        if skins.is_empty() {
            println!("{}: no skin listed (add skin: \"{}\" to creatures.ron)", k.name, k.name);
            continue;
        }
        let layout = Layout::new(&k.model);
        println!("{}: {}x{} px", k.name, layout.width, layout.height);
        for (i, (b, net)) in k.model.iter().zip(&layout.nets).enumerate() {
            let name = if b.name.is_empty() { format!("box {i}") } else { b.name.clone() };
            let (w, h, d) = skin::pixels(b.size);
            println!("  {name:<18} at ({:>3}, {:>3})  {w}x{h}x{d} px", net.bounds.x, net.bounds.y);
        }
        let guide = dir.join("guides").join(format!("{}.png", k.name));
        draw_guide(k, &layout).save(&guide).expect("write guide");
        println!("  guide  {}", guide.display());
        for (v, s) in skins.iter().enumerate() {
            let path = dir.join(format!("{s}.png"));
            if path.exists() && !force {
                match image::image_dimensions(&path) {
                    Ok((w, h)) if w * layout.height != h * layout.width || w % layout.width != 0 => {
                        println!("  kept   {} -- but it is {w}x{h} and the layout is now {}x{}: repaint it against the guide", path.display(), layout.width, layout.height)
                    }
                    _ => println!("  kept   {} (already painted; --force overwrites)", path.display()),
                }
                continue;
            }
            draw_starter(k, &layout, v as u32).save(&path).expect("write skin");
            println!("  wrote  {}", path.display());
        }
    }
}

fn srgb8(c: (f32, f32, f32), shade: f32) -> Rgba<u8> {
    let f = |x: f32| ((x * shade).clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba([f(c.0), f(c.1), f(c.2), 255])
}

/// Cheap hash noise, -1..1.
fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x27d4_eb2d) ^ y.wrapping_mul(0x1656_67b1) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    (h & 0xffff) as f32 / 32767.5 - 1.0
}

fn pixels_of(r: Rect) -> impl Iterator<Item = (u32, u32)> {
    (r.y..r.y + r.h).flat_map(move |y| (r.x..r.x + r.w).map(move |x| (x, y)))
}

/// Each face in its box's colour, lightly speckled, edges a shade darker.
fn draw_starter(k: &Kind, layout: &Layout, variant: u32) -> RgbaImage {
    let mut img = RgbaImage::new(layout.width, layout.height);
    for (b, net) in k.model.iter().zip(&layout.nets) {
        for r in net.faces {
            let edged = r.w >= 4 && r.h >= 4;
            for (x, y) in pixels_of(r) {
                let edge = edged && (x == r.x || y == r.y || x == r.x + r.w - 1 || y == r.y + r.h - 1);
                let shade = 1.0 + 0.07 * noise(x, y, variant + 1) - if edge { 0.12 } else { 0.0 };
                img.put_pixel(x, y, srgb8(b.color, shade));
            }
        }
    }
    img
}

/// Letter drawn on each patch of the guide.
fn letter(f: Face) -> char {
    match f {
        Face::Right => 'R',
        Face::Left => 'L',
        Face::Top => 'T',
        Face::Bottom => 'U',
        Face::Back => 'B',
        Face::Front => 'F',
    }
}

/// 3x5 letters for the face labels.
fn glyph(c: char) -> [u8; 5] {
    match c {
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        _ => [0; 5],
    }
}

fn face_color(f: Face) -> Rgba<u8> {
    match f {
        Face::Top => Rgba([255, 230, 80, 255]),
        Face::Bottom => Rgba([150, 120, 255, 255]),
        Face::Front => Rgba([255, 80, 80, 255]),
        Face::Back => Rgba([80, 200, 255, 255]),
        Face::Left => Rgba([90, 230, 120, 255]),
        Face::Right => Rgba([255, 150, 60, 255]),
    }
}

/// Outlines and letters on a transparent sheet the size of the skin.
fn draw_guide(k: &Kind, layout: &Layout) -> RgbaImage {
    let mut img = RgbaImage::new(layout.width, layout.height);
    for net in layout.nets.iter().take(k.model.len()) {
        for (f, r) in Face::ALL.into_iter().zip(net.faces) {
            let c = face_color(f);
            for (x, y) in pixels_of(r) {
                let edge = x == r.x || y == r.y || x == r.x + r.w - 1 || y == r.y + r.h - 1;
                img.put_pixel(x, y, if edge { c } else { Rgba([c[0], c[1], c[2], 40]) });
            }
            if r.w >= 5 && r.h >= 7 {
                let (ox, oy) = (r.x + (r.w - 3) / 2, r.y + (r.h - 5) / 2);
                for (row, bits) in glyph(letter(f)).into_iter().enumerate() {
                    for col in 0..3 {
                        if bits & (0b100 >> col) != 0 {
                            img.put_pixel(ox + col, oy + row as u32, c);
                        }
                    }
                }
            }
        }
    }
    img
}
