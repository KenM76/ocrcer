# Checkbox truth survey: does ground truth transcribe form checkboxes at all?

Read-only investigation. No `ocr.exe` run, no `cargo build`, no file edited
other than this one. Every count below was produced by a Python script
reading `.truth.json` files directly (the `"lines"` field, joined with `\n`)
and by direct visual inspection of `.pgm` page images opened with PIL and
viewed as PNG crops. No OCR logic was reimplemented; nothing here re-derives
what the engine does, only what the corpus contains.

## 1. Corpus-wide grep for box-like characters

**Counted by**: a script scanning every `*.truth.json` in both
`D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings` (60 pages) and
`D:/Dev/OCRcer/bench/pages-cov` (625 pages) — **685 truth files total** —
counting occurrences of each candidate character/pattern in the joined
`"lines"` text. Candidates: `☐ ☑ ☒ □ ■ ▢ ▪ ▫ ✓ ✔ ✗ ✘ ○ ● ◻ ◼ ◽ ◾`, every
codepoint in the Private-Use-Area Wingdings range `U+F000`–`U+F0FF`, and the
literal bracket patterns `[ ]`, `[X]`, `[x]`, `(X)`, `(x)`, `( )`.

**Result: zero genuine checkbox-glyph occurrences anywhere in 685 truth
files.** The only two candidate hits, both checked individually and both
false positives:

| Pattern found | Count | Pages | What it actually is |
|---|---|---|---|
| `●` (U+25CF) | 4 | `filing__r000011.truth.json` only | Bullet points in a press-release highlights list (`"●Fourth-quarter 2024 GAAP diluted EPS..."`) — this is also the page already named in `2026-09-23_worst_pages_round3.md` as a known-defective, overprinted-text page; unrelated to checkboxes. |
| `(x)` | 2 | `filing__r000561.truth.json`, `filing__r000627.truth.json` | Roman numeral list item ten — `"...business competition; (x) operational and reputational risks; (xi) technological change..."` — not a checkbox. |

No `☐ ☑ ☒ □ ■ ▢ ▪ ▫ ✓ ✔ ✗ ✘ ○ ◻ ◼ ◽ ◾` character, no PUA `U+F000`–`U+F0FF`
codepoint, and no `[ ]`/`[X]`/`[x]`/`(X)`/`( )` pattern appears in any of the
685 truth files. This includes `bench/pages-cov` specifically (0 hits across
all 625 files there) — that corpus is synthetic font-coverage pages and
carries no form checkboxes at all.

## 2. Direct visual inspection of four checkbox-bearing pages

**Counted by**: opening each `.pgm` at native resolution (1653×2339) with
PIL, downscaling 2x for viewing, reading the resulting PNG, and separately
printing the `"lines"` array from the matching `.truth.json` with line
indices, by hand-matching visible checkbox positions in the image to the
truth line immediately before/after them.

- **`filing__r000407`** (named in `2026-09-23_worst_pages_round3.md`,
  end-to-end CER 24.464%). **Observed**, image: every checkbox on the page
  (Yes/No pairs, the 9-item custody-type checklist, repeated per custodian
  record) is rendered as a rectangular outline containing a small "?" glyph
  centred inside the box — a real pixel shape, not a blank box. **Observed**,
  truth: line 10 is `"Fund or its investment adviser(s)?"`, line 11 is
  `"Yes"`, line 12 is `"No"` — there is no line, and no character within an
  adjacent line, standing in for the checkbox that sits visually between the
  question and the word "Yes". The checkbox contributes zero characters to
  truth.
- **`filing__r000396`** (round3, end-to-end CER 29.035%). **Observed**,
  image: identical "?"-in-box rendering on every Yes/No pair and every
  numbered-item checkbox (`i.`/`ii.`/`iii.`/... rows). **Observed**, truth:
  lines 0–7 alternate question text with bare `"Yes"`/`"No"` lines (line 2
  `"Yes"`, line 3 `"No"`, immediately after line 1's question, no
  intervening token); lines 10–17 give item labels (`"i. Revenue sharing
  split"`, `"ii. Non-revenue sharing"`, ..., `"N/A"`) with nothing for the
  checkbox that precedes each label in the image.
