# Resume here

Paused on 2026-09-23. This note describes the present, not the history. The
history lives in `docs/ARCHITECTURE.md` section 11 and in
`docs/measurements/`.

**Under git since 2026-09-23, first commits approved by Ken.** Branch
`master`, no remote. Most recent five commits as of this pause:
`13c422f` merge_overlap_frac 0.3→0.4, `e01cf6a` full-corpus sweep of
merge_overlap_frac, `3816af6` decision-log entry for the 0.3 ship,
`e60211a` merge atoms by overlap fraction (the fix itself), `69fa60d`
research note on drop-fall cuts. `.gitattributes` pins LF everywhere
(fixtures are compared byte-for-byte; a checkout-time CRLF rewrite would
change their hashes). **Commit after each passing change from here
forward.**

---

## 1. State of the tree

`cargo test --workspace --release` was green as of the last recorded run
this session. Rebuild and re-check before trusting that number — it is not
re-verified in this note.

Shipped this session, all in `crates/ocrcer-core/src/segment/` unless
noted:

* **`segment.merge_overlap_frac`, default 0.4, provenance measured.**
  `atoms()` used to merge any two components whose x-ranges overlapped at
  all — correct for an `i` and its dot, wrong for a kerned serif `t`/`h`
  pair that overlaps by a few columns at different heights without the ink
  touching. Now merges only on full column-range containment or overlap ≥
  the fraction × the narrower component's width. Pieces are cropped by
  their own member components (`edge_labels()`), not by column range.
  Shipped in two measured steps: 0.3 first (passed all three gates), then
  a full-corpus sweep found 0.4 better on both `finfilings` CERs with
  `pages-cov` unmoved.
* A research-note addendum on drop-fall (non-vertical/contour) cuts,
  appended to `docs/measurements/2026-09-22_research_classical_techniques.md`
  — queued, not built. This is the candidate fix for touching-ink pages;
  `r000583` (below) did not need it because its defect was chaining, not
  touching ink.

**Current controls, both corpora, at the shipped config (split gate 1.09,
underline strip on, `merge_overlap_frac` 0.4):**

| | CER | line-matched CER | word F1 |
|---|---|---|---|
| `finfilings` (60 real pages) | **16.089%** | **15.910%** | not re-run this filing |
| `pages-cov` (625 synthetic pages) | **6.057%** | 6.057% | **77.540%** (reported, not independently re-run) |

`r000583` was 54.66% CER at session start, traced to bounding-box overlap
chaining seven letters into one atom in a serif face; it is now **40.00%
CER** at the shipped fix and is **still the worst `finfilings` page** —
re-diagnose it before proposing a third rule. `r000022`, `r000055` and
`r000044` were reading 34–37% CER before this fix and have not been
re-measured against the new control; do that first, in order, below.

**Check the binary against the source you changed, not against the
clock** — unchanged advice from before, still true.

---

## 2. Start here — next-up queue, in order

1. **Re-diagnose the new worst pages against the 0.4 control.** `r000583`
   is still worst (40.00% CER, was 54.66%). `r000022`, `r000055`, `r000044`
   were 34–37% before this fix and are unmeasured against it — re-measure
   all four before proposing another segmentation rule.
2. **Optionally sweep 0.35/0.45/0.5 for `merge_overlap_frac` on the full
   `finfilings` corpus.** Only {0.15, 0.2, 0.3, 0.4} were run full-corpus;
   0.5/0.7 were screened on one page only (0.5 read slightly worse than 0.3
   there). Small expected gain — low priority relative to item 1.
3. **Non-vertical (drop-fall/contour) cuts and a width-scaled
   `max_splits`, for touching ink.** Queued since 2026-09-22's classical-
   technique research, now with sharper evidence: the chaining defect that
   `merge_overlap_frac` fixed was a different mechanism (bbox overlap, not
   touching ink) from what drop-fall addresses. Check the re-diagnosed
   worst pages (item 1) for touching-ink cases before building this.
4. **Recognition-gated chopping** (Tesseract-style: chop only the
   least-confident atom, undo non-improving chops) — the next candidate if
   a plain width threshold on the cut-search gate ever stops passing both
   corpora again. Not measured, research only.
5. **Italic.** `filing__r000583` was the concrete worst-page evidence for
   this before today's fix moved it to 40%; re-check whether italic is
   still the dominant remaining loss on that page or on the newly
   re-diagnosed pages from item 1.
6. **A ligature error-share count** on the bold bank. Prevalence known
   (24/60 `finfilings` pages carry the ligature-forming serif family and an
   fi/fl word), error count not taken.
7. **`baseline_split_sep`/`baseline_split_support` sweeps.** Both `guess`
   provenance, unswept, from the 2026-09-23 merged-line fix.
8. **Re-measure `lines.rule_aspect`.** Owed since 2026-09-22.
9. **Re-measure head-to-head vs `ocrs`** on `bench/pages-cov`. Stale; this
   comparison is the project's reason to exist.
10. **Re-run SROIE** against the current reading-order and line-merge
    fixes.
11. **ALTO/hOCR underline-formatting output**, from the `RuleSegment` data
    the underline strip records but nothing yet consumes. Direction from
    Ken; not scheduled as a chunk until he says so.
12. **The column-cut lone-guard per-page diff.** Still not run — both
    leader-line-derived rules failed their real-filings gate and neither
    shipped; `lines.column_lone_guard` stays 0.
13. **The `filing__r000022` dense-table trace.** 9% of an earlier
    six-page deletion sample, a narrow-column/reading-order collapse,
    inferred from confusion pattern, not yet pixel-verified — likely
    folds into item 1's re-diagnosis.
