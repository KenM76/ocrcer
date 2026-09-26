#!/usr/bin/env python3
"""Chunk 15 trainer (`docs/ARCHITECTURE.md` section 11: "Chunk 15's trainer
may be Python", "Chunk 15 interfaces" item 1). Written fresh for chunk 15 --
not a copy of `tools/nnprobe/train*.py`, which is probe evidence only.

Reads *only* what `crates/ocrcer-bench/src/bin/nn15_dump.rs` wrote to its
`<out-dir>` (`classes.tsv`, `model_meta.json`, `summary.json`,
`bank_{train,val}_*`, `real_{train,val}_*`, `junk_nonpath_{train,val}_*`,
`junk_render_{train,val}_*`). It never reads a page, a truth file, a font, or
any fixture -- every G grid and every 107-dim feature vector already carries
the model's own normalisation, applied by Rust (`Model::standardise`). This
script does not normalise anything itself.

Architecture (the "Chunk 15 interfaces" item 1 contract, and the same shape
the probe used before the junk-output amendment changed the output width):

    G (1x32x32) -> conv3x3(1->16) -> relu -> maxpool2      \\
                -> conv3x3(16->32) -> relu -> maxpool2      -> flatten (2048)
    X (107, already standardised) -----------------------/  -> concat (2155)
    concat -> dense(2155->128) -> relu -> dense(128->n_outputs) -> log_softmax

`n_outputs = charset_len + 1`, `junk_index = charset_len` (the amendment).
`Conv2d(padding=1)`, `MaxPool2d(2)`, `torch.flatten(x, 1)`,
`cat([conv_flat, feats], 1)`, `log_softmax` -- all as the parallel `c15-nn`
and `c15-forward` branches were told to expect.

Layer indexing for `spec.json` and the `<layer_index>.<weight|bias>.f32`
files: the *full* ordered layer list (including non-parametric layers) is
numbered 0..10; only conv/dense layers (0, 3, 8, 10) have tensor files. This
indexing is this trainer's own choice -- cross-check it against the
`c15-nn-table` writer's expectation at merge time (same caveat as
`model_meta.json`'s `charset_sha256`, see `tools/nn/README.md`).

Usage:
    python tools/nn/train.py --data-dir <nn15-dump-dir> --out-dir <out> \\
        --device xpu --neg-ratio 2.0 --epochs 12

    python tools/nn/train.py --data-dir <nn15-dump-dir> --out-dir <out> \\
        --device cpu --deterministic --neg-ratio <chosen> --epochs 12
"""
from __future__ import annotations

import argparse
import hashlib
import json
import platform
import random
import struct
import sys
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

FEATURE_DIMS = 107
GRID_SIDE = 32
NONE_CLASS = 0xFFFF
TOPK = 5

HERE = Path(__file__).resolve().parent


# ---------------------------------------------------------------------
# Raw dump readers -- match `nn15_dump.rs`'s doc comment exactly: G is
# 32x32 f32 row-major, X is 107 f32, y/top1/topk_class are u16 (0xFFFF
# sentinel), topk_dist/ratio are f32 (+inf sentinel).
# ---------------------------------------------------------------------


def read_f32(path: Path, cols: int) -> np.ndarray:
    if not path.exists():
        return np.zeros((0, cols), dtype=np.float32)
    a = np.fromfile(path, dtype="<f4")
    if a.size == 0:
        return np.zeros((0, cols), dtype=np.float32)
    return a.reshape(-1, cols)


def read_u16(path: Path) -> np.ndarray:
    if not path.exists():
        return np.zeros((0,), dtype=np.uint16)
    return np.fromfile(path, dtype="<u2")


class Group:
    """One dumped `{prefix}_{G,X,y-or-none,meta}` source."""

    def __init__(self, g: np.ndarray, x: np.ndarray, y: np.ndarray, name: str):
        n = g.shape[0]
        assert x.shape[0] == n, f"{name}: G has {n} rows, X has {x.shape[0]}"
        assert y.shape[0] == n, f"{name}: G has {n} rows, y has {y.shape[0]}"
        self.g = g
        self.x = x
        self.y = y
        self.name = name

    def __len__(self) -> int:
        return self.g.shape[0]


