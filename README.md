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

`assets/textures/blocks.png` is placeholder art: a 512x512 sheet of 32x32 tiles.
Paint over any tile and rerun the game. The row of single blocks at the back of the
island shows every tile so you can check your changes.
