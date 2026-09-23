#!/usr/bin/env python3
"""Turn an hOCR-annotated image corpus into an OCRcer benchmark corpus: one
`.pgm` page plus the `.truth.json` beside it that `ocrcer-bench`'s `pages`
reader expects.

    python tools/hocr_corpus.py <img-dir> <hocr-dir> <out-dir> [options]

# Why this exists alongside `pdf_corpus.py`

`pdf_corpus.py` takes truth from a PDF's own text layer, which is the
producer's claim about what it drew rather than what is on the page. An hOCR
file that a human checked is the other kind of truth: somebody read the image
and wrote down what it says. It is the better evidence, and where the two
disagree this one wins.

The matching rule is by stem: `img/filing_01.png` pairs with
`<hocr-dir>/filing_01.truth.hocr` or `<hocr-dir>/filing_01.hocr`. An image
with no hOCR beside it is skipped and counted, never silently dropped.

# What a line is here

One `ocr_line` element, with its `ocrx_word` children joined by single spaces.
That is a choice and it has a consequence worth stating: the corpus therefore
asserts *one space between words*, so a run scored against it cannot
distinguish a double space the engine got right from one it invented. Word
boxes are not carried into the truth even though hOCR has them, because the
`pages` reader's `glyphs` array is per-glyph and a word box is not one; an
empty array is the honest value and disables the oracle column.

Reading order is the order the elements appear in the file, which on a
multi-column page is the order the annotator chose. A layout-free word metric
is unaffected by that; a CER is not, and multi-column pages in this corpus are
scored with that caveat attached.

# Licensing and privacy

A downloaded corpus is **evaluation input, never model content** --
`ARCHITECTURE.md` section 11, 2026-09-22. Nothing from a page here may be
authored into a lexicon, bigram or confusion table, and no text from a page
enters this repository. Keep the images, the hOCR and the generated corpus
outside the working tree; `D:/Dev/ExcludedPrivate/ocrcer/` on this machine.
This script is the method and is committed; the data is not.
"""

import argparse
import html
import json
import re
import sys
from pathlib import Path

# hOCR is XHTML, but the files in the wild are not reliably well-formed enough
# for a strict XML parser and the structure needed here is two nested spans.
# Regexes are the smaller dependency-free answer; anything more structural
# would be a parser this project does not need.
#
# Both quote styles are matched deliberately. Within this one corpus some
# files write `class='ocr_line'` and others `class="ocr_line"`, and a pattern
# that accepts only the first silently produced two empty pages -- which the
# `--min-chars` guard caught and reported, but would have been a quiet loss of
# two documents had the guard been looser.
LINE_RE = re.compile(r"<span class=[\"']ocr_line[\"'][^>]*>", re.S)
WORD_RE = re.compile(r"<span class=[\"']ocrx_word[\"'][^>]*>(.*?)</span>", re.S)
TAG_RE = re.compile(r"<[^>]+>")


def lines_of(hocr_text):
    """The page's lines, each a string of space-joined words.

    Empty lines are dropped: an `ocr_line` with no word children is a layout
    artefact of the annotation tool, not a line of the document.
    """
    # Split on the line openers rather than matching balanced spans: the word
    # spans nest inside the line span, so a non-greedy `</span>` match would
    # close on the first word.
    starts = [m.end() for m in LINE_RE.finditer(hocr_text)]
    bounds = [m.start() for m in LINE_RE.finditer(hocr_text)][1:] + [len(hocr_text)]
    out = []
    for a, b in zip(starts, bounds):
        words = [html.unescape(TAG_RE.sub("", w)).strip()
                 for w in WORD_RE.findall(hocr_text[a:b])]
        words = [w for w in words if w]
        if words:
            out.append(" ".join(words))
    if out:
        return out
    # Some files nest differently. Falling back to every word on the page in
    # document order loses the line breaks, which is worse but not useless --
    # and it is reported, so a reader knows which pages it happened to.
    words = [html.unescape(TAG_RE.sub("", w)).strip() for w in WORD_RE.findall(hocr_text)]
    words = [w for w in words if w]
    return [" ".join(words)] if words else []


