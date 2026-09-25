"""Digit spacing census: for each shippable face in fonts.tsv, can a gap test tell
two adjacent digits from two digits with a space between them?

Reads font metrics only (hmtx advance, outline bounds); no kerning, no rendering.
Output columns, all in em:
  digits      tab if all ten digits share one advance, else prop
  ink_ratio   largest ink gap between adjacent digits / smallest ink gap across a space
  ink_pair    the adjacent pair with the largest ink gap
  ctr_ratio   largest centre distance between adjacent digits / smallest across a space
A ratio >= 1 means no single threshold separates every pair on a perfect render.
"""
import argparse
import csv
import os

from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont

DIGITS = "0123456789"


def face_row(path):
    f = TTFont(path, fontNumber=0)
    cmap, gs, hm = f.getBestCmap(), f.getGlyphSet(), f["hmtx"].metrics
    upm = f["head"].unitsPerEm
    adv, lsb, rsb, cx = {}, {}, {}, {}
    for c in DIGITS:
        g = cmap.get(ord(c))
        if g is None:
            return None
        bp = BoundsPen(gs)
        gs[g].draw(bp)
        x0, _, x1, _ = bp.bounds
        adv[c], lsb[c], rsb[c], cx[c] = hm[g][0], x0, hm[g][0] - x1, (x0 + x1) / 2
    if 32 not in cmap:
        return None
    sp = hm[cmap[32]][0]
    pairs = [(a, b) for a in DIGITS for b in DIGITS]
    ink = {(a, b): rsb[a] + lsb[b] for a, b in pairs}
    ctr = {(a, b): adv[a] - cx[a] + cx[b] for a, b in pairs}
    worst = max(ink, key=ink.get)
    return {
        "digits": "tab" if len(set(adv.values())) == 1 else "prop",
        "adv": round(sorted(adv.values())[5] / upm, 3),
        "space": round(sp / upm, 3),
        "ink_ratio": round(ink[worst] / (min(ink.values()) + sp), 2),
        "ink_pair": "".join(worst),
        "ctr_ratio": round(max(ctr.values()) / (min(ctr.values()) + sp), 2),
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--fonts", default="model/fonts.tsv")
    args = ap.parse_args()
    cols = ["digits", "adv", "space", "ink_ratio", "ink_pair", "ctr_ratio"]
    print("\t".join(["family", "style"] + cols))
    with open(args.fonts, encoding="utf-8") as fh:
        for r in csv.reader(fh, delimiter="\t"):
            if not r or r[0].startswith("#") or r[5] != "shippable":
                continue
            path = r[6].replace("\\", "/")
            row = face_row(path) if os.path.exists(path) else None
            vals = [str(row[c]) for c in cols] if row else ["missing"]
            print("\t".join([r[0], r[1]] + vals))


if __name__ == "__main__":
    main()
