# Resume here

Paused on 2026-09-24, checkpoint refresh. This note describes the present,
not the history. The history lives in `docs/ARCHITECTURE.md` section 11
and in `docs/measurements/`.

**Public since 2026-09-24: `github.com/KenM76/ocrcer`, MIT, master only.**
Every push still needs Ken's own go, every time — not a standing default
like the commit rule below. **pdfcer vendors this repo's local HEAD**
(`tools/sync-ocrcer.py`), not the GitHub copy or a pinned release, so
**`master` must stay releasable at every commit** — ungated `ocrcer-core`
or adapter changes stay on a branch until they pass.

**Under git since 2026-09-23.** Branch `master`, no remote conflicts to
manage (push is gated, not blocked). `.gitattributes` pins LF everywhere.
**Commit after each passing change from here forward.**

---

## 1. State of the tree

Three operator directives landed today and reopened scope beyond what the
prior checkpoint tracked: (i) the model may be trained (supersedes the
2026-09-18 "constructed, never fitted" rule — provenance labels, the
train/score firewall and `ocrcer-core`'s invariants all survive
unchanged); (ii) build a neural glyph classifier as a second matcher
(chunk 15); (iii) build a self-contained LLM rescoring add-on, `ocrcer-llm`
(chunk 16), Qwen-family, one `.ocrl` file, no network, no server. Full
contracts: `ARCHITECTURE.md` §11, six entries from "Operator: the model may
be trained" through "Chunk 16b rescoring: spec details fixed."

**pdfcer integration no longer waits on beating `ocrs`.** Operator,
verbatim: "don't worry about beating the current ocr before building the
other things needed for integration into pdfcer." The binding (chunk 7)
is merged; `ocrs` stays the default engine until the head-to-head says
otherwise.

**NEW CONTROLS — beat these on both corpora at the shipped config (split
gate 1.09, underline strip on, `merge_overlap_frac` 0.4,
`baseline_split_valley_margin` 0.3, checkbox border-coverage on, italic
gating on, `char_bonus_slanted` = 3.44 neutral):**

| | end-to-end CER | line-matched CER |
|---|---|---|
| `finfilings` (60 real pages) | **12.141%*** | **11.098%*** |
| `pages-cov` (625 synthetic pages) | byte-identical at italic-gating control | — |

\* From the `char_bonus_slanted` sweep at 4.5 (`ARCHITECTURE.md` §11);
that value was **not shipped** (train/score firewall — picking a winner on
finfilings scoring data is exactly what the operator's own new fitting
rule forbids). The **shipped** figures are whatever the italic-gating ship
measured at the neutral 3.44 — not independently re-extracted by this
filing; read `docs/measurements/2026-09-24_italic_gating.txt` directly
before quoting a control number in the next session.

**Dense-page matching speed fixed, alpha blocker for pdfcer.** A second,
exact cross-class early-abandon ceiling in `nearest()`
(`crates/ocrcer-core/src/match.rs`) cut wall time **1.40–1.51x on three
profiled dense `finfilings` pages** (merge `f7757de`); output
byte-identical to the unoptimised matcher on both corpora (diff exit 0).
Full-corpus wall times were measured on a shared machine and are flagged
**indicative only, not a reading** — a pinned-core rerun is still owed.
Full detail: `docs/measurements/2026-09-24_dense_page_speed.md`.

**Chunk 16a (LLM engine) is accepted for correctness, not for speed.**
Tokenizer and logits match reference `transformers` output; Q8 keeps
96.8–97.3% top-1 agreement. **Speed measured at 3–4 tok/s at 20
threads — too slow to rescore a real page.** A speed step (persistent
thread pool, blocked matmul, restricted `lm_head`) is required before 16b
and exists, unmerged, on branch `llm-speed`.

**Five branches exist locally, none merged, none pushed** — see section 2.

---

## 2. Start here — next-up queue, in order

1. **Resume the `fit-12b` tuning campaign — waiting on Ken.** Chunk 12's
   coordinate-descent fit, `finfilings-train` only. Tier 1 (decode
   weights) accepted at confirm scale (21.823 vs 22.091). Tier 2 (line
   params) inner sweeps tentatively chose `descender_fraction` 0.17 and
   `descender_reach_fraction` 0.55; **tier-2 confirm not yet run.** `w_lex`
   reverted 0.35→0.6 on resume, pending a confirm-scale A/B. **Killed
   three times today by the memory-pressure reaper** (once from concurrent
   real-weights LLM oracle tests at ~7 GB, twice from general pressure —
   the campaign itself is ~70 MB). Do not restart without Ken's go;
   consider a detached process to escape the reaper when he does.
2. **`llm-speed` branch — finish and merge.** 16a-speed (persistent pool +
   blocked kernel) and 16a-speed2 (batched `score_candidates`) exist,
   commits `1a21c1f`/`3e6507d`/`fac7b34`. Pinned single-thread readings:
   qwen2.5-0.5b Q8 prefill 1.58→3.82 tok/s, decode 1.57→2.81 tok/s. Batched
   path bit-identical to per-candidate in synthetic tests; **its own speed
   is unmeasured.** Pending before merge: a serial real-weights oracle run
   (`--test-threads=1`, never concurrent with a heavy job) plus pinned
   timings. The ~46 min / ~10 min chunk-16b projection in that branch's
   measurement doc is a **projection**, not yet confirmed.
