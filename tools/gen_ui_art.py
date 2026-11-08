#!/usr/bin/env python3
"""Generate the placeholder UI sprite sheet.

Writes assets/textures/ui.png (256 x 128) and assets/textures/ui.ron
(sprite name -> rectangle in the sheet, plus where to cut it for stretching).

Paint over ui.png in any pixel editor (Aseprite, Pixelorama, Photoshop with
nearest-neighbour). The game draws every UI pixel at 2x. Keep each sprite
inside its rectangle, or move it and change ui.ron to match.

How the frames stretch ("9-slice"): `slice: (left, top, right, bottom)` are
the borders in pixels. The four corners are drawn as they are, the edges
stretch along their length, and the middle stretches to fill. So a 24 x 24
panel with slice 8 can become a window of any size. Sprites without `slice`
(icons, skill nodes) are drawn as they are.

Grey/white sprites (skill nodes, `fill`, `white`) are tinted by the game,
so paint them in greys.

Run from the repo root:  python3 tools/gen_ui_art.py
Warning: this overwrites ui.png. Keep your painted version somewhere else
first, or only run it when you want the placeholder back.
"""
import os
from PIL import Image

W, H = 256, 128
img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
px = img.load()
sprites = []  # (name, x, y, w, h, slice or None)


def hexc(h, a=255):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), a)


def put(x, y, c):
    if 0 <= x < W and 0 <= y < H:
        px[x, y] = c


def rect(x, y, w, h, c):
    for j in range(y, y + h):
        for i in range(x, x + w):
            put(i, j, c)


def frame(x, y, w, h, outline, light, dark, face, inner=None, width=1):
    """A bevelled box: outline, `width` px of light (top/left) and dark
    (bottom/right) bevel, optional 1 px inner line, then the face."""
    rect(x, y, w, h, outline)
    for k in range(width):
        o = 1 + k
        rect(x + o, y + o, w - 2 * o, 1, light)            # top
        rect(x + o, y + o, 1, h - 2 * o, light)            # left
        rect(x + o, y + h - 1 - o, w - 2 * o, 1, dark)     # bottom
        rect(x + w - 1 - o, y + o, 1, h - 2 * o, dark)     # right
    o = 1 + width
    if inner is not None:
        rect(x + o, y + o, w - 2 * o, h - 2 * o, inner)
        o += 1
    rect(x + o, y + o, w - 2 * o, h - 2 * o, face)


def add(name, x, y, w, h, sl=None):
    if isinstance(sl, int):
        sl = (sl, sl, sl, sl)
    sprites.append((name, x, y, w, h, sl))


# Palette: swamp wood, moss and brass.
INK = hexc("0b0d0a")
WOOD_HI, WOOD, WOOD_LO = hexc("9a7646"), hexc("6e5130"), hexc("3d2a16")
MOSS_HI, MOSS, MOSS_LO = hexc("86a36c"), hexc("55724a"), hexc("2d3f28")
BRASS, BRASS_HI, BRASS_LO = hexc("d6ad55"), hexc("f4dc94"), hexc("8a6424")
PANEL_FACE = hexc("141b17", 246)
GOLD = hexc("e8bd52")

# --- Frames (row 0) -------------------------------------------------------

# panel: the window frame (wood with brass corner studs).
x, y = 0, 0
frame(x, y, 24, 24, INK, WOOD_HI, WOOD_LO, PANEL_FACE, inner=INK, width=3)
for i in range(4, 20, 3):  # wood grain on the top and left planks
    put(x + i, y + 2, WOOD)
    put(x + 2, y + i, WOOD)
for cx, cy in [(1, 1), (20, 1), (1, 20), (20, 20)]:
    rect(x + cx, y + cy, 3, 3, BRASS)
    put(x + cx, y + cy, BRASS_HI)
    put(x + cx + 2, y + cy + 2, BRASS_LO)
add("panel", x, y, 24, 24, 8)

# panel_light: a thinner frame for the HUD clusters.
x = 24
frame(x, y, 16, 16, INK, hexc("3f5244"), hexc("1a241d"), hexc("101612", 215))
add("panel_light", x, y, 16, 16, 4)

# inset: a sunken area inside a panel.
x = 40
frame(x, y, 12, 12, hexc("0b0d0a", 0), hexc("070908"), hexc("34413a"), hexc("0b100d", 225))
add("inset", x, y, 12, 12, 3)

# header: a wooden title strip.
x = 52
frame(x, y, 16, 14, INK, WOOD_HI, WOOD_LO, WOOD)
for i in range(3, 13, 3):
    put(x + i, y + 6, hexc("5d4327"))
    put(x + i + 1, y + 8, hexc("5d4327"))
add("header", x, y, 16, 14, 4)

