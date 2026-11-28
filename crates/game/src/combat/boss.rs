//! Bosses: foes.ron kinds with a `boss` block. On top of the usual fight
//! (`ai`) a boss has phases (a new one at each health share in `phases`, 66%
//! and 33% as a rule: it roars and its attacks come faster) and named attacks
//! with a warning before each lands:
//!
//! - Slam: it stops, dust rings the ground round it, then everything on the
//!   ground within the radius is hit. Get out of the ring.
//! - Volley: a fan of bolts at you (a raised guard stops them).
//! - Charge: it plants its feet, sparks fly, then it rushes at you for a second.
//! - Summon: it calls foes to its side.
//!
//! A bar with its name, health and the attack coming shows at the top of the
//! screen while a boss that has seen you is near.

use bevy::prelude::*;

use super::data::{AttackKind, BossDef};
use super::{Foe, FoeBook, Shoot, SpawnFoe, flat};
use crate::player::{Damage, Player};
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};
use crate::survival::Toast;

/// How long a charge runs, seconds.
const CHARGE_FOR: f32 = 1.0;
/// Attacks come this much faster per phase.
const PHASE_HASTE: f32 = 0.3;
/// The most foes a boss keeps at its side.
const MAX_ADDS: usize = 6;
/// The bar shows for a boss within this many blocks.
const BAR_RANGE: f32 = 40.0;

/// A boss's own state.
#[derive(Component, Default, Debug)]
pub struct BossState {
    pub phase: u8,
    /// Seconds before each attack can come again.
    cool: Vec<f32>,
    /// The attack under way: which, seconds in, where it aims.
    pub doing: Option<(usize, f32, Vec3)>,
    hit: bool,
}

/// The phase for a health share (pure): one more for every threshold passed.
pub fn phase_of(share: f32, phases: &[f32]) -> u8 {
    phases.iter().filter(|p| share <= **p).count() as u8
}

