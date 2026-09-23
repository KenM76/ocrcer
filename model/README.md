# model/

Human-editable source tables for the OCRcer `.ocrw` model. `ocrcer-build`
compiles these into the binary container described in `ARCHITECTURE.md`
section 7; nothing here ships directly. Per rule 1 in the project
`CLAUDE.md`, every value in these files is either authored from knowledge or
computed by a deterministic script — never fitted.

## `charset.tsv`

The frozen class table: every character the engine can recognise, one per
line, tab-separated, with a single `#`-prefixed header naming the columns.
TSV rather than JSON so it greps and diffs cleanly.

**`index` is a frozen identifier.** It is the class id used everywhere else
in the system — the prototype bank, the confusion table, the runtime's
output. Once a row is committed, its `index` never changes meaning. New
classes may only be **appended** at the end; a row is never renumbered,
reordered, or removed, because that would silently redefine what an existing
prototype or confusion rule refers to.

187 classes in three tiers, in file order: ASCII printable (`U+0021`–
`U+007E`, no space — word separation is a layout decision, not a
recognition class), engineering/typographic/currency symbols the CAD and
document domain needs, and Latin-1 accented letters for Western European
text.

`aspect_min`/`aspect_max` in this file are `aspect_source =
authored-provisional` — reasoned estimates, not measurements. Chunk 3
renders the actual prototype bank, measures real bounding-box ranges per
class, and rewrites that column to `measured`. Do not treat the provisional
values as anything more than pruning bounds good enough to bootstrap the
bank.

## What arrives later

- **Chunk 3** (computed tables): `feature_norm`, `prototypes`, `proto_index`
  — rendered from glyphs and measured, not authored. Also rewrites
  `charset.tsv`'s `aspect_source` column.
- **Chunk 4** (authored tables): `lexicon`, `bigrams`, `confusions`,
  `params`.

See `ARCHITECTURE.md` section 2 for what each table contains and why, and
`PLAN.md` for chunk staging.
