# The mirror cue does not survive to word level — measured, not projected

MEASURED, synthetic data, one generator pair (`tools/showthrough_lines.py` +
`tools/faded_lines.py`), one face. Private TSVs (not committed):
`D:/Dev/ExcludedPrivate/ocrcer/handoff_2026-09-25/probe_data/{pw_k34,pw_k10}.tsv`
(342 / 750 `PW` rows), from `hl_probe_src/main.rs`'s `OCRCER_PW` mode against
`model/out/ocrcer.ocrw` at Sauvola `k` 0.34 (shipped) and 0.10 (the
"reads through more" candidate from `2026-09-22_research_classical_techniques.md`).
Cross-checked against `mir_k34.conf`/`mir_k10.conf` (whole-page runs, same
images). Script: `tools/mirror_per_word_analysis.py <pw_k34.tsv> <pw_k10.tsv>
[mir_k34.conf mir_k10.conf]`.

## Ground truth

`front_L*` crops are whole `faded_lines.py` pages, no back page mixed in:
every word is real front text by construction. `back_F<frontink>_B<backluma>_*`
crops are cut from `showthrough_lines.py` pages from row 320 down — below the
four front lines, show-through only (`2026-09-22_research_classical_techniques.md`
lines 3985–4030). Attribution is by crop origin: `front_*` → `real`, `back_*` →
`show_through`.

## Size filter

Rect height is bimodal: real-glyph rows ≥21px, fragment rows (bare
punctuation blobs, illegible alone) 1–13px, clean gap at 14–17px. Filter:
height ≥ 15px. Drops 36/342 (k34, 10.5%), 240/750 (k10, 32.0% — lower `k`
makes far more sub-glyph noise blobs, corroborating the "noise cost
unmeasured" flag on `k=0.10` in the faded-ink addendum). After filtering,
72/306 (k34) and 179/510 (k10) rows still read zero characters in both
orientations (delta = 0, uninformative); kept and reported separately.

## Q1 — per-word delta (`flip_geomean − asis_geomean`), height filter only

| class | k | n | mean | median | sd | q1 | q3 |
|---|---|---|---|---|---|---|---|
| real (front) | 34 | 120 | +0.036 | 0.000 | 0.136 | 0.000 | +0.037 |
| show_through (back) | 34 | 186 | +0.020 | 0.000 | 0.116 | −0.036 | +0.073 |
| real (front) | 10 | 216 | +0.009 | 0.000 | 0.109 | 0.000 | 0.000 |
| show_through (back) | 10 | 294 | −0.012 | 0.000 | 0.133 | −0.028 | +0.043 |

Medians sit at or near 0 for both classes at both `k`; class means differ by
0.01–0.03 against a spread an order of magnitude larger. The page-level cue
says real text should drop on flip and show-through should rise; at word
level `real` trends weakly positive at both `k`, and `show_through` flips
sign between k34 (+0.020) and k10 (−0.012) — the per-word signal does not
reliably reproduce even the direction of the page-level cue.

## Q2 — threshold search, best `recall − false-flag`

`false-flag` = fraction of `real` above threshold (wrongly flagged);
`recall` = fraction of `show_through` above threshold.

| condition | k | best threshold | false-flag | recall | rec−ff |
|---|---|---|---|---|---|
| height filter only | 34 | >+0.008 | 0.308 | 0.430 | 0.122 |
| height filter only | 10 | >+0.000 | 0.231 | 0.347 | 0.115 |
| height, both-nonzero (n=84/150) | 34 | >+0.037 | 0.321 | 0.427 | 0.105 |
| height, both-nonzero (n=139/192) | 10 | >+0.000 | 0.360 | 0.531 | 0.172 |

Requiring nchars≥3 as well does not help: samples collapse to 19–74 rows and
the best point is near-zero recall at zero false-flag (k34) or a similarly
weak 0.053 (k10). Best across every condition tried: 0.172 (k10,
both-nonzero subset). At every threshold in the full sweep (−0.20 to +0.30),
false-flag and recall move together rather than recall staying high while
false-flag drops — the signature of no separation, not of a threshold
needing tuning. No usable per-word threshold on this probe.

## Q3 — word length dependence

1–2 char words: delta exactly 0.000 for `real` at both k (crops too small to
read in either orientation, even past the height filter). Beyond that, no
monotonic trend: `real len=10+` is +0.115 at k34 but −0.043 at k10, on only
14–22 rows — too small and too inconsistent across the one varied parameter
to support a length-dependence claim either way.

## Q4 — does the whole-page crop-mean effect reproduce per word?

Sanity check passes: per-word `word_conf` averaged per label matches
`mir_k*.conf`'s whole-page `mean_conf` to 2–3 decimals on all 22 labels at
both k — both harness runs read the same model/page state.

The flip side does not reproduce. Whole-page flip vs. mean isolated-word-crop
flip (one word, padded by half its own height, flipped alone):

| label | k | whole-page flip | mean word-crop flip |
|---|---|---|---|
| front_L000_none | 34 | 0.37 | 0.251 |
| back_F000_B185_lstretch | 34 | 0.54 | 0.213 |
| front_L000_none | 10 | 0.35 | 0.210 |
| back_F000_B185_lstretch | 10 | 0.45 | 0.129 |

Word-crop flip means run 0.13–0.25 against whole-page flip means of
0.24–0.54 (roughly 2–3x smaller), and the ordering across conditions does not
track either. **Inferred, not measured here:** the decoder scores words
independently, so cross-word language context is an unlikely cause. More
plausible: a one-word crop gives layout too few glyphs to estimate x-height,
baseline and slant, and the flipped read of a crop is dominated by that
estimate; a whole crop of several lines does not have this problem. Untested.

## Bottom line

Negative result on this harness's crop design, not on the underlying cue —
the whole-page measurement in the addendum stands on its own terms. A
half-height-padded single-word crop is not a usable per-word show-through
gate. Suggestion for `ocrcer-runtime`/`ocrcer-architect`, not a decision: if
a per-word or per-line confidence signal from mirroring is still wanted, the
next probe is a whole-line crop flip (all words on one line, not one word).
