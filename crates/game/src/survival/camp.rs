//! Bedroll camping and sleep.
//!
//! The bedroll has its own fixed slot: you always have exactly one. B unrolls
//! it on a clear, flat 2x2 patch in front of you (or rolls it back up when
//! you're next to it). Where it lies is your camp: you wake there after
//! dying. Z sleeps on it, at night or when you're worn out: the night passes,
//! you're rested, and wounds mend a little.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use voxel::BlockPos;

use super::health::Layer;
use super::{Survival, SurvivalSet, Toast};
use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::clock::WorldClock;
use crate::player::{MoveMode, Player};
use crate::world::World;

/// How close to the bedroll you must be to sleep or roll it up.
pub const CAMP_REACH: f32 = 4.0;
/// Fatigue below this lets you sleep in the daytime.
pub const EXHAUSTED: f32 = 30.0;
/// Hours you sleep when you lie down in the daytime.
const NAP_HOURS: f32 = 4.0;
/// Sleep fade timeline, seconds: dark by FADE_IN, the night passes, light again by END.
const FADE_IN: f32 = 1.2;
const FADE_OUT: f32 = 2.0;
const END: f32 = 3.5;

pub struct CampPlugin;

impl Plugin for CampPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Sleep>()
            .add_systems(Update, (bedroll_key, sleep_key, sleep).chain().before(SurvivalSet).after(crate::player::PlayerSet))
            .add_systems(Update, show_bedroll);
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Bedroll {
    /// Centre of the 2x2 patch, on the ground.
    pub pos: (f32, f32, f32),
    /// Lies along the x axis (otherwise along z).
    pub along_x: bool,
}

impl Bedroll {
    pub fn center(&self) -> Vec3 {
        Vec3::new(self.pos.0, self.pos.1, self.pos.2)
    }
}

#[derive(Resource, Default, Debug)]
pub struct Camp {
    pub bedroll: Option<Bedroll>,
    entity: Option<(Entity, Bedroll)>,
}

impl Camp {
    /// Where to wake after dying: beside the bedroll.
    pub fn respawn(&self) -> Option<Vec3> {
        self.bedroll.map(|b| {
            let side = if b.along_x { Vec3::Z } else { Vec3::X };
            b.center() + side * 0.5 + Vec3::Y * 0.01
        })
    }

    pub fn near(&self, p: Vec3) -> bool {
        self.bedroll.is_some_and(|b| b.center().distance(p) <= CAMP_REACH)
    }
}

/// A night's sleep in progress.
#[derive(Resource, Default, Debug)]
pub struct Sleep {
    t: Option<f32>,
    slept: bool,
}

impl Sleep {
    pub fn active(&self) -> bool {
        self.t.is_some()
    }

    /// How dark the screen is, 0..1.
    pub fn darkness(&self) -> f32 {
        match self.t {
            None => 0.0,
            Some(t) if t < FADE_IN => t / FADE_IN,
            Some(t) if t < FADE_OUT => 1.0,
            Some(t) => (1.0 - (t - FADE_OUT) / (END - FADE_OUT)).max(0.0),
        }
    }
}