def load_positive_group(data_dir: Path, prefix: str) -> Group:
    g = read_f32(data_dir / f"{prefix}_G.f32", GRID_SIDE * GRID_SIDE)
    x = read_f32(data_dir / f"{prefix}_X.f32", FEATURE_DIMS)
    y = read_u16(data_dir / f"{prefix}_y.u16")
    return Group(g, x, y, prefix)


def load_junk_group(data_dir: Path, prefix: str, junk_index: int) -> Group:
    g = read_f32(data_dir / f"{prefix}_G.f32", GRID_SIDE * GRID_SIDE)
    x = read_f32(data_dir / f"{prefix}_X.f32", FEATURE_DIMS)
    y = np.full((g.shape[0],), junk_index, dtype=np.uint16)
    return Group(g, x, y, prefix)


def load_classes(data_dir: Path) -> dict[int, tuple[int, str]]:
    """`classes.tsv`: `index\\tcodepoint_u32\\tcategory`, one row per charset
    class (`nn15_dump.rs`'s `write_classes_tsv`)."""
    out: dict[int, tuple[int, str]] = {}
    lines = (data_dir / "classes.tsv").read_text(encoding="utf-8").splitlines()
    for line in lines[1:]:
        idx, cp, cat = line.split("\t")
        out[int(idx)] = (int(cp), cat)
    return out


def group_of(codepoint: int, category: str) -> str | None:
    """The probe's class-group buckets (`tools/nnprobe/train.py`'s
    `group_of`), reused verbatim so deliverable 4's per-group report stays
    comparable to the probe result it follows up on (`ARCHITECTURE.md`
    section 11, "The neural probe's result")."""
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


def per_group_accuracy(
    true_idx: np.ndarray, pred_idx: np.ndarray, classes: dict[int, tuple[int, str]]
) -> dict:
    groups: dict[str, list[int]] = {}
    for t, p in zip(true_idx.tolist(), pred_idx.tolist()):
        cp, cat = classes.get(int(t), (0, "?"))
        grp = group_of(cp, cat) or "_other"
        b = groups.setdefault(grp, [0, 0])
        b[0] += 1
        if int(t) == int(p):
            b[1] += 1
    return {grp: {"n": n, "correct": c, "acc": (c / n if n else float("nan"))} for grp, (n, c) in groups.items()}


@torch.no_grad()
def predict_argmax(model: Net, device: str, g: np.ndarray, x: np.ndarray, batch_size: int) -> np.ndarray:
    if g.shape[0] == 0:
        return np.zeros((0,), dtype=np.int64)
    model.eval()
    dev = torch.device(device)
    out_all = []
    for start in range(0, g.shape[0], batch_size):
        gb = torch.from_numpy(g[start : start + batch_size].reshape(-1, 1, GRID_SIDE, GRID_SIDE)).to(dev)
        xb = torch.from_numpy(x[start : start + batch_size]).to(dev)
        out_all.append(model(gb, xb).argmax(dim=1).cpu().numpy())
    return np.concatenate(out_all, axis=0)


def load_matcher_top1(data_dir: Path, prefix: str) -> np.ndarray:
    """Matcher comparison column dumped alongside a val group, where present
    (`bank_val_top1.u16`, `real_{train,val}_top1.u16`). Absent for junk
    groups' matcher column, which is instead in `_top1.u16` + `_ratio.f32`
    (read separately by the junk-rejection report)."""
    return read_u16(data_dir / f"{prefix}_top1.u16")


# ---------------------------------------------------------------------
# Model -- exact layer order the "Chunk 15 interfaces" contract and the
# parallel `c15-forward` branch were told to expect.
# ---------------------------------------------------------------------


class Net(nn.Module):
    def __init__(self, n_outputs: int):
        super().__init__()
        self.conv1 = nn.Conv2d(1, 16, kernel_size=3, padding=1)
        self.conv2 = nn.Conv2d(16, 32, kernel_size=3, padding=1)
        self.pool = nn.MaxPool2d(2)
        self.fc1 = nn.Linear(32 * 8 * 8 + FEATURE_DIMS, 128)
        self.fc2 = nn.Linear(128, n_outputs)

    def forward(self, g: torch.Tensor, feats: torch.Tensor) -> torch.Tensor:
        x = self.pool(F.relu(self.conv1(g)))
        x = self.pool(F.relu(self.conv2(x)))
        x = torch.flatten(x, 1)
        x = torch.cat([x, feats], 1)
        x = F.relu(self.fc1(x))
        x = self.fc2(x)
        return F.log_softmax(x, dim=1)


