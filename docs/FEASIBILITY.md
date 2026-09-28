# OCRcer — feasibility assessment

**What was asked (2026-09-18):** an MIT-licensed OCR engine, built from scratch
to replace the one pdfcer uses, **including the model itself — authored from
prior knowledge rather than produced by a training run.**

**Verdict:** feasible. The requirement that the model not be trained is the
constraint that chose the architecture, and there is exactly one high-accuracy
OCR design that satisfies it.

> **2026-09-24, operator:** the no-training requirement was a judgement about
> data volume, not a principle, and it is lifted. Fitting from licence-clean data
> is permitted under `ARCHITECTURE.md` §11's rules. The assessment below is
> kept as written.

---

## 1. The constraint, and how it is satisfied

**A fitted model's weights cannot be authored.** A convolutional recogniser's
accuracy lives in the specific numeric relationships among millions of
parameters, each the residue of millions of gradient updates. There is no
verbal description of those numbers to draw on, and a plausible-looking matrix
of floats scores zero rather than scoring poorly. So the answer to "author the
weights of a CRNN" is not a smaller version of the right answer; it is a file
that loads cleanly and recognises nothing.

**But a model is not required to be a network.** OCR reached 98–99% on clean
printed text for two decades before neural methods, using designs whose every
parameter is a rendered shape, a measured statistic, or a linguistic fact. Those
parameters are constructible: some are authored directly from knowledge, and
the rest are computed by a deterministic script in minutes.

So the architecture is **segmentation-driven prototype matching with a lattice
decoder**, and the model file is seven things:

- a **prototype bank** of ~12,000 feature vectors, computed by rendering every
  character of the charset in every covered font and style; *(projected here;
  the bank as built is 14,088 at four render sizes and 17,610 at five,
  measured 2026-09-22)*
- a **lexicon**, a **character-bigram table**, and a **confusion table** —
  authored, and the place where prior knowledge is the deliverable rather than
  a means to one;
- a **charset**, **normalisation constants**, and the **parameter block**
  holding every threshold in the pipeline.

(Those seven are the *kinds* of content. On disk the realisation splits two of
them and lands at nine blocks, five of which exist today; `ARCHITECTURE.md`
§2 has the byte-level breakdown.)

Nothing in that list requires a dataset, a gradient, or a licence from anyone.

**What this costs, stated up front.** A fitted network learns robustness to
noise, blur and degradation from data. A constructed model cannot, and gets its
robustness instead from feature design and from the language model correcting
what the classifier fumbles. On clean printed input the difference is
negligible. On badly degraded scans it is real, and section 5 quantifies the
expectation rather than burying it.

---

## 2. Why building this is justified rather than reinventing a wheel

pdfcer surveyed the field (`D:\Dev\pdfcer\docs\ocr-engine-survey.md`, 2026-08-12)
and bound to `ocrs`. That survey ends with an unresolved operator question and a
list of verification gaps. Three are structural, and a from-scratch engine is
the only thing that closes all three at once.

**The weights are CC-BY-SA-4.0, and the licence problem is real but subtle.**
`ocrs`'s models inherit share-alike from HierText. The survey's reading — that
shipping unmodified model files beside MIT code is a *collection* rather than an
*adaptation*, so nothing propagates — is well-argued and probably right. But it
is a reading, it requires hand-authored attribution in a file that is otherwise
machine-generated and never hand-edited, and it forecloses fine-tuning: adapting
those weights creates Adapted Material bound to CC-BY-SA-4.0.

A model with no training corpus has nothing to inherit from. The tables are an
original work and can simply be MIT. That removes an open operator question,
removes the attribution special case, and makes domain adaptation a free action
forever. **This argument is stronger for a constructed model than it would have
been for a locally-trained one**, because there is no corpus at all rather than
a corpus we happen to own.

