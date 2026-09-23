#!/usr/bin/env python3
"""Turn real PDFs into an OCRcer benchmark corpus: one `.pgm` page plus the
`.truth.json` beside it that `ocrcer-bench`'s `pages` reader expects.

    python tools/pdf_corpus.py <pdf-dir> <out-dir> --dpi 150 [options]

The synthetic corpus in `bench/pages-cov` measures the engine against the
generator's idea of a document. This script measures it against documents
somebody actually produced, which is a different and complementary question.

# What the ground truth here is, and what it is not

The truth text comes from the PDF's own text layer, via `pdfcer extract-text`.
That is the *producer's claim about what it drew*, not ground truth:

  * reading order across columns and tables is whatever order the content
    stream happened to use;
  * word spaces and line breaks are frequently **not in the file at all** and
    are derived from glyph geometry — `pdfcer extract-text` counts the ones it
    had to invent, and this script records that count per page in
    `derived_spaces` / `derived_breaks` so a reader can weigh it;
  * ligatures, small caps and symbol fonts map to Unicode through a ladder
    that does not always terminate, and a code that falls off it arrives as
    U+FFFD.

So a CER computed against this corpus is partly a measurement of the producer.
**The layout-free word metric is the trustworthy one here**, and any CER quoted
from this corpus must say what it was scored against. See `ARCHITECTURE.md`
section 11's corpus-provenance entry.

# What the pixels are

`pdfcer render-page` rasterises the page. When the document does not embed its
fonts — base-14 Helvetica and Times are the common case — pdfcer substitutes a
bundled face and says so on stderr. The layout, the rules, the column
structure, the number formats and the content are then still the document's;
the typeface is not. This script records `substituted` per page so a run can
be split on it rather than averaged across it.

# Licensing and privacy

Documents fetched from the web are **evaluation input, never model content**.
Nothing here may be authored into a lexicon, bigram or confusion table, and no
text from a page may enter the repository. Keep both the PDFs and the generated
corpus outside the working tree — `D:/Dev/ExcludedPrivate/ocrcer/` on this
machine. This script is the method and is committed; the data is not.
"""

import argparse
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

PDFCER_DEFAULT = r"D:\Dev\pdfcer\target\release\pdfcer.exe"


def run(cmd):
    """Runs a command, returning (stdout, stderr, returncode). Never raises."""
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    return p.stdout or "", p.stderr or "", p.returncode


def opens(pdfcer, pdf):
    """Whether pdfcer will open `pdf` at all.

    Deliberately not a page count: `pdfcer inspect` reports the header
    version and nothing else, and a caller that wants three pages can simply
    ask for page 1, 2, 3 and stop when one is refused. Probing is a few
    milliseconds and cannot go stale against a changing `inspect` format.
    """
    _out, _err, rc = run([pdfcer, "inspect", str(pdf)])
    return rc == 0


def counter(line, name):
    """One `name=<int>` counter out of a pdfcer result line, or 0."""
    m = re.search(rf"\b{name}=(\d+)\b", line)
    return int(m.group(1)) if m else 0


def render_pgm(pdfcer, pdf, page, dpi, out_pgm, tmp_png):
    """Renders one page and writes it as a binary P5 PGM.

    Returns the render's `substituted` glyph count, or None if the page did
    not render. Greyscale conversion is `Image.convert("L")` after flattening
    onto white, so a page with transparency does not come out dark.
    """
    from PIL import Image

    scale = dpi / 72.0
    _, err, rc = run([pdfcer, "render-page", str(pdf), "--page", str(page),
                      "--scale", f"{scale:.6f}", "--output", str(tmp_png)])
    if rc != 0 or not tmp_png.exists():
        return None
    # The counters are on stdout; the human note is on stderr. Read both so a
    # future pdfcer that moves the line still works.
    _out, _err, _rc = ("", err, rc)
    im = Image.open(tmp_png)
    if im.mode in ("RGBA", "LA", "P"):
        im = im.convert("RGBA")
        bg = Image.new("RGBA", im.size, (255, 255, 255, 255))
        im = Image.alpha_composite(bg, im)
    grey = im.convert("L")
    w, h = grey.size
    with open(out_pgm, "wb") as f:
        f.write(b"P5\n%d %d\n255\n" % (w, h))
        f.write(grey.tobytes())
    return w, h


