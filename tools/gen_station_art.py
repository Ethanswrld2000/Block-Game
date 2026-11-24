#!/usr/bin/env python3
"""Generate the placeholder station texture sheet.

Writes assets/textures/stations.png (8 x 4 tiles of 32 x 32 px) and
assets/textures/stations.ron (material name -> tile index, row-major).

Stations are built from boxes (`game::station::models`); each box has a
material (wood, stone, iron, ...). The game cuts this sheet into one texture
per material and repeats it over every box at world scale: one tile covers one
block, the same pixel size as the terrain, so nothing stretches.

Paint over stations.png in any pixel editor. Tiles must repeat seamlessly
(left edge meets right edge, top meets bottom). Keep tile positions stable, or
change stations.ron to match. Paint in real colours, except `urn`: it is
tinted by each urn's own colour, so paint it in light greys.

Materials not listed here (fire, brew, broth, crystal, spirit, the urn's
marks) stay flat colours.

Run from the repo root:  python3 tools/gen_station_art.py
Warning: this overwrites stations.png.
"""
import os
import random
from PIL import Image

T = 32
COLS, ROWS = 8, 4

# Name, base colour (sRGB 0-1, from `Mat::look`), painter.
# The order is the tile index: append new materials at the end.
MATERIALS = [
    ("wood", (0.5, 0.36, 0.22), "planks"),
    ("wood_dark", (0.34, 0.24, 0.15), "planks"),
    ("groove", (0.22, 0.16, 0.1), "grain"),
    ("stone", (0.5, 0.5, 0.52), "stone"),
    ("stone_dark", (0.36, 0.36, 0.38), "stone"),
    ("iron", (0.3, 0.3, 0.32), "metal"),
    ("steel", (0.58, 0.62, 0.68), "metal"),
    ("brass", (0.72, 0.56, 0.26), "metal"),
    ("reed", (0.64, 0.6, 0.36), "reed"),
    ("cloth", (0.33, 0.4, 0.22), "weave"),
    ("linen", (0.78, 0.72, 0.58), "weave"),
    ("cord", (0.72, 0.63, 0.45), "cord"),
    ("hide", (0.55, 0.35, 0.21), "hide"),
    ("moss", (0.3, 0.45, 0.2), "moss"),
    ("clay", (0.62, 0.38, 0.26), "clay"),
    ("urn", (0.92, 0.92, 0.92), "urn"),
]


def shade(base, k):
    return tuple(max(0, min(255, round(c * k * 255))) for c in base) + (255,)


def wrap_noise(rng, cells):
    """Smooth value noise that repeats every tile, `cells` lumps across."""
    g = [[rng.random() for _ in range(cells)] for _ in range(cells)]
    out = [[0.0] * T for _ in range(T)]
    for y in range(T):
        for x in range(T):
            fx, fy = x * cells / T, y * cells / T
            x0, y0 = int(fx), int(fy)
            tx, ty = fx - x0, fy - y0
            tx, ty = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
            a = g[y0 % cells][x0 % cells]
            b = g[y0 % cells][(x0 + 1) % cells]
            c = g[(y0 + 1) % cells][x0 % cells]
            d = g[(y0 + 1) % cells][(x0 + 1) % cells]
            out[y][x] = (a * (1 - tx) + b * tx) * (1 - ty) + (c * (1 - tx) + d * tx) * ty
    return out


