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

**Decided 2026-09-22: declined**, and not on cost. Measured across the 19
shippable faces, three of them draw U+2212 and U+002D as the same outline and a
fourth reverses the width cue, so no authored rule separates the two. The
requirement moves to chunk 9 as a semantic question and to chunk 11 as a
ground-truth normalisation that has to be declared in the report. See
`ARCHITECTURE.md` section 11, 2026-09-22.

---

## Standing rules

Chunks are capped at roughly 1.5 M billable tokens. Implementation runs on
Sonnet subagents; Opus decides and reviews. A gate that did not pass means the
chunk is not done, and saying so is the cheapest thing in the project.
