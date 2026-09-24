#!/usr/bin/env python3
"""Near-duplicate containment check for `multifinben-englishocr`, the
follow-up the chunk-12 README flags under "What could and couldn't be
checked": exact SHA-1 matching found 123 rows byte-identical to a
`pages/finfilings` score row, and that much duplication means a *different*
page, crop, or re-transcription of the *same underlying filing* is likely
sitting in the training/validation sample too, with even one differing
character -- invisible to a hash.

    python tools/multifinben_near_dup.py score-containment \
        <dataset-dir> <multifinben_index.tsv> <finfilings_rows.tsv> <out.tsv>

    python tools/multifinben_near_dup.py train-val-check \
        <manifest.tsv> <dataset-dir>

    python tools/multifinben_near_dup.py dump-text \
        <dataset-dir> <shard> <row>

# Method

Every transcript is lowercased and whitespace-collapsed, split into words,
and turned into the set of its word 5-gram shingles. Containment of a
candidate row A against a reference row B is `|shingles(A) ∩ shingles(B)| /
|shingles(A)|` -- the fraction of the *candidate's* shingles found in the
reference, not a symmetric Jaccard score, because the question is "how much
of this candidate is covered by that other document", which is exactly what
matters for "did scoring material leak into training" regardless of which
document is longer.

`score-containment` computes, for every MultiFinBen row that survives the
exact-dup and too-short exclusions `bin/split` already applies (i.e. every
row `bin/split` would otherwise consider a train/validation candidate),
containment against each of the 60 `pages/finfilings` score rows, and
reports the max plus which score row it came from. `ocrcer_bench::splits`
reads the output as a plain metadata table (row id, a float, a reference row
id) -- never the transcript text itself, same reasoning as
`multifinben_index.py`'s SHA-1 column: the fingerprint is committed, the
source text is not (`ARCHITECTURE.md` section 11).

`train-val-check` runs the same containment computation between the
*committed* manifest's `train` and `validation` MultiFinBen rows (every
validation row against the union of train shingles) and only reports; it
does not feed the split decision (`PLAN.md` chunk 12 follow-up instructions:
"report only, don't exclude" -- a validation leak is real but much less
severe than a training leak, since validation numbers are read by a human
during tuning, never fitted on).

`dump-text` prints one row's cleaned lines to stdout, for eyeballing a
containment score against the actual text -- used once, by hand, to decide
where the exclusion threshold should sit; not part of any automated pass.

# Performance

Average transcript is ~2,750 characters (~500 words, ~495 shingles); even
the largest is under 10,000 characters. A naive O(candidates x score-rows)
set-intersection pass is affordable at this size, but this script instead
builds one inverted index (shingle -> set of score-row indices) from the 60
score rows and streams each candidate's shingles through it once, so cost is
close to O(total shingles) rather than O(candidates x 60 x average-set-size).
"""

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from multifinben_index import clean_lines  # noqa: E402


def normalize(text):
    """Lowercase, whitespace-collapsed transcript -- the shape shingles are
    built from. Distinct from `clean_lines`'s per-line collapsing: this
    additionally joins lines with a single space and folds case, because a
    5-gram must not break on a line-wrap the source transcript happens to
    carry (see `parquet_corpus.py`'s docstring: transcript line breaks are
    "not render wrapping")."""
    joined = " ".join(clean_lines(text or ""))
    return re.sub(r"\s+", " ", joined.lower()).strip()


def shingles(text, n=5):
    words = normalize(text).split(" ")
    words = [w for w in words if w]
    if len(words) < n:
        return set()
    return {tuple(words[i:i + n]) for i in range(len(words) - n + 1)}


def parse_index_tsv(path):
    rows = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            shard, row, sha1, chars = line.split("\t")
            rows.append((shard, int(row), sha1, int(chars)))
    return rows


def parse_finfilings_rows(path):
    rows = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            shard, row = line.split("\t")
            rows.append((shard, int(row)))
    return rows


def row_id(shard, row):
    return f"{shard}#{row:06d}"


