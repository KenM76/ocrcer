# Research note: classical (non-neural) OCR techniques survey

Not a measurement — a literature/source survey done for OCRcer's segmentation,
layout, binarization, and classification stages. All parameters described here
are authored/deterministic in their source systems, consistent with rule 1.
Everything below is what was actually found at the cited URL; anything drawn
from memory rather than a fetched source is marked "(from memory, unverified)."

---

## 1. Word-space / inter-word gap estimation and fixed-pitch detection

**Tesseract `textord` gap statistics.** Tesseract measures the horizontal gaps
between blobs in a "limited vertical range between baseline and mean line" (i.e.
excluding ascenders/descenders, which would pollute the gap measurement) and
builds a per-row gap-width histogram. Gaps near the row's estimated inter-word
threshold are marked "fuzzy" and the small/large decision is deferred until
after word recognition, rather than committed at layout time. Relevant tunables
seen in source: `textord_wordstats_smooth_factor` (gap-stat smoothing),
`textord_words_width_ile` (percentile of blob widths used for space
estimation), `textord_dotmatrix_gap` (max pixel gap tolerated inside a broken
fixed-pitch glyph). Source:
[tesseract::Textord Class Reference](https://tesseract-ocr.github.io/tessapi/3.x/a00642.html),
[textord/tovars.cpp](https://github.com/ttacon/tesseract/blob/master/textord/tovars.cpp),
[Breaking down Tesseract OCR — notes.pairml.com](https://notes.pairml.com/2019/01/15/breaking-down-tesseract-ocr/).

**Fixed-pitch (monospace) detection and pitch-based chopping.** Tesseract
tests each text line for fixed pitch by checking whether blob-start positions
fall on a regular pitch. When a line is judged fixed-pitch, Tesseract chops
words into characters strictly by pitch position and *disables* the general
chopper/associator (the touching-character splitter and merge logic used for
proportional text) for that line. Source: same `Textord`/`notes.pairml.com`
references above.

**Why it matters here.** CAD title blocks and BOM tables are frequently set
in a monospace or near-monospace font specifically so numeric columns align.
The Tesseract strategy — detect fixed pitch first, then chop by pitch instead
of by gap threshold — is the direct answer to "how do I avoid splitting
`112.50` into `112` and `50` at the decimal point": in fixed-pitch text the
decimal point sits at a known pitch-multiple offset with a *sub-inter-word*
gap on both sides, so a naive gap-threshold splitter over-segments it, while
pitch-based chopping treats the whole run as one word and lets word-level
lexicon/number-pattern context (rule 6 — lexicon is bonus-only, never
constraining) decide the character boundaries after fixed-width chopping, not
before. Implementation cost: medium — needs (a) fixed-pitch line test (blob
left-edge regularity, cheap to compute from existing connected-component
data), (b) a pitch estimator (mode or robust mean of blob start-to-start
distances), (c) a chop-by-pitch path that bypasses the general gap-threshold
word splitter. This is squarely inside the existing lattice/segmentation
architecture — it is a second segmentation policy selected per line, not a
new subsystem.

**Breuel/OCRopus gap statistics.** Search results confirm OCRopus (Breuel)
used "statistical and trainable layout analysis" with adaptive space
estimation, but no fetchable source gave the specific gap-clustering formula;
the closest primary material found was Breuel's own geometric-layout papers
(§2 below), which address column/line geometry rather than intra-line word
spacing directly. Treat the specific OCRopus space-estimation formula as
**not verified** from a source in this pass — do not cite a formula for it.
Source (existence only, not method detail):
[OCRopus — Wikipedia](https://en.wikipedia.org/wiki/OCRopus),
[Two Geometric Algorithms for Layout Analysis — Breuel](https://link.springer.com/content/pdf/10.1007/3-540-45869-7_23.pdf).

---

## 2. Reading order / column detection

**Breuel's maximal whitespace rectangles.** Finds a cover of a page's
background in terms of *maximal empty rectangles* (candidates unconstrained by
axis alignment in the 2003 ICDAR extension) via branch-and-bound: candidate
rectangles and their blocking obstacles are pushed onto a priority queue
ordered by an upper-bound quality function, split against their nearest
obstacle, and re-inserted; the first *obstacle-free* rectangle to reach the
top of the queue is the global optimum. Found rectangles are then filtered by
aspect ratio, width, and proximity to text-sized connected components to
select column-gutter candidates, which become obstacles for a separate
least-squares text-line fit. Reported cost: up to ~11,000 obstacles in under
8 seconds; a complete Java implementation is cited at roughly 200 lines.
Source: [An Algorithm for Finding Maximal Whitespace Rectangles at Arbitrary
Orientations for Document Layout Analysis — Breuel, ICDAR 2003 (DFKI PDF)](https://www.dfki.de/fileadmin/user_upload/import/1998_2003-breuel-icdar.pdf),
corroborating summary: [ResearchGate abstract](https://www.researchgate.net/publication/4033424_An_algorithm_for_finding_maximal_whitespace_rectangles_at_arbitrary_orientations_for_document_layout_analysis).

**Why it matters here.** This is close to a direct fit for invoices/statements
with multi-column tables and for drawing title blocks with dense whitespace
gutters between fields: the "find the biggest empty rectangles, use the
tall/narrow ones as column separators" approach requires no training and is
fully deterministic given a connected-component obstacle list — it is a
geometric optimization, not a learned model, so it fits rule 1 cleanly.
Implementation cost: medium-high for the general branch-and-bound
(arbitrary-orientation version); low-medium for an **axis-aligned-only**
restriction, which is sufficient after the deskew stage already normalizes
page orientation — axis-aligned maximal empty rectangle search is a simpler,
well-known sweep-line problem and avoids reimplementing Breuel's full
interval-arithmetic machinery.

**Recursive XY-cut.** A top-down algorithm: compute the horizontal and
vertical projection profiles of the current region, find the widest valley
(empty band) crossing the whole region in either direction, split there, and
recurse on each half until no valley exceeds a threshold. Produces a tree
whose leaves are the final blocks. Known weakness: sensitive to local skew,
irregular layouts, and uneven spacing, and it can fail outright on layouts
where no clean full-width/full-height valley exists (e.g. an L-shaped table
next to a paragraph). Source: [Segmentation of layout-based documents — ad-blog.cs.uni-freiburg.de](https://ad-blog.cs.uni-freiburg.de/post/segmentation-of-layout-based-documents/),
[XY-Cut++ — arXiv 2504.10258](https://arxiv.org/html/2504.10258v2).

**Why it matters here.** Much cheaper than whitespace-rectangle search
(projection profiles are O(pixels) or O(components), no priority queue) and a
reasonable first cut for simple single/two-column invoices. Its known failure
mode — no valley crossing the whole region — is exactly the CAD title-block
case (dense grid of small boxed fields), so XY-cut alone is not sufficient
for the CAD-drawing document class; it is a good cheap default with
whitespace-rectangles or Docstrum as the fallback for layouts XY-cut can't
cut cleanly. Implementation cost: low.

**Docstrum (O'Gorman).** Bottom-up: compute k-nearest-neighbours (paper
recommends k=5) for every connected component, use the angle/distance
distribution of within-line neighbour pairs to estimate skew and
character/line spacing simultaneously (this is the "document spectrum"), take
the transitive closure of within-line neighbour pairs (thresholded by
distance) to form text lines, then merge lines into blocks using parallel and
perpendicular distance thresholds. Advantages cited: skew-independent
(doesn't need deskew as a precondition), spacing-independent, and can handle
locally-varying text orientation within one image. Source:
[Document layout analysis — Wikipedia](https://en.wikipedia.org/wiki/Document_layout_analysis),
[The Document Spectrum for Page Layout Analysis — O'Gorman, Semantic Scholar](https://www.semanticscholar.org/paper/The-Document-Spectrum-for-Page-Layout-Analysis-O'Gorman/d85097da36118fbccfeb7802abf89bf4b4c63a3e).

**Why it matters here.** Because it derives skew and both spacing scales
(within-line, between-line) from the same k-NN computation, it is attractive
for CAD drawings where deskew can be locally imperfect (large sheets, mixed
rotated views/dimension text) — Docstrum's line-grouping doesn't presuppose a
single global skew angle the way a projection-profile method does.
Implementation cost: medium — needs a k-NN structure over connected
components (a k-d tree, per the Dalitz source below) plus the
angle-histogram skew estimate and the two-threshold (parallel/perpendicular)
merge step. Related: [Kd-Trees for Document Layout Analysis — Dalitz](https://lionel.kr.hs-niederrhein.de/~dalitz/data/publications/sr09-kdtree-layout.pdf)
describes an efficient k-d tree implementation of the neighbour search.

---

## 3. Adaptive binarization for scans

All four below are local/adaptive thresholding methods — the threshold is
computed per-pixel or per-window from local statistics, not one global value —
which is the standard cheap win for scans with uneven illumination, shadow
gradients, or staining. All are closed-form/deterministic (no training).

**Sauvola.** `T(x,y) = m(x,y) * [1 + k * (s(x,y)/R - 1)]`, where `m` and `s`
are the local mean and standard deviation in a `w×w` window, `R` is the
dynamic range of the standard deviation (commonly `R = 128` for 8-bit gray),
and `k` is a bias typically in `[0.2, 0.5]` (0.5 is a common default). An
improvement over Niblack's simpler `T = m + k*s` specifically for stained /
badly illuminated documents — the `s/R` term suppresses the threshold in
low-contrast (background) regions instead of amplifying noise there. Source:
[SauvolaNet — arXiv 2105.05521](https://arxiv.org/pdf/2105.05521),
[Modified Sauvola binarization for degraded document images — ScienceDirect](https://www.sciencedirect.com/science/article/abs/pii/S0952197620301159).

**Wolf-Jolion (2002/2003).** Refines Niblack/Sauvola with a contrast-
normalization term computed from the local min/max image contrast (an
"improved contrast maximization version of Niblack/Sauvola"), aimed at text
localization/binarization in mixed multimedia documents. A practical
downside noted in sources: because it needs a local-neighborhood feature per
pixel, naive implementation is slow relative to Sauvola unless done with
integral images. Source: [Text Localization, Enhancement and Binarization in
Multimedia Documents — Wolf, Jolion, Chassaing, ICPR 2002 (Semantic Scholar)](https://www.semanticscholar.org/paper/Text-localization,-enhancement-and-binarization-in-Wolf-Jolion/6b4f762d9a5acd964411d8c737073c24ce16a3c8),
reference implementation: [chriswolfvision/local_adaptive_binarization](https://github.com/chriswolfvision/local_adaptive_binarization).

**Su-Lu-Tan (2010).** Builds a contrast image from local max/min intensity
(instead of a gradient), locates high-contrast pixels (which lie near stroke
boundaries), then estimates the local threshold from only those high-contrast
pixels within a neighborhood window. Cited advantage over gradient-based
contrast: more tolerant of uneven illumination and smear-type degradation
because local max/min contrast doesn't blow up the same way a raw gradient
does under a smooth illumination ramp. Source:
[Binarization of historical document images using the local maximum and
minimum — Su, Lu, Tan, DAS 2010 (ACM)](https://dl.acm.org/doi/10.1145/1815330.1815351).

**Howe (2011/2012).** Frames binarization as energy minimization on a Markov
Random Field: unary (per-pixel foreground/background) terms come from the
image Laplacian, pairwise (smoothness) terms are gated by Canny edges, and
the whole objective is solved exactly via graph-cut / max-flow. Reported as
placing near the top of the DIBCO-09/H-DIBCO document-binarization
competitions, and Howe-derived methods won the 2014 and 2016 competitions.
This is the most accurate of the four but also the most expensive (graph-cut
over the whole image, plus — in the "with automatic parameter tuning"
follow-up — an internal parameter search). Source:
[A Laplacian Energy for Document Binarization — Howe (Semantic Scholar)](https://www.semanticscholar.org/paper/A-Laplacian-Energy-for-Document-Binarization-Howe/6e5022b90d4b7d2604e7be93ed2789cd16fa3bea),
[Document binarization with automatic parameter tuning — Howe (ResearchGate)](https://www.researchgate.net/publication/280764878_Document_binarization_with_automatic_parameter_tuning).

**Why it matters / ranking by cost.** Sauvola is the standard cheap win —
closed-form, one pass with an integral-image implementation for O(1) local
mean/variance per pixel, both `k` and `R` are authored constants (rule 1
compliant with zero ambiguity about provenance: they're stated in the
1999/2000 Sauvola paper, not tuned). Wolf-Jolion and Su-Lu-Tan are a step up
in robustness to illumination gradients (common on phone-photographed
drawings) at roughly the same order of implementation cost as Sauvola once
integral images are used. Howe is the most accurate but pulls in graph-cut,
which is a materially bigger implementation and runtime cost for what is
likely a marginal win on clean scanned/exported CAD PDFs (as opposed to
badly-degraded historical documents, which is Howe's actual target domain).
For OCRcer's stated domain (printed documents + CAD drawings, not degraded
historical manuscripts), Sauvola with a fallback to Wolf-Jolion-style
contrast normalization on flagged low-contrast pages is the proportionate
choice; Howe is over-engineering for this corpus unless bench results show
Sauvola failing on a specific scan class.

---

## 4. Touching/broken character handling

**Drop-fall algorithm.** Simulates a ball/marble rolling down the contour of
a connected component from the top, following the path of least resistance
along the glyph boundary/valley between two touching characters, and cuts
along the resulting path. Long lineage in the literature (Lu & Shridhar's
1996 *Character Segmentation in Handwritten Words: An Overview*, Pattern
Recognition 29(1):77-96, is cited as foundational), with later variants
unifying "free-falling" segmentation on external contours with "tunnel"
segmentation through internal touching regions, and a 2015-era variant
explicitly built on "digital features for touching digit segmentation."
Source: [A new drop-falling algorithms segmentation touching character — IEEE](https://ieeexplore.ieee.org/document/5552365/),
[A novel drop-fall algorithm based on digital features for touching digit
segmentation — IEEE 2016](https://ieeexplore.ieee.org/document/7746350/),
[Methods and strategies on off-line cursive touched characters segmentation:
a directional review — Artificial Intelligence Review (Springer)](https://link.springer.com/article/10.1007/s10462-011-9271-5).

**Why it matters here.** This is the classical counterpart to the lattice
over-segmentation approach already in the architecture: rather than (or in
addition to) generating multiple candidate cut points and letting the
Viterbi decoder pick among lattice paths, drop-fall gives a single
geometrically-principled candidate cut for a touching pair, which is cheap to
compute and can be used to seed/prioritize lattice cut-point candidates
instead of relying purely on vertical-projection valleys (which fail when two
characters touch above the baseline with no valley at all, e.g. touching
serifs or kerned pairs in tight CAD dimension text). Implementation cost:
low-medium — it's a per-component contour-following procedure, no external
dependency, fits the "authored/deterministic" and `#![forbid(unsafe_code)]`,
zero-dependency constraints directly (pure array/contour walk).
Cross-reference: the *broken*-character side of this problem (a single
character split into 2+ components, e.g. a scan artifact severing a stroke)
is the dual case and is typically handled by a merge heuristic (small
components within a size/proximity threshold of a neighbour get merged
before classification) — no single well-known named algorithm was found for
this in the pass; treat "merge broken components by size+proximity+baseline
alignment threshold" as a design choice to author and justify locally, not a
technique with a citable name.

---

## 5. Font-adaptive classification (per-page adaptive prototypes)

**Tesseract's adaptive classifier.** Tesseract runs recognition in two
passes over a page. Pass 1 recognizes each word with the static
(pre-trained-per-corpus) classifier; every word whose result is judged
"satisfactory" is fed as training data into a separate *adaptive* classifier
that is specific to the current document. Pass 2 re-recognizes (or continues
recognizing) later text using the adaptive classifier, which — because it
has now seen this document's actual font instances — discriminates that
particular font's glyph shapes more sharply than the general-purpose static
classifier can. The documented difference in normalization between the two
classifiers: the adaptive classifier uses isotropic baseline/x-height
normalization, while the static classifier normalizes by centroid position
and (anisotropic) second-moment size. Source:
[An Overview of the Tesseract OCR Engine — Ray Smith, Google (search-result
summary; direct PDF fetch failed with 403/unreadable-binary in this pass — see note)](https://research.google.com/pubs/archive/33418.pdf),
corroborated via: [tesseract::Classify Class Reference](https://tesseract-ocr.github.io/tessapi/3.x/a00319.html),
[Adapting the Tesseract Open Source OCR Engine — MOCR workshop PDF](https://tesseract-ocr.github.io/docs/MOCRadaptingtesseract2.pdf).
**Note on source reliability:** the Ray Smith PDF itself could not be parsed
by the fetch tool in this pass (binary PDF, and the direct URL also 403'd
before redirecting); the description above is reconstructed from search-
result snippets of that paper plus the two corroborating pages, not a full
read of the primary text. Treat the normalization-method detail (isotropic
vs. anisotropic) as needing a follow-up direct read before being relied on
for an implementation decision.

**Why it matters here, and how to keep it deterministic (rule 1).** This is
the one technique on this list that looks superficially like "training,"
which the project's core rule forbids. The compliant framing: it is *not*
gradient learning, it is **nearest-neighbour prototype augmentation** —
exactly the matching primitive OCRcer already has. A page-adaptive prototype
bank would work by: (a) running the existing rendered-font prototype match
per glyph as today, (b) for glyphs whose match margin clears a high-
confidence threshold (reusing the calibrated confidence machinery in rule 5,
not a new invented number), promoting that glyph's own 107-dim feature
vector into a small per-document prototype set, tagged with its accepted
label, (c) matching subsequent low-confidence glyphs on the same page against
*both* the static rendered-font bank and the growing per-document set, with
the per-document entries preferentially weighted since they're drawn from
the actual in-document font instance rather than a rendered approximation.
Every number involved (the promotion threshold, the weighting) is the same
match-margin/calibration-curve machinery already specified for confidence —
no new "trained" parameter is introduced, only a deterministic accept/reuse
rule over data computed at runtime, and it is fully re-derivable and
inspectable per rule 1. Implementation cost: medium — mostly bookkeeping
(a per-document prototype cache scoped to the page/document decode) layered
on the existing coarse-to-fine matcher; no new feature representation and no
new decoder logic required.

---

## Summary ranking (see also the top-of-response TL;DR)

Ranked by expected accuracy payoff per unit of implementation cost, given
OCRcer's actual corpora (printed documents + CAD drawings, both machine-
generated most of the time, not degraded historical scans):

1. **Fixed-pitch detection + pitch-based chopping** (§1) — directly fixes a
   named, concrete failure mode (`112.50` mis-split) that is very likely to
   recur constantly in CAD BOM/dimension tables, for low-medium cost, reusing
   existing connected-component/gap infrastructure.
2. **Sauvola adaptive binarization** (§3) — cheap, closed-form, well-proven
   floor-raiser for any scanned/photographed input, displaces nothing
   currently planned, integral-image implementation is O(1) per pixel.
3. **Axis-aligned maximal-whitespace-rectangle column/gutter detection** (§2)
   — the best fit for invoice/statement/title-block reading order given
   deskew already runs first, at a materially lower cost than the full
   arbitrary-orientation version Breuel published.

Docstrum and drop-fall are good second-tier candidates (§2, §4) — worth
having as fallbacks/refinements once the above three are in and measured
against real corpus failures rather than committed to speculatively. The
adaptive-prototype idea (§5) is the highest-value but highest-integration-
cost item and depends on the calibrated-confidence machinery from a later
chunk being in place first, so it is a natural chunk-8-or-later target rather
than an immediate one. Howe binarization (§3) and Wolf-Jolion (§3) are noted
but not prioritized — their target domain (severely degraded historical
documents) doesn't match this project's corpus of mostly machine-generated
scans and exports; revisit only if bench (`ocrcer-bench`) surfaces a scan
class Sauvola actually fails on.

## Addendum 2026-09-23 — reading-order-independent CER (for the finfilings diagnosis)

Clausner, Pletschacher & Antonacopoulos, "Flexible character accuracy measure
for reading-order-independent evaluation", Pattern Recognition Letters 131
(2020) 390–397 (https://www.sciencedirect.com/science/article/pii/S0167865520300416;
author PDF http://www.primaresearch.org/www/assets/papers/PRL_Clausner_FlexibleCharacterAccuracy.pdf).
Edit distance over the whole serialised page mixes recognition error with
reading-order and segmentation disagreement; their measure matches substrings
(lines) flexibly so ordering variations are not charged as character errors.
It has been used in ICDAR layout competitions since 2017. Relevance: the finfilings truth
comes from HTML text lines, not from the rendered layout, so a line-matched
CER (pair each truth line with its best OCR line, then charge edits) would
separate "misread glyphs" from "different line/order" before anything is
concluded about the 19.5% figure. Reported as a second metric beside the
existing CER, never a replacement; the method is described in the
paper and would be re-implemented, with no code taken from anywhere.

## Addendum 2026-09-23 — how classical engines handle fi/fl ligatures

Tesseract's unicharset lets one class ("unichar") produce a multi-character
UTF-8 string, so a fused `fi`/`fl` glyph is a single class that emits two
letters; uncommon ligatures without code points (`ct`) take private-use
values internally. The legacy (3.0x) classifier adds a "shapetable" layer
between the classifier and the word recogniser, so one matched shape can stand
for a set of unichar ids (https://tesseract-ocr.github.io/tessdoc/tess3/Training-Tesseract-3.03%E2%80%933.05.html).
The project itself records the ambiguity of ligatures coexisting with the
plain letters as unresolved (https://github.com/tesseract-ocr/tesseract/issues/1894).
Relevance to OCRcer: the equivalent is a prototype whose label is a class
*sequence*, consumed by the lattice as one segment emitting two characters.
That is a prototype-label/format change (`ARCHITECTURE.md` §7, charset rule),
so it waits on the finfilings audit's prevalence count before any decision.

## Addendum 2026-09-23 — separating tightly-leaded / merged text lines

The survey literature splits line segmentation into two families: methods using the
inter-line gap (projection profiles, smearing, fringe maps) and methods using
the relationship among characters of one line (connected-component grouping,
baseline following) (Likforman-Sulem et al., survey, https://arxiv.org/pdf/0704.1267).
For touching or merged lines, the common recipe is: detect "pseudo-lines" whose
height is abnormal against the page's character height from the horizontal
projection, estimate a local baseline for each real line, then assign each
component to the nearest baseline (a component that genuinely touches both
is split at the intersection) (Tibetan touching-line work,
https://www.sciencedirect.com/science/article/abs/pii/S0306457321001746;
IJDAR 2025 review, https://link.springer.com/article/10.1007/s10032-025-00526-w).
Relevance: OCRcer's merged bands already have a detectable signature (band
height against its members' heights, x-height collapsing). The candidate
repair is "detect an over-tall band, find its two baseline modes in the
width-weighted bottom-edge histogram, reassign members to the nearer
baseline", which reuses the machinery `measure` already has. Pending
`ocrcer-runtime`'s phase-1 trace of what actually bridges the lines.

## Addendum 2026-09-23 — italic: prototypes vs. shear normalisation

Classical engines handle italic one of two ways: add slanted prototypes, or
estimate the dominant slant of a word/line and undo it with a shear before
segmentation and matching. The usual estimate is a slant projection: shear
the image over a range of angles and keep the angle whose vertical projection
has the most blank columns (the "maximum white columns" criterion), then
apply the inverse shear (Italic Detection and Rectification, JISE,
https://jise.iis.sinica.edu.tw/JISESearch/fullText;jsessionid=94a599032a1d982364e00f403c9b?pId=798&code=3C10F415D25FB98;
gradient-direction skew/slant, https://engr.case.edu/merat_francis/EECS%20490%20F04/References/Document%20Deskew/00619830.pdf;
tilt correction of italic character images, https://link.springer.com/chapter/10.1007/978-981-95-7996-9_18).
One report puts the recognition gain from slant correction at up to 9%
(https://www.academia.edu/59930496/Estimation_of_Tilt_in_Characters_and_Correction_for_better_Readability_by_OCR_Systems;
that is a claim from the paper, not measured here).

Consequence for OCRcer (analysis, nothing decided yet): italic prototypes
are the cheap first step under the architect protocol (a font-coverage
fix, no format change), and they are queued to be measured like bold. But
prototypes alone cannot fix *segmentation*: italic glyphs overlap
horizontally, so gap-based word and glyph cuts see fewer clean gaps. If
the italic-prototype reading shows gains on classification and not on
segmentation, the next candidate is a per-word shear pass (max-white-column
slant estimate, then shear) in `ocrcer-core`. That would be a new pipeline
stage, so it needs a §6 entry and golden fixtures, but it does not touch
the charset, the features or the format.

## Addendum 2026-09-23 — dense tables: remove ruling *pixels*, not ruling *components*

Table-OCR pipelines remove grid lines at the pixel level before connected
components are extracted. They use a morphological opening with a long
horizontal and a long vertical structuring element (one pipeline uses 50 px),
which keeps only streaks longer than any character stroke. They subtract that
from the binary image, then label components on what is left
(Financial Table Extraction in Image Documents, https://arxiv.org/html/2405.05260v1;
invoice table pipeline, https://arxiv.org/pdf/2507.07029).

Why this is a different lever from what OCRcer has (analysis, nothing
decided): both `lines.furniture_fraction` and `lines.rule_aspect` judge a
*whole component*. When a cell rule touches the digits in the cell, rule and
digits are one component. A component filter must either keep all of it
(the rule then pollutes line banding and matching) or drop all of it (the
digits are deleted). That may explain why every `rule_aspect` value measured
worse. Pixel-level removal separates the two cases. The opening length would
be scale-free if tied to the measured em-dash aspect bound
(`ocrcer-build aspect`) times the page's median component height, so no new
guessed constant is needed. Candidate for the r000022 dense-table trace:
first check whether its lost text is rule-touching, before proposing it.

## Addendum 2026-09-23 — cutting touching glyphs: contour valleys and drop-fall

Classical cut-point sources for fused characters, beyond the column
projection (which is what `segment.valley_fraction` uses):
- **Drop-fall**: a "marble" is dropped from above (and from below)
  between the two glyphs. It rolls along the ink boundary and falls through
  the thinnest junction, and its path is the cut, which may be non-vertical.
- **Contour valleys**: trace the valleys of the upper contour, and of the
  lower contour (image flipped). A cut is proposed where an upper valley and
  a lower valley line up.
- The ratio of the second difference of the vertical projection to its
  peak-to-valley depth, as a cut score.
(Sources: fuzzy touching-character segmentation, https://arxiv.org/pdf/1612.04862;
segmentation survey, https://www.academia.edu/981409/Segmentation_methods_for_character_recognition_from_segmentation_to_document_structure_analysis;
USPTO 9922263, https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/9922263.)

Relevance (analysis, not measured): r000022's grid loses `0$` fused into
one component at an x-height of about 10 px. A column-profile minimum between
a round `0` and the full-height stem of `$` need not fall to half the local
mean, so `valley_fraction` 0.5 (a guess) may offer no cut there at all. The
lattice already accepts extra cut candidates without committing to them (§5),
so contour-valley and drop-fall cuts can be added as *additional candidates*
and the matcher decides. That changes neither the format, nor the features, nor
the decoder. First step is the trace: confirm whether any cut was offered
inside the fused component, and what the matcher scored it.

## Addendum 2026-09-23 — underline removal without eating text

Written after the first underline strip failed on finfilings. Classical
underline removers use three safeguards that our first rule lacked:
1. **Position.** An underline sits at or below the text baseline. Detection
   uses bottom-edge analysis of the component, and a long run *above* the
   baseline zone is not a candidate (Underline detection and removal using
   multiple strategies, https://ieeexplore.ieee.org/document/1334314/).
2. **Thin vertical runs only.** At each column, erase only the pixels whose
   *vertical* run length matches the rule's thickness. Where a character
   stroke crosses the rule, the vertical run is longer and is kept, so
   intersecting strokes survive (US20150052426A1, https://patents.google.com/patent/US20150052426;
   approximate digital straightness, https://www.academia.edu/55281672/Detection_and_removal_of_hand_drawn_underlines_in_a_document_image_using_approximate_digital_straightness).
3. **Recognition disambiguation.** For a doubtful case, remove the candidate
   only if recognition confidence improves with it removed (same IEEE paper).

Relevance (analysis; the r000033 damage trace is pending): (1) and (2) map to
testable geometric conditions (run below the component's baseline zone;
erase only vertical runs ≤ measured rule thickness). (3) maps onto OCRcer's
existing match margin: strip only if the stripped component's pieces match
with better margins than the whole. Which of these separates the false hits is
for the trace to show, not to be assumed.

## Addendum 2026-09-23 — recognition-gated chopping (for the split-gate result)

Context: lowering `segment.split_min_x_heights` from 1.15 to 1.0 helped
finfilings (CER 17.064 → 16.885) but hurt pages-cov (6.127 → 6.330). On
clean synthetic pages the extra cuts are mostly over-segmentation.

Tesseract does not offer a chop just because a blob is wide enough. It
chops the blob the classifier is *least* confident about, and only when the
word result is unsatisfactory. It then undoes any chop that does not improve
confidence, while keeping the piece for the associator's best-first search
(Smith, "An Overview of the Tesseract OCR Engine", §4.3; DAS 2016 tutorial,
part 4).

The analogue here would be to offer interior cuts on narrow atoms (below
the current width gate) only when the whole atom's own match margin is below
the calibrated low-confidence point. Well-recognised narrow atoms (`m`,
`w`, `rn`-prone shapes) then stay whole. This costs one extra match per
narrow atom, which already happens for the unsplit path.

This is **not measured**. It is a candidate if a plain width threshold cannot
pass both gates.

Sources: https://research.google.com/pubs/archive/33418.pdf ;
https://tesseract-ocr.github.io/docs/das_tutorial2016/4CharSegmentation.pdf

## Addendum 2026-09-23: drop-fall cuts (queued behind atom merge by overlap fraction)

Drop-fall simulates a droplet falling from the top (or bottom) of a touching
pair and follows the contour, so the cut is not a straight vertical line.
There are four variants (top-left, top-right, bottom-left, bottom-right
start), and each gives a different candidate path. That fits OCRcer's lattice:
every path becomes one more cut candidate, and the decoder chooses among them.
Known weaknesses: picking the start point, and seeping straight down through
a vertical stroke. The improved variants fix both with a start-point rule and
modified dripping rules.

This applies only to touching ink. r000583's letters are pixel-disjoint (see
the worst-page note), so drop-fall does not address that page. Not measured.

Sources:
- Cao/Huang, "A new drop-falling algorithms segmentation touching character", IEEE 5552365: https://ieeexplore.ieee.org/document/5552365
- "A novel drop-fall algorithm based on digital features for touching digit segmentation", IEEE 7746350: https://ieeexplore.ieee.org/document/7746350/

## Addendum 2026-09-23: checkbox detection (queued; the round-3 checkbox-glyph mechanism)

Classical checkbox detection works on a component's geometry, not by
character matching:
- a near-square bounding box (aspect ≈ 1);
- a hollow interior, measured as a low fill ratio inside an inset of the box
  (ticked or filled boxes are high);
- a size band tied to the text size;
- an optional quadrilateral test on the contour.

Wide, shallow rectangles (aspect > 1.5) are text-input fields, not checkboxes.

For OCRcer this suggests a pre-matching filter. A component that passes the
square-and-hollow test is emitted as a declared symbol (☐/☒, if the charset
decision adds them) or dropped as "not text". It never reaches the prototype
matcher, which is currently forcing it to `®`/`~`/`B`. Whether truth
transcribes the boxes decides between emit and drop. Check that first.
Not measured.

Sources:
- Fuzzy Labs, "Checkbox Detection with OpenCV": https://www.fuzzylabs.ai/blog-post/checkbox-detection-with-opencv
- J. Rodriguez, "Checkbox Detection: OpenCV vs YOLO": https://www.jeremias-rodriguez.com/blog/checkbox-detection-opencv-vs-yolo
- Loichau, "Apply computer vision on the questionaire image to detect ticked checkboxes": https://loichau997.medium.com/apply-computer-vision-on-the-questionaire-image-to-detect-ticked-checkboxes-646e9245d293

## Addendum 2026-09-24: concavity-pair chops (Tesseract's chopper), as a cut-candidate source

Read, not measured. Sources: Tesseract `wordrec/chop.h` / `chopper.cpp`
reference docs, and the "Breaking down Tesseract OCR" summary.

- A chop candidate is a **pair of points on the outline**. At least one is
  a concave vertex of a polygonal approximation of the outline; the other
  is an opposite concave vertex or the nearest outline segment. The split is
  the straight segment between them, so it need not be vertical.
- A concavity is outline that does not lie on the convex hull. The candidate
  point in it is the one furthest from the hull line, as a local extremum.
  Separating one joined ASCII pair can take up to about 3 chop pairs.
- Chops are tried in priority order on the lowest-confidence blob. A chop
  that does not improve recognition is undone but kept, so the associator
  (the segmentation search) can still use it later.

How this relates to OCRcer: our lattice already plays the associator's role,
and recognition already arbitrates between candidate cuts. Concavity pairs
would be an extra candidate source, next to projection minima. They are the
natural way to handle serif chains joined at the baseline, where the
vertical projection has no minimum because the serif spans the gap. A
concavity above the serif and one below it (or the baseline) define a short,
nearly vertical cut through the serif. Whether r000583 needs this depends on
the cut-candidate diagnosis (`2026-09-24_cut_candidates_r000583.md`).

## Addendum 2026-09-24: slant estimation and deslant, for the held deslant option

Read, not measured. Sources: "Slant estimation algorithm for OCR systems"
(Pattern Recognition, ScienceDirect S0031320300001539); the Fast-Hough slant
rectification in a passport OCR system (ResearchGate 315365387); and the
survey of deslanting methods for historical documents (J. Imaging 4(6):80).

- **Standard classical method:** shear the binarized line or word by
  candidate angles α and score each result's vertical projection for
  "peakiness". Sources give it as a maximum of a profile functional, e.g. a
  sum of squared column counts or a Wigner–Ville energy. Pick the α that
  maximises the score and apply that shear. The functional peaks when
  vertical strokes line up with columns.
- **Alternative:** a Hough transform over near-vertical stroke edges, from
  the x-derivative of the line image. The histogram of their angles gives
  the slant directly.
- **Printed italic is the easy case.** The slant is uniform per run of text,
  typically 10–15° for Latin serif and sans italics. That is unlike
  handwriting, where slant varies within a word and needs non-uniform
  methods. A per-line or per-word uniform shear is enough.

How this would fit OCRcer, if steps 1–2 of the 2026-09-24 italic decision
leave a residual:
- Estimate α per word with a small integer-degree search (0..20°) on a
  sum-of-squares projection score. That is deterministic, integer-only and
  zero-dep.
- Shear only when the best α beats α=0 by a margin, a guess threshold, so
  upright CAD text is untouched by construction.
- Shearing happens before cut-candidate generation, so slanted touching
  letters recover vertical minima.
- The extractor would then see deslanted italic glyphs, and these match the
  upright bank better. So deslant and italic prototypes partly overlap, and
  the residual decides whether both are needed.

## Addendum 2026-09-24: shrinking the bank by prototype selection (condensing and editing)

Why this is here: the slant-gated italic bank took the model from 1.99 MB to
5.37 MB, at 50,095 prototypes. The ARCHITECTURE §11 entry for that change
pointed at "the deferred bank-pruning work". **No such work item exists in the
docs.** This addendum is the first record of it.

The classical literature splits prototype reduction into two families. Both
are deterministic selection over existing rows, which is compatible with rule
1: the kept set is computed by a re-runnable script, and nothing is fitted.

- **Condensing (Hart's CNN and variants).** Discard prototypes far from any
  class border, because they are redundant: every query they would win is won
  anyway by a same-class neighbour. This shrinks the bank without moving the
  decision boundaries on the selection set.
- **Editing (Wilson's ENN).** Discard prototypes whose own neighbours
  mostly belong to another class. These are noisy border rows. Of the two,
  this one can change accuracy, in either direction.

The survey tracks cited below also have a third family, *prototype
generation*: LVQ, centroids, gradient or annealing optimisation. It
**synthesises** new vectors from an objective, which makes it fitting. It is
outside rule 1 and is not an option here.

Constraints specific to OCRcer if this is ever scheduled:
- Hart's CNN is order-dependent, so the visit order must be fixed (prototype
  row order) for byte-reproducibility.
- Selection must use the runtime's own distance: int8, with feature weights,
  through the same `nearest()` path. A second distance would be a second
  implementation of a stage (rule 4).
- The selection set is the rendered bank itself. Condensing can therefore only
  promise "no change on renders", not "no change on scans". The usual gates
  still decide.
- Italic and upright prototypes must be condensed within their own gating
  pools. Otherwise an upright query could lose its only surviving neighbour
  to an italic row it is no longer allowed to see.
- Size is not currently a gate. This is a candidate, not a scheduled task.

Sources: [Prototype Reduction in Nearest Neighbor Classification — SCI2S](https://sci2s.ugr.es/pr);
[Prototype Selection for Nearest Neighbor Classification: Survey of Methods](https://sci2s.ugr.es/sites/default/files/files/TematicWebSites/pr/T-4-2010-PSMethods.pdf);
[Evaluation of prototype learning algorithms for NN classifiers in character recognition](https://www.academia.edu/14544487/Evaluation_of_prototype_learning_algorithms_for_nearest_neighbor_classifier_in_application_to_handwritten_character_recognition).

## Addendum 2026-09-24: forced alignment for real-scan glyph samples (PLAN chunk 13)

**Problem.** Chunk 13 needs character-labelled crops from real scans. The
training pages carry a transcription, not per-character boxes.

**The classical answer is forced alignment.** Take the page's own
segmentation lattice and constrain the decoder to emit exactly the
ground-truth string for a line or word. The best path through that
constrained lattice assigns each truth character to a cut span, and that span
is the crop. The literature does the same with word boxes plus
transcriptions, or with character boxes transferred from an electronic
original.

What this means for OCRcer:

- **Use core's own segmenter and Viterbi, with the truth as a hard
  constraint.** No second segmenter (rule 4). The crops are then exactly the
  spans the runtime would see, cut by the same rules, which is what a
  prototype row must represent.
- **Align lines first, then characters.** The existing line-matched scorer
  already pairs predicted lines with truth lines, and that pairing is the
  first stage.
- **Accept conservatively.** Keep a crop only when all of these hold:
  - the constrained path exists;
  - every character's matched distance to its truth class is within that
    class's rendered-bank spread;
  - the unconstrained decode agrees on at least the word's neighbours.

  A mislabelled crop in a nearest-neighbour bank is a permanent wrong
  answer. Rejecting a good crop costs only coverage.
- **Diplomatic transcription matters.** Truth text that normalises glyphs
  (ligatures, quotes, dashes) produces mislabelled crops unless it is mapped
  through the charset's own folding first.
- **The training split only.** The manifest check refuses `score` rows
  (chunk 12).

Sources: [Automated OCR Ground Truth Generation — IEEE](https://ieeexplore.ieee.org/document/4669952/);
[Automatic extraction of character ground truth data from images — USPTO 8755595](https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/8755595);
[Aligning Ground Truth Text with OCR Degraded Text](https://www.researchgate.net/publication/333945862_Aligning_Ground_Truth_Text_with_OCR_Degraded_Text).
