# Chunk 14: counts from training-split text

`PLAN.md` row 14. Counting stage only — no edit to `model/lexicon.txt` or
`model/bigram_priors.tsv` in this pass. Tool: `crates/ocrcer-bench/src/bin/count_text.rs`
(`cargo run -p ocrcer-bench --bin count-text -- <pages-train-dir> <out-dir>`).
Output committed to `D:/Dev/ExcludedPrivate/ocrcer/counts/` (outside the repo,
per the project's convention for rendered/derived corpus artifacts):
`word_counts.tsv`, `char_bigrams.tsv`, `sources.tsv`.

## Sources used

| Dataset | Split | Rows | Licence | Status |
|---|---|---|---|---|
| multifinben-englishocr | train | 427 (all) | Apache-2.0 | **counted** |

427 is every `train`-split row `bench/splits/manifest.tsv` assigns to
`multifinben-englishocr` (`bench/splits/README.md`'s committed firewall).
Every row was checked against the manifest with
`ocrcer_bench::splits::assert_fittable` before its text was read, and the
input directory name is gated to end in `-train` in code
(`assert_train_dir_name` in the tool), not left to whoever passes the path.

## Sources excluded, and why

| Dataset | Split | Licence | Reason excluded from this count |
|---|---|---|---|
| cord-v2 | train (800 rows) | CC-BY-4.0 | **Domain fit, not licence.** `ARCHITECTURE.md`'s 2026-09-24 entry records a prior measurement: "the receipts (CORD, SROIE) out of domain... training candidates only for their glyph appearance, not for layout or language." Chunk 13 already spends CORD on glyph appearance; this chunk is language/lexicon, so it stays out. Also not currently rendered to text form — `tools/manifest_render.py` only implements the MultiFinBen path. |
| sroie | train (626 rows) | CC-BY-4.0 | Same as CORD — domain fit, not licence; also unrendered. |
| scribeocr-benchmark | (any) | AGPL-3.0 | Licence exclusion — copyleft, and scene text, out of domain regardless (`bench/splits/README.md`). |
| CRA forms | (any) | Crown copyright, not cleared | Licence exclusion — not yet cleared for use. |
| IRS forms | (any) | (unreviewed) | Excluded per the task's own instruction for this chunk, not a licence finding recorded here. |

`bench/splits/manifest.tsv` rows outside `train`/`validation` (`score`,
`excluded`) were never read — enforced by the dataset+split filter and then
independently re-checked per row via `assert_fittable`, which is the
project's single point of firewall enforcement (`crates/ocrcer-bench/src/splits.rs`).

## Tokenisation, exactly as counted

- **Words** (`word_counts.tsv`): a whitespace token counts as a word
  candidate only if `ocrcer_core::params::identifier_shape` (the same
  function `ocrcer-build`'s lexicon compiler calls, not a re-implementation)
  says it is *not* identifier-shaped, and if trimming leading/trailing
  non-letter characters (letter = charset category `lower`/`upper`) leaves a
  non-empty, all-letter remainder. Case-folded to lowercase. This means every
  identifier-shaped token (a part number, a filing reference, a dimension) is
  excluded from lexicon-candidate counting entirely, per CLAUDE.md rule 6 —
  not merely suppressed later at decode time.
- **Character bigrams** (`char_bigrams.tsv`): every whitespace token,
  including identifier-shaped ones, split into maximal runs of characters
  present in `model/charset.tsv`; each run counted boundary→first,
  adjacent pairs, last→boundary, the same shape `ocrcer-build/src/bigrams.rs`
  already uses per lexicon word. The boundary marker is the literal string
  `<BOUND>` — `model/bigram_priors.tsv`'s own `^` convention could not be
  reused here because `^` (U+005E) is itself a real charset class (index 61,
  category `symbol`) and using it as the boundary marker would have
  collided with genuine `^` bigrams.

## Counts (single run, `D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train`, 427 documents)

| Measure | Value |
|---|---|
| Total whitespace tokens | 161,146 |
| Identifier-shaped tokens (excluded from word counting) | 2,728 |
| Word tokens counted | 99,439 |
| Discarded tokens (not identifier-shaped, not all-letter after trim — currency amounts, dates, punctuation runs, mixed-script noise) | 58,979 |
| Distinct candidate words | 6,497 |
| Distinct character-bigram pairs (incl. boundary) | 3,125 |

Determinism: run twice into separate output directories and diffed byte-for-byte
(`word_counts.tsv`, `char_bigrams.tsv`, `sources.tsv`, and the tool's own stdout
summary) — identical on both runs. `sources.tsv`'s SHA-256 for the first row
was independently cross-checked against Python's `hashlib.sha256`
(`7dd231d56d8dfb7dfe38ef6bd7dd1a8449e253877ac0c805dcffb988f33f15ba`) and matched.

### Top 20 words by count

```
the      6419   (216 docs)
of       4317   (237 docs)
to       2818   (196 docs)
and      2499   (200 docs)
sh       1758   (44 docs)    -- OCR/render artifact of "Sh." in filer names; see caveat below
in       1696   (193 docs)
com      1440   (44 docs)    -- almost certainly ".com" split by the tokenizer, not the word "com"
or       1414   (184 docs)
a        1413   (228 docs)
inc      1293   (139 docs)
sole     877    (39 docs)
action   854    (66 docs)
dfnd     790    (24 docs)    -- filing boilerplate abbreviation, not an English word
for      653    (166 docs)
that     610    (135 docs)
by       597    (146 docs)
with     576    (127 docs)
on       564    (150 docs)
securities 556  (110 docs)
as       547    (148 docs)
```

**Caveat, reported as measured rather than smoothed over:** several
high-count entries (`sh`, `com`, `dfnd`) are tokenizer artifacts of the
MultiFinBen filing-text formatting (fragments split off abbreviations and
domains by whitespace tokenization), not English words a lexicon bonus
should reward. This is exactly what the document-count and minimum-count
thresholds below are for — but a threshold on frequency alone will not
catch these, since they are frequent *and* multi-document (`sh` appears in
44 distinct filings). Flagging this for the authoring pass rather than
filtering it here: this chunk counts, it does not curate.

### Top 15 character bigrams by count (excluding boundary-boundary noise)

```
e <BOUND>   15154
0 <BOUND>   13866
2 0         11642
0 2         11217
<BOUND> t   10667
s <BOUND>    9343
5 <BOUND>    8902
o n          8857
t h          8492
<BOUND> 0    8468
<BOUND> a    8433
i n          8402
/ 2          8219
t i          7984
h e          7963
```

`th`, `he`, `in`, `on`, `ti` are the expected common-English digraphs the
existing authored bigram table already carries priors for
(`model/bigram_priors.tsv`). The digit-heavy bigrams (`2 0`, `0 2`, `/ 2`,
digit→boundary) reflect how date- and dollar-amount-dense MultiFinBen filing
text is; they support the existing `digit-digit`/`digit-punct` category
defaults rather than motivating new pair-level authored entries.

## Lexicon overlap

- Existing shipped lexicon, base forms only (`model/lexicon.txt`, unexpanded,
  lowercased): **1,940** entries.
- Existing shipped lexicon, fully expanded via `ocrcer_build::lexicon::expand`
  (inflected surface forms, lowercased): **4,137** forms.
- Of the 6,497 distinct counted words, **1,126** are exact matches to a base-form
  lexicon entry (17%); the rest are either inflected forms already covered by
  the expansion, or genuinely absent.

## Candidate lexicon additions under three threshold proposals (not applied)

All three thresholds below are **guesses**, not measured or fitted values —
no validation-split scoring has been run against any of them. They are
offered as a starting point for the authoring pass this chunk hands off to,
sized to show how sensitive the candidate count is to the choice:

| Rule (guess) | Candidates (count ≥ N, distinct docs ≥ M, not already in expanded lexicon) |
|---|---|
| count ≥ 3, distinct_docs ≥ 2 | 1,503 |
| count ≥ 5, distinct_docs ≥ 3 | 848 |
| count ≥ 10, distinct_docs ≥ 5 | 405 |

## Proposed rules (authoring pass, not applied by this chunk)

**Lexicon-addition rule (guess).** Add a counted word as a new base-form
lexicon entry only if:
1. It is not identifier-shaped (`ocrcer_core::params::identifier_shape`) —
   already enforced at counting time, so nothing identifier-shaped ever
   reaches this stage;
2. `count ≥ 5` **and** `distinct_docs ≥ 3` (guess, the middle of the three
   rows above) — the two-part form exists specifically so that one
   verbose document repeating a name or boilerplate phrase many times
   cannot alone qualify a word; requiring several distinct filings to use
   it is a cheap proxy for "this is vocabulary, not one document's noise";
3. It survives manual review against the tokenizer-artifact caveat above
   (`sh`, `com`, `dfnd`-shaped fragments) — the two-count/doc-count rule
   does not by itself filter these, since they are genuinely frequent
   *and* multi-document. This review step is a **guess at process, not a
   number**, and is the reason this chunk does not auto-apply the
   threshold even for entries that clear it mechanically;
4. Every accepted word is banded at the lowest authored tier (per
   `model/lexicon.txt`'s tier convention) rather than inferring a tier from
   count — count is a training-domain frequency, not a claim about
   general-English frequency, and the two must not be conflated;
5. Any Apache-2.0-attributable extraction (i.e. any lexicon word actually
   sourced from MultiFinBen counts) needs a `NOTICE` entry for
   `multifinben-englishocr`, parallel to the existing Qwen entries — see
   the NOTICE note below. Not needed for this chunk since nothing was
   added.

**Bigram-blending rule (guess).** For every `(prev, next)` pair, blend the
counted training frequency with the existing authored prior in
`model/bigram_priors.tsv` as a weighted log-probability mix:

```
blended_logp = w_train * log2(train_count[prev,next] / train_total[prev])
             + (1 - w_train) * authored_logp(prev, next)
```

with `w_train` a **guess**, proposed at `0.3` — authored priors stay
dominant because they were built to cover the confusion-table pairs
(`rn`/`m`, `cl`/`d`, etc., `ARCHITECTURE.md` §2/§5) that this training
corpus was never selected to be representative of, while the training
count nudges common-English and filing-domain digraphs the authored table
under- or over-weights. `w_train` and the two-part lexicon-addition
threshold both belong on chunk 12's tuning list (they are exactly the kind
of number `PLAN.md`'s benchmark chunk expects to sweep) rather than being
locked in here.

Category-level backoff (`letter-letter`, `letter-digit`, etc.) is left
untouched by this proposal — the counted totals above are consistent with,
not contradictory to, the category defaults already authored
(`crates/ocrcer-build/src/bigrams.rs`), so no change to the backoff
structure itself is proposed.

## NOTICE

`NOTICE` (repo root) exists and currently only covers the optional LLM
add-on's converted model weights (Qwen3-0.6B, Qwen2.5-0.5B-Instruct,
Apache-2.0) — it states plainly that "the core OCR engine, prototype bank,
lexicon, and decoder tables have no third-party content," which is accurate
today. **If a future authoring pass adds any word or bigram to
`model/lexicon.txt` / `model/bigram_priors.tsv` sourced from these counts,**
`NOTICE` will need a new entry for `multifinben-englishocr` (Apache-2.0,
https://huggingface.co/datasets — MultiFinBen), parallel in form to the
existing Qwen entries, per CLAUDE.md rule 2 ("Attribution licences... are
recorded in the model's `meta` and in `NOTICE`"). This chunk added nothing
to either table, so no NOTICE edit was made or is yet required.

## What this chunk did not do

No edit to `model/lexicon.txt`, `model/bigram_priors.tsv`, `NOTICE`, or any
file under `model/`. No benchmark run. No CORD/SROIE rendering. The counter
never read any `score`- or `validation`-split row, any `pages/finfilings`
(60-page score set) or `pages/finfilings-val` file, or any `pages/cord`/
`pages/sroie` file — enforced by the manifest filter, the per-row
`assert_fittable` check, and the tool's own `-train`-suffix directory-name
gate, all three independently.
