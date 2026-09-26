# Chunk 12c — train ablation, rule applied mechanically; val does not fold

Scope: steps 1-4 of `ARCHITECTURE.md` §11 "Chunk 12c: three targeted
reverts, decided on train by a rule fixed here" (2026-09-25). Measurement
only; no edit to `params.rs` or `model/params.tsv` (an `ocrcer-runtime` task,
separate from this report). All numbers below are **measured**, with the
command and stride that produced them.

Raw `ocr` stdout for every run below is committed at
`docs/measurements/2026-09-25_chunk12c/fitlogs/`.

Worktree `wt-12c` off `master` (`b60348b`), branch `chunk-12c`.
`CARGO_TARGET_DIR=D:/Dev/ExcludedPrivate/ocrcer/target-12c`. Release `ocr`
built via `cargo build --release -p ocrcer-bench --bin ocr`. Model built via:

```
cargo run --release -p ocrcer-build -- write 16,20,24,32,48 21,26,36,56 model/out/ocrcer.ocrw
```

Output: `50095 prototypes, 54 faces, 10 tables, 5.37 MB`; int8/f32 top-1
agreement 99.493%. Every `ocr` invocation below printed a
`** STALE: this binary is older than the model file **` warning. Confirmed
benign, same as `docs/measurements/2026-09-25_12b_regressions.md`'s note:
the release binary was compiled before the model file was written in the
same build sequence, not a real code/schema skew — `ocr.rs`'s check is a
pure `mtime(binary) < mtime(model)` heuristic.

Candidates via `ocr --set` only, `model/params.tsv` untouched:
- **A**: `--set match.top_k=5`
- **B**: `--set words.pitch_tolerance=0.15`
- **C**: `--set segment.valley_fraction=0.5`

## Step 1 — train, stride 2

`ocr model/out/ocrcer.ocrw D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train --stride 2 [--set ...]`,
214 of 427 pages, 526,775 reference chars. The five runs (F, F+A, F+B, F+C,
F+A+B+C) ran concurrently as five separate processes on one 20-core/48 GB
machine.

| config | end-to-end CER | line-matched CER | WER | F1 | wall | ms/page |
|---|---|---|---|---|---|---|
| F (fitted defaults) | 21.441% | 24.029% | 35.336% | 77.747% | 2631.9 s | 12299 |
| F+A (`top_k=5`) | 21.323% | 23.905% | 35.146% | 78.235% | 2837.9 s | 13261 |
| F+B (`pitch_tolerance=0.15`) | 21.407% | 23.905% | 35.492% | 77.905% | 2623.0 s | 12257 |
| F+C (`valley_fraction=0.5`) | 21.593% | 24.224% | 35.821% | 77.262% | 2372.2 s | 11085 |
| F+A+B+C | 21.425% | 23.960% | 35.697% | 77.990% | 2573.6 s | 12026 |

WER/F1 are recorded for information only, per the task; the fold decision
below reads only CER and line-matched CER.

Wall times were measured under five-way concurrent execution and carry
contention noise (confirmed by F+A — top_k=5 alone — showing the slowest
ms/page here, 13261, yet the combined F+A+B+C showing among the fastest,
12026; this is not a claim that adding B and C together made A cheaper,
it is five processes sharing one machine). The step-4 val runs below were
sequential and are the cleaner latency reading.

## Step 2 — keep each revert unless it costs > EPS = 0.02 (CER points) against F on either measure

| revert | dCER vs F | dLM vs F | costs > EPS? | verdict |
|---|---|---|---|---|
| A | −0.118 | −0.124 | no | **keep** |
| B | −0.034 | −0.124 | no | **keep** |
| C | +0.152 | +0.195 | **yes, on both** | **drop** |

A and B each improve on both measures on train. C alone costs more than EPS
on both measures on train — reproducing, at stride 2, the same direction as
the stride-4 diagnosis number in `2026-09-25_12b_regressions.md` (fully-fitted
end-to-end CER 23.092% there vs control 25.385%, i.e. C's own fitted value
was already flagged as part of the regression).

## Step 3 — the combined set must pass the same test against F

