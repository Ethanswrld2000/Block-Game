#!/usr/bin/env python3
"""Generate the placeholder UI sprite sheet.

Writes assets/textures/ui.png (256 x 192) and assets/textures/ui.ron
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

W, H = 256, 192
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


# Palette: dark oak, wrought iron, brass and worn stone. Light falls from the
# top left, so raised things are light there and dark at the bottom right;
# sunken things (slots, meters) the other way round. Every frame has its
# outer corner pixels cut, for the chunky rounded look of pixel-art windows.
INK = hexc("0d0b09")
CLEAR = (0, 0, 0, 0)
OAK_HI, OAK, OAK_MID, OAK_LO, OAK_DEEP = hexc("b58955"), hexc("8a6238"), hexc("74512d"), hexc("4f361d"), hexc("33220f")
IRON_HI, IRON, IRON_LO = hexc("c9ccce"), hexc("8a8f93"), hexc("4b4f53")
STONE_HI, STONE, STONE_LO, STONE_DEEP = hexc("b9b4a8"), hexc("858075"), hexc("5a564e"), hexc("3a3732")
BRASS, BRASS_HI, BRASS_LO = hexc("d9a93f"), hexc("ffe08a"), hexc("8c6218")
PANEL_FACE = hexc("1d1813", 246)
SHADE = hexc("120e0a", 246)
GOLD = hexc("f0c040")


def notch(x, y, w, h, c=CLEAR):
    """Cut the four outer corner pixels."""
    for cx, cy in [(x, y), (x + w - 1, y), (x, y + h - 1), (x + w - 1, y + h - 1)]:
        put(cx, cy, c)


def bevel(x, y, w, h, hi, lo, face, width=2, outline=INK, corner=None, inner=None):
    """A chunky box: an outline with cut corners, `width` px of light
    (top/left) and dark (bottom/right) bevel, an optional 1 px inner line, then
    the face. Where light meets dark (top right, bottom left) the corner takes
    `corner` (the face colour by default), as on carved pixel-art buttons."""
    rect(x, y, w, h, outline)
    notch(x, y, w, h)
    corner = face if corner is None else corner
    for k in range(width):
        o = 1 + k
        rect(x + o, y + o, w - 2 * o, 1, hi)
        rect(x + o, y + o, 1, h - 2 * o, hi)
        rect(x + o, y + h - 1 - o, w - 2 * o, 1, lo)
        rect(x + w - 1 - o, y + o, 1, h - 2 * o, lo)
        put(x + w - 1 - o, y + o, corner)
        put(x + o, y + h - 1 - o, corner)
    o = 1 + width
    if inner is not None:
        rect(x + o, y + o, w - 2 * o, h - 2 * o, inner)
        o += 1
    rect(x + o, y + o, w - 2 * o, h - 2 * o, face)


def rivet(x, y):
    put(x, y, IRON_HI)
    put(x + 1, y, IRON)
    put(x, y + 1, IRON)
    put(x + 1, y + 1, IRON_LO)


def add(name, x, y, w, h, sl=None):
    if isinstance(sl, int):
        sl = (sl, sl, sl, sl)
    sprites.append((name, x, y, w, h, sl))


def oak_frame(x, y, w, h, face, edge=4):
    """A window frame of oak planks (`edge` px wide, grain running along each
    plank so it stretches cleanly), mitred corners, an inked inner edge and a
    shadowed face."""
    rect(x, y, w, h, INK)
    notch(x, y, w, h)
    # Planks: rows from the outside in.
    top = [OAK_HI, OAK, OAK_MID, OAK][:edge]
    bot = [OAK_LO, OAK_MID, OAK_LO, OAK_DEEP][:edge]
    for k in range(edge):
        o = 1 + k
        rect(x + o, y + o, w - 2 * o, 1, top[k])                     # top plank
        rect(x + o, y + o, 1, h - 2 * o, top[k])                     # left plank
        rect(x + o, y + h - 1 - o, w - 2 * o, 1, bot[k])             # bottom plank
        rect(x + w - 1 - o, y + o, 1, h - 2 * o, bot[k])             # right plank
    # Mitre joints: a dark seam on the diagonal of each corner.
    for k in range(edge):
        o = 1 + k
        for cx, cy in [(x + o, y + o), (x + w - 1 - o, y + o), (x + o, y + h - 1 - o), (x + w - 1 - o, y + h - 1 - o)]:
            put(cx, cy, OAK_DEEP)
    o = 1 + edge
    rect(x + o, y + o, w - 2 * o, h - 2 * o, INK)                    # inner ink line
    o += 1
    rect(x + o, y + o, w - 2 * o, h - 2 * o, face)
    rect(x + o, y + o, w - 2 * o, 1, SHADE)                          # the frame's shadow on the face
    rect(x + o, y + o, 1, h - 2 * o, SHADE)


# --- Frames (row 0) -------------------------------------------------------

# panel: the window frame (oak planks, iron corner brackets with rivets).
x, y = 0, 0
oak_frame(x, y, 24, 24, PANEL_FACE)
for cx, cy, sx, sy in [(1, 1, 1, 1), (22, 1, -1, 1), (1, 22, 1, -1), (22, 22, -1, -1)]:
    # an L-shaped iron bracket, 5 px each way, 2 px thick
    for k in range(5):
        for t in range(2):
            put(x + cx + sx * k, y + cy + sy * t, IRON if t == 0 else IRON_LO)
            put(x + cx + sx * t, y + cy + sy * k, IRON if t == 0 else IRON_LO)
    put(x + cx, y + cy, IRON_HI)
    rivet(x + cx + (0 if sx > 0 else -1) + sx * 2, y + cy + (0 if sy > 0 else -1) + sy * 2)
notch(x, y, 24, 24)
add("panel", x, y, 24, 24, 8)

# panel_light: a thinner stone-edged frame for the HUD clusters.
x = 24
bevel(x, y, 16, 16, hexc("6b665b"), hexc("2b2824"), hexc("17130f", 220), width=1, inner=hexc("0d0b09", 230), corner=hexc("4a463f"))
add("panel_light", x, y, 16, 16, 4)

# inset: a sunken well inside a panel (dark top/left, lit bottom/right).
x = 40
bevel(x, y, 12, 12, hexc("080706"), hexc("4a4238"), hexc("100d0a", 228), width=1, outline=hexc("0d0b09", 0), corner=hexc("2a241d"))
add("inset", x, y, 12, 12, 3)

# header: an oak title board with plank seams.
x = 52
bevel(x, y, 16, 14, OAK_HI, OAK_LO, OAK, width=1, corner=OAK_MID)
rect(x + 2, y + 5, 12, 1, OAK_MID)
rect(x + 2, y + 9, 12, 1, OAK_MID)
put(x + 2, y + 2, IRON_HI)
put(x + 13, y + 2, IRON_HI)
put(x + 2, y + 11, IRON)
put(x + 13, y + 11, IRON)
add("header", x, y, 16, 14, 4)

# tooltip: near-black vellum with a double brass rule.
x = 68
bevel(x, y, 16, 16, BRASS, BRASS_LO, hexc("15100c", 250), width=1, inner=hexc("2a1f14"), corner=BRASS)
add("tooltip", x, y, 16, 16, 4)

# banner: the place title card, a dark ribbon with brass edges and notched ends.
x = 84
bw, bh = 40, 20
rect(x, y + 2, bw, bh - 4, hexc("15100c", 242))
rect(x, y + 2, bw, 1, INK)
rect(x, y + 3, bw, 1, BRASS_HI)
rect(x, y + 4, bw, 1, BRASS_LO)
rect(x, y + bh - 3, bw, 1, INK)
rect(x, y + bh - 4, bw, 1, BRASS_LO)
rect(x, y + bh - 5, bw, 1, BRASS)
for j in range(bh):
    d = abs(j - (bh - 1) / 2)
    cut = max(0, 6 - int(d))  # V-shaped notch depth at the ends
    for k in range(cut):
        put(x + k, y + j, CLEAR)
        put(x + bw - 1 - k, y + j, CLEAR)
add("banner", x, y, bw, bh, (12, 6, 12, 6))

# divider: a brass rule with a dark groove and diamond studs at the ends.
x = 124
rect(x + 3, y + 1, 26, 1, BRASS)
rect(x + 3, y + 2, 26, 1, BRASS_LO)
for sx in (x, x + 29):
    rect(sx + 1, y, 1, 4, BRASS)
    rect(sx, y + 1, 3, 2, BRASS)
    put(sx + 1, y + 1, BRASS_HI)
    put(sx + 1, y + 2, BRASS_LO)
add("divider", x, y, 32, 4, (4, 1, 4, 1))

# white: a plain tintable pixel block (for fills and lines).
x = 156
rect(x, y, 4, 4, hexc("ffffff"))
add("white", x, y, 4, 4, 1)

# --- Slots and buttons (row 1, y = 24) -------------------------------------
y = 24
# slot: a sunken square, dark at the top left, lit at the bottom right.
bevel(0, y, 16, 16, hexc("0a0806"), hexc("5a5044"), hexc("221c16", 232), width=2, corner=hexc("2e271f"))
add("slot", 0, y, 16, 16, 4)
bevel(16, y, 16, 16, hexc("15110d"), hexc("a8987c"), hexc("3a3126", 240), width=2, corner=hexc("4a3f31"))
add("slot_hover", 16, y, 16, 16, 4)
# slot_selected: a gold frame round the slot, with bright corner pips.
bevel(32, y, 16, 16, GOLD, BRASS_LO, hexc("2e2614", 238), width=2, corner=BRASS)
rect(32 + 3, y + 3, 10, 1, hexc("0a0806"))
rect(32 + 3, y + 3, 1, 10, hexc("0a0806"))
for cx, cy in [(1, 1), (14, 1), (1, 14), (14, 14)]:
    put(32 + cx, y + cy, BRASS_HI)
add("slot_selected", 32, y, 16, 16, 5)

# item: the raised tile an item sits on in a grid (tinted by the item's colour).
bevel(48, y, 16, 16, hexc("f0f0f0"), hexc("6a6a6a"), hexc("ababab"), width=2, corner=hexc("9a9a9a"))
add("item", 48, y, 16, 16, 4)

# buttons: carved stone blocks.
bevel(64, y, 16, 16, STONE_HI, STONE_DEEP, STONE, width=2, corner=STONE_LO)
rect(64 + 3, y + 12, 10, 1, STONE_LO)  # a worn lower lip
add("button", 64, y, 16, 16, 4)
# hover: the stone lights up with a mossy-gold glow.
bevel(80, y, 16, 16, hexc("f2e2a0"), hexc("5a4a22"), hexc("a8925a"), width=2, corner=hexc("8a763f"))
rect(80 + 3, y + 12, 10, 1, hexc("8a763f"))
add("button_hover", 80, y, 16, 16, 4)
# down: pressed in, light and dark swapped.
bevel(96, y, 16, 16, STONE_DEEP, STONE_HI, STONE_LO, width=2, corner=STONE_LO)
add("button_down", 96, y, 16, 16, 4)
bevel(112, y, 16, 16, hexc("57534c"), hexc("2a2824"), hexc("3e3b35"), width=2, corner=hexc("35322d"))
add("button_off", 112, y, 16, 16, 4)

# bar: the sunken trough round a meter. fill: what goes in it (tinted).
bevel(128, y, 12, 8, hexc("050403"), hexc("4a4238"), hexc("0a0806", 238), width=1, corner=hexc("2a241d"))
add("bar", 128, y, 12, 8, 2)
rect(140, y, 6, 6, hexc("e2e2e2"))
rect(140, y, 6, 1, hexc("ffffff"))          # a bright top edge
rect(140, y + 4, 6, 1, hexc("b4b4b4"))      # shaded lower rows give it a rounded look
rect(140, y + 5, 6, 1, hexc("8e8e8e"))
add("fill", 140, y, 6, 6, (1, 1, 1, 2))

# knob: a settings slider handle, a brass-capped iron peg.
bevel(148, y, 8, 12, IRON_HI, IRON_LO, IRON, width=1, corner=IRON)
rect(148 + 2, y + 2, 4, 2, BRASS)
put(148 + 2, y + 2, BRASS_HI)
rect(148 + 2, y + 5, 4, 1, IRON_LO)
rect(148 + 2, y + 7, 4, 1, IRON_LO)
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
    "bag": [  # the Items tab: a satchel
        "....kkkk....",
        "...kN..Nk...",
        "...k....k...",
        ".kkkkkkkkkk.",
        "knnnnnnnnnnk",
        "knNNNyyNNNnk",
        "knnnnyynnnnk",
        "knnnnnnnnnnk",
        "knnnnnnnnnnk",
        "kNnnnnnnnnNk",
        ".kkkkkkkkkk.",
        "............",
    ],
    "figure": [  # the Equipment tab: a person
        "....kkkk....",
        "...kttttk...",
        "...kttttk...",
        "....kkkk....",
        "..kkbbbbkk..",
        ".kbkbbbbkbk.",
        ".kbkbbbbkbk.",
        ".kk.kBBk.kk.",
        "....kBBk....",
        "...kBkkBk...",
        "...kNk.kNk..",
        "...kk...kk..",
    ],
    "star": [  # the Abilities tab: a gold star
        ".....kk.....",
        ".....ky.....",
        "....kyyk....",
        "kkkkkyykkkkk",
        "kyyyywwyyyyk",
        ".kyyyyyyyyk.",
        "..kyyyyyyk..",
        "..kyyYYyyk..",
        ".kyyYkkYyyk.",
        ".kyYk..kYyk.",
        ".kkk....kkk.",
        "............",
    ],
    "sword": [
        "..........kk",
        ".........kck",
        "........kcsk",
        ".......kcsk.",
        "......kcsk..",
        ".k...kcsk...",
        ".kk.kcsk....",
        "..kkcsk.....",
        "...kYk......",
        "..kYkkk.....",
        ".kYk..k.....",
        ".kk.........",
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


# --- Inventory (y = 128): slot silhouettes, tabs, glow, cards, plinth -------
# Silhouettes show what goes in an empty slot. Grey, so the game tints them faint.
SIL = {
    "helmet": [
        "................",
        ".....######.....",
        "...##########...",
        "..############..",
        "..###++++++###..",
        ".###+......+###.",
        ".###+......+###.",
        ".####......####.",
        ".####.#..#.####.",
        ".####.#..#.####.",
        ".####.#..#.####.",
        "..###......###..",
        "..##........##..",
        "................",
        "................",
        "................",
    ],
    "hat": [
        "................",
        "................",
        "................",
        "......####......",
        ".....######.....",
        ".....######.....",
        ".....######.....",
        ".....++++++.....",
        ".....######.....",
        "..############..",
        ".##############.",
        "..++++++++++++..",
        "................",
        "................",
        "................",
        "................",
    ],
    "chest": [  # body armor
        "................",
        "..###......###..",
        ".#####....#####.",
        ".######++######.",
        ".##############.",
        "..############..",
        "..####+##+####..",
        "..############..",
        "..####+##+####..",
        "..############..",
        "...##########...",
        "...####++####...",
        "...##########...",
        "....########....",
        "................",
        "................",
    ],
    "shirt": [
        "................",
        "....###..###....",
        "..#####++#####..",
        ".##############.",
        "###############+",
        "####.######.####",
        ".##..######..##.",
        ".....######.....",
        ".....######.....",
        ".....######.....",
        ".....######.....",
        ".....######.....",
        ".....++++++.....",
        "................",
        "................",
        "................",
    ],
    "trousers": [
        "................",
        "...##########...",
        "...++++++++++...",
        "...##########...",
        "...##########...",
        "...#####+####...",
        "...####..####...",
        "...####..####...",
        "...####..####...",
        "...####..####...",
        "...####..####...",
        "...####..####...",
        "...####..####...",
        "...++++..++++...",
        "................",
        "................",
    ],
    "boots": [
        "................",
        "................",
        "................",
        "...####...####..",
        "...####...####..",
        "...####...####..",
        "...####...####..",
        "...####...####..",
        "...####...####..",
        "..#####..#####..",
        ".######.######..",
        ".######.######..",
        ".++++++.++++++..",
        "................",
        "................",
        "................",
    ],
    "sword": [
        "................",
        "............###.",
        "...........####.",
        "..........####..",
        ".........####...",
        "........####....",
        ".......####.....",
        "......####......",
        "..#..####.......",
        "..#+####........",
        "...+##..........",
        "...##+#.........",
        "..##..+#........",
        ".##.............",
        "................",
        "................",
    ],
    "hand": [  # an open right hand
        "................",
        "......#.........",
        "....#.#.#.......",
        "....#.#.#.#.....",
        "....#.#.#.#.....",
        "....#######.....",
        "..#.#######.....",
        "..##.######.....",
        "...#########....",
        "....########....",
        ".....#######....",
        ".....######.....",
        ".....++++++.....",
        "................",
        "................",
        "................",
    ],
    "bag": [
        "................",
        ".....######.....",
        "....#......#....",
        "....#......#....",
        "..############..",
        "..############..",
        "..#####++#####..",
        "..#####++#####..",
        "..############..",
        "..############..",
        "..############..",
        "..++++++++++++..",
        "................",
        "................",
        "................",
        "................",
    ],
}
SIL_HI, SIL_LO = hexc("d8d8d8"), hexc("8c8c8c")
y = 128
for i, (name, rows) in enumerate(SIL.items()):
    sx = i * 16
    assert len(rows) == 16 and all(len(r) == 16 for r in rows), name
    for j, row in enumerate(rows):
        for k, ch in enumerate(row):
            if ch == "#":
                put(sx + k, y + j, SIL_HI)
            elif ch == "+":
                put(sx + k, y + j, SIL_LO)
    add("sil_" + name, sx, y, 16, 16)
# The left hand is the right one mirrored.
sx = len(SIL) * 16
for j in range(16):
    for k in range(16):
        c = px[(len(SIL) - 2) * 16 + 15 - k, y + j]
        if c[3]:
            put(sx + k, y + j, c)
add("sil_hand_left", sx, y, 16, 16)

# tab: a folder tab on top of the window, an oak board with its top corners
# cut. tab_active: the open one, the panel's own planks with no bottom edge,
# so it runs into the window. tab_hover: a lighter, raised board.
x = 176
bevel(x, y, 16, 14, hexc("6e5232"), hexc("2e2013"), hexc("2a2118", 242), width=2, corner=hexc("4a3622"))
add("tab", x, y, 16, 14, 4)
x = 192
rect(x, y, 16, 14, INK)
put(x, y, CLEAR)
put(x + 15, y, CLEAR)
rect(x + 1, y + 1, 14, 1, OAK_HI)
rect(x + 2, y + 2, 12, 1, OAK)
rect(x + 1, y + 1, 1, 13, OAK_HI)
rect(x + 2, y + 2, 1, 12, OAK)
rect(x + 14, y + 1, 1, 13, OAK_LO)
rect(x + 13, y + 2, 1, 12, OAK_MID)
rect(x + 3, y + 3, 10, 11, PANEL_FACE)
rect(x + 3, y + 3, 10, 1, SHADE)
rect(x + 1, y + 1, 2, 2, IRON)
put(x + 1, y + 1, IRON_HI)
rect(x + 13, y + 1, 2, 2, IRON)
put(x + 14, y + 2, IRON_LO)
add("tab_active", x, y, 16, 14, (4, 4, 4, 1))
x = 208
bevel(x, y, 16, 14, hexc("c49a62"), hexc("4f361d"), hexc("3d2e1f", 248), width=2, corner=hexc("74512d"))
add("tab_hover", x, y, 16, 14, 4)
# slot_glow: a slot that takes what you carry (a green-gold light round it).
x = 224
bevel(x, y, 16, 16, hexc("eaf7a8"), hexc("6e9a3a"), hexc("1f2a17", 238), width=2, outline=hexc("b8e07a"), corner=hexc("a8cc6a"), inner=hexc("0d0b09"))
add("slot_glow", x, y, 16, 16, 4)
# card: a quiet stone-edged frame for the stats tab, brass pins at the top.
x = 240
bevel(x, y, 16, 16, hexc("4a4338"), hexc("15110d"), hexc("1c1712", 236), width=1, corner=hexc("2e281f"), inner=hexc("0d0b09", 200))
put(x + 2, y + 2, BRASS)
put(x + 13, y + 2, BRASS)
add("card", x, y, 16, 16, 4)

# plinth: the stone disc the player stands on in the equipment tab, with its
# shadow. Drawn under the live picture.
y = 148
pw, ph = 64, 20
for j in range(ph):
    for i in range(pw):
        nx = (i + 0.5 - pw / 2) / (pw / 2)
        ny = (j + 0.5 - 7) / 6.5
        top = nx * nx + ny * ny
        side = nx * nx + ((j + 0.5 - 11) / 6.5) ** 2
        shade = nx * nx + ((j + 0.5 - 13) / 7.0) ** 2
        if top <= 1.0:
            rim = top > 0.78
            c = hexc("5d6a5c") if rim else (hexc("4b574a") if (i + j) % 7 else hexc("566355"))
            if ny < -0.55 and not rim:
                c = hexc("6a786a")
            put(i, y + j, c)
        elif side <= 1.0 and j > 7:
            put(i, y + j, hexc("2b332b") if j > 12 else hexc("374036"))
        elif shade <= 1.0:
            put(i, y + j, (0, 0, 0, int(120 * (1 - shade))))
add("plinth", 0, y, pw, ph)

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
