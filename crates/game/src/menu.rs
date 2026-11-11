//! The title screen: continue the last world, make a new one (name and seed),
//! load another, or quit.
//!
//! Nothing of the game runs until a world is picked. Game plugins put their
//! systems in the `Game*` schedules below instead of Bevy's `Startup` and
//! `Update`; entering `Screen::Game` runs the three startup schedules once
//! (the world's `SaveDir` and `WorldSeed` are in place by then), and from then
//! on `GameUpdate` runs inside `Update` every frame. Systems in `First`,
//! `PreUpdate` or `Last` use `run_if(in_game)`.
//!
//! Screenshot and bench runs skip the screen and open `saves/world1` (or
//! `BLOCKGAME_WORLD=<folder>`). `BLOCKGAME_MENU=1|new|load` keeps it up, with
//! `BLOCKGAME_SCREENSHOT` too, to take a picture of it.

use bevy::ecs::schedule::ScheduleLabel;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::ui::ScrollPosition;
use bevy::window::PrimaryWindow;

use crate::save::{self, SaveDir, SavedWorld};
use crate::theme::{colors, sizes, Theme};
use crate::world::WorldSeed;

/// Startup schedules of a world, run in this order when it opens.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GamePreStartup;
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameStartup;
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GamePostStartup;
/// The game's `Update`, run inside `Update` while a world is open.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameUpdate;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    #[default]
    Title,
    Game,
}

/// Run condition for game systems outside `GameUpdate`.
pub fn in_game(screen: Option<Res<State<Screen>>>) -> bool {
    screen.is_some_and(|s| *s.get() == Screen::Game)
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<Screen>()
            .init_schedule(GamePreStartup)
            .init_schedule(GameStartup)
            .init_schedule(GamePostStartup)
            .init_schedule(GameUpdate)
            .init_resource::<Menu>()
            .add_systems(Startup, skip_title)
            .add_systems(OnEnter(Screen::Title), spawn_title)
            .add_systems(OnEnter(Screen::Game), start_game)
            .add_systems(Update, run_game.run_if(in_state(Screen::Game)))
            .add_systems(
                Update,
                (typing, hover_and_click, scroll_list, refresh, blink, menu_screenshot).chain().run_if(in_state(Screen::Title)),
            );
    }
}

fn start_game(world: &mut World) {
    world.run_schedule(GamePreStartup);
    world.run_schedule(GameStartup);
    world.run_schedule(GamePostStartup);
}

fn run_game(world: &mut World) {
    world.run_schedule(GameUpdate);
}

/// Opens the world in `saves/<folder>` and starts the game.
fn open_world(commands: &mut Commands, next: &mut NextState<Screen>, folder: &str) {
    let (dir, meta) = SaveDir::open(folder);
    info!("opening world \"{}\" ({folder}), seed {}", meta.name, meta.seed);
    commands.insert_resource(dir);
    commands.insert_resource(WorldSeed(meta.seed));
    next.set(Screen::Game);
}

/// The menu page asked for by `BLOCKGAME_MENU`, if any.
fn forced_page() -> Option<Page> {
    match std::env::var("BLOCKGAME_MENU").ok()?.as_str() {
        "new" => Some(Page::New),
        "load" => Some(Page::Load),
        _ => Some(Page::Main),
    }
}

/// Test runs go straight into a world.
fn skips_title() -> bool {
    forced_page().is_none()
        && ["BLOCKGAME_WORLD", "BLOCKGAME_SCREENSHOT", "BLOCKGAME_BENCH"].iter().any(|v| std::env::var_os(v).is_some())
}

