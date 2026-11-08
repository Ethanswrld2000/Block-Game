//! Settings (view distance, mouse, field of view, sound) and the pause menu.
//!
//! Esc opens the menu (Resume, Settings, Save and quit); settings are saved to
//! `settings.ron` next to the world folders, so every world shares them.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use serde::{Deserialize, Serialize};

use crate::camera::{FlyCamera, Menus};
use crate::save::SaveDir;
use crate::theme::{colors, sizes, Theme};
use crate::world::ViewDistance;

pub struct SettingsPlugin;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApplySettings;

impl SettingsPlugin {
    /// Settings are applied in this set (the bench overrides them after it).
    pub const APPLY: ApplySettings = ApplySettings;
}

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Settings>()
            .init_resource::<PauseMenu>()
            .add_systems(PreStartup, load_settings)
            .add_systems(Startup, spawn_menu)
            // Before other screens see Esc, so closing one of them doesn't also pause.
            .add_systems(PreUpdate, open_close.after(bevy::input::InputSystems))
            .add_systems(Update, (hover_and_click, refresh, apply).chain().in_set(SettingsPlugin::APPLY));
    }
}

#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Chunks loaded around you.
    pub view_distance: i32,
    /// Multiplies mouse look speed.
    pub sensitivity: f32,
    /// Vertical field of view, degrees.
    pub fov: f32,
    /// 0..1, all sound.
    pub volume: f32,
    /// 0..1, wind, rain and night sounds.
    pub ambience: f32,
    /// The F3 overlay at start.
    pub debug_overlay: bool,
    /// 0 off, 1 fast, 2 fancy.
    pub clouds: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self { view_distance: 12, sensitivity: 1.0, fov: 70.0, volume: 0.8, ambience: 0.7, debug_overlay: true, clouds: 1 }
    }
}

/// One adjustable line in the settings panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Knob {
    ViewDistance,
    Sensitivity,
    Fov,
    Volume,
    Ambience,
    Clouds,
    DebugOverlay,
}

impl Knob {
    const ALL: [Knob; 7] = [Knob::ViewDistance, Knob::Sensitivity, Knob::Fov, Knob::Volume, Knob::Ambience, Knob::Clouds, Knob::DebugOverlay];

    fn label(self) -> &'static str {
        match self {
            Knob::ViewDistance => "View distance",
            Knob::Sensitivity => "Mouse speed",
            Knob::Fov => "Field of view",
            Knob::Volume => "Volume",
            Knob::Ambience => "Ambience",
            Knob::Clouds => "Clouds",
            Knob::DebugOverlay => "Debug overlay (F3)",
        }
    }

    fn value(self, s: &Settings) -> String {
        match self {
            Knob::ViewDistance => format!("{} chunks", s.view_distance),
            Knob::Sensitivity => format!("{:.0}%", s.sensitivity * 100.0),
            Knob::Fov => format!("{:.0}°", s.fov),
            Knob::Volume => format!("{:.0}%", s.volume * 100.0),
            Knob::Ambience => format!("{:.0}%", s.ambience * 100.0),
            Knob::Clouds => ["off", "fast", "fancy"][s.clouds.min(2) as usize].into(),
            Knob::DebugOverlay => (if s.debug_overlay { "on" } else { "off" }).into(),
        }
    }

    fn step(self, s: &mut Settings, dir: f32) {
        match self {
            Knob::ViewDistance => s.view_distance = (s.view_distance + 2 * dir as i32).clamp(4, 32),
            Knob::Sensitivity => s.sensitivity = (s.sensitivity + 0.1 * dir).clamp(0.2, 3.0),
            Knob::Fov => s.fov = (s.fov + 5.0 * dir).clamp(50.0, 110.0),
            Knob::Volume => s.volume = (s.volume + 0.1 * dir).clamp(0.0, 1.0),
            Knob::Ambience => s.ambience = (s.ambience + 0.1 * dir).clamp(0.0, 1.0),
            Knob::Clouds => s.clouds = (s.clouds as i32 + dir as i32).clamp(0, 2) as u8,
            Knob::DebugOverlay => s.debug_overlay = !s.debug_overlay,
        }
    }
}

