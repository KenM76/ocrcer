#!/usr/bin/env python3
"""Render the committed `train`/`validation` MultiFinBen rows from
`bench/splits/manifest.tsv` into pgm+truth.json pages, by the same method
`pages/finfilings` used.

    python tools/manifest_render.py <manifest.tsv> <dataset-dir> <split> <out-dir>
    python tools/manifest_render.py <manifest.tsv> <dataset-dir> <split> <out-dir> --check

Imports `image_bytes`/`clean_lines`/`write_pgm` from `parquet_corpus.py`
rather than copying them, so a manifest-driven render and the original
stride-driven `pages/finfilings` render can never silently diverge in what
counts as "the same page" -- one function, two call sites, same as
`ocrcer-core` rule 4's reasoning applied to this one-off tooling.

`--check` renders nothing and reads no parquet data; it only compares the
stems the manifest currently implies against the `.pgm` files already on
disk in `<out-dir>`, and fails (nonzero exit) listing any mismatch. This is
what to run after a manifest regeneration removes or adds rows, before
trusting that `<out-dir>` still matches `manifest.tsv` exactly.

# Licensing and privacy

Same rule as `parquet_corpus.py`: the rendered pages are training/scoring
input, never committed. Keep them under `D:/Dev/ExcludedPrivate/ocrcer/`.
This script is the method and is committed; the data is not.

# Stems

`<family>__s<shard-index>__r<row:06d>` -- `pages/finfilings` needed no shard
marker because it only ever drew from shard 0 of the 8; a manifest-driven
train/validation render pulls from all 8, and MultiFinBen's row numbers
repeat per shard, so the shard index disambiguates.
"""

import argparse
import io
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import parquet_corpus as pc  # noqa: E402


def shard_index(shard_name):
    m = re.search(r"-(\d+)-of-\d+", shard_name)
    return int(m.group(1)) if m else 0


def load_wanted(manifest_tsv, dataset, split):
    """{shard filename: {row numbers}} for every manifest row matching
    `dataset` and `split`. Malformed lines are a hard error -- a manifest
    that fails to parse is a manifest that must not be rendered from."""
    wanted = {}
    with open(manifest_tsv, encoding="utf-8") as f:
        header = f.readline()
        if not header.startswith("#dataset\trow_id\tsplit\tlicence\treason"):
            sys.exit(f"{manifest_tsv}: unexpected header {header!r}")
        for lineno, line in enumerate(f, start=2):
            line = line.rstrip("\n")
            if not line:
                continue
            parts = line.split("\t")
            if len(parts) != 5:
                sys.exit(f"{manifest_tsv}:{lineno}: expected 5 tab-separated fields, "
                          f"got {len(parts)}")
            ds, row_id, sp, _licence, _reason = parts
            if ds != dataset or sp != split:
                continue
            shard, row = row_id.split("#")
            wanted.setdefault(shard, set()).add(int(row))
    return wanted


def expected_stems(wanted, family):
    stems = set()
    for shard, rows in wanted.items():
        sidx = shard_index(shard)
        for row in rows:
            stems.add(f"{family}__s{sidx}__r{row:06d}")
    return stems


def cmd_check(args):
    wanted = load_wanted(args.manifest_tsv, args.dataset, args.split)
    want_total = sum(len(v) for v in wanted.values())
    if not wanted:
        sys.exit(f"no {args.split} rows for {args.dataset} in {args.manifest_tsv}")

    expected = expected_stems(wanted, args.family)
    out = Path(args.out_dir)
    present = {p.stem for p in out.glob(f"{args.family}__s*__r*.pgm")}

    missing = sorted(expected - present)
    extra = sorted(present - expected)

    if not missing and not extra:
        print(f"--check: {out} matches {args.manifest_tsv} exactly "
              f"({want_total} {args.split} rows)")
        return

    if missing:
        print(f"--check: {len(missing)} row(s) in the manifest have no rendered page:")
        for s in missing[:20]:
            print(f"  missing: {s}")
        if len(missing) > 20:
            print(f"  ... and {len(missing) - 20} more")
    if extra:
        print(f"--check: {len(extra)} rendered page(s) are not in the manifest:")
        for s in extra[:20]:
            print(f"  extra:   {s}")
        if len(extra) > 20:
            print(f"  ... and {len(extra) - 20} more")
    sys.exit(1)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("manifest_tsv", help="bench/splits/manifest.tsv")
    ap.add_argument("dataset_dir", help=".../datasets/multifinben-englishocr")
    ap.add_argument("split", choices=["train", "validation"])
    ap.add_argument("out_dir")
    ap.add_argument("--dataset", default="multifinben-englishocr",
                    help="must match the manifest's dataset column (default "
                         "multifinben-englishocr)")
    ap.add_argument("--family", default="filing",
                    help="truth `family` field and stem prefix, matching "
                         "pages/finfilings (default 'filing')")
    ap.add_argument("--check", action="store_true",
                    help="compare manifest-implied stems against <out-dir> "
                         "contents; render nothing, read no parquet data")
    args = ap.parse_args()

    if args.check:
        cmd_check(args)
        return

    import pyarrow.parquet as pq
    from PIL import Image

    wanted = load_wanted(args.manifest_tsv, args.dataset, args.split)
    want_total = sum(len(v) for v in wanted.values())
    if not wanted:
        sys.exit(f"no {args.split} rows for {args.dataset} in {args.manifest_tsv}")

    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    src = Path(args.dataset_dir) / "data"

    made = bad = 0
    manifest = []
    for shard in sorted(wanted):
        rows = wanted[shard]
        pf = src / shard
        if not pf.exists():
            sys.exit(f"missing shard file {pf}")
        sidx = shard_index(shard)
        reader = pq.ParquetFile(pf)
        idx = 0
        for batch in reader.iter_batches(batch_size=256, columns=["image", "text"]):
            cols = batch.to_pydict()
            for cell, text in zip(cols["image"], cols["text"]):
                cur = idx
                idx += 1
                if cur not in rows:
                    continue
                lines = pc.clean_lines(text or "", 0)
                chars = sum(len(l) for l in lines)
                raw = pc.image_bytes(cell)
                if not raw:
                    bad += 1
                    continue
                try:
                    im = Image.open(io.BytesIO(raw))
                    im.load()
                except Exception:
                    bad += 1
                    continue
                stem = f"{args.family}__s{sidx}__r{cur:06d}"
                w, h = pc.write_pgm(im, out / f"{stem}.pgm")
                truth = {
                    "family": args.family,
                    "px_per_em": 0.0,
                    "lines": lines,
                    "glyphs": [],
                    "source": {"kind": "parquet-transcript",
                               "width": w, "height": h,
                               "scale": 1.0, "chars": chars, "lines": len(lines),
                               "from": shard, "row": cur},
                }
                (out / f"{stem}.truth.json").write_text(
                    json.dumps(truth, ensure_ascii=False, indent=1), encoding="utf-8")
                manifest.append({"stem": stem, **truth["source"]})
                made += 1

    (out / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=1),
                                       encoding="utf-8")
    print(f"{made}/{want_total} pages written to {out} ({bad} unreadable)")
    if made != want_total:
        print(f"WARNING: manifest asked for {want_total} rows, only {made} rendered "
              f"-- check the {bad} unreadable count and the shard file list above")


if __name__ == "__main__":
    main()