**`ocrs` reports no confidence, and pdfcer has a rule about that.**
`ocrs::TextChar` is a character and a rectangle. No score anywhere, so
`reports_confidence()` returns `false` and pdfcer's whole disclosure chain exists
to carry that absence honestly. Prototype matching yields a **margin** — how
much better the winning class was than its nearest rival — which is both
meaningful and, for a reviewer's purposes, better-behaved than a softmax
posterior. A character that two classes match equally well reports low
confidence even when its absolute match was good, which is exactly the case a
softmax tends to hide.

**The target domain is narrow and nobody targets it.** pdfcer reads CAD exports,
Word, LibreOffice, print-to-PDF and office scans. `ocrs` is trained on HierText —
*scene text*, photographs of signs and storefronts. Clean, axis-aligned,
high-contrast document text with diameter symbols and tolerance callouts is a
domain where a prototype bank covering the relevant faces exhaustively is an
excellent fit, and where the shapes are stable enough that constructed
prototypes generalise well.

**A fourth, smaller win.** pdfcer buys `forbid(unsafe_code)` deliberately and
pays decode speed for it; every neural runtime inverts that trade. This engine
has no tensor library, no C toolchain, no DLL and no prebuilt blob — pure safe
Rust with zero dependencies, passing the `wasm32-unknown-unknown` gate for free.
It is also Rust all the way down, including the tooling that builds the model,
which means one language for the operator to maintain and no second
implementation of any stage that could quietly drift from the shipped one.

---

## 3. What will be built

| | |
|---|---|
| Model | Seven kinds of table, ~2.2 MB, constructed and authored |
| Runtime | `ocrcer-core`, pure safe Rust, zero dependencies, `forbid(unsafe_code)` |
| Tooling | `ocrcer-build` and `ocrcer-bench`, Rust, dependencies permitted, never shipped |
| Charset | 187 classes (projected ~200) including the engineering symbols `Ø ⌀ ° ± × ÷ √ ≤ ≥ ≈` |
| Output | Words with pixel rectangles and calibrated per-word confidence |

`ocrs` ships 12.24 MB. This ships **~2.2 MB**, with confidence scores and no
licence question attached. Full detail in `ARCHITECTURE.md`.

*The size figure in this section was revised 2026-09-22 and the revision went
against the project.* The original projection was ~1.7 MB, assuming a
~12,000-prototype bank. The file that exists measures 1,548,603 B
with five of nine blocks present at four render sizes; adding the four
authored blocks at their own (still unmeasured) estimates, at the five-size
ladder every benchmark figure is quoted at, projects about 2.2 MB. The ratio
the comparison was making is what matters and it is barely dented: a fifth of
`ocrs`, rather than a seventh. `ARCHITECTURE.md` §2 carries the byte-level
breakdown with each figure marked measured or estimated.

---

## 4. Can this machine build it

Measured 2026-09-18: i9-10900KF 10C/20T, Intel Arc Pro B50, 15.9 GB RAM,
119 GB free on D:, 529 fonts in `C:\Windows\Fonts`. Only the CPU and the font
inventory are load-bearing.

**Comfortably, and the margin is large enough to be worth stating plainly.**
Constructing the prototype bank is rendering ~12,000 glyphs and extracting a
107-dimensional vector from each: **minutes on one core**. (Measured
2026-09-22 at the real bank size: 17,610 prototypes from 19 faces at five
sizes, built in **45.0 s** on one core. The projection held.) The GPU is
irrelevant and is not used. The 16 GB of RAM is irrelevant. The disk
requirement is megabytes. Nothing in the build waits on hardware.

The consequence that matters is not speed, it is that the model is **cheap to
rebuild**. Adding a font family, adjusting a feature definition, or extending
the charset is a minutes-long script run producing identical bytes every time.
The tuning loop in the benchmark chunk is therefore tight enough to actually
run many times, which is the difference between tuning being a plan and tuning
being a habit.

---

## 5. What it will and will not do

Projections, to be replaced by measurements from `ocrcer-bench`.

**Expected to work well**

- Digital-born printed text at 200 DPI or better, in covered faces: **98–99.5%**
  character accuracy.
