#!/usr/bin/env python3
"""Second resume (after 14:10 memory kill). Tier-2 tentative carries the two
choices already made (descender_fraction 0.17, descender_reach_fraction 0.55);
sweeps resume at lines.inherit_x_height_below. Tie-break as campaign_resume.py
(imported directly here rather than exec'd, so campaign's paths can be
configured by argparse instead of being hard-coded module constants)."""
import json
import campaign as c
import campaign_resume as cr  # provides the tie-break sweep_param, CURRENT and t2/done_t2


def run_tier2_seeded(params, seed, saved_params):
    c.status["phase"] = "tier:tier2_lines(resume2)"
    c.write_status()
    base = dict(c.status["base"])
    tentative = {**base, **seed}
    rec = {"params": dict(saved_params), "inner_stride": c.INNER_STRIDE}
    for name, (kind, cands) in params.items():
        best, ov, results = c.sweep_param(name, kind, cands, tentative)
        tentative.update(ov)
        rec["params"][name] = {"candidates": cands, "chosen": best,
            "results": [{"value": r.get("value"), "cer": r.get("cer"), "error": r.get("error")} for r in results]}
        c.status["tiers"]["tier2_lines"] = rec
        c.write_status()
    b = c.run(base, c.CONFIRM_STRIDE, "confirm:tier2_lines:baseline")
    t = c.run(tentative, c.CONFIRM_STRIDE, "confirm:tier2_lines:tentative")
    ok = "error" not in b and "error" not in t and t["cer"] < b["cer"] - c.EPS
    rec.update(baseline_confirm={"cer": b.get("cer"), "line_matched_cer": b.get("line_matched_cer")},
               tentative_confirm={"cer": t.get("cer"), "line_matched_cer": t.get("line_matched_cer")}, accepted=ok)
    c.status["tiers"]["tier2_lines"] = rec
    if ok:
        c.status["base"] = tentative
    c.write_status()
    print(f"=== tier tier2_lines: accepted={ok} base={b.get('cer')} tent={t.get('cer')} ===", flush=True)


def main():
    with open(c.STATUS_PATH, encoding="utf-8") as f:
        saved = json.load(f)
    c.status.update(saved)
    c.status["base"]["decode.w_lex"] = 0.6
    c.status["resumed"] = "after memory-pressure kill; tie-break keeps current unless > EPS better; w_lex reverted to 0.6"
    c.sweep_param = cr.sweep_param

    saved_params = dict(c.status["tiers"].get("tier2_lines", {}).get("params", {}))
    seed = {k: v["chosen"] for k, v in saved_params.items() if v.get("chosen") is not None}
    rest = {k: v for k, v in cr.t2.items() if k not in saved_params}

    run_tier2_seeded(rest, seed, saved_params)
    c.run_tier("tier3_words_segment", c.TIERS["tier3_words_segment"])
    c.run_tier("tier4_slant", c.TIERS["tier4_slant"])
    c.run_cost_knobs()
    c.status["phase"] = "done"
    c.write_status()
    print("=== CAMPAIGN DONE ===\n" + json.dumps(c.status["base"], indent=2))


if __name__ == "__main__":
    c.configure(c.build_arg_parser(description=__doc__).parse_args())
    main()
