# Resume here

Paused on 2026-09-23. This note describes the present, not the history. The
history lives in `docs/ARCHITECTURE.md` section 11 and in
`docs/measurements/`.

Nothing is committed yet. Branch `master`, zero commits, no remote, every
file untracked. **A first commit has been offered and not yet asked for.**

---

## 1. State of the tree

`cargo test --workspace` was green as of the last recorded run this session
(`docs/measurements/2026-09-23_line_merge_phase2.txt`; per-crate counts in
`2026-09-23_column_fragment_min.txt`: 146 `ocrcer-core` tests, full workspace
38+3+2+14+82(71 passed, 11 pre-existing ignored)+6+146, no failures).

Shipped this session, all in `crates/ocrcer-core/src/layout/lines.rs` and
`crates/ocrcer-core/src/params.rs` / `model/params.tsv` unless noted:

* `lines.column_lone_guard` (toggle, default **0** — greedy 1.75-height cut,
  guard logic present but off; see section 3).
* `lines.x_height_floor_per_cap` (0.3168, `measured` from
  `ocrcer-build metrics` across the 32-face bank) and `lines.baseline_split`
  (now **1**, `measured`) with its two constants `baseline_split_sep` (0.6)
  and `baseline_split_support` (0.25), both still `guess`, unswept.
* `model/fonts.tsv` — Bold rows added for every bank family with a present,
  licence-cleared Bold file.
* `model/out/ocrcer.ocrw` — rebuilt at the five-size ladder (16/20/24/32/48),
  32 faces, 29,675 prototypes, int8 top-1 agreement 99.486%, 3.15 MB.
* `ocrcer-exporter`'s `meta` record now carries the build size ladder;
  `inspect` prints it. Additive, no format-version bump.
* `ocrcer-bench`'s `cer.rs` — a line-matched-CER bug fixed mid-session (it
  was reading higher than end-to-end CER, which should not happen for a
  reading-order-independent metric). Figures from before the fix are marked
  unquotable in their own measurement files, not silently replaced.

**Check the binary against the source you changed, not against the clock**
— this bit twice last session and is not a stale warning to ignore:
`docs/measurements/2026-09-23_line_merge_phase2.txt` records the shipped
binary printing a STALE warning against a newer model file, verified by
hand to be harmless (no `ocrcer-core` source changed in between) rather than
trusted on sight.

---

## 2. Start here

1. **`filing__r000022`'s dense-table trace.** Distinct from the line-merge
   mechanism fixed this session — 9% of the six-page deletion sample, a
   narrow-column/reading-order collapse on a dense numeric table, inferred
   from confusion-pattern signatures, not yet pixel-verified. Classical
   research (`docs/measurements/2026-09-22_research_classical_techniques.md`
   addendum, 2026-09-23) proposes pixel-level morphological rule removal as
   the untested candidate, once it's confirmed the lost text is
   rule-touching.
2. **Italic**, queued behind the merge fix and now with concrete single-page
   evidence: `filing__r000583` loses ~54% CER to an unrepresented italic
   style with segmentation confirmed clean (matcher/font-coverage gap, not a
   layout defect). Measure like bold was measured before deciding.
3. **A ligature error-share count** on the rebuilt bold bank. Prevalence is
   known (24/60 `finfilings` pages both use an fi/fl-pair word and render in
   the ligature-forming serif family) but the count of ligature-attributable
   errors on the current bank has not been taken.
4. **The column-cut lone-guard per-page diff.** Two rules failed their gate
   this session, both reasoned from leader-line fixtures alone; before a
   third rule is proposed, run the toggle both ways on `finfilings` and diff
   which pages/lines move.
5. **Re-measure `lines.rule_aspect`** — still owed since 2026-09-22; its
   measurement predates the word-valley floor.
6. **Re-measure head-to-head vs `ocrs`** on `bench/pages-cov` — still stale;
   this comparison is the project's reason to exist.
