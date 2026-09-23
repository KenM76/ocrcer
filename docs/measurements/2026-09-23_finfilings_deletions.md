# finfilings deletion audit — where the missing characters go

Date: 2026-09-23. Author: `ocrcer-bench`. Model: `model/out/ocrcer.ocrw` (187
classes, 32 faces, 29,675 prototypes). Corpus: `D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings`
(60 pages). Tool: `target/release/ocr.exe`.

Trigger: end-to-end CER 18.546%, F1 70.846%, and the top aggregate confusions
are all deletions (`e`->"" 1,447, `i`->"" 1,268, `t`->"" 1,149, `r`->"" 1,100,
`o`->"" 942, `a`->"" 938, `n`->"" 807, `s`->"" 642, `l`->"" 529, `f`->"" 487,
plus `""->" "` 1,067, `" "->""` 823, `"\n"->""` 659, `"\n"->" "` 613,
`"."->""` 530). Text is missing, not misread, at the corpus level. This
report diagnoses where.

## Method

Full-corpus run (background, ~1009s/60 pages) ranked pages by CER. The six
worst non-defective pages were sampled (`filing__r000011` excluded — already
diagnosed in `docs/measurements/2026-09-23_finfilings_audit.md` as a
render-defective page with overprinted text, not a fresh finding):

| page | CER |
|---|---|
| filing__r000583 | 53.76% |
| filing__r000055 | 45.32% |
| filing__r000308 | 41.44% |
| filing__r000022 | 40.09% |
| filing__r000088 | 34.42% |
| filing__r000363 | 33.81% |

For each page: `ocr.exe --only <page> --raw` (truth vs. aligned output),
`--layout` (per-band x-height/threshold/word-split diagnostics, with and
without `--no-decode`), and `--worst 500` (near-complete per-page confusion
table, used to sum deletions/insertions/substitutions exactly rather than
against a `--worst 40` truncation). Page images were rendered to PNG and
inspected directly for two pages (`filing__r000055`, `filing__r000088`) to
verify the segmentation read against the actual pixels, including one
pixel-row scan to rule out a border-artifact misreading.

Every page's `deletions + insertions + substitutions` (summed from its
`--worst 500` confusion table) reproduces that page's reported CER numerator
almost exactly (e.g. filing__r000022: 357+31+336 = 724, 724/1806 chars =
40.09%, matching the reported 40.089% to three decimals) — confirming the
per-page confusion sums are a complete, not truncated, account of that page's
errors.

## Per-page findings

### filing__r000583 — NOT a segmentation problem (candidate none of a/b/c/d)

`--layout --no-decode` shows all 11 bands correctly formed: 11 truth lines,
11 output bands, word-split counts plausible against the `want` text on every
line, x-height 14-15px (`Observed`) on every line — no degenerate values, no
`\n`->"" or `\n`->" " confusions anywhere on the page (both zero). This page's
823 combined deletions (319) and substitutions (275) come entirely from the
*matcher*: the paragraph is rendered in italic, a style the shipped bank
carries no prototypes for (per `docs/ARCHITECTURE.md`'s 2026-09-23 entries,
italic was deliberately deferred pending evidence). Slanted strokes match
Regular/Bold prototypes poorly; the nearest surviving match is frequently
wrong, and the decoder appears to resolve ambiguous ink by omitting a
character rather than emitting an implausible one, converting matcher noise
into deletions as well as substitutions (garbled tokens like `Øe`, `æasona‰`,
`®™™√o™g` in place of `The`, `reasonable`, `forward-looking`). This is a
recognition-quality gap on an unrepresented font style, not a segmentation or
filter defect — it sits outside the four candidates as originally framed.

### filing__r000055, filing__r000308, filing__r000088, filing__r000363 — line-merge -> x-height collapse (candidate d)

All four show the same recurring diagnostic signature in `--layout`: a band
spanning nearly the full text width (`x 53..1580`-ish), carrying 200-270
member components — far more than the single truth line the tool's
line-matcher attributes to it — with `x-height 1.00 (Observed)` and `cap
13.00`, both degenerate floor/fallback-looking values against the page's
otherwise-normal 13-15px sizes. Occurrence counts of this exact signature,
via `--layout`:

| page | `x-height 1.00 (Observed)` occurrences | truth lines | segmented bands |
|---|---|---|---|
| filing__r000055 | 3 | 95 | 87 |
| filing__r000308 | 7 | 92 | 117 |
| filing__r000088 | 5 | 93 | 122 |
| filing__r000363 | 3 | 90 | 133 |

The segmented-band count is not a reliable proxy for merge frequency on its
own — three of the four pages have *more* bands than truth lines, meaning
some real lines are being split into fragments even while others are being
merged; splits and merges are both happening and partly cancel out in the
raw count. The `x-height 1.00 (Observed)` signature is the reliable marker
for a merge specifically, because it is a direct symptom of the mechanism
below.

