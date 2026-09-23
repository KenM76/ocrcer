#!/usr/bin/env python3
"""Turn a Hugging Face image+text parquet dataset into an OCRcer benchmark
corpus: one `.pgm` page plus the `.truth.json` beside it that `ocrcer-bench`'s
`pages` reader expects.

    python tools/parquet_corpus.py <parquet-glob> <out-dir> [options]

# Why this exists alongside `pdf_corpus.py` and `hocr_corpus.py`

Three corpora, three kinds of truth, and they are not interchangeable:

  * `hocr_corpus.py` — a human read the image and wrote down what it says.
    The best evidence available, and the smallest corpus.
  * `pdf_corpus.py` — the PDF's own text layer: the producer's claim about
    what it drew.
  * this one — a dataset's paired transcript, which is usually the *source*
    the page was rendered from (SEC filing HTML, in the case that motivated
    this script). That makes the characters trustworthy and the **line breaks
    not**: source wrapping is not render wrapping, and a transcript may carry
    document furniture that was never drawn on the page.

So on a corpus built by this script the **layout-free word metric is the
trustworthy one**, and any CER quoted from it must say so. Same caveat as the
PDF-text-layer corpus, for a different reason. See `ARCHITECTURE.md` section
11's corpus-provenance entry.

# Image columns

Two encodings are handled, because the datasets in the wild use both:

  * a plain string column holding base64 PNG/JPEG bytes;
  * a struct column `{bytes, path}`, which is the `datasets` library's own
    `Image` feature when a parquet is written without decoding.

`--image-col` and `--text-col` name the columns; the defaults suit a dataset
whose columns are literally `image` and `text`.

# Licensing and privacy

A downloaded corpus is **evaluation input, never model content** --
`ARCHITECTURE.md` section 11, 2026-09-22. Nothing from a page here may be
authored into a lexicon, bigram or confusion table, and no text from a page
enters this repository. Keep the parquet files and the generated corpus
outside the working tree; `D:/Dev/ExcludedPrivate/ocrcer/` on this machine.
This script is the method and is committed; the data is not.

# Where the truth's LINE BREAKS come from: --text-mode

Three shapes of truth, and only the first arrives with line breaks in it:

  * `--text-mode column` (default) reads `--text-col` as a transcript and
    splits it on newlines. The breaks are the transcript's, which on a
    source-derived dataset are the source's wrapping and not the page's.
  * `--text-mode word-list` reads `--text-col` as a list of word strings
    (SROIE's `words`) and needs `--box-col` to recover rows; without boxes it
    writes ONE line per page, which leaves the word metric meaningful and the
    CER not.
  * `--text-mode cord` parses CORD-v2's `ground_truth` JSON. Its `valid_line`
    entries are *fields*, not visual lines -- `menu.nm`, `menu.cnt` and
    `menu.price` off one row of a receipt are three separate entries -- so
    taking one truth line per entry would fabricate breaks the page does not
    have. Rows are recovered from the word quads instead.

Row recovery, where boxes exist, is one deterministic rule: sort words by the
top of their box; start a new row when a word's vertical centre falls outside
the current row's accumulated y-range; sort each row by left edge. The
resulting truth records `"kind": "parquet-boxes"` so a report can say the line
structure was RECONSTRUCTED from word boxes rather than given by the dataset.
That distinction matters for CER, which is a statement about reading order.

# Scope note

Photographed receipts (SROIE, CORD) are closer to scene text than to printed
documents, and OCRcer's domain is printed documents and CAD drawing text
(`FEASIBILITY.md` section 6). A loss on such a corpus is an accepted outcome,
not a defect -- but only if the corpus is labelled as out-of-domain when the
result is reported. This script will convert them; the report has to say what
they are.
"""

import argparse
import base64
import binascii
import glob
import io
import json
import re
import sys
from pathlib import Path


def image_bytes(cell):
    """Raw encoded image bytes out of whichever column shape the dataset used.

    Returns None when the cell holds neither, which is reported rather than
    crashing the run: one unreadable row in ten thousand should not cost the
    other 9,999.
    """
    if isinstance(cell, (bytes, bytearray)):
        return bytes(cell)
    if isinstance(cell, dict):
        b = cell.get("bytes")
        return bytes(b) if b else None
    if isinstance(cell, str):
        try:
            return base64.b64decode(cell, validate=False)
        except (binascii.Error, ValueError):
            return None
    return None


def clean_lines(text, drop_leading):
    """The transcript as a list of non-empty lines, whitespace-collapsed.

    Runs of spaces collapse to one because the truth asserts one space per
    gap (the same construction `hocr_corpus.py` documents), and a transcript
    that happens to carry column padding would otherwise charge the engine
    for spaces it was right not to emit.
    """
    lines = [re.sub(r"[ \t\u00a0]+", " ", ln).strip() for ln in text.splitlines()]
    lines = [ln for ln in lines if ln]
    return lines[drop_leading:] if drop_leading else lines


