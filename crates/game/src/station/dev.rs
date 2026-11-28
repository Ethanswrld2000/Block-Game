//! The dev menu (F9): test stations, items and NPCs without gathering for
//! them. Three tabs:
//!
//! - Urns: one row per urn. Give urn (its item, to set down yourself), Set down
//!   (on the ground in front of you), Items (what its next step still needs),
//!   Next step / All steps (finish steps; they give what they give), Reset
//!   (empty the urn and forget its steps).
//! - Items: every item as a picture, filtered by kind and paged (wheel or the
//!   arrows). Click one to get it: 1, 8 or a full stack. Empty pockets clears
//!   pockets, bag, left hand and weapon slot (clothes stay on).
//! - NPCs: one button per kind in npcs.ron; it stands in front of you and
//!   follows you (`npc::SpawnKind`).
//!
//! `BLOCKGAME_DEVMENU=1` (or `=urns|items|npcs`) opens it for screenshots.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::urns::State;
use super::{StepDone, UrnBook, Urns, placed_facing};
use crate::camera::{FlyCamera, Menus};
use crate::clock::{Seasons, WorldClock};
use crate::inventory::grid::Stack;
use crate::inventory::{Inventory, Spill};
use crate::item_art::ItemArt;
use crate::items::{ItemId, ItemKind, Items};
use crate::npc::{NpcBook, SpawnKind};
use crate::survival::Toast;
use crate::theme::{Theme, colors, sizes, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Urns,
    Items,
    Npcs,
    World,
}

const TABS: [(Tab, &str); 4] = [
    (Tab::Urns, "Urns"),
    (Tab::Items, "Items"),
    (Tab::Npcs, "NPCs"),
    (Tab::World, "World"),
];

/// Which items the Items tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Cat {
    #[default]
    All,
    Tools,
    Blocks,
    Food,
    Materials,
    Clothes,
    Other,
}

const CATS: [(Cat, &str); 7] = [
    (Cat::All, "All"),
    (Cat::Tools, "Tools"),
    (Cat::Blocks, "Blocks"),
    (Cat::Food, "Food"),
    (Cat::Materials, "Materials"),
    (Cat::Clothes, "Clothes"),
    (Cat::Other, "Other"),
];

fn cat_of(kind: &ItemKind) -> Cat {
    match kind {
        ItemKind::Tool { .. } => Cat::Tools,
        ItemKind::Block(_) => Cat::Blocks,
        ItemKind::Food { .. } => Cat::Food,
        ItemKind::Material => Cat::Materials,
        ItemKind::Wear { .. } | ItemKind::Bag { .. } => Cat::Clothes,
        ItemKind::Torch | ItemKind::Machine(_) => Cat::Other,
    }
}

fn kind_name(kind: &ItemKind) -> &'static str {
    match kind {
        ItemKind::Tool { .. } => "tool",
        ItemKind::Block(_) => "block",
        ItemKind::Food { .. } => "food",
        ItemKind::Material => "material",
        ItemKind::Wear { .. } => "clothes",
        ItemKind::Bag { .. } => "bag",
        ItemKind::Torch => "torch",
        ItemKind::Machine(_) => "machine",
    }
}

/// How many one click on an item gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Amount {
    #[default]
    One,
    Eight,
    Stack,
}

const AMOUNTS: [(Amount, &str); 3] = [
    (Amount::One, "1"),
    (Amount::Eight, "8"),
    (Amount::Stack, "Stack"),
];

/// Item tiles per row and rows per page.
const COLS: usize = 12;
const ROWS: usize = 6;
const PER_PAGE: usize = COLS * ROWS;
const TILE: f32 = 56.0;

#[derive(Resource, Default)]
pub struct DevMenu {
    pub open: bool,
    dirty: bool,
    tab: Tab,
    cat: Cat,
    amount: Amount,
    page: usize,
}