- **`filing__r000308`** (`2026-09-23_worst_pages_round2.md` section (b).3,
  end-to-end CER 34.45%). **Observed**, image: the same "?"-in-box rendering
  on the `1`/`2`/`3`/`N/A` fair-value-hierarchy checklist and on every
  Yes/No pair further down the page (`"Currently in default? [Y/N]"`,
  `"Are there any interest payments in arrears..."`, etc.). **Observed**,
  truth: lines 18–21 are the bare labels `"1"`, `"2"`, `"3"`, `"N/A"` with no
  preceding checkbox character, matching round2's own finding at the time
  ("four one-character lines... a genuinely complex micro-layout"),
  re-confirmed here directly against the image rather than taken on
  citation.
- **`filing__r000066`** (not previously named in a worst-pages document,
  selected here only because it is one of 30 finfilings pages carrying
  matched `"Yes"`/`"No"` truth lines — see the next paragraph). **Observed**,
  image: same "?"-in-box checkbox rendering throughout the securities-lending
  and repurchase-agreement question block, then transitions to an
  `NPORT-P: Part C` schedule with ordinary label/value fields (no
  checkboxes) for the rest of the visible page. **Observed**, truth: every
  Yes/No pair in this page's `"lines"` array is two consecutive bare lines
  (`"Yes"`, `"No"`) with the checkbox omitted, same as the other three pages.

All four pages show the identical shape: a real, non-blank pixel glyph in
the source image (a "?" centred in a rectangle) at every checkbox position,
and precisely nothing — no character, no placeholder, no reserved codepoint
— for that glyph in the ground-truth text.

**Scale of the pattern in this corpus**: a script search for pages carrying
matched `"Yes"`/`"No"` truth-line pairs (a proxy for this checkbox-question
form layout) found **30 of the 60 finfilings pages** with the pattern (`Yes`
count equals `No` count, both ≥1, on the same page) — consistent with
`2026-09-23_finfilings_audit.md`'s independent finding (by font-family
classification, a different method) that **35/60 finfilings pages** are
"NPORT-P investment-fund grid forms," and with that same audit's explicit,
separately-conducted check concluding the "?" icon is **"a legitimate
unfilled-checkbox UI element, not an artefact"** of rendering — i.e. the
box-with-"?" shape is the source PDF's actual design for an empty checkbox,
not a font-coverage gap or corpus defect.

## 3. Cross-check against prior `--raw`/`--worst` measurements

`2026-09-23_worst_pages_round3.md` already measured, on the unmodified
build, the exact confusion-table signature this omission predicts: on
`r000396`, `""->"®" 24`; on `r000407`, `""->"®" 17` (plus `""->"~" 6` and
`""->"B" 6`). In this notation the empty string on the left is the
**reference** (truth) side of the Levenshtein alignment — these are
insertions, not substitutions: truth has nothing at that position, and the
decoder's output has `®`/`~`/`B` there. That earlier document's own
characterisation — "a checkbox glyph rendered in the source... is not in the
charset and is being matched to the nearest available class" — is consistent
with what this survey confirms independently from the truth side: there is
no truth character for the box glyph to be *matched against* correctly,
because truth never contains one.

## Conclusion

Ground truth in this corpus **omits form checkboxes entirely** — not one of
685 truth files transcribes a checkbox with any character, placeholder, or
private-use codepoint, on any of the pages checked, including four pages
independently confirmed by direct pixel inspection to contain a real,
non-blank checkbox glyph (a "?" inside a box, which a companion audit
document separately established is the source PDF's genuine design, not a
rendering defect). The practical consequence for the not-text-vs-charset
decision this was run to inform: because truth has zero characters at every
checkbox position, **any** non-empty output at that position — including a
correctly-recognised, deliberately-added charset entry for the checkbox
shape — will register as a CER insertion every single time it fires; giving
the box glyph a charset entry could only change *which* wrong character gets
inserted, not eliminate the insertion. The `®`/`~`/`B` confusions are real
and correctly attributed to missing charset coverage, but the fix consistent
with what truth actually contains is to have segmentation/matching treat the
checkbox shape as non-text furniture to be dropped, not to teach the
charset a class for it.
