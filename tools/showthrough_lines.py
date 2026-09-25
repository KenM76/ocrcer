"""Synthetic show-through: text printed on the back of the sheet, seen mirrored and
blurred through the paper, behind the front text.

The front is the four accounting lines of faded_lines.py at luma F (black or faded) on
white paper, 300 dpi and 10 pt. The back is eight other lines drawn black, mirrored
left to right, Gaussian-blurred (PIL radius 2) and multiplied into the page so that
its darkest pixel is luma B; its lines start half a line below the front's and run on
below them, so the lower half of the page holds back text only. Pages are written raw
and after two pre-steps, defined as in faded_lines.py:
  stretch   global: the 0.5th percentile maps to 0, the median (paper) to 255
  lstretch  local: stretched between the 61 px minimum and maximum; under 24 is paper
Writes `F<front>_B<back>__<pre>.pgm` binary PGMs; B 255 is the no-show-through control.
"""
import argparse
import os

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont, ImageOps

LINES = ["Invoice total due 1,234.56 on 2026-09-30",
         "Balance forward (4,812.07) GST 13% applied",
         "Remit to: Account 00417-229 Ref Q3-8841",
         "Cheque 004417 payable to Northfield Supply"]
BACK = ["TERMS: NET 30 DAYS FROM DATE OF INVOICE",
        "Interest at 2% per month on overdue balances",
        "Please quote the account number with payment",
        "Cheques payable to Northfield Supply Ltd.",
        "Direct deposit: transit 00417 institution 229",
        "Questions about this statement? Call 1-800",
        "GST/HST registration no. 12345 6789 RT0001",
        "Thank you for your business"]
PX, W, LH, TOP, H = 42, 1150, 70, 30, 660
FRONTS = (0, 160)
BACKS = (255, 245, 230, 215, 200, 185)


def front(font, ink):
    im = Image.new("L", (W, H), 255)
    d = ImageDraw.Draw(im)
    for i, t in enumerate(LINES):
        d.text((40, TOP + i * LH), t, fill=ink, font=font)
    return np.array(im).astype(float) / 255.0


def back(font):
    im = Image.new("L", (W, H), 0)
    d = ImageDraw.Draw(im)
    for i, t in enumerate(BACK):
        d.text((40, TOP + LH // 2 + i * LH), t, fill=255, font=font)
    im = ImageOps.mirror(im).filter(ImageFilter.GaussianBlur(2))
    a = np.array(im).astype(float)
    return a / a.max()


def box_rank(a, w, fn):
    p = np.pad(a, w // 2, mode="symmetric")
    p = fn(np.lib.stride_tricks.sliding_window_view(p, w, axis=0), axis=-1)
    return fn(np.lib.stride_tricks.sliding_window_view(p, w, axis=1), axis=-1)


def stretch(a):
    lo, hi = np.percentile(a, 0.5), np.median(a)
    return np.clip((a - lo) * 255.0 / max(hi - lo, 1), 0, 255)


def lstretch(a, w=61, floor=24):
    hi, lo = box_rank(a, w, np.max), box_rank(a, w, np.min)
    rng = hi - lo
    return np.where(rng < floor, 255.0, np.clip((a - lo) * 255.0 / np.maximum(rng, 1), 0, 255))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--font", required=True, help="a TrueType face, e.g. Liberation Sans Regular")
    ap.add_argument("--out", default="showthrough_lines")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    font = ImageFont.truetype(args.font, PX)
    bk = back(font)
    pres = {"none": lambda x: x, "stretch": stretch, "lstretch": lstretch}
    for f in FRONTS:
        fr = front(font, f)
        for b in BACKS:
            a = 255.0 * fr * (1.0 - bk * (255 - b) / 255.0)
            for pre, fn in pres.items():
                g = np.clip(np.rint(fn(a)), 0, 255).astype(np.uint8)
                name = f"F{f:03d}_B{b:03d}__{pre}.pgm"
                with open(os.path.join(args.out, name), "wb") as fh:
                    fh.write(f"P5 {g.shape[1]} {g.shape[0]} 255\n".encode() + g.tobytes())


if __name__ == "__main__":
    main()
