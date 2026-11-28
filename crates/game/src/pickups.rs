//! A small note on the right of the screen when you pick something up: the
//! item's picture, its name and how many. Picking up more of the same within
//! `MERGE` seconds adds to that note's count (and it pops again); otherwise
//! the note fades off and a new one starts. Rules in `Feed` (pure).

use bevy::prelude::*;

use crate::inventory::PickedUp;
use crate::item_art::ItemArt;
use crate::items::{ItemId, Items};
use crate::theme::{self, Theme, colors, sizes};

pub struct PickupsPlugin;

impl Plugin for PickupsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Feed>()
            .add_systems(crate::menu::GamePostStartup, spawn)
            .add_systems(crate::menu::GameUpdate, (take, show).chain());
    }
}

/// More of the same item within this many seconds of the last joins its note.
pub const MERGE: f32 = 2.0;
/// After the last pick-up a note stays this long ...
pub const HOLD: f32 = 2.0;
/// ... then fades off over this long.
pub const FADE: f32 = 0.4;
/// Notes shown at once; the oldest goes first.
pub const MOST: usize = 5;
/// Picture size in the note, points.
const ICON: f32 = 32.0;

/// One note.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    pub id: u64,
    pub item: ItemId,
    pub count: u32,
    /// When it first showed.
    pub born: f32,
    /// When it was last added to.
    pub bumped: f32,
}

impl Note {
    /// How much is left of it: 1 while it holds, then down to 0.
    pub fn alpha(&self, now: f32) -> f32 {
        1.0 - ((now - self.bumped - HOLD) / FADE).clamp(0.0, 1.0)
    }

    fn fading(&self, now: f32) -> bool {
        now - self.bumped > HOLD
    }
}

/// The notes showing, oldest first.
#[derive(Resource, Default, Debug)]
pub struct Feed {
    pub notes: Vec<Note>,
    next: u64,
}

impl Feed {
    /// You picked up `count` of `item` at `now`.
    pub fn add(&mut self, item: ItemId, count: u32, now: f32) {
        if count == 0 {
            return;
        }
        if let Some(n) = self
            .notes
            .iter_mut()
            .rev()
            .find(|n| n.item == item && now - n.bumped <= MERGE && !n.fading(now))
        {
            n.count += count;
            n.bumped = now;
            return;
        }
        self.next += 1;
        self.notes.push(Note {
            id: self.next,
            item,
            count,
            born: now,
            bumped: now,
        });
        while self.notes.len() > MOST {
            self.notes.remove(0);
        }
    }

    /// Drop the notes that have faded off.
    pub fn prune(&mut self, now: f32) {
        self.notes.retain(|n| n.alpha(now) > 0.0);
    }
}

/// The column the notes stack in, right side, above the meters.
#[derive(Component)]
struct FeedRoot;

/// A note on screen, by its id.
#[derive(Component)]
struct NoteUi {
    id: u64,
    count: u32,
}

#[derive(Component)]
struct NoteText;

fn spawn(mut commands: Commands) {
    commands.spawn((
        FeedRoot,
        Name::new("pickups"),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(14.0),
            bottom: Val::Px(220.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            row_gap: Val::Px(4.0),
            ..default()
        },
        Pickable::IGNORE,
    ));
}

