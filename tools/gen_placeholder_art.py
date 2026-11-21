#!/usr/bin/env python3
"""Generate the placeholder block spritesheet.

Writes assets/textures/blocks.png (16 x 16 tiles of 32 x 32 px = 512 x 512)
and assets/textures/blocks.ron (tile name -> index).

These are placeholders: paint over blocks.png in any editor. Keep tile
positions the same, or update blocks.ron to match.
Run from the repo root:  python3 tools/gen_placeholder_art.py
The core ground, wood and leaf tiles are drawn in tools/drawn_tiles.py, with
extra versions listed in VARIANTS (named <tile>_2, <tile>_3, ...).
To add new tiles without touching painted ones:
    python3 tools/gen_placeholder_art.py --only tall_grass fern
paints just those tiles into the existing blocks.png (and rewrites blocks.ron).
"""
import os
import sys
import numpy as np
from PIL import Image

from drawn_tiles import (cobblestone as cobble, dirt, grass_side, grass_top, gravel, leaves, log_side,
                         log_top, moss, mud, peat, planks, sand, stone)

T = 32          # tile size in pixels
COLS = 16       # tiles per row
ROWS = 16
rng = np.random.default_rng(7)


def value_noise(scale, seed):
    """Tileable value noise in [0, 1] at the given cell scale."""
    r = np.random.default_rng(seed)
    n = T // scale
    grid = r.random((n, n))
    ys, xs = np.mgrid[0:T, 0:T] / scale
    x0, y0 = xs.astype(int) % n, ys.astype(int) % n
    x1, y1 = (x0 + 1) % n, (y0 + 1) % n
    fx, fy = xs - np.floor(xs), ys - np.floor(ys)
    fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a = grid[y0, x0] * (1 - fx) + grid[y0, x1] * fx
    b = grid[y1, x0] * (1 - fx) + grid[y1, x1] * fx
    return a * (1 - fy) + b * fy


def fbm(seed, scales=(16, 8, 4, 2)):
    out, w = np.zeros((T, T)), 0.0
    for i, s in enumerate(scales):
        amp = 0.5 ** i
        out += value_noise(s, seed + i * 31) * amp
        w += amp
    return out / w


def ramp(t, colors):
    """Map t in [0,1] onto a list of RGB colors."""
    colors = np.array(colors, dtype=float)
    t = np.clip(t, 0, 0.9999) * (len(colors) - 1)
    i = t.astype(int)
    f = (t - i)[..., None]
    return colors[i] * (1 - f) + colors[np.minimum(i + 1, len(colors) - 1)] * f


def rgba(rgb, alpha=255):
    a = np.full(rgb.shape[:2] + (1,), alpha, dtype=float) if np.isscalar(alpha) else alpha[..., None]
    return np.concatenate([rgb, a], axis=-1)


def speckle(img, seed, amount=0.08):
    r = np.random.default_rng(seed)
    img[..., :3] *= 1 + (r.random((T, T, 1)) - 0.5) * amount * 2
    return img


def noisy(seed, colors, scales=(16, 8, 4, 2)):
    return speckle(rgba(ramp(fbm(seed, scales), colors)), seed + 1)


# ---- tile painters -------------------------------------------------------

def water(s):
    img = noisy(s, [(30, 70, 130), (44, 96, 160), (70, 130, 190)], (8, 4))
    img[..., 3] = 170
    return img


def snow(s):
    return noisy(s, [(222, 228, 236), (240, 244, 250), (252, 252, 255)], (4, 2))


def ice(s):
    img = noisy(s, [(150, 190, 230), (180, 214, 244)], (16, 8))
    img[..., 3] = 210
    return img


def clay(s):
    return noisy(s, [(140, 146, 160), (160, 166, 180)], (8, 4))


def ore(s, base_fn, ore_color):
    img = base_fn(s)
    r = np.random.default_rng(s + 77)
    for _ in range(7):
        cx, cy = r.integers(3, T - 3, 2)
        for dx in range(-2, 2):
            for dy in range(-1, 2):
                if r.random() < 0.75:
                    img[(cy + dy) % T, (cx + dx) % T, :3] = np.array(ore_color) * (0.8 + r.random() * 0.4)
    return img


