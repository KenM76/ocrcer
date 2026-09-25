#!/usr/bin/env python3
"""Throwaway neural probe: trains on Rust-dumped features, never computes
one (`docs/ARCHITECTURE.md` section 11, "Candidate: a throwaway neural
probe before chunk 15"; `CLAUDE.md` rule 4).

Every `G` (32x32 grid) and `X` (107-dim vector, standardised by the loaded
model's own mean/sd) this script sees was produced by
`crates/ocrcer-bench/src/bin/nn_dump.rs` calling
`ocrcer_core::feature::extract_with_grid` -- the same extractor the bank and
the runtime matcher use. This script has no rasteriser, no binarizer, no
segmenter, and does not open a font, a page image, or the score set.

Usage:
    python tools/nnprobe/train.py [--dump-dir DIR] [--epochs N] [--device auto|cpu|xpu]

Writes `<dump-dir>/report.json` (private, not committed) and prints a
summary to stdout.
"""
from __future__ import annotations

import argparse
import json
import random
import sys
import time
from pathlib import Path

import numpy as np

try:
    import torch
    import torch.nn as nn
    import torch.nn.functional as F
except ImportError as e:
    print(f"train.py: PyTorch not importable: {e}", file=sys.stderr)
    sys.exit(1)

SEED = 20260925
FEATURE_DIMS = 107
GRID = 32
NONE_CLASS = 0xFFFF


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
        print("train.py: --device xpu requested but torch.xpu.is_available() is False; "
              "falling back to cpu", file=sys.stderr)
        return torch.device("cpu"), "cpu (xpu unavailable)"
    if requested == "auto" and not has_xpu:
        return torch.device("cpu"), "cpu (auto, no xpu)"
    # xpu path: cap memory at <=10 GB per the operator's ceiling.
    try:
        props = torch.xpu.get_device_properties(0)
        total = getattr(props, "total_memory", None)
        if total:
            frac = min(1.0, 10e9 / total)
            torch.xpu.set_per_process_memory_fraction(frac, 0)
            print(f"train.py: xpu device 0 total_memory={total/1e9:.1f} GB, "
                  f"capped to fraction={frac:.3f} (<=10 GB)", file=sys.stderr)
    except Exception as e:  # pragma: no cover - best-effort cap
        print(f"train.py: could not query/cap xpu memory ({e}); relying on small batch size",
              file=sys.stderr)
    return torch.device("xpu"), "xpu"


def load_split(dump_dir: Path, prefix: str, has_top1: bool):
    g = np.fromfile(dump_dir / f"{prefix}_G.f32", dtype="<f4")
    x = np.fromfile(dump_dir / f"{prefix}_X.f32", dtype="<f4")
    y = np.fromfile(dump_dir / f"{prefix}_y.u16", dtype="<u2")
    n = y.shape[0]
    g = g.reshape(n, GRID, GRID)
    x = x.reshape(n, FEATURE_DIMS)
    top1 = None
    if has_top1:
        top1 = np.fromfile(dump_dir / f"{prefix}_top1.u16", dtype="<u2")
        assert top1.shape[0] == n, f"{prefix}_top1.u16 has {top1.shape[0]} rows, expected {n}"
    return g, x, y, top1


def load_classes(dump_dir: Path):
    classes = {}
    with open(dump_dir / "classes.tsv", encoding="utf-8") as f:
        next(f)
        for line in f:
            idx, cp, cat = line.rstrip("\n").split("\t")
            classes[int(idx)] = (int(cp), cat)
    return classes


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


class Net(nn.Module):
    """conv3x3x16-relu-pool-conv3x3x32-relu-pool-dense(2048+107->128)-relu-dense(->n_classes),
    per `docs/ARCHITECTURE.md` section 11's 2026-09-24 architecture contract.
    `use_geometry=False` is the ablation: the 107-dim input is dropped at the
    dense head instead of concatenated.
    """

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
        h = g.unsqueeze(1)  # N,1,32,32
        h = F.max_pool2d(F.relu(self.conv1(h)), 2)
        h = F.max_pool2d(F.relu(self.conv2(h)), 2)
        h = h.flatten(1)
        if self.use_geometry:
            h = torch.cat([h, x], dim=1)
        h = F.relu(self.fc1(h))
        return self.fc2(h)


