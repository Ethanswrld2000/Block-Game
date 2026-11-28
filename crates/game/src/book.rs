//! One book for every menu that reads like a book (bench recipe books, the
//! journal): a leather cover with gilt corners, paper pages, page numbers,
//! a ribbon, and pages that turn. Click the corner arrows, press Left/Right
//! (or roll the wheel where the book allows it) and the leaf folds over the
//! spine while the next pages show beneath.
//!
//! The book only draws the binding and turns pages. Whoever owns it says how
//! many pages there are (`Book::set_pages`) and fills the page entities
//! (`Book::shown`) when `Book::take_refill` says the pages changed.

use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::sound::PlaySound;
use crate::theme::{NoShadow, sizes};

/// Seconds for one page turn.
const TURN: f32 = 0.34;

/// Ink and paper.
pub const INK: Color = Color::srgb(0.24, 0.16, 0.09);
pub const INK_FAINT: Color = Color::srgb(0.42, 0.33, 0.22);
pub const LIT: Color = Color::srgb(0.98, 0.9, 0.62);
const PAPER: Color = Color::srgb(0.87, 0.8, 0.63);
const PAPER_EDGE: Color = Color::srgb(0.74, 0.66, 0.5);
const PAPER_SHADE: Color = Color::srgb(0.62, 0.53, 0.38);
const LEATHER: Color = Color::srgb(0.3, 0.17, 0.1);
const LEATHER_DARK: Color = Color::srgb(0.17, 0.09, 0.05);
const GILT: Color = Color::srgb(0.86, 0.66, 0.26);
const RIBBON: Color = Color::srgb(0.62, 0.12, 0.12);
const HOVER: Color = Color::srgb(0.66, 0.2, 0.12);

pub struct BookPlugin;

impl Plugin for BookPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::menu::GameUpdate, (flip_input, turn_leaves).chain());
    }
}

/// How a book is made.
#[derive(Clone, Copy)]
pub struct BookLook {
    /// 1 = a single page bound on the left, 2 = an open spread.
    pub sides: usize,
    /// Size of one page's paper, in logical pixels.
    pub page: Vec2,
    /// The mouse wheel turns pages.
    pub wheel: bool,
}

/// A book's state. The pages shown are `spread * sides ..`.
#[derive(Component)]
pub struct Book {
    look: BookLook,
    pages: usize,
    spread: usize,
    sides: [Entity; 2],
    leaves: [Entity; 2],
    folios: [Entity; 2],
    turn: Option<(i32, f32)>,
    /// The leaf stays where it is (screenshots).
    hold: bool,
    refill: bool,
}

impl Book {
    /// How many spreads (one page, or a pair) the book has.
    pub fn spreads(&self) -> usize {
        self.pages.div_ceil(self.look.sides).max(1)
    }

    pub fn spread(&self) -> usize {
        self.spread
    }

    /// Set the page count; keeps the place if it's still there.
    pub fn set_pages(&mut self, pages: usize) {
        if pages != self.pages {
            self.pages = pages;
            self.refill = true;
        }
        let last = self.spreads() - 1;
        if self.spread > last {
            self.spread = last;
            self.refill = true;
        }
    }

    /// Open at a spread (no turning).
    pub fn open_at(&mut self, spread: usize) {
        let s = spread.min(self.spreads() - 1);
        if s != self.spread {
            self.spread = s;
            self.refill = true;
        }
    }

    /// Ask for the pages to be filled again (the content changed).
    pub fn refill(&mut self) {
        self.refill = true;
    }

    /// True once after the shown pages changed: clear and fill `shown()`.
    pub fn take_refill(&mut self) -> bool {
        std::mem::take(&mut self.refill)
    }

    /// The page entities on show and the page number each holds.
    pub fn shown(&self) -> impl Iterator<Item = (Entity, usize)> + '_ {
        (0..self.look.sides).map(|s| (self.sides[s], self.spread * self.look.sides + s))
    }

    /// Turn by `dir` spreads (-1 back, +1 on); false at either end.
    pub fn turn(&mut self, dir: i32) -> bool {
        let to = self.spread as i32 + dir;
        if to < 0 || to >= self.spreads() as i32 {
            return false;
        }
        self.spread = to as usize;
        self.turn = Some((dir.signum(), 0.0));
        self.refill = true;
        true
    }
}

