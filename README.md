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

- Click the window to capture the mouse, Esc to release it
- WASD walk, Space jump (or swim up), Shift sprint, Ctrl or C crouch (you won't walk off edges)
- F switches between walking and flying. Flying: Space up, Shift down, Ctrl fast,
  mouse wheel changes flying speed, blocks break instantly
- Keys 1-9 pick what you hold from the hotbar page; the mouse wheel scrolls through every item
- Hold left click to mine; right click places the block or torch you're holding, hold it to eat food
- B unrolls your bedroll on a clear, flat 2x2 patch (your camp), or packs it up when you're beside it
- Z sleeps at camp, at night or when worn out
- Test keys: F6 adds pack weight, F7 is a stab, F8 a club blow
- `-` / `=` change the view distance; hold T to fast-forward time; F4 changes the weather
- F3 toggles the debug overlay

Tools mine their own materials faster: the pickaxe for stone and ore, the axe for wood and
leaves, the shovel for soil.

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

## Art

`assets/textures/blocks.png` is placeholder art: a 512x512 sheet of 32x32 tiles.
Paint over any tile and rerun the game.

## Worlds

You start in the Great Swamp. Your edits are saved to `saves/world1` every 30 seconds and when
you quit; delete that folder to start over. The world seed is in `saves/world1/world.ron`.

A day lasts 20 minutes and each season 10 days. The weather follows the season: rain turns to
snow where it is cold, and autumn turns the leaves.