The combined set as measured is F+A+B+C (all three reverts together, the
only combined configuration run per the task's fixed run list):

| | dCER vs F | dLM vs F | costs > EPS? |
|---|---|---|---|
| F+A+B+C | −0.016 | −0.069 | **no** |

The combined set passes on the first try. **The drop-C-then-B fallback is
not invoked** — there is no AB-only or A-only train measurement in this
report, because the rule only calls for one if the combined set fails, and
it did not.

This is worth stating plainly rather than smoothing over: **C costs on its
own but not combined with A and B.** `2026-09-25_12b_regressions.md` already
flagged Mechanism C as "not isolated" — `segment.valley_fraction`'s revert
was measured together with `segment.min_piece_x_heights` there, and here A
and B are both active whenever C is tested inside the combined set. Which
one offsets C's cost, and by what mechanism, is not decomposed in this
report; it is exactly the kind of interaction the diagnosis document already
warned was present and unexamined for this parameter.

**Chosen set (per the rule, mechanically applied): `match.top_k=5` +
`words.pitch_tolerance=0.15` + `segment.valley_fraction=0.5`, all three.**

## Step 4 — val, once

`ocr model/out/ocrcer.ocrw D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-val`,
full corpus (103 of 103 pages, 258,129 reference chars, stride 1), F then the
chosen set, run sequentially (nothing else heavy concurrent), per
`tools/fit12b`'s convention.

| config | end-to-end CER | line-matched CER | WER | F1 | wall | ms/page |
|---|---|---|---|---|---|---|
| F | 22.082% | 26.902% | 39.597% | 74.139% | 1134.4 s | 11014 |
| chosen (F+A+B+C) | 21.934% | 26.931% | 39.587% | 75.134% | 1022.4 s | 9926 |

F's val numbers here (CER 22.082 / LM 26.902) match `model/params.tsv`'s
recorded fitted-vector val result exactly, confirming this worktree's build
reproduces the committed baseline.

Delta, chosen vs F: **dCER = −0.148** (improves, no cost) — **dLM =
+0.029** (costs more than EPS = 0.02 on the line-matched measure).

**Fold condition: FAILS.** Step 4's rule is "it folds only if it is no
worse than EPS on either measure." The chosen set is worse than F on
line-matched CER by 0.029, which exceeds 0.02. The ordinary end-to-end CER
improved; the line-matched one did not, by a margin larger than the
pre-registered tolerance. Per the rule this is a plain non-fold, not a
close call to be rounded the other way — 0.029 is measured, not estimated,
and the threshold was fixed before this number was run.

Per the honesty rule against tuning on the held-out set, no alternative
subset (A+B alone, A alone) is tried against val here — val is spent, once,
on the set step 3 chose, and the result is what it is.

Latency, the cleaner (sequential) reading: chosen set ran **faster**
end-to-end than F on val (9926 vs 11014 ms/page, about 10% less), contrary
to the expectation that `top_k=5` alone widens the decoder's candidate list
and costs latency. F+A alone, under concurrent contention on train, did show
the highest ms/page of the five configs (13261) — consistent with that
expectation in isolation — but the combined set's net effect, measured
cleanly on val, is a small speed gain, not a cost. Latency remains a tracked
item per the existing note in `ARCHITECTURE.md` §11; this reading does not
resolve which of the three params drives it and does not need to for this
report's decision.

## What this report does and does not decide

- Steps 1-4 are complete and mechanical. The pre-registered rule was
  followed exactly as written; it produced a real result, and that result is
  a **non-fold**.
- Step 5 (identifier gates, synthetic repros, `cargo test`) and step 6
  (score-once) do not run, because they are gated on a fold that did not
  happen.
- No change is made to `params.rs` or `model/params.tsv` in this worktree.
  Branch `chunk-12c` holds only this report; it is not merged.
- The unresolved question this report surfaces for whoever picks up chunk
  12c next: C individually regresses but nets flat inside the combined set,
  and the combined set's ordinary-CER gain does not survive on line-matched
  CER at val scale. A narrower fold (A+B only, dropping C pre-emptively
  rather than only on train failure) was not measured against val and would
  need its own single val shot under the same never-tune-on-test discipline
  — that is a decision for whoever owns the next round, not something this
  report resolves by re-spending val.

## Re-run under the fixed rule: kept set is A+B (2026-09-25)

Source of the fix: `ARCHITECTURE.md` §11, "Chunk 12c: the rule was misapplied;
the kept set is A+B, re-run under the same rule." **One line on what the
earlier A+B+C val shot decided: nothing.** Step 2 (above) dropped C on train;
the run then carried A+B+C into val anyway, contradicting its own step 2. The
rule's wording is now fixed to read "the combined set" in step 3 as *the
reverts kept in step 2* — here, {A, B} — and this section is that corrected
re-run.

Same worktree, same binary (`target-12c/release/ocr.exe`, unchanged since the
earlier section — no rebuild, no code touched), same model
(`model/out/ocrcer.ocrw`). Raw stdout for every run below is committed at
`docs/measurements/2026-09-25_chunk12c/fitlogs/` (`FAB.log`, `F2.log`,
`val_AB.log`).

### Step 3 (corrected) — F+A+B on train, stride 2, judged against F

F+A+B was not run in step 1, so per the fixed rule it is run now, alone (not
concurrently with anything else), with F re-run immediately after for a clean
(uncontended) wall-time reading:

`ocr model/out/ocrcer.ocrw D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train --stride 2 [--set ...]`,
214 of 427 pages, 526,775 reference chars.

| config | end-to-end CER | line-matched CER | WER | F1 | wall | ms/page |
|---|---|---|---|---|---|---|
| F (re-run, clean) | 21.441% | 24.029% | 35.336% | 77.747% | 2443.4 s | 11418 |
| F+A+B (`top_k=5`, `pitch_tolerance=0.15`) | 21.287% | 23.809% | 35.289% | 78.403% | 2670.8 s | 12480 |

F's re-run CER/WER/F1 are bit-for-bit identical to the original step-1 F row
(21.441 / 24.029 / 35.336 / 77.747) — the engine is deterministic, and this
is a useful cross-check that nothing drifted between the two sessions. The
wall-time reading is now clean (each config ran alone): F+A+B costs about 9%
more latency than F alone (12480 vs 11418 ms/page) when not sharing a machine
with four other processes, in contrast to the noisy five-way-concurrent
readings in step 1 where F+A alone appeared to be the slowest of the five.

| revert set | dCER vs F | dLM vs F | costs > EPS (0.02)? | verdict |
|---|---|---|---|---|
| F+A+B | −0.154 | −0.220 | no | **passes** |

F+A+B improves on both measures on train, comfortably inside EPS. Per the
fixed rule, the drop-B fallback is not invoked — there is no train
measurement of A-alone in this section, because it is only called for if
A+B fails, and it did not.

**Kept set going to val: {A, B}** — `match.top_k=5`, `words.pitch_tolerance=0.15`.
C (`valley_fraction`) is not part of the kept set, unchanged from the original
step 2 verdict.

### Step 4 (corrected) — val, once, kept set only

`ocr model/out/ocrcer.ocrw D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-val`,
full corpus (103 of 103 pages, 258,129 reference chars, stride 1). F's val
numbers are **reused** from the original step 4 above (22.082% / 26.902%,
1134.4 s, 11014 ms/page) — not re-run — per the task's explicit instruction
and consistent with never re-spending a held-out set beyond what's needed.

| config | end-to-end CER | line-matched CER | WER | F1 | wall | ms/page |
|---|---|---|---|---|---|---|
| F (reused from step 4 above) | 22.082% | 26.902% | 39.597% | 74.139% | 1134.4 s | 11014 |
| A+B (kept set) | 21.762% | 26.767% | 39.075% | 75.631% | 1298.5 s | 12607 |

Delta, A+B vs F: **dCER = −0.320** (improves) — **dLM = −0.135** (improves).
Both measures improve and both are well inside EPS = 0.02.

**Fold condition: PASSES.** Unlike the original (misapplied) A+B+C val
reading, which improved on end-to-end CER but cost +0.029 on line-matched CER
(over EPS), the correctly-scoped A+B set improves on *both* measures at val.
This is the first val result for chunk 12c that the fixed rule actually
licenses — the earlier A+B+C number above decided nothing, and this one
decides the fold passes.

### What this section does and does not decide

- Steps 3 and 4 (corrected) are complete. The kept set {A, B} —
  `match.top_k=5`, `words.pitch_tolerance=0.15` — folds under the
  pre-registered EPS=0.02 rule on both end-to-end and line-matched CER, at
  both train and val scale.
- **No edit was made to `params.rs` or `model/params.tsv` in this worktree.**
  Applying the fold (steps 5 and 6 of §11: identifier gates, the three
  synthetic repros, `cargo test`, then the one-shot finfilings/pages-cov
  score) is out of scope for this measurement task and belongs to whoever
  owns turning this pass/fail result into a parameter change —
  `ocrcer-runtime` per the agent roster, gated on this report.
- `segment.valley_fraction` (C) stays at its current value; nothing here
  revisits that. The M→IV1 mechanism ARCHITECTURE.md §11 already flagged
  stays open and unscheduled.