/// What a page holds, split into pages by a rough size per entry: the ranges
/// of entries on each page. An entry bigger than a page gets a page of its own.
pub fn paginate(costs: &[usize], budget: usize) -> Vec<std::ops::Range<usize>> {
    let mut pages = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (i, &c) in costs.iter().enumerate() {
        if i > start && used + c > budget {
            pages.push(start..i);
            (start, used) = (i, 0);
        }
        used += c;
    }
    if start < costs.len() {
        pages.push(start..costs.len());
    }
    pages
}

/// Ink text for a page (no drop shadow).
pub fn ink(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont::from_font_size(size),
        TextColor(color),
        NoShadow,
    )
}

/// The corner arrows (-1 back, +1 on).
#[derive(Component)]
struct Flip(Entity, i32);

/// The folding leaf over side 0 or 1.
#[derive(Component)]
struct Leaf;

/// Build a book under `parent`; `extra` goes on the book's root.
pub fn build(parent: &mut ChildSpawnerCommands, look: BookLook, extra: impl Bundle) -> Entity {
    let root = parent
        .spawn((
            Node {
                border: UiRect::all(Val::Px(3.0)),
                padding: UiRect::all(Val::Px(9.0)),
                ..default()
            },
            BackgroundColor(LEATHER),
            BorderColor::all(LEATHER_DARK),
            extra,
        ))
        .id();
    let mut sides = [Entity::PLACEHOLDER; 2];
    let mut leaves = [Entity::PLACEHOLDER; 2];
    let mut folios = [Entity::PLACEHOLDER; 2];
    parent.commands().entity(root).with_children(|cover| {
        // Gilt corner pieces on the leather.
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let at = |far| if far { Val::Px(-1.0) } else { Val::Auto };
            let near = |n| if n { Val::Auto } else { Val::Px(-1.0) };
            cover
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: near(x == 1),
                        right: at(x == 1),
                        top: near(y == 1),
                        bottom: at(y == 1),
                        width: Val::Px(14.0),
                        height: Val::Px(14.0),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(GILT),
                    BorderColor::all(LEATHER_DARK),
                    ZIndex(3),
                ))
                .with_child((
                    Node {
                        width: Val::Px(4.0),
                        height: Val::Px(4.0),
                        margin: UiRect::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(LEATHER_DARK),
                ));
        }
        // Gilt rule inside the cover, then the paper block.
        cover
            .spawn((
                Node {
                    border: UiRect::all(Val::Px(2.0)),
                    padding: UiRect::all(Val::Px(4.0)),
                    ..default()
                },
                BorderColor::all(GILT),
            ))
            .with_children(|trim| {
                for s in 0..look.sides {
                    if s == 1 {
                        // The spine: a dark crease between the pages.
                        trim.spawn(Node {
                            width: Val::Px(8.0),
                            ..default()
                        })
                        .with_children(|sp| {
                            for c in [PAPER_SHADE, LEATHER_DARK, LEATHER_DARK, PAPER_SHADE] {
                                sp.spawn((
                                    Node {
                                        width: Val::Px(2.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(c),
                                ));
                            }
                        });
                    }
                    // Page edges below and at the outer side: the stack of paper under this page.
                    trim.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            border: UiRect {
                                bottom: Val::Px(4.0),
                                left: Val::Px(if s == 0 { 3.0 } else { 0.0 }),
                                right: Val::Px(if s + 1 == look.sides { 3.0 } else { 0.0 }),
                                ..default()
                            },
                            ..default()
                        },
                        BorderColor::all(PAPER_EDGE),
                    ))
                    .with_children(|stack| {
                        stack
                            .spawn((
                                Node {
                                    width: Val::Px(look.page.x),
                                    min_height: Val::Px(look.page.y),
                                    flex_direction: FlexDirection::Column,
                                    flex_grow: 1.0,
                                    padding: UiRect::new(
                                        Val::Px(14.0),
                                        Val::Px(14.0),
                                        Val::Px(12.0),
                                        Val::Px(8.0),
                                    ),
                                    ..default()
                                },
                                BackgroundColor(PAPER),
                            ))
                            .with_children(|paper| {
                                // What the owner fills in.
                                sides[s] = paper
                                    .spawn(Node {
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(6.0),
                                        flex_grow: 1.0,
                                        ..default()
                                    })
                                    .id();
                                // The foot: arrow, page number, arrow.
                                paper
                                    .spawn(Node {
                                        justify_content: JustifyContent::SpaceBetween,
                                        align_items: AlignItems::Center,
                                        margin: UiRect::top(Val::Px(6.0)),
                                        ..default()
                                    })
                                    .with_children(|foot| {
                                        let back = s == 0;
                                        let on = s + 1 == look.sides;
                                        foot.spawn((
                                            Node {
                                                width: Val::Px(28.0),
                                                ..default()
                                            },
                                            ink(if back { "<" } else { "" }, sizes::HEADING, INK),
                                            Flip(root, if back { -1 } else { 0 }),
                                        ));
                                        folios[s] =
                                            foot.spawn(ink("", sizes::SMALL, INK_FAINT)).id();
                                        foot.spawn((
                                            Node {
                                                width: Val::Px(28.0),
                                                justify_content: JustifyContent::End,
                                                ..default()
                                            },
                                            ink(if on { ">" } else { "" }, sizes::HEADING, INK),
                                            TextLayout::justify(Justify::Right),
                                            Flip(root, if on { 1 } else { 0 }),
                                        ));
                                    });
                                // The turning leaf, drawn over the page while it turns.
                                leaves[s] = paper
                                    .spawn((
                                        Leaf,
                                        Node {
                                            position_type: PositionType::Absolute,
                                            top: Val::Px(0.0),
                                            bottom: Val::Px(0.0),
                                            width: Val::Px(0.0),
                                            ..default()
                                        },
                                        BackgroundColor(PAPER),
                                        BorderColor::all(PAPER_SHADE),
                                        Visibility::Hidden,
                                        ZIndex(2),
                                        Pickable::IGNORE,
                                    ))
                                    .id();
                            });
                    });
                }
            });
        // A ribbon hanging from the last page.
        cover.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(48.0),
                bottom: Val::Px(-24.0),
                width: Val::Px(12.0),
                height: Val::Px(44.0),
                border: UiRect::new(Val::Px(2.0), Val::Px(2.0), Val::Px(0.0), Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(RIBBON),
            BorderColor::all(LEATHER_DARK),
        ));
    });
    parent.commands().entity(root).insert(Book {
        look,
        pages: 1,
        spread: 0,
        sides,
        leaves,
        folios,
        turn: None,
        hold: false,
        refill: true,
    });
    root
}