def reed(s):
    img = np.zeros((T, T, 4))
    r = np.random.default_rng(s)
    for x in r.choice(T, 7, replace=False):
        h = r.integers(14, T)
        img[T - h:, x, :] = (*(np.array([90, 120, 56]) * (0.8 + r.random() * 0.4)), 255)
        img[T - h: T - h + 5, x, :3] = (110, 80, 50)  # seed head
    return img


def torch(s):
    img = np.zeros((T, T, 4))
    img[12:30, 14:18] = (110, 80, 50, 255)        # handle
    img[7:13, 13:19] = (255, 190, 60, 255)        # flame
    img[9:12, 15:17] = (255, 245, 180, 255)       # hot core
    return img


def skystone(s):
    img = noisy(s, [(150, 160, 186), (182, 192, 214), (214, 222, 240)], (8, 4, 2))
    vein = np.abs(value_noise(8, s + 4) - 0.5) < 0.04
    img[vein, :3] = (240, 220, 160)
    return img


def cloud_moss(s):
    return noisy(s, [(170, 196, 170), (206, 226, 200), (236, 246, 232)], (4, 2))


def deeprock(s):
    return noisy(s, [(40, 40, 48), (58, 58, 68), (76, 76, 88)], (4, 2))


def rootstone(s):
    """The unbreakable world floor: near-black rock with faint dull red veins."""
    img = noisy(s, [(18, 16, 20), (30, 26, 30), (44, 38, 40)], (8, 4, 2))
    veins = fbm(s + 5, (8, 4)) > 0.68
    img[veins] = (70, 30, 28, 255)
    return img


def vine(s):
    """Thin hanging vine strands with small leaves; transparent between them."""
    img = np.zeros((T, T, 4))
    r = np.random.default_rng(s)
    for x0 in (3, 10, 17, 25):
        x = x0 + int(r.integers(-1, 2))
        bottom = int(r.integers(18, T))
        for y in range(bottom):
            xx = (x + int(round(np.sin(y * 0.4 + x0) * 1.2))) % T
            img[y, xx] = (52, 86, 34, 255)
            if r.random() < 0.3:
                lx = (xx + (1 if r.random() < 0.5 else -1)) % T
                img[y, lx] = (82, 124, 50, 255)
                if y + 1 < T:
                    img[y + 1, lx] = (66, 108, 42, 255)
    return img


def glowcap(s):
    img = np.zeros((T, T, 4))
    img[18:30, 15:17] = (200, 200, 180, 255)       # stem
    ys, xs = np.mgrid[0:T, 0:T]
    cap = ((xs - 15.5) ** 2 / 64 + (ys - 18) ** 2 / 36 < 1) & (ys < 19)
    img[cap] = (80, 200, 220, 255)
    return img


def tall_grass(s):
    """Blades of grass for a crossed-quad plant; transparent around them."""
    img = np.zeros((T, T, 4))
    r = np.random.default_rng(s)
    for _ in range(18):
        x = int(r.integers(2, T - 2))
        h = int(r.integers(12, T - 2))
        lean = r.uniform(-0.25, 0.25)
        shade = 0.75 + r.random() * 0.45
        for i in range(h):
            xx = int(round(x + lean * i)) % T
            g = np.array([70, 128, 48]) * shade * (0.8 + 0.35 * i / h)
            img[T - 1 - i, xx] = (*g, 255)
    return img


def fern(s):
    """A fern: a few arching fronds with leaflets."""
    img = np.zeros((T, T, 4))
    r = np.random.default_rng(s)
    for side in (-1, 1, -0.4, 0.4):
        shade = 0.8 + r.random() * 0.3
        for i in range(24):
            t = i / 24
            x = int(round(15.5 + side * (t * 12)))
            y = int(round(T - 1 - t * 22 + (t ** 2) * 10 * abs(side)))
            if not (0 <= x < T and 0 <= y < T):
                continue
            img[y, x] = (*(np.array([40, 96, 44]) * shade), 255)
            if i % 3 == 0 and i > 2:
                for d in (-2, -1, 1, 2):
                    yy = y + abs(d) // 2 + 1
                    if 0 <= yy < T and 0 <= x + d < T:
                        img[yy, x + d] = (*(np.array([58, 124, 54]) * shade), 255)
    return img


