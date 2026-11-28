//! Exposure stages from `assets/data/exposure.ron`: how cold, heat and thin air
//! wear you down. Plain Rust with no Bevy.
//!
//! Every stage drains Fatigue (stamina) but never past the mortal line; the
//! worst stages also deal real damage, which can kill.

use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Stage {
    pub name: String,
    pub at: f32,
    #[serde(default)]
    pub shake: f32,
    #[serde(default)]
    pub fatigue: f32,
    #[serde(default)]
    pub damage: f32,
    #[serde(default)]
    pub slow: f32,
    #[serde(default)]
    pub dizzy: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Cold,
    Heat,
    Air,
}

#[derive(Deserialize, Debug, Clone, bevy::prelude::Resource)]
pub struct Exposure {
    pub cold: Vec<Stage>,
    pub heat: Vec<Stage>,
    pub air: Vec<Stage>,
    /// Seconds of held breath under water.
    pub breath: f32,
    /// Time constant, seconds, of the felt temperature following the surroundings.
    #[serde(default = "follow")]
    pub follow: f32,
}

fn follow() -> f32 {
    12.0
}

/// A stage in effect and how far past its threshold you are.
#[derive(Debug, Clone, Copy)]
pub struct Active<'a> {
    pub stage: &'a Stage,
    /// 0 for the mildest stage, counting up.
    pub level: usize,
    pub past: f32,
}

impl Active<'_> {
    /// Drain and damage grow the further past the threshold you are.
    pub fn ramp(&self) -> f32 {
        (1.0 + self.past * 0.1).min(3.0)
    }
}

impl Exposure {
    pub fn from_ron(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| format!("exposure.ron: {e}"))
    }

    pub fn stages(&self, scale: Scale) -> &[Stage] {
        match scale {
            Scale::Cold => &self.cold,
            Scale::Heat => &self.heat,
            Scale::Air => &self.air,
        }
    }

    /// The worst stage a value reaches on a scale, if any.
    pub fn stage(&self, scale: Scale, value: f32) -> Option<Active<'_>> {
        let past = |s: &Stage| if scale == Scale::Heat { value - s.at } else { s.at - value };
        self.stages(scale)
            .iter()
            .enumerate()
            .rev()
            .find(|(_, s)| past(s) > 0.0)
            .map(|(level, stage)| Active { stage, level, past: past(stage) })
    }

    /// The stage felt temperature puts you in: cold or heat.
    pub fn temperature(&self, feels: f32) -> Option<(Scale, Active<'_>)> {
        self.stage(Scale::Cold, feels).map(|a| (Scale::Cold, a)).or_else(|| self.stage(Scale::Heat, feels).map(|a| (Scale::Heat, a)))
    }

    /// Where the comfortable band ends on each side, °C.
    pub fn comfort(&self) -> (f32, f32) {
        (self.cold.first().map_or(f32::MIN, |s| s.at), self.heat.first().map_or(f32::MAX, |s| s.at))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn rules() -> Exposure {
        Exposure::from_ron(include_str!("../../../../assets/data/exposure.ron")).unwrap()
    }

    #[test]
    fn stages_get_worse_in_order() {
        let e = rules();
        for scale in [Scale::Cold, Scale::Heat, Scale::Air] {
            let s = e.stages(scale);
            assert!(!s.is_empty());
            for w in s.windows(2) {
                assert!(w[1].shake >= w[0].shake, "{scale:?}");
                if scale == Scale::Heat {
                    assert!(w[1].at > w[0].at);
                } else {
                    assert!(w[1].at < w[0].at);
                }
            }
            // Only the last stage hurts for real.
            assert!(s[..s.len() - 1].iter().all(|x| x.damage == 0.0) && s.last().unwrap().damage > 0.0, "{scale:?}");
        }
    }

    #[test]
    fn picks_the_worst_stage_reached() {
        let e = rules();
        assert!(e.temperature(16.0).is_none());
        let (s, a) = e.temperature(0.0).unwrap();
        assert_eq!((s, a.level), (Scale::Cold, 0));
        let (s, a) = e.temperature(-30.0).unwrap();
        assert_eq!((s, a.level), (Scale::Cold, 2));
        let (s, a) = e.temperature(45.0).unwrap();
        assert_eq!((s, a.level), (Scale::Heat, 2));
        assert!(e.stage(Scale::Air, 100.0).is_none());
        assert_eq!(e.stage(Scale::Air, 50.0).unwrap().level, 1);
    }
}
