# pdfcer smoke-page misses: OCRcer 42/47 vs ocrs 47/47

MEASURED on `fixtures/synthetic/ocr/scan.pdf` page 1, pdfcer branch
`ocrcer-engine` @ 7c520945, debug build, `pdfcer ocr --ocr-engine ocrcer`.
Model `D:\Dev\OCRcer\model\out\ocrcer.ocrw`. Reproduced exactly: 49 words
recognised, `content_pct=89.4` (42/47), matching the filing. N=47 words on
one page — too small to generalise; treat every number here as a lead, not
a corpus result.

Not layout, not segmentation: `--words` shows exactly 49 word boxes with no
splits or merges (47 distinct truth tokens + 2 legit repeats, "never" and
"scanned", each appearing twice on the page = 49 instances). Every miss is a
character- or case-level substitution inside a correctly-bounded word.

## The 5 misses (150 dpi)

| truth | got | confidence | stage | evidence |
|---|---|---|---|---|
| document | documant | 0.469 | classification | mid-word e→a; fixed at 200/300 dpi — DPI-sensitive, a glyph-fidelity ceiling at 150 dpi (x-height 15px), not a persistent confusion |
| anyone | anyona | 0.635 | classification | final e→a; same DPI-sensitive pattern as document/eye |
| eye | eya | 0.331 | classification | final e→a; lowest confidence on the page; fixed at 200/300 dpi |
| cannot | Cannot | 0.689 | classification/decoder (case) | word-initial c→C; fixed at 200 dpi but a *different* c/s word (Scanned) breaks at 300 — DPI-insensitive, not a resolution problem |
| should | Should | 0.394 | classification/decoder (case) | word-initial s→S; still wrong at 200 and 300 dpi — persistent |

Extras (49 vs 47): scorer/counting artefact, not an engine fault — `never`
and `scanned` are genuinely repeated on the page and `ocr-accuracy.py`
counts distinct truth tokens, not instances.

## Two distinct failure modes, distinguished by the DPI sweep

Same page, OCRcer only, `--dump-image`/`--words`, x-height measured from the
`over`/`o-v-e-r` word box (x-height-only glyphs, no ascender/descender),
converted pt→px at each dpi (measured, derived from the reported rect, not
pixel-counted):

| dpi | x-height (px) | content_pct | new failure mode |
|---|---|---|---|
| 150 | 15.0 | 89.4% (42/47) | e→a (document, anyone, eye), c/s case (cannot, should) |
| 200 | 20.0 | 97.9% (46/47) | only `sees`→`Sees` (case) — all e/a misses resolved |
| 300 | 30.0 | 83.0% (39/47) | new garbling: `Recogmtion`, `quaüty`, `withou†`, `anä`, `praject`, plus 3 more case flips (`Scanned`×2, `Should`) |

**e/a confusion is DPI-sensitive** — resolves cleanly at 200 dpi, evidence
it is a glyph-fidelity ceiling at ~15px x-height, not a standing confusion
pair.

**c/C, s/S case confusion is DPI-insensitive** — present at 150, persists
at 200 (moves to a different word), and gets *worse* at 300 (3 flips, up
from 2). More pixels does not fix it, so it is not a rendering problem: `c`
and `s` are shapes that are geometrically near-identical between cases
except for size, and if size-normalisation in feature extraction erases
that cue, classification of case for these letters is close to unsolvable
without an extra signal (absolute scale, or a decoder-side case-consistency
vote across the word).

**300 dpi is a cliff, independently of the c/s case issue** — component-level
garbling (spurious diacritic-shaped glyphs) appears only at 300 dpi,
pointing at binarisation/antialiasing interaction with segmentation at high
pixel density, distinct from the classification-level e/a and case issues.
One page is not enough to set a DPI recommendation; this only says 150 dpi
(pdfcer's current default) is not obviously optimal for OCRcer and a
corpus-level DPI sweep is worth doing in a real tuning round.

## Levers

- **e/a at small x-height** — `ocrcer-glyphs`: check prototype-bank coverage
  and feature discriminability for `e` vs `a` at ~15px x-height; may need
  more/finer prototypes rendered at low DPI rather than only high-DPI
  sources.
- **c/C, s/S case ambiguity** — flag to `ocrcer-architect`, not assigned
  unilaterally: this looks like a feature-vector question (does the
  extractor retain any absolute-scale cue post-normalisation?) as much as a
  decoder-weight one. If the feature vector is confirmed scale-invariant
  by design, the fallback lever is `ocrcer-linguist` (a decoder-side
  case-consistency check across a word's classified letters) rather than a
  new confusion-table entry, since this is a systematic ambiguity, not a
  rare misread.
- **300 dpi garbling** — `ocrcer-runtime`: binarisation/morphological
  repair at high pixel density, per the risk-mitigation ladder.
- **Confidence check** — all 5 misses scored below the page's mean
  confidence (0.698 at 150 dpi); `eye`(0.331) and `should`(0.394) were the
  two lowest-confidence words on the page. On this one page, confidence
  correctly flagged the misses as suspect — consistent with, but not proof
  of, good calibration; the calibration curve itself needs its own
  multi-page measurement per `ARCHITECTURE.md` §4.2, not this single point.
