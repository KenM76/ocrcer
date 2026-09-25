# Identifier-preservation corpus: measurement

Closes the gap flagged in `2026-09-25_score_12b.md` §4: chunk 8 promised a
corpus-level "OCR an image, assert the exact identifier string" harness, and
only two unit tests existed (shape predicate, "lexicon holds nothing the gate
would suppress"). This is that harness, updated twice after architect
review: first to test lexicon *causation* rather than coincidence and report
confidence, then to narrow "causation" to **LEXICON-HARM**
(`ARCHITECTURE.md` §11, 2026-09-25, "Identifier gates") after the one fitted
causal case turned out to be a drop reading back as a different wrong word,
not the rule-6 harm.

Everything below is **measured**, from a real end-to-end pipeline run
(`Engine::recognize_lines` — real binarization/segmentation/decode, not the
oracle reader `pages::read_with_bank` uses elsewhere). Nothing here is fitted
or authored; the corpus content is **authored by me**. The gates themselves
are the architect's ruling, not a proposal from this report (see "Gates"
below).

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
  identical one with `decode.w_lex` forced to `0` — and tags every
  disagreement `LexCase::harm` per the architect's LEXICON-HARM definition.
- `crates/ocrcer-bench/src/bin/ident.rs` — `ident generate <dir> <sizes>`,
  `ident score <model> <dir> [--set k=v]... [--threshold N]
  [--lexicon-threshold N] [--confident-threshold N] [--offenders-out
  <path>]`. Three gates: REWRITTEN count (ratchet, default 0), LEXICON-HARM
  count (hard, default 0), REWRITTEN-at-confidence->=0.9 count
  (report-only unless `--confident-threshold` is set).
- `bench/ident/` — generated corpus + regenerated offender-list files,
  gitignored; `offenders_{fitted,control}.txt` are written by `ident score
  --offenders-out`, not hand-maintained (regenerate with the commands below).

Corpus: 53 licence-cleared faces x 5 sizes (14-40 px/em) x 7 content blocks =
1855 pages, **6360 identifier-shaped tokens**, identical between fitted and
control configs. Deliberately identifier-dense — far denser than a real
drawing/invoice page — read the counts as "how the engine handles a lot of
identifier-shaped text," not a share of pdfcer's corpus. Known predicate
limitation, not a harness gap: `identifier_shape` requires `letters > 0`, so
a purely-numeric code is never identifier-shaped under it — not exercised
here.

## Measured results: REWRITTEN

| Config | Tested | Exact | Dropped/garbled | REWRITTEN | rate |
|---|---:|---:|---:|---:|---:|
| **Fitted** (master defaults) | 6360 | 3912 (61.509%) | 1177 (18.506%) | **1271** | 19.984% |
| **Control** (pre-fold 11-param `--set`) | 6360 | 3684 (57.925%) | 1207 (18.978%) | **1469** | 23.097% |

REWRITTEN split by kind: Fitted 1268 different-identifier / 3 lexicon-word;
Control 1459 / 10 — descriptive (coincidental dictionary-word landings
included), not the causal test below. Fitted beats control here, same
direction as `score_12b.md`'s CER/WER win.

## Lexicon A/B: LEXICON-CHANGED vs. LEXICON-HARM

Every identifier-shaped token is read twice: at the engine's configured
`decode.w_lex`, and with it forced to 0. **LEXICON-CHANGED** = the two runs
disagree and the lexicon-on run is wrong — reported, never gated. Architect
review, 2026-09-25: LEXICON-CHANGED alone is too broad for rule 6. Its one
fitted member turned a dropped token into `"CHAMPER"` — a different wrong
answer, arguably more visible, not the silent-and-confident harm rule 6
names. **LEXICON-HARM** narrows it to LEXICON-CHANGED cases where,
additionally, the lexicon-off run would have been correct, or the lexicon-on
word is itself a lexicon entry — the shape rule 6 actually describes.

