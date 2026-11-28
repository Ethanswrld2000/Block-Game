//! Items, from `assets/data/items.ron`: tools, placeable blocks, torches,
//! food, clothes and armor, bags and crafting materials. Each item has a size
//! in inventory cells, a stack size and a weight.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use voxel::{BlockDef, BlockId, BlockRegistry};

use crate::quality::{Qualities, Quality, Tuning};
use crate::survival::health::Layer;
use crate::swing::Swing;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(pub u16);

#[derive(Deserialize)]
struct ItemsSpec {
    hand: HashMap<String, f32>,
    items: Vec<ItemSpec>,
}

#[derive(Deserialize)]
struct ItemSpec {
    name: String,
    title: String,
    kind: KindSpec,
    #[serde(default = "default_mass")]
    mass: f32,
    /// Cells taken in a grid, (width, height).
    #[serde(default = "one_by_one")]
    size: (u8, u8),
    #[serde(default)]
    stack: Option<u16>,
    /// Damage dealt when you hit a creature with it.
    #[serde(default)]
    damage: Option<f32>,
    /// How it is swung (see `swing`).
    #[serde(default)]
    swing: Swing,
    /// A gun: left click fires it.
    #[serde(default)]
    gun: Option<GunSpec>,
    /// A weapon's moves (light combo, heavy, reach, arc, special).
    #[serde(default)]
    moves: Option<Moves>,
    /// A shield: held in the left hand, raised with right click.
    #[serde(default)]
    shield: Option<Shield>,
}

/// How a weapon fights (`moves:` in items.ron): tap for the light moves in
/// turn (a combo), hold and let go for the heavy.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Moves {
    /// The light moves, in combo order (one to three; the third hits 1.25x).
    pub light: Vec<Swing>,
    /// The charged move.
    pub heavy: Swing,
    /// Blocks from your eyes a move reaches.
    pub reach: f32,
    /// Half the arc a move sweeps, degrees (a spin hits all round).
    pub arc: f32,
    #[serde(default)]
    pub special: Special,
}

/// What sets a weapon apart.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub enum Special {
    /// The sword: nothing extra, nothing missing.
    #[default]
    Balanced,
    /// The dagger: backstabs deal this many times the damage (not `ai::BACKSTAB`).
    Backstab(f32),
    /// The axe: a blow caught on a guard still does this share of its damage.
    Cleave(f32),
    /// The spear: blows push foes back this many times as far.
    KeepAway(f32),
    /// The rapier: blows soon after a parry deal this many times the damage.
    Riposte(f32),
    /// The hammer: every blow breaks a raised guard (and a shield's), and the
    /// heavy knocks down everything close by.
    Wreck,
}

/// A shield in the left hand.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Shield {
    /// Share of a blocked blow's Fatigue cost it takes (a sword alone takes 1).
    pub cost: f32,
    /// Half the arc it covers, degrees.
    pub arc: f32,
    /// Damage of a shield bash (left click with it raised).
    pub bash: f32,
}

#[derive(Deserialize)]
struct GunSpec {
    damage: f32,
    range: f32,
    reload: f32,
    ammo: String,
}

/// A gun: what one shot does, how far it reaches (blocks), seconds between
/// shots, and the item each shot uses up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gun {
    pub damage: f32,
    pub range: f32,
    pub reload: f32,
    pub ammo: ItemId,
}

fn default_mass() -> f32 {
    0.5
}

fn one_by_one() -> (u8, u8) {
    (1, 1)
}

#[derive(Deserialize)]
enum KindSpec {
    Tool { speeds: HashMap<String, f32> },
    Block(String),
    Torch,
    Food { tier: u8, satiety: f32 },
    Wear {
        slot: WearSlot,
        #[serde(default)]
        insulation: f32,
        #[serde(default)]
        protection: HashMap<Layer, f32>,
        #[serde(default)]
        cooling: f32,
        #[serde(default)]
        air: f32,
    },
    Bag { grid: (u8, u8) },
    Material,
    Machine(String),
}

/// Where something is worn. Clothes and armor go on separate layers.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WearSlot {
    Head,
    HeadArmor,
    Body,
    BodyArmor,
    Legs,
    Feet,
    /// The bag on your back.
    Back,
}

