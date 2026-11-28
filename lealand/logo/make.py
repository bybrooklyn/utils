"""Draws the lealand wordmark: "lealand" in Instrument Serif Italic under a weeping willow.

    python3 logo/make.py path/to/InstrumentSerif-Italic.ttf [preview.png]

Writes logo/logo-light.svg and logo/logo-dark.svg (for the README) and src/logo.rs (the same
picture in sextant characters, 2x3 dots a cell, for the launch animation). Needs fonttools,
pillow and rsvg-convert. The font is OFL, from github.com/google/fonts/tree/main/ofl/instrumentserif.
"""

import math, os, random, subprocess, sys, tempfile
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
H = 340
SIZE = 190  # font size in px
BASE = 300  # baseline
LEFT = 70
COLS, ROWS = 80, 18  # terminal size of the launch logo

# willow greens: back layer, middle, front
BACK = ["#1d3b24", "#24502c", "#2c5e33"]
MID = ["#2f6b3a", "#3f8a46", "#4f9a4a", "#64ab52"]
FRONT = ["#6fb65a", "#8cc663", "#a8d46a", "#c6e27c"]
BARK = ("#3e2b1f", "#6e5038", "#8a6a4c")


def word(font_path):
    font = TTFont(font_path)
    gs, cmap = font.getGlyphSet(), font.getBestCmap()
    scale = SIZE / font["head"].unitsPerEm
    pen = SVGPathPen(gs)
    x = 0
    for ch in "lealand":
        name = cmap[ord(ch)]
        gs[name].draw(TransformPen(pen, (scale, 0, 0, -scale, LEFT + x, BASE)))
        x += gs[name].width * scale
    return pen.getCommands(), x


def bez(p0, p1, p2, p3, t):
    u = 1 - t
    return tuple(u**3 * a + 3 * u * u * t * b + 3 * u * t * t * c + t**3 * d for a, b, c, d in zip(p0, p1, p2, p3))


def tapered(P, w0, w1, steps=60):
    """A branch: a filled shape along a cubic curve, w0 thick at the start, w1 at the end."""
    pts = [bez(*P, i / steps) for i in range(steps + 1)]
    top, bot = [], []
    for i, (x, y) in enumerate(pts):
        a, b = pts[min(i + 1, steps)], pts[max(i - 1, 0)]
        dx, dy = a[0] - b[0], a[1] - b[1]
        n = math.hypot(dx, dy) or 1
        w = w1 + (w0 - w1) * (1 - i / steps) ** 1.3
        top.append((x - dy / n * w, y + dx / n * w))
        bot.append((x + dy / n * w, y - dx / n * w))
    return "M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in top + bot[::-1]) + "Z"


def leaf(x, y, ang, length, width, color, opacity=1):
    """A willow leaf: long, narrow, pointed at both ends, hanging from (x, y) at angle ang."""
    a = math.radians(ang)
    ex, ey = x + math.cos(a) * length, y + math.sin(a) * length
    nx, ny = -math.sin(a) * width, math.cos(a) * width
    mx, my = (x + ex) / 2, (y + ey) / 2
    op = f' opacity="{opacity}"' if opacity < 1 else ""
    return (
        f'<path d="M{x:.1f},{y:.1f} Q{mx + nx:.1f},{my + ny:.1f} {ex:.1f},{ey:.1f} '
        f'Q{mx - nx:.1f},{my - ny:.1f} {x:.1f},{y:.1f}Z" fill="{color}"{op}/>'
    )


def strand(rnd, x0, y0, length, palette, chunky, opacity=1):
    """One hanging strand: a thin stem curving down in a soft S, with leaves along it."""
    sway = rnd.uniform(-26, 22)
    P = [(x0, y0), (x0 + sway * 0.2, y0 + length * 0.35), (x0 + sway * 1.1, y0 + length * 0.7), (x0 + sway * 0.8, y0 + length)]
    out = []
    pts = [bez(*P, i / 40) for i in range(41)]
    stem = "M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in pts)
    sw = 3.2 if chunky else 0.9
    out.append(f'<path d="{stem}" fill="none" stroke="#2c4a27" stroke-width="{sw}" stroke-linecap="round" opacity="{0.8 * opacity:.2f}"/>')
    n = max(3, int(length / (11 if chunky else 7)))
    for j in range(1, n + 1):
        s = j / (n + 0.3)
        x, y = bez(*P, s)
        x2, y2 = bez(*P, min(s + 0.01, 1))
        down = math.degrees(math.atan2(y2 - y, x2 - x))
        side = 1 if j % 2 else -1
        ang = down + side * rnd.uniform(12, 30)
        ln = rnd.uniform(11, 17) * (1 - 0.4 * s) * (1.4 if chunky else 1)
        wd = ln * (0.3 if chunky else 0.13)
        col = palette[max(0, min(len(palette) - 1, int(s * len(palette) + rnd.uniform(-0.6, 0.9))))]
        out.append(leaf(x, y, ang, ln, wd, col, opacity))
    return "".join(out)


