# cell_pairing=2 on pages-cov: which pages regress, and why

Diagnosis only. No default changed. `lines.cell_pairing` stays 0.

Follows on from `docs/measurements/2026-09-23_cell_pairing.txt` and
`ARCHITECTURE.md` section 11's "Cell pairing, rule 2" entry, which asked:
list pages-cov per-page CER at control vs rule 2 (slack 2.0), name the top
regressors and the shape they share, before any rule 3.

Config compared throughout: control (`lines.cell_pairing=0`, default) vs
`--set lines.cell_pairing=2 --set lines.cell_wrap_slack=2.0` ("cp2").
Corpus: `bench/pages-cov`, all 625 pages, one page per `.pgm`/`.truth.json`
pair, via `./target/release/ocr.exe model/out/ocrcer.ocrw bench/pages-cov`.

## 0. Tooling used (observed)

`ocr.exe` had no full-precision per-page CER export; `--worst N` rounds to
2 decimals and only sorts one metric at a time. Added a permanent `--csv PATH`
flag to `ocrcer-bench`'s `ocr` binary: writes `stem,cer,line_matched_cer` for
every page in the run, at full `f64` precision (`{:.8}`), regardless of
`--worst`/`--show`. Committed separately as `5983367`
("ocr bench: add --csv for full-precision per-page CER, both metrics").

Two full runs were taken, one at a time: `--csv control.csv` (default) and
`--csv cp2.csv` (`--set lines.cell_pairing=2 --set lines.cell_wrap_slack=2.0`),
both over the full 625-page corpus. Per-page deltas were computed by a small
throwaway script (not committed; scratch, deleted after use).

## 1. Per-page delta ranking (observed)

Of 625 pages, **exactly 37 changed at all** between control and cp2, and
**every one of the 37 is a regression** — no page improved (CER or
line-matched). This is a clean, concentrated signal, not noise spread across
the corpus.

By category: 35 `drawing`-category pages (monospace CAD-style pages, spread
across 7 of the corpus's 8 monospace font families — liberation-mono,
inconsolata, roboto-mono, cascadia-mono, jetbrains-mono, pt-mono, fira-code —
at various sizes), and 2 `invoice`-category pages (open-sans-condensed light,
21px and 40px only).

Top 15 regressors by CER delta (`cp2.cer - control.cer`):

| delta | page | ctrl CER | cp2 CER | ctrl line-matched | cp2 line-matched |
|---|---|---|---|---|---|
| 0.189003 | open-sans-condensed__light__invoice__40px | 0.0619 | 0.2509 | 0.0619 | 0.1031 |
| 0.182131 | open-sans-condensed__light__invoice__21px | 0.1031 | 0.2852 | 0.1031 | 0.1409 |
| 0.100503 | inconsolata__regular__drawing__18px | 0.0201 | 0.1206 | 0.0201 | 0.1156 |
| 0.100503 | liberation-mono__regular__drawing__28px | 0.0201 | 0.1206 | 0.0201 | 0.1156 |
| 0.100503 | pt-mono__regular__drawing__28px | 0.0201 | 0.1206 | 0.0201 | 0.1156 |
| 0.100503 | liberation-mono__regular__drawing__18px | 0.0352 | 0.1357 | 0.0352 | 0.1307 |
| 0.100503 | cascadia-mono__regular__drawing__18px | 0.0251 | 0.1256 | 0.0251 | 0.1206 |
| 0.100503 | cascadia-mono__regular__drawing__28px | 0.0151 | 0.1156 | 0.0151 | 0.1106 |
| 0.100503 | cascadia-mono__regular__drawing__40px | 0.0050 | 0.1055 | 0.0050 | 0.1005 |
| 0.100503 | fira-code__regular__drawing__18px | 0.0050 | 0.1055 | 0.0050 | 0.1005 |
| 0.100503 | fira-code__regular__drawing__21px | 0.0000 | 0.1005 | 0.0000 | 0.0955 |
| 0.100503 | fira-code__regular__drawing__40px | 0.0050 | 0.1055 | 0.0050 | 0.1005 |
| 0.100503 | inconsolata__regular__drawing__21px | 0.0050 | 0.1055 | 0.0050 | 0.1005 |
| 0.100503 | inconsolata__regular__drawing__28px | 0.0101 | 0.1106 | 0.0101 | 0.1055 |
| 0.100503 | inconsolata__regular__drawing__40px | 0.0101 | 0.1106 | 0.0101 | 0.1055 |

