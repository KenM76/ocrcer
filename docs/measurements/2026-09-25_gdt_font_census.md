# GD&T / hole-callout font coverage census (27 codepoints) — read-only

2026-09-25. Font files measured on this machine; no rendering, no engine run.

Step 2 of the charset-change protocol ("cheaper steps first",
`docs/measurements/2026-09-22_research_classical_techniques.md` addendum
"symbols drawings print that the charset cannot emit"). This is a coverage
census only: no charset, bank, model, params or repo file was touched.

Codepoints (27): U+23E4 U+23E5 U+25CB U+232D U+2312 U+2313 U+27C2 U+2220
U+2225 U+2316 U+25CE U+232F U+2197 U+2330 U+24BB U+24C1 U+24C2 U+24C5 U+24C8
U+24C9 U+2334 U+2335 U+2332 U+2333 U+2331 U+21A7 U+25A1.

## Method (rerun with this exact script)

Script: `tools/gdt_font_census.py` (search roots via `--roots`; the defaults
are the four below).

- Same roots as the 2026-09-22 U+2300 audit: `C:\Windows\Fonts`, the
  per-user font dir, and the Program Files / Program Files (x86) trees that
  hold the application-bundled fonts (SOLIDWORKS Corp, Android Studio,
  Scribus, VcXsrv, Inkscape, Office, Ghostscript, Hopsan, etc). LibreOffice
  and Git's `share/fonts` are absent on this machine, same as before.
- **1,616 files measured** (1,596 fonts read OK across TTF/OTF/TTC/OTC; 2
  OneDrive telemetry files misnamed `.otc` were unreadable and skipped).
  fontTools `getBestCmap()` per codepoint, then a `RecordingPen` draw on the
  mapped glyph — a codepoint counts only when the glyph has ≥1 recorded
  outline segment, not merely a cmap entry (this audit's charter explicitly
  excludes .notdef-shaped empty glyphs, unlike the 2026-09-22 audit, which
  measured cmap only).
- Licence: read name IDs 0/8/13/14. A family already resolved in
  `model/fonts.tsv` or the 2026-09-22 audit is classified from that finding
  (`known-family`); otherwise the string is pattern-matched for OFL / Apache
  / public-domain / GPL / proprietary text; no match is `unclear`.
- **751 distinct families** were seen: 264 clean, 444 barred, 43 unclear.

Caveat: this is a static outline test, not a render — it says a shape exists
in the file, not that it looks like the intended GD&T symbol at drawing size.

## Per-codepoint clean/barred family counts (measured)

11 codepoints sit at the coverage floor — **clean=2, barred=3–9, unclear=0**,
the 2 clean faces always Noto Sans Symbols + STIX Two Math and no other:
U+23E4 straightness, U+23E5 flatness, U+232D cylindricity, U+2313 profile
(surface), U+27C2 perpendicularity, U+2316 position, U+232F symmetry,
U+2330 runout (total), U+2334 counterbore, U+2332 conical taper, U+2331
dimension origin. U+2335 countersink is the same floor plus one more
(NovaMono): clean=3, barred=6.

Above the floor:

| codepoint | meaning | clean | barred | unclear |
|---|---|--:|--:|--:|
| U+25CB | circularity | 44 | 112 | 2 |
| U+2312 | profile (line) | 28 | 33 | 0 |
| U+2220 | angularity | 32 | 82 | 0 |
| U+2225 | parallelism | 38 | 58 | 0 |
| U+25CE | concentricity | 38 | 39 | 0 |
| U+2197 | runout (circular) | 45 | 68 | 0 |
| U+24BB/24C1/24C2/24C5/24C9 | modifiers F/L/M/P/T | 28 | 19–20 | 0 |
| U+24C8 | S RFS | 29 | 19 | 0 |
| U+21A7 | depth arrow | 10 | 12 | 0 |
| U+25A1 | square | 52 | 105 | 2 |

**Zero-clean-coverage codepoints: none.** Every one of the 27 has at least 2
licence-clean covering faces — unlike U+2300, which had exactly 2 out of 18
*bank* faces and 3 families total on the machine. The floor here (2 clean
families) is carried entirely by **Noto Sans Symbols** and **STIX Two
Math**, both OFL-1.1, both of which draw **all 27 of 27** codepoints with
non-empty outlines.

**STIX Two Math is already a shipped bank face** (`model/fonts.tsv` row 55,
`eligible-present`/`shippable`). Its already-in-the-bank copy and the
canonical upstream copy fetched below agree on all 27 codepoints — so, per
this census alone, the bank already contains one face that draws every
symbol in this set. That is a coverage finding, not a decision to add the
classes; §3 of the source addendum still gates that on the probe step.

## Faces that cover the most codepoints

- **Noto Sans Symbols** (OFL-1.1) and **STIX Two Math** (OFL-1.1): 27/27
  each, both clean.
- Next tier, clean: the Noto CJK family (Sans/Serif, CJK HK/JP/KR/SC/TC and
  Mono CJK variants, all Apache-2.0/OFL) at 13/27 each — carried by CJK
  script needs, not GD&T-specific coverage.
- Barred faces that nearly match the two clean leaders: **Segoe UI Symbol**
  (Microsoft, 27/27), **DS ISO 1** (Dassault, 26/27), **Myriad CAD**
  (proprietary, 26/27), **Cambria / Cambria Math** (Microsoft, 17/27 each).
  This repeats the U+2300 pattern: the CAD-vendor and OS faces that draw
  these symbols most completely are exactly the ones rule 2 excludes.
