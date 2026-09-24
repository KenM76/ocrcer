# Chunk 7 — pdfcer binding

Built on branch `pdfcer-binding` in an isolated worktree
(`D:/Dev/ExcludedPrivate/ocrcer/wt-pdfcer`, `CARGO_TARGET_DIR` also
isolated), per the chunk's hard constraint: **nothing under `D:\Dev\pdfcer`
was edited.** Everything below is either new/changed on the OCRcer side, or
a throwaway harness outside both repos.

## What was built

1. **`Engine::recognize_bytes(width, height, pixels) -> Result<Vec<Word>, Error>`**
   in `crates/ocrcer-core/src/pipeline.rs`. A one-line forward to the
   existing `Engine::recognize(Gray)` — not a second implementation
   (CLAUDE.md rule 4) — that takes the exact `(u32, u32, &[u8])` shape
   `pdfcer`'s `OcrEngine::recognize` needs, so an embedder never has to name
   `ocrcer_core::Gray` itself. Everything downstream (segmentation, matching,
   decode, calibrated confidence) was already in place from earlier chunks;
   this chunk added no new pipeline stage.

2. **`integration/pdfcer/ocrcer_engine.rs`** — a ready-to-paste
   `impl OcrEngine for OcrcerEngine`, plus **`integration/pdfcer/README.md`**
   with the exact `Cargo.toml`/`ocr/mod.rs` lines. Not built as part of the
   OCRcer workspace (`ocrcer-core` does not and will not depend on
   `pdfcer-core` — the dependency runs the other way).

3. **A throwaway out-of-tree harness**, `D:/Dev/ExcludedPrivate/ocrcer/pdfcer-harness`
   (not committed to either repo), that path-depends on a **real**
   `pdfcer-core` checkout (`D:/Dev/pdfcer/crates/pdfcer-core`,
   `default-features = false`, read-only) and this worktree's real
   `ocrcer-core`, and includes the adapter file byte-for-byte
   (`src/engine_ocrcer.rs`, `diff`-confirmed identical to the committed
   copy). `src/lib.rs` re-exports `pdfcer_core::ocr` and
   `pdfcer_core::page_tree` under the same names the adapter's `crate::`
   paths expect, so the `impl OcrEngine for OcrcerEngine` it produces is a
   real implementation of `pdfcer_core`'s actual trait, not a local
   look-alike.

## What was proven

- `cargo test --workspace --release` (isolated `CARGO_TARGET_DIR`, `-j 2`):
  **green** — every existing suite plus the new
  `crates/ocrcer-bench/tests/pdfcer_entry_point.rs`, which builds a small
  real `.ocrw` model, renders a real page, and asserts
  `recognize_bytes(width, height, &grey)` is exactly equal
  (`assert_eq!`) to `recognize(Gray { .. })` on the same page, every word's
  confidence is in `0.0..=1.0`, and a truncated buffer is refused
  (`Err`, not misread).
- `cargo build -p ocrcer-core --target wasm32-unknown-unknown`: **green**.
- Harness: `cargo check --features ocrcer` — **green** natively and under
  `--target wasm32-unknown-unknown`, matching the exact configuration
  pdfcer's own CI gate exercises for `pdfcer-core`
  (`.github/workflows/ci.yml` line 733: `cargo check -p pdfcer-core -p
  pdfcer-render --target wasm32-unknown-unknown`, and line 91:
  `cargo test -p pdfcer-core --no-default-features`). This is the "real
  proof" the task asked for, not merely an OCRcer-side smoke test.
- `reports_confidence()` returns `true` in the adapter, and it means it:
  every `RecognizedWord.confidence` is `Some(word.confidence)` where
  `word.confidence` is the calibrated match-margin score from
  `ocrcer_core::confidence` (geometric mean over the word's characters),
  never a raw distance or a stub (CLAUDE.md rule 5). There is no code path
  in the adapter that produces `None`.

Not run in this chunk, by design: no `ocr.exe` benchmark (another agent
owns the one-heavy-run slot this cycle) and no CER/WER comparison against
`ocrs` — that comparison, and the operator go it requires, is what actually
unlocks the paste-in (`PLAN.md` §2c).

## Design decision: adapter file, not a new workspace crate

Chosen over adding `crates/ocrcer-pdfcer` to the OCRcer workspace, because:

- `pdfcer` lives in a separate repository. A path dependency from an
  OCRcer-workspace crate to `pdfcer-core` would be machine-specific and
  fragile for any other operator or CI runner.
- The dependency direction is backwards for a workspace crate: `pdfcer`
  depends on `ocrcer-core`, never the other way. A crate depending on both
  would misrepresent that.
- The real adapter necessarily uses `pdfcer-core`'s own types
  (`RecognizedWord`, `Rect`, `OcrEngine`), so it has to live inside
  `pdfcer`'s own tree, consuming `ocrcer-core` as an external dependency —
  exactly how `engine_ocrs.rs` already works. This keeps the future
  `pdfcer`-side change to one new file plus a Cargo feature block, matching
  the existing `ocrs` integration shape exactly.

## The exact pdfcer-side change (when the hand-off gate passes)

1. Copy `integration/pdfcer/ocrcer_engine.rs` to
   `crates/pdfcer-core/src/ocr/engine_ocrcer.rs`, unmodified.
2. Add `#[cfg(feature = "ocrcer")] pub mod engine_ocrcer;` to
   `crates/pdfcer-core/src/ocr/mod.rs`, next to `engine_ocrs`.
3. Add an `ocrcer` feature (`["dep:ocrcer-core"]`) and the corresponding
   optional dependency to `crates/pdfcer-core/Cargo.toml` — full block in
   `integration/pdfcer/README.md`. **Not added to `default`** — that
   promotion is a separate, deliberate decision for after the hand-off gate
   passes, not part of this paste-in.

## What pdfcer's `OcrEngine` trait cannot yet get from OCRcer

Nothing in the trait itself is unsatisfiable — both methods are fully
implementable today, as the harness proves. The only gap is upstream of the
API: OCRcer has not yet been shown to beat `ocrs` on the gated corpora
(`PLAN.md` §2c), which is a model-quality question the hand-off gate exists
to answer, not an integration one.