Sum of all positive (regressing) per-page deltas across the corpus:
**3.813345**. The top 15 alone sum to **1.677667** — **43.99%** of the total
regression. The near-identical 0.100503 deltas across many mono
fonts/sizes are not a coincidence: the `drawing` template's text and
geometry (a CAD title block) is the same across those renders, so the same
mis-pairing lands on the same substrings, producing near-identical CER
deltas.

## 2. Fragment-level trace: two distinct pages, same shape (observed)

Traced with `--raw` (end-to-end text) and `--layout --no-decode` (per-line
boxes and band-fragment tags), correlated against `.truth.json` glyph
baselines/x-ranges directly (**not** `--layout`'s printed `want "..."`
field — that field is `truth.lines.get(output_line_index)`, a positional
lookup that silently desyncs the moment band-splitting produces a different
number of output lines than truth has lines, which happens on almost every
page here; every `want` string quoted below that looked wrong was cross-checked
this way).

### 2a. `open-sans-condensed__light__invoice__21px` / `__40px` (top 2 regressors)

Truth table body (the invoice's line-item table):
```
Qty  Description                 Unit      Amount
  4  Bracket, 6mm plate        112.50      450.00
 12  Dowel pin 8 x 40 mm         3.75       45.00
  1  Setup and programming     280.00      280.00
```

At 40px, the row "12 Dowel pin 8 x 40 mm / 3.75 / 45.00" is correctly
column-split into 2 band fragments (baseline 386): fragment 1/2 at
x31–310 ("12 Dowel pin 8 x 40 mm 3.75"-ish) and fragment 2/2 at x375–591
(the "45.00" Amount cell). The very next row, "1 Setup and programming /
280.00 / 280.00", did **not** get column-split — its internal gaps never
crossed the column-cut threshold, so it stays one wide, unsplit fragment
(x-range spans nearly the same table width as the row above).

Under cp2, `pair_cells` treats the Amount-column fragment as continuing to
wrap. Downstream, this cascades into the totals block (Subtotal/HST/Total,
baseline 440–441, 3 band fragments in control): the rightmost of those three
fragments (x520–590) also gets pulled out, reordered to print immediately
after the Amount fragment, and swallows the next wide unsplit row (a
totals-line at baseline 495) as a "continuation," deferring its own two
row-mates (the Subtotal and HST-percentage fragments) to print at the very
end of the page. This is the observed `--raw` shattering:
"...Bracket... / 45.00 / 1 Setup... / 12 Dowel pin... / 3.75 / Subtotal... / HST... / Total...".

**The deferred fragment is not a hanging indent, a centred heading, a list
marker, or a mis-segmented word fragment. It is a genuine, correctly-split
right-hand table-column cell** — the "Amount" value — whose own row already
read correctly before cp2 touched it.

### 2b. `inconsolata__regular__drawing__18px` (representative of the 35-page drawing cluster)

Truth's CAD title block ends with two adjacent lines:
```
line 7 (baseline 206): SCALE 1:2   SHEET 1 OF 3
line 8 (baseline 231): DO NOT SCALE DRAWING
```

Line 7 is genuinely two cells separated by a wide gap (a scale note and a
sheet-count note in the same title-block row), correctly column-split by the
engine into 2 band fragments at baseline 207: fragment 1/2 "SCALE 1:2"
(x17–112) and fragment 2/2 "SHEET 1 OF 3" (x147–274). In control these two
fragments are correctly emitted together as one line, followed by
"DO NOT SCALE DRAWING" (x18–232, baseline 232, one wide unsplit fragment —
the page's last line).

Under cp2, verified directly from `--layout` boxes: the "SHEET 1 OF 3"
fragment (x147–274, baseline 207) is pulled out of its row and promoted to
print immediately after "BEND R3.0 TYP" (the previous line), then the wide
unsplit "DO NOT SCALE DRAWING" row is swallowed as its "continuation," and
finally "SCALE 1:2" (x17–112, baseline 207, the fragment's own row-mate) is
deferred to print dead last, after the page's true final line. Observed
`--raw`:
```
control: ...BEND R3.0 TYP / SCALE 1:2 SHEET 1 OF 3 / DO NOT SCALE DRAWING
cp2:     ...BEND R3.0 TYP / SHEET 1 OF 3 / DO NOT SCALE DRAWING / SCALE 1:2
```

Again: the deferred fragment ("SCALE 1:2") is a genuine title-block cell,
not a hanging indent or heading — its row was already correct.

## 3. The shape (inferred from the two traces above, generalized)

Both traces have the identical structural signature:

> A row with 2+ genuinely independent fragments — every cell in the row is
> already a complete value, already correctly grouped with its row-mates —
> sits immediately above a subsequent row that is a single, unsplit, WIDE
> fragment (its internal gaps never crossed the column-cut threshold, so it
> stayed one fragment spanning most of the line's width). Because that wide
> next-row fragment geometrically overlaps almost every column on the page
> (`columns_overlap`'s 0.4 threshold is relative to the *narrower* of the two
> boxes, so a wide row is close to guaranteed to pass against any narrower
> fragment above it), the current row's right-hand cell gets misread as
> "wrapping into" that next row by column-overlap coincidence.

A second contributing mechanism, present in both traces (confirmed by
manual computation against the real box coordinates, using the exact
`column_block_extent`/`wrap_run_len` logic in
`crates/ocrcer-core/src/layout/lines.rs`): **the fullness test degenerates
when `column_block_extent`'s up/down scan finds nothing wider than the
candidate's own right edge.** In that case `extent == candidate.x1`, so
`extent - candidate.x1 == 0`, which trivially satisfies
`extent - prev.x1 <= slack * x_height` regardless of the candidate's actual
width relative to its row. The right-hand cell in question is often exactly
this case — it's the widest thing at its column position on the page, so
nothing in the scan ever proves it "ran out of room," yet the test passes
as if it had. In 2a, the effect compounds instead: the wide neighbouring row
inflates the extent to a value that happens to fall within slack of the
candidate's edge by a narrow margin (a coincidence of the specific pixel
geometry, not a structural guarantee — this half of the mechanism is
page-specific, unlike the vacuous-extent case which is structural).

## 4. Proposed condition for rule 3 (proposal only, not implemented)

Two candidate conditions, targeting the two mechanisms above; either alone
would likely close both traced cases, and they are not mutually exclusive:

1. **Don't let an unsplit row's fragment count toward "matches this column"
   at anywhere near full credit.** In `single_column_match` /
   `column_block_extent`'s up/down scan, additionally require that a row
   being scanned was *itself* produced by a column-split (i.e. it is one
   fragment of a multi-fragment band group), OR — for a single-fragment row
   — require its own overlap fraction against the anchor's column to be much
   stricter than 0.4 (e.g. the anchor's box must sit *inside* a plausible
   single-cell slice of the wide row, not merely overlap it by 40% of the
   narrower width). A single wide fragment spanning most of the page width is
   not evidence of "this is the same column," it's evidence of "this row
   wasn't split," which is exactly the case that should be excluded.

2. **Close the vacuous-extent case.** In `column_block_extent`, if the
   up/down scan never finds any row whose matched x1 exceeds the anchor's
   own x1 (i.e. `extent == anchor.x1` after the scan), do not treat the
   fullness test as passed by default — treat it as *undecidable* and fail
   closed (reject the candidate as a wrap-continuation) rather than
   succeeding on a zero-margin technicality. This directly blocks case 2b's
   "SCALE 1:2" / "SHEET 1 OF 3" mechanism, since that candidate's extent was
   exactly its own x1.

Both conditions are stricter than the current rule 2 and would need to be
re-checked against `r000044` (the real filing rule 2 was written to fix) to
confirm they don't reintroduce that regression — that check has not been
done here; this diagnosis stops at proposing the condition, per the task
scope.

## 5. Status

`lines.cell_pairing` stays at its default of 0. No code changed by this
diagnosis. Next: `ocrcer-runtime` or `ocrcer-architect` implements one or
both of the above as "rule 3," re-measures pages-cov (expect the 37-page
regression to close or shrink) and finfilings (expect `r000044`'s win to
hold), before any change to the default is considered.