| Config | LEXICON-CHANGED | LEXICON-HARM |
|---|---:|---:|
| Fitted | **1** | **0** |
| Control | **5** | **3** |

Fitted's one case (not harm — no on-run word, off-run also wrong):

```
"2X45" -> <none> | CHAMPER   ident__liberation-serif__bolditalic__dims-ext__28px line 2
```

Control's five cases (3 of 5 are harm — the on-run word is itself a lexicon
entry in each: `"WE"`, `"Am"`, `"I"`):

```
"INV-2026-0042" -> WE | /Nv-2020-0042   [HARM: on-word is a lexicon entry]
"NG-4471" -> <none> | COOE             [benign: no on-run word]
"A36" -> Am | Aæ                       [HARM: on-word is a lexicon entry]
"8X" -> æ | <none>                     [benign: off-run also wrong, "æ" not a lexicon word]
"R2.1" -> I | t                        [HARM: on-word is a lexicon entry]
```

**Reading this**: LEXICON-HARM is 0 on the fitted (shipped) config, and 3 of
6360 on the pre-fold control — neither is the dominant driver of REWRITTEN
(1271/1469). REWRITTEN is overwhelmingly confusion/segmentation, per the
failure-mode breakdown below, not lexicon suppression.

## Confidence split (rule 5's harm: confident and wrong)

| Config | REWRITTEN n | REWRITTEN median | >=0.5 | >=0.8 | >=0.9 | exact n | exact median |
|---|---:|---:|---:|---:|---:|---:|---:|
| Fitted | 1271 | 0.576 | 817 (64.3%) | 193 (15.2%) | 67 (5.3%) | 3912 | 0.800 |
| Control | 1469 | 0.517 | 777 (52.9%) | 146 (9.9%) | 45 (3.1%) | 3684 | 0.773 |

REWRITTEN's median confidence sits well below exact's in both configs, but
fitted still reports >=0.9 on 67 cases (5.3%). A reviewer trusting a
high-confidence identifier read is misled by roughly 1 in 19 of the
confident fitted cases — the tail, not the median, is rule 5's harm, and it
does not vanish on the winning config. This is the population
`--confident-threshold` gates.

## Failure-mode breakdown (fitted config; control is the same shape, slightly worse)

Four clusters cover ~60% of the 1271 fitted REWRITTEN cases (756/1271); the
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

## Gates (architect ruling, `ARCHITECTURE.md` §11, 2026-09-25, "Identifier gates")

1. **LEXICON-HARM = 0.** Hard, rule 6. Measured fitted: 0. `--lexicon-threshold`
   default 0 now gates this narrower count, not LEXICON-CHANGED.
2. **REWRITTEN <= 1271.** A ratchet at the fitted baseline above. Lowering
   it resets the baseline in `ARCHITECTURE.md` §11; raising it to pass a
   build is not permitted.
3. **REWRITTEN at confidence >= 0.9 <= 67.** Also a ratchet at the fitted
   baseline. `--confident-threshold 67` enforces it; unset, `ident score`
   only reports the count.

Chunk 15 is the first chunk these gates apply to. The clusters above route
to their owners in "Routing"; `0`/`O` also goes to chunk 15, whose probe
read the digit `0` at about 98%.

## Regenerating the offender and LEXICON-CHANGED lists

Full per-case lists are not committed; regenerate with:

```
cargo build --release -p ocrcer-bench --bin ident
target/release/ident score model/out/ocrcer.ocrw bench/ident \
  --threshold 1271 --lexicon-threshold 0 --confident-threshold 67 \
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
wall clock (two full engine passes x 1855 pages). This round, control's
LEXICON-HARM count (3) was computed from the already-generated offender text
(a lexicon-membership check on the 5 on-run words, not a second full
rescore) since neither the tested tokens nor the two engines' outputs change
under the unchanged LEXICON-CHANGED definition — a full `ident score` rerun
with the command above would reproduce the same 5/3.
