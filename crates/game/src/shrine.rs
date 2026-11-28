//! Sky shrines: a carved stone on the biggest sky islands (`worldgen::islands::SHRINE_RADIUS`).
//! Right click one and it gives a skill point and XP, once per shrine (remembered in
//! `Progress.found` as `shrine:x,y,z`, so it saves with progress.ron).

use bevy::prelude::*;
use voxel::BlockPos;

use crate::progress::{GainXp, Progress};
use crate::sound::PlaySound;
use crate::station::particles::{Burst, BurstKind};
use crate::survival::Toast;

/// XP for a shrine's gift.
const SHRINE_XP: f32 = 80.0;

/// Right click on a sky shrine.
#[derive(Message, Debug, Clone, Copy)]
pub struct UseShrine(pub BlockPos);

pub struct ShrinePlugin;

impl Plugin for ShrinePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<UseShrine>().add_systems(crate::menu::GameUpdate, use_shrine);
    }
}

/// The key a shrine's gift is remembered by.
pub fn key(p: BlockPos) -> String {
    format!("shrine:{},{},{}", p.x, p.y, p.z)
}

fn use_shrine(
    mut used: MessageReader<UseShrine>,
    mut progress: ResMut<Progress>,
    mut gain: MessageWriter<GainXp>,
    mut toast: MessageWriter<Toast>,
    mut sound: MessageWriter<PlaySound>,
    mut bursts: MessageWriter<Burst>,
) {
    for UseShrine(p) in used.read() {
        let at = Vec3::new(p.x as f32 + 0.5, p.y as f32 + 1.2, p.z as f32 + 0.5);
        if !progress.found.insert(key(*p)) {
            toast.write(Toast("This shrine has given its gift".into()));
            continue;
        }
        progress.learned.points += 1;
        gain.write(GainXp { xp: SHRINE_XP, why: "Sky shrine".into() });
        toast.write(Toast("The shrine's light settles in you: +1 skill point".into()));
        sound.write(PlaySound::new("craft_shimmer").speed(0.7));
        bursts.write(Burst::new(at, BurstKind::Sparkle, 30));
        bursts.write(Burst::new(at, BurstKind::Pop, 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shrine_has_its_own_key() {
        assert_eq!(key(BlockPos::new(1, -2, 3)), "shrine:1,-2,3");
        assert_ne!(key(BlockPos::new(1, 2, 3)), key(BlockPos::new(1, 3, 2)));
    }
}