def willow(word_w, strands=40, chunky=False):
    rnd = random.Random(11)
    right = LEFT + word_w + 30
    # the main branch comes in from above the left edge and arches over the word
    P = [(-20, 74), (170, 8), (430, 26), (right, 92)]
    branches = [tapered(P, 15, 1.5)]
    twigs = []
    for t, length in [(0.3, 80), (0.56, 100), (0.78, 70)]:
        x, y = bez(*P, t)
        Q = [(x, y), (x + length * 0.35, y + 4), (x + length * 0.75, y + 16), (x + length, y + 38)]
        branches.append(tapered(Q, 4, 0.8, 30))
        twigs.append(Q)

    back, mid, front = [], [], []
    for k in range(strands):
        t = 0.1 + 0.9 * k / (strands - 1) + rnd.uniform(-0.012, 0.012)
        x0, y0 = bez(*P, t)
        # longest over the middle of the word, some reaching past the baseline
        reach = math.sin(math.pi * min(t * 1.1, 1)) ** 0.7
        length = rnd.uniform(60, 250) * (0.45 + 0.55 * reach)
        layer = k % 5
        if layer in (0, 3):
            back.append(strand(rnd, x0 + rnd.uniform(-6, 6), y0, length * 0.85, BACK, chunky, 0.85))
        elif layer == 2:
            front.append(strand(rnd, x0, y0, length, FRONT, chunky))
        else:
            mid.append(strand(rnd, x0, y0, length, MID, chunky))
    for Q in twigs:
        for s in (0.35, 0.65, 0.95):
            x0, y0 = bez(*Q, s)
            mid.append(strand(rnd, x0, y0, rnd.uniform(50, 140), MID, chunky))
    # a few leaves falling past the word
    if not chunky:
        for _ in range(6):
            x, y = rnd.uniform(LEFT, right - 30), rnd.uniform(BASE - 30, BASE + 28)
            front.append(leaf(x, y, rnd.uniform(-50, 50), 10, 1.8, rnd.choice(FRONT), 0.85))
    return branches, back, mid, front


def svg(font_path, ink, strands=40, chunky=False):
    path, word_w = word(font_path)
    branches, back, mid, front = willow(word_w, strands, chunky)
    W = int(LEFT + word_w + 60)
    bark = "".join(f'<path d="{b}" fill="url(#bark)"/>' for b in branches)
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}">
  <title>lealand</title>
  <defs>
    <linearGradient id="ink" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{ink[0]}"/><stop offset="1" stop-color="{ink[1]}"/></linearGradient>
    <linearGradient id="bark" x1="0" y1="0" x2="1" y2="0.3"><stop offset="0" stop-color="{BARK[0]}"/><stop offset=".5" stop-color="{BARK[1]}"/><stop offset="1" stop-color="{BARK[2]}"/></linearGradient>
  </defs>
  <g>{"".join(back)}</g>
  {bark}
  <g>{"".join(mid)}</g>
  <path d="{path}" fill="url(#ink)"/>
  <g>{"".join(front)}</g>
