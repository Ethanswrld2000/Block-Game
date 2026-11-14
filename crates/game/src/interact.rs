//! Using what is in your hand: mining, placing blocks and torches.
//!
//! One hand for now: the hotbar's nine slots are shortcuts to things in your
//! inventory (bind them in the inventory screen). Keys 1-9 or the mouse wheel
//! pick one. Placing a block or torch uses one up; mining sends a `Mined`
//! message, which the inventory turns into drops. Hold left click to mine (how long depends on the block's hardness
//! and your tool, see items.ron); right click places the block or torch you
//! hold. In free-fly mode blocks break instantly.
//!
//! Mining is done in blows: each swing of the arm lands once, at the moment
//! the tool meets the block (`swing::Swing::contact`), and that blow does its
//! share of the work and sends a `Blow` (the knock you hear).
//!
//! `Hands` already has a left slot, always empty for now, so an off-hand can
//! be added later without reworking this.

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use voxel::raycast::raycast;
use voxel::{BlockId, BlockPos, ChunkPos};

use crate::blocks::Blocks;
use crate::camera::FlyCamera;
use crate::inventory::Inventory;
use crate::items::{ItemId, ItemKind, Items};
use crate::player::{MoveMode, Player};
use crate::swing::{self, Swing};
use crate::world::World;

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>()
            .init_resource::<Mining>()
            .init_resource::<HandAnim>()
            .init_resource::<Wielded>()
            .init_resource::<Aimed>()
            // `BLOCKGAME_SLOT=n` starts with hotbar slot n in hand (for screenshots).
            .insert_resource(Hands {
                right: Some(std::env::var("BLOCKGAME_SLOT").ok().and_then(|s| s.parse::<usize>().ok()).map_or(0, |n| n.clamp(1, SLOTS) - 1)),
                left: None,
            })
            .insert_resource(Hotbar { slots: vec![None; SLOTS] })
            .add_message::<Mined>()
            .add_message::<Blow>()
            .add_message::<OpenChest>()
            .add_message::<Placed>()
            .add_message::<Strike>()
            .add_message::<UseCreature>()
            .add_systems(
                Update,
                (choose_items, wield, find_target, use_hands, draw_target)
                    .chain()
                    .in_set(InteractSet)
                    .after(crate::player::PlayerSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct InteractSet;

/// How far you can reach on foot and when flying, in blocks.
pub const REACH: f32 = 5.0;
pub const FLY_REACH: f32 = 10.0;
/// Seconds between placements while a placing click is held.
const PLACE_REPEAT: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hand {
    Left,
    #[default]
    Right,
}

/// Hotbar slots, on keys 1-9.
pub const SLOTS: usize = 9;

/// Shortcuts to item kinds in the inventory.
#[derive(Resource, Debug, Clone)]
pub struct Hotbar {
    pub slots: Vec<Option<ItemId>>,
}

/// What the right hand actually holds: the selected slot's item, if you have any.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct Wielded(pub Option<ItemId>);

/// A block was broken (by hand, not by world generation).
#[derive(Message, Debug, Clone, Copy)]
pub struct Mined {
    pub pos: BlockPos,
    pub block: BlockId,
}

/// A mining swing landed on a block (it may or may not have broken).
#[derive(Message, Debug, Clone, Copy)]
pub struct Blow {
    pub pos: BlockPos,
    pub block: BlockId,
}

/// Right click on a chest.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenChest(pub BlockPos);

/// A block was placed by the player.
#[derive(Message, Debug, Clone, Copy)]
pub struct Placed {
    pub pos: BlockPos,
    pub block: BlockId,
}

/// The creature under the crosshair (set by `creatures`) and how far away.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct Aimed {
    pub entity: Option<Entity>,
    pub dist: f32,
}

/// You hit a creature.
#[derive(Message, Debug, Clone, Copy)]
pub struct Strike {
    pub entity: Entity,
    pub damage: f32,
    /// Which way the blow pushes it.
    pub dir: Vec3,
}

/// Right click on a creature (villagers talk).
#[derive(Message, Debug, Clone, Copy)]
pub struct UseCreature(pub Entity);

/// Seconds between blows.
pub const STRIKE_COOLDOWN: f32 = 0.45;

/// Where a ray enters a box, if it does (distance along the unit `dir`).
pub fn ray_box(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = dir.recip();
    let t1 = (min - origin) * inv;
    let t2 = (max - origin) * inv;
    let near = t1.min(t2).max_element();
    let far = t1.max(t2).min_element();
    (far >= near.max(0.0)).then_some(near.max(0.0))
}

/// What each hand holds: an index into the hotbar, or nothing. Only the
/// right hand is used for now; `left` stays `None` until the off-hand slot.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Hands {
    pub right: Option<usize>,
    pub left: Option<usize>,
}

