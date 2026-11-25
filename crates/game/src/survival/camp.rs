//! Bedroll camping and sleep.
//!
//! The bedroll has its own fixed slot: you always have exactly one. B unrolls
//! it on a clear, flat 2x2 patch in front of you (or rolls it back up when
//! you're next to it). Where it lies is your camp: you wake there after
//! dying. Z sleeps on it, at night or when you're worn out: the night passes,
//! you're rested, and wounds mend a little.
//!
//! Away from home: hold a `portable_bedroll` or `portable_tent` (camp mat
//! recipes) and right click the ground to lay it out on a clear 2x2 patch
//! (`Camp.away`, one at a time). Z by it sleeps there (the home bed's extras
//! don't come along; a tent wakes you warm). Your camp stays where it is: you
//! still wake there after dying. Inside a pitched tent the weather is kept off
//! (`find_cover`). Crouch and right click by it with an empty hand to pack it up.

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
            .add_systems(crate::menu::GameUpdate, (bedroll_key, lay_out, sleep_key, sleep).chain().before(SurvivalSet).after(crate::player::PlayerSet))
            .add_systems(crate::menu::GameUpdate, (show_bedroll, show_away));
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
    /// The spare bedroll or tent laid out away from home.
    pub away: Option<Away>,
    away_entity: Option<(Entity, Away)>,
    /// What the sleeping station has built around the bedroll (`station` keeps it up to date).
    pub bed: Bed,
    entity: Option<(Entity, Bedroll)>,
}

/// A spare bedroll or tent laid out somewhere.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Away {
    pub bed: Bedroll,
    pub tent: bool,
}

impl Away {
    /// Under the tent (or on the bedroll): within its 2x2 patch, up to head height.
    pub fn inside(&self, p: Vec3) -> bool {
        let c = self.bed.center();
        (p.x - c.x).abs() <= 1.2 && (p.z - c.z).abs() <= 1.2 && (p.y - c.y).abs() <= 1.5
    }

    /// The item it packs back into.
    pub fn item(&self) -> &'static str {
        if self.tent { "portable_tent" } else { "portable_bedroll" }
    }
}

/// Warmth you wake with after a night in a tent, °C.
const TENT_WARMTH: f32 = 16.0;

impl Camp {
    pub fn near_away(&self, p: Vec3) -> bool {
        self.away.is_some_and(|a| a.bed.center().distance(p) <= CAMP_REACH)
    }

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

/// The bed the ancestral urn builds around the bedroll.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Bed {
    /// Off the wet ground: deeper sleep, warmer mornings.
    pub frame: bool,
    /// Wide, with furs: sleep warm through winter.
    pub double: bool,
}

impl Bed {
    /// What a night in this bed does on top of the bedroll's own rest:
    /// (extra healing of injuries and blood, food saved, the warmth you wake with, °C).
    pub fn rest(self) -> (f32, f32, Option<f32>) {
        match (self.frame, self.double) {
            (_, true) => (25.0, 8.0, Some(22.0)),
            (true, false) => (15.0, 5.0, Some(16.0)),
            _ => (0.0, 0.0, None),
        }
    }
}

