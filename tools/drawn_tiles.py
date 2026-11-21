"""Drawn block tiles for gen_placeholder_art.py.

Each tile is built the way hand-made 32x packs are: separate shapes (stones,
clods, planks, leaves, blades), a small hue-shifted palette per material
(shadows lean cool, highlights warm), light from the top left (a bright upper
left edge and a dark lower right edge on every shape) and no smooth noise left
in the final pixels. Everything wraps, so tiles repeat without seams.

Painters return a T x T x 4 float RGBA array, like the ones in
gen_placeholder_art.py. Work happens in palette indices (`idx`), turned into
colours at the end by `paint`.
"""
import colorsys

import numpy as np

T = 32
YS, XS = np.mgrid[0:T, 0:T]


# ---- palettes --------------------------------------------------------------

def shifted_ramp(dark, light, n=6, shift=0.04):
    """n colours from dark to light; the dark end leans cooler (towards blue),
    the light end warmer (towards yellow), the way painters shade."""
    out = []
    for i in range(n):
        t = i / (n - 1)
        rgb = np.array(dark, float) * (1 - t) + np.array(light, float) * t
        h, l, s = colorsys.rgb_to_hls(*(rgb / 255))
        # Move the hue towards blue (0.62) in the darks and yellow (0.14) in the lights.
        target = 0.62 if t < 0.5 else 0.14
        amount = shift * abs(t - 0.5) * 2
        dh = ((target - h + 0.5) % 1.0) - 0.5
        h = (h + dh * amount * 4) % 1.0
        s = min(1.0, s * (1.0 + 0.15 * (0.5 - abs(t - 0.5))))
        out.append(np.array(colorsys.hls_to_rgb(h, l, s)) * 255)
    return np.array(out)


def paint(idx, palette, alpha=None):
    idx = np.clip(np.round(idx).astype(int), 0, len(palette) - 1)
    img = np.zeros((T, T, 4))
    img[..., :3] = palette[idx]
    img[..., 3] = 255 if alpha is None else alpha
    return img


# ---- shape helpers ---------------------------------------------------------

