#!/usr/bin/env python3
"""Resume chunk 12b phase 2 after the 2026-09-25 restart. A_final and
B_tier2_reverted completed; C_wlex_035 was killed mid-run, D_control never ran.
Reruns C and D at stride 2 from post_status.json's A_vector, sequentially."""
import argparse
import json
import time
import campaign as c


def build_arg_parser():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--ocr", required=True, help="path to the release ocr.exe binary")
    ap.add_argument("--model", default="model/out/ocrcer.ocrw", help="model path, relative to --cwd")
    ap.add_argument("--train", required=True, help="path to the finfilings-train page directory")
    ap.add_argument("--cwd", required=True, help="working directory ocr.exe is run from (the crate root)")
    ap.add_argument("--log", required=True, help="campaign.py's log path (appended by this script too)")
    ap.add_argument("--campaign-status", required=True, help="campaign.py's status file (unused here, but part of the shared config)")
    ap.add_argument("--post-status", required=True, help="post_status.json, read and rewritten in place")
    return ap


def main():
    args = build_arg_parser().parse_args()
    c.configure(argparse.Namespace(ocr=args.ocr, model=args.model, train=args.train, cwd=args.cwd,
                                    log=args.log, status=args.campaign_status))
    post = json.load(open(args.post_status, encoding="utf-8"))
    a = post["A_vector"]
    runs = {"C_wlex_035": {**a, "decode.w_lex": 0.35}, "D_control": {}}

    def slim(r):
        return {k: r.get(k) for k in ("cer", "line_matched_cer", "wer", "recall", "precision", "f1", "wall", "error")}

    post.setdefault("resume_after_restart", time.strftime("%Y-%m-%d %H:%M:%S"))
    for name, ov in runs.items():
        r = c.run(ov, 2, f"post:ablation:{name}:resume")
        post["phase2"][name] = slim(r)
        json.dump(post, open(args.post_status, "w", encoding="utf-8"), indent=2, default=str)
        print(f"=== post ablation {name}: CER {r.get('cer')} LM {r.get('line_matched_cer')} ===", flush=True)
    if all(post["phase2"][k].get("cer") is not None for k in ("A_final", "B_tier2_reverted", "C_wlex_035", "D_control")):
        post["phase"] = "done"
    json.dump(post, open(args.post_status, "w", encoding="utf-8"), indent=2, default=str)
    print("=== POST DONE ===", flush=True)


if __name__ == "__main__":
    main()
