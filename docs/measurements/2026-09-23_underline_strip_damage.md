# Underline-strip damage diagnosis: what `strip_underlines` actually erases

Follow-up to `docs/ARCHITECTURE.md` section 11's last entry ("Underline strip
fails the real-filings gate; it stays in the code, shipped off") and
`docs/measurements/2026-09-23_underline_strip.txt`. Diagnosis only — nothing
here is shipped, `lines.underline_strip` stays `0`.

Pages: `filing__r000033` (primary), `filing__r000055` (secondary, time
allowed). `D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings`. Model:
`model/out/ocrcer.ocrw`. Binary built with `CARGO_TARGET_DIR=target/runtime-diag`
(`target/release` untouched); `cargo build --release -p ocrcer-bench --bin ocr`.
All readings via `--only <page>` on the single page named — no full-corpus run.

Every claim below is tagged **Observed** (read directly off instrumentation,
the dumped mask, or a cropped image) or **Inferred** (reasoned from the
Observed evidence, not independently measured).

## 1. Instrumentation (temporary, reverted)

`crates/ocrcer-core/src/layout/lines.rs` was temporarily patched:

- `strip_underlines`: gated behind `OCRCER_UNDERLINE_TRACE=1`, printed the
  page's `h` (median glyphish-component height) and `L` (`rule_run_heights *
  h`, the run floor), then for every component whose width reached `L`
  ("CANDIDATE") its bbox/width/height/area, and after the pass the 8 largest
  post-strip components by height ("POST-STRIP-LARGEST").
- `strip_component_runs`: same gate, printed every erased run's row, x-range,
  length, and length as a fraction of its component's own width, plus a
  per-component erased-run count.
- A `diag::dump_mask` helper, gated behind `OCRCER_UNDERLINE_DUMP=<dir>`,
  wrote the binarized page mask to PGM immediately before and immediately
  after the strip pass (`mask_before.pgm`, `mask_after.pgm`), ink as black.

All of this has been reverted; `git diff` (or a byte comparison against the
pre-patch file) shows no change. This document is the only remaining record
of what it printed. No fixture, test, or default was touched.

## 2. Readings, before/after (single page, `--only`, `--line-pages`)

| page | h | L | control CER | control line-matched CER | toggled CER | toggled line-matched CER |
|---|---|---|---|---|---|---|
| `filing__r000033` | 11.000 | 59.630 | 26.849% | 32.266% | 68.017% | 66.368% |
| `filing__r000055` | 13.000 | 70.472 | 32.488% | 35.598% | 55.193% | 59.541% |

Both `h` and `L` are **Observed**, printed by the trace at the top of the
`strip_underlines` call for that page. Page size for both is 1653×2339
(**Observed**, from the trace's page-dims line, matching the source `.pgm`
header). Both pages' CER figures are **Observed**, read from `ocr.exe`
stdout for the exact `--only` run named.

## 3. `filing__r000033` — a merged financial table around a boxed column

**Observed.** With the strip on, exactly 3 components on this page reach the
candidate gate (width ≥ L = 59.63px): two large ones (`label=149`, bbox
(677,296)-(1428,996), 751×700, area 32044; `label=1166`, bbox
(677,1088)-(1428,1428), 751×340, area 13308) and one small one (`label=1124`,
79×19, area 575, 0 runs erased — its width barely clears L but it never
contains a run that long).

**Observed**, from a crop of the original page at each of the two large
components' bboxes (`label149_orig.png`, `label1166_orig.png`): each
component is a quarterly financial table, several rows of dollar figures
under four "Quarter 2024" columns plus a fifth "Year Ended" column, and the
fifth column has a **drawn box outline** around it (all four sides ruled).
`label=149` covers the table's top data block, `label=1166` the next block
down; they are two separate components because a blank row splits them, not
because they are different tables. This is real tabular content, not a
signature, logo, or scan artefact.

**Observed**, `label=149`'s 37 erased runs, grouped into row-bands by shared
x-range and read against the crop:

- 13 bands of 2 consecutive rows, `x0=677 x1≈1422/1423` (span 745/745 =
  **99.2–100.0%** of the component's own width), each pair straight (x0/x1
  identical or off by 1px between its two rows). These land exactly on the
  table's ruled horizontal lines: the underline under each "2024" header row,
  the rule under the first `$150 $159 $157 $155 $621` subtotal row, etc.
  Several pairs of these bands sit 1 row apart (e.g. rows 440-441 then
  443-444) — a **double rule**, the accounting convention for a subtotal.
- 2 bands of 3-4 consecutive rows at `x0=1226 x1=1422` (span 196px = exactly
  **26.1%** of the component's width, but **100%** of the boxed column's own
  width), at the very top (y=296-298) and very bottom (y=992-995) of the
  component. These are the box's own top and bottom edges — visually
  confirmed in `label149_before.png` vs `label149_after.png`: after the
  strip, every horizontal rule in the crop is gone including the box's top
  and bottom, while the box's two vertical sides are still present (now
  unconnected top and bottom — an open bracket, not a closed box).

All of the above are **true rule hits**: straight, multi-row, and either
near the full width of the component or (for the box's own edges) exactly
the full width of the shape they belong to, repeated identically at multiple
locations.

**Observed**, `label=1166`'s damaging exception, at y=1400-1405 (crop
`strip1403_before.png` / `strip1403_after.png`, region under the
"$(75) $(84) $(86) $(83) $(328)" total row):

- y=1400-1401: one band, `x0=491→677 x1=917/916→1422/1423`, span 745-746px
  (99.2-99.3%) — a genuine rule (top line of a double-underline).
- **y=1403: five separate runs on the same row** — (677,762,85px,11.3%),
  (823,900,77px,10.3%), (966,1038,72px,9.6%), (1099,1175,76px,10.1%),
  (1242,1358,116px,15.4%) — none reaching even a third of the component's
  width, each sitting directly under one of the five dollar figures in the
  crop. This is **not** a rule: it is five disjoint, short spans, each under
  a different number, at widths and positions that do not repeat at any
  other row on the page.
- y=1404: one band, span 745px (99.2%) — the second line of the same
  double-underline.
- y=1405: two runs, (677,1216,539px,71.8%) and (1226,1408,182px,24.2%), with
  a 10px gap between them at x≈1216-1226 (the box's left vertical edge) —
  plausibly the same double-underline's antialiasing tail, split by the box
  edge rather than a genuine second rule; more ambiguous than y=1403 but
  still not identical in x-range to y=1400/1401/1404's band.

**Observed**, `label149_after.png` / `label1166_after.png` vs
`label149_before.png` / `label1166_before.png`, and the tight crop at
y=1385-1430 (`strip1403_after.png`): after the strip, the `$(83)` and
`$(328)` figures each lost the bottom of their closing parenthesis, leaving
a bare vertical stroke (`|`) hanging below the numeral where the erased
y=1403 run passed through it. This is the one place on this page where the
strip demonstrably removed pixels that were part of a real character, not a
rule — and it happened at a row squeezed between two real rule rows, where
touching numeral ink and rule antialiasing are hard to tell apart by row
alone.

**Observed**, the post-strip relabelling (`POST-STRIP-LARGEST`, largest 8
components by height after the strip and re-label): the 8 largest surviving
components on the page are all leftover fragments of the box's vertical
sides, e.g. `label=1321` bbox (1409,1138)-(1427,1376), 18×238 (height =
21.6× the page's own h=11), `label=1320` 10×238, four more pairs at
115-139px tall (10.5-12.6× h). None of these existed as separate components
before the strip — they are what remains of the box's left/right edges once
the top and bottom that used to close them into a loop are gone.

**Inferred.** These orphaned tall/narrow fragments (up to 21.6× the page's
median glyph height, nothing like a glyph in aspect) did not exist as inputs
to line-grouping before the strip. `is_glyphish`'s tall-component gate
(`furniture_fraction`) does not exclude them here (238 < 0.2 × 2339 =
467.8), so they are still handed to `lines::group_with_bands` as ordinary
candidate members. What downstream effect they have on banding was not
traced in this pass (that would need a `group_with_bands`-level trace, not
attempted here); flagged as the most likely secondary damage channel, since
the two components stripped here account for only two locations of directly
observed character-ink loss (the `$(83)`/`$(328)` parentheses) — not enough
by itself to explain a page CER move from 26.8% to 68.0%.

## 4. `filing__r000055` — repeated bordered value boxes down the whole page

**Observed.** 19 components reach the candidate gate, all with nearly
identical shape: bbox width 426px, height 51-52px, at the same x-range
(491-917) but spread down the entire page (y = 27, 101, 190, 263, 463, 537,
648, 722, 833, 906, 1295, 1406, 1517, 1651, 1762, 1873, 2006, 2117, 2229).
Fill is sparse — area 995-1420 out of a 426×51 = 21726px box (4.6-6.5%).

**Observed**, crop `r55_top_orig.png` (covers the first two): each is a
bordered numeric input field, rendered like a form field — a rectangle
outline with the value `0.00000000` printed near its top-left, and a short
text label ("–", "ed", etc.) just to the left, outside the box. This is a
financial-filing form/XBRL-style rendering, not a signature, logo, or table
border.

**Observed**, each component's erased runs (identical pattern across all
19): 2-3 consecutive rows at the very top of the bbox (`y = y0..y0+2`),
`x0=491`, `x1=916/917`, span 425-426px (**99.8-100.0%** of the component's
own width) — the box's own top edge, straight, and repeated identically 19
times down the page. No run is logged anywhere else in any of the 19
components — the box's left, right, and bottom edges never register a
qualifying run in this component.

**Observed**, crop `r55_top_after.png` vs `r55_top_orig.png`: after the
strip, the box's top edge is gone; its left, right, and bottom edges and the
`0.00000000` text are untouched. In place of the removed top edge, a short
vertical tick (`|`) is left standing above the text — the same
orphaned-fragment pattern as `filing__r000033`'s box, confirmed by
`POST-STRIP-LARGEST`: the 8 largest post-strip components are all 3px wide ×
49px tall (3.8× h=13) slivers at `x=491-494`, one per box, i.e. what remains
of each box's own left edge once its top is gone.

**Observed.** Unlike `filing__r000033`, no character ink loss was found in
this crop — the erased row is a clean, straight, full-width rule sitting
above where the text starts, with no numeral or punctuation touching it.

**Inferred.** The dominant damage mechanism on this page looks different
from `filing__r000033`'s: not direct character erasure (none observed here)
but scale — 19 separate true-rule hits, each correctly identified by shape,
each nonetheless converting a closed box into an open bracket and leaving a
new tall/narrow orphaned fragment (3.8× h) behind, repeated at 19 locations
spread across the entire page rather than confined to one table. This
matches the escalation note's description of damage "spread across ordinary
text" rather than concentrated on one dense region, and is consistent with
`filing__r000055` landing among the four worst pages in the full-corpus run
even though (on this page, at least) the individual erasures are not
themselves miscutting real glyphs.

## 5. True rule vs. false hit: the distinguishing features, with numbers

| feature | true rule (both pages) | false hit (`filing__r000033` y=1403/1405) |
|---|---|---|
| runs per row | exactly 1 contiguous run | **5 separate runs on one row** (y=1403), or 2 with an internal gap (y=1405) |
| fraction of width per run | 99.2-100% of the component's own width (full-table rules); or, for a sub-shape (the box edges), 26.1% of the parent component but **100% of that sub-shape's own width, repeated identically at ≥2 other rows/edges** | 9.6-15.4% of the component's width per run (y=1403); no matching width recurs elsewhere on the page |
| row-to-row straightness | `x0`/`x1` identical (±1px) across every row of a ≥2-row band | y=1403's x-range matches **none** of y=1400, 1401, or 1404; y=1405's two segments don't match 1404's single span either |
| band thickness | 2-4 consecutive rows (0.18-0.36× h) | 1 row only, isolated between two genuine rule rows |
| ink directly adjacent | none — a rule's own rows carry no other component structure above/below at the run's x-range | **Observed**: the parenthesis strokes of `$(83)` and `$(328)` sit directly at this row's x-position in the original crop, and are visibly severed in the after-crop |
| repetition | occurs 2-19 times per page with identical width/x-range (double rules, box edges, or repeated form boxes) | occurs once, nowhere else on the page |

Thickness relative to `h` does **not** separate the groups cleanly by
itself — both true rule bands (2-4 rows, 0.15-0.36×h across the two pages)
and the one observed false hit (1 row, ~0.09×h) are thin. The reliable,
purely-shape-based signals in this evidence are **band straightness across
≥2 rows** and **single-run-per-row**, not thickness or width fraction alone.

## 6. A narrowed rule (shape only) — proposal, not shipped

Group each candidate component's per-row runs into **bands**: consecutive
rows whose run's `x0`/`x1` agree within a small tolerance (≈2px, for
antialiasing). Erase a band only if:

1. it spans **≥2 consecutive rows** (rejects the isolated y=1403/1405-style
   single-row hits outright — no true rule observed on either page was ever
   1 row alone); and
2. every row in the band has **exactly one** qualifying run, not several
   (rejects the 5-segment row directly, by construction, without needing a
   width threshold at all).

This would leave `filing__r000033`'s and `filing__r000055`'s genuine rules
and box edges intact (all Observed as ≥2-row, single-run-per-row bands) and
would not have erased the `$(83)`/`$(328)`-damaging row. It is a tightening
of *which rows* get erased, not a new gate on top of the existing one.

**What this does not fix, and is flagged rather than proposed here:** even
with this tightening, a genuinely straight rule that is only one edge of a
closed shape (a box's top, or one side of a double-rule) will still be
erased on its own, and the shape it was part of will still be left open —
`filing__r000033`'s box sides and `filing__r000055`'s 19 boxes would still
end up as orphaned tall/narrow fragments (§3, §4's `POST-STRIP-LARGEST`
evidence) feeding into line-grouping as new, glyph-unlike components that
did not exist before the strip. Whether that by itself is enough to explain
the remaining CER gap was not traced past component labelling in this pass
(no `group_with_bands`-level instrumentation was added) — that is a
separate, Inferred-not-Observed open question for whoever picks this back
up, and probably needs its own trace before a shape-only row-selection fix
is trusted to clear the finfilings gate alone.

## 7. Build and test state

`cargo test --workspace` — all green (158 unit tests in `ocrcer-core`, all
other crates' suites unaffected) after the instrumentation revert. The
revert was checked by re-reading `strip_underlines` and `strip_component_runs`
against the pre-patch source; both are byte-for-byte what they were before
this diagnosis. `target/runtime-diag/` holds only the diagnostic build of
`ocr.exe`; `target/release/` was not rebuilt or touched by this pass.
