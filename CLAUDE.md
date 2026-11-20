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
- Chunks are 32^3 (`voxel::CHUNK_SIZE`). Chunks stack vertically. The world floats (`worldgen::floating`):
  nothing generates below `worldgen::WORLD_FLOOR` (y -520, the void; falling past it kills) and there is a
  ceiling at y 1000 (`worldgen::WORLD_CEILING`: nothing generates or can be placed there, movement stops under it).
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
  hold LMB to mine, RMB places; left hand and weapon slot: see "Inventory redesign"), `game::hands`
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
  `interact` writes `Mined`/`OpenChest` messages (hand slots and tabs: see "Inventory redesign" below). Survival reads weight, clothes warmth
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
  kills, crafting, `places` finds and the first sky island; K opens the tree on the inventory's Abilities tab (`progress::ui`);
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
- Known gaps: vertices are 40 bytes, not the packed 8-byte format (performance pass); no snow cover yet; no block light propagation (torches are real point lights); creatures walk straight at goals (no pathfinding, villagers
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
  Leaving ones only fade beyond 16 blocks (`critters::FADE_AWAY`), never in front of you. Dusk crickets
  are 1.6x bigger and out day and night.
  Atmosphere: `game::atmosphere` + `assets/data/atmosphere.ron` (grades by season, weather, biome and
  night: exposure, contrast, saturation, warmth, ground mist, season sunset colour). Terrain (voxel, water,
  LOD) fogs itself towards the sky's horizon colour in the view direction (`apply_atmosphere`, copied in
  `voxel.wesl` and `lod.wesl`; must match `sky.wesl` at the horizon; uniform `AtmosParams`, filled by
  `sky::update_sky`) plus height mist from `MIST_BASE`; Bevy's `DistanceFog` still covers other meshes.
  Camera is `Hdr` with soft `Bloom` (`BLOCKGAME_NO_BLOOM=1`) and TonyMcMapface (`BLOCKGAME_TONEMAP=agx|aces|..`
  to compare). Contrast is a power curve around mid grey (Bevy's linear `contrast` clips darks to black).
  Corner shading (AO) is baked by `voxel::mesher` (`Quad.ao`; quads merge only where shading stays right)
  and packed into UV_1.x's fraction (`chunk_mesh::pack_layer`). Sky: three-stop gradient, sun haze,
  sunset spread and anti-sun band; the faint star band at night is off for now (`sky::MILKY_WAY`). Nights: dim blue ambient, lifted by the moon.
- Weather timing: each weather picks the next one when it starts (`WeatherState.next/until`);
  clouds change only in the last minute before a change (`weather::CLOUD_LEAD_SECONDS`), the rest
  blends in over a minute after. Snow has hysteresis (`SNOW_STOPS_ABOVE`); fog veils the sky dome
  (`sky::fog_veil`, `SkyParams.haze.z`). `game::weather_hud` shows weather, next and countdown in `hud::TopRight` (a
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
- Urn stations: `game::station`. Every station grows around its own urn (`assets/data/urns.ron`, pure rules in
  `station::urns`): each urn has a tree of steps in tiers 1-3; a step takes specific items (`needs`, put in a few at
  a time, `Progress.offer` fills open steps in tree order and passes leftovers on) and `gives` a `Station("kind")`
  placed by the urn, an `Upgrade` (code asks `Urns::has("step")`; models change), `Recipes` (benches.ron recipes
  with that `unlock`) or another `Urn("name")` as an item (`tool_urn`, `carpentry_urn`, `gear_urn`, `vehicle_urn`,
  `hearth_urn`, `star_urn`, `golem_urn`).
  `later: true` steps fill and place their model but their use isn't built yet. The ancestral urn (sleeping station)
  stands by the bedroll; carried urns are set down with right click on the ground and picked up by crouching and
  right clicking with an empty hand. Hold an item + right click an urn to put one in (Shift: all it wants); empty hand
  opens its tree (`station::tree`, "Put in" per step). Models are boxes in `station::models` (upgrades change them);
  concept sheets for real art in `docs/stations/` (`python3 tools/gen_station_concepts.py`). Personal storage is a
  chest at `inventory::STORAGE_AT` (not in the world; 8x5, 8x7 with the extra shelf). Saved to `urns.ron` (the old
  fusion `urn.ron` is gone). F9 = dev menu (`station::dev`: give/set down urns, items for the next step, finish
  steps, reset). Screenshots: `BLOCKGAME_CAMP=1` unrolls the bedroll in front of you, `BLOCKGAME_URNS=tool,hearth`
  sets urns down in a row ahead, `BLOCKGAME_URN=camp_mat,tool_bench` finishes steps, `BLOCKGAME_URNTREE=tool`
  opens a tree, `BLOCKGAME_DEVMENU=1` the dev menu. The planning sheet is the Claude Sheet "Station Urn Trees".
- Hands-on benches: `station::board` (no menu: right click a bench and the camera glides down over its top,
  the HUD and arms step aside (`board_hides`), a strip of your things sits at the bottom; click one to pick it up,
  it hangs under the cursor; click a slot to set it down; knock the stone to work it; A/D next bench, Esc
  leaves and the board empties back into your pockets). Pure rules in `station::bench` (`Board`, slots per bench in
  `slots_of`, where things lie in `board::layout`, recipes in `assets/data/benches.ron`: slots, `work` goes, `makes`,
  `unlock`; the recipe using the most wins; `BenchData.learned` = urn steps done). Camp mat (sleeping station):
  base + tie, knapping stone (2 small rocks + stick = `rough_axe`, the first axe; portable bedroll and tent once
  taught). Tool bench: handle + material (stick + 3 small rocks = stone pickaxe, cobblestone tools; stone axe and
  flint spear after the improved tool bench). Carpentry bench: bed + vise, saw (logs -> planks, planks -> sticks,
  chest). Carpentry table (tool urn, after the carpentry bench): top + joint tray, mallet; building supplies
  (`floorboards`, `timber_wall`, `wattle_wall`, `thatch`: plain cube blocks). recipes.ron (Tab) keeps only what has no station yet. `Menus.still` stops walking at a bench.
  `BLOCKGAME_BOARD=stick,small_rock*2[,made]` (or `carpentry:log`, `mat:small_rock*2,stick`) opens a bench with
  those laid out (`-` skips a slot; the bench must stand, see above).
- Chopping: an axe blow (`swing: Chop`) that lands on a log sends `chop::Chop`, which shakes the
  tree through the voxel material's `shake` uniform (`shake_offset` in voxel_vertex.wesl and voxel_prepass.wesl).
  New characters start with only their clothes (`start_kit.ron`); the bedroll is always there (B).
- Tool swings: `game::swing` (pure): items have `swing: Jab|Chop|Overhead` in items.ron (axe chops diagonally
  from the upper right, pick comes straight down). A swing is keyframed poses (wind up, strike, bounce, rest);
  mining lands one blow per swing at `Swing::contact` (`interact::Blow`, which plays the dig knock), each worth
  `period / break seconds`, so cracks and sound line up with the moment the tool meets the block.
- Title screen: `game::menu` (Continue the last-played world, New world with a name and a seed
  (numbers as is, words hashed, blank = random), Load world, Quit). Each world is `saves/<folder>/` with
  `world.ron` (name, seed, created, last_played; `save::list_worlds`/`create_world`); `world::WorldSeed`
  replaces the fixed `SEED` (kept for folders without a seed). Nothing of the game runs before a world
  opens: plugins add systems to `menu::GamePreStartup`/`GameStartup`/`GamePostStartup`/`GameUpdate`
  instead of Bevy's `Startup`/`Update` (entering `Screen::Game` runs the startup three once, then
  `GameUpdate` runs inside `Update`); systems in `First`/`PreUpdate`/`Last` need `.run_if(menu::in_game)`.
  "Save and quit to title" (pause menu) exits through the usual save-on-exit path, then `menu::relaunch_to_title`
  starts the game again on the title screen (a fresh process, so no state is left over).
- Every station works now (no `later: true` steps left). Boards (`station::board`, rules in `station::bench`)
  also cover the cooking pot (`SlotKind::Mix` slots fill in any order, recipes list a `mix`; a `Fire` slot burns
  `fuel` from benches.ron for a recipe's `heat`; stirring swirls the pot), cutting board, smoking rack, iron anvil
  (steel/alchemical anvil steps teach steel and spirit tools), sharpening stone (any tool: `Special::Sharpen`,
  `station::Edges` speeds mining, saved to `edges.ron`), magical lens (anything: `Special::Study` shows what it's
  made of and adds `lens:item` to the learned set; `Urns.studied`) and cauldron (draughts, unlocked by
  `lens:item`, `a+b` unlocks need both; drinking one (`survival::Ate`, `assets/data/potions.ron`, food tier 0)
  runs a spell free via `Spells.brewed` or gives focus). Herb rack teaches seasoned pot recipes. Stone fridge and
  the golem's pack are chests not in the world (`inventory::virtual_chest`). Bedframe/double bed: `camp::Bed::rest`.
  Telescope (`station::telescope`, `assets/data/constellations.ron`): on a clear night, look through it (zoomed
  view, gizmo stars) and hold a constellation in the middle to chart it (`Urns.charted`, XP); improved telescope /
  diamond lens show tier 2 / 3. Golem (`station::golem`): `golem_heart` wakes it (a walking `StationKind::Golem`,
  right click follow/stay), `golem_hands` gives it a pack (crouch + right click) and it strikes hostile creatures
  near it; saved to `golem.ron`. Cooking moved out of the Tab list (roast meat, peat stew).
  `BLOCKGAME_BOARD=<station name>:...` opens any bench for screenshots (`pot:`, `cut:` short forms).
- Bench tools are their own models (`models::tool`, `board::BenchTool` children of the station): click the tool
  to lift it, it follows the cursor; click to swing it down (hammerstones, anvil hammer, knife, fan; one go when it
  lands, `board::swing_pose`; the tool bench's hammer hangs on a rack, the knife drops edge-down), hold the button and
  stir round the pot (ladle, paddle: a full turn = one stir) or saw back and forth (`board::Gesture`, `SAW_STROKE`). The sharpening wheel and the lens's
  focusing dial (now on a stand beside the plate) turn in place when clicked. Benches can have several views
  (`Layout.views`: pot and fire, cauldron and fire, rack and fire): W / S or the buttons under the title. The cooking
  pot and cauldron are open with see-through broth/brew (`Mat::alpha`, `models::liquid`). What's in them floats upright
  (`station::float`, pure: the ladle pushes things, they bump each other and the walls, tip when shoved and right
  themselves). `station::particles`: pixel-art camera-facing sprites (bubbles that pop, and a `Burst` message of chips,
  sparks, sawdust, bits, smoke, splashes, sparkles per go, `board::burst_of`). `station::forge`: when a recipe makes
  something, the parts used fly together with a holographic glint (`GlintMaterial`, `assets/shaders/glint.wesl`)
  flashing stronger, then pop (`craft_shimmer`/`craft_pop` sounds) into the made thing. `station::book`: a floating
  recipe book at the left shows each recipe as pictures. The strip of your things pages (arrows or wheel), usable
  first. The sharpening stone lays tools head to the wheel (`Layout.flip`). The telescope view is a round eyepiece
  with cross hairs (`telescope::lens_image`) and hides the HUD. Screenshots: `BLOCKGAME_BOARDVIEW=1` (second view),
  `BLOCKGAME_BOARDTOOL=lift|down|0.3`, `BLOCKGAME_SCOPE=1`, `BLOCKGAME_FORGE=0.8` (works the board and holds the
  craft coming together that many seconds in).
- NPCs: `game::npc` (kinds in `assets/data/npcs.ron`: name, title, looks colours, voice pitch/rate, walk speed,
  dialogue tree key). Ctrl+Shift+N (Cmd+Shift+N) spawns the first kind ahead of you, following you. Pure parts:
  `npc::motion` (critically damped `Spring` on every joint, `walk`/`idle`/`gesture` poses, `Gaze`: head on a slow
  spring, pupils lead on a fast one, idle glances, blinks), `npc::voice` (`speak`: text -> timed syllables; mouth,
  voice sounds `voice_0..3` and the typed subtitle all follow it), `npc::dialogue` (trees in `assets/data/dialogue.ron`,
  `goto: "node"` works without `Some`; flags, `needs`/`without`, `once`, tone tags, `again` opening, acts
  Follow/Stay/Wander). `npc::body`: jointed box model (hips, knees, torso, shoulders, elbows, neck, head) with eye
  whites, iris + pupil that move, blinking lids, mood brows and a rectangular mouth with upper and lower teeth;
  animates on real time. `npc::talk`: look at an NPC and press E: black bars, `Time<Virtual>` paused (so autoshot
  uses real time), HUD and arms hidden, camera eases to frame the speaker; click/Space/E/Enter continue, replies by
  mouse, 1-8 or W/S + Enter, Esc leaves. Not saved yet; no combat yet (room left in `npc::Order`).
  Screenshots: `BLOCKGAME_NPC=3` spawns one 3 blocks ahead, `BLOCKGAME_TALK=1` talks to it (`=replies` jumps to the replies).
- Rooms: `voxel::room::find_room` (pure) fills outward from a cell through open cells; walls (`BlockDef::is_wall`:
  solid full cubes that aren't `plant` material; leaves, plants, water, torches are holes) stop it. Closed within
  `MAX_ROOM` (4096) cells = a `Room` (`cells`, `shell`); otherwise `NotRoom::Open`/`InWall`/`Unknown` (unloaded).
  `station::room_at` runs it on the world. A carried urn can only be set down inside a closed room with no other
  urn in it (the ancestral urn by the bedroll is exempt but takes up its room). Insulation tiers, doors and the
  shelter panel from the house plan aren't built yet.
- Floating world: `worldgen::floating`. The land is one island, `MAIN_RADIUS` 2000 blocks (about 2.5 miles across,
  coastline wobbles with `coast` noise), centred on spawn (`WorldGen::island_center`); the normal terrain is cut to it
  and its rim is lifted above sea level so seas stay inside. Underside: a rounded bowl ~300 blocks deep at the
  middle, hung with rounded lobes and stalactite drips (`underside`). Outer islands (jittered 520-block grid,
  radius 50-260, tops at y -140..240) float past the coast with the same climate, biomes and underside, and no
  water. `Column` has `land` (Void/Main/Outer), `bottom` and `water`; villages and barrows are main island only.
  Rootstone no longer generates. `game::lod` draws undersides and leaves the void empty.
- Machination table (vehicle urn, tier 1): `StationKind::MachinationTable`, a board like the
  other benches (slots keel / rigging / envelope / engine, the drive wheel is a `ToolUse::Turn` tool). It builds
  machines: items of kind `Machine("name")` in items.ron, numbers in `assets/data/machines.ron`. Only the airship so
  far (8 planks, 6 cordage, 4 hide, 6 raw iron). `game::airship`: right click open ground with it to set it down
  (bow the way you look; needs ~4 x 9 x 7 clear), right click or E to board onto the deck (`Airships.deck`: ship and
  spot in its frame; `walk_deck` moves you with WASD inside the rails, `DECK_MIN`/`DECK_MAX`), right click within 1.4
  blocks of the helm to take it (`Airships.aboard`): W/S ahead/astern, A/D turn, Space/Ctrl rise/sink, E lets go back
  onto the deck; on deck E steps off over the nearest rail (`over_rail`). Pure flight rules in `airship::flight`
  (eased speed, oriented footprint, stops on solid blocks one axis at a time). At the helm or on deck `player::walk` is
  off (`airship::not_aboard`). Crouch + right click a ship with an empty hand (not aboard) picks it back up as the
  `airship` item. Ships, the helm and the deck spot save to `airships.ron`.
- Urn split (Legacy prompts 2-4): the tool urn keeps only the tool bench and the anvils. Carpentry urn
  (`carpentry_urn`): carpentry bench, carpentry table, Joiner's Table (`chair`, `table`), tier 2 Door Frame
  (`wooden_door`) and Glass Kiln (`glass`, `window`; sand and glass need heat). Vehicle urn (`vehicle_urn`): the
  machination table. Gear urn (`gear_urn`): Sewing Table (the clothes that were on the Tab list), Armor Stand (bark
  and hide armor; iron helm and mail after the tier 3 `metal_armor` step), tier 2 Bladesmith Bench (`iron_dagger`,
  `iron_sword`, moved off the iron anvil; `rapier` after the `rapier` step) and the Sharpening Stone, tier 3 Gunsmith
  Bench (`makeshift_pistol`, `rifle`, `iron_shot`; guns fire, see `combat::guns`). The ancestral urn gives the three new
  urns (carpentry after the tool urn, gear and vehicle after carpentry). New benches reuse an older bench's board
  (`StationKind::base`: layout, slots, tool, size) and add a few parts of their own in `models::parts`. Chairs,
  tables and doors are box models (see Furniture below); glass and windows are see-through blocks.
- Warded dungeons: `worldgen::dungeons` (pure). One per 1024-block cell (chance 0.7, 700+ blocks from spawn) plus a home
  dungeon 170-290 blocks from spawn, away from the home village. A stone gatehouse with one doorway, a stair 20-27 down,
  then 7-10 large rooms (13-23 wide, 6-9 high) on a 30-block grid in the dungeon's own frame, joined by 3-wide corridors
  (a few loops); the furthest room is the warden's hall (25x25, boss). `Dungeon` has `rooms`, `links`, `spaces` (open
  boxes past the veil: `inside(p)`), `spawns` (`SpawnPoint` pos/kind Guard|Pack|Boss/room: hooks for enemies, nothing
  spawns them yet), `bounds`, `outside` (path spot), `guide`, `veil`. Spirit ward: `WorldGen::warded(x, y, z)` covers
  dungeons, barrows and villages; `interact` won't break, place or pick in them ("A spirit ward holds this place").
  `game::dungeon`: `Nearby` dungeons, a shimmering veil in each doorway, a guide NPC (`warden` kind in npcs.ron and
  dialogue.ron, `Npc.post` = dungeon id) by each path. Dialogue `act: Spirit` (`Talk::take_cast`) starts
  `dungeon::spirit`: a 5.6 s cinematic (bars, view tips down, two arms rise and drain to see-through glowing blue with
  sparkles, turn over, drop). As a spirit: arm materials tinted blue, faint blue screen wash, health starts full and the
  body's health waits (`Spirit.body`). Non-spirits inside a dungeon are put back on the path. A spirit that falls is cast
  out to the path with its body's health back (no death); walking out 14+ blocks from the gate ends the spirit form.
  `EnteredRoom` (first time per run) and `LeftDungeon { fell }` drive `dungeon::foes`: entering a room spawns the foes
  (`combat::SpawnFoe`) of it and the rooms its corridors lead to, from the dungeon's spawn points (guards hollows, packs
  mixed hollows and shades, boss a shade: `foes::kind_for`); the run ending despawns every foe whose home is inside that
  dungeon.
  Screenshots: `BLOCKGAME_DUNGEON=1` starts on the home dungeon's path facing the door (its position is logged),
  `BLOCKGAME_SPIRIT=cast` runs the cinematic, `=1` starts as a spirit.
- Player body and third person: `game::avatar` builds your body with `npc::body::build` (the NPC rig; looks in
  `assets/data/player.ron`, also used for the first-person arm) and moves it with `npc::body::drive` (shared with NPCs).
  P toggles `camera::View` third person: the camera is pulled back 4 blocks (less if a wall is in the way) only for
  drawing (`PostUpdate`), and put back at the eyes in `PreUpdate`, so aiming and every other system still use the eyes.
  Off at benches and in talks (`Menus.still`). The body's right fist (`Rig.hands`) carries a `hands::Held`, so it holds
  what you hold. Swings: `swing::Swing::limbs` gives the body's arms on the same keys as the first-person `pose`
  (axe = flat horizontal swing with a bite and hold; pick = high overhead, straight down). `BLOCKGAME_VIEW=third` for screenshots.
  Movement feel: the walk cycle advances by distance over `motion::stride(pace)` (about two unhurried strides a second
  at walking speed) and goes on top of the sprung stance unfiltered; no up-and-down bob (Ethan found it too much); hips
  sway sideways over the planted foot (`Pose.shift`), the chest's twist lags the hips (`Rig.chest` springs), arms
  and forearms trail the legs; the body turns Minecraft-style while moving
  (`motion::moving_facing`: toward the way you go, at most 75° from the head, forwards when backing up; standing it
  turns once the head is 50° off) and leans into speeding up, stopping and turns (`Drive.sway`, from `avatar`).
- Combat: `game::combat`. Swords (`swing: Slash` in items.ron: `iron_sword`, `steel_sword`, `spirit_sword`, made at
  the iron anvil) cut on left click and never mine: the cut lands at `Swing::contact` as `interact::Swung`, hitting every
  foe in a 55° sweep within `SWORD_REACH` (and the creature you aim at). Hold right click to block (`combat::Guard`,
  `swing::guard_pose` / `Limbs::guard`); a block raised under `ai::PARRY_WINDOW` before a cut lands parries it (the foe
  reels). Q shoves every foe close in front back and off balance (2 s cooldown). Foes: kinds in `assets/data/foes.ron`
  (NPC rig looks, weapon item held via `hands::Held::holding`, damage, reach, swing time, sight/forget/leash, style:
  aggression, caution, guard, poise). Pure mind in `combat::ai` (`Brain`: Idle, Return, Approach, Circle, Press, Attack,
  Recover, Guard, Backoff, Stagger; at most `PRESSERS` press you at once, the rest circle; gives up past `forget` or
  `leash` from home and walks back; guards when it sees your cut coming, guard breaks after `poise` blocks; hits
  interrupt its swing; fury when hurt). Pure A* in `combat::path` (steps up one block, drops up to 3, no corner cutting,
  budgeted; goes as near as it can when there's no way). Ghosts (`ghost: true`): own materials made blue and see-through,
  shimmering per part, a faint light, `BurstKind::Wisp` particles trailing (and a burst on death). Sound `clash`.
  Dev keys: Ctrl+Shift+J (Cmd on a Mac) spawns a Hollow Warden, Ctrl+Shift+H a Fen Shade, 6 blocks ahead; either gives
  you an iron sword on the hotbar if you have no sword. Foes don't despawn.
- Rotting Gate (Legacy prompts 5-7): `worldgen::rotting` (pure). The gate is a mossy cobblestone arch placed once on the
  main island 300-600 blocks from spawn (`WorldGen::rotting_gate`, warded, position logged). `rotting_floor(seed, run, depth)`
  lays out a fresh floor per run on a 28-block grid: start room (way out in its west wall), halls, boss room with the stair;
  `DEPTHS` = 4, each deeper floor has more rooms and spawns. Floors are built in the pocket (`POCKET`, x/z 60000, y 780,
  `in_pocket`): `world::PocketFloor` makes chunk generation use `generate_pocket` there, pocket chunks are never saved and
  `World::forget_pocket` drops them when the floor changes; no height cold or thin air there, no creatures or critters.
  `game::delve`: walk into the gate to start a run at the deepest unlocked depth; rooms spawn foes as you reach them
  (`combat::SpawnFoe` with `boost` per depth and a `tag`; each depth's boss is its own kind, see below); the boss's
  `FoeSlain` opens the stair; the stair goes down a depth (after depth 4 it brings you back up); the start room's west
  doorway leads back to the gate. Pure rules in `delve::rules` (`Delve { runs, deepest, cleared }`, saved to `delve.ron`;
  clearing a depth's boss lets the next run start one deeper; `lost_on_death`). Dying below loses what you picked up or put
  on during the run and wakes you outside the gate (`survival::RespawnAt`). Dungeon foes in foes.ron: `sporecaster`
  (ranged: `ranged: Some((speed, color))` shoots bolts a raised guard blocks), `rot_brute` (slow, heavy), `mire_skitter`
  (fast). Place name banners are off (`places::PLACE_CARDS`). Screenshots: `BLOCKGAME_GATE=1` starts facing the gate,
  `BLOCKGAME_DELVE=n` starts a run at depth n.
- Boss urns and the cast (Legacy prompts 8-9): each Rotting Gate depth has its own boss in foes.ron (`bog_warden`,
  `spore_matron` (ranged), `gloam_stalker` (fast), `rotting_king`; `delve::rules::boss_for`). The first time each falls it
  drops its urn (depth 1 carpentry, 2 gear, 3 hearth, 4 vehicle; `Delve.urns`, once per world); the ancestral urn no longer
  gives those four. Foes below roll loot.ron `rotting`, bosses `rotting_warden` (`Loot::roll`): dungeon-only `mold_bloom`
  (tier 2 urn steps need 3), `deepstone_shard` and `warden_sigil` (tier 3 steps need 4 + 1). The cast in npcs.ron and
  dialogue.ron: Bram Holloway, Gilm, Wren Calloway, Liora Vance, Messler, Orrin Vale, Brindlespheimer. NPC looks have
  `scale` (Gilm and Messler 0.55 = one block tall), `extras` (boxes on Head/Torso/Hips: caps, ears, hats, robes) and
  `blank` (no expression). Kinds with `urn: Some(..)` are recruited by asking them to follow and leading them within
  `npc::SETTLE` (5) blocks of that urn: they wander round it from then on (flag `recruited`). `npc::people` saves every
  NPC but the gate guides to `people.ron` (order, flags, place). Nothing places the cast in the world yet: Ctrl+Shift+N
  spawns the next kind each press; `BLOCKGAME_NPC=3 BLOCKGAME_NPC_KIND=gilm` for screenshots.
- Landmarks and cast services (Legacy prompts 10-12): `worldgen::landmarks` (pure, once per world): Sporrow, a village of
  giant mushroom houses (`mushroom_stem`/`mushroom_cap` blocks) 60-110 blocks from the Rotting Gate, a `Village` with
  `folk: Folk::Mushroom` (so `villages_in`, warding and villagers include it; its villagers are the `mushroom_folk` creature,
  wary lines in villagers.ron `mushroom`); Gilm's stone hut with a forge just outside it (`gilm_forge`); the castle
  (`castle`): a walled court on a plinth on the highest level-enough mountain ground, a 7x7 tower 20 high with a stair round
  the inside coming out on the roof. Positions are logged at startup. `game::cast`: Brindlespheimer stands by the gate
  (`GATE_POST`, not saved); Gilm (at his hut) and Orrin (on the tower roof) are placed once per world, the Star Urn is set
  on the tower once (`urns.placed["star"]`; the ancestral urn no longer gives it); `cast.ron` remembers what was placed.
  `cast::services` (`assets/data/services.ron`, `trades.ron`): dialogue replies with `service:` are done between picking and
  the next node (`npc::talk::{TalkInput, TalkRun}`, `Talk::take_service`/`go_to`, `{key}` vars, `Talk.extra` flags):
  Orrin `weather` (WeatherState next/until), Messler `feed` (teaches the next `messler_teaches` recipe: `Urns.taught`, merged
  into the learned set) and remarks on `board::Cooked` (made/failed/burnt), Gilm and Liora `commission:<n>` (Gilm only what
  you can't make yet: `knows:<item>`), Wren `trade:<id>` (`stock:<id>` per day). `cast::wren`: on even days Wren lands a small
  flying machine by the vehicle urn and stays (`WREN_POST`, not saved); the next day she flies off.
- Voice lines, cutscenes, encounters (Legacy prompts 13-15): `assets/data/lines.ron` holds every voiced line (id, speaker,
  text, `trigger` for barks, `direction`); dialogue.ron `say` lists line ids. `game::lines`: a line plays
  `assets/voice/<id>.ogg` if it exists, else the `npc::voice` babble; the subtitle always shows. `lines::Bark { speaker,
  trigger, at }` says a random bark (Messler's cooking remarks use it). `game::cutscene` (`assets/data/cutscenes.ron`; pure
  runner and triggers in `cutscene::script`): triggers Near (NPC + line of sight), Area (place: camp, gate, sporrow, castle,
  forge), Cleared (encounter), Boss (depth), Made (bench item, `board::Crafted`), Placed (block), TimeAfter (flag + hours);
  actors are NPC dialogue keys, brought in (spawned, placed or walked to their mark via `Npc.cue`/`face`) before the steps:
  Say, Camera, CameraMove, Walk, Face, Wait, Fade, Encounter, Flag, Clue, Give, Swap, Place. Draws through the talk's bars
  (`Talk.scene`), locks input, hides the HUD, world keeps running; click ends a line, hold Esc skips (lasting steps still
  apply). Once per world: `Story { flags, clues, played, made, placed }` saved to `story.ron`. Authored: C02 (Bram, no
  trigger), C03 (near Brindlespheimer), C15 (airship made; Wren and Liora). `game::encounter` (`assets/data/encounters.ron`,
  pure rules in `encounter::rules`): foes + counts, `from` Around(r) / Place(name, r) / Airship(r) (a red drop ship comes
  down, they drop), `then` flag / cutscene. Started by a cutscene's Encounter step or `StartEncounter`; dying resets it
  (foes go, all return when you're back within 48 blocks); the one running saves to `encounter.ron` and starts over on load.
  Screenshots: `BLOCKGAME_CUTSCENE=C02`, `BLOCKGAME_ENCOUNTER=bog_ambush`.
- Story beats, journal, the raid (Legacy prompts 16-17): `game::story` (`assets/data/story.ron`; pure rules in
  `story::beats`): a beat lists conditions (NewWorld, Day, Hours, AtCamp, Urn, Step, Depth, Met, Recruited, Flag, Played,
  DayAfter(`flag:`/`played:`/`step:` key, dates in `Story.when`), Near, Area, Made, Placed) and starts one cutscene; it fires
  once (when that cutscene has played). C01-C16 are all wired (C06b and C08b are the second halves of C06 and C08).
  `cutscene::Story` also keeps `met`, `when`, clues in order found, and `edits` (Swap/Place steps wait there until their
  area is loaded: `apply_edits`). New cutscene steps `Follow(actor)` and `Remove(actor)`. Cutscene start requests queue
  while one plays (`Playing::busy`). `cutscene::Places` (camp = bedroll, else the ancestral urn; gate, sporrow, castle,
  forge) is shared with encounters (`Place(name, r)` now spawns at the place). Prisoners: Liora waits in a hall on depth
  2 and Messler in the depth 3 boss room (`story::PRISON_POST`, gone with the floor) until C08/C09 set `liora_found`/
  `messler_found` and they follow you. Flags from the game: `fed_messler` (Messler's feed service), `used_telescope`.
  J opens the journal (clue texts in story.ron `clues`; `BLOCKGAME_JOURNAL=1`). New NPC kinds `sporrow_elder` and `aunt`. The raid: C11 (night after C10, at camp) ends in encounter `raid_camp`
  (5 `court_soldier`s from an airship: rifle bolts at range, a cut within `close`, the new foes.ron field), then C12
  removes Bram, places a `grave` block at camp and swaps Sporrow's mushroom blocks for `burnt_stem`/`burnt_cap`.
- The aunt's island: `worldgen::landmarks::AuntIsland` (`WorldGen::aunt_island`, once per world): a sky island of its
  own (radius 34, base `SKY_MIN` + 40, so above every cloud layer) 700-1300 blocks from spawn over the main island, kept
  clear of the grid's sky islands; `islands_near` includes it, so `island_top`/`island_span`, the LOD and the island XP
  see it. Drawn in `islands::place` (`draw_aunt_island`): a 7x7 stone cottage with a plank roof, a hearth (torch) under
  a chimney on the back wall, a stone path with steps from the door to a broken arch (one pillar whole with a stub of
  the span, the other snapped low, fallen stones). No trees round them (`keeps_clear`). `game::cast` places the aunt
  outside her door once (`cast.ron`), so the C16 beat can fire; the position is logged at startup. No smoke effect yet.
- Inventory redesign: `inventory::ui` is one window with folder tabs (Items, Equipment, Stats, Abilities; submodules
  `ui/equipment.rs`, `ui/stats.rs`, `ui/abilities.rs`); `InvScreen { open, tab }` says which is up. Items: the pockets
  grid with the weapon and hand slots over it, side panel Crafting / Chest. Equipment: a live picture of you
  (`inventory::preview`: a second `npc::body` rig on render layer 3 filmed into `PreviewImage`; idles, looks towards the
  cursor, drag turns it, shrugs when gear changes) with armor slots left, clothes (and the Back/bag slot) right, weapon and
  hands under it, and a strip of carried gear. `preview::Dressed` bodies (the picture and your third-person body) wear a
  box per worn piece in the colour of its sprite. Stats: cards with bars (health, body, gear, temperature, progress, skill
  totals). Abilities: spell cards, and the skill tree drawn over `ui::TreeArea` (`progress::ui` follows the tab; K opens
  the inventory on this tab; the tree no longer has its own screen). Hands: `Inventory.left` is a real left-hand slot (any
  stack, out of the pockets, saved in inventory.ron); `Inventory.weapon` holds one `Item::is_weapon` (tool with no mining
  speeds); `interact::Hands { right, drawn }`: R (`interact::DRAW_KEY`) draws the weapon into the right hand, R or 1-9
  puts it away. `interact::OffHand` mirrors the left hand; right click uses the right item if it has a right-click use,
  else the left (`interact::right_click_hand`: place, eat); a torch in either hand lights and warms (`holding_torch`).
  Both first-person arms exist (`hands`); the left shows only while holding something. Look: new sprites in ui.png
  (`tab`, `tab_active`, `tab_hover`, `slot_glow`, `card`, `plinth`, `sil_*` empty-slot silhouettes, `icon_bag/figure/star/sword`;
  the sheet is now 256x192), slots that take what you hold glow and others dim, drag and drop, window scales in, tabs
  slide, slots lift and settle (`ui::animate`), UI sounds `ui_*` (`gen_placeholder_sounds.py --only ui_`).
  Screenshots: `BLOCKGAME_INVENTORY=items|equipment|stats|abilities`, `BLOCKGAME_SKILLTREE=1` (= abilities),
  `BLOCKGAME_DRAWN=1` starts with the weapon drawn.
- Saving what's in the world: `save::{read_ron, write_ron, screenshot_run}`. Items on the ground (`ground_items.ron`, with
  their age), wild creatures and when barrows woke (`creatures.ron`; not villagers or critters), foes (`foes.ron`; not
  tagged encounter/Rotting Gate foes or a dungeon run's foes, which start over), spirit form (`spirit.ron`: dungeon id and
  the body's waiting health; the run's rooms refill), and what posted NPCs (gate guides, Brindlespheimer, Wren) remember
  of your talks (`posts.ron`, by `Npc.post`, given back when they appear).
- Shelter and campfire: `survival::shelter` (pure). `survival::find_cover` checks once a second: a wall block over your
  head = `Cover::Roof` (half the weather's chill kept off), and if `station::room_at` finds a closed room where you stand,
  `Cover::Room` (all the weather's chill and 60% of the night's cold). Shows as "Shelter" in the temperature breakdown.
  Campfire: block `campfire` (`shape: torch`, so `torches` tracks it: `Torches::fires()` gives `Fire::Torch|Campfire`;
  drawn as a stone ring, crossed sticks, a big flame and a brighter light), made on the camp mat (4 small rocks + 4 sticks);
  warms up to +24 °C close by, fading out at 10 blocks (`CAMPFIRE_MAX` 28 all told).
- Map and waypoints: `game::map`. M opens a top-down map 2048 blocks across (`map::paint`, pure, on the
  `AsyncComputeTaskPool`; biome colour shaded by height, blue water, dark void), repainted when opened over 512 blocks
  from where it was painted. Red mark = you, gold = waypoints. On the map: click adds a waypoint, Space marks where you
  stand, right click removes one. Waypoints within 60° of where you look show along the top of the screen with distance.
  Saved to `waypoints.ron`. `BLOCKGAME_MAP=1` opens it for screenshots.
- Farming: `game::farming` + `assets/data/crops.ron` (stages = plant blocks, `days` per stage, `wet` speed-up with water
  within 4 blocks). Seeds are block items for stage 0 (`barley_seeds` from tall grass, `tuber_eye` from ferns); `interact`
  only lets `shape: plant` blocks go on soil. `Fields` (saved to `crops.ron`) remembers each sown crop; every 2 s loaded
  crops are set to the stage they've reached. Ripe barley drops `barley` (3 make `bread` in the cooking pot), ripe tubers
  `mire_tuber` (food). Tiles `barley_0..2`, `tuber_0..2` in `gen_placeholder_art.py`.
- Music and thunder: `sound::music` plays one track now and then (sounds.ron `music`: `day`/`night` lists by
  `brain::spirit_hours`, `gap` seconds of quiet between, `first` before the first), scaled by the new Music setting.
  Placeholder tracks `music_day_0/1`, `music_night_0` from `gen_placeholder_sounds.py --only music_`. Thunder follows each
  lightning flash after 0.3-7 s (`thunder_delay`), quieter and deeper the later it comes. Voice lines still need recordings
  (`assets/voice/<line id>.ogg`; `lines` plays them when present).
- Guns: `combat::guns`. Items with `gun: (damage, range, reload, ammo)` in items.ron (`Item::gun`; they are
  weapon-slot tools) fire on left click from the right hand: one `ammo` (`iron_shot`, 8 from 1 raw iron + 1 coal at the
  gunsmith bench) per shot, `reload` seconds between, a straight shot from the eyes (`ray_hits_body`) hitting the nearest
  foe or creature before any solid block. Sound `gunshot`, smoke at the muzzle, chips where it hits a wall. `interact`
  never mines or strikes with a gun.
- Furniture and doors: `voxel::Shape::Model` blocks with `model: chair|table|door|door_open` are meshed as a few
  boxes into the chunk's detail mesh (`chunk_mesh::model_boxes`, textured with the block's side tile); facing comes from
  wall blocks beside them (a door spans the gap between two walls; a chair's back goes against a wall). They collide as a
  whole block if `solid`; `is_wall` counts solid models, so a shut door closes a room. Right click a door to swap
  `wooden_door` <-> `wooden_door_open` (`interact::door_swap`), with the doors stacked above and below it; it won't shut
  on you.
- Away camps and twigs: hold a `portable_bedroll` or `portable_tent` and right click the ground to lay it out on a clear
  2x2 patch (`camp::Away`, `Camp.away`, one at a time, saved in player.ron); Z by it sleeps there (no home-bed extras; a
  tent wakes you warm); inside a pitched tent `find_cover` gives `Cover::Room`; crouch + empty hand right click packs it
  up. Home camp and respawn don't move. Snapped twigs grow back after `farming::regrow::REGROW_DAYS` (2) game days if the
  cell is empty and the trunk still stands (`twigs.ron`).
- Next: retire the rest of the Tab crafting list.

## Testing without a screen

Screenshot and bench runs skip the title screen and open `saves/world1` (`BLOCKGAME_WORLD=<folder>` picks
another, and also skips it in normal runs); `BLOCKGAME_MENU=1|new|load` keeps the title screen up for a screenshot.
`BLOCKGAME_SCREENSHOT=shot.png cargo run` waits for chunk loading to settle, saves a screenshot and quits.
`BLOCKGAME_CAMERA=x,y,z,yaw,pitch` (or `x,y,z,@tx,ty,tz` to look at a point) places the camera first.
`BLOCKGAME_TIME=day:hour` and `BLOCKGAME_WEATHER=rain` set the scene; the clock stands still during a screenshot run.
With `BLOCKGAME_CAMERA` the shot is taken flying (`BLOCKGAME_WALK=1` stands there instead); without it, from the player standing at spawn.
`BLOCKGAME_SLOT=n` starts with hotbar slot n in hand; `BLOCKGAME_SKILLS=n` gives n points and learns Wanderer and Mystic;
`BLOCKGAME_CAST=levitate,light` starts spells; `BLOCKGAME_SKILLTREE=1` opens the tree; `BLOCKGAME_LOOK=fen_stag` turns the camera to the nearest creature of a kind;
`BLOCKGAME_SWING=0.55` freezes the right arm that far through a swing; `BLOCKGAME_LINEUP=1` stands a still model of every creature skin in a row in front of the camera.
The home village's name and position are logged at startup.
In the cloud container: `BEVY_ASSET_ROOT=$PWD BLOCKGAME_SCREENSHOT=/tmp/s.png WGPU_BACKEND=vulkan xvfb-run -s "-screen 0 1280x720x24" ./target/debug/block-game`
(needs `mesa-vulkan-drivers xvfb libxkbcommon-x11-0`).
