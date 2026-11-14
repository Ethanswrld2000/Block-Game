#!/usr/bin/env python3
"""Draw a concept sheet for every station: what its urn and each piece it
grows should look like, before and after their upgrades, for painting the
real models and textures later.

  python3 tools/gen_station_concepts.py      writes docs/stations/<station>.png

Each piece is a handful of boxes drawn in a 2:1 isometric view, lit from the
top left, with a little grain per material. The in-game models
(`game::station::models`) are simpler versions of the same shapes; the
names under each drawing match the steps in assets/data/urns.ron.
"""
import math
import os

from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.join(os.path.dirname(__file__), "..")
OUT = os.path.join(ROOT, "docs", "stations")
FONT = os.path.join(ROOT, "assets", "fonts", "ui.ttf")
FONT_BOLD = os.path.join(ROOT, "assets", "fonts", "ui_bold.ttf")

U = 44  # pixels per block along an iso axis (x: U, y: U / 2); set per drawing
CELL_W, CELL_H = 300, 300


def hexc(h):
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def mul(c, k):
    return tuple(max(0, min(255, int(v * k))) for v in c)


def mix(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


# Materials: base colour, grain kind, glow.
MATS = {
    "wood": (hexc("80573a"), "grain", False),
    "wood_dark": (hexc("5a3c26"), "grain", False),
    "plank": (hexc("a07448"), "grain", False),
    "groove": (hexc("382618"), "flat", False),
    "stone": (hexc("8c8c92"), "speck", False),
    "stone_dark": (hexc("5e5e66"), "speck", False),
    "iron": (hexc("4a4c52"), "metal", False),
    "steel": (hexc("9aa4b0"), "metal", False),
    "brass": (hexc("c09048"), "metal", False),
    "reed": (hexc("aaa060"), "weave", False),
    "cloth": (hexc("566a3a"), "weave", False),
    "linen": (hexc("d8ccae"), "weave", False),
    "cord": (hexc("bca878"), "weave", False),
    "hide": (hexc("8c5a36"), "speck", False),
    "fur": (hexc("8a7058"), "speck", False),
    "moss": (hexc("4e7434"), "speck", False),
    "clay": (hexc("a8643e"), "speck", False),
    "fire": (hexc("ffa040"), "flat", True),
    "ember": (hexc("ff6a2a"), "flat", True),
    "brew": (hexc("6ae07a"), "flat", True),
    "spirit": (hexc("8cd2ff"), "flat", True),
    "crystal": (hexc("c8f4ff"), "flat", True),
    "gold": (hexc("f0c858"), "metal", True),
    "fish": (hexc("9c7a52"), "speck", False),
    "glass": (hexc("a8d8e8"), "flat", False),
    "star": (hexc("464e9c"), "speck", False),
    "star_dark": (hexc("2a2e60"), "speck", False),
}


class Scene:
    """Boxes (centre, size, material, turn about y, tilt about z) to draw."""

    def __init__(self):
        self.boxes = []

    def box(self, sx, sy, sz, x, y, z, mat, tilt=0.0, turn=0.0):
        self.boxes.append(((x, y, z), (sx, sy, sz), mat, tilt, turn))
        return self


def corners(c, s, tilt, turn):
    out = []
    for dx in (-1, 1):
        for dy in (-1, 1):
            for dz in (-1, 1):
                p = [dx * s[0] / 2, dy * s[1] / 2, dz * s[2] / 2]
                # tilt about z, then turn about y
                ct, st = math.cos(tilt), math.sin(tilt)
                p = [p[0] * ct - p[1] * st, p[0] * st + p[1] * ct, p[2]]
                cu, su = math.cos(turn), math.sin(turn)
                p = [p[0] * cu + p[2] * su, p[1], -p[0] * su + p[2] * cu]
                out.append((c[0] + p[0], c[1] + p[1], c[2] + p[2]))
    return out


FACES = [  # corner indices (dx, dy, dz bits: x*4 + y*2 + z), and the local normal
    ((4, 5, 7, 6), (1, 0, 0)),
    ((0, 2, 3, 1), (-1, 0, 0)),
    ((2, 6, 7, 3), (0, 1, 0)),
    ((0, 1, 5, 4), (0, -1, 0)),
    ((1, 3, 7, 5), (0, 0, 1)),
    ((0, 4, 6, 2), (0, 0, -1)),
]

VIEW = (1.0, 1.25, 1.0)


def proj(p, ox, oy):
    x, y, z = p
    return (ox + (x - z) * U * 0.87, oy + (x + z) * U * 0.5 - y * U)


def rot_normal(n, tilt, turn):
    ct, st = math.cos(tilt), math.sin(tilt)
    n = [n[0] * ct - n[1] * st, n[0] * st + n[1] * ct, n[2]]
    cu, su = math.cos(turn), math.sin(turn)
    return [n[0] * cu + n[2] * su, n[1], -n[0] * su + n[2] * cu]


def noise(x, y, seed):
    n = ((x * 73856093) ^ (y * 19349663) ^ (seed * 83492791)) & 0xFFFF
    return n / 0xFFFF


def draw_scene(scene, w, h, ox, oy):
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    glow = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    # Back to front: lower boxes first, then the nearer ones (larger x + z).
    order = sorted(enumerate(scene.boxes), key=lambda b: (b[1][0][0] + b[1][0][2]) + 2.0 * (b[1][0][1] - b[1][1][1] / 2))
    px = img.load()
    for idx, (c, s, mat, tilt, turn) in order:
        base, grain, glows = MATS[mat]
        cs = corners(c, s, tilt, turn)
        faces = []
        for ids, n in FACES:
            rn = rot_normal(n, tilt, turn)
            facing = rn[0] * VIEW[0] + rn[1] * VIEW[1] + rn[2] * VIEW[2]
            if facing <= 0.01:
                continue
            light = 1.08 if rn[1] > 0.5 else (0.86 if rn[0] > rn[2] else 0.7)
            if glows:
                light = 0.95 + 0.1 * light
            pts = [proj(cs[i], ox, oy) for i in ids]
            faces.append((pts, light, rn))
        for pts, light, rn in faces:
            mask = Image.new("L", (w, h), 0)
            ImageDraw.Draw(mask).polygon(pts, fill=255)
            bbox = mask.getbbox()
            if not bbox:
                continue
            mp = mask.load()
            col = mul(base, light)
            for y in range(bbox[1], bbox[3]):
                for x in range(bbox[0], bbox[2]):
                    if not mp[x, y]:
                        continue
                    k = 1.0
                    r = noise(x, y, idx + 1)
                    if grain == "grain":
                        k += 0.08 * math.sin((y if rn[1] < 0.5 else x * 0.5 + y) * 1.3 + idx) + (r - 0.5) * 0.08
                    elif grain == "speck":
                        k += (r - 0.5) * 0.22
                    elif grain == "weave":
                        k += 0.07 if (x + y) % 4 < 2 else -0.05
                    elif grain == "metal":
                        k += (r - 0.5) * 0.06
                    px[x, y] = mul(col, k) + (255,)
            # Edges: a lighter top-left rim, a dark outline.
            ImageDraw.Draw(img).line(pts + [pts[0]], fill=mul(base, 0.32) + (255,), width=1)
        if glows:
            cx, cy = proj(c, ox, oy)
            r = max(s) * U * 1.2 + 10
            for k in range(6, 0, -1):
                rr = r * k / 6
                a = int(28 * (1 - k / 7))
                gd.ellipse((cx - rr, cy - rr, cx + rr, cy + rr), fill=base + (a,))
    return Image.alpha_composite(glow, img)


# --- the pieces ------------------------------------------------------------


def legs(sc, w, d, h, t, mat):
    for sx, sz in ((-1, -1), (1, -1), (-1, 1), (1, 1)):
        sc.box(t, h, t, sx * (w / 2 - t), h / 2, sz * (d / 2 - t), mat)


def urn(body, dark, mark, lit=6):
    sc = Scene()
    sc.box(0.44, 0.1, 0.44, 0, 0.05, 0, dark)
    sc.box(0.62, 0.38, 0.62, 0, 0.29, 0, body)
    sc.box(0.48, 0.14, 0.48, 0, 0.55, 0, body)
    sc.box(0.32, 0.16, 0.32, 0, 0.7, 0, dark)
    sc.box(0.44, 0.07, 0.44, 0, 0.81, 0, body)
    sc.box(0.24, 0.02, 0.24, 0, 0.85, 0, "groove")
    for i in range(5):
        x = -0.24 + i * 0.12
        sc.box(0.06, 0.06, 0.02, x, 0.32, 0.315, mark if i < lit else "groove")
        sc.box(0.02, 0.06, 0.06, 0.315, 0.32, x, mark if i + 5 < lit else "groove")
    return sc


def camp_mat():
    sc = Scene()
    sc.box(1.3, 0.03, 0.95, 0, 0.015, 0, "reed")
    sc.box(1.3, 0.04, 0.05, 0, 0.02, 0.45, "cord").box(1.3, 0.04, 0.05, 0, 0.02, -0.45, "cord")
    sc.box(0.3, 0.13, 0.28, 0.38, 0.07, 0.24, "stone").box(0.22, 0.03, 0.2, 0.38, 0.15, 0.24, "stone_dark")
    sc.box(0.18, 0.06, 0.18, 0.3, 0.06, -0.18, "cord")
    sc.box(0.36, 0.05, 0.06, -0.25, 0.05, 0.1, "wood").box(0.1, 0.07, 0.1, -0.05, 0.06, 0.1, "stone")
    return sc


def bedroll(frame=False, double=False):
    sc = Scene()
    sc.box(0.9, 0.12, 1.9, 0, 0.06 + (0.18 if frame else 0), 0, "cloth")
    sc.box(0.72, 0.14, 0.36, 0, 0.14 + (0.18 if frame else 0), -0.72, "linen")
    if frame:
        w = 2.0 if double else 1.04
        off = (w - 1.04) / 2
        sc.box(w, 0.16, 2.0, off, 0.08, 0, "plank")
        sc.box(w, 0.1, 0.08, off, 0.2, 1.0, "wood").box(w, 0.42, 0.08, off, 0.3, -1.0, "wood_dark")
        for x, z in ((-0.52, -1.0), (w - 0.52, -1.0), (-0.52, 1.0), (w - 0.52, 1.0)):
            sc.box(0.1, 0.36, 0.1, x, 0.18, z, "wood_dark")
        if double:
            sc.box(0.9, 0.12, 1.9, 0.98, 0.24, 0, "cloth").box(0.9, 0.06, 1.0, 0.98, 0.33, 0.35, "fur")
            sc.box(0.72, 0.14, 0.36, 0.98, 0.32, -0.72, "linen")
    return sc


def storage(shelf=False):
    sc = Scene()
    sc.box(1.0, 0.45, 0.6, 0, 0.225, 0, "wood").box(1.04, 0.12, 0.64, 0, 0.5, 0, "wood_dark")
    sc.box(0.12, 0.1, 0.04, 0, 0.4, 0.32, "brass")
    for sx in (-1, 1):
        sc.box(0.06, 0.46, 0.62, sx * 0.48, 0.24, 0, "brass")
    if shelf:
        sc.box(0.06, 0.5, 0.36, -0.47, 0.81, -0.12, "wood_dark").box(0.06, 0.5, 0.36, 0.47, 0.81, -0.12, "wood_dark")
        sc.box(1.0, 0.05, 0.36, 0, 0.8, -0.12, "plank").box(1.0, 0.05, 0.36, 0, 1.04, -0.12, "plank")
        sc.box(0.2, 0.18, 0.2, -0.25, 0.92, -0.12, "clay").box(0.16, 0.12, 0.24, 0.2, 0.89, -0.12, "linen")
    return sc


def bedroll_item():
    sc = Scene()
    sc.box(0.9, 0.3, 0.3, 0, 0.15, 0, "cloth", turn=0)
    sc.box(0.06, 0.32, 0.32, -0.2, 0.15, 0, "cord").box(0.06, 0.32, 0.32, 0.2, 0.15, 0, "cord")
    return sc


def tent():
    sc = Scene()
    sc.box(0.06, 0.06, 1.5, 0, 1.0, 0, "wood_dark")
    for z in (-0.72, 0.72):
        sc.box(0.06, 1.0, 0.06, 0, 0.5, z, "wood")
    sc.box(0.86, 0.04, 1.4, -0.34, 0.52, 0, "hide", tilt=0.95)
    sc.box(0.86, 0.04, 1.4, 0.34, 0.52, 0, "hide", tilt=-0.95)
    sc.box(0.04, 0.3, 0.04, 0.6, 0.12, 0.72, "cord", tilt=0.6)
    return sc


def tool_bench(improved=False):
    sc = Scene()
    legs(sc, 1.06, 0.7, 0.72, 0.1, "wood_dark")
    sc.box(1.2, 0.1, 0.8, 0, 0.77, 0, "plank")
    sc.box(0.62, 0.012, 0.14, -0.15, 0.83, -0.12, "groove").box(0.26, 0.012, 0.26, 0.32, 0.83, 0.12, "groove")
    sc.box(0.22, 0.14, 0.18, 0.42, 0.89, -0.24, "stone")
    if improved:
        sc.box(0.08, 0.6, 0.08, -0.55, 1.12, -0.36, "wood_dark").box(0.08, 0.6, 0.08, 0.55, 1.12, -0.36, "wood_dark")
        sc.box(1.18, 0.08, 0.06, 0, 1.32, -0.36, "wood")
        sc.box(0.05, 0.5, 0.05, -0.3, 1.05, -0.3, "wood").box(0.18, 0.1, 0.08, -0.3, 1.27, -0.3, "stone")
        sc.box(0.05, 0.5, 0.05, 0.05, 1.05, -0.3, "wood").box(0.06, 0.16, 0.18, 0.05, 1.22, -0.3, "stone_dark")
        sc.box(0.16, 0.16, 0.2, -0.5, 0.9, 0.28, "stone_dark")
    return sc


def carpentry():
    sc = Scene()
    legs(sc, 1.52, 0.76, 0.72, 0.12, "wood_dark")
    sc.box(1.7, 0.12, 0.9, 0, 0.78, 0, "plank")
    sc.box(0.2, 0.2, 0.3, 0.72, 0.94, 0, "wood_dark").box(0.04, 0.03, 0.32, 0.72, 1.05, 0, "groove")
    sc.box(0.5, 0.012, 0.11, 0.15, 0.85, -0.3, "steel").box(0.16, 0.06, 0.09, -0.16, 0.87, -0.3, "wood_dark")
    sc.box(0.7, 0.14, 0.2, -0.3, 0.91, 0.15, "wood")
    return sc


def sharpening():
    sc = Scene()
    legs(sc, 0.8, 0.5, 0.45, 0.08, "wood_dark")
    sc.box(0.8, 0.06, 0.5, 0, 0.48, 0, "plank")
    sc.box(0.14, 0.56, 0.56, 0, 0.8, 0, "stone").box(0.16, 0.36, 0.36, 0, 0.8, 0, "stone_dark")
    sc.box(0.5, 0.05, 0.05, 0, 0.8, 0, "iron").box(0.05, 0.22, 0.05, 0.27, 0.7, 0, "iron").box(0.12, 0.05, 0.05, 0.3, 0.6, 0, "wood")
    sc.box(0.3, 0.1, 0.3, -0.25, 0.56, 0, "clay")
    return sc


def anvil(metal="iron", runes=False):
    sc = Scene()
    sc.box(0.5, 0.42, 0.46, 0, 0.21, 0, "wood_dark")
    sc.box(0.34, 0.1, 0.26, 0, 0.47, 0, metal).box(0.2, 0.14, 0.16, 0, 0.59, 0, metal)
    sc.box(0.56, 0.12, 0.24, 0, 0.72, 0, metal).box(0.16, 0.08, 0.12, 0.35, 0.72, 0, metal)
    sc.box(0.24, 0.05, 0.05, -0.12, 0.81, 0.08, "wood_dark").box(0.08, 0.08, 0.1, -0.24, 0.81, 0.08, "iron")
    if runes:
        for x in (-0.18, 0.0, 0.18):
            sc.box(0.05, 0.05, 0.02, x, 0.72, 0.125, "spirit")
        sc.box(0.1, 0.1, 0.1, 0.0, 0.9, -0.15, "spirit")
    return sc


def cooking_pot():
    sc = Scene()
    for i in range(8):
        a = i / 8 * math.tau
        sc.box(0.14, 0.1, 0.14, math.cos(a) * 0.32, 0.05, math.sin(a) * 0.32, "stone")
    sc.box(0.24, 0.08, 0.24, 0, 0.04, 0, "wood_dark").box(0.14, 0.18, 0.14, 0, 0.13, 0, "fire")
    sc.box(0.44, 0.32, 0.44, 0, 0.46, 0, "clay").box(0.34, 0.04, 0.34, 0, 0.63, 0, "groove")
    sc.box(0.03, 0.4, 0.03, 0, 0.86, 0, "cord").box(0.9, 0.05, 0.05, 0, 1.06, 0, "wood_dark")
    for sx in (-1, 1):
        sc.box(0.06, 1.1, 0.06, sx * 0.38, 0.52, 0, "wood", tilt=sx * 0.12)
    return sc


def cutting_board():
    sc = Scene()
    sc.box(0.6, 0.44, 0.44, 0, 0.22, 0, "wood_dark").box(0.7, 0.06, 0.46, 0, 0.47, 0, "plank")
    sc.box(0.22, 0.02, 0.06, 0.12, 0.51, 0.12, "steel").box(0.12, 0.03, 0.05, -0.04, 0.515, 0.12, "wood_dark")
    sc.box(0.14, 0.06, 0.1, -0.18, 0.53, -0.08, "hide").box(0.08, 0.06, 0.08, -0.05, 0.53, -0.1, "brew")
    return sc


def smoking_rack():
    sc = Scene()
    sc.box(0.07, 1.25, 0.07, -0.56, 0.62, 0, "wood").box(0.07, 1.25, 0.07, 0.56, 0.62, 0, "wood")
    sc.box(1.2, 0.05, 0.05, 0, 1.2, 0, "wood_dark").box(1.2, 0.05, 0.05, 0, 0.9, 0, "wood_dark")
    sc.box(0.5, 0.12, 0.4, 0, 0.06, 0, "stone_dark").box(0.36, 0.06, 0.28, 0, 0.14, 0, "ember")
    for x in (-0.36, -0.12, 0.12, 0.36):
        sc.box(0.1, 0.22, 0.04, x, 1.03, 0, "fish").box(0.1, 0.18, 0.04, x + 0.06, 0.75, 0, "hide")
    return sc


def herb_rack():
    sc = cutting_board()
    sc.box(0.05, 0.6, 0.05, -0.32, 0.8, -0.2, "wood").box(0.05, 0.6, 0.05, 0.32, 0.8, -0.2, "wood")
    sc.box(0.7, 0.04, 0.04, 0, 1.08, -0.2, "wood_dark")
    for x in (-0.2, 0.0, 0.2):
        sc.box(0.08, 0.2, 0.06, x, 0.95, -0.2, "moss")
    return sc


def fridge():
    sc = Scene()
    sc.box(0.9, 0.86, 0.7, 0, 0.43, 0, "stone").box(0.7, 0.62, 0.04, 0, 0.43, 0.36, "stone_dark")
    sc.box(0.06, 0.18, 0.04, 0.26, 0.45, 0.39, "iron").box(0.94, 0.08, 0.74, 0, 0.9, 0, "moss")
    sc.box(0.2, 0.06, 0.2, -0.25, 0.97, 0.1, "crystal")
    return sc


def telescope(long=0.85, lens="glass"):
    sc = Scene()
    for sx in (-1, 1):
        sc.box(0.05, 0.9, 0.05, sx * 0.18, 0.43, 0, "wood_dark", tilt=sx * 0.22)
    sc.box(0.05, 0.9, 0.05, 0, 0.43, -0.16, "wood_dark")
    sc.box(long, 0.14, 0.14, 0.05, 0.98, 0, "brass", tilt=0.7)
    tip = (0.05 + long / 2 * math.cos(0.7), 0.98 + long / 2 * math.sin(0.7), 0)
    sc.box(0.04, 0.17, 0.17, tip[0], tip[1], tip[2], lens, tilt=0.7)
    sc.box(0.1, 0.1, 0.1, -0.22, 0.78, 0, "brass", tilt=0.7)
    return sc


def magical_lens():
    sc = Scene()
    sc.box(0.36, 0.08, 0.3, 0, 0.04, 0, "stone_dark").box(0.06, 0.6, 0.06, 0, 0.38, 0, "brass")
    sc.box(0.5, 0.06, 0.06, 0, 0.7, 0, "brass").box(0.5, 0.06, 0.06, 0, 1.16, 0, "brass")
    sc.box(0.06, 0.46, 0.06, -0.22, 0.93, 0, "brass").box(0.06, 0.46, 0.06, 0.22, 0.93, 0, "brass")
    sc.box(0.38, 0.4, 0.02, 0, 0.93, 0, "crystal")
    return sc


def cauldron(steel=False):
    sc = Scene()
    m, s = ("steel", 1.15) if steel else ("iron", 1.0)
    for x, z in ((-0.22, -0.2), (0.22, -0.2), (0.0, 0.24)):
        sc.box(0.07, 0.2, 0.07, x * s, 0.1, z * s, m)
    sc.box(0.62 * s, 0.42 * s, 0.62 * s, 0, 0.2 + 0.21 * s, 0, m).box(0.7 * s, 0.06, 0.7 * s, 0, 0.2 + 0.42 * s, 0, m)
    sc.box(0.5 * s, 0.03, 0.5 * s, 0, 0.2 + 0.42 * s, 0, "brew").box(0.2, 0.08, 0.2, 0, 0.04, 0, "fire")
    return sc


def golem(heart=False, hands=False):
    sc = Scene()
    sc.box(0.07, 1.7, 0.07, -0.55, 0.85, -0.3, "wood").box(0.07, 1.7, 0.07, 0.55, 0.85, -0.3, "wood")
    sc.box(1.2, 0.06, 0.06, 0, 1.66, -0.3, "wood_dark")
    sc.box(0.5, 0.3, 0.36, 0, 0.15, 0, "stone_dark").box(0.6, 0.6, 0.4, 0, 0.62, 0, "stone")
    sc.box(0.36, 0.3, 0.32, 0, 1.1, 0, "stone")
    sc.box(0.18, 0.5, 0.2, -0.42, 0.62, 0, "stone").box(0.18, 0.5, 0.2, 0.42, 0.62, 0, "stone")
    core = "spirit" if heart else "groove"
    sc.box(0.14, 0.14, 0.02, 0, 0.7, 0.205, core).box(0.06, 0.05, 0.02, -0.08, 1.12, 0.165, core).box(0.06, 0.05, 0.02, 0.08, 1.12, 0.165, core)
    if hands:
        sc.box(0.22, 0.16, 0.24, -0.42, 0.3, 0, "iron").box(0.22, 0.16, 0.24, 0.42, 0.3, 0, "iron")
    if not heart:
        sc.box(0.03, 0.3, 0.03, -0.3, 1.5, -0.1, "cord").box(0.03, 0.3, 0.03, 0.3, 1.5, -0.1, "cord")
    return sc


STATIONS = [
    ("sleeping", "Sleeping Station", "The Ancestral Urn stands by your bedroll and moves with it.", [
        ("Ancestral Urn", "Root of every station", urn("clay", "wood_dark", "gold")),
        ("Bedroll", "Your bed from the start", bedroll()),
        ("Camp Mat", "Station: first axe, bedroll, tent", camp_mat()),
        ("Bedframe", "Upgrade: off the wet ground", bedroll(frame=True)),
        ("Double-wide Bed", "Upgrade: fur, room for two", bedroll(frame=True, double=True)),
        ("Personal Storage", "Station: a trunk that packs up", storage()),
        ("Extra Shelf", "Upgrade: another row", storage(shelf=True)),
        ("Portable Bedroll", "Made on the camp mat", bedroll_item()),
        ("Portable Tent", "Made on the camp mat", tent()),
    ]),
    ("tool", "Tool Station", "The Tool Urn: grey stone, set down anywhere.", [
        ("Tool Urn", "Given by the ancestral urn", urn("stone", "stone_dark", "linen")),
        ("Tool Bench", "Station: rock pick, shovel", tool_bench()),
        ("Improved Tool Bench", "Upgrade: stone axe, spear", tool_bench(improved=True)),
        ("Carpentry Bench", "Station: planks, sticks, chests", carpentry()),
        ("Sharpening Stone", "Station: hone an edge", sharpening()),
        ("Iron Anvil", "Station: iron tools", anvil()),
        ("Steel Anvil", "Upgrade: steel", anvil("steel")),
        ("Alchemical Anvil", "Upgrade: spirit marks", anvil("steel", runes=True)),
    ]),
    ("cooking", "Cooking Station", "The Hearth Urn: red clay, warm marks.", [
        ("Hearth Urn", "Given by the ancestral urn", urn("clay", "groove", "fire")),
        ("Cooking Pot", "Station: stews and roasts", cooking_pot()),
        ("Cutting Board", "Station: better portions", cutting_board()),
        ("Smoking Rack", "Station: food that keeps", smoking_rack()),
        ("Herb Rack", "Upgrade: a food tier up", herb_rack()),
        ("Stone Fridge", "Station: keeps food fresh", fridge()),
    ]),
    ("astronomy", "Astronomy Station", "The Star Urn: deep blue, gold marks.", [
        ("Star Urn", "For a coin from a barrow", urn("star", "star_dark", "gold")),
        ("Telescope", "Station: find constellations", telescope()),
        ("Improved Telescope", "Upgrade: a longer tube", telescope(long=1.2)),
        ("Diamond Lens", "Upgrade: hidden stars", telescope(long=1.2, lens="crystal")),
        ("Magical Lens", "Station: shows the flux", magical_lens()),
        ("Cauldron", "Station: brews potions", cauldron()),
        ("Steel Cauldron", "Upgrade: stronger brews", cauldron(steel=True)),
    ]),
    ("golem", "Golem Station", "The Golem Urn: moss-grey stone. A draft: what golems do is still open.", [
        ("Golem Urn", "Last gift of the ancestral urn", urn("stone_dark", "moss", "spirit")),
        ("Golem Frame", "Station: a body on a scaffold", golem()),
        ("Golem Heart", "Upgrade: it wakes", golem(heart=True)),
        ("Working Hands", "Upgrade: carries and chops", golem(heart=True, hands=True)),
    ]),
]

BG = hexc("1c2018")
PANEL = hexc("2a3024")
INK = hexc("f2eedc")
MUTED = hexc("a8ac98")
GOLD = hexc("f8d47e")


def sheet(key, title, note, pieces):
    cols = 4
    rows = (len(pieces) + cols - 1) // cols
    pad, head = 14, 70
    w = pad + cols * (CELL_W + pad)
    h = head + rows * (CELL_H + 46 + pad) + pad
    img = Image.new("RGBA", (w, h), BG + (255,))
    d = ImageDraw.Draw(img)
    big = ImageFont.truetype(FONT_BOLD, 36)
    body = ImageFont.truetype(FONT, 18)
    d.text((pad, 10), title, font=big, fill=GOLD)
    d.text((pad, 46), note, font=body, fill=MUTED)
    for i, (name, line, sc) in enumerate(pieces):
        cx = pad + (i % cols) * (CELL_W + pad)
        cy = head + (i // cols) * (CELL_H + 46 + pad)
        d.rectangle((cx, cy, cx + CELL_W, cy + CELL_H + 46), fill=PANEL + (255,))
        # Fit: find the drawing's size, then centre it.
        # The largest scale (pixels per block) at which it fits the cell.
        global U
        art, bb = None, None
        for u in (110, 96, 84, 72, 62, 54, 46, 40, 34):
            U = u
            a = draw_scene(sc, CELL_W * 3, CELL_H * 3, CELL_W * 1.5, CELL_H * 2)
            bb = a.getbbox()
            if bb and bb[2] - bb[0] <= CELL_W - 24 and bb[3] - bb[1] <= CELL_H - 30:
                art = a
                break
        if bb and art is None:
            art = a
        if bb:
            art = art.crop(bb)
            # a soft ground shadow
            sh = Image.new("RGBA", (art.width, 14), (0, 0, 0, 0))
            ImageDraw.Draw(sh).ellipse((0, 0, art.width, 14), fill=(0, 0, 0, 70))
            gx = cx + (CELL_W - art.width) // 2
            gy = cy + CELL_H - 12 - art.height
            img.alpha_composite(sh, (gx, gy + art.height - 9))
            img.alpha_composite(art, (gx, gy))
        d.text((cx + 10, cy + CELL_H + 2), name, font=ImageFont.truetype(FONT_BOLD, 22), fill=INK)
        d.text((cx + 10, cy + CELL_H + 24), line, font=body, fill=MUTED)
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, key + ".png")
    img.convert("RGB").save(path)
    print(path)


if __name__ == "__main__":
    for key, title, note, pieces in STATIONS:
        sheet(key, title, note, pieces)
