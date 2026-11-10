//! Everything about the body that changes over time: health, food, the air
//! around you, felt temperature and carried weight. Plain Rust with no Bevy; the systems in
//! `survival` gather the surroundings into an `Env` and call `tick`.

use serde::{Deserialize, Serialize};

use super::food::FoodTier;
use super::exposure::{Exposure, Scale};
use super::health::{Health, Layer, MAX, MORTAL_AT};

/// Comfortable range of the felt temperature, °C. The exposure stages in
/// `exposure.ron` start here; these are for clothes and skills.
pub const COMFORT_LOW: f32 = 8.0;
pub const COMFORT_HIGH: f32 = 26.0;
/// What you can carry before you count as overloaded, kg.
pub const CAPACITY: f32 = 40.0;
/// Share of capacity that is the sweet spot: no bonus, no penalty.
pub const LIGHT_BELOW: f32 = 0.3;
pub const HEAVY_ABOVE: f32 = 0.6;
/// Satiety lost per real second: a full stomach lasts about a day and a half.
const HUNGER: f32 = 100.0 / (1.5 * 1200.0);
/// Below this satiety the body stops mending itself.
pub const PECKISH: f32 = 15.0;
/// Fatigue drain from exposure stops at the mortal line; only damage goes deeper.
const FATIGUE_FLOOR: f32 = MAX * MORTAL_AT;

/// The surroundings and what the body is doing this frame.
#[derive(Debug, Clone, Copy)]
pub struct Env {
    /// Oxygen in the air where you are, 0..1 of normal (0 underwater).
    pub air: f32,
    /// The same air as your body copes with it (skills help with thin air).
    pub breathe: f32,
    pub underwater: bool,
    /// Felt temperature, °C.
    pub feels: f32,
    pub sprinting: bool,
    /// Carried weight, kg.
    pub carried: f32,
}

