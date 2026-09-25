# OCRcer — session log

Append-only. One section per session date. Owned by `ocrcer-librarian`.

---

## 2026-09-18 — Bootstrap

**Request:** build an MIT-licensed OCR engine from scratch to replace the one
`pdfcer` uses, including the model itself, authored from the assistant's own
knowledge rather than produced by a training run. Plan the project, write the
agents, estimate tokens, stage the work so no chunk exceeds 10% of a weekly
budget, and assess feasibility.

**Reconnaissance performed.** Read `pdfcer`'s OCR engine survey, the `OcrEngine`
trait and the `ocrs` binding module; measured the machine's CPU, GPU, RAM,
disk and font inventory.

**The finding that chose the architecture.** A fitted network's weights cannot
be authored — they are the residue of millions of gradient updates, and a
plausible-looking matrix of floats scores zero rather than scoring poorly. But
a model is not required to be a network. Classical OCR reached 98–99% on clean
print using designs whose every parameter is a rendered shape, a measured
statistic or a linguistic fact, and those are constructible. The engine is
therefore segmentation-driven prototype matching with a lattice decoder, and
the model is seven tables: some computed by a deterministic script, the rest
authored outright. No dataset, no gradient, no licence from anyone.

**The finding that justified building at all.** `ocrs` ships CC-BY-SA-4.0
weights inherited from HierText, reports no confidence of any kind, and is
trained on scene text rather than printed documents. A model with no training
corpus has nothing to inherit; match margin supplies per-character confidence
directly; and the CAD and office-document domain is one nothing else targets.
Three independent reasons, any one of which would be marginal alone. The
licence argument is stronger for a constructed model than for a locally-trained
one, because there is no corpus at all rather than a corpus we happen to own.

**The finding that shortened the schedule.** Constructing the prototype bank is
rendering ~12,000 glyphs and extracting a feature vector from each — minutes on
one core. There is no wall-clock item anywhere in the plan, no GPU dependency,
and the model is cheap enough to rebuild that the tuning loop in chunk 8 can
actually run many times. Adding a typeface is a script run.

**The risk that was accepted rather than solved.** A constructed model cannot
learn robustness to noise and degradation from data. On clean print this costs
nothing measurable; on badly degraded scans it is expected to lose to `ocrs`,
and `FEASIBILITY.md` section 5 projects 85–93% there rather than burying it.
The architecture isolates classification specifically so that one stage could
be substituted later without touching segmentation, decoding, confidence or the
file format.

**The finding that simplified the build.** Written once in Rust, the feature
extractor is shared between bank construction and recognition. Written twice,
in two languages, it becomes a permanent obligation to keep two
implementations identical — and the day they diverge, every prototype in the
bank is measured against a different ruler than the runtime uses, with no
symptom but collapsed accuracy. The workspace is therefore three Rust crates:
`ocrcer-core` (ships, zero dependencies), `ocrcer-build` and `ocrcer-bench`
(never ship, dependencies free). Correctness is carried by golden fixtures with
known ground truth rather than by comparing two implementations, which costs an
independent check on the arithmetic and buys the removal of a silent-failure
mode. That trade is recorded in `ARCHITECTURE.md` section 11 and costs roughly
1.3 M tokens less than the alternative.

**Delivered:** `FEASIBILITY.md`, `PLAN.md`, `ARCHITECTURE.md` (with a decision
log at section 11), `ROADMAP.md`, `CLAUDE.md`, `README.md`, `LICENSE`, and a
seven-agent roster under `.claude/agents/`.

**Open, carried into chunk 1:** the permissive-font shortlist needs operator
confirmation before the bank is frozen; the token budget model is a projection
awaiting calibration against `/usage`; and `pdfcer`'s own open question about
whether `ocrs`'s CC-BY-SA weights are acceptable is still open there, which
affects how strongly the licence argument counts.

---

## 2026-09-18 — Chunk 1: workspace, charset, feature extractor, fixture harness

**Request:** ship chunk 1 — the three-crate Rust workspace, the frozen
charset, the 107-dimension feature extractor, and a fixture harness proven to
fail when it should.

**Delivered:** `ocrcer-core`, `ocrcer-build`, `ocrcer-bench` building clean;
`ocrcer-core` compiling for `wasm32-unknown-unknown` with zero dependencies;
`model/charset.tsv` as the single frozen charset source, 187 classes,
contiguous indices; the extractor producing 107-dimension vectors against
`ARCHITECTURE.md` section 3; 24 tests in `ocrcer-core` and 20 in
`ocrcer-bench` (11 unit, 9 perturbation), all passing; and a fixture runner at
9/9 against the seed glyph set.

**The specification finding.** The extractor contract was written to the
arithmetic before any code existed — exact normalisation formula, exact bin
boundaries, exact index layout — and that specification caught its own
portability flaw during review: it originally called for `atan2`, `hypot` and
`ln`, none of which IEEE 754 requires to be correctly rounded, and
`wasm32-unknown-unknown` supplies its own implementations. A last-bit
difference would have made exact fixture comparison impossible across the two
targets the project is committed to. Rewritten to comparison-based angle
binning, `sqrt`, and `(w-h)/(w+h)` for aspect before anything was
implemented.

**The finding about what a test is worth.** Two separate artefacts this
chunk — the extractor's own case-discrimination unit test and the seed
fixture set — encoded the same mistake independently: both varied the line's
x-height to distinguish an 'o' from an 'O', which proves only that a ratio
depends on its denominator. Both were rewritten to hold the line metric fixed
and vary the glyph's height above the baseline, which is what actually
separates the two in print. A test can be green, specific-looking, and still
assert something trivially true.

**The font licence finding.** The architecture document and one agent
definition had both named a CAD lettering face as licence-approved. Neither
had ever been checked, and it turns out to carry a GPL font exception, which
is not one of the three licences the project permits. A second face was filed
as public domain when its licence is Bitstream Vera. Both now live in
`model/fonts.tsv` with a source column per row, and the prose font lists were
removed from both documents so there is one place to be wrong.

**The gate that mattered.** The coordinator deliberately perturbed a checked-in
fixture — `glyph_8`'s hole-count dimension, 2 to 1 — and verified the harness
caught it: `8/9 fixtures passed`, exit code 1, with a value mismatch reported
at the exact index. `cargo test --workspace` went red on the same tamper,
independently, via a test named
`checked_in_fixtures_are_untouched_by_the_suite_above`. A harness nobody has
watched fail is not known to work; this one has been watched.

**What the operator still owes a decision on:** four items, all in
`ROADMAP.md`'s "Open questions for the operator" section — three font-licence
calls (`osifont`, DejaVu Sans, Terminus Font) and the acquisition of
`norm-stroke`, plus the diameter-sign coverage risk and the still-open
`pdfcer` question from bootstrap. Not restated here.

**Carried into chunk 2:** the budget calibration run against `/usage` was
specified for chunk 1 and has not been done. No figure is filed; none is
invented.

---

## 2026-09-21 — Chunk 3: minimum-resolution floor, `§` redraw, charset ceiling widening, confusion-candidate evidence

**Request:** resolve the minimum-DPI question the render-every-glyph and
charset-band decisions had left open, re-review `§` after its first redraw
still read as a lopsided `8`, and produce the confusion-candidate evidence
chunk 4 needs.

**Delivered:** the minimum-resolution floor; a redrawn `§` in
`crates/ocrcer-build/src/face/glyphs/symbols.rs`; four widened charset
ceilings in `model/charset.tsv`; and `model/confusion_candidates.tsv` (82
rows, 12 fields, well-formed).

**The finding that closed the resolution question.** Three full entries in
`ARCHITECTURE.md` section 11, dated today, carry this narrative and the
`§` and ceiling-widening findings below — not restated here beyond a
summary. In short: an exhaustive unweighted-L2 sweep over all 17,391 pairs
of the raw 107-dim feature vector, every integer size 16–40px, no bucket
filtering, run twice independently and agreeing exactly, found exactly two
colliding pairs (`i`/`ï`, identical at 16/18/19/20px; `.`/`…`, identical at
16/17/18/20px — "identical" verified byte-for-byte, not by trusting the
metric) and zero collisions from 21px up. **The floor is 21px**, quoted
with its range: the sweep supports "no collisions 21–40px," not a claim
about sizes above 40. Both pairs are non-monotonic — clean at one size,
colliding again at a larger one — so a smallest-passing-size search
(bisection) would have returned 17px and shipped three broken sizes above
it; the floor is one more than the largest failing size in an exhaustive
sweep. Unit conversion for the DPI call: `px_per_em = points * dpi / 72`,
so 21px is 10pt at ~150dpi or 8pt at ~190dpi. Root cause: the best-pixel
fallback (adopted so a mark in the design is a mark in the bitmap) trades a
loud build-time failure for a silent one — the same trade `CLAUDE.md` rule
6 rejects for the lexicon.

**The `§` redraw.** The first redraw attempt passed every assertion and
still read as a lopsided `8`; the structural miss was that `§` has two free
stroke terminals where `8` has none. Implementation deviation, reported by
the implementing agent rather than hidden: the brief asked for literal open
curves, but the literal version read as a bisected circle, so the shipped
design closes each bowl with a full ring (as `digit_8` does) and welds a
separate hook stroke to supply the free terminal. Measured stable at every
size 16–96px: hole count 2, aspect 0.580 (band 0.45–0.65), bbox 35..365 x
15..685 design units. Bboxes against `8` now differ (20×36 vs 22×36 at
48px; identical pre-redraw). Verified by rendering it beside `8` and
reading the shapes, not by its metrics. The endpoint-count distinction that
actually separates the two is not currently any dimension of the 107-dim
feature vector — flagged as an open question for the chunk-3 prototype
bank, not filed as a defect in this glyph.

**The `§` re-measurement, a partial win per rule 8.** All four `§`
confusion pairs re-measured post-redraw in one run, at 16/24/32/48/64px:
`§`/`8` 0.37192 → 0.53969 (min at 64px), `§`/`ß` 0.37543 → 0.56056 (min at
48px), `B`/`§` 0.43366 → 0.70177 (min at 64px), `§`/`å` 0.47005 → 0.68661
(min at 64px). Against this face's 64px same-bucket distribution (1st
percentile 0.5276, median 1.5464), all four now clear the 1st percentile
but sit far below the median — all four stay on the confusion-candidate
list; `§` and `8` remain in the same hole-count-2 bucket. The same run
carried the already-established `§`/`8` value as a control and reproduced
it to 0.53968531 against the recorded 0.53969, which is what licenses the
other three deltas as measurements of the shipped glyph rather than of a
stale copy.

**Four charset ceilings widened, by rule rather than by inspection.** `<`
and `>` to 0.84, `Ω` to 0.97, `™` to 1.06 — one rule applied uniformly,
new ceiling = measured + 1.5px of width quantisation (at 64px against a
44.8px cap, one pixel is 0.022 of aspect, so the margin is 0.034).
`aspect_source` stays `authored-provisional`; each glyph's `notes` field
carries the measured value and the derivation. Checked for new collisions —
only partners whose floor sits above the old ceiling, in the same prune
bucket, are ever consulted — and found zero. All 187 glyphs are now in
band.

**New deliverable: `model/confusion_candidates.tsv`.** 82 rows (78 from a
p1-cutoff sweep, 4 named extras) — input evidence for chunk 4's confusion
table, not the table itself. Two findings worth keeping: `0`/`O`, the
textbook digit/letter confusion, does not collide in this face at any
swept size (rank 1351 of 17,391) — the charset's separation-by-aspect
design intent held. The historical `s`/`3` near-zero defect recorded
earlier in `ARCHITECTURE.md` no longer appears anywhere in current data.
`0`/`8` never reaches the distance stage at all (different hole-count
buckets); the confusion that does occur is `Ø`/`⌀` against `8`, per the
existing `ARCHITECTURE.md` entry. `.`/`·` is a different category of
problem from the two bit-identical pairs: feature dims 0–103 are identical
at every size, but the baseline-position dims 104–106 already separate
them — a chunk-4 threshold question, not a raster fix. Honest gap,
recorded in the file's own header rather than silently dropped: `,`/`.`
and `;`/`:` are named in the linguist's authored confusion-family list but
never appear as either glyph's nearest same-bucket rival at any swept
size — no row was invented for them.

