//! Guns: with a gun in your right hand (`Item::gun`, items.ron), left click
//! fires. Each shot uses one of its ammo (iron shot, from the gunsmith bench),
//! then the gun needs `reload` seconds. The shot goes straight from your eyes
//! up to `range` blocks and hits the first foe or creature in its way, unless
//! a block is nearer. A foe facing you with its blade raised still turns it.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use voxel::BlockPos;

use super::{hit_foe, Foe, FoeBook, FoeDice};
use crate::blocks::Blocks;
use crate::camera::{FlyCamera, Menus};
use crate::creatures::data::Kinds;
use crate::creatures::Creature;
use crate::interact::{Hand, HandAnim, Strike, Wielded};
use crate::inventory::Inventory;
use crate::items::Items;
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};
use crate::survival::Toast;
use crate::swing::Swing;
use crate::world::World;

/// How far `along` a ray (origin, unit dir) passes nearest a vertical segment
/// from `foot` up `height`, if it comes within `radius`; `None` if it misses.
pub fn ray_hits_body(origin: Vec3, dir: Vec3, foot: Vec3, height: f32, radius: f32) -> Option<f32> {
    // Closest points between the ray and the segment (both lines, then clamped).
    let u = Vec3::Y;
    let w = origin - foot;
    let b = dir.dot(u);
    let d = dir.dot(w);
    let e = u.dot(w);
    let denom = 1.0 - b * b;
    let (mut t, mut s): (f32, f32) = if denom.abs() < 1e-6 { (0.0, e) } else { ((b * e - d) / denom, (e - b * d) / denom) };
    if s < 0.0 || s > height {
        // Past an end of the segment: the nearest ray point to that end.
        s = s.clamp(0.0, height);
        t = (foot + u * s - origin).dot(dir);
    }
    if t < 0.0 {
        return None;
    }
    let gap = (origin + dir * t).distance(foot + u * s);
    (gap <= radius).then_some(t)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn fire(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    menus: Res<Menus>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    cam: Single<&Transform, With<FlyCamera>>,
    wielded: Res<Wielded>,
    items: Res<Items>,
    (world, blocks): (Res<World>, Res<Blocks>),
    (book, kinds): (Res<FoeBook>, Option<Res<Kinds>>),
    mut inv: ResMut<Inventory>,
    mut anim: ResMut<HandAnim>,
    mut foes: Query<(Entity, &Transform, &mut Foe)>,
    creatures: Query<(Entity, &Transform, &Creature), Without<Foe>>,
    mut dice: ResMut<FoeDice>,
    (mut strikes, mut sound, mut bursts, mut toast, mut impact): (MessageWriter<Strike>, MessageWriter<PlaySound>, MessageWriter<Burst>, MessageWriter<Toast>, MessageWriter<super::feel::Impact>),
    mut reload: Local<f32>,
) {
    *reload -= time.delta_secs();
    let Some(gun) = wielded.0.and_then(|i| items.get(i).gun) else { return };
    if !mouse.just_pressed(MouseButton::Left) || menus.open || cursor.grab_mode == CursorGrabMode::None || *reload > 0.0 {
        return;
    }
    if !inv.take(gun.ammo, 1) {
        sound.write(PlaySound::new("ui_drop").gain(0.8).speed(1.6));
        toast.write(Toast(format!("Out of {}", items.get(gun.ammo).title)));
        *reload = 0.4;
        return;
    }
    *reload = gun.reload;
    anim.start(Hand::Right, Swing::Jab);
    let origin = cam.translation;
    let dir = cam.forward().as_vec3();
    sound.write(PlaySound::new("gunshot"));
    bursts.write(Burst::new(origin + dir * 0.9 + cam.right().as_vec3() * 0.25 - Vec3::Y * 0.2, BurstKind::Smoke, 6));

    let wall = voxel::raycast::raycast(origin.to_array(), dir.to_array(), gun.range, |p: BlockPos| world.block(p).is_some_and(|b| blocks.registry.get(b).solid))
        .map(|h| {
            let min = Vec3::new(h.block.x as f32, h.block.y as f32, h.block.z as f32);
            crate::interact::ray_box(origin, dir, min, min + Vec3::ONE).unwrap_or(gun.range)
        })
        .unwrap_or(gun.range);
    // The nearest body in the shot's way.
    enum Target {
        Foe(Entity),
        Creature(Entity),
    }
    let mut best: Option<(f32, Target)> = None;
    for (e, tf, _) in &foes {
        if let Some(t) = ray_hits_body(origin, dir, tf.translation, 1.85, 0.45)
            && t < wall
            && best.as_ref().is_none_or(|b| t < b.0)
        {
            best = Some((t, Target::Foe(e)));
        }
    }
    for (e, tf, c) in &creatures {
        let (h, r) = kinds.as_ref().map_or((1.0, 0.5), |k| {
            let k = k.get(c.kind);
            (k.height, k.half.max(0.3))
        });
        if let Some(t) = ray_hits_body(origin, dir, tf.translation, h, r)
            && t < wall
            && best.as_ref().is_none_or(|b| t < b.0)
        {
            best = Some((t, Target::Creature(e)));
        }
    }
    match best {
        Some((_, Target::Foe(e))) => {
            if let Ok((_, tf, mut foe)) = foes.get_mut(e) {
                let k = &book.kinds[foe.kind];
                let strength = hit_foe(tf, &mut foe, k, origin, gun.damage, &mut dice.0, &mut sound, &mut bursts);
                impact.write(super::feel::Impact { strength: strength * 0.6 });
            }
        }
        Some((_, Target::Creature(e))) => {
            strikes.write(Strike { entity: e, damage: gun.damage, dir: Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero(), yours: true });
        }
        None if wall < gun.range => {
            bursts.write(Burst::new(origin + dir * (wall - 0.05), BurstKind::Chips, 6));
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shot_hits_a_body_in_its_way_and_misses_one_beside_it() {
        let eye = Vec3::new(0.0, 1.6, 0.0);
        let ahead = Vec3::NEG_Z;
        let t = ray_hits_body(eye, ahead, Vec3::new(0.0, 0.0, -10.0), 1.8, 0.45).expect("hit");
        assert!((t - 10.0).abs() < 0.5);
        assert!(ray_hits_body(eye, ahead, Vec3::new(2.0, 0.0, -10.0), 1.8, 0.45).is_none());
        // Over its head, and behind you.
        assert!(ray_hits_body(eye + Vec3::Y * 3.0, ahead, Vec3::new(0.0, 0.0, -10.0), 1.8, 0.45).is_none());
        assert!(ray_hits_body(eye, ahead, Vec3::new(0.0, 0.0, 10.0), 1.8, 0.45).is_none());
    }

    #[test]
    fn guns_fire_iron_shot() {
        let (_, items) = crate::items::tests::load();
        for g in ["makeshift_pistol", "rifle"] {
            let gun = items.get(items.id(g).unwrap()).gun.expect(g);
            assert_eq!(items.get(gun.ammo).name, "iron_shot");
            assert!(gun.damage > 0.0 && gun.range > 0.0);
        }
    }
}
