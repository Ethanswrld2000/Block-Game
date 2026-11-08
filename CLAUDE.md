# Block Game

An original voxel survival game in Rust on Bevy 0.20. Starting area: the Great Swamp.
The full research and phase plan lives in the Claude Doc "Block Game: Research and Build Plan".
This is not a Minecraft clone: original names, art, creatures and systems only.

## Layout

| Path | What | Rule |
| --- | --- | --- |
| `crates/voxel` | Coordinates, chunks, blocks, tile sheet | No Bevy, no rendering. Shared with a future server. |
| `crates/worldgen` | Terrain, biomes, caves, sky islands | Pure functions of (seed, position); thread-safe; no Bevy. |
| `crates/game` | The Bevy client; one plugin per system | Gameplay state separate from rendering, so multiplayer can come later. |
| `assets/textures/blocks.png` | 16x16 grid of 32x32 tiles | Placeholder art; Ethan repaints it. Keep tile positions stable. |
| `assets/textures/blocks.ron` | Tile name -> index | Regenerate with `python3 tools/gen_placeholder_art.py` (overwrites the PNG). |
| `assets/textures/ui.png` | UI sprite sheet, 2x pixels | Ethan repaints it; `ui.ron` lists each sprite's rect and slice. `tools/gen_ui_art.py` overwrites it. |
| `assets/fonts/` | Pixelify Sans (OFL) | Compiled in; swap the `.ttf` files to change the font. |

## Commands

- `cargo run --features dev` - play, fast rebuilds (dynamic linking).
- `cargo run --release` - check real performance.
- `cargo run --release --features trace` - profile with Tracy.
- `cargo check`, `cargo clippy --workspace`, `cargo test --workspace` - before every hand-off.

## Conventions

- Bevy is pinned to one version; upgrade only between phases. Bevy 0.20 renamed things
  (`shadow_maps_enabled`, `FontSize::Px`, `CursorOptions` as its own component) and shaders are
  WESL (`import bevy_pbr::render::...`, `@group(constants::MATERIAL_BIND_GROUP)`); check compiler
  hints and the 0.20.1 example shaders on GitHub before assuming older API.
- Content (blocks, items, foods, biomes, skill nodes) goes in RON data files, not code.
- Chunks are 32^3 (`voxel::CHUNK_SIZE`). Chunks stack vertically; no world height limit.
- Heavy work (generation, lighting, meshing) runs on `AsyncComputeTaskPool`, never the main thread.
- Pixel art is sampled nearest-neighbour (`ImagePlugin::default_nearest()`).
- Target: 60 fps at 16-chunk view distance on Ethan's MacBook (Apple Silicon, Metal).

## Status

- Phase 1 (foundation): done.
- Phase 2 (voxel core): done. Data-driven blocks (`assets/data/blocks.ron`), palette chunks,
  binary greedy mesher (`voxel::mesher`), texture-array terrain shader (`assets/shaders/voxel.wesl`),
  threaded generation/meshing in `game::world`, break/place in `game::interact`, test terrain in `worldgen`.
- Phase 3 (world generation): done. Climate noise + spline heights, ridged mountains, flat swamp,
  overhangs, caves, ores, data-driven biomes (`assets/data/biomes.ron`, picked by height band plus
  nearest temperature/humidity), trees (`worldgen::trees`), sky islands (`worldgen::islands`, y 340-600),
  spawn in the Great Swamp. Saving: edited chunks only, lz4 (`voxel::save`, `game::save`) to
  `saves/world1/chunks` (override with `BLOCKGAME_SAVES`), autosave every 30 s and on exit.
  Water and ice are see-through (separate blended material in `game::chunk_mesh`).
  `cargo run -p worldgen --release --example worldstats` prints biome stats and writes a map.
