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
