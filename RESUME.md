# Resume here

Paused on 2026-09-23. This note describes the present, not the history. The
history lives in `docs/ARCHITECTURE.md` section 11 and in
`docs/measurements/`.

**Under git since 2026-09-23, first commits approved by Ken.** Branch
`master`, five commits, no remote:
`4c85f69` initial snapshot, `9436f50` LF line-ending pin + binary-fixture
marking, `a8a24be` split-gate ship, `f9dcd8f` underline-strip code moved
into `layout/underline.rs`, `3772a41` underline-strip ship + build-report
fix. `.gitattributes` pins LF everywhere (fixtures are compared
byte-for-byte; a checkout-time CRLF rewrite would change their hashes).
**Commit after each passing change from here forward.**

---

## 1. State of the tree

`cargo test --workspace --release` was green as of the last recorded run
this session: **304 tests, 0 failed**
(`docs/measurements/2026-09-23_split_gate_and_strip_3b.txt`).

Shipped this session (continuing on from the merged-line/bold-weight work
recorded earlier the same day), all in `crates/ocrcer-core/src/` unless
noted:

* `segment.split_min_x_heights` **1.15 → 1.09** (measured) — the cut-search
  width gate on touching-glyph atoms. 1.0 won biggest on `finfilings` but
  failed the `pages-cov` gate (over-segmentation); 1.09 passes both. Not
  1.10: `1.10f32` widens to `1.1000000238` in `f64`, which would still miss
  the target 11px atom under the strict `<` gate.
* `lines.thin_debris_heights` (underline-strip rule 2, part 3b) — a joint
  width-and-height gate on strip-produced debris, fixing the box-side
  sliver mechanism below. Base value 3.4288, measured; ×1.5 headroom, a
  labelled guess.
* `lines.underline_strip` **0 → 1** (measured). Ships.
* Underline-strip code moved from `layout/lines.rs` into its own
  `layout/underline.rs` — a pure move, no behaviour change.
* `ocrcer-bench`'s build report — a params-census bug (measured/authored
  figures swapped) fixed.

**Current controls, both corpora, at the shipped config (split gate 1.09,
underline strip on):**

| | CER | line-matched CER | word F1 |
|---|---|---|---|
| `finfilings` (60 real pages) | **16.756%** | **16.634%** | **74.405%** |
| `pages-cov` (625 synthetic pages) | **6.089%** | 6.089% | **77.429%** |

`r000583` is now the worst `finfilings` page at 54.66% CER (attributed to
italic, segmentation confirmed clean — see item 2 below), not `r000022` or
`r000055` as in earlier readings.

**Check the binary against the source you changed, not against the clock**
— unchanged advice from before, still true.

---

## 2. Start here

1. **`filing__r000022`'s dense-table trace.** 9% of the earlier six-page
   deletion sample, a narrow-column/reading-order collapse, inferred from
   confusion pattern, not yet pixel-verified.
2. **Italic**, queued behind everything above and now the single worst
   page: `filing__r000583` at 54.66% CER, unrepresented italic style,
   segmentation confirmed clean (matcher/font-coverage gap). Measure like
   bold was measured before deciding.
3. **A ligature error-share count** on the bold bank. Prevalence known
   (24/60 `finfilings` pages), error count not taken.
4. **The column-cut lone-guard per-page diff.** Still not run — both
   leader-line-derived rules failed their real-filings gate and neither
   shipped; `lines.column_lone_guard` stays 0.
5. **Re-measure `lines.rule_aspect`** — still owed since 2026-09-22.
6. **Re-measure head-to-head vs `ocrs`** on `bench/pages-cov` — still stale;
   this comparison is the project's reason to exist.
7. **Re-run SROIE** against the current reading-order and line-merge fixes.
8. **Recognition-gated chopping** (Tesseract-style: chop only the
   least-confident atom, undo non-improving chops) — recorded as the next
   candidate for the segmentation cut gate if a plain width threshold ever
   stops passing both corpora again. **Not measured**, research only
   (`docs/measurements/2026-09-22_research_classical_techniques.md`
   addendum 2026-09-23).
