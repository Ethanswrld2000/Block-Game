//! The inventory screen (Tab): one window with four tabs along its top edge.
//!
//! - Items (`items`): the 20x10 pockets grid, the hand and weapon slots, and a
//!   side panel with Crafting and an open Chest.
//! - Equipment (`equipment`): a live picture of you (`preview`) with armor
//!   slots on the left, clothes on the right, and the weapon and hand slots
//!   under it.
//! - Stats (`stats`): health, body, gear, progress and skill totals.
//! - Abilities (`abilities`): your spells, and the skill tree (`progress::ui`
//!   draws the tree over the area this tab leaves for it).
//!
//! Click an item to pick it up, click again to put it down (or drag it and
//! let go); R turns it while you hold it. Right click moves an item straight
//! across (to the chest and back, onto your body, into the weapon slot).
//! Hover an item and press 1-9 to put it on the hotbar. Q throws what you
//! hold. Shift-click picks up half a stack. Slots that take what you hold
//! glow; the rest dim. K opens the screen on the Abilities tab.

mod abilities;
mod equipment;
mod stats;

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use voxel::BlockPos;

use super::crafting::{Recipes, Station};
use super::grid::{dims, Grid, Stack};
use super::world_items::Spill;
use super::{Chests, Inventory};
use crate::camera::{FlyCamera, Menus};
use crate::interact::{Hands, Hotbar, OpenChest};
use crate::item_art::ItemArt;
use crate::items::{ItemId, ItemKind, Items, WearSlot};
use crate::player::Player;
use crate::sound::PlaySound;
use crate::survival::camp::{Camp, Sleep};
use crate::survival::health::Layer;
use crate::survival::Survival;
use crate::theme::{self, colors, sizes, Theme};

/// One grid cell: item sprites are 20 px per cell, drawn at 2x.
const CELL: f32 = 40.0;
/// Width of the side panel (crafting, chest).
const SIDE_W: f32 = 360.0;
/// The space every tab has inside the window.
pub(crate) const CONTENT_W: f32 = 1182.0;
pub(crate) const CONTENT_H: f32 = 456.0;

pub struct InventoryUiPlugin;

impl Plugin for InventoryUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InvUi>()
            .init_resource::<InvScreen>()
            .init_resource::<Motion>()
            .add_systems(crate::menu::GameStartup, open_for_screenshot)
            .add_systems(
                crate::menu::GameUpdate,
                (open_close, hover, click, keys, scroll, refresh, rebuild, follow_cursor, animate).chain().after(crate::interact::InteractSet),
            )
            .add_plugins(super::preview::PreviewPlugin);
    }
}

/// The four tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Items,
    Equipment,
    Stats,
    Abilities,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Items, Tab::Equipment, Tab::Stats, Tab::Abilities];

    fn title(self) -> &'static str {
        match self {
            Tab::Items => "Items",
            Tab::Equipment => "Equipment",
            Tab::Stats => "Stats",
            Tab::Abilities => "Abilities",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tab::Items => "icon_bag",
            Tab::Equipment => "icon_figure",
            Tab::Stats => "icon_damage",
            Tab::Abilities => "icon_star",
        }
    }

    /// `BLOCKGAME_INVENTORY=items|equipment|stats|abilities`.
    fn parse(s: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.title().eq_ignore_ascii_case(s))
    }
}

/// Whether the screen is up and on which tab (read by the skill tree and the
/// live player picture).
#[derive(Resource, Debug, Default)]
pub struct InvScreen {
    pub open: bool,
    pub tab: Tab,
    /// The cursor is over the player picture (drag turns it).
    pub over_preview: bool,
    /// Something was just put on or taken off (the picture reacts).
    pub changed_gear: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridRef {
    Pockets,
    Chest(BlockPos),
}

/// Where a held item came from, so closing the screen can put it back.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Origin {
    Grid(GridRef, u8, u8, bool),
    Worn,
    LeftHand,
    Weapon,
}

#[derive(Debug, Clone, Copy)]
struct Held {
    stack: Stack,
    rotated: bool,
    from: Origin,
}

/// What the cursor is over.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hover {
    Cell(GridRef, u8, u8),
    Slot(WearSlot),
    Weapon,
    LeftHand,
    RightHand,
    /// An item in the equipment tab's strip of gear you carry (index into the pockets).
    Gear(usize),
    Recipe(usize),
    Side(Side),
    Tab(Tab),
    Preview,
}

/// What the Items tab's side panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Side {
    #[default]
    Crafting,
    Chest,
}

impl Side {
    fn title(self) -> &'static str {
        match self {
            Side::Crafting => "Crafting",
            Side::Chest => "Chest",
        }
    }
}

#[derive(Resource, Default)]
struct InvUi {
    open: bool,
    chest: Option<BlockPos>,
    held: Option<Held>,
    hover: Option<Hover>,
    dirty: bool,
    message: String,
    /// How far the crafting list is scrolled, in points.
    scroll: f32,
    /// The cursor is over the crafting list.
    over_list: bool,
    side: Side,
    tab: Tab,
    /// Where a left press started (what it was over, and the cursor), for drag and drop.
    press: Option<(Option<Hover>, Vec2)>,
}

/// Timings for the screen's small animations (real seconds).
#[derive(Resource, Default)]
struct Motion {
    now: f32,
    opened: f32,
    tab: f32,
    hover: f32,
    /// Something was just put into this slot: it settles with a bounce.
    settle: Option<(Hover, f32)>,
}

#[derive(Component)]
struct InvRoot;

/// The dark veil over the world.
#[derive(Component)]
struct Veil;

/// The window (scales in when the screen opens).
#[derive(Component)]
struct WinBox;

/// A tab's content (slides in when the tab changes).
#[derive(Component)]
struct Content;

/// A slot or tile that lifts when hovered and bounces when something lands in it.
#[derive(Component, Clone, Copy)]
struct Lift(Hover);

/// Hit-test regions.
#[derive(Component, Clone, Copy)]
enum Zone {
    Grid(GridRef, u8, u8),
    At(Hover),
    RecipeList,
}

#[derive(Component)]
struct HeldIcon;

#[derive(Component)]
struct Tooltip;

/// Where the skill tree goes on the Abilities tab (`progress::ui` draws it there).
#[derive(Component)]
pub struct TreeArea;

fn open_for_screenshot(mut ui: ResMut<InvUi>, mut menus: ResMut<Menus>, mut motion: ResMut<Motion>) {
    let tab = match (std::env::var("BLOCKGAME_INVENTORY"), std::env::var_os("BLOCKGAME_SKILLTREE")) {
        (_, Some(_)) => Some(Tab::Abilities),
        (Ok(v), _) => Some(Tab::parse(&v).unwrap_or(Tab::Items)),
        _ => None,
    };
    if let Some(tab) = tab {
        ui.open = true;
        ui.tab = tab;
        ui.dirty = true;
        menus.open = true;
        // Screenshots show the window settled.
        motion.opened = -10.0;
        motion.tab = -10.0;
    }
}

fn grid_mut<'a>(inv: &'a mut Inventory, chests: &'a mut Chests, g: GridRef) -> Option<&'a mut Grid> {
    match g {
        GridRef::Pockets => Some(&mut inv.pockets),
        GridRef::Chest(p) => Some(chests.grid(p)),
    }
}

/// Put what the cursor holds back where it came from (or in the pockets, or on the ground).
fn put_back(h: Held, inv: &mut Inventory, chests: &mut Chests, items: &Items, at: Vec3, spill: &mut MessageWriter<Spill>) {
    let back = match h.from {
        Origin::Grid(g, x, y, r) => grid_mut(inv, chests, g).is_some_and(|grid| grid.place(items, h.stack, x, y, r)),
        Origin::LeftHand if inv.left.is_none() => {
            inv.left = Some(h.stack);
            true
        }
        Origin::Weapon if inv.weapon.is_none() && h.stack.count == 1 => inv.arm(items, h.stack.item).is_ok(),
        _ => false,
    };
    if !back && let Some(left) = inv.add(items, h.stack) {
        spill.write(Spill { stack: left, at });
    }
}

