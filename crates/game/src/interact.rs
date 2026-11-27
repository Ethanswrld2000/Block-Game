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
        matches!(it.kind, ItemKind::Block(_) | ItemKind::Torch | ItemKind::Food { .. }) || it.moves.is_some() || it.shield.is_some()
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
    /// Came down with a felled tree (`chop`): it doesn't fell anything itself.
    pub felled: bool,
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
    /// Your blow (not the golem's): it gets the hit feel.
    pub yours: bool,
}

/// A fighting swing (a weapon's move) reached its contact moment: whatever
/// is in the arc in front of `origin` along `dir` within `reach` takes the
/// blow (`combat` resolves it against foes and creatures).
#[derive(Message, Debug, Clone, Copy)]
pub struct Swung {
    pub origin: Vec3,
    pub dir: Vec3,
    pub reach: f32,
    /// Half the arc it sweeps, degrees.
    pub arc: f32,
    pub damage: f32,
    /// A charged move: it breaks a raised guard.
    pub heavy: bool,
    /// The move (a `Spin` hits all round, a `Slam` knocks down all close by).
    pub style: Swing,
    /// What it was swung with (for its special and traits).
    pub weapon: Option<ItemId>,
}

/// Holding the attack button with a sword: up to this long it's a light cut
/// (on release), longer charges a heavy one.
pub const TAP: f32 = 0.22;
/// Seconds for the arm to come all the way up while charging.
const RAISE: f32 = 0.3;
/// Seconds more of charging, after `TAP`, for a full-power heavy cut.
pub const CHARGE: f32 = 0.7;
/// After a light cut, a click within this long carries the combo on.
pub const COMBO_WINDOW: f32 = 0.45;

/// The damage multiplier of a heavy cut charged `power` (0..1).
pub fn heavy_mult(power: f32) -> f32 {
    1.6 + 0.8 * power.clamp(0.0, 1.0)
}

/// The next light move of a combo after `last`, and its damage multiplier:
/// the weapon's light moves in turn (a sword: right to left, back across),
/// the third of three hitting harder.
pub fn combo_cut(light: &[Swing], last: u8, carried: bool) -> (u8, Swing, f32) {
    let n = light.len().max(1) as u8;
    let step = if carried { (last + 1) % n } else { 0 };
    let style = light.get(step as usize).copied().unwrap_or(Swing::Slash);
    (step, style, if step == 2 { 1.25 } else { 1.0 })
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
    /// A charged (heavy) cut.
    pub heavy: bool,
    /// Damage multiplier of this cut (combo finisher, charge).
    mult: f32,
    /// How much faster than usual it swings (a swift weapon's trait).
    speed: f32,
}

impl Arm {
    /// Starts a swing unless one is under way.
    fn start(&mut self, style: Swing, mining: bool) {
        if self.t.is_none() {
            *self = Arm { t: Some(0.0), style, mining, striking: false, heavy: false, mult: 1.0, speed: 1.0 };
        }
    }

    /// A fighting move (it hits foes at contact).
    pub fn striking(&self) -> bool {
        self.striking && self.t.is_some()
    }

    /// A fighting move from `t` (0..1).
    fn fight(t: f32, style: Swing, heavy: bool, mult: f32, speed: f32) -> Self {
        Arm { t: Some(t), style, mining: false, striking: true, heavy, mult, speed }
    }

