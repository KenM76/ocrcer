# tools/fit12b

The chunk-12b fitting campaign that produced `model/params.tsv`'s `fitted`
rows (`docs/ARCHITECTURE.md` §11, "How chunk 12b's vector is chosen" and
"Chunk 12b's vector: a move made on a tie is reverted before the
ablations"). These are the scripts as they actually ran on 2026-09-24/25,
parametrised so they no longer hard-code a machine's paths. **No sweep,
tier, confirm or decision logic was changed** — every edit is either an
`argparse` wrapper around previously-module-level path constants, or the
minimum restructuring needed to defer path resolution until after argument
parsing (see the unified diffs against the as-run copies, saved alongside
this fold's report). All runs used `finfilings-train` only; val ran exactly
once, at the end, against `finfilings-val`.

**A fresh run of these scripts today would not retrace the search.** They
are committed as a record of what produced the fold, not as a reproducible
pipeline to rerun; a rerun would start from a different (post-fold) baseline
and would not reproduce `campaign_log.jsonl` or `decision_12b.json` bit for
bit even with identical flags, because the campaign is stateful across a
kill/resume/restart history that is itself part of the record.

## Scripts, in run order

- **`campaign.py`** — the driver. Defines `TIERS` (tier1_decode,
  tier2_lines, tier3_words_segment, tier4_slant), `COST_KNOBS`, `EPS=0.02`,
  and the coordinate-descent sweep (`sweep_param`, `run_tier`,
  `run_cost_knobs`). Every other script here imports it as a module and
  reuses its `run()`/`expand()`/status machinery. Paths come from
  `configure(args)`, called after `build_arg_parser().parse_args()`.
- **`campaign_resume.py`** — resumes after the first (12:04) memory-pressure
  kill. Carries the architect-reviewed tie-break fix (a candidate only
  replaces the current value if it beats it by more than EPS; `campaign.py`
  itself picked the first/lowest value even on exact CER ties, which is the
  `decode.seg_split_penalty` tie later reverted per the second §11 entry).
  Reverts `decode.w_lex` to 0.6, resumes `tier2_lines` mid-tier.
- **`campaign_resume2.py`** — resumes after a second memory kill (14:10),
  mid-way through `tier2_lines`. Imports `campaign_resume.py` directly for
  its tie-break `sweep_param` and tier-2 candidate set (the original used an
  `exec()`-of-source-text hack to get the same effect without a proper
  import; that hack is incompatible with configure-before-run path
  parametrisation, so this version imports normally and replicates the same
  status-load sequence explicitly — behaviour preserved, only the module
  wiring differs).
- **`campaign_post.py`** — the post-campaign chain: waits for
  `=== CAMPAIGN DONE ===` in the campaign's stdout log and no `ocr.exe`
  running, then phase 1 (edge-walk extension of any tier-1/3/4 param whose
  inner optimum sat at a tried range's edge with a >EPS gradient) and phase
  2 (stride-2 ablations A_final / B_tier2_reverted / C_wlex_035 /
  D_control). Makes no decision; only records `post_status.json`.
- **`campaign_post.as_launched.py`** — the copy actually run in production.
  Differs from `campaign_post.py` only by the absence of the
  `TIE_REVERTS = {"decode.seg_split_penalty": 0.75}` step (defined and
  applied in `campaign_post.py`, absent here) — confirmed by diff to be the
  only difference between the two files, matching the original pair's
  relationship exactly. This is the file whose output is `post_status.json`.
- **`campaign_resume_post.py`** — resumes phase 2 after the 2026-09-25 PC
  restart. At restart, `A_final` and `B_tier2_reverted` had already
  completed; `C_wlex_035` had been killed mid-run; `D_control` had never
  run. Reruns `C_wlex_035` then `D_control` sequentially, at stride 2, from
  `post_status.json`'s `A_vector`.
- **`campaign_resume_one.py`** — a finer-grained alternative to
  `campaign_resume_post.py`, used because `campaign_resume_post.py`'s
  sequential C-then-D run was itself interrupted. Runs exactly one of
  `C_wlex_035` / `D_control` and writes only `phase2_<name>.json`, so C and
  D can be launched as two separate concurrent processes with independent
  log files (`campaign_log_C_wlex_035.jsonl`, `campaign_log_D_control.jsonl`)
  instead of contending for one. `--merge` folds both `phase2_*.json` files
  back into `post_status.json` once both have finished. The two ablations
  are independent stride-2 measurements against the same fixed `A_vector`,
  so running them concurrently changes only wall time, not the result —
  each is deterministic given its own vector and the (fixed) train split.
- **`decide_12b.py`** — applies §11 steps 1-5 to a completed
  `post_status.json`: step 2 (keep tier 2 only if A beats B), step 3 (take
  C's `w_lex` only if C beats A), step 4 (B+C only if both step 2 and step 3
  triggered — printed as an argv to run by hand, never invoked here), step 5
  (the chosen vector must beat the D control). Writes `decision_12b.json`
  and, only if the result is a fold, prints the val argv for step 6 (run by
  hand, once, never invoked by this script).

## As-run command lines

Paths below are the shapes actually used; the real invocations pointed at
`D:/Dev/ExcludedPrivate/ocrcer/...` machine paths that are not reproduced
here verbatim beyond what's already committed in
`docs/measurements/2026-09-25_fit12b/`.

```
# first run (killed at 12:04, memory pressure)
python campaign.py --ocr <target-fit>/release/ocr.exe --train <pages>/finfilings-train \
    --cwd <crate-root> --log fitlogs/campaign_log.jsonl --status fitlogs/campaign_status.json

# resume 1
python campaign_resume.py --ocr <target-fit>/release/ocr.exe --train <pages>/finfilings-train \
    --cwd <crate-root> --log fitlogs/campaign_log.jsonl --status fitlogs/campaign_status.json

# resume 2 (killed again at 14:10, memory pressure)
python campaign_resume2.py --ocr <target-fit>/release/ocr.exe --train <pages>/finfilings-train \
    --cwd <crate-root> --log fitlogs/campaign_log.jsonl --status fitlogs/campaign_status.json

# post-campaign chain (as launched; TIE_REVERTS absent)
python campaign_post.as_launched.py --ocr <target-fit>/release/ocr.exe --train <pages>/finfilings-train \
    --cwd <crate-root> --log fitlogs/campaign_log.jsonl --campaign-status fitlogs/campaign_status.json \
    --fitlogs-dir fitlogs

# PC restart; A_final/B done, C killed mid-run, D never started
python campaign_resume_post.py --ocr <target-fit>/release/ocr.exe --train <pages>/finfilings-train \
    --cwd <crate-root> --log fitlogs/campaign_log.jsonl --campaign-status fitlogs/campaign_status.json \
    --post-status fitlogs/post_status.json
# (interrupted again; C and D finished as two concurrent processes instead)
python campaign_resume_one.py --ocr <target-fit>/release/ocr.exe --train <pages>/finfilings-train \
    --cwd <crate-root> --log fitlogs/campaign_log_C_wlex_035.jsonl \
    --campaign-status fitlogs/campaign_status.json --fitlogs-dir fitlogs C_wlex_035
python campaign_resume_one.py --ocr <target-fit>/release/ocr.exe --train <pages>/finfilings-train \
    --cwd <crate-root> --log fitlogs/campaign_log_D_control.jsonl \
    --campaign-status fitlogs/campaign_status.json --fitlogs-dir fitlogs D_control
python campaign_resume_one.py --fitlogs-dir fitlogs --merge

# decision
python decide_12b.py --params-tsv model/params.tsv --ocr <target-fit>/release/ocr.exe \
    --val <pages>/finfilings-val --fitlogs-dir fitlogs

# val, once, by hand (not run by any script above): default vector, then the
# chosen (fold) vector, sequentially, nothing else heavy running concurrently
<target-fit>/release/ocr.exe model/out/ocrcer.ocrw <pages>/finfilings-val
<target-fit>/release/ocr.exe model/out/ocrcer.ocrw <pages>/finfilings-val --set decode.w_seg=0.55 ...
```
