#!/usr/bin/env python3
"""Row-level metadata index for `multifinben-englishocr`, and the finfilings
score-row list, both committed inputs to `ocrcer-bench --bin split`
(chunk 12, `PLAN.md` section 2c).

    python tools/multifinben_index.py index <dataset-dir> <out.tsv>
    python tools/multifinben_index.py finfilings-rows <finfilings-manifest.json> <out.tsv>

# Why an index file rather than reading the parquet in Rust

`bin/split` must "re-derive the manifest byte-identically" (`PLAN.md` chunk
12 row) without pulling a parquet reader into `ocrcer-bench` for a one-time
job every other corpus tool in `tools/` already does in Python. The index
this script writes carries only **metadata about each row** -- which shard,
which row number, a SHA-1 of its transcript text, and the transcript's
cleaned character count -- never the transcript itself. That is enough for
`bin/split` to (a) find rows whose text is identical to a scoring row's, the
proxy this project uses for "same source document" per `PLAN.md` section 2c,
since the dataset carries no document-id column at all (checked: its Arrow
schema is exactly `image: string, text: string`, nothing else); and (b) drop
rows too short to have been rendered by `parquet_corpus.py` in the first
place. Neither needs the text once the hash and length are known.

The SHA-1 is a content fingerprint, not a security boundary -- collision
resistance is not the property being used, uniqueness-in-practice over a few
thousand rows is. Committing it leaks nothing about the (Apache-2.0,
redistributable) source text; a hash is not the text.

`clean_lines` here is copied verbatim from `parquet_corpus.py`'s function of
the same name, deliberately, so the character count matches exactly what a
real corpus render would measure (rule 4 is about `ocrcer-core` pipeline
stages, not about this kind of one-off Python indexing script, but drift
between "what the index measured" and "what the renderer measures" would
silently mis-place the 200-char floor, so the two copies must not diverge --
if `parquet_corpus.py`'s `clean_lines` ever changes, update this one too).
"""

import argparse
import glob
import hashlib
import json
import re
import sys
from pathlib import Path


def clean_lines(text):
    lines = [re.sub(r"[ \t ]+", " ", ln).strip() for ln in (text or "").splitlines()]
    return [ln for ln in lines if ln]


def cmd_index(args):
    import pyarrow.parquet as pq

    src = Path(args.dataset_dir) / "data"
    files = sorted(src.glob("*.parquet"))
    if not files:
        sys.exit(f"no parquet files under {src}")

    rows = []
    for pf_path in files:
        shard = pf_path.name
        reader = pq.ParquetFile(pf_path)
        idx = 0
        for batch in reader.iter_batches(batch_size=256, columns=["text"]):
            for text in batch.column("text").to_pylist():
                lines = clean_lines(text)
                chars = sum(len(l) for l in lines)
                sha1 = hashlib.sha1((text or "").encode("utf-8", "ignore")).hexdigest()
                rows.append((shard, idx, sha1, chars))
                idx += 1

    out = Path(args.out_tsv)
    with out.open("w", encoding="utf-8", newline="\n") as f:
        f.write("#shard\trow\tsha1\tchars\n")
        for shard, idx, sha1, chars in rows:
            f.write(f"{shard}\t{idx:06d}\t{sha1}\t{chars}\n")
    print(f"{len(rows)} rows indexed across {len(files)} shards -> {out}")


def cmd_finfilings_rows(args):
    manifest = json.loads(Path(args.manifest_json).read_text(encoding="utf-8"))
    out = Path(args.out_tsv)
    with out.open("w", encoding="utf-8", newline="\n") as f:
        f.write("#shard\trow\n")
        for m in sorted(manifest, key=lambda m: (m["from"], m["row"])):
            f.write(f"{m['from']}\t{m['row']:06d}\n")
    print(f"{len(manifest)} finfilings score rows -> {out}")


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p1 = sub.add_parser("index", help="index every row of a multifinben-style dataset")
    p1.add_argument("dataset_dir", help="dataset root, e.g. .../datasets/multifinben-englishocr")
    p1.add_argument("out_tsv")
    p1.set_defaults(func=cmd_index)

    p2 = sub.add_parser("finfilings-rows",
                        help="copy the (shard,row) pairs behind pages/finfilings")
    p2.add_argument("manifest_json", help="pages/finfilings/manifest.json")
    p2.add_argument("out_tsv")
    p2.set_defaults(func=cmd_finfilings_rows)

    args = ap.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
