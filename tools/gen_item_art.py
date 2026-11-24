#!/usr/bin/env python3
"""Generate the placeholder item sprite sheets.

Three sheets in assets/textures/icons/, each a PNG plus a RON list of where
every sprite sits (item name -> (x, y, width, height) in pixels):

  items.png    / items.ron     food, materials, clothes, bags, torches
  weapons.png  / weapons.ron   tools and weapons
  blocks.png   / blocks.ron    blocks you can place, drawn as little cubes

Each item's sprite is 20 pixels per inventory cell it takes: a 1x1 item is
20 x 20, the 2x3 pickaxe 40 x 60. The game draws them at 2x in the
inventory and hotbar, and in 3D (in your hand and on the ground) it turns
every pixel into a little slab, so the sprite is also the held model.
Transparent pixels are empty space. Draw things upright: the bottom of a
tool's sprite is the end you hold.

Paint over the PNGs in any pixel editor (Aseprite, Pixelorama, Photoshop
with nearest-neighbour). Keep each sprite inside its rectangle, or move it
and change the RON to match.

Running this again keeps what is already in the sheets and only draws
sprites for items that don't have one yet (new items in items.ron), so it
never paints over your work. To start a sheet over with the placeholder art:

  python3 tools/gen_item_art.py --reset items weapons blocks

The block sheet is drawn from the block textures (textures/blocks.png): after
repainting those, `--reset blocks` redraws the little cubes to match.

Run from the repo root:  python3 tools/gen_item_art.py
"""
import math
import os
import re
import sys

from PIL import Image

ROOT = os.path.join(os.path.dirname(__file__), "..")
OUT = os.path.join(ROOT, "assets", "textures", "icons")
CELL = 20
SHEET_W = 256

# --- reading the game's data -------------------------------------------------


def read_items():
    """(name, kind, (w, h)) for each item in items.ron, in order."""
    text = open(os.path.join(ROOT, "assets", "data", "items.ron")).read()
    text = re.sub(r"//[^\n]*", "", text)
    out = []
    for m in re.finditer(r'name:\s*"(\w+)"', text):
        # The item's fields run to the next item's name.
        nxt = text.find('name: "', m.end())
        body = text[m.end(): nxt if nxt >= 0 else len(text)]
        kind = re.search(r"kind:\s*(\w+)(?:\(\"?(\w+)\"?)?", body)
        size = re.search(r"size:\s*\((\d+),\s*(\d+)\)", body)
        w, h = (int(size.group(1)), int(size.group(2))) if size else (1, 1)
        out.append((m.group(1), kind.group(1), kind.group(2), (w, h)))
    return out


def sheet_for(kind):
    return {"Tool": "weapons", "Block": "blocks"}.get(kind, "items")


# --- drawing -----------------------------------------------------------------


def hexc(h):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16))


def mul(c, k):
    return tuple(max(0, min(255, int(v * k))) for v in c[:3])


def mix(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


class Canvas:
    """A sprite being drawn. Each pixel has a colour and a part number;
    `finish` shades every part (light from the top left: a soft roundness
    across the part, a bright bevel on its lit edges and a deep one on its
    shaded edges, warm highlights and cool purple shadows) and outlines the
    whole thing in a dark, hue-shifted line.

    Parts have a finish: "matte" (wood, stone, cloth), "metal" (more
    contrast, a bright inner edge), "soft" (cloth and fur: gentle shading),
    "gem" (glassy, strong highlight) or "glow" (barely shaded, for light)."""

    CONTRAST = {"matte": 1.0, "metal": 1.45, "soft": 0.65, "gem": 1.3, "glow": 0.35}

    def __init__(self, w, h, seed=1):
        self.w, self.h = w, h
        self.col = {}
        self.part = {}
        self.parts = 0
        self.seed = seed
        self.flat = set()
        self.kind = {}

    def new_part(self, flat=False, kind="matte"):
        self.parts += 1
        if flat:
            self.flat.add(self.parts)
        self.kind[self.parts] = kind
        return self.parts

    def noise(self, x, y, salt=0):
        n = ((x * 73856093) ^ (y * 19349663) ^ ((self.seed + salt) * 83492791)) & 0xFFFF
        return (n / 0xFFFF) * 2.0 - 1.0

    def put(self, x, y, c, part, grain=0.06):
        x, y = int(x), int(y)
        if 0 <= x < self.w and 0 <= y < self.h:
            self.col[(x, y)] = mul(c, 1.0 + grain * self.noise(x, y))
            self.part[(x, y)] = part

    def rect(self, x0, y0, x1, y1, c, part=None, grain=0.06, kind="matte"):
        part = part or self.new_part(kind=kind)
        for y in range(int(y0), int(y1)):
            for x in range(int(x0), int(x1)):
                self.put(x, y, c, part, grain)
        return part

    def ellipse(self, cx, cy, rx, ry, c, part=None, grain=0.06, kind="matte"):
        part = part or self.new_part(kind=kind)
        for y in range(int(cy - ry - 1), int(cy + ry + 2)):
            for x in range(int(cx - rx - 1), int(cx + rx + 2)):
                dx, dy = (x + 0.5 - cx) / rx, (y + 0.5 - cy) / ry
                if dx * dx + dy * dy <= 1.0:
                    self.put(x, y, c, part, grain)
        return part

    def poly(self, pts, c, part=None, grain=0.06, kind="matte"):
        part = part or self.new_part(kind=kind)
        ys = [p[1] for p in pts]
        for y in range(int(min(ys)), int(max(ys)) + 1):
            yc = y + 0.5
            xs = []
            for i in range(len(pts)):
                (x0, y0), (x1, y1) = pts[i], pts[(i + 1) % len(pts)]
                if (y0 <= yc < y1) or (y1 <= yc < y0):
                    xs.append(x0 + (yc - y0) * (x1 - x0) / (y1 - y0))
            xs.sort()
            for i in range(0, len(xs) - 1, 2):
                for x in range(int(round(xs[i])), int(round(xs[i + 1]))):
                    self.put(x, y, c, part, grain)
        return part

    def line(self, x0, y0, x1, y1, width, c, part=None, grain=0.06, kind="matte"):
        part = part or self.new_part(kind=kind)
        n = int(max(abs(x1 - x0), abs(y1 - y0)) * 2) + 1
        r = width / 2.0
        for i in range(n + 1):
            t = i / n
            x, y = x0 + (x1 - x0) * t, y0 + (y1 - y0) * t
            for yy in range(int(y - r), int(y + r) + 1):
                for xx in range(int(x - r), int(x + r) + 1):
                    if (xx + 0.5 - x) ** 2 + (yy + 0.5 - y) ** 2 <= r * r + 0.25:
                        self.put(xx, yy, c, part, grain)
        return part

    def dot(self, x, y, c, part=None):
        part = part or self.new_part(flat=True)
        self.put(x, y, c, part, 0.0)
        return part

    def paint(self, x, y, c):
        """Recolour a pixel that is already drawn, keeping its part (a pattern
        inside a part: wood grain, a wrap, stitches)."""
        x, y = int(x), int(y)
        if (x, y) in self.col:
            self.col[(x, y)] = c

    def tint(self, x, y, k):
        x, y = int(x), int(y)
        if (x, y) in self.col:
            self.col[(x, y)] = mul(self.col[(x, y)], k)

    def grain(self, part, k=0.82, step=3, vertical=True):
        """Wood grain: broken streaks along the length of a part."""
        for (x, y), p in list(self.part.items()):
            if p != part:
                continue
            a, b = (x, y // step) if vertical else (y, x // step)
            if self.noise(a * 3, b, 5) > 0.55:
                self.tint(x, y, k)
            elif self.noise(a * 3, b, 9) > 0.8:
                self.tint(x, y, 1.12)

    def finish(self):
        img = Image.new("RGBA", (self.w, self.h), (0, 0, 0, 0))
        px = img.load()
        box = {}
        for (x, y), p in self.part.items():
            b = box.get(p, (x, y, x, y))
            box[p] = (min(b[0], x), min(b[1], y), max(b[2], x), max(b[3], y))
        for (x, y), c in self.col.items():
            p = self.part[(x, y)]
            if p not in self.flat:
                k = self.CONTRAST.get(self.kind.get(p, "matte"), 1.0)
                x0, y0, x1, y1 = box[p]
                nx = (x - x0) / max(1, x1 - x0) - 0.5
                ny = (y - y0) / max(1, y1 - y0) - 0.5
                t = -(nx * 0.45 + ny * 0.75) * 0.32

                def same(dx, dy):
                    return self.part.get((x + dx, y + dy)) == p

                if not same(0, -1) or not same(-1, 0):
                    t += 0.34
                elif not same(0, 1) or not same(1, 0):
                    t -= 0.34
                elif not same(0, -2) or not same(-2, 0):
                    t += 0.1
                elif not same(0, 2) or not same(2, 0):
                    t -= 0.12
                c = ramp(c, t * k)
            px[x, y] = c + (255,)
        # A dark outline just outside the shape, in a deep cool shade of what
        # it touches.
        for y in range(self.h):
            for x in range(self.w):
                if (x, y) in self.col:
                    continue
                near = [self.col[q] for q in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)) if q in self.col]
                if near:
                    n = min(near, key=sum)
                    px[x, y] = (int(n[0] * 0.2 + 22), int(n[1] * 0.16 + 13), int(n[2] * 0.2 + 24), 255)
        return img