fn skip_title(mut commands: Commands, mut next: ResMut<NextState<Screen>>) {
    if skips_title() {
        let folder = std::env::var("BLOCKGAME_WORLD").unwrap_or_else(|_| "world1".into());
        open_world(&mut commands, &mut next, &folder);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Page {
    #[default]
    Main,
    New,
    Load,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Field {
    #[default]
    Name,
    Seed,
}

const MAX_NAME: usize = 32;
const MAX_SEED: usize = 24;

#[derive(Resource, Default)]
struct Menu {
    page: Page,
    worlds: Vec<SavedWorld>,
    name: String,
    seed: String,
    focus: Field,
    /// The page needs rebuilding.
    built: Option<Page>,
}

impl Menu {
    fn go(&mut self, page: Page) {
        self.page = page;
        if page == Page::New {
            self.name = format!("World {}", self.worlds.len() + 1);
            self.seed.clear();
            self.focus = Field::Name;
        }
    }
}

#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum MenuButton {
    Continue,
    New,
    Load,
    Quit,
    Create,
    Back,
    Play(usize),
    Field(Field),
}

#[derive(Component)]
struct PageRoot;
#[derive(Component)]
struct FieldText(Field);
#[derive(Component)]
struct WorldList;

fn spawn_title(mut commands: Commands, mut menu: ResMut<Menu>) {
    if skips_title() {
        return;
    }
    menu.worlds = save::list_worlds(&save::saves_root());
    menu.built = None;
    let page = forced_page().unwrap_or_default();
    menu.go(page);
    if std::env::var("BLOCKGAME_MENU").as_deref() == Ok("new") {
        menu.seed = "bog lantern".into();
    }
    commands.spawn((Camera2d, DespawnOnExit(Screen::Title)));
    commands
        .spawn((
            DespawnOnExit(Screen::Title),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(28.0),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::auto(Color::srgb(0.13, 0.20, 0.19)),
                ColorStop::auto(Color::srgb(0.07, 0.11, 0.09)),
                ColorStop::auto(Color::srgb(0.03, 0.05, 0.04)),
            ])),
        ))
        .with_children(|root| {
            root.spawn((Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() },)).with_children(|t| {
                t.spawn(crate::theme::text("BLOCK GAME", sizes::HUGE * 1.5, colors::HEADING));
                t.spawn(crate::theme::text("The Great Swamp", sizes::LABEL, colors::MUTED));
            });
            root.spawn((
                PageRoot,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::axes(Val::Px(36.0), Val::Px(26.0)),
                    min_width: Val::Px(360.0),
                    ..default()
                },
            ));
        });
}

