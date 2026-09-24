# Dense-page matching speed: a second, cross-class early-abandon ceiling

Alpha blocker for pdfcer integration (`ARCHITECTURE.md` section 11): dense
real pages (finfilings, 60 filing scans) ran far slower than the pages-cov
benchmark synthetic pages — measured before this chunk at roughly 1038 s
total / 17 s per page, against roughly 0.8 s/page on pages-cov. This chunk
profiles the gap, fixes the dominant cost with a change proven exact, and
verifies output is byte-identical to the unoptimised matcher on both scoring
corpora.

Tool: `crates/ocrcer-bench/src/bin/profile_page.rs`, reading the stage
counters in `crates/ocrcer-core/src/prof.rs` (`OCRCER_PROFILE=1`, gated to
non-wasm; a no-op module on `wasm32-unknown-unknown`, see that file's
contract comment).

## Profile: three dense finfilings pages, unoptimised matcher

`model/out/ocrcer.ocrw`: 187 classes, 54 faces, **50,095 prototypes** — the
bank has grown well past `ARCHITECTURE.md` section 4.1's original
~12,000-prototype planning figure as later chunks added faces and augmented
variants; that figure is now stale and is flagged to `ocrcer-architect`
separately, since it is what made the per-class-only ceiling below
insufficient at the bank's current size.

| Page | words | edges | wall (ms) | match() share | nearest() calls | prototypes visited/call | early-abandon rate |
|---|---|---|---|---|---|---|---|
| filing__r000011 (overprinted) | 327 | 3,977 | 14,387 | 96.3% | 3,975 | 23,100 | ~58–64%* |
| filing__r000605 | 980 | 9,830 | 38,672 | 97.7% | 9,830 | 26,266 | ~58–64%* |
| filing__r000462 | 269 | 2,400 | 8,871 | 95.8% | 2,400 | 23,988 | ~58–64%* |

\* Early-abandon rate as first measured, before the prototype counter was
re-run against the change below (which recomputes the same visited count but
a much higher abandoned count — see the after-table).

Binarize/layout, segment, extract and decode are each under ~3% of wall
time on every page measured; matching is the entire problem. Segmentation
(edges/word 8.9–12.2) is not unusually high for these pages — the cost is in
how much of a 50,095-prototype bank a single `nearest()` call visits, not in
how many times it is called.

## The bottleneck, once visited counts are in hand

The hole-count gate (section 4.1) narrows the *class* set, but with 187
classes and roughly 268 prototypes per class on average, `nearest()`'s only
other pruning was a per-class ceiling (`best_d[class]`) that starts at
infinity and only tightens as that one class's own prototypes are scanned.
With `top_k = 5` (the shipped default), at most 5 of 187 classes ever reach
`Match::best`, but every allowed class's prototypes were scanned to near
completion regardless, because nothing pruned a losing class against the
classes that were actually winning. That is the 23,000–26,000
prototypes-visited-per-call figure above, and the reason early abandonment
fired only 58–64% of the time even with the existing per-16-dimension
checkpoint.

## The fix: a second, cross-class ceiling (exact)

`crates/ocrcer-core/src/match.rs`, `nearest()`. Alongside the existing
per-class ceiling, track `global_ceiling`: the current m-th smallest finite
value in `best_d`, where `m = max(k, 2)` (`k` for `Match::best`, at least 2
so `d1`/`d2` are always exact). Each of the 16-dimension checkpoints now
abandons against `best_d[class].min(global_ceiling)` rather than
`best_d[class]` alone. `global_ceiling` is recomputed (via
`select_nth_unstable_by` over `best_d`'s ≤187 finite entries — cheap next to
a 107-dimension distance) only when some class's best actually improves, not
per prototype.

### Why this cannot change the answer

1. **`best_d` entries only fall.** Once set, a class's recorded best is only
   ever replaced by something smaller (`acc < ceiling` in the unchanged
   acceptance test). A fixed-rank order statistic of an array whose entries
   only fall is itself non-increasing over time. So any `global_ceiling`
   read at any point during the scan is `>=` its own final value.
2. **A partial sum never exceeds the true distance.** Every per-dimension
   term `w[i] * d * d` is non-negative, so a prototype's partial sum at any
   checkpoint is `<=` its fully-summed distance.
3. Combine the two: for a prototype whose true distance `D <= T` (`T` being
   the *final* global ceiling — the m-th smallest value the scan will end
   with), its partial sum at every checkpoint is `<= D <= T <= global_ceiling
   at that checkpoint`. The abandon test (`acc >= checkpoint_ceiling`) can
   therefore never fire for it before its class's own true best is recorded
   — the class this prototype belongs to gets an exact value regardless of
   whether that value came from this prototype or from whichever one is
   visited first in file order and achieves it.
4. Every class that ends up in the final top-`m` (the only classes
   `Match::best`, `d1` and `d2` ever expose) has, by definition, a true best
   `<= T`, so step 3 covers exactly the classes the caller reads. Classes
   outside the top-`m` may be pruned more aggressively and can end up with a
   looser-than-true recorded value or none at all — invisible to the output,
   since they were never going to appear in it.
5. The final acceptance test (`acc < ceiling`, `ceiling = best_d[class]`
   alone, never the cross-class one) is untouched, so a class's recorded
   value is exactly what it would have been without this change whenever the
   loop does reach it; the cross-class ceiling only decides how early a
   prototype that cannot win its own class is allowed to stop summing.

