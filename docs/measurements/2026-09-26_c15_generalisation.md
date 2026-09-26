# Chunk 15 diagnosis: unseen-page per-crop accuracy — the pre-registered diagnostic

Architect-assigned diagnosis, **no parameter, weight, or default change**.
Answers the single diagnostic `ARCHITECTURE.md` §11's "Chunk 15 mode 2 fails"
entry (2026-09-26) pre-registered as the decision point for whether the net
continues:

> per-crop top-1 accuracy of the net and of the matcher on **unseen-page**
> crops, aligned to ground truth, through the runtime crop path. If the net's
> accuracy on unseen crops falls clearly below its val figure, or below the
> matcher's: generalisation... If it stays above the matcher's: the loss sits
> in the decoder interaction (hypothesis 3). The net is paused until someone
> proposes a specific mechanism. Both outcomes are decided by this entry in
> advance.

Scope discipline observed throughout: no read of `finfilings`,
`finfilings-val`, `pages-cov`, any fixture, or `bench/ident`. No page text is
committed; class pairs, counts and aggregate stats only.

## Verdict: net does not generalise worse per crop — the loss sits in decoder interaction

**Measured.** On unseen finfilings-train pages the net never trained on, its
per-crop top-1 accuracy is **99.27%** (n=10,914) — *above*, not below, both
its internal cluster-disjoint val figure (95.75%, `ARCHITECTURE.md` §11) and
the matcher's accuracy on the identical crops (93.16%). The same holds on
train-fold pages: net 99.50% (n=14,389) vs matcher 92.72%.

Per the decision this entry pre-registered: the net's unseen accuracy did
**not** fall below its val figure or below the matcher's — it stayed clearly
above the matcher's on both samples. **Outcome 2 applies: this is not a
generalisation problem.** The net is paused with the loss attributed to
decoder interaction (hypothesis 3), per the pre-registered rule, until a
specific mechanism is proposed and measured. No further training-data lever
(more fonts, more pages) is indicated by this result.

## 1. Method

New binary, this branch: `crates/ocrcer-bench/src/bin/gen15_probe.rs`. Reuses
the runtime crop path exactly as `skew_probe.rs` and `nn15_dump.rs`'s
rule-4-fixed real-positive path do — no second segmentation or feature-
extraction implementation (rule 4): `binarize::binarize_with` →
`deskew::estimate_with`/`correct_with` → `binarize::binarize_with` →
`underline::strip_underlines` or `components::label`+`components` →
`lines::group_with_bands` → `words::split_band_with` → `segment::build_with` →
`segment::crop` → `Glyph::input` → `feature::extract_with_grid` →
`Model::standardise`.

Alignment gate, identical to `nn15_dump.rs`'s (b) real-positive path: a truth
line pairs with a decoded line by index only when word counts match exactly;
a truth word pairs with a decoded word only when character counts match
exactly; each qualifying character's crop is rebuilt from the reconstructed
lattice and accepted only on an exact box match against the decoded
`CharBox.rect`. Both classifiers (`net.forward`, `match::nearest` with
`italic_ok=true`, per its own doc comment) are then evaluated on the
identical extracted `G`/`X` for that crop — never on separately-built inputs
— so the two accuracies are never computed on different data. One decode per
page at the shipped default (`match.classifier=0`); this only affects which
characters *qualify* for scoring (the segmentation), not which class each
qualifying crop is labelled by, which is read independently for both
classifiers.

Model under test: freshly built in this worktree from
`D:/Dev/ExcludedPrivate/ocrcer/nn15b/weights/cpu_det_run1` — 50,095
prototypes, 54 faces, params 80 rows (11 authored / 23 measured / 11 fitted /
35 guess), int8 top-1 agreement 99.493%. Matches the reference build stats in
`2026-09-26_c15_measure.md` and `2026-09-26_c15_skew.md` exactly — no config
drift.

