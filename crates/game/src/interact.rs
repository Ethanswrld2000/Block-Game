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
//! Two hands: the right holds the selected hotbar item, or the weapon from
//! the weapon slot while it is drawn (R draws it and puts it away; 1-9 and
//! the wheel put it away too). The left hand holds whatever is in the
//! inventory's left-hand slot. Right click uses the right hand's item if it
//! has a right-click use, else the left hand's (place a block or torch, eat):
//! `right_click_hand`.

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
            .init_resource::<OffHand>()
            .init_resource::<Aimed>()
            // `BLOCKGAME_SLOT=n` starts with hotbar slot n in hand (for screenshots).
            .insert_resource(Hands {
                right: Some(std::env::var("BLOCKGAME_SLOT").ok().and_then(|s| s.parse::<usize>().ok()).map_or(0, |n| n.clamp(1, SLOTS) - 1)),
                drawn: std::env::var_os("BLOCKGAME_DRAWN").is_some(),
            })
            .insert_resource(Hotbar { slots: vec![None; SLOTS] })
            .add_message::<Mined>()
            .add_message::<Blow>()
            .add_message::<OpenChest>()
            .add_message::<Placed>()
            .add_message::<Strike>()
            .add_message::<Swung>()
            .add_systems(
                crate::menu::GameUpdate,
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

/// What the right hand actually holds: the drawn weapon, or the selected
/// slot's item if you have any.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct Wielded(pub Option<ItemId>);

/// What the left hand holds (the inventory's left-hand slot).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct OffHand(pub Option<ItemId>);

/// Does a right click do something with this item in hand? (Place a block
/// or torch, eat, raise a sword to block.)
pub fn right_click_use(items: &Items, item: Option<ItemId>) -> bool {
    item.is_some_and(|i| {
        let it = items.get(i);
        matches!(it.kind, ItemKind::Block(_) | ItemKind::Torch | ItemKind::Food { .. }) || it.swing == Swing::Slash
    })
}

/// Which hand a right click uses: the right if its item has a use, else the
/// left if its item has one (Minecraft's off-hand rule).
pub fn right_click_hand(items: &Items, right: Option<ItemId>, left: Option<ItemId>) -> Hand {
    if !right_click_use(items, right) && right_click_use(items, left) { Hand::Left } else { Hand::Right }
}

/// Is either hand holding a torch?
pub fn holding_torch(items: &Items, wielded: &Wielded, off: &OffHand) -> bool {
    [wielded.0, off.0].into_iter().flatten().any(|i| matches!(items.get(i).kind, ItemKind::Torch))
}

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

/// A fighting swing (a sword's) reached its contact moment: whatever is in
/// the arc in front of `origin` along `dir` within `reach` takes the blow
/// (`combat` resolves it against foes and creatures).
#[derive(Message, Debug, Clone, Copy)]
pub struct Swung {
    pub origin: Vec3,
    pub dir: Vec3,
    pub reach: f32,
    pub damage: f32,
}

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

/// The right hand: an index into the hotbar, and whether the weapon slot's
/// weapon is drawn instead. (The left hand is the inventory's `left` slot.)
#[derive(Resource, Debug, Clone, Copy)]
pub struct Hands {
    pub right: Option<usize>,
    pub drawn: bool,
}

impl Hands {
    /// The selected hotbar slot's item.
    pub fn hotbar_item(&self, hotbar: &Hotbar) -> Option<ItemId> {
        self.right.and_then(|s| hotbar.slots.get(s).copied().flatten())
    }
}

/// Key that draws the weapon and puts it away.
pub const DRAW_KEY: KeyCode = KeyCode::KeyR;

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
    /// A fighting swing: it sends a `Swung` at contact.
    striking: bool,
}

impl Arm {
    /// Starts a swing unless one is under way.
    fn start(&mut self, style: Swing, mining: bool) {
        if self.t.is_none() {
            *self = Arm { t: Some(0.0), style, mining, striking: false };
        }
    }

    /// Moves the swing on by `dt` seconds; true on the frame a mining or
    /// fighting swing lands.
    fn advance(&mut self, dt: f32) -> bool {
        let Some(t) = self.t else { return false };
        let next = t + dt / self.style.period();
        self.t = (next < 1.0).then_some(next);
        (self.mining || self.striking) && swing::lands(self.style, t, next)
    }
}

/// Both arms' swings.
#[derive(Resource, Default, Debug)]
pub struct HandAnim {
    pub left: Arm,
    pub right: Arm,
    /// Eating progress, 0..1 (set by `survival`): the hand comes up to the mouth.
    pub eat: f32,
    /// Which hand is eating.
    pub eat_hand: Hand,
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

