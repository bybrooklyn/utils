"""Draws the lealand wordmark: "lealand" in Instrument Serif Italic under a weeping willow.

    python3 logo/make.py path/to/InstrumentSerif-Italic.ttf

Writes logo/logo-light.svg and logo/logo-dark.svg (for the README) and src/logo.rs (the same picture in
coloured half-block characters, for the launch animation). Needs fonttools and pillow, and
rsvg-convert. The font is OFL, from github.com/google/fonts/tree/main/ofl/instrumentserif.
"""

import math, os, random, subprocess, sys, tempfile
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
H = 330
SIZE = 190  # font size in px
BASE = 292  # baseline
LEFT = 70

# willow greens, deep near the branch to light at the tips
COLS, ROWS = 72, 17  # terminal size of the launch logo
LEAVES = ["#24502c", "#2f6b3a", "#3f8a46", "#58a24f", "#7dbb5c", "#a3d06a", "#c8e07c"]
BARK = ("#4a3426", "#7a5a3f")


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


def bezier(p0, p1, p2, p3, t):
    u = 1 - t
    return tuple(u**3 * a + 3 * u * u * t * b + 3 * u * t * t * c + t**3 * d for a, b, c, d in zip(p0, p1, p2, p3))


def willow(word_w, strands=38):
    rnd = random.Random(7)
    # the branch comes in from above the left edge and arches over the word
    P = [(-20, 70), (170, 6), (430, 24), (LEFT + word_w + 30, 88)]
    pts = [bezier(*P, i / 60) for i in range(61)]
    top, bot = [], []
    for i, (x, y) in enumerate(pts):
        t = i / 60
        a = pts[min(i + 1, 60)]
        b = pts[max(i - 1, 0)]
        dx, dy = a[0] - b[0], a[1] - b[1]
        n = math.hypot(dx, dy) or 1
        w = 13 * (1 - t) ** 1.4 + 1.2
        top.append((x - dy / n * w, y + dx / n * w))
        bot.append((x + dy / n * w, y - dx / n * w))
    poly = top + bot[::-1]
    branch = "M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in poly) + "Z"

    back, front = [], []
    for k in range(strands):
        t = 0.12 + 0.88 * k / (strands - 1) + rnd.uniform(-0.01, 0.01)
        x0, y0 = bezier(*P, t)
        length = rnd.uniform(70, 235) * (0.55 + 0.45 * math.sin(math.pi * min(t * 1.15, 1)))
        sway = rnd.uniform(-22, 18)
        x1, y1 = x0 + sway * 0.4, y0 + length * 0.5
        x2, y2 = x0 + sway, y0 + length
        strand = f"M{x0:.1f},{y0:.1f} Q{x1:.1f},{y1:.1f} {x2:.1f},{y2:.1f}"
        bits = [f'<path d="{strand}" fill="none" stroke="#3b5e34" stroke-width="1.1" opacity=".7"/>']
        n = int(length / 9)
        for j in range(1, n + 1):
            s = j / (n + 0.6)
            # point and direction along the quadratic strand
            px = (1 - s) ** 2 * x0 + 2 * (1 - s) * s * x1 + s * s * x2
            py = (1 - s) ** 2 * y0 + 2 * (1 - s) * s * y1 + s * s * y2
            tx = 2 * (1 - s) * (x1 - x0) + 2 * s * (x2 - x1)
            ty = 2 * (1 - s) * (y1 - y0) + 2 * s * (y2 - y1)
            ang = math.degrees(math.atan2(ty, tx))
            side = 1 if j % 2 else -1
            lean = ang + side * rnd.uniform(18, 34)
            col = LEAVES[min(len(LEAVES) - 1, int(s * len(LEAVES) * 0.85 + rnd.uniform(0, 1.6)))]
            ln = rnd.uniform(9, 14) * (1 - 0.35 * s)
            bits.append(
                f'<ellipse cx="{px + side * 2.5:.1f}" cy="{py:.1f}" rx="{ln:.1f}" ry="{ln * 0.24:.1f}" '
                f'transform="rotate({lean:.0f} {px + side * 2.5:.1f} {py:.1f})" fill="{col}"/>'
            )
        (front if k % 4 == 1 else back).append("".join(bits))
    # a few loose leaves drifting down past the word
    for _ in range(5):
        x, y = rnd.uniform(LEFT, LEFT + word_w), rnd.uniform(BASE - 40, BASE + 20)
        front.append(
            f'<ellipse cx="{x:.1f}" cy="{y:.1f}" rx="7" ry="1.8" transform="rotate({rnd.uniform(-60, 60):.0f} {x:.1f} {y:.1f})" '
            f'fill="{rnd.choice(LEAVES[3:])}" opacity=".85"/>'
        )
    return branch, back, front


