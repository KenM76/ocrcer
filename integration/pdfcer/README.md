OCRcer's `pdfcer` binding. `ocrcer_engine.rs` here is the source of truth for
pdfcer's `crates/pdfcer-core/src/ocr/engine_ocrcer.rs`.

## How pdfcer consumes it (pdfcer Pass 327.0 + 327.1, on pdfcer `main`)

- pdfcer's `tools/sync-ocrcer.py` copies OCRcer's **committed HEAD** into
  pdfcer's tree: `crates/ocrcer-core` goes to `vendor/ocrcer-core`, and this
  adapter goes to `engine_ocrcer.rs`. Operator rule (pdfcer decision 160):
  always use the newest local OCRcer, because GitHub may lag.
- pdfcer's `tools/check-ocrcer-vendored.py` fails when its copy differs from
  OCRcer HEAD. So any OCRcer commit that touches `ocrcer-core` code or this
  adapter needs a re-sync on pdfcer's side.
- Feature `ocrcer` is on by default in pdfcer, and `ocrs` stays the default
  engine. CLI: `pdfcer ocr --ocr-engine ocrs|ocrcer [--model-dir DIR]`.
- The model `ocrcer.ocrw` is read from `models/ocrcer/` beside the exe, or
  from `--model-dir`. It is neither shipped nor downloaded.

Rules for OCRcer's side:
- Change the adapter **here**, never in pdfcer's copy.
- OCRcer master must be releasable at every commit, because pdfcer can
  sync it at any time. Keep ungated core work on a branch.
- A new adapter here, such as the LLM rescoring add-on from chunk 16b, is
  the signal that unblocks pdfcer's matching backlog item (Pass 327.2).

## What the adapter uses from `ocrcer-core`

- `ocrcer_core::pipeline::Engine::from_bytes(&[u8]) -> Result<Engine, Error>`
- `Engine::recognize_bytes(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<Word>, Error>`.
  It validates `pixels.len() == width as usize * height as usize` itself.
- `Word { text: String, rect: Rect, confidence: f32, .. }` and
  `Rect { x, y, width, height }`: `u32` pixel space, y-down.
- `ocrcer_core::Error: std::error::Error + Send + Sync + 'static`, which
  satisfies `OcrEngine::Error`'s bound directly.
- `MODEL_DIR` = `"ocrcer"` and `MODEL_FILE` = `"ocrcer.ocrw"`, mirroring
  `engine_ocrs`.

Changing any of these signatures breaks pdfcer on its next sync. Treat them
as a public contract.

## Default engine

Whether OCRcer replaces `ocrs` as the default is decided by the head-to-head
comparison, not by the integration (`ARCHITECTURE.md` §11, 2026-09-24).
Evidence so far: `docs/measurements/2026-09-24_pdfcer_binding.md` and
`2026-09-24_pdfcer_smoke_misses.md`.
