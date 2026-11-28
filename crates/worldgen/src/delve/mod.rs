//! Dungeon floors and the data they are built from.
//!
//! Every dungeon (`crate::dungeons`: an entrance on the island) leads to floors
//! built in the pocket (`POCKET`), a stretch of empty sky far out from the
//! island where nothing else generates. A floor is laid out fresh for every
//! run, as a pure function of the seed, the dungeon, the run and the floor
//! (`floor::floor`), from room templates (`assets/data/rooms.ron`) dressed in
//! the dungeon's theme (`assets/data/dungeons.ron`: blocks, foes, bosses,
//! hazards, puzzles). The game asks `WorldGen::generate_pocket` for chunks there
//! and never saves them.

mod draw;
pub mod floor;

pub(crate) use draw::fill;

use serde::Deserialize;
use voxel::{BlockId, BlockRegistry};

pub use floor::{floor, floors, ChestKind, ChestSpot, Doorway, Floor, FloorPlan, HangKind, Hanging, HazardSpot, Link, Puzzle, Reward, Room};

/// Where the floors are built: far out in the empty sky, above every island
/// (sky islands stop at y 600) and under the ceiling.
pub const POCKET: [i32; 3] = [60_000, 780, 60_000];
/// Half the size of the pocket, blocks (every floor fits well inside).
pub(crate) const POCKET_HALF: i32 = 1_000;
/// The most floors a dungeon has.
pub const MAX_FLOORS: u8 = 4;

/// Is this column in the pocket where the floors are built?
pub fn in_pocket(x: i32, z: i32) -> bool {
    (x - POCKET[0]).abs() <= POCKET_HALF && (z - POCKET[2]).abs() <= POCKET_HALF
}

/// What a room is for.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoomType {
    /// Where you arrive; the way back up is in its west wall.
    Start,
    /// A group of foes and a hazard or two; the doors lock until they fall.
    Fight,
    /// A trap gauntlet.
    Trap,
    /// A puzzle; its prize waits behind a gate.
    Puzzle,
    /// The doors lock and foes come in waves.
    Ambush,
    /// A chest guarded by a trap or an elite.
    Treasure,
    /// Unused (secret rooms behind cracked walls are gone); kept so old data loads.
    Secret,
    /// A campfire that heals.
    Rest,
    /// A merchant who takes old coin.
    Shop,
    /// The boss arena, with the way on.
    Boss,
}

impl RoomType {
    /// Walking in shuts its doors until every foe in it is down.
    pub fn locks(self) -> bool {
        matches!(self, RoomType::Fight | RoomType::Ambush | RoomType::Boss)
    }
}

/// A hazard: blocks and things in the world that hurt whoever is caught,
/// foes too.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hazard {
    /// Spikes that stab up out of a floor plate in turn.
    Spikes,
    /// A hole with spikes at the bottom.
    Pit,
    /// A slick of oil: burns when fire reaches it.
    Oil,
    /// A barrel of blasting powder: blows up when struck, burnt or kicked.
    Barrel,
    /// A wall that shoots poison darts across.
    Darts,
    /// A blade swinging on a chain from the ceiling.
    Blade,
    /// Cracked floor that gives way when stood on.
    Collapse,
    /// A vent in the floor that throws up flame in turn.
    FireJet,
    /// rooms.ron only: one of the theme's hazards, by its weights.
    Any,
}

impl Hazard {
    pub const ALL: [Hazard; 8] = [Hazard::Spikes, Hazard::Pit, Hazard::Oil, Hazard::Barrel, Hazard::Darts, Hazard::Blade, Hazard::Collapse, Hazard::FireJet];
}

/// How a floor's puzzle room is solved.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PuzzleKind {
    /// Weigh every pressure plate down at once: push stone blocks onto them
    /// and stand on the last.
    Plates,
    /// Each lever flips some lamps over the gate; light them all.
    Levers,
    /// A lever opens the gate for a few seconds; get through before it shuts.
    Timed,
}

fn one() -> f32 {
    1.0
}