#[allow(clippy::too_many_arguments)]
fn open_close(
    keys: Res<ButtonInput<KeyCode>>,
    real: Res<Time<Real>>,
    mut chest_msgs: MessageReader<OpenChest>,
    (mut ui, mut screen, mut motion): (ResMut<InvUi>, ResMut<InvScreen>, ResMut<Motion>),
    mut menus: ResMut<Menus>,
    (s, sleep): (Res<Survival>, Res<Sleep>),
    (items, player): (Res<Items>, Res<Player>),
    cam: Query<&Transform, With<FlyCamera>>,
    (mut inv, mut chests): (ResMut<Inventory>, ResMut<Chests>),
    (mut spill, mut sound): (MessageWriter<Spill>, MessageWriter<PlaySound>),
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    loot: Option<Res<super::loot::Loot>>,
    generator: Option<Res<crate::world::Generator>>,
    mut seed: Local<u64>,
) {
    motion.now = real.elapsed_secs();
    let busy = s.vitals.health.is_dead() || sleep.active();
    let mut want = ui.open;
    // Another screen (the pause menu, a bench) has the mouse: Tab, I and K wait.
    let free = ui.open || !menus.open;
    if (free && keys.just_pressed(KeyCode::Tab)) || (ui.open && (keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::KeyI))) {
        want = !ui.open;
    } else if free && keys.just_pressed(KeyCode::KeyI) {
        want = true;
    }
    // K: straight to the skill tree (and K again closes it).
    if free && keys.just_pressed(KeyCode::KeyK) {
        if ui.open && ui.tab == Tab::Abilities {
            want = false;
        } else {
            want = true;
            if ui.tab != Tab::Abilities {
                ui.tab = Tab::Abilities;
                motion.tab = motion.now;
                ui.dirty = true;
            }
        }
    }
    for m in chest_msgs.read() {
        want = true;
        ui.chest = Some(m.0);
        ui.side = Side::Chest;
        ui.tab = Tab::Items;
        if !chests.map.contains_key(&m.0)
            && let (Some(loot), Some(generator)) = (&loot, &generator)
        {
            // A chest found in the world: fill it the first time it's opened.
            let p = m.0;
            let in_barrow = generator.0.barrows_in(p.x - 1, p.z - 1, p.x + 1, p.z + 1).iter().any(|b| b.contains(p.x, p.y, p.z));
            *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15) ^ (p.x as u64).wrapping_mul(31) ^ (p.z as u64).wrapping_mul(1_000_003);
            let mut rng = crate::creatures::brain::Rng(*seed | 1);
            let grid = chests.grid(p);
            loot.fill(if in_barrow { "barrow" } else { "village" }, &items, grid, || rng.unit());
        }
        chests.grid(m.0);
        ui.dirty = true;
    }
    // Walked away from the chest, or it was broken: stop showing it.
    // (Personal storage, the fridge and the golem's pack aren't in the world: they stay open until you close them.)
    if let Some(c) = ui.chest
        && !super::virtual_chest(c)
        && (Vec3::new(c.x as f32 + 0.5, c.y as f32 + 0.5, c.z as f32 + 0.5).distance(player.pos + Vec3::Y) > 6.0 || !chests.map.contains_key(&c))
    {
        ui.chest = None;
        ui.dirty = true;
    }
    if ui.chest.is_none() && ui.side == Side::Chest {
        ui.side = Side::Crafting;
    }
    if busy {
        want = false;
    }
    screen.tab = ui.tab;
    if want == ui.open {
        screen.open = ui.open;
        return;
    }
    ui.open = want;
    screen.open = want;
    ui.dirty = true;
    menus.open = want;
    sound.write(PlaySound::new(if want { "ui_open" } else { "ui_close" }));
    if want {
        motion.opened = motion.now;
        motion.tab = motion.now - 10.0;
    } else {
        if let Some(h) = ui.held.take() {
            let at = cam.single().map(|t| t.translation).unwrap_or(player.pos);
            put_back(h, &mut inv, &mut chests, &items, at, &mut spill);
        }
        ui.chest = None;
        ui.message.clear();
        ui.press = None;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

fn hover(
    mut ui: ResMut<InvUi>,
    mut screen: ResMut<InvScreen>,
    mut motion: ResMut<Motion>,
    window: Single<&bevy::window::Window, With<PrimaryWindow>>,
    zones: Query<(&Zone, &ComputedNode, &UiGlobalTransform)>,
) {
    if !ui.open {
        screen.over_preview = false;
        return;
    }
    let Some(p) = window.physical_cursor_position() else {
        ui.hover = None;
        return;
    };
    let mut found = None;
    let mut over_list = false;
    for (z, node, tf) in &zones {
        if !node.contains_point(*tf, p) {
            continue;
        }
        found = Some(match *z {
            Zone::RecipeList => {
                over_list = true;
                continue;
            }
            Zone::Grid(g, w, h) => {
                let Some(n) = node.normalize_point(*tf, p) else { continue };
                let x = (((n.x + 0.5) * w as f32).floor() as i32).clamp(0, w as i32 - 1) as u8;
                let y = (((n.y + 0.5) * h as f32).floor() as i32).clamp(0, h as i32 - 1) as u8;
                Hover::Cell(g, x, y)
            }
            Zone::At(h) => h,
        });
    }
    if ui.over_list != over_list {
        ui.over_list = over_list;
    }
    screen.over_preview = found == Some(Hover::Preview);
    if ui.hover != found {
        ui.hover = found;
        motion.hover = motion.now;
        ui.dirty = true;
    }
}

/// Put a held stack into a grid at a cell: onto a matching stack, into free
/// space, or swapping with the one item in the way.
fn drop_into(grid: &mut Grid, items: &Items, held: Held, at: (u8, u8), g: GridRef) -> Result<Option<Held>, &'static str> {
    if let Some(i) = grid.at(items, at.0, at.1) {
        let p = grid.items[i];
        let max = items.get(p.stack.item).stack;
        if p.stack.item == held.stack.item && p.stack.count < max {
            let n = (max - p.stack.count).min(held.stack.count);
            grid.items[i].stack.count += n;
            let left = held.stack.count - n;
            return Ok((left > 0).then_some(Held { stack: Stack::new(held.stack.item, left), ..held }));
        }
    }
    if grid.place(items, held.stack, at.0, at.1, held.rotated) {
        return Ok(None);
    }
    // Swap with a single item in the way.
    let (w, h) = dims(items, held.stack.item, held.rotated);
    let mut in_way: Vec<usize> = Vec::new();
    for y in at.1..at.1.saturating_add(h) {
        for x in at.0..at.0.saturating_add(w) {
            if let Some(i) = grid.at(items, x, y)
                && !in_way.contains(&i)
            {
                in_way.push(i);
            }
        }
    }
    if in_way.len() == 1 && grid.fits(items, held.stack.item, at.0, at.1, held.rotated, Some(in_way[0])) {
        let p = grid.items.remove(in_way[0]);
        grid.place(items, held.stack, at.0, at.1, held.rotated);
        return Ok(Some(Held { stack: p.stack, rotated: p.rotated, from: Origin::Grid(g, p.x, p.y, p.rotated) }));
    }
    Err("No room there")
}

/// Put a held stack in the left hand: tops up the same item, else swaps.
fn into_left_hand(inv: &mut Inventory, items: &Items, held: Held) -> Option<Held> {
    if let Some(s) = &mut inv.left
        && s.item == held.stack.item
    {
        let max = items.get(s.item).stack;
        let n = (max - s.count.min(max)).min(held.stack.count);
        s.count += n;
        let rest = held.stack.count - n;
        return (rest > 0).then_some(Held { stack: Stack::new(held.stack.item, rest), ..held });
    }
    inv.hold_left(held.stack).map(|old| Held { stack: old, rotated: false, from: Origin::LeftHand })
}

