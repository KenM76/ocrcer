#!/usr/bin/env python3
"""Chunk 12b coordinate-descent campaign driver.

Runs entirely against finfilings-train (never val, never the score set, never
pages-cov/fixtures). For each tier:
  1. Inner sweep: for each param in the tier, in order, run a small 3-point
     grid (below/current/above) at a small stride against the CURRENT
     accumulated base vector, holding everything else fixed. Pick the
     candidate with the lowest CER on that inner sample as the tentative
     value for that param (coordinate descent -- direction finding only).
  2. Tier confirm: run the tentative full-tier vector against a larger
     stride sample and compare to the same-sample baseline (pre-tier
     vector). If the tentative vector does not beat baseline CER on the
     confirm sample, the WHOLE tier's changes are discarded (kept at
     current values) rather than accepted piecemeal, and this is logged as
     a finding, not silently dropped.
  3. On acceptance, the tier's tentative values are folded into the base
     vector carried into the next tier.

Every run is logged verbatim (argv + parsed metrics) to the log path. A
running status is written to the status path after every group and every
tier confirm, so progress can be read without parsing the whole log.

This script does not decide gates on its own beyond the per-tier
accept/reject above -- the final full-train confirm, validation confirm and
one-time score-set run are separate, deliberate steps run by hand afterward.

Paths (ocr.exe, the model, the train page directory, the crate's working
directory, and where the log/status files go) are arguments, not defaults --
this script never bakes in a machine-specific location. See README.md in
this directory for the argv this campaign actually ran with.
"""
import argparse
import json
import re
import subprocess
import sys
import time

CER_RE = re.compile(r"^end-to-end\s+CER\s+([\d.]+)%\s+WER\s+([\d.]+)%\s+recall\s+([\d.]+)%\s+precision\s+([\d.]+)%\s+F1\s+([\d.]+)%", re.M)
LM_RE = re.compile(r"^line-matched\s+CER\s+([\d.]+)%", re.M)

# Populated by configure() (called from main() after argparse, or by a
# resume script that imports this module and sets them directly).
OCR = None
MODEL = None
TRAIN = None
CWD = None
LOG_PATH = None
STATUS_PATH = None

INNER_STRIDE = 35   # ~13 pages, ~3-4 min/point -- direction-finding only
CONFIRM_STRIDE = 6  # ~71 pages, ~17-18 min -- the real per-tier accept/reject gate

status = {"phase": "starting", "base": {}, "tiers": {}, "started": time.time()}


def build_arg_parser(description=__doc__):
    ap = argparse.ArgumentParser(description=description)
    ap.add_argument("--ocr", required=True, help="path to the release ocr.exe binary")
    ap.add_argument("--model", default="model/out/ocrcer.ocrw",
                     help="model path, resolved relative to --cwd (default: model/out/ocrcer.ocrw)")
    ap.add_argument("--train", required=True, help="path to the finfilings-train page directory")
    ap.add_argument("--cwd", required=True, help="working directory ocr.exe is run from (the crate root)")
    ap.add_argument("--log", required=True, help="path to the campaign log (JSONL, appended)")
    ap.add_argument("--status", required=True, help="path to the campaign status file (JSON, overwritten)")
    return ap


def configure(args):
    """Set the module-level paths from parsed args. Called by this script's
    own __main__, and by any resume/post script that imports this module."""
    global OCR, MODEL, TRAIN, CWD, LOG_PATH, STATUS_PATH
    OCR, MODEL, TRAIN, CWD = args.ocr, args.model, args.train, args.cwd
    LOG_PATH, STATUS_PATH = args.log, args.status


def write_status():
    with open(STATUS_PATH, "w", encoding="utf-8") as f:
        json.dump(status, f, indent=2, default=str)


def log(rec):
    with open(LOG_PATH, "a", encoding="utf-8") as f:
        f.write(json.dumps(rec, default=str) + "\n")


