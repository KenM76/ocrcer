# OCRcer — project rules

An MIT-licensed OCR engine, built from scratch, intended to replace `ocrs` in
`pdfcer` (`D:\Dev\pdfcer`).

**Read before doing anything here:** `docs/FEASIBILITY.md` (what is and is not
possible, and why this is being built at all), `docs/ARCHITECTURE.md` (the
engineering contract), `docs/PLAN.md` (chunk staging and budget).

---

## 1. Every number in the model can be reproduced and explained

Every number in the model file is one of three things:
- **authored** from knowledge: a lexicon entry, a bigram, a confusion rule, a
  threshold;
- **computed** by a deterministic script from rendered glyphs;
- **fitted** by a deterministic script from a licence-clean training split.

Anyone can re-run any of them and get the same bytes. Fitting was lifted by
operator decision on 2026-09-24 (`ARCHITECTURE.md` §11); the model was never
meant to exclude it on principle.

Every value carries its label (authored / measured / fitted / guess) and a
source. A fitted value names the script, the data manifest and the split. The
corollary still bites: **a plausible-looking number is not a number.** A guess
is labelled a guess and goes on the tuning list.

**Train and score never touch.** Data that scores the engine never feeds it.
Splits are fixed in a committed manifest before any fitting runs. The scoring
pages (finfilings, pages-cov, every fixture) are permanently scoring-only.

## 2. Only licence-clean inputs enter the model

Every input to the model has a known licence that permits it:
- a font face;
- a training corpus;
- a word list;
- any optional model OCRcer can call.

Anything unclear goes to the operator; it is not assumed in. Attribution
licences (CC-BY, Apache) are recorded in the model's `meta` and in `NOTICE`.
Share-alike, non-commercial and copyleft data (for example AGPL) stays out.

No font data ships in the model file, only feature vectors derived from
rendered shapes. An optional LLM (see `ARCHITECTURE.md` §11) is never bundled
into the core model or crate. It is a separate, opt-in component.

## 3. Three invariants in `ocrcer-core`, and none of them bend

`#![forbid(unsafe_code)]`. Zero dependencies outside `core`/`alloc`/`std`.
Compiles for `wasm32-unknown-unknown` without feature work.

These exist because pdfcer buys the same posture deliberately and pays decode
speed for it, and because its CI asserts the wasm target. When performance
pressure arrives, the answer is the optional `parallel` feature, then a
feature-gated SIMD path — additively. Never by relaxing one of the three.

## 4. Every pipeline stage is written exactly once

All Rust, one workspace: `ocrcer-core` ships, `ocrcer-build` and `ocrcer-bench`
do not. A stage never exists twice, in any two languages or any two crates.

This is a correctness rule, not a style preference. The feature extractor runs
when the prototype bank is built and again when a glyph is recognised. Two
implementations of it would have to agree exactly, forever, and nothing would
report the day they stopped — every prototype in the bank would be measured
with a different ruler than the runtime uses, and the only symptom would be
accuracy quietly collapsing. `ocrcer-build` calls `ocrcer-core`'s extractor. It
does not have its own.

Regressions are caught by golden fixtures with known ground truth, asserted at
every stage boundary, per `ARCHITECTURE.md` section 8.2. **Blessing a fixture
is a deliberate, reviewed act.** A test that rewrites its own expectation
catches nothing, and a fixture regenerated rather than understood turns the
suite into a record of a bug.

## 5. Confidence must mean something

`reports_confidence()` returning true is a promise. The score is the match
margin — how much better the winning class was than its nearest rival of a
different class — pushed through the calibration curve in the parameter block,
then aggregated as a geometric mean over characters and words.

Two characters that match equally well must report low confidence even when
both matched well in absolute terms. That ambiguity is the thing a reviewer
needs told, and it is precisely what a raw distance would hide. An uncalibrated
score presented as a confidence is worse than reporting none, which is the
situation `ocrs` is at least honest about.

## 6. The lexicon is a bonus, never a constraint

A word that is not in the lexicon loses the bonus. It is never penalised, never
rewritten, and never overridden. Inside identifier-shaped context the lexicon
term is suppressed outright.

An engine that turns `M8x1.25` into a dictionary word is worse than one that
misreads it, because the error is invisible and arrives with high confidence.
In this domain that is the expensive failure, and chunk 8 carries a test that
fails loudly when it happens.

## 7. The domain stays narrow

Printed documents and CAD drawing text. Not handwriting, not scene text, not
non-Latin scripts in v1. Losing to `ocrs` on photographs of shop signs is an
accepted outcome, not a bug to fix.

## 8. Report what was measured

A projection is labelled a projection, a reading is labelled a reading, and an
inferred constraint is never filed as a fact about the environment. Benchmark
losses are reported as prominently as wins. If a gate did not pass, the chunk
is not done.

---

## Agent roster

Dispatch these rather than doing their work inline.

| Agent | Owns |
|---|---|
| `ocrcer-architect` | Architecture, charset, feature definition, fixture adjudication, scope control |
| `ocrcer-glyphs` | Font inventory and licensing, glyph rendering, the feature extractor, the prototype bank |
| `ocrcer-linguist` | Lexicon, character bigrams, confusion table, decoder parameters |
| `ocrcer-exporter` | The `.ocrw` container, quantisation, round-trip and load-time validation |
| `ocrcer-runtime` | The Rust workspace, segmentation, matching, decoding, pdfcer binding |
| `ocrcer-bench` | Evaluation corpora, CER/WER, head-to-head vs `ocrs`, calibration |
| `ocrcer-librarian` | Roadmap, session log, decision log, budget actuals, RAG escalation |

## Budget discipline

Chunks are capped at roughly 1.5 M billable tokens, about 10% of a weekly
budget on the $200/month plan. The lever that makes this work: **Opus decides
and reviews, Sonnet subagents write.** The Sonnet pool is roughly ten times the
Opus pool, so a chunk that does its bulk implementation in the main Opus
session costs about four times as much of the scarce resource for the same
output.

The budget model in `PLAN.md` section 1 is a planning model, not a measurement.
Chunk 1 calibrates it against `/usage`, and `ocrcer-librarian` rescales the
estimates when reality diverges.