#[derive(Resource, Default)]
struct PauseMenu {
    open: bool,
    settings: bool,
}

#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum Button {
    Resume,
    Settings,
    Back,
    Quit,
    Step(usize, i8),
}

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct MainPage;
#[derive(Component)]
struct SettingsPage;
#[derive(Component)]
struct KnobValue(usize);

fn settings_file(dir: &SaveDir) -> std::path::PathBuf {
    dir.shared_file("settings.ron")
}

fn load_settings(dir: Res<SaveDir>, mut settings: ResMut<Settings>) {
    if let Ok(text) = std::fs::read_to_string(settings_file(&dir)) {
        match ron::from_str::<Settings>(&text) {
            Ok(s) => *settings = s,
            Err(e) => warn!("settings.ron: {e}"),
        }
    }
}

fn store(dir: &SaveDir, s: &Settings) {
    if std::env::var_os("BLOCKGAME_SCREENSHOT").is_some() {
        return;
    }
    if let Ok(text) = ron::ser::to_string_pretty(s, ron::ser::PrettyConfig::default())
        && let Err(e) = std::fs::write(settings_file(dir), text)
    {
        warn!("saving settings.ron: {e}");
    }
}

fn text(s: impl Into<String>, size: f32) -> impl Bundle {
    crate::theme::text(s, size, colors::TEXT)
}

