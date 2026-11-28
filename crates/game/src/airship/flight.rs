//! Flying an airship: pure rules, no ECS. The ship is a box (hull below,
//! envelope above) that turns about its middle and moves one axis at a time,
//! stopping on whatever solid block it would run into.

use bevy::math::{IVec3, Quat, Vec3};
use serde::Deserialize;

/// Half the ship's footprint across (x) and along (z), in blocks.
pub const HALF: Vec3 = Vec3::new(1.7, 0.0, 4.2);
/// From the bottom of the keel to the top of the envelope.
pub const HEIGHT: f32 = 6.8;
/// Where you stand at the helm, from the middle of the keel's bottom (ship's frame).
pub const HELM: Vec3 = Vec3::new(0.0, 1.03, 1.8);
/// Where you land when you step off: over the starboard rail.
pub const STEP_OFF: Vec3 = Vec3::new(2.3, 1.03, 1.8);
const _: () = assert!(HELM.x.abs() < HALF.x && HELM.z < HALF.z && STEP_OFF.x > HALF.x);

/// One machine's numbers, from `assets/data/machines.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct Spec {
    /// Blocks per second at full ahead.
    pub speed: f32,
    /// Blocks per second up or down.
    pub climb: f32,
    /// Degrees per second at full rudder.
    pub turn: f32,
    /// Seconds to come up to speed.
    pub ease: f32,
}

/// Every machine the machination table can build.
#[derive(Debug, Clone, Deserialize)]
pub struct Machines {
    pub airship: Spec,
}

impl Machines {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| format!("machines.ron: {e}"))
    }
}

/// An airship in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ship {
    /// The middle of the keel's bottom.
    pub pos: Vec3,
    /// Which way the bow points (0 = -z, like the camera's yaw).
    pub yaw: f32,
    pub vel: Vec3,
    /// Radians per second.
    pub spin: f32,
}

impl Ship {
    pub fn new(pos: Vec3, yaw: f32) -> Self {
        Self {
            pos,
            yaw,
            vel: Vec3::ZERO,
            spin: 0.0,
        }
    }

    pub fn rotation(self) -> Quat {
        Quat::from_rotation_y(self.yaw)
    }

    /// A point in the ship's frame, in the world.
    pub fn to_world(self, local: Vec3) -> Vec3 {
        self.pos + self.rotation() * local
    }
}

/// The helm: each -1..1.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Controls {
    /// + ahead (W), - astern (S).
    pub ahead: f32,
    /// + to port / left (A), - to starboard (D).
    pub turn: f32,
    /// + rise (Space), - sink (Ctrl).
    pub lift: f32,
}

/// Does a ship standing at `pos` turned to `yaw` touch a solid block?
pub fn hits(pos: Vec3, yaw: f32, solid: &dyn Fn(IVec3) -> bool) -> bool {
    let rot = Quat::from_rotation_y(yaw);
    let inv = rot.inverse();
    // The turned footprint's square around the middle.
    let ext = (rot * HALF)
        .abs()
        .max((rot * Vec3::new(HALF.x, 0.0, -HALF.z)).abs());
    let lo = (pos - ext).floor().as_ivec3();
    let hi = (pos + ext).floor().as_ivec3();
    let (y0, y1) = (pos.y.floor() as i32, (pos.y + HEIGHT).ceil() as i32 - 1);
    for x in lo.x..=hi.x {
        for z in lo.z..=hi.z {
            // A block counts when its middle lies within half a block of the footprint.
            let local = inv * (Vec3::new(x as f32 + 0.5, pos.y, z as f32 + 0.5) - pos);
            if local.x.abs() > HALF.x + 0.5 || local.z.abs() > HALF.z + 0.5 {
                continue;
            }
            for y in y0..=y1 {
                // Resting on top of a block isn't touching it.
                if (y + 1) as f32 <= pos.y + 1e-3 || y as f32 >= pos.y + HEIGHT {
                    continue;
                }
                if solid(IVec3::new(x, y, z)) {
                    return true;
                }
            }
        }
    }
    false
}

/// How low and how high the keel may go.
pub fn height_limits() -> (f32, f32) {
    (
        worldgen::WORLD_FLOOR as f32 + 8.0,
        worldgen::WORLD_CEILING as f32 - HEIGHT - 1.0,
    )
}

