"""Apply ARCHITECTURE.md section 11, 2026-09-25 ("How chunk 12b's vector is
chosen") to post_status.json. Decides steps 1-5 only; prints the val argv
for step 6 and never runs it.

Usage:
  python decide_12b.py --params-tsv model/params.tsv --ocr <ocr.exe> \
      --val <finfilings-val dir> --fitlogs-dir <dir with post_status.json>
  python decide_12b.py ... --bc cer,lm    # supply the step-4 B+C stride-2 result
"""
import argparse
import json
import os

EPS = 0.02
METRICS = ("cer", "line_matched_cer")


def defaults(params_tsv):
    d = {}
    with open(params_tsv, encoding="utf-8") as f:
        for line in f:
            if line.startswith("#") or "\t" not in line:
                continue
            k, v = line.split("\t")[:2]
            d[k] = v
    return d


def same(a, b):
    try:
        return float(a) == float(b)
    except (TypeError, ValueError):
        return str(a) == str(b)


def changes(vec, dflt):
    return sum(1 for k, v in vec.items() if not same(v, dflt.get(k)))


def beats(x, y):
    """x beats y: better by > EPS on at least one metric, worse by > EPS on neither."""
    better = any(x[m] < y[m] - EPS for m in METRICS)
    worse = any(x[m] > y[m] + EPS for m in METRICS)
    return better and not worse


def fmt(r):
    return f"CER {r['cer']:.3f} LM {r['line_matched_cer']:.3f}"


def build_arg_parser():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--params-tsv", required=True, help="path to model/params.tsv")
    ap.add_argument("--ocr", required=True, help="path to the release ocr.exe binary (printed in the step-6 argv only)")
    ap.add_argument("--model", default="model/out/ocrcer.ocrw", help="model path, relative to the val cwd")
    ap.add_argument("--val", required=True, help="path to the finfilings-val page directory (printed in the step-6 argv only)")
    ap.add_argument("--fitlogs-dir", required=True, help="directory holding post_status.json (read) and decision_12b.json (written)")
    ap.add_argument("--bc", help="step-4 B+C stride-2 result as 'cer,lm'")
    return ap


def main():
    args = build_arg_parser().parse_args()
    post_path = os.path.join(args.fitlogs_dir, "post_status.json")
    out_path = os.path.join(args.fitlogs_dir, "decision_12b.json")

    post = json.load(open(post_path, encoding="utf-8"))
    if post.get("phase") != "done":
        raise SystemExit(f"post chain not done (phase={post.get('phase')})")
    p2 = post["phase2"]
    for name, r in p2.items():
        if r.get("error") or r.get("cer") is None:
            raise SystemExit(f"{name} has no result: {r}")
    dflt = defaults(args.params_tsv)
    A_vec = post["A_vector"]
    B_vec = {**A_vec, "lines.descender_fraction": 0.12, "lines.descender_reach_fraction": 0.4}
    C_vec = {**A_vec, "decode.w_lex": 0.35}
    A, B, C, D = (p2[k] for k in ("A_final", "B_tier2_reverted", "C_wlex_035", "D_control"))
    log = [f"A {fmt(A)} ({changes(A_vec, dflt)} changes)", f"B {fmt(B)}", f"C {fmt(C)}", f"D {fmt(D)}"]

    # Step 2: tier 2 stays only if A beats B.
    keep_tier2 = beats(A, B)
    log.append(f"step 2: A beats B = {keep_tier2} -> {'keep tier 2' if keep_tier2 else 'revert tier 2 (B)'}")
    # Step 3: C replaces A's w_lex only if C beats A.
    take_c = beats(C, A)
    log.append(f"step 3: C beats A = {take_c} -> {'w_lex 0.35' if take_c else 'keep w_lex'}")

    chosen_vec, chosen_res, label = A_vec, A, "A"
    if not keep_tier2 and not take_c:
        chosen_vec, chosen_res, label = B_vec, B, "B"
    elif keep_tier2 and take_c:
        chosen_vec, chosen_res, label = C_vec, C, "C"
    elif not keep_tier2 and take_c:
        # Step 4: both changes; B+C never measured together.
        bc_vec = {**B_vec, "decode.w_lex": 0.35}
        if not args.bc:
            log.append("step 4: B+C needed -- run one stride-2 pass of this vector, then rerun with --bc cer,lm")
            json.dump({"status": "need_bc", "bc_vector": bc_vec, "log": log}, open(out_path, "w"), indent=1)
            print("\n".join(log))
            print("B+C argv:", " ".join([args.ocr, args.model, "<finfilings-train>", "--stride", "2"]
                                        + sum((["--set", f"{k}={v}"] for k, v in bc_vec.items()), [])))
            return
        cer, lm = (float(x) for x in args.bc.split(","))
        BC = {"cer": cer, "line_matched_cer": lm}
        if beats(BC, A):
            chosen_vec, chosen_res, label = bc_vec, BC, "B+C"
            log.append(f"step 4: B+C {fmt(BC)} beats A -> adopt both")
        else:
            gain_b, gain_c = A["cer"] - B["cer"], A["cer"] - C["cer"]
            if gain_b >= gain_c:
                chosen_vec, chosen_res, label = B_vec, B, "B"
            else:
                chosen_vec, chosen_res, label = C_vec, C, "C"
            log.append(f"step 4: B+C {fmt(BC)} does not beat A -> larger end-to-end gain: {label}"
                       f" (B {gain_b:+.3f}, C {gain_c:+.3f})")

    # Step 5: the result must beat the control.
    fold = beats(chosen_res, D)
    log.append(f"step 5: {label} {fmt(chosen_res)} beats D {fmt(D)} = {fold}"
               + ("" if fold else " -> 12b closes with NO FOLD"))
    moved = {k: v for k, v in chosen_vec.items() if not same(v, dflt.get(k))}
    out = {"status": "fold" if fold else "no_fold", "chosen": label, "vector": chosen_vec,
           "moved_from_default": moved, "log": log}
    json.dump(out, open(out_path, "w"), indent=1)
    print("\n".join(log))
    print(f"{len(moved)} rows move from Params::DEFAULT: {moved}")
    if fold:
        print("step 6 (val once, run by hand, sequentially, nothing else heavy running):")
        print("  default:", " ".join([args.ocr, args.model, args.val]))
        print("  chosen: ", " ".join([args.ocr, args.model, args.val] + sum((["--set", f"{k}={v}"] for k, v in moved.items()), [])))
        print("  cwd: the crate root the val binary is run from; the chosen vector must beat default under the same rule")


if __name__ == "__main__":
    main()
