# fixtures/

Golden fixtures for `ocrcer-bench`. No second implementation exists to check
`ocrcer-core` against, so correctness is carried entirely by these
known-good expectations. See `docs/ARCHITECTURE.md` section 8.2.

## Layout

```
fixtures/
  glyphs/<name>.pbm            ASCII PBM bitmap, input
  glyphs/<name>.meta.json      baseline/x-height + character, input
  expected/glyphs/<name>.feature.json   golden feature vector, output

  decode/<name>.lattice.json            authored word lattice, input
  expected/decode/<name>.decode.json    golden reading, output
```

Page-level fixtures (`fixtures/pages/...`) are chunk 2 and do not exist yet.
A fixture is discovered by the union of names across all three locations —
if any one file is deleted, the fixture still shows up (by name) with a
named failure for the missing file, rather than silently dropping out of
the run.

### Declared line metric

The glyph seed set stands on one declared line, not per-glyph guesses:
**x_height = 14px, cap/ascender height = 20px, descender depth = 5px.**
Every `.meta.json` in `fixtures/glyphs/` carries `x_height: 14.0` — it is a
property of the line, not a knob per glyph. `baseline_dy` varies per glyph
and is measured downward from the bitmap's top row to the baseline; for a
glyph resting on the baseline it equals the bitmap height, and for a
descender (`glyph_p`) it is less than the height because the bbox extends
below the baseline.

Every `.pbm` bounding box is **tight**: no fully-blank row or column on any
edge. A padded box is a shape the runtime's connected-component labeller
will never actually produce, and padding is not neutral — it shifts the
centroid, the normalisation scale, the aspect ratio, ink fraction, and both
projection profiles. A fixture built on a padded box measures the extractor
on an input that cannot occur.

When adding a fixture, place it on this same line (reuse `x_height: 14.0`)
unless you are deliberately adding a second declared line (a new metric
requires ocrcer-architect sign-off, same as any other cross-fixture
semantic change).

## The decode stage

A decode fixture is an **authored** lattice, never one captured from a page.
A captured lattice would make the fixture a record of what the matcher
returned on the day it was captured, and every bank rebuild would demand a
re-bless of a file nobody could check by reading. An authored one has ground
truth in the sense section 8.2 means: the distances say plainly which reading
the image favoured, so the right answer is derivable from the fixture.

For the same reason the decoder runs with **no authored tables** — no
bigrams, no lexicon, no confusions. Those live in the model file and change
when it is rebuilt; a decode fixture that depended on them would fail for
reasons that have nothing to do with the decoder.

`positions` is a linear chain — one edge per character, every candidate listed
as `[character, distance]`. Two candidates at the *same* distance is how a
tie-break expectation is written, and tie-breaking is the reason this stage
needs fixtures at all: a tie broken the wrong way is right about half the time
and is invisible in any aggregate error rate. A chain cannot express a merge
or a split on purpose — those belong to the segmenter's own stage boundary.

Class indices are the fixture's **own** sorted character set, not the shipped
charset's, so inserting a class into `charset.tsv` cannot silently change what
a decode fixture means.

`params` states the values the fixture was authored against, overriding
`Params::DEFAULT` by `model/params.tsv`'s key names. A fixture that inherited
whatever the file carries this week would fail every time a threshold moved.

## File formats

**`.pbm`** — ASCII PBM (`P1`). `P1\n<width> <height>\n<0/1 bits, row-major>`.
Chosen because it needs no image decoder and is human-diffable in a normal
text diff.

**`.meta.json`** — `{"character": "l", "baseline_dy": 20.0, "x_height": 20.0}`.
Ordinary `serde_json`; these are hand-authored, not subject to the bit-exact
rule below.

**`.feature.json`** — `{"fixture": "...", "stage": "feature", "values": [...]}`.
The `values` floats are written with Rust's default `{}` (`Display`)
formatting, which round-trips to the identical bit pattern via `FromStr`.
This file is **not** produced or parsed by `serde_json`'s float path —
that formatter isn't contractually pinned to match `std`'s `{}`, and a
silent formatting drift there would be indistinguishable from a real
regression. Comparison is by `f32::to_bits()`, not `==`, so `-0.0` and
`0.0` are treated as different values. There is no tolerance anywhere in
this harness: `ARCHITECTURE.md` 3.1 guarantees the extractor avoids
transcendentals, so `expected == actual` bit-for-bit is the correct
standard, not an approximation of one.

## Running the harness

```
cargo run -p ocrcer-bench                       # against fixtures/
cargo run -p ocrcer-bench -- --fixtures-dir P    # against another directory
cargo test -p ocrcer-bench                       # unit + perturbation tests
```

Exit code is non-zero on any mismatch. Failure messages name the fixture,
the stage, the field, and — for a value mismatch — the first differing
index with both values and their bit patterns.

## Blessing (regenerating expectations)

```
cargo run -p ocrcer-bench --bin bless                          # dry run: prints diff, writes nothing
cargo run -p ocrcer-bench --bin bless -- --yes                  # writes, if exactly one stage changed
cargo run -p ocrcer-bench --bin bless -- --yes --allow-multi-stage
```

Blessing is deliberately awkward and is never reachable from `cargo test`.
`--yes` is required to write anything; without it you get a dry-run diff.
A plan touching more than one stage boundary is refused even with `--yes`
unless `--allow-multi-stage` is also given — that shape of change needs
`ocrcer-architect` review, not a single command (`ARCHITECTURE.md` 8.2,
`PLAN.md` section 4). **Read the printed diff before blessing.** A fixture
blessed without being read defeats the point of having it.

## Adding a fixture

1. Add `glyphs/<name>.pbm` and `glyphs/<name>.meta.json`.
2. `cargo run -p ocrcer-bench --bin bless` (dry run) to see what would be
   generated, then `-- --yes` to write `expected/glyphs/<name>.feature.json`.
3. **Read the generated expectation** before committing — sanity-check
   hole count, zone densities, whatever is relevant to why the fixture
   exists. Never hand-author a float into an expectation file.
4. Run `cargo test -p ocrcer-bench` to confirm it passes and nothing else
   regressed.
