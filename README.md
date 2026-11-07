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
- WASD move, Space up, Shift down, Ctrl (or Cmd) fast
- Left click breaks a block, right click places one; keys 1-9 choose the block
- `-` / `=` change the view distance
- F3 toggles the debug overlay

## Art

`assets/textures/blocks.png` is a 512x512 sheet of 32x32 pixel-art tiles, painted by
`python3 tools/gen_block_art.py` (add `--preview` for a zoomed, labelled preview). Rerunning the
script overwrites the PNG, so if you hand-paint tiles, stop using the script or move the tile into it.
Keep tile positions stable: append new tiles at the end.

## Worlds

You start in the Great Swamp. Your edits are saved to `saves/world1` every 30 seconds and when
you quit; delete that folder to start over. The world seed is in `saves/world1/world.ron`.