def svg(font_path, ink, strands=38):
    """ink: 'auto' (light/dark by media query) or a fixed colour pair for rasterising."""
    path, word_w = word(font_path)
    branch, back, front = willow(word_w, strands)
    W = int(LEFT + word_w + 60)
    light, dark = ("#16301c", "#2f5a35"), ("#f3ecd8", "#cfe6b0")
    if ink != "auto":
        light = dark = ink
    style = f"""
    .ink {{ fill: url(#inkL) }}
    @media (prefers-color-scheme: dark) {{ .ink {{ fill: url(#inkD) }} }}""" if ink == "auto" else ".ink { fill: url(#inkL) }"
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}">
  <title>lealand</title>
  <style>{style}
  </style>
  <defs>
    <linearGradient id="inkL" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{light[0]}"/><stop offset="1" stop-color="{light[1]}"/></linearGradient>
    <linearGradient id="inkD" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{dark[0]}"/><stop offset="1" stop-color="{dark[1]}"/></linearGradient>
    <linearGradient id="bark" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="{BARK[0]}"/><stop offset="1" stop-color="{BARK[1]}"/></linearGradient>
  </defs>
  <g>{"".join(back)}</g>
  <path d="{branch}" fill="url(#bark)"/>
  <path class="ink" d="{path}"/>
  <g>{"".join(front)}</g>
</svg>
"""


def cells(font_path, cols, rows):
    """The picture as rows of (char, fg, bg) half-block cells, bg None for the terminal's own."""
    with tempfile.TemporaryDirectory() as tmp:
        src, png = os.path.join(tmp, "t.svg"), os.path.join(tmp, "t.png")
        open(src, "w").write(svg(font_path, ("#f3ecd8", "#cfe6b0"), strands=16))
        subprocess.run(["rsvg-convert", "-w", str(cols * 8), "-h", str(rows * 16), "--keep-aspect-ratio", src, "-o", png], check=True)
        big = Image.open(png).convert("RGBA")
        canvas = Image.new("RGBA", (cols * 8, rows * 16))
        canvas.paste(big, (0, rows * 16 - big.height))
        canvas.save(png)
        big = Image.open(png).convert("RGBA")
    # average 8x8 blocks with colour weighted by alpha, so thin strokes keep their colour
    img = Image.new("RGBA", (cols, rows * 2))
    px = big.load()
    for cy in range(rows * 2):
        for cx in range(cols):
            r = g = b = a = 0
            for yy in range(cy * 8, cy * 8 + 8):
                for xx in range(cx * 8, cx * 8 + 8):
                    pr, pg, pb, pa = px[xx, yy]
                    r, g, b, a = r + pr * pa, g + pg * pa, b + pb * pa, a + pa
            img.putpixel((cx, cy), (r // a, g // a, b // a, a // 64) if a else (0, 0, 0, 0))
    out = []
    p = img.load()
    on = lambda c: c[3] > 70
    rgb = lambda c: (c[0] << 16) | (c[1] << 8) | c[2]
    for y in range(rows):
        line = []
        for x in range(cols):
            t, b = p[x, 2 * y], p[x, 2 * y + 1]
            if on(t) and on(b):
                line.append(("▀", rgb(t), rgb(b)))
            elif on(t):
                line.append(("▀", rgb(t), None))
            elif on(b):
                line.append(("▄", rgb(b), None))
            else:
                line.append((" ", 0, None))
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
        "/// The wordmark in half blocks: (char, foreground, background or the terminal's own).\n"
        "pub const LOGO: &[&[(char, u32, Option<u32>)]] = &[\n" + "\n".join(lines) + "\n];\n"
    )


if __name__ == "__main__":
    font = sys.argv[1]
    open(os.path.join(HERE, "logo-light.svg"), "w").write(svg(font, ("#16301c", "#2f5a35")))
    open(os.path.join(HERE, "logo-dark.svg"), "w").write(svg(font, ("#f3ecd8", "#cfe6b0")))
    rows = cells(font, COLS, ROWS)
    open(os.path.join(HERE, "..", "src", "logo.rs"), "w").write(rust(rows))
    # a preview of the terminal version, to look at
    if len(sys.argv) > 2:
        cw, ch = 10, 20
        im = Image.new("RGB", (COLS * cw, ROWS * ch), (30, 32, 36))
        for y, row in enumerate(rows):
            for x, (c, fg, bg) in enumerate(row):
                hexrgb = lambda v: ((v >> 16) & 255, (v >> 8) & 255, v & 255)
                top = hexrgb(fg) if c == "▀" else (hexrgb(bg) if bg is not None else None)
                bot = hexrgb(fg) if c == "▄" else (hexrgb(bg) if bg is not None else None)
                if c == " ":
                    continue
                for yy in range(ch):
                    colr = top if yy < ch // 2 else bot
                    if colr:
                        for xx in range(cw):
                            im.putpixel((x * cw + xx, y * ch + yy), colr)
        im.save(sys.argv[2])
