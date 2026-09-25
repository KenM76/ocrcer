# Style-fields probe: Sarkar & Nagy label-style (LS) vs. plain 1-NN

Diagnostic only. Nothing here feeds `ocrcer-core`, `run_bank`, or any emitted
table. All numbers below are from synthetic rendered glyphs at held-out
sizes, not real pages.

## Command

```
ocrcer-build style 16,20,24,32,48 18,40 1,2,4,8
```

Build ladder 16, 20, 24, 32, 48 px/em (the shipped five-size ladder). Eval
sizes 18 and 40 px/em, both held out of that ladder. Field lengths 1, 2, 4,
8. Gate: `Holes` (the shipped runtime's only enabled pruning step; not a CLI
parameter of this diagnostic). Single-threaded (the binary has no threading
dependency; nothing to disable).

Run split by held-out size (`... 18 1,2,4,8` and, separately, `... 40
1,2,4,8`) to stay inside a single foreground call. Bank build: 54 faces, 187
classes, ~193-206s per run (rebuilt once per invocation, not shared). Held-out
18: 10,019 eval glyphs scored across 54 (face, size) groups in 77.1s, total
wall time 4m30.7s. Held-out 40: 10,019 eval glyphs scored in 132.9s, total
wall time 5m39.2s. Combined: 10m9.9s across the two foreground calls.

**Metric**: `bank.rs`'s standardised, **unweighted** squared L2 — the same
metric `Bank::nearest`/`run_bank` report against. This is *not* the shipped
runtime's weighted matcher (`ocrcer_core::r#match`), which returns one
winner per call and would have to be re-run once per candidate face per
query, multiplying this diagnostic's cost by the face count for no change in
which metric is measured. Caveat carried from `style.rs`'s own doc comment.

## Tie finding and the fix (2026-09-25 follow-up)

The first run of this probe (13fcca9) stopped at its own L=1 sanity gate: on
an exact cross-face distance tie (hyphen, face 31), singlet chose class 120
(en dash) and LS chose class 121 (em dash), because the two classifiers used
different tie-break rules (singlet: lowest class index, matching
`Bank::search`; LS, as first written: lowest face index). Ruling: the gate
was right to fire; the defect was the spec giving LS its own rule instead of
`ARCHITECTURE.md` §8.2's "exact tie breaks by lowest class index".

Fix: `label_field` now breaks an exact sum tie between candidate faces by
the lexicographically smallest per-glyph label vector (class indices, in
field order), and only falls back to the lower face index if the label
vectors are also equal. At L=1 a label vector is one class index, so this
reduces to exactly singlet's rule by construction. Both runs below pass the
L=1 gate in both conditions.

## Result

`glyphs` is the eval-glyph count contributing to that row (drops slightly at
higher L because a field must divide the group evenly). `rel_change%` is
`(ls_err - singlet_err) / singlet_err`, so positive means LS is worse.
`(folded)` columns fold case (`c`/`C`, etc.) before scoring.

### Held-out size 18

| condition | L | glyphs | singlet_err% | ls_err% | rel_change% | singlet_err%(folded) | ls_err%(folded) |
|---|---|---|---|---|---|---|---|
| in-bank | 1 | 10,019 | 14.89 | 14.89 | 0.00 | 11.88 | 11.88 |
| in-bank | 2 | 10,006 | 14.91 | 15.80 | 5.97 | 11.89 | 12.99 |
| in-bank | 4 | 9,916 | 15.04 | 16.16 | 7.44 | 11.99 | 13.41 |
| in-bank | 8 | 9,904 | 15.01 | 15.41 | 2.62 | 11.99 | 12.63 |
| leave-one-face-out | 1 | 10,019 | 17.29 | 17.29 | 0.00 | 13.83 | 13.83 |
| leave-one-face-out | 2 | 10,006 | 17.31 | 19.48 | 12.53 | 13.85 | 16.03 |
| leave-one-face-out | 4 | 9,916 | 17.46 | 21.27 | 21.84 | 13.97 | 17.73 |
| leave-one-face-out | 8 | 9,904 | 17.45 | 22.45 | 28.65 | 13.96 | 18.48 |