    /// Starts a plain swing of `hand` (no blow) unless one is under way.
    pub fn start(&mut self, hand: Hand, style: Swing) {
        self.arm_mut(hand).start(style, false);
    }
}

fn choose_items(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    mode: Res<MoveMode>,
    menus: Res<crate::camera::Menus>,
    inv: Res<Inventory>,
    mut hands: ResMut<Hands>,
    mut sound: MessageWriter<crate::sound::PlaySound>,
) {
    if menus.open {
        return;
    }
    // R draws the weapon from the weapon slot, and puts it away again.
    if keys.just_pressed(DRAW_KEY) && (hands.drawn || inv.weapon.is_some()) {
        hands.drawn = !hands.drawn;
        sound.write(crate::sound::PlaySound::new(if hands.drawn { "ui_draw" } else { "ui_sheathe" }));
    }
    if hands.drawn && inv.weapon.is_none() {
        hands.drawn = false;
    }
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5,
        KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
    ];
    let n = SLOTS;
    for (i, k) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*k) {
            // Picking a hotbar slot puts the weapon away.
            hands.drawn = false;
            if hands.right != Some(i) {
                hands.right = Some(i);
            }
        }
    }
    // The wheel steps through the hotbar on foot (when flying it sets speed).
    if *mode == MoveMode::Walk && scroll.delta.y != 0.0 && n > 0 {
        let step = if scroll.delta.y > 0.0 { n - 1 } else { 1 };
        hands.right = Some((hands.right.unwrap_or(0) + step) % n);
        hands.drawn = false;
    }
}

