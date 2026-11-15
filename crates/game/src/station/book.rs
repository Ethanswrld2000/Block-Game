//! The recipe book that floats at the left of a bench: each recipe as
//! pictures (what goes in, the fire's heat, how many goes of the tool, what
//! comes out), the one your board matches lit up, and the bench's notes
//! (fuel, sharpening, what the lens showed) at the foot of the page. Turn the
//! page with the arrows at its foot.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::bench::Recipe;
use super::board::{board_name, layout, plural, AtBench, Benches};
use super::StationKind;
use crate::item_art::ItemArt;
use crate::items::Items;
use crate::theme::{colors, sizes, NoShadow, Theme};

/// Recipes on one page.
const PER_PAGE: usize = 5;
const ICON: f32 = 40.0;
const INK: Color = Color::srgb(0.24, 0.16, 0.09);
const INK_FAINT: Color = Color::srgb(0.42, 0.33, 0.22);
const PAGE: Color = Color::srgb(0.87, 0.8, 0.63);
const LIT: Color = Color::srgb(0.98, 0.9, 0.62);
const COVER: Color = Color::srgb(0.3, 0.17, 0.1);

#[derive(Component)]
pub(super) struct Book;

#[derive(Component)]
pub(super) struct BookPage;

/// The page-turning arrows (-1 back, +1 on).
#[derive(Component)]
pub(super) struct BookFlip(i32);

pub(super) fn spawn_book(mut commands: Commands) {
    commands
        .spawn((
            Book,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(22.0),
                top: Val::Px(110.0),
                width: Val::Px(330.0),
                padding: UiRect::all(Val::Px(7.0)),
                border: UiRect::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(COVER),
            BorderColor::all(colors::GOLD),
            Visibility::Hidden,
            Pickable::IGNORE,
            // Above the bench's other UI.
            GlobalZIndex(5),
        ))
        .with_children(|b| {
            b.spawn((
                BookPage,
                Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), padding: UiRect::all(Val::Px(10.0)), row_gap: Val::Px(6.0), ..default() },
                BackgroundColor(PAGE),
            ));
        });
}

fn ink(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont::from_font_size(size), TextColor(color), NoShadow)
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
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut at: ResMut<AtBench>,
    (items, benches, art): (Res<Items>, Res<Benches>, Option<Res<ItemArt>>),
    mut book: Query<(&mut Visibility, &mut Node, &mut BorderColor, &mut UiTransform), With<Book>>,
    page: Query<(Entity, Option<&Children>), With<BookPage>>,
    flips: Query<(&BookFlip, &ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
    mut built: Local<String>,
) {
    let Ok((mut vis, mut node, mut border, mut turn)) = book.single_mut() else { return };
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
    let pages = recipes.len().div_ceil(PER_PAGE).max(1);
    // Turning the page.
    if mouse.just_pressed(MouseButton::Left)
        && let Some(phys) = window.physical_cursor_position()
        && let Some((f, ..)) = flips.iter().find(|(_, n, t, v)| v.get() && n.contains_point(**t, phys))
    {
        at.book_page = (at.book_page as i32 + f.0).rem_euclid(pages as i32) as usize;
    }
    let page_i = at.book_page.min(pages - 1);
    // It floats: a slow bob and sway, and the gilt edge glows.
    let t = time.elapsed_secs();
    node.top = Val::Px(110.0 + (t * 1.3).sin() * 6.0);
    turn.rotation = Rot2::radians((t * 0.9).sin() * 0.015);
    let glow = 0.5 + 0.5 * (t * 2.0).sin();
    *border = BorderColor::all(colors::GOLD.mix(&Color::srgb(1.0, 0.97, 0.8), glow * 0.6));
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
    let key = format!("{:?}{page_i}{:?}{:?}{}", st.kind, recipes.iter().map(|r| *r as *const Recipe).collect::<Vec<_>>(), lit, notes.join("|"));
    if *built == key {
        return;
    }
    *built = key;
    let Ok((page_e, kids)) = page.single() else { return };
    for k in kids.into_iter().flatten() {
        commands.entity(*k).despawn();
    }
    let go = layout(st.kind).work.1;
    commands.entity(page_e).with_children(|p| {
        p.spawn(ink(st.kind.title(), sizes::HEADING, INK));
        for r in recipes.iter().skip(page_i * PER_PAGE).take(PER_PAGE) {
            recipe_row(p, &theme, &items, &art, r, go, lit == Some(*r as *const Recipe));
        }
        if recipes.is_empty() && notes.is_empty() {
            p.spawn(ink("Nothing to make here yet", sizes::SMALL, INK_FAINT));
        }
        for n in &notes {
            p.spawn((ink(n.clone(), sizes::SMALL, INK_FAINT), Node { max_width: Val::Px(300.0), ..default() }));
        }
        if pages > 1 {
            p.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }).with_children(|f| {
                for (dir, label) in [(-1, "<"), (1, ">")] {
                    if dir == 1 {
                        f.spawn(ink(format!("page {} of {pages}", page_i + 1), sizes::SMALL, INK_FAINT));
                    }
                    f.spawn((BookFlip(dir), Node { padding: UiRect::axes(Val::Px(12.0), Val::Px(0.0)), ..default() }, theme.skin("button"))).with_children(|b| {
                        b.spawn(crate::theme::text(label, sizes::HEADING, colors::TEXT));
                    });
                }
            });
        }
    });
}
