# Two diagnostics: underline-strip prose loss on r000055/r000044, touching-glyph cut miss on r000022

Diagnostics only. No param default, `params.tsv`, model, or `ARCHITECTURE.md`
entry was changed to produce these findings. All instrumentation was
temporary, gated behind `OCRCER_DEBUG_STRIP` / `OCRCER_DEBUG_SEGMENT`, and has
been removed after this document was written (see the tail of this file for
the exact diff). Builds used `CARGO_TARGET_DIR=target/runtime-diag`. Every
page was run individually, `--only <page>`, one run at a time.

Every claim below is tagged **Observed** (read directly off instrumentation
or pixel data) or **Inferred** (reasoned from Observed evidence plus reading
the code, not separately measured).

---

## Task 1 — why underline-strip rule 2 erases prose on r000055 and r000044

Context: `ARCHITECTURE.md`'s 2026-09-23 "fails the real-filings gate" entry
names one *unchecked* hypothesis — that a merged/tightly-packed text block's
own row runs pass `rule_run_heights x h` because `h` is measured wrong. That
hypothesis is **not what happens**. The real mechanism is answer **(c)**,
something else: the strip's own debris is correctly narrow and correctly
short-lived, but it is *wide enough to survive both furniture filters* and
gets seeded into line-grouping ahead of real text, corrupting two adjacent
lines into one.

### 1.1 What the strip actually erases — bands sit on real rule lines

Command: `./target/runtime-diag/release/ocr.exe model/out/ocrcer.ocrw
D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings --only filing__r000055
--set lines.underline_strip=1 --show 1 --layout` (dropping `--no-decode`,
which is required — `--layout`'s own label/line dump never touches the strip;
only the `recognize_lines()` call that fills in "decoded" text does).

**Observed**, page-level: `h` (median glyphish-component height, computed
once, before line grouping) = 13.000, run floor `L = rule_run_heights(5.4209)
x h = 70.472`.

**Observed**, every one of the 18 bands the strip accepted on this page, e.g.
the first:

```
component bbox=(491,27)-(917,78) w=426 height=51 area=1419
  columns x0=491 x1=917: erased=423 kept(crossing)=3 no_ink=0
  band accepted x0=491 x1=917 y0=27 y1=30 (thickness=3 rows)
```

All 18 accepted bands on this page share the same shape: `w=426`,
`erased=423`, `kept(crossing)=3`, thickness 2-3 rows, `y0` a handful of
pixels below the component's own top. This is a **top border of a bordered
numeric-value box** on a financial-form table (the box holds a right-aligned
number like `"0.00000000"`), not prose: `erased=423` of 426 columns is a
near-total horizontal run, and `kept(crossing)=3` matches the box's own
left/right vertical sides passing straight through the band, exactly the
"keep crossing strokes" guard doing its job. A raw-grayscale crop of three of
these boxes (`(491,27)-(917,78)`, `(491,463)-(917,514)`,
`(491,906)-(917,957)`) confirms bordered numeric cells by eye. An ASCII dump
of the actual binarized mask for the first examined component (own label
only, top-left corner) showed **no digit ink anywhere in that component** —
top 3 rows fully inked (the border), then a persistent 3px left-edge strip
below (the border's left side) — proving the digits were never part of the
stripped component and the erase itself removes zero glyph ink. So the
erase act is (a)'s literal claim — real rule, no ink removed — but that is
not where the damage comes from.

### 1.2 The damage — a debris sliver survives both furniture filters

Erasing a component's border ink leaves behind whatever ink the component
still has. **Observed**, the post-erase remnant of that first component:

```
post-erase piece bbox=(491,27)-(494,78) w=3 height=51 area=152 debris_floor=65.274 dropped=false
```

A 3px-wide, 51px-tall sliver — the border's left edge, now that the top rule
is gone. Two independent filters exist to catch exactly this kind of
strip byproduct and neither fires:

- `lines.furniture_fraction = 0.2`. On this 1653px-wide page the width gate
  is `1653 x 0.2 = 330.6px`; the sliver is 3px wide, nowhere close.
- `lines.debris_heights = 5.0211`, `debris_floor = debris_heights x h =
  5.0211 x 13 = 65.274px`. The sliver is 51px tall, **under** the floor —
  `dropped=false`. It survives as an ordinary component.

Both filters are width/height gates built for a *wide* rule remnant or a
*very tall* one; a border's narrow side, at 51px, is invisible to both. This
same sliver (same 3x51, same `dropped=false`) appears identically at every
one of the 18 boxes on the page, and the identical shape (3x51,
`debris_floor=70.295`) reappears on `filing__r000044` at every box there too
— this is not page-specific, it is the shape every bordered numeric cell's
strip leaves behind.

### 1.3 The sliver corrupts tallest-first line grouping

`lines.rs`'s line-grouping seeds bands tallest-component-first (module doc
comment, `group_with_bands`), using a running median height per band. A
51px-tall sliver — roughly 4x the page's 13px median glyph height — gets
processed as an early, tall seed, exactly like the box's own digit text
would if it were still 51px tall. **Observed**, comparing `[grouping]` line
dumps (control vs. `underline_strip=1`, same page, same code otherwise):

