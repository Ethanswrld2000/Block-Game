//! The inventory screen (Tab): what you wear, your pockets and bag, an open
//! chest, and crafting.
//!
//! Click an item to pick it up, click again to put it down; R turns it while
//! you hold it. Right click moves an item straight across (to the chest and
//! back, or onto your body). Hover an item and press 1-9 to put it on the
//! hotbar. Q throws what you hold on the ground. Shift-click picks up half a
//! stack.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use voxel::BlockPos;

use super::crafting::{Recipes, Station};
use super::grid::{dims, Grid, Stack};
use super::world_items::Spill;
use super::{Chests, Inventory};
use crate::item_art::ItemArt;
use crate::camera::{FlyCamera, Menus};
use crate::interact::{Hotbar, OpenChest};
use crate::items::{ItemKind, Items, WearSlot};
use crate::player::Player;
use crate::survival::camp::{Camp, Sleep};
use crate::survival::health::Layer;
use crate::survival::Survival;
use crate::theme::{self, colors, sizes, Theme};

const CELL: f32 = 46.0;

pub struct InventoryUiPlugin;

impl Plugin for InventoryUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InvUi>()
            .add_systems(crate::menu::GamePostStartup, open_for_screenshot)
            .add_systems(crate::menu::GameUpdate, (open_close, hover, scroll, click, keys, rebuild, follow_cursor).chain().after(crate::interact::InteractSet));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridRef {
    Pockets,
    Bag,
    Chest(BlockPos),
}

/// Where a held item came from, so closing the screen can put it back.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Origin {
    Grid(GridRef, u8, u8, bool),
    Worn,
}

