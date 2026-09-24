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
- Everything else is a candidate. Each candidate row is assigned a
  deterministic fraction in `[0, 1)` from a hash of its `(dataset, row_id)`
  key; a fraction under 0.0523 is `train`, the next 0.0131 is `validation`,
  everything else is left unlisted (neither trained on nor scored — simply
  not needed at this corpus size). This produced **427 train, 103
  validation** — both within a few rows of the ~400/~100 target, and both
  numbers are a property of the fixed fractions and the fixed row set, not a
  knob tuned to hit a target after the fact.

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
`bench/pages-cov`, `bench/sizesweep*` and every fixture under `fixtures/`
are synthetic pages rendered directly from authored text and fonts — they
are not rows of any external dataset, so they carry no manifest entry. They
are scoring-only by construction (that is what a fixture and a held-out set
are for) and stay that way for the same reason the firewall exists at all.

## What could and couldn't be checked for near-duplicates

`multifinben-englishocr`'s Arrow schema is exactly `{image: string, text:
string}` — no document id, no filing id, no source-file column of any kind
(verified directly against the parquet schema, not inferred from the
README). That means the strongest duplicate check available is **exact
transcript-text equality** (SHA-1 of the full cleaned text), which is what
was run: every one of the 7,961 rows was hashed and compared against the 60
finfilings score rows' hashes, catching 123 exact matches.

This check was **not** able to catch a row that is a different page, crop,
or re-transcription of the *same underlying filing* as a score row but with
even one differing character — e.g. a different page of the same 10-K, or
the same page re-OCR'd with different whitespace. Nothing in the dataset
lets that be checked without either a document-id column (absent) or
comparing rendered images (out of scope for a metadata-only index file, and
still heuristic). This limitation is stated here rather than papered over:
the 123-row exclusion is a floor on how much leakage was caught, not a
guarantee that none remains.

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