/// Does this slot take the item? (For the glow and the dimming.)
fn accepts(items: &Items, at: Hover, item: ItemId) -> bool {
    let it = items.get(item);
    match at {
        Hover::Slot(s) => it.wear_slot() == Some(s),
        Hover::Weapon => it.is_weapon(),
        Hover::LeftHand | Hover::RightHand => true,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn click(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&bevy::window::Window, With<PrimaryWindow>>,
    items: Res<Items>,
    recipes: Option<Res<Recipes>>,
    (camp, player): (Res<Camp>, Res<Player>),
    (mut ui, mut screen, mut motion): (ResMut<InvUi>, ResMut<InvScreen>, ResMut<Motion>),
    mut inv: ResMut<Inventory>,
    mut chests: ResMut<Chests>,
    (hands, mut hotbar): (Res<Hands>, ResMut<Hotbar>),
    (mut spill, mut sound): (MessageWriter<Spill>, MessageWriter<PlaySound>),
    (mut gain, prog): (MessageWriter<crate::progress::GainXp>, Option<Res<crate::progress::xp::Progression>>),
) {
    if !ui.open {
        return;
    }
    let cursor = window.cursor_position().unwrap_or_default();
    if mouse.just_pressed(MouseButton::Left) {
        ui.press = Some((ui.hover, cursor));
    }
    // Drag and drop: let go somewhere else while holding what you pressed on.
    let dropped = mouse.just_released(MouseButton::Left)
        && ui.held.is_some()
        && ui.press.is_some_and(|(at, from)| at != ui.hover && from.distance(cursor) > 8.0);
    if mouse.just_released(MouseButton::Left) {
        ui.press = None;
    }
    let left = mouse.just_pressed(MouseButton::Left) || dropped;
    let right = mouse.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    let Some(hover) = ui.hover else { return };
    ui.dirty = true;
    ui.message.clear();
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let had = ui.held.is_some();
    let worn_before = (inv.worn.clone(), inv.weapon, inv.left);
    match hover {
        Hover::Tab(t) => {
            if ui.tab != t {
                ui.tab = t;
                screen.tab = t;
                motion.tab = motion.now;
                sound.write(PlaySound::new("ui_tab"));
            }
        }
        Hover::Side(s) => ui.side = s,
        Hover::Preview => {}
        Hover::Cell(g, x, y) if left => {
            let held = ui.held;
            let Some(grid) = grid_mut(&mut inv, &mut chests, g) else { return };
            match held {
                Some(h) => match drop_into(grid, &items, h, (x, y), g) {
                    Ok(rest) => ui.held = rest,
                    Err(e) => ui.message = e.into(),
                },
                None => {
                    if let Some(i) = grid.at(&items, x, y) {
                        let p = grid.items[i];
                        if shift && p.stack.count > 1 {
                            let half = p.stack.count / 2;
                            grid.items[i].stack.count -= half;
                            ui.held = Some(Held { stack: Stack::new(p.stack.item, half), rotated: p.rotated, from: Origin::Grid(g, p.x, p.y, p.rotated) });
                        } else {
                            grid.items.remove(i);
                            ui.held = Some(Held { stack: p.stack, rotated: p.rotated, from: Origin::Grid(g, p.x, p.y, p.rotated) });
                        }
                    }
                }
            }
        }
        Hover::Gear(i) if left => {
            if ui.held.is_some() {
                // Putting something down on the strip: back into the pockets.
                if let Some(h) = ui.held.take() {
                    put_back(h, &mut inv, &mut chests, &items, player.pos + Vec3::Y, &mut spill);
                }
            } else if i < inv.pockets.items.len() {
                let p = inv.pockets.items.remove(i);
                ui.held = Some(Held { stack: p.stack, rotated: p.rotated, from: Origin::Grid(GridRef::Pockets, p.x, p.y, p.rotated) });
            }
        }
        Hover::Cell(g, x, y) => {
            // Right click: move straight across.
            if ui.held.is_some() {
                return;
            }
            let chest = ui.chest.filter(|_| ui.tab == Tab::Items && ui.side == Side::Chest);
            let Some(grid) = grid_mut(&mut inv, &mut chests, g) else { return };
            let Some(i) = grid.at(&items, x, y) else { return };
            let p = grid.items.remove(i);
            let rest = move_across(&mut inv, &mut chests, &items, p.stack, g, chest, &mut ui.message);
            if let Some(rest) = rest {
                // Didn't all fit: put it back where it was.
                let grid = grid_mut(&mut inv, &mut chests, g).expect("grid still there");
                if !grid.place(&items, rest, p.x, p.y, p.rotated)
                    && let Some(r) = grid.add(&items, rest)
                {
                    spill.write(Spill { stack: r, at: player.pos + Vec3::Y });
                }
                if ui.message.is_empty() && rest == p.stack {
                    ui.message = "No room".into();
                }
            }
        }
        Hover::Gear(i) => {
            // Right click a carried piece of gear: put it on.
            if ui.held.is_some() || i >= inv.pockets.items.len() {
                return;
            }
            let p = inv.pockets.items.remove(i);
            if let Some(rest) = move_across(&mut inv, &mut chests, &items, p.stack, GridRef::Pockets, None, &mut ui.message)
                && !inv.pockets.place(&items, rest, p.x, p.y, p.rotated)
                && let Some(r) = inv.add(&items, rest)
            {
                spill.write(Spill { stack: r, at: player.pos + Vec3::Y });
            }
        }
        Hover::Slot(slot) => match ui.held {
            Some(h) if left => {
                let def = items.get(h.stack.item);
                if def.wear_slot() != Some(slot) {
                    ui.message = match def.wear_slot() {
                        Some(s) => format!("That goes on: {}", s.title()),
                        None => "You can't wear that".into(),
                    };
                    return;
                }
                match inv.wear(&items, h.stack.item) {
                    Ok(old) => {
                        // A stack of one goes on; any more stay in hand.
                        let more = (h.stack.count > 1).then(|| Held { stack: Stack::new(h.stack.item, h.stack.count - 1), ..h });
                        ui.held = old.map(|o| Held { stack: Stack::new(o, 1), rotated: false, from: Origin::Worn }).or(more);
                        if let (Some(_), Some(m)) = (old, more) {
                            put_back(m, &mut inv, &mut chests, &items, player.pos + Vec3::Y, &mut spill);
                        }
                    }
                    Err(e) => ui.message = e.into(),
                }
            }
            Some(_) => {}
            None => match inv.take_off(slot) {
                Ok(item) if left => ui.held = Some(Held { stack: Stack::new(item, 1), rotated: false, from: Origin::Worn }),
                Ok(item) => {
                    if let Some(rest) = inv.add(&items, Stack::new(item, 1)) {
                        // No room: keep it on.
                        let _ = inv.wear(&items, rest.item);
                        ui.message = "No room to take it off".into();
                    }
                }
                Err(e) => {
                    if inv.worn.contains_key(&slot) {
                        ui.message = e.into();
                    }
                }
            },
        },
        Hover::Weapon => match ui.held {
            Some(h) if left => {
                if !items.get(h.stack.item).is_weapon() {
                    ui.message = "Only weapons go in the weapon slot".into();
                    return;
                }
                if let Ok(old) = inv.arm(&items, h.stack.item) {
                    let more = (h.stack.count > 1).then(|| Held { stack: Stack::new(h.stack.item, h.stack.count - 1), ..h });
                    ui.held = old.map(|o| Held { stack: Stack::new(o, 1), rotated: false, from: Origin::Weapon }).or(more);
                }
            }
            Some(_) => {}
            None => {
                if let Some(w) = inv.weapon.take() {
                    if left {
                        ui.held = Some(Held { stack: Stack::new(w, 1), rotated: false, from: Origin::Weapon });
                    } else if inv.add(&items, Stack::new(w, 1)).is_some() {
                        inv.weapon = Some(w);
                        ui.message = "No room in your pockets".into();
                    }
                }
            }
        },
        Hover::LeftHand => match ui.held {
            Some(h) if left => ui.held = into_left_hand(&mut inv, &items, h),
            Some(_) => {}
            None => {
                if let Some(s) = inv.left {
                    if !left {
                        inv.left = None;
                        if let Some(rest) = inv.add(&items, s) {
                            inv.left = Some(rest);
                            ui.message = "No room in your pockets".into();
                        }
                    } else if shift && s.count > 1 {
                        let half = s.count / 2;
                        inv.left = Some(Stack::new(s.item, s.count - half));
                        ui.held = Some(Held { stack: Stack::new(s.item, half), rotated: false, from: Origin::LeftHand });
                    } else {
                        inv.left = None;
                        ui.held = Some(Held { stack: s, rotated: false, from: Origin::LeftHand });
                    }
                }
            }
        },
        Hover::RightHand => match ui.held {
            Some(h) if left => {
                // The right hand holds the selected hotbar slot: put the item
                // in the pockets and on that key.
                let key = hands.right.unwrap_or(0);
                let id = h.stack.item;
                ui.held = None;
                put_back(h, &mut inv, &mut chests, &items, player.pos + Vec3::Y, &mut spill);
                for s in hotbar.slots.iter_mut() {
                    if *s == Some(id) {
                        *s = None;
                    }
                }
                if let Some(s) = hotbar.slots.get_mut(key) {
                    *s = Some(id);
                }
                ui.message = format!("{} in your right hand (hotbar key {})", items.get(id).title, key + 1);
            }
            Some(_) => {}
            None => ui.message = "The right hand holds your hotbar pick (1-9); R draws your weapon".into(),
        },
        Hover::Recipe(i) => {
            if !left {
                return;
            }
            let Some(r) = recipes.as_ref().and_then(|r| r.list.get(i)) else { return };
            if r.at == Station::Camp && !camp.near(player.pos) {
                ui.message = "Make this at your camp (by the bedroll)".into();
                return;
            }
            let times = if shift { 5 } else { 1 };
            let mut made = 0;
            for _ in 0..times {
                match r.craft(&mut inv, &items) {
                    None => break,
                    Some(left) => {
                        made += 1;
                        if let Some(l) = left {
                            spill.write(Spill { stack: l, at: player.pos + Vec3::Y });
                        }
                    }
                }
            }
            if made > 0
                && let Some(prog) = &prog
            {
                gain.write(crate::progress::GainXp { xp: prog.xp.craft * made as f32, why: String::new() });
            }
            ui.message = if made == 0 {
                "Missing parts".into()
            } else {
                format!("Made {} {}", r.makes.count as u32 * made, items.get(r.makes.item).title)
            };
        }
    }
    // Sounds and the settle bounce.
    let gear_changed = worn_before != (inv.worn.clone(), inv.weapon, inv.left);
    if gear_changed {
        screen.changed_gear += 1;
    }
    let has = ui.held.is_some();
    if gear_changed && matches!(hover, Hover::Slot(_) | Hover::Weapon | Hover::LeftHand | Hover::Gear(_)) && (!has || had) {
        sound.write(PlaySound::new("ui_equip"));
        motion.settle = Some((hover, motion.now));
    } else if !had && has {
        sound.write(PlaySound::new("ui_pick"));
    } else if had && !has {
        sound.write(PlaySound::new("ui_drop"));
        motion.settle = Some((hover, motion.now));
    } else if had && has && matches!(hover, Hover::Cell(..) | Hover::Slot(_) | Hover::Weapon | Hover::LeftHand) {
        sound.write(PlaySound::new("ui_drop"));
    }
}

/// Right click on an item: into the open chest, out of a chest, onto your
/// body, or into the weapon slot. Returns what didn't go.
fn move_across(inv: &mut Inventory, chests: &mut Chests, items: &Items, stack: Stack, from: GridRef, chest: Option<BlockPos>, message: &mut String) -> Option<Stack> {
    let def = items.get(stack.item);
    match (from, chest) {
        (GridRef::Chest(_), _) => inv.add(items, stack),
        (_, Some(c)) => chests.grid(c).add(items, stack),
        _ if def.wear_slot().is_some() => match inv.wear(items, stack.item) {
            Ok(old) => {
                let mut rest = old.and_then(|o| inv.add(items, Stack::new(o, 1)));
                if stack.count > 1 && rest.is_none() {
                    rest = inv.add(items, Stack::new(stack.item, stack.count - 1));
                }
                rest
            }
            Err(e) => {
                *message = e.into();
                Some(stack)
            }
        },
        _ if def.is_weapon() => match inv.arm(items, stack.item) {
            Ok(old) => old.and_then(|o| inv.add(items, Stack::new(o, 1))),
            Err(e) => {
                *message = e.into();
                Some(stack)
            }
        },
        _ => Some(stack),
    }
}

#[allow(clippy::too_many_arguments)]
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    items: Res<Items>,
    player: Res<Player>,
    cam: Query<&Transform, With<FlyCamera>>,
    mut ui: ResMut<InvUi>,
    mut inv: ResMut<Inventory>,
    mut chests: ResMut<Chests>,
    mut hotbar: ResMut<Hotbar>,
    mut spill: MessageWriter<Spill>,
) {
    if !ui.open {
        return;
    }
    if keys.just_pressed(KeyCode::KeyR)
        && let Some(h) = &mut ui.held
    {
        h.rotated = !h.rotated;
        ui.dirty = true;
    }
    if keys.just_pressed(KeyCode::KeyQ)
        && let Some(h) = ui.held.take()
    {
        let at = cam.single().map(|t| t.translation + t.forward().as_vec3() * 1.2).unwrap_or(player.pos + Vec3::Y);
        spill.write(Spill { stack: h.stack, at });
        ui.dirty = true;
    }
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5,
        KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
    ];
    for (k, key) in DIGITS.iter().enumerate() {
        if !keys.just_pressed(*key) {
            continue;
        }
        let item = match ui.hover {
            Some(Hover::Cell(g, x, y)) => grid_mut(&mut inv, &mut chests, g).and_then(|grid| grid.at(&items, x, y).map(|i| grid.items[i].stack.item)),
            Some(Hover::Gear(i)) => inv.pockets.items.get(i).map(|p| p.stack.item),
            _ => None,
        }
        .or(ui.held.map(|h| h.stack.item));
        if let Some(id) = item {
            for s in hotbar.slots.iter_mut() {
                if *s == Some(id) {
                    *s = None;
                }
            }
            hotbar.slots[k] = Some(id);
            ui.message = format!("{} on hotbar key {}", items.get(id).title, k + 1);
            ui.dirty = true;
        }
    }
}