fn button(theme: &Theme, kind: MenuButton, label: &str, width: f32) -> impl Bundle {
    (
        kind,
        Node { width: Val::Px(width), height: Val::Px(44.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        theme.skin("button"),
        children![crate::theme::text(label.to_string(), sizes::LABEL, colors::TEXT)],
    )
}

fn divider(theme: &Theme, width: f32) -> impl Bundle {
    (Node { width: Val::Px(width), height: Val::Px(8.0), margin: UiRect::bottom(Val::Px(6.0)), ..default() }, theme.skin("divider"))
}

fn label(s: &str) -> impl Bundle {
    (
        Node { width: Val::Px(420.0), margin: UiRect::top(Val::Px(4.0)), ..default() },
        children![crate::theme::text(s.to_string(), sizes::BODY, colors::MUTED)],
    )
}

fn field(theme: &Theme, f: Field) -> impl Bundle {
    (
        MenuButton::Field(f),
        Node { width: Val::Px(420.0), height: Val::Px(44.0), align_items: AlignItems::Center, padding: UiRect::horizontal(Val::Px(12.0)), ..default() },
        theme.skin("inset"),
        children![(FieldText(f), crate::theme::text("", sizes::LABEL, colors::TEXT))],
    )
}

/// Rebuilds the panel when the page changes.
fn refresh(mut commands: Commands, theme: Res<Theme>, mut menu: ResMut<Menu>, root: Single<Entity, With<PageRoot>>) {
    if menu.built == Some(menu.page) {
        return;
    }
    menu.built = Some(menu.page);
    let theme = &*theme;
    let root = *root;
    commands.entity(root).despawn_children().insert(theme.skin("panel"));
    let now = save::now_secs();
    let menu = &*menu;
    commands.entity(root).with_children(|p| match menu.page {
        Page::Main => {
            if let Some(w) = menu.worlds.first() {
                p.spawn(button(theme, MenuButton::Continue, "Continue", 300.0));
                p.spawn(crate::theme::text(format!("{} - played {}", w.name(), save::ago(w.meta.last_played, now)), sizes::BODY, colors::MUTED));
                p.spawn(Node { height: Val::Px(4.0), ..default() });
            }
            p.spawn(button(theme, MenuButton::New, "New world", 300.0));
            if !menu.worlds.is_empty() {
                p.spawn(button(theme, MenuButton::Load, "Load world", 300.0));
            }
            p.spawn(button(theme, MenuButton::Quit, "Quit", 300.0));
        }
        Page::New => {
            p.spawn((theme.bold(sizes::TITLE), Text::new("New world"), TextColor(colors::HEADING)));
            p.spawn(divider(theme, 420.0));
            p.spawn(label("Name"));
            p.spawn(field(theme, Field::Name));
            p.spawn(label("Seed (a number or any words; blank for a random one)"));
            p.spawn(field(theme, Field::Seed));
            p.spawn(Node { height: Val::Px(8.0), ..default() });
            p.spawn(Node { column_gap: Val::Px(12.0), ..default() }).with_children(|row| {
                row.spawn(button(theme, MenuButton::Back, "Back", 200.0));
                row.spawn(button(theme, MenuButton::Create, "Create", 200.0));
            });
        }
        Page::Load => {
            p.spawn((theme.bold(sizes::TITLE), Text::new("Worlds"), TextColor(colors::HEADING)));
            p.spawn(divider(theme, 480.0));
            p.spawn((
                WorldList,
                Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), max_height: Val::Px(360.0), overflow: Overflow::scroll_y(), ..default() },
                ScrollPosition::default(),
            ))
            .with_children(|list| {
                for (i, w) in menu.worlds.iter().enumerate() {
                    list.spawn((
                        MenuButton::Play(i),
                        Node {
                            width: Val::Px(480.0),
                            min_height: Val::Px(64.0),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::Center,
                            padding: UiRect::axes(Val::Px(16.0), Val::Px(6.0)),
                            ..default()
                        },
                        theme.skin("button"),
                    ))
                    .with_children(|row| {
                        row.spawn(crate::theme::text(w.name().to_string(), sizes::LABEL, colors::TEXT));
                        row.spawn(crate::theme::text(
                            format!("Seed {} - played {}", w.meta.seed, save::ago(w.meta.last_played, now)),
                            sizes::SMALL,
                            colors::TEXT,
                        ));
                    });
                }
            });
            p.spawn(Node { height: Val::Px(6.0), ..default() });
            p.spawn(button(theme, MenuButton::Back, "Back", 300.0));
        }
    });
}

/// Typing into the name and seed fields; Tab switches, Enter creates, Esc goes back.
fn typing(
    mut keys: MessageReader<KeyboardInput>,
    mut menu: ResMut<Menu>,
    mut commands: Commands,
    mut next: ResMut<NextState<Screen>>,
) {
    for k in keys.read() {
        if k.state != ButtonState::Pressed {
            continue;
        }
        if menu.page != Page::New {
            if k.logical_key == Key::Escape && menu.page == Page::Load {
                menu.go(Page::Main);
            }
            continue;
        }
        match &k.logical_key {
            Key::Escape => menu.go(Page::Main),
            Key::Tab => menu.focus = if menu.focus == Field::Name { Field::Seed } else { Field::Name },
            Key::Enter => create(&mut menu, &mut commands, &mut next),
            Key::Backspace => {
                let f = menu.focus;
                text_of(&mut menu, f).pop();
            }
            _ => {
                let Some(t) = &k.text else { continue };
                let f = menu.focus;
                let max = if f == Field::Name { MAX_NAME } else { MAX_SEED };
                let s = text_of(&mut menu, f);
                for c in t.chars().filter(|c| !c.is_control()) {
                    if s.chars().count() < max {
                        s.push(c);
                    }
                }
            }
        }
    }
}

fn text_of(menu: &mut Menu, f: Field) -> &mut String {
    match f {
        Field::Name => &mut menu.name,
        Field::Seed => &mut menu.seed,
    }
}

