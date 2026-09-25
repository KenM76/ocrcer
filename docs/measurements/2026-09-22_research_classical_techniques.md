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


## Addendum 2026-09-24: augmentation for the chunk-15 classifier — use Baird's defect model

The chunk-15 contract lists noise, blur, threshold jitter and sub-pixel
shift. Baird's document-image defect model is the classical, explanatory
version of that list. It is a closed set of physical parameters:
- size (the spatial sampling rate);
- rotation (skew);
- horizontal and vertical scaling, set independently;
- sub-pixel translation;
- per-pixel jitter;
- Gaussian point-spread (blur);
- per-pixel sensor-sensitivity noise;
- the binarisation threshold.

Recommendation for chunk 15:
- Draw each augmentation from these parameters with a seeded PRNG, and
  record the ranges in the build as `guess` until real-scan crops from
  chunk 13 can check them.
- Add the three missing axes: small rotation, anisotropic scale, and
  per-pixel sensitivity noise.
- The ranges can later be checked against measured scan statistics
  (stroke width, edge blur) on the training split. That keeps the
  augmentation explainable parameter by parameter (rule 1).

Tooling like Augraphy (Python) is not usable. Rule 4 requires a single
Rust implementation, and Augraphy's effects are descriptive rather than
physical.

Sources: [The State of the Art of Document Image Degradation Modelling — Baird](https://link.springer.com/chapter/10.1007/978-1-84628-726-8_12),
[Augraphy](https://arxiv.org/pdf/2208.14558).

## Addendum 2026-09-24: per-page adaptive prototypes (document-specific shape model)

Every glyph on a CAD sheet or a filing is usually set in one or two faces,
and those faces are often missing from the bank (SHX stroke fonts, scanner
blur). The classical answer is to let the page train itself. Two papers
describe it:
- **Kae & Learned-Miller (CVPR 2010).** They bootstrap from a subset of
  words recognised with high precision, build document-specific character
  models from them, and re-read the page. *Read from the abstract only; the
  PDF host failed its certificate check.*
- **Lee & Smith (Google, ICDAR 2011), on Tesseract.** Two correction paths,
  image and language, each checked by the other. They report word error
  down 25% on scanned books, and most of that gain comes in the first
  iteration. That is *their* measurement on books, not ours.

What carries over to OCRcer, and why it fits the contract:
1. **Harvest.** Take glyphs from words that are confident after pass 1:
   calibrated confidence above a threshold, and either a lexicon hit or a
   clean identifier shape. Choosing words with the lexicon is a selection
   step. It never rewrites anything, so rule 6 is untouched.
2. **Document prototypes.** Run the harvested glyphs through the *same*
   `ocrcer-core` extractor (rule 4). Hold them in a per-page, in-memory
   extension of the bank, tagged `doc`. That needs no format change, no
   `.ocrw` version bump, no new dependency, and it stays wasm-safe.
3. **Re-match only low-margin glyphs** against bank ∪ doc prototypes. Doc
   prototypes get a small distance discount, fitted on the training split
   (rule 1). High-margin glyphs keep their pass-1 result, which caps the
   extra cost.
4. **Cluster consistency (Lee & Smith's Master/Reject).** A pass-1 glyph
   whose features sit closer to a large doc cluster of a *different* class
   than to its own class is a correction candidate. This check is purely
   image-side, so it is safe inside identifiers. Lee & Smith also cluster
   *pairs* of glyphs, and that fixes segmentation errors like `rn`/`m`
   without a new segmentation pass.
5. **Determinism.** Harvest in a fixed order (line, then x), cluster
   deterministically, and break ties as §8.2 does. Golden fixtures stay
   meaningful.

What does NOT carry over:
- **The document cache lexicon** (P = max(cache, base)). It reinforces a
  *consistent* error, the same misread on every occurrence, with high
  confidence. That is exactly the failure rule 6 exists to prevent in
  identifier-heavy CAD text. Park it. Revisit it only for prose regions,
  gated off in identifier context.

Proposed as a candidate chunk after 13. Chunk 13 harvests real-scan
prototypes *offline* with ground truth; this harvests them *online*,
without it. Chunk 13's forced-alignment code is the natural base.
Expected cost is one extra match on low-margin glyphs only, which has not
been measured yet. Accuracy has not been measured either.

Sources: [Kae & Learned-Miller, CVPR 2010](https://mlanthology.org/cvpr/2010/kae2010cvpr-improving/),
[Lee & Smith, Improving Book OCR by Adaptive Language and Image Models](https://tesseract-ocr.github.io/docs/Improving_Book_OCR_by_Adaptive_Language_and_Image_Models.pdf).

## Addendum 2026-09-24: Sauvola window vs text size, and multiscale Sauvola

Trigger: on pdfcer's smoke page, OCRcer scores 97.9% at 200 dpi and drops to
83.0% at 300 dpi (x-height 30 px), with the letters themselves broken up
(`Recogmtion`, `quaüty`, `anä`).
See `2026-09-24_pdfcer_smoke_misses.md`, one page.

`image/binarize.rs` uses a fixed Sauvola window of 25 px. Lazzara & Géraud
(IJDAR 2014) document the failure this invites: when the window is small
next to a glyph, the pixels inside a stroke look like flat background, and
Sauvola's contrast term can no longer call them ink. Their best
single-scale window was w = 51 on 300-dpi documents, twice ours. That the
300-dpi garbling comes from our window is a **hypothesis**. It is plausible
and not yet measured.

Their fix, multiscale Sauvola, handles mixed text sizes on one page, which
matters for CAD title blocks next to small notes:
1. Subsample the grey image by 2, three times, giving scales 1..4.
2. Binarise every scale with the same window w. At scale s, that equals a
   window of q^(s-1)·w at full size. Keep the components whose area falls
   in that scale's range:
   - scale 1: max = 0.7·w²;
   - higher scales: min = 0.9·max(s−1)/q², max = max(s−1)·q²;
   - the last scale has no upper limit.
3. Label each full-size pixel with the scale that found its component. An
   object found at several scales takes the highest one. Spread labels
   over non-object pixels with an influence-zone (discrete Voronoi) pass.
4. Threshold every pixel with its own scale's Sauvola threshold.

Their reported result: about the same as single-scale on small and medium
x-heights, and clearly better on large text. That is their measurement.

For OCRcer:
- Everything is integral images, subsampling and component labelling.
- It is deterministic, has no dependencies and is safe for wasm32.
- It reuses `components.rs`.

A cheaper first step is to measure a page x-height from a first pass, then
re-binarise with w scaled to that x-height, with the scale factor fitted on
the training split. Measure that first. Go multiscale only if mixed-size
pages still lose.

Candidate runtime task. Diagnosis comes first: dump the 300-dpi binarised
page and check whether the garbled glyphs are hollowed or broken strokes.
Gates are pages-cov (fixed dpi) plus a dpi sweep on pdfcer-style renders,
so this also informs the open "what dpi should pdfcer use for OCRcer"
question.

Source: [Lazzara & Géraud, Efficient Multiscale Sauvola's Binarization, IJDAR 2014](https://www.lre.epita.fr/dload/papers/lazzara.13.ijdar.pdf).

## Addendum 2026-09-25: Tesseract weights character costs by ink, so path length carries no bias

The four decoder losses in the r000583 autopsy were one merged edge beating
two good single-letter edges. The global `char_bonus` re-sweep then showed
that the only lever against that bias trades prose against drawings at every
step. Tesseract's legacy segmentation search has no per-character credit at
all. These lines were read from tesseract `main` source on 2026-09-25:

- `classify/adaptmatch.cpp` (`ConvertMatchesToChoices`):
  `Rating = (1 - match) * rating_scale * BlobLength`. `BlobLength` is the
  outline length divided by `kStandardFeatureLength`, and `rating_scale`
  defaults to 1.5 (`classify.cpp`).
  `Certainty = -(1 - match) * certainty_scale`, with `certainty_scale` 20
  (`dict.cpp`). Certainty is *not* length-scaled.
- `wordrec/language_model.cpp` (`GenerateNgramInfo`): the combined
  classifier and n-gram cost is multiplied by
  `outline_length / language_model_ngram_rating_factor` (16.0). The n-gram
  part is weighted by `language_model_ngram_scale_factor` (0.03).
- The dictionary and consistency adjustments multiply the path cost
  (`ComputeAdjustedPathCost`: `adjustment = 1 + penalties`). They do not add
  a per-character constant.

So a path's cost is an ink-weighted sum. Cutting the same ink into more or
fewer pieces does not change the total's scale. A merged reading must match
about as well as the pieces it replaces, averaged by ink. The per-glyph
acceptance quantity (certainty) is kept separate and unweighted, which is
how OCRcer's margin-based confidence is already kept.

**Fit to OCRcer:**
- Every `Hyp` carries `x0`/`x1`, which are lattice node positions. So edge
  widths along any start-to-end path sum to the word's width exactly. A
  width-weighted credit is therefore identical for every path and cannot
  bias length.
- Width is the exact form here. Ink, Tesseract's form, is not exact:
  `crop` restricts a piece to its atoms' labels, so ink need not add up.
- Proposed as a mode switch and specified in `ARCHITECTURE.md` §11 on
  2026-09-25. That it removes the prose-versus-drawing trade is a
  hypothesis for the gates to test.

## Addendum 2026-09-25: Tesseract's chop scoring, read from source

Read, not measured. This sharpens the 2026-09-24 concavity-pair addendum,
which was based on reference docs. Sources: `wordrec/chop.cpp`,
`findseam.cpp`, `gradechop.cpp`, `wordrec.cpp` (defaults),
`ccstruct/split.cpp`, `seam.cpp`, `normalis.h`, `pageres.cpp` (Tesseract
`main`).

Chopping runs on the baseline-normalised word, where the x-height is 128
units (`kBlnXHeight`). So each default below converts to x-heights.

- **Candidate points.** These are the outline's local y-extrema that pass
  a direction test, plus any vertex that turns inward by more than 50°
  (`chop_inside_angle` −50). Each is prioritised by its turn angle.
- **Pairs.** Two candidate points form a split if they are not neighbours
  and neither lies outside the other's outline. The weighted length is
  `3·dx² + dy²` and must be under 10000. So a vertical split is at most
  0.78 x-height long, and a horizontal one at most 0.45 x-height. The 3×
  weight on dx is what makes near-vertical cuts preferred without
  requiring them.
- **Vertical splits.** A candidate point is also paired with the nearest
  outline crossing straight above or below it. That is our projection cut,
  anchored at a concavity instead of at a column minimum.
- **Split score.** The partial score is `0.5·√(weighted length)`, plus
  `0.06·(angle₁ + angle₂ + 360)` (zero once the two angles sum below
  −360). Lower is better.
- **Seam score.** The full score adds three terms:
  - `0.9 ×` the pixel overlap of the two pieces' boxes, or 100 if one box
    contains the other;
  - `0.15 × |w₁ − w₂|` (capped), applied only when a piece is at most
    0.70 x-height wide;
  - a width-change term.
- **Limits.** A seam scoring at least 100 (`chop_ok_split`) is refused. One
  scoring under 50 (`chop_good_split`) ends the search early. A seam may
  combine up to three splits. Each resulting piece needs at least 6 outline
  points and an area of at least 2000 units², which is 0.12 x-height².

How this maps to OCRcer, if a missing-cut census ever justifies it: the
pair search and the seam score would produce extra cut candidates, and the
decoder would still arbitrate between them. The overlap term is the part
our vertical-only cuts cannot express. A slanted or kerned pair (`Te`,
`ry` in italic) has overlapping boxes, so no vertical line separates it.
Whether that error class is common enough to pay for polygonal outlines in
`ocrcer-core` is exactly the census question. No chunk is proposed here.

## Addendum 2026-09-25: characters touching drawing lines (Tombre et al., DAS 2002)

Read, not measured. Source: Tombre, Tabbone, Pélissier, Lamiroy, Dosch,
"Text/Graphics Separation Revisited", DAS 2002, §4.

- Connected-component separation cannot recover a character whose ink
  touches a line. Such characters are merged into the graphics layer.
- Tombre's method extends each string that was found. It fits the string's
  direction (median regression when the string has more than 4
  characters), then sets search areas at each end, sized by the mean
  character width and spacing. Inside a search area it computes the
  distance skeleton and cuts it at multiple points that join the outside
  graphics exactly once. It then rebuilds each cut-off part by the inverse
  distance transform as a candidate character.
- Reported yield on five drawing extracts: 25 of 70 touching characters
  recovered, raising final recall by 5 to 10 points. Dashed lines cause most
  of the false positives. A string that touches the graphics everywhere has
  no seed, so it is never recovered.
- Relevance to OCRcer: this would be a drawing-only stage after line
  grouping. Whether touching text is a real error class on pages-cov
  drawings is unmeasured, so it needs a census of drawing misses by cause
  first. No chunk is proposed.

## Addendum 2026-09-25: Tesseract's case check needs no line x-height

Read from source, not measured. Sources: `wordrec/lm_consistency.cpp`,
`lm_consistency.h`, `language_model.h`, `language_model.cpp` (Tesseract
`main`).

- Every classifier choice carries `min_xheight` and `max_xheight`. That is
  the range of line x-heights consistent with this blob being this
  character, derived from the blob's size and the character's trained
  top and bottom range.
- As a path grows, `ComputeXheightConsistency` intersects those ranges,
  separately for normal, subscript and superscript positions. The path
  becomes `XH_INCONSISTENT` when any intersection is empty, or when more
  than 40% of the sub- or superscript characters are punctuation, or when a
  sub- or superscript range falls below 0.4 of the mainline range. An
  inconsistent path is penalised, and it is refused as an acceptable
  choice.
- Case is scored separately. `NumInconsistentCase` is
  `min(upper case after the first letter, lower case letters)`. The
  penalty is 0.1 for the first and 0.01 more for each further one
  (`language_model_penalty_case`, `_increment`). A leading capital is
  free.

**How this differs from `case-geom`.** Our check compares a glyph's top
height with the height the bank expects for it and for its case twin. It
measures that height in units of the line's x-height, so it is only as
right as the line x-height. Tesseract's check is relative: in `Sees`, a
capital `S` implies an x-height about 0.7 times the one the `e`s imply, so
the ranges fail to intersect whatever the line estimate says. It gives no
information where every letter in the word is a case twin (`sow`/`SOW`),
and there the line x-height is the only cue.

Candidate, if the `case-geom` sweep leaves case misses on lines whose
x-height estimate is wrong: add word-internal consistency as a second arm.
Each case-twin choice would carry an implied x-height interval from its
bank heights, and each path would intersect them. Not specced.

## Addendum 2026-09-25: fitting the confidence curve, and what a word score should mean

**Why.** The character curve in `confidence.rs` (`AUTHORED`) is six guessed
knots. Nothing in the bench measures calibration yet. The LLM add-on's
low-confidence mode picks lines by this number, so a wrong curve sends the
wrong lines.

**Method: isotonic regression (PAV), not Platt.** Niculescu-Mizil and
Caruana (UAI 2005, *Obtaining Calibrated Probabilities from Boosting*) find
Platt scaling better "when the calibration set is small (less than about
2000 cases)", and isotonic regression better above that. Isotonic
regression is fitted with the pair-adjacent-violators algorithm (PAV). It
assumes only that accuracy is monotone in the score, which is the shape
`AUTHORED` already states. finfilings-val has 103 pages and far more than
2000 characters, so PAV fits. Its output is piecewise constant. Reduce it to
the struct's knots: put the knot x-values at equal-mass quantiles of the
fitting data and read y off the PAV fit. Both steps are deterministic.

**Measuring it.** Plain ECE with equal-width bins hides the sparse high-
and low-confidence regions. Use equal-mass (adaptive) bins, the ACE of
Nixon et al. (CVPR-W 2019, *Measuring Calibration in Deep Learning*), and
show the reliability diagram. Also report the one figure a reviewer acts
on: at each flag threshold, the share of errors caught against the share
of characters flagged.

**Labels.** Align output with truth per matched line, as the line-matched
CER does. An output character is correct if it aligns as a match, and
wrong if it is a substitution or an insertion. A deletion has no output
character, so no confidence can flag it. Report the deletion rate beside
the calibration figures.

**Word scores.** Tesseract reports one certainty per word. It is the
minimum over the word's characters: `WERD_CHOICE::set_unichar_id` keeps
`certainty_` as the running minimum (`ratngs.h`, read 2026-09-25). Word
confidence is then `ClipToRange(100 + 5 * certainty, 0, 100)`, and a line's
is the mean over its words (`ltrresultiterator.cpp`, read 2026-09-25). Ours
is the geometric mean of the characters. That is a per-character average,
not the probability that the word is right. Ten characters at 0.95 give a
word score of 0.95, but if errors were independent the word would be right
about 60% of the time. So a calibrated character curve does not by itself
make the word score calibrated. The word score needs its own measured
curve over the geometric mean, fitted against whole-word correctness.

## Addendum 2026-09-25: how Tesseract drops noise words (read from source)

**The garbage "crunch" is off by default.** `docqual.cpp`'s `garbage_word`
and `tilde_crunch` classify a decoded word as OK, dodgy or terrible from
its character classes, its rating and its certainty. That path runs only
when `unlv_tilde_crunching` is set, and it defaults to false
(`tesseractclass.cpp`, read 2026-09-25). It is an output mode for the UNLV
evaluation, not part of normal recognition. Do not copy it as "what
Tesseract does".

**The default path is a speck test before recognition.**
`Textord::clean_noise_from_words` (`tordmain.cpp`) runs when
`textord_noise_rejwords` is true, which is the default (`textord.cpp`).
Per word, in units of the row's x-height:
- *dot*: an outline whose longer side is under 0.5 (`textord_noise_sizelimit`).
  A blob taller than 2.0 adds two dots, unless it is the row's first blob.
- *normal*: an outline with a hole, height within ±20% of the x-height and
  width within ±40% (`syfract` 0.2, `sxfract` 0.4). Also a blob of size
  0.5 to 2.0 with fewer than 16 ink transitions (`translimit`), counted at
  a threshold of size/10.
- With more than two dots: `dots > 4 × normals` marks the word as noise.
  `dots > 2 × normals` marks it as a suspect, which is acted on only when
  noise words outnumber the good ones in the row (`normratio` 2.0).
- Action: the small outlines are removed (`WERD::CleanNoise`), not the
  whole word. The comment says the whole word used to be dropped. It now
  hands the specks to a reject list and lets the classifier decide.
- `clean_noise_from_row` applies the same counts to a whole row, with
  ratio 6.0 (`rowratio`), and keeps the row if it has at least one
  "super-normal" blob (`sncount` 1).

**Relevance.** OCRcer already drops debris by height (the
`lines.debris_heights` and `thin_debris_heights` params). This is
different: it tests a word's make-up, meaning the ratio of specks to
letter-sized shapes. It does not test each component alone. Candidate
only. It is worth specifying if the whole-train error census shows
insertions from speckled words (scan noise, halftone, dotted rules) as a
real bucket. Not specced.

## Addendum 2026-09-25: the error census should use the ISRI accuracy report's shape

**Source.** The UNLV-ISRI OCR Evaluation Tools, used in the annual OCR
accuracy tests of the 1990s. They are maintained as `ocreval` (Apache-2.0,
Unicode support added). The user guide was read on 2026-09-25. We adopt
the report's shape, not its code. Our scorer stays in `ocrcer-bench`.

**What one `accuracy` report contains**, from the guide's worked example:
1. Truth characters, errors, and accuracy. Errors are the edit operations
   needed to correct the output: insertions, substitutions and deletions.
2. Reject characters (`~`), suspect markers (`^`) and false marks. Then
   **marked character efficiency**: the share of characters marked, and
   the "Accuracy After Correction" when a reviewer fixes the marked errors.
   The example gives 1.72% marked and 94.84% → 96.96%.
3. Insertions, substitutions and deletions, split into marked, unmarked
   and total.
4. Accuracy by class: spacing, special symbols, digits, uppercase and
   lowercase. Missed truth characters always equal insertions plus
   substitutions in the guide's accounting.
5. Confusions as `{correct}-{generated}`, sorted by the errors charged.
   One confusion charges its edit count each time it occurs; `fl`→`n`
   costs 2.
6. Per-character counts for every truth character.

`wordacc` adds word accuracy, split into stopword and non-stopword
accuracy, and distinct non-stopword accuracy. `accsum` and `wordaccsum`
aggregate across pages.

**How this maps onto OCRcer.**
- Item 2 is the reviewer figure in the 2026-09-25 confidence decision
  (§11), under its industry name. Once the confidence curves are fitted,
  a threshold marks characters, and the report gives both the marked share
  and the accuracy after correction.
- Items 3 to 5 are the whole-train error census, already queued. Digit
  accuracy is the headline for financial filings, where one wrong digit
  is one wrong amount.
- The pipeline needs one dump that serves both the census and the
  calibration fitter. Per output character it records: page, line, the
  alignment operation, the truth character, the output character, the
  ratio, the agreed flag and the confidence. Deletions are recorded as
  rows with no output character.
- The ISRI report has no layout buckets. OCRcer's census adds them from
  the gap between the end-to-end and line-matched CER: missing lines,
  merged lines, and reading order.

## Addendum 2026-09-25: letters inside numbers — Tesseract's char-type consistency and number DAWG (read from source)

**What Tesseract does** (`src/wordrec/lm_consistency.h`, `language_model.{h,cpp}`,
`src/dict/dict.h`, main branch, read 2026-09-25):
- `LMConsistencyInfo` counts `num_alphas`, `num_digits`, `num_punc`,
  `num_other` along the path. `NumInconsistentChartype()` =
  inconsistent punc + `num_other` + `min(num_alphas, num_digits)`.
- A non-dictionary path costs `ratings_sum × (1 + adjustment)`. The
  adjustment sums `ComputeAdjustment(n, penalty)` per kind: 0 if n = 0,
  `penalty` if n = 1, else `penalty + 0.01·(n−1)`. Defaults: chartype 0.3,
  punc 0.2, case 0.1, script 0.5, spacing 0.05, non-dict 0.15, non-freq
  0.1. Dictionary paths get only the case and script terms.
- The number DAWG maps every digit to one pattern id (`char_for_dawg`, type
  `DAWG_TYPE_NUMBER`). A number is "in the dictionary" when its *shape*
  (`#,###.##`) is, whichever digits it holds.

**What OCRcer already has.**
- Pairwise, local version of the chartype term: 11 `digit_neighbour` rules
  in `model/confusions.tsv` (l/I/|/†→1, O→0, S→5, B→8, Z→2, G→6, g→9, T→7).
- The lexicon cannot reward a particular number. Checked 2026-09-25:
  `word_counts.tsv` holds 0 digit-bearing rows, because lexicon candidates
  must trim to an all-letter remainder (`2026-09-24_chunk14_counts.md`).
  So `2019` can never pull `2018` toward it.

**Two things Tesseract has that OCRcer does not.**
1. *Word-majority char type.* `min(a, d)` pushes a word toward its majority
   type, which a neighbour rule cannot do (`1O1O`). It is dangerous for
   short codes: `M8` scores 1 and `MB` scores 0, so it pushes a real part
   code toward letters. If built, it must be suppressed inside
   identifier-shaped words, as the case term is. In practice that leaves
   only pure-number runs, where the neighbour rules already fire.
2. *Number-shape lexicon.* Collapse digits to `#` and give a bonus to
   well-formed shapes counted from finfilings-train (`(#,###)`, `$#.##`,
   `##/##/####`). Every digit is equal under the collapse, so it can reward
   well-formedness but can never flip `8`↔`9`. That is compatible with
   CLAUDE.md rule 6 as a bonus that is never a penalty. It is the stronger
   of the two, because financial columns are where the weak blocks are
   (§11, word F1 by block type).

**Decision rule, not a decision.** Neither goes in before the whole-train
census (ISRI shape) shows a residual of letter-for-digit or
punctuation-in-number substitutions inside numeric tokens that the
`digit_neighbour` rules leave behind. If the census shows one, (2) is
specified first: fitted from train counts in chunk 14's machinery, and
gated by the identifier test and a CAD dev set, where `Ø12`, `R3` and
`M8x1.25` must be unchanged.

## Addendum 2026-09-25: a reference bar for flagging, and how ISRI priced layout errors

**Source.** Rice, Jenkins & Nartker, *The Fourth Annual Test of OCR
Accuracy*, ISRI TR-95-03 (1995), §4 and §5. Read 2026-09-25 from the PDF on
stephenvrice.com.

**Marking bar.** In the best 1995 page readers, "marked characters make it
possible for an editor to inspect only one-half of one percent of the
OCR-generated text yet correct 20 to 45% of the errors in the text". Past
that point the curves flatten, because false marks dominate. That is a
reading of 1995 commercial engines, not a gate. It gives the
marked-efficiency table from `fit-calibration` one external comparison
point: at the threshold that flags 0.5% of characters, what share of
errors does OCRcer catch?

**The default flag threshold is a cost choice, not a fit.**
- The fit makes confidence mean accuracy. Where to draw the review line
  depends on how much a checked character costs against a missed error,
  and only the user knows that.
- Proposal for when the curves are fitted: pdfcer exposes the threshold,
  and its default is the one that flags about 0.5% of characters on
  finfilings-val. That is ISRI's operating point, so it can be compared
  directly.
- The rescore "low confidence only" mode is different. Its threshold is
  fitted on CER (16b), not chosen by this rule.

**Layout errors as cost of correction (§5; Kanai et al., IEEE PAMI 1995).**
- A missed region costs its characters as insertions.
- An out-of-order block costs one move, converted to insertions by a
  factor. The cost is plotted over a range of factors, normalised by
  sample length.
- The census's layout bucket (missing lines, extra lines, a residual) is
  the first half of that. Counting block moves would split the residual
  into reading order and merges. Named as a census follow-up only if the
  residual turns out to be large.

**Also in the report, not adopted:** phrase accuracy, as a measure of
error bunching. OCRcer's word accuracy split into identifier and numeric
buckets already asks the question this domain cares about.

## Addendum 2026-09-25: how Tesseract settles doubtful spaces after recognition (read from source)

**Source.** `src/ccmain/fixspace.cpp` in tesseract-ocr/tesseract (main).
Read 2026-09-25: `fix_fuzzy_spaces`, `fix_fuzzy_space_list`,
`transform_to_next_perm`, `eval_word_spacing`, `fixspace_thinks_word_done`.
This fills in the mechanism that the chunk-9 backlog item "Fuzzy-space
decoder resolution" names only by reference.

**Which gaps are reconsidered.** Layout marks a gap near its threshold as
fuzzy (`W_FUZZY_SP` or `W_FUZZY_NON`). Only runs of words joined by fuzzy
gaps are reopened after recognition. Every other gap keeps its layout
decision.

**Search: close the smallest gaps first.**
- Start from layout's arrangement. Each step closes every gap of the
  current minimum width, then recognises and scores the run again. It
  stops when no gaps are left.
- So there is one arrangement per distinct gap width, not 2^n.
- A new arrangement replaces the best only on a **strict** improvement
  (`current_score > best_score`). On a tie, layout's decision stands.
- It stops early once every word is "done" (`PERFECT_WERDS`).

**Score: characters in accepted words, not path cost.**
- A word is "done" when it has no internal space, was accepted, and was
  read by a dictionary or number permuter (`SYSTEM`/`FREQ`/`USER_DAWG`,
  `NUMBER_PERM`). The score is the sum of the lengths of done words.
- **Credit is voided across a digit|`1` split.** If the previous word ends
  in `1` and this one starts with a digit, or the previous ends in a digit
  and this one starts with `1`, the previous word's credit is not counted.
  (For a word that is not done, "`1`" means any of `I`, `l`, `1`.) Splitting
  `112` into `1 12` earns nothing, even though both halves are valid numbers.
- **+1 for each adjacent `1` pair inside a word**, whether or not the word
  is accepted. This biases the choice toward joining `1`s.
- Counting accepted characters avoids comparing Viterbi costs across
  arrangements, which have different numbers of words and word-boundary
  terms.

**What maps onto OCRcer.**
- OCRcer's recorded failure is exactly the case this rule targets. `112.50`
  becomes `1 12 . 50` in monospace, because the wide side bearings of `1`
  and `.` make intra-word gaps look like spaces (`ARCHITECTURE.md` §11,
  2026-09-22 band-pooled entry). The shipped fixed-pitch rule removed
  some of these, not all.
- Lexicon credit is compatible with CLAUDE.md §6. The choice is between two
  spacings of the same ink, no character is rewritten, and a word not in
  the lexicon earns 0 either way. Because a tie keeps layout's decision, an
  out-of-lexicon run is never moved.
- In identifier-shaped context the lexicon credit must be suppressed, as it
  is in the decoder. That leaves only the number-shape credit and the
  digit|`1` rule.
- OCRcer has no "number permuter". The number-shape lexicon proposed in the
  chartype addendum above would play that role. The rule can also stand
  alone: a token is numeric-shaped when it is all digits and numeric
  punctuation.
- The fuzzy band would be a new `guess` parameter: gaps within a fraction of
  the space rule's valley, or inside the pitch rule's tolerance. Setting it
  to 0 must be byte-identical to the current build.

**Proposal, not yet sized.**
- The census (runbook step 8) should count **spurious spaces inside numeric
  truth tokens**, split by whether a neighbour of the space is `1`/`I`/`l`
  or `.`, and missing spaces between truth words, all on finfilings-train.
- If the `1`-adjacent share is large, the cheap first cut is a post-decode
  rejoin of adjacent numeric-shaped words across fuzzy gaps, scored with
  the digit|`1` rule and the joined-`1` credit. It needs no re-recognition.
- Full enumeration with lexicon credit comes only after that, if the
  residual justifies a decoder-stage change.
- Any fuzzy-band threshold is fitted on train and confirmed on val.

## Addendum 2026-09-25: a neural classifier on a segmentation lattice must learn to reject non-characters

**Source.** LeCun, Bottou, Bengio & Haffner, *Gradient-Based Learning
Applied to Document Recognition*, Proc. IEEE 86(11), 1998. Read 2026-09-25
from the gwern.net copy of the PDF: §I-D (the segmentation problem), §II-B
(RBF outputs and rejection), §III (Fig. 10, rejection), §V-A/B (Viterbi
training and its collapse), §VI (normalisation), §IX (the check reader).

**What the paper says.**
- A recognizer that scores the candidates of heuristic over-segmentation
  must give "low penalties for … correctly segmented characters, and high
  penalties for all categories for poorly formed characters". Training
  only on correct segments does not teach the second half.
- Class-posterior normalisation (softmax) "may eliminate information that
  is important for locally rejecting all the classes … when a piece of
  image does not correspond to a valid character class". A junk piece
  still gets p near 1 for *some* class.
- The paper's remedies:
  - train at the string level, discriminatively (GTN);
  - train the recognizer to reject non-characters directly. The deployed
    check reader "was also initially trained to reject noncharacters
    that resulted from segmentation errors";
  - use RBF output units, which fire only inside a bounded region.
- The hard part was getting the non-character examples. Hand-labelling
  segmenter output is "extremely tedious and costly" and inconsistent:
  "should the right half of a cut-up four be labeled as a one or as a
  noncharacter?"
- Rejection used "the difference between the scores of the top two
  classes". That is the same quantity as OCRcer's margin (§4.2).

**What it means for chunk 15.**
- As specified, the chunk-15 network scores every lattice candidate as
  `-log p`, and trains only on correct glyphs (rendered glyphs plus
  chunk 13's aligned crops). That is the setup the paper warns about.
- Prototype distance has this problem less. It is absolute, so a piece
  far from every prototype is expensive. It is not immune: half an `m`
  sits close to `n`.
- OCRcer can get labelled negatives cheaply, where the paper could not:
  - Rendered lines have exact glyph boxes. Every candidate that
    `ocrcer-core`'s own segmenter proposes on them, and that does not
    coincide with a truth box, is a negative, labelled automatically and
    consistently.
  - Chunk 13's forced alignment marks the true path through each aligned
    train word. The other candidates in that word are negatives.
- The paper's ambiguity does not go away. A geometric negative can be a
  real shape: half of `m` is `n`, and `rn` is `m`. How much this matters
  is measured, not assumed.
- Not adopted for v1: string-level discriminative training. It needs
  gradients through the decoder, and chunk 15 keeps the decoder
  unchanged.

The contract change is `ARCHITECTURE.md` §11, 2026-09-25, "Chunk 15
contract amended".

## Addendum 2026-09-25: the n-best list caps what LLM rescoring can gain — measure the ceiling at two sizes

**Source.** X. Liu, Y. Wang, X. Chen, M. J. F. Gales & P. C. Woodland,
*Efficient Lattice Rescoring Using Recurrent Neural Network Language
Models*, ICASSP 2014 (Cambridge; PDF from mi.eng.cam.ac.uk). Read
2026-09-25: abstract, §1, §3, Table 1.

**What it says.**
- Neural LMs carry the whole history, so they are "normally used to rescore
  N-best lists", and "this practical constraint limits the possible
  improvements".
- Lattice rescoring that merges paths sharing their last n−1 words
  "produced 1-best performance comparable with a 10k-best rescoring
  baseline", with over 70% smaller lattices.
- That is ASR, and the baseline was 10,000 candidates. Chunk 16b rescores
  **8 per line**.

**Why it matters here.** The LLM can only pick from the candidates it is
given (rule 6, by construction). If the correct reading of a line is not
among the 8, no λ or β recovers it.
- A line with k independent doubtful characters has 2^k readings. At k ≥ 4,
  8 candidates cannot cover them. How an n-best list spreads its slots
  over the doubtful positions is not measured here.
- The `nbest` branch's pending check is already "oracle best-of-8 CER, on
  finfilings-val" (RESUME item 3). That gives the ceiling, but not whether
  8 is too small.

**Proposal: measure before fitting λ and β.**
- On finfilings-train (design data, not val), take the lines that
  `LowConfidence` would select at a few candidate thresholds. Report CER
  for top-1, best-of-8 and best-of-32.
  - A small top-1 → best-of-8 gap: rescoring has little to gain, and 16b's
    fallback (a small correction model) moves up.
  - A large best-of-8 → best-of-32 gap: the cap, not the scorer, is the
    limit. The cheap lever is a larger N, since the prefix is scored once
    and candidates are batched. Lattice rescoring with history merging is
    the heavier lever, and is named only if a larger N costs too much time.
- Best-of-8 on val stays as RESUME specifies, as the confirmation.
- Also report the share of selected lines whose best-of-32 candidate
  changes an identifier-shaped word. It should be 0 by construction; a
  non-zero share is a bug.

## Addendum 2026-09-25: columns that add up ("footing") — prior art, and how often train pages have them

**Source.** US 5,872,730 (IBM; Shevach & Zlotnick), *Computerized
correction of numeric data*. Filed 1996, priority October 1995, issued
February 1999, **expired 2003** (Google Patents record). Read 2026-09-25:
abstract and claims.

**What it does.** When a form's digits must satisfy an arithmetic
relation (addends and a total), it runs a Viterbi pass over the digit
columns. Each state is the running total so far. Each step is weighted by
the OCR's own per-digit likelihood. The pass returns the most likely
digits that satisfy the relation, and substitutes them.

US 5,625,721 (Matsushita) was also checked. It needs a checksum embedded
in the document, so it does not apply to financial statements.

**How it fits OCRcer.**
- *Evidence, then flagging.* A column whose top-1 digits foot is
  independent evidence that those digits are right. A column that does not
  foot marks its cells for review. Neither changes any text.
- *Selection, only under rule 6.* Picking another reading means choosing
  among OCRcer's own candidates for those cells. It never generates a
  digit. It is allowed only when the table structure is certain.
- *Risks* (why selection is not the first step):
  - rounding ("may not add due to rounding");
  - header cells such as years and note numbers;
  - negatives shown in parentheses;
  - subtotals nested inside totals;
  - totals carried to another page.

**Reading: finfilings-train truth only, text only, no OCR run.**
- **Scan:**
  - The i-th number from the right in each truth line was taken, for
    i < 4.
  - For every such cell with a magnitude of at least 10, the scan checked
    whether a run of 2–24 immediately preceding non-zero cells in the same
    position summed to it exactly.
  - Parentheses counted as negative, and thousands separators were
    stripped.
- **Result:** 126 exact foots on **13 of 427 pages (3%)**. The chance
  baseline, which drew totals from other pages, gave 12.
- **Coverage:** 491 of 83,537 numeric cells (**0.59%**) sit inside a
  footing run. Every sampled hit was on a statement-style page, where
  truth lines are single cells stacked vertically.
- **Limits:** the scan misses:
  - totals of subtotals;
  - cross-footing across rows;
  - runs broken by text;
  - totals on a later page.

  So it is a floor, not a ceiling.

**Verdict.**
- On this corpus footing reaches under 1% of numeric cells, so it is **not
  a priority for finfilings CER**.
- It belongs on the chunk-9 accounting backlog as a flag-first feature,
  default off:
  - a "does not foot" review flag;
  - a confidence boost for columns that do foot.
- Its value must be measured on a statement-heavy corpus before any build
  is specced, and no such licence-clean corpus is in hand.
- Candidate selection under rule 6 comes after flagging has been measured,
  if at all.
- The IBM method is expired prior art, so it is free to use.

## Addendum 2026-09-25: a line that only "descends" is often a capitals line — Tesseract checks it against the page (read from source, and a train reading)

**Source.** Tesseract `main`, `src/textord/makerow.cpp` and `makerow.h`:
`compute_row_descdrop`, `compute_xheight_from_modes`, `correct_row_xheight`,
`get_row_category`, `compute_block_xheight`. Read 2026-09-25.

**What Tesseract does.**
- **Where a descender counts.** A descender counts only if its drop is
  0.25–0.6 of the row's x-height (`textord_descx_ratio_min`, `_max`).
  The descender pile, plus the potential ascenders, must also reach 0.16 of
  the x-height pile (0.08 + 0.08).
- **How rows are categorised.** A row is `ROW_ASCENDERS_FOUND` when it has
  an ascender rise. It is `ROW_DESCENDERS_FOUND` when it has a descender
  drop but no rise, and `ROW_UNKNOWN` when it has neither.
- **Where the block values come from.** The block x-height is the median
  over ascender rows first, then descender rows, then the rest.
- **How a descender-only row is corrected.** `correct_row_xheight` checks a
  `ROW_DESCENDERS_FOUND` row whose x-height is within 10%
  (`textord_xheight_error_margin`) of either:
  - the block x-height; or
  - the block **cap height** (x-height + ascrise).

  Such a row takes the block's values. So "something hangs below the
  baseline" is not taken as proof that the band is lowercase when its
  height matches the page's capitals.
- **How the x mode is found.** `compute_xheight_from_modes` finds the
  x-height as the lower of a pair of modes whose ratio is 1.25–1.8. It need
  not be the tallest pile, so a line whose capitals dominate still finds
  its x-height band below them.

**OCRcer, from `measure` in `layout/lines.rs`.**
- **How the band is read.**
  - The x-height candidate is the width-weighted mode of the line's tops.
  - It looks only for a band 15% *above* that mode.
  - Failing that, if any body member hangs below the baseline by more than
    `lines.descender_fraction` of it, the mode is taken as the x-height and
    labelled `Observed`.
- **What that misses.**
  - A capitals-dominated line never looks below its mode.
  - `(`, `)` and `$` hang below the baseline.
  - `inherit_x_heights` skips `Observed` lines, and counts them in the
    page vote.

**Reading.**
- **How it was taken:**
  - finfilings-train, every 10th page (43 pages), layout only;
  - `ocr --layout --no-decode`, the campaign's binary, default params.
- **How the descender branch was identified:** it is the `Observed` line
  whose cap height equals x-height / `x_height_per_cap`. That value was
  inferred from the `FromCapHeight` lines as 0.7431.
- **Result on all 43 pages:** 905 of 6,141 lines took the descender
  branch. They hold **13.9% of all components**, and 31 of the 43 pages
  have at least one.
- **Pages with a cap-band line** (24 pages; reference = the width-weighted
  median x-height of those lines):
  - 197 descender-branch lines.
  - 186 of them sit at 1.25–1.6 times the reference, peaking at 1.3.
    That is the page cap height (1 / 0.7431 = 1.346).
  - Only 7 sit near 1.0.
- **Pages with no cap-band line** (19 pages; reference = the cap height of
  their `FromCapHeight` lines):
  - The descender-branch lines sit at that cap height.
  - 15 of these pages are trade tables. Each date cell (`23/12/2024`)
    takes the branch because the `/` hangs below the baseline. The time
    and quantity cells in the same row read correctly as
    `FromCapHeight`.
- **Checked by eye:**
  - `filing__s4__r000782`: the line is
    `COMMON STOCKS - 44.0% - (continued)`. The page x-height is 9 px and
    the line is read as 12 px. The bold capitals are the width mode, and
    the parentheses hang below.
  - `filing__s1__r000045`: the date cells are read with an x-height of
    16 px, against 11.89 for the other cells in the same row.
- **A reference built from cap-band lines only** would re-read 185 of the
  905 lines. A reference that also uses the `FromCapHeight` lines' cap
  heights would re-read 893 (13.7% of components). That is the reason
  for the 2026-09-25 amendment in §11.
- **Not observed:** which component tripped the test on each line, and the
  CER effect. The per-line truth shown by `--layout` is by index, so it is
  not aligned.

The decision is recorded in `ARCHITECTURE.md` §11, 2026-09-25 ("A one-band
line that only descends is checked against the page's cap height").

## Addendum 2026-09-25: a lone digit in a number column — using the column's type (a CAD paper, and a train reading)

**Source.** Van Daele, Decleyre, Dubois & Meert, *An Automated Engineering
Assistant: Learning Parsers for Technical Drawings*, arXiv 1909.08552
(KU Leuven with Saint-Gobain Seals), §4.5.2. Read 2026-09-25 from the PDF.

**What it does.**
- The cells of a bill-of-materials quantity column are expected to be
  numbers. That expectation is a prior over characters (for example
  numeric 0.8, alphabetic 0.1, special 0.1).
- The prior is multiplied into the OCR engine's distribution for the cell
  and renormalised ("virtual evidence", Bayes conditioning).
- Their worked cell: Tesseract 4.0 gave `]` 0.630 and `1` 0.130. After the
  prior (`1` 0.615), `1` wins at 0.544.
- The paper's reason is the one that applies here. A quantity cell is a
  single character with no neighbours, so neither a dictionary nor a
  neighbour rule can help.
- Evidence: one illustrated cell. The paper reports **no accuracy
  measurement** for this step.

**What OCRcer already has, and the gap.**
- The 11 `digit_neighbour` confusion rules need a digit next to the
  doubtful glyph. The char-type addendum above adds the word-majority
  type, which needs a word.
- A cell that is **one glyph long** has neither. The only evidence left
  is its column: the cells above and below it, which layout already
  groups.

**Reading: finfilings-train truth only, text only, no OCR run.**
- **Method:**
  - For each truth line, take the i-th whitespace token from the right,
    for i < 4.
  - A *numeric run* is 3 or more consecutive lines where that token is a
    number (digits, `, . ( ) $ % -`).
- **Result:**
  - 2,306 runs on 98 of 427 pages, holding 8,628 numeric cells.
  - **4,095 of those cells (47%) are a single digit**: `0` 2,979, `1` 572,
    and the rest are scattered.
  - They sit on 57 pages. The top 10 pages hold about a third of them
    (13F holdings tables and the trade tables).
- **Limits:**
  - Truth has no geometry. On pages where each cell is its own truth line
    (the 13F tables), a "run" follows reading order, so it can be a row
    rather than a column.
  - The count is the *population* a column prior could reach, not errors.
    Whether OCRcer misreads these cells (`0`→`O`/`o`, `1`→`l`/`I`/`|`) is
    unmeasured. That is the census's job.
  - The 354 non-numbers that interrupt a run are almost all whole text
    lines (row labels, issuer names, dates), not letters inside a numeric
    column. So truth text cannot price the risk. That needs geometry.

**How it would fit OCRcer, if built.**
- **Form:** a type prior on single-glyph and short cells whose column
  neighbours (from layout, not from truth) are overwhelmingly numeric. It
  would add a digit-class bonus into the decoder score for that cell only.
- **It is a bonus under rule 6, never a constraint.** It never removes a
  letter candidate and never generates a character. A clear letter match
  still wins.
- **CAD support, with one assumption flagged.** A search result describing
  the May 2014 ASME Y14.35 draft says revision letters omit I, O, Q, S, X
  and Z, because they read as 1, 0, 5 and 2. The published standard has
  not been read. If it holds, a numeric prior leaking into a revision
  column cannot damage a valid revision letter in those four confusions.
- **Where it must not fire:**
  - cells in identifier-shaped columns (part numbers, CUSIPs such as
    `64110L106`);
  - any cell the identifier test covers.
- **Gate:** the whole-train census must show that isolated single-glyph
  numeric cells are misread at a material rate. Then it is specced as a
  chunk-9 decoder feature, default off, gated by:
  - the identifier test;
  - CAD dev pages with a quantity column;
  - a train stride run.

**Decision rule, not a decision.** Add a census bucket: *errors in cells
of one or two glyphs whose layout column is numeric*. Nothing is specced
until that bucket has a number.

## Addendum 2026-09-25: raised and lowered characters at word edges — Tesseract's re-read, and why it waits (read from source, and a train count)

**Source.** `src/ccmain/superscript.cpp` and the parameter block in
`tesseractclass.cpp`, tesseract-ocr/tesseract `main`. Read 2026-09-25.

**What Tesseract does (`SubAndSuperscriptFix`).**
- **Detect.** A blob at the start or end of a word is a candidate when
  both of these hold:
  - it sits out of position: its bottom is at least
    `superscript_min_y_bottom` (0.3) x-heights above the baseline, or its
    top is at most `subscript_max_y_top` (0.5) x-heights above it;
  - it matched badly: its certainty is at most `superscript_worse_certainty`
    (2.0) times the word's average certainty.
- **Re-read.** The word is split into prefix, core and suffix. The edge
  pieces are classified again with the y-position penalties switched off:
  `classify_class_pruner_multiplier` and
  `classify_integer_matcher_multiplier` are set to 0.
- **Accept.** The re-read is kept only if all of these hold for every
  character:
  - it is not punctuation;
  - it is not italic;
  - it is at least `superscript_scaledown_ratio` (0.4) of the line's font
    size;
  - its certainty beats `superscript_bettered_certainty` (0.97) times the
    old certainty.

  The characters are then tagged superscript or subscript.

**Why it would matter to OCRcer.** Features 103–107 measure a glyph against
the line's baseline and x-height. A raised footnote `1` therefore looks
nothing like any bank `1` in those dimensions. It may match `'` or `°`
instead. The equivalent here would be to re-match edge glyphs that are out
of position with those dimensions masked.

**Reading: finfilings-train truth, text only, no OCR run.** Out of 161,146
tokens:
- 7 are a lowercase word with 1–2 digits attached (`reserves1`,
  `Distributions1`, `slide13`);
- 76 are `†` (already a `digit_neighbour` source);
- the 43 `word(x)` tokens are all `(s)` plurals such as `Document(s)`,
  not markers.

Truth may drop or detach some markers, so this is a floor. Even so, raised
markers are not a visible share of this corpus.

**Verdict: recorded, not queued.**
- The census's substitution table would show it if it matters: digits
  read as `'`, `°` or `"` at word ends.
- For drawings, stacked tolerances and `mm²` are the cases. The 13c
  drawing census counts only rotated strings today. Adding a count of
  raised or stacked text to it is the proposed way to price this, and is
  not yet in any spec.

## Addendum 2026-09-25: words broken across lines by a hyphen — Tesseract carries the dictionary state over (read from source, and a train count)

**Source.** `src/dict/hyphen.cpp` and `dict.h` in tesseract-ocr/tesseract
`main`. Read 2026-09-25.

**What Tesseract does.**
- When the last word on a line ends in a hyphen, `set_hyphen_word` stores
  that word without the hyphen, plus its live dictionary positions
  (`hyphen_active_dawgs_`).
- The first word of the next line resumes the dictionary walk from those
  positions, so `invest-` + `ment` is scored as `investment`.
- `reset_hyphen_vars` drops the stored state unless the pair really is
  last-on-line followed by first-on-line.

**Reading: finfilings-train truth, text only, no OCR run.**
- 64,132 truth lines; 18 end in `letters-` with a lowercase word opening
  the next line, on 17 pages.
- **All 18 are real compound hyphens**, not syllable breaks:
  `employer-sponsored`, `wholly-owned`, `broker-dealer`, `three-month`,
  `attorney-client`.
- None of the joined forms (hyphen removed) is in the lexicon, and 15
  have a fragment that is.

**Verdict: not queued.**
- This corpus does not break words at syllables, so there is nothing for
  a carry-over to recover.
- Removing the hyphen would be wrong for every observed case.
- If a scanned-book or justified-prose corpus is ever added, the design
  is Tesseract's: carry the lexicon position over the line end and keep
  the hyphen in the output.
- It is a bonus under rule 6 either way, so the cost of not having it is
  a missed bonus, never a rewrite.

## Addendum 2026-09-25: how much a longer character context would help — a train-only reading

**Question.** The decoder's language term is a character bigram, and chunk
14 counts it from train text. Would a trigram, with the decoder's beam
carrying the last two characters, add enough to be worth a table?
No source was read. This is a measurement.

**Method.** Held-out, finfilings-train truth only, text only, no OCR run.
- **Split.** Train was split into two halves by blocks of 100 consecutive
  record numbers within each shard. That gives 194 and 233 pages, and
  75,368 and 85,778 tokens. Adjacent pages of one filing fall in the same
  half. An odd/even page split was within 0.1 bits/char for bigram and
  trigram and within 0.15 points on the preference test. Its 4-gram was up
  to 0.27 bits/char better, which is boilerplate leaking between adjacent
  pages, so the block split is the one reported.
- **Model.** Fitted on one half, measured on the other, then swapped and
  averaged. It interpolates absolute discounting with a computed D per
  order, the same estimator as chunk 14, and puts boundary marks on each
  token.
- **Look-alike test.** For every occurrence of a character in 15
  look-alike pairs (`0/O 1/l 1/I 5/S 8/B 2/Z 6/G e/c n/h i/l u/n m/n a/o
  t/f r/t`), score the true token against the token with that one
  character swapped for its twin. Record whether context alone prefers
  the truth, with the visual evidence taken as equal.

**Result: held-out bits per character.**

| Tokens | unigram | bigram | trigram | 4-gram |
|---|---|---|---|---|
| all | 5.225 | 3.800 | 3.060 | 2.694 |
| with a digit | 3.934 | 3.408 | 2.905 | 2.759 |
| letters only | 4.657 | 3.550 | 2.798 | 2.278 |

**Result: context prefers the true character over its look-alike.**

| Tokens | sites | unigram | bigram | trigram | 4-gram |
|---|---|---|---|---|---|
| with a digit | 240,872 | 99.48% | 99.42% | 99.63% | 99.60% |
| letters only | 435,973 | 63.36% | 89.51% | 96.98% | 99.10% |
| letters only, not in the base lexicon | 189,152 | 59.44% | 88.78% | 96.18% | 98.52% |

**Reading.**
- In tokens with a digit, a longer context adds nothing. The bigram, and
  the `digit_neighbour` rules, already carry what there is.
- In letter tokens the lexicon does not cover, a trigram cuts the rate at
  which context prefers the wrong look-alike from 11.2% to 3.8%, about
  two thirds. A 4-gram gets it to 1.5%.
- Those are the words where the lexicon gives no help at all: names,
  terms, and words outside the authored list. So this is the one place
  where the language model is the only contextual evidence.
- "Base lexicon" means the authored base forms, not the expanded
  inflections, so the out-of-lexicon share is an overestimate.

**What it is not.**
- It is not an error rate. It says how often context would prefer the
  truth if the shapes tied. It does not say how often OCRcer's shapes
  tie, or how often the decoder errs today. That is the census's job.
- It is finance text scored against finance text. On drawings a
  finance-trained trigram carries the same leak risk that chunk 14's μ
  and CAD dev set exist to check.
- The 15 pairs are chosen by hand as common look-alikes, not measured
  from OCRcer's confusions. The census's substitution table would supply
  measured pairs.

**Candidate (chunk 14b), decision rule, not a decision.**
- *Trigger:* the census shows letter substitutions inside out-of-lexicon
  letter words as a material bucket.
- *Build, if triggered:* chunk 14's machinery at order 3, fitted on train
  and mixed with μ times the authored evidence. It goes in a new
  **optional** table: an old runtime ignores it and a new runtime without
  it falls back to the bigram. That is additive, as `prototype_face` was,
  so there is no `.ocrw` version bump, provided §7's unknown-table rule
  still holds when it is specified. The decoder's beam carries two
  characters of history.
- *Gates:*
  - chunk 14's gates;
  - the CAD dev set not worse;
  - the identifier test;
  - the table size and the decode wall time, both reported.


---

## Addendum 2026-09-25: white text on dark bars — Tesseract re-reads unsure lines inverted (read from source, and a train count)

**Tesseract.** `invert_threshold` defaults to 0.7: "For lines with a mean
confidence below this value, OCR is also tried with an inverted image"
(`tesseractclass.cpp`). In `LSTMRecognizer::RecognizeLine`
(`lstmrecognizer.cpp`), the line is run once, the mean output confidence is
taken, and if it falls below the threshold the line image is inverted and run
again. The inverted result is kept only if its mean confidence is higher.
The pattern is the general one: a confidence gate, an alternative image, and
keep the better-scoring read.

**OCRcer today.** Polarity is decided once per page
(`binarize.rs`, `auto_polarity`: a mask covering more than half the page is
inverted). A dark bar with white text on an otherwise dark-on-light page is
therefore one large ink component, and the letters are holes in it.

**Train count (finfilings-train only, 427 pages, a numpy/scipy script, not
the engine).** Method:
- global Otsu threshold;
- dark 8-connected components with height ≥ 1.2× the page's median glyph
  height, width ≥ 4×, and fill ≥ 0.5 of the bounding box;
- holes of glyph size (0.5–2× median height, width ≤ 3×);
- a region counts with ≥ 3 such holes.

Result:
- 3 pages, 4 regions, 127 glyph-sized holes;
- against 836,934 dark glyph-sized components on all 427 pages, about 0.015%;
- all three pages viewed, and all are genuine header bars:
  - "Share Classes A C I R3 R4 R5 R6 Y F" (black);
  - "COMMON STOCKS - 96.7%" and its "(continued)" (dark grey);
  - "Analysis of Profit and Loss account items" (dark grey).

The ground truth carries all of it.

**What the engine does with one (a layout-only run, `ocr --layout
--no-decode`, master's binary, on `filing__s4__r000750`).**
- In the bar's band the layout reports 11 one-member lines 3–5 px wide:
  the dark counters inside the white letters.
- No line carries the bar's text.
- So the text is lost, and the fragments may add insertions. Decode was not
  run, so the insertions are not measured.

**For OCRcer, not queued.** On this corpus the ceiling is about 0.015% of
characters. The mechanism, if a trigger fires:
- a dense dark component whose holes are glyph-sized is a reverse region;
- read its bounding box inverted, as its own block;
- drop the dark islands inside its holes.

Both thresholds would be authored guesses and go on the tuning list.
Triggers:
- reverse bars in the CAD dev set or in a pdfcer report (title blocks are
  the place to look);
- or the census showing counter-fragment insertions.

**The general form, parked.** Lund, Kennard & Ringger (SPIE DRR XX, 2013)
read one page at several global thresholds, aligned the outputs into a word
lattice, and committed one word per slot. On 19th-century newspapers their
baseline WER was 13.8% and higher, and the committed WER was 8.41%. The
lattice oracle was 7.6% with two thresholds and 6.8% with five. These are
figures from the abstract, via search results. The committing method was not
read. The OCRcer version would re-read low-confidence words at a few Sauvola
`k` values and keep the best by calibrated word confidence. It waits on:
1. the conf-margin merge, because a calibrated confidence is needed to choose
   with;
2. the Sauvola-window measurement above, which is the cheaper fix for the
   same broken strokes;
3. a train oracle reading: of words wrong at the default threshold, how many
   come out right under any of K variants.

Sources:
- [tesseract `tesseractclass.cpp`](https://github.com/tesseract-ocr/tesseract/blob/main/src/ccmain/tesseractclass.cpp)
- [tesseract `lstmrecognizer.cpp`](https://github.com/tesseract-ocr/tesseract/blob/main/src/lstm/lstmrecognizer.cpp)
- [Lund, Kennard & Ringger, Combining multiple thresholding binarization values to improve OCR output (SPIE 8658)](https://www.spiedigitallibrary.org/conference-proceedings-of-spie/8658/1/Combining-multiple-thresholding-binarization-values-to-improve-OCR-output/10.1117/12.2006228.short)
- [Lund, Kennard & Ringger, Why multiple document image binarizations improve OCR (HIP 2013)](https://www.researchgate.net/publication/255482649_Why_Multiple_Document_Image_Binarizations_Improve_OCR)

## Addendum 2026-09-25: how strongly Tesseract prefers a dictionary word (read from source)

**Why this was looked up.** In the 12b cost-knob run, cutting the matcher's
shortlist from 5 candidates to 3 *lowered* train CER: 21.010 → 20.963 at
stride 6. That is a reading from `fitlogs`. One explanation is that the
decoder's language terms override the matcher too readily when given
rank-4 and rank-5 candidates. The lexicon term is one of those terms.

**Tesseract, read from `src/dict/dict.cpp` (main branch).**
`Dict::adjust_word` multiplies a word's whole rating by a factor (lower is
better):

| Parameter | Default | Applies to |
|---|---|---|
| `segment_penalty_dict_frequent_word` | 1.0 | a frequent dictionary word with good case |
| `segment_penalty_dict_case_ok` | 1.1 | a dictionary word with good case |
| `segment_penalty_dict_nonword` | 1.25 | not a dictionary word |
| `segment_penalty_dict_case_bad` | 1.3125 | a dictionary word that may have case issues |
| `segment_penalty_garbage` | 1.50 | not in the dictionary, and looks like garbage |

Separately, `stopper_smallword_size = 2`: "Size of dict word to be treated
as non-dict word". The stopper does not trust a dictionary match of two
characters or fewer.

**What that means.** The dictionary preference is *proportional to the
word's own evidence*. A good-case dictionary word beats a non-word reading
only if its rating is below 1.25 / 1.1 ≈ 1.14 times the non-word's. The
preference is worth about 12% of the word's total rating (the rating is
match distance × outline length). It therefore grows with word length and
with how badly both readings matched. A two-letter word cannot collect a
large absolute preference. The stopper additionally refuses to trust it.

**OCRcer, read from `decode/viterbi.rs`.** The lexicon term is additive and
fixed per word: `w_lex × lex_bonus[tier]`, which is 0.6 × (1.0 … 0.4) in the
campaign vector. It is added once at the word end, whatever the length.
- Relative to the word's evidence, it is strongest on the shortest words.
  That is where a lexicon match is least informative, and where a word
  competes with digits and symbols in lone cells. CLAUDE.md rule 6's
  expensive failure lives there.
- Under the 12c `width_weighting` mode, the match, bigram and confusion
  terms scale with width and the lexicon term does not. Its relative weight
  would then change again with word length.

Rule 6 is kept in both designs. Tesseract's factor is a penalty on
non-words only in the sense that the argmax sees a ratio. OCRcer's bonus is
never subtracted.

**Not queued. What would decide it.** A census bucket on the train
char-dump:
- among words the engine output *as a lexicon word* whose truth differs,
  count them by output length (1–2, 3–4, 5+ letters);
- split by whether the truth word was itself in the lexicon.

If the errors concentrate at 1–2 letters, two candidates follow:
- a length-scaled bonus (per letter, or proportional to the word's match
  term, as Tesseract does);
- a floor like `stopper_smallword_size`.

Each would be behind a switch, measured on train and confirmed on val.

## Addendum 2026-09-25: OCR-B, OCR-A and MICR — machine-reading faces an accounting office meets (licence check only)

**Where they appear.** OCR-B is "used for machine-readable passports" and
"widely used for the human-readable digits in UPC/EAN barcodes"; it follows
ISO 1073-2 ([Wikipedia, OCR-B](https://en.wikipedia.org/wiki/OCR-B)). It is
also the usual face of the scan line on payment slips and remittance stubs.
OCR-A prints ISBNs and some older remittance lines. MICR E-13B prints the
routing and account line along the foot of a cheque.

**Licences, as checked.**

- OCR-B: Matthew Skala's font (Tsukurimashou project). Skala's files are
  public domain. The Metafont sources they build on are Norbert Schwarz's,
  under "You may freely use, modify and/or distribute this file, without
  limitation" (Ubuntu `fonts-ocr-b` copyright file,
  [launchpad](https://launchpad.net/ubuntu/focal/+source/fonts-ocr-b/+copyright)).
  Clean under `CLAUDE.md` rule 2, with no attribution requirement.
- OCR-A: the upstream font files (John Sauter) are public domain. The GPL-3
  line on that package covers its Debian packaging, not the font
  ([launchpad](https://launchpad.net/ubuntu/focal/+source/fonts-ocr-a/+copyright)).
  Clean.
- MICR E-13B: no licence-clean face has been identified. The widely packaged
  free one, GnuMICR, is GPL, and copyleft stays out.

**What each would cost.** OCR-B and OCR-A draw only characters the charset
already has, including `<` (U+003C), the MRZ filler. Adding either is a bank
coverage change for `ocrcer-glyphs`: one `fonts.tsv` row and a bank rebuild.
There is no format change and no charset change. MICR is different. Its four
control symbols (transit, amount, on-us, dash; U+2446–U+2449) are outside the
charset. Adding them is a charset change under section 2's protocol, and
there is no clean face to render them from.

**Train reading.** None taken. No corpus is known to contain an MRZ, a
cheque or a payment-slip scan line; no corpus has been searched page by
page for one.

**Decision.** Not queued. OCR-B's trigger is a pdfcer report, or a dev-set
page with an MRZ or a scan line that is misread; the fix is then one font
row. OCR-A follows the same trigger. MICR stays out of v1 unless the
operator asks: it needs a charset change and has no clean face.

## Addendum 2026-09-25: reading boxed slips — anchor on the printed box number, not on a layout (prior art, and the CRA rule that makes templates brittle)

For `PLAN.md` section 2a item 3 (boxed forms, the box-number-to-value
mapping).

**The classical method is a form library.** Casey and Ferguson's
Intelligent Forms Processing (IBM Systems Journal 29(3), 435–450, 1990), and
Casey, Ferguson, Mohiuddin and Walach (Machine Vision and Applications,
1992) work in four steps:

1. analyse a blank form once, to build a model of each form type;
2. recognise an incoming form by matching its pattern of ruled lines against
   the library;
3. register the page to the model and extract each field at its model
   position;
4. run "forms dropout", separating the preprinted form from the filled-in
   data.

This works when every copy of a form type shares one layout.

**CRA slips do not share one layout.** Information Circular IC97-2R20,
Customized Forms (2023-10-27), lets issuers produce their own T3, T4, T4A
and T5 slips:

- they "must include all identification areas, as well as income tax and
  code boxes";
- "Except for those required fields, you can choose to include only the
  boxes that meet the recipient's circumstances";
- paper-filed slips "keep the boxes in the same numerical order as the boxes
  on the CRA slip".

So one slip type arrives in as many layouts as there are payroll vendors,
with boxes left out. What stays fixed is the printed box number and the
order of the boxes, not where they sit. A template per slip type would miss
every customised slip, and a template per issuer does not scale.

**Consequences for chunk 9's design.** These are inputs to its spec, not
decisions.

1. **Primary mechanism: no template.** Find the cells from the rulings,
   using the same horizontal and vertical run machinery as the checkbox
   border signal and ruling-pixel removal. Inside each cell, separate the
   label from the value:
   - the label is the box number plus its caption, in smaller type near the
     cell's top-left corner;
   - the value is the rest.

   The box number names the value. Box order is a free consistency check:
   box numbers read in reading order must not decrease on a paper-filed
   slip. A slip that breaks the order is flagged, never reordered.
2. **Box numbers are not all digits.** The T4 has boxes 16A and 17A
   (`C:/tax_rag/rag/form__t4_slip.md`), and the T4A numbers its boxes with
   three digits, 014 to 211 (`form__t4a_slip.md`). A box-number token is two
   or three digits with an optional capital letter, and a leading zero is
   part of the key. The letter must survive the
   numeric-context rules (char-type consistency, the number DAWG
   addendum). Otherwise `16A` is "corrected" into a number, which is
   exactly the rule 6 failure.
3. **Some box numbers are data, not labels.** The T4's "Other information"
   area is twelve empty Box/Amount pairs that the issuer fills in. There, the
   box number is a read value, identifier-shaped, and paired with the amount
   beside it. It is not a printed label.
4. **Identifier fields.** SIN (box 12) and the employer's account number
   (box 54) are identifier-shaped. The lexicon is suppressed there under
   rule 6's identifier-context rule.
5. **Plain-paper slips.** A customised slip may have no ruled cells at all.
   The fallback pairs each box-number label with the nearest value to its
   right or below, and it runs on the same label/value size split.
6. **Fixtures.** The exit gate's boxed-form fixture should be a slip laid
   out by us, carrying CRA box numbers (which are facts) in our own layout,
   plus variants that exercise IC97-2's latitude:
   - boxes omitted;
   - boxes moved but kept in numerical order;
   - no rulings.

   Rendering on CRA's own blank PDFs would put Crown-copyright page
   designs into a public MIT repository. That is an operator question, and
   it does not need asking if the fixtures are our own layouts.

**Not measured.** No boxed-form page exists in any corpus or dev set yet,
so none of this has a reading.

## Addendum 2026-09-25: how many candidates reach the decoder — Tesseract cuts by distance, not by count (read from source, and a train reading)

**What OCRcer does.** `match.top_k` (5, a guess) is a fixed count: every
glyph hypothesis hands its five best classes to the lattice, and the decoder
may pick any of them when the bigram, lexicon or confusion terms outweigh the
distance gap. A clean glyph and a doubtful one get the same five.

**Train reading (campaign cost tier; finfilings-train, stride 6; tier-4
vector otherwise unchanged).** Machine contended, so the wall times are not a
speed reading.

| `match.top_k` | CER | line-matched CER | F1 |
|---|---|---|---|
| 3 | 20.963% | 23.209 | 70.786% |
| 5 | 21.010% | 23.211 | 70.797% |
| 8 | 21.170% | 23.44 | 70.558% |

Error rises with every candidate added: CER 20.963 -> 21.010 -> 21.170, and
line-matched CER moves the same way. The campaign's cost rule takes the
cheapest value within tolerance of the k=5 baseline, so the beam-width runs
that follow use `top_k` 3. That is a reading, not a decision: extra
candidates are, on balance, chosen wrongly more often than rightly. A
distance cut targets exactly that, but override weights that are too strong
would produce the same trend; census bucket (m) below separates the two.
`top_k` 3 also shortens the n-best list that LLM rescoring works from, so
the final value is settled with the n-best ceiling figures in hand, not by
this table alone.

**What Tesseract does** (`src/classify/adaptmatch.cpp`, `classify.cpp`, read
from source on `main`). `RemoveBadMatches` runs on both classifier paths —
after `DoAdaptiveMatch` in `AdaptiveClassifier`, and after
`CharNormClassifier` in `GetAmbiguities` — with no condition. It keeps a
class only if `rating >= best_rating - matcher_bad_match_pad`, where ratings
are 0-1 (1 is perfect) and `matcher_bad_match_pad` is 0.15 ("Bad Match Pad
(0-1)"). `MAX_MATCHES` (10) is only the cap. So the shortlist length depends
on the glyph: a clean glyph passes one or two classes, a doubtful one passes
many. In `classify_bln_numeric_mode` (off by default) the same function also
drops alphabetic classes other than the roman-numeral letters, and turns `l`
into `1` and `O` into `0` when the digit itself fell below the threshold.

**The difference that matters here.** A fixed count lets a distant fourth or
fifth class into the lattice on a clean glyph, where the language terms can
then overturn a confident match. It also shuts out a close sixth on a
doubtful glyph. A distance cut does neither.

**Candidate (not specified, not built): `match.cand_pad`.** Keep candidate
`c` only if `d_c <= d1 * (1 + pad)`. It is a ratio rather than Tesseract's
absolute pad because OCRcer's distances are weighted L2 in standardised
space, not 0-1 similarities, so a fixed absolute pad would mean different
things at different `d1`. `top_k` stays as the cap, in the role of
`MAX_MATCHES`. Default off (infinite pad), so the default path is
byte-identical. Runtime-only, one `params` row, labelled a guess.

**Gate.** Build it only if census bucket (m) — override outcome by matcher
rank and by `d_c/d1` — shows breaks outnumbering fixes, concentrated at large
`d_c/d1`. If breaks are spread evenly across the ratio buckets, the problem
is the override weights (`w_bigram`, `w_lex`, confusion), and a distance cut
would not fix it. A tighter shortlist also lowers the n-best ceiling that
LLM rescoring can reach (see the n-best ceiling addendum above), so the (m)
table should be read next to that ceiling, not alone.

**Not measured:** `cand_pad` at any value; per-rank override outcomes; any
wall-clock effect of `top_k`.

## Addendum 2026-09-25: fitting the matcher's feature weights (metric learning for nearest neighbour) — prior art, and why it waits for chunk 13

**What OCRcer has.** Seven block weights in `model/feature_weights.tsv`, all
1.0 except the four baseline-relative geometry dimensions at 6.0. That 6.0 is
the argmax of a three-point sweep {1, 6, 12} on one block, on synthetic
pages, with section 4.1's per-confusion-pair bar applied (12 was rejected
because it takes `0`/`o` from 7 to 20). No other block has ever been swept.
Fitted values are allowed since the operator's 2026-09-24 decision, so these
weights no longer have to be hand-set.

**Prior art.** Two standard methods fit the distance for nearest-neighbour
classification directly:
- Neighbourhood Components Analysis (Goldberger, Roweis, Hinton and
  Salakhutdinov, NIPS 2004) maximises a smooth (stochastic) version of
  leave-one-out kNN accuracy on the training set.
- Large Margin Nearest Neighbour (Weinberger and Saul, JMLR 10, 2009) fixes
  each point's same-class "target neighbours" in advance, then learns a
  metric that pulls them in and pushes other classes out by a margin.
  Table 1 of that paper has one data set close to OCRcer's case: UCI
  letters, 26 classes and 16 hand-made features. kNN error goes from 4.68%
  with plain Euclidean distance to 3.60% with the learned metric, and to
  2.67% with its energy-based rule. On MNIST it goes from 2.12% to 1.72%.
  The paper reduces dimensions with PCA before fitting "to reduce
  computation time and avoid overfitting".

Both papers fit a full matrix. Neither reports a diagonal-only (per-weight)
fit, so no figure here says how much of the gain survives that restriction.

**What maps onto OCRcer without a format change.** A diagonal fit is exactly
the existing optional `feature_weights` table. The prototypes, the
normalisation constants, the charset and the file version are all
unchanged, so there is no bank rebuild. A full matrix would store the
prototypes in a transformed space, which *is* a feature-vector change and
takes the whole protocol (version bump, full rebuild, §11 entry). Only the
diagonal form is a candidate.

**Candidate (not specified): fitted block weights.** Fit the seven block
weights (six free; zone density stays the 1.0 reference), plus the four
named geometry dimensions, by an NCA-style leave-one-face-out objective.
Target neighbours are same-class prototypes from other faces, which is
where the style probe found the evidence lives. About ten numbers, so
overfitting is a small risk. Two constraints come from section 4.1 and are
not optional:
1. The objective is the errors the decoder cannot repair, not the raw error
   count. The identifier-critical groups are the ones in section 11's
   2026-09-22 tuning table (`0`/`O`, `0`/`o`, `O`/`o`, `1`/`l`, `5`/`S`,
   `8`/`B`), judged the way that entry judged them: it accepted `1`/`l`
   21 -> 22 at weight 6 and rejected `0`/`o` 7 -> 20 at weight 12.
2. The fitted set is reported with that per-pair table beside the aggregate.

**Why it waits.**
- **The distance scale feeds everything downstream.** The decoder scores
  `w_match * (bonus - distance)`, so reweighting shifts the balance that
  chunk 12b is fitting now. The calibration curve is fitted on `d1/d2`, and
  reweighting changes that ratio non-uniformly.
- **So a weight fit forces refits.** Changing the weights after 12b closes
  and after the calibration fit (runbook step 7) means redoing both.
- **The bank-condensing addendum also depends on them.** It requires
  selection by the runtime's own weighted distance.
- **Synthetic glyphs are the wrong training data.** Fitted on them, the
  weights would learn to separate clean renders from other clean renders,
  not scans. Chunk 13's forced-aligned glyph samples from finfilings-train
  are the right data.

So the candidate belongs after chunk 13 yields samples, followed by one
refit of the 12b vector and the calibration curve, not three.

**Pivot-index interaction:** exact pivot bounds hold under any fixed
non-negative diagonal weights, provided the pivot distances are computed
with the same weights. That holds automatically if pivots are built at load
from the file's own table.

**Not measured:** any fitted weight set; how much of the published
full-matrix gain survives a diagonal restriction; any per-pair effect.

## Addendum 2026-09-25: scoring table structure — adjacency relations, tree edit distance, and grid similarity (prior art, for chunk 9's benchmark)

**Why this is needed.** Chunk 9's exit gate (`PLAN.md` §2a) asserts golden
fixtures exactly: region tree, block types, reading order, and the
box-number or line-item-to-value mapping. That is right for fixtures, whose
structure is authored. It gives no number for a real page, where a table is
partly right. The benchmark needs a score that says how much of a table's
structure was recovered, separately from how well its text was read.

**The three metrics in use** (from Smock et al., "GriTS", arXiv 2203.12555,
which compares them):

- **Directed adjacency relations (DAR)**, ICDAR 2013 table competition
  (Göbel et al.). Each non-empty cell is paired with its nearest right and
  lower neighbour; precision and recall are counted over those pairs. The
  paper's criticism: it "mostly ignores the insertion of contiguous blank
  cells", and it scores local neighbourhoods, not the whole grid.
- **Tree edit distance similarity (TEDS)**, from PubTabNet. The table is
  scored as an HTML tree. Per the paper it is "sensitive to whether rows
  are selected or columns are selected": a table stored row by row makes a
  lost row and a lost column cost different amounts.
- **Grid table similarity (GriTS)**. Both tables are matrices of grid
  cells. The score is `2·Σ f(Ã_ij, B̃_ij) / (|A| + |B|)`, where `Ã` and
  `B̃` are the most similar two-dimensional substructures of the two grids
  (a subset of rows and a subset of columns from each). Recall divides by
  `|A|`, precision by `|B|`. Three versions differ only in the cell
  function `f`:
  - `GriTS_Top`: IoU of each cell's span in grid coordinates (structure
    only);
  - `GriTS_Con`: normalised longest common subsequence of cell text;
  - `GriTS_Loc`: IoU of cell boxes in pixels.

  A cell spanning several grid positions repeats its content at each one.
  The exact two-dimensional problem is NP-hard. The paper uses a factored
  heuristic, a nested dynamic programme of cost `O(|A|·|B|)`, which gives
  lower and upper bounds; it reports "little difference" between the bounds
  in practice.

**Why GriTS fits this domain.** Financial statements carry many blank
cells: note-reference columns, the unused year of a comparative pair,
subtotal rows with one filled column. DAR does not see blank cells inserted
or dropped. TEDS charges a dropped column differently from a dropped row,
and a comparative statement loses columns, not rows, when it goes wrong.

**What that suggests for chunk 9 (candidate, not specified).**

- **Fixtures** keep exact assertions, as the gate says.
- **Benchmark:** report `GriTS_Top` and `GriTS_Con` side by side.
  `GriTS_Top` isolates structure. `GriTS_Con` mixes structure with reading
  errors, and the gap between the two shows how much of a table's loss is
  OCR rather than layout.
- **Boxed forms:** score as field-level exact match, box number to value.
  That is a key-value task, not a grid.
- **Where it runs:** in `ocrcer-bench`, in f64, never in `ocrcer-core`.
  Only one implementation of it exists, so the "every stage is written
  once" rule is not engaged.

**A candidate scoring corpus, not cleared.** FinTabNet (IBM) has table
structure annotations for tables in S&P 500 earnings reports, 1999–2019.
Its licence is stated as CDLA-Permissive, and the FinTabNet.c re-release on
Hugging Face as CDLA-Permissive-2.0. Two questions go to the operator
before anything is downloaded:

1. the copyright of the underlying report pages, which neither README
   addresses;
2. whether its annotations fit a scoring-only role.

It would be scoring-only under rule 1's firewall. It is in domain in a way
the synthetic structure fixtures cannot be.

**Not measured:** any GriTS figure on any OCRcer output (no structure layer
exists); the gap between the heuristic's bounds on financial tables;
FinTabNet's page count or annotation format, which neither README states.

## Addendum 2026-09-25: the row hierarchy of a financial statement — ReMine's rectangle rules, and why they decide which cells should foot

**Source.** Chen, Chiticariu, Danilevsky, Evfimievski and Sen (IBM
Research), "A Rectangle Mining Method for Understanding the Semantics of
Financial Tables", ICDAR 2017. Read 2026-09-25 from the author's PDF.

**The task.** In a statement, a row's meaning comes from rows above it
that do not overlap it. "Restricted cash 94" sits under "Current", which
sits under "ASSETS". Recovering those parent rows is a tree over rows.

**The method (ReMine).** Each row gets a few features, and each feature
has an order for "can be a parent of":

- bold ≻ not bold;
- smaller indent ≻ larger indent;
- not blank ≻ blank;
- capitalised ≻ not capitalised;
- section header ≻ not a section header. A section header is a row with
  a label and empty data cells.
- A total row may never take children. It is detected by a label starting
  with "total".

Rows start as one-row rectangles. The algorithm then repeats two steps
until nothing changes:

1. merge adjacent rectangles whose features are equal;
2. attach a rectangle to the one above it when that one is strictly
   greater in the order.

It also uses two whole-rectangle rules:

- **Ended section.** A section that closes with its paired total row
  stops growing. The pair is found by the longest common subsequence
  between the header label and the total label, against a threshold.
- **Empty section.** A lone header row takes the rows below it as
  children.

**Their numbers.** These are theirs, leave-one-company-out, on 72 tables
from six companies' 2015 Q3 statements:

| Measure | ReMine | SVM pair classifier |
|---|---|---|
| Direct parent-child F1 | 84.11 | 68.42 |
| Transitive F1 | 87.90 | 60.89 |
| Transitive F1, ICDAR 2013, trained out of domain | 86.90 | — |

The global section rules are worth about 5 points of transitive F1.

Error analysis:

- The hand-written order is the main source of errors.
- Capitalisation hurts, because acronyms such as "EBITDA" trigger it.
  Transitive F1 rose to 88.94 without it.
- "Starts with total" misses "Net ..." totals.

**What an image engine has that their input did not.** Their footnote 1
says they ignore "lines and spaces, often unavailable in the extracted
table format", because they worked from PDF converted to HTML. OCRcer sees
the page, so it has three signals ReMine went without:

- the accounting rules above totals (single rule: subtotal; double rule:
  grand total), which `PLAN.md` §2a already lists;
- real indent in pixels, not an HTML approximation;
- stroke weight, which the page shows directly. No bold field is exposed
  today; that is a gap for chunk 9.

**Why this matters for footing.** The footing addendum above found exact
foots on 3% of train pages. It listed "totals of subtotals" as something
its flat scan misses. A row tree answers exactly that: a total row's
addends are its section's children, and the subtotals are those children's
own total rows. So the tree decides which cells should foot. Footing then
works in two ways:

- as a check on the tree: a section whose children sum to its total
  confirms the section boundary;
- as the review flag already recommended (flag, never alter a digit),
  when they do not.

**What that suggests for chunk 9 (candidate, not specified).**

- Build the row tree with ReMine's two-step merge and attach, using an
  authored partial order: bold, indent, blank data, section header, rule
  above.
- Drop capitalisation, per their ablation.
- Detect total rows from:
  - a rule above the number cells, first;
  - a label keyword second: "total" and "net", plus "sous-total" for
    French-language Canadian filings.
- Every order and threshold is a labelled guess until it is fitted on a
  train split.
- Score the tree with their transitive F1 on authored statement fixtures.
  The fixtures assert the tree exactly, as the gate requires.

**Their dataset is not usable yet.** The 72 labelled tables are offered
as a zip on the author's page. No licence is stated there, so it goes to
the operator before any use, and even then it would be scoring-only.

**Not measured:** how many finfilings-train statement pages carry indent,
rules or bold that separate the levels; any tree accuracy on OCRcer
output, since no table layer exists.

## Addendum 2026-09-25: cells from ruling lines — how Camelot and Tabula build them (read from source, for chunk 9a)

**Why.** The chunk 9 spec, part 1 (`ARCHITECTURE.md` §11, 2026-09-25),
defines a cell as a minimal rectangle closed by rules. Two widely used PDF
table extractors already build cells from rulings. They were read before
anyone implements 9a, so the spec either matches prior art or says where it
departs.

**Camelot, lattice mode** (`camelot/parsers/lattice.py`,
`camelot/image_processing.py`, read 2026-09-25):

- *Rules.* Morphological opening (erode, then dilate) with a one-pixel-wide
  structuring element whose length is the page dimension divided by
  `line_scale`. The lattice parser passes 15. The function's own default,
  and the "How it works" page, say 40.
  - That makes the shortest detectable rule a fixed fraction of the page.
  - At 300 dpi on a letter page, 15 gives 220 px vertically and 170 px
    horizontally; 40 gives 82 px and 63 px (arithmetic, not a reading).
  - Their docs warn that a `line_scale` above about 150 detects text as
    lines.
- *Joints.* The pixelwise AND of the horizontal and vertical masks.
- *Tables.* External contours of the OR of the two masks, at least
  0.05% of the page area. A table with four or fewer joints is discarded
  (`if len(jc) <= 4: continue`).
- *Grid.* Joint x-coordinates within `line_tol` (2) merge into column
  anchors, and y-coordinates into row anchors. Each grid cell then gets an
  edge flag per side when a segment lies within `joint_tol` (2) of it.
- *Spanning cells.* These are grid cells with a missing edge. Text in a
  spanning cell is moved along `shift_text` (default left, then top) to the
  cell that owns it. `copy_text` optionally repeats it across the span.

**Tabula, spreadsheet mode** (`SpreadsheetExtractionAlgorithm.findCells`,
read 2026-09-25):

- The crossings of horizontal and vertical rulings are the candidate
  corners.
- For each crossing taken as a top-left corner, it walks crossings below
  it on the same vertical ruling and to its right on the same horizontal
  ruling.
- It takes the first pair whose bottom-right crossing exists, with rulings
  present on all four sides. That rectangle is the cell.
- A ruling that stops inside a rectangle does not split it, so spanning
  cells come out directly.
- Whether a page is tabular at all is a ratio test between the rule-based
  and text-based row and column counts (`MAGIC_HEURISTIC_NUMBER = 0.65`).

**What this means for 9a.**

1. The spec's cell is Tabula's cell: the nearest closed rectangle from
   each crossing. Camelot's grid with edge flags gives the same rectangles
   on a well-formed grid, and adds what 9c's `GriTS_Top` needs: each
   cell's row and column span.
   - Both come cheaply: enumerate cells Tabula's way, then take the
     region's distinct cell-edge coordinates as anchors, and report each
     cell's span in those anchors.
2. Neither tool's length floor transfers. Camelot's is page-relative. A
   slip's box side is short compared with the page but long compared with
   the type, which is why the spec measures rule length in `h`, the page's
   median glyph height.
   - That this beats a page-relative floor on real slips is an
     expectation, not a reading. The `rules` fixture's tall-box and
     short-box cases are where it gets checked.
3. Camelot's "four or fewer joints is not a table" rule would drop a
   single isolated box. For 9b that is the wrong rule: a lone box with a
   printed box number is a field. The spec's `form.min_boxes` is the
   form-level threshold, and a cell needs no joint count of its own.
4. Both tools assign a word to a cell by coordinates, and Camelot moves
   spanning-cell text to a single owner cell. The spec's rule, the
   smallest cell containing the word's centre, never needs to move text,
   because a spanning cell is one cell.

**Not measured:** any rule or cell accuracy for either tool, or for
OCRcer, on any page.

## Addendum 2026-09-25: how Tesseract finds ruling lines (read from source, for chunk 9a's rule detector)

**Why.** Chunk 9a's rule detector (`ARCHITECTURE.md` §11, "Candidate chunk
9 spec, part 1") leaves its length and thickness thresholds as unnamed
guesses. Tesseract has shipped a line finder for years. It was read before
anyone builds the detector.

**Source.** `src/textord/linefind.cpp`, tesseract-ocr/tesseract `main`,
read 2026-09-25.

**Thresholds, all fractions of the scan resolution:**

- `kThinLineFraction = 20`: the widest a line may be is `resolution / 20`
  (1/20 inch).
- `kMinLineLengthFraction = 4`: the shortest a line may be is
  `resolution / 4` (1/4 inch).
- `closing_brick = max_line_width / 3`: 1/60 inch.
- `kThickLengthMultiple = 0.75` and `kMinThickLineWidth = 12` px: a
  candidate at least 12 px in both dimensions, with a stroke wider than
  12 px and shorter than 0.75 inch in both dimensions, is too thick for
  its length and is rejected.
- `kMaxNonLineDensity = 0.25`: see the text test below.

**Method (`GetLineMasks`):**

1. A morphological closing with a `closing_brick` square bridges small
   breaks.
2. An opening with a `max_line_width` square finds solid areas, which are
   subtracted, so only thin structures remain.
3. Directional openings (`1 x min_line_length`, then `min_line_length x 1`)
   keep vertical and horizontal lines.
4. Music staves are filtered out, which is out of scope here.

**The text test (`FilterFalsePositives`, quoted):**

```cpp
if (!bad_line && (NumTouchingIntersections(box, intersection_pix) < 2)) {
  int nonline_count = CountPixelsAdjacentToLine(max_width, box, nonline_pix);
  if (nonline_count > box_height * box_width * kMaxNonLineDensity) {
    bad_line = true;
  }
}
```

A line crossed by fewer than two lines of the other orientation is
rejected when the ink beside it is dense. The band counted extends the
line's own stroke width on each side, and the threshold is more than 25%
of the line's box area. This removes strike-throughs and runs formed
inside text.

**Removal.** `SubtractLinesAndResidue` dilates the found lines, seed-fills
the residue connected to them, and erases only the line pixels. Text
touching a line keeps its pixels. Tesseract erases both orientations
before layout.

**What this means for 9a** (the decisions are in `ARCHITECTURE.md` §11,
"Chunk 9 spec, part 1, amended again: rule length, short box sides, and
breaks"):

1. **Length.** OCRcer measures in `h`, not inches. The page's median
   glyphish-component height `h` for 10-point type lies between the
   x-height and the cap height, roughly 0.0625 to 0.092 inch. At that
   size Tesseract's 1/4 inch is 2.7 to 4 `h`, and its 1/20 inch is 0.54
   to 0.8 `h`. This is arithmetic, not a reading.
2. **The floor already exists.** `lines.rule_run_heights` (5.4209, measured)
   is twice the longest horizontal ink run any glyph in the bank makes:
   the em dash, at 2.7105 x-heights. The tallest thin glyph run is `|`,
   at 2.2859. So 5.4209 `h` is a floor no single glyph reaches, in either
   orientation. It is longer than Tesseract's 1/4 inch.
3. **The cost of that floor is short box sides.** A box around one line
   of text is about one line pitch tall, roughly 3 `h` at 10/12 pt
   (arithmetic). That is under any glyph-safe floor, including Tesseract's
   own. What makes a short side safe is that it runs between two rules.
4. **Breaks.** Tesseract's closing would also join a tight dot leader into
   a line. A statement's leaders must stay leaders.
5. **The text test** protects a short, 1/4-inch floor. OCRcer's floor is
   twice the longest glyph run, and the underline strip erases on that
   same floor. A test applied to the detector alone would make the two
   disagree about what a rule is. So the test is counted first, not
   adopted.
6. **Erasure.** Tesseract erases both orientations, protecting residue.
   That is prior art for 9a's deferred vertical-rule erasure question,
   which the `LineCrossesCell` count sizes.

**Not measured:** any rule-detection accuracy, for Tesseract or OCRcer, on
any page.

## Addendum 2026-09-25: columns without rules — how Camelot's stream mode finds them (read from source, a train count, for chunk 9c)

**Why.** Chunk 9c must find table columns where no rules are printed. Most
financial statements are set that way. Camelot's stream mode, which
descends from Nurminen's text-edge method, was read before 9c is
specified.

**Source.** camelot-dev/camelot `master`, read 2026-09-25:
`camelot/core.py` (`TextEdge`, `TextEdges`), `camelot/parsers/stream.py`
(`_generate_columns_and_rows`), and `camelot/parsers/base.py`
(`_group_rows`, `_merge_columns`, `_add_columns`, `_join_columns`).

**Table areas from text edges.**

- The unit is a pdfminer text line, the text run a PDF lays down between
  wide gaps. Units of one character are skipped.
- Each unit offers three coordinates: its left, right and middle x.
- A unit joins an existing edge when its coordinate is within 0.5 pt of
  the edge's coordinate. The edge's coordinate is the running mean of its
  members.
- An edge grows downward while each new unit's bottom is within
  `edge_tol` (default 50 pt) of the edge's current bottom.
- An edge is valid at `TEXTEDGE_REQUIRED_ELEMENTS = 4` units.
- One alignment serves the whole page: whichever of left, right or middle
  has the most units on valid edges.
- Table areas are the union of vertically overlapping valid edges,
  extended by every unit lying inside them, then padded.

**Columns inside an area.**

1. Units are grouped into rows by bottom y, within `row_tol` (2 pt).
2. The column count is the mode of the units-per-row count. If the mode
   is 1, the 1s are dropped and the mode is taken again.
3. The columns are the x-extents of the units in rows with exactly that
   count, merged where they overlap (`column_tol` 0).
4. Units lying between or outside those columns add columns, taken from
   their own rows with the most units.
5. Boundaries go at the midpoints of the gaps.

**What this means for 9c.**

1. **The unit already exists.** OCRcer's column fragment, a line cut at a
   column gap (`lines.column_gap_heights`), plays the pdfminer text line's
   role. Tolerances are in `h`, not points.
2. **One alignment per page is wrong for a statement.** Its labels are
   left-aligned and its numbers right-aligned, so a per-page choice drops
   one or the other. 9c should decide alignment per edge.
3. **Ink edges are not typeset edges.** Excel's built-in accounting format
   is `_($* #,##0.00_);_($* (#,##0.00);_($* "-"??_);_(@_)`.
   - `_)` reserves a parenthesis-wide space after a positive number, so
     the digits of positives and negatives line up.
   - On the page, the ink of a negative's `)` therefore hangs about one
     parenthesis width right of the positives' last digit.
   - A PDF text box includes that reserved space. An image does not.
   - Number fragments should align on the ink of their last digit. That
     needs recognised text, which the structure layer may read and never
     writes.
4. **A dash can mean zero.** The same format prints zero as a dash. A lone
   dash in a number column is a value, and 9d's footing reads it as 0.
5. **The `$` sits apart.** `$*` pads the dollar sign to the column's left
   edge, on the rows that carry one, usually the first and the total. A
   `$` fragment is not a column of its own.
6. **The modal column count breaks on statements.** Section headers and
   subtotal labels are one-unit rows, and the mode ignores them only when
   they are the majority. 9c should take columns from edges, and use row
   counts as a check.

**Train count** (finfilings-train truth text, all 427 pages; a count of
the truth, not a recognition reading):

- 104 pages have at least one line with two or more number tokens, 295
  such lines in all;
- 47 parenthesised negatives on 13 pages;
- 7 lone dashes in those lines, on 6 pages.

Table-shaped text is therefore a minority of finfilings-train. 9c's
benchmark cannot come from finfilings alone. That agrees with the 9a spec:
structure is tuned on our own rendered dev set.

**Not measured:** any column-detection accuracy, for Camelot or OCRcer, on
any page.

## Addendum 2026-09-25: abandon a losing distance sooner — sum the telling dimensions first (partial distance search, and the UCR Suite's reordering)

**Why.** Matching is about 95% of page time (`2026-09-24_dense_page_speed.md`).
The pivot-index branch cut dimensions summed by about 29%, but wall time by
only 3–7%. Where the matcher spends its dimensions decides the next speed
step.

**Prior art.**

- **Partial distance search** (Bei & Gray, 1985). Stop summing a
  candidate's squared differences once the partial sum passes the best so
  far. Their abstract reports up to 70% fewer multiplications for a
  full-search vector quantiser. OCRcer already does this, checking every
  16 dimensions.
- **Reordering early abandoning** (Rakthanmanon et al., the UCR Suite, KDD
  2012; IJCAI 2013 summary, read 2026-09-25). The order of the terms
  decides how soon the sum crosses the bound. Their figure abandons after 5
  of 32 terms instead of 9. For z-normalised series they sort the indices by
  the query's own absolute standardised value: a term is likely to be large
  where the query sits far from the mean.

**What the code does** (read from `match.rs`, `feature.rs` and
`model/feature_weights.tsv`):

- Dimensions are summed in feature order, 0 to 106. The checkpoint fires
  when `i % 16 == 15`, and the last one is at `i = 95`.
- Dimensions 96 to 106 are summed after the last checkpoint: the hole
  count, the six crossings and the four geometry dimensions. They never
  help abandon a candidate.
- Geometry is the only block weighted above 1 (6.0, measured), because it
  is the only block that separates case pairs. So the dimensions most likely
  to be large against a wrong class are the ones never checked.

**Readings.** These are from the pivot report's own counters; the
arithmetic is mine. Wall time is indicative, because the machine is shared.

| Page | dims per visited prototype, before → after | ns of `match()` per dim, before → after |
|---|---|---|
| s0 r000830 | 50.1 → 59.4 | 2.74 → 3.75 |
| s3 r000276 | 49.5 → 59.1 | 2.86 → 3.80 |
| s0 r000917 | 49.9 → 59.3 | 2.87 → 3.75 |

- The pivot walk removes far-off prototypes, which were abandoned early.
  The ones it still visits take about 59 dimensions to abandon.
- Time per dimension rose by about a third. The per-query pivot pass is
  about 20,000 dimension operations (187 pivots × 107). That is small next
  to the roughly 0.93 million dimensions summed per query (dims summed over
  each page's captured query count), so it cannot explain the rise.
- The cause is not measured. Visit order in memory and per-call overhead
  are the candidates.

**What this suggests.** These are candidates; none is measured.

1. **A fixed order, heaviest first.** Sum by descending `w_i · σ_i²`, with
   σ the bank's per-dimension spread, and checkpoint after the first four
   dimensions as well as every 16.
   - If the bank is standardised on itself, σ is about 1 and this puts
     geometry first.
   - The order is computed at load. There is no format change, and
     `Model.prototypes` keeps file order and meaning.
2. **A per-query order: the UCR rule, weighted.** Sort dimensions by
   `w_i · ((q_i − μ_i)² + σ_i²)`. That is a term's expected size against a
   random prototype, with μ and σ the bank's per-dimension mean and spread.
   - Cost: a 107-element sort per query.
   - Reads within each 428-byte row then land out of order.
3. **Exactness.** A different summation order rounds differently in f64,
   so the reordered sum only decides abandonment.
   - It tests against the pivot branch's slack.
   - A candidate that survives is re-summed in file order, and only that
     sum is recorded, so output is byte-identical by construction.
   - Survivors were 2–4% of visits before the pivot branch (abandon rate
     96–98%, dense-page report), so the re-sum should cost little. The rate
     after the pivot branch is not read.
4. **Measure time per dimension apart from the dimension count.** Run the
   captured queries single-threaded, with the process pinned to one core.
   This is still indicative on a busy machine, but it separates the two
   things the pivot report could not.

**Gate:** the pivot index's four gates.
- Identity on the captured queries.
- Byte-identical pages.
- Train stride-6 byte identity.
- A cut in dimensions summed, with wall time reported as indicative.

**Sources.**
- C.-D. Bei and R. M. Gray, "An improvement of the minimum distortion
  encoding algorithm for vector quantization", *IEEE Trans. Commun.*
  33(10):1132–1133, 1985 (abstract only).
- T. Rakthanmanon et al., "Searching and mining trillions of time series
  subsequences under dynamic time warping", KDD 2012.
- The IJCAI 2013 summary, "Data mining a trillion time series subsequences
  under dynamic time warping", section "Reordering Early Abandoning", read
  2026-09-25.

**Not measured:** any speed-up from reordering on OCRcer.

## Addendum 2026-09-25: whole pages scanned sideways or upside down — Tesseract's OSD and Leptonica's flip test (read from source, and a train check)

**Why.** pdfcer applies a page's `/Rotate` before OCR, and the adapter
passes OCRcer an upright raster. A scan whose *image* is sideways or upside
down, with `/Rotate` 0, still reaches OCRcer that way. Chunk 13c (§11,
2026-09-25) turns rotated strings on drawings by exact quarter turns, and
leaves upside-down text and whole pages as follow-ups. This addendum reads
two open-source detectors before that follow-up is specified.

**Leptonica, `flipdetect.c`** (BSD-2, read 2026-09-25).
- *Signal.* A morphological closing joins characters at x-height. Hit-miss
  patterns then count ascenders and descenders. The source's reason: in
  Roman text, straight-line ascenders (b, d, h, k, l, t) outnumber
  descenders (g, p, q).
- *Confidence.* `2 · (n_up − n_down) / sqrt(n_up + n_down)`, computed only
  once the larger count exceeds 70.
- *Decision.* Upright needs `upconf > 8.0` and `|upconf| > 2.5·|leftconf|`,
  where `leftconf` is the same test on the page turned 90°. The other three
  orientations are symmetric.
- *Resolution.* 150 to 300 ppi.
- *Stated failures.* The source says the method "will fail on some
  images, such as tables, where most characters are numbers". A leading 1
  or 3 can count as an ascender, and 7 matches a descender.
  - Number tables are much of OCRcer's domain. So this detector does not
    transfer as a page test.

**Tesseract, OSD** (`osdetect.cpp`, `pagesegmain.cpp`, Apache-2.0, read
2026-09-25).
- *Sample.*
  - Keep blobs with an aspect of 2 or less and a height of at least 10 px.
  - Visit them in a deterministic quasi-random order.
  - Try at least `min_characters_to_try` (50) and at most five times that.
  - Skip the page if fewer than 25 qualify.
- *Per blob.*
  - Normalise the blob at each of the four rotations and classify it with
    the ordinary character classifier.
  - Map the top certainty, which runs from −20 to 0, onto 0 to 1:
    `1 + 0.05·certainty`.
  - An orientation with no choice takes the worst of the others, halved if
    only one orientation scored.
  - Normalise the four scores to sum to 1, and add their logs to page
    totals.
- *Decision.* The best total wins. The margin is its lead over the
  runner-up, and `min_orientation_margin` defaults to 7.0. A weak margin
  still rotates, with one exception: weak evidence for upside-down Latin
  text on horizontal lines is overridden to "do not rotate".
- *Order of questions.* Whether lines run vertically is decided first,
  from geometry: `IsVerticallyAlignedText` checks whether more than half
  the text blobs sit in vertical alignment. The classifier then picks
  among the four turns.
- *Early exit.* None. `detect_blob` returns false with a TODO to add a
  margin-based stop.

**What this means for OCRcer.**
1. The recogniser is the better detector for this domain. It needs no
   ascender statistics, so number tables do not break it outright.
   - OCRcer already turns pixels by exact quarter turns (chunk 13c). So
     "classify at four turns" reuses the bank and the one extractor. It
     needs no new prototypes and no second implementation (rule 4).
   - Each turn's per-blob score would be the calibrated confidence, which
     is already on a 0-to-1 scale.
2. **Digits are weak evidence for 180°.** 6 and 9 turn into each other,
   and 0, 1 and 8 read about the same either way up. A digit-heavy page
   gets its 180° signal mostly from letters and punctuation.
   - That is an expectation from the shapes, not a reading. It argues for
     Tesseract's prior: weak evidence for upside down means do not
     rotate.
3. **Trigger on a failed upright reading.** Run the check only when the
   page's upright pass comes back with low mean calibrated confidence.
   - Upright pages then pay nothing, and upright stays the default that a
     rotation has to beat. That is the same arbitration chunk 13c uses for
     strings.
   - The cost falls only on suspect pages. Arithmetic, not a reading:
     `match()` on the dense-page runs took about 3.5 ms per query. So 50
     blobs at 4 turns is about 0.7 s, and 250 blobs at 4 turns is about
     3.5 s.
4. **Stop early on a clear margin.** Tesseract has only a TODO here. A
   running margin, checked after the 50-blob minimum, would stop most
   pages early. The threshold is fitted, not copied.
5. **Output.** Rotated words keep page coordinates plus a rotation, the
   same additive field chunk 13c adds. No coordinate transform moves into
   the pdfcer adapter.

**Train check** (finfilings-train, 427 pages, 2026-09-25).
- 0 of 427 page images are wider than they are tall. All are 1653×2339.
- A crude projection test counted pages whose blank-column share exceeds
  their blank-row share. It flagged 13 pages.
- Two of those were viewed, plus one page with almost no blank rows. All
  three are upright tables, whose column gaps and rules fool the test. The
  count is therefore not a count of sideways pages, and it was not checked
  page by page.
- The corpus is rendered from filing transcripts. Sideways or upside-down
  content is not expected in it, so finfilings cannot measure this
  feature. It can only guard against false rotation.
- Fitting would need train and val pages turned by exact quarter turns,
  which is the same construction as chunk 13c's synthetic set. Scoring
  pages stay scoring-only.

**Sources.**
- Leptonica `src/flipdetect.c` (D. Bloomberg), read 2026-09-25.
- Tesseract `src/ccmain/osdetect.cpp`, `src/ccmain/pagesegmain.cpp` and
  `src/ccmain/tesseractclass.cpp`, read 2026-09-25.

**Not measured:** the check's accuracy or cost on OCRcer, and how many
real pdfcer scans arrive sideways or upside down.

## Addendum 2026-09-25: identifiers that carry their own check digit — CUSIP and ISIN (standards read, and a train count)

**Why.** PLAN.md chunk 9d uses a page's own arithmetic as a free check on
its digits. Some identifiers carry a smaller check of the same kind inside
one token. A US or Canadian security's CUSIP is one. Holdings tables in
filings list one per row.

**The checks** (read 2026-09-25):
- **CUSIP** (9 characters).
  - Characters 1–6 are the issuer, 7–8 the issue, and 9 the check digit.
  - Digits are worth their value, A–Z are worth 10–35, and `*`, `@`, `#`
    are worth 36–38.
  - Double the value at positions 2, 4, 6 and 8. Add the decimal digits of
    each of the eight values. The check is `(10 − sum mod 10) mod 10`.
  - The letters I and O are not issued, "since they might be mistaken for
    the digits 1 and 0".
  - The page was read through a summariser, which named the odd positions.
    The even-position rule above is the one confirmed on `037833100`
    (Apple) and `921908844`, and by the train pass rate below.
- **ISIN** (12 characters).
  - Two letters for the country, a 9-character national number, then a
    check digit.
  - Turn letters into two-digit numbers (A = 10), then apply Luhn to the
    digit string.
  - A US ISIN wraps the CUSIP: `US0378331005`.
  - Wikipedia notes that it misses some swapped adjacent letters.

**What the CUSIP check catches, computed** (not read):
- Every single-digit substitution. Doubling followed by the digit sum maps
  0–9 onto 0–9 one-to-one, which is the Luhn property.
- An adjacent swap is missed only for 0 and 9.
- Insertions, deletions, splits and merges change the length, so the
  token no longer has the CUSIP shape.
- Twenty pairs were tested: every digit/capital-letter row in
  `model/confusions.tsv`, other digit/capital look-alikes, and common
  digit/digit misreads. Lower-case rows do not apply, because a CUSIP has
  no lower case. Every pair is
  caught at every position, except 5/S at an even position.
  - O/0 and I/1 need no check at all, because the standard excludes the
    letters.

**Train count** (finfilings-train truth text, 427 pages, 2026-09-25):
- Tokens of CUSIP shape were counted: 8 characters from the CUSIP alphabet
  plus a final digit, with at least 5 digits.
  - 936 contain a letter, and 933 pass the check.
  - 837 are all digits, and 830 pass. A random 9-digit number passes one
    time in ten, so these are CUSIPs too.
- They sit on 52 pages. Those pages hold a median of 41 such tokens and a
  maximum of 50.
- **None of the 52 pages contains the word "CUSIP".** They are
  continuation pages of holdings tables, whose headers are elsewhere. A
  header cannot be the cue.
- ISIN shape: 75 tokens on 2 pages, and only 10 pass. Most are other
  codes, so ISIN is not worth a mechanism on this corpus.
- The 10 failing CUSIP-shaped tokens include `000000079` and `CMS040105`.
  Some are not CUSIPs at all; the rest were not checked.

**What this means for OCRcer.**
1. **The column is the cue.** A random 9-character column passes one time
   in ten per token. Three passing tokens out of three in one column is a
   one-in-a-thousand chance.
   - So a column of CUSIP-shaped tokens where nearly all pass is a CUSIP
     column. That is decided from recognised text, which structure may
     read. This is the same column-type idea as the "lone digit in a
     number column" addendum.
   - Chunk 9 supplies the columns. The vote threshold is fitted on train.
2. **First use: flag and confidence only.** In a CUSIP column, a token that
   fails its check gets its confidence capped and is flagged. A token
   that passes keeps its confidence.
   - This is PLAN.md chunk 9's rule for arithmetic, applied to one token:
     a failed check is reported and never silently repaired.
   - It also fits rule 5, because a failed check is exactly the case a
     reviewer should be told about.
3. **Choosing among the n-best is a separate decision, and it waits.**
   - A failing token has about nine single-substitution repairs that pass,
     one per position. The check alone cannot say which is right.
   - Only the recogniser's own alternatives can, so a repair picks a
     passing n-best path. That can still pick the wrong position, and the
     result would be a wrong CUSIP that passes its check at high
     confidence. That is rule 6's expensive failure.
   - Before any repair is adopted, a train reading must show how often the
     best passing alternative is the truth. It needs its own §11 entry.
4. **Cost.** A 9-character sum per token. It needs no model data, only an
   authored table of character values.

**Next reading, when the machine is free.** Run the current engine on the
52 train pages. Count the CUSIPs it misreads, how many of those the check
catches, and how many correct reads sit in a column the vote would miss.
Nothing is built before that ceiling is known.

**Sources.**
- "CUSIP" and "International Securities Identification Number",
  Wikipedia, read 2026-09-25. The CUSIP positions were checked by
  computation against known CUSIPs, as above.

**Not measured:** OCRcer's error rate on CUSIPs. Also not measured: the
SIN's Luhn check (third-party sources only) and any check digit on the
CRA business number (none found in an official source).

## Addendum 2026-09-25: store prototypes dimension-major in blocks of 64 (PDX), which keeps each distance's summation order

**Why.** The previous addendum reorders dimensions so that a losing
distance is abandoned sooner. A SIGMOD 2025 paper finds that on the
ordinary row-per-vector layout, such pruning can lose to a plain scan, and
that the memory layout is what restores its benefit. That fits the pivot
report's reading: 29% fewer dimensions summed, but only 3–7% less wall
time. Fitting is not proof, and the cause is still not measured.

**PDX** (Kuffo, Krippner and Boncz, "PDX: A Data Layout for Vector
Similarity Search", SIGMOD 2025, arXiv 2503.04422; abstract and body read
2026-09-25):
- *Layout.* Vectors are stored in blocks, dimension-major within a block.
  "Processing 64 vectors at-a-time" was fastest on NEON, AVX2 and AVX512.
- *Kernel.* Search runs dimension by dimension over all vectors of a block
  at once, in tight loops. The paper says it uses "only … scalar code that
  gets auto-vectorized", and it beats SIMD-optimised distance kernels on
  the row layout by 40% on average.
- *Pruning.*
  - PDX-BOND prunes on the partial distance alone, against the current
    k-th best exact distance, and "does not have any recall trade-off".
  - Dimensions are visited by how far each dimension's mean is from the
    query. That is the UCR ordering again.
  - A warm-up fetches 2, then 4, then 8 dimensions before pruning, and
    survivors are tracked by count and position within the block.
  - Reported: 2–7× for pruning methods once moved onto PDX, and 2.5×
    over FAISS on exact search, averaged over datasets that include 16-
    and 50-dimension ones.

**How this maps onto OCRcer's matcher** (reading of `match.rs`, not a
measurement):
1. **Each prototype's sum is one serial chain today.** The loop adds `f64`
   terms one after another, so every addition waits on the previous one.
   That is the whole of `acc`'s data dependency.
   - Arithmetic only: about 2.8 ns per dimension is about 11 cycles at
     4 GHz, several times one `f64` add's latency. So the chain is not the
     whole story either. This is a reason to measure, not a finding.
2. **Dimension-major blocks keep every prototype's summation order.**
   - With the block's dimensions visited 0, 1, 2, … as today, each
     prototype still adds its own terms in file order. Its `f64` sum is
     therefore bit-identical to today's, by construction. There is no
     re-sum and no slack, unlike reordering.
   - The 64 prototypes in a block are 64 independent chains, which gives
     the processor, and the compiler's auto-vectoriser, work in parallel.
   - It needs no `unsafe` and no intrinsics, so rule 3 holds, and it
     builds for wasm32 unchanged.
3. **Pruning stays exact under the argument OCRcer already uses.** Inside
   a block, the ceiling is a snapshot from the block's start. That
   snapshot is looser than the running ceiling. The dense-page speed
   report shows a looser snapshot never cuts a true top-m winner. After
   the block, the per-prototype acceptance test runs in the block's visit
   order against the running ceiling.
   - Classes outside the top m may record different bests, as they already
     may on the pivot branch. Nothing reads them.
4. **Blocks follow the pivot order.** On the pivot branch, a class's
   prototypes are already sorted by distance to the pivot. A block of 64
   consecutive ones carries a range of that distance, so a whole block can
   be skipped by the same triangle bound the class uses.
5. **Memory.** The layout is built at load, like the pivot index, and the
   file format does not change. Whether it replaces the row layout or
   sits beside it is a spec question. Beside it adds about 21 MB at `f32`
   (arithmetic: 50,095 × 107 × 4 bytes).

**What this changes in the queued follow-up** (backlog, "reordered early
abandon"):
- Measure the layout **first**. It is byte-identical by construction, and
  it isolates the memory effect the pivot report could not.
- Add the dimension reordering on top, as the addendum above specifies,
  with survivors re-summed in file order.
- Both keep the pivot index's four gates. Time is taken on captured
  queries, pinned to one core.

**Not measured:** any of this on OCRcer. The paper's speed-ups come from
embedding benchmarks with different dimension counts and data.

## Addendum 2026-09-25: symbols drawings print that the charset cannot emit — GD&T and hole callouts (standards and Unicode read; nothing measured)

**Why.** `CLAUDE.md` rule 7 names CAD drawing text as the domain. The
drawings in that domain carry symbols the 187-class charset has no class
for:
- the characteristic symbol at the head of every feature control frame;
- the modifiers inside a frame;
- the counterbore, countersink and depth marks in hole callouts.

The 2026-09-22 ⌀ entry in §11 put the argument this way: "a class the
charset does not name is a character the engine cannot emit". This is
in-domain work, not scope creep. Chunk 10's "no symbol recognition" is about
the vector-primitive extractor, not the charset.

**The inventory.** Names are from `UnicodeData.txt` (unicode.org, read
2026-09-25). Drafting meanings are from Wikipedia's *Geometric dimensioning
and tolerancing* page (read 2026-09-25), which cites ASME Y14.5 and ISO 1101.
None of the 27 codepoints is in `model/charset.tsv` (checked by script).

| Group | Symbol, meaning, codepoint (Unicode name) |
|---|---|
| Form | ⏤ straightness U+23E4 (STRAIGHTNESS); ⏥ flatness U+23E5 (FLATNESS); ○ circularity U+25CB (WHITE CIRCLE); ⌭ cylindricity U+232D (CYLINDRICITY) |
| Profile | ⌒ line U+2312 (ARC); ⌓ surface U+2313 (SEGMENT) |
| Orientation | ⟂ perpendicularity U+27C2 (PERPENDICULAR); ∠ angularity U+2220 (ANGLE); ∥ parallelism U+2225 (PARALLEL TO) |
| Location | ⌖ position U+2316 (POSITION INDICATOR); ◎ concentricity U+25CE (BULLSEYE); ⌯ symmetry U+232F (SYMMETRY) |
| Runout | ↗ circular U+2197 (NORTH EAST ARROW); ⌰ total U+2330 (TOTAL RUNOUT) |
| Modifiers | Ⓕ free state U+24BB; Ⓛ LMC U+24C1; Ⓜ MMC U+24C2; Ⓟ projected zone U+24C5; Ⓢ RFS U+24C8; Ⓣ tangent plane U+24C9 |
| Callouts | ⌴ counterbore U+2334 (COUNTERBORE); ⌵ countersink U+2335 (COUNTERSINK); ⌲ U+2332 (CONICAL TAPER); ⌳ U+2333 (SLOPE); ⌱ U+2331 (DIMENSION ORIGIN) |
| By convention only | ↧ depth U+21A7 (DOWNWARDS ARROW FROM BAR); □ square U+25A1 (WHITE SQUARE) |

- For the last row, Unicode's names do not state the drafting meaning. The
  mapping is recalled, not read, and it needs a source before anything is
  authored on it.
- A summariser reading Wikipedia's *Miscellaneous Technical* page named
  U+2316 TELEPHONE RECORDER and U+232D BENZENE RING. Both are one codepoint
  off. The UCD file is what this table uses. This is the same lesson as the
  CUSIP addendum: a summariser's reading is not a source.

**What `ocrs` does.** `ocrs` 0.12.2's `DEFAULT_ALPHABET` (`src/lib.rs`, read
from the cargo registry) is printable ASCII only. A caller can pass its own
alphabet. A grep of pdfcer's crates for `alphabet` finds no override. So
`ocrs` as pdfcer runs it most likely emits none of these symbols, and not ⌀,
°, ± or × either. That is a coverage lead for OCRcer that the head-to-head
does not score today, because no scoring page is a drawing. Not measured.

**The structure these symbols sit in** (for chunk 9a, from the same page;
geometry proportions not read):
- A feature control frame is one row of small ruled compartments: the
  symbol, then the tolerance (often ⌀ plus a value plus modifiers), then up
  to three datum letters.
  - A composite frame stacks two rows under one symbol compartment, which is
    a spanning cell.
  - This is exactly the region-of-cells shape the 9a cell walk emits.
- A basic dimension is a number in a closed rectangle: a one-cell region.
- A datum feature is a boxed letter attached to a triangle.
- **Risk, not measured:** on a drawing, part geometry is also ruled lines.
  The cell walk will close cells out of outlines and assign dimension text
  to them. What 9a's regions mean on a drawing page has not been specified
  or checked. Before pdfcer uses regions for reading order on drawings, that
  needs a synthetic drawing fixture.

**The expensive failure, reasoned (not measured).** Any out-of-charset
symbol is still matched against 187 classes. Some nearest shapes are digits:
- ○ against `0`/`O`/`o`;
- ⟂ against `1`/`L`;
- ∥ against `11`/`ll`;
- ⏤ against `-`/`—`;
- ⌵ against `v`/`V`;
- ⌴ against `u`/`U`.

`○ 0.05` read as `0 0.05`, or `⟂ 0.1 A` as `1 0.1 A`, changes a number on a
drawing. If the margin is high, confidence says nothing is wrong. That is
rule 6's failure in a different place, and it is the question that decides
whether the charset protocol is warranted.

**Cheaper steps first, in order** (the charset-change protocol, step 1):
1. **Probe what the engine emits today.**
   - Render short callout lines with each symbol in a licence-clean face
     that draws it: `○ 0.05`, `⟂ 0.1 A`, `⌴ ⌀11 ↧6.4`, `⌖ ⌀0.2 Ⓜ A B C`.
   - Read the top-1 class, the margin and the reported confidence for each
     symbol.
   - This is a probe, not a scoring set, and it fits nothing. Run it
     light, one process, not alongside a heavy run.
   - If every symbol comes back as low confidence, rule 5's honest path
     already holds, and the charset change can wait for demand.
   - If any comes back as a confident digit or letter, that is the measured
     accuracy problem the protocol requires.
2. **Coverage census** (`ocrcer-glyphs`). Which licence-clean faces draw
   these 27 codepoints? Candidates, none checked:
   - Noto Sans Symbols;
   - Noto Sans Math and STIX Two Math (both OFL);
   - DejaVu Sans;
   - the authored ISO 3098 face.

   The ⌀ audit found that the CAD-vendor faces which draw them are barred
   by rule 2. If coverage is thin, authoring the shapes is the answer, as it
   was for ⌀.
3. **Only then, the charset change.** Append the classes after class 186,
   so no existing index moves. Then:
   - the §2 edit, the `.ocrw` `version` bump and the meta extractor-identifier
     bump;
   - a full bank rebuild;
   - bigram category backoff rows and confusion rows for the look-alikes
     above;
   - lexicon suppression inside frames (rule 6), because frame contents are
     identifiers, not words;
   - a §11 entry.

**Not measured:** any recognition behaviour on these symbols, how often
they occur (there is no licence-clean drawing corpus to count them in), face
coverage, and how `ocrs` behaves with pdfcer's actual parameters.

## Addendum 2026-09-25: stacked tolerances on drawings — eDOCr's split, and why line grouping likely interleaves them (paper read; nothing measured)

**Why.** A toleranced dimension on a drawing often prints its deviations as
two small lines stacked in one column after the nominal: `25` then `+0.1`
over `−0.05`. The raised-characters addendum above names this case and says
it is in no spec. Its expensive failure is a changed number.

**What the standard says.** Not read. ASME Y14.5 is paywalled, and the free
copies found are not licensed. A search summary says limit dimensions put
the high limit above the low one, and plus/minus tolerances follow the
dimension. Treat that as recalled. The probe below needs only geometry,
which a render supplies exactly.

**What eDOCr does.** Villena Toro et al., *Optical character recognition on
engineering drawings to achieve automation in production quality control*,
Frontiers in Manufacturing Technology 2023. Read through a summariser
2026-09-25; the quoted steps are the paper's wording as returned.
- It runs on a dimension box from a neural detector, not a whole line.
- Step 1: find the box's ink y-range.
- Step 2: scan right to left for the gap to the nearest ink, only in the
  middle 30–70% of the box's width, "to avoid lower or higher characters".
- Step 3: a gap wider than 80% of the ink height means the box holds
  tolerances.
- Step 4: the nominal ends at the first ink-free column from the widest
  gap.
- Step 5: the tolerance column is cut vertically at the first ink below the
  gap's y, which gives the upper and lower boxes.
- Its recogniser alphabet for dimensions is digits, the letters
  `AaBCDGHhMmnR`, and `(), + − ± : /°∅.`. GD&T symbols have a separate
  recogniser that runs on the first compartment of a frame. Frames are found
  as two or more adjacent boxes.
- Reported: 90% detection precision and recall, and 8% CER, on seven
  drawings. One of them is private.

**Why OCRcer is likely to get this wrong today** (reasoned from §6, not
measured):
- Line grouping joins components by vertical overlap. Both deviations
  overlap the nominal's band, so they probably join the nominal's line.
- Components on one line are then ordered by x. Two rows sharing an x
  range would interleave: `+0.1` over `−0.05` could come out like `+−00..15`.
- The cut at `lines.column_gap_heights` does not separate the two rows,
  because it cuts across x, and the rows share x.
- Stacked inch fractions (numerator over a bar over denominator) are the
  same shape. The bar is about one text height long, which is short for
  9a's `rule_min_h`.

**What to do first: the same probe as the GD&T symbols.**
- Render callout lines in a licence-clean face with exact ground truth:
  - a nominal with stacked `+a`/`−b` deviations at about 0.7 of the
    nominal's height (authored, a guess);
  - a stacked limit pair;
  - a stacked inch fraction.
- Read what the engine emits and in what order.
- If the output interleaves, the fix belongs in line grouping. The fix
  would detect a column of two small rows inside a band's x-gap and emit
  them as separate words, top first. That is a spec to write only after
  the reading, with `guess` thresholds and the synthetic fit/fixture split
  13c already uses.

**Not measured:** everything above for OCRcer, and how often stacked
tolerances occur. There is no licence-clean drawing corpus to count them
in, and scoring pages are not counted to justify design.

## Addendum 2026-09-25: digits share one width — measure a space inside a number from digit centres, not from the ink gap (font metrics measured)

**Why.** Census bucket (o) counts spurious spaces inside numeric truth
tokens. Most text faces set their default digits "tabular": every digit
has one advance, so columns of figures line up. A narrow `1` then carries
wide side bearings. The ink gap next to it grows while the pitch stays
fixed. This is a typographic fact about the faces, readable from their
metrics, so it was measured rather than assumed.

**Method.** `tools/digit_pitch.py` (committed), run 2026-09-25 over every
`shippable` row of `model/fonts.tsv`. It reads font metrics only: `hmtx`
advances and outline bounds. It uses no kerning and renders nothing. For
every ordered digit pair (a, b) it compares:
- the ink gap `rsb(a) + lsb(b)` against the same pair with a space,
  `rsb(a) + space + lsb(b)`;
- the ink-centre distance, the same way.

A ratio is the largest within-number value over the smallest across-space
value. A ratio of 1 or more means that no single threshold separates every
pair, even on a perfect render.

**Readings** (53 faces measured; `norm-stroke`'s path is missing):
- 51 faces have tabular digits. Both Inter styles are proportional.
- **Ink gap: five faces have a ratio of 1 or more.**
  - Roboto Italic 1.12;
  - Roboto Condensed Italic 1.04;
  - Noto Sans Regular 1.02;
  - Open Sans Condensed Light 1.02;
  - Noto Serif Italic 1.01.

  Twelve faces are at 0.9 or more. In 24 faces the worst pair is `11`, and
  in 10 it is `17`.
- **Centre distance: every face is below 1.** The largest is 0.92, in both
  Noto Serif italics.
- Liberation Sans and Serif (Arial and Times metrics, the likely filing
  faces):
  - ink ratio 0.36–0.52 upright and 0.56–0.78 italic;
  - centre ratio 0.70–0.88.

**What it means** (reasoned, not measured on pages):
- On faces like these, an ink-gap test cannot be right for both `1 1` and
  `11`. A centre-distance test can, with at least 8% of the across-space
  distance to spare on perfect geometry.
- Rendering and binarisation move ink edges by about a pixel. At 20 px/em
  that is 0.05 em, a large share of that margin. Real pages will overlap
  more than these ratios show.
- The pitch needs no knowledge of the face. It is the median
  centre-to-centre distance of adjacent digits in the same number or
  column.
- The layout-stage word splitter runs before classes are known, so this
  test belongs after recognition. That is the "fuzzy-space decoder
  resolution" backlog item, where Tesseract settles doubtful spaces too:
  - a gap between two digit-classified glyphs whose centre distance is
    within the line's digit pitch plus a margin is joined;
  - the margin is `guess`;
  - the pitch is a median of the line's own digit pairs;
  - it is never applied across a comma or a period.

**Queued, not specced.**
- Bucket (o)'s neighbour split, once the census runs, says whether the
  spurious spaces on finfilings-train sit next to `1`.
- If they do, the test above is a candidate spec. Fitted on train, it is
  gated on val with the usual guards, and the identifier test must pass.
- If they do not, font geometry is not the cause on that corpus, and this
  addendum stays a reading.

**Not measured:** any engine output, and the faces finfilings pages are
actually set in.

## Addendum 2026-09-25: faxed pages — the grid is not square, so "native resolution" is two numbers (standards read, pdfcer and OCRmyPDF source read; nothing measured)

An accounting office still receives faxes: supplier invoices, bank letters,
signed forms. They reach pdfcer as PDFs or TIFFs made from Group 3 fax
images. FEASIBILITY and ARCHITECTURE already name fax-grade input as the
place a trained CNN wins. This addendum is about a narrower thing the
engine controls: how the pixels arrive.

**The fax grid (ITU-T T.4, read 2026-09-25).**
- Horizontal: 1728 pels over 215 mm, 8 pels/mm, about 204 dpi.
- Vertical: 3.85 lines/mm (about 98 lpi) in standard mode, and 7.7 lines/mm
  (about 196 lpi) in fine mode.
- A standard-mode page is therefore sampled half as finely down the page
  as across it. 10 pt text has an em of about 13.6 rows, at or below the
  13/14 px-per-em accuracy cliff (§11, 2026-09-22). The x-height is 6 to 7
  rows.
- Squaring the grid by repeating rows is an established practice. libtiff's
  `fax2tiff -s` "stretch[es] the input image vertically by writing each
  input row of data twice to the output file". So a fax can arrive already
  square, with nearest-neighbour magnification on one axis baked into the
  file. How often fax services do this is not known here.

**What OCRmyPDF does (`_pipeline.py`, read 2026-09-25).**
`get_page_square_dpi` is documented as "Get the DPI when we require xres ==
yres". It rasterises at the larger of the two image resolutions, with
vector pages at 400 dpi. A standard-mode fax is rendered at about 204 dpi
on both axes, so the vertical axis is magnified by about 2.

**What pdfcer's renderer does (read from `pdfcer-render/src/interpret.rs`,
`image_geometry` and `is_minified`; not run).**
- It picks one filter per image, not one per axis.
- Bilinear is used if the image sets `/Interpolate`, or if smooth
  minification is on (the default) and the image is minified.
- "Minified" means either axis: `sx < w || sy < h`. Otherwise it uses
  Nearest.
- Take a standard-mode fax, 1728 × about 1100, on a letter page:
  - rendered below about 203 dpi, the horizontal axis minifies, so both
    axes are drawn bilinear;
  - rendered at or above about 203 dpi, neither axis minifies, so both are
    Nearest and each fax row is painted about twice (three times at 300
    dpi).
- The switch sits at the fax's own horizontal resolution. That is the
  number a caller following §8.1's "pass at native resolution" is most
  likely to pick. The vertical doubling is the same pixel replication the
  §11 2026-09-25 entry measured garbling words (`project` → `projæt`),
  applied on one axis.

**The contract is ambiguous here.** §8.1 says to pass raster sources at
native resolution and to magnify only with a smoothing filter. For a
non-square source, native resolution has two values, and the engine needs
square pixels: every feature assumes one pixel is as tall as it is wide.
Passing 1728 × 1100 unscaled would squash every glyph to half height. The
reading that follows from the existing rule, and matches OCRmyPDF, is:
- square the grid at the larger resolution;
- magnify the other axis with a smoothing filter.

**What smoothing hides (reasoned).** The per-line floor (§11, 2026-09-21)
reads px per em from the recovered x-height.
- After a smooth ×2 vertical magnification, a standard-mode 10 pt line
  measures about 28 px/em. That is in the "calibrated" tier.
- Its vertical information is about 13.6 rows per em, at the accuracy
  cliff.
- So the engine would report confidence calibrated on clean pages for text
  sampled at the cliff. Rule 5 is exposed, not only accuracy.
- The engine sees only pixels. The `OcrEngine` call has no way to say
  "these rows were magnified". Row duplication is detectable: pairs of
  identical inked rows are systematic. Smooth magnification is much harder
  to detect.

**Queued, not specced.**
- Add a fax arm to the nearest-neighbour measurement already queued behind
  the 12b fold and the 16b runs (§11, 2026-09-25, "Candidate, not
  specced"). Train pages only, rendered at 408 dpi grey:
  - F0: 204 × 204 grey, the control;
  - F1: 204 × 204, thresholded to one bit, which isolates the bilevel loss;
  - F2: 204 × 98, from a vertical box average and then a threshold, with
    rows repeated back to square. This is `fax2tiff -s` and pdfcer's
    Nearest path.
  - F3: the same 204 × 98 source, magnified vertically by bilinear to
    square;
  - F4: 204 × 196 one-bit, fine mode, bilinear ×1.04.
- Report both finfilings metrics per arm, and the per-line px/em tier the
  engine assigns. The threshold for F1 to F4 is `guess` (50%).
  Transmission noise is out of scope for a first reading.
- If F2 is clearly worse than F3, the nearest-neighbour detector's spec
  must detect duplication per axis (rows only), not only both axes.
- If F3 is close to F0, smoothing is enough and the only open item is the
  confidence tier.
- If F3 is far below F0, fax text sits at the cliff, and the honest output
  is a capped confidence. That needs a way to learn the source sampling,
  which is a question for the binding's API, not the engine.

**Recorded now:** the §8.1 clarification above (a §11 entry the same day).
The change on the call side belongs to pdfcer, in pdfcer's own session.

## Addendum 2026-09-25: red stamps over invoice text — the engine sees luma, which keeps a stamp as ink (Tesseract and pdfcer source read, scanner practice read; nothing measured)

Accounting paper carries rubber stamps: PAID, RECEIVED, POSTED, ENTERED,
usually red or blue, often across the total. This addendum asks what a
stamp becomes by the time the engine sees it.

**What reaches the engine (pdfcer read, not run).** pdfcer converts each
rendered RGBA page to Rec.601 luma, `(r*299 + g*587 + b*114) / 1000`,
before calling `recognize` (`pdfcer-cli`, around line 14744). The
`OcrEngine` input is 8-bit grey. The engine never sees colour, so any
colour decision is the caller's.

**What luma does to a stamp (reasoned from §6's Sauvola, window 25,
k 0.34, R 128).**
- A stamp red of (200, 30, 40) has luma 82. A blue stamp of (40, 60, 180)
  has luma 68. Both are ink by any threshold, so the stamp enters
  segmentation as large, rotated components, and text it touches merges
  into them.
- On flat white paper, Sauvola's threshold is m(1 − k) ≈ 168. A grey
  above about 0.66 of the local mean becomes paper.

**What Tesseract does (`src/ccstruct/otsuthr.cpp` and
`src/ccmain/thresholder.cpp`, read 2026-09-25).**
- The default method (legacy Otsu) thresholds each colour channel
  separately.
- A pixel is black if any informative channel is on its foreground side.
  That is a union: coloured ink of any hue is kept, stamps included.
- The Leptonica Otsu and Sauvola methods convert to grey first. Tesseract
  does not drop stamps either.

**Colour dropout (read: drop-out ink practice, scanner dropout settings,
patent US7853074B2).** Forms and invoice scanners remove a chosen ink
colour at capture. The patent keeps dark neutral text by setting a
pixel's grey to max(R, G, B). Two transforms, both reasoned:
- max(R, G, B). Stamp red (200, 30, 40) becomes 200 and the blue stamp
  becomes 180. Both are above 168, so both become paper. A dark stamp,
  such as (150, 0, 0), stays ink. Black text survives. Where it overlaps
  the stamp, the inks mix subtractively and stay dark in every channel,
  so the overlapped text survives too.
- The red channel alone. This drops red, orange and pink, and keeps blue
  as ink.

**Why dropout cannot be a default: red negatives.** Spreadsheet number
formats print negative amounts in red.
- Under max(R, G, B) or the red channel, (255, 0, 0) becomes 255. The
  number is erased, not misread.
- A missing amount on a financial page is invisible and costly, the same
  class of failure as rule 6. Coloured headings and logos are also at
  risk. Navy text (max about 90) survives.

**Three designs, none chosen.**
1. pdfcer offers a colour mode: luma by default, dropout as an opt-in for
   a batch known to carry stamps, with the red-negative warning. No
   engine change. This is pdfcer's call.
2. The engine takes colour and decides per component. A text-sized
   coloured component on a text line is content; one that is large,
   rotated or off-line is a stamp. This changes the binding's input and
   adds a stage, an architecture decision this addendum does not make.
3. Read both luma and dropout, and keep the better line by confidence. It
   costs twice the time.

**Queued, not specced.**
- A synthetic reading on finfilings-train pages only. None of our scored
  corpora has stamps with truth.
- Render each page in RGB. Overlay a stamp: a clean face, the word PAID
  or RECEIVED, rotated ±15°, multiply blend, a seeded position over the
  page's lower half. Stamp colours (200, 30, 40), (150, 0, 0) and
  (40, 60, 180) are guesses.
- Arms:
  - S0: no stamp, luma (the control);
  - S1: stamp, luma;
  - S2: stamp, max(R, G, B);
  - S3: stamp, red channel.
- A collateral arm with no stamp: recolour the negative amounts
  (parenthesised or minus-signed tokens) to (255, 0, 0). Count the
  characters lost under each transform.
- Order: behind the 12b fold, 16b and the nearest-neighbour and fax arms.
- If S1 is close to S0, stamps cost little and nothing changes.
- If S2 recovers most of S1's loss, design 1 goes to pdfcer as an opt-in,
  with the collateral figure as its warning, and design 2 becomes a §11
  candidate.

Sources: https://github.com/tesseract-ocr/tesseract/blob/main/src/ccstruct/otsuthr.cpp,
https://github.com/tesseract-ocr/tesseract/blob/main/src/ccmain/thresholder.cpp,
https://patents.google.com/patent/US7853074B2/en,
https://en.wikipedia.org/wiki/Drop-out_ink.

## Addendum 2026-09-25: highlighter and shaded rows — the binarizer marks the edge of a mid-grey band as ink (engine measured on synthetic lines; a train count)

In an accounting office, reviewers highlight amounts and statements shade
their header rows. Both put a flat mid-grey band behind black text once
pdfcer converts the page to luma (the stamp addendum above).

**Measured on synthetic lines.**
- Setup:
  - three accounting-style lines, drawn by `tools/highlight_lines.py` in
    Liberation Sans at 10 pt and 300 dpi;
  - the band is multiplied over the text, then converted to Rec.601 luma;
  - the third line is banded over its first half only, so one band edge
    falls inside the account number;
  - read by `Engine::recognize` from `ocrcer-core` at master, with the
    master model, through a scratch harness (not committed).
- One face, one size, a clean render. The highlighter RGB values are
  guesses.
- Highlighter results:
  - yellow (luma 230): the text is identical to the unbanded page;
  - green (195): one hyphen is lost, `2026-0930`;
  - blue (178): `jnvoice`, `2026-09-3Ó`;
  - pink (168): the first or last letter of every banded line changes
    (`jnvoice`, `galance`, `pemit`, `applieg`). Where the band ends inside
    the number, `00417` becomes `OOÿ17`;
  - orange (182): a spurious `,` word at confidence 0.80 where the band
    starts.
- Grey ladder results:
  - 215 to 190: the text is correct, except one hyphen lost at 200;
  - 180: edge letters change;
  - 170: `1,234.56` splits into `1` and `234.56`, and spurious words
    `L , 4` appear;
  - 150 to 100: words merge across the line (`palancejorward`);
  - 160 read almost clean, so the damage is not monotonic in this range.

**Mechanism (binarizer output and a component diff inspected).**
- The added ink sits at the band's corners and ends. At about 150 and
  below it also runs along the band's top and bottom edges. The glyphs
  nearest that ink merge with it. Thin strokes inside a band also thin
  out, which is the lost hyphen.
- A Sauvola window that straddles the band edge averages paper and band,
  so its threshold rises above the band's own grey.
- Worked, with §6's k = 0.34 and R = 128:
  - a corner window, one quarter band at 168 and three quarters paper:
    m = 233 and s = 38, so T = 233 × (1 + 0.34 × (38/128 − 1)) ≈ 177.
    The band pixel is ink.
  - a top-edge window, half band: T ≈ 164, so 168 is paper, barely. At
    150, T ≈ 162, so the whole edge is ink.
  - Text inside the window raises s and therefore T, which is why the
    marks gather beside glyphs.

**Train count (finfilings-train, 427 pages; pixels counted, nothing
tuned).**
- 25 pages carry a flat grey band across at least 15% of the page width
  for at least 20 rows. The modal grey is 191 on 24 pages and 213 on one.
- On three of those pages, the banded text read identically with the band
  flattened to white. Confidence moved by a few hundredths either way.
- The corpus's shading is lighter than the onset. So the scoring gates
  would not see this failure if it appeared in client documents.

**Candidate fixes, none chosen.**
1. In pdfcer, lightness-gated colour dropout: use max(R, G, B) where luma
   is above a floor (a guess, about 140), and luma below it.
   - Highlighters (luma 158 to 230) become paper. Red text (luma 76) and
     saturated stamps stay ink.
   - It does nothing for grey shading.
   - Reasoned: anti-aliased edges of red text cross the floor and thin by
     about a pixel.
2. In the engine, flatten the background before Sauvola: divide each pixel
   by a local background estimate that keeps the band's sharp edges.
   - One way is a grey closing with an element wider than the thickest
     stroke. Leptonica's `pixBackgroundNorm` is a tile-based relative.
   - The flattening checked above was done by hand, inside a known
     rectangle.
   - A general version must size the element from the text size, or a
     heading's thick strokes become background.
   - It changes the binarization stage, so every binarized fixture hash
     moves and every stage is re-blessed. That is a §11 decision, gated
     on no loss on either scoring corpus.
3. Before either: turn the generator plus a small bench bin into a probe
   that every binarization change must pass. The ladder is the test; no
   corpus with truth has highlighter.

**Queued, behind the 12b fold.** A train-page arm like the stamp arm:
- H0: no band (the control);
- H1: bands at luma 230, 195, 182 and 168 over seeded amount tokens.
Both finfilings metrics, and the count of digits changed inside banded
numbers.

## Addendum 2026-09-25: dot-matrix print — each dot is its own component, so layout finds hundreds of lines (engine measured on synthetic lines; two patents and one abstract read)

Impact printers print multipart forms, cheques and ledgers, so a scan of one
is a plausible accounting input. How common it is in client documents is
not known. Nothing in `docs/` covered it before this entry. No corpus we
hold is known to contain impact print; none was searched for it.

**Measured on synthetic lines.**
- Setup:
  - four accounting-style lines, drawn by `tools/dotmatrix_lines.py`, 166
    characters with line breaks, so one character is about 0.6 points of
    CER;
  - the dot grid is Liberation Mono (a bank face) at 11 px, thresholded to
    5×7-like shapes, one dot per cell, 1/70 in by 1/72 in pitch at 300 dpi
    (9-pin draft geometry at 10 cpi);
  - arms: `solid` (each cell a filled rectangle, the same shapes with no
    gaps: the control); black dots at 1.3, 1.0, 0.8 and 0.6 of the pitch
    (`d130` overlapping to `d060` separate); `worn080` (grey 110 dots at
    0.8, a worn ribbon). The diameters are guesses;
  - grey pre-steps written by the tool (PIL Gaussian radius 1.5 and 2.5, a
    3×3 minimum filter, a 5×5 closing);
  - binary pre-steps in a scratch harness: `binarize` from `ocrcer-core`,
    then a square dilation of radius 1 (`bd3`) or 2 (`bd5`) or a radius-1
    cross (`bx3`), then the result read as a 0/255 image;
  - read by `Engine::recognize_lines` at master with the master model.
- One face, one grid, one resolution, a clean render with no ribbon noise
  or scanner blur. Four lines is a small sample: differences under about
  three points are a few characters.

CER in percent (above 100 means insertions; raw, the `d100`, `d080`,
`d060` and `worn080` arms return 29 to 839 lines for 4):

| arm | none | gauss15 | gauss25 | min3 | close5 | bd3 | bd5 | bx3 |
|---|---|---|---|---|---|---|---|---|
| solid | 12.7 | | | | | 9.0 | 32.5 | 6.6 |
| d130 | 47.0 | 14.5 | 24.7 | 23.5 | 15.7 | 28.9 | 45.8 | 42.2 |
| d100 | 63.3 | 13.3 | 25.3 | 10.2 | 22.3 | 10.8 | 27.7 | 57.2 |
| d080 | 227.1 | 134.3 | 185.5 | 12.7 | 94.0 | 13.9 | 30.1 | 75.9 |
| d060 | 1372.3 | 981.3 | 100.0 | 51.8 | 136.1 | 45.2 | 10.8 | 180.1 |
| worn080 | 555.4 | 95.8 | 100.0 | 7.8 | 156.6 | 9.6 | 9.6 | 70.5 |

**What the table says.**
- Separated dots break the engine completely: at `d060`, 4 lines come
  back as 839 lines holding 937 words.
- The raw failure is loud. Mean word confidence on the raw dot arms is
  0.11 to 0.28, and at most one word reaches 0.8. The low-confidence LLM
  mode and a reviewer would both see it.
- A wrong pre-step turns it quiet. `d060` after `close5` returns 124 words,
  13 of them at confidence 0.8 or above; after `gauss15`, 7 of 803.
- No single fixed join works. Every column fails at least one arm badly:
  - `bd3` 45.2 on `d060`;
  - `bd5` 30.1 on `d080`;
  - `min3` 23.5 on `d130`;
  - `gauss15` 134.3 on `d080`.
  The best join per arm tracks the gap between dots. Separate dots need the
  wider dilation (`d060`: `bd5` 10.8). Nearly touching dots need one pixel
  (`d080`, `d100`: 10 to 14). Overlapping dots need smoothing, not
  dilation (`d130`: `gauss15` 14.5, `bd3` 28.9).
- Overlapping dots are damaged too (`d130` raw 47.0), though every
  component is whole. The scalloped stroke edge is the likely cause;
  that is read from the arms, not traced.
- The shapes cost something on their own. The `solid` control reads 12.7
  with no gaps at all. Thickening it by one pixel reads 9.0 (`bd3`) and
  6.6 (`bx3`), so the thin one-dot strokes are part of that cost. The
  5×7 shapes come from a bank face, but the bank holds no face drawn at
  that resolution.

**Read, not run.**
- Kodak, EP0552704 (priority 1992): a 5×5 spatial average turns dots into
  strokes, then a contrast stretch and an edge enhancement. Larger dots or
  wider gaps need a larger kernel. Detection uses density profiles along
  horizontal and vertical slices.
- Matrox, US10176399 (priority 2016): dots are found as blobs after a
  second-order derivative filter. The dot pitch is measured in two
  orientations, and the join is sized from those two pitches rather than
  from a fixed kernel. The table above is consistent with that choice.
- Yanikoglu, IJDAR 2000 (abstract only): dot-matrix fonts are fixed pitch.
  The pitch is estimated, the text is judged fixed or proportional, and a
  pitch-based segmenter handles both touching and broken characters.

**Candidate designs, none chosen.**
1. In the engine, a dot join between binarization and component
   labelling:
   - A dot census finds components that are small, near-round and similar
     in size. Their centre-to-centre nearest-neighbour distance clusters
     at 1 to 2 diameters, in both axes, many to a cluster.
   - Each qualifying cluster is dilated inside its own box, with a radius
     sized from the measured gap. The rest of the page is untouched, so a
     laser-printed form with a dot-matrix fill-in keeps its laser text.
   - Overlapping dots do not register as dots. The `d130` column says they
     need smoothing instead; that case stays open.
   - This is a new §8.1 stage and needs a §11 decision. Its gate: every
     binarized fixture hash unchanged (the census must not fire on any
     fixture), and a false-fire count on finfilings-train (counting only).
     Every threshold starts as a labelled guess on the tuning list.
2. In the bank, a licence-clean face drawn on a dot grid. This is a
   script rerun by `ocrcer-glyphs` with no format change, the cheaper fix
   the charset protocol asks for first. It addresses only the shape cost
   (the `solid` row), not the broken components. No such face has been
   licence-checked yet.
3. Before either: add the dot arms to the binarization probe bin proposed
   in the highlighter addendum above.

**Real data.** Receipt corpora may hold impact-printed receipts. Their
content and licences are unchecked, and SROIE is already held out on
licence. Nothing is downloaded without the operator.

**Queued, behind the 12b fold and the merge train.**
- The probe bin with the dot arms and the highlighter ladder, for
  `ocrcer-bench`.
- A dot-census spec, measurement only: the false-fire count on
  finfilings-train and the fire rate on the dot arms, before any join is
  built.
- A licence check of dot-grid faces, for `ocrcer-glyphs`.

## Addendum 2026-09-25: faded ink — text lighter than about grey 175 is not misread but lost, with nothing returned (engine measured on synthetic lines; Leptonica source read; a train count)

Thermal receipts fade, and expense receipts are routine accounting input.
§3 above recommended "Sauvola with a fallback to Wolf-Jolion-style
contrast normalization on flagged low-contrast pages" without measuring
where Sauvola fails. This entry measures it.

**Measured on synthetic lines.**
- Setup:
  - the four lines of the dot-matrix entry above, drawn by
    `tools/faded_lines.py` in Liberation Sans at 10 pt and 300 dpi, 166
    characters with line breaks;
  - ink at luma 0 to 215 on white paper (255) and on off-white paper (235,
    a guess for thermal stock);
  - `_mix` pages add one black heading line above the faded lines. The
    heading is not scored;
  - pre-steps written by the tool: a global stretch, Wolf-Jolion, and a
    local stretch (the tool's docstring gives each);
  - `k0.2` and `k0.1` are the raw page read with `binarize.k` set through
    `Engine::set_param` (the shipped value is 0.34);
  - read by `Engine::recognize_lines` at master with the master model,
    through a scratch harness (not committed).
- The pages are noise-free. Every pre-step and every lower `k` below keeps
  more faint grey as ink, and what that costs on paper texture and scanner
  noise is not measured here.

CER in percent on the four faded lines (100 means nothing came back):

| page | engine | k 0.2 | k 0.1 | stretch | wolf | lstretch |
|---|---|---|---|---|---|---|
| white, ink 160 | 0.0 | 0.0 | 0.0 | 0.6 | 0.6 | 0.6 |
| white, ink 170 | 13.9 | 0.0 | 0.0 | 0.6 | 0.6 | 0.6 |
| white, ink 180 | 100 | 0.0 | 0.0 | 0.6 | 0.6 | 0.6 |
| white, ink 190 | 100 | 0.0 | 0.6 | 0.6 | 1.8 | 0.6 |
| white, ink 200 | 100 | 64.5 | 0.0 | 0.6 | 0.6 | 0.6 |
| white, ink 215 | 100 | 100 | 0.0 | 1.8 | 2.4 | 1.8 |
| off-white, ink 160 | 76.5 | 0.0 | 0.0 | 0.6 | 0.6 | 0.6 |
| off-white, ink 170 | 100 | 0.0 | 0.0 | 0.6 | 1.8 | 0.6 |
| off-white, ink 180 | 100 | 13.9 | 0.6 | 0.6 | 0.6 | 0.6 |
| off-white, ink 190 | 100 | 100 | 0.0 | 0.6 | 0.6 | 0.6 |
| off-white, ink 215 | 100 | 100 | 100 | 1.2 | 1.2 | 100 |
| white + black heading, ink 170 | 13.9 | 0.0 | 0.0 | 13.9 | 100 | 0.6 |
| white + black heading, ink 180 | 100 | 0.0 | 0.0 | 100 | 100 | 0.6 |
| white + black heading, ink 200 | 100 | 64.5 | 0.0 | 100 | 100 | 0.6 |
| off-white + black heading, ink 140 | 0.0 | 0.6 | 0.0 | 0.0 | 45.2 | 0.6 |
| off-white + black heading, ink 160 | 76.5 | 0.0 | 0.0 | 68.7 | 100 | 0.6 |
| off-white + black heading, ink 170 | 100 | 0.0 | 0.0 | 100 | 100 | 0.6 |
| off-white + black heading, ink 215 | 100 | 100 | 100 | 100 | 100 | 100 |

Ink from 0 to 140 reads at 0 to 1.2 under the engine on every page. The
lower `k` values cost a few characters there: black ink on white reads 2.4
at both, against 0.6.

**What the table says.**
- The engine's onset is about luma 175 on white paper, and on off-white
  paper it is already failing at 160. That matches the Sauvola formula in
  §3, worked by hand for 20% ink coverage in the window: on white, T is
  about 178 at ink 175 and about 177 at ink 180; on off-white, about 163
  at ink 160 and 162 at ink 170.
- The loss is silent. Above the onset the engine returns zero words: no
  low-confidence line, nothing for the LLM mode or a reviewer to catch.
  From the output alone, a faded receipt and a blank page look the same.
- A global pre-step fixes a uniformly faded page. The global stretch
  reads every uniform page at 1.8 or better, and Wolf-Jolion at 2.4 or
  better.
- Neither survives one black line on the same page.
  - The heading pins the global minimum at 0, so the stretch does
    nothing.
  - Wolf-Jolion takes its global minimum and maximum deviation from the
    heading and does worse than the engine. Nothing comes back from ink
    160 on either paper, and ink 140 on off-white already reads 45.2.
  - §3's proposed fallback is Wolf-Jolion on flagged pages, so it would
    miss exactly this page.
- Lowering `k` extends the onset without any new code. `k` 0.2 reads
  through ink 190 on white and 170 on off-white. `k` 0.1 reads through 215
  on white and 200 on off-white. That is the cheapest lever
  and the one whose cost these pages cannot show: a lower `k` lowers the
  bar for paper texture as well as for faint ink.
- The local stretch reads every page, mixed or not, at 1.8 or better. The
  exception is ink 215 on paper 235, a range of 20, which is under the
  stretch's floor of 24, so it comes back blank.
  That floor is a guess well below Leptonica's advice (below).

**Read, not run: Leptonica `adaptmap.c`.**
- `pixContrastNorm` "adaptively attempts to expand the contrast to the
  full dynamic range in each tile". Tiles are "typically at least 20"
  pixels. A tile whose contrast is under `mindiff` borrows its minimum and
  maximum from neighbouring tiles. The minimum and maximum maps can be
  smoothed.
- On `mindiff`: it "is used to eliminate results for tiles where it is
  likely that either fg or bg is missing. A value around 50 or more is
  reasonable."
- By arithmetic, not run: a floor of 50 keeps ink 200 on white (a range of
  55) but drops off-white pages with ink above 185. The floor trades faint-ink recovery against
  amplified texture, and choosing it needs noisy pages.
- The file's overview names the two-step pattern: background
  normalisation, then a global threshold.

**Train count (finfilings-train, every 7th page, 61 pages; pixels counted,
nothing tuned).**
- Paper is 255 on all 61.
- The darkest 5% of non-white pixels is 0 on the median page and 194 on
  one page, which was not inspected.
- Apart from that page, nothing in these counts suggests faded text. So,
  as with shading, the scoring gates would not see this failure.

**Candidate designs, none chosen.**
1. Lower `binarize.k`. It is a parameter change with no code. It is
   likely to move binarized fixture hashes wherever anti-aliased edges
   shift, so it needs an all-stage re-bless and a §11 decision. Its cost on noisy scans can
   only be priced on scans, and the scanned sets are scoring-only. It
   needs a noisy train source (a synthetic noise model, labelled so)
   before any value is chosen.
2. Retry on empty: keep the binarizer, and where a region comes back with
   almost no ink but its grey range clears a floor, binarize that region
   again after a local contrast stretch.
   - Output is byte-identical wherever ink was found, so the fixtures stay
     put unless one holds faded text.
   - It gates the same way as the dot-census join in the entry above.
   - It is a new §8.1 step and needs a §11 decision. Its floor starts as a
     labelled guess.
3. Local contrast normalisation on every page before Sauvola, in the
   manner of `pixContrastNorm`. This is the general form of design 2. It
   changes the binarization stage everywhere and carries design 1's
   re-bless and noise-pricing costs.
Separately, and whatever is chosen: a page that yields no words over a non-blank grey
image is worth a diagnostic of its own. The failure is silent, and nothing
downstream can recover words the binarizer never returned.

**Queued, behind the 12b fold and the merge train.**
- Add the faded ladder to the binarization probe bin, next to the
  highlighter and dot arms.
- A synthetic noise arm: paper texture plus scanner noise over the ladder,
  so that `k` and the stretch floor get a price before a spec.

## Addendum 2026-09-25: show-through — the shipped threshold ignores it, and every faded-ink fix above reads it as text (engine measured on synthetic lines; one patent read)

On a page printed on both sides of thin paper, the back shows through,
mirrored and soft. How often client documents do this is not known. This entry prices the faded-ink
levers above against that. The shipped threshold is safe. Every lever that
recovers faded ink also reads show-through somewhere.

**Measured on synthetic lines.**
- Setup, drawn by `tools/showthrough_lines.py`:
  - the front is the same four lines, in black or at ink 160, on white
    paper;
  - the back is eight other lines, drawn black, mirrored, blurred (PIL
    Gaussian radius 2, a guess) and multiplied into the page, so that the
    darkest show-through pixel is luma B;
  - the back starts half a line below the front and runs on below it, so
    the lower half of the page holds show-through only;
  - the B values from 245 (faint) to 185 (heavy) are guesses. How dark real
    show-through gets on client paper is not known;
  - read at master as in the faded-ink entry: `k` 0.34 (shipped), 0.2 and
    0.1 through `set_param`; the global and local stretch are written by
    the tool.
- Noise-free, one face. The front is sharp and only the back is blurred,
  which a real scanner would not do.

CER in percent against the four front lines. Lines returned are in
brackets where they are not 4:

| front, back | engine | k 0.2 | k 0.1 | stretch | lstretch |
|---|---|---|---|---|---|
| black, none | 0.6 | 2.4 | 2.4 | 0.6 | 0.6 |
| black, 245 | 0.6 | 1.8 | 2.4 | 0.6 | 0.6 |
| black, 230 | 0.6 | 1.8 | 2.4 | 0.6 | 57.8 (21) |
| black, 215 | 0.6 | 1.8 | 213.3 (145) | 0.6 | 120.5 (14) |
| black, 200 | 0.6 | 3.0 | 199.4 (40) | 0.6 | 120.5 (14) |
| black, 185 | 0.6 | 198.2 (132) | 150.6 (10) | 0.6 | 120.5 (14) |
| ink 160, none | 0.0 | 0.0 | 0.0 | 0.6 | 0.6 |
| ink 160, 230 | 0.0 | 0.0 | 0.0 | 0.6 | 58.4 (22) |
| ink 160, 215 | 0.0 | 0.0 | 209.6 (149) | 241.6 (167) | 183.1 (32) |
| ink 160, 200 | 0.0 | 0.0 | 185.5 (34) | 162.0 (12) | 134.9 (10) |
| ink 160, 185 | 0.6 | 172.9 (113) | 162.0 (16) | 169.3 (17) | 138.0 (12) |

**What the table says.**
- The shipped engine ignores show-through down to 185 on both fronts.
  The Sauvola bias that loses faded ink is the same bias that keeps the
  back out.
- `k` 0.1, which read faded ink through 215 in the entry above, reads
  show-through at 215 and darker.
  - A sample of the extra lines, read by eye, is mostly single dots and
    commas: at low `k` the soft blobs break into fragments.
  - The fragments are not all low-confidence. On black, 215, words at
    confidence 0.8 or above rise from 15 to 34.
- `k` 0.2 has a window on these pages. It reads faded ink through 190 and
  keeps show-through out down to 200 (3.0 there, against 2.4 with no
  back). Whether that window exists on real paper depends on two
  unknowns: how dark real show-through gets, and how
  much the scanner softens the front.
- The global stretch is safe while the page has black ink, which pins the
  minimum. On a faded front it stretches the show-through into ink, and
  reads worse than any other column (241.6 at 215).
- The local stretch reads the show-through-only half as text wherever the
  local range clears its floor of 24, that is at B 230 and darker. The
  mirrored letters come back as whole garbage words, and the stretch also
  damages the front near them (`00417` read as `o0417`). Leptonica's
  `mindiff` of 50 would drop show-through lighter than 205, and would drop
  faded ink lighter than 205 on white with it (by arithmetic, not run).

**Consequence for the faded-ink designs.**
- A fixed threshold cannot tell faint front ink from equally faint
  show-through. Designs 1 and 3 of the faded-ink entry therefore carry a
  show-through gate as well as a noise gate.
- Design 2 (retry on empty) fires exactly where show-through lives: a
  region with no ink found and a grey range above a floor. It needs a cue
  that tells the two apart before it can ship.
- Edge sharpness is one candidate cue. Take the 99.9th-percentile one-pixel
  grey step over the page's grey range:
  - it is 0.25 on each of the four show-through-only regions tested (B 185
    to 230) and 1.00 on each of four faded fronts (ink 160 to 215);
  - but the generator blurs only the back, so on these pages the cue
    separates by construction. The numbers say nothing about scans.
- Mirroring is another candidate cue. It is not measured.

**Read, not run.**
- Seiko Epson, US8553944 (priority 2010), through a summarising fetch of
  the patent page, not the full text. It removes show-through using the
  scans of both sides and a leakage function calibrated per device. That
  needs the back page paired with the front, which a single scanned page
  does not give.
- Single-sided ("blind") bleed-through removal exists for historical
  documents, for example Sun et al., "Blind Bleed-Through Removal for
  Scanned Historical Document Image With Conditional Random Fields". It was
  found in a search and not read; both fetches returned no text.

**Queued, behind the 12b fold and the merge train.**
- Add the show-through ladder to the binarization probe bin, next to the
  faded ladder, so that any faded-ink change is priced against both at
  once.
- An edge-sharpness reading on real train pages, counting only, before
  the cue is trusted: are front strokes on finfilings-train sharp in this
  measure? The corpus has no show-through that we know of, so this prices
  one side only.