- Phase 4 (sky and atmosphere): done. `game::clock` (20-minute days, seasons from
  `assets/data/seasons.ron`, saved to `time.ron`), `game::weather` (kinds in `assets/data/weather.ron`,
  blended into the `Conditions` resource; local biome fog/haze/temperature in `LocalClimate`),
  `game::sky` (sun/moon light, ambient, fog, sky dome `assets/shaders/sky.wesl`, season tint of
  foliage via the voxel material's `season` uniform and UV_1.y), `game::clouds` (blocky, y 236; three layers since phase 10),
  `game::precipitation` (rain/snow pool), `game::lod` (distant terrain and islands as coloured
  columns out to 1500 blocks; `assets/shaders/lod.wesl` cuts it away inside the loaded area).
  Stars turn around the sun's axis; the moon has its own 8-day cycle and phases (`sky::moon_*`).
- Phase 5 (player): done. `game::player` (`Player` resource: AABB walking, gravity, jump, sprint,
  crouch edge-guard, swimming, head-bob, landing dip, fall `Damage` messages; F toggles `MoveMode`
  Walk/Fly), `game::items` (`assets/data/items.ron`: tools with per-material speeds, block items,
  torch; break time = hardness / speed), `game::interact` (one hand for now: 1-9/wheel select,
  hold LMB to mine, RMB places; `Hands.left` reserved for a later off-hand), `game::hands`
  (view-model arm and held item, built per `Hand`), `game::torches` (torch blocks
  are `shape: torch`, skipped by the chunk mesher and drawn as entities with point lights via
  `World::shape_events`), `game::hud` (hotbar, hurt flash). Blocks have `hardness`/`material`.
- Phase 6 (survival): done. `game::survival`: pure rules in `health` (four layers, entry layers,
  mortal rule, healing cap, bleeding), `vitals` (food, air, warmth, weight; `Vitals::tick(Env)`),
  `food` (`assets/data/food_tiers.ron`; foods are `Food(tier, satiety)` in items.ron, items have
  `mass`), `camp` (bedroll B, sleep Z, respawn), `hud`. `player::Body` carries speed/fall/dizzy/frozen
  from survival into movement; `player::DamageCause` picks the entry layer. Weather kinds have
  `chill` and `air`. Player state saves to `player.ron` (not in screenshot runs).
  `BLOCKGAME_VITALS=damage,fatigue,blood,injuries[,food,air,warmth]` sets bars for screenshots.
- Phase 7 (items and inventory): done. `game::inventory`: `grid` (items fill cells by size, rotate,
  stack; saved by item name), `Inventory` (pockets 5x3, worn `WearSlot`s, bag grid; mass,
  insulation, per-layer protection), `Chests` (chest block, `chests.ron`), `crafting`
  (`assets/data/recipes.ron`, `Station::Hand|Camp`), `drops` (`assets/data/drops.ron`; unlisted
  blocks drop their own item), `world_items` (dropped stacks, pickup), `ui` (Tab screen; hit-testing
  via `ComputedNode::contains_point`, since `Interaction` is deprecated in 0.20). Items have `size`,
  `stack`, `color` and kinds `Wear`, `Bag`, `Material`. New characters get `assets/data/start_kit.ron`.
  The hotbar holds shortcuts (`interact::Hotbar`, `Wielded` = selected item if you have any);
  `interact` writes `Mined`/`OpenChest` messages. Survival reads weight, clothes warmth
  (`vitals::clothed`) and armor from the inventory. `camera::Menus.open` frees the mouse.
  Saves: `inventory.ron`, `chests.ron`. `BLOCKGAME_INVENTORY=1` opens the screen for screenshots.
- Phase 8 (living world): done. `worldgen::structures` (villages of stilt huts every ~640 blocks,
  one always 50-90 blocks from spawn; barrows with a stair, hall and side rooms; names from
  syllables), `game::creatures` (kinds in `assets/data/creatures.ron`: temper Skittish/Neutral/
  Hostile/Spirit/Villager, box models, attacks with a `DamageCause`, spawn rules, drops; `brain` has
  the pure rules; ring spawner with caps; barrow wights once a day per barrow; spirits ignore walls,
  keep `brain::TORCH_FEAR` from placed or held torches, fade at dawn), `creatures::villagers`
  (one per hut, `brain::plan` schedule, talk lines in `assets/data/villagers.ron`), combat via
  `interact::{Aimed, Strike, UseCreature}` (items have `damage`), `game::places` (title cards for
  villages, barrows and regions), chest loot (`assets/data/loot.ron`, rolled on first open;
  placed chests start empty via `interact::Placed`), season weather (`cold_snap`, `gale` with
  wind push in `player`, `DamageCause::Chill` -> Fatigue). Creatures ignore a flying player.
- Phase 9 (progression and polish): `game::progress` (pure `xp` levels from
  `assets/data/progression.ron`; pure `skills` tree from `assets/data/skills.ron`: majors with
  weighted links, rings of 8 minors, taking a minor closes its neighbours, `Perks` sums the
  rewards; `Spells` Levitation/Light/Warmth on G/X/V burn focus; `GainXp` message from mining,
  kills, crafting, `places` finds and the first sky island; K opens the map screen in `progress::ui`;
  saved to `progress.ron`), `player::Body.levitate/feather`, `game::sound` (placeholder oggs from
  `tools/gen_placeholder_sounds.py`, material sounds in `assets/data/sounds.ron`, loops follow
  weather/night/levitation), `game::settings` (Esc pause menu, `saves/settings.ron` shared by all
  worlds: view distance, mouse, FOV, volume, ambience, clouds on/off, debug overlay),
  `game::bench` (`BLOCKGAME_BENCH=60` flies a fixed route with vsync off and logs frame times;
  `BLOCKGAME_NO_CLOUDS=1`, `BLOCKGAME_VSYNC=1`).
- Known gaps: vertices are 40 bytes, not the packed 8-byte format (performance pass); no thunder
  sound or snow cover yet; no block light propagation (torches are real point lights); items on the
  ground and creatures are not saved; creatures walk straight at goals (no pathfinding, villagers
  don't climb into huts: they vanish at the door at night); creature models are untextured boxes.
- Phase 10 (UI overhaul): done. `game::theme`: UI sprite sheet `assets/textures/ui.png` + `ui.ron`
  (name -> rect and 9-slice borders, drawn at 2x; placeholder from `tools/gen_ui_art.py`, which
  overwrites the PNG), `theme.skin("panel")` / `theme.icon("icon_food")` / `theme.image(..)`;
  frames draw on the border box (`VisualBox::BorderBox`; Bevy's default is the content box).
  Font: Pixelify Sans (`assets/fonts/ui.ttf`, `ui_bold.ttf`, OFL) loaded into Bevy's default font
  handle so all text uses it; `theme.bold(size)` for headings; every UI `Text` gets `theme::SHADOW`
  unless it has `NoShadow`. Colours in `theme::colors`, sizes in `theme::sizes`. Inventory: one
  framed window, item tooltips by the cursor, crafting list sorted makeable-first and scrolled with
  the wheel; hotbar shows block pictures. Clouds went back to blocky meshes, now three layers
  (`clouds::LAYERS`: low rain deck y 200, middle y 236, high wisps y 300) whose cover follows
  `Conditions.clouds`. `BLOCKGAME_PAUSE=1` (or `=settings`) opens the pause menu for screenshots.
- Next: ask Ethan what comes next.

## Testing without a screen

`BLOCKGAME_SCREENSHOT=shot.png cargo run` waits for chunk loading to settle, saves a screenshot and quits.
`BLOCKGAME_CAMERA=x,y,z,yaw,pitch` (or `x,y,z,@tx,ty,tz` to look at a point) places the camera first.
`BLOCKGAME_TIME=day:hour` and `BLOCKGAME_WEATHER=rain` set the scene; the clock stands still during a screenshot run.
With `BLOCKGAME_CAMERA` the shot is taken flying (`BLOCKGAME_WALK=1` stands there instead); without it, from the player standing at spawn.
`BLOCKGAME_SLOT=n` starts with hotbar slot n in hand; `BLOCKGAME_SKILLS=n` gives n points and learns Wanderer and Mystic;
`BLOCKGAME_CAST=levitate,light` starts spells; `BLOCKGAME_SKILLTREE=1` opens the tree; `BLOCKGAME_LOOK=fen_stag` turns the camera to the nearest creature of a kind.
The home village's name and position are logged at startup.
In the cloud container: `BEVY_ASSET_ROOT=$PWD BLOCKGAME_SCREENSHOT=/tmp/s.png WGPU_BACKEND=vulkan xvfb-run -s "-screen 0 1280x720x24" ./target/debug/block-game`
(needs `mesa-vulkan-drivers xvfb libxkbcommon-x11-0`).
