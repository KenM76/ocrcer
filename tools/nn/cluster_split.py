#!/usr/bin/env python3
"""Chunk 15 data deliverable (b): a fixed, cluster-disjoint internal
train/val split of the `multifinben-englishocr` train-split pages chunk 15's
Rust dumper (`crates/ocrcer-bench/src/bin/nn15_dump.rs`, `list-stems`
subcommand) samples for real crops.

Written fresh for chunk 15 -- not `tools/nnprobe/cluster_pages.py` imported
or copied, per the task's "write the trainer fresh" instruction extended to
the split-generation tooling. The clustering *method* is the same one that
script used (itself the same one `bench/splits/README.md`'s "Near-duplicate
check" documents for the train/score firewall): normalise each page's
transcript, build word 5-gram shingles, and union two pages whenever their
symmetric shingle containment clears a threshold, so that a cluster of
same-template near-duplicate filings never splits across the internal train
and val-internal folds -- which a page-level random split could do, letting
the internal validation number overstate accuracy on boilerplate it had
already seen a near-twin of during training.

`CLUSTER_THRESHOLD = 0.3` is carried over unchanged from `cluster_pages.py`,
where it is *stated, not measured* -- the "somewhat related" boundary
`bench/splits/README.md`'s own containment distribution table treats as the
start of the near-dup regime. This is a fold-balancing choice, not the
train/score firewall's `NEAR_DUP_THRESHOLD = 0.0` (zero tolerance, because a
leak there is a scoring-integrity violation); a leak here only costs the
internal validation number some of its independence, which this whole
mechanism exists to bound, not eliminate by construction the way the
firewall must.

Usage:
    python tools/nn/cluster_split.py <stems.tsv> <pages-train-dir> <out.tsv>

`<stems.tsv>` is `list-stems`'s own output (`row_id\\tstem` lines) --
piped to a file first so this script and the Rust dumper are provably
looking at the same page sample:

    cargo run -p ocrcer-bench --bin nn15_dump -- list-stems \\
        <pages-train-dir> 4 > /tmp/nn15_stems.tsv
    python tools/nn/cluster_split.py /tmp/nn15_stems.tsv \\
        <pages-train-dir> bench/splits/nn15_page_split.tsv

Writes `<out.tsv>`: `#stem\\tfold\\tcluster_id`, fold in {A, B}. `dump`'s
`load_fold_file` treats fold `B` as the internal validation fold (own doc
comment, `crates/ocrcer-bench/src/bin/nn15_dump.rs`).
"""
from __future__ import annotations

import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

CLUSTER_THRESHOLD = 0.3
SHINGLE_N = 5

_WS_RE = re.compile(r"\s+")


def normalize(text: str) -> str:
    return _WS_RE.sub(" ", text.lower()).strip()


def shingles(text: str, n: int = SHINGLE_N) -> set[str]:
    words = text.split(" ")
    if len(words) < n:
        return {" ".join(words)} if words else set()
    return {" ".join(words[i : i + n]) for i in range(len(words) - n + 1)}


def load_stems(stems_path: Path) -> list[str]:
    stems: list[str] = []
    seen = set()
    with open(stems_path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            cols = line.split("\t")
            stem = cols[1] if len(cols) > 1 else cols[0]
            if stem not in seen:
                seen.add(stem)
                stems.append(stem)
    return stems


def page_shingles(pages_dir: Path, stem: str) -> set[str]:
    truth_path = pages_dir / f"{stem}.truth.json"
    with open(truth_path, encoding="utf-8") as f:
        d = json.load(f)
    text = normalize(" ".join(d["lines"]))
    return shingles(text)


class UnionFind:
    def __init__(self, items: list[str]):
        self.parent = {x: x for x in items}

    def find(self, x: str) -> str:
        while self.parent[x] != x:
            self.parent[x] = self.parent[self.parent[x]]
            x = self.parent[x]
        return x

    def union(self, a: str, b: str) -> None:
        ra, rb = self.find(a), self.find(b)
        if ra != rb:
            self.parent[ra] = rb


def main() -> int:
    if len(sys.argv) != 4:
        print(
            "usage: cluster_split.py <stems.tsv> <pages-train-dir> <out.tsv>",
            file=sys.stderr,
        )
        return 1
    stems_path = Path(sys.argv[1])
    pages_dir = Path(sys.argv[2])
    out_path = Path(sys.argv[3])
    out_path.parent.mkdir(parents=True, exist_ok=True)

    stems = load_stems(stems_path)
    print(f"stems: {len(stems)}")

    shingle_sets = {s: page_shingles(pages_dir, s) for s in stems}

    uf = UnionFind(stems)
    n_edges = 0
    pairs_checked = 0
    for i in range(len(stems)):
        a = stems[i]
        sa = shingle_sets[a]
        if not sa:
            continue
        for j in range(i + 1, len(stems)):
            b = stems[j]
            sb = shingle_sets[b]
            if not sb:
                continue
            pairs_checked += 1
            inter = len(sa & sb)
            containment = max(inter / len(sa), inter / len(sb))
            if containment >= CLUSTER_THRESHOLD:
                uf.union(a, b)
                n_edges += 1
    print(f"pairs checked: {pairs_checked}, near-dup edges (>= {CLUSTER_THRESHOLD}): {n_edges}")

    clusters: dict[str, list[str]] = defaultdict(list)
    for s in stems:
        clusters[uf.find(s)].append(s)
    cluster_list = sorted(clusters.values(), key=lambda c: (-len(c), c[0]))

    sizes = [len(c) for c in cluster_list]
    size_hist = Counter(sizes)
    print(f"clusters: {len(cluster_list)}")
    print(f"cluster size distribution: {dict(sorted(size_hist.items()))}")

    # Greedy balanced bin-packing: largest clusters first, always add to the
    # currently-smaller fold. Deterministic given cluster_list's fixed sort
    # order (size desc, then first stem alphabetically) -- no RNG.
    fold_a: list[str] = []
    fold_b: list[str] = []
    cluster_id_of: dict[str, int] = {}
    fold_of: dict[str, str] = {}
    for cid, cluster in enumerate(cluster_list):
        target = fold_a if len(fold_a) <= len(fold_b) else fold_b
        target_name = "A" if target is fold_a else "B"
        for s in cluster:
            cluster_id_of[s] = cid
            fold_of[s] = target_name
        target.extend(cluster)

    print(f"fold A (train): {len(fold_a)} pages, fold B (internal val): {len(fold_b)} pages")

    with open(out_path, "w", encoding="utf-8") as f:
        f.write("#stem\tfold\tcluster_id\n")
        for s in stems:
            f.write(f"{s}\t{fold_of[s]}\t{cluster_id_of[s]}\n")
    print(f"wrote {out_path}")

    multi = [c for c in cluster_list if len(c) > 1]
    print(f"multi-page clusters (size > 1): {len(multi)}")
    for c in multi[:20]:
        print(f"  cluster size {len(c)}: {c}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
