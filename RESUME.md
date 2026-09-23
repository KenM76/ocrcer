# Resume here

Paused on 2026-09-23. This note describes the present, not the history. The
history lives in `docs/ARCHITECTURE.md` section 11 and in
`docs/measurements/`.

**Under git since 2026-09-23, first commits approved by Ken.** Branch
`master`, no remote. `.gitattributes` pins LF everywhere (fixtures are
compared byte-for-byte; a checkout-time CRLF rewrite would change their
hashes). **Commit after each passing change from here forward.**

---

## 1. State of the tree

`cargo test --workspace --release` was green as of the last recorded run
this session (304 tests earlier in the day; re-verified after the
`baseline_split_valley_margin` default change per `ARCHITECTURE.md`).
Rebuild and re-check before trusting that number — it is not re-verified in
this note.

Shipped this session's final leg, in `crates/ocrcer-core/src/layout/lines.rs`:

* **`lines.baseline_split_valley_margin`, default 0.3, provenance
  measured.** The two-baseline split pass (`lines.baseline_split=1`, shipped
  earlier the same day) exists to catch two ordinary text lines fused into
  one band on tight leading, by testing whether a band's baseline-candidate
  population is bimodal with a genuinely empty valley between the two
  peaks. It wasn't firing on real pages: the valley test excluded a fixed
  2-pixel-row margin around each candidate peak, and at this corpus's real
  body sizes, descenders (`g p q y j`) reach past that fixed margin and get
  counted as valley evidence, hiding genuine fusions. Fix: scale the margin
  to the line's own x-height. **This is the largest single gain measured
  on the real-filings corpus to date.**

**NEW CONTROLS — beat these, both corpora, at the shipped config (split
gate 1.09, underline strip on, `merge_overlap_frac` 0.4,
`baseline_split_valley_margin` 0.3):**

| | end-to-end CER | line-matched CER | word F1 |
|---|---|---|---|
| `finfilings` (60 real pages) | **13.161%** | **12.290%** | not re-run this filing |
| `pages-cov` (625 synthetic pages) | **6.064%** | 6.064% | not re-run this filing |

Session-start figures for this leg were 16.089 / 15.910 / 6.057 —
the gain above is real, not a typo.

**The worst `finfilings` pages are unknown at this control and need
re-listing before anything else is proposed.** The last known ranking
(against the *prior* control, `merge_overlap_frac=0.4` before this fix) was
`r000583` (40.63%), `r000022` (37.10%, already diagnosed as touching ink),
`r000308` (34.45%, now fixed by this session — expect it to drop sharply),
`r000055` (32.49%, already diagnosed as the underline-strip sliver),
`r000363` (32.27%, also fixed by this session — expect it to drop sharply).
`r000308` and `r000363` were the two pages this fix targeted directly, so
their post-fix rank is unknown, not merely uncertain — re-list before
touching either again. **`r000583` is expected to still be near the top**:
its defect (serif bounding-box chaining, a different mechanism from the
line-fusion this session fixed) was not touched by this leg's change, and
it was already the single worst page at 40.63% under the *outgoing*
control.

**Check the binary against the source you changed, not against the
clock** — unchanged advice from before, still true.

---

## 2. Start here — next-up queue, in order

1. **Re-list the worst `finfilings` pages against the new 0.3 control.**
   The ranking above is against the outgoing control and is known to be
   stale for at least `r000308`/`r000363`. Do this before proposing
   anything else.
2. **Sweep `baseline_split_valley_margin` below 0.3** — 0.2 and 0.25 were
   not screened; only {0.3, 0.4, 0.6, 0.7} were tried, and 0.3 won that
   set, but the lower end of the range is unexplored.
3. **`r000583`'s residual bbox-chaining mechanism.** Worked down from
   54.66% to 40.63% by the `merge_overlap_frac` fix but not resolved — its
   tightest chains clear the "always merges" branch a fraction threshold
   cannot gate. Candidate fixes, queued since 2026-09-22's classical-
   technique research and now with two consecutive worst-page diagnoses
   pointing at the same page: non-vertical (drop-fall/contour-following)
   cuts, and a width-scaled `max_splits` so a wide chained atom gets more
   than 3 cut attempts.
4. **The small checkbox/form-field column-cut fragmentation on `r000308`.**
   Named in the round-2 diagnosis, well under 10% of that page's
   characters — not urgent, but recorded so it isn't lost once `r000308`'s
   dominant line-fusion defect is gone.
5. **Optionally sweep `merge_overlap_frac` at 0.35/0.45/0.5** on the full
   `finfilings` corpus. Only {0.15, 0.2, 0.3, 0.4} were run full-corpus;
   low priority relative to items 1–4.
6. **Recognition-gated chopping** (Tesseract-style: chop only the
   least-confident atom, undo non-improving chops) — the next candidate if
   a plain width threshold on a cut-search gate ever stops passing both
   corpora again. Not measured, research only.
7. **Italic.** `r000583` was the concrete worst-page evidence for this
   before it dropped to 40.63%/54.66%; check whether italic is still the
   dominant remaining loss anywhere once item 1's re-listing is done.
8. **A ligature error-share count** on the bold bank. Prevalence known
   (24/60 `finfilings` pages carry the ligature-forming serif family and an
   fi/fl word), error count not taken.
9. **`baseline_split_sep`/`baseline_split_support` sweeps.** A *different*
   pair of constants from the ones shipped this session — these are the
   two-baseline-population thresholds from the earlier (same-day) merged-
   line fix, still `guess` provenance, unswept.
