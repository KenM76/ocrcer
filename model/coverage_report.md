# Font coverage sweep, eligible-present faces vs. the 187-class charset

Measured 2026-09-18 against `model/charset.tsv` (187 classes) and the 19 rows
in `model/fonts.tsv` with `status = eligible-present`. Method: read each
face's file at its `path` column with `fontTools`, resolve each class's
codepoint through `getBestCmap()`, and count it present only if the mapped
glyph is not `.notdef` and has a non-empty outline (TrueType `glyf`:
`numberOfContours > 0` or, for a composite, at least one component;
OpenType CFF: the charstring draws a non-empty bounds). All 19 face files
opened without error; no path was missing or unreadable.

Excluded from this sweep, per the task's eligibility filter: 3
`needs-operator` rows (osifont, Terminus Font, DejaVu Sans), 11
`ineligible` rows (Arial and the rest of the proprietary Windows set, the
AutoCAD shx-derived faces, the Nimbus/URW AFPL set), and 1 `eligible-absent`
row (norm-stroke, not present on this machine). None of those 15 rows
contributed to any count below.

## Zero-coverage classes

**None.** Every one of the 187 charset classes has at least one
eligible-present face with a real glyph for it. There is no codepoint this
prototype bank would be blind to, given the current 19-face inventory.

## Thin-coverage classes (n_faces = 1 or 2)

One class, and it is at 2, not 1:

- **index 95, U+2300 `⌀` (diameter sign, category symbol)** — covered by
  **Fira Code** and **STIX Two Math** only. Every other eligible-present
  face is missing it (this matches the per-face gap notes already recorded
  in `fonts.tsv` — most of the sans/serif/mono faces here list "missing
  U+2300 diameter sign" as a known gap).

No class in this sweep has `n_faces = 1`. The diameter sign is the single
thinnest point in the current inventory, and it still has two independent
faces behind it, not one.

## Distribution

| n_faces | classes |
|---|---|
| 0 | 0 |
| 1 | 0 |
| 2 | 1 |
| 3–5 | 0 |
| 6–10 | 0 |
| 11–18 | 9 |
| 19 (all) | 177 |

The 9 classes in the 11–18 band split as 6 at 17 faces (U+221A `√`, U+2264
`≤`, U+2265 `≥`, U+2248 `≈`, U+2260 `≠`, U+03A9 `Ω`) and 3 at 18 faces
(U+2030 `‰`, U+2013 `–`, U+2014 `—`). All nine are covered by every face
except some combination of Noto Sans, Lato, and Inconsolata missing that
particular symbol — consistent with those three faces' documented gaps in
`fonts.tsv`.

## Per-face contribution

| Face | Classes covered (of 187) | Uniquely load-bearing |
|---|---|---|
| Liberation Sans | 186 | 0 |
| Liberation Serif | 186 | 0 |
| Liberation Mono | 186 | 0 |
| Noto Sans | 181 | 0 |
| Noto Serif | 186 | 0 |
| Open Sans Condensed | 186 | 0 |
| Roboto | 186 | 0 |
| Roboto Condensed | 186 | 0 |
| Roboto Mono | 186 | 0 |
| Lato | 185 | 0 |
| PT Sans | 186 | 0 |
| PT Mono | 186 | 0 |
| Inconsolata | 177 | 0 |
| Fira Code | 187 | 0 |
| Inter | 186 | 0 |
| JetBrains Mono | 186 | 0 |
| Cascadia Code | 186 | 0 |
| Cascadia Mono | 186 | 0 |
| STIX Two Math | 187 | 0 |

**No face is uniquely load-bearing for any class.** The floor of the
distribution is 2 (U+2300), never 1, so dropping any single face from the
19 — including Fira Code or STIX Two Math, the two faces behind the
diameter sign — would not create a zero-coverage class on its own; it would
only turn U+2300 into a genuine single-point-of-failure class. That is the
nearest thing to a risk this sweep found, and it is one step removed, not
present today.

## Method and its limits

This measured **presence**, not quality: whether a face's `cmap` resolves a
codepoint to a glyph with any drawable outline at all. It does not measure
whether that glyph is a *good* prototype — visually distinct from its
class's confusable neighbours (per `charset.tsv`'s `notes` column: 0/O,
1/l/I/|, 5/S, 8/B, rn/m, and the accented-letter case-twin pairs), correctly
hinted, or even legible at the bank's render size. A face can contain a
character and draw it badly, or draw a technically-non-empty outline that a
human would call a rendering glitch; this sweep cannot see either. It also
did not check style variants (Bold/Italic) — only the `Regular`/base style
row listed for each face in `fonts.tsv`.

The charset (`model/charset.tsv`) has no whitespace-category class and does
not include U+0020 — confirmed directly from the file, not assumed — so the
`.notdef`-vs-legitimately-empty-glyph special case named in the task
instructions did not arise anywhere in this sweep.
