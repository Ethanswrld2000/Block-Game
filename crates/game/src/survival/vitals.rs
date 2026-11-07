//! Everything about the body that changes over time: health, food, air,
//! body warmth and carried weight. Plain Rust with no Bevy; the systems in
//! `survival` gather the surroundings into an `Env` and call `tick`.

use serde::{Deserialize, Serialize};

use super::food::FoodTier;
use super::health::{Health, Layer, MAX};

/// Comfortable range of the felt temperature, °C.
pub const COMFORT_LOW: f32 = 8.0;
pub const COMFORT_HIGH: f32 = 26.0;
/// Body warmth (0..100) outside this band hurts.
pub const COLD_BELOW: f32 = 30.0;
pub const HOT_ABOVE: f32 = 70.0;
/// What you can carry before you count as overloaded, kg.
pub const CAPACITY: f32 = 40.0;
/// Share of capacity that is the sweet spot: no bonus, no penalty.
pub const LIGHT_BELOW: f32 = 0.3;
pub const HEAVY_ABOVE: f32 = 0.6;
/// Satiety lost per real second: a full stomach lasts about a day and a half.
const HUNGER: f32 = 100.0 / (1.5 * 1200.0);
/// Below this satiety the body stops mending itself.
pub const PECKISH: f32 = 15.0;
/// Air meter levels where effects start.
pub const AIR_SLOW: f32 = 70.0;
pub const AIR_DIZZY: f32 = 45.0;
pub const AIR_CHOKE: f32 = 20.0;

/// The surroundings and what the body is doing this frame.
#[derive(Debug, Clone, Copy)]
pub struct Env {
    /// How breathable the air is, 0 (underwater) .. 1.
    pub air: f32,
    /// Felt temperature, °C.
    pub feels: f32,
    pub sprinting: bool,
    /// Carried weight, kg.
    pub carried: f32,
}

