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
- Keys 1-9 or the mouse wheel pick what you hold from the hotbar
- Hold left click to mine; right click places the block or torch you're holding
- `-` / `=` change the view distance; hold T to fast-forward time; F4 changes the weather
- F3 toggles the debug overlay

Tools mine their own materials faster: the pickaxe for stone and ore, the axe for wood and
leaves, the shovel for soil. Falls of more than about three and a half blocks hurt (water breaks
your fall); health arrives in a later phase, so for now you only see a red flash.

## Art

`assets/textures/blocks.png` is placeholder art: a 512x512 sheet of 32x32 tiles.
Paint over any tile and rerun the game.

## Worlds

You start in the Great Swamp. Your edits are saved to `saves/world1` every 30 seconds and when
you quit; delete that folder to start over. The world seed is in `saves/world1/world.ron`.

A day lasts 20 minutes and each season 10 days. The weather follows the season: rain turns to
snow where it is cold, and autumn turns the leaves.