def run(overrides, stride, label):
    argv = [OCR, MODEL, TRAIN, "--stride", str(stride)]
    for k, v in overrides.items():
        argv += ["--set", f"{k}={v}"]
    t0 = time.time()
    proc = subprocess.run(argv, capture_output=True, text=True, cwd=CWD)
    wall = time.time() - t0
    out = proc.stdout
    rec = {"label": label, "argv": argv, "overrides": dict(overrides), "wall": wall, "stride": stride}
    if proc.returncode != 0:
        rec["error"] = proc.stderr or out
        log(rec)
        print(f"[{label}] ERROR {rec['error'][:300]}", flush=True)
        return rec
    m = CER_RE.search(out)
    lm = LM_RE.search(out)
    if not m:
        rec["error"] = "no CER line found"
        rec["raw"] = out[-1500:]
        log(rec)
        print(f"[{label}] ERROR no CER line ({wall:.0f}s)", flush=True)
        return rec
    cer, wer, recall, precision, f1 = map(float, m.groups())
    rec.update(cer=cer, wer=wer, recall=recall, precision=precision, f1=f1,
               line_matched_cer=float(lm.group(1)) if lm else None)
    log(rec)
    print(f"[{label}] CER {cer:.3f}%  LM {rec['line_matched_cer']}  F1 {f1:.3f}%  ({wall:.0f}s, stride {stride})", flush=True)
    return rec


# name -> (kind, candidates)  kind: "f32" or "u32"
TIERS = {
    "tier1_decode": {
        "decode.w_lex": ("f32", [0.35, 0.6, 0.9]),
        "decode.w_seg": ("f32", [0.12, 0.25, 0.4]),
        "__lex_bonus_scale__": ("scale", [0.7, 1.0, 1.3]),
        "decode.seg_ideal_aspect": ("f32", [0.45, 0.6, 0.75]),
        "decode.seg_aspect_tolerance": ("f32", [0.35, 0.55, 0.75]),
        "decode.seg_merge_penalty": ("f32", [0.3, 0.5, 0.7]),
        "decode.seg_split_penalty": ("f32", [0.5, 0.75, 1.0]),
        "decode.char_bonus_slanted": ("f32", [2.8, 3.44, 4.2]),
    },
    "tier2_lines": {
        "deskew.min_corrected_slope": ("f32", [0.0007, 0.0015, 0.003]),
        "lines.min_area": ("u32", [1, 2, 3]),
        "lines.furniture_fraction": ("f32", [0.13, 0.2, 0.3]),
        "lines.mark_height_fraction": ("f32", [0.25, 0.35, 0.45]),
        "lines.mark_reach_fraction": ("f32", [0.45, 0.6, 0.78]),
        "lines.descender_fraction": ("f32", [0.08, 0.12, 0.17]),
        "lines.descender_reach_fraction": ("f32", [0.28, 0.4, 0.55]),
        "lines.inherit_x_height_below": ("f32", [0.4, 0.5, 0.62]),
        "lines.baseline_split_sep": ("f32", [0.45, 0.6, 0.78]),
        "lines.baseline_split_support": ("f32", [0.17, 0.25, 0.35]),
        "lines.thin_debris_heights": ("f32", [2.8, 3.4288, 4.2]),
    },
    "tier3_words_segment": {
        "words.pitch_min_glyphs": ("u32", [4, 6, 9]),
        "words.pitch_agreement": ("f32", [0.65, 0.8, 0.92]),
        "words.pitch_tolerance": ("f32", [0.1, 0.15, 0.22]),
        "segment.max_merge_x_heights": ("f32", [1.5, 1.8, 2.15]),
        "segment.valley_fraction": ("f32", [0.35, 0.5, 0.65]),
        "segment.min_piece_x_heights": ("f32", [0.12, 0.2, 0.28]),
    },
    "tier4_slant": {
        "layout.slant_min_deg": ("f32", [4, 6, 8.5]),
        "layout.slant_margin": ("f32", [1.08, 1.15, 1.3]),
    },
}

COST_KNOBS = {
    "match.top_k": ("u32", [3, 5, 8]),
    "decode.beam_width": ("u32", [14, 24, 36]),
}

LEX_TIER_BASE = {
    "decode.lex_bonus_tier1": 1.0,
    "decode.lex_bonus_tier2": 0.85,
    "decode.lex_bonus_tier3": 0.7,
    "decode.lex_bonus_tier4": 0.55,
    "decode.lex_bonus_tier5": 0.4,
}

EPS = 0.02  # CER points; treat smaller diffs on the inner sample as noise, prefer no-change


def expand(name, val, base):
    """Turn a (possibly pseudo) param name+value into a dict of real overrides."""
    if name == "__lex_bonus_scale__":
        return {k: round(v * val, 4) for k, v in LEX_TIER_BASE.items()}
    return {name: val}


def sweep_param(name, kind, candidates, base):
    """Inner small-stride sweep. Returns (best_value, best_overrides, results)."""
    results = []
    for v in candidates:
        overrides = dict(base)
        overrides.update(expand(name, v, base))
        label = f"inner:{name}={v}"
        r = run(overrides, INNER_STRIDE, label)
        r["param"] = name
        r["value"] = v
        results.append(r)
    ok = [r for r in results if "error" not in r]
    if not ok:
        return None, {}, results
    best = min(ok, key=lambda r: r["cer"])
    return best["value"], expand(name, best["value"], base), results


