#!/usr/bin/env python3
"""Draws the DMG window's background: `assets/dmg/background.png` (1x) and
`background@2x.png` (Retina). The output is in the repo; the script runs only
when the design changes (it needs Pillow, which is not a dependency of the product).

    python3 tools/dmg_background.py assets/dmg

**Why the background is not black.** Finder writes the icon names ("bateri",
"Applications") in the system's appearance without looking at the background:
black in light mode, white in dark mode, and there is no way to change the
color. bateri's black background would make the names invisible in light mode.
The background is therefore a graphite with a relative luminance of ~0.19:
both black and white text read at ~4.5:1 contrast (`contrast` below, the
script stops on a tone that does not hold the threshold).

The layout uses the **same numbers** as the icon positions in
`assets/dmg/settings.py` (`APP`, `APPS`, `WINDOW`): the arrow is drawn between
the two icons, the title above them; if one changes the other must change too.
"""

import os
import sys

from PIL import Image, ImageDraw, ImageFont

WINDOW = (640, 400)  # points; the content area of the Finder window
ICON = 128
APP = (170, 210)  # icon centers, the same as settings.py
APPS = (470, 210)
# The two ends of the background (top to bottom). Both must stay inside the 4.3:1 band.
TOP = (0x76, 0x79, 0x80)
BOTTOM = (0x71, 0x74, 0x7B)
INK = (0x08, 0x08, 0x0A)  # title and arrow: ≥4.4:1 on the graphite
FONT = "/System/Library/Fonts/SFNS.ttf"
MIN_CONTRAST = 4.3


def luminance(rgb):
    def ch(c):
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    r, g, b = (ch(c) for c in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def font(size, weight):
    f = ImageFont.truetype(FONT, size)
    f.set_variation_by_name(weight)
    return f


def draw(scale):
    w, h = WINDOW[0] * scale, WINDOW[1] * scale
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        t = y / (h - 1)
        row = tuple(round(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3))
        for x in range(w):
            px[x, y] = row

    # NO lighting under the icons: the names are in that band and anything that
    # lightens the background would push the white name's contrast below the threshold.

    d = ImageDraw.Draw(img)

    # Title: a single sentence saying what to do.
    title = "Drag bateri into Applications"
    f = font(22 * scale, b"Semibold")
    tw = d.textlength(title, font=f)
    d.text(((w - tw) / 2, 58 * scale), title, font=f, fill=INK)
    sub = "Then open it from Launchpad or Spotlight."
    f2 = font(14 * scale, b"Regular")
    sw = d.textlength(sub, font=f2)
    d.text(((w - sw) / 2, 90 * scale), sub, font=f2, fill=INK)

    # Arrow: between the two icons, dashed shaft + filled head, in the title's ink.
    x0 = (APP[0] + ICON / 2 + 22) * scale
    x1 = (APPS[0] - ICON / 2 - 22) * scale
    y = APP[1] * scale
    stroke = 3 * scale
    dash, gap = 9 * scale, 7 * scale
    head = 13 * scale
    x = x0
    while x < x1 - head:
        d.rounded_rectangle((x, y - stroke / 2, min(x + dash, x1 - head), y + stroke / 2),
                            radius=stroke / 2, fill=INK)
        x += dash + gap
    d.polygon([(x1, y), (x1 - head, y - head * 0.7), (x1 - head, y + head * 0.7)], fill=INK)
    return img


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: dmg_background.py <output directory>")
    for tone in (TOP, BOTTOM):
        for label in ((0, 0, 0), (0xFF, 0xFF, 0xFF)):
            c = contrast(tone, label)
            if c < MIN_CONTRAST:
                sys.exit(f"name #{bytes(label).hex()} on background #{bytes(tone).hex()} is {c:.2f}:1 — threshold {MIN_CONTRAST}")
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    draw(1).save(os.path.join(out, "background.png"), dpi=(72, 72))
    draw(2).save(os.path.join(out, "background@2x.png"), dpi=(144, 144))


if __name__ == "__main__":
    main()