/// Is a 2x2 patch with its lower corner at `c` flat and clear?
fn clear_patch(world: &World, blocks: &Blocks, c: BlockPos) -> bool {
    let solid = |p: BlockPos| world.block(p).map(|b| blocks.registry.get(b).solid);
    let water = blocks.registry.id("water");
    (0..2).all(|dx| {
        (0..2).all(|dz| {
            let p = BlockPos::new(c.x + dx, c.y, c.z + dz);
            let open = |q: BlockPos| world.block(q).is_some_and(|b| Some(b) != water && !blocks.registry.get(b).solid && blocks.registry.get(b).shape == voxel::Shape::Cube);
            open(p) && open(BlockPos::new(p.x, p.y + 1, p.z)) && solid(BlockPos::new(p.x, p.y - 1, p.z)) == Some(true)
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn bedroll_key(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<MoveMode>,
    player: Res<Player>,
    world: Res<World>,
    blocks: Res<Blocks>,
    cam: Query<&FlyCamera>,
    sleep: Res<Sleep>,
    s: Res<Survival>,
    mut camp: ResMut<Camp>,
    mut toast: MessageWriter<Toast>,
) {
    if !keys.just_pressed(KeyCode::KeyB) || *mode != MoveMode::Walk || sleep.active() || s.vitals.health.is_dead() {
        return;
    }
    if let Some(b) = camp.bedroll {
        if camp.near(player.pos) {
            camp.bedroll = None;
            toast.write(Toast("Rolled up the bedroll".into()));
        } else {
            let c = b.center();
            toast.write(Toast(format!("Your bedroll is at {:.0}, {:.0}: go back to roll it up", c.x, c.z)));
        }
        return;
    }
    if !player.on_ground {
        toast.write(Toast("Stand on solid ground to unroll the bedroll".into()));
        return;
    }
    let Ok(look) = cam.single() else { return };
    let (sy, cy) = look.yaw.sin_cos();
    let forward = Vec3::new(-sy, 0.0, -cy);
    let along_x = forward.x.abs() > forward.z.abs();
    // The patch's centre is the block corner about two blocks ahead.
    let ahead = player.pos + forward * 1.9;
    let (cx, cz) = (ahead.x.round() as i32, ahead.z.round() as i32);
    let y = player.pos.y.floor() as i32;
    // Allow a step up or down.
    for dy in [0, 1, -1] {
        let corner = BlockPos::new(cx - 1, y + dy, cz - 1);
        if clear_patch(&world, &blocks, corner) {
            camp.bedroll = Some(Bedroll { pos: (cx as f32, (y + dy) as f32, cz as f32), along_x });
            toast.write(Toast("Camp made: you'll wake here".into()));
            return;
        }
    }
    toast.write(Toast("Needs a clear, flat 2x2 patch in front of you".into()));
}

#[allow(clippy::too_many_arguments)]
fn sleep_key(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<MoveMode>,
    player: Res<Player>,
    clock: Res<WorldClock>,
    camp: Res<Camp>,
    s: Res<Survival>,
    mut sleep: ResMut<Sleep>,
    mut toast: MessageWriter<Toast>,
) {
    if !keys.just_pressed(KeyCode::KeyZ) || *mode != MoveMode::Walk || sleep.active() || s.vitals.health.is_dead() {
        return;
    }
    if camp.bedroll.is_none() {
        toast.write(Toast("Unroll your bedroll first (B)".into()));
    } else if !camp.near(player.pos) {
        toast.write(Toast("Too far from your bedroll".into()));
    } else if !(is_night(clock.hour()) || s.vitals.health.get(Layer::Fatigue) < EXHAUSTED) {
        toast.write(Toast("You can only sleep at night, or when worn out".into()));
    } else {
        *sleep = Sleep { t: Some(0.0), slept: false };
    }
}

pub fn is_night(hour: f32) -> bool {
    !(6.0..19.0).contains(&hour)
}

/// Morning after sleeping at `hour`: (days to add, new time of day 0..1).
pub fn wake_time(hour: f32) -> (u32, f32) {
    if hour >= 19.0 {
        (1, 6.0 / 24.0)
    } else if hour < 6.0 {
        (0, 6.0 / 24.0)
    } else {
        let h = hour + NAP_HOURS;
        (0, h / 24.0)
    }
}

fn sleep(
    time: Res<Time>,
    camp: Res<Camp>,
    mut sleep: ResMut<Sleep>,
    mut clock: ResMut<WorldClock>,
    mut s: ResMut<Survival>,
    mut player: ResMut<Player>,
    mut toast: MessageWriter<Toast>,
) {
    let Some(t) = sleep.t.as_mut() else { return };
    *t += time.delta_secs();
    let t = *t;
    if t >= FADE_IN && !sleep.slept {
        sleep.slept = true;
        let before = clock.hour();
        let (days, time) = wake_time(before);
        clock.day += days;
        clock.time = time;
        let hours = (days as f32 * 24.0 + time * 24.0 - before).max(0.0);
        s.vitals.sleep();
        if let Some(p) = camp.respawn() {
            player.pos = p;
            player.vel = Vec3::ZERO;
        }
        let note = if s.vitals.health.mortal().is_some() { " Your wounds still need treating." } else { "" };
        toast.write(Toast(format!("Slept {hours:.0} hours.{note}")));
    }
    if t >= END {
        *sleep = Sleep::default();
    }
}

type BedrollAssets = (Handle<Mesh>, Handle<Mesh>, Handle<StandardMaterial>, Handle<StandardMaterial>);

/// Keep a bedroll model where the camp is.
fn show_bedroll(
    mut commands: Commands,
    mut camp: ResMut<Camp>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<BedrollAssets>>,
) {
    if camp.entity.map(|(_, b)| b) == camp.bedroll {
        return;
    }
    if let Some((e, _)) = camp.entity.take() {
        commands.entity(e).despawn();
    }
    let Some(b) = camp.bedroll else { return };
    let (roll, pillow, cloth, linen) = assets
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(0.9, 0.12, 1.9)),
                meshes.add(Cuboid::new(0.72, 0.14, 0.36)),
                materials.add(StandardMaterial { base_color: Color::srgb(0.33, 0.4, 0.22), perceptual_roughness: 1.0, ..default() }),
                materials.add(StandardMaterial { base_color: Color::srgb(0.78, 0.72, 0.58), perceptual_roughness: 1.0, ..default() }),
            )
        })
        .clone();
    let rot = if b.along_x { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
    let e = commands
        .spawn((Transform::from_translation(b.center()).with_rotation(rot), Visibility::default()))
        .with_children(|c| {
            c.spawn((Mesh3d(roll), MeshMaterial3d(cloth), Transform::from_xyz(0.0, 0.06, 0.0), NotShadowCaster));
            c.spawn((Mesh3d(pillow), MeshMaterial3d(linen), Transform::from_xyz(0.0, 0.14, -0.72), NotShadowCaster));
        })
        .id();
    camp.entity = Some((e, b));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeping_wakes_you_in_the_morning() {
        assert!(is_night(22.0) && is_night(3.0) && !is_night(12.0));
        assert_eq!(wake_time(22.0), (1, 0.25));
        assert_eq!(wake_time(2.0), (0, 0.25));
        let (d, t) = wake_time(13.0);
        assert_eq!(d, 0);
        assert!((t * 24.0 - 17.0).abs() < 1e-4);
    }
}
