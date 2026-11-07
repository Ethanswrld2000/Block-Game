//! Using what is in your hand: mining, placing blocks and torches.
//!
//! One hand for now: the hotbar's nine slots are shortcuts to things in your
//! inventory (bind them in the inventory screen). Keys 1-9 or the mouse wheel
//! pick one. Placing a block or torch uses one up; mining sends a `Mined`
//! message, which the inventory turns into drops. Hold left click to mine (how long depends on the block's hardness
//! and your tool, see items.ron); right click places the block or torch you
//! hold. In free-fly mode blocks break instantly.
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
use crate::world::World;

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>()
            .init_resource::<Mining>()
            .init_resource::<HandAnim>()
            .init_resource::<Wielded>()
            .insert_resource(Hands { right: Some(0), left: None })
            .insert_resource(Hotbar { slots: vec![None; SLOTS] })
            .add_message::<Mined>()
            .add_message::<OpenChest>()
            .add_systems(
                Update,
                (choose_items, wield, find_target, use_hands, animate_swings, draw_target)
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
/// Seconds one swing of the arm takes.
pub const SWING_TIME: f32 = 0.32;

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

/// Right click on a chest.
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenChest(pub BlockPos);

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

/// Arm swings, 0 = at rest, counting down from 1 during a swing.
#[derive(Resource, Default, Debug)]
pub struct HandAnim {
    pub left: f32,
    pub right: f32,
    /// Eating progress, 0..1 (set by `survival`): the hand comes up to the mouth.
    pub eat: f32,
    place_cooldown: f32,
}

impl HandAnim {
    fn start(&mut self, hand: Hand) {
        let s = match hand {
            Hand::Left => &mut self.left,
            Hand::Right => &mut self.right,
        };
        if *s <= 0.0 {
            *s = 1.0;
        }
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

#[allow(clippy::too_many_arguments)]
fn use_hands(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Single<&CursorOptions, With<PrimaryWindow>>,
    target: Res<Target>,
    wielded: Res<Wielded>,
    items: Res<Items>,
    mut inv: ResMut<Inventory>,
    mut mined: MessageWriter<Mined>,
    mut open_chest: MessageWriter<OpenChest>,
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
    // Clicks only act while the mouse is captured (the first click captures it).
    if cursor.grab_mode == CursorGrabMode::None || body.frozen {
        mining.block = None;
        return;
    }
    let mut mined_this_frame = false;
    let hand = Hand::Right;
    let item = wielded.0;
    let kind = item.map(|i| &items.get(i).kind);

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
        let into = hit.before;
        let free = world.block(into).is_some_and(|b| !blocks.registry.get(b).solid && blocks.registry.get(b).shape == voxel::Shape::Cube);
        // Never inside yourself (solid blocks only; a torch can go at your feet).
        let blocks_player = blocks.registry.get(place).solid && player.overlaps(into);
        if free && !blocks_player && item.is_some_and(|i| inv.take(i, 1)) && world.set_block(into, place) {
            remesh_around(&mut world, &blocks, into);
            anim.start(hand);
            anim.place_cooldown = PLACE_REPEAT;
        }
    }

    // Left click: mine with whatever you hold (tools are faster).
    if mouse.pressed(MouseButton::Left) {
        mined_this_frame = true;
        anim.start(hand);
        if let Some(hit) = target.hit
            && let Some(b) = world.block(hit.block)
            && let Some(seconds) = items.break_time(item, blocks.registry.get(b))
        {
            if mining.block != Some(hit.block) {
                *mining = Mining { block: Some(hit.block), hand, progress: 0.0 };
            }
            let instant = *mode == MoveMode::Fly || seconds <= 0.0;
            if instant {
                if mouse.just_pressed(MouseButton::Left) {
                    mining.progress = 1.0;
                }
            } else {
                mining.progress += dt / seconds;
            }
            if mining.progress >= 1.0 && world.set_block(hit.block, BlockId::AIR) {
                mined.write(Mined { pos: hit.block, block: b });
                remesh_around(&mut world, &blocks, hit.block);
                mining.block = None;
                mining.progress = 0.0;
            }
        } else {
            mining.block = None;
        }
    }
    if !mined_this_frame {
        mining.block = None;
        mining.progress = 0.0;
    }
}

fn animate_swings(time: Res<Time>, mut anim: ResMut<HandAnim>) {
    let step = time.delta_secs() / SWING_TIME;
    anim.left = (anim.left - step).max(0.0);
    anim.right = (anim.right - step).max(0.0);
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
