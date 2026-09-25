"""Synthetic 9-pin draft dot-matrix text lines, for reading what happens when every
glyph arrives as separate dots.

Four accounting-style lines at 300 dpi. The dot grid is a monospace face rendered at
11 px and thresholded at 140, which gives 5x7-like shapes (cap height 7 cells, 7 cells
per character). Each on-cell becomes a round dot at 1/70 in horizontal pitch (10 cpi)
and 1/72 in vertical pitch, drawn at 4x and box-filtered down. Arms:
  solid     each cell filled as a rectangle: the same shapes with no gaps (control)
  d<NNN>    black dots, diameter NNN/100 of the vertical pitch
  worn080   grey (110) dots at 0.80, a worn ribbon
Each dot arm is also written after a grey pre-step: gauss15 / gauss25 (Gaussian blur,
PIL radius 1.5 / 2.5), min3 (3x3 minimum filter, ink grows one pixel), close5 (5x5
minimum then maximum). Writes `<arm>__<pre>.pgm` binary PGMs.
"""
import argparse
import os

from PIL import Image, ImageDraw, ImageFilter, ImageFont

LINES = ["Invoice total due 1,234.56 on 2026-09-30",
         "Balance forward (4,812.07) GST 13% applied",
         "Remit to: Account 00417-229 Ref Q3-8841",
         "Cheque 004417 payable to Northfield Supply"]
PX, PY, SS = 300 / 70, 300 / 72, 4
LINE_CELLS, MARGIN_X, MARGIN_Y = 18, 8, 4
ARMS = {"solid": (None, 0), "d130": (1.3, 0), "d100": (1.0, 0),
        "d080": (0.8, 0), "d060": (0.6, 0), "worn080": (0.8, 110)}
PRES = {"none": lambda im: im,
        "gauss15": lambda im: im.filter(ImageFilter.GaussianBlur(1.5)),
        "gauss25": lambda im: im.filter(ImageFilter.GaussianBlur(2.5)),
        "min3": lambda im: im.filter(ImageFilter.MinFilter(3)),
        "close5": lambda im: im.filter(ImageFilter.MinFilter(5)).filter(ImageFilter.MaxFilter(5))}


def dot_grid(font_path):
    font = ImageFont.truetype(font_path, 11)
    cells = set()
    for i, t in enumerate(LINES):
        im = Image.new("L", (int(font.getlength(t)) + 4, 16), 0)
        ImageDraw.Draw(im).text((2, 0), t, fill=255, font=font)
        px = im.load()
        for y in range(im.height):
            for x in range(im.width):
                if px[x, y] > 140:
                    cells.add((MARGIN_X + x, MARGIN_Y + i * LINE_CELLS + y))
    gw = max(x for x, _ in cells) + MARGIN_X
    gh = len(LINES) * LINE_CELLS + 2 * MARGIN_Y
    return cells, gw, gh


def draw(cells, w, h, dia, ink):
    im = Image.new("L", (w * SS, h * SS), 255)
    d = ImageDraw.Draw(im)
    for gx, gy in sorted(cells):
        if dia is None:
            d.rectangle([gx * PX * SS, gy * PY * SS, (gx + 1) * PX * SS, (gy + 1) * PY * SS], fill=ink)
        else:
            cx, cy, r = (gx + 0.5) * PX * SS, (gy + 0.5) * PY * SS, dia * PY * SS / 2
            d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=ink)
    return im.resize((w, h), Image.BOX)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--font", required=True, help="a monospace TrueType face, e.g. Liberation Mono Regular")
    ap.add_argument("--out", default="dotmatrix_lines")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    cells, gw, gh = dot_grid(args.font)
    w, h = int(gw * PX), int(gh * PY)
    for arm, (dia, ink) in ARMS.items():
        base = draw(cells, w, h, dia, ink)
        for pre, f in PRES.items():
            if arm == "solid" and pre != "none":
                continue
            with open(os.path.join(args.out, f"{arm}__{pre}.pgm"), "wb") as fh:
                fh.write(f"P5 {w} {h} 255\n".encode() + f(base).tobytes())
    print(w, h, len(cells), "dots")


if __name__ == "__main__":
    main()
