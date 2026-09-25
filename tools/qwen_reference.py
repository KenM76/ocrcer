"""Reference oracle for `ocrcer-llm` (PLAN chunk 16a).

Not part of the shipped model or the Rust workspace: this is a one-off,
out-of-repo-weights script that uses the machine's torch 2.10 (CPU) /
transformers 5.1 install to generate committed fixture files under
`fixtures/llm/`. The fixtures are checked in; the weights it reads are not
(`D:/Dev/ExcludedPrivate/ocrcer/llm/<model>/`, per ARCHITECTURE.md section 11).

Usage:
    python tools/qwen_reference.py <hf_dir> <out_prefix>

Writes:
  - `<out_prefix>_tokens.json`: fixed strings -> HF tokenizer ids.
  - `<out_prefix>_logits.json`: manifest (prompt -> token ids, top5 ids/logits).
  - `<out_prefix>_logits_<i>.bin`: prompt i's full float32 logit vector at the
    last position, raw little-endian, `vocab_size * 4` bytes. Binary rather
    than JSON so a `vocab_size`-long float32 vector costs exactly 4 bytes a
    value instead of ASCII digits per value -- the fixture is otherwise
    committed at 5x-10x the size for no gain in precision, since these are
    already-rounded float32 numbers on both sides of the comparison.
"""

import json
import struct
import sys

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer

# 50 fixed strings spanning prose, financial text, CAD-style identifiers,
# unicode punctuation, and whitespace runs -- the shapes the pretokenizer's
# hand-derived branches (contractions, digit runs, punctuation runs, the
# three whitespace alternatives) all need to be exercised by.
TEST_STRINGS = [
    "Hello, world!",
    "The quick brown fox jumps over the lazy dog.",
    "  leading spaces",
    "trailing spaces  ",
    "a\tb\tc",
    "line one\nline two",
    "line one\r\nline two",
    "\n\n\n",
    "   \n\n   ",
    "don't stop believing",
    "it's a test, isn't it?",
    "I'll be there, you're welcome, we've won, they'd go",
    "M8x1.25",
    "\u00d812 H7",
    "R0.5",
    "\u00b10.05 mm",
    "$1,234.56",
    "-$1,234.56",
    "12.5%",
    "$0.99",
    "Invoice #A-2026-0917",
    "Net 30 days",
    "Q3 2026 revenue: $4,502,118",
    "SKU: 88213-B",
    "Part No. 4402-A-REV2",
    "Tolerance: +0.010/-0.005",
    "\u201cSmart quotes\u201d and \u2018single ones\u2019",
    "an em dash \u2014 like this",
    "an en dash \u2013 like this",
    "caf\u00e9, na\u00efve, r\u00e9sum\u00e9",
    "100000000",
    "3.14159265358979",
    "0x1F4A",
    "user@example.com",
    "https://example.com/path?q=1&r=2",
    "C:\\Users\\Ken\\file.txt",
    "snake_case_identifier",
    "camelCaseIdentifier",
    "CONSTANT_NAME",
    "a-b-c-d-e",
    "()[]{}",
    "!!!???",
    "one two  three   four    five",
    "A B C D E F G",
    "1 2 3 4 5 6 7 8 9 10",
    "The 1990s and the 2020s.",
    "GDP grew 2.3% year-over-year.",
    "Section 5.2.1(a)(ii)",
    "Ken Mantle, P.Eng.",
    "END",
    "",
]

# 5 fixed prompts scored for logit agreement (kept short: this pipeline
# reprocesses each token from scratch in pure-Rust single-thread, and the
# point is exactness, not throughput).
LOGIT_PROMPTS = [
    "The capital of France is",
    "2 + 2 =",
    "Q3 2026 revenue: $4,502,118. Net income was",
    "M8x1.25 thread, tolerance +0.010/-0.005,",
    "Once upon a time, there was a",
]


def main() -> None:
    hf_dir, out_prefix = sys.argv[1], sys.argv[2]

    tok = AutoTokenizer.from_pretrained(hf_dir)
    token_fixture = {s: tok(s, add_special_tokens=False)["input_ids"] for s in TEST_STRINGS}
    with open(f"{out_prefix}_tokens.json", "w", encoding="utf-8") as f:
        json.dump(token_fixture, f, ensure_ascii=False, indent=1)

    model = AutoModelForCausalLM.from_pretrained(hf_dir, dtype=torch.float32)
    model.eval()

    manifest = {"vocab_size": model.config.vocab_size, "prompts": []}
    with torch.no_grad():
        for i, prompt in enumerate(LOGIT_PROMPTS):
            ids = tok(prompt, add_special_tokens=False)["input_ids"]
            input_ids = torch.tensor([ids], dtype=torch.long)
            out = model(input_ids)
            last = out.logits[0, -1, :].to(torch.float32).contiguous()
            top5 = torch.topk(last, 5)
            with open(f"{out_prefix}_logits_{i}.bin", "wb") as f:
                f.write(struct.pack(f"<{last.numel()}f", *last.tolist()))
            manifest["prompts"].append(
                {
                    "text": prompt,
                    "tokens": ids,
                    "top5_ids": top5.indices.tolist(),
                    "top5_logits": top5.values.tolist(),
                    "logits_file": f"{out_prefix.split('/')[-1]}_logits_{i}.bin",
                }
            )
    with open(f"{out_prefix}_logits.json", "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=1)

    print(f"wrote {out_prefix}_tokens.json, {out_prefix}_logits.json, and {len(LOGIT_PROMPTS)} logits_*.bin files")


if __name__ == "__main__":
    main()