def rows_from_boxes(words, boxes):
    """Visual rows out of word strings and their boxes, as a list of lines.

    `boxes` may be [x0, y0, x1, y1] or a CORD quad dict; anything without four
    usable numbers drops the word rather than the page. One rule, applied in
    order: sort by box top, break a row when a word's vertical centre leaves
    the row's accumulated y-range, sort within the row by left edge.

    The y-range accumulates rather than being fixed by the first word, so a
    row led by a comma still admits the capital beside it.
    """
    items = []
    for w, b in zip(words, boxes or []):
        y = box_bounds(b)
        if w and y:
            items.append((w, y))
    if not items:
        return [" ".join(w for w in words if w)] if any(words) else []
    items.sort(key=lambda it: (it[1][1], it[1][0]))
    rows, cur, lo, hi = [], [], None, None
    for w, (x0, y0, x1, y1) in items:
        mid = (y0 + y1) / 2.0
        if cur and not (lo <= mid <= hi):
            rows.append(cur)
            cur, lo, hi = [], None, None
        cur.append((x0, w))
        lo = y0 if lo is None else min(lo, y0)
        hi = y1 if hi is None else max(hi, y1)
    if cur:
        rows.append(cur)
    return [" ".join(w for _, w in sorted(r)) for r in rows]


def box_bounds(b):
    """(x0, y0, x1, y1) out of a flat list, a nested list, or a CORD quad."""
    if isinstance(b, dict):
        xs = [b[k] for k in ("x1", "x2", "x3", "x4") if k in b]
        ys = [b[k] for k in ("y1", "y2", "y3", "y4") if k in b]
        if len(xs) >= 2 and len(ys) >= 2:
            return (min(xs), min(ys), max(xs), max(ys))
        return None
    if isinstance(b, (list, tuple)):
        flat = []
        for v in b:
            if isinstance(v, (list, tuple)):
                flat.extend(v)
            else:
                flat.append(v)
        nums = [v for v in flat if isinstance(v, (int, float))]
        if len(nums) == 4:
            x0, y0, x1, y1 = nums
            return (min(x0, x1), min(y0, y1), max(x0, x1), max(y0, y1))
        if len(nums) >= 8:
            xs, ys = nums[0::2], nums[1::2]
            return (min(xs), min(ys), max(xs), max(ys))
    return None


