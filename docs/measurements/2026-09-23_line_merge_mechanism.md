# Line-merge mechanism: phase 1 diagnosis

Phase 1 of the `ARCHITECTURE.md` section 11 entry "The real-filings deletions
are merged lines; the mechanism is pinned before any rule is written."
Diagnosis only — nothing in this document is shipped. Background:
`docs/measurements/2026-09-23_finfilings_deletions.md`.

Pages: `filing__r000055`, `filing__r000088`
(`D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings`). Build: `target/release`.
Model: `model/out/ocrcer.ocrw`.

Every claim below is tagged **Observed** (read directly off instrumentation,
pixel data, or a run's printed output) or **Inferred** (reasoned from the
code plus the Observed evidence, not independently measured).

## 1. Instrumentation

`best_band` in `crates/ocrcer-core/src/layout/lines.rs` was temporarily
patched to record, for the band each component was actually joined to,
which test admitted it (`overlap_fraction` vs `hangs_below`), gated behind
`OCRCER_DEBUG_BANDS=1` so normal runs are unaffected. `hangs_below` itself
was not modified, only read. The patch has been reverted (see section 6);
this document is the only remaining record of it.

## 2. The mechanism

**Observed.** Every admitted join into every band on both pages, across the
full page — not just the bands that end up flagged x-height 1.00 — went
through `overlap_fraction`:

```
r55: 2898 joins, 2898 admitted_by=overlap_fraction, 0 admitted_by=hangs_below
r88: 2693 joins, 2693 admitted_by=overlap_fraction, 0 admitted_by=hangs_below
```

This resolves the open question in `ARCHITECTURE.md` section 11 directly:
`hangs_below` (the descender-reach test) plays no part in these merges.
`overlap_fraction` alone both builds every legitimate line on these pages
and admits the erroneous cross-line joins.

**Observed**, concrete example — `filing__r000055` band #0 (the band that
becomes the line reported x-height 1.00, "Observed"):

```
DEBUGBAND band#0 band_y=[357,393] band_median=36 n_members=1  <- comp y=[353,373] h=20 h/median=0.56 ov=16 admitted_by=overlap_fraction
DEBUGBAND band#0 band_y=[353,393] band_median=36 n_members=2  <- comp y=[353,373] h=20 h/median=0.56 ov=20 admitted_by=overlap_fraction
DEBUGBAND band#0 band_y=[353,393] band_median=20 n_members=5  <- comp y=[353,371] h=18 h/median=0.90 ov=18 admitted_by=overlap_fraction
...
DEBUGBAND band#0 band_y=[353,393] band_median=18 n_members=15 <- comp y=[375,393] h=18 h/median=1.00 ov=18 admitted_by=overlap_fraction
DEBUGBAND band#0 band_y=[353,393] band_median=18 n_members=16 <- comp y=[375,393] h=18 h/median=1.00 ov=18 admitted_by=overlap_fraction
...（continues absorbing y≈[375,393] members through n_members=247, down to
    3px punctuation at ratio 1.00 by the band's own shrunk median）
```

Members 1–14 (y-tops 353–357, y-bottoms 371–375) are the first text line
("d. For each of the preceding three months..."). Starting at member 15,
components with y=[375,393] begin joining — a full 18px lower, matching the
*second* text line ("attributable to investment other than derivatives...").
The join is admitted with `ov=18`, `den=min(18,18)=18`, ratio 1.00: by the
time the band's running median has shrunk to line 2's own glyph height, line
2's members overlap the band by their *entire* height, clearing
`overlap_fraction` (default 0.5) with no margin to spare, and clearing even a
threshold as tight as 0.7 unchanged (section 4).

**Observed**, visual confirmation (`filing__r000055`, y≈[340,412], ruler
crop `r55_ruler_260_520.png`, and a numpy ink-row scan of the same span):
band #0's absorbed pair is ordinary prose text at tight leading — baseline
spacing of roughly 24px against a ~18–20px glyph height. It is not
punctuation, not an underline or rule. The ink-row scan found no fully-blank
row separating the two lines: minimum non-zero ink count throughout the
span, with one row at exactly 0 immediately adjacent to a row with only 2 ink
pixels. Line 1's own lower-zone ink (round-letter overshoot and its own
descenders) extends down to almost exactly where line 2's upper-zone ink
(x-height tops) begins, so the two lines' bounding rows genuinely,
substantially overlap in y — this is not a mismeasurement of otherwise
well-separated lines, the ink itself interlocks at this leading.