def ramp(c, t):
    """Light (t > 0) towards a warm cream, dark (t < 0) towards a cool
    purple-brown, the way pixel artists shift hue along a ramp."""
    if t >= 0:
        return mix(c, (255, 242, 206), min(0.85, t * 0.9))
    shadow = (int(c[0] * 0.36 + 16), int(c[1] * 0.3 + 10), int(c[2] * 0.42 + 30))
    return mix(c, shadow, min(1.0, -t * 1.5))


# Palette: warm oak and leather, old brass and gold, cool metals.
WOOD, WOOD_D = hexc("8a5a32"), hexc("5c3a20")
STONE, STONE_L = hexc("8a8680"), hexc("aaa69c")
FLINT = hexc("4a5064")
CORD = hexc("c2a26a")
LEATHER, LEATHER_D = hexc("6e4024"), hexc("4a2a18")
LINEN = hexc("ddd2b4")
HIDE, HIDE_D = hexc("94603a"), hexc("6a4228")
FUR, FUR_L = hexc("74584a"), hexc("a4876c")
REED, REED_D = hexc("bcae6a"), hexc("8a8448")
BARK, BARK_D = hexc("5e4630"), hexc("3e2e20")
MEAT, MEAT_FAT = hexc("b84846"), hexc("ecc4b0")
GOLD, GOLD_L = hexc("d0a034"), hexc("ffe48c")
BRASS_C = hexc("b8903c")
IRON, IRON_L = hexc("6c7282"), hexc("c4cad4")
STEEL, STEEL_L = hexc("a4b0c0"), hexc("f4f8ff")
SPIRIT, SPIRIT_L = hexc("46588a"), hexc("8af0ff")
RUBY, SAPPHIRE, EMERALD, AMETHYST = hexc("c42a3a"), hexc("3a78e0"), hexc("30b064"), hexc("9048d0")
STITCH = hexc("d8b878")


def gem(c, x, y, r, col):
    """A round cut stone with a glint."""
    c.ellipse(x, y, r, r, col, kind="gem", grain=0.0)
    c.dot(int(x - r * 0.4), int(y - r * 0.4), mix(col, (255, 255, 255), 0.8))


def rivet(c, x, y, col=GOLD_L):
    c.dot(x, y, col)


def handle(c, x, y0, y1, w=3, cap=IRON, wood=WOOD):
    """A wooden haft with grain, a leather grip wound on the lower part and a
    metal cap at the end."""
    p = c.rect(x, y0, x + w, y1 - 2, wood)
    c.grain(p, step=4)
    g0 = max(y0, y1 - 15)
    g = c.rect(x, g0, x + w, y1 - 2, LEATHER, grain=0.03)
    for y in range(g0, y1 - 2):
        for i in range(w):
            if (i + y) % 3 == 0:
                c.paint(x + i, y, LEATHER_D)
    c.paint(x, g0, STITCH)
    c.paint(x + w - 1, g0, STITCH)
    c.rect(x - 1, y1 - 3, x + w + 1, y1, cap, kind="metal", grain=0.02)
    return p


def binding(c, x0, y0, x1, y1):
    """Cord lashing: crossed strands over a band."""
    p = c.rect(x0, y0, x1, y1, CORD, grain=0.04)
    for y in range(int(y0), int(y1)):
        for x in range(int(x0), int(x1)):
            if (x + y) % 3 == 0 or (x - y) % 4 == 0:
                c.paint(x, y, mul(CORD, 0.72))
    return p


def facets(c, pts, col, lit):
    """A knapped stone: the whole shape, then a lighter flake scar."""
    c.poly(pts, col)
    n = len(pts)
    cx = sum(p[0] for p in pts) / n
    cy = sum(p[1] for p in pts) / n
    a, b = pts[0], pts[1]
    c.poly([a, b, ((a[0] + b[0]) / 2 * 0.4 + cx * 0.6, (a[1] + b[1]) / 2 * 0.4 + cy * 0.6)], lit)


# --- weapons and tools -------------------------------------------------------


def rough_axe(c):
    # A forked stick with a knapped rock jammed in the split.
    p = c.line(10, 39, 10, 10, 3, WOOD)
    c.grain(p)
    c.line(9, 12, 6, 4, 2, WOOD_D)
    c.line(11, 12, 14, 4, 2, WOOD_D)
    facets(c, [(3, 6), (9, 2), (17, 4), (19, 10), (15, 15), (8, 14)], STONE, STONE_L)
    c.line(17, 5, 18, 11, 1, mix(STONE_L, (255, 255, 255), 0.3))
    binding(c, 8, 13, 13, 18)
    c.rect(8, 33, 13, 36, LEATHER)


def stone_pickaxe(c):
    handle(c, 18, 12, 59, 4, cap=STONE)
    facets(c, [(1, 15), (8, 7), (20, 4), (32, 7), (39, 15), (33, 12), (20, 10), (7, 12)], STONE, STONE_L)
    c.line(4, 13, 10, 8, 1, STONE_L)
    c.line(30, 8, 36, 13, 1, mul(STONE, 0.75))
    binding(c, 16, 4, 24, 14)


def stone_axe(c):
    handle(c, 4, 4, 59, 3, cap=STONE)
    facets(c, [(7, 5), (16, 2), (20, 8), (20, 20), (16, 25), (7, 19)], STONE, STONE_L)
    c.line(18, 5, 19, 22, 1, mix(STONE_L, (255, 255, 255), 0.3))
    binding(c, 3, 9, 9, 16)


def stone_shovel(c):
    handle(c, 8, 20, 59, 3, cap=STONE)
    c.rect(6, 54, 13, 57, WOOD_D)
    facets(c, [(3, 6), (6, 2), (13, 2), (16, 6), (16, 18), (12, 22), (7, 22), (3, 18)], STONE, STONE_L)
    binding(c, 7, 17, 12, 25)


def flint_spear(c):
    handle(c, 8, 18, 79, 3, cap=FLINT)
    c.poly([(9.5, 0), (14, 8), (13, 16), (9.5, 19), (6, 16), (5, 8)], FLINT, kind="gem")
    c.line(9.5, 2, 9.5, 16, 1, mix(FLINT, (200, 214, 236), 0.45))
    for y in (6, 10, 14):
        c.paint(7, y, mul(FLINT, 0.7))
        c.paint(12, y, mul(FLINT, 0.7))
    binding(c, 7, 16, 12, 23)
    # A red cloth tassel below the head.
    c.line(12, 23, 15, 31, 1.5, hexc("a8323a"), kind="soft")
    c.line(13, 23, 16, 29, 1, hexc("c84848"), kind="soft")


def pick_head(c, metal, edge, top=3):
    """A curved pick head: thick in the middle, tapering to two points."""
    upper, lower = [], []
    for i in range(41):
        x = i
        dx = (x - 20) / 20.0
        yu = top + 10 * dx * dx
        th = 1.5 + 8.5 * (1 - abs(dx)) ** 0.9
        upper.append((x, yu))
        lower.append((x, yu + th))
    p = c.poly(upper + lower[::-1], metal, kind="metal", grain=0.03)
    # Bright cutting edge along the underside of each point.
    for x in list(range(2, 13)) + list(range(28, 39)):
        dx = (x - 20) / 20.0
        y = top + 10 * dx * dx + 1.5 + 8.5 * (1 - abs(dx)) ** 0.9 - 1
        c.paint(x, int(y), edge)
    return p