/// The right hand: the drawn weapon, or the selected hotbar item if any is
/// left in the inventory. The left hand: the left-hand slot.
fn wield(hands: Res<Hands>, hotbar: Res<Hotbar>, inv: Res<Inventory>, mut wielded: ResMut<Wielded>, mut off: ResMut<OffHand>) {
    let w = Wielded(match (hands.drawn, inv.weapon) {
        (true, Some(weapon)) => Some(weapon),
        _ => hands.hotbar_item(&hotbar).filter(|i| inv.count(*i) > 0),
    });
    if *wielded != w {
        *wielded = w;
    }
    let o = OffHand(inv.left_item());
    if *off != o {
        *off = o;
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

/// Break the block at `p` (it drops as usual), then anything that was held up
/// by it: plants and pebbles on top, twigs and vines on its sides, and vines
/// hanging below those. Returns false if nothing was there to break.
fn break_block(world: &mut World, blocks: &Blocks, mined: &mut MessageWriter<Mined>, p: BlockPos) -> bool {
    let Some(b) = world.block(p).filter(|b| !b.is_air()) else { return false };
    if !world.set_block(p, BlockId::AIR) {
        return false;
    }
    mined.write(Mined { pos: p, block: b });
    remesh_around(world, blocks, p);
    let mut check: Vec<BlockPos> = neighbours(p).collect();
    // A long vine can drop a strand at a time; keep it bounded.
    let mut budget = 256;
    while let Some(q) = check.pop() {
        budget -= 1;
        if budget == 0 {
            break;
        }
        let Some(qb) = world.block(q).filter(|b| !b.is_air()) else { continue };
        let def = blocks.registry.get(qb);
        let supports = def.supports();
        if supports.is_empty() || supports.iter().any(|o| holds(world, blocks, def, q, *o)) {
            continue;
        }
        if world.set_block(q, BlockId::AIR) {
            mined.write(Mined { pos: q, block: qb });
            remesh_around(world, blocks, q);
            check.extend(neighbours(q));
        }
    }
    true
}

fn neighbours(p: BlockPos) -> impl Iterator<Item = BlockPos> {
    [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]].into_iter().map(move |d| BlockPos::new(p.x + d[0], p.y + d[1], p.z + d[2]))
}

/// Does the block at offset `o` from `p` hold up `def`? The ground under a
/// plant must be solid; a vine can also hang from the vine above; anything
/// else clings to any block that isn't itself a small detail or a torch.
fn holds(world: &World, blocks: &Blocks, def: &voxel::BlockDef, p: BlockPos, o: [i32; 3]) -> bool {
    let Some(n) = world.block(BlockPos::new(p.x + o[0], p.y + o[1], p.z + o[2])) else { return true };
    if n.is_air() {
        return false;
    }
    let nd = blocks.registry.get(n);
    match def.shape {
        voxel::Shape::Vine if o == [0, 1, 0] => nd.shape == voxel::Shape::Vine,
        voxel::Shape::Plant | voxel::Shape::Pebble => nd.solid,
        _ => !nd.shape.is_detail() && nd.shape != voxel::Shape::Torch && !nd.translucent,
    }
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
    (wielded, off): (Res<Wielded>, Res<OffHand>),
    items: Res<Items>,
    mut inv: ResMut<Inventory>,
    (mut mined, mut blows, mut chops, mut open_chest, mut placed, mut strikes, mut toast): (
        MessageWriter<Mined>,
        MessageWriter<Blow>,
        MessageWriter<crate::chop::Chop>,
        MessageWriter<OpenChest>,
        MessageWriter<Placed>,
        MessageWriter<Strike>,
        MessageWriter<crate::survival::Toast>,
    ),
    (aimed, cam, perks, station, edges, ship, guard, mut swung, generator): (
        Res<Aimed>,
        Single<&Transform, With<FlyCamera>>,
        Res<crate::progress::skills::Perks>,
        Res<crate::station::StationAim>,
        Option<Res<crate::station::Edges>>,
        Res<crate::airship::ShipAim>,
        Res<crate::combat::Guard>,
        MessageWriter<Swung>,
        Option<Res<crate::world::Generator>>,
    ),
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
    let striking = anim.arm(hand).striking;
    let landed = anim.arm_mut(hand).advance(dt);
    let item = wielded.0;
    // A sword's cut lands at contact, on whatever is in its arc then.
    if landed && striking {
        let damage = item.map_or(crate::items::FIST_DAMAGE, |i| items.get(i).damage) * (1.0 + perks.damage);
        swung.write(Swung { origin: cam.translation, dir: cam.forward().as_vec3(), reach: crate::combat::SWORD_REACH, damage });
    }
    let landed = landed && !striking;
    anim.left.advance(dt);
    // Clicks only act while the mouse is captured (the first click captures it).
    if cursor.grab_mode == CursorGrabMode::None || body.frozen {
        mining.block = None;
        return;
    }
    let style = item.map_or(Swing::Jab, |i| items.get(i).swing);

    // A creature in front of the block you aim at takes the click.
    let block_dist = target.hit.and_then(|h| {
        let min = Vec3::new(h.block.x as f32, h.block.y as f32, h.block.z as f32);
        ray_box(cam.translation, cam.forward().as_vec3(), min, min + Vec3::ONE)
    });
    if let Some(e) = aimed.entity
        && style != Swing::Slash
        && block_dist.is_none_or(|d| aimed.dist < d)
    {
        mining.block = None;
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

    // Named places (dungeons, barrows, villages) are under a spirit ward:
    // nothing in them can be broken or built.
    let warded = |p: BlockPos| generator.as_ref().is_some_and(|g| g.0.warded(p.x, p.y, p.z));
    let ward_note = |toast: &mut MessageWriter<crate::survival::Toast>| toast.write(crate::survival::Toast("A spirit ward holds this place".into()));

    // A station nearer than the block you aim at takes right clicks (`station`).
    // So does an airship (`airship` boards it).
    let at_station = station.0.is_some_and(|(_, d)| block_dist.is_none_or(|b| d < b)) || ship.0.is_some_and(|(_, d)| block_dist.is_none_or(|b| d < b));

    // Right click on a chest opens it (whatever you hold).
    let chest = blocks.registry.id("chest");
    if mouse.just_pressed(MouseButton::Right)
        && !at_station
        && let Some(hit) = target.hit
        && chest.is_some()
        && world.block(hit.block) == chest
    {
        open_chest.write(OpenChest(hit.block));
        return;
    }

    // Right click on a twig or a pebble: snap it off or pick it up.
    if mouse.just_pressed(MouseButton::Right)
        && !at_station
        && let Some(hit) = target.hit
        && world.block(hit.block).is_some_and(|b| matches!(blocks.registry.get(b).shape, voxel::Shape::Twig | voxel::Shape::Pebble))
    {
        if warded(hit.block) {
            ward_note(&mut toast);
            return;
        }
        anim.start(hand, style);
        break_block(&mut world, &blocks, &mut mined, hit.block);
        return;
    }

    // A sword fights and never mines: left click cuts (one cut per swing,
    // landing at contact), right click blocks (`combat::Guard`).
    if style == Swing::Slash {
        mining.block = None;
        if mouse.pressed(MouseButton::Left) && !guard.up && anim.arm(hand).t.is_none() {
            let arm = anim.arm_mut(hand);
            arm.start(Swing::Slash, false);
            arm.striking = true;
        }
        return;
    }

    // Right click: place the block or torch in your hand (the left hand's
    // when the right hand's item has no right-click use).
    let place_hand = right_click_hand(&items, item, off.0);
    let place_item = if place_hand == Hand::Left { off.0 } else { item };
    let place = match place_item.map(|i| &items.get(i).kind) {
        Some(ItemKind::Block(b)) => Some(*b),
        Some(ItemKind::Torch) => blocks.registry.id("torch"),
        _ => None,
    };
    let use_up = |inv: &mut Inventory| match place_hand {
        Hand::Left => inv.use_left(1),
        Hand::Right => place_item.is_some_and(|i| inv.take(i, 1)),
    };
    if let (Some(place), Some(hit)) = (place, target.hit)
        && mouse.pressed(MouseButton::Right)
        && !at_station
        && (mouse.just_pressed(MouseButton::Right) || anim.place_cooldown <= 0.0)
    {
        // Aiming at a plant puts the block in its place.
        let is_plant = |p: BlockPos| world.block(p).is_some_and(|b| blocks.registry.get(b).shape.is_detail());
        let into = if is_plant(hit.block) { hit.block } else { hit.before };
        let free = world.block(into).is_some_and(|b| !blocks.registry.get(b).solid && blocks.registry.get(b).shape != voxel::Shape::Torch);
        // Never inside yourself (solid blocks only; a torch can go at your feet).
        let blocks_player = blocks.registry.get(place).solid && player.overlaps(into);
        // Nothing can be built at or above the world's ceiling.
        let under_ceiling = into.y < worldgen::WORLD_CEILING;
        if free && mouse.just_pressed(MouseButton::Right) && warded(into) {
            ward_note(&mut toast);
        }
        if free && !warded(into) && !blocks_player && under_ceiling && use_up(&mut inv) && world.set_block(into, place) {
            placed.write(Placed { pos: into, block: place });
            remesh_around(&mut world, &blocks, into);
            anim.start(place_hand, place_item.map_or(Swing::Jab, |i| items.get(i).swing));
            anim.place_cooldown = PLACE_REPEAT;
        }
    }

    // Left click: swing at the block in front. Each swing lands one blow
    // when the tool meets the block, and the blows add up to break it.
    let breakable = |world: &World| {
        let hit = target.hit?;
        if warded(hit.block) {
            return None;
        }
        let b = world.block(hit.block)?;
        // A tool sharpened at the sharpening stone cuts faster for a while.
        let sharp = edges.as_ref().map_or(1.0, |e| e.speed(&items, item));
        let seconds = items.break_time(item, blocks.registry.get(b))? / (1.0 + perks.mining) / sharp;
        Some((hit.block, b, seconds))
    };
    let aim = breakable(&world);
    let pressed = mouse.pressed(MouseButton::Left);
    if mouse.just_pressed(MouseButton::Left) && aim.is_none() && target.hit.is_some_and(|h| warded(h.block)) {
        ward_note(&mut toast);
    }
    // A blow lands on whatever is in front when the swing connects (even if
    // you let go of the button partway through the swing).
    let mut broke = None;
    if landed && let Some((pos, b, seconds)) = aim {
        if mining.block != Some(pos) {
            *mining = Mining { block: Some(pos), hand, progress: 0.0 };
        }
        mining.progress += swing::blow_share(anim.arm(hand).style, seconds);
        blows.write(Blow { pos, block: b });
        // An axe blow on a log shakes the tree.
        if anim.arm(hand).style == Swing::Chop && blocks.registry.get(b).material == "log" {
            chops.write(crate::chop::Chop(pos));
        }
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
    if let Some((pos, _)) = broke
        && break_block(&mut world, &blocks, &mut mined, pos)
    {
        mining.block = None;
        mining.progress = 0.0;
    }
}

fn draw_target(target: Res<Target>, mining: Res<Mining>, menus: Res<crate::camera::Menus>, mut gizmos: Gizmos) {
    if let Some(hit) = target.hit.filter(|_| !menus.open) {
        let p = hit.block;
        let center = Vec3::new(p.x as f32 + 0.5, p.y as f32 + 0.5, p.z as f32 + 0.5);
        gizmos.cube(Transform::from_translation(center).with_scale(Vec3::splat(1.003)), Color::srgba(0.05, 0.05, 0.05, 0.9));
        // Cracks close in as the block is mined: a shrinking square on each face,
        // drawn just outside the surface so it shows over the block.
        if mining.block == Some(p) && mining.progress > 0.0 {
            let s = 1.0 - mining.progress.min(1.0) * 0.9;
            let color = Color::srgba(0.9, 0.85, 0.7, 0.8);
            for normal in [Dir3::X, Dir3::NEG_X, Dir3::Y, Dir3::NEG_Y, Dir3::Z, Dir3::NEG_Z] {
                let at = center + *normal * 0.505;
                let turn = Quat::from_rotation_arc(Vec3::Z, *normal);
                gizmos.rect(Isometry3d::new(at, turn), Vec2::splat(s), color);
            }
        }
    }
}
