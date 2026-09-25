#!/usr/bin/env python3
"""Round 3 of the throwaway neural probe
(`docs/measurements/2026-09-25_nn_probe.md` "Round 3"). Answers the
architect's three objections to round 2's headline number (net+realA 97.5%
vs matcher-without-realA 92.3%, page-disjoint):

1. Unequal data -- the matcher's own "+realA" side is computed by the Rust
   bin `nn-probe3-matcherA` (`crates/ocrcer-bench/src/bin/nn_probe3_matcherA.rs`),
   which calls `ocrcer_core::r#match::nearest` on an in-memory bank extended
   with the training fold's real-crop feature vectors -- the runtime's own
   matcher, never reimplemented here (`CLAUDE.md` rule 4). This script only
   loads those predictions and compares.
2. Leakage between folds -- reads `fold_assignment.tsv`
   (`tools/nnprobe/cluster_pages.py`'s near-dup-aware cluster split) instead
   of round 2's random page-disjoint split.
3/4. Reuses round 2's `run_one_fold`, `per_group_accuracy`, and the 0/O/o
   breakdown machinery unchanged, applied to the new split and the new
   matcher+realA column.

Usage:
    python tools/nnprobe/train3.py --dump-dir D:/.../nnprobe2 \\
        --matcherA-dir D:/.../nnprobe3 \\
        --fold-assignment D:/.../nnprobe3/fold_assignment.tsv

Writes `<matcherA-dir>/report3_folds.json` (private, not committed).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import train2  # noqa: E402

NONE_CLASS = train2.NONE_CLASS
TOPK = train2.TOPK


def load_fold_assignment(path: Path):
    fold_of = {}
    cluster_of = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            if not line.strip() or line.startswith("#"):
                continue
            stem, fold, cid = line.rstrip("\n").split("\t")
            fold_of[stem] = fold
            cluster_of[stem] = int(cid)
    fold_a = {s for s, fl in fold_of.items() if fl == "A"}
    fold_b = {s for s, fl in fold_of.items() if fl == "B"}
    return fold_a, fold_b, fold_of, cluster_of


def load_matcherA_preds(matcherA_dir: Path, tag: str):
    """Loads the Rust bin's <tag>_test_matcherA_top1.u16, which is in
    real_meta.tsv row order restricted to the test fold (see the Rust
    module doc)."""
    path = matcherA_dir / f"{tag}_test_matcherA_top1.u16"
    return np.fromfile(path, dtype="<u2")


def oracle_stats(a_correct, b_correct, n):
    both_right = int((a_correct & b_correct).sum())
    both_wrong = int((~a_correct & ~b_correct).sum())
    a_wrong_b_right = int((~a_correct & b_correct).sum())
    b_wrong_a_right = int((a_correct & ~b_correct).sum())
    return {
        "both_right": both_right,
        "both_wrong": both_wrong,
        "a_wrong_b_right": a_wrong_b_right,
        "b_wrong_a_right": b_wrong_a_right,
        "a_wrong_b_right_frac": a_wrong_b_right / n if n else float("nan"),
        "b_wrong_a_right_frac": b_wrong_a_right / n if n else float("nan"),
        "oracle_union_acc": (both_right + a_wrong_b_right + b_wrong_a_right) / n if n else float("nan"),
    }


def run_fair_fold(tag, dump_dir, matcherA_dir, device, device_desc, classes, n_classes,
                   g_train, x_train, y_train, g_val, x_val, y_val,
                   g_real, x_real, y_real, real_topk_class, real_topk_dist, real_stems,
                   train_stems, test_stems, matcherA_tag,
                   max_epochs, patience, batch_size, step_size, gamma):
    # net+realA, matcher(no-realA), and all the per-fold bookkeeping: reuse
    # round 2's fold runner verbatim on the new (cluster-disjoint) split.
    base = train2.run_one_fold(
        tag, dump_dir, device, device_desc, classes, n_classes,
        g_train, x_train, y_train, g_val, x_val, y_val,
        g_real, x_real, y_real, real_topk_class, real_topk_dist, real_stems,
        train_stems, test_stems, max_epochs, patience, batch_size, step_size, gamma)

    test_mask = np.array([s in test_stems for s in real_stems])
    y_test = base["_y_test"]
    n_test = y_test.shape[0]

    matcherA_top1 = load_matcherA_preds(matcherA_dir, matcherA_tag)
    assert matcherA_top1.shape[0] == n_test, (
        f"{matcherA_tag}: {matcherA_top1.shape[0]} predictions, expected {n_test} "
        f"test rows -- row-order mismatch between real_meta.tsv and the fold mask"
    )
    matcherA_valid = matcherA_top1 != NONE_CLASS
    matcherA_correct = (matcherA_top1 == y_test) & matcherA_valid
    matcherA_acc = float(matcherA_correct[matcherA_valid].sum() / matcherA_valid.sum()) \
        if matcherA_valid.any() else float("nan")

    net_correct = base["_net_pred"] == y_test
    net_acc = base["net_acc"]

    base["matcherA_acc"] = matcherA_acc
    base["matcherA_n_no_match"] = int((~matcherA_valid).sum())
    base["matcherA_per_group"] = train2.per_group_accuracy(
        y_test[matcherA_valid], matcherA_top1[matcherA_valid], classes)
    base["matcherA_top_confusions"] = train2.top_confusions(
        y_test[matcherA_valid], matcherA_top1[matcherA_valid], classes)
    base["oracle_netA_vs_matcherA"] = oracle_stats(net_correct, matcherA_correct, n_test)

    return base


def strip_private(d):
    return {k: v for k, v in d.items() if not k.startswith("_")}


def run_0Oo_breakdown_fair(dump_dir, matcherA_dir, real_stems, y_real, classes,
                            fold_a, fold_b):
    """0/O/o breakdown for net+realA and matcher+realA, computed from the
    two fair folds' test-side predictions concatenated (so every real-A row
    is scored exactly once, by whichever fold held it out)."""
    class_of_cp = {cp: idx for idx, (cp, _cat) in classes.items()}
    targets = {ch: class_of_cp[ord(ch)] for ch in ("0", "O", "o") if ord(ch) in class_of_cp}

    def breakdown_for(y_true, pred, valid, name):
        per_class = {}
        case_errors = digit_errors = other_errors = 0
        for ch, cls_idx in targets.items():
            mask = (y_true == cls_idx) & valid
            n = int(mask.sum())
            p = pred[mask]
            correct = int((p == cls_idx).sum())
            wrong_mask = p != cls_idx
            for wp in p[wrong_mask]:
                wp = int(wp)
                pcp = classes.get(wp, (0, "?"))[0]
                pch = chr(pcp) if wp in classes else "?"
                if ch in ("O", "o") and pch in ("O", "o"):
                    case_errors += 1
                elif (ch == "0" and pch in ("O", "o")) or (ch in ("O", "o") and pch == "0"):
                    digit_errors += 1
                else:
                    other_errors += 1
            per_class[ch] = {"n": n, "correct": correct, "acc": correct / n if n else float("nan")}
        total = case_errors + digit_errors + other_errors
        return {
            "classifier": name,
            "per_class": per_class,
            "error_breakdown": {
                "case_level (O<->o)": case_errors,
                "digit_vs_letter (0<->O/o)": digit_errors,
                "other": other_errors,
                "total_errors": total,
                "case_level_frac": case_errors / total if total else float("nan"),
                "digit_vs_letter_frac": digit_errors / total if total else float("nan"),
            },
        }

    return breakdown_for, targets


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dump-dir", default="D:/Dev/ExcludedPrivate/ocrcer/nnprobe2")
    ap.add_argument("--matcherA-dir", default="D:/Dev/ExcludedPrivate/ocrcer/nnprobe3")
    ap.add_argument("--fold-assignment",
                     default="D:/Dev/ExcludedPrivate/ocrcer/nnprobe3/fold_assignment.tsv")
    ap.add_argument("--max-epochs", type=int, default=60)
    ap.add_argument("--patience", type=int, default=10)
    ap.add_argument("--batch-size", type=int, default=256)
    ap.add_argument("--step-size", type=int, default=15)
    ap.add_argument("--gamma", type=float, default=0.5)
    ap.add_argument("--device", default="auto", choices=["auto", "cpu", "xpu"])
    args = ap.parse_args()

    train2.set_seed(train2.SEED)
    dump_dir = Path(args.dump_dir)
    matcherA_dir = Path(args.matcherA_dir)
    device, device_desc = train2.pick_device(args.device)
    print(f"train3.py: device = {device_desc}", file=sys.stderr)

    classes = train2.load_classes(dump_dir)
    n_classes = max(classes.keys()) + 1

    g_train, x_train, y_train = train2.load_gxy(dump_dir, "bank_train")
    g_val, x_val, y_val = train2.load_gxy(dump_dir, "bank_val")
    g_real, x_real, y_real = train2.load_gxy(dump_dir, "real")
    n_real = y_real.shape[0]
    real_topk_class = train2.load_u16_col(dump_dir, "real_topk_class.u16", n_real, TOPK)
    real_topk_dist = train2.load_f32_col(dump_dir, "real_topk_dist.f32", n_real, TOPK)
    real_stems = train2.load_real_stems(dump_dir, n_real)

    fold_a, fold_b, fold_of, cluster_of = load_fold_assignment(Path(args.fold_assignment))
    n_multipage_clusters = len({cid for s, cid in cluster_of.items()
                                 if list(cluster_of.values()).count(cid) > 1})
    print(f"train3.py: cluster-disjoint split: fold A={len(fold_a)} fold B={len(fold_b)} "
          f"pages={len(fold_of)}", file=sys.stderr)

    fold1 = run_fair_fold(
        "fold1(train=A,test=B)", dump_dir, matcherA_dir, device, device_desc, classes, n_classes,
        g_train, x_train, y_train, g_val, x_val, y_val,
        g_real, x_real, y_real, real_topk_class, real_topk_dist, real_stems,
        fold_a, fold_b, "fold1", args.max_epochs, args.patience, args.batch_size,
        args.step_size, args.gamma)
    fold2 = run_fair_fold(
        "fold2(train=B,test=A)", dump_dir, matcherA_dir, device, device_desc, classes, n_classes,
        g_train, x_train, y_train, g_val, x_val, y_val,
        g_real, x_real, y_real, real_topk_class, real_topk_dist, real_stems,
        fold_b, fold_a, "fold2", args.max_epochs, args.patience, args.batch_size,
        args.step_size, args.gamma)

    print(f"train3.py: fold1 net+realA={fold1['net_acc']:.4f} "
          f"matcher(no-realA)={fold1['matcher_acc']:.4f} "
          f"matcher+realA={fold1['matcherA_acc']:.4f}", file=sys.stderr)
    print(f"train3.py: fold2 net+realA={fold2['net_acc']:.4f} "
          f"matcher(no-realA)={fold2['matcher_acc']:.4f} "
          f"matcher+realA={fold2['matcherA_acc']:.4f}", file=sys.stderr)

    # 0/O/o breakdown for net+realA and matcher+realA, concatenated across
    # both folds' test sides (every real-A row scored exactly once).
    y_test_all = np.concatenate([fold1["_y_test"], fold2["_y_test"]])
    net_pred_all = np.concatenate([fold1["_net_pred"], fold2["_net_pred"]])
    matcherA1 = load_matcherA_preds(matcherA_dir, "fold1")
    matcherA2 = load_matcherA_preds(matcherA_dir, "fold2")
    matcherA_pred_all = np.concatenate([matcherA1, matcherA2])
    matcherA_valid_all = matcherA_pred_all != NONE_CLASS
    net_valid_all = np.ones_like(y_test_all, dtype=bool)

    breakdown_for, targets = run_0Oo_breakdown_fair(
        dump_dir, matcherA_dir, real_stems, y_real, classes, fold_a, fold_b)
    oOo_net = breakdown_for(y_test_all, net_pred_all, net_valid_all, "net+realA")
    oOo_matcherA = breakdown_for(y_test_all, matcherA_pred_all, matcherA_valid_all, "matcher+realA")

    results = {
        "device": device_desc,
        "fold_assignment_path": str(args.fold_assignment),
        "n_pages": len(fold_of),
        "fold_a_pages": sorted(fold_a),
        "fold_b_pages": sorted(fold_b),
        "n_multipage_clusters": n_multipage_clusters,
        "max_epochs": args.max_epochs,
        "patience": args.patience,
        "batch_size": args.batch_size,
        "fold1": strip_private(fold1),
        "fold2": strip_private(fold2),
        "mean": {
            "net_acc": (fold1["net_acc"] + fold2["net_acc"]) / 2,
            "matcher_acc_no_realA": (fold1["matcher_acc"] + fold2["matcher_acc"]) / 2,
            "matcherA_acc": (fold1["matcherA_acc"] + fold2["matcherA_acc"]) / 2,
        },
        "0Oo_breakdown": {"net_realA": oOo_net, "matcherA": oOo_matcherA},
    }

    out_path = matcherA_dir / "report3_folds.json"
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(results, f, indent=2)
    print(f"train3.py: wrote {out_path}", file=sys.stderr)
    print(f"train3.py: mean net+realA={results['mean']['net_acc']:.4f} "
          f"matcher(no-realA)={results['mean']['matcher_acc_no_realA']:.4f} "
          f"matcher+realA={results['mean']['matcherA_acc']:.4f}", file=sys.stderr)


if __name__ == "__main__":
    main()
