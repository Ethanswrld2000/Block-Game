//! Villagers: one per hut, named, keeping a day. 21:00-6:00 indoors, mornings
//! and evenings at the plaza, the day about the village. Right click one to
//! hear what they have to say (`assets/data/villagers.ron`).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::Deserialize;

use super::brain::{plan, Plan};
use super::data::Kinds;
use super::{flat, ground_near, spawn_creature, walk_to, wander, Creature, CreatureAssets, CreatureSet, Dice};
use crate::blocks::Blocks;
use crate::clock::WorldClock;
use crate::interact::UseCreature;
use crate::player::Player;
use crate::survival::Toast;
use crate::world::{Generator, World};

pub struct VillagersPlugin;

impl Plugin for VillagersPlugin {
    fn build(&self, app: &mut App) {
        let lines: Talk = ron::from_str(include_str!("../../../../assets/data/villagers.ron")).unwrap_or_else(|e| panic!("villagers.ron: {e}"));
        app.insert_resource(lines)
            .init_resource::<Populated>()
            .add_systems(crate::menu::GameUpdate, (populate, keep_day, talk).chain().in_set(CreatureSet).after(super::take_strikes).before(super::walk));
    }
}

/// Villagers are spawned when you come within this many blocks of a village.
const VISIT: f32 = 120.0;
/// ...and removed when you are this far away.
const LEAVE: f32 = 180.0;

#[derive(Resource, Deserialize)]
struct Talk {
    morning: Vec<String>,
    day: Vec<String>,
    evening: Vec<String>,
    night: Vec<String>,
}

#[derive(Component, Debug)]
pub struct Villager {
    pub name: String,
    pub village_name: String,
    /// Feet position just outside their door.
    pub door: Vec3,
    /// Centre of the plaza.
    pub center: Vec3,
    pub radius: f32,
    /// Indoors (hidden) for the night.
    pub home: bool,
    /// Seconds left standing still to talk.
    pub talking: f32,
}

/// Villages whose people are out, by village id.
#[derive(Resource, Default)]
struct Populated {
    live: HashMap<u64, (Vec3, Vec<Entity>)>,
}

#[allow(clippy::too_many_arguments)]
fn populate(
    mut commands: Commands,
    time: Res<Time>,
    kinds: Option<Res<Kinds>>,
    assets: Option<Res<CreatureAssets>>,
    generator: Option<Res<Generator>>,
    world: Res<World>,
    blocks: Res<Blocks>,
    clock: Res<WorldClock>,
    player: Res<Player>,
    mut populated: ResMut<Populated>,
    mut timer: Local<f32>,
) {
    let (Some(kinds), Some(assets), Some(generator)) = (kinds, assets, generator) else { return };
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 1.0;
    let Some(villager) = kinds.id("villager") else { return };
    // Gone far away: they stay home.
    populated.live.retain(|_, (center, list)| {
        let keep = flat(*center - player.pos).length() < LEAVE;
        if !keep {
            for e in list.iter() {
                commands.entity(*e).despawn();
            }
        }
        keep
    });
    let (px, pz) = (player.pos.x as i32, player.pos.z as i32);
    let r = VISIT as i32;
    let indoors = plan(clock.hour()) == Plan::Home;
    for v in generator.0.villages_in(px - r, pz - r, px + r, pz + r) {
        let center = Vec3::new(v.center[0] as f32 + 0.5, v.center[1] as f32 + 1.0, v.center[2] as f32 + 0.5);
        if populated.live.contains_key(&v.id) || flat(center - player.pos).length() > VISIT {
            continue;
        }
        // Everyone needs ground under them first.
        let doors: Option<Vec<Vec3>> = v
            .huts
            .iter()
            .map(|h| {
                let (x, z) = h.doorstep();
                ground_near(&world, &blocks, x, z, h.ground + 4, h.ground - 8, false).map(|y| Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5))
            })
            .collect();
        let Some(doors) = doors else { continue };
        let mut list = Vec::new();
        for (i, door) in doors.into_iter().enumerate() {
            let name = worldgen::person_name(v.id.wrapping_mul(31).wrapping_add(i as u64 * 0x9E37_79B9));
            let e = spawn_creature(&mut commands, &kinds, &assets, villager, door, 0.0);
            let mut c = Creature::new(villager, kinds.get(villager).health, 0.0);
            c.fade = 1.0;
            c.leash = Some((center, v.radius as f32 - 6.0));
            commands.entity(e).insert((
                c,
                Villager { name, village_name: v.name.clone(), door, center, radius: v.radius as f32, home: indoors, talking: 0.0 },
                if indoors { Visibility::Hidden } else { Visibility::Inherited },
            ));
            list.push(e);
        }
        info!("{} villagers out in {}", list.len(), v.name);
        populated.live.insert(v.id, (center, list));
    }
}