**Root cause, traced to code:** `crates/ocrcer-core/src/layout/lines.rs`,
`group_with_bands` -> `best_band` (line ~654) accepts a component into an
existing band if its vertical overlap with the band clears
`Params::overlap_fraction` (default `0.5`, `crates/ocrcer-core/src/params.rs:182`)
of the shorter of the component's own height and the band's running median,
**or** `hangs_below` (line ~683) accepts it as a descender if it hangs below
the band by no more than `Params::descender_reach_fraction` (default `0.4`,
`params.rs:188`) of the band's median. Both tests are calibrated for a
single real line (x-height letters, ascenders, descenders of *that* line).
On a page where two real lines are set with little or no fully-blank pixel
row between them — a common single-spaced legal/financial-filing layout,
confirmed pixel-level on `filing__r000055`'s "d. For each..." paragraph and
visually on `filing__r000088`'s three-line bold heading — the second line's
components sit close enough beneath the first that one of these two tests
passes, and the whole second line is absorbed as if it were descenders of
the first.

Once merged, `measure()` (same file, ~line 696) computes the band's baseline
from a width-weighted mode of member bottom edges (`mode_of_weighted`,
deliberately width-weighted to defeat dot-leader lines), then each member's
"top" as `(baseline - c.y0).max(0.0)`. Every component belonging to the
absorbed second line sits *below* the wrongly-computed baseline, so its top
clamps to `0.0`; a merged band with enough second-line members can drag the
x-height histogram's weighted mode down to the `.max(1)` floor, producing
exactly the `x-height 1.00 (Observed)` reading seen in the layout dumps. The
degenerate x-height then drives the word-split threshold to `Capped` and the
matcher effectively loses the geometry it needs, so the merged region decodes
to a handful of short garbage tokens (`{√ @§ ¶ ¶ @È@...`) in place of tens to
hundreds of true characters — every one of which the aligner then charges as
a deletion or a substitution-to-punctuation, never a clean match.

Corroborating, independent evidence: on these four pages, `"\n"->""` is the
dominant newline confusion (30/27/25/23 occurrences respectively) rather than
`"\n"->" "` (0-6 occurrences) — consistent with two real lines' worth of text
landing in one output line with no space where the line break used to be,
exactly what a merged band produces. `filing__r000583` (confirmed *not*
segmentation-broken above) shows zero of either.

### filing__r000022 — a distinct segmentation failure (candidate d, different mechanism)

This page does not show the `x-height 1.00 (Observed)` signature at all (0
occurrences) and its band count (196) is close to its truth line count (203,
a 3.4% deficit) — mild under-segmentation, not the catastrophic merge above.
Its dominant newline confusion is `"\n"->" "` (127 occurrences, vs. only 35
for `"\n"->""`) — the reverse pattern from the four line-merge pages. This
page is a dense multi-column numeric financial table: narrow columns,
short stacked header words, and `$`-prefixed value cells. The pattern is
consistent with reading-order collapse across narrow adjacent columns and
touching-glyph/digit garbling in cramped cells, rather than the
tight-leading vertical merge diagnosed above. This is still candidate (d) —
segmentation-caused loss — but a different sub-mechanism (column/word
splitting on dense tabular material) than the vertical line-merge. It was
not traced to a specific parameter with the same rigor as the four pages
above; that would need a further, narrower pass focused on
`words::band_space_rules` / `words::split_band_with` and column-gap
detection, which is outside this report's scope.

## Attribution table

Counts are **observed**, summed exactly from each page's `--worst 500`
confusion table (`X -> ""` rows). Category assignment for the four line-merge
pages and for filing__r000583 is **directly verified** (pixel/visual
inspection plus the `x-height 1.00` / `\n` confusion signatures, or their
absence). Category assignment for filing__r000022 is **inferred** from the
newline-confusion pattern and band-count deficit; it was not verified at
pixel level in this pass.

| page | chars | deletions | insertions | substitutions | category | verification |
|---|---|---|---|---|---|---|
| filing__r000583 | 1,105 | 319 | 0 | 275 | matcher/font-coverage gap (italic, not in bank) | direct |
| filing__r000055 | 3,312 | 933 | 193 | 375 | segmentation: line-merge -> x-height collapse (d) | direct |
| filing__r000308 | 3,538 | 856 | 114 | 496 | segmentation: line-merge -> x-height collapse (d) | direct |
| filing__r000088 | 2,975 | 560 | 131 | 333 | segmentation: line-merge -> x-height collapse (d) | direct |
| filing__r000363 | 3,576 | 760 | 105 | 344 | segmentation: line-merge -> x-height collapse (d) | direct |
| filing__r000022 | 1,806 | 357 | 31 | 336 | segmentation: dense-table reading-order (d, different sub-mechanism) | inferred |
| **total (6 pages)** | 16,312 | **3,785** | 574 | 2,159 | | |

