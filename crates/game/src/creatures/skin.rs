//! Creature skins: where each box of a model sits in its PNG.
//!
//! Every box is unfolded like a cardboard box into a "net" of six patches:
//!
//! ```text
//!          d      w      w
//!       +------+------+------+
//!    d  |      | top  |bottom|
//!       +------+------+------+------+
//!    h  |right |front | left | back |
//!       +------+------+------+------+
//!          d      w      d      w
//! ```
//!
//! `w`, `h` and `d` are the box's width (x), height (y) and depth (z) in pixels,
//! at [`PX_PER_BLOCK`]. "Front" is the face the creature looks out of (-z), and
//! left and right are the creature's own. Each patch is drawn as you'd see that face
//! from outside (so on the front patch the creature's right is on your left). Sides are upright; the top is
//! seen from above with its front edge at the bottom (touching the front patch),
//! the bottom is seen from below with its front edge at the top.
//!
//! Nets are packed in list order into rows of a fixed width, so adding a box at the
//! end of a model never moves the boxes already painted. A box can be pinned with
//! `uv: (x, y)` (its net's top-left pixel); the others pack around it.
//!
//! Plain Rust, no Bevy: the `creature_skins` tool uses it to write templates.

use serde::Deserialize;

/// Skin pixels per block (metre), the same density as the terrain tiles.
pub const PX_PER_BLOCK: f32 = 32.0;

/// One box of a creature's model, in creature space (metres, y up, -z forward).
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct CreatureBox {
    pub size: (f32, f32, f32),
    pub offset: (f32, f32, f32),
    /// Flat colour, used when the creature has no skin and for the template fill.
    pub color: (f32, f32, f32),
    /// A name for the part ("head", "leg_front_left"), for animation later.
    #[serde(default)]
    pub name: String,
    /// Pin this box's net at a pixel in the skin instead of packing it.
    #[serde(default)]
    pub uv: Option<(u32, u32)>,
    /// Glows by itself (a firefly's tail): drawn unlit and bright.
    #[serde(default)]
    pub glow: bool,
}

/// The six faces, in the order of [`Net::faces`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Right,
    Left,
    Top,
    Bottom,
    Back,
    Front,
}

impl Face {
    pub const ALL: [Face; 6] = [Face::Right, Face::Left, Face::Top, Face::Bottom, Face::Back, Face::Front];

    /// Outward normal in creature space.
    pub fn normal(self) -> [f32; 3] {
        match self {
            Face::Right => [1.0, 0.0, 0.0],
            Face::Left => [-1.0, 0.0, 0.0],
            Face::Top => [0.0, 1.0, 0.0],
            Face::Bottom => [0.0, -1.0, 0.0],
            Face::Back => [0.0, 0.0, 1.0],
            Face::Front => [0.0, 0.0, -1.0],
        }
    }
}

/// A rectangle of pixels in the skin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn overlaps(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// A box's size in skin pixels: (w, h, d), at least one pixel each.
pub fn pixels(size: (f32, f32, f32)) -> (u32, u32, u32) {
    let px = |m: f32| ((m * PX_PER_BLOCK).round() as u32).max(1);
    (px(size.0), px(size.1), px(size.2))
}

/// Where one box's net sits, and each of its faces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Net {
    pub bounds: Rect,
    /// Patches in [`Face::ALL`] order.
    pub faces: [Rect; 6],
}

impl Net {
    fn at(x: u32, y: u32, (w, h, d): (u32, u32, u32)) -> Net {
        let r = |x, y, w, h| Rect { x, y, w, h };
        Net {
            bounds: r(x, y, 2 * (w + d), d + h),
            faces: [
                r(x, y + d, d, h),             // right
                r(x + d + w, y + d, d, h),     // left
                r(x + d, y, w, d),             // top
                r(x + d + w, y, w, d),         // bottom
                r(x + 2 * d + w, y + d, w, h), // back
                r(x + d, y + d, w, h),         // front
            ],
        }
    }

    pub fn face(&self, f: Face) -> Rect {
        self.faces[Face::ALL.iter().position(|g| *g == f).unwrap()]
    }
}