/// A room template from rooms.ron.
#[derive(Deserialize, Debug, Clone)]
pub struct RoomDef {
    pub name: String,
    pub kind: RoomType,
    #[serde(default = "one")]
    pub weight: f32,
    /// Half widths across x and z, (min, max) each.
    pub half: ((i32, i32), (i32, i32)),
    pub height: (i32, i32),
    #[serde(default)]
    pub pillars: bool,
    #[serde(default)]
    pub pit: bool,
    #[serde(default)]
    pub hazards: Vec<(Hazard, u8, u8)>,
    /// Foes per wave (min, max).
    #[serde(default)]
    pub foes: (u8, u8),
    /// Waves of foes (0 = 1).
    #[serde(default)]
    pub waves: u8,
    /// Chance one foe is an elite.
    #[serde(default)]
    pub elite: f32,
    #[serde(default)]
    pub min_floor: u8,
    #[serde(default)]
    pub themes: Vec<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ThemeDef {
    pub name: String,
    pub title: String,
    pub place: Vec<String>,
    pub wall: String,
    pub floor: String,
    pub pillar: String,
    #[serde(default)]
    pub growth: Vec<String>,
    #[serde(default)]
    pub growth_share: f32,
    #[serde(default)]
    pub pools: Option<String>,
    /// foes.ron kinds the game picks from by role.
    pub foes: Vec<String>,
    pub warden: String,
    pub boss: String,
    pub hazards: Vec<(Hazard, f32)>,
    pub puzzles: Vec<PuzzleKind>,
}

/// One of the story dungeons.
#[derive(Deserialize, Debug, Clone)]
pub struct ChainDef {
    pub theme: String,
    pub floors: u8,
    pub name: String,
}

#[derive(Deserialize)]
struct DungeonsRon {
    themes: Vec<ThemeDef>,
    chain: Vec<ChainDef>,
}

/// A theme with its blocks looked up.
#[derive(Debug, Clone)]
pub struct Theme {
    pub def: ThemeDef,
    pub(crate) wall: BlockId,
    pub(crate) floor: BlockId,
    pub(crate) pillar: BlockId,
    pub(crate) growth: Vec<BlockId>,
    pub(crate) pools: Option<BlockId>,
}

/// Blocks every floor uses.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Parts {
    pub cracked_floor: BlockId,
    pub slab: BlockId,
    pub oil: BlockId,
    pub plate: BlockId,
    pub sconce: BlockId,
    pub chain: BlockId,
    pub campfire: BlockId,
    pub chest: BlockId,
    pub cobblestone: BlockId,
}

/// All dungeon data: themes, the story chain and room templates.
#[derive(Debug, Clone)]
pub struct Delves {
    pub themes: Vec<Theme>,
    pub chain: Vec<ChainDef>,
    pub rooms: Vec<RoomDef>,
    pub(crate) parts: Parts,
}

impl Delves {
    pub fn load(dungeons_ron: &str, rooms_ron: &str, blocks: &BlockRegistry) -> Result<Self, String> {
        let d: DungeonsRon = ron::from_str(dungeons_ron).map_err(|e| format!("dungeons.ron: {e}"))?;
        let rooms: Vec<RoomDef> = ron::from_str(rooms_ron).map_err(|e| format!("rooms.ron: {e}"))?;
        let b = |n: &str| blocks.id(n).ok_or_else(|| format!("dungeons need block '{n}'"));
        // Every block a floor is built of keeps the outside out: no leaves, ice
        // or glass (light and wind would come through, and you could see the sky).
        let solid = |n: &str| -> Result<BlockId, String> {
            let id = b(n)?;
            let def = blocks.get(id);
            if !def.opaque || !def.solid || def.material == "plant" {
                return Err(format!("dungeons.ron: '{n}' isn't a solid, opaque block"));
            }
            Ok(id)
        };
        let mut themes = Vec::new();
        for def in d.themes {
            themes.push(Theme {
                wall: solid(&def.wall)?,
                floor: solid(&def.floor)?,
                pillar: solid(&def.pillar)?,
                growth: def.growth.iter().map(|n| solid(n)).collect::<Result<_, _>>()?,
                pools: def.pools.as_deref().map(b).transpose()?,
                def,
            });
        }
        if themes.is_empty() {
            return Err("dungeons.ron: no themes".into());
        }
        for c in &d.chain {
            if !themes.iter().any(|t| t.def.name == c.theme) {
                return Err(format!("dungeons.ron: chain theme '{}' isn't a theme", c.theme));
            }
            if !(1..=MAX_FLOORS).contains(&c.floors) {
                return Err(format!("dungeons.ron: {} has {} floors (1-{MAX_FLOORS})", c.name, c.floors));
            }
        }
        for r in &rooms {
            if r.half.0 .0 < 3 || r.half.1 .0 < 3 || r.half.0 .1 > floor::CAP || r.half.1 .1 > floor::CAP || r.half.0 .0 > r.half.0 .1 || r.half.1 .0 > r.half.1 .1 {
                return Err(format!("rooms.ron: {} half sizes must be 3..={}", r.name, floor::CAP));
            }
            if r.height.0 < 4 || r.height.0 > r.height.1 {
                return Err(format!("rooms.ron: {} is too low", r.name));
            }
        }
        // Every kind of room a floor needs has a template.
        for k in [RoomType::Start, RoomType::Fight, RoomType::Trap, RoomType::Puzzle, RoomType::Ambush, RoomType::Treasure, RoomType::Rest, RoomType::Shop, RoomType::Boss] {
            if !rooms.iter().any(|r| r.kind == k && r.min_floor <= 1 && r.themes.is_empty()) {
                return Err(format!("rooms.ron: no {k:?} room for every theme and floor"));
            }
        }
        let parts = Parts {
            cracked_floor: b("cracked_floor")?,
            slab: b("dungeon_slab")?,
            oil: b("oil")?,
            plate: b("pressure_plate")?,
            sconce: b("sconce")?,
            chain: b("iron_chain")?,
            campfire: b("campfire")?,
            chest: b("chest")?,
            cobblestone: b("cobblestone")?,
        };
        Ok(Self { themes, chain: d.chain, rooms, parts })
    }

