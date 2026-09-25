#!/usr/bin/env python3
"""Post-campaign chain for chunk 12b. finfilings-train ONLY (never val/score).

Waits for "=== CAMPAIGN DONE ===" (in the campaign's redirected stdout log)
and for no ocr.exe to be running, then:

Phase 1 - edge extension. Params of accepted tiers 1, 3 and 4 whose inner
  optimum sat at the edge of the tried range with a > EPS gradient are walked
  further out at the inner stride (edge_params); a step is taken only if it beats the best so far by > EPS, and
  the walk stops at the first step that does not. Tier-2 params are excluded
  (tier 2 is provisional: it lowered end-to-end CER but raised line-matched
  CER on the stride-6 confirm). The extended vector must then beat the final
  campaign vector by > EPS on the stride-6 confirm, or phase 1 is discarded.

Phase 2 - stride-2 ablations (about half of train), each reporting both
  end-to-end and line-matched CER:
    A  = phase-1 result (or the final campaign vector)
    B  = A with tier 2 reverted (descender_fraction 0.12, reach 0.40)
    C  = A with decode.w_lex 0.35
    D  = control: no overrides
Decisions are NOT made here; the architect reads post_status.json.

Paths are arguments (this script drives campaign.py's `run`/`configure`, so
it takes the same --ocr/--model/--train/--cwd flags, plus --fitlogs-dir for
where campaign_stdout.log/post_status.json live). See README.md in this
directory for the argv this actually ran with.
"""
import argparse
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import campaign as c  # noqa: E402  (has a __main__ guard; importing runs nothing)

EDGE_TIERS = ("tier1_decode", "tier3_words_segment", "tier4_slant")  # tier 2 provisional; cost knobs excluded


def edge_params(status, base):
    """Params of accepted tiers whose chosen value sits at the edge of the tried
    range with a > EPS gradient toward that edge: walk up to three steps out
    at half the grid spacing (a full step can overshoot a near optimum and
    stop the walk early), positive values only."""
    kinds = {n: k for t in c.TIERS.values() for n, (k, _) in t.items()}
    out = {}
    for tier in EDGE_TIERS:
        rec = status.get("tiers", {}).get(tier, {})
        if not rec.get("accepted"):
            continue
        for name, p in rec.get("params", {}).items():
            if name not in kinds or base.get(name) != p.get("chosen"):
                continue
            res = {r["value"]: r["cer"] for r in p.get("results", []) if r.get("cer") is not None}
            cands = sorted(res)
            if len(cands) < 2:
                continue
            ch = p["chosen"]
            if ch == cands[0] and res[cands[0]] < res[cands[1]] - c.EPS:
                step = -(cands[1] - cands[0]) / 2
            elif ch == cands[-1] and res[cands[-1]] < res[cands[-2]] - c.EPS:
                step = (cands[-1] - cands[-2]) / 2
            else:
                continue
            vals = [ch + k * step for k in (1, 2, 3)]
            if kinds[name] == "u32":
                vals = sorted({int(round(v)) for v in vals if v >= 1} - {ch}, key=lambda v: abs(v - ch))
            else:
                vals = [round(v, 4) for v in vals if v > 0]
            if vals:
                out[name] = (kinds[name], vals)
    return out


TIER2_REVERT = {"lines.descender_fraction": 0.12, "lines.descender_reach_fraction": 0.4}
# ARCHITECTURE.md §11, 2026-09-25: a tier-1 move made on an exact inner tie, reverted before phase 1.
TIE_REVERTS = {"decode.seg_split_penalty": 0.75}

post = {"phase": "waiting", "started": time.time()}
memo = {}

STDOUT_LOG = None
POST_STATUS = None


def save():
    with open(POST_STATUS, "w", encoding="utf-8") as f:
        json.dump(post, f, indent=2, default=str)


def campaign_done():
    try:
        with open(STDOUT_LOG, encoding="utf-8", errors="replace") as f:
            return "=== CAMPAIGN DONE ===" in f.read()
    except OSError:
        return False


def ocr_running():
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq ocr.exe"], capture_output=True, text=True).stdout
    return "ocr.exe" in out


def run(ov, stride, label):
    key = (stride, tuple(sorted((k, str(v)) for k, v in ov.items())))
    if key not in memo:
        memo[key] = c.run(ov, stride, label)
    return memo[key]