def paint(kind, rng):
    """Brightness factor per pixel (1 = the base colour)."""
    k = [[1.0] * T for _ in range(T)]
    n = wrap_noise(rng, 4)
    fine = [[rng.random() for _ in range(T)] for _ in range(T)]
    for y in range(T):
        for x in range(T):
            v = 1.0
            if kind == "planks":
                # Boards 8 px high, grain running along them, a dark seam under each.
                row = y // 8
                grain = wrap_noise_row[row][x]
                v = 0.92 + 0.16 * grain + 0.05 * (fine[y][x] - 0.5)
                v += 0.05 * ((row * 37) % 5 - 2) / 2
                if y % 8 == 7:
                    v = 0.68
                elif y % 8 == 0:
                    v += 0.08
                # A butt joint per board, staggered.
                if x == (row * 11 + 3) % T:
                    v = 0.72
            elif kind == "grain":
                v = 0.9 + 0.2 * wrap_noise_row[y // 4][x] + 0.06 * (fine[y][x] - 0.5)
            elif kind == "stone":
                v = 0.86 + 0.24 * n[y][x] + 0.1 * (fine[y][x] - 0.5)
            elif kind == "metal":
                # Brushed: faint streaks along x, a few dark pits.
                v = 0.94 + 0.08 * streak[y] + 0.06 * n[y][x] + 0.03 * (fine[y][x] - 0.5)
                if fine[y][x] > 0.985:
                    v = 0.75
            elif kind == "reed":
                # Stalks 4 px wide, lit on the left, with a node now and then.
                c = x % 4
                v = (1.08, 1.0, 0.92, 0.72)[c] + 0.05 * (fine[y][x] - 0.5)
                if (y + (x // 4) * 7) % 16 == 0 and c < 3:
                    v = 0.8
            elif kind == "weave":
                over = ((x // 2) + (y // 2)) % 2 == 0
                v = (1.06 if over else 0.9) + 0.08 * n[y][x] + 0.04 * (fine[y][x] - 0.5)
                if (x % 2 == 1) if over else (y % 2 == 1):
                    v -= 0.06
            elif kind == "cord":
                # Twisted strands: diagonal bands.
                s = (x + y) % 6
                v = (1.1, 1.05, 0.98, 0.9, 0.8, 0.95)[s] + 0.05 * (fine[y][x] - 0.5)
            elif kind == "hide":
                v = 0.88 + 0.22 * n[y][x] + 0.08 * (fine[y][x] - 0.5)
                if fine[y][x] > 0.97:
                    v -= 0.12
            elif kind == "moss":
                v = 0.8 + 0.35 * n[y][x] + 0.2 * (fine[y][x] - 0.5)
            elif kind == "clay":
                v = 0.92 + 0.14 * n[y][x] + 0.05 * (fine[y][x] - 0.5)
                if fine[y][x] > 0.99:
                    v = 0.78
            elif kind == "urn":
                # Wheel-thrown: soft rings across, light clay speckle.
                v = 0.93 + 0.05 * ring[y] + 0.06 * n[y][x] + 0.04 * (fine[y][x] - 0.5)
            k[y][x] = v
    return k


def rows_of_grain(rng, rows):
    """Grain along x, one strip per board (wraps along x)."""
    out = []
    for _ in range(rows):
        g = wrap_noise(rng, 8)
        out.append([g[0][x] * 0.5 + g[T // 2][(x * 3) % T] * 0.5 for x in range(T)])
    return out


def main():
    root = os.path.join(os.path.dirname(__file__), "..", "assets", "textures")
    img = Image.new("RGBA", (COLS * T, ROWS * T), (0, 0, 0, 0))
    global wrap_noise_row, streak, ring
    lines = [
        "// Material name -> tile index into stations.png (row-major, 8 tiles per row, 32x32 px each).",
        "// Generated by tools/gen_station_art.py. Safe to edit by hand.",
        "(",
        "    tile_size: 32,",
        "    columns: 8,",
        "    tiles: {",
    ]
    for i, (name, base, kind) in enumerate(MATERIALS):
        rng = random.Random(1000 + i)
        wrap_noise_row = rows_of_grain(rng, 8)
        streak = [rng.random() - 0.5 for _ in range(T)]
        ring = [0.5 + 0.5 * __import__("math").sin(y * 6.2832 * 4 / T) for y in range(T)]
        k = paint(kind, rng)
        ox, oy = (i % COLS) * T, (i // COLS) * T
        for y in range(T):
            for x in range(T):
                img.putpixel((ox + x, oy + y), shade(base, k[y][x]))
        lines.append(f'        "{name}": {i},')
    lines += ["    },", ")", ""]
    img.save(os.path.join(root, "stations.png"))
    with open(os.path.join(root, "stations.ron"), "w") as f:
        f.write("\n".join(lines))
    print(f"wrote stations.png ({len(MATERIALS)} tiles) and stations.ron")


if __name__ == "__main__":
    main()
