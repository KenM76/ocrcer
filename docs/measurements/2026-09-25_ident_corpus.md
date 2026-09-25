# Identifier-preservation corpus: measurement

Closes the gap flagged in `2026-09-25_score_12b.md` §4: chunk 8 promised a
corpus-level "OCR an image, assert the exact identifier string" harness, and
only two unit tests existed (shape predicate, "lexicon holds nothing the gate
would suppress"). This is that harness's first run, updated after architect
review to test lexicon *causation* rather than coincidence and to report
confidence.

Everything below is **measured**, from a real end-to-end pipeline run
(`Engine::recognize_lines` — real binarization/segmentation/decode, not the
oracle reader `pages::read_with_bank` uses elsewhere). Nothing here is fitted
or authored; the corpus content and the threshold proposal are **authored by
me** for this report and are explicitly not yet a gate.

Build under test: `model/out/ocrcer.ocrw`, `build_id d2997ef3` (same build as
`score_12b.md`; not rebuilt this round).

## What was built

- `crates/ocrcer-bench/src/ident_corpus.rs` — authored identifier-shaped text
  blocks (`threads`, `parts`, `revisions`, `dims`/`dims-ext`, `accounts`,
  `dates`) mixed with plain words per line at real-drawing density.
- `crates/ocrcer-bench/src/ident.rs` — classification, calling
  `ocrcer_core::params::identifier_shape` /
  `ocrcer_core::decode::lexicon::lookup` directly, no reimplementation.
  `score_dir` runs every page through **two** engines — configured, and an
  identical one with `decode.w_lex` forced to `0` — to test whether the
  lexicon term actually *caused* a wrong answer, not just whether the wrong
  answer happened to be a dictionary word.
- `crates/ocrcer-bench/src/bin/ident.rs` — `ident generate <dir> <sizes>`,
  `ident score <model> <dir> [--set k=v]... [--threshold N]
  [--lexicon-threshold N] [--offenders-out <path>]`. Two independent hard
  gates, both default 0: REWRITTEN count and LEXICON-CAUSED count.
  `--offenders-out` writes the case lists to a file instead of stdout.
- `bench/ident/` — generated corpus + regenerated offender-list files,
  gitignored; `offenders_{fitted,control}.txt` are written by `ident score
  --offenders-out`, not hand-maintained (regenerate with the commands below).

Corpus: 53 licence-cleared faces x 5 sizes (14-40 px/em) x 7 content blocks =
1855 pages, **6360 identifier-shaped tokens**, identical between fitted and
control configs. Deliberately identifier-dense — far denser than a real
drawing/invoice page, to exercise the predicate rather than estimate its
real-corpus frequency; read the counts as "how the engine handles a lot of
identifier-shaped text," not a share of pdfcer's corpus. Known predicate
limitation, not a harness gap: `identifier_shape` requires `letters > 0`, so
a purely-numeric code is never identifier-shaped under it — not exercised
here.

## Measured results: REWRITTEN

| Config | Tested | Exact | Dropped/garbled | REWRITTEN | rate |
|---|---:|---:|---:|---:|---:|
| **Fitted** (master defaults) | 6360 | 3912 (61.509%) | 1177 (18.506%) | **1271** | 19.984% |
| **Control** (pre-fold 11-param `--set`) | 6360 | 3684 (57.925%) | 1207 (18.978%) | **1469** | 23.097% |

REWRITTEN split by kind (descriptive — see "Lexicon causation" below for the
causal test): Fitted 1268 different-identifier / 3 lexicon-word; Control 1459
/ 10. Fitted beats control on REWRITTEN, same direction as `score_12b.md`'s
CER/WER win — the fold did not trade identifier safety for prose accuracy.

## Lexicon causation: the old sub-class measured the wrong thing

Architect review, 2026-09-25: the "lexicon word" sub-class above counts
whenever the *output* happens to be a lexicon word, and its 3 (fitted) / 10
(control) members are almost entirely truncations landing on a short
dictionary entry by coincidence, not by the lexicon bonus winning —
`"6mm"` -> `"mm"`, `"R2.1"` -> `"I"`, `"INV-2026-0042"` -> `"WE"` are drops,
not pulls. That table is descriptive, not evidence of cause.

**The causal test**: every token is read twice per page — once at the
engine's configured `decode.w_lex`, once with `decode.w_lex = 0`. A case is
**LEXICON-CAUSED** when the two runs disagree *and* the lexicon-on run is
wrong. Now a hard gate (`--lexicon-threshold`, default 0, per rule 6) —
measured here, not made to pass.

| Config | LEXICON-CAUSED |
|---|---:|
| Fitted | **1** |
| Control | **5** |

Fitted's one case:

```
"2X45" -> <none> | CHAMPER   ident__liberation-serif__bolditalic__dims-ext__28px line 2
```

Control's five cases:

```
"INV-2026-0042" -> WE | /Nv-2020-0042   ident__inter__italic__accounts__14px line 0
"NG-4471" -> <none> | COOE             ident__noto-sans__italic__accounts__14px line 3
"A36" -> Am | Aæ                       ident__noto-serif__bold__parts__14px line 1
"8X" -> æ | <none>                     ident__noto-serif__bolditalic__threads__18px line 2
"R2.1" -> I | t                        ident__roboto__bolditalic__revisions__14px line 1
```

The fitted case: `"2X45"` was dropped entirely at the configured
`decode.w_lex`; with the term removed it read as `"CHAMPER"` — also wrong,
but a *different* wrong answer, satisfying "disagree AND on-run wrong." Not
in the REWRITTEN offender list at all (a drop is `DroppedOrGarbled`, not
"lexicon word") — direct evidence the two measurements track different
things, as designed. The control cases are the same shape; none of the five
is a clean "rewritten into a real dictionary word" case either.

**Reading this**: against a corpus built dense in identifier-shaped text, the
lexicon bonus caused a small minority of REWRITTEN failures — 1 of 6360
(fitted), 5 of 6360 (control), against 1271/1469 total REWRITTEN. The
REWRITTEN problem is overwhelmingly confusion/segmentation, not
lexicon-suppression; see the failure-mode breakdown below.

## Confidence split (rule 5's harm: confident and wrong)

Measured only — no gate reads this yet.

| Config | REWRITTEN n | REWRITTEN median | >=0.5 | >=0.8 | >=0.9 | exact n | exact median |
|---|---:|---:|---:|---:|---:|---:|---:|
| Fitted | 1271 | 0.576 | 817 (64.3%) | 193 (15.2%) | 67 (5.3%) | 3912 | 0.800 |
| Control | 1469 | 0.517 | 777 (52.9%) | 146 (9.9%) | 45 (3.1%) | 3684 | 0.773 |

**Reading this**: in both configs REWRITTEN's median confidence sits well
below exact's — calibration is doing *something*. But fitted still reports
>=0.8 on 193 cases (15.2%) and >=0.9 on 67 (5.3%); control's smaller *share*
at each threshold (9.9%, 3.1%) is its larger, noisier pool pulling median and
tail down together, not better calibration — its raw high-confidence counts
(146, 45) are comparable to fitted's. A reviewer trusting a high-confidence
identifier read is misled by roughly 1 in 19 of the confident fitted cases.
That tail, not the median, is rule 5's harm, and it does not vanish on the
winning config.

## Failure-mode breakdown (fitted config; control is the same shape, slightly worse)

Four clusters (pattern matches against the offender list, not a strict
partition) cover ~60% of the 1271 fitted REWRITTEN cases (756/1271); the
remainder is a long tail of one-off substitutions. None traces to the
lexicon — all four are confusion-pair or segmentation problems.

1. **Multiplier case-drop, `NX` -> `Nx`** (`4X`->`4x`, `2X`->`2x`, `8X`->`8x`):
   282 cases (22.2%) — every face, every size including 40px, one glyph pair.
2. **`0`/`O` digit-letter swap**, overwhelmingly `R0.5`->`RO.5`: 121-125
   cases (~9.7%) — concentrated in `dims`.
3. **Hyphen read as a different dash/math glyph** (`-` -> `– — ÷ · ~ ° ≈ ‰`):
   274 cases (21.6%) — concentrated in `accounts`/`dates`, nearly all faces.
4. **Leading-token/leading-digit drop** (hypothesis is a suffix of ground
   truth): 75 cases (5.9%) — concentrated at 14px and line starts.

## Routing

- `ocrcer-linguist` — clusters 1-2 (X/x, 0/O): narrow, uniform-across-font
  confusion pairs at every size — a confusion-table/decision-boundary fix,
  not segmentation or fonts. Cluster 3 (hyphen -> rare extended punctuation)
  looks like a class-prior problem: rare glyphs winning against a common
  ASCII hyphen.
- `ocrcer-runtime` — cluster 4 (leading-token drop at 14px/line-start) looks
  like segmentation losing small leading strokes; try binarization/
  morphological repair before decoder weights, per `PLAN.md`'s ladder.
- `ocrcer-glyphs` — no cluster points at missing font coverage; all four
  spread across nearly the whole face list, arguing against a font-bank fix.
- `ocrcer-architect` — sets `--threshold` / `--lexicon-threshold`; see below.

## Proposed threshold — NOT a decision, `ocrcer-architect` sets the gate

Measured baseline: **1271 REWRITTEN / 1 LEXICON-CAUSED (fitted)**, **1469
REWRITTEN / 5 LEXICON-CAUSED (control)**, of 6360 tokens each. Threshold 0 —
the binary's conservative default — fails all four numbers today, as
designed; it has never passed.

Two honest options for `--threshold` (REWRITTEN): (1) a **baseline-relative
regression gate now** (e.g. ~1300 fitted / ~1500 control) — catches
regressions immediately, doesn't block chunk 8 on a pre-existing gap, but
leaves ~20% of identifier-shaped tokens unprotected relative to rule 6's
stated bar; or (2) a **rate-based ratchet tied to the failure-mode
clusters** — hold today's count while linguist/runtime close clusters 1-4,
ratchet down per closure, more honest about direction, costs more process.

`--lexicon-threshold` is different: at 1/6360 (fitted) and 5/6360 (control)
the lexicon is not currently the main driver of REWRITTEN, so 0 is close to
already achievable and the architect may choose to hold it there.

## Regenerating the offender and LEXICON-CAUSED lists

Full per-case lists are not committed (2920 lines in the prior version of
this file); regenerate them with:

```
cargo build --release -p ocrcer-bench --bin ident
target/release/ident score model/out/ocrcer.ocrw bench/ident \
  --offenders-out bench/ident/offenders_fitted.txt

target/release/ident score model/out/ocrcer.ocrw bench/ident \
  --set decode.w_seg=0.25 --set decode.seg_ideal_aspect=0.6 \
  --set decode.seg_aspect_tolerance=0.55 --set decode.seg_merge_penalty=0.5 \
  --set words.pitch_tolerance=0.15 --set segment.max_merge_x_heights=1.8 \
  --set segment.valley_fraction=0.5 --set segment.min_piece_x_heights=0.2 \
  --set layout.slant_margin=1.15 --set match.top_k=5 --set decode.beam_width=24 \
  --offenders-out bench/ident/offenders_control.txt
```

Both files land in the gitignored `bench/ident/`. Each run took ~35 minutes
wall clock (two full engine passes x 1855 pages).