- CAD drawing text — short, high-contrast strings, including the symbols no
  general engine has seen. The original wording of this bullet said those
  strings sit in a small set of faces the bank can cover exhaustively.
  **Withdrawn, measured 2026-09-22:** the drafting faces are CAD-vendor
  proprietary — Dassault's DS ISO 1, Autodesk's GENISO / ISOCPEUR / ISOCTEUR,
  GOST — and the bank may not hold any of them. It covers this domain with
  general faces plus the authored ISO 3098 face, which is drawn to the same
  standard those faces implement. The projection stands on that; it no longer
  stands on face coverage.
- Clean 300 DPI office scans: **95–98%**.
- Unseen decorative faces: degrades *gracefully* to the nearest covered shape.
  This is a genuine advantage of prototype matching over a network that has
  simply never encountered the shape.
- Per-word confidence that means something, which `ocrs` cannot offer at all.

**Measured against a third party for the first time on 2026-09-22, and the
office-scan bullet is not met.** 29 hand-checked 300 DPI scans, scored by
`scribeocr/ocr-benchmark` — their pages, their truth, their metric, their code,
validated first by reproducing the figures they publish for two engines nobody
here tuned (Tesseract.js 84.76%, Scribe.js 93.65%, both exact). OCRcer scores
**43.40%** on their statistic, which is a box-matched word recall.

Read against the bullets above rather than as a single number: on the family
closest to what "clean 300 DPI office scans" describes, this engine's own CER
is 5.744% on SEC filings and 11.895% on single-column prose — 94.3% and 88.1%
character accuracy against a projected 95–98%. So the projection is close on
the cleanest real input and not yet reached on any of it.

The aggregate is far worse than either, and for a reason that is not about
recognition: word precision is **15.1%** at recall 44.8%, because a 1-NN
matcher with no reject stage turns every chart tick, cell rule and logo
fragment into a character. `table` scores 168.7% CER and `slide` 453.9% on the
same bank the filings read at 5.7%. That gap is a missing pipeline stage, not
a missing projection, and it is `ARCHITECTURE.md` §9's to close;
`ARCHITECTURE.md` §8.2 now records that the generated corpus cannot measure it
and what instrument would.


**Measured against a modern trained recogniser, 2026-09-27: the first bullet
is not met either, and the gap is not small.** PaddleOCR (PP-OCRv6 default
detection and recognition, CPU, Apache-2.0, used as a baseline only) was run
on OCRcer's own scoring pages, the ones the project was built to win.

| corpus | OCRcer CER | PaddleOCR CER |
|---|---|---|
| synthetic digital-born pages, including CAD `drawing` and `technical` | 5.32% | 0.71% |
| real financial filings | 11.34% | 4.98% |

PaddleOCR is ahead in every category and on every metric.

- The corpus biases favour OCRcer: the pages are confined to its charset,
  and they are clean, not scanned.
- Whether Paddle's training data overlaps these pages is unknown.
- Report: `docs/measurements/2026-09-27_paddle_h2h.md`.

This does not touch the per-word confidence bullet or the licence, size and
wasm posture. It does falsify §6's summary that "the engine is very likely to work well on
clean print and CAD text", when "well" means competitive with a current
trained recogniser.

§6 condition 3 still names `ocrs`, not PaddleOCR, as the bar for success.
That comparison has still not been run. See `ARCHITECTURE.md` §11,
2026-09-27.

**Expected to be worse than the alternative**

- Degraded scans, heavy noise, sub-150 DPI, fax-grade: **85–93%**, and this is
  where a fitted CNN wins. Expect the gap against `ocrs` to be widest here, and
  expect it to be the focus of every tuning round.
- Handwriting: **not supported**, not planned.
- Scene text — photographs, perspective, curved baselines: out of scope; `ocrs`
  will beat it and should.
- Non-Latin scripts: out of scope for v1. The design extends by adding
  prototypes, which is a script run rather than a research project — a real
  advantage over retraining, and one that holds only as far as a
  licence-clean face covering the script exists to render from. For most
  scripts Noto does. See the coverage note under *largest technical risk*
  below for the case where nothing does.