/// A night's sleep in progress.
#[derive(Resource, Default, Debug)]
pub struct Sleep {
    t: Option<f32>,
    slept: bool,
    /// Sleeping on the spare bedroll or in the tent, not at home.
    away: bool,
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
            let open = |q: BlockPos| world.block(q).is_some_and(|b| Some(b) != water && !blocks.registry.get(b).solid && blocks.registry.get(b).shape != voxel::Shape::Torch);
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
    story: Res<crate::cutscene::Story>,
    mut sleep: ResMut<Sleep>,
    mut toast: MessageWriter<Toast>,
) {
    if !keys.just_pressed(KeyCode::KeyZ) || *mode != MoveMode::Walk || sleep.active() || s.vitals.health.is_dead() {
        return;
    }
    let away = camp.near_away(player.pos) && !camp.near(player.pos);
    if camp.bedroll.is_none() && !away {
        toast.write(Toast("Unroll your bedroll first (B)".into()));
    } else if !camp.near(player.pos) && !away {
        toast.write(Toast("Too far from your bedroll".into()));
    } else if crate::guide::rules::bed_missing(&story.flags) {
        toast.write(Toast("Make your bed first: put 4 sticks and 4 plant fiber in the urn".into()));
    } else if !(is_night(clock.hour()) || s.vitals.health.get(Layer::Fatigue) < EXHAUSTED) {
        toast.write(Toast("You can only sleep at night, or when worn out".into()));
    } else {
        *sleep = Sleep { t: Some(0.0), slept: false, away };
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
        // The home bed's extras stay at home; a tent wakes you warm.
        let tent = sleep.away && camp.away.is_some_and(|a| a.tent);
        let (mend, fed, warm) = if sleep.away { (0.0, 0.0, tent.then_some(TENT_WARMTH)) } else { camp.bed.rest() };
        s.vitals.health.heal(Layer::Injuries, mend);
        s.vitals.health.heal(Layer::Blood, mend);
        s.vitals.satiety = (s.vitals.satiety + fed).min(100.0);
        if let Some(w) = warm {
            s.vitals.feels = s.vitals.feels.max(w);
        }
        let wake = if sleep.away { camp.away.map(|a| a.bed.center() + Vec3::Y * 0.01) } else { camp.respawn() };
        if let Some(p) = wake {
            player.pos = p;
            player.vel = Vec3::ZERO;
        }
        let note = if s.vitals.health.mortal().is_some() { " Your wounds still need treating." } else { "" };
        let bed = match (camp.bed.frame, camp.bed.double) {
            _ if tent => " The tent kept the weather off.",
            _ if sleep.away => "",
            (_, true) => " Warm under the furs of the double bed.",
            (true, false) => " The bedframe kept you off the wet ground.",
            _ => "",
        };
        toast.write(Toast(format!("Slept {hours:.0} hours.{bed}{note}")));
    }
    if t >= END {
        *sleep = Sleep::default();
    }
}

/// Hold a portable bedroll or tent and right click the ground to lay it out;
/// crouch and right click by it with an empty hand to pack it up.
#[allow(clippy::too_many_arguments)]
fn lay_out(
    mouse: Res<ButtonInput<MouseButton>>,
    mode: Res<MoveMode>,
    menus: Res<crate::camera::Menus>,
    (player, target, wielded, items): (Res<Player>, Res<crate::interact::Target>, Res<crate::interact::Wielded>, Res<crate::items::Items>),
    (world, blocks): (Res<World>, Res<Blocks>),
    cam: Query<&FlyCamera>,
    mut inv: ResMut<crate::inventory::Inventory>,
    mut camp: ResMut<Camp>,
    mut toast: MessageWriter<Toast>,
) {
    if !mouse.just_pressed(MouseButton::Right) || *mode != MoveMode::Walk || menus.open {
        return;
    }
    // Pack it up.
    if player.crouching && wielded.0.is_none() {
        if let Some(a) = camp.away
            && camp.near_away(player.pos)
            && let Some(id) = items.id(a.item())
        {
            if inv.add(&items, crate::inventory::grid::Stack::new(id, 1)).is_some() {
                toast.write(Toast("No room in your pockets to pack it up".into()));
            } else {
                camp.away = None;
                toast.write(Toast(format!("Packed up the {}", if a.tent { "tent" } else { "spare bedroll" })));
            }
        }
        return;
    }
    let Some(item) = wielded.0 else { return };
    let tent = match items.get(item).name.as_str() {
        "portable_bedroll" => false,
        "portable_tent" => true,
        _ => return,
    };
    if camp.away.is_some() {
        toast.write(Toast("Pack up the one you laid out first (crouch and right click by it)".into()));
        return;
    }
    let Some(hit) = target.hit.filter(|h| h.before.y == h.block.y + 1) else {
        toast.write(Toast("Lay it out on top of the ground".into()));
        return;
    };
    let Ok(look) = cam.single() else { return };
    let (sy, cy) = look.yaw.sin_cos();
    let along_x = sy.abs() > cy.abs();
    let (cx, y, cz) = (hit.before.x, hit.before.y, hit.before.z);
    // The 2x2 patch whose corner is nearest where you clicked.
    let corners = [(cx, cz), (cx + 1, cz), (cx, cz + 1), (cx + 1, cz + 1)];
    let Some((x, z)) = corners.into_iter().find(|(x, z)| clear_patch(&world, &blocks, BlockPos::new(x - 1, y, z - 1))) else {
        toast.write(Toast("Needs a clear, flat 2x2 patch".into()));
        return;
    };
    if !inv.take(item, 1) {
        return;
    }
    camp.away = Some(Away { bed: Bedroll { pos: (x as f32, y as f32, z as f32), along_x }, tent });
    toast.write(Toast(if tent { "Tent pitched: Z sleeps in it, and it keeps the weather off" } else { "Spare bedroll laid out: Z sleeps on it" }.into()));
}