</svg>
"""


# Sextants: a cell is 2 dots wide and 3 high. Bit k is dot k, row by row from the top left.
def sextant(bits):
    if bits == 0:
        return " "
    if bits == 63:
        return "█"
    if bits == 21:
        return "▌"
    if bits == 42:
        return "▐"
    i = bits - 1 - (bits > 21) - (bits > 42)
    return chr(0x1FB00 + i)


def unsextant(c):
    if c == "█":
        return 63
    if c == "▌":
        return 21
    if c == "▐":
        return 42
    bits = ord(c) - 0x1FB00 + 1
    bits += bits >= 21
    bits += bits >= 42
    return bits


def cells(font_path, cols, rows):
    """The picture as rows of (char, fg, bg) cells, bg None for the terminal's own."""
    # a dot is half a cell wide and a third of a cell high, so 3:4 on a 1:2 cell
    sx, sy = 6, 8
    with tempfile.TemporaryDirectory() as tmp:
        src, png = os.path.join(tmp, "t.svg"), os.path.join(tmp, "t.png")
        open(src, "w").write(svg(font_path, ("#f3ecd8", "#d7ecbc"), strands=22, chunky=True))
        w, h = cols * 2 * sx, rows * 3 * sy
        subprocess.run(["rsvg-convert", "-w", str(w), "-h", str(h), "--keep-aspect-ratio", src, "-o", png], check=True)
        big = Image.open(png).convert("RGBA")
    canvas = Image.new("RGBA", (w, h))
    canvas.paste(big, (0, h - big.height))
    px = canvas.load()
    dots = [[None] * (cols * 2) for _ in range(rows * 3)]
    for dy in range(rows * 3):
        for dx in range(cols * 2):
            r = g = b = a = 0
            for yy in range(dy * sy, dy * sy + sy):
                for xx in range(dx * sx, dx * sx + sx):
                    pr, pg, pb, pa = px[xx, yy]
                    r, g, b, a = r + pr * pa, g + pg * pa, b + pb * pa, a + pa
            if a / (sx * sy) > 95:
                dots[dy][dx] = (r // a, g // a, b // a)
    lum = lambda c: 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
    avg = lambda cs: tuple(sum(c[i] for c in cs) // len(cs) for i in range(3))
    pack = lambda c: (c[0] << 16) | (c[1] << 8) | c[2]
    out = []
    for cy in range(rows):
        line = []
        for cx in range(cols):
            six = [dots[cy * 3 + k // 2][cx * 2 + k % 2] for k in range(6)]
            on = [c for c in six if c]
            if not on:
                line.append((" ", 0, None))
            elif len(on) < 6:
                bits = sum(1 << k for k, c in enumerate(six) if c)
                line.append((sextant(bits), pack(avg(on)), None))
            else:
                # all six dots coloured: split them into a light and a dark group
                m = sum(map(lum, on)) / 6
                light = [c for c in on if lum(c) >= m]
                dark = [c for c in on if lum(c) < m]
                if not dark or lum(avg(light)) - lum(avg(dark)) < 18:
                    line.append(("█", pack(avg(on)), None))
                else:
                    bits = sum(1 << k for k, c in enumerate(six) if lum(c) >= m)
                    line.append((sextant(bits), pack(avg(light)), pack(avg(dark))))
        while line and line[-1][0] == " ":
            line.pop()
        out.append(line)
    return out


def rust(rows):
    lines = []
    for row in rows:
        items = ", ".join(f"('{c}', 0x{fg:06x}, {'None' if bg is None else f'Some(0x{bg:06x})'})" for c, fg, bg in row)
        lines.append(f"    &[{items}],")
    return (
        "// Made by logo/make.py; don't edit by hand.\n"
        "/// The wordmark in sextant characters: (char, foreground, background or the terminal's own).\n"
        "pub const LOGO: &[&[(char, u32, Option<u32>)]] = &[\n" + "\n".join(lines) + "\n];\n"
    )


def preview(rows, path):
    """What the terminal version looks like, drawn dot by dot."""
    dw, dh = 6, 8
    im = Image.new("RGB", (COLS * 2 * dw, ROWS * 3 * dh), (24, 26, 27))
    unhex = lambda v: ((v >> 16) & 255, (v >> 8) & 255, v & 255)
    for y, row in enumerate(rows):
        for x, (c, fg, bg) in enumerate(row):
            if c == " ":
                continue
            bits = unsextant(c)
            for k in range(6):
                col = unhex(fg) if bits >> k & 1 else (unhex(bg) if bg is not None else None)
                if col:
                    x0, y0 = (x * 2 + k % 2) * dw, (y * 3 + k // 2) * dh
                    im.paste(col, (x0, y0, x0 + dw, y0 + dh))
    im.save(path)


if __name__ == "__main__":
    font = sys.argv[1]
    open(os.path.join(HERE, "logo-light.svg"), "w").write(svg(font, ("#16301c", "#2f5a35")))
    open(os.path.join(HERE, "logo-dark.svg"), "w").write(svg(font, ("#f3ecd8", "#cfe6b0")))
    rows = cells(font, COLS, ROWS)
    open(os.path.join(HERE, "..", "src", "logo.rs"), "w").write(rust(rows))
    if len(sys.argv) > 2:
        preview(rows, sys.argv[2])
