//! An NPC handing you something: whenever one gives you an item (a
//! commission, a trade, a cutscene's `Give`), it holds it out in its right
//! hand (`Anim::Give`) and lets go once the arm is out, as you take it.

use bevy::prelude::*;

use super::dialogue::Anim;
use super::talk::Talk;
use super::Npc;
use crate::hands::Held;
use crate::items::ItemId;
use crate::npc::body::Rig;

pub struct HandoverPlugin;

impl Plugin for HandoverPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<HandOver>().add_systems(crate::menu::GameUpdate, (start, let_go).chain());
    }
}

/// `npc` gives you `item`: play the hand-over.
#[derive(Message, Debug, Clone, Copy)]
pub struct HandOver {
    pub npc: Entity,
    pub item: ItemId,
}

/// The item in an NPC's hand while it hands it over, and seconds since it started.
#[derive(Component)]
struct Handing {
    held: Entity,
    t: f32,
}

/// Size of the item in the hand (as foes hold their weapons).
const HELD_SCALE: f32 = 1.6;
/// Share of `Anim::Give` after which the hand is empty (the arm is all the way out).
const LET_GO: f32 = 0.6;

fn start(mut commands: Commands, mut msgs: MessageReader<HandOver>, mut talk: ResMut<Talk>, mut npcs: Query<(&mut Npc, &Rig, Option<&Handing>)>, mut seq: Local<u32>) {
    for m in msgs.read() {
        let Ok((mut npc, rig, before)) = npcs.get_mut(m.npc) else { continue };
        if let Some(h) = before {
            commands.entity(h.held).despawn();
        }
        let held = commands.spawn((Held::holding(m.item), Transform::from_scale(Vec3::splat(HELD_SCALE)), Visibility::default(), ChildOf(rig.hands[1]))).id();
        commands.entity(m.npc).insert(Handing { held, t: 0.0 });
        // In a talk the talk drives its animations; otherwise (a cutscene) set it here.
        if talk.with() == Some(m.npc) {
            talk.play(Anim::Give);
        } else {
            *seq = seq.wrapping_add(1);
            npc.act = Some((Anim::Give, u32::MAX / 2 + *seq));
        }
    }
}

/// Real time, since a talk pauses the game.
fn let_go(mut commands: Commands, real: Res<Time<Real>>, mut handing: Query<(Entity, &mut Handing)>) {
    for (e, mut h) in &mut handing {
        h.t += real.delta_secs();
        if h.t >= Anim::Give.length() * LET_GO {
            commands.entity(h.held).despawn();
            commands.entity(e).remove::<Handing>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lets_go_with_the_arm_out() {
        // The arm is fully out (past its ease-in) before the hand opens, and still out then.
        let at = Anim::Give.length() * LET_GO;
        let reach = crate::npc::motion::act(Anim::Give, at, 0.76).shoulder[1];
        assert!(reach > 1.3, "arm out when letting go ({reach})");
    }
}