9. **ALTO/hOCR underline-formatting output**, from the `RuleSegment` data
   the underline strip now records but nothing yet consumes. Direction from
   Ken, recorded in `ARCHITECTURE.md` §11; not scheduled as a chunk until
   Ken says so.
10. **A one-time `rustfmt` pass.** 63 files drift against no committed
    `rustfmt.toml`; pending Ken's call — see section 5.
11. DejaVu Serif: withdrawn (2026-09-23, architect). No record shows the
    filings use it; raise again only if a font audit finds it.

**Run heavy full-corpus sweeps in the foreground, one arm at a time** —
still true; two sessions in a row have had a background sweep reaped for
low memory (`personal_rag/ocr/lesson_20260923_background_long_runs_get_reaped_under_memory_pressure.md`).

---

## 3. Where things landed this session

**The underline-strip regression is diagnosed and fixed.** Stripping a
bordered box's rules correctly erased the furniture but left the box's own
sides behind as 10–22×h slivers — too narrow for the width-keyed furniture
filter, too short for the height-keyed debris filter. Tallest-first line
grouping seeded a band on a sliver and fused two real prose lines. Fixed
with a joint width-and-height gate on strip-produced pieces specifically.

**The `0$` touching-atom case was a threshold miss, not a design gap** —
the cut-search gate never ran at that width. Swept and shipped at 1.09
(narrowest value that passes both corpora; not 1.10, because of f32→f64
widening at the exact boundary).

**Underline strip ships** at the new split gate: `finfilings` CER
16.932%→16.756%, F1 74.047%→74.405%; `pages-cov` unchanged.

Full narrative: `docs/ARCHITECTURE.md` section 11, six entries dated
2026-09-23 from "Underline strip fails the real-filings gate" through
"Underline strip ships (rule 2 with part 3b)"; readings in
`docs/measurements/2026-09-23_underline_strip*.txt`,
`_underline_strip_damage.md`, `_underline_r000055_and_touching_r000022.md`,
`_split_gate_and_strip_3b.txt`.

Four new lessons in `personal_rag/ocr/`: the strip-debris width+height gate,
the f32→f64 threshold-widening trap, dual-corpus gating for cut-search
width parameters, and Tesseract's confidence-gated chop/undo (research,
unbuilt).

---

## 4. Constraints that bind the next session

* **Charset, feature vector and normalisation are frozen.** Unchanged this
  session.
* **`ARCHITECTURE.md` section 11 is append-only.** Supersede with a new
  entry and a forward pointer; the old text stays.
* **A projection is labelled a projection; a reading is labelled a
  reading.** Every figure in this file traces to a numbered
  `docs/measurements/2026-09-23_*` file, except the code-health and
  disk-pressure figures in section 5, which are reported by this session
  and explicitly not independently re-verified — no shell was available to
  the filing dispatch.
* **Blessing a fixture is a deliberate, reviewed act.** No fixture was
  reblessed this session.
* **Nothing downloaded from the web enters the repository.** Corpora live
  in `D:/Dev/ExcludedPrivate/ocrcer`.
* **Compare model variants only at an identical size ladder.** Unchanged
  from before — the confound this rule guards against is not this
  session's finding, but the discipline still applies to any future bank
  rebuild.
* **Commit after each passing change**, now that the tree is under git —
  see the header of this file and `ROADMAP.md`'s Standing rules.

---

## 5. Waiting on Ken

* **A one-time `rustfmt` pass** across 63 drifted files (no `rustfmt.toml`
  committed). Not run pending his call.
* **ALTO/hOCR underline-formatting output**, direction given, not yet
  scheduled as a chunk.
* **The RAG rename sweep** — ~247 files in `C:\personal_rag`, ~437 in
  `D:\dev\rag` still say `pdfce` (carried forward, unchanged).
* **`osifont`'s GPL font exception** — unresolved; the face is not in the
  bank (carried forward, unchanged).
* **Confirm current D: free space.** Reported this session as 38 GB free
  after a disk-pressure incident (two untracked scratch files,
  `aspect_err.txt`/`aspect_out.txt`, deleted by an agent; stale `target/`
  dirs cleaned) — not independently re-verified in this filing.
