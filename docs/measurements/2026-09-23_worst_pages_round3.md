# Worst pages, round 3: r000011/r000044/r000396/r000407 named, r000022 re-measured

Diagnostic only. No param default, `params.tsv`, model, or `ARCHITECTURE.md`
entry was changed to produce these findings, and no source file was edited —
every finding below came from the unmodified `./target/release/ocr.exe`'s
existing `--layout`, `--raw`, and `--worst` flags, plus offline pixel
inspection of the corpus `.pgm` files with `PIL`/`numpy` (inspection only, no
OCR logic reimplemented). No temporary instrumentation was built and no
`CARGO_TARGET_DIR=target/runtime-diag` build was needed this round. `git
status` was clean before this document was written and is clean apart from
this file now. Every page was run individually, `--only filing__rXXXXXX`, one
process at a time.

Every claim is tagged **Observed** (read directly off `--layout`/`--raw`
output or a pixel crop) or **Inferred** (reasoned from Observed evidence plus
reading the code, not separately measured).

Context: shipped controls are `lines.baseline_split_valley_margin=0.3`,
`segment.merge_overlap_frac=0.4`, `lines.underline_strip=1`,
`segment.split_min_x_heights=1.09`. Corpus-wide finfilings end-to-end CER is
13.161, line-matched 12.290. Worst pages (from
`docs/measurements/2026-09-23_line_fusion_fix.txt`'s post-ship sweep):
r000583 (40.63, already diagnosed — serif bbox chaining), r000022 (37.10,
already diagnosed — touching ink + header underline fusion), r000011
(31.33), r000044 (30.03), r000396 (29.03), r000407 (24.46).

---

## r000011 — known-defective ground truth (overprint), unchanged

**Observed**: end-to-end CER 31.333%, line-matched CER 30.877% — a 0.456pp
gap, ruling out reading order. Top confusions are broad letter deletion
(`" "->"" 68`, `"e"->"" 52`, `"r"->"" 52`, `"n"->"" 40`), the same shape a
corrupted-pixel region produces rather than a specific mechanism's signature.

**Observed**, pixel crop (`x 40..900, y 100..220`, resized 1:1): the page
body shows two paragraphs of body text superimposed at the same absolute
position, each wrapped at a different column width — e.g. "Fourth-quarter
2024 GAAP diluted EPS of $0.74 and non-GAAP diluted EPS of $1.19" overlapping
"Fourth-quarter 2024 operating cash flow of $204 million; full-year 2024
operating cash flow...". This is visually identical to the defect named in
`docs/measurements/2026-09-23_finfilings_audit.md` ("two renderings of the
same paragraph text, wrapped at two different widths, drawn at the same
absolute vertical position") and confirms that audit's finding still holds
on the current build: this page's ground truth is corrupted upstream (a
parquet-transcript renderer defect, not part of this repository), and no
segmentation, matching, or decoding fix in this engine can recover text from
pixels where two independent letterforms occupy the same location.

**Named mechanism**: known-defective ground truth (overprinted duplicate
text at the render stage, outside this repo). Not an engine defect. No
further action available here; the existing recommendation to exclude or
separately label this page stands.

---

## r000044 — a two-column form's label+value cells stitched onto the wrong row

**Observed**: end-to-end CER 30.032%, line-matched CER **33.279%** — line-
matched is *worse* than end-to-end by 3.247pp, the reverse of the usual
direction. Top confusions: `""->"0" 108`, `"0"->"" 108` (exactly symmetric),
`"\n"->"" 28`, `"i"->"l" 22`.

**Observed**, `--layout`: this page is a repeating form —
`"Monthly net realized gain(loss) – Month 1"` (one logical label, wrapped
across two visual lines: `"Monthly net realized gain(loss) –"` then
`"Month 1"`) beside a bordered numeric box `"0.00000000"`, positioned to the
right of the label's *first* line (`band fragment 1 of 2`/`2 of 2`, baselines
65.0 and 67.0 — 2px apart) while the label's second line (`"Month 1"`, x
54..141, baseline 87.0, 20px lower) is its own unfragmented line. **Observed**
pixel crop (`x 30..650, y 50..100`) confirms this exact layout by eye: the
label wraps to two lines in the left column, the value box sits to the right,
vertically centred against the label's full two-line block but geometrically
closer to line 1's baseline than line 2's.

