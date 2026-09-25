#!/usr/bin/env python3
"""Round 2 of the throwaway neural probe
(`docs/measurements/2026-09-25_nn_probe.md` "Round 2"; `docs/ARCHITECTURE.md`
section 11). Answers the architect's four objections to round 1's headline
number. Still computes nothing from pixels -- every `G`/`X`/`y`/`topk_*`
array here was produced by `crates/ocrcer-bench/src/bin/nn_dump.rs` calling
`ocrcer_core::feature::extract_with_grid`, the same extractor the bank and
runtime matcher use (`CLAUDE.md` rule 4). This script's only inputs beyond
those dumps are page identifiers (for the page-disjoint fold split) and
class metadata -- no rasteriser, no binarizer, no segmenter.

Usage:
    python tools/nnprobe/train2.py --dump-dir DIR --mode {convergence,folds,all}
        [--max-epochs N] [--patience N] [--device auto|cpu|xpu]

Writes `<dump-dir>/report2_convergence.json` and/or
`<dump-dir>/report2_folds.json` (private, not committed).
"""
from __future__ import annotations

import argparse
import copy
import json
import random
import sys
import time
from collections import Counter
from pathlib import Path

import numpy as np

try:
    import torch
    import torch.nn as nn
    import torch.nn.functional as F
except ImportError as e:
    print(f"train2.py: PyTorch not importable: {e}", file=sys.stderr)
    sys.exit(1)

SEED = 20260925
FOLD_SEED = 20260925  # separate Random instance, same numeral by convention
FEATURE_DIMS = 107
GRID = 32
NONE_CLASS = 0xFFFF
TOPK = 5