Two samples, 20 pages each (sorted-stem order, `skip(offset).step_by(stride)`
convention shared with `ocr.rs`):
- **unseen**: `D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train-unseen`
  (320 pages never in the net's training split), stride 2, no split filter.
- **trainfold**: `D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train`,
  filtered to the 107 stems in `bench/splits/nn15_page_split.tsv`, stride 1.

Both runs completed with `no_edge_match: 0` and `out_of_charset: 0` — every
character that passed the word/line count-match filters found an exact
lattice-edge box match; no fuzzy or forced alignment was used anywhere.

**Caveat on what population this measures (stated, not hidden):** the
alignment gate only scores characters whose enclosing word decoded with the
correct character count and whose crop rebuilds to an exact box match. That
selects for already-cleanly-segmented characters — it is not a random sample
of every glyph on the page, and it is stricter than however the internal
val fold (`nn15b`) was sampled. This is the same gate `nn15_dump.rs` uses and
is exactly what the architecture entry asked for ("through the runtime crop
path"), but the absolute accuracy levels here are not directly comparable in
composition to the val-fold number, only the net-vs-matcher ordering within
this sample is safe to compare — which is the comparison the decision rule
actually turns on.

## 2. Headline accuracies

| sample | net accuracy | matcher accuracy | n scored | pages |
|---|---|---|---|---|
| train-fold (in-domain) | **99.50%** (14,317/14,389) | 92.72% (13,341/14,389) | 14,389 | 20 |
| unseen (held out from net training) | **99.27%** (10,834/10,914) | 93.16% (10,168/10,914) | 10,914 | 20 |

Net leads the matcher by ~6.1–6.8 pp on both samples, and the unseen-vs-
train-fold net gap is 0.23 pp — noise-scale on n≈10–14K, not a generalisation
drop. **Measured**, both figures, this run.

Reference points (**measured, prior runs**, restated for comparison only):
net internal cluster-disjoint val, per crop: 95.75% overall (matcher 91.99%,
same fold); net through the runtime crop path on 124 train-fold rows:
96.77%. The 99.27–99.50% figures here are higher than both — consistent with
the caveat above (this sample selects for exact-alignment-qualifying crops,
a cleaner population than either prior measurement's).

## 3. Per-class accuracy (digits 0–9, uppercase A–Z)

Small-n classes are flagged; a single-digit sample is noise, not signal
(honesty rule: do not report a difference the sample is too small to
support).

### 3.1 Unseen

| class | n | net acc | matcher acc |
|---|---|---|---|
| 0 | 1262 | 100.0% | **56.97%** |
| 1 | 620 | 100.0% | 100.0% |
| 2 | 1872 | 100.0% | 100.0% |
| 3 | 292 | 100.0% | 100.0% |
| 4 | 330 | 100.0% | 100.0% |
| 5 | 287 | 100.0% | 100.0% |
| 6 | 97 | 100.0% | 100.0% |
| 7 | 142 | 100.0% | 100.0% |
| 8 | 7 | 100.0% | 100.0% (n=7, noise) |
| 9 | 8 | 87.5% | 100.0% (n=8, noise) |
| A | 7 | 85.7% | 85.7% (n=7, noise) |
| B | 15 | 100.0% | 100.0% |
| C | 15 | 93.3% | 86.7% |
| D | 11 | 90.9% | 100.0% (n=11, noise) |
| E | 30 | 93.3% | 100.0% |
| F | 16 | 100.0% | 100.0% |
| G | 1 | 100.0% (n=1, noise) | 100.0% (n=1, noise) |
| H | 16 | 100.0% | 100.0% |
| I | 50 | 100.0% | 100.0% |
| J | 6 | 100.0% (n=6, noise) | 100.0% (n=6, noise) |
| K | 1 | 0% (n=1, noise) | 100.0% (n=1, noise) |
| L | 15 | 100.0% | 100.0% |
| M | 12 | 91.7% (n=12, noise) | 91.7% (n=12, noise) |
| N | 8 | 100.0% (n=8, noise) | 100.0% (n=8, noise) |
| O | 14 | 50.0% | 50.0% |
| P | 3 | 100.0% (n=3, noise) | 100.0% (n=3, noise) |
| Q | 2 | 50.0% (n=2, noise) | 50.0% (n=2, noise) |
| R | 16 | 75.0% | 100.0% |
| S | 46 | 91.3% | 87.0% |
| T | 16 | 93.8% | 100.0% |
| U | 1 | 100.0% (n=1, noise) | 100.0% (n=1, noise) |
| V | 2 | 100.0% (n=2, noise) | 100.0% (n=2, noise) |
| W | 6 | 83.3% (n=6, noise) | 66.7% (n=6, noise) |
| X | 3 | 66.7% (n=3, noise) | 66.7% (n=3, noise) |
| Y | 1 | 100.0% (n=1, noise) | 100.0% (n=1, noise) |

### 3.2 Train-fold

| class | n | net acc | matcher acc |
|---|---|---|---|
| 0 | 1462 | 99.86% | **45.62%** |
| 1 | 845 | 100.0% | 100.0% |
| 2 | 1705 | 100.0% | 100.0% |
| 3 | 302 | 100.0% | 100.0% |
| 4 | 241 | 100.0% | 100.0% |
| 5 | 462 | 100.0% | 100.0% |
| 6 | 41 | 97.6% | 97.6% |
| 7 | 60 | 100.0% | 100.0% |
| 8 | 52 | 100.0% | 100.0% |
| 9 | 57 | 100.0% | 100.0% |
| A | 31 | 96.8% | 96.8% |
| B | 5 | 80.0% (n=5, noise) | 80.0% (n=5, noise) |
| C | 37 | 100.0% | 100.0% |
| D | 15 | 100.0% | 100.0% |
| E | 26 | 100.0% | 100.0% |
| F | 9 | 77.8% (n=9, noise) | 77.8% (n=9, noise) |
| G | 7 | 100.0% (n=7, noise) | 100.0% (n=7, noise) |
| H | 3 | 100.0% (n=3, noise) | 100.0% (n=3, noise) |
| I | 34 | 94.1% | 94.1% |
| L | 6 | 100.0% (n=6, noise) | 100.0% (n=6, noise) |
| M | 27 | 100.0% | 100.0% |
| N | 14 | 100.0% (n=14, noise) | 100.0% (n=14, noise) |
| O | 60 | 100.0% | 100.0% |
| P | 32 | 96.9% | 96.9% |
| R | 6 | 100.0% (n=6, noise) | 100.0% (n=6, noise) |
| S | 58 | 100.0% | 100.0% |
| T | 28 | 100.0% | 100.0% |
| U | 2 | 100.0% (n=2, noise) | 100.0% (n=2, noise) |
| V | 1 | 0% (n=1, noise) | 0% (n=1, noise) |
| W | 1 | 100.0% (n=1, noise) | 100.0% (n=1, noise) |
| Y | 1 | 100.0% (n=1, noise) | 100.0% (n=1, noise) |

Some letters absent from a table (e.g. `Q`, `X` in train-fold) simply had
zero qualifying crops in that 20-page sample. **Consistent signal across
both samples:** the matcher's digit "0" accuracy is badly depressed
(45.6–57.0%) almost entirely by 0→O confusion, the single class error the
net's addition to the decoder was meant to fix (`ARCHITECTURE.md` §11,
"the single wanted win... 0→O"). That win is confirmed present at the
per-crop level in both samples — it just does not translate into an
end-to-end win once fused into the decoder (mode 2 measurement,
`2026-09-26_c15_mode2.md`).

Per-group summary (**measured**):

| group | unseen n | unseen net | unseen matcher | train-fold n | train-fold net | train-fold matcher |
|---|---|---|---|---|---|---|
| digit | 4917 | 99.98% | 88.96% | 5227 | 99.94% | 84.77% |
| upper | 313 | 91.69% | 93.29% | 403 | 98.01% | 98.01% |
| other | 5684 | 99.07% | 96.80% | 8759 | 99.30% | 97.21% |

Uppercase is the one group where the matcher edges the net on unseen pages
(93.29% vs 91.69%), consistent with the internal val-fold table (net 85.78
vs matcher 90.62 on uppercase) — the net's known uppercase weakness holds up
on real unseen pages, though the unseen upper sample here (n=313) is thin
enough that a few percentage points either way should not be over-read.

## 4. Net confusions and confidence on errors

Total net errors: 80/10,914 on unseen (0.73%), 72/14,389 on train-fold
(0.50%). Top confusions, class pairs and counts only, no page text:

**Unseen** (top 20 by count): O→o (6), '→l (5), S→s (4), e→t (3), E→u (2),
R→n (2), R→r (2), a→i (2), t→e (2), then 11 singletons including (→T, )→u,
.→d, .→e, 9→M, A→g, C→c, D→r, K→c, M→a, O→1.

**Train-fold** (top 20 by count): r→e (3), 0→o (2), d→s (2), e→h (2), e→r
(2), i→. (2), o→t (2), r→a (2), then 12 singletons including 6→3, A→t, B→1,
F→5, F→i, I→,, I→w, P→s, V→v, a→O, a→o, a→p.

Most confusions are case-folding (O→o, S→s) or shape-adjacent lowercase
pairs (r→e, e→h, o→t) — none are systematic digit/uppercase-class errors of
the kind the mode-2 measurement flagged (2→8, C→S, )→h); this sample is too
small (72–80 errors) to say those specific confusions are absent, only that
they did not recur here.

**High-confidence error fraction (measured):** of the net's own errors,
58.75% (47/80) on unseen and 79.17% (57/72) on train-fold were made at
p ≥ 0.9. This reproduces `2026-09-26_c15_skew.md`'s finding that the net's
confidence does not discriminate hard cases well (there: 99.3% of clean-edge
crops score > 0.8 regardless of correctness) — most of the net's mistakes
are made with high stated confidence, not low.

## 5. Reading for the decoder-interaction hypothesis (inferred, not this task's to decide)

This diagnostic's own scope is descriptive; the mechanism proposal below is
offered for `ocrcer-linguist`/`ocrcer-architect` to evaluate, not decided
here. The net wins clearly at the crop level on both in-domain and unseen
pages, yet mode 2 fusion lost end to end by a wide margin
(`2026-09-26_c15_mode2.md`: +44×EPS end-to-end, +99×EPS line-matched), with
the mode-2 entry's own finding that `d_seg` (segmentation signal) is
unaffected by the net term — the loss is a *class-choice* loss, not a
segmentation one. Combined with §4's finding that a majority-to-large-
majority of the net's (rare) errors carry p ≥ 0.9: a plausible, specific
mechanism is that the net's softmax output does not behave like the
matcher's `d1/d2` margin the decoder weights were fitted against — the net
can be confidently wrong on the crops it does miss, in a way the matcher's
distance-based scoring is not (the matcher's own errors tend to correlate
with weaker distances, which the decoder's other terms — lexicon, bigram —
can then out-vote; a net error carrying a strong, confident vote may not get
out-voted the same way). This is a testable, specific mechanism (as
`ARCHITECTURE.md` §11 requires before the net is un-paused): compare the
net's top1/top2 log-prob **margin**, not its raw top1 probability, as the
fused signal, on the theory that margin — not absolute softmax mass — is
the property comparable to the matcher's `d1/d2`. That is a decoder-formula
change, `ocrcer-linguist`'s territory, not this report's to make.

## 6. What this does not change

No parameter, weight, or default was changed. `match.classifier` stays 0.
Mode 2 stays unmerged, per its own entry. This diagnostic closes the
generalisation branch of §11's open question with a measured "no"; the
decoder-interaction branch stays open, paused, pending the specific-
mechanism proposal §11 requires.
