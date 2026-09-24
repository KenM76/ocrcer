# OCRcer — roadmap

Chunk-numbered, mirroring `PLAN.md` section 2. Owned by `ocrcer-librarian`
after bootstrap. A chunk moves to Shipped only when its exit gate passes, and
the measured number that passed it is recorded here.

---

## Shipped

### Chunk 0 — Bootstrap (2026-09-18)

Feasibility assessed, architecture designed, build staged, agent roster
written.

**Exit gate:** design documents exist and the roster loads. Passed.

**Findings that shaped everything downstream:**

- The model is constructed, not fitted. Every parameter is either authored from
  knowledge or computed by a deterministic script, which is what makes an
  authored model possible at all and what chose the architecture.
  `FEASIBILITY.md` section 1 has the reasoning; `CLAUDE.md` rule 1 makes it
  enforceable.
- The engine being replaced is `ocrs`, whose models are CC-BY-SA-4.0 and which
  reports no confidence at all. Those two facts, plus the untargeted CAD and
  office-document domain, are the justification for building rather than
  adopting.
- Building the model is a minutes-long script run, so nothing in the plan waits
  on a machine and the tuning loop is genuinely iterable. This is the basis of
  the schedule in `PLAN.md` section 3.
- Everything is Rust in one workspace, and every pipeline stage is written
  exactly once. The decisive reason is that the feature extractor runs both
  when the bank is built and when a glyph is recognised; two implementations
  could drift apart silently and invalidate the entire bank. Correctness is
  carried by golden fixtures instead of by cross-language comparison.
- Degraded-scan accuracy is the accepted weak spot, projected at 85–93%.
  Classification is deliberately isolated so it remains substitutable if that
  projection proves unacceptable.

**Measured environment (2026-09-18):** i9-10900KF 10C/20T, Intel Arc Pro B50,
15.9 GB RAM, 119 GB free on D:, 529 fonts in `C:\Windows\Fonts`. Only the CPU
and the font inventory are load-bearing.

**Token spend:** to be recorded. This is the first data point for the budget
model, which is currently a projection.

---

### Chunk 1 — Workspace, charset, feature extractor, fixture harness (2026-09-18)

Owners `ocrcer-architect` (charset and feature definition) and `ocrcer-glyphs`
(the extractor).

**Exit gate, clause by clause, each verified directly by the coordinator
running the command, not reported by an implementer:**

- Three crates build: `cargo build --workspace` clean.
- `ocrcer-core` compiles for `wasm32-unknown-unknown`. Verified directly.
- `ocrcer-core` has zero dependencies: `cargo tree -p ocrcer-core` shows the
  crate alone.
- The charset is frozen in a single declared source, `model/charset.tsv`: 187
  classes, indices contiguous 0–186.
- The extractor produces 107-dimension vectors matching `ARCHITECTURE.md`
  section 3.
- No transcendental functions anywhere in the extractor or the component
  labeller — grepped for atan2/hypot/powf/powi/exp/sin/cos/tan/log/ln. This is
  what keeps exact fixture comparison portable between x86 and wasm32.
- Tests: 24 in `ocrcer-core`; 11 unit plus 9 perturbation integration tests in
  `ocrcer-bench`. All pass.
- The fixture runner reports 9/9 against the seed glyph set.
- **The clause that mattered — the harness fails loudly when a fixture is
  deliberately altered.** The coordinator perturbed `glyph_8`'s hole-count
  dimension from 2 to 1 (a semantic change, not a one-ULP synthetic) and got,
  verbatim:
  `FAIL glyph_8 (stage=feature): value mismatch at index 96 of 107 — expected 1 (bits=0x3f800000), actual 2 (bits=0x40000000)`
  with `8/9 fixtures passed` and exit code 1. `cargo test --workspace` also
  went red, on a test named
  `checked_in_fixtures_are_untouched_by_the_suite_above` — tampering is caught
  by the ordinary test suite as well as by the runner. That second detection
  was not asked for and is worth recording.

**Findings:**

- Connected-component labelling was pulled forward from chunk 2: the
  extractor's hole-count dimension needs it, and a second implementation
  appearing later would violate the write-once rule. It is written as the
  general labeller the whole project will use.
- The seed fixtures were rebuilt once. The first version padded every glyph
  bitmap to a uniform height and varied x-height per glyph. Both are wrong:
  bounding boxes from the component labeller are tight, and x-height is a
  property of the line, not the glyph. The padding moved the centroid, the
  normalisation scale, the aspect ratio, the ink fraction and both projection
  profiles, and put every glyph's box top at the same height so the
  baseline-relative dimensions could no longer separate a period from an 'o'.
  The seed set is now nine glyphs from one declared line metric: x-height
  14px, cap/ascender 20px, descender depth 5px.
- A projection that has not been measured is still labelled a projection.
  Nothing in `ARCHITECTURE.md` section 9 has been measured yet.

**Token spend:** not yet measured against `/usage`. The budget calibration
run specified for this chunk has not been done — outstanding, not invented
here.

---

## In progress

### Chunk 3 — Prototype bank construction and the `.ocrw` writer

Owner this session: `ocrcer-glyphs`. Exit gate not yet met — no report this
session of bank build time/byte-identity, 1-NN accuracy on isolated
rendered glyphs, round-trip, or int8 top-1 agreement; see `PLAN.md` section
2 for the full gate, not restated here.

**Measured 2026-09-21** (full account in `SESSION_LOG.md`'s 2026-09-21
entry; decision narrative in `ARCHITECTURE.md` section 11, the three
2026-09-21 entries): the minimum-resolution floor is **21px per em**,
established by an exhaustive 17,391-pair sweep, no collisions 21–40px (two
collisions below it: `i`/`ï`, `.`/`…`). `§` redrawn and re-measured as a
partial win — `§`/`8` minimum distance 0.37192 → 0.53969, still on the
confusion-candidate list. Four charset ceilings widened by one derived rule
(`<` `>` to 0.84, `Ω` to 0.97, `™` to 1.06), zero new collisions, all 187
glyphs now in band. `model/confusion_candidates.tsv` shipped, 82 rows —
input evidence for chunk 4's confusion table, not the table itself.

**Token spend:** not measured. The `/usage` calibration owed since chunk 1
was explicitly waived by the operator this session rather than run.
Outstanding.

Chunk 2's status was not touched this session and is not restated here.

---

### Chunk 9 — Accounting/business document structure (word-space rule, in progress)

Owner this session: `ocrcer-runtime`. Exit gate not yet met — this is
progress on the word/line layout stage chunk 9 depends on (`layout/words.rs`,
`layout/lines.rs`), not the chunk's own ledger/statement/boxed-form fixture
gate; see `PLAN.md` section 2a for that gate, not restated here.

**Measured 2026-09-22** (full narrative in `ARCHITECTURE.md` section 11, five
dated entries starting "Band-pooled space rules measured …"; readings in
`docs/measurements/2026-09-22_band_pooled_spaces.txt`,
`2026-09-22_fixed_pitch_spaces.txt`, `2026-09-22_fallback_full_corpus.txt`):
the column-cut precision collapse from an earlier session (`RESUME.md`
section 3) was traced to spurious spaces inside short numeric fragments,
not a mis-measured x-height. A band-pooled space rule was tried first and
**did not meet its own gate** — a third of the spurious insertions removed
(727→503 on invoice) but nearly as many real spaces newly lost (377→498);
word F1 net flat (65.388%→65.234%). Shipped anyway, provisionally: neutral
on F1, better on CER/WER, reverting buys nothing.

Fixed-pitch (monospace) detection — chop by cell position instead of gap
width on lines that test as fixed-pitch, after Tesseract's `textord`
(`docs/measurements/2026-09-22_research_classical_techniques.md` §1) — went
through four formulations in one session, each measured and each either
shipped or explicitly rejected on a failed gate, never tuned around
silently:

1. Plain pitch test (median centre-distance agreement): passed its own gate
   (invoice F1 65.234%→67.394%) but regressed the `statement` family at
   `column_gap_heights=0` (F1 68.278%→67.594%) — an all-caps proportional
   title (`ACCOUNT STATEMENT`) has near-uniform glyph advances and its one
   real word gap reads as sub-grid.
2. Cell-merge + all-or-nothing grid-consistency check: **fixed the
   regression above but failed its own gate badly** — monospace recall
   88.3%→62.8%, because one off-centre glyph (`.`, `-`, `1`) corrupts two
   neighbour-distances at once and a short line has few wide distances to
   vote with.
3. A vote excusing "touching runs": partial recovery only (recall 66.6%,
   still below the 88.3% gate). **Failed.**
4. A fitted grid (least-squares `x = x0 + n·p`, test residuals not pairwise
   distances): recall recovered (90.6%, passed) but proportional false
   positives rose to 2.48%–8.55% against a 0.5% gate — a least-squares fit
   *absorbs* a single anomalous gap into its slope/intercept rather than
   being thrown off by it, the opposite of the premise it was specified
   under. **Failed**, and the premise was wrong, recorded as this log's own
   error, not the implementation's.