- Other CAD-vendor faces measured: GENISO 10/27, GOST Common 3/27, ISOCPEUR
  3/27, ISOCTEUR 3/27, the AutoCAD `complex_`/`simplex_`/`AIGDT`/`AMGDT`
  shx-derived faces 0–1/27 (they draw almost none of this set — GD&T symbols
  are a Microsoft/Dassault/Autodesk-CAD-app-font strength here, not a
  generic shx-shape one).

## Upstream downloads (not on this machine as canonical copies)

Fetched read-only into `D:/Dev/ExcludedPrivate/ocrcer/fonts-census/`, not
added to the bank, not copied into the repo:

| file | source URL | version (nameID5) | sha256 | covers |
|---|---|---|---|---|
| `NotoSansSymbols[wght].ttf` | raw.githubusercontent.com/google/fonts/main/ofl/notosanssymbols/ | Version 2.003 | `f7e7e04b…128f7` | 19/27 (this variable-weight release omits U+25CB/U+2220/U+2225/U+2316/U+25CE/U+21A7/U+25A1 that the Android-bundled subset draws — the two Noto Sans Symbols copies are not interchangeable) |
| `NotoSansSymbols2-Regular.ttf` | same repo, `ofl/notosanssymbols2/` | Version 2.008 | `7d5fb73b…82a21` | 4/27 |
| `NotoSansMath-Regular.ttf` | same repo, `ofl/notosansmath/` | Version 3.000 | `3f495fe9…2c47d` | 13/27 |
| `STIXTwoMath-Regular.ttf` | same repo, `ofl/stixtwomath/` | Version 2.12 b168a | `562551b1…936de` | 27/27 (matches the bank's already-installed copy) |
| `dejavu-sans-ttf-2.37.zip` | github.com/dejavu-fonts/dejavu-fonts release `version_2_37` | 2.37 | `5c6e497a…9822fd7` | 8/27 (`DejaVuSans.ttf` inside, sha256 `7da195a7…848954`) |

All five carry an explicit OFL/Bitstream-Vera licence statement in nameID13/14
(quoted in full for DejaVu Sans, the SIL OFL boilerplate for the other four).

**Correction to a locally-bundled assumption:** the Android-Studio-bundled
`NotoSansSymbols-Regular-Subsetted.ttf` that the 2026-09-22 audit flagged as
provenance-suspect draws a *different* 19 of the 27 than the canonical
Google Fonts static release does (both cover 19/27, not the same 19 — the
canonical build is missing U+25CB/U+2220/U+2225/U+2316/U+25CE/U+21A7/U+25A1
that the Android subset has). Which release build of Noto Sans Symbols is
taken changes its coverage number.

## Authored ISO 3098 face

Grepped `crates/ocrcer-build/src/face/` (all of `glyphs/{digits,lower,upper,
punct,symbols,accents}.rs`) for all 27 codepoints: **zero matches.** The
authored face draws the ~187 charset classes by construction and none of
these 27 is a charset class, so this is expected, not a gap in the authored
face — consistent with the source addendum's finding that none of the 27 is
in `model/charset.tsv`.

## Licence questions for the operator

1. **Phosphor** (`phosphor-1.3.0.ttf`, user font dir) is explicitly MIT
   (nameID13). MIT is not one of CLAUDE.md rule 2's three named licences
   (OFL, Apache, public domain) nor one of this task's four labels; reported
   `unclear` here rather than assumed in.
2. **`qvPDF.ttf`** (`C:\Windows\Fonts`) carries no licence/copyright text at
   all beyond "Generated by Fontographer 4.1" — genuinely unclear.
3. **DejaVu Sans**: this task's own rubric lists "Bitstream Vera/DejaVu-style
   permissive" under `clean`, so it is counted clean above (8/27). But
   `model/fonts.tsv`'s existing row for it calls the same licence "outside
   CLAUDE.md rule 2 as written" and treats it as an operator call, not an
   assumption. The two documents disagree on this family; flagging rather
   than resolving it.

## Architect review (2026-09-25)

- **Accepted as a coverage reading.** It measures whether an outline exists,
  not whether the shape reads as the GD&T symbol at drawing size.
- **Two canonical clean faces, not one, for position.** The second clean
  face at the coverage floor is Noto Sans Symbols. For U+2316 (position),
  only the provenance-suspect Android subset draws it. The canonical Google
  Fonts release does not. Checked here on the four downloads (fontTools,
  cmap plus a non-empty outline): Noto Sans Symbols 2 and STIX Two Math
  draw U+2316; Noto Sans Math and canonical Noto Sans Symbols do not.
  Canonical coverage of position therefore stands at two faces. The other
  ten floor codepoints are drawn by canonical Noto Sans Symbols. Two faces
  per class is still thin, the same exposure the U+2300 audit found. That
  should be weighed if the probe (addendum step 1) ever justifies the
  charset change (step 3).
- **None of the three licence questions needs an answer for this decision.**
  Two OFL faces cover all 27 codepoints.
  - The DejaVu disagreement is the dispatcher's error. The census rubric
    called Bitstream Vera/DejaVu clean. `model/fonts.tsv` governs: it is an
    operator call. No conclusion above depends on it, because DejaVu is
    never at the floor.
  - Phosphor (MIT) and `qvPDF.ttf` are recorded as unclear and left out.
- **Next.** The callout probe (addendum step 1) stays on hold under the
  operator's accounting-first priority (ROADMAP, 2026-09-21). No class is
  added.