14. **A one-time `rustfmt` pass.** 63 files drift against no committed
    `rustfmt.toml`; **pending Ken's call** — see section 5.
15. DejaVu Serif: withdrawn (2026-09-23, architect). No record shows the
    filings use it; raise again only if a font audit finds it.

**Run heavy full-corpus sweeps in the foreground, one arm at a time.**
Two prior sessions had a background sweep reaped for low system memory
(`personal_rag/ocr/lesson_20260923_background_long_runs_get_reaped_under_memory_pressure.md`).

**Build recipe** (unchanged from prior sessions):

```
cargo build --release -p ocrcer-bench -p ocrcer-build --features pages
ocrcer-build write 16,20,24,32,48 21,26,36,56 model/out/ocrcer.ocrw
# then touch ocr.rs and rebuild ocr.exe — watch for STALE
```

**Corpora:** `finfilings` at
`D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings` (~25 min to run);
`bench/pages-cov` (~12 min). **Gates:** beat control on both `finfilings`
CERs; `pages-cov` no worse than +0.05.

**Working mode:** the `/loop` "continue working on features and research
OCR techniques" prompt is what drives this session's shape — a fix
followed by research followed by the next fix it surfaces, each one gated
and shipped or explicitly rejected before moving on. **Commit after every
passing change, locally, never push.** Replies to Ken are TL;DR — the
detail belongs in these docs, not the chat reply.

---

## 3. Where things landed this session

**The worst page's mechanism was pinned before any fix was written.**
`atoms()`'s any-overlap merge rule, written for an `i` and its dot, was
also gluing kerned serif letters (`t`/`h`) that overlap by a few columns
at different heights without their ink touching. `r000583` had 73 such
chained atoms (up to 7 letters) against 3 on an ordinary page, and a
7-letter chained atom defeats `max_splits = 3` outright.

**The fix narrows the merge rule to a real overlap fraction** rather than
replacing the technique: full containment or ≥ `merge_overlap_frac` ×
narrower width. Shipped at 0.3 (all three gates passed, screened on one
page), then re-swept full-corpus and moved to 0.4 (both `finfilings` CERs
improved further, `pages-cov` unmoved). The architect independently
reproduced the 0.4 result to the digit.

Full narrative: `docs/ARCHITECTURE.md` §11, the three entries "Worst page
named" through `segment.merge_overlap_frac` 0.3 → 0.4; readings in
`docs/measurements/2026-09-23_worst_page_r000583.md`,
`_atom_merge_overlap.txt`.

No new `personal_rag/ocr` lessons were owed from this specific fix beyond
what the diagnosis narrative already carries — see `ARCHITECTURE.md` for
the mechanism; check `personal_rag/ocr/index.md` before assuming one is
missing.

---

## 4. Constraints that bind the next session

* **Charset, feature vector and normalisation are frozen.** Unchanged.
* **`ARCHITECTURE.md` section 11 is append-only.** Supersede with a new
  entry and a forward pointer; the old text stays.
* **A projection is labelled a projection; a reading is labelled a
  reading.** Every CER/F1 figure in this file traces to a numbered
  `docs/measurements/2026-09-23_*` file, except the disk-space and
  clippy/rustfmt figures in section 5, which are reported by their
  sessions and explicitly not independently re-verified — no shell was
  available to the filing dispatch either session.
* **Blessing a fixture is a deliberate, reviewed act.** No fixture was
  reblessed this session.
* **Nothing downloaded from the web enters the repository.** Corpora live
  in `D:/Dev/ExcludedPrivate/ocrcer`.
* **Compare model variants only at an identical size ladder.** Unchanged —
  the confound this rule guards against (`lesson_20260923_compare_model_variants_at_identical_size_ladders.md`)
  is not this session's finding, but the discipline still applies to any
  future bank rebuild.
* **Gate any cut-search width parameter on both corpora, not just the one
  that motivated the change.** Established by the split-gate sweep
  (`lesson_20260923_gate_a_cut_search_width_change_on_both_corpora.md`);
  applies equally to `merge_overlap_frac` and to anything measured under
  item 3 of the queue above.
* **Commit after each passing change**, now that the tree is under git —
  see the header of this file and `ROADMAP.md`'s Standing rules.

---

## 5. Waiting on Ken

* **A one-time `rustfmt` pass** across 63 drifted files (no `rustfmt.toml`
  committed). Not run, pending his call.
* **Does "commit after each passing change" extend to Ken's other project
  trees, or is it scoped to `D:\Dev\OCRcer`?** Newly raised this session —
  every place the rule is written names only this tree, but it was never
  asked whether that scoping was deliberate.
* **ALTO/hOCR underline-formatting output**, direction given, not yet
  scheduled as a chunk.
* **The RAG rename sweep** — ~247 files in `C:\personal_rag`, ~437 in
  `D:\dev\rag` still say `pdfce` (carried forward, unchanged).
* **`osifont`'s GPL font exception** — unresolved; the face is not in the
  bank (carried forward, unchanged).
* **Confirm current D: free space.** Reported this session as **99% full,
  ~10 GB free**, after the architect deleted `target/debug`,
  `runtime-diag`, `glyphs-agent`, `wasm32` and `tmp` build directories
  (~3 GB reclaimed) — worse than the prior session's unverified 38 GB
  figure, and still not independently re-verified in this filing. Any
  diagnostic agent building under its own `CARGO_TARGET_DIR` will rebuild
  from scratch. Keep build directories minimal; check free space with a
  shell before launching a heavy sweep.
* **Clippy: 45 warnings** (top lint `needless_range_loop`, 13 occurrences)
  — reported, report-only, no action requested yet.