3. **`nbest` branch — verify then merge.** `Engine::recognize_lines_nbest`,
   `decode_word = decode_word_nbest(..,1)`. Pending: byte-identical check
   against master's corpus output; oracle best-of-8 CER on
   finfilings-val, unmeasured.
4. **`case-geom` branch.** `decode.case_geom_penalty` default 0.0 (guess).
   Pending: a train-split sweep and its gates; needs rebasing onto
   `nbest` first (shared `decode_word` signature).
5. **Chunk 16b implementation**, once `llm-speed` merges. Spec is fixed
   (`ARCHITECTURE.md` §11, "Chunk 16b rescoring"): score
   `ocr_score + λ·llm_logprob + β·n_tokens`, both fitted on
   finfilings-train/confirmed on -val; previous line's chosen text as LLM
   prefix; 8-candidate n-best cap; a no-harm gate (confident lines
   byte-identical with the add-on on/off); fallback is a small
   char-level correction model if the measured gain is thin.
6. **SROIE licence — waiting on Ken.** CC-BY-4.0 shown on mirrors may be
   the competition paper's licence, not the dataset's — unverified at
   source. SROIE rows held out of any fitting or bank build until cleared.
7. Everything from the pre-existing chunk-9 queue is still open and
   unchanged in kind — see `ROADMAP.md`'s "In progress" chunk 9 section
   for the full list (italic char_bonus lever exhausted on r000583, the
   dense-table trace, ligature error share, `baseline_split_sep`/`support`
   sweeps, `rule_aspect` re-measurement, the `ocrs` head-to-head, the
   `rustfmt` pass — awaiting Ken's go).
8. Research leads for `ocrcer-architect` to schedule or decline, not yet
   chunk-numbered commitments: a Tesseract-style per-page adaptive
   classifier (candidate chunk 13b); vertical/rotated CAD text (candidate
   13c, in scope); x-height rescaling before Sauvola.

**Run LLM oracle tests with `--test-threads=1`, never alongside a fitting
campaign.** This is why `fit-12b` died today. A reaped job restarts only
on Ken's say-so.

**Corpora:** `finfilings` at
`D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings`; `bench/pages-cov`.
Full-corpus sweeps run in the foreground, one arm at a time, never beside
another heavy job — this is now doubly true with LLM oracle tests in the
mix.

**Working mode unchanged:** fix, then research, then the next fix it
surfaces, each gated and shipped or explicitly rejected. Commit after
every passing change, locally; push only on Ken's explicit go. Replies to
Ken are TL;DR — detail belongs in these docs.

---

## 3. Where things landed today (2026-09-24)

Checkbox drop shipped; `max_splits` and cut-candidate generation both
ruled out on `r000583`; a per-letter autopsy found it italic; italic
prototypes tried, broke `pages-cov`'s drawing category, reverted; **slant
gating shipped instead** (`layout.italic_gating` 1); `char_bonus_slanted`
mechanism kept, sweep-picked value not shipped (firewall). Then, same day:
training approved; a neural classifier contract written (chunk 15); the
LLM add-on contract written and its 16a engine built, verified for
correctness, found too slow (chunk 16); the pdfcer binding merged,
published to GitHub, and vendored by pdfcer from local HEAD; integration
priority flipped ahead of beating `ocrs`; dense-page matching speed fixed
as an alpha blocker. Full narrative, all of it: `ARCHITECTURE.md` §11 and
`ROADMAP.md`'s "In progress" section, chunk by chunk, not restated here.

---

## 4. Constraints that bind the next session

* **Charset, feature vector and normalisation are frozen.** Unchanged.
* **`ARCHITECTURE.md` section 11 is append-only.** Supersede with a new
  entry and a forward pointer; the old text stays. This includes the
  2026-09-18 "constructed, not fitted" entry — it stands as history even
  though the rule it stated no longer governs.
* **A projection is labelled a projection; a reading is labelled a
  reading.** The chunk-16a `.ocrl` sizes and speed figures, the `fit-12b`
  inner-sweep numbers, and the dense-page full-corpus wall times are all
  filed with that label attached in their source documents — carry the
  label forward, don't drop it when re-quoting.
* **Train and score never touch.** New this session, load-bearing: no
  value tuned by comparing candidates on `finfilings` or `pages-cov` may
  ship (see `char_bonus_slanted` above for the concrete refusal). Fitting
  happens on the committed training split only.
* **`master` must stay releasable at every commit** — pdfcer vendors HEAD
  directly. New this session.
* **Every push needs Ken's own go, every time.** New this session; not a
  standing default the way commits are.
* **Gate any cut-search width, margin, prototype-bank or matcher change on
  both corpora and on wall time**, not just accuracy. Unchanged.

---

## 5. Waiting on Ken

* **Resume the `fit-12b` tuning campaign** — reaped three times today; not
  restarted without his go.
* **SROIE licence clearance** — unverified at source; held out of
  fitting/bank building until cleared.
* **Each push to `github.com/KenM76/ocrcer`** — needs his go every time.
* **A one-time `rustfmt` pass** across the drifted files (no
  `rustfmt.toml` committed). Carried forward, unresolved.
* **Does "commit after each passing change" extend to Ken's other project
  trees?** Carried forward, unresolved.
* **ALTO/hOCR underline-formatting output** — direction given, not
  scheduled as a chunk.
* **`osifont`'s GPL font exception** — unresolved; face not in the bank.
* **Token spend against `/usage`** — unmeasured for eight sessions running
  now; no shell has been available to any librarian dispatch since the
  calibration debt was first noted. The next session with shell access
  should treat this as priority, not background.