    /// Moves the swing on by `dt` seconds; true on the frame a mining or
    /// fighting swing lands.
    fn advance(&mut self, dt: f32) -> bool {
        let Some(t) = self.t else { return false };
        let next = t + dt * self.speed.max(0.1) / self.style.period();
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
    /// Seconds the attack button has been held with a weapon (a charge).
    charge: Option<f32>,
    /// The heavy move being charged.
    charging: Swing,
    /// A light cut waiting for the arm to come free.
    queued: bool,
    /// Which cut of the combo came last (0, 1, 2) and how long is left to
    /// carry it on.
    combo: u8,
    combo_window: f32,
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

    /// The heavy move being charged and how far up (0..1), while one is.
    pub fn charge_level(&self) -> Option<(Swing, f32)> {
        let c = self.charge?;
        (c > TAP && self.right.t.is_none()).then(|| (self.charging, ((c - TAP) / RAISE).min(1.0)))
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
    // Aim through air, water and puddles; stop at anything solid, or a torch.
    target.hit = raycast(origin, dir, reach, |p| {
        world.block(p).is_some_and(|b| {
            let d = blocks.registry.get(b);
            d.solid || (d.shape != voxel::Shape::Cube && !d.translucent)
        })
    });
}

/// Break the block at `p` (it drops as usual), then anything that was held up
/// by it: plants and pebbles on top, twigs and vines on its sides, and vines
/// hanging below those. Returns false if nothing was there to break.
fn break_block(world: &mut World, blocks: &Blocks, mined: &mut MessageWriter<Mined>, p: BlockPos) -> bool {
    let Some(b) = world.block(p).filter(|b| !b.is_air()) else { return false };
    // Digging snow away leaves the plant or pebble that stood in it.
    if !world.set_block(p, blocks.registry.get(b).inside.unwrap_or(BlockId::AIR)) {
        return false;
    }
    mined.write(Mined { pos: p, block: b, felled: false });
    remesh_around(world, blocks, p);
    drop_loose(world, blocks, mined, p);
    true
}

/// Dig the block at `p` down to the lower layer `to` (soil dug by hand); it
/// drops nothing yet, but what stood on it may lose its footing.
fn dig_down(world: &mut World, blocks: &Blocks, mined: &mut MessageWriter<Mined>, p: BlockPos, to: BlockId) -> bool {
    if !world.set_block(p, to) {
        return false;
    }
    remesh_around(world, blocks, p);
    drop_loose(world, blocks, mined, p);
    true
}

/// Break whatever around `p` is no longer held up, and what that held up.
fn drop_loose(world: &mut World, blocks: &Blocks, mined: &mut MessageWriter<Mined>, p: BlockPos) {
    // A long vine can drop a strand at a time; keep it bounded.
    let mut fell = Vec::new();
    drop_unsupported(world, blocks, neighbours(p).collect(), 256, |q, qb| {
        mined.write(Mined { pos: q, block: qb, felled: false });
        fell.push(q);
    });
    for q in fell {
        remesh_around(world, blocks, q);
    }
}

/// Take away every block among `check` (and, as they go, their neighbours)
/// that has lost what held it up (twigs, vines, plants), up to `budget` looks;
/// `gone` hears of each one. Their chunks are left to remesh in the background.
pub(crate) fn drop_unsupported(world: &mut World, blocks: &Blocks, mut check: Vec<BlockPos>, mut budget: usize, mut gone: impl FnMut(BlockPos, BlockId)) {
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
            gone(q, qb);
            check.extend(neighbours(q));
        }
    }
}

