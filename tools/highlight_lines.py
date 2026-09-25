"""Synthetic highlighted and shaded text lines, for reading what the binarizer
does at the edge of a mid-grey band.

Three accounting-style lines at 300 dpi and 10 pt; the third is banded over
its first half only, so one band edge falls inside a number. The band is
multiplied over the text (ink under a highlighter), then converted to grey by
Rec.601 luma, as pdfcer does before calling the engine. Writes one binary PGM
per band: `plain`, five highlighter colours, and a grey ladder `g<level>`.
Highlighter RGB values are guesses for fluorescent inks as scanned.
"""
import argparse
import os

from PIL import Image, ImageDraw, ImageFont

LINES = ["Invoice total due 1,234.56 on 2026-09-30",
         "Balance forward (4,812.07) GST 13% applied",
         "Remit to: Account 00417-229 Ref Q3-8841"]
COLOURS = {"yellow": (255, 245, 90), "green": (130, 245, 110), "pink": (255, 120, 190),
           "orange": (255, 170, 60), "blue": (110, 200, 250)}
GREYS = (215, 200, 190, 180, 170, 160, 150, 135, 120, 100)
W, LH, TOP, PX = 1100, 70, 30, 42  # 42 px em = 10 pt at 300 dpi


def page(font, band):
    img = Image.new("RGB", (W, TOP * 2 + LH * len(LINES)), (255, 255, 255))
    d = ImageDraw.Draw(img)
    boxes = []
    for i, t in enumerate(LINES):
        d.text((40, TOP + i * LH), t, fill=(0, 0, 0), font=font)
        x0, y0, x1, y1 = d.textbbox((40, TOP + i * LH), t, font=font)
        boxes.append((x0 - 6, y0 - 5, x1 + 6, y1 + 5))
    if band:
        px = img.load()
        for i, (x0, y0, x1, y1) in enumerate(boxes):
            if i == 2:
                x1 = x0 + (x1 - x0) // 2
            for y in range(y0, y1):
                for x in range(x0, x1):
                    r, g, b = px[x, y]
                    px[x, y] = (r * band[0] // 255, g * band[1] // 255, b * band[2] // 255)
    rgb = img.tobytes()
    grey = bytes((rgb[i] * 299 + rgb[i + 1] * 587 + rgb[i + 2] * 114) // 1000
                 for i in range(0, len(rgb), 3))
    return img.size, grey


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--font", required=True, help="a licence-clean TrueType face")
    ap.add_argument("--out", required=True, help="directory for the PGM files")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    font = ImageFont.truetype(args.font, PX)
    bands = {"plain": None, **COLOURS, **{f"g{v}": (v, v, v) for v in GREYS}}
    for name, band in bands.items():
        (w, h), grey = page(font, band)
        with open(os.path.join(args.out, f"{name}.pgm"), "wb") as f:
            f.write(f"P5 {w} {h} 255\n".encode() + grey)


if __name__ == "__main__":
    main()
