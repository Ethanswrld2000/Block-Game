//! Room rewards, boons and the coin shop.
//!
//! Fight rooms and ambushes promise a reward, shown before you go in by a
//! glowing mark in each corridor leading to them (gold coin, blue boon, red
//! heal, purple elite; walking up to one names it). Clearing the room gives it
//! (`locks::RoomCleared`): a handful of old coin, a boon from `boons.ron`
//! (they stack until you leave the dungeon: `Boons`), your wounds mended, or
//! for an elite fight a second share of loot. A shop room has Tallow, who
//! sells for old coin (trades.ron `shop`, through `cast::services`).

use bevy::prelude::*;
use serde::Deserialize;
use worldgen::delve::Reward;

use super::locks::RoomCleared;
use super::{Below, DelveSet};
use crate::inventory::grid::Stack;
use crate::inventory::loot::Loot;
use crate::inventory::Spill;
use crate::items::Items;
use crate::npc::{body::SpawnNpc, Npc, NpcBook, Order};
use crate::player::Player;
use crate::sound::PlaySound;
use crate::survival::health::Layer;
use crate::survival::{Survival, Toast};

pub struct BoonsPlugin;

impl Plugin for BoonsPlugin {
    fn build(&self, app: &mut App) {
        let data: BoonData = ron::from_str(include_str!("../../../../assets/data/boons.ron")).unwrap_or_else(|e| panic!("boons.ron: {e}"));
        app.insert_resource(data)
            .init_resource::<Boons>()
            .add_systems(crate::menu::GameUpdate, (forget, reward, markers, mend, thirst, shopkeeper).chain().after(DelveSet).after(super::locks::LocksSet));
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    Might(f32),
    Ward(f32),
    Fleet(f32),
    Mend(f32),
    Thirst(f32),
    Fortune(f32),
}

#[derive(Deserialize, Debug, Clone)]
pub struct Boon {
    #[allow(dead_code)]
    pub name: String,
    pub title: String,
    pub text: String,
    pub effect: Effect,
}

#[derive(Resource, Deserialize, Debug, Clone)]
pub struct BoonData {
    pub boons: Vec<Boon>,
}

/// The boons taken this run (they stack; gone when you leave the dungeon).
#[derive(Resource, Default, Debug, Clone)]
pub struct Boons {
    pub taken: Vec<Effect>,
}

impl Boons {
    fn sum(&self, f: impl Fn(&Effect) -> Option<f32>) -> f32 {
        self.taken.iter().filter_map(f).sum()
    }
    pub fn might(&self) -> f32 {
        self.sum(|e| if let Effect::Might(v) = e { Some(*v) } else { None })
    }
    /// Never more than 60% off.
    pub fn ward(&self) -> f32 {
        self.sum(|e| if let Effect::Ward(v) = e { Some(*v) } else { None }).min(0.6)
    }
    pub fn fleet(&self) -> f32 {
        self.sum(|e| if let Effect::Fleet(v) = e { Some(*v) } else { None })
    }
    pub fn mend(&self) -> f32 {
        self.sum(|e| if let Effect::Mend(v) = e { Some(*v) } else { None })
    }
    pub fn thirst(&self) -> f32 {
        self.sum(|e| if let Effect::Thirst(v) = e { Some(*v) } else { None })
    }
    pub fn fortune(&self) -> f32 {
        self.sum(|e| if let Effect::Fortune(v) = e { Some(*v) } else { None })
    }
}

/// Old coin for a coin reward (pure): more on deeper floors, `fortune` on top.
pub fn coin_for(floor: u8, roll: f32, fortune: f32) -> u16 {
    let base = 3.0 + 2.0 * floor as f32 + roll * 4.0;
    (base * (1.0 + fortune)).round() as u16
}

/// Leaving the dungeon ends every boon.
fn forget(below: Res<Below>, mut boons: ResMut<Boons>) {
    if below.run.is_none() && !boons.taken.is_empty() {
        boons.taken.clear();
    }
}

#[allow(clippy::too_many_arguments)]
fn reward(
    mut cleared: MessageReader<RoomCleared>,
    below: Res<Below>,
    data: Res<BoonData>,
    mut boons: ResMut<Boons>,
    (items, loot): (Res<Items>, Res<Loot>),
    player: Res<Player>,
    mut s: ResMut<Survival>,
    mut dice: Local<u32>,
    (mut spill, mut toast, mut sound): (MessageWriter<Spill>, MessageWriter<Toast>, MessageWriter<PlaySound>),
) {
    let (Some(floor), Some(run)) = (below.floor.clone(), below.run) else {
        cleared.clear();
        return;
    };
    for RoomCleared(r) in cleared.read() {
        let Some(room) = floor.rooms.get(*r) else { continue };
        let mut roll = || {
            *dice = dice.wrapping_mul(1_664_525).wrapping_add(1_013_904_223 ^ *r as u32);
            (*dice >> 8) as f32 / (1u32 << 24) as f32
        };
        let at = Vec3::from_array(room.middle().map(|v| v as f32)) + Vec3::new(0.5, 1.0, 0.5);
        match room.reward {
            Some(Reward::Coin) => {
                let n = coin_for(run.floor, roll(), boons.fortune());
                if let Some(coin) = items.id("old_coin") {
                    spill.write(Spill { stack: Stack::new(coin, n), at });
                }
                toast.write(Toast(format!("Reward: {n} old coin")));
            }
            Some(Reward::Boon) if !data.boons.is_empty() => {
                let b = &data.boons[(roll() * data.boons.len() as f32) as usize % data.boons.len()];
                boons.taken.push(b.effect);
                toast.write(Toast(format!("Boon: {}. {}", b.title, b.text)));
                sound.write(PlaySound::new("spell_on").gain(0.8));
            }
            Some(Reward::Heal) => {
                let h = &mut s.vitals.health;
                h.bleeding = 0.0;
                for l in [Layer::Injuries, Layer::Blood, Layer::Fatigue, Layer::Damage] {
                    h.heal_any(l, 100.0);
                }
                toast.write(Toast("Reward: your wounds close".into()));
                sound.write(PlaySound::new("spell_on").gain(0.8).speed(1.2));
            }
            Some(Reward::Elite) => {
                for stack in loot.roll("dungeon_treasure", &mut roll) {
                    spill.write(Spill { stack, at });
                }
                toast.write(Toast("Reward: the elite's hoard".into()));
            }
            _ => {}
        }
        let _ = &player;
    }
}

/// The glowing marks in front of rewarded rooms.
#[derive(Component)]
struct Marker {
    room: usize,
    told: bool,
}

#[allow(clippy::too_many_arguments)]
fn markers(
    mut commands: Commands,
    time: Res<Time>,
    below: Res<Below>,
    player: Res<Player>,
    mut marks: Query<(Entity, &mut Marker, &mut Transform)>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut on: Local<Option<(u32, u8)>>,
    mut toast: MessageWriter<Toast>,
) {
    let here = below.run.map(|r| (r.number, r.floor));
    if here != *on {
        *on = here;
        for (e, _, _) in &marks {
            commands.entity(e).despawn();
        }
        let Some(floor) = below.floor.clone() else { return };
        let mesh = meshes.add(Cuboid::new(0.32, 0.32, 0.32));
        for (i, room) in floor.rooms.iter().enumerate() {
            let Some(r) = room.reward else { continue };
            let (c, glow) = match r {
                Reward::Coin => (Color::srgb(1.0, 0.82, 0.3), LinearRgba::rgb(2.0, 1.5, 0.3)),
                Reward::Boon => (Color::srgb(0.4, 0.7, 1.0), LinearRgba::rgb(0.4, 1.0, 2.4)),
                Reward::Heal => (Color::srgb(1.0, 0.35, 0.35), LinearRgba::rgb(2.2, 0.4, 0.4)),
                Reward::Elite => (Color::srgb(0.75, 0.4, 1.0), LinearRgba::rgb(1.4, 0.5, 2.4)),
            };
            let mat = materials.add(StandardMaterial { base_color: c, emissive: glow, unlit: true, ..default() });
            for d in floor.ways_into(i) {
                // Just outside the doorway, in the corridor, at head height.
                let a = &d.at;
                let mid = Vec3::new((a.min[0] + a.max[0] + 1) as f32 / 2.0, a.min[1] as f32 + 2.6, (a.min[2] + a.max[2] + 1) as f32 / 2.0);
                let out = Vec3::new(d.out.0 as f32, 0.0, d.out.1 as f32);
                commands.spawn((
                    Marker { room: i, told: false },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_translation(mid + out * 1.2),
                    bevy::light::NotShadowCaster,
                ));
            }
        }
        return;
    }
    let t = time.elapsed_secs();
    for (e, mut m, mut tf) in &mut marks {
        if below.cleared.contains(&m.room) {
            commands.entity(e).despawn();
            continue;
        }
        tf.rotation = Quat::from_rotation_y(t * 1.2) * Quat::from_rotation_x(0.6);
        if !m.told && tf.translation.distance(player.pos + Vec3::Y * 1.6) < 3.0 {
            m.told = true;
            let what = below.floor.as_ref().and_then(|f| f.rooms.get(m.room)).and_then(|r| r.reward).map_or("", |r| match r {
                Reward::Coin => "Through here: old coin",
                Reward::Boon => "Through here: a boon",
                Reward::Heal => "Through here: healing",
                Reward::Elite => "Through here: an elite, and its hoard",
            });
            toast.write(Toast(what.into()));
        }
    }
}

fn mend(time: Res<Time>, boons: Res<Boons>, mut s: ResMut<Survival>) {
    let m = boons.mend();
    if m > 0.0 {
        let h = &mut s.vitals.health;
        h.heal_any(Layer::Damage, m * time.delta_secs());
        h.heal_any(Layer::Blood, 0.5 * m * time.delta_secs());
    }
}

fn thirst(mut slain: MessageReader<crate::combat::FoeSlain>, boons: Res<Boons>, mut s: ResMut<Survival>) {
    let t = boons.thirst();
    for f in slain.read() {
        if t > 0.0 && super::tag_room(f.tag).is_some() {
            s.vitals.health.heal_any(Layer::Damage, t);
            s.vitals.health.heal_any(Layer::Blood, t * 0.5);
        }
    }
}

/// `Npc.post` of Tallow on a floor (not saved; goes with the floor).
pub const SHOP_POST: u64 = u64::MAX - 7;

/// Tallow stands in the shop room of a floor that has one.
fn shopkeeper(mut commands: Commands, below: Res<Below>, book: Res<NpcBook>, npcs: Query<(Entity, &Npc)>, mut spawn: MessageWriter<SpawnNpc>, mut on: Local<Option<(u32, u8)>>) {
    let here = below.run.map(|r| (r.number, r.floor));
    if here == *on {
        return;
    }
    *on = here;
    for (e, n) in &npcs {
        if n.post == Some(SHOP_POST) {
            commands.entity(e).despawn();
        }
    }
    let Some(at) = below.floor.as_ref().and_then(|f| f.shop) else { return };
    let Some(kind) = book.kinds.iter().position(|k| k.dialogue == "tallow") else { return };
    let mut npc = Npc::new(kind, 0.0);
    npc.order = Order::Stay;
    npc.post = Some(SHOP_POST);
    spawn.write(SpawnNpc { npc, at: Vec3::new(at[0] as f32 + 0.5, at[1] as f32, at[2] as f32 + 0.5) });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boons_load_and_stack() {
        let d: BoonData = ron::from_str(include_str!("../../../../assets/data/boons.ron")).unwrap();
        assert!(d.boons.len() >= 6);
        let mut b = Boons::default();
        assert_eq!(b.might(), 0.0);
        b.taken = vec![Effect::Might(0.2), Effect::Might(0.2), Effect::Ward(0.5), Effect::Ward(0.5)];
        assert!((b.might() - 0.4).abs() < 1e-6, "they stack");
        assert_eq!(b.ward(), 0.6, "ward is capped");
    }

    #[test]
    fn coin_grows_with_depth_and_fortune() {
        assert!(coin_for(3, 0.5, 0.0) > coin_for(1, 0.5, 0.0));
        assert!(coin_for(1, 0.5, 0.5) > coin_for(1, 0.5, 0.0));
    }
}
