//! Measures load time, prefill tokens/s and decode tokens/s for a `.ocrl`
//! file — a *reading*, not a projection (`CLAUDE.md` rule 8). Not a fixture:
//! this is a manual tool run against real weights, which are never
//! committed, so its output is transcribed into
//! `docs/measurements/2026-09-24_llm_engine.txt` by hand rather than
//! asserted against here.
//!
//! Usage: `cargo run -p ocrcer-llm --release --features parallel --example
//! bench -- <path.ocrl> [threads]`. `threads` defaults to 1; pass the host's
//! core count for the all-threads reading. Building without `--features
//! parallel` ignores the thread argument (`with_threads` is a no-op then).

use std::time::Instant;

const PREFILL_TOKENS: usize = 64;
const DECODE_TOKENS: usize = 64;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).unwrap_or_else(|| {
        eprintln!("usage: bench <path.ocrl> [threads]");
        std::process::exit(2);
    });
    let threads: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);

    let bytes = std::fs::read(path).expect("read .ocrl file");
    let file_len = bytes.len();

    let t0 = Instant::now();
    let (model, tokenizer) = ocrcer_llm::load(&bytes).expect("load .ocrl");
    let load_time = t0.elapsed();
    let model = model.with_threads(threads);

    // A long fixed paragraph so both the prefill and decode windows have
    // real token ids to run through, not padding. Domain text (financial +
    // CAD, per this project's stated domain) rather than generic prose.
    let text = "Q3 2026 revenue was $4,502,118 against a budget of $4,250,000, \
        a variance of 5.9 percent driven by the CAD-12 assembly line. Part \
        M8x1.25 thread, tolerance +0.010/-0.005 inch, failed incoming \
        inspection on lot 4471 and was quarantined pending disposition. Net \
        income for the quarter was $612,304, up from $588,910 in Q2. The \
        controller's note flags accrued liabilities of $91,220 as the item \
        most likely to require restatement before the annual filing is \
        submitted to the audit committee next month for final review and \
        sign-off ahead of the board meeting scheduled for the third week.";
    let mut tokens = tokenizer.encode(text);
    while tokens.len() < PREFILL_TOKENS + DECODE_TOKENS {
        tokens.extend_from_slice(&tokenizer.encode(text));
    }
    let prefix = &tokens[..PREFILL_TOKENS];
    let candidate = &tokens[PREFILL_TOKENS..PREFILL_TOKENS + DECODE_TOKENS];

    let t1 = Instant::now();
    let (cache, logits) = model.prefill(prefix);
    let prefill_time = t1.elapsed();

    let t2 = Instant::now();
    let _ = model.score_continue(&cache, &logits, prefix.len(), candidate);
    let decode_time = t2.elapsed();

    println!("file: {path}");
    println!("file size: {file_len} bytes");
    println!("threads: {threads}");
    println!("load time: {:.3} s", load_time.as_secs_f64());
    println!(
        "prefill: {} tokens in {:.3} s = {:.2} tok/s",
        PREFILL_TOKENS,
        prefill_time.as_secs_f64(),
        PREFILL_TOKENS as f64 / prefill_time.as_secs_f64()
    );
    println!(
        "decode: {} tokens in {:.3} s = {:.2} tok/s",
        DECODE_TOKENS,
        decode_time.as_secs_f64(),
        DECODE_TOKENS as f64 / decode_time.as_secs_f64()
    );
}
