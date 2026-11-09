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
    `finish` shades every part (light from the top left) and outlines the
    whole thing."""

    def __init__(self, w, h, seed=1):
        self.w, self.h = w, h
        self.col = {}
        self.part = {}
        self.parts = 0
        self.seed = seed
        self.flat = set()

    def new_part(self, flat=False):
        self.parts += 1
        if flat:
            self.flat.add(self.parts)
        return self.parts

    def put(self, x, y, c, part, grain=0.06):
        x, y = int(x), int(y)
        if 0 <= x < self.w and 0 <= y < self.h:
            n = ((x * 73856093) ^ (y * 19349663) ^ (self.seed * 83492791)) & 0xFFFF
            k = 1.0 + grain * ((n / 0xFFFF) * 2.0 - 1.0)
            self.col[(x, y)] = mul(c, k)
            self.part[(x, y)] = part

    def rect(self, x0, y0, x1, y1, c, part=None, grain=0.06):
        part = part or self.new_part()
        for y in range(int(y0), int(y1)):
            for x in range(int(x0), int(x1)):
                self.put(x, y, c, part, grain)
        return part

    def ellipse(self, cx, cy, rx, ry, c, part=None, grain=0.06):
        part = part or self.new_part()
        for y in range(int(cy - ry - 1), int(cy + ry + 2)):
            for x in range(int(cx - rx - 1), int(cx + rx + 2)):
                dx, dy = (x + 0.5 - cx) / rx, (y + 0.5 - cy) / ry
                if dx * dx + dy * dy <= 1.0:
                    self.put(x, y, c, part, grain)
        return part

    def poly(self, pts, c, part=None, grain=0.06):
        part = part or self.new_part()
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

    def line(self, x0, y0, x1, y1, width, c, part=None, grain=0.06):
        part = part or self.new_part()
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

    def finish(self):
        img = Image.new("RGBA", (self.w, self.h), (0, 0, 0, 0))
        px = img.load()
        for (x, y), c in self.col.items():
            p = self.part[(x, y)]
            if p not in self.flat:
                up, left = self.part.get((x, y - 1)), self.part.get((x - 1, y))
                down, right = self.part.get((x, y + 1)), self.part.get((x + 1, y))
                if up != p or left != p:
                    c = mix(c, (255, 248, 225), 0.22)
                elif down != p or right != p:
                    c = mul(c, 0.72)
            px[x, y] = c + (255,)
        # A dark outline just outside the shape, in a deep shade of what it
        # touches.
        for y in range(self.h):
            for x in range(self.w):
                if (x, y) in self.col:
                    continue
                near = [self.col[q] for q in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)) if q in self.col]
                if near:
                    px[x, y] = mul(near[0], 0.28) + (255,)
        return img


# Palette.
WOOD, WOOD_D = hexc("7a5230"), hexc("5a3a20")
STONE, STONE_L = hexc("8a8a8e"), hexc("a8a8ac")
FLINT = hexc("4c5262")
CORD = hexc("b8a072")
LINEN = hexc("dcd2b8")
HIDE, HIDE_D = hexc("8c5a36"), hexc("6a4228")
FUR, FUR_L = hexc("6e5644"), hexc("9a8068")
REED, REED_D = hexc("b8ac6a"), hexc("8a8648")
BARK, BARK_D = hexc("5e4630"), hexc("3e2e20")
MEAT, MEAT_FAT = hexc("b84a46"), hexc("e8c0b0")


# --- weapons and tools -------------------------------------------------------


def handle(c, x, y0, y1, w=3):
    p = c.rect(x, y0, x + w, y1, WOOD)
    # Grip wrap near the bottom.
    for y in range(y1 - 12, y1 - 4, 2):
        c.rect(x, y, x + w, y + 1, CORD, c.new_part())
    return p


def stone_pickaxe(c):
    handle(c, 18, 10, 59, 4)
    c.poly([(2, 14), (8, 7), (20, 4), (32, 7), (38, 14), (33, 12), (20, 10), (7, 12)], STONE)
    c.rect(16, 5, 24, 13, CORD)


def stone_axe(c):
    handle(c, 4, 4, 59, 3)
    c.poly([(7, 6), (16, 3), (19, 8), (19, 20), (16, 24), (7, 18)], STONE)
    c.line(17, 5, 17, 22, 1, STONE_L)
    c.rect(3, 9, 8, 15, CORD)


def stone_shovel(c):
    handle(c, 8, 20, 59, 3)
    c.rect(6, 54, 13, 57, WOOD_D)
    c.poly([(3, 6), (6, 2), (13, 2), (16, 6), (16, 18), (12, 22), (7, 22), (3, 18)], STONE)
    c.rect(8, 17, 11, 24, CORD)


def flint_spear(c):
    handle(c, 8, 18, 79, 3)
    c.poly([(9.5, 0), (14, 8), (13, 16), (9.5, 19), (6, 16), (5, 8)], FLINT)
    c.line(9.5, 2, 9.5, 16, 1, mix(FLINT, (200, 210, 230), 0.4))
    c.rect(7, 16, 12, 22, CORD)


# --- items -------------------------------------------------------------------


def torch(c):
    c.rect(8, 14, 12, 39, WOOD)
    c.rect(7, 12, 13, 17, CORD)
    c.ellipse(10, 9, 5, 7, hexc("f0a030"))
    c.ellipse(10, 10, 3, 4, hexc("ffe080"))


def marsh_berries(c):
    c.poly([(9, 2), (15, 1), (13, 6)], hexc("4a7a30"))
    for x, y, col in [(7, 9, "5a2a74"), (13, 9, "6c3088"), (10, 14, "4e2266"), (5, 15, "642c80"), (15, 15, "5a2a74")]:
        c.ellipse(x, y, 3.5, 3.5, hexc(col))
        c.dot(x - 1, y - 1, hexc("c8a0e0"))


def swamp_reed(c):
    for x, top in [(6, 3), (10, 1), (14, 4)]:
        c.line(x, top, x - 1 + (x - 10) * 0.2, 18, 2.5, hexc("9eb466"))
        c.rect(x - 1, 14, x + 2, 18, hexc("e8e0b8"))


def smoked_fish(c):
    c.ellipse(17, 10, 13, 6, hexc("a86a36"))
    c.poly([(30, 10), (38, 3), (37, 17)], hexc("8a5428"))
    c.dot(9, 8, (20, 16, 12))
    for x in range(14, 27, 4):
        c.line(x, 6, x - 2, 14, 1, hexc("7a4a22"))


def bread(c):
    c.ellipse(10, 11, 9, 7, hexc("c88c44"))
    for x in (6, 10, 14):
        c.line(x - 1, 7, x + 1, 11, 1, hexc("f0c880"))


def peat_stew(c):
    # A clay pot of stew with steam.
    c.poly([(3, 20), (17, 20), (16, 34), (13, 38), (7, 38), (4, 34)], hexc("8a5a3c"))
    c.rect(2, 18, 18, 21, hexc("9a6a48"))
    c.ellipse(10, 18, 7, 2, hexc("8c4a1c"))
    for x in (6, 10, 14):
        c.line(x, 15, x + 1, 6, 1, hexc("d8d8d0"))


def cured_meat(c):
    c.ellipse(10, 11, 8, 7, hexc("8c2e2a"))
    c.ellipse(8, 9, 3, 2, MEAT_FAT)
    c.line(3, 11, 17, 11, 1, CORD)


def feast_platter(c):
    c.ellipse(20, 24, 19, 12, hexc("c8c2b4"))
    c.ellipse(20, 23, 15, 9, hexc("dcd6c8"))
    c.ellipse(23, 19, 8, 6, hexc("9a5226"))
    c.line(28, 15, 33, 9, 2, hexc("e8dcc8"))
    c.ellipse(11, 22, 4, 4, hexc("5a2a74"))
    c.ellipse(15, 27, 3, 3, hexc("6c3088"))
    c.ellipse(26, 28, 5, 3, hexc("c88c44"))


def ember_herb(c):
    c.line(10, 19, 10, 7, 1.5, hexc("4a6a2a"))
    c.poly([(10, 12), (4, 8), (9, 9)], hexc("5a8a34"))
    c.poly([(10, 14), (16, 10), (11, 11)], hexc("5a8a34"))
    c.ellipse(10, 5, 4, 4, hexc("f06018"))
    c.ellipse(10, 5, 2, 2, hexc("ffd060"))


def stick(c):
    c.line(10, 2, 10, 38, 3, WOOD)
    c.line(10, 14, 15, 9, 2, WOOD)


def plant_fiber(c):
    for i, x in enumerate((5, 8, 11, 14)):
        c.line(x, 2, x + (i - 1.5) * 1.5, 18, 1.5, hexc("8aa04c") if i % 2 else hexc("6e8a3a"))
    c.rect(4, 9, 16, 12, CORD)


def cordage(c):
    for r in (8, 5.5):
        c.ellipse(10, 10, r, r, CORD)
        c.ellipse(10, 10, r - 1.5, r - 1.5, mul(CORD, 0.8))
    c.ellipse(10, 10, 3, 3, CORD)
    c.line(14, 15, 18, 19, 2, CORD)


def coal(c):
    c.poly([(3, 10), (8, 4), (14, 5), (17, 11), (13, 17), (6, 16)], hexc("2a2a2e"))
    c.dot(8, 8, hexc("6a6a74"))
    c.dot(12, 12, hexc("5a5a64"))


def raw_iron(c):
    c.poly([(2, 12), (6, 5), (13, 3), (18, 9), (16, 16), (8, 18)], hexc("8a6c5c"))
    for x, y in [(7, 9), (12, 7), (11, 13)]:
        c.rect(x, y, x + 2, y + 2, hexc("c8a088"))


def flint(c):
    c.poly([(3, 15), (6, 5), (11, 2), (17, 7), (15, 16), (8, 18)], FLINT)
    c.line(7, 7, 13, 13, 1, mix(FLINT, (220, 230, 240), 0.35))


def hide(c):
    c.poly([(6, 4), (14, 8), (26, 8), (34, 4), (32, 14), (36, 26), (30, 36), (20, 33), (10, 36), (4, 26), (8, 14)], HIDE)
    c.ellipse(20, 20, 8, 9, mix(HIDE, (230, 200, 160), 0.2))


def fur(c):
    c.poly([(5, 6), (14, 9), (26, 9), (35, 6), (33, 16), (36, 28), (28, 36), (20, 33), (12, 36), (4, 28), (7, 16)], FUR)
    for i in range(10):
        x, y = 9 + (i * 7) % 22, 11 + (i * 5) % 20
        c.line(x, y, x + 2, y + 3, 1, FUR_L)


def linen_shirt(c):
    c.poly([(10, 3), (16, 3), (20, 7), (24, 3), (30, 3), (39, 12), (34, 18), (31, 14), (31, 37), (9, 37), (9, 14), (6, 18), (1, 12)], LINEN)
    c.line(20, 8, 20, 18, 1, mul(LINEN, 0.8))


def linen_trousers(c):
    c.poly([(8, 2), (32, 2), (35, 38), (23, 38), (20, 16), (17, 38), (5, 38)], hexc("b8ae94"))
    c.rect(8, 2, 32, 6, CORD)


def hide_boots(c):
    for x in (1, 21):
        c.poly([(x + 2, 1), (x + 9, 1), (x + 9, 11), (x + 17, 13), (x + 17, 18), (x + 1, 18)], HIDE)
        c.rect(x + 2, 1, x + 9, 3, FUR_L)


def reed_hat(c):
    c.ellipse(20, 14, 19, 4.5, REED)
    c.poly([(11, 13), (14, 3), (26, 3), (29, 13)], REED_D)
    c.rect(11, 9, 29, 11, hexc("6a4a2a"))


def reed_cloak(c):
    c.poly([(14, 2), (26, 2), (30, 8), (38, 56), (30, 58), (20, 55), (10, 58), (2, 56), (10, 8)], hexc("8a9654"))
    for x in range(8, 33, 5):
        c.line(x, 14, x + (x - 20) * 0.15, 54, 1, hexc("6e7a40"))
    c.rect(15, 3, 25, 8, CORD)


def fur_hood(c):
    c.ellipse(20, 20, 17, 18, FUR)
    c.ellipse(20, 24, 10, 11, hexc("2a201a"))
    c.ellipse(20, 26, 7, 7, hexc("1c1612"), grain=0.0)
    for i in range(8):
        a = math.pi * (0.1 + i * 0.11)
        c.line(20 + math.cos(a) * 13, 24 - math.sin(a) * 13, 20 + math.cos(a) * 16, 24 - math.sin(a) * 16, 1, FUR_L)


def fur_coat(c):
    c.poly([(10, 4), (16, 3), (20, 8), (24, 3), (30, 4), (38, 18), (34, 26), (31, 20), (32, 57), (8, 57), (9, 20), (6, 26), (2, 18)], FUR)
    c.rect(18, 8, 22, 57, FUR_L)
    for y in (16, 28, 40):
        c.dot(23, y, hexc("d8c080"))


def hide_leggings(c):
    c.poly([(8, 2), (32, 2), (35, 38), (23, 38), (20, 16), (17, 38), (5, 38)], HIDE)
    c.rect(8, 2, 32, 6, HIDE_D)
    for y in (12, 22, 32):
        c.line(7, y, 14, y, 1, CORD)
        c.line(26, y, 33, y, 1, CORD)


def bark_helm(c):
    c.poly([(5, 26), (6, 12), (12, 5), (20, 3), (28, 5), (34, 12), (35, 26), (30, 34), (26, 26), (14, 26), (10, 34)], BARK)
    for x in (12, 20, 28):
        c.line(x, 6, x, 24, 1, BARK_D)
    c.rect(4, 22, 36, 26, CORD)


def bark_vest(c):
    c.poly([(9, 3), (16, 3), (20, 10), (24, 3), (31, 3), (35, 12), (33, 57), (7, 57), (5, 12)], BARK)
    for y in range(14, 56, 8):
        c.rect(7, y, 33, y + 1, BARK_D)
    c.line(20, 12, 20, 56, 1, CORD)


def hide_jerkin(c):
    c.poly([(9, 3), (16, 3), (20, 10), (24, 3), (31, 3), (35, 12), (34, 57), (6, 57), (5, 12)], HIDE)
    c.line(20, 11, 20, 56, 1, HIDE_D)
    for y in range(16, 54, 7):
        c.line(17, y, 23, y + 2, 1, CORD)
    c.rect(6, 50, 34, 53, HIDE_D)


def reed_satchel(c):
    c.line(10, 6, 30, 6, 2, CORD)
    c.line(8, 22, 10, 6, 2, CORD)
    c.line(32, 22, 30, 6, 2, CORD)
    c.rect(5, 20, 55, 56, hexc("a09a5c"))
    for x in range(8, 54, 4):
        c.line(x, 22, x, 54, 1, hexc("8a8448"))
    c.poly([(5, 20), (55, 20), (52, 34), (8, 34)], hexc("b4ac6c"))
    c.rect(27, 30, 33, 36, hexc("6a4a2a"))


def hide_pack(c):
    c.line(22, 8, 58, 8, 3, HIDE_D)
    c.line(22, 8, 16, 17, 3, HIDE_D)
    c.line(58, 8, 64, 17, 3, HIDE_D)
    c.rect(10, 16, 70, 76, HIDE)
    c.poly([(10, 16), (70, 16), (68, 38), (12, 38)], mix(HIDE, (230, 190, 140), 0.15))
    c.rect(18, 46, 38, 70, HIDE_D)
    c.rect(42, 46, 62, 70, HIDE_D)
    for x in (24, 54):
        c.line(x, 36, x, 46, 2, CORD)
    c.rect(37, 34, 43, 40, hexc("c8b070"))


def raw_meat(c):
    c.ellipse(10, 11, 8, 6.5, MEAT)
    c.ellipse(12, 9, 3, 2, MEAT_FAT)
    c.ellipse(5, 14, 2, 2, hexc("f0e8dc"))


def roast_meat(c):
    c.ellipse(11, 9, 7, 6, hexc("8a4a22"))
    c.line(6, 13, 2, 18, 2.5, hexc("f0e8dc"))
    c.ellipse(2, 18, 1.5, 1.5, hexc("f0e8dc"))
    c.dot(9, 6, hexc("c8783a"))


def spirit_ember(c):
    c.ellipse(10, 11, 6, 7, hexc("58b4e8"))
    c.ellipse(10, 12, 3.5, 4, hexc("b8ecff"))
    c.poly([(7, 6), (10, 0), (13, 6)], hexc("58b4e8"))
    c.dot(10, 12, (255, 255, 255))


def old_coin(c):
    c.ellipse(10, 10, 7, 7, hexc("c8a040"))
    c.ellipse(10, 10, 4.5, 4.5, hexc("a88430"))
    c.rect(9, 7, 11, 13, hexc("e8c860"))


DRAW = {f.__name__: f for f in [
    stone_pickaxe, stone_axe, stone_shovel, flint_spear, torch, marsh_berries, swamp_reed, smoked_fish, bread, peat_stew,
    cured_meat, feast_platter, ember_herb, stick, plant_fiber, cordage, coal, raw_iron, flint, hide, fur, linen_shirt,
    linen_trousers, hide_boots, reed_hat, reed_cloak, fur_hood, fur_coat, hide_leggings, bark_helm, bark_vest, hide_jerkin,
    reed_satchel, hide_pack, raw_meat, roast_meat, spirit_ember, old_coin,
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