impl Hands {
    pub fn slot(&self, hand: Hand) -> Option<usize> {
        match hand {
            Hand::Left => self.left,
            Hand::Right => self.right,
        }
    }

    pub fn item(&self, hand: Hand, hotbar: &Hotbar) -> Option<ItemId> {
        self.slot(hand).and_then(|s| hotbar.slots.get(s).copied().flatten())
    }
}

#[derive(Resource, Default)]
pub struct Target {
    pub hit: Option<voxel::raycast::RayHit>,
}

/// The block being mined, and how far along (0..1).
#[derive(Resource, Default, Debug)]
pub struct Mining {
    pub block: Option<BlockPos>,
    pub hand: Hand,
    pub progress: f32,
}

/// One arm's swing.
#[derive(Debug, Default, Clone, Copy)]
pub struct Arm {
    /// How far through the swing, 0..1; `None` at rest.
    pub t: Option<f32>,
    /// The style of the swing under way (what was held when it started).
    pub style: Swing,
    /// A mining swing: it lands a blow on the block in front at contact.
    mining: bool,
}

impl Arm {
    /// Starts a swing unless one is under way.
    fn start(&mut self, style: Swing, mining: bool) {
        if self.t.is_none() {
            *self = Arm { t: Some(0.0), style, mining };
        }
    }

    /// Moves the swing on by `dt` seconds; true on the frame a mining swing lands.
    fn advance(&mut self, dt: f32) -> bool {
        let Some(t) = self.t else { return false };
        let next = t + dt / self.style.period();
        self.t = (next < 1.0).then_some(next);
        self.mining && swing::lands(self.style, t, next)
    }
}

/// Both arms' swings.
#[derive(Resource, Default, Debug)]
pub struct HandAnim {
    pub left: Arm,
    pub right: Arm,
    /// Eating progress, 0..1 (set by `survival`): the hand comes up to the mouth.
    pub eat: f32,
    place_cooldown: f32,
    strike_cooldown: f32,
}

impl HandAnim {
    pub fn arm(&self, hand: Hand) -> &Arm {
        match hand {
            Hand::Left => &self.left,
            Hand::Right => &self.right,
        }
    }

    fn arm_mut(&mut self, hand: Hand) -> &mut Arm {
        match hand {
            Hand::Left => &mut self.left,
            Hand::Right => &mut self.right,
        }
    }

    fn start(&mut self, hand: Hand, style: Swing) {
        self.arm_mut(hand).start(style, false);
    }
}

fn choose_items(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    mode: Res<MoveMode>,
    menus: Res<crate::camera::Menus>,
    mut hands: ResMut<Hands>,
) {
    if menus.open {
        return;
    }
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5,
        KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
    ];
    let n = SLOTS;
    for (i, k) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*k) && hands.right != Some(i) {
            hands.right = Some(i);
        }
    }
    // The wheel steps through the hotbar on foot (when flying it sets speed).
    if *mode == MoveMode::Walk && scroll.delta.y != 0.0 && n > 0 {
        let step = if scroll.delta.y > 0.0 { n - 1 } else { 1 };
        hands.right = Some((hands.right.unwrap_or(0) + step) % n);
    }
}

/// The selected hotbar item, if any is left in the inventory.
fn wield(hands: Res<Hands>, hotbar: Res<Hotbar>, inv: Res<Inventory>, mut wielded: ResMut<Wielded>) {
    let w = Wielded(hands.item(Hand::Right, &hotbar).filter(|i| inv.count(*i) > 0));
    if *wielded != w {
        *wielded = w;
    }
}