/// Keep a model of the spare bedroll or tent where it lies.
fn show_away(mut commands: Commands, mut camp: ResMut<Camp>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    if camp.away_entity.map(|(_, a)| a) == camp.away {
        return;
    }
    if let Some((e, _)) = camp.away_entity.take() {
        commands.entity(e).despawn();
    }
    let Some(a) = camp.away else { return };
    let mat = |m: &mut Assets<StandardMaterial>, r: f32, g: f32, b: f32| m.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: 1.0, ..default() });
    let cloth = mat(&mut materials, 0.45, 0.38, 0.26);
    let linen = mat(&mut materials, 0.78, 0.72, 0.58);
    let hide = mat(&mut materials, 0.62, 0.48, 0.32);
    let pole = mat(&mut materials, 0.4, 0.28, 0.16);
    let rot = if a.bed.along_x { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
    let e = commands
        .spawn((Transform::from_translation(a.bed.center()).with_rotation(rot), Visibility::default()))
        .with_children(|c| {
            c.spawn((Mesh3d(meshes.add(Cuboid::new(0.9, 0.12, 1.9))), MeshMaterial3d(cloth), Transform::from_xyz(0.0, 0.06, 0.0), NotShadowCaster));
            c.spawn((Mesh3d(meshes.add(Cuboid::new(0.72, 0.14, 0.36))), MeshMaterial3d(linen), Transform::from_xyz(0.0, 0.14, -0.72), NotShadowCaster));
            if a.tent {
                // Two hide sides leaning together over a ridge pole, open at the ends.
                let side = meshes.add(Cuboid::new(0.06, 1.75, 2.2));
                for s in [-1.0f32, 1.0] {
                    c.spawn((Mesh3d(side.clone()), MeshMaterial3d(hide.clone()), Transform::from_xyz(s * 0.55, 0.78, 0.0).with_rotation(Quat::from_rotation_z(s * 0.62))));
                }
                c.spawn((Mesh3d(meshes.add(Cuboid::new(0.08, 0.08, 2.4))), MeshMaterial3d(pole.clone()), Transform::from_xyz(0.0, 1.5, 0.0)));
                for z in [-1.1f32, 1.1] {
                    c.spawn((Mesh3d(meshes.add(Cuboid::new(0.08, 1.5, 0.08))), MeshMaterial3d(pole.clone()), Transform::from_xyz(0.0, 0.75, z)));
                }
            }
        })
        .id();
    camp.away_entity = Some((e, a));
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
    fn better_beds_rest_you_more() {
        let none = Bed::default().rest();
        let frame = Bed { frame: true, double: false }.rest();
        let double = Bed { frame: true, double: true }.rest();
        assert!(none.0 < frame.0 && frame.0 < double.0);
        assert!(none.2.is_none() && frame.2 < double.2);
    }

    #[test]
    fn a_tent_covers_its_patch() {
        let a = Away { bed: Bedroll { pos: (10.0, 5.0, -3.0), along_x: false }, tent: true };
        assert!(a.inside(Vec3::new(10.4, 5.0, -2.2)));
        assert!(!a.inside(Vec3::new(12.0, 5.0, -3.0)));
        assert!(!a.inside(Vec3::new(10.0, 8.0, -3.0)));
        assert_eq!(a.item(), "portable_tent");
    }

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