def slim(r):
    return {k: r.get(k) for k in ("cer", "line_matched_cer", "wer", "recall", "precision", "f1", "wall", "error")}


def build_arg_parser():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--ocr", required=True, help="path to the release ocr.exe binary")
    ap.add_argument("--model", default="model/out/ocrcer.ocrw", help="model path, relative to --cwd")
    ap.add_argument("--train", required=True, help="path to the finfilings-train page directory")
    ap.add_argument("--cwd", required=True, help="working directory ocr.exe is run from (the crate root)")
    ap.add_argument("--log", required=True, help="campaign.py's log path (appended by this script too)")
    ap.add_argument("--campaign-status", required=True, help="campaign.py's status file (read once, at the end)")
    ap.add_argument("--fitlogs-dir", required=True,
                     help="directory holding campaign_stdout.log (read) and post_status.json (written)")
    return ap


def main():
    global STDOUT_LOG, POST_STATUS
    args = build_arg_parser().parse_args()
    c.configure(argparse.Namespace(ocr=args.ocr, model=args.model, train=args.train, cwd=args.cwd,
                                    log=args.log, status=args.campaign_status))
    STDOUT_LOG = os.path.join(args.fitlogs_dir, "campaign_stdout.log")
    POST_STATUS = os.path.join(args.fitlogs_dir, "post_status.json")

    save()
    while not campaign_done():
        time.sleep(60)
    while ocr_running():
        time.sleep(30)
    with open(c.STATUS_PATH, encoding="utf-8") as f:
        cstatus = json.load(f)
    final = dict(cstatus["base"])
    post["tie_reverts"] = {k: (final.get(k), v) for k, v in TIE_REVERTS.items()}
    final.update(TIE_REVERTS)
    EDGE = edge_params(cstatus, final)
    post["final_campaign_base"] = final
    post["edge_walks_planned"] = {k: v[1] for k, v in EDGE.items()}
    print("=== post: start " + time.strftime("%H:%M:%S") + " ===", flush=True)

    # Phase 1
    post["phase"] = "phase1"
    save()
    tent = dict(final)
    best = run(tent, c.INNER_STRIDE, "post:inner:current")
    best_cer = best.get("cer", 1e9)
    walks = {}
    for name, (kind, steps) in EDGE.items():
        walk = [{"value": tent.get(name), "cer": best_cer}]
        for v in steps:
            r = run({**tent, name: v}, c.INNER_STRIDE, f"post:inner:{name}={v}")
            walk.append({"value": v, "cer": r.get("cer"), "error": r.get("error")})
            if "error" in r or r["cer"] >= best_cer - c.EPS:
                break
            tent[name] = v
            best_cer = r["cer"]
        walks[name] = {"walk": walk, "chosen": tent.get(name)}
        post["phase1_walks"] = walks
        save()
    changed = {k: tent[k] for k in EDGE if tent.get(k) != final.get(k)}
    post["phase1_changed"] = changed
    if changed:
        b = run(final, c.CONFIRM_STRIDE, "post:confirm:phase1:baseline")
        t = run(tent, c.CONFIRM_STRIDE, "post:confirm:phase1:tentative")
        ok = "error" not in b and "error" not in t and t["cer"] < b["cer"] - c.EPS
        post["phase1_confirm"] = {"baseline": slim(b), "tentative": slim(t), "accepted": ok}
        a_vec = tent if ok else final
        print(f"=== post phase1: accepted={ok} base={b.get('cer')} tent={t.get('cer')} ===", flush=True)
    else:
        a_vec = final
        print("=== post phase1: no edge moved ===", flush=True)
    post["A_vector"] = a_vec
    save()

    # Phase 2
    post["phase"] = "phase2"
    save()
    runs = {
        "A_final": a_vec,
        "B_tier2_reverted": {**a_vec, **TIER2_REVERT},
        "C_wlex_035": {**a_vec, "decode.w_lex": 0.35},
        "D_control": {},
    }
    post["phase2"] = {}
    for name, ov in runs.items():
        r = run(ov, 2, f"post:ablation:{name}")
        post["phase2"][name] = slim(r)
        save()
        print(f"=== post ablation {name}: CER {r.get('cer')} LM {r.get('line_matched_cer')} ===", flush=True)
    post["phase"] = "done"
    save()
    print("=== POST DONE ===", flush=True)


if __name__ == "__main__":
    main()