impl WearSlot {
    #[allow(dead_code)] // tests walk every slot
    pub const ALL: [WearSlot; 7] =
        [WearSlot::Head, WearSlot::HeadArmor, WearSlot::Body, WearSlot::BodyArmor, WearSlot::Legs, WearSlot::Feet, WearSlot::Back];

    pub fn title(self) -> &'static str {
        match self {
            WearSlot::Head => "Head",
            WearSlot::HeadArmor => "Helmet",
            WearSlot::Body => "Body",
            WearSlot::BodyArmor => "Body armor",
            WearSlot::Legs => "Legs",
            WearSlot::Feet => "Feet",
            WearSlot::Back => "Back",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ItemKind {
    Tool { speeds: HashMap<String, f32> },
    Block(BlockId),
    Torch,
    /// Eaten by holding right click. `tier` is a tier in food_tiers.ron.
    Food { tier: u8, satiety: f32 },
    /// Clothes and armor: warmth in °C, and the share of a blow kept off each health bar.
    /// `cooling` (°C) takes heat off when it's hot; `air` (0..1) is the share of missing air it makes up.
    Wear { slot: WearSlot, insulation: f32, protection: HashMap<Layer, f32>, cooling: f32, air: f32 },
    /// Worn on the back; holds a grid of this size.
    Bag { grid: (u8, u8) },
    /// Crafting materials.
    Material,
    /// A machine from the machination table, set down in the world (`airship`).
    Machine(String),
}

#[derive(Debug, Clone)]
pub struct Item {
    pub name: String,
    pub title: String,
    pub kind: ItemKind,
    /// Weight in kg, per item.
    pub mass: f32,
    /// Cells in a grid, (width, height), unrotated.
    pub size: (u8, u8),
    /// How many fit in one stack.
    pub stack: u16,
    /// Damage when you hit a creature with it.
    pub damage: f32,
    /// How the arm swings it; mining blows land at the swing's contact.
    pub swing: Swing,
    /// Fires (left click) instead of swinging.
    pub gun: Option<Gun>,
    /// A weapon's moves.
    pub moves: Option<Moves>,
    /// A shield.
    pub shield: Option<Shield>,
    /// The plain item this is a better version of (itself for plain items).
    pub base: ItemId,
    pub quality: Quality,
    /// Indexes into `Qualities::traits`.
    pub traits: Vec<usize>,
    /// What its minor traits add up to.
    pub tuning: Tuning,
    /// Multiplier on the Fatigue its swings and blocks cost.
    pub stamina: f32,
}

/// Damage of a bare-handed blow.
pub const FIST_DAMAGE: f32 = 2.0;

impl Item {
    pub fn wear_slot(&self) -> Option<WearSlot> {
        match self.kind {
            ItemKind::Wear { slot, .. } => Some(slot),
            ItemKind::Bag { .. } => Some(WearSlot::Back),
            _ => None,
        }
    }

    /// A weapon for the weapon slot: a tool made only for hitting (swords,
    /// daggers, the spear), not for working blocks.
    pub fn is_weapon(&self) -> bool {
        matches!(&self.kind, ItemKind::Tool { speeds } if speeds.is_empty()) && self.damage > FIST_DAMAGE && self.shield.is_none()
    }

    /// Can it come in better qualities (tools, weapons, shields, armor)?
    pub fn takes_quality(&self) -> bool {
        matches!(self.kind, ItemKind::Tool { .. } | ItemKind::Wear { .. })
    }

    /// Does it have a trait with this effect?
    pub fn has(&self, quals: &Qualities, f: impl Fn(crate::quality::Effect) -> bool) -> bool {
        self.traits.iter().any(|t| quals.traits.get(*t).is_some_and(|d| f(d.effect)))
    }

    /// The first trait effect `f` picks out.
    pub fn effect<T>(&self, quals: &Qualities, f: impl Fn(crate::quality::Effect) -> Option<T>) -> Option<T> {
        self.traits.iter().filter_map(|t| quals.traits.get(*t)).find_map(|d| f(d.effect))
    }

    /// Reach of its moves (with traits), or a sword's.
    pub fn reach(&self) -> f32 {
        self.moves.as_ref().map_or(3.0, |m| m.reach) + self.tuning.reach
    }
}

#[derive(Resource, Debug, Clone)]
pub struct Items {
    hand: HashMap<String, f32>,
    list: Vec<Item>,
    /// Quality tiers and traits (`traits.ron`).
    quals: Arc<Qualities>,
    /// Better versions made on demand (`variant`), after `list`. Never freed
    /// (there are only ever a few hundred), so they can be lent out freely.
    extra: Arc<RwLock<Vec<&'static Item>>>,
}

impl Items {
    pub fn from_ron(text: &str, blocks: &BlockRegistry) -> Result<Self, String> {
        let spec: ItemsSpec = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
            .map_err(|e| format!("items.ron: {e}"))?;
        let mut list = Vec::new();
        let names: Vec<String> = spec.items.iter().map(|s| s.name.clone()).collect();
        for s in spec.items {
            let kind = match s.kind {
                KindSpec::Tool { speeds } => ItemKind::Tool { speeds },
                KindSpec::Block(b) => ItemKind::Block(
                    blocks.id(&b).ok_or_else(|| format!("item '{}' places unknown block '{b}'", s.name))?,
                ),
                KindSpec::Torch => ItemKind::Torch,
                KindSpec::Food { tier, satiety } => ItemKind::Food { tier, satiety },
                KindSpec::Wear { slot, insulation, protection, cooling, air } => {
                    if slot == WearSlot::Back {
                        return Err(format!("item '{}': things worn on the back are Bag items", s.name));
                    }
                    ItemKind::Wear { slot, insulation, protection, cooling, air: air.clamp(0.0, 0.9) }
                }
                KindSpec::Bag { grid } => ItemKind::Bag { grid },
                KindSpec::Material => ItemKind::Material,
                KindSpec::Machine(m) => ItemKind::Machine(m),
            };
            if list.iter().any(|i: &Item| i.name == s.name) {
                return Err(format!("item '{}' is defined twice", s.name));
            }
            if s.size.0 == 0 || s.size.1 == 0 {
                return Err(format!("item '{}' has an empty size", s.name));
            }
            let stack = s.stack.unwrap_or(match kind {
                ItemKind::Block(_) => 32,
                ItemKind::Torch => 16,
                ItemKind::Food { .. } => 8,
                ItemKind::Material => 32,
                _ => 1,
            });
            let damage = s.damage.unwrap_or(match kind {
                ItemKind::Tool { .. } => 4.0,
                _ => FIST_DAMAGE,
            });
            let gun = match s.gun {
                Some(g) => {
                    let ammo = names.iter().position(|n| *n == g.ammo).ok_or_else(|| format!("item '{}' fires unknown ammo '{}'", s.name, g.ammo))?;
                    Some(Gun { damage: g.damage, range: g.range, reload: g.reload, ammo: ItemId(ammo as u16) })
                }
                None => None,
            };
            if let Some(m) = &s.moves
                && (m.light.is_empty() || m.light.len() > 3 || m.light.iter().chain([&m.heavy]).any(|s| !s.fights() && *s != Swing::Chop))
            {
                return Err(format!("item '{}': moves need one to three light fighting moves and a fighting heavy", s.name));
            }
            if s.name.contains(['@', '+']) {
                return Err(format!("item '{}': names can't hold @ or +", s.name));
            }
            let base = ItemId(list.len() as u16);
            list.push(Item {
                name: s.name,
                title: s.title,
                kind,
                mass: s.mass,
                size: s.size,
                stack: stack.max(1),
                damage,
                swing: s.swing,
                gun,
                moves: s.moves,
                shield: s.shield,
                base,
                quality: Quality::Plain,
                traits: Vec::new(),
                tuning: Tuning::default(),
                stamina: 1.0,
            });
        }
        let quals = Qualities::load(include_str!("../../../assets/data/traits.ron"))?;
        Ok(Self { hand: spec.hand, list, quals: Arc::new(quals), extra: Arc::default() })
    }

    pub fn get(&self, id: ItemId) -> &Item {
        let i = id.0 as usize;
        if i < self.list.len() {
            &self.list[i]
        } else {
            self.extra.read().unwrap_or_else(|e| e.into_inner())[i - self.list.len()]
        }
    }

    /// An item by name; a better version (`iron_sword@fine+reach`) is made
    /// the first time it's asked for.
    pub fn id(&self, name: &str) -> Option<ItemId> {
        if let Some(i) = self.list.iter().position(|i| i.name == name) {
            return Some(ItemId(i as u16));
        }
        let (base, q, traits) = self.quals.parse(name)?;
        let base = self.list.iter().position(|i| i.name == base)?;
        Some(self.variant(ItemId(base as u16), q, &traits))
    }

    /// Quality tiers and traits.
    pub fn qualities(&self) -> &Qualities {
        &self.quals
    }

    /// The version of `base` with quality `q` and these traits (`base` itself
    /// for a plain one, or an item that can't have a quality).
    pub fn variant(&self, base: ItemId, q: Quality, traits: &[usize]) -> ItemId {
        let base = self.get(base).base;
        let b = &self.list[base.0 as usize];
        if !b.takes_quality() || (q == Quality::Plain && traits.is_empty()) {
            return base;
        }
        let mut traits: Vec<usize> = traits.iter().copied().filter(|t| *t < self.quals.traits.len()).collect();
        traits.sort();
        traits.dedup();
        let name = self.quals.name(&b.name, q, &traits);
        let mut extra = self.extra.write().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = extra.iter().position(|i| i.name == name) {
            return ItemId((self.list.len() + i) as u16);
        }
        let tier = self.quals.tier(q);
        let up = 1.0 + tier.power;
        let tuning = self.quals.sum(&traits);
        let mut it = b.clone();
        it.title = format!("{} {}", tier.title, b.title);
        it.name = name;
        it.quality = q;
        it.traits = traits;
        it.tuning = tuning;
        it.stamina = (1.0 - tier.stamina) * (1.0 - tuning.lighter).max(0.2);
        it.damage *= up;
        if let Some(g) = &mut it.gun {
            g.damage *= up;
        }
        if let Some(sh) = &mut it.shield {
            sh.cost /= up;
            sh.bash *= up;
        }
        match &mut it.kind {
            ItemKind::Tool { speeds } => speeds.values_mut().for_each(|v| *v *= up),
            ItemKind::Wear { protection, .. } => protection.values_mut().for_each(|v| *v = (*v * up).min(0.9)),
            _ => {}
        }
        extra.push(Box::leak(Box::new(it)));
        ItemId((self.list.len() + extra.len() - 1) as u16)
    }

    /// Roll a quality from `odds` and traits to go with it, for `base`.
    pub fn roll(&self, base: ItemId, odds: crate::quality::Odds, rng: &mut crate::creatures::brain::Rng) -> ItemId {
        if !self.get(base).takes_quality() {
            return base;
        }
        let q = Qualities::roll_quality(odds, rng);
        let traits = self.quals.roll_traits(q, rng);
        self.variant(base, q, &traits)
    }

    /// One quality up (fine traits kept, one major trait added on the way to
    /// masterwork), or `None` if it can't go higher.
    pub fn upgraded(&self, id: ItemId, rng: &mut crate::creatures::brain::Rng) -> Option<ItemId> {
        let it = self.get(id);
        if !it.takes_quality() {
            return None;
        }
        let next = it.quality.next()?;
        let want = self.quals.roll_traits(next, rng);
        // Keep what it had, topping up with fresh ones of the ranks it lacks.
        let mut traits = it.traits.clone();
        for t in want {
            let rank = self.quals.traits[t].rank;
            let have = traits.iter().filter(|x| self.quals.traits[**x].rank == rank).count();
            let tier = self.quals.tier(next);
            let room = match rank {
                crate::quality::Rank::Minor => tier.minor,
                crate::quality::Rank::Major => tier.major,
                crate::quality::Rank::Relic => tier.relic,
            } as usize;
            if have < room && !traits.contains(&t) {
                traits.push(t);
            }
        }
        Some(self.variant(it.base, next, &traits))
    }

    pub fn ids(&self) -> impl Iterator<Item = ItemId> {
        (0..self.list.len()).map(|i| ItemId(i as u16))
    }

    /// The item that places this block, if any.
    pub fn for_block(&self, block: BlockId) -> Option<ItemId> {
        self.list.iter().position(|i| matches!(i.kind, ItemKind::Block(b) if b == block)).map(|i| ItemId(i as u16))
    }

    /// Mining speed of what is in the hand (nothing, a tool, or anything
    /// else, which works like a bare hand) against a material.
    pub fn speed(&self, held: Option<ItemId>, material: &str) -> f32 {
        let hand = self.hand.get(material).copied().unwrap_or(1.0);
        match held.map(|i| &self.get(i).kind) {
            Some(ItemKind::Tool { speeds }) => speeds.get(material).copied().unwrap_or(hand).max(hand),
            _ => hand,
        }
    }

    /// Whether what's in the hand is a tool made for this material (a shovel
    /// for soil). Only those take a whole block of soil; anything else digs
    /// it down a layer at a time (`BlockDef::dig_to`).
    pub fn rated(&self, held: Option<ItemId>, material: &str) -> bool {
        held.is_some_and(|i| matches!(&self.get(i).kind, ItemKind::Tool { speeds } if speeds.contains_key(material)))
    }

    /// What a blow that breaks `block` leaves in its place: a lower layer of
    /// soil when it wasn't dug with the right tool, else nothing.
    pub fn dug_to(&self, held: Option<ItemId>, block: &BlockDef) -> Option<voxel::BlockId> {
        block.dig_to.filter(|_| !self.rated(held, &block.material))
    }

    /// Seconds to break a block, or `None` if it can't be broken.
    pub fn break_time(&self, held: Option<ItemId>, block: &BlockDef) -> Option<f32> {
        if block.hardness < 0.0 {
            return None;
        }
        // Nothing to it (torches, pebbles, twigs): anything takes it at once.
        if block.hardness == 0.0 {
            return Some(0.0);
        }
        // A speed of 0 means this can't cut it at all (logs by hand).
        let speed = self.speed(held, &block.material);
        (speed > 0.0).then(|| block.hardness / speed)
    }
}

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::menu::GamePreStartup, load_items.after(crate::blocks::load_blocks));
    }
}

