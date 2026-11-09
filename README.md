# Block Game

A voxel survival game in Rust and Bevy. Working title.

## Run it

1. Install Rust: https://rustup.rs (then `rustup update` - Bevy 0.20 needs Rust 1.97.1 or newer).
2. In this folder: `cargo run --features dev`

The first build takes several minutes; later builds are fast.

**macOS note:** if linking fails with "unknown architecture arm64e", update the Command Line
Tools in Software Update, or put
`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk` in front of the command.

## Controls

- Click the window to capture the mouse; Esc pauses (Resume, Settings, Save and quit)
- WASD walk, Space jump (or swim up), Shift sprint, Ctrl or C crouch (you won't walk off edges)
- F switches between walking and flying. Flying: Space up, Shift down, Ctrl fast,
  mouse wheel changes flying speed, blocks break instantly
- Keys 1-9 or the mouse wheel pick what you hold from the hotbar
- Hold left click to mine (what you break goes into your pockets) or to hit the creature in front of
  you; right click places the block or torch you're holding, opens a chest, talks to a villager, or
  (held down) eats food
- Tab opens the inventory: click to pick up and put down, R turns what you hold, right click moves
  things to and from a chest or puts clothes on, hover and press 1-9 to put something on the hotbar,
  Q throws it away; click a recipe to craft it
- B unrolls your bedroll on a clear, flat 2x2 patch (your camp), or packs it up when you're beside it
- Z sleeps at camp, at night or when worn out
- K opens the skill tree; G, X and V cast Levitation, Light and Warmth once you've learned them
  (while levitating, Space rises and Ctrl/C sinks)
- Test keys: F7 is a stab, F8 a club blow, F9 gives you one of everything
- `-` / `=` change the view distance; hold T to fast-forward time; F4 changes the weather
- F3 toggles the debug overlay

Tools mine their own materials faster: the pickaxe for stone and ore, the axe for wood and
leaves, the shovel for soil.

## Carrying things

Items take space by their size: a pickaxe fills 2x3 cells, a berry one. You have pockets (5x3) and a
bag on your back (a reed satchel to start; a hide pack is bigger). Clothes keep you warm and armor
keeps part of each blow off the bar it lands on. Everything you carry and wear counts toward your
load. Craft from what you gather: logs into planks and sticks, grass and leaves give plant fiber for
cordage, and stone, sticks and cordage make tools. Chests (8 planks) hold more. Hide, fur and meat come
from hunting.

## Staying alive

Health has four bars, outermost first: Damage, Fatigue, Blood and Injuries. You die when
Injuries runs out. Most blows drain Damage and spill into the next bar; stabs start at Blood and
make you bleed; a fall from more than 12 blocks starts at Injuries; cold, hunger, lack of air and
running yourself ragged wear down Fatigue. If any bar under Damage drops below half (the tick
mark), it turns mortal: every bar above it empties, new blows land on it, and nothing above it
heals until it is treated back over half.

Food comes in four tiers: berries and reed hearts give a short burst of speed, smoked fish and
bread mend Damage, stew and cured meat reach Blood, and a feast or ember herb restores
everything, even a mortal bar. Sleeping at camp rests Fatigue, mends Blood and Injuries a
little, and restores Damage when nothing is mortal. Carrying little makes you faster, a heavy
load slows you and makes falls hurt more.

Two meters bottom right are always on. **Air** reads the oxygen in the air where you stand: thin
high on the sky islands, stale deep underground, worse in swamp mist and gales. Under water it
reads nothing and you hold your breath (20 seconds). **Temperature** is what you feel: the climate,
the season, day and night, the weather (rain, storms and cold snaps make it colder), being wet,
torches and fire nearby, clothes and skills. Either meter starts to shake when you leave the
comfortable range: at first it only wears down Fatigue, and never past the mortal line. Further out
it shakes harder and drains faster, and at the far end (deadly cold, scorching heat, suffocating
air) it deals real damage and can kill you. The stages live in `assets/data/exposure.ron`.

## The living world

Marsh boars and fen stags roam by day: stags bolt when you come close (crouch to creep up), boars
leave you alone until you hit them. At night bog lurkers hunt the swamp, and ancestral spirits
drift through walls towards you and touch you with cold, wearing down Fatigue. Spirits won't come
within a few steps of a torch, placed or in your hand, and fade at dawn. Whatever you hold is your
weapon: tools hit harder than fists, and a flint spear (flint, sticks, cordage) harder still.
Creatures drop hide, fur, meat and spirit embers; roast meat at camp, and three embers with plant
fiber make an ember herb.

Villages of stilt huts stand on the high ground; one is always within a short walk of where you
start. Each hut has someone living in it, who spends the night indoors, gathers at the plaza in the
morning and evening and goes about the village by day; right click to talk. Barrows are old graves
under standing stones, with a stair down to a hall where wights wake when you come near; their
chests hold old coins and gear. Walking into a village, a barrow or a new region shows its name.

Seasons bring their own weather: gales in spring and autumn push you along and thin the air, and
cold snaps in autumn and winter bite hard.

## Growing stronger

Mining, hunting, crafting and finding new places earn experience; each level gives a skill point.
The skill tree (K) is a map of major nodes, each ringed by eight smaller ones. Every edge has a cost
in points, a major opens its ring, and taking a ring node closes the two beside it, so no two
characters grow quite alike. The Wanderer is where everyone starts; the Mystic next to it teaches
Levitation, enough to reach the sky islands on one full bar of focus. Light keeps spirits back
like a torch, and Warmth takes the edge off the cold (it is freezing up among the islands). Focus
refills while no spell is running.

## Settings

Esc, then Settings: view distance, mouse speed, field of view, volume, ambience, clouds on or
off and the debug overlay. They're saved in `saves/settings.ron` and shared by every world.

## Sound

`assets/sounds/` holds placeholder sounds made by `tools/gen_placeholder_sounds.py`. Swap in real
recordings by name. `assets/data/sounds.ron` says which footsteps and mining sounds each kind of
ground makes.

## Art

`assets/textures/blocks.png` is placeholder art: a 512x512 sheet of 32x32 tiles.
Paint over any tile and rerun the game.

### Item sprites

Every item has a picture on one of three sheets in `assets/textures/icons/`:

- `items.png`: food, materials, clothes, bags and torches
- `weapons.png`: tools and weapons
- `blocks.png`: blocks, drawn as little cubes

Next to each PNG, a `.ron` file says where each item's sprite is: `"stone_pickaxe": (0, 0, 40, 60)`
(x, y, width, height in pixels).

- A sprite is 20 pixels per inventory cell the item takes, so the 2x3 pickaxe is 40x60. Draw tools
  upright, with the end you hold at the bottom.
- The inventory, hotbar and crafting list draw the sprite at 2x.
- The same sprite is the 3D model in your hand and on the ground: every opaque pixel becomes a little
  slab. Transparent pixels are empty space.
- `python3 tools/gen_item_art.py` draws a placeholder for any item that doesn't have one yet (run it
  after adding an item to `items.ron`). It keeps everything already in the sheets, so it never paints
  over your work. `--reset items` (or `weapons`, `blocks`) starts that sheet over; `--reset blocks`
  redraws the cubes after you repaint the block textures.

