//! Putting things into an urn from its tree: each thing put in flies up from
//! below the bottom of the screen into its picture under "It takes", on the
//! same timing as a craft coming together at a bench (`forge`): it closes in
//! over `forge::GATHER` with the glint flashing stronger, then pops
//! (`craft_pop`) and the picture swells in like the made thing does.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::forge::{APPEAR, GATHER, ease_out_back, gather_strength};
use super::tree::{NodeFrame, TreeUi};
use crate::inventory::grid::Stack;
use crate::item_art::ItemArt;
use crate::items::Items;
use crate::sound::PlaySound;
use crate::theme::Theme;

/// Something just went into the urn from the tree's "Put in".
#[derive(Message, Debug, Clone)]
pub(super) struct FlyIn {
    pub item: String,
    pub step: String,
}

/// A picture under "It takes" (the item's name): where things fly to.
#[derive(Component)]
pub(super) struct NeedIcon(pub String);

/// The size of a picture under "It takes", and of a thing flying in.
pub(super) const ICON: f32 = 30.0;
/// Seconds between things put in at once leaving the bottom.
const STAGGER: f32 = 0.12;
/// How far below the screen they start.
const BELOW: f32 = 40.0;

#[derive(Component)]
pub(super) struct Flier {
    item: String,
    step: String,
    t: f32,
    from: Option<Vec2>,
}

/// The glow behind a flier (its glint).
#[derive(Component)]
pub(super) struct FlierGlow;

/// A picture swelling in after something landed in it.
#[derive(Component)]
pub(super) struct Swell(f32);

/// Where a thing flying from `from` to `to` (screen points) is `t` seconds
/// in, how big it is, and how strong its glint: the forge's gathering seen
/// from the side (eased pull, a sway that dies as it closes, shrinking to size).
pub(super) fn flight(from: Vec2, to: Vec2, t: f32) -> (Vec2, f32, f32) {
    let k = (t / GATHER).clamp(0.0, 1.0);
    let pull = k * k * (3.0 - 2.0 * k);
    let sway = (t * 9.0).sin() * 14.0 * (1.0 - pull) * pull.min(0.5) * 2.0;
    (
        from.lerp(to, pull) + Vec2::new(sway, 0.0),
        1.0 + 0.35 * (1.0 - pull * pull),
        gather_strength(t),
    )
}

/// Start a flier for each thing put in.
pub(super) fn launch(
    mut commands: Commands,
    mut put: MessageReader<FlyIn>,
    theme: Res<Theme>,
    items: Res<Items>,
    art: Option<Res<ItemArt>>,
    mut sound: MessageWriter<PlaySound>,
) {
    let Some(art) = art else {
        put.clear();
        return;
    };
    let mut n = 0;
    for f in put.read() {
        let Some(id) = items.id(&f.item) else {
            continue;
        };
        commands
            .spawn((
                Flier {
                    item: f.item.clone(),
                    step: f.step.clone(),
                    t: -STAGGER * n as f32,
                    from: None,
                },
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(ICON),
                    height: Val::Px(ICON),
                    ..default()
                },
                UiTransform::default(),
                GlobalZIndex(26),
                Visibility::Hidden,
            ))
            .with_children(|c| {
                c.spawn((
                    FlierGlow,
                    theme
                        .image("slot_glow")
                        .with_color(Color::WHITE.with_alpha(0.0)),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(-7.0),
                        top: Val::Px(-7.0),
                        width: Val::Px(ICON + 14.0),
                        height: Val::Px(ICON + 14.0),
                        ..default()
                    },
                ));
                crate::inventory::ui::icon(c, &items, &art, Stack::new(id, 1), ICON, ICON, false);
            });
        n += 1;
    }
    if n > 0 {
        sound.write(PlaySound::new("craft_shimmer").gain(1.0));
    }
}

/// Fly them in, pop them, swell the pictures they land in.
#[allow(clippy::type_complexity)]
pub(super) fn fly(
    mut commands: Commands,
    time: Res<Time<Real>>,
    tree: Res<TreeUi>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut fliers: Query<(
        Entity,
        &mut Flier,
        &mut Node,
        &mut UiTransform,
        &mut Visibility,
        &Children,
    )>,
    mut glows: Query<&mut ImageNode, With<FlierGlow>>,
    needs: Query<(Entity, &NeedIcon, &UiGlobalTransform, &InheritedVisibility)>,
    frames: Query<(&NodeFrame, &UiGlobalTransform)>,
    mut swells: Query<(Entity, &mut Swell, &mut UiTransform), Without<Flier>>,
    mut sound: MessageWriter<PlaySound>,
) {
    let dt = time.delta_secs().min(0.1);
    if tree.open.is_none() {
        for (e, ..) in &fliers {
            commands.entity(e).despawn();
        }
    }
    let scale = window.scale_factor().max(0.01);
    for (e, mut f, mut node, mut tf, mut vis, kids) in &mut fliers {
        f.t += dt;
        if f.t < 0.0 {
            continue;
        }
        // Its picture under "It takes", else (the step got finished) the step on the tree.
        let need = needs
            .iter()
            .find(|(_, n, _, v)| n.0 == f.item && v.get())
            .map(|(e, _, g, _)| (Some(e), g.translation));
        let Some((target, at)) = need.or_else(|| {
            frames
                .iter()
                .find(|(n, _)| n.0 == f.step)
                .map(|(_, g)| (None, g.translation))
        }) else {
            commands.entity(e).despawn();
            continue;
        };
        let to = at / scale - Vec2::splat(ICON * 0.5);
        let from = *f
            .from
            .get_or_insert(Vec2::new(to.x, window.height() + BELOW));
        if f.t >= GATHER {
            commands.entity(e).despawn();
            sound.write(PlaySound::new("craft_pop").gain(1.0));
            if let Some(t) = target {
                commands.entity(t).insert(Swell(0.0));
            }
            continue;
        }
        let (p, s, glint) = flight(from, to, f.t);
        node.left = Val::Px(p.x);
        node.top = Val::Px(p.y);
        tf.scale = Vec2::splat(s);
        tf.rotation = Rot2::radians((f.t * 7.0).sin() * 0.25 * (1.0 - f.t / GATHER));
        *vis = Visibility::Inherited;
        for k in kids {
            if let Ok(mut img) = glows.get_mut(*k) {
                img.color = img.color.with_alpha(glint);
            }
        }
    }
    for (e, mut sw, mut tf) in &mut swells {
        sw.0 += dt;
        let k = (sw.0 / APPEAR).clamp(0.0, 1.0);
        tf.scale = Vec2::splat(ease_out_back(k).max(0.02));
        if k >= 1.0 {
            tf.scale = Vec2::ONE;
            commands.entity(e).remove::<Swell>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_leaves_from_below_and_lands_on_its_picture() {
        let (from, to) = (Vec2::new(400.0, 760.0), Vec2::new(400.0, 300.0));
        let (p, s, g) = flight(from, to, 0.0);
        assert!((p - from).length() < 1e-3 && (s - 1.35).abs() < 1e-3 && g == 0.0);
        let (p, s, _) = flight(from, to, GATHER);
        assert!((p - to).length() < 1e-3, "lands at {p}");
        assert!((s - 1.0).abs() < 1e-3);
        // Always rising, never past the picture.
        let ys: Vec<f32> = (0..=60)
            .map(|i| flight(from, to, i as f32 * GATHER / 60.0).0.y)
            .collect();
        assert!(ys.windows(2).all(|w| w[1] <= w[0] + 1e-3));
    }
}