**Observed**, `--raw`: the decoded page text shows the value pasted onto the
*first* line of the label with a space, not after the label's second line as
truth has it — `REF` has `"...gain(loss) –\nMonth 1\n0.00000000\n..."`, `GOT`
has `"...gain(loss) – 000000000\nMonth 1\n..."` (period dropped separately).
**Observed**, counted programmatically: `0.00000000` appears 26 times in
truth, `000000000` (the value with no separating `.`) appears 25 times in
`GOT` — this happens on essentially every occurrence of the pattern on the
page, not a handful of cells.

**Inferred**, from `pipeline.rs`'s own doc comment (`Line::band`, lines
76-84): "Lines sharing a band index are fragments of one visual row that
`column_gap_heights` split apart; joining them with a space and a single
trailing newline is what a page-text renderer needs to say a band is one row
rather than a run of unrelated lines." This is by-design behaviour for a
genuine same-row two-column split, and it is *correct* here in the narrow
sense that the value box really is on the same visual band as the label's
first line — but the label's own wrap makes that band assignment the wrong
place to insert the value in *reading* order: the value belongs after the
label's full (both-line) text, not spliced into its middle.

**Named mechanism**: reading order — specifically, band/fragment-to-row
pairing by nearest baseline mis-stitches a two-column form cell when the
left column's text wraps to more lines than the right column's, splicing the
right column's content into the middle of the left column's wrapped text
instead of after all of it. Confirmed on ~25 of 26 value-box occurrences on
this page (`Observed`).

---

## r000396 — the same band/fragment mechanism at table scale, plus a checkbox tofu glyph

**Observed**: end-to-end CER 29.035%, line-matched CER **19.548%** — a
9.487pp gap, the largest of any page measured in this round or round 2,
strongly implicating reading order. Top confusions: `""->" " 69`,
`""->"®" 24`, `" "->"" 22`, `"i"->"" 20`.

**Observed**, `--raw`: the back half of the page (a checkbox-style table:
`i.`/`ii.`/`iii.`/... item markers beside multi-line question text) shows
whole blocks read in the wrong order — e.g. the shared header text `"Did the
Fund rely on the following statutory exemption..."` is interleaved with
`"Is the Fund excepted from the rule 18f-4..."`, a *different* row's
question, rather than each question's full text appearing together.

**Observed**, `--layout --no-decode`: the underlying structure is the same
shape as r000044's — a narrow, 2-member `band fragment 1 of N` (the roman-
numeral marker, x-height 12.00, cap 22-23) paired by shared baseline with a
wide `band fragment N of N` (the wrapped question body, 24-51 members,
x-height 14-20), e.g. `line 74: x 46..552, ... band fragment 1 of 3` /
`line 75: x 723..750, 2 members, ... band fragment 2 of 3` /
`line 76: x 927..1578, 46 members, ... band fragment 3 of 3` — three-way
splits recur throughout this region. **Inferred**: because each roman-
numeral marker sits beside only the *first* line of its own multi-line
question, and the question bodies here commonly run 2-4 wrapped lines, the
same nearest-baseline pairing failure named on r000044 recurs here at a
larger scale and with more numbered items in play, producing the block-level
scrambling `--raw` shows rather than a single local splice.

**Observed**, secondary mechanism: `""->"®" 24` — a checkbox glyph rendered
in the source (`Observed` by pixel crop of the "Yes"/"No" answer boxes and
the numbered-list bullets) is not in the charset and is being matched to the
nearest available class, `®`, once per checkbox on the page (`"® Yes"`,
`"® NO"`, `"® i."`, `"® ii."`, ... — 24 insertions, consistent with roughly
one per checkbox/bullet row). This is a missing-glyph-shape mechanism
(tofu/notdef substitute), independent of the reading-order mechanism above,
and a smaller contributor by raw edit count (24 vs. the ~91-char
`""->" "`/`" "->""` pair attributable to fragment-splicing).

**Named mechanism**: reading order — the same band/fragment nearest-baseline
mis-pairing as r000044, here operating across a multi-row checkbox table and
producing block-scale reordering rather than a single-cell splice (9.487pp
end-to-end/line-matched gap, the largest measured this round). A checkbox
tofu-glyph substitution (`""->"®"`, 24 occurrences, missing glyph shape) is
a real but smaller secondary mechanism on the same page.

---

## r000407 — the same band/fragment mechanism on field-label/value rows, plus checkbox tofu and a dash-class confusion

**Observed**: end-to-end CER 24.464%, line-matched CER 22.123% — a 2.341pp
gap. Top confusions: `""->" " 69`, `""->"t" 20`, `"-"->"–" 20`, `"t"->"" 20`,
`""->"®" 17`, `"e"->"" 16`, `""->"e" 16`, `"\n"->" " 15`, `" "->"" 15`.

