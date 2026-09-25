# OCRcer — staged build plan and budget

Companion to `docs/FEASIBILITY.md` (why this is being built and what it will
cost in accuracy) and `docs/ARCHITECTURE.md` (what is being built).

---

## 1. The budget model, and how to check it against reality

**The constraint:** no chunk may consume more than **10% of a weekly budget** on
the $200/month Max plan.

**The caveat first.** Anthropic does not publish weekly limits as token counts.
They are enforced as usage hours, they differ by model, and they move. The
figures below are a *planning model*, not a measurement. Correct them by running
`/usage` after the first real chunk and rescaling. Chunk 1 is the calibration
run.

**The working model.** On the Max 20x tier the weekly allowance is roughly
24–40 hours of Opus-class work and roughly ten times that of Sonnet. Counting
only *billable* tokens — fresh input, output, and everything subagents consume,
excluding cache reads — a focused session with active subagents runs somewhere
around 400–800 K tokens per hour. That puts 10% of a week at roughly
**1.0–3.2 M billable tokens**. Every chunk below is sized to a **1.5 M ceiling**,
inside the pessimistic end of that range, leaving headroom for debugging.

**The lever that makes this work: Opus decides and reviews, Sonnet subagents
write.** The Sonnet pool is about ten times the Opus pool, so Opus hours are the
only binding constraint. A chunk that does its bulk implementation in the main
Opus session costs roughly four times as much of the scarce resource for the
same output.

**What this project does not spend.** Constructing the model is a script that
finishes in minutes, so no chunk waits on a machine. And because every pipeline
stage is written once, in Rust, no chunk spends its budget reproducing work a
previous chunk already did in another language.

---

## 2. Chunks

Each chunk has a machine-checkable exit gate. A chunk is done when its gate
passes, not when its code is written.

