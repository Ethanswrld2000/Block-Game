//! Shelter: walls and a roof keep the weather off. Pure rules; `find_cover`
//! in `survival` looks at the blocks round you once a second.
//!
//! - Under a roof (a solid block somewhere over your head): half of the
//!   weather's chill is kept off.
//! - In a closed room (`voxel::room::find_room` from where you stand): all of
//!   the weather's chill, and most of the night's cold.
//!
//! Fire: torches warm a little close by; a campfire warms a lot and further.

/// How sheltered you are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cover {
    #[default]
    Open,
    Roof,
    Room,
}

/// Share of the weather's chill kept off.
pub fn weather_blocked(cover: Cover) -> f32 {
    match cover {
        Cover::Open => 0.0,
        Cover::Roof => 0.5,
        Cover::Room => 1.0,
    }
}

/// Share of the night's cold kept off.
pub fn night_blocked(cover: Cover) -> f32 {
    match cover {
        Cover::Open | Cover::Roof => 0.0,
        Cover::Room => 0.6,
    }
}

/// Warmth (°C) shelter gives back from the weather (`chill`, below zero when
/// cold) and the night (`night`, below zero at night). Heat is never kept off.
pub fn shelter_warmth(cover: Cover, chill: f32, night: f32) -> f32 {
    -chill.min(0.0) * weather_blocked(cover) - night.min(0.0) * night_blocked(cover)
}

/// Warmth of one campfire at `dist` blocks.
pub fn campfire_warmth(dist: f32) -> f32 {
    (CAMPFIRE_HEAT * (1.0 - dist / CAMPFIRE_REACH)).max(0.0)
}

/// A campfire's warmth right beside it, °C, and how far it reaches.
pub const CAMPFIRE_HEAT: f32 = 24.0;
pub const CAMPFIRE_REACH: f32 = 10.0;
/// Most warmth campfires can give all told.
pub const CAMPFIRE_MAX: f32 = 28.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walls_keep_the_cold_out() {
        // A -8 °C gale at night (-5 °C of night).
        assert_eq!(shelter_warmth(Cover::Open, -8.0, -5.0), 0.0);
        assert_eq!(shelter_warmth(Cover::Roof, -8.0, -5.0), 4.0);
        assert_eq!(shelter_warmth(Cover::Room, -8.0, -5.0), 8.0 + 3.0);
        // A hot day stays hot.
        assert_eq!(shelter_warmth(Cover::Room, 4.0, 2.0), 0.0);
    }

    #[test]
    fn campfires_warm_more_than_torches_and_fade_with_distance() {
        assert!(campfire_warmth(1.0) > 16.0);
        assert!(campfire_warmth(5.0) > campfire_warmth(8.0));
        assert_eq!(campfire_warmth(CAMPFIRE_REACH + 1.0), 0.0);
    }
}