# tooltip: dark parchment with a brass rule.
x = 68
frame(x, y, 16, 16, INK, BRASS, BRASS_LO, hexc("1b1812", 248))
add("tooltip", x, y, 16, 16, 4)

# banner: the place title card, a dark ribbon with brass edges and notched ends.
x = 84
bw, bh = 40, 20
rect(x, y + 2, bw, bh - 4, hexc("0e120f", 242))
rect(x, y + 2, bw, 1, BRASS)
rect(x, y + bh - 3, bw, 1, BRASS)
rect(x, y + 4, bw, 1, BRASS_LO)
rect(x, y + bh - 5, bw, 1, BRASS_LO)
for j in range(bh):
    d = abs(j - (bh - 1) / 2)
    cut = max(0, 6 - int(d))  # V-shaped notch depth at the ends
    for k in range(cut):
        put(x + k, y + j, (0, 0, 0, 0))
        put(x + bw - 1 - k, y + j, (0, 0, 0, 0))
add("banner", x, y, bw, bh, (12, 6, 12, 6))

# divider: a thin brass rule with studs at the ends.
x = 124
rect(x, y + 1, 32, 1, BRASS)
rect(x, y + 2, 32, 1, BRASS_LO)
rect(x, y, 2, 4, BRASS_HI)
rect(x + 30, y, 2, 4, BRASS_HI)
add("divider", x, y, 32, 4, (4, 1, 4, 1))

# white: a plain tintable pixel block (for fills and lines).
x = 156
rect(x, y, 4, 4, hexc("ffffff"))
add("white", x, y, 4, 4, 1)

# --- Slots and buttons (row 1, y = 24) -------------------------------------
y = 24
SLOT_FACE = hexc("1a221d", 225)
frame(0, y, 16, 16, INK, hexc("070908"), hexc("3d4a40"), SLOT_FACE)
add("slot", 0, y, 16, 16, 4)
frame(16, y, 16, 16, INK, hexc("101410"), hexc("8aa58e"), hexc("27342b", 235))
add("slot_hover", 16, y, 16, 16, 4)
frame(32, y, 16, 16, INK, GOLD, BRASS_LO, hexc("2b2919", 235), width=2)
put(34, y + 2, BRASS_HI)
add("slot_selected", 32, y, 16, 16, 5)

# item: the raised tile an item sits on in a grid (tinted by the item's colour).
frame(48, y, 16, 16, INK, hexc("d8d8d8"), hexc("7a7a7a"), hexc("a8a8a8"))
add("item", 48, y, 16, 16, 4)

# buttons: moss-green planks.
frame(64, y, 16, 16, INK, MOSS_HI, MOSS_LO, MOSS)
add("button", 64, y, 16, 16, 4)
frame(80, y, 16, 16, INK, hexc("a9c48e"), hexc("3a5233"), hexc("6b8a5c"))
add("button_hover", 80, y, 16, 16, 4)
frame(96, y, 16, 16, INK, MOSS_LO, MOSS_HI, hexc("43603b"))
add("button_down", 96, y, 16, 16, 4)
frame(112, y, 16, 16, INK, hexc("4a4f48"), hexc("252824"), hexc("33372f"))
add("button_off", 112, y, 16, 16, 4)

# bar: the frame round a meter. fill: what goes in it (tinted).
frame(128, y, 12, 8, INK, hexc("050605"), hexc("2c352f"), hexc("070908", 235))
add("bar", 128, y, 12, 8, 2)
rect(140, y, 6, 6, hexc("ffffff"))
rect(140, y, 6, 1, hexc("ffffff"))
rect(140, y + 4, 6, 2, hexc("c8c8c8"))  # darker lower edge gives the fill some depth
add("fill", 140, y, 6, 6, (1, 1, 1, 2))

# knob: a settings slider handle.
frame(148, y, 8, 12, INK, BRASS_HI, BRASS_LO, BRASS)
add("knob", 148, y, 8, 12)

# --- Skill tree nodes (row 2, y = 44), grey so the game can tint them ------
y = 44


def disc(cx, cy, r, c):
    for j in range(int(cy - r - 1), int(cy + r + 2)):
        for i in range(int(cx - r - 1), int(cx + r + 2)):
            if (i + 0.5 - cx) ** 2 + (j + 0.5 - cy) ** 2 <= r * r:
                put(i, j, c)


# major: a ringed stone.
disc(12, y + 12, 12, INK)
disc(12, y + 12, 11, hexc("e6e6e6"))
disc(12, y + 12, 9, hexc("6e6e6e"))
disc(12, y + 12, 8, hexc("bdbdbd"))
disc(11, y + 11, 5, hexc("d9d9d9"))
disc(10, y + 10, 2, hexc("ffffff"))
add("node_major", 0, y, 24, 24)