    pub fn theme_named(&self, name: &str) -> Option<usize> {
        self.themes.iter().position(|t| t.def.name == name)
    }

    /// Pick a template of `kind` for a theme and floor, from a number in 0..1.
    pub fn pick_room(&self, kind: RoomType, theme: usize, floor: u8, u: f32) -> usize {
        let name = &self.themes[theme].def.name;
        let fits = |r: &&RoomDef| r.kind == kind && r.min_floor <= floor && (r.themes.is_empty() || r.themes.contains(name));
        let total: f32 = self.rooms.iter().filter(fits).map(|r| r.weight.max(0.0)).sum();
        let mut left = u.clamp(0.0, 0.9999) * total;
        let mut last = 0;
        for (i, r) in self.rooms.iter().enumerate() {
            if !fits(&r) {
                continue;
            }
            last = i;
            left -= r.weight.max(0.0);
            if left < 0.0 {
                return i;
            }
        }
        last
    }

    /// One of the theme's hazards, by its weights, from a number in 0..1.
    pub fn pick_hazard(&self, theme: usize, u: f32) -> Hazard {
        let list = &self.themes[theme].def.hazards;
        let total: f32 = list.iter().map(|h| h.1.max(0.0)).sum();
        let mut left = u.clamp(0.0, 0.9999) * total;
        for &(h, w) in list {
            left -= w.max(0.0);
            if left < 0.0 && h != Hazard::Any {
                return h;
            }
        }
        Hazard::Spikes
    }
}

#[cfg(test)]
pub(crate) fn test_delves() -> Delves {
    let tiles = voxel::TileSheet::from_ron(include_str!("../../../../assets/textures/blocks.ron")).unwrap();
    let reg = voxel::BlockRegistry::from_ron(include_str!("../../../../assets/data/blocks.ron"), &tiles).unwrap();
    Delves::load(include_str!("../../../../assets/data/dungeons.ron"), include_str!("../../../../assets/data/rooms.ron"), &reg).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dungeon_data_loads_with_five_story_dungeons() {
        let d = test_delves();
        assert_eq!(d.themes.len(), 5);
        assert_eq!(d.chain.len(), 5);
        assert_eq!(d.chain[0].theme, "rotting_bog");
        assert_eq!(d.chain[0].floors, 2, "the Rotting Gate is short");
        assert_eq!(d.chain[4].theme, "frozen_vault");
    }

    #[test]
    fn room_picks_follow_kind_floor_and_weight() {
        let d = test_delves();
        for k in [RoomType::Fight, RoomType::Trap, RoomType::Boss, RoomType::Puzzle] {
            for i in 0..50 {
                let r = &d.rooms[d.pick_room(k, 0, 1, i as f32 / 50.0)];
                assert_eq!(r.kind, k);
                assert!(r.min_floor <= 1, "{} on floor 1", r.name);
            }
        }
        // Over enough rolls every fight template shows up on a deep floor.
        let seen: std::collections::HashSet<_> = (0..200).map(|i| d.pick_room(RoomType::Fight, 0, 4, i as f32 / 200.0)).collect();
        assert_eq!(seen.len(), d.rooms.iter().filter(|r| r.kind == RoomType::Fight).count());
        // Hazard picks never give `Any`.
        assert!((0..100).all(|i| d.pick_hazard(1, i as f32 / 100.0) != Hazard::Any));
    }
}