#[derive(Debug, Clone, Copy)]
struct Held {
    stack: Stack,
    rotated: bool,
    from: Origin,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Hover {
    Cell(GridRef, u8, u8),
    Slot(WearSlot),
    Recipe(usize),
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
}

#[derive(Component)]
struct InvRoot;

/// Hit-test regions.
#[derive(Component, Clone, Copy)]
enum Zone {
    Grid(GridRef, u8, u8),
    Slot(WearSlot),
    Recipe(usize),
    RecipeList,
}

#[derive(Component)]
struct HeldIcon;

#[derive(Component)]
struct Tooltip;

#[derive(Component)]
struct HintText;

fn open_for_screenshot(mut ui: ResMut<InvUi>, mut menus: ResMut<Menus>) {
    if std::env::var_os("BLOCKGAME_INVENTORY").is_some() {
        ui.open = true;
        ui.dirty = true;
        menus.open = true;
    }
}

fn grid_mut<'a>(inv: &'a mut Inventory, chests: &'a mut Chests, g: GridRef) -> Option<&'a mut Grid> {
    match g {
        GridRef::Pockets => Some(&mut inv.pockets),
        GridRef::Bag => inv.bag.as_mut(),
        GridRef::Chest(p) => Some(chests.grid(p)),
    }
}

#[allow(clippy::too_many_arguments)]
fn open_close(
    keys: Res<ButtonInput<KeyCode>>,
    mut chest_msgs: MessageReader<OpenChest>,
    mut ui: ResMut<InvUi>,
    mut menus: ResMut<Menus>,
    s: Res<Survival>,
    sleep: Res<Sleep>,
    items: Res<Items>,
    player: Res<Player>,
    cam: Query<&Transform, With<FlyCamera>>,
    mut inv: ResMut<Inventory>,
    mut chests: ResMut<Chests>,
    mut spill: MessageWriter<Spill>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    loot: Option<Res<super::loot::Loot>>,
    generator: Option<Res<crate::world::Generator>>,
    mut seed: Local<u64>,
) {
    let busy = s.vitals.health.is_dead() || sleep.active();
    let mut want = ui.open;
    if keys.just_pressed(KeyCode::Tab) || (ui.open && (keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::KeyI))) {
        want = !ui.open;
    } else if keys.just_pressed(KeyCode::KeyI) {
        want = true;
    }
    for m in chest_msgs.read() {
        want = true;
        ui.chest = Some(m.0);
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
    if let Some(c) = ui.chest
        && (Vec3::new(c.x as f32 + 0.5, c.y as f32 + 0.5, c.z as f32 + 0.5).distance(player.pos + Vec3::Y) > 6.0 || !chests.map.contains_key(&c))
    {
        ui.chest = None;
        ui.dirty = true;
    }
    if busy {
        want = false;
    }
    if want == ui.open {
        return;
    }
    ui.open = want;
    ui.dirty = true;
    menus.open = want;
    if !want {
        // Put back whatever is in hand.
        if let Some(h) = ui.held.take() {
            let mut back = false;
            if let Origin::Grid(g, x, y, r) = h.from
                && let Some(grid) = grid_mut(&mut inv, &mut chests, g)
            {
                back = grid.place(&items, h.stack, x, y, r);
            }
            if !back && let Some(left) = inv.add(&items, h.stack) {
                let at = cam.single().map(|t| t.translation).unwrap_or(player.pos);
                spill.write(Spill { stack: left, at });
            }
        }
        ui.chest = None;
        ui.message.clear();
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

fn hover(
    mut ui: ResMut<InvUi>,
    window: Single<&Window, With<PrimaryWindow>>,
    zones: Query<(&Zone, &ComputedNode, &UiGlobalTransform)>,
) {
    if !ui.open {
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
            Zone::Slot(s) => Hover::Slot(s),
            Zone::Recipe(i) => Hover::Recipe(i),
        });
    }
    if ui.over_list != over_list {
        ui.over_list = over_list;
    }
    if ui.hover != found {
        ui.hover = found;
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

#[allow(clippy::too_many_arguments)]
fn click(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    items: Res<Items>,
    recipes: Option<Res<Recipes>>,
    camp: Res<Camp>,
    player: Res<Player>,
    mut ui: ResMut<InvUi>,
    mut inv: ResMut<Inventory>,
    mut chests: ResMut<Chests>,
    mut spill: MessageWriter<Spill>,
    (mut gain, prog): (MessageWriter<crate::progress::GainXp>, Option<Res<crate::progress::xp::Progression>>),
) {
    if !ui.open {
        return;
    }
    let left = mouse.just_pressed(MouseButton::Left);
    let right = mouse.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    let Some(hover) = ui.hover else { return };
    ui.dirty = true;
    ui.message.clear();
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    match hover {
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
        Hover::Cell(g, x, y) => {
            // Right click: move straight across.
            if ui.held.is_some() {
                return;
            }
            let chest = ui.chest;
            let Some(grid) = grid_mut(&mut inv, &mut chests, g) else { return };
            let Some(i) = grid.at(&items, x, y) else { return };
            let p = grid.items.remove(i);
            let def = items.get(p.stack.item);
            let result = match (g, chest) {
                (GridRef::Chest(_), _) => inv.add(&items, p.stack),
                (_, Some(c)) => chests.grid(c).add(&items, p.stack),
                _ if def.wear_slot().is_some() => match inv.wear(&items, p.stack.item) {
                    Ok(old) => old.and_then(|o| inv.add(&items, Stack::new(o, 1))),
                    Err(e) => {
                        ui.message = e.into();
                        Some(p.stack)
                    }
                },
                _ => Some(p.stack),
            };
            if let Some(rest) = result {
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
                    Ok(old) => ui.held = old.map(|o| Held { stack: Stack::new(o, 1), rotated: false, from: Origin::Worn }),
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
        Hover::Recipe(i) => {
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

fn text(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
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
const RECIPE_VIEW: f32 = 430.0;

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

#[allow(clippy::too_many_arguments)]
fn grid_node(p: &mut ChildSpawnerCommands, theme: &Theme, title: String, grid: &Grid, g: GridRef, items: &Items, art: &ItemArt, hover: Option<Hover>, held: Option<Held>) {
    p.spawn((theme.bold(sizes::LABEL), Text::new(title), TextColor(colors::HEADING), Node { margin: UiRect::bottom(Val::Px(2.0)), ..default() }));
    p.spawn((Node { padding: UiRect::all(Val::Px(4.0)), margin: UiRect::bottom(Val::Px(10.0)), ..default() }, theme.skin("inset"))).with_children(|frame| {
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

/// An item's name and what it's for.
fn describe(items: &Items, id: crate::items::ItemId) -> (String, String) {
    let it = items.get(id);
    let mut s = format!("{:.1} kg, takes {}x{}", it.mass, it.size.0, it.size.1);
    if it.stack > 1 {
        s.push_str(&format!(", stacks to {}", it.stack));
    }
    match &it.kind {
        ItemKind::Wear { slot, insulation, protection } => {
            s.push_str(&format!("\nWorn on: {}", slot.title()));
            if *insulation > 0.0 {
                s.push_str(&format!("\nWarmth +{insulation:.0}°C"));
            }
            for l in Layer::ALL {
                if let Some(p) = protection.get(&l) {
                    s.push_str(&format!("\n{} -{:.0}%", l.name(), p * 100.0));
                }
            }
        }
        ItemKind::Bag { grid } => s.push_str(&format!("\nWorn on your back; holds {}x{}", grid.0, grid.1)),
        ItemKind::Food { tier, satiety } => s.push_str(&format!("\nFood, tier {tier}, +{satiety:.0} food\nHold right click to eat")),
        ItemKind::Tool { speeds } => {
            let mut v: Vec<_> = speeds.iter().collect();
            v.sort_by(|a, b| a.0.cmp(b.0));
            s.push_str("\nTool: ");
            s.push_str(&v.iter().map(|(m, x)| format!("{m} x{x}")).collect::<Vec<_>>().join(", "));
        }
        ItemKind::Block(_) => s.push_str("\nRight click to place"),
        ItemKind::Torch => s.push_str("\nGives light; right click to place"),
        ItemKind::Material => s.push_str("\nCrafting material"),
    }
    if it.damage > 1.0 {
        s.push_str(&format!("\nHits for {:.0}", it.damage));
    }
    (it.title.clone(), s)
}

/// A heading for a column.
fn heading(p: &mut ChildSpawnerCommands, theme: &Theme, s: &str) {
    p.spawn((theme.bold(sizes::LABEL), Text::new(s), TextColor(colors::HEADING), Node { margin: UiRect::bottom(Val::Px(2.0)), ..default() }));
}

/// A line with an icon in front.
fn icon_line(p: &mut ChildSpawnerCommands, theme: &Theme, icon_name: &str, s: String, color: Color) {
    p.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
        r.spawn(theme.icon(icon_name));
        r.spawn(text(s, sizes::BODY, color));
    });
}

#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    mut ui: ResMut<InvUi>,
    theme: Res<Theme>,
    (inv, chests, items): (Res<Inventory>, Res<Chests>, Res<Items>),
    art: Option<Res<ItemArt>>,
    recipes: Option<Res<Recipes>>,
    camp: Res<Camp>,
    player: Res<Player>,
    s: Res<Survival>,
    roots: Query<Entity, With<InvRoot>>,
) {
    if !(ui.dirty || (ui.open && (inv.is_changed() || chests.is_changed()))) {
        return;
    }
    ui.dirty = false;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let (Some(art), true) = (art, ui.open) else { return };
    let (hover, held) = (ui.hover, ui.held);
    let at_camp = camp.near(player.pos);
    let theme = &*theme;
    commands
        .spawn((
            InvRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.01, 0.0, 0.8)),
            GlobalZIndex(20),
        ))
        .with_children(|root| {
            root.spawn((Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(18.0)), row_gap: Val::Px(12.0), ..default() }, theme.skin("panel")))
                .with_children(|win| {
                    // Title bar.
                    win.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|bar| {
                        bar.spawn((theme.bold(sizes::HEADING), Text::new(if ui.chest.is_some() { "Inventory and chest" } else { "Inventory" }), TextColor(colors::HEADING)));
                        bar.spawn(text("Tab to close", sizes::SMALL, colors::MUTED));
                    });
                    win.spawn((Node { height: Val::Px(8.0), ..default() }, theme.skin("divider")));
                    win.spawn(Node { column_gap: Val::Px(22.0), align_items: AlignItems::Start, ..default() }).with_children(|row| {
                        // Worn.
                        row.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), width: Val::Px(250.0), ..default() }).with_children(|col| {
                            heading(col, theme, "Worn");
                            for slot in WearSlot::ALL {
                                let worn = inv.worn.get(&slot);
                                let hovered = hover == Some(Hover::Slot(slot));
                                let node = Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(40.0),
                                    padding: UiRect::horizontal(Val::Px(10.0)),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::SpaceBetween,
                                    ..default()
                                };
                                let mut e = match worn {
                                    Some(_) => col.spawn((Zone::Slot(slot), node, theme.skin_tinted("item", item_bg(false)))),
                                    None => col.spawn((Zone::Slot(slot), node, theme.skin(if hovered { "slot_hover" } else { "slot" }))),
                                };
                                if hovered {
                                    e.insert(Outline { width: Val::Px(2.0), offset: Val::Px(0.0), color: colors::GOLD });
                                }
                                e.with_children(|c| {
                                    c.spawn(text(slot.title(), sizes::SMALL, colors::MUTED));
                                    match worn {
                                        Some(i) => c.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                                            r.spawn(text(items.get(*i).title.clone(), sizes::BODY, colors::TEXT));
                                            r.spawn(Node { width: Val::Px(34.0), height: Val::Px(34.0), ..default() }).with_children(|q| icon(q, &items, &art, Stack::new(*i, 1), 34.0, 34.0, false));
                                        }).id(),
                                        None => c.spawn(text("empty", sizes::BODY, colors::FAINT)).id(),
                                    };
                                });
                            }
                            // What you're wearing adds up to.
                            col.spawn((
                                Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), padding: UiRect::all(Val::Px(10.0)), margin: UiRect::top(Val::Px(8.0)), ..default() },
                                theme.skin("inset"),
                            ))
                            .with_children(|st| {
                                icon_line(st, theme, "icon_warmth", format!("Clothes +{:.0}°C, feels {:.0}°C", inv.insulation(&items), s.env.feels), colors::TEXT);
                                let heavy = s.env.carried > crate::survival::vitals::CAPACITY;
                                icon_line(st, theme, "icon_load", format!("Load {:.1} of {:.0} kg", s.env.carried, crate::survival::vitals::CAPACITY), if heavy { colors::BAD } else { colors::TEXT });
                                let prot: Vec<String> = Layer::ALL
                                    .iter()
                                    .filter_map(|l| {
                                        let p = inv.protection(&items, *l);
                                        (p > 0.0).then(|| format!("{} -{:.0}%", l.name(), p * 100.0))
                                    })
                                    .collect();
                                icon_line(st, theme, "icon_injuries", if prot.is_empty() { "No armour".into() } else { prot.join(", ") }, colors::TEXT);
                            });
                        });
                        // Grids.
                        row.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|col| {
                            grid_node(col, theme, "Pockets".into(), &inv.pockets, GridRef::Pockets, &items, &art, hover, held);
                            match (&inv.bag, inv.worn.get(&WearSlot::Back)) {
                                (Some(b), Some(i)) => grid_node(col, theme, items.get(*i).title.clone(), b, GridRef::Bag, &items, &art, hover, held),
                                _ => {
                                    heading(col, theme, "Bag");
                                    col.spawn(text("No bag. Wear one on your back\nto carry more.", sizes::BODY, colors::FAINT));
                                }
                            }
                            if let Some(c) = ui.chest
                                && let Some(g) = chests.map.get(&c)
                            {
                                grid_node(col, theme, "Chest".into(), g, GridRef::Chest(c), &items, &art, hover, held);
                            }
                        });
                        // Crafting: what you can make now first.
                        row.spawn(Node { flex_direction: FlexDirection::Column, width: Val::Px(380.0), ..default() }).with_children(|col| {
                            col.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|h| {
                                heading(h, theme, "Crafting");
                                h.spawn(text(if at_camp { "at camp" } else { "Shift: make 5" }, sizes::SMALL, colors::MUTED));
                            });
                            let Some(r) = &recipes else { return };
                            let mut order: Vec<(usize, bool)> =
                                r.list.iter().enumerate().map(|(i, rec)| (i, rec.have_parts(&inv) && (rec.at == Station::Hand || at_camp))).collect();
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
                                        Zone::Recipe(i),
                                        Node {
                                            min_height: Val::Px(RECIPE_ROW),
                                            flex_shrink: 0.0,
                                            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                                            column_gap: Val::Px(10.0),
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        theme.skin(skin),
                                    ))
                                    .with_children(|row| {
                                        row.spawn(Node { width: Val::Px(40.0), height: Val::Px(40.0), flex_shrink: 0.0, ..default() })
                                            .with_children(|q| icon(q, &items, &art, Stack::new(rec.makes.item, 1), 40.0, 40.0, false));
                                        row.spawn(Node { flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, ..default() }).with_children(|c| {
                                            c.spawn(text(title, sizes::BODY, if can { colors::TEXT } else { colors::MUTED }));
                                            c.spawn(text(needs, sizes::SMALL, if can { colors::GOOD } else { colors::FAINT }));
                                        });
                                    });
                                }
                            });
                        });
                    });
                    // Messages and controls.
                    win.spawn((Node { height: Val::Px(8.0), ..default() }, theme.skin("divider")));
                    win.spawn((HintText, text(ui.message.clone(), sizes::BODY, colors::GOLD)));
                    win.spawn((
                        text("Click: pick up / put down    R: turn    Right click: move across or wear    Shift-click: half a stack    1-9: put on hotbar    Q: throw away", sizes::SMALL, colors::MUTED),
                        Node { max_width: Val::Px(1080.0), ..default() },
                    ));
                });
            // A card about what's under the cursor.
            let hovered_item = match hover {
                Some(Hover::Cell(g, x, y)) => {
                    let grid = match g {
                        GridRef::Pockets => Some(&inv.pockets),
                        GridRef::Bag => inv.bag.as_ref(),
                        GridRef::Chest(p) => chests.map.get(&p),
                    };
                    grid.and_then(|gr| gr.at(&items, x, y).map(|i| gr.items[i].stack.item))
                }
                Some(Hover::Slot(s)) => inv.worn.get(&s).copied(),
                Some(Hover::Recipe(i)) => recipes.as_ref().and_then(|r| r.list.get(i)).map(|r| r.makes.item),
                None => None,
            };
            if held.is_none()
                && let Some(id) = hovered_item
            {
                let (title, body) = describe(&items, id);
                root.spawn((
                    Tooltip,
                    Node {
                        position_type: PositionType::Absolute,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
                        max_width: Val::Px(320.0),
                        ..default()
                    },
                    theme.skin("tooltip"),
                    GlobalZIndex(31),
                ))
                .with_children(|t| {
                    t.spawn((theme.bold(sizes::LABEL), Text::new(title), TextColor(colors::HEADING)));
                    t.spawn(text(body, sizes::SMALL, colors::TEXT));
                });
            }
            // The held item, following the cursor.
            if let Some(h) = held {
                let (w, hh) = dims(&items, h.stack.item, h.rotated);
                let (pw, ph) = (w as f32 * CELL, hh as f32 * CELL);
                root.spawn((
                    HeldIcon,
                    Node { position_type: PositionType::Absolute, width: Val::Px(pw - 2.0), height: Val::Px(ph - 2.0), overflow: Overflow::clip(), ..default() },
                    theme.skin_tinted("item", item_bg(true)),
                    Outline { width: Val::Px(2.0), offset: Val::Px(0.0), color: colors::GOLD },
                    GlobalZIndex(30),
                ))
                .with_children(|c| icon(c, &items, &art, h.stack, pw - 2.0, ph - 2.0, h.rotated));
            }
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
fn follow_cursor(window: Single<&Window, With<PrimaryWindow>>, mut held: Query<&mut Node, (With<HeldIcon>, Without<Tooltip>)>, mut tips: Query<(&mut Node, &ComputedNode), With<Tooltip>>) {
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
}