**Shipped fallback, pre-declared before formulation 4 was even tried, so it
was not re-litigated on the day**: cell-merge kept, `pitch_grid_check`
defaults to **0** in `Params::DEFAULT`/`model/params.tsv` — the median
agreement test with cell merge and a `1.5p` empty-cell split. Reproduces the
"merge only" ablation row: monospace recall 94.8% at `column_gap_heights=0`,
85.6% at 2.5; proportional false positives 1.62% and 1.80%.

**Accepted, recorded loss:** all-caps proportional runs of uniform glyph
advance still pass the fixed-pitch test and their one real word gap fuses —
measured on `ACCOUNT STATEMENT`, **29 of 55 font/size pairs** across the 11
proportional font families in `bench/pages-cov`. `statement` family word F1
at `column_gap_heights=0` reads 68.078% against 68.278% with pitch off — a
0.2-point loss, held over the whole corpus, in exchange for the monospace
recall the feature exists to buy. No fourth formulation was attempted this
cycle, per the bounded-fallback decision recorded in `ARCHITECTURE.md`.

**Full-corpus comparison of the shipped config (fallback) against pitch off,
all 625 pages, both `column_gap_heights` arms — interrupted, not
complete.** `docs/measurements/2026-09-22_fallback_full_corpus.txt` records
only arm 1 of 4 (`column_gap_heights=0`, shipped fallback: CER 6.698%, word
F1 76.200%, 1353 deletions/275 insertions). Arm 2 (`column_gap_heights=0`,
pitch off) was in flight when Claude Code reaped the background shell for
low system memory — not a fault of the run, and **not restarted pending
Ken**. Arms 2–4 (cg=0 pitch-off, cg=2.5 pitch-on, cg=2.5 pitch-off) remain
unmeasured. This is the next concrete step, ahead of the older items in
`RESUME.md` §2.

**Token spend:** not measured against `/usage` this session either — the
calibration debt from chunks 1 and 3 is carried forward again, not
invented.

**Measured 2026-09-23** (full narrative: `ARCHITECTURE.md` §11, ten dated
entries starting "A column fragment must hold two components …"; evidence in
`docs/measurements/2026-09-23_*`). Four separate threads, each gated and
each reading labelled as a reading against the corpora `bench/pages-cov`
(625 synthetic pages) and `finfilings` (60 real financial-filing pages):

- **The lone-glyph column-cut guard shipped OFF, after two failed rules.**
  A column-cut rule requiring both sides of a candidate cut to hold ≥2
  components passed on `pages-cov` (F1 76.675%→76.677%) but failed on
  `finfilings` (F1 68.277%→**66.807%**, −1.47, against a 0.2 gate) —
  inferred cause: a filing's `$`-in-its-own-cell table layout, not
  represented in the synthetic corpus. Narrowed to "drop a candidate only
  when *both* neighbouring fragments are single components" — still failed
  (`finfilings` F1 68.277%→67.865%, −0.41; CER 19.544%→19.610%). Both rules
  were reasoned from leader-line fixtures alone, never observed on a real
  page before being proposed twice. **Shipped: `lines.column_lone_guard`
  toggle, default 0** (greedy cut at the measured 1.75-height threshold,
  the 2026-09-22 behaviour), provenance `guess`. Five unit fixtures
  encoding "a leader line is not diced" still run with the guard on,
  because that per-fixture behaviour is still intended — what is not yet
  known is whether dicing leaders costs anything measurable on a real page.
  Not yet done: the per-page diff between greedy and guarded cuts that
  would show *which* lines move, planned before a third rule is proposed.
- **Bold weight enters the bank, and a same-day correction is on record.**
  First reading (32 faces, 23,740 prototypes) compared a four-size candidate
  bank (16/24/32/48 px/em) against the shipped five-size control
  (16/20/24/32/48) — an unintended confound, since the size ladder is a
  build-time argument `inspect` did not print. Re-measured like-for-like at
  the five-size ladder: **32 faces, 29,675 prototypes, int8 top-1 agreement
  99.486%, 3.15 MB.** `bench/pages-cov` (contains no bold): F1 76.675%→
  **77.392%**, CER 6.503%→**6.127%**. `finfilings` (58/60 pages carry bold):
  F1 68.277%→**70.846%**, CER 19.544%→**18.546%**. **Shipped**, both gates
  pass at the corrected ladder. `ocrcer-exporter` now records the build size
  ladder in `meta` and `inspect` prints it, additive, no format bump — so
  this confound cannot recur silently.
- **A line-matched-CER metric bug in `ocrcer-bench` was fixed the same
  session.** Line-matched CER (a reading-order-independent measure,
  Clausner/Pletschacher/Antonacopoulos 2020, pairing each truth line with
  its best-matching OCR line before charging edits) was reading *higher*
  than end-to-end CER on both corpora — an oddity, since a reading-order-
  independent score should not normally exceed the order-sensitive one.
  Fixed in `cer.rs` between the 4-size and 5-size bold readings; figures
  from before the fix are explicitly marked unquotable in
  `docs/measurements/2026-09-23_bold_faces.txt` and are not repeated here.