/// The attack to use now, if any is ready (pure): the first in the list
/// allowed in this phase whose wait is over.
pub fn pick(def: &BossDef, phase: u8, cool: &[f32]) -> Option<usize> {
    def.attacks
        .iter()
        .enumerate()
        .find(|(i, a)| a.phase <= phase && cool.get(*i).is_none_or(|c| *c <= 0.0))
        .map(|(i, _)| i)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn attacks(
    mut commands: Commands,
    time: Res<Time>,
    book: Res<FoeBook>,
    player: Res<Player>,
    mut foes: Query<(Entity, &Transform, &mut Foe, Option<&mut BossState>)>,
    (mut hits, mut shots, mut spawn): (
        MessageWriter<Damage>,
        MessageWriter<Shoot>,
        MessageWriter<SpawnFoe>,
    ),
    (mut sound, mut bursts, mut toast): (
        MessageWriter<PlaySound>,
        MessageWriter<Burst>,
        MessageWriter<Toast>,
    ),
    mut ring: Local<f32>,
) {
    let dt = time.delta_secs().min(0.05);
    if dt <= 0.0 {
        return;
    }
    *ring -= dt;
    let show = *ring <= 0.0;
    if show {
        *ring = 0.12;
    }
    let adds: Vec<u32> = foes
        .iter()
        .filter(|(_, _, f, _)| book.kinds[f.kind].boss.is_none())
        .map(|(_, _, f, _)| f.tag)
        .collect();
    for (e, tf, mut foe, state) in &mut foes {
        let k = &book.kinds[foe.kind];
        let Some(def) = &k.boss else { continue };
        let Some(mut st) = state else {
            commands.entity(e).insert(BossState {
                cool: def.attacks.iter().map(|a| a.every * 0.5).collect(),
                ..default()
            });
            continue;
        };
        let pos = tf.translation;
        // A new phase: a roar, a burst, and everything comes quicker.
        let phase = phase_of(foe.health / foe.max_health.max(1.0), &def.phases);
        if phase > st.phase {
            st.phase = phase;
            sound.write(PlaySound::new("hit").at(pos).speed(0.3).gain(1.6));
            bursts.write(Burst::new(pos + Vec3::Y * 1.5, BurstKind::Smoke, 20));
            toast.write(Toast(format!("{} grows furious", k.title)));
        }
        if !foe.brain.aware || flat(player.pos - pos).length() > BAR_RANGE {
            st.doing = None;
            continue;
        }
        let haste = 1.0 + PHASE_HASTE * st.phase as f32;
        if st.doing.is_none() {
            for c in st.cool.iter_mut() {
                *c -= dt * haste;
            }
            if let Some(i) = pick(def, st.phase, &st.cool) {
                st.doing = Some((i, 0.0, player.pos));
                st.hit = false;
                st.cool[i] = def.attacks[i].every;
                sound.write(PlaySound::new("clash").at(pos).speed(0.5));
            }
            continue;
        }
        let Some((i, t, aim)) = st.doing else {
            continue;
        };
        let a = &def.attacks[i];
        let t = t + dt;
        let mut done = false;
        match &a.kind {
            AttackKind::Slam { radius, damage } => {
                foe.want = Vec3::ZERO;
                if t < a.wind {
                    if show {
                        for n in 0..10 {
                            let ang = n as f32 / 10.0 * std::f32::consts::TAU + t;
                            bursts.write(Burst::new(
                                pos + Vec3::new(ang.cos() * radius, 0.2, ang.sin() * radius),
                                BurstKind::Smoke,
                                1,
                            ));
                        }
                    }
                } else {
                    let to = player.pos - pos;
                    if flat(to).length() < *radius && to.y.abs() < 1.5 {
                        hits.write(Damage {
                            amount: damage * foe.boost,
                            cause: crate::player::DamageCause::Blunt,
                        });
                    }
                    sound.write(PlaySound::new("hit").at(pos).speed(0.4).gain(1.5));
                    sound.write(PlaySound::new("dig_hard").at(pos).speed(0.5));
                    bursts.write(Burst::new(pos, BurstKind::Chips, 24));
                    done = true;
                }
            }
            AttackKind::Volley { count, damage } => {
                foe.want = Vec3::ZERO;
                if t >= a.wind {
                    let from = pos + Vec3::Y * 1.6;
                    let dir = (player.pos + Vec3::Y - from).normalize_or_zero();
                    let n = (*count).max(1);
                    for j in 0..n {
                        let off = if n > 1 {
                            (j as f32 / (n - 1) as f32 - 0.5) * 0.9
                        } else {
                            0.0
                        };
                        let vel = Quat::from_rotation_y(off) * dir * 16.0;
                        shots.write(Shoot {
                            from,
                            vel,
                            damage: damage * foe.boost,
                            cause: k.cause,
                            color: (0.7, 0.9, 0.4),
                        });
                    }
                    sound.write(PlaySound::new("knife").at(from).speed(0.6));
                    done = true;
                } else if show {
                    bursts.write(Burst::new(pos + Vec3::Y * 1.8, BurstKind::Sparkle, 3));
                }
            }
            AttackKind::Charge { speed, damage } => {
                if t < a.wind {
                    foe.want = Vec3::ZERO;
                    if show {
                        bursts.write(Burst::new(pos + Vec3::Y * 0.3, BurstKind::Sparks, 4));
                    }
                } else {
                    let dir = flat(aim - pos).normalize_or_zero();
                    foe.want = dir * *speed;
                    foe.yaw = crate::npc::yaw_of(dir);
                    let to = player.pos - pos;
                    if !st.hit && flat(to).length() < 1.5 && to.y.abs() < 1.6 {
                        st.hit = true;
                        hits.write(Damage {
                            amount: damage * foe.boost,
                            cause: crate::player::DamageCause::Blunt,
                        });
                        sound.write(PlaySound::new("hit").at(player.pos).speed(0.6));
                    }
                    done = t >= a.wind + CHARGE_FOR || foe.bumped;
                }
            }
            AttackKind::Summon { kind, count } => {
                foe.want = Vec3::ZERO;
                if t >= a.wind {
                    let tag = crate::delve::tag_room(foe.tag)
                        .map_or(0, |(r, w)| crate::delve::tag(r, w, false));
                    let near = adds.iter().filter(|t| **t == tag).count();
                    if let Some(add) = book.kinds.iter().position(|f| &f.name == kind) {
                        for j in 0..(*count as usize).min(MAX_ADDS.saturating_sub(near)) {
                            let ang = j as f32 * 2.1 + t;
                            let at = pos + Vec3::new(ang.cos() * 2.5, 0.5, ang.sin() * 2.5);
                            spawn.write(SpawnFoe {
                                tag,
                                boost: foe.boost,
                                ..SpawnFoe::new(add, at)
                            });
                            bursts.write(Burst::new(at, BurstKind::Smoke, 8));
                        }
                    }
                    done = true;
                } else if show {
                    bursts.write(Burst::new(pos + Vec3::Y * 2.2, BurstKind::Wisp, 2));
                }
            }
        }
        st.doing = if done { None } else { Some((i, t, aim)) };
    }
}

/// The boss bar.
#[derive(Component)]
pub(super) struct BossBar;
#[derive(Component)]
pub(super) struct BossName;
#[derive(Component)]
pub(super) struct BossFill;

pub(super) fn spawn_bar(mut commands: Commands) {
    commands
        .spawn((
            BossBar,
            Visibility::Hidden,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(150.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|c| {
            c.spawn((
                BossName,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(crate::theme::sizes::HEADING),
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.86, 0.7)),
            ));
            c.spawn((
                Node {
                    width: Val::Px(420.0),
                    height: Val::Px(14.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.08, 0.04, 0.04, 0.85)),
                BorderColor::all(Color::srgb(0.35, 0.25, 0.2)),
            ))
            .with_child((
                BossFill,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.75, 0.12, 0.1)),
            ));
        });
}

