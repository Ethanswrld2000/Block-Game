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
load slows you and makes falls hurt more. Air runs short underwater, deep underground, high on the
sky islands and in swamp mist; warmth follows the climate, the weather, the time of day and
torches nearby.

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

Esc, then Settings: view distance, mouse speed, field of view, volume, ambience and the debug
overlay. They're saved in `saves/settings.ron` and shared by every world.

## Sound

`assets/sounds/` holds placeholder sounds made by `tools/gen_placeholder_sounds.py`. Swap in real
recordings by name. `assets/data/sounds.ron` says which footsteps and mining sounds each kind of
ground makes.

## Art

`assets/textures/blocks.png` is placeholder art: a 512x512 sheet of 32x32 tiles.
Paint over any tile and rerun the game.

## Worlds

You start in the Great Swamp. Your edits are saved to `saves/world1` every 30 seconds and when
you quit; delete that folder to start over. The world seed is in `saves/world1/world.ron`.

A day lasts 20 minutes and each season 10 days. The weather follows the season: rain turns to
snow where it is cold, and autumn turns the leaves.
