"""Analyse the per-word mirror-cue probe (PW rows from hl_probe_src/main.rs, OCRCER_PW mode).

Reads a private TSV (not committed) with columns:
  PW, crop_label, word_text, word_conf, asis_text, asis_geomean, asis_n,
  flip_text, flip_geomean, flip_n, rectWxH

crop_label encodes the source page: front_L<ink>_<pre>__asis (pure front text,
no show-through, from faded_lines.py-style renders) or
back_F<frontink>_B<backluma>_<pre>__asis (show-through crop, cut from row 320
down on a showthrough_lines.py page -- below the four front lines, holding
show-through only; see 2026-09-22_research_classical_techniques.md lines
3985-4030). Ground truth is attribution by page of origin, by construction:
  - front_* pages: every word is real front text.
  - back_* pages: every word is show-through.
No text-matching against a front/back vocabulary is performed -- that rule
was tried in an earlier pass and produced zero matches out of 1092 rows; it
added no information and could not have, since every back_* row is
show-through by construction of the crop, not by its legibility.

Usage: python mirror_per_word_analysis.py <path-to-pw_k34.tsv> <path-to-pw_k10.tsv>
"""
import sys
import statistics as st

HEIGHT_FLOOR = 15  # rect height (px); drops "." "_" single-artifact fragments, see write-up


def load(path):
    rows = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            p = line.rstrip("\n").split("\t")
            if not p or p[0] != "PW":
                continue
            (_, label, wtext, wconf, atext, ageo, an, ftext, fgeo, fn, wh) = p
            w, h = wh.split("x")
            rows.append(dict(
                label=label, wtext=wtext, wconf=float(wconf),
                atext=atext, ageo=float(ageo), an=int(an),
                ftext=ftext, fgeo=float(fgeo), fn=int(fn),
                w=int(w), h=int(h),
            ))
    return rows


def classify(rows):
    for r in rows:
        page = "front" if r["label"].startswith("front_") else "back"
        r["page"] = page
        r["cls"] = "real" if page == "front" else "show_through"
        r["delta"] = r["fgeo"] - r["ageo"]
        r["nchars"] = max(r["an"], r["fn"])
        r["wlen"] = len(r["wtext"])
    return rows


