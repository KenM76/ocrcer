# Worst pages, round 2: r000583 after the merge fix, and the mechanism dominating r000308/r000363

Diagnostic only. No param default, `params.tsv`, model, or `ARCHITECTURE.md`
entry was changed to produce these findings. The one piece of temporary
instrumentation — an `OCRCER_DEBUG_MEMBERS`-gated dump of each line's member
component bounding boxes, added to `run_layout` in
`crates/ocrcer-bench/src/bin/ocr.rs` — was built with
`CARGO_TARGET_DIR=target/runtime-diag`, is inert unless the env var is set,
and has been removed with `git checkout --` after this document was written;
`git status` is clean apart from this file. All CER/confusion numbers quoted
below come from the unmodified `./target/release/ocr.exe`; only the raw
member-bbox dumps used `target/runtime-diag/release/ocr.exe`. Every page was
run individually, `--only filing__rXXXXXX`, one process at a time.

Every claim is tagged **Observed** (read directly off `--layout`/`--show 1`
output, the member-bbox dump, or pixel crops) or **Inferred** (reasoned from
Observed evidence plus reading the code, not separately measured).

Context: the shipped control is `segment.merge_overlap_frac=0.4`. Corpus-wide
finfilings end-to-end CER is 16.089, line-matched 15.910. Worst pages by
end-to-end CER: r000583 (40.63), r000022 (37.10, already diagnosed as
touching ink), r000308 (34.45), r000055 (32.49, already diagnosed as the
underline-strip sliver), r000363 (32.27).

---

## (a) r000583 — after the merge fix, still the same mechanism, just smaller

**Observed**: end-to-end CER dropped from 54.661% (the `merge_overlap_frac=0`
legacy-any-overlap control measured in
`docs/measurements/2026-09-23_worst_page_r000583.md`) to **40.633%** under
the shipped `0.4`. It is still the worst page on the corpus. Top confusions
are unchanged in *kind*, only in *scale*:

```
47  "t" -> ""      18  "f" -> ""      18  "i" -> ""     18  "l" -> ""
17  "e" -> "æ"     17  "r" -> ""      12  "h" -> "m"    12  "i" -> "m"
12  "o" -> ""      10  "l" -> "/"     10  "t" -> "æ"     9  "," -> ""
```