#[allow(clippy::too_many_arguments)]
fn keep_day(
    time: Res<Time>,
    kinds: Option<Res<Kinds>>,
    clock: Res<WorldClock>,
    player: Res<Player>,
    mut q: Query<(&mut Transform, &mut Creature, &mut Villager, &mut Visibility)>,
    mut dice: ResMut<Dice>,
) {
    let Some(kinds) = kinds else { return };
    let dt = time.delta_secs();
    let now = plan(clock.hour());
    for (mut tf, mut c, mut v, mut vis) in &mut q {
        let speed = kinds.get(c.kind).speed;
        let run = kinds.get(c.kind).run;
        c.hurt = (c.hurt - dt).max(0.0);
        c.scared = (c.scared - dt).max(0.0);
        c.think -= dt;
        v.talking = (v.talking - dt).max(0.0);
        let pos = tf.translation;
        if now != Plan::Home && v.home {
            // Out the door in the morning.
            v.home = false;
            *vis = Visibility::Inherited;
            tf.translation = v.door;
            c.vel = Vec3::ZERO;
            c.think = 0.0;
            continue;
        }
        if v.home {
            c.want = Vec3::ZERO;
            continue;
        }
        if c.scared > 0.0 {
            c.want = -flat(player.pos - pos).normalize_or_zero() * run;
            continue;
        }
        if v.talking > 0.0 {
            c.want = Vec3::ZERO;
            let to = player.pos - pos;
            c.yaw = (-to.x).atan2(-to.z);
            continue;
        }
        match now {
            Plan::Home => {
                c.goal = Some(v.door);
                walk_to(&mut c, pos, speed * 1.2);
                if flat(v.door - pos).length() < 1.3 {
                    v.home = true;
                    *vis = Visibility::Hidden;
                }
            }
            Plan::Plaza => {
                if c.think <= 0.0 {
                    c.think = dice.0.range(5.0, 10.0);
                    let a = dice.0.range(0.0, std::f32::consts::TAU);
                    c.goal = Some(v.center + Vec3::new(a.cos(), 0.0, a.sin()) * dice.0.range(2.5, 6.0));
                }
                walk_to(&mut c, pos, speed);
            }
            Plan::Wander => {
                c.leash = Some((v.center, v.radius - 6.0));
                wander(&mut c, pos, speed, &mut dice.0);
            }
        }
    }
}

fn talk(
    mut uses: MessageReader<UseCreature>,
    lines: Res<Talk>,
    clock: Res<WorldClock>,
    mut q: Query<&mut Villager>,
    mut toast: MessageWriter<Toast>,
    mut dice: ResMut<Dice>,
) {
    for u in uses.read() {
        let Ok(mut v) = q.get_mut(u.0) else { continue };
        let h = clock.hour();
        let pool = match h {
            h if (5.0..10.0).contains(&h) => &lines.morning,
            h if (10.0..18.0).contains(&h) => &lines.day,
            h if (18.0..21.0).contains(&h) => &lines.evening,
            _ => &lines.night,
        };
        if let Some(line) = dice.0.pick(pool) {
            let line = line.replace("{village}", &v.village_name).replace("{name}", &v.name);
            toast.write(Toast(format!("{}: \"{line}\"", v.name)));
        }
        v.talking = 4.0;
    }
}
