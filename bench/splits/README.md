# Training / scoring split manifest

`manifest.tsv` is the committed firewall required by `PLAN.md` section 2c's
2026-09-24 amendment and `CLAUDE.md` rule 1: every row from every dataset
this project might fit parameters against is assigned to exactly one of
`train`, `validation`, `score`, or `excluded` **before any fitting runs**,
and a scoring row never moves. `ocrcer_bench::splits::assert_fittable`
refuses any row not listed as `train` or `validation` — a fitting script
that calls it cannot accidentally read a scoring row even by a typo'd id.

## Regenerating

```
cargo run -p ocrcer-bench --bin split            # writes bench/splits/manifest.tsv
cargo run -p ocrcer-bench --bin split -- --check # fails if the committed file would change
```

The tool re-derives the manifest byte-identically from two committed input
files (`multifinben_index.tsv`, `finfilings_rows.tsv`) plus fixed row-count
constants for CORD-v2 and SROIE. No RNG, no external state at run time — the
sampling is a deterministic hash of `dataset\x1frow_id`, so re-running it a
year from now on the same inputs reproduces the same manifest.

## Sources and how each was split

| Dataset | Licence | Rows | Rule |
|---|---|---|---|
| `multifinben-englishocr` | Apache-2.0 | 7,961 total | see below |
| `cord-v2` | CC-BY-4.0 | 1,000 total | official split, used verbatim |
| `sroie` | CC-BY-4.0 | 987 total | official split, used verbatim |

**`multifinben-englishocr`** (train shard only; the dataset ships no other
split):

- The 60 rows already spent rendering `pages/finfilings` are `score`,
  permanently, regardless of anything else about them.
- Any other row whose transcript text is byte-identical to one of those 60
  rows' text is `excluded` (123 rows) — treated as the same source document,
  since a duplicate transcript almost certainly means a duplicate or
  near-duplicate underlying document, and letting it into training would
  leak scoring material into the model. See "What could and couldn't be
  checked" below.
- Any remaining row with fewer than 200 cleaned characters is `excluded`
  (124 rows) — below `parquet_corpus.py`'s own `--min-chars` floor, so it
  would never have been rendered into a page in the first place and has no
  business being training material either.
- Any remaining row whose word-5-gram shingle containment against a score
  row exceeds `NEAR_DUP_THRESHOLD` is `excluded` (4,255 rows) — see
  "Near-duplicate check" below.
- Everything else is a candidate. Candidates are sorted by a deterministic
  fraction in `[0, 1)` derived from a hash of each row's `(dataset, row_id)`
  key (`sample_fraction`, unchanged), and the lowest `MULTIFINBEN_TRAIN_TARGET`
  (427) fractions are `train`, the next `MULTIFINBEN_VAL_TARGET` (103) are
  `validation`; everything past that is left unlisted (neither trained on nor
  scored). This is a **count-based prefix of the sorted survivors**, not a
  fixed fraction window: because the near-duplicate check removes some
  candidates before this step, a fixed window would produce a train/validation
  size that drifts every time the near-dup exclusion count changes. Selecting
  the lowest N surviving fractions instead reaches the same 427/103 targets
  regardless of how many rows the near-dup check removes, while staying fully
  deterministic (same hash, no RNG, no post-hoc size tuning). Row order in
  the manifest is still index order, not fraction order — selection uses the
  fraction sort internally, then a second pass emits rows in their original
  `(shard, row)` order so the file's row ordering is unaffected by the
  refill mechanism.

**`cord-v2`** and **`sroie`**: both datasets' official splits are used
whole, no sampling needed.

- CORD-v2: official `train` (800 rows) and `validation` (100 rows) are
  listed as such. Official `test` (100 rows) is `score` — `pages/cord`
  already rendered a 95-row sample of exactly this shard.
- SROIE: official `train` (626 rows) is listed as `train`; SROIE has no
  official validation split, so none is claimed. Official `test` (361 rows)
  is `score` — `pages/sroie` already rendered a 120-row sample of exactly
  this shard.

**Excluded entirely, not in this manifest at all:**

- `scribeocr` — AGPL and scene text; out of scope on both licence (rule 2)
  and domain (rule 7) grounds.
- The CRA forms under `inbox/` — not licence-cleared for use.
- IRS forms — US federal works; left out for now per the task instruction,
  not because of a licence problem.

**Noted, not listed as rows:** `bench/holdout`, `bench/holdout-cov`,
`bench/pages-cov`, `bench/sizesweep*`, `bench/ident` and every fixture under
`fixtures/` are synthetic pages rendered directly from authored text and
fonts — they are not rows of any external dataset, so they carry no
manifest entry. They are scoring-only by construction (that is what a
fixture and a held-out set are for) and stay that way for the same reason
the firewall exists at all.

