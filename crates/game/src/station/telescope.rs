//! The telescope: on a clear night, right click it and look through it. The
//! view narrows; turn to find faint stars, and hold a constellation in the
//! middle of the view until it's charted. Charted ones are drawn with their
//! lines from then on, and give XP the first time. The improved telescope
//! shows fainter ones (tier 2), the diamond lens the hidden ones (tier 3).
//! Constellations are in `assets/data/constellations.ron`; pure rules are at
//! the top of this file.

use std::collections::BTreeSet;

use bevy::prelude::*;
use serde::Deserialize;

use super::Urns;
use crate::camera::{FlyCamera, Menus};
use crate::clock::WorldClock;
use crate::sound::PlaySound;
use crate::survival::Toast;
use crate::theme::{colors, sizes};

/// How the view narrows: the field of view is divided by this.
pub const ZOOM: f32 = 3.0;
/// How near the middle of the view a constellation must be, radians.
pub const AIM: f32 = 0.06;
/// Seconds it must stay there to be charted.
pub const CHART_SECONDS: f32 = 1.6;
/// How cloudy the sky can be and still show the stars.
pub const MAX_CLOUDS: f32 = 0.55;

#[derive(Deserialize, Debug, Clone)]
pub struct Constellation {
    pub name: String,
    pub title: String,
    /// 1: any telescope, 2: the improved telescope, 3: the diamond lens.
    pub tier: u8,
    /// Where its middle stands at midnight: compass bearing and height above the horizon, degrees.
    pub midnight: (f32, f32),
    /// Its stars around the middle, degrees across and up.
    pub stars: Vec<(f32, f32)>,
    /// Lines between stars (indexes), drawn once it's charted.
    pub lines: Vec<(usize, usize)>,
    pub xp: f32,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct Constellations(pub Vec<Constellation>);

impl Constellations {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let list: Vec<Constellation> = ron::from_str(text).map_err(|e| format!("constellations.ron: {e}"))?;
        for c in &list {
            if c.stars.len() < 2 || c.lines.iter().any(|(a, b)| *a >= c.stars.len() || *b >= c.stars.len()) || !(1..=3).contains(&c.tier) {
                return Err(format!("constellations.ron: '{}' needs 2+ stars, lines between them and a tier 1-3", c.name));
            }
        }
        Ok(Self(list))
    }
}

/// The best tier the telescope at camp can see.
pub fn tier_seen(has: &dyn Fn(&str) -> bool) -> u8 {
    if has("diamond_lens") {
        3
    } else if has("improved_telescope") {
        2
    } else {
        1
    }
}

/// Direction of a bearing (0 = north, -z; 90 = east, +x) and height, degrees.
fn bearing(az: f32, alt: f32) -> Vec3 {
    let (az, alt) = (az.to_radians(), alt.to_radians());
    Vec3::new(az.sin() * alt.cos(), alt.sin(), -az.cos() * alt.cos())
}

/// Where a constellation's middle is at a time of day (0..1): the sky turns
/// like the sky dome's stars (`sky::sky_rotation`), fixed to where it stands at midnight.
pub fn centre(c: &Constellation, time: f32) -> Vec3 {
    let at_midnight = bearing(c.midnight.0, c.midnight.1);
    let star_map = crate::sky::sky_rotation(0.0).inverse() * at_midnight;
    (crate::sky::sky_rotation(time) * star_map).normalize()
}

/// The directions of its stars at a time of day.
pub fn stars(c: &Constellation, time: f32) -> Vec<Vec3> {
    let mid = centre(c, time);
    let right = mid.cross(Vec3::Y).normalize_or(Vec3::X);
    let up = right.cross(mid).normalize();
    c.stars.iter().map(|(x, y)| (mid + right * x.to_radians().tan() + up * y.to_radians().tan()).normalize()).collect()
}