fn take(time: Res<Time<Real>>, mut picked: MessageReader<PickedUp>, mut feed: ResMut<Feed>) {
    let now = time.elapsed_secs();
    for p in picked.read() {
        feed.add(p.item, p.count as u32, now);
    }
    if feed.notes.iter().any(|n| n.alpha(now) <= 0.0) {
        feed.prune(now);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    time: Res<Time<Real>>,
    theme: Res<Theme>,
    items: Res<Items>,
    art: Option<Res<ItemArt>>,
    feed: Res<Feed>,
    root: Single<Entity, With<FeedRoot>>,
    mut notes: Query<(
        Entity,
        &mut NoteUi,
        &mut UiTransform,
        &mut ImageNode,
        &Children,
    )>,
    mut texts: Query<(&mut Text, &mut TextColor), With<NoteText>>,
    mut pics: Query<&mut ImageNode, Without<NoteUi>>,
    kids: Query<&Children>,
) {
    let now = time.elapsed_secs();
    // Gone from the feed: gone from the screen.
    for (e, ui, ..) in &notes {
        if !feed.notes.iter().any(|n| n.id == ui.id) {
            commands.entity(e).despawn();
        }
    }
    for n in &feed.notes {
        let Some((_, mut ui, mut tf, mut frame, children)) =
            notes.iter_mut().find(|(_, ui, ..)| ui.id == n.id)
        else {
            spawn_note(&mut commands, *root, &theme, &items, art.as_deref(), n);
            continue;
        };
        // Fades in four steps.
        let a = (n.alpha(now) * 4.0).ceil() / 4.0;
        // In: slides from the right in 3 steps. Added to: a small hop left.
        // Out: slides back right as it fades, in the fade's steps.
        let slide_in = 12.0 * (1.0 - theme::stepped((now - n.born) / theme::SNAP, 3));
        let bump = if n.bumped > n.born {
            -6.0 * (1.0 - theme::stepped((now - n.bumped) / theme::SNAP, 3))
        } else {
            0.0
        };
        let out = (1.0 - a) * 16.0;
        let want = UiTransform {
            translation: Val2::px(slide_in + bump + out, 0.0),
            ..default()
        };
        if *tf != want {
            *tf = want;
        }
        if frame.color.alpha() != a {
            frame.color.set_alpha(a);
        }
        for c in children.iter() {
            if let Ok((mut t, mut col)) = texts.get_mut(c) {
                if ui.count != n.count {
                    t.0 = line(&items, n);
                }
                if col.0.alpha() != a {
                    col.0.set_alpha(a);
                }
            }
            for c in std::iter::once(c).chain(kids.get(c).into_iter().flat_map(|k| k.iter())) {
                if let Ok(mut img) = pics.get_mut(c)
                    && img.color.alpha() != a
                {
                    img.color.set_alpha(a);
                }
            }
        }
        ui.count = n.count;
    }
}

fn line(items: &Items, n: &Note) -> String {
    format!("+{}  {}", n.count, items.get(n.item).title)
}

fn spawn_note(
    commands: &mut Commands,
    root: Entity,
    theme: &Theme,
    items: &Items,
    art: Option<&ItemArt>,
    n: &Note,
) {
    commands
        .spawn((
            NoteUi {
                id: n.id,
                count: n.count,
            },
            ChildOf(root),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                padding: UiRect::new(Val::Px(8.0), Val::Px(12.0), Val::Px(4.0), Val::Px(4.0)),
                ..default()
            },
            UiTransform {
                translation: Val2::px(12.0, 0.0),
                ..default()
            },
            theme.skin("panel_light"),
        ))
        .with_children(|c| {
            c.spawn(Node {
                width: Val::Px(ICON),
                height: Val::Px(ICON),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|p| {
                if let Some((img, size)) =
                    art.and_then(|a| Some((a.image(n.item)?, a.size(n.item)?)))
                {
                    let s = size.as_vec2();
                    let k = 2.0f32.min(ICON / s.x).min(ICON / s.y);
                    p.spawn((
                        img,
                        Node {
                            width: Val::Px(s.x * k),
                            height: Val::Px(s.y * k),
                            ..default()
                        },
                    ));
                }
            });
            c.spawn((
                NoteText,
                theme::text(line(items, n), sizes::BODY, colors::TEXT),
            ));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_item_within_two_seconds_adds_up() {
        let (rock, stick) = (ItemId(1), ItemId(2));
        let mut f = Feed::default();
        f.add(rock, 1, 0.0);
        f.add(rock, 2, 1.5);
        assert_eq!(f.notes.len(), 1);
        assert_eq!(f.notes[0].count, 3);
        // Counting from the last one: 1.5 + 1.9 is still in.
        f.add(rock, 1, 3.4);
        assert_eq!((f.notes.len(), f.notes[0].count), (1, 4));
        // Another item gets its own note.
        f.add(stick, 1, 3.5);
        assert_eq!(f.notes.len(), 2);
    }

    #[test]
    fn later_pickups_start_a_new_note_and_old_ones_fade_off() {
        let rock = ItemId(1);
        let mut f = Feed::default();
        f.add(rock, 1, 0.0);
        assert_eq!(f.notes[0].alpha(1.0), 1.0);
        assert!(f.notes[0].alpha(HOLD + FADE * 0.5) < 1.0);
        // Too late to join: a fresh note while the old one fades.
        f.add(rock, 1, 2.5);
        assert_eq!(f.notes.len(), 2);
        assert_eq!(f.notes[1].count, 1);
        f.prune(HOLD + FADE + 0.01);
        assert_eq!(f.notes.len(), 1);
        f.prune(10.0);
        assert!(f.notes.is_empty());
    }

    #[test]
    fn keeps_only_the_newest_few() {
        let mut f = Feed::default();
        for i in 0..(MOST as u16 + 3) {
            f.add(ItemId(i), 1, 0.0);
        }
        assert_eq!(f.notes.len(), MOST);
        assert_eq!(f.notes[0].item, ItemId(3));
    }
}
