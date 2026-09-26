# Chunk 15 trainer

`docs/ARCHITECTURE.md` section 11: "Chunk 15 contract amended... junk
output", "Chunk 15's trainer may be Python", "Chunk 15 interfaces". This
directory is not part of the Rust workspace and never ships (same status as
`tools/fit12b`).

## Reproduce

1. **Data** (Rust, `ocrcer-bench`; never committed, never reads
   `finfilings`/`finfilings-val`/`pages-cov`/fixtures):
   ```
   cargo run -p ocrcer-bench --bin nn15_dump -- list-stems \
       <pages-train-dir> 4 > <scratch>/nn15_stems.tsv
   python tools/nn/cluster_split.py <scratch>/nn15_stems.tsv \
       <pages-train-dir> bench/splits/nn15_page_split.tsv
   cargo run -p ocrcer-bench --bin nn15_dump -- dump \
       <model.ocrw> <pages-train-dir> <dump-out-dir> \
       bench/splits/nn15_page_split.tsv 4
   ```
   `bench/splits/nn15_page_split.tsv` is committed -- the fixed,
   cluster-disjoint internal train/val split of the sampled `finfilings-train`
   pages (`#stem\tfold\tcluster_id`, fold `B` = internal validation).
   `<dump-out-dir>` is not (`D:/Dev/ExcludedPrivate/ocrcer/nn15/` in this
   session).

2. **Train**:
   ```
   python tools/nn/train.py --data-dir <dump-out-dir> --out-dir <nn-out-dir> \
       --device xpu --neg-ratio <R> --epochs 12          # exploration
   python tools/nn/train.py --data-dir <dump-out-dir> --out-dir <nn-out-dir> \
       --device cpu --deterministic --neg-ratio <chosen> --epochs 12   # shipped
   ```
   `<nn-out-dir>` holds `spec.json`, `<layer_index>.<weight|bias>.f32`
   (little-endian, conv `[out][in][3][3]`, dense `[out][in]` -- PyTorch's
   native `Conv2d`/`Linear` weight layout, so `train.py` writes tensors
   straight out, no transposition), and `metrics.json` (deliverable 3's
   numbers, machine-readable). Not committed.

## Environment

One venv covers both modes: `D:/Dev/ExcludedPrivate/ocrcer/nnprobe/venv`
(pre-existing, reused from the chunk 15 probe), Python 3.11.9, `torch
2.14.0+xpu`. `tools/nn/requirements.lock` is the exact `pip freeze`.
**No separate CPU-only venv was created.** The contract's "a separate
CPU-pinned venv must be made for the deterministic mode if needed" is
conditional; the `+xpu` wheel already ships a full CPU backend (`--device cpu`
never touches the XPU device), and the reproducibility claim is scoped to
"same bytes on same platform and pinned versions" -- pinning the same venv
for both modes satisfies that without a second multi-GB install. Trade-off,
not a gap: a genuine CPU-only wheel might differ at the bit level from the
`+xpu` wheel's CPU codepath; this was not required and was not checked.

## Two things this trainer cannot verify alone

- **`charset_sha256` canonicalisation.** `model_meta.json`'s `charset_sha256`
  (and `spec.json`'s copy of it) is computed by
  `crates/ocrcer-bench/src/bin/nn15_dump.rs`'s `charset_sha256_of`: SHA-256
  over `[{"index":I,"cp":C}, ...]` compact JSON, ascending by index, built
  from the loaded model's own parsed `classes`. This is this dumper's own
  choice of canonical form, not read from the `c15-nn-table` writer (a
  parallel, unmerged branch this work could not see). **Cross-check the two
  hashes at merge time** -- if the `nn` table writer hashes a different
  serialisation of `meta.charset`, the writer's refusal check ("writer refuses
  a `charset_sha256`... that does not match") will reject every model this
  trainer produces until one side adopts the other's convention.
- **Tensor file indexing.** `spec.json`'s layer list numbers all 11 layers
  (0..10, including `relu`/`maxpool2`/`flatten`/`concat_features`); only
  layers 0, 3, 8, 10 (the two convs and two denses) have
  `<layer_index>.<weight|bias>.f32` files. Also this trainer's own choice --
  cross-check against what the `c15-nn-table` writer and `c15-forward`'s
  parity fixture expect to find on disk.

## Negative-to-positive ratio

`--neg-ratio` is a **guess** (`ARCHITECTURE.md` section 11: "Negative-to-
positive ratio is a guess, recorded in `meta`, tuned on val"). Per-epoch
negatives are sampled (without replacement if the pool is large enough,
with replacement otherwise) from the pooled junk sources
(`junk_nonpath_train` + `junk_render_train`), reseeded each epoch as
`seed * 1_000_003 + epoch` -- deterministic, no wall-clock dependence.
See `docs/measurements/2026-09-25_c15_trainer.md` for the values explored
and the one chosen for the shipped run.