/// The live numbers on the Stats and Equipment tabs refresh a few times a second.
fn refresh(time: Res<Time<Real>>, mut ui: ResMut<InvUi>, mut t: Local<f32>) {
    if !ui.open || !matches!(ui.tab, Tab::Stats | Tab::Equipment) {
        return;
    }
    *t += time.delta_secs();
    if *t > 0.25 {
        *t = 0.0;
        ui.dirty = true;
    }
}

pub(crate) fn text(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    theme::text(s, size, color)
}

/// The tint of an item's tile: plain, so the picture stands out.
fn item_bg(hovered: bool) -> Color {
    if hovered { Color::srgb(0.62, 0.58, 0.5) } else { Color::srgb(0.42, 0.4, 0.36) }
}

/// Height of a crafting row, and the gap between rows.
const RECIPE_ROW: f32 = 46.0;
const RECIPE_GAP: f32 = 4.0;
/// How tall the crafting list is before it scrolls.
const RECIPE_VIEW: f32 = CONTENT_H - 72.0;

/// An item's picture: its block texture, or its name on a tile of its colour,
/// with the count in the corner.
pub(crate) fn icon(c: &mut ChildSpawnerCommands, items: &Items, art: &ItemArt, stack: Stack, w: f32, h: f32, rotated: bool) {
    match (art.image(stack.item), art.size(stack.item)) {
        (Some(img), Some(size)) => {
            // At 2x, or smaller if that doesn't fit (a tall item on the
            // hotbar); turned with the item.
            let s = size.as_vec2();
            let (bw, bh) = if rotated { (h, w) } else { (w, h) };
            let k = 2.0f32.min((bw - 4.0) / s.x).min((bh - 4.0) / s.y);
            let (sw, sh) = (s.x * k, s.y * k);
            let mut e = c.spawn((
                img,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(((w - sw) / 2.0).round()),
                    top: Val::Px(((h - sh) / 2.0).round()),
                    width: Val::Px(sw),
                    height: Val::Px(sh),
                    ..default()
                },
            ));
            if rotated {
                e.insert(UiTransform { rotation: Rot2::degrees(90.0), ..default() });
            }
        }
        _ => {
            // No sprite yet: the name.
            let size = if w.min(h) < 100.0 { sizes::TINY } else { sizes::SMALL };
            c.spawn((
                text(items.get(stack.item).title.clone(), size, colors::TEXT),
                TextLayout::justify(Justify::Center),
                Node { margin: UiRect::all(Val::Auto), max_width: Val::Px(w - 8.0), ..default() },
            ));
        }
    }
    if stack.count > 1 {
        c.spawn((
            text(format!("{}", stack.count), sizes::BODY, colors::TEXT),
            Node { position_type: PositionType::Absolute, right: Val::Px(5.0), bottom: Val::Px(2.0), ..default() },
        ));
    }
}