def bog_bloom(s):
    """Tall stems with small pale flower heads."""
    img = np.zeros((T, T, 4))
    r = np.random.default_rng(s)
    for x in (9, 15, 22):
        h = int(r.integers(14, 24))
        img[T - h:, x] = (66, 112, 46, 255)
        img[T - h - 3:T - h + 1, x - 1:x + 2] = (226, 214, 236, 255)
        img[T - h - 2, x] = (246, 220, 120, 255)
        for _ in range(3):
            y = int(r.integers(T - h + 2, T - 2))
            img[y, x + r.choice([-1, 1])] = (80, 132, 54, 255)
    return img


def floorboards(s):
    # Long narrow boards running one way, with peg holes at their ends.
    img = noisy(s, [(138, 100, 60), (170, 128, 78)], (16, 2))
    ys, xs = np.mgrid[0:T, 0:T]
    img[(xs % 6) == 5, :3] *= 0.55
    ends = ((ys + (xs // 6) * 13) % 32) == 0
    img[ends & ((xs % 6) != 5), :3] *= 0.6
    img[((ys + (xs // 6) * 13) % 32 == 3) & ((xs % 6) == 2), :3] = (70, 50, 30)
    return img


def timber_wall(s):
    # Pale plank infill inside a dark beam frame with a cross brace.
    img = noisy(s, [(170, 140, 96), (196, 166, 118)], (16, 2))
    ys, xs = np.mgrid[0:T, 0:T]
    img[(ys % 8) == 7, :3] *= 0.75
    beam = noisy(s + 1, [(66, 46, 28), (88, 62, 38)], (8, 2))
    frame = (xs < 4) | (xs > 27) | (ys < 4) | (ys > 27) | (np.abs(xs - ys) < 3)
    img[frame] = beam[frame]
    return img


def wattle_wall(s):
    # Sticks woven over and under upright stakes, mud packed between.
    img = noisy(s, [(84, 64, 44), (106, 82, 56)], (8, 2))
    ys, xs = np.mgrid[0:T, 0:T]
    stick = noisy(s + 2, [(120, 92, 58), (150, 116, 74)], (8, 2))
    row = ys // 4
    weave = ((ys % 4) != 3) & (((xs // 8 + row) % 2 == 0) | ((ys % 4) == 1))
    img[weave] = stick[weave]
    stakes = (xs % 8) == 0
    img[stakes, :3] = (70, 52, 34)
    return img


def thatch(s):
    # Bundles of dry reed laid in overlapping rows, tips hanging down.
    img = noisy(s, [(150, 128, 70), (190, 164, 92)], (2, 1))
    ys, xs = np.mgrid[0:T, 0:T]
    streak = (np.sin(xs * 2.1 + value_noise(4, s) * 6) + 1) / 2
    img[..., :3] *= (0.8 + 0.2 * streak)[..., None]
    img[(ys % 8) == 0, :3] *= 0.6
    img[(ys % 8) == 1, :3] *= 0.8
    return img


def chair(s):
    # A chair seen from the side: seat, back and legs over a dark gap.
    img = noisy(s, [(40, 30, 20), (52, 38, 26)], (8, 2))
    wood = noisy(s + 1, [(132, 96, 56), (164, 122, 74)], (16, 2))
    ys, xs = np.mgrid[0:T, 0:T]
    part = ((ys >= 16) & (ys < 20)) | ((xs >= 4) & (xs < 8)) | ((xs >= 24) & (xs < 28) & (ys >= 16)) | ((xs < 8) & (ys < 16) & (xs >= 4))
    img[part] = wood[part]
    img[(ys < 16) & (xs >= 4) & (xs < 28) & (ys % 5 == 0)] = wood[(ys < 16) & (xs >= 4) & (xs < 28) & (ys % 5 == 0)]
    return img


def table(s):
    # A thick table top over four legs.
    img = noisy(s, [(40, 30, 20), (52, 38, 26)], (8, 2))
    wood = noisy(s + 1, [(138, 100, 60), (170, 128, 78)], (16, 2))
    ys, xs = np.mgrid[0:T, 0:T]
    part = (ys < 7) | (((xs < 5) | (xs > 26)) & (ys >= 7))
    img[part] = wood[part]
    img[ys == 6, :3] *= 0.6
    return img


def wooden_door(s):
    # Upright planks with two cross boards and an iron latch.
    img = noisy(s, [(120, 86, 50), (150, 110, 66)], (16, 2))
    ys, xs = np.mgrid[0:T, 0:T]
    img[(xs % 8) == 7, :3] *= 0.6
    brace = ((ys >= 6) & (ys < 10)) | ((ys >= 22) & (ys < 26))
    img[brace, :3] *= 0.75
    img[(xs >= 24) & (xs < 27) & (ys >= 14) & (ys < 18), :3] = (60, 60, 64)
    return img


def glass(s):
    # Pale see-through glass with a bright rim and a glint.
    img = noisy(s, [(190, 220, 230), (210, 236, 244)], (16, 8))
    img[..., 3] = 90
    ys, xs = np.mgrid[0:T, 0:T]
    rim = (xs < 1) | (xs > 30) | (ys < 1) | (ys > 30)
    img[rim, 3] = 220
    glint = (np.abs((xs - ys) - 6) < 2) & (xs > 6) & (xs < 16)
    img[glint] = (250, 254, 255, 200)
    return img


def window(s):
    # Four glass panes in a wooden frame.
    img = glass(s)
    wood = noisy(s + 1, [(108, 76, 44), (136, 98, 58)], (8, 2))
    ys, xs = np.mgrid[0:T, 0:T]
    frame = (xs < 3) | (xs > 28) | (ys < 3) | (ys > 28) | ((xs >= 15) & (xs < 17)) | ((ys >= 15) & (ys < 17))
    img[frame] = wood[frame]
    return img


def mushroom_stem(s):
    img = noisy(s, [(206, 196, 170), (226, 218, 196)], (8, 2))
    xs = np.mgrid[0:T, 0:T][1]
    img[(xs % 7) == 3, :3] *= 0.9
    return img


def mushroom_cap(s):
    img = noisy(s, [(150, 44, 34), (178, 60, 44)], (8, 2))
    rng = np.random.default_rng(s)
    for _ in range(5):
        cx, cy, r = rng.integers(3, T - 3), rng.integers(3, T - 3), rng.integers(2, 4)
        ys, xs = np.mgrid[0:T, 0:T]
        img[(xs - cx) ** 2 + (ys - cy) ** 2 <= r * r, :3] = (236, 226, 204)
    return img


def mushroom_gills(s):
    img = noisy(s, [(196, 170, 140), (222, 200, 170)], (4, 2))
    xs = np.mgrid[0:T, 0:T][1]
    img[(xs % 3) == 0, :3] *= 0.8
    return img


def burnt_stem(s):
    img = noisy(s, [(40, 34, 30), (66, 56, 48)], (8, 2))
    xs = np.mgrid[0:T, 0:T][1]
    img[(xs % 7) == 3, :3] *= 0.7
    return img


def burnt_cap(s):
    img = noisy(s, [(28, 22, 20), (58, 40, 34)], (8, 2))
    rng = np.random.default_rng(s)
    for _ in range(4):
        cx, cy = rng.integers(2, T - 2), rng.integers(2, T - 2)
        img[cy, cx, :3] = (196, 84, 30)
    return img


def grave(s):
    # A rough stone marker with a carved line.
    img = noisy(s, [(96, 96, 100), (128, 128, 132)], (8, 4))
    ys, xs = np.mgrid[0:T, 0:T]
    img[(ys >= 8) & (ys < 10) & (xs >= 8) & (xs < 24), :3] = (60, 60, 64)
    img[(xs >= 15) & (xs < 17) & (ys >= 4) & (ys < 20), :3] = (60, 60, 64)
    return img


TILES = [
    ("grass_top", grass_top), ("grass_side", grass_side), ("dirt", dirt), ("stone", stone),
    ("cobblestone", cobble), ("sand", sand), ("mud", mud), ("moss", moss),
    ("peat", peat), ("log_side", log_side), ("log_top", log_top), ("leaves", leaves),
    ("planks", planks), ("water", water), ("snow", snow), ("ice", ice),
    ("gravel", gravel), ("clay", clay),
    ("coal_ore", lambda s: ore(s, stone, (30, 30, 30))),
    ("iron_ore", lambda s: ore(s, stone, (196, 150, 120))),
    ("reed", reed), ("torch", torch), ("skystone", skystone), ("cloud_moss", cloud_moss),
    ("deeprock", deeprock),
    ("swamp_log_side", lambda s: log_side(s, (46, 44, 30), (76, 72, 48))),
    ("swamp_log_top", lambda s: log_top(s, (46, 44, 30), (120, 110, 72))),
    ("swamp_leaves", lambda s: leaves(s, ((40, 60, 30), (58, 84, 36), (84, 104, 44)))),
    ("glowcap", glowcap),
    ("tall_grass", tall_grass), ("fern", fern), ("bog_bloom", bog_bloom),
    ("rootstone", rootstone),
    ("vine", vine),
    ("floorboards", floorboards), ("timber_wall", timber_wall), ("wattle_wall", wattle_wall), ("thatch", thatch),
    ("chair", chair), ("table", table), ("wooden_door", wooden_door), ("glass", glass), ("window", window),
    ("mushroom_stem", mushroom_stem), ("mushroom_cap", mushroom_cap), ("mushroom_gills", mushroom_gills),
    ("burnt_stem", burnt_stem), ("burnt_cap", burnt_cap), ("grave", grave),
]

# Other versions of a tile, picked per block by the terrain shader so the
# ground doesn't show a repeating grid. turn = also give each block a random
# quarter turn (only for tiles with no light direction that matters).
VARIANTS = {
    "grass_top": (3, True), "grass_side": (2, False), "dirt": (3, True), "stone": (3, False),
    "cobblestone": (2, False), "sand": (2, True), "gravel": (2, False), "mud": (2, True),
    "moss": (2, True), "peat": (2, False), "planks": (2, False), "log_side": (1, False),
}
for _base, (_n, _turn) in VARIANTS.items():
    _fn = dict(TILES)[_base]
    TILES += [(f"{_base}_{k + 2}", _fn) for k in range(_n)]


def main():
    root = os.path.join(os.path.dirname(__file__), "..", "assets", "textures")
    os.makedirs(root, exist_ok=True)
    path = os.path.join(root, "blocks.png")
    only = sys.argv[sys.argv.index("--only") + 1:] if "--only" in sys.argv else None
    if only:
        unknown = [n for n in only if n not in dict(TILES)]
        if unknown:
            sys.exit(f"no tile called {unknown}")
        sheet = np.array(Image.open(path).convert("RGBA"), dtype=float)
    else:
        sheet = np.zeros((ROWS * T, COLS * T, 4))
    for i, (name, fn) in enumerate(TILES):
        if only and name not in only:
            continue
        tile = np.clip(fn(1000 + i * 17), 0, 255)
        r, c = divmod(i, COLS)
        sheet[r * T:(r + 1) * T, c * T:(c + 1) * T] = tile
    Image.fromarray(sheet.astype(np.uint8), "RGBA").save(path)
    with open(os.path.join(root, "blocks.ron"), "w") as f:
        f.write("// Tile name -> index into blocks.png (row-major, 16 tiles per row, 32x32 px each).\n")
        f.write("// Generated by tools/gen_placeholder_art.py. Safe to edit by hand.\n(\n    tile_size: 32,\n    columns: 16,\n    tiles: {\n")
        for i, (name, _) in enumerate(TILES):
            f.write(f'        "{name}": {i},\n')
        f.write("    },\n    variants: {\n")
        for base, (n, turn) in VARIANTS.items():
            names = ", ".join(f'"{base}_{k + 2}"' for k in range(n))
            f.write(f'        "{base}": (tiles: [{names}], turn: {"true" if turn else "false"}),\n')
        f.write("    },\n)\n")
    print(f"wrote {len(only) if only else len(TILES)} tiles")


if __name__ == "__main__":
    main()