fn find_target(
    world: Res<World>,
    blocks: Res<Blocks>,
    mode: Res<MoveMode>,
    cam: Single<&Transform, With<FlyCamera>>,
    mut target: ResMut<Target>,
) {
    let origin = cam.translation.to_array();
    let dir = cam.forward().as_vec3().to_array();
    let reach = if *mode == MoveMode::Fly { FLY_REACH } else { REACH };
    // Aim through air and water; stop at anything solid, or a torch.
    target.hit = raycast(origin, dir, reach, |p| {
        world.block(p).is_some_and(|b| {
            let d = blocks.registry.get(b);
            d.solid || d.shape != voxel::Shape::Cube
        })
    });
}

/// Remesh the chunk holding `p` and any neighbour sharing a face, right away.
fn remesh_around(world: &mut World, blocks: &Blocks, p: BlockPos) {
    let c = p.chunk();
    for d in [[0, 0, 0], [1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]] {
        let n = ChunkPos::new(c.x + d[0], c.y + d[1], c.z + d[2]);
        if world.needs_mesh(n) {
            world.mesh_now(n, &blocks.registry, &blocks.opaque);
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn use_hands(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    target: Res<Target>,
    wielded: Res<Wielded>,
    items: Res<Items>,
    mut inv: ResMut<Inventory>,
    (mut mined, mut blows, mut open_chest, mut placed, mut strikes, mut use_creature): (
        MessageWriter<Mined>,
        MessageWriter<Blow>,
        MessageWriter<OpenChest>,
        MessageWriter<Placed>,
        MessageWriter<Strike>,
        MessageWriter<UseCreature>,
    ),
    (aimed, cam, perks): (Res<Aimed>, Single<&Transform, With<FlyCamera>>, Res<crate::progress::skills::Perks>),
    blocks: Res<Blocks>,
    mode: Res<MoveMode>,
    player: Res<Player>,
    body: Res<crate::player::Body>,
    mut mining: ResMut<Mining>,
    mut anim: ResMut<HandAnim>,
    mut world: ResMut<World>,
) {
    let dt = time.delta_secs();
    anim.place_cooldown -= dt;
    anim.strike_cooldown -= dt;
    let hand = Hand::Right;
    let landed = anim.arm_mut(hand).advance(dt);
    anim.left.advance(dt);
    // Clicks only act while the mouse is captured (the first click captures it).
    if cursor.grab_mode == CursorGrabMode::None || body.frozen {
        mining.block = None;
        return;
    }
    let item = wielded.0;
    let kind = item.map(|i| &items.get(i).kind);
    let style = item.map_or(Swing::Jab, |i| items.get(i).swing);

    // A creature in front of the block you aim at takes the click.
    let block_dist = target.hit.and_then(|h| {
        let min = Vec3::new(h.block.x as f32, h.block.y as f32, h.block.z as f32);
        ray_box(cam.translation, cam.forward().as_vec3(), min, min + Vec3::ONE)
    });
    if let Some(e) = aimed.entity
        && block_dist.is_none_or(|d| aimed.dist < d)
    {
        mining.block = None;
        if mouse.just_pressed(MouseButton::Right) {
            use_creature.write(UseCreature(e));
        }
        if mouse.pressed(MouseButton::Left) && anim.strike_cooldown <= 0.0 {
            anim.start(hand, style);
            anim.strike_cooldown = STRIKE_COOLDOWN;
            let damage = item.map_or(crate::items::FIST_DAMAGE, |i| items.get(i).damage) * (1.0 + perks.damage);
            let mut dir = cam.forward().as_vec3();
            dir.y = 0.0;
            strikes.write(Strike { entity: e, damage, dir: dir.normalize_or_zero() });
        }
        return;
    }

    // Right click on a chest opens it (whatever you hold).
    let chest = blocks.registry.id("chest");
    if mouse.just_pressed(MouseButton::Right)
        && let Some(hit) = target.hit
        && chest.is_some()
        && world.block(hit.block) == chest
    {
        open_chest.write(OpenChest(hit.block));
        return;
    }

    // Right click: place the block or torch in your hand.
    let place = match kind {
        Some(ItemKind::Block(b)) => Some(*b),
        Some(ItemKind::Torch) => blocks.registry.id("torch"),
        _ => None,
    };
    if let (Some(place), Some(hit)) = (place, target.hit)
        && mouse.pressed(MouseButton::Right)
        && (mouse.just_pressed(MouseButton::Right) || anim.place_cooldown <= 0.0)
    {
        // Aiming at a plant puts the block in its place.
        let is_plant = |p: BlockPos| world.block(p).is_some_and(|b| blocks.registry.get(b).shape == voxel::Shape::Plant);
        let into = if is_plant(hit.block) { hit.block } else { hit.before };
        let free = world.block(into).is_some_and(|b| !blocks.registry.get(b).solid && blocks.registry.get(b).shape != voxel::Shape::Torch);
        // Never inside yourself (solid blocks only; a torch can go at your feet).
        let blocks_player = blocks.registry.get(place).solid && player.overlaps(into);
        if free && !blocks_player && item.is_some_and(|i| inv.take(i, 1)) && world.set_block(into, place) {
            placed.write(Placed { pos: into, block: place });
            remesh_around(&mut world, &blocks, into);
            anim.start(hand, style);
            anim.place_cooldown = PLACE_REPEAT;
        }
    }

    // Left click: swing at the block in front. Each swing lands one blow
    // when the tool meets the block, and the blows add up to break it.
    let breakable = |world: &World| {
        let hit = target.hit?;
        let b = world.block(hit.block)?;
        let seconds = items.break_time(item, blocks.registry.get(b))? / (1.0 + perks.mining);
        Some((hit.block, b, seconds))
    };
    let aim = breakable(&world);
    let pressed = mouse.pressed(MouseButton::Left);
    // A blow lands on whatever is in front when the swing connects (even if
    // you let go of the button partway through the swing).
    let mut broke = None;
    if landed && let Some((pos, b, seconds)) = aim {
        if mining.block != Some(pos) {
            *mining = Mining { block: Some(pos), hand, progress: 0.0 };
        }
        mining.progress += swing::blow_share(anim.arm(hand).style, seconds);
        blows.write(Blow { pos, block: b });
        if mining.progress >= 1.0 - 1e-4 {
            broke = Some((pos, b));
        }
    }
    if pressed {
        match aim {
            // Free flight builds: blocks go at once, one per click.
            Some((pos, b, seconds)) if *mode == MoveMode::Fly || seconds <= 0.0 => {
                anim.start(hand, style);
                if mouse.just_pressed(MouseButton::Left) {
                    broke = Some((pos, b));
                }
            }
            Some((pos, _, _)) => {
                if mining.block != Some(pos) {
                    *mining = Mining { block: Some(pos), hand, progress: 0.0 };
                }
                anim.arm_mut(hand).start(style, true);
            }
            None => {
                mining.block = None;
                anim.start(hand, style);
            }
        }
    } else if anim.arm(hand).t.is_none() {
        // Let go and the swing is over: the cracks heal.
        mining.block = None;
        mining.progress = 0.0;
    }
    if let Some((pos, b)) = broke
        && world.set_block(pos, BlockId::AIR)
    {
        mined.write(Mined { pos, block: b });
        // A plant standing on the block goes with it.
        let above = BlockPos::new(pos.x, pos.y + 1, pos.z);
        if let Some(p) = world.block(above).filter(|p| blocks.registry.get(*p).shape == voxel::Shape::Plant)
            && world.set_block(above, BlockId::AIR)
        {
            mined.write(Mined { pos: above, block: p });
            remesh_around(&mut world, &blocks, above);
        }
        remesh_around(&mut world, &blocks, pos);
        mining.block = None;
        mining.progress = 0.0;
    }
}

fn draw_target(target: Res<Target>, mining: Res<Mining>, mut gizmos: Gizmos) {
    if let Some(hit) = target.hit {
        let p = hit.block;
        let center = Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5);
        gizmos.cube(Transform::from_translation(center).with_scale(Vec3::splat(1.003)), Color::srgba(0.05, 0.05, 0.05, 0.9));
        // Cracks close in as the block is mined: a shrinking inner box.
        if mining.block == Some(p) && mining.progress > 0.0 {
            let s = 1.0 - mining.progress.min(1.0) * 0.9;
            gizmos.cube(Transform::from_translation(center).with_scale(Vec3::splat(s)), Color::srgba(0.9, 0.85, 0.7, 0.8));
        }
    }
}
