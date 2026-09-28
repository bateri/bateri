#!/usr/bin/env python3
"""DMG penceresinin arka planını çizer: `assets/dmg/background.png` (1x) ve
`background@2x.png` (Retina). Çıktı depoda; betik yalnız tasarım değişince
koşar (Pillow ister, ürünün bağımlılığı değil).

    python3 tools/dmg_background.py assets/dmg

**Zemin neden siyah değil.** Finder simge adlarını ("bateri",
"Applications") arka plana bakmadan sistemin görünümüyle yazıyor: açık
kipte siyah, koyu kipte beyaz, ve rengi değiştirmenin yolu yok. bateri'nin
siyah zemini açık kipte adları görünmez yapardı. Zemin bu yüzden bağıl
parlaklığı ~0.19 olan bir grafit: siyah ve beyaz yazının ikisi de ~4.5:1
kontrastla okunuyor (`contrast` aşağıda, betik eşiği tutmayan tonda durur).

Yerleşim `assets/dmg/settings.py`'deki simge konumlarıyla **aynı sayılar**
(`APP`, `APPS`, `WINDOW`): ok iki simgenin arasına, başlık üstlerine
çiziliyor; biri değişirse öteki de değişmeli.
"""

import os
import sys

from PIL import Image, ImageDraw, ImageFont

WINDOW = (640, 400)  # nokta; Finder penceresinin içerik alanı
ICON = 128
APP = (170, 210)  # simge merkezleri, settings.py ile aynı
APPS = (470, 210)
# Zeminin iki ucu (üstten alta). İkisi de 4.3:1 bandının içinde kalmalı.
TOP = (0x76, 0x79, 0x80)
BOTTOM = (0x71, 0x74, 0x7B)
INK = (0x08, 0x08, 0x0A)  # başlık ve ok: grafitte ≥4.4:1
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

    # Simgelerin altında aydınlatma YOK: adlar o bantta ve zemini açan her
    # şey beyaz adın kontrastını eşiğin altına iterdi.

    d = ImageDraw.Draw(img)

    # Başlık: ne yapılacağını söyleyen tek cümle.
    title = "Drag bateri into Applications"
    f = font(22 * scale, b"Semibold")
    tw = d.textlength(title, font=f)
    d.text(((w - tw) / 2, 58 * scale), title, font=f, fill=INK)
    sub = "Then open it from Launchpad or Spotlight."
    f2 = font(14 * scale, b"Regular")
    sw = d.textlength(sub, font=f2)
    d.text(((w - sw) / 2, 90 * scale), sub, font=f2, fill=INK)

    # Ok: iki simgenin arasında, kesik gövde + dolu uç, başlığın mürekkebiyle.
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
        sys.exit("kullanım: dmg_background.py <çıktı dizini>")
    for tone in (TOP, BOTTOM):
        for label in ((0, 0, 0), (0xFF, 0xFF, 0xFF)):
            c = contrast(tone, label)
            if c < MIN_CONTRAST:
                sys.exit(f"zemin #{bytes(tone).hex()} üstünde #{bytes(label).hex()} ad {c:.2f}:1 — eşik {MIN_CONTRAST}")
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    draw(1).save(os.path.join(out, "background.png"), dpi=(72, 72))
    draw(2).save(os.path.join(out, "background@2x.png"), dpi=(144, 144))


if __name__ == "__main__":
    main()
