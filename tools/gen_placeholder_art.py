#!/usr/bin/env python3
"""Generate the placeholder block spritesheet.

Writes assets/textures/blocks.png (16 x 16 tiles of 32 x 32 px = 512 x 512)
and assets/textures/blocks.ron (tile name -> index).

These are placeholders: paint over blocks.png in any editor. Keep tile
positions the same, or update blocks.ron to match.
Run from the repo root:  python3 tools/gen_placeholder_art.py
To add new tiles without touching painted ones:
    python3 tools/gen_placeholder_art.py --only tall_grass fern
paints just those tiles into the existing blocks.png (and rewrites blocks.ron).
"""
import os
import sys
import numpy as np
from PIL import Image

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

def grass_top(s):
    return noisy(s, [(52, 92, 40), (70, 120, 48), (96, 148, 60)], (8, 4, 2))


def dirt(s):
    return noisy(s, [(78, 54, 36), (102, 72, 46), (124, 90, 58)])


def grass_side(s):
    img = dirt(s)
    top = grass_top(s + 5)
    edge = 6 + (value_noise(4, s + 9)[0] * 6).astype(int)
    for x in range(T):
        img[: edge[x], x] = top[: edge[x], x]
    return img


def stone(s):
    img = noisy(s, [(92, 92, 96), (118, 118, 122), (140, 140, 146)], (8, 4, 2))
    cracks = value_noise(4, s + 3)
    img[np.abs(cracks - 0.5) < 0.03, :3] *= 0.7
    return img


def cobble(s):
    img = noisy(s, [(90, 90, 92), (128, 128, 130)], (4, 2))
    ys, xs = np.mgrid[0:T, 0:T]
    # irregular brick-ish mortar lines
    row = (ys // 8)
    off = (row % 2) * 6
    mortar = ((ys % 8) == 0) | (((xs + off) % 12) == 0)
    img[mortar, :3] = (60, 60, 64)
    return img


def sand(s):
    return noisy(s, [(196, 178, 128), (218, 202, 150), (232, 220, 172)], (4, 2))


def mud(s):
    img = noisy(s, [(44, 40, 30), (62, 56, 40), (80, 72, 48)], (16, 8, 4))
    wet = value_noise(8, s + 2) > 0.7
    img[wet, :3] = img[wet, :3] * 0.8 + np.array([30, 40, 30]) * 0.2
    return img


def moss(s):
    return noisy(s, [(40, 70, 34), (60, 98, 40), (110, 140, 60)], (4, 2))


def peat(s):
    img = noisy(s, [(46, 32, 24), (66, 46, 32)], (8, 2))
    ys = np.mgrid[0:T, 0:T][0]
    img[(ys % 6) == 0, :3] *= 0.75
    return img


def log_side(s, dark=(70, 50, 32), light=(110, 80, 50)):
    xs = np.mgrid[0:T, 0:T][1]
    t = (np.sin(xs * 0.9 + value_noise(4, s) * 4) + 1) / 2 * 0.6 + fbm(s) * 0.4
    return speckle(rgba(ramp(t, [dark, light])), s)


def log_top(s, bark=(70, 50, 32), wood=(170, 132, 84)):
    ys, xs = np.mgrid[0:T, 0:T]
    d = np.hypot(xs - 15.5, ys - 15.5)
    rings = (np.sin(d * 1.3) + 1) / 2
    img = rgba(ramp(rings * 0.5 + 0.3, [wood, (196, 156, 104)]))
    img[d > 14, :3] = bark
    return speckle(img, s)


def leaves(s, colors=((34, 74, 30), (54, 104, 38), (86, 136, 52))):
    img = noisy(s, list(colors), (4, 2))
    holes = np.random.default_rng(s).random((T, T)) < 0.12
    img[holes, 3] = 0
    return img


def planks(s):
    img = noisy(s, [(128, 94, 56), (158, 120, 72)], (16, 2))
    ys, xs = np.mgrid[0:T, 0:T]
    img[(ys % 8) == 7, :3] *= 0.6
    img[((xs + (ys // 8) * 11) % 32) == 0, :3] *= 0.6
    return img


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


def gravel(s):
    img = rgba(ramp(np.random.default_rng(s).random((T, T)) * 0.6 + fbm(s, (4, 2)) * 0.4,
                    [(80, 76, 72), (120, 114, 108), (150, 144, 138)]))
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
]


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
        f.write("    },\n)\n")
    print(f"wrote {len(only) if only else len(TILES)} tiles")


if __name__ == "__main__":
    main()