`bench/ident` (`cargo run -p ocrcer-bench --bin ident`) is a further case of
the same rule: it is dense in identifier-shaped tokens by design (see
`crates/ocrcer-bench/src/ident_corpus.rs`'s header), built to catch one
specific failure mode (`CLAUDE.md` rule 6) rather than to represent a
document mix, and its numbers are never a fitting input for the same reason
none of the corpora above are.

## What could and couldn't be checked for near-duplicates

`multifinben-englishocr`'s Arrow schema is exactly `{image: string, text:
string}` — no document id, no filing id, no source-file column of any kind
(verified directly against the parquet schema, not inferred from the
README). That means exact match (SHA-1 of the full cleaned text) cannot
catch a row that is a different page, crop, or re-transcription of the
*same underlying filing* as a score row but with even one differing
character — e.g. a different page of the same 10-K, or the same page
re-OCR'd with different whitespace. Nothing in the dataset lets that be
checked with a document-id column (absent) or by comparing rendered images
(out of scope for a metadata-only index file, and still heuristic). The
near-duplicate check below is the mitigation for exactly this gap; it is a
better floor than exact match alone, not a proof that no leakage remains.

## Near-duplicate check

123 exact-text duplicates in a 7,961-row corpus is a heavy enough
duplication rate that near-duplicates — the same filing re-OCR'd, adjacent
pages of the same filing, boilerplate-heavy pages — are likely too. The
check: normalise every candidate row's cleaned transcript (lowercase,
whitespace-collapsed), build word 5-gram shingles, and for every remaining
MultiFinBen row (after the exact-dup and too-short exclusions) compute
containment `|A∩B|/|A|` against each of the 60 `pages/finfilings` score
rows, where `A` is the candidate — max over score rows. This is computed
once by `tools/multifinben_near_dup.py score-containment` (an inverted
shingle index over the 60 score rows, one pass over the candidate pool) and
committed as metadata-only (`bench/splits/multifinben_near_dup.tsv`: shard,
row, containment, best-matching score row — no corpus text), the same
pattern `multifinben_index.tsv` already uses. `splits::assign_multifinben`
reads it and excludes any row whose containment exceeds
`NEAR_DUP_THRESHOLD`, same firewall-before-fitting reasoning as the
exact-dup check.

**Distribution over the 7,654 rows checked** (post exact-dup/too-short
exclusion):

| Bucket | Count |
|---|---|
| 0 | 3,399 |
| (0, 0.1] | 2,095 |
| (0.1, 0.3] | 93 |
| (0.3, 0.5] | 21 |
| (0.5, 0.8] | 234 |
| (0.8, 1] | 1,812 |

The distribution is bimodal, not a smooth falloff: a large mass at or near
zero (unrelated filings), a second large mass near 1.0, and comparatively
few rows in between. That shape is itself informative — the near-1.0 mass
is almost entirely template-boilerplate reuse (see below), not a gradient
of "somewhat related" filings.

**Threshold decision: `NEAR_DUP_THRESHOLD = 0.0`, not the 0.3 starting
point.** Eyeballing text (`tools/multifinben_near_dup.py dump-text`) across
the full range surfaced two distinct phenomena that overlap in containment
score and cannot be told apart by the number alone:

- **Genuine same-filing leakage at LOW containment.** Henry Schein, Inc.
  Exhibit D continuation pages matched a score row at containment
  0.078–0.124 — well under 0.3 — confirmed by matching CUSIP numbers,
  consecutive page numbers, and continuing legal-clause text across the
  pair. TransAlta / Brookfield Schedule 13D reporting-person continuation
  pages matched at 0.103–0.300, the same phenomenon spanning right up to
  the suggested threshold. A 0.3 cutoff would have let these into training.
- **Shared industry boilerplate at HIGH containment between unrelated
  filers.** Broadway Financial Corporation vs. Henry Schein, Inc. — two
  unrelated companies — share an EX-24 power-of-attorney template at
  containment 0.349. Guggenheim Securities vs. Finantia USA Inc. share
  generic GAAP footnote language at 0.077. Form N-PORT Part C's
  standardized checkbox/label template is reused verbatim by many unrelated
  funds at containment 0.55–0.99. None of these are the same document —
  company names, CIKs, and substantive content differ — they are
  regulatory-form boilerplate.

Because genuine leakage was observed as low as 0.078 and false-positive
boilerplate reused as high as 0.99, **no single containment threshold
separates the two populations**, and the two phenomena are not reliably
distinguishable without reading each pair by hand (infeasible at this
corpus size). Given the project's firewall-first posture (`CLAUDE.md` rule
1, "never tune on the test set", and this project's general preference for
a smaller clean corpus over a larger contaminated one), the threshold is
set to the floor: any candidate with *nonzero* shingle overlap against a
score row is excluded. This is a deliberate, one-time adjustment from the
task's 0.3 starting point, made and recorded here per the instruction that
permits adjusting a clearly-wrong threshold once. It costs boilerplate rows
that were probably safe (the false-positive side), in exchange for not
needing to adjudicate the ambiguous middle by eye. The candidate pool was
large enough to absorb this: 3,399 rows have exactly zero containment,
comfortably above the 530 (427+103) needed, so the refill mechanism above
reaches both targets without exhausting the zero-containment pool.

This raised total MultiFinBen exclusions from 247 (123 exact-dup + 124
too-short) to **4,502** (123 exact-dup + 124 too-short + 4,255 near-dup).
Train/validation counts are unchanged at 427/103 — the refill mechanism
(see above) absorbed the larger exclusion count by construction.

**Top 10 candidates by containment** (all near-1.0, all boilerplate-driven
per the eyeballing above, all excluded under the threshold):

| Candidate | Containment | Best-matching score row |
|---|---|---|
| `train-00005-of-00008.parquet#000969` | 1.0000 | `train-00000-of-00008.parquet#000363` |
| `train-00000-of-00008.parquet#000046` | 1.0000 | `train-00000-of-00008.parquet#000044` |
| `train-00000-of-00008.parquet#000145` | 0.9981 | `train-00000-of-00008.parquet#000242` |
| `train-00007-of-00008.parquet#000069` | 0.9941 | `train-00000-of-00008.parquet#000363` |
| `train-00007-of-00008.parquet#000027` | 0.9941 | `train-00000-of-00008.parquet#000363` |
| `train-00006-of-00008.parquet#000963` | 0.9941 | `train-00000-of-00008.parquet#000363` |
| `train-00006-of-00008.parquet#000913` | 0.9941 | `train-00000-of-00008.parquet#000363` |
| `train-00004-of-00008.parquet#000659` | 0.9941 | `train-00000-of-00008.parquet#000363` |
| `train-00004-of-00008.parquet#000572` | 0.9941 | `train-00000-of-00008.parquet#000363` |
| `train-00004-of-00008.parquet#000531` | 0.9941 | `train-00000-of-00008.parquet#000363` |

