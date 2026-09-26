#!/usr/bin/env python3
"""Chunk 15 parity fixture generator (`docs/ARCHITECTURE.md` section 11,
"Chunk 15 interfaces" item 4). Not part of the Rust workspace, never ships --
same status as `tools/nn/train.py`.

Loads the network's DEQUANTISED tensors (`ocrcer-build write --nn-dequant-out
<dir>`, not the pristine trainer output -- a parity check has to compare the
network the runtime will actually run, post-quantisation) plus a fixed,
deterministic selection of 256 crops from `bank_train` (the nn15b dump's
synthetic, font-rendered positive group; `real_train` is left out of this
fixture on purpose -- see `docs/measurements/2026-09-26_c15_integrate.md` --
because it is derived from `finfilings-train` page content and this script's
output is committed to git). Runs the same architecture `tools/nn/train.py`
trains (`Net`, see its module doc) forward over those 256 crops and writes
the reference log-probs next to the inputs, so a Rust test can assert the
core forward pass agrees.

Usage:
    python tools/nn/parity.py \\
        --spec <nn-out-dir>/spec.json \\
        --dequant-dir <dequant-out-dir> \\
        --dump-dir <nn15-dump-dir> \\
        --out-dir <fixture-out-dir> \\
        --count 256
"""
from __future__ import annotations

import argparse
import json
import struct
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

FEATURE_DIMS = 107
GRID_SIDE = 32


def read_f32(path: Path, cols: int) -> np.ndarray:
    a = np.fromfile(path, dtype="<f4")
    return a.reshape(-1, cols)


def read_u16(path: Path) -> np.ndarray:
    return np.fromfile(path, dtype="<u2")


def write_f32(path: Path, a: np.ndarray) -> None:
    a.astype("<f4").tofile(path)


class Net(nn.Module):
    """Identical to `tools/nn/train.py`'s `Net` -- kept as a second literal
    copy rather than an import, because a parity check that shared the
    trained model's own forward-pass code with the thing that produced the
    weights would not be testing anything: the whole point is an independent
    PyTorch evaluation of the shipped tensors."""

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


def load_dequantised_tensor(dequant_dir: Path, layer_index: int, kind: str) -> torch.Tensor:
    path = dequant_dir / f"{layer_index}.{kind}.f32"
    return torch.from_numpy(np.fromfile(path, dtype="<f4").astype(np.float32))


def build_net(spec: dict, dequant_dir: Path) -> Net:
    net = Net(spec["n_outputs"])
    layers = {l["index"]: l for l in spec["layers"]}

    def weight_shape(idx: int) -> list[int]:
        return layers[idx]["weight_shape"]

    w0 = load_dequantised_tensor(dequant_dir, 0, "weight").reshape(weight_shape(0))
    b0 = load_dequantised_tensor(dequant_dir, 0, "bias")
    w3 = load_dequantised_tensor(dequant_dir, 3, "weight").reshape(weight_shape(3))
    b3 = load_dequantised_tensor(dequant_dir, 3, "bias")
    w8 = load_dequantised_tensor(dequant_dir, 8, "weight").reshape(weight_shape(8))
    b8 = load_dequantised_tensor(dequant_dir, 8, "bias")
    w10 = load_dequantised_tensor(dequant_dir, 10, "weight").reshape(weight_shape(10))
    b10 = load_dequantised_tensor(dequant_dir, 10, "bias")

    with torch.no_grad():
        net.conv1.weight.copy_(w0)
        net.conv1.bias.copy_(b0)
        net.conv2.weight.copy_(w3)
        net.conv2.bias.copy_(b3)
        net.fc1.weight.copy_(w8)
        net.fc1.bias.copy_(b8)
        net.fc2.weight.copy_(w10)
        net.fc2.bias.copy_(b10)
    net.eval()
    return net


def select_indices(n_available: int, count: int) -> list[int]:
    """Every k-th row, `k = n_available // count`, starting at 0 -- the
    deterministic selection `ARCHITECTURE.md` section 11 item 4 asks for."""
    k = n_available // count
    assert k >= 1, f"n_available ({n_available}) smaller than count ({count})"
    idx = [i * k for i in range(count)]
    assert idx[-1] < n_available
    return idx


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", required=True, type=Path)
    ap.add_argument("--dequant-dir", required=True, type=Path)
    ap.add_argument("--dump-dir", required=True, type=Path)
    ap.add_argument("--out-dir", required=True, type=Path)
    ap.add_argument("--count", type=int, default=256)
    args = ap.parse_args()

    spec = json.loads(args.spec.read_text(encoding="utf-8"))

    g = read_f32(args.dump_dir / "bank_train_G.f32", GRID_SIDE * GRID_SIDE)
    x = read_f32(args.dump_dir / "bank_train_X.f32", FEATURE_DIMS)
    y = read_u16(args.dump_dir / "bank_train_y.u16")
    n = g.shape[0]
    assert x.shape[0] == n and y.shape[0] == n

    idx = select_indices(n, args.count)
    k = n // args.count

    sel_g = g[idx].reshape(-1, 1, GRID_SIDE, GRID_SIDE).astype(np.float32)
    sel_x = x[idx].astype(np.float32)
    sel_y = y[idx]

    net = build_net(spec, args.dequant_dir)
    with torch.no_grad():
        log_probs = net(torch.from_numpy(sel_g), torch.from_numpy(sel_x)).numpy().astype(np.float32)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    write_f32(args.out_dir / "crops_G.f32", sel_g.reshape(args.count, GRID_SIDE * GRID_SIDE))
    write_f32(args.out_dir / "crops_X.f32", sel_x)
    np.asarray(sel_y, dtype="<u2").tofile(args.out_dir / "crops_y.u16")
    write_f32(args.out_dir / "expected_log_probs.f32", log_probs)

    manifest = {
        "source": "bank_train",
        "n_available": int(n),
        "count": int(args.count),
        "stride_k": int(k),
        "indices": [int(i) for i in idx],
        "n_outputs": int(spec["n_outputs"]),
        "junk_index": int(spec["junk_index"]),
        "charset_sha256": spec["charset_sha256"],
        "feature_extractor": int(spec["feature_extractor"]),
        "torch_version": torch.__version__,
    }
    (args.out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True), encoding="utf-8")

    print(f"wrote {args.count} crops (stride {k} of {n}) to {args.out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