fn button(theme: &Theme, kind: Button, label: &str, width: f32) -> impl Bundle {
    (
        kind,
        Node { width: Val::Px(width), height: Val::Px(44.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        theme.skin("button"),
        children![text(label.to_string(), sizes::LABEL)],
    )
}

fn spawn_menu(mut commands: Commands, theme: Res<Theme>) {
    let theme = &*theme;
    let page = |display| Node {
        display,
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: Val::Px(10.0),
        padding: UiRect::axes(Val::Px(36.0), Val::Px(26.0)),
        ..default()
    };
    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.01, 0.0, 0.7)),
            GlobalZIndex(30),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((MainPage, page(Display::Flex), theme.skin("panel"))).with_children(|p| {
                p.spawn((theme.bold(sizes::TITLE), Text::new("Paused"), TextColor(colors::HEADING)));
                p.spawn((Node { width: Val::Px(260.0), height: Val::Px(8.0), margin: UiRect::bottom(Val::Px(6.0)), ..default() }, theme.skin("divider")));
                p.spawn(button(theme, Button::Resume, "Resume", 260.0));
                p.spawn(button(theme, Button::Settings, "Settings", 260.0));
                p.spawn(button(theme, Button::Quit, "Save and quit", 260.0));
            });
            root.spawn((SettingsPage, page(Display::None), theme.skin("panel"))).with_children(|p| {
                p.spawn((theme.bold(sizes::TITLE), Text::new("Settings"), TextColor(colors::HEADING)));
                p.spawn((Node { width: Val::Px(520.0), height: Val::Px(8.0), margin: UiRect::bottom(Val::Px(6.0)), ..default() }, theme.skin("divider")));
                for (i, k) in Knob::ALL.iter().enumerate() {
                    p.spawn(Node { width: Val::Px(540.0), align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|row| {
                        row.spawn((Node { width: Val::Px(230.0), ..default() }, children![text(k.label(), sizes::LABEL)]));
                        row.spawn(button(theme, Button::Step(i, -1), "-", 44.0));
                        row.spawn((
                            Node { width: Val::Px(160.0), height: Val::Px(40.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                            theme.skin("inset"),
                            children![(KnobValue(i), crate::theme::text("", sizes::LABEL, colors::HEADING))],
                        ));
                        row.spawn(button(theme, Button::Step(i, 1), "+", 44.0));
                    });
                }
                p.spawn((Node { height: Val::Px(6.0), ..default() },));
                p.spawn(button(theme, Button::Back, "Back", 260.0));
            });
        });
}

fn open_close(
    keys: Res<ButtonInput<KeyCode>>,
    mut pause: ResMut<PauseMenu>,
    mut menus: ResMut<Menus>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut forced: Local<bool>,
) {
    // `BLOCKGAME_PAUSE=1` (or `=settings`) opens the menu for screenshots.
    if !*forced {
        *forced = true;
        if let Ok(v) = std::env::var("BLOCKGAME_PAUSE") {
            pause.open = true;
            pause.settings = v == "settings";
            menus.open = true;
            return;
        }
    }
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if pause.open {
        if pause.settings {
            pause.settings = false;
        } else {
            pause.open = false;
            menus.open = false;
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
    } else if !menus.open {
        // Esc closes other screens itself; with none open it pauses.
        pause.open = true;
        menus.open = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn hover_and_click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut buttons: Query<(&Button, &ComputedNode, &UiGlobalTransform, &InheritedVisibility, &mut ImageNode)>,
    (theme, mouse_down): (Res<Theme>, Res<ButtonInput<MouseButton>>),
    mut pause: ResMut<PauseMenu>,
    mut menus: ResMut<Menus>,
    mut settings: ResMut<Settings>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !pause.open {
        return;
    }
    let p = window.physical_cursor_position();
    let mut clicked = None;
    for (b, node, tf, vis, mut img) in &mut buttons {
        let over = vis.get() && p.is_some_and(|p| node.contains_point(*tf, p));
        let want = if over && mouse_down.pressed(MouseButton::Left) {
            "button_down"
        } else if over {
            "button_hover"
        } else {
            "button"
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
        Some(Button::Resume) => {
            pause.open = false;
            menus.open = false;
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
        Some(Button::Settings) => pause.settings = true,
        Some(Button::Back) => pause.settings = false,
        // Every system that saves on exit runs before the app closes.
        Some(Button::Quit) => {
            exit.write(AppExit::Success);
        }
        Some(Button::Step(i, d)) => Knob::ALL[i].step(&mut settings, d as f32),
        None => {}
    }
}

#[allow(clippy::type_complexity)]
fn refresh(
    pause: Res<PauseMenu>,
    settings: Res<Settings>,
    mut root: Query<&mut Visibility, With<MenuRoot>>,
    mut main: Query<&mut Node, (With<MainPage>, Without<SettingsPage>)>,
    mut page: Query<&mut Node, (With<SettingsPage>, Without<MainPage>)>,
    mut values: Query<(&KnobValue, &mut Text)>,
) {
    if !pause.is_changed() && !settings.is_changed() {
        return;
    }
    for mut v in &mut root {
        *v = if pause.open { Visibility::Inherited } else { Visibility::Hidden };
    }
    for mut n in &mut main {
        n.display = if pause.settings { Display::None } else { Display::Flex };
    }
    for mut n in &mut page {
        n.display = if pause.settings { Display::Flex } else { Display::None };
    }
    for (k, mut t) in &mut values {
        t.0 = Knob::ALL[k.0].value(&settings);
    }
}

/// Settings take effect (and are saved) when they change; the -/= keys
/// change the view distance setting too.
fn apply(
    dir: Res<SaveDir>,
    mut settings: ResMut<Settings>,
    mut view: ResMut<ViewDistance>,
    mut cams: Query<(&mut FlyCamera, &mut Projection)>,
    mut first: Local<bool>,
) {
    if view.is_changed() && !settings.is_changed() && view.chunks != settings.view_distance && *first {
        settings.view_distance = view.chunks;
    }
    if !settings.is_changed() {
        return;
    }
    if view.chunks != settings.view_distance {
        view.chunks = settings.view_distance;
    }
    for (mut c, mut proj) in &mut cams {
        c.sensitivity = 0.002 * settings.sensitivity;
        if let Projection::Perspective(p) = &mut *proj {
            p.fov = settings.fov.to_radians();
        }
    }
    if *first {
        store(&dir, &settings);
    }
    *first = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knobs_stay_in_range() {
        let mut s = Settings::default();
        for _ in 0..50 {
            Knob::ViewDistance.step(&mut s, 1.0);
            Knob::Volume.step(&mut s, -1.0);
        }
        assert_eq!(s.view_distance, 32);
        assert_eq!(s.volume, 0.0);
        let text = ron::to_string(&s).unwrap();
        assert_eq!(ron::from_str::<Settings>(&text).unwrap(), s);
        // Missing fields fall back to defaults.
        assert_eq!(ron::from_str::<Settings>("(fov: 90.0)").unwrap().view_distance, 12);
    }
}