def socket(c, metal, band, x0=15, x1=25, y0=2, y1=14):
    c.rect(x0, y0, x1, y1, mul(metal, 0.78), kind="metal", grain=0.03)
    c.rect(x0, y1 - 3, x1, y1 - 1, band, kind="metal", grain=0.02)
    for x in (x0 + 2, x1 - 3):
        rivet(c, x, y0 + 3, mix(band, (255, 255, 255), 0.5))


def metal_pickaxe(c, metal, edge, band):
    handle(c, 18, 12, 59, 4, cap=band)
    pick_head(c, metal, edge)
    socket(c, metal, band)


def axe_blade(c, metal, edge):
    """A bearded blade on the right of the haft, with a curved bright edge."""
    pts = [(7, 7), (12, 5), (16, 0), (19, 3), (20, 10), (20, 17), (19, 24), (16, 30), (12, 25), (7, 20)]
    p = c.poly(pts, metal, kind="metal", grain=0.03)
    for y in range(1, 30):
        # the edge curve: rightmost pixel of each row
        xs = [x for x in range(20) if c.part.get((x, y)) == p]
        if xs:
            c.paint(max(xs), y, edge)
            if len(xs) > 3:
                c.paint(max(xs) - 1, y, mix(edge, metal, 0.5))
    return p


def metal_axe(c, metal, edge, band):
    handle(c, 4, 4, 59, 3, cap=band)
    axe_blade(c, metal, edge)
    c.rect(3, 8, 9, 17, mul(metal, 0.75), kind="metal", grain=0.03)
    c.rect(3, 14, 9, 16, band, kind="metal", grain=0.02)
    rivet(c, 6, 10, mix(band, (255, 255, 255), 0.5))


def iron_pickaxe(c):
    metal_pickaxe(c, IRON, IRON_L, hexc("8a6a4a"))


def iron_axe(c):
    metal_axe(c, IRON, IRON_L, hexc("8a6a4a"))


def iron_shovel(c):
    handle(c, 8, 20, 59, 3, cap=hexc("8a6a4a"))
    c.rect(6, 54, 13, 57, WOOD_D)
    p = c.poly([(3, 6), (6, 2), (13, 2), (16, 6), (16, 18), (12, 22), (7, 22), (3, 18)], IRON, kind="metal", grain=0.03)
    c.line(9.5, 5, 9.5, 17, 1, mul(IRON, 0.7))
    for x in range(5, 15):
        c.paint(x, 2 if 6 <= x <= 13 else 3, IRON_L)
    c.rect(7, 17, 12, 25, mul(IRON, 0.75), kind="metal")
    rivet(c, 9, 20, IRON_L)
    return p


def steel_pickaxe(c):
    metal_pickaxe(c, STEEL, STEEL_L, GOLD)
    gem(c, 20, 7, 1.6, RUBY)


def steel_axe(c):
    metal_axe(c, STEEL, STEEL_L, GOLD)
    # An etched line along the blade.
    for y in range(8, 20):
        c.paint(14, y, mul(STEEL, 0.78))


def runes(c, pts, col=SPIRIT_L):
    for x, y in pts:
        c.dot(x, y, col)


def spirit_pickaxe(c):
    metal_pickaxe(c, SPIRIT, SPIRIT_L, hexc("6a7cc0"))
    runes(c, [(8, 9), (10, 10), (30, 10), (32, 9), (12, 11), (28, 11)])
    gem(c, 20, 7, 1.6, hexc("5ad8ff"))


def spirit_axe(c):
    metal_axe(c, SPIRIT, SPIRIT_L, hexc("6a7cc0"))
    runes(c, [(13, 9), (14, 11), (13, 13), (14, 15), (13, 17)])


def blade(c, x0, x1, y0, y1, metal, edge):
    """A diamond-section blade from y0 (tip) to y1: the left face catches the
    light, the right face is in shade, a bright ridge runs up the middle."""
    mid = (x0 + x1) / 2.0
    p = c.poly([(x0, y1), (x0, y0 + 6), (mid, y0), (x1, y0 + 6), (x1, y1)], metal, kind="metal", grain=0.02)
    for (x, y), q in list(c.part.items()):
        if q != p:
            continue
        if x < int(mid):
            c.paint(x, y, mix(metal, edge, 0.3))
        elif x > int(mid):
            c.paint(x, y, mul(metal, 0.78))
        else:
            c.paint(x, y, edge)
    return p


def sword(c, metal, edge, guard, pommel_gem=None):
    """A blade, a crossguard with curled ends, a leather grip and a pommel."""
    blade(c, 7, 14, 0, 44, metal, edge)
    c.rect(3, 44, 17, 47, guard, kind="metal", grain=0.02)
    c.rect(1, 42, 4, 45, guard, kind="metal", grain=0.02)
    c.rect(16, 42, 19, 45, guard, kind="metal", grain=0.02)
    c.rect(8, 43, 12, 48, mul(guard, 0.85), kind="metal")
    handle(c, 8, 48, 57, 4, cap=guard, wood=LEATHER)
    c.ellipse(10, 57.5, 3, 2.2, guard, kind="metal")
    if pommel_gem:
        gem(c, 10, 45.5, 1.2, pommel_gem)


def iron_sword(c):
    sword(c, IRON, IRON_L, hexc("7a5a3a"))


def steel_sword(c):
    sword(c, STEEL, STEEL_L, GOLD, RUBY)


def spirit_sword(c):
    sword(c, SPIRIT, SPIRIT_L, hexc("8a9ad8"), hexc("5ad8ff"))
    runes(c, [(8, 10), (12, 14), (8, 19), (12, 24), (8, 29), (12, 34), (8, 39)])


def iron_dagger(c):
    """A short blade, 20 x 40."""
    blade(c, 7, 14, 0, 26, IRON, IRON_L)
    c.rect(3, 26, 17, 29, hexc("7a5a3a"), kind="metal")
    c.rect(2, 25, 4, 27, hexc("7a5a3a"), kind="metal")
    c.rect(16, 25, 18, 27, hexc("7a5a3a"), kind="metal")
    handle(c, 8, 29, 38, 4, cap=hexc("7a5a3a"), wood=LEATHER)


def rapier(c):
    """A long thin blade and a swept brass guard, 20 x 60."""
    c.poly([(9, 44), (9, 4), (10, 0), (11, 4), (11, 44)], STEEL, kind="metal", grain=0.02)
    for y in range(2, 44):
        c.paint(9, y, STEEL_L)
    c.rect(3, 44, 17, 46, GOLD, kind="metal")
    c.ellipse(3, 45, 1.6, 1.6, GOLD, kind="metal")
    c.ellipse(17, 45, 1.6, 1.6, GOLD, kind="metal")
    c.line(4, 46, 5, 54, 1.4, GOLD, kind="metal")
    c.line(5, 54, 13, 57, 1.4, GOLD, kind="metal")
    handle(c, 8, 47, 57, 4, cap=GOLD, wood=LEATHER)
    gem(c, 10, 58, 1.3, SAPPHIRE)


def makeshift_pistol(c):
    """A crude pistol, 40 x 20: iron barrel strapped to a wooden grip."""
    c.rect(4, 4, 34, 8, IRON, kind="metal", grain=0.03)
    c.line(5, 5, 33, 5, 1, IRON_L)
    c.rect(34, 3, 37, 9, mul(IRON, 0.7), kind="metal")
    p = c.poly([(6, 8), (16, 8), (14, 18), (6, 18)], WOOD)
    c.grain(p, vertical=True)
    c.rect(5, 16, 15, 18, BRASS_C, kind="metal")
    binding(c, 10, 4, 13, 9)
    binding(c, 22, 4, 25, 9)
    c.line(16, 9, 18, 13, 1, IRON, kind="metal")
    rivet(c, 9, 11, GOLD_L)


def rifle(c):
    """A long rifle standing up, 20 x 80: barrel on top, stock at the bottom."""
    c.rect(9, 0, 11, 48, IRON, kind="metal", grain=0.02)
    for y in range(1, 48):
        c.paint(9, y, IRON_L)
    p = c.rect(7, 30, 13, 56, WOOD)
    c.grain(p)
    for y in (32, 40):
        c.rect(7, y, 13, y + 2, BRASS_C, kind="metal")
    c.rect(8, 46, 12, 49, BRASS_C, kind="metal")
    q = c.poly([(7, 56), (13, 56), (15, 78), (5, 78)], WOOD_D)
    c.grain(q)
    c.rect(5, 76, 15, 78, BRASS_C, kind="metal")
    c.line(13, 50, 16, 54, 1, IRON, kind="metal")


