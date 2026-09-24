# Resume here

Paused on 2026-09-24. This note describes the present, not the history. The
history lives in `docs/ARCHITECTURE.md` section 11 and in
`docs/measurements/`.

**Under git since 2026-09-23, first commits approved by Ken.** Branch
`master`, no remote. `.gitattributes` pins LF everywhere (fixtures are
compared byte-for-byte; a checkout-time CRLF rewrite would change their
hashes). **Commit after each passing change from here forward.**

---

## 1. State of the tree

Checkbox border-coverage detection shipped this session in
`crates/ocrcer-core`. Segmentation-cap and cut-candidate-generation
diagnoses on `r000583` both ruled out their suspected mechanism — **no
code changed from either**. Italic prototype faces were added, measured,
and **reverted** the same session (`fonts.tsv` and the face table are back
to pre-session state; the enlarged bank was never committed). Slant-gated
italic prototypes — the fix direction chosen instead of pooling — is **in
progress**, not yet gated. `cargo test --workspace --release` was last
recorded green earlier in the prior session; re-verify before trusting
that, it is not re-checked in this note.

**NEW CONTROLS — beat these, both corpora, at the shipped config (split
gate 1.09, underline strip on, `merge_overlap_frac` 0.4,
`baseline_split_valley_margin` 0.3, checkbox border-coverage on):**

| | end-to-end CER | line-matched CER | wall time |
|---|---|---|---|
| `finfilings` (60 real pages) | **12.708%** | **11.602%** | **1019.5 s** |
| `pages-cov` (625 synthetic pages) | **6.064%** | 6.064% | **487.5 s** |

The wall-time column is new this session — first time either corpus has a
recorded wall-clock figure — from the `max_splits` sweep's full-corpus
control reproduction (`docs/measurements/2026-09-24_max_splits_sweep.txt`).
It is now the baseline against which the italic-faces +67% regression
(below) and any future performance change is measured.

**Checkbox detection needed two attempts; only the second shipped.** A
bounding-box-plus-fill-ratio detector deleted hollow letters and digits
wholesale (`o e a 0 6 8 9`, the counters of `D O Q P R B` — CER roughly
doubled at every threshold tried). Replaced with per-side border-ink
coverage (`Component::border_coverage`, gated on all four sides clearing
0.85). Full-corpus gates passed, folded into the controls above.

**`r000583` is still the worst known `finfilings` page (40.633% CER as of
the letter autopsy), and two mechanisms have now been ruled out on it in
one session:** `segment.max_splits` does not bind (byte-identical output
across a 3–8 sweep), and cut-candidate generation is not the failure
either (a full per-atom trace found it correctly rejecting what it
rejects). A per-letter stage autopsy instead traced the loss to
match/decoder-stage scoring on **italic** text with no italic prototypes
in the bank — 10 of 21 lost letters at lattice/atoms, 6 at match, 4 at
decoder, 0 upstream.

**Italic faces were tried, worked on the target, broke an unrelated
population, and were reverted.** 22 licence-cleared Italic/BoldItalic
faces added to the shared bank fixed `r000583` (40.633%→23.077%) and
passed `finfilings` corpus-wide, but **failed `pages-cov`'s own gate**
(6.064%→6.202%) — the drawing/CAD category, upright by construction,
regressed +0.136 against its Δ≤0 gate — because pooling lets italic
prototypes compete on raw distance against upright glyphs they merely
resemble. Wall time rose +67%. Reverted same session; only the
measurement file was kept.

**Check the binary against the source you changed, not against the
clock** — unchanged advice from before, still true.

---

## 2. Start here — next-up queue, in order

1. **Slant-gated italic prototypes — in progress, finish and gate it.**
   Per-word slant estimation (shear + vertical-projection-variance score
   against 0°, `layout.slant_min_deg` guess 6°, `layout.slant_margin`
   guess ratio) plus match-time eligibility gating on each prototype's
   source-face style. A background dispatch on `params.tsv`/estimator
   wiring was underway as of this filing — check its state before
   restarting it. Gate on both corpora before shipping; watch specifically
   for the drawing/CAD category, the one that broke under blind pooling.
2. **The deslant arm.** Structural per-line deslant against the
   upright-only bank, held as the comparison point for the gating
   approach. Not yet run. Needs no bank growth but is a bigger pipeline
   change than gating.
3. **`char_bonus` re-sweep.** The letter autopsy found some decoder losses
   need a bonus above the current ~3.44 (back-solved) to flip — from only
   2 sample cases on one page, so this is a sweep to run, not a value to
   adopt on faith.
4. **`r000022`'s touching-digits mechanism.** Named in the cut-candidates
   trace as a distinct fusion pattern from `r000583`'s; not yet autopsied.
5. **`r000396`'s smaller checkbox/column-cut residue.** Named during this
   session's diagnosis, well under 10% of that page's characters — not
   urgent, recorded so it isn't lost.
6. **Re-measure head-to-head vs `ocrs`** on `bench/pages-cov`. Stale; this
   comparison is the project's reason to exist.
7. **Re-run SROIE** against the current reading-order, line-merge and
   checkbox fixes.
8. **A ligature error-share count** on the bold bank. Prevalence known
   (24/60 `finfilings` pages carry the ligature-forming serif family and
   an fi/fl word), error count not taken.
9. **`filing__r000022` dense-table trace** — a distinct, earlier-named
   issue from item 4's touching-digits mechanism on the same page; 9% of
   an earlier six-page deletion sample, inferred from confusion pattern,
   not yet pixel-verified.
10. **`baseline_split_sep`/`baseline_split_support` sweeps.** A *different*
    pair of constants from `baseline_split_valley_margin` — the
    two-baseline-population thresholds from an earlier merged-line fix,
    still `guess` provenance, unswept.