**RAG lessons filed externally, at `D:\dev\rag\fonts\`** (outside this
role's own tiers — recorded here, not duplicated): new sections added to
`binary_point_sampling_drops_subpixel_marks_nonmonotonically.md` (the
best-pixel fallback's silent-failure trade; the largest-failing-size floor
rule), `aspect_band_floor_equal_to_the_design_value_is_unreachable.md` (what
a legitimate band widening looks like), and
`glyph_review_metric_tests_cannot_see_handedness_or_borrowed_paths.md` (the
reproducing control pair). The subject `index.md` rows were extended for
all three.

**Gate status:** `cargo test --workspace --release` green;
`cargo clippy --workspace --all-targets -- -D warnings` clean. Chunk 3's
own exit gate — bank build time and byte-identity, 1-NN over 99% on
isolated rendered glyphs, round-trip, int8 top-1 agreement — is not
addressed by this session and is not being recorded as passed.

**Budget:** the `/usage` calibration `PLAN.md` section 1 and chunk 1's exit
gate call for was explicitly waived this session by the operator ("go
ahead without usage"). Not measured; the calibration remains outstanding,
carried forward again rather than invented.

**Still open, carried forward:** the four font-licence items and the
`pdfcer`-wants-this question already in `ROADMAP.md`'s "Open questions for
the operator"; acquiring `norm-stroke` before chunk 3 closes; the
minimum-DPI *declaration* itself (input: the 21px floor measured today,
decision belongs to `ocrcer-architect`, not made here); and a new one —
whether hand-lettered block capitals belong in scope at all.

**Watch items, measured and deliberately not chased:** `!` a solid 2×7
block at 16px; `…` collapses to one pixel below 24px via the best-pixel
fallback; `~` and `≈` very squat; `√`'s riser near-vertical; `£`'s curl
counter 2 columns wide; `æ` barely fuses; `Ω`'s legs splay; pre-existing
hole-count instability on `&`, `Å`, `å`.

**Later the same day: the operator asked why not self-author substitutes for
the four open font-licence items above, instead of resolving licensing.**
Answered by cross-referencing `model/fonts.tsv` category counts (8 mono, 6
sans, 2 serif, 2 condensed, 1 technical) against the authored ISO 3098 face
already shipped in `crates/ocrcer-build/src/face/`: authoring is correct only
where a published standard defines the design — the technical/CAD-lettering
category, already closed this way — and cannot substitute for real-designer
independence elsewhere, because faces drawn by one hand correlate rather than
sampling independently. Filed as an argument, not a measurement; an
intra-authored-face vs. intra-real-face variance comparison is proposed for
chunk 8 or 11, not run. Recorded as a new dated entry in `ARCHITECTURE.md`
section 11 (2026-09-21) and as reframing sentences appended to `ROADMAP.md`
items 1-4. All four items stay open — this closes no decision, it only
changes what approving or dropping each one costs.

---

## 2026-09-21 — Three operator directives: font distribution, accounting-firm priority, pdfcer hand-off gate

**Directive (i) — font distribution, closing the four open font-licence
items.** Operator, verbatim: "keep all, just make the ones with
distribution problems due to licensing separated out for ease of removal
in cases where the end user will have to get them themselves." `norm-stroke`
stays shippable; `osifont`, DejaVu Sans and Terminus Font become
**local-only** — not shipped, built on the end user's own machine from a
font file the user already holds a licence for. `ROADMAP.md`'s four
open questions 1-4 moved into a new "Resolved" section with the outcome
recorded per item and the original reasoning kept, not deleted. The
container mechanism itself belongs to `ocrcer-architect`'s concurrent
`ARCHITECTURE.md` section 11 entry and is referenced, not restated. Scope
this creates: work in chunk 3 (the `.ocrw` writer, currently in progress)
for the supplementary-file container, plus possibly a new small end-user
shipping tool, flagged provisional and not chunk-numbered (next append-only
slot would be 12, stated as provisional only). Also flagged: the operator's
own phrasing named this "chunk 5," which `PLAN.md`'s table shows is
pruning/prototype matching, not the `.ocrw` writer (chunk 3) — filed
against the verified number, mismatch recorded rather than silently
resolved.

**Directive (ii) — accounting-firm document support re-prioritised ahead of
drawings.** Operator, verbatim: "we need to support everything that an
accounting firm would need before we continue with supporting drawings.
This includes the shapes of reports and book keeping tables, etc, etc."
`PLAN.md` section 2a (chunk 9) rewritten from generic "page layout
analysis" to accounting/business document structure: ruled and columnar
tables (ledgers, journals, trial balances, bank statements, AR/AP aging,
invoices/receipts) as the core case; financial-statement shape (indentation
as hierarchy, single/double rule for subtotal/total, three negative-number
forms, comparative columns); boxed forms (T4, T4A, T5, T3, T5018, T2125,
GST/HST, T2 schedules — named as identifiers only, no box contents
asserted, per `CLAUDE.md`'s ban on invented claim-bearing specifics) as a
distinct layout class whose deliverable is a box-number-to-value mapping,
called the highest-value output for an accounting firm; and prose structure
kept but demoted from centre of the chunk. Two correctness rules written in
as chunk rules, not nice-to-haves: arithmetic self-validation (columns
summing to stated totals, debits equalling credits, subtotals composing
into totals, a balance sheet balancing) is a proposed, unmeasured
capability unique to this domain in OCR; and **the engine must never alter
a recognised digit to make a total balance** — a failed check raises a
flagged discrepancy naming the fields and their per-field confidences and
never silently rewrites, the accounting analogue of rule 6's lexicon rule,
because silently balancing a ledger is the same invisible-high-confidence
failure rule 6 already names. Confidence-per-field (not per-page) recorded
as a constraint this chunk places on rule 5's confidence machinery. Chunk
10 (drawing primitives) is explicitly deferred behind chunk 9 in both
`PLAN.md` and `ROADMAP.md`'s Backlog — priority only, not renumbered, per
this project's append-only chunk-numbering rule.

**Measured input recorded for chunk 9, verified this session by direct
grep of `model/charset.tsv`:** the 187 classes include `$ ¢ £ ¥ € % – — ‰ †
‡` but not U+2212, the true minus sign — only hyphen-minus (U+002D) is
present. Filed as a gap and as chunk 9 input, explicitly not planned as a
settled addition: adding a class re-derives the 17,391-pair collision
sweep, the 21px resolution floor and every checked-in fixture, and that
cost is `ocrcer-architect`'s decision to make deliberately.

**Directive (iii) — the pdfcer hand-off, as a gate on chunk 11, not an
action.** Operator, verbatim: "once you have determined that our OCR is
better than the one we are currently using in pdfcer I want pdfcer to be
informed to add this one to its options to use." Added to `PLAN.md` section
2c as an explicit gate: "better" is defined now, before it is measured, as
primary CER on a shared corpus (WER alongside), reported per domain
(printed office documents, accounting documents, CAD drawing text) rather
than pooled, with losses reported as prominently as wins per rule 8.
Confidence calibration is recorded as a differentiator `ocrs` cannot even
report against, and is explicitly barred from being blended into the
accuracy comparison to manufacture a win. The existing corpus firewall
(no lexicon entry, bigram, confusion pair or threshold from any evaluation
corpus) is restated as binding on this comparison specifically. The
hand-off itself — informing `pdfcer`, proposing this engine as an
*additional* option alongside `ocrs`, never a replacement — is recorded as
an action outside this working tree, gated on the comparison passing and
on a fresh operator go at that time; this entry does not authorise it.
The still-open "does pdfcer want this at all" question from bootstrap is
recorded as a precondition worth resolving before the gate is reached, not
merely a standing risk.

**Delivered:** `PLAN.md` — chunk 9's table row and section 2a rewritten;
a priority-ordering paragraph added after the chunk 9/10/11 dependency
bullets; a one-line deferral note at the top of section 2b; the pdfcer
hand-off gate added to the end of section 2c. `ROADMAP.md` — a
priority-ordering paragraph added to the Backlog; a new "Resolved" section
carrying the four font-licence items with their outcomes and original
reasoning; "Open questions for the operator" trimmed to point at
*Resolved* and its surviving items (the coverage risk, the pdfcer-wants-this
question now cross-referenced to the new gate, items 5 and 6 unchanged);
a new U+2212 gap note appended after item 6.

**Not touched, verified by direct read before and after editing:**
`ARCHITECTURE.md` (read only, not written); `model/charset.tsv`;
`model/fonts.tsv`. Chunks 0 and 1 in `ROADMAP.md`'s Shipped section and
chunk 3 in *In progress* are unchanged from before this session's edits.
`ROADMAP.md`'s open questions 5, 6, the pdfcer bootstrap question and the
diameter-sign coverage risk all survive, unrenumbered, in the trimmed
"Open questions for the operator" section.

**Open, carried forward:** all of `ROADMAP.md`'s surviving open items
(5, 6, pdfcer-wants-this, diameter-sign coverage); the provisional chunk-3
scope and possible chunk-12 shipping tool from directive (i); the stale
~1.2 M token estimate on chunk 9, flagged rather than revised; the U+2212
charset-addition decision, deliberately left to `ocrcer-architect`; and the
pdfcer hand-off gate itself, which cannot be evaluated until chunk 11 runs.

---

## 2026-09-22 — Chunk 9: band-pooled space rule, fixed-pitch detection through four formulations, shipped fallback

**Request:** resolve the column-cut precision collapse `RESUME.md` had left
as the open blocker (recall rising, precision collapsing, traced to
spurious spaces inside short numeric fragments rather than a mis-measured
x-height), and — separately — do a literature survey of classical
(non-neural) OCR techniques to check the fix against known practice and to
surface further candidates.

**Reconnaissance performed.** A research pass produced
`docs/measurements/2026-09-22_research_classical_techniques.md`: Tesseract's
`textord` gap statistics and fixed-pitch/pitch-based chopping; Breuel's
maximal-whitespace-rectangle column detection, recursive XY-cut, and
Docstrum for reading order; Sauvola/Wolf-Jolion/Su-Lu-Tan/Howe for adaptive
binarization (Sauvola already shipped in this codebase, confirmed by grep);
drop-fall for touching characters; and Tesseract's two-pass adaptive
classifier, reframed here as deterministic per-document prototype promotion
so it stays inside rule 1's no-training constraint. Ranked by payoff per
unit of implementation cost against this project's actual corpora: fixed-
pitch detection first (directly fixes the named failure), Sauvola second
(already have it), whitespace-rectangle columns third.

**The finding that reframed the blocker.** A band-pooled space rule — pool
a band's gaps in x-height units, valley once, apply per fragment — was
specified and measured first, and **did not meet its own gate**: inserted
spaces fell only a third of the way to the target (727→503 on invoice
pages) while deletions of real spaces rose (377→498), net flat on word F1
(65.388%→65.234%). Root cause read directly off the dumped examples:
`112.50` → `1 12 . 50` in a monospace face, where a narrow glyph's wide
side bearings make an intra-word *gap* look exactly like a word space.
**Gap width is the wrong measurement for fixed-pitch text; cell position is
the right one** — which is what sent the session to Tesseract's `textord`
survey material above. The pooled rule shipped anyway, provisionally
(neutral on F1, better on CER/WER, reverting buys nothing), and is not
declared done.

**The finding that took four attempts to get right, each one measured and
none tuned around silently.** Fixed-pitch detection (chop by cell position,
not gap-width valley, on lines that test as monospace) went through four
distinct formulations in this session, each specified in
`ARCHITECTURE.md` section 11 before being built and measured:

1. Plain median-agreement pitch test — passed its own invoice gate but
   regressed the `statement` family: an all-caps proportional title
   (`ACCOUNT STATEMENT`) has such uniform glyph advances in a proportional
   face that its one real word gap reads as sub-grid and fuses.
2. Cell-merge (fold x-overlapping components into one position) plus an
   all-or-nothing grid-consistency check — fixed the title-fusion defect
   but **failed its own gate badly**: monospace recall collapsed 88.3% to
   62.8%, because a centre-to-centre distance carries the placement error
   of *both* its endpoints, so one off-centre glyph (a `.`, a `-`, a `1`)
   corrupts two of a short line's few available votes at once.
3. A vote that excuses "touching runs" (components fused by anti-aliasing)
   — only a partial recovery, recall 66.6%, still below gate. Failed.
4. A fitted grid (ordinary least squares through sequential cell
   positions, testing residuals rather than pairwise distances) — this
   session's own design note predicted it would be *more* sensitive to a
   single proportional word gap than the pairwise vote. Measured, the
   opposite happened: monospace recall recovered to 90.6% (passing) but
   proportional false positives rose to 2.48%–8.55% against a 0.5% gate.
   **The premise was wrong and was this log's own error, not the
   implementation's** — a least-squares fit absorbs a single anomalous step
   into its slope and intercept rather than being thrown off by it, the
   opposite of what a single-gap detector needs from its statistic.

**Delivered, as the pre-declared bounded fallback** (decided before
formulation 4 was even attempted, specifically so it would not be
re-litigated on the day if the fourth formulation also failed): cell-merge
kept, `pitch_grid_check` defaults to 0 in `Params::DEFAULT` and
`model/params.tsv` — the median-agreement test with cell merge and the
`1.5p` empty-cell split, reproducing the "merge only" ablation row exactly.
Monospace recall 94.8% at `column_gap_heights=0`, 85.6% at 2.5;
proportional false positives 1.62% and 1.80%. **Accepted, recorded loss:**
all-caps proportional titles of uniform advance still fuse — measured on
`ACCOUNT STATEMENT`, 29 of 55 font/size pairs across the corpus's 11
proportional families; `statement` family word F1 at `column_gap_heights=0`
reads 68.078% against 68.278% with pitch off. `cargo test --workspace` and
`cargo build -p ocrcer-core --target wasm32-unknown-unknown` both stayed
green throughout all four formulations; no fixture under `fixtures/expected/`
covers the layout stage yet, so none was blessed or broken.

**Interrupted, not a finding:** a full 625-page, four-arm comparison of the
shipped fallback against pitch off (both `column_gap_heights` settings) was
started to close out the session. Only arm 1 of 4 completed
(`docs/measurements/2026-09-22_fallback_full_corpus.txt`: CER 6.698%, word
F1 76.200%) before Claude Code reaped the background shell for low system
memory — not a defect in the run. **Not restarted, pending Ken.**

**Delivered:** `docs/measurements/2026-09-22_research_classical_techniques.md`,
`2026-09-22_band_pooled_spaces.txt`, `2026-09-22_fixed_pitch_spaces.txt`,
`2026-09-22_fallback_full_corpus.txt` (partial); five dated entries appended
to `ARCHITECTURE.md` section 11; `crates/ocrcer-core/src/layout/words.rs`
(band pooling, cell merge, the fixed-pitch rule and its shipped fallback,
new unit tests including a power-of-two x-height fixture chosen to keep the
tests exercising the algorithm rather than float rounding);
`crates/ocrcer-core/src/params.rs` and `model/params.tsv` (three new
`words.pitch_*` params plus two diagnostic-only toggles, all `guess`
provenance, on chunk 8's tuning list); four `personal_rag/ocr` lessons
(least-squares outlier absorption, pairwise-distance error doubling,
component-vs-cell mismatch, power-of-two float fixtures).

**Open, carried forward:** the interrupted full-corpus comparison (3 of 4
arms unmeasured); the fuzzy-space decoder alternative (score joined/split
readings at decode time, let the lexicon decide) as the real fix for the
all-caps-title loss, not attempted this session; the `" " → "\n"` band-
serialisation output-shape question, unblocked now that the space rule has
a shipped fallback; the classical-technique survey's other backlog items
(Breuel whitespace-rectangle columns, drop-fall for touching characters,
per-document adaptive prototypes); and the `/usage` token-spend calibration,
still not run, carried forward again rather than invented.

---

## 2026-09-23 — Chunk 9: column-cut guard reasoning, bold weight ships (with a same-day self-correction), a bench metric bug fixed, merged-line diagnosis and fix

**Request:** resolve the fixture failures the 1.75 column-cut threshold
exposed (a leader-only line dicing into one-dot lines); once `finfilings`
was clean enough to audit, chase down why its real-world CER still ran
around 19.5% when the corpus itself was mostly clean; and act on whatever
that audit found.

**Reconnaissance performed.** A by-eye audit of all 60 `finfilings` pages
(`docs/measurements/2026-09-23_finfilings_audit.md`) found one
render-defective page (`filing__r000011`, the same body paragraph
overprinted at two wrap widths — an upstream corpus defect, not an engine
one) and 59 clean pages; 58/60 carry bold text; 24/60 both use an fi/fl
ligature word and render in the ligature-forming serif family. A follow-up
deletion audit on the six worst clean pages
(`docs/measurements/2026-09-23_finfilings_deletions.md`) attributed 82% of
their deletions to one mechanism (below), 9% to a distinct dense-table
segmentation issue on one page, and 8% to an unrepresented italic style on
one page — with zero pages showing a line never found or a line dropped by
a filter.

**The column-cut guard: two rules proposed from a fixture, both failed on
real pages, neither shipped.** The 1.75-height leader-line fixtures (dot
leaders at 2.0 dot-heights, well under the cut) demanded *some* guard
against dicing a run of lone marks into one-glyph lines. A "both bounding
fragments must hold ≥2 components" rule passed its `pages-cov` gate but
cost `finfilings` 1.47 points of word F1 — inferred, not yet observed, to
be a `$`-in-its-own-cell table layout the synthetic corpus doesn't have.
Narrowed to "drop a candidate only when both neighbours are single
components" (exactly the leader case) — still cost `finfilings` 0.41 points
of F1. **Two rules reasoned from one fixture family, tried and cut down in
one session rather than shipped on the first plausible argument.** Shipped
state: `lines.column_lone_guard` defaults to 0 (greedy cut, unchanged
behaviour), a toggle rather than a silent revert, so both arms stay
runnable from one binary. The per-page diff that would show *which* lines
move under each rule was specified but not run this session.

**Bold weight ships — and a same-day correction is filed rather than left
standing.** Every face in the shipped bank had been Regular weight only;
the finfilings audit's headline evidence (`FOURTH QUARTER` →
`FOURTH OUARTER ANO rU11 wæ`) made the gap concrete. Bold weights of every
bank family with a present, licence-cleared Bold file were added — a
font-coverage fix, no charset/feature/format change. The first reading (32
faces, 23,740 prototypes) looked like a near-miss: `pages-cov` CER missed
its own 0.05-point gate by 0.023, on a corpus containing no bold text at
all, which was a suspicious place for a bold-specific cost to show up. It
was not one: the candidate bank had been built at four sizes (16/24/32/48)
against a five-size control (16/20/24/32/48), a confound nothing in the
tooling flagged because the size ladder is a build-time argument, not a
recorded property `inspect` could print. Re-measured like-for-like at five
sizes — **32 faces, 29,675 prototypes, int8 top-1 agreement 99.486%** —
`pages-cov` F1 76.675%→**77.392%**, CER 6.503%→**6.127%**; `finfilings` F1
68.277%→**70.846%**, CER 19.544%→**18.546%**. Both gates pass; bold ships,
no override needed. `ocrcer-exporter` now records the build ladder in
`meta`, additively, so this exact confound cannot recur silently.

**A line-matched-CER bug in `ocrcer-bench` was caught and fixed the same
session.** The reading-order-independent metric (line-matched CER,
Clausner/Pletschacher/Antonacopoulos 2020 — pair each truth line to its
best OCR line, charge edits only within the pair) was reading *higher*
than end-to-end CER on both corpora, which should not normally happen for
a metric that removes ordering disagreement rather than adding error
sources. Fixed in `cer.rs` between the two bold readings; every line-matched
figure from before the fix is marked unquotable in its own measurement file
rather than silently superseded.

**The merged-line mechanism was pinned before any rule was proposed, and
both halves of the fix were required to ship together.** `ocrcer-bench`
traced 82% of the six worst pages' deletions to two tightly-leaded real
prose lines banding as one: `overlap_fraction` alone admits the join
(2,898 and 2,693 joins traced on two pages, 100% via overlap,
`hangs_below` fired on none), at ratios of 0.90–1.00 indistinguishable
from a legitimate same-line ascender join — tightening the ratio either
changes nothing or breaks real same-line joins elsewhere, confirmed by
counterfactual reruns. The merged band's collapsed x-height (clamped to
1 px against a real 13 px cap, ratio ~0.08 against a normal ~0.74) was
still labelled `Observed`, which — by the existing safety net's own
documented contract ("a line that observed a band is never overridden") —
defeats the one mechanism designed to catch exactly this. Two rules:
**A**, a plausibility floor rejecting `Observed` below
`x_height_floor_per_cap × cap` (0.3168, measured from the shipped bank's
font metrics); **B**, a baseline-population-bimodality split (a real line
has one baseline; a merged band has two) at `baseline_split_sep` 0.6 /
`baseline_split_support` 0.25 (both guesses, unswept). **A alone failed
its own gate**: `finfilings` F1 rose 1.97 points but CER *worsened* 0.46 —
more of the merged text decoded, but still in the wrong order, which is
exactly what a partial fix to a two-cause defect looks like. **A+B
passed every gate**: `pages-cov` unchanged (synthetic corpus, open
leading); `finfilings` CER 18.546%→**17.064%**, F1 70.846%→**73.615%**.
Shipped as A+B; `baseline_split` is 1 and `measured`, its two constants
stay `guess`.

**Delivered:** ten dated entries in `ARCHITECTURE.md` §11;
`docs/measurements/2026-09-23_column_fragment_min.txt`,
`_finfilings_audit.md`, `_bold_faces.txt`, `_finfilings_deletions.md`,
`_line_merge_mechanism.md`, `_line_merge_phase2.txt`; five addenda appended
to `docs/measurements/2026-09-22_research_classical_techniques.md`
(reading-order-independent CER, classical fi/fl ligature handling, tight-
leaded line separation, italic prototypes vs. shear correction, pixel-level
vs. component-level rule removal); `lines.column_lone_guard`,
`lines.x_height_floor_per_cap`, `lines.baseline_split`,
`lines.baseline_split_sep`, `lines.baseline_split_support` in
`crates/ocrcer-core/src/params.rs` / `model/params.tsv`; `model/fonts.tsv`
extended with Bold rows; `model/out/ocrcer.ocrw` rebuilt at 32 faces /
29,675 prototypes / five-size ladder; six new `personal_rag/ocr` lessons
(size-ladder confound, baseline-bimodality vs. overlap ratio, an
`Observed`-label plausibility floor, a partial fix raising CER while
raising F1, pixel- vs. component-level rule removal, background runs
reaped under memory pressure). `cargo test --workspace` green throughout.

**Open, carried forward:** the `filing__r000022` dense-table trace (9% of
sampled deletions, a distinct segmentation sub-mechanism, inferred not
pixel-verified); italic, queued behind the merge fix, now with fresh
single-page evidence (~54% CER loss, segmentation confirmed clean); a
ligature error-share count on the rebuilt bold bank; the lone-guard
per-page diff; `baseline_split_sep`/`support` unswept; `lines.rule_aspect`
re-measurement still owed since 2026-09-22; the `ocrs` head-to-head still
stale; SROIE not re-run against the current reading-order fix; the
`/usage` calibration still not run; and a DejaVu Serif/Bitstream-Vera
question named in this session's brief that this filing could not
independently locate in any 2026-09-23 source — flagged in `ROADMAP.md`
rather than silently resolved either way.

- 2026-09-23 (architect): the DejaVu Serif / Bitstream Vera question is **withdrawn as unverified**. It came from a session summary, with no measurement or record behind it. Reopen only if a font audit of the filings finds the face. This supersedes ROADMAP open question 7.

---

## 2026-09-23 — First git commits, underline-strip debris diagnosis, split gate ships, underline strip ships

**Request:** continue chunk 9 from the merged-line/bold-weight state recorded
earlier today — diagnose why the underline-strip rule was still failing its
real-filings gate, resolve the `0$` touching-atom threshold miss on
`filing__r000022`, and, once Ken approved it, make the project's first git
commits.

**Delivered:** `lines.thin_debris_heights` (rule 2 part 3b, the strip-debris
width+height gate); `segment.split_min_x_heights` 1.15 → 1.09; underline
strip shipped (`lines.underline_strip` = 1); underline code moved out of
`lines.rs` into `layout/underline.rs`; a fixed params-census swap bug in the
build report; five git commits; four new `personal_rag/ocr` lessons.

**The finding that closed the underline-strip regression.** Two rules had
already failed the real-filings gate (`ARCHITECTURE.md` §11, the "fails the
real-filings gate" entries). The trace that finally explained it: erasing a
bordered box's rules correctly removed the furniture, but the box's own
left/right sides survived the strip as new components 10–22× the line
height tall — too narrow for the existing width-keyed furniture filter,
too short for the existing height-keyed debris filter. Tallest-first line
grouping then seeded a band on the sliver and fused two real prose lines
(89→69 lines on one page, 91→72 on another), which read, before the trace,
as unrelated broad letter deletions. Fixed with a joint width-and-height
gate on strip-produced pieces specifically (`lines.thin_debris_heights`,
base value measured at 3.4288 from the shipped font bank, ×1.5 headroom a
labelled guess). Filed as
`personal_rag/ocr/lesson_20260923_strip_debris_thin_slivers_need_width_and_height_gate.md`
— generalises past this project: a pixel-erasure pass's own frame is
exactly the shape its debris filter needs a joint test for.

**The `0$` atom turned out to be a threshold miss, not a segmentation-design
gap.** The 11px-wide touching-glyph atom on `filing__r000022` never reached
the cut-candidate search at all — `segment.split_min_x_heights` (1.15) ×
x-height (10) gated it out before a profile was even computed. Computed for
diagnosis, the profile had an unambiguous valley the existing rule would
have cut correctly. Swept {1.0, 0.85, 1.09} against both `finfilings` and
`pages-cov`: **1.0 won biggest on `finfilings`** (CER 17.064%→16.885%) **but
failed the `pages-cov` gate** (CER 6.127%→6.330%, clean-text
over-segmentation against a 0.05 tolerance). **1.09 passed both**:
`finfilings` CER 17.064%→**16.932%**, line-matched 16.942%→16.843%, F1
73.615%→74.047%; `pages-cov` CER 6.127%→**6.089%**, F1 77.392%→77.429%.
1.09 rather than the more natural-looking 1.10, because `1.10f32` widens to
`1.1000000238` in `f64` — `10 × 1.10 (widened)` is still `> 11.0`, so the
11px atom would have missed its own gate under the strict `<` comparison.
Two lessons filed: on the float-widening boundary miss, and on the general
rule that a cut-search width gate needs both a real and a clean corpus in
its own gate, not just the corpus that motivated the change.

**Underline strip ships, measured at the 1.09 split gate.** `finfilings`
CER 16.932%→**16.756%** (line-matched 16.843%→16.634%, F1
74.047%→**74.405%**); `pages-cov` unchanged (6.089%/77.429%).
`lines.underline_strip` = 1, provenance `measured`. Erased bands are now
also recorded as `RuleSegment`s in the layout output — unconsumed by any
output format yet, but this is the data Ken's ALTO/hOCR underline-
preservation direction (recorded in `ARCHITECTURE.md` §11, not yet
scheduled as a chunk) would read from. `r000583` is now the worst
`finfilings` page at 54.66% CER, not examined this round.

**A build-report bug was caught and fixed the same session.** The build
report's params census had measured and authored figures swapped. Fixed;
any earlier log or report quoting that census before the fix carries the
swapped figures, not silently corrected retroactively.

**Code health, checked this session and reported (not independently
re-verified by the librarian filing — no shell in that dispatch):** 45
`clippy` warnings (top lint `needless_range_loop`, 13 occurrences);
`rustfmt` drift in 63 files against no committed `rustfmt.toml`; 1 `unwrap`
and 11 `expect` in `ocrcer-core`, all reported as guarding internal
invariants rather than untrusted input. A one-time `rustfmt` pass is
recorded as pending Ken's call, not run.

**A disk-pressure incident, reported this session:** D: hit 100% free space
mid-session; an agent deleted two untracked scratch files (`aspect_err.txt`,
`aspect_out.txt`) under that pressure, and stale `target/` build
directories were cleaned. 38 GB free afterward is reported but not
independently re-verified in this filing.

**First git commits, approved by Ken.** The tree had been untracked through
the previous session (`RESUME.md`: "zero commits, no remote"). Five commits
now exist on `master`: `4c85f69` initial snapshot, `9436f50` LF line-ending
pin plus binary-fixture marking, `a8a24be` split-gate ship, `f9dcd8f`
underline-strip code moved into `layout/underline.rs`, `3772a41`
underline-strip ship plus the build-report fix. `.gitattributes` pins LF
everywhere (`* text=auto eol=lf`, plus explicit `binary` for
`.pbm`/`.png`/`.ocrw`) specifically because fixtures are compared
byte-for-byte and a checkout-time CRLF rewrite would change their hashes.
**Standing rule from here forward: commit after each passing change,**
recorded in `ROADMAP.md`'s Standing rules.

**Delivered:** six dated entries in `ARCHITECTURE.md` §11 (underline-strip
diagnosis and shipping, split-gate sweep);
`docs/measurements/2026-09-23_underline_strip*.txt`,
`_underline_strip_damage.md`,
`_underline_r000055_and_touching_r000022.md`,
`_split_gate_and_strip_3b.txt`; `lines.thin_debris_heights`,
`segment.split_min_x_heights` (1.09), `lines.underline_strip` (1) in
`crates/ocrcer-core/src/params.rs` / `model/params.tsv`;
`crates/ocrcer-core/src/layout/underline.rs` (moved out of `lines.rs`); five
git commits; four `personal_rag/ocr` lessons (strip-debris width+height
gate, f32→f64 threshold widening, dual-corpus cut-search gating, Tesseract
confidence-gated chopping as research). `cargo test --workspace --release`:
304 tests, 0 failed.

**Open, carried forward:** `filing__r000022`'s dense-table trace (still
inferred, not pixel-verified); italic, now with `filing__r000583` as
concrete worst-page evidence (54.66% CER); the ligature error-share count on
the bold bank; the lone-guard per-page diff; `baseline_split_sep`/`support`
unswept; `lines.rule_aspect` re-measurement; the `ocrs` head-to-head still
stale; SROIE not re-run; recognition-gated chopping (Tesseract-style,
unmeasured, research only); ALTO/hOCR underline formatting output (awaiting
Ken's go to schedule); the `/usage` calibration still not run, now carried
across four sessions; and the one-time `rustfmt` pass, pending Ken's call.

---

## 2026-09-23 — Chunk 9, continued: worst-page diagnosis, atom merge by overlap fraction ships (0.3 then 0.4)

**Request:** find out why `filing__r000583` had become the worst
`finfilings` page at 54.66% CER after the merged-line and underline-strip
fixes shipped earlier the same day, and act on whatever that diagnosis
found; continue the "research OCR techniques while fixing what the
research surfaces" loop otherwise.

**Reconnaissance performed.** A targeted trace on `r000583`
(`docs/measurements/2026-09-23_worst_page_r000583.md`) rather than a
corpus-wide sweep, since one page was already isolated as the worst.

**The finding that named the mechanism.** `atoms()` had been merging any
two components whose x-ranges overlapped at all — a rule written for an
`i` and its dot, where the overlap is total. In `r000583`'s serif face, a
`t` crossbar and an `h` base serif overlap by 1–6 columns *at different
heights*, so the ink itself never touches, but the any-overlap rule glued
them into one atom anyway. The page carried 73 such atoms chaining three
or more letters, seven at the worst, against three (none over three
letters) on an ordinary page. A chained atom that size defeats
`max_splits = 3` outright — there is no vertical cut that recovers seven
letters glued at a slant, and the underline strip shipped earlier that day
changes nothing about this page, because the defect predates it.

**The fix, and why it was cheap.** The letters were already separate
connected components; the segmenter was gluing components that never
needed gluing. `segment.merge_overlap_frac` now requires either full
column-range containment (still catches `i`/dot, `:`, `;`, `=`) or overlap
≥ a fraction of the narrower component's width. Pieces are cropped by their
own member components' pixels (`edge_labels()`) rather than by a column
range, so a separated `t` no longer scoops up its neighbour's serif when
the merge rule tightens. This is the same shape of fix as the earlier
merged-line and underline-strip diagnoses this project keeps landing on:
find the exact geometric coincidence a coarse rule was confusing for the
case it was written for, then narrow the rule with a real fraction rather
than replacing the technique.

**Measured in two passes, neither guessed past its own evidence.** A
one-page screen at 0.3/0.5/0.7 (0.3 best) shipped first, since it already
passed all three standing gates (`r000583` 54.661%→40.000%;
`finfilings` end-to-end CER 16.756%→16.113%, line-matched
16.634%→16.068%; `pages-cov` CER 6.089%→6.057%) — an improvement on the
synthetic corpus too, so the chaining defect was not confined to scanned
serif filings. A follow-up full-`finfilings` sweep at {0.15, 0.2, 0.3, 0.4}
then found **0.4 better than 0.3 on both `finfilings` CERs**
(end-to-end 16.089%, line-matched 15.910%), with `pages-cov` unmoved at
6.057%. The architect re-ran 0.4 independently and reproduced it to the
digit — recorded as corroboration, not as a new finding. One anomaly was
flagged rather than silently trusted: the swept 0.2 row's word-level
figures read identical to 0.4's in the measurement file, read as a likely
transcription slip on a value that was rejected either way, not corrected
retroactively.

**Delivered:** `docs/measurements/2026-09-23_worst_page_r000583.md`,
`_atom_merge_overlap.txt`; three dated entries in `ARCHITECTURE.md` §11;
`segment.merge_overlap_frac` in `crates/ocrcer-core/src/params.rs` /
`model/params.tsv`, shipped at 0.4, provenance measured; a research-note
addendum on drop-fall cuts appended to
`docs/measurements/2026-09-22_research_classical_techniques.md`, queued
behind this fix for touching-ink cases specifically (this page's defect
was chaining, not touching ink, so it did not need a non-vertical cut);
five git commits (`69fa60d` research note, `e60211a` merge-by-overlap-
fraction implementation, `3816af6` decision-log entry for the 0.3 ship,
`e01cf6a` the 0.15/0.2/0.4-vs-0.3 sweep, `13c422f` the 0.3→0.4 ship).

**New controls:** `finfilings` end-to-end CER **16.089%**, line-matched
**15.910%**; `pages-cov` CER **6.057%**, F1 **77.540%** (this session's own
report; not independently re-run by this filing). Session started at
16.756 / 16.634 / 6.089.

**A disk-space escalation, reported this session, not independently
re-verified (no shell in this filing dispatch):** D: reached 99% full,
~10 GB free, tighter than the prior session's unverified 38 GB figure. The
architect deleted `target/debug`, `runtime-diag`, `glyphs-agent`, `wasm32`
and `tmp` build directories (~3 GB reclaimed), leaving only
`target/release`. Consequence flagged forward: any diagnostic agent that
builds under its own `CARGO_TARGET_DIR` will rebuild from scratch on next
use, since its directory was among those removed.

**Open, carried into the next session:** re-diagnosis of the new worst
pages against the 0.4 control — `r000583` is still worst at 40.00% CER,
and `r000022`/`r000055`/`r000044` (34–37% before this fix) need
re-measurement; optionally sweeping 0.35/0.45/0.5 full-corpus (small
expected gain); non-vertical (drop-fall/contour) cuts and a width-scaled
`max_splits` for touching-ink pages specifically, now queued with sharper
evidence than the 2026-09-22 research note alone; recognition-gated
chopping; italic; ligature share; `baseline_split_sep`/`support` sweeps;
`rule_aspect` re-measurement; the `ocrs` head-to-head; SROIE; ALTO/hOCR
output (awaiting Ken's go); everything else already carried forward above.
Two questions for Ken, newly recorded this filing: whether "commit after
each passing change" extends to his other project trees, and the still-open
`rustfmt` pass go/no-go. Clippy's 45 warnings are recorded as report-only,
no action requested.

---

## 2026-09-23 — Chunk 9, continued further still: worst-pages round 2 diagnosed, line-fusion fix ships (largest real-filings gain to date)

**Request:** re-diagnose the worst `finfilings` pages against the
`merge_overlap_frac=0.4` control from the prior continuation, and act on
whatever that diagnosis found.

**Reconnaissance performed.** A targeted re-diagnosis of the (then) three
worst pages — `r000583`, `r000308`, `r000363` — rather than a corpus-wide
sweep, in `docs/measurements/2026-09-23_worst_pages_round2.md`.

**The finding that split one page into two mechanisms.** `r000583`
(40.63% CER) is the same serif-bbox-chaining defect as before, worked down
from 54.66% by the earlier `merge_overlap_frac` fix but not resolved — its
tightest chains, where one component's ink genuinely sits mostly inside the
other's column range, clear the "always merges" branch a fraction threshold
cannot gate. `r000308` (34.45%) and `r000363` (32.27%) turned out to be a
different, previously undocumented mechanism: `group_with_bands` fuses two
ordinary, cleanly-separated body-text lines into one x-interleaved band
whenever a document's leading is tight enough (~1.7 x-heights here, against
`r000583`'s ~2.0–2.2). Reading order was ruled out on all three pages by
direct comparison of end-to-end and line-matched CER. The mechanism
survives the existing two-baseline split pass, and rather than write a
fourth new rule, the diagnosis named finding out *why the split pass
doesn't fire* as the next step.

**The finding that explained the miss and the fix that followed from it.**
The split test's valley search excluded a fixed 2-pixel-row margin around
each candidate baseline peak — a raw pixel count, not scaled to type size.
At this corpus's body sizes, descenders (`g p q y j`) reach several pixels
past their own baseline, past that fixed margin, and were being counted as
ink inside the valley, hiding genuine two-line fusions from the split test.
Fix: scale the margin to the line's own x-height
(`lines.baseline_split_valley_margin`, 0.0 = legacy fixed margin, the off
switch). Screened at {0.3, 0.4, 0.6, 0.7} on the two known-fused pages (0.3
won both), then gated full-corpus on both standing corpora.

**Shipped, and the largest single gain on the real-filings corpus to
date.** `finfilings` end-to-end CER 16.089%→**13.161%**, line-matched CER
15.910%→**12.290%** — both pass their gates by roughly 3 points, not
fractions of one. `pages-cov` moved from 6.057% to 6.064%, a 0.007-point
loss inside the 0.05 no-worse tolerance and recorded as a loss, not
silently absorbed, per rule 8. `lines.baseline_split_valley_margin` ships
at 0.3, provenance `measured`.

**New controls for every later gate: `finfilings` end-to-end CER 13.161,
line-matched CER 12.290; `pages-cov` CER 6.064.**

**A disk correction, not independently re-verified here (no shell in this
filing dispatch):** D: is reported at **273 GB free** after an outside
cleanup — supersedes the prior two sessions' "38 GB" and "~10 GB" figures,
both themselves unverified. The margin is now wide enough that disk
pressure is no longer read as a live constraint, but the next session with
a shell should still confirm before relying on it.

**Delivered:** two dated entries in `ARCHITECTURE.md` §11 ("Worst pages,
round 2" and "Line fusion fix"); `docs/measurements/2026-09-23_worst_pages_round2.md`,
`_line_fusion_fix.txt`; `lines.baseline_split_valley_margin` in
`crates/ocrcer-core/src/params.rs` / `model/params.tsv`, shipped at 0.3; a
new `personal_rag/ocr` lesson on fixed-pixel margins near a profile valley
needing to scale with x-height, filed this session by `ocrcer-librarian`.

**Open, carried forward, reprioritised:** the worst `finfilings` pages are
unknown again at the new 0.3 control and need re-listing before a third
rule is proposed — `r000583`'s unresolved bbox-chaining residual is
expected to stay near the top since this fix didn't touch it, but that is
a carried expectation, not yet re-measured; `baseline_split_valley_margin`
below 0.3 (0.2, 0.25) was not screened; non-vertical/contour cuts and a
width-scaled `max_splits` for `r000583`'s residual chains; the small
checkbox/form-field fragmentation named on `r000308`; and everything
already carried forward from earlier today (the dense-table trace, italic,
ligature share, the lone-guard diff, `baseline_split_sep`/`support`
(a distinct, still-unswept pair from the earlier merged-line fix),
`rule_aspect` re-measurement, the `ocrs` head-to-head, SROIE,
recognition-gated chopping research, ALTO/hOCR output, the `rustfmt` pass,
and the `/usage` calibration — now six sessions).

## 2026-09-24 — Checkbox drop ships, `max_splits` and cut-candidate generation both ruled out on `r000583`, a per-letter stage autopsy finds it's italic, italic prototypes tried and reverted

**Request:** drop form-field checkboxes as furniture before recognition
without deleting hollow letters; continue the `r000583` worst-page
diagnosis carried forward from the prior session, checking whether the
segmentation split cap or cut-candidate generation was the binding
constraint before tuning either.

**Reconnaissance performed.** A first checkbox detector (near-square
bounding box, low ink-fill ratio) was screened on four real pages with
known checkboxes before being trusted at corpus scale.

**Checkbox detection needed two attempts.** The fill-ratio detector was
catastrophic at every threshold tried (0.30–0.55): CER roughly doubled and
recall on the screening pages fell to a fifth to a third of control,
because a plain hollow letter or digit (`o e a 0 6 8 9`, the counters of
`D O Q P R B`) presents the same near-square aspect and low interior fill
as an empty checkbox — there is no separately-labelled "contained mark"
component for either shape, and a variant requiring one fired on nothing
across all four screening pages. The fix: extend the connected-component
record with per-side border-ink coverage (`Component::border_coverage:
[f32;4]`, a `height/12`px band at each edge), gated on all four sides
clearing 0.85 (measured-at-this-value), with the fill-ratio test demoted
to a loose sanity bound. **Shipped** — all four screening pages improved
or held, full-corpus gates passed byte-identical on `pages-cov` and
improved on `finfilings` (12.786%→12.708% end-to-end, 11.686%→11.602%
line-matched).

**The suspected segmentation-cap bottleneck did not bind.** Sweeping
`segment.max_splits` (guessed cap of 3) from 3 to 8 on `r000583` and a
second suspect page produced byte-identical decode output across the
entire range on both; a third page moved 0.2–0.3pp CER, plateauing at
N=5, with zero word-level change. The cap truncates a sorted candidate
list and the valley detector was already offering fewer candidates per
fused atom than the sweep's own floor allowed for, so raising it had
nothing further to take. No default changed.

**Cut-candidate generation was checked next and also ruled out.** A full
per-atom trace on `r000583` (352 wide atoms, 187 searched; 54 bbox-merged
fused atoms, 2 kept at zero cuts) found the detector correctly rejecting
what it rejected — one fused atom is a `W` crossbar, the other a `d`+comma
pair whose only local minimum is the bowl/stem junction, not the true
letter boundary. Two consecutive ruled-out mechanisms on the same page
with no third hypothesis in hand redirected the method itself rather than
producing a third guess.

**A per-letter stage autopsy pinned the loss on match/decode, and
surfaced that the page is italic.** Three temporary, env-gated debug
hooks — a decoded-character dump, a word-scoped lattice dump gated on
both x- and y-span (an x-only filter had silently matched every line
sharing a left margin), and a decoder score-breakdown — were added,
exercised, and reverted before commit. Tagging each of 21 lost letters
across 5 words by the earliest pipeline stage at which recovery became
impossible produced a hard tally: 10 lattice/atoms, 6 match, 4 decoder, 0
upstream. The passage being italic — with no italic prototypes anywhere
in the bank — came out of the tally as an observation, not a premise fed
into it; filed as a reading pending a cross-page count, not yet a fact
about the corpus.

**Italic prototypes were tried, helped the target, and broke an unrelated
population — reverted the same session.** 22 licence-cleared
Italic/BoldItalic faces were added to the shared bank (32→54 faces,
+67.6% prototypes), no format or charset change. `r000583` improved
40.633%→23.077% CER and `finfilings` passed corpus-wide
(12.708%→12.162%/11.602%→11.123%), but `pages-cov` **failed its own gate**
(6.064%→6.202%) with 6 of 7 categories regressing — the drawing/CAD
category, upright by construction, moved +0.136, failing the Δ≤0 gate —
because pooling italic prototypes into the shared bank lets them compete
on raw distance against upright glyphs they merely resemble, with nothing
gating eligibility by whether the input is actually slanted. Wall time
rose +67% (1019.5s→1699.0s on `finfilings`), tracking the prototype-count
growth. `fonts.tsv` and the face table were reverted; only the
measurement file was kept, and the enlarged bank itself was never
committed (gitignored). The decided fix direction — `ocrcer-architect`'s,
recorded here for the record — is to gate prototype eligibility by
measured per-word slant at match time instead of pooling unconditionally,
with per-line structural deslanting held as a fallback arm to compare
against once both are measured. **This work is in progress** as of this
filing.

**New controls for every later gate, unchanged from before this session's
work (checkbox v2 reproduced the prior corpus scores exactly, and the
segmentation diagnoses changed no defaults): `finfilings` end-to-end CER
12.708%, line-matched CER 11.602%; `pages-cov` CER 6.064%.** First
recorded wall times for either corpus: `finfilings` 1019.5 s (60 pages),
`pages-cov` 487.5 s (625 pages), from the `max_splits` sweep's full-corpus
control reproduction — now the baseline for the italic-faces +67% and any
future wall-time comparison.

**Delivered:** the checkbox border-coverage detector, shipped in
`crates/ocrcer-core` with its own fixtures; six dated entries in
`ARCHITECTURE.md` §11 (two on the checkbox detector, two on the
segmentation diagnosis, one on the letter autopsy's italic finding, one on
the italic-faces measurement and revert); `docs/measurements/
2026-09-23_checkbox_drop.txt`, `2026-09-24_max_splits_sweep.txt`,
`2026-09-24_cut_candidates_r000583.md`, `2026-09-24_letter_autopsy_r000583.md`,
`2026-09-24_italic_faces.txt`; four new `personal_rag/ocr` lessons filed
this session by `ocrcer-librarian` (checkbox border-coverage, measure-
before-tuning-a-cap, the per-letter stage-autopsy method, pooled-style-
prototypes-need-a-competition-gate); this `ROADMAP.md` filing with the new
controls and wall times; this entry.

**Open, carried into chunk 9's continuation:** the per-word slant-gating
implementation and its gate run, in progress; the deslant arm, not yet
run; a `char_bonus` re-sweep (the autopsy found some decoder losses need a
bonus above the current ~3.44 to flip, from only 2 sample cases on one
page); `r000022`'s touching-digits mechanism, not yet autopsied;
`r000396`'s smaller checkbox/column-cut residue, under 10% of that page,
not urgent; the `ocrs` head-to-head, still stale; SROIE, still not re-run;
plus the longer-standing carried queue — `filing__r000022`'s dense-table
trace, the ligature error-share count, the lone-guard per-page diff,
`baseline_split_sep`/`support` re-sweep, `lines.rule_aspect`
re-measurement, recognition-gated chopping research, ALTO/hOCR output and
the one-time `rustfmt` pass (both awaiting Ken's go), and whether "commit
after each passing change" extends to Ken's other project trees. Token
spend against `/usage` remains unmeasured — no shell in this dispatch —
now across seven sessions.

---

## 2026-09-24 — Checkpoint refresh: dense-page speed ships, chunk 16b spec fixed, five unmerged branches inventoried

**Request:** a checkpoint refresh of `ROADMAP.md`, this log and `RESUME.md`
against today's state, without touching `ARCHITECTURE.md`, `PLAN.md`,
`FEASIBILITY.md` or the untracked publish-audit note.

**Reconnaissance performed.** Read `ROADMAP.md` and `RESUME.md` (both last
updated mid-session, before the day's operator directives on training, the
neural classifier, the LLM add-on and the pdfcer hand-off change); read
`ARCHITECTURE.md` §11 forward from the last point either file reflected
(the italic-gating ship) through the newest entry (chunk 16b's spec); read
`docs/measurements/2026-09-24_dense_page_speed.md` in full.

**The finding that this filing exists to fix.** `ROADMAP.md`'s "In
progress"/"Next up"/"Backlog" sections still read as if chunk 9 were the
active frontier and chunks 2/3 were next up, while `ARCHITECTURE.md` §11
had already logged three operator directives opening chunks 12 through 16,
a pdfcer integration-priority change, a GitHub publish, and a pdfcer
vendoring model — none reflected in either file. This is recorded as a
gap this filing closes by pointer to `ARCHITECTURE.md`, not by restating
its narrative; `PLAN.md` §2's table does not yet carry chunks 12–16 either,
flagged to `ocrcer-architect` rather than invented here.

**Delivered.** `ROADMAP.md`: new "In progress" sections for chunk 7
(pdfcer binding — merged, published, vendored), the chunks-12–16 opening
(training approved, neural classifier contract, LLM add-on contract,
16a's correctness-yes/speed-no result), the dense-page speed ship (merge
`f7757de`, per-page 1.40–1.51x, byte-identical on both corpora, full-corpus
wall time explicitly flagged as indicative-only/shared-machine), and an
inventory of five unmerged local branches (`llm-speed`, `nbest`,
`case-geom`, `fit-12b`) with their pending-before-merge conditions; three
new *Open questions for the operator* (SROIE licence, resume `fit-12b`,
per-push approval is not a standing default); research leads (13b/13c
candidates, x-height-before-Sauvola, the 16b fallback model) filed to
Backlog as candidates, not commitments; two new Standing rules (`master`
must stay releasable now that pdfcer vendors HEAD; run LLM oracle tests
`--test-threads=1`, never beside a fitting campaign). `RESUME.md` rewritten
to the current state.

**Not independently re-verified by this filing:** no shell in this
dispatch. The chunk 16a `.ocrl` sizes, wall-time figures, `fit-12b`'s
inner-sweep numbers and the branch commit hashes are filed as reported by
the sessions that measured them, per their own measurement files and
`ARCHITECTURE.md` entries — this filing did not re-run anything.

**Open, carried forward:** everything in `ROADMAP.md`'s now-updated
Backlog and *Open questions* sections; `fit-12b`'s tier-2 confirm run; the
`llm-speed` branch's serial real-weights oracle run and pinned timings;
`nbest`'s byte-identical corpus check and oracle best-of-8 CER; the
`/usage` calibration, still outstanding.

---

## 2026-09-25 — fit-12b campaign tiers 1-4, chunk 12b/12c/13/13b/13c/14/15/16a/16b specs and builds, two confidence defects, a descender-defect correction

**Request.** Checkpoint refresh (this filing's own dispatch: last filing
2026-09-24 16:16 / commit f0481e0; large volume of chunk work landed
since). Not a new engineering request — an append of everything the
architect's working queue recorded.

**fit-12b campaign, all four tiers now have a verdict (measured, train
stride-6/stride-2 confirm runs from `campaign.py`/`campaign_resume2.py`/
`campaign_post.py`, per `fitlogs/campaign_stdout.log`).** Tier 1 (decode
weights) ACCEPTED: CER 22.091→21.823 (stride 6); `w_lex` moved 0.6→0.35 by
a flat-sweep tie only, then reverted after a stride-6 A/B showed 0.6 is
0.021 better (21.802 vs 21.823) — the inner-sample "0.039 worse" reading
was noise. Tier 2 (line params) ACCEPTED but flagged mixed: CER
21.802→21.740 improved, but line-matched CER got worse (23.724→23.846)
and F1/precision/recall all regressed; campaign rule accepted it on CER
alone, an end-of-campaign ablation is required before trusting it
long-term. Tier 3 (segmentation params) ACCEPTED clean: CER 21.740→21.099,
line-matched CER 23.846→23.314, F1 68.338→70.686 — all four metrics
improved together. Tier 4 (slant) ACCEPTED: 21.099→21.010 (`slant_margin`
1.08), LM-metric 23.211, F1 70.797. A late reading (not a decision) found
`match.top_k=3` gives CER 20.963, cheaper than tier-4's `top_k=5` (21.010)
— filed as a reading, not walked into the post-chain because it would cut
headroom the chunk-14/16b work still needs.

**Two mid-campaign incidents, neither restarted without Ken's go.** The
memory-pressure reaper killed the campaign once (PID 29860, 3.8/16 GB
free) mid tier-2 inner sweep; salvage was tier-1's already-accepted result
plus a flat tier-2 x-height-fraction reading. It was killed a second time
when all 6 LLM oracle tests were run concurrently with it (~7.2 GB) — this
was the architect's own error; the standing rule from the prior filing
(oracle tests `--test-threads=1`, never beside a campaign) was violated,
not missing. Both incidents are filed as measurements of what happened,
not projections.

**Confidence-machinery: two real defects found by research, not yet
fixed in the shipped default (`decode.width_weighting`/etc. still 0).**
(1) the pipeline gives every candidate the matcher-winner's d1/d2 instead
of its own; (2) `confidence::adjust` is never called, so `lm_floor` is
dead code. Branch `conf-margin` (per-candidate ratio, an `agreed` flag,
separate agree/override calibration curves, default 0.01→0.05, an
identity word curve) was built and reviewed accepted (commit `21eb6a3`,
one bit-for-bit test nit fixed in `07027db`). §11 entries `924f503`
(research) and `6ad92ab` (decision) hold the defect writeup.

**A fitted confidence curve now has a stated params-row convention
(§11, commit `0467ac5`, already in `ARCHITECTURE.md` — not restated here
beyond the pointer):** one row per knot coordinate (agree r0-r5/c0-c5,
override r0-r1/c0-c1, word r0-r1/c0-c1), labelled `fitted`, naming
`fit-calibration`/the split manifest/`finfilings-val`, `tune=no`; load-time
validation requires ratios ascending and the three curve families
non-rising/non-falling as specified, monotonicity failure is a load error,
not a silent fallback.

**The descender/cap-band defect was corrected upward, not just found.**
First reading (spec `f196a6e`) measured 8.2% of components on the
descender fallback branch, but only counted the 24 of 43 sampled train
pages that also had an independent cap-band line — a silent exclusion.
The corrected full re-read (commit `c52c91e`, all 43 pages, every-10th-page
train sample) found **905 of 6,141 lines on the descender branch, 13.9% of
all components, across 31 of the 43 pages** — trade-table date cells
(`23/12/2024`) are a concrete example, taking the branch purely because
`/` hangs below baseline. §11 carries the amendment; the cap reference is
now the unflagged `Observed ∪ FromCapHeight` union with measured caps only.
Runtime work dispatched to worktree `wt-xhdesc`/branch `xh-desc`
(`f631a10` first spec; a follow-up for the amended cap reference is still
running as of this filing).

**Chunk specs committed this window, each against a verified prior-art
source (see `docs/measurements/2026-09-22_research_classical_techniques.md`
addenda and `ARCHITECTURE.md` §11 for full text — not restated here):**
chunk 13 (real-scan prototypes, `6c62cd1`); chunk 13b (per-page adaptive
prototypes, `9b70410`, verified against Kae et al. CVPR 2010 — a phantom
citation was caught and corrected in the same pass: "3 samples" and
"30-60%" attributed to Smith 2007 are not in that paper); chunk 13c
(rotated CAD text, `8e4ebd4`, verified against Tombre et al. 2002); chunk
14 (counted bigrams + lexicon union, `c820127`, verified absolute
discounting D=n1/(n1+2n2)); chunk 15 (junk-output amendment per LeCun
1998, `ab6cf7a`).

**Process miss, self-reported by the architect (commit `6edae2d` +
reconciliation entries).** The chunk 13/13b/14 specs were written without
first grepping `docs/measurements/` and git log for prior art already on
disk — a count-text stage and two addenda already existed and had to be
reconciled in afterward rather than written once. New standing rule
adopted by the architect for itself: grep measurements + git log before
any spec. Chunk 13c's own census was DROPPED (`84e062c`): pages-cov's
"drawing" category is drawing-*vocabulary*, not drawing-*layout*, so it
has zero rotated strings by construction — the wrong corpus to census
against.

**Chunk 14 built and reviewed (branch `chunk14`, off master `4ac2999`).**
6 commits, ACCEPTED with a fix: a case-folding bug in the lexicon union
gave 1555/889/437 instead of the corrected 1503/848/405 (§11 `f775399`).
Follow-up dispatched; val-split pick for chunk 14 waits until 12b closes
(runbook step 8, below).

**Chunk 16a-speed and 16b reviewed on their own branches.** `llm-speed`
(fka 16a-speed2, commit `fac7b34`): `score_candidates` verified bit-identical
to per-candidate scoring across 31 tests, ACCEPTED with a condition
(`forward_token`/`forward_tokens_batch` are two copies guarded by an
equality test, to be unified in 16b). `nbest` (`8bb5799`,
`recognize_lines_nbest`/`decode_word_nbest`, 204 core tests, wasm clean):
ACCEPTED, merge order fixed as nbest-before-case-geom (case-geom rebased
onto it after: `6404723`, ACCEPTED). `rescore` (16b shallow-fusion n-best
rescoring, built on `88f8447`, reviewed `c616715`→`9f2288e`→`0c0f86e`):
four bugs found and fixed in review — (a) the line-prefix built for line≥2
was missing a trailing newline, gluing two lines together for the LM; (b)
Off mode must not load the LLM or run the nbest path at all; (c) the
confidence cap must apply per word, not just to `LineResult`; (d)
`--llm-dump` added for offline grid-fitting. A tie-break rule (errors →
smaller λ → fewer changed lines → lower threshold, FULL tried last →
smaller |β| → smaller β) and a `--plain-cer` units/tolerance bug (fraction
with 5e-4 tolerance vs. percent-to-3dp) were both found and fixed in the
same review cycle. λ/β/threshold defaults (1.0/0.5/0.7) are flagged as
guesses awaiting a fit grid, not yet fitted values.

**Width-weighted decoder (chunk 12c) built and reviewed** on branch
`width-weight` (off `case-geom` `6404723`, commit `e28ab39` reviewed
accepted with a `Params::get` probe bug fixed in `cb07d71`) — verified
against Tesseract's `Rating=(1-match)×1.5×BlobLength`
(`adaptmatch.cpp:1415`) and the n-gram/classifier cost's
`outline_length/16` scaling (`language_model.cpp:910`); `Certainty` stays
unweighted in Tesseract, matched here. Merge note: the `Hyp` type gains an
`x_height` field as part of this branch, which touches other in-flight
branches.

**DPI finding (dpi-diag worktree, note cherry-picked `91d9cdf`, §11
`91005e2`).** pdfcer's "300dpi garble" smoke page (`scan.pdf`) is
200dpi-native (a 1700×2200 image on a 612×792pt page) upsampled 1.5× via
nearest-neighbour — aliasing, not a genuine resolution/blur problem. Fix
recommendation is filed against pdfcer (native-dpi rasterization, smooth
magnification), not against OCRcer.

**Smaller research addenda landed this window, each already in
`ARCHITECTURE.md`/`docs/measurements/` by commit hash — filed here as
pointers, not restated:** missing-cut/chopper candidate generation
(`57bc4e4`); footing detection (train truth 126 exact foots/13 pages,
IBM prior art expired, chunk-9 backlog, flag-first default off); fixspace
smallest-gap-first enumeration (`bc364df`); ISRI 1995's 0.5%-flagged bar
catching 20-45% of errors, and train truth EUR-sign share 6,692/986,633 =
0.68% (`ba8919e`); column-type numeric-run prior, 4,095/8,628 numeric-run
cells single-digit (`e668c63`); superscript re-read train truth (7
attached-digit markers + 76 † in 161,146 tokens, not queued, `e8c93b5`/
`474ff7d`); cross-line hyphen carry-over (all 18 train line-end hyphens
are compounds, not queued, `d75e6ac`); reverse-video/`invert_threshold`
addendum (train count 3/427 pages, ~0.015% of glyph-sized components,
`51a7408`); n-gram order reading (bits/char 5.225/3.800/3.060/2.694 for
uni/bi/tri/4-gram; look-alike preference in OOL letter words
88.78%→96.18%→98.52%, flat ~99.5% where a digit is present) and the
odd/even-split 4-gram leak (up to 0.27 bits/char inflation from adjacent-
page boilerplate, `eaf3a13`+`12d630d`); a style-consistent field
classification probe dispatched against Sarkar & Nagy PAMI 2005 (singlet
19.8%→LS 16.5%/14.9%→font-oracle 14.2%), branch `style-probe`, still
running as of this filing.

**Post-campaign runbook recorded, as amended (`ROADMAP.md` now carries
the full 9-step text — not restated here).** Amendments since the first
write-up: the `xh-desc` train gate moves to step 2, immediately after 12b
closes; the style-probe and reverse-video candidates are parked with
explicit triggers (CAD dev set / pdfcer report / census counter-fragment
insertions) rather than queued; `--user-patterns` (Tesseract-style CAD
callout bonus, e.g. `M8x1.25`) is parked until the CAD dev set exists.

**Not independently re-verified by this filing.** No shell in this
dispatch (per standing rule, see `feedback_no_shell_label_unverified` in
this agent's memory) — every commit hash, branch state and measured
figure above is filed as reported by `queue.md`, `ARCHITECTURE.md` §11 and
`docs/measurements/`, not re-run or re-grepped independently.

**Open, carried forward:** the `conf-tools` manifest-based split-refusal
fix (blocking bug found this window, follow-up running); the `xh-desc`
amended-cap-reference follow-up (running); the `style-probe` decision
(L=4 relative glyph-error cut ≥10% → candidate, <5% → park, 5-10% → weak-
park); tier-2's required end-of-campaign ablation; the chunk 14 val-split
pick; `ROADMAP.md`'s now-current branch inventory and merge order; the
`/usage` calibration, still outstanding.

---

## 2026-09-25 — Batch 2 (03:08–03:37): merge-train branches accepted, style-probe verdict lands, campaign cost knobs, research addenda

**Request.** File the architect's 03:08–03:37 findings into `ROADMAP.md`
and this log: branch follow-ups accepted and awaiting merge, the
style-probe verdict, campaign cost-knob progress, three research addenda,
a clippy regression, and queued follow-up work for `ocrcer-bench` and
`ocrcer-exporter`. No shell in this dispatch — nothing below is
independently re-run or re-grepped; it is filed as reported.

**Delivered, as bullets (full detail in `ROADMAP.md`'s Unmerged branches
and Backlog sections, not restated here):**

- `xh-desc` (`aab93b2`), `conf-tools` (`0ca2863`, `ce2a5f9`), and
  `chunk14` (`62f3ee5`, `ae150d9`, `fce1e01`) follow-ups all **accepted in
  review, awaiting merge**. `chunk14`'s grid point is still unpicked
  (waits on chunk 12b, runbook step 8); `xh-desc`'s train gate waits for
  the heavy slot (runbook step 2).
- New branch `pivot-index` (worktree `wt-pivot`, exact pivot bounds per
  §11 `9064842`) opened; agent still running. Merge order now ends
  `... → rescore (rebased) → pivot-index`, all after chunk 12b closes.
- **Style-probe verdict measured and merged** (`13fcca9`, `6342fc8`, merge
  `79cc056`, verdict `b9f8920`): label-style classification loses to 1-NN
  at every field length; leave-one-face-out at L=4, glyph error
  **+21.8%** (18 faces) / **+38.8%** (40 faces). Moved from "parked with
  trigger" to **parked, measured**.
- CAD park trigger reworded: reverse-video and `--user-patterns` now
  require **CAD dev set exists AND chunk 10 is active** — the dev set
  alone (150 lines, 0 collisions) would have satisfied the old wording,
  but chunk 10 stays deferred behind chunk 9.
- Campaign cost knobs (train, measured): `top_k` 3/5/8 = CER
  20.963/21.010/21.170 (`bab0a88`); campaign adopts `top_k=3`.
  `beam_width` {14, 24, 36} launched ~03:37, ~25 min/run;
  `campaign_post.py` queued next. **Chunk 12b stays open** pending vector
  pick, val confirm, one scoring pass, params fold-in, `fit-12b` merge,
  §11 close entry.
- Three research addenda filed to
  `docs/measurements/2026-09-22_research_classical_techniques.md`
  (pointers only, not restated): OCR-B/OCR-A licence-clean and
  charset-covered, MICR out of v1 (`950e1bd`); boxed slips anchor on the
  printed box number, not a form template (`e33f4a7`); Tesseract cuts
  candidates by distance, not count, feeding the `top_k` reading above
  (`bab0a88`).
- Clippy regressed from warnings-only to failing:
  `cargo clippy -p ocrcer-core --all-targets -- -D warnings` fails on
  master (~10 lib errors + test-target errors); pdfcer excludes vendored
  `ocrcer-core` from its own lint, so this project is the only linter.
  Queued: an `ocrcer-runtime` clippy-clean pass after the merge train,
  then a clippy gate in `ARCHITECTURE.md`.
- Queued for `ocrcer-bench` (after `conf-margin`+`conf-tools` merge):
  census buckets (m) rank/`d_c`/`d1`, (n) lexicon-word output-length
  errors, (o) spurious numeric-token spaces, (p) missing inter-word
  spaces.
- Queued for `ocrcer-exporter` (after the merge train): a no-flag
  `relanguage` run is content-identical but not byte-identical to its
  base (rebuilt tables appended rather than kept in place) — fix and add
  a base-sha256 reproduction test.

**Not independently verified by this filing** — no shell in this
dispatch; every commit hash, branch state, and measured figure above is
filed as reported by the architect's 03:08–03:37 findings.

**Open, carried forward, none duplicated here:** everything already in
`ROADMAP.md`'s Open questions section (stray `target-case` dir, merged
worktrees `wt-speed`/`wt-pdfcer`/`wt-dpi`, per-push approval, SROIE
licence, `fit-12b` resume) — all pre-existing items, checked against and
left as-is rather than restated.

---

## 2026-09-25 — Batch 3 (early hours): chunk 12b's vector-choice order corrected, `top_k` folds at 3, chunk 9 structure-layer spec (part 1), GriTS/ReMine scoring addenda

**Request.** File six more architect commits into `ROADMAP.md`: the fitted
feature-weight research addendum (waits for chunk 13), the §11 decision
fixing chunk 12b's vector-choice order ahead of the ablations report, two
scoring-methodology addenda (GriTS table structure; ReMine statement row
hierarchy) plus a wording-fix commit to them, and part 1 of the chunk 9
structure-layer spec. Correct three `ROADMAP.md` passages the vector-choice
decision supersedes. No shell available in this dispatch — `git show` was
not run; commit content was verified by reading the current text of
`ARCHITECTURE.md`, `PLAN.md` and
`docs/measurements/2026-09-22_research_classical_techniques.md` directly.

**Delivered:**

- **`d90daea` — chunk 12b's vector-choice order, fixed before the
  ablations report exists.** Corrected two passages in the chunk 12
  `ROADMAP.md` entry that had the fold order wrong or stale: `top_k` now
  reads "12b folds 3 outright, an LLM mode needing more candidates carries
  its own opt-in k, re-read at 5 after chunk 14's grid point"; the
  "picks the final vector..." line now reads in the §11-specified order —
  choice rule (A vs. B tier-2 revert, C `w_lex` 0.35, a combined stride-2
  confirm if both change, must beat control D) → val once against
  `Params::DEFAULT` (fail = no fold, diagnose on train) → fold
  `params.tsv`/`Params::DEFAULT` together, rows labelled `fitted` → merge
  `fit-12b` → score once (finfilings, pages-cov; chooses nothing) → closing
  §11 entry. The backlog's matching "Final top_k is settled together with
  the n-best ceiling figures" line was corrected the same way.
- **`bab0a88`** — already partly filed (backlog "Candidate shortlist
  width" bullet); amended in place to the corrected fold rather than
  duplicated.
- **`2a58702` — fitted feature-weight research addendum.** NCA (Goldberger
  et al. 2004) and LMNN (Weinberger & Saul 2009) prior art for the matcher's
  seven block weights; a diagonal fit maps onto the existing
  `feature_weights` table with no format change. Explicitly waits for
  chunk 13's forced-aligned glyph samples — fitting now would force a 12b
  and calibration refit, and synthetic glyphs are the wrong training data
  for it. Filed as a candidate with its own stated precondition, not as an
  open question.
- **`1c084aa` — scoring table structure (DAR, TEDS, GriTS).** Chunk 9's
  benchmark reports `GriTS_Top` and `GriTS_Con` side by side (Smock et al.
  2022); boxed forms score as field exact match, not a grid metric.
  FinTabNet named an uncleared scoring-only candidate.
- **`eb84fec` — financial-statement row hierarchy (ReMine, Chen et al.,
  ICDAR 2017).** Transitive parent-child F1 87.90 vs. an SVM pair
  classifier's 60.89 on their 72 tables; the row tree decides which cells
  should foot; OCRcer has three signals their HTML-derived input lacked
  (rules above totals, real pixel indent, stroke weight). Their 72-table
  set carries no stated licence — scoring-only if cleared.
- **`6ea346c`** — wording fixes to the footing and row-hierarchy addenda
  (flag attribution, the French "sous-total" total-label case); confirmed
  already reflected in the current addenda text read for this filing, not
  restated.
- **`01042a8` — candidate chunk 9 spec, part 1.** Filed into the existing
  chunk 9 roadmap entry, not a new section: the structure layer (9a
  substrate — rules, ruled cells, word-to-cell assignment, region list; 9b
  boxed forms, following 9a directly; 9c tables; 9d statements; 9e prose),
  structure never changes recognised text so every sub-chunk's gate is a
  structure gate. Candidate only — chunk 9 starts after the current
  runbook. Same commit corrects the boxed-slips addendum's box-number
  format: two or three digits then an optional capital letter, leading
  zero part of the key (T4 10-56 plus 16A/17A; T4A 014-211).
- **Open questions:** added item 16, FinTabNet and the ReMine 72-table set
  as scoring-only candidates pending Ken's clearance; existing items 1-15
  left untouched.
- **Campaign status, 03:59 (train stride 6, measured):** tier 4 accepted,
  CER 21.010; `top_k` 3/5/8 read (20.963/21.010/21.170); `beam_width`
  {14, 24, 36} sweep at `top_k=3` in progress; post chain queued behind
  it. No wall-clock figure is filed as a speed reading — the machine is
  shared.

**Not independently verified by this filing** — no shell in this
dispatch; commit hashes and the campaign status above are filed as given
in the dispatch brief, cross-checked only against the current text of
`ARCHITECTURE.md`, `PLAN.md` and the research-addenda file, not against
`git show` output.

**Open, carried forward, none duplicated here:** everything already in
`ROADMAP.md`'s Open questions section and the post-campaign runbook /
unmerged-branch inventory, unchanged by this filing except the
`top_k`/vector-order corrections and the new FinTabNet/ReMine item.

---

## 2026-09-25 — Batch 4: chunk 9a's two spec amendments plus a 9c addendum, campaign cost-knob results and a tie revert, pivot-index measured and accepted pending gate 3, reordered early-abandon research

**Request.** File six more items into `ROADMAP.md`: two chunk 9a spec
amendments (cell enumeration, the rule detector) plus a 9c research
addendum and the 9a-i dispatch status; the campaign's `beam_width`
cost-knob results and its `seg_split_penalty` tie-revert decision; the
measured pivot-index branch with the architect's review verdict; and a
research addendum on reordering early abandonment. No shell available in
this dispatch — commit content was verified by reading `ARCHITECTURE.md`
§11 and `docs/measurements/2026-09-22_research_classical_techniques.md`
directly, not by `git show`.

**Delivered:**

- **Chunk 9a spec amended twice (`5530430`, `43e18dc`).** Cells are now
  enumerated in Tabula's `findCells` order with a defined grid span
  (`rows`/`cols`) for 9c's `GriTS_Top`; stubs never split a cell; there is
  no joint-count minimum (a single closed box is a valid cell, gated only
  by 9b's `form.min_boxes`). The rule detector's length floor is fixed to
  `lines.rule_run_heights`, **5.4209, measured** — twice the longest
  straight ink run any bank glyph makes — and can never exceed it, so
  every band the underline strip erases is a detected rule by
  construction; a short run still counts as a rule when both ends touch a
  floor-passing rule of the other orientation, closing a one-line text
  box's sides without ever letting a lone glyph qualify; 1-px breaks join
  with no new parameter. Thickness cap `structure.rule_max_thick_h` starts
  at **0.8, a guess**. Tesseract's own text-density rejection test is
  counted on finfilings-train stride 6, not adopted, because adopting it
  in the detector alone would disagree with the strip. Nine new fixtures
  total across `structure` and `rules`.
- **9c research addendum (`03ffb68`), train-truth counted.** Camelot's
  stream/Nurminen text-edge method picks one text alignment per page,
  wrong for a statement's left-aligned labels and right-aligned numbers;
  Excel's accounting number format reserves a parenthesis-width space
  after positives, so ink edges and typeset edges disagree and alignment
  needs recognised text; a lone dash reads as zero. **Train count,
  finfilings-train, all 427 pages: 104 pages/295 lines carry 2+ number
  tokens; 47 parenthesised negatives on 13 pages; 7 lone dashes on 6
  pages** — table-shaped text is a minority of the corpus, so 9c's
  benchmark cannot come from finfilings alone. 9c's own spec still waits
  on these 9a readings.
- **9a-i dispatched, in progress, no result yet.** `ocrcer-runtime`,
  worktree `wt-struct`, branch `structure-9a`, pure `structure::build` on
  authored input. 9a-ii (the detector, wired into the pipeline, plus its
  params rows) waits for the 12b fold and the merge train.
- **Campaign `beam_width` results in (train, stride 6, measured):** 14 =
  CER 20.966 / LM-metric 23.211 / F1 70.782; 24 = CER 20.963 / LM-metric
  23.209 / F1 70.786 — identical to the `top_k=3` run, read as a
  determinism check, not a new finding. 36 still running as of this
  filing.
- **Tier 1's `seg_split_penalty` tie-move reverted before the ablations
  (§11, "a move made on a tie is reverted").** The campaign's tie rule
  changed mid-run on the 2026-09-24 resume (beat by more than `EPS`, not
  first found); `seg_split_penalty` (0.75→0.5, inner CER 22.318 identical
  both sides) was the one other tier-1 move made under the old rule. **A
  now starts at 0.75** — decided before any post-chain number exists, so
  this is the rule the campaign already ran under, not a new choice;
  tiers 2-4 and the cost knobs stand as train readings with 0.5 in the
  base. The fold's merge will also commit the fitting scripts
  (`tools/fit12b/`) and campaign logs
  (`docs/measurements/2026-09-25_fit12b/`) — every later fit commits its
  script before running.
- **Pivot-index branch measured and reviewed, ACCEPTED pending gate 3**
  (worktree `wt-pivot`, commits `ce32a96`/`3c03111`/`05e1f76`, architect
  edit `73eb957`). Exact LAESA-style per-class pivot bounds in the
  matcher. **Measured:** 29,989 captured queries, 0 mismatches,
  byte-identical output on 3 pages; dims summed per query down ~29%,
  prototypes visited down ~40%; model load 38.6→51.3 ms; wall time down
  3–7% (indicative, shared machine). The report's own explanation for the
  wall-time shortfall was relabelled **not measured** in the same review.
  Gate 3 (stride-6 train byte identity) is the architect's own, run after
  the campaign closes.
- **Research addendum: reordering early abandonment (`e657d2b`,
  `52e2edb`).** The matcher's last early-abandon checkpoint lands at
  dimension 95 of 107, so the hole count, six crossings and four geometry
  dimensions (geometry weighted 6.0, the only block separating case pairs)
  never help abandon a candidate. Reading (pivot counters, arithmetic not
  measurement): dims per visited prototype ~50→59 and `match()` ns/dim
  ~2.8→3.8 under the pivot branch; the per-query pivot pass itself
  (~20,000 dimension ops) is small against ~0.93 M dims summed per query
  (dims over captured query count — corrected from an earlier ~1.5 M
  estimate), so it doesn't explain the rise; cause not measured. Candidate
  follow-up, not measured, queued after the merge train on top of
  pivot-index: reorder summation, with any surviving candidate re-summed
  in file order so output stays byte-identical by construction.

**Not independently verified by this filing.** No shell in this dispatch;
every commit hash and measured figure above is filed as reported in the
dispatch brief, cross-checked only against the current text of
`ARCHITECTURE.md` §11 and the research-addenda file, not against `git
show` output.

**Open, carried forward, none duplicated here:** everything already in
`ROADMAP.md`'s Open questions section and the post-campaign runbook /
unmerged-branch inventory, unchanged by this filing except the chunk 9
amendments, the campaign status, and the pivot-index verdict above.

---

## 2026-09-25 — Batch 5: architect session, continued

**Request.** A librarian filing for research and status landed on `master`
and on the `fit-12b` branch since the last batch, covering commits,
chunk 12b's post-campaign status, a chunk 9a-i acceptance, the merge-train
dry run, the operator's accounting-first priority, two pdfcer hand-off
candidates, and a set of queued-not-specced measurement arms. Scope for
this filing was `ROADMAP.md` and `SESSION_LOG.md` only; `ARCHITECTURE.md`
was not touched (its §11 entries were already written by
`ocrcer-architect`).

**Commits landed on `master` since the last batch, all research or
decision-log entries — none change the engine.** `c1f5b37` whole pages
scanned sideways or upside down: a Tesseract-OSD-plus-Leptonica-flip test
was proposed; the train check found no landscape images. `6804e98` and
`9bd08e9` CUSIP/ISIN check digits: 1,763 of 1,773 CUSIP-shaped tokens pass
on 52 train pages (measured, train); flag-only for now; a summariser error
in an earlier count was corrected. `88a1b86` and `020faf2` PDX format:
dimension-major prototype blocks must keep each distance's summation
order; layout gets measured before any reordering. `3af14a9` decision-log
entry: chunk 9a-i (table cells from ruling lines) reviewed. `f3a2759`
GD&T/hole-callout symbols: 27 codepoints, none in the charset; a probe
runs before any charset change. `d33d40f` stacked tolerances on drawings:
likely interleaved by line grouping; a probe runs before any spec.
`c321418` tabular digits: an ink-gap space test cannot separate "11" from
"1 1" in five bank faces, but a centre-distance test separates them in all
53 (measured, font metrics, `tools/digit_pitch.py`). `62fd2b9` GD&T font
census: all 27 drawing symbols have at least two licence-clean faces
(Noto Sans Symbols family, STIX Two Math, already in the bank) —
coverage only, no class added, the probe stays on hold. `f3687dc` faxed
pages: a source with unequal axis resolutions (standard fax ≈204×98 dpi)
is squared at the larger resolution and magnified smoothly (§8.1
clarified, a §11 entry, a research addendum); pdfcer's own renderer
repeats fax rows at or above the fax's horizontal dpi (read from pdfcer
source, not run). `0cce2e2` red stamps over invoice text: pdfcer passes
luma, which keeps a stamp as ink, as does Tesseract's per-channel union;
a max-channel dropout would remove the stamp but also erases red negative
amounts, so no default is set; a synthetic train-only reading is queued.
`2b36d9b` highlighter and shaded rows: Sauvola marks the edge of a
mid-grey band as ink — measured on synthetic lines, pink turns "00417"
into "OOÿ17", grey 150 and darker merges words; finfilings-train shading
(grey 191, measured train) is lighter than that onset; three candidate
fixes were drafted, none chosen; the generator is committed as
`tools/highlight_lines.py`. On branch `fit-12b`, not `master`: `cff5a92`
adds a column-key description of the `fitted` provenance to `params.tsv`.

**Chunk 12b post-campaign status — all readings TRAIN, stride 35 unless
stated, no val or score yet.** The campaign proper finished about 04:43 (log-file time):
`beam_width=36` (train, stride 6, measured) read the same as 24 — CER
20.963, line-matched CER 23.209 at the campaign's confirm stride — so
beam width is read as saturated. The post chain (edge-parameter walks,
then a stride-6 confirm, then four stride-2 A/B/C/D ablations) started at
04:44 and is still running as of this filing. Edge walks so far, train,
measured: `decode.w_seg` 0.475→0.55 accepted; `segment.max_merge_x_heights`
walked 1.5→1.35→1.2→1.05, taking CER 21.298→19.941, line-matched CER
19.841→18.593, word F1 74.059→81.731 — the largest single move in 12b so
far, still improving at the walk's last step, edge not closed. No other
walked parameter moved. A risk check on the `max_merge_x_heights` move,
measured on synthetic clean lines (10 shippable Regular faces × 2 sizes):
1.8/1.5/1.05 all emitted '%' 116 of 120 times, identical — the concern
that a wider merge would start eating '%' is retired on synthetic text.
Whether broken glyphs on real scans suffer the same way is unmeasured and
will only show up in the `pages-cov` score, which must be reported
prominently when it runs. Outcome, val and score are deferred to batch 6.

**Chunk 9a-i follow-up accepted.** It merges last in the merge train, as
`structure-9a`. One nit carries into the 9a-ii dispatch: a comment in
`cells.rs` uses history-referencing wording ("no longer matters") that
gets reworded per the documentation rule against writing history into
source comments.

**Merge-train dry run.** After the 12b close and fold, the planned order
is `llm-speed`, `nbest`, `case-geom`, `conf-margin` (+`conf-tools`),
`width-weight`, `chunk14`, `rescore` (rebased), `pivot-index`, with
`structure-9a` last. The dry run found only mechanical union conflicts:
the `viterbi.rs` test tail; `ocrcer-build`'s `main.rs` module `use` list;
`ocrcer-bench`'s `lib.rs`/`ocr.rs` (`dumpread`+`dump`, `char_dump`+`llm`);
and `ocrcer-bench`'s `Cargo.toml` `[[bin]]` entries.

**Operator priority, restated (Ken, 2026-09-21, verbatim): "we need to
support everything that an accounting firm would need before we continue
with supporting drawings."** The drawing-callout probes (GD&T symbols,
stacked tolerances) stay on hold under this. This session's research was
redirected to accounting-paper degradation instead: fax resolution,
stamp overprint, highlighter/shading damage.

**Two candidate hand-offs to pdfcer, for Ken to relay** (OCRcer never
edits pdfcer's tree directly). Fax: pdfcer's image renderer picks one
resampling filter per image and uses Nearest unless either axis
minifies, so a standard-mode fax page rendered at or above roughly its
own dpi has each row repeated; per the §8.1 clarification, the fix is to
square at the larger resolution and smooth-magnify the other axis; this
was read from pdfcer's source, not run, and is ready to relay. Stamps and
highlighter: a pdfcer colour-mode option (luma default, an opt-in
max-channel dropout) is a candidate, but it is **not** ready to hand off
— it waits on the queued train readings below.

**Queued measurements, all on finfilings-train only, all behind the 12b
fold and the 16b runs — queued, not specced.** A nearest-neighbour
upsampling reading plus fax arms F0–F4. Stamp arms S0–S3 plus a
red-negative collateral arm. Highlighter arms H0/H1 (bands at luma 230,
195, 182, 168 over amounts). A candidate bench probe bin — the
highlighter/grey ladder — that every future binarization change must
pass, for `ocrcer-bench` after the merge train.

**Delivered.** This entry; the corresponding `ROADMAP.md` updates to the
chunk 12 in-progress section, the research-leads backlog, and two new
Open questions items (17: the fax hand-off plus the not-yet-ready
stamps/highlighter note; 18: an LLM preview build off `rescore`, flagged
with no further detail given in this filing's brief). The `ARCHITECTURE.md`
§11 entries themselves are not restated here — they are the record;
see §11 directly. `tools/digit_pitch.py` and `tools/highlight_lines.py`
are pointed to, not reproduced.

**Not independently verified by this filing.** No shell in this dispatch;
every commit hash, branch name and measured figure above is filed as
given in the dispatch brief, not cross-checked against `git show`,
`git branch`, or `/usage` output.

**Token spend.** Not measured this session — no `/usage` reading was taken
and none was supplied in the dispatch brief. This is calibration debt
carried forward, not a filed figure; do not treat its absence as zero
spend.

**Open, carried forward, none duplicated here:** everything already in
`ROADMAP.md`'s Open questions section, including the two items added by
this filing, and the merge-train order and 9a-i status lines in
`ROADMAP.md`'s chunk sections, which this filing's scope did not extend
to updating (see this batch's report for the resulting staleness).

---

## 2026-09-25 — Batch 6: architect session, continued (~05:50-07:10, wrap-up before an operator PC restart)

**Request.** A librarian filing at session wrap-up, dispatched because the
operator was about to restart his PC. Scope was `ROADMAP.md` and
`SESSION_LOG.md` only, `SESSION_LOG.md` append-only; `ARCHITECTURE.md` was
not touched, its §11 entries already written by `ocrcer-architect`. Source:
`D:/Dev/ExcludedPrivate/ocrcer/handoff_2026-09-25/queue.md`, read in full.

**Commits landed since Batch 5, all research addenda or a §11 decision —
none change the engine.** `f15277d` dot-matrix print: separated dots break
layout (4 lines read as 8/3/9 at 0.6-pitch dots), no fixed join works
across dot pitches, the best join tracks the inter-dot gap — measured on
synthetic lines only, generator `tools/dotmatrix_lines.py`. `e2ba601` faded
ink: text lighter than about grey 175 on white (160 on off-white) returns
zero words, a silent loss that looks like a blank page — measured on
synthetic lines only, generator `tools/faded_lines.py`. `c3d65f5`
show-through: the shipped Sauvola `k` ignores mirrored back-side print
down to grey 185, but every faded-ink recovery lever reads it too —
measured on synthetic lines only, generator `tools/showthrough_lines.py`.
`9f15013` pen marks: a hand underline or circle whose column range covers
a figure glues the figure and the mark into one glyph and the figure is
lost, touching or not — measured on synthetic lines only, generator
`tools/pen_marks_lines.py`; this commit also corrects an earlier
show-through sentence (`o0417` is the baseline read on a clean page, not a
local-stretch side effect). `535f913` §11 decision: show-through handling
has four modes (auto-detect default, auto contrast, manual contrast, off),
stage 1 a page flag, stage 2 a mirror-test recovery, each behind its own
gate — plus a research addendum on the mirror cue itself, measured on
synthetic crops only.

**Chunk 9 staleness fixed.** `ROADMAP.md`'s "9a-i dispatched, in progress"
line was stale — the follow-up (`a44d24e`, 05:13) was accepted: all four
changes from the `3af14a9` review entry landed, 5 fixtures hand-checked,
11 old fixtures unchanged, `ocrcer-core` 211 pass, bench structure 9 pass,
wasm32 exit 0. One nit (a history-referencing comment in `cells.rs`)
carries into the 9a-ii dispatch. The merge-order line now has
`structure-9a` appended last, per a 05:05 dry run that found the same four
mechanical union conflicts already on record from Batch 5 (no new
conflicts).

**Chunk 12b post-chain status at the ~07:10 restart.** Phase 1
(edge-parameter walks), stride 6: baseline CER 20.978 → tentative 19.631,
main mover `segment.max_merge_x_heights` 1.8→1.05, edge not closed — a
different stride from the stride-35 figures already filed in Batch 5, not
in disagreement with them. Phase 2 (stride-2 A/B/C/D ablation) completed
only `A_final`: CER 21.418, LM-metric 24.119, F1 77.705, **not comparable
to any stride-6 or stride-35 figure on file.** `B_tier2_reverted`,
`C_wlex_035` and `D_control` were interrupted by the restart before
running. Val, fold, merge, the §11 close and the score all remain undone.
`campaign_post.py` has no resume flag; a resume script needs writing
before the reruns, loading `A_vector` from `fitlogs/post_status.json`. One
heavy `ocr.exe` process at a time, roughly 53 minutes per run.

**Campaign-finish-time correction, confirmed still correct.** Batch 5
already corrected the campaign's finish time from an earlier 04:49 report
to ~04:43 (`campaign_stdout.log` mtime) before that entry was committed;
this filing re-checked both `ROADMAP.md` and the Batch 5 entry above and
found 04:43 throughout, no stale 04:49 anywhere in either file. No further
correction needed.

**Leading-zero finding and its diagnosis.** `00417-229` reads as
`o0417-229` on every black (non-degraded) page, found incidentally during
this research family, not itself a pen-marks or show-through artefact.
Diagnosed via a dispatched agent (`diag-zero`, worktree `wt-zero`, branch
`diag-zero` off `c3d65f5`) that stopped before recording a result; the
worktree is clean. Redispatch is queued, not done this filing.

**Backlog gained an extensive queue of research-family arms**, filed to
`ROADMAP.md`'s Backlog as a new "Queued measurements, research family"
subsection: fax arms F0-F4, stamp arms S0-S3 plus a red-negative
collateral arm, highlighter arms H0/H1, a bench probe bin, a dot-matrix
census spec, a dot-grid licence check, a synthetic noise arm, a
"no words over non-blank grey" diagnostic, an edge-sharpness count, a
pen-marks-into-negative-family spec, an atoms-bound count, per-word
mirror scores (probe already run, not analysed —
`probe_data/pw_k34.tsv`, `pw_k10.tsv`, harness in `hl_probe_src/`, all
under the private handoff directory), a show-through-overlap measurement,
and a mirror false-fire count. None run yet; all behind the 12b fold.

**Open questions 13-16 unchanged.** Question 17 (pdfcer hand-offs) gained
a third candidate note: the show-through four-modes design has a
page-level flag pdfcer would need to surface, but this is **not** ready to
relay until stage 1 (the page flag) ships and passes its own gate.
Question 18 (LLM preview build) is clarified: it means a build of the LLM
re-reading path off `rescore`, offered to Ken this session; still awaiting
his own scope and go, not yet a yes.

**`ROADMAP.md`'s *In progress* section gained a "Resume after restart"
block** at its top, five ordered points: rerun Phase 2's B/C/D ablations
(needs a resume script first); then `decide_12b.py` → val → fold → merge
→ close §11 → score with the `pages-cov` delta prominent; redispatch
`diag-zero`; run the branch merge train with `structure-9a` last; analyse
the per-word mirror probe. Session state for all of this lives at
`D:/Dev/ExcludedPrivate/ocrcer/handoff_2026-09-25/`, private, never
committed.

**RAG escalation, this role's own remit.** Four findings generalizing
beyond OCRcer were written to `C:\personal_rag\ocr\` (subject already
existed, no bootstrap needed): geometric-mean word confidence hides one
garbage glyph inside an otherwise-fine-scoring word; the shipped Sauvola
`k` ignores show-through but every faded-ink recovery lever reads it; a
mirror-flip test separates whole mirrored show-through from real text by
confidence but not at fragment granularity; and the i-dot
column-range-containment merge rule also glues a pen mark to the figure
it covers. All four labelled "measured, synthetic lines/crops, one face."
`C:\personal_rag\ocr\index.md` and the master `C:\personal_rag\index.md`
both got one-line pointers. No `C:\Users\Ken\.claude\CLAUDE.md` flag
needed — `personal_rag/ocr` is already listed there as a current subject.

**Not independently verified by this filing.** No shell in this dispatch;
every commit hash, branch name, agent name and measured figure above is
filed as given in `queue.md`, cross-checked only against the current text
of `ROADMAP.md`, `SESSION_LOG.md`'s own prior entries and (for §11
pointers only) `ARCHITECTURE.md`'s section headers — not against `git
show`, `git branch`, or `/usage` output.

**Token spend.** Not measured this session — no `/usage` reading was taken
or supplied in the dispatch brief. Calibration debt carried forward, same
as Batch 5; do not treat its absence as zero spend.

**Delivered.** The `ROADMAP.md` edits listed above (Resume-after-restart
block, chunk 9 and merge-order staleness fixes, the chunk 12b Batch 6
bullet, the leading-zero and five addenda entries, the new queued-arms
Backlog subsection, and the question 17/18 updates); this entry; four new
`C:\personal_rag\ocr\` lesson files plus their two index pointers.

**Open, carried into the next session:** everything under *Resume after
restart* above — none of it is done, all of it is queued for the first
session after the restart.

---

## 2026-09-25 — Batch 7: librarian filing, chunk 12b closed

**Request.** File chunk 12b's closure (val once, fold, merge, score once,
its two regressions and the identifier-test gap), the chunk 15
Python-trainer/parity-fixture supersession, and the leading-zero diagnosis
plus its `xh-desc` re-test, from three dated `ARCHITECTURE.md` §11 entries
and three measurement files; update the resume runbook; escalate two
generalizable findings to `C:\personal_rag\ocr\`; commit `ROADMAP.md` and
`SESSION_LOG.md` only.

**Reconnaissance performed.** Read the three named §11 entries in full
(candidate neural probe; chunk 15 trainer supersession; chunk 12b closure,
with its score table and loss/gap sections). Read
`docs/measurements/2026-09-25_score_12b.md` and
`docs/measurements/2026-09-25_mirror_per_word.md` in full (both already
read the prior session, re-confirmed this one). Located and read
`docs/measurements/2026-09-25_leading_zero.md` directly off disk in the
`wt-zero` worktree (`D:/Dev/ExcludedPrivate/ocrcer/wt-zero/docs/measurements/`)
— a Bash tool was available this dispatch after all, but the file was
already checked out as a plain committed file on that branch, so a
filesystem `Read` was used in preference to `git show` once located; no
content in it is second-hand.

**Chunk 12b closed, shipped with its losses filed alongside the win.**
Vector B (tier 2 reverted, `w_lex` default) chosen under the pre-registered
rule; val once CER 24.325→22.082 (line-matched 28.871→26.902); fold 11 rows
to `fitted` (`4533f79`); score once, finfilings CER 12.167→11.429
(line-matched 11.122→10.488, WER 28.237→26.013), pages-cov CER 5.900→5.422
but **WER 26.114→26.338 and F1 78.093→77.866 both worse**
(`docs/measurements/2026-09-25_score_12b.md`, `e1fa8cf`). Both
pre-registered gates held (pages-cov CER ≤5.950, drawing Δ≤0 at −0.073).
Three mechanisms behind the losses: monospace `i`→`í`/`î` (154 occurrences,
absent from control's top-12); word fusion on short-token lines (`DO NOT
SCALE DRAWING`→`DONOTSCALEDRAWING`); and one identifier corruption
(`M8x1.25`→`IV18x1.25`) — the exact harm `CLAUDE.md` rule 6 names, arriving
through segmentation rather than the lexicon. A gap was filed as its own
finding: no corpus-level identifier-preservation test exists, only two
unit tests of `is_identifier` — `ocrcer-bench` owns building it, seeded
with `M8x1.25` on the Noto Sans drawing line, as a gate for every later
chunk.

**Chunk 15's trainer may now be Python, under five conditions replacing the
outright ban** — Rust-only inputs and inference, a forward-pass parity
fixture blessed under §8.2, pinned-version CPU reproducibility, GPU for
exploration only, trainer confined to `tools/nn/` and never shipped. A
throwaway neural probe (PyTorch, weights never committed) precedes chunk
15 itself, testing whether a network beats the prototype matcher by enough
to justify building it; in flight on branch `nn-probe`, unmeasured as of
this filing.

**The leading-zero misread is diagnosed and independently re-confirmed
fixed on a four-page sample, not yet gated.** `diag-zero`'s own diagnosis
traced `00417-229`→`o0417-229` to `layout/lines.rs::measure()`'s
width-weighted x-height vote locking onto a digit-heavy identifier line's
digit population instead of its true lowercase x-height. The file's own
2026-09-25 addendum then tested the existing `xh-desc` branch's
`descender_cap_check` — already implemented by `ocrcer-architect`/
`ocrcer-runtime` as the diagnosis's own candidate fix 1, not a fresh
design — against the exact repro line in a throwaway worktree off
`xh-desc`: fixed on all four reproducing pages tested, plus two
previously-uncited words on the same pages (`Ref`→`Ret`, `forward`→
`torward`). This is a four-page diagnostic result, not the
pages-cov/finfilings corpus gate `xh-desc`'s own commit requires before
merge — that gate is now the immediate next runbook step, unblocked by
chunk 12b's closure.

**RAG escalation, this role's own remit.** Two findings written to
`C:\personal_rag\ocr\` (subject already existed): fitting segmentation
parameters on a real-scan train split improved CER on both real and
synthetic corpora while regressing WER on the synthetic one, via word
fusion and a false diacritic — a CER-only choice rule can hide a
word-level loss the same fitting run caused; and a per-word show-through
mirror cue that separates cleanly at whole-page granularity does not
survive being applied to a single-word crop, because an isolated crop
starves the layout stage's x-height/baseline/slant estimate the cue
implicitly depends on. Both labelled "measured, synthetic data, one face."
`C:\personal_rag\ocr\index.md` and the master `C:\personal_rag\index.md`
both got one-line pointers. No `C:\Users\Ken\.claude\CLAUDE.md` flag
needed — `personal_rag/ocr` is already a listed current subject.

**Token spend.** Not measured this session — no `/usage` reading was taken
or supplied. Calibration debt carried forward, same as every prior batch;
its absence is not filed as zero spend.

**Delivered.** `docs/ROADMAP.md`: the resume runbook rewritten against the
five now-resolved points, a new "Chunk 12b closed" entry with the full
score table and loss/gap writeup, the chunk 15 trainer-supersession
amendment, the `fit-12b`/`xh-desc` unmerged-branch rows updated, and the
post-campaign runbook's steps 1–2 marked accordingly. This
`SESSION_LOG.md` entry. Two new `C:\personal_rag\ocr\` lesson files plus
their two index pointers.

**Open, carried into the next session:** the branch merge train
(`structure-9a` last); the `xh-desc` train gate itself (corpus gate, not
yet run); the corpus-level identifier-preservation test (`ocrcer-bench`);
the i→î and word-fusion diagnosis (`ocrcer-runtime`, finfilings-train +
synthetic only); the `nn-probe` neural-probe result, still in flight.

---