def set_seed(seed: int) -> None:
    random.seed(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    torch.use_deterministic_algorithms(False)  # pooling on XPU may lack a deterministic kernel


def pick_device(requested: str):
    if requested == "cpu":
        return torch.device("cpu"), "cpu (requested)"
    has_xpu = hasattr(torch, "xpu") and torch.xpu.is_available()
    if requested == "xpu" and not has_xpu:
        print("train2.py: --device xpu requested but torch.xpu.is_available() is False; "
              "falling back to cpu", file=sys.stderr)
        return torch.device("cpu"), "cpu (xpu unavailable)"
    if requested == "auto" and not has_xpu:
        return torch.device("cpu"), "cpu (auto, no xpu)"
    try:
        props = torch.xpu.get_device_properties(0)
        total = getattr(props, "total_memory", None)
        if total:
            frac = min(1.0, 10e9 / total)
            torch.xpu.set_per_process_memory_fraction(frac, 0)
            print(f"train2.py: xpu device 0 total_memory={total/1e9:.1f} GB, "
                  f"capped to fraction={frac:.3f} (<=10 GB)", file=sys.stderr)
    except Exception as e:  # pragma: no cover - best-effort cap
        print(f"train2.py: could not query/cap xpu memory ({e}); relying on small batch size",
              file=sys.stderr)
    return torch.device("xpu"), "xpu"


# ---------------------------------------------------------------------
# Loading
# ---------------------------------------------------------------------

def load_gxy(dump_dir: Path, prefix: str):
    g = np.fromfile(dump_dir / f"{prefix}_G.f32", dtype="<f4")
    x = np.fromfile(dump_dir / f"{prefix}_X.f32", dtype="<f4")
    y = np.fromfile(dump_dir / f"{prefix}_y.u16", dtype="<u2")
    n = y.shape[0]
    g = g.reshape(n, GRID, GRID)
    x = x.reshape(n, FEATURE_DIMS)
    return g, x, y


def load_u16_col(dump_dir: Path, name: str, n: int, k: int = 1):
    a = np.fromfile(dump_dir / name, dtype="<u2")
    assert a.shape[0] == n * k, f"{name} has {a.shape[0]} entries, expected {n*k}"
    return a.reshape(n, k) if k > 1 else a


def load_f32_col(dump_dir: Path, name: str, n: int, k: int = 1):
    a = np.fromfile(dump_dir / name, dtype="<f4")
    assert a.shape[0] == n * k, f"{name} has {a.shape[0]} entries, expected {n*k}"
    return a.reshape(n, k) if k > 1 else a


def load_classes(dump_dir: Path):
    classes = {}
    with open(dump_dir / "classes.tsv", encoding="utf-8") as f:
        next(f)
        for line in f:
            idx, cp, cat = line.rstrip("\n").split("\t")
            classes[int(idx)] = (int(cp), cat)
    return classes


def load_real_stems(dump_dir: Path, n: int):
    stems = []
    with open(dump_dir / "real_meta.tsv", encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            stems.append(parts[1])
    assert len(stems) == n, f"real_meta.tsv has {len(stems)} rows, expected {n}"
    return stems


def group_of(codepoint: int, category: str):
    ch = chr(codepoint)
    if ch in ("0", "O", "o"):
        return "0/O/o"
    if ch in ("l", "1", "I"):
        return "l/1/I"
    if category == "digit":
        return "digits"
    if category == "lower":
        return "lowercase"
    if category == "upper":
        return "uppercase"
    if category == "punct":
        return "punctuation"
    return None


# ---------------------------------------------------------------------
# Model (identical architecture to round 1's Net, per the section-11
# contract; unchanged here so round-1 and round-2 numbers stay comparable)
# ---------------------------------------------------------------------

class Net(nn.Module):
    def __init__(self, n_classes: int, use_geometry: bool = True, wide: bool = False):
        super().__init__()
        self.use_geometry = use_geometry
        c1, c2, hidden = (16, 32, 128) if not wide else (32, 64, 256)
        self.conv1 = nn.Conv2d(1, c1, 3, padding=1)
        self.conv2 = nn.Conv2d(c1, c2, 3, padding=1)
        flat = c2 * (GRID // 4) * (GRID // 4)
        head_in = flat + (FEATURE_DIMS if use_geometry else 0)
        self.fc1 = nn.Linear(head_in, hidden)
        self.fc2 = nn.Linear(hidden, n_classes)

    def forward(self, g, x):
        h = g.unsqueeze(1)
        h = F.max_pool2d(F.relu(self.conv1(h)), 2)
        h = F.max_pool2d(F.relu(self.conv2(h)), 2)
        h = h.flatten(1)
        if self.use_geometry:
            h = torch.cat([h, x], dim=1)
        h = F.relu(self.fc1(h))
        return self.fc2(h)


def train_to_convergence(model, device, g_train, x_train, y_train, g_val, x_val, y_val,
                          max_epochs, batch_size, patience, tag, lr=1e-3,
                          step_size=15, gamma=0.5):
    """Adam + StepLR, early stopping on render-val top-1 (best-checkpoint
    restore). Addresses task item 1: round 1 trained a fixed 12 epochs with
    a constant LR; this trains until render-val stops improving, decaying
    the LR every `step_size` epochs so late epochs can take smaller steps
    instead of oscillating around whatever local optimum 12 epochs reached.
    """
    model.to(device)
    opt = torch.optim.Adam(model.parameters(), lr=lr)
    sched = torch.optim.lr_scheduler.StepLR(opt, step_size=step_size, gamma=gamma)
    n = y_train.shape[0]
    g_t = torch.from_numpy(g_train)
    x_t = torch.from_numpy(x_train)
    y_t = torch.from_numpy(y_train.astype(np.int64))
    gen = torch.Generator().manual_seed(SEED)

    best_val = -1.0
    best_epoch = 0
    best_state = None
    stale = 0
    curve = []

    for epoch in range(max_epochs):
        model.train()
        perm = torch.randperm(n, generator=gen)
        total_loss = 0.0
        correct = 0
        for i in range(0, n, batch_size):
            idx = perm[i : i + batch_size]
            gb = g_t[idx].to(device)
            xb = x_t[idx].to(device)
            yb = y_t[idx].to(device)
            opt.zero_grad()
            out = model(gb, xb)
            loss = F.cross_entropy(out, yb)
            loss.backward()
            opt.step()
            total_loss += loss.item() * idx.shape[0]
            correct += (out.argmax(1) == yb).sum().item()
        train_acc = correct / n
        val_acc = evaluate_acc(model, device, g_val, x_val, y_val, batch_size)
        lr_now = sched.get_last_lr()[0]
        curve.append({"epoch": epoch + 1, "loss": total_loss / n, "train_acc": train_acc,
                       "render_val_acc": val_acc, "lr": lr_now})
        print(f"[{tag}] epoch {epoch+1}/{max_epochs} loss={total_loss/n:.4f} "
              f"train_acc={train_acc:.4f} render_val_acc={val_acc:.4f} lr={lr_now:.2e}",
              file=sys.stderr)
        if val_acc > best_val + 1e-4:
            best_val = val_acc
            best_epoch = epoch + 1
            best_state = copy.deepcopy(model.state_dict())
            stale = 0
        else:
            stale += 1
        sched.step()
        if stale >= patience:
            print(f"[{tag}] early stop at epoch {epoch+1} "
                  f"(no render-val improvement for {patience} epochs; "
                  f"best={best_val:.4f} @ epoch {best_epoch})", file=sys.stderr)
            break

    model.load_state_dict(best_state)
    return model, curve, best_val, best_epoch


@torch.no_grad()
def evaluate_acc(model, device, g, x, y, batch_size):
    model.eval()
    g_t = torch.from_numpy(g)
    x_t = torch.from_numpy(x)
    y_t = torch.from_numpy(y.astype(np.int64))
    n = y.shape[0]
    correct = 0
    for i in range(0, n, batch_size):
        gb = g_t[i : i + batch_size].to(device)
        xb = x_t[i : i + batch_size].to(device)
        yb = y_t[i : i + batch_size].to(device)
        out = model(gb, xb)
        correct += (out.argmax(1) == yb).sum().item()
    return correct / n if n else float("nan")


@torch.no_grad()
def predict_with_conf(model, device, g, x, batch_size):
    """Returns (pred_class, neg_log_p_of_pred) for every row."""
    model.eval()
    g_t = torch.from_numpy(g)
    x_t = torch.from_numpy(x)
    n = g.shape[0]
    pred = np.empty(n, dtype=np.int64)
    neg_log_p = np.empty(n, dtype=np.float64)
    for i in range(0, n, batch_size):
        gb = g_t[i : i + batch_size].to(device)
        xb = x_t[i : i + batch_size].to(device)
        out = model(gb, xb)
        logp = F.log_softmax(out, dim=1)
        top = logp.argmax(1)
        chosen = logp.gather(1, top.unsqueeze(1)).squeeze(1)
        pred[i : i + gb.shape[0]] = top.cpu().numpy()
        neg_log_p[i : i + gb.shape[0]] = (-chosen).cpu().double().numpy()
    return pred, neg_log_p


def per_group_accuracy(true_idx, pred_idx, classes):
    groups = {}
    for t, p in zip(true_idx, pred_idx):
        cp, cat = classes.get(int(t), (0, "?"))
        grp = group_of(cp, cat)
        if grp is None:
            grp = "_other"
        b = groups.setdefault(grp, [0, 0])
        b[0] += 1
        if int(t) == int(p):
            b[1] += 1
    return {grp: {"n": n, "correct": c, "acc": (c / n if n else float("nan"))}
            for grp, (n, c) in groups.items()}


def top_confusions(true_idx, pred_idx, classes, k=10):
    c = Counter()
    for t, p in zip(true_idx, pred_idx):
        if t != p:
            c[(int(t), int(p))] += 1
    out = []
    for (t, p), n in c.most_common(k):
        tcp, _ = classes.get(t, (0, "?"))
        pcp, _ = classes.get(p, (0, "?"))
        tch = chr(tcp) if t in classes else "?"
        pch = chr(pcp) if p in classes else "?"
        out.append({"true": tch, "pred": pch, "n": n})
    return out


# ---------------------------------------------------------------------
# Item 1: convergence (bank_train only, matches round 1's inputs exactly)
# ---------------------------------------------------------------------

def run_convergence(dump_dir: Path, device, device_desc: str, max_epochs: int,
                     patience: int, batch_size: int, step_size: int, gamma: float):
    classes = load_classes(dump_dir)
    n_classes = max(classes.keys()) + 1

    g_train, x_train, y_train = load_gxy(dump_dir, "bank_train")
    g_val, x_val, y_val = load_gxy(dump_dir, "bank_val")
    n_val = y_val.shape[0]
    matcher_val_top1 = load_u16_col(dump_dir, "bank_val_top1.u16", n_val)

    matcher_valid = matcher_val_top1 != NONE_CLASS
    matcher_acc = float((matcher_val_top1[matcher_valid] == y_val[matcher_valid]).mean()) \
        if matcher_valid.any() else float("nan")

    results = {
        "device": device_desc,
        "bank_train_n": int(y_train.shape[0]),
        "bank_val_n": int(n_val),
        "max_epochs": max_epochs,
        "patience": patience,
        "batch_size": batch_size,
        "step_size": step_size,
        "gamma": gamma,
        "matcher_render_val": {
            "overall_acc": matcher_acc,
            "n_no_match": int((~matcher_valid).sum()),
            "per_group": per_group_accuracy(y_val[matcher_valid], matcher_val_top1[matcher_valid], classes),
        },
    }
    print(f"train2.py: matcher render_val top-1 = {matcher_acc:.4f} "
          f"(n_no_match={int((~matcher_valid).sum())})", file=sys.stderr)

    for tag, use_geom in [("with_geometry", True), ("no_geometry", False)]:
        t0 = time.time()
        model = Net(n_classes, use_geometry=use_geom)
        model, curve, best_val, best_epoch = train_to_convergence(
            model, device, g_train, x_train, y_train, g_val, x_val, y_val,
            max_epochs, batch_size, patience, tag, step_size=step_size, gamma=gamma)
        wall = time.time() - t0
        results[tag] = {
            "wall_s": wall,
            "epochs_run": len(curve),
            "best_epoch": best_epoch,
            "best_render_val_acc": best_val,
            "curve": curve,
        }
        print(f"train2.py: [{tag}] wall={wall:.1f}s epochs_run={len(curve)} "
              f"best_epoch={best_epoch} best_render_val_acc={best_val:.4f}", file=sys.stderr)

    out_path = dump_dir / "report2_convergence.json"
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(results, f, indent=2)
    print(f"train2.py: wrote {out_path}", file=sys.stderr)
    return results


# ---------------------------------------------------------------------
# Item 2/3/4: page-disjoint folds + real training data + fusion + oracle
# ---------------------------------------------------------------------

def fold_split_stems(stems):
    uniq = sorted(set(stems))
    rng = random.Random(FOLD_SEED)
    shuffled = uniq[:]
    rng.shuffle(shuffled)
    half = len(shuffled) // 2
    fold_a = set(shuffled[:half])
    fold_b = set(shuffled[half:])
    return fold_a, fold_b, uniq


def run_one_fold(tag, dump_dir, device, device_desc, classes, n_classes,
                  g_train, x_train, y_train,
                  g_val, x_val, y_val,
                  g_real, x_real, y_real, real_topk_class, real_topk_dist, real_stems,
                  train_stems, test_stems,
                  max_epochs, patience, batch_size, step_size, gamma):
    train_mask = np.array([s in train_stems for s in real_stems])
    test_mask = np.array([s in test_stems for s in real_stems])

    g_extra, x_extra, y_extra = g_real[train_mask], x_real[train_mask], y_real[train_mask]
    g_tr = np.concatenate([g_train, g_extra], axis=0)
    x_tr = np.concatenate([x_train, x_extra], axis=0)
    y_tr = np.concatenate([y_train, y_extra], axis=0)

    y_test = y_real[test_mask].astype(np.int64)
    g_test = g_real[test_mask]
    x_test = x_real[test_mask]
    matcher_topk_c = real_topk_class[test_mask]
    matcher_topk_d = real_topk_dist[test_mask]
    matcher_top1 = matcher_topk_c[:, 0]

    t0 = time.time()
    model = Net(n_classes, use_geometry=True)
    model, curve, best_val, best_epoch = train_to_convergence(
        model, device, g_tr, x_tr, y_tr, g_val, x_val, y_val,
        max_epochs, batch_size, patience, tag, step_size=step_size, gamma=gamma)
    wall = time.time() - t0

    net_pred, net_neg_log_p = predict_with_conf(model, device, g_test, x_test, batch_size)

    matcher_valid = matcher_top1 != NONE_CLASS
    matcher_correct = (matcher_top1 == y_test) & matcher_valid
    net_correct = net_pred == y_test

    net_acc = float(net_correct.mean())
    matcher_acc = float(matcher_correct[matcher_valid].sum() / matcher_valid.sum()) if matcher_valid.any() else float("nan")

    oracle_matcher_wrong_net_right = int((~matcher_correct & net_correct).sum())
    oracle_net_wrong_matcher_right = int((matcher_correct & ~net_correct).sum())
    oracle_both_right = int((matcher_correct & net_correct).sum())
    oracle_both_wrong = int((~matcher_correct & ~net_correct).sum())
    n_test = int(y_test.shape[0])

    return {
        "tag": tag,
        "wall_s": wall,
        "epochs_run": len(curve),
        "best_epoch": best_epoch,
        "best_render_val_acc": best_val,
        "curve": curve,
        "train_n": int(y_tr.shape[0]),
        "train_extra_real_n": int(y_extra.shape[0]),
        "test_n": n_test,
        "net_acc": net_acc,
        "matcher_acc": matcher_acc,
        "matcher_n_no_match": int((~matcher_valid).sum()),
        "net_per_group": per_group_accuracy(y_test, net_pred, classes),
        "matcher_per_group": per_group_accuracy(y_test[matcher_valid], matcher_top1[matcher_valid], classes),
        "net_top_confusions": top_confusions(y_test, net_pred, classes),
        "matcher_top_confusions": top_confusions(y_test[matcher_valid], matcher_top1[matcher_valid], classes),
        "oracle": {
            "both_right": oracle_both_right,
            "both_wrong": oracle_both_wrong,
            "matcher_wrong_net_right": oracle_matcher_wrong_net_right,
            "net_wrong_matcher_right": oracle_net_wrong_matcher_right,
            "matcher_wrong_net_right_frac": oracle_matcher_wrong_net_right / n_test if n_test else float("nan"),
            "net_wrong_matcher_right_frac": oracle_net_wrong_matcher_right / n_test if n_test else float("nan"),
            "oracle_union_acc": (oracle_both_right + oracle_matcher_wrong_net_right + oracle_net_wrong_matcher_right) / n_test if n_test else float("nan"),
        },
        # returned for the caller to fit/apply the fusion scale across folds
        "_matcher_top1": matcher_top1,
        "_matcher_d1": matcher_topk_d[:, 0].astype(np.float64),
        "_matcher_valid": matcher_valid,
        "_net_pred": net_pred,
        "_net_neg_log_p": net_neg_log_p,
        "_y_test": y_test,
    }


def fit_scale(matcher_d1, net_neg_log_p, valid_mask):
    """Ordinary least squares: matcher_d1 ~ a * net_neg_log_p + b, fit on
    rows where the matcher produced a distance at all. This is the
    'net's -log p scaled into distance units' conversion the task specifies,
    fit on one fold and applied (never refit) on the other.
    """
    x = net_neg_log_p[valid_mask]
    y = matcher_d1[valid_mask]
    A = np.vstack([x, np.ones_like(x)]).T
    (a, b), *_ = np.linalg.lstsq(A, y, rcond=None)
    return float(a), float(b)


def apply_fusion(fold_result, a, b, classes):
    """Convert net's -log p to matcher-distance units with (a, b) fit on the
    OTHER fold, then per row: if matcher and net agree, use that class;
    otherwise take whichever classifier reports the smaller distance in the
    shared unit. Rows with no matcher candidate fall back to the net.
    """
    matcher_top1 = fold_result["_matcher_top1"]
    matcher_d1 = fold_result["_matcher_d1"]
    matcher_valid = fold_result["_matcher_valid"]
    net_pred = fold_result["_net_pred"]
    net_neg_log_p = fold_result["_net_neg_log_p"]
    y_test = fold_result["_y_test"]

    net_dist_equiv = a * net_neg_log_p + b

    fused_pred = np.where(matcher_valid & (net_dist_equiv < matcher_d1), net_pred, matcher_top1)
    fused_pred = np.where(~matcher_valid, net_pred, fused_pred)

    fused_correct = fused_pred == y_test
    n_test = y_test.shape[0]
    return {
        "scale_a": a,
        "scale_b": b,
        "fusion_acc": float(fused_correct.mean()),
        "fusion_per_group": per_group_accuracy(y_test, fused_pred, classes),
        "n": int(n_test),
    }


def run_folds(dump_dir: Path, device, device_desc: str, max_epochs: int, patience: int,
              batch_size: int, step_size: int, gamma: float):
    classes = load_classes(dump_dir)
    n_classes = max(classes.keys()) + 1

    g_train, x_train, y_train = load_gxy(dump_dir, "bank_train")
    g_val, x_val, y_val = load_gxy(dump_dir, "bank_val")
    g_real, x_real, y_real = load_gxy(dump_dir, "real")
    n_real = y_real.shape[0]
    real_topk_class = load_u16_col(dump_dir, "real_topk_class.u16", n_real, TOPK)
    real_topk_dist = load_f32_col(dump_dir, "real_topk_dist.f32", n_real, TOPK)
    real_stems = load_real_stems(dump_dir, n_real)

    fold_a, fold_b, uniq_stems = fold_split_stems(real_stems)

    print(f"train2.py: {len(uniq_stems)} unique real pages -> "
          f"fold A={len(fold_a)} fold B={len(fold_b)} (seed={FOLD_SEED})", file=sys.stderr)

    fold1 = run_one_fold("fold1(train=A,test=B)", dump_dir, device, device_desc, classes, n_classes,
                          g_train, x_train, y_train, g_val, x_val, y_val,
                          g_real, x_real, y_real, real_topk_class, real_topk_dist, real_stems,
                          fold_a, fold_b, max_epochs, patience, batch_size, step_size, gamma)
    fold2 = run_one_fold("fold2(train=B,test=A)", dump_dir, device, device_desc, classes, n_classes,
                          g_train, x_train, y_train, g_val, x_val, y_val,
                          g_real, x_real, y_real, real_topk_class, real_topk_dist, real_stems,
                          fold_b, fold_a, max_epochs, patience, batch_size, step_size, gamma)

    # Fusion: scale fit on one fold's data, applied to the OTHER fold's test
    # rows (the task's "scale fitted on real-A, tested on real-B" -- since
    # fold1's *train* side is real-A and fold2's *test* side is real-A, the
    # scale for testing on fold1 is fit on fold2's test rows (= real-A) and
    # vice versa, keeping fit and test always on disjoint pages).
    a1, b1 = fit_scale(fold2["_matcher_d1"], fold2["_net_neg_log_p"], fold2["_matcher_valid"])
    fusion1 = apply_fusion(fold1, a1, b1, classes)
    a2, b2 = fit_scale(fold1["_matcher_d1"], fold1["_net_neg_log_p"], fold1["_matcher_valid"])
    fusion2 = apply_fusion(fold2, a2, b2, classes)

    def strip_private(d):
        return {k: v for k, v in d.items() if not k.startswith("_")}

    results = {
        "device": device_desc,
        "fold_seed": FOLD_SEED,
        "n_pages": len(uniq_stems),
        "fold_a_pages": sorted(fold_a),
        "fold_b_pages": sorted(fold_b),
        "max_epochs": max_epochs,
        "patience": patience,
        "batch_size": batch_size,
        "fold1": strip_private(fold1),
        "fold2": strip_private(fold2),
        "fusion1": fusion1,
        "fusion2": fusion2,
        "mean": {
            "net_acc": (fold1["net_acc"] + fold2["net_acc"]) / 2,
            "matcher_acc": (fold1["matcher_acc"] + fold2["matcher_acc"]) / 2,
            "fusion_acc": (fusion1["fusion_acc"] + fusion2["fusion_acc"]) / 2,
        },
    }

    out_path = dump_dir / "report2_folds.json"
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(results, f, indent=2)
    print(f"train2.py: wrote {out_path}", file=sys.stderr)
    print(f"train2.py: fold1 net={fold1['net_acc']:.4f} matcher={fold1['matcher_acc']:.4f} "
          f"fusion={fusion1['fusion_acc']:.4f}", file=sys.stderr)
    print(f"train2.py: fold2 net={fold2['net_acc']:.4f} matcher={fold2['matcher_acc']:.4f} "
          f"fusion={fusion2['fusion_acc']:.4f}", file=sys.stderr)
    return results, fold1, fold2


# ---------------------------------------------------------------------
# Item 4: matcher-only 0/O/o breakdown on the full real set (round-1 style,
# not fold-restricted -- this reuses round 1's dump layout via real_top1
# when topk isn't needed, but we now have topk_class[:,0] == top1).
# ---------------------------------------------------------------------

def run_0Oo_breakdown(dump_dir: Path):
    classes = load_classes(dump_dir)
    class_of_cp = {cp: idx for idx, (cp, _cat) in classes.items()}
    _, _, y_real = load_gxy(dump_dir, "real")
    n_real = y_real.shape[0]
    real_topk_class = load_u16_col(dump_dir, "real_topk_class.u16", n_real, TOPK)
    matcher_top1 = real_topk_class[:, 0]

    targets = {}
    for ch in ("0", "O", "o"):
        cp = ord(ch)
        if cp in class_of_cp:
            targets[ch] = class_of_cp[cp]

    per_class = {}
    case_errors = 0
    digit_errors = 0
    other_errors = 0
    for ch, cls_idx in targets.items():
        mask = y_real == cls_idx
        n = int(mask.sum())
        pred = matcher_top1[mask]
        valid = pred != NONE_CLASS
        correct = int((pred == cls_idx).sum())
        wrong_mask = valid & (pred != cls_idx)
        wrong_preds = pred[wrong_mask]
        for p in wrong_preds:
            p = int(p)
            pcp = classes.get(p, (0, "?"))[0]
            pch = chr(pcp) if p in classes else "?"
            if ch in ("O", "o") and pch in ("O", "o"):
                case_errors += 1
            elif (ch == "0" and pch in ("O", "o")) or (ch in ("O", "o") and pch == "0"):
                digit_errors += 1
            else:
                other_errors += 1
        per_class[ch] = {
            "n": n,
            "correct": correct,
            "acc": correct / n if n else float("nan"),
            "n_wrong": int(wrong_mask.sum()),
        }

    total_errors = case_errors + digit_errors + other_errors
    return {
        "per_class": per_class,
        "error_breakdown": {
            "case_level (O<->o)": case_errors,
            "digit_vs_letter (0<->O/o)": digit_errors,
            "other": other_errors,
            "total_errors": total_errors,
            "case_level_frac": case_errors / total_errors if total_errors else float("nan"),
            "digit_vs_letter_frac": digit_errors / total_errors if total_errors else float("nan"),
        },
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dump-dir", default="D:/Dev/ExcludedPrivate/ocrcer/nnprobe2")
    ap.add_argument("--mode", default="all", choices=["convergence", "folds", "0Oo", "all"])
    ap.add_argument("--max-epochs", type=int, default=60)
    ap.add_argument("--patience", type=int, default=10)
    ap.add_argument("--batch-size", type=int, default=256)
    ap.add_argument("--step-size", type=int, default=15)
    ap.add_argument("--gamma", type=float, default=0.5)
    ap.add_argument("--device", default="auto", choices=["auto", "cpu", "xpu"])
    args = ap.parse_args()

    set_seed(SEED)
    dump_dir = Path(args.dump_dir)
    device, device_desc = pick_device(args.device)
    print(f"train2.py: device = {device_desc}", file=sys.stderr)

    if args.mode in ("convergence", "all"):
        run_convergence(dump_dir, device, device_desc, args.max_epochs, args.patience,
                         args.batch_size, args.step_size, args.gamma)
    if args.mode in ("folds", "all"):
        run_folds(dump_dir, device, device_desc, args.max_epochs, args.patience,
                   args.batch_size, args.step_size, args.gamma)
    if args.mode in ("0Oo", "all"):
        breakdown = run_0Oo_breakdown(dump_dir)
        out_path = dump_dir / "report2_0Oo.json"
        with open(out_path, "w", encoding="utf-8") as f:
            json.dump(breakdown, f, indent=2)
        print(f"train2.py: wrote {out_path}", file=sys.stderr)
        print(json.dumps(breakdown, indent=2))


if __name__ == "__main__":
    main()