def noise(scale, seed):
    """Tileable value noise in [0, 1]; `scale` is (x, y) cell size or one number."""
    sx, sy = (scale, scale) if np.isscalar(scale) else scale
    r = np.random.default_rng(seed)
    nx, ny = max(1, T // sx), max(1, T // sy)
    grid = r.random((ny, nx))
    xs, ys = XS / sx, YS / sy
    x0, y0 = xs.astype(int) % nx, ys.astype(int) % ny
    x1, y1 = (x0 + 1) % nx, (y0 + 1) % ny
    fx, fy = xs - np.floor(xs), ys - np.floor(ys)
    fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a = grid[y0, x0] * (1 - fx) + grid[y0, x1] * fx
    b = grid[y1, x0] * (1 - fx) + grid[y1, x1] * fx
    return a * (1 - fy) + b * fy


def jittered_points(seed, nx, ny, jitter=0.8):
    """About evenly spread points: one per grid cell, nudged at random."""
    r = np.random.default_rng(seed)
    pts = []
    for j in range(ny):
        for i in range(nx):
            px = (i + 0.5 + (r.random() - 0.5) * jitter) * T / nx
            py = (j + 0.5 + (r.random() - 0.5) * jitter) * T / ny
            pts.append((px, py))
    return np.array(pts)


def voronoi(pts, stretch=(1.0, 1.0), wobble=0.0, seed=0):
    """Wrapped cells around `pts`: (label, distance to own point, distance to
    the nearest edge between cells)."""
    sx, sy = stretch
    x = XS + 0.5
    y = YS + 0.5
    if wobble:
        x = x + (noise(8, seed + 1) - 0.5) * wobble
        y = y + (noise(8, seed + 2) - 0.5) * wobble
    d = np.empty((len(pts),) + XS.shape)
    for k, (px, py) in enumerate(pts):
        dx = np.abs(x - px)
        dy = np.abs(y - py)
        dx = np.minimum(dx, T - dx) * sx
        dy = np.minimum(dy, T - dy) * sy
        d[k] = np.hypot(dx, dy)
    order = np.argsort(d, axis=0)
    label = order[0]
    d1 = np.take_along_axis(d, order[:1], 0)[0]
    d2 = np.take_along_axis(d, order[1:2], 0)[0]
    return label, d1, (d2 - d1) / 2


def bevel(height):
    """-1, 0 or +1 per pixel from top-left lighting of a height field."""
    up_left = np.roll(np.roll(height, 1, 0), 1, 1)
    down_right = np.roll(np.roll(height, -1, 0), -1, 1)
    slope = (height - up_left) + (down_right - height)
    out = np.zeros_like(height)
    out[slope > 0.35] = 1
    out[slope < -0.35] = -1
    return out


def dither_mask(seed, density):
    return np.random.default_rng(seed).random((T, T)) < density


# ---- ground ----------------------------------------------------------------

GRASS = shifted_ramp((30, 54, 32), (150, 178, 72), 7)
DIRT = shifted_ramp((48, 32, 28), (156, 118, 78), 6)
STONE = shifted_ramp((56, 56, 64), (164, 160, 152), 6, 0.03)
SAND = shifted_ramp((170, 140, 96), (242, 228, 186), 5, 0.02)
MUD = shifted_ramp((28, 24, 22), (100, 86, 62), 6)
PEAT = shifted_ramp((34, 24, 20), (128, 94, 62), 6)
MOSS = shifted_ramp((28, 46, 28), (138, 162, 70), 6)
WOOD = shifted_ramp((66, 42, 28), (194, 154, 100), 6)


def grass_blades(seed, idx, count=170, base=3):
    """Short blades seen from above: a light tip, a darker foot down-right."""
    r = np.random.default_rng(seed)
    for _ in range(count):
        x, y = r.integers(0, T, 2)
        dx, dy = [(0, -1), (1, -1), (-1, -1), (1, 0)][r.integers(0, 4)]
        n = r.integers(2, 4)
        lift = r.choice([1, 1, 2])
        b = base[y, x] if not np.isscalar(base) else base
        for i in range(n):
            idx[(y + dy * i) % T, (x + dx * i) % T] = b + lift - (1 if i == 0 else 0)
        idx[(y + 1) % T, (x + 1) % T] = min(idx[(y + 1) % T, (x + 1) % T], b - 2)
    return idx


def grass_top(s):
    idx = np.full((T, T), 3.0)
    patches = noise(8, s) + noise(4, s + 3) * 0.5
    idx[patches > 0.95] = 4
    idx[patches < 0.55] = 2
    idx[patches < 0.35] = 1
    grass_blades(s + 7, idx, base=idx.copy())
    return paint(idx, GRASS)


def dirt_idx(s):
    idx = np.full((T, T), 3.0)
    clump = noise(8, s) + noise(4, s + 1) * 0.6
    idx[clump > 1.0] = 4
    idx[clump < 0.6] = 2
    # Clods: little lumps lit from the top left.
    pts = jittered_points(s + 2, 5, 5, 0.9)
    label, _, edge = voronoi(pts, wobble=3, seed=s)
    r = np.random.default_rng(s + 3)
    lift = r.choice([-1, 0, 0, 1], len(pts))
    idx += lift[label] * (edge > 1)
    soft = r.random(len(pts)) < 0.5
    idx += bevel(np.minimum(edge, 2.0)) * soft[label]
    crease = (edge < 0.5) & soft[label] & dither_mask(s + 5, 0.6)
    idx[crease] = np.minimum(idx[crease], 2)
    # Grit and a few pebbles.
    grit = dither_mask(s + 4, 0.06)
    idx[grit] -= 1
    pebbles = np.zeros((T, T), bool)
    for _ in range(r.integers(3, 6)):
        x, y = r.integers(0, T, 2)
        w, h = r.integers(2, 4), r.integers(1, 3)
        for yy in range(h):
            for xx in range(w):
                pebbles[(y + yy) % T, (x + xx) % T] = True
        idx[(y + h) % T, (x + w) % T] = 0
    return idx, pebbles


def dirt(s):
    idx, pebbles = dirt_idx(s)
    img = paint(idx, DIRT)
    pal = shifted_ramp((80, 74, 70), (168, 160, 148), 4)
    peb_idx = 2 + bevel(pebbles.astype(float) * 3)
    img[pebbles, :3] = pal[np.clip(peb_idx[pebbles].astype(int), 0, 3)]
    return img


def grass_side(s):
    img = dirt(s)
    r = np.random.default_rng(s + 11)
    depth = (4 + noise((4, 32), s + 12)[0] * 4).astype(int)
    for x in range(T):
        if r.random() < 0.22:
            depth[x] += r.integers(2, 5)  # a blade hanging over the edge
    idx = np.full((T, T), 3.0)
    for x in range(T):
        for y in range(depth[x]):
            # Upright blades: streaks down the face.
            idx[y, x] = 3 + (1 if (x * 7 + s) % 5 == 0 else 0) - (1 if (x * 3 + y + s) % 7 == 0 else 0)
        idx[0, x] = 4
        idx[depth[x] - 1, x] = 1  # dark lip at the bottom of the grass
    grass = paint(idx, GRASS)
    for x in range(T):
        img[: depth[x], x] = grass[: depth[x], x]
        # Shadow cast onto the dirt just under the grass.
        y = depth[x]
        if y < T:
            img[y, x, :3] *= 0.7
    return img


def stone(s):
    idx = np.full((T, T), 3.0)
    pts = jittered_points(s, 2, 3, 1.0)
    label, _, edge = voronoi(pts, stretch=(1.0, 1.4), wobble=8, seed=s)
    r = np.random.default_rng(s + 1)
    idx += r.choice([0, 0, -1], len(pts))[label] * 0.6
    idx += bevel(np.minimum(edge, 1.5)) * 1.0
    # Cracks: some of the edges between plates.
    cracked = r.random(len(pts)) < 0.45
    crack = (edge < 0.6) & cracked[label]
    idx[crack] = 0
    # Soft mottling, then a few lighter flecks.
    mottle = noise(4, s + 5) + noise(8, s + 6)
    idx[(mottle > 1.3) & ~crack] -= 1
    idx[(mottle < 0.55) & ~crack] += 1
    idx[dither_mask(s + 7, 0.04) & ~crack] += 1
    return paint(np.clip(idx, 0, 5), STONE)


def cobblestone(s):
    pts = jittered_points(s, 3, 3, 0.7)
    label, d1, edge = voronoi(pts, wobble=4, seed=s)
    r = np.random.default_rng(s + 1)
    tone = r.choice([2, 3, 3, 4], len(pts))
    height = np.minimum(edge, 4.0) + noise(4, s + 2) * 0.8
    idx = tone[label] + bevel(height) * 1.0 + (np.minimum(edge, 4.0) - 2.0) * 0.25
    idx[dither_mask(s + 3, 0.05)] -= 1
    mortar = edge < 1.0
    idx[mortar] = 0
    idx[(edge >= 1.0) & (edge < 1.6)] = np.minimum(idx[(edge >= 1.0) & (edge < 1.6)], 1)
    # Some stones a touch mossier/browner.
    img = paint(np.clip(idx, 0, 5), STONE)
    warm = r.random(len(pts)) < 0.3
    img[warm[label] & ~mortar, :3] *= np.array([1.05, 1.0, 0.92])
    return img


def sand(s):
    idx = np.full((T, T), 2.0)
    ripple = np.sin((XS + YS * 0.35 + noise(8, s) * 8) * (2 * np.pi / 8))
    idx[ripple > 0.93] = 3
    idx[(ripple < -0.97) & dither_mask(s + 4, 0.6)] = 1
    idx[dither_mask(s + 1, 0.10)] += 1
    idx[dither_mask(s + 2, 0.07)] -= 1
    r = np.random.default_rng(s + 3)
    for _ in range(4):  # a few dark grains
        x, y = r.integers(0, T, 2)
        idx[y, x] = 0
    return paint(idx, SAND)


def gravel(s):
    pts = jittered_points(s, 6, 6, 0.9)
    label, _, edge = voronoi(pts, wobble=2, seed=s)
    r = np.random.default_rng(s + 1)
    palettes = [
        shifted_ramp((52, 50, 52), (170, 164, 156), 6),
        shifted_ramp((58, 48, 40), (168, 146, 120), 6),
        shifted_ramp((40, 40, 46), (124, 122, 126), 6),
    ]
    kind = r.choice([0, 0, 1, 2], len(pts))
    tone = r.choice([2, 3, 3, 4], len(pts))
    idx = tone[label] + bevel(np.minimum(edge, 2.5)) * 1.0
    gap = edge < 0.7
    idx[gap] = 0
    img = np.zeros((T, T, 4))
    img[..., 3] = 255
    for k, pal in enumerate(palettes):
        m = kind[label] == k
        img[m, :3] = pal[np.clip(idx[m].astype(int), 0, 5)]
    img[gap, :3] = palettes[2][0]
    return img


def mud(s):
    idx = np.full((T, T), 3.0)
    blob = noise(8, s) + noise(4, s + 1) * 0.5
    idx[blob > 1.0] = 4
    idx[blob < 0.55] = 2
    # Small wet hollows: shadowed on the upper left inside, a glint lower right.
    wet = noise(4, s + 2) * 0.75 + noise(2, s + 3) * 0.25
    pool = wet > 0.74
    idx[pool] = 1
    shadow = pool & ~np.roll(np.roll(pool, 1, 0), 1, 1)
    idx[shadow] = 0
    img = paint(idx, MUD)
    glint = pool & ~np.roll(np.roll(pool, -1, 0), -1, 1) & dither_mask(s + 4, 0.3)
    img[glint, :3] = (104, 104, 94)
    smear = dither_mask(s + 5, 0.05) & ~pool
    img[smear, :3] *= 0.85
    return img


def peat(s):
    idx = np.full((T, T), 2.0)
    r = np.random.default_rng(s)
    for _ in range(90):  # fibres lying flat, packed in layers
        x, y = r.integers(0, T, 2)
        n = r.integers(3, 8)
        d = r.choice([-1, 1, 1])
        for i in range(n):
            idx[(y + (i // 4)) % T, (x + i) % T] = 2 + d
    layer = (YS % 8) == 7
    idx[layer & dither_mask(s + 1, 0.7)] = 0
    img = paint(idx, PEAT)
    roots = dither_mask(s + 2, 0.02)
    img[roots, :3] = (126, 92, 58)
    return img


def moss(s):
    pts = jittered_points(s, 4, 4, 0.9)
    label, d1, edge = voronoi(pts, wobble=4, seed=s)
    r = np.random.default_rng(s + 1)
    tone = r.choice([2, 3, 3, 4], len(pts))
    height = np.minimum(edge, 3.5)
    idx = tone[label] + bevel(height) * 1.0 + (height - 1.8) * 0.35
    idx[edge < 0.8] = 0
    idx[dither_mask(s + 2, 0.12) & (edge > 1.5)] += 1
    return paint(np.clip(idx, 0, 5), MOSS)


# ---- wood and leaves -------------------------------------------------------

def log_side(s, dark=(70, 50, 32), light=(110, 80, 50)):
    pal = shifted_ramp(np.array(dark) * 0.55, np.array(light) * 1.25, 6)
    pts = jittered_points(s, 4, 2, 0.8)
    # Bark plates: tall, narrow and wavy.
    label, _, edge = voronoi(pts, stretch=(1.0, 0.25), wobble=4, seed=s)
    r = np.random.default_rng(s + 1)
    tone = r.choice([2, 3, 3], len(pts))
    height = np.minimum(edge, 2.5)
    left = np.roll(height, 1, 1)
    side = np.where(height - left > 0.3, 1, np.where(height - left < -0.3, -1, 0))
    idx = tone[label] + side + noise((2, 8), s + 2) - 0.5
    idx[edge < 0.6] = 0
    # Horizontal cracks across a few plates.
    for _ in range(5):
        x, y = r.integers(0, T, 2)
        for i in range(r.integers(2, 4)):
            idx[y, (x + i) % T] = 0
            idx[(y + 1) % T, (x + i) % T] = max(idx[(y + 1) % T, (x + i) % T], 4)
    return paint(np.clip(idx, 0, 5), pal)


def log_top(s, bark=(70, 50, 32), wood=(170, 132, 84)):
    pal = shifted_ramp(np.array(wood) * 0.55, np.array(wood) * 1.18, 6)
    bark_pal = shifted_ramp(np.array(bark) * 0.6, np.array(bark) * 1.3, 4)
    cx = cy = 15.5
    ax, ay = np.abs(XS - cx), np.abs(YS - cy)
    # Rounded square rings, a little wobbly, darker in the middle.
    d = (np.maximum(ax, ay) * 0.55 + np.hypot(ax, ay) * 0.45) + (noise(8, s) - 0.5) * 1.6
    ring = np.floor(d / 2.4)
    idx = np.where(ring % 2 == 0, 3.0, 4.0)
    idx[(d % 2.4) < 0.7] = 2
    idx[d < 2.2] = 1
    r = np.random.default_rng(s + 1)
    # A check (radial crack) or two.
    for _ in range(1):
        ang = r.random() * 2 * np.pi
        for t in np.arange(4, 10, 0.5):
            idx[int(cy + np.sin(ang) * t), int(cx + np.cos(ang) * t)] = 0
    img = paint(idx, pal)
    edge = np.minimum.reduce([XS, YS, T - 1 - XS, T - 1 - YS])
    bark_idx = np.where(edge == 0, 1, 2) + bevel((edge < 2).astype(float) * 2) * 0
    bark_idx = np.where((XS + YS + s) % 5 == 0, bark_idx + 1, bark_idx)
    is_bark = edge < 2
    img[is_bark, :3] = bark_pal[np.clip(bark_idx[is_bark].astype(int), 0, 3)]
    img[edge == 2, :3] = pal[0]
    return img


def leaves(s, colors=((34, 74, 30), (54, 104, 38), (86, 136, 52))):
    pal = shifted_ramp(np.array(colors[0]) * 0.55, np.array(colors[2]) * 1.2, 6, 0.05)
    depth = noise(8, s) * 0.6 + noise(4, s + 1) * 0.4
    idx = np.where(depth > 0.5, 1.0, 0.0)
    alpha = np.full((T, T), 255.0)
    r = np.random.default_rng(s + 2)
    leaf = np.zeros((T, T), bool)
    shapes = [
        [(0, 0), (1, 0), (0, 1), (1, 1), (2, 1)],
        [(1, 0), (0, 1), (1, 1), (2, 1), (1, 2)],
        [(0, 0), (1, 0), (1, 1), (2, 1), (2, 2)],
        [(0, 0), (0, 1), (1, 1), (1, 2)],
    ]
    for _ in range(95):
        x, y = r.integers(0, T, 2)
        shape = shapes[r.integers(0, len(shapes))]
        tone = r.choice([2, 3, 3, 4])
        for k, (dx, dy) in enumerate(shape):
            px, py = (x + dx) % T, (y + dy) % T
            idx[py, px] = tone + (1 if k == 0 else 0) - (1 if k == len(shape) - 1 else 0)
            leaf[py, px] = True
        # Shadow under the leaf.
        sx, sy = (x + 2) % T, (y + 3) % T
        if not leaf[sy, sx]:
            idx[sy, sx] = 0
    holes = ~leaf & (depth < 0.42) & dither_mask(s + 3, 0.7)
    alpha[holes] = 0
    return paint(np.clip(idx, 0, 5), pal, alpha)


def planks(s):
    r = np.random.default_rng(s)
    idx = np.zeros((T, T))
    for b in range(4):
        y0 = b * 8
        tone = r.choice([2, 3, 3, 4])
        grain = noise((16, 2), s + b * 5) + noise((8, 1), s + b * 5 + 1) * 0.4
        rows = slice(y0, y0 + 8)
        idx[rows] = tone + np.where(grain[rows] > 0.95, 1, np.where(grain[rows] < 0.45, -1, 0))
        idx[y0] = tone + 1               # lit top edge
        idx[y0 + 7] = 0                  # dark seam under each board
        idx[y0 + 6] = np.minimum(idx[y0 + 6], tone - 1)
        # One butt joint per board, nailed on both sides.
        jx = (r.integers(0, T) + b * 11) % T
        idx[y0:y0 + 7, jx] = 0
        idx[y0 + 1:y0 + 6, (jx + 1) % T] = tone + 1
        for nx in (jx - 2, jx + 3):
            idx[y0 + 3, nx % T] = 0
            idx[y0 + 2, nx % T] = max(idx[y0 + 2, nx % T], tone + 1)
        # A knot now and then.
        if r.random() < 0.5:
            kx = r.integers(0, T)
            idx[y0 + 3, kx % T] = 0
            idx[y0 + 3, (kx + 1) % T] = 1
            idx[y0 + 4, kx % T] = 1
    return paint(np.clip(idx, 0, 5), WOOD)
