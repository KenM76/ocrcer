# Chunk 12b fitted-vector regressions — diagnosis only

Scope: diagnose three failures reported against `ARCHITECTURE.md` §11
"Chunk 12b closed" (2026-09-25) and `docs/measurements/2026-09-25_score_12b.md`
§3. No parameter or code change is made or decided here. Worktree
`wt-diag12` off master, branch `diag-12b-regress`, model rebuilt locally
(`model/out/ocrcer.ocrw`, gitignored, not committed).

Firewall respected: no run against or image read from pages-cov, finfilings,
finfilings-val, or fixtures. Synthetic reproduction used fresh renders via
`ocrcer-build pages` (same renderer pages-cov uses, called directly rather
than duplicated — `CLAUDE.md` rule 4) into
`D:/Dev/ExcludedPrivate/ocrcer/diag12_pages` and a curated 11-page subset
matching the faces/sizes named in the task. Real-scan counts are measured on
`finfilings-train` only, per the task's explicit exception.

All 11 fitted params were reverted only via `ocr --set name=value` at
process invocation; `model/params.tsv` and `Params::DEFAULT` are unmodified
in this worktree.

Fitted vs pre-fold (control) values for the 11 params folded in 12b:

| param | fitted | control |
|---|---|---|
| `words.pitch_tolerance` | 0.22 | 0.15 |
| `segment.max_merge_x_heights` | 1.05 | 1.8 |
| `segment.valley_fraction` | 0.65 | 0.5 |
| `segment.min_piece_x_heights` | 0.28 | 0.2 |
| `layout.slant_margin` | 1.08 | 1.15 |
| `match.top_k` | 3 | 5 |
| `decode.w_seg` | 0.55 | 0.25 |
| `decode.seg_ideal_aspect` | 0.45 | 0.6 |
| `decode.seg_aspect_tolerance` | 0.35 | 0.55 |
| `decode.seg_merge_penalty` | 0.7 | 0.5 |
| `decode.beam_width` | 14 | 24 |

Real-scan numbers below are **measured on a stride-4 sample of
finfilings-train (107/427 pages, ~270k reference chars)**, not the full
corpus — a full-corpus run did not finish in a reasonable window (see
"Incidental finding" below) and stride-4 is stated explicitly rather than
presented as a full-corpus figure.

---

## Mechanism A — monospace "i" read as "í"/"î"/"ñ"

**Responsible param (measured): `match.top_k`.** Reverting this one param
alone (fitted values otherwise unchanged) fully restores correct "i" on all
three monospace synthetic pages (`cascadia-mono`, `pt-mono`, `roboto-mono` ×
`prose` × 14/40px), verified by `--raw` diff.

**Stage (measured): not segmentation — matcher/decoder candidate-list
truncation.** `--distances` (oracle-boxed, matcher-only, bypasses `top_k`
and the decoder entirely) shows "i"→"î" is a **pre-existing** raw
weighted-L2 top-1 confusion at this face/size, unchanged by `top_k` (13
events on `pt-mono@14px` under both 3 and 5). The mechanism: for these
faces the correct "i" class is not always the matcher's #1 raw candidate,
but it is normally within the top 5 the decoder sees, and the bigram+lexicon
prior in `decode/viterbi.rs` picks it from there. Chunk 12b's `top_k`
3→5→3 fold shrinks that candidate list, so the correct class more often
never reaches the decoder to be rescued.

**Train-split frequency (measured, stride-4):**

| config | "i"→"î" | "i"→"í" | "i"→"ñ" |
|---|---|---|---|
| fitted (all 11) | 25 | 24 | 15 |
| control (all 11 reverted) | 0 | 0 | 0 |
| `match.top_k=5` only | 0 | 22 | 0 |

`top_k` alone removes the majority (64→22, ~66%) but not all of it on real
scans — unlike the synthetic subset, where it was fully sufficient. The
residual 22 "í" events are not decomposed to a further param within this
diagnosis's budget; inferred (not measured) that they come from interaction
with one or more of the other 10 fitted params, since only the full
11-param revert reaches 0.

