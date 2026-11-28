//! The recipe book that floats at the left of a bench: each recipe as
//! pictures (what goes in, the fire's heat, how many goes of the tool, what
//! comes out), the one your board matches lit up, and the bench's notes
//! (fuel, sharpening, what the lens showed) at the foot of the page. It is a `crate::book`:
//! turn the page with the corner arrows or Left/Right.

use bevy::prelude::*;

use super::bench::Recipe;
use super::board::{board_name, layout, plural, AtBench, Benches};
use super::StationKind;
use crate::book::{ink, Book, BookLook, INK, INK_FAINT, LIT};
use crate::item_art::ItemArt;
use crate::items::Items;
use crate::theme::{sizes, Theme};

/// Recipes on one page.
const PER_PAGE: usize = 5;
const ICON: f32 = 40.0;

/// What floats at the left of the bench and holds the book.
#[derive(Component)]
pub(super) struct BenchBook;

pub(super) fn spawn_book(mut commands: Commands) {
    commands
        .spawn((
            BenchBook,
            Node { position_type: PositionType::Absolute, left: Val::Px(22.0), top: Val::Px(110.0), ..default() },
            Visibility::Hidden,
            Pickable::IGNORE,
            // Above the bench's other UI.
            GlobalZIndex(5),
        ))
        .with_children(|h| {
            crate::book::build(h, BookLook { sides: 1, page: Vec2::new(316.0, 470.0), wheel: false }, ());
        });
}

/// One recipe as a row of pictures.
fn recipe_row(p: &mut ChildSpawnerCommands, theme: &Theme, items: &Items, art: &ItemArt, r: &Recipe, go: &str, lit: bool) {
    p.spawn((
        Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(4.0)), row_gap: Val::Px(2.0), ..default() },
        BackgroundColor(if lit { LIT } else { Color::NONE }),
    ))
    .with_children(|row| {
        row.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(3.0), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|line| {
            for s in r.slots.values().chain(&r.mix) {
                line.spawn((Node { width: Val::Px(ICON), height: Val::Px(ICON), ..default() }, theme.skin("slot"))).with_children(|c| {
                    crate::inventory::ui::icon(c, items, art, *s, ICON, ICON, false);
                });
            }
            if r.heat > 0 {
                line.spawn(Node { align_items: AlignItems::Center, ..default() }).with_children(|h| {
                    h.spawn(theme.icon("icon_warmth"));
                    h.spawn(ink(format!("{}", r.heat), sizes::BODY, INK));
                });
            }
            line.spawn(ink(">", sizes::HEADING, INK));
            line.spawn((Node { width: Val::Px(ICON), height: Val::Px(ICON), ..default() }, theme.skin("slot_selected"))).with_children(|c| {
                crate::inventory::ui::icon(c, items, art, r.makes, ICON, ICON, false);
            });
        });
        let gos = if r.work == 1 { format!("1 {go}") } else { format!("{} {}", r.work, plural(go)) };
        row.spawn(ink(format!("{}  ·  {gos}", items.get(r.makes.item).title), sizes::SMALL, INK_FAINT));
    });
}

/// Show, float and fill the book while you're at a bench.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn board_book(
    mut commands: Commands,
    time: Res<Time>,
    theme: Res<Theme>,
    mut at: ResMut<AtBench>,
    (items, benches, art): (Res<Items>, Res<Benches>, Option<Res<ItemArt>>),
    mut holder: Query<(&mut Visibility, &mut Node, &mut UiTransform), With<BenchBook>>,
    mut book: Query<&mut Book>,
    holder_kids: Query<&Children, With<BenchBook>>,
    mut built: Local<String>,
) {
    let Ok((mut vis, mut node, mut turn)) = holder.single_mut() else { return };
    let Some(book_e) = holder_kids.iter().flatten().find(|e| book.contains(**e)).copied() else { return };
    let Ok(mut book) = book.get_mut(book_e) else { return };
    let Some(st) = at.station.filter(|_| at.blend > 0.3) else {
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
        built.clear();
        return;
    };
    let Some(art) = art else { return };
    let name = board_name(st.kind).unwrap_or("");
    let recipes: Vec<&Recipe> = benches.0.for_station(name).collect();
    book.set_pages(recipes.len().div_ceil(PER_PAGE).max(1));
    if built.is_empty() {
        book.open_at(at.book_page);
    }
    at.book_page = book.spread();
    // It floats: a slow bob and sway.
    let t = time.elapsed_secs();
    node.top = Val::Px(110.0 + (t * 1.3).sin() * 6.0);
    turn.rotation = Rot2::radians((t * 0.9).sin() * 0.015);
    if *vis != Visibility::Inherited {
        *vis = Visibility::Inherited;
    }
    let lit = benches.0.matching(name, &at.board).map(|r| r as *const Recipe);
    // The notes at the foot of the page.
    let mut notes: Vec<String> = Vec::new();
    match st.kind {
        StationKind::SharpeningStone => {
            if let Some(sh) = benches.0.sharpen {
                notes.push(format!(
                    "Lay any tool on the rest, head to the wheel, then {} {}: it works {:.1}x as fast for {:.0} minutes",
                    sh.work,
                    plural(layout(st.kind).work.1),
                    sh.speed,
                    sh.seconds / 60.0
                ));
            }
        }
        StationKind::MagicalLens => notes.push(at.study.clone().unwrap_or_else(|| {
            "Lay anything on the plate, then turn the dial: it shows what the thing is made of, and teaches the cauldron the draughts it goes into".into()
        })),
        StationKind::Cauldron if recipes.is_empty() => notes.push("No draughts yet: study their parts under the magical lens".into()),
        _ => {}
    }
    if matches!(st.kind, StationKind::CookingPot | StationKind::SmokingRack | StationKind::Cauldron) && !benches.0.fuel.is_empty() {
        let mut fuel: Vec<(u32, &str)> = benches.0.fuel.iter().map(|(i, h)| (*h, items.get(*i).title.as_str())).collect();
        fuel.sort();
        let fuel: Vec<String> = fuel.iter().map(|(h, t)| format!("{t} {h}")).collect();
        notes.push(format!("Heat from each fuel: {}", fuel.join(" · ")));
    }
    let key = format!("{:?}{:?}{:?}{}", st.kind, recipes.iter().map(|r| *r as *const Recipe).collect::<Vec<_>>(), lit, notes.join("|"));
    if *built != key {
        *built = key;
        book.refill();
    }
    if !book.take_refill() {
        return;
    }
    let go = layout(st.kind).work.1;
    for (page_e, page_i) in book.shown() {
        commands.entity(page_e).despawn_related::<Children>();
        commands.entity(page_e).with_children(|p| {
            p.spawn(ink(st.kind.title(), sizes::HEADING, INK));
            for r in recipes.iter().skip(page_i * PER_PAGE).take(PER_PAGE) {
                recipe_row(p, &theme, &items, &art, r, go, lit == Some(*r as *const Recipe));
            }
            if recipes.is_empty() && notes.is_empty() {
                p.spawn(ink("Nothing to make here yet", sizes::SMALL, INK_FAINT));
            }
            for n in &notes {
                p.spawn((ink(n.clone(), sizes::SMALL, INK_FAINT), Node { max_width: Val::Px(290.0), ..default() }));
            }
        });
    }
}
