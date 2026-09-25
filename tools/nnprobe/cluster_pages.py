#!/usr/bin/env python3
"""Round 3 item 2 (docs/measurements/2026-09-25_nn_probe.md): cluster the 86
finfilings-train pages used by Round 1/2's real-crop dump into near-duplicate
groups, so a fold split can keep every near-dup cluster on one side.

Method: the same one `bench/splits/README.md` "Near-duplicate check" uses for
the finfilings train/score firewall -- normalise each page's transcript
(lowercase, whitespace-collapsed), build word 5-gram shingles, and compute
containment |A ∩ B| / |A| between every pair of pages. That check used
NEAR_DUP_THRESHOLD = 0.0 for a *train-vs-score* firewall (zero tolerance,
because any leak there is a scoring-integrity violation); this script targets
a much smaller, purely-CV-fold-balancing problem (86 pages, want ~43/43), so
it uses CLUSTER_THRESHOLD = 0.3, the "somewhat related" boundary the README's
own distribution table treats as the start of the near-dup regime. This
choice is stated, not measured -- a stricter or looser value would move
cluster count and fold sizes; 0.3 is picked to catch same-template
boilerplate reuse (the leakage risk item 2 names) without merging every pair
of filings that merely share generic financial-form phrasing.

Containment is computed symmetrically here (max of both directions) since,
unlike the train-vs-score firewall (candidate vs a fixed score set), there is
no "candidate" / "reference" asymmetry between two train pages -- either
overlaps the other heavily.

Reads truth.json "lines" directly (no parquet/pyarrow dependency -- these are
already-rendered pages, not corpus rows), rather than importing
tools/multifinben_near_dup.py, which is shaped for the corpus-shard case.

Usage:
    python tools/nnprobe/cluster_pages.py <pages-dir> <real_meta.tsv> <out-dir>

Writes <out-dir>/fold_assignment.tsv (#stem\tfold\tcluster_id, fold in {A,B})
and prints cluster count / size distribution / fold balance to stdout.
"""
from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
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


def load_stems(real_meta_path: Path) -> list[str]:
    stems: list[str] = []
    seen = set()
    with open(real_meta_path, encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            cols = line.rstrip("\n").split("\t")
            stem = cols[1]
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
            "usage: cluster_pages.py <pages-dir> <real_meta.tsv> <out-dir>",
            file=sys.stderr,
        )
        return 1
    pages_dir = Path(sys.argv[1])
    real_meta_path = Path(sys.argv[2])
    out_dir = Path(sys.argv[3])
    out_dir.mkdir(parents=True, exist_ok=True)

    stems = load_stems(real_meta_path)
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
            c_ab = inter / len(sa)
            c_ba = inter / len(sb)
            containment = max(c_ab, c_ba)
            if containment >= CLUSTER_THRESHOLD:
                uf.union(a, b)
                n_edges += 1
    print(f"pairs checked: {pairs_checked}, near-dup edges (>= {CLUSTER_THRESHOLD}): {n_edges}")

    clusters: dict[str, list[str]] = defaultdict(list)
    for s in stems:
        clusters[uf.find(s)].append(s)
    cluster_list = sorted(clusters.values(), key=lambda c: (-len(c), c[0]))

    sizes = [len(c) for c in cluster_list]
    from collections import Counter

    size_hist = Counter(sizes)
    print(f"clusters: {len(cluster_list)}")
    print(f"cluster size distribution: {dict(sorted(size_hist.items()))}")

    # Greedy balanced bin-packing: largest clusters first, always add to the
    # currently-smaller fold. Deterministic given cluster_list's fixed sort
    # order (size desc, then first stem alphabetically -- no RNG).
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

    print(f"fold A: {len(fold_a)} pages, fold B: {len(fold_b)} pages")

    out_path = out_dir / "fold_assignment.tsv"
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("#stem\tfold\tcluster_id\n")
        for s in stems:
            f.write(f"{s}\t{fold_of[s]}\t{cluster_id_of[s]}\n")
    print(f"wrote {out_path}")

    # Report the largest clusters explicitly -- these are the ones that
    # would have crossed a random page-disjoint split in Round 2.
    multi = [c for c in cluster_list if len(c) > 1]
    print(f"multi-page clusters (size > 1): {len(multi)}")
    for c in multi[:20]:
        print(f"  cluster size {len(c)}: {c}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
