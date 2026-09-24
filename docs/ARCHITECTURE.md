# OCRcer — architecture

The engine, the model, the weight format, and the Rust runtime contract.

**The governing constraint:** the model is **constructed, not fitted**. Every
number in the shipped model file is either a value authored from knowledge or a
value a deterministic script computes from rendered glyphs. There is no
training run, no dataset, no gradient descent, and therefore no corpus licence
to inherit.

That constraint chose the architecture. A convolutional recogniser's weights
are only reachable by fitting, so the engine is instead a **segmentation-driven
prototype-matching recogniser with a lattice decoder** — the design commercial
OCR used to reach 99% on clean print for two decades before neural methods,
and the only high-accuracy OCR design whose every parameter is constructible.

---

## 1. Pipeline

```
page image (grayscale, any size)
   |
   v  Sauvola adaptive binarization
   v  skew estimate and correction
   |
   v  connected-component labelling (two-pass union-find)
   v  noise rejection by area and aspect
   |
   v  line grouping, baseline and x-height estimation per line
   v  word splitting by bimodal gap analysis
   |
   v  character segmentation -> a LATTICE of hypotheses, not a single cut
   |
   v  per hypothesis: normalise, extract 107-dim feature vector
   v  prune candidate classes, match against the prototype bank
   |
   v  Viterbi decode over the lattice, scored by
   v     match distance + character bigram + lexicon membership
   |
   v  word assembly, confidence aggregation
   |
   v  Vec<Word> { text, rect, confidence, chars }
```

Coordinates leaving the engine are **image pixel coordinates, y-down** — the
contract pdfcer's `OcrEngine::recognize` requires. The engine never flips to PDF
user space; that belongs to pdfcer's `words_to_page_space` and only there.

**The design decision that carries the accuracy:** segmentation produces a
lattice, not a commitment. Touching characters, broken characters and the
`rn`/`m` family are not segmentation problems that can be solved before
recognition — they are resolved *by* recognition, jointly with the language
model, in one Viterbi pass. A pipeline that commits to a single cut before
classifying loses several points of accuracy on exactly the cases that matter
and cannot recover them downstream.

---

## 2. What the model file actually contains

This is the answer to "what is the trained data". Ten blocks, all
constructible, and **all ten exist** — eight as tables, the charset as a
record inside `meta`, and `feature_weights` as an optional ninth table
present only when the matcher has an opinion to state. Every figure in the
Size column was read out of the file on disk; nothing here is an estimate any
more.

| Block | What it is | Source | Size |
|---|---|---|---|
| `meta.charset` | 187 classes, one record each: class index, codepoint, category, case twin | Authored | in `meta`, below |
| `feature_norm` | per-dimension mean and standard deviation for standardisation | Computed over the bank | **856 B**, measured |
| `prototypes` | N x 107 int8 feature vectors | Computed by rendering | **1,884,270 B**, measured |
| `prototype_class` | the class index of each prototype row, u16 | Computed | **35,220 B**, measured |
| `class_holes` | hole count per class, u8 — the realised form of the pruning index | Computed over the bank | **187 B**, measured |
| `lexicon` | word list as a DAWG, with frequency rank | Authored | **38,937 B**, measured |
| `bigrams` | sparse character-pair log-probabilities plus a category backoff table | Authored | **9,432 B**, measured |
| `confusions` | known confusion pairs with prior adjustments and disambiguating tests | Authored | **707 B**, measured |
| `params` | every threshold in the pipeline | Authored | **1,161 B**, measured |
| `feature_weights` | one weight per feature dimension, authored per section 3.1 block | Authored, one block measured | **428 B**, measured; present |

Plus `meta` itself — **13,590 B measured**, UTF-8 JSON carrying the charset,
the feature-extractor version and dimension count, the build size ladder, the
face manifest with each face's distribution, and the build identifier.

**The file on disk today is 1,986,028 B — 1.89 MB** at five sizes and 17,610
prototypes, carrying all nine tables. The four language tables cost 50,237 B
between them — **2.53% of the file** — against the ~338 KB this table projected
for them, because a case-folded DAWG over 2,181 authored entries compresses far
harder than a word list does. The previous line here projected "about 2.2 MB"
for this ladder from the measured per-scale prototype cost; the rebuilt file
measures 1.89 MB, so that projection was 16% high and is now replaced by the
reading. The earlier headline, ~1.7 MB, assumed a bank of ~12,000 prototypes
and is superseded. The comparison it was making still holds by a wide margin:
`ocrs` is 12.24 MB.

**This file is still not a release candidate.** It is now built at the ladder
every accuracy figure here is quoted at, which removes the silent
file-versus-record disagreement, but it does not make the ladder blessed: the
sentence behind 16/20/24/32/48 has still never been written, and until
`ocrcer-glyphs` authors it as a spacing rule no `.ocrw` is canonical. See
section 11, 2026-09-22, *The `.ocrw` on disk is built at a ladder none of this
project's measurements endorse*, and the entry that rebuilt it.

Three things this table used to claim that the emitted file does not do, all
audited on 2026-09-22 and all resolved in section 11's entry
*The model file was audited against section 2*:
the charset is a record inside `meta` rather than a table and carries no
baseline class and no aspect band; prototype rows carry class only, not font
family and style; and the pruning index is one byte per class rather than
bucket lists. The fourth — that `params` did not exist and the thresholds were
still Rust constants — no longer holds: the table ships, and `ocrcer-build`
fails the build when `model/params.tsv` and `Params::DEFAULT` disagree on any
row.

This table describes the **base segment**. Section 11's 2026-09-21
segmentation entry adds a face manifest and optional, non-shipped
segments on top of it; read the two together.

Two of these tables are where prior knowledge is the actual deliverable rather
than a means to one. `bigrams` and `confusions` encode things that are genuinely
known and genuinely hard to get any other way — that `rn` and `m` are the
single most costly confusion in Latin OCR, that `1`/`l`/`I`/`|` separate on
serif presence and baseline contact rather than on shape, that `0`/`O` separate
on aspect ratio and in most fonts on the presence of a slash or the ratio of
counter to stroke, that `cl` and `d` separate on the gap at x-height. Each of
those is a rule plus a threshold, and each is worth more than a large number of
training samples.

---

## 3. The feature vector

107 dimensions per glyph hypothesis. Chosen for discrimination under noise, not
for elegance.

The glyph bitmap is first normalised: centroid-centred, scaled to fit 32x32
preserving aspect ratio, with the original aspect ratio retained separately as
a feature rather than destroyed by the scaling.

| Feature group | Dims | Why it is in the set |
|---|---|---|
| 4x4 zone ink density | 16 | Coarse mass distribution; robust to everything |
| 4x4 zones x 4 gradient orientations | 64 | Stroke direction, the dominant discriminative signal |
| Horizontal projection profile, 8 bins | 8 | Separates `E`/`F`, `P`/`R` |
| Vertical projection profile, 8 bins | 8 | Separates `n`/`h`, `u`/`v` |
| Hole count (Euler number) | 1 | Extremely high value: splits the charset into three near-disjoint groups before any distance is computed |
| Crossing counts, 3 horizontal + 3 vertical cuts | 6 | Separates `S`/`5`, `B`/`8` |
| Aspect ratio, ink fraction, height above baseline, depth below | 4 | Baseline-relative geometry is what separates `o`/`O`, `p`/`P`, `,`/`'` |

**This table was checked against the extractor on 2026-09-22 and matches** —
same groups, same sizes, same order, summing to 107 — after the audit of that
date found section 2 had drifted from what the writer emits. It was worth
checking because this is the most expensive table in the project to have
wrong: `docs/measurements/2026-09-22_ocrw_file_audit.txt` §6 has the
range-by-range comparison. The one known defect nearby is not a drift in this
table but in `charset.tsv`'s authored aspect band, which is a different
denominator from the aspect dimension here; see section 11, 2026-09-22.

The last row is the one most often left out of naive implementations and it is
responsible for a large fraction of case errors. A glyph's identity in print is
not scale-invariant: `O` and `o` are the same shape and different characters,
and only the line's baseline and x-height tell them apart. Hence the per-line
baseline estimation in section 1, which exists to serve these four dimensions.

### 3.1 The exact extractor contract

Section 3 says which features exist and why. This section says exactly how they
are computed, because the extractor runs in two places — building the bank and
recognising a glyph — and "approximately the same feature" is the one failure
mode the single-implementation rule exists to prevent. An implementer must not
have to choose any of the following.

**Input.** The extractor takes the glyph's own binary bitmap and two numbers
from the line it sits on. It never sees page coordinates.

```rust
pub struct GlyphInput<'a> {
    pub ink: &'a [u8],     // width*height, row-major, 0 = background, 1 = ink
    pub width: u32,
    pub height: u32,
    pub baseline_dy: f32,  // baseline, in pixels, measured downward from bbox top
    pub x_height: f32,     // the line's x-height in pixels, strictly positive
}
```

`baseline_dy` may exceed `height` or go negative; a glyph that sits entirely
above the baseline and one that hangs below it are both legitimate.

**Normalisation.** Let `s = 32.0 / max(width, height)` and let `(cx, cy)` be the
ink centroid in bitmap coordinates. Source maps to a 32x32 grid `G` by

```
dst_x = (src_x - cx) * s + 15.5
dst_y = (src_y - cy) * s + 15.5
```

`G` is filled by area-averaging: each destination cell's value is the
ink-weighted fraction of its source preimage, giving values in `[0, 1]`.
Anything outside the bitmap is background. When `s > 1` the preimage is smaller
than a source pixel and the filter degenerates to that pixel's value, which is
blocky for very small glyphs and is accepted — it is deterministic, which is
what matters here.

Aspect ratio is deliberately destroyed by this scaling and reintroduced as an
explicit feature, so that shape and proportion are separately weighted rather
than entangled.

**Dimension layout.** Fixed, and the class id of a dimension never changes.

| Range | Group | Computation |
|---|---|---|
| `0..16` | Zone ink density | Mean of `G` over each 8x8 zone. Index `zr*4 + zc`, row-major. |
| `16..80` | Gradient orientation | Sobel on `G` with zero padding at the border. Fold to `[0, pi)` by negating both components when `gy < 0` — edges are undirected — then bin by comparison alone: `0` if `gx > 0 && gy < gx`; `1` if `gx > 0 && gy >= gx`; `2` if `gx <= 0 && gy >= -gx`; `3` otherwise. Magnitude is `sqrt(gx*gx + gy*gy)`. Accumulate magnitude into the containing zone's bin. Divide all 64 by the total magnitude over the grid, so the group sums to 1. Index `16 + (zr*4 + zc)*4 + bin`. |
| `80..88` | Horizontal projection | Row sums of `G`, pooled into 8 bins of 4 rows, top to bottom, divided by the total. |
| `88..96` | Vertical projection | Column sums of `G`, pooled into 8 bins of 4 columns, left to right, divided by the total. |
| `96` | Hole count | Background components not touching the bitmap border, foreground 8-connected and background 4-connected, clamped to `0..=2`. |
| `97..103` | Crossing counts | `G` thresholded at 0.5. Count 0-to-1 transitions along rows 8, 16, 24 scanning left to right, then columns 8, 16, 24 scanning top to bottom. Clamp each to 6 and divide by 6. |
| `103..107` | Baseline-relative geometry | `(width - height) / (width + height)`; ink fraction `ink_pixels / (width*height)`; `baseline_dy / x_height`; `(height - baseline_dy) / x_height`. The last two are clamped to `[-1, 4]`. |

Two of those choices are load-bearing and would be easy to get wrong.

**The hole count is computed on the original bitmap, not on `G`.** Resampling a
small glyph closes its counters — the bowl of an `e` at eight pixels tall
becomes a blob — and a hole count that silently depends on point size destroys
the single most valuable feature in the set, the one that partitions the charset
into three near-disjoint groups before any distance is computed.

**Aspect is stored as `(w - h) / (w + h)`, not as `w / h`.** A plain ratio puts
`0.5` and `2.0` at wildly different distances from `1.0` under an L2 metric even
though they are the same distortion in opposite directions. A logarithm would
fix that and was the obvious choice, but it is a transcendental — see below.
This form is bounded in `(-1, 1)`, negates under inversion, is monotonic in
`w / h`, and is three arithmetic operations.

**Its denominator is the glyph's own ink height, and `charset`'s band uses a
different one.** The extractor is handed a bitmap cropped tight to the ink, so
`height` here is how tall *that mark* is; `charset.tsv`'s `aspect_min` and
`aspect_max` are width over the face's **cap height**, which is the form a
drafter can read off a drawing. The two coincide only for glyphs whose ink
spans the cap line, and differ by the full cap-to-stroke factor for a flat mark
like `-` or `_`. They are not interchangeable and neither converts into the
other without knowing the glyph's ink height. See section 11's 2026-09-22
entry.

**Output is raw.** The extractor emits `[f32; 107]` with no standardisation.
The `feature_norm` table is computed over the finished bank in chunk 3 and
applied at match time in chunk 5, because a per-dimension mean and standard
deviation cannot exist before the population they describe.

**Determinism, and the constraint it imposes.** All accumulation is `f64`,
narrowed to `f32` only on output. Summation order is fixed and row-major. The
extractor is single-threaded and never iterates a hash container.

**No transcendental function appears anywhere in the extractor**, and that is
the reason for two of the choices above. IEEE 754 requires `sqrt` to be
correctly rounded, so it is identical on every target. It requires nothing of
`atan2`, `hypot`, `ln` or `exp`, and Rust's `wasm32-unknown-unknown` target
supplies its own implementations of them rather than the host's — so a feature
vector built with `atan2` on this machine could differ in the last bit from the
same vector computed in the browser. One bit is enough to fail an exact fixture
comparison, and loosening the comparison to a tolerance would forfeit the
strongest property the golden-fixture contract has. So orientation is binned by
comparison rather than by angle, aspect avoids the logarithm, and magnitude uses
`sqrt` rather than `hypot`.

Fixture comparison is therefore **exact**, with no tolerance anywhere, on both
x86 and wasm32, as section 8.2 requires.

---

## 4. The prototype bank

For each character class, for each face, **at each of several sizes**, render
the glyph, extract the feature vector, store it.

The size list is not a detail. A bank built at one canonical size was measured
at 84.22% 1-NN against the same faces rendered at other sizes; the same bank
built at four sizes measured 91.40%. See section 11's 2026-09-21 multi-scale
entry for the method and the reason.

187 classes x 19 faces x 4 sizes gives **14,088 prototypes**, measured from an
actual build on this machine, which takes about 40 seconds. (It was 20 faces
and 14,832 until section 11's 2026-09-22 duplicate-face entry; the face count
is whatever `model/fonts.tsv` marks `shippable`, plus the authored technical
face, and is not a target.) The bank is the
model's bulk and its construction is a deterministic script: render, extract,
standardise, quantise to int8 with a per-dimension scale, write. That count and
that script describe the base segment; see section 11's 2026-09-21 segmentation
entry for the optional segments built on the end user's machine.

**Font coverage is the accuracy lever.** Prototypes must span the shapes the
engine will meet: humanist and geometric sans, old-style and transitional
serif, slab, monospace, and the ISO-drawing faces for the CAD case. A bank built
from twenty well-chosen families generalises to unseen fonts far better than one
built from a hundred similar ones, because what matters is coverage of the
*shape space*, not headcount.

### 4.1 Matching, and making it fast enough

Exhaustive nearest-neighbour is **17,610 × 107 multiplies per hypothesis** on
the bank as measured 2026-09-22 — 19 faces at five scales; this figure was
written as ~12,000 × 107 when it was a projection — and a page with a lattice
over 3,000 characters would spend billions of operations in scalar safe Rust.
So matching is coarse-to-fine:

1. **Prune by hole count.** Exact integer match. Measured cost: 2 characters
   in 14,832, or 0.02 points of accuracy — effectively free, as claimed.
   Measured on the 20-face bank that section 11's 2026-09-22 duplicate-face
   entry replaced; not re-measured on the 19-face bank of 14,088.
2. **Prune by aspect band and baseline class.** *Not enabled.* Both forms of
   this step were measured and both lose accuracy; see section 11's
   2026-09-21 pruning entry. The step stays in the design as the place a
   working band would go, and the `aspect_source` column in `charset.tsv` is
   what will say when one exists. A band that goes here has to be authored in
   the extractor's own quantity — `charset.tsv`'s is width over cap height and
   does not convert; see section 11's 2026-09-22 entry.
3. **Full weighted-L2 distance** against what survives. The weights live in
   `model/feature_weights.tsv` and compile into the optional `feature_weights`
   table of section 2. An all-`1.0` file writes no table at all, so the
   table's presence in a file is itself the statement that the matcher has an
   opinion about which dimensions carry the evidence. One weight is currently
   not `1.0`: the four baseline-relative geometry dimensions carry **6.0**,
   measured — see section 11's 2026-09-22 tuning entry for the sweep, the
   margin on a disjoint sample, and the per-confusion-pair table the rule
   below requires. A weight is a number that has to be justified by a
   sentence (`CLAUDE.md` rule 1) rather than fitted, and the note column of
   `feature_weights.tsv` is where that sentence lives.

**A weight is authored per section 3.1 block, with the four baseline-relative
dimensions nameable individually so they can override their block, and weights
are not selected against aggregate character accuracy.** Both halves of that
were measured rather than assumed; see section 11's 2026-09-21 weighting entry
and its 2026-09-22 follow-up. A single
multiplier on the baseline-relative group buys most of the case errors and
pays for the last of them by making `0` against `O` substantially worse, and
a sweep scored on total error count will take that trade every time. It is the
wrong trade here: rule 6 suppresses the lexicon inside identifier-shaped
context, so a case error inside a word is one the decoder repairs for free
while `0` against `O` inside a part number is one it is forbidden to touch.
**The matcher is tuned against the errors the decoder cannot repair, not
against the error count.** Any candidate weight set is therefore reported with
its per-confusion-pair effect beside its aggregate, and a set that improves
the aggregate while worsening an identifier-critical pair is a regression.

Projected: comfortably under a second per page single-threaded, which is
*faster* than the neural design it replaces rather than slower.

**The second half of that projection is not supported by anything measured,
and what has been observed points the other way.** Ten head-to-head runs
between 2026-09-21 and 2026-09-22 recorded wall clock for both engines on the
same corpora. OCRcer is faster in four, slower in five and level in one, and
the split tracks bank size: the fast runs are the small and base banks, while
at the current 17,610-prototype five-scale bank it is **277.2 s against
`ocrs`'s 250.8 s over 625 pages**. It is slower there *while skipping
segmentation and layout entirely* — work `ocrs` does on every page and chunks
2 and 6 will add to OCRcer's side.

None of those ten is a controlled timing measurement: the runs are sequential
on a machine carrying varying unrelated load, and the harness's wall clock
includes page rendering and scoring alongside recognition. So the honest
position is that **the per-page figure holds comfortably** — every run lands
between 0.26 and 0.49 s/page — **and the comparative claim is unverified with
the available evidence against it.** A clean timing measurement is an open
item for `ocrcer-bench`, and until it exists the sentence above should not be
quoted. This section's opening line is the reason to expect the cost to have
grown: matching is linear in bank size, and the bank came in 47% above the
~12,000 this claim was written against.

### 4.2 Confidence, which is the capability `ocrs` lacks entirely

Let `d1` be the distance to the best-matching prototype and `d2` the distance to
the best prototype of a *different* class. Confidence derives from the ratio
`d1/d2`, mapped through an authored calibration curve.

This is a better-behaved signal than a neural posterior. It is explicitly a
statement about *margin* — how much better the winner was than its nearest
rival — which is exactly the quantity a reviewer needs. A character matched
well by two competing classes reports low confidence even when its absolute
match was good, which is the case a softmax tends to hide.

- **Character confidence**: calibrated margin, adjusted by the decoder's
  language-model agreement.
- **Word confidence**: geometric mean over characters. Geometric because the
  quantity is a product of independent probabilities; an arithmetic mean lets
  one confident character mask a hopeless one, which is the case that must be
  surfaced.
- **Line confidence**: geometric mean over words, weighted by character count.

`reports_confidence()` returns **true**, and it means it.

---

## 5. The lattice decoder

The segmenter emits, for each word, a directed acyclic graph whose edges are
candidate character images: the plain single-component cuts, plus merge
candidates for broken glyphs and multi-part glyphs (`i`, `j`, `=`, `:`, `%`,
accented letters), plus split candidates at vertical-projection minima inside
wide components.

Each edge carries the top-k class hypotheses from the matcher with their
distances. Viterbi finds the maximum-scoring path:

```
score = w_match * match_score
      + w_bigram * bigram_logprob(prev_char, this_char)
      + w_lex * lexicon_bonus(partial word)
      + w_seg * segmentation_prior(edge width vs line x-height)
      + w_confusion * confusion_adjust(context priors for look-alike pairs)
      - case_shape_penalty  (once per anomalous case transition)
```

The weights are authored parameters and are the main tuning surface in the
benchmark chunk.

**The case-shape term is not a weight but a penalty, and that asymmetry is
deliberate.** A printed alphabetic word takes one of exactly three shapes —
`lower`, `Title`, `UPPER` — and a reading like `payMents` or `AMOunt` takes
none of them. That is the one statement about orthography a character bigram
structurally cannot make, because it is a claim about a whole letter run
rather than about a pair, and it is also the one the lexicon cannot make,
because the lexicon is case-folded through the charset's `case_twin` column by
design. The penalty is charged **incrementally as the path is extended**, not
at the word end: a word-final term cannot steer a beam that has already
dropped the correct reading. The run resets at every non-letter, so `E&OE`,
`PART NO.` and `M8x1.25` cost nothing, and the term is suppressed entirely
inside an identifier-shaped word, where an orthographic prior is most likely
to be confidently wrong. The admitted cost is that `McDonald`, `iPhone` and
`PhD` are charged; the penalty is an overturnable prior, not a filter, so a
sufficiently better match still wins.

**Why this is where the accuracy comes from.** The matcher alone on clean print
gets most characters right and makes its errors in a small number of
systematically confusable families. The bigram and lexicon terms fix precisely
those, because the confusions are not linguistically plausible: `rnodern` is not
a word and `modern` is, `cl0se` is not and `close` is. This is the mechanism by
which classical OCR reached accuracies a bare character classifier cannot.

The lexicon is a *bonus*, never a constraint. Part numbers, dimensions and
identifiers must survive unmolested, so out-of-lexicon strings are scored
without the bonus rather than penalised, and a numeric/identifier context
detector suppresses the lexicon term entirely inside things that look like part
codes. An OCR engine that silently corrects `M8x1.25` into a dictionary word is
worse than useless in this domain.

---

## 6. Segmentation detail

**Binarization** is Sauvola with window 25 and `k = 0.34`, not Otsu. Otsu is a
global threshold and fails on the uneven illumination of real scans; Sauvola is
local and costs one integral-image pass. For digital-born pages either works,
and the cost of Sauvola is negligible.

**Skew** is estimated by maximising the variance of the horizontal projection
profile over a coarse-to-fine sweep of +/-5 degrees. Text lines produce sharp
profile peaks only when horizontal.

**Component filtering** drops what is not a character before lines are formed,
by two rules that catch different things. `lines.furniture_fraction` rejects a
component wider or taller than a fifth of the page: a page border, a table's
outer frame, a signature rule. That bound is measured against the whole page,
so it cannot see a rule drawn inside one cell of a form, which is small in
absolute terms and survives to be matched against the charset, where it lands
on a dash. `lines.rule_aspect` was written as the scale-free companion — a
component whose longer ink extent exceeds N times its shorter one is a rule,
not a glyph — and **it ships disabled at 0, because every value tried measured
worse.** See section 11, 2026-09-22.

The bound that value has to clear is measured rather than assumed.
`ocrcer-build aspect` renders every charset class on every shippable face at
256 px/em and reports the extreme ratio of longer ink extent to shorter, taken
symmetrically so the same number governs a vertical cell rule as a horizontal
one. The flattest class in the charset is the em dash on Open Sans Condensed
Light at 236x10 px, 23.60:1; the extreme in the other orientation is the bar
on the same face at 12x256 px, 21.33:1. The parameter sits
above that measured extreme, so the bound is not what rejected the gate. Zero
disables it, which is both how it is isolated in a paired measurement and what
it currently carries.

**Line grouping** uses component vertical overlap against a running median
component height, rather than a fixed threshold, so it survives mixed type
sizes on one page.

Vertical overlap alone is not enough on a boxed form, where four boxes side by
side genuinely do share a row of the page while carrying different type sizes.
Everything downstream then computes one x-height for the mixture, which is
wrong in both directions at once: the large text over-splits because the
word-gap threshold scaled by that x-height is too small for it, and the small
text under-splits because the same threshold is too large. So each band is cut
again at any horizontal gap wider than `lines.column_gap_heights` times the
band's own width-weighted median component height. The populations separate,
measured over 384 uncut lines of `bench/pages-cov` in that unit and classified
by the word splitter's own per-line threshold. A gap it calls intra-word sits
at 0.14 and reaches 0.30 at the ninetieth percentile. A gap it calls a word
space sits at 0.57, reaches 1.25 at the ninetieth and tops out at 1.64 on
running text. The widest gap inside one banded row is 3.25 on an invoice and
5.30 on a statement, with a tail past 20. So a cut at 2.5 cannot fire on
running prose at all — six of 1072 word spaces on the five families without
banded rows reach it, every one of them in an engineering drawing — and fires
on about two thirds of invoice and statement lines. Those are rendered pages,
not scans, and a scan's gaps carry the additional spread of its own
binarisation. Setting the parameter to zero disables the cut, which is how the
stage is isolated in a paired measurement.

**No cut between two lone glyphs.** Every gap wider than the cut is a
candidate; a candidate is dropped when the fragments on both sides of it, as
formed by all candidates together, each hold exactly one component. The band's
scale is a median of its own heights, so on a band that is mostly leader dots
the scale is the height of a period and every inter-dot gap clears the cut;
without this rule such a band dices into one line per dot. A single glyph
beside a multi-glyph fragment is still cut off: a lone `$` in its own table
cell is a column. No parameter.

This is a line-layout rule and not table reconstruction: it stops one row of
unrelated boxes from sharing one set of line metrics, and says nothing about
which caption belongs to which value. Structure remains chunk 9's.

**Baseline and x-height per line** come from histograms of component bottom and
top edges. These feed the four baseline-relative features in section 3 and the
segmentation prior in section 5. Two rules qualify that, both of them about
lines whose punctuation outnumbers their text:

- The histograms are **weighted by component width**, not by component count,
  so they answer which population owns the line's horizontal extent rather
  than which has the most pieces. A dot-leader run outnumbers the words beside
  it and is outweighed by them.
- A line whose x-height still comes out below `lines.inherit_x_height_below`
  of the page's typical x-height **inherits the page's**, and is labelled
  `Inherited` rather than measured. Only lines that observed a distinct
  x-height band vote for the page value, so a page of leaders cannot bootstrap
  a wrong answer from itself, and the baseline is never inherited — a row of
  full stops sits on its baseline and measured it correctly.

**Word splitting** uses bimodal analysis of inter-component gaps within a line:
intra-word and inter-word gaps separate cleanly in print, and the threshold is
placed at the valley rather than at a fixed multiple of x-height.

That valley is bounded on both sides, and the two bounds are the same claim
seen from opposite ends. `words.lone_gap_x_heights` is the ceiling: no valley
may sit where a gap that *wide* is called intra-word.
`words.min_valley_x_heights` is the floor: no valley may call a gap that
*narrow* a word space. The floor applies to the narrowest gap the valley would
call a space, not to the threshold, because the threshold is the widest
intra-word gap and is small on any well-set line.

The floor exists because a valley search given only intra-word gaps still finds
a valley among them, and Otsu separability on gaps of 1, 1, 3, 1 and 2 px is
high enough to believe it — so a line that is a single word splits into its
letters. Nothing bound that while lines ran the width of the page, since any
line long enough to hold several words holds their spaces too. It appears the
moment `lines.column_gap_heights` cuts a line into segments that are each one
word, which is the ordinary case on an invoice or a form. A valley below the
floor is not believed and the line falls back to `words.no_valley_x_heights`,
which is the honest reading of a segment whose gaps contain no word space at
all. The fallback is the ceiling on the floor rather than a target for it: at
equality the rule reads that a valley is believed only if it calls a space the
no-information rule would also call a space, and above equality it contradicts
itself — rejecting a valley for calling a gap a space, then falling back to a
rule that calls the same gap a space. The swept value sits below it.

The floor converts to an integer pixel count by truncation, so it is a
staircase and is coarse at the x-heights this engine actually sees: on a 9 px
x-height every value from 0.23 to 0.33 is a 2 px floor. It is an aggregate
improvement and not a guarantee about any one line — at the shipped value a
three-character amount cell whose only wide gap is 3 px is still split.

---

## 7. The `.ocrw` model format

Ours, documented, versioned, MIT. Little-endian throughout. The runtime must
have **zero dependencies** to stay pure safe Rust and pass the wasm32 gate, and
a format this simple is a ~200-line parser.

```
Header
  magic        [u8; 4]   "OCRW"
  version      u16       1
  model_kind   u16       1 = document recogniser
  n_tables     u32
  meta_len     u32
  meta         [u8]      UTF-8 JSON
  blob_crc32   u32       CRC-32 of the table blob
  reserved     [u8; 8]   zero

Table directory, n_tables entries
  name_len     u16
  name         [u8]      "prototypes", "faces", "lexicon", "bigrams", ...
  kind         u8        0 = f32, 1 = i8 matrix, 2 = opaque bytes
  ndim         u8
  dims         [u32; ndim]
  n_scales     u32       per-dimension scales for int8 matrices
  scale_off    u64
  data_off     u64       64-byte aligned
  data_len     u64

Blob
  concatenated table data
```

`meta` carries the charset, the feature-extractor version, the normalisation
constants and the construction-run identifier. **The charset lives in the model
file, not in the Rust source**, so a model file and a runtime can never disagree
about what class 137 means.

Int8 is a storage format only. Tables are dequantised to f32 once at load, so no
kernel ever sees an integer, and the only place quantisation error can appear is
the one measurement that reports it: top-1 agreement before and after.

**What guards meaning, and what does not.** Two forward-compatibility rules,
and they are deliberately asymmetric.

- **A `version` the reader does not know is refused.** `version` means *the
  tables you already know have changed meaning* — a charset insertion, a
  feature-dimension reordering, new normalisation constants. There is no safe
  partial read of such a file, so there is no attempt at one. `meta`'s
  feature-extractor identifier is the same guard narrowed to the vector
  definition, and a mismatch there is refused for the same reason.
- **A table name the reader does not know is skipped, silently and without
  error.** A table the reader cannot name is by construction one it does not
  consume, so skipping it cannot change an answer. This is what makes an
  *additive* table cheap: a new diagnostic or auxiliary table ships to old
  runtimes as dead bytes rather than as a load failure, and costs no `version`
  bump. Redefining an existing table is not additive and does need one.
- **A parameter *row* whose name the reader does not know is refused, and the
  whole `params` block with it.** This is not the previous bullet applied one
  level down, and the difference is the point. An unknown *table* degrades to
  *no argument*: the reader falls back to behaviour nobody authored otherwise.
  An unknown *parameter row* degrades to *a different argument*: the row exists
  precisely because a default is being overridden, so dropping it silently
  substitutes the default for the authored value and the engine reads the page
  by a rule the model file does not describe. Forward compatibility loses
  nothing real here — a row a reader cannot name is one it has no slot to
  honour — whereas the silent path produces plausible numbers that are wrong,
  which is the failure this format exists to make impossible.

The distinction is the whole reason the format has both fields. Collapsing it
— refusing on an unknown table name — would force a `version` bump for every
addition and make the bump stop meaning anything, which is precisely how a
version field becomes decoration.

### 7.1 Segments and the face manifest

Section 11's 2026-09-21 segmentation entry decided that the bank segments by
*distribution* rather than by inclusion. This is what that costs the format.

A **face manifest**, one record per source face, generated from
`model/fonts.tsv` at bank-construction time and never hand-maintained — a
second hand-kept list would drift from the first, and the drift would be
invisible.

It lives in **`meta`, as `meta.faces`**, not as a table. This section
originally specified a table; the writer put it in `meta` and the writer is
right, for the same reason the charset is there: a table is typed numeric or
opaque bytes, and a manifest is mostly variable-length strings that would need
their own encoding inside an opaque blob to live in one. Nineteen JSON records
sit inside a `meta` block that is 11,169 B in total and is parsed at load
anyway.

Each record must carry **family, style, distribution, licence, and licence
source**. The emitted file today carries the first three; the last two are
specified here and not yet written, audited 2026-09-22 — see section 11.
`distribution` is valued `shippable` or `local-only`. The licence pair is
there because **the model file travels without this repository.** It ships
into a consuming application, and the entire licence case for this project is
that the tables are an original work derived only from unambiguously
permissive faces. An auditor holding only the `.ocrw` should be able to answer
*what were these prototypes derived from, and under what licence* from the
file itself, rather than being told to go and find a TSV.

**Segment membership is derived, not stored**: the base segment is every
prototype whose face is `shippable`. There is no segment field to get out of
step with the manifest, because there is no segment field.

The derivation happens at bank-construction time, in the face selection, and
the emitted rows carry class only. An earlier wording here said every
prototype row carries its face identifier; **that is not what the writer
emits, audited 2026-09-22.** Segmentation does not need it — an optional
segment is a whole separate file, so every row in a file is already known to
belong to that file's segment. What a per-row face tag would buy is
diagnostic rather than structural, and section 11's 2026-09-22 format-audit
entry records the condition under which it gets added.

An **optional segment is a whole `.ocrw` file**, not a region inside one. It
declares `model_kind = 2` (supplementary prototypes), and its `meta` must
match the base file's feature-extractor version, charset digest and
normalisation constants exactly. A supplementary file that disagrees on any of
the three is refused at load rather than merged, for the reason section 7
exists at all: a mismatched pair does not fail, it answers wrongly.

**Two load-time rules, and they are not symmetrical.**

- **A missing optional file is a normal condition.** Reduced coverage, surfaced
  through the confidence machinery per rule 5, never a load failure. The
  loader records which files were present so a report can say what the engine
  was working with.
- **A class with zero base-segment prototypes is a build-time error.** Not a
  warning. A class carried only by a removable segment is a character the
  engine goes blind to when that segment is absent, which is a silent
  wrong-answer mode rather than a degradation. The builder counts per class
  and refuses to emit. (`⌀`, carried by only two eligible faces, is the live
  instance of this hazard.)

---

## 8. The Rust workspace

Everything is Rust. There is no second implementation of any pipeline stage in
any other language, and that is a correctness property rather than a
convenience: **the feature extractor runs twice in this system** — once to
build the prototype bank, once to recognise a glyph — and if those were two
implementations they would have to agree exactly, forever, with no way to
notice when they stopped. Every prototype in the bank would be measured with a
different ruler than the runtime uses, accuracy would collapse, and nothing in
the output would say why. One extractor, called from both places, makes that
failure unconstructible.

Three crates:

| Crate | Ships | Dependencies | What it is |
|---|---|---|---|
| `ocrcer-core` | yes, into pdfcer | **none** | The engine |
| `ocrcer-build` | no | free | Renders glyphs, builds the bank, compiles the authored tables, writes `.ocrw` |
| `ocrcer-bench` | no | free | Evaluation harness, fixture blessing, head-to-head runs |

The zero-dependency rule binds `ocrcer-core` alone. The other two never reach a
user, so they may use a font rasteriser and an image decoder freely — both are
available as pure-Rust, permissively licensed crates, and neither ends up in
pdfcer's dependency tree or its licence manifest.

**`ocrcer-build` links `ocrcer-core` and calls its extractor.** That is the
whole point of the split. The builder owns rasterisation, which needs a
dependency; the moment it has a bitmap, it hands it to the same
`ocrcer_core::feature::extract` the runtime will call.

### `ocrcer-core`

- `#![forbid(unsafe_code)]`
- **No dependencies.** `core`, `alloc`, `std` only.
- Compiles for `wasm32-unknown-unknown` with no feature work.
- Optional `parallel` feature adding rayon, absent from the wasm build.

```
crates/ocrcer-core/src/
  ocrw.rs              format parser, dequantisation
  image/binarize.rs    Sauvola, integral images
  image/deskew.rs
  image/components.rs  two-pass union-find labelling, Euler number
  layout/lines.rs      line grouping, baseline and x-height
  layout/words.rs      gap analysis
  layout/segment.rs    lattice construction
  feature.rs           normalisation and the 107-dim extractor
  match.rs             pruning and weighted-L2 matching
  decode/viterbi.rs    lattice decode with bigram and lexicon
  decode/lexicon.rs    DAWG traversal
  confidence.rs
  pipeline.rs          Engine
  lib.rs
```

### 8.1 Public API

```rust
pub struct Engine { /* ... */ }

impl Engine {
    pub fn from_bytes(model: &[u8]) -> Result<Self, Error>;
    pub fn recognize(&self, img: Gray<'_>) -> Result<Vec<Word>, Error>;
    pub fn recognize_lines(&self, img: Gray<'_>) -> Result<Vec<Line>, Error>;
}

pub struct Line {
    pub words: Vec<Word>,
    pub rect: Rect,
    pub baseline: f32,     // image pixels, y-down; measured, not the box bottom
    pub x_height: f32,     // pixels; may be the page's, per section 4
    pub confidence: f32,   // geometric mean over words, weighted by characters
}

pub struct Word {
    pub text: String,
    pub rect: Rect,        // image pixels, y-down
    pub confidence: f32,   // 0.0 ..= 1.0, geometric mean over chars
    pub chars: Vec<CharBox>,
}
```

`recognize` is the flat word list pdfcer's trait wants. `recognize_lines`
adds the grouping and the two per-line metrics the engine already measured on
the way there. They are exposed rather than kept private because a consumer
writing an interchange format needs them and would otherwise invent them: an
hOCR importer that drops any line without a `baseline` reads a whole page as
empty, and the repair for that must be the measured baseline, not a plausible
one.

`from_bytes` rather than `from_files` is the primary constructor because pdfcer
forbids the engine from deciding where models live — `pdfcer-core::ocr::models`
owns that, and it never downloads.

Mapping onto pdfcer's trait is direct:

```rust
impl OcrEngine for OcrcerEngine {
    fn recognize(&self, w: u32, h: u32, px: &[u8]) -> Result<Vec<RecognizedWord>, Self::Error>;
    fn reports_confidence(&self) -> bool { true }
}
```

### 8.2 The correctness contract

With one implementation there is nothing to compare against, so regressions are
caught by **golden fixtures**: checked-in inputs with checked-in expected
outputs, asserted at every stage boundary rather than only at the end.

```
fixtures/
  glyphs/         one glyph bitmap plus its declared line metric
  decode/         one authored word lattice, with its candidate distances
  pages/          rendered pages, and scans, whose text is known exactly
  expected/       one JSON per fixture, per stage:
                    binarized image hash
                    component count and bounding boxes
                    lines, baselines, x-heights
                    word boxes
                    per-glyph feature vectors
                    top-1 class and margin per glyph
                    the decoder's chosen reading and its path score
                    final strings with confidences
```

Two of those exist: `glyphs` (feature extraction) and `decode`. `pages` is
chunk 2's and does not.

**So the stages between them carry no golden coverage today** — binarization,
component finding, line grouping, word splitting and segmentation are all
page-level, and the page-level fixtures are the ones that do not exist. The
consequence has to be stated because it is easy to misread in the other
direction: `bless` reporting *0 fixtures would change* after a change to one
of those stages is **weak evidence, not strong**. It means the change did not
reach feature extraction or the decoder, which is the sentence to write down,
rather than that the change was harmless. Until `fixtures/pages` lands, a
tuning result on one of those stages is evidenced by its corpus measurement
and by the per-domain check in section 4.1 — never by a quiet bless.

**The generated corpus has its own contract, and it is not a fixture.** A
fixture asserts that a stage still does what it did; the bench corpus asserts
that the *reference* is credible in the first place, which is the failure
section 11's 2026-09-22 truth-box entry found the hard way. Two checks run
inside the generator, and a page that fails either is refused rather than
written: no alphanumeric truth glyph may have a box two pixels tall or less,
and a character whose outline is a single closed contour must be **drawn** as
a single 8-connected component -- counted on coverage, before any threshold,
because the invariant is about the rasteriser and not about the binarizer, and
one-directional because `O` is two contours and one component. A rejection
names the page. The generator's floor is 13 px/em: below that the height guard
refuses Open Sans Condensed Light outright.

Glyphs the *binarizer* breaks into several marks are a different thing --
difficulty a real scan has -- and are counted and printed per run rather than
refused. At 13-16 px/em that is about 5% of glyphs, which is worth knowing
before a per-size figure is read as a matcher result.

These belong in the generator and not in a script, for the same reason a test
must not rewrite its own expectation: a corpus regenerated without the checks
is a corpus nobody re-checks. Dropping the offending cells would be worse than
refusing the page, because what it silently removes is the hardest faces at
the smallest sizes -- the stratum every per-size figure is read off.

**The generated corpus tests recognition and does not test rejection, and
that is now a named gap.** Its generator draws text and nothing else, so every
connected component on every one of its pages *is* a character. A classifier
that cannot decline is therefore never asked to, and the corpus reports a
precision it has not measured: the same engine scores 77.4% word precision
there and **15.1% on 29 real scans** (section 9), because a real page carries
chart axes, table cell rules, logo fragments, signature strokes and scanner
speckle, and a 1-NN matcher turns each of them into a character.

The instrument this needs is a **separate** corpus family that draws non-text
marks — rules, boxes, tick marks, leader runs, filled swatches — with truth
boxes for the text only. Separate, not a change to the existing families,
because every figure in section 9 and section 11 is measured on those pages
and a generator change silently reprices all of them. Then a component that
overlaps no truth box is a labelled negative, which is what makes a reject
threshold measurable rather than guessable: the two nearest-prototype distance
distributions can be reported side by side and the floor placed where they
separate. `ocr --distances` reports the positive half today, from truth boxes,
and has no negative half to compare it against.

Until that family exists, **no reject threshold may be authored.** A
plausible-looking distance floor deletes real characters on degraded input,
which is a worse failure than the over-emission it would be fixing, and rule 1
forbids the number either way.

**A decode fixture's lattice is authored, not captured.** A lattice recorded
off a real page would make the fixture a record of what the matcher returned
on the day it was recorded, and every bank rebuild would demand a re-bless of
a file nobody could check by reading — precisely the "regenerated rather than
understood" failure this section exists to prevent. An authored lattice has
ground truth in the sense meant above: the distances state plainly which
reading the image favoured, so the right answer is derivable from the fixture
itself. For the same reason the decoder is run there with no bigrams, no
lexicon and no confusions, and the class indices are the fixture's own sorted
character set rather than the shipped charset's — a decode fixture must not
change meaning when a class is inserted into `charset.tsv` or a table is
rebuilt.

This stage needs fixtures more than the aggregate metrics suggest, because the
determinism guarantees below are invisible in an error rate: a tie broken by
the wrong rule is right on about half of its occurrences and wrong on the
rest, which moves no CER figure far enough to notice.

**Why this is not merely "it has not changed".** A golden file on arbitrary
input proves only self-consistency. These fixtures are generated from pages
whose ground truth is known — rendered from text we wrote, or scans we have
transcribed — so the expected output is checkable against the answer rather
than against a previous run. A fixture that is wrong can be found by reading
it.

**Blessing is explicit and reviewed.** Regenerating expectations is
`cargo run -p ocrcer-bench --bin bless`, run deliberately, and the resulting
diff goes through review like any other change. No test rewrites its own
expectation, because a test that does catches nothing. A one-pixel change in
binarization that shifts four hundred feature vectors must be visible as four
hundred changed lines, not absorbed silently.

**Determinism is a requirement, not an accident**, because the same fixtures
must pass on x86 and on wasm32. Lattice nodes are visited in a fixed order; an
exact tie breaks by lowest class index, then earliest segmentation cut; no
stage may resolve anything by iteration order over a hash map. The decoder
accumulates in f64 even though features and prototypes are f32 — a few hundred
thousand operations per page, so the wider accumulator costs nothing and keeps
near-ties from resolving differently on different targets.

**What this contract does not do**, stated plainly: it does not prove the
algorithm is right. Neither did any alternative — comparing two
implementations proves a translation was faithful, not that what was
translated was correct. Correctness is established in section 9's terms, by
measuring against documents whose text is known, and that is unaffected by
how many implementations exist.

---

## 9. Honest accuracy expectation

**Every corpus figure in this section dated before 2026-09-22's truth-box
entry was measured against ground truth that was wrong in one direction.** The
bench renderer's strict-majority fill recorded 338 alphanumeric truth boxes of
two pixels or less across 625 pages, none of them above 21 px/em and 1.08% of
glyphs at 12px, so a reference box a correct read could not match charged the
engine for reading the page right. Those figures are **lower bounds on the
engine, most understated at the smallest sizes**, and none of them is restated
as improved until it has been re-measured on `bench/pages-cov`. The `ocrs`
column carries the same bias, in the same direction, for the same reason.

Every figure in the list below is a **projection**, still to be replaced by
measurement in the benchmark chunk. Partial measurements now exist, and what
they measure is the *matcher alone*: an OCRcer handed oracle segmentation and
oracle spacing, with no decoder, no lexicon and no confusion table. The best
of them, over 625 pages at five render sizes present in neither the tuning
corpus nor any prototype scale, is 95.73% character accuracy and 83.15%
layout-free word F1 against `ocrs`'s 77.72% and 82.72% — see section 11's
2026-09-21 held-out entry, and read its caveats before quoting the number.
That entry reported 95.88% and 83.69% over 660 pages; section 11's 2026-09-22
duplicate-face entry is why the corpus is smaller and the figure lower.

Three things that reading is not. It is not end to end: the segmentation and
layout stages it was given for free are chunks 2 and 6, and they are stubs. It
is not a page: the corpus is synthetic, hard black and white, unskewed and
noise-free, which is the regime this design is strongest in. And it is not
stable across resolution — section 11's size-sweep entries show the same bank
swinging between 48% and 99% word F1 depending on render size, so any single
aggregate is a weighted average over whatever size mix the corpus happened to
carry. Read the projections below as what the *finished* engine is aimed at.

**The first third-party measurement on real scans is a loss, and a large
one.** Everything above is OCRcer scored by OCRcer, on truth this project
converted, by a metric this project chose. On 2026-09-22 the engine was
exported as hOCR and scored by the `scribeocr/ocr-benchmark` harness instead:
29 hand-checked pages, their truth, their metric, their code. The harness was
validated first by reproducing the figures it publishes for the two engines
nobody here tuned, and it did, exactly: Scribe.js 93.65% and Tesseract.js
(LSTM) 84.76%. Against those, **OCRcer scores 43.40%** with the column cut at
1.5, and 33.81% with the cut disabled.

Their statistic is the unweighted mean over pages of `correct / total` over
*ground-truth* words matched by bounding-box overlap, punctuation ignored:
a box-matched word **recall**, with no insertion term. So the number above
understates nothing about spurious output and says nothing about it either.
This crate's own metric on the same 29 pages, same settings, gives the other
half: recall 44.809%, **precision 15.134%**, word F1 22.626%, CER 109.765%.

Precision at 15% with recall at 45% means the engine emits roughly three
tokens for every one the truth has. That is not a matcher failure and it is
not fixed by better prototypes: **the pipeline has no reject anywhere.** Every
component that survives the geometric filters in section 6 is matched to its
nearest class and emitted, so a chart's tick marks, a table's cell rules and a
logo's fragments all become characters with nothing to stop them. Per-family,
the split is stark — with the column cut disabled, `filing` 5.744% CER and
`singlecol` 11.895% against `slide` 453.994%, `table` 168.741% and `chart`
86.205% — which is the same statement: clean single-column text reads well,
and everything the engine cannot refuse costs it more than the text earns.

A match-distance floor, below which a component emits nothing, is the missing
stage. It is not a threshold to guess: the distance distribution of true
glyphs and of non-glyph components has to be measured on a corpus that
contains both before any value is authored, or the floor becomes exactly the
kind of plausible-looking number rule 1 forbids.

- **Digital-born printed text, 200+ DPI, fonts represented in the bank:** 98–99.5%
  character accuracy. This is the domain classical OCR genuinely mastered.
- **CAD drawing text:** strong, and the domain nothing else targets. Dimension
  callouts are short and high-contrast. The clause that used to finish this
  bullet — that they sit in a small set of faces the bank can cover
  exhaustively — is withdrawn: measured 2026-09-22, the drafting faces on this
  machine (DS ISO 1, GENISO, ISOCPEUR, ISOCTEUR, GOST) are all CAD-vendor
  proprietary, so the bank cannot hold any of them and covers this domain
  through general faces plus the authored ISO 3098 face instead. The
  projection is kept — drafting text is geometrically simple and the authored
  face is drawn to the same standard those faces implement — but it now rests
  on that, not on face coverage. See section 11, 2026-09-22.
  **This bullet now has a measurement against it and the measurement is a
  loss.** On the tuning corpus's drawing block, 2026-09-22, OCRcer reads
  96.09% of characters correctly against `ocrs`'s 95.45% — and scores **84.67%
  layout-free word F1 against `ocrs`'s 89.18%** — one of four blocks `ocrs`
  wins, and the one this bullet is about — while holding oracle segmentation.
  More characters right and fewer words right means the errors are spread
  thinner across more distinct words, which is what a
  matcher with no decoder does; chunks 5 and 6 are the answer and 84.67% is
  the number they are measured against. Until then, *strong* in this bullet is
  a projection about the finished engine and not a description of what is
  measured today.
- **Clean 300 DPI office scans:** 95–98%.
- **Degraded scans, heavy noise, sub-150 DPI, fax:** this is where a CNN wins
  and this design does not. Expect 85–93%, and expect the gap against `ocrs` to
  be widest here.
- **Unseen display or decorative faces:** degrades gracefully to the nearest
  covered shape, which is a real advantage of prototype matching over a network
  that has simply never seen the shape.

**The structural trade, stated plainly.** A fitted CNN learns noise robustness
from data; a constructed prototype bank cannot, and gets its robustness from
feature design and the language model instead. On clean print that gap is
negligible. On degraded input it is real and it is the price of a model that
required no training.

**The upgrade path is preserved.** The format, the runtime scaffolding, the
segmentation, the language model and the confidence machinery are all
independent of how characters are classified. If a fitted classifier is ever
wanted, it replaces sections 3 and 4 and nothing else.

---

## 10. Licence posture

- **Code:** MIT.
- **Model:** MIT. Constructed from glyphs rendered here and knowledge authored
  here. No corpus, no upstream licence, nothing to inherit, and no attribution
  gap of the kind `cargo-about` cannot see.
- **Fonts:** no font data ships in the model. Whether a feature vector derived
  from a font is itself redistributable is a **legal question this project has
  not had answered** — an earlier draft of this line asserted it was not
  redistribution, which was an assertion, not a verified position. The design
  routes around it instead: see section 11's 2026-09-21 segmentation entry,
  where anything derived from a `local-only` face is built on the end user's
  machine and never ships. What ships is restricted to
  unambiguously permissive families regardless — SIL OFL, Apache-2.0, or public
  domain. **The authoritative list is `model/fonts.tsv`**, where every row
  carries the source the licence was read from. This document deliberately does
  not repeat it: a font list in prose is a list that drifts, and a face named
  here would read as pre-approved without anyone having checked. Anything not
  marked eligible there is an operator question.
- **Lexicon:** authored, plus public-domain word lists. No scraped text.

**What the posture costs, stated rather than left implicit.** Measured
2026-09-22: of the twelve font families on this machine that draw `⌀`
U+2300, six are CAD-vendor drafting faces and three are Microsoft's — three
are licence-clean. The glyphs this project's target domain depends on are
disproportionately drawn by the vendors whose faces it may not read, which is
a permanent structural cost of an MIT model rather than a gap to close by
looking harder. The answer is the authored ISO 3098 face: where the shapes
cannot be read, they are drawn. See section 11, 2026-09-22.

---

## 11. Decision log

Append-only, dated. A superseded decision gets a new entry with a forward
pointer; the old entry stays.

### 2026-09-18 — Constructed model, not a fitted one

The operator's requirement is a model authored from knowledge rather than
trained. A convolutional recogniser cannot satisfy that, because its weights are
only reachable by fitting. The architecture is therefore classical prototype
matching with a lattice decoder, every parameter of which is authored or
deterministically computed. Accuracy consequences are stated in section 9 rather
than hidden.

### 2026-09-18 — Segmentation emits a lattice, not a commitment

Touching and broken characters cannot be resolved before recognition. Joint
Viterbi decoding over segmentation, classification and the language model is the
mechanism that makes classical OCR competitive on clean print, and a
single-cut pipeline forfeits it irrecoverably.

### 2026-09-18 — The lexicon is a bonus, never a constraint

Part numbers, dimensions and identifiers must survive. Out-of-lexicon strings
lose the bonus rather than taking a penalty, and the lexicon term is suppressed
entirely inside identifier-shaped context.

### 2026-09-18 — The extractor contract is specified to the arithmetic

Section 3.1 pins resampling, gradient binning, cut positions, clamps and
accumulation width, rather than leaving them to the implementer. One extractor
called from two places removes the risk of two implementations drifting; it does
not remove the risk of one implementation being rewritten differently later. A
feature definition that is only as precise as its prose is a definition that
changes silently.

### 2026-09-18 — The font list lives in `model/fonts.tsv`, never in prose

This document previously named example families inline, including `OSIFont` as
an ISO-drawing face. The chunk 1 inventory established that `OSIFont` is
GPL/LGPL with a font exception, which is not one of the three licences rule 2
permits, and that `DejaVu` is under the Bitstream Vera licence rather than the
OFL as implied. Neither had been checked; both read as approved. A licence is a
fact with a source, so it belongs in a table with a source column, and the prose
points at the table.

### 2026-09-18 — No transcendental functions in the extractor

`atan2`, `hypot` and `ln` are not required by IEEE 754 to be correctly rounded,
and the wasm32 target provides its own rather than the host's. A last-bit
difference between targets would break exact fixture comparison, and the
alternative — a tolerance — would give up the strongest property the fixture
contract has. Orientation bins by comparison, aspect uses `(w-h)/(w+h)`, and
magnitude uses `sqrt`, which IEEE does require to be exact. The cost is a
slightly less natural aspect feature; the gain is that a fixture blessed here is
valid in a browser.

### 2026-09-18 — Hole count from the source bitmap, not the normalised grid

Resampling closes counters on small glyphs, which would make the most
discriminative feature in the set depend on point size. Computing it before
normalisation costs one extra pass over a bitmap that is already in cache.

### 2026-09-18 — int8 on disk, f32 in the kernels

Storage format only, dequantised once at load. Keeps quantisation error,
accumulator widths and requantisation scales out of every kernel, so the cost of
quantisation is one number measured once rather than a property of every stage.

### 2026-09-18 — Own model format rather than ONNX or safetensors

The zero-dependency invariant is what makes the wasm32 gate free and leaves no C
toolchain or prebuilt blob to audit. A format this simple is cheaper than any
dependency satisfying the same need.

### 2026-09-18 — Deterministic tie-breaking and f64 decode accumulation

Lattice nodes are visited in a fixed order, exact ties break by lowest class
index then earliest cut, and the decoder accumulates in f64. The same golden
fixtures must pass on x86 and on wasm32, and a near-tie that resolves one way
on one target and the other way on the other would turn a correct engine into
a failing test suite with no bug to find.

### 2026-09-18 — One language, one implementation of every stage

The engine, the model builder and the benchmark harness are all Rust, in one
workspace. The decisive argument is not consistency with pdfcer, though that
holds: it is that the feature extractor runs both when building the prototype
bank and when recognising a glyph, and two implementations of it could drift
apart silently, invalidating every prototype in the bank with no symptom except
collapsed accuracy. Sharing one function removes the failure mode rather than
defending against it. The cost is the loss of an independent cross-check on the
arithmetic; that check was never evidence of correctness, only of faithful
translation, and section 8.2 replaces it with golden fixtures generated from
pages whose text is known.

### 2026-09-18 — `Ø` (U+00D8) and `⌀` (U+2300) are one shape and two classes

Both stay in the charset, both are built from the one authored `O` plus the
same slash, and no attempt is made to separate them on pixels, because the
pixels do not separate them — in most faces the two codepoints are drawn
identically by design, not by coincidence. The engine therefore reports the low
margin honestly (rule 5) rather than letting a bigram or lexicon weight invent a
winner, and disambiguation is left to context: a diameter sign precedes a
number, a letter sits inside a word.

Merging them into one class was rejected. A caller that receives `Ø` where the
page meant `⌀` can fix it; a caller that receives a class the charset does not
name cannot, and pdfcer's text layer has to emit one codepoint or the other
regardless.

A prior favouring U+00D8 is defensible on this toolchain — SolidWorks maps its
diameter token to it and AutoCAD's `%%C` decodes to it — but it is a prior, and
if it is ever applied it is labelled one in the parameter block, not folded into
a distance.

The measured consequence that is easy to get backwards is recorded with the
confusion table, not here: a slashed zero has hole count **2**, so it prunes
into `8`'s bucket, while a dotted zero keeps hole count 1 and prunes with `0`.
The pair to carry is `Ø`/`⌀` ↔ `8`; `0` ↔ `8` models a confusion that does not
occur.

### 2026-09-18 — `baseline_class` is a pruning bucket, not a metric line

Section 4.1 step 2 prunes candidates by it, and that is the whole of its
meaning. Read off its membership, the six values distinguish exactly two
independent bits: whether the glyph drops below the baseline, and where it
reaches relative to the x-line. `ascender` means "rises past the x-line", not
"reaches the cap line" — ISO 3098 draws `t` short of it, the dot on `i` sits
lower still, and an accented capital puts ink above it.

This is written down because a test asserted the stronger reading, that all 98
`ascender` glyphs render to the same height within a pixel, and its first effect
was to talk an author into raising `i`'s dot to the cap line to make it green. A
test whose premise is a misreading of a data field does not fail loudly; it
deforms the data until the data agrees with it. The face suite now asserts the
two bits per class and nothing more, with every bound a face metric rather than
a tuned threshold, and keeps its one tight cap-line equality scoped by the
charset's `category` column to unaccented capitals and digits, where a single
height is a design fact.

The operational consequence, which is not a test concern: a row classed tighter
than the glyph is drawn prunes the right prototype away before any distance is
computed. `ý` and `ÿ` were filed `full` and reach neither the cap line nor the
class's promise; they are `descender`. When a glyph and its row disagree, the
row is the thing to correct.

### 2026-09-18 — Every glyph is reviewed by rendering it, not by reading its tests

Mirror-invariance makes the obvious suite blind: a horizontal reflection
preserves bounding box, width, height, aspect, ink area, stroke count, hole
count, Euler number and baseline extent, so no scalar in a geometric font-QA
suite changes when a glyph is reflected. Capital `S` shipped through eight green
tests as a well-proportioned `Ƨ`, and lowercase `s` shipped as `digit_3`'s
literal path scaled to x-height, documented in the source as a virtue because it
inherited `3`'s verified zero-hole guarantee.

In a text face those are embarrassments. In a prototype bank an `s` and a `3`
at distance ~0 from each other destroy the one thing rule 5 promises: the match
margin either reports maximum confidence on a coin flip or collapses it for
every genuine `s` and `3` on the page. There is no version of it that fails
safely.

Reuse of a sub-shape — a bowl shared between `a`, `o` and `g` — is correct.
Reuse of a whole character's path across two different characters is a defect by
default. The pairs to watch are the mirror and near-mirror relatives: `s`/`3`,
`S`/`5`, `(`/`)`, `[`/`]`, `{`/`}`, `<`/`>`, `≤`/`≥`, `b`/`d`, `p`/`q`, grave
and acute.

### 2026-09-18 — Feature survival is gated at the smallest supported render, not a convenient one

Three defects in the authored face shared one cause: a feature was measured
against the drawing rather than against the pen, and checked at one comfortable
size.

The pen is `STROKE` wide and the authored path is its centreline, so ink reaches
`STROKE/2` past the path in every direction and every white feature the design
relies on shrinks by a full `STROKE`. A ring of radius `r` leaves an interior
`2 * (r - STROKE/2)` across: `Å` at `r = 45` held its hole at 64px and closed at
48px. A wave of crest-to-trough amplitude under `STROKE` puts its rising and
falling ink in the same pixels: the tilde at a 35-unit swing rendered as a
filled diagonal lozenge at *every* size, a sloped macron with a different name.

Hole count is the first pruning stage, so neither of these degrades a score.
`Å` lands in `A`'s bucket, `Ã` in `Ā`'s, and the correct prototype is discarded
before any distance is computed — a confident miss at exactly the sizes 300dpi
body text arrives at. The ring is now `r = STROKE`, giving an interior one full
pen wide, and the tilde swings `1.3 * STROKE` across a mark widened to hold its
aspect near 2:1. Both were judged by rendering them, at several sizes.

`hole_count_survival_report` is the permanent form of the check: every glyph,
swept from 16px to 96px, printing the size at which its hole count stops
matching its design. It is a report rather than a gate because the number it
produces is an input to a minimum-DPI decision that has not been made yet, and
because the honest answer is per glyph, not per face. As of this entry, 146 of
148 glyphs hold their hole count at every size from 16px up; `Å` and `å` lose
one at 16px only, where the ring is 1.1px of pen and breaks rather than fills,
letting the counter leak into the background. That is a connectivity failure,
not a coverage one, and the rasteriser's coverage rule does not address it.

The sweep also found that `.`, `·` and `…` fail to rasterise at all at 16–20px.
That one is not a drawing error and is recorded separately below.

### 2026-09-18 — The rasteriser samples coverage on an odd subgrid, and the subgrid's parity is the whole point

Testing a pixel's centre alone makes ink a function of sub-pixel phase. At 16px
per em the pen is 1.1px across, so `.` and `·` put down no ink at all at 16px
and 20px, while `…`, whose three dots sit at different offsets, survives 16px
and fails at 20px: survival is not monotonic in size, so no threshold can be
bisected for. Well before a mark vanishes the same sampling quantises ink area
by phase — a stem 1.4px wide renders 1px or 2px depending where it lands, and
`extract` reads that as signal.

`render` now estimates each pixel's coverage on a `SUBSAMPLE × SUBSAMPLE`
subgrid and inks it at a strict majority, with the rule that a mark present in
the design is present in the bitmap: if no pixel reaches the majority, the
best-covered one is set. Output stays 1-bit, which is what `extract` requires.

**`SUBSAMPLE` is odd because an even subgrid biases every edge outward.** With
an even count the offsets straddle the pixel centre, so an edge anywhere in the
middle `1 / SUBSAMPLE` of the pixel scores exactly half the samples. Counting a
tie as ink then lays a `1 / (2 · SUBSAMPLE)` px band of extra ink along every
edge — a quarter pixel added to the width of every stroke at 4×4, which is 5.6%
on this face's pen at 64px. That is not a rounding detail: it was measured as
2–5% more ink than FreeType puts down for the same outline, across the whole
face at every size. An odd count puts a sample exactly on the pixel centre, and
a strict majority flips precisely when the edge crosses it.

Verified against FreeType rendering the face's own emitted TrueType by-product,
over 148 glyphs × 8 sizes from 16px to 96px, as the ratio of reference ink area
to FreeType's:

| rasteriser | median | mean | stdev | glyphs producing no ink |
|---|---|---|---|---|
| centre sample, inclusive | 1.000 | 1.012 | 0.180 | 2 at 16px, 3 at 20px |
| 4×4 subgrid, tie counts as ink | 1.022 | 1.055 | 0.176 | none |
| 5×5 subgrid, strict majority | 1.000 | 1.001 | 0.172 | none |

Below 24px the ratio falls to 0.92–0.97 because FreeType's monochrome renderer
applies dropout control, deliberately over-inking features thinner than a pixel.
That is a difference in policy at the bottom of the range, not an error on
either side, and this face's own guarantee — a mark in the design is a mark in
the bitmap — is the narrower one.

The change moves every rendered prototype, so it was landed on its own with the
before-and-after above rather than beside glyph authoring. Every extent, aspect,
symmetry and hole-count expectation in the suite survived it unaltered, and the
aspect values `charset.tsv` records as measured are unchanged to three decimals,
so the notes beside those bands remain true.

### 2026-09-18 — A charset band that equals a design value is a broken band

Twelve glyphs in a face drawn to the charset's own aspect bands report out of
band, from two causes that produce one identical symptom.

`3`, `5` and `k` are drawn 350 units wide against a 700 cap — aspect 0.500
exactly, against a floor of 0.50 — and measure 0.491 at 64px, 22 pixels where
22.4 were needed. The glyphs are right; a bound sitting on a legitimate design
value cannot survive quantisation to whole pixels. One pixel at the gate size is
0.022 of aspect, and every bound needs at least that much clearance.

`i`, `l`, `!`, `'`, `.`, `:` and `·` are each exactly one pen wide: design
aspect `STROKE / CAP` = 0.100, measured 0.089, against floors of 0.10 to 0.15.
Those floors were authored as if for serifed characters. In this face, and in
plenty of geometric sans faces, a period is one stroke wide, so the band is
wrong about the world rather than about the face.

`1` at 0.268 against a 0.30 floor and `æ` at 1.071 against a 1.05 ceiling are
neither of the above — real disagreements between the drawing and the band, each
decided on its own merits. The `1` floor was authored for a foot-serifed form;
technical lettering draws `1` as a stem with a short upper-left flag and no
foot, which is what this face draws and what the floor now admits at 0.20. The
`æ` ceiling now matches the capital ligature's, because 1.071 is inside the
range real faces show — but it is on the wide edge of it, and the reason is that
this face barely fuses the `a` and `e` halves. That is a drawing note, on the
watch list, not a band problem.

Both broken kinds tempt the same wrong fix — widen until green — which turns the
band into a record of whatever was drawn, the one thing a band exists not to be.
Every band that moves carries a note in the charset's notes column saying which
of the two applied. A band changed without that note is indistinguishable from a
band moved to pass a test.

### 2026-09-18 — Every round glyph in the face was 6% oversized on its diagonals

`ring()` built its four quadrant arcs with the control point at the tangent-line
intersection — the quadrant's outer corner — and the source called that "the
exact least-error single-quadratic approximation of a 90-degree arc". It is
neither. A quadratic's midpoint is `(p0 + 2*ctrl + p2) / 4`, so an arc from
`(r, 0)` to `(0, r)` with control `(r, r)` passes through `0.75 * r * sqrt(2)`,
which is `1.0607 * r`. Every `O`, `0`, `8`, `C`, `G`, `Q`, bullet and combining
ring in the face bulged 6.07% at 45 degrees and was a squarish superellipse.

The control is now solved rather than chosen: setting that midpoint radius equal
to `r` gives `k = (2*sqrt(2) - 1) / 2 = 0.9142`, with the remaining radial error
peaking at 0.8% inward near the quarter-points. That is a constant with a
sentence behind it, which is what rule 1 asks for; the old one had a sentence
too, and the sentence was wrong.

Nothing in the suite could have caught this. The arc's endpoints are on-axis, so
bounding box, width, height, aspect, cap contact and baseline contact are all
exact under the error, and a bulge enlarges a counter rather than closing it, so
hole count survives as well. It surfaced only because the TTF emitter became a
second consumer of the same stroke data and disagreed with the rasteriser on
isolated pen dots — where nothing masks the error, because for a ring both
consumers flatten the same already-bulged centreline and it cancels.

Correcting it broke `6`, and that is the part worth remembering. The hook's
terminal had been authored to land exactly on the bowl's centreline, and with a
true circle the hook runs *alongside* the bowl wall instead of crossing it; the
strip of background trapped between the two flanks is a second hole, measured as
2 regions against a design of 1. The bulge had been doing the joinery. The
terminal is now half a pen inside the ring, so the two ink bands overlap across
their width rather than meeting edge to edge — which is the rule for every join
in the face, and the thing to check wherever two strokes meet.

### 2026-09-21 — Two characters in the face are one bitmap below 21px, and that is the minimum-DPI number

The entry above leaves the minimum-DPI decision open and names the per-glyph
survival sweep as its input. That was the wrong input. The sweep answers *at
what size does this mark still render*, and every glyph in the 187-glyph face
now passes it, because the best-pixel fallback guarantees a mark in the design
is a mark in the bitmap. What it cannot answer is whether two glyphs are still
**distinct**, and the fallback is precisely what makes them stop being so: a
mark too small to reach the coverage threshold is not made legible by receiving
its best pixel, it is made *present*, at whatever size and position the
threshold happened to leave it.

MEASURED, unweighted L2 over the raw 107-dim feature vector, every integer size
16–40px, all 17,391 pairs with no bucket filtering, run twice independently:

```
size   i / ï              . / …
16     IDENTICAL          IDENTICAL
17     ok  0.833          IDENTICAL
18     IDENTICAL          IDENTICAL
19     IDENTICAL          ok  2.535
20     IDENTICAL          IDENTICAL
21-40  ok  0.49-0.50      ok  2.55-3.25
```

`IDENTICAL` is exact: distance `0.0` over rasters that are pixel-for-pixel the
same, confirmed by comparing bytes rather than by trusting the metric — `ï`'s
dieresis merges into `i`'s stem and `…`'s three dots collapse to `.`'s single
pixel. Those two pairs are the only collisions anywhere in the range.

**The floor is 21px**, bounded by the top of the sweep: the claim is "no
collisions from 21px to 40px", not a statement about every size. In the units
the decision is actually made in, `px_per_em = points * dpi / 72`, so 21px is
10pt text at roughly 150dpi or 8pt at roughly 190dpi. A 300dpi scan of body text
has three times the margin it needs; small annotation text on a 150dpi scan does
not.

Two properties of that number matter more than the number.

**It is not the smallest size that works.** Both pairs separate at one size and
re-collide at larger ones — `i`/`ï` is clean at 17px and broken at 18, 19 and
20. A bisection, or any search for the smallest passing size, returns 17 and
ships three broken sizes above the floor, each of which would pass a spot check
taken at the floor. A minimum-size floor is one more than the *largest* failing
size found by an exhaustive integer sweep, and the sweep's range is part of the
claim.

**It is a property of a pair, and no per-glyph assertion implies it.** Extent,
aspect, hole count and symmetry all pass on both members of both colliding
pairs, because each glyph is individually correct; the defect exists only in the
relationship between two of them. The check had to be written deliberately as a
face-wide pairwise exact-zero sweep, and it is a report rather than a gate for
the same reason the survival sweep is — the floor is an input to a decision, not
the decision.

A note on what the fallback cost, since it is a rule the project already holds
elsewhere. Before it, a mark too small to render returned `None` and stopped a
build. After it, the same mark ships as a different character and reaches the
classifier as a confident wrong answer. That is the same trade rule 6 rejects
for the lexicon — an invisible error arriving with high confidence is worse than
a visible failure — and it argues for the floor being *declared* rather than for
the fallback being removed, because removing it restores a loud failure at sizes
we have now decided not to support anyway.

### 2026-09-21 — `§` was redrawn to an open form; the metrics were never the problem

The `§` recorded above as narrowed-to-satisfy-a-band was redrawn. The first
redraw attempt passed every assertion in the suite and still read as a lopsided
`8` to anyone who looked at it, which is the failure mode the render-every-glyph
entry exists to catch and did catch.

The structural fact the first attempt missed: **`§` has two free stroke
terminals**, one sweeping up to the right and one down to the left, where `8` is
a closed figure with zero endpoints. That, not width and not hole count, is what
a reader uses. The present glyph has both terminals and two open hooked lobes.

Measured against `8`: minimum distance over 16–64px rose from 0.372 to 0.540, a
45% improvement, and the bounding boxes are no longer identical (20×36 against
22×36 at 48px). Reported as a partial win, because at 64px the same-bucket
distribution has a 1st percentile of 0.528 and a median of 1.546 — the pair
still sits near the 1st percentile, both glyphs still carry hole count 2, and it
therefore stays on the confusion-candidate list rather than coming off it. The
endpoint count is the discriminating feature and it is not currently any
dimension of the feature vector; that is a question for the chunk-3 bank, not a
defect in this glyph.

### 2026-09-21 — Four charset ceilings widened, by a rule rather than by inspection

`<`, `>`, `Ω` and `™` measured outside their `authored-provisional` bands with
correct drawings — the adjudication the band entry above calls for, resolved as
*bound wrong, glyph right* in all four cases. The ceilings moved by a single
derived rule rather than four judgements:

> new ceiling = measured + 1.5 px of width quantisation

At 64px against a 44.8px cap one pixel is 0.022 of aspect, so the margin is
0.034. That gives `<` and `>` 0.84, `Ω` 0.97, `™` 1.06. One and a half pixels
because one leaves a bound a single rounding step can breach again. Uniformity
is the point: a round number chosen just above a measurement is indistinguishable
from a bound moved until the test went green, and rule 1 wants a number that can
be argued with. `aspect_source` stays `authored-provisional` because every floor
is still authored; each `notes` field now carries the measured value and the
derivation.

The widening was checked for new collisions, and the check is narrower than it
first appears: only partners whose floor sits *above* the old ceiling are newly
admitted, and only partners inside the same prune bucket are ever consulted. A
naive query returns twelve `ascender` glyphs overlapping `Ω`'s new range; eleven
already overlapped the old one and the twelfth, `Æ`, is in a different
hole-count bucket. Zero new collisions across all four. All 187 glyphs are now
in band.

### 2026-09-21 — The resolution floor is set per text line, not per document; 300dpi is the practical minimum for drawing annotation

The operator's direction: "the resolution floor should be variable, but if you
need a minimum choose something practical." The floor is a property of the
rendered text size, not of the scan. A single page can carry a 40px heading
and an 18px footnote at one scan DPI; a document-level DPI gate would reject
the whole page or wave the footnote through, and neither is right. The engine
instead measures effective px-per-em per text line, read from the x-height
recovered during line grouping. That is a planned output, not a present
one: `layout/lines.rs` is a one-line stub until chunk 2 is built, and
chunk 2's exit gate already requires baselines and x-heights. So this
policy adds no measurement chunk 2 was not already going to make, and it
cannot be implemented before chunk 2 lands.

Three tiers, and the engine never refuses:

| Effective px per em | Behaviour |
|---|---|
| >= 23 | Normal operation; confidence reported as calibrated. |
| 21–23 | Recognised; confidence capped, the cap value held in the parameter block rather than hard-coded. |
| < 21 | Recognised; confidence capped harder. The engine still returns text. |

The exact cap values are not set here. They are chunk 8 tuning parameters,
flagged as such rather than invented or implied to already exist. The floor
never becomes a refusal — this is the case rule 5 exists for: the honest
report at small text is a low confidence number, not a refusal, and a refusal
would also throw away lines a reviewer could still have used.

Where 21 comes from is measured, and already on the record in the 2026-09-21
minimum-resolution entry above: an exhaustive 17,391-pair sweep, every integer
size from 16px to 40px, found two colliding pairs below 21px and none from
21px to 40px.

Where 23 comes from is a derivation, not a round number. 21px is where
collisions stop in a clean synthetic render with no noise. A real scan adds
sensor noise, optical blur and binarisation error, any of which can close a
gap that is only just open in the clean case. The margin chosen is one stroke
width at the floor — the pen is the smallest feature in the design, so a
margin narrower than the pen cannot protect any feature the design relies on.
`STROKE` is 70 design units against `UPM` 1000, so at 21px the pen is 1.47px,
and 21 + 1.47 rounds up to 23.

Per rule 1: the 21 is measured; the choice of one stroke width as the margin
is authored, and it is on chunk 8's validation list. It has never been checked
against a real scan, only reasoned about, and that is flagged here as plainly
as the number itself.

In the units the operator will actually use, `px_per_em = points * dpi / 72`.
23px is 10pt text at roughly 165dpi. The practical recommendation: scan at
200dpi or better for 10pt body text, 300dpi is comfortable, 150dpi puts 10pt
text below the measured floor. The case that actually bites in CAD drawings is
smaller: 6pt annotation text needs 276dpi to clear 23px, so 300dpi is the real
floor for drawing annotation, not for body text.

### 2026-09-21 — The engine reads any font; the shipped package stays permissive-only — two separate questions

The operator's question, verbatim: "Can we support any font, just not include
in our package the ones that require licensing issues?" Yes, and this entry
separates three questions that had been running together as one.

What the engine can read is not limited by what we ship. The prototype bank
holds shape categories, not faces — a face the bank has never seen is
recognised to the extent its letterforms fall inside the shape variation the
bank already covers, which is why section 4 builds the bank across many faces
rather than many sizes of one. A coverage failure — a blackletter `A`, a
geometric single-storey `a` against a bank built only on double-storey forms —
is a shape problem, not a licence problem, and the two are diagnosed
differently: a shape gap is fixed by widening font coverage per the
escalation protocol above; a licence question never touches the bank at all.

What we ship stays permissive-only, unchanged. Rule 2 is not being relaxed.
Section 10 already records the stricter-than-necessary position: rendering
glyphs to build feature vectors is not redistribution of a font and no font
data ships in the model, yet the bank is still restricted to permissive faces
regardless. That restriction is deliberate belt-and-braces, kept so the
licence case stays arguable in one sentence, and it stays exactly as written.

The new piece is a user-side bank extension. `ocrcer-build` already renders a
font to feature vectors; the same operation, exposed as something the user
runs on their own machine against fonts they already have licensed, produces
a supplementary prototype file that loads alongside the shipped `.ocrw`.
Licensed font data never enters our package, never enters our repository and
never reaches us — the supplementary file stays the user's. That places three
requirements on the container: the supplementary file must be the same format
as the shipped model, must be version-checked against the shipped model it
extends, and its prototypes must be tagged with their origin so a confidence
report can say which bank a match came from. The format itself is not
designed here — it is an input to the `.ocrw` container work, and
`ocrcer-exporter` owns it.

What this does not do: it does not let a user add a class the charset does
not have. The charset is frozen and the decoder's tables are keyed to it;
adding a class is a model rebuild, not an extension.

The same mechanism is the answer to the scripts and hands v1 excludes.
Cursive handwriting, which the operator has said is wanted later, stays out
of scope for v1 per rule 7, unchanged — and the bank-extension mechanism is
the path by which it arrives without a v1 redesign, because a hand is shape
variation, not a new architecture.

### 2026-09-21 — Authored faces are correct where a standard defines the design, and cannot substitute for designer independence anywhere else

The operator asked why not self-author substitutes for the four open
font-licence questions in `ROADMAP.md`. `crates/ocrcer-build/src/face/`
already is that authoring for the one category that needed it: 2,765 lines
across six submodules, an ISO 3098 Type B face covering all 187 charset
classes, with accents composed from authored bases in `glyphs/accents.rs`.
Per `model/fonts.tsv`, eligible-present faces by category are 8 mono, 6 sans,
2 serif, 2 condensed, and exactly 1 technical (STIX Two Math, a mathematical
face, not CAD lettering) — so technical had no genuine eligible-present
member before this face existed.

Authoring is correct there because ISO 3098 publishes the letterforms: the
standard specifies the shapes, so an authored implementation of it is not an
imitation of anyone's face and there is no shape independence to lose, since
conformance is the goal.

It does not generalise. Faces drawn by one author correlate — shared ideas
about stroke contrast, terminal shape, bowl curvature, and joins mean ten
faces by one hand sit closer to one face plus noise than to ten independent
samples. The value of a multi-font prototype bank is independent shape
variation across real type designers; a bank padded with self-drawn
substitutes would look diverse by name and sit narrow in feature space, and
the failure would not show at build time — it would show only as degraded
generalisation to unseen real-world type. **This is filed as an argument,
not a measurement** — the correlation has not been quantified for this
bank. Proposed measurement, not yet run: compare intra-authored-face
feature variance against intra-real-face variance, as a chunk 8 or chunk 11
task.

Consequence for `ROADMAP.md`'s four open questions: osifont, DejaVu Sans and
Terminus each sit in a category already covered by eligible-present real
faces, so each reframes from "approve this licence" to "drop this face,
nothing is lost." `norm-stroke` (item 4) is the exception — it would be a
second, independently drawn ISO 3098 face, exactly the independence
self-authoring cannot supply. All four remain open operator decisions;
nothing in `fonts.tsv` or the roadmap items changed status from this entry.

### 2026-09-21 — The bank segments by distribution, not by inclusion; the drop recommendation above is superseded

The operator's direction, verbatim: "keep all, just make the ones with
distribution problems due to licensing separated out for ease of removal in
cases where the end user will have to get them themselves." This supersedes
the drop recommendation in the entry immediately above for items 1–3 of
`ROADMAP.md`'s open questions — osifont, DejaVu Sans and Terminus all stay
in scope. It does not touch that entry's correlation argument: self-authored
substitutes still cannot supply independent designer variation, which is why
`norm-stroke` (item 4) still buys something the other three don't. What
changes is the container, not the roster.

**The bank segments.** `.ocrw` carries a **base segment**, shipped with the
package — prototypes derived from permissive faces (OFL, Apache-2.0, public
domain) plus the authored ISO 3098 face — and zero or more **optional
segments**, not shipped. Each optional segment is independently droppable.
This amends section 2's "one bank of prototypes" framing and section 7's
table directory, which as currently written describe a single flat
`prototypes` table with no segment concept; neither is rewritten by this
entry, per this log's append-only convention, but both are now read subject
to this amendment. Section 10's "the bank is restricted to unambiguously
permissive families regardless" is narrowed by this entry to describe the
base segment only — the belt-and-braces posture stands for what ships,
not for what the container as a whole can carry.

**Provenance is carried per prototype, not inferred.** Every prototype
records the source-face identifier it was derived from. The container adds
a face manifest table, one row per face: face identifier, licence, licence
source, and a `distribution` field valued `shippable` or `local-only`.
Segment membership is computed from `distribution`, never hand-maintained as
a second list — **`model/fonts.tsv` is authoritative; the face manifest is
generated from it by `ocrcer-build` at bank-construction time.** Concretely,
`fonts.tsv` needs a `distribution` column it does not have today (current
columns: family, style, licence, licence_source, status, path, category,
notes); this entry flags that schema addition as required before segmented
bank construction, owned by `ocrcer-glyphs` since `fonts.tsv` is its table.
Under it, osifont, DejaVu Sans and Terminus are `local-only`; every current
`eligible-present` row is `shippable`.

**Local-only faces build on the end user's machine, from the end user's own
copy of the font.** The package ships neither the font nor any prototype
derived from it. The user points a build tool at a font file they already
hold, and it emits a supplementary segment locally. This is the same
mechanism the 2026-09-21 entry above ("The engine reads any font...")
specified for an arbitrary user-supplied face; this entry applies it to the
four specific faces `ROADMAP.md` already has licence findings for, and
inherits that entry's unresolved dependency unchanged: `ocrcer-build` is
currently specified as not shipping, so some shipping path to the feature
extractor is needed for local segment construction to work at all. Per
rule 4 the extractor exists exactly once, in `ocrcer-core`, so a thin
shipping tool calling it without duplicating the stage is the likely shape —
named here as an input to the decision, not a settled one. Owned jointly by
`ocrcer-runtime` and `ocrcer-exporter`; not resolved by this entry.

**Dropping a segment must never silently remove a class — the load-bearing
safety property.** A class whose only prototypes live in a removable
segment goes unrecognisable when that segment is absent: not degraded
accuracy, a character the engine is blind to. This is the diameter-sign risk
`ROADMAP.md`'s open-questions section already records (`⌀`, carried by only
two eligible faces) generalised into a structural hazard the container must
guard against by construction rather than by inventory. Two obligations
follow:

- The builder computes, per class, how many prototypes survive with only
  the base segment present. **A class reaching zero base-segment prototypes
  is a build-time error, not a warning.**
- The loader treats a missing optional segment as a normal condition —
  reduced coverage, reported through the existing confidence machinery, per
  rule 5 — never a load failure. The container records which segments were
  present at load, so a report can say what the engine was working with.

**A legal point this entry does not resolve.** Whether a feature vector
derived from a licence-encumbered font is itself distributable is a legal
question this document does not take a position on. This design makes the
question moot rather than answering it: nothing derived from a `local-only`
face ever leaves the user's machine, so the container routes around the
uncertainty instead of betting on an answer either way.

**Consequence for `ROADMAP.md`'s four open questions**, not edited here —
that document is owned elsewhere this session. Under this design, osifont,
DejaVu Sans and Terminus are all `local-only` and all stay in scope;
`norm-stroke` (CC0) is `shippable` once acquired, unchanged from the entry
above. Closing those four items in `ROADMAP.md` itself is for its owner to
record.

### 2026-09-21 — Measured charset gap: U+2212 (minus sign) is absent

`model/charset.tsv`'s 187 classes already cover `$ ¢ £ ¥ € % – — ‰ † ‡` —
most of what a financial or accounting document needs. **U+2212, the true
minus sign, is not among them.** Hyphen-minus U+002D is present (index 12),
but U+2212 is the character many financial and accounting PDFs use for a
negative number, and it is a distinct codepoint the charset does not carry
today — measured by grepping `model/charset.tsv` for `2212`, no match.

This is recorded as a measured gap and as an input to the accounting-document
work planned elsewhere, **not as a decision to add the class.** Adding a
class changes the class count that the 17,391-pair collision sweep, the 21px
resolution floor, and every checked-in fixture (2026-09-21 entries above)
were measured against; all of those numbers would need re-deriving against
188 classes rather than 187. That cost is why this is flagged rather than
done.

### 2026-09-21 — Third-party faces are parsed by `ttf-parser` and rasterised by ours; the two are not the same decision

`CLAUDE.md` rule 4 names the feature extractor as the stage that must exist
exactly once, because a bank built with one extractor and read by another
would be measured with two different rulers and nothing would report the day
they diverged. **The same hazard exists one level below it, at the bitmap.**

A font file is two things: a set of curve coordinates, and a rendering
intent. Reading the coordinates is a parsing problem with one right answer —
`glyf` point lists, `CFF` charstrings, `cmap` lookups, `.ttc` indices — and
`ttf-parser` already solves it. Deciding which pixels are ink is not that.
It is a policy: sub-pixel sample positions, a coverage threshold, what
happens to a mark finer than a pixel, where the pixel lattice is anchored
(this document's 2026-09-18 odd-subgrid entry is exactly such a policy, and
the 2026-09-18 symmetric-span entry another). Every rasteriser answers those
differently and every answer is defensible.

So the authored face's prototypes and a licensed face's prototypes must come
off the *same* rasteriser or the bank holds vectors measured two ways. The
split is therefore: **parse with `ttf-parser`, rasterise with
`face::raster`'s policy, always.** A parsed face's contours are flattened at
the same fixed `QUAD_STEPS` and filled by non-zero winding onto the same
`Grid`, through the same sub-pixel sampling, the same majority threshold, the
same best-sub-threshold-pixel fallback and the same tight crop as the pen
path. The one thing that legitimately differs is the inside test itself —
pen-radius distance for a centreline design, winding number for a filled one
— because that difference *is* what the two designs mean.

**Dependency posture.** `ttf-parser` 0.25.1 is `MIT OR Apache-2.0` with an
empty `[dependencies]` table, verified by reading the crate's own
`Cargo.toml` and its `LICENSE-MIT`/`LICENSE-APACHE` files in the local
registry rather than from its README. It is added to `ocrcer-build` only.
`ocrcer-core`'s three invariants (section 3) are untouched: the crate that
ships still has zero dependencies outside `core`/`alloc`/`std`, because font
files are read at bank-construction time and never at recognition time.

**What was built**, and what was not. `outline` (the winding-number fill on
`raster`'s grid and sampling policy) and `ttf_load` (parse, walk, flatten,
never rasterise) exist and are tested; quadratic and cubic outlines both
load. The de Casteljau evaluator that was private to the pen path is now the
crate's only one, reached by both paths. **Measured** on this machine: all 19
`eligible-present` faces in `model/fonts.tsv` parse and render, with `o`
showing one counter, `8` and `B` two, `x` and `v` none, `H` taller than `x`,
and ink touching all four edges of every cropped box — at 48 px/em, chosen
well above the 21 px/em floor so a fused counter reads as a resolution
finding rather than a parsing bug. Neither the prototype bank nor the `.ocrw`
writer is built yet; those are the rest of chunk 3.


### 2026-09-21 — The bank is multi-scale; one canonical size was measured and rejected

Section 4 said "render the glyph at a canonical size" without naming one. It
is named now, and it is not one size.

**Why it was an open question.** Features normalise onto a 32x32 grid and are
mostly scale-invariant by construction. But hole count (dimension 96) and the
crossing counts (97..103) are taken from the *source* bitmap, deliberately, for
the reason section 3 gives — resampling closes counters. So a prototype
rendered large carries clean counters where a runtime glyph near the 21 px/em
floor may have fused ones, and a fused counter does not merely match worse, it
prunes into a different bucket.

**Method.** `ocrcer-build bank <build-sizes> <eval-sizes> <gate>` builds a bank
at one size list and classifies glyphs rendered at a *different* list, so the
number reported is held-out with respect to size. 20 faces (the authored ISO
3098 face plus the 19 `eligible-present` files on this machine), 187 classes,
unweighted squared-L2 over standardised features, 1 nearest neighbour.

**Measured**, on this machine, this date:

| Bank sizes (px/em) | Prototypes | Build | Evaluated at | 1-NN top-1 |
|---|---|---|---|---|
| 32 | 3,708 | 11s | 16, 21, 24, 40, 48, 64 | **84.22%** |
| 16, 24, 32, 48 | 14,832 | 40s | 21, 28, 40, 64 | **91.40%** |
| 16, 24, 32, 48 | 14,832 | 40s | 21, 26, 36, 56 | **90.95%** |
| 14, 16, 18, 20, 24, 28, 32, 40, 48, 64 | 37,080 | 113s | 21, 26, 36, 56 | **92.48%** |

The last two rows share evaluation sizes and so are directly comparable; the
first two do not, and are quoted at the sizes each was run at.

Multi-scale buys roughly 7 points over one size, and the single-size bank
degrades worst exactly where predicted: 69.58% at 16 px/em against 90.37% at
40. **The bank is multi-scale.**

Going from four sizes to ten buys 1.5 points for 2.5x the prototypes and 2.8x
the build time, so **size coverage is not where the remaining accuracy is.**
The specific size list stays a chunk 8 tuning question; what this settles is
the structural point that it is a list.

Neither number meets chunk 3's exit gate of >99%, and the gate is not being
blessed away. Where the shortfall lives, measured in the same run at
16/24/32/48 with hole-count pruning:

| Slice | Top-1 |
|---|---|
| ASCII classes | 95.96% |
| Non-ASCII classes | 86.68% |
| Case-folded (a twin counts as correct) | 92.46% |
| `math` / `symbol` / `currency` | 99.57% / 99.15% / 99.75% |
| `digit` | 98.00% |
| `upper` / `lower` | 87.64% / 87.80% |

The confusions are not diffuse. The largest families are accent *direction*
(`ù`/`ú`, `à`/`á`, `Â`/`Ä`, `È`/`É` — a two-or-three-pixel mark whose slope is
the only difference), dash *length* (`-`/`–`/`—`, which differ only in width
relative to x-height, a quantity the feature vector does not carry because
aspect saturates near +1 for all three), and case twins (`x`/`X`, `v`/`V`,
`w`/`W`, `l`/`I`). All three are families section 5's decoder is designed to
resolve — case from line geometry, accents and dashes from the confusion table
and bigrams — so the matcher-alone number is not the engine's number. It is
still not 99%, and the honest position is that chunk 3's gate as written
measures something the design does not promise in isolation. Restating the gate
is an architecture decision; it is not being made in this entry, because the
cheaper fixes below have not been tried yet.

**What has not been tried, cheapest first**, per section 4's risk pattern:
denser size sampling in the bank; more faces (the licence work is done, the
faces are inventoried); authored per-block weights in the L2; and only then
any change to the 107 dimensions — which would invalidate every vector already
computed and is the expensive move this project is organised to avoid.

### 2026-09-21 — `charset.tsv` authors aspect as `w/h`; the extractor stores `(w-h)/(w+h)`; the loader converts

A silent units mismatch, found by measuring rather than by reading.

`charset.tsv`'s `aspect_min`/`aspect_max` are plain width-over-height ratios —
`!` is authored `0.06 .. 0.25` with the note "STROKE/CAP = 0.100" — because
that is the form a person can read off a drawing and argue with. Section 3
stores the feature as `(w - h) / (w + h)` instead, for the reasons given there.
The two are the same quantity under the exact, monotonic map
`r -> (r - 1) / (r + 1)`, but they are not interchangeable numbers, and the
first implementation of the matcher's aspect prune compared one against the
other.

Nothing failed. The pruner simply rejected the correct class and the engine
looked slightly worse — the failure mode that makes a units mismatch worth a
decision-log entry rather than a commit message. `tables::load_charset` now
converts on load, and a test asserts every band lands inside the extractor's
open `(-1, 1)` and that `!` lands well below zero.

### 2026-09-21 — Aspect-band pruning is disabled: both forms of it were measured and both cost accuracy

Section 4.1 step 2 prunes by aspect band and baseline class. Measured on the
16/24/32/48 bank evaluated at 21/28/40/64, against 91.40% with no prune at all:

| Prune | Top-1 |
|---|---|
| None | **91.40%** |
| Hole count only | 91.38% |
| Hole count + `charset.tsv`'s authored band | 49.22% |
| Hole count + bands measured from the bank's own prototypes | 88.81% |

**The authored bands are not usable as a hard prune.** They are marked
`authored-provisional` in the `aspect_source` column, which is exactly the
label `CLAUDE.md` rule 1 requires of a guess, and the measurement is what that
label was waiting for. They are too tight: at 49.22% the gate is rejecting the
correct class roughly half the time.

**Bands measured from the bank fail differently, and more interestingly.** A
min/max taken over a discrete set of render sizes does not interpolate. A glyph
rendered at a size between two grid points lands outside the observed band on a
quantity that moves non-monotonically with pixel rounding, and the gate then
removes the correct class while leaving the wrong ones — so the "if nothing is
admitted, search unpruned" fallback never fires, because something else always
is. A gate may only exclude a class when exclusion is *certain*; a band
measured from a finite sample is not certain, and a widening tolerance would be
precisely the invented number this project does not permit.

So: **hole-count pruning only.** It costs 0.02 points, which is two characters
in 14,832, and that is the price of a step that cuts most of the bank. The
aspect step stays in section 4.1 as the place a defensible band would go. The
obvious next attempt, not yet run, is to measure the bands over a *dense* size
sweep while storing prototypes at a sparse one — which would make the band
cover the size range rather than sample it, with no invented constant.


### 2026-09-21 — The `.ocrw` writer exists; `int8` costs 0.55% of top-1 agreement and nothing measurable in accuracy

Section 7's container is written, and the claim it makes about `int8` is now
a measurement rather than an assertion.

**Measured**, on this machine, this date, from the 16/24/32/48 bank of 14,832
prototypes evaluated at 21/26/36/56 px/em:

| | |
|---|---|
| File | 1.55 MB |
| `f32` top-1 | 90.95% |
| `int8` top-1 | 90.97% |
| `int8` agrees with `f32` on top-1 | **99.454%** (14,751 of 14,832) |
| Rebuild byte-identical | yes, verified with `cmp` |

Quantisation changes the winner on about one glyph in 180 and costs no
measurable accuracy — the two accuracy figures differ by three glyphs, in
`int8`'s favour, which is noise and is reported as such rather than as a
finding.

**Two things in the writer are deliberate and would be easy to get wrong.**

*The build identifier is a digest, not a timestamp.* `PLAN.md` chunk 3 asks
that a rebuild produce the same bytes. Anything in the file that recorded the
occasion rather than the inputs would defeat that, and the defeat would be
invisible — the file would still load, still work, and still differ. So the
identifier is a CRC-32 over the `meta` block's own claims, computed last, and
it changes exactly when something the file says about itself changes.

*`FEATURE_VERSION` now exists in `ocrcer-core` and is written into `meta`.*
Section 7 required the feature-extractor version in `meta` and there was
nothing to put there. It is `1`. It is not a release number: it identifies the
107-dimension definition, and it is bumped whenever a dimension changes what
it measures, what order it sits in, or what it is measured from. A file whose
`meta` names a different value is to be refused at load rather than read,
because a mismatched pair does not fail — it quietly answers wrongly.

**What is not done.** There is no reader. Chunk 3's "file round-trips" clause
is unmet: the writer checks its own output's structure and CRC in tests, which
is not the same thing as a second implementation reading it back. The reader
belongs in `ocrcer-core` and is `ocrcer-runtime`'s work.

**Chunk 3's exit gate, clause by clause**, as measured rather than as hoped:

| Clause | Status |
|---|---|
| Bank builds in under 5 min | **pass** — 40s at four sizes, 113s at ten |
| Byte-identical on re-run | **pass** — verified |
| 1-NN over 99% on isolated rendered glyphs | **fail** — 90.95% to 92.48% |
| File round-trips | **not met** — no reader yet |
| `int8` top-1 agreement measured and reported | **pass** — 99.454% |

Three of five. The chunk is not done, and the 99% clause is the one that needs
an architecture decision rather than more code — see the multi-scale entry
above for what the shortfall is made of.

### 2026-09-21 — Section 7 now carries the segment design; an optional segment is a separate file

The 2026-09-21 segmentation entry above decided the bank segments by
distribution and explicitly declined to rewrite sections 2, 7 and 10,
deferring to this log's append-only convention. That convention governs
**this log**. Sections 2–10 are the contract, and a contract that has to be
read together with a later entry to be correct is a contract that will be
read wrong. Section 2's table and section 10's licence posture had already
been reconciled; section 7 had not. It now is — see the new section 7.1.

**Two things section 7.1 settles that the earlier entry left open.**

*Segment membership is derived, not stored.* Every prototype already records
its source face, and the `faces` table records each face's `distribution`.
The base segment is therefore computable — every prototype whose face is
`shippable` — and there is no segment field anywhere to fall out of step with
the manifest. The alternative, a per-prototype segment tag, would be a second
copy of a fact and would be wrong silently.

*An optional segment is a whole `.ocrw` file, not a region inside one.* The
earlier entry said "the container carries" optional segments, which reads as
regions of the shipped file. It cannot be that: the shipped file must be
byte-identical whether or not a user later builds a local segment, and a
region inside it would have to be absent from the shipped bytes anyway. So a
supplementary segment is a separate file with `model_kind = 2`, and it must
agree with the base file on feature-extractor version, charset digest and
normalisation constants or be refused at load. That is the same mismatch
guard the header's `version` field exists for, applied across two files
instead of across a file and a runtime.

**The asymmetry in the load rules is deliberate.** A missing optional file is
normal and reported through confidence. A class with zero base-segment
prototypes is a **build-time error**, because that is not reduced coverage —
it is a character the engine is blind to, with nothing in the output to say
so. `⌀` is the live instance.

**Not resolved here, unchanged:** how the feature extractor reaches an end
user's machine to build a local segment at all, given `ocrcer-build` is
specified as not shipping. Per rule 4 the extractor exists exactly once, in
`ocrcer-core`, so a thin shipping tool that calls it is the likely shape.
Owned jointly by `ocrcer-runtime` and `ocrcer-exporter`.

### 2026-09-21 — Head-to-head against `ocrs` over 660 pages: OCRcer loses the layout-free metric by 10 points, with oracle segmentation

`FEASIBILITY.md` section 6 condition 1 says this project wins by being better
than `ocrs` on printed documents and CAD drawings. That is now measured
rather than projected, and **the headline is a loss.** Raw output:
`docs/measurements/2026-09-21_vs_ocrs_660_pages.txt`.

**What was run.** 660 pages — 19 distinct faces x 7 authored text blocks x 5
render sizes (14/18/21/28/40 px/em), 125,815 characters. The bank was 14,832
prototypes from 20 faces at 16/24/32/48 px/em, so **every render size on
every page is held out from the bank.** Corpus text is original, authored for
this project (`CLAUDE.md` rule 2 applies to evaluation material).

|  | OCRcer (oracle seg) | `ocrs` 0.12.2 (end to end) |
|---|---|---|
| character accuracy, in order | **92.28%** | 77.26% |
| word accuracy, in order | **71.30%** | 65.51% |
| word F1, layout-free | 71.30% | **81.12%** |
| seconds | 172.0 | 273.8 |

**Read the second metric, not the first.** The in-order columns flatter
OCRcer enormously and the reason is not recognition: `ocrs` reads multi-column
tables **column-first**, returning nearly every character correctly in the
wrong order, and whole-page Levenshtein charges a wholesale permutation as
mass substitution. That is what produces `ocrs`'s 56.34% on the invoice block
and 49.46% on the statement block against 91.07% and 86.66% word F1 on the
same pages. Reporting only in-order accuracy would be picking a winner by
choosing a metric.

**On the metric that removes the layout disadvantage, `ocrs` wins by 9.8
points — against an OCRcer that was handed every glyph box and every space
from the page's own ground truth.** There is no segmentation, no line finder,
no lattice, no bigram and no lexicon in the OCRcer column. Its number is a
*ceiling* on a future end-to-end result, and the ceiling is already below the
competitor's floor.

**Two biases in this corpus, both favouring OCRcer, stated rather than
buried.** Every character in it is in `charset.tsv` — a charset chosen to fit
the test, which `ocrs` had no say in. And the pages are hard black-and-white
with no anti-aliasing, noise, skew or scanner blur, which is not the input a
convolutional recogniser was built for. A degraded-scan corpus would widen
the gap, not narrow it, exactly as section 9 predicts.

**Where the deficit actually lives: small render sizes.**

| px/em | OCRcer word F1 | `ocrs` word F1 |
|---|---|---|
| 14 | 54.49% | 71.30% |
| 18 | 62.21% | 79.83% |
| 21 | 71.03% | 82.67% |
| 28 | **84.30%** | 86.08% |
| 40 | **84.50%** | 85.47% |

At 28 and 40 px/em the two engines are within two points on word F1 while
OCRcer leads character accuracy by 16. The whole 10-point aggregate loss is
carried by 14 and 18 px/em — below the 21 px/em feature-survival floor this
log already measured. **The aggregate is a weighted average over a size range
two-fifths of which the engine has already been measured as unable to serve.**
That does not excuse the number; it locates it.

**What this does and does not license.**

- It does **not** license a charset or feature-vector change. The deficit is
  concentrated below a known floor and the decoder — the machinery designed
  to resolve exactly the confusion families the bare matcher loses to — does
  not exist yet. Chunks 5 and 6 are the cheap fix and they are unbuilt.
- It does **not** license reporting OCRcer's 92.28% anywhere as a win.
- It **does** make the minimum-DPI declaration urgent rather than deferred
  (`ROADMAP.md` open question 5). An engine that declines input below its
  floor and is level with `ocrs` above it is a different product from one
  that silently reads 14 px/em text at 54% word F1.
- It **does** set the hand-off gate: the precondition for telling `pdfcer`
  this engine is ready is layout-free word F1 above `ocrs`'s **end to end**,
  not with oracle segmentation.

**An incidental finding worth recording so nobody chases it as a bug.**
Cascadia Code and Cascadia Mono scored identically to four significant figures
for both engines. They are two distinct font files, and their rendered pages
are **byte-identical** here (verified with `cmp`), because the only thing
distinguishing them is ligature coverage and the page renderer does no
shaping. So "20 faces" in the bank is 19 distinct shape sets plus one exact
duplicate, and the corpus's 660 pages are 630 distinct ones. The effect on
every figure above is small and in neither engine's favour, but the bank
should stop paying for a duplicate — `ocrcer-glyphs`.

### 2026-09-21 — The wide charset costs 1.39 points, not the deficit; 58% of all errors are same-shape-different-size pairs

The head-to-head entry above left an explanation on the table that this
measurement mostly removes. Raw output:
`docs/measurements/2026-09-21_charset_cost_ablation.txt`, produced by
`charset-cost`, which reads every page twice from the same bank and the same
pixels — once with all 187 classes allowed, once restricted to the 94 ASCII
ones — ungated on both sides so the only difference is which classes may win.
Pages whose text is not ASCII are skipped, because restricting the answer set
would otherwise charge the restricted column for an error the restriction
itself caused. 475 of 660 pages, 102,885 characters.

| | full charset | ASCII-restricted | delta |
|---|---|---|---|
| character accuracy | 92.21% | 93.60% | **+1.39** |
| word accuracy | 70.70% | 75.64% | **+4.94** |

**The hypothesis this was written to test is largely wrong.** The bare
matcher's error list on ASCII text is conspicuously full of substitutions into
classes the text cannot contain — `i`→`ì` 182 times, `-`→`–` 178, `:`→`÷` 176
— and the obvious reading was that charset breadth is what costs the accuracy.
It is not. Of 8,019 substitutions, roughly 1,430 are charset-caused. **The
other 82% are ASCII losing to ASCII**, and a wide charset costing 1.4 points
of character accuracy is a price worth paying for the coverage.

**What is actually happening**, by count, from the same run:

| family | substitutions |
|---|---|
| `0` `o` `O` `Q` in every direction | 2,239 |
| `l` `I` `1` `\|` | 974 |
| `s` `S` `$` `5` | 608 |
| `c`/`C`, `u`/`U`, `v`/`V`, `w`/`W` | 910 |

That is about **58% of every error the matcher makes**, and it is one family:
**glyph pairs that are the same shape at a different size.** Not a confusion
the extractor is failing to resolve at the margin — the one it is failing at
wholesale.

**This is not a request to change the feature vector, and section 3 is why.**
Section 3's table already carries "aspect ratio, ink fraction, height above
baseline, depth below" and already says, in terms, that those four dimensions
exist because "`O` and `o` are the same shape and different characters, and
only the line's baseline and x-height tell them apart". The capability was
designed in. So the finding is not *a missing feature*; it is **a feature that
is present and not working**, which is a cheaper problem and a different one.

**The most likely cause is already named in section 4.1 and is not structural.**
Step 3 says the matching weights "are not yet authored; everything measured so
far is unweighted". Unweighted means all 107 standardised dimensions vote
equally. For `o` against `O` the 80 shape dimensions are near-identical and
carry no signal, while the 4 baseline-geometry dimensions carry all of it — so
even a perfectly informative set of 4 is outvoted 80 to 4 by dimensions that
agree. An authored weight on the baseline-relative group is the cheap fix, it
is already owed, and it changes no table, no charset and no file format.

**Why this family is worth more than its share of the error count.** The
decoder (chunks 5–6) resolves case from context readily *inside words* — but
`CLAUDE.md` rule 6 suppresses the lexicon inside identifier-shaped context,
which is exactly where `4100-02` and `10O1` live. `0` against `O` in a part
number is therefore the one confusion the language model is **forbidden** from
fixing, in the domain where a wrong character is most expensive. The matcher
has to win it on geometry or nobody does.

**Owed, in order, and none of it is a format change:**

1. Author weights for the baseline-relative group and re-measure this same
   ablation. `ocrcer-glyphs` and `ocrcer-linguist` jointly; a weight is a
   number that needs a sentence (rule 1).
2. Only if that fails to move the four families above: ask whether the four
   dimensions are being computed from a sound `x_height`, which is a chunk 2
   question about line geometry rather than a chunk 3 question about features.
3. Only if *that* fails: a feature-vector change, under the escalation this
   document's protocol requires.

**Recorded because it corrects something already written down.** An earlier
note this session attributed the matcher's ASCII errors to charset breadth.
That attribution was an observation of an error list, was labelled at the time
as not being a controlled ablation, and is now measured and substantially
wrong. The error list was real; the inference from it was not.

### 2026-09-21 — The baseline-geometry group was being outvoted: weighting it recovers +2.9 points, and buys the last of them by wrecking `0` against `O`

The entry above predicted that the four baseline-relative dimensions were
present and outvoted, and listed "author weights for the baseline-relative
group and re-measure" as owed item 1. That is now measured. Raw output:
`docs/measurements/2026-09-21_feature_weight_sweep.txt`, produced by
`feature-weights`, which multiplies dims `103..107`'s contribution to the
squared distance and re-reads the same pages from the same bank and the same
pixels once per candidate. Every pass is ASCII-restricted and ungated, so the
multiplier is the only thing that differs between rows. 158 pages at stride 3,
which spans every face rather than the alphabetically first few.

| weight | character accuracy | word accuracy | size-pair errors | other errors |
|---|---|---|---|---|
| 1.0 (control) | 93.53% | 75.13% | 1699 | 518 |
| 2.0 | 94.31% | 77.61% | 1484 | 468 |
| 4.0 | 94.95% | 79.84% | 1274 | 456 |
| 8.0 | 96.01% | 83.69% | 952 | 416 |
| 16.0 | 96.44% | 85.46% | 844 | 375 |
| 32.0 | 96.47% | 85.64% | 821 | 388 |
| 64.0 | 96.54% | 86.23% | 781 | 406 |

**The hypothesis holds.** The control's 93.53% agrees with the charset-cost
ablation's ASCII column (93.60% over 475 pages) to within 0.07 points, which is
an independent check that the subset is representative and that the two tools
agree. Against it, weighting one group of four dimensions buys **+2.91 points
of character accuracy and +10.33 of word accuracy** — from a multiplier, with
no change to the charset, the feature vector, the stored prototypes or the file
format. The capability really was designed in and really was drowned.

**And the aggregate is lying about what it bought.** Per pair, across the
sweep:

| pair | w=1 | w=2 | w=4 | w=8 | w=16 | w=32 | w=64 |
|---|---|---|---|---|---|---|---|
| `O` -> `o` | 155 | 138 | 102 | 54 | 17 | 3 | 3 |
| `o` -> `O` | 141 | 120 | 107 | 50 | 35 | 6 | 0 |
| `0` -> `o` | 172 | 156 | 156 | 51 | 18 | 12 | 12 |
| `C` -> `c` | 74 | 49 | 32 | 1 | 0 | 0 | 0 |
| `u` -> `U` | 69 | 69 | 54 | 41 | 22 | 14 | 0 |
| **`0` -> `O`** | **151** | **137** | **137** | **198** | **231** | **267** | **263** |
| **`O` -> `0`** | **85** | **92** | **108** | **103** | **129** | **137** | **141** |
| `l` -> `I` | 140 | 135 | 133 | 133 | 116 | 105 | 99 |
| `e` -> `c` | 43 | 43 | 43 | 43 | 43 | 43 | 43 |
| `r` -> `c` | 0 | 24 | 24 | 24 | 24 | 24 | 8 |
| `o` -> `a` | 0 | 0 | 0 | 0 | 0 | 28 | 28 |

**The 58% family was never one problem. It is three, and only one of them is a
weighting problem.**

1. **Pairs that differ in height** — `o`/`O`, `c`/`C`, `s`/`S`, `u`/`U`,
   `v`/`V`, `w`/`W`, `0`/`o` — go monotonically to near zero. These are what
   dims `103..107` were put in the vector for and they now work.
2. **Pairs that are the same height** — `0` against `O`, and most of
   `l`/`I`/`1`/`|` — get *worse*. `0`↔`O` is 236 substitutions at the control
   and **404 at the aggregate optimum**, a 71% increase. This is not a tuning
   accident: `0` and `O` are both cap-height, so all four of these dimensions
   agree on them by construction, and turning the group up amplifies whatever
   noise those four carry while quieting the 103 dimensions that hold the only
   real difference — width and counter shape.
3. **Pure shape confusions** — `e` read as `c`, 43 occurrences, identical at
   every weight to the last count. Untouched, as they should be.

The sweep also *creates* errors that did not exist: `r` read as `c` appears at
w=2, `o` read as `a` at w=32, `S` read as `$` quadruples. Same mechanism —
geometry loud enough to outvote shape is the original fault with the sign
flipped.

**Decision: a single scalar on a group is the wrong parameterisation, and it is
not what gets authored.** The measurement's job was to test whether the group's
share of the distance was the fault. It was. It is not evidence that the fix is
one number, and the `0`↔`O` column is evidence that it is not: within these
four dimensions, one number cannot separate a pair that differs in height
without also amplifying a pair that does not. What section 4.1 step 3 owes is a
**per-dimension** weight vector. `ocrcer-glyphs` and `ocrcer-linguist` jointly,
and rule 1 still applies to every entry in it.

**Decision: the weights are not selected against aggregate character accuracy,
and section 4.1 now says so.** This is the part that would have gone wrong
quietly. A sweep that maximises total accuracy picks w=64 and is right to — it
trades roughly 170 new `0`↔`O` errors for roughly 600 fixed case errors.
But `CLAUDE.md` rule 6 suppresses the lexicon inside identifier-shaped context,
so a case error inside a word is one the decoder repairs for free and `0`
against `O` inside `4100-02` is the one confusion the decoder is **forbidden**
to repair. The objective the matcher is tuned against must therefore be
weighted by what the decoder cannot fix downstream, not by the error count.
Of the points swept, **w=2 is the only one where `0`↔`O` is no worse than the
control** (229 against 236) while still gaining +0.78 character and +2.48 word
— which is the shape of the trade, not a recommendation of the value.

**`0` against `O` is not a feature-vector gap, and owed item 2 does not apply
to it.** Owed item 2 asked whether these four dimensions receive a sound
`x_height`. For `0` against `O` the question is empty: both are cap-height, so
`x_height` is not what would tell them apart at any accuracy. The real
separators are dim `103`, aspect — `0` is narrower than `O` in most faces — and
the counter, which the gradient and zone dimensions already carry. Both are
present. Whether a per-dimension weight can reach them is the next measurement,
and **no feature-vector change is authorised on the strength of this run.**
If it turns out it cannot, the architecturally cheapest remaining answer is
already in the document: rule 5's confidence is the margin to the nearest rival
of a different class, so an unresolvable `0`/`O` reports a low margin and gets
flagged rather than silently guessed. That is the behaviour a reviewer needs,
and it costs nothing to build.

**What this does not license anyone to say.** Every figure here is
oracle-segmented and ASCII-restricted on both sides, so all of them are
ceilings, and only the differences between rows are the measurement. The
head-to-head against `ocrs` has not been re-run at any weight as of this entry.
Carrying the +10.33-point word-accuracy delta across to the 660-page
head-to-head would be a projection, and the hand-off gate in `PLAN.md` is a
measured layout-free word F1, end to end on both sides.

### 2026-09-21 — Head-to-head re-run at the candidate weight: the 9.82-point deficit becomes 0.10, and it is still a loss

Supersedes the closing paragraph of the entry above, which recorded that the
head-to-head had not been re-run at any weight. It has been. Raw output:
`docs/measurements/2026-09-21_vs_ocrs_660_pages_w16.txt`, same 660 pages, same
bank, same `ocrs` 0.12.2 and the same weight files `pdfcer` ships, the single
difference from `2026-09-21_vs_ocrs_660_pages.txt` being `--geometry-weight 16`
on the OCRcer side.

| | OCRcer, unweighted | OCRcer, dims 103..107 x16 | `ocrs` 0.12.2 |
|---|---|---|---|
| character accuracy | 92.28% | **95.21%** | 77.26% |
| word accuracy (in order) | 71.30% | **81.02%** | 65.51% |
| layout-free word F1 | 71.30% | **81.02%** | **81.12%** |
| seconds | 172.0 | 229.1 | 251.3 |

**The gate is still not met.** `PLAN.md`'s hand-off condition is a layout-free
word F1 above `ocrs`'s, measured end to end on both sides. This is 0.10 points
below it, and OCRcer's side is not end to end — it was handed every glyph box
and every space from the page's own ground truth. A tenth of a point behind
with oracle segmentation is a loss, and the distance to a real result is
whatever chunk 2's binariser, component finder and segmenter cost, which is
unmeasured and will not be zero.

**Two things did move and are worth naming.** Character accuracy is now 17.95
points ahead rather than 15.02, and the gap that the earlier entry located
entirely at small render sizes has inverted at large ones:

| px/em | OCRcer word F1 | `ocrs` word F1 |
|---|---|---|
| 14 | 66.20% | 71.30% |
| 18 | 75.33% | 79.83% |
| 21 | 79.49% | 82.67% |
| 28 | **91.40%** | 86.08% |
| 40 | **92.68%** | 85.47% |

At 28 and 40 px/em the weighted matcher is 5–7 points ahead on the layout-free
metric, not merely level. Below 21 px/em it is still 3–5 points behind, and
that is the same minimum-DPI question `ROADMAP.md` open question 5 has been
carrying — this measurement makes it the load-bearing one rather than a
deferred one.

**This number was bought with the trade section 4.1 now forbids, and it is
not a recommendation of w=16.** The sweep entry above measured that w=16 makes
`0` against `O` 71% worse than the control while fixing the case pairs. A
head-to-head scored on aggregate F1 cannot see that and gives w=16 full credit
for it. The figure is recorded because it answers "is the geometry weight worth
pursuing at all" — it is, decisively — and for no other purpose. **No weight is
authored on the strength of this run**, and the per-dimension weight set that
eventually is authored has to clear the per-pair bar in section 4.1 as well as
the aggregate.

### 2026-09-21 — Adding a 12px bank scale makes small text *worse*; the answer to a resolution floor is not more prototypes below it

The entry above located the remaining deficit against `ocrs` below 21 px/em and
named it the load-bearing question. The obvious first move — the bank is built
at 16/24/32/48 px/em and the failing pages render at 14 and 18, so give it a
nearer scale — was tried and is wrong. Raw output:
`docs/measurements/2026-09-21_vs_ocrs_660_pages_w16_bank12.txt`, identical to
the weighted run in every respect but the bank's size list.

| | bank 16/24/32/48 | bank **12**/16/24/32/48 | `ocrs` |
|---|---|---|---|
| prototypes | 14,832 | 18,540 | — |
| layout-free word F1 | **81.02%** | 80.58% | 81.12% |
| character accuracy | **95.21%** | 94.97% | 77.26% |
| word F1 at 14 px/em | **66.20%** | 62.58% | 71.30% |
| word F1 at 18 px/em | 76.23% | **76.23%** | 79.83% |
| word F1 at 21 px/em | 79.49% | **80.02%** | 82.67% |
| OCRcer seconds | 229.1 | 297.1 | 251.3 |

25% more prototypes, 30% more time, and the size the change was aimed at lost
**3.62 points**. 18 and 21 px/em gained about half a point each, which does not
pay for it.

**Why, and it is already in this log.** The 2026-09-21 minimum-DPI entry
measured that below 21px distinct glyphs stop being distinct — `i` and `ï`
render pixel-for-pixel identically at 16, 18, 19 and 20px, and `.` and `…` at
most sizes in that range. A 12px prototype set is therefore not a *finer*
sampling of the same shapes; it is a set containing degenerate vectors that are
near-identical to each other and to any small blurred mark. Adding them to the
bank adds attractors, and a nearest-neighbour matcher goes to them. The scale
that is closest in pixels is not the scale that is closest in shape once the
counters have closed.

**Decision: the bank is not extended below its current smallest scale**, and a
future proposal to do so has to defeat this measurement rather than restate the
intuition behind it. The remaining small-text deficit is not a bank-composition
problem and the next measurement must look elsewhere — at the binariser and
segmenter chunk 2 will contribute, which is where a 14px page's ink is actually
being lost, and at whether the corpus's hard-thresholded rendering at 14px is
even a fair stand-in for a real 14px scan.

**Reported because it is a loss.** The hypothesis was mine, it was cheap, it
was reversible, and it was wrong in the direction opposite to the one expected.

### 2026-09-21 — Same prototype count, one different scale: 80.58% against 85.30%. The bank's scale ladder was the small-text deficit

Corrects the entry immediately above, which concluded from the 12px result that
"the remaining small-text deficit is not a bank-composition problem". It is
almost entirely a bank-composition problem. The 12px measurement was right; the
generalisation drawn from it was not. Raw output:
`docs/measurements/2026-09-21_vs_ocrs_660_pages_w16_bank20.txt`.

Both banks hold **18,540 prototypes** — five scales, same faces, same charset,
same weight, same pages. The only difference is which scale was added to
16/24/32/48.

| word F1 | +12 px/em | +20 px/em | `ocrs` |
|---|---|---|---|
| overall | 80.58% | **85.30%** | 81.12% |
| 14 px/em | 62.58% | **73.19%** | 71.30% |
| 18 px/em | 76.23% | **81.85%** | 79.83% |
| 21 px/em | 80.02% | **86.95%** | 82.67% |
| 28 px/em | 91.40% | 91.11% | 86.08% |
| 40 px/em | 92.66% | **93.38%** | 85.47% |

**4.72 points of layout-free word F1 separate two banks of identical size and
identical cost.** Character accuracy is 96.32% against 94.97%. Adding a scale
*below* the existing floor lost 3.62 points at 14 px/em; adding one *inside the
widest gap in the ladder* gained 10.61 at the same size, on pages rendered at
neither scale.

**With the 20px scale, OCRcer is ahead of `ocrs` at every render size in the
corpus**, including 14 px/em where it had been 5.10 points behind — 73.19%
against 71.30%. Overall 85.30% against 81.12%.

**The mechanism is a hypothesis and is labelled as one.** The feature vector is
scale-normalised to a 32x32 grid, so the size-dependent information lives in the
hole count (taken from the original bitmap) and dims `103..107`. A 20px
prototype's counters are open and its hole counts are the ones a 14–21px page
glyph will have; a 12px prototype's have closed, which makes it a near neighbour
of every small blurred mark regardless of identity. That is consistent with both
measurements and is not established by them.

**Decision, and the thing it is guarding against.** The ladder gets chosen by a
stated rule about its spacing, **not by sweeping scales against the benchmark
corpus.** 16/24/32/48 has step ratios 1.50, 1.33, 1.50; the corpus renders at
14, 18, 21, 28, 40, and a bank tuned scale-by-scale against those numbers would
be fitted to the test set in everything but name — precisely what `CLAUDE.md`
rule 1 exists to prevent, and it would not survive contact with a real scan at
an arbitrary DPI. 20px was proposed because it halves the widest step, which is
a sentence about the ladder rather than about the corpus. A uniform geometric
ladder is now being measured on the same basis; whatever is authored into
`model/fonts.tsv`'s scale list has to be justified by its spacing rule, with
the corpus used only to confirm the rule was worth having.

**Superseded by this entry:** the sentence "the remaining small-text deficit is
not a bank-composition problem and the next measurement must look elsewhere".
The rest of that entry stands — the bank is still not extended below 16px, and
that decision is now better supported, not weaker.

**Still not the gate.** OCRcer's column remains oracle-segmented. Leading
`ocrs` by 4.18 points while being handed every glyph box is not the hand-off
condition in `PLAN.md`, which is end to end on both sides.

### 2026-09-21 — The uniform geometric ladder loses to the uneven one; the scale search stops here and goes to `ocrcer-glyphs` with a rule attached

The entry above said a uniform geometric ladder was being measured on the same
basis. It was, and it is worse. Raw output:
`docs/measurements/2026-09-21_vs_ocrs_660_pages_w16_ladder125.txt`.

| bank scales | step ratios | prototypes | seconds | word F1 | 14 px/em |
|---|---|---|---|---|---|
| 16/24/32/48 | 1.50 1.33 1.50 | 14,832 | 229 | 81.02% | 66.20% |
| 12/16/24/32/48 | 1.33 1.50 1.33 1.50 | 18,540 | 297 | 80.58% | 62.58% |
| **16/20/24/32/48** | **1.25 1.20 1.33 1.50** | **18,540** | **258** | **85.30%** | **73.19%** |
| 16/20/25/31/39/48 | 1.25 1.25 1.24 1.26 1.23 | 22,248 | 327 | 84.18% | 71.03% |
| `ocrs` 0.12.2 | — | — | 249 | 81.12% | 71.30% |

**More scales is not better and uniform spacing is not better.** The six-scale
uniform ladder carries 20% more prototypes and costs 26% more time than the
five-scale uneven one, and loses 1.12 points of word F1 to it.

What the four rows are consistent with is that spacing should be **tight at the
bottom and loose at the top**: the winner steps 16→20→24 by a constant 4px and
then 24→32→48 by 8 and 16. Rasterisation error is a roughly fixed number of
pixels, so a fixed *pixel* step is a larger fraction of the shape at small em
sizes and a smaller one at large — which argues for arithmetic spacing where
glyphs are small and geometric where they are big. That is a hypothesis with
four points behind it, not a result.

**Decision: the scale search stops here, and the remaining question goes to
`ocrcer-glyphs` with a constraint rather than an answer.** Four bank
compositions have now been ranked *by the benchmark corpus*, and continuing to
iterate that way fits the bank to the test set no matter how principled each
individual step sounds — the corpus renders at 14/18/21/28/40 and any search
long enough will find the ladder those five numbers prefer. The constraint:

1. The shipped ladder is stated as a **spacing rule** with a sentence behind it
   (`CLAUDE.md` rule 1), not as five numbers that scored best.
2. It is confirmed on **render sizes the corpus does not contain** before it is
   authored into `model/fonts.tsv`. `ocrcer-bench` owns generating those pages;
   this is the held-out check that the ranking above cannot provide for itself.
3. Cost is part of the choice, not an afterthought — 22,248 prototypes at 327
   seconds is a worse engine than 18,540 at 258 on this evidence, and section
   9's projections were written against a ~12,000-prototype bank.

**Where this leaves the head-to-head.** The best measured configuration is
16/20/24/32/48 with dims `103..107` weighted x16: **85.30% layout-free word F1
against `ocrs`'s 81.12%**, ahead at every render size in the corpus. Neither
number in that sentence is authored yet — the weight is a swept candidate that
worsens `0` against `O`, and the ladder is a corpus-ranked choice awaiting a
held-out confirmation. And OCRcer's side is still oracle-segmented, so
`PLAN.md`'s hand-off gate remains unmet.

### 2026-09-21 — Held-out sizes confirm the ladder's *direction* and shrink its margin by 60%; the "ahead at every render size" sentence does not survive

The entry above made the shipped ladder conditional on a check against render
sizes the benchmark corpus does not contain. That check has been run. The
held-out corpus is the same authored text and the same faces rendered at
**15/19/22/30/44 px/em** — five sizes chosen to coincide with neither the
benchmark corpus (14/18/21/28/40) nor any bank scale under consideration
(16/20/24/25/31/32/39/48) — and it scores the same 125,815 characters, so the
only variable against `bench/pages/` is render size. Raw output:
`docs/measurements/2026-09-21_holdout_w16_bank_base.txt` and
`..._holdout_w16_bank20.txt`.

| bank scales | prototypes | corpus F1 | **held-out F1** | corpus smallest | **held-out smallest** |
|---|---|---|---|---|---|
| 16/24/32/48 | 14,832 | 81.02% | 82.05% | 66.20% @14 | 68.53% @15 |
| **16/20/24/32/48** | **18,540** | **85.30%** | **83.69%** | **73.19% @14** | **71.71% @15** |
| margin | — | **+4.28** | **+1.64** | **+10.61** | **+3.18** |

**The ranking reproduces and the margin does not.** Halving the widest step at
the bottom of the ladder is still the better bank on sizes it was not chosen
on — that is the result the constraint was asking for, and it holds. But 4.28
points of word F1 on the corpus is 1.64 held out, and 10.61 points at the
smallest size is 3.18. **Roughly 60% of the measured gain was specific to the
five render sizes the ladder was ranked against.** The 85.30% figure in the
entry above is therefore a corpus-flattered number, and 83.69% is the better
estimate of what the same bank does on a size it has not been tuned toward.

**And the claim that OCRcer is ahead of `ocrs` at every render size is wrong.**
It was true on the benchmark corpus at all five sizes. On the held-out sizes it
is false at three of five:

| px/em | OCRcer F1 | `ocrs` F1 |
|---|---|---|
| 15 | 71.71% | **75.00%** |
| 19 | 81.10% | **81.77%** |
| 22 | 83.04% | **85.32%** |
| 30 | **89.94%** | 86.16% |
| 44 | **92.66%** | 85.19% |

Overall the held-out head-to-head is **83.69% against 82.72%** — a 0.97-point
lead where the corpus said 4.18. OCRcer still wins the aggregate and still wins
decisively on large text, and it is still oracle-segmented while `ocrs` is end
to end, so none of this is a shipping claim. But "ahead everywhere" was an
artefact of which sizes got measured, and it is withdrawn.

**One observation worth chasing, not yet explained.** OCRcer scores *worse* at
15 px/em (71.71%) than at 14 px/em (73.19%) while `ocrs` moves the expected way
(71.30% → 75.00%). Accuracy that is non-monotone in resolution is not what a
scale-normalised feature vector should produce, and the most likely candidates
are rasterisation interacting with the 32×32 normalisation grid at particular
em sizes, or the bank's own scales sitting at unlucky ratios to 15. Two points
is not a phenomenon; it is a reason to sweep integer render sizes 12–24 at
fixed bank and look at the curve. That sweep belongs to `ocrcer-bench` and is
not a bank-composition question.

**What this changes.** Nothing about the constraint in the entry above: the
ladder is still to be authored as a spacing rule with a sentence behind it
rather than as five numbers that scored best, and `ocrcer-glyphs` still owns
that. What it changes is the number that rule has to beat — **83.69%, not
85.30%** — and the expectation of how much a ladder change is worth, which on
this evidence is one to two points of word F1, not four.

### 2026-09-21 — Accuracy as a curve in render size: it spikes 13 points wherever the page size *equals* a bank scale. The feature vector is not as scale-invariant as section 3.1 intends

The entry above asked for a sweep of integer render sizes at a fixed bank,
because OCRcer scored worse at 15 px/em than at 14. The sweep was run: same
authored text and faces at every integer size from 12 to 24 px/em, 1,716 pages,
327,119 characters, bank 16/20/24/32/48 with dims `103..107` weighted x16. Raw
output: `docs/measurements/2026-09-21_size_sweep_w16_bank20.txt`.

| px/em | OCRcer word F1 | `ocrs` word F1 | nearest bank scale |
|---|---|---|---|
| 12 | 48.16% | 51.75% | 16 |
| 13 | 56.12% | 62.65% | 16 |
| 14 | 73.19% | 71.30% | 16 |
| 15 | 71.71% | 75.00% | 16 |
| **16** | **94.69%** | 76.69% | **exact** |
| 17 | 79.38% | 79.85% | 16 |
| 18 | 81.85% | 79.83% | 20 |
| 19 | 81.10% | 81.77% | 20 |
| **20** | **94.55%** | 81.66% | **exact** |
| 21 | 86.95% | 82.67% | 20 |
| 22 | 83.04% | 85.32% | 20/24 |
| 23 | 87.25% | 85.15% | 24 |
| **24** | **97.88%** | 85.61% | **exact** |

**The 14-versus-15 anomaly was the small part of a much larger effect.** The
curve is not a curve. Superposed on a smooth rise with resolution — which
`ocrs` shows and which is what more pixels should buy — is a spike of **10 to
15 points of word F1 at exactly the three sizes the bank was rendered at**, and
nowhere else. A page at 16 px/em scores 94.69%; one at 17 px/em, with *more*
ink to work with, scores 79.38%.

**What this means about the feature vector.** Section 3.1 normalises every
glyph to a 32×32 grid precisely so that a prototype rendered at 24 px/em can
match a glyph drawn at 17. It substantially does not: when the page's
rasterisation is bit-identical to the prototype's, the match is near-perfect,
and any other size pays 10–15 points. The residual the normalisation leaves
behind — which pixels the rasteriser chose to turn on at that particular em
size — is carrying far more of the distance than intended. **This is a
statement about a measured behaviour of the extractor, not a proposal to change
it**; no feature-vector change is authorised here, and the charset, the 107
dimensions and the `.ocrw` version are untouched.

**Three things follow, and none of them is "add more scales" yet.**

1. **Neither headline number is contaminated.** The benchmark corpus renders at
   14/18/21/28/40 and the held-out corpus at 15/19/22/30/44; the bank is at
   16/20/24/32/48. There is **no coincidence between any corpus size and any
   bank scale**, so 85.30% and 83.69% are both off-spike figures. Had a corpus
   size landed on a bank scale, that slice would have been worth a false 13
   points, and nothing in the report would have said so. Any future corpus or
   ladder change has to be checked for that collision explicitly.
2. **The ladder question changes shape.** "Tight at the bottom, loose at the
   top" was a spacing aesthetic inferred from four compositions. What the sweep
   actually shows is that accuracy is a function of *ratio distance to the
   nearest bank scale*, on top of absolute resolution. That is a rule with a
   mechanism behind it, and it is the one `ocrcer-glyphs` should be reasoning
   from — but it also means a denser ladder buys accuracy roughly linearly in
   cost, which section 9's prototype-count projections were not written for.
3. **The corpus's zero-noise property is a bias in OCRcer's favour, and this is
   where it bites hardest.** These pages are hard black-and-white with no
   anti-aliasing, noise, skew or scanner blur, so an exact-size match is
   bit-exact. On a real scan the spike would be blunted — and so, probably,
   would some of the off-spike deficit. The size of that correction is unknown
   and will stay unknown until the corpus has degraded pages in it, which is
   `ocrcer-bench`'s to build.

**Not a benchmark.** The aggregate on this corpus (79.68% against 76.97%) is
not comparable to any other figure in this log: it includes 12 and 13 px/em,
below the distinctness floor, and exists only to make the curve visible.

### 2026-09-21 — The spike is real at every scale and one pixel wide; and the entry above named the wrong mechanism for the ladder gain

Two follow-ups to the entry above, one confirming it and one correcting it.
Raw output: `docs/measurements/2026-09-21_size_sweep_hi_w16_bank20.txt`.

**The exact-match spike is not a small-size artefact.** Sizes either side of the
two largest bank scales, same bank, 924 pages, 176,141 characters:

| px/em | word F1 | | px/em | word F1 |
|---|---|---|---|---|
| 31 | 92.13% | | 47 | 92.61% |
| **32** | **99.03%** | | **48** | **98.48%** |
| 33 | 92.11% | | 49 | 95.39% |
| 36 | 89.79% | | | |

So the effect is present at all five bank scales — 16, 20, 24, 32, 48 — and it
is **one pixel wide**: 31 and 33 px/em score within 0.02 points of each other
and both give back 6.9 points to 32. Its magnitude shrinks with size, roughly
+19 points at 16 px/em, +10 at 20, +9 at 24, +7 at 32, +5 at 48, which is what
a rasterisation residual should do as pixels get cheaper.

**And that width is the decisive fact: the spike cannot be designed for.** A
ladder that put a scale on every integer size from 12 to 24 would need thirteen
scales to catch a 13-point prize that a single pixel of drift destroys, and
real input does not arrive at integer em sizes anyway. The spike is a
diagnostic finding about the extractor, not a lever.

**The correction.** The entry above said accuracy is "a function of ratio
distance to the nearest bank scale" and handed that to `ocrcer-glyphs` as the
rule to reason from. Off the spike, evidence already in this log says it is
not:

- 36 px/em sits ln(1.125) from a bank scale and scores 89.79%; 40 px/em sits
  ln(1.25) from one — **further** — and scores 93.38%. Ratio distance predicts
  the wrong order; absolute resolution predicts the right one.
- Adding a 20 px/em scale to 16/24/32/48 gained **+6.99 points at 14 px/em**,
  where the nearest scale is 16 both before and after and the covering distance
  is **identical**. It gained +6.52 at 18 px/em, where the covering distance
  barely moved. It gained nothing at 28 (−0.29) and 40 (+0.70).

What those two facts are consistent with is that the ladder gain is **prototype
density in the small-size regime**, not proximity. A 14 px/em glyph is matched
against the whole bank, and what decides the vote is whether the *correct*
class has a candidate whose rasterisation character is close to the page's —
not merely whether some scale is nearby. Adding scales just above the
distinctness floor adds correct-class candidates in the regime where the
rasterisation residual is largest; adding them at 28–48 adds nothing, because
the residual there is already small. Adding them *below* the floor adds
wrong-class attractors, which is the 12 px/em result three entries up.

**What `ocrcer-glyphs` should reason from, restated.** Not "cover log-size
space evenly" and not "cover it at all" — **concentrate scales just above the
measured distinctness floor and thin them out as the rasterisation residual
falls, and expect the benefit to saturate somewhere around 24–28 px/em.** That
is still a rule needing a sentence and a held-out confirmation before it is
authored into `model/fonts.tsv`; what has changed is that it now has a
mechanism behind it rather than a curve shape.

**One opportunity, untested, recorded so it is not lost.** In the `pdfcer`
case the caller chooses the rasterisation DPI, so the page's em size in pixels
is a free variable — measure the dominant text height on a first pass and
re-rasterise so it lands on a bank scale. On this evidence that is worth
somewhere between 5 and 19 points of word F1, for the cost of a second
rasterisation. Two reasons it is recorded rather than adopted: it can only help
input that is *rendered* rather than *scanned*, and the whole effect was
measured on noise-free synthetic pages where an exact size match is bit-exact.
On a real scan there is no reason to expect a spike at all. It belongs to
chunk 7, after the end-to-end pipeline exists to try it on.


### 2026-09-22 — The charset's aspect band and the aspect feature are different quantities, and that is the 49.22%

The 2026-09-21 entry above retired the authored aspect band as a hard prune
after it scored 49.22% top-1 against 91.40% unpruned, and explained it as the
band being *too tight*. That explanation is superseded. The band is in a
different denominator from the feature it was being compared against, and the
arithmetic of that mismatch accounts for the number almost exactly.

**Two consumers, two denominators, both in the source.** The face's
`aspect_report` computes `r.width / cap_px` — width over cap height. The bank's
`ClassGate` compares `charset.tsv`'s band against `f[103]`, which section 3
defines as `(w - h) / (w + h)` over a bitmap cropped tight to its ink — width
over *that mark's own* height. For a capital or a digit the two agree, because
the ink spans the cap line. For a hyphen one pen tall they are a factor of ten
apart, and for a round period width over its own ink height is `1.0` by
construction and carries no information at all.

**Measured.** `fontTools` over the 19 shippable faces in `model/fonts.tsv` that
are present on this machine: ink bounding box from the outline, cap height from
`OS/2.sCapHeight` (falling back to the `H` bounding box), every charset
codepoint the face carries — 3,521 class/face pairs.

| Reading of the authored band | Pairs inside it |
|---|---|
| width / own ink height — what the matcher compares | **49.30%** |
| width / cap height — what the face test compares | **64.16%** |

49.30% against a measured 49.22% top-1 under that prune. The gate was not
rejecting the correct class because the band was narrow; it was rejecting it on
precisely the classes where the two quantities disagree.

| Baseline class | Pairs | In band, ink-height reading | In band, cap-height reading |
|---|---|---|---|
| `ascender` | 1842 | 56.95% | 59.55% |
| `xheight` | 665 | 52.48% | 80.30% |
| `above` | 522 | **13.79%** | 59.58% |
| `descender` | 209 | 76.56% | 60.29% |
| `full` | 207 | 51.21% | 65.22% |
| `low` | 76 | **0.00%** | 73.68% |

The split falls exactly where the geometry says it must. Classes whose ink
reaches the cap line are barely affected; `above` and `low` — the flat marks —
are wrong by the whole cap-to-stroke factor. `_`, `.`, `·`, `-`, en dash, em
dash, `~`, `` ` ``, `,` and `"` are in band **zero times out of nineteen** under
the matcher's reading and in band under the other.

**Decision: the column means width over cap height, and the conversion in
`tables::load_charset` is the defect rather than the band.** Three reasons, in
order of weight. It is the denominator the values were authored in — the face
source documents `·` as aspect `0.10` and `•` as `0.30`, and both are discs
whose width over their own ink height is `1.0`. It is what the only working
consumer compares. And it is the form section 2 wants the column in at all: a
number a person can read off a drawing and argue with. The reverse fix is not
available — converting the band into the feature's denominator at load time
needs the glyph's ink height, which is the quantity being measured, and doing
it with a per-face cap-to-x-height ratio would be exactly the invented constant
`CLAUDE.md` rule 1 forbids.

**Nothing in the model file moves.** No feature dimension changes meaning, no
class index shifts, no normalisation constant changes. So there is no `.ocrw`
`version` bump, no `meta` extractor-version change and no bank rebuild — the
cheapest outcome the section 11 protocol allows, and the reason to check for it
before reaching for a structural change. The conversion is inert today: nothing
reads `aspect_min`/`aspect_max` except the `declared` gate mode, which
2026-09-21 already took out of the pipeline.

**What it costs.** Fixing the units does not give the prune back. Even read in
its own denominator the band holds for 64.16% of real-face glyphs, because it
was authored against one drawing face and section 4.1's gate may only exclude a
class when exclusion is certain. A band that could gate has to be authored or
measured in the extractor's own quantity over a dense size sweep — the open
item the 2026-09-21 entry already names. What this entry adds is that its
starting numbers cannot be got by converting this column.

**Work this hands to others, not done here.** `ocrcer-build` to drop the
`r -> (r - 1) / (r + 1)` conversion in `tables::load_charset`, to name the
fields for the denominator they hold, and to stop `ClassGate` comparing them
against `f[103]`. `ocrcer-glyphs` owns any re-authoring of the bands
themselves; every row is still `authored-provisional` and stays that way.

**Caveat on the measurement.** These are outline bounding boxes in font units,
not rasterised tight crops at a render size, so an individual class moves by a
pixel of quantisation at small sizes. The comparison between the two readings
does not depend on that: it is a ratio of heights, and the heights differ by
design, not by rounding.


### 2026-09-22 — U+2212 is declined as a charset class: in three of nineteen shippable faces it is the same outline as the hyphen

`ROADMAP.md` carries the true minus sign as a gap for chunk 9 — financial
documents use it for a negative number, and `model/charset.tsv`'s 187 classes
have only U+002D. The decision is not to add it, and the reason is a
measurement rather than a cost.

**Measured.** `fontTools` over the same 19 shippable faces, ink bounding box
from the outline, vertical centre as a fraction of cap height:

| | hyphen U+002D | minus U+2212 |
|---|---|---|
| width / height, range | 2.74 – 5.49 | 4.00 – 9.34 |
| ink centre / cap height, range | 0.360 – 0.505 | 0.391 – 0.507 |

The central tendency is real: in most faces the minus is wider and sits higher,
level with the bar of the plus sign. The tails are what decide the question.

- **JetBrains Mono, Cascadia Code and Cascadia Mono draw them as the same
  outline** — identical width, height, x origin and y origin in font units.
  Not similar: identical. Every feature vector in section 3 is bit-identical
  for the two, at every render size, by construction.
- **Fira Code draws the minus *narrower* than the hyphen** (4.86 against 5.49),
  so even the direction of the cue reverses.
- The ranges overlap on both cues. Inconsolata's hyphen sits at 0.505 of cap,
  higher than Inter's minus at 0.391.

**So no authored rule separates them.** Section 4.1's confusion machinery works
on a rule plus a threshold — the thing `CLAUDE.md` rule 1 requires a sentence
for. Here the only sentence available is "the minus is wider and higher, in
faces that distinguish them at all, by an amount that depends on the face",
and the engine does not know the face. A threshold that is right for Liberation
Serif and wrong for JetBrains Mono is not a threshold; it is a coin toss
written down.

**Cost was not the deciding factor, and the roadmap's framing of it is worth
correcting.** `charset.tsv` is grouped by category, so the natural home for
U+2212 is inside the punctuation run and that shifts 60 class indices;
appending at index 187 instead shifts none. Either way it is a `version` bump,
a `meta` extractor-version bump, a bank rebuild and a re-run of the collision
sweep against the new class — minutes of compute, per section 4. The reason to
decline is that the class cannot be recognised, not that adding it is
expensive.

**Where the requirement actually belongs: chunk 9, as semantics rather than
shape.** Whether a dash in front of a number is a minus sign is decided by what
the number is — a currency amount in a debit column, a dimension, a tolerance —
and that is a question the structured numeric layer can answer from context.
The glyph cannot answer it at all in three of nineteen faces, because the
document's author and the font's designer between them did not encode it.

**What this costs, stated plainly.** OCRcer will emit U+002D where the source
document contained U+2212. Against ground truth that preserves U+2212 that is a
character error, so chunk 11's ground-truth normalisation has to fold
U+2212 to U+002D — and the benchmark report has to say it does, because it is a
normalisation that favours this engine. `ocrcer-bench` owns that; it is
recorded here so it cannot arrive silently.

**One thing this measurement found that is not about U+2212.** In Cascadia Code
and Cascadia Mono the hyphen and the en dash differ by 6% of width
(`w/h` 5.38 against 5.70) and sit at the same height, because a monospaced face
gives every mark the same advance. That pair is a real confusion candidate in
monospaced faces and belongs on `ocrcer-linguist`'s confusion list, where a
context rule can help — an en dash between two numbers in prose, a hyphen
inside an identifier. Unlike the minus it is at least sometimes separable.

### 2026-09-22 — The accuracy curve refines the resolution tiers: the cliff is at 13/14 px per em, well below the 21 px collision floor

Closes `ROADMAP.md` open question 5. The 2026-09-21 resolution entry set three
tiers from a *collision* measurement — an exhaustive pairwise sweep that found
glyph pairs rasterising identically below 21 px/em and none from 21 to 40. The
size sweep run afterwards measures something different, aggregate accuracy per
size, and the two do not say the same thing.

Off-spike sizes only, from the two sweep files (bank at 16/20/24/32/48; the
five bank scales are omitted because section 11's spike entry shows they are
worth 5–19 points that no real input collects):

| px/em | char accuracy | word F1 | `ocrs` word F1 |
|---|---|---|---|
| 12 | 83.19% | 48.16% | **51.75%** |
| 13 | 87.22% | 56.12% | **62.65%** |
| 14 | 92.70% | **73.19%** | 71.30% |
| 15 | 91.92% | 71.71% | **75.00%** |
| 17 | 94.39% | 79.38% | **79.85%** |
| 18 | 95.50% | **81.85%** | 79.83% |
| 19 | 95.34% | 81.10% | **81.77%** |
| 21 | 96.68% | **86.95%** | 82.67% |
| 22 | 96.17% | 83.04% | **85.32%** |
| 23 | 97.03% | **87.25%** | 85.15% |

**There is no cliff at 21 and none at 23.** Accuracy falls smoothly from 23 down
to 14 — about 1.2 points of word F1 per pixel — and then drops 17 points between
14 and 13. That is the cliff, and it is seven pixels below where the tier
boundaries sit.

**This does not move the boundaries, and the reason matters.** A collision is
not a low score; it is a pair of classes that *cannot* be told apart, whatever
the decoder does. Two pairs out of 17,391 are invisible in an aggregate over
125,815 characters — the absence of a cliff at 21 is not evidence against the
collision floor, it is evidence that the aggregate cannot see it. The tiers
stay where they are, and they stay confidence caps rather than refusals, per
rule 5 and the operator's direction on that entry.

**What the curve does add is a fourth tier, and the number that sets it is the
incumbent.** Below 14 px/em this engine is worse than the one it is replacing —
48.16% against 51.75% at 12 px/em, 56.12% against 62.65% at 13 — and the
comparison is oracle-segmented in OCRcer's favour, so the real gap is wider
than that. From 14 px/em up the two trade slices. That crossover is a property
worth declaring, because it is the one place where the honest answer to
"should this engine run" is no.

| Effective px per em | Behaviour |
|---|---|
| >= 23 | Normal operation; confidence reported as calibrated. |
| 21–23 | Recognised; confidence capped. |
| 14–21 | Recognised; confidence capped harder. |
| < 14 | Recognised, confidence capped hardest, **and the result carries a flag saying the input is below the range where this engine is competitive.** |

The cap values remain chunk 8 tuning parameters in the `params` block, not set
here, and the new boundary is measured rather than chosen: 14 is where OCRcer
passes `ocrs` on this corpus, and 13 is where both fall apart.

**In the operator's units.** `px_per_em = points * dpi / 72`, so 14 px/em is
6 pt at 168 dpi, 8 pt at 126 dpi, 10 pt at 101 dpi. The practical reading is
unchanged from the 2026-09-21 entry — 300 dpi for drawing annotation, 200 dpi
for body text — and what is new is that the floor below which the engine should
say so out loud is roughly 100 dpi for body text and 170 dpi for 6 pt
annotation. For CAD specifically, ISO 3098 lettering height *is* cap height, and
the authored face draws cap at 0.70 em, so 2.5 mm lettering — the smallest ISO
size — clears 21 px/em at 150 dpi and 14 px/em at 100 dpi.

**Provisional, and for a reason that is not hedging.** Every figure here is
oracle-segmented on noise-free synthetic pages. Segmentation degrades faster
than matching at low resolution: touching characters and broken strokes are a
small-text failure mode this measurement cannot see at all, because it was
handed the boxes. So these boundaries are a **lower bound** on the right ones,
and chunk 2 landing is what turns them into a measurement of the product.

### 2026-09-22 — Hand-lettered block capitals stay out of v1, and the path in is more prototypes rather than a redesign

Closes `ROADMAP.md` open question 6. It is a fair question rather than a scope
creep, which is why it gets an answer rather than a citation: hand lettering on
older drawings is squarely inside the target domain, unlike cursive or scene
text, and ISO 3098 — the standard the authored face implements — is itself a
*hand*-lettering standard. The shapes are upright, unconnected, isolated and
drawn to a template. That is the easiest case a prototype matcher could be
asked for.

**Out of scope for v1 anyway**, under `FEASIBILITY.md` section 6 condition 1
and rule 7. The tractability argument for this whole project is that the shape
space is enumerable by rendering fonts. A pen introduces variation a font
cannot: stroke width that varies along a stroke, baseline wander, counters left
unclosed, aspect that drifts across a word. None of that is in the bank, and
none of it arrives by adding a font family.

**No claim is made that it currently fails.** There is no hand-lettered corpus
on this machine and none has been scored. The decision is about scope and about
what the bank contains, not about a measured deficit — recording it the other
way round would be exactly the drift rule 8 is written against.

**The path in, costed so a later session does not have to rediscover it.**
Deterministic perturbation of the authored ISO 3098 face — a script that
jitters stroke width, endpoint position and baseline within stated bounds, and
emits the perturbed renders as additional prototypes for the *same* classes.
That is a script anyone can re-run for the same bytes, so it satisfies rule 1,
and it is not a training run. It changes no class index, no feature dimension
and no normalisation constant, so it needs no `version` bump — it is a bank
rebuild and nothing else, which section 4 measures in minutes.

**The condition for spending it:** evidence from `pdfcer`'s real drawing corpus
that hand lettering appears in material volume. That is a measurement chunk 11
can make and this session cannot. Until then the bank stays fonts-only, and the
same bank-extension mechanism in the 2026-09-21 segmentation entry is what lets
a user add hand-lettered prototypes on their own machine without a model
rebuild.


### 2026-09-22 — `0` against `O` is not a weighting problem: in monospaced faces the aspect feature separates them by less than one pixel

The open question left by the 2026-09-21 weighting entry was whether a
*per-dimension* weight — lifting feature 103 alone rather than the group of
four — could repair `0` against `O`, the pair a group weight made 71% worse and
the one rule 6 forbids the lexicon to fix. The answer is no, and the reason is
that the dimension does not carry the pair in the faces where the pair matters.

**Measured.** Feature 103 as section 3 defines it, computed from outline bounds
over the 19 shippable faces. Difference between the two members of each pair:

| Pair | min | median | max | mean absolute |
|---|---|---|---|---|
| `0` vs `O` | −0.2043 | −0.1067 | +0.0172 | 0.0964 |
| `1` vs `l` | −0.1822 | +0.1851 | +0.5542 | 0.2156 |
| `1` vs `I` | −0.1936 | +0.1009 | +0.5318 | 0.1968 |
| `l` vs `I` | −0.3233 | −0.0114 | +0.2214 | 0.0635 |
| `5` vs `S` | −0.0878 | −0.0303 | +0.0217 | 0.0336 |
| `2` vs `Z` | −0.1198 | −0.0531 | +0.0696 | 0.0566 |

**The yardstick is one pixel, and it has to be computed rather than guessed.**
Feature 103 comes from the source tight-crop bitmap, not from the 32×32 grid,
so a pixel of it is a pixel at the render size. At 24 px/em a cap-height glyph
is about 16.8 px tall and a zero about 13 px wide, and `d/dw` of
`(w - h) / (w + h)` is `2h / (w + h)²` — **0.038 per pixel of width**.

**Read against that, the pair splits by face class.**

| Faces | `0` vs `O` separation | In pixels at 24 px/em |
|---|---|---|
| Liberation Serif, Liberation Sans, Noto Sans, Noto Serif, Lato, STIX Two Math | 0.142 – 0.204 | 3.7 – 5.4 px |
| Roboto, Roboto Condensed, Inter, PT Sans, Open Sans Condensed | 0.106 – 0.121 | about 3 px |
| Fira Code, Inconsolata, Cascadia Code/Mono, Roboto Mono | 0.036 – 0.056 | 0.9 – 1.5 px |
| **Liberation Mono, PT Mono, JetBrains Mono** | 0.015 – 0.022 | **0.4 – 0.6 px** |

JetBrains Mono also reverses the sign: its `0` is marginally *wider* than its
`O`. A monospaced face gives both marks the same advance and the designer fills
it, which is exactly why the separation vanishes there.

**So the decision: `0` against `O` is not the justification for a
per-dimension weight.** Lifting feature 103 buys three to five pixels of
separation in the proportional faces, where the pair is already the easier case,
and buys nothing at all in the monospaced faces where identifier-shaped text —
part numbers, drawing callouts, `M8x1.25` — actually lives. A weight cannot
amplify a difference that rasterisation has already destroyed. The
per-dimension weight vector stays open as an experiment for `ocrcer-glyphs`,
and the bar section 4.1 sets for it stands, but a candidate has to be judged on
`0`/`O` **in monospaced faces specifically** rather than on the pair's
aggregate, which the proportional faces dominate.

**What the pair gets instead**, none of it new architecture: the hole-count
route already recorded (a slashed zero carries hole count 2 and prunes into
`8`'s bucket), the counter-to-stroke ratio section 2 names, and — where those
do not resolve it — rule 5. Two classes inside one pixel of each other *should*
report low confidence. That is the honest output and the one a reviewer can act
on; forcing a pick and reporting it as certain is the failure rule 5 exists to
prevent.

**A second finding, for `ocrcer-linguist` rather than for weighting.** For
`1`/`l` and `1`/`I` the separation is large — 0.20 and 0.22 mean absolute, five
or six pixels — but **the sign flips across faces**: Lato's `1` is far wider
than its `l` (+0.5297), Roboto Mono's is narrower (−0.1822). So aspect can say
these two are *separable* while saying nothing about *which is which*. A
confusion rule keyed on "the narrower one is the `l`" is face-dependent in
exactly the way section 11's U+2212 entry describes, and the direction has to
come from the cues section 2 already names for this family: serif presence and
baseline contact. `5`/`S` at 0.034 mean absolute is under one pixel and is a
pure shape pair; aspect has nothing to say about it in any face.

**Caveat.** Outline bounds in font units, not rasters, so these are the
separations before pixel quantisation rather than after. Quantisation can only
reduce them, which is the direction that matters for the conclusion.


### 2026-09-22 — Cascadia Code and Cascadia Mono are the same outlines: one comes out of the bank, and out of the corpus

`model/fonts.tsv` listed both as `shippable` and every benchmark to date
printed them as two rows that agreed to the last digit. They agreed because
they are the same measurement twice.

**Measured** (`fontTools`, outline units; and `cmp` over pages already on
disk). Over the 187 charset classes the two files draw **186 identical
outlines at identical advances**, and the 187th — U+2300 — is absent from
both. Every SFNT table the rasteriser touches is **byte-identical**: `glyf`,
`loca`, `hmtx`, `cmap`, `cvt`, `fpgm`, `prep`, `gasp`, `hhea`, `maxp`, `post`.
So is every metric the extractor reads, `sxHeight` and `sCapHeight`
included. The tables that differ are `GSUB`, `name`, `OS/2`'s panose byte,
`head`'s checksum and `DSIG`. The 35 rendered benchmark pages per corpus were
**byte-identical, all 70 of them**.

`GSUB` — Cascadia Code's programming ligatures — is the entire substantive
difference, and section 4 builds the bank one isolated codepoint at a time
with no shaping, so it cannot reach the bank. The pair is one face wearing two filenames.

**Decision: Cascadia Code's `distribution` becomes `excluded`; Cascadia Mono
stays `shippable` and is the pair's single representative.** Mono is kept
because it is the member whose own design carries no ligature layer, which is
what we actually render. `excluded` rather than `local-only` because
`local-only` means "built on the end user's machine", and a `--local` build
would reintroduce the duplicate — and because `ocrcer-build pages` gates the
corpus on the same column, so one change removes the duplicate from the bank
and from every corpus at once.

`excluded` has until now only ever meant "the licence does not permit it", and
this is the first row where it does not. **Cascadia Code is excluded for
redundancy, not for licence** — its OFL-1.1 finding and its `eligible-present`
status both stand unchanged, and the `notes` column carries the reason so the
two cases cannot be confused. The `Distribution::Excluded` doc comment in
`tables.rs` said "not usable at all" and has been narrowed to say which of the
two reasons applies, per the `notes` column. `status` and `distribution` are
orthogonal axes and this is simply a cell that had never been used; no schema
change was needed and none was made.

**What it cost to have carried the duplicate.** Two separate costs, and they
are different in kind.

*In the bank*: 930 of 18,540 prototypes at five sizes — **5.01%** — were
bitwise duplicates. At the four-size shipped configuration the file goes from
14,832 prototypes and 1.55 MB to 14,088 and 1.48 MB (744 = 186 × 4, as
predicted). Match time over the held-out corpus fell 9.8%.

*In what was reported*: the per-face table's two Cascadia rows were one
result printed twice, and the corpus aggregate counted the Cascadia design
twice at 98.00% — well above the corpus mean. Removing the duplicate pages
with the bank untouched moves the held-out headline from 95.88%/83.69% to
**95.76%/83.26%**, and `ocrs` from 77.74%/82.72% to 77.72%/82.72%. The
double-count was inflating OCRcer by 0.12 points of character accuracy and
0.43 of word F1, and `ocrs` by 0.02 and nothing. **It ran in OCRcer's
favour**, which is the direction that has to be said out loud in a
head-to-head. Every individual face's own score was unaffected, to the last
reported digit.

That last point corrects the entry that first spotted the duplicate. The
2026-09-21 head-to-head entry recorded the byte-identical pages and said the
effect was "small and in neither engine's favour" — small is right, neither
engine's favour is not. It was asserted rather than measured; it has now been
measured and it favoured OCRcer by about twenty to one.

**Section 9's opening figure is amended to the post-change measurement**:
95.73% / 83.15% against 77.72% / 82.72%, over 625 held-out pages. The
2026-09-21 held-out entry is not rewritten, per this log's append-only
convention; it is read subject to this one.

**The finding underneath, which matters more than the duplicate.** Removing
930 bitwise-duplicate prototypes should have been free, and was not: the
held-out figure fell a further 0.03 points of character accuracy and 0.11 of
word F1, and four unrelated faces moved — PT Mono by **1.83 points of word
F1**, Open Sans Condensed *upward* by 0.33. Cascadia Mono itself did not move
at all.

The mechanism is not in doubt, because only one thing could have changed.
Section 4's matcher is 1-NN over squared L2 on standardised features; under a
fixed metric, deleting a prototype bitwise identical to one that remains
cannot change the winner, and cannot change the margin either — the margin is
to the nearest prototype of a *different* class, and the duplicates were the
same class as the rows that stayed. So the **metric** changed. The
standardisation constants are the per-dimension mean and standard deviation
over every prototype in the bank, so **bank composition sets the scale of all
107 dimensions**, and dropping 5% of the population re-weighted every one of
them.

Three consequences, none of which is a format change:

1. **A per-face accuracy figure is not a property of that face alone.** It is
   a property of the face *and* the rest of the bank. PT Mono lost 1.83 points
   because a face it has nothing to do with left the bank.
2. **Any A/B that adds or removes a face measures two things at once** — the
   new prototypes, and the re-tilted metric. `ocrcer-bench` has to report
   bank composition alongside any such comparison, and must not read a small
   aggregate movement as the new face helping or hurting recognition of the
   others. This bears directly on the pending bank-composition work: adding
   `norm-stroke`, or adding families for coverage, will move every face's
   number a little for this reason alone.
3. **It is not a reason to keep the duplicate.** Keeping a face because its
   presence happens to tilt the normalisation favourably is choosing bank
   composition to move a benchmark number, which is what `CLAUDE.md` rule 1
   exists to forbid: "the duplicate stays because it was worth 0.03 points" is
   not a sentence anyone can argue with.

**Left open, deliberately.** Whether the standardisation constants should stop
being a population statistic over an arbitrarily-composed bank and become
authored values instead is a real question, and this entry does not answer it.
It is not a correctness hazard — the constants ship inside the model file
alongside the bank they were computed from, so a mismatched pair is impossible
by construction, which is exactly what section 7's design is for. It is a
coupling, and a 0.03-point observation is not grounds for changing a format.
Recorded here for whoever revisits bank composition; `ocrcer-glyphs` and
`ocrcer-exporter` own the question if it is ever taken up.

**One thing not to do with the excluded face.** Cascadia Code must not be
pressed into service as a held-out face now that it is out of the bank. Its
score would look like evidence that the bank generalises to an unseen face,
and it is nothing of the kind — the "unseen" face draws pixel-for-pixel what
an in-bank face draws.

**The change exposed a second defect, found by checking the claim instead of
asserting it.** This entry says one `distribution` change removes the face from
the bank and from every corpus at once. That was only half true when written.
`Fonts::load` reads the column correctly — `excluded` is skipped whatever the
flags say. The corpus generator did not: it tested `!local && distribution !=
shippable`, so a `--local` run skipped nothing at all and happily re-rendered
the excluded face. Verified by running it: with `--local`, cascadia-code pages
came back.

Nothing was leaking. The proprietary rows in `fonts.tsv` are all
`status: ineligible`, and `FontEntry::file` returns `None` for those, so the
weaker test never reached them — the licence posture held, by a different
guard than the one that looked like it was holding. What slipped through was
the newly-valid combination this entry created: `eligible-present` *and*
`excluded`.

The underlying fault is `CLAUDE.md` rule 4 in miniature: "which faces may this
build read" was implemented twice, in two files, and the two disagreed. Fixed
by making it one predicate — `Distribution::usable(include_local_only)` in
`tables.rs` — which both `Fonts::load` and the corpus generator now call. Both
`--local` and plain runs re-verified afterwards; the workspace suite is green.
This is an architect-made edit to `ocrcer-build`, made rather than handed off
because it is three lines and the alternative was leaving this entry asserting
a behaviour the code did not have. It is flagged here for that crate's owner to
review rather than presented as their work.

**Caveat.** The accuracy figures here, as everywhere in this document, are
OCRcer under oracle segmentation and oracle spacing against `ocrs` end to end.
They are a ceiling on a future end-to-end result, not a result.


### 2026-09-22 — The diameter sign is carried by two of eighteen faces, because half the families that draw it are CAD-vendor faces this project may not read

Measured today, and the full table is in
`docs/measurements/2026-09-22_charset_coverage_by_face.txt`.

**Coverage is excellent everywhere but one class.** Of the 187 charset classes,
177 are drawn by all eighteen licence-clean font files in the bank, three by
seventeen, six by sixteen — and one by two. Every category except `symbol` has
a minimum of at least sixteen covering faces; `symbol`'s minimum is **2**, and
it is `⌀` U+2300. Nine of the ten thin classes are thin because of a single
narrow-coverage monospace, Inconsolata, which also happens to be the one face
missing both dashes — worth knowing beside the `-` / `–` pair already on the
confusion list. The arithmetic reconciles with the shipped file exactly: 31
uncovered (class, face) pairs × 4 sizes = 124 absent rows, and 187 × 19 × 4 −
124 = 14,088, the prototype count the builder reports.

**The gap is structural, not an inventory oversight, and that is the finding.**
Scanning 1,454 font files across eleven directories on this machine turns up 26
faces in twelve families that draw U+2300. Three families are licence-clean.
Three are Microsoft's. **Six are CAD-vendor drafting faces** — Dassault's
DS ISO 1, Autodesk's GENISO, ISOCPEUR and ISOCTEUR, GOST Common, Myriad CAD.
The codepoint that matters most in this project's target domain lives almost
entirely in the faces `CLAUDE.md` rule 2 forbids it to render from. That will
not improve by scanning harder, and it is the clearest illustration yet of why
the authored ISO 3098 technical face exists: where the shapes cannot be read,
they are authored. Rule 1 applied to font coverage rather than to a threshold.

**The cheap fix exists but is thin, and it is a hand-off, not a decision made
here.** `FEASIBILITY.md` §5's font-coverage risk and `PLAN.md` §4 both say a
coverage gap is answered by adding faces before anything structural is
considered; both have been amended today with what this audit found. Exactly
one licence-clean candidate is on this machine and not already in the bank:
Noto Sans Symbols (OFL-1.1, Google 2013), which covers **6 of 187** classes —
the five math operators and U+2300. `ocrcer-glyphs` owns whether to take it.
Three things go with the question. It should come from Google Fonts, not from
the Android Studio subset the scan found, for the same provenance reason
`norm-stroke` and `osifont` are recorded as upstream fetches. Its diameter sign
measures 0.657 width-over-cap against the class's authored 0.65 floor, and the
class 105 note in `charset.tsv` puts one pixel of width quantisation at about
0.022 of that quantity — so the floor gets re-derived from measurement, not
assumed to survive. And adding it is not free in the way it looks: per the
Cascadia entry above, bank composition sets the standardisation constants, so
an A/B over this face measures the face *and* a re-scaled metric at once.

**What was not measured, said plainly.** Class 95 is 8 of the bank's 14,088
leave-one-out evaluations — 0.057%. The `symbol` category scores 99.52% and no
`Ø`/`⌀` pair appears in the top twenty confusions, but neither figure is
evidence about this class: eight samples is not a measurement, and the tool
prints only the top twenty, so absence from that list is not absence from the
error set. Class 95's own accuracy is unknown.

**A correction to the 2026-09-18 `Ø`/`⌀` entry, which named the right
conclusion and the wrong mechanism.** That entry says the two codepoints are
"drawn identically by design, not by coincidence" in most faces. In the only
two faces that carry both, they are not. `Ø` is a full-height letter crossing
the baseline (Fira Code: ink height 0.936 em, y −0.113..+0.824); `⌀` is a
smaller ring floating entirely above it (0.555 em, y +0.075..+0.630). STIX Two
Math is the same story with different numbers. Sixteen of the eighteen faces
carry only `Ø`, so "most faces" is untestable there rather than true.

The conclusion survives, for a better reason. The cue that separates them is
size and vertical placement **relative to the line** — and §3's feature vector
is normalised per glyph, into the glyph's own box, which discards exactly that.
A `⌀` at 24 px and a `Ø` at 14 px land in nearly the same 107 numbers. The
information is on the page and is not in the vector, so "report the low margin
honestly and leave it to context" remains right; it is a property of the
feature definition, not of the outlines.

**No feature-vector change follows, and the reasoning is recorded so it does
not get re-opened casually.** A dimension carrying ink height over the line's
cap height would separate the pair, and §8.2 already has the segmentation stage
producing baselines and x-heights, so the input exists. It is still declined:
there is no measured accuracy problem to fix, a §3 change costs a `version`
bump, a meta feature-extractor identifier bump and a full bank rebuild, and
§4.1 step 2 — where a height-relative-to-line quantity would naturally live —
is currently **disabled** because both forms of that pruning lost accuracy. The
bar for re-opening this is a measurement showing class 95 or class 94 losing
accuracy to each other on real pages, not the observation that they could.

**Kept.** U+2300 stays in the charset. Two carriers plus the authored technical
face is thin, but a class the charset does not name is a character the engine
cannot emit, and pdfcer's text layer has to emit one codepoint or the other —
the same argument the 2026-09-18 entry used to reject merging the pair, and it
applies unchanged to deleting one of them.

### 2026-09-22 — The model file was audited against section 2, and section 2 was the thing that was wrong

Section 7 exists because a runtime built against one definition and a model
file built against another can disagree silently. That argument applies with
equal force to the *documentation* of the format: a section 2 that describes
blocks the writer does not emit is a specification nobody is checking, and it
decays the same way — quietly, in the direction of flattering the project.
So the shipped `model/out/ocrcer-base.ocrw` was read byte by byte and compared
with what sections 2 and 7.1 claim about it.

**Measured, from the file on disk.** Header `OCRW`, `version` 1,
`model_kind` 1, four tables, `meta` 11,169 B. `prototypes` 14,088 x 107 int8,
1,507,416 B with 107 per-dimension scales; `prototype_class` 14,088 x u16,
28,176 B; `feature_norm` 2 x 107 f32, 856 B; `class_holes` 187 x u8, 187 B.
Whole file 1,548,603 B. `meta` carries `feature_version`, `feature_dims`,
`prototypes`, `sizes`, `build_id`, the nineteen faces each with a
`distribution`, and the 187-class charset as `{index, cp, category, twin}`.

**The licence question was checked first, because it was the one that could
have been serious.** `model/fonts.tsv` holds 19 `shippable`, 3 `local-only`
and 12 `excluded` faces; the base file's manifest lists 19 and the export path
loads local-only faces only under `--local`. So the shipped artifact contains
nothing derived from a face judged not distributable. That is a check that
passed, not a problem found, and it is recorded because the next reader should
not have to re-derive it.

**Four discrepancies, none of them a bug in the writer.**

1. **The charset is a `meta` record, not a table, and carries neither baseline
   class nor an aspect band** — section 2's row claimed both. They exist, in
   `model/charset.tsv`, and `ocrcer-build` reads them: the baseline class
   drives build-time assertions on the authored face, the band feeds
   `ClassGate`. Neither reaches the file. **The writer is right and the doc
   was wrong.** §4.1 step 2, the only consumer either would have at runtime,
   is disabled — and worse, the band in `charset.tsv` is width over *cap
   height* while the extractor's quantity is a different denominator entirely
   (this log, 2026-09-22, aspect entry). Emitting it today would ship a number
   the runtime could only misuse. It gets emitted when step 2 is enabled with
   a band authored in the extractor's own quantity, and not before.

2. **Prototype rows carry class only, not font family and style.** Section 2
   claimed all three and section 7.1 built an argument on the face tag being
   there. It is not. Segmentation does not actually need it — an optional
   segment is a separate file, so a row's segment is known from which file it
   came out of — so the 7.1 mechanism survives its own wording being wrong.
   **What the face tag would buy is diagnostic.** `FEASIBILITY.md` §5 stakes
   the whole font-coverage mitigation on failures being diagnosable, and
   "this glyph matched an Inconsolata prototype at 16 px" is that promise made
   good from the shipped artifact rather than only from a rebuild. Cost, on
   today's bank: face and size as u16 each, 4 B x 14,088 = 56,352 B, 3.6% of
   the file. **Deferred, not declined.** The trigger is the first coverage
   investigation that has to rebuild the bank to find out which face won a
   match; at that point it is `ocrcer-exporter`'s to add as a new table, which
   is additive and needs no `version` bump once the reader skips table names
   it does not know — a property the chunk 3 reader must be written to have.

3. **The pruning index is `class_holes`, one byte per class, not
   `proto_index` bucket lists.** Section 2 estimated ~40 KB; the realised form
   is 187 B, because with aspect and baseline pruning disabled there is
   exactly one integer per class to store. The doc's name and size both move
   to the measured ones.

4. **`params` does not exist.** Every threshold in the pipeline is still a
   Rust constant. That is a chunk 7 deliverable and section 2 now says so
   rather than listing it as though it were in the file. Until it lands,
   `CLAUDE.md` rule 1's "a guess is labelled a guess in the parameter block's
   metadata" has nowhere to write the label, which is worth knowing when
   reading any threshold in the current source.

**The size headline was wrong in the project's favour and is corrected.**
Section 2 said ~1.7 MB total. That assumed ~12,000 prototypes; the bank is
14,088 at four sizes, and 17,610 at the five-size ladder every benchmark
figure in this document is quoted at. Four blocks and four sizes is 1.48 MB
measured; adding the four authored blocks at their own estimates projects
about 1.85 MB, and doing that at five sizes projects about 2.2 MB. Section 2
carries all three figures with the measured one marked. The claim those
numbers were serving is untouched — `ocrs` is 12.24 MB, so even the largest
projection is a fifth of it — but a total that quietly assumed a smaller bank
than the one being benchmarked is the kind of number that gets repeated, and
this one had already been repeated.

**What was not done, deliberately.** Nothing in the file changed and no
`version` bump follows. Every discrepancy above resolved as *the writer is
right, the document was describing an intention* — which is the correct
outcome to want, and the reason to run the audit at all was that it might not
have been. The two items left open, the face tag in point 2 and the aspect
band in point 1, both have a named trigger rather than a date.

### 2026-09-22 — `version` guards meaning; unknown table names are skipped

Raised by the format audit earlier today, which found four blocks section 2
describes and the writer does not emit, and deferred one of them — a per-row
face tag — on the reasoning that adding a table later is cheap. That
reasoning was resting on a property section 7 never actually stated, and an
unstated property is one the chunk 3 reader's author would have had to guess
at.

**Decided.** An unrecognised `version` is refused at load. An unrecognised
table *name* is skipped without error. Written into section 7 as a normative
rule rather than left as an implication.

The asymmetry is the point and is worth stating plainly, because the obvious
instinct — a strict reader refuses anything it does not understand — is wrong
here in one direction. `version` says the tables you know have changed
meaning, and there is no safe partial read of that. A table name you do not
know is one you do not consume, so skipping it cannot make you answer
wrongly; refusing it only makes every future addition a breaking change, and
a `version` field that bumps for additions stops carrying information about
meaning. That is how the guard in section 7 would decay into decoration while
still looking like it was working.

**Consequence, for whoever writes the chunk 3 reader.** The table directory
is walked in full, entries are matched by name, and an unmatched name
advances past its `data_len` and continues. It is not an error and it is not
a warning on the happy path — though the loader recording *which* names it
skipped is cheap and belongs in whatever report says what the engine was
working with, alongside section 7.1's record of which optional files were
present.

### 2026-09-22 — The face manifest ships the licence it was built under, because the model file travels without the repository

A second pass over section 7.1 found a fifth discrepancy the format audit
earlier today missed. That section specifies a **`faces` table** carrying face
identifier, licence, licence source and distribution. The emitted file has a
`meta.faces` array of `{family, style, distribution}`: not a table, and two
fields short.

**Two separate calls, and they go opposite ways.**

**On table versus `meta`, the writer is right.** A manifest is mostly
variable-length strings; a table in this format is typed numeric data or an
opaque blob that would need its own string encoding invented inside it.
Nineteen JSON records are parsed at load with everything else in `meta` and
cost nothing. Section 7.1 now says `meta.faces`. This is the third time today
an audit has resolved as *the implementation chose correctly and the document
described an intention* — which is a good record for the implementation and a
poor one for the document.

**On the two missing fields, the document is right and this is the one thing
found today that should actually be built.** `licence` and `licence_source`
per face get emitted. The reasoning is not completeness for its own sake:
**the model file travels without this repository.** It ships into pdfcer as a
single artifact, and `CLAUDE.md` rule 2 stakes the whole licence position on
the tables being an original work derived only from unambiguously permissive
faces. Today that claim is answerable only by someone holding `model/fonts.tsv`
— which is to say, by someone holding the source tree. An auditor holding the
`.ocrw` alone cannot check it, and "go and find the TSV" is not a provenance
record. Measured cost, summing the two columns across the 19 shippable rows
of `model/fonts.tsv` plus JSON key overhead: **2,471 B** against a
1,548,603 B file, 0.16%. Before any escaping the strings may need.

**What it is not.** Not a `version` bump: `meta` gaining keys does not change
the meaning of any table, and `build_id` is a CRC over `meta` so it moves by
construction, which is the correct signal. Not a bank rebuild in the sense
§3 changes require — the feature vectors are untouched — though the file does
need re-emitting to gain the fields. Not a charset or feature change of any
kind.

**Hand-off.** `ocrcer-exporter`, with `ocrcer-build` supplying the values it
already reads out of `model/fonts.tsv` at face-selection time. The check worth
adding alongside it is the one whose absence caused all five of today's
findings: a test that parses the emitted file and asserts its `meta` keys and
table names against the specification, so that the next divergence between
section 2 or 7.1 and the writer fails a test instead of waiting for someone to
go looking. Two audits today, five findings, and the first pass missed this
one — which is the argument for the test rather than for a third pass.

### 2026-09-22 — The `.ocrw` on disk is built at a ladder none of this project's measurements endorse, and nothing said so

Found while auditing the file format. `model/out/ocrcer-base.ocrw` carries
`sizes: [16, 24, 32, 48]` and 14,088 prototypes. **Every accuracy figure
quoted anywhere in this document is at 16/20/24/32/48**, which is 17,610
prototypes on the current 19-face bank.

Those are not interchangeable, and this log already measured by how much. The
2026-09-21 held-out entry above ranked exactly these two ladders:

| bank scales | corpus word F1 | held-out word F1 | held-out smallest size |
|---|---|---|---|
| 16/24/32/48 — *what is on disk* | 81.02% | 82.05% | 68.53% @ 15 px/em |
| 16/20/24/32/48 — *what every quoted figure is at* | 85.30% | 83.69% | 71.71% @ 15 px/em |

So the artifact sitting in `model/out/` is the ladder that scores **1.64
points of held-out word F1 lower**, and 3.18 lower at the smallest size —
measured, on 20 faces, before the duplicate-face removal. It is there because
it is left over from the 2026-09-21 quantisation test, not because anything
chose it.

**Why this is worth an entry rather than a rebuild.** The obvious move is to
re-emit at the five-size ladder and move on. That is declined, for the reason
the ladder was left unblessed in the first place: `CLAUDE.md` rule 1 says
every number in the model file is authored with a sentence behind it or
script-derived, and *the sentence for 16/20/24/32/48 has never been written*.
It was selected by measuring four candidate ladders against a benchmark
corpus, which is fitting a parameter to a test set — the held-out check was run
precisely because of that and confirmed the ranking while cutting the margin
by 60%. Re-emitting now would promote a corpus-fitted choice to the shipped
artifact and make it much harder to argue with later.

**So the defect is not the ladder, it is that the file and the record
disagreed silently.** An `.ocrw` in a conventional-looking output directory
reads as *the* model. Anyone picking it up — including pdfcer, when the
hand-off gate is met — would get a file 1.64 held-out points worse than every
figure they had been shown, with nothing in the file, the directory or this
document to warn them. The `meta` block does carry its `sizes` array
faithfully, which is the format working correctly; what was missing is anyone
comparing it against the figures being quoted.

**Resolved as follows.**

- **The file stays as it is** and is now documented as what it is: a build
  artifact at a superseded ladder, not a release candidate.
- **No `.ocrw` is canonical until the ladder carries a sentence.** That is
  `ocrcer-glyphs`'s open item — author the ladder as a *spacing rule* with a
  justification, per the 2026-09-21 ladder entry's own conclusion, rather than
  as the winner of a four-way benchmark sweep. The rule then gets its own
  held-out confirmation.
- **Whatever emits a shipped file must record its ladder beside every accuracy
  figure quoted from it.** `meta.sizes` already carries the fact; the gap was
  entirely in the reporting.

**A check for `ocrcer-bench` that would have caught this in one line.** The
head-to-head harness constructs its bank from a ladder passed on the command
line and never reads `model/out/`. Nothing compares the two. A test that loads
the emitted file's `meta.sizes` and asserts it matches the ladder the
currently-reported headline was measured at would have failed the moment the
quantisation test left a four-size file behind — which is the same shape of
fix as the format-audit entry's: the artifact and the document need something
that compares them, because neither compares itself.

### 2026-09-22 — The tuning head-to-head re-measured without the duplicate face: 85.30% becomes 85.22%, and `ocrs` wins the drawing block

The 2026-09-21 entry *“Same prototype count, one different scale: 80.58%
against 85.30%”* quoted its headline on a corpus and a bank that both carried
Cascadia Code and Cascadia Mono, which the same-day duplicate-face entry
established are the same outlines under two names. The held-out figures were
amended that day; the tuning-corpus figures were not, and are amended here.
**Both entries stand — this section is append-only — and the figures below
supersede theirs.**

Re-run at the same settings on the de-duplicated corpus and the 19-face bank:
625 pages, 17,610 prototypes at 16/20/24/32/48, geometry weight 16, bank built
in 45.0 s.

| | OCRcer (oracle seg) | `ocrs` 0.12.2 (end to end) |
|---|---|---|
| character accuracy | **96.28%** | 77.20% |
| layout-free word F1 | **85.22%** | 81.20% |
| characters scored | 119,155 | 119,155 |

| | 660 pages, 20 faces | 625 pages, 19 faces | delta |
|---|---|---|---|
| OCRcer word F1 | 85.30% | 85.22% | −0.08 |
| `ocrs` word F1 | 81.12% | 81.20% | +0.08 |
| **margin** | **+4.18** | **+4.02** | **−0.16** |

**The duplicate was worth 0.16 points of margin, and the projection that said
so was three times too large.** The duplicate-face entry projected roughly
0.43 points of word-F1 inflation for OCRcer and roughly zero for `ocrs`, from
the duplicated face scoring above the corpus average. Measured: 0.08 and
+0.08. Right sign, wrong magnitude, and it is recorded because a projection
nobody goes back and checks is how a projection becomes a fact — the same
failure this document spent the day auditing in the other direction.

**The loss that matters is by text block, and it is in the flagship domain.**
On layout-free word F1, `ocrs` beats OCRcer on four of seven blocks:

| block | OCRcer | `ocrs` | |
|---|---|---|---|
| drawing | 84.67% | **89.18%** | −4.51 |
| invoice | 85.27% | **91.18%** | −5.91 |
| prose | 89.60% | **91.42%** | −1.82 |
| statement | 80.08% | **86.85%** | −6.77 |
| currency | **83.91%** | 72.10% | +11.81 |
| technical | **89.04%** | 56.26% | +32.78 |
| twins | **82.44%** | 58.70% | +23.74 |

The aggregate +4.02 is the average of a 33-point win and a 7-point loss, which
is not a description of an engine that is uniformly better. **CAD drawing text
is the domain section 6 of `FEASIBILITY.md` says this project exists to win,
and it is losing that block while holding oracle segmentation.**

**And the mechanism is visible in the same row.** On the drawing block OCRcer
reads *more* characters correctly — 96.09% against 95.45% — and still loses
the word metric by 4.51 points. Its character errors are spread across more
distinct words than `ocrs`'s are. That is the signature of a matcher with no
decoder: nothing collapses a near-miss back onto a real word, so every isolated
character error destroys a whole word, whereas a decoder with a lexicon and
bigrams repairs exactly that class of error. Chunks 5 and 6 are what address
it, and 84.67% is the number they will be measured against. *(Read the block's
char-accuracy column with care elsewhere: on invoice and statement `ocrs`
scores 56.41% and 49.12% because in-order character accuracy charges it for
reading multi-column layouts column-first. That is a reading-order artefact,
not recognition, and it is why the layout-free metric is the one this project
reports.)*

**Three of eighteen faces lose outright, and both condensed faces are among
the weakest.** `ocrs` takes Inter 88.24% against 82.75%, Roboto Condensed
84.73% against 79.25%, and PT Mono 81.94% against 81.83% — the last of which
is 0.11 points and should be read as a tie. Against a per-face median of
86.86%, OCRcer's five weakest faces are Open Sans Condensed 61.92%
(24.94 below), STIX Two Math 77.17% (9.69), Roboto Condensed 79.25% (7.61),
PT Mono 81.83% (5.03) and Inter 82.75% (4.11). **The corpus contains exactly
two condensed faces and they are the first and third weakest.**

That is a pattern in two rows out of eighteen, so it is stated as a
**hypothesis, not a finding**: a condensed face packs the same stroke count
into a narrower advance, so at a fixed render size its counters and gaps are
the first to close, and the 32×32 normalisation removes the glyph's size
without removing that. The thing that would test it is a per-face error
breakdown against set width and stroke width, which is an open item for
`ocrcer-bench`. Until that is run, nothing here justifies a feature or
charset change; if it were confirmed, the cheap fix is more prototypes from
condensed families, not a new feature dimension.

**What does not change.** No table, no charset, no feature definition, no file
format, no `version`. This is a corpus and bank composition correction to
figures already recorded. The **hand-off gate is unmet and unmoved**: it
requires layout-free word F1 above `ocrs`'s measured end to end on *both*
sides, and OCRcer has no end-to-end path at all — chunks 2 and 6 are stubs and
every figure above was handed every glyph box and every space out of the page's
own ground truth. The better estimate of where the engine stands remains the
held-out corpus (83.15% against 82.72%, +0.43), because `bench/pages` is the
corpus the scale ladder was swept against and its figures are
corpus-flattered by construction.

Full run, including the per-size and per-face tables and the two-variables
caveat, in `docs/measurements/2026-09-22_vs_ocrs_625_tuning_w16_bank19.txt`.

### 2026-09-22 — “Faster than the neural design it replaces” has ten runs against it and none for it; and OCRcer's page cost is linear in bank size while `ocrs`'s is flat

Section 4.1 has said since chunk 0 that the engine would come in *comfortably
under a second per page single-threaded, which is faster than the neural design
it replaces rather than slower*. It has never been tested and was never
labelled untested, which is how a projection becomes a fact. Every head-to-head
run this project has done also recorded wall clock for both engines, so the
evidence was sitting in `docs/measurements/` the whole time and nobody had
looked at that column.

| bank | corpus | pages | OCRcer s | `ocrs` s | OCRcer s/page | `ocrs` s/page |
|---|---|---|---|---|---|---|
| 14,832 @ 4 scales, no weight | tuning | 660 | **172.0** | 273.8 | 0.26 | 0.41 |
| 14,832 @ 4 scales | held out | 660 | **202.3** | 241.4 | 0.31 | 0.37 |
| 14,832 @ 4 scales | tuning | 660 | **229.1** | 251.3 | 0.35 | 0.38 |
| 18,540 @ 5 scales | tuning | 660 | **258.3** | 276.5 | 0.39 | 0.42 |
| 18,540 @ 5 scales | size sweep | 1,716 | 654.3 | **594.9** | 0.38 | 0.35 |
| 18,540 @ 5 scales | size sweep | 924 | 341.7 | **339.5** | 0.37 | 0.37 |
| 18,540 @ 5 scales | held out | 660 | 273.7 | **264.1** | 0.41 | 0.40 |
| 18,540 @ 5, 12px lead | tuning | 660 | 297.1 | **251.5** | 0.45 | 0.38 |
| 17,610 @ 5 scales, 19 faces | tuning | 625 | 277.2 | **250.8** | 0.44 | 0.40 |
| 22,248 @ 6 scales | tuning | 660 | 326.5 | **249.0** | 0.49 | 0.38 |

**Four wins, five losses, one level** — the 924-page sweep is 341.7 against
339.5, six tenths of a percent, which is not a result either way.

**The per-page half of the projection holds and holds easily.** Every run lands
between 0.26 and 0.49 s/page, against a projection of *comfortably under a
second*. That half is confirmed as far as this evidence can confirm anything.

**The comparative half has no support.** All three runs at the 14,832-prototype
bank win; one of seven at a larger bank does. And the pattern underneath is
cleaner than the win/loss split: averaged per bank, OCRcer costs **0.31 s/page
at 14,832, 0.40 at 18,540 and 0.49 at 22,248** — a bank 1.50× larger for a page
1.62× dearer, which is matching dominating and close to linear, exactly as this
section's opening line implies. `ocrs` over the same ten runs sits between 0.35
and 0.42 s/page with no trend, because a fixed network costs what it costs.
*(The 19-face row is off the trend at 0.44 where 0.38 would fit; it is the only
run with known unrelated machine load, and it is left on the trend line's wrong
side rather than smoothed away.)*

**The consequence this document had not priced: bank growth is not free.** Every
charset addition, every face added for coverage, every scale added to the ladder
buys accuracy with page time on a near-linear curve, and `ocrs` pays nothing for
any of it. Section 9's prototype budget was argued on file size and build time,
both of which are trivial; the cost that actually binds is per-page matching.
This is one more reason the answer to a coverage gap is *the right faces*, not
*more faces*.

**And OCRcer is losing these while doing less work.** In every run it was handed
segmentation, spacing and layout out of the page's ground truth, and `ocrs` did
all three itself. Whatever chunks 2 and 6 cost goes on top of the numbers above.

**What is and is not claimed.** None of the ten is a controlled timing
measurement: the runs are sequential on a machine carrying varying unrelated
load, and the harness's wall clock includes page rendering and scoring alongside
recognition, so the absolute numbers are loose upper bounds and the two columns
are not perfectly paired. The per-bank trend is the robust part, because it
holds across four bank sizes and two corpora; a single row's margin is not. This
is recorded as **evidence against a claim, not a measurement replacing it**.

**Resolved as follows.** Section 4.1 and `FEASIBILITY.md` §5 now carry the
evidence beside the claim, and the comparative sentence is marked not to be
quoted until a clean measurement exists. **No design change follows.** The three
levers section 4.1 already names — hole-count pruning, a working aspect band,
and the optional `parallel` feature — are untouched, and none has been measured
for speed either. Spending a feature dimension or a prototype budget chasing a
number nobody has measured properly would be the wrong move; the right one is to
measure it.

**Open item for `ocrcer-bench`:** a timing mode that measures recognition alone
on an idle machine, with rendering and scoring excluded and both engines given
the same pages in the same order — and, once chunks 2 and 6 exist, end to end on
both sides, which is the only comparison that answers the question section 4.1
asked.

All ten runs, their source files and the full caveat list are in
`docs/measurements/2026-09-22_timing_survey_ten_runs.txt`.

### 2026-09-22 — `feature_weights` is now writable, per block with named dimensions on top; the table's absence is now a statement

The `feature_weights` table has been readable since chunk 1 (`T_FEATURE_WEIGHTS`,
validated finite and non-negative, defaulting to all `1.0`) and no writer ever
emitted it. Section 4.1 step 3 has owed an authored weight vector since the
2026-09-21 entry above. `model/feature_weights.tsv` and `ocrcer-build`'s
compiler for it close half of that: the mechanism exists, and the shipped file
holds every block at `1.0`, so today's bytes are unchanged.

**No `version` bump, and section 7 is why.** An unknown table name is skipped
silently by a reader, so adding a table is additive and a file carrying one
loads in a runtime that predates it — with every weight at the `1.0` that
runtime already assumed. This is the asymmetry the format was given
deliberately, and this is the first change to use it.

**An all-`1.0` file writes no table at all.** A table of ones is exactly what
the default already is, so emitting one would say nothing while looking like a
decision. The consequence is the useful part: **the presence of a
`feature_weights` table in an `.ocrw` file is itself the statement that the
matcher has an opinion**, and a reviewer can tell the two apart by reading the
table list.

**The parameterisation answers the 2026-09-21 decision rather than repeating
the thing it rejected.** That entry measured a single scalar over the geometry
block and recorded that one scalar is the wrong parameterisation — `0` against
`O` was 71% worse at the aggregate optimum, because within those four
dimensions one number cannot separate a pair that differs in height without
amplifying a pair that does not — and said what step 3 owes is per dimension.
A flat 107-row file is also not allowed: rule 1 forbids 107 numbers nobody can
justify one at a time. So a row's key is **a section 3.1 block name, or one of
the four baseline-relative dimensions, which override their block**:
`geometry.aspect`, `geometry.ink_fraction`, `geometry.height_above_baseline`,
`geometry.depth_below_baseline`. Nothing else is nameable, because nothing else
has an individual meaning to write a sentence about; gradient bin 37 does not.
Blocks apply first and named dimensions second regardless of file order, so the
file cannot mean two things depending on how it is sorted.

**Every block must appear, including the ones at `1.0`.** A missing row would
read as a default and the build refuses the file instead of guessing. A file
that exists but is malformed is an error, never a fallback to uniform weights:
a typo that quietly reverted the matcher to `1.0` everywhere would cost
accuracy with nothing reporting it. `ocrcer-build write` now prints a
provenance census for weights beside the one it prints for `params.tsv`, so a
weight that is really a guess cannot read as a result.

**What is not claimed.** No weight in the shipped file is anything but `1.0`
and none has been measured into it. The `--geometry-weight` figures in the
entries above remain **a bench-harness measurement, not a shipped parameter**,
and stay labelled that way until a candidate is authored into
`model/feature_weights.tsv` with its per-confusion-pair effect stated beside
its aggregate, per section 4.1's standing rule.

### 2026-09-22 — The fixture suite gained a decode stage, and its lattices are authored rather than captured

Section 8.2 named per-stage fixtures from the start; until now only the glyph
stage had any. `fixtures/decode/<name>.lattice.json` and
`fixtures/expected/decode/<name>.decode.json` add the decoder, and eight are
blessed.

**The decoder needs fixtures more than its error rate suggests.** A tie broken
by the wrong rule is right about half the time it occurs, so it moves an
aggregate CER by almost nothing while making the engine non-deterministic
across platforms — which is exactly the property section 8.2 exists to defend
on x86 and wasm32 both. The total tie order (score descending, then lowest
class index, then earliest cut, then oldest trace entry) is now asserted by a
fixture that fails on a one-ULP score change.

**The lattice is authored, not captured, and that is the whole design.** A
captured lattice would make each fixture a record of what the matcher returned
on the day it was captured: every bank rebuild would demand a re-bless, and
nobody could check the file by reading it. An authored lattice has ground
truth — the candidate distances *state* which reading the image favoured, so
the expected string follows by arithmetic a reviewer can redo on paper. For the
same reason these fixtures run the decoder with **no language tables** and with
class indices drawn from the **fixture's own sorted character set**, not the
shipped charset's: a charset edit must not be able to invalidate a decode
fixture, because the two are testing different things.

Each input carries a `why` field, and every failure prints it. A fixture whose
reason for existing is not written down is one a future session will bless
away.

**The blessing gate now spans both stages in one plan.** `bless` plans the
glyph and decode stages together rather than separately, because the
multi-stage rule — a blessing that moves more than one stage boundary's worth
of output at once is adjudicated, not applied — can only count stages it was
shown. Planned separately, a change that moved the feature vector *and* the
decoder's reading would pass as two single-stage blessings.

**Measured, not projected.** 17 fixtures pass (9 glyph, 8 decode); the
perturbation suite, which alters each stage deliberately and asserts a loud
failure, is 14 tests and passes. Each blessed decode score was predicted by
hand before the bless and matched: `char_bonus 3.44` less the candidate's
distance per character, with `12.20` identical across `lower`, `Title` and
`UPPER` — which is the claim the case-shape term makes — and `19.52 - 1.50 =
18.02` where the penalty fires.

**Still owed:** `fixtures/pages` and the binarization, segmentation and line
stages are chunk 2's and do not exist.

### 2026-09-22 — The 14px weakness is the bench renderer losing hairlines, not the matcher; every per-size figure is provisional until the corpus is re-rendered

Every sweep this project has run reports CER by render size, and the curve has
always had the same shape — worst at 14 px/em, best at 28. The winning row of
the current sweep reads 17.684% at 14px against 2.934% at 28px. That shape is
so expected that nobody had audited it.

**It is largely the corpus.** Two measurements over `bench/pages`, both from
the truth JSON and the PGM bytes, neither touching the engine:

- **Truth glyphs whose whole ink box is two pixels tall or less**, excluding
  punctuation: 202 of 19,661 at 14px (1.03%), 94 at 18px, 58 at 21px, **zero
  at 28px and 40px**. The first glyph of the worst page —
  `open-sans-condensed__light__invoice__14px`, the `I` of `INVOICE` — is
  recorded as `{"ch":"I","x":16,"y":21,"w":1,"h":1}`. One pixel. The truth
  file asserts a character the page does not contain.
- **Single-contour letters and digits arriving as two or more 8-connected
  pieces**: 9.52% corpus-wide at 14px, 6.86% at 18px, 3.40% at 21px, 2.01% at
  28px. By face at 14px: Open Sans Condensed **47.9%**, STIX Two Math 33.0%,
  Liberation Serif 23.9%.

The pages are two-level — a histogram of the worst page has exactly two
distinct values, 0 and 255 — so the generator rasterises by sampling pixel
centres against the filled outline with **no dropout control**. A stem thinner
than the sampling pitch, which is what a Light weight of a Condensed face is at
14 px/em, falls between the samples and is not drawn on most scanlines. An
ASCII dump confirms it: the stems are dotted, not continuous.

**No real input looks like this.** Monochrome rasterisers implement dropout
control because the TrueType specification requires it, and scanner and PDF
pipelines do not emit bilevel at all — they emit coverage, which this engine's
own binarizer turns into a continuous one-pixel stem. So `bench/pages` at the
small sizes is not the "clean two-valued ink, an upper bound on scanned input"
its own header claims; it is an *undeclared* degradation, harsher than and
unlike anything section 5 is designed against.

**What this does and does not invalidate.**

- **Head-to-head figures survive.** `ocrs` read the same broken pages. A
  relative win or loss is still a relative win or loss, and the drawing-block
  loss recorded above stands.
- **Absolute CER is pessimistic by an unknown margin**, concentrated at 14px
  and 18px and concentrated in three faces. Every absolute accuracy number
  this project has published is therefore a lower bound, and is to be quoted
  as one until the corpus is re-rendered.
- **Any parameter selected on a per-size objective is suspect.** Nothing has
  been; the sweeps optimise corpus CER, in which the 14px stratum is one fifth
  of the pages. That is a fifth of the objective spent on a rasterisation
  artefact, which is enough to pull a threshold and not enough to have chosen
  a wrong sign. Measured, at `case_shape_penalty 3.44, w_bigram 0.1` over the
  49-page stride sample: geometry weight 6 is the best of {1, 6, 12} in four
  of the five size strata (14px 17.684 vs 19.053 and 19.110; 18px 8.177;
  21px 4.889; 28px 2.934) and second at 40px, where weight 12 is ahead by
  0.114 points (3.295 vs 3.409). The choice does not rest on the 14px
  stratum.
- **It does not touch `fixtures/`.** The golden fixtures of section 8.2 are
  authored, not rendered by this generator.

**The fix is the bench's, and it is cheap.** Render with 8-bit coverage and let
`ocrcer-core`'s own binarizer threshold it — which is both the faithful
pipeline and a free gain, because a pre-thresholded corpus never exercises the
binarizer at all. Two assertions belong in the generator, not in a script run
once: no inked truth glyph may have a box two pixels tall or less, and a glyph
that is one closed contour must rasterise as one connected component. Dropping
the affected cells is the last resort, not the first.

**No architecture change follows.** This is not a case for a morphological
close before component analysis, or for small-scale prototypes, or for a
feature that tolerates broken strokes: the input that motivated each of those
is an artefact. Broken strokes in genuinely degraded scans are a real problem
and belong to the deliberately degraded corpus, with a named degradation model
and a measured rate, so the number means something. Fixing a renderer is not a
licence to widen section 3.

**Owed:** `ocrcer-bench` re-renders `bench/pages` with coverage output and the
two generator assertions, then every figure in section 9 and every head-to-head
is re-measured on the new corpus and the two sets are reported side by side. No
number here is restated as improved until that has been run.

### 2026-09-22 — Three decoder/matcher numbers move from guess to measured, and `feature_weights` ships for the first time

Section 4.1 step 3 has said since it was written that the feature weights are
not yet authored and everything measured to date is unweighted. That is no
longer true, and this entry is what replaces it.

**What was swept.** The cross product of `decode.case_shape_penalty` ∈ {1.5,
3.44}, `match.geometry_weight` ∈ {1, 6, 12} and `decode.w_bigram` ∈ {0.05,
0.1, 0.35} — 18 points — end to end on `bench/pages`, the engine handed the
page and nothing else. Run twice on disjoint samples: stride 13 offset 0 (49
pages) and stride 13 offset 1 (48 pages). 13 is coprime with both the corpus's
5 render sizes and its 6 text blocks, so neither sample is one stratum.

**The same row won both.**

| sample | winner | control (the file's own values) | margin |
|---|---|---|---|
| stride 13, offset 0 | **7.228%** CER at `3.44 / 6 / 0.1` | 9.296% at `1.5 / 1 / 0.35` | 2.068 pts |
| stride 13, offset 1 | **7.634%** CER at `3.44 / 6 / 0.1` | 9.468% at `1.5 / 1 / 0.35` | 1.835 pts |

Axis decomposition at the winner, first sample: the geometry weight is worth
0.881 points, `w_bigram` 0.35 → 0.1 is worth 1.469, and the case-shape penalty
1.5 → 3.44 is worth 0.196 (0.065 on the held-out sample — a small effect
measured twice in the same direction, and reported as small).

**Section 4.1's standing rule, discharged.** A weight set that improves the
aggregate while worsening an identifier-critical pair is a regression, so the
candidate is quoted with its per-pair effect. Aligned substitution counts over
the 49-page sample at `case 3.44, w_bigram 0.1`:

| confusable group | w=1 | w=6 | w=12 |
|---|---|---|---|
| `0` / `O` | 17 | **17** | 15 |
| `0` / `o` | 7 | **5** | 20 |
| `O` / `o` | 29 | **18** | 16 |
| `1` / `l` | 21 | **22** | 23 |
| `5` / `S` | 5 | **8** | 6 |
| `8` / `B` | 6 | **5** | 3 |
| case `c/C s/S v/V z/Z` | 56 | **29** | 31 |
| space dropped / inserted | 92 / 27 | **95 / 29** | 94 / 29 |
| all listed substitutions | 470 | **439** | 448 |

At weight 6 the case family halves, `0`/`O` is flat, `0`/`o` improves, and
`1`/`l` is one worse. **Weight 12 is rejected on the per-pair rule, not on its
aggregate**: its aggregate is fine (448 against 470) and it wins the 40px
stratum, but it takes `0`/`o` from 7 to 20 — which is the 2026-09-21 weighting
entry's warning reappearing, that one scalar over four geometry dimensions
cannot separate a height-differing pair without amplifying a same-height one.
That warning is why a row in `feature_weights.tsv` may name a single dimension.
At weight 6 that mechanism is not needed, so it stays unused and available.

**Authored as follows.** `decode.w_bigram` 0.35 → **0.1**, guess → measured;
`decode.case_shape_penalty` 1.5 → **3.44**, guess → measured; the `geometry`
block in `model/feature_weights.tsv` 1.0 → **6.0**, authored → measured. The
emitted file now carries a `feature_weights` table for the first time — nine
tables, and `ocrcer-build inspect` reports `dim 103 = 6` with every other
dimension at 1, which is the geometry block and nothing else.

**Why a low bigram weight is a sentence and not just a sweep result.** This
corpus is invoices, statements, currency and drawing text. An authored English
bigram prior is right about prose and wrong about part numbers, amounts and
codes, and at 0.35 it was overriding matcher evidence on exactly the strings
the project exists to read. That is the same claim rule 6 makes about the
lexicon, applied to the other language table.

**Measured, artifact against artifact.** The previous `.ocrw` and the rebuilt
one, both at the 16/24/32/48 ladder, on the two disjoint samples:

| sample | previous file | tuned file |
|---|---|---|
| offset 0 (49 pp) | CER 9.296%, word F1 69.094% | **CER 6.814%, word F1 77.785%** |
| offset 1 (48 pp) | CER 9.468%, word F1 66.728% | **CER 7.240%, word F1 74.307%** |

**That gain is not all parameters.** The rebuild also picked up the lexicon
work of the same session — 2,322 nodes to 2,477 — so the artifact-to-artifact
delta is the three parameters *and* the lexicon together. The parameter-only
effect is the sweep margin above, 2.068 and 1.835 points, and that is the
number to quote for the parameters.

**What this does not settle.** `model/out/ocrcer.ocrw` is still at the
16/24/32/48 ladder, still a build artifact rather than a release candidate, for
the reason the ladder entry above gives: the sentence behind 16/20/24/32/48 has
never been written. These three values were selected against this corpus and
confirmed on a disjoint sample *of the same corpus*, which is not the same as
unseen input. And every absolute figure here is measured on pages whose small
sizes are damaged by the renderer, per the entry above — the comparisons are
sound, the absolute levels are a lower bound.

**Two checks earned their keep on the way in.** `params.tsv` and the
compiled `Params::DEFAULT` are asserted equal by the build's own test, and it
failed the moment the file moved — which is the point: a runtime handed a file
with no parameter table falls back to the compiled defaults, and if those two
disagree the same model reads differently depending on which tables happen to
be present. And `bless` plans **0 fixtures changed, across 0 stages**, which is
correct rather than suspicious: section 8.2's decode fixtures pin the
parameters they exercise in their own `params` block, and the four that do not
are the ones where no penalty fires. A tuning sweep must not be able to force a
re-bless, or the fixtures stop being independent ground truth and start being a
record of the last sweep.

**One duplicated assertion was removed rather than updated.** A core test
spot-checking that `get("decode.w_bigram")` reaches the right slot also
asserted the literal `0.35`. It now compares against the field. What the value
*should* be is asserted once, by the build's file-against-defaults check; a
second copy in another crate can only ever go stale in the direction of being
edited to match whatever the code now says.

**Still guess:** 30 of the 43 parameter rows. The largest remaining error
bucket is word splitting — dropped and inserted spaces together outnumber every
character confusion by roughly four to one at every geometry weight, and both
`words.min_separability` and `words.lone_gap_x_heights` are still labelled
guesses. That is the next sweep, not another matcher weight.

### 2026-09-22 — Word splitting swept: the lone-gap rule was set 43% too high, and it took four disjoint samples to say where it actually sits

The entry above named this the next sweep, on the grounds that dropped and
inserted spaces together outnumber every character confusion by roughly four to
one. This is that sweep.

**What was swept, and how the sample was chosen.** `words.lone_gap_x_heights`
over {0.5, 0.65, 0.7, 0.75, 0.8, 0.85, 0.9, 1.0} and `words.min_separability`
over {0.0, 0.3, 0.45, 0.6, 0.75, 0.9}, end to end on `bench/pages`. Samples are
stride 13 at offsets 0, 1, 2 and 3 — 13 is coprime with both the corpus's five
render sizes and its six text blocks, so no sample is one stratum, and the four
offsets are disjoint.

**One sample was not enough, and that is this entry's main finding.** Offset 0
alone said the argmax was 0.8; a finer grid on the same sample said 0.75;
offset 1 said 0.65. Three different answers from the same corpus, all from
margins of a few hundredths of a CER point. Pooled over all four:

| `lone_gap_x_heights` | off 0 | off 1 | off 2 | off 3 | mean CER |
|---|---|---|---|---|---|
| 0.65 | 6.585 | 6.782 | 7.827 | 7.180 | 7.094 |
| **0.70** | **6.509** | **6.716** | **7.794** | 7.213 | **7.058** |
| 0.75 | 6.487 | 6.793 | 7.980 | 7.432 | 7.173 |
| 0.80 | 6.509 | 6.935 | 8.134 | 7.509 | 7.272 |
| 1.00 — the authored value | 6.814 | 7.240 | 8.265 | 7.773 | 7.523 |

0.70 is the pooled argmax, and — the part that matters more than the argmax —
**it beats the authored 1.0 on every one of the four samples individually**, by
a mean of 0.465 CER points and 1.58 word-F1 points. The single-sample argmaxes
are inside noise of each other; the margin against 1.0 is not.

**The mechanism, so the number has a sentence and not just a rank.** Against
1.0 on the offset-0 sample, dropped spaces fall 99 → 70 while inserted spaces
rise 29 → 34. The authored 1.0 said *no gap as wide as a whole x-height is
intra-word*, which is true and was never the binding claim; the binding claim
is the converse, and at 1.0 the rule was refusing to call a 0.8-x-height gap a
space in exactly the tabular and title-block lines where the valley test has
already been shown to fail.

**Section 4.1's per-domain rule, discharged.** A word-splitting change is
acceptable only if it does not damage identifier-shaped tokens, which the
lexicon is forbidden to repair. Measured over all 90 pages of each block,
1.0 → 0.7:

| block | CER before | CER after | word F1 before | word F1 after |
|---|---|---|---|---|
| drawing | 5.561 | **5.321** | 79.056 | **79.719** |
| technical | 6.303 | 6.314 | 77.843 | 77.775 |

Drawing improves; technical moves by about a hundredth of a point in each
direction, which is flat. Accepted.

**`min_separability` stays 0.6, and its provenance changes anyway.** The
finding is a shape rather than a peak: 0.0, 0.3 and 0.45 measure byte-identical
CER, WER and F1 — below 0.45 the gate never rejects a valley on this corpus —
0.6 is the pooled argmax by roughly a tenth of a point, 0.75 is worse on both
samples, and 0.9 falls off a cliff (9.87% against 6.51% CER at the same lone
gap). So 0.6 is authored as **the top of the flat region**, which is a
statement that survives a corpus change in a way "0.6 won" would not.

**`min_gaps` is measured INERT, and therefore stays authored.** {2, 3, 4, 5}
return byte-identical CER, WER and F1 on two disjoint samples: no line in this
corpus has fewer than five gaps, so the guard never fires. The sweep cannot
choose this value, so it stays **authored** at 3 rather than being promoted to
**measured** on a tie, and its `tune` flag goes to no.

**The harness was printing a winner for that sweep anyway** — `best
words.min_gaps = 2`, which is only the first row. `tune` now detects a sweep
whose rows all measure identically and says so *before* the best line: there is
no argmax, the parameter did not bind. A sweep that always names a winner is
how "3 is the measured optimum" gets written down about a parameter that never
bound.

**No fixture was blessed, and `bless` reporting zero changes proves nothing
here.** There is no word-splitting fixture: `fixtures/expected` holds only
`decode` and `glyphs`, and `fixtures/pages` does not exist. Section 8.2 now
states that gap, and states what a zero-change bless does and does not mean, so
it is visible in the contract rather than discoverable by listing a directory.

**One core test was corrected, not blessed.** `segment`'s edge-cropping fixture
began failing at 0.7 — it asserts its fixture is one word, and the fixture has
a 3px x-height, so a one-pixel inter-letter gap is already a third of one. That
is an artefact of the fixture's scale, not a claim about printed text, and the
fix is that a test of edge cropping must not be steerable by a word-splitting
parameter: the helper now pins `words::Params` the same way it already pinned
`lines::Params`. Core suite 124 passed.

### 2026-09-22 — The oracle column was not a ceiling: it ran a different matcher, and scored below the thing it was supposed to bound

`vs-ocrs` prints three columns — OCRcer end to end, OCRcer given perfect glyph
boxes ("oracle"), and `ocrs`. The oracle column's entire purpose is to be a
**ceiling**: the difference between it and the end-to-end column is what layout
and segmentation cost, because the matcher is held constant.

**It was not held constant.** The harness hard-wired `geometry_weight = 1.0`
for the oracle bank while the end-to-end column took its weight from the
`.ocrw` — which has carried 6 since `feature_weights` shipped. The oracle was
therefore an *unweighted* matcher being reported as a bound on a weighted one.

The signature was visible in the last head-to-head and was not read as a bug:
**oracle word F1 71.19% against end-to-end 72.93%.** A ceiling sitting 1.74
points under the thing it bounds is not a tight ceiling, it is a different
measurement wearing the label — and the near-zero gap it appears to show, which
reads as "segmentation costs nothing", is an artefact of comparing two
matchers.

**Fixed by defaulting to the file rather than to a constant.**
`--geometry-weight` is now optional: absent, the oracle takes the weight out of
the engine file and says so in the header; present, it means "measure a
candidate weight this file does not carry", and the two columns are then
explicitly different matchers. If a file ever weights the four geometry
dimensions unequally — which section 3.1 permits, and which the 2026-09-22
weighting entry reserved as the escape hatch for `0`/`o` — one group scalar
cannot mirror it, so the oracle column is **dropped with a printed reason**
rather than quietly misreported.

**The general shape, because this is the second instance.** A derived column
whose meaning depends on matching another column's configuration must *take*
that configuration rather than duplicate its default. The format-audit entry
above found the same shape between the file and this document; here it was
between two columns of one table. Neither compares itself.

Every figure from the previous 625-page head-to-head is superseded for the
oracle column and unaffected for the other two.

### 2026-09-22 — Rebuilt at the five-size ladder: the "about 2.2 MB" projection measures 1.89 MB

The 2026-09-22 ladder entry left `model/out/` holding a four-size file while
every quoted accuracy figure was at five, and resolved that the defect was the
silent disagreement rather than the ladder. The disagreement is now removed the
other way: `model/out/ocrcer.ocrw` is rebuilt at 16/20/24/32/48 — 17,610
prototypes, 19 faces, nine tables — and carries the tuned parameters, confirmed
by reading them back out of the file rather than out of `params.tsv`.
`inspect` now prints the word-splitting block for exactly that reason: a tuned
value that failed to reach the table would otherwise show up only as accuracy
that did not move.

**This does not bless the ladder.** The sentence behind 16/20/24/32/48 has
still never been written, and `ocrcer-glyphs`'s open item — author it as a
spacing rule, then confirm the rule on held-out sizes — is unchanged. What the
rebuild buys is that the artifact and the record no longer disagree while both
are provisional. Section 2 says so in those terms.

**Section 2's Size column, re-read.** File 1,986,028 B; `prototypes`
1,884,270; `prototype_class` 35,220; `meta` 13,590; the four language tables
50,237 between them, now **2.53%** of the file. The previous line projected
"about 2.2 MB" for this ladder from the measured per-scale prototype cost. The
reading is 1.89 MB, so the projection was 16% high — recorded because this
document's standing rule is that a projection is replaced by a measurement and
the replacement is stated, not overwritten.

`ocrs` remains 12.24 MB.

### 2026-09-22 — The bench corpus was asserting that a capital `I` is one pixel

The 2026-09-22 hairline entry recorded that the 14px weakness was the bench
renderer losing thin strokes, and owed a re-render. This is that re-render, and
the defect was worse than "the pages are hard": the *ground truth* was wrong.

`bench/pages` was rendered by the same rasteriser the prototype bank uses,
which sets a pixel from a **strict majority** of a 5×5 subgrid. That is the
right rule for a canonical prototype shape and the wrong rule for a page. At
14 px/em a Light Condensed stem never reaches 13/25 anywhere along its length,
so the stroke rounded away entirely except where dropout control forced one
pixel on — and the truth box, taken as the tight box of that ink, recorded the
`I` of `INVOICE` as **1 × 1**.

Measured over the old corpus: **338 alphanumeric truth boxes two pixels tall
or less, on 19 of 625 pages**, 0.34% of 98,305 glyphs. Over the size-sweep
corpora the same count is monotone in size and **zero at every size ≥ 22px**:
1.08% at 12px, 0.99% at 13–14px, 0.83% at 15px, 0.63% at 16px, 0.45% at
17–19px, 0.28% at 21px, 0 at 22px and above.

That shape is the finding. A reference box a correct read cannot match charges
the engine a deletion for reading the page right, so every per-size figure
below 22px was biased against the engine, and biased *more* the smaller the
size. The small-size weakness this project has been chasing was partly its own
corpus. The bank size ladder was chosen on those pages.

**What changed, in three parts, none of which touches sections 2, 3 or 7.**

1. The rasteriser keeps the coverage it already computed — `cov = round(hits ×
   255 / 25)` — beside the bilevel plane instead of discarding it. The bank
   still takes the bilevel plane and is **byte-identical**: 71 build tests
   green, no rebuild, no charset, feature or normalisation change.
2. Coverage is carried **uncropped**, with the ink-box offsets beside it.
   Cropping it to the ink box would delete exactly the sub-majority fringe the
   change exists to keep; in the pathological case almost the whole mark lies
   outside the ink box, which is how the `I` became 1 × 1 in the first place.
3. A page is composited in grey — darkest wins where two marks overlap — then
   binarized **once** with the shipped parameters, and each truth box is the
   tight box of binarized ink *restricted to that glyph's own coverage
   footprint*. The footprint restriction stops two touching marks claiming each
   other's pixels. A glyph that lays down coverage and leaves no ink is
   reported in `Page.dropped`, not asserted as ground truth.

A truth box is now, by construction, the box the engine will actually find.
Same page, same geometry, same 98,305 glyphs, `INVOICE` at 14px in Open Sans
Condensed Light: `I` 1 × 1 → **2 × 10**; degenerate boxes corpus-wide
**338 → 0**.

**Two guards now refuse a page rather than write it**, because a script run
once is a thing that was true in September. No alphanumeric truth glyph may
have a box two pixels tall or less — punctuation is exempt, a full stop really
is two pixels at 14px. And a character whose outline is a **single closed
contour** must arrive as a **single 8-connected component**, counted with the
runtime's own labeller per section 4 of the project rules. The rule is
one-directional on purpose and the converse is not asserted: `O` is two
contours and one component, `8` is three and one.

**The guards have teeth, which was checked rather than assumed.** Rendering
the same corpus below its floor fails immediately and names the page:
Liberation Serif Regular rejects at 13px (`L` in two components), 12px (`h`),
11px (`r`) and 10px and 8px (`C` in three). At 14px and above, all 625 pages
pass both guards. So **14 px/em is the measured floor at which this generator
produces credible ground truth for all 18 corpus faces** — and the size-sweep
corpus ran to 12px, two sizes below it.

**Superseded the same day — see the guard-scope entry below.** The component
guard tested *binarized* ink, which conflates the rasteriser with the
binarizer; the sizes quoted in this paragraph are where thresholding broke a
stroke, not where rendering did. The corrected floor is 13 px/em.

**A second thing the re-render buys, not previously true.** A two-valued
corpus makes the binarizer a no-op, so an entire pipeline stage had no
benchmark evidence behind it and nothing in the output said so. The pages are
now grey and every figure exercises it.

`bench/pages-cov` is written beside `bench/pages` rather than over it, because
the point of the entry is the side-by-side. The size-sweep and holdout corpora
are still the old two-valued renders and every figure derived from them —
including the size ladder — inherits the bias above until they are re-rendered.

### 2026-09-22 — The component guard was asking the binarizer a question about the rasteriser

The truth-box entry above added a guard reading *"a character whose outline is
a single closed contour must arrive as a single 8-connected component"*, and
counted those components on **binarized ink**. Applied to the size-sweep
corpus it refused `liberation-serif__regular__technical__16px`, because a
serif `E`'s middle arm separates from its stem after thresholding at that
size. 16 px/em is in the shipped bank ladder, so the guard was refusing to
build a corpus at a size the engine is built for.

**The guard was right to fire and wrong about what it was measuring.** Its
stated invariant is about the *rasteriser*: one closed contour cannot draw two
separate marks, so if it did, the stroke fell between samples. Thresholding is
a later and separate stage. An arm that comes away at 16px is not a
rasterisation artefact — it is difficulty a real scan has, and it is exactly
the kind of input the engine exists to handle. Refusing those pages would have
deleted the hardest faces at the sizes the bank is built for, which is the
failure the entry above set out to avoid and reintroduced one paragraph later.

So the guard now counts components of the glyph's **coverage** footprint —
what was drawn, before any threshold — and the binarized count is kept beside
it as `ink_components`, reported rather than asserted. Every generator run now
prints the rate:

```
1052 of 19661 glyphs (5.35%) binarize into more than one mark   (14 px/em)
 990 of 19661 glyphs (5.04%) binarize into more than one mark   (16 px/em)
1016 of 19661 glyphs (5.17%) binarize into more than one mark   (13 px/em)
```

**That ~5% was invisible before this run and it is a real fact about the
corpus.** One glyph in twenty at small sizes reaches the matcher in pieces.
Any per-size figure below about 20px is measuring segmentation on broken marks
as much as it is measuring the matcher, which is worth knowing before another
round of matcher tuning is aimed at "the small-size weakness".

**The corrected floor is 13 px/em**, and it is the height guard that sets it,
not the component guard: at 12px and below the `I` of `INVOICE` in Open Sans
Condensed Light still lands as a 1 × 1 box even with coverage, because the
mark genuinely is not rendered rather than merely thresholded away. 13px
through 48px pass. The size-sweep corpus is therefore rebuilt at 13–24 rather
than the 12–24 it previously used.

**One honesty note on the guard itself.** The coverage-component check has not
been observed to fire at any size from 8 to 48 px — the height guard trips
first, and coverage is connected wherever any subsample lands on the stroke,
which is most of the time. It is a tripwire on a future rasteriser change
rather than a check with a demonstrated catch, and per the standing rule about
guards that never fire, it is recorded as untested rather than quoted as
evidence the rasteriser is sound.

### 2026-09-22 — Head to head on the honest corpus: the character win narrows, the word loss widens

The re-render is done, so here is the side-by-side the truth-box entry owed.
Same 625 pages, same 18 faces, same five render sizes, same engine file
(17,610 prototypes at 16/20/24/32/48), 119,155 characters scored. The only
difference is that the pages are now grey and the truth boxes are the boxes
the engine can find.

| | old corpus, two-valued | **`bench/pages-cov`** | change |
|---|---|---|---|
| OCRcer character accuracy | 93.59% | **93.21%** | −0.38 |
| `ocrs` 0.12.2 character accuracy | 77.20% | **78.97%** | **+1.77** |
| OCRcer word F1, layout-free | 76.15% | **75.23%** | −0.92 |
| `ocrs` word F1, layout-free | 81.20% | **85.86%** | **+4.66** |
| OCRcer oracle character accuracy | 95.67% | **95.17%** | −0.50 |
| OCRcer oracle word F1 | 82.82% | **80.13%** | −2.69 |

**Fixing our own corpus helped the competitor about five times as much as it
helped us, and that is the headline.** A two-valued, aliased page is out of
distribution for a detector trained on ordinary anti-aliased text, so `ocrs`
had been paying for a defect in our generator. It is no longer. The previous
entry's 15.34-point character win was partly ours and partly the renderer's;
the honest figure is **+14.24 points of character accuracy** (93.21% against
78.97%) and **−10.63 points of layout-free word F1** (75.23% against 85.86%).
The word loss is now more than twice what the old corpus reported.

**The two metrics disagree because they are measuring different things, and
both readings are real.** Character accuracy is computed on the page text in
document order, so `ocrs` is charged for reading a two-column invoice out of
order — 56.35% on `invoice`, 50.96% on `statement`. Word F1 ignores order
entirely, and on those same two blocks `ocrs` scores **96.76%** and **92.32%**
against our 69.94% and 64.67%. So: OCRcer reads a document in the right order
and misreads more of the words in it; `ocrs` finds the words and loses the
order. Neither sentence is the whole story and the project's standing rule is
that the losing one gets said first.

**The oracle column is a ceiling again at every render size**, which it was
not an hour ago — see the guard-scope and stage-bypass entries. And what it
shows is the uncomfortable part: handed every glyph box and every space,
OCRcer reaches **80.13%** word F1, still **5.73 points below `ocrs`'s 85.86%**.
At 14px the oracle reaches 64.70% against `ocrs`'s 83.99%. **Segmentation is
therefore not where most of the word-level loss lives.** The residual is
recognition: our character errors are spread thinly across many words, and one
wrong character destroys a word.

Where that residual concentrates, by word F1, oracle column in brackets:
`twins` 57.18% (77.96%), `statement` 64.67% (71.67%), `invoice` 69.94%
(81.00%). The numeric and columnar blocks are the weak ones, and they are the
blocks this engine exists for.

**A measurement this entry does not make.** The `seconds` row of both runs was
taken on a machine that was simultaneously rendering a second corpus and
compiling, so it is not a timing result and is not quoted here. A clean timing
pass on an idle machine stays on the bench backlog.

Every figure above is measured, from
`docs/measurements/2026-09-22_vs_ocrs_625_coverage_corpus_oracle_binarized.txt`.
The superseded run against the two-valued corpus is kept beside it.


### 2026-09-22 — A parameter row the reader cannot honour now refuses the block, and the table rule does not extend to it

The 2026-09-22 `version`-guards-meaning entry settled that an unknown **table
name** is skipped silently, and that rule stands. The `params` block was built
on the same reasoning one level down — it is name-keyed, rows whose names the
build did not know were dropped, and a unit test named
`an_unknown_name_is_ignored_rather_than_refused` recorded the argument:
*"a name a newer file carries and this build does not know runs the code path
it always ran, so ignoring it is safe."*

**That sentence is false, and the difference between a table and a row is
exactly where it fails.** An unknown table degrades to *no argument*: the
reader was never going to consume it, so the engine behaves as the authored
default says. An unknown parameter row degrades to *a different argument*: the
row exists because a default is being overridden, so dropping it substitutes
the default for the authored value and the engine reads the page by a rule the
model file does not describe. The worse the default was, the more useful the
row, and the more damage dropping it does.

**It cost most of a session to find out.** A word-splitter change moved two
parameters that only make sense together — an existing one whose value changed
and a new one that says what the first now falls back to. The model file
carried both. A reader eighteen minutes stale — the bench binaries sit behind
`required-features`, so an earlier `cargo build -p` had skipped them without
saying so — applied the first and silently dropped the second, producing a
configuration nobody had ever authored: distrust the measurement far more
often, and fall back to the old, far-too-high value. It measured as a clean
regression, CER 6.662% to 7.489% and word F1 75.738% to 72.324% on a 49-page
sample, with space deletions doubling. Every figure was self-consistent and
none of them were real. The conclusion being drafted was the large one — that
the offline derivation does not transfer to the pipeline — which would have
been recorded here as a methodological finding on the strength of a binary
nobody had rebuilt.

So section 7 now carries a third bullet: **a parameter row this build cannot
honour refuses the whole `params` block**, by the same all-or-nothing staging
that already refused a malformed one. A value the setter rejects — a NaN —
takes the same path, for the same reason.

**The cost is real and is accepted rather than engineered around.** Adding a
parameter is now a breaking change for an older reader. That is the correct
description of the situation: an older reader genuinely cannot run the newer
file, and the only question was ever whether it says so or quietly answers by a
rule the file does not describe. No `version` bump is needed for the addition
itself — `version` guards *meaning*, and the meaning of every existing table is
unchanged — but an old build will now refuse a new file instead of misreading
it, which is what section 7's first bullet promises for the case it covers.

**And a guard for the half of this the format cannot fix.** The reader was
wrong about the file; nothing in the file could have told it so. The bench
binaries therefore now print a `built` line beside the `model` line, naming the
engine binary's timestamp and the model file's, and saying **STALE** in as many
words when the binary is the older of the two. It is a heuristic and is
labelled one — a binary newer than the model can still predate an uncompiled
source edit — but it catches the case that occurred, and it puts both
timestamps in front of a reader who can judge the rest. A number nobody can
attribute to a build is not a measurement.


### 2026-09-22 — Third-party documents are admissible as evaluation input and inadmissible as model content

Every figure this project has quoted is measured on pages it rendered itself.
That was the only honest option while the corpus had to be licence-clean, and
it has a cost that no amount of care removes: a synthetic corpus is a
measurement against the generator's idea of a document. The operator's actual
domain is an accounting practice whose clients each send something differently
shaped, and none of those shapes are in `crates/ocrcer-build/src/corpus.rs`.

The operator has directed that real documents be taken from the web for
testing, kept outside the repository at `D:/Dev/ExcludedPrivate/ocrcer/`, and
never committed. This entry records the boundary that makes that safe, because
`CLAUDE.md` rule 2 reads at a glance as though it forbids the whole idea.

**It does not, and the distinction is the one the rule is actually about.**
Rule 2 protects the licence case for the *model file*: the tables are an
original work, so no scraped text, no downloaded corpus, no word list of
unknown provenance may become a lexicon entry, a bigram, a confusion rule or a
prototype. Reading a downloaded page and recording that the engine got 94% of
its characters right produces a *number about the engine*. No byte of the
document reaches the model, and the number is not a derivative of the document
in any sense that matters. Input and content are different things and the rule
governs content.

**What stays forbidden, stated positively so it cannot be read narrowly:**

- No word, phrase, number format or abbreviation observed in a private document
  may be authored into the lexicon, the bigram table or the confusion table
  *on the strength of having been observed there*. Authored content must stand
  on a domain justification that would have been written anyway, and the
  provenance column in `model/*.tsv` cites that justification, never a page.
- No text from a private document appears in this repository — not in a
  measurement file, not in a decision-log entry, not in a comment, not as a
  worst-page listing. Real accounting documents carry names, addresses and
  account numbers, and the operator's instruction on that point is explicit.
- A run against the private corpus is filed as **aggregates only**: rates,
  counts, per-size and per-block breakdowns. `--show` output and any per-page
  listing that names a document stay outside the repository with the corpus.

**Ground truth is the hard part and must not be overclaimed.** A downloaded
document does not come with the text it contains. Two sources are admissible
and they are not equally good:

- **A digital-native PDF's embedded text layer.** Cheap, and it is the same
  input `pdfcer` already handles, so it is the realistic case. But a text layer
  is the *producer's claim about what it drew*, not ground truth: reading order
  across columns, ligature mapping, and whether a space was emitted or implied
  by positioning are all producer-dependent. A CER computed against a text
  layer is therefore partly a measurement of the producer, and it is the
  **layout-free word metric that is trustworthy there** — the same argument
  section 11's head-to-head entry already makes about reading order, arriving
  from the other direction.
- **Hand transcription.** Expensive, exact, and the only option for a scan.

Whichever is used is named in the run, and a CER against a text layer is
labelled as such wherever it is quoted. A figure from the private corpus is
never averaged into a figure from the synthetic corpus: they measure different
populations and the combined number would describe neither.

**What this does not change.** The synthetic corpus remains the regression
surface, because it is reproducible byte for byte from committed text and the
private corpus is not. `fixtures/` and section 8.2 are untouched. The private
corpus is for finding out what real documents do that the generator does not,
which is a question the synthetic corpus cannot be asked.

### 2026-09-22 — The word splitter's fallback was its ceiling wearing another hat; separating them and re-deriving the gate is worth +0.98 word F1 on 625 pages

The 2026-09-22 word-splitting entry recorded `lone_gap_x_heights` swept from
1.0 to 0.7 and treated the result as settled. It was not, because the constant
was doing two jobs and the sweep could only see one of them.

`lone_gap_x_heights` was used as a **ceiling** on a threshold the line's own
gap histogram had measured, and as the **fallback** when the histogram was not
believed at all. Those two uses want opposite things. A ceiling should be
generous: it binds only when the measurement is already extreme, and in the
ordinary case a real valley decides. A fallback must be accurate, because when
it fires it *is* the decision and there is no measurement left to defer to.
Measured against ground-truth boxes over all 625 pages, the best-possible
per-page threshold has a median of 0.406 to 0.466 x-heights across the five
render sizes, and the shared 0.7 sat **above** that best-possible threshold on
64.8% to 88.8% of pages. As a ceiling that is correct. As a fallback it merged
away every space on every line that fell back, and `" " -> ""` was the single
largest error class in the engine.

**The split.** `words.lone_gap_x_heights` keeps 0.7 and keeps only the ceiling.
`words.no_valley_x_heights` is new, is 0.40, and is the fallback. Section 3's
word-splitting paragraph is unchanged in substance; `model/params.tsv` carries
a sentence for each.

**`words.min_separability` is 0.7, not 0.8, and the reason is a disagreement
between two metrics that is worth recording in its own right.** The gate
decides how good a valley has to be before the line's own histogram is
believed, so what it is worth depends entirely on where falling back lands —
a demanding gate is unaffordable while the fallback is a ceiling and cheap
once the fallback is accurate. Re-derived on the gap-classification metric
(the share of adjacent-glyph gaps classified wrongly, scored against
ground-truth boxes) 0.80 is the argmax on both the train and holdout corpora.
Re-derived end to end, **0.70 wins every one of ten paired comparisons across
two disjoint 48/49-page samples**, and beats 0.4, 0.5 and 0.6 on both. The gap
metric scores every gap alike; it cannot see that merging two words costs two
tokens while splitting one costs one. Where an isolating metric and an
end-to-end metric disagree, the end-to-end figure decides, and the isolating
metric's answer is kept on the record rather than quietly dropped.

**Measured, all 625 pages of `bench/pages-cov`, same model file, same binary,
the only difference being the parameter values** (the control applied by
`--set`, which reproduces the pre-change build's figures to the digit — that
is what makes the delta attributable):

| | shipped | fallback split | + gate at 0.70 |
|---|---|---|---|
| `no_valley_x_heights` | 0.7 (shared) | 0.40 | 0.40 |
| `min_separability` | 0.6 | 0.80 | **0.70** |
| CER | 6.792% | 6.651% | **6.554%** |
| WER | 28.759% | 28.233% | **27.478%** |
| token recall | 73.891% | 75.446% | 74.929% |
| token precision | 76.620% | 76.508% | **77.528%** |
| word F1 | 75.231% | 75.973% | **76.206%** |

`" " -> ""` falls from 1279 to 998 across the split, a 22% reduction, and the
deficit to `ocrs`'s 85.86% word F1 on this corpus narrows from 10.63 points to
9.65. `"r" -> ""` is unchanged at 192 in every arm, so it is a recognition
error and not the splitter's.

**The episode this nearly became, recorded because the lesson is not about
word splitting.** The first measurement of the split showed a clean 12%
relative CER regression, and the write-up being drafted concluded that the
offline derivation had not transferred to the pipeline — a methodological
claim, about to be recorded here. The engine binary was 18 minutes older than
the model file. It knew `min_separability`, applied the new 0.8, did not know
`no_valley_x_heights`, and dropped it: fall back far more often, to the old
far-too-high value. A configuration nobody had ever authored, measured
perfectly. The parameter-row entry above is the format fix; `ocrcer-bench` now
prints the engine binary's build time beside the model file's, because a
number nobody can attribute to a build is not a measurement.

### 2026-09-22 — A line metric taken as a count over components loses the whole line to its dot leaders, and the synthetic corpus could not see it

Section 3 measures a line from its own members: the modal distance from the
baseline to a component top is the x-height band, and anything much taller is
an ascender or a capital. The feature vector's last two dimensions are height
above and depth below the baseline **in x-heights**, so on this charset `.`,
`·`, `•` and `'` are separated by the line metric and not by the bitmap.

That construction has one failure mode and it is total rather than partial.
On a line like `Total income .................... 1,234.56` the periods are the
**modal population** — thirty-two of them against about twenty letters — so a
count-based mode elects the height of a period as the line's x-height. Every
letter on the line is then measured against a ruler a fifth of the right
length and matches nothing in the bank. The whole line is lost, not the
leaders. Authored diagnostic page, 15/21/30 px, Liberation Sans:
**CER 102.008%** — more wrong characters than the line contains.

This shape is ordinary in the domain section 6 of `FEASIBILITY.md` names:
contents pages, tax forms, statements, indexes. It was invisible because
`bench/pages-cov` contained no instance of it. A `leaders` block has been
added to `ocrcer-build`'s corpus so the gate can see this class at all; the
625-page figures below predate it.

**Two changes, and they act on disjoint inputs.**

1. **Every per-line histogram and median is weighted by component width**
   rather than counted per component — the reference height, the baseline
   mode, the x-height band and the ascender band. The question a line metric
   is asking is which population owns the line's horizontal extent, and text
   owns it even where leaders outnumber it. Weighted and unweighted agree on
   ordinary text, where every component is about the same width, so this is
   not a trade.
2. **A line whose own x-height came out below `lines.inherit_x_height_below`
   (0.5) of the page's typical x-height takes the page's value**, labelled
   `XHeightSource::Inherited`. Width weighting cannot save a line with no
   text on it at all. Who votes is two-tier: lines that *observed* a distinct
   band vote when any exist, because letting converted lines in would let a
   page of leaders confirm its own mistake; where no line observed one —
   small type, an all-caps form — every line votes, and a page of nothing but
   leaders then elects the leader height and changes nothing, which is the
   correct no-op. The baseline is never inherited: a line of full stops sits
   *on* its baseline and measured it correctly.

**Measured.** Diagnostic corpus: `leaders` 102.008% → **16.466%**, `mixed`
~39.9% → **13.861%**, `plain` 3.226% and `allcaps` 2.083% unchanged,
`leaders_only` 195.062% unchanged by design. On `bench/pages-cov`, all 625
pages, effective parameters identical:

| | before | after | inheritance off |
|---|---|---|---|
| CER | **6.554%** | 6.658% | 6.658% |
| WER | **27.478%** | 27.590% | 27.590% |
| word F1 | **76.206%** | 76.104% | 76.104% |

**The loss is reported as prominently as the win, per `CLAUDE.md` rule 8:
this costs 0.102 word F1 and 0.104 CER on the synthetic gate.** The third
column is identical to the second in every digit, so the inheritance pass
changes nothing on that corpus and the entire cost is width weighting's,
paid on 625 pages that contain no case width weighting exists to fix.

On 13 real documents at 300 DPI, isolating inheritance by override, it is
worth **+0.465 word F1 and −0.074 CER** — against exactly zero on the
synthetic gate. Width weighting's own contribution on real documents is **not
measured**: it has no parameter to disable it and the pre-change binary was
not kept, so the case for it rests on the diagnostic corpus and the mechanism.

**The general form, which is the part worth carrying.** Any modal or median
statistic over a text line is a vote, and a vote needs to know its
constituency. Where a class of marks can be numerically dominant while being
typographically irrelevant — leaders, rules, tick boxes, bullet columns — a
count is measuring the wrong thing. And a normaliser derived per line is a
failure amplifier: because the last two feature dimensions divide by
x-height, one wrong x-height does not degrade a line gracefully, it deletes
it. `XHeightSource` naming where the value came from is what made this
diagnosable in minutes; an estimate that reports its own provenance earns the
enum.

Full run output: `docs/measurements/2026-09-22_line_metrics_width_weighting_and_inheritance.txt`.

### 2026-09-22 — An aspect gate on components was built on a measured bound, measured against two corpora, and rejected

`lines.furniture_fraction` rejects a component wider or taller than a fifth of
the page — a page border, a table's outer frame. That bound is relative to the
whole page, so it cannot see a rule drawn inside one cell of a form: a 300 px
cell rule on a 2550 px page is small in absolute terms, survives to be matched
against the charset, and lands on a dash. On 29 hand-checked scans the engine
was emitting three tokens for every one the truth had, and `"" -> "-"` was
among the top insertions, so a scale-free companion gate looked like the fix:
a component whose longer ink extent exceeds N times its shorter one is a rule.

**The bound was measured before any value was chosen.** `ocrcer-build aspect`
renders every charset class on every shippable face at 256 px/em and reports
the extreme ratio of longer ink extent to shorter, taken symmetrically so one
number governs a vertical cell rule as well as a horizontal one. Over 187
classes and 19 faces — 3522 renders — the flattest class in the charset is the
em dash on Open Sans Condensed Light at 236x10 px, **23.60:1**; the extreme in
the other orientation is `|` on the same face at 12x256 px, 21.33:1, and the
next flat class after the em dash is `_` at 12.83:1. Any gate has to sit above
23.60 or it deletes a character the bank can read.

**Measured, and it loses.** 29 hand-checked scans, `column_gap_heights` held
at 1.5, the gate the only thing that moves:

| `lines.rule_aspect` | CER | recall | precision | word F1 |
|---|---|---|---|---|
| **0 (off)** | **109.765%** | 44.809% | **15.134%** | **22.626%** |
| 28 | 110.746% | 44.892% | 14.988% | 22.473% |
| 16 | 112.354% | 44.884% | 14.758% | 22.213% |
| 10 | 113.163% | 44.394% | 14.500% | 21.860% |

Monotonic in the wrong direction, and the column it was built to move —
precision — falls fastest. On the 625-page synthetic corpus the gate at 28 is
inert to every digit (CER 12.024%, word F1 68.471% with it on and off), which
is the paired isolation: the code does nothing where there are no rules, so
the table above is the gate's effect and not a side effect of the change.

**The gate ships disabled at 0.** The parameter, the code and
`ocrcer-build aspect` stay, because the measurement is worth keeping and the
bound is worth having on record. What is not kept is the value.

Two things this entry does **not** claim. It does not claim to understand why
removing components lowers precision — removing emitted tokens should raise
it, and the mechanism is unexplained; the obvious candidate, that a rule
inside a band collapses the band's width-weighted median height and wrecks
every metric taken from it, predicts the opposite of what was measured and is
therefore not the explanation. And the measurement predates
`words.min_valley_x_heights`, which changes how a short segment splits, so it
is worth re-running once that lands. Until it is, 0 is the value with the
evidence behind it.

The larger point stands and is section 9's: precision at 15% is not a
component-filtering problem. The pipeline has no reject anywhere, so every
component that survives the geometric filters is matched to its nearest class
and emitted. A gate that throws away a few more shapes is a rounding error
against that. A match-distance floor is the missing stage, and its value has
to come from the measured distance distributions of glyph and non-glyph
components, not from a plausible number.

Full run output: `docs/measurements/2026-09-22_component_aspect_gate.txt`.

### 2026-09-22 — Scored by somebody else's harness for the first time: 43.40% against Tesseract.js 84.76% and Scribe.js 93.65%

Every accuracy figure this document carried before today was OCRcer scored by
OCRcer: this project's truth conversion, this project's alignment, this
project's definition of a word. That is enough to compare the engine to
itself across a change and it is not enough to compare it to anything else.
A metric of one's own devising flatters whoever devised it, and two engines
can swap places between two defensible definitions of "word accuracy".

The engine now exports hOCR — a new `hocr` bin in `ocrcer-bench`, behind the
same `pages` feature as `ocr` — and `github.com/scribeocr/ocr-benchmark`
scores it: 29 hand-checked pages, their truth, their metric, their code,
cloned to the excluded-private tree and admissible as evaluation input only.

**The harness was validated before it was quoted.** It publishes figures for
two engines nobody here tuned, and it reproduces both exactly on this
machine: Tesseract.js (LSTM) **84.76%**, Scribe.js **93.65%**. That is the
step that makes the third number worth anything; running a harness and
believing the first figure it prints for your own engine buys none of the
credibility and all of the risk. Worth knowing for next time: their published
statistic is the **unweighted mean over pages**, not the pooled ratio — the
same run gives 86.67% and 94.52% pooled, so a "reproduction" off by a point
is an aggregation mismatch, not a harness mismatch.

**OCRcer scores 43.40%** (mean-of-pages; 45.84% pooled) with the column cut
at 1.5, 43.13% at 2.5, and 33.81% with the cut disabled — which is the first
independent evidence that the column cut is worth having on real scans, and
it points the other way from the synthetic corpus, where the same cut is a
pure loss.

**Their metric is a recall, and that changes what the number means.** Per
page it is `correct / total` over *ground-truth* words matched by
bounding-box overlap, punctuation ignored. There is no insertion term at all.
So 43.40% says nothing about spurious output, and — the sharper consequence —
**a change that only improves precision cannot move it.** Measuring a
component filter against this harness and concluding it did nothing is a
measurement error, not a result; `lines.rule_aspect` was measured that way
before this was understood.

This crate's own metric supplies the other half on the same 29 pages at the
same settings: recall 44.809%, **precision 15.134%**, word F1 22.626%, CER
109.765%. The recall figures agreeing to about a point across two independent
truth conversions is the second validation. The precision figure is the
finding.

**Precision 15% at recall 45% is roughly three emitted tokens for every one
the truth has, and it is not a matcher problem.** The pipeline has no reject
anywhere: every component that survives the geometric filters in section 6 is
matched to its nearest class and emitted, so a chart's tick marks, a table's
cell rules and a logo's fragments all become characters with nothing able to
stop them. Per-family, with the cut disabled: `filing` 5.744% CER and
`singlecol` 11.895% against `slide` 453.994%, `table` 168.741% and `chart`
86.205%. Clean single-column text reads well; everything the engine cannot
refuse costs more than the text on the page earns.

**A match-distance floor is the missing stage**, and section 9 now says so.
It is not a threshold to guess: the distance distributions of true glyphs and
of non-glyph components have to be measured on a corpus containing both
before any value is authored, or the floor becomes exactly the kind of
plausible-looking number rule 1 forbids.

One implementation note that cost an hour and is worth carrying: their
importer does `if (!baselineMatch) return ''` and silently drops any
`ocr_line` without a `baseline` in its title. The tell is `missed = total`
with `extra = 0` — every ground-truth word missed and nothing spurious
reported, which is not what a bad OCR result looks like. The two baseline
numbers are slope and offset, and the offset is measured from the line box's
**bottom**, not the page; `baseline 0 0` imports cleanly and is wrong on
every descender.

Full run output: `docs/measurements/2026-09-22_scribe_ocr_benchmark_29_pages.txt`.

### 2026-09-22 — The word splitter believed a valley among intra-word gaps alone; the fix is a floor, swept to 0.3, worth +13 to +19 word F1 on the families the column cut had wrecked

The column cut — each band re-cut at any horizontal gap wider than
`lines.column_gap_heights` times the band's width-weighted median component
height — fixed what it was built to fix and broke something nobody had thought
to look at. Per family on `bench/pages-cov` at 18 px, cut off against cut at
2.5:

| family | word F1 off | word F1 at 2.5 | precision off | precision at 2.5 |
|---|---|---|---|---|
| prose, technical, twins, currency | *identical* | *identical* | | |
| drawing | 81.290% | 81.290% | 81.818% | 81.818% |
| **invoice** | 71.489% | **53.077%** | 78.129% | **46.780%** |
| **statement** | 65.550% | **45.731%** | 69.401% | **36.536%** |

Precision halving while recall barely moves is over-emission, and reading the
engine's own output on one invoice page showed both halves of the cut at once.
It *fixes* under-splitting — `Bracket,6mmplate` became `Bracket, 6mm plate`,
recall on that page 52.0% → 60.0% — and it *causes* over-splitting on the short
segments it creates: `Unit` read as `U n î t`, `Amount` as `A mo u n t`,
`112.50` as `112 .50`, `3.75` as `3 .75`.

**The cause is that a valley search given only intra-word gaps still finds a
valley.** The letter gaps of `Unit` at 18 px are 1, 1 and 3 px; Otsu splits
that at 1, and the between-class variance is a large enough fraction of the
total that `words.min_separability` passes. Separability is scale-free — a
histogram of 1, 1, 3, 1, 2 is exactly as bimodal as one of 2, 2, 40, 2, 3 — so
the test that exists to reject a unimodal line cannot reject a line that has
no word spaces in it at all. The valley is real. The interpretation is not.

**Nothing bound this before**, because any line long enough to hold several
words holds their spaces too. The guard closest to it, `words.min_gaps`, had
been swept over {2, 3, 4, 5} and authored at 3 — in a regime where lines ran
the width of the page and the guard never bound. A clean sweep of a parameter
that never fires says nothing about its value.

**The fix is a floor, symmetric with the ceiling already there.**
`words.lone_gap_x_heights` says no valley may sit where a gap that *wide* is
called intra-word; `words.min_valley_x_heights` says no valley may call a gap
that *narrow* a word space. The floor applies to the **narrowest gap the valley
would call a space, never to the threshold** — the threshold sits at the widest
intra-word gap and is legitimately small on a well-set line, so flooring it
breaks correct lines. (It did: the existing test that a threshold of 1 px is
right for gaps of 1, 1, 1, 9, 9 failed against the first implementation.) A
valley below the floor is rejected to `words.no_valley_x_heights`, the
no-information rule — not to "this segment has no spaces", so a short segment
that genuinely holds one still gets it.

**The coherence argument bounds the value; the sweep chooses it.** At 0.4 the
floor equals `no_valley_x_heights`, so the rule reads: a valley is believed
only if it calls a space the no-information rule would also call a space.
*Above* that the rule contradicts itself — rejecting a valley for calling a gap
a space, then falling back to a rule that calls the same gap a space. So 0.4 is
a ceiling on the value, not a target for it, and the argument says nothing
about where below it the value belongs. The sweep says **0.3**, and the
provenance in `params.tsv` is `measured`.

Swept over {0, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6} on 90 pages of
`bench/pages-cov` at stride 7 — coprime with the five-size ladder, so every
render size is sampled; stride 5 would have selected 14 px and nothing else —
at two settings of the column cut, because the floor and the cut are coupled:
the cut is what creates the single-word segments the floor exists to protect.
0.3 is the argmax of word F1 on **both** arms, cut off 77.503% against 76.660%
at 0.4, cut at 2.5 75.440% against 74.839%, and with the cut on it is also the
CER minimum at 8.569%. Two independent arms agreeing on one value is the reason
this is recorded as measured rather than as the better of two guesses. The
floor is inert at 0.15 and 0.2 with the cut off, identical in every digit.

**The parameter is a staircase, not a curve.** The conversion truncates, so the
value selects an integer pixel floor, and at the x-heights this corpus actually
contains it is coarse: on a 9 px x-height anything from 0.23 to 0.33 is a 2 px
floor and anything from 0.34 to 0.44 is a 3 px floor. So 0.3 rejects the
`112.50` and `Amount` valleys and still believes the `Unit` and `3.75` ones,
which 0.4 catches. Any two values that truncate alike are the same parameter.

**Measured, paired, cut at 2.5.** The floor-off arm reproduces the pre-change
run to every digit, which is what proves the new code inert rather than
compensating:

| 90 pages, all five sizes | floor 0 | floor 0.3 | floor 0.4 |
|---|---|---|---|
| invoice CER | 19.473% | **15.124%** | 14.914% |
| invoice word F1 | 52.206% | **65.388%** | 66.200% |
| statement CER | 23.685% | **17.371%** | 16.621% |
| statement word F1 | 47.864% | **64.635%** | 67.055% |

**The shipped value loses on both families the floor was built for.** 0.4 is
better on invoice by 0.81 points of word F1 and on statement by 2.42, and 0.3
ships anyway because the pooled corpus is what the engine is graded on and 0.3
wins there on both arms — the other five families lose more under 0.4 than
these two gain. That is a deliberate trade against the pdfcer use case, and if
the corpus mix is ever reweighted toward banded documents it has to be
re-swept before it is trusted.

**The floor rehabilitates the column cut without redeeming it.** At
`lines.column_gap_heights` 1.5 on all 625 pages, measured at floor 0.4, the cut
scored 67.160% word F1 before the floor and 74.665% after, against 75.020% with
the cut disabled — a nine-point deficit narrowed to four tenths of a point. It
is still a deficit.
The cut has not paid for itself on the synthetic corpus at any setting tried,
while being worth 33.81% → 43.40% on 29 real scans through the third-party
harness. That disagreement between corpora is the open question.

**Both earlier column-gap sweeps are withdrawn.** They were measured with the
over-splitting bug live, so every value they visited was scored against a
splitter that mis-handled exactly the segments the cut produces.

## The general form

Any threshold derived from a histogram of the data needs a bound in a unit the
domain understands, at *both* ends, because a scale-free goodness-of-split
statistic will accept a degenerate input as enthusiastically as a real one.
And a guard that never binds in the regime it was swept in has not been
validated — when a new stage changes the shape of a downstream input
distribution, every threshold reading that distribution is untested again.

Full run output: `docs/measurements/2026-09-22_word_valley_floor.txt`.

### 2026-09-22 — Two diagnostic modes ignored the flags the parser accepted, and one measurement table had to be thrown away

`ocrcer-bench`'s `ocr` binary has one argument parser and several modes. The
end-to-end mode honoured every flag. `--layout` and `--distances` did not,
because each entry point took `(model, dir, limit)` — no `--only`, no
`--stride`, no `--offset`, and worst, no `--set`, so both read the shipped
parameter block out of the model file whatever override was on the command
line.

**It does not fail. It answers a different question in the same format with
plausible numbers.** Asking for three invoice pages at 18 px returned the first
three files in the directory, which were `currency` at three different sizes.
The page name is printed on every record, so the evidence was on screen the
whole time and was read straight past.

**The tell was a statistic that stopped exactly on a parameter's value.** A
per-family table of "widest gap inside an uncut line" had been built from a
dump taken with `--set lines.column_gap_heights=0`; the override never applied,
so the dump was post-cut and several independent families reported a maximum of
exactly 2.50. A natural distribution does not stop dead on a round number that
happens to equal a shipped parameter. The second tell, weaker and encountered
first, was varying the flag and watching nothing move — which read at the time
as "this page is unaffected by the cut" and actually meant "the flag is not
wired".

**A flag the parser accepts and a mode ignores is a lie the tool tells.** The
fix routes every mode through one `select_pages` and one `engine_with`, both of
which print what they selected and what they overrode; the signature
`fn run_x(model, dir, limit)` is itself the defect, because a hand-picked
subset of the options struct drifts the moment a flag is added. The alternative
discipline — reject at parse time when a mode cannot honour a flag — is equally
sound. Silently dropping it is the only option that manufactures evidence.

Every conclusion drawn from an affected mode had to be re-examined against one
question: did this claim depend on a flag that mode ignores? A diagnostic run
over a small purpose-built corpus where "the first N pages" and "the pages I
wanted" coincide is unharmed; a run that named a family, a size, or a parameter
value is not. One table was discarded and re-measured, and the entry below is
the result.

### 2026-09-22 — The column cut's gap populations are measured, and the asserted sentence had named both of them wrong

`lines.column_gap_heights` was authored against a sentence about three
populations: a word space "about 0.2" median component heights, the widest
tabbed gap inside a set line "about 1.5", and a form's box boundary "several
times that". The first two are now measured, over 384 uncut lines on 57 pages
of `bench/pages-cov`, with every gap classified by the word splitter's own
per-line threshold rather than by a hand-drawn cut:

| | intra-word p50 | intra-word p90 | space p50 | space p90 | space max |
|---|---|---|---|---|---|
| currency, prose, technical, twins | 0.14–0.17 | 0.25–0.30 | 0.50–0.67 | 1.08–1.36 | 1.25–1.64 |
| drawing | 0.14 | 0.25 | 0.54 | 1.11 | 2.86 |
| invoice | 0.14 | 0.30 | 1.36 | 6.56 | 18.77 |
| statement | 0.14 | 0.31 | 1.17 | 11.50 | 24.12 |

**Both asserted figures named the wrong population.** "About 0.2" is the
*intra-word* gap, not the word space; a word space is 0.57 at the median across
the five families with no banded rows. "About 1.5" is the *ceiling on running
text*, not the tabbed column gap; the widest gap inside one banded row is 3.25
median heights on an invoice and 5.30 on a statement. Both errors understated
the separation the parameter depends on, which is the direction that does not
bite, but a number that reads as measured and is not is exactly what rule 1
forbids.

**The prediction the measurement makes, and the test of it.** If the widest gap
on a family never reaches 2.5, the cut at 2.5 cannot fire there and the family
must score identically with the cut on and off. Measured on 90 pages each, to
every digit: currency 82.367% word F1 both ways, prose 89.425%, technical
77.766%, twins 57.610%. Drawing is the one family without banded rows whose
tail crosses 2.5, and it is the one that moves — word F1 82.198% → 82.797%,
CER 4.121% → 4.528%, a small gain in words paid for in characters. A dump that
predicts an end-to-end result and is then checked against it is worth more than
the dump alone, and this one also re-validates the repaired harness.

These are rendered pages. A scan's gaps carry the additional spread of its own
binarisation, and the 29-scan corpus is where the cut and the synthetic corpus
disagree. **The parameter's value remains a guess**: the sweep that would
choose it was invalidated when `words.min_valley_x_heights` changed under it.
What is now measured is the populations it sits between, not where between them
it belongs.

Full run output: `docs/measurements/2026-09-22_column_gap_populations.txt`.

### 2026-09-22 — A knob existed in four places and the build assertion compared one pair; the stage defaults now delegate to the parameter block

The build test asserts `model/params.tsv` against `Params::DEFAULT`, and it
works — a table edit without the matching constant fails the build with the
knob named and both values printed. It does not reach the three per-stage
`Params` structs in `layout/`, each of which carried its own `impl Default`
restating the same numbers. Four copies of every layout knob, one comparison.

`words.min_valley_x_heights` had drifted: the table and the stage default said
0.3, `Params::DEFAULT` still said the 0.4 the sweep had moved off. The shipped
`.ocrw` was checked directly before anything was changed — the float at that
name's offset decodes to 0.30000001192092896 — so no measurement was taken at
a value other than the one reported, and **nothing in the benchmark record is
mislabelled**. The engine reads its parameters from the model file, which is
generated from the table, so a stale constant cannot reach a scored run. What
it can reach is a unit test: anything calling `Params::default()` was asserting
behaviour at a configuration that never ships, and passing.

The fix is deletion rather than a second assertion, which would have left five
copies with two comparisons. Each stage `Default` now returns
`crate::params::Params::DEFAULT.words()` (or `.lines()`, `.segment()`) —
roughly thirty literals removed, and the drift made unrepresentable instead of
detected. **The parameter block is the single definition of every knob**; a
stage that wants a default takes it from there. Restating a value in a stage
module is a second definition that nothing compares, and section 4's
write-each-stage-once rule applies to the constants a stage reads exactly as it
applies to the code that reads them.

### 2026-09-22 — A banded row came back right-hand-fragment first, worth up to 8.8 CER points, and every word metric was blind to it

Reading order was decided after the column cut, by sorting the finished lines
on their top edge. Two fragments of one banded row do not share a top edge —
whichever holds the taller glyph starts higher — so a label-and-value row came
back value before label. That is what an invoice, a statement and a receipt are
made of.

Found on a photographed receipt: truth `DATE : 15/01/2019 11:05:16 AM` returned
as `; w01/2019 11:0516 AM` then `Date`, and with the cut disabled the same page
returned one line in the right order. The fragments sat at x 30..63 and
x 124..324 — emitted widest-first.

**The fix settles order on whole rows, before the cut.** The bands are sorted
while a row is still one object and each is then cut left to right, so the
fragments leave the stage adjacent and in order and there is nothing to sort
afterwards. For a row that is never cut the two orderings are identical — the
band's bounding box is the line's, and both sorts are stable — which makes the
change a provable no-op wherever the cut does not fire, and that is the control.

Measured on all 625 pages of `bench/pages-cov` at the shipped floor:

| `column_gap_heights` | CER before | CER after | word F1 before | word F1 after |
|---|---|---|---|---|
| 0 (cut off) | 6.732% | 6.732% | 76.023% | 76.023% |
| 1.0 | 22.256% | 13.434% | 67.509% | 67.509% |
| 1.5 | 11.263% | 8.505% | 74.971% | 74.971% |
| 2.0 | 10.381% | 8.412% | 74.363% | 74.363% |
| 2.5 (shipped) | 10.091% | 8.330% | 74.234% | 74.234% |
| 3.0 | 9.449% | 8.277% | 73.736% | 73.736% |
| 4.0 | 9.025% | 8.380% | 72.433% | 72.433% |
| 6.0 | 8.329% | 7.930% | 73.189% | 73.189% |

Word recall and precision are identical at every arm as well, to every digit
printed.

**The shape of that table is the finding, not the size of it.** Every
order-insensitive metric is unchanged everywhere; every order-sensitive one
improves wherever the cut fires. That is the signature of a pure reordering:
the same words, in the same numbers, on the same pages, in a different order.
And it means the defect was invisible to the metric most often quoted about
this stage — word F1 was 74.234% before and 74.234% after. The layout question
"did the cut split in the right places" is naturally asked of a word score, and
a word score cannot answer it. **A metric that is invariant under an error
class is not evidence about that error class**; reading order needs CER or an
explicit order assertion, and nothing else in the suite will catch it.

One fixture expectation was corrected rather than blessed.
`two_boxes_on_the_same_row_are_two_lines` asserted taller-box-first, justified
by *"the heading's top edge is the higher of the two"* — a restatement of the
sort key rather than a claim about the page. A form row is read left to right
whichever box holds the larger type. That is section 8.2's named failure: an
expectation authored from the implementation, which then defends the
implementation.

**The first run of this comparison was false and nearly went into the record.**
It returned byte-identical numbers at every arm, which is exactly what the
no-op control was designed to look like. The binary had not been rebuilt: the
`ocr` bin sits behind `required-features = ["pages"]`, which was not in
`default`, so `cargo build --release` skipped it silently while its crate
printed "Compiling". This is the second false measurement from that same
feature gate; the first is recorded above as a clean regression that was not
real. `pages` is now in `default`, so a plain build builds the measurement
binaries. The mtime check that works is against the *changed source file*, not
against the clock, and the check that actually settled it was making the binary
emit something new — one extra field in the layout dump — and looking for it.

Full run output: `docs/measurements/2026-09-22_reading_order_after_the_column_cut.txt`.


### 2026-09-22 — The column cut's precision collapse is spurious spaces inside short fragments, not a mis-measured x-height

**Supersedes the leading hypothesis** carried in `RESUME.md` and implied by the
`lines.column_gap_heights` row: that a short fragment re-measured on its own
gets the wrong x-height. Tested on the 90 invoice pages of `bench/pages-cov`,
binary rebuilt from current source, and it is false. Taking each page's
reference as the median x-height of its `Observed` lines of ten or more
members, fragments of 3–5 members are off by more than 15% in 6.6% of cases and
fragments of 6–9 in 2.8%. The *uncut* lines are the badly measured ones: 46.2%
of them miss by more than 15% with the cut off, because a merged row mixes
sizes. The cut improves x-height measurement; it does not damage it.

**What the cut actually does, by the confusion table on the same pages:**

| | cut off | cut 2.5 |
|---|---|---|
| deleted spaces `" " -> ""` | 721 | 377 |
| inserted spaces `"" -> " "` | not in top six | **727** |
| words produced (truth: 4,500) | 3,742 | 4,887 |

It halves the deleted spaces — the recall gain is real — and inserts about as
many as it removes. Examples at 14 px: `112.50` → `1 12 . 50`, `450.00` →
`450 . 00`. The mechanism is the word splitter's space rule run on a
fragment's own gaps. A one-word fragment has one gap population; with six or
seven samples Otsu reports high separability anyway (0.83–0.96 on the examples
dumped), so a valley is believed between intra-word gaps of 1 px and 3–4 px,
and the `min_valley_x_heights` floor of 0.3 × 7.43 truncates to 2 px, below
them. Where separability does fall, the 0.4 x-height fallback sits below the
wide side bearings of `1` and `.` in a monospace face. On a full-width line
the real spaces pull the valley upward; the cut removes them. The params note
on `min_valley_x_heights` had already recorded the symptom (`3.75` and `Unit`
still split) without the cause.

**Decision on the direction, not yet on the implementation.** The space rule
for a cut fragment is to be estimated from the band it was cut from, not from
the fragment alone: pool the band's intra-fragment gaps, each divided by its
own fragment's x-height so mixed sizes stay commensurable, excluding the gaps
the cut fired on; run the valley search once on the pool; apply the result to
each fragment as a multiple of that fragment's x-height. A fragment's own gaps
stay the source only when it is the whole band. This mirrors how x-height
falls back to `Inherited` when a line cannot measure itself, and it keeps the
per-fragment x-height the cut exists to obtain. It is an algorithm change in
`layout/words.rs` for `ocrcer-runtime`, touches no charset, feature or
normalisation constant, and needs no `.ocrw` version bump. **It is a proposal
until measured**: the gate is inserted spaces on invoice falling to near the
cut-off level while deleted spaces stay near 377, then all 625 pages.

`lines.column_gap_heights` stays `guess`. A sweep taken now would pick the value
where this defect costs least, not where the threshold belongs.

Evidence: `docs/measurements/2026-09-22_column_cut_precision_collapse.txt`.

### 2026-09-22 — Band-pooled space rules measured: a third of the spurious spaces removed, as many real ones lost; gate not met, fixed-pitch detection is next

**Follows** "The column cut's precision collapse is spurious spaces inside short
fragments". The band-pooled rule was implemented exactly as specified there
(`layout/words.rs` `band_space_rules`, wired into `Engine::recognize_lines` and
`ocr --layout`) and measured on the 90 invoice pages of `bench/pages-cov` at
`column_gap_heights` 2.5, binary confirmed newer than every edited source.
Readings: inserted spaces 727 → 503, deleted spaces 377 → **498**, word F1
65.388% → 65.234%, CER 12.600% → 12.306%, WER 49.511% → 46.978%. The cut-off
arm is byte-identical to before, as it must be. **The gate is not met** —
inserted spaces were to fall near 20 with deletions held near 377 — so the
625-page run was not taken.

**Why pooling cannot meet it.** An invoice band holds several narrow numeric
column fragments and one or two text fragments. Pooled, the many tight
intra-number gaps outvote the few real word spaces and the shared valley rises
past some of them. Pooling moves one threshold between two fragment kinds that
want different thresholds; it trades one error for the other rather than
removing either. That is a property of the algorithm, not of its
implementation.

**Decision.** The pooled rule stays in the code path provisionally: it is
neutral on F1 and better on CER and WER, and reverting it buys nothing. It is
not declared done. The spurious spaces it leaves are dominated by the case the
dumped examples show — `112.50` → `1 12 . 50` in a monospace face, where a
narrow glyph's wide side bearings make an intra-word *gap* look like a space.
Gap width is the wrong measurement for fixed-pitch text; **cell position is the
right one** (Tesseract's textord tests each row for fixed pitch and chops by
pitch position instead of by gap; survey in
`docs/measurements/2026-09-22_research_classical_techniques.md` §1).

**Specified for `ocrcer-runtime`, a proposal until measured:**

1. Per fragment (a whole line when uncut), take the character boxes left to
   right and the centre-to-centre distances `d_i` between neighbours.
2. Pitch estimate `p` = median of the `d_i`. The fragment is **fixed-pitch**
   when it has at least `words.pitch_min_glyphs` boxes and at least
   `words.pitch_agreement` of the `d_i` lie within `words.pitch_tolerance × p`
   of an integer multiple `k·p`, `k ≥ 1`. Centre distance is used because it
   is invariant to glyph width, which is exactly what the gap is not.
3. On a fixed-pitch fragment the space rule is replaced: a space stands between
   neighbours iff `d_i ≥ 1.5 p` (an empty cell). The gap-valley rule, pooled
   or not, is not consulted.
4. Otherwise the existing rule applies unchanged.

New parameters, all `guess` in `model/params.tsv` and on chunk 8's tuning
list: `words.pitch_min_glyphs` 6, `words.pitch_agreement` 0.8,
`words.pitch_tolerance` 0.15. Setting `pitch_min_glyphs` to 0 disables the
test, and that arm must be byte-identical to the current build. No charset,
feature or normalisation constant changes; no `.ocrw` version bump — the params
block carries its own names, confirm the parser tolerates added keys, and
escalate here if it does not.

**Gate:** on invoice at `column_gap_heights` 2.5, inserted spaces fall well
below 503 with deletions no worse than 498 and word F1 above 65.388%; the
cut-off arm must not regress on any family. Then all 625 pages at both arms.

**A separate finding, recorded not acted on:** at 2.5 the largest single
confusion is `" " -> "\n"` (768 on invoice) — fragments of one band are emitted
as separate lines where the truth keeps the row on one. That is an output
serialisation question (row-major vs column-major for a cut band), not a
segmentation error, and it touches CER only. It is not to be tuned against the
metric; it waits until the space rule is settled.

Evidence: `docs/measurements/2026-09-22_band_pooled_spaces.txt`.

### 2026-09-22 — The fixed-pitch rule is precise and net-positive; two defects found in it, and the amendment specified

**Follows** "Band-pooled space rules measured … fixed-pitch detection is next".
Readings, `bench/pages-cov`, binary newer than sources: invoice at
`column_gap_heights` 2.5, inserted spaces 503 → 436, deleted 498 → 492, word F1
65.234% → 67.394%; all 625 pages at 2.5, word F1 74.356% → 75.298%, CER
8.172% → 8.053%; at 0, word F1 76.023% → 76.081%. The `pitch_min_glyphs = 0`
control is byte-identical to the prior build. **The gate's no-regression clause
failed on one family:** statement at 0, word F1 68.278% → 67.594%, deletions
292 → 323.

**The detector itself is precise.** The corpus names its font, so the
classification has ground truth: 7 monospace families, 11 proportional. At 0,
fragments on monospace pages classed fixed-pitch 1,431 of 1,620 (recall 88.3%),
on proportional pages 38 of 2,585 (1.5% false positives); at 2.5, recall 78.2%
(the cut pushes more fragments under the six-glyph floor), false positives
1.6%. The earlier aggregate "fires on a third of fragments, prose included" was
the two populations blended.

**Defect 1 — an all-caps proportional run passes the test.** Every proportional
family regresses on statement; every monospace family is flat or better
(roboto-mono +15.98 F1, inconsolata +9.70). The mechanism is the page title
`ACCOUNT STATEMENT`: capitals in a proportional face have near-uniform advances,
80% of centre distances land within 15% of the median, and the one real word
gap — centre distance about 1.3 p — falls under the 1.5 p empty-cell cut and
fuses. **The signature is that the space is off the grid.** In a true
monospace fragment a gap between glyph centres is an integer number of cells;
1.3 p is not.

**Defect 2 — a component is not always a character cell.** On a genuinely
monospace invoice row (cascadia-mono and fira-code, every size) the centre
distances contain `0.00` and `0.50 / 9.50` pairs: two components sharing one
cell, as a colon, an `i`'s dot, `%`, `=` or a glyph broken by the binarizer
produce. Counted as two positions, they perturb the neighbouring distances and
the row splits into seven words where the truth has two. The pitch estimate
(`p = 11`, the face's real advance) and the classification are both right; the
input to them is not.

**Amendment, specified for `ocrcer-runtime`, a proposal until measured:**

1. *Cell positions, not components.* Before computing centre distances, merge
   neighbouring components whose horizontal extents overlap into one position
   whose box is their union. Overlap in x is the definition of sharing a cell
   and needs no new parameter.
2. *Grid consistency.* A fragment is fixed-pitch only if, in addition to the
   existing test, **every** centre distance `d ≥ (1 + pitch_tolerance) · p`
   lies within `pitch_tolerance · p` of an integer multiple `k · p`, `k ≥ 2`.
   One off-grid wide distance is proportional spacing, and the fragment falls
   back to the gap rule. This also uses only the existing tolerance.
3. The empty-cell threshold stays `1.5 p`.

No new parameters; charset, features, normalisation and `.ocrw` untouched.
**Gate:** statement at 0 no worse than the pitch-off 68.278% F1 and 292
deletions; `ACCOUNT STATEMENT` splits on every proportional font; invoice at
2.5 at or better than 67.394% F1 and 436 insertions; proportional
false-positive rate below 1.5% with monospace recall not below 88.3% at 0; then
all 625 pages at both arms. The residual 436 insertions are not fully
accounted for; `--layout` should print each fragment's decoded words so the
remainder can be attributed rather than inferred from whole-page text.

Evidence: `docs/measurements/2026-09-22_fixed_pitch_spaces.txt`, sections
"Gate assessment" and "Detector diagnosis".

### 2026-09-22 — The grid check was all-or-nothing and cost a third of monospace recall; replaced by a vote over the wide distances that excuses touching pairs

**Follows** "The fixed-pitch rule is precise and net-positive; two defects found
in it". The amendment specified there was implemented and **failed its gate**:
monospace fixed-pitch recall at `column_gap_heights` 0 fell 88.3% → 62.8%,
though proportional false positives fell 1.47% → 0.19%. An ablation with two
diagnostic toggles (`words.pitch_cell_merge`, `words.pitch_grid_check`, both
`tune = no`) separates the parts. Readings at 0, recall on monospace / false
positives on proportional:

| | mono recall | prop FP |
|---|---|---|
| neither | 88.3% | 1.47% |
| merge only | **94.8%** | 1.62% |
| grid only | 62.5% | **0.19%** |
| both | 62.8% | 0.19% |

**Cell merge is sound and stays** — on its own it lifts recall six points; only
12 of 2,524 merged pairs on monospace pages fused two real cells. **The grid
check is the whole regression**, because it rejects a line for one off-grid
wide distance. Two routine things produce one: ink centres that are not cell
centres (`1`, `.`, `-`), and — in 8 of the 10 rejected lines dumped — two
anti-aliased neighbours already joined into one component about 2 p wide by
binarization, whose centre sits half a cell off and makes both its neighbour
distances about 1.5 p. The old majority test absorbed these; an
all-or-nothing test turns each one into a whole-line rejection. The specified
test was wrong to be unconditional, and that is this log's error, not the
implementation's.

**Replacement, specified for `ocrcer-runtime`, a proposal until measured:**

1. Keep the cell merge.
2. A component at least `(2 − pitch_tolerance) · p` wide is a *touching run*
   of `round(width / p)` cells. A wide distance with a touching run at either
   end is excused from the grid vote.
3. Among the unexcused wide distances (`d ≥ (1 + pitch_tolerance) · p`), at
   least `pitch_agreement` must lie within `pitch_tolerance · p` of `k · p`,
   `k ≥ 2`. None unexcused means the vote passes. This reuses the existing
   agreement fraction; no new parameter. `ACCOUNT STATEMENT` still fails: one
   unexcused wide distance, off the grid, is 0 of 1.
4. The empty-cell threshold stays `1.5 p`. An excused distance next to a
   touching run is measured from the run's nearest cell centre, not its box
   centre, so a touching pair does not manufacture a space.

**Gate**, all on `bench/pages-cov` against the recorded pitch-on/amendment-off
build: monospace recall at 0 at least 88.3%; proportional false positives at
0 at most 0.5%; statement at 0 F1 at least 68.278% (the pitch-off level) with
deletions at most 292; invoice at 2.5 F1 at least 67.394%; then all 625 pages at
both arms. The two diagnostic toggles stay and must reproduce the ablation
rows above when set.

Evidence: `docs/measurements/2026-09-22_fixed_pitch_spaces.txt`, sections
"Amendment: cell merge + grid consistency" and "Amendment ablation".

### 2026-09-22 — The grid vote failed too (monospace recall 66.6%); pairwise distances are the wrong statistic, replaced by a fitted grid, with a bounded fallback

**Follows** "The grid check was all-or-nothing …". Implemented as specified;
`pitch_grid_check = 0` reproduces the "merge only" row exactly (94.8% / 1.62%),
so the reading is genuine. At `column_gap_heights` 0: monospace recall
**66.6%** (gate 88.3%, failed), proportional false positives 0.19% (pass);
statement F1 68.844% (pass) with deletions 296 (gate 292, failed); invoice at
2.5 F1 69.860% (pass). `ACCOUNT STATEMENT` still fuses on 4 of 55 font/size
pairs, where its word gap is under `1.15 p` and never counts as wide.

**Why a vote on pairwise distances cannot work.** A centre distance carries the
placement error of *both* its ends, so one off-centre glyph (`1`, `.`, `-`)
spoils two distances, and a short line has only one to three wide distances to
vote with — one spoiled distance fails it. Excusing touching runs addressed
half of the co-occurring pair and left the other half. That is the third
formulation to fail on the same underlying mistake, and it is the statistic,
not the constants, that is wrong.

**Replacement — fit the grid, then test positions, not differences.** A
monospace line is a grid; its defining property is that every cell centre sits
near `x₀ + n·p`, and a proportional line's defining failure is that its error
*accumulates* along the line.

1. Cell positions as now (cell merge kept); a touching run of `m` cells
   contributes `m` virtual centres at its box's equal subdivisions.
2. Assign cell indices sequentially, `n₀ = 0`, `nᵢ = nᵢ₋₁ + max(1, round(dᵢ / p₀))`
   with `p₀` the median distance as today; then fit `x = x₀ + n·p` by least
   squares over all positions to get `(x₀, p)`.
3. The line is fixed-pitch when it has at least `pitch_min_glyphs` positions
   and at least `pitch_agreement` of them lie within `pitch_tolerance · p` of
   their fitted `x₀ + nᵢ·p`. One off-centre glyph now costs one position of
   many; a proportional word gap of `1.3 p` displaces every later position by
   `0.3 p` and fails a large fraction. The pairwise grid vote is removed.
4. A space stands wherever the index advances by two or more (`nᵢ − nᵢ₋₁ ≥ 2`)
   — an empty cell, which is what `d ≥ 1.5 p` approximated.

No new parameters; `pitch_grid_check = 0` now means "skip step 3's residual
test and use the old median-agreement test", preserving the ablation row.

**Gate**, readings at 0 unless stated: monospace recall ≥ 88.3%; proportional
false positives ≤ 0.5%; statement F1 ≥ 68.278% with deletions ≤ 296 (the
ceiling is relaxed from 292 to the grid-vote reading, because four of those
deletions are `ACCOUNT STATEMENT` pairs whose gap is under `1.15 p` and no grid
test can separate them from a monospace neighbour pair — accepted, recorded);
invoice at 2.5 F1 ≥ 67.394%. Then all 625 pages at both arms.

**Bounded fallback, decided now so it is not re-litigated.** If this fails
its gate, the shipped configuration becomes cell merge with
`pitch_grid_check = 0` (the "merge only" row: recall 94.8%, false positives
1.62%), the statement title regression is accepted and recorded as a known
loss, and the grid question goes to the backlog behind the other work in
`RESUME.md` §2. No fourth formulation this cycle.

Evidence: `docs/measurements/2026-09-22_fixed_pitch_spaces.txt`, section "Grid
vote".

### 2026-09-22 — The fitted grid failed on a wrong premise of this log's; the bounded fallback ships: cell merge, no grid test

**Follows** "The grid vote failed too …". Readings at `column_gap_heights` 0:
monospace recall 90.6% (pass), proportional false positives **2.48%** (gate
0.5%; 8.55% at 2.5), `ACCOUNT STATEMENT` fused on more font/size pairs than
before. **Failed.**

**The premise was wrong, and it was this log's.** The entry claimed a `1.3 p`
word gap displaces every later position and fails a large fraction of them.
It does not under a least-squares fit: the fit absorbs a single step into its
slope and intercept, splitting the `0.3 p` error about evenly across both
halves, so no position is displaced by more than about `0.15 p` — inside the
tolerance. A fitted grid is *less* sensitive to one proportional word gap than
a pairwise test, the opposite of what was asserted. `ocrcer-runtime` found this
and flagged it rather than tuning around it.

**Outcome, as pre-decided.** `words.pitch_grid_check` defaults to 0 in
`Params::DEFAULT` and `model/params.tsv`. The shipped rule is the median
agreement test with cell merge and the `1.5 p` empty-cell split, reproducing
the "merge only" row: monospace recall 94.8% at 0 and 85.6% at 2.5,
proportional false positives 1.62% and 1.80%. **Accepted and recorded loss:**
all-caps proportional runs of uniform advance pass the test and their word gap
fuses — `ACCOUNT STATEMENT` on 29 of 55 font/size pairs; statement at 0 reads
F1 68.078% against 68.278% with pitch off. The fitted-grid code path is
retained only if it is reachable from the toggle; otherwise it is removed.

**What a real fix would need, for the backlog, not this cycle:** the
discriminator is the *width* of the anomalous distance relative to intra-word
spread, which is a gap statistic, not a grid statistic — or the decoder, which
could score the joined and split readings of a line the rule is unsure of and
let the lexicon decide (Tesseract's fuzzy spaces). Neither is attempted now.

Evidence: `docs/measurements/2026-09-22_fixed_pitch_spaces.txt`, section
"Fitted grid".

### 2026-09-22 — The shipped fixed-pitch rule is a net gain on all 625 pages; the column cut now wins on words and loses only on CER, and the remaining CER gap is specified as a serialisation fix

**Follows** "The fitted grid failed …". The full-corpus comparison, interrupted
once by the host reaping a background shell for low memory and resumed in the
foreground at Ken's request, binary newer than every source. Readings,
`bench/pages-cov`, 625 pages:

| | pitch on (shipped) | pitch off |
|---|---|---|
| cut off | F1 76.200%, CER 6.698% | F1 76.023%, CER 6.732% |
| cut 2.5 | F1 **76.332%**, CER 7.760% | F1 74.356%, CER 8.172% |

The fixed-pitch rule, as shipped with its recorded title loss, is a gain at
both arms. **With it, `column_gap_heights` 2.5 now beats the cut-off arm on
word F1** — the first time the cut has won on the order-insensitive metric —
and trails only on CER, by 1.06 points.

**What the CER gap is.** At 2.5 the largest single confusion on invoice is
`" " -> "\n"` (768 of them): the cut band's fragments are emitted as separate
output lines where the page has one visual row. Reading order is settled on
bands before the cut, so the fragments of a band are already emitted
consecutively, left to right; only the separator between them is at issue. A
band is one row of ink, and the separator should say so.

**Decision, specified for `ocrcer-runtime`:** fragments cut from one band stay
distinct `Line`s in the API — their boxes, x-heights and space rules are the
point of the cut — but each carries its band index, and the text rendering
joins consecutive fragments of one band with a single space and ends the band
with one newline. This changes no word, no word order and no box; it cannot
alter word recall or precision, and the gate says so: at 2.5 word P/R/F1
identical to the table above to the last digit, `" " -> "\n"` falls by the
number of cut points, CER falls. No parameter, no format change.

**Not tuned against the metric.** This is a correction to what the output
claims about the page, argued from the page, and it would be made the same way
if CER did not exist. Once it lands, `column_gap_heights` is re-decided from a
sweep of both metrics; until then it stays `guess` at 2.5.

Evidence: `docs/measurements/2026-09-22_fallback_full_corpus.txt`.

### 2026-09-22 — `lines.column_gap_heights` is decided at 1.75 from a sweep of both metrics on two corpora, and becomes `measured`

**Follows** "The shipped fixed-pitch rule is a net gain …". The band
serialisation fix landed as specified: at 2.5, word P/R/F1 unchanged to the
last digit, CER 7.760% → 6.653%, and the cut-off arm byte-identical (evidence
`docs/measurements/2026-09-22_band_serialisation.txt`). With both of the
cut's known defects addressed, the value is chosen now. Readings,
`bench/pages-cov`, 625 pages, binary newer than every source:

| cg | word F1 | CER |
|---|---|---|
| 0 | 76.200% | 6.698% |
| 1.0 | 72.716% | 7.348% |
| 1.25 | 75.405% | 6.720% |
| 1.5 | **76.794%** | **6.458%** |
| 1.75 | 76.675% | 6.503% |
| 2.0 | 76.489% | 6.554% |
| 2.5 | 76.332% | 6.653% |

On `finfilings` (60 pages of real printed financial filings, in domain, never
scored before): 0 → F1 62.976%, CER 20.902%; 1.75 → F1 68.277%, CER 19.544%;
2.5 → F1 68.285%, CER 19.544%.

**Decision: 1.75.** Both metrics agree on the synthetic corpus and peak at 1.5,
but 1.5 sits one step from a cliff — 1.25 loses 1.4 F1 points and 1.0 loses
four. 1.75 is within 0.12 F1 and 0.05 CER of the peak with a quarter-step more
margin from it, and the real corpus cannot tell 1.75 from 2.5 while both beat
the cut-off by 5.3 F1 and 1.4 CER. Choosing the plateau over the edge is the
choice that survives a corpus whose gap distribution differs from the one it
was swept on. Provenance becomes `measured`, citing both files.
`params.tsv`, `Params::DEFAULT` and the shipped `.ocrw` change together; no
format change.

**Recorded, not acted on:** `finfilings` at 19.5% CER is three times the
synthetic corpus's error; one 60-page run takes on the order of ten minutes.
Both need a diagnosis before anything is concluded about the engine on real
filings.

Evidence: `docs/measurements/2026-09-22_column_gap_sweep_final.txt`.

### 2026-09-23 — A column fragment must hold two components; the leader-line dicing the 1.75 cut exposed is a rule gap, not a fixture error

**Follows** "`lines.column_gap_heights` is decided at 1.75 …". After the default
moved to 1.75, five `ocrcer-core` unit tests failed: four in line layout (a
leader-only line, a page of nothing but leaders, and the two no-observed-line
inheritance cases) and one in the word splitter (a single wide gap). Adjudicated
by reading each fixture against the rule, not by re-blessing.

**What happened.** The leader fixtures set 2×2 dots at a pitch of 6, so gaps of
4 px, i.e. 2.0 dot heights. A leader-only band's width-weighted median height
*is* the dot height, so at 1.75 the cut threshold is 3.5 px and every gap
clears it: a line of 30 dots becomes 30 one-dot lines. At 2.5 the threshold was
5 px and the geometry happened to sit under it. The fixtures are faithful to
real leaders — period-to-period spacing of two to three dot heights is ordinary
— so the fixtures are right and the rule under-specified this case. The
width-weighting written for mixed text-and-leader lines only protects a band
whose text owns most of its width; a table-of-contents line whose leader run
outweighs its title had the same exposure at 2.5 and has it worse at 1.75.

The word-splitter fixture puts one letter 2.2 heights beyond a two-letter word
(`[0, 10, 40]`, 8×10 glyphs). At 1.75 that is a column cut; the test is about
the fallback space rule, and one letter alone across a gap is a word, not a
column.

**Decision, specified for `ocrcer-runtime`** (§6's line-layout paragraph edited
in the same change): every gap wider than the cut is a candidate; a candidate is
kept only if both fragments it would bound, as formed by all candidates
together, hold at least two components. A candidate adjacent to a singleton is
dropped, and fragments either side of a dropped candidate stay joined. This is
one pass over the candidate list — no iteration — so it is deterministic and a
function of the component set alone. No parameter, no format change, and none of
the five fixtures changes: all five pass under the rule with their geometry and
expectations as authored. Rejected: flooring a band's scale at a page-level
median gated by `lines.inherit_x_height_below`. It repairs leaders beside text
but still dices a page of nothing but leaders, and it borrows a ratio defined on
x-heights for a comparison made on raw component heights.

**Cost, to be measured, not assumed.** The rule also stops cuts next to a lone
table digit or checkbox mark, which the cut currently makes. Gate: `cargo test
--workspace` green; `bench/pages-cov` shipped-default reading taken against the
1.75 control (F1 76.675%, CER 6.503%) and `finfilings` against F1 68.277%, CER
19.544%. If F1 falls by more than 0.2 on either, it comes back here before it
ships.

### 2026-09-23 — The two-component rule failed its gate on real filings; narrowed to "no cut between two lone glyphs"

**Supersedes** "A column fragment must hold two components …" (same date), whose
rule text is withdrawn from §6; that entry stands as the record. Readings,
shipped model, binary newer than `lines.rs`, `cargo test --workspace` green:

| corpus | F1 before → after | CER before → after |
|---|---|---|
| `bench/pages-cov` | 76.675% → 76.677% | 6.503% → 6.502% |
| `finfilings` (60 pages) | 68.277% → **66.807%** | 19.544% → **19.960%** |

The gate was 0.2 F1 on either corpus; `finfilings` lost 1.47. The cost the entry
named as possible — cuts next to a lone glyph that the old greedy cut made —
is real on financial filings. **Why is inferred, not yet shown:** a filing's
tables set a currency sign in its own cell (`$` | `204`), and the old rule's
singleton veto re-merges exactly those cells. Nothing in the synthetic corpus
has that shape, which is why it moved by 0.002.

**Decision, specified for `ocrcer-runtime`:** a candidate is dropped only when
the fragments on *both* sides of it are single components. That is the leader
case exactly: every gap in a run of dots is bounded by singletons on both sides,
and no gap next to a word is. Consequences, argued before measured:

- The four leader fixtures pass unchanged: every inter-dot candidate has a
  singleton either side.
- `a_single_wide_gap_still_separates_two_words` (`[0, 10, 40]`, 8×10 glyphs)
  now cuts, because a two-glyph fragment sits on one side. That fixture was
  authored when the cut was 2.5 heights and sits at 2.2. It tests the fallback
  space rule, not the cut. Its geometry is the thing that is wrong now, so it
  moves: a gap under 1.75 heights that the fallback still calls a space. Its
  expectations (fallback source, two words, a singleton second word) stay as
  authored. If no geometry under the cut keeps the fallback source, that comes
  back here rather than being re-blessed.
- `a_lone_glyph_between_wide_gaps_does_not_fracture_its_band` was written for
  the withdrawn rule and is replaced by two tests: a run of lone glyphs
  separated by wide gaps stays one band, and a lone glyph beside a
  multi-glyph fragment across a wide gap is cut off.

Gate unchanged: `cargo test --workspace` green; within 0.2 F1 of the controls
on both corpora (pages-cov 76.675%, finfilings 68.277%). `finfilings` includes
render-defective pages (overprinted text boxes, audit in progress), which
cost both arms alike and do not excuse a loss.

Evidence: `docs/measurements/2026-09-23_column_fragment_min.txt`.


### 2026-09-23 — Bold faces enter the bank; the real-filings error is the engine's, not the corpus's

A by-eye audit of all 60 `finfilings` pages (`docs/measurements/2026-09-23_finfilings_audit.md`)
found one render-defective page (`filing__r000011`, text overprinted at two wrap
widths) and 59 clean ones. So the 19.5% CER is mostly engine error. 58 of 60
pages carry bold text, and 24 set fi/fl ligatures in a serif that forms them.
Every face in the bank is Regular weight; bold has never been represented.
Headings, bullets and table captions in this corpus come out as garbage
(`FOURTH QUARTER` → `FOURTH OUARTER ANO rU11 wæ`).

**Decision: add the Bold weight of every bank family whose Bold file is present
and whose licence is already cleared** (the licence covers the family; each
file's own metadata is still checked and recorded in `model/fonts.tsv`). This is
the cheap fix §1 of the protocol asks for first: a font-coverage gap, closed by
a script rerun. No charset change, no feature change, no normalisation change,
no format bump — the bank is rebuilt whole, per the protocol's step 4 by habit,
though nothing here requires it. Italic is not added now: nothing in the audit
shows it is a cost, and it would be added on evidence, not by symmetry.

`filing__r000011` is reported separately from now on and excluded from
headline `finfilings` figures, with the exclusion stated wherever the figure is.

**Ligatures wait.** A fused fi/fl is one component the bank has no prototype
for; the classical fix (a prototype labelled with a class *sequence*, as in
Tesseract's multi-character unichars) changes the prototype label table and
therefore the format. It needs a measured share of the error first. The
line-matched CER and the bold rebuild come first, and a count of
ligature-attributable errors is taken on the rebuilt bank.

Gate for the bold bank: `cargo test --workspace` green; on the shipped
defaults, `bench/pages-cov` word F1 and CER not worse than the control by more
than 0.2 and 0.05 points (bold prototypes must not steal Regular matches);
`finfilings` (59 clean pages) reading reported beside the Regular-only
reading on the same 59 pages.

### 2026-09-23 — The narrowed lone-glyph rule also fails the real-filings gate; not shipped until the difference is seen on a page

Readings for the rule "no cut between two lone glyphs" (evidence
`docs/measurements/2026-09-23_column_fragment_min.txt`): `bench/pages-cov` F1
76.675%, CER 6.503%, identical to control; `finfilings` F1 67.865% (control
68.277%, −0.41), CER 19.610% (control 19.544%). The gate is 0.2. **Fails.**

Two rules have now been reasoned from the leader fixture and both have cost
real pages. Before a third one, I have to see which pages and lines differ between the
greedy cut and the guarded cut. The inferred causes (a `$` cell, a
single-digit column) have not been observed. Specified for `ocrcer-runtime`: a
`lines.column_lone_guard` toggle (1 = guard, 0 = greedy cut as shipped on
2026-09-22), provenance `guess`, exactly like `words.pitch_grid_check`, so
both behaviours run from one binary; then a per-page diff on `finfilings` of
the pages whose word F1 moves most, showing the lines that changed.

**Interim state:** the toggle ships at **0**, meaning the greedy cut with the
measured 1.75 value. That is the configuration both corpora measured best. Five
unit fixtures that encode "a leader line is not diced" run with the guard on,
because the behaviour they describe is still the intended one. What is not yet
known is whether dicing leaders costs anything measurable on real pages, and
this log says so rather than letting the fixtures stand in for a measurement.

### 2026-09-23 — Bold faces ship; they missed one of their own gates by 0.02 points of CER, and this entry says so

Readings (`docs/measurements/2026-09-23_bold_faces.txt`), bold candidate (32
faces, 23,740 prototypes) against Regular-only (19 faces, 17,610):

| corpus | word F1 | CER |
|---|---|---|
| `bench/pages-cov` (no bold in it) | 76.675% → 76.513% (−0.16) | 6.503% → 6.576% (+0.07) |
| `finfilings`, 60 pages | 68.277% → **70.996% (+2.72)** | 19.544% → **18.546% (−1.00)** |

The gate was: no worse than 0.2 F1 and 0.05 CER on `pages-cov`. F1 passes;
**CER fails by 0.023 points.** The 59-clean-page split the gate asked for was
not produced; the defective page is in both arms.

**Decision: ship.** The gate existed to catch bold prototypes stealing Regular
matches wholesale. What it measured is a 0.07-point CER cost on a corpus that
contains no bold at all, against a 2.7-point word-F1 gain on the in-domain real
corpus, where 58 of 60 pages carry bold. This is a judgement that overrides a
threshold I set, made in the open. A gate that fails is not re-worded after the
fact to read as passed. Follow-ups, both recorded on the tuning list:
`ocrcer-bench` names the confusions that grew on `pages-cov` (which Regular
classes bold prototypes now win), and whether bold prototypes need different treatment in matching
is decided on that evidence, not before.

`feature_norm` is recomputed from the larger bank, as it always is on a rebuild.
Norm and prototypes live in the same file and are rebuilt together, so this is
not the normalisation change the protocol guards. There is no format bump.

`model/fonts.tsv` keeps its 13 Bold rows; `model/out/ocrcer.ocrw` is rebuilt
from it and carries the `column_lone_guard` row. The font-row count test in
`ocrcer-build` is updated to the new inventory.

### 2026-09-23 — Correction: the bold readings compared a four-size bank against a five-size one; re-measured

**Corrects** "Bold faces ship; they missed one of their own gates …". The
bold candidate (23,740 prototypes) was built at 16/24/32/48; the Regular
control it was compared against is the five-size 16/20/24/32/48 bank
(17,610) that every figure since the ladder entry uses. So that entry's
deltas mix two changes, bold faces and the loss of the 20 px/em rung, and the
`pages-cov` CER miss cannot be attributed to bold. This is the same silent
ladder disagreement the ladder entry recorded once before. The cause this time
is that the ladder is an argument to `ocrcer-build write`. It is not a recorded
property of the file that `inspect` prints.

`model/out/ocrcer.ocrw` is rebuilt at 16/20/24/32/48 with the 32 faces:
29,675 prototypes, 3.15 MB, int8 top-1 agreement 99.486%. Both corpora are
re-measured against it; the ship decision stands or falls on those readings and is
recorded in the next entry.

**Specified for `ocrcer-exporter`**: `meta` records the build size ladder and
`inspect` prints it, so a bank's ladder is read from the file, not remembered.
This is additive to `meta`; the version is bumped only if the parser would
otherwise reject or misread the file.

### 2026-09-23 — Bold re-measured like for like: it passes every gate on both corpora

At the shipped five-size ladder, 32 faces, 29,675 prototypes (evidence
`docs/measurements/2026-09-23_bold_faces.txt`, correction section):
`bench/pages-cov` F1 76.675% → **77.392%**, CER 6.503% → **6.127%**;
`finfilings` F1 68.277% → **70.846%**, CER 19.544% → **18.546%**. The
earlier `pages-cov` CER miss belonged to the dropped 20 px/em rung, not to bold.
Bold ships, with no override needed; the "gate missed" wording in the entry
before last is superseded by this one and left in place as the record. The
follow-up that asked which Regular matches bold steals is withdrawn: at five
sizes nothing measurable is stolen.

**Next target, from the same run:** the top `finfilings` confusions are all
deletions (`e`→∅ 1,447, `i`→∅ 1,268, `t`→∅ 1,149, …), which means text is
missing, not misread. Where it goes (lines never found, lines dropped by a
filter, pages the truth covers but the image crops) is `ocrcer-bench`'s to
diagnose next, before any rule is proposed.

### 2026-09-23 — The real-filings deletions are merged lines; the mechanism is pinned before any rule is written

`ocrcer-bench`'s audit of the six worst clean `finfilings` pages
(`docs/measurements/2026-09-23_finfilings_deletions.md`) attributes 82% of
their deletions to one failure. Two tightly-leaded text lines are banded as
one line: full-width bands of 200+ components, and `measure` reports
x-height **1 px, `Observed`**. The second line's members sit below the elected
baseline and their tops clamp to zero. The line then decodes to a few garbage
tokens, and `\n`→∅ dominates on those pages. 9% is a dense-table page (not yet
traced), and 8% is an italic page on which segmentation was verified clean.
No line was never found or dropped by a filter; that outcome was also observed.

Two things are wrong here and they are separate:
1. **Banding admits a second line.** `best_band` compares a component against
   the band's running extent, and `hangs_below` accepts descender reach. Which
   of these admits the second line, and which components bridge (tall
   punctuation `$ ( ) |`, a descender row, an underline), is inferred from the
   code and not yet observed.
2. **`measure` reports an impossible x-height as `Observed`.** A 1 px x-height
   on a band whose cap height is 13 px is not a measurement. Whatever the
   banding rule, `measure` must never label a value `Observed` when it is below
   what that band's own components could carry.

**Specified for `ocrcer-runtime`, phase 1 only (diagnose, do not ship):**
instrument `best_band` on `filing__r000055` and `filing__r000088` to record,
for each merged band, the first component that joined from the second line,
which test admitted it, and the band's extent at that moment. Report the
counterfactual readings on those pages with the admitting test tightened, and
propose a rule. The rule is decided here; phase 2 implements it.

**Italic:** one page in six is lost to an unrepresented style. This is evidence,
not yet a share of the corpus. It is queued behind the merge fix, measured
the way bold was.

### 2026-09-23 — Merged lines: split on two baselines, and stop calling an implausible x-height observed

Phase 1 (`docs/measurements/2026-09-23_line_merge_mechanism.md`) settles
the mechanism. **Observed:** on both traced pages every join goes through
the overlap test (5,591 of 5,591), and `hangs_below` admits nothing. The
merges are ordinary prose at tight leading, and the ink of the two lines
interlocks with no blank row between them. The bad joins have overlap ratios
of 0.90–1.00, the same range as a legitimate ascender-over-x-height join.
The few-page readings agree: `overlap_fraction` 0.6/0.7 leaves the merges and
makes r000088 worse; 0.95 removes them and makes both pages much worse;
`descender_reach_fraction` has no effect. **The overlap ratio is the wrong
axis, and it is not tightened.**

Two rules are decided. They are independent, and each is gated on its own
readings:

**A. Plausibility floor on `Observed` (in `measure`).** A band is labelled
`Observed` only when `main ≥ lines.x_height_floor_per_cap × cap`. Otherwise
the branch falls through exactly as if `upper` were empty, and the line
reaches `inherit_x_heights` as it already should. The floor is half of the
smallest per-face x-height/cap-height ratio in the shipped bank, measured by
`ocrcer-build metrics` across all 32 faces. It is recorded as `measured` with
that derivation in its note. Phase 1 saw a ratio of about 0.08 on the defect,
against 0.7431 for a typical face. Tests: the traced shape (a tops pile-up at 0,
real tops at 13/17) is not `Observed`; ordinary mixed-case, all-caps and
condensed-face lines keep `Observed`.

**B. Two-baseline split (a post-pass after band growth, before `measure`),
behind the toggle `lines.baseline_split`, default 0 until measured.** The
population is the band's `body` members (the same ≥0.5×reference height filter
`measure` uses, so marks are excluded). Build their width-weighted `y1`
histogram. Let the primary peak p1 be its mode. The secondary peak p2 is the
mode among bins at distance ≥ `lines.baseline_split_sep` × (the band's median
body height) from p1. Split when:
- support(p2) ≥ `lines.baseline_split_support` × support(p1), and
- the weight strictly between the two peaks' ±1-bin neighbourhoods is ≤ the
  same fraction of support(p2).

The cut is the midpoint of the two peaks. A member goes above the cut when its
`y1` ≤ cut, and below it otherwise. Recurse on each half so that a
three-line heading splits into three. The upper band comes first in
reading order. Starting values: `sep` 0.6, `support` 0.25. Both are
**guesses**, labelled so in params.tsv and on the chunk-8 list. The
rationale for choosing baselines over tops: a real line has one baseline but
two top tiers (x-height and cap height), so bimodal tops are normal and
bimodal baselines are not. Named false-positive risks, each of which needs a
unit test that does *not* split: a mixed-case line with descenders; a line with
superscript/footnote markers; a line of digits with a tall `$ ( ) |`. The
tests that *must* split are: two tight lines, and three tight lines.

**Gates, per rule, both corpora, like for like against the shipped
readings (pages-cov F1 77.392 / CER 6.127; finfilings F1 70.846 / CER
18.546):** `cargo test --workspace` green; pages-cov not worse on F1 or CER
by more than 0.05; finfilings better on CER. A ships alone if it passes
alone. B ships (toggle to 1) only if A+B beats A. If B fails, it stays in
the code shipped off, the way `column_lone_guard` did, and the per-page
diff is required before any retune.

### 2026-09-23 — Merged lines ship: the floor and the split go together; the floor alone does not

Readings are in `docs/measurements/2026-09-23_line_merge_phase2.txt`, measured
like for like against the bold-ship controls:

- **Rule A alone fails its gate.** On finfilings, F1 is +1.97 but CER is
  +0.46, against a gate of "finfilings better on CER". Inferred cause: the
  merged lines now carry a plausible x-height, so more of their text is
  decoded, but the two lines are still interleaved in one band.
- **A+B passes every gate.** pages-cov is unchanged (F1 77.392, CER 6.127),
  since that corpus is synthetic with open leading. Finfilings CER goes
  18.546 → **17.064** and F1 70.846 → **73.615**.

The previous entry required A to ship alone only if it passed alone. It did not
pass, so it ships only as part of A+B. **`lines.baseline_split` is 1**, and its
provenance moves to `measured`. `baseline_split_sep` (0.6) and
`baseline_split_support` (0.25) remain **guesses** on the chunk-8 list; neither
was swept.

Still open from this work:
- the few-page x-height-1.00 band counts on r000055/r000088, not re-counted;
- the dense-table page r000022;
- italic.

### 2026-09-23 — Underlines are stripped from the pixels of over-wide components, not judged as whole components

The r000022 trace (`docs/measurements/2026-09-23_dense_table_r000022.md`)
disproves the table-rule hypothesis for that page's grid, which has **no
rule pixels** (Observed). It confirms the same mechanism in another place:
the page's five **underlined section headers** fuse the underline and the
letters into one component. Three of them exceed `furniture_fraction` and are
dropped; two survive as one blob that decodes to a few characters. Those
headers hold about 11% of the page's characters and an **estimated** ~38% of
its deletions. Retuning either whole-component gate fails:
`furniture_fraction` barely moves; `rule_aspect` improves end-to-end CER but
**worsens line-matched CER by 5–8 points** because it deletes real hyphens and
minus signs. End-to-end looked better only because of reading order. That
metric trap is recorded here so it is not re-learned.

**Rule (a new step inside component filtering, behind the toggle
`lines.underline_strip`, default 0 until measured):**
1. After labelling, take the page's median height of glyphish components,
   *h*, and the run floor *L* = `lines.rule_run_heights` × *h*.
2. Only a component whose **width ≥ L** is examined. Inside its bounding box,
   erase every horizontal run of ink of length ≥ L. Re-label the remaining
   pixels of that box into sub-components, which then pass through the normal
   filters.
   - A component narrower than L is never touched. This protects every hyphen,
     minus sign, em dash and ordinary word, and keeps the cost to the few
     over-wide components.
3. The erased pixels are discarded, since nothing downstream consumes
   "underlined". A descender crossing the underline loses the crossing
   pixels; accepted.
4. Horizontal only. There is no evidence yet of vertical rules touching text,
   so vertical stripping is not built.

`lines.rule_run_heights` is **derived, not guessed**. Extend
`ocrcer-build aspect` (or `metrics`) to report, per face and per class, the
longest horizontal ink run divided by that face's x-height; take the maximum
over all 187 classes × 32 faces. Using the x-height is conservative, because
*h* on a real page is at least the x-height. The value is 2 × that maximum.
The factor 2 is authored headroom, stated in the params note, and the
provenance is `measured` with the derivation.

**Gates:** tests covering an underlined word (splits into letters with the line
gone), a long em dash / a row of hyphens (untouched), and a table rule touching
digits (digits recovered). `cargo test --workspace` green. Readings like for
like against the line-merge ship (pages-cov F1 77.392 / CER 6.127; finfilings
F1 73.615 / CER 17.064). pages-cov may be no worse by more than 0.05 on either;
finfilings must be better on **both** end-to-end and line-matched CER (the
lesson above).

**Separately queued, not decided:** in r000022's numeric grid the loss is
touching glyphs at an x-height of about 10 px (`0$` fused and read as `®`).
Splitting touching glyphs is the lattice segmenter's job, so why it did not
split there is a separate trace.

### 2026-09-23 — Underline strip fails the real-filings gate; it stays in the code, shipped off

Readings (`docs/measurements/2026-09-23_underline_strip.txt`; `lines.rule_run_heights`
= 5.4209, measured as 2 × the em dash's 2.7105 x-heights on OCRcer Technical):
- r000022, its target page: CER 40.14 → 34.99, line-matched 37.15 → 33.44.
- pages-cov: bit-identical.
- **finfilings: F1 73.615 → 71.985, CER 17.064 → 19.253, line-matched
  16.942 → 19.414.** A clear failure, on both CERs.

The losses are flat deletions of common letters (e, i, t, r, o …). They are
worst on pages that were not near the top of the control's worst list (r000033
68%, r000044 65%, r000055 55%, r000583 54%). The inferred reading: the strip
is firing on over-wide components that are **not rules**, for example words
whose glyphs touch at scan resolution with continuous serifs or baseline ink,
and erasing their bottom strokes. That is inferred and not observed.

`lines.underline_strip` stays **0**. The code and its tests stay, as with
`column_lone_guard`. No retune (a longer floor, a thickness test, a
span-of-component test) until a per-page trace on r000033 names what the
strip erased there: the component, its size, the runs removed, and whether
it held text. The rule is re-decided here from that evidence.

### 2026-09-23 — Underline strip, second rule: straight bands only, keep crossing strokes, drop strip debris, and keep what was stripped

Trace: `docs/measurements/2026-09-23_underline_strip_damage.md` (r000033,
r000055; single-page readings: r000033 CER 26.8 → 68.0, r000055 32.5 → 55.2
with the first rule). **Observed:** every true rule and box edge is a band of
2–4 consecutive rows, one contiguous run per row, with x0/x1 equal to within
±1 px across the band. The one false hit was a single row of five disjoint
short runs, squeezed between the two lines of a double rule; erasing it cut
the bottoms off `)` in `$(83)`, `$(328)`. The larger damage is **strip
debris**: erasing a box's top edge leaves its vertical sides as new components
10–22× *h* tall (19 of them on r000055). They did not exist before the strip,
and they are the inferred driver of the spread-out deletions.

The rule replaces the previous entry's step 2. Toggle, floor and scope
(components of width ≥ L) are unchanged:
1. **Bands, not rows.** Within an examined component, erase only a band of
   ≥ 2 consecutive rows, each with exactly one run ≥ L, with x0/x1 agreeing
   within ±2 px down the band. A single row is never erased, and neither is a
   row holding more than one qualifying run.
   - Accepted cost: a 1-px rule on a low-resolution scan is not stripped.
     Every rule observed was 2–4 rows.
2. **Keep crossing strokes.** Within the band's x-range, erase a column's pixels
   only where that column's vertical ink run is no taller than the band's
   thickness plus 1. A stroke crossing the rule keeps its pixels.
3. **Drop strip debris.** A piece produced by re-labelling a stripped
   component, whose height exceeds `lines.debris_heights` × *h*, is discarded
   as a rule remnant. This applies only to strip-produced pieces, never to
   ordinary components. `lines.debris_heights` is **derived**: 2 × the
   tallest ink height of any class on any shipped face divided by that face's
   x-height, from the same `ocrcer-build aspect` pass. It is `measured` with
   the derivation in its note.
4. **Keep what was stripped.** Each erased band is recorded as a rule
   segment (x0, x1, y0, y1) on the page layout output, instead of being
   thrown away. Nothing consumes it yet. It exists for the formatting
   direction below, and it costs one small vector per page.

Gates as in the first rule. finfilings must beat F1 73.615 / CER 17.064 /
line-matched 16.942 on both CERs; pages-cov no worse by 0.05. If this fails,
the toggle stays 0 and the underline work stops until the per-page diff
names a new mechanism.

**Direction recorded, not scheduled (Ken, 2026-09-23): preserving formatting.**
Ken would like OCRcer eventually to detect and preserve formatting such as
underlines, and to emit open document formats that can then be converted
into MS Office formats. Architect's position:
- **Output format:** do not invent one. ALTO XML already carries per-word
  style (`TextStyle` with underline, bold and italic), with geometry at the
  line, word and glyph level. hOCR (HTML) carries the same for HTML-based
  tooling. ODT/DOCX conversion from those is existing downstream tooling,
  not this engine's job.
- **Signals:** underlines come from item 4 above (a rule segment under a
  word's baseline span). Bold and italic *may* come from which face the
  winning prototypes were rendered from; whether the bank keeps a per-prototype
  face index is **not checked**. Font size comes from the x-height `measure`
  already reports.
- **Scope:** this is an output stage over data the pipeline already has,
  not a charset, feature or format change. It sits after chunk 8's accuracy
  work, and it is kept in scope because printed documents are the domain.
  It enters `PLAN.md` as a staged chunk only when Ken asks for it to be
  scheduled.

### 2026-09-23 — Checked: the bank does not record which face each prototype came from

This follows up the previous entry's "not checked". `.ocrw` stores
`prototype_class` per prototype and a `meta.faces` list, but **no
per-prototype face index**. That was checked in `ocrcer-core/src/ocrw.rs`'s
table names. Bold/italic signals for the formatting direction would therefore
need an optional, additive `prototype_face` table (u8 per prototype, about
30 KB at 29,675 prototypes). It would be read the way `feature_weights` is
read: absent means unknown. It changes neither the features nor the classes,
so it needs no version bump. It is not scheduled, and it belongs with the
formatting chunk when Ken asks for it.

### 2026-09-23 — Underline strip, second rule: fails the real-filings gate; it stays off

These are readings, from `docs/measurements/2026-09-23_underline_strip.txt`.
Each pair is control → `lines.underline_strip=1` under rule 2.

- finfilings (60 pages): CER 17.064 → **18.555**, F1 73.615 → **72.469**.
  That fails the gate, which required beating control. The loss is about
  70% of rule 1's (F1 −1.146 against −1.630).
- pages-cov (625 pages): bit-identical to control, at F1 77.392 and
  CER 6.127.
- Target pages: r000022 went from 40.144 to 37.320 CER, and r000033 from
  26.849 to 22.657, so both improved. r000055 went from 32.488 to
  **57.005**.
- The loss comes from a few pages: r000044 (64.61 CER), r000055 and
  r000583 (53.76). Their confusions are **broad deletions of ordinary prose
  letters** (`e`, `n`, `a` and spaces lost), not the rule-shaped
  confusions r000022 shows. Real body text is being erased, and none of
  rule 2's guards catch it.

**Decision.** `lines.underline_strip` stays 0, and the code stays in place.
As the previous entry required, the underline work stops until a per-page
diff names the mechanism. One diagnostic is allowed, and it changes no code
or params: on r000055, dump which bands the strip accepted and what they
overlapped, to name why prose rows qualified as a "band". One hypothesis,
**not checked**: a merged or tightly packed text block whose row runs
pass `rule_run_heights` × h. If a band is measured against a wrongly
small x-height, the run-length threshold L drops low enough that ordinary
ink runs qualify.

### 2026-09-23 — Two mechanisms named: the strip leaves a box-side sliver that merges lines; `0$` misses the split gate by half a pixel

These are readings, from
`docs/measurements/2026-09-23_underline_r000055_and_touching_r000022.md`.

**Underline strip (rule 2).** The previous entry's hypothesis, a wrong
`h`, is **false**. The strip correctly erases the top borders of bordered
number boxes: 18 per page on r000055, `w=426`, 423 of 426 columns erased,
no glyph ink touched (checked with a mask dump). But each erase leaves the
box's left side behind as a 3×51 px sliver.

- The sliver is too narrow for `furniture_fraction`, and too short for
  `debris_heights` (51 < 65.3).
- Before stripping, the whole box was furniture, 426 px against a 330.6 px
  gate. The strip turns furniture into a tall "glyph".
- Tallest-first grouping then seeds a band on the sliver, and two real
  prose lines fuse into one (33 + 29 → 62 members).
- The result is 89 → 69 lines on r000055 and 91 → 72 on r000044, which is
  the broad letter deletion the gate reading saw.

**Rule 2, part 3b (decided).** A strip-produced piece whose width is
≤ 0.5·h **and** whose height is > `lines.thin_debris_heights`·h is
debris and is dropped.

- `thin_debris_heights` = 1.5 × the tallest ink height (in x-heights)
  among classes whose ink width is ≤ 0.5 x-height, over all 32 faces,
  taken from the `aspect` tool.
- The ink height is measured. The ×1.5 headroom is a **guess**, labelled
  as one, and it goes on chunk 8's tuning list.
- The earlier ×2 convention is not reused, because it would sit too close
  to the observed sliver (about 3.6–3.9·h).
- Gates are unchanged: finfilings must beat CER 17.064 and line-matched
  16.942; pages-cov must be no worse by 0.05.

**Touching glyphs.** The `0$` atom on r000022 is 11 px wide. The search
gate `split_min_x_heights` (1.15) × x-height (10) is 11.5 px, so no profile
was ever computed. Computed for diagnosis, the profile has an unambiguous
valley at x=314 (value 1, ceiling 2.64), and the existing valley rule would
have cut it correctly. So this is a threshold problem, not a
cutting-technique problem, and drop-fall is not needed here.

`segment.split_min_x_heights` is a guess. Sweep it at {1.15 (control),
1.0, 0.85}. The gates are the same pair as above; finfilings runs first
because the case comes from there. The narrowest value that passes both
gates ships, with provenance "measured".

### 2026-09-23 — Split gate: `segment.split_min_x_heights` 1.15 → 1.09, measured

These are readings, from `docs/measurements/2026-09-23_split_gate_and_strip_3b.txt`.

| value | finfilings CER / line-matched / F1 | pages-cov CER / F1 |
|---|---|---|
| 1.15 (control) | 17.064 / 16.942 / 73.615 | 6.127 / 77.392 |
| 1.0 | 16.885 / 16.799 / 74.150 | **6.330** / 77.091 (fails) |
| 0.85 | 16.908 / 16.822 / 74.090 | not run |
| **1.09** | **16.932 / 16.843 / 74.047** | **6.089 / 77.429** |

1.09 passes both gates and ships. 1.0 gains more on finfilings but costs
pages-cov +0.203, which is over-segmentation on clean pages.

1.09 rather than 1.10 because of the float path: f32 1.10 becomes
1.1000000238 in f64, so a 10 px × 1.10 gate would still reject the 11 px
`0$` atom under the strict `<`.

Recognition-gated chopping is the recorded next step if more gain is
wanted: offer cuts below the gate only when the whole atom matches poorly
(addendum in `2026-09-22_research_classical_techniques.md`). It is **not
measured**.

The new controls for later gates are finfilings F1 74.047, CER 16.932,
line-matched 16.843, and pages-cov F1 77.429, CER 6.089.

### 2026-09-23 — Underline strip ships (rule 2 with part 3b)

These are readings, from `docs/measurements/2026-09-23_split_gate_and_strip_3b.txt`,
all measured with the split gate at 1.09.

- finfilings: CER 16.932 → **16.756**, line-matched 16.843 → **16.634**,
  F1 74.047 → **74.405**.
- pages-cov: 6.089 → 6.089, identical.
- Pages the earlier strips had broken are back: r000044 went from 64.61
  (rule 2 without 3b) to 29.22, and r000055 from 57.005 to 32.49.
- `lines.thin_debris_heights` = 3.4288. The ink height is measured; the
  ×1.5 headroom is a guess on chunk 8's tuning list.

`lines.underline_strip` = 1, with provenance "measured". `strip_underlines`
also returns the erased bands as `RuleSegment`s. That output is what the
formatting direction (underlines in ALTO/hOCR) would consume. It is not
yet surfaced in any output format.

r000583 is now the worst page at 54.66. It was not examined in this
round.

### 2026-09-23 — Worst page named: bounding-box chaining glues pixel-disjoint serif letters into one atom

Measured in `docs/measurements/2026-09-23_worst_page_r000583.md`. On
r000583 (54.66% CER), `atoms()` merges consecutive components whenever their
x-ranges overlap. That rule exists for an `i` and its dot. In this serif face,
a `t` crossbar and an `h` base serif overlap by 1–6 columns at different
heights, even though the ink never touches. The page has 73 atoms chaining 3
or more letters, up to 7. An ordinary page (r000572, 4.91%) has 3, none larger
than 3. Once letters are chained, no vertical cut separates them cleanly, and
`max_splits = 3` cannot carve a 7-letter atom into enough pieces. The
underline strip changes nothing on this page. The split gate costs 0.9 points.

Decision: the cheapest fix comes before any non-vertical cut. The letters are
already separate components, so the segmenter should not glue them together
in the first place. Next experiment: merge overlapping components only when
the overlap is a real fraction of the narrower one's width (an i-dot sits
entirely inside its stem's columns; a kerned serif overlaps by a few columns),
as a new guess-labelled parameter. Each piece is matched on its own members'
pixels, not on a column crop that picks up the neighbour's serif. Gates are
unchanged: beat finfilings on both CERs (16.756 / 16.634), and keep pages-cov
within 0.05 of 6.089. Drop-fall or contour cuts and a width-scaled
`max_splits` stay queued behind this. They address touching ink, which this
page does not have.

### 2026-09-23 — Atom merge by overlap fraction: `segment.merge_overlap_frac` 0.3, measured

This implements the preceding entry. `atoms()` now merges two components
only if one lies entirely inside the other's column range (dots, `:`, `;`,
`=`), or if their overlap is at least `merge_overlap_frac` × the narrower
one's width. A value of 0 reproduces the old any-overlap rule exactly. With
merging this strict, neighbouring atoms can now share columns. So a piece's
pixels come from its own member components (`edge_labels()`), not a column
crop, and a separated `t` no longer picks up the `h` serif. The feature
extractor is unchanged.

Readings are in `docs/measurements/2026-09-23_atom_merge_overlap.txt`:

| corpus | control | frac 0.3 | gate |
|---|---|---|---|
| r000583 alone, CER | 54.661 | 40.000 | (screen; 0.3 best of 0.3/0.5/0.7) |
| finfilings end-to-end CER | 16.756 | 16.113 | < 16.756, pass |
| finfilings line-matched CER | 16.634 | 16.068 | < 16.634, pass |
| pages-cov CER | 6.089 | 6.057 | ≤ 6.139, pass |

This ships as the default, labelled measured. It improved the synthetic
pages too, so the chaining was not confined to scanned serif filings. The
new controls are finfilings 16.113 / 16.068 and pages-cov 6.057. Only 0.3,
0.5 and 0.7 were screened, and only on one page. Values below 0.3 are
unmeasured.

### 2026-09-23 — `segment.merge_overlap_frac` 0.3 → 0.4, measured

The previous entry screened only 0.3, 0.5 and 0.7, and only on one page. This
round ran the full finfilings corpus at other values:

| frac | finfilings end-to-end CER | line-matched CER | pages-cov CER |
|---|---|---|---|
| 0.15 | 16.232 | 16.259 | not run |
| 0.2 | 16.167 | 16.210 | not run |
| 0.3 (control) | 16.113 | 16.068 | 6.057 |
| 0.4 | **16.089** | **15.910** | 6.057 |

The architect re-ran 0.4 on finfilings independently and got the same result
to the digit. The 0.2 row's word-level figures in the measurement file are
identical to 0.4's. That looks like a transcription slip on the 0.2 line; it
affects only a rejected value.

0.4 beats 0.3 on both finfilings CERs, and pages-cov does not move. It ships.
New controls: finfilings 16.089 / 15.910, pages-cov 6.057. Not measured on
the full corpus: 0.35, 0.45 and 0.5. The one-page screen showed 0.5 slightly
worse than 0.3 there.

### 2026-09-23 — Worst pages, round 2: whole-line fusion on tight leading is the next target

Measured in `docs/measurements/2026-09-23_worst_pages_round2.md`. r000583
(40.63%) is the same serif-chaining mechanism as before, just smaller now.
r000308 (34.45%) and r000363 (32.27%) are a different one. Line grouping
fuses two ordinary adjacent text lines into one band. On r000308, 4 fused
bands hold 26% of the page's components. On r000363, the fused bands'
x-height reads about 26px against a true 13px. Word splitting then sees two
interleaved rows and finds no gaps. Reading order is ruled out on all three
pages (line-matched equals end-to-end).

This mechanism survives the two-baseline split post-pass
(`lines.baseline_split = 1`). Next experiment: find out why that split does
not fire on these bands. The candidates are the separation or support
thresholds (both guesses), or the band's shape. Fix whichever it is,
preferring the split pass over a new rule. Gates are unchanged against the
current controls: beat finfilings 16.089 / 15.910, and keep pages-cov
within 0.05 of 6.057.

### 2026-09-23 — Line fusion fix: `lines.baseline_split_valley_margin` 0.3, measured

Cause: the two-baseline split measures the empty valley between two
candidate baseline peaks, excluding a fixed 2-pixel-row margin around each
peak. That margin was a raw pixel count, not scaled to type size. At body
sizes, a line's own descenders reach well past 2 rows below its baseline, so
they were counted as ink in the valley. The split test then rejected genuine
two-line fusions. Fix: the margin is now `baseline_split_valley_margin` ×
the line's measured x-height. 0 keeps the legacy fixed margin, so the old
behaviour is a switch position.

Readings are in `docs/measurements/2026-09-23_line_fusion_fix.txt`:

| measure | control | margin 0.3 | gate |
|---|---|---|---|
| r000308 CER (screen) | 34.45 | 14.50 | 0.3 best of 0.3/0.4/0.6/0.7 |
| r000363 CER (screen) | 32.27 | 18.90 | |
| finfilings end-to-end CER | 16.089 | **13.161** | < 16.089, pass |
| finfilings line-matched CER | 15.910 | **12.290** | < 15.910, pass |
| pages-cov CER | 6.057 | 6.064 | ≤ 6.107, pass |

This is the largest single gain on the real-filings corpus to date. It ships.
New controls: finfilings 13.161 / 12.290, pages-cov 6.064. Values below 0.3
were not screened. The pages-cov movement (+0.007) is within tolerance but is
a small loss, recorded as one.

### 2026-09-23 — Worst pages, round 3: form cells spliced into the wrong line of a wrapped label; the next target

Measured in `docs/measurements/2026-09-23_worst_pages_round3.md`.
- r000044 (30.03%), r000396 (29.03%) and r000407 (24.46%) share one
  mechanism. When a band is split at a column cut, each narrow right-column
  fragment (a value box, a checkbox, a list marker) is joined to whichever
  left-column line shares its baseline. If the left label wraps to two or more
  lines, the value lands in the middle of the label: after line 1, before
  line 2.
- The evidence: r000044 has the same symmetric `""↔"0"` pair 108 times, and
  r000396 has a 9.5-point gap between end-to-end and line-matched CER, the
  largest measured.
- r000011 is a ground-truth defect already on record: two overprinted
  renderings of one paragraph. No engine fix applies.
- r000022 is unchanged: touching digits and header fusion.
- A smaller mechanism recurs on two pages: checkbox glyphs have no class and
  match to `®`/`~`/`B`.

Decision: pair by cell, not by line. A narrow fragment is emitted after the
full wrapped text of the left-column cell it sits beside. The cell runs from
that fragment's row down to the line before the next right-column fragment
starts (or until the left column's line spacing breaks). A genuine
single-line label/value row keeps today's output, so this cannot break the
ordinary two-column case. Before coding, the implementer must confirm from
the ground truth how these pages order and break a wrapped label and its
value. The fix follows the truth's convention, not a guess about it.

The fix sits behind a switch that defaults to today's behaviour until
measured. Gates are unchanged against the current controls: finfilings
13.161 / 12.290 (beat both), pages-cov ≤ 6.114. Checkbox glyphs are a charset
question for later (a class for ☐/☑, or a declared "not text" drop). They are
not bundled into this change.

### 2026-09-23 — Cell pairing, first rule: large finfilings win, fails pages-cov; stays off

Readings are in `docs/measurements/2026-09-23_cell_pairing.txt`, commit
`fce36d1`. The truth convention is confirmed on all three pages: a wrapped
label's lines come first, then the value as its own line.

| measure | control | `cell_pairing=1` | gate |
|---|---|---|---|
| finfilings end-to-end | 13.161 | 12.968 | pass |
| finfilings line-matched | 12.290 | **10.744** | pass |
| pages-cov | 6.064 | 6.987 | ≤ 6.114, **fail** |

Named failure: "wrap continuation" was decided from column overlap and
steady line pitch alone. That cannot tell a wrapped label from the next,
unrelated one-line label at the same margin and pitch. r000407 shows it
(`ii. LEI, if any` got its value deferred past `iii. State…`), and so do
pages-cov's two-column "twins" layouts.

Decision: a continuation must pass two more tests, both standard line-wrap
cues:
- **The line before it must be full.** Its right edge must reach within
  `cell_wrap_slack` × x-height of the left column's right extent in that
  block. A line wraps because it ran out of room, so a short label like
  `ii. LEI, if any` cannot be wrapped.
- **The continuation must have no right-column fragment of its own row.** In
  two-column prose every row has one, so nothing defers.

`cell_wrap_slack` is a new guess, swept. The switch stays default 0 until
both gates pass.

### 2026-09-23 — Cell pairing, rule 2: fixes r000407, still fails pages-cov; stays off

Readings are in `docs/measurements/2026-09-23_cell_pairing.txt`, commit
`663f730`. The full-line and own-row tests make r000407 bit-identical to the
control at every slack. r000044 keeps rule 1's win at slack ≥ 2.0. But
pages-cov is 6.639 / 6.730 / 6.848 at slack 1 / 2 / 3, against a gate of
≤ 6.114. Rule 1 was 6.987. finfilings was not run, because the failing gate
was checked first.

The twins pages named as examples in rule 1's round turned out not to move.
The pages that actually carry the pages-cov regression have not been
identified. Line-matched CER regresses too (6.609), so the damage is lines
being joined or split differently, not just reordered.

Next: list pages-cov per-page CER at control vs rule 2 (slack 2.0). Name the
top regressors and the layout shape they share before any rule 3. The switch
stays default 0.

### 2026-09-23 — Cell pairing, rule 3 decided: fail-closed fullness, and an unsplit row cannot vouch for a column

Measured in `docs/measurements/2026-09-23_cell_pairing_pagescov_regressors.md`
(commit `782395c`; per-page `--csv` added to the bench in `5983367`). Only 37
of 625 pages-cov pages move under rule 2, and every one gets worse: 35
`drawing` pages (CAD title blocks, 7 mono fonts) and 2 invoices. That is the
core domain (`FEASIBILITY.md` §6), so this regression matters more than the
aggregate suggests. There are two mechanisms, both traced
fragment-by-fragment:
- An unsplit wide row below a correctly split row overlaps every column
  above it. The upper row's right cell (`SHEET 1 OF 3`, an `Amount` value)
  then reads as wrapping into it.
- The fullness test passes vacuously when the column-extent scan finds
  nothing wider than the candidate, because the extent equals its own edge.

Decision: rule 3 = rule 2 plus both proposed conditions:
- **(i)** In the column-match and extent scans, a single-fragment row counts
  only if the anchor lies inside a cell-width slice of it, not merely 40%
  overlapped.
- **(ii)** An extent that equals the candidate's own right edge fails closed.

Gates are as before, plus one addition: `drawing`-category pages must not
regress at all in aggregate (per-page CSV, control vs rule 3), because CAD
text is what the engine is for. r000044's win must be re-checked.

### 2026-09-23 — Checkboxes are furniture, not text: drop them, add no charset entry

Measured in `docs/measurements/2026-09-23_checkbox_truth_survey.md` (counted by
grep over all 685 truth files, 60 finfilings + 625 pages-cov, and by viewing
four checkbox pages directly). No truth file transcribes a checkbox with any
character or placeholder. At each box position the truth moves straight from
the question to `Yes`/`No`/`N/A`. The box glyph in finfilings, a `?` in a
square, is the source PDF's real design for an empty box (confirmed by the
earlier finfilings audit). Roughly 30–35 of the 60 finfilings pages carry
this layout. The `®`/`~`/`B` confusions on r000396/r000407 are therefore pure
insertions.

Decision:
- A charset entry for checkboxes is **declined**. It could never score,
  because any output at a box position is an insertion.
- Checkbox shapes are declared **non-text furniture** and dropped before
  recognition, just as rules are dropped by `lines.furniture_fraction` and
  `rule_aspect`.
- The detector is classical: a small, near-square, closed rectangular outline
  whose interior is empty or holds a single small mark, near x-height to cap
  height. Sizes are authored, as a multiple of x-height, and are guesses until
  swept.
- Gates are the usual ones: beat both finfilings CERs, and keep pages-cov
  within control + 0.05. In addition, `drawing` pages must not regress in
  aggregate. A CAD title-block cell or a boxed `0`/`O`/`D` is the obvious
  false positive, so the detector must refuse a box whose interior mark is
  glyph-sized.

Out of scope: detecting checked state (ticked vs empty) and emitting it as
text. Truth does not record it, and v1 does not need it.