#[derive(Component)]
pub(super) struct DevRoot;

#[derive(Component)]
pub(super) struct DevPanel;

/// The line under the item grid: what the cursor is over, or the page.
#[derive(Component)]
pub(super) struct DevHint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Give,
    SetDown,
    Items,
    Next,
    All,
    Reset,
}

const ACTS: [(Act, &str); 6] = [
    (Act::Give, "Give urn"),
    (Act::SetDown, "Set down"),
    (Act::Items, "Items"),
    (Act::Next, "Next step"),
    (Act::All, "All steps"),
    (Act::Reset, "Reset"),
];

#[derive(Debug, Clone, Copy, PartialEq)]
enum Press {
    Tab(Tab),
    Urn(usize, Act),
    Cat(Cat),
    Amount(Amount),
    Item(ItemId),
    Page(i32),
    Empty,
    Npc(usize),
    Season(usize),
}

/// How a button is drawn: plain, hovered, and when it is the chosen one (or held down).
#[derive(Debug, Clone, Copy)]
enum Look {
    Button,
    Tab,
    Slot,
}

impl Look {
    fn skins(self) -> [&'static str; 3] {
        match self {
            Look::Button => ["button", "button_hover", "button_down"],
            Look::Tab => ["tab", "tab_hover", "tab_active"],
            Look::Slot => ["slot", "slot_hover", "slot_selected"],
        }
    }
}

#[derive(Component)]
pub(super) struct DevButton {
    press: Press,
    look: Look,
    /// Shown as chosen (the open tab, the filter, the amount).
    on: bool,
}

fn button(press: Press, look: Look, on: bool) -> DevButton {
    DevButton { press, look, on }
}

pub(super) fn spawn_dev_ui(mut commands: Commands) {
    commands
        .spawn((
            DevRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.45)),
            GlobalZIndex(26),
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn((
                DevPanel,
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
            ));
        });
}