/// What a slot shows when it's empty.
fn silhouette(at: Hover) -> &'static str {
    match at {
        Hover::Slot(WearSlot::Head) => "sil_hat",
        Hover::Slot(WearSlot::HeadArmor) => "sil_helmet",
        Hover::Slot(WearSlot::Body) => "sil_shirt",
        Hover::Slot(WearSlot::BodyArmor) => "sil_chest",
        Hover::Slot(WearSlot::Legs) => "sil_trousers",
        Hover::Slot(WearSlot::Feet) => "sil_boots",
        Hover::Slot(WearSlot::Back) => "sil_bag",
        Hover::Weapon => "sil_sword",
        Hover::LeftHand => "sil_hand_left",
        _ => "sil_hand",
    }
}

/// Everything a slot needs to draw itself.
struct SlotView<'a> {
    theme: &'a Theme,
    items: &'a Items,
    art: &'a ItemArt,
    hover: Option<Hover>,
    held: Option<Held>,
}

impl SlotView<'_> {
    /// A square equipment or hand slot of `size` points: the item, or a faint
    /// silhouette of what goes there. It glows when it takes what you hold,
    /// dims when it doesn't.
    fn slot(&self, p: &mut ChildSpawnerCommands, at: Hover, stack: Option<Stack>, size: f32) {
        let hovered = self.hover == Some(at);
        let fits = self.held.map(|h| accepts(self.items, at, h.stack.item));
        let skin = match (fits, hovered) {
            (Some(true), _) => "slot_glow",
            (_, true) => "slot_hover",
            _ => "slot",
        };
        let dim = fits == Some(false);
        let tint = if dim { Color::srgba(0.55, 0.55, 0.55, 0.8) } else { Color::WHITE };
        p.spawn((
            Zone::At(at),
            Lift(at),
            UiTransform::default(),
            Node { width: Val::Px(size), height: Val::Px(size), flex_shrink: 0.0, ..default() },
            self.theme.skin_tinted(skin, tint),
            Outline { width: Val::Px(if hovered { 2.0 } else { 0.0 }), offset: Val::Px(0.0), color: colors::GOLD },
        ))
        .with_children(|c| match stack {
            Some(s) => {
                c.spawn((
                    Node { position_type: PositionType::Absolute, left: Val::Px(4.0), top: Val::Px(4.0), right: Val::Px(4.0), bottom: Val::Px(4.0), ..default() },
                    self.theme.skin_tinted("item", item_bg(hovered).with_alpha(if dim { 0.5 } else { 1.0 })),
                ))
                .with_children(|q| icon(q, self.items, self.art, s, size - 8.0, size - 8.0, false));
            }
            None => {
                let k = ((size - 12.0) / 32.0).floor().max(1.0) * 32.0;
                let mut img = self.theme.image(silhouette(at));
                img.color = Color::srgba(0.85, 0.9, 0.82, if dim { 0.12 } else { 0.28 });
                c.spawn((
                    img,
                    Node { position_type: PositionType::Absolute, left: Val::Px((size - k) / 2.0), top: Val::Px((size - k) / 2.0), width: Val::Px(k), height: Val::Px(k), ..default() },
                ));
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn grid_node(p: &mut ChildSpawnerCommands, theme: &Theme, grid: &Grid, g: GridRef, items: &Items, art: &ItemArt, hover: Option<Hover>, held: Option<Held>) {
    p.spawn((Node { padding: UiRect::all(Val::Px(4.0)), ..default() }, theme.skin("inset"))).with_children(|frame| {
        frame
            .spawn((Zone::Grid(g, grid.w, grid.h), Node { width: Val::Px(grid.w as f32 * CELL), height: Val::Px(grid.h as f32 * CELL), ..default() }))
            .with_children(|gn| {
                // Where the held item would land.
                let preview = match (hover, held) {
                    (Some(Hover::Cell(hg, x, y)), Some(h)) if hg == g => {
                        let (w, hh) = dims(items, h.stack.item, h.rotated);
                        let ok = grid.fits(items, h.stack.item, x, y, h.rotated, None)
                            || grid.at(items, x, y).is_some_and(|i| grid.items[i].stack.item == h.stack.item);
                        Some((x, y, w, hh, ok))
                    }
                    _ => None,
                };
                for y in 0..grid.h {
                    for x in 0..grid.w {
                        let lit = preview.is_some_and(|(px, py, w, h, _)| x >= px && x < px + w && y >= py && y < py + h);
                        let cell = Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(x as f32 * CELL),
                            top: Val::Px(y as f32 * CELL),
                            width: Val::Px(CELL),
                            height: Val::Px(CELL),
                            ..default()
                        };
                        match preview {
                            Some((.., ok)) if lit => {
                                let tint = if ok { Color::srgb(0.55, 1.0, 0.55) } else { Color::srgb(1.0, 0.45, 0.4) };
                                gn.spawn((cell, theme.skin_tinted("slot_hover", tint)));
                            }
                            _ => {
                                gn.spawn((cell, theme.skin("slot")));
                            }
                        }
                    }
                }
                for p in &grid.items {
                    let (w, h) = dims(items, p.stack.item, p.rotated);
                    let (pw, ph) = (w as f32 * CELL, h as f32 * CELL);
                    let hovered = matches!(hover, Some(Hover::Cell(hg, x, y)) if hg == g && grid.at(items, x, y).is_some_and(|i| grid.items[i] == *p));
                    gn.spawn((
                        Lift(Hover::Cell(g, p.x, p.y)),
                        UiTransform::default(),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(p.x as f32 * CELL + 1.0),
                            top: Val::Px(p.y as f32 * CELL + 1.0),
                            width: Val::Px(pw - 2.0),
                            height: Val::Px(ph - 2.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        theme.skin_tinted("item", item_bg(hovered)),
                        Outline { width: Val::Px(if hovered { 2.0 } else { 0.0 }), offset: Val::Px(0.0), color: colors::GOLD },
                    ))
                    .with_children(|c| icon(c, items, art, p.stack, pw - 2.0, ph - 2.0, p.rotated));
                }
            });
    });
}

/// One line of an item's card: an icon (optional), the text, its colour.
type Line = (Option<&'static str>, String, Color);

fn layer_icon(l: Layer) -> &'static str {
    match l {
        Layer::Damage => "icon_damage",
        Layer::Fatigue => "icon_fatigue",
        Layer::Blood => "icon_blood",
        Layer::Injuries => "icon_injuries",
    }
}

/// An item's name, what kind of thing it is, and what it does.
fn describe(items: &Items, id: ItemId) -> (String, String, Vec<Line>) {
    let it = items.get(id);
    let mut lines: Vec<Line> = Vec::new();
    let kind = match &it.kind {
        ItemKind::Wear { slot, insulation, protection } => {
            if *insulation > 0.0 {
                lines.push((Some("icon_warmth"), format!("Warmth +{insulation:.0}°C"), colors::GOOD));
            }
            for l in Layer::ALL {
                if let Some(p) = protection.get(&l) {
                    lines.push((Some(layer_icon(l)), format!("{} -{:.0}%", l.name(), p * 100.0), colors::GOOD));
                }
            }
            format!("Worn on: {}", slot.title())
        }
        ItemKind::Bag { grid } => format!("Bag: holds {}x{}", grid.0, grid.1),
        ItemKind::Food { tier, satiety } => {
            lines.push((Some("icon_food"), format!("+{satiety:.0} food (tier {tier})"), colors::GOOD));
            lines.push((None, "Hold right click to eat".into(), colors::MUTED));
            "Food".into()
        }
        ItemKind::Tool { speeds } if it.is_weapon() => {
            lines.push((None, "Weapon slot: R draws it".into(), colors::MUTED));
            let _ = speeds;
            "Weapon".into()
        }
        ItemKind::Tool { speeds } => {
            let mut v: Vec<_> = speeds.iter().collect();
            v.sort_by(|a, b| a.0.cmp(b.0));
            lines.push((None, v.iter().map(|(m, x)| format!("{m} x{x}")).collect::<Vec<_>>().join(", "), colors::TEXT));
            "Tool".into()
        }
        ItemKind::Block(_) => {
            lines.push((None, "Right click to place".into(), colors::MUTED));
            "Block".into()
        }
        ItemKind::Torch => {
            lines.push((Some("icon_light"), "Gives light; right click to place".into(), colors::TEXT));
            "Light".into()
        }
        ItemKind::Material => "Crafting material".into(),
        ItemKind::Machine(_) => {
            lines.push((None, "Right click open ground to set it down".into(), colors::MUTED));
            "Machine".into()
        }
    };
    if it.damage > crate::items::FIST_DAMAGE {
        lines.push((Some("icon_sword"), format!("Hits for {:.0}", it.damage), colors::TEXT));
    }
    let mut weight = format!("{:.1} kg, takes {}x{}", it.mass, it.size.0, it.size.1);
    if it.stack > 1 {
        weight.push_str(&format!(", stacks to {}", it.stack));
    }
    lines.push((Some("icon_load"), weight, colors::MUTED));
    (it.title.clone(), kind, lines)
}

/// A line with an icon in front.
fn icon_line(p: &mut ChildSpawnerCommands, theme: &Theme, icon_name: &str, s: String, color: Color) {
    p.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
        r.spawn(theme.icon(icon_name));
        r.spawn(text(s, sizes::BODY, color));
    });
}

/// The item under the cursor, for the card.
fn hovered_item(hover: Option<Hover>, inv: &Inventory, chests: &Chests, items: &Items, recipes: Option<&Recipes>, wielded: Option<ItemId>) -> Option<ItemId> {
    match hover? {
        Hover::Cell(g, x, y) => {
            let grid = match g {
                GridRef::Pockets => Some(&inv.pockets),
                GridRef::Chest(p) => chests.map.get(&p),
            };
            grid.and_then(|gr| gr.at(items, x, y).map(|i| gr.items[i].stack.item))
        }
        Hover::Slot(s) => inv.worn.get(&s).copied(),
        Hover::Weapon => inv.weapon,
        Hover::LeftHand => inv.left_item(),
        Hover::RightHand => wielded,
        Hover::Gear(i) => inv.pockets.items.get(i).map(|p| p.stack.item),
        Hover::Recipe(i) => recipes.and_then(|r| r.list.get(i)).map(|r| r.makes.item),
        Hover::Tab(_) | Hover::Side(_) | Hover::Preview => None,
    }
}

/// Everything the tabs draw from.
#[derive(bevy::ecs::system::SystemParam)]
struct Data<'w> {
    inv: Res<'w, Inventory>,
    chests: Res<'w, Chests>,
    items: Res<'w, Items>,
    art: Option<Res<'w, ItemArt>>,
    recipes: Option<Res<'w, Recipes>>,
    camp: Res<'w, Camp>,
    player: Res<'w, Player>,
    s: Res<'w, Survival>,
    wielded: Res<'w, crate::interact::Wielded>,
    progress: Res<'w, crate::progress::Progress>,
    prog: Res<'w, crate::progress::xp::Progression>,
    perks: Res<'w, crate::progress::skills::Perks>,
    spells: Res<'w, crate::progress::Spells>,
    preview: Option<Res<'w, super::preview::PreviewImage>>,
}

#[allow(clippy::too_many_arguments)]
fn rebuild(mut commands: Commands, mut ui: ResMut<InvUi>, theme: Res<Theme>, d: Data, roots: Query<Entity, With<InvRoot>>) {
    if !(ui.dirty || (ui.open && (d.inv.is_changed() || d.chests.is_changed()))) {
        return;
    }
    ui.dirty = false;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let (Some(art), true) = (d.art.as_deref(), ui.open) else { return };
    let (hover, held) = (ui.hover, ui.held);
    let theme = &*theme;
    let items = &*d.items;
    let inv = &*d.inv;
    let view = SlotView { theme, items, art, hover, held };
    let tab = ui.tab;
    commands
        .spawn((
            InvRoot,
            Veil,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.01, 0.0, 0.78)),
            GlobalZIndex(20),
        ))
        .with_children(|root| {
            root.spawn((WinBox, UiTransform::default(), Node { flex_direction: FlexDirection::Column, ..default() })).with_children(|col| {
                // Folder tabs along the top edge; the open one runs into the window.
                col.spawn((ZIndex(2), Node { column_gap: Val::Px(4.0), padding: UiRect::left(Val::Px(18.0)), margin: UiRect::bottom(Val::Px(-12.0)), align_items: AlignItems::FlexEnd, ..default() }))
                    .with_children(|tabs| {
                        for t in Tab::ALL {
                            let active = tab == t;
                            let hovered = hover == Some(Hover::Tab(t));
                            let skin = if active { "tab_active" } else if hovered { "tab_hover" } else { "tab" };
                            tabs.spawn((
                                Zone::At(Hover::Tab(t)),
                                Node {
                                    column_gap: Val::Px(8.0),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::new(Val::Px(18.0), Val::Px(20.0), Val::Px(10.0), Val::Px(if active { 18.0 } else { 10.0 })),
                                    margin: UiRect::bottom(Val::Px(if active { 0.0 } else { 12.0 })),
                                    ..default()
                                },
                                theme.skin(skin),
                            ))
                            .with_children(|b| {
                                b.spawn(theme.icon(t.icon()));
                                b.spawn((theme.bold(sizes::LABEL), Text::new(t.title()), TextColor(if active { colors::HEADING } else if hovered { colors::TEXT } else { colors::MUTED })));
                            });
                        }
                    });
                col.spawn((Node { flex_direction: FlexDirection::Column, padding: UiRect::new(Val::Px(18.0), Val::Px(18.0), Val::Px(20.0), Val::Px(14.0)), row_gap: Val::Px(10.0), ..default() }, BackgroundColor(Color::srgb(0.06, 0.08, 0.07)), theme.skin("panel")))
                    .with_children(|win| {
                        win.spawn((Content, UiTransform::default(), Node { width: Val::Px(CONTENT_W), height: Val::Px(CONTENT_H), ..default() })).with_children(|c| match tab {
                            Tab::Items => items_tab(c, &view, &d, &mut ui),
                            Tab::Equipment => equipment::build(c, &view, &d),
                            Tab::Stats => stats::build(c, theme, &d),
                            Tab::Abilities => abilities::build(c, theme, &d),
                        });
                        win.spawn((Node { height: Val::Px(8.0), ..default() }, theme.skin("divider")));
                        win.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, column_gap: Val::Px(20.0), ..default() }).with_children(|bar| {
                            let (msg, color) = if ui.message.is_empty() { (hint(tab).to_string(), colors::MUTED) } else { (ui.message.clone(), colors::GOLD) };
                            bar.spawn((text(msg, sizes::SMALL, color), Node { max_width: Val::Px(CONTENT_W - 160.0), ..default() }));
                            bar.spawn(text("Tab: close", sizes::SMALL, colors::FAINT));
                        });
                    });
            });
            // A card about what's under the cursor.
            if held.is_none()
                && let Some(id) = hovered_item(hover, inv, &d.chests, items, d.recipes.as_deref(), d.wielded.0)
            {
                tooltip(root, theme, items, art, id);
            }
            // The held item, following the cursor.
            if let Some(h) = held {
                let (w, hh) = dims(items, h.stack.item, h.rotated);
                let (pw, ph) = (w as f32 * CELL, hh as f32 * CELL);
                root.spawn((
                    HeldIcon,
                    Node { position_type: PositionType::Absolute, width: Val::Px(pw - 2.0), height: Val::Px(ph - 2.0), overflow: Overflow::clip(), ..default() },
                    theme.skin_tinted("item", item_bg(true)),
                    Outline { width: Val::Px(2.0), offset: Val::Px(0.0), color: colors::GOLD },
                    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.6), Val::Px(4.0), Val::Px(6.0), Val::Px(0.0), Val::Px(6.0)),
                    GlobalZIndex(30),
                ))
                .with_children(|c| icon(c, items, art, h.stack, pw - 2.0, ph - 2.0, h.rotated));
            }
        });
}