pub(crate) fn neighbours(p: BlockPos) -> impl Iterator<Item = BlockPos> {
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

/// What a door block turns into when used: open <-> closed (`model: door` / `door_open`).
pub fn door_swap(registry: &voxel::BlockRegistry, b: BlockId) -> Option<BlockId> {
    let want = match registry.get(b).model? {
        voxel::Model::Door => voxel::Model::DoorOpen,
        voxel::Model::DoorOpen => voxel::Model::Door,
        _ => return None,
    };
    let name = &registry.get(b).name;
    let base = name.strip_suffix("_open").unwrap_or(name);
    let other = if want == voxel::Model::DoorOpen { format!("{base}_open") } else { base.to_string() };
    registry.id(&other).filter(|o| registry.get(*o).model == Some(want))
}

/// Remesh the chunk holding `p`, and a neighbour whose face it touches, right
/// away. Chunks whose light alone changed are remeshed in the background.
pub(crate) fn remesh_around(world: &mut World, blocks: &Blocks, p: BlockPos) {
    let c = p.chunk();
    let l = p.local();
    let last = voxel::CHUNK_SIZE - 1;
    let mut near = vec![[0, 0, 0]];
    for axis in 0..3 {
        let mut d = [0; 3];
        if l[axis] == 0 {
            d[axis] = -1;
        } else if l[axis] == last {
            d[axis] = 1;
        } else {
            continue;
        }
        near.push(d);
    }
    for d in near {
        let n = ChunkPos::new(c.x + d[0], c.y + d[1], c.z + d[2]);
        if world.needs_mesh(n) {
            world.mesh_now(n, blocks);
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
    (mut mined, mut blows, mut chops, mut open_chest, mut placed, mut strikes, mut toast, mut shrine_use): (
        MessageWriter<Mined>,
        MessageWriter<Blow>,
        MessageWriter<crate::chop::Chop>,
        MessageWriter<OpenChest>,
        MessageWriter<Placed>,
        MessageWriter<Strike>,
        MessageWriter<crate::survival::Toast>,
        MessageWriter<crate::shrine::UseShrine>,
    ),
    (aimed, cam, perks, station, edges, ship, guard, mut swung, generator, mut survival): (
        Res<Aimed>,
        Single<&Transform, With<FlyCamera>>,
        Res<crate::progress::skills::Perks>,
        Res<crate::station::StationAim>,
        Option<Res<crate::station::Edges>>,
        Res<crate::airship::ShipAim>,
        Res<crate::combat::Guard>,
        MessageWriter<Swung>,
        Option<Res<crate::world::Generator>>,
        ResMut<crate::survival::Survival>,
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
    let (striking, heavy, mult) = (anim.right.striking, anim.right.heavy, anim.right.mult);
    let landed = anim.arm_mut(hand).advance(dt);
    let item = wielded.0;
    // A sword's cut lands at contact, on whatever is in its arc then.
    if landed && striking {
        let it = item.map(|i| items.get(i));
        let damage = it.map_or(crate::items::FIST_DAMAGE, |i| i.damage) * (1.0 + perks.damage) * mult;
        let reach = it.map_or(crate::combat::SWORD_REACH, |i| i.reach()) + if anim.right.style == Swing::Lunge { 1.0 } else { 0.0 };
        let arc = match anim.right.style {
            Swing::Spin => 180.0,
            _ => it.and_then(|i| i.moves.as_ref()).map_or(55.0, |m| m.arc),
        };
        swung.write(Swung { origin: cam.translation, dir: cam.forward().as_vec3(), reach, arc, damage, heavy, style: anim.right.style, weapon: item });
    }
    let landed = landed && !striking;
    anim.left.advance(dt);
    // Clicks only act while the mouse is captured (the first click captures it).
    if cursor.grab_mode == CursorGrabMode::None || body.frozen {
        mining.block = None;
        anim.charge = None;
        anim.queued = false;
        return;
    }
    let style = item.map_or(Swing::Jab, |i| items.get(i).swing);
    // A shield raised: left click bashes (`combat::shield`), nothing else.
    if guard.up && guard.shield.is_some() {
        mining.block = None;
        anim.charge = None;
        anim.queued = false;
        return;
    }

    // A creature in front of the block you aim at takes the click.
    let block_dist = target.hit.and_then(|h| {
        let min = Vec3::new(h.block.x as f32, h.block.y as f32, h.block.z as f32);
        ray_box(cam.translation, cam.forward().as_vec3(), min, min + Vec3::ONE)
    });
    // A gun fires (`combat::guns`) and never mines or strikes.
    let gun = item.is_some_and(|i| items.get(i).gun.is_some());
    // A weapon with moves fights: a sword (or spear, dagger, hammer) always,
    // a tool with moves (an axe) whenever it isn't aimed at a block it can work.
    let moves = item.and_then(|i| items.get(i).moves.clone());
    let works_blocks = item.is_some_and(|i| matches!(&items.get(i).kind, ItemKind::Tool { speeds } if !speeds.is_empty()));
    let foe_first = aimed.entity.is_some() && block_dist.is_none_or(|d| aimed.dist < d);
    let fighting = moves.is_some() && (!works_blocks || foe_first || target.hit.is_none() || anim.charge.is_some() || anim.right.striking && anim.right.t.is_some());
    if let Some(e) = aimed.entity
        && !fighting
        && !gun
        && block_dist.is_none_or(|d| aimed.dist < d)
    {
        mining.block = None;
        if mouse.pressed(MouseButton::Left) && anim.strike_cooldown <= 0.0 {
            anim.start(hand, style);
            anim.strike_cooldown = STRIKE_COOLDOWN;
            let damage = item.map_or(crate::items::FIST_DAMAGE, |i| items.get(i).damage) * (1.0 + perks.damage);
            let mut dir = cam.forward().as_vec3();
            dir.y = 0.0;
            strikes.write(Strike { entity: e, damage, dir: dir.normalize_or_zero(), yours: true });
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

    // Right click on a sky shrine (`shrine`).
    if mouse.just_pressed(MouseButton::Right)
        && !at_station
        && let Some(hit) = target.hit
        && let Some(b) = world.block(hit.block)
        && blocks.registry.id("sky_shrine") == Some(b)
    {
        shrine_use.write(crate::shrine::UseShrine(hit.block));
        return;
    }

    // Right click on a door: open or close it, and the door above or below it.
    if mouse.just_pressed(MouseButton::Right)
        && !at_station
        && let Some(hit) = target.hit
        && let Some(other) = world.block(hit.block).and_then(|b| door_swap(&blocks.registry, b))
    {
        if warded(hit.block) {
            ward_note(&mut toast);
            return;
        }
        let closing = blocks.registry.get(other).solid;
        let mut cells = vec![hit.block];
        for dy in [1, -1] {
            let mut p = BlockPos::new(hit.block.x, hit.block.y + dy, hit.block.z);
            while world.block(p).is_some_and(|b| door_swap(&blocks.registry, b).is_some()) {
                cells.push(p);
                p.y += dy;
            }
        }
        // Never shut a door on yourself.
        if closing && cells.iter().any(|c| player.overlaps(*c)) {
            return;
        }
        for c in cells {
            if let Some(to) = world.block(c).and_then(|b| door_swap(&blocks.registry, b))
                && world.set_block(c, to)
            {
                remesh_around(&mut world, &blocks, c);
            }
        }
        placed.write(Placed { pos: hit.block, block: other });
        anim.start(hand, Swing::Jab);
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

    if gun {
        mining.block = None;
        return;
    }

    // A weapon fights: left click for its moves (one blow per swing,
    // landing at contact), right click blocks (`combat::Guard`).
    if fighting && let (Some(moves), Some(i)) = (&moves, item) {
        mining.block = None;
        let input = SwordInput {
            pressed: mouse.pressed(MouseButton::Left),
            just: mouse.just_pressed(MouseButton::Left),
            guard: guard.up,
        };
        let it = items.get(i);
        let wield = Wield { moves, stamina: it.stamina, speed: 1.0 + it.tuning.speed };
        sword(&mut anim, input, wield, &mut survival.vitals.health, dt);
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
        let free = world.block(into).is_some_and(|b| !blocks.registry.get(b).solid && !matches!(blocks.registry.get(b).shape, voxel::Shape::Torch | voxel::Shape::Model));
        // Never inside yourself (solid blocks only; a torch can go at your feet).
        let blocks_player = blocks.registry.get(place).solid && player.overlaps(into);
        // Nothing can be built at or above the world's ceiling.
        let under_ceiling = into.y < worldgen::WORLD_CEILING;
        // Plants (sown crops) only go on soil.
        let below = BlockPos::new(into.x, into.y - 1, into.z);
        let rooted = blocks.registry.get(place).shape != voxel::Shape::Plant || world.block(below).is_some_and(|b| blocks.registry.get(b).material == "soil");
        if free && !rooted && mouse.just_pressed(MouseButton::Right) {
            toast.write(crate::survival::Toast("Sow it on soil: dirt, grass, mud or peat".into()));
        }
        if free && mouse.just_pressed(MouseButton::Right) && warded(into) {
            ward_note(&mut toast);
        }
        if free && rooted && !warded(into) && !blocks_player && under_ceiling && use_up(&mut inv) && world.set_block(into, place) {
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
    // Soil dug without a shovel goes down a layer at a time (not when flying to build).
    let dug = broke.and_then(|(_, b)| items.dug_to(item, blocks.registry.get(b))).filter(|_| *mode != MoveMode::Fly);
    if let Some((pos, _)) = broke
        && match dug {
            Some(to) => dig_down(&mut world, &blocks, &mut mined, pos, to),
            None => break_block(&mut world, &blocks, &mut mined, pos),
        }
    {
        mining.block = None;
        mining.progress = 0.0;
    }
}

/// The attack button, for a sword.
#[derive(Clone, Copy, Default)]
pub struct SwordInput {
    pub pressed: bool,
    pub just: bool,
    /// Your guard is up (no cutting).
    pub guard: bool,
}

/// The weapon in hand: its moves, and what its quality and traits change.
#[derive(Clone, Copy)]
pub struct Wield<'a> {
    pub moves: &'a crate::items::Moves,
    /// Multiplier on the Fatigue each move costs.
    pub stamina: f32,
    /// Multiplier on how fast it swings.
    pub speed: f32,
}

/// A weapon in hand: tap for a light move (they chain into a combo), hold to
/// charge the heavy one, let go to swing it. Each wears Fatigue down
/// (`combat::stamina`); winded, nothing swings.
pub fn sword(anim: &mut HandAnim, input: SwordInput, wield: Wield, health: &mut crate::survival::health::Health, dt: f32) {
    use crate::combat::stamina::{self, HEAVY, LIGHT};
    anim.charging = wield.moves.heavy;
    let busy = anim.right.t.is_some();
    if busy {
        anim.combo_window = COMBO_WINDOW;
    } else {
        anim.combo_window -= dt;
    }
    if input.guard {
        anim.charge = None;
        anim.queued = false;
        return;
    }
    if input.just && anim.charge.is_none() {
        anim.charge = Some(0.0);
    }
    if let Some(c) = anim.charge {
        if input.pressed {
            // The charge builds only once the arm is free.
            if !busy {
                anim.charge = Some(c + dt);
            }
        } else {
            anim.charge = None;
            if c < TAP {
                anim.queued = true;
            } else if !busy && stamina::spend(health, HEAVY * wield.stamina) {
                let raised = ((c - TAP) / RAISE).min(1.0);
                let power = ((c - TAP) / CHARGE).min(1.0);
                let heavy = wield.moves.heavy;
                anim.right = Arm::fight(heavy.windup() * raised, heavy, true, heavy_mult(power), wield.speed);
                // The next light cut starts a fresh combo.
                anim.combo = 2;
            }
        }
    }
    if anim.queued && !busy {
        anim.queued = false;
        if stamina::spend(health, LIGHT * wield.stamina) {
            let (step, style, mult) = combo_cut(&wield.moves.light, anim.combo, anim.combo_window > 0.0);
            anim.combo = step;
            anim.right = Arm::fight(0.0, style, false, mult, wield.speed);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::stamina;
    use crate::survival::health::{Health as Stamina, Layer};

    const DT: f32 = 1.0 / 60.0;

    fn moves() -> crate::items::Moves {
        crate::items::Moves { light: vec![Swing::Slash, Swing::SlashBack, Swing::Slash], heavy: Swing::Heavy, reach: 3.0, arc: 55.0, special: Default::default() }
    }

    fn sword(anim: &mut HandAnim, input: SwordInput, st: &mut Stamina, dt: f32) {
        let m = moves();
        super::sword(anim, input, Wield { moves: &m, stamina: 1.0, speed: 1.0 }, st, dt);
    }

    /// Hold the button for `secs`, let go, and let the arm move for `after`.
    fn click(anim: &mut HandAnim, st: &mut Stamina, secs: f32) {
        let mut t = 0.0;
        let mut first = true;
        while t < secs {
            sword(anim, SwordInput { pressed: true, just: first, guard: false }, st, DT);
            anim.right.advance(DT);
            first = false;
            t += DT;
        }
        sword(anim, SwordInput::default(), st, DT);
    }

    fn finish(anim: &mut HandAnim, st: &mut Stamina) {
        while anim.right.t.is_some() {
            sword(anim, SwordInput::default(), st, DT);
            anim.right.advance(DT);
        }
    }

    #[test]
    fn taps_chain_into_a_three_cut_combo() {
        let (mut anim, mut st) = (HandAnim::default(), Stamina::default());
        let mut styles = Vec::new();
        for _ in 0..4 {
            click(&mut anim, &mut st, 0.1);
            styles.push((anim.right.style, anim.right.mult));
            finish(&mut anim, &mut st);
        }
        assert_eq!(styles[0], (Swing::Slash, 1.0));
        assert_eq!(styles[1], (Swing::SlashBack, 1.0));
        assert_eq!(styles[2], (Swing::Slash, 1.25));
        assert_eq!(styles[3], (Swing::Slash, 1.0), "the combo starts over");
        assert!(st.get(Layer::Fatigue) < 100.0);
    }

    #[test]
    fn a_pause_breaks_the_combo() {
        let (mut anim, mut st) = (HandAnim::default(), Stamina::default());
        click(&mut anim, &mut st, 0.1);
        finish(&mut anim, &mut st);
        for _ in 0..60 {
            sword(&mut anim, SwordInput::default(), &mut st, DT);
        }
        click(&mut anim, &mut st, 0.1);
        assert_eq!(anim.right.style, Swing::Slash);
        assert_eq!(anim.combo, 0);
    }

    #[test]
    fn holding_charges_a_heavy_cut() {
        let (mut anim, mut st) = (HandAnim::default(), Stamina::default());
        // Halfway up the arm is held there, no swing yet.
        let mut first = true;
        for _ in 0..30 {
            sword(&mut anim, SwordInput { pressed: true, just: first, guard: false }, &mut st, DT);
            first = false;
        }
        assert!(anim.right.t.is_none());
        assert!(anim.charge_level().is_some_and(|(s, k)| s == Swing::Heavy && k > 0.5));
        sword(&mut anim, SwordInput::default(), &mut st, DT);
        assert_eq!(anim.right.style, Swing::Heavy);
        assert!(anim.right.heavy && anim.right.mult > 1.6);
        // It starts where the charge left the arm, not back at rest.
        assert!(anim.right.t.unwrap() > 0.2);
        assert_eq!(heavy_mult(1.0), 2.4);
    }

    #[test]
    fn each_weapon_chains_its_own_moves() {
        let (mut anim, mut st) = (HandAnim::default(), Stamina::default());
        let spear = crate::items::Moves { light: vec![Swing::Thrust], heavy: Swing::Lunge, reach: 4.0, arc: 25.0, special: Default::default() };
        let light = Wield { moves: &spear, stamina: 0.5, speed: 1.2 };
        for _ in 0..2 {
            let mut first = true;
            for _ in 0..3 {
                super::sword(&mut anim, SwordInput { pressed: true, just: first, guard: false }, light, &mut st, DT);
                first = false;
            }
            super::sword(&mut anim, SwordInput::default(), light, &mut st, DT);
            assert_eq!((anim.right.style, anim.right.mult), (Swing::Thrust, 1.0));
            while anim.right.t.is_some() {
                anim.right.advance(DT);
            }
        }
        // Half the Fatigue of a sword's two light cuts.
        assert!((st.get(Layer::Fatigue) - (100.0 - stamina::LIGHT)).abs() < 1e-3);
        assert_eq!(combo_cut(&[Swing::Thrust, Swing::Thrust, Swing::Lunge], 1, true), (2, Swing::Lunge, 1.25));
    }

    #[test]
    fn nothing_swings_while_winded_or_guarding() {
        let (mut anim, mut st) = (HandAnim::default(), Stamina::default());
        stamina::spend(&mut st, 100.0);
        click(&mut anim, &mut st, 0.1);
        assert!(anim.right.t.is_none());
        let mut st = Stamina::default();
        sword(&mut anim, SwordInput { pressed: true, just: true, guard: true }, &mut st, DT);
        sword(&mut anim, SwordInput::default(), &mut st, DT);
        assert!(anim.right.t.is_none());
    }
}