`filing__r000088`'s largest merged band (218 members, y=[34,78], ruler crop
`r88_top_ruler.png`) is the same mechanism on a 3-line bold heading
paragraph: tight leading, `overlap_fraction`-only admission, no visible
blank separator row.

**Inferred.** The two tests are not interchangeable in this failure:
`hangs_below` requires a component's *top* edge to sit inside the band and
its bottom to hang down by no more than `descender_reach_fraction × median`
below the band's current bottom — a component whose top is already 18px
below the band's top (as line 2's members are, relative to where the band
started) is well outside that test's `c.y0 < band.y1` admission window in
spirit, and in practice this trace shows it is `overlap_fraction`, not
`hangs_below`, doing the admitting throughout. The bug is not that the
descender-reach test is too permissive; it is that the plain overlap ratio,
computed against the band's own *running* median (which shrinks every time a
smaller component joins), cannot tell "more ink belonging to this line" from
"a second line's ink that happens to occupy the same rows."

## 3. Why `measure` reports x-height 1 as `Observed`

**Observed** (code trace, `crates/ocrcer-core/src/layout/lines.rs:719-801`):

1. `baseline` is the width-weighted mode of `body` members' `y1`
   (line 765-766). For band #0, the printed baseline is 370.0 — line 1's
   baseline wins the histogram peak (comparable total width between the two
   absorbed lines, but line 1's peak wins in this instance).
2. `tops` is built as `(baseline - c.y0).max(0.0)` per member (line 772-775).
   Every member of the absorbed *second* line has `c.y0 >= baseline` (its
   top sits at or below where line 1's baseline was measured), so its `top`
   value clamps to exactly `0` for every one of them. This creates a large,
   width-weighted pileup of zeros in the `tops` histogram.
3. `main = mode_of_weighted(tops, 0, top_max+2).max(1)` (line 777) is pulled
   down to the floor of `1` by that pileup — this is the literal source of
   the reported `x-height 1.00`.
4. `upper` is every top value exceeding `main * 1.15` (line 781-782). With
   `main ≈ 1`, essentially every genuine top value from line 1 (its real
   x-height ~13px and cap-height ~17px measurements) clears `1.15` trivially,
   so `upper` is non-empty.
5. `!upper.is_empty()` (line 790) is the *sole* test for `XHeightSource::
   Observed` in this branch. It asks only "does a band clearly above the
   modal one exist," not whether the modal one itself has plausible support.
   With `main` a clamped artifact rather than a real measurement, the branch
   still returns `Observed`.
6. `cap = mode_of_weighted(upper, main, top_max+2)` (line 791) is the
   weighted mode over the combined `upper` bucket, which now holds *both*
   line 1's true x-height values (~13) and its true cap-height values
   (~17) merged into one histogram, because the `1.15×main` cut is no longer
   discriminating between them (both clear it once `main≈1`). The printed
   `cap 13.00` for band #0 is consistent with this: ordinary running text has
   far more x-height-only ink (width-wise) than ascender/cap ink, so the
   weighted mode of the merged bucket lands near the true x-height, not the
   true cap-height. **This specific cap-height claim is inferred from the
   code and is consistent with the printed value, not independently verified
   against exact histogram bin counts.**

**Observed, and the more consequential half of this defect:**
`inherit_x_heights` (lines 365-397) is the codebase's existing safety net
for exactly this class of symptom — "a line's x-height came out below a
plausible fraction of the page's typical x-height." Its doc comment states
the design choice explicitly: *"a line that observed a band is never
overridden, however small it is, because it has evidence and a page average
does not outrank evidence"* (lines 360-361), and the code enforces this with
`if l.x_height_source == XHeightSource::Observed { continue; }` (line
387-389). Because step 5 above hands this degenerate line the `Observed`
label, the one mechanism designed to catch an implausible x-height is
explicitly and correctly (per its own contract) skipped for it. The
mislabeling is not merely cosmetic — it actively defeats the existing
safety net.

## 4. Counterfactuals (few-page readings, not measurements)

`--set` overrides via `ocr.exe`, `--only <page>`, both pages, single run
each. CER/F1/WER are end-to-end (decoded output vs. line-level ground
truth), not line-matched.

| config | r000055 CER | r000055 F1 | r000088 CER | r000088 F1 |
|---|---|---|---|---|
| baseline (0.5 / 0.4) | 45.320% | 56.460% | 34.420% | 58.144% |
| `overlap_fraction=0.6` | 45.320% (unchanged) | 56.460% | 35.294% (worse) | 57.462% |
| `overlap_fraction=0.7` | 45.320% (unchanged) | 56.460% | 35.294% (same as 0.6) | 57.462% |
| `descender_reach_fraction=0.3` | 45.320% (unchanged) | 56.460% | 34.420% (unchanged) | 58.144% |
| `descender_reach_fraction=0.2` | 45.320% (unchanged) | 56.460% | 34.420% (unchanged) | 58.144% |
| `overlap=0.6` + `reach=0.3` | 45.320% (unchanged) | 56.460% | 35.294% (same as overlap alone) | 57.462% |
| `overlap_fraction=0.95` (probe, not in original list) | 51.178% (worse) | — | 39.966% (worse) | — |

Occurrence count of bands reported x-height 1.00 (`--layout --no-decode`,
same pages):

| config | r000055 | r000088 |
|---|---|---|
| baseline | 3 | 5 |
| `overlap_fraction=0.6` | 3 (unchanged) | 5 (unchanged) |
| `overlap_fraction=0.7` | 3 (unchanged) | 5 (unchanged) |
| `overlap_fraction=0.95` | 0 | (not rerun; CER already much worse) |
| `descender_reach_fraction=0.2` | 3 (unchanged) | 5 (unchanged) |

**Observed.** `descender_reach_fraction` has zero effect at either tested
value, on either page, on either metric — fully consistent with section 2's
finding that `hangs_below` never fires on these merges; tightening a test
that never admits anything here cannot change the outcome.

`overlap_fraction` at 0.6 or 0.7 has zero effect on `r000055`'s three
degenerate bands or its CER, and only eliminates the diagnosis at the
extreme value 0.95 — at which point CER on *both* pages gets markedly worse
(51.2% and 40.0% vs. 45.3% and 34.4% baseline), not better. On `r000088`,
even the mild 0.6/0.7 tightening already regresses CER (34.420% →
35.294%) without changing the x-height 1.00 occurrence count at all.

**Inferred.** The band #0 trace in section 2 explains both halves of this:
the joins that build the erroneous merge on `r000055` have overlap ratios of
0.90 and 1.00 — at or above even a 0.7 threshold, so no reading in the
originally requested 0.6/0.7 range can exclude them; only pushing past where
real intra-line joins in this font/leading also live (ascenders overlapping
their own x-height band, round-letter overshoot commonly sit in the 0.8–1.0
range) starts to work, and by then it is also excluding legitimate same-line
joins elsewhere on the page — which is the direct cause of the CER
regression at 0.95, and very likely also the unexplored cause of `r000088`'s
milder regression at 0.6/0.7 (not independently root-caused in this pass;
would need a targeted `--layout` diff to name the specific broken line).
**The overlap ratio, on its own, cannot separate this defect from the cases
`overlap_fraction` exists to admit** — the ratio a cross-line join achieves
at tight leading is not distinguishable from the ratio a same-line
ascender-over-x-height join achieves. Any fix operating purely on this ratio
trades one failure for another; it is the wrong axis to tighten.

## 5. Proposed rules

These are proposals with reasoning and expected side effects, per the task.
Constants and final rule text are `ocrcer-architect` territory — this is not
a self-serve fix, per this agent's scope limits.

### 5a. The second line being admitted

**Do not tighten `overlap_fraction` or `hangs_below`.** Section 4 shows the
ratios that admit the bad join and the ratios that admit legitimate
same-line joins overlap; a threshold that excludes one excludes the other,
and the counterfactuals show the net effect is a worse page, not a better
one, well before it is tight enough to fix these two bands.

**Proposed direction: a baseline-bimodality check, not a ratio check.**
Every real text line has exactly one baseline. A band produced by merging
two lines has two — the same asymmetry section 3 exploits to explain the
`tops` pileup, seen from the other side. Add a check, either during band
growth or as a post-`measure()` pass, that looks at the *baseline*
candidates (`c.y1` for `body`-sized members, the same population `measure`
already filters to before building its `tops` histogram) rather than the
top-edge histogram: if that population is bimodal — two well-separated,
comparably-supported peaks, separated by roughly the band's own median
height or more, with a near-empty valley between them — the band is two
lines, and should be split at the y-value between the peaks, each half
re-measured independently.

This is deliberately keyed off the *baseline* population and not the *top*
population: an ordinary line with both cap-height and x-height content (or
descenders) has one baseline and a bimodal top histogram by design — that
bimodality is exactly what lets `measure` tell x-height from cap-height in
the first place (line 781-782), and a rule that flagged top-histogram
bimodality as "two lines" would break every mixed-case line on the corpus.
Baseline bimodality does not have that false-positive path: mixed case,
descenders, ascenders all still land on one baseline.

**Expected side effects.** Marks (`i`-dots, accents) are already excluded
from `body` — and so from this proposed baseline population — by the
existing `>= 0.5 × reference` height filter (line 757-761); a baseline-
bimodality check inherits that protection for free and should not need to
reason about marks separately. A line with heavy descenders still has one
baseline (descenders extend `y1` for their members but `measure` reads
baseline from the modal `y1`, and a `g`/`p`/`q` population large enough to
shift the baseline mode would itself indicate genuinely two rows of
baseline-bearing ink, which is the case this rule is meant to catch, not
avoid). The known risk is very tight interline leading in a *real* single
page layout that is not a merge — e.g. a table row where two independent
short fields happen to sit close vertically — where a bimodal baseline could
be a false positive; that risk needs a fixture built specifically to probe
it before this ships, which is `ocrcer-architect`/fixture work, not
something resolved here.

### 5b. The non-observation being labelled `Observed`

**Proposed direction: a plausibility floor between `main` and `cap` in the
`!upper.is_empty()` branch**, i.e. reject `Observed` (fall through as if
`upper` were empty, into the existing `descends`/`FromCapHeight` branches)
when `main` is implausibly small relative to `cap` — e.g.
`main < floor × cap` for a conservative floor well below any real typeface's
x-height/cap-height ratio (`X_HEIGHT_PER_CAP` is measured at 0.7431; even a
very condensed or geometric face is not going to read below roughly 0.3–0.4;
the observed defect here is ~0.08). The exact floor is an authored constant
and belongs with `ocrcer-architect`, the same way `x_height_per_cap` itself
was measured rather than guessed (line 110-112) — this document proposes the
*shape* of the check, not the number.

This is the more surgical of the two proposals because of what section 3
already established: falling through routes the line into
`inherit_x_heights`, a safety net that already exists, is already tested,
and is already designed to replace an implausible x-height with the page's
typical one. The defect is that `Observed` currently defeats that net by
construction; this proposal simply stops mislabeling the input to a
mechanism that already does the right thing once it sees the line.

**Expected side effects.** Genuine all-caps or small-caps lines are not at
risk: those have `main` equal to the cap-height reading itself (no separate
x-height tier), so `!upper.is_empty()` should rarely trigger for them in the
first place — a single stray tall mark would need enough width-weighted
support to form its own histogram peak, which an isolated accent or stray
speckle does not have. The plausibility floor should not fire on true
condensed-font body text either, provided the floor is set with headroom
below any real measured `x_height_per_cap` value across the shippable font
set — which is exactly the kind of number `ocrcer-build metrics` already
computes for `X_HEIGHT_PER_CAP`, and the natural place to source this floor
from rather than inventing it. The main open question, left to the
architect: whether the fallback on rejection should be `FromCapHeight`
(treat `main` as unreliable, `cap` as the only usable measurement) or route
through `descends` first — this document has not evaluated which reads
better on a merged band vs. a genuine edge case, only that either is
preferable to the current unconditional `Observed`.

### Relationship between 5a and 5b

5a, if implemented, removes the specific bands this document traced (the
merged band would be split before `measure` ever sees it, so `main` would
not be a clamped artifact). 5b is proposed independently and in addition,
because the `!upper.is_empty()` gate's blindness to `main`'s own plausibility
is a defect on its own terms — any other pathological band, from an
unrelated cause, that produces a tiny `main` next to a real `upper` would
trigger the same mislabeling and the same defeat of `inherit_x_heights`. It
is a cheap, narrowly-scoped check that reuses existing, already-tested
machinery, independent of whether 5a ships.

## 6. Instrumentation reverted

The temporary `OCRCER_DEBUG_BANDS`-gated patch to `best_band` in
`crates/ocrcer-core/src/layout/lines.rs` has been reverted to its original
form. `cargo test --workspace` is green (see report).