fn hint(tab: Tab) -> &'static str {
    match tab {
        Tab::Items => "Click: pick up / put down   Drag and drop   R: turn   Right click: move across or wear   Shift-click: half   1-9: hotbar   Q: throw",
        Tab::Equipment => "Click or drag gear onto a slot   Right click: put on / take off   Drag the picture to turn it   R in play: draw your weapon",
        Tab::Stats => "Your body, gear and progress right now",
        Tab::Abilities => "Spells use focus. Learn new ones in the tree   K: open this tab",
    }
}

/// The item card: the sprite large at the top, the name, then its numbers.
fn tooltip(root: &mut ChildSpawnerCommands, theme: &Theme, items: &Items, art: &ItemArt, id: ItemId) {
    let (title, kind, lines) = describe(items, id);
    root.spawn((
        Tooltip,
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            padding: UiRect::axes(Val::Px(14.0), Val::Px(12.0)),
            min_width: Val::Px(220.0),
            max_width: Val::Px(340.0),
            ..default()
        },
        theme.skin("tooltip"),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.55), Val::Px(0.0), Val::Px(6.0), Val::Px(0.0), Val::Px(10.0)),
        GlobalZIndex(31),
    ))
    .with_children(|t| {
        // The picture, as big as fits.
        let size = art.size(id).map_or(Vec2::splat(20.0), |s| s.as_vec2());
        let k = (4.0f32).min(96.0 / size.y).min(200.0 / size.x).max(1.0).floor();
        t.spawn((Node { justify_content: JustifyContent::Center, padding: UiRect::all(Val::Px(6.0)), margin: UiRect::bottom(Val::Px(4.0)), ..default() }, theme.skin("inset")))
            .with_children(|b| {
                b.spawn(Node { width: Val::Px(size.x * k), height: Val::Px(size.y * k), ..default() }).with_children(|q| icon(q, items, art, Stack::new(id, 1), size.x * k + 4.0, size.y * k + 4.0, false));
            });
        t.spawn((theme.bold(sizes::LABEL), Text::new(title), TextColor(colors::HEADING)));
        t.spawn(text(kind, sizes::SMALL, colors::MUTED));
        t.spawn((Node { height: Val::Px(4.0), margin: UiRect::vertical(Val::Px(2.0)), ..default() }, theme.skin("divider")));
        for (ic, s, c) in lines {
            match ic {
                Some(name) => icon_line(t, theme, name, s, c),
                None => {
                    t.spawn(text(s, sizes::SMALL, c));
                }
            }
        }
    });
}

