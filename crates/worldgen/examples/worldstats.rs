//! Prints a summary of the world: `cargo run -p worldgen --example worldstats --release`.
//! Also writes a top-down biome/height map to `worldmap.png` if a path is given.

use std::collections::BTreeMap;
use voxel::{BlockRegistry, TileSheet};
use worldgen::WorldGen;

fn main() {
    let seed: u64 = std::env::var("SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(0x005E_ED0F_5A4D);
    let tiles = TileSheet::from_ron(include_str!("../../../assets/textures/blocks.ron")).unwrap();
    let reg = BlockRegistry::from_ron(include_str!("../../../assets/data/blocks.ron"), &tiles).unwrap();
    let g = WorldGen::new(seed, &reg, include_str!("../../../assets/data/biomes.ron")).unwrap();

    let spawn = g.find_spawn();
    println!("spawn {:?} in {}", spawn, g.biome_at(spawn[0], spawn[1], spawn[2]).title);

    let size = 4096;
    let step = 16;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    let n = size / step;
    let mut img = vec![0u8; (n * n * 3) as usize];
    for (iz, z) in (-size / 2..size / 2).step_by(step as usize).enumerate() {
        for (ix, x) in (-size / 2..size / 2).step_by(step as usize).enumerate() {
            let c = g.column(x, z);
            lo = lo.min(c.height);
            hi = hi.max(c.height);
            let b = g.biomes.get(c.biome);
            *counts.entry(b.name.clone()).or_default() += 1;
            let shade = (0.6 + (c.height as f32 / 400.0)).clamp(0.3, 1.4);
            let i = (iz * n as usize + ix) * 3;
            for k in 0..3 {
                img[i + k] = (b.fog[k] * 255.0 * shade).clamp(0.0, 255.0) as u8;
            }
            if c.height < 0 {
                img[i] /= 3;
                img[i + 1] /= 2;
            }
        }
    }
    println!("heights {lo}..{hi} over {size}x{size} blocks");
    // Landmarks near spawn, for screenshots and testing.
    let (mut peak, mut island) = ((0, i32::MIN, 0), None);
    for z in (spawn[2] - 1500..spawn[2] + 1500).step_by(8) {
        for x in (spawn[0] - 1500..spawn[0] + 1500).step_by(8) {
            let h = g.surface_height(x, z);
            if h > peak.1 {
                peak = (x, h, z);
            }
            if island.is_none() && (x - spawn[0]).abs() < 400 && (z - spawn[2]).abs() < 400 {
                if let Some(t) = g.island_top(x, z) {
                    island = Some((x, t, z));
                }
            }
        }
    }
    println!("highest peak near spawn {:?}, an island near spawn {:?}", peak, island);
    let total: usize = counts.values().sum();
    for (k, v) in &counts {
        println!("  {k:12} {:5.1}%", *v as f32 * 100.0 / total as f32);
    }
    if let Some(path) = std::env::args().nth(1) {
        let mut out = format!("P6 {n} {n} 255\n").into_bytes();
        out.extend_from_slice(&img);
        std::fs::write(&path, out).unwrap();
        println!("wrote {path} (PPM)");
    }
}