# minor: a small stone.
disc(32, y + 8, 8, INK)
disc(32, y + 8, 7, hexc("d0d0d0"))
disc(32, y + 8, 5, hexc("a8a8a8"))
disc(31, y + 7, 2, hexc("efefef"))
add("node_minor", 24, y, 16, 16)

# halo: drawn behind a node you can take now.
for j in range(28):
    for i in range(28):
        d = ((i + 0.5 - 14) ** 2 + (j + 0.5 - 14) ** 2) ** 0.5
        if 11 <= d <= 14:
            a = int(255 * (1 - abs(d - 12.5) / 1.5))
            put(40 + i, y + j, (255, 255, 255, max(0, a)))
add("node_halo", 40, y, 28, 28)

# crosshair: white with a dark edge so it shows on snow and sky alike.
cx0 = 68
cross = [(k, 5) for k in range(1, 10)] + [(5, k) for k in range(1, 10)]
for (i, j) in cross:
    for di in (-1, 0, 1):
        for dj in (-1, 0, 1):
            put(cx0 + i + di, y + j + dj, hexc("0b0d0a", 200))
for (i, j) in cross:
    put(cx0 + i, y + j, hexc("ffffff"))
add("crosshair", cx0, y, 11, 11)

# --- Icons (row 3, y = 76): 12 x 12, one letter per colour -----------------
ICON_PAL = {
    ".": None,
    "k": INK,
    "w": hexc("ffffff"),
    "r": hexc("d8332f"), "R": hexc("8c1c1c"), "p": hexc("ff8a80"),
    "y": hexc("f2cf4a"), "Y": hexc("a8781c"), "o": hexc("ef8a2e"),
    "b": hexc("5ab0e8"), "B": hexc("2a6aa8"), "c": hexc("bfe6ff"),
    "g": hexc("74c85c"), "G": hexc("3c7a32"),
    "t": hexc("e9d8b0"), "T": hexc("a8926a"),
    "v": hexc("a873e0"), "V": hexc("5e3a8c"),
    "s": hexc("9aa0a6"), "S": hexc("5a6066"),
    "n": hexc("b5763e"), "N": hexc("6e4520"),
}