/// Clicks on the corner arrows, the keys and the wheel turn the page.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn flip_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut books: Query<(Entity, &mut Book, &InheritedVisibility)>,
    mut flips: Query<(&Flip, &ComputedNode, &UiGlobalTransform, &mut TextColor)>,
    mut sound: MessageWriter<PlaySound>,
    mut held: Local<bool>,
) {
    let cursor = window.physical_cursor_position();
    let clicked = mouse.just_pressed(MouseButton::Left);
    let mut click: Option<(Entity, i32)> = None;
    for (f, n, t, mut color) in &mut flips {
        // An arrow shows only where there's a page to turn to.
        let can = f.1 != 0
            && books
                .get(f.0)
                .is_ok_and(|(_, b, _)| (0..b.spreads() as i32).contains(&(b.spread as i32 + f.1)));
        let over = can && cursor.is_some_and(|c| n.contains_point(*t, c));
        let want = if !can {
            Color::NONE
        } else if over {
            HOVER
        } else {
            INK
        };
        if color.0 != want {
            color.0 = want;
        }
        if over && clicked {
            click = Some((f.0, f.1));
        }
    }
    let combo = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    // Screenshots: `BLOCKGAME_BOOKTURN=0.4` turns the first open book once and holds the leaf there.
    let hold = std::env::var("BLOCKGAME_BOOKTURN")
        .ok()
        .and_then(|v| v.parse::<f32>().ok());
    for (e, mut book, vis) in &mut books {
        if !vis.get() {
            continue;
        }
        if let Some(h) = hold
            && !*held
        {
            book.open_at(0);
            if book.turn(1) {
                *held = true;
                book.turn = Some((1, h.clamp(0.0, 0.99)));
                book.hold = true;
            }
        }
        let mut dir = click.filter(|c| c.0 == e).map_or(0, |c| c.1);
        if !combo {
            dir += keys.just_pressed(KeyCode::ArrowRight) as i32
                - keys.just_pressed(KeyCode::ArrowLeft) as i32;
        }
        if book.look.wheel && scroll.delta.y != 0.0 {
            dir -= scroll.delta.y.signum() as i32;
        }
        if dir != 0 && book.turn(dir.signum()) {
            sound.write(PlaySound::new("ui_tab").gain(0.7).speed(0.75));
        }
    }
}

