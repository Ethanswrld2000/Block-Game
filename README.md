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
- WASD move, Space up, Shift down, Ctrl (or Cmd) fast; mouse wheel changes flying speed
- Left click breaks a block, right click places one; keys 1-9 choose the block
- `-` / `=` change the view distance
- F3 toggles the debug overlay
- Hold T to fast-forward time; F4 changes the weather

## Art

`assets/textures/blocks.png` is placeholder art: a 512x512 sheet of 32x32 tiles.
Paint over any tile and rerun the game.

## Worlds

You start in the Great Swamp. Your edits are saved to `saves/world1` every 30 seconds and when
you quit; delete that folder to start over. The world seed is in `saves/world1/world.ron`.

A day lasts 20 minutes and each season 10 days. The weather follows the season: rain turns to
snow where it is cold, and autumn turns the leaves.