/// F9 opens and closes the dev menu; Esc closes it.
pub(super) fn dev_keys(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut dev: ResMut<DevMenu>,
    mut menus: ResMut<Menus>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    (urns, inv): (Res<Urns>, Res<Inventory>),
    mut forced: Local<bool>,
) {
    if !*forced {
        *forced = true;
        if let Ok(v) = std::env::var("BLOCKGAME_DEVMENU") {
            dev.tab = match v.as_str() {
                "items" => Tab::Items,
                "npcs" => Tab::Npcs,
                "world" => Tab::World,
                _ => Tab::Urns,
            };
            dev.open = true;
            dev.dirty = true;
            menus.open = true;
        }
    }
    let toggle = keys.just_pressed(KeyCode::F9);
    if dev.open && (toggle || keys.just_pressed(KeyCode::Escape)) {
        keys.clear_just_pressed(KeyCode::Escape);
        dev.open = false;
        dev.dirty = true;
        menus.open = false;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        return;
    }
    if toggle && !menus.open {
        dev.open = true;
        dev.dirty = true;
        menus.open = true;
    }
    if dev.open {
        if !cursor.visible {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
        // The Urns tab shows progress; the Items tab doesn't need a rebuild per item given.
        if dev.tab == Tab::Urns && (urns.is_changed() || inv.is_changed()) {
            dev.dirty = true;
        }
    }
}

/// The items the Items tab lists under the current filter.
fn shown_items(items: &Items, cat: Cat) -> Vec<ItemId> {
    items
        .ids()
        .filter(|&id| cat == Cat::All || cat_of(&items.get(id).kind) == cat)
        .collect()
}

fn pages(n: usize) -> usize {
    n.div_ceil(PER_PAGE).max(1)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dev_click(
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    window: Single<&Window, With<PrimaryWindow>>,
    theme: Res<Theme>,
    mut buttons: Query<(
        &DevButton,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
        &mut ImageNode,
    )>,
    mut hint: Query<&mut Text, With<DevHint>>,
    (book, items, mut dev): (Option<Res<UrnBook>>, Res<Items>, ResMut<DevMenu>),
    (player, cam): (
        Res<crate::player::Player>,
        Single<&Transform, With<FlyCamera>>,
    ),
    (mut inv, mut urns): (ResMut<Inventory>, ResMut<Urns>),
    (mut done, mut spill, mut toast, mut npc): (
        MessageWriter<StepDone>,
        MessageWriter<Spill>,
        MessageWriter<Toast>,
        MessageWriter<SpawnKind>,
    ),
    (npcs, seasons, mut clock): (Res<NpcBook>, Res<Seasons>, ResMut<WorldClock>),
) {
    let scrolled: f32 = wheel.read().map(|w| w.y).sum();
    let Some(book) = book else { return };
    if !dev.open {
        return;
    }
    let p = window.physical_cursor_position();
    let mut clicked = None;
    let mut hovered_item = None;
    for (b, node, tf, vis, mut img) in &mut buttons {
        let over = vis.get() && p.is_some_and(|p| node.contains_point(*tf, p));
        let [plain, hover, chosen] = b.look.skins();
        let want = if b.on || (over && mouse.pressed(MouseButton::Left)) {
            chosen
        } else if over {
            hover
        } else {
            plain
        };
        let skin = theme.image(want);
        if img.rect != skin.rect {
            *img = skin;
        }
        if over {
            if let Press::Item(id) = b.press {
                hovered_item = Some(id);
            }
            if mouse.just_pressed(MouseButton::Left) {
                clicked = Some(b.press);
            }
        }
    }

    // Items tab: the line under the grid, and the wheel turning pages.
    if dev.tab == Tab::Items {
        let total = pages(shown_items(&items, dev.cat).len());
        if scrolled != 0.0 {
            let page = if scrolled < 0.0 {
                (dev.page + 1).min(total - 1)
            } else {
                dev.page.saturating_sub(1)
            };
            if page != dev.page {
                dev.page = page;
                dev.dirty = true;
            }
        }
        if let Ok(mut t) = hint.single_mut() {
            let line = match hovered_item {
                Some(id) => {
                    let it = items.get(id);
                    format!(
                        "{} ({}, stacks to {}) - you have {}",
                        it.title,
                        kind_name(&it.kind),
                        it.stack,
                        inv.count(id)
                    )
                }
                None => format!(
                    "Page {} of {}. Wheel or the arrows turn pages.",
                    dev.page + 1,
                    total
                ),
            };
            if t.0 != line {
                t.0 = line;
            }
        }
    }

    let Some(press) = clicked else { return };
    let mut give = |inv: &mut Inventory, name: &str, n: u32| {
        let Some(id) = items.id(name) else { return };
        let mut left = n;
        while left > 0 {
            let k = left.min(items.get(id).stack as u32).max(1);
            if let Some(rest) = inv.add(&items, Stack::new(id, k as u16)) {
                spill.write(Spill {
                    stack: rest,
                    at: player.pos + Vec3::Y,
                });
            }
            left -= k;
        }
    };
    match press {
        Press::Tab(t) => {
            dev.tab = t;
            dev.dirty = true;
        }
        Press::Cat(c) => {
            dev.cat = c;
            dev.page = 0;
            dev.dirty = true;
        }
        Press::Amount(a) => {
            dev.amount = a;
            dev.dirty = true;
        }
        Press::Page(d) => {
            let total = pages(shown_items(&items, dev.cat).len()) as i32;
            dev.page = (dev.page as i32 + d).clamp(0, total - 1) as usize;
            dev.dirty = true;
        }
        Press::Item(id) => {
            let it = items.get(id);
            let n = match dev.amount {
                Amount::One => 1,
                Amount::Eight => 8,
                Amount::Stack => it.stack.max(1) as u32,
            };
            give(&mut inv, &it.name, n);
            toast.write(Toast(format!("Dev: gave you {n} {}", it.title)));
        }
        Press::Empty => {
            inv.empty_carried();
            toast.write(Toast(
                "Dev: emptied your pockets, bag, hands and weapon slot".to_string(),
            ));
        }
        Press::Npc(k) => {
            npc.write(SpawnKind(k));
            if let Some(kind) = npcs.kinds.get(k) {
                toast.write(Toast(format!("Dev: {} is in front of you", kind.name)));
            }
        }
        Press::Season(k) => {
            clock.day = seasons.next_start(clock.day, k);
            dev.dirty = true;
            toast.write(Toast(format!(
                "Dev: it is now {} (day {})",
                seasons.list[k].title,
                clock.day + 1
            )));
        }
        Press::Urn(i, act) => urn_act(
            i, act, &book, &mut urns, &mut inv, &mut give, &player, &cam, &mut done, &mut toast,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn urn_act(
    i: usize,
    act: Act,
    book: &UrnBook,
    urns: &mut Urns,
    inv: &mut Inventory,
    give: &mut impl FnMut(&mut Inventory, &str, u32),
    player: &crate::player::Player,
    cam: &Transform,
    done: &mut MessageWriter<StepDone>,
    toast: &mut MessageWriter<Toast>,
) {
    let def = &book.0.urns[i];
    match act {
        Act::Give => {
            if let Some(it) = &def.item {
                give(inv, it, 1);
                toast.write(Toast(format!("Dev: gave you the {}", def.title)));
            }
        }
        Act::SetDown => {
            if def.item.is_some() {
                let mut f = cam.forward().as_vec3();
                f.y = 0.0;
                let f = f.normalize_or(Vec3::NEG_Z);
                urns.placed
                    .insert(def.name.clone(), placed_facing(player.pos + f * 3.0, f));
                toast.write(Toast(format!(
                    "Dev: set the {} down in front of you",
                    def.title
                )));
            }
        }
        Act::Items => {
            let p = urns.of(&def.name).clone();
            match p.next(def) {
                Some(s) => {
                    for (name, n) in p.missing(s) {
                        give(inv, &name, n);
                    }
                    toast.write(Toast(format!("Dev: gave you what the {} needs", s.title)));
                }
                None => {
                    toast.write(Toast(format!("Dev: the {} has no open step", def.title)));
                }
            }
        }
        Act::Next | Act::All => loop {
            let p = urns.of_mut(&def.name);
            let Some(s) = def.steps.iter().find(|s| p.state(s) == State::Open) else {
                break;
            };
            p.finish(&s.name);
            done.write(StepDone(s.name.clone()));
            if act == Act::Next {
                break;
            }
        },
        Act::Reset => {
            urns.progress.remove(&def.name);
            urns.placed.remove(&def.name);
            toast.write(Toast(format!("Dev: reset the {}", def.title)));
        }
    }
}

fn labelled(c: &mut ChildSpawnerCommands, theme: &Theme, b: DevButton, label: &str, width: f32) {
    let skin = theme.skin(b.look.skins()[0]);
    c.spawn((
        b,
        Node {
            width: Val::Px(width),
            height: Val::Px(36.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        skin,
        children![text(label, sizes::SMALL, colors::TEXT)],
    ));
}

#[allow(clippy::too_many_arguments)]
pub(super) fn dev_build(
    mut commands: Commands,
    theme: Res<Theme>,
    mut dev: ResMut<DevMenu>,
    (book, urns): (Option<Res<UrnBook>>, Res<Urns>),
    (items, art, npcs): (Res<Items>, Res<ItemArt>, Res<NpcBook>),
    (seasons, clock): (Res<Seasons>, Res<WorldClock>),
    mut root: Query<&mut Visibility, With<DevRoot>>,
    panel: Query<(Entity, Option<&Children>), With<DevPanel>>,
) {
    if !dev.dirty {
        return;
    }
    let Some(book) = book else { return };
    dev.dirty = false;
    let (Ok(mut vis), Ok((panel, kids))) = (root.single_mut(), panel.single()) else {
        return;
    };
    for k in kids.into_iter().flatten() {
        commands.entity(*k).despawn();
    }
    if !dev.open {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Inherited;
    let theme = &*theme;
    let dev = &*dev;
    commands.entity(panel).insert((
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(20.0), Val::Px(14.0)),
            row_gap: Val::Px(8.0),
            ..default()
        },
        theme.skin("panel"),
    ));
    commands.entity(panel).with_children(|c| {
        c.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(16.0),
            ..default()
        })
        .with_children(|r| {
            r.spawn((
                theme.bold(sizes::TITLE),
                Text::new("Dev"),
                TextColor(colors::HEADING),
            ));
            r.spawn(Node {
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|r| {
                for (t, label) in TABS {
                    labelled(
                        r,
                        theme,
                        button(Press::Tab(t), Look::Tab, dev.tab == t),
                        label,
                        110.0,
                    );
                }
            });
            r.spawn(text("F9 or Esc closes", sizes::SMALL, colors::MUTED));
        });
        match dev.tab {
            Tab::Urns => urns_tab(c, theme, &book, &urns),
            Tab::Items => items_tab(c, theme, dev, &items, &art),
            Tab::Npcs => npcs_tab(c, theme, &npcs),
            Tab::World => world_tab(c, theme, &seasons, &clock),
        }
    });
}

fn urns_tab(c: &mut ChildSpawnerCommands, theme: &Theme, book: &UrnBook, urns: &Urns) {
    c.spawn(text(
        "Hand yourself urns and skip steps to test the stations.",
        sizes::SMALL,
        colors::MUTED,
    ));
    for (i, def) in book.0.urns.iter().enumerate() {
        let p = urns.of(&def.name);
        let n = def.steps.iter().filter(|s| p.is_done(&s.name)).count();
        let where_ = if def.item.is_none() {
            "by the bedroll"
        } else if urns.placed.contains_key(&def.name) {
            "set down"
        } else {
            "not set down"
        };
        c.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|r| {
            r.spawn(Node {
                flex_direction: FlexDirection::Column,
                width: Val::Px(250.0),
                ..default()
            })
            .with_children(|t| {
                t.spawn((
                    theme.bold(sizes::BODY),
                    Text::new(format!("{} · {}", def.title, def.station)),
                    TextColor(colors::TEXT),
                ));
                let next = p
                    .next(def)
                    .map_or("all done".to_string(), |s| format!("next: {}", s.title));
                t.spawn(text(
                    format!("{n}/{} steps, {where_}, {next}", def.steps.len()),
                    sizes::TINY,
                    colors::MUTED,
                ));
            });
            for (act, label) in ACTS {
                if def.item.is_none() && matches!(act, Act::Give | Act::SetDown) {
                    r.spawn(Node {
                        width: Val::Px(112.0),
                        height: Val::Px(36.0),
                        ..default()
                    });
                    continue;
                }
                labelled(
                    r,
                    theme,
                    button(Press::Urn(i, act), Look::Button, false),
                    label,
                    112.0,
                );
            }
        });
    }
}

fn items_tab(
    c: &mut ChildSpawnerCommands,
    theme: &Theme,
    dev: &DevMenu,
    items: &Items,
    art: &ItemArt,
) {
    c.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|r| {
        for (cat, label) in CATS {
            labelled(
                r,
                theme,
                button(Press::Cat(cat), Look::Button, dev.cat == cat),
                label,
                100.0,
            );
        }
    });
    c.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|r| {
        r.spawn(text("Each click gives", sizes::SMALL, colors::MUTED));
        for (a, label) in AMOUNTS {
            labelled(
                r,
                theme,
                button(Press::Amount(a), Look::Button, dev.amount == a),
                label,
                72.0,
            );
        }
        r.spawn(Node {
            width: Val::Px(24.0),
            ..default()
        });
        labelled(
            r,
            theme,
            button(Press::Empty, Look::Button, false),
            "Empty pockets",
            170.0,
        );
    });
    let shown = shown_items(items, dev.cat);
    let page = dev.page.min(pages(shown.len()) - 1);
    c.spawn(Node {
        display: Display::Grid,
        grid_template_columns: RepeatedGridTrack::px(COLS as u16, TILE),
        grid_auto_rows: vec![GridTrack::px(TILE)],
        column_gap: Val::Px(4.0),
        row_gap: Val::Px(4.0),
        min_height: Val::Px(ROWS as f32 * (TILE + 4.0)),
        ..default()
    })
    .with_children(|g| {
        for &id in shown.iter().skip(page * PER_PAGE).take(PER_PAGE) {
            g.spawn((
                button(Press::Item(id), Look::Slot, false),
                Node {
                    width: Val::Px(TILE),
                    height: Val::Px(TILE),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                theme.skin("slot"),
            ))
            .with_children(|s| {
                let Some(img) = art.image(id) else {
                    s.spawn(text(
                        items.get(id).title.chars().take(3).collect::<String>(),
                        sizes::TINY,
                        colors::TEXT,
                    ));
                    return;
                };
                // Fit the sprite in the tile, at most 2x.
                let px = art.size(id).unwrap_or(UVec2::splat(20)).as_vec2();
                let k = (40.0 / px.x).min(40.0 / px.y).min(2.0);
                s.spawn((
                    img,
                    Node {
                        width: Val::Px(px.x * k),
                        height: Val::Px(px.y * k),
                        ..default()
                    },
                ));
            });
        }
    });
    c.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(8.0),
        ..default()
    })
    .with_children(|r| {
        labelled(
            r,
            theme,
            button(Press::Page(-1), Look::Button, false),
            "<",
            48.0,
        );
        labelled(
            r,
            theme,
            button(Press::Page(1), Look::Button, false),
            ">",
            48.0,
        );
        r.spawn((
            DevHint,
            text(
                format!(
                    "Page {} of {}. Wheel or the arrows turn pages.",
                    page + 1,
                    pages(shown.len())
                ),
                sizes::SMALL,
                colors::TEXT,
            ),
        ));
    });
}

fn npcs_tab(c: &mut ChildSpawnerCommands, theme: &Theme, npcs: &NpcBook) {
    c.spawn(text(
        "Click a person to have them stand in front of you and follow you.",
        sizes::SMALL,
        colors::MUTED,
    ));
    c.spawn(Node {
        display: Display::Grid,
        grid_template_columns: RepeatedGridTrack::px(3, 345.0),
        column_gap: Val::Px(6.0),
        row_gap: Val::Px(6.0),
        ..default()
    })
    .with_children(|g| {
        for (k, kind) in npcs.kinds.iter().enumerate() {
            let label = if kind.title.is_empty() {
                kind.name.clone()
            } else {
                format!("{} · {}", kind.name, kind.title)
            };
            labelled(
                g,
                theme,
                button(Press::Npc(k), Look::Button, false),
                &label,
                345.0,
            );
        }
    });
}

fn world_tab(c: &mut ChildSpawnerCommands, theme: &Theme, seasons: &Seasons, clock: &WorldClock) {
    let now = seasons.at(clock).current;
    c.spawn(text(
        "Season: jumps ahead to the first day of the one you pick (time of day stays).",
        sizes::SMALL,
        colors::MUTED,
    ));
    c.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|r| {
        for (k, s) in seasons.list.iter().enumerate() {
            labelled(
                r,
                theme,
                button(Press::Season(k), Look::Button, k == now),
                &s.title,
                130.0,
            );
        }
    });
}