/// Fold the leaf over the spine, shade it as it stands up, and number the pages.
fn turn_leaves(
    time: Res<Time<Real>>,
    mut books: Query<&mut Book>,
    mut leaves: Query<
        (
            &mut Node,
            &mut Visibility,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Leaf>,
    >,
    mut folios: Query<&mut Text>,
) {
    for mut book in &mut books {
        // Page numbers.
        let first = book.spread * book.look.sides;
        for s in 0..book.look.sides {
            let n = first + s;
            let want = if n < book.pages {
                format!("{}", n + 1)
            } else {
                String::new()
            };
            if let Ok(mut t) = folios.get_mut(book.folios[s])
                && t.0 != want
            {
                t.0 = want;
            }
        }
        let Some((dir, t)) = book.turn else { continue };
        let t = if book.hold {
            t
        } else {
            t + time.delta_secs() / TURN
        };
        book.turn = (t < 1.0).then_some((dir, t));
        let p = t.min(1.0);
        // Each side's leaf: (how much of the page it covers 0..1, anchored at the left).
        let mut cover = [(0.0f32, true); 2];
        if book.look.sides == 1 {
            // The old page lifts off towards the binding (on) or the edge (back).
            cover[0] = (1.0 - p, dir > 0);
        } else if dir > 0 {
            // Right page lifts to the spine, then lands on the left.
            cover = if p < 0.5 {
                [(0.0, false), (1.0 - 2.0 * p, true)]
            } else {
                [(2.0 * p - 1.0, false), (0.0, true)]
            };
        } else {
            cover = if p < 0.5 {
                [(1.0 - 2.0 * p, false), (0.0, true)]
            } else {
                [(0.0, false), (2.0 * p - 1.0, true)]
            };
        }
        for s in 0..book.look.sides {
            let Ok((mut node, mut vis, mut bg, mut edge)) = leaves.get_mut(book.leaves[s]) else {
                continue;
            };
            let (w, left) = cover[s];
            let on = w > 0.001 && t < 1.0;
            *vis = if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            node.width = Val::Percent(w * 100.0);
            (node.left, node.right) = if left {
                (Val::Px(0.0), Val::Auto)
            } else {
                (Val::Auto, Val::Px(0.0))
            };
            // The free edge of the leaf has a dark line.
            node.border = if left {
                UiRect::right(Val::Px(2.0))
            } else {
                UiRect::left(Val::Px(2.0))
            };
            // Narrow = standing up off the page = in shade; the leaf that lands fades into the page beneath.
            let shade = 1.0 - w;
            let landing =
                (book.look.sides == 2 && ((dir > 0 && s == 0) || (dir < 0 && s == 1))) as u8 as f32;
            let fade = if landing > 0.0 && p > 0.75 {
                1.0 - (p - 0.75) / 0.25
            } else {
                1.0
            };
            bg.0 = PAPER.mix(&PAPER_SHADE, shade * 0.8).with_alpha(fade);
            *edge = BorderColor::all(PAPER_SHADE.with_alpha(fade));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_split_into_pages_by_size() {
        assert_eq!(paginate(&[], 10), Vec::<std::ops::Range<usize>>::new());
        assert_eq!(paginate(&[4, 4, 4, 4], 10), vec![0..2, 2..4]);
        // Too big for a page: one of its own.
        assert_eq!(paginate(&[3, 30, 3], 10), vec![0..1, 1..2, 2..3]);
    }
}
