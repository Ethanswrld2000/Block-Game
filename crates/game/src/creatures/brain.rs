//! Small pure rules for how creatures decide, kept apart from the systems so
//! they can be tested (and later run on a server).

use bevy::math::Vec3;

/// Where a villager wants to be at an hour of the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// 21:00-6:00: indoors, asleep.
    Home,
    /// 6:00-8:00 and 18:00-21:00: gathered at the plaza.
    Plaza,
    /// 8:00-18:00: about the village.
    Wander,
}

pub fn plan(hour: f32) -> Plan {
    if !(6.0..21.0).contains(&hour) {
        Plan::Home
    } else if hour < 8.0 || hour >= 18.0 {
        Plan::Plaza
    } else {
        Plan::Wander
    }
}

/// When spirits walk.
pub fn spirit_hours(hour: f32) -> bool {
    !(5.5..19.5).contains(&hour)
}

/// How close a light keeps a spirit back.
pub const TORCH_FEAR: f32 = 6.0;

/// Away from the nearest light that is too close, if any (horizontal, unit length).
pub fn flee_light(pos: Vec3, lights: impl IntoIterator<Item = Vec3>) -> Option<Vec3> {
    let mut best: Option<(f32, Vec3)> = None;
    for l in lights {
        let d = pos - l;
        let dist = d.length();
        if dist < TORCH_FEAR && best.is_none_or(|(b, _)| dist < b) {
            best = Some((dist, d));
        }
    }
    best.map(|(_, d)| {
        let flat = Vec3::new(d.x, 0.0, d.z);
        if flat.length_squared() < 1e-4 { Vec3::X } else { flat.normalize() }
    })
}

/// A tiny random number generator (xorshift), so creatures don't need a crate.
#[derive(Debug, Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// 0..1
    pub fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }

    pub fn pick<'a, T>(&mut self, list: &'a [T]) -> Option<&'a T> {
        (!list.is_empty()).then(|| &list[(self.next() % list.len() as u64) as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn villager_day() {
        assert_eq!(plan(2.0), Plan::Home);
        assert_eq!(plan(22.0), Plan::Home);
        assert_eq!(plan(6.5), Plan::Plaza);
        assert_eq!(plan(12.0), Plan::Wander);
        assert_eq!(plan(19.0), Plan::Plaza);
    }

    #[test]
    fn spirits_keep_away_from_torches() {
        let p = Vec3::new(10.0, 64.0, 10.0);
        assert!(flee_light(p, [Vec3::new(40.0, 64.0, 40.0)]).is_none());
        let away = flee_light(p, [Vec3::new(8.0, 64.0, 10.0)]).unwrap();
        assert!(away.x > 0.9, "{away}");
        assert!(spirit_hours(23.0) && spirit_hours(3.0) && !spirit_hours(12.0));
    }

    #[test]
    fn rng_spreads() {
        let mut r = Rng(12345);
        let xs: Vec<f32> = (0..1000).map(|_| r.unit()).collect();
        let mean = xs.iter().sum::<f32>() / 1000.0;
        assert!((0.4..0.6).contains(&mean) && xs.iter().all(|x| (0.0..1.0).contains(x)));
    }
}