#[allow(clippy::type_complexity)]
pub(super) fn bar(
    book: Res<FoeBook>,
    player: Res<Player>,
    foes: Query<(&Transform, &Foe, &BossState)>,
    mut ui: Query<&mut Visibility, With<BossBar>>,
    mut name: Query<&mut Text, With<BossName>>,
    mut fill: Query<&mut Node, With<BossFill>>,
) {
    let near = foes
        .iter()
        .filter(|(tf, f, _)| {
            f.brain.aware
                && f.health > 0.0
                && flat(tf.translation - player.pos).length() < BAR_RANGE
        })
        .min_by(|a, b| {
            a.0.translation
                .distance(player.pos)
                .total_cmp(&b.0.translation.distance(player.pos))
        });
    for mut v in &mut ui {
        v.set_if_neq(if near.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let Some((_, f, st)) = near else { return };
    let k = &book.kinds[f.kind];
    let coming = st
        .doing
        .and_then(|(i, _, _)| k.boss.as_ref().map(|b| b.attacks[i].name.clone()));
    let line = match coming {
        Some(a) => format!("{}  .  {a}!", k.title),
        None => k.title.clone(),
    };
    for mut t in &mut name {
        if t.0 != line {
            t.0 = line.clone();
        }
    }
    let share = (f.health / f.max_health.max(1.0)).clamp(0.0, 1.0);
    for mut n in &mut fill {
        let w = Val::Percent(share * 100.0);
        if n.width != w {
            n.width = w;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_turn_at_the_thresholds() {
        let p = [0.66, 0.33];
        assert_eq!(phase_of(1.0, &p), 0);
        assert_eq!(phase_of(0.66, &p), 1);
        assert_eq!(phase_of(0.5, &p), 1);
        assert_eq!(phase_of(0.2, &p), 2);
    }

    #[test]
    fn every_boss_has_phases_and_named_attacks() {
        let kinds =
            super::super::data::load(include_str!("../../../../assets/data/foes.ron")).unwrap();
        let bosses: Vec<_> = kinds
            .iter()
            .filter_map(|k| k.boss.as_ref().map(|b| (k, b)))
            .collect();
        assert!(bosses.len() >= 8, "every dungeon boss and warden is phased");
        for (k, b) in bosses {
            assert_eq!(b.phases, vec![0.66, 0.33], "{}", k.name);
            assert!(
                (3..=5).contains(&b.attacks.len()),
                "{}: 3-5 attacks",
                k.name
            );
            assert!(
                b.attacks.iter().any(|a| a.phase == 0),
                "{}: something from the start",
                k.name
            );
            assert!(
                b.attacks
                    .iter()
                    .all(|a| a.phase <= 2 && a.wind > 0.2 && a.every > a.wind),
                "{}",
                k.name
            );
            for a in &b.attacks {
                if let AttackKind::Summon { kind, .. } = &a.kind {
                    assert!(
                        kinds.iter().any(|f| &f.name == kind && f.boss.is_none()),
                        "{}: summons `{kind}`",
                        k.name
                    );
                }
            }
            assert!(k.looks.scale >= 1.1, "{}: bigger than a person", k.name);
            // Picks only what its phase allows.
            let cool = vec![0.0; b.attacks.len()];
            let first = pick(b, 0, &cool).unwrap();
            assert_eq!(b.attacks[first].phase, 0);
        }
    }
}