**Observed**, `--raw`: this page is a repeated custodian-record form —
`"i. Full name"` / value, `"ii. LEI, if any"` / value, each pair on its own
truth line (`REF`: `"i. Full name\nJ.P. Morgan S.A. DTVM\n..."`) but decoded
onto one line joined by a space (`GOT`: `"i. Full name J.P. Morgan s.A
DTVM\n..."`), the identical band/fragment mechanism named on r000044 (label
+ value share a baseline, joined with a space rather than matching truth's
per-field newline). This is the direct source of the large `""->" "` (69)
and `"\n"->" "`/`" "->""` (15/15) counts, and the resulting local
misalignment is `Inferred` to cascade into the symmetric `""->"t"`/`"t"->""`
(20/20) and `""->"e"`/`"e"->""` (16/16) pairs — same-count insert/delete
pairs of common letters are the signature of an edit-distance alignment
recovering after a splice, not of the letters themselves being
misrecognised.

**Observed**, secondary mechanisms, both real and separately countable:
- `""->"®" 17` (plus related `"~" 6`, `"B" 6`): the same checkbox-glyph
  tofu substitution as r000396, once per Yes/No/numbered-item checkbox on
  this page's 9-item custody-type list, repeated for each of the 3
  custodian records shown.
- `"-"->"–" 20`: every numbered list item's separator hyphen (`"1.Bank -
  section..."`) is read as an en-dash rather than the source's plain
  hyphen — a genuine matcher-level class confusion between two dash glyphs,
  recurring with total reliability on this page's list format (`Observed`,
  every instance in the visible `--raw` text shows the same substitution).
  `Inferred`: unrelated to reading order or segmentation; a prototype-bank
  confusion between hyphen and en-dash shapes at this font/size.

**Named mechanism**: reading order — the same band/fragment nearest-baseline
label+value splicing as r000044 (field label and its value joined with a
space instead of truth's newline, `Observed` on every field row shown).
Two smaller, independent secondary mechanisms recur: checkbox tofu-glyph
substitution (missing glyph shape, ~17-23 occurrences) and a hyphen/en-dash
matcher confusion (20 occurrences, specific to this page's numbered-list
formatting).

---

## r000022 re-measured: touching-ink and header-fusion diagnosis still holds

Prior diagnosis: `docs/measurements/2026-09-23_dense_table_r000022.md`,
measured at end-to-end CER 40.144%, line-matched CER 37.154%, before
`lines.baseline_split_valley_margin=0.3` shipped.

**Observed**, current build (no STALE, engine `t=1790197955` > model
`t=1790197657`): end-to-end CER **37.099%**, line-matched CER **34.330%** —
both down ~3.0pp from the prior reading, closely tracking the corpus-wide
average improvement from the line-fusion fix (13.161 vs. 16.089, ~2.9pp).
This is consistent with generic page-wide gains (ordinary body text lines
no longer fusing) rather than any change to this page's own named
mechanisms.

**Observed**, touching-ink mechanism (Finding 1 of the prior doc):
`--raw`'s `GOT` string still contains `"Merchandise $ 255® 2427 -3.3% ..."`
for truth `"Merchandise\n$\n2,350$\n2,427\n-3.3%\n..."` — **byte-identical**
to the prior document's `"255®"` reading for `"2,350$"` (comma lost,
`3`->`5`, the touching `0$` pair still fused into one component matched to
`®`). The specific atom coordinates named in the prior document (`x
263..360, baseline 530.0`) now correspond to a *different* word
(`"Equipment"`, confirmed by pixel crop) because band/line indexing shifted
slightly with the intervening fixes — but the same touching-glyph mechanism,
on the same `"2,350$"` cell, produces the same wrong output. Diagnosis
holds, mechanism unchanged.

**Observed**, header-fusion mechanism (Finding 2 of the prior doc), checked
against all five underlined headers:

| header | prior reading | current reading | changed? |
|---|---|---|---|
| Local Currency Growth | `"≈b"` (2 chars) | `"Local cu™ncy Gæwt11"` (legible) | **improved** |
| U.S. Distribution and... | `"l≈d Se™cæ"` | `"US. Dis™iutioo and valu‰Added Semîcæ"` (legible) | **improved** |
| International Distribution... | `". . . . ded Semîcee"` | `". . . . ded Semîcee"` | **unchanged, byte-identical** |
| Global Distribution... | `". . . . ed Semïcee"` | `". . . . ed Semïcee"` | **unchanged, byte-identical** |