# --- items -------------------------------------------------------------------


def flame(c, cx, cy, s=1.0):
    c.ellipse(cx, cy, 5 * s, 7 * s, hexc("e8641c"), kind="glow", grain=0.0)
    c.poly([(cx - 3 * s, cy - 3 * s), (cx, cy - 10 * s), (cx + 3 * s, cy - 3 * s)], hexc("e8641c"), kind="glow", grain=0.0)
    c.ellipse(cx, cy + 0.5, 3.4 * s, 5 * s, hexc("ffa830"), kind="glow", grain=0.0)
    c.ellipse(cx, cy + 1.5, 1.8 * s, 3 * s, hexc("fff0a0"), kind="glow", grain=0.0)


def torch(c):
    p = c.rect(8, 15, 12, 39, WOOD)
    c.grain(p)
    c.rect(7, 13, 13, 18, hexc("4a3020"))
    binding(c, 7, 16, 13, 19)
    flame(c, 10, 9.5, 0.9)
    c.dot(6, 3, hexc("ffd060"))
    c.dot(14, 5, hexc("ff9a30"))


def marsh_berries(c):
    c.line(11, 1, 9, 6, 1, hexc("4a3a20"))
    c.poly([(10, 3), (16, 1), (14, 6)], hexc("4a7a30"))
    c.line(11, 3, 15, 2, 1, hexc("6a9a40"))
    for x, y, col in [(7, 9, "5a2a74"), (13, 9, "6c3088"), (10, 14, "4e2266"), (5, 15, "642c80"), (15, 15, "5a2a74")]:
        c.ellipse(x, y, 3.5, 3.5, hexc(col), kind="gem")
        c.dot(x - 1, y - 1, hexc("e0c0f4"))


def swamp_reed(c):
    for x, top in [(6, 3), (10, 1), (14, 4)]:
        c.line(x, top, x - 1 + (x - 10) * 0.2, 18, 2.5, hexc("9eb466"))
        c.paint(x, top + 5, hexc("7a8a48"))
        c.paint(x, top + 9, hexc("7a8a48"))
    binding(c, 4, 12, 16, 15)