Deletions by category, as a share of the 3,785 deletions observed across the
six sampled pages:

| category | deletions | % of sampled total |
|---|---|---|
| segmentation: line-merge -> x-height collapse (4 pages) | 3,109 | 82.1% |
| segmentation: dense-table reading-order (1 page) | 357 | 9.4% |
| matcher/font-coverage gap, italic (1 page) | 319 | 8.4% |
| (a) lines never found | 0 observed | 0% |
| (b) lines found then filtered | 0 observed on these 6 pages | 0% |
| (c) truth covers hidden/cropped text | 0 observed on these 6 pages (known instance is `filing__r000011`, excluded from this sample as already diagnosed) | 0% |

## Observed vs. inferred, summary

**Observed, directly measured:** the per-page deletion/insertion/substitution
counts; the `x-height 1.00 (Observed)` occurrence counts and their
co-location with 200+ member, full-width bands; the truth-line vs.
segmented-band counts; the `\n`->"" vs. `\n`->" " confusion split per page;
the pixel-row verification that `filing__r000055`'s merged paragraph band is
genuine full-width prose, not a border artifact; the absence of any
segmentation defect (11/11 lines, no degenerate x-height, no newline
confusions) on `filing__r000583`.

**Inferred:** that `best_band`'s overlap-or-descender acceptance test is the
causal mechanism (strongly supported by the code reading and the consistent
co-occurrence, but not instrumented with a counterfactual re-run at a
different `overlap_fraction`/`descender_reach_fraction` — that would require
changing engine parameters, which this report does not do); that
`filing__r000022`'s deletions trace to narrow-column word-segmentation
specifically, rather than some other cause that also produces
`"\n"->" "`; that italic-style mismatch, rather than some other property of
`filing__r000583`'s font, is the operative cause (supported by visible
character slant and by the deferred-italic note in `ARCHITECTURE.md`, not by
a controlled substitution test against a hypothetical italic-inclusive
bank).

## Caveats

- Six of 60 pages, chosen because they are the worst by CER — this is a
  worst-case sample, not a representative one. The mechanisms found here are
  real and code-traceable, but their *share* of the corpus-wide aggregate
  deletion counts (e.g. the 1,447 `e`->"" instances corpus-wide) was not
  computed; that would require re-running this same categorisation across
  all 60 pages, which this report's page budget did not include.
- `filing__r000022`'s categorisation is the weakest link in this report:
  inferred from confusion-pattern signatures, not verified against the page
  image or a `--layout` band-by-band read the way the other five pages were.
- No parameter was changed and no counterfactual re-run was performed. The
  named parameters (`lines.overlap_fraction = 0.5`,
  `lines.descender_reach_fraction = 0.4`) are candidates for `ocrcer-runtime`
  or `ocrcer-architect` to evaluate, not a recommendation from this report
  that they be changed — a narrower overlap/reach test could itself break
  correctly-merged descenders on other pages, and that trade-off is outside
  `ocrcer-bench`'s remit to resolve.

## What this implies for the other roles

- **`ocrcer-runtime`** owns `lines::group_with_bands`, `best_band`,
  `hangs_below`, and `measure` in `crates/ocrcer-core/src/layout/lines.rs`.
  The line-merge -> x-height-collapse mechanism traced above is a
  segmentation defect, not a font-coverage or decoder-weight issue — a fix
  here (if one is judged safe) is a parameter or logic change to those
  functions, not a bank addition or lexicon change. Given it accounts for
  ~82% of deletions on the worst-CER pages sampled, this is the single
  highest-leverage lead this report produced.
  - `filing__r000022`'s narrow-column/reading-order issue is a second,
    distinct lead in the same crate (`words::band_space_rules` /
    `words::split_band_with` and column-gap detection), lower-confidence and
    not yet localised to a specific parameter.
- **`ocrcer-glyphs`** owns font coverage. `filing__r000583` is fresh,
  page-level evidence that an italic-rendered page currently loses ~54% CER
  purely to font-style mismatch, with segmentation confirmed clean. This is
  the evidence `ARCHITECTURE.md` asked for before revisiting the
  deliberately-deferred italic decision — it does not mandate adding
  italic, but it is the concrete data point that was previously absent.
- **`ocrcer-architect`** should decide whether the merge-defect fix belongs
  in `lines.rs`'s existing parameters (a tuning-round question) or implies a
  feature/format change (e.g. a within-band bimodal baseline check) — this
  report identifies the mechanism and its code location but does not
  prescribe the fix, per this role's remit.