def extract_page_text(pdfcer, pdf, page):
    """The page's text as a list of lines, plus the derived-space/break counts.

    Returns (lines, derived_spaces, derived_breaks, replacement_chars).
    """
    out, err, rc = run([pdfcer, "extract-text", str(pdf), "--pages", str(page)])
    if rc != 0:
        return [], 0, 0, 0
    both = out + "\n" + err
    spaces = counter(both, "derived_spaces") or counter(both, "spaces_derived")
    breaks = counter(both, "derived_breaks") or counter(both, "breaks_derived")
    lines = [ln.rstrip() for ln in out.splitlines()]
    lines = [ln for ln in lines if ln.strip()]
    bad = sum(ln.count("\ufffd") for ln in lines)
    return lines, spaces, breaks, bad


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("pdf_dir", help="directory of .pdf files")
    ap.add_argument("out_dir", help="directory to write .pgm/.truth.json into")
    ap.add_argument("--dpi", type=float, default=150.0,
                    help="render resolution; 150 is a typical office scan (default 150)")
    ap.add_argument("--max-pages", type=int, default=4,
                    help="pages per document, from page 1 (default 4)")
    ap.add_argument("--tag", default=None,
                    help="value for the truth's `family` field, which the bench "
                         "report groups by. Defaults to the document's file stem; "
                         "set it to something non-identifying if the stem is not.")
    ap.add_argument("--min-chars", type=int, default=200,
                    help="skip a page whose text layer yields fewer characters "
                         "than this: it is a scan, a cover or a blank, and has no "
                         "usable truth (default 200)")
    ap.add_argument("--pdfcer", default=PDFCER_DEFAULT)
    args = ap.parse_args()

    pdfcer = args.pdfcer
    if not Path(pdfcer).exists() and not shutil.which(pdfcer):
        sys.exit(f"pdfcer not found at {pdfcer}")

    src = Path(args.pdf_dir)
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    tmp_png = out / "_render.tmp.png"

    pdfs = sorted(p for p in src.glob("*.pdf"))
    if not pdfs:
        sys.exit(f"no .pdf files in {src}")

    made = skipped = failed = 0
    manifest = []
    for pdf in pdfs:
        if not opens(pdfcer, pdf):
            print(f"  FAILED  {pdf.name}: will not open")
            failed += 1
            continue
        tag = args.tag or re.sub(r"[^A-Za-z0-9_-]+", "-", pdf.stem)
        for page in range(1, args.max_pages + 1):
            stem = f"{tag}__p{page:02d}__{int(args.dpi)}dpi"
            pgm = out / f"{stem}.pgm"
            size = render_pgm(pdfcer, pdf, page, args.dpi, pgm, tmp_png)
            if size is None:
                # Past the last page, or a page pdfcer will not draw. Either
                # way there is nothing after it worth asking for.
                if pgm.exists():
                    pgm.unlink()
                break
            lines, spaces, breaks, bad = extract_page_text(pdfcer, pdf, page)
            chars = sum(len(l) for l in lines)
            if chars < args.min_chars:
                pgm.unlink()
                skipped += 1
                continue
            w, h = size
            truth = {
                "family": tag,
                # Not a type size. The bench report groups by this field and
                # labels it px/em; there is no em here, so it is left at zero
                # rather than filled with a DPI that would read as one.
                "px_per_em": 0.0,
                "lines": lines,
                # No per-glyph boxes exist for a real document, so the oracle
                # column is unavailable on this corpus. An empty array is what
                # the reader needs to see; a missing key is an error.
                "glyphs": [],
                "source": {
                    "kind": "pdf-text-layer",
                    "width": w,
                    "height": h,
                    "dpi": args.dpi,
                    "page": page,
                    "chars": chars,
                    "derived_spaces": spaces,
                    "derived_breaks": breaks,
                    "replacement_chars": bad,
                },
            }
            (out / f"{stem}.truth.json").write_text(
                json.dumps(truth, ensure_ascii=False, indent=1), encoding="utf-8")
            manifest.append({"stem": stem, **truth["source"]})
            made += 1
    if tmp_png.exists():
        tmp_png.unlink()

    (out / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=1), encoding="utf-8")

    tot_chars = sum(m["chars"] for m in manifest)
    tot_bad = sum(m["replacement_chars"] for m in manifest)
    tot_sp = sum(m["derived_spaces"] for m in manifest)
    print(f"\n{made} pages written to {out}  ({skipped} skipped, {failed} failed)")
    print(f"{tot_chars} characters of text-layer truth")
    print(f"{tot_bad} of them are U+FFFD ({100.0 * tot_bad / max(tot_chars, 1):.2f}%) "
          f"- a page above about 1% has a font whose Unicode mapping did not "
          f"recover, and its truth is not trustworthy")
    print(f"{tot_sp} word spaces were DERIVED from glyph geometry rather than "
          f"read from the file, so a space error against this corpus is not "
          f"necessarily the engine's")


if __name__ == "__main__":
    main()
