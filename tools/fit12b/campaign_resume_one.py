#!/usr/bin/env python3
"""Run one chunk-12b phase-2 stride-2 ablation (C_wlex_035 or D_control) from
post_status.json's A_vector; writes fitlogs/phase2_<name>.json only, so two can
run concurrently. Merge into post_status.json afterwards with --merge.

Usage:
  campaign_resume_one.py --ocr ... --train ... --cwd ... --log ... \
      --campaign-status ... --fitlogs-dir ... C_wlex_035
  campaign_resume_one.py --fitlogs-dir ... --merge
"""
import argparse
import json
import os
import sys
import campaign as c

NAMES = ("A_final", "B_tier2_reverted", "C_wlex_035", "D_control")


def build_arg_parser():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--ocr", help="path to the release ocr.exe binary (not needed for --merge)")
    ap.add_argument("--model", default="model/out/ocrcer.ocrw", help="model path, relative to --cwd")
    ap.add_argument("--train", help="path to the finfilings-train page directory (not needed for --merge)")
    ap.add_argument("--cwd", help="working directory ocr.exe is run from (not needed for --merge)")
    ap.add_argument("--log", help="log path this run appends to (not needed for --merge)")
    ap.add_argument("--campaign-status", help="campaign.py's status file (unused here, but part of the shared config)")
    ap.add_argument("--fitlogs-dir", required=True, help="directory holding post_status.json and phase2_<name>.json")
    ap.add_argument("--merge", action="store_true", help="merge phase2_C_wlex_035.json/phase2_D_control.json into post_status.json")
    ap.add_argument("name", nargs="?", choices=["C_wlex_035", "D_control"], help="which ablation to run")
    return ap


def main():
    args = build_arg_parser().parse_args()
    post_path = os.path.join(args.fitlogs_dir, "post_status.json")
    post = json.load(open(post_path, encoding="utf-8"))

    if args.merge:
        for n in ("C_wlex_035", "D_control"):
            post["phase2"][n] = json.load(open(os.path.join(args.fitlogs_dir, f"phase2_{n}.json"), encoding="utf-8"))
        if all(post["phase2"][n].get("cer") is not None for n in NAMES):
            post["phase"] = "done"
        json.dump(post, open(post_path, "w", encoding="utf-8"), indent=2, default=str)
        print("merged; phase =", post["phase"])
        return

    if not args.name:
        print("error: NAME is required unless --merge is given", file=sys.stderr)
        sys.exit(2)
    c.configure(argparse.Namespace(ocr=args.ocr, model=args.model, train=args.train, cwd=args.cwd,
                                    log=os.path.join(args.fitlogs_dir, f"campaign_log_{args.name}.jsonl"),
                                    status=args.campaign_status))
    ov = {"C_wlex_035": {**post["A_vector"], "decode.w_lex": 0.35}, "D_control": {}}[args.name]
    r = c.run(ov, 2, f"post:ablation:{args.name}:resume")
    slim = {k: r.get(k) for k in ("cer", "line_matched_cer", "wer", "recall", "precision", "f1", "wall", "error")}
    json.dump(slim, open(os.path.join(args.fitlogs_dir, f"phase2_{args.name}.json"), "w", encoding="utf-8"), indent=2, default=str)
    print(f"=== post ablation {args.name}: CER {r.get('cer')} LM {r.get('line_matched_cer')} ===", flush=True)


if __name__ == "__main__":
    main()
