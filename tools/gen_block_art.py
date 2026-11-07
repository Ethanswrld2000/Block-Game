#!/usr/bin/env python3
"""Paint the block spritesheet.

Writes assets/textures/blocks.png (16 x 16 tiles of 32 x 32 px = 512 x 512)
and assets/textures/blocks.ron (tile name -> index).

Every tile is procedural pixel art: a small hand-picked palette per material,
seamless (toroidal) noise and Voronoi cells for shapes, and a top-left light
for bevels, so the whole sheet reads as one set. Faces repeat their tile per
block, so every solid tile wraps without seams.

Tile positions are part of the format: append new tiles at the end of TILES,
never reorder. Run from the repo root:  python3 tools/gen_block_art.py
Pass --preview to also write a zoomed, labelled preview to /tmp/blocks_preview.png.
"""
import os
import sys
import numpy as np
from PIL import Image

T = 32          # tile size in pixels
COLS = 16       # tiles per row
ROWS = 16
YS, XS = np.mgrid[0:T, 0:T]

# 4x4 ordered-dither matrix, used lightly so gradients become pixel patterns.
BAYER = (np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]) + 0.5) / 16 - 0.5
BAYER = np.tile(BAYER, (T // 4, T // 4))


def hexes(*codes):
    return [tuple(int(c[i:i + 2], 16) for i in (1, 3, 5)) for c in codes]


# ---- palettes (dark -> light) -------------------------------------------
P = {
    "grass": hexes("#2c4a22", "#3a6128", "#4a782f", "#5b8c36", "#72a342", "#8ab854"),
    "dirt": hexes("#3b2a1e", "#4d3726", "#5f4530", "#72543a", "#866647", "#9a7955"),
    "stone": hexes("#4a4b50", "#5d5f64", "#717378", "#85878c", "#9a9ca0", "#b0b2b5"),
    "cobble": hexes("#2e2f33", "#4b4c51", "#606268", "#777980", "#8e9097", "#a8aab0"),
    "sand": hexes("#a88a5c", "#bc9e6c", "#cdb07c", "#dbc08c", "#e6cf9e", "#efdcb0"),
    "mud": hexes("#211c15", "#2d261c", "#3a3123", "#483d2b", "#574a34", "#6b5c42"),
    "moss": hexes("#1f3a1c", "#2b4d23", "#386129", "#46742f", "#5a8a37", "#78a344"),
    "peat": hexes("#1e1510", "#2a1e16", "#36281c", "#433222", "#523d29", "#634a31"),
    "bark": hexes("#2e1f14", "#3e2b1b", "#4f3823", "#62472d", "#755737", "#886742"),
    "wood": hexes("#7a5532", "#8e663d", "#a27849", "#b58a56", "#c69c64", "#d6ae74"),
    "leaves": hexes("#18321a", "#224322", "#2d552a", "#3a6a31", "#4b803a", "#629746"),
    "planks": hexes("#5a3c22", "#6e4b2b", "#835b35", "#976b3f", "#aa7c4a", "#bc8d56"),
    "water": hexes("#163d6e", "#1c4c86", "#235c9c", "#2e6eb0", "#3f84c2", "#62a2d6"),
    "snow": hexes("#b9c6d6", "#cbd6e3", "#dbe3ee", "#e8eef5", "#f3f6fa", "#fdfeff"),
    "ice": hexes("#6f9fcf", "#80afdb", "#93bfe4", "#a8ceec", "#bfdcf2", "#dcedf9"),
    "gravel": hexes("#3c3a38", "#545150", "#6a6764", "#807c78", "#97928d", "#aea9a3"),
    "clay": hexes("#6d7484", "#7b8293", "#8990a1", "#979eae", "#a5acba", "#b4bac6"),
    "sky": hexes("#7c84a8", "#8f98bb", "#a3abcb", "#b6bed9", "#c8cfe5", "#dce1ef"),
    "cloud": hexes("#8fb39a", "#a3c4a9", "#b6d3b8", "#c8e0c7", "#d9ebd6", "#eaf5e6"),
    "deep": hexes("#16161d", "#1e1e28", "#272734", "#313142", "#3c3c50", "#48485f"),
    "sbark": hexes("#1f211a", "#2a2d22", "#35392b", "#414634", "#4e543e", "#5c6349"),
    "swood": hexes("#4e4a33", "#5d583d", "#6c6747", "#7b7651", "#8a855c", "#999468"),
    "sleaves": hexes("#202b17", "#2b381d", "#364624", "#42552b", "#506533", "#60763c"),
    "reed": hexes("#3d5424", "#4d6a2c", "#5f8034", "#73953f", "#8aa84c"),
}


# ---- tileable building blocks -------------------------------------------

def rng(seed):
    return np.random.default_rng(seed)


def value_noise(cells, seed):
    """Seamless value noise with `cells` lattice cells across the tile, in [0, 1]."""
    g = rng(seed).random((cells, cells))
    x, y = XS * cells / T, YS * cells / T
    x0, y0 = x.astype(int) % cells, y.astype(int) % cells
    x1, y1 = (x0 + 1) % cells, (y0 + 1) % cells
    fx, fy = x - np.floor(x), y - np.floor(y)
    fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a = g[y0, x0] * (1 - fx) + g[y0, x1] * fx
    b = g[y1, x0] * (1 - fx) + g[y1, x1] * fx
    return a * (1 - fy) + b * fy


def fbm(seed, cells=(2, 4, 8, 16), gain=0.5):
    out, w, amp = np.zeros((T, T)), 0.0, 1.0
    for i, c in enumerate(cells):
        out += value_noise(c, seed + i * 101) * amp
        w += amp
        amp *= gain
    return out / w


def normalize(f):
    lo, hi = f.min(), f.max()
    return (f - lo) / (hi - lo + 1e-9)


def jitter_points(n, seed, jitter=0.8):
    """n*n points, one per grid cell, jittered: even coverage, no clumps."""
    r = rng(seed)
    step = T / n
    pts = []
    for gy in range(n):
        for gx in range(n):
            pts.append(((gx + 0.5 + (r.random() - 0.5) * jitter) * step,
                        (gy + 0.5 + (r.random() - 0.5) * jitter) * step))
    return np.array(pts)


def voronoi(pts, sx=1.0, sy=1.0):
    """Toroidal Voronoi: (F1, F2, nearest index). sx/sy stretch the metric."""
    d = np.full((len(pts), T, T), np.inf)
    for i, (px, py) in enumerate(pts):
        dx = np.abs(XS - px)
        dy = np.abs(YS - py)
        dx = np.minimum(dx, T - dx) * sx
        dy = np.minimum(dy, T - dy) * sy
        d[i] = np.hypot(dx, dy)
    order = np.argsort(d, axis=0)
    f = np.take_along_axis(d, order[:2], axis=0)
    return f[0], f[1], order[0]


def shade(height, strength=1.0):
    """Light from the top-left: positive where the surface faces the light."""
    return (height - np.roll(np.roll(height, -1, 0), -1, 1)) * strength


def outline_edges(mask):
    """Pixels inside mask whose right or bottom neighbour is outside (bottom-right rim)."""
    return mask & ~(np.roll(mask, -1, 0) & np.roll(mask, -1, 1))


def quant(field, pal, dither=0.0, alpha=255):
    """Map field in [0,1] onto palette indices, with optional ordered dither."""
    pal = np.array(pal, dtype=float)
    n = len(pal)
    t = np.clip(field + BAYER * dither, 0, 0.9999) * n
    img = pal[t.astype(int)]
    a = np.full((T, T, 1), float(alpha)) if np.isscalar(alpha) else alpha[..., None].astype(float)
    return np.concatenate([img, a], axis=-1)


def blank():
    return np.zeros((T, T, 4))


def put(img, x, y, color, a=255):
    img[y % T, x % T] = (*color, a)


# ---- earth ---------------------------------------------------------------

def grass_top(s, pal=P["grass"]):
    base = fbm(s, (2, 4, 8)) * 0.65 + value_noise(16, s + 3) * 0.35
    img = quant(normalize(base) * 0.85 + 0.08, pal, 0.22)
    r = rng(s + 9)
    # little blades: a light pixel over a dark one, pointing up
    for _ in range(46):
        x, y = r.integers(0, T, 2)
        put(img, x, y, pal[5] if r.random() < 0.5 else pal[4])
        put(img, x, y + 1, pal[1])
    return img


def dirt(s):
    base = fbm(s, (2, 4, 8, 16))
    img = quant(normalize(base) * 0.6 + 0.15, P["dirt"], 0.25)
    r = rng(s + 5)
    for _ in range(9):     # pebbles: lit top-left, shadow bottom-right
        x, y = r.integers(0, T, 2)
        put(img, x, y, P["dirt"][5])
        put(img, x + 1, y, P["dirt"][4])
        put(img, x, y + 1, P["dirt"][4])
        put(img, x + 1, y + 1, P["dirt"][1])
    for _ in range(26):    # dark crumbs
        x, y = r.integers(0, T, 2)
        put(img, x, y, P["dirt"][0])
    return img


def grass_side(s):
    img = dirt(s)
    top = grass_top(s + 5)
    r = rng(s + 11)
    # a ragged fringe 3..8 px deep, smooth-ish from column to column, wrapping
    depth = (4 + value_noise(8, s + 7)[0] * 3).astype(int)
    for x in range(T):
        d = depth[x] + (2 if r.random() < 0.18 else 0)
        img[:d, x] = top[:d, x]
        img[d - 1, x, :3] = P["grass"][1]          # dark lip under the turf
        img[d, x, :3] = P["dirt"][0]               # shadow on the soil
        if r.random() < 0.5:
            img[d + 1, x, :3] = P["dirt"][1]
    return img


def sand(s):
    ripple = np.sin((XS * 0.35 + YS) * 2 * np.pi / 8 + value_noise(4, s) * 3)
    base = fbm(s, (4, 8, 16)) * 0.7 + (ripple + 1) / 2 * 0.3
    img = quant(normalize(base) * 0.7 + 0.15, P["sand"], 0.3)
    r = rng(s + 2)
    for _ in range(20):
        x, y = r.integers(0, T, 2)
        put(img, x, y, P["sand"][0] if r.random() < 0.6 else (226, 214, 196))
    return img


def mud(s):
    base = fbm(s, (2, 4, 8))
    img = quant(normalize(base) * 0.75 + 0.1, P["mud"], 0.2)
    wet = value_noise(4, s + 2) * 0.7 + value_noise(8, s + 5) * 0.3
    puddle = wet > 0.62
    img[puddle, :3] = img[puddle, :3] * 0.7 + np.array((40, 52, 44)) * 0.3
    # glossy glints on the wet patches, upper-left of each puddle
    edge = puddle & ~np.roll(np.roll(puddle, 1, 0), 1, 1) & (rng(s + 9).random((T, T)) < 0.3)
    img[edge, :3] = (96, 104, 86)
    r = rng(s + 8)
    for _ in range(14):
        x, y = r.integers(0, T, 2)
        put(img, x, y, P["mud"][0])
    return img


def moss(s):
    pts = jitter_points(5, s, 0.9)
    f1, f2, idx = voronoi(pts)
    lump = 1 - np.clip(f1 / 5.5, 0, 1)
    h = lump * 0.6 + fbm(s + 4, (4, 8, 16)) * 0.4
    t = normalize(h * 0.7 + shade(h, 1.8) + 0.15)
    img = quant(t * 0.8 + 0.1, P["moss"], 0.25)
    img[(f2 - f1) < 0.8, :3] = P["moss"][0]       # crevices between clumps
    r = rng(s + 3)
    for _ in range(12):                            # spore tips
        x, y = r.integers(0, T, 2)
        put(img, x, y, (164, 176, 82))
    return img


def peat(s):
    warp = value_noise(4, s) * 4
    layers = np.sin((YS + warp) * 2 * np.pi / 6.4) * 0.5 + 0.5
    base = layers * 0.45 + fbm(s + 1, (4, 8, 16)) * 0.55
    img = quant(normalize(base) * 0.8 + 0.1, P["peat"], 0.25)
    r = rng(s + 6)
    for _ in range(9):       # pale root fibres running sideways
        x, y = r.integers(0, T, 2)
        for i in range(r.integers(3, 8)):
            y += r.integers(-1, 2) if r.random() < 0.3 else 0
            put(img, x + i, y, (122, 98, 66))
    for _ in range(5):       # bits of half-rotted plant, greenish
        x, y = r.integers(0, T, 2)
        put(img, x, y, (70, 74, 40))
        put(img, x + 1, y, (58, 62, 34))
    return img


def clay(s):
    warp = fbm(s, (2, 4)) * 5
    layers = np.sin((YS + warp) * 2 * np.pi / 8) * 0.5 + 0.5
    base = fbm(s + 1, (2, 4, 8)) * 0.6 + layers * 0.4
    img = quant(normalize(base) * 0.6 + 0.2, P["clay"], 0.2)
    r = rng(s + 4)
    for _ in range(10):
        x, y = r.integers(0, T, 2)
        put(img, x, y, P["clay"][0])
        put(img, x + 1, y, P["clay"][1])
    return img


def gravel(s, pal=P["gravel"]):
    pts = jitter_points(6, s, 1.0)
    f1, f2, idx = voronoi(pts)
    r = rng(s + 1)
    tint = r.random(len(pts))
    dome = 1 - np.clip(f1 / 4.0, 0, 1)
    h = dome * 0.6 + tint[idx] * 0.4
    t = h * 0.5 + shade(dome, 2.2) + 0.25 + value_noise(16, s) * 0.1
    img = quant(np.clip(t, 0, 1), pal, 0.15)
    img[(f2 - f1) < 1.0, :3] = pal[0]
    warm = tint[idx] > 0.75                       # a few brownish pebbles
    img[warm, :3] = img[warm, :3] * 0.85 + np.array((120, 96, 70)) * 0.15
    return img


# ---- stone ---------------------------------------------------------------

def stone(s, pal=P["stone"], crack=0.55):
    base = fbm(s, (2, 4, 8, 16))
    pts = jitter_points(3, s + 7, 1.0)
    f1, f2, idx = voronoi(pts, 1.0, 1.4)
    f2 = f2 + (value_noise(8, s + 5) - 0.5) * 0.9
    slab = rng(s + 2).random(len(pts))[idx] * 0.18
    h = base + slab
    t = normalize(h + shade(base, 2.0)) * 0.55 + 0.2
    img = quant(t, pal, 0.22)
    cracks = ((f2 - f1) < crack) & (value_noise(4, s + 6) > 0.35)
    img[cracks, :3] = pal[1]
    lit = np.roll(cracks, 1, 0) & ~cracks          # lit lip just below a crack
    img[lit, :3] = img[lit, :3] * 0.5 + np.array(pal[4]) * 0.5
    return img


def cobble(s, pal=P["cobble"]):
    pts = jitter_points(4, s, 0.75)
    f1, f2, idx = voronoi(pts)
    edge = f2 - f1
    rad = np.clip(f1 / ((f1 + f2) / 2 + 1e-9), 0, 1)
    dome = np.sqrt(1 - rad ** 2)                   # each stone a rounded cap
    r = rng(s + 3)
    tint = r.random(len(pts)) * 0.2
    t = dome * 0.45 + tint[idx] + np.clip(shade(dome, 1.5), -0.2, 0.2) + fbm(s + 8, (8, 16)) * 0.15 + 0.15
    img = quant(np.clip(t, 0, 1), pal, 0.15)
    img[edge < 1.0, :3] = pal[0]                  # mortar
    return img


def ore(s, base_fn, metal, glint, shadow, count=5):
    img = base_fn(s)
    r = rng(s + 77)
    pts = jitter_points(3, s + 40, 0.9)
    r.shuffle(pts)
    for cx, cy in pts[:count]:
        cx, cy = int(cx), int(cy)
        blob = [(0, 0), (1, 0), (0, 1), (1, 1), (-1, 0), (0, -1), (2, 1), (1, 2), (-1, 1), (1, -1)]
        for dx, dy in blob[: r.integers(6, 11)]:
            put(img, cx + dx, cy + dy, metal)
        for dx, dy in [(2, 2), (2, 0), (0, 2), (-1, 2)]:
            if r.random() < 0.7:
                put(img, cx + dx, cy + dy, shadow)
        put(img, cx, cy, glint)
        if r.random() < 0.6:
            put(img, cx - 1, cy, glint)
    return img


def skystone(s):
    img = stone(s, P["sky"], 0.45)
    vein = np.abs(fbm(s + 4, (2, 4, 8)) - 0.5) < 0.011
    img[vein, :3] = (232, 206, 128)
    halo = (np.roll(vein, 1, 1) | np.roll(vein, -1, 1)) & ~vein
    img[halo, :3] = img[halo, :3] * 0.6 + np.array((240, 226, 170)) * 0.4
    r = rng(s + 2)
    for _ in range(6):
        x, y = r.integers(0, T, 2)
        put(img, x, y, (255, 248, 220))
    return img


def deeprock(s):
    warp = value_noise(4, s) * 6
    strata = np.sin((YS * 0.9 + XS * 0.15 + warp) * 2 * np.pi / 10) * 0.5 + 0.5
    base = fbm(s + 1, (2, 4, 8, 16)) * 0.6 + strata * 0.4
    t = normalize(base + shade(base, 2.5)) * 0.7 + 0.15
    img = quant(t, P["deep"], 0.25)
    pts = jitter_points(3, s + 9, 1.0)
    f1, f2, _ = voronoi(pts, 1.0, 2.0)
    img[(f2 - f1) < 0.5, :3] = (8, 8, 12)
    r = rng(s + 3)
    for _ in range(7):     # faint violet mineral glints
        x, y = r.integers(0, T, 2)
        put(img, x, y, (104, 84, 140))
    return img


# ---- wood and plants -------------------------------------------------------

def bark(s, pal, moss_color=None):
    warp = value_noise(4, s) * 5 + value_noise(8, s + 1) * 2
    # bark plates run vertically: stretch the noise along y
    ridge = np.sin((XS + warp) * 2 * np.pi / 6.4) * 0.5 + 0.5
    grain = value_noise(8, s + 2)[:, :] * 0.3
    h = ridge * 0.65 + grain + fbm(s + 3, (4, 16)) * 0.2
    t = normalize(h + shade(h, 1.5)) * 0.8 + 0.1
    img = quant(t, pal, 0.2)
    img[ridge < 0.12, :3] = pal[0]                # deep grooves
    r = rng(s + 4)
    for _ in range(4):                            # cross-cracks in the plates
        x, y = r.integers(0, T, 2)
        for i in range(3):
            put(img, x + i, y, pal[1])
    if moss_color is not None:
        mossy = (fbm(s + 9, (2, 4, 8)) > 0.56) & (ridge > 0.3)
        img[mossy, :3] = np.array(moss_color) * (0.85 + 0.3 * value_noise(16, s + 5)[mossy, None])
    return img


def log_top(s, bark_pal, wood_pal):
    cx = cy = 15.5
    dx, dy = np.abs(XS - cx), np.abs(YS - cy)
    d = (dx ** 4 + dy ** 4) ** 0.25 * 0.6 + np.hypot(dx, dy) * 0.4
    ang = np.arctan2(YS - cy, XS - cx)
    wobble = d + np.sin(ang * 3 + s) * 0.6 + value_noise(4, s) * 1.2
    rings = (np.sin(wobble * 2 * np.pi / 3.2) * 0.5 + 0.5) ** 1.5
    t = rings * 0.55 + (1 - d / 16) * 0.3 + 0.1
    img = quant(np.clip(t, 0, 1), wood_pal, 0.15)
    rim = np.maximum(dx, dy) > 13.6
    img[rim] = quant(fbm(s + 2, (4, 8, 16)) * 0.6 + 0.1, bark_pal, 0.3)[rim]
    img[(np.maximum(dx, dy) > 12.6) & ~rim, :3] = wood_pal[0]
    img[d < 1.2, :3] = wood_pal[0]                 # pith
    r = rng(s + 3)
    for _ in range(2):                              # radial checks (drying cracks)
        a = r.random() * 2 * np.pi
        for k in range(3, 9):
            put(img, int(cx + np.cos(a) * k), int(cy + np.sin(a) * k), wood_pal[1])
    return img


def leaves(s, pal, holes=0.16, droop=False):
    pts = jitter_points(6, s, 1.0)
    f1, f2, idx = voronoi(pts, 1.0, 1.25 if not droop else 0.8)
    r = rng(s + 1)
    tint = r.random(len(pts)) * 0.3
    lobe = 1 - np.clip(f1 / 4.2, 0, 1)
    t = lobe * 0.45 + tint[idx] + shade(lobe, 2.6) + 0.15
    img = quant(np.clip(t, 0, 1), pal, 0.2)
    img[(f2 - f1) < 0.7, :3] = pal[0]
    gaps = (f1 > 3.6) & (value_noise(8, s + 2) < holes * 3)
    img[gaps, 3] = 0
    if droop:  # hanging strands
        for _ in range(5):
            x, y = r.integers(0, T, 2)
            for k in range(r.integers(4, 9)):
                put(img, x, y + k, pal[2] if k % 3 else pal[3])
    for _ in range(8):  # sunlit leaf tips
        x, y = r.integers(0, T, 2)
        if img[y, x, 3] > 0:
            put(img, x, y, pal[5])
    return img


def planks(s, pal=P["planks"]):
    img = blank()
    r = rng(s)
    board_h = 8
    for b in range(T // board_h):
        y0 = b * board_h
        seam = int(r.integers(0, T))
        tone = r.random() * 0.15
        grain = np.sin((YS[y0:y0 + board_h] * 2.2 + value_noise(4, s + b)[y0:y0 + board_h] * 6)) * 0.5 + 0.5
        g = grain * 0.35 + value_noise(16, s + 10 + b)[y0:y0 + board_h] * 0.25 + tone + 0.3
        sub = quant(np.pad(g, ((0, T - board_h), (0, 0)), mode="edge"), pal, 0.2)[:board_h]
        img[y0:y0 + board_h] = sub
        img[y0, :, :3] = pal[5]                    # lit top edge
        img[y0 + board_h - 1, :, :3] = pal[0]      # shadow gap below
        img[y0:y0 + board_h, seam, :3] = pal[0]    # board end
        img[y0 + 1:y0 + board_h - 1, (seam + 1) % T, :3] = pal[4]
        for nx in ((seam + 3) % T, (seam - 3) % T):  # nails either side of the seam
            put(img, nx, y0 + 3, (58, 50, 44))
            put(img, nx, y0 + 4, (88, 76, 66))
    return img


def reed(s):
    img = blank()
    r = rng(s)
    pal = P["reed"]
    xs = r.choice(np.arange(1, T - 2, 2), 9, replace=False)
    for x in xs:
        h = int(r.integers(14, 31))
        lean = r.choice([-1, 0, 1])
        for k in range(h):
            y = T - 1 - k
            xx = int(x + (lean * k) // 14)
            put(img, xx, y, pal[1 + (k // 3) % 3])
            put(img, xx + 1, y, pal[0])
        if r.random() < 0.55:                       # brown seed head
            top = T - h
            xx = int(x + (lean * h) // 14)
            for k in range(5):
                put(img, xx, top + k, (96, 62, 36))
                put(img, xx + 1, top + k, (70, 44, 26))
            put(img, xx, top - 1, pal[2])
    return img


def torch(s):
    img = blank()
    for y in range(13, 31):                         # stick: 2 px, lit left side
        put(img, 15, y, (124, 90, 54))
        put(img, 16, y, (86, 60, 36))
    put(img, 15, 30, (66, 46, 28))
    put(img, 16, 30, (66, 46, 28))
    for y in range(11, 14):                         # wrapped head
        put(img, 14, y, (60, 44, 30))
        put(img, 15, y, (82, 60, 38))
        put(img, 16, y, (60, 44, 30))
        put(img, 17, y, (46, 34, 22))
    flame = {
        (15, 4): 2, (16, 5): 2, (14, 6): 2, (15, 5): 1, (15, 6): 1, (16, 6): 1, (17, 7): 2,
        (14, 7): 1, (15, 7): 0, (16, 7): 0, (17, 8): 1, (14, 8): 1, (15, 8): 0, (16, 8): 0,
        (14, 9): 1, (15, 9): 0, (16, 9): 0, (17, 9): 1, (14, 10): 2, (15, 10): 1, (16, 10): 1,
        (17, 10): 2, (13, 8): 2, (18, 9): 2,
    }
    colors = [(255, 246, 196), (255, 196, 72), (226, 106, 34)]
    for (x, y), c in flame.items():
        put(img, x, y, colors[c])
    return img


def glowcap(s):
    img = blank()
    for y in range(17, 31):                         # stem, slightly curved
        x = 15 + (1 if y > 25 else 0)
        put(img, x, y, (214, 208, 186))
        put(img, x + 1, y, (176, 168, 146))
    for x in range(12, 21):                         # gills under the cap
        put(img, x, 17, (40, 108, 118))
    cap_hi, cap_mid, cap_lo = (164, 248, 240), (78, 210, 214), (34, 140, 160)
    for y in range(8, 17):
        half = int(np.sqrt(max(0.0, 1 - ((y - 16.5) / 8.5) ** 2)) * 9)
        for x in range(16 - half, 16 + half):
            c = cap_mid
            if (x - 13) ** 2 + (y - 10) ** 2 < 6:
                c = cap_hi
            elif y >= 15 or x >= 16 + half - 2:
                c = cap_lo
            put(img, x, y, c)
    for x, y in [(12, 13), (18, 11), (15, 14), (20, 14)]:   # glowing spots
        put(img, x, y, (232, 255, 214))
    # a small second cap
    for y in range(22, 31):
        put(img, 8, y, (200, 194, 172))
    for y in range(19, 23):
        half = 3 if y > 19 else 2
        for x in range(8 - half, 9 + half):
            put(img, x, y, cap_mid if y < 22 else cap_lo)
    put(img, 7, 20, cap_hi)
    return img


# ---- water, snow, sky ------------------------------------------------------

def water(s):
    base = fbm(s, (2, 4, 8))
    waves = np.sin((YS + value_noise(4, s + 1) * 6) * 2 * np.pi / 8) * 0.5 + 0.5
    t = base * 0.6 + waves * 0.4
    img = quant(normalize(t) * 0.75 + 0.1, P["water"], 0.2, alpha=176)
    crest = (waves > 0.93) & (value_noise(8, s + 2) > 0.45)
    img[crest, :3] = P["water"][5]
    img[crest, 3] = 200
    return img


def snow(s):
    pts = jitter_points(4, s, 1.0)
    f1, _, _ = voronoi(pts)
    drift = 1 - np.clip(f1 / 8, 0, 1)
    h = drift * 0.4 + fbm(s + 1, (2, 4, 8)) * 0.6
    t = normalize(h + shade(h, 2.5)) * 0.75 + 0.22
    img = quant(t, P["snow"], 0.2)
    r = rng(s + 2)
    for _ in range(10):
        x, y = r.integers(0, T, 2)
        put(img, x, y, (255, 255, 255))
    return img


def ice(s):
    base = fbm(s, (2, 4))
    img = quant(normalize(base) * 0.55 + 0.25, P["ice"], 0.15, alpha=206)
    pts = jitter_points(3, s + 3, 1.0)
    f1, f2, _ = voronoi(pts)
    crack = (f2 - f1) < 0.9
    img[crack, :3] = P["ice"][5]
    img[crack, 3] = 232
    # streaky reflections, diagonal
    streak = ((XS + YS + (value_noise(4, s) * 6).astype(int)) % 16) < 2
    img[streak & ~crack, :3] = img[streak & ~crack, :3] * 0.6 + np.array(P["ice"][5]) * 0.4
    return img


def cloud_moss(s):
    pts = jitter_points(4, s, 1.0)
    f1, f2, _ = voronoi(pts)
    puff = 1 - np.clip(f1 / 6.5, 0, 1)
    h = puff * 0.6 + fbm(s + 1, (4, 8, 16)) * 0.4
    t = normalize(h + shade(h, 2.4)) * 0.7 + 0.2
    img = quant(t, P["cloud"], 0.2)
    img[(f2 - f1) < 0.7, :3] = P["cloud"][1]
    r = rng(s + 5)
    for _ in range(8):     # tiny pale-gold flowers
        x, y = r.integers(0, T, 2)
        put(img, x, y, (246, 236, 168))
    return img


# Order is the tile index. Append only.
TILES = [
    ("grass_top", grass_top), ("grass_side", grass_side), ("dirt", dirt), ("stone", stone),
    ("cobblestone", cobble), ("sand", sand), ("mud", mud), ("moss", moss),
    ("peat", peat), ("log_side", lambda s: bark(s, P["bark"])),
    ("log_top", lambda s: log_top(s, P["bark"], P["wood"])),
    ("leaves", lambda s: leaves(s, P["leaves"])),
    ("planks", planks), ("water", water), ("snow", snow), ("ice", ice),
    ("gravel", gravel), ("clay", clay),
    ("coal_ore", lambda s: ore(s, stone, (28, 28, 32), (96, 100, 112), (14, 14, 16))),
    ("iron_ore", lambda s: ore(s, stone, (198, 142, 102), (238, 206, 176), (128, 80, 54))),
    ("reed", reed), ("torch", torch), ("skystone", skystone), ("cloud_moss", cloud_moss),
    ("deeprock", deeprock),
    ("swamp_log_side", lambda s: bark(s, P["sbark"], moss_color=(70, 96, 44))),
    ("swamp_log_top", lambda s: log_top(s, P["sbark"], P["swood"])),
    ("swamp_leaves", lambda s: leaves(s, P["sleaves"], holes=0.2, droop=True)),
    ("glowcap", glowcap),
]


def build_sheet():
    sheet = np.zeros((ROWS * T, COLS * T, 4))
    for i, (name, fn) in enumerate(TILES):
        tile = np.clip(np.round(fn(1000 + i * 17)), 0, 255)
        r, c = divmod(i, COLS)
        sheet[r * T:(r + 1) * T, c * T:(c + 1) * T] = tile
    return sheet.astype(np.uint8)


def preview(sheet, path, scale=4):
    """Each tile 2x2-repeated (to show seams), zoomed, on a dark card, with labels."""
    from PIL import ImageDraw
    cols = 6
    cell = T * 2 * scale
    pad, label = 14, 18
    rows = (len(TILES) + cols - 1) // cols
    out = Image.new("RGB", (cols * (cell + pad) + pad, rows * (cell + pad + label) + pad), (30, 32, 36))
    draw = ImageDraw.Draw(out)
    checker = ((XS // 4 + YS // 4) % 2)[..., None] * 18 + 52
    for i, (name, _) in enumerate(TILES):
        r, c = divmod(i, COLS)
        tile = sheet[r * T:(r + 1) * T, c * T:(c + 1) * T].astype(float)
        a = tile[..., 3:4] / 255
        rgb = tile[..., :3] * a + np.repeat(checker, 3, -1) * (1 - a)
        img = Image.fromarray(np.tile(rgb, (2, 2, 1)).astype(np.uint8)).resize((cell, cell), Image.NEAREST)
        pr, pc = divmod(i, cols)
        x, y = pad + pc * (cell + pad), pad + pr * (cell + pad + label)
        out.paste(img, (x, y))
        draw.text((x, y + cell + 3), f"{i}  {name}", fill=(220, 222, 226))
    out.save(path)


def main():
    root = os.path.join(os.path.dirname(__file__), "..", "assets", "textures")
    os.makedirs(root, exist_ok=True)
    sheet = build_sheet()
    Image.fromarray(sheet, "RGBA").save(os.path.join(root, "blocks.png"), optimize=True)
    with open(os.path.join(root, "blocks.ron"), "w") as f:
        f.write("// Tile name -> index into blocks.png (row-major, 16 tiles per row, 32x32 px each).\n")
        f.write("// Generated by tools/gen_block_art.py. Safe to edit by hand.\n(\n    tile_size: 32,\n    columns: 16,\n    tiles: {\n")
        for i, (name, _) in enumerate(TILES):
            f.write(f'        "{name}": {i},\n')
        f.write("    },\n)\n")
    print(f"wrote {len(TILES)} tiles")
    if "--preview" in sys.argv:
        out = os.environ.get("PREVIEW", "/tmp/blocks_preview.png")
        preview(sheet, out)
        print(f"preview: {out}")


if __name__ == "__main__":
    main()