impl Default for Env {
    fn default() -> Self {
        Self { air: 1.0, breathe: 1.0, underwater: false, feels: 16.0, sprinting: false, carried: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vitals {
    pub health: Health,
    /// 0..100, how fed you are.
    pub satiety: f32,
    /// Oxygen in the air where you are, % of normal (the air meter).
    pub air: f32,
    /// Oxygen your body is getting, %: the air, eased by skills, or a held
    /// breath under water. Exposure stages go by this.
    #[serde(default = "full")]
    pub oxygen: f32,
    /// Seconds of breath left under water.
    #[serde(default = "full_breath")]
    pub breath: f32,
    /// Felt temperature, °C: follows the surroundings (`exposure.ron` `follow`).
    #[serde(default = "mild")]
    pub feels: f32,
    /// A food buff: extra run speed (0.1 = +10%) and seconds left.
    pub buff_speed: f32,
    pub buff_left: f32,
}

impl Default for Vitals {
    fn default() -> Self {
        Self { health: Health::default(), satiety: 80.0, air: 100.0, oxygen: 100.0, breath: BREATH, feels: 16.0, buff_speed: 0.0, buff_left: 0.0 }
    }
}

/// Default breath before `exposure.ron` is read (and for old saves).
const BREATH: f32 = 20.0;

fn full() -> f32 {
    100.0
}
fn full_breath() -> f32 {
    BREATH
}
fn mild() -> f32 {
    16.0
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
        (1.0 - (-64.0 - y) / 400.0).max(0.15)
    } else if y > 350.0 {
        (1.0 - (y - 350.0) / 500.0).max(0.45)
    } else {
        1.0
    }
}

/// Felt temperature with clothes on: they warm you up to the top of the
/// comfortable range in the cold, and make heat worse (by half) when it's hot.
pub fn clothed(feels: f32, insulation: f32) -> f32 {
    if feels >= COMFORT_HIGH {
        feels + insulation * 0.5
    } else {
        (feels + insulation).min(COMFORT_HIGH.max(feels))
    }
}

impl Vitals {
    /// Movement multiplier from food buffs, weight, heat, cold and thin air.
    pub fn speed(&self, carried: f32, rules: &Exposure) -> f32 {
        let buff = if self.buff_left > 0.0 { 1.0 + self.buff_speed } else { 1.0 };
        let slow: f32 = [rules.temperature(self.feels).map(|t| t.1), rules.stage(Scale::Air, self.oxygen)].iter().flatten().map(|a| a.stage.slow).sum();
        weight_speed(carried) * buff * (1.0 - slow).max(0.3)
    }

    /// How dizzy thin air or heat make you, 0..1 (camera sway and dark edges).
    pub fn dizzy(&self, rules: &Exposure) -> f32 {
        let d: f32 = [rules.temperature(self.feels).map(|t| t.1), rules.stage(Scale::Air, self.oxygen)].iter().flatten().map(|a| a.stage.dizzy).sum();
        d.min(1.0)
    }

    /// Whether cold, heat or thin air is wearing you down.
    pub fn exposed(&self, rules: &Exposure) -> bool {
        rules.temperature(self.feels).is_some() || rules.stage(Scale::Air, self.oxygen).is_some()
    }

    /// Advance the body by `dt` seconds.
    pub fn tick(&mut self, env: &Env, rules: &Exposure, dt: f32) {
        if self.health.is_dead() {
            return;
        }

        // Food.
        let hunger = if env.sprinting { HUNGER * 2.0 } else { HUNGER };
        self.satiety = (self.satiety - hunger * dt).max(0.0);
        if self.satiety <= 0.0 {
            self.health.hit(Layer::Fatigue, 0.4 * dt);
        }
        self.buff_left = (self.buff_left - dt).max(0.0);

        // Air: the meter reads the air around you. Under water you hold your
        // breath until it runs out, then the water's lack of air reaches you.
        if env.underwater {
            self.breath = (self.breath - dt).max(0.0);
        } else {
            self.breath = (self.breath + dt * 5.0).min(rules.breath);
        }
        let ease = |v: &mut f32, to: f32, rate: f32| *v += (to - *v).clamp(-rate * dt, rate * dt);
        ease(&mut self.air, env.air.clamp(0.0, 1.0) * 100.0, 40.0);
        let gets = if env.underwater && self.breath > 0.0 { 1.0 } else { env.breathe };
        ease(&mut self.oxygen, gets.clamp(0.0, 1.0) * 100.0, 40.0);

        // The felt temperature follows the surroundings (`follow` seconds).
        self.feels += (env.feels - self.feels) * (1.0 - (-dt / rules.follow.max(0.1)).exp());

        // Exposure: stamina first, down to the mortal line; the worst stages hurt.
        let heavy = if is_heavy(env.carried) { 2.0 } else { 1.0 };
        let stages = [
            rules.temperature(self.feels).map(|(scale, a)| (a, if scale == Scale::Heat { heavy } else { 1.0 })),
            rules.stage(Scale::Air, self.oxygen).map(|a| (a, 1.0)),
        ];
        for (a, extra) in stages.into_iter().flatten() {
            let f = self.health.get(Layer::Fatigue);
            if self.health.mortal().is_none() && f > FATIGUE_FLOOR {
                self.health.hit(Layer::Fatigue, (a.stage.fatigue * a.ramp() * extra * dt).min(f - FATIGUE_FLOOR));
            }
            if a.stage.damage > 0.0 {
                self.health.hit(Layer::Damage, a.stage.damage * a.ramp() * dt);
            }
        }

        // Running wears you out, but never into the mortal half on its own.
        let suffering = self.satiety <= 0.0 || self.exposed(rules);
        let h = &mut self.health;
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
        let rules = crate::survival::exposure::tests::rules();
        for _ in 0..(seconds * 10.0) as usize {
            v.tick(&env, &rules, 0.1);
        }
    }

    fn cold(feels: f32) -> Env {
        Env { feels, ..Default::default() }
    }

    #[test]
    fn mild_cold_drains_stamina_but_stops_at_the_mortal_line() {
        let mut v = Vitals::default();
        run(&mut v, cold(0.0), 60.0);
        let f = v.health.get(Layer::Fatigue);
        assert!(f < MAX, "{:?}", v.health.bars);
        run(&mut v, cold(0.0), 600.0);
        assert_eq!(v.health.get(Layer::Fatigue), FATIGUE_FLOOR);
        assert_eq!(v.health.mortal(), None);
        // Damage heals only once you're out of the cold, so it stays as it was.
        assert_eq!(v.health.get(Layer::Damage), MAX);
    }

    #[test]
    fn colder_drains_faster_and_deadly_cold_hurts() {
        let mut a = Vitals::default();
        let mut b = Vitals::default();
        run(&mut a, cold(0.0), 30.0);
        run(&mut b, cold(-10.0), 30.0);
        assert!(b.health.get(Layer::Fatigue) < a.health.get(Layer::Fatigue));
        assert_eq!(b.health.get(Layer::Damage), MAX);
        let mut c = Vitals::default();
        run(&mut c, cold(-30.0), 120.0);
        assert!(c.health.get(Layer::Damage) < MAX, "{:?}", c.health.bars);
        // Left long enough, it kills.
        run(&mut c, cold(-30.0), 3000.0);
        assert!(c.health.is_dead(), "{:?}", c.health.bars);
    }

    #[test]
    fn heat_works_the_same_way() {
        let mut v = Vitals::default();
        run(&mut v, cold(30.0), 600.0);
        assert_eq!(v.health.get(Layer::Fatigue), FATIGUE_FLOOR);
        assert_eq!(v.health.get(Layer::Damage), MAX);
        run(&mut v, cold(48.0), 120.0);
        assert!(v.health.get(Layer::Damage) < MAX);
    }

    #[test]
    fn felt_temperature_follows_the_surroundings_slowly() {
        let mut v = Vitals::default();
        run(&mut v, cold(-20.0), 2.0);
        assert!(v.feels > 5.0, "{}", v.feels);
        run(&mut v, cold(-20.0), 200.0);
        assert!((v.feels + 20.0).abs() < 0.1);
    }

    #[test]
    fn comfortable_air_does_nothing() {
        let mut v = Vitals::default();
        run(&mut v, Env::default(), 120.0);
        assert_eq!(v.health.bars, [MAX; 4]);
        assert_eq!(v.air, 100.0);
    }

    #[test]
    fn starving_drains_fatigue() {
        let mut v = Vitals { satiety: 0.0, ..Default::default() };
        run(&mut v, Env::default(), 60.0);
        assert!(v.health.get(Layer::Fatigue) < 80.0, "{:?}", v.health.bars);
        assert_eq!(v.health.get(Layer::Blood), MAX);
    }

    #[test]
    fn thin_air_reads_on_the_meter_and_tires_you() {
        let mut v = Vitals::default();
        let thin = Env { air: 0.7, breathe: 0.7, ..Default::default() };
        run(&mut v, thin, 30.0);
        assert!((v.air - 70.0).abs() < 0.01);
        assert!(v.health.get(Layer::Fatigue) < MAX);
        assert_eq!(v.health.get(Layer::Damage), MAX);
        // A skill that eases thin air: same meter, no strain.
        let mut v = Vitals::default();
        run(&mut v, Env { air: 0.7, breathe: 0.9, ..Default::default() }, 30.0);
        assert!((v.air - 70.0).abs() < 0.01);
        assert_eq!(v.health.bars, [MAX; 4]);
    }

    #[test]
    fn under_water_you_hold_your_breath_then_suffocate() {
        let mut v = Vitals::default();
        let water = Env { air: 0.0, breathe: 0.0, underwater: true, ..Default::default() };
        run(&mut v, water, 10.0);
        assert_eq!(v.air, 0.0);
        assert_eq!(v.health.bars, [MAX; 4]);
        run(&mut v, water, 20.0);
        assert!(v.health.get(Layer::Damage) < MAX, "{:?}", v.health.bars);
        // Surfacing brings the breath back quickly.
        run(&mut v, Env::default(), 5.0);
        assert!(v.breath >= BREATH - 0.01 && v.oxygen > 99.0);
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
        assert!(v.buff_left > 0.0 && v.speed(15.0, &crate::survival::exposure::tests::rules()) > 1.05);
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
    fn clothes_warm_you_in_the_cold_and_cook_you_in_the_heat() {
        assert_eq!(clothed(-10.0, 12.0), 2.0);
        assert_eq!(clothed(20.0, 12.0), COMFORT_HIGH);
        assert_eq!(clothed(34.0, 12.0), 40.0);
        // A fur coat turns a freezing night into a merely chilly one.
        let rules = crate::survival::exposure::tests::rules();
        assert_eq!(rules.temperature(-10.0).unwrap().1.stage.name, "Freezing");
        assert_eq!(rules.temperature(clothed(-10.0, 16.0)).unwrap().1.stage.name, "Chilly");
    }

    #[test]
    fn air_is_thin_up_high_and_deep_down() {
        assert_eq!(air_at(64.0), 1.0);
        assert!(air_at(600.0) < 0.6);
        assert!(air_at(-300.0) < 0.5);
    }
}