/// One step of flight. Speed and turning ease towards what the helm asks;
/// the ship turns and moves only where it fits (one axis at a time, so it
/// slides along walls and settles onto the ground). A ship already stuck in
/// something may move to work itself free.
pub fn step(ship: &mut Ship, c: Controls, spec: &Spec, dt: f32, solid: &dyn Fn(IVec3) -> bool) {
    let k = (dt / spec.ease.max(0.05)).min(1.0);
    let forward = ship.rotation() * Vec3::NEG_Z;
    let want = forward * (c.ahead.clamp(-1.0, 1.0) * spec.speed)
        + Vec3::Y * (c.lift.clamp(-1.0, 1.0) * spec.climb);
    ship.vel += (want - ship.vel) * k;
    ship.spin += (c.turn.clamp(-1.0, 1.0) * spec.turn.to_radians() - ship.spin) * k;

    let stuck = hits(ship.pos, ship.yaw, solid);
    let yaw = ship.yaw + ship.spin * dt;
    if stuck || !hits(ship.pos, yaw, solid) {
        ship.yaw = yaw;
    } else {
        ship.spin = 0.0;
    }
    let (low, high) = height_limits();
    for axis in 0..3 {
        let mut p = ship.pos;
        p[axis] += ship.vel[axis] * dt;
        p.y = p.y.clamp(low, high);
        if stuck || !hits(p, ship.yaw, solid) {
            ship.pos = p;
        } else {
            ship.vel[axis] = 0.0;
        }
    }
}

/// The ship is left alone: it stops where it is.
pub fn moor(ship: &mut Ship) {
    ship.vel = Vec3::ZERO;
    ship.spin = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Spec {
        Machines::from_ron(include_str!("../../../../assets/data/machines.ron"))
            .unwrap()
            .airship
    }

    fn fly(ship: &mut Ship, c: Controls, seconds: f32, solid: &dyn Fn(IVec3) -> bool) {
        for _ in 0..(seconds * 60.0) as usize {
            step(ship, c, &spec(), 1.0 / 60.0, solid);
        }
    }

    #[test]
    fn full_ahead_flies_the_way_the_bow_points() {
        let mut s = Ship::new(Vec3::new(0.5, 100.0, 0.5), 0.0);
        fly(
            &mut s,
            Controls {
                ahead: 1.0,
                ..default()
            },
            5.0,
            &|_| false,
        );
        assert!(s.pos.z < -50.0, "{}", s.pos);
        assert!(
            (s.pos.x - 0.5).abs() < 1e-3 && (s.pos.y - 100.0).abs() < 1e-3,
            "{}",
            s.pos
        );
        assert!((s.vel.length() - spec().speed).abs() < 0.5);
    }

    #[test]
    fn turning_left_swings_the_bow_left_and_climbing_rises() {
        let mut s = Ship::new(Vec3::new(0.5, 100.0, 0.5), 0.0);
        fly(
            &mut s,
            Controls {
                turn: 1.0,
                lift: 1.0,
                ..default()
            },
            1.5,
            &|_| false,
        );
        let bow = s.rotation() * Vec3::NEG_Z;
        assert!(bow.x < -0.5, "bow {bow}");
        assert!(s.pos.y > 103.0, "{}", s.pos);
    }

    #[test]
    fn it_settles_on_the_ground_and_stops_at_walls() {
        let ground = |p: IVec3| p.y < 10;
        let mut s = Ship::new(Vec3::new(0.5, 14.0, 0.5), 0.0);
        fly(
            &mut s,
            Controls {
                lift: -1.0,
                ..default()
            },
            3.0,
            &ground,
        );
        assert!(
            (s.pos.y - 10.0).abs() < 0.2,
            "rests on the ground: {}",
            s.pos
        );
        assert!(!hits(s.pos, s.yaw, &ground));

        let wall = |p: IVec3| p.z <= -20;
        let mut s = Ship::new(Vec3::new(0.5, 50.0, 0.5), 0.0);
        fly(
            &mut s,
            Controls {
                ahead: 1.0,
                ..default()
            },
            5.0,
            &wall,
        );
        assert!(!hits(s.pos, s.yaw, &wall));
        assert!(s.pos.z - HALF.z < -18.0, "flew up to the wall: {}", s.pos);
    }

    #[test]
    fn the_footprint_turns_with_the_ship() {
        // A post beside the hull's middle: clear when the ship lies along z,
        // in the way when it lies across it.
        let post = |p: IVec3| p.x == 3 && p.z == 0;
        let at = Vec3::new(0.5, 10.0, 0.5);
        assert!(!hits(at, 0.0, &post));
        assert!(hits(at, std::f32::consts::FRAC_PI_2, &post));
    }

    fn default() -> Controls {
        Controls::default()
    }
}