Restart the game to see changes.

### The UI sheet

Every frame, button, slot, bar, icon and skill-tree stone on screen comes from
`assets/textures/ui.png` (256x128). `assets/textures/ui.ron` says where each sprite is:
`"panel": (rect: (x, y, w, h), slice: Some((left, top, right, bottom)))`.

- Each sheet pixel is drawn 2x2 on screen (`scale` in ui.ron).
- Frames with a `slice` stretch: the corners stay as painted, the edges stretch along their length
  and the middle fills the rest. So a 24x24 `panel` can be a window of any size. Keep the corners
  inside the slice lines when you repaint.
- Grey sprites (`node_major`, `node_minor`, `item`, `fill`, `white`) are tinted by the game, so paint
  them in greys.
- To move or resize a sprite, change its line in `ui.ron`. The names are what the game asks for.
- `python3 tools/gen_ui_art.py` rebuilds the placeholder sheet. It overwrites `ui.png`, so keep a copy
  of your own painting first.

Restart the game to see changes to `ui.png`.

### The font

`assets/fonts/ui.ttf` and `ui_bold.ttf` are Jersey 15 (SIL Open Font License, see `OFL.txt`); both are the same file for now, so a bolder face can go in `ui_bold.ttf`.
To use another font, replace those two files with any `.ttf` or `.otf` and rebuild: the font is
compiled into the game. Every piece of text gets a dark shadow so it reads over a bright sky or snow.

### Clouds

Three blocky layers: a low grey deck (y 200) that only forms in rain, snow and storms, the puffy
middle layer (y 236) and thin high wisps (y 300) that drift faster. How much sky they cover follows
the weather's `clouds` value in `assets/data/weather.ron`: a clear day has a scattering of clouds,
rain and snow close the sky over.

## Worlds

You start in the Great Swamp. Your edits are saved to `saves/world1` every 30 seconds and when
you quit; delete that folder to start over. The world seed is in `saves/world1/world.ron`.

A day lasts 20 minutes and each season 10 days. The weather follows the season: rain turns to
snow where it is cold, and autumn turns the leaves.
