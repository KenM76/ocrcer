"""Synthetic pen marks beside printed text: an accountant's ticks, circles, underlines
and initials, for reading whether a mark changes the printed figures next to it.

The four accounting lines of faded_lines.py in black at 300 dpi and 10 pt, set 100 px
in so the left margin can hold a mark. Marks are drawn in black with a 4 px pen (about
a 0.35 mm ballpoint; a guess), one kind per page:
  clean          no marks (control)
  tick_g<N>      a tick N px after the end of lines 1, 3 and 4
  tick_touch     the same tick starting 4 px inside the last glyph
  margin_tick    a tick in the left margin, 30 px before line 4
  circle_tight   an ellipse on the box of `1,234.56` (line 1) and of `004417` (line 4)
                 padded 10 px; it clips the corners of the end glyphs
  circle_wide    the ellipse through the box corners plus 4 px: clear of the circled
                 glyphs, across the neighbouring words
  uline_clear    a hand underline under `(4,812.07)` (line 2): a 2 px sine wave centred
                 8 px below the brackets' lowest ink, so about 4 px clear of it, running
                 10 px past each end of the figure
  uline_touch    the same, centred 1 px below that: the pen crosses the bracket tails
  uline_straight uline_clear drawn straight (no wave)
  uline_short    uline_clear stopping 4 px inside the figure's ends instead of 10 px past them
  initials       a two-loop scribble 60 px after the end of line 3
Tick 36 x 48 px, about 1.6 cap heights tall. Every mark is a fixed polyline; nothing
is random. Writes `<page>.pgm` binary PGMs.
"""
import argparse
import math
import os

from PIL import Image, ImageDraw, ImageFont

LINES = ["Invoice total due 1,234.56 on 2026-09-30",
         "Balance forward (4,812.07) GST 13% applied",
         "Remit to: Account 00417-229 Ref Q3-8841",
         "Cheque 004417 payable to Northfield Supply"]
PX, W, H, LH, TOP, X0, PEN = 42, 1250, 360, 70, 30, 100, 4


def span(font, line, sub):
    """Ink box (x0, y0, x1, y1) of `sub` inside LINES[line], in page pixels."""
    t = LINES[line]
    i = t.index(sub)
    x = X0 + font.getlength(t[:i])
    b = font.getbbox(sub)
    y = TOP + line * LH
    return x + b[0], y + b[1], x + b[2], y + b[3]


def tick(d, x, ybot):
    w, h = 36, 48
    d.line([(x, ybot - 0.45 * h), (x + 0.3 * w, ybot), (x + w, ybot - h)], fill=0, width=PEN, joint="curve")


def ellipse(d, box, wide):
    x0, y0, x1, y1 = box
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    if wide:
        a, b = (x1 - x0) / 2 * math.sqrt(2) + 4, (y1 - y0) / 2 * math.sqrt(2) + 4
    else:
        a, b = (x1 - x0) / 2 + 10, (y1 - y0) / 2 + 10
    d.ellipse([cx - a, cy - b, cx + a, cy + b], outline=0, width=PEN)


def uline(d, box, below, amp=2, ext=10):
    x0, _, x1, y1 = box
    pts = [(x, y1 + below + amp * math.sin((x - x0) / 25.0)) for x in range(int(x0) - ext, int(x1) + ext + 1, 2)]
    d.line(pts, fill=0, width=PEN, joint="curve")


def initials(d, x, ymid):
    for cx in (x + 18, x + 58):
        pts = [(cx + 16 * math.cos(a / 10.0) + a * 0.8, ymid + 22 * math.sin(a / 10.0)) for a in range(0, 70)]
        d.line(pts, fill=0, width=PEN, joint="curve")


def page(font, kind):
    im = Image.new("L", (W, H), 255)
    d = ImageDraw.Draw(im)
    for i, t in enumerate(LINES):
        d.text((X0, TOP + i * LH), t, fill=0, font=font)
    ends = {i: span(font, i, LINES[i].split()[-1]) for i in range(len(LINES))}
    if kind.startswith("tick_g"):
        g = int(kind[6:])
        for i in (0, 2, 3):
            tick(d, ends[i][2] + g, ends[i][3])
    elif kind == "tick_touch":
        for i in (0, 2, 3):
            tick(d, ends[i][2] - 4, ends[i][3])
    elif kind == "margin_tick":
        tick(d, X0 - 30 - 36, ends[3][3])
    elif kind.startswith("circle"):
        ellipse(d, span(font, 0, "1,234.56"), kind == "circle_wide")
        ellipse(d, span(font, 3, "004417"), kind == "circle_wide")
    elif kind == "uline_straight":
        uline(d, span(font, 1, "(4,812.07)"), 8, amp=0)
    elif kind == "uline_short":
        uline(d, span(font, 1, "(4,812.07)"), 8, ext=-4)
    elif kind.startswith("uline"):
        uline(d, span(font, 1, "(4,812.07)"), 8 if kind == "uline_clear" else 1)
    elif kind == "initials":
        initials(d, ends[2][2] + 60, (ends[2][1] + ends[2][3]) / 2)
    return im


KINDS = ["clean", "tick_g60", "tick_g25", "tick_g12", "tick_touch", "margin_tick",
         "circle_tight", "circle_wide", "uline_clear", "uline_touch", "uline_straight", "uline_short", "initials"]


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--font", required=True, help="a TrueType face, e.g. Liberation Sans Regular")
    ap.add_argument("--out", default="pen_marks_lines")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    font = ImageFont.truetype(args.font, PX)
    for k in KINDS:
        im = page(font, k)
        with open(os.path.join(args.out, f"{k}.pgm"), "wb") as fh:
            fh.write(f"P5 {W} {H} 255\n".encode() + im.tobytes())


if __name__ == "__main__":
    main()