def iter_shard_text(dataset_dir, shard, wanted_rows):
    """Yields (row_index, text) for every row index in `wanted_rows` (a set)
    from one parquet shard, in row order, without loading rows nobody asked
    for into memory beyond the batch pyarrow itself hands back."""
    import pyarrow.parquet as pq

    pf_path = Path(dataset_dir) / "data" / shard
    reader = pq.ParquetFile(pf_path)
    idx = 0
    for batch in reader.iter_batches(batch_size=256, columns=["text"]):
        for text in batch.column("text").to_pylist():
            if idx in wanted_rows:
                yield idx, text
            idx += 1


def cmd_score_containment(args):
    index = parse_index_tsv(args.index_tsv)
    finfilings = parse_finfilings_rows(args.finfilings_tsv)
    score_set = set(finfilings)

    by_key = {(shard, row): (sha1, chars) for shard, row, sha1, chars in index}
    score_hashes = {by_key[(s, r)][0] for s, r in finfilings if (s, r) in by_key}

    # Candidate rows: exactly the population bin/split's assign_multifinben
    # would sample from -- not a score row, not an exact-dup, not too short.
    candidates_by_shard = {}
    for shard, row, sha1, chars in index:
        if (shard, row) in score_set:
            continue
        if sha1 in score_hashes:
            continue
        if chars < 200:
            continue
        candidates_by_shard.setdefault(shard, set()).add(row)

    n_candidates = sum(len(v) for v in candidates_by_shard.values())
    print(f"{n_candidates} candidate rows to check (post exact-dup/short-row exclusion)",
          file=sys.stderr)

    # Pass 1: score-row shingle sets + inverted index. All 60 rows are on one
    # shard as of 2026-09-24 (checked: finfilings_rows.tsv lists only
    # train-00000-of-00008.parquet); this loop still handles more than one
    # shard correctly if that ever changes.
    score_rows_by_shard = {}
    for shard, row in finfilings:
        score_rows_by_shard.setdefault(shard, set()).add(row)

    score_shingle_sets = {}  # (shard, row) -> set of shingles
    for shard, wanted in score_rows_by_shard.items():
        for idx, text in iter_shard_text(args.dataset_dir, shard, wanted):
            score_shingle_sets[(shard, idx)] = shingles(text)

    score_order = list(score_shingle_sets.keys())  # deterministic: dict preserves insertion
    # insertion order came from finfilings_rows.tsv's own (already sorted) order
    inverted = {}  # shingle -> set of indices into score_order
    for i, key in enumerate(score_order):
        for sh in score_shingle_sets[key]:
            inverted.setdefault(sh, set()).add(i)

    # Pass 2: every candidate row's containment against the inverted index.
    out_rows = []
    for shard in sorted(candidates_by_shard):
        wanted = candidates_by_shard[shard]
        for idx, text in iter_shard_text(args.dataset_dir, shard, wanted):
            cand_shingles = shingles(text)
            if not cand_shingles:
                out_rows.append((shard, idx, 0.0, None))
                continue
            hit_counts = Counter()
            for sh in cand_shingles:
                for score_i in inverted.get(sh, ()):
                    hit_counts[score_i] += 1
            if not hit_counts:
                out_rows.append((shard, idx, 0.0, None))
                continue
            best_i, best_hits = max(hit_counts.items(), key=lambda kv: kv[1])
            containment = best_hits / len(cand_shingles)
            out_rows.append((shard, idx, containment, score_order[best_i]))

    out_rows.sort(key=lambda r: (r[0], r[1]))
    with open(args.out_tsv, "w", encoding="utf-8", newline="\n") as f:
        f.write("#shard\trow\tcontainment\tbest_score_shard\tbest_score_row\n")
        for shard, idx, containment, best in out_rows:
            best_shard, best_row = best if best else ("", -1)
            best_row_s = f"{best_row:06d}" if best is not None else ""
            f.write(f"{shard}\t{idx:06d}\t{containment:.4f}\t{best_shard}\t{best_row_s}\n")
    print(f"{len(out_rows)} rows -> {args.out_tsv}", file=sys.stderr)