End-to-end (stride-4): fitted CER 23.092%/WER 37.224%/F1 76.748%; full
control CER 25.385%/WER 47.274%/F1 66.508%; `top_k=5`-only CER
22.961%/WER 37.009%/F1 77.284% (flat-to-slightly-better than fully fitted —
reverting this one param does not cost the fold's overall real-scan gain).

---

## Mechanism B — short-token word fusion ("DO NOT SCALE DRAWING" →
"DONOTSCALEDRAWING", "1 OF 3" → "10F3")

**Responsible param (measured): `words.pitch_tolerance`.** Reverting this
one param alone fully restores correct word splitting on both synthetic
repros (`liberation-serif@40px` "DO NOT SCALE DRAWING", `roboto-condensed@28px`
"SCALE 1:2 SHEET 1 OF 3"), verified by `--raw` and `--layout`.

**Stage (measured): `layout/words.rs`, the fixed-pitch/monospace detection
gate in `pitch_estimate()`.** `--layout` shows the word-gap list for
"SCALE 1:2   SHEET 1 OF 3" (`[16,15,16,13,19,11,10,34,16,16,14,14,19,23,16,19.5]`,
genuinely proportional) is misclassified under fitted:
`threshold 24 (FixedPitch, eta 0.000) -> 2 words` → `["SCALE1:2",
"SHEET10F3"]`; under control it correctly falls to
`threshold 5 (Fallback, eta 0.000) -> 6 words` → `["SCALE","1:2","SHEET",
"1","OF","3"]`. Same pattern on the serif line: fitted
`threshold 42 (FixedPitch) -> 1 words`; control `threshold 3 (Valley,
eta 0.965) -> 4 words`. Raising `pitch_tolerance` (0.15→0.22) widens how far
a gap may deviate from an integer multiple of the median pitch and still
"agree" with it, so a proportional line with enough short tokens gets
misread as fixed-pitch and collapsed to one gap-threshold pass.

**Train-split frequency (measured, stride-4), proxy = `" " -> ""`
(deleted-space, i.e. fused-word) alignment events:**

| config | `" " -> ""` | `"" -> " "` |
|---|---|---|
| fitted (all 11) | 1294 | 2999 |
| control (all 11 reverted) | 1132 | 2582 |
| `words.pitch_tolerance=0.15` only | 1123 | 3017 |
| `match.top_k=5` only | 1300 | 2988 |
| `segment.valley_fraction=0.5`+`min_piece_x_heights=0.2` only | 1292 | 2969 |

`pitch_tolerance` alone reproduces (and slightly overshoots) the full
control's reduction in the deleted-space count (1123 vs control's 1132,
fitted's 1294) while the other two single-param reverts leave it
essentially at the fitted level — confirms `pitch_tolerance` as the real-scan
driver for this proxy, consistent with the synthetic isolation.

`"" -> " "` (spurious space *insertion*, over-splitting) does **not** move
consistently with `pitch_tolerance` — it is a different error type, not part
of Mechanism B; not decomposed further here.

End-to-end (stride-4), `pitch_tolerance=0.15`-only: CER 23.037%/WER
37.119%/F1 77.046% — flat vs fully fitted (23.092%/37.224%/76.748%).

---

## Mechanism C — "M8x1.25" → "IV18x1.25" (M split into I, V, 1)

**Responsible param (measured, primary): `segment.valley_fraction`.**
Reverting this one param alone fully restores the correct atom on
`noto-sans@18px` "4X M8x1.25 THRU" (decodes "M8x1", not "IV18x1").
**Secondary/interacting (measured): `segment.min_piece_x_heights`** —
reverting it alone does *not* fix the token, it changes the corruption to
"l\/18x1" (still wrong, different shape), so it modulates but does not
independently cause the failure.