def run_tier(tier_name, params):
    status["phase"] = f"tier:{tier_name}"
    write_status()
    base = dict(status["base"])
    tentative = dict(base)
    tier_record = {"params": {}, "inner_stride": INNER_STRIDE}
    for name, (kind, candidates) in params.items():
        best_val, overrides, results = sweep_param(name, kind, candidates, tentative)
        tentative.update(overrides)
        tier_record["params"][name] = {
            "candidates": candidates,
            "results": [{"value": r.get("value"), "cer": r.get("cer"), "error": r.get("error")} for r in results],
            "chosen": best_val,
        }
        status["tiers"].setdefault(tier_name, {})["params"] = tier_record["params"]
        write_status()

    # Tier confirm: tentative vs baseline, larger sample.
    baseline_confirm = run(base, CONFIRM_STRIDE, f"confirm:{tier_name}:baseline")
    tentative_confirm = run(tentative, CONFIRM_STRIDE, f"confirm:{tier_name}:tentative")
    accepted = False
    reason = ""
    if "error" in baseline_confirm or "error" in tentative_confirm:
        reason = "confirm run errored; tier changes discarded"
    elif tentative_confirm["cer"] < baseline_confirm["cer"] - EPS:
        accepted = True
        reason = f"tentative CER {tentative_confirm['cer']:.3f} beats baseline {baseline_confirm['cer']:.3f} on {CONFIRM_STRIDE}-stride confirm"
    else:
        reason = f"tentative CER {tentative_confirm.get('cer')} did not beat baseline {baseline_confirm.get('cer')} by > {EPS}; tier changes discarded (inner-sweep signal did not replicate at confirm scale)"

    tier_record["baseline_confirm"] = {"cer": baseline_confirm.get("cer"), "line_matched_cer": baseline_confirm.get("line_matched_cer")}
    tier_record["tentative_confirm"] = {"cer": tentative_confirm.get("cer"), "line_matched_cer": tentative_confirm.get("line_matched_cer")}
    tier_record["accepted"] = accepted
    tier_record["reason"] = reason
    status["tiers"][tier_name] = tier_record
    if accepted:
        status["base"] = tentative
    write_status()
    print(f"=== tier {tier_name}: accepted={accepted} :: {reason} ===", flush=True)


def run_cost_knobs():
    status["phase"] = "cost_knobs"
    write_status()
    base = dict(status["base"])
    knob_record = {}
    for name, (kind, candidates) in COST_KNOBS.items():
        results = []
        for v in sorted(candidates):  # cheapest first
            overrides = dict(base)
            overrides[name] = v
            r = run(overrides, CONFIRM_STRIDE, f"cost:{name}={v}")
            r["value"] = v
            results.append(r)
        ok = [r for r in results if "error" not in r]
        knob_record[name] = {"results": [{"value": r["value"], "cer": r.get("cer"), "error": r.get("error")} for r in results]}
        if ok:
            current_val_result = None
            for r in ok:
                if abs(r["value"] - {"match.top_k": 5, "decode.beam_width": 24}[name]) < 1e-9:
                    current_val_result = r
            baseline_cer = current_val_result["cer"] if current_val_result else min(r["cer"] for r in ok)
            cheapest_ok = None
            for r in ok:  # already sorted cheapest first
                if r["cer"] <= baseline_cer + EPS:
                    cheapest_ok = r
                    break
            if cheapest_ok and abs(cheapest_ok["value"] - {"match.top_k": 5, "decode.beam_width": 24}[name]) > 1e-9:
                base[name] = cheapest_ok["value"]
                knob_record[name]["chosen"] = cheapest_ok["value"]
                knob_record[name]["reason"] = f"cheapest candidate within {EPS} CER of baseline {baseline_cer:.3f}"
            else:
                knob_record[name]["chosen"] = {"match.top_k": 5, "decode.beam_width": 24}[name]
                knob_record[name]["reason"] = "no cheaper candidate matched baseline accuracy; kept current"
        status["tiers"]["cost_knobs"] = knob_record
        write_status()
    status["base"] = base
    write_status()


def main():
    for tier_name, params in TIERS.items():
        run_tier(tier_name, params)
    run_cost_knobs()
    status["phase"] = "done"
    write_status()
    print("\n=== CAMPAIGN DONE ===")
    print(json.dumps(status["base"], indent=2))


if __name__ == "__main__":
    configure(build_arg_parser().parse_args())
    main()