fn load_items(mut commands: Commands, blocks: Res<crate::blocks::Blocks>) {
    let items = Items::from_ron(include_str!("../../../assets/data/items.ron"), &blocks.registry).unwrap_or_else(|e| panic!("{e}"));
    commands.insert_resource(items);
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use voxel::TileSheet;

    pub fn load() -> (BlockRegistry, Items) {
        let tiles = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
        let blocks = BlockRegistry::from_ron(include_str!("../../../assets/data/blocks.ron"), &tiles).unwrap();
        let items = Items::from_ron(include_str!("../../../assets/data/items.ron"), &blocks).unwrap();
        (blocks, items)
    }

    #[test]
    fn doors_open_and_close() {
        let (blocks, _) = load();
        let shut = blocks.id("wooden_door").unwrap();
        let open = crate::interact::door_swap(&blocks, shut).expect("opens");
        assert!(!blocks.get(open).solid);
        assert_eq!(crate::interact::door_swap(&blocks, open), Some(shut));
        assert_eq!(crate::interact::door_swap(&blocks, blocks.id("chair").unwrap()), None);
    }

    #[test]
    fn right_tool_is_faster() {
        let (blocks, items) = load();
        let stone = blocks.get(blocks.expect("stone"));
        let log = blocks.get(blocks.expect("log"));
        let pick = items.id("stone_pickaxe");
        let axe = items.id("stone_axe");
        let hand = items.break_time(None, stone);
        let picked = items.break_time(pick, stone).unwrap();
        assert!(picked < 7.0, "pick {picked}");
        // Bare hands can't break stone, wood or giant mushrooms at all.
        assert_eq!(hand, None);
        for b in ["stone", "cobblestone", "planks", "mushroom_stem", "mushroom_cap", "coal_ore"] {
            assert_eq!(items.break_time(None, blocks.get(blocks.expect(b))), None, "{b}");
        }
        // Logs need an axe: not bare hands, not a pickaxe.
        assert!(items.break_time(axe, log).is_some_and(|t| t < 1.0));
        assert_eq!(items.break_time(pick, log), None);
        assert_eq!(items.break_time(None, log), None);
        // A torch in the hand mines like a bare hand.
        assert_eq!(items.break_time(items.id("torch"), stone), None);
        let dirt = blocks.get(blocks.expect("dirt"));
        assert_eq!(items.break_time(items.id("torch"), dirt), items.break_time(None, dirt));
    }

    #[test]
    fn base_tools_take_about_twelve_swings() {
        use crate::swing::{blow_share, Swing};
        let (blocks, items) = load();
        let swings = |tool: &str, block: &str, swing: Swing| {
            let t = items.break_time(items.id(tool), blocks.get(blocks.expect(block))).unwrap();
            (1.0 / blow_share(swing, t) - 1e-4).ceil() as u32
        };
        assert_eq!(swings("rough_axe", "log", Swing::Chop), 12);
        assert_eq!(swings("rough_axe", "swamp_log", Swing::Chop), 12);
        assert_eq!(swings("stone_pickaxe", "stone", Swing::Overhead), 12);
        assert_eq!(swings("stone_pickaxe", "cobblestone", Swing::Overhead), 12);
    }

    #[test]
    fn hands_dig_soil_a_layer_at_a_time_and_shovels_take_it_whole() {
        use crate::swing::{blow_share, Swing};
        let (blocks, items) = load();
        let punches = |held: Option<ItemId>, b: voxel::BlockId, s: Swing| (1.0 / blow_share(s, items.break_time(held, blocks.get(b)).unwrap()) - 1e-4).ceil() as u32;
        // By hand: dirt -> 6/8 -> 4/8 -> 2/8 -> gone, a few punches each.
        let mut b = blocks.expect("grass");
        let (mut steps, mut total) = (0, 0);
        loop {
            total += punches(None, b, Swing::Jab);
            steps += 1;
            match items.dug_to(None, blocks.get(b)) {
                Some(next) => b = next,
                None => break,
            }
        }
        assert_eq!(steps, 4);
        assert!((12..=24).contains(&total), "{total} punches");
        // A pickaxe isn't a shovel: it digs dirt like a hand.
        assert!(items.dug_to(items.id("stone_pickaxe"), blocks.get(blocks.expect("dirt"))).is_some());
        // A shovel takes the whole block, and snow, in a couple of swings.
        let shovel = items.id("stone_shovel");
        assert_eq!(items.get(shovel.unwrap()).swing, Swing::Dig);
        for name in ["dirt", "grass", "sand", "snow", "dirt_layer_4"] {
            let b = blocks.expect(name);
            assert_eq!(items.dug_to(shovel, blocks.get(b)), None, "{name}");
            assert!(punches(shovel, b, Swing::Dig) <= 3, "{name}");
        }
    }

    #[test]
    fn foliage_takes_a_few_punches_and_brush_slows_you() {
        use crate::swing::{blow_share, Swing};
        let (blocks, items) = load();
        let punches = |name: &str| (1.0 / blow_share(Swing::Jab, items.break_time(None, blocks.get(blocks.expect(name))).unwrap()) - 1e-4).ceil() as u32;
        for name in ["leaves", "swamp_leaves", "redwood_leaves", "shrub", "dry_brush"] {
            assert!((3..=4).contains(&punches(name)), "{name}: {}", punches(name));
        }
        assert!(blocks.get(blocks.expect("shrub")).slows < 0.8);
        assert_eq!(blocks.get(blocks.expect("dirt")).slows, 1.0);
        // An axe still cuts through leaves in one swing.
        let t = items.break_time(items.id("rough_axe"), blocks.get(blocks.expect("leaves"))).unwrap();
        assert_eq!((1.0 / blow_share(Swing::Chop, t) - 1e-4).ceil() as u32, 1);
    }

    #[test]
    fn water_cannot_be_broken_and_torches_break_instantly() {
        let (blocks, items) = load();
        assert_eq!(items.break_time(None, blocks.get(blocks.expect("water"))), None);
        assert_eq!(items.break_time(None, blocks.get(blocks.expect("torch"))), Some(0.0));
    }

    #[test]
    fn wearables_and_bags() {
        let (_, items) = load();
        let satchel = items.get(items.id("reed_satchel").unwrap());
        assert_eq!(satchel.wear_slot(), Some(WearSlot::Back));
        let coat = items.get(items.id("fur_coat").unwrap());
        assert!(matches!(coat.kind, ItemKind::Wear { slot: WearSlot::Body, insulation, .. } if insulation > 8.0));
    }
}