/// The constellation you are holding in view: up, seen with this telescope, nearest the middle.
pub fn in_view(list: &[Constellation], time: f32, look: Vec3, tier: u8) -> Option<usize> {
    list.iter()
        .enumerate()
        .filter(|(_, c)| c.tier <= tier)
        .map(|(i, c)| (i, centre(c, time)))
        .filter(|(_, d)| d.y > 0.05)
        .map(|(i, d)| (i, d.angle_between(look)))
        .filter(|(_, a)| *a < AIM)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Looking through the telescope.
#[derive(Resource, Default)]
pub struct Scope {
    pub open: bool,
    /// The field of view to go back to.
    fov: Option<f32>,
    /// What's held in the middle, and for how long.
    holding: Option<(usize, f32)>,
    /// 0 = just put your eye to it, 1 = looking through.
    blend: f32,
}

#[derive(Component)]
pub(super) struct ScopeUi;

/// The line under the lens (what's charted).
#[derive(Component)]
pub(super) struct ScopeLine;

/// The round eyepiece, the dark around it, and the blink as you put your eye to it.
#[derive(Component)]
pub(super) struct ScopeLens;

#[derive(Component)]
pub(super) struct ScopeDark;

#[derive(Component)]
pub(super) struct ScopeBlink;

/// The eyepiece as an image: clear in the middle with faint cross hairs, a
/// darker rim and a brass ring, black outside the circle.
pub fn lens_image(size: u32) -> Vec<u8> {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let c = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = ((x as f32 + 0.5 - c) / c, (y as f32 + 0.5 - c) / c);
            let r = (dx * dx + dy * dy).sqrt();
            // Black outside, a soft edge, the lens darkening toward its rim.
            let mut a = ((r - 0.9) / 0.06).clamp(0.0, 1.0);
            a = a.max(((r - 0.55) / 0.35).clamp(0.0, 1.0).powf(2.0) * 0.45);
            let mut rgb = [0.0f32, 0.0, 0.0];
            // A thin brass ring just inside the edge.
            let ring = 1.0 - ((r - 0.905).abs() / 0.012).min(1.0);
            if ring > 0.0 {
                rgb = [0.55 * ring, 0.42 * ring, 0.2 * ring];
                a = a.max(ring * 0.9);
            }
            // Cross hairs, with a gap in the middle so the stars show.
            let line = |d: f32, along: f32| d.abs() < 0.0035 && (0.06..0.42).contains(&along.abs());
            if line(dx, dy) || line(dy, dx) {
                rgb = [0.75, 0.7, 0.55];
                a = a.max(0.35);
            }
            let i = ((y * size + x) * 4) as usize;
            px[i] = (rgb[0] * 255.0) as u8;
            px[i + 1] = (rgb[1] * 255.0) as u8;
            px[i + 2] = (rgb[2] * 255.0) as u8;
            px[i + 3] = (a * 255.0) as u8;
        }
    }
    px
}

pub(super) fn load_constellations(mut commands: Commands) {
    match Constellations::from_ron(include_str!("../../../../assets/data/constellations.ron")) {
        Ok(c) => commands.insert_resource(c),
        Err(e) => {
            error!("{e}");
            commands.insert_resource(Constellations::default());
        }
    }
}

pub(super) fn spawn_scope_ui(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const SIZE: u32 = 512;
    let mut img = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        lens_image(SIZE),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::linear();
    let lens = images.add(img);
    let full = || Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands
        .spawn((
            ScopeUi,
            Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), flex_direction: FlexDirection::Column, justify_content: JustifyContent::FlexEnd, align_items: AlignItems::Center, padding: UiRect::all(Val::Px(18.0)), ..default() },
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .with_children(|c| {
            // The eyepiece: a round lens the height of the screen, black either side.
            c.spawn((Node { flex_direction: FlexDirection::Row, ..full() }, Pickable::IGNORE)).with_children(|row| {
                row.spawn((ScopeDark, Node { flex_grow: 1.0, height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::BLACK)));
                row.spawn((ScopeLens, Node { height: Val::Percent(100.0), aspect_ratio: Some(1.0), ..default() }, ImageNode::new(lens)));
                row.spawn((ScopeDark, Node { flex_grow: 1.0, height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::BLACK)));
            });
            c.spawn((ScopeBlink, full(), BackgroundColor(Color::BLACK)));
            c.spawn((ScopeLine, crate::theme::text("", sizes::BODY, colors::TEXT), Node::default()));
            c.spawn((crate::theme::text("Turn to look around · hold a constellation in the middle to chart it · Right click or move: step back", sizes::SMALL, colors::FAINT), Node::default()));
        });
}

