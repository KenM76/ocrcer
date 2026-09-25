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
parameter of this diagnostic). Single-threaded. Total wall time 6m40s
(`real 6m39.847s`), inside the ~15 minute budget.

Bank: 54 faces, 187 classes, built in 195.0s. 20,038 eval glyphs scored
across 108 (face, size) groups in 204.8s.

**Metric**: `bank.rs`'s standardised, **unweighted** squared L2 — the same
metric `Bank::nearest`/`run_bank` report against. This is *not* the shipped
runtime's weighted matcher (`ocrcer_core::r#match`), which returns one
winner per call and would have to be re-run once per candidate face per
query, multiplying this diagnostic's cost by the face count for no change in
which metric is measured. Caveat carried from `style.rs`'s own doc comment.

## Result: the run stopped at its own mandatory sanity gate

No condition x field-length table exists for this run. Per the task's own
step 8 ("must hold or stop and report"), `run_style` asserts before
producing any table that a one-glyph field classifies identically under LS
and under plain 1-NN (singlet), in both the in-bank and leave-one-face-out
conditions — this has to hold, since a one-glyph field's LS "style score"
is just that glyph's own per-face best distance, so LS and singlet should
be scanning the exact same numbers. The assertion failed on the first
violation encountered, in the in-bank condition, before leave-one-face-out
was ever reached:

```
scored 20038 glyphs across 108 (face, size) groups in 204.8s
ocrcer-build: L=1 sanity check failed (in-bank): class - face 31: singlet Some(120) != LS Some(121)
```

(The process exits with `ExitCode::FAILURE` on this path; the `[exited with
code 0]` seen when piping through `tee` is `tee`'s own exit code, not
`ocrcer-build`'s.)

Truth glyph: hyphen-minus, U+002D, rendered by face index 31. Singlet
(global 1-NN, ties broken to the lower **class** index, matching
`Bank::search`) picked class 120: en dash, U+2013. LS (ties broken to the
lower **face** index, per the task's own step 5) picked class 121: em dash,
U+2014. The two winning distances were exactly equal — two different faces'
best-match prototypes tied at the same standardised squared-L2 distance to
this query — and the two classifiers' deliberately different tie-break
rules (class-first vs. face-first) resolved that tie two different ways.

This is not a coding bug. It was checked directly: `label_field`'s k*
selection and `singlet`'s combine both scan the identical `per_face`
array; at a strict (non-tied) minimum they necessarily agree, because both
are just finding the argmin of the same distance values. A disagreement at
L=1 is only possible when two different faces' best distances are exactly
equal, which is what happened here. The hyphen/en-dash/em-dash family is
exactly the kind of degenerate, near-featureless bar shape most likely to
produce genuine cross-face distance ties once rendered to pixels and
standardised, so this is not expected to be a fluke of this particular
choice of eval sizes — a different pair of held-out sizes would plausibly
hit an equivalent tie on the same glyph family before reaching the table
stage. That is a reasoned expectation, not a further verified fact, and no
second run was made to chase it, per this task's single-run scope.

## Sanity checks, status

- L=1 LS == singlet, both conditions: **failed**, as reported above. Also
  covered by two unit tests in `style.rs` (`l1_ls_equals_singlet_in_both_
  conditions` on a small real bank, and `ls_and_singlet_break_ties_by_
  different_rules`, which hand-builds the exact tie shape seen here and
  documents the two tie-break rules as intentionally different).
- Condition (a) L=1 singlet reproducing `run_bank`'s own figure: not
  reached; the run stopped before the reporting loop.

## Toolchain note, unrelated to this change

`cargo clippy -p ocrcer-build --all-targets -- -D warnings` does not pass
on this worktree. Verified via `git stash` that the same failures (same
files, same "10 previous errors" count) are present on the unmodified
checkout — they are in `ocrcer-core` and pre-existing files inside
`ocrcer-build` (`confusions.rs`, `face/raster.rs`, `lexicon.rs`,
`llm_pack.rs`, `weights.rs`) that this task never touched. `cargo clippy
-p ocrcer-build --all-targets --no-deps -- -D warnings` shows zero
violations in `style.rs` or the `main.rs`/`lib.rs` lines added for this
task. `cargo test -p ocrcer-build` passes in full (lib, `main.rs`, and
`roundtrip.rs` tests, 0 failures).