`k*` (LS's chosen face) equals the true rendering face: L=1 30.58%
(3064/10019), L=2 48.09% (2406/5003), L=4 65.11% (1614/2479), L=8 80.37%
(995/1238) — in-bank condition only; leave-one-out has no true face to match
against by construction.

### Held-out size 40

| condition | L | glyphs | singlet_err% | ls_err% | rel_change% | singlet_err%(folded) | ls_err%(folded) |
|---|---|---|---|---|---|---|---|
| in-bank | 1 | 10,019 | 5.08 | 5.08 | 0.00 | 4.41 | 4.41 |
| in-bank | 2 | 10,006 | 5.09 | 5.78 | 13.56 | 4.42 | 5.25 |
| in-bank | 4 | 9,916 | 5.13 | 5.72 | 11.39 | 4.46 | 5.23 |
| in-bank | 8 | 9,904 | 5.14 | 5.70 | 11.00 | 4.46 | 5.32 |
| leave-one-face-out | 1 | 10,019 | 10.15 | 10.15 | 0.00 | 7.92 | 7.92 |
| leave-one-face-out | 2 | 10,006 | 10.16 | 12.29 | 20.94 | 7.94 | 9.88 |
| leave-one-face-out | 4 | 9,916 | 10.26 | 14.24 | 38.84 | 8.01 | 11.39 |
| leave-one-face-out | 8 | 9,904 | 10.27 | 14.55 | 41.69 | 8.02 | 11.63 |

`k*` equals the true rendering face (in-bank only): L=1 64.83% (6495/10019),
L=2 82.59% (4132/5003), L=4 94.47% (2342/2479), L=8 98.87% (1224/1238).

### Highlighted cell: leave-one-face-out, L=4

| held-out size | singlet_err% | ls_err% | rel_change% |
|---|---|---|---|
| 18 | 17.46 | 21.27 | +21.84% |
| 40 | 10.26 | 14.24 | +38.84% |

LS is worse than singlet at every field length in the leave-one-face-out
condition, at both held-out sizes, and the relative gap widens with L (12.5%
to 28.7% at held-out 18; 20.9% to 41.7% at held-out 40) rather than
narrowing. In-bank, LS is also worse than singlet at every L>1, by a smaller
margin (2.6%-7.4% at held-out 18; 11.0%-13.6% at held-out 40).

Top confusions LS fixes vs. introduces at L=4 (leave-one-out) are dominated
by case pairs (`S`/`s`, `X`/`x`, `C`/`c`, `Z`/`z`, `O`/`o`/`0`, `W`/`w`) and
the dash/dot family (`–`/`—`/`-`, `•`/`·`) in both held-out conditions —
consistent with the near-featureless-glyph tie risk noted in the original
gate finding, now resolved deterministically rather than left to fire the
gate.

## Sanity checks, status

- L=1 LS == singlet, both conditions, both held-out sizes: **passed**.
  Covered by `l1_ls_equals_singlet_in_both_conditions` (small real bank) and
  by two hand-built adversarial-tie unit tests in `style.rs`:
  `ls_and_singlet_agree_on_an_exact_cross_face_tie` (the L=1 shape from the
  original gate failure) and
  `l2_cross_face_sum_tie_is_broken_by_the_label_vector_not_the_face_index`
  (an L=2 sum tie where the lexicographic label rule picks the higher-index
  face over the lower one, showing the rule is label-vector-first, not
  face-index-first).

## Toolchain note, unrelated to this change

`cargo clippy -p ocrcer-build --all-targets -- -D warnings` does not pass on
this worktree. Verified via `git stash` that the same failures (same files,
same "10 previous errors" count) are present on the unmodified checkout —
they are in `ocrcer-core` and pre-existing files inside `ocrcer-build`
(`confusions.rs`, `face/raster.rs`, `lexicon.rs`, `llm_pack.rs`,
`weights.rs`) that this task never touched. `cargo clippy -p ocrcer-build
--no-deps` (default targets) shows zero violations in `style.rs` or its
tests. `cargo test -p ocrcer-build` passes in full (lib, `main.rs`, and
`roundtrip.rs` tests, 0 failures), including the two new/changed `style.rs`
tests above.

## Verdict (architect, 2026-09-25): parked

The decision rule was set before the run: a relative glyph-error cut of at
least 10% at leave-one-face-out L=4 writes a candidate, 5–10% parks it as
weak, and anything under 5% parks it. The measured change is +21.8% (held-out
18) and +38.8% (held-out 40), where positive means LS makes more errors. Style-consistent
field classification is parked. No candidate spec, and no §11 entry, since
nothing was ever proposed there.

What the reading says, beyond the rule. In-bank, LS names the true face for
94.5% of L=4 fields and is still worse than singlet at every L>1. So a
correctly identified face's own prototypes at the nearest ladder size label
glyphs worse than the whole bank does. Other faces' prototypes of the same
class fill in for the size the ladder lacks. The bank gains more from its
spread of faces than a page gains from staying consistent within one face.
That bears on the per-page adaptive prototype candidate
(`2026-09-22_research_classical_techniques.md`, 2026-09-24 addendum) in one
direction only: that candidate *adds* page-specific prototypes to the bank,
while LS *restricts* the bank to one face. This result is evidence against
restricting, not against adding.

Caveat: the metric is `bank.rs`'s unweighted L2, not the runtime's weighted
matcher, and the glyphs are synthetic renders at held-out sizes. Reopen only
if a weighted-metric reading or real-page evidence contradicts it.