This is the same signature the prior document named: deletions and
symbol substitutions concentrated on ascender/crossbar letters
(`t h f l k b d`, plus `i l` from dot/serif chaining), round letters
(`a n d o s u`) largely spared. Line-matched CER (40.633%) is bit-identical
to end-to-end, so this remains not a reading-order problem, exactly as
before. **Inferred**: raising `merge_overlap_frac` to 0.4 required more
column overlap before gluing two components into one atom, which fixed the
majority of the 2-and-3-member chains from the prior measurement (the ones
whose overlap was a small fraction of the narrower component's width) but
left the tightest chains — where one component's ink genuinely sits mostly
inside the other's column range — still glued, because those clear the
"either fully inside the other's range always merges" branch the 0.4
threshold does not gate. The mechanism is unchanged: serif crossbar/base-serif
bounding-box overlap glues pixel-disjoint letters into oversized atoms that a
fixed 3-cut budget and a vertical-only cut cannot cleanly separate. It is
smaller (54.66 -> 40.63) but not resolved, and remains this page's dominant
loss.

## (b) r000308 — line-grouping merges pairs of adjacent real text lines into one band; word-splitting's "Fallback" mode is the downstream symptom, not the cause

**Observed**: end-to-end CER 34.454%, line-matched CER 33.917% — a 0.537pp
gap, small enough that reading order is not a material factor here (ruled
out). Top confusions are a different shape from r000583's: broad deletion
across nearly every common letter in rough proportion to English letter
frequency, plus space churn:

```
87  " " -> ""      68  "e" -> ""      65  "i" -> ""     52  "t" -> ""
45  ""  -> " "      38  "a" -> ""      38  "n" -> ""     37  "s" -> ""
36  "o" -> ""      36  "r" -> ""      31  " " -> "g"    26  "h" -> ""
```

`--layout` reports **123 read-side lines against 92 truth lines** — net
*more* lines than truth, which looks like over-splitting, but that net
figure hides two opposite failures happening in different regions of the
same page (section (b).3).

### (b).1 The dominant failure: two ordinary text lines fused into one band

**Observed**, `--layout --show 1`: 18 of the 123 read-lines fall back to
`ThresholdSource::Fallback` (no valley found in the line's gap population,
`eta 0.000`) — but member-count-weighted, those 18 lines are not uniform.
Four of them are large (72, 246, 264, 270 members); the other 14 are tiny
(1–25 members, mostly checkbox/label fragments, see (b).3). The four large
ones alone hold 852 of the page's 3272 total glyphish members (26.0%); all
18 Fallback lines together hold 943 (28.8%).

Member-bbox dump (`OCRCER_DEBUG_MEMBERS=1`) of the first large Fallback
line (`x 53..1574, 270 members, x-height 13.00 (Inherited), baseline 70.0`):

```
member x0=53 x1=63 y0=57 y1=71 w=10 h=14
member x0=54 x1=65 y0=79 y1=97 w=11 h=18
member x0=65 x1=76 y0=57 y1=70 w=11 h=13
member x0=67 x1=79 y0=79 y1=92 w=12 h=13
...
```

**Observed**: members alternate between two disjoint y-bands — `y0≈57..71`
and `y0≈79..97` — 8px apart vertically with no overlap, and each band's own
component heights (13–18px) are ordinary single-glyph sizes. This is not
touching ink and not serif bbox chaining (section (a)): it is **two whole,
cleanly-separated text lines, interleaved by x-position into one line
object**. A raw-pixel crop of `x=0..900, y=50..100` confirms it by eye —
two normal, non-overlapping rows of prose (`categories only in the
following circumstances: (1) if portions of the position...` directly above
`portions separately; (2) if a fund has multiple sub-advisers with
differing liquidity...`), each perfectly legible on its own, no visual
ambiguity a person would have.

**Observed**, cross-checking against `filing__r000308.truth.json`: this
matches truth lines 1 (139 chars) and 2 (144 chars) exactly — the read-side
line's primary/seed baseline (70.0) equals truth line 1's real page
baseline, and the absorbed secondary row (baseline ≈92) is truth line 2.
`--layout`'s own `want <i>` printer indexes into the truth array by the
*read*-side loop counter, so it happens to print truth line 1's text for
this read-line by coincidence of index, not by content matching — a second
member-bbox dump (`OCRCER_DEBUG_MEMBERS=2`) of the *next* large Fallback
line shows the identical pattern one baseline pair later (`y0≈98..115` and
`y0≈120..138`, baseline spacing again 22–23px), which by the same reasoning
fuses truth lines 3 and 4 (137 and 133 chars). These two merges alone
account for 553 of the page's 3538 truth chars (15.6%) decoded as one
fused, x-interleaved atom stream each; the other two large Fallback lines
(72 and 264 members, one of them inside the narrow sidebar column named in
section (b).3) are the same mechanism recurring elsewhere on the page.

**Inferred**: `lines.rs`'s `group_with_bands` visits components tallest
first, page-wide, and `best_band` absorbs a component into an existing band
whenever their vertical overlap clears `overlap_fraction (0.5)` of the
smaller height, or the component "hangs below" the band by no more than
`descender_reach_fraction (0.4)` of the band's median height. Once a band's
`y1` creeps downward absorbing one borderline component, subsequent
components from the *next* real line become eligible by the same test,
avalanching into a full two-line merge. **Observed**: the merged region's
baseline-to-baseline spacing here is ~22–23px against a 13px x-height (ratio
≈1.7); the same measurement on r000583 (baselines 108, 138, 169, 199 within
one paragraph, x-height 14–15) gives ~30–31px spacing, ratio ≈2.0–2.2 — a
visibly looser leading. r000583 has only 1 Fallback line of 11 (a short,
2-word line failing purely for lack of gaps, x-height source `Observed`, not
a merge), consistent with looser leading keeping it clear of the merge
threshold that this page's tighter, single-spaced regulatory-filing leading
crosses. This is *not* the same mechanism as the underline-strip sliver
merge on r000055/r000044 (no strip, no debris component here — plain body
text merging directly) and *not* touching ink (the two rows have a clean
8px vertical gap between them in the example above). It is a third,
distinct instance of the same general failure class named in that
document's task 1: `group_with_bands`'s absorb test is not conservative
enough for some real leading ratios.

Once two lines are fused and x-interleaved, `words::gaps` — which assumes
its `line.members` (sorted by `x0`) represent one reading-order sequence —
computes near-zero gaps almost everywhere, because at any given x-position
the "next" member by x0 is as likely to be the *other* row's neighbouring
letter as this row's own next letter. The pooled gap population then shows
no separable valley (`ThresholdSource::Fallback`, `eta 0.000`), and the
`no_valley_x_heights` fallback threshold, applied to a population that no
longer represents real word boundaries at all, returns 3–5 "words" for what
should be ~15–19. This is why the corpus's confusion table shows broad
letter-frequency-proportional deletion (`e i t a n s o r h`) rather than the
ascender/crossbar-specific pattern of section (a): the decoder is not
failing on a particular letter shape, it is being handed a nonsense
multi-row atom stream and doing its best, which for most of it is nothing
— hence `" " -> ""` (87, the highest count: interleaved rows destroy real
inter-word spaces) and `"" -> " "`/`" " -> "g"` (spurious insertions where
the merged stream's decode occasionally lands on a class that happens to
look like a lowercase `g`).

### (b).2 Reading order and word splitting in isolation: not the story

**Observed**: the 0.537pp end-to-end/line-matched gap is far too small to
implicate reading order. **Observed**: on lines that are *not* fused (the
majority — 104 of 123 report `Valley`), word counts track truth closely
(e.g. line 0, 19 words decoded against 19 truth words; line 3, 19 against
19). Word splitting works normally once line grouping hands it a single,
un-fused row; it only collapses to `Fallback` as the direct downstream
consequence of section (b).1's line fusion.

### (b).3 A smaller, separate mechanism: a checkbox/form-field region fragments

**Observed**, an image crop at `y=240..300` (page 1653×2339) shows a genuine
form widget: a narrow left-hand text sidebar ("Indicate the level within the
fair value hierarchy...") beside four checkboxes labelled `1`, `2`, `3`,
`N/A`, each checkbox rendered with a visible "?" placeholder glyph inside
the box outline (a missing-glyph/tofu box — **Inferred**: the source PDF's
checkbox Unicode glyph had no coverage in whatever font rendered this page,
so a fallback notdef box was drawn; this is upstream corpus content, not an
engine defect). **Observed**, `filing__r000308.truth.json` lines 8–21
confirm this structurally: 10 short wrapped lines of the sidebar text
followed by four one-character lines (`"1"`, `"2"`, `"3"`, `"N/A"`) — a
genuinely complex micro-layout in a small page region. **Observed**: this
region drives `n_fragments > 1` ("band fragment k of N") column-cut splits
— e.g. one band split into 9 fragments, several holding just 1–2 members
(the isolated checkbox digit/N-A labels, each landing on its own narrow
column-cut slice) and one holding 32–72 members (the sidebar text, itself
still subject to the same line-fusion mechanism as (b).1, per the 72-member
Fallback line at `x 53..468`). This region is real and does cost characters
(72 of the 92 truth lines are outside this region; the sidebar+checkbox
block is roughly 14 short truth lines, well under 10% of the page's 3538
chars), but it is a much smaller contributor than (b).1 — an order of
magnitude fewer affected characters — and is named here for completeness,
not as the dominant mechanism.

### (b).4 Named mechanism, in one line

> Not reading order (0.537pp gap), and word-splitting only as a downstream
> symptom: r000308's 34.45% CER is dominated by `lines.rs`'s line-grouping
> fusing pairs of ordinary, cleanly-separated body-text lines into one band
> (4 such fusions hold 852 of 3272 page members, 26.0%; two of them
> confirmed by member-bbox dump and truth cross-reference to be exactly
> truth lines 1+2 and 3+4, 553 of 3538 truth chars, 15.6%, from just those
> two), because this filing's ~1.7x-height leading crosses
> `group_with_bands`'s absorb thresholds where r000583's ~2.0–2.2x-height
> leading does not. Once fused, word-gap valley detection sees an
> x-interleaved two-row stream with no real word boundaries left, falls
> back to a default threshold, and returns 3–5 "words" for what should be
> 15–19 — which is why the confusion table shows broad, letter-frequency-
> proportional deletion rather than a specific letter-shape signature. A
> smaller, separate checkbox/form-field column-cut fragmentation exists in
> one page region (well under 10% of the page's characters) and is not the
> driver.

---

## (c) r000363 — the same line-fusion mechanism, with a cleaner smoking gun

**Observed**: end-to-end CER 32.271%, line-matched CER 32.271% — bit-identical,
ruling out reading order even more cleanly than (b). Top confusions are the
same shape as r000308's: broad letter-frequency-proportional deletion plus
space churn:

```
72  "e" -> ""      69  " " -> ""      61  "i" -> ""     46  "t" -> ""
42  ""  -> " "      42  "a" -> ""      39  "n" -> ""     39  "r" -> ""
38  "o" -> ""      35  "s" -> ""      23  "f" -> ""     23  "u" -> ""
```

`--layout` reports 137 read-side lines against 90 truth lines. **Observed**:
17 of 137 lines are `Fallback`; four are large (72, 200, 261, 264 members),
holding 797 of the page's 3306 total members (24.1%) — 89.3% of all
Fallback-line members are in just these four, the same "handful of large
fusions plus many tiny benign short-line Fallbacks" shape as r000308.

**Observed**, this page's `family`/`px_per_em`/text content is the same
underlying filing document as r000308 (identical instructional paragraphs
for Item C.7/C.8, a different page/row of the same source), consistent with
the mechanism recurring because it is a property of this document's layout
(tight leading), not of one specific page.

**Observed**, the two largest Fallback lines here give a cleaner smoking gun
than r000308's did: line 5 (`x 53..1574, 261 members, baseline 198.0`)
reports `x-height 26.01 (FromCapHeight)` and line 25 (`x 53..1526, 200
members, baseline 243.0`) reports `x-height 26.75 (FromCapHeight)` — almost
exactly **double** the page's ordinary 13.00px x-height. **Inferred**: this
is the per-line x-height measurement failing outright (falling back to
deriving x-height from cap-height) on a band whose true population is two
stacked normal-size rows; the derived value lands near 2x the real x-height
because the fallback path is effectively measuring the fused band's total
vertical extent rather than one row's. This is a stronger and more direct
signature of the same "two lines merged into one band" failure named in
(b).1 than r000308's examples showed (which stayed at a correctly-Observed
13.00, since the modal component-height histogram still peaked at the
common per-glyph size even while fused).

### Named mechanism, in one line

> Same mechanism as r000308, confirmed independently on a different page of
> the same source filing and with a sharper signature: `lines.rs` fuses
> pairs of ordinary body-text lines into one band on this document's tight
> leading (4 fusions hold 797 of 3306 page members, 24.1%; the two largest
> report x-height inflated to almost exactly double the page's true 13.00px
> via `FromCapHeight` fallback, direct evidence the measurement is being
> taken over two stacked rows rather than one). Reading order is ruled out
> more cleanly than on r000308 (0.000pp end-to-end/line-matched gap). Word
> splitting's `Fallback` mode is again the downstream symptom of the fused,
> x-interleaved atom stream, not an independent cause.

---

## What this changes about where the corpus-wide payoff is

Two of the three pages examined here (r000308, r000363) are dominated by
the *same* mechanism, previously undocumented: `lines::group_with_bands`
fuses pairs of adjacent, cleanly-separated real text lines into one band on
this corpus's tighter-leading pages, at a rate that accounts for roughly a
quarter to a third of each page's ink and does not depend on any of the
already-shipped or already-diagnosed mechanisms (underline-strip debris,
serif bbox chaining, touching ink, split-gate width). **Inferred**: given
r000308 and r000363 are two pages of the *same* source filing and both show
it, and given the mechanism is a property of leading ratio rather than of
any one page's specific content, this is plausibly not confined to these
two pages — a full-corpus sweep for `ThresholdSource::Fallback` lines with
member counts well above their page's per-line median, or for
`x_height_source == FromCapHeight` lines whose derived x-height is roughly
double another line's `Observed` value on the same page, would be needed to
confirm the prevalence claim rather than infer it from two pages.

**This document does not recommend a specific parameter change.** Per
`CLAUDE.md`, tightening `overlap_fraction`/`descender_reach_fraction` (or
adding a distinct check — e.g. rejecting an absorption that would make the
band's height-to-median-height ratio jump sharply, which is what both
worked examples above show) is a fix that needs sweeping against both
`bench/pages-cov` and `finfilings`, the same discipline the column-cut and
split-gate changes in this file's neighbours followed; a plausible-looking
threshold change is not evidence until it is measured on both corpora.

## Single next change with the largest corpus-wide payoff

**Fix line-grouping's absorb test to stop fusing two full baselines'
worth of ink into one band** (`crates/ocrcer-core/src/layout/lines.rs`,
`best_band`/`hangs_below`). Reasoning: this single mechanism accounts for
roughly a quarter to a third of the ink on two of the five worst pages
measured on this corpus, on a document whose tight leading is unlikely to
be unique to these two pages of it, and its downstream cost is
disproportionate to its rate — a fused line does not lose a few characters,
it turns an entire line's worth of content into interleaved garbage that
scores as broad, near-total deletion. By contrast, section (a)'s bbox-
chaining mechanism (still the single worst page) was already worked down
from 54.66% to 40.63% by the shipped `merge_overlap_frac` change and is a
narrower, already-partially-addressed problem; the checkbox/form-field
fragmentation in (b).3 affects a small fraction of one page's characters.
A fix here is also the more surprising of the two remaining candidates —
unlike (a), which is a known, partially-shipped direction, this mechanism
was previously undocumented, meaning the corpus-wide finfilings CER number
(16.089) has not yet had a chance to reflect any correction for it.

---

## Debug code removed

The following temporary, env-gated instrumentation was added to
`crates/ocrcer-bench/src/bin/ocr.rs`'s `run_layout` to produce this
document and has been removed via `git checkout --`, confirmed by `git
status` showing no tracked changes outside this file:

- `OCRCER_DEBUG_MEMBERS`-gated `eprintln!`s printing each line's member
  components' `(x0, x1, y0, y1, width, height, area)`, filtered to a
  comma-separated list of line indices (or `all`) named by the env var.

None of this changed any default, `params.tsv` entry, or code path taken
when the env var is unset.