/// The whole skin of one model: its size in pixels and every box's net.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    pub nets: Vec<Net>,
}

impl Layout {
    pub fn new(model: &[CreatureBox]) -> Layout {
        let dims: Vec<_> = model.iter().map(|b| pixels(b.size)).collect();
        let widest = dims.iter().map(|(w, _, d)| 2 * (w + d)).chain(model.iter().zip(&dims).filter_map(|(b, (w, _, d))| b.uv.map(|(x, _)| x + 2 * (w + d)))).max().unwrap_or(1);
        let width = widest.next_power_of_two().max(64);
        let pinned: Vec<Rect> = model.iter().zip(&dims).filter_map(|(b, &dim)| b.uv.map(|(x, y)| Net::at(x, y, dim).bounds)).collect();
        let (mut x, mut y, mut row) = (0, 0, 0);
        let mut nets = Vec::with_capacity(model.len());
        for (b, &dim) in model.iter().zip(&dims) {
            if let Some((px, py)) = b.uv {
                nets.push(Net::at(px, py, dim));
                continue;
            }
            loop {
                let net = Net::at(x, y, dim);
                if net.bounds.x + net.bounds.w > width {
                    (x, y, row) = (0, y + row, 0);
                    continue;
                }
                if let Some(p) = pinned.iter().find(|p| p.overlaps(&net.bounds)) {
                    // Hop over it; this row now reaches at least to its bottom.
                    x = p.x + p.w;
                    row = row.max(p.y + p.h - y);
                    continue;
                }
                x += net.bounds.w;
                row = row.max(net.bounds.h);
                nets.push(net);
                break;
            }
        }
        let bottom = nets.iter().map(|n| n.bounds.y + n.bounds.h).max().unwrap_or(1);
        Layout { width, height: bottom.next_power_of_two().max(16), nets }
    }

    /// Texture coordinates (0..1) of a face's four corners, in the order
    /// [`face_corners`] lists the box's vertices for that face.
    pub fn uvs(&self, part: usize, face: Face) -> [[f32; 2]; 4] {
        let r = self.nets[part].face(face);
        let (w, h) = (self.width as f32, self.height as f32);
        let (u0, v0, u1, v1) = (r.x as f32 / w, r.y as f32 / h, (r.x + r.w) as f32 / w, (r.y + r.h) as f32 / h);
        [[u0, v0], [u1, v0], [u1, v1], [u0, v1]]
    }
}