def cord_rows(raw):
    """CORD-v2 `ground_truth` JSON to visual rows, via its word quads."""
    try:
        g = json.loads(raw) if isinstance(raw, str) else (raw or {})
    except (ValueError, TypeError):
        return []
    words, boxes = [], []
    for line in g.get("valid_line", []):
        for w in line.get("words", []):
            t = (w.get("text") or "").strip()
            if t:
                words.append(t)
                boxes.append(w.get("quad"))
    return rows_from_boxes(words, boxes)


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
    ap.add_argument("parquet", help="a .parquet file, a directory of them, or a glob")
    ap.add_argument("out_dir")
    ap.add_argument("--image-col", default="image")
    ap.add_argument("--text-col", default="text")
    ap.add_argument("--family", default="doc",
                    help="value for the truth's `family` field, which the bench report "
                         "groups by. A dataset of one document kind wants one name "
                         "(default 'doc')")
    ap.add_argument("--limit", type=int, default=200,
                    help="stop after this many pages. These datasets run to thousands "
                         "of pages and a benchmark does not need them all (default 200)")
    ap.add_argument("--stride", type=int, default=1,
                    help="take every Nth row, so a small sample is spread across the "
                         "shard rather than taken from its head (default 1)")
    ap.add_argument("--scale", type=float, default=1.0,
                    help="resample the page by this factor. The dataset's own "
                         "resolution is a choice somebody else made; this is how to "
                         "ask what the engine does at another one (default 1.0)")
    ap.add_argument("--max-side", type=int, default=0,
                    help="cap the longer side at this many pixels, preserving aspect. "
                         "0 means no cap (default 0)")
    ap.add_argument("--text-mode", default="column",
                    choices=["column", "word-list", "cord"],
                    help="how to read the truth. `column` splits --text-col on "
                         "newlines; `word-list` treats it as a list of words and uses "
                         "--box-col to recover rows; `cord` parses CORD-v2 "
                         "ground_truth JSON (default column)")
    ap.add_argument("--box-col", default=None,
                    help="column of per-word boxes, used by --text-mode word-list to "
                         "recover visual rows. Without it the page becomes one line and "
                         "only the word metric is meaningful")
    ap.add_argument("--min-chars", type=int, default=200,
                    help="skip a page whose transcript is shorter than this (default 200)")
    ap.add_argument("--drop-leading-lines", type=int, default=0,
                    help="drop this many lines from the top of each transcript. Some "
                         "datasets prefix a filing header that was never drawn on the "
                         "page; dropping it is honest, guessing is not (default 0)")
    args = ap.parse_args()

    import pyarrow.parquet as pq
    from PIL import Image

    src = Path(args.parquet)
    if src.is_dir():
        files = sorted(src.glob("*.parquet"))
    else:
        files = sorted(Path(p) for p in glob.glob(args.parquet))
    if not files:
        sys.exit(f"no parquet files matched {args.parquet}")

    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)

    made = short = bad = 0
    row = 0
    manifest = []
    for pf in files:
        if made >= args.limit:
            break
        reader = pq.ParquetFile(pf)
        want = [args.image_col, args.text_col]
        if args.box_col:
            want.append(args.box_col)
        for batch in reader.iter_batches(batch_size=16, columns=want):
            cols = batch.to_pydict()
            boxcol = cols.get(args.box_col) if args.box_col else None
            if boxcol is None:
                boxcol = [None] * len(cols[args.image_col])
            for cell, text, bxs in zip(cols[args.image_col], cols[args.text_col], boxcol):
                idx = row
                row += 1
                if made >= args.limit:
                    break
                if args.stride > 1 and idx % args.stride:
                    continue
                if args.text_mode == "cord":
                    lines = cord_rows(text)
                elif args.text_mode == "word-list":
                    lines = rows_from_boxes(list(text or []), bxs)
                else:
                    lines = clean_lines(text or "", args.drop_leading_lines)
                if args.text_mode != "column":
                    lines = clean_lines("\n".join(lines), args.drop_leading_lines)
                chars = sum(len(l) for l in lines)
                if chars < args.min_chars:
                    short += 1
                    continue
                raw = image_bytes(cell)
                if not raw:
                    bad += 1
                    continue
                try:
                    im = Image.open(io.BytesIO(raw))
                    im.load()
                except Exception:
                    bad += 1
                    continue
                scale = args.scale
                if args.max_side:
                    scale = min(scale, args.max_side / max(im.size))
                if abs(scale - 1.0) > 1e-9:
                    im = im.resize((max(1, round(im.width * scale)),
                                    max(1, round(im.height * scale))), Image.LANCZOS)
                stem = f"{args.family}__r{idx:06d}"
                w, h = write_pgm(im, out / f"{stem}.pgm")
                truth = {
                    "family": args.family,
                    # Not a type size; there is no single one on a real page.
                    # The report labels this px/em and zero is what it means.
                    "px_per_em": 0.0,
                    "lines": lines,
                    # No per-glyph boxes exist, so the oracle column is
                    # unavailable. An empty array is what the reader needs to
                    # see; a missing key is an error.
                    "glyphs": [],
                    "source": {"kind": "parquet-transcript" if args.text_mode == "column"
                                       else "parquet-boxes",
                               "width": w, "height": h,
                               "scale": scale, "chars": chars, "lines": len(lines),
                               "from": pf.name, "row": idx},
                }
                (out / f"{stem}.truth.json").write_text(
                    json.dumps(truth, ensure_ascii=False, indent=1), encoding="utf-8")
                manifest.append({"stem": stem, **truth["source"]})
                made += 1
            if made >= args.limit:
                break

    (out / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=1),
                                       encoding="utf-8")
    tot = sum(m["chars"] for m in manifest)
    px = sorted({(m["width"], m["height"]) for m in manifest})
    print(f"\n{made} pages written to {out}  ({short} too short, {bad} unreadable)")
    print(f"{tot} characters of transcript truth across "
          f"{sum(m['lines'] for m in manifest)} lines")
    print(f"page sizes present: {px[:5]}{' ...' if len(px) > 5 else ''}")
    if args.text_mode == "column":
        print("this truth is a TRANSCRIPT, not a reading of the image: the characters "
              "are trustworthy, the line breaks are source wrapping rather than render "
              "wrapping, so quote the layout-free word metric from this corpus and say "
              "what the CER was scored against")
    else:
        print("this truth's WORDS are the dataset's annotation of the image and its "
              "LINE BREAKS were reconstructed here from the word boxes, by one rule "
              "(sort by box top, break when a word's vertical centre leaves the row's "
              "accumulated y-range, sort within the row by left edge). Any CER from "
              "this corpus is scored against that reconstruction, not against a "
              "reading of the page, and has to say so")


if __name__ == "__main__":
    main()
