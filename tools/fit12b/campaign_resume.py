#!/usr/bin/env python3
"""Resume chunk 12b campaign after the 12:04 memory-pressure kill.

Changes vs campaign.py (architect review, 2026-09-24):
- Tie-break: a candidate replaces the current value only if it beats the
  current value's inner CER by more than EPS; otherwise the current value
  (and its guess label) stays. campaign.py picked the first/lowest even on
  ties, which chose w_lex=0.35 on byte-identical CER (0.6 had better F1).
- Base = tier1 accepted vector with decode.w_lex reverted to 0.6.
- tier2_lines resumes at lines.descender_fraction; its first five params
  are kept at current values (all inner deltas <= 0.009 < EPS).
Log/status files are shared with campaign.py (append-only log); pass the
same --log/--status paths campaign.py was given.
"""
import json
import campaign as c

CURRENT = {}
for tier in c.TIERS.values():
    for name, (kind, cands) in tier.items():
        CURRENT[name] = cands[1]  # middle candidate = current value in every grid


def sweep_param(name, kind, candidates, base):
    results = []
    for v in candidates:
        overrides = dict(base)
        overrides.update(c.expand(name, v, base))
        r = c.run(overrides, c.INNER_STRIDE, f"inner:{name}={v}")
        r["param"] = name
        r["value"] = v
        results.append(r)
    ok = [r for r in results if "error" not in r]
    if not ok:
        return None, {}, results
    cur = next((r for r in ok if r["value"] == CURRENT.get(name)), None)
    best = min(ok, key=lambda r: r["cer"])
    if cur is not None and best["cer"] >= cur["cer"] - c.EPS:
        best = cur
    return best["value"], c.expand(name, best["value"], base), results


c.sweep_param = sweep_param

done_t2 = ["deskew.min_corrected_slope", "lines.min_area", "lines.furniture_fraction",
           "lines.mark_height_fraction", "lines.mark_reach_fraction"]
t2 = {k: v for k, v in c.TIERS["tier2_lines"].items() if k not in done_t2}


def main():
    with open(c.STATUS_PATH, encoding="utf-8") as f:
        saved = json.load(f)
    c.status.update(saved)
    c.status["base"]["decode.w_lex"] = 0.6
    c.status["resumed"] = "after memory-pressure kill; tie-break keeps current unless > EPS better; w_lex reverted to 0.6"

    c.run_tier("tier2_lines", t2)
    c.run_tier("tier3_words_segment", c.TIERS["tier3_words_segment"])
    c.run_tier("tier4_slant", c.TIERS["tier4_slant"])
    c.run_cost_knobs()
    c.status["phase"] = "done"
    c.write_status()
    print("\n=== CAMPAIGN DONE ===")
    print(json.dumps(c.status["base"], indent=2))


if __name__ == "__main__":
    c.configure(c.build_arg_parser(description=__doc__).parse_args())
    main()