### Train-vs-validation check (report only, not exclusionary)

The same shingle-containment method was run between the final `train` and
`validation` MultiFinBen rows (`tools/multifinben_near_dup.py
train-val-check`), report-only per the task — a train/validation leak is
less severe than a train/score leak (it can't inflate a benchmark result
against `ocrs`, only make the validation set a weaker check on
overfitting), so no exclusion is applied here.

| Bucket | Count (of 103 validation rows) |
|---|---|
| 0 | 35 |
| (0, 0.1] | 44 |
| (0.1, 0.3] | 10 |
| (0.3, 0.5] | 8 |
| (0.5, 0.8] | 2 |
| (0.8, 1] | 4 |

6 of 103 validation rows (5.8%) have containment ≥ 0.5 against some train
row, 3 of those at or near 1.0. This is worth knowing when reading
validation-set numbers during tuning: a handful of validation rows are
near-duplicates of training rows and will over-predict how well tuning
generalizes. It is not acted on here — re-splitting train/validation on
this signal would require a second refill pass and was out of scope for
this pass — but any tuning round that leans heavily on validation-set
movement should discount the ~6 rows flagged here.

## Rendered training/validation pages

`tools/manifest_render.py` renders every `train`/`validation` MultiFinBen
row into the same pgm+truth.json format `pages/finfilings` uses (it imports
`parquet_corpus.py`'s `image_bytes`/`clean_lines`/`write_pgm` rather than
reimplementing them, so the two renders can't silently diverge on what
counts as "the same page"):

```
python tools/manifest_render.py bench/splits/manifest.tsv \
    D:/Dev/ExcludedPrivate/ocrcer/datasets/multifinben-englishocr \
    train D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train
python tools/manifest_render.py bench/splits/manifest.tsv \
    D:/Dev/ExcludedPrivate/ocrcer/datasets/multifinben-englishocr \
    validation D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-val
```

Both directories live outside the repo (`D:/Dev/ExcludedPrivate/ocrcer/`),
same as every other rendered corpus. Current sizes: `finfilings-train` is
427 pages, 1.6 GB; `finfilings-val` is 103 pages, 381 MB. Stems are
`filing__s<shard-index>__r<row:06d>` — `pages/finfilings` needed no shard
marker since it only ever drew from shard 0; these renders pull from all 8
shards, whose row numbers repeat, so the shard index disambiguates.

## Format

```
#dataset	row_id	split	licence	reason
```

`row_id` is `<shard-file>#<row-number:06d>` for `multifinben-englishocr`
(row numbers repeat across the 8 shards, so the shard name disambiguates)
and `<parquet-file>#<row-number:06d>` for CORD-v2/SROIE. `split` is one of
`train`, `validation`, `score`, `excluded`. `reason` is free text explaining
the assignment, always specific enough to re-derive by hand.