/// The Items tab: the pockets grid with the hand slots over it, and the
/// crafting / chest panel beside it.
fn items_tab(c: &mut ChildSpawnerCommands, view: &SlotView, d: &Data, ui: &mut InvUi) {
    let (theme, items, art, hover, held) = (view.theme, view.items, view.art, view.hover, view.held);
    let inv = &*d.inv;
    c.spawn(Node { column_gap: Val::Px(22.0), align_items: AlignItems::Start, ..default() }).with_children(|row| {
        row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() }).with_children(|col| {
            col.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::End, height: Val::Px(44.0), ..default() }).with_children(|bar| {
                bar.spawn(Node { column_gap: Val::Px(14.0), align_items: AlignItems::Center, ..default() }).with_children(|l| {
                    l.spawn((theme.bold(sizes::HEADING), Text::new("Carried"), TextColor(colors::HEADING)));
                    let heavy = crate::survival::vitals::is_heavy(d.s.env.carried);
                    l.spawn(theme.icon("icon_load"));
                    l.spawn(text(format!("{:.1} / {:.0} kg", d.s.env.carried, crate::survival::vitals::CAPACITY), sizes::BODY, if heavy { colors::BAD } else { colors::MUTED }));
                });
                // The hands and the weapon, right above the grid.
                bar.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|h| {
                    h.spawn((text("Weapon", sizes::SMALL, colors::MUTED), Node { margin: UiRect::right(Val::Px(2.0)), ..default() }));
                    view.slot(h, Hover::Weapon, inv.weapon.map(|w| Stack::new(w, 1)), 44.0);
                    h.spawn((text("Hands", sizes::SMALL, colors::MUTED), Node { margin: UiRect::new(Val::Px(12.0), Val::Px(2.0), Val::ZERO, Val::ZERO), ..default() }));
                    view.slot(h, Hover::RightHand, d.wielded.0.map(|w| Stack::new(w, 1)), 44.0);
                    view.slot(h, Hover::LeftHand, inv.left, 44.0);
                });
            });
            grid_node(col, theme, &inv.pockets, GridRef::Pockets, items, art, hover, held);
        });
        // Side panel: Crafting, and the chest when one is open.
        row.spawn(Node { flex_direction: FlexDirection::Column, width: Val::Px(SIDE_W), ..default() }).with_children(|side| {
            let chest = ui.chest.and_then(|c| d.chests.map.get(&c).map(|g| (c, g)));
            side.spawn(Node { column_gap: Val::Px(6.0), margin: UiRect::bottom(Val::Px(8.0)), height: Val::Px(40.0), align_items: AlignItems::End, ..default() }).with_children(|tabs| {
                for t in [Side::Crafting, Side::Chest] {
                    if t == Side::Chest && chest.is_none() {
                        continue;
                    }
                    let skin = if ui.side == t { "button_down" } else if hover == Some(Hover::Side(t)) { "button_hover" } else { "button" };
                    tabs.spawn((Zone::At(Hover::Side(t)), Node { padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)), ..default() }, theme.skin(skin))).with_children(|b| {
                        b.spawn(text(t.title(), sizes::BODY, if ui.side == t { colors::HEADING } else { colors::TEXT }));
                    });
                }
            });
            match ui.side {
                Side::Chest => {
                    if let Some((c, g)) = chest {
                        grid_node(side, theme, g, GridRef::Chest(c), items, art, hover, held);
                    }
                }
                Side::Crafting => crafting_list(side, theme, items, art, d, ui),
            }
        });
    });
}

fn crafting_list(side: &mut ChildSpawnerCommands, theme: &Theme, items: &Items, art: &ItemArt, d: &Data, ui: &mut InvUi) {
    let hover = ui.hover;
    let at_camp = d.camp.near(d.player.pos);
    side.spawn(Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), ..default() }).with_children(|col| {
        col.spawn(Node { justify_content: JustifyContent::FlexEnd, margin: UiRect::bottom(Val::Px(2.0)), ..default() }).with_children(|h| {
            h.spawn(text(if at_camp { "at camp" } else { "Shift: make 5" }, sizes::SMALL, colors::MUTED));
        });
        let Some(r) = d.recipes.as_deref() else { return };
        let inv = &*d.inv;
        let mut order: Vec<(usize, bool)> = r.list.iter().enumerate().map(|(i, rec)| (i, rec.have_parts(inv) && (rec.at == Station::Hand || at_camp))).collect();
        order.sort_by_key(|(i, can)| (!*can, r.list[*i].at == Station::Camp, *i));
        let rows = order.len() as f32;
        let max_scroll = (rows * (RECIPE_ROW + RECIPE_GAP) + 8.0 - RECIPE_VIEW).max(0.0);
        ui.scroll = ui.scroll.clamp(0.0, max_scroll);
        col.spawn((
            Zone::RecipeList,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(RECIPE_GAP),
                padding: UiRect::all(Val::Px(4.0)),
                height: Val::Px(RECIPE_VIEW),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition(Vec2::new(0.0, ui.scroll)),
            theme.skin("inset"),
        ))
        .with_children(|list| {
            for (i, can) in order {
                let rec = &r.list[i];
                let hovered = hover == Some(Hover::Recipe(i));
                let skin = match (can, hovered) {
                    (true, true) => "button_hover",
                    (true, false) => "button",
                    (false, true) => "slot_hover",
                    (false, false) => "button_off",
                };
                let needs: Vec<String> = rec.needs.iter().map(|n| format!("{} {}", n.count, items.get(n.item).title)).collect();
                let mut needs = needs.join(", ");
                if rec.at == Station::Camp {
                    needs.push_str("  · at camp");
                }
                let title = if rec.makes.count > 1 { format!("{} x{}", items.get(rec.makes.item).title, rec.makes.count) } else { items.get(rec.makes.item).title.clone() };
                list.spawn((
                    Zone::At(Hover::Recipe(i)),
                    Node { min_height: Val::Px(RECIPE_ROW), flex_shrink: 0.0, padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)), column_gap: Val::Px(10.0), align_items: AlignItems::Center, ..default() },
                    theme.skin(skin),
                ))
                .with_children(|row| {
                    row.spawn(Node { width: Val::Px(40.0), height: Val::Px(40.0), flex_shrink: 0.0, ..default() }).with_children(|q| icon(q, items, art, Stack::new(rec.makes.item, 1), 40.0, 40.0, false));
                    row.spawn(Node { flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, ..default() }).with_children(|c| {
                        c.spawn(text(title, sizes::BODY, if can { colors::TEXT } else { colors::MUTED }));
                        c.spawn(text(needs, sizes::SMALL, if can { colors::GOOD } else { colors::FAINT }));
                    });
                });
            }
        });
    });
}