7. **Re-run SROIE** against the current reading-order and line-merge fixes,
   to see how much of its CER was layout defect vs. corpus (out-of-domain,
   photographed receipts).
8. **DejaVu Serif: withdrawn (2026-09-23, architect).** No record shows the filings
   use it; raise again only if a font audit of the filings finds it.

**Run heavy full-corpus sweeps in the foreground, one arm at a time.** Two
separate sessions have now had a background sweep reaped for low system
memory while another project built concurrently (2026-09-22, 2026-09-23) —
see `personal_rag/ocr/lesson_20260923_background_long_runs_get_reaped_under_memory_pressure.md`.

---

## 3. Where things landed this session

**The column-cut lone-glyph guard is specified but shipped off.** Two rules
were reasoned from the leader-line fixtures (a run of dots must not dice
into one-glyph lines) and both cost real pages when measured on
`finfilings`: "both bounding fragments ≥2 components" cost 1.47 F1 points;
narrowed to "drop only between two singletons" still cost 0.41. Neither
shipped. `lines.column_lone_guard` exists as a toggle, default 0, so both
behaviours run from one binary once the per-page diff (item 4 above) is
done.

**Bold weight ships, after a same-day self-correction.** First reading
compared a four-size bold candidate against the shipped five-size control —
an unrecorded-ladder confound that made a real accuracy gain look like a
near-miss on `pages-cov` CER. Re-measured like-for-like: `pages-cov` F1
76.675%→77.392%, CER 6.503%→6.127%; `finfilings` F1 68.277%→70.846%, CER
19.544%→18.546%. Both gates pass.

**The merged-line defect — 82% of the worst pages' deletions — is diagnosed
and fixed.** Two tightly-leaded real lines banding into one, admitted by
`overlap_fraction` alone at a ratio indistinguishable from a legitimate
same-line join; the resulting collapsed x-height was mislabelled `Observed`,
defeating the existing `inherit_x_heights` safety net. A plausibility floor
alone (rule A) raised F1 but worsened CER — more text recovered, still
interleaved — and failed its own gate. Floor + a baseline-bimodality split
(rule B) together passed every gate: `finfilings` CER 18.546%→17.064%, F1
70.846%→73.615%; `pages-cov` unchanged. Shipped as A+B.

Full narrative and every reading: `docs/ARCHITECTURE.md` section 11, ten
entries dated 2026-09-23; `docs/measurements/2026-09-23_*`.

---

## 4. Constraints that bind the next session

* **Charset, feature vector and normalisation are frozen.** Changing any
  needs a `.ocrw` `version` bump and a full bank rebuild, and is the last
  resort. Nothing this session touches them.
* **`ARCHITECTURE.md` section 11 is append-only.** Supersede with a new
  entry and a forward pointer; the old text stays — this session did that
  twice (the column-lone-guard rule, and the bold-ladder correction).
* **A projection is labelled a projection; a reading is labelled a
  reading.** Every figure in this file traces to a numbered
  `docs/measurements/2026-09-23_*` file.
* **Blessing a fixture is a deliberate, reviewed act.** No fixture was
  reblessed this session; two column-cut rules were withdrawn instead of
  having fixtures rewritten to fit them.
* **Nothing downloaded from the web enters the repository.** Corpora live
  in `D:/Dev/ExcludedPrivate/ocrcer`.
* **Compare model variants only at an identical size ladder.** The bold
  correction this session is the second time an unrecorded build parameter
  has confounded a comparison; `inspect` now prints the ladder specifically
  so this stops being possible silently.

---

## 5. Waiting on Ken

* **The first git commit.** Offered, not yet asked for.
* **The RAG rename sweep** — ~247 files in `C:\personal_rag`, ~437 in
  `D:\dev\rag` still say `pdfce`.
* **`osifont`'s GPL font exception** — unresolved; the face is not in the
  bank.