ICONS = {
    "damage": [  # a heart
        "............",
        ".kkk...kkk..",
        "krrrk.krrrk.",
        "krprrkrrrrk.",
        "krprrrrrrrk.",
        "krrrrrrrrrk.",
        ".krrrrrrrk..",
        "..krrrrrk...",
        "...krrrk....",
        "....krk.....",
        ".....k......",
        "............",
    ],
    "fatigue": [  # a lightning bolt
        "......kkkk..",
        ".....kyyyk..",
        "....kyyyk...",
        "...kyyyk....",
        "..kyyyykkk..",
        "..kkkyyyyk..",
        "....kyyyk...",
        "...kyyyk....",
        "..kyyk......",
        "..kyk.......",
        "..kk........",
        "............",
    ],
    "blood": [  # a drop
        ".....k......",
        "....krk.....",
        "....krk.....",
        "...krrrk....",
        "...krrrk....",
        "..krprrrk...",
        "..krprrrk...",
        "..krrrrrk...",
        "..kRrrrRk...",
        "...kRRRk....",
        "....kkk.....",
        "............",
    ],
    "injuries": [  # a bandage cross
        "....kkkk....",
        "....kttk....",
        "....kTtk....",
        "kkkkkttkkkk.",
        "kttttvvttttk",
        "kTTTtvvtTTTk",
        "kkkkkttkkkk.",
        "....kttk....",
        "....kTtk....",
        "....kttk....",
        "....kkkk....",
        "............",
    ],
    "food": [  # a drumstick
        "......kkkk..",
        ".....knnnnk.",
        "....knnnnnnk",
        "....knnNnnnk",
        "....knnnnnNk",
        ".....knnnNk.",
        "....kttkkk..",
        "...ktk......",
        ".kkttk......",
        "ktttkk......",
        "kttk........",
        ".kk.........",
    ],
    "air": [  # bubbles
        ".......kkk..",
        "......kcwck.",
        "......kcbck.",
        ".......kkk..",
        "..kkkk......",
        ".kcwbbk.....",
        "kcwbbbbk.kk.",
        "kcbbbbbkkck.",
        "kbbbbbBk.kk.",
        ".kbbbBk.....",
        "..kkkk......",
        "............",
    ],
    "warmth": [  # a flame
        ".....k......",
        "....kok.....",
        "....kok..k..",
        "...kook.kok.",
        "..kooyok.ok.",
        "..koyyyook..",
        ".kooyyyyok..",
        ".koyywyyok..",
        ".koyywwyok..",
        "..koyyyok...",
        "...kkkkk....",
        "............",
    ],
    "load": [  # a weight
        "....kkkk....",
        "...kk..kk...",
        "...k....k...",
        "..kkkkkkkk..",
        ".kssssssssk.",
        ".ksSssssssk.",
        "kssSsssssssk",
        "kssssssssssk",
        "ksssssssssSk",
        "kSssssssSSSk",
        ".kkkkkkkkkk.",
        "............",
    ],
    "focus": [  # a four-point star
        ".....k......",
        "....kvk.....",
        "....kvk.....",
        "...kvwvk....",
        ".kkvvwvvkk..",
        "kvvwwwwwvvk.",
        ".kkvvwvvkk..",
        "...kvwvk....",
        "....kvk.....",
        "....kVk.....",
        ".....k......",
        "............",
    ],
    "xp": [  # a gem
        "..kkkkkkk...",
        ".kgwgggGGk..",
        "kgwggggGGGk.",
        "kkkkkkkkkkk.",
        ".kggggggGk..",
        "..kgggggk...",
        "...kgggk....",
        "....kGk.....",
        ".....k......",
        "............",
        "............",
        "............",
    ],
    "levitate": [  # a feather
        "........kkk.",
        ".......kcwk.",
        "......kcwck.",
        ".....kcwcck.",
        "....kcwcck..",
        "...kcwcck...",
        "..kcwcck....",
        "..kwcck.....",
        ".kkkkk......",
        ".kk.........",
        "k...........",
        "............",
    ],
    "light": [  # a sun
        ".....k......",
        "..k..y..k...",
        "...kkkkk....",
        "..kyyyyyk...",
        "kykyywyyky.k",
        "..kyywyyk...",
        "..kyyyyyk...",
        "...kkkkk....",
        "..k..y..k...",
        ".....k......",
        "............",
        "............",
    ],
    "lock": [
        "....kkkk....",
        "...kk..kk...",
        "...k....k...",
        "...k....k...",
        "..kkkkkkkk..",
        "..kyyyyyyk..",
        "..kyYkkYyk..",
        "..kyyYYyyk..",
        "..kyyYYyyk..",
        "..kYYYYYYk..",
        "..kkkkkkkk..",
        "............",
    ],
    "check": [
        "............",
        "..........k.",
        ".........kgk",
        "........kgk.",
        ".k.....kgk..",
        "kgk...kgk...",
        ".kgk.kgk....",
        "..kgkgk.....",
        "...kgk......",
        "....k.......",
        "............",
        "............",
    ],
    "close": [
        "............",
        ".kk......kk.",
        "kwwk....kwwk",
        ".kwwk..kwwk.",
        "..kwwkkwwk..",
        "...kwwwwk...",
        "...kwwwwk...",
        "..kwwkkwwk..",
        ".kwwk..kwwk.",
        "kwwk....kwwk",
        ".kk......kk.",
        "............",
    ],
    "camp": [  # a bedroll and fire
        "............",
        "......k.....",
        ".....kok....",
        ".....kyok...",
        "....kkkkk...",
        "...kNkkNk...",
        "............",
        "kkkkkkkkkkkk",
        "kGgggggggggk",
        "kgGggggggggk",
        "kkkkkkkkkkkk",
        "............",
    ],
}

y = 76
for i, (name, rows) in enumerate(ICONS.items()):
    ix, iy = (i % 16) * 12, y + (i // 16) * 12
    assert len(rows) == 12 and all(len(r) == 12 for r in rows), name
    for j, row in enumerate(rows):
        for k, ch in enumerate(row):
            c = ICON_PAL[ch]
            if c is not None:
                put(ix + k, iy + j, c)
    add("icon_" + name, ix, iy, 12, 12)

# --- Write -----------------------------------------------------------------
root = os.path.join(os.path.dirname(__file__), "..", "assets", "textures")
img.save(os.path.join(root, "ui.png"))
lines = [
    "// UI sprites in ui.png: name -> (x, y, w, h) in pixels, and where to cut",
    "// a frame for stretching (left, top, right, bottom). Written by",
    "// tools/gen_ui_art.py; edit by hand if you move or resize a sprite.",
    "(",
    "    scale: 2.0,",
    "    sprites: {",
]
for name, x, y, w, h, sl in sprites:
    s = f"slice: Some(({sl[0]}, {sl[1]}, {sl[2]}, {sl[3]}))" if sl else "slice: None"
    lines.append(f'        "{name}": (rect: ({x}, {y}, {w}, {h}), {s}),')
lines += ["    },", ")", ""]
with open(os.path.join(root, "ui.ron"), "w") as f:
    f.write("\n".join(lines))
print(f"wrote ui.png and ui.ron with {len(sprites)} sprites")