def load_manifest_rows(manifest_tsv, dataset, split):
    wanted = {}
    with open(manifest_tsv, encoding="utf-8") as f:
        header = f.readline()
        if not header.startswith("#dataset\trow_id\tsplit\tlicence\treason"):
            sys.exit(f"{manifest_tsv}: unexpected header {header!r}")
        for line in f:
            line = line.rstrip("\n")
            if not line:
                continue
            ds, row_id_s, sp, _lic, _reason = line.split("\t", 4)
            if ds != dataset or sp != split:
                continue
            shard, row = row_id_s.split("#")
            wanted.setdefault(shard, set()).add(int(row))
    return wanted


def cmd_train_val_check(args):
    dataset = "multifinben-englishocr"
    train = load_manifest_rows(args.manifest_tsv, dataset, "train")
    val = load_manifest_rows(args.manifest_tsv, dataset, "validation")

    print(f"train rows: {sum(len(v) for v in train.values())}, "
          f"validation rows: {sum(len(v) for v in val.values())}", file=sys.stderr)

    train_shingles = {}
    for shard, rows in train.items():
        for idx, text in iter_shard_text(args.dataset_dir, shard, rows):
            train_shingles[(shard, idx)] = shingles(text)

    train_order = list(train_shingles.keys())
    inverted = {}
    for i, key in enumerate(train_order):
        for sh in train_shingles[key]:
            inverted.setdefault(sh, set()).add(i)

    results = []
    for shard, rows in val.items():
        for idx, text in iter_shard_text(args.dataset_dir, shard, rows):
            cand = shingles(text)
            if not cand:
                results.append((shard, idx, 0.0, None))
                continue
            hit_counts = Counter()
            for sh in cand:
                for i in inverted.get(sh, ()):
                    hit_counts[i] += 1
            if not hit_counts:
                results.append((shard, idx, 0.0, None))
                continue
            best_i, best_hits = max(hit_counts.items(), key=lambda kv: kv[1])
            results.append((shard, idx, best_hits / len(cand), train_order[best_i]))

    results.sort(key=lambda r: -r[2])
    buckets = Counter()
    for _, _, c, _ in results:
        buckets[bucket_label(c)] += 1

    print("\nvalidation-vs-train containment distribution:")
    for label in BUCKET_LABELS:
        print(f"  {label:>10}: {buckets.get(label, 0)}")

    print("\ntop 10 validation rows by containment against train:")
    for shard, idx, c, best in results[:10]:
        best_s = row_id(*best) if best else "(none)"
        print(f"  {row_id(shard, idx)}  c={c:.4f}  best={best_s}")


BUCKET_LABELS = ["0", "(0,0.1]", "(0.1,0.3]", "(0.3,0.5]", "(0.5,0.8]", "(0.8,1]"]


def bucket_label(c):
    if c <= 0.0:
        return "0"
    if c <= 0.1:
        return "(0,0.1]"
    if c <= 0.3:
        return "(0.1,0.3]"
    if c <= 0.5:
        return "(0.3,0.5]"
    if c <= 0.8:
        return "(0.5,0.8]"
    return "(0.8,1]"


def cmd_dump_text(args):
    for idx, text in iter_shard_text(args.dataset_dir, args.shard, {args.row}):
        lines = clean_lines(text or "")
        print(f"=== {args.shard}#{args.row:06d} ({sum(len(l) for l in lines)} chars) ===")
        for ln in lines:
            print(ln)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                  formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p1 = sub.add_parser("score-containment")
    p1.add_argument("dataset_dir")
    p1.add_argument("index_tsv")
    p1.add_argument("finfilings_tsv")
    p1.add_argument("out_tsv")
    p1.set_defaults(func=cmd_score_containment)

    p2 = sub.add_parser("train-val-check")
    p2.add_argument("manifest_tsv")
    p2.add_argument("dataset_dir")
    p2.set_defaults(func=cmd_train_val_check)

    p3 = sub.add_parser("dump-text")
    p3.add_argument("dataset_dir")
    p3.add_argument("shard")
    p3.add_argument("row", type=int)
    p3.set_defaults(func=cmd_dump_text)

    args = ap.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