**Inferred**: the two headers whose fused blob previously fell *under*
`furniture_fraction`'s width gate (kept as one wrong glyphish blob) now
decode far more legibly than before, plausibly because a downstream change
since that document (`segment.split_min_x_heights` 1.15->1.09, or
`lines.baseline_split_valley_margin`) lets the matcher/decoder recover more
of the fused shape's structure even though the component itself is still
one fused blob — this was not traced further (out of this report's
diagnosis-only, no-code-change scope) and is offered as an observation, not
a claim about which specific change caused it. The two headers whose fused
blob exceeds the width gate (dropped outright as furniture) are completely
unaffected — same input, same rejection, same near-total text loss.

**Verdict**: the touching-ink and header-fusion diagnosis from
`2026-09-23_dense_table_r000022.md` **still holds** as this page's dominant
mechanism. The page's ~3pp CER improvement is attributable to the
corpus-wide line-fusion fix helping this page's ordinary body text, not to
either named mechanism being resolved; one of the two named mechanisms
(short fused headers under the furniture-width gate) shows a real but
partial secondary improvement not previously present.

---

## Which mechanisms recur across pages

**The dominant new finding this round**: a single mechanism —
*band/fragment-to-row pairing by nearest baseline, joining a narrow column's
content (a value box, a checkbox, a roman-numeral list marker) onto whichever
wide-column line happens to share its baseline* — is the dominant mechanism
on **three of the four newly-examined pages** (r000044, r000396, r000407),
and is previously undocumented (distinct from the whole-line-fusion
mechanism `lines.baseline_split_valley_margin=0.3` already fixed, which
merges two *same-column* lines rather than mis-pairing two *different-
column* fragments). It manifests differently by page shape — a single-cell
splice on r000044 and r000407's simple label/value forms, a block-scale
scramble on r000396's larger checkbox table — but the underlying cause
(`Line::band`'s same-baseline join, per `pipeline.rs`'s own doc comment) is
the same in all three, and is directly implicated by each page's line-
matched-vs-end-to-end gap (r000044: -3.2pp reversed; r000396: +9.5pp, the
largest measured either round; r000407: +2.3pp).

**A recurring secondary mechanism**: checkbox/form-widget glyphs with no
charset coverage matching to a nearest available class (`®`, `~`, `B`) —
present on r000396 (24 occurrences) and r000407 (17-23 occurrences,
depending on which garbled variant is counted) — the same missing-glyph-
shape category `docs/measurements/2026-09-23_worst_pages_round2.md` section
(b).3 named as a smaller contributor on r000308's checkbox sidebar, now
confirmed on two more pages. This is consistent with
`2026-09-23_finfilings_audit.md`'s note that NPORT-P grid forms (35/60
pages) are checkbox-heavy.

**r000011 and r000022 are not part of this pattern**: r000011 is a known
render-side ground-truth defect (unfixable in this engine), and r000022
remains dominated by the already-diagnosed touching-ink/header-fusion pair,
unchanged in kind by the recent fixes.

## Single next change with the largest likely corpus-wide payoff

**Fix the band/fragment join so a narrow-column fragment is placed after
the full multi-line text of the wide-column fragment it is visually
adjacent to, not spliced in at whichever single line shares its baseline**
(`crates/ocrcer-core/src/pipeline.rs`'s `Line::band` assignment, built from
`layout/lines.rs`'s column-cut fragmentation). Reasoning: this mechanism was
observed dominant on three of the four pages examined this round — more
pages than any single mechanism named in round 2 — and its per-page cost is
large where it fires (r000396's 9.487pp reading-order gap is the largest
gap measured across both rounds' worst-page work). It is also, per
`2026-09-23_finfilings_audit.md`, tied to a common corpus layout (two-column
label/value forms and checkbox tables), so a fix here plausibly generalises
beyond these four pages the way the whole-line-fusion fix generalised beyond
r000308/r000363 — a claim this document does not confirm without a
full-corpus sweep for the same `band fragment` narrow-plus-wide-baseline-
pairing shape, which would be the natural next measurement before any
parameter or logic change ships. By contrast, r000583's bbox-chaining
mechanism (the single worst page) is already partially shipped and
document-specific to one filing's font rendering; the checkbox tofu-glyph
mechanism is real but smaller per-page than the band/fragment mechanism
everywhere it co-occurs with it (r000396, r000407); and r000011's defect is
not fixable in this engine at all.

**This document does not recommend specific code or constants.** Per
`CLAUDE.md`, the shape of the fix (whether to defer band assignment until a
narrow fragment's paired wide fragment's *full* wrapped extent is known,
whether to key it off column-cut geometry rather than baseline alone, and
what fixture would pin the corrected behaviour without breaking genuine
same-row two-column text) is `ocrcer-architect` territory.