/// The four corners of a face of a box with half-extents `e`, in the order
/// top-left, top-right, bottom-right, bottom-left as the patch is drawn in the skin.
pub fn face_corners(face: Face, e: [f32; 3]) -> [[f32; 3]; 4] {
    let [x, y, z] = e;
    match face {
        // Looking at the creature's right side (+x), its front (-z) is on our right.
        Face::Right => [[x, y, z], [x, y, -z], [x, -y, -z], [x, -y, z]],
        // Looking at its left side (-x), its front is on our left.
        Face::Left => [[-x, y, -z], [-x, y, z], [-x, -y, z], [-x, -y, -z]],
        // From above, front edge at the bottom of the patch (so its right is on our left).
        Face::Top => [[x, y, z], [-x, y, z], [-x, y, -z], [x, y, -z]],
        // From below, front edge at the top of the patch; its right side on our left.
        Face::Bottom => [[x, -y, -z], [-x, -y, -z], [-x, -y, z], [x, -y, z]],
        // From behind: its right side is on our right.
        Face::Back => [[-x, y, z], [x, y, z], [x, -y, z], [-x, -y, z]],
        // From the front: its right side is on our left.
        Face::Front => [[x, y, -z], [-x, y, -z], [-x, -y, -z], [x, -y, -z]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(size: (f32, f32, f32)) -> CreatureBox {
        CreatureBox { size, offset: (0.0, 0.0, 0.0), color: (1.0, 1.0, 1.0), name: String::new(), uv: None, glow: false }
    }

    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }

    #[test]
    fn net_matches_the_diagram() {
        let n = Net::at(0, 0, (8, 12, 4));
        assert_eq!(n.bounds, Rect { x: 0, y: 0, w: 24, h: 16 });
        assert_eq!(n.face(Face::Top), Rect { x: 4, y: 0, w: 8, h: 4 });
        assert_eq!(n.face(Face::Bottom), Rect { x: 12, y: 0, w: 8, h: 4 });
        assert_eq!(n.face(Face::Right), Rect { x: 0, y: 4, w: 4, h: 12 });
        assert_eq!(n.face(Face::Front), Rect { x: 4, y: 4, w: 8, h: 12 });
        assert_eq!(n.face(Face::Left), Rect { x: 12, y: 4, w: 4, h: 12 });
        assert_eq!(n.face(Face::Back), Rect { x: 16, y: 4, w: 8, h: 12 });
    }

    #[test]
    fn corners_wind_outward() {
        // Corners go clockwise as seen from outside (TL, TR, BR, BL), so the
        // triangles (0, 3, 1) and (1, 3, 2) face outward.
        for f in Face::ALL {
            let c = face_corners(f, [1.0, 2.0, 3.0]);
            let n = cross(sub(c[3], c[0]), sub(c[1], c[0]));
            let dot: f32 = n.iter().zip(f.normal()).map(|(a, b)| a * b).sum();
            assert!(dot > 0.0, "{f:?}");
            for p in c {
                let along: f32 = p.iter().zip(f.normal()).map(|(a, b)| a * b).sum();
                assert!(along > 0.0, "{f:?} corner on the wrong side");
            }
        }
    }

    #[test]
    fn patches_meet_where_the_box_folds() {
        // The top patch's bottom edge touches the front patch's top edge, and the
        // shared 3D edge is the box's top front edge.
        let e = [1.0, 2.0, 3.0];
        let top = face_corners(Face::Top, e);
        let front = face_corners(Face::Front, e);
        assert_eq!((top[3], top[2]), (front[0], front[1]));
        // Right patch sits left of the front: its right edge is the front's left edge.
        let right = face_corners(Face::Right, e);
        assert_eq!((right[1], right[2]), (front[0], front[3]));
        let left = face_corners(Face::Left, e);
        assert_eq!((left[0], left[3]), (front[1], front[2]));
        let back = face_corners(Face::Back, e);
        assert_eq!((back[0], back[3]), (left[1], left[2]));
    }

    #[test]
    fn packing_never_overlaps_and_is_stable() {
        let model: Vec<_> = [(0.8, 0.6, 1.2), (0.5, 0.45, 0.45), (0.08, 0.08, 0.2), (0.18, 0.3, 0.18), (0.18, 0.3, 0.18)].into_iter().map(b).collect();
        let l = Layout::new(&model);
        for (i, a) in l.nets.iter().enumerate() {
            assert!(a.bounds.x + a.bounds.w <= l.width && a.bounds.y + a.bounds.h <= l.height);
            for c in &l.nets[i + 1..] {
                assert!(!a.bounds.overlaps(&c.bounds));
            }
        }
        // A new box at the end leaves the others where they were.
        let mut more = model.clone();
        more.push(b((0.1, 0.1, 0.1)));
        assert_eq!(&Layout::new(&more).nets[..model.len()], &l.nets[..]);
    }

    #[test]
    fn pinned_boxes_stay_put_and_others_avoid_them() {
        let mut model: Vec<_> = (0..6).map(|_| b((0.5, 0.5, 0.5))).collect();
        model[3].uv = Some((0, 0));
        let l = Layout::new(&model);
        assert_eq!((l.nets[3].bounds.x, l.nets[3].bounds.y), (0, 0));
        for (i, a) in l.nets.iter().enumerate() {
            for c in &l.nets[i + 1..] {
                assert!(!a.bounds.overlaps(&c.bounds));
            }
        }
    }

    #[test]
    fn tiny_parts_get_at_least_a_pixel() {
        assert_eq!(pixels((0.01, 0.06, 0.2)), (1, 2, 6));
    }
}
