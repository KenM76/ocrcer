# OCRcer

An OCR engine for printed documents, MIT licensed in both its code and its
model.

**Status: end-to-end, and losing to the alternatives on real scans.** The
workspace builds, `ocrcer-core` compiles for wasm32, and a page goes in and
words with confidences come out: binarization, deskew, components, lines,
words, segmentation, prototype matching, a Viterbi decode over lexicon and
bigrams, and a 1.89 MB model file built by a script from authored tables.

What is not done: the pdfcer binding (chunk 7), page-level golden fixtures
(chunk 2's, and their absence is why a tuning result here is evidenced by a
corpus measurement and never by a quiet re-bless), and a reject stage — the
matcher currently has no way to decline, which is the dominant error source on
real documents.

Measured, not projected: on 29 hand-checked 300 DPI scans scored by the
third-party `scribeocr/ocr-benchmark` harness, OCRcer reads **43.40%** where
Tesseract.js reads 84.76% and Scribe.js 93.65%. `docs/ARCHITECTURE.md` section
9 says what is wrong and what the fix is. Do not use this yet.

---

## What it is for

Reading text out of printed documents — CAD exports, word-processor output,
print-to-PDF, office scans — and returning each word with its position on the
page and a confidence score. It is built to drop into
[pdfcer](https://github.com/) as the recogniser behind an invisible, selectable
PDF text layer.

## Why it exists when good OCR engines already do

Three reasons, in descending order of how much they matter.

**Licence purity, including the model.** Most permissively licensed OCR engines
ship model files trained on corpora carrying their own, more restrictive
licences. `ocrs` is Apache/MIT code with CC-BY-SA-4.0 models inherited from
HierText. Shipping those files alongside MIT code is probably fine, but
"probably" is doing real work in that sentence, and adapting them is
unambiguously not fine — an adapted model inherits share-alike. OCRcer's model
has no training corpus at all: it is a set of tables, some computed from
rendered shapes and some authored outright. There is nothing upstream to
inherit from, so the model is simply MIT and adapting it for a new domain is a
free action.

**Per-word confidence.** `ocrs` produces a character and a rectangle, with no
score anywhere. Prototype matching produces a margin — how much better the
winning character was than its nearest rival — which is both meaningful and,
for a reviewer's purposes, better-behaved than a neural posterior: two
characters that match equally well report low confidence even when both matched
well. For a consumer that must show a reviewer what was guessed and how surely,
that is the difference between a usable and an unusable engine.

**The target domain is narrow and nobody targets it.** The leading small open
engines are trained on scene text — photographs of signs and storefronts. The
documents this engine will actually see are clean, printed, axis-aligned, and
full of things a scene-text model has never encountered: dimension callouts,
diameter and tolerance symbols, title blocks, part numbers. A prototype bank
can cover exactly those faces and exactly those symbols.

That last claim is narrower than it was. Measured 2026-09-22: the drafting
faces on the build machine are all CAD-vendor proprietary and cannot enter the
bank, and the diameter sign is drawn by two of the eighteen licence-clean
faces. Where no clean face draws a glyph the fallback is authoring it, which
is what the bundled ISO 3098 technical face is for. `docs/FEASIBILITY.md`
section 5.

## What it will not do

Handwriting is not supported and is not planned. Scene text — photographs,
signage, perspective, curved baselines — is out of scope and general-purpose
engines will be better at it. Badly degraded scans are the known weak spot and
[`docs/FEASIBILITY.md`](docs/FEASIBILITY.md) section 5 says so with numbers
attached. v1 is Latin script only; the design extends to other scripts by
adding prototypes, which is a script run rather than a research project.

## Design

Segmentation-driven prototype matching with a lattice decoder — the design that
carried commercial OCR to 98–99% on clean print, chosen here because every
parameter in it can be constructed rather than fitted.

A page is binarized, deskewed, split into connected components, grouped into
lines and words, and cut into character hypotheses. Each hypothesis becomes a
107-dimensional feature vector — zone ink density, gradient orientation,
projection profiles, hole count, crossing counts, and geometry relative to the
line's own baseline and x-height. That vector is matched against a bank of
prototypes, and a Viterbi pass over the segmentation lattice picks the reading
that best balances match quality, character bigram plausibility and lexicon
support.

The model file is seven kinds of table. All of them exist, and the file
measures **1.89 MB** against the 12.24 MB `ocrs` ships — the four language
tables are 2.5% of it and the prototype bank is nearly all the rest.
(`docs/ARCHITECTURE.md` section 2 has the byte-level breakdown, with each
figure marked measured or estimated. The 2.2 MB this line used to project for
the shipped 17,610-prototype bank was 16% high.)

| Table | What it is |
|---|---|
| `charset` | 187 classes, including `Ø ⌀ ° ± × ÷ √ ≤ ≥ ≈` |
| `feature_norm` | Per-dimension normalisation constants |
| `prototypes` | 17,610 vectors — every class, in every covered face and style, at five render sizes |
| `proto_index` | The pruning index that makes matching fast |
| `lexicon` | A DAWG. A bonus to the decoder, never a constraint |
| `bigrams` | Character transition probabilities |
| `confusions` | Named confusion pairs and the tests that separate them |
| `params` | Every threshold in the pipeline, including the confidence curve |

Everything is Rust, in one workspace of three crates:

| Crate | Ships | Dependencies |
|---|---|---|
| `ocrcer-core` | yes | **none** |
| `ocrcer-build` | no | free |
| `ocrcer-bench` | no | free |

`ocrcer-core` is the engine: pure safe Rust with **no dependencies at all**
outside the standard library, forbidding `unsafe`, compiling for
`wasm32-unknown-unknown` without feature work. It parses its own documented
model format rather than depending on ONNX, safetensors or a third-party tensor
runtime. There is no tensor library, no C toolchain and no prebuilt blob
anywhere in it.

The other two never reach a user, so they may take dependencies freely — a font
rasteriser, an image decoder — without any of it touching pdfcer's dependency
tree. `ocrcer-build` links `ocrcer-core` and calls its feature extractor rather
than having one of its own, which is the point of the split: the extractor runs
both when the bank is built and when a glyph is recognised, and two
implementations of it could drift apart with no symptom except accuracy
quietly collapsing.

Full detail in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## How it gets built

The prototype bank is produced by rendering every character of the charset in
every covered font and extracting its feature vector — a deterministic script
that finishes in minutes, on one core, with the same bytes every time. The
lexicon, bigrams, confusions and thresholds are authored directly.

Because building the model is minutes rather than days, tuning is a real option
rather than an aspiration: adding a typeface or adjusting a feature is a script
run, not a campaign.

The build is staged into twelve chunks with machine-checkable exit gates, laid
out in [`docs/PLAN.md`](docs/PLAN.md) — nine for the engine itself, plus three
added later for accounting-document structure, drawing primitives and
evaluation corpora. Each stage is gated on golden fixtures —
checked-in pages whose text is known exactly, with checked-in expected output
at every stage boundary — which is why the work is verifiable long before there
is an accuracy number to look at. Blessing a fixture is a deliberate, reviewed
act, never something a test does to itself.

## Layout

```
docs/              design, plan, feasibility, decisions
crates/
  ocrcer-core/     the engine - zero dependencies, ships into pdfcer
  ocrcer-build/    renders glyphs, builds the bank, writes the model file
  ocrcer-bench/    evaluation harness, fixture blessing, head-to-head runs
model/             authored source tables: charset, lexicon, bigrams, params
fixtures/          golden pages and their expected per-stage output
```

## Licence

MIT, for the code and for the model. See [`LICENSE`](LICENSE).