/// The mouse wheel scrolls the crafting list.
fn scroll(mut wheel: MessageReader<bevy::input::mouse::MouseWheel>, mut ui: ResMut<InvUi>, mut lists: Query<&mut ScrollPosition, With<Zone>>) {
    let mut dy = 0.0;
    for w in wheel.read() {
        dy -= match w.unit {
            bevy::input::mouse::MouseScrollUnit::Line => w.y * (RECIPE_ROW + RECIPE_GAP),
            bevy::input::mouse::MouseScrollUnit::Pixel => w.y,
        };
    }
    if dy == 0.0 || !ui.open || !ui.over_list {
        return;
    }
    ui.scroll = (ui.scroll + dy).max(0.0);
    for mut s in &mut lists {
        s.0.y = ui.scroll;
    }
    // Rebuilding clamps it to the list's length.
    ui.dirty = true;
}

#[allow(clippy::type_complexity)]
fn follow_cursor(window: Single<&bevy::window::Window, With<PrimaryWindow>>, mut held: Query<&mut Node, (With<HeldIcon>, Without<Tooltip>)>, mut tips: Query<(&mut Node, &ComputedNode), With<Tooltip>>) {
    let Some(p) = window.cursor_position() else { return };
    for mut n in &mut held {
        // The cursor holds the item by its top-left cell.
        n.left = Val::Px(p.x - CELL * 0.5);
        n.top = Val::Px(p.y - CELL * 0.5);
    }
    let (ww, wh) = (window.width(), window.height());
    for (mut n, c) in &mut tips {
        let size = c.size() * c.inverse_scale_factor();
        // Beside the cursor, kept on screen.
        let x = if p.x + 22.0 + size.x > ww { p.x - 14.0 - size.x } else { p.x + 22.0 };
        let y = (p.y + 10.0).min(wh - size.y - 8.0).max(8.0);
        n.left = Val::Px(x);
        n.top = Val::Px(y);
    }
}

/// Ease out (fast, then settling).
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// The window scales in and the veil fades in when the screen opens; a new
/// tab slides in; slots lift under the cursor and bounce when something lands.
#[allow(clippy::type_complexity)]
fn animate(
    ui: Res<InvUi>,
    motion: Res<Motion>,
    mut window: Query<&mut UiTransform, (With<WinBox>, Without<Content>, Without<Lift>)>,
    mut veil: Query<&mut BackgroundColor, With<Veil>>,
    mut content: Query<&mut UiTransform, (With<Content>, Without<WinBox>, Without<Lift>)>,
    mut lifts: Query<(&Lift, &mut UiTransform), (Without<WinBox>, Without<Content>)>,
) {
    if !ui.open {
        return;
    }
    let now = motion.now;
    let open = ease((now - motion.opened) / 0.16);
    for mut t in &mut window {
        t.scale = Vec2::splat(0.95 + 0.05 * open);
    }
    for mut b in &mut veil {
        b.0 = b.0.with_alpha(0.78 * open);
    }
    let slide = 1.0 - ease((now - motion.tab) / 0.18);
    for mut t in &mut content {
        t.translation = Val2::px(slide * 18.0, 0.0);
    }
    for (l, mut t) in &mut lifts {
        let mut lift = 0.0;
        let mut scale = 1.0;
        if ui.hover == Some(l.0) && ui.held.is_none() {
            let k = ease((now - motion.hover) / 0.1);
            lift = -2.0 * k;
            scale += 0.03 * k;
        }
        if let Some((at, since)) = motion.settle
            && at == l.0
        {
            let s = now - since;
            scale += 0.12 * (-s * 9.0).exp() * (s * 28.0).sin();
        }
        let want = UiTransform { translation: Val2::px(0.0, lift), scale: Vec2::splat(scale), ..default() };
        if *t != want {
            *t = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::tests::load;

    #[test]
    fn put_down_merges_places_or_swaps() {
        let (_, items) = load();
        let id = |n: &str| items.id(n).unwrap();
        let mut g = Grid::new(4, 3);
        g.place(&items, Stack::new(id("dirt"), 30), 0, 0, false);
        g.place(&items, Stack::new(id("stone_axe"), 1), 3, 0, false);
        let held = |s: Stack| Held { stack: s, rotated: false, from: Origin::Worn };
        // Onto a matching stack: tops it up to 32, the rest stays in hand.
        let rest = drop_into(&mut g, &items, held(Stack::new(id("dirt"), 5)), (0, 0), GridRef::Pockets).unwrap();
        assert_eq!(rest.unwrap().stack.count, 3);
        // Into free space.
        assert!(drop_into(&mut g, &items, held(Stack::new(id("torch"), 4)), (1, 0), GridRef::Pockets).unwrap().is_none());
        // A shovel onto the axe swaps them.
        let swapped = drop_into(&mut g, &items, held(Stack::new(id("stone_shovel"), 1)), (3, 0), GridRef::Pockets).unwrap().unwrap();
        assert_eq!(swapped.stack.item, id("stone_axe"));
        // No room for a 2x3 pickaxe in the middle.
        assert!(drop_into(&mut g, &items, held(Stack::new(id("stone_pickaxe"), 1)), (1, 1), GridRef::Pockets).is_err());
    }

    #[test]
    fn slots_take_only_what_fits() {
        let (_, items) = load();
        let id = |n: &str| items.id(n).unwrap();
        assert!(accepts(&items, Hover::Weapon, id("iron_sword")));
        assert!(!accepts(&items, Hover::Weapon, id("stone_axe")));
        assert!(accepts(&items, Hover::Slot(WearSlot::Body), id("linen_shirt")));
        assert!(!accepts(&items, Hover::Slot(WearSlot::BodyArmor), id("linen_shirt")));
        // The left hand takes anything.
        assert!(accepts(&items, Hover::LeftHand, id("dirt")));
    }

    #[test]
    fn left_hand_tops_up_or_swaps() {
        let (_, items) = load();
        let id = |n: &str| items.id(n).unwrap();
        let mut inv = Inventory::empty();
        let held = |s: Stack| Held { stack: s, rotated: false, from: Origin::Worn };
        assert!(into_left_hand(&mut inv, &items, held(Stack::new(id("torch"), 3))).is_none());
        assert!(into_left_hand(&mut inv, &items, held(Stack::new(id("torch"), 2))).is_none());
        assert_eq!(inv.left, Some(Stack::new(id("torch"), 5)));
        let back = into_left_hand(&mut inv, &items, held(Stack::new(id("dirt"), 1))).unwrap();
        assert_eq!(back.stack, Stack::new(id("torch"), 5));
        assert_eq!(back.from, Origin::LeftHand);
    }

    #[test]
    fn right_click_puts_a_weapon_in_its_slot() {
        let (_, items) = load();
        let id = |n: &str| items.id(n).unwrap();
        let mut inv = Inventory::empty();
        let mut chests = Chests::default();
        let mut msg = String::new();
        let rest = move_across(&mut inv, &mut chests, &items, Stack::new(id("iron_sword"), 1), GridRef::Pockets, None, &mut msg);
        assert!(rest.is_none());
        assert_eq!(inv.weapon, Some(id("iron_sword")));
        // Another weapon swaps the first back into the pockets.
        move_across(&mut inv, &mut chests, &items, Stack::new(id("rapier"), 1), GridRef::Pockets, None, &mut msg);
        assert_eq!(inv.weapon, Some(id("rapier")));
        assert_eq!(inv.count(id("iron_sword")), 1);
    }
}
