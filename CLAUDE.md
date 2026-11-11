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
| `assets/textures/blocks.ron` | Tile name -> index | Regenerate with `python3 tools/gen_placeholder_art.py` (overwrites the PNG; `--only <tiles>` adds tiles without touching the rest). |
| `assets/textures/ui.png` | UI sprite sheet, 2x pixels | Ethan repaints it; `ui.ron` lists each sprite's rect and slice. `tools/gen_ui_art.py` overwrites it. |
| `assets/textures/icons/` | Item sprites: `items`, `weapons` (tools), `blocks` (.png + .ron) | 20 px per inventory cell; Ethan repaints them. `tools/gen_item_art.py` only adds missing sprites (`--reset <sheet>` redraws). |
| `assets/fonts/` | Jersey 15 (OFL) | Compiled in; swap the `.ttf` files to change the font. |

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
- Chunks are 32^3 (`voxel::CHUNK_SIZE`). Chunks stack vertically. The world runs from unbreakable
  `rootstone` at y -300 (`worldgen::WORLD_FLOOR`, top a little bumpy) to a ceiling at y 1000
  (`worldgen::WORLD_CEILING`: nothing generates or can be placed there, movement stops under it).
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
  foliage via the voxel material's `season` uniform and UV_1.y), `game::clouds` (blocky, y 236; three layers since phase 10; drift with `game::wind` since phase 11),
  `game::precipitation` (rain/snow pool), `game::lod` (distant terrain and islands as coloured
  columns out to 1500 blocks; `assets/shaders/lod.wesl` cuts it away only where a real chunk is drawn, from a per-chunk mask `World::drawn_mask`).
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
  stack; saved by item name), `Inventory` (pockets 20x10, worn `WearSlot`s, bag grid unused for now: no bag recipes or start bag; mass,
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
  weather/levitation; no night loop, Ethan asked for it removed), `game::settings` (Esc pause menu, `saves/settings.ron` shared by all
  worlds: view distance, mouse, FOV, volume, ambience, clouds on/off, debug overlay),
  `game::bench` (`BLOCKGAME_BENCH=60` flies a fixed route with vsync off and logs frame times;
  `BLOCKGAME_NO_CLOUDS=1`, `BLOCKGAME_VSYNC=1`).