- **Line-merge diagnosis and ship.** `ocrcer-bench`'s audit of the six
  worst clean `finfilings` pages attributed **82% of their deletions** to
  one mechanism: two tightly-leaded real text lines banded as one,
  `overlap_fraction` alone admitting the join (0.90–1.00 overlap ratio,
  the same range a legitimate same-line ascender join achieves — the ratio
  cannot separate the two cases; tightening it either changes nothing or
  breaks real pages). The merged band's degenerate x-height (clamped to
  1 px against a real 13 px cap) was still labelled `Observed`, which
  defeats the existing `inherit_x_heights` safety net by the net's own
  design contract. Two independent rules specified: **A**, a plausibility
  floor (`main ≥ x_height_floor_per_cap × cap`, measured 0.3168 from the
  shipped bank's font metrics, provenance `measured`) that stops the
  mislabelling; **B**, a baseline-bimodality split (two-baseline
  population test, `baseline_split_sep` 0.6 and `baseline_split_support`
  0.25, both `guess`) that separates the physically interleaved lines.
  **A alone failed its own gate** — `finfilings` F1 rose (+1.97) but CER
  *worsened* (+0.46): more text decoded, still in the wrong order. **A+B
  passed every gate**: `pages-cov` unchanged (F1 77.392%, CER 6.127%,
  synthetic corpus has open leading); `finfilings` CER 18.546%→**17.064%**,
  F1 70.846%→**73.615%**. **Shipped as A+B**; `lines.baseline_split` is 1,
  provenance `measured`; its two constants stay `guess`, unswept, on the
  chunk-8 tuning list.

**Open, carried forward from 2026-09-23:** the `filing__r000022` dense-table
trace (9% of the sampled deletions, a different segmentation sub-mechanism —
narrow-column/reading-order collapse, not the vertical line-merge above —
inferred from confusion pattern, not yet pixel-verified; classical-technique
research proposes pixel-level morphological rule removal as the untested
candidate fix, see the `personal_rag/ocr` lesson); italic, queued behind the
merge fix and now carrying fresh page-level evidence (`filing__r000583`,
~54% CER loss attributed to font-style mismatch with segmentation confirmed
clean, not yet a corpus-wide share); a ligature-attributable error count on
the rebuilt bold bank (24/60 finfilings pages, 40%, set true fi/fl ligatures
in the serif family — prevalence measured, error share not yet taken); the
lone-guard per-page diff; `baseline_split_sep`/`baseline_split_support`
remain unswept guesses; `lines.rule_aspect` still needs the re-measurement
flagged since 2026-09-22 (predates the word-valley floor); the `ocrs`
head-to-head is still stale; SROIE is still unre-run against the current
reading-order fix; and a **DejaVu Serif / Bitstream Vera licence question
for the operator** was named in this session's brief but is **not
independently found** in `ARCHITECTURE.md` §11's 2026-09-23 entries or in
today's `docs/measurements/` files by this filing — the repo's only
DejaVu-family entry is DejaVu **Sans** (Bitstream Vera, already `local-only`
per the 2026-09-21 resolution above). Flagged rather than silently folded
into that existing item; needs the operator or the next session to confirm
whether a *new* DejaVu Serif face is actually in question.

**Token spend:** not measured against `/usage` this session; the
calibration debt is carried forward again.

**Measured 2026-09-23, continued — split gate, underline strip shipped,
first git commits.** Full narrative: `ARCHITECTURE.md` §11, the six entries
from "Underline strip fails the real-filings gate" through "Underline strip
ships (rule 2 with part 3b)"; evidence in
`docs/measurements/2026-09-23_underline_strip*.txt`,
`_underline_strip_damage.md`, `_underline_r000055_and_touching_r000022.md`,
`_split_gate_and_strip_3b.txt`.

- **Two mechanisms diagnosed on the underline-strip regression.** Stripping
  a bordered box's rules correctly erased the furniture but left the box's
  sides behind as 10–22×h slivers that dodged both the width-based
  furniture filter and the height-based debris filter; tallest-first line
  grouping seeded a band on a sliver and fused two real prose lines (89→69
  lines on one page). Fixed by a joint width-and-height debris gate on
  strip-produced pieces (`lines.thin_debris_heights`, measured base value
  3.4288, ×1.5 headroom a labelled guess). Separately, the `0$` touching-atom
  case on `filing__r000022` turned out to be a threshold problem, not a
  cutting-technique problem — the valley existed but the search gate never
  ran at that width. Lesson filed:
  `personal_rag/ocr/lesson_20260923_strip_debris_thin_slivers_need_width_and_height_gate.md`.
- **Split gate `segment.split_min_x_heights` 1.15 → 1.09, measured.**
  Swept {1.0, 0.85, 1.09} against both corpora. 1.0 won biggest on
  `finfilings` (CER 17.064%→16.885%) but **failed pages-cov** (CER
  6.127%→6.330%, over-segmentation on clean text, against a 0.05 gate).
  **1.09 passes both**: `finfilings` CER 17.064%→**16.932%** (line-matched
  16.942%→16.843%, F1 73.615%→74.047%); `pages-cov` CER 6.127%→**6.089%**
  (F1 77.392%→77.429%). 1.09 rather than the more natural 1.10 because
  `1.10f32` widens to `1.1000000238` in `f64`, which would still reject the
  11px `0$` atom under the strict `<` gate. Two lessons filed:
  `personal_rag/ocr/lesson_20260923_f32_to_f64_widening_moves_threshold_boundaries.md`,
  `personal_rag/ocr/lesson_20260923_gate_a_cut_search_width_change_on_both_corpora.md`.
  Recognition-gated chopping (Tesseract chops only the least-confident blob
  and undoes non-improving chops) is recorded as the next candidate if a
  plain width gate stops passing both corpora — **not measured**, research
  only (`docs/measurements/2026-09-22_research_classical_techniques.md`
  addendum; lesson
  `personal_rag/ocr/lesson_20260923_tesseract_chops_only_low_confidence_blobs_and_undoes_non_improving_ones.md`).
- **Underline strip ships (rule 2 with part 3b), measured at the 1.09 split
  gate.** `finfilings` CER 16.932%→**16.756%** (line-matched
  16.843%→16.634%, F1 74.047%→**74.405%**); `pages-cov` unchanged
  (6.089%/77.429%). `lines.underline_strip` = 1, provenance `measured`.
  Erased bands are also now recorded as `RuleSegment`s in the layout output
  — not yet surfaced in any output format, but this is what the ALTO/hOCR
  underline-preservation direction (raised by Ken, recorded in
  `ARCHITECTURE.md` §11, not yet scheduled) would consume.
- **New controls for later gates:** `finfilings` F1 **74.405**, CER
  **16.756**, line-matched **16.634**; `pages-cov` F1 **77.429**, CER
  **6.089**.
- **Underline-strip code moved out of `lines.rs` into
  `layout/underline.rs`, a pure move** (no behaviour change). `cargo test
  --workspace --release`: **304 tests, 0 failed.**
- **Build report bug fixed:** the params census in the build report had
  measured and authored figures swapped. Fixed same session. Any earlier
  log or report quoting that census before the fix carries the swapped
  figures — not retroactively corrected here.
- **`r000583` is now the worst `finfilings` page at 54.66% CER**, having
  been overtaken by the fixes above; not examined this round.

**Code-health snapshot, 2026-09-23 (reported this session; not
independently re-run by `ocrcer-librarian` — no shell available in this
dispatch to verify clippy/rustfmt output directly):** 45 `clippy` warnings
(top single lint: `needless_range_loop`, 13 occurrences); `rustfmt` drift in
63 files (no `rustfmt.toml` committed, so there is no pinned style to drift
from yet); 1 `unwrap` and 11 `expect` in `ocrcer-core`, all reported as
guarding internal invariants rather than untrusted input. **A one-time
`rustfmt` pass is pending Ken's decision** — see *Open questions for the
operator* below.

**Disk-pressure incident, reported this session, not independently
re-verified:** D: hit 100% free space mid-session; an agent deleted two
untracked scratch files (`aspect_err.txt`, `aspect_out.txt`) under that
pressure, and stale `target/` build directories were cleaned. 38 GB free is
reported as the result — this figure is **unverified from here** (no shell
available in this dispatch to run a disk-space check); the next session
with a shell should confirm before relying on it as free headroom.

**First git commits, approved by Ken 2026-09-23.** The repository was
untracked through 2026-09-22 (per the earlier `RESUME.md`, "zero commits,
no remote, every file untracked"); five commits now exist on `master`:
`4c85f69` initial snapshot, `9436f50` LF line-ending pin + binary fixture
marking, `a8a24be` split-gate ship, `f9dcd8f` underline-strip code move,
`3772a41` underline-strip ship + build-report fix. `.gitattributes` pins
`* text=auto eol=lf` (plus explicit `binary` for `.pbm`/`.png`/`.ocrw`)
specifically because fixtures are compared byte-for-byte and a
checkout-time CRLF rewrite would change their hashes. **Standing rule from
here forward: commit after each passing change** — see *Standing rules*
below.

**Queued, carried forward:** `filing__r000022`'s dense-table trace (9% of
sampled deletions, still inferred not pixel-verified); italic, still queued,
with `filing__r000583` now the concrete worst-page evidence at 54.66% CER;
a ligature error-share count on the bold bank; the lone-guard per-page
diff; `baseline_split_sep`/`support` unswept; `lines.rule_aspect`
re-measurement; the `ocrs` head-to-head still stale; SROIE not re-run;
recognition-gated chopping (above, unmeasured); ALTO/hOCR underline
formatting output from the new `RuleSegment` data (awaiting Ken's go to
schedule it as a chunk); DejaVu Serif still withdrawn per the architect's
2026-09-23 note below.

**Token spend:** not measured against `/usage` this continuation either;
the calibration debt is carried forward again, now across four sessions.

**Measured 2026-09-23, continued further — worst-page diagnosis, atom
merge by overlap fraction ships at 0.4.** Full narrative:
`ARCHITECTURE.md` §11, the three entries "Worst page named" through
`segment.merge_overlap_frac` 0.3 → 0.4; evidence in
`docs/measurements/2026-09-23_worst_page_r000583.md`,
`_atom_merge_overlap.txt`.

- **Root cause of the worst page, pinned before any fix was written.**
  `filing__r000583` (54.66% CER) was diagnosed: `atoms()` merged any two
  components whose x-ranges overlapped at all, a rule meant for an `i` and
  its dot. In this serif face a `t` crossbar and an `h` base serif overlap
  by 1–6 columns at different heights without the ink ever touching, and
  the page had 73 atoms chaining 3+ letters (up to 7) against 3 on an
  ordinary page. Once chained, `max_splits = 3` cannot carve a 7-letter
  atom apart; the underline strip changes nothing on this page.
- **Shipped: `segment.merge_overlap_frac`, default 0.4, provenance
  measured.** Merge two components only when one sits fully inside the
  other's column range, or the overlap is ≥ `merge_overlap_frac` × the
  narrower one's width (0 reproduces the old any-overlap rule exactly).
  Pieces are now cropped by atom membership (`edge_labels()`), not by
  column, so a separated `t` no longer picks up the neighbouring `h`'s
  serif. Shipped in two steps, both measured, neither guessed past a
  one-page screen: **0.3** first (`r000583` 54.661%→40.000%; `finfilings`
  end-to-end CER 16.756%→**16.113%**, line-matched 16.634%→**16.068%**;
  `pages-cov` CER 6.089%→**6.057%**, all three gates passed), then the full
  `finfilings` corpus swept at {0.15, 0.2, 0.3, 0.4} found **0.4 better on
  both `finfilings` CERs** (end-to-end **16.089%**, line-matched
  **15.910%**; `pages-cov` unchanged at 6.057%). The architect re-ran 0.4
  independently and reproduced it to the digit.
- **A suspicious row, flagged rather than silently repeated.** In the sweep
  table, 0.2's word-level (WER/F1) figures read identical to 0.4's — read
  as a likely transcription slip on the rejected 0.2 line, not a real
  coincidence; it affects only a value that was not shipped, and is not
  corrected retroactively here.
- **New controls for later gates:** `finfilings` end-to-end CER **16.089**,
  line-matched CER **15.910**; `pages-cov` CER **6.057**, F1 **77.540**
  (F1 as reported this session; not independently re-run by this filing).
  This started the session at 16.756 / 16.634 / 6.089.
- **Queued behind this, unchanged in kind, sharper in evidence:** drop-fall
  or other non-vertical (contour-following) cuts and a width-scaled
  `max_splits`, for touching ink specifically — `r000583` did not have
  touching ink, so this fix did not need them, but the research note on
  drop-fall cuts (`docs/measurements/2026-09-22_research_classical_techniques.md`,
  appended this session) stays queued for the touching-ink cases still on
  the worst-pages list.

**Disk, reported this session, not independently re-verified (no shell in
this dispatch):** D: is at **99% full, ~10 GB free**. The architect deleted
`target/debug`, `runtime-diag`, `glyphs-agent`, `wasm32` and `tmp` build
directories (~3 GB reclaimed) under this pressure; only `target/release`
remains. This supersedes the prior session's "38 GB free" figure, which
was itself unverified — read this as the latest unverified report, not a
confirmed measurement. **Any diagnostic agent invoking a build with its own
`CARGO_TARGET_DIR` (runtime-diag, glyphs-agent, wasm32) will rebuild from
scratch next use**, since its target directory was among those removed.
Keep build directories minimal going forward; a full disk on Windows can
present as a link error rather than an out-of-space error (see the Rust
ecosystem RAG entry filed this session,
`D:\dev\rag\rust\cargo_target_dir_debug_deps_grows_without_bound_and_fills_the_disk_195gb_test_binaries.md`,
dated footer).

**Open, carried forward from this continuation:** everything already
carried forward above (dense-table trace, italic, ligature share, lone-guard
diff, `baseline_split_sep`/`support`, `rule_aspect` re-measurement, `ocrs`
head-to-head, SROIE, recognition-gated chopping research, ALTO/hOCR output,
`rustfmt` pass, `/usage` calibration — now five sessions), plus, new this
filing: **re-diagnosis of the new worst pages** — `r000583` is still the
worst `finfilings` page at 40.00% CER even after the fix, and `r000022`,
`r000055` and `r000044` were reading 34–37% before this fix, so all four
need re-measurement against the new 0.4 control before the next rule is
proposed; and optionally running **0.35/0.45/0.5** on the full `finfilings`
corpus (small expected gain, not yet run — only 0.15/0.2/0.3/0.4 were
swept full-corpus, 0.5/0.7 only on the one-page screen).

**Token spend:** not measured against `/usage` this continuation either;
the calibration debt is carried forward again, now across five sessions.

**Measured 2026-09-23, continued once more — worst pages round 2
diagnosed, line-fusion fix ships, largest single gain on the real-filings
corpus to date.** Full narrative: `ARCHITECTURE.md` §11, the two entries
"Worst pages, round 2" and "Line fusion fix:
`lines.baseline_split_valley_margin` 0.3, measured"; evidence in
`docs/measurements/2026-09-23_worst_pages_round2.md`,
`_line_fusion_fix.txt`.

- **Worst-pages round 2, diagnostic only, no default changed.** Re-diagnosed
  the three worst `finfilings` pages at the `merge_overlap_frac=0.4`
  control (finfilings 16.089%/15.910%, pages-cov 6.057%). `r000583`
  (40.63% CER) is the same serif-bbox-chaining mechanism as before, worked
  down from 54.66% by the prior fix but not resolved — its tightest chains,
  where one component's ink genuinely sits mostly inside the other's column
  range, clear the "always merges" branch a fraction threshold cannot gate,
  and a fixed 3-cut budget with vertical-only cuts cannot separate a
  7-letter chain. `r000308` (34.45%) and `r000363` (32.27%) are a
  **different, previously undocumented mechanism**: `group_with_bands`
  fuses pairs of ordinary, cleanly-separated body-text lines into one
  x-interleaved band on this document's tight leading (~1.7 x-heights,
  against `r000583`'s ~2.0–2.2). Reading order ruled out on all three pages
  (line-matched ≈ end-to-end). This mechanism survives the existing
  `lines.baseline_split=1` two-baseline split post-pass; the diagnosis
  named finding out *why the split pass doesn't fire* as the next step,
  preferring a fix to the split pass over a new rule. A smaller checkbox/
  form-field column-cut fragmentation was also named on `r000308` (well
  under 10% of that page's characters, not the driver).
- **Root cause of the split-pass miss, found and fixed the same session.**
  `split_point`'s valley test excluded a fixed 2-pixel-row margin around
  each candidate baseline peak — not scaled to type size. At body sizes a
  line's own descenders (`g p q y j`) reach several pixels past baseline,
  past that fixed margin, and were counted as ink inside the valley, hiding
  genuine two-line fusions from the split test. **Shipped:
  `lines.baseline_split_valley_margin`, default 0.3, provenance measured**
  (0.0 keeps the legacy fixed margin as an off switch). Screened first at
  {0.3, 0.4, 0.6, 0.7} on the two known-fused pages (0.3 best: `r000308`
  34.45%→14.50%, `r000363` 32.27%→18.90%), then gated full-corpus on both
  standing corpora: `finfilings` end-to-end CER 16.089%→**13.161%**,
  line-matched 15.910%→**12.290%** (passes by 2.928pp and 3.620pp
  respectively); `pages-cov` CER 6.057%→6.064% (+0.007, within the 0.05
  no-worse tolerance, recorded as a small loss per rule 8, not silently
  absorbed). **This is the largest single gain on the real-filings corpus
  to date.**
- **New controls for later gates: `finfilings` end-to-end CER 13.161,
  line-matched CER 12.290; `pages-cov` CER 6.064.** Session started this
  leg at 16.089/15.910/6.057. Margin values below 0.3 were not screened —
  queued next.
- **Worst pages are now unknown at the new control** and need re-listing
  before a third rule is proposed; `r000583`'s residual bbox-chaining
  mechanism is expected to remain near the top since this fix did not touch
  it, but that is a carried expectation, not yet re-measured.

**Disk, corrected this filing.** The prior entry's "99% full, ~10 GB free"
figure is **superseded**: D: now shows **273 GB free**, reported after an
outside cleanup — not from any build-directory deletion recorded in this
project's own sessions. Still not independently re-verified by this filing
(no shell in this dispatch); the next session with a shell should confirm
before relying on it, though the margin is now wide enough that disk space
is no longer read as the live constraint the prior two sessions' readings
implied.

**Open, carried forward, reordered by priority per this filing's brief:**
(1) re-list the worst `finfilings` pages against the new 0.3 control; (2)
sweep `baseline_split_valley_margin` below 0.3 (0.2, 0.25) — not yet
screened; (3) `r000583`'s residual bbox-chaining/serif mechanism —
candidate fixes are non-vertical (drop-fall/contour-following) cuts and a
width-scaled `max_splits`, queued since 2026-09-22's research, now with two
consecutive worst-page diagnoses pointing at the same page; (4) the small
checkbox/form-field column-cut fragmentation named on `r000308` (well under
10% of that page, not urgent). Then the existing queue, unchanged in kind:
`filing__r000022`'s dense-table trace; italic; the ligature error-share
count on the bold bank; the lone-guard per-page diff;
`baseline_split_sep`/`support` (a different pair of constants, from the
earlier merged-line fix, still unswept); `lines.rule_aspect`
re-measurement; the `ocrs` head-to-head, stale; SROIE not re-run;
recognition-gated chopping (research only); ALTO/hOCR underline output
(awaiting Ken's go); the one-time `rustfmt` pass (awaiting Ken's go); the
`/usage` calibration, now six sessions outstanding; and whether "commit
after each passing change" extends to Ken's other project trees.

**Token spend:** not measured against `/usage` this continuation either;
the calibration debt is carried forward again, now across six sessions.

**Measured 2026-09-24 — checkbox drop ships, `max_splits` and cut-candidate
generation both ruled out as `r000583`'s bottleneck, a per-letter stage
autopsy pins it on italic text, italic prototypes tried and reverted.**
Full narrative: `ARCHITECTURE.md` §11, the six entries dated 2026-09-23 and
2026-09-24 from "Checkbox drop, first detector: falsified at screening"
through "Italic faces: huge win on italic, broad loss on upright; gate them
by measured slant". Evidence: `docs/measurements/2026-09-23_checkbox_drop.txt`,
`2026-09-24_max_splits_sweep.txt`, `2026-09-24_cut_candidates_r000583.md`,
`2026-09-24_letter_autopsy_r000583.md`, `2026-09-24_italic_faces.txt`.

- **Checkbox drop, v1 falsified then v2 shipped.** A bounding-box-plus-fill
  detector for form checkboxes was screened first and found to delete
  hollow letters/digits (`o e a 0 6 8 9`, the counters of `D O Q P R B`)
  wholesale — CER roughly doubled at every fill threshold tried, recall on
  the screening pages fell to a fifth to a third of control. Replaced with
  per-side border-ink coverage (`Component::border_coverage: [f32;4]`,
  gated on all four sides clearing 0.85 measured-at-this-value, fill ratio
  demoted to a loose sanity bound). **Shipped.** Full-corpus gates passed:
  `finfilings` 12.786%→12.708% end-to-end, 11.686%→11.602% line-matched;
  `pages-cov` byte-identical at 6.064%.
- **`segment.max_splits` (guessed cap, 3) ruled out as the bottleneck.**
  Swept 3–8 on `r000583` and a second suspect page: decode output came back
  byte-identical across the whole range on both. A third page moved
  0.2–0.3pp CER, plateauing at N=5, with no word-level change. The cap
  truncates a sorted candidate list and the valley detector was already
  offering fewer candidates than the sweep's own floor, so raising the cap
  had nothing further to take. No default changed.
- **Cut-candidate generation also ruled out, same page.** A full per-atom
  trace on `r000583` (352 wide atoms, 187 searched) found the detector
  correctly rejecting the atoms it rejected — the two fused-bbox atoms kept
  at zero cuts were both legitimately un-splittable (a `W` crossbar, a
  `d`+comma pair whose only local minimum is the bowl/stem junction, not
  the true boundary). Diagnosis redirected upstream/downstream rather than
  re-tuning either mechanism.
- **Per-letter stage autopsy pinned the loss on match/decode, not
  segmentation, and surfaced that the page is italic.** Three temporary
  env-gated debug hooks (`OCRCER_DEBUG_BOXES`, `OCRCER_DEBUG_WORD`, a
  decoder score-breakdown), reverted before commit, tagged each of 21 lost
  letters across 5 words with the pipeline stage at which recovery became
  impossible: 10 lattice/atoms, 6 match, 4 decoder, 0 upstream
  (components/binarize/lines). The unifying observation — the passage is
  italic and the bank had no italic prototypes — came out of the tally, is
  filed as a **reading pending cross-page confirmation**, not yet a fact
  about the whole corpus.
- **Italic faces tried, measurably helped the target and measurably broke
  an unrelated population, reverted.** 22 licence-cleared Italic/BoldItalic
  faces added to the shared prototype bank (32→54 faces, 29,675→50,095
  prototypes, +67.6% bank size), no format/charset change. `r000583`
  40.633%→23.077% CER; `finfilings` corpus-wide also passed
  (12.708%→12.162% end-to-end, 11.602%→11.123% line-matched). But
  `pages-cov` **failed its own gate** (6.064%→6.202%, over tolerance) with
  6 of 7 categories regressing — the **drawing/CAD category, upright by
  construction, moved +0.136 (fails the Δ≤0 gate)** — because pooling lets
  italic prototypes compete on raw distance against upright glyphs they
  merely resemble, with nothing gating eligibility by whether the input is
  actually slanted. Wall time on `finfilings` rose 1019.5s→1699.0s (+67%),
  tracking the prototype-count growth. **`fonts.tsv` and the face table
  reverted; only the measurement file was kept. The prototype bank itself
  is gitignored and was never committed.**
- **Decision (architect's, filed here for the record): gate italic
  prototypes by measured per-word slant instead of pooling them
  unconditionally.** Per-word slant estimation (integer-angle shear +
  vertical-projection-variance score against 0°, `layout.slant_min_deg`
  guess 6°, `layout.slant_margin` guess ratio — both explicitly guesses,
  not yet measured) plus match-time eligibility gating keyed on each
  prototype's source-face style (`meta.faces[].style`). Per-line
  structural deslanting is held as a fallback arm to compare against
  gating once both are measured, not a replacement for it. **This work is
  in progress** — a background dispatch on `params.tsv`/slant-estimation
  wiring was underway as of this filing; not yet gated against either
  corpus.
- **New controls for later gates:** `finfilings` **12.708%** end-to-end /
  **11.602%** line-matched CER; `pages-cov` **6.064%** CER — all three
  figures unchanged from the pre-checkbox-v1 state because v2 reproduced
  the prior corpus scores exactly (checkbox and segmentation work were
  either net-neutral at the corpus level or reverted). **First recorded
  wall times for either corpus:** `finfilings` **1019.5 s** (60 pages, 60
  passes), `pages-cov` **487.5 s** (625 pages) — both from the `max_splits`
  sweep's full-corpus control reproduction, now the baseline against which
  the italic-faces +67% and any future wall-time change is measured.
- **Open queue, reconciled and reordered by this filing:** the per-word
  slant-gating implementation and its gate run (in progress, above);
  **deslant arm** — measure per-line structural deslant against the
  upright-only bank as the comparison point for the gating approach, not
  yet run; **`char_bonus` re-sweep** — the letter autopsy found some
  decoder losses need a bonus above the current ~3.44 (back-solved) to
  flip, sampled from only 2 cases on one page, so this is a re-sweep to
  run, not a value to adopt; **`r000022`'s touching digits** — a distinct
  fusion mechanism named in the cut-candidates trace, not yet autopsied;
  **`r000396`'s checkbox residue** — a smaller checkbox/column-cut
  fragmentation issue named on a different page during this diagnosis,
  under 10% of that page's characters, not urgent; the `ocrs` head-to-head,
  still stale; SROIE, still not re-run. Then the longer-standing carried
  queue, unchanged in kind: `filing__r000022`'s dense-table trace (distinct
  from its touching-digits mechanism above); the ligature error-share count
  on the bold bank; the lone-guard per-page diff; `baseline_split_sep`/
  `support` re-sweep; `lines.rule_aspect` re-measurement; recognition-gated
  chopping (research only); ALTO/hOCR underline output (awaiting Ken's
  go); the one-time `rustfmt` pass (awaiting Ken's go); and whether "commit
  after each passing change" extends to Ken's other project trees.

**Token spend:** not measured against `/usage` this session either — no
shell available in this dispatch to read it. The calibration debt is
carried forward again, now across seven sessions; the next session with
shell access should treat reading `/usage` as priority, not optional,
given how long this has been outstanding.

---

### Chunk 7 — pdfcer binding: merged; published; vendored by pdfcer (2026-09-24)

Full narrative: `ARCHITECTURE.md` §11, five entries from "Operator: integrate
into pdfcer now" through "pdfcer vendors OCRcer's local HEAD" — pointer
only, not restated.

- **Operator directive:** integrate into pdfcer now; beating `ocrs` is no
  longer a precondition of the hand-off (supersedes the 2026-09-21 gate in
  `PLAN.md` §2c). The head-to-head still runs and still reports losses
  (rule 8) — it now decides the *default* engine, not whether integration
  happens.
- **Merged to master:** the binding entry point, proven byte-identical to
  the pipeline path, native and wasm32.
- **Published:** `github.com/KenM76/ocrcer`, public, MIT, master only, after
  a full-history audit (no font data, datasets, weights, SolidWorks
  tooling, secrets). **Standing rule from here: each future push needs the
  operator's own go, every time** — not a one-time approval, per the
  publish entry's own wording.
- **pdfcer vendors OCRcer's local HEAD**, not the GitHub copy: pdfcer's
  `tools/sync-ocrcer.py` copies `ocrcer-core` and the adapter from
  `D:\Dev\OCRcer` HEAD into pdfcer's `vendor/`. Consequence for this repo:
  **`master` must stay releasable at every commit** — ungated core changes
  now stay on a branch until they pass, because a consumer resyncs from
  HEAD, not from a pinned release. This was already the working practice;
  it now has a name and a downstream reason.
- **A five-miss smoke test on real pdfcer pages** found no feature-vector
  gap: all five were substitutions inside correctly-bounded words (e→a at
  150dpi, gone by 200dpi; word-initial c→C/s→S at any dpi). The case cue
  already exists at feature dims 105–107; declined as a feature-vector
  change, redirected to `ocrcer-runtime` as a possible decode-side geometric
  case check, off by default, threshold `fitted` on the training split, not
  measured yet. A 300dpi binarisation garble on the same page is flagged,
  not yet chased (one page is not evidence).

**Exit gate:** not restated here (chunk 7's own gate is in `PLAN.md`); this
entry records what shipped and what remains open, per the rule against
restating a passed-gate claim without its number.

---

### Chunks 12–16 opened by three operator directives (2026-09-24) — fitting, real-scan prototypes, a neural classifier, and a self-contained LLM add-on

**Full narrative: `ARCHITECTURE.md` §11, six entries from "Operator: the
model may be trained" through "Chunk 16b rescoring: spec details fixed" —
pointer only.** `PLAN.md` §2's table does not yet carry these six chunk
numbers or their scope — **flagged to `ocrcer-architect`** rather than
invented here; this filing uses the numbers exactly as they appear in the
decision log and no others.

- **Operator, verbatim: "The model can be trained. I only stated that
  because I didn't think we had enough data."** This supersedes the
  2026-09-18 "constructed model, not a fitted one" entry as a *rule*
  (`ARCHITECTURE.md` marks that entry itself, "superseded in part,
  2026-09-24"). `CLAUDE.md` rule 1 already documents the corollary — every
  fitted value still carries provenance, split manifest and script. Data
  licences checked same-day: MultiFinBen-EnglishOCR (Apache-2.0, training
  pool minus the 60 finfilings pages already in scoring use), CORD-v2 and
  SROIE (CC-BY-4.0, **SROIE's licence at source is unverified** — held out
  of any fitting or bank build until the operator clears it, see *Open
  questions* below), scribeocr (AGPL-3.0, excluded), IRS/CRA forms (not
  cleared, held for the operator).
- **Chunk 12 — fit the existing `guess` params on the training split.** In
  progress, unmerged, on branch `fit-12b` — see the *Unmerged work*
  section below for status.
- **Chunk 13 — real-scan prototypes from aligned training crops** (kNN
  "training", no format change) — scope decided, build status not
  independently confirmed by this filing.
- **Chunk 14 — a domain lexicon and bigrams counted from training-split
  text** — scope decided, build status not independently confirmed.
- **Chunk 15 — a neural glyph classifier as a second matcher, selected by
  `match.classifier` (0 prototypes / 1 network / 2 fused), pure safe Rust
  in `ocrcer-core`, wasm32-buildable, weights in an optional `nn` table,
  gated on beating the prevailing controls plus a wall-time report** — full
  contract in `ARCHITECTURE.md`'s "build the neural network recognizer"
  entry; build status not independently confirmed by this filing.
- **Chunk 16 — the LLM add-on, `ocrcer-llm`, a new workspace crate.**
  Pure safe Rust, `std`-only (GPU is a later, feature-gated relaxation),
  implements exactly the Qwen decoder-only family, ships as one file
  (`.ocrl`, same container style as `.ocrw`, weights never committed).
  Rescoring only — an n-best list on low-confidence lines, never free
  generation, identifier-shaped tokens held fixed (rule 6 applied to an
  LLM). Sub-stages, per the operator's own ordering: **16a** (Qwen3/
  Qwen2.5 CPU engine, correctness) → **16b** (OCR rescoring, gated) →
  **16c** (Qwen3.5 text) → **16d** (Qwen3.5 vision, image-conditioned
  rescoring) → **16e** (optional `gpu` feature via `wgpu`, no extra
  download).
  - **Qwen3.5-0.8B addition is flagged as an operator-directive reading**:
    he asked for "Qwen3-0.8B", no such model exists, and the model with the
    described capabilities is Qwen3.5-0.8B — recorded as an inference to
    him in `ARCHITECTURE.md`, not silently assumed correct.
  - **16a measured and accepted for correctness, not for speed**
    (`docs/measurements/2026-09-24_llm_engine.txt`): tokenizer matches
    50/50 reference strings both models; f32 logits within 1.1e-4 of
    `transformers`, top-1 matching every prompt; Q8 keeps 96.8–97.3% top-1
    agreement, mean KL ~1e-3 nats, on a 219-token text. `.ocrl` sizes 673 MB
    (Qwen3-0.6B) / 558 MB (Qwen2.5-0.5B) at Q8, neither committed. **Speed
    measured at 3–4 tok/s at 20 threads (only 2.1–2.3× from 1 thread) — too
    slow for rescoring a real page.** A 16a-speed step (persistent thread
    pool, blocked-matmul kernel, restricted `lm_head`) was required before
    16b, against the measured f32/Q8 outputs as the regression oracle.
  - **`decode.char_bonus_slanted` (from the italic/slant-gating work):
    mechanism kept, sweep-picked value not shipped.** Sweeping it on
    scoring data (finfilings) to pick a winner would have broken the
    train/score firewall the operator's own directive just wrote in;
    shipped at the neutral 3.44 (= `char_bonus`), labelled `guess`, handed
    to chunk 12's fit rather than hand-tuned. `r000583`'s residual loss is
    not fixed by this lever.

**Measured 2026-09-24 — dense-page matching speed, a second cross-class
early-abandon ceiling (merge `f7757de`).** Full detail:
`docs/measurements/2026-09-24_dense_page_speed.md`, filed by this entry as
an alpha blocker for the pdfcer integration above, not as chunk 16 work —
`nearest()`'s only cross-prototype pruning was per-class; with 187 classes
and a 50,095-prototype bank, a losing class's prototypes were still scanned
to near completion. Added an **exact** cross-class ceiling
(`global_ceiling`, the m-th smallest finite `best_d`, `m = max(top_k, 2)`);
proof of exactness recorded in the measurement file (partial sums never
exceed true distance; `best_d` entries only fall; the final acceptance test
is untouched). **Per-page: 1.40–1.51x speedup on three profiled dense
`finfilings` pages** (early-abandon rate 58–64%→96–98%); **output
byte-identical to the unoptimised matcher on both scoring corpora**
(`pages-cov` 625/625 rows, `finfilings` 60/60 rows, `diff` exit 0 both).
**Full-corpus wall times are reported but explicitly not filed as a
reading**: the machine was shared with other jobs during the four runs
(finfilings 25.8 min baseline → 15.9 min optimised) — indicative only,
per-page 1.40–1.51x is this chunk's actual measured claim, a pinned-core
rerun is the outstanding step. `cargo test --workspace --release` green;
`cargo build -p ocrcer-core --target wasm32-unknown-unknown` clean.

---

### Unmerged branches, inventoried 2026-09-24 (all local, `D:\Dev\ExcludedPrivate\ocrcer\wt-*`, none pushed)

Not shipped, not on master. Recorded so the next session does not have to
reconstruct branch state from scratch.

- **`llm-speed`** (commits `1a21c1f`, `3e6507d`, `fac7b34`) — 16a-speed
  (persistent thread pool + blocked matmul kernel) and 16a-speed2 (batched
  `score_candidates`). **Pinned single-thread readings**, from the branch's
  own `docs/measurements/2026-09-24_llm_speed.md`: qwen2.5-0.5b Q8 prefill
  1.58→3.82 tok/s, decode 1.57→2.81 tok/s. Batched path is bit-identical to
  the per-candidate path in synthetic tests; **speed of the batched path
  itself is unmeasured**. The ~46 min single-thread / ~10 min
  multi-thread-for-50-lines×8-candidates figure in that doc is a
  **projection**, not a measurement. **Pending before merge:** a serial
  real-weights oracle run (`--test-threads=1`, never concurrent with
  another heavy job) plus pinned timings.
- **`nbest`** (`8bb5799`) — `Engine::recognize_lines_nbest`,
  `decode_word = decode_word_nbest(..,1)`. **Pending:** a byte-identical
  check against master's corpus output, and an oracle best-of-8 CER on
  finfilings-val (unmeasured).
- **`case-geom`** (`5e0dd1a`, `6567531`) — `decode.case_geom_penalty`
  default 0.0, provenance `guess`. **Pending:** a train-split sweep and its
  gates; needs rebasing onto `nbest` first, since `nbest` changes
  `decode_word`'s signature.
- **`fit-12b`** — chunk 12's coordinate-descent fit, **in progress, on
  `finfilings-train` only.** Tier 1 (decode weights) accepted at confirm
  scale: 21.823 vs. 22.091 (stride-6 train sample). Tier 2 (line params)
  inner sweeps tentatively chose `descender_fraction` 0.17 and
  `descender_reach_fraction` 0.55 (inner 22.129 vs. 22.348, others kept);
  **tier-2 confirm not yet run.** `w_lex` reverted 0.35→0.6 on resume,
  pending a confirm-scale A/B. **The campaign was killed three times today
  by Claude Code's memory-pressure reaper** — once because the architect
  ran all real-weights LLM oracle tests concurrently at ~7 GB, twice from
  general machine memory pressure (the campaign itself runs at ~70 MB).
  **Awaiting the operator's go to resume** — not restarted on any agent's
  own initiative, per the standing rule below.

**Standing rule, new this filing:** run LLM oracle tests with
`--test-threads=1` and never alongside a fitting campaign — the one
documented case of concurrent heavy jobs is what killed `fit-12b`'s run
today. A job killed by the memory-pressure reaper is restarted only on the
operator's say-so, never automatically by the next agent that notices it
stopped.

---

## Next up

Whichever of chunk 2 or the remainder of chunk 3 the operator prioritises
next. Scope for both lives in `PLAN.md` section 2 and the Backlog section
below; not restated here so the two documents cannot drift.

---

## Backlog

Chunks 2 (remaining) and 4 through 8 as laid out in `PLAN.md` section 2.
Not restated here until they become Next up, so the two documents cannot
drift.

Chunks 9 through 11 — accounting/business document structure (chunk 9,
rescoped 2026-09-21 from the original "page layout analysis" per operator
directive, `PLAN.md` sections 2 and 2a), drawing primitives (chunk 10,
`PLAN.md` section 2b), and evaluation corpora (chunk 11, `PLAN.md` section
2c) — appended after chunk 8 without renumbering. The operator raised all
three on 2026-09-21; none changes v1's scope (`FEASIBILITY.md` section 6
condition 1, `CLAUDE.md` rule 7).

**Priority, not numbering, 2026-09-21.** Operator, verbatim: "we need to
support everything that an accounting firm would need before we continue
with supporting drawings." Chunk numbering stays append-only per the rule
above, so chunk 10 is not renumbered; it is **deferred behind chunk 9** in
execution order. Chunk 9's dependency (chunk 2 only) is unchanged, and
chunk 10 still has no dependency on any recognition chunk (3-6) per
`PLAN.md` section 2 — the deferral is an operator priority choice, not a
new technical dependency. Chunk 11 also gained a gate this session: the
pdfcer hand-off, `PLAN.md` section 2c.

**Chunk 9 backlog items, added 2026-09-22 from this session's word-space-rule
work** (see the *In progress* entry above and `ARCHITECTURE.md` section 11
for the full narrative):

- **Fuzzy-space decoder resolution.** The layout stage's geometric vote
  cannot reliably tell a genuine monospace gap from a fused proportional
  word gap by shape alone — four formulations tried and measured, the best
  shipped only with an accepted loss. The backlog alternative, not yet
  attempted: score both the joined and split readings of an ambiguous gap
  at decode time and let the lexicon/bigram machinery decide, the way
  Tesseract defers "fuzzy" gap classifications until after word
  recognition (`docs/measurements/2026-09-22_research_classical_techniques.md`
  §1). This is a decoder-stage change, not a layout-stage one, and is
  larger than a parameter tune.
- **The `" " → "\n"` band-serialisation question**, raised
  2026-09-22 in `ARCHITECTURE.md`'s first "Band-pooled space rules measured"
  entry and deliberately not acted on: at `column_gap_heights=2.5` the
  largest single confusion is a band's cut fragments emitted as separate
  output lines where the truth keeps the row on one — an output-shape
  question, not a segmentation error, touching CER only. It was waiting on
  the space rule settling; the space rule now has a shipped (if imperfect)
  fallback, so this is unblocked and ready to pick up.
- **Axis-aligned maximal-whitespace-rectangle column/gutter detection**
  (Breuel, ICDAR 2003) — ranked top-3 in this session's classical-technique
  survey (`docs/measurements/2026-09-22_research_classical_techniques.md`
  §2) as the best fit for invoice/statement/title-block reading order given
  deskew already runs first; not yet implemented.
- **Drop-fall segmentation for touching/broken characters** (survey §4) —
  a single geometrically-principled cut candidate for two touching glyphs,
  proposed to seed or prioritise lattice cut-point candidates where a
  vertical-projection valley finds none (touching serifs, tight CAD
  dimension text). Not yet implemented.
- **Per-document adaptive prototypes** (survey §5) — the highest-value,
  highest-integration-cost idea in the survey: promote high-confidence
  runtime glyph matches into a small per-document prototype set using the
  existing calibrated-confidence machinery (rule 5), no new "trained"
  parameter. Flagged in the survey itself as a natural chunk-8-or-later
  target, not an immediate one.
- **Sauvola adaptive binarization** — also ranked in the same survey (§3)
  as a top-3 idea, but **not a backlog item**: it already exists in
  `crates/ocrcer-core/src/image/binarize.rs`, confirmed by grep this
  session. Recorded here only so the survey's own ranking isn't misread as
  a to-do list against current code.

**Research leads added to the architect queue, 2026-09-24 — candidates,
not commitments; none scheduled as a chunk by this filing:**

- A Tesseract-style per-page adaptive classifier, candidate chunk **13b**.
- Vertical/rotated CAD text — candidate chunk **13c**, explicitly **in
  scope** (CAD drawing text is named domain, `CLAUDE.md` rule 7).
- x-height rescaling before Sauvola binarization.
- A specialised small correction model (char-level, n-best-constrained,
  fitted on finfilings-train) as chunk 16b's fallback arm if the LLM's
  measured gain comes in under its own gates — already named in
  `ARCHITECTURE.md`'s 16b spec entry as the fallback, not a new idea, but
  not started, per that entry's own condition.

---

## Resolved

### Font licensing — osifont, DejaVu Sans, Terminus Font, `norm-stroke` (operator directive, 2026-09-21)

Operator's decision, verbatim: "keep all, just make the ones with
distribution problems due to licensing separated out for ease of removal in
cases where the end user will have to get them themselves." This closes the
four items below — previously open questions 1-4 — with all four faces
staying in scope. The keep-and-separate choice means the "drop this face,
nothing is lost" reframing recorded on each item on 2026-09-21 (after the
operator's self-authoring question, kept below) no longer decides anything;
it stays for its reasoning, not as the basis for what happened.

**Outcome:**

- **`norm-stroke` (item 4, CC0)** — shippable. A straightforward
  acquisition, unchanged from the original recommendation.
- **`osifont`, DejaVu Sans, Terminus Font (items 1-3)** — become
  **local-only**: not shipped in the package; built on the end user's own
  machine from a font file the user supplies and already holds a licence
  for.

The container mechanism that implements local-only faces is being designed
concurrently by `ocrcer-architect`, in `ARCHITECTURE.md` section 11 (the
2026-09-21 entry, "The engine reads any font; the shipped package stays
permissive-only"): a user-side bank-extension file, same container format
as the shipped `.ocrw`, version-checked against it, prototypes tagged by
origin so a confidence report can say which bank a match came from. Not
restated here, so the two documents cannot drift — read that entry for the
mechanism itself.

**Scope this creates.** Work lands in chunk 3 (the `.ocrw` writer and
prototype bank, currently *In progress* above) — the container needs to
support a supplementary/extension file, version-check it against the
shipped model, and tag prototypes by origin. (The operator's session
referred to this as "chunk 5"; `PLAN.md` section 2's table has the `.ocrw`
writer as chunk 3 and chunk 5 as pruning/prototype matching — this entry
files the scope against the verified chunk number and flags the mismatch
rather than silently resolving it either way.) Separately, a new small
shipping/build tool exposing `ocrcer-build`'s render-to-feature-vector step
to the end user is plausible scope, not yet a committed chunk — if it needs
to be its own unit of work rather than living inside chunk 3's deliverable,
the next append-only number would be **12** (after 9, 10 and 11), stated
here as provisional only, not decided, not to be read as committed.

**Reasoning kept from when these were open**, not restated, only annotated:

The chunk 1 font inventory replaced question 1 below with four specific
decisions. `model/fonts.tsv` is the authoritative table; every row carries the
source its licence was read from. Nineteen faces on this machine are eligible
and present, which is enough to build a bank. **Eighteen of them are in the
bank**: Cascadia Code and Cascadia Mono were measured on 2026-09-22 to draw
identical outlines for every class, so Cascadia Code is `excluded` — for
redundancy, not for licence, its eligibility finding standing unchanged. See
`ARCHITECTURE.md` section 11, 2026-09-22, duplicate-face entry.

**Operator question, 2026-09-21: why not self-author substitutes for all
four?** Self-authoring already happened and already closes the one category
with a genuine gap — `crates/ocrcer-build/src/face/` is an authored
single-stroke ISO 3098 Type B face covering all 187 classes, and per
`model/fonts.tsv` the eligible-present count by category is 8 mono, 6 sans, 2
serif, 2 condensed, and exactly 1 technical (STIX Two Math, a mathematical
face, not CAD lettering) — so technical had no genuine eligible-present member
until this face existed. Where ISO 3098 publishes the letterforms, an authored
implementation of them is not an imitation of anyone's face, it is an
implementation of a published standard, so authoring was the correct move
there. That does not generalise to the other three: **faces drawn by one
author correlate** in stroke contrast, terminal shape, bowl curvature and
joins, so a bank of self-drawn substitutes would look diverse by name while
sitting narrow in feature space — the entire value of a multi-font bank is
independent shape variation across real type designers, and authoring cannot
manufacture that independence. This is filed as an argument, not a
measurement: the correlation has not been quantified for this bank, and
comparing intra-authored-face feature variance against intra-real-face
variance is proposed as a chunk 8 or chunk 11 measurement, not yet run. Full
narrative in `ARCHITECTURE.md` section 11 (2026-09-21 entry).

1. **`osifont` — GPL/LGPL with a font-linking exception.** The canonical
   ISO 3098 CAD lettering face, and `ARCHITECTURE.md` had named it as
   pre-approved without anyone having checked. It is not one of the three
   licences `CLAUDE.md` rule 2 permits. Reframed 2026-09-21: the technical
   category this face was wanted for is now also carried by the authored
   ISO 3098 face. **Resolved 2026-09-21: kept in scope, local-only** per
   the operator's keep-and-separate decision above — not shipped, not
   dropped.

2. **DejaVu Sans — Bitstream Vera, not public domain.** Permissive, but with a
   name-change clause and a clause against selling the font on its own.
   Outside rule 2 as written. Reframed 2026-09-21: sans is already covered
   six times over independent of this question. **Resolved 2026-09-21:
   kept in scope, local-only.**

3. **Terminus Font — version-dependent.** GPLv2 through 4.30, SIL OFL from
   4.32. The bundled copy's version cannot be determined from the files
   present. Also a bitmap face, so it needs separate handling in
   `ocrcer-build`. Reframed 2026-09-21: monospace is the best-covered
   category in the inventory regardless. **Resolved 2026-09-21: kept in
   scope, local-only.**

4. **Acquire `norm-stroke` (CC0) before chunk 3.** A genuine single-line
   ISO 3098 Type B face, unambiguously public domain. **Resolved
   2026-09-21: shippable**, acquisition unchanged from the original
   recommendation.

---

## Open questions for the operator

Questions 1-4 (the four font-licence items) resolved 2026-09-21 — see
*Resolved* above. What remains:

**A coverage risk that is not a licence question.** The diameter sign (U+2300
⌀) is carried by exactly two of the eligible faces on this machine — Fira Code
and STIX Two Math — and by nothing else. A class with no prototype in the bank
cannot be recognised at all; it is not degraded accuracy, it is a character the
engine is blind to, so dropping either face would silently remove the class.
Noto Sans and Inconsolata also lack √ ≤ ≥ ≈ ≠; Lato lacks Ω. Latin-1 accented
letters are complete everywhere.

The authored ISO 3098 face closes this one: it draws `⌀` from the same `O` and
slash as `Ø`, so the class has a prototype that does not depend on any
third-party face surviving the licence review. **There are no classes it does
not reach** — measured 2026-09-22, the authored face contributes 187 × 4 = 748
prototypes with zero absences, which is what makes the builder's 14,088 total
reconcile.

**Audited 2026-09-22, and the risk is confirmed and explained.** Every figure
above is exactly right, plus ‰ – — which Inconsolata also lacks. The reason
U+2300 is thin: of the twelve font families on this machine that draw it, six
are CAD-vendor drafting faces (Dassault, Autodesk, GOST, Myriad CAD) and three
are Microsoft's — the codepoint this domain needs most lives almost entirely in
faces the licence posture forbids. One licence-clean candidate exists and is
not yet in the bank: Noto Sans Symbols, OFL-1.1, covering 6 of 187 classes.
See `ARCHITECTURE.md` section 11, 2026-09-22, and
`docs/measurements/2026-09-22_charset_coverage_by_face.txt`.

**Still open from bootstrap: whether pdfcer wants this at all.** The survey's
operator question about `ocrs`'s CC-BY-SA weights remains open there. If the
answer is "aggregation is fine and we will never fine-tune", one of the three
arguments for building weakens — though confidence scoring and CAD-domain
accuracy stand on their own. This question is now also a stated precondition
of chunk 11's pdfcer hand-off gate (`PLAN.md` section 2c, added 2026-09-21) —
worth resolving before that gate is reached, not merely a standing risk.

5. **The minimum-DPI declaration itself.** Raised 2026-09-21, **decided
   2026-09-22** by `ocrcer-architect` — see `ARCHITECTURE.md` section 11,
   2026-09-22 resolution-tiers entry. The three collision-derived tiers stand
   and a fourth is added below 14px per em, where the accuracy sweep measures
   this engine behind `ocrs`. The engine still never refuses; below the new
   floor it flags the result instead.

6. **Whether hand-lettered block capitals belong in scope at all.** Raised
   2026-09-21, **decided 2026-09-22** — out of v1 per `FEASIBILITY.md`
   section 6 condition 1, with the path in recorded as deterministic
   perturbation of the authored ISO 3098 face rather than a redesign. See
   `ARCHITECTURE.md` section 11, 2026-09-22.

**New gap, chunk 9 input, raised 2026-09-21 — not a decision.** The true
minus sign, U+2212, is absent from `model/charset.tsv`'s 187 classes, which
already cover `$ ¢ £ ¥ € % – — ‰ † ‡`. Many financial PDFs use U+2212 rather
than hyphen-minus (U+002D, already in the charset) for a negative number.
Flagged as a gap for chunk 9 (accounting/business documents, `PLAN.md`
section 2a) to carry as input — **not planned as settled**: adding a class
changes the class count the 17,391-pair collision sweep, the 21px floor and
every checked-in fixture were measured against, so it carries a
re-derivation cost `ocrcer-architect` has to decide deliberately, not one
this entry decides by adding a line to `charset.tsv`.

7. **DejaVu Serif — WITHDRAWN 2026-09-23 as unverified (see SESSION_LOG); reopen only on a font-audit finding.** Carried into
   this filing from the 2026-09-23 session brief as "the question to Ken on
   DejaVu Serif (Bitstream Vera licence)," but no DejaVu Serif face, row, or
   question appears anywhere in `model/fonts.tsv`, `ARCHITECTURE.md` §11's
   2026-09-23 entries, or today's `docs/measurements/` files as reviewed for
   this filing — only DejaVu **Sans** does, already resolved 2026-09-21 as
   `local-only` (see *Resolved* above). This item is recorded rather than
   silently merged into that resolved one, per the rule that an inferred
   constraint is never filed as a fact: either a DejaVu Serif question exists
   somewhere not yet surfaced to `ocrcer-librarian`, or the brief meant
   DejaVu Sans and the wording drifted. Needs a direct answer from Ken or
   the next session before it is filed either way.

8. **A one-time `rustfmt` pass, raised 2026-09-23.** The code-health
   snapshot found `rustfmt` drift in 63 files against no committed
   `rustfmt.toml`; a one-time formatting pass is pending Ken's call before
   it is run, per the `CLAUDE.md` global rule against commissioning a
   fan-out "to bring files to a standard" without a plan for how that pass
   is reviewed. Not run this session. **Still awaiting Ken's go** as of the
   end-of-session filing below.

9. **Does "commit everywhere" mean other projects too? Raised end of
   session, 2026-09-23.** The standing rule "commit after each passing
   change" is scoped to this project's tree
   (`D:\Dev\OCRcer`) in every place it is written — `ROADMAP.md`'s Standing
   rules, `RESUME.md`'s header. Whether the same discipline is wanted
   across Ken's other project trees is a question about those projects'
   own conventions, not this one's, and is not this role's to decide or
   assume either way. Needs a direct answer.

10. **Clippy — 45 warnings, reported this session, report only, no action
    requested.** Top single lint `needless_range_loop`, 13 occurrences.
    Recorded so the count has a place to be checked against later; not a
    question requiring an answer, filed here rather than silently dropped.

**Decided 2026-09-22: declined**, and not on cost. Measured across the 19
shippable faces, three of them draw U+2212 and U+002D as the same outline and a
fourth reverses the width cue, so no authored rule separates the two. The
requirement moves to chunk 9 as a semantic question and to chunk 11 as a
ground-truth normalisation that has to be declared in the report. See
`ARCHITECTURE.md` section 11, 2026-09-22.

11. **SROIE's licence needs the operator's clearance before use, raised
    2026-09-24.** The CC-BY-4.0 shown on SROIE mirrors may be the
    competition paper's licence rather than the dataset's own — unverified
    at source (`ARCHITECTURE.md` §11, "Published" entry). SROIE rows are
    held out of any fitting or prototype-bank build until he clears it
    (`CLAUDE.md` rule 2). Distinct from the older, unrelated "re-run SROIE
    as an eval corpus" backlog item elsewhere in this file, which is about
    scoring, not training input.

12. **Resume the `fit-12b` tuning campaign.** Killed three times today by
    the memory-pressure reaper (see *Unmerged branches* above); not
    restarted without his go, optionally as a detached process to escape
    the reaper.

13. **Each future push to `github.com/KenM76/ocrcer` needs his go, every
    time — not a standing default.** Recorded here because it is the one
    case in this project where a one-time approval (the initial publish)
    explicitly does **not** generalise to a standing rule, unlike the git
    *commit* default below — the publish entry states this itself.

---

## Standing rules

Chunks are capped at roughly 1.5 M billable tokens. Implementation runs on
Sonnet subagents; Opus decides and reviews. A gate that did not pass means the
chunk is not done, and saying so is the cheapest thing in the project.

**Under git since 2026-09-23, first commits approved by Ken.** `.gitattributes`
pins `* text=auto eol=lf` (LF everywhere) because fixtures are compared
byte-for-byte and a checkout-time CRLF rewrite would change their hashes;
`.pbm`/`.png`/`.ocrw` are also marked binary explicitly. **Commit after each
passing change from here forward** — not batched at session end.

**Public since 2026-09-24: `github.com/KenM76/ocrcer`, MIT, master
only.** Unlike the commit-after-each-change default above, **pushing is
not a standing default** — every push still needs Ken's own go, per the
publish decision itself (`ARCHITECTURE.md` §11). **`master` must stay
releasable at every commit**: pdfcer vendors this repo's local HEAD
directly (`tools/sync-ocrcer.py`), not a pinned release, so an unreviewed
or ungated change to `ocrcer-core` or the pdfcer adapter now has a live
downstream consumer and stays on a branch until it passes.

**Run LLM oracle tests with `--test-threads=1`, never concurrently with a
fitting campaign** — added 2026-09-24 after `fit-12b`'s run was killed by
the memory-pressure reaper while a full real-weights LLM oracle suite ran
alongside it at ~7 GB. A campaign killed by the reaper is resumed only on
Ken's own say-so.

As of the last verified git log (most recent five before this filing:
`13c422f`, `e01cf6a`, `3816af6`, `e60211a`, `69fa60d`), the tree was clean
through the atom-merge-overlap ship. Commit activity since then (merge
`f7757de` for dense-page speed, `49b4df9` for the chunk 16b spec, and
whatever this filing's own doc commit adds) is per this session's own
report and not independently re-walked by this filing beyond the two
named hashes given in the dispatch brief.