impl Default for Env {
    fn default() -> Self {
        Self { air: 1.0, feels: 16.0, sprinting: false, carried: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vitals {
    pub health: Health,
    /// 0..100, how fed you are.
    pub satiety: f32,
    /// 0..100, the air meter.
    pub air: f32,
    /// 0..100 with 50 comfortable.
    pub warmth: f32,
    /// A food buff: extra run speed (0.1 = +10%) and seconds left.
    pub buff_speed: f32,
    pub buff_left: f32,
}

impl Default for Vitals {
    fn default() -> Self {
        Self { health: Health::default(), satiety: 80.0, air: 100.0, warmth: 50.0, buff_speed: 0.0, buff_left: 0.0 }
    }
}

/// Load as a share of capacity.
pub fn load(carried: f32) -> f32 {
    carried / CAPACITY
}

pub fn is_heavy(carried: f32) -> bool {
    load(carried) > HEAVY_ABOVE
}

/// Movement speed multiplier from weight: up to +15% travelling light, down
/// to -40% at double the heavy line.
pub fn weight_speed(carried: f32) -> f32 {
    let l = load(carried);
    if l < LIGHT_BELOW {
        1.0 + 0.15 * (1.0 - l / LIGHT_BELOW)
    } else if l <= HEAVY_ABOVE {
        1.0
    } else {
        1.0 - 0.4 * ((l - HEAVY_ABOVE) / HEAVY_ABOVE).min(1.0)
    }
}

/// Fall damage multiplier from weight.
pub fn weight_fall(carried: f32) -> f32 {
    1.0 + (load(carried) - HEAVY_ABOVE).max(0.0) * 1.5
}

/// How breathable the air is at a height: thin high up, stale deep down.
pub fn air_at(y: f32) -> f32 {
    if y < -64.0 {
        (1.0 - (-64.0 - y) / 400.0).max(0.35)
    } else if y > 350.0 {
        (1.0 - (y - 350.0) / 500.0).max(0.45)
    } else {
        1.0
    }
}

/// Where body warmth settles for a felt temperature.
pub fn warmth_target(feels: f32) -> f32 {
    let w = if feels < COMFORT_LOW {
        50.0 - (COMFORT_LOW - feels) * 2.5
    } else if feels > COMFORT_HIGH {
        50.0 + (feels - COMFORT_HIGH) * 2.5
    } else {
        50.0
    };
    w.clamp(0.0, 100.0)
}

impl Vitals {
    /// Movement multiplier from food buffs, weight and lack of air.
    pub fn speed(&self, carried: f32) -> f32 {
        let buff = if self.buff_left > 0.0 { 1.0 + self.buff_speed } else { 1.0 };
        let air = if self.air < AIR_SLOW { 1.0 - 0.25 * ((AIR_SLOW - self.air) / AIR_SLOW).min(1.0) } else { 1.0 };
        weight_speed(carried) * buff * air
    }

    /// How dizzy lack of air makes you, 0..1 (camera sway and dark edges).
    pub fn dizzy(&self) -> f32 {
        ((AIR_DIZZY - self.air) / AIR_DIZZY).clamp(0.0, 1.0)
    }

    pub fn too_cold(&self) -> bool {
        self.warmth < COLD_BELOW
    }

    pub fn too_hot(&self) -> bool {
        self.warmth > HOT_ABOVE
    }

    /// Advance the body by `dt` seconds.
    pub fn tick(&mut self, env: &Env, dt: f32) {
        if self.health.is_dead() {
            return;
        }
        let h = &mut self.health;

        // Food.
        let hunger = if env.sprinting { HUNGER * 2.0 } else { HUNGER };
        self.satiety = (self.satiety - hunger * dt).max(0.0);
        if self.satiety <= 0.0 {
            h.hit(Layer::Fatigue, 0.4 * dt);
        }
        self.buff_left = (self.buff_left - dt).max(0.0);

        // Air: the meter settles toward what the air gives; underwater it runs out.
        let target = env.air.clamp(0.0, 1.0) * 100.0;
        let rate = if target < self.air { if env.air <= 0.0 { 6.0 } else { 1.5 } } else { 20.0 };
        self.air += (target - self.air).clamp(-rate * dt, rate * dt);
        if self.air < AIR_CHOKE {
            h.hit(Layer::Fatigue, (AIR_CHOKE - self.air) / AIR_CHOKE * 5.0 * dt);
        }

        // Warmth drifts toward what the weather and fire make it.
        let target = warmth_target(env.feels);
        self.warmth += (target - self.warmth) * (1.0 - (-dt / 90.0).exp());
        if self.warmth < COLD_BELOW {
            h.hit(Layer::Fatigue, (COLD_BELOW - self.warmth) / COLD_BELOW * 0.6 * dt);
        } else if self.warmth > HOT_ABOVE {
            let heavy = if is_heavy(env.carried) { 2.0 } else { 1.0 };
            h.hit(Layer::Fatigue, (self.warmth - HOT_ABOVE) / 30.0 * 1.0 * heavy * dt);
        }

        // Running wears you out, but never into the mortal half on its own.
        let suffering = self.satiety <= 0.0 || self.warmth < COLD_BELOW || self.warmth > HOT_ABOVE || self.air < AIR_DIZZY;
        if env.sprinting {
            let f = h.get(Layer::Fatigue);
            if f > 55.0 {
                h.hit(Layer::Fatigue, (0.8 * dt).min(f - 55.0));
            }
        } else if !suffering {
            h.heal(Layer::Fatigue, 0.4 * dt);
        }

        // Open wounds.
        h.bleed(dt);

        // Bruises mend on their own when nothing deeper is wrong.
        if self.satiety > PECKISH && !suffering {
            h.heal(Layer::Damage, 0.5 * dt);
        }
    }

    /// Eat something of a food tier.
    pub fn eat(&mut self, tier: &FoodTier, satiety: f32) {
        if self.health.is_dead() {
            return;
        }
        self.satiety = (self.satiety + satiety).min(100.0);
        // Deepest first, so treating a mortal layer lifts the cap for the rest.
        let mut heals: Vec<(Layer, f32)> = tier.heals.iter().map(|(l, a)| (*l, *a)).collect();
        heals.sort_by_key(|h| std::cmp::Reverse(h.0));
        for (layer, amount) in heals {
            if tier.ignore_cap {
                self.health.heal_any(layer, amount);
            } else {
                self.health.heal(layer, amount);
            }
        }
        if tier.ignore_cap {
            self.health.bleeding = 0.0;
        }
        if let Some(b) = tier.buff {
            self.buff_speed = self.buff_speed.max(b.speed);
            self.buff_left = self.buff_left.max(b.seconds);
        }
    }

    /// A night's sleep: rested, wounds a little better, and hungrier.
    /// Damage comes back in full only if no layer is mortal.
    pub fn sleep(&mut self) {
        let h = &mut self.health;
        h.heal(Layer::Injuries, 15.0);
        h.heal(Layer::Blood, 10.0);
        h.heal(Layer::Fatigue, MAX);
        h.heal(Layer::Damage, MAX);
        h.bleeding = 0.0;
        self.satiety = (self.satiety - 15.0).max(0.0);
        self.air = 100.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survival::food::FoodTiers;

    fn tiers() -> FoodTiers {
        FoodTiers::from_ron(include_str!("../../../../assets/data/food_tiers.ron")).unwrap()
    }

    fn run(v: &mut Vitals, env: Env, seconds: f32) {
        for _ in 0..(seconds * 10.0) as usize {
            v.tick(&env, 0.1);
        }
    }

    #[test]
    fn freezing_drains_fatigue() {
        let mut v = Vitals::default();
        run(&mut v, Env { feels: -15.0, ..Default::default() }, 150.0);
        assert!(v.too_cold(), "warmth {}", v.warmth);
        let h = v.health;
        assert!(h.get(Layer::Fatigue) < 90.0, "{:?}", h.bars);
        assert_eq!(h.get(Layer::Blood), MAX);
        assert_eq!(h.get(Layer::Injuries), MAX);
    }

    #[test]
    fn comfortable_air_does_nothing() {
        let mut v = Vitals::default();
        run(&mut v, Env::default(), 120.0);
        assert_eq!(v.health.bars, [MAX; 4]);
        assert!((v.warmth - 50.0).abs() < 0.01);
    }

    #[test]
    fn starving_drains_fatigue() {
        let mut v = Vitals { satiety: 0.0, ..Default::default() };
        run(&mut v, Env::default(), 60.0);
        assert!(v.health.get(Layer::Fatigue) < 80.0, "{:?}", v.health.bars);
        assert_eq!(v.health.get(Layer::Blood), MAX);
    }

    #[test]
    fn drowning_runs_out_of_air_then_hurts() {
        let mut v = Vitals::default();
        run(&mut v, Env { air: 0.0, ..Default::default() }, 10.0);
        assert!(v.air < 50.0 && v.health.get(Layer::Fatigue) == MAX);
        run(&mut v, Env { air: 0.0, ..Default::default() }, 20.0);
        assert!(v.health.get(Layer::Fatigue) < MAX);
        // Surfacing refills it quickly.
        run(&mut v, Env::default(), 6.0);
        assert!(v.air > 99.0);
    }

    #[test]
    fn sprinting_tires_but_not_into_the_mortal_half() {
        let mut v = Vitals::default();
        run(&mut v, Env { sprinting: true, ..Default::default() }, 200.0);
        let f = v.health.get(Layer::Fatigue);
        assert!((54.9..56.0).contains(&f), "{f}");
        assert_eq!(v.health.mortal(), None);
    }

    #[test]
    fn weight_bonus_and_penalty() {
        assert!((weight_speed(0.0) - 1.15).abs() < 1e-5);
        assert_eq!(weight_speed(CAPACITY * 0.45), 1.0);
        assert!((weight_speed(CAPACITY * 1.2) - 0.6).abs() < 1e-5);
        assert_eq!(weight_fall(5.0), 1.0);
        assert!(weight_fall(CAPACITY) > 1.5);
    }

    #[test]
    fn food_tiers_heal_by_the_rules() {
        let t = tiers();
        // Tier 1: buff only.
        let mut v = Vitals::default();
        v.health.hit(Layer::Damage, 50.0);
        v.eat(t.get(1).unwrap(), 5.0);
        assert_eq!(v.health.get(Layer::Damage), 50.0);
        assert!(v.buff_left > 0.0 && v.speed(15.0) > 1.05);
        // Tier 2 heals Damage but cannot touch a mortal layer.
        let mut v = Vitals::default();
        v.health.hit(Layer::Blood, 60.0);
        v.eat(t.get(2).unwrap(), 20.0);
        assert_eq!(v.health.get(Layer::Damage), 0.0);
        // Tier 3 treats some Blood; tier 4 brings everything back.
        v.eat(t.get(3).unwrap(), 30.0);
        assert!(v.health.get(Layer::Blood) > 40.0);
        v.eat(t.get(4).unwrap(), 50.0);
        assert_eq!(v.health.bars, [MAX; 4]);
    }

    #[test]
    fn sleep_restores_damage_only_without_a_mortal_layer() {
        let mut v = Vitals::default();
        v.health.hit(Layer::Damage, 80.0);
        v.sleep();
        assert_eq!(v.health.bars, [MAX; 4]);

        let mut v = Vitals::default();
        v.health.hit(Layer::Injuries, 60.0); // Injuries 40: mortal
        v.sleep();
        // Injuries 55 now, so the cap lifts one layer: Blood (emptied by the
        // mortal rule) is now the mortal layer and starts mending.
        assert_eq!(v.health.get(Layer::Injuries), 55.0);
        assert_eq!(v.health.mortal(), Some(Layer::Blood));
        assert_eq!(v.health.get(Layer::Blood), 10.0);
        assert_eq!(v.health.get(Layer::Damage), 0.0);

        let mut v = Vitals::default();
        v.health.hit(Layer::Injuries, 80.0); // Injuries 20: still mortal after sleep
        v.sleep();
        assert_eq!(v.health.get(Layer::Injuries), 35.0);
        assert_eq!(v.health.get(Layer::Damage), 0.0);
        assert_eq!(v.health.get(Layer::Fatigue), 0.0);
    }

    #[test]
    fn air_is_thin_up_high_and_deep_down() {
        assert_eq!(air_at(64.0), 1.0);
        assert!(air_at(600.0) < 0.6);
        assert!(air_at(-300.0) < 0.5);
    }
}