fn create(menu: &mut Menu, commands: &mut Commands, next: &mut NextState<Screen>) {
    let name = menu.name.trim();
    let name = if name.is_empty() { format!("World {}", menu.worlds.len() + 1) } else { name.to_string() };
    let seed = save::seed_from_text(&menu.seed).unwrap_or_else(save::random_seed);
    let folder = save::create_world(&save::saves_root(), &name, seed);
    open_world(commands, next, &folder);
}

#[allow(clippy::too_many_arguments)]
fn hover_and_click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut buttons: Query<(&MenuButton, &ComputedNode, &UiGlobalTransform, &InheritedVisibility, &mut ImageNode)>,
    theme: Res<Theme>,
    mut menu: ResMut<Menu>,
    mut commands: Commands,
    mut next: ResMut<NextState<Screen>>,
    mut exit: MessageWriter<AppExit>,
) {
    let p = window.physical_cursor_position();
    let mut clicked = None;
    for (b, node, tf, vis, mut img) in &mut buttons {
        let over = vis.get() && p.is_some_and(|p| node.contains_point(*tf, p));
        let want = match b {
            MenuButton::Field(f) => {
                if *f == menu.focus {
                    "slot_selected"
                } else {
                    "inset"
                }
            }
            _ if over && mouse.pressed(MouseButton::Left) => "button_down",
            _ if over => "button_hover",
            _ => "button",
        };
        let skin = theme.image(want);
        if img.rect != skin.rect {
            *img = skin;
        }
        if over && mouse.just_pressed(MouseButton::Left) {
            clicked = Some(*b);
        }
    }
    match clicked {
        Some(MenuButton::Continue) => {
            if let Some(w) = menu.worlds.first() {
                open_world(&mut commands, &mut next, &w.folder.clone());
            }
        }
        Some(MenuButton::Play(i)) => {
            if let Some(w) = menu.worlds.get(i) {
                open_world(&mut commands, &mut next, &w.folder.clone());
            }
        }
        Some(MenuButton::New) => menu.go(Page::New),
        Some(MenuButton::Load) => menu.go(Page::Load),
        Some(MenuButton::Back) => menu.go(Page::Main),
        Some(MenuButton::Create) => create(&mut menu, &mut commands, &mut next),
        Some(MenuButton::Field(f)) => menu.focus = f,
        Some(MenuButton::Quit) => {
            exit.write(AppExit::Success);
        }
        None => {}
    }
}

fn scroll_list(scroll: Res<AccumulatedMouseScroll>, mut list: Query<&mut ScrollPosition, With<WorldList>>) {
    if scroll.delta.y == 0.0 {
        return;
    }
    let step = match scroll.unit {
        bevy::input::mouse::MouseScrollUnit::Line => 40.0,
        bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
    };
    for mut pos in &mut list {
        pos.y = (pos.y - scroll.delta.y * step).max(0.0);
    }
}

/// Field text with a blinking caret in the focused one.
fn blink(time: Res<Time>, menu: Res<Menu>, mut texts: Query<(&FieldText, &mut Text, &mut TextColor)>) {
    let caret = ((time.elapsed_secs() * 2.0) as u32).is_multiple_of(2);
    for (f, mut text, mut color) in &mut texts {
        let value = match f.0 {
            Field::Name => &menu.name,
            Field::Seed => &menu.seed,
        };
        let focused = f.0 == menu.focus;
        let shown = if value.is_empty() && !focused && f.0 == Field::Seed {
            color.0 = colors::FAINT;
            "random".to_string()
        } else {
            color.0 = colors::TEXT;
            format!("{value}{}", if focused && caret { "_" } else { "" })
        };
        if text.0 != shown {
            text.0 = shown;
        }
    }
}

/// `BLOCKGAME_MENU=.. BLOCKGAME_SCREENSHOT=out.png`: a picture of the title screen.
fn menu_screenshot(mut commands: Commands, mut frames: Local<u32>, mut exit: MessageWriter<AppExit>) {
    let (Some(_), Ok(path)) = (forced_page(), std::env::var("BLOCKGAME_SCREENSHOT")) else { return };
    *frames += 1;
    if *frames == 30 {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if *frames == 60 {
        exit.write(AppExit::Success);
    }
}
