//! How the UI looks: the sprite sheet (`assets/textures/ui.png`, laid out in
//! `assets/textures/ui.ron`, placeholder made by `tools/gen_ui_art.py`), the
//! font (`assets/fonts/ui.ttf` and `ui_bold.ttf`, Pixelify Sans, OFL), the
//! text colours, and a shadow under every piece of text so it reads over a
//! bright sky or snow.
//!
//! Screens ask for sprites by name: `theme.skin("panel")` gives a node a
//! stretchable frame, `theme.icon("icon_food")` a picture. Every pixel of the
//! sheet is drawn `scale` (2) screen points wide.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::sprite::{BorderRect, TextureSlicer};
use bevy::text::FontSize;
use bevy::ui::widget::NodeImageMode;
use bevy::ui::VisualBox;
use serde::Deserialize;

pub struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        let spec: SheetSpec = ron::from_str(include_str!("../../../assets/textures/ui.ron")).unwrap_or_else(|e| panic!("ui.ron: {e}"));
        // The game's font replaces Bevy's built-in one, so every piece of
        // text uses it without asking.
        let bold = {
            let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
            let _ = fonts.insert(AssetId::default(), Font::from_bytes(include_bytes!("../../../assets/fonts/ui.ttf").to_vec()));
            fonts.add(Font::from_bytes(include_bytes!("../../../assets/fonts/ui_bold.ttf").to_vec()))
        };
        let sheet = app.world().resource::<AssetServer>().load("textures/ui.png");
        app.insert_resource(Theme { sheet, sprites: spec.sprites, scale: spec.scale, bold }).add_systems(PostUpdate, shadow_all_text);
    }
}

#[derive(Deserialize)]
struct SheetSpec {
    scale: f32,
    sprites: HashMap<String, SpriteSpec>,
}

#[derive(Deserialize, Clone, Copy, Debug)]
struct SpriteSpec {
    rect: (u32, u32, u32, u32),
    slice: Option<(u32, u32, u32, u32)>,
}

/// The UI sprite sheet and fonts.
#[derive(Resource, Clone)]
pub struct Theme {
    sheet: Handle<Image>,
    sprites: HashMap<String, SpriteSpec>,
    /// Screen points per sheet pixel.
    scale: f32,
    pub bold: Handle<Font>,
}

/// A node drawn with a sprite from the UI sheet.
#[derive(Component)]
pub struct Skinned;

impl Theme {
    fn spec(&self, name: &str) -> SpriteSpec {
        match self.sprites.get(name) {
            Some(s) => *s,
            None => {
                warn_once!("ui.ron has no sprite '{name}'");
                SpriteSpec { rect: (0, 0, 1, 1), slice: None }
            }
        }
    }

    /// A sprite as an image, stretched to fill its node (frames keep their
    /// corners and stretch their edges).
    pub fn image(&self, name: &str) -> ImageNode {
        let s = self.spec(name);
        let (x, y, w, h) = s.rect;
        let mode = match s.slice {
            Some((l, t, r, b)) => NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect { min_inset: Vec2::new(l as f32, t as f32), max_inset: Vec2::new(r as f32, b as f32) },
                max_corner_scale: self.scale,
                ..default()
            }),
            None => NodeImageMode::Stretch,
        };
        ImageNode {
            image: self.sheet.clone(),
            rect: Some(Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32)),
            image_mode: mode,
            visual_box: VisualBox::BorderBox,
            ..default()
        }
    }

    /// A frame for a node: `commands.spawn((node, theme.skin("panel")))`.
    pub fn skin(&self, name: &str) -> impl Bundle {
        (self.image(name), Skinned)
    }

    /// A frame tinted with a colour (white parts take the colour).
    pub fn skin_tinted(&self, name: &str, color: Color) -> impl Bundle {
        (self.image(name).with_color(color), Skinned)
    }

    /// A sprite at its own size times the pixel scale, e.g. an icon.
    pub fn icon(&self, name: &str) -> impl Bundle {
        let size = self.size(name);
        (self.image(name), Node { width: Val::Px(size.x), height: Val::Px(size.y), flex_shrink: 0.0, ..default() })
    }

    /// A sprite's size on screen, in points.
    pub fn size(&self, name: &str) -> Vec2 {
        let s = self.spec(name);
        Vec2::new(s.rect.2 as f32, s.rect.3 as f32) * self.scale
    }

    /// The bold face.
    pub fn bold(&self, size: f32) -> TextFont {
        TextFont { font: self.bold.clone().into(), font_size: FontSize::Px(size), ..default() }
    }
}

/// Text colours.
pub mod colors {
    use bevy::prelude::Color;

    /// Ordinary text.
    pub const TEXT: Color = Color::srgb(0.95, 0.93, 0.86);
    /// Secondary text: hints, labels.
    pub const MUTED: Color = Color::srgb(0.72, 0.74, 0.68);
    /// Things you can't use right now.
    pub const FAINT: Color = Color::srgb(0.50, 0.52, 0.48);
    /// Titles and headings.
    pub const HEADING: Color = Color::srgb(0.98, 0.84, 0.50);
    /// The selected thing.
    pub const GOLD: Color = Color::srgb(1.0, 0.80, 0.32);
    /// Something you can do.
    pub const GOOD: Color = Color::srgb(0.64, 0.93, 0.52);
    /// A warning.
    pub const BAD: Color = Color::srgb(1.0, 0.48, 0.40);
}

/// Text sizes, in points.
pub mod sizes {
    pub const SMALL: f32 = 13.0;
    pub const BODY: f32 = 16.0;
    pub const LABEL: f32 = 18.0;
    pub const HEADING: f32 = 22.0;
    pub const TITLE: f32 = 30.0;
}

/// The shadow put under text so it reads on any background.
pub const SHADOW: TextShadow = TextShadow { offset: Vec2::new(2.0, 2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) };

/// A piece of text in the game's font, with a shadow.
pub fn text(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (Text::new(s), TextFont { font_size: FontSize::Px(size), ..default() }, TextColor(color), SHADOW)
}

/// Text that should have no shadow (e.g. on a light button).
#[derive(Component)]
pub struct NoShadow;

/// Every piece of UI text gets a shadow unless it has one or asks not to.
#[allow(clippy::type_complexity)]
fn shadow_all_text(mut commands: Commands, q: Query<Entity, (Added<Text>, Without<TextShadow>, Without<NoShadow>)>) {
    for e in &q {
        commands.entity(e).insert(SHADOW);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sheet_lists_what_the_game_uses() {
        let spec: super::SheetSpec = ron::from_str(include_str!("../../../assets/textures/ui.ron")).unwrap();
        for name in [
            "panel", "panel_light", "inset", "header", "tooltip", "banner", "divider", "slot", "slot_hover", "slot_selected", "item", "button",
            "button_hover", "button_down", "button_off", "bar", "fill", "knob", "node_major", "node_minor", "node_halo", "crosshair",
            "icon_damage", "icon_fatigue", "icon_blood", "icon_injuries", "icon_food", "icon_air", "icon_warmth", "icon_load", "icon_focus",
            "icon_xp", "icon_levitate", "icon_light", "icon_lock", "icon_check", "icon_close", "icon_camp", "white",
        ] {
            let s = spec.sprites.get(name).unwrap_or_else(|| panic!("missing {name}"));
            if let Some((l, t, r, b)) = s.slice {
                assert!(l + r < s.rect.2 && t + b < s.rect.3, "{name}: slice bigger than the sprite");
            }
        }
    }
}