def train_one(model, device, g_train, x_train, y_train, g_val, x_val, y_val, epochs, batch_size, tag):
    model.to(device)
    opt = torch.optim.Adam(model.parameters(), lr=1e-3)
    n = y_train.shape[0]
    g_t = torch.from_numpy(g_train)
    x_t = torch.from_numpy(x_train)
    y_t = torch.from_numpy(y_train.astype(np.int64))
    gen = torch.Generator().manual_seed(SEED)

    for epoch in range(epochs):
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
        print(f"[{tag}] epoch {epoch+1}/{epochs} loss={total_loss/n:.4f} "
              f"train_acc={train_acc:.4f} render_val_acc={val_acc:.4f}", file=sys.stderr)
    return model


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
def predict(model, device, g, x, batch_size):
    model.eval()
    g_t = torch.from_numpy(g)
    x_t = torch.from_numpy(x)
    n = g.shape[0]
    out_all = np.empty(n, dtype=np.int64)
    for i in range(0, n, batch_size):
        gb = g_t[i : i + batch_size].to(device)
        xb = x_t[i : i + batch_size].to(device)
        out = model(gb, xb)
        out_all[i : i + gb.shape[0]] = out.argmax(1).cpu().numpy()
    return out_all


def top_confusions(true_idx, pred_idx, classes, k=10):
    from collections import Counter

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
    return {grp: {"n": n, "correct": c, "acc": (c / n if n else float("nan"))} for grp, (n, c) in groups.items()}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dump-dir", default="D:/Dev/ExcludedPrivate/ocrcer/nnprobe")
    ap.add_argument("--epochs", type=int, default=12)
    ap.add_argument("--batch-size", type=int, default=256)
    ap.add_argument("--device", default="auto", choices=["auto", "cpu", "xpu"])
    ap.add_argument("--wide", action="store_true", help="also train one wider variant")
    args = ap.parse_args()

    set_seed(SEED)
    dump_dir = Path(args.dump_dir)
    device, device_desc = pick_device(args.device)
    print(f"train.py: device = {device_desc}", file=sys.stderr)

    classes = load_classes(dump_dir)
    n_classes = max(classes.keys()) + 1

    t0 = time.time()
    g_train, x_train, y_train, _ = load_split(dump_dir, "bank_train", has_top1=False)
    g_val, x_val, y_val, _ = load_split(dump_dir, "bank_val", has_top1=False)
    g_real, x_real, y_real, top1_real = load_split(dump_dir, "real", has_top1=True)
    print(f"train.py: loaded bank_train={y_train.shape[0]} bank_val={y_val.shape[0]} "
          f"real={y_real.shape[0]} in {time.time()-t0:.1f}s", file=sys.stderr)

    results = {"device": device_desc, "n_classes": n_classes,
               "bank_train_n": int(y_train.shape[0]), "bank_val_n": int(y_val.shape[0]),
               "real_n": int(y_real.shape[0])}

    variants = [("with_geometry", True, False), ("no_geometry", False, False)]
    if args.wide:
        variants.append(("wide_with_geometry", True, True))

    real_true = y_real.astype(np.int64)

    # Prototype matcher, already computed by nn_dump.rs on the identical
    # extracted features -- not recomputed here.
    matcher_pred = top1_real.astype(np.int64)
    matcher_valid = matcher_pred != NONE_CLASS
    results["matcher"] = {
        "overall_acc": float((matcher_pred[matcher_valid] == real_true[matcher_valid]).mean()) if matcher_valid.any() else float("nan"),
        "n_no_match": int((~matcher_valid).sum()),
        "per_group": per_group_accuracy(real_true[matcher_valid], matcher_pred[matcher_valid], classes),
        "top_confusions": top_confusions(real_true[matcher_valid], matcher_pred[matcher_valid], classes),
    }

    for tag, use_geom, wide in variants:
        t1 = time.time()
        model = Net(n_classes, use_geometry=use_geom, wide=wide)
        model = train_one(model, device, g_train, x_train, y_train, g_val, x_val, y_val,
                           args.epochs, args.batch_size, tag)
        wall = time.time() - t1
        render_val_acc = evaluate_acc(model, device, g_val, x_val, y_val, args.batch_size)
        pred_real = predict(model, device, g_real, x_real, args.batch_size)
        real_acc = float((pred_real == real_true).mean()) if real_true.shape[0] else float("nan")
        results[tag] = {
            "wall_s": wall,
            "render_val_acc": render_val_acc,
            "real_overall_acc": real_acc,
            "per_group": per_group_accuracy(real_true, pred_real, classes),
            "top_confusions": top_confusions(real_true, pred_real, classes),
        }
        print(f"train.py: [{tag}] wall={wall:.1f}s render_val_acc={render_val_acc:.4f} "
              f"real_overall_acc={real_acc:.4f}", file=sys.stderr)

    out_path = dump_dir / "report.json"
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(results, f, indent=2)
    print(f"train.py: wrote {out_path}", file=sys.stderr)
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
