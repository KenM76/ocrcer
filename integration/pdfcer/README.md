Ready-to-paste `pdfcer` binding for OCRcer.

**Do not paste this in yet.** Per `docs/PLAN.md` §2c, `pdfcer` is only told
to add OCRcer after a documented CER/WER comparison shows it beats `ocrs` on
the finfilings/pages-cov corpora — that comparison has not passed as of this
writing. This directory is infrastructure for that future, gated hand-off,
built now so the `pdfcer`-side change is a few lines when it happens.

Proven against a real `pdfcer-core` checkout in a throwaway out-of-tree
harness — see `docs/measurements/2026-09-24_pdfcer_binding.md` for what was
run and what it showed. Nothing here has been applied to the `pdfcer` repo.

## What to paste, when the gate passes

1. Copy `ocrcer_engine.rs` to `crates/pdfcer-core/src/ocr/engine_ocrcer.rs`,
   unmodified.

2. In `crates/pdfcer-core/src/ocr/mod.rs`, next to the existing
   `engine_ocrs` declaration:

   ```rust
   #[cfg(feature = "ocrcer")]
   pub mod engine_ocrcer;
   ```

3. In `crates/pdfcer-core/Cargo.toml`, a feature mirroring the `ocrs` block
   (do **not** add `ocrcer` to `default` — see "Why not default" below):

   ```toml
   # The OCRcer text-recognition engine — see
   # https://github.com/<org>/ocrcer, `crates/pdfcer-core/src/ocr/engine_ocrcer.rs`.
   #
   # WHAT IS AND IS NOT GATED, mirroring the `ocrs` feature above: NOT gated
   # is everything in `ocr::{RecognizedWord, OcrPage, OcrEngine,
   # words_to_page_space}` and `ocr::layer`; gated is `ocr::engine_ocrcer`
   # alone.
   #
   # LICENCE: MIT, zero dependencies outside core/alloc/std
   # (`ocrcer-core`'s own CLAUDE.md rule 3), so this feature adds no new
   # licence obligation and no new supply-chain surface — no C toolchain, no
   # prebuilt binary, nothing for `cargo-about` to miss.
   #
   # NO NETWORK, structurally: `ocrcer_core::pipeline::Engine::from_bytes`
   # takes bytes the caller already has; the crate contains no I/O of its
   # own. Model location is resolved by `ocr::models::resolve_model_dir`,
   # exactly as it is for `ocrs`.
   ocrcer = ["dep:ocrcer-core"]
   ```

   And in `[dependencies]`:

   ```toml
   ocrcer-core = { version = "<pin the released version>", optional = true }
   ```

## Why not default

The strippable-capability convention in this file's header (rule 1: default
ON) is written for a capability already known to be at least as good as what
it might replace. OCRcer is not that yet — the hand-off gate in OCRcer's
`PLAN.md` §2c is specifically the thing that turns "an available engine"
into "the trusted default", and that decision belongs to a fresh operator
go, not to this paste-in. Ship it opt-in first; promote it (and decide
`ocrs`'s own fate — replaced, kept as a fallback, or both available) as a
separate, deliberate change once the comparison is in hand.

## What the adapter needs from `ocrcer-core`, already present

- `ocrcer_core::pipeline::Engine::from_bytes(&[u8]) -> Result<Engine, Error>`
- `Engine::recognize_bytes(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<Word>, Error>`
  — validates `pixels.len() == width as usize * height as usize` itself.
- `Word { text: String, rect: Rect, confidence: f32, .. }`, `Rect { x, y, width, height }` (`u32`, pixel space, y-down).
- `ocrcer_core::Error: std::error::Error + Send + Sync + 'static` — satisfies
  `OcrEngine::Error`'s bound directly; the adapter does not wrap it.

## What OCRcer cannot yet satisfy

Nothing in `pdfcer`'s `OcrEngine` trait itself is unsatisfiable — the trait
is two methods and both are fully implementable today, as the adapter shows.
The gap is entirely upstream of the trait: OCRcer has not yet been shown to
beat `ocrs` on the gated corpora (`PLAN.md` §2c), which is a model-quality
question, not an API one. See
`docs/measurements/2026-09-24_pdfcer_binding.md`.