11. **Re-measure `lines.rule_aspect`.** Owed since 2026-09-22.
12. **Recognition-gated chopping** (Tesseract-style: chop only the
    least-confident atom, undo non-improving chops). Not measured,
    research only.
13. **The column-cut lone-guard per-page diff.** Still not run; both
    leader-line-derived rules failed their real-filings gate and neither
    shipped; `lines.column_lone_guard` stays 0.
14. **ALTO/hOCR underline-formatting output**, from the `RuleSegment` data
    the underline strip records but nothing yet consumes. Direction from
    Ken; not scheduled as a chunk until he says so.
15. **A one-time `rustfmt` pass.** 63 files drift against no committed
    `rustfmt.toml`; **pending Ken's call** — see section 5.
16. DejaVu Serif: withdrawn (2026-09-23, architect). No record shows the
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
`D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings` (~17 min to run, measured
this session: 1019.5 s); `bench/pages-cov` (~8 min, measured: 487.5 s).
**Gates:** beat control on both `finfilings` CERs (now 12.708 / 11.602);
`pages-cov` no worse than +0.05 (now baselined at 6.064). Any change
touching the prototype bank or match stage should also compare wall time
against these two figures — the italic-faces experiment is the concrete
example of a change that passed accuracy-only review and would have
shipped a 67% slowdown unnoticed.

**Working mode:** the `/loop` "continue working on features and research
OCR techniques" prompt is what drives this session's shape — a fix
followed by research followed by the next fix it surfaces, each one gated
and shipped or explicitly rejected before moving on. **Commit after every
passing change, locally, never push.** Replies to Ken are TL;DR — the
detail belongs in these docs, not the chat reply.

---

## 3. Where things landed this session

**Checkbox detection shipped on the second attempt.** See section 1 above
and `docs/ARCHITECTURE.md` §11's two 2026-09-23/2026-09-24 checkbox
entries; readings in `docs/measurements/2026-09-23_checkbox_drop.txt`.

**Two candidate mechanisms for `r000583` were measured and ruled out in
the same session** (`segment.max_splits`, cut-candidate generation),
redirecting the method to an exhaustive per-letter stage autopsy rather
than a third guess. Full narrative: `docs/ARCHITECTURE.md` §11; readings
in `docs/measurements/2026-09-24_max_splits_sweep.txt`,
`_cut_candidates_r000583.md`, `_letter_autopsy_r000583.md`.

**Italic prototypes were measured and reverted.** Full narrative:
`docs/ARCHITECTURE.md` §11, "Italic faces: huge win on italic, broad loss
on upright; gate them by measured slant"; readings in
`docs/measurements/2026-09-24_italic_faces.txt`.

Four new `personal_rag/ocr` lessons from this session (checkbox
border-coverage over bbox+fill, measure-a-cap-before-tuning-it, the
per-letter stage-autopsy method, pooled-style-prototypes-need-a-
competition-gate) — see `C:\personal_rag\ocr\index.md`.

---

## 4. Constraints that bind the next session

* **Charset, feature vector and normalisation are frozen.** Unchanged.
* **`ARCHITECTURE.md` section 11 is append-only.** Supersede with a new
  entry and a forward pointer; the old text stays.
* **A projection is labelled a projection; a reading is labelled a
  reading.** Every CER/F1/wall-time figure in this file traces to a
  numbered `docs/measurements/2026-09-2[3-4]_*` file. The italic-is-the-
  unifying-cause finding from the letter autopsy is explicitly a reading
  from one page's sample, not yet confirmed cross-page.
* **Blessing a fixture is a deliberate, reviewed act.** No fixture was
  reblessed this session.
* **Nothing downloaded from the web enters the repository.** Corpora live
  in `D:/Dev/ExcludedPrivate/ocrcer`.
* **Compare model variants only at an identical size ladder.** Unchanged.
* **Gate any cut-search width, margin, or prototype-bank change on both
  corpora, not just the one that motivated it — and on wall time, not
  just accuracy.** The italic-faces experiment is this session's concrete
  case: it would have shipped a 6-of-7-category regression and a 67%
  slowdown if only the target corpus had been checked.
* **Commit after each passing change**, now that the tree is under git —
  see the header of this file and `ROADMAP.md`'s Standing rules.

---

## 5. Waiting on Ken

* **A one-time `rustfmt` pass** across 63 drifted files (no `rustfmt.toml`
  committed). Not run, pending his call.
* **Does "commit after each passing change" extend to Ken's other project
  trees, or is it scoped to `D:\Dev\OCRcer`?** Unresolved, carried forward.
* **ALTO/hOCR underline-formatting output**, direction given, not yet
  scheduled as a chunk.
* **The RAG rename sweep** — ~247 files in `C:\personal_rag`, ~437 in
  `D:\dev\rag` still say `pdfce` (carried forward, unchanged).
* **`osifont`'s GPL font exception** — unresolved; the face is not in the
  bank (carried forward, unchanged).
* **Disk space — resolved, no longer waiting.** Reported 2026-09-23 as
  273 GB free on D:; not independently re-verified since (no shell in
  this or the prior filing dispatch), margin wide enough this stays closed
  unless a future session finds otherwise.
* **Clippy: 45 warnings** (top lint `needless_range_loop`, 13 occurrences)
  — reported, report-only, no action requested yet.
* **Token spend against `/usage`** — unmeasured for seven sessions running
  now; no shell has been available to any of the librarian dispatches
  since the calibration debt was first noted. Flagging this explicitly:
  the next session with shell access should treat reading `/usage` as a
  priority action, not a background one.
