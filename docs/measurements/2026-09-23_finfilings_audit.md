# finfilings corpus audit (60 pages)

Scope: `D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings/` (60 pages, `.pgm` +
`.truth.json`, `manifest.json`). Triggered by a visual defect spotted on
`filing__r000011`: two renderings of the same paragraph text, wrapped at two
different widths, drawn at the same absolute vertical position, overlapping
through the page body. Purpose of this audit: find out how common that defect
is across the corpus before treating this corpus's CER as a clean measurement
of engine accuracy, and record bold/ligature prevalence for anyone tuning the
prototype bank or decoder against it.

Per-page results are in `D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings/audit.tsv`
(outside this repo, per the corpus-privacy rule — see Method below). This
document carries only aggregate counts and generic, structural descriptions;
no corpus text is reproduced here.

## Corpus provenance (computed, not judged)

`tools/parquet_corpus.py` builds this corpus by extracting already-rendered
page images and their paired transcript text from an external parquet dataset
(`train-00000-of-00008.parquet`, HuggingFace-style). It does not render HTML or
any other markup to pixels itself — every `truth.json` in this corpus carries
`"kind": "parquet-transcript"`, confirming the extraction-only path. Two
consequences that bear on how trustworthy an automatic check can be here:

- `glyphs` is always `[]` for this corpus kind — there is no per-glyph or
  per-word bounding box ground truth. An automatic position-based overprint
  detector (e.g. "flag any region where two truth lines' boxes would occupy
  the same pixels") is not buildable from this corpus's metadata; the boxes
  it would need do not exist.
- The truth's line breaks are the *source's* wrapping, not the page's actual
  visual line wrapping. This matters below: a band-count vs. truth-line-count
  heuristic could not be trusted as a discriminator, because the two counts
  are not measuring the same wrapping in the first place.

## Method

**Classification (class, bold) — judged by eye, 100% coverage.** Every one of
the 60 pages was rendered to PNG contact sheets (15 sheets of top-900px crops,
8 sheets of full-page thumbnails) and reviewed directly, exceeding the
originally suggested "spot check a handful" bar. Several candidate automatic
pixel-only overprint detectors were tried first and rejected because none
discriminated the one known-defective page from legitimate multi-line-label
pages without false positives:

- row-wise ink-density band detection (max/mean density) — the defective page
  did not stand out from clean pages with dense multi-column tables;
- band-count vs. truth-line-count ratio — landed unremarkably mid-pack for the
  defective page, consistent with the source-wrapping caveat above;
- "tall band fraction" (bands taller than the page median) — the defective
  page scored highest, but several legitimate multi-line-label pages scored
  within the same range, so no clean threshold exists.

Given no reliable automatic signal, the full visual review became the primary
method. Ambiguous cases (short/sparse pages, bold-weight judgment calls) were
additionally cropped and zoomed 4-6x to confirm.

**Ligatures — a computed test plus a by-eye confirmation.** A page's use of
the "fi"/"fl" letter-pair is detected automatically and reproducibly: every
word (`[A-Za-z']+`) in that page's truth lines is scanned for the substring
"fi" or "fl". Separately, by-eye pixel inspection of multiple independent
instances established that this corpus has exactly two visual font families,
and that ligature rendering tracks family, not individual word choice: the
serif family (used by press-release/prospectus body text) reliably renders
true "fi"/"fl"/"ffi" ligatures; the sans-serif family (used by the NPORT-P
investment-fund grid forms and by one other structured legal document) does
not — confirmed by zooming on 5 independent word instances across both
families, in every case showing the expected ligature or the expected
separate glyphs with an intact dot on the "i". The reported ligature verdict
combines both: "yes" only when a page both contains an fi/fl-pair word (a
computed fact) and is rendered in the serif family (a by-eye fact).

## Results

| | count |
|---|---|
| clean | 59 |
| overprinted | 1 |
| other-defect | 0 |
| **total** | **60** |

Only `filing__r000011` is defective, and it is defective in the way described
above across its entire body, not only the top of the page. No other page in
this 60-page sample showed a missing image, a truth/render mismatch, or any
other structural defect — this includes a specific check of the NPORT-P
forms' small "?" tooltip icons, which are legitimate unfilled-checkbox UI
elements, not artefacts.

Bold text: 58/60 pages (97%) contain bold text (headings, bullets, or
emphasized labels); 2/60 do not.

Ligatures (fi/fl): 24/60 pages (40%) both contain an fi/fl-pair word and are
rendered in the ligature-forming serif family; the remaining 36/60 (60%) are
either the non-ligature-forming sans-serif family (the NPORT-P grid forms and
one other structured document — 35 pages) or a serif page with no fi/fl-pair
word in its text (1 page).

**Small-corpus caveat.** 60 pages is not enough to treat any of these
proportions — including the 59/60 clean rate — as a stable estimate. One
defective page out of 60 is a single occurrence; take it as "this defect
exists and is at least present," not as a rate.

## What this means for a benchmark run against this corpus

Any aggregate CER/WER computed across all 60 pages conflates one page's
render-side ground-truth corruption with genuine engine error. The fix that
requires no re-rendering: exclude `filing__r000011` from headline CER/WER
numbers (or report it separately, labelled as "known-defective ground
truth"), and note that its expected contribution is a CER spike neither
engine could avoid — a hypothesis that no text on that page is fully
recoverable through the overprinted region, since two independent letterforms
occupy the same pixels.

## Renderer finding (task 3)

The renderer responsible for the overprinting defect is **not part of this
repository**. `tools/parquet_corpus.py` only extracts pre-rendered page images
and their paired transcript text from an external parquet dataset; it
performs no HTML/CSS layout or rasterization of its own. There is therefore no
renderer code here to patch, and nothing was re-rendered as part of this
audit, per instruction.

Working hypothesis for the upstream defect's mechanism (offered for whoever
maintains the upstream dataset, not actionable in this repo): the same body
paragraph appears twice, each copy wrapped at a different column width, both
drawn at the same absolute page position. That pattern is consistent with a
responsive/media-query layout where two viewport-width variants of a flowed
text block both ended up captured into one static image, or with a duplicate
absolutely-positioned text layer sharing coordinates with the underlying flow
layout. This is a hypothesis from the visual evidence on one page, not a
confirmed root cause — nobody involved in producing this repo has access to
the upstream rendering pipeline to verify it.

## Constraints observed

No OCR engine or benchmark was run as part of this audit. `crates/`, `model/`,
and `docs/ARCHITECTURE.md` were not touched. No commit was made. This document
and `audit.tsv` contain only aggregate statistics, computed facts (fi/fl
substring detection), and generic structural descriptions of document
types/defects — no verbatim corpus text.