New test: `the_cross_class_ceiling_gives_the_same_answer_as_a_full_scan` —
400 classes, `top_k = 3` (so `m = 3 <<` class count, the regime where the new
ceiling actually restricts), compared against a brute-force per-class best
over 25 query points. All existing tests
(`early_abandonment_gives_the_same_answer_as_a_full_scan`, and the rest of
`match.rs`'s suite) pass unchanged. Note that both existing small-bank tests
use `top_k >= class count`, which makes `m >= n_classes`: the m-th order
statistic of the whole array is its maximum, so *every* true distance is
`<= T` trivially and the new ceiling proves out to give zero extra pruning
on those fixtures by the same argument above — it is not that they happen to
pass, it is that the proof predicts they must.

## After: same three pages, same instrumentation

| Page | wall before (ms) | wall after (ms) | speedup | match() before (ms) | match() after (ms) | abandon rate after |
|---|---|---|---|---|---|---|
| filing__r000011 | 14,386.8 | 10,255.8 | 1.40x | 13,822.4 | 9,699.3 | 96.2% |
| filing__r000605 | 38,672.1 | 25,583.4 | 1.51x | 37,751.4 | 24,711.2 | 98.1% |
| filing__r000462 | 8,871.2 | 6,301.4 | 1.41x | 8,487.5 | 5,929.0 | 98.0% |

Both binaries built from the same worktree, same model file, same page
files, same machine, sequentially (never concurrently with another `ocr.exe`
process, checked via `tasklist` before each run per the isolation
instructions for this chunk). "Before" is a temporary `git checkout` of
`match.rs` to `HEAD` (pre-optimisation) rebuilt with the same profiling
instrumentation as "after"; prototypes-visited counts are unaffected by the
change (same allowed-class set is scanned either way — see the raw-count
column in the profile table above, unchanged before/after at ~23–26k/call),
so the entire speedup shown here is fewer *dimensions summed per visited
prototype*, exactly what the abandon-rate jump from ~58–64% to ~96–98%
predicts.

## Full-corpus verification

Full-corpus runs are exclusive machine-wide (only one heavy `ocr.exe` run at
a time, per this chunk's isolation instructions); each pair below was run
sequentially against a baseline `ocr.exe` (temporary `HEAD` `match.rs`,
otherwise identical build) and the optimised one, same model file, same
page directories.

**pages-cov (625 pages, `bench/pages-cov`):**

- Baseline CSV/CER: end-to-end 5.900%, line-matched 5.899% (625 rows)
- Optimised CSV/CER: end-to-end 5.900%, line-matched 5.899% (625 rows)
- `diff` of the two `--csv` outputs: identical (exit 0)

**finfilings (60 pages, `pages/finfilings`):**

- Baseline CSV/CER: end-to-end 12.167%, line-matched 11.122% (60 rows)
- Optimised CSV/CER: end-to-end 12.167%, line-matched 11.122% (60 rows)
- `diff` of the two `--csv` outputs: identical (exit 0)

`cargo test --workspace --release`: 194 `ocrcer-core` tests plus the rest of
the workspace, all green (see the two new `match.rs` tests above; nothing
else in the suite touches matching).

`cargo build -p ocrcer-core --target wasm32-unknown-unknown`: clean, no
warnings. `prof.rs`'s wasm32 module never reaches `Instant`, matching its
existing contract comment; `match.rs`'s only new state is a `Vec<f64>`
scratch buffer and a few `f64`/`usize` locals, all `core`/`alloc`/`std`.

## What this chunk did not do (non-exact proposals, not implemented)

Everything below changes output and is out of scope for this chunk. Sizes
are rough, unmeasured estimates, not projections against real data:

- **Aspect-band / baseline-class second-stage pruning.** Already tried and
  measured to cost accuracy (section 4.1's own comment); not revisited here.
- **Feature caching across lattice edges.** Two edges in the same word's
  lattice can crop overlapping ink; if a caller ever calls `nearest()` twice
  with an identical raw feature vector for the same word (this pipeline does
  not currently do so — each edge crops distinct ink and extracts once), a
  memo keyed on the raw vector's bits would be exact. Not pursued because
  profiling here found no such duplicate calls to memoize.
- **The `parallel` (rayon) feature**, matching hypotheses/prototypes across
  threads. Left off this round because the exact fix already closes ~30–35%
  of the gap on the pages measured, and a parallel path needs its own
  byte-identical-output proof against these same fixtures before it can
  merge (project rule) — a second piece of work, not a small addition to
  this one. Rough guess, unmeasured: on a 4+ core host, additional headroom
  in the same ballpark as this chunk's gain, since matching is still ~95%+
  of wall time and the classes-per-query loop parallelises without changing
  which prototype wins each class.
- **Tightening the hole-count gate further** (e.g. an aspect ratio band that
  does not lose accuracy, unlike the one already rejected) was not
  attempted; it is a model/feature question for `ocrcer-architect`, not a
  runtime change.

## Wall time on the full corpora: not a reading

The four full-corpus runs (2026-09-24, 12:09-13:16) ran while the operator's machine was
shared with other jobs. Their wall times (finfilings 25.8 min baseline, 15.9 min optimised)
are indicative only and are not filed as a speedup. The per-page 1.40-1.51x figures above
stand as the chunk's reading; a pinned-core rerun replaces the corpus wall times later.