def smoked_fish(c):
    c.ellipse(17, 10, 13, 6, hexc("a86a36"))
    c.poly([(30, 10), (38, 3), (36, 10), (37, 17)], hexc("8a5428"))
    c.poly([(14, 4), (20, 1), (24, 4)], hexc("8a5428"))
    c.dot(9, 8, (20, 16, 12))
    c.dot(8, 7, (250, 240, 220))
    c.line(12, 6, 12, 14, 1, hexc("7a4a22"))
    for x in range(16, 28, 3):
        for y in range(7, 14, 2):
            c.paint(x + (y % 4) // 2, y, hexc("c8884a"))


def bread(c):
    c.ellipse(10, 11, 9, 7, hexc("c88c44"))
    c.ellipse(10, 9, 7, 4.5, hexc("d8a050"))
    for x in (6, 10, 14):
        c.line(x - 1, 7, x + 1, 11, 1, hexc("f4d08c"))
    c.line(2, 15, 18, 15, 1, hexc("8a5428"))


def clay_pot(c, x0, x1, y_rim, y_bot, body=hexc("8a5a3c")):
    w = x1 - x0
    c.poly([(x0 + 1, y_rim + 2), (x1 - 1, y_rim + 2), (x1 - 2, y_bot - 4), (x1 - 2 - w * 0.2, y_bot), (x0 + 2 + w * 0.2, y_bot), (x0 + 2, y_bot - 4)], body)
    c.rect(x0, y_rim, x1, y_rim + 3, mix(body, (240, 200, 150), 0.15))
    for x in range(int(x0 + 3), int(x1 - 2), 3):
        c.dot(x, y_rim + 7, mix(body, (240, 210, 160), 0.35))


def steam(c, xs, y0, y1):
    for i, x in enumerate(xs):
        for y in range(int(y1), int(y0)):
            if (y + i) % 5 < 3:
                c.dot(x + (1 if (y // 3 + i) % 2 else 0), y, hexc("e4e0d8"))


def peat_stew(c):
    clay_pot(c, 2, 18, 18, 38)
    c.ellipse(10, 19, 7, 1.6, hexc("8c4a1c"), kind="soft")
    steam(c, (6, 10, 14), 16, 6)


def cured_meat(c):
    c.ellipse(10, 11, 8, 7, hexc("8c2e2a"))
    c.ellipse(8, 9, 3, 2, MEAT_FAT, kind="soft")
    c.line(3, 11, 17, 11, 1, CORD)
    c.line(10, 3, 10, 19, 1, CORD)
    c.dot(10, 11, mul(CORD, 0.7))


def feast_platter(c):
    c.ellipse(20, 24, 19, 12, hexc("b8a888"), kind="metal")
    c.ellipse(20, 23, 15, 9, hexc("d8ceb6"))
    c.ellipse(23, 19, 8, 6, hexc("9a5226"))
    c.line(28, 15, 33, 9, 2, hexc("ece0cc"))
    c.ellipse(33, 9, 1.5, 1.5, hexc("ece0cc"))
    for x, y in [(11, 22), (8, 25), (13, 26)]:
        c.ellipse(x, y, 2.4, 2.4, hexc("5a2a74"), kind="gem")
    c.ellipse(26, 28, 5, 3, hexc("c88c44"))
    c.line(15, 17, 19, 14, 1.5, hexc("5a8a34"))


def ember_herb(c):
    c.line(10, 19, 10, 7, 1.5, hexc("4a6a2a"))
    c.poly([(10, 12), (3, 8), (9, 9)], hexc("5a8a34"))
    c.poly([(10, 15), (17, 10), (11, 11)], hexc("5a8a34"))
    c.ellipse(10, 5, 4, 4, hexc("e85018"), kind="glow")
    c.ellipse(10, 5, 2, 2, hexc("ffd060"), kind="glow")
    c.dot(5, 3, hexc("ffb040"))
    c.dot(15, 2, hexc("ff8030"))


def stick(c):
    p = c.line(10, 2, 10, 38, 3, WOOD)
    c.grain(p)
    q = c.line(10, 14, 15, 9, 2, WOOD)
    c.dot(15, 8, hexc("6a9a40"))
    c.dot(16, 8, hexc("4a7a30"))
    return q


def plant_fiber(c):
    for i, x in enumerate((5, 8, 11, 14)):
        c.line(x, 2, x + (i - 1.5) * 1.5, 18, 1.5, hexc("8aa04c") if i % 2 else hexc("6e8a3a"), kind="soft")
    binding(c, 4, 9, 16, 12)


def cordage(c):
    for r in (8, 5.5):
        c.ellipse(10, 10, r, r, CORD)
        c.ellipse(10, 10, r - 1.5, r - 1.5, mul(CORD, 0.75))
    c.ellipse(10, 10, 3, 3, CORD)
    for a in range(0, 360, 30):
        x, y = 10 + math.cos(math.radians(a)) * 7, 10 + math.sin(math.radians(a)) * 7
        c.paint(x, y, mul(CORD, 0.7))
    c.line(14, 15, 18, 19, 2, CORD)


def coal(c):
    c.poly([(3, 10), (8, 4), (14, 5), (17, 11), (13, 17), (6, 16)], hexc("2c2a30"), kind="gem")
    c.poly([(8, 4), (14, 5), (11, 9)], hexc("44424c"))
    c.dot(8, 8, hexc("8a8a98"))
    c.dot(12, 12, hexc("6a6a78"))
    c.dot(6, 13, hexc("5a5a68"))


def raw_iron(c):
    c.poly([(2, 12), (6, 5), (13, 3), (18, 9), (16, 16), (8, 18)], hexc("8a6c5c"))
    for x, y in [(6, 9), (12, 6), (11, 13), (15, 10)]:
        c.rect(x, y, x + 2, y + 2, hexc("d4aa8c"), kind="metal")
    c.dot(5, 14, hexc("6a4a3a"))


def flint(c):
    c.poly([(3, 15), (6, 5), (11, 2), (17, 7), (15, 16), (8, 18)], FLINT, kind="gem")
    c.poly([(6, 5), (11, 2), (10, 9)], mix(FLINT, (200, 214, 236), 0.25))
    c.line(7, 9, 13, 14, 1, mix(FLINT, (220, 230, 244), 0.4))


def small_rock(c):
    c.poly([(3, 13), (5, 7), (10, 4), (15, 6), (17, 12), (13, 16), (7, 16)], hexc("8a8a86"))
    c.poly([(6, 8), (10, 5), (14, 7), (11, 9)], hexc("aaa8a2"))
    c.dot(12, 13, hexc("6a6a66"))
    c.dot(6, 12, hexc("5a7a4a"))
    c.dot(7, 13, hexc("4a6a3a"))


def hide(c):
    c.poly([(6, 4), (14, 8), (26, 8), (34, 4), (32, 14), (36, 26), (30, 36), (20, 33), (10, 36), (4, 26), (8, 14)], HIDE, kind="soft")
    c.ellipse(20, 20, 8, 9, mix(HIDE, (230, 200, 160), 0.22), kind="soft")
    for x, y in [(10, 12), (29, 13), (8, 26), (31, 27), (20, 30)]:
        c.dot(x, y, HIDE_D)


def fur(c):
    c.poly([(5, 6), (14, 9), (26, 9), (35, 6), (33, 16), (36, 28), (28, 36), (20, 33), (12, 36), (4, 28), (7, 16)], FUR, kind="soft")
    for i in range(16):
        x, y = 8 + (i * 7) % 24, 10 + (i * 5) % 22
        c.line(x, y, x + 1, y + 3, 1, FUR_L if i % 2 else mul(FUR, 0.75), kind="soft")


def stitches(c, x0, y0, x1, y1, col=STITCH, step=3):
    n = int(max(abs(x1 - x0), abs(y1 - y0)) / step)
    for i in range(n + 1):
        t = i / max(1, n)
        c.paint(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, col)


def linen_shirt(c):
    c.poly([(10, 3), (16, 3), (20, 7), (24, 3), (30, 3), (39, 12), (34, 18), (31, 14), (31, 37), (9, 37), (9, 14), (6, 18), (1, 12)], LINEN, kind="soft")
    c.line(20, 8, 20, 18, 1, mul(LINEN, 0.75))
    for y in (10, 13, 16):
        c.dot(21, y, hexc("8a6a4a"))
    c.rect(9, 33, 31, 35, hexc("b89a6a"))
    stitches(c, 9, 36, 31, 36, mul(LINEN, 0.8), 2)


def linen_trousers(c):
    c.poly([(8, 2), (32, 2), (35, 38), (23, 38), (20, 16), (17, 38), (5, 38)], hexc("b8ae94"), kind="soft")
    c.rect(8, 2, 32, 6, LEATHER)
    c.rect(18, 2, 22, 6, GOLD, kind="metal")
    stitches(c, 12, 8, 9, 36, mul(hexc("b8ae94"), 0.8), 2)
    stitches(c, 28, 8, 31, 36, mul(hexc("b8ae94"), 0.8), 2)


def hide_boots(c):
    for x in (1, 21):
        c.poly([(x + 2, 1), (x + 9, 1), (x + 9, 11), (x + 17, 13), (x + 17, 18), (x + 1, 18)], HIDE)
        c.rect(x + 2, 1, x + 9, 4, FUR_L, kind="soft")
        c.rect(x + 1, 16, x + 17, 18, LEATHER_D)
        stitches(c, x + 9, 12, x + 16, 14, STITCH, 2)


def reed_hat(c):
    c.ellipse(20, 14, 19, 4.5, REED)
    c.poly([(11, 13), (14, 3), (26, 3), (29, 13)], REED_D)
    for x in range(13, 28, 2):
        c.paint(x, 6, mul(REED_D, 0.8))
    c.rect(11, 9, 29, 11, hexc("8a3a2a"))
    c.line(26, 8, 31, 2, 1.5, hexc("c84848"), kind="soft")


def reed_cloak(c):
    c.poly([(14, 2), (26, 2), (30, 8), (38, 56), (30, 58), (20, 55), (10, 58), (2, 56), (10, 8)], hexc("8a9654"), kind="soft")
    for x in range(8, 33, 4):
        c.line(x, 14, x + (x - 20) * 0.15, 54, 1, hexc("6e7a40"))
    c.rect(12, 6, 28, 9, LEATHER)
    c.ellipse(20, 7.5, 2.5, 2.5, GOLD, kind="metal")
    c.dot(19, 6, GOLD_L)


def fur_hood(c):
    c.ellipse(20, 20, 17, 18, FUR, kind="soft")
    c.ellipse(20, 24, 10, 11, hexc("2a201a"))
    c.ellipse(20, 26, 7, 7, hexc("1c1612"), grain=0.0)
    for i in range(10):
        a = math.pi * (0.05 + i * 0.1)
        c.line(20 + math.cos(a) * 12, 24 - math.sin(a) * 12, 20 + math.cos(a) * 16, 24 - math.sin(a) * 16, 1, FUR_L, kind="soft")


def fur_coat(c):
    c.poly([(10, 4), (16, 3), (20, 8), (24, 3), (30, 4), (38, 18), (34, 26), (31, 20), (32, 57), (8, 57), (9, 20), (6, 26), (2, 18)], FUR, kind="soft")
    c.rect(17, 8, 23, 57, FUR_L, kind="soft")
    c.rect(8, 53, 32, 57, FUR_L, kind="soft")
    c.rect(8, 30, 32, 33, LEATHER)
    c.rect(18, 29, 22, 34, GOLD, kind="metal")
    for y in (16, 24, 42):
        c.dot(24, y, GOLD_L)


def hide_leggings(c):
    c.poly([(8, 2), (32, 2), (35, 38), (23, 38), (20, 16), (17, 38), (5, 38)], HIDE)
    c.rect(8, 2, 32, 6, LEATHER_D)
    c.rect(18, 2, 22, 6, BRASS_C, kind="metal")
    for y in (12, 22, 32):
        c.line(7, y, 14, y, 1, CORD)
        c.line(26, y, 33, y, 1, CORD)
    stitches(c, 20, 8, 20, 15, STITCH, 2)


def bark_helm(c):
    c.poly([(5, 26), (6, 12), (12, 5), (20, 3), (28, 5), (34, 12), (35, 26), (30, 34), (26, 26), (14, 26), (10, 34)], BARK)
    for x in (12, 20, 28):
        c.line(x, 6, x, 24, 1, BARK_D)
    binding(c, 4, 22, 36, 26)
    c.dot(16, 10, hexc("5a8a34"))
    c.dot(24, 14, hexc("5a8a34"))


def bark_vest(c):
    c.poly([(9, 3), (16, 3), (20, 10), (24, 3), (31, 3), (35, 12), (33, 57), (7, 57), (5, 12)], BARK)
    for y in range(14, 56, 8):
        c.rect(7, y, 33, y + 1, BARK_D)
    c.line(20, 12, 20, 56, 1, CORD)
    for y in range(16, 56, 8):
        c.dot(20, y, GOLD_L)


def hide_jerkin(c):
    c.poly([(9, 3), (16, 3), (20, 10), (24, 3), (31, 3), (35, 12), (34, 57), (6, 57), (5, 12)], HIDE)
    c.line(20, 11, 20, 56, 1, HIDE_D)
    for y in range(16, 54, 7):
        c.line(17, y, 23, y + 2, 1, CORD)
    c.rect(6, 48, 34, 52, LEATHER_D)
    c.rect(17, 47, 23, 53, BRASS_C, kind="metal")
    stitches(c, 8, 12, 7, 46, STITCH, 2)
    stitches(c, 32, 12, 33, 46, STITCH, 2)


def reed_satchel(c):
    c.line(10, 6, 30, 6, 2, LEATHER)
    c.line(8, 22, 10, 6, 2, LEATHER)
    c.line(32, 22, 30, 6, 2, LEATHER)
    c.rect(5, 20, 55, 56, hexc("a09a5c"))
    for x in range(8, 54, 4):
        c.line(x, 22, x, 54, 1, hexc("8a8448"))
    for y in range(38, 54, 4):
        c.line(6, y, 54, y, 1, hexc("b4ac6c"))
    c.poly([(5, 20), (55, 20), (52, 34), (8, 34)], hexc("b4ac6c"))
    c.rect(26, 30, 34, 37, BRASS_C, kind="metal")
    c.dot(29, 32, GOLD_L)


def hide_pack(c):
    c.line(22, 8, 58, 8, 3, LEATHER_D)
    c.line(22, 8, 16, 17, 3, LEATHER_D)
    c.line(58, 8, 64, 17, 3, LEATHER_D)
    c.rect(10, 16, 70, 76, HIDE)
    c.poly([(10, 16), (70, 16), (68, 38), (12, 38)], mix(HIDE, (230, 190, 140), 0.15))
    c.rect(18, 46, 38, 70, HIDE_D)
    c.rect(42, 46, 62, 70, HIDE_D)
    stitches(c, 12, 18, 12, 74, STITCH, 3)
    stitches(c, 68, 18, 68, 74, STITCH, 3)
    stitches(c, 19, 47, 37, 47, STITCH, 3)
    stitches(c, 43, 47, 61, 47, STITCH, 3)
    for x in (24, 54):
        c.line(x, 36, x, 46, 2, LEATHER_D)
        c.rect(x - 2, 44, x + 3, 48, BRASS_C, kind="metal")
    c.rect(36, 33, 44, 41, GOLD, kind="metal")
    gem(c, 40, 37, 1.6, RUBY)


def raw_meat(c):
    c.ellipse(10, 11, 8, 6.5, MEAT)
    c.ellipse(12, 9, 3, 2, MEAT_FAT, kind="soft")
    c.line(4, 14, 9, 11, 1, hexc("d87a72"))
    c.ellipse(5, 14, 2, 2, hexc("f4ece0"))


def roast_meat(c):
    c.ellipse(11, 9, 7, 6, hexc("8a4a22"))
    c.ellipse(12, 8, 4, 3, hexc("b06a34"))
    c.line(6, 13, 2, 18, 2.5, hexc("f4ece0"))
    c.ellipse(2, 18, 1.5, 1.5, hexc("f4ece0"))
    c.dot(9, 6, hexc("e0a060"))


def meat_strips(c):
    for x, y in [(4, 4), (9, 3), (14, 5)]:
        c.line(x, y, x - 1, y + 12, 3.5, MEAT)
        c.line(x + 1, y + 2, x, y + 9, 1, MEAT_FAT)


def chopped_reeds(c):
    for x, y in [(5, 13), (10, 14), (15, 13), (7, 9), (13, 9), (10, 5)]:
        c.ellipse(x, y, 3, 2.5, hexc("e8e0b8"))
        c.ellipse(x, y, 1.5, 1, hexc("9eb466"))


def fish_flakes(c):
    for x, y, a in [(5, 12, 0.3), (11, 14, -0.2), (15, 9, 0.5), (8, 7, -0.4), (12, 4, 0.2)]:
        c.line(x - 3 * math.cos(a), y - 3 * math.sin(a), x + 3 * math.cos(a), y + 3 * math.sin(a), 2.5, hexc("c08850"))
        c.dot(x, y, hexc("8a5428"))


def roast_slices(c):
    for x, y in [(7, 8), (13, 12)]:
        c.ellipse(x, y, 5.5, 4.5, hexc("8a4a22"))
        c.ellipse(x, y, 3.5, 2.5, hexc("c0705a"), kind="soft")


def reed_broth(c):
    c.poly([(3, 9), (17, 9), (15, 18), (5, 18)], hexc("8a5a3c"))
    c.rect(2, 7, 18, 10, hexc("9a6a48"))
    c.ellipse(10, 8, 7, 1.6, hexc("8aa050"), kind="soft")
    for x in (5, 9, 13):
        c.dot(x, 13, hexc("c09a70"))
    steam(c, (7, 12), 6, 1)


def bog_stew(c):
    clay_pot(c, 1, 19, 18, 38, hexc("6a4a34"))
    c.ellipse(10, 19, 8, 1.8, hexc("5a3418"), kind="soft")
    c.dot(7, 19, MEAT)
    c.dot(12, 19, hexc("e8e0b8"))
    c.dot(14, 19, hexc("5a2a74"))
    steam(c, (5, 10, 15), 16, 5)


def draught(c, brew, glow):
    """A stoppered glass flask of something bright, 20 x 40: a cork tied
    down with cord, a long neck, a round belly with the brew glowing in it."""
    c.rect(8, 3, 12, 8, hexc("a07850"))
    c.rect(7, 8, 13, 10, CORD)
    c.rect(8, 10, 12, 16, hexc("b4c4c8"), kind="gem", grain=0.0)
    c.ellipse(10, 26, 8.5, 10.5, hexc("a8b8bc"), kind="gem", grain=0.0)
    c.ellipse(10, 28, 7, 8, brew, kind="glow", grain=0.02)
    c.ellipse(10, 30, 4.5, 5, mix(brew, glow, 0.35), kind="glow", grain=0.0)
    c.rect(4, 21, 17, 22, mix(brew, glow, 0.5))
    c.line(6, 19, 6, 26, 1, (250, 252, 255))
    c.dot(7, 18, (250, 252, 255))
    c.dot(13, 33, glow)
    c.dot(9, 35, glow)


def levitation_draught(c):
    draught(c, hexc("6cb4e8"), hexc("e8f8ff"))


def light_draught(c):
    draught(c, hexc("e8c440"), hexc("fff8c8"))


def warmth_draught(c):
    draught(c, hexc("d8602a"), hexc("ffc890"))


def focus_tonic(c):
    draught(c, hexc("8c50c8"), hexc("ecd0ff"))


def spirit_ember(c):
    c.ellipse(10, 11, 6, 7, hexc("4aa4e8"), kind="glow")
    c.poly([(7, 6), (10, 0), (13, 6)], hexc("4aa4e8"), kind="glow")
    c.ellipse(10, 12, 3.5, 4, hexc("b8ecff"), kind="glow")
    c.dot(10, 12, (255, 255, 255))
    c.dot(4, 5, hexc("8ad8ff"))
    c.dot(16, 8, hexc("8ad8ff"))
    c.dot(15, 17, hexc("8ad8ff"))


def old_coin(c):
    c.ellipse(10, 10, 7.5, 7.5, GOLD, kind="metal")
    c.ellipse(10, 10, 5, 5, hexc("b08428"), kind="metal")
    c.rect(9, 7, 11, 13, GOLD_L)
    c.dot(8, 8, GOLD_L)
    c.dot(12, 12, GOLD_L)


def mold_bloom(c):
    c.ellipse(10, 12, 7, 6, hexc("6a8a4a"), kind="soft")
    c.ellipse(10, 10, 5, 4, hexc("9ab86a"), kind="soft")
    for x, y in [(6, 12), (13, 9), (11, 14), (8, 8)]:
        c.dot(x, y, hexc("e4f4b4"))
    c.dot(4, 6, hexc("c8e890"))
    c.dot(16, 5, hexc("c8e890"))


def deepstone_shard(c):
    c.poly([(4, 17), (7, 6), (11, 1), (16, 8), (14, 18)], hexc("343c4c"), kind="gem")
    c.poly([(8, 7), (11, 3), (13, 9), (10, 12)], hexc("6a7a96"), kind="gem")
    c.line(8, 15, 13, 10, 1, hexc("8ac4f0"))
    c.dot(11, 4, hexc("d8f0ff"))


def warden_sigil(c):
    c.ellipse(10, 10, 8.5, 8.5, hexc("8a7448"), kind="metal")
    c.ellipse(10, 10, 6.5, 6.5, hexc("3a3428"))
    c.line(10, 5, 10, 15, 1, hexc("a8e070"))
    c.line(6, 8, 14, 12, 1, hexc("a8e070"))
    c.line(6, 12, 14, 8, 1, hexc("a8e070"))
    c.dot(10, 10, hexc("f0ffd0"))
    for x, y in [(10, 1), (19, 10), (10, 19), (1, 10)]:
        c.dot(x, y, hexc("c8a860"))


def urn_sprite(c, body, band, mark):
    """A carried urn, 40 x 40: foot, round belly with a band of glowing
    marks, gold rims on the lip and foot."""
    c.rect(13, 34, 27, 38, mul(body, 0.75))
    c.rect(12, 36, 28, 38, GOLD, kind="metal")
    c.ellipse(20, 23, 13, 12, body)
    c.rect(14, 8, 26, 14, mul(body, 0.85))
    c.rect(11, 5, 29, 9, GOLD, kind="metal")
    c.rect(15, 5, 25, 7, mul(body, 0.35))
    c.rect(8, 20, 32, 25, band)
    for x in range(9, 32):
        c.paint(x, 20, mul(GOLD, 0.95))
        c.paint(x, 24, mul(GOLD, 0.8))
    for x in (11, 17, 23, 29):
        c.dot(x, 22, mark)
        c.dot(x - 1, 22, mix(mark, band, 0.5))
        c.dot(x + 1, 22, mix(mark, band, 0.5))
    for x in (14, 20, 26):
        c.dot(x, 30, mix(body, band, 0.6))


def tool_urn(c):
    urn_sprite(c, hexc("8c8e96"), hexc("4e5058"), hexc("f0ecd8"))


def hearth_urn(c):
    urn_sprite(c, hexc("b8503a"), hexc("6a2a1c"), hexc("ffc070"))


def star_urn(c):
    urn_sprite(c, hexc("464e9c"), hexc("22264e"), hexc("ffe890"))


def golem_urn(c):
    urn_sprite(c, hexc("6a806a"), hexc("3a4a40"), hexc("a8f0c0"))


def carpentry_urn(c):
    urn_sprite(c, hexc("9e7a4c"), hexc("5a4028"), hexc("f0d8a0"))


def gear_urn(c):
    urn_sprite(c, hexc("80424c"), hexc("4a2228"), hexc("f0b0b8"))


def vehicle_urn(c):
    urn_sprite(c, hexc("6a808e"), hexc("3a4a54"), hexc("c8e8f8"))


def portable_bedroll(c):
    """A rolled bedroll tied with two leather straps, 40 x 20."""
    c.ellipse(20, 10, 18, 7, hexc("546638"), kind="soft")
    c.ellipse(5, 10, 3, 6, hexc("7a8a50"), kind="soft")
    c.ellipse(5, 10, 1.5, 3, hexc("3e4a28"))
    for y in range(5, 16, 3):
        c.paint(20, y, hexc("6a7a48"))
    for x in (12, 26):
        c.rect(x, 3, x + 3, 17, LEATHER)
        c.dot(x + 1, 9, GOLD_L)


def portable_tent(c):
    """A folded hide tent strapped to its poles, 40 x 40."""
    p = c.line(6, 36, 30, 4, 2, WOOD)
    q = c.line(12, 37, 36, 6, 2, WOOD)
    c.grain(p)
    c.grain(q)
    c.rect(6, 14, 34, 30, HIDE, kind="soft")
    c.rect(6, 20, 34, 22, HIDE_D)
    stitches(c, 7, 15, 33, 15, STITCH, 2)
    for x in (12, 26):
        c.rect(x, 12, x + 3, 32, LEATHER)
        c.dot(x + 1, 22, GOLD_L)


def airship(c):
    """A little airship, 60 x 40: hide envelope over a wooden gondola."""
    c.ellipse(30, 13, 26, 10, HIDE, kind="soft")
    for x in (14, 22, 30, 38, 46):
        for y in range(5, 22):
            if c.part.get((x, y)):
                c.paint(x, y, mul(HIDE, 0.82))
    c.rect(6, 12, 54, 14, hexc("8c3828"))
    c.poly([(52, 6), (58, 2), (58, 13), (52, 13)], hexc("8c3828"))
    for x in (18, 42):
        c.line(x, 21, x - 2 if x < 30 else x + 2, 29, 1, CORD)
    p = c.poly([(12, 29), (48, 29), (44, 36), (16, 36)], WOOD)
    c.grain(p, vertical=False)
    c.rect(12, 28, 48, 30, BRASS_C, kind="metal")
    c.rect(47, 31, 51, 34, hexc("4a4a50"), kind="metal")
    c.line(52, 28, 52, 37, 2, WOOD_D)


def iron_helm(c):
    c.poly([(5, 28), (6, 12), (12, 5), (20, 3), (28, 5), (34, 12), (35, 28), (30, 34), (26, 28), (14, 28), (10, 34)], IRON, kind="metal", grain=0.03)
    c.rect(19, 3, 22, 27, mul(IRON, 1.15), kind="metal")
    c.rect(4, 23, 36, 28, mul(IRON, 0.75), kind="metal")
    for x in (8, 14, 26, 32):
        rivet(c, x, 25, IRON_L)
    c.rect(18, 22, 23, 32, mul(IRON, 0.85), kind="metal")


def iron_mail(c):
    c.poly([(9, 3), (16, 3), (20, 10), (24, 3), (31, 3), (35, 12), (34, 57), (6, 57), (5, 12)], IRON, kind="metal")
    for y in range(12, 56, 2):
        for x in range(7 + (y // 2) % 2, 34, 2):
            c.paint(x, y, mul(IRON, 0.7))
    c.rect(6, 48, 34, 52, LEATHER_D)
    c.rect(17, 47, 23, 53, BRASS_C, kind="metal")
    c.rect(9, 3, 31, 6, LEATHER)


def linen_robe(c):
    c.poly([(11, 3), (16, 3), (20, 8), (24, 3), (29, 3), (38, 14), (33, 19), (30, 15), (34, 57), (6, 57), (10, 15), (7, 19), (2, 14)], hexc("e4dcc4"), kind="soft")
    c.line(20, 9, 20, 57, 1, hexc("c8bea2"))
    c.rect(9, 30, 31, 33, hexc("8a3a2a"))
    c.rect(18, 29, 22, 34, GOLD, kind="metal")
    c.rect(6, 53, 34, 55, hexc("b09060"))


def sun_hat(c):
    c.ellipse(20, 14, 19, 5, hexc("d8c078"))
    c.poly([(12, 13), (15, 4), (25, 4), (28, 13)], hexc("c4a860"))
    for x in range(14, 27, 2):
        c.paint(x, 7, hexc("a88c48"))
    c.rect(12, 10, 28, 12, hexc("8a3a2a"))
    c.dot(15, 11, GOLD_L)


def fur_boots(c):
    for x in (1, 21):
        c.poly([(x + 2, 1), (x + 10, 1), (x + 10, 11), (x + 17, 13), (x + 17, 18), (x + 1, 18)], FUR, kind="soft")
        c.rect(x + 1, 1, x + 11, 5, FUR_L, kind="soft")
        c.rect(x + 1, 16, x + 17, 18, LEATHER_D)


def reed_breather(c):
    c.rect(8, 2, 12, 26, REED)
    for y in (7, 13, 19):
        c.rect(8, y, 12, y + 1, REED_D)
    c.ellipse(10, 30, 8, 7, HIDE)
    c.ellipse(10, 31, 4, 3, hexc("2a201a"))
    c.rect(7, 23, 13, 26, BRASS_C, kind="metal")
    c.line(2, 30, 0, 26, 1, CORD)
    c.line(18, 30, 20, 26, 1, CORD)


def barley(c):
    c.line(10, 19, 10, 6, 1, hexc("b89a50"))
    for i, y in enumerate(range(3, 12, 2)):
        c.ellipse(9 if i % 2 else 11, y, 1.5, 2, hexc("dcbc6a"))
    for x0 in (6, 14):
        c.line(x0, 19, 10, 10, 1, hexc("c8a85a"))
    c.line(12, 3, 15, 0, 1, hexc("e8d08a"))
    binding(c, 7, 14, 13, 16)


def mire_tuber(c):
    c.ellipse(10, 12, 7, 5.5, hexc("8a6a48"))
    c.ellipse(13, 9, 3, 2.5, hexc("9a7a54"))
    for x, y in [(7, 11), (11, 14), (14, 11)]:
        c.dot(x, y, hexc("5a4030"))
    c.line(5, 7, 3, 2, 1, hexc("6a9a40"))
    c.line(6, 7, 8, 1, 1, hexc("5a8a34"))


def iron_shot(c):
    for x, y in [(6, 7), (13, 7), (9.5, 13), (5, 15), (15, 15)]:
        c.ellipse(x, y, 3, 3, IRON, kind="metal")
        c.dot(int(x) - 1, int(y) - 1, IRON_L)


DRAW = {f.__name__: f for f in [
    stone_pickaxe, stone_axe, stone_shovel, flint_spear, iron_sword, steel_sword, spirit_sword, torch, marsh_berries, swamp_reed, smoked_fish, bread, peat_stew,
    cured_meat, feast_platter, ember_herb, stick, plant_fiber, cordage, coal, raw_iron, flint, hide, fur, linen_shirt,
    linen_trousers, hide_boots, reed_hat, reed_cloak, fur_hood, fur_coat, hide_leggings, bark_helm, bark_vest, hide_jerkin,
    reed_satchel, hide_pack, raw_meat, roast_meat, spirit_ember, old_coin, small_rock, rough_axe,
    meat_strips, chopped_reeds, fish_flakes, roast_slices, reed_broth, bog_stew,
    iron_pickaxe, iron_axe, iron_shovel, steel_pickaxe, steel_axe, spirit_pickaxe, spirit_axe,
    levitation_draught, light_draught, warmth_draught, focus_tonic,
    tool_urn, hearth_urn, star_urn, golem_urn, portable_bedroll, portable_tent, airship,
    carpentry_urn, gear_urn, vehicle_urn, iron_dagger, rapier, makeshift_pistol, rifle, iron_helm, iron_mail,
    mold_bloom, deepstone_shard, warden_sigil, linen_robe, sun_hat, fur_boots, reed_breather,
    barley, mire_tuber, iron_shot,
]}


def fallback(c):
    """For an item with no drawing yet: a bundle tied with cord."""
    w, h = c.w, c.h
    c.ellipse(w / 2, h / 2 + 1, w / 2 - 3, h / 2 - 3, hexc("8a7a5a"))
    c.line(3, h / 2, w - 3, h / 2, 1, CORD)

# --- blocks ------------------------------------------------------------------


def block_textures():
    tiles_ron = open(os.path.join(ROOT, "assets", "textures", "blocks.ron")).read()
    tiles = {m[0]: int(m[1]) for m in re.findall(r'"(\w+)":\s*(\d+)', tiles_ron)}
    size = int(re.search(r"tile_size:\s*(\d+)", tiles_ron).group(1))
    cols = int(re.search(r"columns:\s*(\d+)", tiles_ron).group(1))
    sheet = Image.open(os.path.join(ROOT, "assets", "textures", "blocks.png")).convert("RGBA")
    blocks = {}
    for line in open(os.path.join(ROOT, "assets", "data", "blocks.ron")):
        m = re.search(r'name:\s*"(\w+)"', line)
        if not m:
            continue
        top = re.search(r'top:\s*"(\w+)"', line)
        side = re.search(r'side:\s*"(\w+)"', line)
        if top:
            t = top.group(1)
            blocks[m.group(1)] = (t, side.group(1) if side else t)

    def tile(name):
        i = tiles[name]
        x, y = (i % cols) * size, (i // cols) * size
        return sheet.crop((x, y, x + size, y + size))

    return blocks, tile


def iso_cube(c, top, side, shade=(1.0, 0.8, 0.6)):
    """A little cube seen from above and the front-left: the top, the left
    face and the right face, each sampled from its texture."""
    s = c.w - 2  # 1 px for the outline on each side
    half, q = s / 2.0, s / 4.0
    ts = top.size[0]
    tp, sp = top.load(), side.load()
    faces = [c.new_part(), c.new_part(), c.new_part()]

    def sample(img, a, b):
        return img[min(ts - 1, max(0, int(a * ts))), min(ts - 1, max(0, int(b * ts)))]

    for y in range(c.h):
        for x in range(c.w):
            fx, fy = x + 0.5 - 1, y + 0.5 - 1
            # Top: a diamond with corners (half, 0), (s, q), (half, 2q), (0, q).
            a = ((fx - half) / half + fy / q) / 2
            b = (fy / q - (fx - half) / half) / 2
            if 0 <= a <= 1 and 0 <= b <= 1:
                p, face, k = sample(tp, a, b), 0, shade[0]
            elif 0 <= fx < half:
                y0 = q + fx / half * q
                if not y0 <= fy <= y0 + 2 * q:
                    continue
                p, face, k = sample(sp, fx / half, (fy - y0) / (2 * q)), 1, shade[1]
            elif half <= fx <= s:
                y0 = 2 * q - (fx - half) / half * q
                if not y0 <= fy <= y0 + 2 * q:
                    continue
                p, face, k = sample(sp, (fx - half) / half, (fy - y0) / (2 * q)), 2, shade[2]
            else:
                continue
            if p[3] > 0:
                c.put(x, y, mul(p, k), faces[face], 0.0)
    c.flat.update(faces)


def chest_extras(c):
    """A brass band and latch on a chest's front faces."""
    s = c.w - 2
    half, q = s / 2.0, s / 4.0
    band = c.new_part(flat=True)
    for x in range(c.w):
        fx = x + 0.5 - 1
        y0 = (q + fx * (q / half)) if fx <= half else (2 * q - (fx - half) * (q / half))
        if 0 <= fx <= s:
            c.put(x, int(1 + y0 + 2 * q * 0.38), hexc("c8a040"), band, 0.0)
    c.rect(1 + half - 4, 1 + 2 * q + 2 * q * 0.3, 1 + half - 1, 1 + 2 * q + 2 * q * 0.55, hexc("e8c860"), c.new_part(flat=True), 0.0)


# --- sheets ------------------------------------------------------------------


def read_ron(path):
    if not os.path.exists(path):
        return {}
    return {m[0]: tuple(int(v) for v in m[1:]) for m in re.findall(r'"(\w+)":\s*\((\d+),\s*(\d+),\s*(\d+),\s*(\d+)\)', open(path).read())}


def write_ron(path, sheet, rects):
    lines = [
        f"// Item name -> (x, y, width, height) of its sprite in {sheet}.png.",
        "// 20 pixels per inventory cell. Made by tools/gen_item_art.py; safe to edit by hand",
        "// (move a sprite in the PNG and change its numbers here).",
        "(",
        f"    cell: {CELL},",
        "    sprites: {",
    ]
    for name, (x, y, w, h) in rects.items():
        lines.append(f'        "{name}": ({x}, {y}, {w}, {h}),')
    lines += ["    },", ")", ""]
    open(path, "w").write("\n".join(lines))


def place(rects, w, h):
    """Find room for a w x h sprite below and beside the ones already there
    (rows of sprites, 2 px apart)."""
    gap = 2
    taken = list(rects.values())

    def free(x, y):
        return all(x + w + gap <= rx or rx + rw + gap <= x or y + h + gap <= ry or ry + rh + gap <= y for rx, ry, rw, rh in taken)

    y = 0
    while True:
        for x in range(0, SHEET_W - w + 1, 2):
            if free(x, y):
                return x, y
        y += 2


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    reset = "--reset" in sys.argv
    os.makedirs(OUT, exist_ok=True)
    items = read_items()
    blocks, tile = block_textures()
    for sheet in ("items", "weapons", "blocks"):
        png, ron = os.path.join(OUT, sheet + ".png"), os.path.join(OUT, sheet + ".ron")
        fresh = reset and (not args or sheet in args)
        rects = {} if fresh else read_ron(ron)
        old = None if fresh or not os.path.exists(png) else Image.open(png).convert("RGBA")
        wanted = [(n, k, b, s) for n, k, b, s in items if sheet_for(k) == sheet]
        new = []
        for i, (name, kind, block, (w, h)) in enumerate(wanted):
            if name in rects:
                continue
            pw, ph = w * CELL, h * CELL
            c = Canvas(pw, ph, seed=i + 7)
            if kind == "Block":
                top, side = blocks.get(block, ("dirt", "dirt"))
                iso_cube(c, tile(top), tile(side))
                if block == "chest":
                    chest_extras(c)
            else:
                DRAW.get(name, fallback)(c)
                if name not in DRAW:
                    print(f"  {name}: no drawing yet, made a plain bundle")
            rects[name] = place(rects, pw, ph) + (pw, ph)
            new.append((name, c.finish()))
        height = max([y + h for _, y, _, h in rects.values()] + [16])
        height = (height + 15) // 16 * 16
        img = Image.new("RGBA", (SHEET_W, height), (0, 0, 0, 0))
        if old is not None:
            img.paste(old.crop((0, 0, min(SHEET_W, old.width), min(height, old.height))), (0, 0))
        for name, sprite in new:
            x, y, _, _ = rects[name]
            img.paste(sprite, (x, y))
        img.save(png)
        write_ron(ron, sheet, rects)
        print(f"{sheet}: {len(rects)} sprites ({len(new)} new), {SHEET_W}x{height}")


if __name__ == "__main__":
    main()