**Speed** is projected as a strength, and is the one projection in this
section with evidence against it. Candidate pruning by hole count reduces
matching, and the whole pipeline is integer image operations plus a few
hundred thousand multiplies per page. (Aspect-band pruning was named here
too; it was measured on 2026-09-21, lost accuracy in both forms tried, and is
disabled — `ARCHITECTURE.md` §4.1 step 2.) Projected well under a second per
page single-threaded — **faster** than the neural design it replaces.

**Partly checked 2026-09-22, and the comparative half does not hold up.** Ten
head-to-head runs put OCRcer between 0.26 and 0.49 seconds per page, so the
absolute projection holds easily; but OCRcer is faster than `ocrs` in only four
of the ten, and at the bank every current accuracy figure is quoted from it is
**slower** — 277.2 s against 250.8 s over 625 pages — while doing less work,
since it was handed segmentation. Page cost rises close to linearly with bank
size (0.31 s/page at 14,832 prototypes, 0.49 at 22,248) where `ocrs` is flat at
0.35–0.42 whatever it is given, so **prototype count is a speed budget as well
as an accuracy lever** — and the bank came in 47% above the count this
projection assumed. None of the ten is a
controlled timing run, so this is evidence rather than a measurement, and it
is recorded because the claim was pointing the other way. `ARCHITECTURE.md`
§4.1.

**The largest technical risk is font coverage.** If a document's typeface is far
from anything in the bank, accuracy falls off. The mitigation is cheap and
direct: the bank is minutes to rebuild, so a coverage failure found in
benchmarking is fixed by adding families and re-running a script. This is a much
better risk to own than the synthetic-to-real gap a trained design would have
carried, because it is diagnosable — a bad match reports low confidence and
names the character it struggled with.

**The mitigation has now been tested once, and it half held.** A per-class
coverage audit on 2026-09-22 found 177 of 187 classes drawn by all eighteen
licence-clean faces, and one class — `⌀` U+2300, the diameter sign — drawn by
**two**. Adding a family is indeed a script run; what is not guaranteed is that
a family exists to add. Of the twelve families on the build machine that draw
that glyph, six are CAD-vendor proprietary and three are Microsoft's, leaving
one clean candidate not already in the bank, covering 6 of 187 classes. So the
real shape of this risk is narrower and sharper than the paragraph above: it is
not *faces the bank has not got round to*, it is *glyphs whose only good
exemplars are proprietary*. Where that happens the fallback is not another
font, it is authoring the glyph — which is what the ISO 3098 technical face in
`ocrcer-build` is for. `ARCHITECTURE.md` §11, 2026-09-22.

---

## 6. Verdict

**Feasible, staged across nine chunks, roughly 9.1 M tokens**, paced so no
chunk exceeds 10% of a weekly budget. At one to two chunks per week that is
four to eight weeks. `docs/PLAN.md` holds the staging — and now holds three
further chunks, added after this assessment at operator direction, covering
accounting-document structure, drawing primitives and evaluation corpora for
roughly **3.4 M tokens** more. Those are scope the verdict above did not
assess; every figure in both sets is a planning projection per `PLAN.md`
section 1, not a measurement.

Three conditions attach:

1. **The domain stays narrow.** This design wins on printed documents and CAD
   drawings. Scope creep toward handwriting or scene text converts a tractable
   project into an intractable one.
2. **Accuracy is measured, not asserted.** Section 5 is a set of projections
   from how this class of engine has historically performed. The benchmark chunk
   exists to replace them with numbers, including the unflattering ones.
3. **Success is measured against `ocrs` on pdfcer's own corpus**, not against
   published figures on public scene-text sets. The survey notes no such
   comparison exists anywhere; building the harness that produces it is valuable
   independently of what it concludes.

The honest summary: the engine is very likely to work well on clean print and
CAD text, likely to lose to `ocrs` on degraded scans, and the whole thing is
cheap enough to rebuild that tuning is a real option rather than an aspiration.