**Stage (measured): `layout/segment.rs::interior_cuts()`, not word
splitting.** `--layout` shows the word-split boundaries for the line are
byte-identical fitted vs control (`gaps [1,6,2,2,3,4,3,2,6,2,3,2]`,
`threshold 3 (Valley, eta 0.778) -> 4 words` in both) — the corruption is
entirely inside how the "M8x1"-word's own segmentation lattice is built.
Source: `interior_cuts()` computes `ceiling = mean * valley_fraction` as the
column-ink-density threshold below which a column becomes a candidate
interior split. Raising `valley_fraction` 0.5→0.65 raises that ceiling, so
more of the "M" glyph's own internal density dip (from its diagonal
strokes) qualifies as a valley/split candidate that was never proposed
under the lower control ceiling — the atom for "M" gets cut into pieces the
matcher then reads as I/V/1.

**Train-split frequency: not measured.** Unlike Mechanisms A/B, an
identifier-shaped multi-character corruption like M→I,V,1 does not reduce
to one aligned single-character confusion-table row, so the character-level
alignment proxy used for A/B does not isolate it. A `--worst 300` run
isolating `segment.valley_fraction` alone was not completed within this
diagnosis's time budget (each finfilings-train stride-4 pass took
15–25 minutes; five configurations were already run for A/B). This is a
genuine scope gap, stated rather than filled with an inferred number.
`fin_valley05.log` (default `--worst 12`, both `valley_fraction` and
`min_piece_x_heights` reverted together) end-to-end: CER 23.286%/WER
37.912%/F1 76.021% — close to fully-fitted, consistent with this mechanism
being comparatively rare on finfilings-train's mostly non-identifier prose,
but that is inferred from the aggregate delta, not a direct count.

---

## Incidental finding: real per-page latency

Measured (not projected) on finfilings-train during these runs: ~10.7–14.2
seconds/page single-threaded, well above `ARCHITECTURE.md` §4.1's "well
under a second per page" projection. Surfaced only because a full 427-page
run did not complete in ~67 minutes wall-clock across 5 concurrent
processes (confirmed still actively computing via climbing CPU time, not
hung, before being killed to switch to stride-4 sampling). Noted per
`CLAUDE.md` rule 8 ("report what was measured"); not investigated further —
out of scope for this diagnosis.

## Other notes

- No new probe binary was written: `ocrcer-build pages <out> <px> [--local]`
  already renders every face × corpus block × size combination needed for
  synthetic reproduction, so reusing it avoided a second implementation of
  the renderer (`CLAUDE.md` rule 4).
- `ocr`'s provenance line printed `** STALE **` on some runs in this
  worktree; read `crates/ocrcer-bench/src/provenance.rs` and confirmed it is
  a pure `mtime(binary) < mtime(model)` heuristic — benign here, since the
  binary was compiled before the model file was written in the same build
  sequence, not a real code/schema version skew.
- `--worst N` controls both the worst-pages list and the confusion-table
  length (`ocr.rs`, `top.into_iter().take(worst)`); the default (`12`) is
  too short to surface the specific confusions this diagnosis needed —
  `--worst 300` was used throughout.

## Candidate fixes (unscheduled, not decided here)

- Mechanism A: raise `match.top_k` back toward 5, or make it
  face/pitch-aware, while keeping the other 10 fitted values (the
  `top_k=5`-only stride-4 numbers above suggest this is close to free on
  CER/WER/F1).
- Mechanism B: raise `words.pitch_tolerance`'s rejection strictness, or add
  a minimum-token-count / minimum-line-width guard to the fixed-pitch
  branch of `pitch_estimate()` so a short all-caps drawing line doesn't
  qualify as monospace on gap-agreement alone.
- Mechanism C: lower `segment.valley_fraction` back toward 0.5, or gate
  `interior_cuts()`'s candidate acceptance on stroke-width/x-height context
  so a single glyph's internal density dip is harder to mistake for an
  inter-glyph valley.

None of these are scheduled or approved; this document is diagnosis only,
per the task.