/// Step up to the telescope (from `use_station`): only on a clear night.
pub(super) fn look_through(scope: &mut Scope, menus: &mut Menus, hour: f32, clouds: f32, toast: &mut MessageWriter<Toast>) {
    if !crate::survival::camp::is_night(hour) {
        toast.write(Toast("The stars are out only at night: come back after dusk".into()));
    } else if clouds > MAX_CLOUDS {
        toast.write(Toast("Too cloudy to see the stars tonight".into()));
    } else {
        scope.open = true;
        scope.holding = None;
        scope.blend = 0.0;
        menus.still = true;
    }
}

/// While looking: narrow the view, chart what's held in the middle, step back on a click or a step.
#[allow(clippy::too_many_arguments)]
pub(super) fn scope_view(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    clock: Res<WorldClock>,
    list: Res<Constellations>,
    mut scope: ResMut<Scope>,
    mut menus: ResMut<Menus>,
    mut urns: ResMut<Urns>,
    mut cam: Single<(&Transform, &mut Projection), With<FlyCamera>>,
    (mut toast, mut gain, mut sound): (MessageWriter<Toast>, MessageWriter<crate::progress::GainXp>, MessageWriter<PlaySound>),
    mut ui: Query<&mut Visibility, With<ScopeUi>>,
    mut line_text: Query<&mut Text, With<ScopeLine>>,
    mut blink: Query<&mut BackgroundColor, (With<ScopeBlink>, Without<ScopeDark>)>,
    mut was_open: Local<bool>,
    mut screenshot: Local<bool>,
) {
    // `BLOCKGAME_SCOPE=1`: look through the telescope from the start, for screenshots.
    if !*screenshot {
        *screenshot = true;
        if std::env::var("BLOCKGAME_SCOPE").is_ok() {
            scope.open = true;
            menus.still = true;
        }
    }
    let (tf, proj) = &mut *cam;
    let moved = [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD, KeyCode::Space, KeyCode::Escape].iter().any(|k| keys.just_pressed(*k));
    if scope.open && (*was_open && mouse.just_pressed(MouseButton::Right) || moved || !crate::survival::camp::is_night(clock.hour()) || menus.open) {
        scope.open = false;
        menus.still = false;
    }
    *was_open = scope.open;
    let Projection::Perspective(p) = &mut **proj else {
        return;
    };
    // Putting your eye to it: the view goes dark for a blink, then the lens opens.
    scope.blend = if scope.open { (scope.blend + time.delta_secs() / 0.45).min(1.0) } else { 0.0 };
    let k = scope.blend * scope.blend * (3.0 - 2.0 * scope.blend);
    for mut b in &mut blink {
        b.0 = Color::srgba(0.0, 0.0, 0.0, 1.0 - k);
    }
    if let Ok(mut vis) = ui.single_mut() {
        let want = if scope.open { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if scope.open {
            let line = match scope.holding {
                Some((i, _)) if urns.charted.contains(&list.0[i].name) => {
                    format!("{} (charted)", list.0[i].title)
                }
                Some((_, t)) => format!("Charting a constellation... {:.0}%", (t / CHART_SECONDS * 100.0).min(100.0)),
                None => format!("{} of {} constellations charted", list.0.iter().filter(|c| urns.charted.contains(&c.name)).count(), list.0.len()),
            };
            if let Ok(mut t) = line_text.single_mut()
                && t.0 != line
            {
                t.0 = line;
            }
        }
    }
    if !scope.open {
        if let Some(f) = scope.fov.take() {
            p.fov = f;
        }
        return;
    }
    let base = *scope.fov.get_or_insert(p.fov);
    p.fov = base / (1.0 + (ZOOM - 1.0) * k);
    let has = |s: &str| urns.has(s);
    let tier = tier_seen(&has);
    let look = tf.forward().as_vec3();
    let now = in_view(&list.0, clock.time, look, tier);
    scope.holding = match (now, scope.holding) {
        (Some(i), Some((j, t))) if i == j => Some((i, t + time.delta_secs())),
        (Some(i), _) => Some((i, 0.0)),
        (None, _) => None,
    };
    if let Some((i, t)) = scope.holding
        && t >= CHART_SECONDS
        && !urns.charted.contains(&list.0[i].name)
    {
        let c = &list.0[i];
        urns.charted.insert(c.name.clone());
        sound.write(PlaySound::new("spell_on").gain(0.6));
        toast.write(Toast(format!("Charted {}", c.title)));
        gain.write(crate::progress::GainXp { xp: c.xp, why: format!("Charted {}", c.title) });
    }
}

/// The stars seen through the telescope, and the lines of the charted ones.
pub(super) fn draw_constellations(scope: Res<Scope>, clock: Res<WorldClock>, list: Res<Constellations>, urns: Res<Urns>, cam: Single<&Transform, With<FlyCamera>>, mut gizmos: Gizmos) {
    if !scope.open {
        return;
    }
    let has = |s: &str| urns.has(s);
    let tier = tier_seen(&has);
    let eye = cam.translation;
    let far = 120.0;
    let charted: &BTreeSet<String> = &urns.charted;
    for c in list.0.iter().filter(|c| c.tier <= tier) {
        let dirs = stars(c, clock.time);
        if centre(c, clock.time).y < 0.0 {
            continue;
        }
        let done = charted.contains(&c.name);
        let star = if done { Color::srgb(1.0, 0.86, 0.5) } else { Color::srgb(0.85, 0.9, 1.0) };
        for d in &dirs {
            let p = eye + *d * far;
            let r = d.cross(Vec3::Y).normalize_or(Vec3::X) * 0.35;
            let u = r.cross(*d).normalize() * 0.35;
            gizmos.line(p - r, p + r, star);
            gizmos.line(p - u, p + u, star);
        }
        if done {
            for (a, b) in &c.lines {
                gizmos.line(eye + dirs[*a] * far, eye + dirs[*b] * far, Color::srgba(1.0, 0.8, 0.45, 0.6));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Constellations {
        Constellations::from_ron(include_str!("../../../../assets/data/constellations.ron")).unwrap()
    }

    #[test]
    fn every_constellation_is_up_at_midnight_and_findable() {
        let l = list();
        assert!(l.0.iter().any(|c| c.tier == 1) && l.0.iter().any(|c| c.tier == 3));
        for (i, c) in l.0.iter().enumerate() {
            let mid = centre(c, 0.0);
            assert!(mid.y > 0.3, "{} is low at midnight: {mid}", c.name);
            assert_eq!(in_view(&l.0, 0.0, mid, 3), Some(i), "{}", c.name);
        }
    }

    #[test]
    fn hidden_ones_need_the_better_telescope() {
        let l = list();
        let hidden = l.0.iter().position(|c| c.tier == 3).unwrap();
        let look = centre(&l.0[hidden], 0.0);
        assert_eq!(in_view(&l.0, 0.0, look, 1), None);
        assert_eq!(in_view(&l.0, 0.0, look, 3), Some(hidden));
        assert_eq!(tier_seen(&|s| s == "improved_telescope"), 2);
    }

    #[test]
    fn the_lens_is_clear_in_the_middle_and_black_outside() {
        let size = 64;
        let px = lens_image(size);
        let alpha = |x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
        assert!(alpha(28, 28) < 40, "near the middle");
        assert_eq!(alpha(0, 0), 255, "the corner");
        assert_eq!(alpha(32, 0), 255, "past the top edge");
    }

    #[test]
    fn the_sky_turns_through_the_night() {
        let c = &list().0[0];
        assert!(centre(c, 0.0).angle_between(centre(c, 0.1)) > 0.3);
    }
}