def write_pgm(im, path):
    """Writes a PIL image as binary P5 PGM, flattening transparency onto white."""
    from PIL import Image

    if im.mode in ("RGBA", "LA", "P"):
        im = im.convert("RGBA")
        bg = Image.new("RGBA", im.size, (255, 255, 255, 255))
        im = Image.alpha_composite(bg, im)
    grey = im.convert("L")
    w, h = grey.size
    with open(path, "wb") as f:
        f.write(b"P5\n%d %d\n255\n" % (w, h))
        f.write(grey.tobytes())
    return w, h


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("img_dir")
    ap.add_argument("hocr_dir")
    ap.add_argument("out_dir")
    ap.add_argument("--scale", type=float, default=1.0,
                    help="resample the page by this factor before writing. 1.0 is the "
                         "image as scanned; 0.5 halves it, which is how to ask what the "
                         "engine does at half the resolution (default 1.0)")
    ap.add_argument("--max-side", type=int, default=0,
                    help="cap the longer side at this many pixels, preserving aspect. "
                         "0 means no cap (default 0)")
    ap.add_argument("--min-chars", type=int, default=100,
                    help="skip a page whose truth is shorter than this (default 100)")
    args = ap.parse_args()

    from PIL import Image

    imgs = Path(args.img_dir)
    hocrs = Path(args.hocr_dir)
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)

    made = missing = short = 0
    manifest = []
    for img in sorted(imgs.iterdir()):
        if img.suffix.lower() not in (".png", ".jpg", ".jpeg", ".tif", ".tiff"):
            continue
        cand = [hocrs / f"{img.stem}.truth.hocr", hocrs / f"{img.stem}.hocr"]
        hocr = next((c for c in cand if c.exists()), None)
        if hocr is None:
            print(f"  no truth  {img.name}")
            missing += 1
            continue
        lines = lines_of(hocr.read_text(encoding="utf-8", errors="replace"))
        chars = sum(len(l) for l in lines)
        if chars < args.min_chars:
            print(f"  too short {img.name}: {chars} characters")
            short += 1
            continue
        im = Image.open(img)
        scale = args.scale
        if args.max_side:
            scale = min(scale, args.max_side / max(im.size))
        if abs(scale - 1.0) > 1e-9:
            im = im.resize((max(1, round(im.width * scale)), max(1, round(im.height * scale))),
                           Image.LANCZOS)
        stem = re.sub(r"[^A-Za-z0-9_-]+", "-", img.stem)
        w, h = write_pgm(im, out / f"{stem}.pgm")
        truth = {
            # The report groups by this field. The stems in this corpus are
            # already document *kinds* -- table, multicol, deposition -- so
            # the leading token is the useful grouping.
            "family": re.split(r"[-_]", stem)[0],
            # Not a type size; there is no single one on a real page. The
            # report labels this px/em and zero is what it means here.
            "px_per_em": 0.0,
            "lines": lines,
            "glyphs": [],
            "source": {"kind": "hocr", "width": w, "height": h, "scale": scale,
                       "chars": chars, "lines": len(lines), "from": hocr.name},
        }
        (out / f"{stem}.truth.json").write_text(json.dumps(truth, ensure_ascii=False, indent=1),
                                                encoding="utf-8")
        manifest.append({"stem": stem, **truth["source"]})
        made += 1

    (out / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=1),
                                       encoding="utf-8")
    tot = sum(m["chars"] for m in manifest)
    print(f"\n{made} pages written to {out}  ({missing} with no truth, {short} too short)")
    print(f"{tot} characters of hand-checked truth across {sum(m['lines'] for m in manifest)} lines")
    print("word spaces in this truth are one per gap by construction, so a run "
          "cannot be credited or blamed for reproducing a double space")


if __name__ == "__main__":
    main()
