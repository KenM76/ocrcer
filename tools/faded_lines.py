"""Synthetic faded-ink text lines, for reading where the binarizer stops seeing light
text and what a contrast pre-step recovers.

Four accounting-style lines at 300 dpi and 10 pt, ink at luma L on paper at luma P,
noise-free. `_mix` pages add one black heading line above the faded ones, as on a page
where faded print sits beside dark print. Each page is written raw and after three
pre-steps:
  stretch   global: the 0.5th percentile maps to 0, the median (paper) to 255
  wolf      Wolf-Jolion threshold, k 0.5, 25 px window, global min and max std,
            written as a 0/255 image
  lstretch  local: each pixel stretched between the 61 px minimum and maximum around
            it; where that range is under 24 the pixel is paper
Writes `P<paper>_L<ink>[_mix]__<pre>.pgm` binary PGMs.
"""
import argparse
import os

import numpy as np
from PIL import Image, ImageDraw, ImageFont

LINES = ["Invoice total due 1,234.56 on 2026-09-30",
         "Balance forward (4,812.07) GST 13% applied",
         "Remit to: Account 00417-229 Ref Q3-8841",
         "Cheque 004417 payable to Northfield Supply"]
HEADING = "STATEMENT OF ACCOUNT"
PX, W, LH, TOP = 42, 1150, 70, 30
INKS = (0, 100, 140, 160, 170, 180, 190, 200, 215)
PAPERS = (255, 235)


def page(font, ink, paper, mix):
    n = len(LINES) + (1 if mix else 0)
    im = Image.new("L", (W, TOP * 2 + LH * n), paper)
    d = ImageDraw.Draw(im)
    y = TOP
    if mix:
        d.text((40, y), HEADING, fill=0, font=font)
        y += LH
    for t in LINES:
        d.text((40, y), t, fill=ink, font=font)
        y += LH
    return np.array(im).astype(float)


def box_mean(a, w):
    r = w // 2
    p = np.pad(a, r, mode="symmetric")
    c = np.pad(p.cumsum(0).cumsum(1), ((1, 0), (1, 0)))
    h, v = a.shape
    return (c[w:w + h, w:w + v] - c[:h, w:w + v] - c[w:w + h, :v] + c[:h, :v]) / (w * w)


def box_rank(a, w, fn):
    r = w // 2
    p = np.pad(a, r, mode="symmetric")
    p = fn(np.lib.stride_tricks.sliding_window_view(p, w, axis=0), axis=-1)
    return fn(np.lib.stride_tricks.sliding_window_view(p, w, axis=1), axis=-1)


def stretch(a):
    lo, hi = np.percentile(a, 0.5), np.median(a)
    return np.clip((a - lo) * 255.0 / max(hi - lo, 1), 0, 255)


def wolf(a, k=0.5, w=25):
    m = box_mean(a, w)
    s = np.sqrt(np.maximum(box_mean(a * a, w) - m * m, 0))
    lo, rmax = a.min(), s.max()
    t = (1 - k) * m + k * lo + k * (s / rmax) * (m - lo)
    return np.where(a <= t, 0.0, 255.0)


def lstretch(a, w=61, floor=24):
    hi, lo = box_rank(a, w, np.max), box_rank(a, w, np.min)
    rng = hi - lo
    return np.where(rng < floor, 255.0, np.clip((a - lo) * 255.0 / np.maximum(rng, 1), 0, 255))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--font", required=True, help="a TrueType face, e.g. Liberation Sans Regular")
    ap.add_argument("--out", default="faded_lines")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    font = ImageFont.truetype(args.font, PX)
    pres = {"none": lambda x: x, "stretch": stretch, "wolf": wolf, "lstretch": lstretch}
    for paper in PAPERS:
        for ink in INKS:
            if ink >= paper:
                continue
            for mix in (False, True):
                a = page(font, ink, paper, mix)
                for pre, f in pres.items():
                    g = np.clip(np.rint(f(a)), 0, 255).astype(np.uint8)
                    name = f"P{paper}_L{ink:03d}{'_mix' if mix else ''}__{pre}.pgm"
                    with open(os.path.join(args.out, name), "wb") as fh:
                        fh.write(f"P5 {g.shape[1]} {g.shape[0]} 255\n".encode() + g.tobytes())


if __name__ == "__main__":
    main()