| # | Chunk | Agents | Model | Est. tokens | Exit gate |
|---|---|---|---|---|---|
| 0 | Bootstrap: feasibility, architecture, plan, agent roster | — | Opus | ~350 K | Design docs exist; seven agents load |
| 1 | Workspace, charset, feature extractor, fixture harness | `ocrcer-architect`, `ocrcer-glyphs` | Sonnet | ~0.9 M | Three crates build; `ocrcer-core` compiles for wasm32; charset frozen; extractor produces 107-dim vectors; the harness fails loudly when a fixture is deliberately altered |
| 2 | Image pipeline: binarize, deskew, components, lines, words | `ocrcer-runtime` | Sonnet | ~1.1 M | A rendered page produces golden-checked components, lines, baselines, x-heights and word boxes; wasm32 still green |
| 3 | Prototype bank construction and the `.ocrw` writer | `ocrcer-glyphs`, `ocrcer-exporter` | Sonnet | ~1.4 M | Bank builds in under 5 min and byte-identically on re-run; 1-NN classifies isolated rendered glyphs at over 99%; file round-trips; int8 top-1 agreement measured and reported; the reader skips an unknown table name without error and refuses an unknown `version` (`ARCHITECTURE.md` §7); a test asserts the emitted `meta` keys and table names against `ARCHITECTURE.md` §2 and §7.1; `meta.faces` carries `licence` and `licence_source` per face (§7.1); and the bank's scale ladder is an authored rule rather than a benchmark sweep's winner |
| 4 | The authored model: lexicon, bigrams, confusions, parameters | `ocrcer-linguist` | Sonnet | ~1.5 M | Tables compile into the model file and load; decoder beats the bare classifier by a measured margin |
| 5 | Pruning and prototype matching | `ocrcer-runtime` | Sonnet | ~0.9 M | Top-1 class and margin golden-checked over 10 K glyph fixtures; pruning provably never discards the eventual winner on that set |
| 6 | Lattice, Viterbi, lexicon, confidence | `ocrcer-runtime` | Sonnet | ~1.0 M | Whole-page strings and confidences match fixtures on x86 and wasm32; identifier-preservation test passes |
| 7 | pdfcer integration | `ocrcer-runtime` | Opus/Sonnet | ~0.7 M | `OcrEngine` implemented; `reports_confidence()` true; wasm32 CI gate green |
| 8 | Benchmark harness, head-to-head vs `ocrs`, tuning rounds | `ocrcer-bench` | Sonnet | ~1.2 M | CER/WER table across four document classes, both engines, plus a calibration curve; and every reported headline states the bank configuration it was measured at, asserted against the emitted model file's `meta.sizes` rather than against a command-line argument; and every generated benchmark page passes the corpus-generator guards in `ARCHITECTURE.md` §8.2 — no alphanumeric truth box two pixels tall or less, one closed contour drawn as one connected component in coverage — with a failure refusing the page rather than dropping the cell |
| 9 | Accounting/business document structure: ruled & columnar tables, financial-statement shape, boxed forms (T-slip-style), plus prose structure — rescoped 2026-09-21, prioritised ahead of chunk 10 | `ocrcer-runtime` | Sonnet | ~1.2 M (pre-rescope estimate, flagged stale — see section 2a) | See section 2a: ledger/journal/statement/boxed-form/prose fixtures plus an arithmetic self-validation fixture; harness fails loudly on deliberate alteration |
| 10 | Drawing primitives: line tracing, line-type classification, shape fitting (sibling crate `ocrcer-draw`, proposed) | `ocrcer-runtime` | Sonnet | ~1.4 M | Traced polylines, ISO 128/ASME Y14.2 line-type classification, and fitted circles/arcs/rectangles golden-checked against fixture drawings |
| 11 | Evaluation corpora: acquisition, licence table, ground-truth normalisation | `ocrcer-bench` | Sonnet (prep/normalisation), Opus (adjudication) | ~0.8 M | Corpus table populated with licence-as-read-from-source and a redistributable column per candidate; chunk 8's CER/WER harness fed only from admitted corpora |
| 12 | Training split manifest, then fitting the `guess` params on the training split | `ocrcer-bench` (split), `ocrcer-linguist` (fit) | Sonnet | ~1.0 M | Committed manifest disjoint from every scoring page; each fitted param labelled `fitted` with script+manifest; pages-cov, drawing, finfilings gates pass |
| 12c | Width-weighted decoder terms (candidate; follows chunk 12's fit): mode `decode.width_weighting`, then a refit of the terms it rescales | `ocrcer-runtime` (mode), `ocrcer-bench` (refit) | Sonnet | ~0.5 M (estimate) | Mode 0 byte-identical on every fixture; refit on train only, confirmed once on val; usual gates including drawing Δ ≤ 0; merged and split edges per 1,000 characters reported on and off |
| 13 | Real-scan prototypes: ground-truth-aligned glyph crops from the training split added to the bank (after 15; must beat naive append, 89.61% probe top-1, and be compared to the network — §11 2026-09-25) | `ocrcer-glyphs`, `ocrcer-bench` (alignment) | Sonnet | ~1.3 M | Deterministic crop extraction through core's own segmenter and extractor; a test refuses any non-train page; crops accepted only under `ARCHITECTURE.md` §11's 2026-09-25 acceptance tests (truth folded through the charset, per-class rendered-spread bound, neighbour agreement); rows record source page; usual gates plus an ECE re-measure; wall time and file size reported |
| 13b | Per-page adaptive prototypes (candidate): second pass over the page with prototypes taken from its own clean-list words | `ocrcer-runtime` (overlay, second pass), `ocrcer-bench` (measure) | Sonnet | ~1.0 M (estimate) | `adapt.enabled` off byte-identical; clean-list selection per `ARCHITECTURE.md` §11 2026-09-25, never confidence; a promoted-word fixture; mean CER and the count of pages made worse both reported; usual gates; wall time reported |
| 13c | Text rotated ±90° on drawings (candidate) | `ocrcer-runtime`, `ocrcer-bench` (census, measure) | Sonnet | ~0.8 M (estimate) | Transpose-equivariance property test; `layout.rotated_text` off byte-identical; finfilings-val no worse than control + 0.02; drawing gates; pages-cov rotated-string census reported as counts only |
| 14 | Domain lexicon and bigrams counted from training-split text | `ocrcer-linguist` | Sonnet | ~0.8 M | Counts from training split only (count mixture with absolute discounting, μ and the lexicon rule chosen on val per `ARCHITECTURE.md` §11 2026-09-25); attribution in `meta`/`NOTICE`; identifier-preservation test passes; usual gates |
| 15 | (Scheduled before 13, §11 2026-09-25 neural-probe result) Neural glyph classifier (optional matcher, `match.classifier`), pure-Rust inference in core, Python/PyTorch trainer in `tools/nn/` on Rust-extractor dumps with a Rust↔PyTorch forward-parity fixture (§11 2026-09-25) | `ocrcer-runtime` (inference), `ocrcer-glyphs` (training data + trainer) | Sonnet | ~2.5 M, two sittings | See `ARCHITECTURE.md` §11 2026-09-24 neural-classifier entry; core invariants intact; byte-reproducible CPU training at pinned versions; parity fixture passes; beats kNN-only on finfilings with pages-cov and drawing gates passing |
| 16 | Optional local LLM add-on: `ocrcer-llm` crate (pure safe Rust, std only, in-process), one-file `.ocrl` add-on (weights + tokenizer + licence), n-best rescoring of low-confidence lines | `ocrcer-runtime` (engine), `ocrcer-exporter` (`.ocrl` + converter), `ocrcer-bench` (measure) | Sonnet | ~2.0 M, two sittings | 16a: logits match a reference implementation, tokenizer matches on a test set; 16b: rescoring beats the controls on finfilings with the identifier-preservation test passing and drawing Δ ≤ 0; wall time and add-on size reported |
| 16c–e | LLM add-on, continued: (c) Qwen3.5-0.8B text path (Gated DeltaNet + gated attention); (d) its vision encoder, for image-conditioned rescoring of n-best candidates on line crops; (e) optional GPU backend (`gpu` feature, wgpu, uses the system's existing graphics driver) | `ocrcer-runtime`, `ocrcer-exporter`, `ocrcer-bench` | Sonnet | ~3.0 M, three sittings | Each stage: logits/embeddings match the reference; fits 8 GB RAM and 2–4 GB VRAM with measured peaks; 16b's gates apply to any OCR use; CPU path stays the default and the fallback |

**Chunk 3 status, 2026-09-21: three of its five gate clauses pass.** Build
time, byte-identical rebuild and `int8` agreement are measured and pass; the
`.ocrw` file round-trip has no reader yet; and 1-NN on isolated rendered
glyphs measures 90.95% to 92.48% against a gate of over 99%. The chunk is not
done. `ARCHITECTURE.md` section 11's three 2026-09-21 bank entries carry the
measurements and what the shortfall is made of.

**Two clauses added to this chunk's gate on 2026-09-22**, after an audit read
the emitted `.ocrw` against `ARCHITECTURE.md` and found five places the
document described an intention the writer never implemented. A round-trip
test cannot catch that, because it writes and reads the same tables; nor can
it exercise the forward-compatibility rule section 7 now states. So the gate
also requires a reader that skips an unknown table name and refuses an unknown
`version`, and a test that asserts the emitted `meta` keys and table names
against the specification. `ARCHITECTURE.md` section 11, 2026-09-22, three
entries: the format audit, the `version`-versus-table-name rule, and the face
manifest's licence fields.

**Two further clauses, same date, and both are about what makes a file
shippable rather than merely well-formed.** The first: **`meta.faces` must
carry `licence` and `licence_source` per face.** The model file travels
without this repository — it ships into pdfcer as a single artifact — and
`CLAUDE.md` rule 2 stakes the entire licence position on the tables being
derived only from unambiguously permissive faces. Today that is checkable only
by someone holding `model/fonts.tsv`, which is to say by someone holding the
source tree. Measured cost on the 19-face manifest: 2,471 B against
1,548,603 B, 0.16%. The second: **the bank's scale ladder must be an authored
rule before any `.ocrw` is treated as canonical.** The current ladder,
16/20/24/32/48, was chosen by sweeping four candidates against a benchmark
corpus — fitting a parameter to a test set — and the held-out check confirmed
its ranking while cutting its margin by 60%, which is the profile of a mildly
overfitted choice. Rule 1 requires a sentence behind every number in the model
file and that sentence has never been written. `ocrcer-glyphs` owns it; until
it exists, files in `model/out/` are build artifacts, not release candidates,
and `ARCHITECTURE.md` section 11's 2026-09-22 artifact entry says why the one
sitting there now is at a ladder nothing endorses.

**Chunk 8 baseline taken early, 2026-09-21, and it is a loss.** The
head-to-head against `ocrs` that chunk 8 owns was run ahead of its chunk to
establish where the project actually stands, at operator direction ("that is
the benchmark to beat for now"). Over 660 pages of the tuning corpus — held
out from the *bank*, in that no page's text or rendering is a prototype, but
not a held-out *corpus* in the sense used further down this section; that one
is `bench/holdout/` at render sizes the tuning corpus does not contain —
**`ocrs` beats
OCRcer by 9.8 points of layout-free word F1 — 81.12% against 71.30% — while
OCRcer was given oracle segmentation and `ocrs` ran end to end.** Every
660-page figure in this passage is on a corpus later found to score one face
twice; the de-duplicated 625-page re-measurement is at the end of the ladder
paragraph below. OCRcer's
in-order character accuracy of 92.28% against 77.26% is not a win: it is
`ocrs` being charged for reading multi-column tables column-first, which is a
reading-order failure and not a recognition one. `ARCHITECTURE.md` section
11's 2026-09-21 head-to-head entry has the full table, the two corpus biases
that favour OCRcer, and the finding that the entire deficit sits at 14 and
18 px/em — below the already-measured feature-survival floor.

This does **not** close chunk 8, which still owes the four-document-class
table and the calibration curve, and cannot be complete before the decoder
(chunks 5–6) exists to be measured. What it establishes is the **hand-off
gate**: the precondition for telling `pdfcer` this engine is ready is
layout-free word F1 above `ocrs`'s, measured end to end on both sides.

**Re-measured the same day at a candidate feature weight: 9.82 points becomes
0.10, and the gate is still not met.** Weighting the four baseline-relative
feature dimensions — a multiplier, changing no table, no charset and no file
format — takes OCRcer to **81.02% layout-free word F1 against `ocrs`’s
81.12%**, with character accuracy 95.21% against 77.26%. Above 21 px/em OCRcer
is now 5–7 points *ahead* on the layout-free metric; below it, 3–5 behind.
That is a different project from the one the first baseline described, but a
tenth of a point behind *while holding oracle segmentation* is a loss, and the
weight that produced it makes `0` against `O` 71% worse — the one confusion
rule 6 forbids the decoder to repair. `ARCHITECTURE.md` section 4.1 now
requires any candidate weight set to clear a per-confusion-pair bar as well as
the aggregate. The gate is unchanged and unmet.

**And the small-size deficit turned out to be the bank’s scale ladder.** Four
bank compositions were measured on the same 660 pages. Adding a 12 px/em scale
*lost* 3.62 points at 14 px/em; adding a 20 px/em scale instead — **the same
prototype count, the same cost** — gained 10.61 at that size. Best measured
configuration to date: **85.30% layout-free word F1 against `ocrs`’s 81.12%,
ahead at every render size in the corpus**, character accuracy 96.32% against
77.26%. Four caveats, all of them load-bearing: OCRcer is still
oracle-segmented; the feature weight is a swept candidate that makes `0` against
`O` worse; the ladder was ranked *by this corpus* and needs confirming on render
sizes the corpus does not contain; and neither number is authored into the model
file. **The hand-off gate — end to end on both sides — is still unmet.**

*Amended 2026-09-22:* that corpus and that bank both carried Cascadia Code and
Cascadia Mono, which are the same outlines under two names. Re-run at the same
settings on the de-duplicated 625-page corpus with a 19-face, 17,610-prototype
bank, the configuration measures **85.22% layout-free word F1 against `ocrs`’s
81.20%** — a margin of 4.02 points rather than 4.18 — with character accuracy
96.28% against 77.20%. **The re-run also breaks the aggregate down by text
block for the first time, and `ocrs` is ahead on four of seven**, including
**drawing, 84.67% against 89.18%** — the domain `FEASIBILITY.md` section 6 says
this project exists to win, lost by 4.51 points while OCRcer holds oracle
segmentation. The aggregate lead is carried by the three blocks `ocrs` reads
badly (technical +32.78, twins +23.74, currency +11.81). `ARCHITECTURE.md`
section 11, 2026-09-22, tuning head-to-head entry; full run in
`docs/measurements/2026-09-22_vs_ocrs_625_tuning_w16_bank19.txt`.

**Held out on unseen render sizes the same day: the ranking holds, the margin
does not, and the “ahead everywhere” claim is withdrawn.** The same text and
faces rendered at 15/19/22/30/44 px/em — sizes that are neither in the
benchmark corpus nor any candidate bank scale — put the chosen ladder at
**83.69% layout-free word F1 against `ocrs`’s 82.72%**. It still beats the
four-scale ladder it replaced (82.05%), so the direction is confirmed; but the
gap between them is 1.64 points held out where the corpus said 4.28, meaning
about 60% of the measured gain belonged to those five render sizes rather than
to the bank. Worse for the headline: at 15, 19 and 22 px/em `ocrs` is *ahead*
on the layout-free metric, so OCRcer’s lead is now an aggregate lead carried
by large text, not a win at every size. The load-bearing caveat above is
unchanged — OCRcer is oracle-segmented and `ocrs` is end to end — so **the
hand-off gate remains unmet**, and the number a future ladder rule has to beat
is 83.69%, not 85.30%. *Amended 2026-09-22:* that corpus carried one face
twice. On the de-duplicated 625-page corpus the same configuration measures
**83.15% against `ocrs`’s 82.72%**, so the number to beat is 83.15% and the
lead is 0.43 points, not 0.97. See `ARCHITECTURE.md` section 11,
2026-09-22, duplicate-face entry.

**A sweep of every integer render size found the reason the ladder matters so
much, and it is not a comfortable one.** At a fixed bank, accuracy across
12–24 px/em is a smooth rise with **spikes of 10–15 points of word F1 at
exactly the three sizes the bank was rendered at**, and nowhere else — a page
at 17 px/em scores 15 points worse than one at 16 despite having more ink. The
32×32 normalisation removes the glyph's size but not its rasterisation, so an
exact size match is a bit-exact match. Neither benchmark corpus collides with a
bank scale, so no figure quoted above is inflated by it — but **any future
corpus or ladder change must be checked for a size collision explicitly**,
because a collision would be worth roughly thirteen free points with nothing in
the report to say so. The spike itself is **one pixel wide** and present at all
five bank scales, so it is a diagnostic about the extractor rather than a lever
to design a ladder around. What does move the ladder is prototype density in
the size range where the rasterisation residual is large — just above the
distinctness floor — and it saturates by roughly 28 px/em, which is a cheaper
answer than section 9's prototype budget feared.

Chunks 9–11 are scope additions the operator raised after chunk 0's original
staging (`ROADMAP.md` Backlog has the date). **Numbering is append-only**:
chunks 0–8 are cross-referenced by number from `ARCHITECTURE.md` and
`ROADMAP.md`, so a chunk inserted between existing numbers would break those
references. The three additions are numbered 9, 10 and 11 regardless of where
they would sit in the dependency graph, and their dependencies are stated
explicitly here rather than implied by table position:

- **Chunk 9** depends on chunk 2 (components, lines, baselines, x-heights,
  word boxes) and feeds chunk 7 (pdfcer integration), because structured
  output is what a PDF consumer actually wants.
- **Chunk 10** depends on chunk 2 only. It does not depend on any recognition
  chunk (3–6), so it can be built in parallel with them. It must not delay
  v1: v1 is the OCR replacement `pdfcer` needs, and chunk 10 is additive.
- **Chunk 11** extends chunk 8 rather than replacing it — chunk 8 already
  owns CER/WER and the head-to-head against `ocrs`; chunk 11 is the corpus
  acquisition and preparation work that feeds it.

**Priority ordering, operator directive 2026-09-21:** accounting/business
document support (chunk 9) is prioritised ahead of drawing support (chunk
10) — verbatim, "we need to support everything that an accounting firm
would need before we continue with supporting drawings." This changes
*execution order*, not numbering or dependency: chunk 10 still depends on
chunk 2 only and can still be built in parallel with the recognition chunks
whenever it is picked up, but it is deferred behind chunk 9 by operator
choice rather than by the dependency graph. See section 2a.

**Total: roughly 9.1 M tokens across nine chunks**, of which chunk 0 is already
spent. At one to two chunks per week that is **four to eight weeks**. Chunks
9–11 add roughly **3.4 M tokens** more, each figure a projection per section 1
and not a measurement — additive to the 9.1 M above, not a revision of it.

The saving against a two-language build is about 1.3 M tokens, and it is not
where it looks. The chunks are not fewer — the ceiling forbids merging them —
they are individually smaller, because none of them contains a second
implementation of a stage or the work of chasing a float divergence between two
of them.

Tuning rounds after chunk 8 are ~400 K each and are genuinely cheap in this
design, because a model rebuild is a script run.

---

## 2a. Chunk 9 detail — accounting and business document structure

**Rescoped and re-prioritised, operator directive 2026-09-21.** Verbatim:
"we need to support everything that an accounting firm would need before we
continue with supporting drawings. This includes the shapes of reports and
book keeping tables, etc, etc." Two consequences, both also recorded in
`ROADMAP.md`'s Backlog and Resolved sections rather than only here: chunk
9's scope moves from generic page layout toward accounting- and
business-document structure specifically, and chunk 9 is prioritised
**ahead of** chunk 10 (drawing primitives, section 2b) in execution order —
deferred, not renumbered; append-only numbering per section 2 above is
unchanged.

The original geometric groundwork stands. `ARCHITECTURE.md` section 8's
module sketch has `layout/lines.rs` (line grouping, baseline and x-height)
and `layout/words.rs` (gap analysis), and nothing above the word — chunk 2
stops at word boxes, and chunk 9 is still the chunk that produces a
document structure tree over them. **Page segmentation is still a
classical geometry problem needing no training**, still one of the few
capabilities where the constructed-not-fitted rule (rule 1) costs nothing
against a trained competitor. What changes is which document shapes chunk
9 is built to recognise first, and in what order.

**1. Ruled and columnar tables — the core case.** General ledgers,
journals, trial balances (paired debit/credit columns), bank statements,
AR/AP aging schedules with bucketed columns, invoices and receipts. The
structural cues here differ from prose:

- **Column alignment does the grouping work that whitespace runs do in
  flowing text.** A ledger row's fields are grouped by vertical alignment
  across many rows, not by any property of a single row read alone.
- **Ruled lines are semantic separators**, not decoration. The ruled-table
  detector from the original scope — long horizontal/vertical runs and the
  intersection graph they form, which also gives the cell grid directly —
  is the right tool, now aimed specifically at ledger and statement shapes
  rather than generic tables.
- **Dot leaders connect a label to a distant number** — a row where label
  and value are far apart with a repeating `.` or `-` run between them is
  one logical field, not two.
- **A single logical row may span several physical lines** — a wrapped
  invoice line-item description, or a multi-line account name in a trial
  balance, has to be recognised as one row, not several unrelated short
  ones.

**2. Financial statement shape.** Balance sheet, income statement, cash
flow. Distinct cues from item 1's tables:

- **Indentation encodes account hierarchy** — a more-indented line is a
  child of the nearest less-indented line above it.
- **A single rule above a number means subtotal; a double rule means
  total** — a convention, distinguished both from item 1's ruled-table
  separators and from item 4's prose-heading underlines.
- **Negatives appear three ways**: brackets, a leading hyphen, or a true
  minus sign — the charset gap below (U+2212) is direct input to reading
  the third form correctly.
- **Comparative prior-year columns sit alongside current-year ones** — item
  1's column-alignment machinery applies, but the semantic pairing (this
  year vs. last year, not debit vs. credit) is a distinct authored rule.

**3. Boxed forms — a distinct layout class, not a table and not prose.**
T4, T4A, T5, T3, T5018, T2125, GST/HST returns, and the T2 schedules are
named here as **identifiers only** — nothing here asserts what any specific
box on any of them contains, per `CLAUDE.md`'s rule against inventing
claim-bearing specifics and rule 1's rule against a plausible number
standing in for a real one. What matters structurally is the shape:
numbered boxes at fixed-ish positions, not rows in a table and not
paragraphs of prose. The deliverable is a box-number-to-value mapping, not
a transcript. This is the **highest-value output for an accounting firm**
among everything chunk 9 produces — it feeds data entry directly.

**4. Prose structure — still in scope, no longer the centre of the
chunk.** Paragraphs, headings, reading order, for notes to financial
statements and engagement letters. What the original scope specified —
the three paragraph-grouping signals (inter-line spacing against the
region's own median, left-edge alignment, first-line indent), the
relative-not-absolute header signals (height against regional median
x-height, stroke-weight-to-height ratio, surrounding whitespace,
shortness relative to column width — relative because absolute point
sizes are meaningless at an unknown scan DPI, the same reasoning behind
the resolution floor in `ARCHITECTURE.md` section 11), the region tree
and its traversal for reading order — is unchanged; it is demoted in
priority within the chunk, not removed or redesigned.

Every threshold any of the above needs is an authored parameter in the
parameter block, on chunk 8's tuning list, per rule 1. None is invented
here.

**Two correctness properties, stated as rules for this chunk, not as
nice-to-haves.**

- **Arithmetic self-validation is available here and nowhere else in OCR,
  and the design is meant to exploit it — proposed, not yet built or
  measured.** Accounting documents carry their own check digits: columns
  that sum to a stated total, debits that equal credits, subtotals that
  compose into totals, a balance sheet that balances. A recognised page can
  be checked against its own arithmetic at zero marginal cost — the check
  is trivial arithmetic, not a model call — and a column that fails to sum
  localises the error to the small set of fields that feed it. This fits
  rule 1 exactly: deterministic, authored, every step explainable in a
  sentence.
- **The engine must never alter a recognised digit to make a total
  balance.** This is the accounting analogue of rule 6's lexicon rule, and
  it is as absolute: a failed arithmetic check raises a flagged discrepancy
  naming the fields involved and their per-field confidences, and it never
  silently rewrites a figure to make the books agree. Silently balancing a
  ledger is the catastrophic failure mode in this domain, in exactly the
  shape rule 6 already names for the lexicon — the error becomes invisible
  and arrives with high confidence, and someone files on it.
- **Confidence must be reportable per field, not per page**, or neither
  rule above is actionable — a discrepancy report that cannot name which
  field is suspect is not one a reviewer can act on. This is a constraint
  chunk 9 places on the confidence machinery rule 5 defines, not a new
  confidence design of its own.

**A charset gap, flagged as chunk 9 input, not a settled addition.**
`model/charset.tsv`'s 187 classes already cover `$ ¢ £ ¥ € % – — ‰ † ‡`, but
**U+2212, the true minus sign, is absent** — only hyphen-minus (U+002D) is
in the charset. Many financial PDFs use U+2212, not hyphen-minus, for a
negative number, which is direct input to item 2's third negative-number
form above. This is reported as a gap, not planned as an addition: adding a
class changes the class count the 17,391-pair collision sweep, the 21px
resolution floor, and every checked-in fixture were measured against
(`ARCHITECTURE.md` section 11, 2026-09-21 entries), so it carries a
re-derivation cost across all three that `ocrcer-architect` has to decide
deliberately — not something this entry decides by adding a line to
`charset.tsv`.

**Decided 2026-09-22: the class is declined and the requirement lands here
instead.** Three of the nineteen shippable faces draw U+2212 and U+002D as the
same outline, so the glyph cannot carry the distinction and no threshold can
recover it. Whether a dash before a number is a minus is a question this
chunk's structured numeric layer answers from context — a debit column, a
dimension, a tolerance — and it is the only layer that can. Chunk 11 inherits
the other half: ground truth that preserves U+2212 has to be folded to U+002D
before scoring, and the benchmark report has to say so, because the fold
favours this engine (`ARCHITECTURE.md` section 11, 2026-09-22).

**Exit gate**, updated for the rescoped priority — the original gate's
ruled/unruled-table and heading-hierarchy fixtures stay, extended with the
new document classes: golden page fixtures with authored ground-truth
structure — a ruled columnar table (ledger/journal/trial-balance shape), an
unruled columnar table, a financial-statement page (subtotal/total rule
convention, at least one bracketed negative), a boxed form, and a
heading/paragraph prose fixture — assert region tree, block types and
reading order, and for the boxed-form and financial-statement fixtures,
the box-number/line-item-to-value mapping specifically; plus a dedicated
arithmetic-validation fixture whose ground truth deliberately breaks a
column sum, asserting the discrepancy is flagged with the fields and
confidences involved and that no digit is altered to close it. The harness
fails loudly on deliberate alteration, matching chunk 1's harness rule.
Token estimate: the original ~1.2 M projection predates this expanded
scope and is flagged as likely stale rather than silently revised without
a basis — re-estimating is chunk-start work for whoever picks up chunk 9,
not done here.

**Staging** (`ARCHITECTURE.md` section 11, 2026-09-25, "Candidate chunk 9
spec, part 1"). Chunk 9 is built as five sub-chunks, each with its own gate
and each default-off until that gate passes:

- 9a: the structure substrate (rules, ruled cells, word-to-cell assignment,
  regions);
- 9b: boxed forms;
- 9c: tables;
- 9d: statements;
- 9e: prose.

9b follows 9a directly. The structure layer never changes recognised text,
so every sub-chunk's gate is a structure gate. The exit gate above is the
union of the five.

## 2b. Chunk 10 detail — drawing primitives

**Deferred behind chunk 9, operator priority, 2026-09-21** — see section
2a. Scope below is unchanged by the reprioritisation; only execution order
moved.

**Not OCR.** A second engine that shares the front of the pipeline and
nothing else: it consumes the same binarised, deskewed raster chunk 2
produces, it serves the same consumer (`pdfcer`), and it carries the same
three `ocrcer-core` invariants (rule 3). It is proposed as a sibling crate,
`ocrcer-draw` — a proposal, not a decision — rather than living inside
`ocrcer-core`. It produces a list of vector primitives with types, not text.

- **Line tracing.** Skeletonise the ink, trace polylines, fit straight
  segments with a tolerance. A Hough transform is the textbook answer and
  the wrong one here: it finds infinite lines and then has to recover
  endpoints and handle every near-parallel neighbour in a dense drawing;
  tracing recovers endpoints directly and does not confuse two parallel
  lines a millimetre apart, which is the normal case in a drawing, not the
  exceptional one.
- **Line type classification** — the part that fits this project unusually
  well. Walk the run-length pattern of ink and gap along a traced polyline
  and classify it against the standard dash patterns — continuous,
  dashed/hidden, chain/centre, phantom — whose proportions are *specified*
  in ISO 128 and ASME Y14.2. Those standards are the sources to author the
  ratio table from; the actual ratios must be read from the standard, not
  recalled from memory. This is an authored table in exactly the sense rule
  1 wants: every number in it can be pointed at a published clause.
- **Basic shapes.** Circle and arc fitting by algebraic least squares on
  traced points; rectangles and polygons recovered from the segment graph by
  junction analysis. Scope limit: circles, arcs, rectangles and polylines,
  not splines, not hatching, not dimension-chain interpretation.
- **What this deliberately does not do.** No dimension association, no
  GD&T parsing, no title-block extraction, no symbol recognition — it does
  not interpret the drawing. Naming these here is scope control, not a
  promise.

**Dependency:** chunk 2 only. It does not depend on any recognition chunk,
so it can be built in parallel with the OCR line. This must not delay v1 —
v1 is the OCR replacement `pdfcer` needs, and chunk 10 is additive. Token
estimate roughly 1.4 M, a projection, not a measurement.

## 2c. Chunk 11 detail — evaluation corpora and the firewall that governs them

Extends chunk 8 rather than replacing it: chunk 8 already owns CER/WER and
the head-to-head against `ocrs`. Chunk 11 is the corpus acquisition and
preparation work that feeds it. Owner `ocrcer-bench`.

- **The firewall.** A corpus used to *score* the engine must never
  contribute a single value to the model. Two independent reasons, either
  sufficient alone: it would break the rule 2 licence case, and it would
  make the benchmark meaningless by testing on training data. Standing
  prohibition, same register as the other project rules: no lexicon entry,
  no bigram, no confusion pair and no threshold may be derived from any
  evaluation corpus. This is a live risk, not a theoretical one — a future
  session looking for lexicon material will find a large clean text corpus
  already sitting in the repository, and the prohibition is what stops it.
  **Amended 2026-09-24 (operator, `ARCHITECTURE.md` §11):** a corpus may now
  feed the model, but only from a *training split* that is disjoint from every
  scoring split. The splits are fixed by a committed manifest before any fitting
  runs. The scoring pages (the 60 `finfilings` pages, `pages-cov`, and every
  fixture) stay on the scoring side permanently.
- **Not before the engine can read a page.** Acquisition does not start until
  chunks 2, 5 and 6 have landed. An external corpus carries a page-level
  transcription, not the per-glyph boxes the present oracle-segmented harness
  needs, so until there is a path from pixels to text an acquired corpus
  produces no number at all — it produces a directory. Two further reasons
  point the same way: the firewall's live risk is a corpus *sitting in the
  repository*, and acquiring months before it can be scored maximises the
  window in which it can be mistaken for lexicon material; and the documented
  bias in the current corpus — noise-free, unskewed, hard black-and-white — is
  attacked more cheaply by deterministic degradation of our own rendered pages,
  which keeps exact ground truth and needs no licence review. Reading and
  tabulating candidate terms is desk work and may start earlier; downloading
  may not.
- **Public scene-text sets are not the yardstick.** `FEASIBILITY.md` section 6
  condition 3 fixes the comparison as `ocrs` on pdfcer's own corpus, not
  published figures on public sets. Most widely cited OCR benchmark suites are
  scene text or handwriting, which rule 7 puts out of scope; a candidate earns
  a row in the corpus table by being printed office, accounting or CAD drawing
  text, not by being well known.
- **Admission.** Modelled on the existing `model/fonts.tsv` discipline:
  every corpus gets a row in a table, and every row carries the licence **as
  read from the corpus's own stated terms**, plus the URL that was read. No
  licence claim is made here about any specific named corpus — the licence
  of each candidate must be read from source before use, and a corpus whose
  terms are unclear goes to the operator, exactly as an unclear font face
  does (`CLAUDE.md` rule 2). The table needs a redistributable column
  separate from its licence column: a corpus may be usable locally for
  scoring and still not be redistributable.
- **The cost question.** Scoring is edit distance — CER and WER are a
  script, not a judgement, so the per-run token cost is near zero regardless
  of which model runs it; what reaches a model is a summary table of a few
  dozen numbers. The expensive part is ground-truth preparation and
  normalisation — mapping a corpus's transcription conventions onto our
  charset — which is mechanical, high volume, and exactly the shape of work
  a small fast model does well. Recommendation: corpus preparation and
  normalisation run on Haiku; adjudicating a disagreement between ground
  truth and our output is a judgement and does not.

**Token estimate:** roughly 0.8 M, a projection, not a measurement.

**The pdfcer hand-off gate.** Operator directive 2026-09-21, verbatim: "once
you have determined that our OCR is better than the one we are currently
using in pdfcer I want pdfcer to be informed to add this one to its options
to use." This is a gate attached to chunk 11, not an action taken now.

**Superseded 2026-09-24 (operator):** integration no longer waits on
beating `ocrs`. OCRcer ships to pdfcer as an opt-in engine as soon as the
binding, speed and packaging are ready; the head-to-head decides the
default. See `ARCHITECTURE.md` §11, 2026-09-24.

- **"Better" is defined before it is measured**, or the metric gets chosen
  after the fact to produce a win. Primary metric: **CER** on a shared
  corpus admitted under this section's firewall, WER reported alongside,
  both **reported per domain** — printed office documents, accounting
  documents, CAD drawing text — rather than pooled into one number. Losses
  are reported as prominently as wins, per `CLAUDE.md` rule 8: a domain
  where `ocrs` wins is filed with the same weight as one where it does not.
- **Confidence calibration is a differentiator, not part of the accuracy
  comparison.** `ocrs` reports no confidence at all (`SESSION_LOG.md`'s
  2026-09-18 bootstrap entry), so this engine's calibrated per-character/
  per-word confidence is a genuine capability gap in its favour — but it
  must not be blended into the CER/WER numbers to manufacture a win on a
  metric `ocrs` cannot even report against. It is reported as its own line,
  separate from the accuracy table.
- **The corpus firewall stands.** Nothing that scores the engine in this
  comparison may have contributed a lexicon entry, bigram, confusion pair
  or threshold to the model — the firewall stated earlier in this section,
  restated here because the pdfcer hand-off is exactly the comparison it
  exists to keep meaningful.
- **The hand-off itself is out of this working tree, gated, and needs a
  fresh operator go.** Informing `pdfcer` and proposing this engine as an
  option there happens only after the gate above passes, and only with the
  operator's go **at that time** — this entry does not pre-authorise it.
  It is proposed as an *additional option* alongside `ocrs`, not a
  replacement — `ocrs` stays available regardless of the outcome here.
- **A precondition this gate does not resolve:** whether `pdfcer` wants this
  engine at all is still open there (`ROADMAP.md`'s "still open from
  bootstrap" item; section 5's second bullet below) — worth resolving
  before the gate is reached, not after, so this chunk's comparison work is
  not spent answering a question that was never open on the receiving end.

---

## 3. Scheduling

```
0 -> 1 -+-> 2 ---------------+
        |                    +-> 5 -> 6 -> 7 -> 8
        +-> 3 -> 4 ----------+
```

Chunk 2 (the image pipeline) and chunks 3–4 (the model) are independent once
the charset and the feature extractor are frozen in chunk 1, so they can
proceed in either order or interleave. The critical path is 0-1-3-4-5-6-7-8.

**The fixture harness built in chunk 1 is the spine of every gate that
follows.** Each later chunk asserts against checked-in expectations at its own
stage boundary, which is what makes those chunks verifiable long before an
accuracy number exists — a stage either reproduces its fixtures or it does not,
and that question has an answer on day one. `ARCHITECTURE.md` section 8.2 has
the contract, including why blessing a fixture is a deliberate, reviewed act
and never automatic.

---

## 4. Risks that change the plan

**Font coverage is insufficient for real documents.** The main accuracy risk.
Invisible until chunk 8. Mitigation is structural rather than heroic: the bank
rebuilds in minutes, low-confidence output names the characters it struggled
with, and adding families is a config change. Budget a tuning round, not a
redesign.

Split off 2026-09-22, because a per-class audit showed the two halves of this
risk behave differently. *Coverage by family* — a document set in a face the
bank has no near neighbour for — mitigates exactly as written. *Coverage by
class* does not: 177 of 187 classes are drawn by all eighteen licence-clean
faces, and `⌀` U+2300 by two, because the families that draw it are
overwhelmingly CAD-vendor proprietary. Adding a family is a config change only
where a licence-clean family exists to add, and for domain symbols that may be
nobody. The lever there is authoring the glyph into the ISO 3098 face in
`ocrcer-build`, which is a day of drawing rather than a config change — still
not a redesign, but budget it separately. `ARCHITECTURE.md` §11, 2026-09-22.

**Degraded-scan accuracy is unacceptable rather than merely worse.**
`FEASIBILITY.md` section 5 projects 85–93% there. If it lands below that, the
levers in order of cost are: better binarization (adaptive parameter selection
per page), morphological repair of broken glyphs before segmentation, and a
richer merge-hypothesis set in the lattice. Only if all three fail does the
question of a fitted classifier for that one stage reopen — and the architecture
deliberately isolates classification so that substitution is possible without
touching anything else.

**Touching-character segmentation is harder than expected at small point
sizes.** The known hard case for this design. The lattice is the mitigation and
it is in the design from the start rather than retrofitted, which is the
difference between this being tunable and being a rewrite.

**The budget model is wrong.** Likely, since weekly limits are not published as
tokens. Chunk 1 calibrates against `/usage`; `ocrcer-librarian` rescales section
2 when reality diverges.

**Fixtures get blessed to make a test pass.** The structural risk of a
single-implementation design. A wrong expectation that was regenerated rather
than understood turns the whole suite into a record of a bug. The defences are
that blessing is a separate deliberate command, that its diff is reviewed, and
that fixture inputs have known ground truth so a wrong expectation can be
caught by reading it. `ocrcer-architect` adjudicates any blessing that changes
more than a stage boundary's worth of output.

**Lexicon over-correction damages identifiers.** An engine that rewrites
`M8x1.25` into a dictionary word is worse than one that reads it wrongly, because
the error is invisible and confident. The design answer is in `ARCHITECTURE.md`
section 5 — the lexicon is a bonus, never a penalty, and is suppressed in
identifier-shaped context — and chunk 8 must include an identifier-preservation
test that fails loudly.

---

## 5. What would make this not worth finishing

- If chunk 8 shows `ocrs` beating it on pdfcer's own corpus across *all four*
  document classes after two tuning rounds, ship `ocrs` and keep the benchmark
  harness, which is the thing the survey says does not exist anywhere.
- If the operator's open question about `ocrs`'s CC-BY-SA weights resolves to
  "aggregation is fine and we will never fine-tune", one of three justifying
  arguments weakens — though the confidence capability and the CAD-domain fit
  stand on their own.
- If scope creeps to handwriting, scene text or non-Latin v1, stop. Those are
  different projects wearing this one's name.