LAYER_SPEC_TEMPLATE = [
    {"index": 0, "kind": "conv3x3", "in_channels": 1, "out_channels": 16},
    {"index": 1, "kind": "relu"},
    {"index": 2, "kind": "maxpool2"},
    {"index": 3, "kind": "conv3x3", "in_channels": 16, "out_channels": 32},
    {"index": 4, "kind": "relu"},
    {"index": 5, "kind": "maxpool2"},
    {"index": 6, "kind": "flatten"},
    {"index": 7, "kind": "concat_features", "feature_dims": FEATURE_DIMS},
    {"index": 8, "kind": "dense", "in_features": 32 * 8 * 8 + FEATURE_DIMS, "out_features": 128},
    {"index": 9, "kind": "relu"},
    {"index": 10, "kind": "dense", "in_features": 128, "out_features": None},  # filled with n_outputs
]


# ---------------------------------------------------------------------
# Determinism
# ---------------------------------------------------------------------


def set_determinism(seed: int, deterministic: bool, device: str) -> None:
    random.seed(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    if deterministic:
        if device != "cpu":
            raise SystemExit("--deterministic requires --device cpu (shipped weights are CPU-only, per contract)")
        torch.use_deterministic_algorithms(True)
        torch.set_num_threads(1)


# ---------------------------------------------------------------------
# Negative sampling to a target ratio (per-epoch, seeded, deterministic
# given seed+epoch -- no wall-clock or thread-order dependence).
# ---------------------------------------------------------------------


def sample_indices(n_available: int, n_wanted: int, seed: int) -> np.ndarray:
    rng = np.random.RandomState(seed)
    if n_available == 0:
        return np.zeros((0,), dtype=np.int64)
    if n_wanted <= n_available:
        return rng.choice(n_available, size=n_wanted, replace=False)
    return rng.choice(n_available, size=n_wanted, replace=True)


# ---------------------------------------------------------------------
# Training
# ---------------------------------------------------------------------


def train_one_run(
    data_dir: Path,
    device: str,
    deterministic: bool,
    seed: int,
    neg_ratio: float,
    epochs: int,
    lr: float,
    batch_size: int,
    n_classes: int,
) -> tuple[Net, dict]:
    set_determinism(seed, deterministic, device)
    junk_index = n_classes

    bank_train = load_positive_group(data_dir, "bank_train")
    real_train = load_positive_group(data_dir, "real_train")
    nonpath_train = load_junk_group(data_dir, "junk_nonpath_train", junk_index)
    render_train = load_junk_group(data_dir, "junk_render_train", junk_index)

    pos_g = np.concatenate([bank_train.g, real_train.g], axis=0)
    pos_x = np.concatenate([bank_train.x, real_train.x], axis=0)
    pos_y = np.concatenate([bank_train.y, real_train.y], axis=0)

    neg_g_pool = np.concatenate([nonpath_train.g, render_train.g], axis=0)
    neg_x_pool = np.concatenate([nonpath_train.x, render_train.x], axis=0)
    n_pos = pos_g.shape[0]
    n_neg_pool = neg_g_pool.shape[0]
    n_neg_wanted = int(round(n_pos * neg_ratio))

    dev = torch.device(device)
    model = Net(junk_index + 1).to(dev)
    opt = torch.optim.Adam(model.parameters(), lr=lr)
    loss_fn = nn.NLLLoss()

    pos_g_t = torch.from_numpy(pos_g.reshape(-1, 1, GRID_SIDE, GRID_SIDE))
    pos_x_t = torch.from_numpy(pos_x)
    pos_y_t = torch.from_numpy(pos_y.astype(np.int64))
    neg_g_full = torch.from_numpy(neg_g_pool.reshape(-1, 1, GRID_SIDE, GRID_SIDE)) if n_neg_pool else None
    neg_x_full = torch.from_numpy(neg_x_pool) if n_neg_pool else None

    history = []
    t0 = time.time()
    for epoch in range(epochs):
        model.train()
        neg_idx = sample_indices(n_neg_pool, n_neg_wanted, seed=seed * 1_000_003 + epoch)
        if n_neg_pool:
            epoch_g = torch.cat([pos_g_t, neg_g_full[neg_idx]], dim=0)
            epoch_x = torch.cat([pos_x_t, neg_x_full[neg_idx]], dim=0)
            epoch_y = torch.cat(
                [pos_y_t, torch.full((len(neg_idx),), junk_index, dtype=torch.int64)], dim=0
            )
        else:
            epoch_g, epoch_x, epoch_y = pos_g_t, pos_x_t, pos_y_t

        order = np.random.RandomState(seed * 7_919 + epoch).permutation(epoch_g.shape[0])
        total_loss = 0.0
        n_batches = 0
        for start in range(0, len(order), batch_size):
            idx = order[start : start + batch_size]
            gb = epoch_g[idx].to(dev)
            xb = epoch_x[idx].to(dev)
            yb = epoch_y[idx].to(dev)
            opt.zero_grad()
            out = model(gb, xb)
            loss = loss_fn(out, yb)
            loss.backward()
            opt.step()
            total_loss += loss.item()
            n_batches += 1
        history.append({"epoch": epoch, "mean_loss": total_loss / max(n_batches, 1)})
        print(f"  epoch {epoch}: mean loss {history[-1]['mean_loss']:.4f}", file=sys.stderr)

    wall_s = time.time() - t0
    return model, {
        "n_pos_train": int(n_pos),
        "n_neg_pool_train": int(n_neg_pool),
        "n_neg_used_per_epoch": int(n_neg_wanted),
        "neg_ratio": neg_ratio,
        "epochs": epochs,
        "history": history,
        "wall_s": wall_s,
    }


# ---------------------------------------------------------------------
# Evaluation -- deliverable 3: per-group internal-val top-1 vs the
# prototype matcher on the *same* crops, and junk-negative rejection.
# ---------------------------------------------------------------------


@torch.no_grad()
def eval_top1(model: Net, device: str, g: np.ndarray, x: np.ndarray, y: np.ndarray, batch_size: int) -> float:
    if g.shape[0] == 0:
        return float("nan")
    model.eval()
    dev = torch.device(device)
    correct = 0
    for start in range(0, g.shape[0], batch_size):
        gb = torch.from_numpy(g[start : start + batch_size].reshape(-1, 1, GRID_SIDE, GRID_SIDE)).to(dev)
        xb = torch.from_numpy(x[start : start + batch_size]).to(dev)
        out = model(gb, xb)
        pred = out.argmax(dim=1).cpu().numpy()
        correct += int((pred == y[start : start + batch_size]).sum())
    return correct / g.shape[0]


@torch.no_grad()
def eval_junk_rejection(
    model: Net, device: str, g: np.ndarray, x: np.ndarray, junk_index: int, batch_size: int
) -> dict:
    if g.shape[0] == 0:
        return {"n": 0, "argmax_is_junk": float("nan"), "argmax_not_junk_low_conf": float("nan")}
    model.eval()
    dev = torch.device(device)
    n_argmax_junk = 0
    n_low_conf_not_junk = 0
    for start in range(0, g.shape[0], batch_size):
        gb = torch.from_numpy(g[start : start + batch_size].reshape(-1, 1, GRID_SIDE, GRID_SIDE)).to(dev)
        xb = torch.from_numpy(x[start : start + batch_size]).to(dev)
        out = model(gb, xb)  # log-probs
        probs = out.exp().cpu().numpy()
        pred = probs.argmax(axis=1)
        is_junk = pred == junk_index
        n_argmax_junk += int(is_junk.sum())
        # For rows where junk isn't argmax: is the top charset-class prob < 0.5?
        charset_probs = probs.copy()
        charset_probs[:, junk_index] = -1.0  # exclude junk from this max
        top_charset_p = charset_probs.max(axis=1)
        not_junk = ~is_junk
        n_low_conf_not_junk += int(((top_charset_p < 0.5) & not_junk).sum())
    n = g.shape[0]
    return {
        "n": n,
        "argmax_is_junk": n_argmax_junk / n,
        "argmax_not_junk_low_conf": n_low_conf_not_junk / n,
    }


# ---------------------------------------------------------------------
# spec.json + tensor export (Chunk 15 interfaces item 1)
# ---------------------------------------------------------------------


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()


def export_spec(
    model: Net,
    out_dir: Path,
    n_classes: int,
    charset_sha256: str,
    feature_extractor: int,
    training_manifest_id: str,
    seed: int,
    device: str,
    deterministic: bool,
) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    n_outputs = n_classes + 1
    junk_index = n_classes

    tensors = {
        "0.weight": model.conv1.weight.detach().cpu().numpy().astype("<f4"),
        "0.bias": model.conv1.bias.detach().cpu().numpy().astype("<f4"),
        "3.weight": model.conv2.weight.detach().cpu().numpy().astype("<f4"),
        "3.bias": model.conv2.bias.detach().cpu().numpy().astype("<f4"),
        "8.weight": model.fc1.weight.detach().cpu().numpy().astype("<f4"),
        "8.bias": model.fc1.bias.detach().cpu().numpy().astype("<f4"),
        "10.weight": model.fc2.weight.detach().cpu().numpy().astype("<f4"),
        "10.bias": model.fc2.bias.detach().cpu().numpy().astype("<f4"),
    }
    for name, arr in tensors.items():
        (out_dir / f"{name}.f32").write_bytes(arr.tobytes())

    layer_list = []
    for layer in LAYER_SPEC_TEMPLATE:
        entry = dict(layer)
        if entry["index"] == 0:
            entry["weight_shape"] = list(model.conv1.weight.shape)
            entry["bias_shape"] = list(model.conv1.bias.shape)
        elif entry["index"] == 3:
            entry["weight_shape"] = list(model.conv2.weight.shape)
            entry["bias_shape"] = list(model.conv2.bias.shape)
        elif entry["index"] == 8:
            entry["weight_shape"] = list(model.fc1.weight.shape)
            entry["bias_shape"] = list(model.fc1.bias.shape)
        elif entry["index"] == 10:
            entry["out_features"] = n_outputs
            entry["weight_shape"] = list(model.fc2.weight.shape)
            entry["bias_shape"] = list(model.fc2.bias.shape)
        layer_list.append(entry)

    lock_path = HERE / "requirements.lock"
    spec = {
        "nn_version": 1,
        "layers": layer_list,
        "n_outputs": n_outputs,
        "junk_index": junk_index,
        "charset_sha256": charset_sha256,
        "feature_extractor": feature_extractor,
        "training_manifest_id": training_manifest_id,
        "seed": seed,
        "device": device,
        "deterministic": deterministic,
        "torch_version": torch.__version__,
        "python_version": platform.python_version(),
        "platform": platform.platform(),
        "lock_file_sha256": sha256_file(lock_path),
        "tensor_index_note": (
            "Tensor file names use the full ordered layer index (0..10, "
            "including non-parametric layers); only 0, 3, 8, 10 have "
            "weight/bias files. This indexing is this trainer's own choice, "
            "not read from the c15-nn-table writer -- cross-check at merge "
            "time (see tools/nn/README.md)."
        ),
    }
    (out_dir / "spec.json").write_text(json.dumps(spec, indent=2) + "\n", encoding="utf-8")


def training_manifest_id_of(split_file: Path) -> str:
    """`spec.json`'s "training manifest id": sha256 of the committed
    cluster-disjoint split file (`bench/splits/nn15_page_split.tsv`) that
    fixes which train pages fed which fold -- the one artifact that, together
    with `model_meta.json`'s `charset_sha256`/`feature_extractor`, pins
    exactly which data and which model produced this run."""
    return sha256_file(split_file)


# ---------------------------------------------------------------------
# main
# ---------------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--data-dir", required=True, type=Path)
    ap.add_argument("--out-dir", required=True, type=Path)
    ap.add_argument(
        "--split-file",
        type=Path,
        default=HERE.parent.parent / "bench" / "splits" / "nn15_page_split.tsv",
    )
    ap.add_argument("--device", choices=["cpu", "xpu"], default="cpu")
    ap.add_argument("--deterministic", action="store_true")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--neg-ratio", type=float, default=2.0, help="guess, see ARCHITECTURE.md section 11")
    ap.add_argument("--epochs", type=int, default=12)
    ap.add_argument("--lr", type=float, default=1e-3)
    ap.add_argument("--batch-size", type=int, default=256)
    args = ap.parse_args()

    model_meta = json.loads((args.data_dir / "model_meta.json").read_text(encoding="utf-8"))
    n_classes = int(model_meta["n_classes"])
    charset_sha256 = model_meta["charset_sha256"]
    feature_extractor = int(model_meta["feature_extractor"])
    junk_index = n_classes

    if args.device == "xpu" and not (hasattr(torch, "xpu") and torch.xpu.is_available()):
        raise SystemExit("--device xpu requested but torch.xpu is not available in this venv")

    model, train_meta = train_one_run(
        args.data_dir,
        args.device,
        args.deterministic,
        args.seed,
        args.neg_ratio,
        args.epochs,
        args.lr,
        args.batch_size,
        n_classes,
    )

    bank_val = load_positive_group(args.data_dir, "bank_val")
    real_val = load_positive_group(args.data_dir, "real_val")
    nonpath_val = load_junk_group(args.data_dir, "junk_nonpath_val", junk_index)
    render_val = load_junk_group(args.data_dir, "junk_render_val", junk_index)

    net_top1_bank = eval_top1(model, args.device, bank_val.g, bank_val.x, bank_val.y, args.batch_size)
    net_top1_real = eval_top1(model, args.device, real_val.g, real_val.x, real_val.y, args.batch_size)

    matcher_top1_bank_col = load_matcher_top1(args.data_dir, "bank_val")
    matcher_top1_real_col = load_matcher_top1(args.data_dir, "real_val")
    matcher_acc_bank = (
        float((matcher_top1_bank_col == bank_val.y).mean()) if matcher_top1_bank_col.size else float("nan")
    )
    matcher_acc_real = (
        float((matcher_top1_real_col == real_val.y).mean()) if matcher_top1_real_col.size else float("nan")
    )

    junk_nonpath_report = eval_junk_rejection(
        model, args.device, nonpath_val.g, nonpath_val.x, junk_index, args.batch_size
    )
    junk_render_report = eval_junk_rejection(
        model, args.device, render_val.g, render_val.x, junk_index, args.batch_size
    )

    # Deliverable 4: per-group internal-val top-1, network vs. the prototype
    # matcher, on the same crops (`ARCHITECTURE.md` section 11, the probe's
    # groups: digits, 0/O/o, l/1/I, lower, upper, punct).
    classes = load_classes(args.data_dir)
    net_pred_real = predict_argmax(model, args.device, real_val.g, real_val.x, args.batch_size)
    net_pred_bank = predict_argmax(model, args.device, bank_val.g, bank_val.x, args.batch_size)
    per_group_real_net = per_group_accuracy(real_val.y, net_pred_real, classes)
    per_group_real_matcher = (
        per_group_accuracy(real_val.y, matcher_top1_real_col, classes) if matcher_top1_real_col.size else {}
    )
    per_group_bank_net = per_group_accuracy(bank_val.y, net_pred_bank, classes)
    per_group_bank_matcher = (
        per_group_accuracy(bank_val.y, matcher_top1_bank_col, classes) if matcher_top1_bank_col.size else {}
    )

    report = {
        "net_top1_bank_val": net_top1_bank,
        "matcher_top1_bank_val": matcher_acc_bank,
        "net_top1_real_val": net_top1_real,
        "matcher_top1_real_val": matcher_acc_real,
        "per_group_real_val": {"net": per_group_real_net, "matcher": per_group_real_matcher},
        "per_group_bank_val": {"net": per_group_bank_net, "matcher": per_group_bank_matcher},
        "junk_nonpath_val": junk_nonpath_report,
        "junk_render_val": junk_render_report,
        "train": train_meta,
    }
    print(json.dumps(report, indent=2))

    args.out_dir.mkdir(parents=True, exist_ok=True)
    (args.out_dir / "metrics.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    export_spec(
        model,
        args.out_dir,
        n_classes,
        charset_sha256,
        feature_extractor,
        training_manifest_id_of(args.split_file),
        args.seed,
        args.device,
        args.deterministic,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