- Creature skins: `creatures.ron` kinds name `skin: "x"` (or `skins: [...]`, one picked per
  creature) = `assets/textures/creatures/x.png`. `game::creatures::skin` (pure) unfolds each model
  box into a net (top/bottom row, then right/front/left/back) at 32 px per block, packed in list order
  (append new boxes so painted ones don't move; `uv: (x, y)` pins one). Boxes have `name`s for
  animation later. `cargo run --bin creature_skins` writes starter PNGs for missing skins and
  guides in `assets/textures/creatures/guides/`. No skin or missing PNG = flat `color`s. Hurt tints
  the skin red; spirits blend the PNG's alpha. `--features dev` hot-reloads PNGs (`bevy/file_watcher`).
  A PNG may be painted at 2x/4x as long as its shape matches (tested).
- Known gaps: vertices are 40 bytes, not the packed 8-byte format (performance pass); no thunder
  sound or snow cover yet; no block light propagation (torches are real point lights); items on the
  ground and creatures are not saved; creatures walk straight at goals (no pathfinding, villagers
  don't climb into huts: they vanish at the door at night); creature parts don't animate yet.
- Phase 10 (UI overhaul): done. `game::theme`: UI sprite sheet `assets/textures/ui.png` + `ui.ron`
  (name -> rect and 9-slice borders, drawn at 2x; placeholder from `tools/gen_ui_art.py`, which
  overwrites the PNG), `theme.skin("panel")` / `theme.icon("icon_food")` / `theme.image(..)`;
  frames draw on the border box (`VisualBox::BorderBox`; Bevy's default is the content box).
  Font: Jersey 15 (`assets/fonts/ui.ttf`, `ui_bold.ttf`, OFL; Pixelify Sans was too hard to read; sizes in `theme::sizes` sit on its 1/27-em pixel grid) loaded into Bevy's default font
  handle so all text uses it; `theme.bold(size)` for headings; every UI `Text` gets `theme::SHADOW`
  unless it has `NoShadow`. Colours in `theme::colors`, sizes in `theme::sizes`. Inventory: one
  framed window (the 20x10 grid, plus a side panel with Crafting / Clothes / Chest tabs), item tooltips by the cursor, crafting list sorted makeable-first and scrolled with
  the wheel; hotbar shows block pictures. Clouds went back to blocky meshes, now three layers
  (`clouds::LAYERS`: low rain deck y 200, middle y 236, high wisps y 300) whose cover follows
  `Conditions.clouds`. `BLOCKGAME_PAUSE=1` (or `=settings`) opens the pause menu for screenshots.
- Item sprites: done. `game::item_art` (`ItemArt`: every item's sprite from the three sheets in
  `assets/textures/icons/`, picked by kind: Tool -> weapons, Block -> blocks, else items; 20 px per
  cell; `image(id)` for UI, `model(id)` extrudes opaque pixels into a mesh once the PNGs load).
  `inventory::ui::icon` draws sprites at 2x (turned with `UiTransform` when the item is rotated) in
  grids, hotbar, worn slots and crafting rows. Held items and dropped non-blocks use the extruded
  sprite; blocks stay textured cubes. items.ron has no `model`/`color` any more; a test checks every
  item has a sprite of its size.
- Exposure: `survival::exposure` (stages in `assets/data/exposure.ron`: cold, heat, air; each drains
  Fatigue down to the mortal line, the last stage deals Damage). Air meter = oxygen where you stand
  (`Vitals.air`; `Vitals.oxygen` is what the body gets after skills or a held breath, `breath` under
  water); temperature meter = felt °C (`Vitals.feels`, follows surroundings, time constant `follow` in exposure.ron). Both always
  shown and shake by stage. `BLOCKGAME_FEELS=-25` / `BLOCKGAME_OXYGEN=50` force them for screenshots.
  `Survival.temperature` lists every part of the felt temperature (biome, season, height, sun,
  weather, wet, torches, clothes, skills, spell); the HUD shows it top right (F6 toggles).
  The gameplay loop and what to build next: `docs/GAMEPLAY_LOOP.md`.
- Critters: `game::creatures::critters`. `temper: Critter` kinds in creatures.ron with a `critter: (...)` block
  (motion Bird/Hopper/Drifter, count, ring, calls, hearing, height, hop, rest, shy, near_water, min_temp,
  fair_weather, to_light): marsh finches and grey kites circle in flocks, land, hop and call; reed frogs hop
  and croak near water; dusk crickets, fireflies (`glow: true` boxes blink) and lamp moths (circle lights)
  at night; bog darters over water by day. Boxes named `wing_*` flap. Not hittable, no drops. Calls are
  panned (`PlaySound::ambient_at`, camera has a `SpatialListener`) and follow the ambience volume.
  Critter sounds: `python3 tools/gen_placeholder_sounds.py --only bird_ frog_ ...`. Spirits are off
  for now (spawn weight 0 in creatures.ron).
- Phase 11 (atmosphere, wind and critters everywhere): in progress. Critters spawn in every biome for now
  (`biomes: []`) and only leave below freezing (`min_temp: 0.0`); frogs and darters still need water nearby.
  Atmosphere: `game::atmosphere` + `assets/data/atmosphere.ron` (grades by season, weather, biome and
  night: exposure, contrast, saturation, warmth, ground mist, season sunset colour). Terrain (voxel, water,
  LOD) fogs itself towards the sky's horizon colour in the view direction (`apply_atmosphere`, copied in
  `voxel.wesl` and `lod.wesl`; must match `sky.wesl` at the horizon; uniform `AtmosParams`, filled by
  `sky::update_sky`) plus height mist from `MIST_BASE`; Bevy's `DistanceFog` still covers other meshes.
  Camera is `Hdr` with soft `Bloom` (`BLOCKGAME_NO_BLOOM=1`) and TonyMcMapface (`BLOCKGAME_TONEMAP=agx|aces|..`
  to compare). Contrast is a power curve around mid grey (Bevy's linear `contrast` clips darks to black).
  Corner shading (AO) is baked by `voxel::mesher` (`Quad.ao`; quads merge only where shading stays right)
  and packed into UV_1.x's fraction (`chunk_mesh::pack_layer`). Sky: three-stop gradient, sun haze,
  sunset spread and anti-sun band, faint star band at night. Nights: dim blue ambient, lifted by the moon.
- Weather timing: each weather picks the next one when it starts (`WeatherState.next/until`);
  clouds change only in the last 30 s before a change (`weather::CLOUD_LEAD_SECONDS`), the rest
  blends in after. `game::weather_hud` shows weather, next and countdown in `hud::TopRight` (a
  column at the top right; spawn panels there with `ChildOf`). Low and middle cloud layers are
  opaque (blended ones flickered by view angle) and thin out at their edge. Table: `docs/WEATHER.md`
  and `docs/weather.csv` (update when weather.ron or seasons.ron change).
- Phase 11, wind and foliage: `game::wind` (`Wind` resource: strength eases to the weather's `wind`
  over ~20 s; direction wanders slowly by itself (`wind::wander`, under ~0.6°/s) and veers a little per
  weather, eased over ~2 min; gusts; the weather HUD title names where it comes from). Clouds drift along
  it at `Wind::cloud_drift` (square root of the wind, so gales are ~5 blocks/s, not 24+); rain, snow and
  wind push follow it too (`weather::WIND_DIR` is gone). Clear days: cloud cover 0.1 (cold snap 0.06) and
  a stricter cloud threshold, so a few puffs and thin wisps. Ground plants: `shape: plant` blocks
  (`tall_grass`, `reeds`, `fern`, `bog_bloom`) are two crossed quads meshed into a third per-chunk mesh
  (`ChunkMeshes.plants`, no shadows), scattered by `worldgen::plants` from each biome's `plants` list in
  biomes.ron (patch noise; `shore: true` = only by the water), removed with the block under them, and
  replaced when you place a block into them. Sway: `assets/shaders/voxel_vertex.wesl` (and
  `voxel_prepass.wesl` for shadows, same `sway_offset`) bends plant tops and rustles leaves by world position
  and time, from the voxel material's `wind` uniform; how far a vertex sways is packed into UV_1.y with the
  season tint (`chunk_mesh::pack_tint_sway`). New tiles: `python3 tools/gen_placeholder_art.py --only <names>`
  paints just those into the existing blocks.png.
- Tree details: `voxel::Shape::{Vine, Twig, Pebble}` (+ `Plant`) are small "detail" blocks (`Shape::is_detail`)
  meshed into the plants mesh by `chunk_mesh::push_detail`; vines and twigs have `attach: north|south|east|west`
  in blocks.ron (`vine_*`, `twig_*`), `BlockDef::supports()` says what holds each up, and `interact::break_block`
  drops whatever loses its support (a vine strand falls with the leaf above it). `worldgen::trees` adds twigs,
  canopy and trunk vines and leaf tufts; `worldgen::plants` scatters `pebble`s. Right click snaps a twig off
  (stick) or picks a pebble up (`small_rock`). Logs are material `log`: bare hands and the pickaxe can't cut
  them (a speed of 0 in items.ron = can't break), only the axe.
- Sleeping station: `game::station` (urn and stations as models around the bedroll, packed with it; `StationAim`
  makes `interact` leave right clicks to a station in front of the block you aim at). Pure rules in
  `station::fusion` (`assets/data/fusion.ron`: fusion kinds, what each item gives, upgrades with `needs`/`after`/
  `station`; upgrades spend fusion). Right click the urn with an item (Shift: whole stack); 10 earth places the
  tool bench, 40 more the carpentry bench. Saved to `urn.ron`. `BLOCKGAME_CAMP=1` unrolls the bedroll in front of
  you, `BLOCKGAME_URN=tool_bench,earth=5` sets upgrades and fusion (screenshots). Benches can't be worked yet.
- Arm: `hands` turns the arm about the shoulder (`hands::ARM`, `swing_pose`: wind-up, strike, return).
- Next: the sleeping station / fusion / hands-on stations plan (Claude Doc "Sleeping Station and Fusion: Plan"):
  trees with vines and twigs, pebbles, the ancestral urn, tool bench, carpentry bench. New characters will start
  with only the bedroll and clothes; the first axe is a stick plus two small rocks at the tool bench.

## Testing without a screen

`BLOCKGAME_SCREENSHOT=shot.png cargo run` waits for chunk loading to settle, saves a screenshot and quits.
`BLOCKGAME_CAMERA=x,y,z,yaw,pitch` (or `x,y,z,@tx,ty,tz` to look at a point) places the camera first.
`BLOCKGAME_TIME=day:hour` and `BLOCKGAME_WEATHER=rain` set the scene; the clock stands still during a screenshot run.
With `BLOCKGAME_CAMERA` the shot is taken flying (`BLOCKGAME_WALK=1` stands there instead); without it, from the player standing at spawn.
`BLOCKGAME_SLOT=n` starts with hotbar slot n in hand; `BLOCKGAME_SKILLS=n` gives n points and learns Wanderer and Mystic;
`BLOCKGAME_CAST=levitate,light` starts spells; `BLOCKGAME_SKILLTREE=1` opens the tree; `BLOCKGAME_LOOK=fen_stag` turns the camera to the nearest creature of a kind;
`BLOCKGAME_LINEUP=1` stands a still model of every creature skin in a row in front of the camera.
The home village's name and position are logged at startup.
In the cloud container: `BEVY_ASSET_ROOT=$PWD BLOCKGAME_SCREENSHOT=/tmp/s.png WGPU_BACKEND=vulkan xvfb-run -s "-screen 0 1280x720x24" ./target/debug/block-game`
(needs `mesa-vulkan-drivers xvfb libxkbcommon-x11-0`).