def describe(vals):
    if not vals:
        return "n=0"
    vals = sorted(vals)
    n = len(vals)
    mean = sum(vals) / n
    med = st.median(vals)
    sd = st.pstdev(vals) if n > 1 else 0.0
    q1 = vals[n // 4]
    q3 = vals[(3 * n) // 4]
    return f"n={n} mean={mean:+.3f} median={med:+.3f} sd={sd:.3f} q1={q1:+.3f} q3={q3:+.3f} min={vals[0]:+.3f} max={vals[-1]:+.3f}"


def roc_table(real_deltas, show_deltas, thresholds):
    lines = []
    for t in thresholds:
        ff = sum(1 for d in real_deltas if d > t) / len(real_deltas) if real_deltas else float("nan")
        rec = sum(1 for d in show_deltas if d > t) / len(show_deltas) if show_deltas else float("nan")
        lines.append((t, ff, rec))
    return lines


def best_threshold(real_deltas, show_deltas):
    cands = sorted(set(real_deltas) | set(show_deltas))
    best = None
    for t in cands:
        ff = sum(1 for d in real_deltas if d > t) / len(real_deltas) if real_deltas else 1.0
        rec = sum(1 for d in show_deltas if d > t) / len(show_deltas) if show_deltas else 0.0
        score = rec - ff
        if best is None or score > best[0]:
            best = (score, t, ff, rec)
    return best


def run_for_k(name, path, mir_conf_path=None):
    print(f"\n===== {name} : {path} =====")
    raw = load(path)
    classify(raw)
    n_total = len(raw)
    kept = [r for r in raw if r["h"] >= HEIGHT_FLOOR]
    n_dropped = n_total - len(kept)
    print(f"rows: {n_total} total, height>={HEIGHT_FLOOR}px filter drops {n_dropped}, {len(kept)} kept")

    n_zero_both = sum(1 for r in kept if r["an"] == 0 and r["fn"] == 0)
    print(f"of kept rows, {n_zero_both} still have zero chars in BOTH asis and flip (delta=0, uninformative) -- kept but flagged")

    real = [r for r in kept if r["cls"] == "real"]
    show_through = [r for r in kept if r["cls"] == "show_through"]
    print(f"class counts (post-filter): real(front page)={len(real)}  show_through(back page)={len(show_through)}")

    print("\n-- Q1: delta = flip_geomean - asis_geomean, by class (height filter only) --")
    print("real (front page):         ", describe([r["delta"] for r in real]))
    print("show_through (back page):  ", describe([r["delta"] for r in show_through]))

    real_nz = [r for r in real if r["an"] > 0 or r["fn"] > 0]
    show_nz = [r for r in show_through if r["an"] > 0 or r["fn"] > 0]
    print(f"\n-- Q1b: same, excluding rows where BOTH asis and flip read zero chars ({len(real)-len(real_nz)} of real, {len(show_through)-len(show_nz)} of show_through dropped) --")
    print("real (front page):         ", describe([r["delta"] for r in real_nz]))
    print("show_through (back page):  ", describe([r["delta"] for r in show_nz]))

    print("\n-- Q2: threshold separation, real vs show_through --")
    real_d = [r["delta"] for r in real]
    show_d = [r["delta"] for r in show_through]
    thresholds = [-0.2, -0.1, -0.05, 0.0, 0.05, 0.1, 0.15, 0.2, 0.3]
    print(f"{'thresh':>8} {'false_flag_on_real':>20} {'recall_on_show_through':>23}")
    for t, ff, rec in roc_table(real_d, show_d, thresholds):
        print(f"{t:>8.2f} {ff:>20.3f} {rec:>23.3f}")
    score, t, ff, rec = best_threshold(real_d, show_d)
    print(f"best (Youden-style) threshold: delta > {t:+.3f}  ->  false_flag_on_real={ff:.3f}  recall_on_show_through={rec:.3f}  (rec-ff={score:.3f})")

    print("\n-- Q2b: threshold + nchars combined (require nchars>=3 as well) --")
    real_big = [r for r in real if r["nchars"] >= 3]
    show_big = [r for r in show_through if r["nchars"] >= 3]
    print(f"n real (nchars>=3)={len(real_big)}  n show_through (nchars>=3)={len(show_big)}")
    real_d2 = [r["delta"] for r in real_big]
    show_d2 = [r["delta"] for r in show_big]
    for t, ff, rec in roc_table(real_d2, show_d2, thresholds):
        print(f"{t:>8.2f} {ff:>20.3f} {rec:>23.3f}")
    if real_d2 and show_d2:
        score2, t2, ff2, rec2 = best_threshold(real_d2, show_d2)
        print(f"best threshold (nchars>=3): delta > {t2:+.3f}  ->  false_flag_on_real={ff2:.3f}  recall_on_show_through={rec2:.3f}  (rec-ff={score2:.3f})")

    print("\n-- Q2c: threshold separation, both-nonzero subset (drops delta=0 blanks) --")
    real_d3 = [r["delta"] for r in real_nz]
    show_d3 = [r["delta"] for r in show_nz]
    print(f"n real_nz={len(real_d3)}  n show_through_nz={len(show_d3)}")
    for t, ff, rec in roc_table(real_d3, show_d3, thresholds):
        print(f"{t:>8.2f} {ff:>20.3f} {rec:>23.3f}")
    if real_d3 and show_d3:
        score3, t3, ff3, rec3 = best_threshold(real_d3, show_d3)
        print(f"best threshold (nz subset): delta > {t3:+.3f}  ->  false_flag_on_real={ff3:.3f}  recall_on_show_through={rec3:.3f}  (rec-ff={score3:.3f})")

    print("\n-- Q3: delta vs word length (in-page word_text length, chars) --")
    buckets = {}
    for r in real + show_through:
        b = "1-2" if r["wlen"] <= 2 else ("3-5" if r["wlen"] <= 5 else ("6-9" if r["wlen"] <= 9 else "10+"))
        buckets.setdefault((r["cls"], b), []).append(r["delta"])
    for cls in ("real", "show_through"):
        for b in ("1-2", "3-5", "6-9", "10+"):
            vals = buckets.get((cls, b), [])
            if vals:
                print(f"  {cls:14s} len={b:4s} {describe(vals)}")

    print("\n-- Q4a: in-page word_conf (whole-page, ASIS orientation) vs mir_k*.conf whole-page mean_conf --")
    print("   (sanity check: these two are the SAME quantity computed two ways; should match)")
    by_label_all = {}
    for r in raw:
        by_label_all.setdefault(r["label"], []).append(r)
    mir_conf = load_mir_conf(mir_conf_path) if mir_conf_path else {}
    for label in sorted(by_label_all):
        rs = by_label_all[label]
        pw_mean = sum(r["wconf"] for r in rs) / len(rs)
        mir_asis = mir_conf.get(label)
        print(f"  {label:32s} n={len(rs):3d}  pw_mean_word_conf={pw_mean:.3f}  mir_asis_mean_conf={mir_asis}")

    print("\n-- Q4b: whole-page FLIP mean_conf (mir_k*.conf, page flipped as one image) vs")
    print("   mean per-word ISOLATED-CROP flip geomean (fgeo, this probe) -- NOT the same quantity --")
    flip_label = None
    for label in sorted(by_label_all):
        rs = by_label_all[label]
        flip_label = label.replace("__asis", "__flip")
        mir_flip = mir_conf.get(flip_label)
        pw_fgeo_mean = sum(r["fgeo"] for r in kept if r["label"] == label) / max(1, len([r for r in kept if r["label"] == label]))
        print(f"  {label:32s} mir_whole_page_flip={mir_flip}  pw_mean_isolated_word_crop_flip={pw_fgeo_mean:.3f}")

    return dict(real=real, show_through=show_through, raw=raw, kept=kept)


def load_mir_conf(path):
    d = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            label = parts[0]
            mc = None
            for tok in parts:
                if tok.startswith("mean_conf "):
                    mc = float(tok.split()[1])
            d[label] = mc
    return d


def main():
    if len(sys.argv) < 3:
        print("usage: mirror_per_word_analysis.py <pw_k34.tsv> <pw_k10.tsv> [mir_k34.conf mir_k10.conf]")
        sys.exit(1)
    mir34 = sys.argv[3] if len(sys.argv) > 3 else None
    mir10 = sys.argv[4] if len(sys.argv) > 4 else None
    run_for_k("k34 (shipped Sauvola k=0.34)", sys.argv[1], mir34)
    run_for_k("k10 (lowered Sauvola k=0.10)", sys.argv[2], mir10)


if __name__ == "__main__":
    main()