10. **Re-measure `lines.rule_aspect`.** Owed since 2026-09-22.
11. **Re-measure head-to-head vs `ocrs`** on `bench/pages-cov`. Stale; this
    comparison is the project's reason to exist.
12. **Re-run SROIE** against the current reading-order and line-merge
    fixes.
13. **ALTO/hOCR underline-formatting output**, from the `RuleSegment` data
    the underline strip records but nothing yet consumes. Direction from
    Ken; not scheduled as a chunk until he says so.
14. **The column-cut lone-guard per-page diff.** Still not run; both
    leader-line-derived rules failed their real-filings gate and neither
    shipped; `lines.column_lone_guard` stays 0.
15. **The `filing__r000022` dense-table trace.** 9% of an earlier
    six-page deletion sample, a narrow-column/reading-order collapse,
    inferred from confusion pattern, not yet pixel-verified.
16. **A one-time `rustfmt` pass.** 63 files drift against no committed
    `rustfmt.toml`; **pending Ken's call** — see section 5.
17. DejaVu Serif: withdrawn (2026-09-23, architect). No record shows the
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
CERs (now 13.161 / 12.290); `pages-cov` no worse than +0.05 (now
baselined at 6.064).

**Working mode:** the `/loop` "continue working on features and research
OCR techniques" prompt is what drives this session's shape — a fix
followed by research followed by the next fix it surfaces, each one gated
and shipped or explicitly rejected before moving on. **Commit after every
passing change, locally, never push.** Replies to Ken are TL;DR — the
detail belongs in these docs, not the chat reply.

---

## 3. Where things landed this session's final leg

**Round-2 diagnosis split one "worst page" problem into two.** `r000583`
is the same serif-bbox-chaining mechanism as before, smaller (54.66% →
40.63%) but unresolved. `r000308` and `r000363` turned out to be a
different, previously undocumented mechanism: `group_with_bands` fuses
pairs of ordinary body-text lines into one x-interleaved band on tight
leading (~1.7 x-heights). The existing two-baseline split pass should have
caught this and wasn't — traced to a fixed 2-pixel valley-exclusion margin
that doesn't scale with type size, so descenders at real body sizes were
miscounted as valley evidence.

**The fix scales that margin to x-height** (`lines.baseline_split_valley_margin`,
0.3, measured). Two known-fused pages: 34.45%/32.27% CER → 14.50%/18.90%.
Full corpus: `finfilings` end-to-end CER 16.089% → **13.161%**, line-matched
15.910% → **12.290%**; `pages-cov` 6.057% → 6.064% (a 0.007-point loss,
within tolerance, recorded not absorbed).

Full narrative: `docs/ARCHITECTURE.md` §11, "Worst pages, round 2" and
"Line fusion fix: `lines.baseline_split_valley_margin` 0.3, measured";
readings in `docs/measurements/2026-09-23_worst_pages_round2.md`,
`_line_fusion_fix.txt`.

One new `personal_rag/ocr` lesson from this leg:
`lesson_20260923_fixed_pixel_margins_near_a_profile_valley_must_scale_with_x_height.md`
— generalises past this project: any profile-peak/valley test's exclusion
margin needs to scale with the population's own size metric, not a raw
pixel count.

---

## 4. Constraints that bind the next session

* **Charset, feature vector and normalisation are frozen.** Unchanged.
* **`ARCHITECTURE.md` section 11 is append-only.** Supersede with a new
  entry and a forward pointer; the old text stays.
* **A projection is labelled a projection; a reading is labelled a
  reading.** Every CER/F1 figure in this file traces to a numbered
  `docs/measurements/2026-09-23_*` file, except the disk figure in section
  5, which is reported this session and explicitly not independently
  re-verified — no shell was available to the filing dispatch.
* **Blessing a fixture is a deliberate, reviewed act.** No fixture was
  reblessed this session.
* **Nothing downloaded from the web enters the repository.** Corpora live
  in `D:/Dev/ExcludedPrivate/ocrcer`.
* **Compare model variants only at an identical size ladder.** Unchanged —
  guards a confound found on 2026-09-23, still applies to any future bank
  rebuild.
* **Gate any cut-search width or margin parameter on both corpora, not
  just the one that motivated the change.** Established by the split-gate
  sweep and reconfirmed by this leg's `baseline_split_valley_margin` gate
  (both `finfilings` and `pages-cov` were checked before shipping).
* **Commit after each passing change**, now that the tree is under git —
  see the header of this file and `ROADMAP.md`'s Standing rules.

---

## 5. Waiting on Ken

* **A one-time `rustfmt` pass** across 63 drifted files (no `rustfmt.toml`
  committed). Not run, pending his call.
* **Does "commit after each passing change" extend to Ken's other project
  trees, or is it scoped to `D:\Dev\OCRcer`?** Every place the rule is
  written names only this tree, but it was never asked whether that
  scoping was deliberate.
* **ALTO/hOCR underline-formatting output**, direction given, not yet
  scheduled as a chunk.
* **The RAG rename sweep** — ~247 files in `C:\personal_rag`, ~437 in
  `D:\dev\rag` still say `pdfce` (carried forward, unchanged).
* **`osifont`'s GPL font exception** — unresolved; the face is not in the
  bank (carried forward, unchanged).
* **Disk space — resolved, no longer waiting.** Reported this session as
  **273 GB free** on D: after an outside cleanup, superseding the prior
  two sessions' "38 GB" and "~10 GB" figures (both themselves unverified).
  Still not independently re-verified in this filing (no shell available),
  but the margin is now wide enough that this item is closed unless a
  future session finds otherwise.
* **Clippy: 45 warnings** (top lint `needless_range_loop`, 13 occurrences)
  — reported, report-only, no action requested yet.