```
control:  line x=54..422 y=102..127 members=33 x_height=13.00 baseline=119.0
control:  line x=53..402 y=124..146 members=29 x_height=13.00 baseline=141.0
stripped: line x=53..422 y=102..146 members=62 x_height=13.00 baseline=119.0
```

Two separate real prose lines (33 + 29 = 62 members) become one merged line
of 62 members — the member count is exact, so this is not approximate, it is
the same components pooled into one band. Total line count for the page:
**89 lines (control) -> 69 lines (stripped)**, 20 lines lost to merges of
this kind. The box's own line is corrupted the same way, absorbing the
sliver as an extra member and inflating its reported metrics:

```
control:  line x=498..613 y=107..122 members=10 x_height=11.15 baseline=122.0 median_height=15
stripped: line x=491..613 y=101..152 members=11 x_height=15.00 baseline=122.0 median_height=15
```

`x0` moves from 498 to 491 (the sliver's `x0`), `y0..y1` widens from
107..122 (15px) to 101..152 (51px, exactly the sliver's own bbox), member
count 10 -> 11, and `x_height` is pulled from a correct 11.15 to a wrong
15.00. Once two prose lines are fused, decode runs the merged text through
the wrong baseline/x-height and the segmentation lattice built over it,
which is the broad, non-rule-shaped letter deletions the gate's readings
already showed (`e`, `n`, `a`, spaces lost) — deletions of prose that was
never touched by the erase itself, only by the grouping it corrupted
downstream.

**`filing__r000044` — same mechanism, confirmed independently:**

```
post-erase piece bbox=(491,46)-(494,97) w=3 height=51 area=152 debris_floor=70.295 dropped=false
control:  line x=54..422 y=121..146 members=33 x_height=13.00 baseline=138.0
stripped: line x=53..422 y=121..165 members=62 x_height=13.00 baseline=138.0
control:  line x=498..613 y=... members=10 x_height=11.15
stripped: line x=491..613 y=120..171 members=11 x_height=15.00
```

Line count: 91 (control) -> 72 (stripped), 19 lost. Identical shape, identical
merge pattern, different page.

### 1.4 Named mechanism, with numbers

> The strip correctly erases 18 top-border bands per page (`w=426`,
> `erased=423/426` columns, `L=70.472`, no digit ink touched — confirmed by
> mask ASCII dump). Each erase leaves a `3x51px` sliver
> (`bbox=(491,27)-(494,78)`, `area=152`) that is **too narrow** for
> `furniture_fraction`'s 330.6px width gate and **too short** for
> `debris_heights`'s 65.274px height floor (`51 < 65.274`), so it survives
> as an ordinary glyphish component. At 51px it is ~4x the page's 13px
> median glyph height, so tallest-first line grouping seeds a band on it
> early and swallows two adjacent real prose lines into one
> (`33 + 29 -> 62` members, exact), corrupting that line's x-height from
> 11.15 to a wrong 15.00. 20 of 89 lines on r000055 and 19 of 91 lines on
> r000044 are lost to merges of this shape. This is why the confusions the
> gate saw were plain letter/space deletions rather than rule-shaped
> confusions: the damage is in line grouping, not in the erase.

Answer: **(c)**, something else — a furniture/debris-filter gap in the
strip byproduct, not (a) direct ink loss (ruled out by the mask dump) and
not (b) a wrongly-small `h` or bold-run false positive (the accepted bands
are genuine rule bands at the correct `h`; the damage is downstream of the
erase, not in the band-acceptance test itself).

Supporting crop (raw grayscale, 3x upscaled, shows the bordered numeric
cell the first analysed component came from): scratchpad
`r055_a.png` (`bbox (491,27)-(917,78)` + padding) — box border containing
`"0.00000000"`.

---

## Task 2 — touching-glyph cut miss on r000022

Target: `filing__r000022`, `--layout` line 27: `x 263..360, 10 members,
x-height 10.00 (Observed), cap 13.00, baseline 530.0`, `want "2,350$"`. The
same atom recurs at line 78 (baseline 741.0), byte-identical, confirming
determinism.

### 2.1 The fused atom

Atom sequence for this word (`interior_cuts`, per-atom): `...300..308 ("5",
w=8) | 309..320 ("0$", w=11, NARROW) | 321..330 (w=9)...`. Truth is
`"2,350$"` — `"0"` and `"$"` are adjacent and land in a **single** component,
one atom 11px wide.

**Observed**, no cut was offered, and no valley search ever ran:

```
atom x=309..320 width=11 line.x_height=10.000 split_min_x_heights=1.15 -> threshold=11.500 (NARROW: no split search)
```

`segment.split_min_x_heights = 1.15`, so the width floor is
`x_height(10.0) x 1.15 = 11.5px`. The atom is `11px` wide — **0.5px under
the gate**. `interior_cuts` returns before computing a column profile at
all, so production code never gets a chance to look.

### 2.2 What the profile shows (computed diagnostically, gate bypassed for this atom only)

```
profile=[1, 3, 14, 13, 3, 1, 3, 3, 9, 7, 1]   (columns x=309..319)
mean=5.273  valley_fraction=0.5  ceiling=2.636
```

Two ink masses separated by a deep pinch: columns 309-312 rise to a stroke
(`14, 13` at x=311-312 — the left stroke of `"0"`), fall to `3, 1` at
x=313-314, then rise again to `9, 7` at x=317-318 (`"$"`'s stroke/curl)
before tailing off. **The valley floor is at x=314, value 1** — 19% of the
mean and comfortably under the 2.636 ceiling.

### 2.3 What would have happened had the width gate not blocked the search

`segment.min_piece_x_heights = 0.2` -> `margin = x_height(10) x 0.2 = 2px`,
so with `width=11 > 2*margin=4` the margin gate would **not** have blocked a
search either. Valid cut window: `[ax0+margin, ax1-margin] = [311, 318]`.
Column x=314 sits inside that window, and by the same local-minimum rule the
production code already uses (`v <= ceiling && v <= profile[i-1] && v <
profile[i+1]`) — `1 <= 2.636`, `1 <= 3` (x=313), `1 < 3` (x=315) — **x=314
qualifies as a candidate cut**. The algorithm the codebase already has
would have found the correct boundary; it never got the chance because the
atom is 0.5px short of the search-eligibility floor.

### 2.4 True boundary, judged by eye

The profile's deep pinch at x=314 (value 1, background-level ink) is the
clearest signal available and reads as the `"0"`/`"$"` boundary: `"0"`
occupies roughly x=309-313 (one visible stroke, no second full-height
stroke before the pinch — consistent with the curved right side of a `"0"`
contributing little ink at this row range), and `"$"` occupies roughly
x=314-319 (stroke + curl material building to `9` then tailing to `7,1`). A
raw-grayscale ASCII crop of the region (`x=305..322, y=516..534`, ~3
strokes visible with a slimmer gap around the same columns) is consistent
with this reading, though the pipeline's own Sauvola-binarized profile above
is the authoritative source — a hand-thresholded grayscale crop uses a
different threshold than the engine's own binarizer and is supporting
context only, not independent proof.

### 2.5 Drop-fall / contour-valley estimate

Not needed to reach a different answer here: the existing column-projection
valley method, if it ran, already finds x=314 unambiguously (value 1 vs.
neighbours of 3 and 3, well clear of the 2.636 ceiling — this is not a
borderline valley). A drop-fall dropped from the top of the atom in the
x=313-315 range would meet almost no ink resistance at any row in that
column band and fall essentially straight down, landing at the same x=314
(paper estimate, not simulated). Contour-valley tracing (deepest concavity
between the upper and lower ink contours) would likewise locate its pinch
at the same column, since the ink count there is close to the row's
background level. All three techniques converge on the same column; the
gap here is not that column-valley cutting fails on this shape, it is that
`segment.split_min_x_heights`'s 11.5px width floor excludes an 11px atom
that the search would otherwise have solved correctly.

### 2.6 Named mechanism, with numbers

> The `"0$"` atom at `x=309..320` is `11px` wide against a
> `split_min_x_heights(1.15) x x_height(10.0) = 11.5px` search-eligibility
> floor — a miss by exactly `0.5px`. `interior_cuts` returns early and never
> computes a column profile in production. Computed for diagnosis only, the
> profile (`[1,3,14,13,3,1,3,3,9,7,1]`, mean 5.273, ceiling 2.636) has an
> unambiguous valley at `x=314` (value 1) that the codebase's own
> local-minimum rule would have selected as a candidate cut had the width
> gate let the search run, landing inside the valid `[311,318]` margin
> window. This is a width-threshold miss on an otherwise well-separated
> touching pair, not a case that needs a different cutting technique.

---

## Debug code removed

The following temporary, env-gated instrumentation was added to produce
this document and has been removed (or was never left in a merged state)
after it was written, confirmed by `cargo test --workspace`:

- `crates/ocrcer-core/src/layout/lines.rs` — `OCRCER_DEBUG_STRIP`-gated
  `eprintln!`s in `strip_underlines`, `strip_component_bands`, and the
  post-erase re-labelling block, plus an ASCII mask dump.
- `crates/ocrcer-core/src/pipeline.rs` — an `OCRCER_DEBUG_STRIP`-gated
  second, diagnostic-only call to `lines::group_with_bands` inside
  `recognize_lines` that printed each resulting `TextLine`.
- `crates/ocrcer-core/src/layout/segment.rs` — `OCRCER_DEBUG_SEGMENT`
  (plus `OCRCER_DEBUG_SEGMENT_X0`/`_X1` range filters) gated `eprintln!`s
  in `interior_cuts`, including a diagnostic-only profile computation for
  atoms the width gate would otherwise skip.

None of these change any default, `params.tsv` entry, or code path taken
when the env vars are unset; `underline_strip` remains `0` (guess) and
`segment.split_min_x_heights` remains `1.15` (guess), unchanged by this
diagnostic.
