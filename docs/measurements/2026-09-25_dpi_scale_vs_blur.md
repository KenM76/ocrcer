# 300 dpi garbling: scale or blur? Aliasing, not blur.

MEASURED on one page, `D:\Dev\pdfcer\fixtures\synthetic\ocr\scan.pdf` /
`printed.pdf`, N = 47 distinct truth tokens (`GROUND_TRUTH.json`). Diagnostic
only — nothing here was fitted or tuned. Model
`D:\Dev\OCRcer\model\out\ocrcer.ocrw`, read-only, release build of `hocr` from
a throwaway worktree (`dpi-diag` branch). Scorer: a truth token counts as
matched if it appears verbatim as a whole recognised word anywhere on the
page (case- and punctuation-sensitive) — the same content-recall rule the
89.4%/97.9%/83.0% figures in `2026-09-24_pdfcer_smoke_misses.md` used.

The architect's prior finding: `scan.pdf`'s embedded `pdfceIm1.png` is
1700x2200 on a 612x792pt page — 200 dpi native. The smoke doc's 300 dpi run
was therefore reading an *upsampled* image, not native pixels.

## Results

| run | dpi / scale | x-height (px, median) | word % | garbled/missed |
|---|---|---|---|---|
| a_native | 200 dpi, untouched embedded image | 19 | 97.9% (46/47) | sees→Sees (case) |
| b_nn150 | (a) x1.5 nearest-neighbour (scale, no blur) | 29 | **87.2% (41/47)** | project→**projæt**, quality→qua11ty, over→Over, should→Should, sees→Sees, scanned missing |
| c_bilinear150 | (a) x1.5 bilinear (scale + blur) | 28-29 | 97.9% (46/47) | below→beiow (l→i) |
| d_scan_150 | pdfium2 render scan.pdf @150 (downsample) | 14 | 91.5% (43/47) | cannot→cannol, eye→eya, anyone, where |
| d_scan_200 | pdfium2 render scan.pdf @200 (native, no resample) | 19 | 97.9% (46/47) | sees→Sees |
| d_scan_300 | pdfium2 render scan.pdf @300 (upsample from 200dpi image) | 29 | 93.6% (44/47) | Recognition→**Recognítion**, cannot→Cannot |
| d_scan_400 | pdfium2 render scan.pdf @400 (upsample x2) | 38 | 93.6% (44/47) | over, sees, sleeping |
| e_printed_150 | pdfium2 render printed.pdf (vector, no upsample) @150 | 13-14 | 95.7% (45/47) | find, printed |
| e_printed_200 | vector @200 | 18 | 95.7% (45/47) | find, should |
| e_printed_300 | vector @300 (same x-height as b/c/d_scan_300) | 27 | 93.6% (44/47) | scanned, sees, should — **all case flips, no diacritic garbling** |
| e_printed_400 | vector @400 | 36 | 97.9% (46/47) | should |

`(d)` reproduces the same *direction* as the original pdfcer-path sweep
(dip at 150, peak at 200, dip at 300) but not the same magnitude (93.6% vs
83.0% at 300) — pypdfium2's raster scaler is evidently not byte-identical to
pdfcer's renderer, but it reproduces the qualitative cliff and, importantly,
the same *diacritic-shaped garbling signature* (`Recognítion`) the original
smoke doc reported (`Recogmtion`, `quaüty`, `anä`).

Binarized crops (`crops/{native,nn,bilinear}_{quality,project}.png`,
engine's own binarize+deskew, 3x nearest-neighbour zoom for viewing): native
and bilinear strokes are clean, solid, evenly separated. The nearest-neighbour
crop shows blocky/staircased edges and the gap between "c" and "t" in
"project" visibly narrowing — consistent with a spurious ink bridge merging
two components into the shape the matcher read as "æ". No hollowing (Sauvola
window too small) in any crop; the failure is edge aliasing, not thresholding.

## Verdict: aliasing, not blur — and not scale alone

The controlled pair (b) vs (c) holds source pixels and target scale (29px
x-height) identical and varies only the interpolation kernel. Nearest
(no blur) scores 10.7 points worse than bilinear (blur) and is the only one
of the two to produce diacritic-shaped garbling. **Blur suppresses the
failure; its absence causes it.** Cross-checking against (e): pure vector
text rendered fresh at the *same* x-height (27-29px), never upsampled from a
raster at all, shows zero diacritic garbling — its misses are exactly the
pre-existing, DPI-insensitive c/s case-flip confusion already documented in
`2026-09-24_pdfcer_smoke_misses.md`, unrelated to this question. So scale
alone (larger x-height) does not cause the diacritic garbling either.

**The 300 dpi cliff is driven by unsmoothed (aliased) upsampling of an
already-rasterized image specifically** — blocky pixel-replicated edges
create spurious ink bridges between adjacent strokes, which the matcher then
reads as ligature/diacritic-shaped glyphs. This is separate from, and
compounds with, the already-documented DPI-insensitive case-confusion issue,
which persists in every condition here including clean vector text.

## Lever

This points at `ocrcer-runtime`'s binarization/morphological-repair stage
per the existing risk-mitigation ladder (aliased edges from an upsampled
source are a harder input than either native-resolution or properly
antialiased/blurred input, and repair should target bridge-merges, not
holes). Whether the fix belongs upstream instead — pdfcer/OCRmyPDF choosing
a smoothing (bilinear/bicubic) resampler rather than a blockier one whenever
it must rasterize above an embedded image's native DPI — is a call-site/
format question outside this crate; flagged for `ocrcer-architect` rather
than decided here. One page, N=47 words: enough to identify the mechanism,
not enough to set a DPI recommendation or a repair threshold.
